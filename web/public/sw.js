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

// addAll commits atomically, and each production Request verifies the bytes
// emitted by this build. A deployment racing installation therefore fails closed.
self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE_NAME).then((cache) => cache.addAll(APP_SHELL.map((path) =>
    new Request(path, { cache: 'reload', integrity: typeof BUILD === 'undefined' ? '' : BUILD.integrity[path] }),
  ))))
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
        .then((cached) => cached || fetch(typeof BUILD === 'undefined' ? request :
          new Request(request, { integrity: BUILD.integrity[url.pathname] }))),
    )
    return
  }

  if (request.mode === 'navigate') {
    event.respondWith(
      // Only installation may populate the offline shell. Even BASE can now be a
      // different deployment, a redirect or a soft 404; navigation never writes it.
      fetch(request).catch(() => caches.match(BASE, { cacheName: CACHE_NAME })),
    )
    return
  }

  // Installed public files are immutable too: a fetch for index.html or a
  // manifest must not silently refresh one member of the installed build.
  if (url.origin === self.location.origin && !url.search && APP_SHELL.includes(url.pathname)) {
    event.respondWith(caches.match(request, { cacheName: CACHE_NAME }))
    return
  }

  // Unknown or queried build assets belong to the network's deployment, not
  // this worker's precache. In particular B's hashed assets must not enter A.
  if (url.origin === self.location.origin && url.pathname.startsWith(BASE + 'assets/')) {
    event.respondWith(fetch(request))
    return
  }

  const cached = caches.match(request, { cacheName: CACHE_NAME })
  const network = cached.then(() => fetch(request))
  // Register while dispatching the event, including when a cache hit returns
  // before the refresh finishes. Cache failures do not discard a good response.
  event.waitUntil(network.then(async (response) => {
    if (response.ok) {
      const copy = response.clone()
      const cache = await caches.open(CACHE_NAME)
      await cache.put(request, copy)
    }
  }).catch((error) => console.warn('SafeMesh refresh unavailable.', error)))
  event.respondWith(cached.then((hit) => hit || network.catch(() => undefined)))
})
