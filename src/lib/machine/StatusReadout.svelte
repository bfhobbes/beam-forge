<script lang="ts">
  import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
  import { airAssist, isAlert, mm, stateLabel } from "./status.js";

  let { status }: { status: MachineStatus | null } = $props();

  const axes = ["x", "y", "z"] as const;
  const air = $derived(status ? airAssist(status.accessories) : null);
</script>

<section aria-label="Machine status" class="space-y-2 text-sm">
  <div class="flex items-center justify-between gap-2">
    <span class="text-muted-foreground">State</span>
    <span
      role="status"
      class={status && isAlert(status.state) ? "font-medium text-destructive" : "font-medium"}
    >
      {status ? stateLabel(status.state) : "Waiting for status…"}
    </span>
  </div>
  <table class="w-full text-right tabular-nums">
    <thead class="text-xs text-muted-foreground">
      <tr>
        <th class="text-left font-normal"><span class="sr-only">Axis</span></th>
        <th class="font-normal">Work</th>
        <th class="font-normal">Machine</th>
      </tr>
    </thead>
    <tbody>
      {#each axes as axis (axis)}
        <tr>
          <th scope="row" class="text-left font-medium uppercase">{axis}</th>
          <td>{status?.workPosition ? mm(status.workPosition[axis]) : "—"}</td>
          <td class="text-muted-foreground">
            {status?.machinePosition ? mm(status.machinePosition[axis]) : "—"}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if status}
    <dl class="grid grid-cols-2 gap-x-2 text-xs text-muted-foreground">
      <dt>Feed</dt>
      <dd class="text-right tabular-nums">{Math.round(status.feed)} mm/min</dd>
      {#if status.spindle !== null}
        <dt>Power (S)</dt>
        <dd class="text-right tabular-nums">{Math.round(status.spindle)}</dd>
      {/if}
      {#if status.overrides}
        <dt>Overrides</dt>
        <dd class="text-right tabular-nums">
          {status.overrides.feed}% · {status.overrides.rapid}% · {status.overrides.spindle}%
        </dd>
      {/if}
      {#if air !== null}
        <dt>Air assist</dt>
        <dd class="text-right">{air ? "On" : "Off"}</dd>
      {/if}
    </dl>
  {/if}
</section>
