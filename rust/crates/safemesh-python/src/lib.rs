// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: Apache-2.0

use pyo3::prelude::*;
use pyo3::types::PyBytes;
use safemesh_crdt::ownership::{allocate_token, WriterConfig};
use safemesh_crdt::{
    CollectionLimits, Crdt, DecodeError, DecodeLimits, EnableWinsFlag, EnableWinsFlagDelta,
    EventLog, GCounter, GCounterDelta, GSet, LwwMap, LwwMapDelta, LwwRegister, LwwRegisterDelta,
    OrSet, OrSetDelta, PnCounter, Record, Rga, WireDecode, WireEncode, WireError,
};

// Reject bool at the Python boundary before any method body can mutate state.
// Ordinary extraction retains PyO3's TypeError/OverflowError behavior.
fn numeric<'py, T: FromPyObject<'py>>(value: &Bound<'py, PyAny>) -> PyResult<T> {
    if value.is_instance_of::<pyo3::types::PyBool>() {
        return Err(pyo3::exceptions::PyTypeError::new_err(
            "expected an integer, got bool",
        ));
    }
    value.extract()
}

fn collection_budget(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<usize>> {
    match value {
        None => Ok(None),
        Some(value) if value.is_none() => Ok(None),
        Some(value) => numeric(value).map(Some),
    }
}

// Capacity is a byte limit, not just a usize limit. Checking it here keeps
// an unrepresentable Vec allocation from escaping as a PyO3 PanicException.
fn numeric_replicas(value: &Bound<'_, PyAny>) -> PyResult<usize> {
    let replicas: usize = numeric(value)?;
    if replicas > isize::MAX as usize / std::mem::size_of::<u64>() {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "replicas exceeds counter capacity",
        ));
    }
    Ok(replicas)
}

fn numeric_tokens(value: &Bound<'_, PyAny>) -> PyResult<Vec<u64>> {
    value
        .extract::<Vec<Bound<'_, PyAny>>>()?
        .iter()
        .map(numeric)
        .collect()
}

fn encode_bytes<'py>(
    py: Python<'py>,
    bytes: Result<Vec<u8>, safemesh_crdt::WireError>,
    message: &'static str,
) -> PyResult<Bound<'py, PyBytes>> {
    bytes
        .map(|bytes| PyBytes::new_bound(py, &bytes))
        .map_err(|_| pyo3::exceptions::PyValueError::new_err(message))
}

// Each decode path keeps one stable prefix and names the core `WireError` as
// its cause, so the same malformed bytes read the same on either path.
fn record_decode_error(error: safemesh_crdt::WireError) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(format!("failed to decode record: {error}"))
}

fn event_log_decode_error(error: safemesh_crdt::WireError) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(match error {
        safemesh_crdt::WireError::RecordCollision => "record ID collision".to_owned(),
        safemesh_crdt::WireError::ReplicaCountMismatch { .. } => {
            "replica count mismatch".to_owned()
        }
        safemesh_crdt::WireError::DeltaTypeMismatch => "delta type mismatch".to_owned(),
        safemesh_crdt::WireError::MissingShape => "event log missing shape".to_owned(),
        safemesh_crdt::WireError::OwnershipViolation => {
            "counter coordinate out of range or not owned by record author".to_owned()
        }
        cause @ (safemesh_crdt::WireError::ZeroSequenceAdd { .. }
        | safemesh_crdt::WireError::ZeroSequenceRemove { .. }) => cause.to_string(),
        cause => format!("failed to decode event log: {cause}"),
    })
}

#[cfg(test)]
#[test]
fn python_zero_sequence_remove_mapping_keeps_core_cause() {
    pyo3::prepare_freethreaded_python();
    let record = Record {
        id: safemesh_crdt::RecordId {
            replica: 1,
            sequence: 0,
        },
        delta: safemesh_crdt::OrSetDelta::<u64, u64>::Remove { tokens: vec![2] },
    };
    let cause = OrSet::<u64, u64>::new()
        .validate_record(record.id, &record.delta)
        .unwrap_err();
    assert_eq!(
        cause,
        safemesh_crdt::WireError::ZeroSequenceRemove { replica: 1 }
    );
    for error in [
        event_log_decode_error(cause),
        record_verdict(safemesh_crdt::Admission::Invalid(cause)).unwrap_err(),
    ] {
        let text = error.to_string();
        assert_eq!(text, format!("ValueError: {cause}"));
        assert!(text.contains("Recovery: "), "{text}");
    }
}

fn bounded_event_log_decode_error(error: DecodeError) -> PyErr {
    match error {
        DecodeError::Wire(error) => event_log_decode_error(error),
        DecodeError::RecordLimitExceeded { max_records } => {
            pyo3::exceptions::PyValueError::new_err(format!(
                "failed to decode event log: RecordLimitExceeded: {max_records}"
            ))
        }
    }
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

// A single record reports the verdict `merge_log_bytes` reports for it: a
// duplicate or collision is a verdict, not an error. An invalid record raises.
fn record_verdict(admission: safemesh_crdt::Admission) -> PyResult<String> {
    match admission {
        safemesh_crdt::Admission::Invalid(
            cause @ (safemesh_crdt::WireError::ZeroSequenceAdd { .. }
            | safemesh_crdt::WireError::ZeroSequenceRemove { .. }),
        ) => Err(pyo3::exceptions::PyValueError::new_err(cause.to_string())),
        safemesh_crdt::Admission::Invalid(_) => {
            Err(pyo3::exceptions::PyValueError::new_err("invalid record"))
        }
        admission => Ok(admission_name(admission)),
    }
}

#[pyclass(name = "GCounter")]
pub struct PyGCounter {
    inner: GCounter,
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod py_g_counter_python {
    use super::*;

    #[pymethods]
    impl PyGCounter {
        #[new]
        pub fn new(#[pyo3(from_py_with = "numeric_replicas")] replicas: usize) -> Self {
            PyGCounter {
                inner: GCounter::new(replicas),
            }
        }

        pub fn apply_bump(
            &mut self,
            #[pyo3(from_py_with = "numeric")] replica: usize,
            #[pyo3(from_py_with = "numeric")] tally: u64,
        ) -> PyResult<()> {
            self.try_apply_bump(replica, tally)
        }

        /// Apply a coordinate delta, raising IndexError without mutation for a bad index.
        pub fn try_apply_bump(
            &mut self,
            #[pyo3(from_py_with = "numeric")] replica: usize,
            #[pyo3(from_py_with = "numeric")] tally: u64,
        ) -> PyResult<()> {
            self.inner
                .try_apply_bump(replica, tally)
                .map_err(|error| pyo3::exceptions::PyIndexError::new_err(format!("{error:?}")))
        }

        /// The counter total as a Python `int`, exact past the 64-bit boundary.
        pub fn value(&self) -> u128 {
            self.inner.value()
        }

        pub fn state(&self) -> Vec<u64> {
            self.inner.state().to_vec()
        }
    }
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod gcounter_delta_to_wire_python {
    use super::*;

    #[pyfunction]
    pub fn gcounter_delta_to_wire(
        py: Python<'_>,
        #[pyo3(from_py_with = "numeric")] replica: usize,
        #[pyo3(from_py_with = "numeric")] tally: u64,
    ) -> PyResult<Bound<'_, PyBytes>> {
        encode_bytes(
            py,
            GCounterDelta { replica, tally }.to_wire_bytes(),
            "failed to encode G-Counter delta",
        )
    }
}
pub use gcounter_delta_to_wire_python::gcounter_delta_to_wire;

#[pyclass(name = "LwwRegister")]
#[derive(Default)]
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

    pub fn set(
        &mut self,
        #[pyo3(from_py_with = "numeric")] timestamp: u64,
        #[pyo3(from_py_with = "numeric")] replica: u64,
        #[pyo3(from_py_with = "numeric")] value: u64,
    ) {
        self.inner.set(timestamp, replica, value);
    }

    pub fn has_value(&self) -> bool {
        self.inner.value().is_some()
    }

    pub fn value_or(&self, #[pyo3(from_py_with = "numeric")] default_value: u64) -> u64 {
        self.inner.value().copied().unwrap_or(default_value)
    }

    pub fn timestamp_or(&self, #[pyo3(from_py_with = "numeric")] default_value: u64) -> u64 {
        self.inner
            .entry()
            .map(|entry| entry.dot.timestamp)
            .unwrap_or(default_value)
    }

    pub fn writer_replica_or(&self, #[pyo3(from_py_with = "numeric")] default_value: u64) -> u64 {
        self.inner
            .entry()
            .map(|entry| entry.dot.replica)
            .unwrap_or(default_value)
    }
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod lww_register_delta_to_wire_python {
    use super::*;

    #[pyfunction]
    pub fn lww_register_delta_to_wire(
        py: Python<'_>,
        #[pyo3(from_py_with = "numeric")] timestamp: u64,
        #[pyo3(from_py_with = "numeric")] replica: u64,
        #[pyo3(from_py_with = "numeric")] value: u64,
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
}
pub use lww_register_delta_to_wire_python::lww_register_delta_to_wire;

#[pyclass(name = "LwwMap")]
#[derive(Default)]
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

    pub fn set(
        &mut self,
        #[pyo3(from_py_with = "numeric")] key: u64,
        #[pyo3(from_py_with = "numeric")] timestamp: u64,
        #[pyo3(from_py_with = "numeric")] replica: u64,
        #[pyo3(from_py_with = "numeric")] value: u64,
    ) {
        self.inner.set(key, timestamp, replica, value);
    }

    pub fn remove(
        &mut self,
        #[pyo3(from_py_with = "numeric")] key: u64,
        #[pyo3(from_py_with = "numeric")] timestamp: u64,
        #[pyo3(from_py_with = "numeric")] replica: u64,
    ) {
        self.inner.remove(key, timestamp, replica);
    }

    pub fn has_key(&self, #[pyo3(from_py_with = "numeric")] key: u64) -> bool {
        self.inner.get(&key).is_some()
    }

    pub fn value_or(
        &self,
        #[pyo3(from_py_with = "numeric")] key: u64,
        #[pyo3(from_py_with = "numeric")] default_value: u64,
    ) -> u64 {
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

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod lww_map_set_delta_to_wire_python {
    use super::*;

    #[pyfunction]
    pub fn lww_map_set_delta_to_wire(
        py: Python<'_>,
        #[pyo3(from_py_with = "numeric")] key: u64,
        #[pyo3(from_py_with = "numeric")] timestamp: u64,
        #[pyo3(from_py_with = "numeric")] replica: u64,
        #[pyo3(from_py_with = "numeric")] value: u64,
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
}
pub use lww_map_set_delta_to_wire_python::lww_map_set_delta_to_wire;

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod lww_map_remove_delta_to_wire_python {
    use super::*;

    #[pyfunction]
    pub fn lww_map_remove_delta_to_wire(
        py: Python<'_>,
        #[pyo3(from_py_with = "numeric")] key: u64,
        #[pyo3(from_py_with = "numeric")] timestamp: u64,
        #[pyo3(from_py_with = "numeric")] replica: u64,
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
}
pub use lww_map_remove_delta_to_wire_python::lww_map_remove_delta_to_wire;

#[pyclass(name = "EnableWinsFlag")]
#[derive(Default)]
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

    pub fn enable(&mut self, #[pyo3(from_py_with = "numeric")] token: u64) {
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

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod enable_wins_flag_enable_delta_to_wire_python {
    use super::*;

    #[pyfunction]
    pub fn enable_wins_flag_enable_delta_to_wire(
        py: Python<'_>,
        #[pyo3(from_py_with = "numeric")] token: u64,
    ) -> PyResult<Bound<'_, PyBytes>> {
        encode_bytes(
            py,
            EnableWinsFlagDelta::Enable { token }.to_wire_bytes(),
            "failed to encode enable-wins flag enable delta",
        )
    }
}
pub use enable_wins_flag_enable_delta_to_wire_python::enable_wins_flag_enable_delta_to_wire;

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod enable_wins_flag_disable_delta_to_wire_python {
    use super::*;

    #[pyfunction]
    pub fn enable_wins_flag_disable_delta_to_wire(
        py: Python<'_>,
        #[pyo3(from_py_with = "numeric_tokens")] tokens: Vec<u64>,
    ) -> PyResult<Bound<'_, PyBytes>> {
        encode_bytes(
            py,
            EnableWinsFlagDelta::Disable { tokens }.to_wire_bytes(),
            "failed to encode enable-wins flag disable delta",
        )
    }
}
pub use enable_wins_flag_disable_delta_to_wire_python::enable_wins_flag_disable_delta_to_wire;

#[pyclass(name = "GCounterReplica")]
pub struct PyGCounterReplica {
    replica_id: u64,
    state: GCounter,
    log: EventLog<GCounterDelta>,
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod py_g_counter_replica_python {
    use super::*;

    #[pymethods]
    impl PyGCounterReplica {
        #[new]
        pub fn new(
            #[pyo3(from_py_with = "numeric")] replica_id: u64,
            #[pyo3(from_py_with = "numeric_replicas")] replicas: usize,
        ) -> Self {
            PyGCounterReplica {
                replica_id,
                state: GCounter::new(replicas),
                log: EventLog::with_replica_count(replicas),
            }
        }

        pub fn append_bump<'py>(
            &mut self,
            py: Python<'py>,
            #[pyo3(from_py_with = "numeric")] counter_replica: usize,
            #[pyo3(from_py_with = "numeric")] tally: u64,
        ) -> PyResult<Bound<'py, PyBytes>> {
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
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "counter coordinate out of range or not owned by record author",
                ));
            }
            let delta = GCounterDelta {
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
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("event log sequence exhausted")
                })?;
            encode_bytes(
                py,
                Record { id, delta }.to_wire_bytes(),
                "failed to encode record",
            )
        }

        /// Return the core admission verdict for the decoded input record, as
        /// `merge_log_bytes` does per record. Only "accepted" changes state.
        pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<String> {
            let record =
                Record::<GCounterDelta>::from_wire_bytes(bytes).map_err(record_decode_error)?;
            if safemesh_crdt::ownership::check_counter_record(
                self.state.len(),
                record.id,
                &record.delta,
            )
            .is_err()
            {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "counter coordinate out of range or not owned by record author",
                ));
            }
            record_verdict(
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    }),
            )
        }

        /// Return one core admission verdict for every decoded input record.
        #[pyo3(signature = (bytes, *, max_records = None))]
        pub fn merge_log_bytes(
            &mut self,
            bytes: &[u8],
            max_records: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<Vec<String>> {
            let max_records = collection_budget(max_records)?;
            let limits = DecodeLimits {
                max_records,
                ..DecodeLimits::default()
            };
            let log = EventLog::<GCounterDelta>::records_from_wire_bytes_for_with_limits(
                bytes,
                &self.state,
                limits,
            )
            .map_err(bounded_event_log_decode_error)?;
            if log.iter().any(|r| {
                safemesh_crdt::ownership::check_counter_record(self.state.len(), r.id, &r.delta)
                    .is_err()
            }) {
                return Err(pyo3::exceptions::PyValueError::new_err(
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

        pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
        }

        pub fn version_for(&self, #[pyo3(from_py_with = "numeric")] replica: u64) -> u64 {
            self.log.version().get(replica)
        }

        /// The counter total as a Python `int`, exact past the 64-bit boundary.
        pub fn value(&self) -> u128 {
            self.state.value()
        }

        pub fn state(&self) -> Vec<u64> {
            self.state.state().to_vec()
        }
    }
}

#[pyclass(name = "EnableWinsFlagReplica")]
pub struct PyEnableWinsFlagReplica {
    replica_id: u64,
    state: EnableWinsFlag<u64>,
    log: EventLog<EnableWinsFlagDelta<u64>>,
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod py_enable_wins_flag_replica_python {
    use super::*;

    #[pymethods]
    impl PyEnableWinsFlagReplica {
        #[new]
        pub fn new(#[pyo3(from_py_with = "numeric")] replica_id: u64) -> Self {
            PyEnableWinsFlagReplica {
                replica_id,
                state: EnableWinsFlag::new(),
                log: EventLog::new(),
            }
        }

        pub fn append_enable<'py>(
            &mut self,
            py: Python<'py>,
            #[pyo3(from_py_with = "numeric")] token: u64,
        ) -> PyResult<Bound<'py, PyBytes>> {
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
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("event log sequence exhausted")
                })?;
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
                .append_with(
                    &mut self.state,
                    self.replica_id,
                    delta.clone(),
                    |state, delta| {
                        state.apply_delta(delta.clone());
                    },
                )
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("event log sequence exhausted")
                })?;
            encode_bytes(
                py,
                Record { id, delta }.to_wire_bytes(),
                "failed to encode record",
            )
        }

        /// Return the core admission verdict for the decoded input record, as
        /// `merge_log_bytes` does per record. Only "accepted" changes state.
        pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<String> {
            let record = Record::<EnableWinsFlagDelta<u64>>::from_wire_bytes(bytes)
                .map_err(record_decode_error)?;
            record_verdict(
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    }),
            )
        }

        /// Return one core admission verdict for every decoded input record.
        #[pyo3(signature = (bytes, *, max_records = None))]
        pub fn merge_log_bytes(
            &mut self,
            bytes: &[u8],
            max_records: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<Vec<String>> {
            let max_records = collection_budget(max_records)?;
            let limits = DecodeLimits {
                max_records,
                ..DecodeLimits::default()
            };
            let log =
                EventLog::<EnableWinsFlagDelta<u64>>::records_from_wire_bytes_for_with_limits(
                    bytes,
                    &self.state,
                    limits,
                )
                .map_err(bounded_event_log_decode_error)?;
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

        pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
        }

        pub fn version_for(&self, #[pyo3(from_py_with = "numeric")] replica: u64) -> u64 {
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
}

#[pyclass(name = "LwwMapReplica")]
pub struct PyLwwMapReplica {
    replica_id: u64,
    state: LwwMap<u64, u64>,
    log: EventLog<LwwMapDelta<u64, u64>>,
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod py_lww_map_replica_python {
    use super::*;

    #[pymethods]
    impl PyLwwMapReplica {
        #[new]
        pub fn new(#[pyo3(from_py_with = "numeric")] replica_id: u64) -> Self {
            PyLwwMapReplica {
                replica_id,
                state: LwwMap::new(),
                log: EventLog::new(),
            }
        }

        pub fn append_set<'py>(
            &mut self,
            py: Python<'py>,
            #[pyo3(from_py_with = "numeric")] key: u64,
            #[pyo3(from_py_with = "numeric")] timestamp: u64,
            #[pyo3(from_py_with = "numeric")] writer_replica: u64,
            #[pyo3(from_py_with = "numeric")] value: u64,
        ) -> PyResult<Bound<'py, PyBytes>> {
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
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("event log sequence exhausted")
                })?;
            encode_bytes(
                py,
                Record { id, delta }.to_wire_bytes(),
                "failed to encode record",
            )
        }

        pub fn append_remove<'py>(
            &mut self,
            py: Python<'py>,
            #[pyo3(from_py_with = "numeric")] key: u64,
            #[pyo3(from_py_with = "numeric")] timestamp: u64,
            #[pyo3(from_py_with = "numeric")] writer_replica: u64,
        ) -> PyResult<Bound<'py, PyBytes>> {
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
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("event log sequence exhausted")
                })?;
            encode_bytes(
                py,
                Record { id, delta }.to_wire_bytes(),
                "failed to encode record",
            )
        }

        /// Return the core admission verdict for the decoded input record, as
        /// `merge_log_bytes` does per record. Only "accepted" changes state.
        pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<String> {
            let record = Record::<LwwMapDelta<u64, u64>>::from_wire_bytes(bytes)
                .map_err(record_decode_error)?;
            record_verdict(
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    }),
            )
        }

        /// Return one core admission verdict for every decoded input record.
        #[pyo3(signature = (bytes, *, max_records = None))]
        pub fn merge_log_bytes(
            &mut self,
            bytes: &[u8],
            max_records: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<Vec<String>> {
            let max_records = collection_budget(max_records)?;
            let limits = DecodeLimits {
                max_records,
                ..DecodeLimits::default()
            };
            let log = EventLog::<LwwMapDelta<u64, u64>>::records_from_wire_bytes_for_with_limits(
                bytes,
                &self.state,
                limits,
            )
            .map_err(bounded_event_log_decode_error)?;
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

        pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
        }

        pub fn version_for(&self, #[pyo3(from_py_with = "numeric")] replica: u64) -> u64 {
            self.log.version().get(replica)
        }

        pub fn has_key(&self, #[pyo3(from_py_with = "numeric")] key: u64) -> bool {
            self.state.get(&key).is_some()
        }

        pub fn value_or(
            &self,
            #[pyo3(from_py_with = "numeric")] key: u64,
            #[pyo3(from_py_with = "numeric")] default_value: u64,
        ) -> u64 {
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
}

#[pyclass(name = "LwwRegisterReplica")]
pub struct PyLwwRegisterReplica {
    replica_id: u64,
    state: LwwRegister<u64>,
    log: EventLog<LwwRegisterDelta<u64>>,
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod py_lww_register_replica_python {
    use super::*;

    #[pymethods]
    impl PyLwwRegisterReplica {
        #[new]
        pub fn new(#[pyo3(from_py_with = "numeric")] replica_id: u64) -> Self {
            PyLwwRegisterReplica {
                replica_id,
                state: LwwRegister::new(),
                log: EventLog::new(),
            }
        }

        pub fn append_set<'py>(
            &mut self,
            py: Python<'py>,
            #[pyo3(from_py_with = "numeric")] timestamp: u64,
            #[pyo3(from_py_with = "numeric")] writer_replica: u64,
            #[pyo3(from_py_with = "numeric")] value: u64,
        ) -> PyResult<Bound<'py, PyBytes>> {
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
                .map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("event log sequence exhausted")
                })?;
            encode_bytes(
                py,
                Record { id, delta }.to_wire_bytes(),
                "failed to encode record",
            )
        }

        /// Return the core admission verdict for the decoded input record, as
        /// `merge_log_bytes` does per record. Only "accepted" changes state.
        pub fn merge_record_bytes(&mut self, bytes: &[u8]) -> PyResult<String> {
            let record = Record::<LwwRegisterDelta<u64>>::from_wire_bytes(bytes)
                .map_err(record_decode_error)?;
            record_verdict(
                self.log
                    .admit_with(&mut self.state, record, |state, delta| {
                        state.apply_delta(delta.clone());
                    }),
            )
        }

        /// Return one core admission verdict for every decoded input record.
        #[pyo3(signature = (bytes, *, max_records = None))]
        pub fn merge_log_bytes(
            &mut self,
            bytes: &[u8],
            max_records: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<Vec<String>> {
            let max_records = collection_budget(max_records)?;
            let limits = DecodeLimits {
                max_records,
                ..DecodeLimits::default()
            };
            let log = EventLog::<LwwRegisterDelta<u64>>::records_from_wire_bytes_for_with_limits(
                bytes,
                &self.state,
                limits,
            )
            .map_err(bounded_event_log_decode_error)?;
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

        pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            encode_bytes(py, self.log.to_wire_bytes(), "failed to encode event log")
        }

        pub fn version_for(&self, #[pyo3(from_py_with = "numeric")] replica: u64) -> u64 {
            self.log.version().get(replica)
        }

        pub fn has_value(&self) -> bool {
            self.state.value().is_some()
        }

        pub fn value_or(&self, #[pyo3(from_py_with = "numeric")] default_value: u64) -> u64 {
            self.state.value().copied().unwrap_or(default_value)
        }

        pub fn timestamp_or(&self, #[pyo3(from_py_with = "numeric")] default_value: u64) -> u64 {
            self.state
                .entry()
                .map(|entry| entry.dot.timestamp)
                .unwrap_or(default_value)
        }

        pub fn writer_replica_or(
            &self,
            #[pyo3(from_py_with = "numeric")] default_value: u64,
        ) -> u64 {
            self.state
                .entry()
                .map(|entry| entry.dot.replica)
                .unwrap_or(default_value)
        }
    }
}

#[pymodule]
fn safemesh_python(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyGCounter>()?;
    m.add_class::<PyOrSet>()?;
    m.add_class::<PyStringOrSetReplica>()?;
    m.add_class::<PyStringOrSetRecord>()?;
    m.add_class::<PyGSet>()?;
    m.add_class::<PyPnCounter>()?;
    m.add_class::<PyRga>()?;
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
#[derive(Default)]
pub struct PyOrSet {
    inner: OrSet<u64, u64>,
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod py_or_set_python {
    use super::*;

    #[pymethods]
    impl PyOrSet {
        #[new]
        pub fn new() -> Self {
            Self {
                inner: OrSet::new(),
            }
        }

        /// Add an element with a caller-supplied token, exactly as in the Rust core.
        pub fn add(
            &mut self,
            #[pyo3(from_py_with = "numeric")] element: u64,
            #[pyo3(from_py_with = "numeric")] token: u64,
        ) {
            self.inner.add(element, token);
        }
        /// Tombstone tokens globally, including tokens whose adds have not arrived yet.
        pub fn apply_remove(&mut self, #[pyo3(from_py_with = "numeric_tokens")] tokens: Vec<u64>) {
            self.inner.apply_remove(tokens);
        }

        pub fn observed_tokens(&self, #[pyo3(from_py_with = "numeric")] element: u64) -> Vec<u64> {
            self.inner.observed_tokens(&element).into_iter().collect()
        }
        pub fn elements(&self) -> Vec<u64> {
            self.inner.elements().into_iter().collect()
        }
        pub fn contains(&self, #[pyo3(from_py_with = "numeric")] element: u64) -> bool {
            self.inner.contains(&element)
        }
        pub fn tombstones(&self) -> Vec<u64> {
            self.inner.tombstones().iter().copied().collect()
        }
        /// Check Python identity before extracting either Rust borrow.
        #[pyo3(name = "merge")]
        pub fn merge_py(slf: &Bound<'_, Self>, other: &Bound<'_, Self>) -> PyResult<()> {
            if slf.is(other) {
                return Ok(());
            }
            let other = other.try_borrow()?;
            slf.try_borrow_mut()?.merge(&other);
            Ok(())
        }
    }
}

impl PyOrSet {
    pub fn merge(&mut self, other: &PyOrSet) {
        self.inner.merge(&other.inner);
    }
}

// Record-decode texts match WASM. Log-decode texts normally match too, but
// Python prefixes zero-sequence core errors with "failed to decode event log: "
// while WASM returns those reasons alone.
fn string_orset_record_decode_error(error: WireError) -> String {
    match error {
        WireError::CollectionElementLimitExceeded { max_elements } => {
            format!("maxCollectionElements limit exceeded: {max_elements}")
        }
        cause => format!("failed to decode record: {cause}"),
    }
}

fn string_orset_log_decode_error(error: DecodeError) -> String {
    match error {
        DecodeError::Wire(WireError::CollectionElementLimitExceeded { max_elements }) => {
            format!("maxCollectionElements limit exceeded: {max_elements}")
        }
        DecodeError::Wire(WireError::RecordCollision) => "record ID collision".to_owned(),
        DecodeError::Wire(WireError::ReplicaCountMismatch { .. }) => {
            "replica count mismatch".to_owned()
        }
        DecodeError::Wire(WireError::DeltaTypeMismatch) => "delta type mismatch".to_owned(),
        DecodeError::Wire(WireError::MissingShape) => "event log missing shape".to_owned(),
        DecodeError::Wire(cause) => format!("failed to decode event log: {cause}"),
        DecodeError::RecordLimitExceeded { .. } => "failed to decode event log".to_owned(),
    }
}

/// A record decoded by the core, exposed field by field.
///
/// `delta_kind()` is `"add"` (then `element()` and `token()` are set, `tokens()`
/// is empty) or `"remove"` (then `tokens()` carries the tombstoned tokens and
/// `element()`/`token()` are `None`).
#[pyclass(name = "StringOrSetRecord", frozen)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyStringOrSetRecord {
    id: safemesh_crdt::RecordId,
    delta: OrSetDelta<String, u64>,
}

#[pymethods]
impl PyStringOrSetRecord {
    pub fn replica(&self) -> u64 {
        self.id.replica
    }

    pub fn sequence(&self) -> u64 {
        self.id.sequence
    }

    pub fn delta_kind(&self) -> &'static str {
        match self.delta {
            OrSetDelta::Add { .. } => "add",
            OrSetDelta::Remove { .. } => "remove",
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
/// and holds a live-author claim within this Python process. Tokens remain
/// global to the set, exactly as in `OrSet`. Python exposes the WASM
/// `SafeMeshStringOrSetReplica` replica, read, merge, inspect and allocated-writer
/// operations in snake_case where WASM uses camelCase. The WASM lifecycle methods
/// `free()` and `[Symbol.dispose]()` have no Python counterpart. Verdict strings
/// match for the shared operations. Record-decode and allocated-writer refusal texts
/// match WASM; Python raises `ValueError`. Invalid record admission differs:
/// for example, a sequence-0 OR-Set remove raises `ValueError("invalid record")`
/// in Python, while WASM names the reason (`ZeroSequenceRemove`). For a log
/// containing that record, Python prefixes the core reason with
/// `failed to decode event log: `; WASM returns the core reason alone.
#[pyclass(name = "StringOrSetReplica")]
pub struct PyStringOrSetReplica {
    replica_id: u64,
    allocated_writers: Option<u64>,
    state: OrSet<String, u64>,
    log: EventLog<OrSetDelta<String, u64>>,
}

// WASM keeps this registry `thread_local`, which is one WASM instance. A Python
// object can be created on one thread and dropped on another, so the Python
// registry is one process-wide set behind a mutex: at most one live allocated
// handle per author in this process. This is not cross-process fencing.
// Replicas built with the plain constructor never enter it.
static STRING_ORSET_ALLOCATED_AUTHORS: std::sync::Mutex<std::collections::BTreeSet<u64>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

// The set holds plain integers, so a panic elsewhere cannot leave it half
// updated; recover the guard instead of refusing every later claim.
fn string_orset_allocated_authors(
) -> std::sync::MutexGuard<'static, std::collections::BTreeSet<u64>> {
    STRING_ORSET_ALLOCATED_AUTHORS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Drop for PyStringOrSetReplica {
    fn drop(&mut self) {
        if self.allocated_writers.is_some() {
            string_orset_allocated_authors().remove(&self.replica_id);
        }
    }
}

impl PyStringOrSetReplica {
    fn unallocated(replica_id: u64) -> Self {
        PyStringOrSetReplica {
            replica_id,
            allocated_writers: None,
            state: OrSet::new(),
            log: EventLog::new(),
        }
    }

    fn claim(&mut self, writers: u64) -> Result<(), String> {
        WriterConfig {
            writers,
            writer: self.replica_id,
        }
        .validate()
        .map_err(|_| "invalid writer configuration".to_owned())?;
        if !string_orset_allocated_authors().insert(self.replica_id) {
            return Err("author already has a live allocated writer".to_owned());
        }
        self.allocated_writers = Some(writers);
        Ok(())
    }

    fn try_create_allocated(writers: u64, author: u64) -> Result<Self, String> {
        let mut replica = Self::unallocated(author);
        replica.claim(writers)?;
        Ok(replica)
    }

    fn check_owned_record(
        writers: u64,
        record: &Record<OrSetDelta<String, u64>>,
    ) -> Result<(), String> {
        if record.id.replica >= writers || record.id.sequence == 0 {
            return Err(
                "allocation/history consistency: invalid record author or sequence".to_owned(),
            );
        }
        if let OrSetDelta::Add { token, .. } = &record.delta {
            if allocate_token(writers, record.id.replica, record.id.sequence) != Some(*token) {
                return Err("allocation/history consistency: token mismatch".to_owned());
            }
        }
        Ok(())
    }

    // Check every add, including tombstoned adds, and require a complete local
    // history. Peer histories may contain gaps during ordinary record exchange.
    fn checked_next(&self, writers: u64) -> Result<u64, String> {
        WriterConfig {
            writers,
            writer: self.replica_id,
        }
        .validate()
        .map_err(|_| "invalid writer configuration".to_owned())?;
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
            return Err("allocation/history consistency: incomplete local history".to_owned());
        }
        last.checked_add(1)
            .ok_or_else(|| "allocation sequence exhausted".to_owned())
    }

    fn checked_write_next(&self, writers: u64) -> Result<u64, String> {
        let next = self.checked_next(writers)?;
        // Keep an exhausted but consistent identity exportable/restorable.
        // A write must leave its subsequent cursor representable as well.
        if next == u64::MAX {
            return Err("allocation sequence exhausted".to_owned());
        }
        Ok(next)
    }

    fn check_incoming(&self, record: &Record<OrSetDelta<String, u64>>) -> Result<(), String> {
        if let Some(writers) = self.allocated_writers {
            Self::check_owned_record(writers, record)?;
            if record.id.replica == self.replica_id
                && !self.log.records().iter().any(|known| known.id == record.id)
            {
                return Err("incoming record claims the local author".to_owned());
            }
        }
        Ok(())
    }

    fn try_append_allocated_add(&mut self, element: String) -> Result<Vec<u8>, String> {
        let writers = self
            .allocated_writers
            .ok_or_else(|| "replica has no allocated identity".to_owned())?;
        let sequence = self.checked_write_next(writers)?;
        let token = allocate_token(writers, self.replica_id, sequence)
            .ok_or_else(|| "token allocation exhausted".to_owned())?;
        self.append(OrSetDelta::Add { element, token })
    }

    fn try_export_identity(&self) -> Result<Vec<u8>, String> {
        let writers = self
            .allocated_writers
            .ok_or_else(|| "replica has no allocated identity".to_owned())?;
        let next = self.checked_next(writers)?;
        // Local identity storage only, not a new CRDT transport encoding. The
        // suffix is the existing core log, including its shape/integrity checks.
        // Byte for byte the layout WASM `exportIdentity` writes.
        let mut bytes = b"SMOI\x01".to_vec();
        for word in [writers, self.replica_id, next] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend(
            self.log
                .to_wire_bytes()
                .map_err(|error| format!("failed to encode event log: {error:?}"))?,
        );
        Ok(bytes)
    }

    fn try_import_identity(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 29 || &bytes[..5] != b"SMOI\x01" {
            return Err("allocation/history consistency: invalid identity storage".to_owned());
        }
        let word = |i: usize| {
            let mut buffer = [0u8; 8];
            buffer.copy_from_slice(&bytes[i..i + 8]);
            u64::from_le_bytes(buffer)
        };
        let (writers, author, next) = (word(5), word(13), word(21));
        let mut candidate = Self::unallocated(author);
        candidate.log = EventLog::from_wire_bytes_for(&bytes[29..], &candidate.state)
            .map_err(|error| string_orset_log_decode_error(DecodeError::Wire(error)))?;
        if candidate.checked_next(writers)? != next {
            return Err("allocation/history consistency: next sequence mismatch".to_owned());
        }
        for record in candidate.log.records() {
            candidate.state.apply_delta(record.delta.clone());
        }
        // Claim only after all checks; a failed import creates no live writer.
        candidate.claim(writers)?;
        Ok(candidate)
    }

    fn append(&mut self, delta: OrSetDelta<String, u64>) -> Result<Vec<u8>, String> {
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
            .map_err(|_| "event log sequence exhausted".to_owned())?;
        Record { id, delta }
            .to_wire_bytes()
            .map_err(|error| format!("failed to encode record: {error:?}"))
    }

    fn try_append_add(&mut self, element: String, token: u64) -> Result<Vec<u8>, String> {
        if self.allocated_writers.is_some() {
            return Err(
                "allocated replica rejects caller-supplied tokens; use appendAllocatedAdd"
                    .to_owned(),
            );
        }
        self.append(OrSetDelta::Add { element, token })
    }

    fn try_append_remove_observed(&mut self, element: String) -> Result<Vec<u8>, String> {
        if let Some(writers) = self.allocated_writers {
            self.checked_write_next(writers)?;
        }
        let tokens = self.state.observed_tokens(&element).into_iter().collect();
        self.append(OrSetDelta::Remove { tokens })
    }

    fn decode_record(
        bytes: &[u8],
        max_collection_elements: Option<usize>,
    ) -> Result<Record<OrSetDelta<String, u64>>, String> {
        Record::<OrSetDelta<String, u64>>::from_wire_bytes_with_collection_limits(
            bytes,
            CollectionLimits {
                max_elements: max_collection_elements
                    .or(CollectionLimits::WIRE_DEFAULT.max_elements),
            },
        )
        .map_err(string_orset_record_decode_error)
    }

    // A duplicate or collision is a verdict, not an error, as on the log path.
    // Only an invalid record raises.
    fn try_merge_record_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<usize>,
    ) -> Result<String, String> {
        let record = Self::decode_record(bytes, max_collection_elements)?;
        self.check_incoming(&record)?;
        match self
            .log
            .admit_with(&mut self.state, record, |state, delta| {
                state.apply_delta(delta.clone());
            }) {
            safemesh_crdt::Admission::Invalid(_) => Err("invalid record".to_owned()),
            admission => Ok(admission_name(admission)),
        }
    }

    fn try_merge_log_bytes(
        &mut self,
        bytes: &[u8],
        max_collection_elements: Option<usize>,
    ) -> Result<Vec<String>, String> {
        let log = EventLog::<OrSetDelta<String, u64>>::records_from_wire_bytes_for_with_limits(
            bytes,
            &self.state,
            DecodeLimits {
                max_collection_elements,
                ..DecodeLimits::default()
            },
        )
        .map_err(string_orset_log_decode_error)?;
        // Every record passes the ownership check before any is admitted.
        for record in &log {
            self.check_incoming(record)?;
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

    fn try_log_bytes(&self) -> Result<Vec<u8>, String> {
        self.log
            .to_wire_bytes()
            .map_err(|error| format!("failed to encode event log: {error:?}"))
    }

    fn try_inspect_record_bytes(
        bytes: &[u8],
        max_collection_elements: Option<usize>,
    ) -> Result<PyStringOrSetRecord, String> {
        let Record { id, delta } = Self::decode_record(bytes, max_collection_elements)?;
        Ok(PyStringOrSetRecord { id, delta })
    }
}

fn py_value_error(message: String) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(message)
}

// PyO3 0.22 generates redundant PyErr conversions outside the annotated item.
// Scope this lint allowance to this entry point and its generated wrappers.
#[allow(clippy::useless_conversion)]
mod py_string_or_set_replica_python {
    use super::*;

    #[pymethods]
    impl PyStringOrSetReplica {
        #[new]
        pub fn new(#[pyo3(from_py_with = "numeric")] replica_id: u64) -> Self {
            Self::unallocated(replica_id)
        }

        /// Create an allocated writer. At most one allocated handle per author
        /// may live in this Python process; dropping the last reference
        /// releases it. The caller provides any cross-process exclusion and
        /// must not restore stale snapshots.
        #[staticmethod]
        pub fn create_allocated(
            #[pyo3(from_py_with = "numeric")] writers: u64,
            #[pyo3(from_py_with = "numeric")] author: u64,
        ) -> PyResult<Self> {
            Self::try_create_allocated(writers, author).map_err(py_value_error)
        }

        /// Allocate through the Rust ownership rule, append, and return record bytes.
        pub fn append_allocated_add<'py>(
            &mut self,
            py: Python<'py>,
            element: String,
        ) -> PyResult<Bound<'py, PyBytes>> {
            self.try_append_allocated_add(element)
                .map(|bytes| PyBytes::new_bound(py, &bytes))
                .map_err(py_value_error)
        }

        /// Export fixed writer configuration, next sequence, and the complete
        /// log. The bytes are caller-persisted identity storage, not a
        /// transport packet.
        pub fn export_identity<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            self.try_export_identity()
                .map(|bytes| PyBytes::new_bound(py, &bytes))
                .map_err(py_value_error)
        }

        /// Allocation/history consistency check; failure never creates a
        /// fresh writer. A self-consistent stale snapshot is not detected.
        /// There is no disk I/O.
        #[staticmethod]
        pub fn import_identity(bytes: &[u8]) -> PyResult<Self> {
            Self::try_import_identity(bytes).map_err(py_value_error)
        }

        /// Append an add record for `(element, token)` and return its wire bytes.
        /// Allocated instances reject caller tokens; use `append_allocated_add`.
        pub fn append_add<'py>(
            &mut self,
            py: Python<'py>,
            element: String,
            #[pyo3(from_py_with = "numeric")] token: u64,
        ) -> PyResult<Bound<'py, PyBytes>> {
            self.try_append_add(element, token)
                .map(|bytes| PyBytes::new_bound(py, &bytes))
                .map_err(py_value_error)
        }

        /// Append a remove record tombstoning every token this replica has
        /// observed for `element`, as the core reports them, and return its bytes.
        pub fn append_remove_observed<'py>(
            &mut self,
            py: Python<'py>,
            element: String,
        ) -> PyResult<Bound<'py, PyBytes>> {
            self.try_append_remove_observed(element)
                .map(|bytes| PyBytes::new_bound(py, &bytes))
                .map_err(py_value_error)
        }

        /// Decode one record and admit it through the core event log.
        ///
        /// Returns the core's admission verdict, as `merge_log_bytes` does per
        /// record: `"accepted"` when the record was new and applied,
        /// `"duplicate"` when a record with the same identity and payload was
        /// already in the log, and `"collision"` when the identity is known
        /// with a different payload. Only `"accepted"` changes state. Decode
        /// and ownership failures raise.
        #[pyo3(signature = (bytes, *, max_collection_elements = None))]
        pub fn merge_record_bytes(
            &mut self,
            bytes: &[u8],
            max_collection_elements: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<String> {
            let max_collection_elements = collection_budget(max_collection_elements)?;
            self.try_merge_record_bytes(bytes, max_collection_elements)
                .map_err(py_value_error)
        }

        /// Return one core admission verdict for every decoded input record.
        #[pyo3(signature = (bytes, *, max_collection_elements = None))]
        pub fn merge_log_bytes(
            &mut self,
            bytes: &[u8],
            max_collection_elements: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<Vec<String>> {
            let max_collection_elements = collection_budget(max_collection_elements)?;
            self.try_merge_log_bytes(bytes, max_collection_elements)
                .map_err(py_value_error)
        }

        pub fn log_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            self.try_log_bytes()
                .map(|bytes| PyBytes::new_bound(py, &bytes))
                .map_err(py_value_error)
        }

        pub fn version_for(&self, #[pyo3(from_py_with = "numeric")] replica: u64) -> u64 {
            self.log.version().get(replica)
        }

        /// Live members, sorted and unique, as the core computes them.
        pub fn elements(&self) -> Vec<String> {
            self.state.elements().into_iter().collect()
        }

        /// Live add tokens for `element`, excluding tombstoned tokens.
        pub fn observed_tokens(&self, element: String) -> Vec<u64> {
            self.state.observed_tokens(&element).into_iter().collect()
        }

        pub fn tombstones(&self) -> Vec<u64> {
            self.state.tombstones().iter().copied().collect()
        }

        /// Every `(element, token)` add pair the core holds, tombstoned or not.
        pub fn add_entries(&self) -> Vec<(String, u64)> {
            self.state.adds().iter().cloned().collect()
        }

        /// Decode record bytes through the core without admitting them anywhere.
        #[staticmethod]
        #[pyo3(signature = (bytes, *, max_collection_elements = None))]
        pub fn inspect_record_bytes(
            bytes: &[u8],
            max_collection_elements: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<PyStringOrSetRecord> {
            let max_collection_elements = collection_budget(max_collection_elements)?;
            Self::try_inspect_record_bytes(bytes, max_collection_elements).map_err(py_value_error)
        }
    }
}

/// Python value wrapper over Rust `GSet<u64>`; no durable constructor is exposed by the core.
#[pyclass(name = "GSet")]
#[derive(Default)]
pub struct PyGSet {
    inner: GSet<u64>,
}

// PyO3 0.22 generates redundant PyErr conversions in its wrappers.
#[allow(clippy::useless_conversion)]
mod py_gset_python {
    use super::*;
    #[pymethods]
    impl PyGSet {
        #[new]
        pub fn new() -> Self {
            Self { inner: GSet::new() }
        }

        pub fn insert(&mut self, #[pyo3(from_py_with = "numeric")] element: u64) {
            self.inner.insert(element);
        }
        pub fn contains(&self, #[pyo3(from_py_with = "numeric")] element: u64) -> bool {
            self.inner.contains(&element)
        }
        pub fn elements(&self) -> Vec<u64> {
            self.inner.elements().iter().copied().collect()
        }

        pub fn to_wire_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            encode_bytes(py, self.inner.to_wire_bytes(), "failed to encode G-Set")
        }

        #[staticmethod]
        #[pyo3(signature = (bytes, *, max_collection_elements = None))]
        pub fn from_wire_bytes(
            bytes: &[u8],
            max_collection_elements: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<Self> {
            let max_collection_elements = collection_budget(max_collection_elements)?;
            GSet::<u64>::from_wire_bytes_with_limits(
                bytes,
                CollectionLimits {
                    max_elements: max_collection_elements
                        .or(CollectionLimits::WIRE_DEFAULT.max_elements),
                },
            )
            .map(|inner| Self { inner })
            .map_err(|error| pyo3::exceptions::PyValueError::new_err(format!("{error:?}")))
        }

        /// Merge full state; self-merge is an identity operation.
        #[pyo3(name = "merge")]
        pub fn merge_py(slf: &Bound<'_, Self>, other: &Bound<'_, Self>) -> PyResult<()> {
            if slf.is(other) {
                return Ok(());
            }
            let other = other.try_borrow()?;
            slf.try_borrow_mut()?.inner.merge(&other.inner);
            Ok(())
        }
    }
}

/// Python value wrapper over Rust `Rga<u64, u64>`; no durable constructor is exposed by the core.
#[pyclass(name = "Rga")]
#[derive(Default)]
pub struct PyRga {
    inner: Rga<u64, u64>,
}

// PyO3 0.22 generates redundant PyErr conversions in its wrappers.
#[allow(clippy::useless_conversion)]
mod py_rga_python {
    use super::*;
    #[pymethods]
    impl PyRga {
        #[new]
        pub fn new() -> Self {
            Self { inner: Rga::new() }
        }

        pub fn insert(
            &mut self,
            #[pyo3(from_py_with = "numeric")] position: u64,
            #[pyo3(from_py_with = "numeric")] value: u64,
        ) {
            self.inner.insert(position, value);
        }
        pub fn delete(&mut self, #[pyo3(from_py_with = "numeric")] position: u64) {
            self.inner.delete(position);
        }
        pub fn placed(&self) -> Vec<(u64, u64)> {
            self.inner.placed().iter().copied().collect()
        }
        pub fn tombstones(&self) -> Vec<u64> {
            self.inner.tombstones().iter().copied().collect()
        }
        pub fn live_entries(&self) -> Vec<(u64, u64)> {
            self.inner.live_entries()
        }
        pub fn read_positions(&self) -> Vec<u64> {
            self.inner.read_positions()
        }

        pub fn to_wire_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
            encode_bytes(py, self.inner.to_wire_bytes(), "failed to encode RGA")
        }

        #[staticmethod]
        #[pyo3(signature = (bytes, *, max_collection_elements = None))]
        pub fn from_wire_bytes(
            bytes: &[u8],
            max_collection_elements: Option<&Bound<'_, PyAny>>,
        ) -> PyResult<Self> {
            let max_collection_elements = collection_budget(max_collection_elements)?;
            Rga::<u64, u64>::from_wire_bytes_with_limits(
                bytes,
                CollectionLimits {
                    max_elements: max_collection_elements
                        .or(CollectionLimits::WIRE_DEFAULT.max_elements),
                },
            )
            .map(|inner| Self { inner })
            .map_err(|error| pyo3::exceptions::PyValueError::new_err(format!("{error:?}")))
        }

        /// Merge full state; self-merge is an identity operation.
        #[pyo3(name = "merge")]
        pub fn merge_py(slf: &Bound<'_, Self>, other: &Bound<'_, Self>) -> PyResult<()> {
            if slf.is(other) {
                return Ok(());
            }
            let other = other.try_borrow()?;
            slf.try_borrow_mut()?.inner.merge(&other.inner);
            Ok(())
        }
    }
}

/// Python value wrapper over Rust `PnCounter`; no durable constructor is exposed by the core.
#[pyclass(name = "PnCounter")]
pub struct PyPnCounter {
    inner: PnCounter,
}

// PyO3 0.22 generates redundant PyErr conversions in its wrappers.
#[allow(clippy::useless_conversion)]
mod py_pncounter_python {
    use super::*;
    #[pymethods]
    impl PyPnCounter {
        #[new]
        pub fn new(#[pyo3(from_py_with = "numeric_replicas")] replicas: usize) -> Self {
            Self {
                inner: PnCounter::new(replicas),
            }
        }

        /// Exact signed read, including totals outside the 64-bit range.
        pub fn value(&self) -> i128 {
            self.inner.value()
        }
        pub fn p_state(&self) -> Vec<u64> {
            self.inner.p_state().to_vec()
        }
        pub fn n_state(&self) -> Vec<u64> {
            self.inner.n_state().to_vec()
        }

        pub fn apply_inc(
            &mut self,
            #[pyo3(from_py_with = "numeric")] replica: usize,
            #[pyo3(from_py_with = "numeric")] tally: u64,
        ) -> PyResult<()> {
            self.try_apply_inc(replica, tally)
        }
        pub fn try_apply_inc(
            &mut self,
            #[pyo3(from_py_with = "numeric")] replica: usize,
            #[pyo3(from_py_with = "numeric")] tally: u64,
        ) -> PyResult<()> {
            self.inner
                .try_apply_inc(replica, tally)
                .map_err(|error| pyo3::exceptions::PyIndexError::new_err(format!("{error:?}")))
        }

        pub fn apply_dec(
            &mut self,
            #[pyo3(from_py_with = "numeric")] replica: usize,
            #[pyo3(from_py_with = "numeric")] tally: u64,
        ) -> PyResult<()> {
            self.try_apply_dec(replica, tally)
        }
        pub fn try_apply_dec(
            &mut self,
            #[pyo3(from_py_with = "numeric")] replica: usize,
            #[pyo3(from_py_with = "numeric")] tally: u64,
        ) -> PyResult<()> {
            self.inner
                .try_apply_dec(replica, tally)
                .map_err(|error| pyo3::exceptions::PyIndexError::new_err(format!("{error:?}")))
        }

        /// Merge full state; self-merge is an identity operation.
        #[pyo3(name = "merge")]
        pub fn merge_py(slf: &Bound<'_, Self>, other: &Bound<'_, Self>) -> PyResult<()> {
            if slf.is(other) {
                return Ok(());
            }
            let other = other.try_borrow()?;
            slf.try_borrow_mut()?
                .inner
                .merge(&other.inner)
                .map_err(|error| pyo3::exceptions::PyValueError::new_err(format!("{error:?}")))
        }
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
    fn zero_sequence_add_uses_wire_cause_in_python_errors() {
        with_python(|py| {
            let cause = safemesh_crdt::WireError::ZeroSequenceAdd { replica: 1 };
            for error in [
                record_verdict(safemesh_crdt::Admission::Invalid(cause)).unwrap_err(),
                event_log_decode_error(cause),
            ] {
                assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
                assert_eq!(error.to_string(), format!("ValueError: {cause}"));
            }
        });
    }

    #[test]
    fn python_collection_wrappers() {
        with_python(|py| {
            let module = PyModule::new_bound(py, "safemesh_python").unwrap();
            safemesh_python(&module).unwrap();
            let globals = pyo3::types::PyDict::new_bound(py);
            globals.set_item("sm", module).unwrap();
            py.run_bound(
                include_str!("../tests/collection_wrappers.py"),
                Some(&globals),
                None,
            )
            .unwrap();
        });
    }

    #[test]
    fn python_collection_wire_matches_core() {
        with_python(|py| {
            let module = PyModule::new_bound(py, "safemesh_python").unwrap();
            safemesh_python(&module).unwrap();
            let globals = pyo3::types::PyDict::new_bound(py);
            globals.set_item("sm", module).unwrap();

            for elements in [vec![], vec![913827640125], vec![7, 913827640125, u64::MAX]] {
                let mut rust = GSet::<u64>::new();
                for element in &elements {
                    rust.insert(*element);
                }
                let rust_bytes = rust.to_wire_bytes().unwrap();
                globals.set_item("elements", &elements).unwrap();
                globals
                    .set_item("rust_bytes", PyBytes::new_bound(py, &rust_bytes))
                    .unwrap();
                py.run_bound(
                    "value = sm.GSet()\nfor element in elements: value.insert(element)\nassert value.to_wire_bytes() == rust_bytes\nassert sm.GSet.from_wire_bytes(rust_bytes).elements() == elements",
                    Some(&globals), None,
                ).unwrap();
                let python_bytes: Vec<u8> = globals
                    .get_item("value")
                    .unwrap()
                    .unwrap()
                    .call_method0("to_wire_bytes")
                    .unwrap()
                    .extract()
                    .unwrap();
                assert_eq!(
                    GSet::<u64>::from_wire_bytes(&python_bytes)
                        .unwrap()
                        .elements(),
                    rust.elements()
                );
            }

            for (placed, deleted) in [
                (vec![], vec![]),
                (vec![(913827640125, 17)], vec![]),
                (vec![(2, 20), (913827640125, 17), (3, 30)], vec![2, 99]),
            ] {
                let mut rust = Rga::<u64, u64>::new();
                for (position, value) in &placed {
                    rust.insert(*position, *value);
                }
                for position in &deleted {
                    rust.delete(*position);
                }
                let rust_bytes = rust.to_wire_bytes().unwrap();
                let mut expected_placed = placed.clone();
                expected_placed.sort_unstable();
                globals.set_item("placed", &placed).unwrap();
                globals
                    .set_item("expected_placed", &expected_placed)
                    .unwrap();
                globals.set_item("deleted", &deleted).unwrap();
                globals
                    .set_item("rust_bytes", PyBytes::new_bound(py, &rust_bytes))
                    .unwrap();
                py.run_bound(
                    "value = sm.Rga()\nfor position, item in placed: value.insert(position, item)\nfor position in deleted: value.delete(position)\nassert value.to_wire_bytes() == rust_bytes\nassert sm.Rga.from_wire_bytes(rust_bytes).placed() == expected_placed\nassert sm.Rga.from_wire_bytes(rust_bytes).tombstones() == deleted",
                    Some(&globals), None,
                ).unwrap();
                let python_bytes: Vec<u8> = globals
                    .get_item("value")
                    .unwrap()
                    .unwrap()
                    .call_method0("to_wire_bytes")
                    .unwrap()
                    .extract()
                    .unwrap();
                let decoded = Rga::<u64, u64>::from_wire_bytes(&python_bytes).unwrap();
                assert_eq!(decoded.placed(), rust.placed());
                assert_eq!(decoded.tombstones(), rust.tombstones());
            }
        });
    }

    #[test]
    fn python_counter_capacity_is_rejected_at_the_boundary() {
        with_python(|py| {
            let module = PyModule::new_bound(py, "safemesh_python").unwrap();
            safemesh_python(&module).unwrap();
            let globals = pyo3::types::PyDict::new_bound(py);
            globals.set_item("sm", module).unwrap();
            py.run_bound(
                r#"
import sys
capacity_errors = []
for make in [lambda n: sm.GCounter(n), lambda n: sm.GCounterReplica(0, n)]:
    for count in [sys.maxsize // 8 + 1, sys.maxsize]:
        try:
            make(count)
        except BaseException as error:
            if not isinstance(error, ValueError) or 'replicas' not in str(error):
                capacity_errors.append((count, type(error).__name__, str(error)))
        else:
            raise AssertionError('impossible counter capacity accepted')
    assert make(0).value() == 0
    assert make(2).value() == 0
assert not capacity_errors, capacity_errors
"#,
                Some(&globals),
                None,
            )
            .unwrap();
        });
    }

    #[test]
    fn audit_counter_coordinates_dimensions_and_precision() {
        with_python(|py| {
            let module = PyModule::new_bound(py, "safemesh_python").unwrap();
            safemesh_python(&module).unwrap();
            let globals = pyo3::types::PyDict::new_bound(py);
            globals.set_item("sm", module).unwrap();
            py.run_bound(
                r#"
for bad in [0.5, float('nan'), float('inf'), -1]:
    counter = sm.GCounter(2)
    replica = sm.GCounterReplica(0, 2)
    for call in [lambda: sm.GCounter(bad), lambda: sm.GCounterReplica(0, bad),
                 lambda: counter.apply_bump(bad, 1), lambda: counter.try_apply_bump(bad, 1),
                 lambda: sm.gcounter_delta_to_wire(bad, 1), lambda: replica.append_bump(bad, 1)]:
        try: call()
        except (TypeError, OverflowError): pass
        else: raise AssertionError(('accepted bad coordinate', bad))
        assert counter.state() == [0, 0]
        assert replica.state() == [0, 0]
a = sm.GCounter(2)
a.apply_bump(0, 9007199254740991); a.apply_bump(1, 2)
assert a.value() == 9007199254740993
b = sm.GCounterReplica(0, 1); c = sm.GCounterReplica(1, 2)
c.append_bump(1, 8)
for dest, source in [(b,c),(c,b)]:
    before = dest.log_bytes()
    try: dest.merge_log_bytes(source.log_bytes())
    except ValueError: pass
    else: raise AssertionError('accepted mismatched counter dimensions')
    assert dest.log_bytes() == before
"#,
                Some(&globals),
                None,
            )
            .unwrap();
        });
    }

    #[test]
    fn python_numeric_bools_and_batch_occurrences() {
        with_python(|py| {
            let module = PyModule::new_bound(py, "safemesh_python").unwrap();
            safemesh_python(&module).unwrap();
            let globals = pyo3::types::PyDict::new_bound(py);
            globals.set_item("sm", module).unwrap();
            py.run_bound(r#"
import struct, zlib

def refused(fn, args, snapshot=lambda: None):
    before = snapshot()
    try:
        fn(*args)
    except TypeError:
        pass
    else:
        raise AssertionError((fn, args, 'accepted bool'))
    assert snapshot() == before, (fn, args, 'mutated')

constructors = [(sm.GCounter, [2]), (sm.GCounterReplica, [0, 2]),
                (sm.EnableWinsFlagReplica, [0]), (sm.LwwMapReplica, [0]),
                (sm.LwwRegisterReplica, [0])]
functions = [(sm.gcounter_delta_to_wire, [0, 1]),
             (sm.lww_register_delta_to_wire, [1, 0, 1]),
             (sm.lww_map_set_delta_to_wire, [1, 1, 0, 1]),
             (sm.lww_map_remove_delta_to_wire, [1, 1, 0]),
             (sm.enable_wins_flag_enable_delta_to_wire, [1])]
for fn, args in constructors + functions:
    for i in range(len(args)):
        for b in [True, False, 0.5, float("nan"), float("inf")]:
            bad = args.copy(); bad[i] = b
            refused(fn, bad)

cases = [
 (sm.GCounter(2), [('apply_bump',[0,1]), ('try_apply_bump',[0,1])], lambda o: o.state()),
 (sm.LwwRegister(), [('set',[1,0,1]), ('value_or',[0]), ('timestamp_or',[0]), ('writer_replica_or',[0])], lambda o: (o.value_or(0),o.timestamp_or(0),o.writer_replica_or(0))),
 (sm.LwwMap(), [('set',[1,1,0,1]), ('remove',[1,1,0]), ('has_key',[1]), ('value_or',[1,0])], lambda o: (o.visible_keys(),o.entry_keys(),o.removal_keys(),o.value_or(1,0))),
 (sm.EnableWinsFlag(), [('enable',[1])], lambda o: (o.enabled_tokens(),o.tombstone_tokens())),
 (sm.OrSet(), [('add',[1,1]), ('observed_tokens',[1]), ('contains',[1])], lambda o: (o.elements(),o.observed_tokens(1),o.tombstones())),
 (sm.GCounterReplica(0,2), [('append_bump',[0,1]), ('version_for',[0])], lambda o: (o.log_bytes(),o.state())),
 (sm.EnableWinsFlagReplica(0), [('append_enable',[1]), ('version_for',[0])], lambda o: (o.log_bytes(),o.enabled_tokens(),o.tombstone_tokens())),
 (sm.LwwMapReplica(0), [('append_set',[1,1,0,1]), ('append_remove',[1,1,0]), ('version_for',[0]), ('has_key',[1]), ('value_or',[1,0])], lambda o: (o.log_bytes(),o.visible_keys(),o.removal_keys())),
 (sm.LwwRegisterReplica(0), [('append_set',[1,0,1]), ('version_for',[0]), ('value_or',[0]), ('timestamp_or',[0]), ('writer_replica_or',[0])], lambda o: (o.log_bytes(),o.value_or(0),o.timestamp_or(0),o.writer_replica_or(0))),
]
for obj, methods, snapshot in cases:
    for name, args in methods:
        for i in range(len(args)):
            for b in [True, False, 0.5, float("nan"), float("inf")]:
                bad=args.copy(); bad[i]=b
                refused(getattr(obj,name),bad,lambda: snapshot(obj))
for b in [True, False, 0.5, float("nan"), float("inf")]:
    obj=sm.OrSet(); obj.add(1,1)
    refused(obj.apply_remove, [[1,b]], lambda: (obj.elements(),obj.tombstones()))
    refused(sm.enable_wins_flag_disable_delta_to_wire, [[1,b]])

# Independent audit of every numeric position, including sequence elements.
# Python integers past JS's safe limit are valid and must stay exact.
numeric_cases = 0
invalid_numbers = [-1, 0.5, float('nan'), float('inf'), -(1 << 64), 1 << 64]
def refused_numeric(fn, args, snapshot=lambda: None):
    global numeric_cases
    before = snapshot()
    try:
        fn(*args)
    except (TypeError, OverflowError):
        pass
    else:
        raise AssertionError((fn, args, 'accepted invalid numeric input'))
    assert snapshot() == before, (fn, args, 'mutated')
    numeric_cases += 1

for fn, args in constructors + functions:
    for i in range(len(args)):
        for bad in invalid_numbers:
            values = args.copy(); values[i] = bad
            refused_numeric(fn, values)
for obj, methods, snapshot in cases:
    for name, args in methods:
        for i in range(len(args)):
            for bad in invalid_numbers:
                values = args.copy(); values[i] = bad
                refused_numeric(getattr(obj, name), values, lambda: snapshot(obj))
for bad in invalid_numbers:
    obj = sm.OrSet(); obj.add(1, 1)
    refused_numeric(obj.apply_remove, [[1, bad]], lambda: (obj.elements(), obj.tombstones()))
    refused_numeric(sm.enable_wins_flag_disable_delta_to_wire, [[1, bad]])
for exact in [(1 << 53) - 1, 1 << 53, (1 << 53) + 1, (1 << 64) - 1]:
    counter = sm.GCounter(2)
    counter.apply_bump(0, exact); counter.apply_bump(1, exact)
    assert counter.state() == [exact, exact]
    assert counter.value() == 2 * exact
    reg = sm.LwwRegister(); reg.set(exact, exact, exact)
    assert (reg.value_or(0), reg.timestamp_or(0), reg.writer_replica_or(0)) == (exact, exact, exact)
    entries = sm.OrSet(); entries.add(exact, exact)
    assert entries.elements() == [exact]
    assert entries.observed_tokens(exact) == [exact]
print('PYTHON_NUMERIC_EXPORTS=%s CASES=%s' % (len(constructors) + len(functions) + sum(len(methods) for _, methods, _ in cases) + 2, numeric_cases))

# Recompute framing with the same documented CRC and lengths; no fixtures changed.
def occurrences(frame, order):
    body=frame[9:-4]
    arity=8+struct.unpack_from('<I',body,4)[0]
    offset=arity+1+(8 if body[arity] == 1 else 0)
    pos=offset+4; records=[]
    while pos<len(body):
        n=4+struct.unpack_from('<I',body,pos)[0]
        records.append(body[pos:pos+n]); pos+=n
    body=body[:offset]+struct.pack('<I',len(order))+b''.join(records[i] for i in order)
    checked=struct.pack('<II',len(body),len(body)^0xffffffff)+body
    return b'\x03'+checked+struct.pack('<I',zlib.crc32(checked))
for make, append, read in [
    (lambda: sm.GCounterReplica(0,2), lambda r: r.append_bump(0,1), lambda r: r.state()),
    (lambda: sm.EnableWinsFlagReplica(0), lambda r: r.append_enable(1), lambda r: (r.value(), r.enabled_tokens())),
    (lambda: sm.LwwMapReplica(0), lambda r: r.append_set(1,1,0,1), lambda r: r.value_or(1,0)),
    (lambda: sm.LwwRegisterReplica(0), lambda r: r.append_set(1,0,1), lambda r: (r.value_or(0), r.timestamp_or(0))),
]:
    sender=make(); append(sender); append(sender)
    for order, expected in [([0,0,1],['accepted','duplicate','accepted']),
                            ([0,0,0],['accepted','duplicate','duplicate']),
                            ([0,1,0],['accepted','accepted','duplicate'])]:
        receiver=make(); frame=occurrences(sender.log_bytes(),order)
        bounded=make(); before=bounded.log_bytes(); state_before=read(bounded)
        try: bounded.merge_log_bytes(frame, max_records=2)
        except ValueError as error:
            assert 'RecordLimitExceeded' in str(error) and '2' in str(error)
        else: raise AssertionError('record budget did not refuse three occurrences')
        assert read(bounded)==state_before, 'state unchanged after budget refusal'
        assert bounded.log_bytes()==before
        for invalid in (-1, 1.5, True):
            try: bounded.merge_log_bytes(frame, max_records=invalid)
            except (TypeError, OverflowError, ValueError): pass
            else: raise AssertionError('invalid record budget accepted')
            assert read(bounded)==state_before, 'state unchanged after invalid budget'
            assert bounded.log_bytes()==before
        assert bounded.merge_log_bytes(frame, max_records=3)==expected
        assert receiver.merge_log_bytes(frame)==expected
        canonical=receiver.log_bytes()
        assert receiver.merge_log_bytes(frame)==['duplicate']*3
        assert receiver.log_bytes()==canonical
        assert receiver.merge_log_bytes(canonical)==['duplicate']*len(set(order))
        untouched=make(); before=untouched.log_bytes()
        try: untouched.merge_log_bytes(frame+b'\x00')
        except ValueError: pass
        else: raise AssertionError('accepted trailing bytes')
        assert untouched.log_bytes()==before
"#, Some(&globals), None).unwrap();
        });
    }

    #[test]
    fn orset_self_merge_keeps_python_handle_usable() {
        with_python(|py| {
            let set = Py::new(py, PyOrSet::new()).unwrap();
            let set = set.bind(py);
            set.call_method1("add", (42u64, 7u64)).unwrap();
            set.call_method1("apply_remove", (vec![8u64],)).unwrap();
            set.call_method1("merge", (set,)).unwrap();
            assert_eq!(
                set.call_method0("elements")
                    .unwrap()
                    .extract::<Vec<u64>>()
                    .unwrap(),
                vec![42]
            );
            assert_eq!(
                set.call_method0("tombstones")
                    .unwrap()
                    .extract::<Vec<u64>>()
                    .unwrap(),
                vec![8]
            );
            set.call_method1("add", (43u64, 9u64)).unwrap();
            assert_eq!(
                set.call_method0("elements")
                    .unwrap()
                    .extract::<Vec<u64>>()
                    .unwrap(),
                vec![42, 43]
            );
            let other = Py::new(py, PyOrSet::new()).unwrap();
            other.bind(py).call_method1("add", (99u64, 10u64)).unwrap();
            set.call_method1("merge", (other.bind(py),)).unwrap();
            set.call_method1("merge", (other.bind(py),)).unwrap();
            assert_eq!(
                set.call_method0("elements")
                    .unwrap()
                    .extract::<Vec<u64>>()
                    .unwrap(),
                vec![42, 43, 99]
            );
        });
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
            let error = counter.apply_bump(2, 9).unwrap_err();
            assert!(error.is_instance_of::<pyo3::exceptions::PyIndexError>(py));
            // Exercise PyO3 extraction as well as the native shape guard.
            let exported = Py::new(py, PyGCounter::new(2)).unwrap();
            for replica in [1u64 << 32, 1u64 << 53] {
                let error = exported
                    .bind(py)
                    .call_method1("apply_bump", (replica, 9))
                    .unwrap_err();
                assert!(
                    error.is_instance_of::<pyo3::exceptions::PyIndexError>(py)
                        || error.is_instance_of::<pyo3::exceptions::PyOverflowError>(py)
                );
                assert_eq!(exported.borrow(py).state(), vec![0, 0]);
                assert_eq!(exported.borrow(py).value(), 0);
            }
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
        counter.apply_bump(1, 5).unwrap();
        counter.apply_bump(1, 2).unwrap();
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
            left.merge_log_bytes(&log_bytes, None).unwrap();
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
    fn python_enable_wins_flag_wire_helpers_preserve_exact_bytes() {
        with_python(|py| {
            let enable = enable_wins_flag_enable_delta_to_wire(py, 42).unwrap();
            assert_eq!(enable.as_bytes().len(), 9);
            assert_eq!(enable.as_bytes()[0], 0x60);

            let disable = enable_wins_flag_disable_delta_to_wire(py, vec![9, 2, 2]).unwrap();
            assert_eq!(disable.as_bytes().len(), 29);
            assert_eq!(disable.as_bytes()[0], 0x61);
            assert_eq!(&disable.as_bytes()[1..5], &[3, 0, 0, 0]);
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
            left.merge_log_bytes(&log_bytes, None).unwrap();
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
            left.merge_log_bytes(&log_bytes, None).unwrap();
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
            left.merge_log_bytes(&log_bytes, None).unwrap();
            assert_eq!(left.value_or(0), right.value_or(0));
            assert_eq!(left.writer_replica_or(0), 2);
        });
    }
    #[test]
    fn legacy_event_log_fixtures_raise_the_named_core_error() {
        use safemesh_crdt::{LegacyFrame, WireError};
        macro_rules! fixture {
            ($frame:literal, $name:literal) => {
                include_bytes!(concat!(
                    "../../safemesh-crdt/tests/fixtures/legacy-event-log/",
                    $frame,
                    "/",
                    $name
                ))
                .as_slice()
            };
        }
        with_python(|py| {
            let cases = [
                (
                    "tag02",
                    LegacyFrame::Tag02,
                    [
                        fixture!("tag02", "gcounter.log"),
                        fixture!("tag02", "enable-wins-flag-u64.log"),
                        fixture!("tag02", "lww-map-u64.log"),
                        fixture!("tag02", "lww-register-u64.log"),
                    ],
                ),
                (
                    "tag03-unshaped",
                    LegacyFrame::Tag03Unshaped,
                    [
                        fixture!("tag03-unshaped", "gcounter.log"),
                        fixture!("tag03-unshaped", "enable-wins-flag-u64.log"),
                        fixture!("tag03-unshaped", "lww-map-u64.log"),
                        fixture!("tag03-unshaped", "lww-register-u64.log"),
                    ],
                ),
            ];
            for (frame, found, [counter, flag, map, register]) in cases {
                let core = WireError::LegacyEventLogFrame { found }.to_string();
                let expected = format!("ValueError: failed to decode event log: {core}");
                let mut c = PyGCounterReplica::new(0, 2);
                let mut f = PyEnableWinsFlagReplica::new(0);
                let mut m = PyLwwMapReplica::new(0);
                let mut r = PyLwwRegisterReplica::new(0);
                let errors = [
                    c.merge_log_bytes(counter, None).unwrap_err(),
                    f.merge_log_bytes(flag, None).unwrap_err(),
                    m.merge_log_bytes(map, None).unwrap_err(),
                    r.merge_log_bytes(register, None).unwrap_err(),
                ];
                for error in errors {
                    assert!(error.is_instance_of::<pyo3::exceptions::PyValueError>(py));
                    assert_eq!(error.to_string(), expected, "{frame}");
                }
                println!("Python {frame}: {expected}");
                assert_eq!(c.value(), 0);
                assert!(c.log.records().is_empty());
                assert!(f.log.records().is_empty());
                assert!(m.log.records().is_empty());
                assert!(r.log.records().is_empty());
            }
        });
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use safemesh_crdt::{RecordId, WireError};

    #[test]
    fn all_replica_bindings_reject_conflicting_records_and_report_log_collisions() {
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
                    let mut replica = $replica;
                    assert_eq!(
                        replica
                            .merge_record_bytes(&first.to_wire_bytes().unwrap())
                            .unwrap(),
                        "accepted"
                    );
                    let state = replica.state.clone();
                    let log = replica.log.clone();
                    // Both paths name the same core verdict for the same record. A
                    // redelivery or a conflicting payload never raises or moves state.
                    for (record, verdict) in [(first, "duplicate"), (second, "collision")] {
                        assert_eq!(
                            replica
                                .merge_record_bytes(&record.to_wire_bytes().unwrap())
                                .unwrap(),
                            verdict
                        );
                        assert_eq!(replica.state, state);
                        assert_eq!(replica.log, log);
                        let mut incoming = EventLog::for_crdt(&replica.state);
                        assert_eq!(
                            incoming.insert_record(&replica.state, record),
                            safemesh_crdt::Admission::Accepted
                        );
                        assert_eq!(
                            replica
                                .merge_log_bytes(&incoming.to_wire_bytes().unwrap(), None)
                                .unwrap(),
                            vec![verdict]
                        );
                        assert_eq!(replica.state, state);
                        assert_eq!(replica.log, log);
                    }
                    // Conflicting entries inside a single wire log must not be silently deduped.
                    // Generated by safemesh-crdt's product EventLog encoder.
                    let bytes = include_bytes!($fixture);
                    let mut empty = $replica;
                    let state = empty.state.clone();
                    assert!(empty
                        .merge_log_bytes(bytes, None)
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

    /// Both decode paths name the core `WireError` for the same malformed input.
    /// Truncation and trailing bytes are applied to each path's own frame: a record
    /// frame handed to the log decoder is only ever an unexpected tag.
    #[test]
    fn record_and_log_paths_name_the_same_core_wire_error() {
        pyo3::prepare_freethreaded_python();

        fn cause(message: &str, prefix: &str) -> Option<String> {
            message
                .strip_prefix("ValueError: ")?
                .strip_prefix(prefix)?
                .strip_prefix(": ")
                .map(str::to_owned)
        }

        macro_rules! check {
            ($name:literal, $replica:expr, $delta:expr) => {{
                let replica = $replica;
                let record = Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 1,
                    },
                    delta: $delta,
                };
                let record_bytes = record.to_wire_bytes().unwrap();
                let mut log = EventLog::for_crdt(&replica.state);
                assert_eq!(
                    log.insert_record(&replica.state, record),
                    safemesh_crdt::Admission::Accepted
                );
                let log_bytes = log.to_wire_bytes().unwrap();
                let truncate = |bytes: &[u8]| bytes[..bytes.len() - 1].to_vec();
                let trail = |bytes: &[u8]| [bytes, &[0][..]].concat();
                let cases = [
                    ("empty_bytes", vec![], vec![], WireError::UnexpectedEof),
                    (
                        "truncated_frame",
                        truncate(&record_bytes),
                        truncate(&log_bytes),
                        WireError::UnexpectedEof,
                    ),
                    (
                        "trailing_bytes",
                        trail(&record_bytes),
                        trail(&log_bytes),
                        WireError::TrailingBytes,
                    ),
                    ("undecodable_bytes", vec![0], vec![0], WireError::InvalidTag),
                ];
                for (case, record_input, log_input, expected) in cases {
                    let mut target = $replica;
                    let single = target
                        .merge_record_bytes(&record_input)
                        .unwrap_err()
                        .to_string();
                    let batch = target
                        .merge_log_bytes(&log_input, None)
                        .unwrap_err()
                        .to_string();
                    println!("Python {} {case}: record={single:?} log={batch:?}", $name);
                    let single = cause(&single, "failed to decode record");
                    let batch = cause(&batch, "failed to decode event log");
                    assert_eq!(single, batch, "{} {case}: causes differ by path", $name);
                    assert_eq!(
                        single,
                        Some(expected.to_string()),
                        "{} {case}: not the core error",
                        $name
                    );
                    assert!(target.log.records().is_empty());
                }
            }};
        }

        Python::with_gil(|_| {
            check!(
                "GCounterReplica",
                PyGCounterReplica::new(2, 2),
                GCounterDelta {
                    replica: 1,
                    tally: 5
                }
            );
            check!(
                "EnableWinsFlagReplica",
                PyEnableWinsFlagReplica::new(2),
                EnableWinsFlagDelta::Enable { token: 5 }
            );
            check!(
                "LwwRegisterReplica",
                PyLwwRegisterReplica::new(2),
                LwwRegisterDelta {
                    timestamp: 1,
                    replica: 1,
                    value: 5
                }
            );
            check!(
                "LwwMapReplica",
                PyLwwMapReplica::new(2),
                LwwMapDelta::Set {
                    key: 1,
                    timestamp: 1,
                    replica: 1,
                    value: 5
                }
            );
        });
    }

    #[test]
    fn python_batch_reports_every_admission() {
        pyo3::prepare_freethreaded_python();

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

        let mut target = PyGCounterReplica::new(0, 2);
        assert_eq!(
            target
                .merge_record_bytes(&existing.to_wire_bytes().unwrap())
                .unwrap(),
            "accepted"
        );
        assert_eq!(
            target.merge_log_bytes(&wire([]), None).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(target.state(), vec![5, 0]);

        let mut one = PyGCounterReplica::new(0, 2);
        assert_eq!(
            one.merge_log_bytes(&wire([accepted.clone()]), None)
                .unwrap(),
            vec!["accepted"]
        );
        assert_eq!(one.state(), vec![0, 7]);

        // The single-record path reports the collision the batch reports.
        let before = target.state();
        assert_eq!(
            target
                .merge_record_bytes(&collision.to_wire_bytes().unwrap())
                .unwrap(),
            "collision"
        );
        assert_eq!(
            target
                .merge_log_bytes(&wire([collision.clone()]), None)
                .unwrap(),
            vec!["collision"]
        );
        assert_eq!(target.state(), before);

        assert_eq!(
            target
                .merge_record_bytes(&accepted.to_wire_bytes().unwrap())
                .unwrap(),
            "accepted"
        );
        let before = target.state();
        for record in [&existing, &accepted] {
            assert_eq!(
                target
                    .merge_record_bytes(&record.to_wire_bytes().unwrap())
                    .unwrap(),
                "duplicate"
            );
        }
        assert_eq!(
            target
                .merge_log_bytes(&wire([existing.clone(), accepted.clone()]), None)
                .unwrap(),
            vec!["duplicate", "duplicate"]
        );
        assert_eq!(target.state(), before);

        let mut late = PyGCounterReplica::new(0, 2);
        assert_eq!(
            late.merge_record_bytes(&existing.to_wire_bytes().unwrap())
                .unwrap(),
            "accepted"
        );
        let before = late.state();
        let admissions = late
            .merge_log_bytes(&wire([accepted, collision, after_collision]), None)
            .unwrap();
        let after = late.state();
        println!("PYTHON admissions={admissions:?} before_state={before:?} after_state={after:?}");
        assert_eq!(admissions, vec!["accepted", "collision", "accepted"]);
        assert_eq!(before, vec![5, 0]);
        assert_eq!(after, vec![5, 8]);
    }

    #[test]
    fn python_record_and_log_paths_name_the_same_verdict() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let module = PyModule::new_bound(py, "safemesh_python").unwrap();
            safemesh_python(&module).unwrap();
            let globals = pyo3::types::PyDict::new_bound(py);
            globals.set_item("sm", module).unwrap();
            py.run_bound(
                r#"
# Two writers reusing record ID (1, 1) with different payloads force a collision.
cases = [
    ('GCounterReplica', lambda: sm.GCounterReplica(1, 2), lambda: sm.GCounterReplica(0, 2),
     lambda r: r.append_bump(1, 5), lambda r: r.append_bump(1, 9), lambda r: r.state()),
    ('EnableWinsFlagReplica', lambda: sm.EnableWinsFlagReplica(1), lambda: sm.EnableWinsFlagReplica(0),
     lambda r: r.append_enable(5), lambda r: r.append_enable(9),
     lambda r: (r.enabled_tokens(), r.tombstone_tokens())),
    ('LwwMapReplica', lambda: sm.LwwMapReplica(1), lambda: sm.LwwMapReplica(0),
     lambda r: r.append_set(1, 1, 1, 5), lambda r: r.append_remove(1, 2, 1),
     lambda r: (r.visible_keys(), r.entry_keys(), r.removal_keys(), r.value_or(1, 0))),
    ('LwwRegisterReplica', lambda: sm.LwwRegisterReplica(1), lambda: sm.LwwRegisterReplica(0),
     lambda r: r.append_set(1, 1, 5), lambda r: r.append_set(2, 1, 9),
     lambda r: (r.value_or(0), r.timestamp_or(0), r.writer_replica_or(0))),
]
verdicts = 0
for name, writer, reader, write, forge, read in cases:
    author, forger, receiver = writer(), writer(), reader()
    record, forged = write(author), forge(forger)
    assert receiver.merge_record_bytes(record) == 'accepted', name
    before = (receiver.log_bytes(), read(receiver))
    for single, batch, verdict in [(record, author.log_bytes(), 'duplicate'),
                                   (forged, forger.log_bytes(), 'collision')]:
        result = receiver.merge_record_bytes(single)
        assert type(result) is str and result == verdict, (name, result)
        assert receiver.merge_log_bytes(batch) == [result], name
        assert (receiver.log_bytes(), read(receiver)) == before, name
        verdicts += 1
    for bad in [b'', record + b'\x00']:
        try:
            receiver.merge_record_bytes(bad)
        except ValueError as error:
            assert 'failed to decode record' in str(error), (name, error)
        else:
            raise AssertionError((name, 'accepted undecodable record bytes'))
        assert (receiver.log_bytes(), read(receiver)) == before, name

# Ownership is still an error: coordinate 2 is outside a width-2 counter.
receiver = sm.GCounterReplica(0, 2)
try:
    receiver.merge_record_bytes(sm.GCounterReplica(2, 3).append_bump(2, 5))
except ValueError as error:
    assert 'not owned by record author' in str(error), error
else:
    raise AssertionError('accepted an out-of-range counter coordinate')
assert receiver.state() == [0, 0] and receiver.version_for(2) == 0
print('PYTHON_RECORD_VERDICTS=%d' % verdicts)
"#,
                Some(&globals),
                None,
            )
            .unwrap();
        });
    }

    #[test]
    fn persisted_counter_shape_mismatch_leaves_destination_unchanged() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let mut source = PyGCounterReplica::new(0, 2);
            source.append_bump(py, 0, 10).unwrap();
            let mut second_writer = PyGCounterReplica::new(1, 2);
            let second = second_writer.append_bump(py, 1, 20).unwrap();
            source.merge_record_bytes(second.as_bytes()).unwrap();
            let bytes = source.log.to_wire_bytes().unwrap();
            let mut target = PyGCounterReplica::new(0, 3);
            let state = target.state.clone();
            let log = target.log.clone();
            assert!(target
                .merge_log_bytes(&bytes, None)
                .unwrap_err()
                .to_string()
                .contains("replica count mismatch"));
            assert_eq!(target.state, state);
            assert_eq!(target.log, log);
            let mut matching = PyGCounterReplica::new(0, 2);
            matching.merge_log_bytes(&bytes, None).unwrap();
            assert_eq!(matching.state, source.state);
        });
    }

    #[test]
    fn invalid_counter_coordinates_are_rejected_before_admission() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let mut replica = PyGCounterReplica::new(1, 2);
            assert!(replica.append_bump(py, 2, 5).is_err());
            let mut errors = Vec::new();
            for (name, coordinate) in [("out-of-range", 2), ("not-owned", 0)] {
                let record = Record {
                    id: RecordId {
                        replica: 1,
                        sequence: 1,
                    },
                    delta: GCounterDelta {
                        replica: coordinate,
                        tally: 5,
                    },
                };
                let single = replica
                    .merge_record_bytes(&record.to_wire_bytes().unwrap())
                    .unwrap_err()
                    .to_string();
                let mut bytes = Vec::new();
                EventLog::encode_records(Some(2), &[record], &mut bytes).unwrap();
                let log = replica
                    .merge_log_bytes(&bytes, None)
                    .unwrap_err()
                    .to_string();
                println!("Python {name}: single={single:?}, log={log:?}");
                errors.push((name, single, log));
            }
            for (name, single, log) in errors {
                assert_eq!(log, single, "{name} ownership error differs by path");
                assert_eq!(
                    single,
                    "ValueError: counter coordinate out of range or not owned by record author"
                );
            }
            assert_eq!(replica.value(), 0);
            assert!(replica.log.records().is_empty());
        });
    }

    #[test]
    fn invalid_second_counter_log_record_admits_neither_record() {
        pyo3::prepare_freethreaded_python();
        let mut replica = PyGCounterReplica::new(0, 2);
        let good = Record {
            id: RecordId {
                replica: 1,
                sequence: 1,
            },
            delta: GCounterDelta {
                replica: 1,
                tally: 5,
            },
        };
        let bad = Record {
            id: RecordId {
                replica: 1,
                sequence: 2,
            },
            delta: GCounterDelta {
                replica: 0,
                tally: 6,
            },
        };
        let mut bytes = Vec::new();
        EventLog::encode_records(Some(2), &[good, bad], &mut bytes).unwrap();
        let error = replica
            .merge_log_bytes(&bytes, None)
            .unwrap_err()
            .to_string();
        println!(
            "Python good-then-bad: error={error:?}, value={}",
            replica.value()
        );
        assert_eq!(replica.value(), 0);
        assert!(replica.log.records().is_empty());
    }
}

// Mirrors the `wasm_string_orset_*` host tests in safemesh-wasm: same inputs,
// same verdict strings and the tested decode/allocated-writer refusal texts, checked against
// the Rust core. Invalid record admission error texts differ between bindings.
#[cfg(test)]
mod string_orset_tests {
    use super::*;
    use safemesh_crdt::RecordId;

    type Replica = PyStringOrSetReplica;

    fn replica(replica_id: u64) -> Replica {
        PyStringOrSetReplica::new(replica_id)
    }

    // The allocated registry is process-wide and the test harness runs tests on
    // parallel threads, so every allocated test below owns a distinct author.

    #[test]
    fn python_allocated_identity_checks_history_and_restarts() {
        // WASM `allocated_identity_checks_history_and_restarts`, author moved to
        // 1000 so it cannot meet another test's claim.
        let author = 1000;
        let writers = 1001;
        let mut left = Replica::try_create_allocated(writers, author).unwrap();
        let first = left.try_append_allocated_add("water".into()).unwrap();
        let record = Replica::decode_record(&first, None).unwrap();
        assert_eq!(record.id.sequence, 1);
        assert_eq!(
            record.delta,
            OrSetDelta::Add {
                element: "water".into(),
                token: writers + author
            }
        );
        let saved = left.try_export_identity().unwrap();
        assert_eq!(&saved[..5], b"SMOI\x01");
        // A second live handle for the same author is refused.
        assert_eq!(
            Replica::try_import_identity(&saved).err().unwrap(),
            "author already has a live allocated writer"
        );
        assert_eq!(
            Replica::try_create_allocated(writers, author)
                .err()
                .unwrap(),
            "author already has a live allocated writer"
        );
        drop(left);
        for (offset, word) in [
            (5, 0u64),
            (5, author),
            (5, writers + 1),
            (13, author - 1),
            (21, 0),
            (21, u64::MAX),
        ] {
            let mut bad = saved.clone();
            bad[offset..offset + 8].copy_from_slice(&word.to_le_bytes());
            assert!(
                Replica::try_import_identity(&bad).is_err(),
                "{offset} {word}"
            );
        }
        let mut restored = Replica::try_import_identity(&saved).unwrap();
        let next = restored.try_append_allocated_add("radio".into()).unwrap();
        let record = Replica::decode_record(&next, None).unwrap();
        assert_eq!(record.id.sequence, 2);
        assert_eq!(
            record.delta,
            OrSetDelta::Add {
                element: "radio".into(),
                token: 2 * writers + author
            }
        );
        assert_eq!(
            restored.elements(),
            vec!["radio".to_string(), "water".to_string()]
        );
    }

    #[test]
    fn python_allocated_history_refuses_gaps_zero_and_max_sequence() {
        let mut zero = replica(0);
        assert_eq!(
            zero.log.insert_record(
                &zero.state,
                Record {
                    id: RecordId {
                        replica: 0,
                        sequence: 0,
                    },
                    delta: OrSetDelta::Remove { tokens: vec![] },
                },
            ),
            safemesh_crdt::Admission::Invalid(WireError::ZeroSequenceRemove { replica: 0 })
        );
        assert!(zero.log.records().is_empty());
        assert_eq!(zero.checked_next(1), Ok(1));
        for sequence in [2, u64::MAX] {
            let mut replica = replica(0);
            replica.log.insert_record(
                &replica.state,
                Record {
                    id: RecordId {
                        replica: 0,
                        sequence,
                    },
                    delta: OrSetDelta::Remove { tokens: vec![] },
                },
            );
            assert!(replica.checked_next(1).is_err());
        }
    }

    #[test]
    fn python_allocated_writer_refuses_misuse_with_wasm_texts() {
        let author = 2000;
        assert_eq!(
            Replica::try_create_allocated(author, author).err().unwrap(),
            "invalid writer configuration"
        );
        assert_eq!(
            Replica::try_create_allocated(0, 0).err().unwrap(),
            "invalid writer configuration"
        );
        // A failed claim leaves no live writer behind.
        let mut writer = Replica::try_create_allocated(author + 1, author).unwrap();
        assert_eq!(
            writer.try_append_add("x".into(), 5).unwrap_err(),
            "allocated replica rejects caller-supplied tokens; use appendAllocatedAdd"
        );
        let mut plain = replica(author);
        assert_eq!(
            plain.try_append_allocated_add("x".into()).unwrap_err(),
            "replica has no allocated identity"
        );
        assert_eq!(
            plain.try_export_identity().unwrap_err(),
            "replica has no allocated identity"
        );
        assert_eq!(
            Replica::try_import_identity(b"SMOI").err().unwrap(),
            "allocation/history consistency: invalid identity storage"
        );

        // Incoming records must follow the allocation rule and must not claim
        // the local author.
        let own: Record<OrSetDelta<String, u64>> = Record {
            id: RecordId {
                replica: author,
                sequence: 1,
            },
            delta: OrSetDelta::Add {
                element: "x".into(),
                token: allocate_token(author + 1, author, 1).unwrap(),
            },
        };
        assert_eq!(
            writer
                .try_merge_record_bytes(&own.to_wire_bytes().unwrap(), None)
                .unwrap_err(),
            "incoming record claims the local author"
        );
        let wrong_token: Record<OrSetDelta<String, u64>> = Record {
            id: RecordId {
                replica: 3,
                sequence: 1,
            },
            delta: OrSetDelta::Add {
                element: "x".to_string(),
                token: 7u64,
            },
        };
        assert_eq!(
            writer
                .try_merge_record_bytes(&wrong_token.to_wire_bytes().unwrap(), None)
                .unwrap_err(),
            "allocation/history consistency: token mismatch"
        );
        let outsider: Record<OrSetDelta<String, u64>> = Record {
            id: RecordId {
                replica: author + 1,
                sequence: 1,
            },
            delta: OrSetDelta::Remove { tokens: vec![] },
        };
        assert_eq!(
            writer
                .try_merge_record_bytes(&outsider.to_wire_bytes().unwrap(), None)
                .unwrap_err(),
            "allocation/history consistency: invalid record author or sequence"
        );
        assert!(writer.log.records().is_empty());
        assert!(writer.elements().is_empty());
    }

    #[test]
    fn python_allocated_writers_report_a_collision_as_a_verdict() {
        // Two allocated writers in two processes may share an author id; the
        // registry only fences one process. Simulate the second process by
        // releasing the first handle, then show the core reports the clash.
        let (writers, author) = (3010, 3000);
        let mut first = Replica::try_create_allocated(writers, author).unwrap();
        let from_first = first.try_append_allocated_add("apple".into()).unwrap();
        drop(first);
        let mut second = Replica::try_create_allocated(writers, author).unwrap();
        let from_second = second.try_append_allocated_add("pear".into()).unwrap();
        drop(second);

        let mut reader = Replica::try_create_allocated(writers, 3002).unwrap();
        assert_eq!(
            reader.try_merge_record_bytes(&from_first, None).unwrap(),
            "accepted"
        );
        let state = reader.state.clone();
        let log = reader.log.clone();
        assert_eq!(
            reader.try_merge_record_bytes(&from_second, None).unwrap(),
            "collision"
        );
        assert_eq!(reader.state, state);
        assert_eq!(reader.log, log);
    }

    fn fields(
        record: &PyStringOrSetRecord,
    ) -> (
        u64,
        u64,
        &'static str,
        Option<String>,
        Option<u64>,
        Vec<u64>,
    ) {
        (
            record.replica(),
            record.sequence(),
            record.delta_kind(),
            record.element(),
            record.token(),
            record.tokens(),
        )
    }

    #[test]
    fn python_string_orset_replica_round_trips_records_through_core() {
        let mut left = replica(1);
        let mut right = replica(2);
        let mut core = OrSet::<String, u64>::new();

        let add_bytes = left.try_append_add("vaccine".to_string(), 11).unwrap();
        assert_eq!(
            right.try_merge_record_bytes(&add_bytes, None).unwrap(),
            "accepted"
        );
        core.apply_delta(OrSetDelta::Add {
            element: "vaccine".to_string(),
            token: 11,
        });
        assert_eq!(right.elements(), left.elements());
        assert_eq!(right.elements(), vec!["vaccine".to_string()]);
        assert_eq!(
            right.elements(),
            core.elements().into_iter().collect::<Vec<_>>()
        );
        assert_eq!(right.observed_tokens("vaccine".to_string()), vec![11]);

        let remove_bytes = left
            .try_append_remove_observed("vaccine".to_string())
            .unwrap();
        assert_eq!(
            right.try_merge_record_bytes(&remove_bytes, None).unwrap(),
            "accepted"
        );
        core.apply_delta(OrSetDelta::Remove { tokens: vec![11] });
        assert_eq!(right.elements(), left.elements());
        assert!(right.elements().is_empty());
        assert_eq!(right.tombstones(), left.tombstones());
        assert_eq!(right.tombstones(), vec![11]);
        assert_eq!(
            right.tombstones(),
            core.tombstones().iter().copied().collect::<Vec<_>>()
        );
        let entries = right.add_entries();
        assert_eq!(entries, left.add_entries());
        assert_eq!(entries, vec![("vaccine".to_string(), 11)]);
        assert_eq!(entries, core.adds().iter().cloned().collect::<Vec<_>>());

        let mut third = replica(3);
        third
            .try_merge_log_bytes(&left.try_log_bytes().unwrap(), None)
            .unwrap();
        assert_eq!(third.elements(), left.elements());
        assert_eq!(third.tombstones(), left.tombstones());
        assert_eq!(third.add_entries(), left.add_entries());
        for replica in [&left, &right, &third] {
            assert_eq!(replica.version_for(1), 2);
            assert_eq!(replica.version_for(2), 0);
        }
    }

    #[test]
    fn python_string_orset_replica_rejects_duplicate_record_without_moving_state() {
        let mut author = replica(1);
        let mut reader = replica(2);
        let bytes = author.try_append_add("vaccine".to_string(), 11).unwrap();

        let first = reader.try_merge_record_bytes(&bytes, None).unwrap();
        let before = (
            reader.elements(),
            reader.tombstones(),
            reader.version_for(1),
            reader.log.records().len(),
        );
        let second = reader.try_merge_record_bytes(&bytes, None).unwrap();
        let after = (
            reader.elements(),
            reader.tombstones(),
            reader.version_for(1),
            reader.log.records().len(),
        );
        assert_eq!(first, "accepted");
        assert_eq!(second, "duplicate");
        assert_eq!(before, after);
        assert_eq!(after.3, 1);

        // Same identity, different payload: the core reports a collision. The
        // binding returns that verdict, as the log path does, and absorbs
        // neither reading.
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
            .try_merge_record_bytes(&forged.to_wire_bytes().unwrap(), None)
            .unwrap();
        assert_eq!(verdict, "collision");
        let mut incoming = EventLog::for_crdt(&reader.state);
        assert_eq!(
            incoming.insert_record(&reader.state, forged),
            safemesh_crdt::Admission::Accepted
        );
        assert_eq!(
            reader
                .try_merge_log_bytes(&incoming.to_wire_bytes().unwrap(), None)
                .unwrap(),
            vec![verdict]
        );
        assert_eq!(reader.elements(), before.0);
        assert_eq!(reader.tombstones(), before.1);
        assert_eq!(reader.log.records().len(), 1);
    }

    #[test]
    fn python_string_orset_replica_reports_log_collision_without_moving_state() {
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
        let mut replica = replica(2);
        assert_eq!(
            replica
                .try_merge_record_bytes(&first.to_wire_bytes().unwrap(), None)
                .unwrap(),
            "accepted"
        );
        let state = replica.state.clone();
        let log = replica.log.clone();
        let mut incoming = EventLog::for_crdt(&replica.state);
        assert_eq!(
            incoming.insert_record(&replica.state, second),
            safemesh_crdt::Admission::Accepted
        );
        assert_eq!(
            replica
                .try_merge_log_bytes(&incoming.to_wire_bytes().unwrap(), None)
                .unwrap(),
            vec!["collision"]
        );
        assert_eq!(replica.state, state);
        assert_eq!(replica.log, log);
    }

    #[test]
    fn python_string_orset_replica_refuses_corrupted_bytes_without_panicking() {
        let mut author = replica(1);
        let good = author.try_append_add("vaccine".to_string(), 11).unwrap();

        let mut reader = replica(2);
        assert_eq!(
            reader.try_merge_record_bytes(&good, None).unwrap(),
            "accepted"
        );
        assert_eq!(reader.elements(), vec!["vaccine".to_string()]);

        let mut planted = good.clone();
        planted[0] ^= 0xff;
        let mut reader = replica(2);
        let error = reader.try_merge_record_bytes(&planted, None).unwrap_err();
        assert_eq!(error, "failed to decode record: unexpected wire tag");
        assert!(reader.elements().is_empty());
        assert_eq!(reader.log.records().len(), 0);

        // Every single-byte change on the bare record path either errors or
        // decodes as a visibly different record. None panics, none is absorbed
        // as the original.
        let original = Replica::try_inspect_record_bytes(&good, None).unwrap();
        let (mut errored, mut decoded_differently) = (0usize, 0usize);
        for position in 0..good.len() {
            let mut bad = good.clone();
            bad[position] ^= 0x01;
            let mut reader = replica(2);
            match reader.try_merge_record_bytes(&bad, None) {
                Err(_) => {
                    errored += 1;
                    assert!(reader.elements().is_empty());
                    assert_eq!(reader.log.records().len(), 0);
                }
                Ok(_) => {
                    decoded_differently += 1;
                    let seen = Replica::try_inspect_record_bytes(&bad, None).unwrap();
                    assert_ne!(
                        (
                            seen.replica(),
                            seen.sequence(),
                            seen.element(),
                            seen.token()
                        ),
                        (
                            original.replica(),
                            original.sequence(),
                            original.element(),
                            original.token()
                        )
                    );
                }
            }
        }
        assert_eq!(errored + decoded_differently, good.len());
        assert!(errored > 0);

        // The event-log frame carries a CRC, so every single-byte change errors.
        let log = author.try_log_bytes().unwrap();
        for position in 0..log.len() {
            let mut bad = log.clone();
            bad[position] ^= 0x01;
            let mut reader = replica(2);
            assert!(reader.try_merge_log_bytes(&bad, None).is_err());
            assert!(reader.elements().is_empty());
        }
        let mut reader = replica(2);
        reader.try_merge_log_bytes(&log, None).unwrap();
        assert_eq!(reader.elements(), vec!["vaccine".to_string()]);
    }

    #[test]
    fn python_string_orset_replica_keeps_core_token_semantics() {
        // (a) A reused token keeps both pairs.
        let mut replica_a = replica(1);
        replica_a.try_append_add("a".to_string(), 7).unwrap();
        replica_a.try_append_add("b".to_string(), 7).unwrap();
        let mut core = OrSet::<String, u64>::new();
        core.add("a".to_string(), 7);
        core.add("b".to_string(), 7);
        assert_eq!(replica_a.elements(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(
            replica_a.elements(),
            core.elements().into_iter().collect::<Vec<_>>()
        );
        assert_eq!(
            replica_a.add_entries(),
            vec![("a".to_string(), 7), ("b".to_string(), 7)]
        );
        // Tokens are global: removing what was observed for `a` tombstones 7 and
        // takes `b` with it, as the core does.
        replica_a
            .try_append_remove_observed("a".to_string())
            .unwrap();
        core.apply_remove([7]);
        assert!(replica_a.elements().is_empty());
        assert!(core.elements().is_empty());

        // (b) Observed tokens exclude removals, while tombstones retain history.
        let mut replica_b = replica(1);
        replica_b.try_append_add("a".to_string(), 7).unwrap();
        let remove = replica_b
            .try_append_remove_observed("a".to_string())
            .unwrap();
        let mut core = OrSet::<String, u64>::new();
        core.add("a".to_string(), 7);
        core.apply_remove([7]);
        assert_eq!(
            replica_b.observed_tokens("a".to_string()),
            Vec::<u64>::new()
        );
        assert_eq!(
            replica_b.observed_tokens("a".to_string()),
            core.observed_tokens(&"a".to_string())
                .into_iter()
                .collect::<Vec<_>>()
        );
        assert!(replica_b.elements().is_empty());
        assert_eq!(replica_b.tombstones(), vec![7]);
        let record = Replica::try_inspect_record_bytes(&remove, None).unwrap();
        assert_eq!(record.delta_kind(), "remove");
        assert_eq!(record.tokens(), vec![7]);
    }

    #[test]
    fn python_string_orset_record_inspector_reports_core_decoded_fields() {
        let mut author = replica(9);
        let add = author.try_append_add("vaccine".to_string(), 11).unwrap();
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

        let view = Replica::try_inspect_record_bytes(&add, None).unwrap();
        assert_eq!(
            fields(&view),
            (9, 1, "add", Some("vaccine".to_string()), Some(11), vec![])
        );

        let remove = author
            .try_append_remove_observed("vaccine".to_string())
            .unwrap();
        let view = Replica::try_inspect_record_bytes(&remove, None).unwrap();
        assert_eq!(fields(&view), (9, 2, "remove", None, None, vec![11]));

        // Inspecting admits nothing: a reader still accepts the record afterwards.
        let mut reader = replica(2);
        assert_eq!(
            reader.try_merge_record_bytes(&add, None).unwrap(),
            "accepted"
        );

        let mut bad = add.clone();
        bad[0] ^= 0xff;
        assert_eq!(
            Replica::try_inspect_record_bytes(&bad, None).unwrap_err(),
            "failed to decode record: unexpected wire tag"
        );
    }

    #[test]
    fn python_string_orset_replica_surface() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let module = PyModule::new_bound(py, "safemesh_python").unwrap();
            safemesh_python(&module).unwrap();
            let globals = pyo3::types::PyDict::new_bound(py);
            globals.set_item("sm", module).unwrap();
            py.run_bound(
                include_str!("../tests/string_orset_replica.py"),
                Some(&globals),
                None,
            )
            .unwrap();
        });
    }
}
