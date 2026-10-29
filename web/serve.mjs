// Dev server for the browser engine: node web/serve.mjs [port] [--https] [--static <dir>]
// Serves web/ at /, the repository's data/ at /data/, and saves what the page POSTs to /result into
// web/results/ (as PPM + JSON), so a headless browser run can be checked from
// the shell. --https serves over TLS (port 8443 by default) with the
// certificate web/dev-cert.sh makes, for opening it on a phone or iPad;
// either server hands out that certificate's authority at /ca.crt.
// /sw.js is the service worker with the list of what this server serves
// (web/offline.mjs). --static serves a package (web/package-static.mjs)
// as it is instead, its own sw.js included, as the host would.
import { createServer } from "node:http";
import { createServer as createSecureServer } from "node:https";
import { readFileSync } from "node:fs";
import { readFile, writeFile, appendFile, mkdir } from "node:fs/promises";
import { join, extname, normalize, dirname, resolve } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { devAssets, serviceWorker } from "./offline.mjs";

const web = dirname(fileURLToPath(import.meta.url));
const data = join(web, "..", "data");
const https = process.argv.includes("--https");
const args = process.argv.slice(2);
const packaged = args.includes("--static") ? resolve(args[args.indexOf("--static") + 1]) : null;
const port = Number(args.find((a, i) => /^\d+$/.test(a) && args[i - 1] !== "--static") ?? (https ? 8443 : 8137));
const certs = process.env.NEGS_DEV_CERTS ?? join(process.env.XDG_DATA_HOME ?? join(homedir(), ".local", "share"), "negs-dev-certs");
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".css": "text/css", ".jpg": "image/jpeg",
  ".png": "image/png", ".webmanifest": "application/manifest+json", ".txt": "text/plain", ".md": "text/plain" };

async function serve(req, res) {
  const url = new URL(req.url, "http://x");
  // A device reload must use the JS and WASM from the same local build.
  res.setHeader("Cache-Control", "no-store");
  // Cross-origin isolation, which SharedArrayBuffer (wasm threads) needs.
  res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  res.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  // An on-device test logs before every action, so a tab termination leaves
  // its last action on the Mac even when the USB debugger disconnects.
  if (req.method === "POST" && url.pathname === "/device-log") {
    const run = url.searchParams.get("run") ?? "";
    if (!/^[a-zA-Z0-9_-]{1,80}$/.test(run)) return res.writeHead(400).end();
    const chunks = [];
    let length = 0;
    for await (const chunk of req) {
      length += chunk.length;
      if (length > 1024 * 1024) return res.writeHead(413).end();
      chunks.push(chunk);
    }
    let record;
    try { record = JSON.parse(Buffer.concat(chunks).toString()); } catch { return res.writeHead(400).end(); }
    if (!record || typeof record !== "object" || Array.isArray(record)) return res.writeHead(400).end();
    await mkdir(join(web, "results"), { recursive: true });
    await appendFile(join(web, "results", `device-${run}.jsonl`), JSON.stringify({ ...record, receivedAt: new Date().toISOString() }) + "\n");
    return res.end("ok");
  }
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
  if (url.pathname === "/sw.js" && !packaged) {
    const source = serviceWorker(await readFile(join(web, "sw.js"), "utf8"), devAssets(web, data), { dev: true });
    return res.writeHead(200, { "Content-Type": "text/javascript" }).end(source);
  }
  const [root, rel] = packaged ? [packaged, url.pathname.slice(1) || "index.html"]
    : url.pathname.startsWith("/data/") ? [data, url.pathname.slice(6)] : [web, url.pathname.slice(1) || "index.html"];
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
const bind = process.env.NEGS_DEV_HOST;
server.listen(port, bind, () => console.log(`${https ? "https" : "http"}://${bind ?? "localhost"}:${port}/`));
