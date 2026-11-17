import { describe, expect, it } from "vite-plus/test";
import type { BurnSegment } from "#lib/bindings/BurnSegment.js";
import {
  along,
  BurnModel,
  defaultOptions,
  gridStep,
  HIGHLIGHT,
  kind,
  modal,
  PASS_COLOURS,
  styleOf,
} from "./burn.js";

type Point = [number, number];

let clock = 0;
/** A segment starting where the clock is; speeds in mm/min. */
function seg(start: Point, end: Point, ms: number, fields: Partial<BurnSegment> = {}): BurnSegment {
  const t0 = clock;
  clock += ms;
  return {
    start,
    end,
    t0,
    t1: clock,
    startSpeed: 600,
    endSpeed: 600,
    feed: 600,
    power: 0,
    laser: "off",
    airAssist: false,
    rapid: false,
    stationary: false,
    line: 1,
    ...fields,
  };
}

/**
 * A small job as bf-sim traces it: a rapid to the start, an M4 square side
 * that ramps power with speed, a cruise at full power, an M3 dwell, and a
 * rapid home.
 */
function fixture(): BurnSegment[] {
  clock = 0;
  const m4 = { laser: "dynamic", airAssist: true, line: 5 } as const;
  return [
    seg([0, 0], [10, 10], 200, { rapid: true, feed: 6000, line: 4 }),
    seg([10, 10], [10.05, 10], 10, { ...m4, startSpeed: 0, endSpeed: 300, power: 0.25 }),
    seg([10.05, 10], [10.15, 10], 10, { ...m4, startSpeed: 300, endSpeed: 600, power: 0.75 }),
    seg([10.15, 10], [30, 10], 1985, { ...m4, power: 1 }),
    seg([30, 10], [30, 10], 500, {
      laser: "constant",
      power: 0.5,
      stationary: true,
      startSpeed: 0,
      endSpeed: 0,
      feed: 0,
      line: 6,
    }),
    seg([30, 10], [0, 0], 300, { rapid: true, feed: 6000, line: 8 }),
  ];
}

describe("drawing model", () => {
  it("styles cuts, travel and dwells from the trace, shading burns by power", () => {
    const job = fixture();
    expect(job.map(kind)).toEqual(["travel", "burn", "burn", "burn", "dwell", "travel"]);
    const styles = job.map((segment) => styleOf(segment, 0, defaultOptions));

    const travel = styles[0]!;
    expect(travel.dash.length).toBeGreaterThan(0);
    expect(travel.alpha).toBeLessThan(0.5);
    for (const burn of styles.slice(1, 4)) expect(burn!.dash).toEqual([]);

    // Darker with more power, following the trace's power fraction.
    const alphas = styles.slice(1, 4).map((style) => style!.alpha);
    expect(alphas[0]).toBeLessThan(alphas[1]);
    expect(alphas[1]).toBeLessThan(alphas[2]);
    expect(alphas[2]).toBe(1);
    expect(alphas[0]).toBeCloseTo(0.08 + 0.92 * 0.25);

    expect(styles[4]).toMatchObject({ kind: "dwell", alpha: 0.08 + 0.92 * 0.5 });
  });

  it("groups segments into one layer per style, travel first and dwells last", () => {
    const model = new BurnModel();
    model.append(fixture());
    const layers = model.layers(0, model.length, defaultOptions);
    expect(layers.map((layer) => [layer.style.kind, layer.indexes])).toEqual([
      ["travel", [0, 5]],
      ["burn", [1]],
      ["burn", [2]],
      ["burn", [3]],
      ["dwell", [4]],
    ]);
    const hidden = model.layers(0, model.length, { ...defaultOptions, showTravel: false });
    expect(hidden.some((layer) => layer.style.kind === "travel")).toBe(false);
  });

  it("numbers passes by how often a path has started at the same point", () => {
    clock = 0;
    const on = { power: 1, laser: "constant" } as const;
    const path = () => [seg([0, 0], [5, 0], 10, on), seg([5, 0], [5, 5], 10, on)];
    const back = () => seg([5, 5], [0, 0], 10, { rapid: true });
    const model = new BurnModel();
    model.append([...path(), back(), ...path(), back(), seg([9, 9], [9, 0], 10, on)]);
    expect(model.passes).toEqual([0, 0, 0, 1, 1, 1, 0]);
    const byPass = { ...defaultOptions, colouring: "pass" } as const;
    expect(styleOf(model.segments[3], model.passes[3], byPass)?.colour).toBe(PASS_COLOURS[1]);
  });

  it("indexes segments by the line that queued them", () => {
    const model = new BurnModel();
    model.append(fixture());
    expect(model.byLine.get(5)).toEqual([1, 2, 3]);
    expect(model.byLine.get(6)).toEqual([4]);
    expect(model.byLine.has(7)).toBe(false);
    expect(HIGHLIGHT).toMatch(/^#/);
  });
});

describe("playback", () => {
  it("finds the head mid-segment, accelerating evenly", () => {
    clock = 0;
    // 0 to 600 mm/min (10 mm/s) over 1 s covers 5 mm; half time is a quarter of that.
    const ramp = seg([0, 0], [5, 0], 1000, { startSpeed: 0, endSpeed: 600 });
    expect(along(ramp, 500)[0]).toBeCloseTo(1.25);
    expect(along(ramp, 1000)).toEqual([5, 0]);
    expect(along(ramp, -5)).toEqual([0, 0]);
  });

  it("reports the segment and modal state at a time", () => {
    const model = new BurnModel();
    model.append(fixture());
    expect(model.span()).toEqual([0, 3005]);
    expect(model.at(-1)).toBeNull();
    const cutting = model.at(1220)!;
    expect(cutting.running).toBe(true);
    expect(cutting.segment.line).toBe(5);
    expect(cutting.position[0]).toBeCloseTo(10.15 + 10);
    expect(modal(cutting.segment)).toEqual(["G1", "M4", "100% power", "F600", "M8 air"]);
    const done = model.at(10_000)!;
    expect(done.running).toBe(false);
    expect(done.position).toEqual([0, 0]);
    expect(model.finishedBy(219)).toBe(2);
    expect(model.finishedBy(220)).toBe(3);
  });

  it("finds the burn under a click and ignores travel", () => {
    const model = new BurnModel();
    model.append(fixture());
    expect(model.nearest([20, 10.2], 0.5, model.length)).toBe(3);
    expect(model.nearest([5, 5], 0.5, model.length)).toBeNull();
    // Not yet burnt at that point in playback.
    expect(model.nearest([20, 10.2], 0.5, 3)).toBeNull();
  });

  it("scrubs a 10k-segment job quickly", () => {
    clock = 0;
    const model = new BurnModel();
    model.append(
      Array.from({ length: 10_000 }, (_, n) =>
        seg([n % 100, Math.floor(n / 100)], [(n % 100) + 1, Math.floor(n / 100)], 10, {
          power: (n % 10) / 10,
          laser: "dynamic",
          line: n,
        }),
      ),
    );
    const started = performance.now();
    // Thirty scrub positions, each restyling everything up to the cursor.
    for (let step = 1; step <= 30; step++) {
      const time = (model.span()[1] * step) / 30;
      model.layers(0, model.finishedBy(time), defaultOptions);
      model.at(time);
    }
    const perFrame = (performance.now() - started) / 30;
    // A frame at 60 Hz is 16 ms; leave room for slow CI machines.
    expect(perFrame).toBeLessThan(50);
  });
});

describe("grid", () => {
  it("picks a step that keeps grid lines apart", () => {
    expect(gridStep(2, 8)).toBe(5);
    expect(gridStep(0.1, 8)).toBe(100);
  });
});
