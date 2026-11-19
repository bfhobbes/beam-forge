import { describe, expect, it, vi } from "vite-plus/test";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { Position } from "#lib/bindings/Position.js";
import type { Preflight } from "#lib/bindings/Preflight.js";
import type { JobSummary } from "#lib/bindings/gcode/JobSummary.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { emptyDocument } from "#lib/document/model.js";
import { PREVIEW_NO_GCODE, previewGcode, type GcodeAdapter } from "./adapter.js";
import type { DocumentJob, FrameRequest } from "#lib/machine/job-adapter.js";
import { PreflightPrefs } from "#lib/machine/preflight.svelte.js";
import {
  DEFAULT_PLACEMENT,
  EXPORT_BUSY,
  JobSetup,
  frameLaserLabel,
  type JobSetupDeps,
} from "./job-setup.svelte.js";

const summary: JobSummary = {
  bounds: { min: { x: 10, y: 10 }, max: { x: 50, y: 30 } },
  insideBed: true,
  warning: null,
  lines: 12,
  cutLength: 120,
  travelLength: 14,
  constantPowerLayers: [],
  constantPowerLine: null,
  unknownBounds: null,
};

const allPass: Preflight = {
  checks: [
    { id: "machineReady", level: "pass", title: "Machine idle", detail: null },
    { id: "laserMode", level: "pass", title: "Laser mode on ($32=1)", detail: null },
    { id: "bounds", level: "pass", title: "Job inside the bed", detail: null },
  ],
};

const offTheBed: Preflight = {
  checks: [
    { id: "machineReady", level: "pass", title: "Machine idle", detail: null },
    { id: "bounds", level: "warn", title: "Job leaves the bed", detail: "By 5 mm." },
  ],
};

const inAlarm: Preflight = {
  checks: [
    { id: "machineReady", level: "block", title: "Machine in alarm", detail: "Unlock it." },
    { id: "bounds", level: "warn", title: "Job leaves the bed", detail: "By 5 mm." },
  ],
};

function memoryStorage(initial: Record<string, string> = {}) {
  const items = new Map(Object.entries(initial));
  return {
    items,
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => void items.set(key, value),
  };
}

function setup(overrides: Partial<JobSetupDeps> = {}) {
  let connected = false;
  let position: Position | null = null;
  let profile: MachineProfile | null = null;
  const adapter = {
    available: true,
    summary: vi.fn(async () => summary),
    export: vi.fn(async (): Promise<string | null> => "C:/jobs/Coaster.gcode"),
  } satisfies GcodeAdapter;
  const lifecycle = new Lifecycle();
  const run = vi.fn(async (_job: DocumentJob) => true);
  const frame = vi.fn(async (_request: FrameRequest) => true);
  const preflight = vi.fn(async (_job: DocumentJob): Promise<Preflight | null> => allPass);
  // Not asking about framing unless a test wants it.
  const prefs = new PreflightPrefs();
  prefs.askFramed = false;
  const deps: JobSetupDeps = {
    adapter,
    run,
    frame,
    preflight,
    prefs,
    runError: () => "The machine is busy.",
    jobActive: () => false,
    lifecycle,
    document: () => emptyDocument(),
    profile: () => profile,
    saveProfile: vi.fn(async (changes: Partial<MachineProfile>) => {
      profile = { ...(profile as MachineProfile), ...changes };
      return true;
    }),
    connected: () => connected,
    position: () => position,
    name: () => "Coaster",
    ...overrides,
  };
  return {
    job: new JobSetup(deps),
    adapter,
    lifecycle,
    run,
    frame,
    preflight,
    prefs,
    deps,
    connect: (at: Position | null = { x: 100, y: 50, z: 0 }) => {
      connected = true;
      position = at;
      profile ??= { userOrigin: null } as MachineProfile;
    },
  };
}

describe("JobSetup", () => {
  it("summarises the job when it opens", async () => {
    const { job, adapter } = setup();
    job.open("export");
    await vi.waitFor(() => expect(job.summary).toEqual(summary));
    expect(adapter.summary).toHaveBeenCalledWith({
      document: emptyDocument(),
      profile: null,
      placement: DEFAULT_PLACEMENT,
    });
    expect(job.canConfirm).toBe(true);
  });

  it("explains why current position and user origin need a machine", async () => {
    const { job, adapter, connect } = setup();
    expect(job.unavailable("absolute")).toBeNull();
    expect(job.unavailable("currentPosition")).toMatch(/Connect to a machine/);
    expect(job.unavailable("userOrigin")).toMatch(/No user origin yet/);

    job.open("export");
    job.setStartFrom("currentPosition");
    await job.refresh();
    expect(job.problem).toMatch(/Connect to a machine/);
    expect(job.summary).toBeNull();
    expect(job.canConfirm).toBe(false);
    adapter.summary.mockClear();

    connect(null);
    expect(job.unavailable("currentPosition")).toMatch(/head's position/);
    connect();
    expect(job.unavailable("currentPosition")).toBeNull();
    await job.refresh();
    expect(adapter.summary).toHaveBeenCalledOnce();
    expect(job.problem).toBe("");
  });

  it("frames the job as placed, in the chosen shape", async () => {
    const storage = memoryStorage();
    const { job, frame, connect } = setup({ storage });
    expect(job.canFrame).toBe(false);
    connect();
    expect(job.canFrame).toBe(true);
    job.setStartFrom("currentPosition");
    job.setAnchor("center");

    expect(await job.frame()).toBe(true);
    expect(frame).toHaveBeenLastCalledWith({
      document: emptyDocument(),
      profile: { userOrigin: null },
      placement: { startFrom: "currentPosition", anchor: "center" },
      shape: "box",
    });

    job.setFrameShape("rubberBand");
    await job.frame();
    expect(frame.mock.lastCall?.[0].shape).toBe("rubberBand");
    expect(new JobSetup({ ...setup().deps, storage }).frameShape).toBe("rubberBand");
  });

  it("explains a refused frame and won't frame over a running job", async () => {
    let active = false;
    const { job, frame, connect } = setup({ jobActive: () => active });
    connect();
    frame.mockResolvedValueOnce(false);
    expect(await job.frame()).toBe(false);
    expect(job.error).toBe("The machine is busy.");
    active = true;
    expect(job.canFrame).toBe(false);
  });

  it("labels whether the laser fires while framing", () => {
    expect(frameLaserLabel(null)).toBe("Laser off while framing");
    expect(frameLaserLabel({ framePower: 0 } as MachineProfile)).toBe("Laser off while framing");
    expect(frameLaserLabel({ framePower: 1.5 } as MachineProfile)).toBe(
      "Low-power frame: laser on at 1.5% (M4)",
    );
  });

  it("sets the user origin from the head and starts from it", async () => {
    const { job, deps, connect } = setup();
    expect(job.canSetOrigin).toBe(false);
    connect({ x: 12.5, y: 40, z: 0 });
    expect(job.canSetOrigin).toBe(true);

    await job.setUserOrigin();

    expect(deps.saveProfile).toHaveBeenCalledWith({ userOrigin: { x: 12.5, y: 40 } });
    expect(job.userOrigin).toEqual({ x: 12.5, y: 40 });
    expect(job.placement.startFrom).toBe("userOrigin");
    expect(job.unavailable("userOrigin")).toBeNull();
  });

  it("remembers the placement", () => {
    const storage = memoryStorage();
    const { job } = setup({ storage });
    job.setStartFrom("userOrigin");
    job.setAnchor("center");

    const again = setup({ storage }).job;
    expect(again.placement).toEqual({ startFrom: "userOrigin", anchor: "center" });
    expect(job.anchored).toBe(true);

    const damaged = setup({ storage: memoryStorage({ "beam-forge.job-placement": "{" }) }).job;
    expect(damaged.placement).toEqual(DEFAULT_PLACEMENT);
  });

  it("exports under the project name, holding the lifecycle busy", async () => {
    const { job, adapter, lifecycle } = setup();
    let busy: readonly string[] = [];
    adapter.export.mockImplementation(async () => {
      busy = lifecycle.busy;
      return "C:/jobs/Coaster.gcode";
    });
    job.open("export");
    await job.refresh();

    await job.confirm();

    expect(adapter.export).toHaveBeenCalledWith(expect.anything(), "Coaster");
    expect(busy).toEqual([EXPORT_BUSY]);
    expect(lifecycle.busy).toEqual([]);
    expect(job.mode).toBeNull();
    expect(job.notice).toBe("Exported C:/jobs/Coaster.gcode");
  });

  it("keeps the dialog open when the export is cancelled or fails", async () => {
    const { job, adapter } = setup();
    job.open("export");
    await job.refresh();
    adapter.export.mockResolvedValueOnce(null);
    await job.confirm();
    expect(job.mode).toBe("export");
    expect(job.notice).toBe("");

    adapter.export.mockRejectedValueOnce(new Error("Disk full"));
    await job.confirm();
    expect(job.mode).toBe("export");
    expect(job.error).toBe("Disk full");
  });

  it("starts the job through the machine job, and needs a connection", async () => {
    const { job, run, connect } = setup();
    job.open("run");
    await job.refresh();
    expect(job.canConfirm).toBe(false);
    connect();
    expect(job.canConfirm).toBe(true);

    await job.confirm();

    expect(run).toHaveBeenCalledWith({
      document: emptyDocument(),
      profile: { userOrigin: null },
      placement: DEFAULT_PLACEMENT,
      name: "Coaster",
      acknowledged: [],
    });
    expect(job.mode).toBeNull();

    run.mockResolvedValueOnce(false);
    job.open("run");
    await job.refresh();
    await job.confirm();
    expect(job.mode).toBe("run");
    expect(job.error).toBe("The machine is busy.");
  });

  it("runs the pre-run checks first, and starts without asking when they pass", async () => {
    const { job, run, preflight, connect } = setup();
    connect();
    job.open("run");
    await job.refresh();

    await job.confirm();

    expect(preflight).toHaveBeenCalledWith({
      document: emptyDocument(),
      profile: { userOrigin: null },
      placement: DEFAULT_PLACEMENT,
      name: "Coaster",
    });
    expect(job.checklist).toBeNull();
    expect(run).toHaveBeenCalledOnce();
    expect(job.mode).toBeNull();
  });

  it("shows warnings first, then starts with them acknowledged", async () => {
    const { job, run, preflight, connect } = setup();
    preflight.mockResolvedValue(offTheBed);
    connect();
    job.open("run");
    await job.refresh();

    await job.confirm();
    expect(run).not.toHaveBeenCalled();
    expect(job.mode).toBe("run");
    expect(job.checklist?.map((item) => item.level)).toEqual(["pass", "warn"]);
    expect(job.canConfirm).toBe(true);

    await job.confirm();
    expect(run.mock.lastCall?.[0].acknowledged).toEqual(["bounds"]);
    expect(preflight).toHaveBeenCalledOnce();
    expect(job.checklist).toBeNull();
    expect(job.mode).toBeNull();
  });

  it("won't start past a blocking check, and Back returns to the settings", async () => {
    const { job, run, preflight, connect } = setup();
    preflight.mockResolvedValue(inAlarm);
    connect();
    job.open("run");
    await job.refresh();

    await job.confirm();
    expect(job.checklist).not.toBeNull();
    expect(job.canConfirm).toBe(false);
    await job.confirm();
    expect(run).not.toHaveBeenCalled();

    job.back();
    expect(job.checklist).toBeNull();
    expect(job.mode).toBe("run");
  });

  it("checks again after the machine refuses, and explains a check that failed", async () => {
    const { job, run, preflight, connect } = setup();
    preflight.mockResolvedValue(offTheBed);
    connect();
    job.open("run");
    await job.refresh();
    await job.confirm();
    run.mockResolvedValueOnce(false);

    await job.confirm();
    expect(job.error).toBe("The machine is busy.");
    expect(job.checklist).toBeNull();
    expect(job.mode).toBe("run");

    preflight.mockResolvedValueOnce(null);
    await job.confirm();
    expect(job.error).toBe("The machine is busy.");
    expect(job.checklist).toBeNull();
  });

  it("always shows the checklist when asked to", async () => {
    const { job, run, prefs, connect } = setup();
    prefs.alwaysShow = true;
    connect();
    job.open("run");
    await job.refresh();

    await job.confirm();
    expect(job.checklist?.every((item) => item.level === "pass")).toBe(true);
    expect(run).not.toHaveBeenCalled();
    await job.confirm();
    expect(run).toHaveBeenCalledOnce();
  });

  it("asks whether the job was framed since it last changed", async () => {
    let doc = emptyDocument();
    const { job, run, prefs, connect } = setup({ document: () => doc });
    prefs.askFramed = true;
    connect();
    job.open("run");
    await job.refresh();

    await job.confirm();
    const framing = () => job.checklist?.find((item) => item.id === "framed");
    expect(framing()?.level).toBe("warn");
    expect(run).not.toHaveBeenCalled();
    // Framing from the checklist answers it.
    await job.frame();
    expect(framing()?.level).toBe("pass");
    expect(job.framed).toBe(true);
    await job.confirm();
    expect(run.mock.lastCall?.[0].acknowledged).toEqual([]);

    // Framed and unchanged: nothing to ask.
    run.mockClear();
    job.open("run");
    await job.confirm();
    expect(run).toHaveBeenCalledOnce();

    // Moving the job or changing the design asks again.
    job.setAnchor("center");
    job.setStartFrom("currentPosition");
    expect(job.framed).toBe(false);
    job.setStartFrom("absolute");
    job.setAnchor("bottomLeft");
    expect(job.framed).toBe(true);
    doc = emptyDocument();
    expect(job.framed).toBe(false);
  });

  it("says the browser preview can't generate GCode", async () => {
    const { job } = setup({ adapter: previewGcode });
    expect(job.available).toBe(false);
    job.open("export");
    await job.refresh();
    expect(job.problem).toBe(PREVIEW_NO_GCODE);
    expect(job.canConfirm).toBe(false);
  });
});
