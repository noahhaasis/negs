// Dev server for the browser engine: node web/serve.mjs [port] [--https]
// Serves web/ at /, the repository's data/ at /data/, and saves what the page POSTs to /result into
// web/results/ (as PPM + JSON), so a headless browser run can be checked from
// the shell. --https serves over TLS (port 8443 by default) with the
// certificate web/dev-cert.sh makes, for opening it on a phone or iPad;
// either server hands out that certificate's authority at /ca.crt.
import { createServer } from "node:http";
import { createServer as createSecureServer } from "node:https";
import { readFileSync } from "node:fs";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { join, extname, normalize, dirname } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const web = dirname(fileURLToPath(import.meta.url));
const data = join(web, "..", "data");
const https = process.argv.includes("--https");
const port = Number(process.argv.slice(2).find((a) => /^\d+$/.test(a)) ?? (https ? 8443 : 8137));
const certs = process.env.NEGS_DEV_CERTS ?? join(process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share"), "negs-dev-certs");
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".css": "text/css", ".jpg": "image/jpeg" };

async function serve(req, res) {
  const url = new URL(req.url, "http://x");
  // Cross-origin isolation, which SharedArrayBuffer (wasm threads) needs.
  res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  res.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  // Revalidate every file, so a tab never mixes a rebuild's files with older ones.
  res.setHeader("Cache-Control", "no-cache");
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
  if (url.pathname === "/ca.crt") {
    try {
      return res.writeHead(200, { "Content-Type": "application/x-x509-ca-cert" }).end(await readFile(join(certs, "ca.crt")));
    } catch {
      return res.writeHead(404).end("No certificate yet: run web/dev-cert.sh");
    }
  }
  const [root, rel] = url.pathname.startsWith("/data/") ? [data, url.pathname.slice(6)] : [web, url.pathname.slice(1) || "index.html"];
  // A directory is its index.js: wasm-bindgen-rayon's worker helper imports
  // the package by its directory, as bundlers resolve it.
  const path = normalize(join(root, decodeURIComponent(rel), rel.endsWith("/") ? "index.js" : ""));
  if (!path.startsWith(root)) return res.writeHead(403).end();
  try {
    const bytes = await readFile(path);
    // The length lets the page show a download's progress (the sample photograph).
    res.writeHead(200, { "Content-Type": types[extname(path)] ?? "application/octet-stream", "Content-Length": bytes.length }).end(bytes);
  } catch {
    res.writeHead(404).end();
  }
}

const server = https
  ? createSecureServer({ key: readFileSync(join(certs, "server.key")), cert: readFileSync(join(certs, "server.crt")) }, serve)
  : createServer(serve);
server.listen(port, () => console.log(`${https ? "https" : "http"}://localhost:${port}/`));
