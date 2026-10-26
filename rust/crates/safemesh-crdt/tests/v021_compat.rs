// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0
//! Bytes retained from the v0.2.1 tag. A changed encoder needs a checked migration,
//! never a refresh of these release fixtures.
use safemesh_crdt::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn fixtures() -> PathBuf {
    std::env::var_os("SMV021_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.2.1"))
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
                assert_eq!(<$ty>::from_wire_bytes(&bytes), Ok(expected.clone()), "{name}: v0.2.1 bytes must decode to the documented value; ship a checked migration, never refresh release fixtures");
                assert_eq!(expected.to_wire_bytes().unwrap(), bytes, "{name}: v0.2.1 bytes changed; ship a checked migration, never refresh release fixtures");
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
fn release_wire_bytes_read() {
    assert_eq!(wire_cases(None), 25);
}

#[cfg(all(feature = "local-writer", target_os = "linux"))]
mod store {
    use super::*;
    use safemesh_crdt::{local::DurableReplica, ownership::WriterConfig};
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
            "smv021-{}-{}-{label}",
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
                let name = p.file_name().unwrap().to_string_lossy();
                name.starts_with(".safemesh-") && name.ends_with("-writer-0.counter")
            })
            .unwrap()
    }
    const KINDS: [&str; 2] = ["counter", "set"];
    const PARTS: [&str; 4] = ["journal", "fence", "rollback", "counter"];
    fn save(out: &Path) {
        for kind in KINDS {
            let root = scratch(&format!("generate-{kind}")).join("store");
            if kind == "counter" {
                let mut replica = DurableReplica::counter(&root, config()).unwrap();
                replica.bump(replica.ticket(), 5).unwrap();
            } else {
                let mut replica = DurableReplica::utf8_set(&root, config()).unwrap();
                replica.add(replica.ticket(), "café".into()).unwrap();
            }
            for part in PARTS {
                let source = if part == "counter" {
                    counter_path(&root)
                } else {
                    root.join(format!("writer-0.{part}"))
                };
                fs::copy(source, out.join(format!("{kind}-{part}.bin"))).unwrap();
            }
        }
    }
    fn installed(kind: &str) -> PathBuf {
        let root = scratch(&format!("read-{kind}")).join("store");
        if kind == "counter" {
            drop(DurableReplica::counter(&root, config()).unwrap());
        } else {
            drop(DurableReplica::utf8_set(&root, config()).unwrap());
        }
        for part in PARTS {
            let target = if part == "counter" {
                counter_path(&root)
            } else {
                root.join(format!("writer-0.{part}"))
            };
            fs::copy(fixtures().join(format!("{kind}-{part}.bin")), target).unwrap();
        }
        root
    }
    #[test]
    fn release_file_count() {
        let found = fs::read_dir(fixtures())
            .unwrap()
            .filter_map(|entry| {
                let p = entry.unwrap().path();
                (p.extension().is_some_and(|extension| extension == "bin")).then_some(p)
            })
            .count();
        assert_eq!(found, 33, "committed v0.2.1 file count");
        for kind in KINDS {
            for part in PARTS {
                let file = format!("{kind}-{part}.bin");
                assert!(fixtures().join(&file).is_file(), "missing {file}");
            }
        }
    }
    #[test]
    fn release_stores_restart_to_documented_state() {
        let root = installed("counter");
        let replica = DurableReplica::restart_counter(&root, config())
            .unwrap_or_else(|error| panic!("counter-journal.bin: {error:?}"));
        assert_eq!(replica.state().state(), &[6, 0], "counter tally");
        assert_eq!(
            replica.log().records().len(),
            1,
            "counter allocation sequence"
        );
        drop(replica);
        let root = installed("set");
        let replica = DurableReplica::restart_utf8_set(&root, config())
            .unwrap_or_else(|error| panic!("set-journal.bin: {error:?}"));
        assert!(
            replica.state().contains(&"café".to_string()),
            "set membership"
        );
        assert_eq!(replica.log().records().len(), 1, "set allocation sequence");
    }
    #[test]
    #[ignore = "explicit v0.2.1 tagged generator"]
    fn generate_v021() {
        let out =
            PathBuf::from(std::env::var_os("SMV021_GENERATE").expect("SMV021_GENERATE required"));
        fs::create_dir_all(&out).unwrap();
        if std::env::var_os("SMV021_SEEDED").is_none() {
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
                    "store::generate_v021",
                    "--nocapture"
                ])
                .env("SMV021_SEEDED", "1")
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
