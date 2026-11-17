import { describe, expect, it, vi } from "vite-plus/test";
import { ActionRegistry } from "#lib/actions/registry.svelte.js";
import type { JobProgress } from "#lib/bindings/JobProgress.js";
import type { JobState } from "#lib/bindings/JobState.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { OverrideKind } from "#lib/bindings/OverrideKind.js";
import type { Preflight } from "#lib/bindings/Preflight.js";
import type { RunPlan } from "#lib/bindings/gcode/RunPlan.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { emptyDocument } from "#lib/document/model.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";
import { registerJobActions } from "./job-actions.js";
import { previewJob, type FrameRequest, type JobAdapter } from "./job-adapter.js";
import { BUSY_REASON, formatElapsed, jobStateLabel, MachineJob } from "./job.svelte.js";
import { estimateError, formatError, outcomeNote } from "./run.svelte.js";
import { atFeedRate } from "#lib/toolpath/fixtures.js";

function progress(state: JobState, acknowledged = 0, total: number | null = 40): JobProgress {
  return { name: "sample-job.gc", state, acknowledged, total, elapsedMs: 0 };
}

/** A square, 10 mm a side at 600 mm/min (a second each), after a rapid to its corner; G0/G1 on lines 4-8. */
function squarePlan(name = "Tag"): RunPlan {
  const cut = (x: number, y: number) => ({
    kind: "cut" as const,
    to: { x, y },
    power: 50,
    speed: 600,
  });
  return {
    name,
    toolpath: atFeedRate(
      {
        start: { x: 0, y: 0 },
        layers: [
          {
            layerId: "a",
            airAssist: false,
            moves: [
              { kind: "rapid", to: { x: 10, y: 10 } },
              cut(20, 10),
              cut(20, 20),
              cut(10, 20),
              cut(10, 10),
            ],
          },
        ],
      },
      6000,
    ),
    moveLines: [4, 5, 6, 7, 8],
    placement: {
      mirrorWidth: null,
      mirrorHeight: null,
      offset: { x: 0, y: 0 },
    },
  };
}

function status(x: number, y: number): MachineStatus {
  const position = { x, y, z: 0 };
  return {
    state: { name: "run" },
    machinePosition: position,
    workPosition: position,
    workOffset: { x: 0, y: 0, z: 0 },
    feed: 600,
    spindle: 500,
    overrides: { feed: 100, rapid: 100, spindle: 100 },
    buffer: null,
    pins: {
      limitX: false,
      limitY: false,
      limitZ: false,
      probe: false,
      door: false,
      hold: false,
      softReset: false,
      cycleStart: false,
    },
    accessories: null,
    line: null,
  };
}

function fakeAdapter(current: JobProgress | null = null) {
  let emit: (progress: JobProgress) => void = () => {};
  const adapter = {
    runFile: vi.fn(async (): Promise<string | null> => "sample-job.gc"),
    preflight: vi.fn(async (_job: unknown): Promise<Preflight> => ({
      checks: [
        {
          id: "machineReady",
          level: "pass",
          title: "Machine idle",
          detail: null,
        },
      ],
    })),
    runDocument: vi.fn(async (job: { name: string }) => squarePlan(job.name)),
    frame: vi.fn(async (_request: FrameRequest) => "Frame"),
    current: vi.fn(async () => current),
    pause: vi.fn(async () => {}),
    resume: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    stopAndWait: vi.fn(async () => {}),
    setOverride: vi.fn(async (_kind: OverrideKind, _percent: number) => {}),
    onProgress: vi.fn(async (handler: (progress: JobProgress) => void) => {
      emit = handler;
      return () => {};
    }),
  } satisfies JobAdapter;
  return { adapter, emit: (p: JobProgress) => emit(p) };
}

describe("MachineJob", () => {
  it("holds the lifecycle busy from the first progress until the job is done", async () => {
    const lifecycle = new Lifecycle();
    const { adapter, emit } = fakeAdapter();
    const job = new MachineJob(adapter, lifecycle);
    await job.start();
    expect(lifecycle.canClose).toBe(true);

    await job.runFile();
    emit(progress({ state: "running" }));
    expect(job.active).toBe(true);
    expect(lifecycle.busy).toEqual([BUSY_REASON]);
    emit(progress({ state: "paused" }, 10));
    emit(progress({ state: "running" }, 20));
    expect(lifecycle.busy).toEqual([BUSY_REASON]);
    expect(job.fraction).toBe(0.5);

    emit(progress({ state: "stopped", warning: null }, 25));
    expect(job.active).toBe(false);
    expect(lifecycle.canClose).toBe(true);
  });

  it("picks up a job already running when it starts", async () => {
    const lifecycle = new Lifecycle();
    const { adapter } = fakeAdapter(progress({ state: "running" }, 5));
    const job = new MachineJob(adapter, lifecycle);

    const stop = await job.start();

    expect(job.progress?.acknowledged).toBe(5);
    expect(lifecycle.busy).toEqual([BUSY_REASON]);
    stop();
    expect(lifecycle.canClose).toBe(true);
  });

  it("treats a rejected line as paused until continued or aborted", async () => {
    const { adapter, emit } = fakeAdapter();
    const job = new MachineJob(adapter, new Lifecycle());
    await job.start();

    emit(
      progress({
        state: "rejected",
        line: 3,
        command: "G99",
        message: "GRBL error 20",
      }),
    );
    expect(job.paused).toBe(true);
    await job.resume();
    await job.stop();

    expect(adapter.resume).toHaveBeenCalledOnce();
    expect(adapter.stop).toHaveBeenCalledOnce();
  });

  it("keeps the last failed command's error until the next command", async () => {
    const { adapter } = fakeAdapter();
    adapter.runFile.mockRejectedValueOnce(new Error("A job is already running."));
    const job = new MachineJob(adapter, new Lifecycle());

    await job.runFile();
    expect(job.error).toBe("A job is already running.");
    await job.runFile();
    expect(job.error).toBe("");
  });

  it("says why in the browser preview", async () => {
    const job = new MachineJob(previewJob, new Lifecycle());
    await job.runFile();
    expect(job.error).toBe(PREVIEW_UNAVAILABLE);
    const request = {
      document: emptyDocument(),
      profile: null,
      placement: { startFrom: "absolute", anchor: "center" },
      shape: "box",
    } as const;
    expect(await job.frame(request)).toBe(false);
    expect(job.error).toBe(PREVIEW_UNAVAILABLE);
  });

  it("gets the pre-run checks, explaining a failure", async () => {
    const { adapter } = fakeAdapter();
    const job = new MachineJob(adapter, new Lifecycle());
    const request = {
      document: emptyDocument(),
      profile: null,
      placement: { startFrom: "absolute", anchor: "center" },
      name: "Tag",
    } as const;
    expect((await job.preflight(request))?.checks[0]?.id).toBe("machineReady");
    adapter.preflight.mockRejectedValueOnce(new Error("Connect to the machine first."));
    expect(await job.preflight(request)).toBeNull();
    expect(job.error).toBe("Connect to the machine first.");
  });

  it("stops for closing and says whether the machine confirmed it", async () => {
    const { adapter } = fakeAdapter();
    const job = new MachineJob(adapter, new Lifecycle());
    expect(await job.stopForClose()).toBe(true);
    expect(adapter.stopAndWait).toHaveBeenCalledOnce();
    adapter.stopAndWait.mockRejectedValueOnce(
      new Error("The machine didn't confirm the stop. Check that the laser is off."),
    );
    expect(await job.stopForClose()).toBe(false);
    expect(job.error).toMatch(/didn't confirm the stop/);
  });

  it("frames through the job path and reports refusals", async () => {
    const { adapter } = fakeAdapter();
    const job = new MachineJob(adapter, new Lifecycle());
    const request = {
      document: emptyDocument(),
      profile: null,
      placement: { startFrom: "currentPosition", anchor: "center" },
      shape: "rubberBand",
    } as const;

    expect(await job.frame(request)).toBe(true);
    expect(adapter.frame).toHaveBeenCalledWith(request);
    adapter.frame.mockRejectedValueOnce(new Error("A job is already running."));
    expect(await job.frame(request)).toBe(false);
    expect(job.error).toBe("A job is already running.");
  });

  it("follows a design run on its toolpath, and compares its time with the estimate", async () => {
    const { adapter, emit } = fakeAdapter();
    const log: string[] = [];
    const job = new MachineJob(adapter, new Lifecycle(), (note) => log.push(note));
    await job.start();
    const request = {
      document: emptyDocument(),
      profile: null,
      placement: { startFrom: "absolute", anchor: "center" },
      name: "Tag",
    } as const;

    // The first progress can arrive before the plan does.
    adapter.runDocument.mockImplementationOnce(async (started) => {
      emit({ ...progress({ state: "running" }, 0, 9), name: started.name });
      return squarePlan(started.name);
    });
    expect(await job.runDocument(request)).toBe(true);
    const run = job.run!;
    expect(run.name).toBe("Tag");
    expect(run.acknowledged).toBe(0);
    expect(run.estimate).toBeCloseTo(4 + Math.hypot(10, 10) / 100);

    // Lines are taken ahead of the head; the head says how far it got.
    emit({ ...progress({ state: "running" }, 7, 9), name: "Tag" });
    expect(run.completed).toBe(0);
    job.follow(status(20, 15));
    expect(run.completed).toBe(2);
    expect(run.head).toEqual({ x: 20, y: 15 });
    expect(run.layer).toBe(0);
    expect(run.remaining).toBeCloseTo(2.5);
    expect(run.point.head).toEqual({ x: 20, y: 15 });

    emit({
      ...progress({ state: "finished" }, 9, 9),
      name: "Tag",
      elapsedMs: 5_000,
    });
    expect(run.completed).toBe(5);
    expect(run.outcome).toEqual({
      state: { state: "finished" },
      actualMs: 5_000,
      estimateMs: Math.round(run.estimate * 1000),
    });
    // 4.141 s estimated, 5 s taken: logged once, as 21% over.
    expect(log).toEqual(["Tag finished in 0:05, estimated 0:04 (+21%)"]);
    emit({ ...progress({ state: "finished" }, 9, 9), name: "Tag", elapsedMs: 5_000 });
    expect(log).toHaveLength(1);

    // Another job ends the run.
    await job.runFile();
    expect(job.run).toBeNull();
  });

  it("compares a finished run's time with its estimate", () => {
    const outcome = (state: JobState, actualMs: number, estimateMs: number) => ({
      state,
      actualMs,
      estimateMs,
    });
    const finished = { state: "finished" } as const;
    expect(estimateError(outcome(finished, 110_000, 100_000))).toBeCloseTo(0.1);
    expect(estimateError(outcome(finished, 5_000, 0))).toBeNull();
    expect(formatError(0.004)).toBe("±0%");
    expect(formatError(-0.123)).toBe("−12%");
    expect(outcomeNote("Box", outcome(finished, 125_000, 120_000))).toBe(
      "Box finished in 2:05, estimated 2:00 (+4%)",
    );
    // A stopped run says nothing about the estimate.
    const stopped = { state: "stopped", warning: null } as const;
    expect(outcomeNote("Box", outcome(stopped, 60_000, 120_000))).toBeNull();
  });

  it("keeps override targets until the job ends", async () => {
    const { adapter, emit } = fakeAdapter();
    const job = new MachineJob(adapter, new Lifecycle());
    await job.start();
    emit(progress({ state: "running" }));

    await job.setOverride("feed", 250);
    await job.setOverride("spindle", 42.4);
    expect(adapter.setOverride).toHaveBeenCalledWith("feed", 200);
    expect(adapter.setOverride).toHaveBeenCalledWith("spindle", 42);
    expect(job.overrideTargets).toEqual({ feed: 200, spindle: 42 });

    emit(progress({ state: "stopped", warning: null }));
    expect(job.overrideTargets).toEqual({});
  });

  it("has no fraction when the total is unknown", async () => {
    const { adapter, emit } = fakeAdapter();
    const job = new MachineJob(adapter, new Lifecycle());
    await job.start();
    emit(progress({ state: "running" }, 5, null));
    expect(job.fraction).toBeNull();
  });
});

describe("job actions", () => {
  it("enable run, pause, resume and stop to suit the job", async () => {
    const registry = new ActionRegistry({ storage: null });
    const { adapter, emit } = fakeAdapter();
    const job = new MachineJob(adapter, new Lifecycle());
    await job.start();
    let connected = false;
    registerJobActions(job, () => connected, registry);
    const enabled = (id: string) => registry.enabled(registry.get(id)!);

    expect(enabled("machine.run-file")).toBe(false);
    connected = true;
    expect(enabled("machine.run-file")).toBe(true);
    expect(enabled("machine.job.stop")).toBe(false);

    emit(progress({ state: "running" }));
    expect(enabled("machine.run-file")).toBe(false);
    expect(enabled("machine.job.pause")).toBe(true);
    expect(enabled("machine.job.resume")).toBe(false);
    expect(enabled("machine.job.stop")).toBe(true);

    emit(progress({ state: "paused" }));
    expect(enabled("machine.job.pause")).toBe(false);
    expect(enabled("machine.job.resume")).toBe(true);
    expect(registry.run("machine.job.resume")).toBe(true);
    expect(adapter.resume).toHaveBeenCalledOnce();

    emit(progress({ state: "stopping" }));
    expect(enabled("machine.job.stop")).toBe(false);
    emit(progress({ state: "finished" }, 40));
    expect(enabled("machine.run-file")).toBe(true);
  });
});

describe("formatting", () => {
  it("formats elapsed time", () => {
    expect(formatElapsed(0)).toBe("0:00");
    expect(formatElapsed(65_400)).toBe("1:05");
    expect(formatElapsed(3_725_000)).toBe("1:02:05");
  });

  it("labels each state", () => {
    expect(jobStateLabel({ state: "rejected", line: 1, command: "", message: "" })).toBe(
      "Line rejected",
    );
    expect(jobStateLabel({ state: "failed", reason: "" })).toBe("Failed");
  });
});
