<script lang="ts">
  import { onMount } from "svelte";
  import { check } from "@tauri-apps/plugin-updater";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { Download } from "@lucide/svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import { lifecycle } from "./lifecycle.svelte.js";
  import { desktop } from "./platform.js";
  import { createUpdater } from "./updates.js";

  const updater = createUpdater({ check: () => check({ timeout: 15_000 }), relaunch }, () =>
    lifecycle.acquireInstallLock(),
  );
  const status = updater.state;
  let open = $state(false);
  let prompted = false;
  const working = $derived(["checking", "downloading", "installing"].includes($status.stage));

  onMount(() =>
    actions.register({
      id: "app.check-updates",
      title: "Check for updates",
      group: "App",
      keywords: ["update", "version", "upgrade"],
      run: show,
    }),
  );

  onMount(() => {
    if (!desktop) return;
    const unsubscribe = status.subscribe((value) => {
      if (value.stage === "available" && !prompted) {
        prompted = true;
        open = true;
      }
    });
    const startup = setTimeout(() => void updater.check(), 3000);
    const interval = setInterval(() => void updater.check(), 4 * 60 * 60 * 1000);
    return () => {
      clearTimeout(startup);
      clearInterval(interval);
      unsubscribe();
      void updater.dispose();
    };
  });

  function show() {
    open = true;
    if (desktop) void updater.check();
  }
</script>

<Button variant="ghost" size="sm" onclick={show} aria-label="Software updates">
  <Download />
  {$status.stage === "ready" ? "Update ready" : "Updates"}
</Button>
<Dialog.Root bind:open>
  <Dialog.Content
    onInteractOutside={(event) => {
      if ($status.stage === "installing") event.preventDefault();
    }}
    onEscapeKeydown={(event) => {
      if ($status.stage === "installing") event.preventDefault();
    }}
    showCloseButton={$status.stage !== "installing"}
  >
    <Dialog.Header>
      <Dialog.Title>
        {$status.version ? `Beam Forge ${$status.version}` : "Software updates"}
      </Dialog.Title>
      <Dialog.Description>
        {#if !desktop}Updates are available in the installed desktop app.
        {:else if $status.stage === "checking"}Checking for updates…
        {:else if $status.stage === "current"}You’re running the latest version.
        {:else if $status.stage === "available"}A new version is available. Download it now, or
          continue working.
        {:else if $status.stage === "downloading"}Downloading and verifying the update…
        {:else if $status.stage === "ready"}Your update is verified and ready. Installing will close
          and restart Beam Forge.
        {:else if $status.stage === "installing"}Installing the update. Beam Forge will restart
          shortly.
        {:else if $status.stage === "restart"}The update is installed. Restart Beam Forge to finish.
        {:else if $status.stage === "error"}Couldn’t check for updates. You can keep working and try
          again later.
        {:else}Check for a newer version of Beam Forge.{/if}
      </Dialog.Description>
    </Dialog.Header>
    {#if $status.notes}
      <p class="max-h-48 overflow-auto text-sm whitespace-pre-wrap">{$status.notes}</p>
    {/if}
    {#if $status.stage === "downloading"}
      <progress
        class="h-2 w-full accent-primary"
        max={$status.total || 1}
        value={$status.total ? $status.received : undefined}
        aria-label="Update download"
      ></progress>
      <p class="text-xs text-muted-foreground" role="status">
        {($status.received / 1048576).toFixed(1)} MB downloaded{#if $status.total}
          of {($status.total / 1048576).toFixed(1)} MB{/if}
      </p>
    {/if}
    {#if $status.error}<p role="alert" class="text-sm text-destructive">{$status.error}</p>{/if}
    {#if ["ready", "restart"].includes($status.stage) && !lifecycle.canClose}
      <p class="text-sm text-muted-foreground">
        Save your project and finish any running job before restarting.
      </p>
    {/if}
    <Dialog.Footer>
      {#if $status.stage !== "installing"}
        <Button variant="outline" onclick={() => (open = false)}>
          {working ? "Continue working" : "Later"}
        </Button>
      {/if}
      {#if desktop}
        {#if $status.stage === "available"}
          <Button onclick={() => updater.download()}>Download update</Button>
        {:else if $status.stage === "ready"}
          <Button disabled={!lifecycle.canClose} onclick={() => updater.install()}>
            Install and restart
          </Button>
        {:else if $status.stage === "restart"}
          <Button disabled={!lifecycle.canClose} onclick={() => updater.restart()}>
            Restart now
          </Button>
        {:else if !working}
          <Button onclick={() => updater.check()}>Check again</Button>
        {/if}
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
