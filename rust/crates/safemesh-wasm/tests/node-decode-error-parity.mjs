#!/usr/bin/env node

// Both decode paths of every WASM replica name the same core WireError for the
// same malformed input. Truncation and trailing bytes are applied to each path's
// own frame: a record frame handed to the log decoder is only an unexpected tag.

import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

const packageDir = resolve(process.argv[2] ?? "pkg-node");
const require = createRequire(import.meta.url);
const wasm = require(join(packageDir, "safemesh_wasm.js"));

const RECORD_PREFIX = "failed to decode record: ";
const LOG_PREFIX = "failed to decode event log: ";

// safemesh-crdt's `Display` text for the WireError each case must produce.
const UNEXPECTED_EOF = "unexpected end of wire input";
const TRAILING_BYTES = "unexpected trailing bytes after wire value";
const INVALID_TAG = "unexpected wire tag";

const replicas = [
  ["SafeMeshGCounterReplica", () => new wasm.SafeMeshGCounterReplica(1n, 2), (r) => r.appendBump(1, 5n)],
  ["SafeMeshPnCounterReplica", () => new wasm.SafeMeshPnCounterReplica(1n, 2), (r) => r.appendInc(1, 5n)],
  ["SafeMeshEnableWinsFlagReplica", () => new wasm.SafeMeshEnableWinsFlagReplica(1n), (r) => r.appendEnable(5n)],
  ["SafeMeshLwwRegisterReplica", () => new wasm.SafeMeshLwwRegisterReplica(1n), (r) => r.appendSet(1n, 1n, 5n)],
  ["SafeMeshLwwMapReplica", () => new wasm.SafeMeshLwwMapReplica(1n), (r) => r.appendSet(1n, 1n, 1n, 5n)],
  ["SafeMeshStringOrSetReplica", () => new wasm.SafeMeshStringOrSetReplica(1n), (r) => r.appendAdd("x", 5n)],
];

const truncate = (bytes) => bytes.slice(0, bytes.length - 1);
const trail = (bytes) => Uint8Array.from([...bytes, 0]);

function cause(operation, prefix) {
  try {
    operation();
  } catch (error) {
    assert.equal(error.name, "SafeMeshError");
    assert.equal(error.code, 1);
    return error.message.startsWith(prefix) ? error.message.slice(prefix.length) : null;
  }
  assert.fail("expected SafeMeshError");
}

const failures = [];
for (const [name, create, append] of replicas) {
  const author = create();
  const record = append(author);
  const log = author.logBytes();
  author.free();
  const cases = [
    ["empty_bytes", new Uint8Array(), new Uint8Array(), UNEXPECTED_EOF],
    ["truncated_frame", truncate(record), truncate(log), UNEXPECTED_EOF],
    ["trailing_bytes", trail(record), trail(log), TRAILING_BYTES],
    ["undecodable_bytes", new Uint8Array([0]), new Uint8Array([0]), INVALID_TAG],
  ];
  for (const [kase, recordInput, logInput, expected] of cases) {
    const target = create();
    const before = target.logBytes();
    const single = cause(() => target.mergeRecordBytes(recordInput), RECORD_PREFIX);
    const batch = cause(() => target.mergeLogBytes(logInput), LOG_PREFIX);
    assert.deepEqual(target.logBytes(), before);
    target.free();
    console.log(`WASM ${name} ${kase}: record=${JSON.stringify(single)} log=${JSON.stringify(batch)}`);
    if (single !== batch) failures.push(`${name} ${kase}: causes differ by path`);
    if (single !== expected) failures.push(`${name} ${kase}: ${JSON.stringify(single)} is not ${JSON.stringify(expected)}`);
  }
}
assert.deepEqual(failures, []);

console.log("NODE_DECODE_ERROR_PARITY=true");
