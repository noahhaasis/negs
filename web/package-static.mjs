// Package a freshly built browser app for Cloudflare Pages (no server code).
// bash web/build.sh --threads && node web/package-static.mjs [output-directory]
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { SHELL, serviceWorker } from "./offline.mjs";

const web = dirname(fileURLToPath(import.meta.url));
const root = dirname(web);
const output = resolve(process.argv[2] ?? join(root, "dist", "cloudflare"));
// Never replace a checkout or an earlier release accidentally.
if (existsSync(output) && readdirSync(output).length) throw new Error(`Output must be empty: ${output}`);
for (const file of ["catalog.json", "pkg/spektrafilm_wasm_bg.wasm", "pkg-threads/index_bg.wasm"]) {
  if (!existsSync(join(web, file))) throw new Error(`Missing web/${file}; run bash web/build.sh --threads first`);
}
mkdirSync(output, { recursive: true });
// wasm-bindgen also emits editor declarations. They are not runtime assets;
// Cloudflare's dashboard mistakes .d.ts files for TypeScript needing a build.
// The icons' script isn't one either.
const copy = (source, destination = source) => cpSync(join(root, source), join(output, destination), {
  recursive: true, filter: (path) => !path.endsWith(".d.ts") && !path.endsWith(".sh"),
});
// The app (web/offline.mjs's SHELL: its pages, scripts, manifest, icons and
// both engine builds); web/sw.js is written below.
for (const file of SHELL) copy(`web/${file}`, file);
// Sample inputs and their provenance; never ship the development generator.
// offline.mjs retains just the default photograph, not the whole collection.
for (const file of readdirSync(join(web, "sample")).filter((f) => /\.(dng|json|md|txt)$/.test(f))) {
  copy(`web/sample/${file}`, `sample/${file}`);
}
// Border textures, fetched when a border style needs one, with their
// licences and attribution (data/borders/README.md).
for (const dir of ["profiles", "filters", "borders"]) copy(`data/${dir}`);
const luts = "data/luts/spectral_upsampling";
mkdirSync(join(output, luts), { recursive: true });
for (const file of readdirSync(join(root, luts)).filter((f) => /\.(npy|toml)$/.test(f))) copy(`${luts}/${file}`);
for (const file of ["LICENSE", "data/SPEKTRAFILM_LICENSE.txt", "data/CHANGELOG.txt"]) copy(file);
copy("data/SPEKTRAFILM_LICENSE.txt", "SPEKTRAFILM_LICENSE.txt");
// The 35mm edge print's font is compiled into the engine.
copy("crates/spektrafilm-io/fonts/OFL-BarlowCondensed.txt", "OFL-BarlowCondensed.txt");
for (const file of readdirSync(join(web, "static"))) {
  copy(`web/static/${file}`, file);
}

// wasm-bindgen-rayon assumes a bundler/dev server resolves directory imports.
// Static hosts require the actual JavaScript module URL, including its name.
const snippets = join(output, "pkg-threads", "snippets");
const rayon = readdirSync(snippets).filter((f) => f.startsWith("wasm-bindgen-rayon-"));
if (rayon.length !== 1) throw new Error("Expected exactly one wasm-bindgen-rayon snippet");
const helper = join(snippets, rayon[0], "src", "workerHelpers.js");
const original = readFileSync(helper, "utf8");
const fixed = original.replace("import('../../..')", "import('../../../index.js')");
if (fixed === original) throw new Error("Rayon helper changed; check its package import before publishing");
writeFileSync(helper, fixed);

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? files(path) : [path];
  });
}
const asset = (path) => ({
  path: relative(output, path), bytes: statSync(path).size,
  sha256: createHash("sha256").update(readFileSync(path)).digest("hex"),
});
// The service worker, with the hash of every file it keeps for offline use.
const packaged = files(output).map(asset);
writeFileSync(join(output, "sw.js"), serviceWorker(readFileSync(join(web, "sw.js"), "utf8"), packaged));
const assets = [...packaged, asset(join(output, "sw.js"))];
for (const asset of assets) if (asset.bytes > 25 * 1024 ** 2) throw new Error(`Exceeds Cloudflare's 25 MiB limit: ${asset.path}`);
if (assets.length + 1 > 1000) throw new Error("Exceeds the dashboard's 1,000-file upload limit; use Wrangler");
const hasGit = existsSync(join(root, ".git"));
const manifest = {
  sourceCommit: hasGit ? execFileSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).trim() : null,
  sourceChanges: hasGit ? execFileSync("git", ["status", "--porcelain", "--untracked-files=normal"], { cwd: root, encoding: "utf8" }).trim() : "Built without Git metadata",
  builtAt: new Date().toISOString(),
  assets,
};
writeFileSync(join(output, "build-info.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`${output}: ${assets.length + 1} files, ${(assets.reduce((n, a) => n + a.bytes, 0) / 1024 ** 2).toFixed(1)} MiB`);
console.log(`Largest asset: ${assets.toSorted((a, b) => b.bytes - a.bytes)[0].path}`);
