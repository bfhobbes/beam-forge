import { invoke } from "@tauri-apps/api/core";
import type { MaterialPreset } from "#lib/bindings/presets/MaterialPreset.js";
import type { PresetImport } from "#lib/bindings/presets/PresetImport.js";
import type { PresetLibrary } from "#lib/bindings/presets/PresetLibrary.js";
import type { SavedPreset } from "#lib/bindings/presets/SavedPreset.js";
import { desktop } from "#lib/desktop/platform.js";

/**
 * Typed access to the material library commands in `src-tauri/src/presets.rs`.
 * `profile` is the machine profile id whose library to use; null before any
 * machine has a profile.
 */
export interface PresetAdapter {
  /** False in the browser preview, which has no app data folder. */
  readonly available: boolean;
  /** The profile's library: the starter presets until it has its own. */
  library(profile: string | null): Promise<PresetLibrary>;
  /** Adds a preset (an empty id gets a new one) or updates the one with its id. */
  save(profile: string | null, preset: MaterialPreset): Promise<SavedPreset>;
  remove(profile: string | null, id: string): Promise<PresetLibrary>;
  /** Shows the save dialog and writes the library; null if the user cancelled. */
  exportLibrary(profile: string | null, name: string): Promise<string | null>;
  /** Shows the open dialog and merges the file in; null if the user cancelled. */
  importLibrary(profile: string | null): Promise<PresetImport | null>;
}

export const desktopPresets: PresetAdapter = {
  available: true,
  library: (profile) => invoke<PresetLibrary>("presets_library", { profile }),
  save: (profile, preset) => invoke<SavedPreset>("presets_save", { profile, preset }),
  remove: (profile, id) => invoke<PresetLibrary>("presets_delete", { profile, id }),
  exportLibrary: (profile, name) => invoke<string | null>("presets_export", { profile, name }),
  importLibrary: (profile) => invoke<PresetImport | null>("presets_import", { profile }),
};

export const PREVIEW_NO_PRESETS =
  "Browser preview: the material library is stored by the desktop app.";

// The browser preview has no app data folder: an empty library that says why.
export const previewPresets: PresetAdapter = {
  available: false,
  library: async () => ({ kind: "beam-forge-material-library", formatVersion: 1, presets: [] }),
  save: async () => {
    throw new Error(PREVIEW_NO_PRESETS);
  },
  remove: async () => {
    throw new Error(PREVIEW_NO_PRESETS);
  },
  exportLibrary: async () => {
    throw new Error(PREVIEW_NO_PRESETS);
  },
  importLibrary: async () => {
    throw new Error(PREVIEW_NO_PRESETS);
  },
};

export const presetAdapter = desktop ? desktopPresets : previewPresets;
