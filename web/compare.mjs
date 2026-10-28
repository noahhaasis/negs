// Per-pixel agreement of two PPMs (8-bit RGB): node web/compare.mjs a.ppm b.ppm
import { readFileSync } from "node:fs";
const ppm = (f) => { const b = readFileSync(f); let i = 0, fields = [];
  while (fields.length < 4) { let s = ""; while (b[i] > 32) s += String.fromCharCode(b[i++]); i++; if (s) fields.push(s); }
  return { w: +fields[1], h: +fields[2], px: b.subarray(i) }; };
const [a, b] = process.argv.slice(2).map(ppm);
if (a.w !== b.w || a.h !== b.h) { console.log("size differs", a.w, a.h, b.w, b.h); process.exit(); }
let exact = 0, within1 = 0, max = 0; const n = a.w * a.h;
for (let p = 0; p < n; p++) { let m = 0; for (let c = 0; c < 3; c++) m = Math.max(m, Math.abs(a.px[p*3+c] - b.px[p*3+c])); if (!m) exact++; if (m <= 1) within1++; max = Math.max(max, m); }
console.log(`${process.argv[2].split("/").pop()} vs ${process.argv[3].split("/").pop()}: exact ${(exact/n*100).toFixed(3)}%  ≤1 ${(within1/n*100).toFixed(3)}%  max ${max}`);
