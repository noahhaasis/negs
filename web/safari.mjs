// Physical Safari tests, without Selenium/npm dependencies. Start Apple's driver:
//   safaridriver --port 18138 --diagnose
//   SAFARI_DEVICE_UDID=<udid> node web/safari.mjs create
//   SAFARI_DEVICE_TYPE=iPhone SAFARI_TEST_DIR=target/iphone-tests node web/safari.mjs create
//   node web/safari.mjs navigate https://example.com/
//   node web/safari.mjs upload ~/Downloads/photo.raf
//   node web/safari.mjs probe
//   node web/safari.mjs run web/checks/ipad.mjs
// SAFARI_TEST_DIR holds session.json, host-side JSONL logs, and screenshots.
// SAFARI_SCREENSHOTS=0 disables ALL captures, including failure screenshots
// (important on devices where screenshot capture kills webinspectord).
// The upload uses the page's file input change path; the native Files picker
// and system gestures must be checked separately.
import { appendFileSync, mkdirSync, readFileSync, writeFileSync, unlinkSync } from "node:fs";
import { resolve, join, basename } from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

const root = resolve(process.env.SAFARI_TEST_DIR ?? "target/ipad-tests");
mkdirSync(root, { recursive: true });
const sessionPath = join(root, "session.json");
let session;
try { session = JSON.parse(readFileSync(sessionPath, "utf8")); } catch {}
const driver = process.env.SAFARI_DRIVER_URL ?? session?.driver ?? "http://127.0.0.1:18138";
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
function log(event, details = {}) {
  const row = { at: new Date().toISOString(), event, ...details };
  appendFileSync(join(root, "events.jsonl"), JSON.stringify(row) + "\n");
  console.log(JSON.stringify(row));
}
async function request(method, path, body) {
  const response = await fetch(`${driver}${path}`, {
    method, headers: { "Content-Type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(45000),
  });
  const result = await response.json();
  if (!response.ok || result.value?.error) {
    throw new Error(`${result.value?.error ?? response.status}: ${result.value?.message ?? JSON.stringify(result)}`);
  }
  return result.value;
}
function call(method, path, body) {
  if (!session?.id) throw new Error("No Safari session. Run create first.");
  return request(method, `/session/${session.id}${path}`, body);
}
const execute = (script, ...args) => call("POST", "/execute/sync", { script, args });
async function probe() {
  return execute(`
    const n = globalThis.negs, t = globalThis.__negsTest;
    return {
      url: location.href, timeOrigin: performance.timeOrigin, epoch: t?.epoch,
      userAgent: navigator.userAgent, secure: isSecureContext, isolated: crossOriginIsolated,
      gpu: !!navigator.gpu, cores: navigator.hardwareConcurrency,
      coarse: matchMedia('(pointer: coarse)').matches,
      engine: document.getElementById('engine-info')?.textContent,
      engineInstances: n?.engineInstances?.(),
      photo: n?.photo?.(), idle: n?.idle?.(), frames: n?.renders?.length,
      recentFrames: n?.renders?.slice(-5), badge: n?.badge?.(),
      message: document.getElementById('message')?.hidden === false ? document.getElementById('message').textContent : null,
      errorVisible: document.getElementById('message')?.hidden === false && document.getElementById('message').classList.contains('error'),
      hud: document.getElementById('hud')?.textContent,
      zoom: document.getElementById('zoom-level')?.textContent,
      renderSide: n?.photo?.() ? n.renderSide?.() : null,
      errors: t?.errors, recentLogs: t?.logs.slice(-20),
      exports: t?.exports,
    };
  `);
}
async function instrument() {
  const epoch = await execute(`
    if (globalThis.__negsTest) return __negsTest.epoch;
    const t = globalThis.__negsTest = { epoch: String(performance.timeOrigin), logs: [], errors: [], exports: [] };
    const record = (type, args) => {
      const entry = { at: performance.now(), type, text: args.map(a => a?.stack ?? String(a)).join(' ') };
      t.logs.push(entry);
      if (t.logs.length > 500) t.logs.shift();
      if (type === 'error' || type === 'panic') t.errors.push(entry);
    };
    for (const type of ['log', 'warn', 'error']) {
      const original = console[type].bind(console);
      console[type] = (...args) => { record(type, args); original(...args); };
    }
    addEventListener('error', e => record('error', [e.error ?? e.message]), true);
    addEventListener('unhandledrejection', e => record('error', [e.reason]));
    const createURL = URL.createObjectURL.bind(URL);
    URL.createObjectURL = blob => {
      if (blob.type === 'image/jpeg') t.exports.push({ at: performance.now(), bytes: blob.size, type: blob.type });
      return createURL(blob);
    };
    t.panics = new BroadcastChannel('negs-engine-panic');
    t.panics.onmessage = e => record('panic', [JSON.stringify(e.data)]);
    return t.epoch;
  `);
  session.epoch = epoch;
  writeFileSync(sessionPath, JSON.stringify(session, null, 2));
  return epoch;
}
function assertHealthy(state) {
  if (session.epoch && state.epoch !== session.epoch) throw new Error("Page reloaded or lost the test monitor");
  if (state.errors?.length || state.errorVisible) throw new Error(`Editor failed: ${JSON.stringify(state.errors ?? state.message)}`);
}
async function waitFor(expression, { timeout = 180000, healthy = true } = {}) {
  const started = Date.now();
  let nextProgress = started + 15000;
  for (;;) {
    const result = await execute(`return Boolean(${expression});`);
    const state = await probe();
    if (healthy) assertHealthy(state);
    if (result) return state;
    if (Date.now() - started > timeout) throw new Error(`Timed out waiting for ${expression}: ${JSON.stringify(state)}`);
    if (Date.now() >= nextProgress) {
      log("waiting", { expression, elapsedMs: Date.now() - started, state });
      nextProgress = Date.now() + 15000;
    }
    await wait(750);
  }
}
async function screenshot(name) {
  if (process.env.SAFARI_SCREENSHOTS === "0") {
    log("screenshot-skipped", { name, reason: "SAFARI_SCREENSHOTS=0" });
    return null;
  }
  const data = await call("GET", "/screenshot");
  const path = join(root, `${name.replace(/[^a-zA-Z0-9_-]/g, "-")}.png`);
  writeFileSync(path, Buffer.from(data, "base64"));
  return path;
}
async function step(label, action, { settle = true, timeout = 180000 } = {}) {
  const before = await probe();
  assertHealthy(before);
  log("step-start", { label, state: before });
  const started = Date.now();
  await action(before);
  const after = settle ? await waitFor(`negs.renders.length > ${before.frames} && negs.idle()`, { timeout }) : await probe();
  assertHealthy(after);
  log("step-pass", { label, elapsedMs: Date.now() - started, state: after });
  return after;
}
async function upload(path) {
  const bytes = readFileSync(path);
  log("upload-start", { name: basename(path), bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") });
  await execute("globalThis.__negsUpload = []; return true;");
  for (let offset = 0; offset < bytes.length; offset += 512 * 1024) {
    await execute(`
      const text = atob(arguments[0]), bytes = new Uint8Array(text.length);
      for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
      __negsUpload.push(bytes);
      return __negsUpload.length;
    `, bytes.subarray(offset, offset + 512 * 1024).toString("base64"));
  }
  await execute(`
    const transfer = new DataTransfer();
    transfer.items.add(new File(__negsUpload, arguments[0]));
    delete globalThis.__negsUpload;
    const input = document.getElementById('file');
    input.files = transfer.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
    return { name: input.files[0].name, bytes: input.files[0].size };
  `, basename(path));
  log("upload-submitted");
}
async function element(selector) {
  const found = await call("POST", "/element", { using: "css selector", value: selector });
  return found["element-6066-11e4-a52e-4f735466cecf"];
}
async function click(selector) { return call("POST", `/element/${await element(selector)}/click`, {}); }
const api = { root, log, wait, call, execute, probe, instrument, assertHealthy, waitFor, screenshot, step, upload, click };

const [command = "probe", argument] = process.argv.slice(2);
try {
  if (command === "create") {
    if (session?.id) throw new Error("A session file exists; end the existing session or use a new SAFARI_TEST_DIR");
    const capabilities = { browserName: "Safari", platformName: process.env.SAFARI_PLATFORM ?? "iOS" };
    if (capabilities.platformName.toLowerCase() === "ios") {
      capabilities["safari:deviceType"] = process.env.SAFARI_DEVICE_TYPE ?? "iPad";
      capabilities["safari:useSimulator"] = false;
      if (process.env.SAFARI_DEVICE_UDID) capabilities["safari:deviceUDID"] = process.env.SAFARI_DEVICE_UDID;
    }
    if (argument === "--insecure") capabilities.acceptInsecureCerts = true;
    const result = await request("POST", "/session", { capabilities: { alwaysMatch: capabilities } });
    session = { id: result.sessionId, driver, capabilities: result.capabilities };
    writeFileSync(sessionPath, JSON.stringify(session, null, 2));
    await call("POST", "/timeouts", { implicit: 0, pageLoad: 45000, script: 30000 });
    log("session-created", { session });
  } else if (command === "navigate" || command === "page") {
    log("navigate-start", { url: argument });
    await call("POST", "/url", { url: argument });
    session.epoch = undefined;
    if (command === "navigate") {
      await waitFor("!!globalThis.negs && !document.getElementById('engine-info').textContent.includes('Starting')", { healthy: false });
      await instrument();
    }
    log("navigate-pass", { state: await probe() });
  } else if (command === "upload") {
    await instrument();
    await step(`open ${basename(argument)}`, () => upload(argument));
  } else if (command === "open-url") {
    await instrument();
    await step(`open ${argument}`, () => execute("negs.open(arguments[0]).catch(e => console.error(e));", argument));
  } else if (command === "probe") {
    log("probe", { state: await probe() });
  } else if (command === "screenshot") {
    log("screenshot", { path: await screenshot(argument ?? "ipad") });
  } else if (command === "state") {
    const state = await execute("return negs.state();");
    const path = resolve(argument ?? join(root, "edit.json"));
    writeFileSync(path, JSON.stringify(state.params, null, 2));
    log("state-saved", { path, film: state.film, paper: state.paper });
  } else if (command === "eval") {
    const script = readFileSync(argument === "-" ? 0 : argument, "utf8");
    log("eval", { result: await execute(script) });
  } else if (command === "run") {
    await instrument();
    log("suite-start", { script: argument });
    const result = await (await import(pathToFileURL(resolve(argument)))).default(api);
    log("suite-pass", { result });
  } else if (command === "end") {
    await call("DELETE", "");
    unlinkSync(sessionPath);
    log("session-ended");
  } else throw new Error(`Unknown command: ${command}`);
} catch (error) {
  log("failure", { command, message: error.message, stack: error.stack });
  try { log("failure-state", { state: await probe() }); } catch (e) { log("probe-failed", { message: e.message }); }
  try { log("failure-screenshot", { path: await screenshot(`failure-${Date.now()}`) }); } catch (e) { log("screenshot-failed", { message: e.message }); }
  process.exitCode = 1;
}
