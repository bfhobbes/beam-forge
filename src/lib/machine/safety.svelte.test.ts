import { describe, expect, it, vi } from "vite-plus/test";
import type { RealHardware } from "#lib/bindings/RealHardware.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";
import { previewSafety } from "./safety-adapter.js";
import { NOTICE_VERSION, RealHardwareSwitch } from "./safety.svelte.js";

function fakeAdapter(initial = false) {
  let enabled = initial;
  const state = (): RealHardware => ({ enabled, noticeVersion: NOTICE_VERSION });
  return {
    realHardware: vi.fn(async () => state()),
    setRealHardware: vi.fn(async (on: boolean, accepted?: number) => {
      if (on && accepted !== NOTICE_VERSION) throw new Error("The safety notice has changed.");
      enabled = on;
      return state();
    }),
  };
}

describe("RealHardwareSwitch", () => {
  it("starts off and loads the saved choice", async () => {
    const real = new RealHardwareSwitch(fakeAdapter(true));
    expect(real.enabled).toBe(false);
    await real.load();
    expect(real.enabled).toBe(true);
  });

  it("turns on only by accepting the current notice, and off again", async () => {
    const adapter = fakeAdapter();
    const real = new RealHardwareSwitch(adapter);
    expect(await real.accept()).toBe(true);
    expect(adapter.setRealHardware).toHaveBeenLastCalledWith(true, NOTICE_VERSION);
    expect(real.enabled).toBe(true);

    expect(await real.turnOff()).toBe(true);
    expect(adapter.setRealHardware).toHaveBeenLastCalledWith(false, undefined);
    expect(real.enabled).toBe(false);
  });

  it("stays off and says why when the backend refuses", async () => {
    const real = new RealHardwareSwitch(previewSafety);
    await real.load();
    expect(real.enabled).toBe(false);
    expect(await real.accept()).toBe(false);
    expect(real.enabled).toBe(false);
    expect(real.error).toBe(PREVIEW_UNAVAILABLE);
    expect(real.saving).toBe(false);
  });
});
