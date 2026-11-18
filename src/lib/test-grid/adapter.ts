import { invoke } from "@tauri-apps/api/core";
import type { BedSize } from "#lib/bindings/BedSize.js";
import type { TestGrid } from "#lib/bindings/test-grid/TestGrid.js";
import type { TestGridSettings } from "#lib/bindings/test-grid/TestGridSettings.js";
import { desktop } from "#lib/desktop/platform.js";

/** Typed access to the test-grid command in `src-tauri/src/test_grid.rs`. */
export interface TestGridAdapter {
  /** False in the browser preview, which has no fonts to set the labels in. */
  readonly available: boolean;
  /**
   * The grid's layers and objects, ids made from `id`. Rejects with why it
   * can't be made, including that it doesn't fit on `bed`.
   */
  generate(id: string, settings: TestGridSettings, bed: BedSize): Promise<TestGrid>;
}

export const desktopTestGrid: TestGridAdapter = {
  available: true,
  generate: (id, settings, bed) => invoke<TestGrid>("test_grid_generate", { id, settings, bed }),
};

export const PREVIEW_NO_TEST_GRID =
  "Browser preview: test grids need the desktop app, which sets the labels in the system's fonts.";

export const previewTestGrid: TestGridAdapter = {
  available: false,
  generate: async () => {
    throw new Error(PREVIEW_NO_TEST_GRID);
  },
};

export const testGridAdapter = desktop ? desktopTestGrid : previewTestGrid;
