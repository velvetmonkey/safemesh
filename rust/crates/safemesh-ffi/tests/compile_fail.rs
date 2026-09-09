// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

//! Compile-fail gate for SafeMesh finding F1.
//!
//! Every pointer operation in `safemesh-ffi` that relies on caller-enforced ownership must be
//! unreachable from safe Rust. Stable rustdoc runs `compile_fail` doctests but does not enforce
//! their error code, so this test invokes `rustc` on planted source files and requires the
//! specific error `E0133` (call to unsafe function requires an `unsafe` block). A negative
//! control checks that the same calls inside `unsafe` compile, so a planted failure is the
//! missing `unsafe` and not a broken harness. Nothing compiled here is ever executed.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PLANTED_SAFE_CASES: [&str; 5] = [
    "safe_gcounter_free.rs",
    "safe_gcounter_apply_bump.rs",
    "safe_gcounter_value.rs",
    "safe_bytes_free.rs",
    "safe_double_free.rs",
];

const NEGATIVE_CONTROL: &str = "unsafe_calls_compile.rs";

fn rustc() -> PathBuf {
    if let Some(rustc) = env::var_os("RUSTC") {
        return PathBuf::from(rustc);
    }
    if let Some(cargo) = env::var_os("CARGO") {
        let sibling = Path::new(&cargo).with_file_name("rustc");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("rustc")
}

/// The directory holding this test binary and every rlib it was linked against.
fn deps_dir() -> PathBuf {
    env::current_exe()
        .expect("current_exe")
        .parent()
        .expect("deps dir")
        .to_path_buf()
}

/// The newest `safemesh_ffi` rlib in the deps directory: the crate under test. Cargo names it
/// `libsafemesh_ffi.rlib` when the cdylib and staticlib share the invocation, and
/// `libsafemesh_ffi-<hash>.rlib` otherwise; accept both.
fn safemesh_ffi_rlib(deps: &Path) -> PathBuf {
    let mut candidates: Vec<PathBuf> = fs::read_dir(deps)
        .expect("read deps dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name == "libsafemesh_ffi.rlib"
                || (name.starts_with("libsafemesh_ffi-") && name.ends_with(".rlib"))
        })
        .collect();
    assert!(
        !candidates.is_empty(),
        "no safemesh_ffi rlib under {}",
        deps.display()
    );
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .expect("rlib mtime")
    });
    candidates.pop().expect("newest rlib")
}

fn compile(case: &str) -> (bool, String) {
    let deps = deps_dir();
    let rlib = safemesh_ffi_rlib(&deps);
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("compile_fail")
        .join(case);
    let out_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("compile_fail");
    fs::create_dir_all(&out_dir).expect("create tmp out dir");
    let output = Command::new(rustc())
        .arg("--edition=2021")
        .arg("--crate-type=lib")
        .arg("--emit=metadata")
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg("--extern")
        .arg(format!("safemesh_ffi={}", rlib.display()))
        .arg("--out-dir")
        .arg(&out_dir)
        .arg(&source)
        .output()
        .expect("spawn rustc");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), stderr)
}

#[test]
fn safe_rust_cannot_call_pointer_operations() {
    for case in PLANTED_SAFE_CASES {
        let (compiled, stderr) = compile(case);
        println!("--- {case}\n{stderr}");
        assert!(
            !compiled,
            "{case} compiled from safe Rust; finding F1 is open again"
        );
        assert!(
            stderr.contains("E0133"),
            "{case} failed to compile for a reason other than E0133:\n{stderr}"
        );
    }
}

#[test]
fn the_same_calls_compile_inside_unsafe() {
    let (compiled, stderr) = compile(NEGATIVE_CONTROL);
    assert!(
        compiled,
        "negative control {NEGATIVE_CONTROL} did not compile; the harness is broken:\n{stderr}"
    );
}
