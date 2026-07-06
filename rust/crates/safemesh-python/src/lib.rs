// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use safemesh_crdt::{
    EventLog, GCounter, GCounterDelta, LwwRegister, LwwRegisterDelta, Record, WireDecode,
    WireEncode,
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
            log: EventLog::new(),
        }
    }

    pub fn append_bump<'py>(
        &mut self,
        py: Python<'py>,
        counter_replica: usize,
        tally: u64,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let delta = GCounterDelta {
            replica: counter_replica,
            tally,
        };
        self.state.apply_bump(delta.replica, delta.tally);
        let id = self.log.append(self.replica_id, delta.clone());
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let record = Record::<GCounterDelta>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode record"))?;
        self.state
            .apply_bump(record.delta.replica, record.delta.tally);
        self.log.merge_records([record]);
        Ok(())
    }

    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let log = EventLog::<GCounterDelta>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode event log"))?;
        for record in log.records().iter().cloned() {
            self.state
                .apply_bump(record.delta.replica, record.delta.tally);
            self.log.merge_records([record]);
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
        self.state.set(delta.timestamp, delta.replica, delta.value);
        let id = self.log.append(self.replica_id, delta.clone());
        encode_bytes(
            py,
            Record { id, delta }.to_wire_bytes(),
            "failed to encode record",
        )
    }

    pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let record = Record::<LwwRegisterDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode record"))?;
        self.state.set(
            record.delta.timestamp,
            record.delta.replica,
            record.delta.value,
        );
        self.log.merge_records([record]);
        Ok(())
    }

    pub fn merge_log_bytes(&mut self, bytes: &[u8]) -> PyResult<()> {
        let log = EventLog::<LwwRegisterDelta<u64>>::from_wire_bytes(bytes)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to decode event log"))?;
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
    m.add_class::<PyGCounterReplica>()?;
    m.add_class::<PyLwwRegister>()?;
    m.add_class::<PyLwwRegisterReplica>()?;
    m.add_function(wrap_pyfunction!(gcounter_delta_to_wire, m)?)?;
    m.add_function(wrap_pyfunction!(lww_register_delta_to_wire, m)?)?;
    Ok(())
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
