import { describe, expect, it } from "vite-plus/test";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import {
  completedSteps,
  jogExpectation,
  SetupChecklist,
  stepStatus,
  testSquare,
  type MachineSetup,
} from "./setup.svelte.js";

const PORT = "serial:COM3";

function profile(setup: MachineSetup | null, changes: Partial<MachineProfile> = {}) {
  return {
    id: "serial-com3",
    portKey: PORT,
    bed: { width: 400, height: 300 },
    origin: "frontLeft",
    setup,
    ...changes,
  } as MachineProfile;
}

/** The settings store's profile and save, kept in memory. */
function profiles(initial: MachineProfile | null) {
  const view = $state({
    profile: initial,
    saves: 0,
    async saveProfile(changes: Partial<MachineProfile>) {
      if (!view.profile) return false;
      view.profile = { ...view.profile, ...changes };
      view.saves++;
      return true;
    },
  });
  return view;
}

const fresh = (): MachineSetup => ({ steps: {}, dismissed: false });

describe("SetupChecklist", () => {
  it("is offered on first connect to a new machine, and ticks off Connected", async () => {
    const view = profiles(profile(fresh()));
    const checklist = new SetupChecklist(view);
    checklist.follow(null, view.profile);
    expect(checklist.open).toBe(false);

    checklist.follow(PORT, view.profile);
    expect(checklist.open).toBe(true);
    expect(checklist.expanded).toBe("connected");
    await checklist.resolve("bed", "done"); // queued after the Connected save
    expect(view.profile?.setup?.steps).toEqual({ connected: "done", bed: "done" });
    expect(checklist.completed).toBe(2);
    expect(checklist.expanded).toBe("laserMode");
  });

  it("waits for the connected machine's own profile", () => {
    const view = profiles(profile(fresh(), { portKey: "serial:COM9" }));
    const checklist = new SetupChecklist(view);
    checklist.follow(PORT, view.profile);
    expect(checklist.open).toBe(false);
    view.profile = profile(fresh());
    checklist.follow(PORT, view.profile);
    expect(checklist.open).toBe(true);
  });

  it("isn't offered for profiles from before the checklist", () => {
    const view = profiles(profile(null));
    const checklist = new SetupChecklist(view);
    checklist.follow(PORT, view.profile);
    expect(checklist.open).toBe(false);
    expect(view.saves).toBe(0);
  });

  it("lets every step be skipped, and the hands-on step skips only what's left", async () => {
    const view = profiles(profile(fresh()));
    const checklist = new SetupChecklist(view);
    await checklist.resolve("jogTest", "done");
    for (const step of ["connected", "bed", "laserMode", "homing", "handsOn", "airAssist"] as const)
      await checklist.skip(step);
    expect(view.profile?.setup?.steps).toEqual({
      connected: "skipped",
      bed: "skipped",
      laserMode: "skipped",
      homing: "skipped",
      jogTest: "done",
      frameTest: "skipped",
      fireTest: "skipped",
      airAssist: "skipped",
    });
    expect(checklist.status("handsOn")).toBe("skipped");
    expect(checklist.finished).toBe(true);
    expect(checklist.expanded).toBeNull();
  });

  it("resumes where it was left on the next connect", async () => {
    const view = profiles(profile(fresh()));
    const first = new SetupChecklist(view);
    first.follow(PORT, view.profile);
    await first.resolve("bed", "done");
    await first.skip("laserMode");
    first.follow(null, view.profile); // disconnected

    const later = new SetupChecklist(view); // a later session
    later.follow(PORT, view.profile);
    expect(later.open).toBe(true);
    expect(later.expanded).toBe("homing");
    expect(later.status("laserMode")).toBe("skipped");
    expect(later.completed).toBe(3);
  });

  it("isn't offered again once dismissed, until reopened", async () => {
    const view = profiles(profile(fresh()));
    const checklist = new SetupChecklist(view);
    checklist.follow(PORT, view.profile);
    await checklist.dismiss();
    expect(checklist.open).toBe(false);
    expect(view.profile?.setup?.dismissed).toBe(true);

    checklist.follow(null, view.profile);
    checklist.follow(PORT, view.profile);
    expect(checklist.open).toBe(false);

    await checklist.reopen();
    expect(checklist.open).toBe(true);
    expect(view.profile?.setup?.dismissed).toBe(false);
    expect(checklist.expanded).toBe("bed");
  });

  it("isn't offered again once finished", async () => {
    const steps = {
      connected: "done",
      bed: "done",
      laserMode: "done",
      homing: "skipped",
      jogTest: "done",
      frameTest: "done",
      fireTest: "done",
      airAssist: "skipped",
    } as const;
    const view = profiles(profile({ steps, dismissed: false }));
    const checklist = new SetupChecklist(view);
    checklist.follow(PORT, view.profile);
    expect(checklist.open).toBe(false);
    expect(checklist.finished).toBe(true);
  });

  it("can be reopened for a profile from before the checklist", async () => {
    const view = profiles(profile(null));
    const checklist = new SetupChecklist(view);
    checklist.follow(PORT, view.profile);
    await checklist.reopen();
    expect(checklist.open).toBe(true);
    expect(view.profile?.setup).toEqual({ steps: { connected: "done" }, dismissed: false });
  });

  it("redoing a skipped step marks it done", async () => {
    const view = profiles(profile(fresh()));
    const checklist = new SetupChecklist(view);
    await checklist.skip("homing");
    await checklist.resolve("homing", "done");
    expect(checklist.status("homing")).toBe("done");
  });
});

describe("step status", () => {
  it("counts the hands-on tests as one step", () => {
    const setup: MachineSetup = { steps: { jogTest: "done", frameTest: "done" }, dismissed: false };
    expect(stepStatus(setup, "handsOn")).toBe("todo");
    setup.steps.fireTest = "done";
    expect(stepStatus(setup, "handsOn")).toBe("done");
    expect(completedSteps(setup)).toBe(1);
    expect(stepStatus(null, "bed")).toBe("todo");
  });
});

describe("hands-on helpers", () => {
  it("says which way the head should go for each origin corner", () => {
    expect(jogExpectation("frontLeft")).toEqual({ x: "right", y: "away from you (to the back)" });
    expect(jogExpectation("rearRight")).toEqual({ x: "left", y: "towards you (to the front)" });
  });

  it("puts the test square 10 mm in from the origin corner", () => {
    const at = (origin: MachineProfile["origin"]) => {
      const [square] = testSquare(profile(null, { origin })).objects;
      return square.transform.slice(4);
    };
    expect(at("frontLeft")).toEqual([10, 10]);
    expect(at("frontRight")).toEqual([370, 10]);
    expect(at("rearLeft")).toEqual([10, 270]);
    expect(at("rearRight")).toEqual([370, 270]);
    const doc = testSquare(profile(null));
    expect(doc.objects[0]).toMatchObject({ type: "rect", width: 20, height: 20 });
    expect(doc.layers[0]).toMatchObject({ output: true, airAssist: false });
  });
});
