import { invoke, isTauri } from "@tauri-apps/api/core";
import type { BurnBatch } from "#lib/bindings/BurnBatch.js";
import type { HostSnapshot } from "#lib/bindings/HostSnapshot.js";
import type { InspectorBatch } from "#lib/bindings/InspectorBatch.js";
import type { Preset } from "#lib/bindings/Preset.js";
import type { PresetInfo } from "#lib/bindings/PresetInfo.js";
import type { SettingRow } from "#lib/bindings/SettingRow.js";
import type { Speed } from "#lib/bindings/Speed.js";

/** True inside the Tauri window; false in the browser preview (`npm run dev`). */
export const desktop = isTauri();

/** The browser preview has no emulator or TCP server: it says so instead of pretending. */
export const previewSnapshot: HostSnapshot = {
  link: {
    address: null,
    error: "Browser preview: no TCP server. Run npm run sim:dev.",
    client: null,
    refused: 0,
  },
  machine: {
    profile: "Browser preview (no emulator)",
    state: "Offline",
    machinePosition: [0, 0, 0],
    workPosition: [0, 0, 0],
    clockMs: 0,
    speed: "x1",
    laserPower: 0,
    airAssist: false,
    bed: { origin: [0, 0], size: [400, 400] },
    scenarioPending: 0,
  },
};

/** No emulator in the browser preview, so no traffic. */
export const previewInspector: InspectorBatch = {
  events: [],
  next: 0,
  missed: false,
  samples: [],
  rxCapacity: 128,
  plannerCapacity: 15,
};

/** No emulator in the browser preview, so nothing has burned. */
export const previewBurns: BurnBatch = {
  generation: 0,
  segments: [],
  next: 0,
  total: 0,
  truncated: false,
};

const previewOnly = () => Promise.reject(new Error("Not available in the browser preview"));

export async function snapshot(): Promise<HostSnapshot> {
  return desktop ? invoke<HostSnapshot>("snapshot") : previewSnapshot;
}

/** Wire traffic from sequence number `since` on, and the buffer history. */
export async function inspector(since: number): Promise<InspectorBatch> {
  return desktop ? invoke<InspectorBatch>("inspector", { since }) : previewInspector;
}

/** Saves the inspector log to a new file in Downloads; resolves to its path. */
export async function exportLog(text: string): Promise<string> {
  return desktop ? invoke<string>("export_log", { text }) : previewOnly();
}

/** Burn view segments from index `since` on. */
export async function burns(since: number): Promise<BurnBatch> {
  return desktop ? invoke<BurnBatch>("burns", { since }) : previewBurns;
}

/** Empties the bed: the burn view starts again from nothing. */
export async function clearBurns(): Promise<void> {
  return desktop ? invoke("clear_burns") : previewOnly();
}

/** Saves a PNG of the burn view to a new file in Downloads; resolves to its path. */
export async function exportPng(png: Uint8Array): Promise<string> {
  return desktop ? invoke<string>("export_png", { png: Array.from(png) }) : previewOnly();
}

export async function settings(): Promise<SettingRow[]> {
  return desktop ? invoke<SettingRow[]>("settings") : [];
}

export async function presets(): Promise<PresetInfo[]> {
  return desktop ? invoke<PresetInfo[]>("presets") : [];
}

/** Rebinds the fake USB server; resolves to the new address. */
export async function listen(port: number): Promise<string> {
  return desktop ? invoke<string>("listen", { port }) : previewOnly();
}

export async function setSpeed(speed: Speed): Promise<void> {
  return desktop ? invoke("set_speed", { speed }) : previewOnly();
}

/** Power-cycles the controller; a connected host sees the banner. */
export async function resetMachine(): Promise<void> {
  return desktop ? invoke("reset_machine") : previewOnly();
}

/** Writes a `$` setting and power-cycles the controller. */
export async function setSetting(id: number, value: number): Promise<void> {
  return desktop ? invoke("set_setting", { id, value }) : previewOnly();
}

export async function loadPreset(preset: Preset): Promise<void> {
  return desktop ? invoke("load_preset", { preset }) : previewOnly();
}

/** Injects one fault in bf-sim's text form, e.g. `alarm 1` or `stall 2s`. */
export async function injectFault(fault: string): Promise<void> {
  return desktop ? invoke("inject_fault", { fault }) : previewOnly();
}

/** Starts a scenario script; its lines count from now. */
export async function runScenario(script: string): Promise<void> {
  return desktop ? invoke("run_scenario", { script }) : previewOnly();
}

/** Ends every fault in force and any running scenario. */
export async function clearFaults(): Promise<void> {
  return desktop ? invoke("clear_faults") : previewOnly();
}
