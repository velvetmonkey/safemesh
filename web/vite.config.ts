import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

// The Lab's public base path (`vite build --base`): / for local servers, the mount
// path when the documentation site bundles the Lab under its own address.
let base = '/'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    {
      name: 'build-metadata',
      apply: 'build',
      transformIndexHtml() {
        const packageManifest = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'))
        const sourceManifest = fileURLToPath(new URL('../rust/crates/safemesh-crdt/Cargo.toml', import.meta.url))
        // Ask Cargo to resolve the actual crate manifest, rather than a lockfile or WASM package copy.
        const cargo = JSON.parse(execFileSync('cargo', [
          'metadata', '--no-deps', '--format-version', '1', '--manifest-path', sourceManifest,
        ], { encoding: 'utf8' }))
        const source = cargo.packages.find((pkg: { manifest_path: string }) => pkg.manifest_path === sourceManifest)
        if (typeof packageManifest.version !== 'string' || !packageManifest.version || !source?.version) {
          throw new Error('Build metadata requires declared package and CRDT source versions')
        }
        const record = {
          package: packageManifest.version,
          source: source.version,
          wire: 'NOT DECLARED',
          scenario: 'NOT DECLARED',
        }
        const line = `Package ${record.package} · Source ${record.source} · Wire ${record.wire} · Scenario ${record.scenario}`
        return [
          {
            tag: 'script',
            attrs: { id: 'build-metadata', type: 'application/json' },
            children: JSON.stringify(record).replace(/</g, '\\u003c'),
            injectTo: 'body',
          },
          {
            tag: 'footer',
            attrs: { 'aria-label': 'Build versions', style: 'padding: 1rem; color: var(--muted); font-size: 0.75rem; text-align: center' },
            children: line.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;'),
            injectTo: 'body',
          },
        ]
      },
    },
    {
      name: 'precache-lab',
      apply: 'build',
      configResolved(config) {
        base = config.base
      },
      writeBundle(options, bundle) {
        const outDir = resolve(options.dir ?? 'dist')
        const worker = readFileSync(resolve(outDir, 'sw.js'), 'utf8')
        // Every precached URL is absolute under the build's base, which is also the worker's scope.
        const shell = ['', 'manifest.webmanifest', 'pwa-icon.svg', 'favicon.svg']
        const assets = [...new Set([...shell, ...Object.keys(bundle)].map((name) => base + name))].sort()
        // Version the complete artifact, including public files, with the build's own bytes.
        const hash = createHash('sha256').update(worker)
        for (const asset of assets) {
          hash.update(asset).update(readFileSync(resolve(outDir, asset.slice(base.length) || 'index.html')))
        }
        // The name must carry the worker's CACHE_PREFIX (web/public/sw.js) or activation never sweeps it.
        const build = { cacheName: `safemesh-pwa-${hash.digest('hex')}`, assets }
        writeFileSync(resolve(outDir, 'sw.js'), `const BUILD = ${JSON.stringify(build)}\n${worker}`)
      },
    },
  ],
})
