// Garden Planner's offline cache. No libraries: the list of files and the
// cache version come from cache-list.js, written at build time by
// scripts/sw-cache-list.sh.
importScripts("cache-list.js");

const APP = "garden-app-" + self.CACHE_VERSION;
const RUNTIME = "garden-runtime"; // web fonts, kept across versions
const DATA = "garden-data"; // the due summary and the last reminder day
const FONT_HOSTS = ["fonts.googleapis.com", "fonts.gstatic.com"];

const scoped = (path) => new URL(path, self.registration.scope).href;
const LAZY = new Set(self.LAZY_FILES.map(scoped));

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(APP);
      await cache.addAll(["./", ...self.APP_FILES].map(scoped));
      // the 3D view is large: cache it if we can, otherwise when first opened
      try {
        await cache.addAll([...LAZY]);
      } catch (_) {}
    })()
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) {
        if (key.startsWith("garden-app-") && key !== APP) await caches.delete(key);
      }
      await self.clients.claim();
    })()
  );
});

// The page asks a waiting version to take over only at a safe moment.
self.addEventListener("message", (event) => {
  if (event.data && event.data.type === "skip-waiting") self.skipWaiting();
});

async function fromApp(request) {
  const cache = await caches.open(APP);
  const hit = await cache.match(request, { ignoreSearch: true });
  if (hit) return hit;
  const response = await fetch(request);
  if (response.ok && LAZY.has(new URL(request.url).href)) {
    await cache.put(request, response.clone());
  }
  return response;
}

async function staleWhileRevalidate(request) {
  const cache = await caches.open(RUNTIME);
  const hit = await cache.match(request);
  const fresh = fetch(request)
    .then((response) => {
      if (response.ok || response.type === "opaque") cache.put(request, response.clone());
      return response;
    })
    .catch(() => hit);
  return hit || fresh;
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (request.mode === "navigate") {
    event.respondWith(
      caches
        .open(APP)
        .then((c) => c.match(scoped("./")))
        .then((hit) => hit || fetch(request))
    );
  } else if (url.href === scoped("garden-due.json")) {
    event.respondWith(
      caches
        .open(DATA)
        .then((c) => c.match(url.href))
        .then((hit) => hit || new Response('{"items":[]}', { headers: { "content-type": "application/json" } }))
    );
  } else if (FONT_HOSTS.includes(url.hostname)) {
    event.respondWith(staleWhileRevalidate(request));
  } else if (url.origin === self.location.origin) {
    event.respondWith(fromApp(request));
  }
});
