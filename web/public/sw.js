// The registration scope is the Lab base path: / locally, or the mount path on the documentation site.
// It is stable across worker updates and differs between mounts on one origin.
const SCOPE_PATH = new URL(self.registration.scope).pathname
const SCOPE_NAMESPACE = SCOPE_PATH === '/' ? 'root' : encodeURIComponent(SCOPE_PATH)
const BASE = SCOPE_PATH
// Cache Storage is shared by every application served from this origin, so activation may
// only remove caches this Lab named itself. Every name the Lab has ever used carries this
// prefix. Legacy product-wide names have no scope identity, so they are deliberately orphaned
// rather than risk deleting another deployment's data.
const CACHE_PREFIX = 'safemesh-pwa-'
const CACHE_NAMESPACE = `${CACHE_PREFIX}${SCOPE_NAMESPACE}-`
// Production builds prepend BUILD with every emitted asset and a content-derived cache name.
const CACHE_NAME = typeof BUILD === 'undefined' ? CACHE_NAMESPACE + 'dev' : BUILD.cacheName
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
      .then((keys) => keys.filter((key) => key.startsWith(CACHE_NAMESPACE) && key !== CACHE_NAME))
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
