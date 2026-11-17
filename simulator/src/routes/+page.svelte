<script lang="ts">
  import { onMount } from "svelte";
  import { Cable, RotateCcw, Unplug } from "@lucide/svelte";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import { Separator } from "#lib/components/ui/separator/index.js";
  import BurnView from "#lib/components/burn-view.svelte";
  import FaultPanel from "#lib/components/fault-panel.svelte";
  import ProtocolInspector from "#lib/components/protocol-inspector.svelte";
  import type { HostSnapshot } from "#lib/bindings/HostSnapshot.js";
  import type { PresetInfo } from "#lib/bindings/PresetInfo.js";
  import type { SettingRow } from "#lib/bindings/SettingRow.js";
  import type { Speed } from "#lib/bindings/Speed.js";
  import { clock, coordinate, parsePort, speeds } from "#lib/format.js";
  import * as sim from "#lib/sim.js";
  import favicon from "#lib/assets/favicon.svg";

  let host = $state<HostSnapshot>(sim.previewSnapshot);
  let rows = $state<SettingRow[]>([]);
  let presetList = $state<PresetInfo[]>([]);
  let port = $state("");
  let error = $state("");

  const link = $derived(host.link);
  const machine = $derived(host.machine);
  const axes = ["X", "Y", "Z"] as const;

  async function refresh() {
    try {
      host = await sim.snapshot();
    } catch (cause) {
      error = String(cause);
    }
  }

  async function refreshSettings() {
    rows = await sim.settings();
  }

  async function attempt(action: () => Promise<unknown>) {
    error = "";
    try {
      await action();
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause);
    }
    await refresh();
  }

  // Settings and presets power-cycle the controller, so reload the table after.
  async function reconfigure(action: () => Promise<unknown>) {
    await attempt(action);
    await refreshSettings();
  }

  function applyPort(event: SubmitEvent) {
    event.preventDefault();
    const parsed = parsePort(port);
    if (parsed === null) {
      error = `“${port}” isn’t a TCP port (1–65535).`;
      return;
    }
    void attempt(() => sim.listen(parsed));
  }

  async function editSetting(row: SettingRow, input: HTMLInputElement) {
    const value = Number(input.value);
    if (input.value.trim() === "" || !Number.isFinite(value)) {
      input.value = row.text;
      error = `$${row.id} needs a number.`;
      return;
    }
    await reconfigure(() => sim.setSetting(row.id, value));
    // Show what the controller stored: GRBL truncates integers, and rejected values revert.
    input.value = rows.find((stored) => stored.id === row.id)?.text ?? row.text;
  }

  function chooseSpeed(speed: Speed) {
    void attempt(() => sim.setSpeed(speed));
  }

  onMount(() => {
    void refresh().then(() => {
      port = link.address?.split(":").pop() ?? "23232";
    });
    void refreshSettings();
    void sim.presets().then((list) => (presetList = list));
    if (!sim.desktop) return;
    const status = setInterval(() => void refresh(), 100);
    // A host can change settings with $n=value; pick those up too.
    const table = setInterval(() => void refreshSettings(), 1000);
    return () => {
      clearInterval(status);
      clearInterval(table);
    };
  });
</script>

<div class="flex h-dvh flex-col bg-background text-foreground">
  <header class="flex items-center gap-3 border-b px-4 py-2">
    <img src={favicon} alt="" class="size-6" />
    <span class="font-semibold">Beam Forge Simulator</span>
    {#if !sim.desktop}<Badge variant="outline">Browser preview</Badge>{/if}
    <div class="ml-auto flex items-center gap-2">
      <span class="text-xs text-muted-foreground">Speed</span>
      <div class="flex gap-1" role="group" aria-label="Simulation speed">
        {#each speeds as speed (speed.id)}
          <Button
            size="sm"
            variant={machine.speed === speed.id ? "secondary" : "ghost"}
            aria-pressed={machine.speed === speed.id}
            disabled={!sim.desktop}
            onclick={() => chooseSpeed(speed.id)}>{speed.label}</Button
          >
        {/each}
      </div>
    </div>
  </header>

  {#if error}
    <p role="alert" class="border-b bg-destructive/10 px-4 py-2 text-sm text-destructive">
      {error}
    </p>
  {/if}

  <div class="flex min-h-0 flex-1">
    <aside class="flex w-80 shrink-0 flex-col gap-4 overflow-y-auto border-r p-4">
      <section class="space-y-3" aria-labelledby="link-heading">
        <div class="flex items-center justify-between">
          <h2 id="link-heading" class="text-sm font-semibold">Fake USB</h2>
          {#if link.client}
            <Badge><Cable />Connected</Badge>
          {:else if link.address}
            <Badge variant="secondary"><Unplug />Waiting</Badge>
          {:else}
            <Badge variant="destructive">Not listening</Badge>
          {/if}
        </div>
        <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
          <dt class="text-muted-foreground">Address</dt>
          <dd class="font-mono">{link.address ?? "—"}</dd>
          <dt class="text-muted-foreground">Client</dt>
          <dd class="font-mono">{link.client?.peer ?? "—"}</dd>
          {#if link.refused}
            <dt class="text-muted-foreground">Refused</dt>
            <dd>{link.refused} (one client at a time)</dd>
          {/if}
        </dl>
        {#if link.error}<p class="text-sm text-destructive">{link.error}</p>{/if}
        <form class="flex gap-2" onsubmit={applyPort}>
          <Input
            aria-label="Port"
            inputmode="numeric"
            class="font-mono"
            bind:value={port}
            disabled={!sim.desktop}
          />
          <Button type="submit" variant="outline" disabled={!sim.desktop}>Listen</Button>
        </form>
      </section>

      <Separator />

      <section class="space-y-3" aria-labelledby="machine-heading">
        <div class="flex items-center justify-between">
          <h2 id="machine-heading" class="text-sm font-semibold">Machine</h2>
          <Badge variant={machine.state === "Alarm" ? "destructive" : "outline"}>
            {machine.state}
          </Badge>
        </div>
        <table class="w-full text-sm">
          <thead class="text-xs text-muted-foreground">
            <tr>
              <th class="text-left font-normal"><span class="sr-only">Axis</span></th>
              <th class="text-right font-normal">Machine</th>
              <th class="text-right font-normal">Work</th>
            </tr>
          </thead>
          <tbody class="font-mono">
            {#each axes as axis, index (axis)}
              <tr>
                <th class="text-left font-normal text-muted-foreground">{axis}</th>
                <td class="text-right">{coordinate(machine.machinePosition[index])}</td>
                <td class="text-right">{coordinate(machine.workPosition[index])}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
          <dt class="text-muted-foreground">Clock</dt>
          <dd class="font-mono">{clock(machine.clockMs)}</dd>
          <dt class="text-muted-foreground">Profile</dt>
          <dd>{machine.profile}</dd>
        </dl>
        <Button
          variant="outline"
          class="w-full"
          disabled={!sim.desktop}
          onclick={() => attempt(sim.resetMachine)}><RotateCcw />Reset machine</Button
        >
      </section>

      <Separator />

      <FaultPanel {machine} {attempt} />

      <Separator />

      <section class="space-y-3" aria-labelledby="settings-heading">
        <h2 id="settings-heading" class="text-sm font-semibold">Settings</h2>
        <p class="text-xs text-muted-foreground">
          Loading a preset or changing a setting power-cycles the controller, like flashing its
          EEPROM.
        </p>
        <div class="flex flex-wrap gap-2">
          {#each presetList as preset (preset.id)}
            <Button
              size="sm"
              variant="outline"
              onclick={() => reconfigure(() => sim.loadPreset(preset.id))}>{preset.name}</Button
            >
          {/each}
        </div>
        {#if rows.length}
          <div class="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-1">
            {#each rows as row (row.id)}
              <label for="setting-{row.id}" class="font-mono text-sm text-muted-foreground"
                >${row.id}</label
              >
              <Input
                id="setting-{row.id}"
                class="h-7 text-right font-mono"
                inputmode="decimal"
                value={row.text}
                onchange={(event) => void editSetting(row, event.currentTarget)}
              />
            {/each}
          </div>
        {:else}
          <p class="text-sm text-muted-foreground">No emulator in the browser preview.</p>
        {/if}
      </section>
    </aside>

    <main class="flex min-w-0 flex-1 flex-col">
      <BurnView {machine} />
      <ProtocolInspector />
    </main>
  </div>
</div>
