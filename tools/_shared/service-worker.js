// Shared service worker for every tool under /tools/<name>/.
// Each tool's index.html copies this file next to itself (Trunk `copy-file`),
// so it is served as /tools/<name>/service-worker.js with that scope.
//
// Caching strategy
// - Page navigation: network-first, falling back to the cached page offline.
//   index.html must not come from cache while online: Trunk renames the
//   hashed assets on every build, so a stale page would reference old files.
// - Hashed build assets (name-<16 hex>.js / _bg.wasm / .css): cache-first.
//   Their content never changes under the same name.
// - Everything else (manifest, icon): stale-while-revalidate.
// - After a fresh index.html arrives, hashed assets it no longer references
//   (left over from older builds) are deleted from the cache.

// Cache Storage is shared by every tool on this origin, so each tool only
// ever touches caches carrying its own prefix, derived from the scope path
// (/tools/trackit/ -> "trackit-"; "app-" when served from the root).
var SCOPE = self.registration.scope;
var TOOL = new URL(SCOPE).pathname.split('/').filter(Boolean).pop() || 'app';
var CACHE_PREFIX = TOOL + '-';
var CACHE_NAME = CACHE_PREFIX + 'v4';
// Names used before the shared worker existed, cleaned up on activate.
var LEGACY_PREFIXES = TOOL === 'trackit' ? ['event-tracker-'] : [];

var HASHED_ASSET = /-[0-9a-f]{16}(?:_bg)?\.(?:js|wasm|css)$/;

self.addEventListener('install', function(event) {
  event.waitUntil(
    caches.open(CACHE_NAME).then(function(cache) {
      return cache.addAll([SCOPE, SCOPE + 'manifest.json', SCOPE + 'icon.svg']);
    })
  );
  self.skipWaiting();
});

self.addEventListener('activate', function(event) {
  event.waitUntil(
    caches.keys().then(function(names) {
      return Promise.all(names.filter(function(name) {
        var ours = name.startsWith(CACHE_PREFIX) ||
                   LEGACY_PREFIXES.some(function(p) { return name.startsWith(p); });
        return ours && name !== CACHE_NAME;
      }).map(function(name) { return caches.delete(name); }));
    }).then(function() { return self.clients.claim(); })
  );
});

// Delete cached hashed assets that the given page HTML does not reference.
function pruneHashedAssets(html) {
  return caches.open(CACHE_NAME).then(function(cache) {
    return cache.keys().then(function(requests) {
      return Promise.all(requests.map(function(request) {
        var path = new URL(request.url).pathname;
        var file = path.slice(path.lastIndexOf('/') + 1);
        if (HASHED_ASSET.test(file) && html.indexOf(file) === -1) {
          return cache.delete(request);
        }
      }));
    });
  });
}

function networkFirstPage(event) {
  return fetch(event.request).then(function(response) {
    if (response.ok) {
      var forCache = response.clone();
      var forPrune = response.clone();
      event.waitUntil(
        caches.open(CACHE_NAME)
          .then(function(cache) { return cache.put(SCOPE, forCache); })
          .then(function() { return forPrune.text(); })
          .then(pruneHashedAssets)
      );
    }
    return response;
  }).catch(function() {
    // Offline: the cached page, whatever the query string (e.g. ?add=…).
    return caches.match(SCOPE, { ignoreSearch: true });
  });
}

function cacheFirst(event) {
  return caches.match(event.request).then(function(cached) {
    if (cached) return cached;
    return fetch(event.request).then(function(response) {
      if (response.ok) {
        var clone = response.clone();
        event.waitUntil(caches.open(CACHE_NAME).then(function(c) { return c.put(event.request, clone); }));
      }
      return response;
    });
  });
}

function staleWhileRevalidate(event) {
  return caches.match(event.request).then(function(cached) {
    var refresh = fetch(event.request).then(function(response) {
      if (response.ok) {
        var clone = response.clone();
        caches.open(CACHE_NAME).then(function(c) { return c.put(event.request, clone); });
      }
      return response;
    });
    if (cached) {
      event.waitUntil(refresh.catch(function() {}));
      return cached;
    }
    return refresh;
  });
}

self.addEventListener('fetch', function(event) {
  if (event.request.method !== 'GET') return;
  var url = new URL(event.request.url);
  if (url.origin !== self.location.origin || !url.href.startsWith(SCOPE)) return;

  if (event.request.mode === 'navigate') {
    event.respondWith(networkFirstPage(event));
  } else if (HASHED_ASSET.test(url.pathname)) {
    event.respondWith(cacheFirst(event));
  } else {
    event.respondWith(staleWhileRevalidate(event));
  }
});
