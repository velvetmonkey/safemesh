#!/usr/bin/env node

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

const packageDir = resolve(process.argv[2] ?? "pkg-node");
const require = createRequire(import.meta.url);
const wasm = require(join(packageDir, "safemesh_wasm.js"));

function assertSafeMeshError(operation, code, message) {
  assert.throws(operation, (error) => {
    assert.equal(error instanceof Error, true);
    assert.equal(error.constructor.name, "SafeMeshError");
    assert.equal(error.name, "SafeMeshError");
    assert.equal(error.code, code);
    assert.equal(error.message, message);
    return true;
  });
}

const counter = new wasm.SafeMeshGCounter(2);
assertSafeMeshError(
  () => counter.tryApplyBump(2, 1n),
  2,
  "replica out of range",
);

const replica = new wasm.SafeMeshGCounterReplica(1n, 2);
assertSafeMeshError(
  () => replica.appendBump(2, 1n),
  2,
  "counter replica out of range",
);
assertSafeMeshError(
  () => replica.mergeRecordBytes(new Uint8Array([0])),
  1,
  "failed to decode record",
);

console.log("NODE_ERROR_SHAPE=true");
