#!/usr/bin/env node
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { unified } from 'unified';
import remarkParse from 'remark-parse';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const rootFlag = process.argv.indexOf('--root');
if (rootFlag !== -1 && !process.argv[rootFlag + 1]) {
  throw new Error('--root requires a directory');
}
const root = rootFlag === -1 ? repository : resolve(process.argv[rootFlag + 1]);
const content = join(root, 'docs/src/content/docs');
const forbidden = '/home/' + 'monkey';
const parser = unified().use(remarkParse);

function pagesUnder(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return pagesUnder(path);
    return entry.isFile() && ['.md', '.mdx'].includes(extname(entry.name)) ? [path] : [];
  });
}

function* codeNodes(node) {
  if (node.type === 'code') yield node;
  for (const child of node.children ?? []) yield* codeNodes(child);
}

const pages = pagesUnder(content).sort();
if (pages.length === 0) throw new Error(`no published pages in ${content}`);
let fences = 0;
let refusals = 0;
for (const page of pages) {
  const source = readFileSync(page, 'utf8');
  const tree = parser.parse(source);
  for (const node of codeNodes(tree)) {
    // The parser also calls four-space indented blocks "code". A fence starts
    // at the node's source position with up to three leading spaces.
    if (!/^ {0,3}(?:`{3,}|~{3,})/.test(source.slice(node.position.start.offset))) continue;
    fences++;
    for (const line of node.value.split(/\r?\n/)) {
      if (!line.includes(forbidden)) continue;
      refusals++;
      const name = relative(root, page).split(sep).join('/');
      console.error(`FAIL ${name}:${node.position.start.line}: ${line.trim()}`);
    }
  }
}
if (refusals) process.exitCode = 1;
else console.log(`PASS ${pages.length} pages, ${fences} fenced blocks read`);
