// Record listing and since batches on every WASM replica class with logBytes.
//
//   node node-since-batch.mjs /absolute/path/to/pkg-node
//
// A holds sequences 1..10 from two authors, B holds 1..6 of each. A's batch for
// B's version must carry exactly the 8 missing records and leave B's log
// byte-identical to A's. A batch for a version ahead of A carries no record, a
// malformed version throws naming `peerVersion`, and the batch honours the
// budgets of the receiver's mergeLogBytes.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

if (!process.argv[2]) throw Error("Provide the generated Node package directory");
const wasm = createRequire(import.meta.url)(join(resolve(process.argv[2]), "safemesh_wasm.js"));

// Each class: a constructor, a record for (writer, author, i), and state reads.
const CLASSES = {
  SafeMeshGCounterReplica: {
    make: id => new wasm.SafeMeshGCounterReplica(id, 2),
    append: (w, author, i) => w.appendBump(Number(author), BigInt(i)),
    read: r => ({ value: r.value(), state: Array.from(r.state()) }),
  },
  SafeMeshPnCounterReplica: {
    make: id => new wasm.SafeMeshPnCounterReplica(id, 2),
    append: (w, author, i) =>
      i % 3 ? w.appendInc(Number(author), BigInt(i)) : w.appendDec(Number(author), BigInt(i)),
    read: r => ({ value: r.value(), state: Array.from(r.state()) }),
  },
  SafeMeshEnableWinsFlagReplica: {
    make: id => new wasm.SafeMeshEnableWinsFlagReplica(id),
    append: (w, author, i) =>
      i % 2 ? w.appendEnable(author * 100n + BigInt(i)) : w.appendDisableObserved(),
    read: r => ({
      value: r.value(),
      enabled: Array.from(r.enabledTokens()),
      tombstones: Array.from(r.tombstoneTokens()),
    }),
  },
  SafeMeshLwwMapReplica: {
    make: id => new wasm.SafeMeshLwwMapReplica(id),
    append: (w, author, i) =>
      i % 4
        ? w.appendSet(BigInt(i % 3), BigInt(i), author, author * 100n + BigInt(i))
        : w.appendRemove(BigInt(i % 3), BigInt(i), author),
    read: r => ({
      visible: Array.from(r.visibleKeys(), key => [key, r.valueOr(key, 0n)]),
      entries: Array.from(r.entryKeys()),
      removals: Array.from(r.removalKeys()),
    }),
  },
  SafeMeshLwwRegisterReplica: {
    make: id => new wasm.SafeMeshLwwRegisterReplica(id),
    append: (w, author, i) => w.appendSet(BigInt(i), author, author * 100n + BigInt(i)),
    read: r => ({
      value: r.valueOr(0n),
      timestamp: r.timestampOr(0n),
      writer: r.writerReplicaOr(0n),
    }),
  },
  SafeMeshStringOrSetReplica: {
    make: id => new wasm.SafeMeshStringOrSetReplica(id),
    append: (w, author, i) =>
      i % 5
        ? w.appendAdd(`item-${author}-${i % 4}`, author * 1000n + BigInt(i))
        : w.appendRemoveObserved(`item-${author}-${(i - 1) % 4}`),
    read: r => ({ elements: r.elements(), tombstones: Array.from(r.tombstones()) }),
  },
};

const hex = bytes => Buffer.from(bytes).toString("hex");
const pairs = ids => {
  const out = [];
  for (let i = 0; i < ids.length; i += 2) out.push(`${ids[i]}:${ids[i + 1]}`);
  return out;
};
const refusal = fn => {
  try {
    fn();
  } catch (error) {
    assert.equal(error.name, "SafeMeshError", `expected SafeMeshError, got ${error}`);
    return { code: error.code, message: error.message };
  }
  return null;
};
const AUTHORS = [0n, 1n];

for (const [name, cls] of Object.entries(CLASSES)) {
  const writers = AUTHORS.map(author => cls.make(author));
  const a = cls.make(8n);
  const b = cls.make(9n);
  for (let i = 1; i <= 10; i++) {
    writers.forEach((w, index) => cls.append(w, AUTHORS[index], i));
    if (i === 6) {
      for (const w of writers) {
        a.mergeLogBytes(w.logBytes());
        b.mergeLogBytes(w.logBytes());
      }
    }
  }
  for (const w of writers) a.mergeLogBytes(w.logBytes());

  // Listing: every ID, in log order, and B's version in the versionFor shape.
  const aIds = pairs(a.recordIds());
  const bIds = pairs(b.recordIds());
  assert.equal(aIds.length, 20, `${name}: A lists 20 records`);
  assert.equal(bIds.length, 12, `${name}: B lists 12 records`);
  const missing = aIds.filter(id => !bIds.includes(id));
  assert.deepEqual(
    missing,
    AUTHORS.flatMap(author => [7, 8, 9, 10].map(sequence => `${author}:${sequence}`)),
  );
  const peer = b.versionVector();
  assert(peer instanceof BigUint64Array);
  assert.deepEqual(Array.from(peer), AUTHORS.flatMap(author => [author, b.versionFor(author)]));
  assert.deepEqual(Array.from(peer), [0n, 6n, 1n, 6n]);

  // The batch: exactly the missing records, and B converges to A byte for byte.
  const batch = a.sinceLogBytes(peer);
  const verdicts = b.mergeLogBytes(batch);
  assert.equal(
    verdicts.length,
    missing.length,
    `${name}: batch holds ${verdicts.length} records; B was missing ${missing.length}`,
  );
  assert.deepEqual(verdicts, missing.map(() => "accepted"));
  assert.equal(hex(b.logBytes()), hex(a.logBytes()), `${name}: B's log bytes equal A's`);
  assert.deepEqual(pairs(b.recordIds()), aIds);
  assert.deepEqual(cls.read(b), cls.read(a));
  assert.deepEqual(b.mergeLogBytes(a.sinceLogBytes(b.versionVector())), []);

  // Budgets: the since batch refuses what B's mergeLogBytes would refuse.
  const probe = cls.make(7n);
  for (const budgets of [[undefined, 7], [undefined, 0], [0, undefined], [1, undefined]]) {
    const sent = refusal(() => a.sinceLogBytes(peer, ...budgets));
    const merged = refusal(() => probe.mergeLogBytes(batch, ...budgets));
    assert.deepEqual(sent, merged, `${name}: budgets ${budgets} match mergeLogBytes`);
  }
  assert.deepEqual(refusal(() => a.sinceLogBytes(peer, undefined, 7)), {
    code: 1,
    message: "failed to decode event log: RecordLimitExceeded: 7",
  });
  assert.deepEqual(hex(a.sinceLogBytes(peer, undefined, 8)), hex(batch));
  assert.equal(refusal(() => a.sinceLogBytes(peer, undefined, -1)).code, 2);
  assert.equal(refusal(() => a.sinceLogBytes(peer, 1.5)).code, 2);

  // A version ahead of A, including an author A has never seen, selects nothing.
  const ahead = new BigUint64Array([0n, 11n, 1n, 11n, 5n, 3n]);
  const empty = a.sinceLogBytes(ahead);
  assert.deepEqual(probe.mergeLogBytes(empty), []);
  assert.equal(hex(empty), hex(cls.make(6n).logBytes()), `${name}: ahead batch is the empty log`);

  // Malformed versions are refused by name, before anything is selected.
  const before = hex(a.logBytes());
  const malformed = [
    [new BigUint64Array([0n]), "peerVersion must hold (author, prefix) pairs; got 1 values"],
    [new BigUint64Array([0n, 1n, 0n, 2n]), "peerVersion repeats author 0"],
    [new BigUint64Array([0n, 0n]), "peerVersion: replica 0 has a noncanonical zero prefix"],
    [[0n, 6n], "peerVersion must be a BigUint64Array of (author, prefix) pairs"],
    [
      new BigUint64Array(Array.from({ length: 4097 }, (_, i) => [BigInt(i), 1n]).flat()),
      "peerVersion: peer version exceeds author limit 4096",
    ],
  ];
  for (const [version, message] of malformed) {
    assert.deepEqual(refusal(() => a.sinceLogBytes(version)), { code: 2, message });
  }
  assert.equal(hex(a.logBytes()), before);

  console.log(
    `${name}: missing=${missing.length} batch_records=${verdicts.length} ` +
      `log_bytes_equal=${hex(b.logBytes()) === hex(a.logBytes())} ` +
      `ahead_records=${probe.mergeLogBytes(empty).length} malformed_refused=${malformed.length}`,
  );
  for (const handle of [...writers, a, b, probe]) handle.free();
}
console.log(`SINCE_BATCH=true classes=${Object.keys(CLASSES).length}`);
