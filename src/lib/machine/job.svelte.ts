import type { JobProgress } from "#lib/bindings/JobProgress.js";
import type { JobState } from "#lib/bindings/JobState.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { OverrideKind } from "#lib/bindings/OverrideKind.js";
import type { Preflight } from "#lib/bindings/Preflight.js";
import { lifecycle as appLifecycle, type Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { jobAdapter, type DocumentJob, type FrameRequest, type JobAdapter } from "./job-adapter.js";
import { clampOverride } from "./overrides.js";
import { machineConsole } from "./console.svelte.js";
import { DesignRun, outcomeNote } from "./run.svelte.js";

/** Why close and updates are refused while a job runs. */
export const BUSY_REASON = "Running job";

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

export function isDone(state: JobState): boolean {
  return state.state === "finished" || state.state === "stopped" || state.state === "failed";
}

/** The job's state in a few words, e.g. `Paused` or `Stopped`. */
export function jobStateLabel(state: JobState): string {
  switch (state.state) {
    case "running":
      return "Running";
    case "paused":
      return "Paused";
    case "rejected":
      return "Line rejected";
    case "stopping":
      return "Stopping…";
    case "finished":
      return "Finished";
    case "stopped":
      return "Stopped";
    case "failed":
      return "Failed";
  }
}

/** `m:ss`, or `h:mm:ss` past an hour. */
export function formatElapsed(ms: number): string {
  const total = Math.floor(ms / 1000);
  const seconds = String(total % 60).padStart(2, "0");
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3600);
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}

/**
 * The GCode job as the UI sees it: its latest progress and the commands that
 * run, pause, resume, stop and override it. A running job holds the
 * lifecycle busy, so the window can't close and updates won't install
 * under it. A design run from the editor is also followed on its toolpath
 * (`run`).
 */
export class MachineJob {
  progress = $state.raw<JobProgress | null>(null);
  /** The design being run (or last run), followed on its toolpath; null for a file or a frame. */
  run = $state.raw<DesignRun | null>(null);
  /**
   * Override percentages asked for, by kind, until a job starts or ends.
   * The status reports' `overrides` say where the machine got to.
   */
  overrideTargets = $state.raw<Partial<Record<OverrideKind, number>>>({});
  /** The last failed command, until the next one. */
  error = $state("");

  readonly #adapter: JobAdapter;
  readonly #lifecycle: Lifecycle;
  #release: (() => void) | null = null;
  /** Progress events seen, to tell whether one arrived while a job started. */
  #updates = 0;

  readonly #log: (note: string) => void;

  /** `log` records each finished design run's time against its estimate. */
  constructor(adapter: JobAdapter, lifecycle: Lifecycle, log: (note: string) => void = () => {}) {
    this.#adapter = adapter;
    this.#lifecycle = lifecycle;
    this.#log = log;
  }

  /** True from the start of a job until its last progress. */
  get active(): boolean {
    return this.progress !== null && !isDone(this.progress.state);
  }

  get paused(): boolean {
    const state = this.progress?.state.state;
    return state === "paused" || state === "rejected";
  }

  /** Acknowledged lines as a fraction of the total, or null if unknown. */
  get fraction(): number | null {
    const total = this.progress?.total;
    if (!this.progress || !total) return null;
    return Math.min(1, this.progress.acknowledged / total);
  }

  /** Follows job progress. Returns the cleanup. */
  async start(): Promise<() => void> {
    const stop = await this.#adapter.onProgress((progress) => this.#update(progress));
    const current = await this.#adapter.current();
    // An event may have arrived meanwhile; it is newer.
    if (current && !this.progress) this.#update(current);
    return () => {
      stop();
      this.#setBusy(false);
    };
  }

  /** Asks for a file and streams it. */
  async runFile(): Promise<void> {
    await this.#run(() => this.#adapter.runFile(), true);
  }

  /**
   * Generates GCode from the design in memory and streams it, through the
   * same job path as a file, then follows it on its toolpath. Returns
   * whether it started; `error` says why not.
   */
  async runDocument(job: DocumentJob): Promise<boolean> {
    return this.#run(async () => {
      const seen = this.#updates;
      const plan = await this.#adapter.runDocument(job);
      const run = new DesignRun(plan);
      // The job's first progress may have arrived before its plan.
      if (this.#updates !== seen && this.progress) this.#follow(run, this.progress);
      this.run = run;
    }, true);
  }

  /** The pre-run checklist for `job`, or null if it couldn't be made; `error` says why. */
  async preflight(job: DocumentJob): Promise<Preflight | null> {
    this.error = "";
    try {
      return await this.#adapter.preflight(job);
    } catch (cause) {
      this.error = message(cause);
      return null;
    }
  }

  /**
   * For closing the app mid-job: stops the job and resolves once the machine
   * has turned the laser off and is idle. False if it couldn't confirm that;
   * `error` says why.
   */
  async stopForClose(): Promise<boolean> {
    return this.#run(() => this.#adapter.stopAndWait());
  }

  /** Frames the job on the bed. Returns whether it started; `error` says why not. */
  async frame(request: FrameRequest): Promise<boolean> {
    return this.#run(() => this.#adapter.frame(request), true);
  }

  /** Follows the head's reported position on a running design. */
  follow(status: MachineStatus | null): void {
    this.run?.onStatus(status);
  }

  /** Moves the feed or laser power override towards `percent`, live. */
  async setOverride(kind: OverrideKind, percent: number): Promise<void> {
    const target = clampOverride(percent);
    this.overrideTargets = { ...this.overrideTargets, [kind]: target };
    await this.#run(() => this.#adapter.setOverride(kind, target));
  }

  async pause(): Promise<void> {
    await this.#run(() => this.#adapter.pause());
  }

  async resume(): Promise<void> {
    await this.#run(() => this.#adapter.resume());
  }

  async stop(): Promise<void> {
    await this.#run(() => this.#adapter.stop());
  }

  async #run(command: () => Promise<unknown>, starts = false): Promise<boolean> {
    this.error = "";
    // A new job replaces the last design run, even if it fails to start.
    if (starts) {
      this.run = null;
      this.overrideTargets = {};
    }
    try {
      await command();
      return true;
    } catch (cause) {
      this.error = message(cause);
      return false;
    }
  }

  #update(progress: JobProgress): void {
    this.#updates++;
    this.progress = progress;
    if (this.run) this.#follow(this.run, progress);
    if (isDone(progress.state)) this.overrideTargets = {};
    this.#setBusy(!isDone(progress.state));
  }

  /** Feeds a design run its progress, logging how its time compared with the estimate once it finishes. */
  #follow(run: DesignRun, progress: JobProgress): void {
    const ended = run.done;
    run.onProgress(progress);
    if (ended || !run.outcome) return;
    const note = outcomeNote(run.name, run.outcome);
    if (note) this.#log(note);
  }

  #setBusy(busy: boolean): void {
    if (busy && !this.#release) {
      this.#release = this.#lifecycle.beginBusy(BUSY_REASON);
    } else if (!busy && this.#release) {
      this.#release();
      this.#release = null;
    }
  }
}

export const machineJob = new MachineJob(jobAdapter, appLifecycle, (note) =>
  machineConsole.note(note),
);
