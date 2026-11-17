/**
 * Paints the toolpath over the bed: cuts in their layer's colour, more opaque
 * the higher the power; rapids and other laser-off moves as faint dashed lines; the laser head as a
 * marker. What's still to come is drawn as a faint ghost under what's done.
 */
import type { View } from "#lib/canvas/view.js";
import { moveStart, type PlaybackPoint, type Timeline } from "./timeline.js";

export interface ToolpathDrawing {
  timeline: Timeline;
  /** CSS colour of each of the timeline's layers. */
  colors: readonly string[];
  progress: PlaybackPoint;
}

export interface ToolpathTheme {
  rapid: string;
  head: string;
}

/** Power is shown in steps of this many percent. */
const POWER_STEP = 10;
const STEPS = 100 / POWER_STEP + 1;

/**
 * A batch: every move that strokes the same way. Rapids are `-1`, and so are
 * moves at power 0 (a fill's overscan and gap crossings): the laser is off.
 */
function batchOf(timeline: Timeline, index: number): number {
  const power = timeline.power[index]!;
  if (power <= 0) return -1;
  const step = Math.round(Math.min(100, Math.max(0, power)) / POWER_STEP);
  return timeline.layer[index]! * STEPS + step;
}

/** Opacity for a power step: faint at low power, solid at full. */
const powerAlpha = (batch: number) => 0.3 + 0.7 * ((batch % STEPS) / (STEPS - 1));

/** Moves `from..to` as one path per batch, joining moves that continue each other. */
function tracePaths(timeline: Timeline, from: number, to: number): Map<number, Path2D> {
  const paths = new Map<number, Path2D>();
  const lastIndex = new Map<number, number>();
  for (let index = from; index < to; index++) {
    const batch = batchOf(timeline, index);
    let path = paths.get(batch);
    if (!path) paths.set(batch, (path = new Path2D()));
    if (lastIndex.get(batch) !== index - 1) {
      const start = moveStart(timeline, index);
      path.moveTo(start.x, start.y);
    }
    path.lineTo(timeline.x[index]!, timeline.y[index]!);
    lastIndex.set(batch, index);
  }
  return paths;
}

const ghosts = new WeakMap<Timeline, Map<number, Path2D>>();

/** The whole job, traced once per timeline. */
function ghostPaths(timeline: Timeline): Map<number, Path2D> {
  let paths = ghosts.get(timeline);
  if (!paths) ghosts.set(timeline, (paths = tracePaths(timeline, 0, timeline.count)));
  return paths;
}

function strokeBatches(
  ctx: CanvasRenderingContext2D,
  paths: Map<number, Path2D>,
  drawing: ToolpathDrawing,
  theme: ToolpathTheme,
  zoom: number,
  dim: number,
): void {
  for (const [batch, path] of paths) {
    if (batch < 0) {
      ctx.strokeStyle = theme.rapid;
      ctx.globalAlpha = 0.5 * dim;
      ctx.lineWidth = 1 / zoom;
      ctx.setLineDash([4 / zoom, 4 / zoom]);
    } else {
      const layer = Math.floor(batch / STEPS);
      ctx.strokeStyle = drawing.colors[layer] ?? theme.rapid;
      ctx.globalAlpha = powerAlpha(batch) * dim;
      ctx.lineWidth = 1.5 / zoom;
      ctx.setLineDash([]);
    }
    ctx.stroke(path);
  }
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;
}

/** Paints the toolpath in world space, then the head marker. */
export function drawToolpath(
  ctx: CanvasRenderingContext2D,
  view: View,
  dpr: number,
  drawing: ToolpathDrawing,
  theme: ToolpathTheme,
): void {
  const { timeline, progress } = drawing;
  ctx.setTransform(dpr * view.zoom, 0, 0, -dpr * view.zoom, dpr * view.x, dpr * view.y);
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  const done = Math.min(progress.completed, timeline.count);
  if (done < timeline.count)
    strokeBatches(ctx, ghostPaths(timeline), drawing, theme, view.zoom, 0.25);
  const finished = done === timeline.count ? ghostPaths(timeline) : tracePaths(timeline, 0, done);
  strokeBatches(ctx, finished, drawing, theme, view.zoom, 1);

  // The move in progress, up to the head.
  if (done < timeline.count) {
    const start = moveStart(timeline, done);
    const partial = new Path2D();
    partial.moveTo(start.x, start.y);
    partial.lineTo(progress.head.x, progress.head.y);
    strokeBatches(ctx, new Map([[batchOf(timeline, done), partial]]), drawing, theme, view.zoom, 1);
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);

  // The head: a ring with a dot, a fixed size on screen.
  const x = (view.x + progress.head.x * view.zoom) * dpr;
  const y = (view.y - progress.head.y * view.zoom) * dpr;
  ctx.strokeStyle = theme.head;
  ctx.fillStyle = theme.head;
  ctx.lineWidth = 1.5 * dpr;
  ctx.beginPath();
  ctx.arc(x, y, 6 * dpr, 0, 2 * Math.PI);
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(x, y, 2 * dpr, 0, 2 * Math.PI);
  ctx.fill();
}
