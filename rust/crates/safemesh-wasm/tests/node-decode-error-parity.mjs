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

// Decode only the public frame envelope to enumerate identities; payloads are
// checked independently by the product loader and by unique visible elements.
function retentionIds(bytes) {
  const frame = Buffer.from(bytes);
  let offset = 17 + frame.readUInt32LE(13); // tag, length pair, marker, schema length
  const shape = frame[offset++];
  if (shape === 1) offset += 8;
  const count = frame.readUInt32LE(offset); offset += 4;
  const ids = [];
  for (let i = 0; i < count; i++) {
    const length = frame.readUInt32LE(offset); offset += 4;
    // A record opens with its tag followed by author and sequence.
    ids.push(`${frame.readBigUInt64LE(offset + 1)}:${frame.readBigUInt64LE(offset + 9)}`);
    offset += length;
  }
  return ids.sort();
}

function retention_wasm_merge_and_identity_all_5000() {
  const SetReplica = wasm.SafeMeshStringOrSetReplica;
  const authors = [0n, 1n, 2n].map(a => SetReplica.createAllocated(3n, a));
  const expectedIds = [];
  for (let index = 0; index < 5000; index++) {
    const author = index % 3;
    const record = authors[author].appendAllocatedAdd(`retained-${String(index).padStart(4, '0')}`);
    expectedIds.push(`${author}:${Math.floor(index / 3) + 1}`);
    if (author !== 0) assert.equal(authors[0].mergeRecordBytes(record), 'accepted');
  }
  const log = authors[0].logBytes();
  const identity = authors[0].exportIdentity();
  const elements = authors[0].elements();
  const check = replica => {
    assert.equal(retentionIds(replica.logBytes()).length, 5000, 'retention count');
    assert.deepEqual(retentionIds(replica.logBytes()), expectedIds.sort(), 'retention identity set');
    assert.deepEqual(replica.elements(), elements, 'retention every unique element');
    assert.deepEqual(replica.logBytes(), log, 'retention byte-identical log');
  };
  const fresh = new SetReplica(0n);
  fresh.appendAdd('destination-before-refusal', 999999n);
  const before = fresh.logBytes();
  const stateBefore = fresh.elements();
  assert.throws(() => fresh.mergeLogBytes(log, null, 4999), /RecordLimitExceeded/);
  assert.deepEqual(fresh.logBytes(), before);
  assert.deepEqual(fresh.elements(), stateBefore);
  fresh.free();
  const exact = new SetReplica(0n);
  assert.deepEqual(exact.mergeLogBytes(log, null, 5000), Array(5000).fill('accepted'));
  check(exact);
  exact.free();
  authors.forEach(a => a.free());
  assert.throws(() => SetReplica.importIdentity(identity, 4999), /RecordLimitExceeded/);
  const restored = SetReplica.importIdentity(identity, 5000);
  check(restored);
  assert.deepEqual(restored.exportIdentity(), identity, 'retention byte-identical identity');
  restored.free();
  console.log('retention_wasm_merge_and_identity_all_5000 PASS');
}
retention_wasm_merge_and_identity_all_5000();
