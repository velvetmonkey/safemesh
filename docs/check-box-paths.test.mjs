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

function check(page, name = 'planted.md') {
  const root = mkdtempSync(join(process.env.TMPDIR || tmpdir(), 'smdocsboxpath-'));
  const content = join(root, 'docs/src/content/docs');
  mkdirSync(content, { recursive: true });
  const files = typeof page === 'string' ? { [`docs/src/content/docs/${name}`]: page } : page;
  for (const [path, text] of Object.entries(files)) {
    const target = join(root, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, text);
  }
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

test('MDX-A: an indented JSX fence with a development-box path refuses', () => {
  const result = check(`<div>\n\n    \`\`\`sh\n    cat ${forbidden}/scratch/input\n    \`\`\`\n</div>\n`, 'planted.mdx');
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /planted\.mdx:3: cat \/home\/monkey\/scratch\/input/);
});

test('MDX-B: a JSX fence without a blank line refuses', () => {
  const result = check(`<div>\n\`\`\`sh\ncat ${forbidden}/scratch/input\n\`\`\`\n</div>\n`, 'planted.mdx');
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /planted\.mdx:2: cat \/home\/monkey\/scratch\/input/);
});

test('MDX-C: a column-one fence between JSX blocks refuses', () => {
  const result = check(`<div>first</div>\n\n\`\`\`sh\ncat ${forbidden}/scratch/input\n\`\`\`\n\n<div>last</div>\n`, 'planted.mdx');
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /planted\.mdx:3: cat \/home\/monkey\/scratch\/input/);
});

test('MDX-D: a clean indented JSX fence is counted', () => {
  const result = check('<div>\n\n    ```sh\n    echo clean\n    ```\n</div>\n', 'planted.mdx');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^PASS 1 pages, 1 fenced blocks read/m);
});

test('MDX-E: a development-box path in JSX text alone passes', () => {
  const result = check(`<div><p>${forbidden}/scratch/input</p></div>\n`, 'planted.mdx');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^PASS 1 pages, 0 fenced blocks read/m);
});

test('MDX-F: invalid MDX refuses with the page name', () => {
  const result = check('<div>\n', 'planted.mdx');
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /FAIL docs\/src\/content\/docs\/planted\.mdx:/);
});

const claimsStub = '---\ntitle: Claims\n---\n\n<!-- assurance-source: CLAIMS.md -->\n';
const proofStub = '---\ntitle: Proof\n---\n\n<!-- assurance-source: WHAT-IS-PROVEN.md -->\n';
const footnote = command => `# Page\n\nText.[^1]\n\n[^1]: Note.\n\n    \`\`\`sh\n    ${command}\n    \`\`\`\n`;

test('ROOT-A: a fenced path in CLAIMS.md refuses', () => {
  const result = check({
    'docs/src/content/docs/claims.md': claimsStub,
    'CLAIMS.md': `# Claims\n\n\`\`\`sh\ncat ${forbidden}/scratch/input\n\`\`\`\n`,
  });
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /FAIL CLAIMS\.md:3: cat \/home\/monkey\/scratch\/input/);
});

test('ROOT-B: a fenced path in WHAT-IS-PROVEN.md refuses', () => {
  const result = check({
    'docs/src/content/docs/proof.md': proofStub,
    'WHAT-IS-PROVEN.md': `# Proof\n\n\`\`\`sh\ncat ${forbidden}/scratch/input\n\`\`\`\n`,
  });
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /FAIL WHAT-IS-PROVEN\.md:3: cat \/home\/monkey\/scratch\/input/);
});

test('ROOT-C: a clean fence in CLAIMS.md is counted', () => {
  const result = check({
    'docs/src/content/docs/claims.md': claimsStub,
    'CLAIMS.md': '# Claims\n\n```sh\necho clean\n```\n',
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /^PASS 2 pages, 1 fenced blocks read/m);
});

test('FN-A: a fenced path in a GFM footnote refuses', () => {
  const result = check(footnote(`cat ${forbidden}/scratch/input`));
  assert.equal(result.status, 1, result.stdout + result.stderr);
  assert.match(result.stderr, /planted\.md:7: cat \/home\/monkey\/scratch\/input/);
});

test('FN-B: a clean GFM footnote fence is counted', () => {
  const result = check(footnote('echo clean'));
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(result.stdout, /^PASS 1 pages, 1 fenced blocks read/m);
});
