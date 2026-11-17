import { describe, expect, it } from "vite-plus/test";
import type { Toolpath } from "#lib/bindings/gcode/Toolpath.js";
import type { Move } from "#lib/bindings/plan/Move.js";
import { buildTimeline, clock, layerDuration, moveAt, pointAfter, pointAt } from "./timeline.js";
import { atFeedRate } from "./fixtures.js";

const cut = (x: number, y: number, speed = 600, power = 50): Move => ({
  kind: "cut",
  to: { x, y },
  power,
  speed,
});
const rapid = (x: number, y: number): Move => ({ kind: "rapid", to: { x, y } });

/** A 10 mm rapid at 6000 mm/min (0.1 s), then a 10 mm square at 600 mm/min (1 s per side). */
const square: Toolpath = atFeedRate(
  {
    start: { x: 0, y: 0 },
    layers: [
      {
        layerId: "a",
        airAssist: true,
        moves: [rapid(10, 0), cut(20, 0), cut(20, 10)],
      },
      {
        layerId: "b",
        airAssist: false,
        moves: [cut(10, 10, 300, 100), cut(10, 0, 300, 100)],
      },
    ],
  },
  6000,
);

/** Zig-zag cuts with a rapid every 100 moves: `count` moves over three layers. */
function bigToolpath(count: number): Toolpath {
  const moves: Move[] = [];
  for (let i = 0; i < count; i++) {
    const x = (i % 100) * 2;
    const y = Math.floor(i / 100) * 2 + (i % 2);
    moves.push(i % 100 === 0 ? rapid(x, y) : cut(x, y, 1000 + (i % 7) * 100, i % 101));
  }
  const third = Math.ceil(count / 3);
  return atFeedRate(
    {
      start: { x: 0, y: 0 },
      layers: [0, 1, 2].map((n) => ({
        layerId: `layer-${n}`,
        airAssist: false,
        moves: moves.slice(n * third, (n + 1) * third),
      })),
    },
    5000,
  );
}

describe("toolpath timeline", () => {
  it("times each move as the toolpath's estimate says", () => {
    const timeline = buildTimeline(square);
    expect(timeline.count).toBe(5);
    expect([...timeline.ends]).toEqual([
      expect.closeTo(0.1),
      expect.closeTo(1.1),
      expect.closeTo(2.1),
      expect.closeTo(4.1),
      expect.closeTo(6.1),
    ]);
    expect(timeline.duration).toBeCloseTo(6.1);
    expect([...timeline.power]).toEqual([-1, 50, 50, 100, 100]);
    expect(timeline.layers).toEqual([
      { layerId: "a", first: 0, end: 3, start: 0 },
      { layerId: "b", first: 3, end: 5, start: expect.closeTo(2.1) },
    ]);
  });

  it("uses the estimate's move times, which allow for acceleration", () => {
    const timeline = buildTimeline({ ...square, moveSeconds: [1, 2, 3, 4, 5] });
    expect([...timeline.ends]).toEqual([1, 3, 6, 10, 15]);
    expect(timeline.duration).toBe(15);
  });

  it("times each layer", () => {
    const timeline = buildTimeline(square);
    expect(layerDuration(timeline, 0)).toBeCloseTo(2.1);
    expect(layerDuration(timeline, 1)).toBeCloseTo(4);
    expect(layerDuration(timeline, 2)).toBe(0);
  });

  it("finds the head, the move in progress and the layer at any time", () => {
    const timeline = buildTimeline(square);
    expect(pointAt(timeline, -1)).toEqual({
      time: 0,
      completed: 0,
      head: { x: 0, y: 0 },
      layer: 0,
    });
    const halfway = pointAt(timeline, 0.6);
    expect(halfway.completed).toBe(1);
    expect(halfway.head.x).toBeCloseTo(15);
    expect(halfway.head.y).toBeCloseTo(0);
    expect(halfway.layer).toBe(0);
    const second = pointAt(timeline, 3.1);
    expect(second.layer).toBe(1);
    expect(second.head.x).toBeCloseTo(15);
    expect(second.head.y).toBeCloseTo(10);
    expect(pointAt(timeline, 99)).toEqual({
      time: timeline.duration,
      completed: 5,
      head: { x: 10, y: 0 },
      layer: 1,
    });
  });

  it("follows completed moves for a running job", () => {
    const timeline = buildTimeline(square);
    expect(pointAfter(timeline, 0).head).toEqual({ x: 0, y: 0 });
    const after = pointAfter(timeline, 3);
    expect(after.head).toEqual({ x: 20, y: 10 });
    expect(after.layer).toBe(1);
    expect(after.time).toBeCloseTo(2.1);
    expect(pointAfter(timeline, 100).completed).toBe(5);
  });

  it("handles an empty plan", () => {
    const timeline = buildTimeline(atFeedRate({ start: { x: 1, y: 2 }, layers: [] }, 1000));
    expect(timeline.duration).toBe(0);
    expect(pointAt(timeline, 5)).toEqual({
      time: 0,
      completed: 0,
      head: { x: 1, y: 2 },
      layer: -1,
    });
  });

  it("scrubs a 10k-move job in well under a frame", () => {
    const toolpath = bigToolpath(10_000);
    const built = performance.now();
    const timeline = buildTimeline(toolpath);
    const buildMs = performance.now() - built;
    expect(timeline.count).toBe(10_000);

    // Agrees with a linear scan everywhere.
    for (let i = 0; i < 500; i++) {
      const time = (i / 499) * timeline.duration;
      const linear = timeline.ends.findIndex((end) => end > time);
      expect(moveAt(timeline, time)).toBe(linear === -1 ? timeline.count : linear);
    }

    // A drag across the whole scrubber: 10k positions, each a binary search.
    const scrubbed = performance.now();
    let checksum = 0;
    for (let i = 0; i <= 10_000; i++) {
      checksum += pointAt(timeline, (i / 10_000) * timeline.duration).completed;
    }
    const scrubMs = performance.now() - scrubbed;
    expect(checksum).toBeGreaterThan(0);
    // One frame is 16 ms. Building happens once per re-plan, lookups once
    // per frame; generous budgets keep this stable on a busy CI runner.
    expect(buildMs).toBeLessThan(50);
    expect(scrubMs).toBeLessThan(50);
  });

  it("formats clock times", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(61.9)).toBe("1:01");
    expect(clock(3725)).toBe("1:02:05");
  });
});
