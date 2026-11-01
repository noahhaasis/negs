// Something to show at once while the engine develops a photograph, as the
// desktop apps do (Linux linux/src/photo.rs `decode`): the browser's own
// decode of a JPEG or PNG, or for a camera RAW the camera's JPEG preview
// inside it. The engine's decode of a 24 MP RAW takes seconds; this takes
// tens of milliseconds, decoded off the main thread by createImageBitmap.

// Baseline, extended and progressive Huffman JPEGs: what browsers decode.
// (A CR2's or DNG's raw data is a lossless JPEG, SOF3, and is skipped.)
const DECODABLE = new Set([0xc0, 0xc1, 0xc2]);

// `bytes` (a Uint8Array) as an image whose long side is at most `longSide`:
// { bitmap, width, height }, where width and height are the photograph's
// size as far as the file tells (a RAW's largest preview), oriented. Null
// when there is nothing the browser decodes.
export async function placeholder(bytes, longSide) {
  const tiff = isTiff(bytes);
  if (tiff || !isImage(bytes)) {
    const shown = await embedded(bytes, longSide);
    if (shown || !tiff) return shown;
  }
  // A JPEG or PNG, or a TIFF without a preview (Safari decodes TIFFs). The
  // browser applies the file's orientation, as the engine does.
  const bitmap = await createImageBitmap(new Blob([bytes])).catch(() => null);
  return bitmap && scaled(bitmap, 1, longSide, bitmap.width, bitmap.height);
}

const isTiff = (b) => (b[0] === 0x49 && b[1] === 0x49 && b[2] === 0x2a && b[3] === 0) || (b[0] === 0x4d && b[1] === 0x4d && b[2] === 0 && b[3] === 0x2a);
const isImage = (b) => (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) || (b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47);

// The camera's preview: of the JPEGs the file holds, the smallest that
// covers `longSide`, or the largest. A TIFF-based RAW's (NEF, DNG, ARW, CR2)
// primary directory says how the camera was held, and replaces whatever the
// preview's own EXIF says, so the orientation is applied exactly once.
// Other containers keep the preview's.
async function embedded(bytes, longSide) {
  const raw = rawData(bytes);
  const found = jpegs(bytes).filter((j) => !raw.has(j.start)).sort((a, b) => a.width * a.height - b.width * b.height);
  if (!found.length) return null;
  const largest = found.at(-1);
  const container = tiffOrientation(bytes)?.value;
  const first = found.findIndex((j) => Math.max(j.width, j.height) >= longSide);
  const order = first < 0 ? found.toReversed() : [...found.slice(first), ...found.slice(0, first).reverse()];
  for (const jpeg of order.slice(0, 5)) {
    let data = bytes.subarray(jpeg.start, jpeg.end);
    let orientation = 1;
    if (container) {
      orientation = container;
      const own = jpeg.exif === undefined ? null : tiffOrientation(bytes, jpeg.exif);
      if (own && own.value !== 1) {
        data = data.slice();
        data.set(own.little ? [1, 0] : [0, 1], own.offset - jpeg.start);
      }
    }
    const bitmap = await createImageBitmap(new Blob([data])).catch(() => null);
    if (!bitmap) continue;
    // Without the container's orientation, the browser applied the preview's.
    const turned = container ? orientation >= 5 : bitmap.width < bitmap.height !== jpeg.width < jpeg.height;
    const [width, height] = turned ? [largest.height, largest.width] : [largest.width, largest.height];
    return scaled(bitmap, orientation, longSide, width, height);
  }
  return null;
}

// Every complete JPEG in `bytes`: where it starts and ends, its size, and
// where its EXIF's TIFF header is. Found by walking each candidate's
// markers, so EXIF thumbnails inside a preview don't cut it short.
export function jpegs(bytes) {
  const found = [];
  for (let i = bytes.indexOf(0xff); i >= 0; i = bytes.indexOf(0xff, i + 1)) {
    if (bytes[i + 1] !== 0xd8 || bytes[i + 2] !== 0xff) continue;
    const jpeg = parseJpeg(bytes, i);
    if (!jpeg) continue;
    found.push(jpeg);
    i = jpeg.end - 1;
  }
  return found;
}

function parseJpeg(bytes, start) {
  const n = bytes.length;
  let p = start + 2, frame = null, exif;
  while (p + 4 <= n) {
    if (bytes[p] !== 0xff) return null;
    const marker = bytes[p + 1];
    if (marker === 0xff) { p++; continue; }
    const length = (bytes[p + 2] << 8) | bytes[p + 3];
    if (length < 2 || p + 2 + length > n) return null;
    if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
      if (frame || length < 8) return null;
      frame = { marker, height: (bytes[p + 5] << 8) | bytes[p + 6], width: (bytes[p + 7] << 8) | bytes[p + 8] };
    }
    if (marker === 0xe1 && exif === undefined && length >= 16 && String.fromCharCode(...bytes.subarray(p + 4, p + 10)) === "Exif\0\0") exif = p + 10;
    p += 2 + length;
    if (marker !== 0xda) continue;
    // A scan's entropy-coded data runs to the next marker that isn't a
    // stuffed byte or a restart: the end, or (progressive) the next segment.
    if (!frame) return null;
    for (;;) {
      p = bytes.indexOf(0xff, p);
      if (p < 0 || p + 1 >= n) return null;
      const next = bytes[p + 1];
      if (next === 0xff) p++;
      else if (next === 0 || (next >= 0xd0 && next <= 0xd7)) p += 2;
      else break;
    }
    if (bytes[p + 1] === 0xd9) {
      if (!DECODABLE.has(frame.marker) || !frame.width || !frame.height) return null;
      return { start, end: p + 2, width: frame.width, height: frame.height, exif };
    }
  }
  return null;
}

// Where a TIFF-based RAW's raw data starts: its strips and tiles, which in a
// lossy DNG are JPEGs too, of linear camera RGB, not something to show.
// The directories are IFD0's chain and their SubIFDs (DNG, NEF).
export function rawData(bytes) {
  const starts = new Set();
  const head = bytes.length >= 8 && new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (!head || !isTiff(bytes)) return starts;
  const little = bytes[0] === 0x49;
  const short = (o) => (o + 2 <= bytes.length ? head.getUint16(o, little) : undefined);
  const long = (o) => (o + 4 <= bytes.length ? head.getUint32(o, little) : undefined);
  // A tag's SHORT or LONG values.
  const values = (entry) => {
    const [kind, count] = [short(entry + 2), long(entry + 4)];
    const size = kind === 3 ? 2 : kind === 4 || kind === 13 ? 4 : 0;
    if (!size || count > 1e5) return [];
    const at = size * count <= 4 ? entry + 8 : long(entry + 8);
    return Array.from({ length: count }, (_, i) => (size === 2 ? short(at + 2 * i) : long(at + 4 * i))).filter((v) => v !== undefined);
  };
  const queue = [long(4)], seen = new Set();
  while (queue.length && seen.size < 64) {
    const at = queue.shift();
    if (at === undefined || seen.has(at)) continue;
    seen.add(at);
    const count = short(at);
    if (count === undefined || at + 2 + count * 12 > bytes.length) continue;
    const tags = new Map();
    for (let i = 0; i < count; i++) tags.set(short(at + 2 + i * 12), at + 2 + i * 12);
    const photometric = tags.has(262) ? values(tags.get(262))[0] : undefined;
    if (photometric === 32803 || photometric === 34892) {
      for (const tag of [273, 324]) if (tags.has(tag)) values(tags.get(tag)).forEach((v) => starts.add(v));
    }
    if (tags.has(330)) queue.push(...values(tags.get(330)));
    queue.push(long(at + 2 + count * 12) || undefined);
  }
  return starts;
}

// The orientation (1 to 8) in the first directory of the TIFF structure at
// `base` (a TIFF-based RAW's start, or a JPEG's EXIF), with where its value
// is and the byte order; null when absent or invalid (Linux photo.rs
// `tiff_orientation`).
export function tiffOrientation(bytes, base = 0) {
  const at = (offset, size) => (offset >= base && offset + size <= bytes.length ? new DataView(bytes.buffer, bytes.byteOffset + offset, size) : null);
  const head = at(base, 4);
  if (!head) return null;
  const little = head.getUint32(0) === 0x49492a00;
  if (!little && head.getUint32(0) !== 0x4d4d002a) return null;
  const short = (offset) => at(offset, 2)?.getUint16(0, little);
  const long = (offset) => at(offset, 4)?.getUint32(0, little);
  const directory = long(base + 4);
  if (directory === undefined) return null;
  const count = short(base + directory);
  if (count === undefined || !at(base + directory + 2, count * 12)) return null;
  for (let index = 0; index < count; index++) {
    const entry = base + directory + 2 + index * 12;
    if (short(entry) === 0x0112 && short(entry + 2) === 3 && long(entry + 4) === 1) {
      const value = short(entry + 8);
      return value >= 1 && value <= 8 ? { value, offset: entry + 8, little } : null;
    }
  }
  return null;
}

// `bitmap` turned to EXIF `orientation` and reduced to `longSide`, as a new
// bitmap (the old one is closed).
function scaled(bitmap, orientation, longSide, width, height) {
  const turned = orientation >= 5;
  const [w, h] = turned ? [bitmap.height, bitmap.width] : [bitmap.width, bitmap.height];
  const factor = Math.min(1, longSide / Math.max(w, h));
  const canvas = new OffscreenCanvas(Math.max(1, Math.round(w * factor)), Math.max(1, Math.round(h * factor)));
  const ctx = canvas.getContext("2d");
  ctx.imageSmoothingQuality = "high";
  // Each orientation's transform from the stored image to the shown one.
  const [cw, ch] = [canvas.width, canvas.height];
  const transforms = {
    1: [1, 0, 0, 1, 0, 0], 2: [-1, 0, 0, 1, cw, 0], 3: [-1, 0, 0, -1, cw, ch], 4: [1, 0, 0, -1, 0, ch],
    5: [0, 1, 1, 0, 0, 0], 6: [0, 1, -1, 0, cw, 0], 7: [0, -1, -1, 0, cw, ch], 8: [0, -1, 1, 0, 0, ch],
  };
  ctx.setTransform(...transforms[orientation]);
  ctx.drawImage(bitmap, 0, 0, turned ? ch : cw, turned ? cw : ch);
  bitmap.close();
  return { bitmap: canvas.transferToImageBitmap(), width, height };
}
