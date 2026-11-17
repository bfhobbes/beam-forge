import { describe, expect, it } from "vite-plus/test";
import { panelLayout, type LayoutStorage } from "./panel-layout.js";

function memory(
  initial: Record<string, string> = {},
): LayoutStorage & { data: Map<string, string> } {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
  };
}

describe("panel layout persistence", () => {
  it("uses the defaults the first time", () => {
    expect(panelLayout("main", [75, 25], memory()).sizes).toEqual([75, 25]);
  });

  it("restores saved sizes on the next launch", () => {
    const storage = memory();
    panelLayout("main", [75, 25], storage).save([60, 40]);
    expect(panelLayout("main", [75, 25], storage).sizes).toEqual([60, 40]);
  });

  it("keeps groups separate", () => {
    const storage = memory();
    panelLayout("main", [75, 25], storage).save([60, 40]);
    expect(panelLayout("other", [50, 50], storage).sizes).toEqual([50, 50]);
  });

  it.each([
    ["corrupt JSON", "{nope"],
    ["the wrong pane count", "[30, 30, 40]"],
    ["sizes that do not fill the group", "[10, 10]"],
    ["non-numbers", '["a", "b"]'],
    ["negative sizes", "[110, -10]"],
  ])("falls back to the defaults on %s", (_label, stored) => {
    const storage = memory({ "beam-forge:panels:main": stored });
    expect(panelLayout("main", [75, 25], storage).sizes).toEqual([75, 25]);
  });

  it("ignores invalid layouts on save", () => {
    const storage = memory();
    panelLayout("main", [75, 25], storage).save([10, 10]);
    expect(storage.data.size).toBe(0);
  });

  it("survives storage that throws", () => {
    const broken: LayoutStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("quota");
      },
    };
    const layout = panelLayout("main", [75, 25], broken);
    expect(layout.sizes).toEqual([75, 25]);
    expect(() => layout.save([60, 40])).not.toThrow();
  });

  it("works without storage at all", () => {
    const layout = panelLayout("main", [75, 25], null);
    expect(layout.sizes).toEqual([75, 25]);
    expect(() => layout.save([60, 40])).not.toThrow();
  });
});
