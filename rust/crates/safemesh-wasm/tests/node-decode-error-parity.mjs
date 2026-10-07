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

const defaultLimit = wasm.defaultRecordLimit();
const defaultCause = `RecordLimitExceeded: ${defaultLimit}; pass unlimitedRecords() as maxRecords to retry`;
assert.equal(typeof wasm.unlimitedRecords, 'function');

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

// Reuse one product encoded record per replica type at both sides of the default.
const crcTable = Array.from({ length: 256 }, (_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit++) value = (value >>> 1) ^ ((value & 1) ? 0xedb88320 : 0);
  return value >>> 0;
});
function repeatedLog(input, count) {
  const frame = Buffer.from(input);
  let countOffset = 17 + frame.readUInt32LE(13);
  if (frame[countOffset++] === 1) countOffset += 8;
  assert.equal(frame.readUInt32LE(countOffset), 1);
  const record = frame.subarray(countOffset + 4, frame.length - 4);
  const bodyLength = countOffset - 9 + 4 + record.length * count;
  const output = Buffer.alloc(1 + 8 + bodyLength + 4);
  output[0] = 3;
  output.writeUInt32LE(bodyLength, 1);
  output.writeUInt32LE((~bodyLength) >>> 0, 5);
  frame.copy(output, 9, 9, countOffset);
  output.writeUInt32LE(count, countOffset);
  for (let i = 0; i < count; i++) record.copy(output, countOffset + 4 + i * record.length);
  let crc = 0xffffffff;
  for (let i = 1; i < output.length - 4; i++) crc = (crc >>> 8) ^ crcTable[(crc ^ output[i]) & 255];
  output.writeUInt32LE((~crc) >>> 0, output.length - 4);
  return output;
}
for (const [name, create, append] of replicas) {
  const source = create(); append(source);
  const at = repeatedLog(source.logBytes(), defaultLimit);
  const over = repeatedLog(source.logBytes(), defaultLimit + 1);
  const six = repeatedLog(source.logBytes(), 6);
  source.free();
  const target = create(); const before = target.logBytes();
  assert.equal(cause(() => target.mergeLogBytes(over), LOG_PREFIX), defaultCause, name);
  assert.equal(cause(() => target.mergeLogBytes(over, undefined, defaultLimit), LOG_PREFIX), `RecordLimitExceeded: ${defaultLimit}`, name);
  assert.deepEqual(target.logBytes(), before, name);
  assert.equal(target.mergeLogBytes(at).length, defaultLimit, name);
  target.free();
  const unlimited = create();
  assert.equal(unlimited.mergeLogBytes(over, undefined, wasm.unlimitedRecords()).length, defaultLimit + 1, name);
  unlimited.free();
  const limited = create();
  assert.equal(cause(() => limited.mergeLogBytes(six, undefined, 5), LOG_PREFIX), 'RecordLimitExceeded: 5', name);
  limited.free();
}
console.log('NODE_DEFAULT_RECORD_LIMIT=6 PASS');

{
  const SetReplica = wasm.SafeMeshStringOrSetReplica;
  const author = SetReplica.createAllocated(3n, 0n);
  author.appendAllocatedAdd('large history');
  const prefix = Buffer.from(author.exportIdentity()).subarray(0, 29);
  const at = Buffer.concat([prefix, repeatedLog(author.logBytes(), defaultLimit)]);
  const over = Buffer.concat([prefix, repeatedLog(author.logBytes(), defaultLimit + 1)]);
  const six = Buffer.concat([prefix, repeatedLog(author.logBytes(), 6)]);
  author.free();
  assert.equal(cause(() => SetReplica.importIdentity(over), LOG_PREFIX), defaultCause);
  assert.equal(cause(() => SetReplica.importIdentity(over, defaultLimit), LOG_PREFIX), `RecordLimitExceeded: ${defaultLimit}`);
  const restored = SetReplica.importIdentity(over, wasm.unlimitedRecords());
  assert.deepEqual(restored.elements(), ['large history']);
  restored.free();
  const exact = SetReplica.importIdentity(at);
  assert.deepEqual(exact.elements(), ['large history']);
  exact.free();
  assert.equal(cause(() => SetReplica.importIdentity(six, 5), LOG_PREFIX), 'RecordLimitExceeded: 5');
}
console.log('NODE_DEFAULT_IDENTITY_LIMIT=1 PASS');

class MemoryStore {
  revision = 0n; bytes; lease;
  open(mode, writer) {
    if (mode !== 'fresh' || this.lease) throw Error('unexpected store open');
    return this.lease = { writer };
  }
  readCommitted(lease) {
    assert.equal(lease, this.lease);
    return { bytes: this.bytes.slice(), revision: this.revision, anchor: this.revision };
  }
  commit(lease, expectedRevision, bytes) {
    assert.equal(lease, this.lease);
    assert.equal(expectedRevision, this.revision);
    this.bytes = bytes.slice(); return ++this.revision;
  }
  close(lease) { assert.equal(lease, this.lease); this.lease = undefined; }
}
for (const [name, makeSource, append, Managed] of [
  ['counter', () => new wasm.SafeMeshGCounterReplica(1n, 2), r => r.appendBump(1, 1n), wasm.SafeMeshManagedGCounter],
  ['set', () => wasm.SafeMeshStringOrSetReplica.createAllocated(2n, 1n), r => r.appendAllocatedAdd('x'), wasm.SafeMeshManagedStringOrSet],
]) {
  const source = makeSource(); append(source);
  const at = repeatedLog(source.logBytes(), defaultLimit);
  const over = repeatedLog(source.logBytes(), defaultLimit + 1);
  source.free();
  const create = () => {
    const store = new MemoryStore();
    const replica = Managed.open(store, { mode: 'fresh', writer: 0n, writers: 2 });
    return [store, replica];
  };
  const [store, target] = create(); const before = target.peerLogBytes(); const revision = store.revision;
  assert.throws(() => target.mergeLogBytes(over), /RecordLimitExceeded/, name);
  assert.deepEqual(target.peerLogBytes(), before, name);
  assert.equal(store.revision, revision, name);
  assert.equal(target.mergeLogBytes(at).length, defaultLimit, name);
  target.close(); target.free();
  const [, unlimited] = create();
  assert.equal(unlimited.mergeLogBytes(over, undefined, wasm.unlimitedRecords()).length, defaultLimit + 1, name);
  unlimited.close(); unlimited.free();
  const [, limited] = create();
  assert.throws(() => limited.mergeLogBytes(at, undefined, 5), /RecordLimitExceeded/, name);
  limited.close(); limited.free();
}
console.log('NODE_DEFAULT_MANAGED_LIMIT=2 PASS');

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
  const started = performance.now();
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
  console.log('retention_wasm_merge_and_identity_all_5000 PASS seconds=' + (performance.now() - started) / 1000);
}
retention_wasm_merge_and_identity_all_5000();
