<script lang="ts">
  import { onMount, untrack } from "svelte";
  import * as Resizable from "#lib/components/ui/resizable/index.js";
  import * as Tooltip from "#lib/components/ui/tooltip/index.js";
  import Actions from "#lib/actions/Actions.svelte";
  import FileImport from "#lib/import/FileImport.svelte";
  import JobSetupDialog from "#lib/gcode/JobSetupDialog.svelte";
  import TestGridDialog from "#lib/test-grid/TestGridDialog.svelte";
  import ProjectFiles from "#lib/project/ProjectFiles.svelte";
  import { project } from "#lib/project/project.svelte.js";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { arrangeActions } from "#lib/document/arrange-actions.js";
  import { editActions } from "#lib/document/edit-actions.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import { selection } from "#lib/document/selection.svelte.js";
  import CanvasArea from "#lib/shell/CanvasArea.svelte";
  import Inspector from "#lib/shell/Inspector.svelte";
  import StatusBar from "#lib/shell/StatusBar.svelte";
  import ToolRail from "#lib/shell/ToolRail.svelte";
  import TopBar from "#lib/shell/TopBar.svelte";
  import { panelLayout } from "#lib/shell/panel-layout.js";
  import type { ToolId } from "#lib/shell/tools.js";
  import ConnectionPanel from "#lib/machine/ConnectionPanel.svelte";
  import { machine as connection } from "#lib/machine/connection.svelte.js";
  import { machineSettings } from "#lib/machine/settings.svelte.js";
  import MachineConsole from "#lib/machine/MachineConsole.svelte";
  import { startConsole } from "#lib/machine/console-actions.js";
  import { machineConsole } from "#lib/machine/console.svelte.js";
  import JobPanel from "#lib/machine/JobPanel.svelte";
  import JogPad from "#lib/machine/JogPad.svelte";
  import SetupChecklist from "#lib/machine/SetupChecklist.svelte";
  import { machineSetup } from "#lib/machine/setup.svelte.js";
  import { startJob } from "#lib/machine/job-actions.js";
  import { machineJob } from "#lib/machine/job.svelte.js";
  import { toolpathPreview } from "#lib/toolpath/preview.svelte.js";

  // Canvas | inspector, as percentages of the window width.
  const layout = panelLayout("main", [76, 24]);
  let tool = $state<ToolId>("select");
  let inspectorTab = $state("properties");
  let cursor = $state<{ x: number; y: number } | null>(null);
  // Where the pointer last was over the canvas: "Paste" from the palette lands there.
  let lastCursor: { x: number; y: number } | null = null;
  $effect(() => {
    if (cursor) lastCursor = cursor;
  });

  // App-wide: the status bar follows the connection even when the Machine tab is closed.
  onMount(() => {
    const started = connection.start();
    // The machine profile too: it sets the bed size the editor uses.
    const profile = machineSettings.start();
    return () => {
      void started.then((stop) => stop());
      void profile.then((stop) => stop());
    };
  });
  onMount(() => startConsole(machineConsole, () => connection.connected));
  // A running job holds the app busy, so it is followed app-wide too.
  onMount(() => startJob(machineJob, () => connection.connected));
  onMount(() =>
    actions.register(
      ...editActions(editor, selection, { pasteTarget: () => lastCursor }),
      // The active machine profile's bed, as the canvas draws it.
      ...arrangeActions(editor, selection, () => machineSettings.bed),
      {
        id: "machine.setup",
        title: "Set up this machine…",
        group: "Machine",
        keywords: ["setup", "wizard", "checklist", "first run", "calibrate", "profile"],
        enabled: () => machineSetup.available,
        hint: "Connect to a machine first",
        run: () => {
          inspectorTab = "machine";
          void machineSetup.reopen();
        },
      },
    ),
  );
  // Offers the setup checklist the first time a machine without a profile connects.
  $effect(() => {
    const state = connection.state;
    const port = state.status === "connected" ? state.port.key : null;
    const profile = machineSettings.profile;
    untrack(() => machineSetup.follow(port, profile));
  });

  // A running design follows the head's reported position...
  $effect(() => {
    const status = connection.status;
    untrack(() => machineJob.follow(status));
  });
  // ...and shows its progress on the canvas's toolpath view while it runs.
  $effect(() => {
    const run = machineJob.run;
    if (run && !run.done) {
      const progress = { completed: run.completed, head: run.head };
      untrack(() => toolpathPreview.follow(run.plan.toolpath, progress));
    } else {
      untrack(() => toolpathPreview.unfollow());
    }
  });
</script>

<Tooltip.Provider delayDuration={400}>
  <div class="flex h-dvh flex-col overflow-hidden bg-background text-foreground">
    <TopBar project={project.name} />
    <div class="flex min-h-0 flex-1">
      <ToolRail bind:tool />
      <Resizable.PaneGroup direction="horizontal" onLayoutChange={layout.save}>
        <Resizable.Pane defaultSize={layout.sizes[0]} minSize={40}>
          <CanvasArea bind:tool bind:cursor />
        </Resizable.Pane>
        <Resizable.Handle />
        <Resizable.Pane defaultSize={layout.sizes[1]} minSize={16} maxSize={45}>
          <Inspector bind:tab={inspectorTab}>
            {#snippet machine()}
              <ConnectionPanel />
              <SetupChecklist />
              <JobPanel />
              <JogPad />
              <MachineConsole />
            {/snippet}
          </Inspector>
        </Resizable.Pane>
      </Resizable.PaneGroup>
    </div>
    <StatusBar
      connection={connection.statusText}
      status={connection.status}
      job={machineJob.progress}
      {cursor}
    />
  </div>
  <Actions />
  <FileImport />
  <ProjectFiles />
  <JobSetupDialog />
  <TestGridDialog />
</Tooltip.Provider>
