// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_crdt::{GCounter, GCounterDelta, WireEncode};

pub struct SafeMeshGCounter {
    inner: GCounter,
}

#[repr(C)]
pub struct SafeMeshBytes {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

impl SafeMeshBytes {
    fn empty() -> Self {
        SafeMeshBytes {
            ptr: core::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }

    fn from_vec(mut bytes: Vec<u8>) -> Self {
        let out = SafeMeshBytes {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        };
        core::mem::forget(bytes);
        out
    }
}

#[no_mangle]
pub extern "C" fn safemesh_gcounter_new(replicas: usize) -> *mut SafeMeshGCounter {
    Box::into_raw(Box::new(SafeMeshGCounter {
        inner: GCounter::new(replicas),
    }))
}

#[no_mangle]
pub extern "C" fn safemesh_gcounter_free(counter: *mut SafeMeshGCounter) {
    if !counter.is_null() {
        unsafe {
            drop(Box::from_raw(counter));
        }
    }
}

#[no_mangle]
pub extern "C" fn safemesh_gcounter_apply_bump(
    counter: *mut SafeMeshGCounter,
    replica: usize,
    tally: u64,
) -> bool {
    match unsafe { counter.as_mut() } {
        Some(counter) => {
            counter.inner.apply_bump(replica, tally);
            true
        }
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn safemesh_gcounter_value(counter: *const SafeMeshGCounter) -> u64 {
    match unsafe { counter.as_ref() } {
        Some(counter) => counter.inner.value(),
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn safemesh_gcounter_delta_to_wire(replica: usize, tally: u64) -> SafeMeshBytes {
    match (GCounterDelta { replica, tally }).to_wire_bytes() {
        Ok(bytes) => SafeMeshBytes::from_vec(bytes),
        Err(_) => SafeMeshBytes::empty(),
    }
}

#[no_mangle]
pub extern "C" fn safemesh_bytes_free(bytes: SafeMeshBytes) {
    if !bytes.ptr.is_null() {
        unsafe {
            drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
        }
    }
}
