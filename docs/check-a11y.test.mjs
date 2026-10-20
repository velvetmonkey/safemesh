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

const zeroBaseline = join(root, 'zero-baseline.json');
writeFileSync(zeroBaseline, JSON.stringify({ measuredOn: '2026-10-16', engine: 'axe-core@4.10.3', tags: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'], total: 0, counts: {} }));
const overflowingPre = '<div class="expressive-code"><pre style="width:100px;overflow:auto;white-space:pre">' + 'long-line-'.repeat(60) + '</pre></div>';
function scanFixture(route) {
  return spawnSync(process.execPath, ['docs/check-a11y.mjs', '--root', join(root, route), '--baseline', zeroBaseline], { cwd: new URL('..', import.meta.url), encoding: 'utf8', env: process.env });
}

test('late focus settles before axe runs on every repeat', () => {
  const fixture = join(root, 'late-focus');
  mkdirSync(fixture);
  writeFileSync(join(fixture, 'index.html'), `<!doctype html><html lang="en"><title>Late focus</title><body><main><h1>Late focus</h1>${overflowingPre}</main><script>setTimeout(() => document.querySelector('.expressive-code pre').setAttribute('tabindex', '0'), 600)</script></body></html>`);
  for (let i = 0; i < 5; i++) {
    const result = scanFixture('late-focus');
    assert.equal(result.status, 0, `repeat ${i + 1}: ${result.stdout}\n${result.stderr}`);
    assert.doesNotMatch(result.stdout, /scrollable-region-focusable|UNSETTLED/);
  }
});

test('a scroll region that never receives focus fails on every repeat', () => {
  const fixture = join(root, 'never-focus');
  mkdirSync(fixture);
  writeFileSync(join(fixture, 'index.html'), `<!doctype html><html lang="en"><title>Never focus</title><body><main><h1>Never focus</h1>${overflowingPre}</main></body></html>`);
  for (let i = 0; i < 5; i++) {
    const result = scanFixture('never-focus');
    assert.equal(result.status, 1, `repeat ${i + 1}: ${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /UNSETTLED \/safemesh\//);
  }
});

test('closed details with a never focusable scroll region fail on every repeat', () => {
  const fixture = join(root, 'closed-details');
  mkdirSync(fixture);
  writeFileSync(join(fixture, 'index.html'), `<!doctype html><html lang="en"><title>Closed details</title><style>details:not([open]) .expressive-code pre{display:none}</style><body><main><h1>Closed details</h1><details><summary>Code</summary>${overflowingPre}</details></main></body></html>`);
  for (let i = 0; i < 5; i++) {
    const result = scanFixture('closed-details');
    assert.equal(result.status, 1, `repeat ${i + 1}: ${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /UNSETTLED \/safemesh\/|scrollable-region-focusable/);
  }
});

test('closed details code that gains focus after opening stays green', () => {
  const fixture = join(root, 'closed-late-focus');
  mkdirSync(fixture);
  writeFileSync(join(fixture, 'index.html'), `<!doctype html><html lang="en"><title>Closed late focus</title><body><main><h1>Closed late focus</h1><details><summary>Code</summary>${overflowingPre}</details></main><script>new ResizeObserver(() => { const pre = document.querySelector('.expressive-code pre'); if (pre.scrollWidth > pre.clientWidth) pre.setAttribute('tabindex', '0') }).observe(document.querySelector('.expressive-code pre'))</script></body></html>`);
  for (let i = 0; i < 5; i++) {
    const result = scanFixture('closed-late-focus');
    assert.equal(result.status, 0, `repeat ${i + 1}: ${result.stdout}\n${result.stderr}`);
    assert.doesNotMatch(result.stdout, /scrollable-region-focusable|UNSETTLED/);
  }
});

test('closed details image without alt is scanned when code gains focus', () => {
  const fixture = join(root, 'closed-image');
  mkdirSync(fixture);
  writeFileSync(join(fixture, 'index.html'), `<!doctype html><html lang="en"><title>Closed image</title><style>details:not([open]) .expressive-code pre{display:none}</style><body><main><h1>Closed image</h1><details><summary>Code</summary>${overflowingPre}<img src="/safemesh/pixel.png"></details></main><script>new ResizeObserver(() => { const pre = document.querySelector('.expressive-code pre'); if (pre.scrollWidth > pre.clientWidth) pre.setAttribute('tabindex', '0') }).observe(document.querySelector('.expressive-code pre'))</script></body></html>`);
  for (let i = 0; i < 5; i++) {
    const result = scanFixture('closed-image');
    assert.equal(result.status, 1, `repeat ${i + 1}: ${result.stdout}\n${result.stderr}`);
    assert.match(result.stdout, /FOUND \/safemesh\/: image-alt:/);
  }
});
