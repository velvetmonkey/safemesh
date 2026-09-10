// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

// Negative control for tests/compile_fail.rs: the same calls inside `unsafe` must compile, so a
// failure in the planted cases is the missing `unsafe` and not a broken harness. Never run.
pub fn calls_with_unsafe() -> u64 {
    let counter = safemesh_ffi::safemesh_gcounter_new(1);
    let bytes = safemesh_ffi::safemesh_gcounter_delta_to_wire(0, 1);
    // SAFETY: fresh live handle and buffer, each used on this thread and released exactly once.
    unsafe {
        safemesh_ffi::safemesh_gcounter_apply_bump(counter, 0, 1);
        let value = safemesh_ffi::safemesh_gcounter_value(counter);
        safemesh_ffi::safemesh_bytes_free(bytes);
        safemesh_ffi::safemesh_gcounter_free(counter);
        value
    }
}
