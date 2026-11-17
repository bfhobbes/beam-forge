/**
 * Canvas view maths: how the Y-up millimetre world maps to Y-down CSS pixels.
 * Pure functions over plain data so they test without a DOM.
 */
import type { Point } from "#lib/document/model.js";

/**
 * `zoom` is CSS pixels per millimetre; `x`, `y` is where the world origin
 * sits on screen, in CSS pixels from the canvas's top-left corner.
 */
export interface View {
  zoom: number;
  x: number;
  y: number;
}

/** An axis-aligned box in world millimetres. */
export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

/** A screen rectangle in CSS pixels. */
export interface Viewport {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** 0.02 px/mm shows a 50 m span; 2000 px/mm shows 0.5 µm per pixel. */
export const MIN_ZOOM = 0.02;
export const MAX_ZOOM = 2000;

const clampZoom = (zoom: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));

export function worldToScreen(view: View, { x, y }: Point): Point {
  return { x: view.x + x * view.zoom, y: view.y - y * view.zoom };
}

export function screenToWorld(view: View, { x, y }: Point): Point {
  return { x: (x - view.x) / view.zoom, y: (view.y - y) / view.zoom };
}

/** The world rectangle a canvas of this CSS size shows. */
export function visibleWorld(view: View, size: { width: number; height: number }): Bounds {
  const topLeft = screenToWorld(view, { x: 0, y: 0 });
  const bottomRight = screenToWorld(view, { x: size.width, y: size.height });
  return { minX: topLeft.x, minY: bottomRight.y, maxX: bottomRight.x, maxY: topLeft.y };
}

/** Zooms by `factor` about a screen point, which stays over the same world point. */
export function zoomAt(view: View, at: Point, factor: number): View {
  const zoom = clampZoom(view.zoom * factor);
  if (zoom === view.zoom) return view;
  const anchor = screenToWorld(view, at);
  return { zoom, x: at.x - anchor.x * zoom, y: at.y + anchor.y * zoom };
}

export function panBy(view: View, dx: number, dy: number): View {
  return { ...view, x: view.x + dx, y: view.y + dy };
}

/** The view that centres `bounds` in `viewport`, leaving `padding` CSS px on every side. */
export function fitBounds(bounds: Bounds, viewport: Viewport, padding: number): View {
  const width = bounds.maxX - bounds.minX;
  const height = bounds.maxY - bounds.minY;
  const room = (span: number, size: number) =>
    span > 0 ? Math.max(1, size - 2 * padding) / span : Infinity;
  const zoom = clampZoom(Math.min(room(width, viewport.width), room(height, viewport.height)));
  const centre = { x: (bounds.minX + bounds.maxX) / 2, y: (bounds.minY + bounds.maxY) / 2 };
  return {
    zoom,
    x: viewport.x + viewport.width / 2 - centre.x * zoom,
    y: viewport.y + viewport.height / 2 + centre.y * zoom,
  };
}

/** The smallest 1, 2 or 5 × 10ⁿ millimetre step that is at least `minPx` on screen. */
function niceStep(zoom: number, minPx: number): number {
  const target = minPx / zoom;
  const power = 10 ** Math.floor(Math.log10(target));
  for (const multiple of [1, 2, 5, 10]) {
    // Round off float noise such as 0.30000000000000004.
    const step = Number((multiple * power).toPrecision(12));
    if (step >= target * (1 - 1e-9)) return step;
  }
  return 10 * power;
}

/**
 * Grid spacing in mm for a zoom: minor lines at least `minPx` apart, and a
 * major line every 10 of the next decade (5 × 2, 2 × 5 or 1 × 10).
 */
export function gridSteps(zoom: number, minPx: number): { minor: number; major: number } {
  const minor = niceStep(zoom, minPx);
  const lead = Math.round(minor / 10 ** Math.floor(Math.log10(minor) + 1e-9));
  const major = Number((minor * (lead === 1 ? 10 : lead === 2 ? 5 : 2)).toPrecision(12));
  return { minor, major };
}

/** Spacing in mm between labelled ruler ticks, at least `minPx` apart. */
export function rulerStep(zoom: number, minPx: number): number {
  return niceStep(zoom, minPx);
}

/** The slice of `WheelEvent` gesture detection reads. */
export interface WheelInput {
  deltaX: number;
  deltaY: number;
  /** 0 pixels, 1 lines, 2 pages. */
  deltaMode: number;
  ctrlKey: boolean;
  shiftKey: boolean;
}

export type WheelGesture =
  | { kind: "zoom"; factor: number }
  | { kind: "pan"; dx: number; dy: number };

/**
 * Tells a mouse wheel from a trackpad. Browsers report a trackpad pinch as
 * ctrl+wheel, so that zooms. A mouse wheel notch is a line-mode delta or a
 * large whole-pixel one with no sideways part, and zooms. Anything
 * else is a two-finger scroll and pans; shift+wheel pans sideways.
 */
export function wheelGesture(event: WheelInput): WheelGesture {
  // Negate without producing -0, which reads oddly and fails strict equality.
  const negate = (value: number) => (value === 0 ? 0 : -value);
  if (event.ctrlKey) return { kind: "zoom", factor: Math.exp(-event.deltaY * 0.01) };
  if (event.shiftKey) return { kind: "pan", dx: negate(event.deltaX || event.deltaY), dy: 0 };
  const notch =
    event.deltaMode !== 0 ||
    (event.deltaX === 0 && Number.isInteger(event.deltaY) && Math.abs(event.deltaY) >= 50);
  if (notch) {
    const pixels = event.deltaMode === 0 ? event.deltaY : event.deltaY * 33;
    return { kind: "zoom", factor: 1.0015 ** -pixels };
  }
  return { kind: "pan", dx: negate(event.deltaX), dy: negate(event.deltaY) };
}
