import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir, readFile } from 'node:fs/promises';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || new URL('../web/node_modules/playwright/index.mjs', import.meta.url).href);
const url = process.env.DOCS_URL || 'http://localhost:4321/safemesh/';
for (const width of [1280, 390]) for (const reducedMotion of ['no-preference', 'reduce']) {
  test(`merge and replay: ${width}px ${reducedMotion}`, async () => {
    const browser = await chromium.launch({ headless: true });
    try {
      const page = await browser.newPage({ viewport: { width, height: 960 }, reducedMotion, colorScheme: 'dark' });
      page.setDefaultTimeout(10000);
      await page.goto(url);
      const source = await readFile(new URL('../rust/crates/safemesh-crdt/examples/home_counter.rs', import.meta.url), 'utf8');
      assert.equal(await page.locator('[data-counter-card] pre').innerText(), source.split('\n#[test]')[0].trim());
      const merge = page.getByRole('button', { name: 'Merge once', exact: true });
      await merge.waitFor({ timeout: 10000 });
      assert.equal(await page.locator('[data-counter-total]').innerText(), '5', 'static result needs no motion or runtime');
      await merge.click();
      assert.equal(await page.locator('[data-counter-total]').innerText(), '5');
      assert.match(await page.getByRole('status').innerText(), /Merged.*5/);
      const replay = page.getByRole('button', { name: 'Replay same state', exact: true });
      await replay.click();
      assert.equal(await page.locator('[data-counter-total]').innerText(), '5');
      assert.match(await page.getByRole('status').innerText(), /Replayed.*5/);
      await merge.focus();
      await page.keyboard.press('Enter');
      assert.match(await page.getByRole('status').innerText(), /Merged.*5/);
      await page.keyboard.press('Tab');
      assert.equal(await replay.evaluate(el => el === document.activeElement), true);
      await page.keyboard.press('Space');
      assert.match(await page.getByRole('status').innerText(), /Replayed.*5/);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
      assert.equal(await page.locator('h1').count(), 1);
      if (process.env.SCREENSHOT_DIR) {
        await mkdir(process.env.SCREENSHOT_DIR, { recursive: true });
        await page.evaluate(() => scrollTo(0, 0));
        await page.screenshot({ path: `${process.env.SCREENSHOT_DIR}/${width}-${reducedMotion}.png`, fullPage: true });
      }
    } finally { await browser.close(); }
  });
}
test('static code and expected result survive without JavaScript', async () => {
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage({ javaScriptEnabled: false, viewport: { width: 390, height: 960 } });
    page.setDefaultTimeout(10000);
    await page.goto(url);
    assert.equal(await page.locator('[data-counter-total]').innerText(), '5');
    assert.match(await page.locator('[data-counter-card] pre').innerText(), /assert_eq!\(a.value\(\), 5\)/);
    assert.equal(await page.getByRole('button', { name: 'Merge once', exact: true }).isDisabled(), true);
  } finally { await browser.close(); }
});
