import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

const root = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'smdoc-a11y-'));
mkdirSync(join(root, 'article'));
writeFileSync(join(root, 'article', 'index.html'), '<!doctype html><html lang="en"><title>Fixture</title><body><main><h1>Fixture</h1><img src="/safemesh/pixel.png"><button></button></main></body></html>');
writeFileSync(join(root, 'pixel.png'), Buffer.from('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000b49444154789c636000020000050001a5f645400000000049454e44ae426082', 'hex'));

test('planted image without alt and unnamed button make the accessibility check RED', () => {
  const result = spawnSync(process.execPath, ['docs/check-a11y.mjs', '--root', root], { cwd: new URL('..', import.meta.url), encoding: 'utf8', env: process.env });
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /article\/.*image-alt.*img/s);
  assert.match(result.stdout, /article\/.*button-name.*button/s);
});

test('per-rule ratchet permits the baseline and rejects only a rise', () => {
  const baseline = join(root, 'baseline.json');
  const data = { measuredOn: '2026-10-14', engine: 'axe-core@4.10.3', tags: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'], total: 2, counts: { 'button-name': 1, 'image-alt': 1 } };
  writeFileSync(baseline, JSON.stringify(data));
  const allowed = spawnSync(process.execPath, ['docs/check-a11y.mjs', '--root', root, '--baseline', baseline], { cwd: new URL('..', import.meta.url), encoding: 'utf8', env: process.env });
  assert.equal(allowed.status, 0, allowed.stderr);
  assert.match(allowed.stdout, /PASS button-name: 1\/1/);
  data.counts['button-name'] = 0;
  data.total = 1;
  writeFileSync(baseline, JSON.stringify(data));
  const raised = spawnSync(process.execPath, ['docs/check-a11y.mjs', '--root', root, '--baseline', baseline], { cwd: new URL('..', import.meta.url), encoding: 'utf8', env: process.env });
  assert.equal(raised.status, 1, raised.stderr);
  assert.match(raised.stdout, /FAIL button-name: 1\/0/);
  assert.match(raised.stdout, /PASS image-alt: 1\/1/);
});
