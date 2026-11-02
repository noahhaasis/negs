// Node CDP adapter. Actual editor in application track; Engine.renderRaw
// on verified float pixels in separately labelled diagnostic tracks.
import { readFileSync, writeFileSync, mkdirSync, existsSync, renameSync, symlinkSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { createHash } from 'node:crypto';

const out = resolve(process.env.ACCURACY_RUN ?? '');
const track = process.env.ACCURACY_TRACK ?? 'application';
const config = JSON.parse(readFileSync(join(out, 'config.json')));
const provenance = JSON.parse(readFileSync(join(out, 'provenance.json')));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const write = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2));

export default async ({ evaluate }) => {
  // Refuse a server with a different build. New CDP profile: no old SW/cache.
  for (const [path, expected] of Object.entries(provenance.wasm_assets)) {
    const response = await fetch(config.server + '/' + path.replace(/^web\//, ''));
    if (!response.ok || hash(Buffer.from(await response.arrayBuffer())) !== expected) throw Error(`server asset changed: ${path}`);
  }
  await evaluate(`globalThis.waitAccuracy = ms => new Promise(r => setTimeout(r, ms));
    globalThis.settleAccuracy = async () => {
      const end = performance.now() + 240000;
      while (performance.now() < end) {
        if (globalThis.negs?.source()?.current && !negs.source()?.busy && negs.idle() && !negs.presetBusy()) {
          await waitAccuracy(200);
          if (negs.idle() && negs.source()?.current) return;
        }
        await waitAccuracy(100);
      }
      throw Error('editor did not settle: '+document.getElementById('message').textContent);
    };`);
  if (track !== 'application') {
    await evaluate(`globalThis.accuracyWasm = await import('/pkg/spektrafilm_wasm.js');
      await accuracyWasm.default();
      const {client} = await import('/engine.js');
      globalThis.accuracyEngine = client(accuracyWasm, 'gpu');
      await accuracyEngine.start();`);
  }
  let image = null;
  const records = [];
  for (const c of config.cases) {
    const dest = join(out, 'renders', track, c.name, 'web');
    if (existsSync(dest)) throw Error(`refusing to overwrite ${dest}`);
    mkdirSync(dest, { recursive: true });
    console.log('START', track, c.name);
    let record = { status: 'error', engine: 'web', case: c.name, track };
    try {
      const text = readFileSync(c.preset, 'utf8');
      if (hash(readFileSync(c.image)) !== c.raw_sha256 || hash(Buffer.from(text)) !== c.preset_sha256) throw Error('input identity changed');
      const postName = `${config.token}-${track}-${c.name}`;
      if (track === 'application') {
        if (image !== c.image) {
          await evaluate(`await negs.open(${JSON.stringify('/' + c.url)}); await settleAccuracy();`);
          image = c.image;
        }
        record = await evaluate(`
          negs.reset(); await settleAccuracy();
          const before = negs.state();
          if (!await negs.applyPreset(${JSON.stringify(text)}, ${JSON.stringify(c.label)})) throw Error('preset import failed');
          await settleAccuracy();
          const source = negs.source(), photo = negs.photo(), state = negs.state();
          if (!source.raw || !source.current || source.needsDevelop) throw Error('source is not current RAW');
          if (${JSON.stringify(c.image_id)} === 'DSC_2498' && (photo.width !== 4912 || photo.height !== 7360)) throw Error('Nikon full RAW assertion failed');
          const r = await negs.exportBytes();
          if (!r.rgba?.length || !r.backend?.startsWith('wgpu')) throw Error('unexpected export backend '+r.backend);
          if (r.width !== photo.width || r.height !== photo.height) throw Error('export is capped/resized, not full source');
          const posted = await fetch('/result?name=${postName}&w='+r.width+'&h='+r.height,{method:'POST',body:r.rgba});
          if (!posted.ok) throw Error('capture POST failed');
          return {status:'ok', before, state, effective_params:state.params, source, photo, source_dimensions:[photo.width,photo.height],
            width:r.width,height:r.height,backend:r.backend,elapsedMs:r.elapsedMs,stages:r.stages,
            border:negs.border(),crop:negs.crop(),userAgent:navigator.userAgent,crossOriginIsolated,
            hardwareConcurrency:navigator.hardwareConcurrency,instances:negs.engineInstances(),
            actualEngineInfo:document.getElementById('engine-info').textContent,
            engineSelection:document.getElementById('engine').value,wb:source.wb,decoder:source.colour.summary,
            resources:performance.getEntriesByType('resource').map(r=>r.name).filter(n=>/pkg|worker/.test(n))};`);
      } else {
        const fixture = join(out, 'fixtures', c.fixture_id);
        const meta = JSON.parse(readFileSync(fixture + '.json'));
        if (hash(readFileSync(fixture + '.f32')) !== meta.rgb_sha256) throw Error('fixture identity changed');
        const url = `/results/${config.token}-fixture-${c.fixture_id}.f32`;
        const link = resolve('web', '.' + url);
        if (!existsSync(link)) symlinkSync(fixture + '.f32', link);
        record = await evaluate(`
          const response = await fetch(${JSON.stringify(url)});
          if (!response.ok) throw Error('fixture fetch failed');
          const bytes = await response.arrayBuffer();
          const digest = [...new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))].map(v=>v.toString(16).padStart(2,'0')).join('');
          if (digest !== ${JSON.stringify(meta.rgb_sha256)}) throw Error('browser fixture transfer changed');
          accuracyEngine.loadDecoded({pixels:new Float32Array(bytes),width:${meta.width},height:${meta.height},fullWidth:${meta.width},fullHeight:${meta.height},
            colour:JSON.stringify({source:'manual',encoding:'Linear sRGB',summary:'Identical float fixture',automatic:'Identical float fixture',warning:null,scene_referred:true,linear_srgb:false})});
          const imported = await accuracyEngine.importPreset(${JSON.stringify(text)}, '');
          const params = JSON.parse(imported.params);
          Object.assign(params.io,{input_color_space:'sRGB',input_cctf_decoding:false,input_interpretation:{mode:'manual'},photo_long_side_px:null,upscale_factor:1.0});
          const overrides = ${track === 'deterministic'};
          if (overrides) {params.film_render.grain.active=false;params.film_render.glare.active=false;params.print_render.glare.active=false;}
          const r = await accuracyEngine.renderRaw(imported.film,imported.paper,JSON.stringify(params),${Math.max(meta.width, meta.height)});
          if (!r.backend.startsWith('wgpu') || !r.scan?.length) throw Error('missing GPU scan');
          if (r.width !== ${meta.width} || r.height !== ${meta.height}) throw Error('fixture render unexpectedly resized');
          let nonfinite=0;for (const v of r.scan) if(!Number.isFinite(v)) nonfinite++;
          if(nonfinite) throw Error('nonfinite scan: '+nonfinite);
          let posted = await fetch('/result?name=${postName}&w='+r.width+'&h='+r.height,{method:'POST',body:r.rgba});
          if(!posted.ok) throw Error('RGB POST failed');
          posted = await fetch('/result?name=${postName}&format=f32&w='+r.width+'&h='+r.height,{method:'POST',body:r.scan});
          if(!posted.ok) throw Error('scan POST failed');
          return {status:'ok',effective_params:params,width:r.width,height:r.height,source_dimensions:[${meta.width},${meta.height}],
            backend:r.backend,nonfinite,fixture_transfer_sha256:digest,threads:1,decoder:'verified identical float fixture',
            overrides:overrides?{'film_render.grain.active':false,'film_render.glare.active':false,'print_render.glare.active':false}:{},stages:r.stages};`);
        record.fixture = meta;
        renameSync(resolve('web/results', postName + '.f32'), join(dest, 'scan.f32'));
      }
      renameSync(resolve('web/results', postName + '.ppm'), join(dest, 'output.ppm'));
      Object.assign(record, { output: 'output.ppm', output_space: 'sRGB', output_encoded: true });
    } catch (error) {
      Object.assign(record, { status: 'error', error: error.stack ?? String(error) });
      image = null;
    }
    Object.assign(record, { engine: 'web', case: c.name, track });
    write(join(dest, 'result.json'), record);
    records.push({ case: c.name, status: record.status, error: record.error });
    write(join(out, `browser-${track}-progress.json`), records);
    console.log('DONE', track, c.name, record.status, record.error ?? '');
  }
  return records;
};
