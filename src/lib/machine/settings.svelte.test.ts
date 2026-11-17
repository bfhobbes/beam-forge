import { describe, expect, it, vi } from "vite-plus/test";
import type { ConnectionState } from "#lib/bindings/ConnectionState.js";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { MachineSetting } from "#lib/bindings/MachineSetting.js";
import type { PortInfo } from "#lib/bindings/PortInfo.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";
import { previewSettings, type SettingsAdapter } from "./settings-adapter.js";
import { DEFAULT_BED, MachineSettingsStore } from "./settings.svelte.js";

const port: PortInfo = {
  key: "serial:COM3",
  address: { kind: "serial", path: "COM3" },
  label: "COM3",
  description: null,
  vid: null,
  pid: null,
};
const connected: ConnectionState = {
  status: "connected",
  port,
  machine: { firmware: "grbl", name: "Grbl", version: "1.1h", banner: "" },
};
const disconnected: ConnectionState = { status: "disconnected", reason: null };

function profile(changes: Partial<MachineProfile> = {}): MachineProfile {
  return {
    id: "serial-com3",
    name: "Grbl on COM3",
    firmware: "grbl",
    portKey: port.key,
    bed: { width: 400, height: 400 },
    bedFromMachine: true,
    origin: "frontLeft",
    maxSpindle: 1000,
    minSpindle: 0,
    laserMode: false,
    maxRate: { x: 6000, y: 6000, z: 500 },
    acceleration: { x: 500, y: 500, z: 10 },
    homing: false,
    airAssist: "M8",
    userOrigin: null,
    framePower: 0,
    ...changes,
  };
}

function laser(value: string): MachineSetting {
  return {
    number: 32,
    value,
    name: "Laser-mode enable",
    unit: null,
    description: "",
    kind: { kind: "boolean" },
  };
}

function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    values,
  };
}

/** A machine whose `$32` starts at 0, like GRBL's factory settings. */
function fakeMachine(saved: MachineProfile[] = []) {
  let laserMode = "0";
  let emit: (state: ConnectionState) => void = () => {};
  const snapshot = () => ({
    settings: [laser(laserMode)],
    profile: profile({ laserMode: laserMode === "1" }),
  });
  const adapter = {
    read: vi.fn(async () => snapshot()),
    write: vi.fn(async (number: number, value: string) => {
      if (value === "2") throw new Error("$32 (Laser-mode enable) must be 0 or 1.");
      if (number === 32) laserMode = value;
      return snapshot();
    }),
    profiles: vi.fn(async () => saved),
    saveProfile: vi.fn(async (changed: MachineProfile) => changed),
  } satisfies SettingsAdapter;
  const machine = {
    state: vi.fn(async (): Promise<ConnectionState> => disconnected),
    onState: vi.fn(async (handler: (state: ConnectionState) => void) => {
      emit = handler;
      return () => {};
    }),
  };
  return { adapter, machine, emit: (state: ConnectionState) => emit(state) };
}

/** Lets the read started by a state event finish. */
const settle = () => new Promise((resolve) => setTimeout(resolve));

describe("machine settings", () => {
  it("reads the settings and adopts the profile when a machine connects", async () => {
    const { adapter, machine, emit } = fakeMachine();
    const storage = memory();
    const store = new MachineSettingsStore(adapter, machine, storage);
    await store.start();
    expect(store.profile).toBeNull();
    expect(store.bed).toEqual(DEFAULT_BED);

    emit(connected);
    emit(connected);
    await settle();

    expect(adapter.read).toHaveBeenCalledOnce();
    expect(store.settings).toHaveLength(1);
    expect(store.profile?.id).toBe("serial-com3");
    expect(store.bed).toEqual({ width: 400, height: 400 });
    expect(storage.values.get("beam-forge.machine-profile")).toBe("serial-com3");
  });

  it("warns while laser mode is off, until $32 is written", async () => {
    const { adapter, machine, emit } = fakeMachine();
    const store = new MachineSettingsStore(adapter, machine, memory());
    await store.start();
    emit(connected);
    await settle();
    expect(store.laserModeOff).toBe(true);

    expect(await store.write(32, "1")).toBe(true);

    expect(adapter.write).toHaveBeenCalledWith(32, "1");
    expect(store.settings[0].value).toBe("1");
    expect(store.laserModeOff).toBe(false);
    expect(store.profile?.laserMode).toBe(true);
  });

  it("keeps the table and reports why a write was refused", async () => {
    const { adapter, machine, emit } = fakeMachine();
    const store = new MachineSettingsStore(adapter, machine, memory());
    await store.start();
    emit(connected);
    await settle();

    expect(await store.write(32, "2")).toBe(false);

    expect(store.error).toBe("$32 (Laser-mode enable) must be 0 or 1.");
    expect(store.settings[0].value).toBe("0");
    expect(store.writing).toBeNull();
  });

  it("clears the table on disconnect but keeps the profile", async () => {
    const { adapter, machine, emit } = fakeMachine();
    const store = new MachineSettingsStore(adapter, machine, memory());
    await store.start();
    emit(connected);
    await settle();

    emit(disconnected);

    expect(store.settings).toEqual([]);
    expect(store.profile?.id).toBe("serial-com3");
    expect(store.laserModeOff).toBe(true);
  });

  it("starts from the remembered profile while disconnected", async () => {
    const other = profile({ id: "tcp-sim", name: "A simulator" });
    const { adapter, machine } = fakeMachine([other, profile({ name: "Phecda" })]);
    const store = new MachineSettingsStore(
      adapter,
      machine,
      memory({ "beam-forge.machine-profile": "serial-com3" }),
    );
    await store.start();

    expect(store.profile?.name).toBe("Phecda");
    expect(adapter.read).not.toHaveBeenCalled();
    store.selectProfile("tcp-sim");
    expect(store.profile?.name).toBe("A simulator");
  });

  it("saves profile edits through the adapter", async () => {
    const { adapter, machine, emit } = fakeMachine();
    const store = new MachineSettingsStore(adapter, machine, memory());
    await store.start();
    emit(connected);
    await settle();

    expect(await store.saveProfile({ name: "Phecda", origin: "rearLeft" })).toBe(true);

    expect(adapter.saveProfile).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "serial-com3",
        name: "Phecda",
        origin: "rearLeft",
      }),
    );
    expect(store.profiles.map((p) => p.name)).toEqual(["Phecda"]);
  });

  it("says why in the browser preview", async () => {
    const { machine } = fakeMachine();
    const store = new MachineSettingsStore(previewSettings, machine, memory());
    await store.start();
    await store.read();
    expect(store.error).toBe(PREVIEW_UNAVAILABLE);
    expect(store.profiles).toEqual([]);
  });
});
