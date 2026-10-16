#!/usr/bin/env node
import { chromium } from '../web/node_modules/playwright/index.mjs';
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { resolve, relative, extname, join, sep } from 'node:path';
import { execFileSync } from 'node:child_process';

const args = process.argv.slice(2);
const root = resolve(args[args.indexOf('--root') + 1] && args.includes('--root') ? args[args.indexOf('--root') + 1] : 'docs/dist');
const baselinePath = resolve(args.includes('--baseline') ? args[args.indexOf('--baseline') + 1] : 'docs/a11y-baseline.json');
const writeBaseline = args.includes('--write-baseline');
const site = new URL(process.env.SITE_URL || 'https://velvetmonkey.github.io/safemesh/');
const base = site.pathname;
const origin = 'http://127.0.0.1';
const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.json': 'application/json' };
const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];
const engine = `axe-core@${JSON.parse(readFileSync('docs/node_modules/axe-core/package.json', 'utf8')).version}`;
if (!existsSync(root)) throw new Error(`built output missing: ${root}`);
const pages = [];
function walk(dir) { for (const item of readdirSync(dir, { withFileTypes: true })) { const file = join(dir, item.name); if (item.isDirectory()) walk(file); else if (file.endsWith('.html')) pages.push(file); } }
walk(root); pages.sort();
if (!pages.length) throw new Error(`no HTML pages in ${root}`);
function fileFor(url) {
  const parsed = new URL(url);
  if (parsed.origin !== origin || !parsed.pathname.startsWith(base)) return null;
  const rel = decodeURIComponent(parsed.pathname.slice(base.length));
  let file = resolve(root, rel);
  if (file !== root && !file.startsWith(root + sep)) return null;
  if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
  return existsSync(file) ? file : null;
}
const axe = resolve('docs/node_modules/axe-core/axe.min.js');
if (!existsSync(axe)) throw new Error(`pinned axe-core missing: ${axe}`);
const browser = await chromium.launch({ headless: true });
let violations = 0;
let unsettled = 0;
const counts = {};
try {
  for (const htmlFile of pages) {
    const rel = relative(root, htmlFile).split(sep).join('/');
    const route = base + (rel === 'index.html' ? '' : rel.replace(/index\.html$/, ''));
    const context = await browser.newContext();
    try {
      await context.route('**/*', async request => {
        const file = fileFor(request.request().url());
        if (!file) { await request.abort(); return; }
        await request.fulfill({ status: 200, contentType: MIME[extname(file).toLowerCase()] || 'application/octet-stream', body: readFileSync(file) });
      });
      const page = await context.newPage();
      await page.goto(origin + route, { waitUntil: 'load', timeout: 30000 });
      try {
        await page.waitForFunction(() =>
          document.fonts.status === 'loaded' &&
          [...document.querySelectorAll('.expressive-code pre')].every(pre =>
            pre.closest('details:not([open])') ||
            (pre.scrollWidth > pre.clientWidth) === pre.hasAttribute('tabindex')),
        null, { polling: 50, timeout: 10000 });
      } catch (error) {
        const state = await page.evaluate(() => ({
          fonts: document.fonts.status,
          waiting: [...document.querySelectorAll('.expressive-code pre')]
            .filter(pre => !pre.closest('details:not([open])') &&
              (pre.scrollWidth > pre.clientWidth) !== pre.hasAttribute('tabindex'))
            .map(pre => `${pre.getAttribute('data-language') || 'pre'} ${pre.scrollWidth}/${pre.clientWidth} tabindex=${pre.hasAttribute('tabindex')}`),
        })).catch(() => null);
        console.log(`UNSETTLED ${route}: ${state ? `fonts=${state.fonts}; waiting=${state.waiting.join(', ') || 'none'}` : String(error)}`);
        unsettled++;
        continue;
      }
      await page.addScriptTag({ path: axe });
      const result = await page.evaluate(tags => window.axe.run(document, { runOnly: { type: 'tag', values: tags } }), TAGS);
      for (const rule of result.violations) {
        violations++;
        counts[rule.id] = (counts[rule.id] || 0) + 1;
        const elements = rule.nodes.map(node => node.target.join(' '));
        console.log(`FOUND ${route}: ${rule.id}: ${elements.length} element(s): ${elements.slice(0, 3).join(', ')}${elements.length > 3 ? `, +${elements.length - 3} more` : ''}`);
      }
    } finally { await context.close(); }
  }
} finally { await browser.close(); }
const sortedCounts = Object.fromEntries(Object.entries(counts).sort(([a], [b]) => a.localeCompare(b)));
if (writeBaseline) {
  if (unsettled) throw new Error(`${unsettled} page(s) did not settle; baseline not written`);
  const sourceHead = execFileSync('git', ['rev-parse', 'origin/main'], { encoding: 'utf8' }).trim();
  const baseline = { measuredOn: new Date().toISOString().slice(0, 10), sourceHead, engine, tags: TAGS, total: violations, counts: sortedCounts };
  writeFileSync(baselinePath, JSON.stringify(baseline, null, 2) + '\n');
  console.log(`WROTE accessibility baseline ${baselinePath}: ${violations} rule occurrences`);
} else {
  const baseline = JSON.parse(readFileSync(baselinePath, 'utf8'));
  if (baseline.engine !== engine || JSON.stringify(baseline.tags) !== JSON.stringify(TAGS)) throw new Error(`accessibility baseline engine/tags differ from ${engine}`);
  if (Object.values(baseline.counts).reduce((sum, count) => sum + count, 0) !== baseline.total) throw new Error('accessibility baseline total does not match rule counts');
  let regressions = 0;
  for (const rule of [...new Set([...Object.keys(baseline.counts), ...Object.keys(counts)])].sort()) {
    const current = counts[rule] || 0;
    const allowed = baseline.counts[rule] || 0;
    const red = current > allowed;
    console.log(`${red ? 'FAIL' : 'PASS'} ${rule}: ${current}/${allowed} rule occurrences`);
    if (red) regressions++;
  }
  console.log(`Accessibility ratchet: ${pages.length} pages, ${violations}/${baseline.total} total rule occurrences, ${regressions} rules above baseline; automated WCAG A/AA tag scan only, manual review remains open.`);
  if (regressions || unsettled) process.exitCode = 1;
}
