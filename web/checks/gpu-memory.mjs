// GPU memory over a long session: cycles photographs, sizes and effects in
// one page and logs, after each frame read back, the memory nvidia-smi
// reports for this Chrome's GPU process, and whether the frame came out
// black (what a failed allocation gives). Needs an NVIDIA GPU.
//   node web/cdp.mjs http://localhost:8137/ web/checks/gpu-memory.mjs 3000
// ROUNDS (default 6) sets how many times the cycle runs.
import { execFileSync } from "node:child_process";

const gpuMiB = () => {
  const pids = new Set(execFileSync("pgrep", ["-f", "chrome.*--type=gpu-process"]).toString().trim().split("\n"));
  const rows = execFileSync("nvidia-smi", ["--query-compute-apps=pid,used_memory", "--format=csv,noheader,nounits"]).toString().trim().split("\n");
  return rows.map((r) => r.split(",").map((s) => s.trim())).filter(([pid]) => pids.has(pid)).reduce((sum, [, mib]) => sum + Number(mib), 0);
};

export default async ({ evaluate }) => {
  await evaluate(`
    globalThis.wait = (ms) => new Promise((r) => setTimeout(r, ms));
    globalThis.settle = async (n) => { for (let i = 0; i < 2400 && (negs.renders.length <= n || !negs.idle()); i++) await wait(25); await wait(30); };
    globalThis.frame = async (image, film, edits, max) => {
      let n = negs.renders.length;
      if (globalThis.opened !== image) { await negs.open(image); await settle(n); globalThis.opened = image; }
      n = negs.renders.length;
      negs.start(film, null, edits);
      await settle(n);
      const r = await negs.read(max);
      let s = 0, k = 0;
      for (let i = 0; i < r.rgba.length; i += 4 * 101, k++) s += r.rgba[i] + r.rgba[i + 1] + r.rgba[i + 2];
      return { size: r.width + "x" + r.height, mean: +(s / k / 3).toFixed(1) };
    };`);
  const cycle = [
    ["results/card.jpg", "kodak_portra_400", {}, 1 << 20],
    ["results/cliffs.jpg", "fujifilm_velvia_100", { "camera.diffusion_filter.active": true }, 1 << 20],
    ["results/sony.arw", "kodak_gold_200", {}, 1 << 20],
    ["results/sony.arw", "kodak_gold_200", { "enlarger.diffusion_filter.active": true }, 3000],
    ["results/hills.jpg", "kodak_vision3_500t", {}, 1 << 20],
  ];
  const out = [`start ${gpuMiB()} MiB`];
  for (let round = 0; round < Number(process.env.ROUNDS ?? 6); round++) {
    for (const [image, film, edits, max] of cycle) {
      const f = await evaluate(`return frame(${JSON.stringify(image)}, ${JSON.stringify(film)}, ${JSON.stringify(edits)}, ${max})`);
      const line = `${round} ${image.split("/").at(-1)} ${film} ${f.size} mean ${f.mean} gpu ${gpuMiB()} MiB`;
      console.log(line);
      out.push(line);
    }
  }
  return out;
};
