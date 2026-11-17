/**
 * Typing text on the canvas. A hidden textarea holds the string being
 * edited; each change is laid out in Rust and written to the document as one
 * coalesced undo step per editing session, while the canvas draws the caret
 * from the layout's caret positions.
 *
 * A new text isn't added to the document until something is typed, so
 * clicking with the text tool and leaving leaves nothing behind. Emptying a
 * text and leaving removes it.
 */
import {
  batch,
  editText,
  insert,
  remove,
  type Command,
  type TextChanges,
} from "#lib/document/commands.js";
import { drawingLayer, type DrawingLayer } from "#lib/document/drawing-layer.svelte.js";
import { editor, type DocumentEditor } from "#lib/document/editor.svelte.js";
import {
  findPlacement,
  newId,
  type Point,
  type TextAlign,
  type TextObject,
} from "#lib/document/model.js";
import { selection, type Selection } from "#lib/document/selection.svelte.js";
import { applyToPoint, translate, type Transform } from "#lib/document/transform.js";
import {
  styleOf,
  textAdapter,
  type TextAdapter,
  type TextLayout,
  type TextStyle,
} from "./adapter.js";

/** Families tried, in order, for new text when no other choice was made. */
const PREFERRED_FAMILIES = ["Arial", "Helvetica", "Liberation Sans", "DejaVu Sans", "Noto Sans"];

export const DEFAULT_TEXT_STYLE: TextStyle = {
  fontFamily: "Arial",
  fontWeight: 400,
  italic: false,
  height: 10,
  letterSpacing: 0,
  lineSpacing: 1,
  align: "left" as TextAlign,
};

/** A family to start new text in: a common sans if installed, else the first. */
export function defaultFamily(families: readonly string[]): string | null {
  const lower = new Map(families.map((name) => [name.toLowerCase(), name]));
  for (const name of PREFERRED_FAMILIES) {
    const found = lower.get(name.toLowerCase());
    if (found) return found;
  }
  return families[0] ?? null;
}

/** A text being typed: one already in the document, or a new one not yet added. */
interface Session {
  id: string;
  /** Set until the first character adds the text to the document. */
  pending: { transform: Transform; layerId: string; addLayer: Command[] } | null;
}

/** The caret and selection to draw, in world mm. */
export interface CaretGeometry {
  /** Bottom to top of the caret, or of each selected line's highlight edge. */
  caret: { from: Point; to: Point } | null;
  /** One quadrilateral per selected line, corners in order. */
  selection: Point[][];
}

export class TextEditing {
  /** The text being edited, or null. */
  session = $state.raw<Session | null>(null);
  /** The string as typed; the document catches up as layouts arrive. */
  value = $state("");
  selectionStart = $state(0);
  selectionEnd = $state(0);
  /** Style for new text: the last one picked, so a run of labels match. */
  defaults = $state.raw<TextStyle>(DEFAULT_TEXT_STYLE);
  /** Why the last layout failed, shown in the inspector. */
  error = $state("");
  /** The latest layout of the text being edited, and the string it was made from. */
  #layout = $state.raw<{ text: string; layout: TextLayout } | null>(null);
  #requests = 0;

  readonly #editor: DocumentEditor;
  readonly #selection: Selection;
  readonly #layer: DrawingLayer;
  readonly #adapter: TextAdapter;

  constructor(
    editor: DocumentEditor,
    selection: Selection,
    layer: DrawingLayer,
    adapter: TextAdapter,
  ) {
    this.#editor = editor;
    this.#selection = selection;
    this.#layer = layer;
    this.#adapter = adapter;
  }

  get editing(): string | null {
    return this.session?.id ?? null;
  }

  /** The edited text as it stands in the document, if it is there yet. */
  #object(id: string): TextObject | null {
    const placement = findPlacement(this.#editor.doc.objects, id);
    return placement?.object.type === "text" ? placement.object : null;
  }

  #style(): TextStyle {
    const object = this.session && this.#object(this.session.id);
    return object ? styleOf(object) : this.defaults;
  }

  /** Starts a new text with its first baseline at `at` (world mm). */
  create(at: Point): void {
    this.finish();
    const { layer, add } = this.#layer.forNewShape(this.#editor.doc);
    this.session = {
      id: newId(),
      pending: { transform: translate(at.x, at.y), layerId: layer.id, addLayer: add ? [add] : [] },
    };
    this.#selection.clear();
    this.#reset("");
  }

  /** Starts new text in an installed family when the current default isn't one. */
  useInstalledFamily(families: readonly string[]): void {
    const lower = this.defaults.fontFamily.toLowerCase();
    if (families.some((name) => name.toLowerCase() === lower)) return;
    const family = defaultFamily(families);
    if (family) this.defaults = { ...this.defaults, fontFamily: family };
  }

  /** Edits a text already in the document, caret at the end. */
  edit(id: string): void {
    const object = this.#object(id);
    if (!object) return;
    if (this.session?.id === id) return;
    this.finish();
    this.session = { id, pending: null };
    this.#selection.set([id]);
    this.#reset(object.text);
  }

  #reset(text: string): void {
    this.value = text;
    this.selectionStart = this.selectionEnd = text.length;
    this.error = "";
    this.#layout = null;
    this.#editor.endCoalesce();
    void this.#relayout(text, this.#style(), {});
  }

  /** The textarea's string changed. */
  input(value: string, selectionStart: number, selectionEnd: number): Promise<void> {
    this.value = value;
    this.select(selectionStart, selectionEnd);
    return this.#relayout(value, this.#style(), { text: value });
  }

  select(start: number, end: number): void {
    this.selectionStart = start;
    this.selectionEnd = end;
  }

  /**
   * Changes a text's style (from the inspector). Lays it out again and
   * writes the style and outline as one undo step.
   */
  async restyle(id: string, changes: Partial<TextStyle>): Promise<boolean> {
    const object = this.#object(id);
    this.defaults = { ...(object ? styleOf(object) : this.defaults), ...changes };
    if (!object) {
      // A new text not typed into yet: just move its caret to the new size.
      if (this.session?.id === id) await this.#relayout(this.value, this.defaults, {});
      return this.session?.id === id;
    }
    const style = { ...styleOf(object), ...changes };
    try {
      const layout = await this.#adapter.layout(object.text, style);
      this.error = "";
      const done = this.#editor.execute(editText(id, { ...changes, outline: layout.outline }));
      if (done && this.session?.id === id) this.#layout = { text: object.text, layout };
      return done;
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return false;
    }
  }

  async #relayout(text: string, style: TextStyle, changes: TextChanges): Promise<void> {
    const session = this.session;
    if (!session) return;
    const request = ++this.#requests;
    let layout: TextLayout;
    try {
      layout = await this.#adapter.layout(text, style);
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error);
      return;
    }
    // A later keystroke or another text took over while this was laid out.
    if (request !== this.#requests || this.session !== session) return;
    this.error = "";
    this.#layout = { text, layout };
    if (!("text" in changes)) return;
    this.#write(session, text, style, layout);
  }

  #write(session: Session, text: string, style: TextStyle, layout: TextLayout): void {
    const coalesce = `text:${session.id}`;
    if (session.pending) {
      if (text === "") return;
      const { transform, layerId, addLayer } = session.pending;
      const object: TextObject = {
        type: "text",
        id: session.id,
        name: "Text",
        layerId,
        transform,
        text,
        ...style,
        outline: layout.outline,
      };
      if (this.#editor.execute(batch([...addLayer, insert([object])]), { coalesce })) {
        this.session = { id: session.id, pending: null };
        this.#selection.set([session.id]);
      }
      return;
    }
    this.#editor.execute(editText(session.id, { text, outline: layout.outline }), { coalesce });
  }

  /** Stops editing. A text left empty is removed. */
  finish(): void {
    const session = this.session;
    if (!session) return;
    this.session = null;
    this.#layout = null;
    const object = this.#object(session.id);
    if (object && object.text.trim() === "") {
      this.#editor.execute(remove([session.id]), { coalesce: `text:${session.id}` });
      this.#selection.clear();
    }
    this.#editor.endCoalesce();
  }

  /** Where the caret (or selection) is, in world mm; null when not editing. */
  caret(): CaretGeometry | null {
    const session = this.session;
    const laid = this.#layout;
    if (!session || !laid) return null;
    const transform = session.pending?.transform ?? this.#object(session.id)?.transform;
    if (!transform) return null;
    const at = (x: number, y: number) => applyToPoint(transform, { x, y });
    const { lines } = laid.layout;
    // The layout may trail the textarea by a keystroke: clamp into it.
    const position = (index: number) => {
      let rest = Math.min(index, laid.text.length);
      for (const [row, line] of lines.entries()) {
        if (rest < line.carets.length) return { row, x: line.carets[rest] };
        rest -= line.carets.length;
      }
      const last = lines.length - 1;
      return { row: last, x: lines[last].carets.at(-1) ?? 0 };
    };
    const edge = (row: number, x: number) => {
      const { baseline, capHeight } = lines[row];
      const descent = capHeight * 0.25;
      return { from: at(x, baseline - descent), to: at(x, baseline + capHeight * 1.05) };
    };
    const start = position(Math.min(this.selectionStart, this.selectionEnd));
    const end = position(Math.max(this.selectionStart, this.selectionEnd));
    if (start.row === end.row && start.x === end.x) {
      return { caret: edge(start.row, start.x), selection: [] };
    }
    const selection: Point[][] = [];
    for (let row = start.row; row <= end.row; row++) {
      const carets = lines[row].carets;
      const from = row === start.row ? start.x : carets[0];
      const to = row === end.row ? end.x : (carets.at(-1) ?? from);
      const a = edge(row, from);
      const b = edge(row, to);
      selection.push([a.from, b.from, b.to, a.to]);
    }
    return { caret: null, selection };
  }
}

export const textEditing = new TextEditing(editor, selection, drawingLayer, textAdapter);
