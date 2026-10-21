import { strict as assert } from "node:assert";
import { mkdirSync, unlinkSync, rmdirSync } from "node:fs";
import { SafeMeshManagedGCounter, SafeMeshManagedStringOrSet,
  SafeMeshGCounterReplica as CounterPeer, SafeMeshStringOrSetReplica as MembersPeer } from "./pkg/safemesh_wasm";

// tsc checks the generated package and the typed example Store, with no `any` shim.
import("../../../rust/crates/safemesh-wasm/examples/node-filesystem-store.mjs").then(({ FileSystemStore }) => {
const step = process.argv[2];
const root = ".gold-typescript";
assert(["persist", "restart"].includes(step), "pass persist or restart");
let counter: SafeMeshManagedGCounter | undefined;
let members: SafeMeshManagedStringOrSet | undefined;
const counterStore = new FileSystemStore(`${root}/counter`, `${root}/counter.anchor`);
const membersStore = new FileSystemStore(`${root}/members`, `${root}/members.anchor`);
function accepted(verdicts: ("accepted" | "duplicate" | "collision")[]) {
  assert(!verdicts.includes("collision"), `record collision: ${verdicts}`);
}
function close() {
  counter?.close();
  members?.close();
  counter?.free();
  members?.free();
  counter = undefined;
  members = undefined;
}
function open(mode: "fresh" | "restart") {
  const options = mode === "fresh"
    ? { mode, writer: 0n, writers: 2 } as const
    : { mode, writer: 0n } as const;
  counter = SafeMeshManagedGCounter.open(counterStore, options);
  members = SafeMeshManagedStringOrSet.open(membersStore, options);
}
try {
  if (step === "persist") {
    mkdirSync(root); // Refuse to overwrite an existing exercise.
    open("fresh");
    counter!.appendBump(3n);
    members!.appendAdd("compass"); // Rust allocates token 2.
    assert.equal(counter!.value(), 3n);
    assert.deepEqual(members!.elements(), ["compass"]);
    close();
    console.log("saved counter=3 members=[compass]");
  } else {
    assert.equal(step, "restart", "pass persist or restart");
    open("restart"); // Writer count comes only from committed metadata.
    assert(counter && members);
    assert.equal(counter!.value(), 3n);
    assert.deepEqual(members!.elements(), ["compass"]);
    console.log("restored counter=3 members=[compass]");
    counter!.appendBump(4n); // New cumulative tally, not +4.
    members!.appendAdd("map"); // Restored allocation produces token 4.
    let peerCounter: CounterPeer | undefined;
    let peerMembers: MembersPeer | undefined;
    try {
      peerCounter = new CounterPeer(1n, 2);
      peerMembers = MembersPeer.createAllocated(2n, 1n);
      peerCounter.appendBump(1, 2n);
      peerMembers.appendAllocatedAdd("rope"); // Rust allocates token 3.
      // The transport hands peer bytes across; the peer stays in memory.
      accepted(peerCounter.mergeLogBytes(counter!.peerLogBytes()));
      accepted(peerMembers.mergeLogBytes(members!.peerLogBytes()));
      accepted(counter!.mergeLogBytes(peerCounter.logBytes()));
      accepted(members!.mergeLogBytes(peerMembers.logBytes()));
      assert.equal(counter.value(), 6n);
      assert.deepEqual(members.elements(), ["compass", "map", "rope"]);
      assert.equal(counter.value(), peerCounter.value());
      assert.deepEqual(members.elements(), peerMembers.elements());
      assert.deepEqual(counter.mergeLogBytes(peerCounter.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(members.mergeLogBytes(peerMembers.logBytes()), ["duplicate", "duplicate", "duplicate"]);
      assert.deepEqual(Array.from(members.versionVector(), String), Array.from(peerMembers.versionVector(), String));
      assert.deepEqual(members.elements(), ["compass", "map", "rope"]);
      for (const replica of [counter, peerCounter]) {
        assert.equal(replica.versionFor(0n), 2n);
        assert.equal(replica.versionFor(1n), 1n);
      }
      console.log("synced counter=6 members=[compass,map,rope] records=3+3");
      const before = counter.peerLogBytes();
      assert.throws(() => counter!.mergeRecordBytes(new Uint8Array()), (error: unknown) => {
        assert(error instanceof Error);
        assert.equal(error.name, "SafeMeshError");
        assert.equal(error.message, "failed to decode record: unexpected end of wire input");
        console.log(`malformed record: ${error.name}: ${error.message}`);
        return true;
      });
      assert.deepEqual(counter.peerLogBytes(), before);
    } finally {
      peerCounter?.free();
      peerMembers?.free();
    }
    // Release both leases, then read the latest committed state again.
    const latestVersion = Array.from(members.versionVector());
    close();
    open("restart");
    assert.equal(counter!.value(), 6n);
    assert.deepEqual(members!.elements(), ["compass", "map", "rope"]);
    assert.deepEqual(Array.from(members!.versionVector()), latestVersion);
    console.log("saved latest managed state members=[compass,map,rope] local-sequence=2");
    close();
    for (const name of ["counter", "members"]) {
      unlinkSync(`${root}/${name}/committed.bin`);
      rmdirSync(`${root}/${name}`);
      unlinkSync(`${root}/${name}.anchor`);
    }
    rmdirSync(root);
    console.log("cleaned exercise stores");
  }
} finally {
  if (counter || members) close();
}
});
