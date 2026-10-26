#!/usr/bin/env node
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const rootFlag = process.argv.indexOf('--root');
if (rootFlag !== -1 && !process.argv[rootFlag + 1]) throw new Error('--root requires a directory');
const root = rootFlag === -1 ? repository : resolve(process.argv[rootFlag + 1]);
const claims = readFileSync(join(root, 'CLAIMS.md'), 'utf8');
const sourceExtensions = new Set(['.rs', '.py', '.mjs', '.ts', '.lean']);
const escape = name => name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

function defined(source, name) {
  // A mention in prose or a comment is not a declaration. Remove block
  // comments too, since their interior may start at column one.
  const code = source.replace(/\/\*[\s\S]*?\*\/|\/-[\s\S]*?-\//g, '');
  const declaration = new RegExp(
    `^\\s*(?:(?:pub(?:\\([^)]*\\))?|async|private|protected|noncomputable)\\s+)*(?:fn|def|theorem|lemma)\\s+${escape(name)}\\b`, 'm');
  const jsTest = new RegExp(`^\\s*(?:test|it)\\s*\\(\\s*['"\\x60]${escape(name)}['"\\x60]`, 'm');
  return declaration.test(code) || jsTest.test(code);
}

let links = 0;
let names = 0;
let missing = 0;
for (const match of claims.matchAll(/\[([^\]]+)\]\(([^)]+)\)/g)) {
  const [, label, destination] = match;
  const path = destination.split('#', 1)[0];
  if (!sourceExtensions.has(extname(path))) continue;
  const ticks = [...label.matchAll(/`([^`]+)`/g)];
  if (!ticks.length) continue;
  links++;
  const target = join(root, path);
  const source = existsSync(target) ? readFileSync(target, 'utf8') : null;
  for (const tick of ticks) {
    for (const item of tick[1].split(';')) {
      const name = item.trim();
      if (!name) continue;
      names++;
      if (source !== null && (name === basename(path, extname(path)) || defined(source, name))) continue;
      missing++;
      console.error(`FAIL ${path}: ${name}${source === null ? ' (target missing)' : ''}`);
    }
  }
}
if (names < 40) {
  missing++;
  console.error(`FAIL CLAIMS.md: only ${names} source evidence names (minimum 40)`);
}
console.log(`CLAIMS EVIDENCE LINKS ${links}, NAMES ${names}, MISSING ${missing}`);
if (missing) process.exitCode = 1;
