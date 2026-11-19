// Proves scripts/smoke-web.mjs fails on each startup failure it guards against,
// using small pages in a temp directory instead of the real build.
import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";
import { smokeWeb } from "./smoke-web.mjs";

const dirs = [];
after(() => Promise.all(dirs.map((dir) => rm(dir, { recursive: true, force: true }))));

/** Smoke-tests a one-page site whose body is `body`. */
async function smoke(body) {
  const root = await mkdtemp(join(tmpdir(), "smoke-web-"));
  dirs.push(root);
  await writeFile(
    join(root, "index.html"),
    `<!doctype html><html><head><link rel="icon" href="data:,"></head><body>${body}</body></html>`,
  );
  return smokeWeb({ root, timeoutMs: 3_000, settleMs: 300 });
}

const shell = '<div data-testid="editor-shell">editor</div>';

test("passes when the editor shell mounts cleanly", async () => {
  const result = await smoke(shell);
  assert.deepEqual(result.failures, []);
  assert.equal(result.ok, true);
});

test("fails on an uncaught exception, with its message", async () => {
  const result = await smoke(
    `${shell}<script>throw new Error('Action "machine.frame" is already registered')</script>`,
  );
  assert.equal(result.ok, false);
  assert.match(result.failures.join("\n"), /Action "machine\.frame" is already registered/);
});

test("fails on a console.error after mount", async () => {
  const result = await smoke(
    `${shell}<script>setTimeout(() => console.error("late failure"), 100)</script>`,
  );
  assert.equal(result.ok, false);
  assert.match(result.failures.join("\n"), /console\.error: late failure/);
});

test("fails when SvelteKit's error page is showing", async () => {
  const result = await smoke("<h1>500</h1><p>Internal Error</p>");
  assert.equal(result.ok, false);
  assert.match(result.failures.join("\n"), /error page is showing \(h1 "500"\)/);
});

test("fails when the editor shell marker is missing", async () => {
  const result = await smoke("<div>editor without its marker</div>");
  assert.equal(result.ok, false);
  assert.match(result.failures.join("\n"), /editor shell .* is not on the page/);
});
