<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import { BUSY_REASON, machineJob } from "#lib/machine/job.svelte.js";
  import { closePrompt, stopJobAndClose } from "./close-guard.js";
  import { lifecycle } from "./lifecycle.svelte.js";
  import { desktop } from "./platform.js";

  let open = $state(false);
  let error = $state("");
  let stopping = $state(false);
  /** The job couldn't be confirmed stopped; closing anyway is up to the user. */
  let stopFailed = $state(false);
  const prompt = $derived(closePrompt(lifecycle, BUSY_REASON));

  onMount(() => {
    if (!desktop) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    getCurrentWindow()
      .onCloseRequested((event) => {
        if (lifecycle.canClose) return;
        event.preventDefault();
        error = "";
        stopFailed = false;
        open = true;
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((cause) => {
        error = `Close protection is unavailable: ${String(cause)}`;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  async function discardAndClose() {
    lifecycle.dirty = false;
    try {
      await getCurrentWindow().close();
    } catch (cause) {
      error = String(cause);
    }
  }

  /** Stops the job, waits for the laser to be off, then closes. */
  async function stopAndClose() {
    stopping = true;
    error = "";
    const failed = await stopJobAndClose({
      stop: () => machineJob.stopForClose(),
      error: () => machineJob.error,
      discard: () => (lifecycle.dirty = false),
      destroy: () => getCurrentWindow().destroy(),
    });
    stopping = false;
    if (failed) {
      error = failed;
      stopFailed = true;
    }
  }

  /** After a stop the machine didn't confirm: the app's exit still stops and resets it. */
  async function closeAnyway() {
    lifecycle.dirty = false;
    try {
      await getCurrentWindow().destroy();
    } catch (cause) {
      error = String(cause);
    }
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content>
    <Dialog.Header>
      <Dialog.Title>
        {#if prompt === "job"}
          A laser job is running
        {:else if prompt === "busy"}
          Beam Forge is busy
        {:else}
          Unsaved changes
        {/if}
      </Dialog.Title>
      <Dialog.Description>
        {#if prompt === "job"}
          Closing stops the job first: the machine holds, resets and turns the laser off, and then
          Beam Forge closes.
          {#if lifecycle.dirty}Your unsaved changes will be lost.{/if}
        {:else if lifecycle.locked}
          An update is installing. Beam Forge will restart when it finishes.
        {:else if prompt === "busy"}
          Wait for this to finish before closing: {lifecycle.busy.join(", ")}.
        {:else}
          Your project has changes that haven't been saved. Close anyway and lose them?
        {/if}
      </Dialog.Description>
    </Dialog.Header>
    {#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
    <Dialog.Footer>
      <Button variant="outline" disabled={stopping} onclick={() => (open = false)}>
        Keep working
      </Button>
      {#if prompt === "job" && stopFailed}
        <Button variant="destructive" onclick={closeAnyway}>Close anyway</Button>
      {:else if prompt === "job"}
        <Button variant="destructive" disabled={stopping} onclick={stopAndClose}>
          {stopping ? "Stopping the job…" : "Stop job and close"}
        </Button>
      {:else if prompt !== "busy"}
        <Button variant="destructive" onclick={discardAndClose}>Discard and close</Button>
      {/if}
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
