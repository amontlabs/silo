// A service worker the app installs on its origin. In the browser that shows previews it stays
// after the page, and after the computer that served it is gone. It answers every later request of
// that origin itself, whatever the computer now behind the name serves.
self.addEventListener('install', e => self.skipWaiting());
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', e => {
  const u = new URL(e.request.url);
  if (u.pathname === '/' ) e.respondWith(new Response('<!doctype html><title>left behind</title><p>served by a worker an earlier computer left<script>fetch("/ok?what=reborn&d=page-from-old-worker")</script>', {headers: {'content-type': 'text/html'}}));
});
