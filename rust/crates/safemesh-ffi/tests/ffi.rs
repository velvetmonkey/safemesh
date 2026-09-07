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

#[test]
fn checked_coordinate_reports_error_and_retains_silent_path() {
    use safemesh_ffi::{safemesh_gcounter_try_apply_bump, SafeMeshStatus};
    unsafe {
        let counter = safemesh_gcounter_new(2);
        assert_eq!(
            safemesh_gcounter_try_apply_bump(counter, 2, 9),
            SafeMeshStatus::ReplicaOutOfRange
        );
        assert_eq!(safemesh_gcounter_value(counter), 0);
        assert!(safemesh_gcounter_apply_bump(counter, 2, 9));
        assert_eq!(safemesh_gcounter_value(counter), 0);
        assert_eq!(
            safemesh_gcounter_try_apply_bump(counter, 1, 9),
            SafeMeshStatus::Ok
        );
        assert_eq!(safemesh_gcounter_value(counter), 9);
        assert_eq!(
            safemesh_gcounter_try_apply_bump(core::ptr::null_mut(), 2, 9),
            SafeMeshStatus::NullPointer
        );
        safemesh_gcounter_free(counter);
    }
}

#[test]
fn orset_c_abi_matches_core() {
    use safemesh_crdt::OrSet;
    use safemesh_ffi::*;
    unsafe {
        let left = safemesh_orset_new();
        let right = safemesh_orset_new();
        let mut core_left = OrSet::<u64, u64>::new();
        let mut core_right = OrSet::<u64, u64>::new();
        for (element, token) in [(10, 101), (20, 102)] {
            assert_eq!(safemesh_orset_add(left, element, token), SafeMeshStatus::Ok);
            core_left.add(element, token);
        }
        assert_eq!(safemesh_orset_merge(right, left), SafeMeshStatus::Ok);
        core_right.merge(&core_left);
        for (element, token) in [(10, 201), (30, 203), (40, 999), (50, 999)] {
            assert_eq!(
                safemesh_orset_add(right, element, token),
                SafeMeshStatus::Ok
            );
            core_right.add(element, token);
        }
        let mut values = SafeMeshU64s {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        };
        for element in [10, 20] {
            assert_eq!(
                safemesh_orset_observed_tokens(left, element, &mut values),
                SafeMeshStatus::Ok
            );
            for token in core::slice::from_raw_parts(values.ptr, values.len) {
                assert_eq!(
                    safemesh_orset_apply_remove(left, *token),
                    SafeMeshStatus::Ok
                );
            }
            safemesh_u64s_free(values);
            values = SafeMeshU64s {
                ptr: core::ptr::null_mut(),
                len: 0,
                cap: 0,
            };
            core_left.apply_remove(core_left.observed_tokens(&element));
        }
        assert_eq!(safemesh_orset_apply_remove(left, 999), SafeMeshStatus::Ok);
        core_left.apply_remove([999]);
        assert_eq!(safemesh_orset_merge(left, right), SafeMeshStatus::Ok);
        assert_eq!(safemesh_orset_merge(left, left), SafeMeshStatus::Ok);
        core_left.merge(&core_right);
        assert_eq!(
            safemesh_orset_elements(left, &mut values),
            SafeMeshStatus::Ok
        );
        let actual = core::slice::from_raw_parts(values.ptr, values.len);
        let expected: Vec<_> = core_left.elements().into_iter().collect();
        println!("FFI OR-Set={actual:?} Rust core={expected:?}");
        assert_eq!(actual, expected);
        assert_eq!(actual, [10, 30]);
        safemesh_u64s_free(values);
        assert_eq!(
            safemesh_orset_elements(left, core::ptr::null_mut()),
            SafeMeshStatus::NullPointer
        );
        safemesh_orset_free(left);
        safemesh_orset_free(right);
    }
}
