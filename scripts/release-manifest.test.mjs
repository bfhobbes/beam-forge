import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildManifest, findPackages, verifySignature } from "./release-manifest.mjs";

// Packages signed by `tauri signer sign` with a throwaway key.
const bundles = fileURLToPath(new URL("./fixtures/release-bundles/", import.meta.url));
const publicKey = readFileSync(new URL("./fixtures/release-bundles.pub", import.meta.url), "utf8");
const appImage = join(bundles, "linux/appimage/Beam Forge_9.9.9_amd64.AppImage");

// What GitHub's release API returns: spaces in asset names become dots.
const release = {
  body: "## What's Changed\n* Linux\n",
  published_at: "2026-11-15T12:00:00Z",
  assets: [
    "Beam.Forge_9.9.9_x64-setup.exe",
    "Beam.Forge_9.9.9_x64_en-US.msi",
    "Beam.Forge_9.9.9_amd64.AppImage",
    "Beam.Forge_9.9.9_amd64.deb",
    "Beam.Forge.Simulator_9.9.9_amd64.deb",
  ].map((name, id) => ({ name, url: `https://api.github.com/assets/${id}` })),
};

test("Tauri updater signatures verify against the public key", () => {
  const bytes = readFileSync(appImage);
  const signature = readFileSync(`${appImage}.sig`, "utf8");
  verifySignature(bytes, signature, publicKey);
  assert.throws(
    () => verifySignature(Buffer.concat([bytes, Buffer.from("!")]), signature, publicKey),
    /doesn't match the package/,
  );
  const other = readFileSync(join(bundles, "linux/deb/Beam Forge_9.9.9_amd64.deb.sig"), "utf8");
  assert.throws(() => verifySignature(bytes, other, publicKey), /doesn't match/);
  // A key with a different id is rejected before the signature is checked.
  const [comment, line] = Buffer.from(publicKey, "base64").toString().split("\n");
  const key = Buffer.from(line, "base64");
  key[2] ^= 1;
  const otherKey = Buffer.from(`${comment}\n${key.toString("base64")}\n`).toString("base64");
  assert.throws(() => verifySignature(bytes, signature, otherKey), /different key/);
});

test("the manifest lists every platform with its signature and API asset URL", () => {
  const found = findPackages(bundles, "9.9.9", publicKey);
  const manifest = buildManifest({ version: "9.9.9", release, found });
  assert.equal(manifest.version, "9.9.9");
  assert.equal(manifest.notes, "## What's Changed\n* Linux");
  assert.equal(manifest.pub_date, "2026-11-15T12:00:00Z");
  assert.deepEqual(
    Object.fromEntries(Object.entries(manifest.platforms).map(([key, { url }]) => [key, url])),
    {
      "windows-x86_64-nsis": "https://api.github.com/assets/0",
      "windows-x86_64-msi": "https://api.github.com/assets/1",
      "linux-x86_64-appimage": "https://api.github.com/assets/2",
      "linux-x86_64-deb": "https://api.github.com/assets/3",
      "windows-x86_64": "https://api.github.com/assets/0",
      "linux-x86_64": "https://api.github.com/assets/2",
    },
  );
  assert.equal(
    manifest.platforms["linux-x86_64"].signature,
    readFileSync(`${appImage}.sig`, "utf8").trim(),
  );
});

test("missing assets, packages and bad signatures stop the manifest", () => {
  const found = findPackages(bundles, "9.9.9", publicKey);
  const partial = { ...release, assets: release.assets.filter((a) => !a.name.endsWith(".deb")) };
  assert.throws(
    () => buildManifest({ version: "9.9.9", release: partial, found }),
    /no asset ending in _9.9.9_amd64.deb/,
  );
  assert.throws(() => findPackages(bundles, "1.0.0", publicKey), /found 0/);

  const dir = mkdtempSync(join(tmpdir(), "beam-forge-manifest-"));
  try {
    cpSync(bundles, dir, { recursive: true });
    const copy = join(dir, "linux/appimage/Beam Forge_9.9.9_amd64.AppImage");
    writeFileSync(copy, "tampered");
    assert.throws(() => findPackages(dir, "9.9.9", publicKey), /AppImage: .*doesn't match/);
    rmSync(`${copy}.sig`);
    assert.throws(() => findPackages(dir, "9.9.9", publicKey), /has no signature/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
