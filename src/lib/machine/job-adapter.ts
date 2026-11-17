import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { JobProgress } from "#lib/bindings/JobProgress.js";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { OverrideKind } from "#lib/bindings/OverrideKind.js";
import type { Preflight } from "#lib/bindings/Preflight.js";
import type { PreflightCheckId } from "#lib/bindings/PreflightCheckId.js";
import type { FrameShape } from "#lib/bindings/gcode/FrameShape.js";
import type { JobPlacement } from "#lib/bindings/gcode/JobPlacement.js";
import type { RunPlan } from "#lib/bindings/gcode/RunPlan.js";
import type { DesignDocument } from "#lib/bindings/project/DesignDocument.js";
import { desktop } from "#lib/desktop/platform.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";

/** What `runDocument` generates GCode from. */
export interface DocumentJob {
  document: DesignDocument;
  /** The active machine profile; GRBL defaults when there is none yet. */
  profile: MachineProfile | null;
  placement: JobPlacement;
  /** Shown in the Job panel, e.g. the project name. */
  name: string;
  /**
   * Pre-run warnings the user has seen and accepted. The run refuses if any
   * other check warns, or any blocks, by the time it would start.
   */
  acknowledged?: PreflightCheckId[];
}

/** What `frame` traces: the job `runDocument` would run, placed the same way. */
export interface FrameRequest {
  document: DesignDocument;
  profile: MachineProfile | null;
  placement: JobPlacement;
  /** The bounding box, or the rubber band (convex hull). */
  shape: FrameShape;
}

/** Typed access to the job commands and events in `src-tauri/src/machine/job.rs`. */
export interface JobAdapter {
  /** Asks for a GCode file and streams it. The job's name, or null if cancelled. */
  runFile(): Promise<string | null>;
  /** The pre-run checklist for the job `runDocument` would run. */
  preflight(job: DocumentJob): Promise<Preflight>;
  /**
   * Generates GCode from the design in memory and streams it. The plan to
   * follow it on the canvas with.
   */
  runDocument(job: DocumentJob): Promise<RunPlan>;
  /**
   * Traces the job's outline with the laser off (or at the profile's
   * low frame power), as a job named "Frame". The job's name.
   */
  frame(request: FrameRequest): Promise<string>;
  /** The running job's progress, or the last job's final progress. */
  current(): Promise<JobProgress | null>;
  pause(): Promise<void>;
  /** Resumes a paused job, or continues past a rejected line. */
  resume(): Promise<void>;
  /** Feed hold, soft reset, then `M5`: the laser off and the machine idle. */
  stop(): Promise<void>;
  /** Stops any running job and resolves once the laser is off and the machine idle. */
  stopAndWait(): Promise<void>;
  /** Moves the feed or laser power override towards `percent` (10-200); status reports show where it got to. */
  setOverride(kind: OverrideKind, percent: number): Promise<void>;
  onProgress(handler: (progress: JobProgress) => void): Promise<() => void>;
}

export const desktopJob: JobAdapter = {
  runFile: () => invoke<string | null>("machine_run_file"),
  preflight: ({ document, profile, placement, name }) =>
    invoke<Preflight>("machine_preflight", { document, profile, placement, name }),
  runDocument: (job) => invoke<RunPlan>("machine_run_document", { ...job }),
  frame: (request) => invoke<string>("machine_frame", { ...request }),
  current: () => invoke<JobProgress | null>("machine_job"),
  pause: () => invoke<void>("machine_job_pause"),
  resume: () => invoke<void>("machine_job_resume"),
  stop: () => invoke<void>("machine_job_stop"),
  stopAndWait: () => invoke<void>("machine_job_stop_wait"),
  setOverride: (kind, percent) => invoke<void>("machine_override", { kind, percent }),
  onProgress: (handler) => listen<JobProgress>("machine:job", (event) => handler(event.payload)),
};

// The browser preview has no machine to stream to.
export const previewJob: JobAdapter = {
  runFile: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  preflight: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  runDocument: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  frame: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  current: async () => null,
  pause: async () => {},
  resume: async () => {},
  stop: async () => {},
  stopAndWait: async () => {},
  setOverride: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  onProgress: async () => () => {},
};

export const jobAdapter = desktop ? desktopJob : previewJob;
