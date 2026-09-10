// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Transcription of SafeMesh.RecordKernel ownership rules. The existing Lean
//! corpus binds these decisions to their specification; no Lean runtime is used.
use crate::{GCounterDelta, OrSetDelta, PnCounterDelta, RecordId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriterConfig {
    pub writers: u64,
    pub writer: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused;

impl WriterConfig {
    pub fn validate(self) -> Result<(), Refused> {
        if self.writer < self.writers {
            Ok(())
        } else {
            Err(Refused)
        }
    }
}

/// Facts supplied by the local fence, not by a remote packet. Public for oracle
/// conformance; constructing these facts does not grant a local writer lease.
#[derive(Clone, Copy, Debug)]
pub struct WriteContext {
    pub config: WriterConfig,
    pub held: bool,
    pub generation: u64,
    pub current_generation: u64,
    pub local: bool,
}

/// An add mints exactly one token at its record ID. Removes reference observed
/// tokens, including other writers' tokens, and do not mint tokens.
#[derive(Clone, Copy, Debug)]
pub enum OwnedPayload {
    Counter(u64),
    Add(u64),
    Remove,
}

pub fn next_sequence(last: u64) -> Option<u64> {
    last.checked_add(1)
}

/// Pure allocation rule, evaluated with checked bounded arithmetic. The record
/// sequence starts at one; configuration must be identical across a replica set.
pub fn allocate_token(writers: u64, author: u64, sequence: u64) -> Option<u64> {
    if author >= writers || sequence == 0 {
        return None;
    }
    sequence.checked_mul(writers)?.checked_add(author)
}

/// The sole Rust transcription of the packet-A refusal predicate. Every local
/// and incoming operation in LocalReplica passes here before M1 admission.
pub fn refuses(ctx: WriteContext, id: RecordId, payload: OwnedPayload) -> bool {
    !(ctx.config.writer < ctx.config.writers
        && ctx.held
        && ctx.generation > 0
        && ctx.generation == ctx.current_generation
        && id.replica < ctx.config.writers
        && id.sequence > 0
        && (!ctx.local || id.replica == ctx.config.writer)
        && match payload {
            OwnedPayload::Counter(coordinate) => coordinate == id.replica,
            OwnedPayload::Add(token) => {
                allocate_token(ctx.config.writers, id.replica, id.sequence) == Some(token)
            }
            OwnedPayload::Remove => true,
        })
}

/// Extract only the ownership-relevant facts from the existing wire payload.
pub trait OwnedDelta {
    fn owned_payload(&self) -> OwnedPayload;
}
impl OwnedDelta for GCounterDelta {
    fn owned_payload(&self) -> OwnedPayload {
        OwnedPayload::Counter(self.replica as u64)
    }
}
impl OwnedDelta for PnCounterDelta {
    fn owned_payload(&self) -> OwnedPayload {
        match self {
            Self::Inc { replica, .. } | Self::Dec { replica, .. } => {
                OwnedPayload::Counter(*replica as u64)
            }
        }
    }
}
impl<T> OwnedDelta for OrSetDelta<T, u64> {
    fn owned_payload(&self) -> OwnedPayload {
        match self {
            Self::Add { token, .. } => OwnedPayload::Add(*token),
            Self::Remove { .. } => OwnedPayload::Remove,
        }
    }
}

/// Checked counter boundary for existing, unfenced replica adapters. This does
/// not grant fencing: it checks the record author's coordinate before admission.
pub fn check_counter_record<D: OwnedDelta>(
    writers: usize,
    id: RecordId,
    delta: &D,
) -> Result<(), Refused> {
    let ctx = WriteContext {
        config: WriterConfig {
            writers: writers as u64,
            writer: 0,
        },
        held: true,
        generation: 1,
        current_generation: 1,
        local: false,
    };
    if refuses(ctx, id, delta.owned_payload()) {
        Err(Refused)
    } else {
        Ok(())
    }
}
