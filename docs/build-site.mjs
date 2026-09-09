#!/usr/bin/env node
// Build the complete published site at one address: documentation, search index, then the Lab.
//
// SITE_URL (see site-url.mjs) is the only input. The Lab's public address and base path derive
// from it, and that address is passed to the documentation build as LAB_URL, so the header's
// Lab link and the Lab's own location can never drift apart. Plain `npm run build` keeps the
// local default (a separately started Lab at http://localhost:4173/) and never builds the Lab.
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { LAB_MOUNT, labUrl, siteUrl } from './site-url.mjs';

const site = siteUrl();
const lab = labUrl(site);
if (process.env.LAB_URL && process.env.LAB_URL !== lab.href) {
  throw new Error(`LAB_URL=${process.env.LAB_URL} contradicts the bundled Lab at ${lab.href}; unset it or use plain npm run build`);
}
const docs = fileURLToPath(new URL('./', import.meta.url));
const web = fileURLToPath(new URL('../web/', import.meta.url));
const labDir = fileURLToPath(new URL(`./dist/${LAB_MOUNT}`, import.meta.url));
const run = (args, options) => execFileSync('npm', args, { stdio: 'inherit', ...options });

console.log(`Site ${site.href}\nLab  ${lab.href} (base path ${lab.pathname})`);
// Search indexes the documentation before the Lab shell exists under dist/.
run(['run', 'build'], { cwd: docs, env: { ...process.env, LAB_URL: lab.href } });
run(['run', 'build', '--', '--base', lab.pathname, '--outDir', labDir, '--emptyOutDir'], { cwd: web });
console.log(`Lab built into ${labDir} for ${lab.href}`);
