// node --test web/checks/samples.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import { createSamplePicker, DEFAULT_SAMPLE } from "../samples.js";

const ids = ["DSC_6377", "DSC_6028", "DSC_6426", "DSC_6008", "DSC_2498", "IMG_0316", "IMG_4416"];
const data = { default: ids[0], samples: ids.map((id) => ({ id, url: `sample/${id}.dng` })) };
const tick = () => new Promise((r) => setImmediate(r));
function deferred() {
  let resolve, reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
}
class Element extends EventTarget {
  hidden = false;
  disabled = false;
  options = [];
  attributes = {};
  ownerDocument = { createElement: () => ({}) };
  replaceChildren(...options) { this.options = options; }
  setAttribute(key, value) { this.attributes[key] = value; }
}
function fixture(fetchCatalog = async () => structuredClone(data)) {
  const container = new Element(), select = new Element(), next = new Element(), note = new Element();
  note.id = "sample-offline";
  let current = null, online = true, catalogCalls = 0;
  const loads = [], errors = [];
  const picker = createSamplePicker({
    container, select, next, note, current: () => current, online: () => online,
    fetchCatalog: () => { catalogCalls++; return fetchCatalog(); },
    onError: (e) => errors.push(e),
    load: (entry, number, signal) => {
      const d = deferred();
      loads.push({ entry, number, signal, ...d });
      signal.addEventListener("abort", () => d.reject(new DOMException("Aborted", "AbortError")));
      return d.promise.then(() => { if (!signal.aborted) current = entry.id; });
    },
  });
  return { picker, container, select, next, note, loads, errors,
    catalogCalls: () => catalogCalls,
    setCurrent(id) { current = id; picker.sync(); },
    setOnline(value) { online = value; picker.sync(); },
  };
}

test("lazy one-click default; numbered choices, Next, direct choice and wraparound", async () => {
  const f = fixture();
  assert.equal(f.container.hidden, true);
  assert.equal(f.catalogCalls(), 0);
  assert.equal(f.loads.length, 0);
  const initial = f.picker.open();
  assert.equal(f.loads.length, 1, "RAW starts without waiting for catalog");
  assert.equal(f.loads[0].entry.url, DEFAULT_SAMPLE.url);
  await tick();
  assert.deepEqual(f.select.options.map((o) => o.textContent), ids.map((_, i) => `Sample ${i + 1}`));
  assert.equal(f.loads.length, 1, "catalog does not trigger other RAWs");
  f.loads[0].resolve(); await initial;
  f.next.dispatchEvent(new Event("click")); await tick();
  assert.equal(f.loads[1].number, 2);
  f.loads[1].resolve(); await tick();
  f.select.value = ids[6]; f.select.dispatchEvent(new Event("change")); await tick();
  assert.equal(f.loads[2].number, 7);
  f.loads[2].resolve(); await tick();
  f.next.dispatchEvent(new Event("click")); await tick();
  assert.equal(f.loads[3].number, 1);
  f.loads[3].resolve(); await tick();
  assert.equal(f.select.value, ids[0]);
  assert.equal(f.catalogCalls(), 1);
  assert.equal(f.errors.length, 0);
});

test("rapid changes abort old downloads; failed selection restores actual photo", async () => {
  const f = fixture();
  const initial = f.picker.open(); await tick(); f.loads[0].resolve(); await initial;
  const second = f.picker.open(ids[1]);
  const third = f.picker.open(ids[2]);
  assert.equal(f.loads[1].signal.aborted, true);
  await second;
  assert.equal(f.select.value, ids[2], "old completion must not change new selection");
  f.loads[2].reject(new Error("Network failed")); await third;
  assert.equal(f.select.value, ids[0]);
  assert.equal(f.container.attributes["aria-busy"], "false");
  assert.equal(f.errors.length, 1);
});

test("opening a local photo cancels a sample and metadata cannot reveal the picker later", async () => {
  const metadata = deferred();
  const f = fixture(() => metadata.promise);
  const initial = f.picker.open();
  await tick();
  f.picker.cancel(); f.setCurrent(null);
  metadata.resolve(structuredClone(data)); await initial; await tick();
  assert.equal(f.loads[0].signal.aborted, true);
  assert.equal(f.container.hidden, true);
  assert.equal(f.loads.length, 1);
  assert.equal(f.errors.length, 0);
});

test("offline choices are restricted to Sample 1; returning online restores them", async () => {
  const f = fixture();
  const initial = f.picker.open(); await tick(); f.loads[0].resolve(); await initial;
  const second = f.picker.open(ids[1]); f.loads[1].resolve(); await second;
  f.setOnline(false);
  assert.equal(f.note.hidden, false);
  assert.equal(f.next.disabled, true);
  assert.deepEqual(f.select.options.map((o) => o.disabled), [false, true, true, true, true, true, true]);
  await f.picker.open(ids[2]);
  assert.equal(f.loads.length, 2, "no online-only RAW requested offline");
  const fallback = f.picker.open();
  f.loads[2].resolve(); await fallback;
  assert.equal(f.select.value, ids[0]);
  f.setOnline(true);
  assert.equal(f.next.disabled, false);
  assert.equal(f.note.hidden, true);
  assert.ok(f.select.options.every((o) => !o.disabled));
});

test("catalog failure does not block default; Next retries metadata", async () => {
  let fail = true;
  const f = fixture(async () => { if (fail) throw new Error("Offline"); return structuredClone(data); });
  const initial = f.picker.open(); await tick(); f.loads[0].resolve(); await initial;
  assert.equal(f.container.hidden, false);
  assert.equal(f.select.value, ids[0]);
  assert.equal(f.next.disabled, false);
  assert.equal(f.errors.length, 1);
  fail = false;
  f.next.dispatchEvent(new Event("click")); await tick();
  assert.equal(f.catalogCalls(), 2);
  assert.equal(f.loads[1].number, 2);
  f.loads[1].resolve(); await tick();
});
