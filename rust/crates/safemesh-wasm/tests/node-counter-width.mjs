import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { resolve, join } from 'node:path';
const packageFile = join(resolve(process.argv[2]), 'safemesh_wasm.js');
const failures = [];
for (const name of ['SafeMeshGCounter', 'SafeMeshGCounterReplica', 'SafeMeshPnCounterReplica']) {
  const result = spawnSync(process.execPath, ['-e', `
    const assert = require('node:assert/strict');
    const wasm = require(process.argv[1]);
    const name = process.argv[2];
    const make = n => name === 'SafeMeshGCounter' ? new wasm[name](n) : new wasm[name](0n, n);
    for (const width of [4097, 4097]) {
      assert.throws(() => make(width), e => e.name === 'SafeMeshError' && e.code === 2 && /4097/.test(e.message) && /4096/.test(e.message));
    }
    for (const width of [0, 4096]) {
      const counter = make(width);
      assert.equal(counter.value(), 0n);
      counter.free();
    }
  `, packageFile, name], { encoding: 'utf8' });
  console.log(`${name}: ${result.status === 0 ? 'PASS' : 'FAIL'}`);
  if (result.status !== 0) failures.push(`${name}: ${result.stderr}`);
}
assert.deepEqual(failures, []);
console.log('counter width: named refusal, repeat refusal, legal retry passed');
