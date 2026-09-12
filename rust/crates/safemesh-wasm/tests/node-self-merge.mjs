// The Monkey's own self-merge probe. Written independently on 2026-09-11 from the
// outside review's description, because the reviewer's script is on a Windows path
// this box cannot reach.
//
//   node safemesh-self-merge-probe.mjs /absolute/path/to/pkg-node
//
// Source: /home/monkey/safemesh-repros/safemesh-self-merge-probe.mjs
// Copied byte-identically before inversion; demands self-merge is a usable no-op.
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {join, resolve} from 'node:path';

if (!process.argv[2]) throw Error('Provide the generated Node package directory');
const api = createRequire(import.meta.url)(join(resolve(process.argv[2]), 'safemesh_wasm.js'));
const {SafeMeshOrSet: OrSet} = api;
const bytes = array => Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString('hex');
// This state-only OR-Set has no event log. Include observed tokens as well as
// elements/tombstones so a hidden change to live add tokens cannot pass.
const snapshot = set => ({
  elements: bytes(set.elements()),
  tombstones: bytes(set.tombstones()),
  tokens: [42n, 99n, 123n].map(element => bytes(set.observedTokens(element))),
});
if (!OrSet) throw Error('SafeMeshOrSet is not exported by this package');

const attempt = (label, fn) => {
  try {
    const value = fn();
    return {label, threw: false, value};
  } catch (error) {
    return {label, threw: true, message: String(error && error.message || error)};
  }
};

// Control first. A distinct set merged twice must work, and must stay usable.
const control = (() => {
  const a = new OrSet();
  const b = new OrSet();
  a.add(42n, 7n);
  b.add(99n, 8n);
  a.add(123n, 10n);
  b.applyRemove(new BigUint64Array([10n, 11n]));
  const first = attempt('control merge 1', () => a.merge(b));
  const second = attempt('control merge 2', () => a.merge(b));
  const read = attempt('control read', () => {
    assert.deepEqual(Array.from(a.elements()), [42n, 99n]);
    assert.deepEqual(Array.from(a.tombstones()), [10n, 11n]);
    assert.deepEqual(Array.from(a.observedTokens(123n)), [10n]);
    return snapshot(a);
  });
  const write = attempt('control write', () => a.add(7n, 9n));
  const free = attempt('control free', () => { a.free(); b.free(); });
  return {first, second, read, write, free};
})();

// The reviewed case. Merge a set with itself, then try to keep using the handle.
const subject = (() => {
  const set = new OrSet();
  set.add(42n, 7n);
  set.add(99n, 8n);
  set.applyRemove(new BigUint64Array([8n, 10n]));
  const before = attempt('subject read before', () => snapshot(set));
  const merge = attempt('self merge', () => set.merge(set));
  const read = attempt('subject read after (byte comparison)', () => {
    const after = snapshot(set);
    for (const key of ['elements', 'tombstones']) {
      assert.equal(Buffer.compare(Buffer.from(after[key], 'hex'), Buffer.from(before.value[key], 'hex')), 0, key);
    }
    after.tokens.forEach((token, index) => {
      assert.equal(Buffer.compare(Buffer.from(token, 'hex'), Buffer.from(before.value.tokens[index], 'hex')), 0, `tokens ${index}`);
    });
    return after;
  });
  const write = attempt('subject write after', () => {
    set.add(43n, 9n);
    assert.equal(set.contains(43n), true);
  });
  const free = attempt('subject free after', () => set.free());
  return {before, merge, read, write, free};
})();

// A newly created set must still work, proving the damage is handle-local.
const survivor = (() => {
  const fresh = new OrSet();
  const write = attempt('survivor write', () => fresh.add(1n, 1n));
  const read = attempt('survivor read', () => Array.from(fresh.elements()).map(String));
  const free = attempt('survivor free', () => fresh.free());
  return {write, read, free};
})();

for (const group of [control, subject, survivor]) {
  for (const step of Object.values(group)) {
    console.log(`${step.threw ? 'THREW ' : 'ok    '} ${step.label}${step.threw ? `: ${step.message}` : ''}`);
  }
}

const claims = [
  ['control: a distinct set merges twice and stays usable',
    !control.first.threw && !control.second.threw && !control.read.threw && !control.write.threw && !control.free.threw],
  ['subject: the set is readable before the self merge', !subject.before.threw],
  ['self merge succeeds', !subject.merge.threw],
  ['state remains byte-identical and readable after self merge', !subject.read.threw],
  ['handle is writable after self merge', !subject.write.threw],
  ['handle is freeable after self merge', !subject.free.threw],
  ['a freshly created set is unaffected',
    !survivor.write.threw && !survivor.read.threw && !survivor.free.threw],
];

console.log('CONTROL_STATE=' + JSON.stringify(control.read.value));
console.log('SELF_STATE_BEFORE=' + JSON.stringify(subject.before.value));
console.log('SELF_STATE_AFTER=' + JSON.stringify(subject.read.value));

// Shared/shared aliasing must also remain safe.
const counter = new api.SafeMeshGCounterReplica(1n, 3);
assert.equal(counter.sameStateAs(counter), true);
counter.state();
counter.free();

let held = 0;
console.log('');
for (const [name, ok] of claims) {
  console.log(`${ok ? 'HOLDS   ' : 'NOT HELD'} ${name}`);
  if (ok) held++;
}
console.log(`\n${held} of ${claims.length} claims hold. Exit 0 means self-merge regression checks pass.`);
process.exitCode = held === claims.length ? 0 : 1;
