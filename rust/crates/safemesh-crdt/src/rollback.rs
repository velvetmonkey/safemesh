// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Independent own-writer high-water for new durable stores.
use super::{persistence, LocalError};
use crate::{codec::frame_crc32, ownership::WriterConfig};
use alloc::{format, vec::Vec};
use std::{
    fs::{self, File},
    io::Read,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

const MARKER_MAGIC: &[u8; 8] = b"SMROLLB1";
const COUNTER_MAGIC: &[u8; 8] = b"SMCOUNT1";

pub(super) struct Counter {
    path: PathBuf,
    identity: [u8; 16],
    config: WriterConfig,
    sequence: u64,
}

pub(super) fn paths(root: &Path, writer: u64) -> Result<(PathBuf, PathBuf), LocalError> {
    let parent = root.parent().ok_or(LocalError::Configuration)?;
    let name = frame_crc32(root.as_os_str().as_bytes());
    Ok((
        root.join(format!("writer-{writer}.rollback")),
        parent.join(format!(".safemesh-{name:08x}-writer-{writer}.counter")),
    ))
}

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>, LocalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(Some(fs::read(path)?)),
        Ok(_) => Err(LocalError::RecoveryRequired),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn identity_bytes(magic: &[u8; 8], config: WriterConfig, identity: &[u8; 16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(40);
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&config.writers.to_le_bytes());
    bytes.extend_from_slice(&config.writer.to_le_bytes());
    bytes.extend_from_slice(identity);
    bytes
}

impl Counter {
    pub(super) fn create(root: &Path, config: WriterConfig) -> Result<Self, LocalError> {
        let (marker, path) = paths(root, config.writer)?;
        if read_regular(&marker)?.is_some() || read_regular(&path)?.is_some() {
            return Err(LocalError::RecoveryRequired);
        }
        let mut identity = [0u8; 16];
        File::open("/dev/urandom")?.read_exact(&mut identity)?;
        let counter = Self {
            path,
            identity,
            config,
            sequence: 0,
        };
        counter.write(0)?;
        persistence::replace(&marker, &identity_bytes(MARKER_MAGIC, config, &identity))?;
        Ok(counter)
    }

    pub(super) fn open(
        root: &Path,
        config: WriterConfig,
        sequence: u64,
    ) -> Result<Option<Self>, LocalError> {
        let (marker, path) = paths(root, config.writer)?;
        let marker = read_regular(&marker)?;
        let bytes = read_regular(&path)?;
        let (marker, bytes) = match (marker, bytes) {
            (Some(marker), Some(bytes)) => (marker, bytes),
            (None, None) => return Ok(None), // A pre-protection store keeps its old open behavior.
            _ => return Err(LocalError::RecoveryRequired),
        };
        if marker.len() != 40
            || marker[..24] != identity_bytes(MARKER_MAGIC, config, &[0; 16])[..24]
        {
            return Err(LocalError::RecoveryRequired);
        }
        let identity: [u8; 16] = marker[24..40].try_into().unwrap();
        if bytes.len() != 52
            || &bytes[..40] != identity_bytes(COUNTER_MAGIC, config, &identity).as_slice()
            || u32::from_le_bytes(bytes[48..52].try_into().unwrap()) != frame_crc32(&bytes[..48])
        {
            return Err(LocalError::RecoveryRequired);
        }
        let high = u64::from_le_bytes(bytes[40..48].try_into().unwrap());
        if high != sequence {
            return Err(LocalError::RecoveryRequired);
        }
        Ok(Some(Self {
            path,
            identity,
            config,
            sequence: high,
        }))
    }

    fn write(&self, sequence: u64) -> Result<(), LocalError> {
        let mut bytes = identity_bytes(COUNTER_MAGIC, self.config, &self.identity);
        bytes.extend_from_slice(&sequence.to_le_bytes());
        bytes.extend_from_slice(&frame_crc32(&bytes).to_le_bytes());
        persistence::replace(&self.path, &bytes)?;
        Ok(())
    }

    pub(super) fn advance(&mut self, sequence: u64) -> Result<(), LocalError> {
        if sequence != self.sequence.checked_add(1).ok_or(LocalError::Exhausted)? {
            return Err(LocalError::RecoveryRequired);
        }
        persistence::checkpoint(19)?;
        self.write(sequence)?;
        persistence::checkpoint(20)?;
        self.sequence = sequence;
        Ok(())
    }
}
