import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = join(dirname(fileURLToPath(import.meta.url)), '..');
const checker = join(repository, 'docs/check-claims-evidence.mjs');
const durable = 'rust/crates/safemesh-crdt/src/durable_tests.rs';
const local = 'rust/crates/safemesh-crdt/src/local.rs';
const cold = 'rust/crates/safemesh-crdt/examples/cold_chain_kill_test.rs';

function fixture(change = () => {}) {
  const root = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'smclaims-'));
  const claims = readFileSync(join(repository, 'CLAIMS.md'), 'utf8');
  writeFileSync(join(root, 'CLAIMS.md'), claims);
  for (const match of claims.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
    const path = match[1].split('#', 1)[0];
    if (!/\.(?:rs|py|mjs|ts|lean)$/.test(path)) continue;
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(repository, path), target);
  }
  change(root);
  return spawnSync(process.execPath, [checker, '--root', root], { encoding: 'utf8' });
}

function replaceClaims(root, before, after) {
  const file = join(root, 'CLAIMS.md');
  const text = readFileSync(file, 'utf8');
  assert.ok(text.includes(before), `missing fixture text: ${before}`);
  writeFileSync(file, text.replace(before, after));
}

test('T1 head copy passes with all source evidence', () => {
  const result = fixture();
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /CLAIMS EVIDENCE LINKS 23, NAMES 52, MISSING 0/);
});

test('T2 a link pointed back to local.rs names the moved test', () => {
  const result = fixture(root => replaceClaims(root, `](${durable})`, `](${local})`));
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /restart_invalid_history/);
});

test('T3 a typo mentioned only in a comment is not a definition', () => {
  const typo = 'restart_invalid_history_typo';
  const result = fixture(root => {
    replaceClaims(root, 'restart_invalid_history`]', `${typo}\`]`);
    const file = join(root, durable);
    writeFileSync(file, readFileSync(file, 'utf8') + `\n// fn ${typo}\n`);
  });
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /restart_invalid_history_typo/);
});

test('T4 a missing target file fails', () => {
  const result = fixture(root => replaceClaims(root, `](${durable})`, '](rust/crates/safemesh-crdt/src/absent.rs)'));
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /absent\.rs: restart_invalid_history \(target missing\)/);
});

test('T5 the example name passes by matching its file stem', () => {
  const result = fixture(root => writeFileSync(join(root, cold), '// No function declaration here.\n'));
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /MISSING 0/);
});

test('T6 silence fails the source-name floor', () => {
  const result = fixture(root => {
    const file = join(root, 'CLAIMS.md');
    writeFileSync(file, readFileSync(file, 'utf8').replace(/\]\([^)]*\.(?:rs|py|mjs|ts|lean)(?:#[^)]*)?\)/g, '](evidence.md)'));
  });
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /only 0 source evidence names \(minimum 40\)/);
});
