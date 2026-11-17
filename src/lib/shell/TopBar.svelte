<script lang="ts">
  import { Crosshair, Play } from "@lucide/svelte";
  import { actions as registry } from "#lib/actions/registry.svelte.js";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Separator } from "#lib/components/ui/separator/index.js";
  import * as Tooltip from "#lib/components/ui/tooltip/index.js";
  import UpdateManager from "#lib/desktop/UpdateManager.svelte";
  import { lifecycle } from "#lib/desktop/lifecycle.svelte.js";
  import { desktop } from "#lib/desktop/platform.js";
  import favicon from "#lib/assets/favicon.svg";
  import JobTimeEstimate from "#lib/toolpath/JobTimeEstimate.svelte";
  import AboutPopover from "./AboutPopover.svelte";

  let { project = "Untitled" }: { project?: string } = $props();

  // Both actions are registered by JobSetupDialog: `machine.frame` frames the
  // job, `machine.run` opens the job dialog.
  const buttons = [
    { id: "machine.frame", label: "Frame", icon: Crosshair, variant: "outline" },
    { id: "machine.run", label: "Run", icon: Play, variant: "default" },
  ] as const;
</script>

<header class="flex h-11 shrink-0 items-center gap-2 border-b bg-background px-2.5">
  <img src={favicon} alt="" class="size-5" />
  <span class="text-sm font-semibold">Beam Forge</span>
  {#if !desktop}<Badge variant="outline">Browser preview</Badge>{/if}
  <Separator orientation="vertical" class="mx-1 my-auto h-5" />
  <span class="flex min-w-0 items-center gap-1.5 text-sm">
    <span class="truncate">{project}</span>
    {#if lifecycle.dirty}
      <span class="size-1.5 rounded-full bg-foreground/60" aria-hidden="true"></span>
      <span class="sr-only">(unsaved changes)</span>
    {/if}
  </span>

  <div class="ml-auto flex items-center gap-1">
    <UpdateManager />
    <JobTimeEstimate />
    {#each buttons as button (button.id)}
      {@const action = registry.get(button.id)}
      {@const enabled = action ? registry.enabled(action) : false}
      <Tooltip.Root>
        <!-- Disabled buttons get no pointer events, so the tooltip hangs off a wrapper. -->
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <span {...props}>
              <Button
                variant={button.variant}
                size="sm"
                disabled={!enabled}
                onclick={() => registry.run(button.id)}
              >
                <button.icon />{button.label}
              </Button>
            </span>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content>{enabled ? action?.title : action?.hint}</Tooltip.Content>
      </Tooltip.Root>
    {/each}
    <Separator orientation="vertical" class="mx-1 my-auto h-5" />
    <AboutPopover />
  </div>
</header>
