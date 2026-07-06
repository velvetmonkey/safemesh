// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use safemesh_crdt::{
    EventLog, GCounter, GCounterDelta, LwwRegister, LwwRegisterDelta, Record, WireDecode,
    WireEncode,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct SafeMeshGCounter {
    inner: GCounter,
}

#[wasm_bindgen]
impl SafeMeshGCounter {
    #[wasm_bindgen(constructor)]
    pub fn new(replicas: usize) -> Self {
        SafeMeshGCounter {
            inner: GCounter::new(replicas),
        }
    }

    #[wasm_bindgen(js_name = applyBump)]
    pub fn apply_bump(&mut self, replica: usize, tally: u64) {
        self.inner.apply_bump(replica, tally);
    }

    pub fn value(&self) -> u64 {
        self.inner.value()
    }

    #[wasm_bindgen(js_name = state)]
    pub fn state(&self) -> Vec<u64> {
        self.inner.state().to_vec()
    }
}

#[wasm_bindgen(js_name = gcounterDeltaToWire)]
pub fn gcounter_delta_to_wire(replica: usize, tally: u64) -> Result<Vec<u8>, JsValue> {
    GCounterDelta { replica, tally }
        .to_wire_bytes()
        .map_err(|_| JsValue::from_str("failed to encode G-Counter delta"))
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

    pub fn set(&mut self, timestamp: u64, replica: u64, value: u64) {
        self.inner.set(timestamp, replica, value);
    }

    #[wasm_bindgen(js_name = hasValue)]
    pub fn has_value(&self) -> bool {
        self.inner.value().is_some()
    }

    #[wasm_bindgen(js_name = valueOr)]
    pub fn value_or(&self, default_value: u64) -> u64 {
        self.inner.value().copied().unwrap_or(default_value)
    }

    #[wasm_bindgen(js_name = timestampOr)]
    pub fn timestamp_or(&self, default_value: u64) -> u64 {
        self.inner
            .entry()
            .map(|entry| entry.dot.timestamp)
            .unwrap_or(default_value)
    }

    #[wasm_bindgen(js_name = writerReplicaOr)]
    pub fn writer_replica_or(&self, default_value: u64) -> u64 {
        self.inner
            .entry()
            .map(|entry| entry.dot.replica)
            .unwrap_or(default_value)
    }
}

#[wasm_bindgen(js_name = lwwRegisterDeltaToWire)]
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
    .map_err(|_| JsValue::from_str("failed to encode LWW register delta"))
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
    pub fn new(replica_id: u64, replicas: usize) -> Self {
        SafeMeshGCounterReplica {
            replica_id,
            state: GCounter::new(replicas),
            log: EventLog::new(),
        }
    }

    #[wasm_bindgen(js_name = appendBump)]
    pub fn append_bump(&mut self, counter_replica: usize, tally: u64) -> Result<Vec<u8>, JsValue> {
        let delta = GCounterDelta {
            replica: counter_replica,
            tally,
        };
        self.state.apply_bump(delta.replica, delta.tally);
        let id = self.log.append(self.replica_id, delta.clone());
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| JsValue::from_str("failed to encode record"))
    }

    #[wasm_bindgen(js_name = mergeRecordBytes)]
    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let record = Record::<GCounterDelta>::from_wire_bytes(bytes)
            .map_err(|_| JsValue::from_str("failed to decode record"))?;
        self.state
            .apply_bump(record.delta.replica, record.delta.tally);
        self.log.merge_records([record]);
        Ok(())
    }

    #[wasm_bindgen(js_name = mergeLogBytes)]
    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let log = EventLog::<GCounterDelta>::from_wire_bytes(bytes)
            .map_err(|_| JsValue::from_str("failed to decode event log"))?;
        for record in log.records().iter().cloned() {
            self.state
                .apply_bump(record.delta.replica, record.delta.tally);
            self.log.merge_records([record]);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.log
            .to_wire_bytes()
            .map_err(|_| JsValue::from_str("failed to encode event log"))
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }

    pub fn value(&self) -> u64 {
        self.state.value()
    }

    #[wasm_bindgen(js_name = state)]
    pub fn state(&self) -> Vec<u64> {
        self.state.state().to_vec()
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
    pub fn new(replica_id: u64) -> Self {
        SafeMeshLwwRegisterReplica {
            replica_id,
            state: LwwRegister::new(),
            log: EventLog::new(),
        }
    }

    #[wasm_bindgen(js_name = appendSet)]
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
        self.state.set(delta.timestamp, delta.replica, delta.value);
        let id = self.log.append(self.replica_id, delta.clone());
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| JsValue::from_str("failed to encode record"))
    }

    #[wasm_bindgen(js_name = mergeRecordBytes)]
    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let record = Record::<LwwRegisterDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| JsValue::from_str("failed to decode record"))?;
        self.state.set(
            record.delta.timestamp,
            record.delta.replica,
            record.delta.value,
        );
        self.log.merge_records([record]);
        Ok(())
    }

    #[wasm_bindgen(js_name = mergeLogBytes)]
    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let log = EventLog::<LwwRegisterDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| JsValue::from_str("failed to decode event log"))?;
        for record in log.records().iter().cloned() {
            self.state.set(
                record.delta.timestamp,
                record.delta.replica,
                record.delta.value,
            );
            self.log.merge_records([record]);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = logBytes)]
    pub fn log_bytes(&self) -> Result<Vec<u8>, JsValue> {
        self.log
            .to_wire_bytes()
            .map_err(|_| JsValue::from_str("failed to encode event log"))
    }

    #[wasm_bindgen(js_name = versionFor)]
    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }

    #[wasm_bindgen(js_name = hasValue)]
    pub fn has_value(&self) -> bool {
        self.state.value().is_some()
    }

    #[wasm_bindgen(js_name = valueOr)]
    pub fn value_or(&self, default_value: u64) -> u64 {
        self.state.value().copied().unwrap_or(default_value)
    }

    #[wasm_bindgen(js_name = timestampOr)]
    pub fn timestamp_or(&self, default_value: u64) -> u64 {
        self.state
            .entry()
            .map(|entry| entry.dot.timestamp)
            .unwrap_or(default_value)
    }

    #[wasm_bindgen(js_name = writerReplicaOr)]
    pub fn writer_replica_or(&self, default_value: u64) -> u64 {
        self.state
            .entry()
            .map(|entry| entry.dot.replica)
            .unwrap_or(default_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use safemesh_crdt::{RecordId, WireEncode};

    #[test]
    fn wasm_counter_calls_rust_core() {
        let mut counter = SafeMeshGCounter::new(3);
        counter.apply_bump(1, 5);
        counter.apply_bump(1, 2);
        assert_eq!(counter.value(), 5);
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
        assert_eq!(right.value(), 5);
        assert_eq!(right.version_for(1), 1);

        left.merge_log_bytes(&right.log_bytes().unwrap()).unwrap();
        assert_eq!(left.value(), right.value());
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
}
