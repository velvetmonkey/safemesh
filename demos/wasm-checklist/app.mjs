import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { mkdirSync, existsSync, readFileSync, writeFileSync, renameSync, openSync, fsyncSync, closeSync } from 'node:fs';
import { createServer } from 'node:http';
// Resolution starts in the external npm consumer, never in the checkout.
const require = createRequire(resolve(process.env.SAFEMESH_CONSUMER, 'package.json'));
const { SafeMeshStringOrSetReplica: Set } = require('safemesh-wasm');
const [dir, authorText] = process.argv.slice(2);
const author = BigInt(authorText);
mkdirSync(dir, { recursive: true });
const identity = resolve(dir, 'identity.bin'), log = resolve(dir, 'log.bin');
let replica;
function saveFile(path, bytes) {
  const fd = openSync(`${path}.next`, 'w');
  try { writeFileSync(fd, bytes); fsyncSync(fd); } finally { closeSync(fd); }
  renameSync(`${path}.next`, path);
  const parent = openSync(dir, 'r');
  try { fsyncSync(parent); } finally { closeSync(parent); }
}
function persist() {
  // Identity is the full recovery snapshot. The extra log is a checked mirror.
  // A kill between these two commits fails closed, rather than resetting a writer.
  saveFile(identity, replica.exportIdentity());
  saveFile(log, replica.logBytes());
}
try {
  if (existsSync(identity)) {
    if (!existsSync(log)) throw Error('RESTART_LOG_MISSING');
    replica = process.env.CHECKLIST_FRESH_IDENTITY === '1'
      ? Set.createAllocated(2n, author) : Set.importIdentity(readFileSync(identity));
    if (process.env.CHECKLIST_FRESH_IDENTITY !== '1' &&
        !Buffer.from(replica.logBytes()).equals(readFileSync(log))) throw Error('RESTART_LOG_MISMATCH');
  } else {
    if (existsSync(log)) throw Error('RESTART_IDENTITY_MISSING');
    replica = Set.createAllocated(2n, author);
    persist();
  }
} catch (error) { console.error(`FAIL ${error.message}`); process.exit(1); }
const b64 = bytes => Buffer.from(bytes).toString('base64');
function state() {
  const entries = replica.addEntries();
  try {
    return { elements: replica.elements(), log: b64(replica.logBytes()), identity: b64(replica.exportIdentity()),
      versions: [replica.versionFor(0n).toString(), replica.versionFor(1n).toString()],
      adds: entries.map(e => [e.element(), e.token().toString()]),
      observed: Array.from(replica.observedTokens('naïve'), String),
      tombstones: Array.from(replica.tombstones(), String) };
  } finally { entries.forEach(e => e.free()); }
}
const server = createServer(async (req, res) => {
  try {
    const chunks = []; for await (const chunk of req) chunks.push(chunk);
    const body = chunks.length ? JSON.parse(Buffer.concat(chunks)) : {};
    let result;
    if (req.method === 'GET' && req.url === '/state') result = state();
    else if (req.method === 'GET' && req.url === '/log') result = { log: b64(replica.logBytes()) };
    else if (req.method === 'POST' && req.url === '/edit') {
      const records = [];
      for (const edit of body.edits) records.push(b64(edit.remove
        ? replica.appendRemoveObserved(edit.item) : replica.appendAllocatedAdd(edit.item)));
      persist(); result = { records, state: state() };
    } else if (req.method === 'POST' && req.url === '/merge') {
      const verdicts = body.records ? body.records.map(r => replica.mergeRecordBytes(Buffer.from(r, 'base64')))
        : replica.mergeLogBytes(Buffer.from(body.log, 'base64'));
      persist(); result = { verdicts };
    } else { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify(result));
  } catch (error) { res.writeHead(400); res.end(JSON.stringify({ error: String(error) })); }
});
server.listen(0, '127.0.0.1', () => console.log(JSON.stringify({ port: server.address().port, pid: process.pid,
  package: require.resolve('safemesh-wasm') })));
