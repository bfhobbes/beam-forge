import { describe, expect, it } from "vite-plus/test";
import { ViewPrefs, type PrefsStorage } from "./view-prefs.svelte.js";

const memory = (initial: Record<string, string> = {}): PrefsStorage & { data: typeof initial } => {
  const data = { ...initial };
  return {
    data,
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => void (data[key] = value),
  };
};

describe("view prefs", () => {
  it("shows fills by default", () => {
    expect(new ViewPrefs(memory()).showFills).toBe(true);
    expect(new ViewPrefs().showFills).toBe(true);
  });

  it("remembers turning fills off across launches", () => {
    const storage = memory();
    new ViewPrefs(storage).setShowFills(false);
    expect(new ViewPrefs(storage).showFills).toBe(false);
  });

  it("falls back to the default when storage throws", () => {
    const broken: PrefsStorage = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    const prefs = new ViewPrefs(broken);
    expect(prefs.showFills).toBe(true);
    prefs.setShowFills(false);
    expect(prefs.showFills).toBe(false);
  });
});
