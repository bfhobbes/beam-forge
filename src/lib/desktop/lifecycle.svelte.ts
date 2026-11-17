/**
 * One application lifecycle policy shared by window close, updates and (later)
 * project saves and machine jobs.
 *
 * - dirty: unsaved document changes. Close asks before discarding them.
 * - busy: work that must not be interrupted (saving, a running laser job).
 *   Close and update installation are refused while anything is busy.
 * - install lock: held while an update installs; blocks new edits and saves.
 */
export class Lifecycle {
  dirty = $state(false);
  #busy = $state<string[]>([]);
  locked = $state(false);

  get busy(): readonly string[] {
    return this.#busy;
  }

  /** Close without asking only when nothing would be lost or interrupted. */
  get canClose(): boolean {
    return !this.dirty && this.#busy.length === 0 && !this.locked;
  }

  /** Marks work in progress until the returned release function is called. */
  beginBusy(reason: string): () => void {
    this.#busy = [...this.#busy, reason];
    let released = false;
    return () => {
      if (released) return;
      released = true;
      const index = this.#busy.indexOf(reason);
      if (index !== -1) this.#busy = this.#busy.toSpliced(index, 1);
    };
  }

  /** Edits and saves call this first; it fails while an update installs. */
  canEdit(): boolean {
    return !this.locked;
  }

  /**
   * Synchronously takes the install lock if the app is clean and idle.
   * Returns the release function, or null when installing would lose work.
   */
  acquireInstallLock(): (() => void) | null {
    if (!this.canClose) return null;
    this.locked = true;
    return () => {
      this.locked = false;
    };
  }
}

export const lifecycle = new Lifecycle();
