import { mkdir, readFile, writeFile, access, copyFile, rename } from "node:fs/promises";
import { resolve, basename } from "node:path";
import { spawnSync } from "node:child_process";
import semver from "semver";

const root = resolve(".local");
const key = resolve(root, "keys/updater.key");
const config = resolve(root, "tauri.updater.json");
const cli = resolve("node_modules/@tauri-apps/cli/tauri.js");
function run(args, options = {}) {
  const result = spawnSync(process.execPath, [cli, ...args], { stdio: "inherit", ...options });
  if (result.error || result.status !== 0)
    throw new Error(`Tauri command failed (${result.status ?? result.error?.message}).`);
  return result;
}
async function setup() {
  await mkdir(resolve(root, "keys"), { recursive: true });
  try {
    await access(key);
  } catch {
    run(["signer", "generate", "--ci", "--password", "", "--write-keys", key], { stdio: "pipe" });
  }
  const pubkey = (await readFile(`${key}.pub`, "utf8")).trim();
  await writeFile(
    config,
    JSON.stringify(
      {
        productName: "Beam Forge Update Lab",
        identifier: "dev.bfhobbes.beamforge.updaterdemo",
        bundle: { createUpdaterArtifacts: true },
        plugins: {
          updater: {
            pubkey,
            endpoints: ["http://127.0.0.1:17845/updates/{{target}}/{{arch}}/{{current_version}}"],
            dangerousInsecureTransportProtocol: true,
            windows: { installMode: "passive" },
          },
        },
      },
      null,
      2,
    ),
  );
  console.log("Local demo configuration ready. Private signing key stays in ignored .local/keys.");
}
const [command, ...args] = process.argv.slice(2);
if (command === "setup") await setup();
else if (command === "dev" || command === "build") {
  await setup();
  const version = args[0];
  if (version && !semver.valid(version)) throw new Error("Expected a semantic version, e.g. 0.2.1");
  // Builds bundle the simulator sidecar, as `npm run desktop:build` does.
  const configArgs = ["--config", config];
  if (command === "build") {
    const staged = spawnSync(process.execPath, [resolve("scripts/stage-simulator.mjs")], {
      stdio: "inherit",
    });
    if (staged.status !== 0) throw new Error("Staging the simulator failed.");
    configArgs.push("--config", resolve("src-tauri/tauri.bundle.conf.json"));
  }
  if (version) configArgs.push("--config", JSON.stringify({ version }));
  run([command, ...configArgs], {
    env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "" },
  });
} else if (command === "publish") {
  const [version, target, artifact, notes = "A new Beam Forge release is ready."] = args;
  if (
    !semver.valid(version) ||
    !/^(windows|darwin|linux)-(x86_64|aarch64|i686|armv7)$/.test(target ?? "") ||
    !artifact
  )
    throw new Error("Usage: updates:publish -- VERSION TARGET-ARCH ARTIFACT [NOTES]");
  const signature = (await readFile(`${artifact}.sig`, "utf8")).trim();
  const directory = resolve(root, "update-server");
  await mkdir(resolve(directory, "artifacts"), { recursive: true });
  const file = `${version}-${target}-${basename(artifact)}`;
  await copyFile(artifact, resolve(directory, "artifacts", file));
  const manifest = resolve(directory, "releases.json");
  let releases = {};
  try {
    releases = JSON.parse(await readFile(manifest, "utf8"));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  releases[target] = { version, file, signature, notes, pub_date: new Date().toISOString() };
  await writeFile(`${manifest}.tmp`, JSON.stringify(releases, null, 2));
  await rename(`${manifest}.tmp`, manifest);
  console.log(`Published ${version} for ${target}.`);
} else throw new Error("Expected setup, dev, build or publish.");
