<script lang="ts">
  import { onMount } from "svelte";
  import { ArrowDownToLine, Copy, Pause, Save, Trash2 } from "@lucide/svelte";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import BufferGraph from "#lib/components/buffer-graph.svelte";
  import type { BufferSample } from "#lib/bindings/BufferSample.js";
  import {
    defaultFilter,
    exportText,
    filterKinds,
    ProtocolLog,
    segments,
    visible,
    type Entry,
    type Tone,
  } from "#lib/inspector.js";
  import { selection } from "#lib/selection.svelte.js";
  import * as sim from "#lib/sim.js";

  /** Samples the graphs span: all the host keeps (15 s). */
  const SPAN = 300;

  const log = new ProtocolLog();
  let entries = $state.raw<Entry[]>([]);
  let samples = $state.raw<BufferSample[]>([]);
  let rxCapacity = $state(sim.previewInspector.rxCapacity);
  let plannerCapacity = $state(sim.previewInspector.plannerCapacity);
  let kinds = $state({ ...defaultFilter.kinds });
  let search = $state("");
  let follow = $state(true);
  let notice = $state("");
  let scroller = $state<HTMLDivElement>();

  const shown = $derived(visible(entries, { kinds, search }));

  const tones: Record<Tone, string> = {
    time: "text-muted-foreground",
    out: "text-sky-700 dark:text-sky-300",
    in: "text-foreground",
    realtime: "rounded bg-amber-500/15 px-1 text-amber-700 dark:text-amber-300",
    error: "font-semibold text-destructive",
    muted: "text-muted-foreground",
    event: "text-violet-700 dark:text-violet-300",
  };

  let since = 0;
  let polling = false;

  async function poll() {
    if (polling) return;
    polling = true;
    try {
      const batch = await sim.inspector(since);
      if (batch.missed && since > 0) log.gap(batch.events[0]?.at ?? 0);
      since = batch.next;
      if (batch.events.length || batch.missed) {
        log.apply(batch.events);
        entries = log.entries.slice();
      }
      samples = batch.samples;
      rxCapacity = batch.rxCapacity;
      plannerCapacity = batch.plannerCapacity;
    } catch (cause) {
      notice = `Inspector: ${cause instanceof Error ? cause.message : String(cause)}`;
    } finally {
      polling = false;
    }
  }

  function clear() {
    log.clear();
    entries = [];
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(exportText(shown));
      notice = `Copied ${shown.length} lines.`;
    } catch (cause) {
      notice = `Couldn't copy: ${String(cause)}`;
    }
  }

  async function save() {
    try {
      notice = `Saved ${shown.length} lines to ${await sim.exportLog(exportText(shown))}`;
    } catch (cause) {
      notice = cause instanceof Error ? cause.message : String(cause);
    }
  }

  // Keep the newest line in view unless auto-scroll is paused.
  $effect(() => {
    void shown;
    if (follow && scroller) scroller.scrollTop = scroller.scrollHeight;
  });

  onMount(() => {
    void poll();
    if (!sim.desktop) return;
    const timer = setInterval(() => void poll(), 100);
    return () => clearInterval(timer);
  });
</script>

<section class="flex min-h-0 flex-1 flex-col" aria-labelledby="inspector-heading">
  <div class="flex flex-wrap items-center gap-2 border-b px-4 py-2">
    <h2 id="inspector-heading" class="mr-2 text-sm font-semibold">Protocol inspector</h2>
    <div class="flex flex-wrap gap-1" role="group" aria-label="Show entry types">
      {#each filterKinds as kind (kind.id)}
        <Button
          size="xs"
          variant={kinds[kind.id] ? "secondary" : "ghost"}
          aria-pressed={kinds[kind.id]}
          onclick={() => (kinds[kind.id] = !kinds[kind.id])}>{kind.label}</Button
        >
      {/each}
    </div>
    <Input aria-label="Search the log" placeholder="Search" class="h-7 w-44" bind:value={search} />
    <div class="ml-auto flex gap-1">
      <Button
        size="sm"
        variant={follow ? "ghost" : "secondary"}
        aria-pressed={!follow}
        onclick={() => (follow = !follow)}
      >
        {#if follow}<Pause />Pause scroll{:else}<ArrowDownToLine />Follow{/if}
      </Button>
      <Button size="sm" variant="ghost" onclick={clear}><Trash2 />Clear</Button>
      <Button size="sm" variant="ghost" onclick={copy} disabled={!shown.length}><Copy />Copy</Button
      >
      <Button size="sm" variant="outline" onclick={save} disabled={!sim.desktop || !shown.length}
        ><Save />Export</Button
      >
    </div>
  </div>

  <div class="grid grid-cols-2 gap-4 border-b px-4 py-2">
    <BufferGraph
      label="RX buffer"
      unit="bytes"
      values={samples.map((sample) => sample.rx)}
      overflowed={samples.map((sample) => sample.overflowed > 0)}
      capacity={rxCapacity}
      span={SPAN}
    />
    <BufferGraph
      label="Planner"
      unit="blocks"
      values={samples.map((sample) => sample.planner)}
      capacity={plannerCapacity}
      span={SPAN}
    />
  </div>

  {#if notice}
    <p class="border-b px-4 py-1 text-xs text-muted-foreground" role="status">{notice}</p>
  {/if}

  <div
    bind:this={scroller}
    class="min-h-0 flex-1 overflow-auto px-4 py-2 font-mono text-xs leading-5"
    role="log"
    aria-label="Wire traffic"
  >
    {#snippet text(entry: Entry)}
      {#each segments(entry) as segment, index (index)}<span class={tones[segment.tone]}
          >{segment.text}</span
        >{/each}
    {/snippet}
    {#each shown as entry (entry.id)}
      {@const line = entry.line}
      {#if line === undefined}
        <div class={["whitespace-pre", entry.error && "bg-destructive/10"]}>
          {@render text(entry)}
        </div>
      {:else}
        <!-- A line can be picked to highlight what it burned. -->
        <button
          type="button"
          title="Line {line}: highlight its burn"
          aria-pressed={selection.line === line}
          class={[
            "block w-full text-left whitespace-pre hover:bg-muted",
            entry.error && "bg-destructive/10",
            selection.line === line && "bg-amber-500/20 hover:bg-amber-500/25",
          ]}
          onclick={() => (selection.line = selection.line === line ? null : line)}
        >
          {@render text(entry)}
        </button>
      {/if}
    {:else}
      <p class="font-sans text-sm text-muted-foreground">
        {sim.desktop
          ? "No traffic yet. Connect a host to see every byte it exchanges."
          : "No emulator in the browser preview."}
      </p>
    {/each}
  </div>
</section>
