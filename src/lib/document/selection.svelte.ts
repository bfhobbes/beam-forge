import { editor as appEditor, type DocumentEditor } from "./editor.svelte.js";

/**
 * The selected top-level objects. Selection is editor state, not part of the
 * document, so it never enters undo history or saved files. Ids that leave
 * the document are hidden rather than forgotten, so undoing a delete brings
 * the selection back with the objects. Loading a new document (New, Open,
 * recovery) empties it, even if the new document reuses the same ids.
 */
export class Selection {
  #ids = $state.raw<string[]>([]);
  /** The editor generation `#ids` belongs to; a later `load` makes them stale. */
  #generation = $state(0);
  #editor: Pick<DocumentEditor, "doc" | "generation">;

  constructor(editor: Pick<DocumentEditor, "doc" | "generation">) {
    this.#editor = editor;
    this.#generation = editor.generation;
  }

  /** Selected ids that are top-level objects in the document, in selection order. */
  get ids(): string[] {
    if (this.#generation !== this.#editor.generation) return [];
    const present = new Set(this.#editor.doc.objects.map((object) => object.id));
    return this.#ids.filter((id) => present.has(id));
  }

  has(id: string): boolean {
    return this.ids.includes(id);
  }

  set(ids: readonly string[]): void {
    this.#ids = [...new Set(ids)];
    this.#generation = this.#editor.generation;
  }

  add(ids: readonly string[]): void {
    this.set([...this.ids, ...ids]);
  }

  toggle(id: string): void {
    const ids = this.ids;
    this.set(ids.includes(id) ? ids.filter((other) => other !== id) : [...ids, id]);
  }

  selectAll(): void {
    this.set(this.#editor.doc.objects.map((object) => object.id));
  }

  clear(): void {
    this.#ids = [];
  }
}

export const selection = new Selection(appEditor);
