<script lang="ts">
  import { DoorClosed, DoorOpen, Hand, Play, Siren, Unplug, X } from "@lucide/svelte";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import type { MachineSnapshot } from "#lib/bindings/MachineSnapshot.js";
  import { alarms, errors, exampleScenario, wholeNumber } from "#lib/faults.js";
  import * as sim from "#lib/sim.js";

  interface Props {
    machine: MachineSnapshot;
    /** Runs an action, showing any failure in the window's error banner. */
    attempt: (action: () => Promise<unknown>) => Promise<void>;
  }

  let { machine, attempt }: Props = $props();

  let alarm = $state(1);
  let error = $state(20);
  let errorLine = $state("1");
  let stall = $state("2000");
  let latency = $state("50");
  let rx = $state("64");
  let script = $state(exampleScenario);

  const field =
    "h-8 w-full min-w-0 rounded-lg border border-input bg-transparent px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50 dark:bg-input/30";

  function inject(fault: string) {
    return attempt(() => sim.injectFault(fault));
  }

  /** Injects `make(n)` for a whole number typed in a field, or explains the problem. */
  function withNumber(text: string, what: string, make: (n: number) => string) {
    const n = wholeNumber(text);
    return attempt(async () => {
      if (n === null) throw new Error(`${what} needs a whole number.`);
      await sim.injectFault(make(n));
    });
  }
</script>

<section class="space-y-3" aria-labelledby="faults-heading">
  <div class="flex items-center justify-between">
    <h2 id="faults-heading" class="text-sm font-semibold">Faults</h2>
    <Button
      size="xs"
      variant="ghost"
      disabled={!sim.desktop}
      onclick={() => attempt(sim.clearFaults)}><X />Clear faults</Button
    >
  </div>
  <p class="text-xs text-muted-foreground">
    Faults show in the protocol inspector. Clearing ends stalls, latency, a smaller RX buffer,
    pending errors and the scenario; alarms need a reset, as on the real board.
  </p>

  <div class="grid grid-cols-[1fr_auto] gap-2">
    <select aria-label="Alarm" class={field} bind:value={alarm} disabled={!sim.desktop}>
      {#each alarms as option (option.code)}
        <option value={option.code}>ALARM:{option.code} {option.name}</option>
      {/each}
    </select>
    <Button
      size="sm"
      variant="destructive"
      disabled={!sim.desktop}
      onclick={() => inject(`alarm ${alarm}`)}><Siren />Raise</Button
    >

    <div class="flex gap-2">
      <select aria-label="Error" class={field} bind:value={error} disabled={!sim.desktop}>
        {#each errors as option (option.code)}
          <option value={option.code}>error:{option.code} {option.name}</option>
        {/each}
      </select>
      <Input
        aria-label="Line, counting from the next (1)"
        title="Line, counting from the next (1)"
        class="w-14 font-mono"
        inputmode="numeric"
        bind:value={errorLine}
        disabled={!sim.desktop}
      />
    </div>
    <Button
      size="sm"
      variant="outline"
      disabled={!sim.desktop}
      onclick={() => withNumber(errorLine, "The line", (line) => `error ${error} line ${line}`)}
      >Answer</Button
    >
  </div>

  <div class="grid grid-cols-2 gap-2">
    {#if machine.state.startsWith("Door")}
      <Button
        size="sm"
        variant="outline"
        disabled={!sim.desktop}
        onclick={() => inject("door closed")}><DoorClosed />Close door</Button
      >
    {:else}
      <Button
        size="sm"
        variant="outline"
        disabled={!sim.desktop}
        onclick={() => inject("door open")}><DoorOpen />Open door</Button
      >
    {/if}
    <Button size="sm" variant="outline" disabled={!sim.desktop} onclick={() => inject("hold")}
      ><Hand />Feed hold</Button
    >
    <Button
      size="sm"
      variant="outline"
      class="col-span-2"
      disabled={!sim.desktop}
      onclick={() => inject("disconnect")}><Unplug />Pull the USB cable</Button
    >
  </div>

  <div class="grid grid-cols-[1fr_5rem_auto] items-center gap-2 text-sm">
    <label for="fault-stall" class="text-muted-foreground">Stall, ms</label>
    <Input
      id="fault-stall"
      class="font-mono"
      inputmode="numeric"
      bind:value={stall}
      disabled={!sim.desktop}
    />
    <Button
      size="sm"
      variant="outline"
      disabled={!sim.desktop}
      onclick={() => withNumber(stall, "The stall", (ms) => `stall ${ms}ms`)}>Stall</Button
    >
    <label for="fault-latency" class="text-muted-foreground">Ack latency, ms</label>
    <Input
      id="fault-latency"
      class="font-mono"
      inputmode="numeric"
      bind:value={latency}
      disabled={!sim.desktop}
    />
    <Button
      size="sm"
      variant="outline"
      disabled={!sim.desktop}
      onclick={() => withNumber(latency, "The latency", (ms) => `latency ${ms}ms`)}>Set</Button
    >
    <label for="fault-rx" class="text-muted-foreground">RX buffer, bytes</label>
    <Input
      id="fault-rx"
      class="font-mono"
      inputmode="numeric"
      bind:value={rx}
      disabled={!sim.desktop}
    />
    <Button
      size="sm"
      variant="outline"
      disabled={!sim.desktop}
      onclick={() => withNumber(rx, "The RX buffer", (bytes) => `rx ${bytes}`)}>Shrink</Button
    >
  </div>

  <div class="space-y-2">
    <div class="flex items-center justify-between">
      <label for="fault-scenario" class="text-sm font-medium">Scenario</label>
      {#if machine.scenarioPending}
        <Badge variant="secondary">{machine.scenarioPending} to fire</Badge>
      {/if}
    </div>
    <textarea
      id="fault-scenario"
      class="{field} h-36 resize-y py-1 font-mono text-xs"
      spellcheck="false"
      bind:value={script}
      disabled={!sim.desktop}></textarea>
    <Button
      size="sm"
      variant="outline"
      class="w-full"
      disabled={!sim.desktop}
      onclick={() => attempt(() => sim.runScenario(script))}><Play />Run scenario</Button
    >
  </div>
</section>
