import { describe, expect, it } from "vite-plus/test";
import { buildScene } from "#lib/canvas/scene.js";
import { applyCommand, compose, convertToPath, DocumentError, editShape } from "./commands.js";
import type { DesignDocument, SceneObject } from "./model.js";
import { hasShape, toPath } from "./shapes.js";
import { translate } from "./transform.js";

const base = (id: string) => ({ id, name: id, layerId: "layer-1" });

const shapes: SceneObject[] = [
  {
    ...base("rounded"),
    type: "rect",
    transform: translate(5, 5),
    width: 30,
    height: 20,
    cornerRadius: 4,
  },
  { ...base("ellipse"), type: "ellipse", transform: [2, 0.5, -0.5, 1, 60, 20], rx: 10, ry: 6 },
  {
    ...base("hexagon"),
    type: "polygon",
    transform: translate(100, 30),
    sides: 6,
    radius: 12,
    innerRadius: null,
  },
  {
    ...base("star"),
    type: "polygon",
    transform: translate(140, 30),
    sides: 5,
    radius: 12,
    innerRadius: 5,
  },
  {
    ...base("group"),
    type: "group",
    transform: translate(0, 80),
    children: [
      {
        ...base("inner"),
        type: "rect",
        transform: translate(1, 1),
        width: 8,
        height: 8,
        cornerRadius: 0,
      },
    ],
  },
];

const doc = (objects: SceneObject[]): DesignDocument => ({
  version: 1,
  units: "mm",
  layers: [],
  objects,
});

/** What the canvas draws: each outline in world coordinates. */
const drawn = (document: DesignDocument) =>
  buildScene(document).items.map(({ id, subpaths, bounds }) => ({ id, subpaths, bounds }));

describe("convert to path", () => {
  it("keeps the geometry identical, ids and all", () => {
    const before = doc(shapes);
    const { doc: after } = applyCommand(
      before,
      convertToPath(
        before,
        shapes.map((shape) => shape.id),
      ),
    );
    expect(drawn(after)).toEqual(drawn(before));
    expect(after.objects.map((object) => object.id)).toEqual(shapes.map((shape) => shape.id));
    expect(after.objects.some(hasShape)).toBe(false);
    const group = after.objects[4];
    expect(group.type === "group" && group.children[0].type).toBe("path");
  });

  it("undoes back to the shapes", () => {
    const before = doc(shapes);
    const applied = applyCommand(before, convertToPath(before, ["rounded", "star"]));
    expect(applyCommand(applied.doc, applied.inverse).doc).toEqual(before);
  });

  it("leaves paths alone", () => {
    const path = toPath(shapes[0]);
    expect(toPath(path)).toBe(path);
    const only = doc([path]);
    expect(applyCommand(only, convertToPath(only, [path.id])).doc).toEqual(only);
  });
});

describe("edit shape", () => {
  it("changes a shape's size and undoes", () => {
    const before = doc(shapes);
    const { doc: after, inverse } = applyCommand(
      before,
      editShape("rounded", { width: 40, cornerRadius: 2 }),
    );
    expect(after.objects[0]).toMatchObject({ width: 40, height: 20, cornerRadius: 2 });
    expect(applyCommand(after, inverse).doc).toEqual(before);
  });

  it("turns a polygon into a star and back", () => {
    const before = doc(shapes);
    const { doc: star } = applyCommand(before, editShape("hexagon", { innerRadius: 6 }));
    expect(star.objects[2]).toMatchObject({ innerRadius: 6 });
    const { doc: back } = applyCommand(star, editShape("hexagon", { innerRadius: null }));
    expect(back).toEqual(before);
  });

  it("edits shapes inside groups", () => {
    const { doc: after } = applyCommand(doc(shapes), editShape("inner", { height: 3 }));
    const group = after.objects[4];
    expect(group.type === "group" && group.children[0]).toMatchObject({ height: 3 });
  });

  it.each([
    ["rounded", { width: 0 }, "Size must be above zero"],
    ["rounded", { cornerRadius: -1 }, "Corner radius can't be negative"],
    ["ellipse", { rx: Number.NaN }, "Size must be above zero"],
    ["hexagon", { sides: 2 }, "Sides is a whole number"],
    ["hexagon", { sides: 4.5 }, "Sides is a whole number"],
    ["star", { innerRadius: 0 }, "Inner radius must be above zero"],
    ["rounded", { rx: 3 }, "A rect has no rx"],
    ["group", { width: 3 }, "is not a rectangle, ellipse or polygon"],
  ] as const)("refuses %s %o", (id, changes, message) => {
    expect(() => applyCommand(doc(shapes), editShape(id, changes))).toThrow(DocumentError);
    expect(() => applyCommand(doc(shapes), editShape(id, changes))).toThrow(message);
  });

  it("merges successive edits of one shape", () => {
    expect(compose(editShape("a", { width: 1 }), editShape("a", { height: 2 }))).toEqual(
      editShape("a", { width: 1, height: 2 }),
    );
  });
});
