import { describe, expect, it } from "vite-plus/test";
import type { Toolpath } from "#lib/bindings/gcode/Toolpath.js";
import type { Point } from "#lib/bindings/project/Point.js";
import { buildTimeline } from "#lib/toolpath/timeline.js";
import { followHead, takenMoves, timeAtHead, toDocument } from "./follow.js";
import { clampOverride, reportedOverride, sliderValue } from "./overrides.js";
import { atFeedRate } from "#lib/toolpath/fixtures.js";

const cut = (x: number, y: number) => ({
  kind: "cut" as const,
  to: { x, y },
  power: 50,
  speed: 600,
});

/** A 10 mm square from (10, 10), cut twice over: moves 1-4 and 5-8 trace the same edges. */
const twoPasses: Toolpath = atFeedRate(
  {
    start: { x: 0, y: 0 },
    layers: [
      {
        layerId: "a",
        airAssist: false,
        moves: [
          { kind: "rapid", to: { x: 10, y: 10 } },
          ...[1, 2].flatMap(() => [cut(20, 10), cut(20, 20), cut(10, 20), cut(10, 10)]),
        ],
      },
    ],
  },
  6000,
);
const timeline = buildTimeline(twoPasses);
const at = (x: number, y: number): Point => ({ x, y });

describe("following a running design", () => {
  it("maps the reported work position back onto the design", () => {
    const placed = {
      mirrorWidth: null,
      mirrorHeight: null,
      offset: { x: -5, y: 3 },
    };
    expect(toDocument(placed, { x: 5, y: 13 })).toEqual({ x: 10, y: 10 });
    // Origin rear right on a 400 × 300 bed.
    const mirrored = {
      mirrorWidth: 400,
      mirrorHeight: 300,
      offset: { x: 0, y: 0 },
    };
    expect(toDocument(mirrored, { x: 390, y: 280 })).toEqual({ x: 10, y: 20 });
  });

  it("counts the moves whose lines the controller took", () => {
    const lines = [4, 5, 6, 9];
    expect(takenMoves(lines, 0)).toBe(0);
    expect(takenMoves(lines, 4)).toBe(1);
    expect(takenMoves(lines, 8)).toBe(3);
    expect(takenMoves(lines, 99)).toBe(4);
  });

  it("finds the move the head is on, no further than the controller has taken", () => {
    expect(followHead(timeline, 9, at(0, 0), 0)).toBe(0);
    expect(followHead(timeline, 9, at(15, 10), 0)).toBe(1);
    // At a corner, the next move has begun.
    expect(followHead(timeline, 9, at(20, 10), 1)).toBe(2);
    // A move the controller hasn't taken can't be under way.
    expect(followHead(timeline, 2, at(20, 15), 1)).toBe(1);
  });

  it("never jumps to a later pass over the same edge, nor goes back", () => {
    // Second pass along the bottom edge: the first pass's move 1 matches too.
    expect(followHead(timeline, 9, at(15, 10), 5)).toBe(5);
    expect(followHead(timeline, 9, at(15, 10), 3)).toBe(5);
    // Off the path (a stale report) or unknown: stay.
    expect(followHead(timeline, 9, at(50, 50), 6)).toBe(6);
    expect(followHead(timeline, 9, null, 6)).toBe(6);
  });

  it("places the head on the timeline for the time left", () => {
    const rapid = Math.hypot(10, 10) / 100;
    expect(timeAtHead(timeline, 0, null)).toBe(0);
    expect(timeAtHead(timeline, 1, at(15, 10))).toBeCloseTo(rapid + 0.5);
    expect(timeAtHead(timeline, 2, null)).toBeCloseTo(rapid + 1);
    expect(timeAtHead(timeline, timeline.count, null)).toBe(timeline.duration);
  });
});

describe("override sliders", () => {
  it("clamps to GRBL's limits in whole percent", () => {
    expect(clampOverride(5)).toBe(10);
    expect(clampOverride(137.6)).toBe(138);
    expect(clampOverride(999)).toBe(200);
  });

  it("show the target until the machine reports it", () => {
    const overrides = { feed: 120, rapid: 100, spindle: 80 };
    expect(reportedOverride(overrides, "feed")).toBe(120);
    expect(reportedOverride(overrides, "spindle")).toBe(80);
    expect(reportedOverride(null, "feed")).toBeNull();
    expect(sliderValue(150, 120)).toBe(150);
    expect(sliderValue(120, 120)).toBe(120);
    expect(sliderValue(undefined, 80)).toBe(80);
    expect(sliderValue(undefined, null)).toBe(100);
  });
});
