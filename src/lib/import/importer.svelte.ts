import type { ImportedFile } from "#lib/bindings/ImportedFile.js";
import type { ImportOutcome } from "#lib/bindings/ImportOutcome.js";
import type { LengthUnit } from "#lib/bindings/LengthUnit.js";
import { editor as appEditor, type DocumentEditor } from "#lib/document/editor.svelte.js";
import { importAdapter, isImportPath, type ImportAdapter } from "./adapter.js";
import { describeImport, planImport } from "./drawing.js";

export interface ImportNotice {
  kind: "done" | "error";
  text: string;
}

/** A DXF that doesn't say its units, waiting for the user to choose. */
export interface UnitsQuestion {
  name: string;
  /** The drawing's size in its own units. */
  width: number;
  height: number;
}

/**
 * Runs SVG and DXF imports from the menu, a drop or a paste, one at a time,
 * and inserts each as a single undoable command. `notice` reports the
 * outcome; `question` asks for a DXF's units when the file doesn't say.
 */
export class Importer {
  busy = $state(false);
  notice = $state.raw<ImportNotice | null>(null);
  question = $state.raw<UnitsQuestion | null>(null);
  #answer: ((units: LengthUnit | null) => void) | null = null;
  #adapter: ImportAdapter;
  #editor: DocumentEditor;

  constructor(adapter: ImportAdapter, editor: DocumentEditor) {
    this.#adapter = adapter;
    this.#editor = editor;
  }

  get available(): boolean {
    return this.#adapter.available;
  }

  /** Imports a file the user picks. */
  pick(): Promise<boolean> {
    return this.#run(() => this.#adapter.pick());
  }

  /** Imports the SVG and DXF files among `paths`, in order, e.g. a drop. */
  async files(paths: readonly string[]): Promise<boolean> {
    const importable = paths.filter(isImportPath);
    if (importable.length === 0) {
      this.notice = { kind: "error", text: "Only SVG and DXF files can be imported." };
      return false;
    }
    let imported = false;
    for (const path of importable) {
      if (await this.#run(() => this.#adapter.file(path))) imported = true;
    }
    return imported;
  }

  /** Imports SVG markup, e.g. a paste. */
  text(text: string): Promise<boolean> {
    return this.#run(async () => ({
      kind: "imported",
      file: { name: "Pasted SVG", drawing: await this.#adapter.svgText(text) },
    }));
  }

  /** Answers `question`: the units to read the DXF in, or null to cancel. */
  answer(units: LengthUnit | null): void {
    const answer = this.#answer;
    this.#answer = null;
    this.question = null;
    answer?.(units);
  }

  dismiss(): void {
    this.notice = null;
  }

  async #run(load: () => Promise<ImportOutcome | null>): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    try {
      const file = await this.#resolve(await load());
      if (!file) return false;
      const plan = planImport(file.drawing, file.name, this.#editor.doc);
      if (!this.#editor.execute(plan.command)) {
        this.notice = { kind: "error", text: "Can't import while an update installs." };
        return false;
      }
      this.notice = {
        kind: "done",
        text: describeImport(file.drawing, file.name, plan.layers.length),
      };
      return true;
    } catch (error) {
      this.notice = { kind: "error", text: error instanceof Error ? error.message : String(error) };
      return false;
    } finally {
      this.busy = false;
    }
  }

  /** The file an outcome brings in, asking for units first if it needs them. */
  async #resolve(outcome: ImportOutcome | null): Promise<ImportedFile | null> {
    if (!outcome) return null;
    if (outcome.kind === "imported") return outcome.file;
    const units = await new Promise<LengthUnit | null>((resolve) => {
      this.#answer = resolve;
      this.question = { name: outcome.name, width: outcome.width, height: outcome.height };
    });
    if (!units) return null;
    const retried = await this.#adapter.file(outcome.path, units);
    return retried.kind === "imported" ? retried.file : null;
  }
}

export const importer = new Importer(importAdapter, appEditor);
