// RAW input white balance in the editor (docs/raw-white-balance.md): which
// developments differ, and the queue every change of the engine's decoded
// photograph goes through. The WB itself is spektrafilm-io's, applied when
// the engine (or web/decoder.js) develops a RAW's bytes; nothing here
// approximates it.

export const RAW_WB_PATH = "io.raw_white_balance";

// What a RAW develops differently at: the mode, and the temperature and tint
// only while Custom uses them. Inactive custom values never re-develop.
export function wbKey(wb) {
  const mode = wb?.mode ?? "as_shot";
  return mode === "custom" ? `custom:${Number(wb.temperature)}:${Number(wb.tint)}` : String(mode);
}
export const sameWb = (a, b) => wbKey(a) === wbKey(b);

// Whether the loader read a photograph as camera RAW (spektrafilm-io's
// InputColour JSON, `source`), not by its file name.
export function isRawColour(colour) {
  try {
    return (typeof colour === "string" ? JSON.parse(colour) : colour)?.source === "raw";
  } catch {
    return false;
  }
}

// Runs the operations that replace the engine's photograph one at a time,
// in the order asked: an open, a re-development at another WB, a reload
// after an engine restart. Then the photograph the engine holds is always
// the last one installed, whatever each took. `busy` counts those queued
// or running.
export function createSourceQueue() {
  let tail = Promise.resolve(), queued = 0;
  return {
    get busy() { return queued > 0; },
    run(operation) {
      queued++;
      const done = tail.then(operation);
      tail = done.catch(() => {});
      return done.finally(() => { queued--; });
    },
  };
}
