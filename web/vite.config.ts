import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    {
      name: 'precache-lab',
      apply: 'build',
      writeBundle(options, bundle) {
        const outDir = resolve(options.dir ?? 'dist')
        const worker = readFileSync(resolve(outDir, 'sw.js'), 'utf8')
        const shell = ['/', '/manifest.webmanifest', '/pwa-icon.svg', '/favicon.svg']
        const assets = [...new Set([...shell, ...Object.keys(bundle).map((name) => `/${name}`)])].sort()
        // Version the complete artifact, including public files, with the build's own bytes.
        const hash = createHash('sha256').update(worker)
        for (const asset of assets) {
          hash.update(asset).update(readFileSync(resolve(outDir, asset === '/' ? 'index.html' : asset.slice(1))))
        }
        const build = { cacheName: `safemesh-pwa-${hash.digest('hex')}`, assets }
        writeFileSync(resolve(outDir, 'sw.js'), `const BUILD = ${JSON.stringify(build)}\n${worker}`)
      },
    },
  ],
})
