// One sample/session across rotations, viewports and enlarged text.
export default async ({ send, evaluate }) => {
  const check = (ok, text) => { if (!ok) throw new Error(text); };
  await evaluate(`
    globalThis.wait = (ms) => new Promise(r => setTimeout(r, ms));
    globalThis.until = async (test) => { for (let i=0;i<4000;i++) { if(test()) return; await wait(30); } throw new Error('Timed out'); };
    await until(() => !!globalThis.negs?.tutorial());
    document.getElementById('start-tutorial').click();
    await until(() => negs.tutorial().phase === 'active');
    document.getElementById('tour-next').click();
    await until(() => negs.tutorial().phase === 'active' && negs.tutorial().index === 1);
    document.getElementById('tour-next').click();
    await until(() => negs.tutorial().phase === 'active' && negs.tutorial().index === 2);
    document.getElementById('tour-next').click();
    await until(() => negs.tutorial().phase === 'active' && negs.tutorial().index === 3);
    negs.edit('camera.exposure_compensation_ev', 0.4);
    await until(() => negs.idle());
  `);
  const sizes = [[320,568],[390,844],[844,390],[768,1024],[1024,768],[1440,900]];
  const results = [];
  await send('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
  async function layout() {
    return evaluate(`
      await wait(500);
      const scroll = document.getElementById('scroll');
      const target = document.querySelector('.tour-control'); target.scrollIntoView({block:'nearest'});
      const s = scroll.getBoundingClientRect();
      const reachable = [...target.querySelectorAll('input,select,button')].filter((n) => n.getBoundingClientRect().height > 0).every((n) => {
        n.scrollIntoView({block:'nearest'}); const r = n.getBoundingClientRect();
        return r.top >= s.top-1 && r.bottom <= s.bottom+1 && r.right <= innerWidth+1;
      });
      const end = document.getElementById('tour-end').getBoundingClientRect();
      const nav = document.getElementById('tutorial-nav').getBoundingClientRect();
      const view = document.getElementById('view').getBoundingClientRect();
      return { size:[innerWidth,innerHeight], target:reachable,
        end:end.top >=0 && end.bottom <= innerHeight && end.height >=44,
        nav:nav.bottom <= innerHeight+1 && nav.right <= innerWidth+1,
        overflow:document.documentElement.scrollWidth > innerWidth,
        photo:view.height >=90 && view.width >=100,
        step:negs.tutorial().index, exposure:negs.state().params.camera.exposure_compensation_ev };
    `);
  }
  for (const [width,height] of sizes) {
    await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: true });
    const r = await layout();
    check(r.target && r.end && r.nav && r.photo && !r.overflow && r.step === 3 && r.exposure === 0.4, `resize failed: ${JSON.stringify(r)}`);
    results.push(r);
  }
  await send('Emulation.setDeviceMetricsOverride', { width: 320, height: 568, deviceScaleFactor: 1, mobile: true });
  await evaluate(`
    const style = document.createElement('style'); style.id = 'large-tour-text';
    style.textContent = '.tour-guide p,.tour-guide h2,.tour-heading,.tour-nav,.tour-control { font-size: 200% !important; }';
    document.head.append(style);
  `);
  const large = await layout();
  check(large.target && large.end && large.nav && !large.overflow, `large text failed: ${JSON.stringify(large)}`);
  await evaluate(`document.getElementById('large-tour-text').remove();`);
  // Approximate the reduced visual viewport available above a software keyboard.
  await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 420, deviceScaleFactor: 1, mobile: true });
  const keyboard = await layout();
  check(keyboard.target && keyboard.end && keyboard.nav && !keyboard.overflow, `short viewport failed: ${JSON.stringify(keyboard)}`);
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'c', code: 'KeyC' });
  check(await evaluate(`return !negs.cropSession();`), 'hidden crop shortcut hijacked tutorial');
  await evaluate(`document.getElementById('tour-end').click(); await until(() => negs.tutorial().phase === 'idle');`);
  return { sizes: results, large, keyboard };
};
