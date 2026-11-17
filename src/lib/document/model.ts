/**
 * The design document: plain JSON-serialisable data in millimetres, Y-up to
 * match the machine. Documents are immutable; commands return new ones.
 */
import type { CutLayer } from "#lib/bindings/project/CutLayer.js";
import type { TextAlign } from "#lib/bindings/text/TextAlign.js";
import type { Transform } from "./transform.js";

export type { CutLayer } from "#lib/bindings/project/CutLayer.js";
export type { LayerMode } from "#lib/bindings/project/LayerMode.js";
export type { TextAlign } from "#lib/bindings/text/TextAlign.js";

export interface Point {
  x: number;
  y: number;
}

export type Segment =
  | { kind: "line"; to: Point }
  | { kind: "quad"; control: Point; to: Point }
  | { kind: "cubic"; control1: Point; control2: Point; to: Point };

export interface Subpath {
  start: Point;
  segments: Segment[];
  closed: boolean;
}

/** Fields every object has. `layerId` names one of the document's cut layers. */
interface ObjectBase {
  id: string;
  name: string;
  layerId: string;
  transform: Transform;
}

export interface PathObject extends ObjectBase {
  type: "path";
  subpaths: Subpath[];
}

/** Axis-aligned in object space, with its origin at the bottom-left corner. */
export interface RectObject extends ObjectBase {
  type: "rect";
  width: number;
  height: number;
  cornerRadius: number;
}

/** Centred on the object-space origin. */
export interface EllipseObject extends ObjectBase {
  type: "ellipse";
  rx: number;
  ry: number;
}

/** A regular polygon centred on the origin; a star when `innerRadius` is set. */
export interface PolygonObject extends ObjectBase {
  type: "polygon";
  sides: number;
  radius: number;
  innerRadius: number | null;
}

/**
 * Editable text. `height` is the cap height in mm (how tall a capital H
 * stands). `outline` is the text laid out by Rust (`crates/bf-core/src/text`)
 * when it was last edited, in object space with the first baseline at y = 0;
 * the canvas and planner draw it, so a file looks and cuts the same without
 * the font installed.
 */
export interface TextObject extends ObjectBase {
  type: "text";
  text: string;
  fontFamily: string;
  /** CSS weight: 400 regular, 700 bold. */
  fontWeight: number;
  italic: boolean;
  height: number;
  /** Extra space after each character, mm. */
  letterSpacing: number;
  /** Baseline distance as a multiple of the font's line height. */
  lineSpacing: number;
  align: TextAlign;
  outline: Subpath[];
}

export interface GroupObject extends ObjectBase {
  type: "group";
  /** Bottom to top: later children draw over earlier ones. */
  children: SceneObject[];
}

export type SceneObject =
  | PathObject
  | RectObject
  | EllipseObject
  | PolygonObject
  | TextObject
  | GroupObject;

export interface DesignDocument {
  version: 1;
  units: "mm";
  /** Cut layers in cut order. Defined in Rust (`project/layer.rs`) for the planner. */
  layers: CutLayer[];
  /** Top-level objects, bottom to top. */
  objects: SceneObject[];
}

export function emptyDocument(): DesignDocument {
  return { version: 1, units: "mm", layers: [], objects: [] };
}

/** Where an object sits: its parent group (null for top level) and index there. */
export interface Placement {
  parent: string | null;
  index: number;
  object: SceneObject;
}

export function findPlacement(
  objects: readonly SceneObject[],
  id: string,
  parent: string | null = null,
): Placement | undefined {
  for (const [index, object] of objects.entries()) {
    if (object.id === id) return { parent, index, object };
    if (object.type === "group") {
      const found = findPlacement(object.children, id, object.id);
      if (found) return found;
    }
  }
  return undefined;
}

/** Every object id in the tree, depth first. */
export function* allIds(objects: readonly SceneObject[]): Generator<string> {
  for (const object of objects) {
    yield object.id;
    if (object.type === "group") yield* allIds(object.children);
  }
}

export function newId(): string {
  return crypto.randomUUID();
}
