// Writes web/catalog.json, the film and paper catalog with the engine's
// defaults, from the engine itself (spektrafilm_core::catalog, as the native
// helper answers `catalog`): node web/make-catalog.mjs. web/build.sh runs it.
// The page can't list data/profiles, and resolving every stock's presets in
// the browser would download all the profiles first.
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import init, { catalog, registerData } from "./pkg/spektrafilm_wasm.js";

const here = dirname(fileURLToPath(import.meta.url));
const profiles = join(here, "..", "data", "profiles");

await init({ module_or_path: readFileSync(join(here, "pkg/spektrafilm_wasm_bg.wasm")) });
const stocks = readdirSync(profiles).filter((f) => f.endsWith(".json")).map((f) => f.slice(0, -5));
for (const stock of stocks) registerData(`profiles/${stock}.json`, readFileSync(join(profiles, `${stock}.json`)));
const out = JSON.parse(catalog(stocks));
if (!out.profiles.length) throw new Error(`No profiles found in ${profiles}`);
out.attribution = "Profile metadata derived from Spektrafilm profiles by Andrea Volpato and contributors, CC BY-SA 4.0. See data/profiles and SPEKTRAFILM_LICENSE.txt.";
writeFileSync(join(here, "catalog.json"), JSON.stringify(out) + "\n");
console.error(`web/catalog.json: ${out.profiles.length} stocks`);
