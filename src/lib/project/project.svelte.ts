import { lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { editor } from "#lib/document/editor.svelte.js";
import { machineSettings } from "#lib/machine/settings.svelte.js";
import { projectAdapter } from "./adapter.js";
import { ProjectSession } from "./session.svelte.js";

/** The app's open project. */
export const project = new ProjectSession({
  adapter: projectAdapter,
  editor,
  lifecycle,
  profile: () => {
    const active = machineSettings.profile;
    return active ? { id: active.id, name: active.name } : null;
  },
});
