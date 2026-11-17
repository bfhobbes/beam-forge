<script lang="ts">
  import { onMount } from "svelte";
  import { X } from "@lucide/svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { isTyping } from "#lib/actions/shortcut.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import { importAdapter, isImportPath } from "./adapter.js";
  import { looksLikeSvg, sizeInUnits } from "./drawing.js";
  import { importer } from "./importer.svelte.js";

  /**
   * Mount once per window: the Import action, SVG and DXF files dropped on
   * the window, SVG markup pasted outside a text field, the units question
   * for DXFs that don't say, and the outcome notice.
   */
  let dragging = $state(false);

  onMount(() =>
    actions.register({
      id: "file.import",
      title: "Import SVG or DXF…",
      group: "File",
      keywords: [
        "svg",
        "dxf",
        "vector",
        "cad",
        "inkscape",
        "illustrator",
        "affinity",
        "fusion",
        "librecad",
        "artwork",
      ],
      enabled: () => importer.available && !importer.busy,
      hint: importer.available ? "An import is in progress" : "Import needs the desktop app",
      run: () => void importer.pick(),
    }),
  );

  onMount(() => {
    const stop = importAdapter.onFileDrop({
      hover: (paths) => (dragging = paths?.some(isImportPath) ?? false),
      drop: (paths) => void importer.files(paths),
    });
    return () => void stop.then((unlisten) => unlisten());
  });

  // Done notices fade on their own; errors stay until dismissed.
  $effect(() => {
    if (importer.notice?.kind !== "done") return;
    const timer = setTimeout(() => importer.dismiss(), 6000);
    return () => clearTimeout(timer);
  });

  function onpaste(event: ClipboardEvent) {
    if (event.defaultPrevented || isTyping(event.target)) return;
    const text = event.clipboardData?.getData("text/plain") ?? "";
    if (!looksLikeSvg(text)) return;
    event.preventDefault();
    void importer.text(text);
  }

  const question = $derived(importer.question);
</script>

<svelte:window {onpaste} />

{#if dragging}
  <div
    class="pointer-events-none fixed inset-0 z-40 m-2 grid place-items-center rounded-lg border-2 border-dashed border-primary bg-background/60 text-sm font-medium"
  >
    Drop to import SVG or DXF
  </div>
{/if}

<Dialog.Root
  open={question !== null}
  onOpenChange={(open) => {
    if (!open) importer.answer(null);
  }}
>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>What units is “{question?.name}” in?</Dialog.Title>
      <Dialog.Description>
        The DXF doesn’t say whether it was drawn in millimetres or inches. Pick the size that looks
        right.
      </Dialog.Description>
    </Dialog.Header>
    {#if question}
      <div class="grid gap-2">
        <Button variant="outline" class="justify-between" onclick={() => importer.answer("mm")}>
          <span>Millimetres</span>
          <span class="text-muted-foreground tabular-nums">{sizeInUnits(question, "mm")}</span>
        </Button>
        <Button variant="outline" class="justify-between" onclick={() => importer.answer("in")}>
          <span>Inches</span>
          <span class="text-muted-foreground tabular-nums">{sizeInUnits(question, "in")}</span>
        </Button>
      </div>
    {/if}
  </Dialog.Content>
</Dialog.Root>

{#if importer.notice}
  <div
    role={importer.notice.kind === "error" ? "alert" : "status"}
    class={[
      "fixed bottom-10 left-16 z-40 flex max-w-md items-start gap-2 rounded-md border bg-popover px-3 py-2 text-sm text-popover-foreground shadow-md",
      importer.notice.kind === "error" && "border-destructive/50 text-destructive",
    ]}
  >
    <span class="min-w-0 flex-1">{importer.notice.text}</span>
    <Button
      variant="ghost"
      size="icon-xs"
      aria-label="Dismiss"
      class="-mr-1"
      onclick={() => importer.dismiss()}
    >
      <X />
    </Button>
  </div>
{/if}
