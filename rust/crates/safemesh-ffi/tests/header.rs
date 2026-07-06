// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

const HEADER: &str = include_str!("../include/safemesh.h");

const EXPECTED_HEADER: &str = r#"/* Generated with cbindgen; do not edit by hand. */
#ifndef SAFEMESH_H
#define SAFEMESH_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct SafeMeshGCounter SafeMeshGCounter;

typedef struct SafeMeshBytes {
  uint8_t *ptr;
  size_t len;
  size_t cap;
} SafeMeshBytes;

SafeMeshGCounter *safemesh_gcounter_new(size_t replicas);

void safemesh_gcounter_free(SafeMeshGCounter *counter);

bool safemesh_gcounter_apply_bump(SafeMeshGCounter *counter,
                                  size_t replica,
                                  uint64_t tally);

uint64_t safemesh_gcounter_value(const SafeMeshGCounter *counter);

SafeMeshBytes safemesh_gcounter_delta_to_wire(size_t replica,
                                              uint64_t tally);

void safemesh_bytes_free(SafeMeshBytes bytes);

#ifdef __cplusplus
}
#endif

#endif /* SAFEMESH_H */
"#;

#[test]
fn committed_header_has_not_drifted() {
    assert_eq!(HEADER, EXPECTED_HEADER);
}
