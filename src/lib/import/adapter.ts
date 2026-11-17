import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { ImportedDrawing } from "#lib/bindings/ImportedDrawing.js";
import type { ImportOutcome } from "#lib/bindings/ImportOutcome.js";
import type { LengthUnit } from "#lib/bindings/LengthUnit.js";
import { desktop } from "#lib/desktop/platform.js";

/** Typed access to the import commands in `src-tauri/src/import.rs`. */
export interface ImportAdapter {
  /** False in the browser preview, which has no importer. */
  readonly available: boolean;
  /** Asks for an SVG or DXF file and reads it; null when the user cancels. */
  pick(): Promise<ImportOutcome | null>;
  /**
   * Reads an SVG or DXF file by path, e.g. one dropped on the window.
   * `units` answers a DXF's `needsUnits` outcome.
   */
  file(path: string, units?: LengthUnit): Promise<ImportOutcome>;
  /** Reads SVG markup, e.g. pasted text. */
  svgText(text: string): Promise<ImportedDrawing>;
  /**
   * Files dragged over the window: `hover` with their paths while over it
   * (null once they leave), `drop` with the paths dropped.
   */
  onFileDrop(handler: FileDropHandler): Promise<() => void>;
}

export interface FileDropHandler {
  hover(paths: string[] | null): void;
  drop(paths: string[]): void;
}

/** Paths the importer accepts: SVG and DXF. */
export function isImportPath(path: string): boolean {
  return /\.(svgz?|dxf)$/i.test(path);
}

export const desktopImport: ImportAdapter = {
  available: true,
  pick: () => invoke<ImportOutcome | null>("import_pick"),
  file: (path, units) => invoke<ImportOutcome>("import_file", { path, units: units ?? null }),
  svgText: (text) => invoke<ImportedDrawing>("import_svg_text", { text }),
  onFileDrop: (handler) =>
    getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === "enter") handler.hover(payload.paths);
      else if (payload.type === "leave") handler.hover(null);
      else if (payload.type === "drop") {
        handler.hover(null);
        handler.drop(payload.paths);
      }
    }),
};

export const PREVIEW_IMPORT_MESSAGE =
  "Browser preview: importing needs the desktop app (npm run desktop:dev).";

// The browser preview has no Rust side: importing says why.
export const previewImport: ImportAdapter = {
  available: false,
  pick: () => Promise.reject(new Error(PREVIEW_IMPORT_MESSAGE)),
  file: () => Promise.reject(new Error(PREVIEW_IMPORT_MESSAGE)),
  svgText: () => Promise.reject(new Error(PREVIEW_IMPORT_MESSAGE)),
  onFileDrop: () => Promise.resolve(() => {}),
};

export const importAdapter = desktop ? desktopImport : previewImport;
