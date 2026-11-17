<script lang="ts">
  import EyeIcon from "@lucide/svelte/icons/eye";
  import EyeOffIcon from "@lucide/svelte/icons/eye-off";
  import GripVerticalIcon from "@lucide/svelte/icons/grip-vertical";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import ZapIcon from "@lucide/svelte/icons/zap";
  import ZapOffIcon from "@lucide/svelte/icons/zap-off";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import * as ToggleGroup from "#lib/components/ui/toggle-group/index.js";
  import { moveLayer, removeLayer } from "#lib/document/commands.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import {
    intervalToLpi,
    LAYER_PALETTE,
    layerUsage,
    lpiToInterval,
    type LayerChanges,
  } from "#lib/document/layers.js";
  import type { CutLayer, LayerMode } from "#lib/document/model.js";
  import { drawingLayer } from "#lib/document/drawing-layer.svelte.js";
  import { selection } from "#lib/document/selection.svelte.js";
  import { clickSwatch, editLayerSetting } from "./layers-panel.js";
  import NumberField from "./NumberField.svelte";

  /**
   * Cut layers: the colour palette, the layers in cut order (drag to
   * reorder) and the settings of the layer being edited, which is also the
   * layer new shapes are drawn on. Every change is an undoable document
   * command.
   */
  let problem = $state("");
  let dragging = $state<string | null>(null);
  let dropIndex = $state<number | null>(null);

  const layers = $derived(editor.doc.layers);
  const active = $derived(drawingLayer.current(editor.doc));
  const usage = $derived(layerUsage(editor.doc.objects));
  const usedColors = $derived(new Set(layers.map((layer) => layer.color)));
  const selectedLayers = $derived(
    new Set(
      editor.doc.objects
        .filter((object) => selection.has(object.id))
        .map((object) => object.layerId),
    ),
  );
  const hasSelection = $derived(selection.ids.length > 0);

  function swatch(slot: number) {
    const { command, layerId } = clickSwatch(editor.doc, selection.ids, slot);
    if (command && !editor.execute(command)) return;
    drawingLayer.id = layerId;
    problem = "";
  }

  function edit(layer: CutLayer, changes: LayerChanges, coalesce = true): boolean {
    const refused = editLayerSetting(editor, layer.id, changes, coalesce);
    problem = refused ?? "";
    return refused === null;
  }

  function move(layer: CutLayer, index: number) {
    const from = layers.indexOf(layer);
    if (index < 0 || index >= layers.length || index === from) return;
    editor.execute(moveLayer(layer.id, index));
  }

  function remove(layer: CutLayer) {
    if (editor.execute(removeLayer(layer.id)) && drawingLayer.id === layer.id)
      drawingLayer.id = null;
  }

  function onrowkeydown(event: KeyboardEvent, layer: CutLayer, index: number) {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    move(layer, index + (event.key === "ArrowUp" ? -1 : 1));
  }

  function ondragover(event: DragEvent, index: number) {
    if (dragging === null) return;
    event.preventDefault();
    const row = event.currentTarget as HTMLElement;
    const { top, height } = row.getBoundingClientRect();
    dropIndex = event.clientY < top + height / 2 ? index : index + 1;
  }

  function ondrop(event: DragEvent) {
    event.preventDefault();
    const layer = layers.find((other) => other.id === dragging);
    if (layer && dropIndex !== null) {
      const from = layers.indexOf(layer);
      // Dropping below itself shifts the target up by the row it leaves.
      move(layer, dropIndex > from ? dropIndex - 1 : dropIndex);
    }
    dragging = null;
    dropIndex = null;
  }

  function summary(layer: CutLayer): string {
    const mode = layer.mode === "fill" ? "Fill" : "Line";
    const passes = layer.passes > 1 ? ` · ×${layer.passes}` : "";
    return `${mode} · ${layer.speed} mm/min · ${layer.maxPower}%${passes}`;
  }

  const numberFields: {
    key: "speed" | "maxPower" | "minPower" | "passes";
    label: string;
    unit: string;
    digits: number;
  }[] = [
    { key: "speed", label: "Speed", unit: "mm/min", digits: 0 },
    { key: "maxPower", label: "Max power", unit: "%", digits: 1 },
    { key: "minPower", label: "Min power", unit: "%", digits: 1 },
    { key: "passes", label: "Passes", unit: "", digits: 0 },
  ];

  const fillFields: {
    key: "lineInterval" | "scanAngle" | "overscan" | "fillGap";
    label: string;
    unit: string;
    digits: number;
  }[] = [
    { key: "lineInterval", label: "Interval", unit: "mm", digits: 3 },
    { key: "scanAngle", label: "Scan angle", unit: "°", digits: 1 },
    { key: "overscan", label: "Overscan", unit: "mm", digits: 2 },
    { key: "fillGap", label: "Cross gaps under", unit: "mm", digits: 2 },
  ];
</script>

<div class="flex flex-col gap-4">
  <section aria-labelledby="layer-palette" class="flex flex-col gap-1.5">
    <h3 id="layer-palette" class="text-xs font-medium text-muted-foreground">
      {hasSelection ? "Put the selection on a layer" : "Add a layer"}
    </h3>
    <div class="grid grid-cols-10 gap-1">
      {#each LAYER_PALETTE as color, slot (color)}
        {@const label = `Layer ${slot + 1}${usedColors.has(color) ? "" : " (new)"}`}
        <button
          type="button"
          aria-label={label}
          title={label}
          class={[
            "aspect-square rounded-sm border border-black/20 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none dark:border-white/20",
            usedColors.has(color)
              ? "ring-1 ring-foreground/60 ring-offset-1 ring-offset-background"
              : "opacity-70 hover:opacity-100",
          ]}
          style:background-color={color}
          onclick={() => swatch(slot)}
        ></button>
      {/each}
    </div>
  </section>

  {#if layers.length === 0}
    <p class="text-sm text-muted-foreground">
      No layers yet. Click a colour to add one; imported artwork adds its own.
    </p>
  {:else}
    <section aria-labelledby="layer-list" class="flex flex-col gap-1.5">
      <h3 id="layer-list" class="text-xs font-medium text-muted-foreground">
        Cut order <span class="font-normal">(drag, or Alt+↑/↓, to reorder)</span>
      </h3>
      <ol class="flex flex-col" ondragleave={() => (dropIndex = null)}>
        {#each layers as layer, index (layer.id)}
          <li
            class={[
              "flex items-center gap-1 rounded-md border-y-2 border-transparent pr-1",
              layer.id === active?.id ? "bg-accent" : "hover:bg-muted/60",
              dropIndex === index && "border-t-primary",
              dropIndex === index + 1 && index === layers.length - 1 && "border-b-primary",
              dragging === layer.id && "opacity-50",
            ]}
            draggable="true"
            ondragstart={(event) => {
              dragging = layer.id;
              event.dataTransfer?.setData("text/plain", layer.id);
            }}
            ondragend={() => {
              dragging = null;
              dropIndex = null;
            }}
            ondragover={(event) => ondragover(event, index)}
            {ondrop}
          >
            <GripVerticalIcon class="size-3.5 shrink-0 cursor-grab text-muted-foreground" />
            <button
              type="button"
              class="flex min-w-0 flex-1 items-center gap-2 rounded-sm py-1 text-left focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              aria-pressed={layer.id === active?.id}
              onclick={() => (drawingLayer.id = layer.id)}
              onkeydown={(event) => onrowkeydown(event, layer, index)}
            >
              <span
                class="size-3.5 shrink-0 rounded-sm border border-black/20 dark:border-white/20"
                style:background-color={layer.color}
              ></span>
              <span class={["min-w-0 flex-1", !layer.shown && "text-muted-foreground"]}>
                <span class="flex items-center gap-1 truncate text-sm">
                  {layer.name}
                  {#if selectedLayers.has(layer.id)}
                    <span
                      class="size-1.5 rounded-full bg-primary"
                      title="Selection is on this layer"
                    ></span>
                  {/if}
                </span>
                <span class="block truncate text-xs text-muted-foreground">
                  {summary(layer)} · {usage.get(layer.id) ?? 0} objects
                </span>
              </span>
            </button>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={`Cut ${layer.name}`}
              title={layer.output
                ? "Output on: this layer is cut"
                : "Output off: this layer is skipped"}
              aria-pressed={layer.output}
              onclick={() => edit(layer, { output: !layer.output }, false)}
            >
              {#if layer.output}<ZapIcon />{:else}<ZapOffIcon class="text-muted-foreground" />{/if}
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={`Show ${layer.name}`}
              title={layer.shown ? "Shown on the canvas" : "Hidden on the canvas"}
              aria-pressed={layer.shown}
              onclick={() => edit(layer, { shown: !layer.shown }, false)}
            >
              {#if layer.shown}<EyeIcon />{:else}<EyeOffIcon class="text-muted-foreground" />{/if}
            </Button>
          </li>
        {/each}
      </ol>
    </section>

    {#if active}
      {@render settings(active)}
    {/if}
  {/if}
</div>

{#snippet settings(layer: CutLayer)}
  <section aria-label={`${layer.name} settings`} class="flex flex-col gap-2 border-t pt-3">
    <div class="flex items-center gap-2">
      <span
        class="size-4 shrink-0 rounded-sm border border-black/20 dark:border-white/20"
        style:background-color={layer.color}
      ></span>
      <Input
        class="h-7 px-2 md:text-sm"
        aria-label="Layer name"
        value={layer.name}
        onchange={(event) => {
          const name = event.currentTarget.value.trim();
          if (name) edit(layer, { name });
          else event.currentTarget.value = layer.name;
        }}
      />
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={`Delete ${layer.name}`}
        title={usage.has(layer.id)
          ? "Move this layer's objects to another layer before deleting it"
          : "Delete layer"}
        disabled={usage.has(layer.id)}
        onclick={() => remove(layer)}
      >
        <Trash2Icon />
      </Button>
    </div>

    <ToggleGroup.Root
      type="single"
      variant="outline"
      size="sm"
      class="w-full"
      aria-label="Mode"
      bind:value={
        () => layer.mode,
        (mode) => mode && mode !== layer.mode && edit(layer, { mode: mode as LayerMode }, false)
      }
    >
      <ToggleGroup.Item value="line" class="flex-1">Line</ToggleGroup.Item>
      <ToggleGroup.Item value="fill" class="flex-1">Fill</ToggleGroup.Item>
    </ToggleGroup.Root>

    <div class="flex flex-col gap-1.5">
      {#each numberFields as field (field.key)}
        <NumberField
          wide
          label={field.label}
          unit={field.unit}
          digits={field.digits}
          value={layer[field.key]}
          oncommit={(value) => edit(layer, { [field.key]: value })}
        />
      {/each}
    </div>

    {@render toggle(layer, "airAssist", "Air assist")}
    {@render toggle(layer, "constantPower", "Constant power (M3)")}

    {#if layer.mode === "fill"}
      <div class="flex flex-col gap-1.5">
        {#each fillFields as field (field.key)}
          <NumberField
            wide
            label={field.label}
            unit={field.unit}
            digits={field.digits}
            value={layer[field.key]}
            oncommit={(value) => edit(layer, { [field.key]: value })}
          />
          {#if field.key === "lineInterval"}
            <NumberField
              wide
              label="Lines/inch"
              unit="LPI"
              digits={1}
              value={intervalToLpi(layer.lineInterval)}
              oncommit={(lpi) => lpi > 0 && edit(layer, { lineInterval: lpiToInterval(lpi) })}
            />
          {/if}
        {/each}
      </div>
      {@render toggle(layer, "bidirectional", "Bidirectional")}
    {/if}

    {#if problem}
      <p role="alert" class="text-xs text-destructive">{problem}</p>
    {/if}
  </section>
{/snippet}

{#snippet toggle(
  layer: CutLayer,
  key: "airAssist" | "bidirectional" | "constantPower",
  label: string,
)}
  <label class="flex items-center gap-2 text-xs">
    <input
      type="checkbox"
      class="size-3.5 accent-primary"
      checked={layer[key]}
      onchange={(event) => {
        const box = event.currentTarget;
        if (!edit(layer, { [key]: box.checked }, false)) box.checked = layer[key];
      }}
    />
    {label}
  </label>
{/snippet}
