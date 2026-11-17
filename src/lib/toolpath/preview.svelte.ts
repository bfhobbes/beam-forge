import type { Toolpath } from "#lib/bindings/gcode/Toolpath.js";
import type { Point } from "#lib/bindings/project/Point.js";
import { editor } from "#lib/document/editor.svelte.js";
import type { GcodeRequest } from "#lib/gcode/adapter.js";
import { jobSetup } from "#lib/gcode/job-setup.svelte.js";
import { machineSettings } from "#lib/machine/settings.svelte.js";
import { toolpathAdapter, type ToolpathAdapter } from "./adapter.js";
import {
  buildTimeline,
  pointAfter,
  pointAt,
  type PlaybackPoint,
  type Timeline,
} from "./timeline.js";

/** Playback speed multipliers. */
export const SPEEDS: readonly number[] = [1, 2, 5, 10, 50];

/**
 * Drives the view from outside instead of the playback clock, e.g. a running
 * job: `completed` moves are done; `head`, when known, is where the
 * machine reports the head.
 */
export interface ExternalProgress {
  completed: number;
  head?: Point | null;
}

export interface ToolpathPreviewDeps {
  adapter: ToolpathAdapter;
  /** What to plan: the document, profile and the job dialog's placement. */
  request: () => GcodeRequest;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The toolpath view: the planned job (the same moves its GCode is written
 * from), a playback clock over it, and whether the canvas shows it.
 */
export class ToolpathPreview {
  /** The canvas shows the toolpath instead of the design. */
  shown = $state(false);
  toolpath = $state.raw<Toolpath | null>(null);
  /** Why there is no toolpath (nothing to cut, the browser preview…). */
  problem = $state("");
  loading = $state(false);
  /** Playback position, seconds into the job. */
  time = $state(0);
  playing = $state(false);
  speed = $state(1);
  /** Set to follow something else (a running job) instead of the clock. */
  external = $state.raw<ExternalProgress | null>(null);

  readonly timeline: Timeline | null = $derived(
    this.toolpath ? buildTimeline(this.toolpath) : null,
  );

  /** Where the head is drawn and how much of the path is done. */
  readonly progress: PlaybackPoint | null = $derived.by(() => {
    const timeline = this.timeline;
    if (!timeline) return null;
    if (this.external) {
      const point = pointAfter(timeline, this.external.completed);
      return this.external.head ? { ...point, head: this.external.head } : point;
    }
    return pointAt(timeline, this.time);
  });

  readonly #deps: ToolpathPreviewDeps;
  #refreshes = 0;

  constructor(deps: ToolpathPreviewDeps) {
    this.#deps = deps;
  }

  get available(): boolean {
    return this.#deps.adapter.available;
  }

  get duration(): number {
    return this.timeline?.duration ?? 0;
  }

  get atEnd(): boolean {
    return this.time >= this.duration;
  }

  toggle(shown = !this.shown): void {
    this.shown = shown;
    if (!shown) this.playing = false;
  }

  /**
   * Shows a running job's toolpath, followed by its progress instead of the
   * playback clock, until `unfollow`. The design isn't re-planned meanwhile:
   * the progress counts moves of this toolpath.
   */
  follow(toolpath: Toolpath, progress: ExternalProgress): void {
    if (this.toolpath !== toolpath) {
      ++this.#refreshes;
      this.toolpath = toolpath;
      this.problem = "";
      this.loading = false;
      this.shown = true;
    }
    this.playing = false;
    this.external = progress;
  }

  /** Back to playback, showing the whole path, and re-planned from the design. */
  unfollow(): void {
    if (!this.external) return;
    this.external = null;
    this.time = this.duration;
    if (this.shown) void this.refresh();
  }

  /** Re-plans the job. A newer refresh wins over an older one still in flight. */
  async refresh(): Promise<void> {
    if (this.external) return;
    const id = ++this.#refreshes;
    this.loading = true;
    try {
      const toolpath = await this.#deps.adapter.toolpath(this.#deps.request());
      if (id !== this.#refreshes) return;
      const wasAtEnd = !this.toolpath || this.atEnd;
      this.toolpath = toolpath;
      this.problem = "";
      // A finished preview stays finished, showing the whole new path.
      this.time = wasAtEnd ? this.duration : Math.min(this.time, this.duration);
    } catch (cause) {
      if (id !== this.#refreshes) return;
      this.toolpath = null;
      this.playing = false;
      this.problem = message(cause);
    } finally {
      if (id === this.#refreshes) this.loading = false;
    }
  }

  /** Plays from here, or from the start once finished; pauses if playing. */
  playPause(): void {
    if (this.playing) {
      this.playing = false;
      return;
    }
    if (!this.timeline || this.duration <= 0) return;
    if (this.atEnd) this.time = 0;
    this.playing = true;
  }

  /** Moves the playhead, pausing playback. */
  scrub(time: number): void {
    this.playing = false;
    this.time = Math.min(this.duration, Math.max(0, time));
  }

  setSpeed(speed: number): void {
    this.speed = speed;
  }

  /** Advances playback by `ms` of wall-clock time; stops at the end. */
  tick(ms: number): void {
    if (!this.playing) return;
    this.time = Math.min(this.duration, this.time + (ms / 1000) * this.speed);
    if (this.atEnd) this.playing = false;
  }
}

export const toolpathPreview = new ToolpathPreview({
  adapter: toolpathAdapter,
  request: () => ({
    document: editor.doc,
    profile: machineSettings.profile,
    placement: jobSetup.placement,
  }),
});
