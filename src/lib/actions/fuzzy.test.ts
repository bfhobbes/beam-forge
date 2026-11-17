import { describe, expect, it } from "vite-plus/test";
import { fuzzyScore } from "./fuzzy.js";

describe("fuzzy search", () => {
  it("matches characters in order, ignoring case", () => {
    expect(fuzzyScore("rct", "Rectangle")).not.toBeNull();
    expect(fuzzyScore("RECT", "rectangle")).not.toBeNull();
    expect(fuzzyScore("tcr", "Rectangle")).toBeNull();
  });

  it("matches everything with an empty query", () => {
    expect(fuzzyScore("", "Anything")).toBe(0);
    expect(fuzzyScore("  ", "Anything")).toBe(0);
  });

  it("ranks prefixes and word starts above scattered letters", () => {
    const prefix = fuzzyScore("up", "Updates")!;
    const wordStart = fuzzyScore("up", "Check for updates")!;
    const scattered = fuzzyScore("up", "Group shapes")!;
    expect(prefix).toBeGreaterThan(wordStart);
    expect(wordStart).toBeGreaterThan(scattered);
  });

  it("ranks consecutive runs above gaps", () => {
    expect(fuzzyScore("ell", "Ellipse")!).toBeGreaterThan(fuzzyScore("ell", "Equal line")!);
  });

  it("matches across words in any order of terms", () => {
    expect(fuzzyScore("updates check", "Check for updates")).not.toBeNull();
    expect(fuzzyScore("updates frame", "Check for updates")).toBeNull();
  });
});
