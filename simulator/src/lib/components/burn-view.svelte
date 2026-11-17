<script lang="ts">
  import { onMount } from "svelte";
  import { Eraser, ImageDown, Maximize, Pause, Play, Radio, Wind, X, Zap } from "@lucide/svelte";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import type { BurnSegment } from "#lib/bindings/BurnSegment.js";
  import type { MachineSnapshot } from "#lib/bindings/MachineSnapshot.js";
  import {
    BurnModel,
    gridStep,
    HIGHLIGHT,
    modal,
    partial,
    styleOf,
    type Colouring,
    type DrawOptions,
    type Layer,
    type Style,
  } from "#lib/burn.js";
  import { clock } from "#lib/format.js";
  import { selection } from "#lib/selection.svelte.js";
  import * as sim from "#lib/sim.js";

  interface Props {
    machine: MachineSnapshot;
  }

  let { machine }: Props = $props();

  /** Playback speeds: simulated ms per wall ms. */
  const rates = [1, 10, 100];
  /** How close (screen px) a click must be to pick a burn. */
  const PICK_RADIUS = 6;
  const colourings: { id: Colouring; label: string }[] = [
    { id: "power", label: "By power" },
    { id: "pass", label: "By pass" },
  ];
  const OUTSIDE = "#d4d4d8";
  const MATERIAL = "#f3eadb";

  // The trace is large and append-only, so it lives outside runes; `version`
  // tells the view it changed.
  let model = new BurnModel();
  let version = $state(0);
  let generation: number | null = null;
  let since = 0;
  let truncated = $state(false);
  let notice = $state("");

  /** Following the machine, or replaying the trace at `cursor` (app clock ms). */
  let live = $state(true);
  let playing = $state(false);
  let cursor = $state(0);
  let rate = $state(10);
  let showTravel = $state(true);
  let colouring = $state<Colouring>("power");

  /** Bed mm at the canvas's bottom-left corner, and pixels per mm. */
  let view = $state({ x: 0, y: 0, scale: 1 });
  let fitted = false;
  let width = $state(0);
  let height = $state(0);
  let canvas = $state<HTMLCanvasElement>();

  const count = $derived.by(() => {
    void version;
    return model.length;
  });
  const span = $derived.by(() => {
    void version;
    return model.span();
  });
  const time = $derived(live ? span[1] : cursor);
  const at = $derived.by(() => {
    void version;
    return model.at(live ? Number.POSITIVE_INFINITY : cursor);
  });
  const highlighted = $derived.by(() => {
    void version;
    return selection.line === null ? 0 : (model.byLine.get(selection.line)?.length ?? 0);
  });
  /** Laser and air at the cursor: the machine's own when live. */
  const laser = $derived(
    live ? machine.laserPower : at?.running && !at.segment.stationary ? at.segment.power : 0,
  );
  const air = $derived(live ? machine.airAssist : (at?.segment.airAssist ?? false));
  const options = $derived<DrawOptions>({ showTravel, colouring });

  // ---- coordinates ----

  const toScreen = (x: number, y: number): [number, number] => [
    (x - view.x) * view.scale,
    height - (y - view.y) * view.scale,
  ];
  const toBed = (px: number, py: number): [number, number] => [
    view.x + px / view.scale,
    view.y + (height - py) / view.scale,
  ];

  function fit() {
    const box = model.bounds(machine.bed);
    if (!box || !width || !height) return;
    const pad = 24;
    const w = Math.max(1, box.max[0] - box.min[0]);
    const h = Math.max(1, box.max[1] - box.min[1]);
    const scale = Math.min((width - 2 * pad) / w, (height - 2 * pad) / h);
    view = {
      scale,
      x: box.min[0] - (width / scale - w) / 2,
      y: box.min[1] - (height / scale - h) / 2,
    };
    fitted = true;
  }

  // ---- painting ----

  let offscreen: HTMLCanvasElement | undefined;
  let bakedKey = "";
  let baked = 0;
  let frame = 0;

  function paint(ctx: CanvasRenderingContext2D, style: Style, segments: Iterable<BurnSegment>) {
    ctx.globalAlpha = style.alpha;
    ctx.strokeStyle = style.colour;
    ctx.fillStyle = style.colour;
    ctx.lineWidth = style.width;
    ctx.setLineDash(style.dash as number[]);
    ctx.beginPath();
    for (const segment of segments) {
      const [x0, y0] = toScreen(...segment.start);
      if (style.kind === "dwell") {
        // Longer dwells burn a bigger spot.
        const radius = 2.5 + Math.min(3, (segment.t1 - segment.t0) / 1000);
        ctx.moveTo(x0 + radius, y0);
        ctx.arc(x0, y0, radius, 0, 2 * Math.PI);
      } else {
        const [x1, y1] = toScreen(...segment.end);
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1, y1);
      }
    }
    if (style.kind === "dwell") ctx.fill();
    else ctx.stroke();
    ctx.globalAlpha = 1;
    ctx.setLineDash([]);
  }

  function paintLayers(ctx: CanvasRenderingContext2D, layers: Layer[]) {
    for (const layer of layers) {
      paint(
        ctx,
        layer.style,
        layer.indexes.map((index) => model.segments[index]),
      );
    }
  }

  function paintBed(ctx: CanvasRenderingContext2D) {
    ctx.fillStyle = OUTSIDE;
    ctx.fillRect(0, 0, width, height);
    const { origin, size } = machine.bed;
    const [left, top] = toScreen(origin[0], origin[1] + size[1]);
    const [right, bottom] = toScreen(origin[0] + size[0], origin[1]);
    ctx.fillStyle = MATERIAL;
    ctx.fillRect(left, top, right - left, bottom - top);
    // Fine lines at least 8 px apart, bold every fifth (or tenth) of those.
    const step = gridStep(view.scale, 8);
    const major = gridStep(view.scale, 60);
    const [minX, minY] = toBed(0, height);
    const [maxX, maxY] = toBed(width, 0);
    const lines = (from: number, to: number, every: number) =>
      Array.from(
        { length: Math.max(0, Math.floor(to / every) - Math.ceil(from / every) + 1) },
        (_, n) => (Math.ceil(from / every) + n) * every,
      );
    for (const [every, alpha] of [
      [step, 0.08],
      [major, 0.2],
    ] as const) {
      ctx.strokeStyle = `rgba(0, 0, 0, ${alpha})`;
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (const x of lines(
        Math.max(minX, origin[0]),
        Math.min(maxX, origin[0] + size[0]),
        every,
      )) {
        const [px] = toScreen(x, 0);
        ctx.moveTo(Math.round(px) + 0.5, Math.max(0, top));
        ctx.lineTo(Math.round(px) + 0.5, Math.min(height, bottom));
      }
      for (const y of lines(
        Math.max(minY, origin[1]),
        Math.min(maxY, origin[1] + size[1]),
        every,
      )) {
        const [, py] = toScreen(0, y);
        ctx.moveTo(Math.max(0, left), Math.round(py) + 0.5);
        ctx.lineTo(Math.min(width, right), Math.round(py) + 0.5);
      }
      ctx.stroke();
    }
    ctx.strokeStyle = "rgba(0, 0, 0, 0.45)";
    ctx.strokeRect(left + 0.5, top + 0.5, right - left - 1, bottom - top - 1);
    ctx.fillStyle = "rgba(0, 0, 0, 0.55)";
    ctx.font = "11px ui-sans-serif, system-ui, sans-serif";
    ctx.fillText(`grid ${step} mm · bold ${major} mm`, 8, height - 8);
  }

  function paintHead(ctx: CanvasRenderingContext2D) {
    const position = live ? machine.machinePosition : (at?.position ?? [0, 0]);
    const [x, y] = toScreen(position[0], position[1]);
    if (laser > 0) {
      ctx.fillStyle = `rgba(239, 68, 68, ${0.25 + 0.5 * laser})`;
      ctx.beginPath();
      ctx.arc(x, y, 9, 0, 2 * Math.PI);
      ctx.fill();
    }
    if (air) {
      ctx.strokeStyle = "rgba(14, 165, 233, 0.9)";
      ctx.lineWidth = 1.5;
      ctx.setLineDash([3, 3]);
      ctx.beginPath();
      ctx.arc(x, y, 13, 0, 2 * Math.PI);
      ctx.stroke();
      ctx.setLineDash([]);
    }
    ctx.strokeStyle = laser > 0 ? "#dc2626" : "#18181b";
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.moveTo(x - 7, y);
    ctx.lineTo(x + 7, y);
    ctx.moveTo(x, y - 7);
    ctx.lineTo(x, y + 7);
    ctx.stroke();
  }

  function render() {
    frame = 0;
    if (!canvas || !width || !height) return;
    if (!fitted) fit();
    const ratio = window.devicePixelRatio || 1;
    const pixels = [Math.round(width * ratio), Math.round(height * ratio)];
    for (const target of [canvas, (offscreen ??= document.createElement("canvas"))]) {
      if (target.width !== pixels[0] || target.height !== pixels[1]) {
        [target.width, target.height] = pixels;
      }
    }
    const ctx = canvas.getContext("2d");
    const layer = offscreen.getContext("2d");
    if (!ctx || !layer) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    layer.setTransform(ratio, 0, 0, ratio, 0, 0);

    // Finished segments are baked into the offscreen layer and only the new
    // ones drawn each frame; scrubbing back or changing the view rebakes.
    const done = live ? model.length : model.finishedBy(cursor);
    const key = [view.x, view.y, view.scale, ...pixels, showTravel, colouring, generation].join();
    if (key !== bakedKey || done < baked) {
      layer.clearRect(0, 0, width, height);
      bakedKey = key;
      baked = 0;
    }
    paintLayers(layer, model.layers(baked, done, options));
    baked = done;

    paintBed(ctx);
    ctx.drawImage(offscreen, 0, 0, width, height);
    if (!live && at?.running) {
      const style = styleOf(at.segment, model.passes[at.index], options);
      if (style) paint(ctx, style, [partial(at.segment, cursor)]);
    }
    if (selection.line !== null) {
      const indexes = model.byLine.get(selection.line) ?? [];
      const shown = indexes.filter((index) => index < done).map((index) => model.segments[index]);
      if (!live && at?.running && at.segment.line === selection.line) {
        shown.push(partial(at.segment, cursor));
      }
      const glow = { colour: HIGHLIGHT, alpha: 0.9, width: 3.5, dash: [] } as const;
      paint(
        ctx,
        { ...glow, kind: "burn" },
        shown.filter((segment) => !segment.stationary),
      );
      paint(
        ctx,
        { ...glow, kind: "dwell" },
        shown.filter((segment) => segment.stationary),
      );
    }
    paintHead(ctx);
  }

  function schedule() {
    frame ||= requestAnimationFrame(render);
  }

  $effect(() => {
    void [version, live, cursor, view, width, height, options, selection.line];
    void [machine.machinePosition, machine.laserPower, machine.airAssist, machine.bed];
    schedule();
  });

  // ---- trace ----

  function reset(next: number) {
    model = new BurnModel();
    generation = next;
    since = 0;
    live = true;
    playing = false;
    version++;
  }

  let polling = false;

  async function poll() {
    if (polling) return;
    polling = true;
    try {
      // A big job arrives in several batches; take them all now.
      for (let round = 0; round < 50; round++) {
        const batch = await sim.burns(since);
        if (batch.generation !== generation) {
          const asked = since;
          reset(batch.generation);
          if (asked !== 0) continue;
        }
        if (batch.segments.length) {
          model.append(batch.segments);
          version++;
        }
        since = batch.next;
        truncated = batch.truncated;
        if (batch.next >= batch.total) break;
      }
    } catch (cause) {
      notice = `Burn view: ${cause instanceof Error ? cause.message : String(cause)}`;
    } finally {
      polling = false;
    }
  }

  async function clearBed() {
    try {
      await sim.clearBurns();
      await poll();
      notice = "";
    } catch (cause) {
      notice = cause instanceof Error ? cause.message : String(cause);
    }
  }

  async function exportPng() {
    render();
    const blob = await new Promise<Blob | null>((resolve) => canvas?.toBlob(resolve, "image/png"));
    if (!blob) {
      notice = "Couldn't capture the burn view.";
      return;
    }
    try {
      notice = `Saved ${await sim.exportPng(new Uint8Array(await blob.arrayBuffer()))}`;
    } catch (cause) {
      notice = cause instanceof Error ? cause.message : String(cause);
    }
  }

  // ---- playback ----

  let lastTick = 0;

  function tick(now: number) {
    if (!playing) return;
    cursor = Math.min(span[1], cursor + (now - lastTick) * rate);
    lastTick = now;
    if (cursor >= span[1]) playing = false;
    else requestAnimationFrame(tick);
  }

  function play() {
    if (playing) {
      playing = false;
      return;
    }
    if (live || cursor >= span[1]) cursor = span[0];
    live = false;
    playing = true;
    lastTick = performance.now();
    requestAnimationFrame(tick);
  }

  function goLive() {
    live = true;
    playing = false;
  }

  function scrub(value: number) {
    live = false;
    playing = false;
    cursor = value;
  }

  // ---- pointer ----

  let drag: { x: number; y: number; moved: boolean } | null = null;

  function onpointerdown(event: PointerEvent) {
    canvas?.setPointerCapture(event.pointerId);
    drag = { x: event.offsetX, y: event.offsetY, moved: false };
  }

  function onpointermove(event: PointerEvent) {
    if (!drag) return;
    const dx = event.offsetX - drag.x;
    const dy = event.offsetY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 3) return;
    drag = { x: event.offsetX, y: event.offsetY, moved: true };
    view = { ...view, x: view.x - dx / view.scale, y: view.y + dy / view.scale };
  }

  function onpointerup(event: PointerEvent) {
    if (drag && !drag.moved) {
      // A click picks the burn under it and highlights its line.
      const done = live ? model.length : model.finishedBy(cursor);
      const index = model.nearest(
        toBed(event.offsetX, event.offsetY),
        PICK_RADIUS / view.scale,
        done,
      );
      selection.line = index === null ? null : model.segments[index].line;
    }
    drag = null;
  }

  function onwheel(event: WheelEvent) {
    event.preventDefault();
    const scale = Math.min(500, Math.max(0.05, view.scale * Math.exp(-event.deltaY * 0.0015)));
    const [x, y] = toBed(event.offsetX, event.offsetY);
    view = {
      scale,
      x: x - event.offsetX / scale,
      y: y - (height - event.offsetY) / scale,
    };
  }

  onMount(() => {
    void poll();
    if (!sim.desktop) return;
    const timer = setInterval(() => void poll(), 100);
    return () => {
      clearInterval(timer);
      playing = false;
      if (frame) cancelAnimationFrame(frame);
    };
  });
</script>

<section class="flex h-[55%] min-h-72 flex-col border-b" aria-labelledby="burn-heading">
  <div class="flex flex-wrap items-center gap-2 border-b px-4 py-2">
    <h2 id="burn-heading" class="mr-2 text-sm font-semibold">Burn view</h2>
    <Button
      size="xs"
      variant={showTravel ? "secondary" : "ghost"}
      aria-pressed={showTravel}
      onclick={() => (showTravel = !showTravel)}>Travel</Button
    >
    <div class="flex gap-1" role="group" aria-label="Colour burns by">
      {#each colourings as option (option.id)}
        <Button
          size="xs"
          variant={colouring === option.id ? "secondary" : "ghost"}
          aria-pressed={colouring === option.id}
          onclick={() => (colouring = option.id)}>{option.label}</Button
        >
      {/each}
    </div>
    {#if selection.line !== null}
      <Badge variant="outline" class="gap-1 border-amber-500 font-mono">
        Line {selection.line}
        <span class="font-sans text-muted-foreground">
          {highlighted ? `${highlighted} segments` : "no burn"}
        </span>
        <button
          type="button"
          aria-label="Stop highlighting line {selection.line}"
          class="rounded-sm hover:bg-muted"
          onclick={() => (selection.line = null)}><X class="size-3" /></button
        >
      </Badge>
    {/if}
    <div class="ml-auto flex items-center gap-2">
      <Badge variant={laser > 0 ? "destructive" : "outline"}>
        <Zap />{laser > 0 ? `Laser ${Math.round(laser * 100)}%` : "Laser off"}
      </Badge>
      <Badge variant={air ? "secondary" : "outline"} class={[!air && "text-muted-foreground"]}>
        <Wind />{air ? "Air on" : "Air off"}
      </Badge>
      <Button size="sm" variant="ghost" onclick={fit}><Maximize />Fit</Button>
      <Button size="sm" variant="ghost" onclick={clearBed} disabled={!sim.desktop}
        ><Eraser />Clear bed</Button
      >
      <Button size="sm" variant="outline" onclick={exportPng} disabled={!sim.desktop}
        ><ImageDown />PNG</Button
      >
    </div>
  </div>

  <div class="relative min-h-0 flex-1" bind:clientWidth={width} bind:clientHeight={height}>
    <canvas
      bind:this={canvas}
      class="absolute inset-0 size-full cursor-crosshair touch-none"
      style:width="{width}px"
      style:height="{height}px"
      aria-label="Bed with {count} traced segments{selection.line === null
        ? ''
        : `, line ${selection.line} highlighted`}"
      {onpointerdown}
      {onpointermove}
      {onpointerup}
      {onwheel}
      ondblclick={fit}
    ></canvas>
    {#if !sim.desktop}
      <p
        class="absolute top-2 left-2 rounded bg-background/80 px-2 py-1 text-sm text-muted-foreground"
      >
        No emulator in the browser preview.
      </p>
    {/if}
  </div>

  <div class="flex flex-wrap items-center gap-3 border-t px-4 py-2 text-xs">
    <Button size="icon-sm" variant="outline" aria-label={playing ? "Pause" : "Play"} onclick={play}>
      {#if playing}<Pause />{:else}<Play />{/if}
    </Button>
    <input
      type="range"
      aria-label="Playback position"
      class="min-w-40 flex-1 accent-amber-600"
      min={span[0]}
      max={span[1]}
      step="any"
      value={time}
      disabled={!count}
      oninput={(event) => scrub(Number(event.currentTarget.value))}
    />
    <span class="font-mono tabular-nums">{clock(time - span[0])} / {clock(span[1] - span[0])}</span>
    <div class="flex gap-1" role="group" aria-label="Playback speed">
      {#each rates as option (option)}
        <Button
          size="xs"
          variant={rate === option ? "secondary" : "ghost"}
          aria-pressed={rate === option}
          onclick={() => (rate = option)}>{option}x</Button
        >
      {/each}
    </div>
    <Button size="xs" variant={live ? "secondary" : "ghost"} aria-pressed={live} onclick={goLive}
      ><Radio />Live</Button
    >
  </div>
  <div
    class="flex min-h-7 flex-wrap items-center gap-x-3 px-4 pb-2 text-xs text-muted-foreground"
    role="status"
  >
    {#if at}
      <button
        type="button"
        class="font-mono text-foreground underline-offset-2 hover:underline"
        title="Highlight this line"
        onclick={() => (selection.line = at.segment.line)}>Line {at.segment.line}</button
      >
      <span class="font-mono">{modal(at.segment).join(" · ")}</span>
    {:else}
      <span>Nothing burned yet. Run a job from a connected host.</span>
    {/if}
    {#if truncated}<span class="text-destructive">Trace full: later motion isn't shown.</span>{/if}
    {#if notice}<span>{notice}</span>{/if}
  </div>
</section>
