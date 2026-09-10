// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

// Planted known-bad case for tests/compile_fail.rs: safe Rust must not be able to write through a handle.
pub fn bump_from_safe_code() -> bool {
    let counter = safemesh_ffi::safemesh_gcounter_new(1);
    safemesh_ffi::safemesh_gcounter_apply_bump(counter, 0, 1)
}
