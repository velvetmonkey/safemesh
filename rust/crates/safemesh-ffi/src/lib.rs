// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_crdt::{GCounter, GCounterDelta, OrSet, WireEncode};

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

/// Status returned by checked operations. Out-of-range coordinates do not mutate state.
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub enum SafeMeshStatus {
    Ok = 0,
    NullPointer = 1,
    ReplicaOutOfRange = 2,
}

/// # Safety
/// `counter` must be null or a live, exclusively accessible counter handle.
#[no_mangle]
pub unsafe extern "C" fn safemesh_gcounter_try_apply_bump(
    counter: *mut SafeMeshGCounter,
    replica: usize,
    tally: u64,
) -> SafeMeshStatus {
    let Some(counter) = counter.as_mut() else {
        return SafeMeshStatus::NullPointer;
    };
    match counter.inner.try_apply_bump(replica, tally) {
        Ok(()) => SafeMeshStatus::Ok,
        Err(_) => SafeMeshStatus::ReplicaOutOfRange,
    }
}

/// OR-Set with u64 elements and globally scoped, caller-supplied u64 tokens.
pub struct SafeMeshOrSet {
    inner: OrSet<u64, u64>,
}

/// Owned array. Release exactly once with safemesh_u64s_free; do not modify its fields.
#[repr(C)]
pub struct SafeMeshU64s {
    pub ptr: *mut u64,
    pub len: usize,
    pub cap: usize,
}

impl SafeMeshU64s {
    fn from_vec(mut values: Vec<u64>) -> Self {
        let result = Self {
            ptr: values.as_mut_ptr(),
            len: values.len(),
            cap: values.capacity(),
        };
        core::mem::forget(values);
        result
    }
}

#[no_mangle]
pub extern "C" fn safemesh_orset_new() -> *mut SafeMeshOrSet {
    Box::into_raw(Box::new(SafeMeshOrSet {
        inner: OrSet::new(),
    }))
}

/// # Safety
/// `set` must be null or a live handle, freed exactly once with no outstanding uses.
#[no_mangle]
pub unsafe extern "C" fn safemesh_orset_free(set: *mut SafeMeshOrSet) {
    if !set.is_null() {
        drop(Box::from_raw(set));
    }
}

/// # Safety
/// `values` must be an unmodified array returned by this API, released exactly once.
#[no_mangle]
pub unsafe extern "C" fn safemesh_u64s_free(values: SafeMeshU64s) {
    if !values.ptr.is_null() {
        drop(Vec::from_raw_parts(values.ptr, values.len, values.cap));
    }
}

/// # Safety
/// `set` must be null or a live, exclusively accessible handle.
#[no_mangle]
pub unsafe extern "C" fn safemesh_orset_add(
    set: *mut SafeMeshOrSet,
    element: u64,
    token: u64,
) -> SafeMeshStatus {
    let Some(set) = set.as_mut() else {
        return SafeMeshStatus::NullPointer;
    };
    set.inner.add(element, token);
    SafeMeshStatus::Ok
}

/// Tombstone a token globally, even if its add has not arrived. Repeat for multiple tokens.
/// # Safety
/// `set` must be null or a live, exclusively accessible handle.
#[no_mangle]
pub unsafe extern "C" fn safemesh_orset_apply_remove(
    set: *mut SafeMeshOrSet,
    token: u64,
) -> SafeMeshStatus {
    let Some(set) = set.as_mut() else {
        return SafeMeshStatus::NullPointer;
    };
    set.inner.apply_remove([token]);
    SafeMeshStatus::Ok
}

/// Merge all adds and tombstones. Merging a handle with itself is a no-op.
/// # Safety
/// Handles must be null or live; `set` must be exclusively accessible during the call.
#[no_mangle]
pub unsafe extern "C" fn safemesh_orset_merge(
    set: *mut SafeMeshOrSet,
    other: *const SafeMeshOrSet,
) -> SafeMeshStatus {
    if set.is_null() || other.is_null() {
        return SafeMeshStatus::NullPointer;
    }
    if core::ptr::eq(set, other) {
        return SafeMeshStatus::Ok;
    }
    (*set).inner.merge(&(*other).inner);
    SafeMeshStatus::Ok
}

/// Return an owned sorted array; on error, `out` is untouched.
/// # Safety
/// `set` must be null or live. `out` must be null or writable, non-aliasing storage.
/// Release a successful output with safemesh_u64s_free before overwriting it.
#[no_mangle]
pub unsafe extern "C" fn safemesh_orset_elements(
    set: *const SafeMeshOrSet,
    out: *mut SafeMeshU64s,
) -> SafeMeshStatus {
    let Some(set) = set.as_ref() else {
        return SafeMeshStatus::NullPointer;
    };
    if out.is_null() {
        return SafeMeshStatus::NullPointer;
    }
    out.write(SafeMeshU64s::from_vec(
        set.inner.elements().into_iter().collect(),
    ));
    SafeMeshStatus::Ok
}

/// Return an owned sorted array; on error, `out` is untouched.
/// # Safety
/// `set` must be null or live. `out` must be null or writable, non-aliasing storage.
/// Release a successful output with safemesh_u64s_free before overwriting it.
#[no_mangle]
pub unsafe extern "C" fn safemesh_orset_observed_tokens(
    set: *const SafeMeshOrSet,
    element: u64,
    out: *mut SafeMeshU64s,
) -> SafeMeshStatus {
    let Some(set) = set.as_ref() else {
        return SafeMeshStatus::NullPointer;
    };
    if out.is_null() {
        return SafeMeshStatus::NullPointer;
    }
    out.write(SafeMeshU64s::from_vec(
        set.inner.observed_tokens(&element).into_iter().collect(),
    ));
    SafeMeshStatus::Ok
}

/// Return an owned sorted array; on error, `out` is untouched.
/// # Safety
/// `set` must be null or live. `out` must be null or writable, non-aliasing storage.
/// Release a successful output with safemesh_u64s_free before overwriting it.
#[no_mangle]
pub unsafe extern "C" fn safemesh_orset_tombstones(
    set: *const SafeMeshOrSet,
    out: *mut SafeMeshU64s,
) -> SafeMeshStatus {
    let Some(set) = set.as_ref() else {
        return SafeMeshStatus::NullPointer;
    };
    if out.is_null() {
        return SafeMeshStatus::NullPointer;
    }
    out.write(SafeMeshU64s::from_vec(
        set.inner.tombstones().iter().copied().collect(),
    ));
    SafeMeshStatus::Ok
}
