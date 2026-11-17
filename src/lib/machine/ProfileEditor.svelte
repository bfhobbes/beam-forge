<script lang="ts">
  import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
  import type { OriginCorner } from "#lib/bindings/OriginCorner.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import { machineSettings } from "./settings.svelte.js";

  let { profile }: { profile: MachineProfile } = $props();

  const ORIGINS: [OriginCorner, string][] = [
    ["frontLeft", "Front left"],
    ["frontRight", "Front right"],
    ["rearLeft", "Rear left"],
    ["rearRight", "Rear right"],
  ];
  /** Mirrors `MAX_FRAME_POWER` in `bf-core`'s frame module; the profile save enforces it. */
  const MAX_FRAME_POWER = 5;
  const selectClass = "h-8 w-full rounded-lg border border-input bg-background px-2 text-sm";

  // The form starts again from the profile whenever it changes.
  let name = $derived(profile.name);
  let origin = $derived(profile.origin);
  let airAssist = $derived(profile.airAssist);
  let bedFromMachine = $derived(profile.bedFromMachine);
  let width = $derived(profile.bed.width);
  let height = $derived(profile.bed.height);
  let framePower = $derived(profile.framePower);
  let saved = $state(false);

  const changed = $derived(
    Number(framePower) !== profile.framePower ||
      name !== profile.name ||
      origin !== profile.origin ||
      airAssist !== profile.airAssist ||
      bedFromMachine !== profile.bedFromMachine ||
      width !== profile.bed.width ||
      height !== profile.bed.height,
  );

  async function save(event: SubmitEvent) {
    event.preventDefault();
    saved = await machineSettings.saveProfile({
      name: name.trim(),
      origin,
      airAssist: airAssist.trim(),
      bedFromMachine,
      bed: { width: Number(width), height: Number(height) },
      framePower: Number(framePower) || 0,
    });
    // Picks up max travel straight away while connected.
    if (saved && bedFromMachine && machineSettings.settings.length > 0) {
      await machineSettings.read();
    }
  }

  const rate = (value: number) => `${Math.round(value)} mm/min`;
  const accel = (value: number) => `${Math.round(value)} mm/s²`;
</script>

<form class="space-y-4" onsubmit={save} oninput={() => (saved = false)}>
  <div class="grid gap-3 sm:grid-cols-2">
    <label class="space-y-1 sm:col-span-2">
      <span class="text-xs text-muted-foreground">Name</span>
      <Input bind:value={name} required />
    </label>
    <label class="space-y-1">
      <span class="text-xs text-muted-foreground">Origin (X0 Y0)</span>
      <select class={selectClass} bind:value={origin}>
        {#each ORIGINS as [value, text] (value)}
          <option {value}>{text}</option>
        {/each}
      </select>
    </label>
    <label class="space-y-1">
      <span class="text-xs text-muted-foreground">Air-assist command</span>
      <Input class="font-mono" bind:value={airAssist} required />
    </label>
    <fieldset class="space-y-2 sm:col-span-2">
      <legend class="text-xs text-muted-foreground">Bed size</legend>
      <label class="flex items-center gap-2 text-sm">
        <input type="checkbox" bind:checked={bedFromMachine} />
        Use max travel ($130 × $131)
      </label>
      <div class="flex items-center gap-2">
        <Input
          aria-label="Bed width"
          type="number"
          min="1"
          step="any"
          class="w-28"
          bind:value={width}
          disabled={bedFromMachine}
        />
        <span class="text-muted-foreground">×</span>
        <Input
          aria-label="Bed depth"
          type="number"
          min="1"
          step="any"
          class="w-28"
          bind:value={height}
          disabled={bedFromMachine}
        />
        <span class="text-xs text-muted-foreground">mm</span>
      </div>
    </fieldset>
    <label class="space-y-1 sm:col-span-2">
      <span class="text-xs text-muted-foreground">Low-power frame (% power, 0 = laser off)</span>
      <div class="flex items-center gap-2">
        <Input
          type="number"
          min="0"
          max={MAX_FRAME_POWER}
          step="0.1"
          class="w-28"
          bind:value={framePower}
        />
        <span
          class={Number(framePower) > 0
            ? "text-xs font-medium text-destructive"
            : "text-xs text-muted-foreground"}
        >
          {Number(framePower) > 0
            ? `The laser fires (M4) at ${framePower}% while framing. Wear eye protection.`
            : "Framing moves the head with the laser off."}
        </span>
      </div>
    </label>
  </div>

  <section aria-label="From the controller" class="space-y-1">
    <h3 class="text-xs font-medium text-muted-foreground">
      From the controller (change these in Settings)
    </h3>
    <dl class="grid grid-cols-2 gap-x-4 gap-y-0.5 text-xs sm:grid-cols-4">
      <dt class="text-muted-foreground">Firmware</dt>
      <dd>{profile.firmware}</dd>
      <dt class="text-muted-foreground">Power ($31–$30)</dt>
      <dd class="tabular-nums">S{profile.minSpindle}–S{profile.maxSpindle}</dd>
      <dt class="text-muted-foreground">Laser mode ($32)</dt>
      <dd class={profile.laserMode ? "" : "font-medium text-destructive"}>
        {profile.laserMode ? "On" : "Off"}
      </dd>
      <dt class="text-muted-foreground">Homing ($22)</dt>
      <dd>{profile.homing ? "On" : "Off"}</dd>
      <dt class="text-muted-foreground">Max rate X / Y</dt>
      <dd class="tabular-nums">{rate(profile.maxRate.x)} / {rate(profile.maxRate.y)}</dd>
      <dt class="text-muted-foreground">Acceleration X / Y</dt>
      <dd class="tabular-nums">
        {accel(profile.acceleration.x)} / {accel(profile.acceleration.y)}
      </dd>
      <dt class="text-muted-foreground">Junction deviation ($11)</dt>
      <dd class="tabular-nums">{profile.junctionDeviation} mm</dd>
    </dl>
  </section>

  <div class="flex items-center justify-end gap-2">
    {#if saved}
      <span role="status" class="text-xs text-muted-foreground">Saved</span>
    {/if}
    <Button type="submit" disabled={!changed}>Save profile</Button>
  </div>
</form>
