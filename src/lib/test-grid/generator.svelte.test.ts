import { describe, expect, it } from "vite-plus/test";
import type { BedSize } from "#lib/bindings/BedSize.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { addLayer, assignLayer, insert, remove } from "#lib/document/commands.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import { newLayer } from "#lib/document/layers.js";
import type { SceneObject } from "#lib/document/model.js";
import { Selection } from "#lib/document/selection.svelte.js";
import { translate } from "#lib/document/transform.js";
import { PREVIEW_NO_TEST_GRID, previewTestGrid, type TestGridAdapter } from "./adapter.js";
import { TestGridGenerator } from "./generator.svelte.js";
import {
  addTestGrid,
  defaultTestGridSettings,
  findTestGrid,
  type TestGrid,
  type TestGridSettings,
} from "./test-grid.js";

const BED: BedSize = { width: 400, height: 400 };

function rectOn(id: string, layerId: string, x = 0, y = 0): SceneObject {
  return {
    type: "rect",
    id,
    name: id,
    layerId,
    transform: translate(x, y),
    width: 10,
    height: 10,
    cornerRadius: 0,
  };
}

/** What `bf_core::material_test` makes, without the label outlines. */
function fakeGrid(id: string, settings: TestGridSettings): TestGrid {
  const values = ({ from, to, steps }: { from: number; to: number; steps: number }) =>
    Array.from({ length: steps }, (_, i) =>
      steps === 1 ? from : from + ((to - from) * i) / (steps - 1),
    );
  const speeds = values(settings.speed);
  const powers = values(settings.power);
  const labels = {
    ...newLayer(7, `${id}-labels`),
    name: "Test labels",
    speed: 3000,
    maxPower: 10,
  };
  const layers = [labels];
  const children: SceneObject[] = [];
  let labelCount = 0;
  const text = (value: string): SceneObject => ({
    type: "text",
    id: `${id}-label-${labelCount++}`,
    name: value,
    layerId: labels.id,
    transform: translate(0, 0),
    text: value,
    fontFamily: settings.labels.fontFamily,
    fontWeight: 400,
    italic: false,
    height: settings.labels.height,
    letterSpacing: 0,
    lineSpacing: 1,
    align: "left",
    outline: [],
  });
  speeds.forEach((speed, row) =>
    powers.forEach((power, column) => {
      const layerId = `${id}-r${row}-c${column}`;
      const name = `${speed} mm/min · ${power}%`;
      layers.push({ ...newLayer(row, layerId), name, mode: settings.mode, speed, maxPower: power });
      children.push({
        ...rectOn(`${id}-cell-r${row}-c${column}`, layerId, column * 12, -row * 12),
        name,
      });
    }),
  );
  for (const speed of speeds) children.push(text(String(speed)));
  for (const power of powers) children.push(text(String(power)));
  children.push(text("Power (%)"), text("Speed (mm/min)"));
  const group: SceneObject = {
    type: "group",
    id,
    name: "Material test",
    layerId: labels.id,
    transform: translate(0, 0),
    children,
  };
  return { layers, objects: [group], width: 80, height: 75, labelFont: settings.labels.fontFamily };
}

function setup(bed: BedSize = BED) {
  const editor = new DocumentEditor(new Lifecycle());
  const selection = new Selection(editor);
  const requests: { id: string; settings: TestGridSettings; bed: BedSize }[] = [];
  const adapter: TestGridAdapter = {
    available: true,
    generate: async (id, settings, size) => {
      requests.push({ id, settings, bed: size });
      if (settings.cellSize * settings.power.steps > size.width) {
        throw new Error("The test is too big for the bed");
      }
      return fakeGrid(id, settings);
    },
  };
  const generator = new TestGridGenerator(editor, selection, adapter, () => bed);
  return { editor, selection, generator, requests };
}

describe("TestGridGenerator", () => {
  it("adds a 5 × 5 grid as one undo step: a label layer, a layer per cell and labels", async () => {
    const { editor, selection, generator, requests } = setup();
    await generator.open("Geist");
    expect(requests[0].settings.labels.fontFamily).toBe("Geist");
    expect(requests[0].bed).toEqual(BED);
    expect(generator.grid?.layers).toHaveLength(26);

    expect(await generator.add()).toBe(true);
    const doc = editor.doc;
    expect(doc.layers).toHaveLength(26);
    expect(doc.layers[0].id).toMatch(/^test-[0-9a-f]{8}-labels$/);
    expect(doc.layers.slice(1).map((layer) => [layer.speed, layer.maxPower])).toContainEqual([
      2000, 60,
    ]);
    const [group] = doc.objects;
    if (group.type !== "group") throw new Error("expected the grid's group");
    const texts = group.children.flatMap((child) => (child.type === "text" ? [child] : []));
    expect(texts.map((text) => text.text)).toEqual([
      "1000",
      "1500",
      "2000",
      "2500",
      "3000",
      "20",
      "40",
      "60",
      "80",
      "100",
      "Power (%)",
      "Speed (mm/min)",
    ]);
    expect(texts.every((text) => text.layerId === doc.layers[0].id)).toBe(true);
    expect(selection.ids).toEqual([group.id]);
    expect(generator.view).toBe("pick");
    expect(generator.added).toBe(true);

    const cells = generator.cells;
    expect(cells?.id).toBe(group.id);
    expect([cells?.rows, cells?.columns]).toEqual([5, 5]);
    expect(cells?.cells[4][0]?.speed).toBe(3000);
    expect(cells?.cells[0][4]?.maxPower).toBe(100);

    editor.undo();
    expect(editor.doc.layers).toEqual([]);
    expect(editor.doc.objects).toEqual([]);
    expect(generator.cells).toBeNull();
  });

  it("gives each grid its own ids, and the picker the latest grid", async () => {
    const { editor, generator, requests } = setup();
    await generator.open();
    await generator.add();
    await generator.update({ speed: { from: 100, to: 300, steps: 3 } });
    await generator.add();
    const ids = [...new Set(requests.map((request) => request.id))];
    expect(ids).toHaveLength(2);
    expect(editor.doc.layers).toHaveLength(26 + 16);
    expect(generator.cells?.id).toBe(ids[1]);
    expect(generator.cells?.rows).toBe(3);
  });

  it("says why a grid can't be made and doesn't add it", async () => {
    const { editor, generator } = setup({ width: 30, height: 30 });
    await generator.open();
    expect(generator.grid).toBeNull();
    expect(generator.problem).toBe("The test is too big for the bed");
    expect(await generator.add()).toBe(false);
    expect(editor.doc.layers).toEqual([]);

    await generator.update({ cellSize: 5 });
    expect(generator.problem).toBe("");
    expect(generator.grid).not.toBeNull();
  });

  it("drops a slow layout for settings that have since changed", async () => {
    let release: () => void = () => {};
    const slow = new Promise<void>((resolve) => (release = resolve));
    const editor = new DocumentEditor(new Lifecycle());
    const generator = new TestGridGenerator(
      editor,
      new Selection(editor),
      {
        available: true,
        generate: async (id, settings) => {
          if (settings.mode === "fill") await slow;
          return fakeGrid(id, settings);
        },
      },
      () => BED,
    );
    const first = generator.refresh();
    await generator.update({ mode: "line" });
    release();
    await first;
    expect(generator.grid?.layers[1].mode).toBe("line");
  });

  it("resets to the defaults but keeps the label font", async () => {
    const { generator } = setup();
    await generator.open("Geist");
    await generator.update({ passes: 3 });
    await generator.reset();
    expect(generator.settings).toEqual(defaultTestGridSettings("Geist"));
  });

  it("is unavailable in the browser preview, and says so", async () => {
    const editor = new DocumentEditor(new Lifecycle());
    const preview = new TestGridGenerator(
      editor,
      new Selection(editor),
      previewTestGrid,
      () => BED,
    );
    expect(preview.available).toBe(false);
    await preview.open();
    expect(preview.problem).toBe(PREVIEW_NO_TEST_GRID);
  });
});

describe("findTestGrid", () => {
  it("leaves out cells with nothing on them, and layers that only look like cells", () => {
    const editor = new DocumentEditor(new Lifecycle());
    const settings = { ...defaultTestGridSettings(), speed: { from: 1, to: 2, steps: 2 } };
    const grid = fakeGrid("g", settings);
    editor.execute(addTestGrid(grid));
    // A layer named like a cell, but with no label layer beside it.
    editor.execute(addLayer(newLayer(3, "other-r0-c0")));
    editor.execute(insert([rectOn("x", "other-r0-c0")]));
    editor.execute(remove(["g-cell-r1-c2"]));

    const found = findTestGrid(editor.doc);
    expect(found?.id).toBe("g");
    expect([found?.rows, found?.columns]).toEqual([2, 5]);
    expect(found?.cells[1][2]).toBeNull();
    expect(found?.cells[1][3]?.id).toBe("g-r1-c3");

    // A cell moved onto another layer leaves its own empty.
    editor.execute(assignLayer(editor.doc, ["g-cell-r0-c0"], "g-labels"));
    expect(findTestGrid(editor.doc)?.cells[0][0]).toBeNull();
  });

  it("finds nothing in a design without a grid", () => {
    const editor = new DocumentEditor(new Lifecycle());
    editor.execute(addLayer(newLayer(0)));
    editor.execute(insert([rectOn("a", "layer-1")]));
    expect(findTestGrid(editor.doc)).toBeNull();
  });
});
