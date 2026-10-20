import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const repo = join(dirname(fileURLToPath(import.meta.url)), '..');
const checker = join(repo, 'scripts/check-python-matrix-claims.mjs');
const workflow = '.github/workflows/ci.yml';
const readme = 'README.md';
const persist = 'docs/src/content/docs/persist-and-restart.md';
const using = 'docs/src/content/docs/using-safemesh.md';
const crate = 'rust/crates/safemesh-python/README.md';
const paths = [workflow, readme, persist, using, crate];

function withTree(verify) {
  const root = mkdtempSync(join(process.env.TMPDIR ?? repo, 'sm-python-matrix-'));
  try {
    for (const path of paths) {
      const target = join(root, path);
      mkdirSync(dirname(target), { recursive: true });
      writeFileSync(target, readFileSync(join(repo, path)));
    }
    const edit = (path, before, after) => {
      const target = join(root, path);
      const source = readFileSync(target, 'utf8');
      assert.equal(source.includes(before), true, `missing plant text in ${path}`);
      writeFileSync(target, source.replace(before, after));
    };
    const run = () => spawnSync(process.execPath, [checker, '--root', root], { encoding: 'utf8' });
    verify({ edit, run });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
function failureLines(result) {
  return result.stderr.trim().split('\n').filter(line => line.startsWith('FAIL '));
}

test('CONTROL: seven versions and five surfaces pass', () => withTree(({ run }) => {
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /PASS python matrix 3\.8\.\.3\.14 \(7 versions\), 5 surfaces/);
}));
test('T1: a removed matrix floor fails every surface by path', () => withTree(({ edit, run }) => {
  edit(workflow, '"3.8", "3.9"', '"3.9"');
  const result = run();
  assert.equal(result.status, 1);
  const lines = failureLines(result);
  assert.equal(lines.length, 5, result.stderr);
  assert.deepEqual(lines.map(line => line.match(/^FAIL ([^:]+):/)[1]), [readme, persist, using, crate, crate]);
}));
test('T2: a shorter en dash claim fails only README', () => withTree(({ edit, run }) => {
  edit(readme, 'CPython 3.8–3.14', 'CPython 3.8–3.13');
  const result = run();
  assert.equal(result.status, 1);
  assert.equal(failureLines(result).length, 1, result.stderr);
  assert.match(result.stderr, /^FAIL README\.md:126:/);
}));
test('T3: the release-assets setup-python version is not the matrix', () => withTree(({ edit, run }) => {
  edit(workflow, "python-version: '3.11'", "python-version: '3.15'");
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /5 surfaces/);
}));
test('T4: a missing anchor fails by name', () => withTree(({ edit, run }) => {
  edit(using, 'The Python CI matrix builds and installs a local wheel', 'A local wheel is built and installed');
  const result = run();
  assert.equal(result.status, 1);
  assert.deepEqual(failureLines(result), [`FAIL ${using}: anchor not found: The Python CI matrix builds and installs a local wheel`]);
}));
test('a range with to is parsed', () => withTree(({ edit, run }) => {
  edit(readme, 'CPython 3.8–3.14', 'CPython 3.8 to 3.14');
  const result = run();
  assert.equal(result.status, 0, result.stderr);
}));
test('a different named runner fails by path', () => withTree(({ edit, run }) => {
  edit(readme, 'on `ubuntu-latest` with CPython', 'on `windows-latest` with CPython');
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /^FAIL README\.md:126: runner windows-latest differs from python job ubuntu-latest/m);
}));
