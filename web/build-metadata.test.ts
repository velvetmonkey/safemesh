import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { beforeAll, expect, it } from 'vitest'

beforeAll(() => {
  execFileSync('npm', ['run', 'build'], { cwd: import.meta.dirname, stdio: 'pipe' })
}, 120_000)

function readRecord(): Record<string, string> {
  const html = readFileSync(new URL('./dist/index.html', import.meta.url), 'utf8')
  const stamp = html.match(/<script id="build-metadata" type="application\/json">([^<]+)<\/script>/)
  if (!stamp) throw new Error('Built Lab is missing its build-metadata record')
  return JSON.parse(stamp[1])
}

it('records the authoritative web package version', () => {
  const manifest = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'))
  expect(readRecord().package).toBe(manifest.version)
})

it('records the authoritative CRDT source version', () => {
  const manifest = readFileSync(new URL('../rust/crates/safemesh-crdt/Cargo.toml', import.meta.url), 'utf8')
  const section = manifest.split(/^\[package\]\s*$/m)[1]?.split(/^\[/m)[0]
  const version = section?.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1]
  if (!version) throw new Error('Cannot read the CRDT package version declaration')
  expect(readRecord().source).toBe(version)
})

it('explicitly records wire as NOT DECLARED', () => {
  expect(readRecord().wire).toBe('NOT DECLARED')
})

it('explicitly records scenario as NOT DECLARED', () => {
  expect(readRecord().scenario).toBe('NOT DECLARED')
})
