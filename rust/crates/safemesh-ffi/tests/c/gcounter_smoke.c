/* SafeMesh — delta-state CRDT convergence, built on crdt-lean.
 * Copyright (C) 2026 Ben Cassie
 * SPDX-License-Identifier: Apache-2.0
 *
 * External C caller for the SafeMesh C ABI, driven by scripts/ffi-c-smoke.sh.
 * Valid ownership only: every handle and buffer is released exactly once.
 * A non-zero exit names the first check that failed.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include "safemesh.h"

int main(void) {
    SafeMeshGCounter *counter = safemesh_gcounter_new(3);
    if (counter == NULL) {
        return 2;
    }
    if (!safemesh_gcounter_apply_bump(counter, 1, 5)) {
        return 3;
    }
    /* A lower tally for the same replica is absorbed by the max join. */
    if (!safemesh_gcounter_apply_bump(counter, 1, 2)) {
        return 4;
    }
    if (safemesh_gcounter_try_apply_bump(counter, 3, 9) != ReplicaOutOfRange) {
        return 5;
    }
    if (safemesh_gcounter_try_apply_bump(counter, 0, 4) != Ok) {
        return 6;
    }
    uint64_t value = 0;
    if (safemesh_gcounter_try_value(counter, &value) != Ok || value != 9) {
        return 7;
    }
    /* A total past UINT64_MAX is reported as ValueOverflow and `value` is left alone. */
    if (!safemesh_gcounter_apply_bump(counter, 2, UINT64_MAX)) {
        return 9;
    }
    if (safemesh_gcounter_try_value(counter, &value) != ValueOverflow || value != 9) {
        return 10;
    }

    SafeMeshBytes bytes = safemesh_gcounter_delta_to_wire(2, 7);
    if (bytes.ptr == NULL || bytes.len != 17 || bytes.ptr[0] != 0x10) {
        return 8;
    }
    /* Ownership of the buffer passes to the library here; `bytes` is not touched again. */
    safemesh_bytes_free(bytes);
    /* Ownership of the handle passes to the library here; `counter` is not touched again. */
    safemesh_gcounter_free(counter);

    printf("SAFEMESH_C_ABI value=%llu wire_len=%zu\n", (unsigned long long)value, (size_t)17);
    return 0;
}
