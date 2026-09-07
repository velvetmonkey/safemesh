/* Generated TypeScript surface for SafeMesh WASM bindings. */
export class SafeMeshGCounter {
  constructor(replicas: number);
  applyBump(replica: number, tally: bigint): void;
  tryApplyBump(replica: number, tally: bigint): void;
  value(): bigint;
  state(): BigUint64Array;
  free(): void;
}

export class SafeMeshGCounterReplica {
  constructor(replicaId: bigint, replicas: number);
  appendBump(counterReplica: number, tally: bigint): Uint8Array;
  mergeRecordBytes(bytes: Uint8Array): void;
  mergeLogBytes(bytes: Uint8Array): void;
  logBytes(): Uint8Array;
  versionFor(replica: bigint): bigint;
  value(): bigint;
  sameStateAs(other: SafeMeshGCounterReplica): boolean;
  state(): BigUint64Array;
  free(): void;
}

export class SafeMeshLwwRegister {
  constructor();
  set(timestamp: bigint, replica: bigint, value: bigint): void;
  hasValue(): boolean;
  valueOr(defaultValue: bigint): bigint;
  timestampOr(defaultValue: bigint): bigint;
  writerReplicaOr(defaultValue: bigint): bigint;
  free(): void;
}

export class SafeMeshEnableWinsFlag {
  constructor();
  enable(token: bigint): void;
  disableObserved(): void;
  value(): boolean;
  enabledTokens(): BigUint64Array;
  tombstoneTokens(): BigUint64Array;
  free(): void;
}

export class SafeMeshLwwMap {
  constructor();
  set(key: bigint, timestamp: bigint, replica: bigint, value: bigint): void;
  remove(key: bigint, timestamp: bigint, replica: bigint): void;
  hasKey(key: bigint): boolean;
  valueOr(key: bigint, defaultValue: bigint): bigint;
  visibleKeys(): BigUint64Array;
  entryKeys(): BigUint64Array;
  removalKeys(): BigUint64Array;
  free(): void;
}

export class SafeMeshLwwRegisterReplica {
  constructor(replicaId: bigint);
  appendSet(timestamp: bigint, writerReplica: bigint, value: bigint): Uint8Array;
  mergeRecordBytes(bytes: Uint8Array): void;
  mergeLogBytes(bytes: Uint8Array): void;
  logBytes(): Uint8Array;
  versionFor(replica: bigint): bigint;
  hasValue(): boolean;
  valueOr(defaultValue: bigint): bigint;
  timestampOr(defaultValue: bigint): bigint;
  writerReplicaOr(defaultValue: bigint): bigint;
  free(): void;
}

export class SafeMeshLwwMapReplica {
  constructor(replicaId: bigint);
  appendSet(key: bigint, timestamp: bigint, writerReplica: bigint, value: bigint): Uint8Array;
  appendRemove(key: bigint, timestamp: bigint, writerReplica: bigint): Uint8Array;
  mergeRecordBytes(bytes: Uint8Array): void;
  mergeLogBytes(bytes: Uint8Array): void;
  logBytes(): Uint8Array;
  versionFor(replica: bigint): bigint;
  hasKey(key: bigint): boolean;
  valueOr(key: bigint, defaultValue: bigint): bigint;
  visibleKeys(): BigUint64Array;
  entryKeys(): BigUint64Array;
  removalKeys(): BigUint64Array;
  free(): void;
}

export class SafeMeshEnableWinsFlagReplica {
  constructor(replicaId: bigint);
  appendEnable(token: bigint): Uint8Array;
  appendDisableObserved(): Uint8Array;
  mergeRecordBytes(bytes: Uint8Array): void;
  mergeLogBytes(bytes: Uint8Array): void;
  logBytes(): Uint8Array;
  versionFor(replica: bigint): bigint;
  value(): boolean;
  enabledTokens(): BigUint64Array;
  tombstoneTokens(): BigUint64Array;
  free(): void;
}

export function gcounterDeltaToWire(replica: number, tally: bigint): Uint8Array;
export function lwwRegisterDeltaToWire(timestamp: bigint, replica: bigint, value: bigint): Uint8Array;
export function lwwMapSetDeltaToWire(key: bigint, timestamp: bigint, replica: bigint, value: bigint): Uint8Array;
export function lwwMapRemoveDeltaToWire(key: bigint, timestamp: bigint, replica: bigint): Uint8Array;
export function enableWinsFlagEnableDeltaToWire(token: bigint): Uint8Array;
export function enableWinsFlagDisableDeltaToWire(tokens: BigUint64Array): Uint8Array;

export class SafeMeshOrSet {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Add an element with a caller-supplied token, exactly as in the Rust core.
     */
    add(element: bigint, token: bigint): void;
    /**
     * Tombstone tokens globally, including tokens whose adds have not arrived yet.
     */
    applyRemove(tokens: BigUint64Array): void;
    contains(element: bigint): boolean;
    elements(): BigUint64Array;
    merge(other: SafeMeshOrSet): void;
    constructor();
    observedTokens(element: bigint): BigUint64Array;
    tombstones(): BigUint64Array;
}
