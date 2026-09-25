#!/usr/bin/env node
// Query the active Pagefind index. Old files left in fragment/ are not evidence
// that a page belongs to the index described by pagefind-entry.json.
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { LAB_MOUNT, labUrl, siteUrl } from './site-url.mjs';

const pagefindDir = resolve(import.meta.dirname, 'dist/pagefind');
const site = siteUrl();
const expected = `/${labUrl(site).pathname.slice(site.pathname.length)}`;
const fail = (reason) => { throw new Error(`Pagefind index is missing ${expected}: ${reason}`); };

try {
  const entry = JSON.parse(await readFile(resolve(pagefindDir, 'pagefind-entry.json'), 'utf8'));
  if (!Object.values(entry.languages ?? {}).some(({ page_count }) => page_count > 0)) {
    fail('the current index is empty');
  }

  // Pagefind's generated client fetches the entry, metadata, index chunks and
  // only fragments referenced by those chunks. Supply those files from disk.
  globalThis.fetch = async (input) => {
    const url = new URL(input.toString());
    const name = decodeURIComponent(url.pathname).slice(1);
    // Restrict the shim to Pagefind's own files, including chunk subdirectories.
    if (url.origin !== 'https://pagefind.local' ||
        !/^(?:[^/]+|(?:index|fragment)\/[^/]+)$/.test(name) || name === '..') {
      return new Response('not found', { status: 404 });
    }
    try {
      const bytes = await readFile(resolve(pagefindDir, name));
      return new Response(bytes, { headers: { 'content-type': name.endsWith('.wasm')
        ? 'application/wasm' : 'application/octet-stream' } });
    } catch {
      return new Response('not found', { status: 404 });
    }
  };

  const pagefind = await import(pathToFileURL(resolve(pagefindDir, 'pagefind.js')).href);
  await pagefind.options({ basePath: 'https://pagefind.local/' });
  const results = await pagefind.search(LAB_MOUNT.replace(/\/$/, ''));
  for (const result of results.results) {
    if ((await result.data()).raw_url === expected) {
      console.log(`Pagefind index contains ${expected}`);
      process.exit(0);
    }
  }
  fail('no result in the current index has that URL');
} catch (error) {
  if (error.message.startsWith('Pagefind index is missing ')) throw error;
  fail(error.message);
}
