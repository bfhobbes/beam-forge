<script lang="ts">
  import { onMount } from "svelte";
  import { Cpu, Info, MonitorSmartphone } from "@lucide/svelte";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Popover from "#lib/components/ui/popover/index.js";
  import type { AppInfo } from "#lib/bindings/AppInfo.js";
  import { appInfo } from "#lib/desktop/platform.js";
  import { actions } from "#lib/actions/registry.svelte.js";

  let info = $state<AppInfo>();
  let error = $state("");
  let open = $state(false);

  async function load() {
    error = "";
    try {
      info = await appInfo();
    } catch (cause) {
      error = String(cause);
    }
  }

  // Loaded when first opened, so a slow or failing core never blocks the shell.
  function onOpenChange(open: boolean) {
    if (open && !info) void load();
  }

  onMount(() =>
    actions.register({
      id: "app.about",
      title: "About Beam Forge",
      group: "Help",
      keywords: ["version", "info"],
      run: () => {
        open = true;
        onOpenChange(true);
      },
    }),
  );
</script>

<Popover.Root bind:open {onOpenChange}>
  <Popover.Trigger>
    {#snippet child({ props })}
      <Button {...props} variant="ghost" size="icon-sm" aria-label="About Beam Forge">
        <Info />
      </Button>
    {/snippet}
  </Popover.Trigger>
  <Popover.Content align="end" class="w-72">
    <Popover.Header>
      <Popover.Title>Beam Forge</Popover.Title>
      <Popover.Description>Laser cutter editor and controller</Popover.Description>
    </Popover.Header>
    {#if error}
      <div role="alert" class="space-y-2">
        <p class="text-sm text-destructive">Couldn’t reach the Beam Forge core: {error}</p>
        <Button variant="outline" size="sm" onclick={load}>Try again</Button>
      </div>
    {:else if info}
      <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-sm">
        <dt class="flex items-center gap-2 text-muted-foreground">
          <MonitorSmartphone class="size-4" />App
        </dt>
        <dd>{info.appVersion} · {info.platform}{info.arch ? ` (${info.arch})` : ""}</dd>
        <dt class="flex items-center gap-2 text-muted-foreground"><Cpu class="size-4" />Core</dt>
        <dd>{info.core.version}</dd>
      </dl>
      {#if info.core.firmware.length}
        <div class="space-y-1.5">
          <p class="text-xs text-muted-foreground">Planned firmware support</p>
          <div class="flex flex-wrap gap-1.5">
            {#each info.core.firmware as firmware (firmware.id)}
              <Badge variant="secondary">{firmware.name}</Badge>
            {/each}
          </div>
        </div>
      {/if}
    {:else}
      <p class="text-sm text-muted-foreground" role="status">Connecting to the core…</p>
    {/if}
  </Popover.Content>
</Popover.Root>
