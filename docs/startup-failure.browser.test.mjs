import assert from 'node:assert/strict';
import { test } from 'node:test';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { extname, join, resolve, sep } from 'node:path';
import { chromium } from '../web/node_modules/playwright/index.mjs';

const root = resolve('docs/dist');
const origin = 'http://127.0.0.1';
const base = '/safemesh/';
const sentence = 'WASM could not load. The expected result above is a code illustration.';
const rust = readFileSync(new URL('../rust/crates/safemesh-crdt/examples/home_counter.rs', import.meta.url), 'utf8').split('\n#[test]')[0].trim();
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.png': 'image/png', '.webp': 'image/webp', '.woff2': 'font/woff2' };

function builtFile(url) {
  const parsed = new URL(url);
  if (parsed.origin !== origin || !parsed.pathname.startsWith(base)) return null;
  const relative = decodeURIComponent(parsed.pathname.slice(base.length));
  let file = resolve(root, relative);
  if (file !== root && !file.startsWith(root + sep)) return null;
  if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
  return existsSync(file) ? file : null;
}

async function checkStartupFailure(mode) {
  assert.ok(existsSync(join(root, 'index.html')), 'build the real home page first');
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext();
    if (mode === 'unsupported runtime') {
      await context.addInitScript(() => {
        Object.defineProperty(window, 'WebAssembly', { value: undefined, configurable: true });
      });
    }
    let htmlServed = false;
    await context.route('**/*', async route => {
      const request = route.request();
      const url = new URL(request.url());
      if (mode === 'no network' && htmlServed) { await route.abort('internetdisconnected'); return; }
      const file = builtFile(request.url());
      if (!file) { await route.abort(); return; }
      if (mode === 'failed WASM load' && url.pathname.endsWith('/hero-wasm/safemesh_wasm.js')) {
        await route.fulfill({ status: 404, contentType: 'text/plain', body: 'missing' });
        return;
      }
      if (mode === 'malformed import' && url.pathname.endsWith('/hero-wasm/safemesh_wasm.js')) {
        await route.fulfill({ status: 200, contentType: 'text/javascript', body: 'export default (((;' });
        return;
      }
      await route.fulfill({ status: 200, contentType: mime[extname(file).toLowerCase()] || 'application/octet-stream', body: readFileSync(file) });
      if (request.isNavigationRequest() && request.resourceType() === 'document') htmlServed = true;
    });
    const page = await context.newPage();
    page.setDefaultTimeout(5000);
    await page.goto(origin + base, { waitUntil: 'domcontentloaded', timeout: 10000 });
    const card = page.locator('[data-counter-card]');
    const status = card.getByRole('status');
    await status.getByText(sentence, { exact: true }).waitFor({ state: 'visible', timeout: 5000 });
    assert.equal(await status.innerText(), sentence);
    assert.equal(await card.locator('[data-counter-total]').innerText(), '5');
    assert.equal(await card.locator('[data-counter-total]').isVisible(), true);
    assert.equal(await card.locator('pre').innerText(), rust);
    assert.equal(await card.locator('pre').isVisible(), true);
    assert.equal(await card.getByRole('button', { name: 'Merge once' }).isDisabled(), true);
    assert.equal(await card.getByRole('button', { name: 'Replay same state' }).isDisabled(), true);
    assert.equal(await card.locator('[aria-busy="true"], [role="progressbar"]').count(), 0);
  } finally { await browser.close(); }
}

for (const mode of ['failed WASM load', 'no network', 'unsupported runtime', 'malformed import']) {
  test(mode, () => checkStartupFailure(mode));
}
