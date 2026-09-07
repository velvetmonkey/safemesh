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

void safemesh_gcounter_free(struct SafeMeshGCounter *counter);

bool safemesh_gcounter_apply_bump(struct SafeMeshGCounter *counter, size_t replica, uint64_t tally);

uint64_t safemesh_gcounter_value(const struct SafeMeshGCounter *counter);

struct SafeMeshBytes safemesh_gcounter_delta_to_wire(size_t replica, uint64_t tally);

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
