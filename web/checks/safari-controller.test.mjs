// No physical device required: verify screenshot-free controller operation,
// including the automatic failure path, against a tiny WebDriver stub.
import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

for (const command of ["screenshot", "eval"]) test(`SAFARI_SCREENSHOTS=0 skips ${command === "eval" ? "automatic failure" : "explicit"} captures`, async () => {
  const paths = [];
  let executed = false;
  const server = createServer((req, res) => {
    paths.push(req.url);
    res.setHeader("Content-Type", "application/json");
    if (command === "eval" && !executed) {
      executed = true;
      res.writeHead(500).end(JSON.stringify({ value: { error: "javascript error", message: "intentional test failure" } }));
    } else res.end(JSON.stringify({ value: {} }));
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  const root = await mkdtemp(join(tmpdir(), "negs-safari-test-"));
  try {
    await writeFile(join(root, "session.json"), JSON.stringify({ id: "test", driver: `http://127.0.0.1:${server.address().port}` }));
    const script = join(root, "eval.js");
    await writeFile(script, "throw new Error('intentional test failure');");
    const child = spawn(process.execPath, ["web/safari.mjs", command, command === "eval" ? script : "test"], {
      env: { ...process.env, SAFARI_TEST_DIR: root, SAFARI_SCREENSHOTS: "0", SAFARI_DRIVER_URL: `http://127.0.0.1:${server.address().port}` },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "";
    child.stdout.on("data", chunk => { output += chunk; });
    child.stderr.on("data", chunk => { output += chunk; });
    const code = await new Promise((resolve, reject) => { child.on("exit", resolve); child.on("error", reject); });
    assert.equal(code, command === "eval" ? 1 : 0, output);
    assert.match(output, /screenshot-skipped/);
    assert.ok(!paths.some(path => path.endsWith("/screenshot")), "no device screenshot request even after failure");
    if (command === "eval") assert.match(output, /intentional test failure/);
  } finally {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    await rm(root, { recursive: true, force: true });
  }
});
