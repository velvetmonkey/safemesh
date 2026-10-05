// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Bytes retained from the v0.1.0 tag. A changed encoder needs a checked migration,
//! never a refresh of these release fixtures.
use safemesh_crdt::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn fixtures() -> PathBuf {
    std::env::var_os("SMQ3_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.1.0"))
}

// One declaration drives tagged generation and the independent current-reader
// assertions. In read mode the bytes always come from disk, never a fresh encoder.
fn wire_cases(out: Option<&Path>) -> usize {
    let mut count = 0;
    macro_rules! case {
        ($name:literal, $ty:ty, $value:expr) => {{
            let expected: $ty = $value;
            let name = concat!($name, ".bin");
            if let Some(dir) = out {
                fs::write(dir.join(name), expected.to_wire_bytes().unwrap()).unwrap();
            } else {
                let bytes = fs::read(fixtures().join(name)).unwrap();
                assert_eq!(<$ty>::from_wire_bytes(&bytes), Ok(expected.clone()), "{name}: v0.1.0 bytes must decode to the documented value; ship a checked migration, never refresh release fixtures");
                assert_eq!(expected.to_wire_bytes().unwrap(), bytes, "{name}: v0.1.0 bytes changed; ship a checked migration, never refresh release fixtures");
                let mut unknown = bytes.clone(); unknown[0] = 0xff;
                assert_eq!(<$ty>::from_wire_bytes(&unknown), Err(WireError::InvalidTag), "{name}: unknown tag");
                assert_eq!(<$ty>::from_wire_bytes(&bytes[..1]), Err(WireError::UnexpectedEof), "{name}: truncated header");
                let mut trailing = bytes.clone(); trailing.push(0xa5);
                assert_eq!(<$ty>::from_wire_bytes(&trailing), Err(WireError::TrailingBytes), "{name}: trailing byte");
            }
            count += 1;
        }};
    }
    case!("version-vector", VersionVector, VersionVector::new());
    case!(
        "gcounter-delta",
        GCounterDelta,
        GCounterDelta {
            replica: 0,
            tally: 5
        }
    );
    case!(
        "pn-inc",
        PnCounterDelta,
        PnCounterDelta::Inc {
            replica: 0,
            tally: 5
        }
    );
    case!(
        "pn-dec",
        PnCounterDelta,
        PnCounterDelta::Dec {
            replica: 1,
            tally: 2
        }
    );
    let mut gset = GSet::new();
    gset.insert(7);
    case!("gset-state", GSet<u64>, gset);
    case!("orset-u64-add", OrSetDelta<u64, u64>, OrSetDelta::Add { element: 7, token: 2 });
    case!("orset-u64-remove", OrSetDelta<u64, u64>, OrSetDelta::Remove { tokens: vec![2] });
    case!("orset-utf8-add", OrSetDelta<String, u64>, OrSetDelta::Add { element: "café".into(), token: 2 });
    case!("orset-utf8-remove", OrSetDelta<String, u64>, OrSetDelta::Remove { tokens: vec![2] });
    let mut ou = OrSet::new();
    ou.add(7, 2);
    case!("orset-u64-state", OrSet<u64, u64>, ou);
    let mut os = OrSet::new();
    os.add("café".to_string(), 2);
    case!("orset-utf8-state", OrSet<String, u64>, os);
    case!("rga-insert", RgaDelta<u64, u64>, RgaDelta::Insert { position: 2, value: 7 });
    case!("rga-delete", RgaDelta<u64, u64>, RgaDelta::Delete { position: 2 });
    let mut rga = Rga::new();
    rga.insert(2, 7);
    case!("rga-state", Rga<u64, u64>, rga);
    case!(
        "record",
        Record<GCounterDelta>,
        Record {
            id: RecordId {
                replica: 0,
                sequence: 1
            },
            delta: GCounterDelta {
                replica: 0,
                tally: 5
            }
        }
    );
    case!(
        "event-log",
        EventLog<GCounterDelta>,
        EventLog::for_crdt(&GCounter::new(2))
    );
    case!(
        "register-delta",
        LwwRegisterDelta<u64>,
        LwwRegisterDelta {
            timestamp: 2,
            replica: 0,
            value: 7
        }
    );
    let mut register = LwwRegister::new();
    register.set(2, 0, 7);
    case!("register-state", LwwRegister<u64>, register);
    case!(
        "flag-enable",
        EnableWinsFlagDelta<u64>,
        EnableWinsFlagDelta::Enable { token: 2 }
    );
    case!(
        "flag-disable",
        EnableWinsFlagDelta<u64>,
        EnableWinsFlagDelta::Disable { tokens: vec![2] }
    );
    let mut flag = EnableWinsFlag::new();
    flag.enable(2);
    case!("flag-state", EnableWinsFlag<u64>, flag);
    case!("map-set", LwwMapDelta<u64, u64>, LwwMapDelta::Set { key: 2, timestamp: 3, replica: 0, value: 7 });
    case!("map-remove", LwwMapDelta<u64, u64>, LwwMapDelta::Remove { key: 2, timestamp: 3, replica: 0 });
    let mut map = LwwMap::new();
    map.set(2, 3, 0, 7);
    case!("map-state", LwwMap<u64, u64>, map);
    case!(
        "collision-report",
        CollisionReport<GCounterDelta>,
        CollisionReport { collisions: vec![] }
    );
    count
}

#[test]
fn release_wire_bytes_read_and_refuse() {
    assert_eq!(wire_cases(None), 25);
}

#[cfg(all(feature = "local-writer", target_os = "linux"))]
mod store {
    use super::*;
    use safemesh_crdt::{
        local::{CommittedTransaction, DurableReplica, LocalError},
        ownership::WriterConfig,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

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
    #[test]
    fn release_store_files_restart_to_documented_state() {
        check(&installed("journal"));
        check(&installed("transaction"));
    }
    #[test]
    fn store_refusals_are_named() {
        macro_rules! refusal {
            ($root:expr, $pattern:pat, $label:expr) => {{
                assert!(
                    matches!(
                        DurableReplica::restart_counter(&$root, config()),
                        Err($pattern)
                    ),
                    "{}: expected named refusal",
                    $label
                );
            }};
        }
        // Magic-bearing files have a first-byte discriminator. The unversioned
        // transaction and fence retain configuration checks instead.
        let root = installed("journal");
        let journal = root.join("writer-0.journal");
        let mut bytes = fs::read(&journal).unwrap();
        bytes[0] = 0xff;
        fs::write(&journal, &bytes).unwrap();
        refusal!(root, LocalError::RecoveryRequired, "journal magic");

        let root = installed("journal");
        let journal = root.join("writer-0.journal");
        fs::write(&journal, &fs::read(&journal).unwrap()[..7]).unwrap();
        refusal!(
            root,
            LocalError::RecoveryRequired,
            "journal truncated header"
        );

        let root = installed("journal");
        let journal = root.join("writer-0.journal");
        let mut bytes = fs::read(&journal).unwrap();
        let entry = 36 + u32::from_le_bytes(bytes[32..36].try_into().unwrap()) as usize;
        bytes[entry + 4] ^= 1; // completed entry's length complement
        fs::write(&journal, bytes).unwrap();
        refusal!(
            root,
            LocalError::History(WireError::IntegrityMismatch),
            "malformed complete journal entry"
        );

        for (name, kind, path, expected) in [
            ("fence", "journal", "writer-0.fence", "configuration"),
            ("rollback", "journal", "writer-0.rollback", "recovery"),
            ("counter", "journal", "", "recovery"),
            ("alarms", "journal", "writer-0.alarms", "tag"),
            (
                "transaction",
                "transaction",
                "writer-0.transaction",
                "configuration",
            ),
        ] {
            let file = |root: &Path| {
                if name == "counter" {
                    counter_path(root)
                } else {
                    root.join(path)
                }
            };
            let root = installed(kind);
            let target = file(&root);
            let mut bytes = fs::read(&target).unwrap();
            bytes[0] = 0xff;
            fs::write(&target, bytes).unwrap();
            match expected {
                "configuration" => refusal!(root, LocalError::Configuration, name),
                "tag" => refusal!(root, LocalError::History(WireError::InvalidTag), name),
                _ => refusal!(root, LocalError::RecoveryRequired, name),
            }

            let root = installed(kind);
            let target = file(&root);
            fs::write(&target, &fs::read(&target).unwrap()[..7]).unwrap();
            match name {
                "alarms" => refusal!(
                    root,
                    LocalError::History(WireError::UnexpectedEof),
                    "truncated alarms"
                ),
                _ => refusal!(root, LocalError::RecoveryRequired, name),
            }

            let root = installed(kind);
            let target = file(&root);
            let mut bytes = fs::read(&target).unwrap();
            bytes.push(0xa5);
            fs::write(&target, bytes).unwrap();
            match name {
                "alarms" | "transaction" => {
                    refusal!(root, LocalError::History(WireError::TrailingBytes), name)
                }
                _ => refusal!(root, LocalError::RecoveryRequired, name),
            }
        }
    }
    // The source test is copied verbatim into an isolated checkout at the tag.
    // A preload supplies fixed identity bytes to the tag's ordinary rollback
    // writer; all fixture bytes still pass through the tagged product writer.
    #[test]
    #[ignore = "explicit v0.1.0 generator"]
    fn generate_v010() {
        let out = PathBuf::from(std::env::var_os("SMQ3_GENERATE").expect("SMQ3_GENERATE required"));
        fs::create_dir_all(&out).unwrap();
        if std::env::var_os("SMQ3_SEEDED").is_none() {
            let scratch = scratch("seed-hook");
            let source = scratch.join("seed.c");
            let lib = scratch.join("seed.so");
            fs::write(
                &source,
                r#"#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
ssize_t read(int fd, void *buf, size_t count) {
  char link[80], path[160];
  snprintf(link, sizeof(link), "/proc/self/fd/%d", fd);
  ssize_t n = readlink(link, path, sizeof(path)-1);
  if (n > 0) { path[n] = 0; if (strcmp(path, "/dev/urandom") == 0) {
    for (size_t i = 0; i < count; i++) ((unsigned char*)buf)[i] = (unsigned char)(i+1);
    return (ssize_t)count;
  }}
  return ((ssize_t (*)(int,void*,size_t))dlsym(RTLD_NEXT,"read"))(fd,buf,count);
}
"#,
            )
            .unwrap();
            assert!(std::process::Command::new("cc")
                .args(["-shared", "-fPIC"])
                .arg(&source)
                .args(["-ldl", "-o"])
                .arg(&lib)
                .status()
                .unwrap()
                .success());
            assert!(std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "store::generate_v010",
                    "--nocapture"
                ])
                .env("SMQ3_SEEDED", "1")
                .env("LD_PRELOAD", lib)
                .status()
                .unwrap()
                .success());
            return;
        }
        assert_eq!(wire_cases(Some(&out)), 25);
        save(&out);
    }
}
