// SP42 Service Worker
// App-shell caching only. No auth, coordination, debug, or API responses are cached.

const SW_VERSION = "2026-03-24.2";
const CACHE_NAME = `sp42-shell-${SW_VERSION}`;
const OFFLINE_FALLBACK = "/offline.html";
const SHELL_ASSETS = [
  "/",
  "/manifest.json",
  OFFLINE_FALLBACK,
  "/icons/sp42-icon-192.svg",
  "/icons/sp42-icon-512.svg",
  "/icons/sp42-icon-maskable.svg",
];

const NETWORK_ONLY_PATTERNS = [
  /\/dev\//,
  /\/dev-auth\//,
  /\/oauth\//,
  /\/coordination\//,
  /\/ws\//,
  /\/debug\//,
  /\/healthz/,
  /\/api\//,
  /stream\.wikimedia\.org/,
  /api\.wikimedia\.org/,
  /meta\.wikimedia\.org/,
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      await cache.addAll(SHELL_ASSETS);
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const cacheNames = await caches.keys();
      await Promise.all(
        cacheNames.filter((name) => name !== CACHE_NAME).map((name) => caches.delete(name)),
      );

      if (self.registration.navigationPreload) {
        try {
          await self.registration.navigationPreload.enable();
        } catch (_) {
          // Ignore browsers that advertise the API but reject enabling it.
        }
      }

      await self.clients.claim();
      await notifyClients({
        type: "SP42_SW_ACTIVE",
        version: SW_VERSION,
        cache: CACHE_NAME,
      });
    })(),
  );
});

self.addEventListener("message", (event) => {
  const type = event.data && event.data.type;

  if (type === "SKIP_WAITING") {
    event.waitUntil(self.skipWaiting());
    return;
  }

  if (type === "GET_VERSION" && event.ports && event.ports[0]) {
    event.ports[0].postMessage({
      type: "SP42_SW_VERSION",
      version: SW_VERSION,
      cache: CACHE_NAME,
    });
    return;
  }

  if (type === "CLIENT_READY") {
    event.waitUntil(
      notifyClients({
        type: "SP42_SW_CLIENT_READY",
        version: SW_VERSION,
      }),
    );
  }
});

self.addEventListener("fetch", (event) => {
  // Defense-in-depth: never cache non-idempotent requests. MediaWiki actions
  // use POST with CSRF tokens in the body, which must never touch the cache.
  if (event.request.method !== "GET" && event.request.method !== "HEAD") {
    return;
  }

  const url = new URL(event.request.url);

  if (NETWORK_ONLY_PATTERNS.some((pattern) => pattern.test(event.request.url))) {
    return;
  }

  if (event.request.mode === "navigate") {
    event.respondWith(networkFirstNavigate(event.request, event.preloadResponse));
    return;
  }

  if (
    url.origin === self.location.origin &&
    (url.pathname.endsWith(".wasm") || url.pathname.endsWith(".js"))
  ) {
    event.respondWith(staleWhileRevalidate(event.request));
    return;
  }

  if (
    url.pathname === "/manifest.json" ||
    url.pathname === OFFLINE_FALLBACK ||
    url.pathname.endsWith(".png") ||
    url.pathname.endsWith(".svg") ||
    url.pathname.endsWith(".ico")
  ) {
    event.respondWith(cacheFirst(event.request));
    return;
  }

  if (url.origin === self.location.origin) {
    event.respondWith(networkFirst(event.request));
  }
});

async function networkFirstNavigate(request, preloadResponsePromise) {
  try {
    const preloadResponse = await preloadResponsePromise;
    if (preloadResponse) {
      return preloadResponse;
    }

    const response = await fetch(request);
    if (response.ok) {
      const cache = await caches.open(CACHE_NAME);
      cache.put(request, response.clone());
      return response;
    }

    const cached = await caches.match(request);
    return cached || response;
  } catch (_) {
    const cached = await caches.match(request);
    if (cached) return cached;
    const offline = await caches.match(OFFLINE_FALLBACK);
    return (
      offline ||
      new Response("SP42 is offline.", {
        status: 503,
        headers: { "Content-Type": "text/plain; charset=utf-8" },
      })
    );
  }
}

async function networkFirst(request) {
  try {
    const response = await fetch(request);
    if (response.ok) {
      const cache = await caches.open(CACHE_NAME);
      cache.put(request, response.clone());
    }
    return response;
  } catch (_) {
    const cached = await caches.match(request);
    return (
      cached ||
      new Response("SP42 is offline.", {
        status: 503,
        headers: { "Content-Type": "text/plain; charset=utf-8" },
      })
    );
  }
}

async function staleWhileRevalidate(request) {
  const cache = await caches.open(CACHE_NAME);
  const cached = await cache.match(request);
  const fetchPromise = fetch(request)
    .then((response) => {
      if (response.ok) {
        cache.put(request, response.clone());
      }
      return response;
    })
    .catch(() => cached ?? new Response("", { status: 503 }));

  return cached ?? fetchPromise;
}

async function cacheFirst(request) {
  const cached = await caches.match(request);
  if (cached) return cached;

  try {
    const response = await fetch(request);
    if (response.ok) {
      const cache = await caches.open(CACHE_NAME);
      cache.put(request, response.clone());
    }
    return response;
  } catch (_) {
    return new Response("", { status: 503 });
  }
}

async function notifyClients(message) {
  const clients = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
  await Promise.all(
    clients.map((client) => {
      client.postMessage(message);
    }),
  );
}
