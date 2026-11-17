import type { ProfileRef } from "#lib/bindings/project/ProfileRef.js";
import type { Project } from "#lib/bindings/project/Project.js";
import type { RecentProject } from "#lib/bindings/project/RecentProject.js";
import type { Recovery } from "#lib/bindings/project/Recovery.js";
import type { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import type { DocumentEditor } from "#lib/document/editor.svelte.js";
import { emptyDocument, type DesignDocument } from "#lib/document/model.js";
import type { ProjectAdapter } from "./adapter.js";

/** The `.bfp` format this build writes; Rust migrates older files on open. */
export const FORMAT_VERSION = 2;

/** How often unsaved work is copied to the recovery file. */
export const RECOVERY_INTERVAL_MS = 2 * 60 * 1000;

export const UNTITLED = "Untitled";

/** The answer to "save your changes first?". */
export type UnsavedChoice = "save" | "discard" | "cancel";

export interface ProjectSessionOptions {
  adapter: ProjectAdapter;
  editor: Pick<DocumentEditor, "doc" | "load">;
  lifecycle: Lifecycle;
  /** The active machine profile, saved as the project's machine. */
  profile: () => ProfileRef | null;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The open project: which file the document belongs to, and New / Open /
 * Save / Save As over the editor's document. Asks before discarding unsaved
 * changes, keeps the recent list, and copies unsaved work to a recovery file
 * every couple of minutes so a crash loses little.
 */
export class ProjectSession {
  /** The project's file; null until it is first saved. */
  path = $state<string | null>(null);
  name = $state(UNTITLED);
  recent = $state.raw<RecentProject[]>([]);
  /** The last failed open or save, shown until dismissed or the next attempt. */
  error = $state("");
  /** Set while asking whether to save unsaved changes; `answerUnsaved` resolves it. */
  unsaved = $state<{ action: string } | null>(null);
  /** Unsaved work from a session that didn't close cleanly, offered back at launch. */
  recovery = $state.raw<Recovery | null>(null);

  readonly #adapter: ProjectAdapter;
  readonly #editor: ProjectSessionOptions["editor"];
  readonly #lifecycle: Lifecycle;
  readonly #activeProfile: () => ProfileRef | null;
  /** The machine the open file was made for, kept when no profile is active. */
  #fileProfile: ProfileRef | null = null;
  #resolveUnsaved: ((choice: UnsavedChoice) => void) | null = null;
  /** The document last copied to the recovery file. */
  #recovered: DesignDocument | null = null;
  /** A file to open once the recovery offer is answered. */
  #launchFile: string | null = null;

  constructor({ adapter, editor, lifecycle, profile }: ProjectSessionOptions) {
    this.#adapter = adapter;
    this.#editor = editor;
    this.#lifecycle = lifecycle;
    this.#activeProfile = profile;
  }

  get available(): boolean {
    return this.#adapter.available;
  }

  /** The window title: the file name, marked while there are unsaved changes. */
  get title(): string {
    return `${this.#lifecycle.dirty ? "● " : ""}${this.name} – Beam Forge`;
  }

  /** The project as it would be saved now. */
  snapshot(document: DesignDocument = this.#editor.doc): Project {
    return {
      formatVersion: FORMAT_VERSION,
      document,
      machineProfile: this.#activeProfile() ?? this.#fileProfile,
    };
  }

  /**
   * Loads the recent list, then offers a recovery copy or opens the file the
   * app was launched with. Returns a function that stops the recovery timer.
   */
  async start(interval = RECOVERY_INTERVAL_MS): Promise<() => void> {
    const timer = setInterval(() => void this.writeRecovery(), interval);
    try {
      this.recent = await this.#adapter.recent();
      this.#launchFile = await this.#adapter.launchFile();
      this.recovery = await this.#adapter.recovery();
      if (!this.recovery) await this.#openLaunchFile();
    } catch (cause) {
      this.error = message(cause);
    }
    return () => clearInterval(timer);
  }

  /** Starts an empty, untitled project. */
  async newProject(): Promise<boolean> {
    if (!(await this.#confirmDiscard("starting a new project"))) return false;
    if (!this.#editor.load(emptyDocument())) return false;
    this.#setFile(null, UNTITLED, null);
    await this.#clearRecovery();
    return true;
  }

  /** Opens `path`, or asks which file to open. */
  async open(path?: string): Promise<boolean> {
    if (!this.#lifecycle.canEdit()) return false;
    if (!(await this.#confirmDiscard("opening another project"))) return false;
    this.error = "";
    try {
      const opened = path ? await this.#adapter.open(path) : await this.#adapter.openDialog();
      if (!opened) return false;
      if (!this.#editor.load(opened.project.document)) return false;
      this.recent = opened.recent;
      this.#setFile(opened.path, opened.name, opened.project.machineProfile);
      await this.#clearRecovery();
      return true;
    } catch (cause) {
      this.error = `Couldn't open ${path ?? "the project"}. ${message(cause)}`;
      // A recent file that has gone away drops off the list.
      this.recent = await this.#adapter.recent().catch(() => this.recent);
      return false;
    }
  }

  /** Saves to the project's file, asking where first if it has none. */
  save(): Promise<boolean> {
    return this.#write(this.path);
  }

  /** Asks where to save, then saves there. */
  saveAs(): Promise<boolean> {
    return this.#write(null);
  }

  /** Answers the unsaved-changes question. */
  answerUnsaved(choice: UnsavedChoice): void {
    const resolve = this.#resolveUnsaved;
    this.#resolveUnsaved = null;
    this.unsaved = null;
    resolve?.(choice);
  }

  /** Brings back the recovered work as unsaved changes. */
  async restoreRecovery(): Promise<void> {
    const recovery = this.recovery;
    if (!recovery || !this.#editor.load(recovery.project.document)) return;
    this.recovery = null;
    this.#setFile(recovery.path, recovery.name, recovery.project.machineProfile);
    this.#lifecycle.dirty = true;
    this.#recovered = this.#editor.doc;
    await this.#openLaunchFile();
  }

  async discardRecovery(): Promise<void> {
    this.recovery = null;
    await this.#clearRecovery();
    await this.#openLaunchFile();
  }

  /** Copies unsaved work to the recovery file if it changed since the last copy. */
  async writeRecovery(): Promise<void> {
    const doc = this.#editor.doc;
    if (!this.#lifecycle.dirty || this.recovery || doc === this.#recovered) return;
    try {
      await this.#adapter.saveRecovery(this.snapshot(doc), this.path);
      this.#recovered = doc;
    } catch {
      // Best effort: the next tick tries again, and saving is unaffected.
    }
  }

  async #write(path: string | null): Promise<boolean> {
    if (!this.#lifecycle.canEdit()) {
      this.error = "Beam Forge can't save while an update installs.";
      return false;
    }
    this.error = "";
    const doc = this.#editor.doc;
    const project = this.snapshot(doc);
    const release = this.#lifecycle.beginBusy("Saving");
    try {
      const saved = path
        ? await this.#adapter.save(project, path)
        : await this.#adapter.saveAs(project, this.path);
      if (!saved) return false;
      this.recent = saved.recent;
      this.#setFile(saved.path, saved.name, project.machineProfile);
      // Edits made while saving are still unsaved.
      if (this.#editor.doc === doc) {
        this.#lifecycle.dirty = false;
        await this.#clearRecovery();
      }
      return true;
    } catch (cause) {
      this.error = `Couldn't save ${this.name}. ${message(cause)}`;
      return false;
    } finally {
      release();
    }
  }

  /** True when there are no unsaved changes, or the user saved or discarded them. */
  async #confirmDiscard(action: string): Promise<boolean> {
    if (!this.#lifecycle.dirty) return true;
    this.#resolveUnsaved?.("cancel");
    const choice = await new Promise<UnsavedChoice>((resolve) => {
      this.#resolveUnsaved = resolve;
      this.unsaved = { action };
    });
    if (choice === "save") return this.save();
    return choice === "discard";
  }

  async #openLaunchFile(): Promise<void> {
    const path = this.#launchFile;
    this.#launchFile = null;
    if (path) await this.open(path);
  }

  #setFile(path: string | null, name: string, profile: ProfileRef | null): void {
    this.path = path;
    this.name = name;
    this.#fileProfile = profile;
  }

  async #clearRecovery(): Promise<void> {
    this.#recovered = null;
    await this.#adapter.clearRecovery().catch(() => {});
  }
}
