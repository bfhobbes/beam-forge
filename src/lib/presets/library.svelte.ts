import type { MaterialPreset } from "#lib/bindings/presets/MaterialPreset.js";
import type { PresetImport } from "#lib/bindings/presets/PresetImport.js";
import { presetAdapter, type PresetAdapter } from "./adapter.js";

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The active machine profile's material library. `follow` loads it whenever
 * the profile changes; every change is saved by Rust before it shows here.
 */
export class PresetLibraryStore {
  presets = $state.raw<MaterialPreset[]>([]);
  /** Whether the profile's library has been read. */
  loaded = $state(false);
  /** The last failed load, save, import or export, until the next attempt. */
  error = $state("");

  readonly #adapter: PresetAdapter;
  #profile: string | null = null;
  #following = false;
  /** Bumped on every profile change, so a slow load for an old profile is dropped. */
  #generation = 0;

  constructor(adapter: PresetAdapter) {
    this.#adapter = adapter;
  }

  /** False in the browser preview. */
  get available(): boolean {
    return this.#adapter.available;
  }

  find(id: string | undefined): MaterialPreset | undefined {
    return id === undefined ? undefined : this.presets.find((preset) => preset.id === id);
  }

  /** Loads `profile`'s library, unless it is the one already loaded. */
  async follow(profile: string | null): Promise<void> {
    if (this.#following && profile === this.#profile) return;
    this.#following = true;
    this.#profile = profile;
    await this.reload();
  }

  async reload(): Promise<void> {
    const generation = ++this.#generation;
    this.error = "";
    try {
      const library = await this.#adapter.library(this.#profile);
      if (generation !== this.#generation) return;
      this.presets = library.presets;
      this.loaded = true;
    } catch (cause) {
      if (generation !== this.#generation) return;
      this.presets = [];
      this.loaded = false;
      this.error = message(cause);
    }
  }

  /** Adds or updates a preset. The preset as saved (with its id), or null if it failed. */
  async save(preset: MaterialPreset): Promise<MaterialPreset | null> {
    return this.#change(async () => {
      const saved = await this.#adapter.save(this.#profile, preset);
      this.presets = saved.library.presets;
      return saved.preset;
    });
  }

  async remove(id: string): Promise<boolean> {
    const done = await this.#change(async () => {
      this.presets = (await this.#adapter.remove(this.#profile, id)).presets;
      return true;
    });
    return done ?? false;
  }

  /** Writes the library to a file the user picks. The path, or null if cancelled or failed. */
  async exportLibrary(name: string): Promise<string | null> {
    return this.#change(() => this.#adapter.exportLibrary(this.#profile, name));
  }

  /** Merges in a library file the user picks. What it did, or null if cancelled or failed. */
  async importLibrary(): Promise<PresetImport | null> {
    return this.#change(async () => {
      const imported = await this.#adapter.importLibrary(this.#profile);
      if (imported) this.presets = imported.library.presets;
      return imported;
    });
  }

  async #change<T>(work: () => Promise<T>): Promise<T | null> {
    this.error = "";
    try {
      return await work();
    } catch (cause) {
      this.error = message(cause);
      return null;
    }
  }
}

export const presetLibrary = new PresetLibraryStore(presetAdapter);
