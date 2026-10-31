// Real browser exports: encoded JPEG survives canvas release; an encoder
// failure also releases it and leaves the same engine usable for edit/retry.
// VIEW=390x844@3m node web/cdp.mjs URL web/checks/export-recovery.mjs 3000
// EXPORT_FIXTURE=synthetic is a smaller raster control for slow CPU runs.
export default async ({ evaluate }) => evaluate(`
  const wait = ms => new Promise(r => setTimeout(r, ms));
  const until = async predicate => {
    for (let i = 0; i < 3600; i++) { if (predicate()) return; await wait(50); }
    throw new Error('export recovery check timed out');
  };
  await until(() => globalThis.negs && !document.getElementById('engine-info').textContent.includes('Starting'));
  if (${JSON.stringify(process.env.EXPORT_FIXTURE)} === 'synthetic') {
    const canvas = new OffscreenCanvas(512, 384), ctx = canvas.getContext('2d');
    const gradient = ctx.createLinearGradient(0, 0, 512, 384);
    gradient.addColorStop(0, '#405080'); gradient.addColorStop(1, '#e0b080');
    ctx.fillStyle = gradient; ctx.fillRect(0, 0, 512, 384);
    const url = URL.createObjectURL(await canvas.convertToBlob({ type: 'image/png' }));
    try { await negs.open(url); } finally { URL.revokeObjectURL(url); canvas.width = canvas.height = 1; }
  } else await negs.open('sample/DSC_6377.dng');
  await until(() => negs.idle());
  const instances = negs.engineInstances(), photo = JSON.stringify(negs.photo());
  const canvases = [], blobs = [];
  let failNext = false;
  const convert = OffscreenCanvas.prototype.convertToBlob;
  OffscreenCanvas.prototype.convertToBlob = function(...args) {
    canvases.push(this);
    if (failNext) { failNext = false; return Promise.reject(new Error('Injected JPEG encoder failure')); }
    return convert.apply(this, args);
  };
  const createURL = URL.createObjectURL.bind(URL);
  URL.createObjectURL = blob => { if (blob.type === 'image/jpeg') blobs.push(blob); return createURL(blob); };
  const check = () => {
    if (canvases.some(c => c.width !== 1 || c.height !== 1)) throw new Error('Export canvas backing store retained');
    if (negs.engineInstances() !== instances || JSON.stringify(negs.photo()) !== photo) throw new Error('Export replaced engine/photo');
  };
  const exportJpeg = async () => {
    document.getElementById('export').click();
    await until(() => !document.getElementById('export').disabled && negs.idle());
    check();
  };
  await exportJpeg();
  if (blobs.length !== 1 || !blobs[0].size) throw new Error('No first JPEG');
  const bitmap = await createImageBitmap(blobs[0]);
  const jpeg = { width: bitmap.width, height: bitmap.height, bytes: blobs[0].size };
  bitmap.close();
  if (jpeg.width * jpeg.height < ${process.env.EXPORT_FIXTURE === "synthetic" ? 1e5 : 1e6}) throw new Error('JPEG was shrunk with the canvas');
  failNext = true;
  await exportJpeg();
  const message = document.getElementById('message');
  if (blobs.length !== 1 || message.hidden || !message.textContent.includes('Injected JPEG encoder failure')) throw new Error('Expected encoding failure was not reported');
  const n = negs.renders.length;
  negs.edit('camera.exposure_compensation_ev', .3);
  await until(() => negs.renders.length > n && negs.idle());
  if (!message.hidden) throw new Error('Editor did not recover after encoding failure');
  await exportJpeg();
  if (blobs.length !== 2 || !blobs[1].size) throw new Error('JPEG retry failed');
  return { engine: document.getElementById('engine-info').textContent, instances, jpeg,
    exportCanvases: canvases.map(c => [c.width, c.height]), retryBytes: blobs[1].size, recovered: true };
`);
