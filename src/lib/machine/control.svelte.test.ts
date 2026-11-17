import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { MachineSetting } from "#lib/bindings/MachineSetting.js";
import type { MachineState } from "#lib/bindings/MachineState.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { TestFire } from "#lib/bindings/TestFire.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";
import { previewControl, type ControlAdapter } from "./control-adapter.js";
import {
  arrowDirection,
  FIRING_REASON,
  HOLD_DELAY_MS,
  JOG_PAD,
  MachineControl,
  RENEW_MS,
  TEST_FIRE_MAX_MS,
  TEST_FIRE_MAX_POWER,
} from "./control.svelte.js";

function status(state: MachineState, flood = false): MachineStatus {
  return {
    state,
    machinePosition: { x: 0, y: 0, z: 0 },
    workPosition: { x: 0, y: 0, z: 0 },
    workOffset: null,
    feed: 0,
    spindle: 0,
    overrides: null,
    buffer: null,
    pins: {
      limitX: false,
      limitY: false,
      limitZ: false,
      probe: false,
      door: false,
      hold: false,
      softReset: false,
      cycleStart: false,
    },
    accessories: { spindleCw: false, spindleCcw: false, flood, mist: false },
    line: null,
  };
}

function setting(number: number, value: string): MachineSetting {
  return { number, value, name: null, unit: null, description: null, kind: { kind: "boolean" } };
}

function fakeAdapter() {
  return {
    jog: vi.fn<ControlAdapter["jog"]>(async () => {}),
    jogHold: vi.fn<ControlAdapter["jogHold"]>(async () => {}),
    jogStop: vi.fn<ControlAdapter["jogStop"]>(async () => {}),
    command: vi.fn<ControlAdapter["command"]>(async () => {}),
    testFire: vi.fn<ControlAdapter["testFire"]>(async (fire: TestFire) => ({
      power: Math.min(fire.power, TEST_FIRE_MAX_POWER),
      durationMs: Math.min(fire.durationMs, TEST_FIRE_MAX_MS),
    })),
  } satisfies ControlAdapter;
}

function setup(
  options: { state?: MachineState; settings?: MachineSetting[]; homing?: boolean } = {},
) {
  const adapter = fakeAdapter();
  const machine = $state({
    connected: true,
    status: status(options.state ?? { name: "idle" }) as MachineStatus | null,
  });
  const job = $state({ active: false });
  const profile = { homing: options.homing ?? false } as MachineProfile;
  const settings = $state({ settings: options.settings ?? [], profile });
  const lifecycle = new Lifecycle();
  const control = new MachineControl(adapter, machine, job, settings, lifecycle);
  return { adapter, machine, job, settings, lifecycle, control };
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("MachineControl jogging", () => {
  it("a click jogs one step in each of the eight directions", async () => {
    const { adapter, control } = setup();
    control.setStep(10);
    control.setFeed(1500);
    for (const arrow of JOG_PAD) {
      control.press(arrow);
      control.release();
    }
    await vi.runAllTimersAsync();
    expect(adapter.jog.mock.calls).toEqual(JOG_PAD.map(({ x, y }) => [{ x, y }, 10, 1500]));
    expect(adapter.jogHold).not.toHaveBeenCalled();
    expect(adapter.jogStop).not.toHaveBeenCalled();
  });

  it("holding a button jogs until release, renewing the hold, then cancels", async () => {
    const { adapter, control } = setup();
    const right = { x: 1, y: 0 };
    control.press(right);
    await vi.advanceTimersByTimeAsync(HOLD_DELAY_MS);
    expect(control.holding).toEqual(right);
    expect(adapter.jogHold).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(RENEW_MS * 5);
    expect(adapter.jogHold).toHaveBeenCalledTimes(6);
    expect(adapter.jogHold).toHaveBeenLastCalledWith(right, control.feed);

    control.release();
    expect(control.holding).toBeNull();
    expect(adapter.jogStop).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(RENEW_MS * 5);
    expect(adapter.jogHold).toHaveBeenCalledTimes(6);
    expect(adapter.jog).not.toHaveBeenCalled();
  });

  it("a lost press stops a held jog without stepping", async () => {
    const { adapter, control } = setup();
    control.press({ x: 0, y: -1 });
    control.cancel();
    await vi.advanceTimersByTimeAsync(HOLD_DELAY_MS * 2);
    expect(adapter.jog).not.toHaveBeenCalled();
    expect(adapter.jogHold).not.toHaveBeenCalled();

    control.press({ x: 0, y: -1 });
    await vi.advanceTimersByTimeAsync(HOLD_DELAY_MS);
    control.cancel();
    expect(adapter.jogStop).toHaveBeenCalledTimes(1);
  });

  it("stops renewing when the machine refuses the hold", async () => {
    const { adapter, control } = setup();
    adapter.jogHold.mockRejectedValueOnce(new Error("A job is running. Stop it first."));
    control.press({ x: 1, y: 1 });
    await vi.advanceTimersByTimeAsync(HOLD_DELAY_MS);
    expect(control.error).toBe("A job is running. Stop it first.");
    expect(control.holding).toBeNull();
    await vi.advanceTimersByTimeAsync(RENEW_MS * 3);
    expect(adapter.jogHold).toHaveBeenCalledTimes(1);
  });

  it("jogs only while connected and idle or jogging, without a job", () => {
    const { machine, job, control } = setup();
    expect(control.canJog).toBe(true);
    machine.status = status({ name: "jog" });
    expect(control.canJog).toBe(true);
    expect(control.idle).toBe(false);
    machine.status = status({ name: "alarm" });
    expect(control.canJog).toBe(false);
    machine.status = status({ name: "idle" });
    job.active = true;
    expect(control.canJog).toBe(false);
    job.active = false;
    machine.connected = false;
    expect(control.canJog).toBe(false);
  });

  it("maps arrow keys to directions", () => {
    expect(arrowDirection("ArrowUp")).toEqual({ x: 0, y: 1 });
    expect(arrowDirection("ArrowDown")).toEqual({ x: 0, y: -1 });
    expect(arrowDirection("ArrowLeft")).toEqual({ x: -1, y: 0 });
    expect(arrowDirection("ArrowRight")).toEqual({ x: 1, y: 0 });
    expect(arrowDirection("a")).toBeNull();
  });

  it("keeps only the offered step sizes and positive speeds", () => {
    const { control } = setup();
    control.setStep(7);
    expect(control.step).toBe(1);
    control.setStep(0.1);
    expect(control.step).toBe(0.1);
    expect(control.setFeed(0)).toBe(false);
    expect(control.setFeed(-5)).toBe(false);
    expect(control.setFeed(6000)).toBe(true);
    expect(control.feed).toBe(6000);
  });
});

describe("MachineControl homing", () => {
  it("disables Home when $22=0", () => {
    const { control } = setup({ settings: [setting(22, "0")], homing: true });
    expect(control.homingEnabled).toBe(false);
    expect(control.canHome).toBe(false);
  });

  it("enables Home when $22=1, including from an alarm", () => {
    const { machine, control } = setup({ settings: [setting(22, "1")] });
    expect(control.canHome).toBe(true);
    machine.status = status({ name: "alarm" });
    expect(control.canHome).toBe(true);
    expect(control.canUnlock).toBe(true);
  });

  it("falls back to the profile before the settings are read", () => {
    expect(setup({ homing: false }).control.canHome).toBe(false);
    expect(setup({ homing: true }).control.canHome).toBe(true);
  });
});

describe("MachineControl commands", () => {
  it("sends commands with the active profile", async () => {
    const { adapter, settings, control } = setup();
    await control.command({ command: "setOrigin" });
    await control.goToOrigin();
    expect(adapter.command.mock.calls).toEqual([
      [{ command: "setOrigin" }, settings.profile],
      [{ command: "goToOrigin", feed: control.feed }, settings.profile],
    ]);
  });

  it("toggles air assist from the reported state", async () => {
    const { adapter, machine, control } = setup();
    await control.toggleAirAssist();
    machine.status = status({ name: "idle" }, true);
    expect(control.airAssist).toBe(true);
    await control.toggleAirAssist();
    expect(adapter.command.mock.calls.map(([command]) => command)).toEqual([
      { command: "airAssist", on: true },
      { command: "airAssist", on: false },
    ]);
  });
});

describe("MachineControl test fire", () => {
  it("defaults to 1% for 100 ms and refuses values above the caps", () => {
    const { control } = setup();
    expect([control.power, control.durationMs]).toEqual([1, 100]);
    expect(control.setPower(TEST_FIRE_MAX_POWER + 1)).toBe(false);
    expect(control.setDuration(TEST_FIRE_MAX_MS + 1)).toBe(false);
    expect(control.setPower(0)).toBe(false);
    expect([control.power, control.durationMs]).toEqual([1, 100]);
    expect(control.setPower(10)).toBe(true);
    expect(control.setDuration(1000)).toBe(true);
  });

  it("holds the app busy while the laser fires", async () => {
    const { adapter, lifecycle, control } = setup();
    const fired = control.testFire();
    expect(control.firing).toBe(true);
    expect(control.idle).toBe(false);
    expect(lifecycle.busy).toEqual([FIRING_REASON]);
    await fired;
    expect(adapter.testFire).toHaveBeenCalledWith({ power: 1, durationMs: 100 }, expect.anything());
    await vi.advanceTimersByTimeAsync(100);
    expect(control.firing).toBe(false);
    expect(lifecycle.canClose).toBe(true);
  });

  it("releases the app at once when the fire is refused", async () => {
    const { adapter, lifecycle, control } = setup();
    adapter.testFire.mockRejectedValueOnce(new Error("The machine must be idle to test fire."));
    await control.testFire();
    expect(control.firing).toBe(false);
    expect(control.error).toBe("The machine must be idle to test fire.");
    expect(lifecycle.canClose).toBe(true);
  });
});

describe("previewControl", () => {
  it("says the preview can't reach a machine", async () => {
    await expect(previewControl.jog({ x: 1, y: 0 }, 1, 1000)).rejects.toThrow(PREVIEW_UNAVAILABLE);
    await expect(previewControl.testFire({ power: 1, durationMs: 100 }, null)).rejects.toThrow(
      PREVIEW_UNAVAILABLE,
    );
  });
});
