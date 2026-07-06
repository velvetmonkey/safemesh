/* Generated TypeScript surface for SafeMesh WASM bindings. */
export class SafeMeshGCounter {
  constructor(replicas: number);
  applyBump(replica: number, tally: bigint): void;
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

export function gcounterDeltaToWire(replica: number, tally: bigint): Uint8Array;
export function lwwRegisterDeltaToWire(timestamp: bigint, replica: bigint, value: bigint): Uint8Array;
