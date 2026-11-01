# Handoff: Negs on the home screen, and offline

> **Done**, 2026-10-29, in the shared worktree (the owner's choice): see
> `docs/web-wasm.md` item 19 for what was built, measured and tested,
> and `docs/web-wasm-handoff.md`, "The service worker in development".
> The owner said yes to `file_handlers` and to "Save everything". Still
> open: trying it on an iPhone, iPad and Android phone (the home-screen
> app, exporting from it, WebGPU in standalone mode, memory).

For the agent who makes the browser version of Negs installable: added to
an iPhone's or iPad's home screen, or installed as an app from Chrome or
Edge on a computer, Android phone or tablet. It should then open and
develop photographs with no network, like an app. The owner asked for
this on 2026-10-29.

Read first: `docs/web-wasm-handoff.md` (build, serve, test tools, code
map, gotchas) and `docs/web-wasm.md` "Limits and open problems". The MVP
plan is `docs/web-mvp-plan.md`. Its piece 13 ("Download and caching") asked
for a service worker with versioned caches. This work covers that, so
note in piece 13 what you did.

## Where to work

- The browser app is on branch `wasm-investigation`, in the worktree
  `/home/noah/Work/Negs-wasm`. Another agent is building the MVP's
  remaining pieces there (9 Export LUT, 10 Export Preset, then 5), so
  ask the owner whether to use a worktree of your own, branched from
  `wasm-investigation`, or to share this one. Either way, don't push and
  don't merge to `main` until the owner says so. Commits end with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **The owner's uncommitted work** is in that worktree. Leave it
  uncommitted unless they say otherwise: `web/credits.html`,
  `web/package-static.mjs` (the Cloudflare packager), `web/static/`
  (`_headers`, `404.html`, `robots.txt`), `docs/cloudflare.md`, and two
  lines in `web/index.html`. `docs/web-next-handoff.md`, "The owner's
  uncommitted work", says how to commit `index.html` without them. You
  will have to change the packager and `_headers`, so tell the owner what
  you changed there.
- The site is deployed as static files on Cloudflare Pages
  (`docs/cloudflare.md`). There is no server code and there won't be.

## What the app is made of

Measured on 2026-10-29, at commit `3cc95ec`:

| | Size | Loaded |
|---|---|---|
| `index.html`, `app.js`, `border.js`, `controls.js`, `engine.js`, `view.js`, `placeholder.js`, `worker.js`, `credits.html` | ~150 KB | at start |
| `catalog.json` (the stock catalog, made at build time) | small | at start |
| `pkg/` (single-threaded wasm, 8.1 MB) or `pkg-threads/` (9.8 MB, plus wasm-bindgen-rayon's worker snippet) | one of them | at start: `pkg-threads` when `crossOriginIsolated` |
| `data/profiles/*.json` (5 MB in all) | per film and paper chosen | on demand (`fetchData` in `engine.js`) |
| `data/filters/` (176 KB) | some, on demand | on demand |
| `data/luts/spectral_upsampling/hanatos2025_irradiance_xy_tc.npy` (6 MB) | the default LUT | the first render |
| Arctic and Mallett LUTs (6 MB each) | when chosen (piece 5) | on demand |
| `data/borders/` (17 MB: `film.png` 170 KB, a 35mm strip 280–430 KB each, `35mm/material.png` 498 KB for exports) | when a border needs one | on demand (`fetchBorderTexture` in `app.js`; exports through `fetchData`) |
| `sample/DSC_6377.dng` (3.4 MB) | "Try a sample photo" | on demand |

How data arrives: a render that reads a file nobody has registered
reports it in `missing`. `engine.js` then fetches
`new URL("data/<path>", location.origin)` and renders again. These are
plain `fetch`es from the engine's worker, so a service worker that
controls the page and its workers sees them. A photograph never leaves
the device. The page reads it from a file input and keeps it in memory.

## What to build

1. **A web app manifest.** Name "Negs"; `display: standalone`; theme
   and background `#161616` (index.html's `--bg`); `start_url` and
   `scope` relative (`./`), because the site can be served from a
   subpath. Icons come from `assets/negs-icon.svg` or `negs-icon.png`
   (1024 px, the desktop icon): 192 and 512 px, a maskable variant with
   padding, and a 180 px `apple-touch-icon` for iOS. Add the
   `<link rel="manifest">`, `apple-touch-icon` and
   `apple-mobile-web-app-*` / `theme-color` meta tags to `index.html`.
   Check the layout in standalone mode: there's no browser chrome, and
   the safe areas matter (`index.html` already uses
   `env(safe-area-inset-*)` for the phone's sheet). A
   `file_handlers` entry (open RAW, JPEG, PNG and TIFF files with Negs, on
   Chrome for desktop, through `launchQueue`) would be a good addition;
   ask the owner first.
2. **A service worker** (`web/sw.js`, registered from `app.js`):
   - **Precache the shell when it installs:** the HTML, the JS modules,
     `catalog.json`, both wasm builds or just the one this browser will
     use (decide, and measure), the default LUT and the profiles. All the
     profiles are 5 MB, and offline you should still be able to pick any
     film. Precache them, or say why not.
   - **Cache on first use, then serve from the cache:** `data/` (the
     other LUTs, filters, border textures) and the sample. Once something
     has been used, it works offline.
   - **Offer "make everything available offline"**, or don't. Ask the
     owner. It would be about 30 MB more with every border texture and
     LUT.
   - **Versioned caches.** Name the cache after the build. The packager
     already writes `build-info.json` with each file's sha256, which can
     become the precache list and its version. Delete old caches when the
     new worker activates. When an update has been downloaded, tell the
     user unobtrusively (a "New version: reload" message, `showMessage`)
     instead of reloading under them. Never mix files of two builds: the
     wasm and its JS glue must come from the same build.
   - **COOP/COEP.** The threaded engine needs `crossOriginIsolated`,
     which needs `Cross-Origin-Opener-Policy: same-origin` and
     `Cross-Origin-Embedder-Policy: require-corp` on the document and
     on the workers' scripts (`_headers` sets them on Cloudflare; the dev
     server `web/serve.mjs` sets them too). Responses served from the
     cache must keep those headers. Check `crossOriginIsolated` and the
     "12 threads" engine line, offline, from the home screen. A
     service worker can also add the headers for hosts that can't set
     them (`coi-serviceworker`-style). Plan piece 13 asked for that;
     Cloudflare doesn't need it.
   - **Don't break development.** `serve.mjs` sends `no-cache` and the
     checks reload pages constantly. Either don't register on
     `localhost` unless asked (`?sw=1`), or make sure a stale worker can
     never serve old files to `web/checks/*` and `web/bench.sh`. Say which
     you chose in `docs/web-wasm-handoff.md`.
3. **An easy way to install.**
   - **Chrome, Edge and Android:** catch `beforeinstallprompt` and show an
     "Install app" button. The sidebar's footer (`#bottom` in
     `index.html`) or the empty screen are the natural places.
   - **iOS and iPadOS Safari** have no install prompt. Show short
     instructions instead ("Share, then Add to Home Screen"), only in
     Safari, not when already installed (`navigator.standalone` or
     `matchMedia("(display-mode: standalone)")`), and dismissible, with
     the dismissal remembered (`remember` in `app.js`).
   - **Desktop Safari** (macOS Sonoma or later) has File → Add to Dock.
   - Keep it quiet: no nagging, nothing over the photograph.
4. **Storage.** Call `navigator.storage.persist()` once installed, so the
   browser doesn't evict the cache. Show what's cached
   (`navigator.storage.estimate()`), probably on the credits page or in
   the footer, and give a way to clear it.

## Things to check, which may bite

- **Exporting from a home-screen app on iOS.** `exportJpeg` in `app.js`
  clicks an `<a download>` with a blob URL. In standalone mode on iOS that
  may open a viewer or do nothing. Test it, and fall back to the Web Share
  API (`navigator.share({ files })`), which saves to Photos or Files.
- **WebGPU in standalone mode** on iOS 26 and iPadOS: nobody has opened the
  page on a real iPhone or iPad yet (ufw blocks the dev ports from the LAN,
  which is the owner's call; `docs/web-wasm-handoff.md` "On an iPad or
  iPhone" has the HTTPS setup). The CPU engine is the fallback. Ask the
  owner to try the deployed site on their devices, or for the ports.
- **Memory on phones.** An installed app gets no more memory than a tab.
  The page caps previews at 12 MP on touch screens. A 24 MP export can
  reach 2.4 GB of wasm memory with a 35mm border and Instagram
  (`docs/web-wasm.md` item 18). Test an export on a phone.
- **Opening photographs** works offline (file input, drag and drop).
  RAWs and HEIC from the Photos picker are research item R1 in the plan.
- **Offline is the test:** in Chrome DevTools' Application panel, set
  Offline (or `Network.emulateNetworkConditions` over `web/cdp.mjs`).
  Then: reload, open a photograph, change film (a profile not yet used
  must fail gracefully, or be precached), add each border (cached ones
  work; others say why they can't), and export. Also: an update while the
  app is open; two tabs on two versions; a cleared cache.
- **Packaging.** `web/package-static.mjs` lists the files it copies. Add
  the manifest, the icons and `sw.js`. Make sure `_headers` doesn't let
  the browser cache `sw.js` itself (it sends `no-cache` everywhere today,
  which is right for it). Then check the package with
  `node web/package-static.mjs <empty dir>`.

## Done when

- Chrome (desktop and Android) offers Install; iOS Safari shows the Add
  to Home Screen instructions, and the home-screen app opens standalone
  with the right icon and colours.
- Installed, then offline: the app starts (threaded where it was online),
  opens a photograph, develops it on any film and paper, and exports it.
  Anything not cached says so instead of failing silently.
- A new build reaches an installed app without serving mixed files, and
  the user is told when it's ready.
- First-visit and repeat-visit start-up times, online and offline, are in
  `docs/web-wasm.md` (a new item), and `docs/web-wasm-handoff.md` says
  how the service worker behaves in development.
- The existing checks still pass (`web/checks/*.mjs` and `*.js`; see the
  handoff), with the service worker on and off.
