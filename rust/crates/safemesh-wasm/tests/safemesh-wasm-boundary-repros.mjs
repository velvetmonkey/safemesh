// Run against a wasm-pack --target nodejs package:
// node safemesh-wasm-boundary-repros.mjs /absolute/path/to/pkg-node
// Returns exit 1 if a numeric/reporting or persistence regression is present.
// Original numeric/reporting probes: outside SafeMesh review, forwarded by Ben on 2026-09-11 at 21:29.
// Extended with the SM-024 persist regression.
import assert from 'node:assert/strict';
import {spawnSync} from 'node:child_process';
import {mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {createRequire} from 'node:module';
import {join,resolve} from 'node:path';
if (!process.argv[2]) throw Error('Provide the generated Node package directory');
const require=createRequire(import.meta.url);
const {SafeMeshGCounter:Counter,SafeMeshGCounterReplica:Replica,SafeMeshStringOrSetReplica:OrSetReplica}=require(join(resolve(process.argv[2]),'safemesh_wasm.js'));
let failures=0;
const check=(name,fn)=>{try{fn();console.log(`PASS ${name}`);}catch(error){failures++;console.log(`FAIL ${name}: ${error.message}`);}};
check('ordinary out-of-range coordinate refuses without mutation',()=>{
  const c=new Counter(2);
  try{assert.throws(()=>c.tryApplyBump(2,7n));assert.equal(c.value(),0n);}finally{c.free();}
});
for(const coordinate of [NaN,Infinity,0.5,2**32]) check(`coordinate ${coordinate} refuses without mutation`,()=>{
  const c=new Counter(2);
  try{let refused=false;try{c.tryApplyBump(coordinate,7n);}catch{refused=true;}assert.equal(c.value(),0n,'invalid coordinate changed state');assert.ok(refused,'invalid coordinate did not throw');}finally{c.free();}
});
for(const tally of [-1n,1n<<64n]) check(`tally ${tally} refuses without changing state or log`,()=>{
  const r=new Replica(0n,2);
  try{const before=r.logBytes();let refused=false;try{r.appendBump(0,tally);}catch{refused=true;}assert.deepEqual(r.logBytes(),before,'invalid tally changed log');assert.equal(r.value(),0n);assert.ok(refused,'invalid tally did not throw');}finally{r.free();}
});
const u32=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n>>>0);return b;};
const crc32=bytes=>{let crc=0xffffffff;for(const byte of bytes){crc^=byte;for(let i=0;i<8;i++)crc=(crc>>>1)^(0xedb88320 & -(crc&1));}return (~crc)>>>0;};
check('importIdentity record budget refuses before claim and permits append on retry',()=>{
  const writer=OrSetReplica.createAllocated(2n,0n);
  let saved;
  try {
    for(const element of ['first','second','third']) writer.appendAllocatedAdd(element);
    saved=writer.exportIdentity();
  } finally { writer.free(); }
  assert.throws(()=>OrSetReplica.importIdentity(saved,2),error=>
    error.code===1 && error.message==='failed to decode event log: RecordLimitExceeded: 2');
  const restored=OrSetReplica.importIdentity(saved,3);
  try {
    restored.appendAllocatedAdd('fourth');
    assert.deepEqual(restored.elements(),['first','fourth','second','third']);
  } finally { restored.free(); }
});
function repeatFirst(frame){
  const body=Buffer.from(frame).subarray(9,-4);
  const arity=8+body.readUInt32LE(4);
  const countOffset=arity+1+(body[arity]===1?8:0);
  const count=body.readUInt32LE(countOffset);
  const records=body.subarray(countOffset+4);
  const first=records.subarray(0,4+records.readUInt32LE(0));
  const next=Buffer.concat([body.subarray(0,countOffset),u32(count+1),first,records]);
  const crcInput=Buffer.concat([u32(next.length),u32(~next.length),next]);
  return Buffer.concat([Buffer.from([3]),crcInput,u32(crc32(crcInput))]);
}
for(const [name,make,append] of [
  ['counter',()=>new Replica(0n,2),r=>r.appendBump(0,1n)],
  ['UTF-8 OR-Set',()=>new OrSetReplica(0n),r=>r.appendAdd('water',2n)],
]) check(`${name}: frame A,A,B has one outcome per input record`,()=>{
  const sender=make(),receiver=make();
  try{append(sender);append(sender);const result=receiver.mergeLogBytes(repeatFirst(sender.logBytes()));assert.deepEqual(result,['accepted','duplicate','accepted']);}finally{sender.free();receiver.free();}
});
check('persist refuses existing histories and partial stores without mutation', () => {
  const root = mkdtempSync(join(tmpdir(), 'safemesh-persist-'));
  const example = fileURLToPath(new URL('../examples/node-persist.mjs', import.meta.url));
  const files = ['left-counter.log', 'left-set.identity', 'right-counter.log', 'right-set.identity'];
  function run(dir, step, status = 0) {
    const result = spawnSync(process.execPath, [example, resolve(process.argv[2]), dir, step], {encoding: 'utf8'});
    assert.equal(result.status, status, result.stdout + result.stderr);
    return result;
  }
  function snapshot(dir) {
    return Object.fromEntries(readdirSync(dir).sort().map(name => [name, readFileSync(join(dir, name))]));
  }
  try {
    const history = join(root, 'history');
    assert.match(run(history, 'persist').stdout, /counter=12/);
    const initial = snapshot(history);
    run(history, 'partition');
    assert.match(run(history, 'reconcile').stdout, /counter=19/);
    const before = snapshot(history);
    const refused = run(history, 'persist', 2);
    assert.match(refused.stderr, /PERSIST REFUSED/);
    assert.match(refused.stderr, /different, empty directory/);
    for (const name of files) assert.ok(refused.stderr.includes(join(history, name)));
    assert.deepEqual(snapshot(history), before);
    assert.match(run(history, 'restore').stdout, /counter=19/);

    const empty = join(root, 'empty');
    mkdirSync(empty);
    run(empty, 'persist');
    assert.deepEqual(snapshot(empty), initial, 'fresh empty directory keeps the original bytes');
    for (const name of files) {
      const partial = join(root, name);
      mkdirSync(partial);
      writeFileSync(join(partial, name), 'existing history');
      const before = snapshot(partial);
      assert.ok(run(partial, 'persist', 2).stderr.includes(join(partial, name)));
      assert.deepEqual(snapshot(partial), before, 'no new or overwritten files in a partial store');
    }
    const dangling = join(root, 'dangling');
    mkdirSync(dangling);
    symlinkSync(join(root, 'missing-target'), join(dangling, files[0]));
    assert.ok(run(dangling, 'persist', 2).stderr.includes(files[0]));
    assert.deepEqual(readdirSync(dangling), [files[0]]);
  } finally {
    rmSync(root, {recursive: true, force: true});
  }
});
check('persisted partition repeats allocate fresh adds and retain vaccine after repair',()=>{
  const directory=mkdtempSync(join(tmpdir(),'safemesh-partition-'));
  const example=fileURLToPath(new URL('../examples/node-persist.mjs',import.meta.url));
  const phase=step=>{
    const result=spawnSync(process.execPath,[example,resolve(process.argv[2]),directory,step],{encoding:'utf8'});
    assert.equal(result.status,0,`${step}: ${result.stdout}\n${result.stderr}`);
  };
  const carrier=replica=>{
    const entries=replica.addEntries();
    try {
      return {
        adds:entries.map(entry=>[entry.element(),String(entry.token())]).sort(),
        tombstones:Array.from(replica.tombstones(),String).sort(),
      };
    } finally { entries.forEach(entry=>entry.free()); }
  };
  try {
    phase('persist');
    let previousTokens=new Set();
    for(let round=0;round<3;round++) {
      phase('partition');
      phase('reconcile');
      phase('restore'); // another process must recover the latest state
      const left=OrSetReplica.importIdentity(readFileSync(join(directory,'left-set.identity')));
      let right;
      try {
        right=OrSetReplica.importIdentity(readFileSync(join(directory,'right-set.identity')));
        assert.deepEqual(left.elements(),['gauze','insulin','vaccine']);
        assert.deepEqual(right.elements(),left.elements());
        const state=carrier(left);
        assert.deepEqual(state,carrier(right),'full add/tombstone carrier differs');
        assert.equal(state.adds.length,2+2*(round+1),'both partition adds must be fresh');
        assert.equal(new Set(state.adds.map(([,token])=>token)).size,state.adds.length);
        const tokens=state.adds.filter(([element])=>element==='vaccine').map(([,token])=>token);
        const removed=new Set(state.tombstones);
        const live=tokens.filter(token=>!removed.has(token));
        assert.deepEqual(Array.from(left.observedTokens('vaccine'),String).sort(),[...live].sort(),'observed tokens must exclude tombstones');
        assert.equal(live.length,1,'exactly one vaccine add must survive');
        assert(!previousTokens.has(live[0]),'repeat reused a previous vaccine token');
        assert.equal(tokens.length,round+2,'each phase must retain a new vaccine add');
        previousTokens=new Set(tokens);
        for(const replica of [left,right]) {
          const peer=replica===left?right:left;
          assert(replica.mergeLogBytes(peer.logBytes()).every(result=>result==='duplicate'));
        }
      } finally { left.free(); right?.free(); }
    }
  } finally { rmSync(directory,{recursive:true,force:true}); }
});
check('persist advice distinguishes complete history from every partial store', () => {
  const root = mkdtempSync(join(tmpdir(), 'safemesh-partial-advice-'));
  const example = fileURLToPath(new URL('../examples/node-persist.mjs', import.meta.url));
  const files = ['left-counter.log', 'left-set.identity', 'right-counter.log', 'right-set.identity'];
  const run = (dir, step, status) => {
    const result = spawnSync(process.execPath, [example, resolve(process.argv[2]), dir, step], {encoding: 'utf8'});
    assert.equal(result.status, status, result.stdout + result.stderr);
    return result;
  };
  const snapshot = dir => Object.fromEntries(readdirSync(dir).sort().map(name => [name, readFileSync(join(dir, name))]));
  try {
    const complete = join(root, 'complete');
    run(complete, 'persist', 0);
    const original = snapshot(complete);
    // Copy real persisted bytes, including the crash-realistic left-counter-only case.
    for (let mask = 1; mask < 15; mask++) {
      const partial = join(root, `partial-${mask}`);
      mkdirSync(partial);
      const existing = files.filter((_, index) => mask & (1 << index));
      const missing = files.filter(name => !existing.includes(name));
      for (const name of existing) writeFileSync(join(partial, name), original[name]);
      const before = snapshot(partial);
      const refused = run(partial, 'persist', 2);
      assert.match(refused.stderr, /Partial store: this walkthrough cannot restore an incomplete history/);
      assert.match(refused.stderr, /Move the files aside or choose a different, empty directory/);
      assert.ok(refused.stderr.includes(`existing store files: ${existing.map(name => join(partial, name)).join(', ')}`));
      assert.ok(refused.stderr.includes(`missing store files: ${missing.map(name => join(partial, name)).join(', ')}`));
      assert.doesNotMatch(refused.stderr, /use restore/i);
      assert.deepEqual(snapshot(partial), before, 'refusal changed the partial store');
      assert.match(run(partial, 'restore', 2).stderr, /error=missing/);
      assert.deepEqual(snapshot(partial), before, 'restore changed the partial store');
    }
    const refused = run(complete, 'persist', 2);
    assert.match(refused.stderr, /All four store paths exist; use restore to read a complete, valid history/);
    assert.deepEqual(snapshot(complete), original);
    assert.match(run(complete, 'restore', 0).stdout, /RESTORED=true/);
  } finally {
    rmSync(root, {recursive: true, force: true});
  }
});
console.log(`Completed: ${failures} failing boundary cases`);
process.exitCode=failures?1:0;
