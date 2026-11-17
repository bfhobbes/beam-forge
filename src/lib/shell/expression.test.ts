import { describe, expect, it } from "vite-plus/test";
import { evaluate } from "./expression.js";

describe("numeric field expressions", () => {
  it("reads plain numbers", () => {
    expect(evaluate("12")).toBe(12);
    expect(evaluate(" 12.5 ")).toBe(12.5);
    expect(evaluate(".5")).toBe(0.5);
    expect(evaluate("-3")).toBe(-3);
  });

  it("evaluates arithmetic with precedence and parentheses", () => {
    expect(evaluate("120/2")).toBe(60);
    expect(evaluate("10 + 4 * 2")).toBe(18);
    expect(evaluate("(10 + 4) * 2")).toBe(28);
    expect(evaluate("10 - 2 - 3")).toBe(5);
    expect(evaluate("100 / 4 / 5")).toBe(5);
    expect(evaluate("-(2 + 3) * -2")).toBe(10);
    expect(evaluate("2 * -3")).toBe(-6);
  });

  it("rejects anything else", () => {
    expect(evaluate("")).toBeNull();
    expect(evaluate("abc")).toBeNull();
    expect(evaluate("2 +")).toBeNull();
    expect(evaluate("(2")).toBeNull();
    expect(evaluate("2)")).toBeNull();
    expect(evaluate("1 2")).toBeNull();
    expect(evaluate("1/0")).toBeNull();
    expect(evaluate("1..2")).toBeNull();
  });
});
