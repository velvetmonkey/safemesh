#[cfg(all(feature = "local-writer", target_os = "linux"))]
mod store {
    use safemesh_crdt::*;
    use safemesh_crdt::{
        local::{CommittedTransaction, DurableReplica, LocalError},
        ownership::WriterConfig,
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::{
        fs,
        path::{Path, PathBuf},
    };
    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.1.0")
    }
    fn config() -> WriterConfig {
        WriterConfig {
            writers: 2,
            writer: 0,
        }
    }
    fn scratch(label: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "smq3-{}-{}-{label}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        root
    }
    fn counter_path(root: &Path) -> PathBuf {
        fs::read_dir(root.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".safemesh-")
                    && p.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .ends_with("-writer-0.counter")
            })
            .unwrap()
    }
    fn save(out: &Path) {
        let root = scratch("generate-store").join("store");
        let mut replica = DurableReplica::counter(&root, config()).unwrap();
        replica.bump(replica.ticket(), 5).unwrap();
        let conflict = Record {
            id: RecordId {
                replica: 0,
                sequence: 1,
            },
            delta: GCounterDelta {
                replica: 0,
                tally: 7,
            },
        };
        assert_eq!(
            replica.receive(replica.ticket(), conflict).unwrap(),
            Admission::Collision
        );
        let transaction = [
            replica.allocation_bytes(),
            replica.log().to_wire_bytes().unwrap(),
        ]
        .concat();
        drop(replica);
        fs::write(out.join("transaction.bin"), transaction).unwrap();
        for (name, source) in [
            ("journal.bin", root.join("writer-0.journal")),
            ("fence.bin", root.join("writer-0.fence")),
            ("rollback.bin", root.join("writer-0.rollback")),
            ("counter.bin", counter_path(&root)),
            ("alarms.bin", root.join("writer-0.alarms")),
        ] {
            fs::copy(source, out.join(name)).unwrap();
        }
    }
    fn installed(kind: &str) -> PathBuf {
        let root = scratch(kind).join("store");
        let replica = DurableReplica::counter(&root, config()).unwrap();
        drop(replica);
        let sidecar = counter_path(&root);
        fs::write(sidecar, fs::read(fixtures().join("counter.bin")).unwrap()).unwrap();
        for (name, target) in [
            ("fence.bin", root.join("writer-0.fence")),
            ("rollback.bin", root.join("writer-0.rollback")),
            ("alarms.bin", root.join("writer-0.alarms")),
        ] {
            fs::copy(fixtures().join(name), target).unwrap();
        }
        if kind == "transaction" {
            fs::remove_file(root.join("writer-0.journal")).unwrap();
            fs::copy(
                fixtures().join("transaction.bin"),
                root.join("writer-0.transaction"),
            )
            .unwrap();
        } else {
            fs::copy(
                fixtures().join("journal.bin"),
                root.join("writer-0.journal"),
            )
            .unwrap();
        }
        root
    }
    fn check(root: &Path) {
        let saved = CommittedTransaction::read(root, config()).unwrap_or_else(|error|
            panic!("v0.1.0 stored bytes failed to open: {error:?}; ship a checked migration, never refresh release fixtures"));
        assert_eq!(saved.last_sequence, 1);
        let log = EventLog::<GCounterDelta>::from_wire_bytes(&saved.log_bytes).unwrap_or_else(|error|
            panic!("v0.1.0 stored history failed to decode: {error:?}; ship a checked migration, never refresh release fixtures"));
        assert_eq!(log.records().len(), 1);
        let replica = DurableReplica::restart_counter(root, config()).unwrap_or_else(|error|
            panic!("v0.1.0 store failed to restart: {error:?}; ship a checked migration, never refresh release fixtures"));
        assert_eq!(replica.state().state(), &[5, 0]);
        assert_eq!(replica.log().collisions().len(), 1);
    }
    fn versioned(magic: &[u8; 8], version: u8, legacy: &[u8]) -> Vec<u8> {
        [magic.as_slice(), &[version], legacy].concat()
    }

    #[test]
    fn version_one_fence_and_transaction_restart() {
        let root = installed("transaction");
        for (name, magic) in [("fence", b"SMFENCE1"), ("transaction", b"SMTRANS1")] {
            let path = root.join(format!("writer-0.{name}"));
            let bytes = fs::read(&path).unwrap();
            fs::write(path, versioned(magic, 1, &bytes)).unwrap();
        }
        check(&root);
        let fence = fs::read(root.join("writer-0.fence")).unwrap();
        assert_eq!(fence.len(), 33);
        assert_eq!(u64::from_le_bytes(fence[25..33].try_into().unwrap()), 2);
    }

    fn assert_future(root: &Path, file: &'static str) {
        let error = DurableReplica::restart_counter(root, config())
            .err()
            .unwrap();
        assert!(
            matches!(error, LocalError::UnsupportedFormat { file: found, version: 2 } if found == file),
            "{file}: {error:?}"
        );
        assert_eq!(error.to_string(), format!("unsupported {file} version 2; highest version this build reads is 1; use a newer SafeMesh"));
    }

    #[test]
    fn future_fence_version_is_named() {
        let root = installed("journal");
        let path = root.join("writer-0.fence");
        fs::write(&path, versioned(b"SMFENCE1", 2, &fs::read(&path).unwrap())).unwrap();
        assert_future(&root, "fence");
    }

    #[test]
    fn future_transaction_version_is_named() {
        let root = installed("transaction");
        let path = root.join("writer-0.transaction");
        fs::write(&path, versioned(b"SMTRANS1", 2, &fs::read(&path).unwrap())).unwrap();
        assert_future(&root, "transaction");
    }

    #[test]
    fn future_journal_version_is_named() {
        let root = installed("journal");
        let path = root.join("writer-0.journal");
        let mut bytes = fs::read(&path).unwrap();
        bytes[7] = b'2';
        fs::write(path, bytes).unwrap();
        assert_future(&root, "journal");
    }

    #[test]
    fn future_rollback_marker_version_is_named() {
        let root = installed("journal");
        let path = root.join("writer-0.rollback");
        let mut bytes = fs::read(&path).unwrap();
        bytes[7] = b'2';
        fs::write(path, bytes).unwrap();
        assert_future(&root, "rollback marker");
    }

    #[test]
    fn future_rollback_counter_version_is_named() {
        let root = installed("journal");
        let path = counter_path(&root);
        let mut bytes = fs::read(&path).unwrap();
        bytes[7] = b'2';
        fs::write(path, bytes).unwrap();
        assert_future(&root, "rollback counter");
    }

    #[test]
    fn damaged_magic_with_high_digit_needs_recovery() {
        for file in ["journal", "rollback marker", "rollback counter"] {
            let root = installed("journal");
            let path = match file {
                "journal" => root.join("writer-0.journal"),
                "rollback marker" => root.join("writer-0.rollback"),
                "rollback counter" => counter_path(&root),
                _ => unreachable!(),
            };
            let mut bytes = fs::read(&path).unwrap();
            bytes[0] ^= 0xff;
            bytes[7] = b'2';
            fs::write(&path, bytes).unwrap();
            let error = DurableReplica::restart_counter(&root, config())
                .err()
                .unwrap();
            assert!(
                matches!(error, LocalError::RecoveryRequired),
                "{file}: {error:?}"
            );
        }
    }

    #[test]
    fn legacy_transaction_magic_writers_word_stays_legacy() {
        let root = scratch("magic-writers").join("store");
        fs::create_dir(&root).unwrap();
        let mut bytes = fs::read(fixtures().join("transaction.bin")).unwrap();
        let writers = u64::from_le_bytes(*b"SMTRANS1");
        bytes[..8].copy_from_slice(&writers.to_le_bytes());
        bytes[8..16].copy_from_slice(&1_u64.to_le_bytes());
        fs::write(root.join("writer-1.transaction"), &bytes).unwrap();
        let saved = CommittedTransaction::read(&root, WriterConfig { writers, writer: 1 }).unwrap();
        assert_eq!(saved.log_bytes, bytes[24..]);
    }

    #[test]
    fn legacy_transaction_tag_02_stays_legacy() {
        let root = scratch("legacy-tag-02").join("store");
        fs::create_dir(&root).unwrap();
        let mut bytes = fs::read(fixtures().join("transaction.bin")).unwrap();
        let writers = u64::from_le_bytes(*b"SMTRANS1");
        bytes[..8].copy_from_slice(&writers.to_le_bytes());
        bytes[8..16].copy_from_slice(&1_u64.to_le_bytes());
        bytes[24] = 0x02;
        fs::write(root.join("writer-1.transaction"), &bytes).unwrap();
        let saved = CommittedTransaction::read(&root, WriterConfig { writers, writer: 1 }).unwrap();
        assert_eq!(saved.log_bytes, bytes[24..]);
    }

    #[test]
    fn fresh_writer_versions_fence_and_legacy_store_commit_versions_transaction() {
        let out = scratch("writer-guard");
        save(&out);
        assert_eq!(
            fs::read(out.join("fence.bin")).unwrap(),
            versioned(
                b"SMFENCE1",
                1,
                &fs::read(fixtures().join("fence.bin")).unwrap()
            ),
            "fence.bin"
        );
        for name in ["transaction.bin", "journal.bin"] {
            assert_eq!(
                fs::read(out.join(name)).unwrap(),
                fs::read(fixtures().join(name)).unwrap(),
                "{name}"
            );
        }
        let root = installed("transaction");
        let mut replica = DurableReplica::restart_counter(&root, config()).unwrap();
        replica.bump(replica.ticket(), 1).unwrap();
        let expected_legacy = [
            replica.allocation_bytes(),
            replica.log().to_wire_bytes().unwrap(),
        ]
        .concat();
        drop(replica);
        assert_eq!(
            fs::read(root.join("writer-0.transaction")).unwrap(),
            versioned(b"SMTRANS1", 1, &expected_legacy),
            "writer-0.transaction"
        );
    }
}
