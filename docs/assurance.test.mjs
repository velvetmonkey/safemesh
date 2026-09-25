// Real Astro regressions, adapted from the supplied tooling review probes.
// All source mutations and builds happen in a disposable clone.
import fs from 'node:fs';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawn, spawnSync, execFileSync } from 'node:child_process';
import assert from 'node:assert/strict';
import { test, after } from 'node:test';
import { parse, serialize } from 'parse5';
const live = fileURLToPath(new URL('../', import.meta.url));
// Astro resolves symlinked dependencies against the fixture root during builds.
const scratch = fs.mkdtempSync(join(dirname(live), 'assurance-'));
const cleanup = () => fs.rmSync(scratch, { recursive: true, force: true });
// node --test can outlive npm on SIGINT, leaving its worker in a separate
// process group. Watch the runner as well as the worker's pipe.
const watcherScript = `
const fs = require('node:fs');
const [scratch, done, runnerPid, workerPid] = process.argv.slice(1);
function finish() {
  if (fs.existsSync(done)) fs.rmSync(done);
  else fs.rmSync(scratch, { recursive: true, force: true });
  process.exit();
}
process.stdin.resume();
process.stdin.on('end', finish);
setInterval(() => {
  try { process.kill(Number(runnerPid), 0); }
  catch (error) {
    if (error.code !== 'ESRCH') return;
    try { process.kill(Number(workerPid), 'SIGTERM'); } catch {}
    finish();
  }
}, 10);
`;
const watcher = spawn(process.execPath, ['-e', watcherScript,
  scratch, `${scratch}.cleaned`, String(process.ppid), String(process.pid)],
  { detached: true, stdio: ['pipe', 'ignore', 'ignore'] });
watcher.stdin.unref();
watcher.unref();
process.once('SIGINT', () => { cleanup(); process.exit(130); });
// node --test may forward an interrupt to its worker as SIGTERM.
process.once('SIGTERM', () => { cleanup(); process.exit(143); });
const root = join(scratch, 'repo'), out = join(scratch, 'results');
execFileSync('git', ['clone', '--quiet', '--shared', live, root]);
const docs = join(root, 'docs');
for (const name of ['assurance.mjs', 'check-assurance.mjs', 'astro.config.mjs']) {
  fs.copyFileSync(join(live, 'docs', name), join(docs, name));
}
fs.symlinkSync(join(live, 'docs/node_modules'), join(docs, 'node_modules'), 'junction');
fs.mkdirSync(out);
console.log(`Regression artifacts: ${scratch}`);
const { default: assurance, pages, revision, sourceTree } = await import(pathToFileURL(join(docs, 'assurance.mjs')));
const source=resolve(root,'WHAT-IS-PROVEN.md'),control=resolve(docs,'src/content/docs/concepts.md');
const original=fs.readFileSync(source,'utf8'),originalControl=fs.readFileSync(control,'utf8');
const config = fs.readFileSync(join(docs, 'astro.config.mjs'), 'utf8');
const env={...process.env,ASTRO_TELEMETRY_DISABLED:'1'};
const run=args=>spawnSync(process.execPath,args,{cwd:docs,env,encoding:'utf8',maxBuffer:5e6});
function nodes(n,fn,result=[]){if(fn(n))result.push(n);for(const c of n.childNodes??[])nodes(c,fn,result);return result;}
function attr(n,k){return n.attrs?.find(a=>a.name===k)?.value;}
const result=[];
let started = 0;
const caseTest = (name, body) => test(name, () => { started++; return body(); });
// Each route has two separator probes, plus two standalone probes and the
// Cartesian product of the platform and mutation definitions below.
let expectedCases;
after(() => {
  try {
    console.log(`Results: ${JSON.stringify(result)}`);
    assert.equal(started, expectedCases, `assurance cases run: ${started}/${expectedCases}`);
    console.log(`Assurance matrix complete: ${started}/${expectedCases}`);
  } finally {
    cleanup();
    // Tell the watcher the after hook handled normal completion.
    fs.writeFileSync(`${scratch}.cleaned`, '');
  }
});
for (const [route, source] of Object.entries(pages)) {
  for (const separator of ['/', String.fromCharCode(92)]) {
    caseTest(`D01 ${route} separator ${JSON.stringify(separator)}`, () => {
      const tree = {type:'root', children:[{type:'html',value:`<!-- assurance-source: ${source} -->`}]};
      assurance()(tree, {path: `/checkout/docs/src/content/docs/${route}.md`.replaceAll('/', separator)});
      assert.ok(tree.children.some(n => n.value?.includes('data-assurance-source=')), 'canonical article must replace marker');
    });
  }
}
caseTest('unrelated pages pass through, misplaced source markers fail loudly', () => {
  const tree = {type:'root',children:[{type:'paragraph',children:[]}]};
  assurance()(tree, {path:'/checkout/docs/src/content/docs/concepts.md'});
  assert.equal(tree.children.length, 1);
  assert.throws(() => assurance()({type:'root',children:[{type:'html',value:'<!-- assurance-source: CLAIMS.md -->'}]}, {path:'/wrong/claims.md'}), /assurance/);
});
caseTest('nested source resources resolve against their canonical directory', () => {
  const relative = 'docs/resource-probe.md';
  fs.writeFileSync(join(root, relative), '# Resources\n\n[link][shared] ![image][shared]\n\n[shared]: ../assets/safemesh-logo.png\n\n[fragment](#keep) [external](https://example.com/a)\n');
  const tree = sourceTree(relative, revision());
  const found = [];
  function visit(n) { if (n.url) found.push([n.type, n.url]); for (const c of n.children ?? []) visit(c); }
  visit(tree);
  assert.ok(found.some(([type, url]) => type === 'definition' && url === `https://github.com/velvetmonkey/safemesh/blob/${revision()}/assets/safemesh-logo.png`));
  assert.ok(found.some(([type, url]) => type === 'image' && url === `https://raw.githubusercontent.com/velvetmonkey/safemesh/${revision()}/assets/safemesh-logo.png`));
  assert.ok(found.some(([, url]) => url === '#keep'));
  assert.ok(found.some(([, url]) => url === 'https://example.com/a'));
});
const cases={
 references:'\n\n[Reference link probe][audit-reference]\n\n[audit-reference]: rust/crates/safemesh-crdt/src/lib.rs\n',
 image:'\n\n![SafeMesh logo probe](assets/safemesh-logo.png)\n',
 gfm:'\n\n| Review column | Value |\n| --- | --- |\n| Alpha | Beta |\n\n~~Removed sentence probe~~\n',
 html:'\n\n<details><summary>Review explanation</summary><p>Details probe visible words.</p></details>\n',
 whitespace:'\n\n```python\nif True:\n    print(42)\n```\n',
};
const matrix = {
  platforms: ['POSIX', 'Windows'],
  names: ['unchanged', 'references', 'image', 'gfm', 'html', 'whitespace'],
};
assert.deepEqual(Object.keys(cases), matrix.names.slice(1), 'assurance mutation definitions must be complete');
expectedCases = Object.keys(pages).length * 2 + 2 + matrix.platforms.length * matrix.names.length;
for (const platform of matrix.platforms) {
 for (const name of matrix.names) {
  const extra = name === 'unchanged' ? '' : cases[name];
  caseTest(`${platform} ${name}`, () => {
  try {
    // Feed Windows paths into the actual plugin during a full Astro build.
    // Do not normalize them in the harness: the product must do that itself.
    fs.writeFileSync(join(docs, 'astro.config.mjs'), platform === 'POSIX' ? config : config.replace('[assurance]', '[() => (tree, file) => { file.path = String(file.path).replaceAll("/", String.fromCharCode(92)); }, assurance]'));
  fs.writeFileSync(source,original+extra);
  fs.writeFileSync(control,originalControl+(name==='gfm'?extra:''));
  const build=run(['node_modules/astro/bin/astro.mjs','build']);
  const row={name,build:build.status};
  fs.mkdirSync(resolve(out,'docs-matrix-logs'),{recursive:true});
  fs.writeFileSync(resolve(out,'docs-matrix-logs',name+'.build.txt'),build.stdout+build.stderr);
  if(build.status===0){
   const file=resolve(docs,'dist/proof/index.html'),html=fs.readFileSync(file,'utf8');
   const document=parse(html),article=nodes(document,n=>attr(n,'data-assurance-source')==='WHAT-IS-PROVEN.md')[0];
   if(!article)throw Error('Fixture platform path control did not render the source');
   if(name==='references')row.href=nodes(article,n=>n.tagName==='a').map(n=>attr(n,'href')).filter(h=>h?.includes('rust/crates/safemesh-crdt/src/lib.rs'));
   if(name==='image') row.src=nodes(article,n=>n.tagName==='img').map(n=>attr(n,'src'));
   if(name==='gfm'){
    row.sourceTables=nodes(article,n=>n.tagName==='table').length;
    row.sourceStrikes=nodes(article,n=>n.tagName==='del').length;
    row.literalStrike=html.includes('~~Removed sentence probe~~');
    const ordinary=parse(fs.readFileSync(resolve(docs,'dist/concepts/index.html'),'utf8'));
    row.ordinaryTables=nodes(ordinary,n=>n.tagName==='table').length;
    row.ordinaryStrikes=nodes(ordinary,n=>n.tagName==='del').length;
   }
   if(name==='html')row.details=nodes(article,n=>n.tagName==='details').length;
   const check=run(['check-assurance.mjs']);row.check=check.status;row.checkOutput=(check.stdout+check.stderr).trim().slice(-1800);
   if(name==='html' || name==='image') {
    const mutated = name === 'html' ? html.replace('Details probe visible words.', 'Changed visible words.') : html.replace('assets/safemesh-logo.png', 'assets/missing.png');
    assert.notEqual(mutated, html);
    fs.writeFileSync(file, mutated);
    row.mutatedCheck=run(['check-assurance.mjs']).status;
    fs.writeFileSync(file, html);
   }
   if(name==='whitespace'){
    const text=nodes(article,n=>n.nodeName==='#text'&&n.value==='    ')[0];
    if(!text)throw Error('Indented code fixture not found');
    text.value='';
    fs.writeFileSync(file,serialize(document).replace('data-code="if True:    print(42)"','data-code="if True:print(42)"'));
    const altered=run(['check-assurance.mjs']);row.changedCodeCheck=altered.status;row.changedOutput=(altered.stdout+altered.stderr).trim().slice(-1200);
    const tokenDocument=parse(html);
    const tokenArticle=nodes(tokenDocument,n=>attr(n,'data-assurance-source')==='WHAT-IS-PROVEN.md')[0];
    const printed=nodes(tokenArticle,n=>n.nodeName==='#text'&&n.value==='print')[0];
    if(!printed)throw Error('Python print token not found');
    printed.value='input';fs.writeFileSync(file,serialize(tokenDocument));
    const changedWord=run(['check-assurance.mjs']);row.changedWordCheck=changedWord.status;
   }
  }else row.error=(build.stdout+build.stderr).trim().slice(-2000);

  result.push({platform, ...row});
  fs.writeFileSync(join(out, `${platform}-${name}.json`), JSON.stringify(row,null,2));
  assert.equal(row.build, 0, row.error);
  assert.equal(row.check, 0, row.checkOutput);
  if (name === 'unchanged') {
    const tree = sourceTree('WHAT-IS-PROVEN.md', revision());
    const links = [];
    function visit(n) { if (n.type === 'link') links.push(n.url); for (const c of n.children ?? []) visit(c); }
    visit(tree);
    assert.ok(links.includes(`https://github.com/velvetmonkey/safemesh/blob/${revision()}/README.md#build-toolchain`), 'inline link remains commit-pinned');
  }
  if (name === 'references') assert.ok(row.href.length > 0 && row.href.every(h => h === `https://github.com/velvetmonkey/safemesh/blob/${revision()}/rust/crates/safemesh-crdt/src/lib.rs`), JSON.stringify(row.href));
  if (name === 'image') assert.ok(row.src.includes(`https://raw.githubusercontent.com/velvetmonkey/safemesh/${revision()}/assets/safemesh-logo.png`), JSON.stringify(row.src));
  if (name === 'gfm') {
    assert.ok(row.ordinaryTables > 0 && row.ordinaryStrikes > 0, 'ordinary Concepts GFM control');
    assert.ok(row.sourceTables > 0 && row.sourceStrikes > 0 && !row.literalStrike, JSON.stringify(row));
  }
  if (name === 'html') assert.equal(row.details, 1);
  if (name === 'html' || name === 'image') assert.equal(row.mutatedCheck, 1, 'rendered HTML/image drift must fail');
  if (name === 'whitespace') {
    assert.equal(row.changedWordCheck, 1, 'changed token control');
    assert.equal(row.changedCodeCheck, 1, 'indentation mutation must be rejected');
  }
  } finally {
    fs.writeFileSync(source, original);
    fs.writeFileSync(control, originalControl);
    fs.writeFileSync(join(docs, 'astro.config.mjs'), config);
  }
 });
}
}
