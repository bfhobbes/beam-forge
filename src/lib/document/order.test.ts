import { describe, expect, it } from "vite-plus/test";
import { applyCommand } from "./commands.js";
import { emptyDocument, type DesignDocument, type SceneObject } from "./model.js";
import { orderCommand, reorder, type OrderMove } from "./order.js";
import { IDENTITY } from "./transform.js";

const rect = (id: string): SceneObject => ({
  type: "rect",
  id,
  name: id,
  layerId: "layer-1",
  transform: IDENTITY,
  width: 10,
  height: 5,
  cornerRadius: 0,
});

const docOf = (...ids: string[]): DesignDocument => ({
  ...emptyDocument(),
  objects: ids.map(rect),
});

const order = (ids: string[], selected: string[], move: OrderMove) =>
  reorder(ids.map(rect), selected, move).map((object) => object.id);

describe("stacking order", () => {
  it("brings forward one step, a run of selected objects moving as a block", () => {
    expect(order(["a", "b", "c", "d"], ["b"], "forward")).toEqual(["a", "c", "b", "d"]);
    expect(order(["a", "b", "c", "d"], ["a", "b"], "forward")).toEqual(["c", "a", "b", "d"]);
    expect(order(["a", "b", "c", "d"], ["a", "c"], "forward")).toEqual(["b", "a", "d", "c"]);
    expect(order(["a", "b", "c"], ["c"], "forward")).toEqual(["a", "b", "c"]);
  });

  it("sends backward one step", () => {
    expect(order(["a", "b", "c", "d"], ["c"], "backward")).toEqual(["a", "c", "b", "d"]);
    expect(order(["a", "b", "c", "d"], ["b", "d"], "backward")).toEqual(["b", "a", "d", "c"]);
    expect(order(["a", "b", "c"], ["a"], "backward")).toEqual(["a", "b", "c"]);
  });

  it("brings to the front or sends to the back, keeping the selection's own order", () => {
    expect(order(["a", "b", "c", "d"], ["c", "a"], "front")).toEqual(["b", "d", "a", "c"]);
    expect(order(["a", "b", "c", "d"], ["d", "b"], "back")).toEqual(["b", "d", "a", "c"]);
  });

  it("makes a command that undoes, or none when nothing would move", () => {
    const doc = docOf("a", "b", "c");
    expect(orderCommand(doc, ["c"], "forward")).toBeNull();
    expect(orderCommand(doc, ["a"], "back")).toBeNull();
    const command = orderCommand(doc, ["a"], "front")!;
    const { doc: next, inverse } = applyCommand(doc, command);
    expect(next.objects.map((object) => object.id)).toEqual(["b", "c", "a"]);
    expect(applyCommand(next, inverse).doc).toEqual(doc);
  });
});
