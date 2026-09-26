// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Every retained legacy EventLog fixture decodes to its exact outcome: the
//! named legacy-frame error on every loader, and a lossless explicit migration.
//! Provenance: tests/fixtures/legacy-event-log/README.md.
//! LEGACY_FIXTURE_DIR selects a separate corpus copy for negative controls.
use safemesh_crdt::{
    Crdt, DecodeError, DecodeLimits, EnableWinsFlag, EnableWinsFlagDelta, EventLog, GCounter,
    GCounterDelta, LegacyFrame, LwwMap, LwwMapDelta, LwwRegister, LwwRegisterDelta, OrSet,
    OrSetDelta, PnCounter, PnCounterDelta, Record, RecordId, WireDecode, WireEncode, WireError,
    WireSchema,
};
use std::{fmt::Debug, fs, path::PathBuf};

const FRAMES: [(&str, LegacyFrame); 2] = [
    ("tag02", LegacyFrame::Tag02),
    ("tag03-unshaped", LegacyFrame::Tag03Unshaped),
];

#[test]
fn migration_budget_refuses_before_third_record_decode_and_retry_succeeds() {
    let old = include_bytes!("fixtures/legacy-event-log/tag02/gcounter.log");
    let state = GCounter::new(2);
    assert_eq!(u32::from_le_bytes(old[1..5].try_into().unwrap()), 3);
    let limited = DecodeLimits {
        max_records: Some(2),
        ..DecodeLimits::default()
    };
    assert_eq!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for_with_limits(old, &state, limited),
        Err(DecodeError::RecordLimitExceeded { max_records: 2 })
    );
    let mut malformed = old.to_vec();
    let mut offset = 5;
    for _ in 0..2 {
        let length = u32::from_le_bytes(malformed[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4 + length;
    }
    malformed.truncate(offset);
    malformed.extend_from_slice(&1u32.to_le_bytes());
    malformed.push(0xff);
    assert_eq!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for_with_limits(
            &malformed, &state, limited
        ),
        Err(DecodeError::RecordLimitExceeded { max_records: 2 })
    );
    assert!(EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for(&malformed, &state).is_err());
    let migrated = EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for(old, &state).unwrap();
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes_for(&migrated, &state)
            .unwrap()
            .records()
            .len(),
        3
    );
    assert_eq!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for_with_limits(
            &migrated, &state, limited
        ),
        Err(DecodeError::RecordLimitExceeded { max_records: 2 })
    );
    assert_eq!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for_with_limits(
            &migrated,
            &state,
            DecodeLimits {
                max_records: Some(3),
                ..DecodeLimits::default()
            }
        )
        .unwrap(),
        migrated
    );
    let empty = [0x02, 0, 0, 0, 0];
    assert!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for_with_limits(
            &empty,
            &state,
            DecodeLimits {
                max_records: Some(0),
                ..DecodeLimits::default()
            }
        )
        .is_ok()
    );
    assert_eq!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for_with_limits(
            old,
            &state,
            DecodeLimits {
                max_records: Some(0),
                ..DecodeLimits::default()
            }
        ),
        Err(DecodeError::RecordLimitExceeded { max_records: 0 })
    );
    let first_len = u32::from_le_bytes(old[5..9].try_into().unwrap()) as usize;
    let first = &old[5..9 + first_len];
    let mut duplicates = vec![0x02];
    duplicates.extend_from_slice(&3u32.to_le_bytes());
    for _ in 0..3 {
        duplicates.extend_from_slice(first);
    }
    assert_eq!(
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for_with_limits(
            &duplicates,
            &state,
            limited
        ),
        Err(DecodeError::RecordLimitExceeded { max_records: 2 })
    );
    let deduplicated =
        EventLog::<GCounterDelta>::migrate_legacy_wire_bytes_for(&duplicates, &state).unwrap();
    assert_eq!(
        EventLog::<GCounterDelta>::from_wire_bytes_for(&deduplicated, &state)
            .unwrap()
            .records()
            .len(),
        1
    );
}

// SHA-256 of every retained file. Any changed byte, added or missing file fails.
const PINNED: [(&str, &str); 16] = [
    (
        "tag02/enable-wins-flag-u64.log",
        "45702e51671eae142e2bc9f157bea4fb6573b144ac270d2e536521fdc0747bbf",
    ),
    (
        "tag02/gcounter-empty.log",
        "395c2f5598a1643a205154c6f4c46ce36895b28e6c35660a95e5c6fd5ef9aeab",
    ),
    (
        "tag02/gcounter.log",
        "3d11f237ea835e7add03632848f746f1a8c84027eb430698589f4f2e2463daf1",
    ),
    (
        "tag02/lww-map-u64.log",
        "9960425152d7613b4087391449cde914f5b5387d3af6e757fc7082d1b9ec7e2a",
    ),
    (
        "tag02/lww-register-u64.log",
        "11913fd59c8fc52a2419807e275713a7e808b7c9a579ebbba7155c72b4eb442e",
    ),
    (
        "tag02/orset-u64.log",
        "f9f4d1279f74e4693419980d97f1ed367994d72f7869c2220d5aa93abaf8ecc0",
    ),
    (
        "tag02/orset-utf8.log",
        "0974ecabea8364fee091ad5950052ca691fb63e0f99da784700c2328d559d73f",
    ),
    (
        "tag02/pncounter.log",
        "7b2bbd78de12c1e437d490f64d3e6d84f78a327b4e63a57e9494c75fc4888cc9",
    ),
    (
        "tag03-unshaped/enable-wins-flag-u64.log",
        "f55bf2f86f7486917ab08e84f2653ddea19f009ced639a6d172688a5ec71d32d",
    ),
    (
        "tag03-unshaped/gcounter-empty.log",
        "d9029644ef33deb42cd5806949eb78047e9e25e2b56918a23148c6647e31fab3",
    ),
    (
        "tag03-unshaped/gcounter.log",
        "ca3aa98fd8901dcd5302551c2e43e5eb7d80741951559e3f421a549b443c355e",
    ),
    (
        "tag03-unshaped/lww-map-u64.log",
        "be5813fb91e9d25af56f5508da84d1d686de794fbff312fd5a19dd71cc5ffc45",
    ),
    (
        "tag03-unshaped/lww-register-u64.log",
        "49703165110a8d454a285f6c77395d9b91257c71f918a32cf1b8f687a24868c2",
    ),
    (
        "tag03-unshaped/orset-u64.log",
        "d64fd2703a6cbf5302283d0a1c7e313af16c1e8665b92d126e6727e54912f995",
    ),
    (
        "tag03-unshaped/orset-utf8.log",
        "cdd59fa071f0f21475c086194f6b75d6294bd3955ccc12c49e8a92779499d2bf",
    ),
    (
        "tag03-unshaped/pncounter.log",
        "6f6213ca3c158516dce148e9f0408f33dd906c61e31e1d54de33cfacce56013c",
    ),
];

fn fixture_dir() -> PathBuf {
    std::env::var_os("LEGACY_FIXTURE_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-event-log"),
        PathBuf::from,
    )
}

fn rec<D>(replica: u64, sequence: u64, delta: D) -> Record<D> {
    Record {
        id: RecordId { replica, sequence },
        delta,
    }
}

type Outcome = Result<(), String>;

macro_rules! ensure {
    ($cond:expr, $($msg:tt)*) => {
        if !$cond {
            return Err(format!($($msg)*));
        }
    };
}

// Split an old frame into its record byte strings without the product decoder.
fn legacy_records(bytes: &[u8], frame: LegacyFrame) -> Vec<Vec<u8>> {
    let body = match frame {
        LegacyFrame::Tag02 => &bytes[1..],
        LegacyFrame::Tag03Unshaped => &bytes[9..bytes.len() - 4],
    };
    let word = |at: usize| u32::from_le_bytes(body[at..at + 4].try_into().unwrap()) as usize;
    let mut at = 4;
    let mut records = Vec::new();
    for _ in 0..word(0) {
        let len = word(at);
        records.push(body[at + 4..at + 4 + len].to_vec());
        at += 4 + len;
    }
    assert_eq!(at, body.len());
    records
}

/// The exact outcome for one legacy file, as a value so negative controls can
/// assert that a changed input or decoder is detected.
fn verify<C: Crdt>(
    bytes: &[u8],
    frame: LegacyFrame,
    state: &C,
    expected: &[Record<C::Delta>],
) -> Outcome
where
    C::Delta: WireDecode + WireEncode + WireSchema + PartialEq + Clone + Debug,
{
    let named = WireError::LegacyEventLogFrame { found: frame };
    let loaders = [
        (
            "from_wire_bytes",
            EventLog::<C::Delta>::from_wire_bytes(bytes).map(drop),
        ),
        (
            "from_wire_bytes_for",
            EventLog::from_wire_bytes_for(bytes, state).map(drop),
        ),
        (
            "records_from_wire_bytes_for",
            EventLog::records_from_wire_bytes_for(bytes, state).map(drop),
        ),
    ];
    for (loader, outcome) in loaders {
        ensure!(
            outcome == Err(named),
            "{loader}: {outcome:?}, expected Err({named:?})"
        );
    }
    let bounded = EventLog::<C::Delta>::from_wire_bytes_for_with_limits(
        bytes,
        state,
        DecodeLimits::default(),
    );
    ensure!(
        bounded.as_ref().err() == Some(&DecodeError::Wire(named)),
        "bounded loader: {:?}",
        bounded.map(drop)
    );
    let text = named.to_string();
    let found = match frame {
        LegacyFrame::Tag02 => "found tag 0x02 (no CRC, no shape header)",
        LegacyFrame::Tag03Unshaped => "found tag 0x03 without shape header",
    };
    for part in [
        found,
        "expected tag 0x03 with shape header",
        "migrate_legacy_wire_bytes_for",
        "--example migrate_event_log",
        "Migrating a legacy EventLog",
    ] {
        ensure!(text.contains(part), "error text {text:?} lacks {part:?}");
    }

    let migrated = EventLog::migrate_legacy_wire_bytes_for(bytes, state)
        .map_err(|e| format!("migrate: {e:?}"))?;
    let mut canonical = Vec::new();
    EventLog::encode_records(state.replica_count(), expected, &mut canonical).unwrap();
    ensure!(
        migrated == canonical,
        "migrated bytes differ from the canonical shaped frame"
    );
    let log =
        EventLog::from_wire_bytes_for(&migrated, state).map_err(|e| format!("reload: {e:?}"))?;
    ensure!(
        log.records() == expected,
        "records {:?}, expected {expected:?}",
        log.records()
    );
    ensure!(
        log.replica_count() == state.replica_count(),
        "arity {:?}",
        log.replica_count()
    );
    let kept: Vec<_> = log
        .records()
        .iter()
        .map(|r| r.to_wire_bytes().unwrap())
        .collect();
    ensure!(
        kept == legacy_records(bytes, frame),
        "record bytes changed by migration"
    );
    ensure!(
        EventLog::migrate_legacy_wire_bytes_for(&migrated, state).as_ref() == Ok(&migrated),
        "migration is not idempotent"
    );
    Ok(())
}

fn verify_named(name: &str, bytes: &[u8], frame: LegacyFrame) -> Outcome {
    match name {
        "gcounter-empty.log" => verify::<GCounter>(bytes, frame, &GCounter::new(2), &[]),
        "gcounter.log" => verify(
            bytes,
            frame,
            &GCounter::new(2),
            &[
                rec(
                    0,
                    1,
                    GCounterDelta {
                        replica: 0,
                        tally: 10,
                    },
                ),
                rec(
                    1,
                    1,
                    GCounterDelta {
                        replica: 1,
                        tally: 20,
                    },
                ),
                rec(
                    0,
                    2,
                    GCounterDelta {
                        replica: 0,
                        tally: 15,
                    },
                ),
            ],
        ),
        "pncounter.log" => verify(
            bytes,
            frame,
            &PnCounter::new(2),
            &[
                rec(
                    0,
                    1,
                    PnCounterDelta::Inc {
                        replica: 0,
                        tally: 5,
                    },
                ),
                rec(
                    1,
                    1,
                    PnCounterDelta::Dec {
                        replica: 1,
                        tally: 3,
                    },
                ),
                rec(
                    1,
                    2,
                    PnCounterDelta::Inc {
                        replica: 1,
                        tally: 9,
                    },
                ),
            ],
        ),
        "orset-u64.log" => verify(
            bytes,
            frame,
            &OrSet::<u64, u64>::new(),
            &[
                rec(
                    0,
                    1,
                    OrSetDelta::Add {
                        element: 7,
                        token: 100,
                    },
                ),
                rec(
                    1,
                    1,
                    OrSetDelta::Add {
                        element: 8,
                        token: 101,
                    },
                ),
                rec(0, 2, OrSetDelta::Remove { tokens: vec![100] }),
            ],
        ),
        "orset-utf8.log" => verify(
            bytes,
            frame,
            &OrSet::<String, u64>::new(),
            &[
                rec(
                    0,
                    1,
                    OrSetDelta::Add {
                        element: "milk".into(),
                        token: 100,
                    },
                ),
                rec(
                    1,
                    1,
                    OrSetDelta::Add {
                        element: "eggs".into(),
                        token: 101,
                    },
                ),
                rec(1, 2, OrSetDelta::Remove { tokens: vec![100] }),
            ],
        ),
        "lww-register-u64.log" => verify(
            bytes,
            frame,
            &LwwRegister::<u64>::new(),
            &[
                rec(
                    0,
                    1,
                    LwwRegisterDelta {
                        timestamp: 1,
                        replica: 0,
                        value: 42,
                    },
                ),
                rec(
                    1,
                    1,
                    LwwRegisterDelta {
                        timestamp: 2,
                        replica: 1,
                        value: 43,
                    },
                ),
            ],
        ),
        "enable-wins-flag-u64.log" => verify(
            bytes,
            frame,
            &EnableWinsFlag::<u64>::new(),
            &[
                rec(0, 1, EnableWinsFlagDelta::Enable { token: 100 }),
                rec(1, 1, EnableWinsFlagDelta::Disable { tokens: vec![100] }),
            ],
        ),
        "lww-map-u64.log" => verify(
            bytes,
            frame,
            &LwwMap::<u64, u64>::new(),
            &[
                rec(
                    0,
                    1,
                    LwwMapDelta::Set {
                        key: 1,
                        timestamp: 1,
                        replica: 0,
                        value: 10,
                    },
                ),
                rec(
                    1,
                    1,
                    LwwMapDelta::Remove {
                        key: 1,
                        timestamp: 2,
                        replica: 1,
                    },
                ),
            ],
        ),
        other => Err(format!("no expected outcome for {other}")),
    }
}

// The full check for one retained file: pinned hash, then exact outcome.
fn check_file(path: &str, bytes: &[u8]) -> Outcome {
    let (subdir, name) = path.split_once('/').unwrap();
    let frame = FRAMES.iter().find(|(dir, _)| *dir == subdir).unwrap().1;
    let pinned = PINNED
        .iter()
        .find(|(file, _)| *file == path)
        .map(|(_, hash)| *hash);
    let actual = hex(&sha256(bytes));
    ensure!(
        pinned == Some(actual.as_str()),
        "{path}: SHA-256 {actual}, pinned {pinned:?}"
    );
    verify_named(name, bytes, frame).map_err(|error| format!("{path}: {error}"))
}

#[test]
fn every_retained_legacy_fixture_has_its_exact_outcome() {
    let root = fixture_dir();
    let mut present = Vec::new();
    for (subdir, _) in FRAMES {
        for entry in fs::read_dir(root.join(subdir)).unwrap() {
            present.push(format!(
                "{subdir}/{}",
                entry.unwrap().file_name().to_str().unwrap()
            ));
        }
    }
    present.sort();
    let pinned: Vec<_> = PINNED.iter().map(|(path, _)| path.to_string()).collect();
    assert_eq!(
        present, pinned,
        "retained fixture set differs from the pinned set"
    );
    let mut failures = Vec::new();
    for path in &present {
        let outcome = check_file(path, &fs::read(root.join(path)).unwrap());
        println!(
            "{path}: {}",
            outcome
                .as_ref()
                .map_or_else(|e| format!("FAIL {e}"), |()| "ok".into())
        );
        failures.extend(outcome.err());
    }
    assert!(
        failures.is_empty(),
        "{} fixture(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// Permanent negative control: every single-byte change to every fixture is caught.
#[test]
fn any_one_byte_change_to_a_fixture_is_red() {
    let root = fixture_dir();
    for (path, _) in PINNED {
        let bytes = fs::read(root.join(path)).unwrap();
        for at in 0..bytes.len() {
            let mut changed = bytes.clone();
            changed[at] ^= 0x01;
            assert!(
                check_file(path, &changed).is_err(),
                "{path}: byte {at} change not detected"
            );
        }
    }
}

// Legacy classification comes after integrity: a damaged old 0x03 frame is
// corruption, never offered as a migratable legacy frame.
#[test]
fn damaged_unshaped_frame_is_integrity_not_legacy() {
    let bytes = fs::read(fixture_dir().join("tag03-unshaped/gcounter.log")).unwrap();
    for at in 1..bytes.len() {
        let mut damaged = bytes.clone();
        damaged[at] ^= 0x01;
        let state = GCounter::new(2);
        let decoded = EventLog::<GCounterDelta>::from_wire_bytes_for(&damaged, &state);
        let migrated = EventLog::migrate_legacy_wire_bytes_for(&damaged, &state);
        assert_eq!(decoded, Err(WireError::IntegrityMismatch), "byte {at}");
        assert_eq!(migrated, Err(WireError::IntegrityMismatch), "byte {at}");
    }
}

// Migration takes the arity from the caller and refuses a destination the
// records do not fit; it never guesses and never returns partial output.
#[test]
fn migration_refuses_a_destination_the_records_do_not_fit() {
    for (subdir, _) in FRAMES {
        let bytes = fs::read(fixture_dir().join(subdir).join("gcounter.log")).unwrap();
        assert_eq!(
            EventLog::migrate_legacy_wire_bytes_for(&bytes, &GCounter::new(1)),
            Err(WireError::OwnershipViolation)
        );
        assert_eq!(
            EventLog::<OrSetDelta<u64, u64>>::migrate_legacy_wire_bytes_for(&bytes, &OrSet::new()),
            Err(WireError::InvalidTag)
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            EventLog::migrate_legacy_wire_bytes_for(&trailing, &GCounter::new(2)),
            Err(WireError::TrailingBytes)
        );
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// FIPS 180-4 SHA-256, kept local so the pin needs no tool or dependency.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[4 * i..4 * i + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            (hh, g, f, e, d, c, b, a) = (g, f, e, d.wrapping_add(t1), c, b, a, t1.wrapping_add(t2));
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut out = [0u8; 32];
    for (chunk, word) in out.chunks_mut(4).zip(h) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[test]
fn sha256_matches_standard_vectors() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
