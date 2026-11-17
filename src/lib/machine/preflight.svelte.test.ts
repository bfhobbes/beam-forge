import { describe, expect, it } from "vite-plus/test";
import type { Preflight } from "#lib/bindings/Preflight.js";
import {
  PreflightPrefs,
  acknowledgements,
  blocked,
  checklist,
  checklistNeeded,
  framingCheck,
  withFraming,
} from "./preflight.svelte.js";

const passing: Preflight = {
  checks: [
    { id: "machineReady", level: "pass", title: "Machine idle", detail: null },
    { id: "laserMode", level: "pass", title: "Laser mode on ($32=1)", detail: null },
  ],
};

const warning: Preflight = {
  checks: [
    { id: "machineReady", level: "pass", title: "Machine idle", detail: null },
    { id: "bounds", level: "warn", title: "Job leaves the bed", detail: "By 5 mm." },
    { id: "overrides", level: "warn", title: "Overrides left", detail: "Feed 50%." },
  ],
};

function memoryStorage() {
  const items = new Map<string, string>();
  return {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => void items.set(key, value),
  };
}

describe("pre-run checklist", () => {
  it("is skipped when everything passes, unless always shown", () => {
    const items = checklist(passing, true);
    expect(items.map((item) => item.id)).toEqual(["machineReady", "laserMode", "framed"]);
    expect(checklistNeeded(items, false)).toBe(false);
    expect(checklistNeeded(items, true)).toBe(true);
    expect(checklistNeeded(checklist(warning, null), false)).toBe(true);
  });

  it("asks about framing only when wanted, and passes a framed job", () => {
    expect(checklist(passing, null).some((item) => item.id === "framed")).toBe(false);
    expect(framingCheck(true).level).toBe("pass");
    const unframed = checklist(passing, false);
    expect(checklistNeeded(unframed, false)).toBe(true);
    const framed = withFraming(unframed, true);
    expect(checklistNeeded(framed, false)).toBe(false);
  });

  it("acknowledges the machine's warnings, not the framing question", () => {
    const items = checklist(warning, false);
    expect(acknowledgements(items)).toEqual(["bounds", "overrides"]);
    expect(acknowledgements(checklist(passing, true))).toEqual([]);
  });

  it("blocks the start only on a blocking check", () => {
    expect(blocked(checklist(warning, false))).toBe(false);
    const alarm: Preflight = {
      checks: [{ id: "machineReady", level: "block", title: "Machine in alarm", detail: null }],
    };
    expect(blocked(checklist(alarm, null))).toBe(true);
  });

  it("remembers whether it always shows and asks about framing", () => {
    const storage = memoryStorage();
    const prefs = new PreflightPrefs(storage);
    expect(prefs.alwaysShow).toBe(false);
    expect(prefs.askFramed).toBe(true);
    prefs.setAlwaysShow(true);
    prefs.setAskFramed(false);
    const again = new PreflightPrefs(storage);
    expect(again.alwaysShow).toBe(true);
    expect(again.askFramed).toBe(false);
  });
});
