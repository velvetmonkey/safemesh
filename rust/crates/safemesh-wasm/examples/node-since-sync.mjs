// Incremental sync: send a peer only the records its version says it lacks.
//
//   node examples/node-since-sync.mjs pkg-node
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";

if (!process.argv[2]) throw Error("Provide the generated Node package directory");
const { SafeMeshStringOrSetReplica } = createRequire(import.meta.url)(
  join(resolve(process.argv[2]), "safemesh_wasm.js"),
);

const a = new SafeMeshStringOrSetReplica(1n);
const b = new SafeMeshStringOrSetReplica(2n);
a.appendAdd("milk", 10n);
b.mergeLogBytes(a.logBytes());
a.appendAdd("eggs", 11n);
a.appendAdd("bread", 12n);

// B sends its version; A answers with only what B is missing.
const peerVersion = b.versionVector(); // [author, versionFor(author), ...]
const batch = a.sinceLogBytes(peerVersion);
const verdicts = b.mergeLogBytes(batch);
assert.deepEqual(verdicts, ["accepted", "accepted"]);
assert.deepEqual(b.recordIds(), a.recordIds()); // [author, sequence, ...]
assert.deepEqual(b.logBytes(), a.logBytes());

console.log(
  `SINCE_SYNC=true sent=${verdicts.length} of ${a.recordIds().length / 2} version=${Array.from(peerVersion)} elements=${b.elements()}`,
);
a.free();
b.free();
