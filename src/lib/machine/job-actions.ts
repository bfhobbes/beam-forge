import { actions as appActions, type ActionRegistry } from "#lib/actions/registry.svelte.js";
import type { MachineJob } from "./job.svelte.js";

const NOT_CONNECTED = "Connect to a machine first.";
const NO_JOB = "No job is running.";

/** Registers the GCode job actions. Returns the cleanup. */
export function registerJobActions(
  job: MachineJob,
  connected: () => boolean,
  registry: ActionRegistry = appActions,
): () => void {
  const running = () => connected() && job.active;
  return registry.register(
    {
      id: "machine.run-file",
      title: "Run GCode file…",
      group: "Machine",
      keywords: ["gcode", "nc", "stream", "send", "job", "start"],
      enabled: () => connected() && !job.active,
      hint: NOT_CONNECTED,
      run: () => void job.runFile(),
    },
    {
      id: "machine.job.pause",
      title: "Pause job",
      group: "Machine",
      keywords: ["feed hold", "!"],
      enabled: () => running() && job.progress?.state.state === "running",
      hint: NO_JOB,
      run: () => void job.pause(),
    },
    {
      id: "machine.job.resume",
      title: "Resume job",
      group: "Machine",
      keywords: ["cycle start", "~", "continue"],
      enabled: () => running() && job.paused,
      hint: "No job is paused.",
      run: () => void job.resume(),
    },
    {
      id: "machine.job.stop",
      title: "Stop job",
      group: "Machine",
      keywords: ["abort", "cancel", "laser off"],
      enabled: () => running() && job.progress?.state.state !== "stopping",
      hint: NO_JOB,
      run: () => void job.stop(),
    },
  );
}

/**
 * Starts following the job app-wide, so a job keeps the app busy even while
 * the Machine tab is closed. Returns the cleanup.
 */
export function startJob(job: MachineJob, connected: () => boolean): () => void {
  const unregister = registerJobActions(job, connected);
  const started = job.start();
  return () => {
    unregister();
    void started.then((stop) => stop());
  };
}
