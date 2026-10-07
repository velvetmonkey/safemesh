// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

//! The record-ID collision alarm and its wire report.
//!
//! Two replicas can each admit one [`RecordId`] with a different payload.
//! Their version vectors are then equal, so `since` offers neither payload and
//! ordinary anti-entropy never repairs or reveals the fork. When a peer does
//! offer the other payload, admission refuses it as
//! [`Admission::Collision`](crate::Admission::Collision) and also raises the
//! alarm in the receiving log ([`EventLog::collisions`]). The receiver then
//! sends [`EventLog::collision_report_bytes`] back to the peer, whose
//! [`EventLog::merge_collision_report_bytes`] raises the same alarm on its side.
//!
//! The alarm never admits, applies or replaces a payload; it only names the
//! fork. It is not part of the encoded log or log equality. A durable
//! replica stores the alarm separately and reloads it on restart; a restored
//! plain log drops it. A report payload is decoded under the log's schema but
//! is not validated against a carrier, because it is never applied.

use crate::{
    codec::{frame_crc32, read_checked_body, read_tag},
    write_bytes, write_len, write_u32, write_u64, write_u8, CollectionLimits, DecodeError,
    DecodeLimits, EventLog, RecordId, WireCursor, WireDecode, WireEncode, WireError, WireSchema,
};
use alloc::{borrow::Cow, vec::Vec};

/// First byte of a [`CollisionReport`] frame. No earlier decoder accepts it:
/// every record, log, version and state decoder returns
/// [`WireError::InvalidTag`] for it before reading further.
pub(super) const TAG_COLLISION_REPORT: u8 = 0x05;

/// One record ID held with two different payloads.
///
/// In [`EventLog::collisions`], `local` is the payload this log admitted and
/// `remote` the payload a peer holds. In a [`CollisionReport`] the perspective
/// is the reporter's: `local` is the reporter's payload and `remote` is the
/// payload it was offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordCollision<D> {
    pub id: RecordId,
    pub local: D,
    pub remote: D,
}

/// The wire message that carries collision alarms to a peer.
///
/// Frame: tag `0x05`, little-endian `u32` body length, its complement, the
/// body, and a CRC-32 of both length fields and the body, as for an EventLog
/// frame. The body is the length-prefixed schema
/// `safemesh/collision-report/v1/<payload schema>`, a `u32` entry count, then
/// for each entry the author and sequence as `u64` and the reporter's payload
/// and the offered payload, each length-prefixed in its own wire encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollisionReport<D> {
    pub collisions: Vec<RecordCollision<D>>,
}

/// What merging one report entry did to the receiving log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollisionVerdict {
    /// This log holds the ID, the entry names a payload that differs from the
    /// one held, and the alarm for the ID is now raised here.
    Recorded,
    /// The alarm for the ID was already raised here. Nothing changed.
    Known,
    /// This log does not hold the ID. Nothing changed.
    Unheld,
    /// Both payloads the entry names equal the one held: no fork at this ID
    /// here. Nothing changed.
    Agrees,
}

impl<D: WireSchema> WireSchema for CollisionReport<D> {
    fn wire_schema() -> Cow<'static, [u8]> {
        let mut schema = b"safemesh/collision-report/v1/".to_vec();
        schema.extend_from_slice(D::wire_schema().as_ref());
        Cow::Owned(schema)
    }
}

impl<D: WireEncode + WireSchema> WireEncode for CollisionReport<D> {
    fn encode_wire(&self, out: &mut Vec<u8>) -> Result<(), WireError> {
        let mut body = Vec::new();
        write_bytes(&mut body, Self::wire_schema().as_ref())?;
        write_len(&mut body, self.collisions.len())?;
        for entry in &self.collisions {
            write_u64(&mut body, entry.id.replica);
            write_u64(&mut body, entry.id.sequence);
            write_bytes(&mut body, &entry.local.to_wire_bytes()?)?;
            write_bytes(&mut body, &entry.remote.to_wire_bytes()?)?;
        }
        let len = u32::try_from(body.len()).map_err(|_| WireError::LengthOverflow)?;
        write_u8(out, TAG_COLLISION_REPORT);
        let start = out.len();
        write_u32(out, len);
        write_u32(out, !len);
        out.extend_from_slice(&body);
        let checksum = frame_crc32(&out[start..]);
        write_u32(out, checksum);
        Ok(())
    }
}

impl<D: WireDecode + WireSchema> CollisionReport<D> {
    /// Decode a report with an entry budget and nested payload budgets.
    ///
    /// Checks the tag, length pair and CRC before reading the body, then the
    /// schema, and stops before entry `max_records + 1`. Any failure returns
    /// no partial report.
    pub fn from_wire_bytes_with_limits(
        bytes: &[u8],
        limits: DecodeLimits,
    ) -> Result<Self, DecodeError> {
        let mut cursor = WireCursor::new(bytes);
        let report = Self::decode_limited(
            &mut cursor,
            CollectionLimits {
                max_elements: limits
                    .max_collection_elements
                    .or(CollectionLimits::WIRE_DEFAULT.max_elements),
            },
            limits.max_records,
            limits.max_records,
        )?;
        if !cursor.is_empty() {
            return Err(WireError::TrailingBytes.into());
        }
        Ok(report)
    }

    fn decode_limited(
        cursor: &mut WireCursor<'_>,
        collections: CollectionLimits,
        max_entries: Option<usize>,
        nested_max_records: Option<usize>,
    ) -> Result<Self, DecodeError> {
        read_tag(cursor, TAG_COLLISION_REPORT)?;
        let mut body = WireCursor::new(read_checked_body(cursor)?);
        let schema_len = body.read_len()?;
        if body.read_exact(schema_len)? != Self::wire_schema().as_ref() {
            return Err(WireError::DeltaTypeMismatch.into());
        }
        let mut collisions = Vec::new();
        for index in 0..body.read_len()? {
            if let Some(max_records) = max_entries {
                if index >= max_records {
                    return Err(DecodeError::RecordLimitExceeded { max_records });
                }
            }
            let id = RecordId {
                replica: body.read_u64()?,
                sequence: body.read_u64()?,
            };
            let local = decode_payload(&mut body, collections, nested_max_records)?;
            let remote = decode_payload(&mut body, collections, nested_max_records)?;
            collisions.push(RecordCollision { id, local, remote });
        }
        if !body.is_empty() {
            return Err(WireError::TrailingBytes.into());
        }
        Ok(Self { collisions })
    }
}

fn decode_payload<D: WireDecode>(
    body: &mut WireCursor<'_>,
    collections: CollectionLimits,
    max_records: Option<usize>,
) -> Result<D, WireError> {
    let len = body.read_len()?;
    let mut cursor = WireCursor::new(body.read_exact(len)?);
    let payload = D::decode_wire_with_limits(&mut cursor, collections, max_records)?;
    if !cursor.is_empty() {
        return Err(WireError::TrailingBytes);
    }
    Ok(payload)
}

impl<D: WireDecode + WireSchema> WireDecode for CollisionReport<D> {
    fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
        Self::decode_wire_with_collection_limits(cursor, CollectionLimits::WIRE_DEFAULT)
    }

    fn decode_wire_with_collection_limits(
        cursor: &mut WireCursor<'_>,
        limits: CollectionLimits,
    ) -> Result<Self, WireError> {
        Self::decode_limited(
            cursor,
            limits,
            DecodeLimits::default().max_records,
            DecodeLimits::default().max_records,
        )
        .map_err(WireError::from)
    }

    fn decode_wire_with_limits(
        cursor: &mut WireCursor<'_>,
        collections: CollectionLimits,
        max_records: Option<usize>,
    ) -> Result<Self, WireError> {
        Self::decode_limited(cursor, collections, max_records, max_records).map_err(|error| {
            match error {
                DecodeError::Wire(error) => error,
                DecodeError::RecordLimitExceeded { max_records } => {
                    WireError::NestedRecordLimitExceeded { max_records }
                }
            }
        })
    }
}

impl<D> EventLog<D> {
    /// Every held ID whose alarm is raised, in ID order, with the payload held
    /// here and the first differing payload a peer offered or reported.
    /// Empty when no fork has been seen. Durable replicas store it separately.
    pub fn collisions(&self) -> Vec<RecordCollision<D>>
    where
        D: Clone,
    {
        self.collisions
            .iter()
            .map(|(&id, remote)| RecordCollision {
                id,
                local: self.records[self.seen[&id]].delta.clone(),
                remote: remote.clone(),
            })
            .collect()
    }

    /// The report a peer needs to raise the same alarms: every entry of
    /// [`Self::collisions`], from this log's perspective.
    pub fn collision_report(&self) -> CollisionReport<D>
    where
        D: Clone,
    {
        CollisionReport {
            collisions: self.collisions(),
        }
    }

    /// Merge a peer's report. Each entry is judged against the payload held
    /// here and returns its verdict in input order. Only
    /// [`CollisionVerdict::Recorded`] changes anything, and it changes only the
    /// alarm: never the records, version or a carrier.
    pub fn merge_collision_report(
        &mut self,
        report: CollisionReport<D>,
    ) -> Vec<(RecordId, CollisionVerdict)>
    where
        D: PartialEq,
    {
        report
            .collisions
            .into_iter()
            .map(|entry| (entry.id, self.merge_collision_entry(entry)))
            .collect()
    }

    fn merge_collision_entry(&mut self, entry: RecordCollision<D>) -> CollisionVerdict
    where
        D: PartialEq,
    {
        let Some(&index) = self.seen.get(&entry.id) else {
            return CollisionVerdict::Unheld;
        };
        let held = &self.records[index].delta;
        // The reporter's own payload is the usual witness; the offered payload
        // is normally this log's own.
        let witness = if entry.local != *held {
            entry.local
        } else if entry.remote != *held {
            entry.remote
        } else {
            return CollisionVerdict::Agrees;
        };
        if self.collisions.contains_key(&entry.id) {
            return CollisionVerdict::Known;
        }
        self.collisions.insert(entry.id, witness);
        CollisionVerdict::Recorded
    }
}

impl<D: Clone + WireEncode + WireSchema> EventLog<D> {
    /// Encode [`Self::collision_report`] for the peer, or `None` when no alarm
    /// is raised here. Send it back after merging a peer's batch.
    pub fn collision_report_bytes(&self) -> Result<Option<Vec<u8>>, WireError> {
        if self.collisions.is_empty() {
            return Ok(None);
        }
        self.collision_report().to_wire_bytes().map(Some)
    }
}

impl<D: PartialEq + WireDecode + WireSchema> EventLog<D> {
    /// Decode a peer's report under `limits`, then merge it. A decode failure
    /// refuses the whole message and changes nothing.
    pub fn merge_collision_report_bytes(
        &mut self,
        bytes: &[u8],
        limits: DecodeLimits,
    ) -> Result<Vec<(RecordId, CollisionVerdict)>, DecodeError> {
        let report = CollisionReport::from_wire_bytes_with_limits(bytes, limits)?;
        Ok(self.merge_collision_report(report))
    }
}
