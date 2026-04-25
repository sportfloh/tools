const CACHE_NAME = 'tech-event-announce-v1';

self.addEventListener('install', function(event) {
  var scope = self.registration.scope;
  event.waitUntil(
    caches.open(CACHE_NAME).then(function(cache) {
      return cache.addAll([scope, scope + 'manifest.json', scope + 'icon.svg']);
    })
  );
  self.skipWaiting();
});

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

  var isHashedAsset = url.pathname.endsWith('.wasm') ||
                      (url.pathname.endsWith('.js') && !url.pathname.endsWith('service-worker.js'));
  var isNavigation   = event.request.mode === 'navigate';

  if (isHashedAsset || isNavigation) {
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
          if (event.request.mode === 'navigate') {
            return caches.match(self.registration.scope);
          }
        });
      })
    );
  }
});
