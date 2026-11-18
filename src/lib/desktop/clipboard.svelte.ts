import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { desktop } from "./platform.js";

/** Plain-text clipboard access, so copy and paste can be tested without a window. */
export interface TextClipboard {
  /** False when the clipboard is known to be empty: nothing copied yet in the browser preview. */
  readonly mayHaveText: boolean;
  read(): Promise<string | null>;
  write(text: string): Promise<void>;
}

/** Keeps the text in memory: the browser preview's clipboard, and tests'. */
export class MemoryClipboard implements TextClipboard {
  #text = $state<string | null>(null);

  get mayHaveText(): boolean {
    return this.#text !== null;
  }

  async read(): Promise<string | null> {
    return this.#text;
  }

  async write(text: string): Promise<void> {
    this.#text = text;
  }
}

/**
 * The system clipboard through Tauri's clipboard plugin, so objects copy
 * between Beam Forge windows and survive a restart. If writing to it fails,
 * the copy stays in memory and paste reads that until the next copy.
 */
class SystemClipboard implements TextClipboard {
  readonly mayHaveText = true;
  /** Set while the last copy only reached memory. */
  #unwritten: string | null = null;

  async read(): Promise<string | null> {
    if (this.#unwritten !== null) return this.#unwritten;
    try {
      return await readText();
    } catch {
      return null; // Empty, or holding something other than text.
    }
  }

  async write(text: string): Promise<void> {
    try {
      await writeText(text);
      this.#unwritten = null;
    } catch (error) {
      console.warn("Writing the system clipboard failed; kept an in-app copy", error);
      this.#unwritten = text;
    }
  }
}

/** The app's clipboard: the system one on the desktop, in memory in the browser preview. */
export const clipboard: TextClipboard = desktop ? new SystemClipboard() : new MemoryClipboard();
