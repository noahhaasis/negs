// The placeholder (web/placeholder.js), in the page:
//   CDP_TMP=<scratch> node web/cdp.mjs http://localhost:8137/ web/checks/placeholder.js
// 0. The page starts on the empty screen and downloads no sample.
// 1. Orientation, as Linux photo.rs `raw_preview_uses_container_orientation_once`:
//    a RAW container's orientation is applied exactly once, over a
//    conflicting EXIF tag in its preview; without one, the preview's own.
// 2. The sample's lossy DNG: its placeholder is the camera preview, not one
//    of the JPEG tiles that hold its raw data, at any size.
// 3. Opening the Sony ARW (web/results/sony.arw) and the sample: when the
//    placeholder shows, when the engine has decoded, when the first frame
//    replaces it, and that it stays in place (the view's rectangle, within a
//    pixel or 0.5%: a camera's preview may be cropped a little differently;
//    the ARW's 1616 × 1080 is 0.25% narrower than its 6000 × 4000).
const { placeholder, tiffOrientation } = await import("/placeholder.js");
const failures = [];
const expect = (ok, what) => ok || failures.push(what);

expect(!document.getElementById("empty").hidden && !negs.photo(), "empty screen at start");
expect(!performance.getEntriesByType("resource").some((r) => r.name.includes("/sample/")), "no sample download at start");

// A TIFF header with one directory entry: the orientation.
function rawHeader(orientation, little) {
  const b = new DataView(new ArrayBuffer(26));
  b.setUint16(0, little ? 0x4949 : 0x4d4d);
  b.setUint16(2, 42, little);
  b.setUint32(4, 8, little);
  b.setUint16(8, 1, little);
  b.setUint16(10, 0x0112, little);
  b.setUint16(12, 3, little);
  b.setUint32(14, 1, little);
  b.setUint16(18, orientation, little);
  return new Uint8Array(b.buffer);
}
const concat = (...parts) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) { out.set(p, at); at += p.length; }
  return out;
};

for (const little of [true, false]) {
  for (let o = 1; o <= 8; o++) expect(tiffOrientation(rawHeader(o, little))?.value === o, `tiffOrientation ${o} ${little}`);
  for (const bad of [0, 9, 0xffff]) expect(tiffOrientation(rawHeader(bad, little)) === null, `tiffOrientation invalid ${bad}`);
  for (let size = 0; size < 22; size++) expect(tiffOrientation(rawHeader(8, little).subarray(0, size)) === null, `tiffOrientation truncated ${size}`);
}

// 40 × 20: red on the left half, green on the top half.
const source = new OffscreenCanvas(40, 20);
const sctx = source.getContext("2d");
const pixels = sctx.createImageData(40, 20);
for (let y = 0; y < 20; y++) for (let x = 0; x < 40; x++) pixels.data.set([x < 20 ? 240 : 10, y < 10 ? 240 : 10, 10, 255], (y * 40 + x) * 4);
sctx.putImageData(pixels, 0, 0);
const encoded = new Uint8Array(await (await source.convertToBlob({ type: "image/jpeg", quality: 1 })).arrayBuffer());
// The preview carries a conflicting EXIF orientation (6).
const exif = concat(new TextEncoder().encode("Exif\0\0"), rawHeader(6, true));
const jpeg = concat(new Uint8Array([0xff, 0xd8, 0xff, 0xe1, (exif.length + 2) >> 8, (exif.length + 2) & 0xff]), exif, encoded.subarray(2));

async function topLeft(bitmap) {
  const c = new OffscreenCanvas(bitmap.width, bitmap.height).getContext("2d");
  c.drawImage(bitmap, 0, 0);
  return [...c.getImageData(0, 0, 1, 1).data.slice(0, 2)];
}
// The source's corner that each orientation puts at the top left.
const corners = [[240, 240], [10, 240], [10, 10], [240, 10], [240, 240], [240, 10], [10, 10], [10, 240]];
const near = (a, b) => Math.abs(a - b) < 16;
for (let o = 1; o <= 8; o++) {
  const raw = concat(rawHeader(o, o % 2 === 0), jpeg);
  const dims = o >= 5 ? [20, 40] : [40, 20];
  const p = await placeholder(raw, 192);
  expect(p && p.bitmap.width === dims[0] && p.bitmap.height === dims[1], `orientation ${o}: shown ${p?.bitmap.width}×${p?.bitmap.height}`);
  expect(p && p.width === dims[0] && p.height === dims[1], `orientation ${o}: photograph ${p?.width}×${p?.height}`);
  const [r, g] = await topLeft(p.bitmap);
  expect(near(r, corners[o - 1][0]) && near(g, corners[o - 1][1]), `orientation ${o}: corner ${r},${g}`);
  const small = await placeholder(raw, 10);
  expect(small.bitmap.width === (o >= 5 ? 5 : 10) && small.bitmap.height === (o >= 5 ? 10 : 5), `orientation ${o}: small ${small.bitmap.width}×${small.bitmap.height}`);
  expect(small.width === dims[0] && small.height === dims[1], `orientation ${o}: small photograph`);
}
// A JPEG on its own, and a container with an invalid orientation, keep the preview's (6).
for (const [what, bytes] of [["jpeg", jpeg], ["invalid container", concat(rawHeader(0, true), jpeg)]]) {
  const p = await placeholder(bytes, 192);
  expect(p && p.bitmap.width === 20 && p.bitmap.height === 40 && p.width === 20 && p.height === 40, `${what}: ${p?.bitmap.width}×${p?.bitmap.height}`);
}
expect((await placeholder(new Uint8Array(1000), 192)) === null, "no image");

const dng = new Uint8Array(await (await fetch("sample/DSC_6377.dng")).arrayBuffer());
for (const side of [256, 600, 2000]) {
  const p = await placeholder(dng, side);
  expect(p && p.width === 1620 && p.height === 1080 && Math.max(...[p.bitmap.width, p.bitmap.height]) === Math.min(side, 1620), `DNG placeholder at ${side}: ${p?.bitmap.width}×${p?.bitmap.height} of ${p?.width}×${p?.height}`);
}

// Opening real photographs: the ARW as the Open button does, the sample by its button.
const opened = [];
for (const url of ["results/sony.arw", "sample"]) {
  const bytes = await (await fetch(url === "sample" ? "sample/DSC_6377.dng" : url)).arrayBuffer();
  const from = negs.timeline.length, renders = negs.renders.length;
  url === "sample" ? document.getElementById("open-sample").click() : negs.open(url);
  let rect = null, hidden = false;
  while (negs.renders.length === renders || !negs.idle()) {
    const p = negs.placeholder();
    if (p && !rect) rect = negs.view.rect();
    hidden ||= p && !p.visible;
    await new Promise((r) => setTimeout(r, 10));
  }
  const events = Object.fromEntries(negs.timeline.slice(from).map((e) => [e.event, Math.round(e.t - negs.timeline[from].t)]));
  const after = negs.view.rect();
  const same = rect && ["x", "y", "width", "height"].every((k) => Math.abs(rect[k] - after[k]) <= Math.max(1, 0.005 * after.width, 0.005 * after.height));
  expect(events.placeholder !== undefined && events.placeholder < events.decoded, `${url}: placeholder before decode`);
  expect(!hidden, `${url}: placeholder hidden while it showed`);
  expect(same, `${url}: placeholder in place ${JSON.stringify(rect)} vs ${JSON.stringify(after)}`);
  opened.push({ url, megabytes: +(bytes.byteLength / 1e6).toFixed(1), ...events, inPlace: same, photo: negs.photo() });
}
return { failures, opened };
