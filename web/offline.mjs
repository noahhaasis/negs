// What the service worker (web/sw.js) keeps for offline use, and the worker
// with its list written in. web/package-static.mjs packages the app with it;
// web/serve.mjs serves the same list for what it serves (development).
import { readFileSync, readdirSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, relative } from "node:path";

// The app's own files, at the site's root as in web/.
export const SHELL = ["index.html", "credits.html", "app.js", "border.js", "crop.js", "crop-editor.js", "controls.js", "engine.js", "view.js", "placeholder.js", "samples.js",
  "raw-wb.js", "presets.js", "preset-ui.js", "tutorial.js", "tutorial.css", "worker.js", "decoder.js", "pool.js", "catalog.json", "manifest.webmanifest", "icons", "fonts", "pkg", "pkg-threads"];
// The spectral table the defaults use; the others are downloaded when chosen.
const DEFAULT_LUT = "data/luts/spectral_upsampling/hanatos2025_irradiance_xy_tc";

// One fixed sample is enough offline. Other photographs remain online-only,
// including when the viewer explicitly asks to "save everything".
export const OFFLINE_SAMPLE = "sample/DSC_6377.dng";

// How a file at `path` (relative to the site's root) is kept: saved when
// the worker installs ("install"), when first used ("use"), or never
// (null: host/build files, generators, or online-only sample photographs).
export function role(path) {
  if (path.startsWith("sample/") && path.endsWith(".dng") && path !== OFFLINE_SAMPLE) return null;
  if (path.endsWith(".py")) return null;
  if (["sw.js", "build-info.json", "_headers", "_redirects", "404.html", "robots.txt"].includes(path)) return null;
  if (path.endsWith(".d.ts") || path.endsWith(".sh")) return null;
  const top = path.split("/")[0];
  if (SHELL.includes(path) || SHELL.includes(top)) return "install";
  if (/^data\/(profiles|filters)\//.test(path) || path.startsWith(DEFAULT_LUT) || /^data\/luts\/.*\.toml$/.test(path)) return "install";
  return "use";
}

// web/sw.js with the list of `assets` ({ path, bytes, sha256 }) in it,
// named by a hash of the list: any changed file makes a new worker, which
// the browser then installs.
export function serviceWorker(source, assets, { dev = false } = {}) {
  const files = {};
  for (const a of assets.toSorted((x, y) => (x.path < y.path ? -1 : 1))) {
    const r = role(a.path);
    if (r) files[a.path] = [a.sha256, a.bytes, r === "install" ? 1 : 0];
  }
  const version = createHash("sha256").update(JSON.stringify(files)).digest("hex").slice(0, 12);
  const build = JSON.stringify({ version, dev, files });
  const out = source.replace(/\/\*BUILD\*\/.*\/\*END\*\//, () => `/*BUILD*/ ${build} /*END*/`);
  if (out === source) throw new Error("web/sw.js has no /*BUILD*/ … /*END*/ to fill");
  return out;
}

const hashes = new Map(); // path → { mtimeMs, size, sha256 }
export function asset(path, file) {
  const { mtimeMs, size } = statSync(file);
  let h = hashes.get(file);
  if (!h || h.mtimeMs !== mtimeMs || h.size !== size) {
    h = { mtimeMs, size, sha256: createHash("sha256").update(readFileSync(file)).digest("hex") };
    hashes.set(file, h);
  }
  return { path, bytes: size, sha256: h.sha256 };
}

// What the dev server serves of what a package would hold: web/'s shell
// and sample, and data/'s profiles, filters, borders and spectral tables.
export function devAssets(web, data) {
  const walk = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
  const files = [];
  for (const name of [...SHELL, "sample"]) {
    const path = join(web, name);
    for (const file of statSync(path).isDirectory() ? walk(path) : [path]) files.push(asset(relative(web, file), file));
  }
  for (const dir of ["profiles", "filters", "borders", "luts/spectral_upsampling"]) {
    for (const file of walk(join(data, dir))) {
      if (dir.startsWith("luts") && !/\.(npy|toml)$/.test(file)) continue;
      files.push(asset(`data/${relative(data, file)}`, file));
    }
  }
  return files.filter((a) => !a.path.endsWith(".py"));
}
