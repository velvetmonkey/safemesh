// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

// Planted known-bad case for tests/compile_fail.rs: safe Rust must not be able to read through a handle.
pub fn value_from_safe_code() -> u64 {
    let counter = safemesh_ffi::safemesh_gcounter_new(1);
    let mut total = 0u64;
    safemesh_ffi::safemesh_gcounter_try_value(counter, &mut total);
    total
}
