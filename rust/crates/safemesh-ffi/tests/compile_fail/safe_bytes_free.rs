// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

// Planted known-bad case for tests/compile_fail.rs: safe Rust must not be able to free a buffer.
pub fn free_bytes_from_safe_code() {
    let bytes = safemesh_ffi::safemesh_gcounter_delta_to_wire(0, 1);
    safemesh_ffi::safemesh_bytes_free(bytes);
}
