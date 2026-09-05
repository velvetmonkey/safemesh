// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_ffi::{
    safemesh_bytes_free, safemesh_gcounter_apply_bump, safemesh_gcounter_delta_to_wire,
    safemesh_gcounter_free, safemesh_gcounter_new, safemesh_gcounter_value,
};

#[test]
fn c_abi_gcounter_smoke_test() {
    let counter = safemesh_gcounter_new(3);
    assert!(!counter.is_null());
    assert!(safemesh_gcounter_apply_bump(counter, 1, 5));
    assert!(safemesh_gcounter_apply_bump(counter, 1, 2));
    assert_eq!(safemesh_gcounter_value(counter), 5);
    safemesh_gcounter_free(counter);
}

#[test]
fn c_abi_exports_wire_bytes() {
    let bytes = safemesh_gcounter_delta_to_wire(2, 7);
    assert_eq!(bytes.len, 17);
    assert!(!bytes.ptr.is_null());
    unsafe {
        assert_eq!(*bytes.ptr, 0x10);
    }
    safemesh_bytes_free(bytes);
}
