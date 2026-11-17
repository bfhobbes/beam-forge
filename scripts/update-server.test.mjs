import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { once } from "node:events";
import { createUpdateServer } from "./update-server.mjs";

test("local service selects newer compatible releases and serves exact package bytes", async () => {
  const directory = await mkdtemp(join(tmpdir(), "beam-forge-updates-"));
  const server = createUpdateServer(directory, 17845);
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const url = `http://127.0.0.1:${server.address().port}`;
  try {
    assert.equal((await fetch(`${url}/updates/windows/x86_64/0.1.1`)).status, 204);
    await mkdir(join(directory, "artifacts"));
    await writeFile(join(directory, "artifacts/update.exe"), "package bytes");
    await writeFile(
      join(directory, "releases.json"),
      JSON.stringify({
        "windows-x86_64": {
          version: "0.2.1",
          file: "update.exe",
          signature: "signature",
          notes: "Notes",
        },
      }),
    );
    const response = await fetch(`${url}/updates/windows/x86_64/0.1.1`);
    assert.equal(response.status, 200);
    assert.equal((await response.json()).version, "0.2.1");
    for (const route of ["windows/x86_64/0.2.1", "windows/x86_64/1.0.0", "darwin/aarch64/0.1.1"])
      assert.equal((await fetch(`${url}/updates/${route}`)).status, 204);
    assert.equal((await fetch(`${url}/updates/windows/x86_64/nope`)).status, 400);
    assert.equal(await (await fetch(`${url}/artifacts/update.exe`)).text(), "package bytes");
    assert.equal((await fetch(`${url}/artifacts/..%5Creleases.json`)).status, 400);
    assert.equal((await fetch(`${url}/artifacts/missing.exe`)).status, 404);
    assert.equal(
      (await fetch(`${url}/updates/windows/x86_64/0.1.1`, { method: "POST" })).status,
      405,
    );
  } finally {
    server.close();
    server.closeAllConnections();
    await once(server, "close");
    await rm(directory, { recursive: true, force: true });
  }
});
