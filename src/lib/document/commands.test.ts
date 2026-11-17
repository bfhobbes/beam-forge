import { describe, expect, it } from "vite-plus/test";
import {
  applyCommand,
  compose,
  DocumentError,
  group,
  insert,
  remove,
  setTransforms,
  ungroup,
  type Command,
} from "./commands.js";
import { emptyDocument, findPlacement, type DesignDocument, type SceneObject } from "./model.js";
import { IDENTITY, translate } from "./transform.js";

function rect(id: string, transform = IDENTITY): SceneObject {
  return {
    type: "rect",
    id,
    name: id,
    layerId: "layer-1",
    transform,
    width: 10,
    height: 5,
    cornerRadius: 0,
  };
}

function ids(objects: readonly SceneObject[]): string[] {
  return objects.map((object) => object.id);
}

/** Applies a command, checks its inverse restores the document, and returns the result. */
function roundTrip(doc: DesignDocument, command: Command): DesignDocument {
  const { doc: next, inverse } = applyCommand(doc, command);
  const { doc: back, inverse: redo } = applyCommand(next, inverse);
  expect(back).toEqual(doc);
  expect(applyCommand(back, redo).doc).toEqual(next);
  return next;
}

function docWith(...objects: SceneObject[]): DesignDocument {
  return { ...emptyDocument(), objects };
}

describe("document commands", () => {
  it("inserts objects at the end or at an index, and the inverse removes them", () => {
    const doc = docWith(rect("a"), rect("b"));
    const appended = roundTrip(doc, insert([rect("c")]));
    expect(ids(appended.objects)).toEqual(["a", "b", "c"]);
    const placed = roundTrip(doc, insert([rect("c")], { index: 1 }));
    expect(ids(placed.objects)).toEqual(["a", "c", "b"]);
  });

  it("does not mutate the input document", () => {
    const doc = docWith(rect("a"));
    const frozen = structuredClone(doc);
    applyCommand(doc, insert([rect("b")]));
    applyCommand(doc, setTransforms({ a: translate(1, 2) }));
    expect(doc).toEqual(frozen);
  });

  it("refuses duplicate ids and unknown targets", () => {
    const doc = docWith(rect("a"));
    expect(() => applyCommand(doc, insert([rect("a")]))).toThrow(DocumentError);
    expect(() => applyCommand(doc, remove(["missing"]))).toThrow(DocumentError);
    expect(() => applyCommand(doc, setTransforms({ missing: IDENTITY }))).toThrow(DocumentError);
    expect(() => applyCommand(doc, insert([rect("b")], { parent: "a" }))).toThrow(DocumentError);
  });

  it("removes objects anywhere in the tree and restores them in place", () => {
    const grouped = roundTrip(
      docWith(rect("a"), rect("b"), rect("c"), rect("d")),
      group(["b", "c"], "g"),
    );
    const doc = roundTrip(grouped, insert([rect("e")], { index: 0 }));
    const next = roundTrip(doc, remove(["c", "e", "d"]));
    expect(ids(next.objects)).toEqual(["a", "g"]);
    expect(findPlacement(next.objects, "b")?.parent).toBe("g");
  });

  it("removes a group and a child of it together", () => {
    const doc = applyCommand(docWith(rect("a"), rect("b")), group(["a", "b"], "g")).doc;
    expect(roundTrip(doc, remove(["a", "g"])).objects).toEqual([]);
  });

  it("sets absolute transforms and the inverse restores the old ones", () => {
    const doc = docWith(rect("a"), rect("b", translate(5, 5)));
    const next = roundTrip(doc, setTransforms({ a: translate(1, 0), b: translate(0, 1) }));
    expect(findPlacement(next.objects, "a")?.object.transform).toEqual(translate(1, 0));
    expect(findPlacement(next.objects, "b")?.object.transform).toEqual(translate(0, 1));
  });

  it("groups siblings at the topmost member's position, keeping their order", () => {
    const doc = docWith(rect("a"), rect("b"), rect("c"), rect("d"));
    const next = roundTrip(doc, group(["c", "a"], "g", "Group 1"));
    expect(ids(next.objects)).toEqual(["b", "g", "d"]);
    const created = findPlacement(next.objects, "g")?.object;
    expect(created?.type === "group" && ids(created.children)).toEqual(["a", "c"]);
    expect(created?.name).toBe("Group 1");
  });

  it("refuses to group objects with different parents", () => {
    const doc = applyCommand(docWith(rect("a"), rect("b"), rect("c")), group(["a", "b"], "g")).doc;
    expect(() => applyCommand(doc, group(["a", "c"], "h"))).toThrow(DocumentError);
    expect(() => applyCommand(doc, group([], "h"))).toThrow(DocumentError);
  });

  it("ungroups in place, baking the group's transform into its children", () => {
    const doc = docWith(
      rect("a"),
      {
        type: "group",
        id: "g",
        name: "g",
        layerId: "layer-1",
        transform: translate(10, 0),
        children: [rect("b", translate(0, 3)), rect("c")],
      },
      rect("d"),
    );
    const next = roundTrip(doc, ungroup(["g"]));
    expect(ids(next.objects)).toEqual(["a", "b", "c", "d"]);
    expect(findPlacement(next.objects, "b")?.object.transform).toEqual(translate(10, 3));
    expect(findPlacement(next.objects, "c")?.object.transform).toEqual(translate(10, 0));
  });

  it("refuses to ungroup something that is not a group", () => {
    expect(() => applyCommand(docWith(rect("a")), ungroup(["a"]))).toThrow(DocumentError);
  });

  it("composes commands, merging transform updates where they meet", () => {
    let step = insert([rect("a")]);
    for (let i = 1; i <= 100; i++) step = compose(step, setTransforms({ a: translate(i, 0) }));
    expect(step).toEqual({
      type: "batch",
      commands: [insert([rect("a")]), setTransforms({ a: translate(100, 0) })],
    });
  });

  it("round-trips through plain JSON", () => {
    let doc = docWith(rect("a"), rect("b"));
    doc = applyCommand(doc, group(["a", "b"], "g")).doc;
    doc = applyCommand(
      doc,
      insert([
        {
          type: "path",
          id: "p",
          name: "p",
          layerId: "layer-1",
          transform: IDENTITY,
          subpaths: [
            {
              start: { x: 0, y: 0 },
              segments: [
                { kind: "line", to: { x: 1, y: 0 } },
                { kind: "quad", control: { x: 2, y: 1 }, to: { x: 1, y: 2 } },
                {
                  kind: "cubic",
                  control1: { x: 0, y: 3 },
                  control2: { x: -1, y: 1 },
                  to: { x: 0, y: 0 },
                },
              ],
              closed: true,
            },
          ],
        },
      ]),
    ).doc;
    expect(JSON.parse(JSON.stringify(doc))).toEqual(doc);
  });
});
