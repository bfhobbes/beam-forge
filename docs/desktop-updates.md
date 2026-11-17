# Desktop updates

Beam Forge checks for updates shortly after launch and every four hours. A newer release opens a prompt with plain-text release notes. You can defer it, download while working, and then explicitly install and restart. A download becomes installable only after Tauri verifies its signature. Installation is refused while the project has unsaved changes or something is busy (later: a running laser job), and edits are locked while it installs. Background check failures stay quiet; open **Updates** to see them and retry. Downloaded packages are held for the current session only.

Release builds (`npm run release:build`, see [Releases](#releases)) check this repository's GitHub releases. Other builds have no update endpoint or signing key, so **Updates** reports a check error.

## Try the local service (Windows)

Run from the repository root:

```powershell
npm run updates:setup
npm run updates:serve
```

In another terminal, build a baseline and a newer signed release:

```powershell
npm run updates:build -- 0.1.0
# Install the 0.1.0 installer from target/release/bundle/nsis.
npm run updates:build -- 0.1.1
npm run updates:publish -- 0.1.1 windows-x86_64 "target/release/bundle/nsis/Beam Forge Update Lab_0.1.1_x64-setup.exe" "Signed updates work."
```

Launch the installed **Beam Forge Update Lab** 0.1.0. It should offer 0.1.1. Choose **Download update**, then **Install and restart**; the version on the home screen comes from the native app, so the result is visible. The demo has its own application identifier and data directory, separate from Beam Forge. A loose executable is not an installed app, so install the baseline first, and don't mix MSI and NSIS installs.

`npm run updates:dev` points a development window at the same service, for debugging the prompt and download. Test installation with the installed release build.

`updates:setup` creates an unencrypted development signing key, its public key, and a Tauri config override in the ignored `.local/` directory, and keeps an existing key. Keep `.local/` private; the keys are for local testing only. The override enables plain HTTP for the loopback service only, and turns on signed updater artifacts.

The service binds `127.0.0.1:17845`. `GET /updates/:target/:arch/:current_version` returns 204 when there is no newer release for that platform, or Tauri's dynamic update JSON otherwise. `GET /artifacts/:file` streams package bytes. Publishing copies an artifact into `.local/update-server` and atomically replaces the manifest; there is no network write API. Only one release per target/architecture is advertised.

## Code layout

- `src/lib/desktop/updates.ts`: UI-independent update lifecycle with an injectable provider and install-lock callback.
- `src/lib/desktop/UpdateManager.svelte`: startup and periodic checks, prompt, progress, retries, and the manual entry point.
- `src/lib/desktop/lifecycle.svelte.ts`: the shared dirty/busy/install-lock policy also used by the close guard.
- `src-tauri/src/lib.rs` registers the updater and process plugins; `capabilities/default.json` grants only check, download, install and restart.
- `scripts/update-server.mjs` and `scripts/updates.mjs`: the local service and the signing/publishing workflow.

## Releases

Releases are published from the public `bfhobbes/beam-forge` repository:

1. Run `npm run release:version -- 0.1.1`. It sets the app and simulator version in `package.json`, `tauri.conf.json` and `Cargo.toml`. Commit the change through a PR and merge it.
2. Tag the merge on `main` and push the tag: `git tag v0.1.1 && git push origin v0.1.1`.
3. `.github/workflows/release.yml` checks the tag matches the version, then runs `npm run release:build` on Windows (`windows-latest`) and Linux (`ubuntu-22.04`, so the AppImage also runs on older glibc). That build bundles the simulator and adds `src-tauri/tauri.release.conf.json`, which turns on signed updater artifacts and sets the public key and endpoint. The Linux build also checks the simulator sidecar launches from the AppImage and the `.deb` (`scripts/check-linux-sidecar.sh`). A publish job then verifies every package's signature against the public key, publishes the NSIS and MSI installers, the AppImage and the `.deb` as one GitHub release with generated notes, and `scripts/release-manifest.mjs` writes `latest.json` to the `updates` branch.

The endpoint is `https://raw.githubusercontent.com/bfhobbes/beam-forge/updates/latest.json`. The manifest links each package by its public download URL (`browser_download_url`), so the updater needs no credentials and no GitHub API rate limit applies. Each install updates through its own package type: `windows-x86_64-nsis`, `windows-x86_64-msi`, `linux-x86_64-appimage` and `linux-x86_64-deb`, with `windows-x86_64` (NSIS) and `linux-x86_64` (AppImage) as fallbacks. Raw files can be cached for about five minutes, so a new release may take that long to appear.

Any other run of the release workflow is a dry run: a PR that changes the release pipeline (the workflow, `release-manifest.mjs`, `stage-simulator.mjs`, `check-linux-sidecar.sh` or the Tauri configs), or **Run workflow** on the Actions tab. It builds and signs both platforms and uploads the `latest.json` it would publish (with placeholder URLs) as the `latest-json-dry-run` artifact, but creates no release and leaves the `updates` branch alone.

The updater sends no credentials: the manifest and packages are public, and packages are trusted by their updater signatures alone.

The repository secrets are `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The signing key and its password are kept in the ignored `.local/release/`; back them up outside this machine. Losing the key means installed copies can't verify any later update. Changing the public key needs a planned rotation through an already-trusted release. Keep the identifier `dev.bfhobbes.beamforge` stable, and never use `dangerousInsecureTransportProtocol` in the release config.

## Linux

Each release ships `Beam.Forge_<version>_amd64.AppImage` and `Beam.Forge_<version>_amd64.deb` (x86_64). Both update in place through the same flow as Windows (the public manifest and download URLs):

- **AppImage** (recommended): `chmod +x` it and run it from a folder you own, such as `~/Applications`. An update rewrites the AppImage file itself, so it needs no password. Older distributions may need `libfuse2` to mount AppImages.
- **`.deb`** (Debian, Ubuntu, Mint): `sudo apt install ./Beam.Forge_<version>_amd64.deb` pulls in WebKitGTK and the other dependencies. An update installs the newer `.deb` with `dpkg -i`, so it asks for an administrator password (through `pkexec`).

Use one or the other, not both. The built-in simulator is bundled next to the app in both (`usr/bin/bf-simulator`).

To use the laser over USB, your user needs access to the serial port (`/dev/ttyUSB0` or `/dev/ttyACM0`), which is usually owned by the `dialout` group (`uucp` on Arch): run `sudo usermod -aG dialout $USER`, then log out and back in. Without it, connecting fails with "Permission denied" and the app shows this hint. If the port doesn't appear at all, check `dmesg` after plugging it in; on some Ubuntu releases `brltty` claims CH340 USB-serial adapters, and removing it (`sudo apt remove brltty`) frees the port.

Developing on Linux needs Tauri's system packages, as in CI: `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libudev-dev build-essential file xdg-utils`.

## Signatures

Updater signatures authenticate packages. Windows code signing (to avoid SmartScreen warnings) is separate and not configured. The client never forces updates, downgrades, or rolls back.

## Validation

`npm run verify` covers the lifecycle and HTTP service tests. `npm run test:rust` checks that the plugins initialize with the production config. With the local service running and a newer signed package published:

```powershell
cargo test -p beam-forge local_signed_update -- --ignored
```

This downloads through the real Rust updater, verifies the package, then confirms that a one-byte change fails verification. It does not launch an installer.
