<script lang="ts">
  import { Plug, RefreshCw, Unplug } from "@lucide/svelte";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { BAUD_RATES, machine } from "./connection.svelte.js";
  import StatusReadout from "./StatusReadout.svelte";
  import LaserModeWarning from "./LaserModeWarning.svelte";
  import MachineSettingsDialog from "./MachineSettingsDialog.svelte";
  import SafetyNotice from "./SafetyNotice.svelte";

  const selectClass =
    "h-8 min-w-0 rounded-lg border border-input bg-background px-2 text-sm disabled:opacity-50";
  const busy = $derived(machine.state.status === "connecting");
  const locked = $derived(machine.state.status !== "disconnected");
  // Baud rate means nothing to the simulators, reached over TCP.
  const kind = $derived(machine.selectedPort?.address.kind);
  const noBaud = $derived(kind === "tcp" || kind === "launch");
</script>

<section aria-label="Machine connection" class="space-y-3">
  <div class="flex items-center justify-between gap-2">
    <h2 class="text-sm font-medium">Machine</h2>
    <Badge variant={machine.connected ? "default" : "outline"} role="status">
      {machine.statusText}
    </Badge>
  </div>
  <div class="flex gap-2">
    <select
      aria-label="Port"
      class="{selectClass} flex-1"
      value={machine.portKey}
      disabled={locked || machine.ports.length === 0}
      onchange={(event) => machine.select(event.currentTarget.value)}
    >
      {#each machine.ports as port (port.key)}
        <option value={port.key}>{port.label}</option>
      {:else}
        <option value="">No ports found</option>
      {/each}
    </select>
    <Button
      variant="outline"
      size="icon"
      aria-label="Refresh ports"
      disabled={locked || machine.refreshing}
      onclick={() => machine.refresh()}
    >
      <RefreshCw class={machine.refreshing ? "animate-spin" : ""} />
    </Button>
  </div>
  <div class="flex gap-2">
    <select
      aria-label="Baud rate"
      class={selectClass}
      value={machine.baud}
      disabled={locked || noBaud}
      onchange={(event) => machine.setBaud(Number(event.currentTarget.value))}
    >
      {#each BAUD_RATES as baud (baud)}
        <option value={baud}>{baud} baud</option>
      {/each}
    </select>
    {#if locked}
      <Button class="flex-1" variant="outline" onclick={() => machine.disconnect()}>
        <Unplug />{busy ? "Cancel" : "Disconnect"}
      </Button>
    {:else}
      <Button class="flex-1" onclick={() => machine.connect()}><Plug />Connect</Button>
    {/if}
  </div>
  <SafetyNotice />
  {#if !machine.available}
    <p class="text-xs text-muted-foreground">
      Browser preview: machine connection needs the desktop app.
    </p>
  {/if}
  {#if machine.problem}
    <p role="alert" class="text-sm text-destructive">{machine.problem}</p>
  {/if}
  {#if machine.connected}
    <StatusReadout status={machine.status} />
  {/if}
  <LaserModeWarning />
  <MachineSettingsDialog />
</section>
