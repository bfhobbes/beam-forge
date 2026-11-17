import { invoke } from "@tauri-apps/api/core";
import type { OpenedProject } from "#lib/bindings/project/OpenedProject.js";
import type { Project } from "#lib/bindings/project/Project.js";
import type { RecentProject } from "#lib/bindings/project/RecentProject.js";
import type { Recovery } from "#lib/bindings/project/Recovery.js";
import type { SavedProject } from "#lib/bindings/project/SavedProject.js";
import { desktop } from "#lib/desktop/platform.js";

/** Typed access to the project commands in `src-tauri/src/project.rs`. */
export interface ProjectAdapter {
  /** False in the browser preview, which has no file system. */
  readonly available: boolean;
  open(path: string): Promise<OpenedProject>;
  /** Shows the open dialog; null if the user cancelled. */
  openDialog(): Promise<OpenedProject | null>;
  save(project: Project, path: string): Promise<SavedProject>;
  /** Shows the save dialog, starting from `current`; null if the user cancelled. */
  saveAs(project: Project, current: string | null): Promise<SavedProject | null>;
  recent(): Promise<RecentProject[]>;
  recovery(): Promise<Recovery | null>;
  saveRecovery(project: Project, path: string | null): Promise<void>;
  clearRecovery(): Promise<void>;
  /** The `.bfp` file the app was launched with (file association), once. */
  launchFile(): Promise<string | null>;
}

export const desktopProjects: ProjectAdapter = {
  available: true,
  open: (path) => invoke<OpenedProject>("project_open", { path }),
  openDialog: () => invoke<OpenedProject | null>("project_open_dialog"),
  save: (project, path) => invoke<SavedProject>("project_save", { project, path }),
  saveAs: (project, current) =>
    invoke<SavedProject | null>("project_save_as", { project, current }),
  recent: () => invoke<RecentProject[]>("project_recent"),
  recovery: () => invoke<Recovery | null>("project_recovery"),
  saveRecovery: (project, path) => invoke<void>("project_recovery_save", { project, path }),
  clearRecovery: () => invoke<void>("project_recovery_clear"),
  launchFile: () => invoke<string | null>("project_launch_file"),
};

export const PREVIEW_NO_FILES =
  "Browser preview: projects are opened and saved by the desktop app.";

// The browser preview has no file system: it says so instead of pretending.
export const previewProjects: ProjectAdapter = {
  available: false,
  open: async () => {
    throw new Error(PREVIEW_NO_FILES);
  },
  openDialog: async () => {
    throw new Error(PREVIEW_NO_FILES);
  },
  save: async () => {
    throw new Error(PREVIEW_NO_FILES);
  },
  saveAs: async () => {
    throw new Error(PREVIEW_NO_FILES);
  },
  recent: async () => [],
  recovery: async () => null,
  saveRecovery: async () => {},
  clearRecovery: async () => {},
  launchFile: async () => null,
};

export const projectAdapter = desktop ? desktopProjects : previewProjects;
