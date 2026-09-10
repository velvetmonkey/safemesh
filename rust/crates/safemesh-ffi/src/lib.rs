// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

//! Thin C ABI spine over the SafeMesh Rust core.
//!
//! Every function that takes a handle or an owned buffer is `unsafe extern "C"`, because the
//! caller, not the compiler, guarantees that the pointer is live, unaliased, and released
//! exactly once. Each such function carries a `# Safety` section naming what the caller must
//! guarantee and whether the call consumes the pointer. The C ABI is unaffected: an
//! `unsafe extern "C"` function exports the same symbol and calling convention as `extern "C"`.
//!
//! # Safety gate
//!
//! Safe Rust cannot reach any pointer operation whose validity it cannot prove. Each block below
//! is a `compile_fail` doctest that `cargo test -p safemesh-ffi --doc` rejects if it ever
//! compiles. Stable rustdoc does not check the annotated error code, so the enforced gate is
//! `tests/compile_fail.rs`, which compiles the same cases from `tests/compile_fail/` and requires
//! the specific error `E0133` (call to unsafe function requires an `unsafe` block).
//!
//! ```compile_fail,E0133
//! let counter = safemesh_ffi::safemesh_gcounter_new(1);
//! safemesh_ffi::safemesh_gcounter_free(counter);
//! ```
//!
//! ```compile_fail,E0133
//! let counter = safemesh_ffi::safemesh_gcounter_new(1);
//! safemesh_ffi::safemesh_gcounter_apply_bump(counter, 0, 1);
//! ```
//!
//! ```compile_fail,E0133
//! let counter = safemesh_ffi::safemesh_gcounter_new(1);
//! let mut total = 0u64;
//! safemesh_ffi::safemesh_gcounter_try_value(counter, &mut total);
//! ```
//!
//! ```compile_fail,E0133
//! let bytes = safemesh_ffi::safemesh_gcounter_delta_to_wire(0, 1);
//! safemesh_ffi::safemesh_bytes_free(bytes);
//! ```
//!
//! The double free that motivated the gate is also rejected outright:
//!
//! ```compile_fail,E0133
//! let counter = safemesh_ffi::safemesh_gcounter_new(2);
//! safemesh_ffi::safemesh_gcounter_free(counter);
//! safemesh_ffi::safemesh_gcounter_free(counter);
//! ```

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

/// Release a counter handle. Null is accepted and ignored.
///
/// # Safety
/// `counter` must be null or a handle returned by `safemesh_gcounter_new` that has not been
/// freed. The handle is consumed: ownership passes to this call, the memory is released, and
/// the caller no longer owns the pointer afterwards. It must not read, write, or free the
/// pointer again; a second call on the same handle is a double free. No other use of the
/// counter may be in progress during the call. The null check cannot detect a dangling or
/// already-freed pointer; passing one is undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn safemesh_gcounter_free(counter: *mut SafeMeshGCounter) {
    if !counter.is_null() {
        drop(Box::from_raw(counter));
    }
}

/// Apply a bump. Returns false only for a null handle; an out-of-range replica is ignored.
///
/// # Safety
/// `counter` must be null or a live handle returned by `safemesh_gcounter_new` that has not
/// been freed. The call needs exclusive access for its duration: no other read or write of
/// the same counter may overlap it. The caller keeps ownership; the handle stays valid after
/// the call and must still be released once with `safemesh_gcounter_free`. The null check
/// cannot detect a dangling or already-freed pointer; passing one is undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn safemesh_gcounter_apply_bump(
    counter: *mut SafeMeshGCounter,
    replica: usize,
    tally: u64,
) -> bool {
    match counter.as_mut() {
        Some(counter) => {
            counter.inner.apply_bump(replica, tally);
            true
        }
        None => false,
    }
}

/// Read the counter total into `out`. Returns `Ok` after writing the true total;
/// `NullPointer` for a null handle or a null `out`; `ValueOverflow` when the true total does
/// not fit in `uint64_t`. On any status other than `Ok`, `out` is untouched, so a caller never
/// receives a wrapped, truncated or clamped total in place of the real one.
///
/// # Safety
/// `counter` must be null or a live handle returned by `safemesh_gcounter_new` that has not
/// been freed. Concurrent reads may overlap, but no write to the same counter may be in
/// progress during the call. The caller keeps ownership; the handle stays valid after the
/// call. `out` must be null or point to writable, properly aligned `uint64_t` storage that no
/// other access overlaps during the call. The null checks cannot detect a dangling or
/// already-freed pointer; passing one is undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn safemesh_gcounter_try_value(
    counter: *const SafeMeshGCounter,
    out: *mut u64,
) -> SafeMeshStatus {
    let Some(counter) = counter.as_ref() else {
        return SafeMeshStatus::NullPointer;
    };
    let Some(out) = out.as_mut() else {
        return SafeMeshStatus::NullPointer;
    };
    match u64::try_from(counter.inner.value()) {
        Ok(total) => {
            *out = total;
            SafeMeshStatus::Ok
        }
        Err(_) => SafeMeshStatus::ValueOverflow,
    }
}

#[no_mangle]
pub extern "C" fn safemesh_gcounter_delta_to_wire(replica: usize, tally: u64) -> SafeMeshBytes {
    match (GCounterDelta { replica, tally }).to_wire_bytes() {
        Ok(bytes) => SafeMeshBytes::from_vec(bytes),
        Err(_) => SafeMeshBytes::empty(),
    }
}

/// Release a byte buffer returned by `safemesh_gcounter_delta_to_wire`. A null `ptr` is ignored.
///
/// # Safety
/// `bytes` must be a value returned by this library with `ptr`, `len` and `cap` unmodified,
/// and it must not have been released before. The buffer is consumed: ownership passes to
/// this call and the caller must not read or free `ptr` again; a second call on the same
/// value is a double free. A buffer that did not come from this library, or whose fields were
/// changed, is undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn safemesh_bytes_free(bytes: SafeMeshBytes) {
    if !bytes.ptr.is_null() {
        drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
    }
}

/// Status returned by checked operations. Out-of-range coordinates do not mutate state.
#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub enum SafeMeshStatus {
    Ok = 0,
    NullPointer = 1,
    ReplicaOutOfRange = 2,
    /// The true counter total does not fit the 64-bit output; nothing was written.
    ValueOverflow = 3,
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
