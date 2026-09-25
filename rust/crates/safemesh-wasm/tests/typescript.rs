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
    assert_eq!(
        DTS.matches(
            "mergeStateBytes(bytes: Uint8Array, max_collection_elements?: number | null): void"
        )
        .count(),
        2
    );
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
    assert!(DTS.contains("mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): \"accepted\" | \"duplicate\""));
    assert!(DTS.contains("elements(): string[]"));
    assert!(DTS.contains("observedTokens(element: string): BigUint64Array"));
    assert!(DTS.contains("addEntries(): SafeMeshStringOrSetAddEntry[]"));
    assert!(DTS.contains("static inspectRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): SafeMeshStringOrSetRecord"));
    assert!(DTS.contains("deltaKind(): \"add\" | \"remove\""));
    assert!(DTS.contains("element(): string | undefined"));
    assert!(DTS.contains("token(): bigint | undefined"));
    assert!(DTS.contains("applyBump(replica: number, tally: bigint): void"));
    assert!(DTS.contains(
        "appendSet(timestamp: bigint, writer_replica: bigint, value: bigint): Uint8Array"
    ));
    assert!(DTS.contains(
        "appendSet(key: bigint, timestamp: bigint, writer_replica: bigint, value: bigint): Uint8Array"
    ));
    assert!(DTS.contains(
        "appendRemove(key: bigint, timestamp: bigint, writer_replica: bigint): Uint8Array"
    ));
    assert!(DTS.contains("appendEnable(token: bigint): Uint8Array"));
    assert!(DTS.contains("appendDisableObserved(): Uint8Array"));
    assert!(DTS.contains(
        "mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): void"
    ));
    assert_eq!(
        DTS.matches(
            "mergeLogBytes(bytes: Uint8Array, max_collection_elements?: number | null): (\"accepted\" | \"duplicate\" | \"collision\")[]"
        )
        .count(),
        6
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

/// Use the same wasm-pack generation command as scripts/package-wasm.sh (bundler).
/// Keep build artifacts separate: this test runs inside an outer cargo test build.
#[test]
fn committed_typescript_matches_fresh_generation() {
    use std::{fs, path::Path, process::Command, time::SystemTime};

    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = crate_dir.parent().unwrap().parent().unwrap();
    let generation_root = workspace.join("target/typescript-generation");
    fs::create_dir_all(&generation_root).unwrap();
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let output_dir = generation_root.join(format!("pkg-{}-{nonce}", std::process::id()));
    // A new directory guarantees an old generated declaration cannot mask drift.
    fs::create_dir(&output_dir).unwrap();
    let generated = Command::new("wasm-pack")
        .arg("build")
        .arg(crate_dir)
        .args(["--target", "bundler", "--out-dir"])
        .arg(&output_dir)
        .args(["--release", "--", "--locked"])
        .env("CARGO_TARGET_DIR", generation_root.join("build"))
        .output()
        .expect("wasm-pack 0.15.0 must be installed to regenerate the declarations");
    assert!(
        generated.status.success(),
        "TypeScript generation failed ({}):\n{}\n{}",
        generated.status,
        String::from_utf8_lossy(&generated.stdout),
        String::from_utf8_lossy(&generated.stderr),
    );

    let committed = crate_dir.join("bindings/safemesh_wasm.d.ts");
    let fresh = output_dir.join("safemesh_wasm.d.ts");
    // Compare bytes, including whitespace, and report a unified diff on any drift.
    if fs::read(&committed).unwrap() != fs::read(&fresh).unwrap() {
        let diff = Command::new("diff")
            .args([
                "-u",
                "--label",
                "committed/safemesh_wasm.d.ts",
                "--label",
                "generated/safemesh_wasm.d.ts",
            ])
            .arg(&committed)
            .arg(&fresh)
            .output()
            .expect("diff is required to report TypeScript declaration drift");
        panic!(
            "committed TypeScript declarations have drifted; regenerate with scripts/package-wasm.sh bundler and copy its safemesh_wasm.d.ts into bindings/\n{}\n{}",
            String::from_utf8_lossy(&diff.stdout),
            String::from_utf8_lossy(&diff.stderr),
        );
    }
}
