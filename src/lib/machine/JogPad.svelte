<script lang="ts">
  import { onDestroy } from "svelte";
  import {
    ArrowDown,
    ArrowDownLeft,
    ArrowDownRight,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowUpLeft,
    ArrowUpRight,
    Crosshair,
    Flame,
    House,
    LocateFixed,
    LocateOff,
    LockOpen,
    Wind,
  } from "@lucide/svelte";
  import type { JogDirection } from "#lib/bindings/JogDirection.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as ToggleGroup from "#lib/components/ui/toggle-group/index.js";
  import NumberField from "#lib/shell/NumberField.svelte";
  import {
    JOG_PAD,
    JOG_STEPS,
    machineControl as control,
    TEST_FIRE_MAX_MS,
    TEST_FIRE_MAX_POWER,
  } from "./control.svelte.js";

  const icons = [
    ArrowUpLeft,
    ArrowUp,
    ArrowUpRight,
    ArrowLeft,
    ArrowRight,
    ArrowDownLeft,
    ArrowDown,
    ArrowDownRight,
  ];
  const pad = JOG_PAD.map((arrow, index) => ({ ...arrow, icon: icons[index] }));
  // The origin button sits in the middle of the 3×3 grid.
  const top = pad.slice(0, 4);
  const bottom = pad.slice(4);

  const notReady = "Connect, and wait for the machine to be idle.";
  const holding = (arrow: JogDirection) =>
    control.holding?.x === arrow.x && control.holding?.y === arrow.y;

  // A held jog stops if the machine can no longer jog (an alarm, a
  // disconnect) or the panel closes, even if the release never arrives.
  $effect(() => {
    if (control.holding && !control.canJog) control.cancel();
  });
  onDestroy(() => control.cancel());

  function onpointerdown(event: PointerEvent, arrow: JogDirection) {
    if (event.button !== 0) return;
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
    control.press(arrow);
  }

  function onkeydown(event: KeyboardEvent, arrow: JogDirection) {
    if ((event.key === "Enter" || event.key === " ") && !event.repeat) {
      event.preventDefault();
      void control.jog(arrow);
    }
  }
</script>

<!-- Releases bubble here from the captured button; one outside it, or lost to a
     disabled button, still stops the jog. -->
<svelte:window onblur={() => control.cancel()} onpointerup={() => control.release()} />

{#snippet arrowButton(arrow: (typeof pad)[number])}
  <Button
    variant={holding(arrow) ? "default" : "outline"}
    size="icon"
    class="size-9 touch-none"
    aria-label={arrow.label}
    title="{arrow.label}: click to jog {control.step} mm, hold to keep moving"
    disabled={!control.canJog && !holding(arrow)}
    onpointerdown={(event: PointerEvent) => onpointerdown(event, arrow)}
    onpointercancel={() => control.cancel()}
    onkeydown={(event: KeyboardEvent) => onkeydown(event, arrow)}
  >
    <arrow.icon />
  </Button>
{/snippet}

<section aria-label="Jog" class="space-y-3">
  <h2 class="text-sm font-medium">Jog</h2>

  <div class="flex gap-3">
    <div class="grid shrink-0 grid-cols-3 gap-1">
      {#each top as arrow (arrow.label)}
        {@render arrowButton(arrow)}
      {/each}
      <Button
        variant="outline"
        size="icon"
        class="size-9"
        aria-label="Go to work origin"
        title="Go to work origin (X0 Y0)"
        disabled={!control.idle}
        onclick={() => control.goToOrigin()}
      >
        <LocateFixed />
      </Button>
      {#each bottom as arrow (arrow.label)}
        {@render arrowButton(arrow)}
      {/each}
    </div>

    <div class="min-w-0 flex-1 space-y-2">
      <ToggleGroup.Root
        type="single"
        variant="outline"
        size="sm"
        class="w-full"
        aria-label="Jog step"
        bind:value={() => String(control.step), (step) => step && control.setStep(Number(step))}
      >
        {#each JOG_STEPS as step (step)}
          <ToggleGroup.Item value={String(step)} class="flex-1 px-1 tabular-nums">
            {step}
          </ToggleGroup.Item>
        {/each}
      </ToggleGroup.Root>
      <p class="text-xs text-muted-foreground">Step, mm</p>
      <NumberField
        wide
        label="Speed"
        unit="mm/min"
        digits={0}
        value={control.feed}
        oncommit={(feed) => control.setFeed(feed)}
      />
    </div>
  </div>

  <div class="grid grid-cols-2 gap-1.5">
    <Button
      size="sm"
      variant="outline"
      disabled={!control.canHome}
      title={control.homingEnabled
        ? "Run the homing cycle ($H)"
        : "Homing isn't configured on this machine ($22=0)."}
      onclick={() => control.command({ command: "home" })}
    >
      <House />Home
    </Button>
    <Button
      size="sm"
      variant="outline"
      disabled={!control.canUnlock}
      title="Clear the alarm without homing ($X)"
      onclick={() => control.command({ command: "unlock" })}
    >
      <LockOpen />Unlock
    </Button>
    <Button
      size="sm"
      variant="outline"
      disabled={!control.idle}
      title="Make the head's position work X0 Y0"
      onclick={() => control.command({ command: "setOrigin" })}
    >
      <Crosshair />Set origin
    </Button>
    <Button
      size="sm"
      variant="outline"
      disabled={!control.idle}
      title="Put work X0 Y0 back on machine X0 Y0"
      onclick={() => control.command({ command: "clearOrigin" })}
    >
      <LocateOff />Clear origin
    </Button>
    <Button
      size="sm"
      variant={control.airAssist ? "default" : "outline"}
      class="col-span-2"
      aria-pressed={control.airAssist ?? false}
      disabled={!control.idle}
      title="Air assist (M8 on, M9 off)"
      onclick={() => control.toggleAirAssist()}
    >
      <Wind />Air assist {control.airAssist ? "on" : "off"}
    </Button>
  </div>

  <div class="space-y-2 rounded-lg border border-destructive/40 p-2">
    <div class="flex items-center justify-between gap-2">
      <h3 class="text-xs font-medium">Laser test fire</h3>
      <span class="text-xs text-muted-foreground">
        max {TEST_FIRE_MAX_POWER}% · {TEST_FIRE_MAX_MS} ms
      </span>
    </div>
    <NumberField
      wide
      label="Power"
      unit="%"
      digits={1}
      value={control.power}
      oncommit={(power) => control.setPower(power)}
    />
    <NumberField
      wide
      label="Duration"
      unit="ms"
      digits={0}
      value={control.durationMs}
      oncommit={(ms) => control.setDuration(ms)}
    />
    <Button
      size="sm"
      variant="destructive"
      class="w-full"
      disabled={!control.idle}
      title={control.idle ? "The laser turns on. Wear eye protection." : notReady}
      onclick={() => control.testFire()}
    >
      <Flame />{control.firing
        ? "Firing…"
        : `Fire laser ${control.power}% · ${control.durationMs} ms`}
    </Button>
  </div>

  <p class="text-xs text-muted-foreground">
    Arrow keys jog one step while the canvas has focus and nothing is selected.
  </p>

  {#if control.error}
    <p role="alert" class="text-sm text-destructive">{control.error}</p>
  {/if}
</section>
