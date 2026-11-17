<script lang="ts">
  import AlignCenter from "@lucide/svelte/icons/align-center";
  import AlignLeft from "@lucide/svelte/icons/align-left";
  import AlignRight from "@lucide/svelte/icons/align-right";
  import { onMount } from "svelte";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as ToggleGroup from "#lib/components/ui/toggle-group/index.js";
  import { convertToPath } from "#lib/document/commands.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import type { TextAlign, TextObject } from "#lib/document/model.js";
  import type { FontStyle, TextStyle } from "#lib/text/adapter.js";
  import { fontList, styleName } from "#lib/text/fonts.svelte.js";
  import { textEditing } from "#lib/text/text-editing.svelte.js";
  import FontPicker from "./FontPicker.svelte";
  import NumberField from "./NumberField.svelte";

  /**
   * A text object's font and spacing. Each change lays the text out
   * again and is one undo step. The string itself is typed on the canvas.
   */
  let { text }: { text: TextObject } = $props();

  onMount(() => void fontList.load());

  const family = $derived(fontList.find(text.fontFamily));
  const missing = $derived(fontList.loaded && !family);
  const styles = $derived<FontStyle[]>(
    family?.styles ?? [{ weight: text.fontWeight, italic: text.italic }],
  );
  const styleKey = (style: FontStyle) => `${style.weight}${style.italic ? "i" : ""}`;

  function restyle(changes: Partial<TextStyle>): boolean {
    void textEditing.restyle(text.id, changes);
    return true;
  }

  /** A new family keeps the weight and slant if it has them, else its closest style. */
  function pickFamily(name: string) {
    const options = fontList.find(name)?.styles ?? [];
    const closest = options.toSorted(
      (a, b) =>
        Number(a.italic !== text.italic) - Number(b.italic !== text.italic) ||
        Math.abs(a.weight - text.fontWeight) - Math.abs(b.weight - text.fontWeight),
    )[0];
    restyle({
      fontFamily: name,
      ...(closest ? { fontWeight: closest.weight, italic: closest.italic } : {}),
    });
  }

  const aligns: { value: TextAlign; label: string; icon: typeof AlignLeft }[] = [
    { value: "left", label: "Align left", icon: AlignLeft },
    { value: "center", label: "Align centre", icon: AlignCenter },
    { value: "right", label: "Align right", icon: AlignRight },
  ];
</script>

<section aria-label="Text" class="flex flex-col gap-1.5 border-t pt-3">
  <div class="flex items-center gap-1.5 text-xs">
    <span class="w-20 shrink-0 text-muted-foreground">Font</span>
    <FontPicker value={text.fontFamily} onpick={pickFamily} />
  </div>
  <label class="flex items-center gap-1.5 text-xs">
    <span class="w-20 shrink-0 text-muted-foreground">Style</span>
    <select
      class="h-7 min-w-0 flex-1 rounded-md border border-input bg-transparent px-2 text-xs"
      value={styleKey({ weight: text.fontWeight, italic: text.italic })}
      onchange={(event) => {
        const style = styles.find((option) => styleKey(option) === event.currentTarget.value);
        if (style) restyle({ fontWeight: style.weight, italic: style.italic });
      }}
    >
      {#each styles as style (styleKey(style))}
        <option value={styleKey(style)}>{styleName(style.weight, style.italic)}</option>
      {/each}
    </select>
  </label>
  {#if missing}
    <p class="text-xs text-amber-600 dark:text-amber-400">
      {text.fontFamily} isn’t installed here. The text keeps its shape until you edit it, then a stand-in
      font is used.
    </p>
  {/if}
  <NumberField
    wide
    label="Height"
    unit="mm"
    value={text.height}
    oncommit={(height) => height > 0 && restyle({ height })}
  />
  <NumberField
    wide
    label="Letter space"
    unit="mm"
    value={text.letterSpacing}
    oncommit={(letterSpacing) => restyle({ letterSpacing })}
  />
  <NumberField
    wide
    label="Line space"
    unit="×"
    digits={2}
    value={text.lineSpacing}
    oncommit={(lineSpacing) => lineSpacing > 0 && restyle({ lineSpacing })}
  />
  <div class="flex items-center gap-1.5 text-xs">
    <span class="w-20 shrink-0 text-muted-foreground">Align</span>
    <ToggleGroup.Root
      type="single"
      variant="outline"
      size="sm"
      aria-label="Alignment"
      bind:value={() => text.align, (align) => align && restyle({ align: align as TextAlign })}
    >
      {#each aligns as option (option.value)}
        <ToggleGroup.Item value={option.value} aria-label={option.label} title={option.label}>
          <option.icon />
        </ToggleGroup.Item>
      {/each}
    </ToggleGroup.Root>
  </div>
  <p class="text-xs text-muted-foreground">Height is the height of a capital H.</p>
  {#if textEditing.error}
    <p role="alert" class="text-xs text-destructive">{textEditing.error}</p>
  {/if}
  <Button
    variant="outline"
    size="sm"
    class="self-start"
    onclick={() => editor.execute(convertToPath(editor.doc, [text.id]))}
  >
    Convert to path
  </Button>
</section>
