<script lang="ts">
  import { Unplug } from "@lucide/svelte";
  import type { JobProgress } from "#lib/bindings/JobProgress.js";
  import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
  import { isDone, jobStateLabel } from "#lib/machine/job.svelte.js";
  import { formatPosition, isAlert, mm, stateLabel } from "#lib/machine/status.js";

  /**
   * `status` is the live machine report; `job` the GCode job's progress, shown
   * while it runs; `cursor` is the canvas pointer in mm.
   */
  let {
    connection = "Not connected",
    status = null,
    job = null,
    cursor = null,
  }: {
    connection?: string;
    status?: MachineStatus | null;
    job?: JobProgress | null;
    cursor?: { x: number; y: number } | null;
  } = $props();

  const jobText = $derived.by(() => {
    if (!job || isDone(job.state)) return null;
    const percent = job.total ? ` ${Math.floor((job.acknowledged / job.total) * 100)}%` : "";
    return `${jobStateLabel(job.state)}${percent}`;
  });
</script>

<footer
  class="flex h-7 shrink-0 items-center gap-4 border-t bg-background px-2.5 text-xs text-muted-foreground tabular-nums"
>
  <span class="flex items-center gap-1.5" title="Connection">
    <Unplug class="size-3.5" />{connection}
  </span>
  {#if status}
    <span
      title="Machine state"
      class={isAlert(status.state) ? "font-medium text-destructive" : "text-foreground"}
    >
      {stateLabel(status.state)}
    </span>
  {/if}
  {#if job && jobText}
    <span title="Job: {job.name}" class="text-foreground">{jobText}</span>
  {/if}
  <span title="Work position">Work {formatPosition(status?.workPosition)}</span>
  <span title="Machine position">Machine {formatPosition(status?.machinePosition)}</span>
  <span class="ml-auto" title="Cursor position">
    Cursor
    {#if cursor}X {mm(cursor.x)} Y {mm(cursor.y)} mm{:else}—{/if}
  </span>
</footer>
