import { describe, expect, it } from "vite-plus/test";
import type { ImportedDrawing } from "#lib/bindings/ImportedDrawing.js";
import { applyCommand } from "#lib/document/commands.js";
import { emptyDocument, type GroupObject } from "#lib/document/model.js";
import { IDENTITY } from "#lib/document/transform.js";
import { describeImport, looksLikeSvg, planImport, sizeInUnits } from "./drawing.js";

function drawing(colors: string[]): ImportedDrawing {
  return {
    format: "svg",
    width: 100,
    height: 50,
    paths: colors.map((color, i) => ({
      name: i === 0 ? "outline" : "",
      color,
      subpaths: [
        {
          start: { x: 0, y: 0 },
          segments: [
            { kind: "line", to: { x: 10, y: 0 } },
            { kind: "line", to: { x: 10, y: 10 } },
            { kind: "line", to: { x: 0, y: 10 } },
          ],
          closed: true,
        },
      ],
    })),
    segments: colors.length * 3,
    dpi: 72,
    dpiReason: "Adobe Illustrator",
    units: null,
    skipped: 1,
    substitutedFonts: [],
  };
}

function ids() {
  let next = 0;
  return () => `id-${++next}`;
}

describe("planning an import", () => {
  it("groups the paths under the file name, each on its colour's layer", () => {
    const plan = planImport(drawing(["#ff0000", "#000000"]), "Coaster", emptyDocument(), ids());
    const group = plan.object as GroupObject;
    expect(group).toMatchObject({ type: "group", name: "Coaster", transform: IDENTITY });
    expect(group.children.map((child) => [child.name, child.layerId])).toEqual([
      ["outline", "layer-2"],
      ["Path 2", "layer-1"],
    ]);
    expect(plan.layers).toEqual(["layer-2", "layer-1"]);
  });

  it("is one undoable step that adds the layers it needs", () => {
    const plan = planImport(drawing(["#ff0000", "#0000ff"]), "Coaster", emptyDocument(), ids());
    const { doc, inverse } = applyCommand(emptyDocument(), plan.command);
    expect(doc.objects).toEqual([plan.object]);
    expect(doc.layers.map((layer) => [layer.id, layer.color])).toEqual([
      ["layer-2", "#ff0000"],
      ["layer-1", "#0000ff"],
    ]);
    expect(applyCommand(doc, inverse).doc).toEqual(emptyDocument());

    const again = planImport(drawing(["#ff0000"]), "Tag", doc, ids());
    expect(again.command.type).toBe("insert");
  });

  it("inserts a lone path ungrouped, named after the file if unnamed", () => {
    const single = drawing(["#ff0000"]);
    single.paths[0].name = "";
    const plan = planImport(single, "Tag", emptyDocument(), ids());
    expect(plan.object).toMatchObject({ type: "path", name: "Tag", layerId: "layer-2" });
  });

  it("summarises size, DPI and what was left out", () => {
    expect(describeImport(drawing(["#ff0000"]), "Coaster", 1)).toBe(
      "Imported “Coaster”: 1 path on 1 layer, 100 × 50 mm at 72 DPI (Adobe Illustrator). Left out 1 image.",
    );
  });

  it("summarises a DXF with its units and what it left out", () => {
    const dxf: ImportedDrawing = {
      ...drawing(["#ff0000", "#0000ff"]),
      format: "dxf",
      dpi: null,
      dpiReason: null,
      units: "inches",
      skipped: 2,
    };
    expect(describeImport(dxf, "", 2)).toBe(
      "Imported DXF: 2 paths on 2 layers, 100 × 50 mm, drawn in inches. Left out 2 items that can’t be cut, such as hatching or dimensions.",
    );
  });

  it("names the fonts a DXF's text was drawn in instead", () => {
    const dxf: ImportedDrawing = {
      ...drawing(["#ff0000"]),
      format: "dxf",
      dpi: null,
      dpiReason: null,
      units: "millimetres",
      skipped: 0,
      substitutedFonts: [
        { requested: "romans.shx", used: "Arial" },
        { requested: "ISOCPEUR", used: "Arial" },
      ],
    };
    expect(describeImport(dxf, "Panel", 1)).toBe(
      "Imported “Panel”: 1 path on 1 layer, 100 × 50 mm, drawn in millimetres. Text in romans.shx was drawn in Arial. Text in ISOCPEUR was drawn in Arial.",
    );
  });

  it("shows a unitless drawing's size both ways", () => {
    expect(sizeInUnits({ width: 4, height: 2 }, "mm")).toBe("4 × 2 mm");
    expect(sizeInUnits({ width: 4, height: 2 }, "in")).toBe("101.6 × 50.8 mm");
  });

  it("recognises pasted SVG markup", () => {
    expect(looksLikeSvg('<?xml version="1.0"?>\n<svg xmlns="http://www.w3.org/2000/svg"/>')).toBe(
      true,
    );
    expect(looksLikeSvg("  <svg>")).toBe(true);
    expect(looksLikeSvg("an <svg> mentioned in prose")).toBe(false);
    expect(looksLikeSvg("<svgfoo/>")).toBe(false);
  });
});
