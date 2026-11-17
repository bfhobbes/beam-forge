import { describe, expect, it } from "vite-plus/test";
import { Lifecycle } from "./lifecycle.svelte.js";

describe("application lifecycle", () => {
  it("closes freely when clean and idle", () => {
    expect(new Lifecycle().canClose).toBe(true);
  });

  it("refuses close and install while dirty", () => {
    const app = new Lifecycle();
    app.dirty = true;
    expect(app.canClose).toBe(false);
    expect(app.acquireInstallLock()).toBeNull();
  });

  it("tracks overlapping busy work and releases each once", () => {
    const app = new Lifecycle();
    const save = app.beginBusy("Saving");
    const job = app.beginBusy("Running job");
    save();
    save();
    expect(app.busy).toEqual(["Running job"]);
    expect(app.canClose).toBe(false);
    job();
    expect(app.canClose).toBe(true);
  });

  it("blocks edits and a second install while the install lock is held", () => {
    const app = new Lifecycle();
    const unlock = app.acquireInstallLock();
    expect(unlock).not.toBeNull();
    expect(app.canEdit()).toBe(false);
    expect(app.acquireInstallLock()).toBeNull();
    unlock?.();
    expect(app.canEdit()).toBe(true);
  });
});
