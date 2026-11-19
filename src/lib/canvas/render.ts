/**
 * Paints the canvas: bed, adaptive grid, document outlines (or the toolpath),
 * selection overlay, origin marker and rulers. Screen-space lines are drawn in device pixels so they stay crisp
 * on HiDPI displays; outlines use a world transform and Path2D caches.
 */
import type { Point, Subpath } from "#lib/document/model.js";
import { drawToolpath, type ToolpathDrawing } from "#lib/toolpath/render.js";
import type { Bed } from "./bed.js";
import type { CaretGeometry } from "#lib/text/text-editing.svelte.js";
import { HANDLE_SIZE, ROTATE_RADIUS, type Handle } from "./handles.js";
import type { MarqueeMode } from "./marquee.js";
import type { Scene, SceneItem } from "./scene.js";
import type { Gap, Guide } from "./snap.js";
import {
  GRID_MIN_PX,
  gridSteps,
  rulerStep,
  visibleWorld,
  worldToScreen,
  type Bounds,
  type View,
} from "./view.js";

/** Ruler thickness in CSS pixels. */
export const RULER_SIZE = 20;
/** Closest labelled ruler ticks may get, in CSS pixels. */
const RULER_LABEL_MIN_PX = 56;

/** Canvas colours, resolved from the app's CSS variables so the canvas follows the theme. */
export interface CanvasTheme {
  surround: string;
  bed: string;
  grid: string;
  edge: string;
  ruler: string;
  rulerText: string;
  origin: string;
  selection: string;
  /** Snapping guide lines. */
  guide: string;
}

export function readTheme(element: Element): CanvasTheme {
  const style = getComputedStyle(element);
  const read = (name: string) => style.getPropertyValue(name).trim();
  return {
    surround: read("--muted"),
    bed: read("--background"),
    grid: read("--foreground"),
    edge: read("--muted-foreground"),
    ruler: read("--background"),
    rulerText: read("--muted-foreground"),
    origin: read("--destructive"),
    selection: read("--foreground"),
    guide: read("--destructive"),
  };
}

export interface Frame {
  view: View;
  /** CSS pixels. */
  width: number;
  height: number;
  dpr: number;
  scene: Scene;
  bed: Bed;
  theme: CanvasTheme;
  overlay?: Overlay;
  /** Drawn instead of the document and overlay: the toolpath view. */
  toolpath?: ToolpathDrawing | null;
  /** Shade what each Fill layer burns, under the outlines. */
  showFills?: boolean;
}

/** What the select tool draws over the document. */
export interface Overlay {
  /** Top-level ids drawn as selected. */
  selected: ReadonlySet<string>;
  /** The selection box in world mm, or null when nothing is selected. */
  bounds: Bounds | null;
  handles: readonly Handle[];
  /** An in-progress marquee, in CSS px. */
  marquee: { from: Point; to: Point; mode: MarqueeMode } | null;
  /** A shape being drawn, in world mm. */
  draft: readonly Subpath[];
  /** The caret or selection of text being typed, in world mm. */
  text?: CaretGeometry | null;
  /** Snapping guide lines, in world mm. */
  guides?: readonly Guide[];
  /** Equal-spacing hints, in world mm. */
  gaps?: readonly Gap[];
}

const itemPaths = new WeakMap<SceneItem, Path2D>();
const colourBatches = new WeakMap<Scene, Map<string, Path2D>>();
const fillBatches = new WeakMap<Scene, { color: string; path: Path2D }[]>();

/** Opacity of the "Show fills" shading, so outlines and the grid stay readable. */
const FILL_ALPHA = 0.3;

function tracePath(
  path: Path2D,
  item: { readonly subpaths: readonly Subpath[] },
  closedOnly = false,
): void {
  for (const subpath of item.subpaths) {
    if (closedOnly && !subpath.closed) continue;
    path.moveTo(subpath.start.x, subpath.start.y);
    for (const segment of subpath.segments) {
      const { to } = segment;
      if (segment.kind === "line") path.lineTo(to.x, to.y);
      else if (segment.kind === "quad")
        path.quadraticCurveTo(segment.control.x, segment.control.y, to.x, to.y);
      else
        path.bezierCurveTo(
          segment.control1.x,
          segment.control1.y,
          segment.control2.x,
          segment.control2.y,
          to.x,
          to.y,
        );
    }
    if (subpath.closed) path.closePath();
  }
}

function itemPath(item: SceneItem): Path2D {
  let path = itemPaths.get(item);
  if (!path) {
    path = new Path2D();
    tracePath(path, item);
    itemPaths.set(item, path);
  }
  return path;
}

/** One path per colour for the whole scene, so a full view is a handful of strokes. */
function batches(scene: Scene): Map<string, Path2D> {
  let byColour = colourBatches.get(scene);
  if (!byColour) {
    byColour = new Map();
    for (const item of scene.items) {
      let path = byColour.get(item.color);
      if (!path) byColour.set(item.color, (path = new Path2D()));
      tracePath(path, item);
    }
    colourBatches.set(scene, byColour);
  }
  return byColour;
}

/**
 * One path per Fill layer holding its closed outlines. Filled by the even-odd
 * rule across the whole layer, as the fill planner hatches it, so the counter
 * of an O and a shape inside another stay empty. Open paths aren't filled.
 */
function fills(scene: Scene): { color: string; path: Path2D }[] {
  let layers = fillBatches.get(scene);
  if (!layers) {
    const byLayer = new Map<string, { color: string; path: Path2D }>();
    for (const item of scene.items) {
      if (!item.fill) continue;
      let layer = byLayer.get(item.layerId);
      if (!layer) byLayer.set(item.layerId, (layer = { color: item.color, path: new Path2D() }));
      tracePath(layer.path, item, true);
    }
    layers = [...byLayer.values()];
    fillBatches.set(scene, layers);
  }
  return layers;
}

/** Whole device pixel plus a half, so a 1px line covers exactly one pixel row. */
const crisp = (cssPx: number, dpr: number) => Math.round(cssPx * dpr) + 0.5;

/** Ruler label: no float noise, no trailing zeros, no "-0". */
function label(value: number): string {
  const rounded = Number(value.toFixed(4));
  return String(rounded === 0 ? 0 : rounded);
}

/** Multiples of `step` from `min` to `max`, inclusive. */
function multiples(min: number, max: number, step: number): number[] {
  const first = Math.ceil(min / step - 1e-9);
  const last = Math.floor(max / step + 1e-9);
  return Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => (first + i) * step);
}

function drawGrid(ctx: CanvasRenderingContext2D, frame: Frame): void {
  const { view, dpr, bed, theme } = frame;
  const visible = visibleWorld(view, frame);
  const minX = Math.max(0, visible.minX);
  const maxX = Math.min(bed.width, visible.maxX);
  const minY = Math.max(0, visible.minY);
  const maxY = Math.min(bed.height, visible.maxY);
  if (minX > maxX || minY > maxY) return;

  const topLeft = worldToScreen(view, { x: minX, y: maxY });
  const bottomRight = worldToScreen(view, { x: maxX, y: minY });
  const { minor, major } = gridSteps(view.zoom, GRID_MIN_PX);
  const isMajor = (value: number) => Math.abs(value / major - Math.round(value / major)) < 1e-6;

  for (const pass of ["minor", "major"] as const) {
    ctx.beginPath();
    for (const x of multiples(minX, maxX, minor)) {
      if (isMajor(x) !== (pass === "major")) continue;
      const sx = crisp(view.x + x * view.zoom, dpr);
      ctx.moveTo(sx, topLeft.y * dpr);
      ctx.lineTo(sx, bottomRight.y * dpr);
    }
    for (const y of multiples(minY, maxY, minor)) {
      if (isMajor(y) !== (pass === "major")) continue;
      const sy = crisp(view.y - y * view.zoom, dpr);
      ctx.moveTo(topLeft.x * dpr, sy);
      ctx.lineTo(bottomRight.x * dpr, sy);
    }
    ctx.strokeStyle = theme.grid;
    ctx.globalAlpha = pass === "major" ? 0.16 : 0.06;
    ctx.lineWidth = 1;
    ctx.stroke();
  }
  ctx.globalAlpha = 1;
}

function drawObjects(ctx: CanvasRenderingContext2D, frame: Frame): void {
  const { view, dpr, scene } = frame;
  const visible = scene.query(visibleWorld(view, frame));
  if (visible.length === 0) return;
  // World mm → device px, flipping Y up.
  ctx.setTransform(dpr * view.zoom, 0, 0, -dpr * view.zoom, dpr * view.x, dpr * view.y);
  if (frame.showFills) {
    ctx.globalAlpha = FILL_ALPHA;
    for (const { color, path } of fills(scene)) {
      ctx.fillStyle = color;
      ctx.fill(path, "evenodd");
    }
    ctx.globalAlpha = 1;
  }
  ctx.lineWidth = 1 / view.zoom;
  ctx.lineJoin = "round";
  if (visible.length * 2 > scene.items.length) {
    for (const [colour, path] of batches(scene)) {
      ctx.strokeStyle = colour;
      ctx.stroke(path);
    }
  } else {
    for (const item of visible) {
      ctx.strokeStyle = item.color;
      ctx.stroke(itemPath(item));
    }
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
}

/** The caret or selected-text highlight of text being typed. */
function drawCaret(ctx: CanvasRenderingContext2D, frame: Frame, text: CaretGeometry): void {
  const { view, dpr, theme } = frame;
  ctx.setTransform(dpr * view.zoom, 0, 0, -dpr * view.zoom, dpr * view.x, dpr * view.y);
  if (text.selection.length > 0) {
    ctx.globalAlpha = 0.25;
    ctx.fillStyle = theme.selection;
    for (const quad of text.selection) {
      ctx.beginPath();
      quad.forEach(({ x, y }, i) => (i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)));
      ctx.closePath();
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }
  if (text.caret) {
    ctx.beginPath();
    ctx.moveTo(text.caret.from.x, text.caret.from.y);
    ctx.lineTo(text.caret.to.x, text.caret.to.y);
    ctx.lineWidth = 1.5 / view.zoom;
    ctx.strokeStyle = theme.selection;
    ctx.stroke();
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.lineWidth = 1;
}

function drawOverlay(ctx: CanvasRenderingContext2D, frame: Frame, overlay: Overlay): void {
  const { view, dpr, scene, theme } = frame;
  if (overlay.selected.size > 0) {
    // Selected outlines get a dashed highlight over their normal stroke.
    const selected = scene
      .query(visibleWorld(view, frame))
      .filter((item) => overlay.selected.has(item.rootId));
    ctx.setTransform(dpr * view.zoom, 0, 0, -dpr * view.zoom, dpr * view.x, dpr * view.y);
    ctx.lineWidth = 2 / view.zoom;
    ctx.setLineDash([5 / view.zoom, 4 / view.zoom]);
    for (const item of selected) {
      ctx.strokeStyle = item.color;
      ctx.stroke(itemPath(item));
    }
    ctx.setLineDash([]);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
  }

  ctx.strokeStyle = theme.selection;
  ctx.fillStyle = theme.bed;
  ctx.lineWidth = 1;
  if (overlay.bounds) {
    const topLeft = worldToScreen(view, { x: overlay.bounds.minX, y: overlay.bounds.maxY });
    const bottomRight = worldToScreen(view, { x: overlay.bounds.maxX, y: overlay.bounds.minY });
    ctx.setLineDash([4 * dpr, 3 * dpr]);
    ctx.strokeRect(
      crisp(topLeft.x, dpr),
      crisp(topLeft.y, dpr),
      Math.round((bottomRight.x - topLeft.x) * dpr),
      Math.round((bottomRight.y - topLeft.y) * dpr),
    );
    ctx.setLineDash([]);
  }
  for (const handle of overlay.handles) {
    const x = crisp(handle.at.x, dpr);
    const y = crisp(handle.at.y, dpr);
    ctx.beginPath();
    if (handle.kind === "scale") {
      const half = Math.round((HANDLE_SIZE / 2) * dpr);
      ctx.rect(x - half, y - half, half * 2, half * 2);
    } else {
      ctx.arc(x, y, ROTATE_RADIUS * dpr, 0, 2 * Math.PI);
    }
    ctx.fill();
    ctx.stroke();
  }

  if (overlay.draft.length > 0) {
    const draft = new Path2D();
    tracePath(draft, { subpaths: overlay.draft });
    ctx.setTransform(dpr * view.zoom, 0, 0, -dpr * view.zoom, dpr * view.x, dpr * view.y);
    ctx.lineWidth = 1.5 / view.zoom;
    ctx.strokeStyle = theme.selection;
    ctx.stroke(draft);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.lineWidth = 1;
  }

  if (overlay.text) drawCaret(ctx, frame, overlay.text);
  if (overlay.guides?.length) drawGuides(ctx, frame, overlay.guides);
  if (overlay.gaps?.length) drawGaps(ctx, frame, overlay.gaps);

  if (overlay.marquee) {
    // Window: solid edge. Crossing: dashed edge, the CAD convention.
    const { from, to, mode } = overlay.marquee;
    const x = Math.min(from.x, to.x) * dpr;
    const y = Math.min(from.y, to.y) * dpr;
    const width = Math.abs(to.x - from.x) * dpr;
    const height = Math.abs(to.y - from.y) * dpr;
    ctx.fillStyle = theme.selection;
    ctx.globalAlpha = 0.06;
    ctx.fillRect(x, y, width, height);
    ctx.globalAlpha = 0.8;
    if (mode === "crossing") ctx.setLineDash([4 * dpr, 3 * dpr]);
    ctx.strokeRect(x, y, width, height);
    ctx.setLineDash([]);
    ctx.globalAlpha = 1;
  }
}

/** Snapping guides: crisp 1px lines, with a cross at each end. */
function drawGuides(ctx: CanvasRenderingContext2D, frame: Frame, guides: readonly Guide[]): void {
  const { view, dpr, theme } = frame;
  const cross = 3 * dpr;
  ctx.beginPath();
  for (const guide of guides) {
    const ends = (
      guide.axis === "x"
        ? [
            { x: guide.at, y: guide.from },
            { x: guide.at, y: guide.to },
          ]
        : [
            { x: guide.from, y: guide.at },
            { x: guide.to, y: guide.at },
          ]
    ).map((point) => worldToScreen(view, point));
    const [a, b] = ends.map(({ x, y }) =>
      guide.axis === "x" ? { x: crisp(x, dpr), y: y * dpr } : { x: x * dpr, y: crisp(y, dpr) },
    );
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    for (const { x, y } of [a, b]) {
      ctx.moveTo(x - cross, y - cross);
      ctx.lineTo(x + cross, y + cross);
      ctx.moveTo(x - cross, y + cross);
      ctx.lineTo(x + cross, y - cross);
    }
  }
  ctx.strokeStyle = theme.guide;
  ctx.lineWidth = 1;
  ctx.stroke();
}

/** Equal-spacing hints: a line across each gap with a tick at each end. */
function drawGaps(ctx: CanvasRenderingContext2D, frame: Frame, gaps: readonly Gap[]): void {
  const { view, dpr, theme } = frame;
  const tick = 4 * dpr;
  ctx.beginPath();
  for (const gap of gaps) {
    const [a, b] = (
      gap.axis === "x"
        ? [
            { x: gap.from, y: gap.at },
            { x: gap.to, y: gap.at },
          ]
        : [
            { x: gap.at, y: gap.from },
            { x: gap.at, y: gap.to },
          ]
    ).map((point) => worldToScreen(view, point));
    if (gap.axis === "x") {
      const y = crisp(a.y, dpr);
      ctx.moveTo(a.x * dpr, y);
      ctx.lineTo(b.x * dpr, y);
      for (const x of [crisp(a.x, dpr), crisp(b.x, dpr)]) {
        ctx.moveTo(x, y - tick);
        ctx.lineTo(x, y + tick);
      }
    } else {
      const x = crisp(a.x, dpr);
      ctx.moveTo(x, a.y * dpr);
      ctx.lineTo(x, b.y * dpr);
      for (const y of [crisp(a.y, dpr), crisp(b.y, dpr)]) {
        ctx.moveTo(x - tick, y);
        ctx.lineTo(x + tick, y);
      }
    }
  }
  ctx.strokeStyle = theme.guide;
  ctx.lineWidth = 1;
  ctx.stroke();
}

function drawOrigin(ctx: CanvasRenderingContext2D, frame: Frame): void {
  const { view, dpr, theme } = frame;
  const origin = worldToScreen(view, { x: 0, y: 0 });
  const x = crisp(origin.x, dpr);
  const y = crisp(origin.y, dpr);
  const arm = 14 * dpr;
  ctx.strokeStyle = theme.origin;
  ctx.fillStyle = theme.origin;
  ctx.lineWidth = Math.max(1, Math.round(1.5 * dpr));
  ctx.beginPath();
  ctx.moveTo(x, y);
  ctx.lineTo(x + arm, y);
  ctx.moveTo(x, y);
  ctx.lineTo(x, y - arm);
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(x, y, 3 * dpr, 0, 2 * Math.PI);
  ctx.fill();
}

function drawRulers(ctx: CanvasRenderingContext2D, frame: Frame): void {
  const { view, dpr, theme, width, height } = frame;
  const size = RULER_SIZE * dpr;
  const step = rulerStep(view.zoom, RULER_LABEL_MIN_PX);
  const lead = Math.round(step / 10 ** Math.floor(Math.log10(step) + 1e-9));
  const tick = step / (lead === 2 ? 4 : 5);
  const visible = visibleWorld(view, frame);

  ctx.fillStyle = theme.ruler;
  ctx.fillRect(0, 0, width * dpr, size);
  ctx.fillRect(0, 0, size, height * dpr);

  ctx.beginPath();
  for (const x of multiples(visible.minX, visible.maxX, tick)) {
    const sx = crisp(view.x + x * view.zoom, dpr);
    if (sx < size) continue;
    const long = Math.abs(x / step - Math.round(x / step)) < 1e-6;
    ctx.moveTo(sx, size);
    ctx.lineTo(sx, size - (long ? size * 0.6 : size * 0.25));
  }
  for (const y of multiples(visible.minY, visible.maxY, tick)) {
    const sy = crisp(view.y - y * view.zoom, dpr);
    if (sy < size) continue;
    const long = Math.abs(y / step - Math.round(y / step)) < 1e-6;
    ctx.moveTo(size, sy);
    ctx.lineTo(size - (long ? size * 0.6 : size * 0.25), sy);
  }
  // Ruler edges.
  ctx.moveTo(size, crisp(RULER_SIZE - 0.5, dpr));
  ctx.lineTo(width * dpr, crisp(RULER_SIZE - 0.5, dpr));
  ctx.moveTo(crisp(RULER_SIZE - 0.5, dpr), size);
  ctx.lineTo(crisp(RULER_SIZE - 0.5, dpr), height * dpr);
  ctx.strokeStyle = theme.rulerText;
  ctx.globalAlpha = 0.6;
  ctx.lineWidth = 1;
  ctx.stroke();
  ctx.globalAlpha = 1;

  ctx.fillStyle = theme.rulerText;
  ctx.font = `${10 * dpr}px "Geist Variable", sans-serif`;
  ctx.textBaseline = "top";
  ctx.textAlign = "left";
  for (const x of multiples(visible.minX, visible.maxX, step)) {
    const sx = (view.x + x * view.zoom) * dpr;
    if (sx >= size) ctx.fillText(label(x), sx + 3 * dpr, 3 * dpr);
  }
  for (const y of multiples(visible.minY, visible.maxY, step)) {
    const sy = (view.y - y * view.zoom) * dpr;
    if (sy < size) continue;
    ctx.save();
    ctx.translate(3 * dpr, sy - 3 * dpr);
    ctx.rotate(-Math.PI / 2);
    ctx.fillText(label(y), 0, 0);
    ctx.restore();
  }
  // Corner square over where the rulers meet.
  ctx.fillStyle = theme.ruler;
  ctx.fillRect(0, 0, size, size);
}

/** Paints one frame. The canvas backing store is `width × dpr` by `height × dpr`. */
export function drawFrame(ctx: CanvasRenderingContext2D, frame: Frame): void {
  const { view, dpr, bed, theme, width, height } = frame;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = theme.surround;
  ctx.fillRect(0, 0, width * dpr, height * dpr);

  const corner = worldToScreen(view, { x: 0, y: bed.height });
  ctx.fillStyle = theme.bed;
  ctx.fillRect(
    corner.x * dpr,
    corner.y * dpr,
    bed.width * view.zoom * dpr,
    bed.height * view.zoom * dpr,
  );
  drawGrid(ctx, frame);
  ctx.strokeStyle = theme.edge;
  ctx.lineWidth = 1;
  ctx.strokeRect(
    crisp(corner.x, dpr),
    crisp(corner.y, dpr),
    Math.round(bed.width * view.zoom * dpr),
    Math.round(bed.height * view.zoom * dpr),
  );

  if (frame.toolpath) {
    drawToolpath(ctx, view, dpr, frame.toolpath, { rapid: theme.edge, head: theme.origin });
  } else {
    drawObjects(ctx, frame);
    if (frame.overlay) drawOverlay(ctx, frame, frame.overlay);
  }
  drawOrigin(ctx, frame);
  drawRulers(ctx, frame);
}
