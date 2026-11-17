import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { Position } from "#lib/bindings/Position.js";
import type { Preflight } from "#lib/bindings/Preflight.js";
import type { FrameShape } from "#lib/bindings/gcode/FrameShape.js";
import type { JobAnchor } from "#lib/bindings/gcode/JobAnchor.js";
import type { JobPlacement } from "#lib/bindings/gcode/JobPlacement.js";
import type { JobSummary } from "#lib/bindings/gcode/JobSummary.js";
import type { StartFrom } from "#lib/bindings/gcode/StartFrom.js";
import type { DesignDocument } from "#lib/bindings/project/DesignDocument.js";
import { lifecycle, type Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { editor } from "#lib/document/editor.svelte.js";
import { machine } from "#lib/machine/connection.svelte.js";
import type { DocumentJob, FrameRequest } from "#lib/machine/job-adapter.js";
import { machineJob } from "#lib/machine/job.svelte.js";
import {
  acknowledgements,
  blocked,
  checklist,
  checklistNeeded,
  preflightPrefs,
  withFraming,
  type ChecklistItem,
  type PreflightPrefs,
} from "#lib/machine/preflight.svelte.js";
import { machineSettings } from "#lib/machine/settings.svelte.js";
import { project } from "#lib/project/project.svelte.js";
import { gcodeAdapter, type GcodeAdapter } from "./adapter.js";

const STORAGE_KEY = "beam-forge.job-placement";
const FRAME_SHAPE_KEY = "beam-forge.frame-shape";

/** Why close and updates are refused while a file is written. */
export const EXPORT_BUSY = "Exporting GCode";

/** The 3 × 3 job origin grid, rows from the top. */
export const ANCHORS: readonly JobAnchor[] = [
  "topLeft",
  "top",
  "topRight",
  "left",
  "center",
  "right",
  "bottomLeft",
  "bottom",
  "bottomRight",
];

export const START_FROM: readonly { value: StartFrom; label: string }[] = [
  { value: "absolute", label: "Absolute coordinates" },
  { value: "currentPosition", label: "Current position" },
  { value: "userOrigin", label: "User origin" },
];

export const DEFAULT_PLACEMENT: JobPlacement = { startFrom: "absolute", anchor: "bottomLeft" };

/** What the dialog is for: writing a file, or sending the job to the machine. */
export type JobSetupMode = "export" | "run";

type Storage = Pick<globalThis.Storage, "getItem" | "setItem">;

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function isPlacement(value: unknown): value is JobPlacement {
  if (typeof value !== "object" || value === null) return false;
  const { startFrom, anchor } = value as Record<string, unknown>;
  return (
    START_FROM.some((option) => option.value === startFrom) && ANCHORS.includes(anchor as JobAnchor)
  );
}

export interface JobSetupDeps {
  adapter: GcodeAdapter;
  /** Starts the generated job through the Job panel's job. */
  run: (job: DocumentJob) => Promise<boolean>;
  /** The pre-run checks for the job; null if they couldn't be run (`runError` says why). */
  preflight: (job: DocumentJob) => Promise<Preflight | null>;
  /** Whether the checklist always shows, and asks about framing. */
  prefs: PreflightPrefs;
  /** Frames the job on the bed through the Job panel's job. */
  frame: (request: FrameRequest) => Promise<boolean>;
  /** Why the last `run` or `frame` failed. */
  runError: () => string;
  /** A job (or frame) is running. */
  jobActive: () => boolean;
  lifecycle: Lifecycle;
  document: () => DesignDocument;
  profile: () => MachineProfile | null;
  /** Saves changes to the active profile; false if it couldn't. */
  saveProfile: (changes: Partial<MachineProfile>) => Promise<boolean>;
  connected: () => boolean;
  /** The head's work position from the latest status report. */
  position: () => Position | null;
  /** The job's name: the project's. */
  name: () => string;
  storage?: Storage;
}

/** How a frame runs, in words, so the user knows whether the laser fires. */
export function frameLaserLabel(profile: MachineProfile | null): string {
  const power = profile?.framePower ?? 0;
  return power > 0 ? `Low-power frame: laser on at ${power}% (M4)` : "Laser off while framing";
}

/**
 * The job dialog behind "Export GCode…" and "Start job…": where the job goes
 * (start from and job origin), what it would do, and the export or run.
 */
export class JobSetup {
  /** Which dialog is open, or null. */
  mode = $state<JobSetupMode | null>(null);
  placement = $state.raw<JobPlacement>(DEFAULT_PLACEMENT);
  summary = $state.raw<JobSummary | null>(null);
  /** Why the job can't be generated as placed, until it can. */
  problem = $state("");
  /** A failed export or run. */
  error = $state("");
  /** Where the last export went. */
  notice = $state("");
  busy = $state(false);
  /** What Frame traces: the bounding box or the rubber band. */
  frameShape = $state<FrameShape>("box");
  /**
   * The pre-run checklist, while it waits for the user to start the job
   * or go back. Null otherwise, including when every check passed.
   */
  checklist = $state.raw<ChecklistItem[] | null>(null);

  readonly #deps: JobSetupDeps;
  #refreshes = 0;
  /** What was framed last: the document, and the rest of what places it. */
  #framed: { doc: DesignDocument; key: string } | null = null;

  constructor(deps: JobSetupDeps) {
    this.#deps = deps;
    if (deps.storage?.getItem(FRAME_SHAPE_KEY) === "rubberBand") this.frameShape = "rubberBand";
    const saved = deps.storage?.getItem(STORAGE_KEY);
    if (saved) {
      try {
        const placement: unknown = JSON.parse(saved);
        if (isPlacement(placement)) this.placement = placement;
      } catch {
        // A damaged setting falls back to the default.
      }
    }
  }

  get available(): boolean {
    return this.#deps.adapter.available;
  }

  /** Why `startFrom` can't be used right now, or null if it can. */
  unavailable(startFrom: StartFrom): string | null {
    switch (startFrom) {
      case "absolute":
        return null;
      case "currentPosition":
        if (!this.#deps.connected()) return "Connect to a machine to start from where the head is.";
        if (!this.#deps.position()) return "Waiting for the machine to report the head's position.";
        return null;
      case "userOrigin":
        if (this.#deps.profile()?.userOrigin) return null;
        return this.#deps.connected()
          ? 'No user origin yet. Move the head, then choose "Set user origin here".'
          : "No user origin yet. Connect, move the head and set one.";
    }
  }

  /** True when the job origin grid matters: not for absolute coordinates. */
  get anchored(): boolean {
    return this.placement.startFrom !== "absolute";
  }

  /** The user origin can be set from the head's position. */
  get canSetOrigin(): boolean {
    return (
      this.#deps.connected() && this.#deps.position() !== null && this.#deps.profile() !== null
    );
  }

  get userOrigin(): MachineProfile["userOrigin"] {
    return this.#deps.profile()?.userOrigin ?? null;
  }

  /** Whether export or start can go ahead: a job was generated (and, to run, a machine is connected). */
  get canConfirm(): boolean {
    if (this.busy || !this.available || this.problem || !this.summary) return false;
    if (this.mode === "run" && this.checklist) {
      return this.#deps.connected() && !blocked(this.checklist);
    }
    return this.mode !== "run" || this.#deps.connected();
  }

  /** The job, as it would be placed now, was framed since it last changed. */
  get framed(): boolean {
    const framed = this.#framed;
    return (
      framed !== null && framed.doc === this.#deps.document() && framed.key === this.#frameKey()
    );
  }

  /** Frame can go ahead: connected, nothing running, and the job can be placed. */
  get canFrame(): boolean {
    if (this.busy || !this.available || !this.#deps.connected() || this.#deps.jobActive()) {
      return false;
    }
    return this.mode === null || (!this.problem && this.summary !== null);
  }

  setFrameShape(shape: FrameShape): void {
    this.frameShape = shape;
    this.#deps.storage?.setItem(FRAME_SHAPE_KEY, shape);
  }

  /**
   * Traces the job's outline on the bed, placed as the job would be, with
   * the laser off unless the profile sets a low frame power. Leaves the
   * dialog open, so the job can start next.
   */
  async frame(): Promise<boolean> {
    if (this.busy) return false;
    this.busy = true;
    this.error = "";
    try {
      const framed = await this.#deps.frame({ ...this.#request(), shape: this.frameShape });
      if (!framed) {
        this.error = this.#deps.runError();
        return false;
      }
      this.#framed = { doc: this.#deps.document(), key: this.#frameKey() };
      if (this.checklist) this.checklist = withFraming(this.checklist, true);
      return true;
    } finally {
      this.busy = false;
    }
  }

  open(mode: JobSetupMode): void {
    this.mode = mode;
    this.checklist = null;
    this.error = "";
    this.notice = "";
    void this.refresh();
  }

  close(): void {
    if (this.busy) return;
    this.mode = null;
    this.checklist = null;
  }

  /** Leaves the checklist for the job's settings, without starting. */
  back(): void {
    if (!this.busy) this.checklist = null;
  }

  setStartFrom(startFrom: StartFrom): void {
    this.#place({ ...this.placement, startFrom });
  }

  setAnchor(anchor: JobAnchor): void {
    this.#place({ ...this.placement, anchor });
  }

  /** Makes the head's position the user origin, and starts from it. */
  async setUserOrigin(): Promise<void> {
    const position = this.#deps.position();
    if (!position) return;
    const saved = await this.#deps.saveProfile({ userOrigin: { x: position.x, y: position.y } });
    if (!saved) {
      this.error = "Couldn't save the user origin to the machine profile.";
      return;
    }
    this.#place({ ...this.placement, startFrom: "userOrigin" });
  }

  /** Regenerates the summary for the current document and placement. */
  async refresh(): Promise<void> {
    if (!this.mode) return;
    const id = ++this.#refreshes;
    const blocked = this.unavailable(this.placement.startFrom);
    if (blocked) {
      this.summary = null;
      this.problem = blocked;
      return;
    }
    try {
      const summary = await this.#deps.adapter.summary(this.#request());
      if (id !== this.#refreshes) return;
      this.summary = summary;
      this.problem = "";
    } catch (cause) {
      if (id !== this.#refreshes) return;
      this.summary = null;
      this.problem = message(cause);
    }
  }

  /** Exports or starts the job, then closes the dialog. */
  async confirm(): Promise<void> {
    if (!this.mode || this.busy) return;
    this.busy = true;
    this.error = "";
    try {
      if (this.mode === "export") await this.#export();
      else await this.#run();
    } finally {
      this.busy = false;
    }
  }

  async #export(): Promise<void> {
    const release = this.#deps.lifecycle.beginBusy(EXPORT_BUSY);
    try {
      const path = await this.#deps.adapter.export(this.#request(), this.#deps.name());
      if (path === null) return;
      this.notice = `Exported ${path}`;
      this.mode = null;
    } catch (cause) {
      this.error = message(cause);
    } finally {
      release();
    }
  }

  /**
   * Runs the pre-run checks, then starts the job if they all pass, or shows
   * them. Confirming the checklist starts it, acknowledging its warnings.
   */
  async #run(): Promise<void> {
    const job = { ...this.#request(), name: this.#deps.name() };
    let items = this.checklist;
    if (!items) {
      const checks = await this.#deps.preflight(job);
      if (!checks) {
        this.error = this.#deps.runError();
        return;
      }
      const prefs = this.#deps.prefs;
      items = checklist(checks, prefs.askFramed ? this.framed : null);
      if (checklistNeeded(items, prefs.alwaysShow)) {
        this.checklist = items;
        return;
      }
    }
    if (blocked(items)) return;
    const started = await this.#deps.run({ ...job, acknowledged: acknowledgements(items) });
    // Refused or not, the checks run again on the next start.
    this.checklist = null;
    if (started) this.mode = null;
    else this.error = this.#deps.runError();
  }

  /** What else placing the job depends on, to tell whether it was framed as it is now. */
  #frameKey(): string {
    const { placement, profile } = this.#request();
    const position = placement.startFrom === "currentPosition" ? this.#deps.position() : null;
    return JSON.stringify({ placement, profile, position });
  }

  #request() {
    return {
      document: this.#deps.document(),
      profile: this.#deps.profile(),
      placement: this.placement,
    };
  }

  #place(placement: JobPlacement): void {
    this.placement = placement;
    this.#deps.storage?.setItem(STORAGE_KEY, JSON.stringify(placement));
    void this.refresh();
  }
}

export const jobSetup = new JobSetup({
  adapter: gcodeAdapter,
  run: (job) => machineJob.runDocument(job),
  preflight: (job) => machineJob.preflight(job),
  prefs: preflightPrefs,
  frame: (request) => machineJob.frame(request),
  runError: () => machineJob.error,
  jobActive: () => machineJob.active,
  lifecycle,
  document: () => editor.doc,
  profile: () => machineSettings.profile,
  saveProfile: (changes) => machineSettings.saveProfile(changes),
  connected: () => machine.connected,
  position: () => machine.status?.workPosition ?? null,
  name: () => project.name,
  storage: globalThis.window?.localStorage,
});
