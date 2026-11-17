<script lang="ts">
  import { untrack } from "svelte";
  import { Timer } from "@lucide/svelte";
  import * as Tooltip from "#lib/components/ui/tooltip/index.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import { jobSetup } from "#lib/gcode/job-setup.svelte.js";
  import { machineSettings } from "#lib/machine/settings.svelte.js";
  import { jobTime } from "./estimate.svelte.js";
  import { clock } from "./timeline.js";

  /**
   * The design's estimated job time for the top bar, with each layer's time
   * in its tooltip. Mount once per window: it re-estimates (debounced) when
   * the design, machine or placement changes.
   */

  /** Re-estimate this long after the last change, so typing a value estimates once. */
  const DEBOUNCE_MS = 400;

  $effect(() => {
    if (!jobTime.available) return;
    void [editor.doc, machineSettings.profile, jobSetup.placement];
    // Estimate at once the first time; untracked, or each estimate would trigger the next.
    const delay = untrack(() => (jobTime.estimate || jobTime.problem ? DEBOUNCE_MS : 0));
    const timer = setTimeout(() => untrack(() => void jobTime.refresh()), delay);
    return () => clearTimeout(timer);
  });

  const estimate = $derived(jobTime.estimate);
  const layerOf = (id: string) => editor.doc.layers.find((layer) => layer.id === id);
</script>

{#if estimate}
  <Tooltip.Root>
    <Tooltip.Trigger
      class="flex items-center gap-1 rounded-md px-1.5 py-1 text-xs text-muted-foreground tabular-nums hover:text-foreground"
      aria-label={`Estimated job time ${clock(estimate.seconds)}`}
    >
      <Timer class="size-3.5" />~{clock(estimate.seconds)}
    </Tooltip.Trigger>
    <Tooltip.Content>
      <p class="font-medium">Estimated job time</p>
      <ul class="mt-1 space-y-0.5 tabular-nums">
        {#each estimate.layers as layer, index (index)}
          {@const cut = layerOf(layer.layerId)}
          <li class="flex items-center gap-2">
            <span
              class="size-2 shrink-0 rounded-full"
              style:background-color={cut?.color ?? "currentColor"}
              aria-hidden="true"
            ></span>
            <span class="flex-1 truncate">{cut?.name ?? layer.layerId}</span>
            <span>{clock(layer.seconds)}</span>
          </li>
        {/each}
      </ul>
      <p class="mt-1 opacity-80">From the machine profile's rates and accelerations</p>
    </Tooltip.Content>
  </Tooltip.Root>
{/if}
