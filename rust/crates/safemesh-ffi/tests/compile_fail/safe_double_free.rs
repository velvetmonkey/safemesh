// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

// Planted known-bad case for tests/compile_fail.rs: the double free from SafeMesh finding F1.
// This file is only ever compiled, never run; it compiling is the defect.
pub fn double_free_from_safe_code() {
    let counter = safemesh_ffi::safemesh_gcounter_new(2);
    safemesh_ffi::safemesh_gcounter_free(counter);
    safemesh_ffi::safemesh_gcounter_free(counter);
}
