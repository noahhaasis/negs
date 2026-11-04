// The Settings sheet on a phone: the previews' default (nothing
// remembered: on) and the gear's sheet turning them off and on.
//   node web/safari.mjs run web/checks/settings-phone.mjs
export default async function (test) {
  const { execute, wait, log, screenshot } = test;
  const ready = async () => {
    for (let i = 0; i < 120; i++) {
      if (await execute("return !!globalThis.negs && /thread|CPU|GPU/i.test(document.getElementById('engine-info')?.textContent ?? '');").catch(() => false)) return true;
      await wait(500);
    }
    return false;
  };
  const shot = async (name) => { if (process.env.SAFARI_SCREENSHOTS !== "0") await screenshot(name).catch((e) => log("screenshot-failed", { message: e.message })); };
  await ready();
  // Nothing remembered: the default.
  await execute("localStorage.removeItem('negs.stockPreviews'); location.reload();");
  await wait(2000);
  await ready();
  const fresh = await execute("return { on: negs.previews().on, alive: localStorage.getItem('negs.alive'), nav: performance.getEntriesByType('navigation')[0]?.type, message: document.getElementById('message').hidden ? null : document.getElementById('message').textContent };");
  log("default", fresh);
  // The gear's sheet.
  await execute("document.getElementById('settings').click();");
  await wait(500);
  const sheet = await execute(`const d = document.getElementById('settings-dialog'), r = d.getBoundingClientRect();
    return { open: d.open, previews: document.getElementById('previews').checked, match: document.getElementById('match-new-raws').checked,
      engine: document.getElementById('engine').value, rect: [r.x, r.y, r.width, r.height].map(Math.round), viewport: [innerWidth, innerHeight] };`);
  log("sheet", sheet);
  await shot("settings-sheet");
  await execute("document.getElementById('previews').click();");
  await wait(300);
  const off = await execute("return { on: negs.previews().on, stored: localStorage.getItem('negs.stockPreviews'), alive: localStorage.getItem('negs.alive') };");
  log("switched off", off);
  await execute("document.getElementById('previews').click(); document.getElementById('settings-dialog').close();");
  await wait(300);
  const on = await execute("return { on: negs.previews().on, alive: localStorage.getItem('negs.alive') };");
  log("switched on", on);
  // A memory crash can't be checked here: under automation the phone lifts
  // the tab's memory limit, and killing the tab's process ends this session
  // instead of reloading it. docs/web-prep-port-plan.md has the manual check.
  return { fresh, sheet, off, on };
}
