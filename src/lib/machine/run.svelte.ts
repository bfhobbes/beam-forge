import type { JobProgress } from "#lib/bindings/JobProgress.js";
import type { JobState } from "#lib/bindings/JobState.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { RunPlan } from "#lib/bindings/gcode/RunPlan.js";
import type { Point } from "#lib/bindings/project/Point.js";
import {
  buildTimeline,
  clock,
  pointAfter,
  type PlaybackPoint,
  type Timeline,
} from "#lib/toolpath/timeline.js";
import { followHead, takenMoves, timeAtHead, toDocument } from "./follow.js";

/** How a design run ended, for the finish notice. */
export interface RunOutcome {
  state: JobState;
  /** Wall-clock time the job took. */
  actualMs: number;
  /** The estimate it started with. */
  estimateMs: number;
}

/** How far off the estimate was: `+0.05` when the job took 5% longer. Null without an estimate. */
export function estimateError(outcome: RunOutcome): number | null {
  return outcome.estimateMs > 0 ? outcome.actualMs / outcome.estimateMs - 1 : null;
}

/** `+5%`, `-12%`, `±0%`. */
export function formatError(error: number): string {
  const percent = Math.round(error * 100);
  return percent === 0 ? "±0%" : `${percent > 0 ? "+" : "−"}${Math.abs(percent)}%`;
}

/**
 * A finished run's time against its estimate, for the machine console, so
 * the estimator can be checked against the real machine. Null for a run
 * that didn't finish: it says nothing about the estimate.
 */
export function outcomeNote(name: string, outcome: RunOutcome): string | null {
  const error = estimateError(outcome);
  if (outcome.state.state !== "finished" || error === null) return null;
  return (
    `${name} finished in ${clock(outcome.actualMs / 1000)}, ` +
    `estimated ${clock(outcome.estimateMs / 1000)} (${formatError(error)})`
  );
}

function isDone(state: JobState): boolean {
  return state.state === "finished" || state.state === "stopped" || state.state === "failed";
}

/**
 * A design running on the machine, followed on its toolpath: moves done, the
 * head, the current layer and the time left. Fed the job's progress and the
 * machine's status reports.
 *
 * Times come from the toolpath timeline: the desktop app's estimate from
 * the machine profile's rates and accelerations.
 */
export class DesignRun {
  readonly plan: RunPlan;
  readonly timeline: Timeline;
  /** Moves finished, as far as the head's position shows. */
  completed = $state(0);
  /** The head on the design, from the latest status report. */
  head = $state.raw<Point | null>(null);
  acknowledged = $state(0);
  outcome = $state.raw<RunOutcome | null>(null);

  constructor(plan: RunPlan) {
    this.plan = plan;
    this.timeline = buildTimeline(plan.toolpath);
  }

  get name(): string {
    return this.plan.name;
  }

  get done(): boolean {
    return this.outcome !== null;
  }

  /** Estimated seconds for the whole job. */
  get estimate(): number {
    return this.timeline.duration;
  }

  /** Where the canvas draws the head, and how much of the path is done. */
  get point(): PlaybackPoint {
    const point = pointAfter(this.timeline, this.completed);
    return this.head && !this.done ? { ...point, head: this.head } : point;
  }

  /** Index into the timeline's layers of the layer being cut, or -1 before the first move. */
  get layer(): number {
    return this.point.layer;
  }

  /** Estimated seconds left, from where the head is. */
  get remaining(): number {
    if (this.done) return 0;
    return Math.max(0, this.estimate - timeAtHead(this.timeline, this.completed, this.head));
  }

  /** Follows the job's progress. Progress for another job is ignored. */
  onProgress(progress: JobProgress): void {
    if (progress.name !== this.name || this.done) return;
    this.acknowledged = progress.acknowledged;
    if (progress.state.state === "finished") this.completed = this.timeline.count;
    else this.#follow();
    if (isDone(progress.state)) {
      this.outcome = {
        state: progress.state,
        actualMs: progress.elapsedMs,
        estimateMs: Math.round(this.estimate * 1000),
      };
    }
  }

  /** Follows the head's reported work position. */
  onStatus(status: MachineStatus | null): void {
    if (this.done) return;
    const work = status?.workPosition;
    if (!work) return;
    this.head = toDocument(this.plan.placement, work);
    this.#follow();
  }

  #follow(): void {
    const taken = takenMoves(this.plan.moveLines, this.acknowledged);
    this.completed = followHead(this.timeline, taken, this.head, this.completed);
  }
}
