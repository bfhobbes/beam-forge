import type { JobEstimate } from "#lib/bindings/plan/JobEstimate.js";
import { editor } from "#lib/document/editor.svelte.js";
import type { GcodeRequest } from "#lib/gcode/adapter.js";
import { jobSetup } from "#lib/gcode/job-setup.svelte.js";
import { machineSettings } from "#lib/machine/settings.svelte.js";
import { estimateAdapter, type EstimateAdapter } from "./adapter.js";

export interface JobTimeDeps {
  adapter: EstimateAdapter;
  /** What to time: the document, profile and the job dialog's placement. */
  request: () => GcodeRequest;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * How long the design would take to run, by layer: the desktop app's
 * estimate from the machine profile's rates and accelerations.
 */
export class JobTime {
  estimate = $state.raw<JobEstimate | null>(null);
  /** Why there is no estimate (nothing to cut, the browser preview…). */
  problem = $state("");

  readonly #deps: JobTimeDeps;
  #refreshes = 0;

  constructor(deps: JobTimeDeps) {
    this.#deps = deps;
  }

  get available(): boolean {
    return this.#deps.adapter.available;
  }

  /** Re-estimates the job. A newer refresh wins over an older one still in flight. */
  async refresh(): Promise<void> {
    const id = ++this.#refreshes;
    try {
      const estimate = await this.#deps.adapter.estimate(this.#deps.request());
      if (id !== this.#refreshes) return;
      this.estimate = estimate;
      this.problem = "";
    } catch (cause) {
      if (id !== this.#refreshes) return;
      this.estimate = null;
      this.problem = message(cause);
    }
  }
}

export const jobTime = new JobTime({
  adapter: estimateAdapter,
  request: () => ({
    document: editor.doc,
    profile: machineSettings.profile,
    placement: jobSetup.placement,
  }),
});
