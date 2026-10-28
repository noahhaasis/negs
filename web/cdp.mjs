// Drive the page over the DevTools protocol: node web/cdp.mjs <url> <script.js|-> [waitMs]
// Opens url in a windowed Chrome with WebGPU (as web/bench.sh), prints console
// messages and exceptions, then evaluates the script (an async function body)
// in the page and prints its result. SHOT=<file.png> also saves a screenshot;
// VIEW=<w>x<h>[@<dpr>][m] emulates that viewport (m: a touch phone or tablet).
// The page exposes `negs` for this (web/app.js). Chrome profiles
// go in $CDP_TMP (default: the system temp dir); $CHROME_FLAGS adds flags
// (for example --ignore-certificate-errors for serve.mjs --https).
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [url, scriptFile, waitMs = "4000"] = process.argv.slice(2);
const profile = mkdtempSync(join(process.env.CDP_TMP ?? tmpdir(), "cdp-"));
const port = 9333 + Math.floor(Math.random() * 500);
const chrome = spawn("/opt/google/chrome/chrome", [
  "--ozone-platform=x11", "--window-size=1440,900", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check",
  "--enable-unsafe-webgpu", "--enable-features=Vulkan", "--ignore-gpu-blocklist", `--remote-debugging-port=${port}`,
  ...(process.env.CHROME_FLAGS?.split(" ").filter(Boolean) ?? []), "about:blank",
], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let targets;
for (let i = 0; i < 100; i++) {
  try { targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (targets.some((t) => t.type === "page")) break; } catch {}
  await sleep(100);
}
const ws = new WebSocket(targets.find((t) => t.type === "page").webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0;
const waiting = new Map();
ws.onmessage = ({ data }) => {
  const m = JSON.parse(data);
  if (m.id && waiting.has(m.id)) { waiting.get(m.id)(m); waiting.delete(m.id); }
  else if (m.method === "Runtime.consoleAPICalled") console.log(`[console.${m.params.type}]`, m.params.args.map((a) => a.value ?? a.description).join(" "));
  else if (m.method === "Runtime.exceptionThrown") console.log("[exception]", m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
};
const send = (method, params = {}) => new Promise((r) => { waiting.set(++id, r); ws.send(JSON.stringify({ id, method, params })); });
globalThis.send = send;
await send("Runtime.enable");
await send("Page.enable");
const view = process.env.VIEW?.match(/^(\d+)x(\d+)(?:@([\d.]+))?(m)?$/);
if (view) {
  const [, width, height, dpr = "1", mobile] = view;
  await send("Emulation.setDeviceMetricsOverride", { width: +width, height: +height, deviceScaleFactor: +dpr, mobile: !!mobile });
  if (mobile) await send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 5 });
}
await send("Page.navigate", { url });
await sleep(Number(waitMs));
if (scriptFile && scriptFile !== "-") {
  const body = readFileSync(scriptFile, "utf8");
  const r = await send("Runtime.evaluate", { expression: `(async () => { ${body} })()`, awaitPromise: true, returnByValue: true });
  const failed = r.result?.exceptionDetails;
  console.log(failed ? `[script failed] ${failed.exception?.description ?? failed.text}` : JSON.stringify(r.result?.result?.value ?? r, null, 1));
}
if (process.env.SHOT) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  (await import("node:fs")).writeFileSync(process.env.SHOT, Buffer.from(shot.result.data, "base64"));
}
ws.close();
chrome.kill();
process.exit(0);
