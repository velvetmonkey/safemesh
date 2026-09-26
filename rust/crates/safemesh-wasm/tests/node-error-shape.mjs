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
  "failed to decode record: unexpected wire tag",
);

function replaceRecordInLog(log, original, replacement) {
  const frame = Uint8Array.from(log);
  let offset = -1;
  for (let i = 0; i <= frame.length - original.length; i++) {
    if (original.every((byte, j) => frame[i + j] === byte)) {
      offset = i;
      break;
    }
  }
  assert.notEqual(offset, -1, "record is present in encoded log");
  frame.set(replacement, offset);
  let crc = 0xffffffff;
  for (const byte of frame.subarray(1, frame.length - 4)) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  new DataView(frame.buffer).setUint32(frame.length - 4, (crc ^ 0xffffffff) >>> 0, true);
  return frame;
}

function caughtError(operation) {
  try {
    operation();
    assert.fail("expected SafeMeshError");
  } catch (error) {
    assert.equal(error.name, "SafeMeshError");
    return { code: error.code, message: error.message };
  }
}

const ownershipErrors = [];
for (const [name, coordinate] of [["out-of-range", 2], ["not-owned", 0]]) {
  const author = new wasm.SafeMeshGCounterReplica(1n, 2);
  const valid = author.appendBump(1, 5n);
  const invalid = Uint8Array.from(valid);
  new DataView(invalid.buffer).setBigUint64(22, BigInt(coordinate), true);
  const log = replaceRecordInLog(author.logBytes(), valid, invalid);
  const receiver = new wasm.SafeMeshGCounterReplica(0n, 2);
  const singleError = caughtError(() => receiver.mergeRecordBytes(invalid));
  const logError = caughtError(() => receiver.mergeLogBytes(log));
  console.log(`WASM ${name}: single=${JSON.stringify(singleError)}, log=${JSON.stringify(logError)}`);
  ownershipErrors.push({ name, singleError, logError });
  author.free();
  receiver.free();
}
{
  const author = new wasm.SafeMeshGCounterReplica(1n, 2);
  const first = author.appendBump(1, 5n);
  const second = author.appendBump(1, 6n);
  const badSecond = Uint8Array.from(second);
  new DataView(badSecond.buffer).setBigUint64(22, 0n, true);
  const log = replaceRecordInLog(author.logBytes(), second, badSecond);
  const receiver = new wasm.SafeMeshGCounterReplica(0n, 2);
  const error = caughtError(() => receiver.mergeLogBytes(log));
  console.log(`WASM good-then-bad: error=${JSON.stringify(error)}, value=${receiver.value()}`);
  assert.equal(receiver.value(), 0n);
  assert.equal(receiver.logBytes().length < log.length, true);
  author.free();
  receiver.free();
}
for (const { name, singleError, logError } of ownershipErrors) {
  assert.deepEqual(logError, singleError, `${name} ownership error differs by path`);
  assert.deepEqual(singleError, {
    code: 2,
    message: "counter coordinate out of range or not owned by record author",
  });
}

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
  [() => new wasm.SafeMeshPnCounterReplica(0n, 2), r => r.appendInc(0, 1n)],
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
        const beforeBudget = snapshot(untouched);
        assertSafeMeshError(() => untouched.mergeLogBytes(frame, undefined, 2), 1,
          "failed to decode event log: RecordLimitExceeded: 2");
        assert.deepEqual(snapshot(untouched), beforeBudget);
        for (const invalid of [-1, 1.5, "2", true]) {
          assertSafeMeshError(() => untouched.mergeLogBytes(frame, undefined, invalid), 2,
            "maxRecords must be a nonnegative integer at most 4294967295");
          assert.deepEqual(snapshot(untouched), beforeBudget);
        }
        assert.deepEqual(untouched.mergeLogBytes(frame, undefined, 3), expected);
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

// A single record gets the verdict its log merge names, and neither path throws
// for it. Two writers reusing record ID (1, 1) with different payloads collide.
let verdictCases = 0;
for (const [make, write, forge] of [
  [id => new wasm.SafeMeshGCounterReplica(id, 2), r => r.appendBump(1, 5n), r => r.appendBump(1, 9n)],
  [id => new wasm.SafeMeshPnCounterReplica(id, 2), r => r.appendInc(1, 5n), r => r.appendDec(1, 9n)],
  [id => new wasm.SafeMeshEnableWinsFlagReplica(id), r => r.appendEnable(5n), r => r.appendEnable(9n)],
  [id => new wasm.SafeMeshLwwMapReplica(id), r => r.appendSet(1n, 1n, 1n, 5n), r => r.appendRemove(1n, 2n, 1n)],
  [id => new wasm.SafeMeshLwwRegisterReplica(id), r => r.appendSet(1n, 1n, 5n), r => r.appendSet(2n, 1n, 9n)],
  [id => new wasm.SafeMeshStringOrSetReplica(id), r => r.appendAdd("first", 5n), r => r.appendAdd("second", 9n)],
]) {
  const author = make(1n), forger = make(1n), receiver = make(0n);
  try {
    const name = receiver.constructor.name;
    const record = write(author), forged = forge(forger);
    assert.equal(receiver.mergeRecordBytes(record), "accepted", name);
    const before = snapshot(receiver);
    for (const [single, batch, verdict] of [
      [record, author.logBytes(), "duplicate"],
      [forged, forger.logBytes(), "collision"],
    ]) {
      const result = receiver.mergeRecordBytes(single);
      assert.equal(result, verdict, name);
      assert.deepEqual(receiver.mergeLogBytes(batch), [result], name);
      assert.deepEqual(snapshot(receiver), before, name);
      verdictCases++;
    }
    assert.throws(() => receiver.mergeRecordBytes(new Uint8Array([0])), error =>
      error.name === "SafeMeshError" && error.code === 1 && error.message.startsWith("failed to decode record"));
    assert.deepEqual(snapshot(receiver), before, name);
  } finally { author.free(); forger.free(); receiver.free(); }
}
// Ownership refusals are errors, not verdicts: coordinate 2 is outside width 2,
// and an allocated replica refuses a new record claiming its own author.
for (const [wide, narrow, write] of [
  [new wasm.SafeMeshGCounterReplica(2n, 3), new wasm.SafeMeshGCounterReplica(0n, 2), r => r.appendBump(2, 5n)],
  [new wasm.SafeMeshPnCounterReplica(2n, 3), new wasm.SafeMeshPnCounterReplica(0n, 2), r => r.appendInc(2, 5n)],
]) {
  try {
    const record = write(wide), before = snapshot(narrow);
    assert.throws(() => narrow.mergeRecordBytes(record), error =>
      error.name === "SafeMeshError" && error.code === 2 && /not owned by record author/.test(error.message));
    assert.deepEqual(snapshot(narrow), before);
  } finally { wide.free(); narrow.free(); }
}
{
  const allocated = wasm.SafeMeshStringOrSetReplica.createAllocated(2n, 0n);
  const impostor = new wasm.SafeMeshStringOrSetReplica(0n);
  try {
    const record = impostor.appendAdd("clone", 2n), before = snapshot(allocated);
    assert.throws(() => allocated.mergeRecordBytes(record), error =>
      error.name === "SafeMeshError" && error.code === 1 && /local author/.test(error.message));
    assert.deepEqual(snapshot(allocated), before);
  } finally { allocated.free(); impostor.free(); }
}
console.log(`SINGLE_RECORD_VERDICT_CASES=${verdictCases}`);

// Audit CRDT-core probes: exact reads, dimensions, and token collisions.
{
  const a = new wasm.SafeMeshGCounter(2);
  const b = new wasm.SafeMeshGCounterReplica(0n, 1);
  const c = new wasm.SafeMeshGCounterReplica(1n, 2);
  const left = new wasm.SafeMeshOrSet();
  const right = new wasm.SafeMeshOrSet();
  const ab = new wasm.SafeMeshOrSet();
  const ba = new wasm.SafeMeshOrSet();
  try {
    a.applyBump(0, 9007199254740991n);
    a.applyBump(1, 2n);
    assert.equal(a.value(), 9007199254740993n);
    c.appendBump(1, 8n);
    assert.throws(() => b.mergeLogBytes(c.logBytes()));
    assert.throws(() => c.mergeLogBytes(b.logBytes()));
    assert.deepEqual([...b.state()], [0n]);
    assert.deepEqual([...c.state()], [0n, 8n]);
    left.add(1n, 7n); right.add(2n, 7n);
    ab.merge(left); ab.merge(right);
    ba.merge(right); ba.merge(left);
    assert.deepEqual([...ab.elements()], [1n, 2n]);
    assert.deepEqual([...ba.elements()], [...ab.elements()]);
  } finally { for (const obj of [a,b,c,left,right,ab,ba]) obj.free(); }
  console.log('CRDT_AUDIT_BOUNDARIES=true');
}

// Each old build traps on capacity overflow; isolate it so every constructor is
// measured without reusing a WASM instance after an unreachable instruction.
const { spawnSync } = await import('node:child_process');
const capacityFailures = [];
for (const name of ['SafeMeshGCounter', 'SafeMeshGCounterReplica']) {
  const result = spawnSync(process.execPath, ['-e', `
    const assert = require('node:assert/strict');
    const wasm = require(process.argv[1]);
    const make = n => process.argv[2] === 'SafeMeshGCounter'
      ? new wasm.SafeMeshGCounter(n) : new wasm.SafeMeshGCounterReplica(0n, n);
    for (const n of [2 ** 28, 2 ** 32 - 1]) {
      assert.throws(() => make(n), e => e.name === 'SafeMeshError' && e.code === 2 && /replicas/.test(e.message));
      const valid = make(2);
      assert.equal(valid.value(), 0n);
      valid.free();
    }
  `, join(packageDir, 'safemesh_wasm.js'), name], { encoding: 'utf8' });
  if (result.status !== 0) capacityFailures.push(`${name}: ${result.stderr}`);
}
console.log(`COUNTER_CAPACITY_CONSTRUCTORS=2 FAILURES=${capacityFailures.length}`);
assert.deepEqual(capacityFailures, []);

// Retained legacy EventLog fixtures: every JS log loader names the frame found,
// the frame expected and the migration step, with the core text intact.
{
  const { readFileSync } = await import("node:fs");
  const fixtures = new URL("../../safemesh-crdt/tests/fixtures/legacy-event-log/", import.meta.url);
  const step = "expected tag 0x03 with shape header; migrate once with " +
    "EventLog::migrate_legacy_wire_bytes_for(bytes, &destination) or " +
    "`cargo run -p safemesh-crdt --example migrate_event_log`, giving the original " +
    "replica count (safemesh-crdt README, \"Migrating a legacy EventLog\")";
  const frames = [
    ["tag02", "tag 0x02 (no CRC, no shape header)"],
    ["tag03-unshaped", "tag 0x03 without shape header"],
  ];
  const loaders = [
    ["gcounter.log", () => new wasm.SafeMeshGCounterReplica(0n, 2)],
    ["pncounter.log", () => new wasm.SafeMeshPnCounterReplica(0n, 2)],
    ["enable-wins-flag-u64.log", () => new wasm.SafeMeshEnableWinsFlagReplica(0n)],
    ["lww-map-u64.log", () => new wasm.SafeMeshLwwMapReplica(0n)],
    ["lww-register-u64.log", () => new wasm.SafeMeshLwwRegisterReplica(0n)],
    ["orset-utf8.log", () => new wasm.SafeMeshStringOrSetReplica(0n)],
  ];
  let legacyCases = 0;
  for (const [dir, found] of frames) {
    const message = `failed to decode event log: legacy EventLog frame: found ${found}, ${step}`;
    for (const [file, make] of loaders) {
      const bytes = readFileSync(new URL(`${dir}/${file}`, fixtures));
      const replica = make();
      assertSafeMeshError(() => replica.mergeLogBytes(bytes), 1, message);
      replica.free();
      legacyCases += 1;
    }
    console.log(`WASM ${dir}: ${message}`);
  }
  console.log(`LEGACY_EVENT_LOG_CASES=${legacyCases}`);
}
