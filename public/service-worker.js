const CACHE_NAME = 'event-tracker-v1';

// Pre-cache the app shell on install
self.addEventListener('install', function(event) {
  event.waitUntil(
    caches.open(CACHE_NAME).then(function(cache) {
      return cache.addAll(['/tools/', '/tools/manifest.json', '/tools/icon.svg']);
    })
  );
  self.skipWaiting();
});

// Remove old caches on activate
self.addEventListener('activate', function(event) {
  event.waitUntil(
    caches.keys().then(function(cacheNames) {
      return Promise.all(
        cacheNames
          .filter(function(name) { return name !== CACHE_NAME; })
          .map(function(name) { return caches.delete(name); })
      );
    }).then(function() { return self.clients.claim(); })
  );
});

self.addEventListener('fetch', function(event) {
  if (event.request.method !== 'GET') return;

  var url = new URL(event.request.url);
  // Trunk hashes .wasm and .js filenames — use network-first for these
  var isHashedAsset = url.pathname.endsWith('.wasm') ||
                      (url.pathname.endsWith('.js') && !url.pathname.endsWith('sw.js'));

  if (isHashedAsset) {
    // Network-first: update cache on each successful fetch
    event.respondWith(
      fetch(event.request).then(function(response) {
        var clone = response.clone();
        caches.open(CACHE_NAME).then(function(cache) {
          cache.put(event.request, clone);
        });
        return response;
      }).catch(function() {
        return caches.match(event.request);
      })
    );
  } else {
    // Cache-first for HTML/CSS/manifest/icons
    event.respondWith(
      caches.match(event.request).then(function(cached) {
        if (cached) return cached;
        return fetch(event.request).then(function(response) {
          if (response && response.status === 200) {
            var clone = response.clone();
            caches.open(CACHE_NAME).then(function(cache) {
              cache.put(event.request, clone);
            });
          }
          return response;
        }).catch(function() {
          // Offline fallback for navigation
          if (event.request.mode === 'navigate') {
            return caches.match('/tools/');
          }
        });
      })
    );
  }
});
