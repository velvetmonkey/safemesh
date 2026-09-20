// Run against the docs dev/preview server:
// PLAYWRIGHT_MODULE=/path/to/playwright-core/index.mjs CHROMIUM_PATH=/path/to/chrome \
// DOCS_URL=http://localhost:4321/safemesh/ node --test docs/convergence.browser.test.mjs
import assert from 'node:assert/strict';
import { test } from 'node:test';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || new URL('../web/node_modules/playwright/index.mjs', import.meta.url).href);
const url = process.env.DOCS_URL || 'http://localhost:4321/safemesh/';
for (const width of [1280, 390]) for (const theme of ['light', 'dark']) {
  test(`reduced motion exposes the whole story: ${width}px ${theme}`, async () => {
    const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true });
    try {
      const page = await browser.newPage({ viewport: { width, height: 800 }, reducedMotion: 'reduce', colorScheme: theme });
      await page.goto(url);
      await page.locator('[data-mesh]').waitFor();
      await page.evaluate(theme => document.documentElement.dataset.theme = theme, theme);
      const panels = page.locator('[data-story-step]');
      assert.equal(await panels.count(), 3, 'reduced motion must expose all three story panels, never one frozen frame');
      const evidence = [];
      for (const panel of await panels.all()) {
        assert.equal(await panel.isVisible(), true);
        const box = await panel.boundingBox();
        assert.ok(box.x >= 0 && box.y >= 0 && box.x + box.width <= width && box.y + box.height <= 800,
          `story panel outside first screen: ${JSON.stringify(box)}`);
        evidence.push({ text: await panel.innerText(), box });
      }
      assert.match(evidence[0].text, /Offline/);
      assert.match(evidence[1].text, /Reconnect/);
      assert.match(evidence[2].text, /Same everywhere/);
      const devices = panels.last().locator('[data-device]');
      assert.equal(await devices.count(), 3);
      const lists = [];
      for (const device of await devices.all()) {
        lists.push(await device.locator('span').allTextContents());
        assert.match(await device.innerText(), /Pump checked/);
        assert.match(await device.innerText(), /Valve leaking/);
      }
      assert.deepEqual(lists[0], lists[1]);
      assert.deepEqual(lists[1], lists[2]);
      console.log(JSON.stringify({ width, theme, evidence }));
    } finally { await browser.close(); }
  });
}

test('motion loops from step 3 to step 1; controls pause, play and select phases', async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, reducedMotion: 'no-preference' });
    await page.goto(url);
    const mesh = page.locator('[data-mesh]');
    await page.waitForFunction(() => document.querySelector('[data-mesh]')?.dataset.running === 'true');
    const samples = [];
    for (let elapsed = 0; elapsed <= 20000; elapsed += 2500) {
      if (elapsed) await page.waitForTimeout(2500);
      samples.push({ elapsed, phase: await mesh.getAttribute('data-phase') });
    }
    console.log(JSON.stringify({ samples }));
    const phases = samples.map(s => s.phase).filter((p, i, all) => i === 0 || p !== all[i - 1]);
    assert.deepEqual(phases.slice(0, 4), ['offline', 'sync', 'replay', 'offline']);
    const playback = page.locator('[data-mesh-playback]');
    await playback.click();
    const pausedPhase = await mesh.getAttribute('data-phase');
    await page.waitForTimeout(4500);
    assert.equal(await mesh.getAttribute('data-phase'), pausedPhase);
    assert.equal(await mesh.getAttribute('data-running'), 'false');
    for (const phase of ['offline', 'sync', 'replay']) {
      const button = page.locator(`[data-phase-button="${phase}"]`);
      await button.click();
      assert.equal(await button.getAttribute('aria-pressed'), 'true');
      assert.equal(await page.locator(`[data-story-step="${phase}"]`).isVisible(), true);
      assert.equal(await page.locator('[data-story-step]:visible').count(), 1);
    }
    await playback.click();
    await page.waitForTimeout(4500);
    assert.equal(await mesh.getAttribute('data-phase'), 'offline');
    await page.emulateMedia({ reducedMotion: 'reduce' });
    assert.equal(await page.locator('[data-story-step]:visible').count(), 3);
    await page.waitForTimeout(4500);
    assert.equal(await mesh.getAttribute('data-running'), 'false');
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await page.waitForFunction(() => document.querySelector('[data-mesh]')?.dataset.running === 'true');
    assert.equal(await mesh.getAttribute('data-phase'), 'offline');
  } finally { await browser.close(); }
});

test('without JavaScript the full illustration is readable', async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true });
  try {
    const page = await browser.newPage({ javaScriptEnabled: false, viewport: { width: 390, height: 800 } });
    await page.goto(url);
    assert.equal(await page.locator('[data-story-step]:visible').count(), 3);
    assert.match(await page.locator('[data-mesh]').innerText(), /Neither addition is lost/);
  } finally { await browser.close(); }
});

async function assertCounterData(page) {
  const mesh = page.locator('[data-primitive-mesh]');
  assert.equal(await mesh.isVisible(), true);
  assert.match(await mesh.innerText(), /2 \+ 3 \+ 0 = 5/);
  const replicas = mesh.locator('[data-counter-replica]');
  assert.equal(await replicas.count(), 3);
  for (const [i, replica] of (await replicas.all()).entries()) {
    assert.equal(await replica.isVisible(), true);
    assert.match(await replica.textContent(), new RegExp(`Replica ${'ABC'[i]}`));
    assert.match(await replica.textContent(), /A:2 · B:3 · C:0/);
    assert.match(await replica.textContent(), /Total 5/);
  }
  const packets = mesh.locator('[data-counter-packet]');
  assert.deepEqual(await packets.locator('text').allTextContents(), ['A:2', 'B:3', 'A:2']);
  for (const packet of await packets.all()) assert.equal(await packet.isVisible(), true);
}

for (const width of [1280, 390]) for (const reducedMotion of ['no-preference', 'reduce']) {
  test(`counter data and motion: ${width}px ${reducedMotion}`, async () => {
    const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true });
    try {
      const page = await browser.newPage({ viewport: { width, height: 900 }, reducedMotion });
      await page.goto(url);
      await assertCounterData(page);
      const mesh = page.locator('[data-primitive-mesh]');
      await mesh.scrollIntoViewIfNeeded();
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'no horizontal overflow');
      const intro = await page.locator('.sm-hero-intro').innerText();
      for (const language of ['Rust', 'TypeScript/JavaScript', 'WASM', 'Python', 'C.']) assert.ok(intro.includes(language));
      assert.match(await page.locator('.sm-hero-intro a').getAttribute('href'), /\/using-safemesh\/$/);
      const transforms = [];
      const frames = [];
      for (let i = 0; i < 3; i++) {
        if (i) await page.waitForTimeout(1000);
        transforms.push(await mesh.locator('[data-counter-packet]').first().evaluate(el => getComputedStyle(el).transform));
        if (process.env.SCREENSHOT_DIR) {
          const { mkdir } = await import('node:fs/promises');
          await mkdir(process.env.SCREENSHOT_DIR, { recursive: true });
          frames.push(await mesh.screenshot({ path: `${process.env.SCREENSHOT_DIR}/${width}-${reducedMotion}-${i + 1}.png`, animations: 'allow' }));
        }
      }
      if (reducedMotion === 'reduce') {
        assert.equal(new Set(transforms).size, 1);
        assert.equal(await page.locator('[data-story-step]:visible').count(), 3);
        assert.equal(await mesh.locator('[data-counter-playback]').isVisible(), false);
        assert.equal(await mesh.locator('[data-counter-packet]').first().evaluate(el => getComputedStyle(el).animationName), 'none');
      } else {
        assert.equal(new Set(transforms).size, 3, 'labelled data must actually move');
        if (frames.length) assert.ok(!frames[0].equals(frames[1]) && !frames[1].equals(frames[2]), 'rendered frames differ');
        await mesh.locator('[data-counter-playback]').click();
        assert.equal(await mesh.getAttribute('data-running'), 'false');
        await page.emulateMedia({ reducedMotion: 'reduce' });
        await assertCounterData(page);
        assert.equal(await page.locator('[data-story-step]:visible').count(), 3);
        await page.emulateMedia({ reducedMotion: 'no-preference' });
        await mesh.locator('[data-counter-playback]').click();
        assert.equal(await mesh.getAttribute('data-running'), 'true');
      }
      if (process.env.SCREENSHOT_DIR) {
        await page.emulateMedia({ reducedMotion });
        await page.screenshot({ path: `${process.env.SCREENSHOT_DIR}/${width}-${reducedMotion}-page.png`, fullPage: true });
      }
      console.log(JSON.stringify({ width, reducedMotion, transforms }));
      // Physical tamper: the same content assertion must reject a missing coordinate.
      await mesh.locator('[data-counter-replica] text').nth(1).evaluate(el => el.textContent = 'missing data');
      await assert.rejects(() => assertCounterData(page), /A:2/);
    } finally { await browser.close(); }
  });
}

test('counter illustration is complete without JavaScript', async () => {
  const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true });
  try {
    const page = await browser.newPage({ javaScriptEnabled: false, viewport: { width: 390, height: 800 } });
    await page.goto(url);
    await assertCounterData(page);
    assert.equal(await page.locator('[data-counter-playback]').isVisible(), false);
  } finally { await browser.close(); }
});
