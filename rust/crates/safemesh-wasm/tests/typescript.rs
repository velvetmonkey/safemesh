// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

const DTS: &str = include_str!("../bindings/safemesh_wasm.d.ts");

#[test]
fn typescript_surface_lists_the_public_binding() {
    assert!(DTS.contains("export class SafeMeshGCounter"));
    assert!(DTS.contains("export class SafeMeshGCounterReplica"));
    assert!(DTS.contains("applyBump(replica: number, tally: bigint): void"));
    assert!(DTS.contains("mergeRecordBytes(bytes: Uint8Array): void"));
    assert!(DTS.contains("gcounterDeltaToWire(replica: number, tally: bigint): Uint8Array"));
}
