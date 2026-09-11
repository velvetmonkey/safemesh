// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

const DTS: &str = include_str!("../bindings/safemesh_wasm.d.ts");

#[test]
fn typescript_surface_lists_the_public_binding() {
    assert!(DTS.contains("export class SafeMeshGCounter"));
    assert!(DTS.contains("export class SafeMeshOrSet"));
    assert!(DTS.contains("tryApplyBump(replica: number, tally: bigint): void"));
    assert!(DTS.contains("applyRemove(tokens: BigUint64Array): void"));
    assert!(DTS.contains("export class SafeMeshGCounterReplica"));
    assert!(DTS.contains("export class SafeMeshLwwRegister"));
    assert!(DTS.contains("export class SafeMeshLwwRegisterReplica"));
    assert!(DTS.contains("export class SafeMeshEnableWinsFlag"));
    assert!(DTS.contains("export class SafeMeshEnableWinsFlagReplica"));
    assert!(DTS.contains("export class SafeMeshLwwMap"));
    assert!(DTS.contains("export class SafeMeshLwwMapReplica"));
    assert!(DTS.contains("export class SafeMeshStringOrSetReplica"));
    assert!(DTS.contains("export class SafeMeshStringOrSetAddEntry"));
    assert!(DTS.contains("export class SafeMeshStringOrSetRecord"));
    assert!(DTS.contains("appendAdd(element: string, token: bigint): Uint8Array"));
    assert!(DTS.contains(
        "static createAllocated(writers: bigint, author: bigint): SafeMeshStringOrSetReplica"
    ));
    assert!(DTS.contains("appendAllocatedAdd(element: string): Uint8Array"));
    assert!(DTS.contains("exportIdentity(): Uint8Array"));
    assert!(DTS.contains("static importIdentity(bytes: Uint8Array): SafeMeshStringOrSetReplica"));
    assert!(DTS.contains("appendRemoveObserved(element: string): Uint8Array"));
    assert!(DTS.contains("mergeRecordBytes(bytes: Uint8Array): \"accepted\" | \"duplicate\""));
    assert!(DTS.contains("elements(): string[]"));
    assert!(DTS.contains("observedTokens(element: string): BigUint64Array"));
    assert!(DTS.contains("addEntries(): SafeMeshStringOrSetAddEntry[]"));
    assert!(DTS.contains("static inspectRecordBytes(bytes: Uint8Array): SafeMeshStringOrSetRecord"));
    assert!(DTS.contains("deltaKind(): \"add\" | \"remove\""));
    assert!(DTS.contains("element(): string | undefined"));
    assert!(DTS.contains("token(): bigint | undefined"));
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
    assert_eq!(
        DTS.matches(
            "mergeLogBytes(bytes: Uint8Array): (\"accepted\" | \"duplicate\" | \"collision\")[]"
        )
        .count(),
        5
    );
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
