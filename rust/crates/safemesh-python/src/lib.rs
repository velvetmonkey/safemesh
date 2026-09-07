// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use safemesh_crdt::{
    Crdt, EnableWinsFlag, EnableWinsFlagDelta, EventLog, GCounter, GCounterDelta, LwwMap,
    LwwMapDelta, LwwRegister, LwwRegisterDelta, OrSet, Record, WireDecode, WireEncode,
};

fn encode_bytes<'py>(
    py: Python<'py>,
    bytes: Result<Vec<u8>, safemesh_crdt::WireError>,
    message: &'static str,
) -> PyResult<Bound<'py, PyBytes>> {
    bytes
        .map(|bytes| PyBytes::new_bound(py, &bytes))
        .map_err(|_| pyo3::exceptions::PyValueError::new_err(message))
}

#[pyclass(name = "GCounter")]
pub struct PyGCounter {
    inner: GCounter,
}

#[pymethods]
impl PyGCounter {
    #[new]
    pub fn new(replicas: usize) -> Self {
        PyGCounter {
            inner: GCounter::new(replicas),
        }
    }

    pub fn apply_bump(&mut self, replica: usize, tally: u64) {
        self.inner.apply_bump(replica, tally);
    }

    /// Apply a coordinate delta, raising IndexError without mutation for a bad index.
    pub fn try_apply_bump(&mut self, replica: usize, tally: u64) -> PyResult<()> {
        self.inner
            .try_apply_bump(replica, tally)
            .map_err(|error| pyo3::exceptions::PyIndexError::new_err(format!("{error:?}")))
    }

    pub fn value(&self) -> u64 {
        self.inner.value()
    }

    pub fn state(&self) -> Vec<u64> {
        self.inner.state().to_vec()
    }
}

#[pyfunction]
pub fn gcounter_delta_to_wire(
    py: Python<'_>,
    replica: usize,
    tally: u64,
) -> PyResult<Bound<'_, PyBytes>> {
    encode_bytes(
        py,
        GCounterDelta { replica, tally }.to_wire_bytes(),
        "failed to encode G-Counter delta",
    )
}

#[pyclass(name = "LwwRegister")]
pub struct PyLwwRegister {
    inner: LwwRegister<u64>,
}

#[pymethods]
impl PyLwwRegister {
    #[new]
    pub fn new() -> Self {
        PyLwwRegister {
            inner: LwwRegister::new(),
        }
    }

    pub fn set(&mut self, timestamp: u64, replica: u64, value: u64) {
        self.inner.set(timestamp, replica, value);
    }

    pub fn has_value(&self) -> bool {
        self.inner.value().is_some()
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

#[pyfunction]
pub fn lww_register_delta_to_wire(
    py: Python<'_>,
    timestamp: u64,
    replica: u64,
    value: u64,
) -> PyResult<Bound<'_, PyBytes>> {
    encode_bytes(
        py,
        LwwRegisterDelta {
            timestamp,
            replica,
            value,
        }
        .to_wire_bytes(),
        "failed to encode LWW register delta",
    )
}

#[pyclass(name = "LwwMap")]
pub struct PyLwwMap {
    inner: LwwMap<u64, u64>,
}

#[pymethods]
impl PyLwwMap {
    #[new]
    pub fn new() -> Self {
        PyLwwMap {
            inner: LwwMap::new(),
        }
    }

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

    pub fn visible_keys(&self) -> Vec<u64> {
        self.inner.value().keys().copied().collect()
    }

    pub fn entry_keys(&self) -> Vec<u64> {
        self.inner.entries().keys().copied().collect()
    }

    pub fn removal_keys(&self) -> Vec<u64> {
        self.inner.removals().keys().copied().collect()
    }
}

#[pyfunction]
pub fn lww_map_set_delta_to_wire(
    py: Python<'_>,
    key: u64,
    timestamp: u64,
    replica: u64,
    value: u64,
) -> PyResult<Bound<'_, PyBytes>> {
    encode_bytes(
        py,
        LwwMapDelta::Set {
            key,
            timestamp,
            replica,
            value,
        }
        .to_wire_bytes(),
        "failed to encode LWW map set delta",
    )
}

#[pyfunction]
pub fn lww_map_remove_delta_to_wire(
    py: Python<'_>,
    key: u64,
    timestamp: u64,
    replica: u64,
) -> PyResult<Bound<'_, PyBytes>> {
    encode_bytes(
        py,
        LwwMapDelta::<u64, u64>::Remove {
            key,
            timestamp,
            replica,
        }
        .to_wire_bytes(),
        "failed to encode LWW map remove delta",
    )
}

#[pyclass(name = "EnableWinsFlag")]
pub struct PyEnableWinsFlag {
    inner: EnableWinsFlag<u64>,
}

#[pymethods]
impl PyEnableWinsFlag {
    #[new]
    pub fn new() -> Self {
        PyEnableWinsFlag {
            inner: EnableWinsFlag::new(),
        }
    }

    pub fn enable(&mut self, token: u64) {
        self.inner.enable(token);
    }

    pub fn disable_observed(&mut self) {
        let tokens = self.inner.observed_tokens();
        self.inner.disable(tokens);
    }

    pub fn value(&self) -> bool {
        self.inner.value()
    }

    pub fn enabled_tokens(&self) -> Vec<u64> {
        self.inner.enables().iter().copied().collect()
    }

    pub fn tombstone_tokens(&self) -> Vec<u64> {
        self.inner.tombstones().iter().copied().collect()
    }
}

#[pyfunction]
pub fn enable_wins_flag_enable_delta_to_wire(
    py: Python<'_>,
    token: u64,
) -> PyResult<Bound<'_, PyBytes>> {
    encode_bytes(
        py,
        EnableWinsFlagDelta::Enable { token }.to_wire_bytes(),
        "failed to encode enable-wins flag enable delta",
    )
}

#[pyfunction]
pub fn enable_wins_flag_disable_delta_to_wire(
    py: Python<'_>,
    tokens: Vec<u64>,
) -> PyResult<Bound<'_, PyBytes>> {
    encode_bytes(
        py,
        EnableWinsFlagDelta::Disable { tokens }.to_wire_bytes(),
        "failed to encode enable-wins flag disable delta",
    )
}

#[pyclass(name = "GCounterReplica")]
pub struct PyGCounterReplica {
    replica_id: u64,
    state: GCounter,
    log: EventLog<GCounterDelta>,
}

#[pymethods]
impl PyGCounterReplica {
    #[new]
    pub fn new(replica_id: u64, replicas: usize) -> Self {
        PyGCounterReplica {
            replica_id,
            state: GCounter::new(replicas),
            log: EventLog::with_replica_count(replicas),
        }
    }

    pub fn append_bump<'py>(
        &mut self,
        py: Python<'py>,
        counter_replica: usize,
        tally: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        if counter_replica >= self.state.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "counter replica out of range",
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
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("event log sequence exhausted"))?;
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let record = Record::<GCounterDelta>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode record"))?;
        if record.delta.replica >= self.state.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "counter replica out of range",
            ));
        }
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "record ID collision",
            ));
        }
        Ok(())
    }

    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let log = EventLog::<GCounterDelta>::from_wire_bytes_for(bytes, &self.state).map_err(
            |error| {
                pyo3::exceptions::PyValueError::new_err(match error {
                    safemesh_crdt::WireError::RecordCollision => "record ID collision",
                    safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                        "replica count mismatch"
                    }
                    safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                    safemesh_crdt::WireError::MissingShape => "event log missing shape",
                    _ => "failed to decode event log",
                })
            },
        )?;
        if log
            .records()
            .iter()
            .any(|r| r.delta.replica >= self.state.len())
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "counter replica out of range",
            ));
        }
        for record in log.records().iter().cloned() {
            if self.log.admit_with(record, |delta| {
                self.state.apply_delta(delta.clone());
            }) == safemesh_crdt::Admission::Collision
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "record ID collision",
                ));
            }
        }
        Ok(())
    }

    pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }

    pub fn value(&self) -> u64 {
        self.state.value()
    }

    pub fn state(&self) -> Vec<u64> {
        self.state.state().to_vec()
    }
}

#[pyclass(name = "EnableWinsFlagReplica")]
pub struct PyEnableWinsFlagReplica {
    replica_id: u64,
    state: EnableWinsFlag<u64>,
    log: EventLog<EnableWinsFlagDelta<u64>>,
}

#[pymethods]
impl PyEnableWinsFlagReplica {
    #[new]
    pub fn new(replica_id: u64) -> Self {
        PyEnableWinsFlagReplica {
            replica_id,
            state: EnableWinsFlag::new(),
            log: EventLog::new(),
        }
    }

    pub fn append_enable<'py>(
        &mut self,
        py: Python<'py>,
        token: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let delta = EnableWinsFlagDelta::Enable { token };
        let id = self
            .log
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("event log sequence exhausted"))?;
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn append_disable_observed<'py>(
        &mut self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let delta = EnableWinsFlagDelta::Disable {
            tokens: self.state.observed_tokens().into_iter().collect(),
        };
        let id = self
            .log
            .append_with(self.replica_id, delta.clone(), |delta| {
                self.state.apply_delta(delta.clone());
            })
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("event log sequence exhausted"))?;
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let record = Record::<EnableWinsFlagDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode record"))?;
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "record ID collision",
            ));
        }
        Ok(())
    }

    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let log = EventLog::<EnableWinsFlagDelta<u64>>::from_wire_bytes_for(bytes, &self.state)
            .map_err(|error| {
                pyo3::exceptions::PyValueError::new_err(match error {
                    safemesh_crdt::WireError::RecordCollision => "record ID collision",
                    safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                        "replica count mismatch"
                    }
                    safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                    safemesh_crdt::WireError::MissingShape => "event log missing shape",
                    _ => "failed to decode event log",
                })
            })?;
        for record in log.records().iter().cloned() {
            if self.log.admit_with(record, |delta| {
                self.state.apply_delta(delta.clone());
            }) == safemesh_crdt::Admission::Collision
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "record ID collision",
                ));
            }
        }
        Ok(())
    }

    pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }

    pub fn value(&self) -> bool {
        self.state.value()
    }

    pub fn enabled_tokens(&self) -> Vec<u64> {
        self.state.enables().iter().copied().collect()
    }

    pub fn tombstone_tokens(&self) -> Vec<u64> {
        self.state.tombstones().iter().copied().collect()
    }
}

#[pyclass(name = "LwwMapReplica")]
pub struct PyLwwMapReplica {
    replica_id: u64,
    state: LwwMap<u64, u64>,
    log: EventLog<LwwMapDelta<u64, u64>>,
}

#[pymethods]
impl PyLwwMapReplica {
    #[new]
    pub fn new(replica_id: u64) -> Self {
        PyLwwMapReplica {
            replica_id,
            state: LwwMap::new(),
            log: EventLog::new(),
        }
    }

    pub fn append_set<'py>(
        &mut self,
        py: Python<'py>,
        key: u64,
        timestamp: u64,
        writer_replica: u64,
        value: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
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
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("event log sequence exhausted"))?;
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn append_remove<'py>(
        &mut self,
        py: Python<'py>,
        key: u64,
        timestamp: u64,
        writer_replica: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
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
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("event log sequence exhausted"))?;
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let record = Record::<LwwMapDelta<u64, u64>>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode record"))?;
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "record ID collision",
            ));
        }
        Ok(())
    }

    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let log = EventLog::<LwwMapDelta<u64, u64>>::from_wire_bytes_for(bytes, &self.state)
            .map_err(|error| {
                pyo3::exceptions::PyValueError::new_err(match error {
                    safemesh_crdt::WireError::RecordCollision => "record ID collision",
                    safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                        "replica count mismatch"
                    }
                    safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                    safemesh_crdt::WireError::MissingShape => "event log missing shape",
                    _ => "failed to decode event log",
                })
            })?;
        for record in log.records().iter().cloned() {
            if self.log.admit_with(record, |delta| {
                self.state.apply_delta(delta.clone());
            }) == safemesh_crdt::Admission::Collision
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "record ID collision",
                ));
            }
        }
        Ok(())
    }

    pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
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

    pub fn visible_keys(&self) -> Vec<u64> {
        self.state.value().keys().copied().collect()
    }

    pub fn entry_keys(&self) -> Vec<u64> {
        self.state.entries().keys().copied().collect()
    }

    pub fn removal_keys(&self) -> Vec<u64> {
        self.state.removals().keys().copied().collect()
    }
}

#[pyclass(name = "LwwRegisterReplica")]
pub struct PyLwwRegisterReplica {
    replica_id: u64,
    state: LwwRegister<u64>,
    log: EventLog<LwwRegisterDelta<u64>>,
}

#[pymethods]
impl PyLwwRegisterReplica {
    #[new]
    pub fn new(replica_id: u64) -> Self {
        PyLwwRegisterReplica {
            replica_id,
            state: LwwRegister::new(),
            log: EventLog::new(),
        }
    }

    pub fn append_set<'py>(
        &mut self,
        py: Python<'py>,
        timestamp: u64,
        writer_replica: u64,
        value: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
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
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("event log sequence exhausted"))?;
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let record = Record::<LwwRegisterDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode record"))?;
        if self.log.admit_with(record, |delta| {
            self.state.apply_delta(delta.clone());
        }) == safemesh_crdt::Admission::Collision
        {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "record ID collision",
            ));
        }
        Ok(())
    }

    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let log = EventLog::<LwwRegisterDelta<u64>>::from_wire_bytes_for(bytes, &self.state)
            .map_err(|error| {
                pyo3::exceptions::PyValueError::new_err(match error {
                    safemesh_crdt::WireError::RecordCollision => "record ID collision",
                    safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
                        "replica count mismatch"
                    }
                    safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch",
                    safemesh_crdt::WireError::MissingShape => "event log missing shape",
                    _ => "failed to decode event log",
                })
            })?;
        for record in log.records().iter().cloned() {
            if self.log.admit_with(record, |delta| {
                self.state.apply_delta(delta.clone());
            }) == safemesh_crdt::Admission::Collision
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "record ID collision",
                ));
            }
        }
        Ok(())
    }

    pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
    }

    pub fn version_for(&self, replica: u64) -> u64 {
        self.log.version().get(replica)
    }

    pub fn has_value(&self) -> bool {
        self.state.value().is_some()
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

#[pymodule]
fn safemesh_python(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGCounter>()?;
    m.add_class::<PyOrSet>()?;
    m.add_class::<PyGCounterReplica>()?;
    m.add_class::<PyLwwRegister>()?;
    m.add_class::<PyLwwRegisterReplica>()?;
    m.add_class::<PyEnableWinsFlag>()?;
    m.add_class::<PyEnableWinsFlagReplica>()?;
    m.add_class::<PyLwwMap>()?;
    m.add_class::<PyLwwMapReplica>()?;
    m.add_function(wrap_pyfunction!(gcounter_delta_to_wire, m)?)?;
    m.add_function(wrap_pyfunction!(lww_register_delta_to_wire, m)?)?;
    m.add_function(wrap_pyfunction!(lww_map_set_delta_to_wire, m)?)?;
    m.add_function(wrap_pyfunction!(lww_map_remove_delta_to_wire, m)?)?;
    m.add_function(wrap_pyfunction!(enable_wins_flag_enable_delta_to_wire, m)?)?;
    m.add_function(wrap_pyfunction!(enable_wins_flag_disable_delta_to_wire, m)?)?;
    Ok(())
}

/// Observed-remove set of u64 elements and u64 tokens; tokens are global to the set.
#[pyclass(name = "OrSet")]
pub struct PyOrSet {
    inner: OrSet<u64, u64>,
}

#[pymethods]
impl PyOrSet {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: OrSet::new(),
        }
    }

    /// Add an element with a caller-supplied token, exactly as in the Rust core.
    pub fn add(&mut self, element: u64, token: u64) {
        self.inner.add(element, token);
    }
    /// Tombstone tokens globally, including tokens whose adds have not arrived yet.
    pub fn apply_remove(&mut self, tokens: Vec<u64>) {
        self.inner.apply_remove(tokens);
    }

    pub fn observed_tokens(&self, element: u64) -> Vec<u64> {
        self.inner.observed_tokens(&element).into_iter().collect()
    }
    pub fn elements(&self) -> Vec<u64> {
        self.inner.elements().into_iter().collect()
    }
    pub fn contains(&self, element: u64) -> bool {
        self.inner.contains(&element)
    }
    pub fn tombstones(&self) -> Vec<u64> {
        self.inner.tombstones().iter().copied().collect()
    }
    pub fn merge(&mut self, other: &PyOrSet) {
        self.inner.merge(&other.inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use safemesh_crdt::{RecordId, WireEncode};

    fn with_python<T>(f: impl FnOnce(Python<'_>) -> T) -> T {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(f)
    }

    #[test]
    fn checked_counter_raises_index_error_without_mutation() {
        with_python(|py| {
            let mut counter = PyGCounter::new(2);
            let error = counter.try_apply_bump(2, 9).unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyIndexError>(py));
            assert_eq!(
                error.to_string(),
                "IndexError: ReplicaOutOfRange { replica: 2, replica_count: 2 }"
            );
            assert_eq!(counter.state(), vec![0, 0]);
            counter.apply_bump(2, 9);
            assert_eq!(counter.state(), vec![0, 0]);
            counter.try_apply_bump(1, 9).unwrap();
            counter.try_apply_bump(1, 2).unwrap();
            assert_eq!(counter.value(), 9);
        });
    }

    #[test]
    fn orset_matches_core_with_concurrent_add_and_early_tombstone() {
        let mut left = PyOrSet::new();
        let mut right = PyOrSet::new();
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
        println!("python OR-Set={:?} Rust core={expected:?}", left.elements());
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
    fn python_counter_calls_rust_core() {
        let mut counter = PyGCounter::new(3);
        counter.apply_bump(1, 5);
        counter.apply_bump(1, 2);
        assert_eq!(counter.value(), 5);
        assert_eq!(counter.state(), vec![0, 5, 0]);
    }

    #[test]
    fn python_wire_helper_uses_canonical_bytes() {
        with_python(|py| {
            let bytes = gcounter_delta_to_wire(py, 2, 7).unwrap();
            assert_eq!(bytes.as_bytes().len(), 17);
            assert_eq!(bytes.as_bytes()[0], 0x10);
        });
    }

    #[test]
    fn python_replicas_exchange_canonical_record_bytes() {
        with_python(|py| {
            let mut left = PyGCounterReplica::new(1, 3);
            let mut right = PyGCounterReplica::new(2, 3);

            let bytes = left.append_bump(py, 1, 5).unwrap().as_bytes().to_vec();
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

            let log_bytes = right.log_bytes(py).unwrap().as_bytes().to_vec();
            left.merge_log_bytes(&log_bytes).unwrap();
            assert_eq!(left.value(), right.value());
        });
    }

    #[test]
    fn python_lww_register_calls_rust_core() {
        let mut register = PyLwwRegister::new();
        assert!(!register.has_value());
        register.set(10, 1, 100);
        register.set(9, 99, 900);
        register.set(10, 2, 200);
        assert_eq!(register.value_or(0), 200);
        assert_eq!(register.timestamp_or(0), 10);
        assert_eq!(register.writer_replica_or(0), 2);
    }

    #[test]
    fn python_lww_wire_helper_uses_canonical_bytes() {
        with_python(|py| {
            let bytes = lww_register_delta_to_wire(py, 9, 2, 42).unwrap();
            assert_eq!(bytes.as_bytes().len(), 25);
            assert_eq!(bytes.as_bytes()[0], 0x50);
        });
    }

    #[test]
    fn python_enable_wins_flag_calls_rust_core() {
        let mut flag = PyEnableWinsFlag::new();
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
    fn python_enable_wins_flag_wire_helpers_use_canonical_bytes() {
        with_python(|py| {
            let enable = enable_wins_flag_enable_delta_to_wire(py, 42).unwrap();
            assert_eq!(enable.as_bytes().len(), 9);
            assert_eq!(enable.as_bytes()[0], 0x60);

            let disable = enable_wins_flag_disable_delta_to_wire(py, vec![9, 2, 2]).unwrap();
            assert_eq!(disable.as_bytes().len(), 21);
            assert_eq!(disable.as_bytes()[0], 0x61);
            assert_eq!(&disable.as_bytes()[1..5], &[2, 0, 0, 0]);
        });
    }

    #[test]
    fn python_lww_map_calls_rust_core() {
        let mut map = PyLwwMap::new();
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
    fn python_lww_map_wire_helpers_use_canonical_bytes() {
        with_python(|py| {
            let set = lww_map_set_delta_to_wire(py, 7, 9, 2, 42).unwrap();
            assert_eq!(set.as_bytes().len(), 33);
            assert_eq!(set.as_bytes()[0], 0x70);

            let remove = lww_map_remove_delta_to_wire(py, 7, 10, 2).unwrap();
            assert_eq!(remove.as_bytes().len(), 25);
            assert_eq!(remove.as_bytes()[0], 0x71);
        });
    }

    #[test]
    fn python_flag_replicas_exchange_canonical_record_bytes() {
        with_python(|py| {
            let mut left = PyEnableWinsFlagReplica::new(1);
            let mut right = PyEnableWinsFlagReplica::new(2);

            let enable_10 = left.append_enable(py, 10).unwrap().as_bytes().to_vec();
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

            let disable_10 = right
                .append_disable_observed(py)
                .unwrap()
                .as_bytes()
                .to_vec();
            assert!(!right.value());

            let enable_11 = left.append_enable(py, 11).unwrap().as_bytes().to_vec();
            right.merge_record_bytes(&enable_11).unwrap();
            assert!(right.value());

            left.merge_record_bytes(&disable_10).unwrap();
            assert!(left.value());
            let log_bytes = right.log_bytes(py).unwrap().as_bytes().to_vec();
            left.merge_log_bytes(&log_bytes).unwrap();
            assert_eq!(left.value(), right.value());
            assert_eq!(left.enabled_tokens(), vec![10, 11]);
            assert_eq!(left.tombstone_tokens(), vec![10]);
        });
    }

    #[test]
    fn python_lww_map_replicas_exchange_canonical_record_bytes() {
        with_python(|py| {
            let mut left = PyLwwMapReplica::new(1);
            let mut right = PyLwwMapReplica::new(2);

            let set_100 = left
                .append_set(py, 7, 10, 1, 100)
                .unwrap()
                .as_bytes()
                .to_vec();
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

            let remove_100 = right
                .append_remove(py, 7, 11, 2)
                .unwrap()
                .as_bytes()
                .to_vec();
            assert!(!right.has_key(7));

            let set_300 = left
                .append_set(py, 7, 12, 1, 300)
                .unwrap()
                .as_bytes()
                .to_vec();
            right.merge_record_bytes(&set_300).unwrap();
            assert_eq!(right.value_or(7, 0), 300);

            left.merge_record_bytes(&remove_100).unwrap();
            assert_eq!(left.value_or(7, 0), 300);
            let log_bytes = right.log_bytes(py).unwrap().as_bytes().to_vec();
            left.merge_log_bytes(&log_bytes).unwrap();
            assert_eq!(left.value_or(7, 0), right.value_or(7, 0));
            assert_eq!(left.visible_keys(), vec![7]);
            assert_eq!(left.removal_keys(), vec![7]);
        });
    }

    #[test]
    fn python_lww_replicas_exchange_canonical_record_bytes() {
        with_python(|py| {
            let mut left = PyLwwRegisterReplica::new(1);
            let mut right = PyLwwRegisterReplica::new(2);

            let bytes = left.append_set(py, 10, 1, 100).unwrap().as_bytes().to_vec();
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
            right.append_set(py, 10, 2, 200).unwrap();
            assert_eq!(right.value_or(0), 200);
            assert_eq!(right.version_for(1), 1);

            let log_bytes = right.log_bytes(py).unwrap().as_bytes().to_vec();
            left.merge_log_bytes(&log_bytes).unwrap();
            assert_eq!(left.value_or(0), right.value_or(0));
            assert_eq!(left.writer_replica_or(0), 2);
        });
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use safemesh_crdt::RecordId;

    #[test]
    fn all_replica_bindings_reject_conflicting_records_and_logs() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|_| {
            macro_rules! check {
                ($replica:expr, $first:expr, $second:expr, $fixture:literal) => {{
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
                    let a = first.to_wire_bytes().unwrap();
                    let b = second.to_wire_bytes().unwrap();
                    let mut replica = $replica;
                    replica.merge_record_bytes(&a).unwrap();
                    let state = replica.state.clone();
                    let log = replica.log.clone();
                    replica.merge_record_bytes(&a).unwrap();
                    assert!(replica
                        .merge_record_bytes(&b)
                        .unwrap_err()
                        .to_string()
                        .contains("record ID collision"));
                    assert_eq!(replica.state, state);
                    assert_eq!(replica.log, log);
                    let mut incoming = EventLog::for_crdt(&replica.state);
                    assert_eq!(
                        incoming.insert_record(second),
                        safemesh_crdt::Admission::Accepted
                    );
                    assert!(replica
                        .merge_log_bytes(&incoming.to_wire_bytes().unwrap())
                        .is_err());
                    assert_eq!(replica.state, state);
                    assert_eq!(replica.log, log);
                    // Conflicting entries inside a single wire log must not be silently deduped.
                    // Generated by safemesh-crdt's product EventLog encoder.
                    let bytes = include_bytes!($fixture);
                    let mut empty = $replica;
                    let state = empty.state.clone();
                    assert!(empty
                        .merge_log_bytes(bytes)
                        .unwrap_err()
                        .to_string()
                        .contains("record ID collision"));
                    assert_eq!(empty.state, state);
                    assert!(empty.log.records().is_empty());
                }};
            }
            check!(
                PyGCounterReplica::new(2, 2),
                GCounterDelta {
                    replica: 1,
                    tally: 5
                },
                GCounterDelta {
                    replica: 1,
                    tally: 9
                },
                "../tests/fixtures/gcounter-collision.bin"
            );
            check!(
                PyEnableWinsFlagReplica::new(2),
                EnableWinsFlagDelta::Enable { token: 5 },
                EnableWinsFlagDelta::Enable { token: 9 },
                "../tests/fixtures/flag-collision.bin"
            );
            check!(
                PyLwwRegisterReplica::new(2),
                LwwRegisterDelta {
                    timestamp: 1,
                    replica: 1,
                    value: 5
                },
                LwwRegisterDelta {
                    timestamp: 2,
                    replica: 1,
                    value: 9
                },
                "../tests/fixtures/register-collision.bin"
            );
            check!(
                PyLwwMapReplica::new(2),
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
                },
                "../tests/fixtures/map-collision.bin"
            );
        });
    }

    #[test]
    fn persisted_counter_shape_mismatch_leaves_destination_unchanged() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let mut source = PyGCounterReplica::new(0, 2);
            source.append_bump(py, 0, 10).unwrap();
            source.append_bump(py, 1, 20).unwrap();
            let bytes = source.log.to_wire_bytes().unwrap();
            let mut target = PyGCounterReplica::new(0, 3);
            let state = target.state.clone();
            let log = target.log.clone();
            assert!(target
                .merge_log_bytes(&bytes)
                .unwrap_err()
                .to_string()
                .contains("replica count mismatch"));
            assert_eq!(target.state, state);
            assert_eq!(target.log, log);
            let mut matching = PyGCounterReplica::new(0, 2);
            matching.merge_log_bytes(&bytes).unwrap();
            assert_eq!(matching.state, source.state);
        });
    }

    #[test]
    fn invalid_counter_coordinates_are_rejected_before_admission() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let mut replica = PyGCounterReplica::new(1, 2);
            assert!(replica.append_bump(py, 2, 5).is_err());
            let record = Record {
                id: RecordId {
                    replica: 1,
                    sequence: 1,
                },
                delta: GCounterDelta {
                    replica: 2,
                    tally: 5,
                },
            };
            assert!(replica
                .merge_record_bytes(&record.to_wire_bytes().unwrap())
                .is_err());
            let mut log = EventLog::with_replica_count(2);
            log.insert_record(record);
            assert!(replica
                .merge_log_bytes(&log.to_wire_bytes().unwrap())
                .is_err());
            assert_eq!(replica.value(), 0);
            assert!(replica.log.records().is_empty());
        });
    }
}
