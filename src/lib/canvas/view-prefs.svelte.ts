/**
 * How the design canvas draws the document and behaves, remembered between
 * launches. `showFills` shades each Fill layer's shapes so what will be
 * burned solid reads at a glance. `snapping` pulls drags onto other objects,
 * the bed and (with `snapToGrid`) the grid. All are on by default.
 */

const SHOW_FILLS_KEY = "beam-forge:show-fills";
const SNAPPING_KEY = "beam-forge:snapping";
const SNAP_TO_GRID_KEY = "beam-forge:snap-to-grid";

/** The slice of `Storage` the prefs need, so tests can use an in-memory map. */
export interface PrefsStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function read(storage: PrefsStorage | undefined, key: string): string | null {
  try {
    return storage?.getItem(key) ?? null;
  } catch {
    return null; // Disabled storage: fall back to the default.
  }
}

export class ViewPrefs {
  showFills = $state(true);
  snapping = $state(true);
  snapToGrid = $state(true);
  #storage: PrefsStorage | undefined;

  constructor(storage?: PrefsStorage) {
    this.#storage = storage;
    this.showFills = read(storage, SHOW_FILLS_KEY) !== "false";
    this.snapping = read(storage, SNAPPING_KEY) !== "false";
    this.snapToGrid = read(storage, SNAP_TO_GRID_KEY) !== "false";
  }

  setShowFills(value: boolean): void {
    this.showFills = value;
    this.#write(SHOW_FILLS_KEY, value);
  }

  setSnapping(value: boolean): void {
    this.snapping = value;
    this.#write(SNAPPING_KEY, value);
  }

  setSnapToGrid(value: boolean): void {
    this.snapToGrid = value;
    this.#write(SNAP_TO_GRID_KEY, value);
  }

  #write(key: string, value: boolean): void {
    try {
      this.#storage?.setItem(key, String(value));
    } catch {
      // Full or disabled storage only costs remembering the choice.
    }
  }
}

function browserStorage(): PrefsStorage | undefined {
  try {
    return globalThis.window?.localStorage;
  } catch {
    return undefined;
  }
}

export const viewPrefs = new ViewPrefs(browserStorage());
