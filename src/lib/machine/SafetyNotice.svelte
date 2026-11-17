<script lang="ts">
  import { onMount } from "svelte";
  import { TriangleAlert } from "@lucide/svelte";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Checkbox } from "#lib/components/ui/checkbox/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import { Switch } from "#lib/components/ui/switch/index.js";
  import { machine } from "./connection.svelte.js";
  import { realHardware } from "./safety.svelte.js";

  // Changing this text in a way users must read again? Bump NOTICE_VERSION
  // in safety.svelte.ts and src-tauri/src/machine/safety.rs.

  let open = $state(false);
  let understood = $state(false);
  // The switch can't change under a live connection.
  const locked = $derived(machine.state.status !== "disconnected" || realHardware.saving);

  onMount(() => void realHardware.load());

  // Controlled: the switch only shows on once the notice is accepted.
  function toggle(checked: boolean) {
    if (checked) {
      understood = false;
      open = true;
    } else {
      void realHardware.turnOff();
    }
  }

  async function accept() {
    if (await realHardware.accept()) {
      open = false;
      // Drop the "real laser control is off" refusal from the last connect.
      machine.error = "";
    }
  }
</script>

<div class="space-y-1">
  <label class="flex items-center justify-between gap-2 text-sm">
    <span class="font-medium">Real laser</span>
    <Switch
      aria-label="Real laser"
      bind:checked={() => realHardware.enabled, toggle}
      disabled={locked || !machine.available}
    />
  </label>
  <p class="text-xs text-muted-foreground">
    {#if realHardware.enabled}
      On: serial and network ports can drive a laser. Alpha software, so never leave it unattended.
    {:else}
      Off: only the simulators can connect.
    {/if}
  </p>
  {#if realHardware.error && !open}
    <p role="alert" class="text-xs text-destructive">{realHardware.error}</p>
  {/if}
</div>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-lg">
    <Dialog.Header>
      <Dialog.Title class="flex items-center gap-2">
        <TriangleAlert class="size-5 shrink-0 text-destructive" />Before you drive a real laser
      </Dialog.Title>
      <Dialog.Description>
        Beam Forge is alpha software and has had little testing on real lasers. Read this before
        turning on real laser control.
      </Dialog.Description>
    </Dialog.Header>
    <ul class="max-h-[50vh] list-disc space-y-2 overflow-y-auto pl-5 text-sm">
      <li>
        <strong>Expect bugs.</strong> Beam Forge may move the head somewhere you didn't expect, fire the
        laser when you didn't ask it to, use the wrong power, or fail to turn the laser off.
      </li>
      <li>
        <strong>Never leave the laser unattended.</strong> Stay with the machine for the whole job, watching
        it and ready to stop it.
      </li>
      <li>
        <strong>Keep a way to cut power within reach</strong>, such as the machine's emergency stop
        or power switch. Don't rely on Beam Forge's Stop button alone.
      </li>
      <li>
        <strong>Protect your eyes.</strong> Wear safety glasses rated for your laser's wavelength, and
        keep other people and pets away from the beam.
      </li>
      <li>
        <strong>Lasers start fires.</strong> Keep a fire extinguisher nearby, ventilate the fumes, and
        never cut materials that give off toxic or corrosive gases, such as PVC or vinyl.
      </li>
      <li>
        <strong>Check jobs before running them.</strong> Preview them, try them in the simulator, and
        frame them on the machine first.
      </li>
      <li>
        <strong>You use Beam Forge at your own risk.</strong> It is provided "as is", without warranty
        of any kind (see the MIT licence). You are responsible for your machine, your workspace and anything
        that happens while Beam Forge controls it.
      </li>
    </ul>
    <label class="flex items-start gap-3 rounded-lg border p-3 text-sm">
      <Checkbox class="mt-0.5" bind:checked={understood} />
      <span>
        I have read this notice. I understand Beam Forge is alpha software, and I accept the risk of
        using it to control a real laser.
      </span>
    </label>
    {#if realHardware.error}
      <p role="alert" class="text-sm text-destructive">{realHardware.error}</p>
    {/if}
    <Dialog.Footer>
      <Button variant="outline" onclick={() => (open = false)}>Cancel</Button>
      <Button variant="destructive" disabled={!understood || realHardware.saving} onclick={accept}>
        Turn on real laser control
      </Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
