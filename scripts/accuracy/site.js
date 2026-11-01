const $ = id => document.getElementById(id);
const report = await (await fetch('report.json')).json();
const engines = Object.keys(report.engines);
const cache = new Map();
let current, canvases = [], center = [.5, .5], zoom = null, sequence = 0, dragging, blink = false;
const addOptions = (id, options) => { $(id).replaceChildren(...options.map(([value, text]) => Object.assign(document.createElement('option'), {value, textContent:text}))); };
addOptions('image', [...new Map(report.cases.map(c => [c.image_id, [c.image_id, c.image_id]])).values()]);
addOptions('preset', [...new Map(report.cases.map(c => [c.label, [c.label, c.label]])).values()]);
for (const id of ['pairA', 'pairB']) addOptions(id, engines.map(e => [e, report.engines[e].label]));
$('pairA').value = 'web'; $('pairB').value = 'python';
$('run').textContent = `${report.run_id} · ${report.generation_status ?? 'complete'} · ${report.cases.length}/${report.expected_case_tracks ?? report.cases.length} case-tracks · ${JSON.stringify(report.summary ?? 'metrics still generating; refresh for progress')} · strict equality required`;
$('method').textContent = JSON.stringify(report.method, null, 2);
const availableTracks = new Set(report.cases.map(c => c.track));
for (const option of $('track').options) option.disabled = !availableTracks.has(option.value);
$('track').value = [...availableTracks][0];
function img(url) {
  if (!cache.has(url)) {
    const promise = new Promise((resolve, reject) => { const image = new Image(); image.onload = () => resolve(image); image.onerror = () => reject(Error('Missing image: '+url)); image.src = url; });
    cache.set(url, promise);
    if (cache.size > 180) cache.delete(cache.keys().next().value);
  }
  return cache.get(url);
}
function rect(canvas, render) {
  const w = canvas.clientWidth, h = canvas.clientHeight;
  const scale = zoom ?? Math.min(w/render.width, h/render.height);
  return {w,h,scale,left:w/2-center[0]*render.width*scale,top:h/2-center[1]*render.height*scale};
}
function setupCanvas(canvas) {
  canvas.addEventListener('pointerdown', event => {
    const render = current.renders[canvas.dataset.engine] ?? current.renders[$('pairA').value];
    if (render?.status !== 'ok') return;
    const r = rect(canvas,render); zoom = r.scale;
    dragging = {x:event.clientX,y:event.clientY,center:[...center],width:render.width,height:render.height};
    canvas.setPointerCapture(event.pointerId);
  });
  canvas.addEventListener('pointermove', event => {
    if (!dragging) return;
    center = [dragging.center[0]-(event.clientX-dragging.x)/(zoom*dragging.width), dragging.center[1]-(event.clientY-dragging.y)/(zoom*dragging.height)];
    redraw();
  });
  canvas.addEventListener('pointerup', () => dragging = null);
  canvas.addEventListener('pointercancel', () => dragging = null);
  canvas.addEventListener('wheel', event => {event.preventDefault(); changeZoom(event.deltaY < 0 ? 1.25 : .8);}, {passive:false});
}
function changeZoom(factor) {
  const first = current?.renders[$('pairA').value];
  const canvas = $('difference');
  if (!first || first.status !== 'ok') return;
  zoom = Math.max(.02, Math.min(16, (zoom ?? rect(canvas, first).scale)*factor)); redraw();
}
function sizeCanvas(canvas) {
  const dpr = devicePixelRatio;
  const width = Math.round(canvas.clientWidth*dpr), height = Math.round(canvas.clientHeight*dpr);
  if (canvas.width !== width || canvas.height !== height) {canvas.width=width;canvas.height=height;}
  const ctx=canvas.getContext('2d');ctx.setTransform(dpr,0,0,dpr,0,0);ctx.clearRect(0,0,canvas.clientWidth,canvas.clientHeight);ctx.imageSmoothingEnabled=false;
  return ctx;
}
async function drawEngine(canvas, engine, serial) {
  const render = current?.renders[engine];
  const ctx=sizeCanvas(canvas);
  if (render?.status !== 'ok') {ctx.fillStyle='#ffc0b8';ctx.fillText(render?.error ?? 'Missing render',12,30);return;}
  const r=rect(canvas,render);
  if (zoom === null || r.scale < .3) {
    const image=await img(render.preview);if(serial!==sequence)return;
    ctx.drawImage(image,r.left,r.top,render.width*r.scale,render.height*r.scale);return;
  }
  const startX=Math.max(0,Math.floor(-r.left/(512*r.scale))),startY=Math.max(0,Math.floor(-r.top/(512*r.scale)));
  const endX=Math.min(Math.ceil(render.width/512),Math.ceil((r.w-r.left)/(512*r.scale)));
  const endY=Math.min(Math.ceil(render.height/512),Math.ceil((r.h-r.top)/(512*r.scale)));
  await Promise.all(Array.from({length:Math.max(0,endY-startY)},(_,iy)=>Array.from({length:Math.max(0,endX-startX)},async(_,ix)=>{
    const x=startX+ix,y=startY+iy,image=await img(`${render.tile_base}/tile-${x}-${y}.png`);if(serial!==sequence)return;
    ctx.drawImage(image,r.left+x*512*r.scale,r.top+y*512*r.scale,image.width*r.scale,image.height*r.scale);
  })).flat());
}
function pair() {return Object.values(current?.pairs ?? {}).find(p => (p.a===$('pairA').value&&p.b===$('pairB').value)||(p.b===$('pairA').value&&p.a===$('pairB').value));}
function pixels(image) {const c=document.createElement('canvas');c.width=image.width;c.height=image.height;const ctx=c.getContext('2d',{willReadFrequently:true});ctx.drawImage(image,0,0);return ctx.getImageData(0,0,c.width,c.height);}
function differenceImage(a,b,gain=1,rgb=false) {
  const pa=pixels(a),pb=pixels(b);if(pa.width!==pb.width||pa.height!==pb.height)throw Error('Tile dimensions differ');
  const canvas=document.createElement('canvas');canvas.width=pa.width;canvas.height=pa.height;
  const ctx=canvas.getContext('2d');
  for(let i=0;i<pa.data.length;i+=4){const d=[0,1,2].map(c=>Math.abs(pa.data[i+c]-pb.data[i+c]));const maximum=Math.max(...d);for(let c=0;c<3;c++)pa.data[i+c]=Math.min(255,(rgb?d[c]:maximum)*gain);pa.data[i+3]=255;}
  ctx.putImageData(pa,0,0);return canvas;
}
async function drawDifference(serial) {
  const canvas=$('difference'),ctx=sizeCanvas(canvas),metric=pair();
  const a=current?.renders[$('pairA').value],b=current?.renders[$('pairB').value];
  if (!a || !b || a.status!=='ok'||b.status!=='ok'||a.width!==b.width||a.height!==b.height) {
    ctx.fillStyle='#ffc0b8';ctx.fillText('Missing outputs or incompatible dimensions. No implicit alignment.',10,30);return;
  }
  if ($('diffmode').value==='blink') {await drawEngine(canvas,blink?$('pairA').value:$('pairB').value,serial);return;}
  if ($('pairA').value===$('pairB').value) return;
  const r=rect(canvas,a),gain=+$('gain').value;
  if(zoom===null||r.scale<.3){
    if(!metric?.difference_preview)return;
    const image=await img(metric.difference_preview);if(serial!==sequence)return;
    const data=pixels(image);for(let i=0;i<data.data.length;i+=4){const v=Math.min(255,data.data[i]*gain);data.data[i]=data.data[i+1]=data.data[i+2]=v;}
    const tmp=document.createElement('canvas');tmp.width=data.width;tmp.height=data.height;tmp.getContext('2d').putImageData(data,0,0);
    ctx.drawImage(tmp,r.left,r.top,a.width*r.scale,a.height*r.scale);return;
  }
  const x0=Math.max(0,Math.floor(-r.left/(512*r.scale))),y0=Math.max(0,Math.floor(-r.top/(512*r.scale)));
  const x1=Math.min(Math.ceil(a.width/512),Math.ceil((r.w-r.left)/(512*r.scale))),y1=Math.min(Math.ceil(a.height/512),Math.ceil((r.h-r.top)/(512*r.scale)));
  const tasks=[];
  for(let y=y0;y<y1;y++)for(let x=x0;x<x1;x++)tasks.push((async()=>{
    const [one,two]=await Promise.all([img(`${a.tile_base}/tile-${x}-${y}.png`),img(`${b.tile_base}/tile-${x}-${y}.png`)]);
    if(serial!==sequence)return;const diff=differenceImage(one,two,gain);ctx.drawImage(diff,r.left+x*512*r.scale,r.top+y*512*r.scale,diff.width*r.scale,diff.height*r.scale);
  })());await Promise.all(tasks);
}
function numbers() {
  const metric=pair(), target=$('numbers');target.replaceChildren();
  if(!metric){target.textContent='Select two different engines.';return;}
  const values={Status:metric.status,'Strict equality':metric.exact_equal?'PASS':'FAIL',
    'Exact RGB pixels':metric.exact_rgb_pixel_fraction,'Within one RGB code':metric.within_one_code_rgb_pixel_fraction,
    'Max |channel error| / 255':metric.max_abs_channel_codes,'Mean |channel error| / 255':metric.mean_abs_channel_codes,
    ...Object.fromEntries(Object.entries(metric.delta_e_2000??{}).map(([k,v])=>['ΔE00 '+k,v]))};
  const table=document.createElement('table');for(const[k,v]of Object.entries(values)){const tr=document.createElement('tr');for(const value of [k,v??'—']){const td=document.createElement('td');td.textContent=typeof value==='number'?Number(value.toPrecision(8)):value;tr.append(td);}table.append(tr);}target.append(table);
  if(metric.float_scan){const pre=document.createElement('pre');pre.textContent='Unclipped scan (separate from RGB8 export):\n'+JSON.stringify(metric.float_scan,null,2);target.append(pre);}
}
async function redraw() {
  const serial=++sequence;
  $('position').textContent=`${zoom===null?'fit':(zoom*100).toFixed(0)+'% / '+zoom.toFixed(2)+' CSS px per source pixel'} · center ${(center[0]*100).toFixed(1)}%, ${(center[1]*100).toFixed(1)}%`;
  $('legend').textContent=`Black=0. White≥${(255/+$('gain').value).toFixed(2)} codes; grayscale=min(255,gain×max |ΔR,ΔG,ΔB|). ${zoom===null?'Fit: max-pooled blocks of '+(pair()?.difference_pool_block_px??'?')+' source pixels; NOT a numeric source.':'At ≥30% zoom: exact lossless source tiles.'} 1:1 = one source pixel per CSS pixel (Retina uses devicePixelRatio).`;
  try{await Promise.all([...canvases.map(c=>drawEngine(c,c.dataset.engine,serial)),drawDifference(serial)]);}catch(error){$('legend').textContent=error.message;}
}
function selectCase() {
  sequence++;cache.clear();
  current=report.cases.find(c=>c.track===$('track').value&&c.image_id===$('image').value&&c.label===$('preset').value);
  $('panels').replaceChildren();$('fixtureLinks').replaceChildren();canvases=[];
  if(!current){$('scope').textContent='Track / case not present in this review.';return;}
  $('scope').textContent=current.track==='application'?'PRIMARY: full-resolution application exports. Independent RAW developers; no alignment/crop/resizing of numeric sources. Original grain and all preset effects retained.':'DIAGNOSTIC ONLY: explicitly resized, shared float32 linear-sRGB fixture. This is NOT the full-resolution end-to-end RAW comparison. '+(current.track==='deterministic'?'Only grain and film/print glare disabled.':'Original photographic effects retained, including grain.')+' Engine-specific LUT/fast-stats and arithmetic defaults are retained, not forced to match.';
  if(current.track!=='application')for(const extension of ['json','tiff','f32']){const link=document.createElement('a');link.href=current.fixture_downloads?.[extension]??`fixtures/${current.fixture_id}.${extension}`;link.textContent=`Shared input fixture · ${extension}`;$('fixtureLinks').append(link);}
  const selected=engines.filter(e=>e==='web'?$('web').checked:e==='python'?$('python').checked:e.startsWith('installed')?$('installed').checked&&($('native').value==='both'||e.endsWith($('native').value)):$('source').checked&&($('native').value==='both'||e.endsWith($('native').value)));
  for(const engine of selected){
    const result=current.renders[engine],article=document.createElement('article'),title=document.createElement('h2');title.textContent=report.engines[engine].label;article.append(title);
    const canvas=document.createElement('canvas');canvas.dataset.engine=engine;setupCanvas(canvas);article.append(canvas);canvases.push(canvas);
    const meta=document.createElement('p');meta.className='meta';meta.textContent=result?.status==='ok'?`${result.width} × ${result.height} · ${result.backend} · ${result.decoder??''}`:`${result?.status??'missing'}: ${result?.error??'no output'}`;article.append(meta);
    if(result?.download){for(const[label,url]of[['Lossless original',result.download],['Metadata / params',result.metadata_url]]){const link=document.createElement('a');link.href=url;link.textContent=label;link.style.marginRight='12px';article.append(link);}}
    if(result?.scan_sha256){const link=document.createElement('a');link.href=result.scan_download??result.download.replace(/[^/]+$/,engine==='web'?'scan.f32':'scan.f64');link.textContent='Unclipped float scan';article.append(link);}
    $('panels').append(article);
  }
  $('metadata').textContent=JSON.stringify(current,null,2);numbers();redraw();
}
for(const id of ['track','image','preset'])$(id).onchange=()=>{center=[.5,.5];zoom=null;selectCase();};
for(const id of ['native','source','web','python','installed'])$(id).onchange=selectCase;
for(const id of ['pairA','pairB','gain','diffmode'])$(id).onchange=()=>{numbers();redraw();};
$('fit').onclick=()=>{center=[.5,.5];zoom=null;redraw();};$('actual').onclick=()=>{zoom=1;redraw();};
$('zoomin').onclick=()=>changeZoom(1.5);$('zoomout').onclick=()=>changeZoom(1/1.5);
for(const[id,direction]of[['previous',-1],['next',1]])$(id).onclick=()=>{$('preset').selectedIndex=($('preset').selectedIndex+direction+$('preset').options.length)%$('preset').options.length;selectCase();};
$('downloadDiff').onclick=async()=>{
  try{const a=current.renders[$('pairA').value],b=current.renders[$('pairB').value];if(a.width!==b.width||a.height!==b.height)throw Error('Incompatible dimensions');
    const x=Math.max(0,Math.min(Math.ceil(a.width/512)-1,Math.floor(center[0]*a.width/512))),y=Math.max(0,Math.min(Math.ceil(a.height/512)-1,Math.floor(center[1]*a.height/512)));
    const[one,two]=await Promise.all([img(`${a.tile_base}/tile-${x}-${y}.png`),img(`${b.tile_base}/tile-${x}-${y}.png`)]);
    const diff=differenceImage(one,two,1,true),link=document.createElement('a');link.download=`${current.name}-absolute-RGB-difference-x${x*512}-y${y*512}.png`;link.href=diff.toDataURL();link.click();
  }catch(error){alert(error.message);}
};
setupCanvas($('difference'));new ResizeObserver(()=>redraw()).observe($('panels'));
setInterval(()=>{if($('diffmode').value==='blink'){blink=!blink;redraw();}},600);
window.addEventListener('resize',()=>redraw());selectCase();
