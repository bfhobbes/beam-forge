import { describe, expect, it } from "vite-plus/test";
import { alarms, errors, wholeNumber } from "./faults.js";

describe("fault codes", () => {
  it("lists every GRBL 1.1 alarm and error bf-sim accepts", () => {
    expect(alarms.map((alarm) => alarm.code)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    const codes = errors.map((error) => error.code);
    expect(codes).toHaveLength(36);
    expect(codes).not.toContain(18);
    expect(codes).not.toContain(19);
    expect(Math.max(...codes)).toBe(38);
  });
});

describe("wholeNumber", () => {
  it("accepts whole numbers only", () => {
    expect(wholeNumber(" 250 ")).toBe(250);
    expect(wholeNumber("0")).toBe(0);
    expect(wholeNumber("2.5")).toBeNull();
    expect(wholeNumber("-1")).toBeNull();
    expect(wholeNumber("")).toBeNull();
  });
});
