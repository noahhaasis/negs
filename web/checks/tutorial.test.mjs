import test from 'node:test';
import assert from 'node:assert/strict';
import { STEPS, tutorialChoice, createTutorialSession } from '../tutorial.js';

const deferred = () => { let resolve; const promise = new Promise((r) => { resolve = r; }); return { promise, resolve }; };
function fixture(overrides = {}) {
  const calls = [], choices = [], saved = { original: true };
  const adapter = {
    capture: () => { calls.push('capture'); return saved; },
    enter: async () => { calls.push('enter'); },
    step: async (step) => { calls.push(step.id); },
    cancel: () => { calls.push('cancel'); },
    restore: async (snapshot) => { assert.equal(snapshot, saved); calls.push('restore'); },
    remember: (choice) => choices.push(choice),
    completed: () => calls.push('completed'),
    ...overrides,
  };
  const updates = [], session = createTutorialSession(adapter, (s) => updates.push(s));
  return { session, calls, choices, updates };
}

test('recognizes only explicit tutorial choices, including older versions', () => {
  for (const value of [null, false, 'completed', {}, { status: 'anything' }]) assert.equal(tutorialChoice(value), false);
  for (const status of ['started', 'dismissed', 'completed']) assert.equal(tutorialChoice({ version: 0, status }), true);
});

test('nine stable steps; normal paper and print exposure have their own lessons', () => {
  assert.equal(STEPS.length, 9);
  assert.equal(new Set(STEPS.map((s) => s.id)).size, 9);
  assert.equal(STEPS[1].target, 'paper');
  assert.deepEqual(STEPS[7].paths, ['enlarger.print_exposure']);
});

test('start, reset, back, finish: no edit is required to continue', async () => {
  const { session: s, calls, choices } = fixture();
  await s.start();
  assert.equal(s.state().phase, 'active');
  await s.go(1); await s.go(0); await s.reset();
  assert.deepEqual(calls, ['capture', 'enter', 'film', 'paper', 'film', 'film']);
  for (let i = 1; i < STEPS.length; i++) await s.go(i);
  await s.end(true);
  assert.equal(s.state().phase, 'idle');
  assert.deepEqual(calls.slice(-3), ['cancel', 'restore', 'completed']);
  assert.deepEqual(choices.map((c) => c.status), ['started', 'completed']);
  assert.ok(choices.every((c) => Object.keys(c).sort().join() === 'status,version'));
});

test('completion tells the editor to show photo selection on first run', async () => {
  let options;
  const { session: s } = fixture({ restore: async (_, value) => { options = value; } });
  await s.start(); await s.end(true);
  assert.deepEqual(options, { completed: true });
});

test('duplicate start and rapid Next cannot race', async () => {
  const pending = deferred();
  const { session: s, calls } = fixture({ step: async (step) => { calls.push(step.id); await pending.promise; } });
  const starting = s.start();
  s.start(); s.go(1);
  await Promise.resolve();
  assert.equal(calls.filter((c) => c === 'capture').length, 1);
  assert.equal(s.state().index, 0);
  pending.resolve(); await starting;
  await s.end();
});

test('cancel aborts immediately, drains pending work, then restores once', async () => {
  const pending = deferred(); let signal;
  const { session: s, calls } = fixture({ enter: async (s) => { signal = s; calls.push('enter'); await pending.promise; } });
  const starting = s.start();
  const ending = s.end();
  assert.ok(signal.aborted);
  assert.equal(s.state().phase, 'restoring');
  assert.ok(!calls.includes('restore'));
  s.end();
  pending.resolve();
  await Promise.all([starting, ending]);
  assert.equal(s.state().phase, 'idle');
  assert.equal(calls.filter((c) => c === 'restore').length, 1);
  assert.ok(!calls.includes('film'));
});

test('load failure can retry without capturing the failed sample as the original', async () => {
  let fail = true;
  const { session: s, calls } = fixture({ enter: async () => { if (fail) throw new Error('offline'); } });
  await s.start();
  assert.equal(s.state().phase, 'error');
  assert.equal(s.state().error, 'offline');
  fail = false;
  await s.retry();
  assert.equal(s.state().phase, 'active');
  assert.equal(calls.filter((c) => c === 'capture').length, 1);
  await s.end();
});

test('restoration failure retains the snapshot and Retry retries restoration', async () => {
  let attempts = 0;
  const { session: s, calls } = fixture({ restore: async () => { if (++attempts === 1) throw new Error('engine stopped'); } });
  await s.start(); await s.end();
  assert.equal(s.state().phase, 'restore-error');
  await s.retry();
  assert.equal(s.state().phase, 'idle');
  assert.equal(attempts, 2);
  assert.equal(calls.filter((c) => c === 'enter').length, 1);
});
