// node web/cdp.mjs http://127.0.0.1:8140/ scripts/accuracy/site-check.mjs 1500
import assert from 'node:assert/strict';
export default async ({ evaluate }) => {
  const initial = await evaluate(`await new Promise(r=>setTimeout(r,1000));
    return {title:document.title,panels:document.querySelectorAll('#panels canvas').length,
      scope:document.getElementById('scope').textContent,case:document.getElementById('metadata').textContent,
      errors:document.getElementById('legend').textContent};`);
  assert.match(initial.title, /accuracy/);
  assert.equal(initial.panels, 3);
  assert.match(initial.scope, /PRIMARY|DIAGNOSTIC/);
  assert.doesNotMatch(initial.errors, /Missing image:/);
  const detailed = await evaluate(`
    document.getElementById('pairB').value='installed-gpu';
    document.getElementById('pairB').dispatchEvent(new Event('change'));
    document.getElementById('actual').click();
    await new Promise(r=>setTimeout(r,1500));
    const canvas=document.getElementById('difference'),ctx=canvas.getContext('2d');
    const pixel=Array.from(ctx.getImageData(Math.floor(canvas.width/2),Math.floor(canvas.height/2),1,1).data);
    const first={position:document.getElementById('position').textContent,
      numbers:document.getElementById('numbers').textContent,pixel,legend:document.getElementById('legend').textContent};
    document.getElementById('source').click();
    document.getElementById('native').value='both';
    document.getElementById('native').dispatchEvent(new Event('change'));
    await new Promise(r=>setTimeout(r,500));
    return {...first,allPanels:document.querySelectorAll('#panels canvas').length,
      fullImageRequests:performance.getEntriesByType('resource').map(r=>r.name).filter(n=>/output\.(png|ppm)$/.test(n)),
      tileRequests:performance.getEntriesByType('resource').filter(r=>/tile-/.test(r.name)).length};`);
  assert.match(detailed.position, /100%/);
  assert.equal(detailed.allPanels, 6);
  assert.equal(detailed.fullImageRequests.length, 0, 'normal viewing must not decode full-resolution originals');
  assert.ok(detailed.tileRequests > 0);
  assert.equal(detailed.pixel[3], 255, 'difference tiles rendered');
  assert.doesNotMatch(detailed.legend, /Missing image:/);
  return detailed;
};
