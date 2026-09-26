import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createInterface } from 'node:readline';
import { mkdtempSync, cpSync, unlinkSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const here = dirname(fileURLToPath(import.meta.url));
const root = mkdtempSync(resolve(process.env.CHECKLIST_SCRATCH || process.env.TMPDIR, 'checklist-'));
const live = new Set();
const tamper = process.env.CHECKLIST_TAMPER;
const plantIdx = process.argv.indexOf('--plant');
const plant = plantIdx >= 0 ? process.argv[plantIdx + 1] : '';
function check(condition, name) { assert.ok(condition, name); }
async function start(dir, author, fresh = false) {
  const child = spawn(process.execPath, [resolve(here, 'app.mjs'), dir, String(author)], {
    env: { ...process.env, CHECKLIST_FRESH_IDENTITY: fresh ? '1' : '0' }, stdio: ['ignore', 'pipe', 'pipe'] });
  live.add(child);
  let errors = ''; child.stderr.on('data', b => errors += b);
  const lines = createInterface({ input: child.stdout });
  return await new Promise((yes, no) => {
    const timer = setTimeout(() => { child.kill('SIGKILL'); no(Error('APP_START_TIMEOUT')); }, 20000);
    child.once('exit', () => { clearTimeout(timer); live.delete(child); no(Error(errors.trim() || 'APP_START_EXIT')); });
    lines.once('line', line => { clearTimeout(timer); yes({ child, dir, ...JSON.parse(line) }); });
  });
}
async function kill(app) { const exited = once(app.child, 'exit'); app.child.kill('SIGKILL'); await exited; live.delete(app.child); }
async function request(app, path, body) {
  const res = await fetch(`http://127.0.0.1:${app.port}${path}`, body === undefined ? {} : {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  const value = await res.json(); check(res.ok, `HTTP_${path}: ${JSON.stringify(value)}`); return value;
}
const state = app => request(app, '/state');
const edit = (app, edits) => request(app, '/edit', { edits });
async function merge(app, body, duplicate = false) {
  const { verdicts } = await request(app, '/merge', body);
  check(verdicts.length > 0 && verdicts.every(v => v === 'accepted' || v === 'duplicate'), 'MERGE_ADMISSION');
  if (duplicate) check(verdicts.every(v => v === 'duplicate'), 'SECOND_ORDER_DUPLICATES');
  return verdicts;
}
function allDuplicate(verdicts) {
  return verdicts.length > 0 && verdicts.every(v => v === 'duplicate');
}
// Whole-log duplicate admission in BOTH directions. A missing record on one
// side is `accepted` by that side, not `duplicate`; a late extra record on
// one side is `accepted` by the other. Either case fails this named oracle.
// Verdicts, not counts: equal lengths can still hide a distinct record.
async function assertWholeLogDuplicates(left, right, name) {
  const toRight = await merge(right, await request(left, '/log'));
  const toLeft = await merge(left, await request(right, '/log'));
  check(allDuplicate(toRight) && allDuplicate(toLeft), name);
  return { toRight, toLeft };
}
try {
  if (plantIdx >= 0 && plant !== 'drop-one-record' && plant !== 'extra-record') {
    throw Error(`UNKNOWN_PLANT ${plant || ''}`);
  }
  let a = await start(resolve(root, 'a'), 0), b = await start(resolve(root, 'b'), 1);
  check(a.pid !== b.pid && a.port !== b.port, 'INDEPENDENT_APPS');
  check(a.package.includes('/node_modules/safemesh-wasm/') && b.package === a.package, 'INSTALLED_PACKAGE');
  await edit(a, [{ item: 'naïve' }, { item: 'γειά' }]); await edit(b, [{ item: '🧭' }]);
  await merge(b, await request(a, '/log')); await merge(a, await request(b, '/log'));
  check((await state(a)).elements.includes('γειά') && (await state(b)).elements.includes('🧭'), 'UTF8');
  // Partition: no merges until both offline edits and the restart are complete.
  const ar = await edit(a, [{ item: 'naïve' }, ...Array.from({ length: 1000 }, (_, i) => ({ item: `α-${i}` }))]);
  const br = await edit(b, [{ item: 'naïve', remove: true }, ...Array.from({ length: 1000 }, (_, i) => ({ item: `β-${i}` }))]);
  check(ar.state.elements.includes('naïve') && !br.state.elements.includes('naïve'), 'PARTITION_CONFLICT');
  const before = await state(a); await kill(a);
  // Every tamper operates on a throwaway store copy, retaining the original.
  let restartDir = a.dir;
  if (tamper) { restartDir = resolve(root, 'tampered'); cpSync(a.dir, restartDir, { recursive: true }); }
  if (tamper === 'delete-log') unlinkSync(resolve(restartDir, 'log.bin'));
  if (tamper === 'corrupt-log') writeFileSync(resolve(restartDir, 'log.bin'), Buffer.from([0]));
  a = await start(restartDir, 0, tamper === 'fresh-identity');
  const restored = await state(a);
  check(restored.identity === before.identity && restored.log === before.log, 'RESTART_IDENTITY_CONTINUITY');
  const after = await edit(a, [{ item: 'restart-✅' }]);
  check(BigInt(after.state.versions[0]) === BigInt(before.versions[0]) + 1n, 'RESTART_SEQUENCE');
  check(after.state.adds.find(e => e[0] === 'restart-✅')[1] !== before.adds.at(-1)[1], 'RESTART_TOKEN');
  // Explicit later-before-earlier record batches, followed by duplicate delivery.
  let aRecords = [...ar.records, ...after.records].reverse();
  const bRecords = [...br.records].reverse();
  if (plant === 'drop-one-record') {
    check(aRecords.length > 1, 'PLANT_HAS_RECORD_TO_DROP');
    aRecords = aRecords.slice(1);
  }
  await merge(b, { records: aRecords }); await merge(b, { records: aRecords }, true);
  await merge(a, { records: bRecords }); await merge(a, { records: bRecords }, true);
  const observedA = await state(a), observedB = await state(b);
  // using-safemesh.md:502-505: logs retain arrival order. Compare records by
  // duplicate admission and carrier metadata separately, rather than byte order.
  console.log(`OBSERVE log-bytes-equal=${observedA.log === observedB.log}`);
  await assertWholeLogDuplicates(a, b, 'CONVERGENCE_WHOLE_LOG_DUPLICATES');
  const sa = await state(a), sb = await state(b);
  assert.deepEqual(sa.elements, sb.elements, 'CONVERGENCE_ELEMENTS');
  assert.deepEqual(sa.versions, sb.versions, 'CONVERGENCE_VECTORS');
  assert.deepEqual(sa.adds, sb.adds, 'CONVERGENCE_ADDS');
  assert.deepEqual(sa.tombstones, sb.tombstones, 'CONVERGENCE_TOMBSTONES');
  const conflict = sa.adds.filter(e => e[0] === 'naïve');
  check(conflict.length === 2 && sa.observed.length === 1 && sa.tombstones.includes(conflict[0][1]) &&
    !sa.tombstones.includes(conflict[1][1]) && sa.elements.includes('naïve'), 'CONFLICT_INSPECTION');
  const count = (await merge(a, await request(b, '/log'), true)).length;
  check(count >= 2000, 'RETAINED_HISTORY');
  if (plant === 'extra-record') {
    await edit(a, [{ item: 'late-extra' }]);
    console.log('PLANTED extra-record on app a after convergence snapshot');
    await assertWholeLogDuplicates(a, b, 'CONVERGENCE_WHOLE_LOG_DUPLICATES');
    throw Error('ORACLE_MISSED_EXTRA_RECORD');
  }
  await kill(a); await kill(b); a = await start(a.dir, 0); b = await start(b.dir, 1);
  await merge(a, await request(b, '/log'), true); await merge(b, await request(a, '/log'), true);
  assert.deepEqual(await state(a), sa, 'SECOND_ORDER_A_UNCHANGED');
  assert.deepEqual(await state(b), sb, 'SECOND_ORDER_B_UNCHANGED');
  const cloneDir = resolve(root, 'clone'); cpSync(a.dir, cloneDir, { recursive: true });
  const clone = await start(cloneDir, 0);
  const cloneEdit = await edit(clone, [{ item: 'clone-⚠' }]);
  const originalEdit = await edit(a, [{ item: 'original-⚠' }]);
  check(cloneEdit.state.versions[0] === originalEdit.state.versions[0], 'CLONE_SAME_SEQUENCE');
  const cloneAdmission = await request(a, '/merge', { records: cloneEdit.records });
  assert.deepEqual(cloneAdmission.verdicts, ['collision'], 'CLONE_RECORD_COLLISION');
  console.log(`SECOND_ORDER PASS records=${count} clone-start=allowed clone-sequence=reused clone-merge=collision`);
  console.log(`PASS records=${count} elements=${sa.elements.length} versions=${sa.versions.join(',')}`);
} catch (error) { console.error(`FAIL ${error.message}`); process.exitCode = 1; }
finally { for (const child of live) child.kill('SIGKILL'); }
