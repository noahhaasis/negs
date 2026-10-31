// Deterministic editor pixel checks for baseline/candidate browser builds.
// node web/cdp.mjs URL web/checks/stability-pixels.mjs 3000
// Run the same backend/thread/viewport on each arm; compare `frames`, not timings.
export default async ({ evaluate }) => evaluate(`
  const wait = ms => new Promise(r => setTimeout(r, ms));
  const until = async predicate => {
    for (let i = 0; i < 2400; i++) { if (predicate()) return; await wait(50); }
    throw new Error('pixel check timed out');
  };
  await until(() => globalThis.negs && !document.getElementById('engine-info').textContent.includes('Starting'));
  const engine = document.getElementById('engine-info').textContent, instances = negs.engineInstances();
  const quiet = async () => { await until(() => negs.idle() && negs.source().current); await wait(100); };
  const hash = async rgba => [...new Uint8Array(await crypto.subtle.digest('SHA-256', rgba))].map(x => x.toString(16).padStart(2, '0')).join('');
  await negs.open('sample/DSC_6028.dng'); await quiet();
  const frames = [];
  for (const [label, edits] of [
    ['default', {}],
    ['exposure and post', { 'camera.exposure_compensation_ev': .7, 'post.contrast': 20, 'post.saturation': -10 }],
    ['diffusion', { 'camera.diffusion_filter.active': true, 'enlarger.diffusion_filter.active': true }],
    ['daylight', { 'io.raw_white_balance.mode': 'daylight' }],
  ]) {
    const state = negs.state();
    negs.start(state.film, state.paper, edits); await quiet();
    const r = await negs.read(512), original = await negs.readOriginal(512);
    if (r.rgba.length !== r.width * r.height * 4) throw new Error('Invalid frame length');
    if (negs.engineInstances() !== instances) throw new Error('Unexpected engine replacement');
    const message = document.getElementById('message');
    if (!message.hidden && message.classList.contains('error')) throw new Error(message.textContent);
    frames.push({ label, width: r.width, height: r.height, rgba: await hash(r.rgba), original: await hash(original.rgba) });
  }
  return { engine, instances, coarse: matchMedia('(pointer: coarse)').matches, frames };
`);
