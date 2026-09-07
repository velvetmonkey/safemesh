#!/usr/bin/env node

import { createRequire } from "node:module";
import { resolve, join } from "node:path";

const packageDir = resolve(process.argv[2] ?? process.env.SAFEMESH_WASM_PKG ?? "pkg-node");
const require = createRequire(import.meta.url);
const {
  SafeMeshGCounterReplica,
  SafeMeshEnableWinsFlagReplica,
} = require(join(packageDir, "safemesh_wasm.js"));

const left = new SafeMeshGCounterReplica(1n, 3);
const right = new SafeMeshGCounterReplica(2n, 3);

const leftRecord = left.appendBump(1, 5n);
const rightRecord = right.appendBump(2, 7n);

left.mergeRecordBytes(rightRecord);
right.mergeRecordBytes(leftRecord);
left.mergeLogBytes(right.logBytes());
right.mergeLogBytes(left.logBytes());

const flagLeft = new SafeMeshEnableWinsFlagReplica(1n);
const flagRight = new SafeMeshEnableWinsFlagReplica(2n);
flagRight.mergeRecordBytes(flagLeft.appendEnable(44n));
const removeObserved = flagRight.appendDisableObserved();
flagLeft.appendEnable(45n);
flagLeft.mergeRecordBytes(removeObserved);
flagRight.mergeLogBytes(flagLeft.logBytes());

const counterConverged = left.value() === 12n && right.value() === 12n;
const flagConverged = flagLeft.value() === true && flagRight.value() === true;

if (!counterConverged || !flagConverged) {
  console.error(
    `CONVERGED=false counter_left=${left.value()} counter_right=${right.value()} flag_left=${flagLeft.value()} flag_right=${flagRight.value()}`,
  );
  process.exit(1);
}

console.log(
  `CONVERGED=true js_wasm_counter=${left.value()} flag=${flagLeft.value()} versions=${left.versionFor(1n)}/${right.versionFor(2n)}`,
);
