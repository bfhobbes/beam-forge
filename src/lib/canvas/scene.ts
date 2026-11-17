/**
 * The retained scene: the document's shapes resolved to world-space outlines
 * once per document change, with an R-tree for culling and hit-testing.
 */
import RBush from "rbush";
import {
  newId,
  type DesignDocument,
  type Point,
  type SceneObject,
  type Segment,
  type Subpath,
} from "#lib/document/model.js";
import { newLayer } from "#lib/document/layers.js";
import { outline, transformSubpath } from "#lib/document/shapes.js";
import { IDENTITY, multiply, type Transform } from "#lib/document/transform.js";
import type { Bounds } from "./view.js";

/** One drawable shape: a leaf object with its outline in world millimetres. */
export interface SceneItem {
  /** The leaf object's id. */
  id: string;
  /** The top-level object it belongs to: what a click selects. */
  rootId: string;
  layerId: string;
  color: string;
  /** On a Fill layer: its closed outlines are hatched, so "Show fills" shades them. */
  fill: boolean;
  /** Paint order, bottom first. */
  order: number;
  subpaths: Subpath[];
  bounds: Bounds;
  /** Flattened outlines, for hit-testing. */
  polylines: Point[][];
  /** Hit anywhere in its bounds, not just near its outline: text, whose letters are small targets. */
  boxHit: boolean;
}

interface Entry extends Bounds {
  item: SceneItem;
}

/** Line pieces per curve when flattening for bounds and hit-tests. */
const CURVE_STEPS = 16;

/** Drawn for an object whose layer the document lacks; saving refuses such a document. */
export const MISSING_LAYER_COLOR = "#808080";

const line = (x: number, y: number): Segment => ({ kind: "line", to: { x, y } });

function flatten(subpath: Subpath): Point[] {
  const points = [subpath.start];
  let from = subpath.start;
  for (const segment of subpath.segments) {
    if (segment.kind === "line") {
      points.push(segment.to);
    } else {
      for (let step = 1; step <= CURVE_STEPS; step++) {
        const t = step / CURVE_STEPS;
        const u = 1 - t;
        if (segment.kind === "quad") {
          const { control: c, to } = segment;
          points.push({
            x: u * u * from.x + 2 * u * t * c.x + t * t * to.x,
            y: u * u * from.y + 2 * u * t * c.y + t * t * to.y,
          });
        } else {
          const { control1: c1, control2: c2, to } = segment;
          const [a, b, c, d] = [u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t];
          points.push({
            x: a * from.x + b * c1.x + c * c2.x + d * to.x,
            y: a * from.y + b * c1.y + c * c2.y + d * to.y,
          });
        }
      }
    }
    from = segment.to;
  }
  if (subpath.closed) points.push(subpath.start);
  return points;
}

function boundsOf(polylines: Point[][]): Bounds {
  const bounds = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity };
  for (const polyline of polylines) {
    for (const { x, y } of polyline) {
      bounds.minX = Math.min(bounds.minX, x);
      bounds.minY = Math.min(bounds.minY, y);
      bounds.maxX = Math.max(bounds.maxX, x);
      bounds.maxY = Math.max(bounds.maxY, y);
    }
  }
  return bounds;
}

/** The union of some boxes, or null for none. */
export function unionBounds(boxes: Iterable<Bounds>): Bounds | null {
  let union: Bounds | null = null;
  for (const box of boxes) {
    union = union
      ? {
          minX: Math.min(union.minX, box.minX),
          minY: Math.min(union.minY, box.minY),
          maxX: Math.max(union.maxX, box.maxX),
          maxY: Math.max(union.maxY, box.maxY),
        }
      : { ...box };
  }
  return union;
}

function distanceToSegment(p: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = dx * dx + dy * dy;
  const t =
    length === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

export class Scene {
  /** Bottom to top. */
  readonly items: SceneItem[];
  /** Everything in the document, or null when it is empty. */
  readonly bounds: Bounds | null;
  #index = new RBush<Entry>();

  constructor(items: SceneItem[]) {
    this.items = items;
    this.bounds = unionBounds(items.map((item) => item.bounds));
    this.#index.load(items.map((item) => ({ ...item.bounds, item })));
  }

  /** Items overlapping a world box, bottom to top. */
  query(box: Bounds): SceneItem[] {
    return this.#index
      .search(box)
      .map((entry) => entry.item)
      .sort((a, b) => a.order - b.order);
  }

  /** The top-level id of the topmost outline within `tolerance` mm of `point`. */
  hitTest(point: Point, tolerance: number): string | null {
    const near = this.query({
      minX: point.x - tolerance,
      minY: point.y - tolerance,
      maxX: point.x + tolerance,
      maxY: point.y + tolerance,
    });
    for (const item of near.reverse()) {
      const { minX, minY, maxX, maxY } = item.bounds;
      const inBox =
        point.x >= minX - tolerance &&
        point.x <= maxX + tolerance &&
        point.y >= minY - tolerance &&
        point.y <= maxY + tolerance;
      if (item.boxHit && inBox) return item.rootId;
      for (const polyline of item.polylines) {
        for (let i = 1; i < polyline.length; i++) {
          if (distanceToSegment(point, polyline[i - 1], polyline[i]) <= tolerance)
            return item.rootId;
        }
      }
    }
    return null;
  }

  /** Bounds of the given top-level objects, or null if none are in the scene. */
  boundsOf(rootIds: Iterable<string>): Bounds | null {
    const wanted = new Set(rootIds);
    return unionBounds(
      this.items.filter((item) => wanted.has(item.rootId)).map((item) => item.bounds),
    );
  }
}

/**
 * The document's shapes in their layer colours. Objects on hidden layers are
 * left out, so they are neither drawn nor hit.
 */
export function buildScene(doc: DesignDocument): Scene {
  const layers = new Map(doc.layers.map((layer) => [layer.id, layer]));
  const items: SceneItem[] = [];
  const visit = (object: SceneObject, parent: Transform, rootId: string) => {
    const transform = multiply(parent, object.transform);
    if (object.type === "group") {
      for (const child of object.children) visit(child, transform, rootId);
      return;
    }
    const layer = layers.get(object.layerId);
    if (layer && !layer.shown) return;
    const subpaths = outline(object).map((subpath) => transformSubpath(transform, subpath));
    // Text being typed can be empty for a moment: there is nothing to draw or hit.
    if (subpaths.length === 0) return;
    const polylines = subpaths.map(flatten);
    items.push({
      id: object.id,
      rootId,
      layerId: object.layerId,
      color: layer?.color ?? MISSING_LAYER_COLOR,
      fill: layer?.mode === "fill",
      order: items.length,
      subpaths,
      bounds: boundsOf(polylines),
      polylines,
      boxHit: object.type === "text",
    });
  };
  for (const object of doc.objects) visit(object, IDENTITY, object.id);
  return new Scene(items);
}

/** Path segments in the scene, not counting the implicit closing edges. */
export function segmentCount(scene: Scene): number {
  let count = 0;
  for (const item of scene.items)
    for (const subpath of item.subpaths) count += subpath.segments.length;
  return count;
}

/**
 * A stress-test document of `segments` line segments: zig-zag paths of 100
 * segments tiled over a 400 mm bed, cycling through layer colours.
 */
export function benchmarkDocument(segments: number): DesignDocument {
  const perPath = 100;
  const paths = Math.ceil(segments / perPath);
  const columns = Math.ceil(Math.sqrt(paths));
  const cell = 400 / columns;
  const layers = Array.from({ length: 8 }, (_, slot) => newLayer(slot));
  const objects: SceneObject[] = Array.from({ length: paths }, (_, i) => {
    const count = Math.min(perPath, segments - i * perPath);
    const x0 = (i % columns) * cell;
    const y0 = Math.floor(i / columns) * cell;
    return {
      id: newId(),
      name: `Benchmark ${i + 1}`,
      layerId: layers[i % layers.length].id,
      transform: IDENTITY,
      type: "path",
      subpaths: [
        {
          start: { x: x0, y: y0 },
          segments: Array.from({ length: count }, (_, s) =>
            line(x0 + ((s + 1) / count) * cell * 0.9, y0 + (s % 2 === 0 ? cell * 0.9 : 0)),
          ),
          closed: false,
        },
      ],
    };
  });
  return { version: 1, units: "mm", layers, objects };
}
