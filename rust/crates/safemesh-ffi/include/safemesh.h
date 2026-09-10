#ifndef SAFEMESH_H
#define SAFEMESH_H

/* Generated with cbindgen; do not edit by hand. */

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/**
 * Status returned by checked operations. Out-of-range coordinates do not mutate state.
 */
typedef enum SafeMeshStatus {
  Ok = 0,
  NullPointer = 1,
  ReplicaOutOfRange = 2,
} SafeMeshStatus;

typedef struct SafeMeshGCounter SafeMeshGCounter;

/**
 * OR-Set with u64 elements and globally scoped, caller-supplied u64 tokens.
 */
typedef struct SafeMeshOrSet SafeMeshOrSet;

typedef struct SafeMeshBytes {
  uint8_t *ptr;
  size_t len;
  size_t cap;
} SafeMeshBytes;

/**
 * Owned array. Release exactly once with safemesh_u64s_free; do not modify its fields.
 */
typedef struct SafeMeshU64s {
  uint64_t *ptr;
  size_t len;
  size_t cap;
} SafeMeshU64s;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

struct SafeMeshGCounter *safemesh_gcounter_new(size_t replicas);

/**
 * Release a counter handle. Null is accepted and ignored.
 *
 * # Safety
 * `counter` must be null or a handle returned by `safemesh_gcounter_new` that has not been
 * freed. The handle is consumed: ownership passes to this call, the memory is released, and
 * the caller no longer owns the pointer afterwards. It must not read, write, or free the
 * pointer again; a second call on the same handle is a double free. No other use of the
 * counter may be in progress during the call. The null check cannot detect a dangling or
 * already-freed pointer; passing one is undefined behaviour.
 */
void safemesh_gcounter_free(struct SafeMeshGCounter *counter);

/**
 * Apply a bump. Returns false only for a null handle; an out-of-range replica is ignored.
 *
 * # Safety
 * `counter` must be null or a live handle returned by `safemesh_gcounter_new` that has not
 * been freed. The call needs exclusive access for its duration: no other read or write of
 * the same counter may overlap it. The caller keeps ownership; the handle stays valid after
 * the call and must still be released once with `safemesh_gcounter_free`. The null check
 * cannot detect a dangling or already-freed pointer; passing one is undefined behaviour.
 */
bool safemesh_gcounter_apply_bump(struct SafeMeshGCounter *counter, size_t replica, uint64_t tally);

/**
 * Read the counter total. A null handle reads as 0.
 *
 * # Safety
 * `counter` must be null or a live handle returned by `safemesh_gcounter_new` that has not
 * been freed. Concurrent reads may overlap, but no write to the same counter may be in
 * progress during the call. The caller keeps ownership; the handle stays valid after the
 * call. The null check cannot detect a dangling or already-freed pointer; passing one is
 * undefined behaviour.
 */
uint64_t safemesh_gcounter_value(const struct SafeMeshGCounter *counter);

struct SafeMeshBytes safemesh_gcounter_delta_to_wire(size_t replica, uint64_t tally);

/**
 * Release a byte buffer returned by `safemesh_gcounter_delta_to_wire`. A null `ptr` is ignored.
 *
 * # Safety
 * `bytes` must be a value returned by this library with `ptr`, `len` and `cap` unmodified,
 * and it must not have been released before. The buffer is consumed: ownership passes to
 * this call and the caller must not read or free `ptr` again; a second call on the same
 * value is a double free. A buffer that did not come from this library, or whose fields were
 * changed, is undefined behaviour.
 */
void safemesh_bytes_free(struct SafeMeshBytes bytes);

/**
 * # Safety
 * `counter` must be null or a live, exclusively accessible counter handle.
 */
enum SafeMeshStatus safemesh_gcounter_try_apply_bump(struct SafeMeshGCounter *counter,
                                                     size_t replica,
                                                     uint64_t tally);

struct SafeMeshOrSet *safemesh_orset_new(void);

/**
 * # Safety
 * `set` must be null or a live handle, freed exactly once with no outstanding uses.
 */
void safemesh_orset_free(struct SafeMeshOrSet *set);

/**
 * # Safety
 * `values` must be an unmodified array returned by this API, released exactly once.
 */
void safemesh_u64s_free(struct SafeMeshU64s values);

/**
 * # Safety
 * `set` must be null or a live, exclusively accessible handle.
 */
enum SafeMeshStatus safemesh_orset_add(struct SafeMeshOrSet *set, uint64_t element, uint64_t token);

/**
 * Tombstone a token globally, even if its add has not arrived. Repeat for multiple tokens.
 * # Safety
 * `set` must be null or a live, exclusively accessible handle.
 */
enum SafeMeshStatus safemesh_orset_apply_remove(struct SafeMeshOrSet *set, uint64_t token);

/**
 * Merge all adds and tombstones. Merging a handle with itself is a no-op.
 * # Safety
 * Handles must be null or live; `set` must be exclusively accessible during the call.
 */
enum SafeMeshStatus safemesh_orset_merge(struct SafeMeshOrSet *set,
                                         const struct SafeMeshOrSet *other);

/**
 * Return an owned sorted array; on error, `out` is untouched.
 * # Safety
 * `set` must be null or live. `out` must be null or writable, non-aliasing storage.
 * Release a successful output with safemesh_u64s_free before overwriting it.
 */
enum SafeMeshStatus safemesh_orset_elements(const struct SafeMeshOrSet *set,
                                            struct SafeMeshU64s *out);

/**
 * Return an owned sorted array; on error, `out` is untouched.
 * # Safety
 * `set` must be null or live. `out` must be null or writable, non-aliasing storage.
 * Release a successful output with safemesh_u64s_free before overwriting it.
 */
enum SafeMeshStatus safemesh_orset_observed_tokens(const struct SafeMeshOrSet *set,
                                                   uint64_t element,
                                                   struct SafeMeshU64s *out);

/**
 * Return an owned sorted array; on error, `out` is untouched.
 * # Safety
 * `set` must be null or live. `out` must be null or writable, non-aliasing storage.
 * Release a successful output with safemesh_u64s_free before overwriting it.
 */
enum SafeMeshStatus safemesh_orset_tombstones(const struct SafeMeshOrSet *set,
                                              struct SafeMeshU64s *out);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* SAFEMESH_H */
