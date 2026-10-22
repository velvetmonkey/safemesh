// Check the python CI job's matrix against the five prose claims about that matrix.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
if (args.length !== 0 && (args.length !== 2 || args[0] !== '--root')) {
  console.error('Usage: node scripts/check-python-matrix-claims.mjs [--root <dir>]');
  process.exit(1);
}
const root = args.length ? args[1] : join(dirname(fileURLToPath(import.meta.url)), '..');
const failures = [];
const workflow = '.github/workflows/ci.yml';
const surfaces = [
  ['README.md', 'CI configures wheel builds'],
  ['docs/src/content/docs/persist-and-restart.md', 'job matrix is CPython'],
  ['docs/src/content/docs/using-safemesh.md', 'The Python CI matrix builds and installs a local wheel'],
  ['rust/crates/safemesh-python/README.md', 'CI builds and installs a wheel and runs'],
  ['rust/crates/safemesh-python/README.md', 'The Python CI matrix builds and installs the wheel'],
];

function read(path) {
  try { return readFileSync(join(root, path), 'utf8'); }
  catch (error) {
    failures.push(`FAIL ${path}:0: ${error.code ?? error.message}`);
    return null;
  }
}
function lineAt(source, offset) {
  return source.slice(0, offset).split('\n').length;
}
function minor(version) {
  const match = /^3\.(\d+)$/.exec(version);
  return match ? Number(match[1]) : null;
}
function parseMatrix(source) {
  if (source === null) return null;
  const lines = source.split('\n');
  const start = lines.findIndex(line => /^  python:\s*$/.test(line));
  if (start < 0) {
    failures.push(`FAIL ${workflow}:0: python job not found`);
    return null;
  }
  let end = start + 1;
  while (end < lines.length && !/^  [\w-]+:\s*$/.test(lines[end])) end++;
  const block = lines.slice(start + 1, end);
  const runner = block.find(line => /^    runs-on:/.test(line))?.match(/^    runs-on:\s*([\w.-]+)\s*$/)?.[1];
  const matrixStart = block.findIndex(line => /^      matrix:\s*$/.test(line));
  // The matrix key must be nested under this job's strategy, never in setup-python.
  const versionLine = matrixStart < 0 ? undefined : block.slice(matrixStart + 1).find(line => /^        python-version:/.test(line));
  const match = versionLine?.match(/^        python-version:\s*\[([^\]]+)\]\s*$/);
  const versions = match ? [...match[1].matchAll(/"(3\.\d+)"/g)].map(item => item[1]) : [];
  if (!runner || !match || versions.length === 0 ||
      match[1].replace(/"3\.\d+"|[\s,]/g, '') !== '' ||
      versions.some((version, i) => minor(version) === null || (i > 0 && minor(version) !== minor(versions[i - 1]) + 1))) {
    failures.push(`FAIL ${workflow}:${start + 1}: invalid python job runner or python-version matrix`);
    return null;
  }
  return { runner, versions };
}
function claimVersions(text) {
  const after = text.match(/CPython\s+([^\n]+)/)?.[1];
  if (!after) return null;
  // A later sentence or semicolon clause may describe a different CI job.
  const clause = after.split(/;|\.(?=\s|$)/, 1)[0];
  const range = clause.match(/^(3\.\d+)\s*(?:–|to)\s*(3\.\d+)/);
  if (range) {
    const first = minor(range[1]);
    const last = minor(range[2]);
    if (first === null || last === null || first > last) return null;
    const versions = Array.from({ length: last - first + 1 }, (_, i) => `3.${first + i}`);
    return versions.concat([...clause.slice(range[0].length).matchAll(/3\.\d+/g)].map(match => match[0]));
  }
  const list = clause.match(/^(3\.\d+(?:,\s*3\.\d+)+,\s*and\s*3\.\d+)/);
  return list ? [...clause.matchAll(/3\.\d+/g)].map(match => match[0]) : null;
}
const matrix = parseMatrix(read(workflow));
let checked = 0;
for (const [path, anchor] of surfaces) {
  const source = read(path);
  if (source === null) continue;
  const offset = source.indexOf(anchor);
  if (offset < 0) {
    failures.push(`FAIL ${path}: anchor not found: ${anchor}`);
    continue;
  }
  checked++;
  const line = lineAt(source, offset);
  const paragraph = source.slice(offset).split(/\n\s*\n/)[0].replace(/\s+/g, ' ');
  const versions = claimVersions(paragraph);
  if (!versions) {
    failures.push(`FAIL ${path}:${line}: CPython matrix versions not found`);
    continue;
  }
  if (matrix) {
    const actual = matrix.versions;
    if (versions.length !== actual.length || versions.some((value, i) => value !== actual[i])) {
      failures.push(`FAIL ${path}:${line}: claim ${versions[0]}..${versions.at(-1)} (${versions.length} versions) differs from python matrix ${actual[0]}..${actual.at(-1)} (${actual.length} versions)`);
    }
    const namedRunners = [...paragraph.matchAll(/`((?:ubuntu|windows|macos)-(?:latest|\d+(?:\.\d+)*))`/g)].map(match => match[1]);
    if (namedRunners.some(runner => runner !== matrix.runner)) {
      failures.push(`FAIL ${path}:${line}: runner ${namedRunners.join(', ')} differs from python job ${matrix.runner}`);
    }
  }
}
if (failures.length) {
  for (const failure of failures) console.error(failure);
  process.exit(1);
}
console.log(`PASS python matrix ${matrix.versions[0]}..${matrix.versions.at(-1)} (${matrix.versions.length} versions), ${checked} surfaces`);
