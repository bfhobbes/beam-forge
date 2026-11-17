/**
 * The toolpath as a timeline: every move with when it ends, in flat typed
 * arrays, so finding where the head is at any moment is a binary search.
 * Scrubbing a 10k-move job is then a ~14-step lookup per frame.
 *
 * Times are the desktop app's estimate (`Toolpath.moveSeconds`), from the
 * machine profile's rates and accelerations, so the playback clock runs at
 * the machine's pace.
 */
import type { Toolpath } from "#lib/bindings/gcode/Toolpath.js";
import type { Point } from "#lib/bindings/project/Point.js";

/** One layer's run of moves. */
export interface TimelineLayer {
  layerId: string;
  /** Index of its first move. */
  first: number;
  /** One past its last move. */
  end: number;
  /** Seconds into the job it starts. */
  start: number;
}

export interface Timeline {
  /** Where the head is before the first move. */
  start: Point;
  /** Number of moves. */
  count: number;
  /** Where each move ends, mm. */
  x: Float64Array;
  y: Float64Array;
  /** Seconds into the job each move ends; never decreasing. */
  ends: Float64Array;
  /** Cut power in percent, or -1 for a rapid. */
  power: Float64Array;
  /** Index into `layers` of each move's layer. */
  layer: Uint32Array;
  layers: TimelineLayer[];
  /** Seconds for the whole job. */
  duration: number;
}

/** Where the head is at a moment. */
export interface PlaybackPoint {
  /** Seconds into the job, clamped to it. */
  time: number;
  /** Moves finished: moves `0..completed` are done; move `completed` is in progress (if any). */
  completed: number;
  /** The head, mm. */
  head: Point;
  /** Index into the timeline's layers, or -1 before the first move. */
  layer: number;
}

export function buildTimeline(toolpath: Toolpath): Timeline {
  const { plan } = toolpath;
  const count = plan.layers.reduce((sum, layer) => sum + layer.moves.length, 0);
  const x = new Float64Array(count);
  const y = new Float64Array(count);
  const ends = new Float64Array(count);
  const power = new Float64Array(count);
  const layer = new Uint32Array(count);
  const layers: TimelineLayer[] = [];
  let time = 0;
  let index = 0;
  for (const [layerIndex, planned] of plan.layers.entries()) {
    layers.push({ layerId: planned.layerId, first: index, end: index, start: time });
    for (const step of planned.moves) {
      time += Math.max(0, toolpath.moveSeconds[index] ?? 0);
      x[index] = step.to.x;
      y[index] = step.to.y;
      ends[index] = time;
      power[index] = step.kind === "cut" ? step.power : -1;
      layer[index] = layerIndex;
      index++;
    }
    layers[layerIndex]!.end = index;
  }
  return { start: plan.start, count, x, y, ends, power, layer, layers, duration: time };
}

/** Seconds layer `index` takes, every pass included. */
export function layerDuration(timeline: Timeline, index: number): number {
  const layer = timeline.layers[index];
  if (!layer) return 0;
  return (timeline.layers[index + 1]?.start ?? timeline.duration) - layer.start;
}

/** The first move still in progress at `time`: the first that ends after it. `count` once all are done. */
export function moveAt(timeline: Timeline, time: number): number {
  const { ends, count } = timeline;
  let low = 0;
  let high = count;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (ends[mid]! <= time) low = mid + 1;
    else high = mid;
  }
  return low;
}

/** Where move `index` starts. */
export function moveStart(timeline: Timeline, index: number): Point {
  if (index <= 0) return timeline.start;
  return { x: timeline.x[index - 1]!, y: timeline.y[index - 1]! };
}

/** Where the head is `time` seconds into the job. */
export function pointAt(timeline: Timeline, time: number): PlaybackPoint {
  const clamped = Math.min(timeline.duration, Math.max(0, time));
  const index = moveAt(timeline, clamped);
  if (index >= timeline.count) {
    const last = timeline.count - 1;
    return {
      time: clamped,
      completed: timeline.count,
      head: moveStart(timeline, timeline.count),
      layer: last >= 0 ? timeline.layer[last]! : -1,
    };
  }
  const from = moveStart(timeline, index);
  const began = index > 0 ? timeline.ends[index - 1]! : 0;
  const span = timeline.ends[index]! - began;
  const t = span > 0 ? (clamped - began) / span : 1;
  return {
    time: clamped,
    completed: index,
    head: {
      x: from.x + (timeline.x[index]! - from.x) * t,
      y: from.y + (timeline.y[index]! - from.y) * t,
    },
    layer: timeline.layer[index]!,
  };
}

/**
 * Where the head is once `completed` moves are done, for following a running
 * job by acknowledged move rather than by clock.
 */
export function pointAfter(timeline: Timeline, completed: number): PlaybackPoint {
  const done = Math.min(timeline.count, Math.max(0, Math.floor(completed)));
  const layerOf = Math.min(done, timeline.count - 1);
  return {
    time: done > 0 ? timeline.ends[done - 1]! : 0,
    completed: done,
    head: moveStart(timeline, done),
    layer: layerOf >= 0 ? timeline.layer[layerOf]! : -1,
  };
}

/** `m:ss`, or `h:mm:ss` from an hour. */
export function clock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  const s = String(whole % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
