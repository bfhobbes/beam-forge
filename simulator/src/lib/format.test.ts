import { describe, expect, it } from "vite-plus/test";
import { clock, coordinate, parsePort, stamp } from "./format.js";

describe("format", () => {
  it("prints coordinates like a GRBL status report", () => {
    expect(coordinate(12.5)).toBe("12.500");
    expect(coordinate(-0)).toBe("0.000");
  });

  it("prints the simulated clock", () => {
    expect(clock(0)).toBe("0:00:00.0");
    expect(clock(3_723_450)).toBe("1:02:03.4");
  });

  it("stamps log entries to the millisecond", () => {
    expect(stamp(0)).toBe("0:00:00.000");
    expect(stamp(3_723_456.7)).toBe("1:02:03.456");
  });

  it("accepts only valid TCP ports", () => {
    expect(parsePort("23232")).toBe(23232);
    expect(parsePort(" 80 ")).toBe(80);
    for (const bad of ["", "0", "65536", "12a", "-1", "1.5"]) expect(parsePort(bad)).toBeNull();
  });
});
