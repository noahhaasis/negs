// Async function body, run by cdp.mjs or golden.html's startGolden().
// Uses the production worker, raw diagnostic readback AND normal RGBA path.
const q = new URLSearchParams(location.search);
const cases = await (await fetch(q.get('cases') ?? '/results/golden-cases.json')).json();
const only = q.get('only')?.split(',');
const backend = q.get('backend') ?? 'gpu';
const threads = q.get('threads') !== '0';
const prefix = q.get('prefix') ?? 'golden';
const worker = new Worker('/worker.js', { type: 'module' });
let id = 0;
const pending = new Map();
const fail = (e) => { for (const p of pending.values()) { clearTimeout(p.timer); p.reject(e); } pending.clear(); };
worker.onerror = e => fail(new Error(e.message));
worker.onmessage = ({data: {id, result, error}}) => {
  const p = pending.get(id);
  if (!p) return;
  clearTimeout(p.timer); pending.delete(id);
  error ? p.reject(new Error(error)) : p.resolve(result);
};
const call = (method, ...args) => new Promise((resolve, reject) => {
  const n = ++id;
  const timer = setTimeout(() => { pending.delete(n); reject(new Error(`${method} timed out`)); }, 180000);
  pending.set(n, {resolve, reject, timer});
  worker.postMessage({id: n, method, args});
});
const save = async (name, body, suffix = '') => {
  const r = await fetch(`/result?name=${name}${suffix}`, {method: 'POST', body});
  if (!r.ok) throw new Error(`Saving ${name}: HTTP ${r.status}`);
};
const digest = async bytes => [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map(x => x.toString(16).padStart(2, '0')).join('');
const rows = [];
const info = {userAgent: navigator.userAgent, secure: isSecureContext, isolated: crossOriginIsolated,
  coarse: matchMedia('(pointer: coarse)').matches, threads, backend, budget: Number(q.get('budget') ?? 0)};
try {
  info.at = new Date().toISOString();
  info.assets = {};
  for (const path of ['/worker.js', '/engine.js', '/decoder.js', threads ? '/pkg-threads/index.js' : '/pkg/spektrafilm_wasm.js',
                      threads ? '/pkg-threads/index_bg.wasm' : '/pkg/spektrafilm_wasm_bg.wasm']) {
    const response = await fetch(path);
    if (!response.ok) throw new Error(`Missing engine asset: ${path}`);
    info.assets[path] = await digest(await response.arrayBuffer());
  }
  Object.assign(info, await call('init', backend, threads, 1));
  if ((backend === 'gpu') !== info.backend.includes('wgpu')) throw new Error(`Wrong backend: ${JSON.stringify(info)}`);
  await call('setDiffusionGridBudget', info.budget);
  let image;
  for (const c of cases) {
    if (only && !only.includes(c.name)) continue;
    if (image !== c.image) {
      const r = await fetch(c.image);
      if (!r.ok) throw new Error(`Missing input ${c.image}: ${r.status}`);
      const bytes = new Uint8Array(await r.arrayBuffer());
      if (await digest(bytes) !== (c.source_sha256 ?? c.input_sha256)) throw new Error(`Input hash mismatch: ${c.image}`);
      if (c.keep_pixels) {
        // The phone's actual short-lived decoder and worker-to-engine handoff.
        const decoder = new Worker('/decoder.js', {type: 'module'});
        const decoded = await new Promise((resolve, reject) => {
          const timer = setTimeout(() => { decoder.terminate(); reject(new Error('decode timed out')); }, 180000);
          decoder.onerror = e => { clearTimeout(timer); reject(new Error(e.message)); };
          decoder.onmessage = ({data}) => { clearTimeout(timer); data.error ? reject(new Error(data.error)) : resolve(data.result); };
          decoder.postMessage({threads});
          decoder.postMessage({bytes, maxDim: 0, maxPixels: c.keep_pixels});
        });
        await save(`${prefix}-${c.source_name}-input`, decoded.pixels, `&format=f32&w=${decoded.width}&h=${decoded.height}`);
        await call('loadDecoded', decoded);
      } else {
        await call('load', bytes, 0, 0);
      }
      image = c.image;
    }
    const budget = c.budget ?? info.budget;
    await call('setDiffusionGridBudget', budget);
    const params = JSON.stringify(c.params);
    const r = await call('renderRaw', c.film, c.paper, params, 1 << 20);
    if (r.backend !== info.backend) throw new Error(`${c.name}: CPU fallback: ${r.backend}`);
    if (r.scan.length !== r.width * r.height * 3 || r.scan.some(x => !Number.isFinite(x))) throw new Error(`${c.name}: invalid scan`);
    if (q.has('repeat')) {
      const again = await call('renderRaw', c.film, c.paper, params, 1 << 20);
      if (again.backend !== r.backend || await digest(again.scan) !== await digest(r.scan)) throw new Error(`${c.name}: not repeatable`);
    }
    const b = await call('render', c.film, c.paper, params, 1 << 20, false, false);
    if (b.backend !== info.backend || b.width !== r.width || b.height !== r.height || b.rgba.length !== r.width * r.height * 4) throw new Error(`${c.name}: invalid normal readback`);
    const name = `${prefix}-${c.name}`;
    await save(name, r.scan, `&format=f32&w=${r.width}&h=${r.height}`);
    await save(name, b.rgba, `&w=${r.width}&h=${r.height}`);
    const row = {name: c.name, case_sha256: c.case_sha256, width: r.width, height: r.height, backend: r.backend, budget, stages: r.stages};
    rows.push(row);
    document.getElementById('status').textContent = JSON.stringify(row);
    console.log('golden', JSON.stringify(row));
  }
  if (!rows.length) throw new Error('No cases ran');
  await save(`${prefix}-run`, JSON.stringify({info, rows}));
  return {info, cases: rows.length};
} finally {
  worker.postMessage({method: 'dispose'});
}
