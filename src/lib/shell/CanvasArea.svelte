<script lang="ts">
  import PaintBucket from "@lucide/svelte/icons/paint-bucket";
  import { onMount, untrack } from "svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import DesignCanvas from "#lib/canvas/DesignCanvas.svelte";
  import { MISSING_LAYER_COLOR } from "#lib/canvas/scene.js";
  import { viewPrefs } from "#lib/canvas/view-prefs.svelte.js";
  import { Toggle } from "#lib/components/ui/toggle/index.js";
  import * as ToggleGroup from "#lib/components/ui/toggle-group/index.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import { machineSettings } from "#lib/machine/settings.svelte.js";
  import type { Point } from "#lib/document/model.js";
  import ToolpathBar from "#lib/toolpath/ToolpathBar.svelte";
  import { toolpathPreview as preview } from "#lib/toolpath/preview.svelte.js";
  import type { ToolpathDrawing } from "#lib/toolpath/render.js";
  import { fontList } from "#lib/text/fonts.svelte.js";
  import { textEditing } from "#lib/text/text-editing.svelte.js";
  import { shapeTool, type ToolId } from "./tools.js";

  /**
   * The canvas region: the design, or its toolpath with the playback bar
   * docked below. `cursor` is the pointer's position on the bed in mm.
   * After a drawing tool finishes a shape, `tool` goes back to select.
   */
  let {
    tool = $bindable("select"),
    cursor = $bindable(null),
  }: { tool?: ToolId; cursor?: Point | null } = $props();

  const drawing = $derived.by((): ToolpathDrawing | null => {
    const { timeline, progress } = preview;
    if (!preview.shown || !timeline || !progress) return null;
    const colors = timeline.layers.map(
      (layer) =>
        editor.doc.layers.find((candidate) => candidate.id === layer.layerId)?.color ??
        MISSING_LAYER_COLOR,
    );
    return { timeline, colors, progress };
  });

  // Fonts load when the text tool is first picked, so new text starts in an installed family.
  $effect(() => {
    if (tool !== "text") return;
    void fontList
      .load()
      .then(() => textEditing.useInstalledFamily(fontList.families.map((family) => family.name)));
  });

  // Leaving the text tool or the design view ends typing; double-click editing is select-tool work.
  $effect(() => {
    if (preview.shown || (tool !== "text" && tool !== "select"))
      untrack(() => textEditing.finish());
  });

  onMount(() =>
    actions.register({
      id: "view.toggle-fills",
      title: "Show or hide fills",
      group: "View",
      keywords: ["fill", "shade", "solid", "preview"],
      run: () => viewPrefs.setShowFills(!viewPrefs.showFills),
    }),
  );
</script>

<section aria-label="Canvas" data-tool={tool} class="flex h-full flex-col bg-muted/40">
  <div class="relative min-h-0 flex-1">
    <DesignCanvas
      bed={machineSettings.bed}
      selecting={tool === "select" && !preview.shown}
      drawing={preview.shown ? null : shapeTool(tool)}
      ondrawn={() => (tool = "select")}
      toolpath={drawing}
      texting={tool === "text" && !preview.shown}
      showFills={viewPrefs.showFills}
      bind:cursor
    />
    <div class="absolute top-7 right-2 flex gap-2">
      {#if !preview.shown}
        <Toggle
          variant="outline"
          size="sm"
          class="bg-background px-3"
          title="Shade the shapes on Fill layers"
          bind:pressed={() => viewPrefs.showFills, (value) => viewPrefs.setShowFills(value)}
        >
          <PaintBucket />
          Fills
        </Toggle>
      {/if}
      <ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        class="bg-background"
        aria-label="Canvas view"
        bind:value={
          () => (preview.shown ? "toolpath" : "design"),
          (view) => view && preview.toggle(view === "toolpath")
        }
      >
        <ToggleGroup.Item value="design" class="px-3">Design</ToggleGroup.Item>
        <ToggleGroup.Item value="toolpath" class="px-3">Toolpath</ToggleGroup.Item>
      </ToggleGroup.Root>
    </div>
  </div>
  <div class:hidden={!preview.shown}>
    <ToolpathBar />
  </div>
</section>
