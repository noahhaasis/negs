// Dev server for the browser engine: node web/serve.mjs [port]
// Serves web/ at /, the repository's data/ at /data/ and linux/catalog.json
// at /catalog.json, and saves what the page POSTs to /result into
// web/results/ (as PPM + JSON), so a headless browser run can be checked from
// the shell.
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { join, extname, normalize, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const web = dirname(fileURLToPath(import.meta.url));
const data = join(web, "..", "data");
const linux = join(web, "..", "linux");
const port = Number(process.argv[2] ?? 8137);
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".css": "text/css" };

createServer(async (req, res) => {
  const url = new URL(req.url, "http://x");
  // Cross-origin isolation, which SharedArrayBuffer (wasm threads) needs.
  res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  res.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  if (req.method === "POST" && url.pathname === "/result") {
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const body = Buffer.concat(chunks);
    const name = url.searchParams.get("name") ?? "result";
    const [w, h] = ["w", "h"].map((k) => Number(url.searchParams.get(k)));
    await mkdir(join(web, "results"), { recursive: true });
    if (w && h) {
      const rgb = Buffer.alloc(w * h * 3);
      for (let i = 0; i < w * h; i++) body.copy(rgb, i * 3, i * 4, i * 4 + 3);
      await writeFile(join(web, "results", `${name}.ppm`), Buffer.concat([Buffer.from(`P6\n${w} ${h}\n255\n`), rgb]));
    } else {
      await writeFile(join(web, "results", `${name}.json`), body);
    }
    console.log(`saved ${name}`);
    return res.end("ok");
  }
  // catalog.json: the film and paper catalog the Linux app ships.
  const [root, rel] = url.pathname.startsWith("/data/") ? [data, url.pathname.slice(6)]
    : url.pathname === "/catalog.json" ? [linux, "catalog.json"] : [web, url.pathname.slice(1) || "index.html"];
  // A directory is its index.js: wasm-bindgen-rayon's worker helper imports
  // the package by its directory, as bundlers resolve it.
  const path = normalize(join(root, decodeURIComponent(rel), rel.endsWith("/") ? "index.js" : ""));
  if (!path.startsWith(root)) return res.writeHead(403).end();
  try {
    const bytes = await readFile(path);
    res.writeHead(200, { "Content-Type": types[extname(path)] ?? "application/octet-stream" }).end(bytes);
  } catch {
    res.writeHead(404).end();
  }
}).listen(port, () => console.log(`http://localhost:${port}/`));
