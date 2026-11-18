import type { BedSize } from "#lib/bindings/BedSize.js";
import { DocumentError } from "#lib/document/commands.js";
import { editor as appEditor, type DocumentEditor } from "#lib/document/editor.svelte.js";
import { newId } from "#lib/document/model.js";
import { selection as appSelection, type Selection } from "#lib/document/selection.svelte.js";
import { machineSettings } from "#lib/machine/settings.svelte.js";
import { testGridAdapter, type TestGridAdapter } from "./adapter.js";
import {
  addTestGrid,
  defaultTestGridSettings,
  findTestGrid,
  type TestGrid,
  type TestGridCells,
  type TestGridSettings,
} from "./test-grid.js";

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** A fresh grid id: short, so layer ids stay readable. */
function gridId(): string {
  return `test-${newId().slice(0, 8)}`;
}

/**
 * The test-grid dialog: its settings, the grid they make (laid out again on
 * every change, so the dialog can say whether it fits on the bed), adding it
 * to the document, and the document's latest grid to pick a cell from.
 */
export class TestGridGenerator {
  /** Which view of the dialog is open: the settings, or picking a cell; null when closed. */
  view = $state<"settings" | "pick" | null>(null);
  settings = $state.raw<TestGridSettings>(defaultTestGridSettings());
  /** The grid the settings make, once laid out. */
  grid = $state.raw<TestGrid | null>(null);
  /** Why the settings don't make a grid (e.g. it doesn't fit), or "". */
  problem = $state("");

  readonly #editor: DocumentEditor;
  readonly #selection: Selection;
  readonly #adapter: TestGridAdapter;
  readonly #bed: () => BedSize;
  #id = gridId();
  /** Bumped on every layout request, so a slow one for old settings is dropped. */
  #request = 0;

  constructor(
    editor: DocumentEditor,
    selection: Selection,
    adapter: TestGridAdapter,
    bed: () => BedSize,
  ) {
    this.#editor = editor;
    this.#selection = selection;
    this.#adapter = adapter;
    this.#bed = bed;
  }

  get available(): boolean {
    return this.#adapter.available;
  }

  get bed(): BedSize {
    return this.#bed();
  }

  /** The document's latest grid, whose cells can be saved as presets. */
  get cells(): TestGridCells | null {
    return findTestGrid(this.#editor.doc);
  }

  /** Opens the settings; `fontFamily` sets the labels in the text tool's family. */
  open(fontFamily?: string): Promise<void> {
    if (fontFamily) {
      this.settings = {
        ...this.settings,
        labels: { ...this.settings.labels, fontFamily },
      };
    }
    this.view = "settings";
    return this.refresh();
  }

  /** Opens the cell picker for the document's latest grid. */
  pick(): void {
    this.view = "pick";
  }

  close(): void {
    this.view = null;
  }

  /**
   * Whether a run that cut `layerIds` burned the document's latest grid, so
   * its cells are ready to be judged and one saved as a preset.
   */
  burnedIn(layerIds: Iterable<string>): boolean {
    const cells = this.cells;
    if (!cells) return false;
    const ids = new Set(cells.cells.flat().flatMap((cell) => (cell ? [cell.id] : [])));
    for (const id of layerIds) if (ids.has(id)) return true;
    return false;
  }

  update(changes: Partial<TestGridSettings>): Promise<void> {
    this.settings = { ...this.settings, ...changes };
    return this.refresh();
  }

  reset(): Promise<void> {
    this.settings = defaultTestGridSettings(this.settings.labels.fontFamily);
    return this.refresh();
  }

  /** Lays the grid out for the current settings and bed. */
  async refresh(): Promise<void> {
    const request = ++this.#request;
    const settings = this.settings;
    try {
      const grid = await this.#adapter.generate(this.#id, settings, this.#bed());
      if (request !== this.#request) return;
      this.grid = grid;
      this.problem = "";
    } catch (cause) {
      if (request !== this.#request) return;
      this.grid = null;
      this.problem = message(cause);
    }
  }

  /**
   * Adds the grid to the document as one undo step, selects it and closes
   * the dialog: there's nothing to pick until the grid has been burned. The
   * Job panel offers the cell picker from then on. False if there is no grid
   * or editing isn't allowed.
   */
  async add(): Promise<boolean> {
    await this.refresh();
    const grid = this.grid;
    if (!grid) return false;
    try {
      if (!this.#editor.execute(addTestGrid(grid))) return false;
    } catch (cause) {
      if (!(cause instanceof DocumentError)) throw cause;
      this.problem = cause.message;
      return false;
    }
    this.#selection.set(grid.objects.map((object) => object.id));
    // The next grid gets ids of its own.
    this.#id = gridId();
    this.grid = null;
    this.view = null;
    return true;
  }
}

export const testGrid = new TestGridGenerator(
  appEditor,
  appSelection,
  testGridAdapter,
  () => machineSettings.bed,
);
