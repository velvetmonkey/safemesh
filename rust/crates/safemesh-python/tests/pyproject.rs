// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: BUSL-1.1

const PYPROJECT: &str = include_str!("../pyproject.toml");

#[test]
fn pyproject_declares_maturin_pyo3_binding() {
    assert!(PYPROJECT.contains("build-backend = \"maturin\""));
    assert!(PYPROJECT.contains("bindings = \"pyo3\""));
    assert!(PYPROJECT.contains("module-name = \"safemesh_python\""));
}
