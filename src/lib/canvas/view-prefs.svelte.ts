/**
 * How the design canvas draws the document, remembered between launches.
 * `showFills` shades each Fill layer's shapes so what will be burned solid
 * reads at a glance; it is on by default.
 */

const SHOW_FILLS_KEY = "beam-forge:show-fills";

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
  #storage: PrefsStorage | undefined;

  constructor(storage?: PrefsStorage) {
    this.#storage = storage;
    this.showFills = read(storage, SHOW_FILLS_KEY) !== "false";
  }

  setShowFills(value: boolean): void {
    this.showFills = value;
    try {
      this.#storage?.setItem(SHOW_FILLS_KEY, String(value));
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
