import { describe, expect, it } from "vite-plus/test";
import type { MaterialPreset } from "#lib/bindings/presets/MaterialPreset.js";
import type { PresetLibrary } from "#lib/bindings/presets/PresetLibrary.js";
import { PREVIEW_NO_PRESETS, previewPresets, type PresetAdapter } from "./adapter.js";
import { PresetLibraryStore } from "./library.svelte.js";

function library(presets: MaterialPreset[]): PresetLibrary {
  return { kind: "beam-forge-material-library", formatVersion: 1, presets };
}

function preset(id: string, material: string): MaterialPreset {
  return {
    id,
    material,
    thickness: 3,
    settings: {
      operation: "cut",
      speed: 300,
      maxPower: 100,
      minPower: 0,
      passes: 1,
      airAssist: true,
    },
    starter: false,
    notes: "",
  };
}

/** Libraries per profile in memory, saved and merged as the Rust store does. */
function fakeAdapter() {
  const stored = new Map<string, MaterialPreset[]>();
  const starters = [preset("starter", "Plywood")];
  const get = (profile: string | null) => stored.get(profile ?? "default") ?? [...starters];
  const put = (profile: string | null, presets: MaterialPreset[]) =>
    stored.set(profile ?? "default", presets);
  let file: PresetLibrary | null = null;
  let next = 1;
  const adapter: PresetAdapter = {
    available: true,
    library: async (profile) => library(get(profile)),
    save: async (profile, given) => {
      const saved = { ...given, id: given.id || `preset-${next++}` };
      const presets = get(profile);
      const index = presets.findIndex((other) => other.id === saved.id);
      const updated = index < 0 ? [...presets, saved] : presets.with(index, saved);
      put(profile, updated);
      return { preset: saved, library: library(updated) };
    },
    remove: async (profile, id) => {
      put(
        profile,
        get(profile).filter((other) => other.id !== id),
      );
      return library(get(profile));
    },
    exportLibrary: async (profile, name) => {
      file = library(get(profile));
      return `/exports/${name}.json`;
    },
    importLibrary: async (profile) => {
      if (!file) return null;
      const presets = get(profile);
      let added = 0;
      let replaced = 0;
      for (const incoming of file.presets) {
        const index = presets.findIndex((other) => other.id === incoming.id);
        if (index < 0) {
          presets.push(incoming);
          added++;
        } else {
          presets[index] = incoming;
          replaced++;
        }
      }
      put(profile, presets);
      return { library: library(presets), added, replaced };
    },
  };
  return adapter;
}

describe("the material library store", () => {
  it("loads the profile's library and reloads when the profile changes", async () => {
    const adapter = fakeAdapter();
    const store = new PresetLibraryStore(adapter);
    await store.follow("serial-com3");
    expect(store.loaded).toBe(true);
    expect(store.presets.map((p) => p.id)).toEqual(["starter"]);

    const saved = await store.save(preset("", "Walnut"));
    expect(saved?.id).toBe("preset-1");
    expect(store.find("preset-1")?.material).toBe("Walnut");

    await store.follow("tcp-127-0-0-1-23232");
    expect(store.presets.map((p) => p.id)).toEqual(["starter"]);
    await store.follow("serial-com3");
    expect(store.presets.map((p) => p.id)).toEqual(["starter", "preset-1"]);
  });

  it("updates presets in place and persists them", async () => {
    const adapter = fakeAdapter();
    const store = new PresetLibraryStore(adapter);
    await store.follow(null);
    const saved = (await store.save(preset("", "Walnut")))!;
    await store.save({ ...saved, settings: { ...saved.settings, speed: 120 } });
    expect(store.presets).toHaveLength(2);
    expect(store.find(saved.id)?.settings.speed).toBe(120);

    const fresh = new PresetLibraryStore(adapter);
    await fresh.follow(null);
    expect(fresh.presets).toEqual(store.presets);

    expect(await store.remove(saved.id)).toBe(true);
    expect(store.find(saved.id)).toBeUndefined();
  });

  it("exports a library and imports it into another machine", async () => {
    const adapter = fakeAdapter();
    const store = new PresetLibraryStore(adapter);
    await store.follow("serial-com3");
    await store.save(preset("", "Walnut"));
    const exported = store.presets;
    expect(await store.exportLibrary("Phecda presets")).toBe("/exports/Phecda presets.json");

    await store.follow("other");
    const imported = await store.importLibrary();
    expect(imported).toMatchObject({ added: 1, replaced: 1 });
    expect(store.presets).toEqual(exported);
  });

  it("keeps the last error until the next attempt", async () => {
    const adapter = fakeAdapter();
    const store = new PresetLibraryStore({
      ...adapter,
      save: async () => {
        throw new Error("The preset for Walnut has no passes.");
      },
    });
    await store.follow(null);
    expect(await store.save(preset("", "Walnut"))).toBeNull();
    expect(store.error).toBe("The preset for Walnut has no passes.");
    await store.reload();
    expect(store.error).toBe("");
  });

  it("drops a slow load for a profile that is no longer active", async () => {
    let release: () => void = () => {};
    const adapter = fakeAdapter();
    const store = new PresetLibraryStore({
      ...adapter,
      library: async (profile) => {
        if (profile === "slow") await new Promise<void>((resolve) => (release = resolve));
        return library([preset(profile ?? "default", "Plywood")]);
      },
    });
    const slow = store.follow("slow");
    await store.follow("fast");
    release();
    await slow;
    expect(store.presets.map((p) => p.id)).toEqual(["fast"]);
  });

  it("says the browser preview has no library", async () => {
    const store = new PresetLibraryStore(previewPresets);
    await store.follow(null);
    expect(store.available).toBe(false);
    expect(store.presets).toEqual([]);
    expect(await store.save(preset("", "Walnut"))).toBeNull();
    expect(store.error).toBe(PREVIEW_NO_PRESETS);
  });
});
