<script lang="ts">
  import { onMount } from "svelte";
  import { X } from "@lucide/svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import { desktop } from "#lib/desktop/platform.js";
  import { project } from "./project.svelte.js";

  /**
   * Mount once per window: the New / Open / Save / Save As and Open Recent
   * actions, the window title, the unsaved-changes and recovery prompts, and
   * the error notice for a failed open or save.
   */
  const hint = "Projects need the desktop app";

  onMount(() =>
    actions.register(
      {
        id: "file.new",
        title: "New project",
        group: "File",
        keywords: ["blank", "clear", "bfp"],
        run: () => void project.newProject(),
      },
      {
        id: "file.open",
        title: "Open project…",
        group: "File",
        keywords: ["load", "bfp"],
        enabled: () => project.available,
        hint,
        run: () => void project.open(),
      },
      {
        id: "file.save",
        title: "Save project",
        group: "File",
        keywords: ["write", "bfp"],
        enabled: () => project.available,
        hint,
        run: () => void project.save(),
      },
      {
        id: "file.save-as",
        title: "Save project as…",
        group: "File",
        keywords: ["copy", "rename", "bfp"],
        enabled: () => project.available,
        hint,
        run: () => void project.saveAs(),
      },
    ),
  );

  // Recent projects are palette actions, rebuilt whenever the list changes.
  $effect(() =>
    actions.register(
      ...project.recent.map((recent) => ({
        id: `file.open-recent:${recent.path}`,
        title: `Open recent: ${recent.name}`,
        group: "File",
        keywords: ["recent", "bfp", recent.path],
        run: () => void project.open(recent.path),
      })),
    ),
  );

  onMount(() => {
    const started = project.start();
    return () => void started.then((stop) => stop());
  });

  $effect(() => {
    const title = project.title;
    document.title = title;
    if (desktop)
      void getCurrentWindow()
        .setTitle(title)
        .catch(() => {});
  });

  function savedAt(milliseconds: number): string {
    return new Date(milliseconds).toLocaleString();
  }
</script>

<Dialog.Root
  open={project.unsaved !== null}
  onOpenChange={(open) => {
    if (!open) project.answerUnsaved("cancel");
  }}
>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Save changes to {project.name}?</Dialog.Title>
      <Dialog.Description>
        Your project has changes that haven't been saved. Save them before {project.unsaved
          ?.action ?? "continuing"}?
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => project.answerUnsaved("cancel")}>Cancel</Button>
      <Button variant="destructive" onclick={() => project.answerUnsaved("discard")}>
        Don't save
      </Button>
      <Button disabled={!project.available} onclick={() => project.answerUnsaved("save")}>
        Save
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

<Dialog.Root
  open={project.recovery !== null}
  onOpenChange={(open) => {
    if (!open) void project.discardRecovery();
  }}
>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>Restore unsaved work?</Dialog.Title>
      <Dialog.Description>
        Beam Forge didn't close cleanly last time. A copy of {project.recovery?.name ??
          "your project"}
        {#if project.recovery}from {savedAt(project.recovery.savedAt)}{/if} was kept.
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer>
      <Button variant="outline" onclick={() => void project.discardRecovery()}>Discard it</Button>
      <Button onclick={() => void project.restoreRecovery()}>Restore</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>

{#if project.error}
  <div
    role="alert"
    class="fixed right-4 bottom-10 z-40 flex max-w-md items-start gap-2 rounded-md border border-destructive/50 bg-popover px-3 py-2 text-sm text-destructive shadow-md"
  >
    <span class="min-w-0 flex-1">{project.error}</span>
    <Button
      variant="ghost"
      size="icon-xs"
      aria-label="Dismiss"
      class="-mr-1"
      onclick={() => (project.error = "")}
    >
      <X />
    </Button>
  </div>
{/if}
