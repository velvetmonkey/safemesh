/* tslint:disable */
/* eslint-disable */

export interface SafeMeshStore {
    open(mode: "fresh" | "restart", writer: bigint): unknown;
    readCommitted(lease: unknown): { bytes: Uint8Array; revision: bigint; anchor: bigint };
    commit(lease: unknown, expectedRevision: bigint, nextBytes: Uint8Array): bigint;
    close(lease: unknown): void;
}
export interface SafeMeshManagedCounterOptions {
    mode: "fresh" | "restart";
    writer: bigint;
    writers?: number;
}
export type SafeMeshCounterEnvelopeVersion = 1;
export type SafeMeshStoreErrorCode = "MISSING" | "CORRUPT" | "STALE" | "EXISTS" |
"LOCKED" | "COMMIT" | "DISABLED" | "REENTRY" | "CLOSED" | "COLLISION";
export interface SafeMeshStoreError extends Error {
    name: "SafeMeshStoreError";
    code: SafeMeshStoreErrorCode;
}



export class SafeMeshEnableWinsFlag {
    free(): void;
    [Symbol.dispose](): void;
    disableObserved(): void;
    enable(token: bigint): void;
    enabledTokens(): BigUint64Array;
    constructor();
    tombstoneTokens(): BigUint64Array;
    value(): boolean;
}

export class SafeMeshEnableWinsFlagReplica {
    free(): void;
    [Symbol.dispose](): void;
    appendDisableObserved(): Uint8Array;
    appendEnable(token: bigint): Uint8Array;
    enabledTokens(): BigUint64Array;
    logBytes(): Uint8Array;
    /**
     * Return one core admission verdict for every decoded input record.
     * `maxRecords` is the third argument; the second limits collection elements.
     */
    mergeLogBytes(bytes: Uint8Array, max_collection_elements?: number | null, maxRecords?: number | null): ("accepted" | "duplicate" | "collision")[];
    /**
     * Return the core admission verdict for the decoded input record, as
     * `mergeLogBytes` does per record. Only `"accepted"` changes state.
     */
    mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): "accepted" | "duplicate" | "collision";
    constructor(replica_id: bigint);
    /**
     * Every record ID in log order, as `[author, sequence, ...]` pairs.
     * Reads IDs only; no record payload is decoded.
     */
    recordIds(): BigUint64Array;
    /**
     * The records a peer at `peerVersion` is missing, as one log batch
     * for its `mergeLogBytes`. `peerVersion` is `[author, prefix, ...]`
     * as `versionVector` returns it; sequence-zero records are always
     * included. The budgets are `mergeLogBytes`'s, in the same places,
     * and a batch over them throws the error that merge would throw.
     */
    sinceLogBytes(peerVersion: BigUint64Array, max_collection_elements?: number | null, maxRecords?: number | null): Uint8Array;
    tombstoneTokens(): BigUint64Array;
    value(): boolean;
    versionFor(replica: bigint): bigint;
    /**
     * `[author, versionFor(author), ...]` for every author with a
     * nonzero prefix, sorted by author: a peer's `sinceLogBytes` input.
     */
    versionVector(): BigUint64Array;
}

export class SafeMeshGCounter {
    free(): void;
    [Symbol.dispose](): void;
    applyBump(replica: number, tally: bigint): void;
    constructor(replicas: number);
    state(): BigUint64Array;
    /**
     * Apply a coordinate delta, throwing a descriptive Error for a bad index.
     */
    tryApplyBump(replica: number, tally: bigint): void;
    /**
     * The counter total as an exact `bigint`, also past the 64-bit boundary.
     */
    value(): bigint;
}

export class SafeMeshGCounterReplica {
    free(): void;
    [Symbol.dispose](): void;
    appendBump(counter_replica: number, tally: bigint): Uint8Array;
    logBytes(): Uint8Array;
    /**
     * Return one core admission verdict for every decoded input record.
     * `maxRecords` is the third argument; the second limits collection elements.
     */
    mergeLogBytes(bytes: Uint8Array, max_collection_elements?: number | null, maxRecords?: number | null): ("accepted" | "duplicate" | "collision")[];
    /**
     * Return the core admission verdict for the decoded input record, as
     * `mergeLogBytes` does per record. Only `"accepted"` changes state.
     */
    mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): "accepted" | "duplicate" | "collision";
    constructor(replica_id: bigint, replicas: number);
    /**
     * Every record ID in log order, as `[author, sequence, ...]` pairs.
     * Reads IDs only; no record payload is decoded.
     */
    recordIds(): BigUint64Array;
    /**
     * Compare the Rust-core carrier states without reproducing its equality in JavaScript.
     */
    sameStateAs(other: SafeMeshGCounterReplica): boolean;
    /**
     * The records a peer at `peerVersion` is missing, as one log batch
     * for its `mergeLogBytes`. `peerVersion` is `[author, prefix, ...]`
     * as `versionVector` returns it; sequence-zero records are always
     * included. The budgets are `mergeLogBytes`'s, in the same places,
     * and a batch over them throws the error that merge would throw.
     */
    sinceLogBytes(peerVersion: BigUint64Array, max_collection_elements?: number | null, maxRecords?: number | null): Uint8Array;
    state(): BigUint64Array;
    /**
     * The counter total as an exact `bigint`, also past the 64-bit boundary.
     */
    value(): bigint;
    versionFor(replica: bigint): bigint;
    /**
     * `[author, versionFor(author), ...]` for every author with a
     * nonzero prefix, sorted by author: a peer's `sinceLogBytes` input.
     */
    versionVector(): BigUint64Array;
}

/**
 * State-based WASM replica using the core's canonical full-carrier wire codec.
 * Operations return snapshots, not event-log records.
 */
export class SafeMeshGSetReplica {
    free(): void;
    [Symbol.dispose](): void;
    insert(value: bigint): Uint8Array;
    mergeStateBytes(bytes: Uint8Array, max_collection_elements?: number | null): void;
    constructor();
    stateBytes(): Uint8Array;
}

export class SafeMeshLwwMap {
    free(): void;
    [Symbol.dispose](): void;
    entryKeys(): BigUint64Array;
    hasKey(key: bigint): boolean;
    constructor();
    removalKeys(): BigUint64Array;
    remove(key: bigint, timestamp: bigint, replica: bigint): void;
    set(key: bigint, timestamp: bigint, replica: bigint, value: bigint): void;
    valueOr(key: bigint, default_value: bigint): bigint;
    visibleKeys(): BigUint64Array;
}

export class SafeMeshLwwMapReplica {
    free(): void;
    [Symbol.dispose](): void;
    appendRemove(key: bigint, timestamp: bigint, writer_replica: bigint): Uint8Array;
    appendSet(key: bigint, timestamp: bigint, writer_replica: bigint, value: bigint): Uint8Array;
    entryKeys(): BigUint64Array;
    hasKey(key: bigint): boolean;
    logBytes(): Uint8Array;
    /**
     * Return one core admission verdict for every decoded input record.
     * `maxRecords` is the third argument; the second limits collection elements.
     */
    mergeLogBytes(bytes: Uint8Array, max_collection_elements?: number | null, maxRecords?: number | null): ("accepted" | "duplicate" | "collision")[];
    /**
     * Return the core admission verdict for the decoded input record, as
     * `mergeLogBytes` does per record. Only `"accepted"` changes state.
     */
    mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): "accepted" | "duplicate" | "collision";
    constructor(replica_id: bigint);
    /**
     * Every record ID in log order, as `[author, sequence, ...]` pairs.
     * Reads IDs only; no record payload is decoded.
     */
    recordIds(): BigUint64Array;
    removalKeys(): BigUint64Array;
    /**
     * The records a peer at `peerVersion` is missing, as one log batch
     * for its `mergeLogBytes`. `peerVersion` is `[author, prefix, ...]`
     * as `versionVector` returns it; sequence-zero records are always
     * included. The budgets are `mergeLogBytes`'s, in the same places,
     * and a batch over them throws the error that merge would throw.
     */
    sinceLogBytes(peerVersion: BigUint64Array, max_collection_elements?: number | null, maxRecords?: number | null): Uint8Array;
    /**
     * Canonical complete carrier, including hidden entries and remove dots.
     */
    stateBytes(): Uint8Array;
    valueOr(key: bigint, default_value: bigint): bigint;
    versionFor(replica: bigint): bigint;
    /**
     * `[author, versionFor(author), ...]` for every author with a
     * nonzero prefix, sorted by author: a peer's `sinceLogBytes` input.
     */
    versionVector(): BigUint64Array;
    visibleKeys(): BigUint64Array;
}

export class SafeMeshLwwRegister {
    free(): void;
    [Symbol.dispose](): void;
    hasValue(): boolean;
    constructor();
    set(timestamp: bigint, replica: bigint, value: bigint): void;
    timestampOr(default_value: bigint): bigint;
    valueOr(default_value: bigint): bigint;
    writerReplicaOr(default_value: bigint): bigint;
}

export class SafeMeshLwwRegisterReplica {
    free(): void;
    [Symbol.dispose](): void;
    appendSet(timestamp: bigint, writer_replica: bigint, value: bigint): Uint8Array;
    hasValue(): boolean;
    logBytes(): Uint8Array;
    /**
     * Return one core admission verdict for every decoded input record.
     * `maxRecords` is the third argument; the second limits collection elements.
     */
    mergeLogBytes(bytes: Uint8Array, max_collection_elements?: number | null, maxRecords?: number | null): ("accepted" | "duplicate" | "collision")[];
    /**
     * Return the core admission verdict for the decoded input record, as
     * `mergeLogBytes` does per record. Only `"accepted"` changes state.
     */
    mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): "accepted" | "duplicate" | "collision";
    constructor(replica_id: bigint);
    /**
     * Every record ID in log order, as `[author, sequence, ...]` pairs.
     * Reads IDs only; no record payload is decoded.
     */
    recordIds(): BigUint64Array;
    /**
     * The records a peer at `peerVersion` is missing, as one log batch
     * for its `mergeLogBytes`. `peerVersion` is `[author, prefix, ...]`
     * as `versionVector` returns it; sequence-zero records are always
     * included. The budgets are `mergeLogBytes`'s, in the same places,
     * and a batch over them throws the error that merge would throw.
     */
    sinceLogBytes(peerVersion: BigUint64Array, max_collection_elements?: number | null, maxRecords?: number | null): Uint8Array;
    timestampOr(default_value: bigint): bigint;
    valueOr(default_value: bigint): bigint;
    versionFor(replica: bigint): bigint;
    /**
     * `[author, versionFor(author), ...]` for every author with a
     * nonzero prefix, sorted by author: a peer's `sinceLogBytes` input.
     */
    versionVector(): BigUint64Array;
    writerReplicaOr(default_value: bigint): bigint;
}

/**
 * Owns a synchronous Store lease. Call close explicitly before free.
 * All exported methods borrow through try_borrow: callback reentry is rejected
 * before touching state, including read/close/free attempts during commit.
 */
export class SafeMeshManagedGCounter {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    appendBump(tally: bigint): Uint8Array;
    close(): void;
    mergeLogBytes(bytes: Uint8Array): ("accepted" | "duplicate")[];
    mergeRecordBytes(bytes: Uint8Array): "accepted" | "duplicate";
    static open(store: SafeMeshStore, options: SafeMeshManagedCounterOptions): SafeMeshManagedGCounter;
    peerLogBytes(): Uint8Array;
    state(): BigUint64Array;
    value(): bigint;
    versionFor(writer: bigint): bigint;
}

/**
 * Observed-remove set of u64 elements and u64 tokens; tokens are global to the set.
 */
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

export class SafeMeshPnCounterReplica {
    free(): void;
    [Symbol.dispose](): void;
    appendDec(counter_replica: number, tally: bigint): Uint8Array;
    appendInc(counter_replica: number, tally: bigint): Uint8Array;
    logBytes(): Uint8Array;
    /**
     * Return one core admission verdict for every decoded input record.
     * `maxRecords` is the third argument; the second limits collection elements.
     */
    mergeLogBytes(bytes: Uint8Array, max_collection_elements?: number | null, maxRecords?: number | null): ("accepted" | "duplicate" | "collision")[];
    /**
     * Return the core admission verdict for the decoded input record, as
     * `mergeLogBytes` does per record. Only `"accepted"` changes state.
     */
    mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): "accepted" | "duplicate" | "collision";
    constructor(replica_id: bigint, replicas: number);
    /**
     * Every record ID in log order, as `[author, sequence, ...]` pairs.
     * Reads IDs only; no record payload is decoded.
     */
    recordIds(): BigUint64Array;
    /**
     * Compare the Rust-core carrier states without reproducing its equality in JavaScript.
     */
    sameStateAs(other: SafeMeshPnCounterReplica): boolean;
    /**
     * The records a peer at `peerVersion` is missing, as one log batch
     * for its `mergeLogBytes`. `peerVersion` is `[author, prefix, ...]`
     * as `versionVector` returns it; sequence-zero records are always
     * included. The budgets are `mergeLogBytes`'s, in the same places,
     * and a batch over them throws the error that merge would throw.
     */
    sinceLogBytes(peerVersion: BigUint64Array, max_collection_elements?: number | null, maxRecords?: number | null): Uint8Array;
    /**
     * Complete carrier: increment coordinates followed by decrement coordinates.
     */
    state(): BigUint64Array;
    /**
     * The counter total as an exact `bigint`, also past the 64-bit boundary.
     */
    value(): bigint;
    versionFor(replica: bigint): bigint;
    /**
     * `[author, versionFor(author), ...]` for every author with a
     * nonzero prefix, sorted by author: a peer's `sinceLogBytes` input.
     */
    versionVector(): BigUint64Array;
}

/**
 * State-based WASM replica using the core's canonical full-carrier wire codec.
 * Operations return snapshots, not event-log records.
 */
export class SafeMeshRgaReplica {
    free(): void;
    [Symbol.dispose](): void;
    delete(position: bigint): Uint8Array;
    insert(position: bigint, value: bigint): Uint8Array;
    mergeStateBytes(bytes: Uint8Array, max_collection_elements?: number | null): void;
    constructor();
    stateBytes(): Uint8Array;
}

/**
 * One `(element, token)` add pair as the core `OrSet` stores it.
 */
export class SafeMeshStringOrSetAddEntry {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    element(): string;
    token(): bigint;
}

/**
 * A record decoded by the core, exposed field by field so a consumer can label
 * record bytes without keeping its own metadata alongside them.
 *
 * `deltaKind()` is `"add"` (then `element()` and `token()` are set, `tokens()`
 * is empty) or `"remove"` (then `tokens()` carries the tombstoned tokens and
 * `element()`/`token()` are undefined).
 */
export class SafeMeshStringOrSetRecord {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    deltaKind(): "add" | "remove";
    element(): string | undefined;
    replica(): bigint;
    sequence(): bigint;
    token(): bigint | undefined;
    tokens(): BigUint64Array;
}

/**
 * Observed-remove set of UTF-8 string elements and u64 tokens, carried by an
 * event log so records can be replayed, deduplicated and repaired from a log.
 *
 * Every value is computed by `safemesh_crdt::OrSet<String, u64>` and
 * `safemesh_crdt::EventLog`. The optional allocated lifecycle checks ownership
 * and holds a live-author claim within this WASM instance. Tokens remain global
 * to the set, exactly as in `SafeMeshOrSet`.
 */
export class SafeMeshStringOrSetReplica {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Every `(element, token)` add pair the core holds, tombstoned or not.
     */
    addEntries(): SafeMeshStringOrSetAddEntry[];
    /**
     * Append an add record for `(element, token)` and return its wire bytes.
     * Allocated instances reject caller tokens; use appendAllocatedAdd instead.
     */
    appendAdd(element: string, token: bigint): Uint8Array;
    /**
     * Allocate through the Rust ownership rule, append, and return record bytes.
     */
    appendAllocatedAdd(element: string): Uint8Array;
    /**
     * Append a remove record tombstoning every token this replica has observed
     * for `element`, as the core reports them, and return its wire bytes.
     */
    appendRemoveObserved(element: string): Uint8Array;
    /**
     * Create an allocated writer. At most one allocated handle per author may
     * live in this WASM instance; free() releases it. The caller provides any
     * cross-instance/process exclusion and must not restore stale snapshots.
     */
    static createAllocated(writers: bigint, author: bigint): SafeMeshStringOrSetReplica;
    /**
     * Live members, sorted and unique, as the core computes them.
     */
    elements(): string[];
    /**
     * Export fixed writer configuration, next sequence, and the complete log.
     * The bytes are caller-persisted identity storage, not a transport packet.
     */
    exportIdentity(): Uint8Array;
    /**
     * Allocation/history consistency check; failure never creates a fresh writer.
     * A self-consistent stale snapshot is not detected. There is no disk I/O.
     */
    static importIdentity(bytes: Uint8Array, maxRecords?: number | null): SafeMeshStringOrSetReplica;
    /**
     * Decode record bytes through the core without admitting them anywhere.
     *
     * Named after `mergeRecordBytes`: same input, but this only looks. It does
     * not touch any replica, so it is static.
     */
    static inspectRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): SafeMeshStringOrSetRecord;
    logBytes(): Uint8Array;
    /**
     * Return one core admission verdict for every decoded input record.
     * `maxRecords` is the third argument; the second limits collection elements.
     */
    mergeLogBytes(bytes: Uint8Array, max_collection_elements?: number | null, maxRecords?: number | null): ("accepted" | "duplicate" | "collision")[];
    /**
     * Decode one record and admit it through the core event log.
     *
     * Returns the core's admission verdict, as `mergeLogBytes` does per
     * record: `"accepted"` when the record was new and applied, `"duplicate"`
     * when a record with the same identity and payload was already in the log,
     * and `"collision"` when the identity is known with a different payload.
     * Only `"accepted"` changes state. Decode and ownership failures throw.
     */
    mergeRecordBytes(bytes: Uint8Array, max_collection_elements?: number | null): "accepted" | "duplicate" | "collision";
    constructor(replica_id: bigint);
    /**
     * Live add tokens for `element`, excluding tombstoned tokens.
     */
    observedTokens(element: string): BigUint64Array;
    /**
     * Every record ID in log order, as `[author, sequence, ...]` pairs.
     * Reads IDs only; no record payload is decoded.
     */
    recordIds(): BigUint64Array;
    /**
     * The records a peer at `peerVersion` is missing, as one log batch
     * for its `mergeLogBytes`. `peerVersion` is `[author, prefix, ...]`
     * as `versionVector` returns it; sequence-zero records are always
     * included. The budgets are `mergeLogBytes`'s, in the same places,
     * and a batch over them throws the error that merge would throw.
     */
    sinceLogBytes(peerVersion: BigUint64Array, max_collection_elements?: number | null, maxRecords?: number | null): Uint8Array;
    tombstones(): BigUint64Array;
    versionFor(replica: bigint): bigint;
    /**
     * `[author, versionFor(author), ...]` for every author with a
     * nonzero prefix, sorted by author: a peer's `sinceLogBytes` input.
     */
    versionVector(): BigUint64Array;
}

export function enableWinsFlagDisableDeltaToWire(tokens: BigUint64Array): Uint8Array;

export function enableWinsFlagEnableDeltaToWire(token: bigint): Uint8Array;

export function gcounterDeltaToWire(replica: number, tally: bigint): Uint8Array;

export function initialize_bindings(): void;

export function lwwMapRemoveDeltaToWire(key: bigint, timestamp: bigint, replica: bigint): Uint8Array;

export function lwwMapSetDeltaToWire(key: bigint, timestamp: bigint, replica: bigint, value: bigint): Uint8Array;

export function lwwRegisterDeltaToWire(timestamp: bigint, replica: bigint, value: bigint): Uint8Array;
