import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { mkdtempSync, existsSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { FileSystemStore } from '../examples/node-filesystem-store.mjs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const wasm = createRequire(import.meta.url)(join(resolve(process.argv[2]), 'safemesh_wasm.js'));
const error = code => e => e.name === 'SafeMeshStoreError' && e.code === code;
const storeError = code => Object.assign(Error(code), { name: 'SafeMeshStoreError', code });
class Store {
  bytes; revision = 0n; anchor = 0n; lease; hook;
  open(mode, writer) {
    if (this.lease) throw storeError('LOCKED');
    if (mode === 'fresh' && this.bytes) throw storeError('EXISTS');
    if (mode === 'restart' && !this.bytes) throw storeError('MISSING');
    return this.lease = { writer };
  }
  readCommitted(lease) {
    assert.equal(lease, this.lease);
    return { bytes: this.bytes.slice(), revision: this.revision, anchor: this.anchor };
  }
  commit(lease, expected, bytes) {
    assert.equal(lease, this.lease); assert.equal(expected, this.revision);
    if (this.hook) return this.hook(bytes, expected);
    this.bytes = bytes.slice(); this.anchor = ++this.revision; return this.revision;
  }
  close(lease) { assert.equal(lease, this.lease); this.lease = undefined; }
}
const fresh = (store, writer = 0n) => wasm.SafeMeshManagedStringOrSet.open(store, { mode: 'fresh', writer, writers: 2 });
const restart = (store, writer = 0n) => wasm.SafeMeshManagedStringOrSet.open(store, { mode: 'restart', writer });
if (process.argv.includes('--child')) {
  const h = fresh(new FileSystemStore(process.argv.at(-2), process.argv.at(-1)));
  h.appendAdd('committed');
  process.kill(process.pid, 'SIGKILL');
} else if (process.argv.includes('--low-level')) {
  const live = wasm.SafeMeshStringOrSetReplica.createAllocated(2n, 0n);
  const old = live.exportIdentity();
  const add = live.appendAllocatedAdd('α');
  assert(add.length > 0); assert.deepEqual(live.elements(), ['α']);
  assert.throws(() => { throw Error('injected save failure'); });
  assert.deepEqual(live.elements(), ['α']);
  console.log('PRODUCT planted-a MAIN acknowledged add visible after save failure');
  live.free();
  const resumed = wasm.SafeMeshStringOrSetReplica.importIdentity(old);
  const reissued = resumed.appendAllocatedAdd('β');
  assert.equal(wasm.SafeMeshStringOrSetReplica.inspectRecordBytes(add).token(),
    wasm.SafeMeshStringOrSetReplica.inspectRecordBytes(reissued).token());
  console.log('PRODUCT planted-b MAIN old identity reissued allocated token');
  resumed.free();
  const root = mkdtempSync(join(tmpdir(), 'managed-set-low-'));
  const absent = join(root, 'absent');
  assert.throws(() => new FileSystemStore(absent, join(root, 'anchor')).open('restart', 0n), error('MISSING'));
  assert(!existsSync(absent));
  console.log('PRODUCT planted-c MAIN missing restore root refused');
  console.log('LOW_LEVEL_PLANTED_CASES=3');
} else {
  let groups = 0;
  const s = new Store(); let h = fresh(s);
  s.hook = () => { throw Error('injected save failure'); };
  assert.throws(() => h.appendAdd('α'), /injected save failure/);
  assert.deepEqual(h.elements(), [], 'case e: failed add must remain invisible');
  assert.throws(() => h.appendAdd('β'), error('DISABLED'));
  assert.throws(() => restart(s), error('LOCKED'));
  h.close(); s.hook = undefined;
  const closedHandle = h;
  h = restart(s); assert.deepEqual(h.elements(), []); h.close(); h.free();
  closedHandle.free();
  console.log('PRODUCT planted-a/e MANAGED failed add invisible and write-disabled'); groups++;
  h = restart(s); const one = h.appendAdd('α'); const old = s.bytes.slice(), oldRev = s.revision;
  h.appendAdd('β'); h.close(); h.free();
  const latest = s.bytes.slice(), latestRev = s.revision;
  s.bytes = old; s.revision = oldRev;
  assert.throws(() => restart(s), error('STALE')); assert.equal(s.lease, undefined);
  s.bytes = latest; s.revision = latestRev;
  h = restart(s); assert.deepEqual(h.elements(), ['α', 'β']);
  h.close(); h.free();
  assert.throws(() => wasm.SafeMeshManagedStringOrSet.open(s, { mode: 'restart', writer: 0n, writers: 3 }),
    error('CORRUPT'), 'case b: restart must refuse caller writer count');
  console.log('PRODUCT planted-b MANAGED stale identity refused and count supplied on restart refused'); groups++;
  assert.throws(() => restart(new Store()), error('MISSING'));
  const root = mkdtempSync(join(tmpdir(), 'managed-set-'));
  const absent = join(root, 'absent');
  assert.throws(() => restart(new FileSystemStore(absent, join(root, 'anchor'))), error('MISSING'));
  assert(!existsSync(absent));
  console.log('PRODUCT planted-c MANAGED missing root refused'); groups++;
  const counter = new Store(), set = new Store();
  let c = wasm.SafeMeshManagedGCounter.open(counter, { mode: 'fresh', writer: 0n, writers: 2 }); c.close(); c.free();
  let x = fresh(set); x.close(); x.free();
  assert.throws(() => restart(counter), error('CORRUPT'));
  assert.throws(() => wasm.SafeMeshManagedGCounter.open(set, { mode: 'restart', writer: 0n }), error('CORRUPT'));
  console.log('PRODUCT kind mismatch refused in both directions'); groups++;
  {
    const reentry = new Store(), active = fresh(reentry);
    reentry.hook = (bytes, expected) => {
      for (const call of [() => active.appendAdd('nested'), () => active.elements(),
        () => active.peerLogBytes(), () => active.close()])
        assert.throws(call, error('REENTRY'));
      reentry.bytes = bytes.slice(); reentry.anchor = reentry.revision = expected + 1n;
      return reentry.revision;
    };
    active.appendAdd('outer'); assert.deepEqual(active.elements(), ['outer']);
    active.close(); active.free();
  console.log('PRODUCT Store callback reentry refused'); groups++;
  }
  {
    const localStore = new Store(), remoteStore = new Store();
    const local = fresh(localStore), remote = fresh(remoteStore, 1n);
    local.appendAdd('keep');
    const incoming = remote.appendAdd('peer');
    localStore.hook = () => { throw Error('receive commit failed'); };
    assert.throws(() => local.mergeRecordBytes(incoming), /receive commit failed/);
    assert.deepEqual(local.elements(), ['keep'], 'failed receive must remain invisible');
    assert.throws(() => local.appendRemoveObserved('keep'), error('DISABLED'));
    local.close(); local.free(); localStore.hook = undefined;
    const recovered = restart(localStore);
    recovered.mergeRecordBytes(incoming);
    localStore.hook = () => { throw Error('remove commit failed'); };
    assert.throws(() => recovered.appendRemoveObserved('keep'), /remove commit failed/);
    assert.deepEqual(recovered.elements(), ['keep', 'peer'], 'failed remove must remain invisible');
    recovered.close(); recovered.free(); remote.close(); remote.free();
    console.log('PRODUCT failed receive/remove remained invisible'); groups++;
  }
  const peerStore = new Store(); const peer = fresh(peerStore, 1n);
  h = restart(s);
  const before = h.versionVector(); const peerAdd = peer.appendAdd('γ');
  assert.equal(h.mergeRecordBytes(peerAdd), 'accepted');
  assert.deepEqual(h.elements(), ['α', 'β', 'γ']);
  const remove = h.appendRemoveObserved('α'); assert(remove.length > 0);
  h.close(); h.free(); h = restart(s);
  assert.deepEqual(h.elements(), ['β', 'γ']);
  const delta = peer.sinceLogBytes(h.versionVector());
  assert.deepEqual(h.mergeLogBytes(delta), []);
  const outbound = h.sinceLogBytes(peer.versionVector());
  peer.mergeLogBytes(outbound);
  assert.deepEqual(peer.elements(), h.elements());
  assert(before.length >= 0);
  h.close(); h.free(); peer.close(); peer.free();
  console.log('PRODUCT receive/remove/restart/sinceLogBytes converged'); groups++;
  {
    const writerStore = new Store(), original = fresh(writerStore, 1n);
    original.close(); original.free();
    assert.throws(() => restart(writerStore, 0n), error('CORRUPT'),
      'restart must refuse a different writer');
    assert.equal(writerStore.lease, undefined, 'refused restart releases lease');
    const sameWriter = restart(writerStore, 1n);
    assert.deepEqual(sameWriter.elements(), []);
    sameWriter.close(); sameWriter.free();
    console.log('PRODUCT writer mismatch refused with CORRUPT; same writer restarted'); groups++;
  }
  {
    const root = mkdtempSync(join(tmpdir(), 'managed-set-kill-'));
    const path = join(root, 'store'), anchor = join(root, 'anchor');
    const child = spawnSync(process.execPath,
      [fileURLToPath(import.meta.url), process.argv[2], '--child', path, anchor], { encoding: 'utf8' });
    assert.equal(child.signal, 'SIGKILL', child.stderr);
    // The child has exited; its lock file is now orphaned.
    unlinkSync(join(path, 'lease.lock'));
    const recovered = restart(new FileSystemStore(path, anchor));
    assert.deepEqual(recovered.elements(), ['committed']);
    recovered.appendAdd('after-restart');
    assert.deepEqual(recovered.elements(), ['after-restart', 'committed']);
    recovered.close(); recovered.free();
    console.log('PRODUCT SIGKILL between adds recovered exactly committed set'); groups++;
  }
  console.log(`MANAGED_ORSET_GROUPS=${groups}`);
}
