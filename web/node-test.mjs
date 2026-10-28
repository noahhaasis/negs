// Headless CPU render: node web/node-test.mjs <image> <film> <paper> <maxDim> <out.ppm> [params.json]
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import init, { Engine, registerData } from "./pkg/spektrafilm_wasm.js";

const here = dirname(fileURLToPath(import.meta.url));
const dataDir = join(here, "..", "data");
const [image, film, paper, maxDim, out, paramsFile] = process.argv.slice(2);

await init({ module_or_path: readFileSync(join(here, "pkg/spektrafilm_wasm_bg.wasm")) });
const engine = Engine.newCpu();
engine.load(readFileSync(image), Number(maxDim));
const params = paramsFile ? readFileSync(paramsFile, "utf8") : "";
let rendered;
for (;;) {
  rendered = await engine.render(film, paper, params, Number(maxDim), false, false);
  if (rendered.missing.length === 0) break;
  for (const path of rendered.missing) {
    console.error(`registering ${path}`);
    registerData(path, readFileSync(join(dataDir, path)));
  }
}
console.error(`${rendered.width}x${rendered.height} on ${rendered.backend} in ${rendered.elapsedMs.toFixed(0)} ms`);
const { width, height, rgba } = rendered;
const rgb = Buffer.alloc(width * height * 3);
for (let i = 0; i < width * height; i++) rgba.subarray(i * 4, i * 4 + 3).forEach((v, c) => (rgb[i * 3 + c] = v));
writeFileSync(out, Buffer.concat([Buffer.from(`P6\n${width} ${height}\n255\n`), rgb]));
