// Check local build names and README.md install-matrix pins against package manifests.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const defaultRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const args = process.argv.slice(2);
if (args.length !== 0 && (args.length !== 2 || args[0] !== '--root')) {
  console.error('Usage: node scripts/check-doc-versions.mjs [--root <dir>]');
  process.exit(1);
}
const root = args.length ? args[1] : defaultRoot;
const failures = [];
let checked = 0;
const checkedFiles = new Set();

function read(path) {
  try {
    return readFileSync(join(root, path), 'utf8');
  } catch (error) {
    failures.push(`FAIL ${path}:0: ${error.code ?? error.message}`);
    return null;
  }
}

function manifestVersion(path, table) {
  const source = read(path);
  if (source === null) return null;
  const section = source.split(/^\[([^\]]+)\]\s*$/m);
  for (let i = 1; i < section.length; i += 2) {
    if (section[i] !== table) continue;
    const match = section[i + 1].match(/^version\s*=\s*"(\d+\.\d+\.\d+)"\s*$/m);
    if (match) return match[1];
    break;
  }
  failures.push(`FAIL ${path}:0: missing version in [${table}]`);
  return null;
}

function check(path, pattern, expected, manifest) {
  checkedFiles.add(path);
  const source = read(path);
  if (source === null) return;
  let matches = 0;
  for (const line of source.split('\n').map((text, index) => ({ text, line: index + 1 }))) {
    for (const match of line.text.matchAll(pattern)) {
      matches++;
      checked++;
      if (expected !== null && match[1] !== expected) {
        failures.push(`FAIL ${path}:${line.line}: found ${match[1]} expected ${expected} (${manifest})`);
      }
    }
  }
  if (matches === 0) failures.push(`FAIL ${path}:0: found no version name expected ${expected ?? 'manifest version'} (${manifest})`);
}

const wasmManifest = 'rust/crates/safemesh-wasm/Cargo.toml';
const pythonManifest = 'rust/crates/safemesh-python/pyproject.toml';
const crdtManifest = 'rust/crates/safemesh-crdt/Cargo.toml';
const wasm = manifestVersion(wasmManifest, 'package');
const python = manifestVersion(pythonManifest, 'project');
const crdt = manifestVersion(crdtManifest, 'package');
check('README.md', /safemesh-crdt@(\d+\.\d+\.\d+)/g, crdt, crdtManifest);
check('README.md', /safemesh-wasm@(\d+\.\d+\.\d+)/g, wasm, wasmManifest);
check('README.md', /safemesh-python==(\d+\.\d+\.\d+)/g, python, pythonManifest);
check('demos/wasm-checklist/README.md', /safemesh-wasm-(\d+\.\d+\.\d+)\.tgz/g, wasm, wasmManifest);
check('demos/two-app-inventory/README.md', /safemesh-python==(\d+\.\d+\.\d+)/g, python, pythonManifest);
check('demos/two-app-inventory/requirements.txt', /safemesh-python==(\d+\.\d+\.\d+)/g, python, pythonManifest);
check('docs/src/content/docs/persist-and-restart.md', /currently records `(\d+\.\d+\.\d+)` as an unpublished crate version/g, crdt, crdtManifest);

if (failures.length) {
  for (const failure of failures) console.error(failure);
  process.exit(1);
}
console.log(`PASS ${checkedFiles.size} files, ${checked} version names checked`);
