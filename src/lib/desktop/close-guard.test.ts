import { describe, expect, it, vi } from "vite-plus/test";
import { closePrompt, stopJobAndClose } from "./close-guard.js";
import { Lifecycle } from "./lifecycle.svelte.js";

const JOB = "Running job";

describe("closePrompt", () => {
  it("closes straight away when clean and idle, and asks about unsaved changes", () => {
    const app = new Lifecycle();
    expect(closePrompt(app, JOB)).toBe("none");
    app.dirty = true;
    expect(closePrompt(app, JOB)).toBe("unsaved");
  });

  it("offers to stop a running job, even with unsaved changes", () => {
    const app = new Lifecycle();
    const release = app.beginBusy(JOB);
    expect(closePrompt(app, JOB)).toBe("job");
    app.dirty = true;
    expect(closePrompt(app, JOB)).toBe("job");
    release();
    expect(closePrompt(app, JOB)).toBe("unsaved");
  });

  it("waits for work it can't stop, and for an update installing", () => {
    const app = new Lifecycle();
    app.beginBusy(JOB);
    const save = app.beginBusy("Saving");
    expect(closePrompt(app, JOB)).toBe("busy");
    save();
    expect(closePrompt(app, JOB)).toBe("job");
    expect(closePrompt({ dirty: false, busy: [], locked: true }, JOB)).toBe("busy");
  });
});

describe("stopJobAndClose", () => {
  function steps(stopped: boolean) {
    const order: string[] = [];
    let finishStop: (stopped: boolean) => void = () => {};
    const close = {
      stop: vi.fn(
        () =>
          new Promise<boolean>((resolve) => {
            order.push("stop");
            finishStop = (value) => {
              order.push("stopped");
              resolve(value);
            };
          }),
      ),
      error: () => "The machine didn't confirm the stop. Check that the laser is off.",
      discard: vi.fn(() => void order.push("discard")),
      destroy: vi.fn(async () => void order.push("destroy")),
    };
    return { close, order, finish: () => finishStop(stopped) };
  }

  it("closes the window only after the machine confirms the stop", async () => {
    const { close, order, finish } = steps(true);
    const closing = stopJobAndClose(close);
    await Promise.resolve();
    expect(close.destroy).not.toHaveBeenCalled();

    finish();

    expect(await closing).toBeNull();
    expect(order).toEqual(["stop", "stopped", "discard", "destroy"]);
  });

  it("keeps the window open and says why when the stop isn't confirmed", async () => {
    const { close, finish } = steps(false);
    const closing = stopJobAndClose(close);
    finish();

    expect(await closing).toMatch(/didn't confirm the stop/);
    expect(close.discard).not.toHaveBeenCalled();
    expect(close.destroy).not.toHaveBeenCalled();
  });

  it("reports a window that won't close", async () => {
    const { close, finish } = steps(true);
    close.destroy.mockRejectedValueOnce("not allowed");
    const closing = stopJobAndClose(close);
    finish();
    expect(await closing).toBe("not allowed");
  });
});
