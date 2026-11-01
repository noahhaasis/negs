// The service worker: Negs installed, and offline (docs/web-wasm.md item 19).
//
// Every file the app can use is listed below with its sha256, which
// web/offline.mjs writes in when it packages the app (or the dev server
// serves this file). Files are cached by content (`negs-files`, keyed by
// path and hash), so a new build downloads only what changed, and a page
// only ever gets the files of the build it started with: each page and its
// workers are pinned to their build's list until they close, even after a
// newer build takes over other tabs. Never the network's copy of a newer
// build either: a file whose hash doesn't match isn't served.
//
// The shell, both engine builds, the profiles, filters and the default
// spectral table are saved when the worker installs; anything else (border
// textures, the other tables, the sample) when first used, or all at once
// when the page asks ("save-all"). A new build waits until the page says
// to take over ("skip-waiting"), so a page never reloads under its viewer.
//
// In development (dev: the dev server's list), the network comes first and
// the cache is only the fallback, so a running server always wins.
const BUILD = /*BUILD*/ { version: "unbuilt", dev: true, files: {} } /*END*/;

const FILES = "negs-files", META = "negs-meta";
const scope = self.registration.scope;
const key = (path, sha) => `${new URL(path, scope)}?sha256=${sha}`;
const offline = new BroadcastChannel("negs-offline");

// ---- Builds and the pages using them -----------------------------------------

const manifests = new Map([[BUILD.version, BUILD.files]]);
async function manifest(version) {
  if (!manifests.has(version)) {
    const r = await (await caches.open(META)).match(`${scope}manifest/${version}`);
    manifests.set(version, r ? await r.json() : null);
  }
  return manifests.get(version);
}

// Client id → { version, window }: which build a page (a window) started
// with, and its workers the same, under their window's id.
let pinned = null;
async function pins() {
  if (!pinned) {
    const r = await (await caches.open(META)).match(`${scope}clients`);
    pinned = new Map(r ? Object.entries(await r.json()) : []);
  }
  return pinned;
}
async function pin(id, version, window = id) {
  if (!id) return;
  (await pins()).set(id, { version, window });
  await savePins();
}
async function savePins() {
  await (await caches.open(META)).put(`${scope}clients`, Response.json(Object.fromEntries(await pins())));
}

// Drop what neither this build nor an open page's uses: pins of closed
// windows, their builds' lists and the files only those listed. It runs
// when a build takes over, never while another installs, whose files it
// would take for unused; an older build's files outlive its last page
// until then.
async function prunePins() {
  const open = new Set((await self.clients.matchAll({ type: "window", includeUncontrolled: true })).map((c) => c.id));
  const map = await pins();
  for (const [id, p] of map) if (!open.has(p.window)) map.delete(id);
  await savePins();
  return map;
}
async function cleanUp() {
  const map = await prunePins();
  const live = new Set([BUILD.version, ...[...map.values()].map((p) => p.version)]);
  const meta = await caches.open(META);
  const keep = new Set();
  for (const version of live) {
    for (const [path, [sha]] of Object.entries((await manifest(version)) ?? {})) keep.add(key(path, sha));
  }
  for (const request of await meta.keys()) {
    const m = request.url.match(/\/manifest\/([^/]+)$/);
    if (m && !live.has(m[1])) await meta.delete(request);
  }
  const files = await caches.open(FILES);
  for (const request of await files.keys()) if (!keep.has(request.url)) await files.delete(request);
  // Caches of any other name are an earlier scheme's.
  for (const name of await caches.keys()) if (name.startsWith("negs-") && name !== FILES && name !== META) await caches.delete(name);
}

// ---- Downloading -------------------------------------------------------------

async function sha256(bytes) {
  return [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// A file of this build from the network, kept if it's the one listed.
async function save(path, sha, files) {
  const res = await fetch(new URL(path, scope), { cache: "no-cache" });
  if (!res.ok) throw new Error(`${path}: ${res.status}`);
  const bytes = await res.arrayBuffer();
  if (await sha256(bytes) !== sha) throw Object.assign(new Error(`${path} has changed: a newer Negs is out`), { stale: true });
  await files.put(key(path, sha), new Response(bytes, { status: res.status, statusText: res.statusText, headers: res.headers }));
  return bytes.byteLength;
}

// Download `wanted` ([path, [sha, bytes]]) a few at a time, telling the
// pages how far it got.
async function saveAll(wanted, task) {
  const files = await caches.open(FILES);
  const have = new Set((await files.keys()).map((r) => r.url));
  const missing = wanted.filter(([path, [sha]]) => !have.has(key(path, sha)));
  const total = missing.reduce((n, [, [, bytes]]) => n + bytes, 0);
  let done = 0;
  const queue = [...missing];
  offline.postMessage({ type: "progress", task, done, total });
  await Promise.all(Array.from({ length: 4 }, async () => {
    for (let next; (next = queue.shift());) {
      done += await save(next[0], next[1][0], files);
      offline.postMessage({ type: "progress", task, done, total });
    }
  }));
}

// A file that can't be had fails the install; the pages say so (the
// browser tries again when one next opens) instead of waiting at its percent.
self.addEventListener("install", (event) => event.waitUntil((async () => {
  await (await caches.open(META)).put(`${scope}manifest/${BUILD.version}`, Response.json(BUILD.files));
  await saveAll(Object.entries(BUILD.files).filter(([, [, , precache]]) => precache), "install");
})().catch((e) => {
  offline.postMessage({ type: "failed", task: "install", error: e.message });
  throw e;
})));

self.addEventListener("activate", (event) => event.waitUntil((async () => {
  // Pages that opened before any worker came from the network, which
  // served this build; pages a worker served are pinned already.
  const map = await pins();
  for (const c of await self.clients.matchAll({ type: "window", includeUncontrolled: true })) {
    if (!map.has(c.id)) map.set(c.id, { version: BUILD.version, window: c.id });
  }
  await cleanUp();
  await self.clients.claim();
  offline.postMessage({ type: "activated", version: BUILD.version });
})()));

self.addEventListener("message", (event) => {
  const { type } = event.data ?? {};
  const reply = (message) => event.ports[0]?.postMessage(message);
  if (type === "skip-waiting") return self.skipWaiting();
  if (type === "status") return event.waitUntil(status().then(reply));
  if (type === "save-all") {
    return event.waitUntil(saveAll(Object.entries(BUILD.files), "save-all").then(
      () => reply({ ok: true }),
      (e) => reply({ error: e.message, stale: e.stale })));
  }
});

// What this build has saved: the files installing saves, and everything.
async function status() {
  const have = new Set((await (await caches.open(FILES)).keys()).map((r) => r.url));
  const count = { version: BUILD.version, saved: 0, total: 0, shellMissing: 0 };
  for (const [path, [sha, bytes, precache]] of Object.entries(BUILD.files)) {
    count.total += bytes;
    if (have.has(key(path, sha))) count.saved += bytes;
    else if (precache) count.shellMissing += bytes;
  }
  return count;
}

// ---- Serving -------------------------------------------------------------------

// The threaded engine needs cross-origin isolation; responses keep the
// headers the host sent, and get them where it sent none.
function isolated(res) {
  if (res.headers.has("Cross-Origin-Embedder-Policy") || res.type === "opaque") return res;
  const headers = new Headers(res.headers);
  headers.set("Cross-Origin-Opener-Policy", "same-origin");
  headers.set("Cross-Origin-Embedder-Policy", "require-corp");
  headers.set("Cross-Origin-Resource-Policy", "same-origin");
  return new Response(res.body, { status: res.status, statusText: res.statusText, headers });
}

// Why a file can't be had; the page says so (fetchFailed in engine.js).
const unavailable = (path, why) => new Response(`${path}: ${why}`, {
  status: 503, statusText: why === "updated" ? "Negs was updated" : "Offline",
  headers: { "Content-Type": "text/plain", "X-Negs-Offline": why },
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  const url = new URL(request.url);
  if (request.method !== "GET" || !url.href.startsWith(scope)) return;
  let path = decodeURIComponent(url.pathname.slice(new URL(scope).pathname.length));
  if (request.mode === "navigate" && path === "") path = "index.html";
  // wasm-bindgen-rayon's worker imports the package by its directory.
  if (path.endsWith("/") && BUILD.files[`${path}index.js`]) path += "index.js";
  event.respondWith(respond(event, path));
});

async function respond(event, path) {
  const { request } = event;
  // The build this request's page started with.
  let version = BUILD.version;
  if (request.mode === "navigate") {
    event.waitUntil(pin(event.resultingClientId, version).then(prunePins));
  } else {
    const p = (await pins()).get(event.clientId);
    if (p && (await manifest(p.version))) version = p.version;
    if (request.destination === "worker" || request.destination === "sharedworker") {
      event.waitUntil(pin(event.resultingClientId, version, p?.window ?? event.clientId));
    }
  }
  const entry = (await manifest(version))[path];
  if (!entry) return fetch(request).catch(() => unavailable(path, "offline"));
  const [sha] = entry;
  const files = await caches.open(FILES);
  if (BUILD.dev && version === BUILD.version) {
    try {
      const res = await fetch(request);
      if (res.ok) event.waitUntil(res.clone().arrayBuffer().then(async (bytes) => {
        if (await sha256(bytes) === sha) await files.put(key(path, sha), new Response(bytes, { headers: res.headers }));
      }));
      return res;
    } catch {}
  }
  const cached = await files.match(key(path, sha));
  if (cached) return isolated(cached);
  // Not saved yet: download it, the page reading along (a download's
  // progress), and keep it if it's this build's.
  let res;
  try {
    res = await fetch(new URL(path, scope), { cache: "no-cache" });
  } catch {
    return unavailable(path, "offline");
  }
  if (!res.ok || !res.body) return res;
  const [page, mine] = res.body.tee();
  const checked = new Response(mine).arrayBuffer().then(async (bytes) => {
    if (await sha256(bytes) !== sha) return false;
    await files.put(key(path, sha), new Response(bytes, { status: res.status, statusText: res.statusText, headers: res.headers }));
    return true;
  });
  // Only files that are certainly this build's reach the page: small ones
  // are checked before they're served, and so is the code; a large data
  // file streams, and a newer build's is cut off.
  const code = /\.(js|wasm|json|html)$/.test(path) || entry[1] < 1 << 20;
  if (code) {
    if (!(await checked)) {
      self.registration.update();
      return unavailable(path, "updated");
    }
    return isolated((await files.match(key(path, sha))) ?? unavailable(path, "offline"));
  }
  event.waitUntil(checked.then((ok) => ok || self.registration.update()));
  const reader = page.getReader();
  const body = new ReadableStream({
    async pull(controller) {
      const { done, value } = await reader.read();
      if (!done) return controller.enqueue(value);
      if (await checked) controller.close();
      else controller.error(new Error(`${path} has changed: a newer Negs is out`));
    },
    cancel: (reason) => reader.cancel(reason),
  });
  return isolated(new Response(body, { status: res.status, statusText: res.statusText, headers: res.headers }));
}
