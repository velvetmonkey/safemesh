import { strict as assert } from "node:assert";
import { mkdirSync, readFileSync, writeFileSync, unlinkSync, rmdirSync } from "node:fs";
import { SafeMeshGCounterReplica as Counter, SafeMeshStringOrSetReplica as Members } from "./pkg/safemesh_wasm";

// tsc checks these imports against the generated package's .d.ts, with no `any` shim.
const step = process.argv[2];
const root = ".gold-typescript";
const counter = new Counter(0n, 2);
const members = new Members(0n);
function memberState(replica: Members) {
  const entries = replica.addEntries();
  try {
    return {
      adds: entries.map(entry => `${entry.element()}:${entry.token()}`).sort(),
      tombstones: Array.from(replica.tombstones(), String).sort(),
    };
  } finally {
    entries.forEach(entry => entry.free());
  }
}
function accepted(verdicts: ("accepted" | "duplicate" | "collision")[]) {
  assert(!verdicts.includes("collision"), `record collision: ${verdicts}`);
}
try {
  if (step === "persist") {
    mkdirSync(root); // refuse to overwrite an existing exercise
    counter.appendBump(0, 3n);
    members.appendAdd("compass", 10n); // application-owned unique token
    writeFileSync(`${root}/counter.log`, counter.logBytes());
    writeFileSync(`${root}/members.log`, members.logBytes());
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("saved counter=3 members=[compass]");
  } else {
    assert.equal(step, "restart", "pass persist or restart");
    // Single-writer exercise: restore the complete logs before allocating new edits.
    accepted(counter.mergeLogBytes(readFileSync(`${root}/counter.log`)));
    accepted(members.mergeLogBytes(readFileSync(`${root}/members.log`)));
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("restored counter=3 members=[compass]");
    counter.appendBump(0, 4n); // new cumulative tally, not +4
    members.appendAdd("map", 11n); // different from every previous add token
    const peerCounter = new Counter(1n, 2);
    const peerMembers = new Members(1n);
    try {
      peerCounter.appendBump(1, 2n);
      peerMembers.appendAdd("rope", 20n);
      // Your transport delivers these Uint8Arrays; this exercise hands them across.
      accepted(peerCounter.mergeLogBytes(counter.logBytes()));
      accepted(peerMembers.mergeLogBytes(members.logBytes()));
      accepted(counter.mergeLogBytes(peerCounter.logBytes()));
      accepted(members.mergeLogBytes(peerMembers.logBytes()));
      assert.equal(counter.value(), 6n);
      assert.deepEqual(members.elements(), ["compass", "map", "rope"]);
      assert(counter.sameStateAs(peerCounter));
      // Logs retain arrival order. Compare all records by duplicate admission,
      // and compare carrier metadata separately, rather than comparing byte order.
      assert.deepEqual(counter.mergeLogBytes(peerCounter.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(members.mergeLogBytes(peerMembers.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(memberState(members), memberState(peerMembers));
      assert.deepEqual(memberState(members), { adds: ["compass:10", "map:11", "rope:20"], tombstones: [] });
      for (const replica of [counter, members, peerCounter, peerMembers]) {
        assert.equal(replica.versionFor(0n), 2n);
        assert.equal(replica.versionFor(1n), 1n);
      }
      console.log("synced counter=6 members=[compass,map,rope] records=3+3");
      const before = counter.logBytes();
      assert.throws(() => counter.mergeRecordBytes(new Uint8Array()), (error: unknown) => {
        assert(error instanceof Error);
        assert.equal(error.name, "SafeMeshError");
        assert.equal(error.message, "failed to decode record");
        console.log(`malformed record: ${error.name}: ${error.message}`);
        return true;
      });
      assert.deepEqual(counter.logBytes(), before);
    } finally {
      peerCounter.free();
      peerMembers.free();
    }
    unlinkSync(`${root}/counter.log`);
    unlinkSync(`${root}/members.log`);
    rmdirSync(root);
    console.log("cleaned exercise stores");
  }
} finally {
  counter.free();
  members.free();
}
