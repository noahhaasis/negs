import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as k from "./iso-kit.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "../..");
const theme = process.argv.includes("--dark") ? "dark" : "light";
const W = 1000, H = 390;
const bench = { x: -255, y: -82, w: 510, d: 164, r: 10 };
const stations = [-205, -117, -22, 75, 182];
const P = k.fitProjection(
  [...k.boxCorners(bench, -19, 98), [-225, 0, 125], [182, 0, 110]],
  W, H, { pad: 34, azimuth: 42, elevation: 33 },
);
const A = (p) => k.iso(p, P);
const S = (shape, tone = "mid", opts = {}) => k.solidSvg(shape, { tone, ...opts });
const L = (path, tone = "lo", opts = {}) => k.lineSvg(path, { tone, ...opts });
const D = (pts, size = .6, tone = "mid") => k.dotsSvg(pts.map(A), { size, tone });
const slab = (x,y,w,d,z,h,tone="mid",r=2) => S(k.slabOf({x,y,w,d,r},z,h,P,5,.7),tone);
const cyl = (x,y,r,z,h,tone="mid") => S(k.cylinder(x,y,r,z,h,P,28,.5),tone);
const screw = (x,y,z) => L(k.ring(x,y,1.8,z,P,18),"lo") + D([[x,y,z]],.45,"hi");
const plate = (x,y,w,d,z,h=1) => slab(x,y,w,d,z,h,"lo",1.5);
const cap = (x,y,z,r=3) => cyl(x,y,r,z,2,"hi") + L(k.ring(x,y,r-.7,z+2,P,20),"mid");

function base() {
  const out = [`<path class="iso-halo" d="${k.haloOf(bench,-19,18,P)}"/>`];
  for (const [x,y] of k.corners(bench,15)) {
    out.push(cyl(x,y,9,-19,6,"lo"),cyl(x,y,6,-13,4,"mid"));
  }
  out.push(S(k.slabOf(bench,-9,9,P,10,1.4),"mid"));
  out.push(L(k.planOutline(k.insetPlan(bench,5),0,P),"lo"));
  for (const [x,y] of k.corners(bench,11)) out.push(screw(x,y,0));
  for (const y of [-56,56]) {
    out.push(plate(-237,y,474,7,0,2));
    for (let x=-225;x<=225;x+=15) out.push(D([[x,y+3.5,2]],.38,"lo"));
  }
  return out.join("");
}

function rawDock(x) {
  let o=[];
  o.push(slab(x-34,-33,68,66,0,4,"lo",4));
  o.push(slab(x-25,-23,50,46,4,3,"mid",3));
  o.push(slab(x-22,-18,44,36,7,3,"hi",2));
  o.push(slab(x-27,-27,54,4,7,13,"mid",1.5));
  o.push(slab(x-27,23,54,4,7,13,"mid",1.5));
  o.push(slab(x-27,-23,4,46,7,13,"mid",1.5));
  o.push(slab(x+23,-23,4,46,7,13,"mid",1.5));
  for (const yy of [-21,21]) for (const xx of [-25,25]) o.push(cap(x+xx,yy,20,2.4));
  for(let i=0;i<6;i++) o.push(slab(x-18+i*7,-15,1.4,30,10,.3,"lo",.4));
  o.push(slab(x-15,-43,30,8,0,17,"mid",2));
  o.push(cyl(x,-40,7,17,6,"hi"));
  o.push(L(k.knurl(x,-40,7,17,23,28,P),"lo"));
  return o.join("");
}

function filmGate(x) {
  let o=[];
  o.push(slab(x-36,-36,72,72,0,5,"lo",4));
  for (const xx of [x-27,x+27]) for(const yy of [-27,27]) o.push(cyl(xx,yy,3,5,32,"mid"),cap(xx,yy,37,4));
  o.push(slab(x-30,-34,60,68,36,5,"mid",3));
  o.push(slab(x-24,-27,48,54,41,3,"hi",2));
  o.push(L(k.planOutline({x:x-19,y:-22,w:38,d:44,r:2},44,P),"mid"));
  for(let i=0;i<9;i++) {
    o.push(slab(x-23+i*5,-25,2,3,44,.6,"lo",.5));
    o.push(slab(x-23+i*5,22,2,3,44,.6,"lo",.5));
  }
  for (const yy of [-32,32]) o.push(slab(x-31,yy-2,62,4,5,31,"mid",1));
  o.push(cyl(x+26,0,7,44,5,"hi"));
  o.push(L(k.knurl(x+26,0,7,44,49,28,P),"lo"));
  o.push(D([[x-29,-29,41],[x+29,29,41]],.55,"hi"));
  return o.join("");
}

function enlarger(x) {
  let o=[];
  o.push(slab(x-38,-34,76,68,0,5,"lo",4));
  o.push(slab(x-30,-28,60,56,5,3,"mid",2));
  o.push(slab(x-24,-21,48,42,8,1,"hi",2));
  for(const xx of [x-30,x+30]) for(const yy of [-28,28]) o.push(screw(xx,yy,8));
  o.push(cyl(x,0,22,9,3,"lo"));
  o.push(cyl(x,0,16,12,3,"mid"));
  o.push(cyl(x,0,11,15,5,"hi"));
  o.push(L(k.ring(x,0,8,20,P,36),"hi"));
  o.push(slab(x-26,-5,52,10,20,4,"mid",1.5));
  for(const xx of [x-22,x+22]) o.push(cyl(xx,0,3,24,47,"mid"),cap(xx,0,71,4));
  o.push(slab(x-32,-20,64,40,70,7,"mid",3));
  o.push(slab(x-27,-17,54,34,77,3,"hi",2));
  o.push(cyl(x,0,15,80,10,"mid"));
  o.push(cyl(x,0,9,90,5,"hi"));
  for(const xx of [x-25,x+25]) o.push(screw(xx,0,80));
  for(const yy of [-14,14]) o.push(slab(x-22,yy-.7,44,1.4,80,.3,"lo",.4));
  return o.join("");
}

function paperBath(x) {
  let o=[];
  o.push(slab(x-37,-34,74,68,0,6,"lo",4));
  o.push(slab(x-32,-29,64,58,6,9,"mid",3));
  o.push(slab(x-27,-24,54,48,15,1,"hi",2));
  o.push(L(k.planOutline({x:x-25,y:-22,w:50,d:44,r:2},16,P),"lo"));
  for(const yy of [-30,27]) o.push(slab(x-33,yy,66,3,15,3,"mid",1));
  for(const xx of [x-32,x+29]) o.push(slab(xx,-27,3,54,15,3,"mid",1));
  for(let i=0;i<4;i++) o.push(slab(x-21,-13+i*8,42,1.2,16,.3,"lo",.4));
  for (const xx of [x-29,x+29]) for(const yy of [-26,26]) o.push(screw(xx,yy,18));
  o.push(slab(x-15,33,30,6,1,10,"mid",2));
  o.push(cyl(x,36,7,11,5,"hi"));
  return o.join("");
}

function scanner(x) {
  let o=[];
  o.push(slab(x-38,-36,76,72,0,8,"lo",4));
  o.push(slab(x-34,-32,68,64,8,15,"mid",3));
  o.push(slab(x-28,-26,56,52,23,3,"hi",2));
  o.push(L(k.planOutline({x:x-23,y:-21,w:46,d:42,r:1},26,P),"mid"));
  for(let i=0;i<9;i++) o.push(slab(x-20+i*5,-19,1.1,38,26,.3,"lo",.3));
  o.push(slab(x-32,-31,64,5,26,5,"mid",1));
  o.push(slab(x-32,26,64,5,26,5,"mid",1));
  for(const xx of [x-31,x+31]) o.push(cyl(xx,0,2.7,26,17,"mid"),cap(xx,0,43,3.5));
  o.push(slab(x-34,-35,68,70,43,5,"mid",3));
  o.push(slab(x-29,-29,58,58,48,2,"hi",2));
  o.push(L(k.planOutline({x:x-23,y:-23,w:46,d:46,r:1},50,P),"lo"));
  for(const xx of [x-30,x+30]) for(const yy of [-31,31]) o.push(screw(xx,yy,48));
  return o.join("");
}

const makers = [rawDock, filmGate, enlarger, paperBath, scanner];
const body = [base()];
makers.forEach((make,i) => body.push(`<g data-station="${i}" class="machine-station">${make(stations[i])}</g>`));
// A transfer puck glides along the front rail as the reader selects a step.
body.push(`<g data-part="puck" transform="${k.translateAlong(P,[stations[0],59,3])}">${cyl(0,0,8,0,5,"hi")}${L(k.ring(0,0,5,5,P,28),"hi")}${D([[0,0,5]],.8,"hi")}</g>`);
const svg = k.figureSvg({width:W,height:H,label:"An isometric processing bench with a RAW image gate, film emulsion gate, enlarger, paper bath and scanner. Select a station to follow the data through the code.",body});

const stages = [
  {name:"RAW to linear RGB",machine:0,tag:"Input",summary:"rawler develops the camera mosaic: rescale, demosaic, crop, white balance and camera-to-sRGB matrix. The final sRGB gamma step is removed, and EXIF orientation is applied. The simulator receives linear RGB pixels.",equation:"RAW mosaic → demosaic/WB/matrix → linear sRGB",files:["crates/spektrafilm-io/src/lib.rs","crates/spektrafilm-io/src/white_balance.rs"],tip:"To change RAW decoding or white balance, start in spektrafilm-io. The spectral simulator never sees Bayer samples."},
  {name:"Film exposure",machine:1,tag:"Filming · expose",summary:"Exposure compensation and optional auto exposure scale RGB. The selected upsampler maps RGB to three film-layer exposures using the stock's spectral sensitivity and reference illuminant. Optical effects act before log10.",equation:"RGB → spectral reconstruction → ∑λ light(λ) × film sensitivity(λ) → log exposure",files:["crates/spektrafilm-core/src/stages/filming.rs","crates/spektrafilm-core/src/spectral_service.rs"],tip:"A stock's log_sensitivity changes which wavelengths excite each layer. The default Hanatos path precomputes a stock-specific TC LUT during pipeline construction."},
  {name:"Film development",machine:1,tag:"Filming · develop",summary:"The stock's density curves turn log exposure into CMY dye density. Optional DIR couplers change neighboring layers; grain changes density. These values are a virtual negative or positive film, not display RGB.",equation:"log exposure → density curves → DIR + grain → film CMY density",files:["crates/spektrafilm-core/src/stages/filming.rs","crates/spektrafilm-model/src/density_curves.rs","crates/spektrafilm-model/src/grain.rs"],tip:"To alter stock contrast or color response, inspect density_curves, channel_density and base_density together. A curve alone does not define the final look."},
  {name:"Enlarger and paper",machine:2,tag:"Printing",summary:"A filtered enlarger illuminant passes through the film dye and base density. Paper sensitivity integrates that transmitted spectrum. Midgray calibration sets print exposure; paper density curves then develop a second CMY density image.",equation:"film density → 10^(−spectral density) × enlarger light → paper exposure → paper CMY",files:["crates/spektrafilm-core/src/enlarger.rs","crates/spektrafilm-core/src/stages/printing.rs"],tip:"Film and paper form a pair. CLI defaults to the film profile's target_print; the GUI selects its paired paper. scan_film bypasses this station."},
  {name:"Paper image",machine:3,tag:"Print state",summary:"The paper profile provides its own sensitivity, density curves, dye spectra and viewing illuminant. The output here is still spectral density. A print stock can change contrast and palette even when the film is unchanged.",equation:"paper log exposure → paper density curves → paper CMY density",files:["crates/spektrafilm-core/src/stages/printing.rs","crates/spektrafilm-core/src/profile.rs"],tip:"To add a print stock, provide a paper profile and test it with multiple films. The filtered enlarger spectrum and neutral filter database also affect the match."},
  {name:"Scan and PNG",machine:4,tag:"Scanning · output",summary:"The viewing light passes through film or paper density. Spectral integration yields XYZ, then chromatic adaptation and output RGB. Glare, gamut compression, scanner blur and sharpening follow. CCTF encoding and clipping precede 8-bit PNG quantization and EXIF writing.",equation:"density → spectrum → XYZ → RGB → CCTF/clip → 8-bit PNG",files:["crates/spektrafilm-core/src/stages/scanning.rs","crates/spektrafilm-io/src/save.rs"],tip:"GUI Export runs the f64 CPU CLI with enlarger/scanner LUTs; live Preview uses a resident f32 GPU chain. The same conceptual stages have distinct implementations."},
];

function profile(name) {
  const p=JSON.parse(readFileSync(join(root,"data/profiles",name),"utf8"));
  const d=p.data, info=p.info;
  const every=(arr,n)=>arr.filter((_,i)=>i%n===0||i===arr.length-1);
  return {id:info.stock,name:info.name,type:info.type,support:info.support,channel:info.channel_model,reference:info.reference_illuminant,target:info.target_print,wavelengths:every(d.wavelengths||[],4),sensitivity:every(d.log_sensitivity||[],4),curveX:every(d.log_exposure||[],5),curves:every(d.density_curves||[],5),development:d.development_time||[],source:`../../data/profiles/${name}`};
}
const profileNames=readdirSync(join(root,"data/profiles")).filter(name=>name.endsWith(".json")).sort();
const profiles=profileNames.map(profile);

const css=`<style>
body.iso{--ink:#1a2428;--muted:#59666b;--line:#c7d1d1;--paper:#f4f5f2;--green:#1c9a80;--orange:#d78f56;--blue:#638fbe;background:#e9eeea;color:var(--ink)}
.iso-plate{--anatomy-paper:#f8faf7;--anatomy-face:#e7ede8;--anatomy-top:#edf2ed;--anatomy-shade-0:#abbab2;--anatomy-shade-1:#bfcac2;--anatomy-shade-2:#d0dad2;--anatomy-shade-3:#e0e8df;--anatomy-mid:#5d746e;--anatomy-lo:#8da29a;--anatomy-faint:#b6c6bf}
body.iso[data-theme="dark"] .iso-plate{--anatomy-paper:#111b1b;--anatomy-face:#223334;--anatomy-top:#2b3c3a;--anatomy-shade-0:#162323;--anatomy-shade-1:#1c2b2a;--anatomy-shade-2:#263736;--anatomy-shade-3:#314341;--anatomy-mid:#8ba7a3;--anatomy-lo:#5e7b76;--anatomy-faint:#3c5753}
body.iso[data-theme="dark"]{--ink:#e4eceb;--muted:#9eafaf;--line:#354242;--paper:#111b1b;--green:#66d9ba;--orange:#efa96d;--blue:#95bce9;background:#0b1314;color:var(--ink)}
.iso-page{width:min(1180px,calc(100% - 32px));margin:32px auto 90px}
.eyebrow{text-transform:uppercase;letter-spacing:.17em;font:700 11px var(--anatomy-mono);color:var(--green)}
h1{font-size:clamp(30px,4vw,52px);line-height:1.05;letter-spacing:-.045em;margin:8px 0 10px;color:var(--ink)}
h2{font-size:25px;letter-spacing:-.03em;color:var(--ink);margin:0 0 12px}
h3{color:var(--ink);font-size:17px;margin:0 0 7px}
p{margin:0 0 12px}.intro{max-width:790px;font-size:16px;color:var(--muted)}
.iso-figure{margin:24px 0 0}.iso-plate{border-radius:16px;overflow:hidden}.iso-stage{cursor:pointer;outline:none}.iso-stage:focus-visible{outline:2px solid var(--green);outline-offset:3px}.iso-stage svg{display:block;width:100%}
.machine-station{transition:opacity .35s}.machine-station:not(.active){opacity:.82}.machine-station.active{opacity:1}
[data-part="puck"]{transition:transform .7s cubic-bezier(.2,.75,.2,1)}
.controls{display:grid;grid-template-columns:repeat(6,minmax(0,1fr));gap:7px;margin:18px 0}.step{border:1px solid var(--line);border-radius:10px;background:var(--paper);color:var(--muted);padding:11px 8px;text-align:left;cursor:pointer;font:600 12px/1.25 var(--anatomy-mono);min-height:68px}.step span{display:block;color:var(--green);margin-bottom:5px}.step[aria-current="step"]{border-color:var(--green);box-shadow:inset 0 0 0 1px var(--green);color:var(--ink)}
.detail,.section{background:var(--paper);border:1px solid var(--line);border-radius:14px;padding:22px;margin-top:16px}.detail{display:grid;grid-template-columns:1.5fr 1fr;gap:28px}.stage-tag{font:700 11px var(--anatomy-mono);text-transform:uppercase;letter-spacing:.1em;color:var(--green)}.formula{font:13px/1.6 var(--anatomy-mono);padding:12px;background:color-mix(in srgb,var(--green) 8%,transparent);border-left:3px solid var(--green);overflow-wrap:anywhere}.source-list{display:flex;flex-direction:column;gap:4px;margin-top:10px}.source-list a,.paths a{font:12px var(--anatomy-mono);color:var(--blue);overflow-wrap:anywhere}.note{color:var(--muted)}
.stock-controls{display:flex;gap:12px;flex-wrap:wrap;margin-bottom:18px;align-items:center}.stock-controls label{font:700 12px var(--anatomy-mono);color:var(--muted)}select{background:var(--paper);color:var(--ink);border:1px solid var(--line);border-radius:8px;padding:9px;max-width:100%}.stock-grid{display:grid;grid-template-columns:1fr 1fr;gap:18px}.chart-box{border:1px solid var(--line);border-radius:10px;padding:14px;min-width:0}.chart-box svg{width:100%;height:190px}.chart-box small{color:var(--muted)}.facts{display:grid;grid-template-columns:repeat(4,1fr);gap:8px;margin:0 0 18px}.fact{padding:10px;border:1px solid var(--line);border-radius:8px}.fact strong{display:block;color:var(--ink);font-size:13px}.fact span{font:10px var(--anatomy-mono);text-transform:uppercase;color:var(--muted)}
.routes{display:grid;grid-template-columns:repeat(2,1fr);gap:16px}.route{border:1px solid var(--line);padding:16px;border-radius:10px}.route code{font:12px var(--anatomy-mono);color:var(--green)}.route p{color:var(--muted)}
.map{display:grid;grid-template-columns:repeat(2,1fr);gap:14px}.map-item{border-top:1px solid var(--line);padding-top:12px}.map-item strong{display:block;color:var(--ink)}.map-item code{font:11px var(--anatomy-mono);color:var(--blue);overflow-wrap:anywhere}
.footer{margin-top:25px;color:var(--muted);font-size:12px}
@media(max-width:800px){.controls{grid-template-columns:repeat(3,1fr)}.detail,.stock-grid,.routes,.map{grid-template-columns:1fr}.facts{grid-template-columns:repeat(2,1fr)}.iso-page{margin-top:22px}.detail,.section{padding:16px}.iso-stage svg{width:115%;max-width:none;margin-left:-7.5%}}
@media(prefers-reduced-motion:reduce){[data-part="puck"],.machine-station{transition:none}}
</style>`;

const figure=k.plateHtml({fig:"Fig 01",title:"The virtual darkroom",hint:"Select a stage · ← → keys",readout:"RAW → linear RGB",caption:"The bench follows data, not material: no physical negative or paper is produced. The film, enlarger, paper and scanner are numerical models.",body:`<div class="iso-stage" tabindex="0" role="group" aria-label="Processing bench. Use left and right arrows to change stage.">${svg}</div>`});
const controls=stages.map((s,i)=>`<button class="step" type="button" data-step="${i}" aria-current="${i===0?"step":"false"}"><span>${String(i+1).padStart(2,"0")}</span>${s.name}</button>`).join("");
const profileOptions=profiles.filter(p=>p.support==="film").map(p=>`<option value="${p.id}">${p.name}</option>`).join("");
const paperOptions=profiles.filter(p=>p.support==="paper").map(p=>`<option value="${p.id}">${p.name}</option>`).join("");
const htmlBody=`${css}<div class="eyebrow">Spektrafilm / architecture atlas</div><h1>From RAW to a simulated print</h1><p class="intro">An interactive map of the Rust pipeline. Follow image data through a virtual film negative, enlarger, photographic paper and scanner, then inspect the profile data that makes stocks differ.</p>${figure}<div class="controls" role="group" aria-label="Pipeline stages">${controls}</div><section class="detail" aria-live="polite"><div><span class="stage-tag" id="stage-tag"></span><h2 id="stage-name"></h2><p id="stage-summary"></p><div class="formula" id="stage-equation"></div></div><div><h3>Where to work</h3><p class="note" id="stage-tip"></p><div class="source-list" id="stage-sources"></div></div></section>
<section class="section"><div class="eyebrow">Stock anatomy / real profile samples</div><h2>What changes when you select a stock?</h2><p class="note">The charts below are sampled from this repository's profile JSON. They show inputs to the model, not a predicted rendered image.</p><div class="stock-controls"><label for="film">Film</label><select id="film">${profileOptions}</select><label for="paper">Paper</label><select id="paper">${paperOptions}</select></div><div class="facts" id="facts"></div><div class="stock-grid"><div class="chart-box"><h3>Film sensitivity by wavelength</h3><svg id="sensitivity" viewBox="0 0 460 190" role="img" aria-label="Stock sensitivity across wavelengths"></svg><small>Relative log sensitivity; each color channel is shifted to its own maximum so spectral shape is visible.</small></div><div class="chart-box"><h3>Film development curves</h3><svg id="curves" viewBox="0 0 460 190" role="img" aria-label="Density against log exposure"></svg><small>Stored profile density against log exposure. B&amp;W shows the default middle time; the runtime reconstructs its curve from the fitted model.</small></div></div><p class="note" id="stock-note" style="margin-top:14px"></p></section>
<section class="section"><div class="eyebrow">The stock model / conceptual math</div><h2>Why two stocks produce different colors</h2><div class="map"><div class="map-item"><strong>1 · What reaches each layer</strong><p>The RGB input is reconstructed into a spectrum. Each film channel integrates that light against its wavelength sensitivity.</p><div class="formula">E<sub>c</sub> ≈ Σ<sub>λ</sub> L(λ) S<sub>c</sub>(λ)</div><code>log_sensitivity · reference_illuminant</code></div><div class="map-item"><strong>2 · How the layer develops</strong><p>The stock's curve maps log exposure to dye density. DIR couplers and grain change that density field.</p><div class="formula">D<sub>c</sub> = curve<sub>c</sub>(log₁₀ E<sub>c</sub>)</div><code>log_exposure · density_curves</code></div><div class="map-item"><strong>3 · What the negative transmits</strong><p>Dye spectra and film base shape the light passing through every wavelength. This is why equal CMY values from different stocks need not print alike.</p><div class="formula">T(λ) = 10<sup>−[base(λ) + Σ<sub>c</sub>D<sub>c</sub>dye<sub>c</sub>(λ)]</sup></div><code>channel_density · base_density</code></div><div class="map-item"><strong>4 · Paper and scanner see it again</strong><p>The paper integrates filtered enlarger light through the film; its own curves and dyes make a new density. The scan integrates viewing light through that density to XYZ/RGB.</p><div class="formula">film → paper exposure → paper density → XYZ → RGB</div><code>paper profile · enlarger filters · viewing illuminant</code></div></div><p class="note" style="margin-top:14px">These equations show the data flow. The implementation also applies calibration, adaptation, optional optical effects and precision-specific numeric paths. Spectral arrays use 81 samples from 380 to 780 nm in 5 nm steps.</p></section>
<section class="section"><div class="eyebrow">Runtime / where time and memory go</div><h2>Two routes through the same model</h2><div class="routes"><div class="route"><h3>Live preview</h3><code>GUI → f32 wgpu resident chain</code><p>One upload and one readback across filming, printing and scanning. The GUI can render a borrowed working image. Shader work lives in the wgpu backend and WGSL files.</p></div><div class="route"><h3>Final export</h3><code>GUI → child CLI → f64 CPU → PNG</code><p>The GUI launches spektrafilm-f64 with the CPU backend. Enlarger and scanner PCHIP LUTs are enabled for export. PNG is 8-bit RGB with EXIF copied from the source where available.</p></div></div><p class="note" style="margin-top:14px">The bench shows the default print chain. <code>io.scan_film</code> skips enlarger and paper development, scanning film density directly. The scanner profile and glare source follow that choice.</p><p class="note">Optimization handles: use <code>SPEKTRAFILM_STAGE_TIMINGS=1</code> to locate a slow stage; measure resident GPU passes separately; compare peak buffers before changing ownership or scratch reuse. <a href="../handoff-memory.md">Memory handoff</a> · <a href="../upstream-sync.md">Parity constraints</a>.</p></section>
<section class="section"><div class="eyebrow">Extension map</div><h2>Where a change belongs</h2><div class="map"><div class="map-item"><strong>New film or paper stock</strong><p>Start with a measured profile JSON: sensitivity, density curves, dye spectra, base density and metadata. Validate its shape and compare with known stocks.</p><code>data/profiles/ · spektrafilm-core/src/profile.rs</code></div><div class="map-item"><strong>New physical effect</strong><p>Define its stage and units first, then implement the CPU model and resident GPU equivalent. Check parity and preview behavior.</p><code>spektrafilm-model/ · stages/ · spektrafilm-gpu/</code></div><div class="map-item"><strong>RAW handling or output file</strong><p>RAW white balance and orientation live before the spectral chain. Quantization and metadata live after it.</p><code>spektrafilm-io/src/lib.rs · save.rs</code></div><div class="map-item"><strong>Faster export</strong><p>Preserve the f64 reference behavior. Target LUT construction, spectral reductions and image-sized temporary buffers based on measured stage times.</p><code>spektrafilm-core/src/pipeline.rs · spektrafilm-gpu/src/cpu_backend.rs</code></div></div></section><p class="footer">Source of truth: the current Rust code and bundled profile JSON. This page is generated by <code>docs/anatomy/build.mjs</code>; run <code>node docs/anatomy/build.mjs</code> after profile changes. The charts do not execute the renderer. Profile samples are derived from <a href="https://github.com/andreavolpato/spektrafilm">Andrea Volpato's spektrafilm</a> profiles, licensed <a href="https://creativecommons.org/licenses/by-sa/4.0/">CC BY-SA 4.0</a>; this explainer modifies them by sampling and charting. The SVG kit is from <a href="https://github.com/wheresryan22/anatomy">Anatomy</a> (<a href="LICENSE-anatomy.txt">MIT license</a>).</p>`;

const js=`const stages=${JSON.stringify(stages)};
const profiles=${JSON.stringify(profiles)};
const moves=${JSON.stringify(stations.map(x=>k.translateAlong(P,[x,59,3])))};
let current=0,tourTimer=0;
const q=s=>document.querySelector(s),qa=s=>[...document.querySelectorAll(s)];
function selectStage(i, manual=true){current=(i+stages.length)%stages.length;const s=stages[current];qa('.step').forEach((b,n)=>b.setAttribute('aria-current',n===current?'step':'false'));qa('.machine-station').forEach((g,n)=>g.classList.toggle('active',n===s.machine));q('[data-part="puck"]').setAttribute('transform',moves[s.machine]);q('[data-readout]').textContent=s.tag;q('#stage-tag').textContent=s.tag;q('#stage-name').textContent=s.name;q('#stage-summary').textContent=s.summary;q('#stage-equation').textContent=s.equation;q('#stage-tip').textContent=s.tip;q('#stage-sources').innerHTML=s.files.map(f=>'<a href="../../'+f+'">'+f+'</a>').join('');if(manual){clearInterval(tourTimer);tourTimer=0;}}
qa('.step').forEach(b=>b.addEventListener('click',()=>selectStage(+b.dataset.step)));
q('.iso-stage').addEventListener('keydown',e=>{if(e.key==='ArrowRight'||e.key==='ArrowLeft'){e.preventDefault();selectStage(current+(e.key==='ArrowRight'?1:-1));}});
q('.iso-stage').addEventListener('click',e=>{const box=e.currentTarget.getBoundingClientRect();selectStage(Math.min(stages.length-1,Math.floor((e.clientX-box.left)/box.width*stages.length)));});
selectStage(0,false);if(!matchMedia('(prefers-reduced-motion: reduce)').matches)tourTimer=setInterval(()=>selectStage(current+1,false),4500);
const color=['var(--anatomy-red)','var(--anatomy-green)','var(--anatomy-blue)'];
function chart(sel,xs,rows,kind){const svg=q(sel),W=460,H=190,left=38,right=12,top=10,bottom=28;let vals=rows.map(row=>row.map(v=>Number.isFinite(v)?v:null));let channels=Math.min(3,Math.max(...vals.map(r=>r.length),1));if(kind==='sensitivity'){let max=Array.from({length:channels},(_,c)=>Math.max(...vals.map(r=>r[c]).filter(Number.isFinite)));vals=vals.map(row=>row.map((v,c)=>v===null?null:v-max[c]));}let flat=vals.flat().filter(Number.isFinite),ymin=kind==='sensitivity'?-5:Math.min(0,...flat),ymax=kind==='sensitivity'?0:Math.max(...flat,1);const sx=x=>left+(x-xs[0])/(xs.at(-1)-xs[0]||1)*(W-left-right),sy=y=>H-bottom-(y-ymin)/(ymax-ymin||1)*(H-top-bottom);let out='<path d="M'+left+' '+top+'V'+(H-bottom)+'H'+(W-right)+'" fill="none" stroke="var(--line)"/>';for(let j=0;j<=4;j++){let y=top+j*(H-top-bottom)/4;out+='<path d="M'+left+' '+y+'H'+(W-right)+'" stroke="var(--line)" opacity=".35"/><text x="'+(left-5)+'" y="'+(y+3)+'" text-anchor="end" fill="var(--muted)" font-size="10">'+(ymax-j*(ymax-ymin)/4).toFixed(1)+'</text>';}for(let c=0;c<channels;c++){let d='',up=false;vals.forEach((row,i)=>{let v=row[c];if(!Number.isFinite(v)){up=false;return}d+=(up?'L':'M')+sx(xs[i]).toFixed(1)+' '+sy(Math.max(ymin,Math.min(ymax,v))).toFixed(1);up=true});out+='<path d="'+d+'" fill="none" stroke="'+(channels===1?'var(--ink)':color[c])+'" stroke-width="2.2"/>';}out+='<text x="'+left+'" y="'+(H-5)+'" fill="var(--muted)" font-size="10">'+xs[0]+'</text><text x="'+(W-right)+'" y="'+(H-5)+'" text-anchor="end" fill="var(--muted)" font-size="10">'+xs.at(-1)+'</text>';svg.innerHTML=out;}
function renderStock(){const film=profiles.find(p=>p.id===q('#film').value),paper=profiles.find(p=>p.id===q('#paper').value);const facts=[['Film type',film.type],['Channels',film.channel],['Reference light',film.reference],['Paired paper',film.target||'manual']];q('#facts').innerHTML=facts.map(([a,b])=>'<div class="fact"><span>'+a+'</span><strong>'+b+'</strong></div>').join('');chart('#sensitivity',film.wavelengths,film.sensitivity,'sensitivity');let curves=film.curves;if(film.development.length){let c=Math.floor((film.development.length-1)/2);curves=curves.map(row=>[row[c]]);}chart('#curves',film.curveX,curves,'density');q('#stock-note').innerHTML='<a href="'+film.source+'">'+film.name+' profile JSON</a> sets the film response. <a href="'+paper.source+'">'+paper.name+' profile JSON</a> sets the paper response. Changing paper does not change the film curves shown above; it changes the enlarger/print/scan portions of the renderer.';}
q('#film').value='kodak_gold_200';q('#paper').value='kodak_portra_endura';q('#film').addEventListener('change',e=>{const p=profiles.find(v=>v.id===e.target.value);if(p.target&&profiles.some(v=>v.id===p.target&&v.support==='paper'))q('#paper').value=p.target;renderStock();});q('#paper').addEventListener('change',renderStock);renderStock();`;

const html=k.pageHtml({title:"Spektrafilm anatomy — RAW to PNG",theme,body:htmlBody,script:js,width:1180});
const outfile=join(here,theme==="light"?"index.html":"index-dark.html");
writeFileSync(outfile,html);
console.log(`wrote ${outfile}`);
