// The record-ID collision alarm on every WASM replica class with an event log.
//
//   node node-collision-alarm.mjs /absolute/path/to/pkg-node
//
// Two replicas write one record ID with different payloads. The receiver of
// the offer refuses it as "collision" and returns a collision report; the
// offerer merges the report. Both then hold the alarm, neither state moves,
// and an equal payload stays "duplicate" with no report. A malformed report
// throws a named SafeMeshError, and the record and log decoders a peer without
// the report surface would use refuse the report's tag.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

if (!process.argv[2]) throw Error("Provide the generated Node package directory");
const wasm = createRequire(import.meta.url)(join(resolve(process.argv[2]), "safemesh_wasm.js"));

// Each class: a replica for author 1, and a write whose payload depends on `v`.
const CLASSES = {
  SafeMeshGCounterReplica: {
    make: () => new wasm.SafeMeshGCounterReplica(1n, 2),
    write: (r, v) => r.appendBump(1, BigInt(v)),
  },
  SafeMeshPnCounterReplica: {
    make: () => new wasm.SafeMeshPnCounterReplica(1n, 2),
    write: (r, v) => r.appendInc(1, BigInt(v)),
  },
  SafeMeshEnableWinsFlagReplica: {
    make: () => new wasm.SafeMeshEnableWinsFlagReplica(1n),
    write: (r, v) => r.appendEnable(BigInt(v)),
  },
  SafeMeshLwwMapReplica: {
    make: () => new wasm.SafeMeshLwwMapReplica(1n),
    write: (r, v) => r.appendSet(7n, 10n, 1n, BigInt(v)),
  },
  SafeMeshLwwRegisterReplica: {
    make: () => new wasm.SafeMeshLwwRegisterReplica(1n),
    write: (r, v) => r.appendSet(10n, 1n, BigInt(v)),
  },
  SafeMeshStringOrSetReplica: {
    make: () => new wasm.SafeMeshStringOrSetReplica(1n),
    write: (r, v) => r.appendAdd("x", BigInt(v)),
  },
};

// The offerer sends its log; the receiver merges it and reports back.
function exchange(offerer, receiver) {
  const admissions = receiver.mergeLogBytes(offerer.logBytes());
  const report = receiver.collisionReportBytes();
  const verdicts = report === undefined ? undefined : offerer.mergeCollisionReportBytes(report);
  return { admissions, verdicts };
}

function alarms(replica) {
  return replica.collisions().map(c => ({
    author: c.author(),
    sequence: c.sequence(),
    local: Array.from(c.local()),
    remote: Array.from(c.remote()),
  }));
}

function rejects(call, code, message) {
  assert.throws(call, error => {
    assert.equal(error.name, "SafeMeshError");
    assert.equal(error.code, code);
    assert.equal(error.message, message);
    return true;
  });
}

for (const [name, { make, write }] of Object.entries(CLASSES)) {
  const left = make();
  const right = make();
  const five = Array.from(write(left, 5));
  const nine = Array.from(write(right, 9));
  assert.deepEqual(left.collisions(), [], name);
  assert.equal(left.collisionReportBytes(), undefined, name);
  const leftLog = Array.from(left.logBytes());
  const rightLog = Array.from(right.logBytes());

  assert.deepEqual(exchange(right, left), { admissions: ["collision"], verdicts: ["recorded"] }, name);
  assert.deepEqual(alarms(left), [{ author: 1n, sequence: 1n, local: five, remote: nine }], name);
  assert.deepEqual(alarms(right), [{ author: 1n, sequence: 1n, local: nine, remote: five }], name);
  // The alarm never merges either payload; repeating the exchange is idempotent.
  assert.deepEqual(Array.from(left.logBytes()), leftLog, name);
  assert.deepEqual(Array.from(right.logBytes()), rightLog, name);
  assert.deepEqual(exchange(right, left), { admissions: ["collision"], verdicts: ["known"] }, name);
  assert.deepEqual(exchange(left, right), { admissions: ["collision"], verdicts: ["known"] }, name);

  // A replica that never held the ID records nothing.
  const stranger = make();
  assert.deepEqual(stranger.mergeCollisionReportBytes(left.collisionReportBytes()), ["unheld"], name);
  assert.deepEqual(stranger.collisions(), [], name);

  // Control: an equal payload is a duplicate and raises nothing.
  const same = make();
  const twin = make();
  write(same, 5);
  write(twin, 5);
  assert.deepEqual(exchange(twin, same), { admissions: ["duplicate"], verdicts: undefined }, name);
  assert.deepEqual(exchange(same, twin), { admissions: ["duplicate"], verdicts: undefined }, name);
  assert.deepEqual(same.collisions(), [], name);
  assert.deepEqual(Array.from(same.logBytes()), Array.from(twin.logBytes()), name);
  for (const replica of [left, right, stranger, same, twin]) replica.free();
}

// A malformed report is refused whole with its cause named.
const left = new wasm.SafeMeshGCounterReplica(1n, 2);
const right = new wasm.SafeMeshGCounterReplica(1n, 2);
left.appendBump(1, 5n);
right.appendBump(1, 9n);
assert.deepEqual(right.mergeLogBytes(left.logBytes()), ["collision"]);
const report = right.collisionReportBytes();
assert.equal(report[0], 0x05);
const flipped = report.slice();
flipped[flipped.length - 1] ^= 1;
const prefix = "failed to decode collision report: ";
rejects(() => left.mergeCollisionReportBytes(new Uint8Array()), 1, prefix + "unexpected end of wire input");
rejects(() => left.mergeCollisionReportBytes(report.slice(0, -1)), 1, prefix + "unexpected end of wire input");
rejects(() => left.mergeCollisionReportBytes(flipped), 1, prefix + "wire frame integrity check failed");
rejects(() => left.mergeCollisionReportBytes(Uint8Array.of(...report, 0)), 1,
  prefix + "unexpected trailing bytes after wire value");
rejects(() => left.mergeCollisionReportBytes(left.logBytes()), 1, prefix + "unexpected wire tag");
rejects(() => new wasm.SafeMeshPnCounterReplica(1n, 2).mergeCollisionReportBytes(report), 1,
  prefix + "wire delta schema does not match the expected type");
rejects(() => left.mergeCollisionReportBytes(report, undefined, 0), 1, prefix + "RecordLimitExceeded: 0");
rejects(() => left.mergeCollisionReportBytes(report, -1), 2,
  "maxCollectionElements must be a nonnegative integer at most 4294967295");
assert.deepEqual(left.collisions(), []);
assert.deepEqual(left.mergeCollisionReportBytes(report, undefined, 1), ["recorded"]);

// What a peer without the report surface sees: its record and log decoders
// refuse the report's tag before reading further, and nothing changes.
const peer = new wasm.SafeMeshGCounterReplica(1n, 2);
peer.appendBump(1, 5n);
const before = Array.from(peer.logBytes());
rejects(() => peer.mergeRecordBytes(report), 1, "failed to decode record: unexpected wire tag");
rejects(() => peer.mergeLogBytes(report), 1, "failed to decode event log: unexpected wire tag");
assert.deepEqual(Array.from(peer.logBytes()), before);
assert.deepEqual(Array.from(peer.state()), [0n, 5n]);

console.log("NODE_COLLISION_ALARM=true");
