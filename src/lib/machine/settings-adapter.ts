import { invoke } from "@tauri-apps/api/core";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { MachineSettings } from "#lib/bindings/MachineSettings.js";
import { desktop } from "#lib/desktop/platform.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";

/** Typed access to the settings and profile commands in `src-tauri/src/machine/settings.rs`. */
export interface SettingsAdapter {
  /** Reads `$$` and creates or refreshes the connected machine's profile. */
  read(): Promise<MachineSettings>;
  /** Writes `$number=value`, then reads every setting back. */
  write(number: number, value: string): Promise<MachineSettings>;
  profiles(): Promise<MachineProfile[]>;
  saveProfile(profile: MachineProfile): Promise<MachineProfile>;
}

export const desktopSettings: SettingsAdapter = {
  read: () => invoke<MachineSettings>("machine_settings"),
  write: (number, value) => invoke<MachineSettings>("machine_write_setting", { number, value }),
  profiles: () => invoke<MachineProfile[]>("machine_profiles"),
  saveProfile: (profile) => invoke<MachineProfile>("machine_profile_save", { profile }),
};

// The browser preview has no machine and no app data folder to keep profiles in.
export const previewSettings: SettingsAdapter = {
  read: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  write: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  profiles: async () => [],
  saveProfile: async () => {
    throw new Error("Browser preview: machine profiles are saved by the desktop app.");
  },
};

export const settingsAdapter = desktop ? desktopSettings : previewSettings;
