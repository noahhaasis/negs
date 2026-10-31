// After `web/safari.mjs page https://HOST:8443/checks/golden.html?...`,
// run this with `web/safari.mjs run web/checks/golden-safari.mjs`.
// Start asynchronously to avoid Safari's 30 s single-script timeout.
export default async ({execute, waitFor}) => {
  await execute(`
    globalThis.__goldenDone = null;
    globalThis.__goldenError = null;
    startGolden().then(r => globalThis.__goldenDone = r,
      e => globalThis.__goldenError = String(e.stack ?? e));
  `);
  await waitFor('globalThis.__goldenDone || globalThis.__goldenError', {timeout: 1800000});
  const result = await execute('return {result: __goldenDone, error: __goldenError};');
  if (result.error) throw new Error(result.error);
  return result.result;
};
