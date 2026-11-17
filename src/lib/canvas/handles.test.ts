import { describe, expect, it } from "vite-plus/test";
import { handleAt, handleCursor, selectionHandles } from "./handles.js";
import type { View } from "./view.js";

// 2 px/mm with the world origin at screen (0, 200): world (x, y) is screen (2x, 200 - 2y).
const view: View = { zoom: 2, x: 0, y: 200 };
const box = { minX: 10, minY: 10, maxX: 60, maxY: 40 };

describe("selection handles", () => {
  const handles = selectionHandles(view, box);

  it("puts scale handles on the corners and edge middles, in screen space", () => {
    const at = (id: string) => handles.find((h) => h.kind === "scale" && h.id === id)?.at;
    expect(at("sw")).toEqual({ x: 20, y: 180 });
    expect(at("ne")).toEqual({ x: 120, y: 120 });
    expect(at("n")).toEqual({ x: 70, y: 120 });
    expect(at("e")).toEqual({ x: 120, y: 150 });
  });

  it("puts rotate handles diagonally outside each corner", () => {
    const ne = handles.find((h) => h.kind === "rotate" && h.id === "ne")!;
    expect(ne.at.x).toBeGreaterThan(120);
    expect(ne.at.y).toBeLessThan(120);
    expect(handles.filter((h) => h.kind === "rotate")).toHaveLength(4);
  });

  it("hits the nearest handle within reach", () => {
    expect(handleAt(handles, { x: 122, y: 118 })).toMatchObject({ kind: "scale", id: "ne" });
    const ne = handles.find((h) => h.kind === "rotate" && h.id === "ne")!;
    expect(handleAt(handles, ne.at)).toMatchObject({ kind: "rotate", id: "ne" });
    expect(handleAt(handles, { x: 70, y: 150 })).toBeNull();
  });

  it("drops edge handles when the box is too small to tell them apart", () => {
    const tiny = selectionHandles(view, { minX: 10, minY: 10, maxX: 14, maxY: 12 });
    expect(tiny.filter((h) => h.kind === "scale").map((h) => h.id)).toEqual([
      "nw",
      "ne",
      "se",
      "sw",
    ]);
  });

  it("picks a resize cursor that matches the handle", () => {
    expect(handleCursor({ kind: "scale", id: "nw", at: { x: 0, y: 0 } })).toBe("nwse-resize");
    expect(handleCursor({ kind: "scale", id: "ne", at: { x: 0, y: 0 } })).toBe("nesw-resize");
    expect(handleCursor({ kind: "scale", id: "e", at: { x: 0, y: 0 } })).toBe("ew-resize");
    expect(handleCursor({ kind: "scale", id: "s", at: { x: 0, y: 0 } })).toBe("ns-resize");
  });
});
