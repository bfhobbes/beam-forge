// Sets the app and simulator version everywhere it is recorded.
//   npm run release:version -- 0.1.1
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) throw new Error("Usage: release:version <x.y.z>");

const replace = (file, pattern) => {
  const text = readFileSync(file, "utf8");
  if (!pattern.test(text)) throw new Error(`No version found in ${file}.`);
  writeFileSync(
    file,
    text.replace(pattern, (_, before) => `${before}${version}"`),
  );
};
for (const file of [
  "package.json",
  "simulator/package.json",
  "src-tauri/tauri.conf.json",
  "simulator/src-tauri/tauri.conf.json",
])
  replace(file, /^(\s*"version": ")[^"]+"/m);
replace("Cargo.toml", /^(version = ")[^"]+"/m);

// Edit the lockfile's own version fields; `npm install --package-lock-only`
// would also re-resolve unrelated packages.
const lock = JSON.parse(readFileSync("package-lock.json", "utf8"));
lock.version = version;
for (const key of ["", "simulator"]) lock.packages[key].version = version;
writeFileSync(
  "package-lock.json",
  `${JSON.stringify(lock, null, 2)}
`,
);
execFileSync("cargo", ["update", "--workspace"], { stdio: "inherit" });
console.log(`Set the version to ${version}. Commit, merge, then push the tag v${version}.`);
