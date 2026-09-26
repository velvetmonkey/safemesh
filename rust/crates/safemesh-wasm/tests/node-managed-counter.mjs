// Planted persistence cases: run --low-level on main before managed implementation.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { mkdtempSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { FileSystemStore } from '../examples/node-filesystem-store.mjs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const pkg = resolve(process.argv[2]);
const wasm = createRequire(import.meta.url)(join(pkg, 'safemesh_wasm.js'));
const root = mkdtempSync(join(process.env.TMPDIR, 'managed-counter-'));
if (process.argv.includes('--low-level')) {
  // (a) The low-level counter acknowledges and exposes an edit before saving.
  const live = new wasm.SafeMeshGCounterReplica(0n, 2);
  const acknowledged = live.appendBump(0, 1n);
  assert(acknowledged.length > 0);
  assert.throws(() => { throw Error('injected storage failure'); });
  assert.equal(live.value(), 1n);
  console.log('PRODUCT planted-a MAIN exposed=1 after storage failure');
  // (b) Replay of a self-consistent old log reissues an acknowledged sequence.
  const old = live.logBytes();
  live.appendBump(0, 2n);
  assert.equal(live.versionFor(0n), 2n);
  const restored = new wasm.SafeMeshGCounterReplica(0n, 2);
  restored.mergeLogBytes(old);
  restored.appendBump(0, 3n);
  assert.equal(restored.versionFor(0n), 2n);
  console.log('PRODUCT planted-b MAIN old log reissues sequence=2');
  // (c) The existing restore example refuses an absent root (exit 2).
  const example = fileURLToPath(new URL('../examples/node-persist.mjs', import.meta.url));
  const restore = spawnSync(process.execPath, [example, pkg, join(root, 'missing'), 'restore'], { encoding: 'utf8' });
  assert.equal(restore.status, 2, restore.stderr);
  assert.match(restore.stderr + restore.stdout, /missing/);
  console.log('PRODUCT planted-c MAIN missing restore exit=2');
  live.free(); restored.free();
  console.log('LOW_LEVEL_PLANTED_CASES=3');
} else {
  const Managed = wasm.SafeMeshManagedGCounter;
  let groups = 0;
  function error(code) { return e => e.name === 'SafeMeshStoreError' && e.code === code; }
  function fail(code) { const e = Error(code); e.name = 'SafeMeshStoreError'; e.code = code; throw e; }
  class Store {
    bytes; revision = 0n; anchor = 0n; lease; commits = 0; hook;
    open(mode, writer) {
      if (this.lease) fail('LOCKED');
      if (mode === 'fresh' && this.bytes) fail('EXISTS');
      if (mode === 'restart' && !this.bytes) fail('MISSING');
      return this.lease = { writer };
    }
    readCommitted(lease) {
      assert.equal(lease, this.lease);
      return { bytes: this.bytes.slice(), revision: this.revision, anchor: this.anchor };
    }
    commit(lease, expected, bytes) {
      assert.equal(lease, this.lease); assert.equal(expected, this.revision);
      this.commits++;
      if (this.hook) return this.hook(bytes, expected);
      this.bytes = bytes.slice(); this.anchor = ++this.revision;
      return this.revision;
    }
    close(lease) { assert.equal(lease, this.lease); this.lease = undefined; }
  }
  const fresh = (store, writer = 0n) => Managed.open(store, { mode: 'fresh', writer, writers: 2 });
  const restart = (store, writer = 0n) => Managed.open(store, { mode: 'restart', writer });
  const store = new Store();
  let live = fresh(store);
  const initial = live.peerLogBytes();
  // (a) No state or record bytes escape a failed synchronous commit.
  store.hook = () => { throw Error('injected storage failure'); };
  assert.throws(() => live.appendBump(1n), /injected storage failure/);
  assert.equal(live.value(), 0n);
  assert.deepEqual(live.peerLogBytes(), initial);
  assert.throws(() => live.appendBump(2n), error('DISABLED'));
  assert.throws(() => restart(store), error('LOCKED'));
  assert(store.lease, 'failure retains the lease');
  live.close(); live.free(); store.hook = undefined;
  live = restart(store);
  assert.equal(live.value(), 0n, 'ordinary close/restart equals last committed state');
  console.log('PRODUCT planted-a MANAGED refused; close/restart value=0'); groups++;
  live.appendBump(1n); const old = store.bytes.slice(), oldRevision = store.revision;
  live.appendBump(2n); live.close(); live.free();
  // (b) Old valid snapshot cannot cross the independent revision anchor.
  const latest = store.bytes.slice(), latestRevision = store.revision;
  store.bytes = old; store.revision = oldRevision;
  assert.throws(() => restart(store), error('STALE'));
  assert.equal(store.lease, undefined, 'failed open releases lease');
  store.bytes = latest; store.revision = latestRevision;
  live = restart(store); assert.equal(live.value(), 2n); live.close(); live.free();
  console.log('PRODUCT planted-b MANAGED stale snapshot refused'); groups++;
  // (c) Missing restart refuses and fresh creates committed initial metadata.
  const missing = new Store();
  assert.throws(() => restart(missing), error('MISSING'));
  const created = fresh(missing); assert.equal(created.value(), 0n); created.close(); created.free();
  assert.throws(() => fresh(missing), error('EXISTS'));
  console.log('PRODUCT planted-c MANAGED missing restart refused; fresh committed'); groups++;
  // Wrong revision and asynchronous callbacks are ambiguous failures: no publication.
  for (const result of [99n, 2, undefined, Promise.resolve(2n)]) {
    const s = new Store(), h = fresh(s);
    s.hook = () => result;
    assert.throws(() => h.appendBump(1n), error('COMMIT'));
    assert.equal(h.value(), 0n);
    assert.throws(() => h.appendBump(2n), error('DISABLED'));
    h.close(); h.free(); s.hook = undefined;
    const r = restart(s); assert.equal(r.value(), 0n); r.close(); r.free();
  }
  groups++;
  // A store may commit and then throw: recovery reads that verified commit,
  // while the original handle continues to expose its previous committed state.
  {
    const s = new Store(), h = fresh(s);
    s.hook = (bytes, expected) => {
      s.bytes = bytes.slice(); s.revision = s.anchor = expected + 1n;
      throw Error('ambiguous after commit');
    };
    assert.throws(() => h.appendBump(7n), /ambiguous/); assert.equal(h.value(), 0n);
    h.close(); h.free(); s.hook = undefined;
    const r = restart(s); assert.equal(r.value(), 7n); r.close(); r.free();
  }
  groups++;
  // Same-handle callback reentry rejects edit, read, peer bytes and close.
  {
    const s = new Store(), h = fresh(s);
    s.hook = (bytes, expected) => {
      for (const call of [() => h.appendBump(9n), () => h.value(), () => h.state(),
        () => h.peerLogBytes(), () => h.versionFor(0n), () => h.mergeLogBytes(initial), () => h.close()])
        assert.throws(call, error('REENTRY'));
      s.bytes = bytes.slice(); s.revision = s.anchor = expected + 1n; return s.revision;
    };
    h.appendBump(3n); assert.equal(h.value(), 3n); h.close();
    assert.throws(() => h.appendBump(4n), error('CLOSED')); h.close(); h.free();
  }
  groups++;
  // Every envelope field, full replay and independent anchor are checked.
  for (const offset of [0, 8, 9, 17, 25, 41]) {
    const s = new Store(), h = fresh(s); h.appendBump(1n); h.close(); h.free();
    s.bytes[offset] ^= 255;
    assert.throws(() => restart(s), error('CORRUPT')); assert.equal(s.lease, undefined);
  }
  {
    const s = new Store(), h = fresh(s); h.close(); h.free(); s.bytes = new Uint8Array(2);
    assert.throws(() => restart(s), error('CORRUPT'));
    assert.throws(() => Managed.open(s, { mode: 'restart', writer: 0n, writers: 2 }), error('CORRUPT'));
    assert.throws(() => Managed.open(s, { mode: 'fresh', writer: 2n, writers: 2 }), error('CORRUPT'));
  }
  groups++;
  // Accepted peer batches commit once; duplicates don't; collisions and bad
  // ownership in a batch expose no prefix and cause no store call.
  {
    const s = new Store(), h = fresh(s);
    const peer = new wasm.SafeMeshGCounterReplica(1n, 2);
    const first = peer.appendBump(1, 4n); peer.appendBump(1, 5n);
    const before = s.commits;
    assert.deepEqual(h.mergeLogBytes(peer.logBytes()), ['accepted', 'accepted']);
    assert.equal(s.commits, before + 1); assert.equal(h.value(), 5n);
    assert.equal(h.mergeRecordBytes(first), 'duplicate');
    assert.deepEqual(h.mergeLogBytes(peer.logBytes()), ['duplicate', 'duplicate']);
    assert.equal(s.commits, before + 1);
    const collision = new wasm.SafeMeshGCounterReplica(1n, 2);
    const collisionBytes = collision.appendBump(1, 9n);
    assert.throws(() => h.mergeRecordBytes(collisionBytes), error('COLLISION'));
    assert.throws(() => h.mergeLogBytes(collision.logBytes()), error('COLLISION'));
    // New accepted author-0 record followed by an author-1 collision.
    const mixed = new wasm.SafeMeshGCounterReplica(0n, 2);
    mixed.appendBump(0, 6n); mixed.mergeRecordBytes(collisionBytes);
    assert.throws(() => h.mergeLogBytes(mixed.logBytes()), error('COLLISION'));
    assert.equal(h.value(), 5n); assert.equal(s.commits, before + 1);
    const bad = peer.logBytes().slice(); bad[bad.length - 1] ^= 255;
    assert.throws(() => h.mergeLogBytes(bad));
    assert.equal(h.value(), 5n); assert.equal(s.commits, before + 1);
    s.hook = () => { throw Error('peer commit failed'); };
    const next = peer.appendBump(1, 8n);
    assert.throws(() => h.mergeRecordBytes(next), /peer commit failed/);
    assert.equal(h.value(), 5n); h.close(); h.free(); s.hook = undefined;
    const r = restart(s); assert.equal(r.value(), 5n); r.close(); r.free();
    peer.free(); collision.free(); mixed.free();
  }
  groups++;
  // Real filesystem Store: missing root, exclusive lease, fresh refusal,
  // committed restart, stale snapshot and corrupt envelope.
  {
    const path = join(root, 'counter'), anchorPath = join(root, 'anchor');
    const s = new FileSystemStore(path, anchorPath);
    assert.throws(() => restart(s), error('MISSING')); assert(!existsSync(path));
    const h = fresh(s); h.appendBump(2n);
    assert.throws(() => restart(new FileSystemStore(path, anchorPath)), error('LOCKED'));
    const saved = readFileSync(join(path, 'committed.bin'));
    h.appendBump(4n); h.close(); h.free();
    assert.throws(() => fresh(new FileSystemStore(path, anchorPath)), error('EXISTS'));
    const r = restart(new FileSystemStore(path, anchorPath)); assert.equal(r.value(), 4n); r.close(); r.free();
    const final = readFileSync(join(path, 'committed.bin'));
    writeFileSync(join(path, 'committed.bin'), saved);
    assert.throws(() => restart(new FileSystemStore(path, anchorPath)), error('STALE'));
    final[8] = 255; writeFileSync(join(path, 'committed.bin'), final);
    assert.throws(() => restart(new FileSystemStore(path, anchorPath)), error('CORRUPT'));
  }
  groups++;
  console.log(`MANAGED_COUNTER_GROUPS=${groups}`);
}
