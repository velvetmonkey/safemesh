// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use pyo3::prelude::*;
use safemesh_crdt::{GCounter, GCounterDelta, WireEncode};

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

#[pymodule]
fn safemesh_python(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGCounter>()?;
    m.add_function(wrap_pyfunction!(gcounter_delta_to_wire, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
