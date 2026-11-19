// Smoke test for the built frontend: serves build/ (SPA fallback to index.html),
// loads it in a headless system browser and fails on any uncaught exception or
// console.error, on SvelteKit's error page, or if the editor shell never mounts.
// Unit tests mount components in isolation and the build never runs the page, so
// this is the only check that the shipped editor actually starts (0.2.0 shipped
// opening to a 500 page). Uses playwright-core with the system browser: Edge on
// Windows, Chrome elsewhere; override with SMOKE_BROWSER_CHANNEL.
//
//   node scripts/smoke-web.mjs [root]   # root defaults to build/
//
// On failure it writes a screenshot and the captured console to target/smoke-web/.
import { createReadStream } from "node:fs";
import { mkdir, stat, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const repo = resolve(fileURLToPath(import.meta.url), "../..");
export const SHELL_SELECTOR = '[data-testid="editor-shell"]';

/**
 * Console errors the browser preview is known to log at startup, matched by
 * message. Each entry needs a comment saying why it is expected. Keep it empty
 * unless there is no way to fix the cause.
 * @type {RegExp[]}
 */
const ALLOWED_CONSOLE_ERRORS = [];

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".wasm": "application/wasm",
  ".txt": "text/plain; charset=utf-8",
};

/** Serves `root` on 127.0.0.1 with SPA fallback; resolves to the server and its URL. */
export async function serve(root) {
  const base = resolve(root);
  const server = createServer(async (request, response) => {
    const path = decodeURIComponent(new URL(request.url ?? "/", "http://localhost").pathname);
    let file = resolve(base, `.${path}`);
    if (file !== base && !file.startsWith(base + sep)) {
      response.writeHead(403).end();
      return;
    }
    const info = await stat(file).catch(() => null);
    if (info?.isDirectory()) file = join(file, "index.html");
    else if (!info) {
      // Assets that don't exist are real 404s; routes fall back to the SPA shell.
      if (extname(path)) {
        response.writeHead(404).end();
        return;
      }
      file = join(base, "index.html");
    }
    const type = MIME[extname(file)] ?? "application/octet-stream";
    response.writeHead(200, { "content-type": type, "cache-control": "no-store" });
    createReadStream(file)
      .on("error", () => response.destroy())
      .pipe(response);
  });
  await new Promise((done, fail) => server.once("error", fail).listen(0, "127.0.0.1", done));
  const { port } = /** @type {import("node:net").AddressInfo} */ (server.address());
  return { server, url: `http://127.0.0.1:${port}/` };
}

/**
 * Loads `root` and checks the editor starts cleanly.
 * @param {{ root: string, channel?: string, timeoutMs?: number, settleMs?: number, artifacts?: string | null }} options
 * @returns {Promise<{ ok: boolean, failures: string[], console: string[] }>}
 */
export async function smokeWeb({
  root,
  channel = process.env.SMOKE_BROWSER_CHANNEL ||
    (process.platform === "win32" ? "msedge" : "chrome"),
  timeoutMs = 15_000,
  settleMs = 1_500,
  artifacts = null,
}) {
  const { server, url } = await serve(root);
  const browser = await chromium.launch({ channel, headless: true });
  /** @type {string[]} */
  const failures = [];
  /** @type {string[]} */
  const log = [];
  /** @type {(reason: string) => void} */
  let failed = () => {};
  const firstFailure = new Promise((done) => (failed = done));
  const fail = (/** @type {string} */ reason) => {
    failures.push(reason);
    failed(reason);
  };

  try {
    const page = await browser.newPage();
    page.on("console", (message) => {
      const location = message.location();
      const where = location.url ? ` (${location.url}:${location.lineNumber})` : "";
      log.push(`[${message.type()}] ${message.text()}${where}`);
      if (message.type() !== "error") return;
      if (ALLOWED_CONSOLE_ERRORS.some((allowed) => allowed.test(message.text()))) return;
      fail(`console.error: ${message.text()}${where}`);
    });
    page.on("pageerror", (error) => {
      log.push(`[pageerror] ${error.stack ?? error.message}`);
      fail(`uncaught exception: ${error.stack ?? error.message}`);
    });

    try {
      await page.goto(url, { waitUntil: "load", timeout: timeoutMs });
      // Wait for the editor (or the first error), then keep listening briefly
      // for errors thrown by effects that run after mount.
      const mounted = await Promise.race([
        page
          .waitForFunction(
            (selector) =>
              document.querySelector(selector) !== null ||
              /^[45]\d\d$/.test(document.querySelector("h1")?.textContent?.trim() ?? ""),
            SHELL_SELECTOR,
            { timeout: timeoutMs },
          )
          .then(() => page.locator(SHELL_SELECTOR).count())
          .then((count) => count > 0)
          .catch(() => false),
        firstFailure.then(() => false),
      ]);
      if (mounted) {
        await Promise.race([page.waitForTimeout(settleMs), firstFailure]);
      } else {
        // Give SvelteKit a moment to finish rendering its error page for the report.
        await page.waitForTimeout(500);
      }
      const heading = (
        await page
          .locator("h1")
          .first()
          .textContent({ timeout: 1_000 })
          .catch(() => null)
      )?.trim();
      if (heading && /^[45]\d\d$/.test(heading)) {
        fail(`SvelteKit's error page is showing (h1 "${heading}")`);
      }
      if ((await page.locator(SHELL_SELECTOR).count()) === 0) {
        fail(`the editor shell (${SHELL_SELECTOR}) is not on the page`);
      }
    } catch (error) {
      fail(`page did not load: ${error instanceof Error ? error.message : String(error)}`);
    }

    if (failures.length > 0 && artifacts) {
      await mkdir(artifacts, { recursive: true });
      await page
        .screenshot({ path: join(artifacts, "screenshot.png"), fullPage: true })
        .catch(() => {});
      await writeFile(join(artifacts, "console.log"), `${log.join("\n")}\n`);
    }
  } finally {
    await browser.close();
    await new Promise((done) => server.close(done));
  }
  return { ok: failures.length === 0, failures, console: log };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = resolve(process.argv[2] ?? join(repo, "build"));
  const artifacts = join(repo, "target", "smoke-web");
  if (!(await stat(join(root, "index.html")).catch(() => null))) {
    console.error(`FAIL: ${root} has no index.html; run npm run build first.`);
    process.exit(1);
  }
  const started = Date.now();
  const result = await smokeWeb({ root, artifacts });
  const seconds = ((Date.now() - started) / 1000).toFixed(1);
  if (result.ok) {
    console.log(`PASS: the editor shell mounted with no console errors (${seconds} s).`);
  } else {
    console.error("Browser console:");
    for (const line of result.console) console.error(`  ${line}`);
    for (const failure of result.failures) console.error(`FAIL: ${failure}`);
    console.error(`Screenshot and console saved to ${artifacts}`);
    process.exit(1);
  }
}
