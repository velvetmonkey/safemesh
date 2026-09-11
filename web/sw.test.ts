import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

// Runs web/public/sw.js, the worker source, against an in-memory CacheStorage that
// mimics the origin-wide store a browser gives every application on one origin.
const source = readFileSync(new URL('./public/sw.js', import.meta.url), 'utf8')
const NAMESPACE_A = 'safemesh-pwa-%2Flab%2F-'
const NAMESPACE_B = 'safemesh-pwa-%2Fother%2F-'
const CURRENT = `${NAMESPACE_A}dev` // the name the source picks when no BUILD is prepended

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

async function activate(names: string[], rejectDelete: string[] = [], scope = 'http://lab.example/lab/') {
  const { store, caches } = fakeOrigin(names, rejectDelete)
  const listeners: Record<string, (event: unknown) => void> = {}
  let claimed = false
  const self = {
    location: { href: `${scope}sw.js`, origin: 'http://lab.example' },
    registration: { scope },
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
    const shipped = [`${NAMESPACE_A}old`, `${NAMESPACE_A}older`]
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
      [`${NAMESPACE_A}old`, `${NAMESPACE_A}older`, CURRENT],
      [`${NAMESPACE_A}old`],
    )
    expect(claimed).toBe(true)
    expect(remaining).toEqual([CURRENT, `${NAMESPACE_A}old`])
    expect(warnings).toHaveLength(1)
    expect(warnings[0]).toContain(`${NAMESPACE_A}old`)
  })

  it("does not delete a second deployment's current cache", async () => {
    const { remaining } = await activate([CURRENT, `${NAMESPACE_B}dev`])
    expect(remaining).toEqual([CURRENT, `${NAMESPACE_B}dev`])
  })

  it('uses a root-safe namespace', async () => {
    const { remaining } = await activate(['safemesh-pwa-root-old', 'safemesh-pwa-root-dev'], [], 'http://lab.example/')
    expect(remaining).toEqual(['safemesh-pwa-root-dev'])
  })

  it('retains legacy product-wide caches without a recoverable scope', async () => {
    const { remaining } = await activate(['safemesh-pwa-dev', CURRENT])
    expect(remaining).toEqual(['safemesh-pwa-%2Flab%2F-dev', 'safemesh-pwa-dev'])
  })

  it('production builds name their cache inside the prefix the worker sweeps', () => {
    const config = readFileSync(new URL('./vite.config.ts', import.meta.url), 'utf8')
    expect(config).toContain('cacheName: `safemesh-pwa-${scopeNamespace(base)}-')
  })
})

function fetchWorker() {
  const origin = 'http://lab.example'
  const key = (input: string | { url: string }) => new URL(typeof input === 'string' ? input : input.url, origin).href
  const stores = new Map<string, Map<string, Response>>()
  const writes: Promise<unknown>[] = []
  let barrier = Promise.resolve()
  let release: () => void = () => {}
  const caches = {
    async open(name: string) {
      if (!stores.has(name)) stores.set(name, new Map())
      const entries = stores.get(name)!
      return {
        async match(input: string | Request, options?: { ignoreVary?: boolean }) {
          const response = entries.get(key(input))
          if (response?.headers.has('vary') && !options?.ignoreVary) return undefined
          return response?.clone()
        },
        put(input: string | Request, response: Response) {
          const write = barrier.then(() => { entries.set(key(input), response.clone()) })
          writes.push(write)
          return write
        },
      }
    },
    async match(input: string | Request, options?: { cacheName?: string }) {
      for (const name of options?.cacheName ? [options.cacheName] : stores.keys()) {
        const response = await (await caches.open(name)).match(input)
        if (response) return response
      }
    },
  }
  const listeners: Record<string, (event: unknown) => void> = {}
  let online = true
  let body = 'build A'
  const self = {
    location: { origin }, registration: { scope: `${origin}/lab/` },
    addEventListener(type: string, listener: (event: unknown) => void) { listeners[type] = listener },
  }
  const requests: string[] = []
  const fetch = async (request: Request) => {
    requests.push(request.url)
    if (!online) throw new Error('offline')
    const response = new Response(body, { headers: { 'content-type': 'text/html' } })
    Object.defineProperty(response, 'url', { value: request.url })
    return response
  }
  const build = { cacheName: CURRENT, assets: ['/lab/', '/lab/assets/a.js', '/lab/README.md'] }
  new Function('self', 'caches', 'fetch', `const BUILD = ${JSON.stringify(build)};\n${source}`)(self, caches, fetch)
  return {
    caches, requests, writes,
    offline() { online = false },
    body(value: string) { body = value },
    block() { barrier = new Promise<void>((resolve) => { release = resolve }); return () => release() },
    dispatch(path: string, mode = 'navigate', method = 'GET') {
      let response: Promise<Response | undefined> | undefined
      const lifetime: Promise<unknown>[] = []
      listeners.fetch({ request: { url: key(path), mode, method },
        respondWith(promise: Promise<Response | undefined>) { response = promise },
        waitUntil(promise: Promise<unknown>) { lifetime.push(promise) },
      })
      return { response, lifetime, async finish() { await response; await Promise.all(lifetime); await Promise.all(writes) } }
    },
  }
}

describe('service worker fetch ownership', () => {
  it('defect 1: deep HTML navigation cannot replace the installed shell', async () => {
    const w = fetchWorker(), cache = await w.caches.open(CURRENT)
    await cache.put('/lab/', new Response('build A'))
    w.body('soft 404')
    await w.dispatch('/lab/missing').finish()
    expect(await (await cache.match('/lab/'))!.text()).toBe('build A')
  })
  it('defect 2: build B navigation cannot enter build A cache', async () => {
    const w = fetchWorker(), cache = await w.caches.open(CURRENT)
    await cache.put('/lab/', new Response('build A'))
    w.body('build B')
    await w.dispatch('/lab/').finish()
    w.offline()
    expect(await (await w.dispatch('/lab/').response)!.text()).toBe('build A')
  })
  it('defect 3: neither fallback reads orphan or foreign caches', async () => {
    const w = fetchWorker(), orphan = await w.caches.open('safemesh-pwa-dev')
    await orphan.put('/lab/', new Response('wrong shell'))
    await orphan.put('/lab/data.json', new Response('wrong data'))
    w.offline()
    expect.soft(await w.dispatch('/lab/').response).toBeUndefined()
    expect(await w.dispatch('/lab/data.json', 'cors').response).toBeUndefined()
  })
  it('defect 4: every runtime write is covered by the fetch event lifetime', async () => {
    const w = fetchWorker(), release = w.block()
    const event = w.dispatch('/lab/data.json', 'cors')
    await event.response
    let done = false
    const lifetime = Promise.all(event.lifetime).then(() => { done = true })
    await new Promise((resolve) => setTimeout(resolve, 0))
    const tracked = w.writes.length > 0 && !done
    release()
    await lifetime
    await event.finish()
    expect(tracked).toBe(true)
  })
  it('keeps ignoreVary for owned unqueried build assets', async () => {
    const w = fetchWorker()
    await (await w.caches.open(CURRENT)).put('/lab/assets/a.js', new Response('asset A', { headers: { vary: 'Origin' } }))
    expect(await (await w.dispatch('/lab/assets/a.js', 'cors').response)!.text()).toBe('asset A')
    expect(w.requests).toEqual([])
    await w.dispatch('/lab/assets/a.js?v=1', 'cors').finish()
    expect(w.requests).toContain('http://lab.example/lab/assets/a.js?v=1')
  })
  it('never caches another build asset or a queried asset in this build cache', async () => {
    const w = fetchWorker(), cache = await w.caches.open(CURRENT)
    for (const path of ['/lab/assets/b.js', '/lab/assets/a.js?v=1']) {
      await w.dispatch(path, 'cors').finish()
      expect(await cache.match(path)).toBeUndefined()
    }
  })
  it('keeps non-navigation cache hits and online refresh', async () => {
    const w = fetchWorker(), cache = await w.caches.open(CURRENT)
    await cache.put('/lab/data.json', new Response('cached'))
    w.body('fresh')
    const event = w.dispatch('/lab/data.json', 'cors')
    expect(await (await event.response)!.text()).toBe('cached')
    await event.finish()
    expect(await (await cache.match('/lab/data.json'))!.text()).toBe('fresh')
  })
  it('does not refresh an installed public file through a non-navigation fetch', async () => {
    const w = fetchWorker(), cache = await w.caches.open(CURRENT)
    await cache.put('/lab/README.md', new Response('installed A'))
    w.body('deployed B')
    const event = w.dispatch('/lab/README.md', 'cors')
    expect(await (await event.response)!.text()).toBe('installed A')
    await event.finish()
    expect(await (await cache.match('/lab/README.md'))!.text()).toBe('installed A')
    expect(w.requests).toEqual([])
  })
  it('leaves non-GET requests to the browser', () => {
    const w = fetchWorker()
    expect(w.dispatch('/lab/data.json', 'cors', 'POST').response).toBeUndefined()
    expect(w.requests).toEqual([])
  })
})
