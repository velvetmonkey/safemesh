#!/usr/bin/env node
// Baseline: 2026-10-14, generated Rust reference maximum 953.4 KiB gzip.
// The product ruling allows 10% headroom for that generated class: ceil(953.4 * 1.10) = 1049 KiB.
import { chromium } from '../web/node_modules/playwright/index.mjs';
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { resolve, relative, extname, join, sep } from 'node:path';
import { gzipSync } from 'node:zlib';

const args = process.argv.slice(2);
const root = resolve(args[args.indexOf('--root') + 1] && args.includes('--root') ? args[args.indexOf('--root') + 1] : 'docs/dist');
const site = new URL(process.env.SITE_URL || 'https://velvetmonkey.github.io/safemesh/');
const base = site.pathname;
const origin = 'http://127.0.0.1';
const GUIDE_TOTAL_KIB = 350;
const JS_KIB = 100;
const RUST_REFERENCE_TOTAL_KIB = 1049;
const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg', '.jpeg': 'image/jpeg', '.webp': 'image/webp', '.woff': 'font/woff', '.woff2': 'font/woff2', '.ttf': 'font/ttf', '.json': 'application/json' };
const ASSETS = new Set(['.css', '.js', '.mjs', '.woff', '.woff2', '.ttf', '.png', '.jpg', '.jpeg', '.gif', '.svg', '.webp', '.avif']);
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
const browser = await chromium.launch({ headless: true });
let failures = 0;
try {
  for (const htmlFile of pages) {
    const rel = relative(root, htmlFile).split(sep).join('/');
    const route = base + (rel === 'index.html' ? '' : rel.replace(/index\.html$/, ''));
    const assets = new Set();
    const wasm = new Set();
    const context = await browser.newContext();
    try {
      await context.route('**/*', async request => {
        const file = fileFor(request.request().url());
        if (!file) { await request.abort(); return; }
        await request.fulfill({ status: 200, contentType: MIME[extname(file).toLowerCase()] || 'application/octet-stream', body: readFileSync(file) });
      });
      const page = await context.newPage();
      page.on('request', request => {
        const file = fileFor(request.url());
        if (!file || file === htmlFile) return;
        const ext = extname(file).toLowerCase();
        if (ext === '.wasm') wasm.add(relative(root, file).split(sep).join('/'));
        if (ASSETS.has(ext)) assets.add(file);
      });
      await page.goto(origin + route, { waitUntil: 'load', timeout: 30000 });
      await page.waitForTimeout(200);
      const htmlBytes = gzipSync(readFileSync(htmlFile)).length;
      let totalBytes = htmlBytes;
      let jsBytes = 0;
      for (const file of assets) {
        const bytes = gzipSync(readFileSync(file)).length;
        totalBytes += bytes;
        if (['.js', '.mjs'].includes(extname(file).toLowerCase())) jsBytes += bytes;
      }
      const total = totalBytes / 1024, js = jsBytes / 1024;
      const lab = rel === 'lab/index.html';
      const rustReference = rel.startsWith('reference/rust/');
      const limit = rustReference ? RUST_REFERENCE_TOTAL_KIB : GUIDE_TOTAL_KIB;
      const label = lab ? 'Lab exemption' : rustReference ? `generated Rust reference ${limit} KiB (2026-10-14 baseline 953.4 KiB + 10%)` : `guide ${limit} KiB`;
      const bad = !lab && (totalBytes > limit * 1024 || jsBytes > JS_KIB * 1024 || wasm.size > 0);
      const detail = `${route}: total ${total.toFixed(1)} KiB/${limit} KiB, JS ${js.toFixed(1)} KiB/${JS_KIB} KiB, WASM ${wasm.size ? [...wasm].join(', ') : 'none'}; ${label}`;
      console.log(`${bad ? 'FAIL' : 'PASS'} ${detail}`);
      if (bad) failures++;
    } finally { await context.close(); }
  }
} finally { await browser.close(); }
console.log(`Page budget: ${pages.length} pages, ${failures} failures; Lab exemption is size only.`);
if (failures) process.exitCode = 1;
