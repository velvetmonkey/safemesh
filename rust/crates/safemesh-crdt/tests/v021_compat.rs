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
        assert_eq!(replica.state().state(), &[5, 0], "counter tally");
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
        assert_eq!(replica.log().records().len(), 1, "set record count");
        assert_eq!(
            &replica.allocation_bytes()[16..24],
            &1u64.to_le_bytes(),
            "set allocation sequence"
        );
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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

#[test]
fn release_files_match_readme_sha256() {
    use std::collections::{BTreeMap, BTreeSet};

    let dir = fixtures();
    let readme = fs::read_to_string(dir.join("README.md")).unwrap();
    let table = readme
        .split_once("## Values and SHA-256\n")
        .expect("missing v0.2.1 SHA-256 table")
        .1
        .split("\n## ")
        .next()
        .unwrap();
    let mut expected = BTreeMap::new();
    for line in table.lines().filter(|line| line.starts_with('|')) {
        if line == "| File | Documented value | SHA-256 |" || line == "| --- | --- | --- |" {
            continue;
        }
        let cells: Vec<_> = line.split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 5, "malformed v0.2.1 SHA-256 row: {line}");
        let name = cells[1]
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
            .expect("fixture name must use backticks");
        assert!(
            name.ends_with(".bin")
                && name[..name.len() - 4]
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "invalid fixture name: {name}"
        );
        let digest = cells[3]
            .strip_prefix('`')
            .and_then(|value| value.strip_suffix('`'))
            .expect("fixture SHA-256 must use backticks");
        assert!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "invalid SHA-256 for {name}: {digest}"
        );
        assert!(
            expected
                .insert(name.to_string(), digest.to_string())
                .is_none(),
            "duplicate v0.2.1 fixture row: {name}"
        );
    }
    assert_eq!(expected.len(), 33, "v0.2.1 SHA-256 table row count");
    let found: BTreeSet<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bin"))
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let listed: BTreeSet<_> = expected.keys().cloned().collect();
    assert_eq!(
        found, listed,
        "v0.2.1 .bin file names differ from the README table"
    );
    for (name, digest) in expected {
        let actual = hex(&sha256(&fs::read(dir.join(&name)).unwrap()));
        assert_eq!(actual, digest, "{name}: expected {digest}, actual {actual}");
    }
}
