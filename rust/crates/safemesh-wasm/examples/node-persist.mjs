#!/usr/bin/env node
// SafeMesh persist / restore / partition / reconcile walk for Node.
//
// Usage: node node-persist.mjs <package-dir> <log-dir> <step>
//   <package-dir> is a `wasm-pack build --target nodejs` output directory.
//   <log-dir>     is where the four log files live.
//   <step>        is one of: persist, restore, partition, reconcile.
//
// Each step is a separate process. State survives only through the log files.

import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const [packageDir, logDir, step] = process.argv.slice(2);
if (!packageDir || !logDir || !step) {
  console.error("usage: node node-persist.mjs <package-dir> <log-dir> <step>");
  process.exit(64);
}

const require = createRequire(import.meta.url);
const { SafeMeshGCounterReplica, SafeMeshStringOrSetReplica } = require(
  join(resolve(packageDir), "safemesh_wasm.js"),
);

// Two replicas, ids 1 and 2. A G-Counter replica may only bump the counter
// slot whose index equals its own replica id, so the counter width must be
// larger than the largest replica id in use: width 3 here, slot 0 unused.
const WIDTH = 3;
const SIDES = { left: 1n, right: 2n };

const logPath = (side, kind) => join(logDir, `${side}-${kind}.log`);

function fresh(side) {
  return {
    counter: new SafeMeshGCounterReplica(SIDES[side], WIDTH),
    set: new SafeMeshStringOrSetReplica(SIDES[side]),
  };
}

function save(side, replica) {
  mkdirSync(logDir, { recursive: true });
  for (const kind of ["counter", "set"]) {
    const bytes = replica[kind].logBytes();
    writeFileSync(logPath(side, kind), bytes);
    console.log(`  wrote ${logPath(side, kind)} (${bytes.length} bytes)`);
  }
}

function load(side) {
  const replica = fresh(side);
  for (const kind of ["counter", "set"]) {
    const path = logPath(side, kind);
    if (!existsSync(path)) {
      console.error(`RESTORE FAILED file=${path} error=missing (run the persist step first)`);
      process.exit(2);
    }
    try {
      replica[kind].mergeLogBytes(readFileSync(path));
    } catch (error) {
      console.error(`RESTORE FAILED file=${path} error=${error.name}: ${error.message}`);
      process.exit(2);
    }
  }
  return replica;
}

function show(label, replica) {
  const versions = `v1=${replica.set.versionFor(1n)} v2=${replica.set.versionFor(2n)}`;
  console.log(
    `  ${label}: counter=${replica.counter.value()} set=${JSON.stringify(replica.set.elements())} (${versions})`,
  );
}

function same(a, b) {
  return (
    a.counter.sameStateAs(b.counter) &&
    JSON.stringify(a.set.elements()) === JSON.stringify(b.set.elements())
  );
}

function exchange(a, b) {
  for (const kind of ["counter", "set"]) {
    a[kind].mergeLogBytes(b[kind].logBytes());
    b[kind].mergeLogBytes(a[kind].logBytes());
  }
}

switch (step) {
  case "persist": {
    console.log("persist: two fresh replicas, one edit each, full exchange, then write logs");
    const left = fresh("left");
    const right = fresh("right");
    // appendBump(slot, tally): tally is the slot's new running total, not an increment.
    left.counter.appendBump(1, 5n);
    left.set.appendAdd("vaccine", 11n);
    right.counter.appendBump(2, 7n);
    right.set.appendAdd("insulin", 21n);
    exchange(left, right);
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    process.exit(same(left, right) ? 0 : 1);
  }
  case "restore": {
    console.log("restore: fresh process, replicas rebuilt from the log files alone");
    const left = load("left");
    const right = load("right");
    show("left ", left);
    show("right", right);
    const ok = same(left, right) && left.counter.value() > 0n;
    console.log(`RESTORED=${ok}`);
    process.exit(ok ? 0 : 1);
  }
  case "partition": {
    console.log("partition: both sides edit offline; nothing is exchanged");
    const left = load("left");
    const right = load("right");
    left.counter.appendBump(1, 8n); // slot 1 total 5 -> 8
    left.set.appendRemoveObserved("vaccine"); // removes the token left has seen (11)
    right.counter.appendBump(2, 11n); // slot 2 total 7 -> 11
    right.set.appendAdd("vaccine", 22n); // concurrent re-add with a fresh token
    right.set.appendAdd("gauze", 23n);
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    const diverged = !same(left, right);
    console.log(`DIVERGED=${diverged}`);
    process.exit(diverged ? 0 : 1);
  }
  case "reconcile": {
    console.log("reconcile: each side merges the other's log file, then both are written back");
    const left = load("left");
    const right = load("right");
    for (const kind of ["counter", "set"]) {
      left[kind].mergeLogBytes(readFileSync(logPath("right", kind)));
      right[kind].mergeLogBytes(readFileSync(logPath("left", kind)));
    }
    show("left ", left);
    show("right", right);
    save("left", left);
    save("right", right);
    const converged = same(left, right);
    console.log(`CONVERGED=${converged}`);
    process.exit(converged ? 0 : 1);
  }
  default:
    console.error(`unknown step ${JSON.stringify(step)}; expected persist, restore, partition or reconcile`);
    process.exit(64);
}
