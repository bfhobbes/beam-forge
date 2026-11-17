/** The slice of `Storage` panel layouts need, so tests can use an in-memory map. */
export interface LayoutStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const prefix = "beam-forge:panels:";

function browserStorage(): LayoutStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null; // Storage can be disabled; layouts then reset each launch.
  }
}

function valid(value: unknown, defaults: readonly number[]): value is number[] {
  if (!Array.isArray(value) || value.length !== defaults.length) return false;
  if (!value.every((size) => typeof size === "number" && Number.isFinite(size) && size >= 0))
    return false;
  const total = value.reduce((sum, size) => sum + size, 0);
  return Math.abs(total - 100) < 0.5;
}

/**
 * Pane sizes (percentages) for one resizable group, persisted in localStorage.
 * Sizes are a UI preference, not project data. Unreadable, stale or corrupt
 * entries fall back to the defaults.
 */
export function panelLayout(
  id: string,
  defaults: readonly number[],
  storage: LayoutStorage | null = browserStorage(),
) {
  const key = prefix + id;

  function load(): number[] {
    try {
      const stored: unknown = JSON.parse(storage?.getItem(key) ?? "null");
      if (valid(stored, defaults)) return stored;
    } catch {
      // Corrupt entry: use the defaults.
    }
    return [...defaults];
  }

  return {
    /** The sizes to start with: the last saved layout, or the defaults. */
    sizes: load(),
    save(sizes: readonly number[]) {
      if (!valid(sizes, defaults)) return;
      try {
        storage?.setItem(key, JSON.stringify(sizes));
      } catch {
        // Quota or privacy errors only cost the preference.
      }
    },
  };
}
