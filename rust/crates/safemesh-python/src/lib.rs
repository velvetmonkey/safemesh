// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use pyo3::prelude::*;
use safemesh_crdt::{EventLog, GCounter, GCounterDelta, Record, WireDecode, WireEncode};

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
pub fn gcounter_delta_to_wire(replica: usize, tally: u64) -> PyResult<Vec<u8>> {
    GCounterDelta { replica, tally }
        .to_wire_bytes()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to encode G-Counter delta"))
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

    pub fn append_bump(&mut self, counter_replica: usize, tally: u64) -> PyResult<Vec<u8>> {
        let delta = GCounterDelta {
            replica: counter_replica,
            tally,
        };
        self.state.apply_bump(delta.replica, delta.tally);
        let id = self.log.append(self.replica_id, delta.clone());
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to encode record"))
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

    pub fn log_bytes(&self) -> PyResult<Vec<u8>> {
        self.log
            .to_wire_bytes()
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("failed to encode event log"))
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

#[pymodule]
fn safemesh_python(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGCounter>()?;
    m.add_class::<PyGCounterReplica>()?;
    m.add_function(wrap_pyfunction!(gcounter_delta_to_wire, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use safemesh_crdt::{RecordId, WireEncode};

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
        let bytes = gcounter_delta_to_wire(2, 7).unwrap();
        assert_eq!(bytes.len(), 17);
        assert_eq!(bytes[0], 0x10);
    }

    #[test]
    fn python_replicas_exchange_canonical_record_bytes() {
        let mut left = PyGCounterReplica::new(1, 3);
        let mut right = PyGCounterReplica::new(2, 3);

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
}
