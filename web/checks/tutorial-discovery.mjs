// First-open choice persistence and denied-storage fallback. No sample fetch.
export default async ({ send, evaluate }) => {
  const check = (ok, text) => { if (!ok) throw new Error(text); };
  const ready = () => evaluate(`
    for (let i=0;i<1000;i++) { if(globalThis.negs?.tutorial()) return true; await new Promise(r=>setTimeout(r,30)); }
    throw new Error('Editor did not start');
  `);
  await ready();
  check(await evaluate(`return !document.getElementById('tutorial-invitation').hidden && document.getElementById('empty-normal').hidden && document.getElementById('open-empty').getBoundingClientRect().height === 0;`), 'first-open invitation must not also show photo selection');
  await evaluate(`document.getElementById('dismiss-tutorial').click();`);
  check(await evaluate(`return JSON.parse(localStorage.getItem('negs.tutorial')).status === 'dismissed' && document.getElementById('tutorial-invitation').hidden && !document.getElementById('empty-normal').hidden && document.getElementById('open-empty').getBoundingClientRect().height > 0;`), 'dismissal must show photo selection and be remembered');
  await send('Page.reload', { ignoreCache: true });
  await new Promise((r) => setTimeout(r, 500)); await ready();
  check(await evaluate(`return document.getElementById('tutorial-invitation').hidden && document.getElementById('tutorial').getBoundingClientRect().height > 0;`), 'dismissal/replay button did not survive reload');
  await evaluate(`localStorage.setItem('negs.tutorial','broken json');`);
  await send('Page.reload', { ignoreCache: true });
  await new Promise((r) => setTimeout(r, 500)); await ready();
  check(await evaluate(`return !document.getElementById('tutorial-invitation').hidden;`), 'malformed preference broke invitation');
  const injection = await send('Page.addScriptToEvaluateOnNewDocument', { source: `Object.defineProperty(window,'localStorage',{get(){throw new DOMException('Blocked','SecurityError')}});` });
  await send('Page.reload', { ignoreCache: true });
  await new Promise((r) => setTimeout(r, 500)); await ready();
  const denied = await evaluate(`
    document.getElementById('dismiss-tutorial').click();
    return document.getElementById('tutorial-invitation').hidden && !!negs.tutorial() && performance.getEntriesByType('resource').every((r)=>!r.name.endsWith('.dng'));
  `);
  check(denied, 'blocked storage broke dismissal or downloaded a sample');
  await send('Page.removeScriptToEvaluateOnNewDocument', { identifier: injection.result.identifier });
  await evaluate(`history.replaceState(null,'',location.pathname+location.search+'#preset=bad');`);
  await send('Page.reload', { ignoreCache: true });
  await new Promise((r) => setTimeout(r, 500)); await ready();
  check(await evaluate(`return document.getElementById('tutorial-invitation').hidden && document.getElementById('incoming-dialog').open;`), 'tutorial competed with incoming preset');
  return { dismissal: 'persists', malformed: 'safe', denied: 'safe', incomingPreset: 'takes priority', sampleRequests: 0 };
};
