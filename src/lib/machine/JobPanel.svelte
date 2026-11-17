<script lang="ts">
  import { FileCode, Frame, Pause, Play, Square } from "@lucide/svelte";
  import { frameLaserLabel, jobSetup } from "#lib/gcode/job-setup.svelte.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import { machine } from "./connection.svelte.js";
  import { machineSettings } from "./settings.svelte.js";
  import { formatElapsed, jobStateLabel, machineJob as job } from "./job.svelte.js";
  import OverrideSliders from "./OverrideSliders.svelte";
  import { estimateError, formatError } from "./run.svelte.js";

  const progress = $derived(job.progress);
  const jobState = $derived(progress?.state);
  const percent = $derived(job.fraction === null ? null : Math.round(job.fraction * 100));
  /** The design run this progress belongs to, if it is one. */
  const run = $derived(job.run && job.run.name === progress?.name ? job.run : null);
  const layerName = $derived.by(() => {
    if (!run || run.layer < 0) return null;
    const id = run.timeline.layers[run.layer]!.layerId;
    return editor.doc.layers.find((layer) => layer.id === id)?.name ?? id;
  });
  const outcomeError = $derived(run?.outcome ? estimateError(run.outcome) : null);
  const ESTIMATE_HINT = "Estimated from the machine profile's rates and accelerations";
</script>

<section aria-label="Job" class="space-y-2">
  <div class="flex items-center justify-between gap-2">
    <h2 class="text-sm font-medium">Job</h2>
    {#if !job.active}
      <div class="flex gap-1.5">
        <Button
          size="sm"
          disabled={!machine.connected || !jobSetup.available}
          title={machine.connected
            ? "Generate GCode from the design and run it"
            : "Connect to a machine first."}
          onclick={() => jobSetup.open("run")}
        >
          <Play />Start job…
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={!jobSetup.canFrame}
          title={machine.connected
            ? `Trace the job's outline on the bed, placed as last set up. ${frameLaserLabel(machineSettings.profile)}.`
            : "Connect to a machine first."}
          onclick={() => void jobSetup.frame()}
        >
          <Frame />Frame
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={!machine.connected}
          title={machine.connected ? undefined : "Connect to a machine first."}
          onclick={() => job.runFile()}
        >
          <FileCode />Run file…
        </Button>
      </div>
    {/if}
  </div>

  {#if progress && jobState}
    <div class="space-y-1.5">
      <div class="flex items-baseline justify-between gap-2 text-sm">
        <span class="truncate" title={progress.name}>{progress.name}</span>
        <span
          role="status"
          class={jobState.state === "failed" || jobState.state === "rejected"
            ? "shrink-0 font-medium text-destructive"
            : "shrink-0 text-muted-foreground"}
        >
          {jobStateLabel(jobState)}
        </span>
      </div>
      <div
        role="progressbar"
        aria-label="Lines acknowledged"
        aria-valuemin={0}
        aria-valuemax={progress.total ?? undefined}
        aria-valuenow={progress.acknowledged}
        class="h-1.5 overflow-hidden rounded-full bg-muted"
      >
        <div
          class="h-full rounded-full bg-primary transition-[width]"
          style:width="{percent ?? 0}%"
        ></div>
      </div>
      <div class="flex justify-between text-xs text-muted-foreground tabular-nums">
        <span>
          {progress.acknowledged}{progress.total === null ? "" : ` / ${progress.total}`} lines
          {#if percent !== null}· {percent}%{/if}
        </span>
        <span title="Elapsed">{formatElapsed(progress.elapsedMs)}</span>
      </div>
      {#if run && job.active}
        <div class="flex justify-between gap-2 text-xs text-muted-foreground tabular-nums">
          <span class="truncate">
            Layer <span class="text-foreground">{layerName ?? "–"}</span>
            {#if run.layer >= 0}({run.layer + 1} of {run.timeline.layers.length}){/if}
          </span>
          <span class="shrink-0" title={ESTIMATE_HINT}>
            ~{formatElapsed(run.remaining * 1000)} left
          </span>
        </div>
      {/if}
    </div>

    {#if run?.outcome}
      <p role="status" class="text-xs text-muted-foreground tabular-nums" title={ESTIMATE_HINT}>
        {run.outcome.state.state === "finished" ? "Finished in" : "Ran for"}
        <span class="text-foreground">{formatElapsed(run.outcome.actualMs)}</span>
        · estimated {formatElapsed(run.outcome.estimateMs)}
        {#if outcomeError !== null}({formatError(outcomeError)}){/if}
      </p>
    {/if}

    {#if jobState.state === "rejected"}
      <div role="alert" class="space-y-2 rounded-lg border border-destructive/40 p-2 text-sm">
        <p>
          The machine rejected line {jobState.line}
          <code class="rounded bg-muted px-1 font-mono text-xs">{jobState.command}</code>
          and is holding.
        </p>
        <p class="text-xs text-muted-foreground">{jobState.message}</p>
        <div class="flex gap-2">
          <Button size="sm" class="flex-1" onclick={() => job.resume()}>
            <Play />Continue
          </Button>
          <Button size="sm" variant="destructive" class="flex-1" onclick={() => job.stop()}>
            <Square />Abort
          </Button>
        </div>
      </div>
    {:else if job.active}
      <OverrideSliders />
      <div class="flex gap-2">
        {#if jobState.state === "paused"}
          <Button size="sm" class="flex-1" onclick={() => job.resume()}><Play />Resume</Button>
        {:else}
          <Button
            size="sm"
            variant="outline"
            class="flex-1"
            disabled={jobState.state !== "running"}
            onclick={() => job.pause()}
          >
            <Pause />Pause
          </Button>
        {/if}
        <Button
          size="sm"
          variant="destructive"
          class="flex-1"
          disabled={jobState.state === "stopping"}
          onclick={() => job.stop()}
        >
          <Square />Stop
        </Button>
      </div>
    {:else if jobState.state === "stopped" && jobState.warning}
      <p role="alert" class="text-sm text-destructive">{jobState.warning}</p>
    {:else if jobState.state === "failed"}
      <p role="alert" class="text-sm text-destructive">{jobState.reason}</p>
    {/if}
  {/if}

  {#if job.error}
    <p role="alert" class="text-sm text-destructive">{job.error}</p>
  {/if}
</section>
