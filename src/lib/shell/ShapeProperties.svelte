<script lang="ts">
  import { Button } from "#lib/components/ui/button/index.js";
  import { convertToPath, DocumentError, editShape } from "#lib/document/commands.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import type { ShapeChanges, ShapeObject } from "#lib/document/shapes.js";
  import NumberField from "./NumberField.svelte";

  /**
   * A rectangle's, ellipse's or polygon's own size and radii, which
   * stay editable until it is converted to a path. Each edit is one undo
   * step.
   */
  let { shape }: { shape: ShapeObject } = $props();

  let problem = $state("");

  type Field = { key: keyof ShapeChanges; label: string; unit: string; digits?: number };
  const fields = $derived.by((): Field[] => {
    switch (shape.type) {
      case "rect":
        return [
          { key: "width", label: "Width", unit: "mm" },
          { key: "height", label: "Height", unit: "mm" },
          { key: "cornerRadius", label: "Corner radius", unit: "mm" },
        ];
      case "ellipse":
        return [
          { key: "rx", label: "Radius X", unit: "mm" },
          { key: "ry", label: "Radius Y", unit: "mm" },
        ];
      case "polygon":
        return [
          { key: "sides", label: "Sides", unit: "", digits: 0 },
          { key: "radius", label: "Radius", unit: "mm" },
          ...(shape.innerRadius === null
            ? []
            : [{ key: "innerRadius" as const, label: "Inner radius", unit: "mm" }]),
        ];
    }
  });

  function value(key: keyof ShapeChanges): number {
    return ((shape as unknown as Record<string, number | null>)[key] ?? 0) as number;
  }

  function edit(changes: ShapeChanges): boolean {
    try {
      const done = editor.execute(editShape(shape.id, changes));
      problem = "";
      return done;
    } catch (error) {
      if (!(error instanceof DocumentError)) throw error;
      problem = error.message;
      return false;
    }
  }
</script>

<section aria-label="Shape" class="flex flex-col gap-1.5 border-t pt-3">
  {#each fields as field (field.key)}
    <NumberField
      wide
      label={field.label}
      unit={field.unit}
      digits={field.digits}
      value={value(field.key)}
      oncommit={(next) => edit({ [field.key]: next })}
    />
  {/each}
  {#if shape.type === "polygon"}
    <label class="flex items-center gap-2 text-xs">
      <input
        type="checkbox"
        class="size-3.5 accent-primary"
        checked={shape.innerRadius !== null}
        onchange={(event) => {
          const box = event.currentTarget;
          const innerRadius = box.checked ? shape.radius / 2 : null;
          if (!edit({ innerRadius })) box.checked = shape.innerRadius !== null;
        }}
      />
      Star
    </label>
  {/if}
  {#if problem}
    <p role="alert" class="text-xs text-destructive">{problem}</p>
  {/if}
  <Button
    variant="outline"
    size="sm"
    class="self-start"
    onclick={() => editor.execute(convertToPath(editor.doc, [shape.id]))}
  >
    Convert to path
  </Button>
</section>
