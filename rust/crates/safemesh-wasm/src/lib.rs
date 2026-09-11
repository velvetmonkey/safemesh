// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use safemesh_crdt::ownership::{allocate_token, WriterConfig};
use safemesh_crdt::{
    Crdt, EnableWinsFlag, EnableWinsFlagDelta, EventLog, GCounter, GCounterDelta, LwwMap,
    LwwMapDelta, LwwRegister, LwwRegisterDelta, OrSet, OrSetDelta, Record, WireDecode, WireEncode,
};
use std::{cell::RefCell, collections::BTreeSet};
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
"#)]
extern "C" {
    pub type SafeMeshError;

    #[wasm_bindgen(js_name = installOrSetSelfMergeGuard)]
    fn install_orset_self_merge_guard(sample: JsValue);

    #[wasm_bindgen(constructor)]
    fn new(code: u32, message: &str) -> SafeMeshError;

    #[wasm_bindgen(catch, js_name = checkedTokens)]
    fn checked_tokens(value: JsValue) -> Result<Vec<u64>, JsValue>;
}

// Exporting the sample uses wasm-bindgen's own class wrapper on every target.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn initialize_bindings() {
    install_orset_self_merge_guard(SafeMeshOrSet::new().into());
}

fn safe_mesh_error(code: u32, message: &str) -> JsValue {
    SafeMeshError::new(code, message).into()
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

fn admission_name(admission: safemesh_crdt::Admission) -> String {
    match admission {
        safemesh_crdt::Admission::Accepted => "accepted",
        safemesh_crdt::Admission::Duplicate => "duplicate",
        safemesh_crdt::Admission::Collision => "collision",
    }
    .to_owned()
}

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
        let replicas = checked_index(replicas, "replicas")?;
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
        Ok(self.set(timestamp, replica, value))
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
        Ok(self.set(key, timestamp, replica, value))
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
        Ok(self.remove(key, timestamp, replica))
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
        Ok(self.enable(token))
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
    state: GCounter,
    log: EventLog<GCounterDelta>,
}

#[wasm_bindgen]
impl SafeMeshGCounterReplica {
    #[wasm_bindgen(constructor)]
    pub fn new_js(
        #[wasm_bindgen(unchecked_param_type = "bigint")] replica_id: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] replicas: JsValue,
    ) -> Result<Self, JsValue> {
        let replica_id = checked_u64(replica_id, "replica_id")?;
        let replicas = checked_index(replicas, "replicas")?;
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

    #[wasm_bindgen(js_name = mergeRecordBytes)]
    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let record = Record::<GCounterDelta>::from_wire_bytes(bytes)
            .map_err(|_| safe_mesh_error(1, "failed to decode record"))?;
        if safemesh_crdt::ownership::check_counter_record(
            self.state.len(),
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
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(safe_mesh_error(1, "record ID collision"));
        }
        Ok(())
    }

    /// Return one core admission verdict for every decoded input record.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<GCounterDelta>::from_wire_bytes_for(bytes, &self.state).map_err(
            |error| {
                safe_mesh_error(
                    1,
                    match error {
                        safemesh_crdt::WireError::RecordCollision => "record ID collision",
                        safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                            "replica count mismatch"
                        }
                        safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                        safemesh_crdt::WireError::MissingShape => "event log missing shape",
                        _ => "failed to decode event log",
                    },
                )
            },
        )?;
        if log.records().iter().any(|r| {
            safemesh_crdt::ownership::check_counter_record(self.state.len(), r.id, &r.delta)
                .is_err()
        }) {
            return Err(safe_mesh_error(
                2,
                "counter coordinate out of range or not owned by record author",
            ));
        }
        Ok(log
            .records()
            .iter()
            .cloned()
            .map(|record| {
                self.log.admit_with(record, |delta| {
                    self.state.apply_delta(delta.clone());
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
        exact_bigint(self.total())
    }

    /// The Rust-core total behind [`Self::value`]; not exported, so host-side
    /// tests can read it without constructing a JavaScript value.
    fn total(&self) -> u128 {
        self.state.value()
    }

    /// Compare the Rust-core carrier states without reproducing its equality in JavaScript.
    #[wasm_bindgen(js_name = sameStateAs)]
    pub fn same_state_as(&self, other: &SafeMeshGCounterReplica) -> bool {
        self.state == other.state
    }

    #[wasm_bindgen(js_name = state)]
    pub fn state(&self) -> Vec<u64> {
        self.state.state().to_vec()
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
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| safe_mesh_error(1, "event log sequence exhausted"))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    #[wasm_bindgen(js_name = mergeRecordBytes)]
    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let record = Record::<EnableWinsFlagDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| safe_mesh_error(1, "failed to decode record"))?;
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(safe_mesh_error(1, "record ID collision"));
        }
        Ok(())
    }

    /// Return one core admission verdict for every decoded input record.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<EnableWinsFlagDelta<u64>>::from_wire_bytes_for(bytes, &self.state)
            .map_err(|error| {
                safe_mesh_error(
                    1,
                    match error {
                        safemesh_crdt::WireError::RecordCollision => "record ID collision",
                        safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                            "replica count mismatch"
                        }
                        safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                        safemesh_crdt::WireError::MissingShape => "event log missing shape",
                        _ => "failed to decode event log",
                    },
                )
            })?;
        Ok(log
            .records()
            .iter()
            .cloned()
            .map(|record| {
                self.log.admit_with(record, |delta| {
                    self.state.apply_delta(delta.clone());
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

    #[wasm_bindgen(js_name = mergeRecordBytes)]
    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let record = Record::<LwwMapDelta<u64, u64>>::from_wire_bytes(bytes)
            .map_err(|_| safe_mesh_error(1, "failed to decode record"))?;
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(safe_mesh_error(1, "record ID collision"));
        }
        Ok(())
    }

    /// Return one core admission verdict for every decoded input record.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<LwwMapDelta<u64, u64>>::from_wire_bytes_for(bytes, &self.state)
            .map_err(|error| {
                safe_mesh_error(
                    1,
                    match error {
                        safemesh_crdt::WireError::RecordCollision => "record ID collision",
                        safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                            "replica count mismatch"
                        }
                        safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                        safemesh_crdt::WireError::MissingShape => "event log missing shape",
                        _ => "failed to decode event log",
                    },
                )
            })?;
        Ok(log
            .records()
            .iter()
            .cloned()
            .map(|record| {
                self.log.admit_with(record, |delta| {
                    self.state.apply_delta(delta.clone());
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

    #[wasm_bindgen(js_name = mergeRecordBytes)]
    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let record = Record::<LwwRegisterDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| safe_mesh_error(1, "failed to decode record"))?;
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(safe_mesh_error(1, "record ID collision"));
        }
        Ok(())
    }

    /// Return one core admission verdict for every decoded input record.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>, JsValue> {
        let log = EventLog::<LwwRegisterDelta<u64>>::from_wire_bytes_for(bytes, &self.state)
            .map_err(|error| {
                safe_mesh_error(
                    1,
                    match error {
                        safemesh_crdt::WireError::RecordCollision => "record ID collision",
                        safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                            "replica count mismatch"
                        }
                        safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                        safemesh_crdt::WireError::MissingShape => "event log missing shape",
                        _ => "failed to decode event log",
                    },
                )
            })?;
        Ok(log
            .records()
            .iter()
            .cloned()
            .map(|record| {
                self.log.admit_with(record, |delta| {
                    self.state.apply_delta(delta.clone());
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
        Ok(self.add(element, token))
    }
    /// Tombstone tokens globally, including tokens whose adds have not arrived yet.
    #[wasm_bindgen(js_name = applyRemove)]
    pub fn apply_remove_js(
        &mut self,
        #[wasm_bindgen(unchecked_param_type = "BigUint64Array")] tokens: JsValue,
    ) -> Result<(), JsValue> {
        let tokens = checked_tokens(tokens)?;
        Ok(self.apply_remove(tokens))
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
    binding_error(
        1,
        match error {
            safemesh_crdt::WireError::RecordCollision => "record ID collision".to_string(),
            safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                "replica count mismatch".to_string()
            }
            safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch".to_string(),
            safemesh_crdt::WireError::MissingShape => "event log missing shape".to_string(),
            other => format!("failed to decode event log: {other:?}"),
        },
    )
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
    state: OrSet<String, u64>,
    log: EventLog<OrSetDelta<String, u64>>,
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
                && !self.log.records().iter().any(|known| known.id == record.id)
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
        bytes.extend(
            self.log.to_wire_bytes().map_err(|error| {
                binding_error(1, format!("failed to encode event log: {error:?}"))
            })?,
        );
        Ok(bytes)
    }

    fn try_import_identity(bytes: &[u8]) -> Result<Self, BindingError> {
        if bytes.len() < 29 || &bytes[..5] != b"SMOI\x01" {
            return Err(binding_error(
                1,
                "allocation/history consistency: invalid identity storage",
            ));
        }
        let word = |i| u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        let (writers, author, next) = (word(5), word(13), word(21));
        let mut candidate = Self::new(author);
        candidate.log = EventLog::from_wire_bytes_for(&bytes[29..], &candidate.state)
            .map_err(event_log_decode_error)?;
        if candidate.checked_next(writers)? != next {
            return Err(binding_error(
                1,
                "allocation/history consistency: next sequence mismatch",
            ));
        }
        for record in candidate.log.records() {
            candidate.state.apply_delta(record.delta.clone());
        }
        // Claim only after all checks; a failed import creates no live writer.
        candidate.claim(writers)?;
        Ok(candidate)
    }

    fn append(&mut self, delta: OrSetDelta<String, u64>) -> Result<Vec<u8>, BindingError> {
        let id = self
            .log
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| binding_error(1, "event log sequence exhausted"))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|error| binding_error(1, format!("failed to encode record: {error:?}")))
    }

    fn admit(
        &mut self,
        record: Record<OrSetDelta<String, u64>>,
    ) -> Result<safemesh_crdt::Admission, BindingError> {
        self.check_incoming(&record)?;
        match self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) {
            safemesh_crdt::Admission::Collision => Err(binding_error(1, "record ID collision")),
            admission => Ok(admission),
        }
    }

    fn decode_record(bytes: &[u8]) -> Result<Record<OrSetDelta<String, u64>>, BindingError> {
        Record::<OrSetDelta<String, u64>>::from_wire_bytes(bytes)
            .map_err(|error| binding_error(1, format!("failed to decode record: {error:?}")))
    }

    fn try_merge_record_bytes(&mut self, bytes: &[u8]) -> Result<&'static str, BindingError> {
        let record = Self::decode_record(bytes)?;
        Ok(match self.admit(record)? {
            safemesh_crdt::Admission::Accepted => "accepted",
            safemesh_crdt::Admission::Duplicate => "duplicate",
            safemesh_crdt::Admission::Collision => unreachable!("admit maps collision to Err"),
        })
    }

    fn try_merge_log_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>, BindingError> {
        let log = EventLog::<OrSetDelta<String, u64>>::from_wire_bytes_for(bytes, &self.state)
            .map_err(event_log_decode_error)?;
        for record in log.records() {
            self.check_incoming(record)?;
        }
        Ok(log
            .records()
            .iter()
            .cloned()
            .map(|record| {
                self.log.admit_with(record, |delta| {
                    self.state.apply_delta(delta.clone());
                })
            })
            .map(admission_name)
            .collect())
    }

    fn try_inspect_record_bytes(bytes: &[u8]) -> Result<SafeMeshStringOrSetRecord, BindingError> {
        let Record { id, delta } = Self::decode_record(bytes)?;
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
    pub fn import_identity(bytes: &[u8]) -> Result<Self, JsValue> {
        Self::try_import_identity(bytes).map_err(JsValue::from)
    }

    /// Append an add record for `(element, token)` and return its wire bytes.
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
        let tokens = self.state.observed_tokens(&element).into_iter().collect();
        self.append(OrSetDelta::Remove { tokens })
            .map_err(JsValue::from)
    }

    /// Decode one record and admit it through the core event log.
    ///
    /// Returns the core's admission verdict: `"accepted"` when the record was
    /// new and applied, `"duplicate"` when a record with the same identity and
    /// payload was already in the log (state does not move). A record whose
    /// identity is known but whose payload differs throws `record ID collision`.
    #[wasm_bindgen(
        js_name = mergeRecordBytes,
        unchecked_return_type = "\"accepted\" | \"duplicate\""
    )]
    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> Result<String, JsValue> {
        self.try_merge_record_bytes(bytes)
            .map(str::to_string)
            .map_err(JsValue::from)
    }

    /// Return one core admission verdict for every decoded input record.
    #[wasm_bindgen(
        js_name = mergeLogBytes,
        unchecked_return_type = "(\"accepted\" | \"duplicate\" | \"collision\")[]"
    )]
    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>, JsValue> {
        self.try_merge_log_bytes(bytes).map_err(JsValue::from)
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.log
            .to_wire_bytes()
            .map_err(|error| safe_mesh_error(1, &format!("failed to encode event log: {error:?}")))
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
        self.state.elements().into_iter().collect()
    }

    /// Every token ever added for `element`, including tombstoned ones.
    #[wasm_bindgen(js_name = observedTokens)]
    pub fn observed_tokens(&self, element: String) -> Vec<u64> {
        self.state.observed_tokens(&element).into_iter().collect()
    }

    pub fn tombstones(&self) -> Vec<u64> {
        self.state.tombstones().iter().copied().collect()
    }

    /// Every `(element, token)` add pair the core holds, tombstoned or not.
    #[wasm_bindgen(js_name = addEntries)]
    pub fn add_entries(&self) -> Vec<SafeMeshStringOrSetAddEntry> {
        self.state
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
    pub fn inspect_record_bytes(bytes: &[u8]) -> Result<SafeMeshStringOrSetRecord, JsValue> {
        Self::try_inspect_record_bytes(bytes).map_err(JsValue::from)
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
            state: GCounter::new(replicas),
            log: EventLog::with_replica_count(replicas),
        }
    }

    pub fn append_bump(&mut self, counter_replica: usize, tally: u64) -> Result<Vec<u8>, JsValue> {
        if safemesh_crdt::ownership::check_counter_record(
            self.state.len(),
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
        let id = self
            .log
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| safe_mesh_error(1, "event log sequence exhausted"))?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| safe_mesh_error(1, "failed to encode record"))
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
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
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| safe_mesh_error(1, "event log sequence exhausted"))?;
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
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| safe_mesh_error(1, "event log sequence exhausted"))?;
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
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| safe_mesh_error(1, "event log sequence exhausted"))?;
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
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| safe_mesh_error(1, "event log sequence exhausted"))?;
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
            state: OrSet::new(),
            log: EventLog::new(),
        }
    }

    pub fn append_add(&mut self, element: String, token: u64) -> Result<Vec<u8>, JsValue> {
        self.append(OrSetDelta::Add { element, token })
            .map_err(JsValue::from)
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use safemesh_crdt::{RecordId, WireEncode};

    #[test]
    fn allocated_identity_checks_history_and_restarts() {
        let mut left = SafeMeshStringOrSetReplica::try_create_allocated(2, 0).unwrap();
        left.try_append_allocated_add("water".into()).unwrap();
        let saved = left.try_export_identity().unwrap();
        assert!(SafeMeshStringOrSetReplica::try_import_identity(&saved).is_err());
        drop(left);
        for (offset, word) in [(5, 0u64), (5, 3), (13, 1), (21, 0), (21, u64::MAX)] {
            let mut bad = saved.clone();
            bad[offset..offset + 8].copy_from_slice(&word.to_le_bytes());
            assert!(SafeMeshStringOrSetReplica::try_import_identity(&bad).is_err());
        }
        let mut restored = SafeMeshStringOrSetReplica::try_import_identity(&saved).unwrap();
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
    }

    #[test]
    fn allocated_history_refuses_gaps_zero_and_max_sequence() {
        for sequence in [0, 2, u64::MAX] {
            let mut replica = SafeMeshStringOrSetReplica::new(0);
            replica.log.insert_record(Record {
                id: RecordId {
                    replica: 0,
                    sequence,
                },
                delta: OrSetDelta::Remove { tokens: vec![] },
            });
            assert!(replica.checked_next(1).is_err());
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

        right.merge_record_bytes(&bytes).unwrap();
        right.merge_record_bytes(&bytes).unwrap();
        assert_eq!(right.total(), 5);
        assert_eq!(right.version_for(1), 1);

        left.merge_log_bytes(&right.log_bytes().unwrap()).unwrap();
        assert_eq!(left.total(), right.total());
    }

    #[test]
    fn wasm_batch_reports_every_admission() {
        fn wire(records: impl IntoIterator<Item = Record<GCounterDelta>>) -> Vec<u8> {
            let mut log = EventLog::with_replica_count(2);
            for record in records {
                assert_eq!(
                    log.insert_record(record),
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
        target
            .merge_record_bytes(&existing.to_wire_bytes().unwrap())
            .unwrap();
        assert_eq!(
            target.merge_log_bytes(&wire([])).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(target.state(), vec![5, 0]);

        let mut one = SafeMeshGCounterReplica::new(0, 2);
        assert_eq!(
            one.merge_log_bytes(&wire([accepted.clone()])).unwrap(),
            vec!["accepted"]
        );
        assert_eq!(one.state(), vec![0, 7]);

        let before = target.state();
        assert_eq!(
            target.merge_log_bytes(&wire([collision.clone()])).unwrap(),
            vec!["collision"]
        );
        assert_eq!(target.state(), before);

        target
            .merge_record_bytes(&accepted.to_wire_bytes().unwrap())
            .unwrap();
        let before = target.state();
        assert_eq!(
            target
                .merge_log_bytes(&wire([existing.clone(), accepted.clone()]))
                .unwrap(),
            vec!["duplicate", "duplicate"]
        );
        assert_eq!(target.state(), before);

        let mut late = SafeMeshGCounterReplica::new(0, 2);
        late.merge_record_bytes(&existing.to_wire_bytes().unwrap())
            .unwrap();
        let before = late.state();
        let admissions = late
            .merge_log_bytes(&wire([accepted, collision, after_collision]))
            .unwrap();
        let after = late.state();
        println!("WASM admissions={admissions:?} before_state={before:?} after_state={after:?}");
        assert_eq!(admissions, vec!["accepted", "collision", "accepted"]);
        assert_eq!(before, vec![5, 0]);
        assert_eq!(after, vec![5, 8]);
    }

    #[test]
    fn wasm_all_replica_batches_report_collisions() {
        macro_rules! check {
            ($replica:expr, $first:expr, $second:expr) => {{
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
                    .merge_record_bytes(&first.to_wire_bytes().unwrap())
                    .unwrap();
                let state = replica.state.clone();
                let log = replica.log.clone();
                let mut incoming = EventLog::for_crdt(&replica.state);
                assert_eq!(
                    incoming.insert_record(second),
                    safemesh_crdt::Admission::Accepted
                );
                assert_eq!(
                    replica
                        .merge_log_bytes(&incoming.to_wire_bytes().unwrap())
                        .unwrap(),
                    vec!["collision"]
                );
                assert_eq!(replica.state, state);
                assert_eq!(replica.log, log);
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
        let state = replica.state.clone();
        let log = replica.log.clone();
        let mut incoming = EventLog::for_crdt(&replica.state);
        assert_eq!(
            incoming.insert_record(second),
            safemesh_crdt::Admission::Accepted
        );
        assert_eq!(
            replica
                .try_merge_log_bytes(&incoming.to_wire_bytes().unwrap())
                .unwrap(),
            vec!["collision"]
        );
        assert_eq!(replica.state, state);
        assert_eq!(replica.log, log);
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

        right.merge_record_bytes(&enable_10).unwrap();
        right.merge_record_bytes(&enable_10).unwrap();
        assert!(right.value());
        assert_eq!(right.version_for(1), 1);

        let disable_10 = right.append_disable_observed().unwrap();
        assert!(!right.value());

        let enable_11 = left.append_enable(11).unwrap();
        right.merge_record_bytes(&enable_11).unwrap();
        assert!(right.value());

        left.merge_record_bytes(&disable_10).unwrap();
        assert!(left.value());
        left.merge_log_bytes(&right.log_bytes().unwrap()).unwrap();
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

        right.merge_record_bytes(&set_100).unwrap();
        right.merge_record_bytes(&set_100).unwrap();
        assert_eq!(right.value_or(7, 0), 100);
        assert_eq!(right.version_for(1), 1);

        let remove_100 = right.append_remove(7, 11, 2).unwrap();
        assert!(!right.has_key(7));

        let set_300 = left.append_set(7, 12, 1, 300).unwrap();
        right.merge_record_bytes(&set_300).unwrap();
        assert_eq!(right.value_or(7, 0), 300);

        left.merge_record_bytes(&remove_100).unwrap();
        assert_eq!(left.value_or(7, 0), 300);
        left.merge_log_bytes(&right.log_bytes().unwrap()).unwrap();
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

        right.merge_record_bytes(&bytes).unwrap();
        right.merge_record_bytes(&bytes).unwrap();
        right.append_set(10, 2, 200).unwrap();
        assert_eq!(right.value_or(0), 200);
        assert_eq!(right.version_for(1), 1);

        left.merge_log_bytes(&right.log_bytes().unwrap()).unwrap();
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
            reader.log.records().len(),
        );
        let second = reader.try_merge_record_bytes(&bytes).unwrap();
        let after = (
            reader.elements(),
            reader.tombstones(),
            reader.version_for(1),
            reader.log.records().len(),
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

        // Same identity, different payload: the core reports a collision and the
        // binding refuses it rather than absorbing either reading.
        let forged = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: OrSetDelta::Add {
                element: "forged".to_string(),
                token: 99,
            },
        }
        .to_wire_bytes()
        .unwrap();
        let error = reader.try_merge_record_bytes(&forged).unwrap_err();
        println!("C2 same id, different payload: {}", error.message);
        assert_eq!(error.message, "record ID collision");
        assert_eq!(reader.elements(), before.0);
        assert_eq!(reader.log.records().len(), 1);
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
        assert_eq!(error.message, "failed to decode record: InvalidTag");
        assert!(reader.elements().is_empty());
        assert_eq!(reader.log.records().len(), 0);

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
                    assert_eq!(reader.log.records().len(), 0);
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

        // (b) Observed tokens survive removal; the mirror returned [].
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
        assert_eq!(replica.observed_tokens("a".to_string()), vec![7]);
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
            "failed to decode record: InvalidTag"
        );
    }
}
