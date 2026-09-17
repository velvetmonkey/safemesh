import { strict as assert } from "node:assert";
import { mkdirSync, readFileSync, writeFileSync, unlinkSync, rmdirSync } from "node:fs";
import { SafeMeshGCounterReplica as Counter, SafeMeshStringOrSetReplica as Members } from "./pkg/safemesh_wasm";

// tsc checks these imports against the generated package's .d.ts, with no `any` shim.
const step = process.argv[2];
const root = ".gold-typescript";
assert(["persist", "restart"].includes(step), "pass persist or restart");
let counter: Counter | undefined;
let members: Members | undefined;
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
  counter = new Counter(0n, 2);
  if (step === "persist") {
    mkdirSync(root); // refuse to overwrite an existing exercise
    members = Members.createAllocated(2n, 0n);
    counter.appendBump(0, 3n);
    members.appendAllocatedAdd("compass"); // Rust allocates token 2
    writeFileSync(`${root}/counter.log`, counter.logBytes());
    writeFileSync(`${root}/members.identity`, members.exportIdentity());
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("saved counter=3 members=[compass]");
  } else {
    assert.equal(step, "restart", "pass persist or restart");
    // No fallback to a new identity if the saved file is missing or invalid.
    members = Members.importIdentity(readFileSync(`${root}/members.identity`));
    accepted(counter.mergeLogBytes(readFileSync(`${root}/counter.log`)));
    assert.equal(counter.value(), 3n);
    assert.deepEqual(members.elements(), ["compass"]);
    console.log("restored counter=3 members=[compass]");
    counter.appendBump(0, 4n); // new cumulative tally, not +4
    members.appendAllocatedAdd("map"); // restored allocation produces token 4
    writeFileSync(`${root}/counter.log`, counter.logBytes());
    writeFileSync(`${root}/members.identity`, members.exportIdentity());
    let peerCounter: Counter | undefined;
    let peerMembers: Members | undefined;
    try {
      peerCounter = new Counter(1n, 2);
      peerMembers = Members.createAllocated(2n, 1n);
      peerCounter.appendBump(1, 2n);
      peerMembers.appendAllocatedAdd("rope"); // Rust allocates token 3
      writeFileSync(`${root}/peer.identity`, peerMembers.exportIdentity());
      // Your transport delivers these Uint8Arrays; this exercise hands them across.
      accepted(peerCounter.mergeLogBytes(counter.logBytes()));
      accepted(peerMembers.mergeLogBytes(members.logBytes()));
      writeFileSync(`${root}/peer.identity`, peerMembers.exportIdentity());
      accepted(counter.mergeLogBytes(peerCounter.logBytes()));
      accepted(members.mergeLogBytes(peerMembers.logBytes()));
      writeFileSync(`${root}/counter.log`, counter.logBytes());
      writeFileSync(`${root}/members.identity`, members.exportIdentity());
      assert.equal(counter.value(), 6n);
      assert.deepEqual(members.elements(), ["compass", "map", "rope"]);
      assert(counter.sameStateAs(peerCounter));
      // Logs retain arrival order. Compare all records by duplicate admission,
      // and compare carrier metadata separately, rather than comparing byte order.
      assert.deepEqual(counter.mergeLogBytes(peerCounter.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(members.mergeLogBytes(peerMembers.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(memberState(members), memberState(peerMembers));
      assert.deepEqual(memberState(members), { adds: ["compass:2", "map:4", "rope:3"], tombstones: [] });
      for (const replica of [counter, members, peerCounter, peerMembers]) {
        assert.equal(replica.versionFor(0n), 2n);
        assert.equal(replica.versionFor(1n), 1n);
      }
      console.log("synced counter=6 members=[compass,map,rope] records=3+3");
      const before = counter.logBytes();
      const malformedTarget = counter;
      assert.throws(() => malformedTarget.mergeRecordBytes(new Uint8Array()), (error: unknown) => {
        assert(error instanceof Error);
        assert.equal(error.name, "SafeMeshError");
        assert.equal(error.message, "failed to decode record");
        console.log(`malformed record: ${error.name}: ${error.message}`);
        return true;
      });
      assert.deepEqual(counter.logBytes(), before);
    } finally {
      peerCounter?.free();
      peerMembers?.free();
    }
    // Release the claim, then verify the latest file, including the post-restart add.
    const latest = memberState(members);
    members.free();
    members = undefined;
    members = Members.importIdentity(readFileSync(`${root}/members.identity`));
    assert.deepEqual(memberState(members), latest);
    assert.equal(members.versionFor(0n), 2n);
    assert.equal(members.versionFor(1n), 1n);
    console.log("saved latest identity members=[compass,map,rope] local-sequence=2");
    counter.free();
    counter = undefined;
    members.free();
    members = undefined;
    unlinkSync(`${root}/counter.log`);
    unlinkSync(`${root}/members.identity`);
    unlinkSync(`${root}/peer.identity`);
    rmdirSync(root);
    console.log("cleaned exercise stores");
  }
} finally {
  counter?.free();
  members?.free();
}
