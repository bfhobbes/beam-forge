// The burn view's model: what each trace segment looks like, which pass it
// belongs to, and where the head is at any moment of playback. Pure, so the
// styling is testable without a canvas; `burn-view.svelte` only paints it.

import type { Bed } from "#lib/bindings/Bed.js";
import type { BurnSegment } from "#lib/bindings/BurnSegment.js";

/** Travel: laser off. Burn: moving with the laser on. Dwell: on, head still. */
export type Kind = "travel" | "burn" | "dwell";

export function kind(segment: BurnSegment): Kind {
  if (segment.stationary) return "dwell";
  return segment.power > 0 ? "burn" : "travel";
}

export type Colouring = "power" | "pass";

export interface DrawOptions {
  readonly showTravel: boolean;
  readonly colouring: Colouring;
}

export const defaultOptions: DrawOptions = { showTravel: true, colouring: "power" };

/** How a group of segments is stroked. Widths are in screen pixels. */
export interface Style {
  readonly kind: Kind;
  readonly colour: string;
  readonly alpha: number;
  readonly width: number;
  readonly dash: readonly number[];
}

/** Shades of power: enough to see M4 ramps, few enough to batch strokes. */
export const POWER_LEVELS = 32;
/** Scorch colour on the bed's material. */
const SCORCH = "#2b1608";
const TRAVEL = "#2563eb";
/** Distinct hues for pass 1, 2, 3... repeating. */
export const PASS_COLOURS = ["#1f2937", "#dc2626", "#2563eb", "#16a34a", "#9333ea", "#ea580c"];
export const HIGHLIGHT = "#f59e0b";

const level = (power: number) =>
  Math.round(Math.min(1, Math.max(0, power)) * POWER_LEVELS) / POWER_LEVELS;

/**
 * The style a segment is drawn with, or null if it is hidden. In power
 * colouring a burn's opacity is its power fraction, so `M4` ramps fade in
 * and out with speed exactly as the trace says.
 */
export function styleOf(segment: BurnSegment, pass: number, options: DrawOptions): Style | null {
  const k = kind(segment);
  if (k === "travel") {
    return options.showTravel
      ? { kind: k, colour: TRAVEL, alpha: 0.35, width: 1, dash: [4, 4] }
      : null;
  }
  const byPass = options.colouring === "pass";
  const colour = byPass ? PASS_COLOURS[pass % PASS_COLOURS.length] : SCORCH;
  // Faint but visible even at the lowest power.
  const alpha = byPass ? 0.9 : 0.08 + 0.92 * level(segment.power);
  return k === "dwell"
    ? { kind: k, colour, alpha, width: 0, dash: [] }
    : { kind: k, colour, alpha, width: 1.5, dash: [] };
}

export const styleKey = (style: Style) =>
  `${style.kind}|${style.colour}|${style.alpha}|${style.width}|${style.dash.join(",")}`;

export interface Layer {
  readonly style: Style;
  /** Segment indexes, in trace order. */
  readonly indexes: number[];
}

/** Rounds a point to 0.01 mm so a repeated path starts at the "same" place. */
const pointKey = ([x, y]: readonly [number, number]) =>
  `${Math.round(x * 100)},${Math.round(y * 100)}`;

/**
 * The recorded trace, with what the view needs precomputed as segments
 * arrive: each segment's pass and the segments each line queued.
 *
 * A path is a run of laser-on segments between travel moves. A job that
 * repeats a path starts the repeat where the first started, so a path's
 * pass is how many earlier paths started at the same point, plus one.
 */
export class BurnModel {
  readonly segments: BurnSegment[] = [];
  /** 0-based pass of each segment; travel takes the pass of the path before. */
  readonly passes: number[] = [];
  readonly byLine = new Map<number, number[]>();
  private starts = new Map<string, number>();
  private pass = 0;
  private burning = false;

  append(segments: readonly BurnSegment[]): void {
    for (const segment of segments) {
      const on = kind(segment) !== "travel";
      if (on && !this.burning) {
        const key = pointKey(segment.start);
        this.pass = this.starts.get(key) ?? 0;
        this.starts.set(key, this.pass + 1);
      }
      this.burning = on;
      const index = this.segments.push(segment) - 1;
      this.passes.push(this.pass);
      const indexes = this.byLine.get(segment.line);
      if (indexes) indexes.push(index);
      else this.byLine.set(segment.line, [index]);
    }
  }

  get length(): number {
    return this.segments.length;
  }

  /** When the trace starts and ends, in app clock ms; [0, 0] when empty. */
  span(): [number, number] {
    return [this.segments[0]?.t0 ?? 0, this.segments.at(-1)?.t1 ?? 0];
  }

  /** Segments from `from` up to `to`, grouped by style so each group is one stroke. */
  layers(from: number, to: number, options: DrawOptions): Layer[] {
    const groups = new Map<string, Layer>();
    for (let index = from; index < to; index++) {
      const style = styleOf(this.segments[index], this.passes[index], options);
      if (!style) continue;
      const key = styleKey(style);
      const group = groups.get(key);
      if (group) group.indexes.push(index);
      else groups.set(key, { style, indexes: [index] });
    }
    // Travel underneath, dwells on top.
    const order: Record<Kind, number> = { travel: 0, burn: 1, dwell: 2 };
    return [...groups.values()].sort((a, b) => order[a.style.kind] - order[b.style.kind]);
  }

  /** How many segments have finished by `time`. */
  finishedBy(time: number): number {
    let low = 0;
    let high = this.segments.length;
    while (low < high) {
      const mid = (low + high) >>> 1;
      if (this.segments[mid].t1 <= time) low = mid + 1;
      else high = mid;
    }
    return low;
  }

  /** The head at `time`: where it is, and the segment it is on or last finished. */
  at(time: number): Cursor | null {
    const finished = this.finishedBy(time);
    const running = this.segments[finished];
    if (running && running.t0 < time) {
      return {
        index: finished,
        segment: running,
        position: along(running, time),
        running: true,
      };
    }
    const last = this.segments[finished - 1];
    if (!last) return null;
    return { index: finished - 1, segment: last, position: last.end, running: false };
  }

  /** Bounds of the trace and bed together, so a fit shows both. */
  bounds(bed: Bed | null): Bounds | null {
    let box: Bounds | null = bed
      ? {
          min: [bed.origin[0], bed.origin[1]],
          max: [bed.origin[0] + bed.size[0], bed.origin[1] + bed.size[1]],
        }
      : null;
    for (const { start, end } of this.segments) {
      for (const [x, y] of [start, end]) {
        box = box
          ? {
              min: [Math.min(box.min[0], x), Math.min(box.min[1], y)],
              max: [Math.max(box.max[0], x), Math.max(box.max[1], y)],
            }
          : { min: [x, y], max: [x, y] };
      }
    }
    return box;
  }

  /** The nearest segment to a point within `radius` mm, if any is drawn there. */
  nearest(point: readonly [number, number], radius: number, upTo: number): number | null {
    let best: number | null = null;
    let bestDistance = radius;
    for (let index = 0; index < Math.min(upTo, this.segments.length); index++) {
      const segment = this.segments[index];
      if (kind(segment) === "travel") continue;
      const distance = distanceToSegment(point, segment.start, segment.end);
      if (distance <= bestDistance) {
        best = index;
        bestDistance = distance;
      }
    }
    return best;
  }
}

export interface Bounds {
  readonly min: [number, number];
  readonly max: [number, number];
}

export interface Cursor {
  readonly index: number;
  readonly segment: BurnSegment;
  readonly position: readonly [number, number];
  /** The head is partway along `segment`, rather than stopped after it. */
  readonly running: boolean;
}

/**
 * Where the head is on a segment at `time`. Speed changes evenly from
 * `startSpeed` to `endSpeed`, so distance grows quadratically.
 */
export function along(segment: BurnSegment, time: number): [number, number] {
  const total = (segment.t1 - segment.t0) / 1000;
  const elapsed = Math.min(Math.max(0, (time - segment.t0) / 1000), total);
  const v0 = segment.startSpeed / 60;
  const v1 = segment.endSpeed / 60;
  const distance = ((v0 + v1) / 2) * total;
  const travelled = total > 0 ? v0 * elapsed + (((v1 - v0) / total) * elapsed * elapsed) / 2 : 0;
  const fraction = distance > 0 ? travelled / distance : total > 0 ? elapsed / total : 1;
  const [x0, y0] = segment.start;
  const [x1, y1] = segment.end;
  return [x0 + (x1 - x0) * fraction, y0 + (y1 - y0) * fraction];
}

/** The part of a segment the head has covered by `time`: the one running now. */
export function partial(segment: BurnSegment, time: number): BurnSegment {
  return { ...segment, end: along(segment, time) };
}

function distanceToSegment(
  [px, py]: readonly [number, number],
  [x0, y0]: readonly [number, number],
  [x1, y1]: readonly [number, number],
): number {
  const dx = x1 - x0;
  const dy = y1 - y0;
  const lengthSquared = dx * dx + dy * dy;
  const t =
    lengthSquared > 0
      ? Math.min(1, Math.max(0, ((px - x0) * dx + (py - y0) * dy) / lengthSquared))
      : 0;
  return Math.hypot(px - (x0 + t * dx), py - (y0 + t * dy));
}

/** The modal state a segment ran under, as G-code words. */
export function modal(segment: BurnSegment): string[] {
  const words = [segment.stationary ? "Dwell" : segment.rapid ? "G0" : "G1"];
  words.push(segment.laser === "dynamic" ? "M4" : segment.laser === "constant" ? "M3" : "M5");
  words.push(`${Math.round(segment.power * 100)}% power`);
  if (!segment.stationary) words.push(`F${Math.round(segment.feed)}`);
  words.push(segment.airAssist ? "M8 air" : "M9");
  return words;
}

/** A grid step in mm that leaves at least `minPixels` between lines. */
export function gridStep(pixelsPerMm: number, minPixels: number): number {
  for (const step of [1, 5, 10, 50, 100, 500, 1000]) {
    if (step * pixelsPerMm >= minPixels) return step;
  }
  return 1000;
}
