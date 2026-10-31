# Cloudflare test deployment

The browser app runs entirely on the visitor's device. It needs static files
over HTTPS, with COOP/COEP response headers for the threaded engine. No API,
Pages Functions, storage bucket, database or paid Worker is needed.

Cloudflare Pages offers free static requests and bandwidth and a
`<project-name>.pages.dev` address; you do not need to buy a domain.
This package fits the 25 MiB per-file limit and the dashboard's 1,000-file
Direct Upload limit. The packager checks both limits.

## Prepare a release

From the Negs source checkout, with Node.js 22+ and the Rust toolchain installed:

```sh
bash web/build.sh --threads
node web/package-static.mjs
```

Upload `dist/cloudflare/`. The output directory must be empty or absent.
An explicit output directory is also supported:

```sh
node web/package-static.mjs /tmp/negs-release/public
```

The initial handoff to `../negs-web` has the prebuilt site in `public/`.
Its README and `.gitignore` sit outside the upload directory. All `public/`
assets can be tracked in that repository for a buildless Git deployment.

## Deploy through the dashboard

1. In **Workers & Pages**, create a **Pages** application using **Direct Upload**
   (drag and drop). Choose any available project name.
2. Upload the **contents of `public/`**, keeping `_headers` at the deployment
   root next to `index.html`. Do not upload the enclosing repository.
3. Deploy and share the resulting `https://<project-name>.pages.dev` URL.

Alternatively, after authenticating Wrangler:

```sh
npx wrangler login
npx wrangler pages deploy public --project-name YOUR_PROJECT_NAME
```

If you prefer automatic deployment from Git, choose Git integration when
creating the Pages project, with framework **None**, build command `exit 0`,
and build output directory **public**. Cloudflare cannot switch a Direct
Upload project to Git integration later; use a new Pages project for that.

## Verify the hosted app

- Open the HTTPS URL. The film and paper pickers should populate and the
  engine footer should finish starting. On a supported browser it should
  show WebGPU and multiple threads. CPU is the fallback without WebGPU.
- Open a local JPEG, change the film or exposure, and export. Then try a RAW.
- In the browser console, `crossOriginIsolated` should be `true`.
- Confirm `/pkg/spektrafilm_wasm_bg.wasm` has `Content-Type: application/wasm`
  and the page has `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`.
- `?threads=0` selects the single-threaded engine for comparison;
  `?backend=cpu` forces CPU rendering.

The static package patches wasm-bindgen-rayon's directory import to
`../../../index.js`; the development server's directory resolution is not
available on Pages. It excludes generated `.d.ts` declarations: they are
editor metadata, and the dashboard can mistake them for unbuilt TypeScript.
If the uploader reports TypeScript files, use the updated `public/` folder.
Keep `_headers` in every release. `404.html` prevents
missing data files from being served as the app HTML through SPA fallback.
No photos, benchmark outputs, development certificates or test endpoints
are shipped. `noindex` headers and robots.txt discourage indexing; the URL
is publicly accessible to anyone who has it.

## Updating

Rebuild and package into a new empty staging directory, then replace `public/`
with the new output and redeploy. Replace the complete folder to avoid mixing
old JS and WASM. The site requests cache revalidation so repeat testers get
updates. `build-info.json` records the revision and asset checksums.

From the Negs source checkout:

```sh
negs_release=$(mktemp -d)
bash web/build.sh --threads &&
node web/package-static.mjs "$negs_release" &&
rsync -a --delete "$negs_release/" ../negs-web/public/
```

The credits page links to the source code on GitHub. Deployments contain
browser runtime assets, data and license notices; source archives and build
scripts are excluded. The profile/table data retains its attribution and licenses.

Phone/tablet performance and memory limits depend on the device. Safari and
iOS still need real-device testing; the existing investigation's desktop
results do not establish compatibility there.

References checked 2026-10-29:

- [Pages pricing](https://www.cloudflare.com/products/pages/)
- [Pages limits](https://developers.cloudflare.com/pages/platform/limits/)
- [Direct Upload](https://developers.cloudflare.com/pages/get-started/direct-upload/)
- [Response headers](https://developers.cloudflare.com/pages/configuration/headers/)
