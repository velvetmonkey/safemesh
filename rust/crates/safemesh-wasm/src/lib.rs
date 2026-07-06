// SafeMesh — delta-state CRDT convergence, built on crdt-lean.
// Copyright (C) 2026 Ben Cassie
// SPDX-License-Identifier: AGPL-3.0-or-later

use safemesh_crdt::{GCounter, GCounterDelta, WireEncode};
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
