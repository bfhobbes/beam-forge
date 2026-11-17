// Writes the updater manifest (latest.json) for a GitHub release built on
// every platform. Packages are linked through the GitHub API asset URLs,
// because the repository is private: the app downloads them with its
// read-only token. Each package's signature is checked against the release
// public key first, so a manifest never points the updater at a package it
// would reject.
//
//   node scripts/release-manifest.mjs <tag> <bundles-dir> <output> [--dry-run]
//
// <bundles-dir> holds the bundles from every platform's build, in any layout
// (each package next to its `.sig`). --dry-run doesn't look for a GitHub
// release: it links each package to a placeholder URL, to check the build.
import { createHash, createPublicKey, verify } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

/**
 * The packages a release ships, by updater target. Tauri tries
 * `<os>-<arch>-<installer>` first, so each install keeps updating through its
 * own package type; the plain `<os>-<arch>` aliases are for anything else.
 */
export const packages = (version) => ({
  "windows-x86_64-nsis": `_${version}_x64-setup.exe`,
  "windows-x86_64-msi": `_${version}_x64_en-US.msi`,
  "linux-x86_64-appimage": `_${version}_amd64.AppImage`,
  "linux-x86_64-deb": `_${version}_amd64.deb`,
});
const aliases = {
  "windows-x86_64": "windows-x86_64-nsis",
  "linux-x86_64": "linux-x86_64-appimage",
};

/**
 * Whether `name` is the app's package with `suffix`. GitHub renames assets
 * (spaces become dots), and the simulator's packages share the suffix.
 */
const isPackage = (name, suffix) =>
  name.endsWith(suffix) && /^Beam[ .]Forge$/.test(name.slice(0, -suffix.length));

/** Every file under `dir`, recursively. */
function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? files(join(dir, entry.name)) : [join(dir, entry.name)],
  );
}

/** The single minisign key or signature line in a Tauri base64 key/signature file. */
function minisignLines(base64) {
  return Buffer.from(base64.trim(), "base64").toString("utf8").split(/\r?\n/);
}

/**
 * Checks a Tauri updater signature (a base64-wrapped minisign signature) of
 * `bytes` against a Tauri public key (a base64-wrapped minisign public key).
 * Throws with the reason when it doesn't verify.
 */
export function verifySignature(bytes, signature, publicKey) {
  const keyLine = Buffer.from(minisignLines(publicKey)[1] ?? "", "base64");
  if (keyLine.length !== 42 || keyLine.toString("latin1", 0, 2) !== "Ed")
    throw new Error("The public key isn't a minisign Ed25519 key.");
  const key = createPublicKey({
    key: { kty: "OKP", crv: "Ed25519", x: keyLine.subarray(10).toString("base64url") },
    format: "jwk",
  });
  const [, sigText, trusted, globalText] = minisignLines(signature);
  const sig = Buffer.from(sigText ?? "", "base64");
  if (sig.length !== 74) throw new Error("The signature isn't a minisign signature.");
  const algorithm = sig.toString("latin1", 0, 2);
  if (!sig.subarray(2, 10).equals(keyLine.subarray(2, 10)))
    throw new Error("The signature was made with a different key.");
  const message =
    algorithm === "ED" ? createHash("blake2b512").update(bytes).digest() : Buffer.from(bytes);
  if (!verify(null, message, key, sig.subarray(10)))
    throw new Error("The signature doesn't match the package.");
  const comment = (trusted ?? "").replace(/^trusted comment: /, "");
  const global = Buffer.from(globalText ?? "", "base64");
  if (!verify(null, Buffer.concat([sig.subarray(10), Buffer.from(comment)]), key, global))
    throw new Error("The signature's trusted comment has been altered.");
}

/**
 * Finds each platform's package and signature under `dir` and verifies the
 * signature. Returns `{ [platform]: { name, signature } }`.
 */
export function findPackages(dir, version, publicKey) {
  const all = files(dir);
  const found = {};
  for (const [platform, suffix] of Object.entries(packages(version))) {
    const matches = all.filter((file) => isPackage(basename(file), suffix));
    if (matches.length !== 1)
      throw new Error(
        `Expected one package ending in ${suffix} under ${dir}, found ${matches.length}.`,
      );
    const [file] = matches;
    let signature;
    try {
      signature = readFileSync(`${file}.sig`, "utf8").trim();
    } catch {
      throw new Error(`${basename(file)} has no signature (${basename(file)}.sig).`);
    }
    try {
      verifySignature(readFileSync(file), signature, publicKey);
    } catch (error) {
      throw new Error(`${basename(file)}: ${error.message}`);
    }
    found[platform] = { name: basename(file), signature };
  }
  return found;
}

/**
 * The manifest for `release` (GitHub's release JSON: `body`, `published_at`
 * and `assets` of `{ name, url }`).
 */
export function buildManifest({ version, release, found }) {
  const platforms = {};
  for (const [platform, suffix] of Object.entries(packages(version))) {
    const asset = release.assets.find((a) => isPackage(a.name, suffix));
    if (!asset) throw new Error(`The release has no asset ending in ${suffix}.`);
    platforms[platform] = { signature: found[platform].signature, url: asset.url };
  }
  for (const [alias, platform] of Object.entries(aliases)) platforms[alias] = platforms[platform];
  return {
    version,
    notes: release.body?.trim() || `Beam Forge ${version}`,
    pub_date: release.published_at ?? new Date().toISOString(),
    platforms,
  };
}

function main(args) {
  const dryRun = args.includes("--dry-run");
  const [tag, bundles, output] = args.filter((arg) => arg !== "--dry-run");
  if (!tag || !bundles || !output)
    throw new Error("Usage: release-manifest.mjs <tag> <bundles-dir> <output> [--dry-run]");
  const version = tag.replace(/^v/, "");
  const publicKey = JSON.parse(readFileSync(resolve("src-tauri/tauri.release.conf.json"), "utf8"))
    .plugins.updater.pubkey;
  const found = findPackages(resolve(bundles), version, publicKey);
  const release = dryRun
    ? {
        body: `Dry run of Beam Forge ${version}`,
        assets: Object.values(found).map(({ name }) => ({
          name,
          url: `https://example.invalid/dry-run/${encodeURIComponent(name)}`,
        })),
      }
    : JSON.parse(
        execFileSync("gh", ["api", `repos/{owner}/{repo}/releases/tags/${tag}`], {
          encoding: "utf8",
        }),
      );
  writeFileSync(output, `${JSON.stringify(buildManifest({ version, release, found }), null, 2)}\n`);
  console.log(
    `Wrote the ${tag} update manifest${dryRun ? " (dry run)" : ""} to ${output}: ` +
      Object.keys(found).join(", "),
  );
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) main(process.argv.slice(2));
