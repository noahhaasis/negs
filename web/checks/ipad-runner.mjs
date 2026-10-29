// Runs the same cases as the USB WebDriver harness, inside actual Safari.
// Only the initial page opening needs a person; results stream to the Mac.
import runSuite from "./ipad.mjs";

const frame = document.getElementById("app"), panel = document.getElementById("panel");
const status = document.getElementById("status"), details = document.getElementById("details");
const button = document.getElementById("start");
const wait = (ms) => new Promise(r => setTimeout(r, ms));
let run, epoch, sequence = 0, passed = 0, wakeLock, finished = false;
let pendingLog = Promise.resolve();
function log(event, fields = {}) {
  const entry = { at: new Date().toISOString(), sequence: ++sequence, event, ...fields };
  const targetRun = run;
  console.log(entry);
  pendingLog = pendingLog.then(async () => {
    const response = await fetch(`/device-log?run=${targetRun}`, {
      method: "POST", body: JSON.stringify(entry), signal: AbortSignal.timeout(20000),
    });
    if (!response.ok) throw new Error(`Saving device log: HTTP ${response.status}`);
  });
  return pendingLog;
}
const execute = async (script, ...args) => new frame.contentWindow.Function(script)(...args);
async function instrument() {
  epoch = await execute(`
    const t = globalThis.__negsTest = { epoch: String(performance.timeOrigin), logs: [], errors: [], exports: [] };
    const record = (type, args) => {
      const row = { at: performance.now(), type, text: args.map(a => a?.stack ?? String(a)).join(' ') };
      t.logs.push(row); if (t.logs.length > 100) t.logs.shift();
      if (type === 'error' || type === 'panic') t.errors.push(row);
    };
    for (const type of ['log','warn','error']) {
      const original = console[type].bind(console);
      console[type] = (...args) => { record(type,args); original(...args); };
    }
    addEventListener('error',e=>record('error',[e.error ?? e.message]),true);
    addEventListener('unhandledrejection',e=>record('error',[e.reason]));
    const create = URL.createObjectURL.bind(URL);
    URL.createObjectURL = blob => {
      if(blob.type === 'image/jpeg') t.exports.push({at:performance.now(),bytes:blob.size,type:blob.type});
      return create(blob);
    };
    t.panics = new BroadcastChannel('negs-engine-panic');
    t.panics.onmessage = e => record('panic',[JSON.stringify(e.data)]);
    return t.epoch;
  `);
}
async function probe() {
  return execute(`
    const n=globalThis.negs,t=globalThis.__negsTest,m=document.getElementById('message');
    return {url:location.href,epoch:t?.epoch,secure:isSecureContext,isolated:crossOriginIsolated,
      userAgent:navigator.userAgent,gpu:!!navigator.gpu,cores:navigator.hardwareConcurrency,
      engine:document.getElementById('engine-info')?.textContent,engineInstances:n?.engineInstances?.(),
      photo:n?.photo?.(),idle:n?.idle?.(),frames:n?.renders.length,recentFrames:n?.renders.slice(-5),
      message:m?.hidden===false?m.textContent:null,errorVisible:m?.hidden===false&&m.classList.contains('error'),
      hud:document.getElementById('hud')?.textContent,badge:n?.badge?.(),
      renderSide:n?.photo?.()?n.renderSide():null,zoom:document.getElementById('zoom-level')?.textContent,
      errors:t?.errors,recentLogs:t?.logs.slice(-10),exports:t?.exports};
  `);
}
function assertHealthy(state) {
  if (epoch && state.epoch !== epoch) throw new Error("Editor reloaded or lost its monitor");
  if (state.errors?.length || state.errorVisible) throw new Error(`Editor error: ${JSON.stringify(state.errors ?? state.message)}`);
}
async function waitFor(expression, { timeout = 180000, healthy = true } = {}) {
  const start = Date.now();
  let progress = start;
  for (;;) {
    const result = await execute(`return Boolean(${expression});`), state = await probe();
    if (healthy) assertHealthy(state);
    if (result) return state;
    if (Date.now()-start > timeout) throw new Error(`Timeout: ${expression}`);
    if (Date.now()-progress > 15000) { await log("waiting",{expression,state}); progress=Date.now(); }
    await wait(500);
  }
}
async function step(label, action, {settle=true,timeout=180000} = {}) {
  const before=await probe(); assertHealthy(before);
  status.textContent=label;
  details.textContent=`${passed} actions passed · ${run}`;
  await log("step-start",{label,state:before});
  const start=Date.now();
  await action(before);
  const after=settle ? await waitFor(`negs.renders.length > ${before.frames} && negs.idle()`,{timeout}) : await probe();
  assertHealthy(after); passed++;
  await log("step-pass",{label,elapsedMs:Date.now()-start,state:after});
  return after;
}
async function keepAwake() {
  try { wakeLock=await navigator.wakeLock?.request("screen"); }
  catch(e) { await log("wake-lock-unavailable",{message:e.message}); }
}
const api={execute,probe,instrument,assertHealthy,waitFor,step,wait,log,
  click:selector=>execute("document.querySelector(arguments[0]).click();",selector),
  call:()=>{throw new Error("Native gestures require the USB driver")},screenshot:async()=>null};

document.addEventListener("visibilitychange",()=>{
  if(run&&!finished) { log("visibility",{state:document.visibilityState}); if(!document.hidden) keepAwake(); }
});
const previous=JSON.parse(sessionStorage.getItem("negs-device-test")??"null");
if(previous&&!previous.finished) {
  run=previous.run;
  await log("unexpected-reload",{previous});
  status.textContent="The previous test page reloaded before finishing. Its last action is saved on the Mac. Tap Start for a new run.";
}
button.addEventListener("click",async()=>{
  button.disabled=true; panel.dataset.status="running"; finished=false; passed=0; sequence=0;
  pendingLog=Promise.resolve();
  run=`${Date.now().toString(36)}-${crypto.randomUUID().slice(0,8)}`;
  sessionStorage.setItem("negs-device-test",JSON.stringify({run,finished:false}));
  try {
    await log("run-start",{userAgent:navigator.userAgent,secure:isSecureContext,isolated:crossOriginIsolated});
    if(!isSecureContext||!crossOriginIsolated) throw new Error("Open the trusted HTTPS test URL; secure context and cross-origin isolation are required.");
    await keepAwake();
    // The app's own flags (?threads=0, ?backend=cpu, ?present=0) go to the app frame.
    const query=new URLSearchParams(location.search);
    const app=new URLSearchParams({"device-test":run});
    for(const flag of ["threads","backend","present"]) if(query.has(flag)) app.set(flag,query.get(flag));
    frame.src=`/?${app}`;
    await waitFor("!!globalThis.negs && !document.getElementById('engine-info').textContent.includes('Starting')",{healthy:false});
    await instrument();
    const image=query.get('image')??'results/0955097116.raf';
    if(!/^results\/[A-Za-z0-9_.-]+$/.test(image)) throw new Error('Invalid local fixture');
    await step(`open ${image}`,()=>execute("negs.open(arguments[0]).catch(e=>console.error(e));",image));
    const available=["smoke","extremes","zoom","heavy","limited-export","soak","lifecycle","export","after-export"];
    const phases=query.has('phases')?query.get('phases').split(','):available;
    if(phases.some(phase=>!available.includes(phase))) throw new Error('Unknown test phase');
    for(const phase of phases) {
      status.textContent=`Starting ${phase}`;
      await log("phase-start",{phase});
      const result=await runSuite({...api,env:{IPAD_PHASE:phase,IPAD_ROUNDS:"10"}});
      await log("phase-pass",{phase,result});
      if(phase==="limited-export") await step("reset after large diffusion",()=>execute("document.getElementById('reset').click();"));
    }
    await log("run-pass",{passed,state:await probe()});
    finished=true; panel.dataset.status="passed";
    status.textContent=`Passed ${passed} actions without a detected engine crash or reload.`;
    sessionStorage.setItem("negs-device-test",JSON.stringify({run,finished:true}));
  } catch(error) {
    panel.dataset.status="failed"; status.textContent=`Stopped: ${error.message}`;
    try { await log("run-failure",{message:error.message,stack:error.stack,state:await probe()}); }
    catch(e) { details.textContent=`Logging also failed: ${e.message}. Run ${run}`; }
  } finally {
    await wakeLock?.release(); button.disabled=false;
  }
});
