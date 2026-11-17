<script lang="ts">
  import { RotateCcw } from "@lucide/svelte";
  import type { OverrideKind } from "#lib/bindings/OverrideKind.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { machine } from "./connection.svelte.js";
  import { machineJob as job } from "./job.svelte.js";
  import {
    OVERRIDE_DEFAULT,
    OVERRIDE_LABELS,
    OVERRIDE_MAX,
    OVERRIDE_MIN,
    reportedOverride,
    sliderValue,
  } from "./overrides.js";

  /**
   * Live feed and laser power overrides for the running job. Each slider
   * shows its target until the machine reports reaching it.
   */
  const KINDS: readonly OverrideKind[] = ["feed", "spindle"];
  const HINTS: Record<OverrideKind, string> = {
    feed: "Feed rate of cuts, as a percentage of each layer's speed",
    spindle: "Laser power, as a percentage of each layer's power",
  };
</script>

<div role="group" aria-label="Overrides" class="space-y-1.5">
  {#each KINDS as kind (kind)}
    {@const reported = reportedOverride(machine.status?.overrides, kind)}
    {@const value = sliderValue(job.overrideTargets[kind], reported)}
    {@const pending = value !== reported}
    <div class="flex items-center gap-2 text-xs">
      <label for="override-{kind}" class="w-10 shrink-0 text-muted-foreground" title={HINTS[kind]}>
        {OVERRIDE_LABELS[kind]}
      </label>
      <input
        id="override-{kind}"
        type="range"
        class="min-w-0 flex-1 accent-primary"
        min={OVERRIDE_MIN}
        max={OVERRIDE_MAX}
        step="1"
        {value}
        aria-valuetext="{value}%"
        oninput={(event) => void job.setOverride(kind, Number(event.currentTarget.value))}
      />
      <span
        class={pending
          ? "w-16 text-right text-muted-foreground tabular-nums"
          : "w-16 text-right tabular-nums"}
        title={pending
          ? `Machine at ${reported ?? "?"}%, moving to ${value}%`
          : "As the machine reports it"}
      >
        {value}%{#if pending}…{/if}
      </span>
      <Button
        size="icon-xs"
        variant="ghost"
        aria-label="Reset {OVERRIDE_LABELS[kind].toLowerCase()} to 100%"
        disabled={value === OVERRIDE_DEFAULT}
        onclick={() => void job.setOverride(kind, OVERRIDE_DEFAULT)}
      >
        <RotateCcw />
      </Button>
    </div>
  {/each}
</div>
