<script lang="ts">
  import {
    CENTRE,
    propertyTransform,
    readProperties,
    selectionBounds,
    transformObjects,
    type Anchor,
    type Properties,
  } from "#lib/canvas/arrange.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import { selection } from "#lib/document/selection.svelte.js";
  import { isShape } from "#lib/document/shapes.js";
  import NumberField from "./NumberField.svelte";
  import ShapeProperties from "./ShapeProperties.svelte";
  import TextProperties from "./TextProperties.svelte";

  /**
   * Position, size and rotation of the selection, measured from a chosen
   * anchor point. Edits use the same maths as the canvas handles, and
   * each one is a single undo step.
   */
  let anchor = $state<Anchor>(CENTRE);

  const ids = $derived(selection.ids);
  const bounds = $derived(selectionBounds(editor.doc, ids));
  const properties = $derived(bounds && readProperties(editor.doc, ids, bounds, anchor));
  const single = $derived(
    ids.length === 1 ? editor.doc.objects.find((object) => object.id === ids[0]) : undefined,
  );
  const shape = $derived(single && isShape(single) ? single : null);
  const text = $derived(single?.type === "text" ? single : null);
  const title = $derived(ids.length === 1 ? (single?.name ?? "1 object") : `${ids.length} objects`);

  const fields: { key: keyof Properties; label: string; unit: string; digits?: number }[] = [
    { key: "x", label: "X", unit: "mm" },
    { key: "y", label: "Y", unit: "mm" },
    { key: "width", label: "W", unit: "mm" },
    { key: "height", label: "H", unit: "mm" },
    { key: "rotation", label: "Rot", unit: "°", digits: 2 },
  ];

  // Top row first: Y is up, so the top row is y = 1.
  const anchors: { anchor: Anchor; label: string }[] = [1, 0.5, 0].flatMap((y) =>
    [0, 0.5, 1].map((x) => ({
      anchor: { x, y } as Anchor,
      label: `${y === 1 ? "Top" : y === 0 ? "Bottom" : "Middle"} ${x === 0 ? "left" : x === 1 ? "right" : "centre"}`,
    })),
  );

  function commit(key: keyof Properties, value: number): boolean {
    if (!bounds || !properties) return false;
    if (Math.abs(value - properties[key]) < 1e-9) return true;
    const by = propertyTransform(properties, bounds, anchor, key, value);
    if (!by) return false;
    return editor.execute(transformObjects(editor.doc, ids, by));
  }
</script>

{#if properties}
  <div class="flex flex-col gap-3">
    <p class="truncate text-sm font-medium" {title}>{title}</p>
    <div class="flex gap-3">
      <div
        role="radiogroup"
        aria-label="Anchor point"
        class="grid size-fit shrink-0 grid-cols-3 gap-0.5 rounded-md border p-1"
      >
        {#each anchors as option (option.label)}
          {@const checked = option.anchor.x === anchor.x && option.anchor.y === anchor.y}
          <button
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={option.label}
            title={`Anchor: ${option.label.toLowerCase()}`}
            class="grid size-4 place-items-center rounded-sm hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            onclick={() => (anchor = option.anchor)}
          >
            <span
              class={[
                "size-1.5 rounded-full",
                checked ? "bg-primary ring-2 ring-primary/30" : "bg-muted-foreground/50",
              ]}
            ></span>
          </button>
        {/each}
      </div>
      <div class="flex min-w-0 flex-1 flex-col gap-1.5">
        {#each fields as field (field.key)}
          <NumberField
            label={field.label}
            unit={field.unit}
            digits={field.digits}
            value={properties[field.key]}
            oncommit={(value) => commit(field.key, value)}
          />
        {/each}
      </div>
    </div>
    {#if shape}
      {#key shape.id}
        <ShapeProperties {shape} />
      {/key}
    {/if}
    {#if text}
      {#key text.id}
        <TextProperties {text} />
      {/key}
    {/if}
    <p class="text-xs text-muted-foreground">Fields accept arithmetic, such as 120/2.</p>
  </div>
{:else}
  <p class="text-sm text-muted-foreground">Select a shape to see its properties.</p>
{/if}
