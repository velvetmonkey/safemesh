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
