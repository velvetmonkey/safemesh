import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = join(dirname(fileURLToPath(import.meta.url)), '..');
const checker = join(repository, 'docs/check-box-paths.mjs');
const forbidden = '/home/' + 'monkey';

function check(page) {
  const root = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'smdocsboxpath-'));
  const content = join(root, 'docs/src/content/docs');
  mkdirSync(content, { recursive: true });
  writeFileSync(join(content, 'planted.md'), page);
  return spawnSync(process.execPath, [checker, '--root', root], {
    cwd: repository, encoding: 'utf8', env: process.env,
  });
}

test('a clean fenced command passes', () => {
  const result = check('# Page\n\n```sh\necho clean\n```\n');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^PASS 1 pages, 1 fenced blocks read/m);
});

test('a development-box path in a fenced command refuses with page, fence line, and text', () => {
  const result = check(`# Page\n\n\`\`\`sh\ncat ${forbidden}/scratch/input\n\`\`\`\n`);
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /docs\/src\/content\/docs\/planted\.md:3: cat \/home\/monkey\/scratch\/input/);
});

test('a development-box path in prose alone passes', () => {
  const result = check(`# Page\n\nThis records ${forbidden}/scratch/input.\n`);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^PASS 1 pages, 0 fenced blocks read/m);
});

test('a development-box path in a four-backtick fence refuses', () => {
  const result = check(`# Page\n\n\`\`\`\`sh\ncat ${forbidden}/scratch/input\n\`\`\`\`\n`);
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /planted\.md:3: cat \/home\/monkey\/scratch\/input/);
});

test('four-space indented backticks are not a fenced command', () => {
  const result = check(`# Page\n\n    \`\`\`sh\n    cat ${forbidden}/scratch/input\n    \`\`\`\n`);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^PASS 1 pages, 0 fenced blocks read/m);
});
