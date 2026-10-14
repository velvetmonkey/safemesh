#!/usr/bin/env node
import { chromium } from '../web/node_modules/playwright/index.mjs';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { resolve, relative, extname, join, sep } from 'node:path';

const args = process.argv.slice(2);
const root = resolve(args[args.indexOf('--root') + 1] && args.includes('--root') ? args[args.indexOf('--root') + 1] : 'docs/dist');
const site = new URL(process.env.SITE_URL || 'https://velvetmonkey.github.io/safemesh/');
const base = site.pathname;
const origin = 'http://127.0.0.1';
const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.json': 'application/json' };
const TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];
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
      await page.waitForTimeout(200);
      await page.addScriptTag({ path: axe });
      const result = await page.evaluate(tags => window.axe.run(document, { runOnly: { type: 'tag', values: tags } }), TAGS);
      for (const rule of result.violations) {
        violations++;
        const elements = rule.nodes.map(node => node.target.join(' '));
        console.log(`FAIL ${route}: ${rule.id}: ${elements.length} element(s): ${elements.slice(0, 3).join(', ')}${elements.length > 3 ? `, +${elements.length - 3} more` : ''}`);
      }
    } finally { await context.close(); }
  }
} finally { await browser.close(); }
console.log(`Accessibility: ${pages.length} pages, ${violations} rule occurrences; automated WCAG A/AA tag scan only, manual review remains open.`);
if (violations) process.exitCode = 1;
