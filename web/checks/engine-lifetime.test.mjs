// Run: node --test web/checks/engine-lifetime.test.mjs
// wasm-bindgen getters copy into JS, but the Rust result still owns its Vecs
// until free() (or an unpredictable FinalizationRegistry callback).
import test from "node:test";
import assert from "node:assert/strict";
import { client } from "../engine.js";

function result(fields, freed) {
  let alive = true;
  const wrapper = { free() { assert.ok(alive, "not freed twice"); alive = false; freed.push(fields); } };
  for (const [key, value] of Object.entries(fields)) Object.defineProperty(wrapper, key, { get() {
    assert.ok(alive, `${key} read before free`);
    if (value instanceof Error) throw value;
    return ArrayBuffer.isView(value) ? value.slice() : value;
  } });
  return wrapper;
}
const frame = { missing: [], width: 2, height: 1, rgba: new Uint8Array([1, 2, 3, 255, 4, 5, 6, 255]),
  scan: new Float32Array([.1, .2, .3]), presented: false, backend: "test", elapsedMs: 1, stages: "test" };
async function setup(method, responses) {
  const freed = [];
  const call = () => result(responses.shift(), freed);
  const engine = { backend: () => "test", [method]: call };
  const wasm = { Engine: { newCpu: () => engine }, memoryBytes: () => 1 << 20, [method]: call, registerData() {} };
  const host = client(wasm, "cpu");
  await host.start();
  return { host, freed };
}

test("releasing render buffers does not forget the source or display", async () => {
  let released = 0;
  const engine = { releaseRenderBuffers() { released++; }, forget() { throw new Error("source must stay loaded"); } };
  const host = client({ Engine: { newCpu: () => engine } }, "cpu");
  await host.start();
  host.releaseRenderBuffers();
  assert.equal(released, 1);
});

for (const method of ["render", "renderRaw", "original", "addBorder", "balancePrint"]) {
  test(`${method}: explicitly frees a successful WASM result after copying its fields`, async () => {
    const fields = { ...frame, mFilterShift: 2, yFilterShift: 3 };
    const { host, freed } = await setup(method, [fields]);
    const out = await host[method]();
    assert.equal(freed.length, 1);
    assert.equal(out.elapsedMs ?? out.stages, method === "renderRaw" ? "test" : 1);
    if (out.rgba) assert.deepEqual(out.rgba, frame.rgba, "JS copy survives freeing Rust's Vec");
  });
}
for (const method of ["render", "renderRaw", "addBorder", "balancePrint"]) {
  test(`${method}: frees the missing-data result before awaiting a fetch, including a failed fetch`, async (t) => {
    const missing = { ...frame, missing: ["profiles/missing.json"] };
    const { host, freed } = await setup(method, [missing, frame]);
    t.mock.method(globalThis, "fetch", async () => {
      assert.equal(freed.length, 1, "no WASM result retained across network waits");
      return { ok: true, arrayBuffer: async () => new ArrayBuffer(0) };
    });
    await host[method]();
    assert.equal(freed.length, 2, "both the retry and successful result were freed");
    const failed = await setup(method, [missing]);
    t.mock.method(globalThis, "fetch", async () => {
      assert.equal(failed.freed.length, 1);
      throw new Error("offline");
    });
    await assert.rejects(failed.host[method](), /offline/);
  });
}
for (const method of ["render", "renderRaw", "original", "addBorder", "balancePrint"]) {
  test(`${method}: frees even if copying a result field throws`, async () => {
    const { host, freed } = await setup(method, [{ ...frame, rgba: new Error("copy failed"), mFilterShift: new Error("copy failed") }]);
    await assert.rejects(async () => host[method](), /copy failed/);
    assert.equal(freed.length, 1);
  });
}
