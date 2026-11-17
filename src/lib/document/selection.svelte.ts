import { editor } from "./editor.svelte.js";
import type { DesignDocument } from "./model.js";

/**
 * The selected top-level objects. Selection is editor state, not part of the
 * document, so it never enters undo history or saved files. Ids that leave
 * the document are hidden rather than forgotten, so undoing a delete brings
 * the selection back with the objects.
 */
export class Selection {
  #ids = $state.raw<string[]>([]);
  #doc: () => DesignDocument;

  constructor(doc: () => DesignDocument) {
    this.#doc = doc;
  }

  /** Selected ids that are top-level objects in the document, in selection order. */
  get ids(): string[] {
    const present = new Set(this.#doc().objects.map((object) => object.id));
    return this.#ids.filter((id) => present.has(id));
  }

  has(id: string): boolean {
    return this.ids.includes(id);
  }

  set(ids: readonly string[]): void {
    this.#ids = [...new Set(ids)];
  }

  add(ids: readonly string[]): void {
    this.set([...this.ids, ...ids]);
  }

  toggle(id: string): void {
    const ids = this.ids;
    this.#ids = ids.includes(id) ? ids.filter((other) => other !== id) : [...ids, id];
  }

  selectAll(): void {
    this.set(this.#doc().objects.map((object) => object.id));
  }

  clear(): void {
    this.#ids = [];
  }
}

export const selection = new Selection(() => editor.doc);
