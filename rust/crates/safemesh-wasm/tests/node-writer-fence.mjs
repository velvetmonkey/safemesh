import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { existsSync, mkdtempSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { once } from 'node:events';
import test from 'node:test';
import { FileSystemStore } from '../examples/node-filesystem-store.mjs';

const [pkgArg, rootArg, anchorArg, mode] = process.argv.slice(2);
const pkg = resolve(pkgArg);
const wasm = createRequire(import.meta.url)(join(pkg, 'safemesh_wasm.js'));
const open = (root, anchor, mode) => wasm.SafeMeshManagedGCounter.open(
  new FileSystemStore(root, anchor), mode === 'fresh'
    ? { mode, writer: 0n, writers: 2 }
    : { mode, writer: 0n });

if (mode === 'hold') {
  const handle = open(rootArg, anchorArg, 'fresh');
  process.stdout.write('READY\n');
  setInterval(() => { assert(handle); }, 1000);
} else if (mode === 'try') {
  assert.throws(() => open(rootArg, anchorArg, 'restart'),
    error => error.name === 'SafeMeshStoreError' && error.code === 'LOCKED',
    'second process must get LOCKED');
  console.log('LOCKED');
} else {
  await test('node_writer_fence_two_process_and_killed_holder', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'node-writer-fence-'));
    const root = join(dir, 'store'), anchor = join(dir, 'anchor');
    const holder = spawn(process.execPath, [process.argv[1], pkg, root, anchor, 'hold'],
      { stdio: ['ignore', 'pipe', 'pipe'] });
    try {
      await new Promise((done, fail) => {
        holder.stdout.once('data', chunk => chunk.toString().includes('READY') ? done() : fail(Error('no READY')));
        holder.once('exit', code => fail(Error(`holder exited ${code}`)));
      });
      const second = spawnSync(process.execPath, [process.argv[1], pkg, root, anchor, 'try'], { encoding: 'utf8' });
      assert.equal(second.status, 0, second.stderr);
      assert.match(second.stdout, /LOCKED/);
      holder.kill('SIGKILL');
      await once(holder, 'exit');
      assert(existsSync(join(root, 'lease.lock')), 'killed holder left no lock file');
      const afterKill = spawnSync(process.execPath, [process.argv[1], pkg, root, anchor, 'try'], { encoding: 'utf8' });
      assert.equal(afterKill.status, 0, afterKill.stderr);
      unlinkSync(join(root, 'lease.lock')); // Person removes it only after all users stop.
      const restarted = open(root, anchor, 'restart');
      restarted.close(); restarted.free();
    } finally {
      if (holder.exitCode === null && holder.signalCode === null) holder.kill('SIGKILL');
    }
  });
}
