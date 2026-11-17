import type { BedSize } from "#lib/bindings/BedSize.js";
import type { ConnectionState } from "#lib/bindings/ConnectionState.js";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { MachineSetting } from "#lib/bindings/MachineSetting.js";
import type { MachineSettings } from "#lib/bindings/MachineSettings.js";
import { machineAdapter, type MachineAdapter } from "./adapter.js";
import { settingsAdapter, type SettingsAdapter } from "./settings-adapter.js";

const STORAGE_KEY = "beam-forge.machine-profile";

/** GRBL's factory max travel, used until a profile exists. */
export const DEFAULT_BED: BedSize = { width: 200, height: 200 };

type Storage = Pick<globalThis.Storage, "getItem" | "setItem">;

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The connected machine's `$` settings and the active machine profile. The
 * profile is the one source for the bed size, power range and speeds that the
 * editor, GCode generator and time estimator use. Connecting reads `$$`, which
 * creates or refreshes that machine's profile on disk.
 */
export class MachineSettingsStore {
  /** The settings table; empty while disconnected. */
  settings = $state.raw<MachineSetting[]>([]);
  profile = $state.raw<MachineProfile | null>(null);
  profiles = $state.raw<MachineProfile[]>([]);
  loading = $state(false);
  /** The setting being written, if any. */
  writing = $state<number | null>(null);
  /** The last failed read, write or save, until the next attempt. */
  error = $state("");

  readonly #adapter: SettingsAdapter;
  readonly #machine: Pick<MachineAdapter, "state" | "onState">;
  readonly #storage: Storage | undefined;
  /** The port whose settings were read, so a repeated state doesn't re-read. */
  #readFor: string | null = null;

  constructor(
    adapter: SettingsAdapter,
    machine: Pick<MachineAdapter, "state" | "onState">,
    storage?: Storage,
  ) {
    this.#adapter = adapter;
    this.#machine = machine;
    this.#storage = storage;
  }

  /** The `$32` warning: with laser mode off GRBL stops at every move. */
  get laserModeOff(): boolean {
    const row = this.settings.find((setting) => setting.number === 32);
    if (row) return Number(row.value) !== 1;
    return this.profile !== null && !this.profile.laserMode;
  }

  get bed(): BedSize {
    return this.profile?.bed ?? DEFAULT_BED;
  }

  /** Loads saved profiles and reads settings whenever a machine connects. */
  async start(): Promise<() => void> {
    const stop = await this.#machine.onState((state) => void this.#follow(state));
    await this.#loadProfiles();
    await this.#follow(await this.#machine.state());
    return stop;
  }

  /** Reads `$$` again. */
  async read(): Promise<void> {
    this.loading = true;
    this.error = "";
    try {
      this.#adopt(await this.#adapter.read());
    } catch (cause) {
      this.error = message(cause);
    } finally {
      this.loading = false;
    }
  }

  /** Writes one setting. Returns whether the controller accepted it. */
  async write(number: number, value: string): Promise<boolean> {
    this.writing = number;
    this.error = "";
    try {
      this.#adopt(await this.#adapter.write(number, value));
      return true;
    } catch (cause) {
      this.error = message(cause);
      return false;
    } finally {
      this.writing = null;
    }
  }

  /** Saves changes to the active profile. Returns whether they were valid. */
  async saveProfile(changes: Partial<MachineProfile>): Promise<boolean> {
    if (!this.profile) return false;
    this.error = "";
    try {
      this.#setProfile(await this.#adapter.saveProfile({ ...this.profile, ...changes }));
      return true;
    } catch (cause) {
      this.error = message(cause);
      return false;
    }
  }

  selectProfile(id: string): void {
    const profile = this.profiles.find((candidate) => candidate.id === id);
    if (profile) this.#setProfile(profile);
  }

  async #follow(state: ConnectionState): Promise<void> {
    if (state.status !== "connected") {
      this.#readFor = null;
      this.settings = [];
      return;
    }
    if (this.#readFor === state.port.key) return;
    this.#readFor = state.port.key;
    await this.read();
  }

  async #loadProfiles(): Promise<void> {
    try {
      this.profiles = await this.#adapter.profiles();
    } catch (cause) {
      this.error = message(cause);
      return;
    }
    const remembered = this.#storage?.getItem(STORAGE_KEY);
    const profile = this.profiles.find((p) => p.id === remembered) ?? this.profiles[0];
    if (profile && !this.profile) this.profile = profile;
  }

  #adopt(read: MachineSettings): void {
    this.settings = read.settings;
    this.#setProfile(read.profile);
  }

  #setProfile(profile: MachineProfile): void {
    this.profile = profile;
    const others = this.profiles.filter((p) => p.id !== profile.id);
    this.profiles = [...others, profile].sort((a, b) => a.name.localeCompare(b.name));
    this.#storage?.setItem(STORAGE_KEY, profile.id);
  }
}

export const machineSettings = new MachineSettingsStore(
  settingsAdapter,
  machineAdapter,
  globalThis.window?.localStorage,
);
