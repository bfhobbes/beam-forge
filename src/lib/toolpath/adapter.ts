import { invoke } from "@tauri-apps/api/core";
import type { Toolpath } from "#lib/bindings/gcode/Toolpath.js";
import type { JobEstimate } from "#lib/bindings/plan/JobEstimate.js";
import { desktop } from "#lib/desktop/platform.js";
import type { GcodeRequest } from "#lib/gcode/adapter.js";

/** Typed access to `gcode_toolpath` in `src-tauri/src/gcode.rs`. */
export interface ToolpathAdapter {
  /** False in the browser preview, which has no planner. */
  readonly available: boolean;
  /** The moves the job's GCode is written from, in document coordinates. */
  toolpath(request: GcodeRequest): Promise<Toolpath>;
}

export const desktopToolpath: ToolpathAdapter = {
  available: true,
  toolpath: (request) => invoke<Toolpath>("gcode_toolpath", { ...request }),
};

export const PREVIEW_NO_TOOLPATH =
  "Browser preview: the toolpath is planned by the desktop app (npm run desktop:dev).";

// The browser preview has no Rust side, so it can't plan the job.
export const previewToolpath: ToolpathAdapter = {
  available: false,
  toolpath: async () => {
    throw new Error(PREVIEW_NO_TOOLPATH);
  },
};

export const toolpathAdapter = desktop ? desktopToolpath : previewToolpath;

/** Typed access to `gcode_estimate` in `src-tauri/src/gcode.rs`. */
export interface EstimateAdapter {
  /** False in the browser preview, which has no planner. */
  readonly available: boolean;
  /** How long the job would take on the machine, by layer. */
  estimate(request: GcodeRequest): Promise<JobEstimate>;
}

export const desktopEstimate: EstimateAdapter = {
  available: true,
  estimate: (request) => invoke<JobEstimate>("gcode_estimate", { ...request }),
};

// The browser preview has no Rust side, so it can't time the job.
export const previewEstimate: EstimateAdapter = {
  available: false,
  estimate: async () => {
    throw new Error(PREVIEW_NO_TOOLPATH);
  },
};

export const estimateAdapter = desktop ? desktopEstimate : previewEstimate;
