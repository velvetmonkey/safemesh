// A failed lookup must never silently become an unreleased badge.
let releaseStatus = 'release status not checked';
try {
  const response = await fetch('https://api.github.com/repos/velvetmonkey/safemesh/releases?per_page=1', {
    headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'safemesh-docs-build',
      ...(process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {}) },
    signal: AbortSignal.timeout(15000),
  });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  const releases = await response.json();
  if (!Array.isArray(releases)) throw new Error('invalid release response');
  releaseStatus = releases.length === 0 ? 'main, unreleased' : 'main, see releases';
  console.log(`SafeMesh release status at build time: ${releaseStatus}`);
} catch (error) {
  console.warn(`SafeMesh release status not checked: ${error instanceof Error ? error.message : String(error)}`);
}
export { releaseStatus };
