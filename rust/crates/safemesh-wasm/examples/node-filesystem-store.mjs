// Example synchronous Node Store: exclusive wx lock, atomic rename, file fsync
// and directory fsync (POSIX filesystems). No stale-lock recovery is automatic.
// After a crash leaves lease.lock, stop all programs that can open this store and
// prevent their supervisors from restarting them. Check the crashed process has
// exited (for example with ps), and use lsof /absolute/store/root/lease.lock to
// check for open holders. An empty lsof result alone is not enough: keep every
// potential store user stopped while removing /absolute/store/root/lease.lock
// with rm, then restart in mode "restart". Do not remove the snapshot or anchor.
// After a crash between completed edits, restart reads the last committed value.
// The independent anchorPath MUST be retained outside snapshot rollback policy.
// Anchor and envelope are separate durable replacements: interruption between
// them fails closed at restart; this example does not promise recovery then.
// Crash between a rename and directory fsync is not injected by this example.
import { mkdirSync, openSync, closeSync, writeFileSync, readFileSync,
  fsyncSync, renameSync, unlinkSync, statSync, fstatSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

function failure(code, message) {
  const error = new Error(message); error.name = 'SafeMeshStoreError'; error.code = code; return error;
}
function read(path) {
  try { return readFileSync(path); }
  catch (error) { throw failure(error.code === 'ENOENT' ? 'MISSING' : 'CORRUPT', `Cannot read ${path}: ${error.message}`); }
}
function revision(bytes) {
  if (bytes.length < 41) throw failure('CORRUPT', 'Truncated envelope');
  return bytes.readBigUInt64LE(33);
}
function anchor(path) {
  const text = read(path).toString();
  if (!/^[1-9][0-9]*\n$/.test(text)) throw failure('CORRUPT', 'Invalid independent anchor');
  const value = BigInt(text.trim());
  if (value > 18446744073709551615n) throw failure('CORRUPT', 'Anchor overflow');
  return value;
}
function replace(path, bytes) {
  const temporary = `${path}.next-${process.pid}`;
  let fd;
  try {
    fd = openSync(temporary, 'wx', 0o600);
    writeFileSync(fd, bytes); fsyncSync(fd); closeSync(fd); fd = undefined;
    renameSync(temporary, path);
    const directory = openSync(dirname(path), 'r');
    try { fsyncSync(directory); } finally { closeSync(directory); }
  } finally {
    if (fd !== undefined) closeSync(fd);
    try { unlinkSync(temporary); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
}
export class FileSystemStore {
  constructor(root, anchorPath) {
    this.root = resolve(root); this.anchorPath = resolve(anchorPath);
    if (this.anchorPath === this.root || this.anchorPath.startsWith(`${this.root}/`))
      throw failure('CORRUPT', 'Independent anchor must be outside the snapshot root');
    this.transaction = join(this.root, 'committed.bin');
    this.lock = join(this.root, 'lease.lock');
    this.lease = undefined;
  }
  open(mode, writer) {
    if (this.lease) throw failure('LOCKED', 'Store already leased');
    if (mode === 'fresh') {
      try { statSync(this.anchorPath); throw failure('EXISTS', 'Independent anchor exists'); }
      catch (error) { if (error.code !== 'ENOENT') throw error; }
      try { mkdirSync(this.root); }
      catch (error) { throw failure(error.code === 'EEXIST' ? 'EXISTS' : 'MISSING', error.message); }
      const parent = openSync(dirname(this.root), 'r');
      try { fsyncSync(parent); } finally { closeSync(parent); }
    } else if (mode === 'restart') {
      try { if (!statSync(this.root).isDirectory()) throw failure('CORRUPT', 'Root is not a directory'); }
      catch (error) { if (error.code === 'ENOENT') throw failure('MISSING', 'Restart root missing'); throw error; }
    } else throw failure('CORRUPT', 'Unknown mode');
    let fd;
    try { fd = openSync(this.lock, 'wx', 0o600); }
    catch (error) { throw failure('LOCKED', error.message); }
    this.lease = { fd, writer, mode, revision: 0n };
    try {
      if (mode === 'restart') this.lease.revision = this.readCommitted(this.lease).revision;
      return this.lease;
    } catch (error) { this.close(this.lease); throw error; }
  }
  check(lease) {
    if (!lease || lease !== this.lease) throw failure('LOCKED', 'Lost lease');
    const owned = fstatSync(lease.fd);
    let current;
    try { current = statSync(this.lock); } catch { throw failure('LOCKED', 'Lost lock'); }
    if (current.ino !== owned.ino || current.dev !== owned.dev) throw failure('LOCKED', 'Replaced lock');
  }
  readCommitted(lease) {
    this.check(lease);
    const bytes = read(this.transaction), current = revision(bytes), high = anchor(this.anchorPath);
    if (current !== high) throw failure('STALE', 'Snapshot differs from independent anchor');
    return { bytes, revision: current, anchor: high };
  }
  commit(lease, expectedRevision, nextBytes) {
    this.check(lease);
    if (lease.revision !== expectedRevision) throw failure('STALE', 'Obsolete expected revision');
    if (expectedRevision !== 0n && this.readCommitted(lease).revision !== expectedRevision)
      throw failure('STALE', 'Obsolete committed revision');
    const bytes = Buffer.from(nextBytes), next = revision(bytes);
    if (next !== expectedRevision + 1n) throw failure('CORRUPT', 'Wrong next revision');
    // Fence first: a partial commit is detectable rather than silently rolled back.
    replace(this.anchorPath, `${next}\n`);
    replace(this.transaction, bytes);
    lease.revision = next;
    return next;
  }
  close(lease) {
    this.check(lease);
    unlinkSync(this.lock); closeSync(lease.fd); this.lease = undefined;
  }
}
