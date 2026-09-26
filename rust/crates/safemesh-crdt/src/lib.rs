// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

//! Delta-state G-Counter and PN-Counter for constrained mesh links.
//!
//! This crate is the PRODUCT half of the SafeMesh verification-guided loop:
//! a thin `no_std + alloc` implementation of exactly the model proven in
//! `lean/SafeMesh/` (carrier = join-semilattice, merge = join, delta =
//! single-coordinate bump), held to those proofs by a differential
//! conformance test (`tests/conformance.rs`) that replays a Lean-emitted
//! JSON corpus through this code and compares state vectors, sets, read vectors,
//! numeric values, ownership decisions and optional allocation/sequence results
//! with the parsed expectations.
//!
//! What is PROVEN (in Lean, kernel-checked) vs what is TESTED (here): the
//! Lean theorems are universal; this crate is checked against them over a
//! finite corpus. The bridge (Lean compiled eval → JSON → this crate) is the
//! named trusted component. See the repo README for the full TCB statement.

#![no_std]
#![deny(unsafe_code)]

extern crate alloc;
#[cfg(all(feature = "local-writer", target_os = "linux"))]
extern crate std;

#[cfg(all(feature = "local-writer", target_os = "linux"))]
pub mod local;
pub mod ownership;
pub mod wire;

#[path = "crdt/gcounter.rs"]
mod gcounter;
pub use gcounter::{CoordinateError, GCounter, GCounterDelta};

#[path = "crdt/gset.rs"]
mod gset;
pub use gset::GSet;

#[path = "crdt/pncounter.rs"]
mod pncounter;
pub use pncounter::{PnCounter, PnCounterDelta};

#[path = "crdt/orset.rs"]
mod orset;
pub use orset::{OrSet, OrSetDelta};

#[path = "crdt/rga.rs"]
mod rga;
pub use rga::{Rga, RgaDelta};

#[path = "crdt/enable_wins_flag.rs"]
mod enable_wins_flag;
pub use enable_wins_flag::{EnableWinsFlag, EnableWinsFlagDelta};

#[path = "crdt/lww_register.rs"]
mod lww_register;
pub use lww_register::{LwwDot, LwwEntry, LwwRegister, LwwRegisterDelta};

#[path = "crdt/lww_map.rs"]
mod lww_map;
pub use lww_map::{LwwMap, LwwMapDelta};

mod record;
pub use record::{Record, RecordId};

mod version_vector;
pub use version_vector::{
    VersionVector, VersionVectorError, VersionVectorLimitError, VersionVectorLimits,
};

mod event_log;
pub use event_log::{Admission, AppendError, EventLog};

mod replica;
pub use replica::{Replica, ReplicaError};

mod transport;
pub use transport::{
    anti_entropy, InMemoryTransport, TransportAdapter, TransportEnvelope, TransportError,
};

mod codec;
pub use codec::{
    write_bytes, write_len, write_u32, write_u64, write_u8, CollectionLimits, DecodeError,
    DecodeLimits, LegacyFrame, WireCursor, WireDecode, WireEncode, WireError, WireSchema,
};

use codec::*;

#[cfg(test)]
use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec,
    vec::Vec,
};

/// State-based merge contract: `merge` is expected to be a semilattice join.
///
/// For in-house types, this contract is backed by the Lean proof suite and
/// differential conformance corpus. For user-defined types, it is a tested
/// contract enforced by the laws harness, not a proof. Admission is a separate
/// obligation: user-defined CRDTs must implement [`Crdt::validate_record`].
pub trait Mergeable {
    /// Join compatible states, returning an error if their replica domains differ.
    fn merge(&mut self, other: &Self) -> Result<(), MergeError>;
}

/// Delta application surface for CRDT product types.
///
/// Every implementation, including user-defined types, must explicitly provide
/// [`Crdt::validate_record`]. The compiler checks its presence; behavioral tests
/// must check its contract (the merge laws alone do not establish admission).
/// An implementation in another module cannot inherit permissive admission:
///
/// ```compile_fail,E0046
/// mod downstream {
///     use safemesh_crdt::{Crdt, Mergeable};
///     pub struct MissingValidation;
///     impl Mergeable for MissingValidation {
///         fn merge(&mut self, _: &Self) -> Result<(), safemesh_crdt::MergeError> {
///             Ok(())
///         }
///     }
///     impl Crdt for MissingValidation {
///         type Delta = ();
///         fn apply_delta(&mut self, _: ()) {}
///     }
/// }
/// ```
pub trait Crdt: Mergeable {
    type Delta;

    /// Fixed replica domain, or `None` for CRDTs without a fixed arity.
    fn replica_count(&self) -> Option<usize> {
        None
    }

    /// Validate a decoded record before admission or replay into this carrier.
    ///
    /// Refuse records outside the carrier's shape or ownership domain: no
    /// carrier of that same shape may legitimately apply such a record. For
    /// example, a fixed-width counter refuses an out-of-range coordinate or a
    /// coordinate belonging to a different record author.
    ///
    /// Accept legitimate replay even when the current state absorbs the delta
    /// (duplicates, lower tallies, losing writes, or existing tombstones). On a
    /// fresh carrier of the same shape, it must yield the state the record
    /// denotes, including lattice bottom for an empty remove. A lack of visible
    /// change is not evidence of invalidity. Types with a total typed delta
    /// domain may explicitly return `Ok(())`; they have no domain refusal case.
    /// This check must not mutate state. Wire decoding and log-shape validation
    /// are separate checks performed before this method.
    fn validate_record(&self, id: RecordId, delta: &Self::Delta) -> Result<(), WireError>;

    fn apply_delta(&mut self, delta: Self::Delta);
}

/// Error raised by full-state merge (`merge` or `try_merge`).
///
/// The Lean model fixes the replica set (`Fin n`), so a length mismatch is
/// unrepresentable there. At the Rust boundary an untrusted peer can hand us a
/// state vector of a different width; merging it by `zip` would silently drop
/// the trailing coordinates (WS1 silent state loss). The checked path surfaces
/// the mismatch as an error instead of corrupting state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeError {
    /// The two counters cover a different number of replicas.
    ReplicaCountMismatch { own: usize, other: usize },
}

impl core::fmt::Display for MergeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MergeError::ReplicaCountMismatch { own, other } => write!(
                f,
                "replica-count mismatch: own={own}, other={other} (fixed replica set violated)"
            ),
        }
    }
}

impl core::error::Error for MergeError {}

#[cfg(feature = "laws")]
pub mod laws;

#[cfg(test)]
mod frame_tests {
    use super::*;

    #[test]
    fn seqzero_saturated_prefixes() {
        extern crate std;
        let mut cases = 0;
        let mut mismatches = 0;
        for author in [0, 1, u64::MAX] {
            for sequence in [0, 1, 2, 3, u64::MAX - 1, u64::MAX] {
                for prefix in [u64::MAX - 1, u64::MAX] {
                    let mut log = EventLog::new();
                    let r = Record {
                        id: RecordId {
                            replica: author,
                            sequence,
                        },
                        delta: 7u64,
                    };
                    assert_eq!(
                        log.insert_record(&GSet::<u64>::new(), r.clone()),
                        Admission::Accepted
                    );
                    let mut version = VersionVector::new();
                    // Exercise saturated prefixes without allocating 2^64 records.
                    version.set(author, prefix);
                    let expected = if sequence == 0 || sequence > prefix {
                        vec![r]
                    } else {
                        vec![]
                    };
                    mismatches += usize::from(log.since(&version) != expected);
                    cases += 1;
                }
            }
        }
        std::println!("SATURATED CASES {cases} DISAGREE {mismatches}");
        assert_eq!(mismatches, 0);
    }

    use alloc::vec;

    #[test]
    fn legacy_integrity_frames_name_the_unshaped_frame() {
        for records in [vec![], vec![record(1, 5)]] {
            let mut body = Vec::new();
            write_len(&mut body, records.len()).unwrap();
            for record in records {
                write_bytes(&mut body, &record.to_wire_bytes().unwrap()).unwrap();
            }
            let mut bytes = vec![TAG_EVENT_LOG];
            let len = u32::try_from(body.len()).unwrap();
            write_u32(&mut bytes, len);
            write_u32(&mut bytes, !len);
            bytes.extend(body);
            let crc = frame_crc32(&bytes[1..]);
            write_u32(&mut bytes, crc);
            assert_eq!(
                EventLog::<GCounterDelta>::from_wire_bytes(&bytes),
                Err(WireError::LegacyEventLogFrame {
                    found: LegacyFrame::Tag03Unshaped
                })
            );
        }
    }

    #[test]
    fn crc32_matches_standard_check_vector() {
        assert_eq!(frame_crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn binding_collision_fixtures_use_product_encoder() {
        extern crate std;
        fn fixture<D: WireEncode + WireDecode + WireSchema + PartialEq>(
            name: &str,
            deltas: [D; 2],
        ) {
            let shape = D::REQUIRES_ARITY.then_some(2);
            let records: Vec<_> = deltas
                .into_iter()
                .map(|delta| Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 1,
                    },
                    delta,
                })
                .collect();
            let mut bytes = Vec::new();
            EventLog::encode_records(shape, &records, &mut bytes).unwrap();
            assert!(matches!(
                EventLog::<D>::from_wire_bytes(&bytes),
                Err(WireError::RecordCollision)
            ));
            if let Some(directory) = std::env::var_os("SAFEMESH_FRAME_FIXTURE_DIR") {
                std::fs::write(std::path::Path::new(&directory).join(name), bytes).unwrap();
            }
        }
        fixture(
            "gcounter-collision.bin",
            [
                GCounterDelta {
                    replica: 1,
                    tally: 5,
                },
                GCounterDelta {
                    replica: 1,
                    tally: 9,
                },
            ],
        );
        fixture(
            "flag-collision.bin",
            [
                EnableWinsFlagDelta::Enable { token: 5u64 },
                EnableWinsFlagDelta::Enable { token: 9u64 },
            ],
        );
        fixture(
            "register-collision.bin",
            [
                LwwRegisterDelta {
                    timestamp: 1,
                    replica: 1,
                    value: 5u64,
                },
                LwwRegisterDelta {
                    timestamp: 2,
                    replica: 1,
                    value: 9u64,
                },
            ],
        );
        fixture(
            "map-collision.bin",
            [
                LwwMapDelta::Set {
                    key: 1u64,
                    timestamp: 1,
                    replica: 1,
                    value: 5u64,
                },
                LwwMapDelta::Remove {
                    key: 1u64,
                    timestamp: 2,
                    replica: 1,
                },
            ],
        );
    }

    fn record(sequence: u64, tally: u64) -> Record<GCounterDelta> {
        Record {
            id: RecordId {
                replica: 1,
                sequence,
            },
            delta: GCounterDelta { replica: 1, tally },
        }
    }

    // Encode conflicting occurrences as inert input to the decoder's collision gate.
    // The production encoder derives all frame bytes, lengths and checksums.
    fn wire_log(records: &[Record<GCounterDelta>]) -> Vec<u8> {
        let mut bytes = Vec::new();
        EventLog::encode_records(Some(2), records, &mut bytes).unwrap();
        bytes
    }
    #[test]
    fn decoder_surfaces_conflicts_before_deduplication() {
        for records in [
            vec![record(1, 5), record(1, 9)],
            vec![record(1, 9), record(1, 5)],
        ] {
            assert_eq!(
                EventLog::<GCounterDelta>::from_wire_bytes(&wire_log(&records)),
                Err(WireError::RecordCollision)
            );
        }
        let decoded =
            EventLog::<GCounterDelta>::from_wire_bytes(&wire_log(&[record(1, 5), record(1, 5)]))
                .unwrap();
        assert_eq!(decoded.records(), &[record(1, 5)]);
        for records in [
            vec![record(1, 5), record(1, 5), record(1, 5)],
            vec![record(1, 5), record(2, 7), record(1, 5)],
        ] {
            let bytes = wire_log(&records);
            let state = GCounter::new(2);
            let plain = EventLog::<GCounterDelta>::from_wire_bytes(&bytes).unwrap();
            let shaped = EventLog::<GCounterDelta>::from_wire_bytes_for(&bytes, &state).unwrap();
            assert_eq!(plain, shaped);
            let expected = if records[1] == records[0] {
                &records[..1]
            } else {
                &records[..2]
            };
            assert_eq!(plain.records(), expected);
            assert_eq!(plain.version(), shaped.version());
            assert_eq!(plain.to_wire_bytes(), shaped.to_wire_bytes());
            assert_eq!(
                EventLog::<GCounterDelta>::records_from_wire_bytes_for(&bytes, &state).unwrap(),
                records
            );
            let mut trailing = bytes;
            trailing.push(0);
            assert_eq!(
                EventLog::<GCounterDelta>::records_from_wire_bytes_for(&trailing, &state),
                Err(WireError::TrailingBytes)
            );
        }
    }
}

#[cfg(test)]
mod version_vector_tests {
    #[test]
    fn planted_peer_collection_budgets_are_enforced_independently() {
        let entries = BTreeMap::from([(1, 1), (2, u64::MAX), (3, 7)]);
        let zeros = BTreeSet::from([4, 5]);
        for (max_authors, max_zero_replicas, expected) in [
            (
                Some(2),
                None,
                VersionVectorLimitError::AuthorLimitExceeded { max_authors: 2 },
            ),
            (
                None,
                Some(1),
                VersionVectorLimitError::ZeroReplicaLimitExceeded {
                    max_zero_replicas: 1,
                },
            ),
            (
                Some(2),
                Some(1),
                VersionVectorLimitError::AuthorLimitExceeded { max_authors: 2 },
            ),
        ] {
            assert_eq!(
                VersionVector::from_peer_prefixes_with_limits(
                    &entries,
                    &zeros,
                    VersionVectorLimits {
                        max_authors,
                        max_zero_replicas
                    }
                ),
                Err(expected)
            );
        }
        let unbounded = VersionVector::from_peer_prefixes(&entries, &zeros).unwrap();
        assert_eq!(unbounded.entries(), &entries);
        assert_eq!(unbounded.zero_replicas(), &zeros);
        for limits in [
            VersionVectorLimits::default(),
            VersionVectorLimits {
                max_authors: Some(3),
                max_zero_replicas: Some(2),
            },
        ] {
            assert_eq!(
                VersionVector::from_peer_prefixes_with_limits(&entries, &zeros, limits),
                Ok(unbounded.clone())
            );
        }
    }

    #[test]
    fn peer_budgets_allow_empty_and_precede_prefix_validation() {
        let no_entries = BTreeMap::new();
        let no_zeros = BTreeSet::new();
        let limits = VersionVectorLimits {
            max_authors: Some(0),
            max_zero_replicas: Some(0),
        };
        assert_eq!(
            VersionVector::from_peer_prefixes_with_limits(&no_entries, &no_zeros, limits),
            Ok(VersionVector::new())
        );
        let invalid = BTreeMap::from([(7, 0)]);
        assert_eq!(
            VersionVector::from_peer_prefixes_with_limits(&invalid, &no_zeros, limits),
            Err(VersionVectorLimitError::AuthorLimitExceeded { max_authors: 0 })
        );
        let zeros = BTreeSet::from([7]);
        assert_eq!(
            VersionVector::from_peer_prefixes_with_limits(
                &invalid,
                &zeros,
                VersionVectorLimits {
                    max_authors: None,
                    max_zero_replicas: Some(0)
                }
            ),
            Err(VersionVectorLimitError::ZeroReplicaLimitExceeded {
                max_zero_replicas: 0
            })
        );
        assert_eq!(
            VersionVector::from_peer_prefixes_with_limits(
                &invalid,
                &no_zeros,
                VersionVectorLimits::default()
            ),
            Err(VersionVectorLimitError::Prefix(
                VersionVectorError::ZeroPrefix { replica: 7 }
            ))
        );
    }

    #[test]
    fn large_peer_prefixes_are_copied_directly() {
        for (authors, prefix) in [
            (1024, 1000),
            (1024, 1_000_000),
            (1024, u64::MAX),
            (2048, 1_000_000),
            (2048, u64::MAX),
        ] {
            let entries = (0..authors).map(|r| (r, prefix)).collect();
            let zeros = (0..authors).collect();
            let vector = VersionVector::from_peer_prefixes(&entries, &zeros).unwrap();
            assert_eq!(vector.entries(), &entries);
            assert_eq!(vector.zero_replicas(), &zeros);
        }
    }

    use super::*;

    #[test]
    fn large_zero_acknowledgement_set_is_copied_directly() {
        let zeros: BTreeSet<u64> = (0..=1_000_000).collect();
        let vector = VersionVector::from_peer_prefixes(&BTreeMap::new(), &zeros).unwrap();
        assert_eq!(vector.zero_replicas().len(), 1_000_001);
        assert_eq!(vector.zero_replicas(), &zeros);
        assert!(vector.entries().is_empty());
    }

    #[test]
    fn planted_zero_prefix_is_rejected() {
        let entries = BTreeMap::from([(7, 0)]);
        let zeros = BTreeSet::new();
        assert_eq!(
            VersionVector::from_peer_prefixes(&entries, &zeros),
            Err(VersionVectorError::ZeroPrefix { replica: 7 })
        );
    }

    #[test]
    fn all_positive_u64_prefixes_are_accepted() {
        for prefix in [1, 999_999, 1_000_000, 1_000_001, u64::MAX - 1, u64::MAX] {
            let entries = BTreeMap::from([(0, prefix)]);
            let zeros = BTreeSet::from([0]);
            let vector = VersionVector::from_peer_prefixes(&entries, &zeros).unwrap();
            assert_eq!(vector.entries(), &entries);
            assert_eq!(vector.zero_replicas(), &zeros);
            assert!(vector.includes(RecordId {
                replica: 0,
                sequence: prefix
            }));
        }
    }

    #[test]
    fn million_plus_one_record_log_version_is_reconstructible() {
        let mut log = EventLog::new();
        for sequence in 1..=1_000_001 {
            assert_eq!(
                log.append(&mut GSet::<u64>::new(), 0, sequence).unwrap(),
                RecordId {
                    replica: 0,
                    sequence
                }
            );
        }
        assert_eq!(log.records().len(), 1_000_001);
        assert_eq!(log.version().get(0), 1_000_001);
        let rebuilt = VersionVector::from_peer_prefixes(
            log.version().entries(),
            log.version().zero_replicas(),
        )
        .unwrap();
        assert_eq!(&rebuilt, log.version());
        assert!(log.since(&rebuilt).is_empty());
    }

    #[test]
    fn terminal_log_prefix_is_reconstructible_without_overflow() {
        let mut log = EventLog::new();
        // Seed the boundary; materializing all 2^64 - 1 records is infeasible.
        // This exercises the terminal transition, not a full retained history.
        log.version.set(0, u64::MAX - 1);
        let last = RecordId {
            replica: 0,
            sequence: u64::MAX,
        };
        assert_eq!(
            log.insert_record(
                &GSet::<u64>::new(),
                Record {
                    id: last,
                    delta: 7u64
                }
            ),
            Admission::Accepted
        );
        assert_eq!(log.version().get(0), u64::MAX);
        let mut rebuilt = VersionVector::from_peer_prefixes(
            log.version().entries(),
            log.version().zero_replicas(),
        )
        .unwrap();
        assert_eq!(&rebuilt, log.version());
        assert!(rebuilt.includes(last));
        assert!(log.since(&rebuilt).is_empty());
        assert!(!rebuilt.includes(RecordId {
            replica: 0,
            sequence: 0
        }));
        rebuilt.observe(RecordId {
            replica: 0,
            sequence: 0,
        });
        assert_eq!(rebuilt.get(0), u64::MAX);
        assert!(rebuilt.includes(RecordId {
            replica: 0,
            sequence: 0
        }));
        assert_eq!(
            log.append_with(&mut GSet::<u64>::new(), 0, 8, |_, _| {}),
            Err(AppendError::SequenceExhausted)
        );
    }

    #[test]
    fn legitimate_peer_maps_construct_and_match_observe() {
        let cases = [
            (BTreeMap::new(), BTreeSet::new()),
            (BTreeMap::from([(1, 1)]), BTreeSet::new()),
            (BTreeMap::new(), BTreeSet::from([2])),
            (BTreeMap::from([(3, 1_000_000)]), BTreeSet::new()),
            (BTreeMap::from([(4, 2)]), BTreeSet::from([4, 5])),
            (BTreeMap::from([(0, 1)]), BTreeSet::new()),
            (BTreeMap::from([(u64::MAX, 3)]), BTreeSet::new()),
            (
                (1..=32).map(|replica| (replica, replica)).collect(),
                BTreeSet::new(),
            ),
            (BTreeMap::from([(0, 5)]), BTreeSet::from([0])),
            (BTreeMap::new(), BTreeSet::from([0, u64::MAX])),
            (
                BTreeMap::from([(11, 1_000_000), (12, 1_000_000)]),
                BTreeSet::new(),
            ),
            (BTreeMap::from([(8, 1_000_000 - 1)]), BTreeSet::new()),
            (BTreeMap::from([(1, 4), (3, 2)]), BTreeSet::from([2, 9])),
            (BTreeMap::from([(1, 2), (1, 4)]), BTreeSet::new()),
        ];
        let mut constructed = 0;
        for (entries, zeros) in cases {
            let actual = VersionVector::from_peer_prefixes(&entries, &zeros).unwrap();
            let mut expected = VersionVector::new();
            for (&replica, &prefix) in &entries {
                for sequence in 1..=prefix {
                    expected.observe(RecordId { replica, sequence });
                }
            }
            for &replica in &zeros {
                expected.observe(RecordId {
                    replica,
                    sequence: 0,
                });
            }
            assert_eq!(actual, expected);
            assert_eq!(actual.entries(), &entries);
            assert_eq!(actual.zero_replicas(), &zeros);
            let mut local = EventLog::new();
            let replicas: BTreeSet<_> = entries
                .keys()
                .chain(zeros.iter())
                .copied()
                .chain([0, 99, u64::MAX])
                .collect();
            for replica in replicas {
                let prefix = entries.get(&replica).copied().unwrap_or(0);
                let sequences = BTreeSet::from([0, 1, prefix, prefix + 1, u64::MAX]);
                for sequence in sequences {
                    let id = RecordId { replica, sequence };
                    assert_eq!(
                        local.insert_record(
                            &GSet::<u64>::new(),
                            Record {
                                id,
                                delta: sequence
                            }
                        ),
                        Admission::Accepted
                    );
                }
            }
            assert_eq!(local.since(&actual), local.since(&expected));
            // Subsequent contiguous, gap, repeated and zero observations agree too.
            let mut actual_next = actual.clone();
            let mut expected_next = expected.clone();
            for sequence in [0, 1, 3, 2, 2, 0] {
                let id = RecordId {
                    replica: 99,
                    sequence,
                };
                actual_next.observe(id);
                expected_next.observe(id);
                assert_eq!(actual_next, expected_next);
            }
            constructed += 1;
        }
        assert_eq!(constructed, 14);
    }

    #[test]
    fn refused_peer_map_leaves_since_on_previous_version_unchanged() {
        let mut local = EventLog::new();
        let acknowledged = local.append(&mut GSet::<u64>::new(), 7, 10u64).unwrap();
        let missing = local.append(&mut GSet::<u64>::new(), 7, 20u64).unwrap();
        let remote_version =
            VersionVector::from_peer_prefixes(&BTreeMap::from([(7, 1)]), &BTreeSet::new()).unwrap();
        let before = local.since(&remote_version);
        {
            let (prefix, error) = (0, VersionVectorError::ZeroPrefix { replica: 7 });
            assert_eq!(
                VersionVector::from_peer_prefixes(&BTreeMap::from([(7, prefix)]), &BTreeSet::new()),
                Err(error)
            );
            let after = local.since(&remote_version);
            assert_eq!(after, before);
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].id, missing);
            assert!(remote_version.includes(acknowledged));
            assert!(!remote_version.includes(missing));
        }
        assert_eq!(local.since(&VersionVector::new()).len(), 2);
    }

    #[test]
    fn accepted_peer_vector_drives_since_and_anti_entropy() {
        let mut local = EventLog::new();
        for id in [
            RecordId {
                replica: 1,
                sequence: 0,
            },
            RecordId {
                replica: 1,
                sequence: 1,
            },
            RecordId {
                replica: 1,
                sequence: 2,
            },
            RecordId {
                replica: 1,
                sequence: 3,
            },
            RecordId {
                replica: 2,
                sequence: 0,
            },
            RecordId {
                replica: 2,
                sequence: 1,
            },
            RecordId {
                replica: 2,
                sequence: 2,
            },
        ] {
            assert_eq!(
                local.insert_record(&GSet::<u64>::new(), Record { id, delta: 7u64 }),
                Admission::Accepted
            );
        }
        let peer =
            VersionVector::from_peer_prefixes(&BTreeMap::from([(1, 2)]), &BTreeSet::from([2]))
                .unwrap();
        assert_eq!(local.since(&peer).len(), 4);

        let malicious =
            VersionVector::from_peer_prefixes(&BTreeMap::from([(1, u64::MAX)]), &BTreeSet::new())
                .unwrap();
        let mut transport = InMemoryTransport::new();
        transport.subscribe(1);
        transport.subscribe(2);
        anti_entropy(&mut transport, 1, 2, &local, &malicious).unwrap();
        assert_eq!(transport.pending_len(), 1);
        assert_eq!(transport.drain(2)[0].records.len(), 4);
    }
}

#[cfg(test)]
mod wire_helper_tests {
    use super::*;
    #[derive(Debug, PartialEq)]
    struct Stranger {
        replica: u64,
        tally: u64,
    }
    impl WireDecode for Stranger {
        fn decode_wire(cursor: &mut WireCursor<'_>) -> Result<Self, WireError> {
            if cursor.read_u8()? != 0x10 {
                return Err(WireError::InvalidTag);
            }
            Ok(Self {
                replica: cursor.read_u64()?,
                tally: cursor.read_u64()?,
            })
        }
    }

    #[test]
    fn external_length_extremes_are_checked() {
        for len in [0, 1, u32::MAX] {
            let mut bytes = Vec::new();
            write_u32(&mut bytes, len);
            let mut cursor = WireCursor::new(&bytes);
            let expected = usize::try_from(len).map_err(|_| WireError::LengthOverflow);
            assert_eq!(cursor.read_len(), expected);
            assert!(cursor.is_empty());
        }
        if let Ok(too_large) = usize::try_from(u64::from(u32::MAX) + 1) {
            let mut bytes = vec![99];
            assert_eq!(
                write_len(&mut bytes, too_large),
                Err(WireError::LengthOverflow)
            );
            assert_eq!(bytes, vec![99]);
        }
    }

    #[test]
    fn external_tag_and_trailing_bytes_are_checked() {
        let mut cursor = WireCursor::new(&[3, 4]);
        assert_eq!(read_tag(&mut cursor, 2), Err(WireError::InvalidTag));
        assert_eq!(read_tag(&mut cursor, 4), Ok(()));
        assert_eq!(read_tag(&mut cursor, 4), Err(WireError::UnexpectedEof));
        assert!(cursor.is_empty());
        let mut bytes = GCounterDelta {
            replica: 2,
            tally: 7,
        }
        .to_wire_bytes()
        .unwrap();
        bytes.push(0);
        assert_eq!(
            Stranger::from_wire_bytes(&bytes),
            Err(WireError::TrailingBytes)
        );
    }
}
