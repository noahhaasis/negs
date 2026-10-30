// Presets in the browser (docs/web-presets-design.md): the naming policy and
// the shared-link codec. The file format and its mapping are the shared Rust
// importer/exporter's (crates/spektrafilm-core presets.rs, through the
// engine's importPreset and exportPreset); nothing here reads a preset's
// settings.

// macOS PresetLibrary.maxFileSize: larger files aren't GUI states.
export const MAX_FILE_BYTES = 1_048_576;

// macOS PresetName: names Windows, macOS and Linux all accept as file names.
export const MAX_NAME_LENGTH = 120;
const INVALID = /[/\\:*?"<>|]/;
const RESERVED = new Set(["CON", "PRN", "AUX", "NUL", ...[1, 2, 3, 4, 5, 6, 7, 8, 9].flatMap((n) => [`COM${n}`, `LPT${n}`])]);
const length = (s) => [...s].length;
// Swift Character.isNewline.
const NEWLINE = /[\n\r\v\f\x85\p{Zl}\p{Zp}]/u;

// Why `name` can't name a preset, or null (PresetName.problem).
export function nameProblem(name) {
  if (!name.trim()) return "Enter a name.";
  if (length(name) > MAX_NAME_LENGTH) return `Use at most ${MAX_NAME_LENGTH} characters.`;
  if (/\p{Cc}/u.test(name) || INVALID.test(name)) return "A name can’t contain / \\ : * ? \" < > or |.";
  if (name.startsWith(".")) return "A name can’t start with a period.";
  if (name.endsWith(".") || name.endsWith(" ") || name.startsWith(" ")) return "A name can’t start with a space or end with a space or period.";
  if (RESERVED.has(name.split(".")[0].toUpperCase().trim())) return `“${name}” is reserved on Windows.`;
  return null;
}

// A file's name as a preset name: invalid characters become spaces
// (PresetName.from(fileName:)).
export function nameFrom(fileName) {
  const trim = (s) => s.replace(/^[ .]+|[ .]+$/g, "");
  let name = trim([...fileName].map((c) => INVALID.test(c) || NEWLINE.test(c) ? " " : c).join("")).replaceAll("  ", " ");
  if (length(name) > MAX_NAME_LENGTH) name = trim([...name].slice(0, MAX_NAME_LENGTH).join(""));
  return nameProblem(name) === null ? name : "Preset";
}

// The name without its extension, as desktop names an imported file.
export const stem = (fileName) => fileName.replace(/\.[^.]*$/, "") || fileName;

// ---- Links --------------------------------------------------------------------
//
// `#preset=` then base64url of a version byte (1) and the raw DEFLATE of the
// UTF-8 JSON {"name": …, "preset": the file's text}. The file stays text, so
// Python's Infinity passes through untouched. Fragments aren't sent to the
// server; the link isn't secret either.

export const LINK_KEY = "preset";
export const LINK_VERSION = 1;
// Longest link offered. A full-precision preset compresses to about 2–3 kB
// (web/checks/presets-link.test.mjs measures them); 16 kB stays clear of
// messaging apps' and browsers' limits while far above that.
export const MAX_LINK_CHARS = 16_384;
// Largest decompressed envelope accepted: a preset file and its name.
export const MAX_ENVELOPE_BYTES = MAX_FILE_BYTES + 4096;

export class LinkError extends Error {}

const canCompress = () => typeof CompressionStream === "function" && typeof DecompressionStream === "function";

async function pipe(bytes, stream, limit = Infinity) {
  const reader = new Blob([bytes]).stream().pipeThrough(stream).getReader();
  const chunks = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.length;
    if (total > limit) {
      await reader.cancel().catch(() => {});
      throw new LinkError("The preset in this link is too large.");
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const c of chunks) { out.set(c, at); at += c.length; }
  return out;
}

const toBase64Url = (bytes) => {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
};
function fromBase64Url(text) {
  if (!/^[A-Za-z0-9_-]*$/.test(text) || text.length % 4 === 1) throw new LinkError("This preset link is damaged or incomplete.");
  const s = atob(text.replaceAll("-", "+").replaceAll("_", "/"));
  return Uint8Array.from(s, (c) => c.charCodeAt(0));
}

// The fragment value for `name` and the preset file's `text`; throws a
// LinkError when this browser can't compress or the link would be too long.
export async function encodeLink(name, text) {
  if (!canCompress()) throw new LinkError("This browser can’t make preset links. Download the preset instead.");
  const json = new TextEncoder().encode(JSON.stringify({ name, preset: text }));
  const packed = await pipe(json, new CompressionStream("deflate-raw"));
  if (Math.ceil(((packed.length + 1) * 4) / 3) + LINK_KEY.length + 2 > MAX_LINK_CHARS) {
    throw new LinkError("This preset is too large for a link. Download the preset to share it.");
  }
  const envelope = new Uint8Array(packed.length + 1);
  envelope[0] = LINK_VERSION;
  envelope.set(packed, 1);
  const value = toBase64Url(envelope);
  if (value.length + LINK_KEY.length + 2 > MAX_LINK_CHARS) {
    throw new LinkError("This preset is too large for a link. Download the preset to share it.");
  }
  return value;
}

// The fragment's preset link value, or null if it has none.
export function linkValue(hash) {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  return params.has(LINK_KEY) ? params.get(LINK_KEY) : null;
}

// A link's { name, text }; throws a LinkError saying why it can't be read.
// The text is only checked to be text: the shared importer validates it.
export async function decodeLink(value) {
  if (!value) throw new LinkError("This preset link is empty.");
  if (value.length + LINK_KEY.length + 2 > MAX_LINK_CHARS) throw new LinkError("This preset link is too long.");
  const bytes = fromBase64Url(value);
  if (!bytes.length) throw new LinkError("This preset link is damaged or incomplete.");
  if (bytes[0] !== LINK_VERSION) throw new LinkError("This preset link is from a newer Negs. Reload to update, then open it again.");
  if (!canCompress()) throw new LinkError("This browser can’t open preset links.");
  let json;
  try {
    json = await pipe(bytes.subarray(1), new DecompressionStream("deflate-raw"), MAX_ENVELOPE_BYTES);
  } catch (e) {
    if (e instanceof LinkError) throw e;
    throw new LinkError("This preset link is damaged or incomplete.");
  }
  let envelope;
  try {
    envelope = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(json));
  } catch {
    throw new LinkError("This preset link is damaged or incomplete.");
  }
  const { name, preset } = envelope ?? {};
  if (typeof preset !== "string" || typeof name !== "string") throw new LinkError("This preset link is damaged or incomplete.");
  return { name: nameFrom(name), text: preset };
}

// The page's address with `value` as its preset link, nothing else in the
// fragment or query (no debug parameters or photo).
export function linkUrl(location, value) {
  return `${location.origin}${location.pathname}#${LINK_KEY}=${value}`;
}
