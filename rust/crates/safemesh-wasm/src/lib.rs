// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_crdt::ownership::{allocate_token, WriterConfig};
use safemesh_crdt::{
    CollectionLimits, Crdt, DecodeError, DecodeLimits, EnableWinsFlag, EnableWinsFlagDelta,
    EventLog, GCounter, GCounterDelta, GSet, LwwMap, LwwMapDelta, LwwRegister, LwwRegisterDelta,
    OrSet, OrSetDelta, PnCounter, PnCounterDelta, Record, Replica, ReplicaError, Rga,
    VersionVector, VersionVectorLimits, WireDecode, WireEncode, WireError, WireSchema,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
// Installed during module initialization, before any consumer can call merge.
// The identity check must precede the generated call: that call borrows both
// arguments before entering the Rust method, so a Rust-body guard is too late.
export function installOrSetSelfMergeGuard(sample) {
    const prototype = Object.getPrototypeOf(sample);
    const merge = prototype.merge;
    prototype.merge = function(other) {
        if (this === other) return;
        return merge.call(this, other);
    };
    sample.free();
}

// Wrap the generated JS methods before wasm-bindgen converts a number to u32.
// All decode entry points take the optional budget as their second argument.
export function installCollectionBudgetGuard(sample) {
    const prototype = Object.getPrototypeOf(sample);
    const check = budget => {
        if (budget == null) return;
        if (typeof budget !== 'number' || !Number.isSafeInteger(budget) ||
            budget < 0 || budget > 4294967295) {
            throw new SafeMeshError(2,
                'maxCollectionElements must be a nonnegative integer at most 4294967295');
        }
    };
    for (const name of ['mergeStateBytes', 'mergeRecordBytes']) {
        if (typeof prototype[name] !== 'function') continue;
        const original = prototype[name];
        prototype[name] = function(bytes, budget) {
            check(budget);
            return original.call(this, bytes, budget);
        };
    }
    // A since batch takes the budgets its receiver's mergeLogBytes takes.
    for (const name of ['mergeLogBytes', 'sinceLogBytes']) {
        if (typeof prototype[name] !== 'function') continue;
        const original = prototype[name];
        prototype[name] = function(input, collectionBudget, recordBudget) {
            check(collectionBudget);
            if (recordBudget != null &&
                (typeof recordBudget !== 'number' || !Number.isSafeInteger(recordBudget) ||
                 recordBudget < 0 || recordBudget > 4294967295)) {
                throw new SafeMeshError(2,
                    'maxRecords must be a nonnegative integer at most 4294967295');
            }
            return original.call(this, input, collectionBudget, recordBudget);
        };
    }
    const klass = sample.constructor;
    if (typeof klass.importIdentity === 'function') {
        const original = klass.importIdentity;
        klass.importIdentity = function(bytes, recordBudget) {
            if (recordBudget != null &&
                (typeof recordBudget !== 'number' || !Number.isSafeInteger(recordBudget) ||
                 recordBudget < 0 || recordBudget > 4294967295)) {
                throw new SafeMeshError(2,
                    'maxRecords must be a nonnegative integer at most 4294967295');
            }
            return original.call(this, bytes, recordBudget);
        };
    }
    if (typeof klass.inspectRecordBytes === 'function') {
        const original = klass.inspectRecordBytes;
        klass.inspectRecordBytes = function(bytes, budget) {
            check(budget);
            return original.call(this, bytes, budget);
        };
    }
    sample.free();
}

export class SafeMeshError extends Error {
    constructor(code, message) {
        super(message);
        this.name = 'SafeMeshError';
        this.code = code;
    }
}

// Reject array-like inputs before wasm-bindgen's BigUint64Array copy can wrap
// their elements. The declared typed array already contains exact u64 values.
export function checkedTokens(value) {
    if (!(value instanceof BigUint64Array)) {
        throw new SafeMeshError(2, 'tokens must be a BigUint64Array');
    }
    return value;
}

// The same check for a peer version, named by its parameter.
export function checkedVersionPairs(value) {
    if (!(value instanceof BigUint64Array)) {
        throw new SafeMeshError(2,
            'peerVersion must be a BigUint64Array of (author, prefix) pairs');
    }
    return value;
}
"#)]
extern "C" {
    pub type SafeMeshError;

    #[wasm_bindgen(js_name = installOrSetSelfMergeGuard)]
    fn install_orset_self_merge_guard(sample: JsValue);

    #[wasm_bindgen(js_name = installCollectionBudgetGuard)]
    fn install_collection_budget_guard(sample: JsValue);

    #[wasm_bindgen(constructor)]
    fn new(code: u32, message: &str) -> SafeMeshError;

    #[wasm_bindgen(catch, js_name = checkedTokens)]
    fn checked_tokens(value: JsValue) -> Result<Vec<u64>, JsValue>;

    #[wasm_bindgen(catch, js_name = checkedVersionPairs)]
    fn checked_version_pairs(value: JsValue) -> Result<Vec<u64>, JsValue>;
}

// Exporting the sample uses wasm-bindgen's own class wrapper on every target.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn initialize_bindings() {
    install_orset_self_merge_guard(SafeMeshOrSet::new().into());
    install_collection_budget_guard(SafeMeshGCounterReplica::new(0, 1).into());
    install_collection_budget_guard(SafeMeshEnableWinsFlagReplica::new(0).into());
    install_collection_budget_guard(SafeMeshLwwMapReplica::new(0).into());
    install_collection_budget_guard(SafeMeshLwwRegisterReplica::new(0).into());
    install_collection_budget_guard(SafeMeshStringOrSetReplica::new(0).into());
    install_collection_budget_guard(SafeMeshPnCounterReplica::new(0, 1).into());
    install_collection_budget_guard(SafeMeshGSetReplica::new().into());
    install_collection_budget_guard(SafeMeshRgaReplica::new().into());
}

fn safe_mesh_error(code: u32, message: &str) -> JsValue {
    SafeMeshError::new(code, message).into()
}

fn wire_decode_error(error: WireError, context: &str) -> JsValue {
    match error {
        WireError::CollectionElementLimitExceeded { max_elements } => safe_mesh_error(
            3,
            &format!("maxCollectionElements limit exceeded: {max_elements}"),
        ),
        _ => safe_mesh_error(1, context),
    }
}

// Each decode path keeps one stable prefix and names the core `WireError` as
// its cause, so the same malformed bytes read the same on either path.
fn record_decode_js_error(error: WireError) -> JsValue {
    match error {
        WireError::CollectionElementLimitExceeded { .. } => {
            wire_decode_error(error, "failed to decode record")
        }
        cause => safe_mesh_error(1, &format!("failed to decode record: {cause}")),
    }
}

fn decode_limits(value: Option<u32>, max_records: Option<u32>) -> DecodeLimits {
    DecodeLimits {
        max_collection_elements: value.map(|value| value as usize),
        max_records: max_records.map(|value| value as usize),
    }
}

fn event_log_decode_js_error(error: DecodeError) -> JsValue {
    match error {
        DecodeError::Wire(WireError::OwnershipViolation) => safe_mesh_error(
            2,
            "counter coordinate out of range or not owned by record author",
        ),
        DecodeError::Wire(WireError::CollectionElementLimitExceeded { max_elements }) => {
            safe_mesh_error(
                3,
                &format!("maxCollectionElements limit exceeded: {max_elements}"),
            )
        }
        DecodeError::Wire(WireError::RecordCollision) => safe_mesh_error(1, "record ID collision"),
        DecodeError::Wire(WireError::ReplicaCountMismatch { .. }) => {
            safe_mesh_error(1, "replica count mismatch")
        }
        DecodeError::Wire(WireError::DeltaTypeMismatch) => {
            safe_mesh_error(1, "delta type mismatch")
        }
        DecodeError::Wire(WireError::MissingShape) => safe_mesh_error(1, "event log missing shape"),
        DecodeError::RecordLimitExceeded { max_records } => safe_mesh_error(
            1,
            &format!("failed to decode event log: RecordLimitExceeded: {max_records}"),
        ),
        DecodeError::Wire(cause) => {
            safe_mesh_error(1, &format!("failed to decode event log: {cause}"))
        }
    }
}

fn collection_limits(value: Option<u32>) -> CollectionLimits {
    CollectionLimits {
        max_elements: value
            .map(|value| value as usize)
            .or(CollectionLimits::WIRE_DEFAULT.max_elements),
    }
}

/// A counter total as an exact JavaScript `bigint`. wasm-bindgen carries `u64`
/// as `bigint` but has no `u128` mapping, so the total crosses as its decimal
/// text and is parsed by `BigInt`, which is unbounded; no digit is lost.
fn exact_bigint(total: u128) -> JsValue {
    JsValue::bigint_from_str(&total.to_string())
}

// Preserve the original JS value until its type and full range have been checked.
fn checked_u64(value: JsValue, name: &str) -> Result<u64, JsValue> {
    u64::try_from(value).map_err(|_| safe_mesh_error(2, &format!("{name} must be a u64 bigint")))
}

fn checked_index(value: JsValue, name: &str) -> Result<usize, JsValue> {
    let number = value.as_f64().ok_or_else(|| {
        safe_mesh_error(
            2,
            &format!("{name} must be a finite nonnegative integer number"),
        )
    })?;
    if !number.is_finite()
        || number.is_sign_negative()
        || number.fract() != 0.0
        || number > u32::MAX as f64
    {
        return Err(safe_mesh_error(
            2,
            &format!("{name} must be a finite nonnegative wasm32 integer"),
        ));
    }
    Ok(number as usize)
}

// Vec<u64> cannot address more than isize::MAX bytes on wasm32. Reject
// impossible capacities before allocation can trap and poison the instance.
fn checked_replica_count(value: JsValue) -> Result<usize, JsValue> {
    let replicas = checked_index(value, "replicas")?;
    if replicas > i32::MAX as usize / std::mem::size_of::<u64>() {
        return Err(safe_mesh_error(
            2,
            "replicas exceeds wasm32 counter capacity",
        ));
    }
    Ok(replicas)
}

fn admission_name(admission: safemesh_crdt::Admission) -> String {
    match admission {
        safemesh_crdt::Admission::Accepted => "accepted",
        safemesh_crdt::Admission::Duplicate => "duplicate",
        safemesh_crdt::Admission::Collision => "collision",
        safemesh_crdt::Admission::Invalid(_) => "invalid",
    }
    .to_owned()
}

// A single record reports the verdict `mergeLogBytes` reports for it: a
// duplicate or collision is a verdict, not an error. An invalid record throws.
fn record_verdict(admission: safemesh_crdt::Admission) -> Result<String, BindingError> {
    match admission {
        safemesh_crdt::Admission::Invalid(
            error @ (WireError::ZeroSequenceAdd { .. } | WireError::ZeroSequenceRemove { .. }),
        ) => Err(binding_error(1, error.to_string())),
        safemesh_crdt::Admission::Invalid(WireError::OwnershipViolation) => Err(binding_error(
            1,
            "writer replica does not match record author",
        )),
        safemesh_crdt::Admission::Invalid(_) => Err(binding_error(1, "invalid record")),
        admission => Ok(admission_name(admission)),
    }
}

fn append_error(error: safemesh_crdt::AppendError) -> BindingError {
    match error {
        safemesh_crdt::AppendError::SequenceExhausted => {
            binding_error(1, "event log sequence exhausted")
        }
        safemesh_crdt::AppendError::InvalidRecord(cause) => {
            record_verdict(safemesh_crdt::Admission::Invalid(cause)).unwrap_err()
        }
    }
}

// Record listing and since batches share one flat pair shape: `recordIds` is
// `[author, sequence, ...]` and a version is `[author, prefix, ...]`, where
// each prefix is the value `versionFor(author)` returns for that author.

/// Every admitted record ID in log order. Reads IDs only: no payload is
/// encoded, decoded or copied.
fn record_id_pairs<D>(log: &EventLog<D>) -> Vec<u64> {
    log.records()
        .iter()
        .flat_map(|record| [record.id.replica, record.id.sequence])
        .collect()
}

/// The core version's positive prefixes, sorted by author. An author whose
/// `versionFor` is zero has no entry, as in the core.
fn version_pairs<D>(log: &EventLog<D>) -> Vec<u64> {
    log.version()
        .entries()
        .iter()
        .flat_map(|(&author, &prefix)| [author, prefix])
        .collect()
}

/// Rebuild a peer version with the core's checked constructor and its wire
/// author budget. The pair shape carries no sequence-zero acknowledgement.
fn peer_version(pairs: &[u64]) -> Result<VersionVector, BindingError> {
    if !pairs.len().is_multiple_of(2) {
        return Err(binding_error(
            2,
            format!(
                "peerVersion must hold (author, prefix) pairs; got {} values",
                pairs.len()
            ),
        ));
    }
    let mut entries = BTreeMap::new();
    for pair in pairs.chunks_exact(2) {
        if entries.insert(pair[0], pair[1]).is_some() {
            return Err(binding_error(
                2,
                format!("peerVersion repeats author {}", pair[0]),
            ));
        }
    }
    VersionVector::from_peer_prefixes_with_limits(
        &entries,
        &BTreeSet::new(),
        VersionVectorLimits::WIRE_DEFAULT,
    )
    .map_err(|error| binding_error(2, format!("peerVersion: {error}")))
}

fn checked_peer_version(value: JsValue) -> Result<VersionVector, JsValue> {
    Ok(peer_version(&checked_version_pairs(value)?)?)
}

/// The core's `since` selection for `peer`, framed as `logBytes` frames a log.
/// The batch is then decoded under `limits` exactly as the receiver's
/// `mergeLogBytes` decodes it, so an over-budget batch is refused here.
fn since_log_bytes<C>(
    state: &C,
    log: &EventLog<C::Delta>,
    peer: &VersionVector,
    limits: DecodeLimits,
) -> Result<Vec<u8>, ReplicaError>
where
    C: Crdt,
    C::Delta: Clone + PartialEq + WireEncode + WireDecode + WireSchema,
{
    let mut bytes = Vec::new();
    EventLog::encode_records(log.replica_count(), &log.since(peer), &mut bytes)
        .map_err(ReplicaError::LogEncode)?;
    EventLog::records_from_wire_bytes_for_with_limits(&bytes, state, limits)
        .map_err(ReplicaError::LogDecode)?;
    Ok(bytes)
}

fn since_js_error(error: ReplicaError) -> JsValue {
    match error {
        ReplicaError::LogDecode(error) => event_log_decode_js_error(error),
        _ => safe_mesh_error(1, "failed to encode event log"),
    }
}

/// Record listing and since batches for a replica class with an event log.
/// `$parts` names the replica's core state and log.
macro_rules! record_exchange_methods {
    ($class:ty, |$this:ident| $parts:expr) => {
        #[wasm_bindgen]
        impl $class {
            /// Every record ID in log order, as `[author, sequence, ...]` pairs.
            /// Reads IDs only; no record payload is decoded.
            #[wasm_bindgen(js_name = recordIds)]
            pub fn record_ids(&self) -> Vec<u64> {
                let $this = self;
                record_id_pairs($parts.1)
            }

            /// `[author, versionFor(author), ...]` for every author with a
            /// nonzero prefix, sorted by author: a peer's `sinceLogBytes` input.
            #[wasm_bindgen(js_name = versionVector)]
            pub fn version_vector(&self) -> Vec<u64> {
                let $this = self;
                version_pairs($parts.1)
            }

            /// The records a peer at `peerVersion` is missing, as one log batch
            /// for its `mergeLogBytes`. `peerVersion` is `[author, prefix, ...]`
            /// as `versionVector` returns it; sequence-zero records are always
            /// included. The budgets are `mergeLogBytes`'s, in the same places,
            /// and a batch over them throws the error that merge would throw.
            #[wasm_bindgen(js_name = sinceLogBytes)]
            #[allow(non_snake_case)]
            pub fn since_log_bytes(
                &self,
                #[wasm_bindgen(unchecked_param_type = "BigUint64Array")] peerVersion: JsValue,
                max_collection_elements: Option<u32>,
                maxRecords: Option<u32>,
            ) -> Result<Vec<u8>, JsValue> {
                let peer = checked_peer_version(peerVersion)?;
                let $this = self;
                let (state, log) = $parts;
                since_log_bytes(
                    state,
                    log,
                    &peer,
                    decode_limits(max_collection_elements, maxRecords),
                )
                .map_err(since_js_error)
            }
        }
    };
}

record_exchange_methods!(SafeMeshGCounterReplica, |this| (
    this.replica.state(),
    this.replica.log()
));
record_exchange_methods!(SafeMeshEnableWinsFlagReplica, |this| (
    &this.state,
    &this.log
));
record_exchange_methods!(SafeMeshLwwMapReplica, |this| (&this.state, &this.log));
record_exchange_methods!(SafeMeshLwwRegisterReplica, |this| (&this.state, &this.log));
record_exchange_methods!(SafeMeshStringOrSetReplica, |this| (
    this.replica.state(),
    this.replica.log()
));
record_exchange_methods!(SafeMeshPnCounterReplica, |this| (&this.state, &this.log));

#[wasm_bindgen]
pub struct SafeMeshGCounter {
    inner: GCounter,
}

#[wasm_bindgen]
impl SafeMeshGCounter {
    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "number")] replicas: JsValue,
    ) -> Result<Self, JsValue> {
        let replicas = checked_replica_count(replicas)?;
        Ok(Self::new(replicas))
    }

    #[wasm_bindgen(js_name = applyBump)]
    pub fn apply_bump_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "number")] replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] tally: JsValue,
    ) -> Result<(), JsValue> {
        let replica = checked_index(replica, "replica")?;
        let tally = checked_u64(tally, "tally")?;
        self.try_apply_bump(replica, tally)
    }

    /// Apply a coordinate delta, throwing a descriptive Error for a bad index.
    #[wasm_bindgen(js_name = tryApplyBump)]
    pub fn try_apply_bump_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "number")] replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] tally: JsValue,
    ) -> Result<(), JsValue> {
        let replica = checked_index(replica, "replica")?;
        let tally = checked_u64(tally, "tally")?;
        self.try_apply_bump(replica, tally)
    }

    /// The counter total as an exact `bigint`, also past the 64-bit boundary.
    #[wasm_bindgen(unchecked_return_type = "bigint")]
    pub fn value(&self) -> JsValue {
        exact_bigint(self.total())
    }

    /// The Rust-core total behind [`Self::value`]; not exported, so host-side
    /// tests can read it without constructing a JavaScript value.
    fn total(&self) -> u128 {
        self.inner.value()
    }

    #[wasm_bindgen(js_name = state)]
    pub fn state(&self) -> Vec<u64> {
        self.inner.state().to_vec()
    }
}

#[wasm_bindgen(js_name = gcounterDeltaToWire)]
pub fn gcounter_delta_to_wire_js(
    #[wasm_bindgen(unchecked_param_type = "number")] replica: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] tally: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let replica = checked_index(replica, "replica")?;
    let tally = checked_u64(tally, "tally")?;
    gcounter_delta_to_wire(replica, tally)
}

#[wasm_bindgen]
#[derive(Default)]
pub struct SafeMeshLwwRegister {
    inner: LwwRegister<u64>,
}

#[wasm_bindgen]
impl SafeMeshLwwRegister {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        SafeMeshLwwRegister {
            inner: LwwRegister::new(),
        }
    }

    #[wasm_bindgen(js_name = set)]
    pub fn set_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
    ) -> Result<(), JsValue> {
        let timestamp = checked_u64(timestamp, "timestamp")?;
        let replica = checked_u64(replica, "replica")?;
        let value = checked_u64(value, "value")?;
        self.set(timestamp, replica, value);
        Ok(())
    }

    #[wasm_bindgen(js_name = hasValue)]
    pub fn has_value(&self) -> bool {
        self.inner.value().is_some()
    }

    #[wasm_bindgen(js_name = valueOr)]
    pub fn value_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.value_or(default_value))
    }

    #[wasm_bindgen(js_name = timestampOr)]
    pub fn timestamp_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.timestamp_or(default_value))
    }

    #[wasm_bindgen(js_name = writerReplicaOr)]
    pub fn writer_replica_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.writer_replica_or(default_value))
    }
}

#[wasm_bindgen(js_name = lwwRegisterDeltaToWire)]
pub fn lww_register_delta_to_wire_js(
    #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let timestamp = checked_u64(timestamp, "timestamp")?;
    let replica = checked_u64(replica, "replica")?;
    let value = checked_u64(value, "value")?;
    lww_register_delta_to_wire(timestamp, replica, value)
}

#[wasm_bindgen]
#[derive(Default)]
pub struct SafeMeshLwwMap {
    inner: LwwMap<u64, u64>,
}

#[wasm_bindgen]
impl SafeMeshLwwMap {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        SafeMeshLwwMap {
            inner: LwwMap::new(),
        }
    }

    #[wasm_bindgen(js_name = set)]
    pub fn set_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
    ) -> Result<(), JsValue> {
        let key = checked_u64(key, "key")?;
        let timestamp = checked_u64(timestamp, "timestamp")?;
        let replica = checked_u64(replica, "replica")?;
        let value = checked_u64(value, "value")?;
        self.set(key, timestamp, replica, value);
        Ok(())
    }

    #[wasm_bindgen(js_name = remove)]
    pub fn remove_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    ) -> Result<(), JsValue> {
        let key = checked_u64(key, "key")?;
        let timestamp = checked_u64(timestamp, "timestamp")?;
        let replica = checked_u64(replica, "replica")?;
        self.remove(key, timestamp, replica);
        Ok(())
    }

    #[wasm_bindgen(js_name = hasKey)]
    pub fn has_key_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
    ) -> Result<bool, JsValue> {
        let key = checked_u64(key, "key")?;
        Ok(self.has_key(key))
    }

    #[wasm_bindgen(js_name = valueOr)]
    pub fn value_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let key = checked_u64(key, "key")?;
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.value_or(key, default_value))
    }

    #[wasm_bindgen(js_name = visibleKeys)]
    pub fn visible_keys(&self) -> Vec<u64> {
        self.inner.value().keys().copied().collect()
    }

    #[wasm_bindgen(js_name = entryKeys)]
    pub fn entry_keys(&self) -> Vec<u64> {
        self.inner.entries().keys().copied().collect()
    }

    #[wasm_bindgen(js_name = removalKeys)]
    pub fn removal_keys(&self) -> Vec<u64> {
        self.inner.removals().keys().copied().collect()
    }
}

#[wasm_bindgen(js_name = lwwMapSetDeltaToWire)]
pub fn lww_map_set_delta_to_wire_js(
    #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let key = checked_u64(key, "key")?;
    let timestamp = checked_u64(timestamp, "timestamp")?;
    let replica = checked_u64(replica, "replica")?;
    let value = checked_u64(value, "value")?;
    lww_map_set_delta_to_wire(key, timestamp, replica, value)
}

#[wasm_bindgen(js_name = lwwMapRemoveDeltaToWire)]
pub fn lww_map_remove_delta_to_wire_js(
    #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
    #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let key = checked_u64(key, "key")?;
    let timestamp = checked_u64(timestamp, "timestamp")?;
    let replica = checked_u64(replica, "replica")?;
    lww_map_remove_delta_to_wire(key, timestamp, replica)
}

#[wasm_bindgen]
#[derive(Default)]
pub struct SafeMeshEnableWinsFlag {
    inner: EnableWinsFlag<u64>,
}

#[wasm_bindgen]
impl SafeMeshEnableWinsFlag {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        SafeMeshEnableWinsFlag {
            inner: EnableWinsFlag::new(),
        }
    }

    #[wasm_bindgen(js_name = enable)]
    pub fn enable_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] token: JsValue,
    ) -> Result<(), JsValue> {
        let token = checked_u64(token, "token")?;
        self.enable(token);
        Ok(())
    }

    #[wasm_bindgen(js_name = disableObserved)]
    pub fn disable_observed(&mut self) {
        let tokens = self.inner.observed_tokens();
        self.inner.disable(tokens);
    }

    pub fn value(&self) -> bool {
        self.inner.value()
    }

    #[wasm_bindgen(js_name = enabledTokens)]
    pub fn enabled_tokens(&self) -> Vec<u64> {
        self.inner.enables().iter().copied().collect()
    }

    #[wasm_bindgen(js_name = tombstoneTokens)]
    pub fn tombstone_tokens(&self) -> Vec<u64> {
        self.inner.tombstones().iter().copied().collect()
    }
}

#[wasm_bindgen(js_name = enableWinsFlagEnableDeltaToWire)]
pub fn enable_wins_flag_enable_delta_to_wire_js(
    #[wasm_bindgen(unchecked_param_type = "bigint")] token: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let token = checked_u64(token, "token")?;
    enable_wins_flag_enable_delta_to_wire(token)
}

#[wasm_bindgen(js_name = enableWinsFlagDisableDeltaToWire)]
pub fn enable_wins_flag_disable_delta_to_wire_js(
    #[wasm_bindgen(unchecked_param_type = "BigUint64Array")] tokens: JsValue,
) -> Result<Vec<u8>, JsValue> {
    let tokens = checked_tokens(tokens)?;
    enable_wins_flag_disable_delta_to_wire(tokens)
}

#[wasm_bindgen]
pub struct SafeMeshGCounterReplica {
    replica_id: u64,
    replica: Replica<GCounter>,
}

#[wasm_bindgen]
impl SafeMeshGCounterReplica {
    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica_id: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] replicas: JsValue,
    ) -> Result<Self, JsValue> {
        let replica_id = checked_u64(replica_id, "replica_id")?;
        let replicas = checked_replica_count(replicas)?;
        Ok(Self::new(replica_id, replicas))
    }

    #[wasm_bindgen(js_name = appendBump)]
    pub fn append_bump_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "number")] counter_replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] tally: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let counter_replica = checked_index(counter_replica, "counter_replica")?;
        let tally = checked_u64(tally, "tally")?;
        self.append_bump(counter_replica, tally)
    }

    /// Return the core admission verdict for the decoded input record, as
    /// `mergeLogBytes` does per record. Only `"accepted"` changes state.
    #[wasm_bindgen(
        js_name = mergeRecordBytes,
        unchecked_return_type = "\"accepted\" | \"duplicate\" | \"collision\""
    )]
    pub fn merge_record_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<String, JsValue> {
        let record = Replica::<GCounter>::inspect_record_bytes(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(|e| match e {
            ReplicaError::RecordDecode(e) => record_decode_js_error(e),
            _ => unreachable!(),
        })?;
        if safemesh_crdt::ownership::check_counter_record(
            self.replica.state().len(),
            record.id,
            &record.delta,
        )
        .is_err()
        {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        record_verdict(self.replica.admit(record)).map_err(JsValue::from)
    }

    /// Return one core admission verdict for every decoded input record.
    /// `maxRecords` is the third argument; the second limits collection elements.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    #[allow(non_snake_case)]
    pub fn merge_log_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
        maxRecords: Option<u32>,
    ) -> Result<Vec<String>, JsValue> {
        let log = self
            .replica
            .decode_log_bytes(bytes, decode_limits(max_collection_elements, maxRecords))
            .map_err(|e| match e {
                ReplicaError::LogDecode(e) => event_log_decode_js_error(e),
                _ => unreachable!(),
            })?;
        if log.iter().any(|r| {
            safemesh_crdt::ownership::check_counter_record(
                self.replica.state().len(),
                r.id,
                &r.delta,
            )
            .is_err()
        }) {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        Ok(log
            .iter()
            .cloned()
            .map(|record| self.replica.admit(record))
            .map(admission_name)
            .collect())
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.replica
            .log_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode event log"))
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    ) -> Result<u64, JsValue> {
        let replica = checked_u64(replica, "replica")?;
        Ok(self.version_for(replica))
    }

    /// The counter total as an exact `bigint`, also past the 64-bit boundary.
    #[wasm_bindgen(unchecked_return_type = "bigint")]
    pub fn value(&self) -> JsValue {
        exact_bigint(self.total())
    }

    /// The Rust-core total behind [`Self::value`]; not exported, so host-side
    /// tests can read it without constructing a JavaScript value.
    fn total(&self) -> u128 {
        self.replica.state().value()
    }

    /// Compare the Rust-core carrier states without reproducing its equality in JavaScript.
    #[wasm_bindgen(js_name = sameStateAs)]
    pub fn same_state_as(&self, other: &SafeMeshGCounterReplica) -> bool {
        self.replica.state() == other.replica.state()
    }

    #[wasm_bindgen(js_name = state)]
    pub fn state(&self) -> Vec<u64> {
        self.replica.state().state().to_vec()
    }
}

#[wasm_bindgen]
pub struct SafeMeshEnableWinsFlagReplica {
    replica_id: u64,
    state: EnableWinsFlag<u64>,
    log: EventLog<EnableWinsFlagDelta<u64>>,
}

#[wasm_bindgen]
impl SafeMeshEnableWinsFlagReplica {
    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica_id: JsValue,
    ) -> Result<Self, JsValue> {
        let replica_id = checked_u64(replica_id, "replica_id")?;
        Ok(Self::new(replica_id))
    }

    #[wasm_bindgen(js_name = appendEnable)]
    pub fn append_enable_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] token: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let token = checked_u64(token, "token")?;
        self.append_enable(token)
    }

    #[wasm_bindgen(js_name = appendDisableObserved)]
    pub fn append_disable_observed(&mut self) -> Result<Vec<u8>, JsValue> {
        let delta = EnableWinsFlagDelta::Disable {
            tokens: self.state.observed_tokens().into_iter().collect(),
        };
        let id = self
            .log
            .append_with(
                &mut self.state,
                self.replica_id,
                delta.clone(),
                |state, delta| {
                    state.apply_delta(delta.clone());
                },
            )
            .map_err(|error| JsValue::from(append_error(error)))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    /// Return the core admission verdict for the decoded input record, as
    /// `mergeLogBytes` does per record. Only `"accepted"` changes state.
    #[wasm_bindgen(
        js_name = mergeRecordBytes,
        unchecked_return_type = "\"accepted\" | \"duplicate\" | \"collision\""
    )]
    pub fn merge_record_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<String, JsValue> {
        let record = Record::<EnableWinsFlagDelta<u64>>::from_wire_bytes_with_collection_limits(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(record_decode_js_error)?;
        record_verdict(
            self.log
                .admit_with(&mut self.state, record, |state, delta| {
                    state.apply_delta(delta.clone());
                }),
        )
        .map_err(JsValue::from)
    }

    /// Return one core admission verdict for every decoded input record.
    /// `maxRecords` is the third argument; the second limits collection elements.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    #[allow(non_snake_case)]
    pub fn merge_log_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
        maxRecords: Option<u32>,
    ) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<EnableWinsFlagDelta<u64>>::records_from_wire_bytes_for_with_limits(
            bytes,
            &self.state,
            decode_limits(max_collection_elements, maxRecords),
        )
        .map_err(event_log_decode_js_error)?;
        Ok(log
            .iter()
            .cloned()
            .map(|record| {
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    })
            })
            .map(admission_name)
            .collect())
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.log
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode event log"))
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    ) -> Result<u64, JsValue> {
        let replica = checked_u64(replica, "replica")?;
        Ok(self.version_for(replica))
    }

    pub fn value(&self) -> bool {
        self.state.value()
    }

    #[wasm_bindgen(js_name = enabledTokens)]
    pub fn enabled_tokens(&self) -> Vec<u64> {
        self.state.enables().iter().copied().collect()
    }

    #[wasm_bindgen(js_name = tombstoneTokens)]
    pub fn tombstone_tokens(&self) -> Vec<u64> {
        self.state.tombstones().iter().copied().collect()
    }
}

#[wasm_bindgen]
pub struct SafeMeshLwwMapReplica {
    replica_id: u64,
    state: LwwMap<u64, u64>,
    log: EventLog<LwwMapDelta<u64, u64>>,
}

#[wasm_bindgen]
impl SafeMeshLwwMapReplica {
    /// Canonical complete carrier, including hidden entries and remove dots.
    #[wasm_bindgen(js_name = stateBytes)]
    pub fn state_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.state
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode state"))
    }

    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica_id: JsValue,
    ) -> Result<Self, JsValue> {
        let replica_id = checked_u64(replica_id, "replica_id")?;
        Ok(Self::new(replica_id))
    }

    #[wasm_bindgen(js_name = appendSet)]
    pub fn append_set_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] writer_replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let key = checked_u64(key, "key")?;
        let timestamp = checked_u64(timestamp, "timestamp")?;
        let writer_replica = checked_u64(writer_replica, "writer_replica")?;
        let value = checked_u64(value, "value")?;
        self.append_set(key, timestamp, writer_replica, value)
    }

    #[wasm_bindgen(js_name = appendRemove)]
    pub fn append_remove_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] writer_replica: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let key = checked_u64(key, "key")?;
        let timestamp = checked_u64(timestamp, "timestamp")?;
        let writer_replica = checked_u64(writer_replica, "writer_replica")?;
        self.append_remove(key, timestamp, writer_replica)
    }

    /// Return the core admission verdict for the decoded input record, as
    /// `mergeLogBytes` does per record. Only `"accepted"` changes state.
    #[wasm_bindgen(
        js_name = mergeRecordBytes,
        unchecked_return_type = "\"accepted\" | \"duplicate\" | \"collision\""
    )]
    pub fn merge_record_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<String, JsValue> {
        let record = Record::<LwwMapDelta<u64, u64>>::from_wire_bytes_with_collection_limits(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(record_decode_js_error)?;
        record_verdict(
            self.log
                .admit_with(&mut self.state, record, |state, delta| {
                    state.apply_delta(delta.clone());
                }),
        )
        .map_err(JsValue::from)
    }

    /// Return one core admission verdict for every decoded input record.
    /// `maxRecords` is the third argument; the second limits collection elements.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    #[allow(non_snake_case)]
    pub fn merge_log_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
        maxRecords: Option<u32>,
    ) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<LwwMapDelta<u64, u64>>::records_from_wire_bytes_for_with_limits(
            bytes,
            &self.state,
            decode_limits(max_collection_elements, maxRecords),
        )
        .map_err(event_log_decode_js_error)?;
        Ok(log
            .iter()
            .cloned()
            .map(|record| {
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    })
            })
            .map(admission_name)
            .collect())
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.log
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode event log"))
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    ) -> Result<u64, JsValue> {
        let replica = checked_u64(replica, "replica")?;
        Ok(self.version_for(replica))
    }

    #[wasm_bindgen(js_name = hasKey)]
    pub fn has_key_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
    ) -> Result<bool, JsValue> {
        let key = checked_u64(key, "key")?;
        Ok(self.has_key(key))
    }

    #[wasm_bindgen(js_name = valueOr)]
    pub fn value_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] key: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let key = checked_u64(key, "key")?;
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.value_or(key, default_value))
    }

    #[wasm_bindgen(js_name = visibleKeys)]
    pub fn visible_keys(&self) -> Vec<u64> {
        self.state.value().keys().copied().collect()
    }

    #[wasm_bindgen(js_name = entryKeys)]
    pub fn entry_keys(&self) -> Vec<u64> {
        self.state.entries().keys().copied().collect()
    }

    #[wasm_bindgen(js_name = removalKeys)]
    pub fn removal_keys(&self) -> Vec<u64> {
        self.state.removals().keys().copied().collect()
    }
}

#[wasm_bindgen]
pub struct SafeMeshLwwRegisterReplica {
    replica_id: u64,
    state: LwwRegister<u64>,
    log: EventLog<LwwRegisterDelta<u64>>,
}

#[wasm_bindgen]
impl SafeMeshLwwRegisterReplica {
    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica_id: JsValue,
    ) -> Result<Self, JsValue> {
        let replica_id = checked_u64(replica_id, "replica_id")?;
        Ok(Self::new(replica_id))
    }

    #[wasm_bindgen(js_name = appendSet)]
    pub fn append_set_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] timestamp: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] writer_replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let timestamp = checked_u64(timestamp, "timestamp")?;
        let writer_replica = checked_u64(writer_replica, "writer_replica")?;
        let value = checked_u64(value, "value")?;
        self.append_set(timestamp, writer_replica, value)
    }

    /// Return the core admission verdict for the decoded input record, as
    /// `mergeLogBytes` does per record. Only `"accepted"` changes state.
    #[wasm_bindgen(
        js_name = mergeRecordBytes,
        unchecked_return_type = "\"accepted\" | \"duplicate\" | \"collision\""
    )]
    pub fn merge_record_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<String, JsValue> {
        let record = Record::<LwwRegisterDelta<u64>>::from_wire_bytes_with_collection_limits(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(record_decode_js_error)?;
        record_verdict(
            self.log
                .admit_with(&mut self.state, record, |state, delta| {
                    state.apply_delta(delta.clone());
                }),
        )
        .map_err(JsValue::from)
    }

    /// Return one core admission verdict for every decoded input record.
    /// `maxRecords` is the third argument; the second limits collection elements.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    #[allow(non_snake_case)]
    pub fn merge_log_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
        maxRecords: Option<u32>,
    ) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<LwwRegisterDelta<u64>>::records_from_wire_bytes_for_with_limits(
            bytes,
            &self.state,
            decode_limits(max_collection_elements, maxRecords),
        )
        .map_err(event_log_decode_js_error)?;
        Ok(log
            .iter()
            .cloned()
            .map(|record| {
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    })
            })
            .map(admission_name)
            .collect())
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.log
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode event log"))
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    ) -> Result<u64, JsValue> {
        let replica = checked_u64(replica, "replica")?;
        Ok(self.version_for(replica))
    }

    #[wasm_bindgen(js_name = hasValue)]
    pub fn has_value(&self) -> bool {
        self.state.value().is_some()
    }

    #[wasm_bindgen(js_name = valueOr)]
    pub fn value_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.value_or(default_value))
    }

    #[wasm_bindgen(js_name = timestampOr)]
    pub fn timestamp_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.timestamp_or(default_value))
    }

    #[wasm_bindgen(js_name = writerReplicaOr)]
    pub fn writer_replica_or_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] default_value: JsValue,
    ) -> Result<u64, JsValue> {
        let default_value = checked_u64(default_value, "default_value")?;
        Ok(self.writer_replica_or(default_value))
    }
}

/// Observed-remove set of u64 elements and u64 tokens; tokens are global to the set.
#[wasm_bindgen]
#[derive(Default)]
pub struct SafeMeshOrSet {
    inner: OrSet<u64, u64>,
}

#[wasm_bindgen]
impl SafeMeshOrSet {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            inner: OrSet::new(),
        }
    }

    /// Add an element with a caller-supplied token, exactly as in the Rust core.
    #[wasm_bindgen(js_name = add)]
    pub fn add_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] element: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] token: JsValue,
    ) -> Result<(), JsValue> {
        let element = checked_u64(element, "element")?;
        let token = checked_u64(token, "token")?;
        self.add(element, token);
        Ok(())
    }
    /// Tombstone tokens globally, including tokens whose adds have not arrived yet.
    #[wasm_bindgen(js_name = applyRemove)]
    pub fn apply_remove_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "BigUint64Array")] tokens: JsValue,
    ) -> Result<(), JsValue> {
        let tokens = checked_tokens(tokens)?;
        self.apply_remove(tokens);
        Ok(())
    }
    #[wasm_bindgen(js_name = observedTokens)]
    pub fn observed_tokens_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] element: JsValue,
    ) -> Result<Vec<u64>, JsValue> {
        let element = checked_u64(element, "element")?;
        Ok(self.observed_tokens(element))
    }
    pub fn elements(&self) -> Vec<u64> {
        self.inner.elements().into_iter().collect()
    }
    #[wasm_bindgen(js_name = contains)]
    pub fn contains_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] element: JsValue,
    ) -> Result<bool, JsValue> {
        let element = checked_u64(element, "element")?;
        Ok(self.contains(element))
    }
    pub fn tombstones(&self) -> Vec<u64> {
        self.inner.tombstones().iter().copied().collect()
    }
    pub fn merge(&mut self, other: &SafeMeshOrSet) {
        self.inner.merge(&other.inner);
    }
}

/// Binding-level failure carried across the native/wasm boundary.
///
/// Native tests read the code and message directly; the wasm methods convert
/// it into a `SafeMeshError` with the same code and message.
#[derive(Clone, Debug, PartialEq, Eq)]
struct BindingError {
    code: u32,
    message: String,
}

impl From<BindingError> for JsValue {
    fn from(error: BindingError) -> JsValue {
        safe_mesh_error(error.code, &error.message)
    }
}

fn binding_error(code: u32, message: impl Into<String>) -> BindingError {
    BindingError {
        code,
        message: message.into(),
    }
}

fn event_log_decode_error(error: safemesh_crdt::WireError) -> BindingError {
    if let WireError::CollectionElementLimitExceeded { max_elements } = error {
        return binding_error(
            3,
            format!("maxCollectionElements limit exceeded: {max_elements}"),
        );
    }
    binding_error(
        1,
        match error {
            safemesh_crdt::WireError::RecordCollision => "record ID collision".to_string(),
            safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                "replica count mismatch".to_string()
            }
            safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch".to_string(),
            safemesh_crdt::WireError::MissingShape => "event log missing shape".to_string(),
            // Same text as the single-record path: it names the record and a recovery step.
            error @ (safemesh_crdt::WireError::ZeroSequenceAdd { .. }
            | safemesh_crdt::WireError::ZeroSequenceRemove { .. }) => error.to_string(),
            other => format!("failed to decode event log: {other}"),
        },
    )
}

fn bounded_event_log_decode_error(error: DecodeError) -> BindingError {
    match error {
        DecodeError::Wire(error) => event_log_decode_error(error),
        DecodeError::RecordLimitExceeded { max_records } => binding_error(
            1,
            format!("failed to decode event log: RecordLimitExceeded: {max_records}"),
        ),
    }
}

/// One `(element, token)` add pair as the core `OrSet` stores it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[wasm_bindgen]
pub struct SafeMeshStringOrSetAddEntry {
    element: String,
    token: u64,
}

#[wasm_bindgen]
impl SafeMeshStringOrSetAddEntry {
    pub fn element(&self) -> String {
        self.element.clone()
    }

    pub fn token(&self) -> u64 {
        self.token
    }
}

/// A record decoded by the core, exposed field by field so a consumer can label
/// record bytes without keeping its own metadata alongside them.
///
/// `deltaKind()` is `"add"` (then `element()` and `token()` are set, `tokens()`
/// is empty) or `"remove"` (then `tokens()` carries the tombstoned tokens and
/// `element()`/`token()` are undefined).
#[derive(Clone, Debug, PartialEq, Eq)]
#[wasm_bindgen]
pub struct SafeMeshStringOrSetRecord {
    id: safemesh_crdt::RecordId,
    delta: OrSetDelta<String, u64>,
}

#[wasm_bindgen]
impl SafeMeshStringOrSetRecord {
    pub fn replica(&self) -> u64 {
        self.id.replica
    }

    pub fn sequence(&self) -> u64 {
        self.id.sequence
    }

    #[wasm_bindgen(
        js_name = deltaKind,
        unchecked_return_type = "\"add\" | \"remove\""
    )]
    pub fn delta_kind(&self) -> String {
        match self.delta {
            OrSetDelta::Add { .. } => "add".to_string(),
            OrSetDelta::Remove { .. } => "remove".to_string(),
        }
    }

    pub fn element(&self) -> Option<String> {
        match &self.delta {
            OrSetDelta::Add { element, .. } => Some(element.clone()),
            OrSetDelta::Remove { .. } => None,
        }
    }

    pub fn token(&self) -> Option<u64> {
        match &self.delta {
            OrSetDelta::Add { token, .. } => Some(*token),
            OrSetDelta::Remove { .. } => None,
        }
    }

    pub fn tokens(&self) -> Vec<u64> {
        match &self.delta {
            OrSetDelta::Add { .. } => Vec::new(),
            OrSetDelta::Remove { tokens } => tokens.clone(),
        }
    }
}

/// Observed-remove set of UTF-8 string elements and u64 tokens, carried by an
/// event log so records can be replayed, deduplicated and repaired from a log.
///
/// Every value is computed by `safemesh_crdt::OrSet<String, u64>` and
/// `safemesh_crdt::EventLog`. The optional allocated lifecycle checks ownership
/// and holds a live-author claim within this WASM instance. Tokens remain global
/// to the set, exactly as in `SafeMeshOrSet`.
#[wasm_bindgen]
pub struct SafeMeshStringOrSetReplica {
    replica_id: u64,
    allocated_writers: Option<u64>,
    replica: Replica<OrSet<String, u64>>,
}

// Scoped to one WASM instance (one thread in native host tests). This is not
// cross-tab/process fencing. Legacy constructors deliberately remain unfenced.
thread_local! {
    static ALLOCATED_AUTHORS: RefCell<BTreeSet<u64>> = const { RefCell::new(BTreeSet::new()) };
}

impl Drop for SafeMeshStringOrSetReplica {
    fn drop(&mut self) {
        if self.allocated_writers.is_some() {
            ALLOCATED_AUTHORS.with(|authors| authors.borrow_mut().remove(&self.replica_id));
        }
    }
}

impl SafeMeshStringOrSetReplica {
    fn claim(&mut self, writers: u64) -> Result<(), BindingError> {
        WriterConfig {
            writers,
            writer: self.replica_id,
        }
        .validate()
        .map_err(|_| binding_error(2, "invalid writer configuration"))?;
        if !ALLOCATED_AUTHORS.with(|authors| authors.borrow_mut().insert(self.replica_id)) {
            return Err(binding_error(
                1,
                "author already has a live allocated writer",
            ));
        }
        self.allocated_writers = Some(writers);
        Ok(())
    }

    fn try_create_allocated(writers: u64, author: u64) -> Result<Self, BindingError> {
        let mut replica = Self::new(author);
        replica.claim(writers)?;
        Ok(replica)
    }

    fn check_owned_record(
        writers: u64,
        record: &Record<OrSetDelta<String, u64>>,
    ) -> Result<(), BindingError> {
        // The core OR-Set hook runs first, so a sequence-0 add reads the same
        // on allocated and legacy replicas, for one record or a whole log.
        OrSet::<String, u64>::new()
            .validate_record(record.id, &record.delta)
            .map_err(|error| binding_error(1, error.to_string()))?;
        if record.id.replica >= writers || record.id.sequence == 0 {
            return Err(binding_error(
                1,
                "allocation/history consistency: invalid record author or sequence",
            ));
        }
        if let OrSetDelta::Add { token, .. } = &record.delta {
            if allocate_token(writers, record.id.replica, record.id.sequence) != Some(*token) {
                return Err(binding_error(
                    1,
                    "allocation/history consistency: token mismatch",
                ));
            }
        }
        Ok(())
    }

    // Check every add, including tombstoned adds, and require a complete local
    // history. Peer histories may contain gaps during ordinary record exchange.
    fn checked_next(&self, writers: u64) -> Result<u64, BindingError> {
        WriterConfig {
            writers,
            writer: self.replica_id,
        }
        .validate()
        .map_err(|_| binding_error(2, "invalid writer configuration"))?;
        let mut last = 0;
        let mut count = 0;
        for record in self.replica.log().records() {
            Self::check_owned_record(writers, record)?;
            if record.id.replica == self.replica_id {
                last = last.max(record.id.sequence);
                count += 1;
            }
        }
        if count != last {
            return Err(binding_error(
                1,
                "allocation/history consistency: incomplete local history",
            ));
        }
        let next = last
            .checked_add(1)
            .ok_or_else(|| binding_error(1, "allocation sequence exhausted"))?;
        Ok(next)
    }

    fn checked_write_next(&self, writers: u64) -> Result<u64, BindingError> {
        let next = self.checked_next(writers)?;
        // Keep an exhausted but consistent identity exportable/restorable.
        // A write must leave its subsequent cursor representable as well.
        if next == u64::MAX {
            return Err(binding_error(1, "allocation sequence exhausted"));
        }
        Ok(next)
    }

    fn check_incoming(&self, record: &Record<OrSetDelta<String, u64>>) -> Result<(), BindingError> {
        if let Some(writers) = self.allocated_writers {
            Self::check_owned_record(writers, record)?;
            if record.id.replica == self.replica_id
                && !self
                    .replica
                    .log()
                    .records()
                    .iter()
                    .any(|known| known.id == record.id)
            {
                return Err(binding_error(1, "incoming record claims the local author"));
            }
        }
        Ok(())
    }

    fn try_append_allocated_add(&mut self, element: String) -> Result<Vec<u8>, BindingError> {
        let writers = self
            .allocated_writers
            .ok_or_else(|| binding_error(1, "replica has no allocated identity"))?;
        let sequence = self.checked_write_next(writers)?;
        let token = allocate_token(writers, self.replica_id, sequence)
            .ok_or_else(|| binding_error(1, "token allocation exhausted"))?;
        self.append(OrSetDelta::Add { element, token })
    }

    fn try_export_identity(&self) -> Result<Vec<u8>, BindingError> {
        let writers = self
            .allocated_writers
            .ok_or_else(|| binding_error(1, "replica has no allocated identity"))?;
        let next = self.checked_next(writers)?;
        // Local identity storage only, not a new CRDT transport encoding. The
        // suffix is the existing core log, including its shape/integrity checks.
        let mut bytes = b"SMOI\x01".to_vec();
        for word in [writers, self.replica_id, next] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend(self.replica.log_bytes().map_err(|error| {
            let ReplicaError::LogEncode(error) = error else {
                unreachable!()
            };
            binding_error(1, format!("failed to encode event log: {error:?}"))
        })?);
        Ok(bytes)
    }

    fn try_import_identity_with_limits(
        bytes: &[u8],
        max_records: Option<u32>,
    ) -> Result<Self, BindingError> {
        if bytes.len() < 29 || &bytes[..5] != b"SMOI\x01" {
            return Err(binding_error(
                1,
                "allocation/history consistency: invalid identity storage",
            ));
        }
        let word = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        let (writers, author, next) = (word(5), word(13), word(21));
        let mut candidate = Self::new(author);
        candidate.replica =
            Replica::restore(OrSet::new(), &bytes[29..], decode_limits(None, max_records))
                .map_err(|error| match error {
                    ReplicaError::LogDecode(error) => bounded_event_log_decode_error(error),
                    _ => unreachable!(),
                })?;
        if candidate.checked_next(writers)? != next {
            return Err(binding_error(
                1,
                "allocation/history consistency: next sequence mismatch",
            ));
        }
        // Claim only after all checks; a failed import creates no live writer.
        candidate.claim(writers)?;
        Ok(candidate)
    }

    fn append(&mut self, delta: OrSetDelta<String, u64>) -> Result<Vec<u8>, BindingError> {
        self.replica
            .append(self.replica_id, delta)
            .map_err(|_| binding_error(1, "event log sequence exhausted"))?
            .to_wire_bytes()
            .map_err(|error| binding_error(1, format!("failed to encode record: {error:?}")))
    }

    fn admit(&mut self, record: Record<OrSetDelta<String, u64>>) -> Result<String, BindingError> {
        self.check_incoming(&record)?;
        record_verdict(self.replica.admit(record))
    }

    #[cfg(test)]
    fn decode_record(bytes: &[u8]) -> Result<Record<OrSetDelta<String, u64>>, BindingError> {
        Self::decode_record_with_limits(bytes, None)
    }

    fn decode_record_with_limits(
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<Record<OrSetDelta<String, u64>>, BindingError> {
        Replica::<OrSet<String, u64>>::inspect_record_bytes(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(|error| {
            let ReplicaError::RecordDecode(error) = error else {
                unreachable!()
            };
            match error {
                WireError::CollectionElementLimitExceeded { max_elements } => binding_error(
                    3,
                    format!("maxCollectionElements limit exceeded: {max_elements}"),
                ),
                other => binding_error(1, format!("failed to decode record: {other}")),
            }
        })
    }

    #[cfg(test)]
    fn try_merge_record_bytes(&mut self, bytes: &[u8]) -> Result<String, BindingError> {
        self.try_merge_record_bytes_with_limits(bytes, None)
    }

    fn try_merge_record_bytes_with_limits(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<String, BindingError> {
        let record = Self::decode_record_with_limits(bytes, max_collection_elements)?;
        self.admit(record)
    }

    #[cfg(test)]
    fn try_merge_log_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>, BindingError> {
        self.try_merge_log_bytes_with_limits(bytes, None, None)
    }

    fn try_merge_log_bytes_with_limits(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
        max_records: Option<u32>,
    ) -> Result<Vec<String>, BindingError> {
        let log = self
            .replica
            .decode_log_bytes(bytes, decode_limits(max_collection_elements, max_records))
            .map_err(|error| match error {
                ReplicaError::LogDecode(error) => bounded_event_log_decode_error(error),
                _ => unreachable!(),
            })?;
        for record in &log {
            self.check_incoming(record)?;
        }
        Ok(log
            .iter()
            .cloned()
            .map(|record| self.replica.admit(record))
            .map(admission_name)
            .collect())
    }

    #[cfg(test)]
    fn try_inspect_record_bytes(bytes: &[u8]) -> Result<SafeMeshStringOrSetRecord, BindingError> {
        Self::try_inspect_record_bytes_with_limits(bytes, None)
    }

    fn try_inspect_record_bytes_with_limits(
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<SafeMeshStringOrSetRecord, BindingError> {
        let Record { id, delta } = Self::decode_record_with_limits(bytes, max_collection_elements)?;
        Ok(SafeMeshStringOrSetRecord { id, delta })
    }
}

#[wasm_bindgen]
impl SafeMeshStringOrSetReplica {
    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica_id: JsValue,
    ) -> Result<Self, JsValue> {
        let replica_id = checked_u64(replica_id, "replica_id")?;
        Ok(Self::new(replica_id))
    }

    /// Create an allocated writer. At most one allocated handle per author may
    /// live in this WASM instance; free() releases it. The caller provides any
    /// cross-instance/process exclusion and must not restore stale snapshots.
    #[wasm_bindgen(js_name = createAllocated)]
    pub fn create_allocated(
        #[wasm_bindgen(unchecked_param_type = "bigint")] writers: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] author: JsValue,
    ) -> Result<Self, JsValue> {
        let writers = u64::try_from(writers)
            .map_err(|_| safe_mesh_error(2, "writers must be a u64 bigint"))?;
        let author =
            u64::try_from(author).map_err(|_| safe_mesh_error(2, "author must be a u64 bigint"))?;
        Self::try_create_allocated(writers, author).map_err(JsValue::from)
    }

    /// Allocate through the Rust ownership rule, append, and return record bytes.
    #[wasm_bindgen(js_name = appendAllocatedAdd)]
    pub fn append_allocated_add(&mut self, element: String) -> Result<Vec<u8>, JsValue> {
        self.try_append_allocated_add(element)
            .map_err(JsValue::from)
    }

    /// Export fixed writer configuration, next sequence, and the complete log.
    /// The bytes are caller-persisted identity storage, not a transport packet.
    #[wasm_bindgen(js_name = exportIdentity)]
    pub fn export_identity(&self) -> Result<Vec<u8>, JsValue> {
        self.try_export_identity().map_err(JsValue::from)
    }

    /// Allocation/history consistency check; failure never creates a fresh writer.
    /// A self-consistent stale snapshot is not detected. There is no disk I/O.
    #[wasm_bindgen(js_name = importIdentity)]
    #[allow(non_snake_case)]
    pub fn import_identity(bytes: &[u8], maxRecords: Option<u32>) -> Result<Self, JsValue> {
        Self::try_import_identity_with_limits(bytes, maxRecords).map_err(JsValue::from)
    }

    /// Append an add record for `(element, token)` and return its wire bytes.
    /// Allocated instances reject caller tokens; use appendAllocatedAdd instead.
    #[wasm_bindgen(js_name = appendAdd)]
    pub fn append_add_js(
        &mut self,
        element: String,
        #[wasm_bindgen(unchecked_param_type = "bigint")] token: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let token = checked_u64(token, "token")?;
        self.append_add(element, token)
    }

    /// Append a remove record tombstoning every token this replica has observed
    /// for `element`, as the core reports them, and return its wire bytes.
    #[wasm_bindgen(js_name = appendRemoveObserved)]
    pub fn append_remove_observed(&mut self, element: String) -> Result<Vec<u8>, JsValue> {
        if let Some(writers) = self.allocated_writers {
            self.checked_write_next(writers).map_err(JsValue::from)?;
        }
        let tokens = self
            .replica
            .state()
            .observed_tokens(&element)
            .into_iter()
            .collect();
        self.append(OrSetDelta::Remove { tokens })
            .map_err(JsValue::from)
    }

    /// Decode one record and admit it through the core event log.
    ///
    /// Returns the core's admission verdict, as `mergeLogBytes` does per
    /// record: `"accepted"` when the record was new and applied, `"duplicate"`
    /// when a record with the same identity and payload was already in the log,
    /// and `"collision"` when the identity is known with a different payload.
    /// Only `"accepted"` changes state. Decode and ownership failures throw.
    #[wasm_bindgen(
        js_name = mergeRecordBytes,
        unchecked_return_type = "\"accepted\" | \"duplicate\" | \"collision\""
    )]
    pub fn merge_record_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<String, JsValue> {
        self.try_merge_record_bytes_with_limits(bytes, max_collection_elements)
            .map_err(JsValue::from)
    }

    /// Return one core admission verdict for every decoded input record.
    /// `maxRecords` is the third argument; the second limits collection elements.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    #[allow(non_snake_case)]
    pub fn merge_log_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
        maxRecords: Option<u32>,
    ) -> Result<Vec<String>, JsValue> {
        self.try_merge_log_bytes_with_limits(bytes, max_collection_elements, maxRecords)
            .map_err(JsValue::from)
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.replica.log_bytes().map_err(|error| {
            let ReplicaError::LogEncode(error) = error else {
                unreachable!()
            };
            safe_mesh_error(1, &format!("failed to encode event log: {error:?}"))
        })
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    ) -> Result<u64, JsValue> {
        let replica = checked_u64(replica, "replica")?;
        Ok(self.version_for(replica))
    }

    /// Live members, sorted and unique, as the core computes them.
    pub fn elements(&self) -> Vec<String> {
        self.replica.state().elements().into_iter().collect()
    }

    /// Live add tokens for `element`, excluding tombstoned tokens.
    #[wasm_bindgen(js_name = observedTokens)]
    pub fn observed_tokens(&self, element: String) -> Vec<u64> {
        self.replica
            .state()
            .observed_tokens(&element)
            .into_iter()
            .collect()
    }

    pub fn tombstones(&self) -> Vec<u64> {
        self.replica.state().tombstones().iter().copied().collect()
    }

    /// Every `(element, token)` add pair the core holds, tombstoned or not.
    #[wasm_bindgen(js_name = addEntries)]
    pub fn add_entries(&self) -> Vec<SafeMeshStringOrSetAddEntry> {
        self.replica
            .state()
            .adds()
            .iter()
            .map(|(element, token)| SafeMeshStringOrSetAddEntry {
                element: element.clone(),
                token: *token,
            })
            .collect()
    }

    /// Decode record bytes through the core without admitting them anywhere.
    ///
    /// Named after `mergeRecordBytes`: same input, but this only looks. It does
    /// not touch any replica, so it is static.
    #[wasm_bindgen(js_name = inspectRecordBytes)]
    pub fn inspect_record_bytes(
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<SafeMeshStringOrSetRecord, JsValue> {
        Self::try_inspect_record_bytes_with_limits(bytes, max_collection_elements)
            .map_err(JsValue::from)
    }
}

// Native entry points retain the Rust API used by host-side core integration tests.
// Only the checked *_js entry points above are exported to JavaScript.
impl SafeMeshGCounter {
    pub fn new(replicas: usize) -> Self {
        SafeMeshGCounter {
            inner: GCounter::new(replicas),
        }
    }

    pub fn apply_bump(&mut self, replica: usize, tally: u64) {
        self.inner.apply_bump(replica, tally);
    }

    pub fn try_apply_bump(&mut self, replica: usize, tally: u64) -> Result<(), JsValue> {
        self.inner
            .try_apply_bump(replica, tally)
            .map_err(|_| safe_mesh_error(2, "replica out of range"))
    }
}
pub fn gcounter_delta_to_wire(replica: usize, tally: u64) -> Result<Vec<u8>, JsValue> {
    GCounterDelta { replica, tally }
        .to_wire_bytes()
        .map_err(|_| safe_mesh_error(1, "failed to encode G-Counter delta"))
}

pub fn lww_register_delta_to_wire(
    timestamp: u64,
    replica: u64,
    value: u64,
) -> Result<Vec<u8>, JsValue> {
    LwwRegisterDelta {
        timestamp,
        replica,
        value,
    }
    .to_wire_bytes()
    .map_err(|_| safe_mesh_error(1, "failed to encode LWW register delta"))
}

pub fn lww_map_set_delta_to_wire(
    key: u64,
    timestamp: u64,
    replica: u64,
    value: u64,
) -> Result<Vec<u8>, JsValue> {
    LwwMapDelta::Set {
        key,
        timestamp,
        replica,
        value,
    }
    .to_wire_bytes()
    .map_err(|_| safe_mesh_error(1, "failed to encode LWW map set delta"))
}

pub fn lww_map_remove_delta_to_wire(
    key: u64,
    timestamp: u64,
    replica: u64,
) -> Result<Vec<u8>, JsValue> {
    LwwMapDelta::<u64, u64>::Remove {
        key,
        timestamp,
        replica,
    }
    .to_wire_bytes()
    .map_err(|_| safe_mesh_error(1, "failed to encode LWW map remove delta"))
}

pub fn enable_wins_flag_enable_delta_to_wire(token: u64) -> Result<Vec<u8>, JsValue> {
    EnableWinsFlagDelta::Enable { token }
        .to_wire_bytes()
        .map_err(|_| safe_mesh_error(1, "failed to encode enable-wins flag enable delta"))
}

pub fn enable_wins_flag_disable_delta_to_wire(tokens: Vec<u64>) -> Result<Vec<u8>, JsValue> {
    EnableWinsFlagDelta::Disable { tokens }
        .to_wire_bytes()
        .map_err(|_| safe_mesh_error(1, "failed to encode enable-wins flag disable delta"))
}
impl SafeMeshLwwRegister {
    pub fn set(&mut self, timestamp: u64, replica: u64, value: u64) {
        self.inner.set(timestamp, replica, value);
    }

    pub fn value_or(&self, default_value: u64) -> u64 {
        self.inner.value().copied().unwrap_or(default_value)
    }

    pub fn timestamp_or(&self, default_value: u64) -> u64 {
        self.inner
            .entry()
            .map(|entry| entry.dot.timestamp)
            .unwrap_or(default_value)
    }

    pub fn writer_replica_or(&self, default_value: u64) -> u64 {
        self.inner
            .entry()
            .map(|entry| entry.dot.replica)
            .unwrap_or(default_value)
    }
}
impl SafeMeshLwwMap {
    pub fn set(&mut self, key: u64, timestamp: u64, replica: u64, value: u64) {
        self.inner.set(key, timestamp, replica, value);
    }

    pub fn remove(&mut self, key: u64, timestamp: u64, replica: u64) {
        self.inner.remove(key, timestamp, replica);
    }

    pub fn has_key(&self, key: u64) -> bool {
        self.inner.get(&key).is_some()
    }

    pub fn value_or(&self, key: u64, default_value: u64) -> u64 {
        self.inner.get(&key).copied().unwrap_or(default_value)
    }
}
impl SafeMeshEnableWinsFlag {
    pub fn enable(&mut self, token: u64) {
        self.inner.enable(token);
    }
}
impl SafeMeshGCounterReplica {
    pub fn new(replica_id: u64, replicas: usize) -> Self {
        SafeMeshGCounterReplica {
            replica_id,
            replica: Replica::new(GCounter::new(replicas)),
        }
    }

    pub fn append_bump(&mut self, counter_replica: usize, tally: u64) -> Result<Vec<u8>, JsValue> {
        if safemesh_crdt::ownership::check_counter_record(
            self.replica.state().len(),
            safemesh_crdt::RecordId {
                replica: self.replica_id,
                sequence: 1,
            },
            &GCounterDelta {
                replica: counter_replica,
                tally,
            },
        )
        .is_err()
        {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        let delta = GCounterDelta {
            replica: counter_replica,
            tally,
        };
        self.replica
            .append(self.replica_id, delta)
            .map_err(|_| safe_mesh_error(1, "event log sequence exhausted"))?
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.replica.version().get(replica)
    }
}
impl SafeMeshEnableWinsFlagReplica {
    pub fn new(replica_id: u64) -> Self {
        SafeMeshEnableWinsFlagReplica {
            replica_id,
            state: EnableWinsFlag::new(),
            log: EventLog::new(),
        }
    }

    pub fn append_enable(&mut self, token: u64) -> Result<Vec<u8>, JsValue> {
        let delta = EnableWinsFlagDelta::Enable { token };
        let id = self
            .log
            .append_with(
                &mut self.state,
                self.replica_id,
                delta.clone(),
                |state, delta| {
                    state.apply_delta(delta.clone());
                },
            )
            .map_err(|error| JsValue::from(append_error(error)))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }
}
impl SafeMeshLwwMapReplica {
    pub fn new(replica_id: u64) -> Self {
        SafeMeshLwwMapReplica {
            replica_id,
            state: LwwMap::new(),
            log: EventLog::new(),
        }
    }

    pub fn append_set(
        &mut self,
        key: u64,
        timestamp: u64,
        writer_replica: u64,
        value: u64,
    ) -> Result<Vec<u8>, JsValue> {
        let delta = LwwMapDelta::Set {
            key,
            timestamp,
            replica: writer_replica,
            value,
        };
        let id = self
            .log
            .append_with(
                &mut self.state,
                self.replica_id,
                delta.clone(),
                |state, delta| {
                    state.apply_delta(delta.clone());
                },
            )
            .map_err(|error| JsValue::from(append_error(error)))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn append_remove(
        &mut self,
        key: u64,
        timestamp: u64,
        writer_replica: u64,
    ) -> Result<Vec<u8>, JsValue> {
        let delta = LwwMapDelta::Remove {
            key,
            timestamp,
            replica: writer_replica,
        };
        let id = self
            .log
            .append_with(
                &mut self.state,
                self.replica_id,
                delta.clone(),
                |state, delta| {
                    state.apply_delta(delta.clone());
                },
            )
            .map_err(|error| JsValue::from(append_error(error)))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }

    pub fn has_key(&self, key: u64) -> bool {
        self.state.get(&key).is_some()
    }

    pub fn value_or(&self, key: u64, default_value: u64) -> u64 {
        self.state.get(&key).copied().unwrap_or(default_value)
    }
}
impl SafeMeshLwwRegisterReplica {
    pub fn new(replica_id: u64) -> Self {
        SafeMeshLwwRegisterReplica {
            replica_id,
            state: LwwRegister::new(),
            log: EventLog::new(),
        }
    }

    pub fn append_set(
        &mut self,
        timestamp: u64,
        writer_replica: u64,
        value: u64,
    ) -> Result<Vec<u8>, JsValue> {
        let delta = LwwRegisterDelta {
            timestamp,
            replica: writer_replica,
            value,
        };
        let id = self
            .log
            .append_with(
                &mut self.state,
                self.replica_id,
                delta.clone(),
                |state, delta| {
                    state.apply_delta(delta.clone());
                },
            )
            .map_err(|error| JsValue::from(append_error(error)))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }

    pub fn value_or(&self, default_value: u64) -> u64 {
        self.state.value().copied().unwrap_or(default_value)
    }

    pub fn timestamp_or(&self, default_value: u64) -> u64 {
        self.state
            .entry()
            .map(|entry| entry.dot.timestamp)
            .unwrap_or(default_value)
    }

    pub fn writer_replica_or(&self, default_value: u64) -> u64 {
        self.state
            .entry()
            .map(|entry| entry.dot.replica)
            .unwrap_or(default_value)
    }
}
impl SafeMeshOrSet {
    pub fn add(&mut self, element: u64, token: u64) {
        self.inner.add(element, token);
    }

    pub fn apply_remove(&mut self, tokens: Vec<u64>) {
        self.inner.apply_remove(tokens);
    }

    pub fn observed_tokens(&self, element: u64) -> Vec<u64> {
        self.inner.observed_tokens(&element).into_iter().collect()
    }

    pub fn contains(&self, element: u64) -> bool {
        self.inner.contains(&element)
    }
}
impl SafeMeshStringOrSetReplica {
    pub fn new(replica_id: u64) -> Self {
        SafeMeshStringOrSetReplica {
            replica_id,
            allocated_writers: None,
            replica: Replica::new(OrSet::new()),
        }
    }

    pub fn append_add(&mut self, element: String, token: u64) -> Result<Vec<u8>, JsValue> {
        if self.allocated_writers.is_some() {
            return Err(safe_mesh_error(
                1,
                "allocated replica rejects caller-supplied tokens; use appendAllocatedAdd",
            ));
        }
        self.append(OrSetDelta::Add { element, token })
            .map_err(JsValue::from)
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.replica.version().get(replica)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use safemesh_crdt::{RecordId, WireEncode};

    // Explicit pre-migration EventLog + state oracle from main 7bb4e9a7.
    // Keep binding preflight independent of the migrated wrapper.
    struct LegacyStringOrSet {
        replica_id: u64,
        allocated_writers: Option<u64>,
        state: OrSet<String, u64>,
        log: EventLog<OrSetDelta<String, u64>>,
    }

    impl LegacyStringOrSet {
        fn new(replica_id: u64, allocated_writers: Option<u64>) -> Self {
            Self {
                replica_id,
                allocated_writers,
                state: OrSet::new(),
                log: EventLog::new(),
            }
        }
        fn check_owned_record(
            writers: u64,
            record: &Record<OrSetDelta<String, u64>>,
        ) -> Result<(), BindingError> {
            // The core OR-Set hook runs first, so a sequence-0 add reads the same
            // on allocated and legacy replicas, for one record or a whole log.
            OrSet::<String, u64>::new()
                .validate_record(record.id, &record.delta)
                .map_err(|error| binding_error(1, error.to_string()))?;
            if record.id.replica >= writers || record.id.sequence == 0 {
                return Err(binding_error(
                    1,
                    "allocation/history consistency: invalid record author or sequence",
                ));
            }
            if let OrSetDelta::Add { token, .. } = &record.delta {
                if allocate_token(writers, record.id.replica, record.id.sequence) != Some(*token) {
                    return Err(binding_error(
                        1,
                        "allocation/history consistency: token mismatch",
                    ));
                }
            }
            Ok(())
        }

        // Check every add, including tombstoned adds, and require a complete local
        // history. Peer histories may contain gaps during ordinary record exchange.
        fn checked_next(&self, writers: u64) -> Result<u64, BindingError> {
            WriterConfig {
                writers,
                writer: self.replica_id,
            }
            .validate()
            .map_err(|_| binding_error(2, "invalid writer configuration"))?;
            let mut last = 0;
            let mut count = 0;
            for record in self.log.records() {
                Self::check_owned_record(writers, record)?;
                if record.id.replica == self.replica_id {
                    last = last.max(record.id.sequence);
                    count += 1;
                }
            }
            if count != last {
                return Err(binding_error(
                    1,
                    "allocation/history consistency: incomplete local history",
                ));
            }
            let next = last
                .checked_add(1)
                .ok_or_else(|| binding_error(1, "allocation sequence exhausted"))?;
            Ok(next)
        }

        fn check_incoming(
            &self,
            record: &Record<OrSetDelta<String, u64>>,
        ) -> Result<(), BindingError> {
            if let Some(writers) = self.allocated_writers {
                Self::check_owned_record(writers, record)?;
                if record.id.replica == self.replica_id
                    && !self.log.records().iter().any(|known| known.id == record.id)
                {
                    return Err(binding_error(1, "incoming record claims the local author"));
                }
            }
            Ok(())
        }

        fn append(&mut self, delta: OrSetDelta<String, u64>) -> Vec<u8> {
            let id = self
                .log
                .append_with(
                    &mut self.state,
                    self.replica_id,
                    delta.clone(),
                    |state, delta| state.apply_delta(delta.clone()),
                )
                .unwrap();
            Record { id, delta }.to_wire_bytes().unwrap()
        }
        fn merge(
            &mut self,
            bytes: &[u8],
            max_records: Option<u32>,
        ) -> Result<Vec<String>, BindingError> {
            let records =
                EventLog::<OrSetDelta<String, u64>>::records_from_wire_bytes_for_with_limits(
                    bytes,
                    &self.state,
                    decode_limits(None, max_records),
                )
                .map_err(bounded_event_log_decode_error)?;
            for record in &records {
                self.check_incoming(record)?;
            }
            Ok(records
                .into_iter()
                .map(|record| {
                    admission_name(
                        self.log
                            .admit_with(&mut self.state, record, |state, delta| {
                                state.apply_delta(delta.clone())
                            }),
                    )
                })
                .collect())
        }
        fn identity(&self) -> Vec<u8> {
            let writers = self.allocated_writers.unwrap();
            let mut bytes = b"SMOI\x01".to_vec();
            for word in [
                writers,
                self.replica_id,
                self.checked_next(writers).unwrap(),
            ] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            bytes.extend(self.log.to_wire_bytes().unwrap());
            bytes
        }
        fn assert_same(&self, actual: &SafeMeshStringOrSetReplica) {
            assert_eq!(&self.state, actual.replica.state());
            assert_eq!(&self.log, actual.replica.log());
            assert_eq!(self.log.version(), actual.replica.version());
            assert_eq!(
                self.log.to_wire_bytes().unwrap(),
                actual.log_bytes().unwrap()
            );
            if self.allocated_writers.is_some() {
                assert_eq!(self.identity(), actual.try_export_identity().unwrap());
            }
        }
    }

    #[test]
    fn allocated_identity_checks_history_and_restarts() {
        let mut left = SafeMeshStringOrSetReplica::try_create_allocated(2, 0).unwrap();
        left.try_append_allocated_add("water".into()).unwrap();
        let mut old = LegacyStringOrSet::new(0, Some(2));
        old.append(OrSetDelta::Add {
            element: "water".into(),
            token: 2,
        });
        old.assert_same(&left);
        let saved = left.try_export_identity().unwrap();
        assert!(SafeMeshStringOrSetReplica::try_import_identity_with_limits(&saved, None).is_err());
        drop(left);
        for (offset, word) in [(5, 0u64), (5, 3), (13, 1), (21, 0), (21, u64::MAX)] {
            let mut bad = saved.clone();
            bad[offset..offset + 8].copy_from_slice(&word.to_le_bytes());
            assert!(
                SafeMeshStringOrSetReplica::try_import_identity_with_limits(&bad, None).is_err()
            );
        }
        let mut restored =
            SafeMeshStringOrSetReplica::try_import_identity_with_limits(&saved, None).unwrap();
        old.assert_same(&restored);
        let expected = old.append(OrSetDelta::Add {
            element: "radio".into(),
            token: 4,
        });
        let next = restored.try_append_allocated_add("radio".into()).unwrap();
        let record = SafeMeshStringOrSetReplica::decode_record(&next).unwrap();
        assert_eq!(record.id.sequence, 2);
        assert_eq!(
            record.delta,
            OrSetDelta::Add {
                element: "radio".into(),
                token: 4
            }
        );
        assert_eq!(next, expected);
        old.assert_same(&restored);
        let log = restored.log_bytes().unwrap();
        let mut peer = SafeMeshStringOrSetReplica::try_create_allocated(2, 1).unwrap();
        let mut old_peer = LegacyStringOrSet::new(1, Some(2));
        for _ in 0..2 {
            let expected = old_peer.merge(&log, None).unwrap();
            let actual = peer.try_merge_log_bytes(&log).unwrap();
            assert_eq!(actual, expected);
            old_peer.assert_same(&peer);
        }
    }

    #[test]
    fn identity_budget_refuses_before_claim_then_allows_retry_and_append() {
        let mut writer = SafeMeshStringOrSetReplica::try_create_allocated(2, 0).unwrap();
        LegacyStringOrSet::new(0, Some(2)).assert_same(&writer);
        for element in ["first", "second", "third"] {
            writer.try_append_allocated_add(element.into()).unwrap();
        }
        let saved = writer.try_export_identity().unwrap();
        let mut old = LegacyStringOrSet::new(0, Some(2));
        for (element, token) in [("first", 2), ("second", 4), ("third", 6)] {
            old.append(OrSetDelta::Add {
                element: element.into(),
                token,
            });
        }
        old.assert_same(&writer);
        // With an already-live author, budget failure must still win over claim.
        let error =
            match SafeMeshStringOrSetReplica::try_import_identity_with_limits(&saved, Some(2)) {
                Err(error) => error,
                Ok(_) => panic!("over-budget live identity imported"),
            };
        assert_eq!(
            (error.code, error.message.as_str()),
            (1, "failed to decode event log: RecordLimitExceeded: 2")
        );
        assert!(ALLOCATED_AUTHORS.with(|authors| authors.borrow().contains(&0)));
        drop(writer);
        let Err(error) =
            SafeMeshStringOrSetReplica::try_import_identity_with_limits(&saved, Some(2))
        else {
            panic!("over-budget identity imported");
        };
        assert_eq!(error.code, 1);
        assert_eq!(
            error.message,
            "failed to decode event log: RecordLimitExceeded: 2"
        );
        assert!(!ALLOCATED_AUTHORS.with(|authors| authors.borrow().contains(&0)));
        let mut restored =
            SafeMeshStringOrSetReplica::try_import_identity_with_limits(&saved, Some(3)).unwrap();
        old.assert_same(&restored);
        assert_eq!(restored.elements(), vec!["first", "second", "third"]);
        assert!(ALLOCATED_AUTHORS.with(|authors| authors.borrow().contains(&0)));
        let expected = old.append(OrSetDelta::Add {
            element: "fourth".into(),
            token: 8,
        });
        let fourth = restored.try_append_allocated_add("fourth".into()).unwrap();
        assert_eq!(
            SafeMeshStringOrSetReplica::decode_record(&fourth)
                .unwrap()
                .id
                .sequence,
            4
        );
        assert_eq!(fourth, expected);
        old.assert_same(&restored);
    }

    #[test]
    fn legacy_event_log_fixtures_keep_the_core_text() {
        use safemesh_crdt::LegacyFrame;
        let cases: [(LegacyFrame, &[u8]); 2] = [
            (
                LegacyFrame::Tag02,
                include_bytes!("../../safemesh-crdt/tests/fixtures/legacy-event-log/tag02/orset-utf8.log"),
            ),
            (
                LegacyFrame::Tag03Unshaped,
                include_bytes!(
                    "../../safemesh-crdt/tests/fixtures/legacy-event-log/tag03-unshaped/orset-utf8.log"
                ),
            ),
        ];
        for (found, bytes) in cases {
            let expected = format!(
                "failed to decode event log: {}",
                WireError::LegacyEventLogFrame { found }
            );
            let mut replica = SafeMeshStringOrSetReplica::new(0);
            for error in [
                replica.try_merge_log_bytes(bytes).unwrap_err(),
                replica
                    .try_merge_log_bytes_with_limits(bytes, Some(4096), None)
                    .unwrap_err(),
            ] {
                assert_eq!((error.code, error.message.as_str()), (1, expected.as_str()));
            }
            assert!(replica.replica.log().records().is_empty());
            // A saved identity whose history is a legacy frame names it too.
            let mut identity = b"SMOI\x01".to_vec();
            for word in [2u64, 0, 1] {
                identity.extend_from_slice(&word.to_le_bytes());
            }
            identity.extend_from_slice(bytes);
            let Err(error) =
                SafeMeshStringOrSetReplica::try_import_identity_with_limits(&identity, None)
            else {
                panic!("legacy identity history imported");
            };
            assert_eq!((error.code, error.message.as_str()), (1, expected.as_str()));
            println!(
                "WASM {found:?}: code={} message={}",
                error.code, error.message
            );
        }
    }

    #[test]
    fn allocated_history_refuses_gaps_zero_and_max_sequence() {
        for sequence in [0, 2, u64::MAX] {
            let mut replica = SafeMeshStringOrSetReplica::new(0);
            let admission = replica.replica.admit(Record {
                id: RecordId {
                    replica: 0,
                    sequence,
                },
                delta: OrSetDelta::Remove { tokens: vec![] },
            });
            if sequence == 0 {
                assert_eq!(
                    admission,
                    safemesh_crdt::Admission::Invalid(WireError::ZeroSequenceRemove { replica: 0 })
                );
                assert!(replica.replica.log().records().is_empty());
            } else {
                assert_eq!(admission, safemesh_crdt::Admission::Accepted);
                assert!(replica.checked_next(1).is_err());
            }
        }
        assert_eq!(allocate_token(1, 0, u64::MAX), Some(u64::MAX));
        assert_eq!(allocate_token(2, 0, u64::MAX), None);
        assert_eq!(allocate_token(0, 0, 1), None);
        assert_eq!(allocate_token(2, 2, 1), None);
        assert_eq!(allocate_token(2, 0, 0), None);
    }

    #[test]
    fn orset_matches_core_with_concurrent_add_and_early_tombstone() {
        let mut left = SafeMeshOrSet::new();
        let mut right = SafeMeshOrSet::new();
        let mut core_left = OrSet::<u64, u64>::new();
        let mut core_right = OrSet::<u64, u64>::new();
        for (element, token) in [(10, 101), (20, 102)] {
            left.add(element, token);
            core_left.add(element, token);
        }
        right.merge(&left);
        core_right.merge(&core_left);
        for (element, token) in [(10, 201), (30, 203), (40, 999), (50, 999)] {
            right.add(element, token);
            core_right.add(element, token);
        }
        for element in [10, 20] {
            left.apply_remove(left.observed_tokens(element));
            core_left.apply_remove(core_left.observed_tokens(&element));
        }
        left.apply_remove(vec![999]);
        core_left.apply_remove([999]);
        left.merge(&right);
        core_left.merge(&core_right);
        left.merge(&right);
        core_left.merge(&core_right);
        let expected: Vec<_> = core_left.elements().into_iter().collect();
        println!("wasm OR-Set={:?} Rust core={expected:?}", left.elements());
        assert_eq!(left.elements(), expected);
        assert_eq!(left.elements(), vec![10, 30]);
        assert_eq!(
            left.observed_tokens(10),
            core_left
                .observed_tokens(&10)
                .into_iter()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            left.tombstones(),
            core_left.tombstones().iter().copied().collect::<Vec<_>>()
        );
    }

    #[test]
    fn wasm_counter_calls_rust_core() {
        let mut counter = SafeMeshGCounter::new(3);
        counter.apply_bump(1, 5);
        counter.apply_bump(1, 2);
        assert_eq!(counter.total(), 5);
        assert_eq!(counter.state(), vec![0, 5, 0]);
    }

    #[test]
    fn wasm_wire_helper_uses_canonical_bytes() {
        let bytes = gcounter_delta_to_wire(2, 7).unwrap();
        assert_eq!(bytes.len(), 17);
        assert_eq!(bytes[0], 0x10);
    }

    #[test]
    fn wasm_replicas_exchange_canonical_record_bytes() {
        let mut left = SafeMeshGCounterReplica::new(1, 3);
        let mut right = SafeMeshGCounterReplica::new(2, 3);

        let bytes = left.append_bump(1, 5).unwrap();
        let expected = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: 5,
            },
        }
        .to_wire_bytes()
        .unwrap();
        assert_eq!(bytes, expected);

        right.merge_record_bytes(&bytes, None).unwrap();
        right.merge_record_bytes(&bytes, None).unwrap();
        assert_eq!(right.total(), 5);
        assert_eq!(right.version_for(1), 1);

        left.merge_log_bytes(&right.log_bytes().unwrap(), None, None)
            .unwrap();
        assert_eq!(left.total(), right.total());
    }

    #[test]
    fn wasm_batch_reports_every_admission() {
        fn wire(records: impl IntoIterator<Item = Record<GCounterDelta>>) -> Vec<u8> {
            let mut log = EventLog::with_replica_count(2);
            for record in records {
                assert_eq!(
                    log.insert_record(&GCounter::new(2), record),
                    safemesh_crdt::Admission::Accepted
                );
            }
            log.to_wire_bytes().unwrap()
        }

        let existing = Record {
            id: RecordId {
                replica: 0,
                sequence: 1,
            },
            delta: GCounterDelta {
                replica: 0,
                tally: 5,
            },
        };
        let accepted = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: 7,
            },
        };
        let collision = Record {
            id: existing.id,
            delta: GCounterDelta {
                replica: 0,
                tally: 9,
            },
        };
        let after_collision = Record {
            id: RecordId {
                replica: 1,
                sequence: 2,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: 8,
            },
        };

        let mut target = SafeMeshGCounterReplica::new(0, 2);
        assert_eq!(
            target
                .merge_record_bytes(&existing.to_wire_bytes().unwrap(), None)
                .unwrap(),
            "accepted"
        );
        assert_eq!(
            target.merge_log_bytes(&wire([]), None, None).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(target.state(), vec![5, 0]);

        let mut one = SafeMeshGCounterReplica::new(0, 2);
        assert_eq!(
            one.merge_log_bytes(&wire([accepted.clone()]), None, None)
                .unwrap(),
            vec!["accepted"]
        );
        assert_eq!(one.state(), vec![0, 7]);

        // The single-record path reports the collision the batch reports.
        let before = target.state();
        assert_eq!(
            target
                .merge_record_bytes(&collision.to_wire_bytes().unwrap(), None)
                .unwrap(),
            "collision"
        );
        assert_eq!(
            target
                .merge_log_bytes(&wire([collision.clone()]), None, None)
                .unwrap(),
            vec!["collision"]
        );
        assert_eq!(target.state(), before);

        assert_eq!(
            target
                .merge_record_bytes(&accepted.to_wire_bytes().unwrap(), None)
                .unwrap(),
            "accepted"
        );
        let before = target.state();
        for record in [&existing, &accepted] {
            assert_eq!(
                target
                    .merge_record_bytes(&record.to_wire_bytes().unwrap(), None)
                    .unwrap(),
                "duplicate"
            );
        }
        assert_eq!(
            target
                .merge_log_bytes(&wire([existing.clone(), accepted.clone()]), None, None)
                .unwrap(),
            vec!["duplicate", "duplicate"]
        );
        assert_eq!(target.state(), before);

        let mut late = SafeMeshGCounterReplica::new(0, 2);
        assert_eq!(
            late.merge_record_bytes(&existing.to_wire_bytes().unwrap(), None)
                .unwrap(),
            "accepted"
        );
        let before = late.state();
        let admissions = late
            .merge_log_bytes(&wire([accepted, collision, after_collision]), None, None)
            .unwrap();
        let after = late.state();
        println!("WASM admissions={admissions:?} before_state={before:?} after_state={after:?}");
        assert_eq!(admissions, vec!["accepted", "collision", "accepted"]);
        assert_eq!(before, vec![5, 0]);
        assert_eq!(after, vec![5, 8]);
    }

    #[test]
    fn wasm_all_replica_batches_report_collisions() {
        macro_rules! view {
            ($replica:ident) => {
                (&$replica.state, &$replica.log)
            };
            ($replica:ident, core) => {
                ($replica.replica.state(), $replica.replica.log())
            };
        }
        macro_rules! check {
            ($replica:expr, $first:expr, $second:expr $(, $core:ident)?) => {{
                let first = Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 1,
                    },
                    delta: $first,
                };
                let second = Record {
                    id: first.id,
                    delta: $second,
                };
                let mut replica = $replica;
                replica
                    .merge_record_bytes(&first.to_wire_bytes().unwrap(), None)
                    .unwrap();
                let state = view!(replica $(, $core)?).0.clone();
                let log = view!(replica $(, $core)?).1.clone();
                let mut incoming = EventLog::for_crdt(view!(replica $(, $core)?).0);
                assert_eq!(
                    incoming.insert_record(view!(replica $(, $core)?).0, second),
                    safemesh_crdt::Admission::Accepted
                );
                assert_eq!(
                    replica
                        .merge_log_bytes(&incoming.to_wire_bytes().unwrap(), None, None)
                        .unwrap(),
                    vec!["collision"]
                );
                assert_eq!(view!(replica $(, $core)?).0, &state);
                assert_eq!(view!(replica $(, $core)?).1, &log);
            }};
        }
        check!(
            SafeMeshGCounterReplica::new(2, 2),
            GCounterDelta {
                replica: 1,
                tally: 5
            },
            GCounterDelta {
                replica: 1,
                tally: 9
            },
            core
        );
        check!(
            SafeMeshEnableWinsFlagReplica::new(2),
            EnableWinsFlagDelta::Enable { token: 5 },
            EnableWinsFlagDelta::Enable { token: 9 }
        );
        check!(
            SafeMeshLwwRegisterReplica::new(2),
            LwwRegisterDelta {
                timestamp: 1,
                replica: 1,
                value: 5
            },
            LwwRegisterDelta {
                timestamp: 2,
                replica: 1,
                value: 9
            }
        );
        check!(
            SafeMeshLwwMapReplica::new(2),
            LwwMapDelta::Set {
                key: 1,
                timestamp: 1,
                replica: 1,
                value: 5
            },
            LwwMapDelta::Remove {
                key: 1,
                timestamp: 2,
                replica: 1
            }
        );

        let first = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: OrSetDelta::Add {
                element: "first".to_owned(),
                token: 5,
            },
        };
        let second = Record {
            id: first.id,
            delta: OrSetDelta::Add {
                element: "second".to_owned(),
                token: 9,
            },
        };
        let mut replica = SafeMeshStringOrSetReplica::new(2);
        replica.admit(first).unwrap();
        let state = replica.replica.state().clone();
        let log = replica.replica.log().clone();
        let mut incoming = EventLog::for_crdt(replica.replica.state());
        assert_eq!(
            incoming.insert_record(replica.replica.state(), second),
            safemesh_crdt::Admission::Accepted
        );
        assert_eq!(
            replica
                .try_merge_log_bytes(&incoming.to_wire_bytes().unwrap())
                .unwrap(),
            vec!["collision"]
        );
        assert_eq!(replica.replica.state(), &state);
        assert_eq!(replica.replica.log(), &log);
    }

    #[test]
    fn wasm_record_and_log_paths_name_the_same_verdict() {
        macro_rules! view {
            ($replica:ident) => {
                (&$replica.state, &$replica.log)
            };
            ($replica:ident, core) => {
                ($replica.replica.state(), $replica.replica.log())
            };
        }
        macro_rules! check {
            ($replica:expr, $first:expr, $second:expr $(, $core:ident)?) => {{
                let first = Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 1,
                    },
                    delta: $first,
                };
                let second = Record {
                    id: first.id,
                    delta: $second,
                };
                let mut replica = $replica;
                assert_eq!(
                    replica
                        .merge_record_bytes(&first.to_wire_bytes().unwrap(), None)
                        .unwrap(),
                    "accepted"
                );
                let state = view!(replica $(, $core)?).0.clone();
                let log = view!(replica $(, $core)?).1.clone();
                // A redelivery and a conflicting payload each get one verdict, named
                // the same on both paths, and neither moves state or the log.
                for (record, verdict) in [(first, "duplicate"), (second, "collision")] {
                    let single = replica
                        .merge_record_bytes(&record.to_wire_bytes().unwrap(), None)
                        .unwrap();
                    let mut incoming = EventLog::for_crdt(view!(replica $(, $core)?).0);
                    assert_eq!(
                        incoming.insert_record(view!(replica $(, $core)?).0, record),
                        safemesh_crdt::Admission::Accepted
                    );
                    let batch = replica
                        .merge_log_bytes(&incoming.to_wire_bytes().unwrap(), None, None)
                        .unwrap();
                    println!(
                        "WASM {}: single={single} batch={batch:?}",
                        stringify!($replica)
                    );
                    assert_eq!(single, verdict);
                    assert_eq!(batch, vec![single]);
                    assert_eq!(view!(replica $(, $core)?).0, &state);
                    assert_eq!(view!(replica $(, $core)?).1, &log);
                }
            }};
        }
        check!(
            SafeMeshGCounterReplica::new(2, 2),
            GCounterDelta {
                replica: 1,
                tally: 5
            },
            GCounterDelta {
                replica: 1,
                tally: 9
            },
            core
        );
        check!(
            SafeMeshPnCounterReplica::new(2, 2),
            PnCounterDelta::Inc {
                replica: 1,
                tally: 5
            },
            PnCounterDelta::Dec {
                replica: 1,
                tally: 9
            }
        );
        check!(
            SafeMeshEnableWinsFlagReplica::new(2),
            EnableWinsFlagDelta::Enable { token: 5 },
            EnableWinsFlagDelta::Enable { token: 9 }
        );
        check!(
            SafeMeshLwwRegisterReplica::new(2),
            LwwRegisterDelta {
                timestamp: 1,
                replica: 1,
                value: 5
            },
            LwwRegisterDelta {
                timestamp: 2,
                replica: 1,
                value: 9
            }
        );
        check!(
            SafeMeshLwwMapReplica::new(2),
            LwwMapDelta::Set {
                key: 1,
                timestamp: 1,
                replica: 1,
                value: 5
            },
            LwwMapDelta::Remove {
                key: 1,
                timestamp: 2,
                replica: 1
            }
        );
        check!(
            SafeMeshStringOrSetReplica::new(2),
            OrSetDelta::Add {
                element: "first".to_owned(),
                token: 5,
            },
            OrSetDelta::Add {
                element: "second".to_owned(),
                token: 9,
            },
            core
        );
    }

    #[test]
    fn wasm_lww_register_calls_rust_core() {
        let mut register = SafeMeshLwwRegister::new();
        assert!(!register.has_value());
        register.set(10, 1, 100);
        register.set(9, 99, 900);
        register.set(10, 2, 200);
        assert_eq!(register.value_or(0), 200);
        assert_eq!(register.timestamp_or(0), 10);
        assert_eq!(register.writer_replica_or(0), 2);
    }

    #[test]
    fn wasm_lww_wire_helper_uses_canonical_bytes() {
        let bytes = lww_register_delta_to_wire(9, 2, 42).unwrap();
        assert_eq!(bytes.len(), 25);
        assert_eq!(bytes[0], 0x50);
    }

    #[test]
    fn wasm_enable_wins_flag_calls_rust_core() {
        let mut flag = SafeMeshEnableWinsFlag::new();
        assert!(!flag.value());
        flag.enable(2);
        flag.enable(1);
        assert!(flag.value());
        assert_eq!(flag.enabled_tokens(), vec![1, 2]);

        flag.disable_observed();
        assert!(!flag.value());
        assert_eq!(flag.tombstone_tokens(), vec![1, 2]);

        flag.enable(3);
        assert!(flag.value());
    }

    #[test]
    fn wasm_enable_wins_flag_wire_helpers_preserve_exact_bytes() {
        let enable = enable_wins_flag_enable_delta_to_wire(42).unwrap();
        assert_eq!(enable.len(), 9);
        assert_eq!(enable[0], 0x60);

        let disable = enable_wins_flag_disable_delta_to_wire(vec![9, 2, 2]).unwrap();
        assert_eq!(disable.len(), 29);
        assert_eq!(disable[0], 0x61);
        assert_eq!(&disable[1..5], &[3, 0, 0, 0]);
    }

    #[test]
    fn wasm_lww_map_calls_rust_core() {
        let mut map = SafeMeshLwwMap::new();
        assert!(!map.has_key(7));
        map.set(7, 10, 1, 100);
        map.set(2, 1, 1, 20);
        assert_eq!(map.value_or(7, 0), 100);
        assert_eq!(map.visible_keys(), vec![2, 7]);

        map.remove(7, 11, 1);
        assert!(!map.has_key(7));
        assert_eq!(map.value_or(7, 0), 0);
        assert_eq!(map.entry_keys(), vec![2, 7]);
        assert_eq!(map.removal_keys(), vec![7]);

        map.set(7, 12, 1, 300);
        assert_eq!(map.value_or(7, 0), 300);
    }

    #[test]
    fn wasm_lww_map_wire_helpers_use_canonical_bytes() {
        let set = lww_map_set_delta_to_wire(7, 9, 2, 42).unwrap();
        assert_eq!(set.len(), 33);
        assert_eq!(set[0], 0x70);

        let remove = lww_map_remove_delta_to_wire(7, 10, 2).unwrap();
        assert_eq!(remove.len(), 25);
        assert_eq!(remove[0], 0x71);
    }

    #[test]
    fn wasm_flag_replicas_exchange_canonical_record_bytes() {
        let mut left = SafeMeshEnableWinsFlagReplica::new(1);
        let mut right = SafeMeshEnableWinsFlagReplica::new(2);

        let enable_10 = left.append_enable(10).unwrap();
        let expected = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: EnableWinsFlagDelta::Enable { token: 10 },
        }
        .to_wire_bytes()
        .unwrap();
        assert_eq!(enable_10, expected);

        right.merge_record_bytes(&enable_10, None).unwrap();
        right.merge_record_bytes(&enable_10, None).unwrap();
        assert!(right.value());
        assert_eq!(right.version_for(1), 1);

        let disable_10 = right.append_disable_observed().unwrap();
        assert!(!right.value());

        let enable_11 = left.append_enable(11).unwrap();
        right.merge_record_bytes(&enable_11, None).unwrap();
        assert!(right.value());

        left.merge_record_bytes(&disable_10, None).unwrap();
        assert!(left.value());
        left.merge_log_bytes(&right.log_bytes().unwrap(), None, None)
            .unwrap();
        assert_eq!(left.value(), right.value());
        assert_eq!(left.enabled_tokens(), vec![10, 11]);
        assert_eq!(left.tombstone_tokens(), vec![10]);
    }

    #[test]
    fn wasm_lww_map_replicas_exchange_canonical_record_bytes() {
        let mut left = SafeMeshLwwMapReplica::new(1);
        let mut right = SafeMeshLwwMapReplica::new(2);

        let set_100 = left.append_set(7, 10, 1, 100).unwrap();
        let expected = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: LwwMapDelta::Set {
                key: 7,
                timestamp: 10,
                replica: 1,
                value: 100,
            },
        }
        .to_wire_bytes()
        .unwrap();
        assert_eq!(set_100, expected);

        right.merge_record_bytes(&set_100, None).unwrap();
        right.merge_record_bytes(&set_100, None).unwrap();
        assert_eq!(right.value_or(7, 0), 100);
        assert_eq!(right.version_for(1), 1);

        let remove_100 = right.append_remove(7, 11, 2).unwrap();
        assert!(!right.has_key(7));

        let set_300 = left.append_set(7, 12, 1, 300).unwrap();
        right.merge_record_bytes(&set_300, None).unwrap();
        assert_eq!(right.value_or(7, 0), 300);

        left.merge_record_bytes(&remove_100, None).unwrap();
        assert_eq!(left.value_or(7, 0), 300);
        left.merge_log_bytes(&right.log_bytes().unwrap(), None, None)
            .unwrap();
        assert_eq!(left.value_or(7, 0), right.value_or(7, 0));
        assert_eq!(left.visible_keys(), vec![7]);
        assert_eq!(left.removal_keys(), vec![7]);
    }

    #[test]
    fn wasm_lww_replicas_exchange_canonical_record_bytes() {
        let mut left = SafeMeshLwwRegisterReplica::new(1);
        let mut right = SafeMeshLwwRegisterReplica::new(2);

        let bytes = left.append_set(10, 1, 100).unwrap();
        let expected = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: LwwRegisterDelta {
                timestamp: 10,
                replica: 1,
                value: 100,
            },
        }
        .to_wire_bytes()
        .unwrap();
        assert_eq!(bytes, expected);

        right.merge_record_bytes(&bytes, None).unwrap();
        right.merge_record_bytes(&bytes, None).unwrap();
        right.append_set(10, 2, 200).unwrap();
        assert_eq!(right.value_or(0), 200);
        assert_eq!(right.version_for(1), 1);

        left.merge_log_bytes(&right.log_bytes().unwrap(), None, None)
            .unwrap();
        assert_eq!(left.value_or(0), right.value_or(0));
        assert_eq!(left.writer_replica_or(0), 2);
    }

    fn string_orset_pairs(entries: Vec<SafeMeshStringOrSetAddEntry>) -> Vec<(String, u64)> {
        entries
            .iter()
            .map(|entry| (entry.element(), entry.token()))
            .collect()
    }

    #[test]
    fn wasm_string_orset_replica_round_trips_records_through_core() {
        let mut left = SafeMeshStringOrSetReplica::new(1);
        let mut right = SafeMeshStringOrSetReplica::new(2);
        let mut core = OrSet::<String, u64>::new();

        let add_bytes = left.append_add("vaccine".to_string(), 11).unwrap();
        assert_eq!(
            right.try_merge_record_bytes(&add_bytes).unwrap(),
            "accepted"
        );
        core.apply_delta(OrSetDelta::Add {
            element: "vaccine".to_string(),
            token: 11,
        });
        println!(
            "C1 appendAdd: left.elements={:?} right.elements={:?} core.elements={:?}",
            left.elements(),
            right.elements(),
            core.elements()
        );
        assert_eq!(right.elements(), left.elements());
        assert_eq!(right.elements(), vec!["vaccine".to_string()]);
        assert_eq!(
            right.elements(),
            core.elements().into_iter().collect::<Vec<_>>()
        );
        assert_eq!(right.observed_tokens("vaccine".to_string()), vec![11]);

        let remove_bytes = left.append_remove_observed("vaccine".to_string()).unwrap();
        assert_eq!(
            right.try_merge_record_bytes(&remove_bytes).unwrap(),
            "accepted"
        );
        core.apply_delta(OrSetDelta::Remove { tokens: vec![11] });
        println!(
            "C1 appendRemoveObserved: left.elements={:?} right.elements={:?} core.elements={:?} tombstones={:?}",
            left.elements(),
            right.elements(),
            core.elements(),
            right.tombstones()
        );
        assert_eq!(right.elements(), left.elements());
        assert!(right.elements().is_empty());
        assert_eq!(right.tombstones(), left.tombstones());
        assert_eq!(right.tombstones(), vec![11]);
        assert_eq!(
            right.tombstones(),
            core.tombstones().iter().copied().collect::<Vec<_>>()
        );
        let entries = string_orset_pairs(right.add_entries());
        assert_eq!(entries, string_orset_pairs(left.add_entries()));
        assert_eq!(entries, vec![("vaccine".to_string(), 11)]);
        assert_eq!(entries, core.adds().iter().cloned().collect::<Vec<_>>());

        let mut third = SafeMeshStringOrSetReplica::new(3);
        third
            .try_merge_log_bytes(&left.log_bytes().unwrap())
            .unwrap();
        assert_eq!(third.elements(), left.elements());
        assert_eq!(third.tombstones(), left.tombstones());
        assert_eq!(
            string_orset_pairs(third.add_entries()),
            string_orset_pairs(left.add_entries())
        );
        for replica in [&left, &right, &third] {
            assert_eq!(replica.version_for(1), 2);
            assert_eq!(replica.version_for(2), 0);
        }
        let record = |author, sequence, delta| Record {
            id: RecordId {
                replica: author,
                sequence,
            },
            delta,
        };
        let add = record(
            0,
            1,
            OrSetDelta::Add {
                element: "water".into(),
                token: 2,
            },
        );
        let other = record(
            1,
            1,
            OrSetDelta::Add {
                element: "radio".into(),
                token: 3,
            },
        );
        let remove = record(1, 2, OrSetDelta::Remove { tokens: vec![2] });
        let early = record(1, 3, OrSetDelta::Remove { tokens: vec![100] });
        let late = record(
            0,
            2,
            OrSetDelta::Add {
                element: "late".into(),
                token: 100,
            },
        );
        let collision = record(
            0,
            1,
            OrSetDelta::Add {
                element: "collision".into(),
                token: 2,
            },
        );
        let frame = |records: &[Record<OrSetDelta<String, u64>>]| {
            let mut bytes = Vec::new();
            EventLog::encode_records(None, records, &mut bytes).unwrap();
            bytes
        };
        let mut actual = SafeMeshStringOrSetReplica::new(9);
        let mut oracle = LegacyStringOrSet::new(9, None);
        for records in [
            vec![add.clone(), other.clone()],
            vec![remove.clone(), early],
            vec![late],
            vec![add.clone(), add.clone()],
            vec![collision],
        ] {
            let bytes = frame(&records);
            assert_eq!(
                actual.try_merge_log_bytes(&bytes).unwrap(),
                oracle.merge(&bytes, None).unwrap()
            );
            oracle.assert_same(&actual);
        }
        assert_eq!(actual.elements(), vec!["radio"]);
        assert_eq!(actual.tombstones(), vec![2, 100]);

        let mut allocated = SafeMeshStringOrSetReplica::try_create_allocated(2, 0).unwrap();
        let mut oracle = LegacyStringOrSet::new(0, Some(2));
        let invalid = record(
            1,
            2,
            OrSetDelta::Add {
                element: "not owned".into(),
                token: 4,
            },
        );
        let local = record(
            0,
            1,
            OrSetDelta::Add {
                element: "local".into(),
                token: 2,
            },
        );
        let zero = record(1, 0, OrSetDelta::Remove { tokens: vec![3] });
        for records in [
            vec![other.clone(), invalid],
            vec![other.clone(), local],
            vec![other, zero],
        ] {
            let bytes = frame(&records);
            let expected = oracle.merge(&bytes, None).unwrap_err();
            let error = allocated.try_merge_log_bytes(&bytes).unwrap_err();
            assert_eq!(
                (error.code, error.message),
                (expected.code, expected.message)
            );
            oracle.assert_same(&allocated);
            assert!(allocated.elements().is_empty());
        }
    }

    #[test]
    fn wasm_string_orset_replica_rejects_duplicate_record_without_moving_state() {
        let mut author = SafeMeshStringOrSetReplica::new(1);
        let mut reader = SafeMeshStringOrSetReplica::new(2);
        let bytes = author.append_add("vaccine".to_string(), 11).unwrap();

        let first = reader.try_merge_record_bytes(&bytes).unwrap();
        let before = (
            reader.elements(),
            reader.tombstones(),
            reader.version_for(1),
            reader.replica.log().records().len(),
        );
        let second = reader.try_merge_record_bytes(&bytes).unwrap();
        let after = (
            reader.elements(),
            reader.tombstones(),
            reader.version_for(1),
            reader.replica.log().records().len(),
        );
        println!("C2 first merge={first} second merge={second}");
        println!(
            "C2 state before second merge (elements, tombstones, version[1], records)={before:?}"
        );
        println!(
            "C2 state after  second merge (elements, tombstones, version[1], records)={after:?}"
        );
        assert_eq!(first, "accepted");
        assert_eq!(second, "duplicate");
        assert_eq!(before, after);
        assert_eq!(after.3, 1);

        // Same identity, different payload: the core reports a collision. The
        // binding returns that verdict, as the log path does, and absorbs neither
        // reading.
        let forged = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: OrSetDelta::Add {
                element: "forged".to_string(),
                token: 99,
            },
        };
        let verdict = reader
            .try_merge_record_bytes(&forged.to_wire_bytes().unwrap())
            .unwrap();
        println!("C2 same id, different payload: {verdict}");
        assert_eq!(verdict, "collision");
        let mut incoming = EventLog::for_crdt(reader.replica.state());
        assert_eq!(
            incoming.insert_record(reader.replica.state(), forged),
            safemesh_crdt::Admission::Accepted
        );
        assert_eq!(
            reader
                .try_merge_log_bytes(&incoming.to_wire_bytes().unwrap())
                .unwrap(),
            vec![verdict]
        );
        assert_eq!(reader.elements(), before.0);
        assert_eq!(reader.tombstones(), before.1);
        assert_eq!(reader.replica.log().records().len(), 1);
    }

    #[test]
    fn wasm_string_orset_replica_refuses_corrupted_bytes_without_panicking() {
        let mut author = SafeMeshStringOrSetReplica::new(1);
        let good = author.append_add("vaccine".to_string(), 11).unwrap();

        let mut reader = SafeMeshStringOrSetReplica::new(2);
        assert_eq!(reader.try_merge_record_bytes(&good).unwrap(), "accepted");
        assert_eq!(reader.elements(), vec!["vaccine".to_string()]);
        println!(
            "C3 good record ({} bytes): accepted, elements={:?}",
            good.len(),
            reader.elements()
        );

        let mut planted = good.clone();
        planted[0] ^= 0xff;
        let mut reader = SafeMeshStringOrSetReplica::new(2);
        let error = reader.try_merge_record_bytes(&planted).unwrap_err();
        println!("C3 planted bad byte 0 (record tag): {}", error.message);
        assert_eq!(
            error.message,
            "failed to decode record: unexpected wire tag"
        );
        assert!(reader.elements().is_empty());
        assert_eq!(reader.replica.log().records().len(), 0);

        // Every single-byte change on the bare record path either errors or
        // decodes as a visibly different record. None panics, none is absorbed
        // as the original.
        let original = SafeMeshStringOrSetReplica::try_inspect_record_bytes(&good).unwrap();
        let (mut errored, mut decoded_differently) = (0usize, 0usize);
        for position in 0..good.len() {
            let mut bad = good.clone();
            bad[position] ^= 0x01;
            let mut reader = SafeMeshStringOrSetReplica::new(2);
            match reader.try_merge_record_bytes(&bad) {
                Err(error) => {
                    errored += 1;
                    assert!(reader.elements().is_empty());
                    assert_eq!(reader.replica.log().records().len(), 0);
                    println!("C3 record byte {position}: {}", error.message);
                }
                Ok(verdict) => {
                    decoded_differently += 1;
                    let seen = SafeMeshStringOrSetReplica::try_inspect_record_bytes(&bad).unwrap();
                    let seen_fields = (
                        seen.replica(),
                        seen.sequence(),
                        seen.element(),
                        seen.token(),
                    );
                    let original_fields = (
                        original.replica(),
                        original.sequence(),
                        original.element(),
                        original.token(),
                    );
                    assert_ne!(seen_fields, original_fields);
                    println!(
                        "C3 record byte {position}: {verdict} as a different record {seen_fields:?} (original {original_fields:?})"
                    );
                }
            }
        }
        println!(
            "C3 record sweep: {} bytes, {errored} errored, {decoded_differently} decoded as a different record",
            good.len()
        );
        assert_eq!(errored + decoded_differently, good.len());
        assert!(errored > 0);

        // The event-log frame carries a CRC, so every single-byte change errors.
        let log = author.log_bytes().unwrap();
        let mut log_errors = 0usize;
        for position in 0..log.len() {
            let mut bad = log.clone();
            bad[position] ^= 0x01;
            let mut reader = SafeMeshStringOrSetReplica::new(2);
            let error = reader.try_merge_log_bytes(&bad).unwrap_err();
            assert!(reader.elements().is_empty());
            log_errors += 1;
            if position < 2 || position + 1 == log.len() {
                println!("C3 log byte {position}: {}", error.message);
            }
        }
        println!("C3 log sweep: {} bytes, {log_errors} errored", log.len());
        assert_eq!(log_errors, log.len());
        let mut reader = SafeMeshStringOrSetReplica::new(2);
        reader.try_merge_log_bytes(&log).unwrap();
        assert_eq!(reader.elements(), vec!["vaccine".to_string()]);
        let mut trailing = log.clone();
        trailing.push(0);
        let mut checksum = log.clone();
        *checksum.last_mut().unwrap() ^= 1;
        let wrong = EventLog::<GCounterDelta>::for_crdt(&GCounter::new(2))
            .to_wire_bytes()
            .unwrap();
        for bytes in [
            &log[..log.len() - 1],
            trailing.as_slice(),
            checksum.as_slice(),
            wrong.as_slice(),
        ] {
            let mut oracle = LegacyStringOrSet::new(2, None);
            let mut actual = SafeMeshStringOrSetReplica::new(2);
            let expected = oracle.merge(bytes, None).unwrap_err();
            let error = actual.try_merge_log_bytes(bytes).unwrap_err();
            assert_eq!(
                (error.code, error.message),
                (expected.code, expected.message)
            );
            oracle.assert_same(&actual);
        }
    }

    #[test]
    fn wasm_string_orset_refuses_sequence_zero_add_with_core_text() {
        // Every WASM path surfaces the core `WireError::ZeroSequenceAdd` text:
        // legacy and allocated, one record or a whole log, and stored identity.
        let expected = WireError::ZeroSequenceAdd { replica: 1 }.to_string();
        assert!(expected.contains("replica 1, sequence 0"));
        assert!(expected.contains("Recovery: "));
        let add = Record {
            id: RecordId {
                replica: 1,
                sequence: 0,
            },
            delta: OrSetDelta::Add {
                element: "water".to_string(),
                token: 0,
            },
        };
        let add_bytes = add.to_wire_bytes().unwrap();
        // An inert batch, as a peer or an old store would hold it.
        let mut log_bytes = Vec::new();
        EventLog::encode_records(None, core::slice::from_ref(&add), &mut log_bytes).unwrap();

        let mut legacy = SafeMeshStringOrSetReplica::new(0);
        let single = legacy.try_merge_record_bytes(&add_bytes).unwrap_err();
        assert_eq!(
            (single.code, single.message.as_str()),
            (1, expected.as_str())
        );
        let batch = legacy.try_merge_log_bytes(&log_bytes).unwrap_err();
        assert_eq!((batch.code, batch.message.as_str()), (1, expected.as_str()));
        assert!(legacy.replica.log().records().is_empty());
        assert!(legacy.elements().is_empty());

        let mut allocated = SafeMeshStringOrSetReplica::try_create_allocated(2, 0).unwrap();
        let single = allocated.try_merge_record_bytes(&add_bytes).unwrap_err();
        assert_eq!(
            (single.code, single.message.as_str()),
            (1, expected.as_str())
        );
        let batch = allocated.try_merge_log_bytes(&log_bytes).unwrap_err();
        assert_eq!((batch.code, batch.message.as_str()), (1, expected.as_str()));
        assert!(allocated.replica.log().records().is_empty());
        assert!(allocated.elements().is_empty());
        drop(allocated);

        // Stored identity whose log holds a sequence-0 add fails loudly with
        // the same text and creates no writer.
        let mut identity = b"SMOI\x01".to_vec();
        for word in [2u64, 0, 1] {
            identity.extend_from_slice(&word.to_le_bytes());
        }
        identity.extend_from_slice(&log_bytes);
        let stored =
            match SafeMeshStringOrSetReplica::try_import_identity_with_limits(&identity, None) {
                Err(error) => error,
                Ok(_) => panic!("identity with a sequence-0 add was imported"),
            };
        assert_eq!(
            (stored.code, stored.message.as_str()),
            (1, expected.as_str())
        );
        assert!(SafeMeshStringOrSetReplica::try_create_allocated(2, 0).is_ok());

        // Inspection only decodes; it neither admits nor refuses.
        assert_eq!(
            SafeMeshStringOrSetReplica::try_inspect_record_bytes(&add_bytes)
                .unwrap()
                .sequence(),
            0
        );
    }

    #[test]
    fn wasm_string_orset_refuses_sequence_zero_remove_with_core_text() {
        let expected = WireError::ZeroSequenceRemove { replica: 1 }.to_string();
        let remove: Record<OrSetDelta<String, u64>> = Record {
            id: RecordId {
                replica: 1,
                sequence: 0,
            },
            delta: OrSetDelta::Remove { tokens: vec![0] },
        };
        let record_bytes = remove.to_wire_bytes().unwrap();
        let mut log_bytes = Vec::new();
        EventLog::encode_records(None, &[remove], &mut log_bytes).unwrap();
        for mut replica in [
            SafeMeshStringOrSetReplica::new(0),
            SafeMeshStringOrSetReplica::try_create_allocated(2, 0).unwrap(),
        ] {
            let single = replica.try_merge_record_bytes(&record_bytes).unwrap_err();
            assert_eq!(
                (single.code, single.message.as_str()),
                (1, expected.as_str())
            );
            let batch = replica.try_merge_log_bytes(&log_bytes).unwrap_err();
            assert_eq!((batch.code, batch.message.as_str()), (1, expected.as_str()));
            assert!(replica.replica.log().records().is_empty());
            assert!(replica.elements().is_empty());
        }
        let mut identity = b"SMOI\x01".to_vec();
        for word in [2u64, 0, 1] {
            identity.extend_from_slice(&word.to_le_bytes());
        }
        identity.extend_from_slice(&log_bytes);
        let stored =
            match SafeMeshStringOrSetReplica::try_import_identity_with_limits(&identity, None) {
                Err(error) => error,
                Ok(_) => panic!("identity with a sequence-0 remove was imported"),
            };
        assert_eq!(
            (stored.code, stored.message.as_str()),
            (1, expected.as_str())
        );
    }

    #[test]
    fn wasm_string_orset_replica_keeps_core_token_semantics() {
        // (a) A reused token keeps both pairs; the mirror overwrote to [b].
        let mut replica = SafeMeshStringOrSetReplica::new(1);
        replica.append_add("a".to_string(), 7).unwrap();
        replica.append_add("b".to_string(), 7).unwrap();
        let mut core = OrSet::<String, u64>::new();
        core.add("a".to_string(), 7);
        core.add("b".to_string(), 7);
        println!(
            "C4a reused token: binding.elements={:?} core.elements={:?} addEntries={:?}",
            replica.elements(),
            core.elements(),
            string_orset_pairs(replica.add_entries())
        );
        assert_eq!(replica.elements(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            replica.elements(),
            core.elements().into_iter().collect::<Vec<_>>()
        );
        assert_eq!(
            string_orset_pairs(replica.add_entries()),
            vec![("a".to_string(), 7), ("b".to_string(), 7)]
        );
        // Tokens are global: removing what was observed for `a` tombstones 7 and
        // takes `b` with it, as the core does.
        replica.append_remove_observed("a".to_string()).unwrap();
        core.apply_remove([7]);
        println!(
            "C4a then removeObserved(a): binding.elements={:?} core.elements={:?} tombstones={:?}",
            replica.elements(),
            core.elements(),
            replica.tombstones()
        );
        assert!(replica.elements().is_empty());
        assert!(core.elements().is_empty());

        // (b) Observed tokens exclude removals, while tombstones retain history.
        let mut replica = SafeMeshStringOrSetReplica::new(1);
        replica.append_add("a".to_string(), 7).unwrap();
        let remove = replica.append_remove_observed("a".to_string()).unwrap();
        let mut core = OrSet::<String, u64>::new();
        core.add("a".to_string(), 7);
        core.apply_remove([7]);
        println!(
            "C4b observed after removal: binding.observedTokens(a)={:?} core.observed_tokens(a)={:?} elements={:?} tombstones={:?}",
            replica.observed_tokens("a".to_string()),
            core.observed_tokens(&"a".to_string()),
            replica.elements(),
            replica.tombstones()
        );
        assert_eq!(replica.observed_tokens("a".to_string()), Vec::<u64>::new());
        assert_eq!(
            replica.observed_tokens("a".to_string()),
            core.observed_tokens(&"a".to_string())
                .into_iter()
                .collect::<Vec<_>>()
        );
        assert!(replica.elements().is_empty());
        assert_eq!(replica.tombstones(), vec![7]);
        let record = SafeMeshStringOrSetReplica::try_inspect_record_bytes(&remove).unwrap();
        assert_eq!(record.delta_kind(), "remove");
        assert_eq!(record.tokens(), vec![7]);
    }

    #[test]
    fn since_batch_is_the_core_since_selection_for_a_checked_peer_version() {
        let mut writer = SafeMeshGCounterReplica::new(0, 2);
        let mut ahead = SafeMeshGCounterReplica::new(8, 2);
        let mut behind = SafeMeshGCounterReplica::new(9, 2);
        for tally in 1..=10 {
            let record = writer.append_bump(0, tally).unwrap();
            ahead.merge_record_bytes(&record, None).unwrap();
            if tally <= 6 {
                behind.merge_record_bytes(&record, None).unwrap();
            }
        }
        let peer = peer_version(&version_pairs(behind.replica.log())).unwrap();
        assert_eq!(&peer, behind.replica.version());
        let unbounded = DecodeLimits::default();
        let batch =
            since_log_bytes(ahead.replica.state(), ahead.replica.log(), &peer, unbounded).unwrap();
        let mut expected = Vec::new();
        EventLog::encode_records(Some(2), &ahead.replica.since(&peer), &mut expected).unwrap();
        assert_eq!(batch, expected);
        assert_eq!(
            behind.merge_log_bytes(&batch, None, None).unwrap(),
            vec!["accepted"; 4]
        );
        assert_eq!(behind.log_bytes().unwrap(), ahead.log_bytes().unwrap());
        assert_eq!(
            record_id_pairs(behind.replica.log()),
            (1..=10)
                .flat_map(|sequence| [0, sequence])
                .collect::<Vec<_>>()
        );
        let three = DecodeLimits {
            max_records: Some(3),
            ..unbounded
        };
        assert_eq!(
            since_log_bytes(ahead.replica.state(), ahead.replica.log(), &peer, three),
            Err(ReplicaError::LogDecode(DecodeError::RecordLimitExceeded {
                max_records: 3
            }))
        );
    }

    #[test]
    fn peer_version_refuses_malformed_pairs_by_name() {
        assert_eq!(peer_version(&[]).unwrap(), VersionVector::new());
        let too_many: Vec<u64> = (0..4097).flat_map(|author| [author, 1]).collect();
        for (pairs, message) in [
            (
                &[7][..],
                "peerVersion must hold (author, prefix) pairs; got 1 values",
            ),
            (&[7, 1, 7, 2][..], "peerVersion repeats author 7"),
            (
                &[7, 0][..],
                "peerVersion: replica 7 has a noncanonical zero prefix",
            ),
            (
                &too_many[..],
                "peerVersion: peer version exceeds author limit 4096",
            ),
        ] {
            assert_eq!(peer_version(pairs), Err(binding_error(2, message)));
        }
    }

    #[test]
    fn wasm_string_orset_record_inspector_reports_core_decoded_fields() {
        let mut author = SafeMeshStringOrSetReplica::new(9);
        let add = author.append_add("vaccine".to_string(), 11).unwrap();
        let expected = Record {
            id: RecordId {
                replica: 9,
                sequence: 1,
            },
            delta: OrSetDelta::Add {
                element: "vaccine".to_string(),
                token: 11,
            },
        }
        .to_wire_bytes()
        .unwrap();
        assert_eq!(add, expected);

        let view = SafeMeshStringOrSetReplica::try_inspect_record_bytes(&add).unwrap();
        let fields = (
            view.replica(),
            view.sequence(),
            view.delta_kind(),
            view.element(),
            view.token(),
            view.tokens(),
        );
        println!("inspectRecordBytes(add)={fields:?}");
        assert_eq!(
            fields,
            (
                9,
                1,
                "add".to_string(),
                Some("vaccine".to_string()),
                Some(11),
                vec![]
            )
        );

        let remove = author
            .append_remove_observed("vaccine".to_string())
            .unwrap();
        let view = SafeMeshStringOrSetReplica::try_inspect_record_bytes(&remove).unwrap();
        let fields = (
            view.replica(),
            view.sequence(),
            view.delta_kind(),
            view.element(),
            view.token(),
            view.tokens(),
        );
        println!("inspectRecordBytes(remove)={fields:?}");
        assert_eq!(fields, (9, 2, "remove".to_string(), None, None, vec![11]));

        // Inspecting admits nothing: a reader still accepts the record afterwards.
        let mut reader = SafeMeshStringOrSetReplica::new(2);
        assert_eq!(reader.try_merge_record_bytes(&add).unwrap(), "accepted");

        let mut bad = add.clone();
        bad[0] ^= 0xff;
        assert_eq!(
            SafeMeshStringOrSetReplica::try_inspect_record_bytes(&bad)
                .unwrap_err()
                .message,
            "failed to decode record: unexpected wire tag"
        );
    }
}

#[wasm_bindgen]
pub struct SafeMeshPnCounterReplica {
    replica_id: u64,
    state: PnCounter,
    log: EventLog<PnCounterDelta>,
}

#[wasm_bindgen]
impl SafeMeshPnCounterReplica {
    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica_id: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] replicas: JsValue,
    ) -> Result<Self, JsValue> {
        let replica_id = checked_u64(replica_id, "replica_id")?;
        let replicas = checked_replica_count(replicas)?;
        Ok(Self::new(replica_id, replicas))
    }

    #[wasm_bindgen(js_name = appendInc)]
    pub fn append_inc_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "number")] counter_replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] tally: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let counter_replica = checked_index(counter_replica, "counter_replica")?;
        let tally = checked_u64(tally, "tally")?;
        self.append_inc(counter_replica, tally)
    }

    #[wasm_bindgen(js_name = appendDec)]
    pub fn append_dec_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "number")] counter_replica: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] tally: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let counter_replica = checked_index(counter_replica, "counter_replica")?;
        let tally = checked_u64(tally, "tally")?;
        self.append_dec(counter_replica, tally)
    }

    /// Return the core admission verdict for the decoded input record, as
    /// `mergeLogBytes` does per record. Only `"accepted"` changes state.
    #[wasm_bindgen(
        js_name = mergeRecordBytes,
        unchecked_return_type = "\"accepted\" | \"duplicate\" | \"collision\""
    )]
    pub fn merge_record_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<String, JsValue> {
        let record = Record::<PnCounterDelta>::from_wire_bytes_with_collection_limits(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(record_decode_js_error)?;
        if safemesh_crdt::ownership::check_counter_record(
            self.state.p_state().len(),
            record.id,
            &record.delta,
        )
        .is_err()
        {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        record_verdict(
            self.log
                .admit_with(&mut self.state, record, |state, delta| {
                    state.apply_delta(delta.clone());
                }),
        )
        .map_err(JsValue::from)
    }

    /// Return one core admission verdict for every decoded input record.
    /// `maxRecords` is the third argument; the second limits collection elements.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    #[allow(non_snake_case)]
    pub fn merge_log_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
        maxRecords: Option<u32>,
    ) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<PnCounterDelta>::records_from_wire_bytes_for_with_limits(
            bytes,
            &self.state,
            decode_limits(max_collection_elements, maxRecords),
        )
        .map_err(event_log_decode_js_error)?;
        if log.iter().any(|r| {
            safemesh_crdt::ownership::check_counter_record(
                self.state.p_state().len(),
                r.id,
                &r.delta,
            )
            .is_err()
        }) {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        Ok(log
            .iter()
            .cloned()
            .map(|record| {
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    })
            })
            .map(admission_name)
            .collect())
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.log
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode event log"))
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for_js(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica: JsValue,
    ) -> Result<u64, JsValue> {
        let replica = checked_u64(replica, "replica")?;
        Ok(self.version_for(replica))
    }

    /// The counter total as an exact `bigint`, also past the 64-bit boundary.
    #[wasm_bindgen(unchecked_return_type = "bigint")]
    pub fn value(&self) -> JsValue {
        JsValue::bigint_from_str(&self.total().to_string())
    }

    /// The Rust-core total behind [`Self::value`]; not exported, so host-side
    /// tests can read it without constructing a JavaScript value.
    fn total(&self) -> i128 {
        self.state.value()
    }

    /// Compare the Rust-core carrier states without reproducing its equality in JavaScript.
    #[wasm_bindgen(js_name = sameStateAs)]
    pub fn same_state_as(&self, other: &SafeMeshPnCounterReplica) -> bool {
        self.state == other.state
    }

    /// Complete carrier: increment coordinates followed by decrement coordinates.
    #[wasm_bindgen(js_name = state)]
    pub fn state(&self) -> Vec<u64> {
        self.state
            .p_state()
            .iter()
            .chain(self.state.n_state())
            .copied()
            .collect()
    }
}

impl SafeMeshPnCounterReplica {
    pub fn new(replica_id: u64, replicas: usize) -> Self {
        SafeMeshPnCounterReplica {
            replica_id,
            state: PnCounter::new(replicas),
            log: EventLog::with_replica_count(replicas),
        }
    }

    pub fn append_inc(&mut self, counter_replica: usize, tally: u64) -> Result<Vec<u8>, JsValue> {
        if safemesh_crdt::ownership::check_counter_record(
            self.state.p_state().len(),
            safemesh_crdt::RecordId {
                replica: self.replica_id,
                sequence: 1,
            },
            &PnCounterDelta::Inc {
                replica: counter_replica,
                tally,
            },
        )
        .is_err()
        {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        let delta = PnCounterDelta::Inc {
            replica: counter_replica,
            tally,
        };
        let id = self
            .log
            .append_with(
                &mut self.state,
                self.replica_id,
                delta.clone(),
                |state, delta| {
                    state.apply_delta(delta.clone());
                },
            )
            .map_err(|error| JsValue::from(append_error(error)))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn append_dec(&mut self, counter_replica: usize, tally: u64) -> Result<Vec<u8>, JsValue> {
        if safemesh_crdt::ownership::check_counter_record(
            self.state.p_state().len(),
            safemesh_crdt::RecordId {
                replica: self.replica_id,
                sequence: 1,
            },
            &PnCounterDelta::Dec {
                replica: counter_replica,
                tally,
            },
        )
        .is_err()
        {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        let delta = PnCounterDelta::Dec {
            replica: counter_replica,
            tally,
        };
        let id = self
            .log
            .append_with(
                &mut self.state,
                self.replica_id,
                delta.clone(),
                |state, delta| {
                    state.apply_delta(delta.clone());
                },
            )
            .map_err(|error| JsValue::from(append_error(error)))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }
}

/// State-based WASM replica using the core's canonical full-carrier wire codec.
/// Operations return snapshots, not event-log records.
#[wasm_bindgen]
#[derive(Default)]
pub struct SafeMeshGSetReplica {
    state: GSet<u64>,
}

#[wasm_bindgen]
impl SafeMeshGSetReplica {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { state: GSet::new() }
    }
    #[wasm_bindgen(js_name = insert)]
    pub fn insert(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        self.state.insert(checked_u64(value, "value")?);
        self.state_bytes()
    }

    #[wasm_bindgen(js_name = stateBytes)]
    pub fn state_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.state
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode state"))
    }
    #[wasm_bindgen(js_name = mergeStateBytes)]
    pub fn merge_state_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<(), JsValue> {
        let other = <GSet<u64>>::from_wire_bytes_with_limits(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(|error| wire_decode_error(error, "failed to decode state"))?;
        self.state.merge(&other);
        Ok(())
    }
}

/// State-based WASM replica using the core's canonical full-carrier wire codec.
/// Operations return snapshots, not event-log records.
#[wasm_bindgen]
#[derive(Default)]
pub struct SafeMeshRgaReplica {
    state: Rga<u64, u64>,
}

#[wasm_bindgen]
impl SafeMeshRgaReplica {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { state: Rga::new() }
    }
    #[wasm_bindgen(js_name = insert)]
    pub fn insert(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] position: JsValue,
        #[wasm_bindgen(unchecked_param_type = "bigint")] value: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let position = checked_u64(position, "position")?;
        let value = checked_u64(value, "value")?;
        self.state.insert(position, value);
        self.state_bytes()
    }
    #[wasm_bindgen(js_name = delete)]
    pub fn delete(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] position: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        self.state.delete(checked_u64(position, "position")?);
        self.state_bytes()
    }

    #[wasm_bindgen(js_name = stateBytes)]
    pub fn state_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.state
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode state"))
    }
    #[wasm_bindgen(js_name = mergeStateBytes)]
    pub fn merge_state_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<u32>,
    ) -> Result<(), JsValue> {
        let other = <Rga<u64, u64>>::from_wire_bytes_with_limits(
            bytes,
            collection_limits(max_collection_elements),
        )
        .map_err(|error| wire_decode_error(error, "failed to decode state"))?;
        self.state.merge(&other);
        Ok(())
    }
}
// Managed Node persistence: the envelope is local storage, never peer bytes.
#[wasm_bindgen(typescript_custom_section)]
const MANAGED_STORE_TYPES: &str = r#"
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
"#;

#[wasm_bindgen(inline_js = r#"
export function managedError(code, message) {
    const error = new Error(message); error.name = 'SafeMeshStoreError'; error.code = code;
    return error;
}
function sync(value) {
    if (value != null && typeof value.then === 'function')
        throw managedError('COMMIT', 'Store callbacks must be synchronous');
    return value;
}
export function managedMode(options) {
    if (options.mode !== 'fresh' && options.mode !== 'restart')
        throw managedError('CORRUPT', 'mode must be fresh or restart');
    if (options.mode === 'restart' && options.writers !== undefined)
        throw managedError('CORRUPT', 'restart obtains writers only from committed metadata');
    return options.mode;
}
export function managedWriter(options) {
    if (typeof options.writer !== 'bigint' || options.writer < 0n || options.writer > 18446744073709551615n)
        throw managedError('CORRUPT', 'writer must be u64 bigint');
    return options.writer;
}
export function managedWriters(options) {
    if (!Number.isSafeInteger(options.writers) || options.writers < 1 || options.writers > 1000000)
        throw managedError('CORRUPT', 'writers must be an integer in 1..1000000');
    return options.writers;
}
export function managedOpen(store, mode, writer) { return sync(store.open(mode, writer)); }
export function managedRead(store, lease) {
    const snapshot = sync(store.readCommitted(lease));
    if (!snapshot || !(snapshot.bytes instanceof Uint8Array) || snapshot.bytes.length < 41 ||
        typeof snapshot.revision !== 'bigint' || typeof snapshot.anchor !== 'bigint')
        throw managedError('CORRUPT', 'invalid committed snapshot');
    const revision = new DataView(snapshot.bytes.buffer, snapshot.bytes.byteOffset, snapshot.bytes.byteLength).getBigUint64(33, true);
    if (revision !== snapshot.revision || revision < 1n || snapshot.anchor !== revision)
        throw managedError('STALE', 'envelope revision differs from independent store anchor');
    return Array.from(snapshot.bytes, b => b.toString(16).padStart(2, '0')).join('');
}
export function managedCommit(store, lease, revision, bytes) {
    const next = sync(store.commit(lease, revision, Uint8Array.from(bytes)));
    if (typeof next !== 'bigint' || next !== revision + 1n)
        throw managedError('COMMIT', 'Store.commit returned wrong revision');
}
export function managedClose(store, lease) { sync(store.close(lease)); }
"#)]
extern "C" {
    #[wasm_bindgen(js_name = managedError)]
    fn managed_error(code: &str, message: &str) -> JsValue;
    #[wasm_bindgen(catch, js_name = managedMode)]
    fn managed_mode(options: &JsValue) -> Result<String, JsValue>;
    #[wasm_bindgen(catch, js_name = managedWriter)]
    fn managed_writer(options: &JsValue) -> Result<u64, JsValue>;
    #[wasm_bindgen(catch, js_name = managedWriters)]
    fn managed_writers(options: &JsValue) -> Result<u32, JsValue>;
    #[wasm_bindgen(catch, js_name = managedOpen)]
    fn managed_open(store: &JsValue, mode: &str, writer: u64) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch, js_name = managedRead)]
    fn managed_read(store: &JsValue, lease: &JsValue) -> Result<String, JsValue>;
    #[wasm_bindgen(catch, js_name = managedCommit)]
    fn managed_commit(
        store: &JsValue,
        lease: &JsValue,
        revision: u64,
        bytes: &[u8],
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(catch, js_name = managedClose)]
    fn managed_close(store: &JsValue, lease: &JsValue) -> Result<(), JsValue>;
}

struct ManagedCounterState {
    counter: SafeMeshGCounterReplica,
    revision: u64,
    closed: bool,
    disabled: bool,
}

/// Owns a synchronous Store lease. Call close explicitly before free.
/// Instance methods implemented in Rust borrow through try_borrow: callback reentry
/// is rejected as REENTRY before touching state, including read/close during commit.
/// The wasm-bindgen-generated free() is not guarded by try_borrow. Calling free()
/// during commit throws a wasm-bindgen ownership error and invalidates the JS handle;
/// subsequent instance methods on that handle throw a null-pointer error. Do not
/// call free() from a Store callback; close the handle before freeing it.
#[wasm_bindgen]
pub struct SafeMeshManagedGCounter {
    store: JsValue,
    lease: JsValue,
    inner: RefCell<ManagedCounterState>,
}

fn managed_envelope(counter: &SafeMeshGCounterReplica, revision: u64) -> Result<Vec<u8>, JsValue> {
    let mut bytes = b"SMNODEGC".to_vec();
    bytes.push(1); // envelope version; magic identifies the G-Counter kind
    bytes.extend_from_slice(&(counter.replica.state().len() as u64).to_le_bytes());
    bytes.extend_from_slice(&counter.replica_id.to_le_bytes());
    bytes.extend_from_slice(&counter.version_for(counter.replica_id).to_le_bytes());
    bytes.extend_from_slice(&revision.to_le_bytes());
    bytes.extend_from_slice(&counter.log_bytes()?);
    Ok(bytes)
}

fn managed_restore(bytes: &[u8], writer: u64) -> Result<(SafeMeshGCounterReplica, u64), JsValue> {
    let corrupt = || managed_error("CORRUPT", "invalid managed counter envelope or replay");
    if bytes.len() < 41 || &bytes[..8] != b"SMNODEGC" || bytes[8] != 1 {
        return Err(corrupt());
    }
    let word = |offset| u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
    let count = word(9);
    let author = word(17);
    let cursor = word(25);
    let revision = word(33);
    if count == 0 || count > 1_000_000 || author != writer || author >= count || revision == 0 {
        return Err(corrupt());
    }
    let mut counter = SafeMeshGCounterReplica::new(author, count as usize);
    let verdicts = counter
        .merge_log_bytes(&bytes[41..], None, None)
        .map_err(|_| corrupt())?;
    if verdicts.iter().any(|v| v != "accepted") || counter.version_for(author) != cursor {
        return Err(corrupt());
    }
    // Canonical encoding also refuses omitted history, trailing or alternate encodings.
    if managed_envelope(&counter, revision)? != bytes {
        return Err(corrupt());
    }
    Ok((counter, revision))
}

impl SafeMeshManagedGCounter {
    fn borrow(&self, write: bool) -> Result<std::cell::RefMut<'_, ManagedCounterState>, JsValue> {
        let inner = self
            .inner
            .try_borrow_mut()
            .map_err(|_| managed_error("REENTRY", "Store callback reentry"))?;
        if inner.closed {
            return Err(managed_error("CLOSED", "managed handle is closed"));
        }
        if write && inner.disabled {
            return Err(managed_error(
                "DISABLED",
                "close and restart after failed commit",
            ));
        }
        Ok(inner)
    }
    fn candidate(inner: &ManagedCounterState) -> Result<SafeMeshGCounterReplica, JsValue> {
        let bytes = managed_envelope(&inner.counter, inner.revision)?;
        Ok(managed_restore(&bytes, inner.counter.replica_id)?.0)
    }
    fn publish(
        &self,
        inner: &mut ManagedCounterState,
        candidate: SafeMeshGCounterReplica,
    ) -> Result<(), JsValue> {
        let next = inner
            .revision
            .checked_add(1)
            .ok_or_else(|| managed_error("COMMIT", "revision exhausted"))?;
        let bytes = managed_envelope(&candidate, next)?;
        if let Err(error) = managed_commit(&self.store, &self.lease, inner.revision, &bytes) {
            inner.disabled = true;
            return Err(error);
        }
        inner.counter = candidate;
        inner.revision = next;
        Ok(())
    }
}

#[wasm_bindgen]
impl SafeMeshManagedGCounter {
    #[wasm_bindgen(js_name = open)]
    pub fn open(
        #[wasm_bindgen(unchecked_param_type = "SafeMeshStore")] store: JsValue,
        #[wasm_bindgen(unchecked_param_type = "SafeMeshManagedCounterOptions")] options: JsValue,
    ) -> Result<SafeMeshManagedGCounter, JsValue> {
        let mode = managed_mode(&options)?;
        let writer = managed_writer(&options)?;
        let count = if mode == "fresh" {
            Some(managed_writers(&options)?)
        } else {
            None
        };
        if count.is_some_and(|n| writer >= u64::from(n)) {
            return Err(managed_error("CORRUPT", "writer outside committed count"));
        }
        let lease = managed_open(&store, &mode, writer)?;
        let result = (|| {
            if let Some(count) = count {
                let counter = SafeMeshGCounterReplica::new(writer, count as usize);
                managed_commit(&store, &lease, 0, &managed_envelope(&counter, 1)?)?;
                Ok((counter, 1))
            } else {
                let hex = managed_read(&store, &lease)?;
                let bytes: Result<Vec<u8>, _> = hex
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16))
                    .collect();
                managed_restore(
                    &bytes.map_err(|_| managed_error("CORRUPT", "invalid snapshot bytes"))?,
                    writer,
                )
            }
        })();
        match result {
            Ok((counter, revision)) => Ok(Self {
                store,
                lease,
                inner: RefCell::new(ManagedCounterState {
                    counter,
                    revision,
                    closed: false,
                    disabled: false,
                }),
            }),
            Err(error) => {
                let _ = managed_close(&store, &lease);
                Err(error)
            }
        }
    }

    #[wasm_bindgen(js_name = appendBump)]
    pub fn append_bump(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] tally: JsValue,
    ) -> Result<Vec<u8>, JsValue> {
        let tally = checked_u64(tally, "tally")?;
        let mut inner = self.borrow(true)?;
        let mut candidate = Self::candidate(&inner)?;
        let bytes = candidate.append_bump(candidate.replica_id as usize, tally)?;
        self.publish(&mut inner, candidate)?;
        Ok(bytes)
    }

    #[wasm_bindgen(js_name = mergeRecordBytes, unchecked_return_type = "\"accepted\" | \"duplicate\"")]
    pub fn merge_record_bytes(&self, bytes: &[u8]) -> Result<String, JsValue> {
        let mut inner = self.borrow(true)?;
        let mut candidate = Self::candidate(&inner)?;
        let verdict = candidate.merge_record_bytes(bytes, None)?;
        match verdict.as_str() {
            "accepted" => self.publish(&mut inner, candidate)?,
            "duplicate" => (),
            _ => return Err(managed_error("COLLISION", "peer record collision")),
        }
        Ok(verdict)
    }

    #[wasm_bindgen(js_name = mergeLogBytes, unchecked_return_type = "(\"accepted\" | \"duplicate\")[]")]
    pub fn merge_log_bytes(&self, bytes: &[u8]) -> Result<Vec<String>, JsValue> {
        let mut inner = self.borrow(true)?;
        let mut candidate = Self::candidate(&inner)?;
        let verdicts = candidate.merge_log_bytes(bytes, None, None)?;
        if verdicts.iter().any(|v| v == "collision") {
            return Err(managed_error("COLLISION", "peer batch collision"));
        }
        if verdicts.iter().any(|v| v == "accepted") {
            self.publish(&mut inner, candidate)?;
        }
        Ok(verdicts)
    }

    #[wasm_bindgen(unchecked_return_type = "bigint")]
    pub fn value(&self) -> Result<JsValue, JsValue> {
        Ok(self.borrow(false)?.counter.value())
    }
    pub fn state(&self) -> Result<Vec<u64>, JsValue> {
        Ok(self.borrow(false)?.counter.state())
    }
    #[wasm_bindgen(js_name = peerLogBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.borrow(false)?.counter.log_bytes()
    }
    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for(
        &self,
        #[wasm_bindgen(unchecked_param_type = "bigint")] writer: JsValue,
    ) -> Result<u64, JsValue> {
        Ok(self
            .borrow(false)?
            .counter
            .version_for(checked_u64(writer, "writer")?))
    }
    pub fn close(&self) -> Result<(), JsValue> {
        let mut inner = self
            .inner
            .try_borrow_mut()
            .map_err(|_| managed_error("REENTRY", "Store callback reentry"))?;
        if !inner.closed {
            // A throwing close may have released the lease: retain no write authority.
            inner.disabled = true;
            managed_close(&self.store, &self.lease)?;
            inner.closed = true;
        }
        Ok(())
    }
}
