// Chrome/CDP diagnostic, NOT a physical phone/process-memory measurement.
// Tracks GPUBuffer create/destroy calls in engine workers. Counts are explicit
// destruction accounting, not live GPU memory (GC and in-flight work differ).
// VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/resource-lifetimes.mjs 1000
// PHOTO=results/3980774776.dng with ?rawDevelop=bounded tests the large-RAW path.
// RESOURCE_ROUNDS=3 repeats in one page; RESOURCE_EXPECT_FIXED=1 asserts cleanup.
export default async ({ send, evaluate }) => {
  const audit = `
    const sizes = new WeakMap(), labels = new Map();
    let bytes = 0, peakBytes = 0;
    const channel = new BroadcastChannel('negs-gpu-resource-audit');
    const report = () => channel.postMessage({ bytes, peakBytes, labels: Object.fromEntries(labels) });
    const create = GPUDevice.prototype.createBuffer, destroy = GPUBuffer.prototype.destroy;
    GPUDevice.prototype.createBuffer = function(desc) {
      const buffer = create.call(this, desc), label = desc.label ?? '(unlabelled)';
      sizes.set(buffer, { size: desc.size, label });
      bytes += desc.size; peakBytes = Math.max(peakBytes, bytes);
      labels.set(label, (labels.get(label) ?? 0) + desc.size); report();
      return buffer;
    };
    GPUBuffer.prototype.destroy = function() {
      const entry = sizes.get(this);
      if (entry) { bytes -= entry.size; labels.set(entry.label, labels.get(entry.label) - entry.size); sizes.delete(this); }
      const out = destroy.call(this); report(); return out;
    };
  `;
  await send("Page.addScriptToEvaluateOnNewDocument", { source: `
    globalThis.__gpuResources = null;
    globalThis.__auditErrors = [];
    addEventListener('error', e => __auditErrors.push(String(e.error ?? e.message)));
    addEventListener('unhandledrejection', e => __auditErrors.push(String(e.reason)));
    const error = console.error.bind(console);
    console.error = (...args) => { __auditErrors.push(args.map(String).join(' ')); error(...args); };
    const channel = new BroadcastChannel('negs-gpu-resource-audit');
    channel.onmessage = e => { globalThis.__gpuResources = e.data; };
    const NativeWorker = Worker;
    globalThis.Worker = class extends NativeWorker {
      constructor(url, options) {
        const absolute = new URL(url, location.href);
        if (absolute.pathname.endsWith('/worker.js')) {
          const source = ${JSON.stringify(audit)} + '\\nconst queued = []; onmessage = e => queued.push(e); await import(' + JSON.stringify(absolute.href) + '); for (const e of queued) onmessage(e);';
          const blob = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
          super(blob, options); URL.revokeObjectURL(blob);
        } else super(url, options);
      }
    };
  ` });
  await send("Page.reload", { ignoreCache: true });
  await evaluate(`
    globalThis.auditWait = ms => new Promise(r => setTimeout(r, ms));
    globalThis.auditUntil = async predicate => {
      for (let i = 0; i < 2400; i++) { if (predicate()) return; await auditWait(50); }
      throw new Error('resource audit timed out');
    };
    await auditUntil(() => globalThis.negs && !document.getElementById('engine-info').textContent.includes('Starting'));
    globalThis.auditSettle = async n => {
      await auditUntil(() => negs.renders.length > n && negs.idle());
      await auditWait(150);
      const message = document.getElementById('message');
      if (!message.hidden && message.classList.contains('error')) throw new Error(message.textContent);
    };
    globalThis.auditExports = [];
    globalThis.auditCanvases = [];
    const convert = OffscreenCanvas.prototype.convertToBlob;
    OffscreenCanvas.prototype.convertToBlob = function(...args) { auditCanvases.push(this); return convert.apply(this, args); };
    const createURL = URL.createObjectURL.bind(URL);
    URL.createObjectURL = blob => { if (blob.type === 'image/jpeg') auditExports.push(blob.size); return createURL(blob); };
  `);
  const photo = process.env.PHOTO ?? "sample/DSC_6377.dng";
  const snapshots = [];
  async function stage(label, action, render = true) {
    await evaluate(`const n = negs.renders.length; ${action}; ${render ? "await auditSettle(n);" : ""}`);
    const state = await evaluate(`return { label: ${JSON.stringify(label)}, engine: document.getElementById('engine-info').textContent,
      photo: negs.photo(), instances: negs.engineInstances(), frame: negs.renders.at(-1), exports: auditExports,
      exportCanvases: auditCanvases.map(c => [c.width, c.height]), errors: __auditErrors, gpuBuffers: __gpuResources };`);
    if (state.errors.length) throw new Error(JSON.stringify(state.errors));
    if (!state.gpuBuffers) throw new Error("GPU resource monitor did not attach");
    if (!state.engine.startsWith("wgpu") || state.frame?.presented !== true || state.instances !== 1) throw new Error(`Unexpected engine/frame at ${label}`);
    if (process.env.RESOURCE_EXPECT_FIXED === "1") {
      if (Object.entries(state.gpuBuffers?.labels ?? {}).some(([k, v]) => k.startsWith("grain_sums") && v !== 0)) throw new Error(`Undestroyed grain reduction buffers at ${label}`);
      if (state.exportCanvases.some(([w, h]) => w !== 1 || h !== 1)) throw new Error(`Retained export canvas at ${label}`);
    }
    console.log(JSON.stringify(state)); snapshots.push(state);
  }
  for (let round = 0; round < Number(process.env.RESOURCE_ROUNDS ?? 1); round++) {
    await stage(`round ${round + 1}: open`, `await negs.open(${JSON.stringify(photo)})`);
    await stage(`round ${round + 1}: fit edit`, `negs.edit('camera.exposure_compensation_ev', .5)`);
    await stage(`round ${round + 1}: zoom`, `negs.view.actualSize()`);
    await stage(`round ${round + 1}: zoom edit`, `negs.edit('camera.exposure_compensation_ev', 1)`);
    await stage(`round ${round + 1}: fit`, `negs.view.fit()`);
    await stage(`round ${round + 1}: export`, `const count = auditExports.length; document.getElementById('export').click();
      await auditUntil(() => !document.getElementById('export').disabled && negs.idle());
      if (auditExports.length !== count + 1) throw new Error('No new JPEG produced'); await auditWait(150)`, false);
    await stage(`round ${round + 1}: after export`, `negs.edit('camera.exposure_compensation_ev', .3)`);
    await stage(`round ${round + 1}: reopen`, `await negs.open(${JSON.stringify(photo)})`);
  }
  return snapshots;
};
