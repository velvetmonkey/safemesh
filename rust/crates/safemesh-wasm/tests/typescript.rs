// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

const DTS: &str = include_str!("../bindings/safemesh_wasm.d.ts");

#[test]
fn typescript_surface_lists_the_public_binding() {
    assert!(DTS.contains("export class SafeMeshGCounter"));
    assert!(DTS.contains("export class SafeMeshGCounterReplica"));
    assert!(DTS.contains("export class SafeMeshLwwRegister"));
    assert!(DTS.contains("export class SafeMeshLwwRegisterReplica"));
    assert!(DTS.contains("export class SafeMeshEnableWinsFlag"));
    assert!(DTS.contains("export class SafeMeshEnableWinsFlagReplica"));
    assert!(DTS.contains("export class SafeMeshLwwMap"));
    assert!(DTS.contains("export class SafeMeshLwwMapReplica"));
    assert!(DTS.contains("applyBump(replica: number, tally: bigint): void"));
    assert!(DTS.contains(
        "appendSet(timestamp: bigint, writerReplica: bigint, value: bigint): Uint8Array"
    ));
    assert!(DTS.contains(
        "appendSet(key: bigint, timestamp: bigint, writerReplica: bigint, value: bigint): Uint8Array"
    ));
    assert!(DTS.contains(
        "appendRemove(key: bigint, timestamp: bigint, writerReplica: bigint): Uint8Array"
    ));
    assert!(DTS.contains("appendEnable(token: bigint): Uint8Array"));
    assert!(DTS.contains("appendDisableObserved(): Uint8Array"));
    assert!(DTS.contains("mergeRecordBytes(bytes: Uint8Array): void"));
    assert!(DTS.contains("gcounterDeltaToWire(replica: number, tally: bigint): Uint8Array"));
    assert!(DTS.contains(
        "lwwRegisterDeltaToWire(timestamp: bigint, replica: bigint, value: bigint): Uint8Array"
    ));
    assert!(DTS.contains(
        "lwwMapSetDeltaToWire(key: bigint, timestamp: bigint, replica: bigint, value: bigint): Uint8Array"
    ));
    assert!(DTS.contains(
        "lwwMapRemoveDeltaToWire(key: bigint, timestamp: bigint, replica: bigint): Uint8Array"
    ));
    assert!(DTS.contains("enableWinsFlagEnableDeltaToWire(token: bigint): Uint8Array"));
    assert!(DTS.contains("enableWinsFlagDisableDeltaToWire(tokens: BigUint64Array): Uint8Array"));
}
