#!/usr/bin/env node
import { readdirSync, readFileSync } from 'node:fs';
import { dirname, extname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { unified } from 'unified';
import remarkParse from 'remark-parse';
import { createProcessor } from '@mdx-js/mdx';
import remarkGfm from 'remark-gfm';
import remarkSmartypants from 'remark-smartypants';
import remarkDirective from 'remark-directive';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const rootFlag = process.argv.indexOf('--root');
if (rootFlag !== -1 && !process.argv[rootFlag + 1]) {
  throw new Error('--root requires a directory');
}
const root = rootFlag === -1 ? repository : resolve(process.argv[rootFlag + 1]);
const content = join(root, 'docs/src/content/docs');
const forbidden = '/home/' + 'monkey';
const markdownParser = unified().use(remarkParse);
const mdxParser = createProcessor({
  format: 'mdx',
  remarkPlugins: [remarkGfm, remarkSmartypants, remarkDirective],
});

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
let parseErrors = 0;
for (const page of pages) {
  const source = readFileSync(page, 'utf8');
  const mdx = extname(page) === '.mdx';
  let tree;
  try {
    tree = (mdx ? mdxParser : markdownParser).parse(source);
  } catch (error) {
    const name = relative(root, page).split(sep).join('/');
    console.error(`FAIL ${name}: ${error.message}`);
    parseErrors++;
    continue;
  }
  for (const node of codeNodes(tree)) {
    // Markdown calls four-space indented blocks "code" too. MDX has no
    // indented code blocks, so every MDX code node is a fence.
    if (!mdx && !/^ {0,3}(?:`{3,}|~{3,})/.test(source.slice(node.position.start.offset))) continue;
    fences++;
    for (const line of node.value.split(/\r?\n/)) {
      if (!line.includes(forbidden)) continue;
      refusals++;
      const name = relative(root, page).split(sep).join('/');
      console.error(`FAIL ${name}:${node.position.start.line}: ${line.trim()}`);
    }
  }
}
if (refusals || parseErrors) process.exitCode = 1;
else console.log(`PASS ${pages.length} pages, ${fences} fenced blocks read`);
