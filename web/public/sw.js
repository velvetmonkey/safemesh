// The worker is served from the Lab's root, so its own location names the base path:
// / on the local servers, the mount path when the documentation site bundles the Lab.
const BASE = new URL('./', self.location.href).pathname
// Production builds prepend BUILD with every emitted asset and a content-derived cache name.
const CACHE_NAME = typeof BUILD === 'undefined' ? 'safemesh-pwa-dev' : BUILD.cacheName
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
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))))
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
