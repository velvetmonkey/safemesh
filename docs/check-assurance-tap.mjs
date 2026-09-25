import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';

const testFile = fileURLToPath(new URL('./assurance.test.mjs', import.meta.url));
const expected = JSON.parse(execFileSync(process.execPath, [testFile, '--manifest'], { encoding: 'utf8' }));
assert.equal(expected.length, 18, 'assurance manifest must contain 18 cases');
assert.equal(new Set(expected).size, expected.length, 'assurance case names must be unique');

const tap = fs.readFileSync(process.argv[2], 'utf8');
const actual = [...tap.matchAll(/^ok (\d+) - (.+)$/gm)];
assert.deepEqual(actual.map(match => Number(match[1])), expected.map((_, index) => index + 1), 'TAP case numbering');
assert.deepEqual(actual.map(match => match[2]), expected, 'TAP assurance case names');
