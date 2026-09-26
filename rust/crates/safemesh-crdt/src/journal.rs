// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Append-only durable history for `DurableReplica` (`writer-<id>.journal`).
//!
//! Layout: a fixed header written once by atomic replacement, then one entry per
//! committed record. The header binds the writer configuration and carries a
//! base EventLog frame (empty for a fresh store, the whole history for a
//! migrated one). Each entry is `len`, `!len`, `sequence` + record wire bytes,
//! then CRC-32 over everything before it. Records keep their existing wire
//! encoding; the journal is a storage container, not a transport encoding.
use super::LocalError;
use crate::{codec::frame_crc32, ownership::WriterConfig, WireCursor, WireError};
use alloc::vec::Vec;
use std::path::{Path, PathBuf};

pub(crate) const MAGIC: [u8; 8] = *b"SMJOURN1";
/// Magic, writer count, writer, base allocation sequence, base frame length.
pub(crate) const HEADER: usize = 8 + 8 + 8 + 8 + 4;
/// Length pair before an entry's payload.
const PAIR: usize = 8;
/// Allocation sequence at the start of each payload.
const SEQUENCE: usize = 8;
/// Trailing CRC-32.
const CRC: usize = 4;

pub(crate) fn path(root: &Path, writer: u64) -> PathBuf {
    root.join(alloc::format!("writer-{writer}.journal"))
}

pub(crate) fn header(
    config: WriterConfig,
    base_sequence: u64,
    base_frame: &[u8],
) -> Result<Vec<u8>, LocalError> {
    let len = u32::try_from(base_frame.len())
        .map_err(|_| LocalError::History(WireError::LengthOverflow))?;
    let mut bytes = Vec::with_capacity(HEADER + base_frame.len());
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&config.writers.to_le_bytes());
    bytes.extend_from_slice(&config.writer.to_le_bytes());
    bytes.extend_from_slice(&base_sequence.to_le_bytes());
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(base_frame);
    Ok(bytes)
}

pub(crate) fn entry(sequence: u64, record: &[u8]) -> Result<Vec<u8>, LocalError> {
    let len = u32::try_from(SEQUENCE + record.len())
        .map_err(|_| LocalError::History(WireError::LengthOverflow))?;
    let mut bytes = Vec::with_capacity(PAIR + SEQUENCE + record.len() + CRC);
    bytes.extend_from_slice(&len.to_le_bytes());
    bytes.extend_from_slice(&(!len).to_le_bytes());
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(record);
    let crc = frame_crc32(&bytes);
    bytes.extend_from_slice(&crc.to_le_bytes());
    Ok(bytes)
}

/// Bytes after the last complete entry that restart discards: an entry cut
/// short, a final entry whose checksum fails, or zero fill. Such an entry was
/// never acknowledged, because acknowledgement follows its file sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Tail {
    pub offset: u64,
    pub bytes: u64,
}

pub(crate) struct Parsed<'a> {
    pub base_sequence: u64,
    pub base_frame: &'a [u8],
    /// (allocation sequence after the record, record wire bytes)
    pub entries: Vec<(u64, &'a [u8])>,
    pub valid_len: u64,
    pub tail: Option<Tail>,
}

/// Check the header against `config`, then scan entries. A damaged entry that
/// is followed by other data is refused as `History(IntegrityMismatch)`; only
/// an incomplete final entry is reported as a tail to discard.
pub(crate) fn parse(bytes: &[u8], config: WriterConfig) -> Result<Parsed<'_>, LocalError> {
    let (base_sequence, base_len) = check_header(bytes, config)?;
    let base_frame = bytes
        .get(HEADER..HEADER + base_len)
        .ok_or(LocalError::History(WireError::UnexpectedEof))?;
    let mut offset = HEADER + base_len;
    let mut entries = Vec::new();
    let tail = loop {
        let rest = &bytes[offset..];
        if rest.is_empty() {
            break None;
        }
        match scan_entry(rest) {
            Scan::Entry(len) => {
                let payload = &rest[PAIR..PAIR + len];
                let sequence = u64::from_le_bytes(payload[..SEQUENCE].try_into().unwrap());
                entries.push((sequence, &payload[SEQUENCE..]));
                offset += PAIR + len + CRC;
            }
            Scan::Torn => {
                break Some(Tail {
                    offset: offset as u64,
                    bytes: rest.len() as u64,
                })
            }
            Scan::Damaged(error) => return Err(LocalError::History(error)),
        }
    };
    Ok(Parsed {
        base_sequence,
        base_frame,
        entries,
        valid_len: offset as u64,
        tail,
    })
}

/// Returns (base sequence, base frame length) for a header matching `config`.
pub(crate) fn check_header(bytes: &[u8], config: WriterConfig) -> Result<(u64, usize), LocalError> {
    if bytes.len() < HEADER || bytes[..8] != MAGIC {
        return Err(LocalError::RecoveryRequired);
    }
    let word = |i: usize| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
    if word(8) != config.writers || word(16) != config.writer {
        return Err(LocalError::Configuration);
    }
    let len = u32::from_le_bytes(bytes[32..36].try_into().unwrap());
    Ok((word(24), len as usize))
}

enum Scan {
    /// A complete, checksummed entry with this payload length.
    Entry(usize),
    /// The rest of the file is an unfinished final append.
    Torn,
    Damaged(WireError),
}

fn scan_entry(rest: &[u8]) -> Scan {
    if rest.len() < PAIR || rest.iter().all(|&byte| byte == 0) {
        // A length pair cut short, or space the file gained without data.
        return Scan::Torn;
    }
    let len = u32::from_le_bytes(rest[..4].try_into().unwrap());
    if u32::from_le_bytes(rest[4..8].try_into().unwrap()) != !len {
        return Scan::Damaged(WireError::IntegrityMismatch);
    }
    let len = len as usize;
    if len < SEQUENCE {
        return Scan::Damaged(WireError::UnexpectedEof);
    }
    let Some(end) = (PAIR + len).checked_add(CRC) else {
        return Scan::Damaged(WireError::LengthOverflow);
    };
    if rest.len() < end {
        // The length pair is whole but the entry is not: an interrupted append.
        return Scan::Torn;
    }
    let crc = u32::from_le_bytes(rest[PAIR + len..end].try_into().unwrap());
    if crc != frame_crc32(&rest[..PAIR + len]) {
        return if rest.len() == end {
            Scan::Torn
        } else {
            Scan::Damaged(WireError::IntegrityMismatch)
        };
    }
    Scan::Entry(len)
}

/// Count entries through a fixed buffer, stopping after `stop` of them or at the
/// first entry that is not complete. Payloads are skipped, not read or decoded.
pub(crate) fn count_entries(
    reader: &mut std::io::BufReader<std::fs::File>,
    mut offset: u64,
    file_len: u64,
    stop: usize,
) -> std::io::Result<usize> {
    use std::io::Read;
    let mut count = 0;
    while count < stop {
        let mut pair = [0u8; PAIR];
        if offset + PAIR as u64 > file_len || reader.read_exact(&mut pair).is_err() {
            break;
        }
        let len = u32::from_le_bytes(pair[..4].try_into().unwrap());
        if u32::from_le_bytes(pair[4..].try_into().unwrap()) != !len {
            break;
        }
        let skip = u64::from(len) + CRC as u64;
        offset += PAIR as u64 + skip;
        if offset > file_len {
            break;
        }
        reader.seek_relative(skip as i64)?;
        count += 1;
    }
    Ok(count)
}

/// Rebuild the EventLog frame a whole-history transaction would hold: the base
/// frame's shape and records followed by every journal record, in order.
/// Verifies the base frame's CRC; records are copied as stored.
pub(crate) fn assemble(base_frame: &[u8], records: &[&[u8]]) -> Result<Vec<u8>, WireError> {
    let mut cursor = WireCursor::new(base_frame);
    if cursor.read_u8()? != crate::codec::TAG_EVENT_LOG {
        return Err(WireError::InvalidTag);
    }
    let body_len = cursor.read_u32()?;
    if cursor.read_u32()? != !body_len {
        return Err(WireError::IntegrityMismatch);
    }
    let body = cursor.read_exact(body_len as usize)?;
    let crc = cursor.read_u32()?;
    if crc != frame_crc32(&base_frame[1..9 + body_len as usize]) {
        return Err(WireError::IntegrityMismatch);
    }
    if !cursor.is_empty() {
        return Err(WireError::TrailingBytes);
    }
    // Shape header: marker, schema length and schema, arity kind and arity.
    let mut body_cursor = WireCursor::new(body);
    body_cursor.read_u32()?;
    let schema_len = body_cursor.read_len()?;
    body_cursor.read_exact(schema_len)?;
    let mut shape = 4 + 4 + schema_len + 1;
    if body_cursor.read_u8()? == 1 {
        body_cursor.read_u64()?;
        shape += 8;
    }
    let count = body_cursor.read_u32()?;
    let base_records = &body[shape + 4..];
    let added = u32::try_from(records.len()).map_err(|_| WireError::LengthOverflow)?;
    let mut new_body = Vec::new();
    new_body.extend_from_slice(&body[..shape]);
    crate::codec::write_u32(
        &mut new_body,
        count.checked_add(added).ok_or(WireError::LengthOverflow)?,
    );
    new_body.extend_from_slice(base_records);
    for record in records {
        crate::codec::write_u32(
            &mut new_body,
            u32::try_from(record.len()).map_err(|_| WireError::LengthOverflow)?,
        );
        new_body.extend_from_slice(record);
    }
    let len = u32::try_from(new_body.len()).map_err(|_| WireError::LengthOverflow)?;
    let mut frame = Vec::with_capacity(13 + new_body.len());
    frame.push(crate::codec::TAG_EVENT_LOG);
    crate::codec::write_u32(&mut frame, len);
    crate::codec::write_u32(&mut frame, !len);
    frame.extend_from_slice(&new_body);
    let crc = frame_crc32(&frame[1..]);
    crate::codec::write_u32(&mut frame, crc);
    Ok(frame)
}
