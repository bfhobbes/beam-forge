# Beam Forge

Laser cutter editor and controller with a modern UX. Personal tool. Target machine: Elegoo Phecda 20W diode (GRBL over USB serial, air assist). Windows and Linux are both first-class (the laser laptop runs Linux); CI and releases cover both. Keep code cross-platform. Roadmap: GitHub issue #1; each issue lists its dependencies.

## Architecture

- **Frontend** (`src/`): SvelteKit 3 static SPA in Tauri, Svelte 5 runes, Tailwind 4, shadcn-svelte (Nova style). Owns the design document, selection, undo/redo and canvas rendering.
- **Rust core** (`crates/bf-core`): product logic with no Tauri dependency, so it is unit-testable without a window or machine. File import, fonts and text outlines (`text/`, shaped with `rustybuzz`; text objects store their outline so files cut the same without the font), geometry, toolpath planning, GCode generation (behind a firmware `Dialect` trait) and the serial machine driver go here.
- **Simulator core** (`crates/bf-sim`): deterministic GRBL 1.1 emulator (bytes in, replies out, simulated clock). Never depend on `bf-core` from it: it is an independent implementation so the app and simulator cannot share a bug. Match real GRBL behaviour (check against gnea/grbl source); tests that need a machine should drive `bf-sim`, not hand-written mocks. To exercise failure paths, inject a `Fault` (`Simulator::inject`) or run a `Scenario` ("after line N: fault", `src/fault.rs`); in-process tests reach the emulator via `bf-core`'s `SimLink::sim()`.
- **Simulator app** (`simulator/`): separate Tauri app ("Beam Forge Simulator") hosting `bf-sim` behind a TCP "fake USB" server on `127.0.0.1:23232`. Same stack as the main app; npm workspace package, Cargo member `simulator/src-tauri`. The emulator clock is in `machine.rs`, the TCP link in `link.rs`. Its bindings go to `simulator/src/lib/bindings` (`export_to`). Root `vp` config ignores `simulator/`; it has its own config and checks.
- **Built-in simulator**: `desktop:build` bundles the simulator as a Tauri sidecar (`bf-simulator(.exe)` next to the `beam-forge` executable; `usr/bin/` in the AppImage and `.deb`). `sim:stage` builds it in release and copies it to the gitignored `src-tauri/binaries/bf-simulator-<target-triple>[.exe]`; `src-tauri/tauri.bundle.conf.json` adds `externalBin` only for packaging, so `cargo test`, clippy and `desktop:dev` don't need it. The "Built-in simulator" port (`PortAddress::Launch`, `src-tauri/src/machine/launch.rs`) runs it with `--launched-by-app`: it listens on an OS-assigned port, prints `BF_SIM_LISTENING <address>` on stdout and exits when stdin closes. The connection's transport owns the child, so disconnecting or quitting closes it. In `desktop:dev`, run `npm run sim:dev` first and keep it running: it builds `target/debug/bf-simulator.exe` and serves the launched simulator's window on :1421.
- **Desktop shell** (`src-tauri`): thin Tauri commands over `bf-core`, plugins, capabilities. Privileged work (files, serial port) stays in Rust behind narrow typed commands.
- **Contracts**: Rust types with `#[derive(TS)] #[ts(export)]` generate `src/lib/bindings/*.ts` via `npm run bindings`. Commit the generated files; CI fails on drift. Call commands through small typed adapters (see `src/lib/desktop/platform.ts`).
- **Lifecycle** (`src/lib/desktop/lifecycle.svelte.ts`): one dirty/busy/install-lock policy shared by window close and updates. Mark unsaved work with `lifecycle.dirty` and uninterruptible work (saves, laser jobs) with `lifecycle.beginBusy(reason)`.
- **Browser preview** (`npm run dev`): no Rust side. `desktop` from `platform.ts` is false; adapters must return clearly-labelled preview values, never pretend to reach the machine.

## Commands

```sh
npm ci
npm run desktop:dev      # native app with hot reload
npm run dev              # browser preview at http://127.0.0.1:1420
npm run verify           # svelte-check, Oxlint + ESLint, format check, tests, web build
npm run smoke:web        # load build/ in headless Edge/Chrome; fails on errors or no editor
npm run test:rust        # cargo test --workspace (also regenerates bindings)
npm run lint:rust        # rustfmt + clippy -D warnings
npm run bindings         # regenerate src/lib/bindings from Rust
npm run desktop:build    # installers in target/release/bundle
npm run release:version -- <x.y.z>  # bump versions; then tag v<x.y.z> on main to publish a release
npm run sim:dev          # simulator app with hot reload
npm run sim:verify       # simulator's svelte-check, lint, format, tests, web build
npm run sim:build        # simulator installers
npm run sim:stage        # release simulator staged as the app's sidecar (desktop:build runs it)
cargo run -p bf-sim -- check <file.gcode> [--profile <json>] [--json]  # headless job check
```

Run `npm run verify` and `npm run smoke:web` for frontend changes (`sim:verify` for `simulator/`), plus `test:rust` and `lint:rust` for Rust changes.

## Conventions and gotchas

- Imports use `#lib/...` with explicit `.js` paths (e.g. `#lib/components/ui/button/index.js`); `.js` resolves to the TS source.
- The root is an npm workspace, so root `vp dev`/`build`/`preview` need `-C .` (the scripts already pass it).
- Keep plugins eagerly instantiated in `vite.config.ts`. Vite+'s `lazyPlugins` breaks `svelte-kit sync`.
- Oxlint (`vp lint`) does not read Svelte markup, so ESLint lints `.svelte` files (Svelte rules + `@shadcn/lint`). Oxlint's `typeCheck` is off because it cannot resolve `.svelte` exports; svelte-check owns types.
- TypeScript is held at 6.x: Kit 3, svelte-check and typescript-eslint don't support TS 7 yet.
- Tauri stays on 2.x (3 is alpha).
- `src-tauri/build.rs` embeds a Common Controls v6 manifest; without it Rust IPC tests fail on Windows with `STATUS_ENTRYPOINT_NOT_FOUND`.
- The main window needs `core:window:allow-destroy` for `onCloseRequested` to close. `ipc_tests` load the real capabilities to catch this.
- Use the Svelte MCP server (`.mcp.json`) for Svelte docs and `svelte-autofixer` on changed components when available.
- Linux needs Tauri's system packages (WebKitGTK 4.1, `libudev-dev`, ...; list in `docs/desktop-updates.md#linux`) and serial access via the `dialout` group. Linux CI runs `scripts/check-linux-sidecar.sh` under `xvfb-run` instead of the Windows-only `test-native-close.ps1`.
- Updates and releases: see `docs/desktop-updates.md`. Pushing a `v*` tag publishes a signed GitHub release and points the updater at it. Never commit `.local/` (signing keys).
