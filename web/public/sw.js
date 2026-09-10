// The worker is served from the Lab's root, so its own location names the base path:
// / on the local servers, the mount path when the documentation site bundles the Lab.
const BASE = new URL('./', self.location.href).pathname
// Cache Storage is shared by every application served from this origin, so activation may
// only remove caches this Lab named itself. Every name the Lab has ever used carries this
// prefix: safemesh-pwa-mvp-v1, safemesh-pwa-antientropy-v1, safemesh-pwa-dev and the
// content-hashed production names from vite.config.ts. Caches from builds before this rule
// therefore still count as the Lab's own and are swept as stale; a cache named by any other
// application on the origin is left alone. Two Lab deployments on one origin share the prefix
// deliberately and evict each other's stale caches.
const CACHE_PREFIX = 'safemesh-pwa-'
// Production builds prepend BUILD with every emitted asset and a content-derived cache name.
const CACHE_NAME = typeof BUILD === 'undefined' ? CACHE_PREFIX + 'dev' : BUILD.cacheName
const APP_SHELL = typeof BUILD === 'undefined'
  ? ['', 'README.md', 'manifest.webmanifest', 'pwa-icon.svg', 'favicon.svg'].map((name) => BASE + name)
  : BUILD.assets

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE_NAME).then((cache) => cache.addAll(APP_SHELL)))
  self.skipWaiting()
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => keys.filter((key) => key.startsWith(CACHE_PREFIX) && key !== CACHE_NAME))
      // One cache refusing deletion stays for the next activation; it must not stop the
      // remaining deletions or leave open pages uncontrolled.
      .then((stale) => Promise.allSettled(stale.map((key) => caches.delete(key))).then((results) => {
        results.forEach((result, index) => {
          if (result.status === 'rejected') console.warn(`SafeMesh could not delete stale cache ${stale[index]}.`, result.reason)
        })
      }))
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('fetch', (event) => {
  const request = event.request
  if (request.method !== 'GET') return

  const url = new URL(request.url)
  if (url.origin === self.location.origin && !url.search &&
      url.pathname.startsWith(BASE + 'assets/') && APP_SHELL.includes(url.pathname)) {
    // These build assets are identical static bytes. Vite's Vary: Origin must not
    // make module/CSS requests miss entries installed with cache.addAll().
    event.respondWith(
      caches.open(CACHE_NAME)
        .then((cache) => cache.match(request, { ignoreVary: true }))
        .then((cached) => cached || fetch(request)),
    )
    return
  }

  if (request.mode === 'navigate') {
    event.respondWith(
      fetch(request)
        .then((response) => {
          if (response.ok) {
            const copy = response.clone()
            const contentType = response.headers.get('content-type') || ''
            // The root key is the offline application fallback, so only an HTML
            // shell may replace it. Other navigation responses retain their own URL.
            const cacheKey = contentType.includes('text/html') ? BASE : response.url
            caches.open(CACHE_NAME).then((cache) => cache.put(cacheKey, copy))
          }
          return response
        })
        .catch(() => caches.match(BASE)),
    )
    return
  }

  event.respondWith(
    caches.match(request).then((cached) => {
      const network = fetch(request)
        .then((response) => {
          if (response.ok) {
            const copy = response.clone()
            caches.open(CACHE_NAME).then((cache) => cache.put(request, copy))
          }
          return response
        })
        .catch(() => cached)
      return cached || network
    }),
  )
})
