// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use crate::{EventLog, Record, VersionVector};
use alloc::{collections::BTreeSet, vec::Vec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportEnvelope<D> {
    pub from: u64,
    pub to: u64,
    pub records: Vec<Record<D>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportError {
    NotSubscribed { peer: u64 },
    Disconnected { from: u64, to: u64 },
}

/// Engineered transport coverage contract.
///
/// A transport adapter is responsible for peer subscription, connectivity
/// checks, and moving record batches. It is tested infrastructure, not a
/// proof that any real network delivers packets.
pub trait TransportAdapter<D> {
    fn subscribe(&mut self, peer: u64);
    fn is_subscribed(&self, peer: u64) -> bool;
    fn set_connected(&mut self, a: u64, b: u64, connected: bool);
    fn is_connected(&self, a: u64, b: u64) -> bool;
    fn send(&mut self, from: u64, to: u64, records: Vec<Record<D>>) -> Result<(), TransportError>;
    fn drain(&mut self, peer: u64) -> Vec<TransportEnvelope<D>>;
}

/// Deterministic in-memory adapter for coverage-contract fault campaigns.
///
/// This adapter can drop, duplicate, reorder, partition, and heal links. It is
/// useful for CI and demos; it is not a real radio/network adapter.
/// Packets queued before a partition remain pending until the link heals;
/// draining a peer delivers only packets whose links are currently connected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InMemoryTransport<D> {
    subscribed: BTreeSet<u64>,
    disconnected: BTreeSet<(u64, u64)>,
    queue: Vec<TransportEnvelope<D>>,
    dropped: Vec<TransportEnvelope<D>>,
    drop_next: bool,
    duplicate_next: bool,
}

impl<D> InMemoryTransport<D> {
    pub fn new() -> Self {
        InMemoryTransport {
            subscribed: BTreeSet::new(),
            disconnected: BTreeSet::new(),
            queue: Vec::new(),
            dropped: Vec::new(),
            drop_next: false,
            duplicate_next: false,
        }
    }

    pub fn drop_next_send(&mut self) {
        self.drop_next = true;
    }

    pub fn duplicate_next_send(&mut self) {
        self.duplicate_next = true;
    }

    pub fn reverse_pending_for(&mut self, peer: u64) {
        let mut peer_items = Vec::new();
        let mut retained = Vec::new();
        for envelope in self.queue.drain(..) {
            if envelope.to == peer {
                peer_items.push(envelope);
            } else {
                retained.push(envelope);
            }
        }
        peer_items.reverse();
        retained.extend(peer_items);
        self.queue = retained;
    }

    pub fn pending_len(&self) -> usize {
        self.queue.len()
    }

    pub fn dropped_len(&self) -> usize {
        self.dropped.len()
    }

    fn link(a: u64, b: u64) -> (u64, u64) {
        if a <= b {
            (a, b)
        } else {
            (b, a)
        }
    }
}

impl<D> Default for InMemoryTransport<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D: Clone> TransportAdapter<D> for InMemoryTransport<D> {
    fn subscribe(&mut self, peer: u64) {
        self.subscribed.insert(peer);
    }

    fn is_subscribed(&self, peer: u64) -> bool {
        self.subscribed.contains(&peer)
    }

    fn set_connected(&mut self, a: u64, b: u64, connected: bool) {
        let link = Self::link(a, b);
        if connected {
            self.disconnected.remove(&link);
        } else {
            self.disconnected.insert(link);
        }
    }

    fn is_connected(&self, a: u64, b: u64) -> bool {
        a == b || !self.disconnected.contains(&Self::link(a, b))
    }

    fn send(&mut self, from: u64, to: u64, records: Vec<Record<D>>) -> Result<(), TransportError> {
        if !self.is_subscribed(from) {
            return Err(TransportError::NotSubscribed { peer: from });
        }
        if !self.is_subscribed(to) {
            return Err(TransportError::NotSubscribed { peer: to });
        }
        if !self.is_connected(from, to) {
            return Err(TransportError::Disconnected { from, to });
        }

        let envelope = TransportEnvelope { from, to, records };
        if self.drop_next {
            self.drop_next = false;
            self.dropped.push(envelope);
            return Ok(());
        }

        self.queue.push(envelope.clone());
        if self.duplicate_next {
            self.duplicate_next = false;
            self.queue.push(envelope);
        }
        Ok(())
    }

    fn drain(&mut self, peer: u64) -> Vec<TransportEnvelope<D>> {
        let mut incoming = Vec::new();
        let mut retained = Vec::new();
        for envelope in self.queue.drain(..) {
            let connected = envelope.from == envelope.to
                || !self
                    .disconnected
                    .contains(&Self::link(envelope.from, envelope.to));
            if envelope.to == peer && connected {
                incoming.push(envelope);
            } else {
                retained.push(envelope);
            }
        }
        self.queue = retained;
        incoming
    }
}

pub fn anti_entropy<D, T>(
    transport: &mut T,
    from: u64,
    to: u64,
    local: &EventLog<D>,
    remote_version: &VersionVector,
) -> Result<(), TransportError>
where
    D: Clone,
    T: TransportAdapter<D>,
{
    let records = local.since(remote_version);
    if records.is_empty() {
        Ok(())
    } else {
        transport.send(from, to, records)
    }
}
