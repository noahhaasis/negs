// The phone layout by touch: tabs, a group, dragging a slider, a vertical
// swipe over a slider (scrolls, doesn't move it), the reset button, the
// sheet's grabber, and a pinch on the photograph (the page doesn't zoom).
//   VIEW=390x844@3m node web/cdp.mjs http://localhost:8137/ web/checks/touch.mjs 2000
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export default async function ({ send, evaluate }) {
  const page = (body) => evaluate(`
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    const settle = async () => { await wait(300); for (let i = 0; i < 400 && !negs.idle(); i++) await wait(25); };
    const g = (path) => path.split(".").reduce((o, k) => o?.[k], negs.state().params) ?? null;
    const rowOf = (label) => [...document.querySelectorAll(".ctl")].find((r) => r.querySelector(".label, label > span")?.textContent === label);
    const center = (e) => { const r = e.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2, left: r.x, width: r.width }; };
    ${body}`);
  const touch = (type, points) => send("Input.dispatchTouchEvent", { type, touchPoints: points.map(([x, y], id) => ({ x, y, id })) });
  // One finger from a to b in steps, as a drag.
  async function drag([x0, y0], [x1, y1], steps = 12) {
    await touch("touchStart", [[x0, y0]]);
    for (let i = 1; i <= steps; i++) { await touch("touchMove", [[x0 + ((x1 - x0) * i) / steps, y0 + ((y1 - y0) * i) / steps]]); await sleep(16); }
    await touch("touchEnd", []);
  }
  const tap = async ({ x, y }) => { await touch("touchStart", [[x, y]]); await sleep(40); await touch("touchEnd", []); await sleep(100); };
  const out = {};

  await page(`for (let i = 0; i < 150 && !document.querySelector("#engine-info").textContent.includes("·"); i++) await wait(100);
    await negs.open("results/card.jpg"); await settle();`);
  out.layout = await page(`return { sheet: getComputedStyle(document.querySelector("#tabs")).display !== "none", rows: getComputedStyle(document.querySelector("#app")).gridTemplateRows };`);

  // Tabs.
  await tap(await page(`return center([...document.querySelectorAll("#tabs button")].find((b) => b.textContent === "Film"));`));
  out.filmTab = await page(`return [...document.querySelectorAll("#scroll > *, #sections > *")].filter((e) => e.offsetParent).map((e) => e.dataset.tab ?? e.className);`);
  // A group opens by tap and is remembered.
  await tap(await page(`return center([...document.querySelectorAll("summary")].find((s) => s.textContent === "Grain" && s.offsetParent));`));
  out.grainOpen = await page(`return { open: [...document.querySelectorAll("details")].find((d) => d.querySelector("summary").textContent === "Grain").open, saved: JSON.parse(localStorage.getItem("negs.groups"))["Film/Grain"] };`);

  // A vertical swipe that starts on a slider (Grain's first) scrolls the sheet and leaves it alone.
  const lane = await page(`const track = [...document.querySelectorAll(".track")].find((t) => t.offsetParent);
    const range = track.querySelector("input"); range.dataset.probe = 1;
    return { ...center(track), value: range.value, top: document.querySelector("#scroll").scrollTop };`);
  await drag([lane.left + lane.width * 0.6, lane.y], [lane.left + lane.width * 0.6, lane.y - 150], 15);
  out.swipe = await page(`await wait(500); return { unchanged: document.querySelector("[data-probe]").value === "${lane.value}",
    scrolled: document.querySelector("#scroll").scrollTop - ${lane.top} };`);

  // Drag Exposure's thumb to the right by touch.
  await tap(await page(`return center([...document.querySelectorAll("#tabs button")].find((b) => b.textContent === "Camera"));`));
  const thumb = await page(`
    const range = rowOf("Exposure (EV)").querySelector("input[type=range]");
    const c = center(range), f = (Number(range.value) - Number(range.min)) / (range.max - range.min);
    return { x: c.left + 13 + (c.width - 26) * f, y: c.y, width: c.width, n: negs.renders.length };`);
  await drag([thumb.x, thumb.y], [thumb.x + thumb.width / 4, thumb.y]);
  out.drag = await page(`await settle(); return { ev: g("camera.exposure_compensation_ev"),
    frames: negs.renders.slice(${thumb.n}).map((r) => (r.interactive ? "i" : "s") + r.width + (r.presented ? "" : "!")).join(" ") };`);

  // The reset button puts Exposure back.
  await page(`document.querySelector("#scroll").scrollTop = 0; await wait(100);`);
  await tap(await page(`return center(rowOf("Exposure (EV)").querySelector(".reset"));`));
  out.reset = await page(`await settle(); return g("camera.exposure_compensation_ev");`);

  // The grabber: drag up snaps the sheet high; a tap steps on to the lowest.
  await page(`negs.sheetHeight = () => document.querySelector("#side").offsetHeight; localStorage.setItem("negs.sheet", '"mid"');`);
  const grab = await page(`return { ...center(document.querySelector("#grabber")), before: negs.sheetHeight(), h: innerHeight };`);
  await drag([grab.x, grab.y], [grab.x, grab.y - 250]);
  const high = await page(`await wait(400); return { height: negs.sheetHeight(), saved: localStorage.getItem("negs.sheet") };`);
  await tap(await page(`return center(document.querySelector("#grabber"));`));
  const low = await page(`await wait(400); return { height: negs.sheetHeight(), saved: localStorage.getItem("negs.sheet") };`);
  out.sheet = { before: grab.before, high, low, screen: grab.h };

  // A mouse on a touch screen (an iPad's trackpad) sets the value under it:
  // pressing three quarters along Film format, then dragging to a quarter.
  await tap(await page(`return center(document.querySelector("#grabber"));`));
  await tap(await page(`return center(document.querySelector("#grabber"));`));
  await tap(await page(`return center([...document.querySelectorAll("#tabs button")].find((b) => b.textContent === "Camera"));`));
  const format = await page(`const t = rowOf("Film format (mm)").querySelector(".track"); t.scrollIntoView({ block: "center" }); await wait(100); return center(t);`);
  const travel = format.width - 26, x0 = format.left + 13;
  const mouse = (type, x) => send("Input.dispatchMouseEvent", { type, x, y: format.y, button: "left", buttons: type === "mouseReleased" ? 0 : 1, clickCount: 1 });
  await mouse("mousePressed", x0 + travel * 0.75);
  const pressed = await page(`return g("camera.film_format_mm");`);
  await mouse("mouseMoved", x0 + travel * 0.5);
  await mouse("mouseMoved", x0 + travel * 0.25);
  await mouse("mouseReleased", x0 + travel * 0.25);
  out.mouse = { pressed, released: await page(`await settle(); return g("camera.film_format_mm");`), range: await page(`const r = rowOf("Film format (mm)").querySelector("input"); return [r.min, r.max];`) };

  // A pinch on the photograph doesn't zoom the page.
  const view = await page(`return center(document.querySelector("#view"));`);
  await touch("touchStart", [[view.x - 20, view.y], [view.x + 20, view.y]]);
  for (let i = 1; i <= 10; i++) { await touch("touchMove", [[view.x - 20 - 10 * i, view.y], [view.x + 20 + 10 * i, view.y]]); await sleep(16); }
  await touch("touchEnd", []);
  out.pinch = await page(`await wait(300); return { scale: visualViewport.scale, scrollY };`);
  return out;
}
