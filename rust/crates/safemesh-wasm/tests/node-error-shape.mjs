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
  "counter coordinate out of range or not owned by record author",
);
assertSafeMeshError(
  () => replica.mergeRecordBytes(new Uint8Array([0])),
  1,
  "failed to decode record",
);

console.log("NODE_ERROR_SHAPE=true");

// Exercise the original JS arguments against the compiled binding. Keep all cases
// running so an old build reports the whole narrowing population, not just NaN.
const invalidNumbers = [0.5, NaN, Infinity, -Infinity, -0, 2 ** 32, 2 ** 53, -1, "0", 0n, null, undefined, true, {}];
const invalidBigints = [-1n, 2n ** 64n, 0.5, NaN, Infinity, -Infinity, -0, 2 ** 32, 2 ** 53, "0", 0, null, undefined, true, {}];
const numericExports = [
  ["SafeMeshGCounter", "constructor", ["usize"]],
  ["SafeMeshGCounter", "applyBump", ["usize", "u64"]],
  ["SafeMeshGCounter", "tryApplyBump", ["usize", "u64"]],
  [null, "gcounterDeltaToWire", ["usize", "u64"]],
  ["SafeMeshLwwRegister", "set", ["u64", "u64", "u64"]],
  ["SafeMeshLwwRegister", "valueOr", ["u64"]],
  ["SafeMeshLwwRegister", "timestampOr", ["u64"]],
  ["SafeMeshLwwRegister", "writerReplicaOr", ["u64"]],
  [null, "lwwRegisterDeltaToWire", ["u64", "u64", "u64"]],
  ["SafeMeshLwwMap", "set", ["u64", "u64", "u64", "u64"]],
  ["SafeMeshLwwMap", "remove", ["u64", "u64", "u64"]],
  ["SafeMeshLwwMap", "hasKey", ["u64"]],
  ["SafeMeshLwwMap", "valueOr", ["u64", "u64"]],
  [null, "lwwMapSetDeltaToWire", ["u64", "u64", "u64", "u64"]],
  [null, "lwwMapRemoveDeltaToWire", ["u64", "u64", "u64"]],
  ["SafeMeshEnableWinsFlag", "enable", ["u64"]],
  [null, "enableWinsFlagEnableDeltaToWire", ["u64"]],
  [null, "enableWinsFlagDisableDeltaToWire", ["Vec<u64>"]],
  ["SafeMeshGCounterReplica", "constructor", ["u64", "usize"]],
  ["SafeMeshGCounterReplica", "appendBump", ["usize", "u64"]],
  ["SafeMeshGCounterReplica", "versionFor", ["u64"]],
  ["SafeMeshEnableWinsFlagReplica", "constructor", ["u64"]],
  ["SafeMeshEnableWinsFlagReplica", "appendEnable", ["u64"]],
  ["SafeMeshEnableWinsFlagReplica", "versionFor", ["u64"]],
  ["SafeMeshLwwMapReplica", "constructor", ["u64"]],
  ["SafeMeshLwwMapReplica", "appendSet", ["u64", "u64", "u64", "u64"]],
  ["SafeMeshLwwMapReplica", "appendRemove", ["u64", "u64", "u64"]],
  ["SafeMeshLwwMapReplica", "versionFor", ["u64"]],
  ["SafeMeshLwwMapReplica", "hasKey", ["u64"]],
  ["SafeMeshLwwMapReplica", "valueOr", ["u64", "u64"]],
  ["SafeMeshLwwRegisterReplica", "constructor", ["u64"]],
  ["SafeMeshLwwRegisterReplica", "appendSet", ["u64", "u64", "u64"]],
  ["SafeMeshLwwRegisterReplica", "versionFor", ["u64"]],
  ["SafeMeshLwwRegisterReplica", "valueOr", ["u64"]],
  ["SafeMeshLwwRegisterReplica", "timestampOr", ["u64"]],
  ["SafeMeshLwwRegisterReplica", "writerReplicaOr", ["u64"]],
  ["SafeMeshOrSet", "add", ["u64", "u64"]],
  ["SafeMeshOrSet", "applyRemove", ["Vec<u64>"]],
  ["SafeMeshOrSet", "observedTokens", ["u64"]],
  ["SafeMeshOrSet", "contains", ["u64"]],
  ["SafeMeshStringOrSetReplica", "constructor", ["u64"]],
  ["SafeMeshStringOrSetReplica", "createAllocated", ["JsValue", "JsValue"]],
  ["SafeMeshStringOrSetReplica", "appendAdd", ["String", "u64"]],
  ["SafeMeshStringOrSetReplica", "versionFor", ["u64"]],
];
function fresh(className) {
  if (className === "SafeMeshGCounter") return new wasm[className](2);
  if (className === "SafeMeshGCounterReplica") return new wasm[className](0n, 2);
  if (className.endsWith("Replica")) return new wasm[className](0n);
  return new wasm[className]();
}
function snapshot(object) {
  if (!object) return null;
  const result = {};
  for (const method of ["value", "state", "logBytes", "enabledTokens", "tombstoneTokens", "visibleKeys", "entryKeys", "removalKeys", "elements", "tombstones"]) {
    if (typeof object[method] === "function") result[method] = object[method]();
  }
  if (object.visibleKeys) result.mapValues = Array.from(object.visibleKeys(), key => [key, object.valueOr(key, 0n)]);
  if (object.timestampOr) result.timestamp = object.timestampOr(0n);
  if (object.writerReplicaOr) result.writer = object.writerReplicaOr(0n);
  if (object.valueOr) result.valueOr = object.constructor.name.includes("Map") ? object.valueOr(0n, 0n) : object.valueOr(0n);
  return result;
}
let numericCases = 0;
const numericFailures = [];
for (const [className, method, types] of numericExports) {
  for (let position = 0; position < types.length; position++) {
    const type = types[position];
    if (type === "String") continue;
    const invalid = type === "usize" ? invalidNumbers : type === "Vec<u64>" ? [...invalidBigints, [-1n], [2n ** 64n], ["0"], [0n, -1n]] : invalidBigints;
    for (const bad of invalid) {
      for (const populated of [false, true]) {
        const label = `${className ?? "module"}.${method}[${position}] ${String(bad)} (${typeof bad}; ${populated ? "populated" : "fresh"}${Object.is(bad, -0) ? ", negative zero" : ""})`;
        const object = className && method !== "constructor" && method !== "createAllocated" ? fresh(className) : null;
        if (populated && object) {
          if (object.tryApplyBump) object.tryApplyBump(0, 3n);
          else if (object.appendBump) object.appendBump(0, 3n);
          else if (object.enable) object.enable(3n);
          else if (object.appendEnable) object.appendEnable(3n);
          else if (object.appendAdd) object.appendAdd("item", 3n);
          else if (object.add) object.add(7n, 3n);
          else if (object.set) object.constructor.name.includes("Map") ? object.set(7n, 3n, 0n, 3n) : object.set(3n, 0n, 3n);
          else if (object.appendSet) object.constructor.name.includes("Map") ? object.appendSet(7n, 3n, 0n, 3n) : object.appendSet(3n, 0n, 3n);
        }
        const args = types.map(t => t === "usize" ? (method === "constructor" ? 2 : 0) : t === "String" ? "item" : t === "Vec<u64>" ? new BigUint64Array([7n]) : 7n);
        if (method === "createAllocated") { args[0] = 2n; args[1] = 0n; }
        args[position] = bad;
        const before = snapshot(object);
        let returned;
        let error;
        try {
          returned = method === "constructor" ? new wasm[className](...args) : method === "createAllocated" ? wasm[className][method](...args) : object ? object[method](...args) : wasm[method](...args);
        } catch (caught) { error = caught; }
        try {
          assert.deepEqual(snapshot(object), before, "refusal changed state or log bytes");
          assert.ok(error instanceof Error, "invalid input accepted");
          assert.equal(error.name, "SafeMeshError");
          assert.equal(error.constructor.name, "SafeMeshError");
          assert.equal(error.code, 2);
        } catch (failure) {
          numericFailures.push(`${label}: ${failure.message}`);
        } finally {
          object?.free();
          returned?.free?.();
        }
        numericCases++;
      }
    }
  }
}
for (const failure of numericFailures) console.error(`NUMERIC_RED ${failure}`);
console.log(`NUMERIC_EXPORTS=${numericExports.length} NUMERIC_CASES=${numericCases} NUMERIC_FAILURES=${numericFailures.length}`);
assert.equal(numericFailures.length, 0, "numeric boundary refusals");

// Zero is valid; negative zero is refused. The top coordinate and tally remain exact.
const maxU64 = (1n << 64n) - 1n;
for (const coordinate of [0, 1]) {
  const local = new wasm.SafeMeshGCounter(2);
  local.applyBump(coordinate, 0n);
  local.tryApplyBump(coordinate, maxU64);
  assert.equal(local.value(), maxU64);
  const author = new wasm.SafeMeshGCounterReplica(BigInt(coordinate), 2);
  const peer = new wasm.SafeMeshGCounterReplica(BigInt(1 - coordinate), 2);
  author.appendBump(coordinate, 0n);
  const record = author.appendBump(coordinate, maxU64);
  peer.mergeRecordBytes(record);
  assert.equal(author.value(), maxU64);
  assert.equal(peer.value(), maxU64);
  assert.equal(author.versionFor(BigInt(coordinate)), 2n);
  local.free(); author.free(); peer.free();
}
console.log("NUMERIC_LEGIT_VALUES=true");

// Every numeric entry point also accepts its legitimate scalar extremes.
let legitimateCases = 0;
for (const [className, method, types] of numericExports) {
  for (const bigint of [0n, maxU64]) {
    const object = className && method !== "constructor" && method !== "createAllocated" ? fresh(className) : null;
    const args = types.map(t => t === "usize" ? (method === "constructor" ? 2 : 0) : t === "String" ? "item" : t === "Vec<u64>" ? new BigUint64Array([bigint]) : bigint);
    if (method === "createAllocated") { args[0] = 2n; args[1] = bigint === 0n ? 0n : 1n; }
    let returned;
    try {
      returned = method === "constructor" ? new wasm[className](...args) : method === "createAllocated" ? wasm[className][method](...args) : object ? object[method](...args) : wasm[method](...args);
    } finally { object?.free(); returned?.free?.(); }
    legitimateCases++;
  }
}
const emptyCounter = new wasm.SafeMeshGCounter(0);
assert.equal(emptyCounter.value(), 0n);
emptyCounter.free();
const wire = wasm.gcounterDeltaToWire(2 ** 32 - 1, maxU64);
assert.equal(Buffer.from(wire).readBigUInt64LE(1), (1n << 32n) - 1n);
assert.equal(Buffer.from(wire).readBigUInt64LE(9), maxU64);
const map = new wasm.SafeMeshLwwMap();
assert.equal(map.valueOr(maxU64, maxU64), maxU64);
map.set(maxU64, maxU64, maxU64, maxU64);
assert.equal(map.hasKey(maxU64), true);
assert.equal(map.valueOr(maxU64, 0n), maxU64);
map.free();
console.log(`NUMERIC_LEGIT_EXPORT_CASES=${legitimateCases}`);

for (const method of ["applyBump", "tryApplyBump", "appendBump"]) {
  const object = method === "appendBump" ? new wasm.SafeMeshGCounterReplica(0n, 2) : new wasm.SafeMeshGCounter(2);
  object[method](0, 3n);
  for (const coordinate of [2, 2 ** 32 - 1]) {
    const before = snapshot(object);
    assert.throws(() => object[method](coordinate, 7n), error => error.name === "SafeMeshError" && error.code === 2);
    assert.deepEqual(snapshot(object), before);
  }
  object.free();
}
console.log("NUMERIC_SHAPE_REFUSALS=6");

// Exercise input positions independently of the deduplicating log encoder.
function frameOccurrences(frame, order) {
  const body = Buffer.from(frame).subarray(9, -4);
  const arity = 8 + body.readUInt32LE(4);
  const countOffset = arity + 1 + (body[arity] === 1 ? 8 : 0);
  const records = [];
  for (let pos = countOffset + 4; pos < body.length;) {
    const end = pos + 4 + body.readUInt32LE(pos);
    records.push(body.subarray(pos, end));
    pos = end;
  }
  const word = n => { const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0); return b; };
  const next = Buffer.concat([body.subarray(0, countOffset), word(order.length), ...order.map(i => records[i])]);
  const checked = Buffer.concat([word(next.length), word(~next.length), next]);
  let crc = 0xffffffff;
  for (const byte of checked) {
    crc ^= byte;
    for (let i = 0; i < 8; i++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return Buffer.concat([Buffer.from([3]), checked, word(~crc)]);
}
let occurrenceCases = 0;
for (const [make, append] of [
  [() => new wasm.SafeMeshGCounterReplica(0n, 2), r => r.appendBump(0, 1n)],
  [() => new wasm.SafeMeshEnableWinsFlagReplica(0n), r => r.appendEnable(1n)],
  [() => new wasm.SafeMeshLwwMapReplica(0n), r => r.appendSet(1n, 1n, 0n, 1n)],
  [() => new wasm.SafeMeshLwwRegisterReplica(0n), r => r.appendSet(1n, 0n, 1n)],
  [() => new wasm.SafeMeshStringOrSetReplica(0n), r => r.appendAdd("water", 1n)],
]) {
  const sender = make();
  try {
    append(sender); append(sender);
    for (const [order, expected] of [
      [[0, 0, 1], ["accepted", "duplicate", "accepted"]],
      [[0, 0, 0], ["accepted", "duplicate", "duplicate"]],
      [[0, 1, 0], ["accepted", "accepted", "duplicate"]],
    ]) {
      const receiver = make(), untouched = make();
      try {
        const frame = frameOccurrences(sender.logBytes(), order);
        assert.deepEqual(receiver.mergeLogBytes(frame), expected);
        const canonical = receiver.logBytes();
        assert.deepEqual(receiver.mergeLogBytes(frame), ["duplicate", "duplicate", "duplicate"]);
        assert.deepEqual(receiver.logBytes(), canonical);
        assert.deepEqual(receiver.mergeLogBytes(canonical), Array(new Set(order).size).fill("duplicate"));
        const before = snapshot(untouched);
        assert.throws(() => untouched.mergeLogBytes(Buffer.concat([frame, Buffer.from([0])])), error => error.name === "SafeMeshError" && error.code === 1);
        assert.deepEqual(snapshot(untouched), before);
        occurrenceCases++;
      } finally { receiver.free(); untouched.free(); }
    }
  } finally { sender.free(); }
}
console.log(`BATCH_OCCURRENCE_CASES=${occurrenceCases}`);
