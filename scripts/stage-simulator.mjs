// Builds the simulator in release (embedded frontend, no installer) and stages
// it as Beam Forge's sidecar, src-tauri/binaries/bf-simulator-<target-triple>,
// for `tauri build --config src-tauri/tauri.bundle.conf.json`.
import { copyFileSync, mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { execFileSync, spawnSync } from "node:child_process";

const cli = resolve("node_modules/@tauri-apps/cli/tauri.js");
const build = spawnSync(process.execPath, [cli, "build", "--no-bundle"], {
  cwd: resolve("simulator"),
  stdio: "inherit",
});
if (build.error || build.status !== 0)
  throw new Error(`Simulator build failed (${build.status ?? build.error?.message}).`);

const triple = /^host: (\S+)$/m.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))?.[1];
if (!triple) throw new Error("Couldn't read the target triple from `rustc -vV`.");
const extension = process.platform === "win32" ? ".exe" : "";
const staged = resolve("src-tauri/binaries", `bf-simulator-${triple}${extension}`);
mkdirSync(resolve("src-tauri/binaries"), { recursive: true });
copyFileSync(resolve("target/release", `bf-simulator${extension}`), staged);
console.log(`Staged the simulator sidecar at ${staged}`);
