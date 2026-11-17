import { invoke } from "@tauri-apps/api/core";
import type { JogDirection } from "#lib/bindings/JogDirection.js";
import type { MachineCommand } from "#lib/bindings/MachineCommand.js";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { TestFire } from "#lib/bindings/TestFire.js";
import { desktop } from "#lib/desktop/platform.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";

/** Typed access to the jog pad commands in `src-tauri/src/machine/control.rs`. */
export interface ControlAdapter {
  /** Jogs `distance` mm in `direction` at `feed` mm/min. */
  jog(direction: JogDirection, distance: number, feed: number): Promise<void>;
  /** Starts or renews a held jog; it stops by itself ~400 ms after the last call. */
  jogHold(direction: JogDirection, feed: number): Promise<void>;
  /** Cancels any jog at once. */
  jogStop(): Promise<void>;
  command(command: MachineCommand, profile: MachineProfile | null): Promise<void>;
  /** Fires the laser, capped in Rust at 10% for 1 s. Returns what fired. */
  testFire(fire: TestFire, profile: MachineProfile | null): Promise<TestFire>;
}

export const desktopControl: ControlAdapter = {
  jog: (direction, distance, feed) => invoke<void>("machine_jog", { direction, distance, feed }),
  jogHold: (direction, feed) => invoke<void>("machine_jog_hold", { direction, feed }),
  jogStop: () => invoke<void>("machine_jog_stop"),
  command: (command, profile) => invoke<void>("machine_command", { command, profile }),
  testFire: (fire, profile) => invoke<TestFire>("machine_test_fire", { fire, profile }),
};

const unavailable = async (): Promise<never> => {
  throw new Error(PREVIEW_UNAVAILABLE);
};

// The browser preview has no machine to move.
export const previewControl: ControlAdapter = {
  jog: unavailable,
  jogHold: unavailable,
  jogStop: async () => {},
  command: unavailable,
  testFire: unavailable,
};

export const controlAdapter = desktop ? desktopControl : previewControl;
