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
  wasm: 'demos/wasm-checklist/README.md',
  python: 'demos/two-app-inventory/README.md',
  requirements: 'demos/two-app-inventory/requirements.txt',
  crdt: 'docs/src/content/docs/persist-and-restart.md',
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
    wasm: 'npm install safemesh-wasm-9.8.7.tgz\n',
    python: 'safemesh-python==9.8.7\n',
    requirements: 'safemesh-python==9.8.7\n',
    crdt: 'currently records `9.8.7` as an unpublished crate version\n',
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
  assert.match(result.stdout, /PASS 4 files, 4 version names checked/);
}));
test('W-OLD exits 1 for the wasm README line', () => fails({ ...clean(), wasm: 'npm install safemesh-wasm-0.1.0.tgz\n' }, paths.wasm));
test('W-NONE exits 1 without a tarball name', () => fails({ ...clean(), wasm: 'npm install local-package\n' }, paths.wasm));
test('P-README exits 1 for the Python README line', () => fails({ ...clean(), python: 'safemesh-python==0.1.0\n' }, paths.python));
test('P-REQ exits 1 for requirements.txt:1', () => fails({ ...clean(), requirements: 'safemesh-python==0.1.0\n' }, paths.requirements));
test('C-OLD exits 1 for the docs sentence', () => fails({ ...clean(), crdt: 'currently records `0.1.0` as an unpublished crate version\n' }, paths.crdt));
test('BUMP exits 1 when only the wasm manifest changes', () => withTree({
  ...clean(),
  wasm: 'npm install safemesh-wasm-0.2.0.tgz\n',
  wasmManifest: '[package]\nversion = "0.2.0"\n',
}, (run, root) => {
  assert.equal(run().status, 0);
  writeFileSync(join(root, paths.wasmManifest), '[package]\nversion = "9.8.8"\n');
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /FAIL demos\/wasm-checklist\/README.md:\d+:/);
}));
