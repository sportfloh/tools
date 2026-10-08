// Registers the shared service worker (tools/_shared/service-worker.js,
// copied next to each tool's index.html). Inlined by Trunk (rel="inline").
if ('serviceWorker' in navigator) {
  window.addEventListener('load', function() {
    navigator.serviceWorker.register('./service-worker.js').catch(function(err) {
      console.log('ServiceWorker registration failed:', err);
    });
  });
}
