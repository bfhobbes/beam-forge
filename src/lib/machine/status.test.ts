import { describe, expect, it } from "vite-plus/test";
import { airAssist, formatPosition, isAlert, stateLabel } from "./status.js";

describe("machine status formatting", () => {
  it("labels every GRBL state, including sub-states", () => {
    expect(stateLabel({ name: "idle" })).toBe("Idle");
    expect(stateLabel({ name: "run" })).toBe("Running");
    expect(stateLabel({ name: "hold", code: 0 })).toBe("Hold · ready");
    expect(stateLabel({ name: "hold", code: 1 })).toBe("Hold · stopping");
    expect(stateLabel({ name: "door", code: 2 })).toBe("Door · parking");
    expect(stateLabel({ name: "door", code: 9 })).toBe("Door:9");
    expect(stateLabel({ name: "home" })).toBe("Homing");
    expect(stateLabel({ name: "check" })).toBe("Check mode");
  });

  it("flags alarm and door states", () => {
    expect(isAlert({ name: "alarm" })).toBe(true);
    expect(isAlert({ name: "door", code: 0 })).toBe(true);
    expect(isAlert({ name: "run" })).toBe(false);
  });

  it("formats positions in millimetres", () => {
    expect(formatPosition({ x: 12.5, y: -3, z: 0 })).toBe("X 12.50 Y -3.00 Z 0.00");
    expect(formatPosition(null)).toBe("—");
  });

  it("reads air assist from flood or mist", () => {
    const off = { spindleCw: false, spindleCcw: false, flood: false, mist: false };
    expect(airAssist(off)).toBe(false);
    expect(airAssist({ ...off, flood: true })).toBe(true);
    expect(airAssist({ ...off, mist: true })).toBe(true);
    expect(airAssist(null)).toBeNull();
  });
});
