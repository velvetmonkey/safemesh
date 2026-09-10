import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

// Runs web/public/sw.js, the worker source, against an in-memory CacheStorage that
// mimics the origin-wide store a browser gives every application on one origin.
const source = readFileSync(new URL('./public/sw.js', import.meta.url), 'utf8')
const CURRENT = 'safemesh-pwa-dev' // the name the source picks when no BUILD is prepended

class FakeCache {
  entries = new Map<string, unknown>()
  async put(key: unknown, value: unknown) { this.entries.set(String(key), value) }
  async addAll(keys: string[]) { for (const key of keys) this.entries.set(key, `fetched ${key}`) }
  async match(key: unknown) { return this.entries.get(String(key)) }
}

function fakeOrigin(names: string[], rejectDelete: string[] = []) {
  const store = new Map<string, FakeCache>(names.map((name) => [name, new FakeCache()]))
  const caches = {
    async keys() { return [...store.keys()] },
    async open(name: string) {
      if (!store.has(name)) store.set(name, new FakeCache())
      return store.get(name)!
    },
    async delete(name: string) {
      if (rejectDelete.includes(name)) throw new Error(`delete of ${name} refused`)
      return store.delete(name)
    },
    async match() { return undefined },
  }
  return { store, caches }
}

async function activate(names: string[], rejectDelete: string[] = []) {
  const { store, caches } = fakeOrigin(names, rejectDelete)
  const listeners: Record<string, (event: unknown) => void> = {}
  let claimed = false
  const self = {
    location: { href: 'http://lab.example/sw.js', origin: 'http://lab.example' },
    addEventListener(type: string, listener: (event: unknown) => void) { listeners[type] = listener },
    skipWaiting() {},
    clients: { claim: async () => { claimed = true } },
  }
  const warnings: string[] = []
  const console = { warn: (...args: unknown[]) => warnings.push(args.map(String).join(' ')) }
  new Function('self', 'caches', 'fetch', 'console', source)(self, caches, async () => new Response(), console)
  let pending: Promise<unknown> = Promise.resolve()
  listeners.activate({ waitUntil: (promise: Promise<unknown>) => { pending = promise } })
  await pending
  return { remaining: [...store.keys()].sort(), claimed, warnings }
}

describe('service worker activation', () => {
  it('leaves a cache another application created on the same origin', async () => {
    const { remaining } = await activate(['other-app-shell-v3', CURRENT])
    expect(remaining).toContain('other-app-shell-v3')
  })

  it('still deletes every stale cache this Lab named itself', async () => {
    const shipped = ['safemesh-pwa-mvp-v1', 'safemesh-pwa-antientropy-v1', `safemesh-pwa-${'0'.repeat(64)}`]
    const { remaining } = await activate([...shipped, CURRENT, 'other-app-shell-v3'])
    expect(remaining).toEqual(['other-app-shell-v3', CURRENT])
  })

  it('keeps its own current cache', async () => {
    const { remaining } = await activate([CURRENT])
    expect(remaining).toEqual([CURRENT])
  })

  it('activates and claims clients when the origin holds no caches at all', async () => {
    const { remaining, claimed } = await activate([])
    expect(remaining).toEqual([])
    expect(claimed).toBe(true)
  })

  it('claims clients and finishes the other deletions when one delete rejects', async () => {
    const { remaining, claimed, warnings } = await activate(
      ['safemesh-pwa-mvp-v1', 'safemesh-pwa-antientropy-v1', CURRENT],
      ['safemesh-pwa-mvp-v1'],
    )
    expect(claimed).toBe(true)
    expect(remaining).toEqual([CURRENT, 'safemesh-pwa-mvp-v1'])
    expect(warnings).toHaveLength(1)
    expect(warnings[0]).toContain('safemesh-pwa-mvp-v1')
  })

  it('production builds name their cache inside the prefix the worker sweeps', () => {
    const config = readFileSync(new URL('./vite.config.ts', import.meta.url), 'utf8')
    expect(config).toContain('cacheName: `safemesh-pwa-')
  })
})
