// Run against a wasm-pack --target nodejs package:
// node safemesh-wasm-boundary-repros.mjs /absolute/path/to/pkg-node
// Returns exit 1 if the reviewed numeric/reporting issues are present.
// Source: outside SafeMesh review, forwarded by Ben on 2026-09-11 at 21:29. Saved verbatim.
import assert from 'node:assert/strict';
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
console.log(`Completed: ${failures} failing boundary cases`);
process.exitCode=failures?1:0;
