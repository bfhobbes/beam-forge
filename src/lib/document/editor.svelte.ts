import { lifecycle as appLifecycle, type Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { applyCommand, compose, type Command } from "./commands.js";
import { emptyDocument, type DesignDocument } from "./model.js";

/** One undo step: the edit and the command that reverses it. */
interface Step {
  forward: Command;
  inverse: Command;
}

export interface ExecuteOptions {
  /**
   * Consecutive commands with the same key become one undo step, e.g. every
   * update of a drag. Call `endCoalesce` when the gesture ends.
   */
  coalesce?: string;
}

/**
 * Holds the open design document and its command-based undo/redo history.
 * Every change marks `lifecycle.dirty` and is refused while
 * `lifecycle.canEdit()` is false.
 */
export class DocumentEditor {
  doc = $state.raw<DesignDocument>(emptyDocument());
  #undo = $state.raw<Step[]>([]);
  #redo = $state.raw<Step[]>([]);
  #coalescing: string | null = null;
  #lifecycle: Lifecycle;

  constructor(lifecycle: Lifecycle, doc: DesignDocument = emptyDocument()) {
    this.#lifecycle = lifecycle;
    this.doc = doc;
  }

  get canUndo(): boolean {
    return this.#undo.length > 0;
  }

  get canRedo(): boolean {
    return this.#redo.length > 0;
  }

  /**
   * Applies a command as a new undo step, or folds it into the current one
   * when `coalesce` matches. Returns false if editing is not allowed; throws
   * `DocumentError` (leaving everything unchanged) if the command is invalid.
   */
  execute(command: Command, { coalesce }: ExecuteOptions = {}): boolean {
    if (!this.#lifecycle.canEdit()) return false;
    const { doc, inverse } = applyCommand(this.doc, command);
    const top = this.#undo.at(-1);
    if (top && coalesce !== undefined && coalesce === this.#coalescing) {
      const merged = {
        forward: compose(top.forward, command),
        inverse: compose(inverse, top.inverse),
      };
      this.#undo = [...this.#undo.slice(0, -1), merged];
    } else {
      this.#undo = [...this.#undo, { forward: command, inverse }];
    }
    this.#coalescing = coalesce ?? null;
    this.#redo = [];
    this.#commit(doc);
    return true;
  }

  /** Ends the current coalesced step, so the next command starts a new one. */
  endCoalesce(): void {
    this.#coalescing = null;
  }

  undo(): boolean {
    const step = this.#undo.at(-1);
    if (!step || !this.#lifecycle.canEdit()) return false;
    this.#commit(applyCommand(this.doc, step.inverse).doc);
    this.#coalescing = null;
    this.#undo = this.#undo.slice(0, -1);
    this.#redo = [...this.#redo, step];
    return true;
  }

  redo(): boolean {
    const step = this.#redo.at(-1);
    if (!step || !this.#lifecycle.canEdit()) return false;
    this.#commit(applyCommand(this.doc, step.forward).doc);
    this.#coalescing = null;
    this.#redo = this.#redo.slice(0, -1);
    this.#undo = [...this.#undo, step];
    return true;
  }

  /** Replaces the document (new or opened from disk) with a fresh, clean history. */
  load(doc: DesignDocument): boolean {
    if (!this.#lifecycle.canEdit()) return false;
    this.doc = doc;
    this.#undo = [];
    this.#redo = [];
    this.#coalescing = null;
    this.#lifecycle.dirty = false;
    return true;
  }

  #commit(doc: DesignDocument): void {
    this.doc = doc;
    this.#lifecycle.dirty = true;
  }
}

export const editor = new DocumentEditor(appLifecycle);
