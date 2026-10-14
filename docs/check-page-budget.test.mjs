import { test } from 'node:test';
import assert from 'node:assert/strict';
import { randomBytes } from 'node:crypto';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const fixture = (html, js) => {
  const root = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'smdoc-budget-'));
  mkdirSync(join(root, 'article'));
  writeFileSync(join(root, 'article', 'index.html'), html);
  if (js) writeFileSync(join(root, 'big.js'), js);
  return root;
};
const check = root => spawnSync(process.execPath, ['docs/check-page-budget.mjs', '--root', root], { cwd: new URL('..', import.meta.url), encoding: 'utf8', env: process.env });

test('planted 120 KiB compressed JS makes the page budget RED', () => {
  const js = `window.payload = '${randomBytes(120 * 1024).toString('base64')}';`;
  const root = fixture('<!doctype html><html lang="en"><title>Fixture</title><script src="/safemesh/big.js"></script><body><main>Article</main></body></html>', js);
  const result = check(root);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /article\/.*JS.*KiB.*100 KiB/s);
});

test('planted WASM request at load makes an ordinary article RED', () => {
  const root = fixture('<!doctype html><html lang="en"><title>Fixture</title><script>fetch("/safemesh/demo.wasm")</script><body><main>Article</main></body></html>');
  writeFileSync(join(root, 'demo.wasm'), Buffer.from('0061736d01000000', 'hex'));
  const result = check(root);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /article\/.*WASM.*demo\.wasm/s);
});

test('generated Rust reference uses its dated class while the same guide weight stays RED', () => {
  const content = randomBytes(500 * 1024).toString('base64');
  const html = `<!doctype html><html lang="en"><title>Fixture</title><body><main>${content}</main></body></html>`;
  const guide = fixture(html);
  const guideResult = check(guide);
  assert.equal(guideResult.status, 1, guideResult.stderr);
  assert.match(guideResult.stdout, /article\/.*350 KiB/s);
  const reference = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'smdoc-reference-'));
  mkdirSync(join(reference, 'reference', 'rust'), { recursive: true });
  writeFileSync(join(reference, 'reference', 'rust', 'index.html'), html);
  const referenceResult = check(reference);
  assert.equal(referenceResult.status, 0, referenceResult.stderr);
  assert.match(referenceResult.stdout, /reference\/rust\/.*1049 KiB.*2026-10-14/s);
});
