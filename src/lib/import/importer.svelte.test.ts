import { describe, expect, it, vi } from "vite-plus/test";
import type { ImportedDrawing } from "#lib/bindings/ImportedDrawing.js";
import type { LengthUnit } from "#lib/bindings/LengthUnit.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { previewImport, PREVIEW_IMPORT_MESSAGE, type ImportAdapter } from "./adapter.js";
import { Importer } from "./importer.svelte.js";

const drawing: ImportedDrawing = {
  format: "svg",
  width: 20,
  height: 10,
  paths: [
    {
      name: "",
      color: "#ff0000",
      subpaths: [
        { start: { x: 0, y: 0 }, segments: [{ kind: "line", to: { x: 5, y: 5 } }], closed: false },
      ],
    },
  ],
  segments: 1,
  dpi: null,
  dpiReason: null,
  units: null,
  skipped: 0,
  substitutedFonts: [],
};

function setup(overrides: Partial<ImportAdapter> = {}) {
  const lifecycle = new Lifecycle();
  const editor = new DocumentEditor(lifecycle);
  const files: string[] = [];
  const adapter: ImportAdapter = {
    available: true,
    pick: () => Promise.resolve({ kind: "imported", file: { name: "Picked", drawing } }),
    file: (path) => {
      files.push(path);
      return Promise.resolve({ kind: "imported", file: { name: path, drawing } });
    },
    svgText: () => Promise.resolve(drawing),
    onFileDrop: () => Promise.resolve(() => {}),
    ...overrides,
  };
  return { lifecycle, editor, files, importer: new Importer(adapter, editor) };
}

describe("Importer", () => {
  it("inserts a picked file as one undo step and reports it", async () => {
    const { editor, importer, lifecycle } = setup();
    expect(await importer.pick()).toBe(true);
    expect(editor.doc.objects).toHaveLength(1);
    expect(editor.doc.objects[0]).toMatchObject({ name: "Picked", layerId: "layer-2" });
    expect(lifecycle.dirty).toBe(true);
    expect(importer.notice?.kind).toBe("done");
    editor.undo();
    expect(editor.doc.objects).toEqual([]);
  });

  it("does nothing when the dialog is cancelled", async () => {
    const { editor, importer } = setup({ pick: () => Promise.resolve(null) });
    expect(await importer.pick()).toBe(false);
    expect(editor.doc.objects).toEqual([]);
    expect(importer.notice).toBeNull();
  });

  it("imports only the SVGs and DXFs among dropped files", async () => {
    const { editor, importer, files } = setup();
    expect(await importer.files(["a.svg", "notes.txt", "b.SVGZ", "c.dxf"])).toBe(true);
    expect(files).toEqual(["a.svg", "b.SVGZ", "c.dxf"]);
    expect(editor.doc.objects).toHaveLength(3);
    expect(await importer.files(["photo.png"])).toBe(false);
    expect(importer.notice).toEqual({
      kind: "error",
      text: "Only SVG and DXF files can be imported.",
    });
  });

  it("asks for a DXF's units when the file doesn't say, then reads it in them", async () => {
    const asked: (LengthUnit | undefined)[] = [];
    const { editor, importer } = setup({
      file: (path, units) => {
        asked.push(units);
        return Promise.resolve(
          units
            ? {
                kind: "imported",
                file: { name: "Bracket", drawing: { ...drawing, format: "dxf" } },
              }
            : { kind: "needsUnits", path, name: "Bracket", width: 4, height: 2 },
        );
      },
    });
    const done = importer.files(["Bracket.dxf"]);
    await vi.waitFor(() => expect(importer.question).not.toBeNull());
    expect(importer.question).toEqual({ name: "Bracket", width: 4, height: 2 });
    expect(importer.busy).toBe(true);
    importer.answer("in");
    expect(await done).toBe(true);
    expect(asked).toEqual([undefined, "in"]);
    expect(importer.question).toBeNull();
    expect(editor.doc.objects[0].name).toBe("Bracket");
  });

  it("imports nothing when the units question is cancelled", async () => {
    const { editor, importer } = setup({
      file: (path) => Promise.resolve({ kind: "needsUnits", path, name: "B", width: 1, height: 1 }),
    });
    const done = importer.files(["B.dxf"]);
    await vi.waitFor(() => expect(importer.question).not.toBeNull());
    importer.answer(null);
    expect(await done).toBe(false);
    expect(editor.doc.objects).toEqual([]);
    expect(importer.notice).toBeNull();
    expect(importer.busy).toBe(false);
  });

  it("names pasted markup and reports backend errors", async () => {
    const { editor, importer } = setup({
      svgText: (text) =>
        text === "bad"
          ? Promise.reject(new Error("This isn't a valid SVG: oops"))
          : Promise.resolve(drawing),
    });
    await importer.text("<svg/>");
    expect(editor.doc.objects[0].name).toBe("Pasted SVG");
    expect(await importer.text("bad")).toBe(false);
    expect(importer.notice).toEqual({ kind: "error", text: "This isn't a valid SVG: oops" });
  });

  it("refuses to edit while an update installs", async () => {
    const { importer, lifecycle, editor } = setup();
    lifecycle.locked = true;
    expect(await importer.pick()).toBe(false);
    expect(editor.doc.objects).toEqual([]);
    expect(importer.notice?.kind).toBe("error");
  });

  it("says the browser preview can't import", async () => {
    const importer = new Importer(previewImport, setup().editor);
    expect(importer.available).toBe(false);
    await importer.pick();
    expect(importer.notice).toEqual({ kind: "error", text: PREVIEW_IMPORT_MESSAGE });
  });
});
