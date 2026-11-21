// Loma's offline cache. No libraries: the list of files and the
// cache version come from cache-list.js, written at build time by
// scripts/sw-cache-list.sh.
importScripts("cache-list.js");

const APP = "loma-app-" + self.CACHE_VERSION;
const RUNTIME = "loma-runtime"; // web fonts, kept across versions
const DATA = "loma-data"; // the due summary and the last reminder day
const FONT_HOSTS = ["fonts.googleapis.com", "fonts.gstatic.com"];

const scoped = (path) => new URL(path, self.registration.scope).href;
const LAZY = new Set(self.LAZY_FILES.map(scoped));

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(APP);
      // straight from the server: an HTTP-cached copy of a fixed-name file
      // (index.html) could pair an old page with this version's files
      await cache.addAll(["./", ...self.APP_FILES].map((p) => new Request(scoped(p), { cache: "reload" })));
      // the 3D view is large: it is cached the first time it is opened
    })()
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) {
        if (key.startsWith("loma-app-") && key !== APP) await caches.delete(key);
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
  const lazy = LAZY.has(new URL(request.url).href);
  const response = await fetch(request, lazy ? { cache: "reload" } : undefined);
  if (response.ok && lazy) {
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
  } else if (url.href === scoped("loma-due.json")) {
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

// ---- reminders (no server): the app keeps loma-due.json up to date ----

function localDay(d = new Date()) {
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

async function readData(name) {
  const cache = await caches.open(DATA);
  const hit = await cache.match(scoped(name));
  return hit ? hit.json() : null;
}

async function remind() {
  const summary = (await readData("loma-due.json")) || { items: [] };
  const today = localDay();
  // the same rule as the app: each (garden, job) with a date on or before today
  const due = new Map();
  for (const item of summary.items) {
    if (item.date <= today) due.set(item.design_id + "/" + item.task, item);
  }
  if (self.navigator.setAppBadge) {
    try {
      due.size ? await self.navigator.setAppBadge(due.size) : await self.navigator.clearAppBadge();
    } catch (_) {}
  }
  if (!due.size) return;
  const last = await readData("loma-reminded.json");
  if (last && last.day === today) return; // at most one a day
  const items = [...due.values()];
  const gardens = new Set(items.map((i) => i.design_id)).size;
  // name the garden when the jobs come from more than one
  const titles = items.slice(0, 3).map((i) => (gardens > 1 ? `${i.design}: ${i.title}` : i.title));
  const n = due.size;
  await self.registration.showNotification(n === 1 ? "1 garden job due today" : `${n} garden jobs due today`, {
    body: titles.join("\n"),
    tag: "loma-due",
    icon: scoped("icons/icon-192.png"),
    badge: scoped("icons/icon-192.png"),
    data: { url: scoped("./?week=1") },
  });
  const cache = await caches.open(DATA);
  await cache.put(scoped("loma-reminded.json"), new Response(JSON.stringify({ day: today })));
}

self.addEventListener("periodicsync", (event) => {
  if (event.tag === "loma-due") event.waitUntil(remind());
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = (event.notification.data && event.notification.data.url) || scoped("./?week=1");
  event.waitUntil(
    (async () => {
      const windows = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const client of windows) {
        if ("navigate" in client) {
          await client.focus();
          return client.navigate(url);
        }
      }
      return self.clients.openWindow(url);
    })()
  );
});
