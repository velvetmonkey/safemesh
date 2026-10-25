import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const checker = join(repo, 'scripts/check-doc-versions.mjs');
const paths = {
  readme: 'README.md',
  wasm: 'demos/wasm-checklist/README.md',
  python: 'demos/two-app-inventory/README.md',
  requirements: 'demos/two-app-inventory/requirements.txt',
  crdt: 'docs/src/content/docs/persist-and-restart.md',
  crdtReadme: 'rust/crates/safemesh-crdt/README.md',
  pythonReadme: 'rust/crates/safemesh-python/README.md',
  wasmReadme: 'rust/crates/safemesh-wasm/README.md',
  ffiReadme: 'rust/crates/safemesh-ffi/README.md',
  demosReadme: 'demos/README.md',
  coldChainReadme: 'demos/python-cold-chain/README.md',
  wasmManifest: 'rust/crates/safemesh-wasm/Cargo.toml',
  pythonManifest: 'rust/crates/safemesh-python/pyproject.toml',
  crdtManifest: 'rust/crates/safemesh-crdt/Cargo.toml',
};

function withTree(files, verify) {
  const root = mkdtempSync(join(tmpdir(), 'smdocsversion-'));
  try {
    for (const [key, source] of Object.entries(files)) {
      const path = join(root, paths[key]);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, source);
    }
    const run = () => spawnSync(process.execPath, [checker, '--root', root], { encoding: 'utf8' });
    verify(run, root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

function clean() {
  return {
    readme: 'cargo add safemesh-crdt@9.8.7\nnpm install safemesh-wasm@9.8.7\npip install safemesh-python==9.8.7\n',
    wasm: 'npm install safemesh-wasm-9.8.7.tgz\n',
    python: 'safemesh-python==9.8.7\n',
    requirements: 'safemesh-python==9.8.7\n',
    crdt: 'currently records `9.8.7` as an unpublished crate version\n',
    crdtReadme: 'No release-line label here.\n',
    pythonReadme: 'No release-line label here.\n',
    wasmReadme: 'No release-line label here.\n',
    ffiReadme: 'No release-line label here.\n',
    demosReadme: 'No release-line label here.\n',
    coldChainReadme: 'No release-line label here.\n',
    wasmManifest: '[package]\nversion = "9.8.7"\n',
    pythonManifest: '[project]\nversion = "9.8.7"\n',
    crdtManifest: '[package]\nversion = "9.8.7"\n',
  };
}

function fails(files, path) {
  withTree(files, run => {
    const result = run();
    assert.equal(result.status, 1);
    assert.match(result.stderr, new RegExp(`FAIL ${path.replaceAll('.', '\\.').replaceAll('/', '\\/')}:\\d+:`));
  });
}

test('CLEAN exits 0 and reports four files', () => withTree(clean(), run => {
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /PASS 11 files, 7 version names checked/);
}));
test('L-OLD exits 1 for an old demo release-line label', () => fails({ ...clean(), demosReadme: 'v' + '0.1 showcase demos\n' }, paths.demosReadme));
test('L-NONE exits 0 when the seven README files have no release-line label', () => withTree(clean(), run => {
  const result = run();
  assert.equal(result.status, 0, result.stderr);
}));
test('L-README exits 1 for an old root README release-line label', () => fails({ ...clean(), readme: `${clean().readme}v${'0.1'} release line\n` }, paths.readme));
test('R-CRDT exits 1 for the README Rust pin', () => fails({ ...clean(), readme: 'cargo add safemesh-crdt@0.1.0\nnpm install safemesh-wasm@9.8.7\npip install safemesh-python==9.8.7\n' }, paths.readme));
test('R-WASM exits 1 for the README WASM pin', () => fails({ ...clean(), readme: 'cargo add safemesh-crdt@9.8.7\nnpm install safemesh-wasm@0.1.0\npip install safemesh-python==9.8.7\n' }, paths.readme));
test('R-PY exits 1 for the README Python pin', () => fails({ ...clean(), readme: 'cargo add safemesh-crdt@9.8.7\nnpm install safemesh-wasm@9.8.7\npip install safemesh-python==0.1.0\n' }, paths.readme));
test('W-OLD exits 1 for the wasm README line', () => fails({ ...clean(), wasm: 'npm install safemesh-wasm-0.1.0.tgz\n' }, paths.wasm));
test('W-NONE exits 1 without a tarball name', () => fails({ ...clean(), wasm: 'npm install local-package\n' }, paths.wasm));
test('P-README exits 1 for the Python README line', () => fails({ ...clean(), python: 'safemesh-python==0.1.0\n' }, paths.python));
test('P-REQ exits 1 for requirements.txt:1', () => fails({ ...clean(), requirements: 'safemesh-python==0.1.0\n' }, paths.requirements));
test('C-OLD exits 1 for the docs sentence', () => fails({ ...clean(), crdt: 'currently records `0.1.0` as an unpublished crate version\n' }, paths.crdt));
test('BUMP exits 1 when only the wasm manifest changes', () => withTree({
  ...clean(),
  readme: 'cargo add safemesh-crdt@9.8.7\nnpm install safemesh-wasm@0.2.0\npip install safemesh-python==9.8.7\n',
  wasm: 'npm install safemesh-wasm-0.2.0.tgz\n',
  wasmManifest: '[package]\nversion = "0.2.0"\n',
}, (run, root) => {
  assert.equal(run().status, 0);
  writeFileSync(join(root, paths.wasmManifest), '[package]\nversion = "9.8.8"\n');
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /FAIL demos\/wasm-checklist\/README.md:\d+:/);
}));

test('LINE reports the exact wasm README failure line', () => withTree({
  ...clean(),
  wasm: 'intro\nnpm install safemesh-wasm-9.8.7.tgz\nnpm install safemesh-wasm-0.1.0.tgz\n',
}, run => {
  const result = run();
  assert.equal(result.status, 1);
  assert.equal(result.stderr, 'FAIL demos/wasm-checklist/README.md:3: found 0.1.0 expected 9.8.7 (rust/crates/safemesh-wasm/Cargo.toml)\n');
}));

test('NO-VERSION refuses a wasm manifest without a version', () => withTree({
  ...clean(),
  wasmManifest: '[package]\nname = "x"\n',
}, run => {
  const result = run();
  assert.equal(result.status, 1);
  assert.equal(result.stderr, 'FAIL rust/crates/safemesh-wasm/Cargo.toml:0: missing version in [package]\n');
}));

test('WRONG-TABLE refuses a Python manifest without a project version', () => withTree({
  ...clean(),
  pythonManifest: '[package]\nversion = "9.8.7"\n',
}, run => {
  const result = run();
  assert.equal(result.status, 1);
  assert.equal(result.stderr, 'FAIL rust/crates/safemesh-python/pyproject.toml:0: missing version in [project]\n');
}));
