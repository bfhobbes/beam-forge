<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { LoaderCircle, Pause, Play } from "@lucide/svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import { jobSetup } from "#lib/gcode/job-setup.svelte.js";
  import { machineSettings } from "#lib/machine/settings.svelte.js";
  import { SPEEDS, toolpathPreview as preview } from "./preview.svelte.js";
  import { clock } from "./timeline.js";

  /**
   * The toolpath view's docked playback bar: play/pause, the scrubber over
   * the job's timeline, speed, and the elapsed time and layer at the
   * playhead. Mount once per window: it also re-plans the job (debounced)
   * while the toolpath is shown, and registers the toolpath actions.
   */

  /** Re-plan this long after the last change, so typing a value plans once. */
  const DEBOUNCE_MS = 250;

  onMount(() =>
    actions.register(
      {
        id: "view.toolpath",
        title: "Toggle toolpath preview",
        group: "View",
        keywords: ["preview", "simulate", "playback", "gcode", "path", "design"],
        run: () => preview.toggle(),
      },
      {
        id: "view.toolpath-play",
        title: "Play or pause toolpath preview",
        group: "View",
        keywords: ["preview", "simulate", "playback"],
        enabled: () => preview.shown && preview.timeline !== null && !preview.external,
        hint: "Show the toolpath preview first",
        run: () => preview.playPause(),
      },
    ),
  );

  // Re-plan when the design, machine or placement changes, while shown.
  $effect(() => {
    if (!preview.shown) return;
    void [editor.doc, machineSettings.profile, jobSetup.placement];
    // Plan at once the first time; untracked, or each plan would trigger the next.
    const delay = untrack(() => (preview.toolpath || preview.problem ? DEBOUNCE_MS : 0));
    const timer = setTimeout(() => untrack(() => void preview.refresh()), delay);
    return () => clearTimeout(timer);
  });

  // The playback clock: one tick per animation frame while playing.
  $effect(() => {
    if (!preview.playing) return;
    let last = performance.now();
    let id = requestAnimationFrame(function step(now) {
      preview.tick(now - last);
      last = now;
      id = requestAnimationFrame(step);
    });
    return () => cancelAnimationFrame(id);
  });

  const timeline = $derived(preview.timeline);
  const progress = $derived(preview.progress);
  const layerName = $derived.by(() => {
    if (!timeline || !progress || progress.layer < 0) return null;
    const id = timeline.layers[progress.layer]!.layerId;
    return editor.doc.layers.find((layer) => layer.id === id)?.name ?? id;
  });
  const colorOf = (layerId: string) =>
    editor.doc.layers.find((layer) => layer.id === layerId)?.color ?? "currentColor";
  const following = $derived(preview.external !== null);
</script>

<div
  class="flex flex-col gap-1 border-t bg-background px-3 py-2 text-xs"
  role="region"
  aria-label="Toolpath playback"
>
  {#if !timeline}
    <p class="flex min-h-7 items-center gap-2 text-muted-foreground" role="status">
      {#if preview.loading}
        <LoaderCircle class="size-3.5 animate-spin" />Planning the toolpath…
      {:else if preview.problem}
        {preview.problem}
      {/if}
    </p>
  {:else}
    <div class="flex items-center gap-3">
      <Button
        size="icon-sm"
        variant="outline"
        aria-label={preview.playing ? "Pause" : "Play"}
        disabled={following || timeline.duration <= 0}
        onclick={() => preview.playPause()}
      >
        {#if preview.playing}<Pause />{:else}<Play />{/if}
      </Button>
      <div class="flex min-w-40 flex-1 flex-col gap-0.5">
        <input
          type="range"
          aria-label="Playback position"
          class="w-full accent-primary"
          min="0"
          max={timeline.duration}
          step="any"
          value={progress?.time ?? 0}
          disabled={following || timeline.duration <= 0}
          oninput={(event) => preview.scrub(Number(event.currentTarget.value))}
        />
        <!-- Each layer's share of the job, in its colour. -->
        <div class="flex h-1 overflow-hidden rounded-full bg-muted" aria-hidden="true">
          {#each timeline.layers as layer, index (index)}
            {@const end = timeline.layers[index + 1]?.start ?? timeline.duration}
            <div
              class="h-full"
              style:background-color={colorOf(layer.layerId)}
              style:flex-grow={Math.max(0, end - layer.start)}
            ></div>
          {/each}
        </div>
      </div>
      <span class="font-mono tabular-nums" title="Elapsed / total, at feed rate">
        {clock(progress?.time ?? 0)} / {clock(timeline.duration)}
      </span>
      <div class="flex gap-1" role="group" aria-label="Playback speed">
        {#each SPEEDS as speed (speed)}
          <Button
            size="xs"
            variant={preview.speed === speed ? "secondary" : "ghost"}
            aria-pressed={preview.speed === speed}
            disabled={following}
            onclick={() => preview.setSpeed(speed)}>{speed}×</Button
          >
        {/each}
      </div>
    </div>
    <!-- Not a live region: it changes every frame while playing. -->
    <p class="flex flex-wrap gap-x-3 text-muted-foreground">
      <span>
        Layer
        <span class="text-foreground">{layerName ?? "–"}</span>
        {#if progress && progress.layer >= 0}({progress.layer + 1} of {timeline.layers.length}){/if}
      </span>
      <span class="tabular-nums">
        Move {Math.min(progress?.completed ?? 0, timeline.count)} of {timeline.count}
      </span>
      <span class="ml-auto">Times at feed rate, without acceleration</span>
    </p>
  {/if}
</div>
