/**
 * What closing the window asks, and how "Stop job and close" goes, kept out
 * of `CloseGuard.svelte` so it is unit-testable.
 */

/**
 * - `none`: close straight away.
 * - `unsaved`: offer to discard unsaved changes.
 * - `job`: a laser job runs; offer to stop it, then close.
 * - `busy`: something that can't be stopped from here (a save, an update
 *   install); wait for it.
 */
export type ClosePrompt = "none" | "unsaved" | "job" | "busy";

export interface CloseState {
  dirty: boolean;
  busy: readonly string[];
  locked: boolean;
}

/** What closing asks, given the lifecycle and the busy reason a running job holds. */
export function closePrompt(state: CloseState, jobReason: string): ClosePrompt {
  if (state.locked) return "busy";
  if (state.busy.some((reason) => reason !== jobReason)) return "busy";
  if (state.busy.includes(jobReason)) return "job";
  return state.dirty ? "unsaved" : "none";
}

export interface StopAndClose {
  /** Stops the job; resolves true once the laser is off and the machine idle. */
  stop: () => Promise<boolean>;
  /** Why `stop` failed. */
  error: () => string;
  /** Drops unsaved changes, which the user agreed to lose. */
  discard: () => void;
  /** Closes the window without asking again. */
  destroy: () => Promise<void>;
}

/**
 * "Stop job and close": stops the job and waits for the machine to confirm
 * the laser is off, and only then closes the window. Returns why it didn't
 * close, or null once closing.
 */
export async function stopJobAndClose(close: StopAndClose): Promise<string | null> {
  if (!(await close.stop())) return close.error() || "The job didn't stop.";
  close.discard();
  try {
    await close.destroy();
    return null;
  } catch (cause) {
    return String(cause);
  }
}
