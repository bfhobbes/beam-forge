<script lang="ts">
  import { TriangleAlert } from "@lucide/svelte";
  import { Button } from "#lib/components/ui/button/index.js";
  import { machineSettings } from "./settings.svelte.js";

  let confirming = $state(false);
  // Fixable only while the controller is connected and its settings are read.
  const connected = $derived(machineSettings.settings.length > 0);

  async function turnOn() {
    if (await machineSettings.write(32, "1")) confirming = false;
  }
</script>

{#if machineSettings.laserModeOff}
  <div
    role="alert"
    class="space-y-2 rounded-lg border border-destructive/50 bg-destructive/10 p-3 text-sm"
  >
    <p class="flex items-center gap-2 font-medium text-destructive">
      <TriangleAlert class="size-4 shrink-0" />Laser mode is off ($32=0)
    </p>
    <p class="text-xs">
      GRBL stops at the end of every move, so the laser lingers and burns dots at every corner and
      curve. Set $32 to 1 before running a job.
    </p>
    {#if connected}
      {#if confirming}
        <p class="text-xs font-medium">Write $32=1 to the controller?</p>
        <div class="flex gap-2">
          <Button
            size="sm"
            variant="destructive"
            disabled={machineSettings.writing !== null}
            onclick={turnOn}
          >
            Write $32=1
          </Button>
          <Button size="sm" variant="outline" onclick={() => (confirming = false)}>Cancel</Button>
        </div>
      {:else}
        <Button size="sm" variant="destructive" onclick={() => (confirming = true)}>
          Turn on laser mode…
        </Button>
      {/if}
    {/if}
  </div>
{/if}
