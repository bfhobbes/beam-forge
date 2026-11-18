<script lang="ts">
  import type { Snippet } from "svelte";
  import {
    ChevronDown,
    ChevronRight,
    Circle,
    CircleCheck,
    CircleSlash,
    ListChecks,
    X,
  } from "@lucide/svelte";
  import type { OriginCorner } from "#lib/bindings/OriginCorner.js";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import { machine } from "./connection.svelte.js";
  import { machineControl as control } from "./control.svelte.js";
  import { machineJob } from "./job.svelte.js";
  import OriginDiagram from "./OriginDiagram.svelte";
  import { machineSettings } from "./settings.svelte.js";
  import {
    CHECKLIST,
    cornerName,
    EYE_SAFETY_REMINDER,
    JOG_TEST_MM,
    jogExpectation,
    machineSetup as checklist,
    TEST_SQUARE,
    testSquare,
    type ChecklistStep,
    type StepStatus,
  } from "./setup.svelte.js";

  /**
   * "Set up this machine": a checklist at the top of the Machine tab. It
   * never blocks the panel: the jog pad, status and console stay usable
   * while a step is open.
   */

  const TITLES: Record<ChecklistStep, string> = {
    connected: "Connected",
    bed: "Bed and origin",
    laserMode: "Laser mode",
    homing: "Homing",
    handsOn: "Hands-on tests",
    airAssist: "Air assist",
  };
  const ORIGINS: [OriginCorner, string][] = [
    ["frontLeft", "Front left"],
    ["frontRight", "Front right"],
    ["rearLeft", "Rear left"],
    ["rearRight", "Rear right"],
  ];
  const selectClass = "h-8 rounded-lg border border-input bg-background px-2 text-sm";

  const profile = $derived(machineSettings.profile);
  const settingsRead = $derived(machineSettings.settings.length > 0);

  // Bed and origin: the form starts again from the profile whenever it changes.
  let origin = $derived(profile?.origin ?? "frontLeft");
  let bedFromMachine = $derived(profile?.bedFromMachine ?? true);
  let width = $derived(profile?.bed.width ?? 0);
  let height = $derived(profile?.bed.height ?? 0);

  async function saveBed() {
    const saved = await machineSettings.saveProfile({
      origin,
      bedFromMachine,
      bed: { width: Number(width), height: Number(height) },
    });
    // Picks up max travel straight away.
    if (saved && bedFromMachine && settingsRead) await machineSettings.read();
    if (saved) await checklist.resolve("bed", "done");
  }

  async function laserModeOn() {
    if (await machineSettings.write(32, "1")) await checklist.resolve("laserMode", "done");
  }

  let homeResult = $state<"homed" | "failed" | null>(null);
  async function testHome() {
    homeResult = null;
    const homed = await control.testHome();
    homeResult = homed ? "homed" : "failed";
    if (homed) await checklist.resolve("homing", "done");
  }

  // The jog test: +X, then +Y, each confirmed by what the user saw.
  const expected = $derived(jogExpectation(profile?.origin ?? "frontLeft"));
  let jogged = $state<{ x: boolean; y: boolean }>({ x: false, y: false });
  let jogSeen = $state<{ x: boolean | null; y: boolean | null }>({ x: null, y: null });
  async function jogTest(axis: "x" | "y") {
    const direction = axis === "x" ? { x: 1, y: 0 } : { x: 0, y: 1 };
    jogSeen[axis] = null;
    jogged[axis] = await control.jog(direction, JOG_TEST_MM);
  }
  async function sawJog(axis: "x" | "y", moved: boolean) {
    jogSeen[axis] = moved;
    if (jogSeen.x && jogSeen.y) await checklist.resolve("jogTest", "done");
  }

  // The test square.
  let framed = $state(false);
  let frameSeen = $state<boolean | null>(null);
  let frameError = $state("");
  async function frameSquare() {
    if (!profile) return;
    frameSeen = null;
    framed = await machineJob.frame({
      document: testSquare(profile),
      profile,
      placement: { startFrom: "absolute", anchor: "bottomLeft" },
      shape: "box",
    });
    frameError = framed ? "" : machineJob.error;
  }
  const frameLaser = $derived(
    profile && profile.framePower > 0
      ? `at ${profile.framePower}% power (the profile's low-power frame)`
      : "with the laser off",
  );

  // The test fire.
  let safetyChecked = $state(false);
  let fired = $state(false);
  let fireSeen = $state<boolean | null>(null);
  async function testFire() {
    fireSeen = null;
    fired = await control.testFire();
  }

  // Air assist.
  let airToggled = $state(false);
  let airSeen = $state<boolean | null>(null);
  async function toggleAir() {
    airSeen = null;
    if (await control.setAirAssist(!control.airAssist)) airToggled = true;
  }

  async function seen(step: "frameTest" | "fireTest" | "airAssist", yes: boolean) {
    if (step === "frameTest") frameSeen = yes;
    if (step === "fireTest") fireSeen = yes;
    if (step === "airAssist") airSeen = yes;
    if (yes) await checklist.resolve(step, "done");
  }

  const notReady = $derived(
    !machine.connected
      ? "Connect to the machine first."
      : machineJob.active
        ? "Wait for the job to finish."
        : "Wait for the machine to be idle.",
  );
</script>

{#snippet statusIcon(status: StepStatus, label: string)}
  {#if status === "done"}
    <CircleCheck class="size-4 shrink-0 text-primary" aria-label="{label}: done" />
  {:else if status === "skipped"}
    <CircleSlash class="size-4 shrink-0 text-muted-foreground" aria-label="{label}: skipped" />
  {:else}
    <Circle class="size-4 shrink-0 text-muted-foreground" aria-label="{label}: to do" />
  {/if}
{/snippet}

{#snippet question(text: string, answer: (yes: boolean) => void, answered: boolean | null)}
  <div class="space-y-1.5">
    <p class="text-xs font-medium">{text}</p>
    <div class="flex gap-2">
      <Button
        size="sm"
        variant={answered === true ? "default" : "outline"}
        onclick={() => answer(true)}
      >
        Yes
      </Button>
      <Button
        size="sm"
        variant={answered === false ? "default" : "outline"}
        onclick={() => answer(false)}
      >
        No
      </Button>
    </div>
  </div>
{/snippet}

{#snippet hint(text: string)}
  <p role="alert" class="rounded-md bg-muted p-2 text-xs">{text}</p>
{/snippet}

{#snippet test(label: string, step: "jogTest" | "frameTest" | "fireTest", body: Snippet)}
  <div class="space-y-2 rounded-md border p-2">
    <div class="flex items-center gap-2">
      {@render statusIcon(checklist.outcome(step), label)}
      <span class="flex-1 text-xs font-medium">{label}</span>
      {#if checklist.outcome(step) === "todo"}
        <Button
          size="sm"
          variant="ghost"
          class="h-6 px-2 text-xs"
          onclick={() => checklist.resolve(step, "skipped")}
        >
          Skip
        </Button>
      {/if}
    </div>
    {@render body()}
  </div>
{/snippet}

{#snippet connectedBody()}
  {#if machine.state.status === "connected"}
    <p>
      Connected on {machine.state.port.label}: {machine.state.machine.name}
      {machine.state.machine.version}, found by the connection handshake.
    </p>
  {:else}
    <p>Connect to the machine above. The checklist picks up from here.</p>
  {/if}
{/snippet}

{#snippet bedBody()}
  {#if profile}
    <p>
      The bed size and origin corner come from the controller's settings ($130, $131) and this
      profile. Check them against the machine: which corner does the head go to for X0 Y0?
    </p>
    <div class="flex flex-wrap items-start gap-3">
      <OriginDiagram {origin} />
      <div class="min-w-36 flex-1 space-y-2">
        <label class="block space-y-1">
          <span class="text-muted-foreground">Origin (X0 Y0)</span>
          <select class="{selectClass} w-full" bind:value={origin}>
            {#each ORIGINS as [value, text] (value)}
              <option {value}>{text}</option>
            {/each}
          </select>
        </label>
        <label class="flex items-center gap-2">
          <input type="checkbox" bind:checked={bedFromMachine} />
          Bed from max travel ($130 × $131)
        </label>
        <div class="flex items-center gap-1">
          <Input
            aria-label="Bed width"
            type="number"
            min="1"
            step="any"
            class="h-8 w-20"
            bind:value={width}
            disabled={bedFromMachine}
          />
          <span class="text-muted-foreground">×</span>
          <Input
            aria-label="Bed depth"
            type="number"
            min="1"
            step="any"
            class="h-8 w-20"
            bind:value={height}
            disabled={bedFromMachine}
          />
          <span class="text-muted-foreground">mm</span>
        </div>
      </div>
    </div>
    <div class="flex flex-wrap gap-2">
      <Button size="sm" onclick={saveBed}>Looks right</Button>
      <Button
        size="sm"
        variant="outline"
        disabled={!machine.connected || machineSettings.loading}
        onclick={() => machineSettings.read()}
      >
        Read $$ again
      </Button>
    </div>
  {:else}
    <p>Connect to the machine to read its settings and make its profile.</p>
  {/if}
{/snippet}

{#snippet laserModeBody()}
  {#if !settingsRead}
    <p>Connect to the machine to read laser mode ($32).</p>
  {:else if machineSettings.laserModeOff}
    <p>
      Laser mode is off ($32=0). GRBL then stops at the end of every move, so the laser lingers and
      burns dots at every corner. Turn it on before running jobs.
    </p>
    <Button size="sm" disabled={machineSettings.writing !== null} onclick={laserModeOn}>
      Write $32=1
    </Button>
  {:else}
    <p>Laser mode is on ($32=1): the laser keeps moving through corners.</p>
    <Button size="sm" onclick={() => checklist.resolve("laserMode", "done")}>Continue</Button>
  {/if}
{/snippet}

{#snippet homingBody()}
  {#if !settingsRead}
    <p>Connect to the machine to read whether homing is configured ($22).</p>
  {:else if control.homingEnabled}
    <p>
      Homing is on ($22=1). A test home runs the head to its limit switches and stops there. Keep
      hands clear of the gantry.
    </p>
    <Button
      size="sm"
      disabled={!control.canHome}
      title={control.canHome ? undefined : notReady}
      onclick={testHome}
    >
      {control.homing ? "Homing…" : "Test home"}
    </Button>
    {#if homeResult === "homed"}
      <p role="status">Homed: the machine found its switches.</p>
    {:else if homeResult === "failed"}
      {@render hint(
        `${control.error || "Homing failed."} Check the limit switches and the homing direction ($23), or skip this step.`,
      )}
    {/if}
  {:else}
    <p>
      Homing isn't configured ($22=0). Without it the machine can't find a fixed origin, so X0 Y0 is
      wherever the head was at power-on. Use "Start from: current position" or "user origin" for
      jobs rather than "absolute", or set the head at the origin corner before switching on.
    </p>
    <Button size="sm" variant="outline" onclick={() => checklist.resolve("homing", "skipped")}>
      Got it, skip
    </Button>
  {/if}
{/snippet}

{#snippet jogBody()}
  <p>Jog {JOG_TEST_MM} mm on each axis and say which way the head went.</p>
  <div class="flex flex-wrap gap-2">
    <Button
      size="sm"
      variant="outline"
      disabled={!control.canJog}
      title={control.canJog ? undefined : notReady}
      onclick={() => jogTest("x")}
    >
      Jog +X {JOG_TEST_MM} mm
    </Button>
    <Button
      size="sm"
      variant="outline"
      disabled={!control.canJog}
      title={control.canJog ? undefined : notReady}
      onclick={() => jogTest("y")}
    >
      Jog +Y {JOG_TEST_MM} mm
    </Button>
  </div>
  {#if jogged.x}
    {@render question(`Did the head move ${expected.x}?`, (yes) => sawJog("x", yes), jogSeen.x)}
    {#if jogSeen.x === false}
      {@render hint(
        "X runs the wrong way. Check the origin corner under Bed and origin; if that's right, the X axis is inverted: flip bit 0 (X) of $3, direction port invert, in Machine settings.",
      )}
    {/if}
  {/if}
  {#if jogged.y}
    {@render question(`Did the head move ${expected.y}?`, (yes) => sawJog("y", yes), jogSeen.y)}
    {#if jogSeen.y === false}
      {@render hint(
        "Y runs the wrong way. Check the origin corner under Bed and origin; if that's right, the Y axis is inverted: flip bit 1 (Y) of $3, direction port invert, in Machine settings.",
      )}
    {/if}
  {/if}
{/snippet}

{#snippet frameBody()}
  {#if profile}
    <p>
      Trace a {TEST_SQUARE.size} mm square {TEST_SQUARE.inset} mm in from the {cornerName(
        profile.origin,
      )} corner, {frameLaser}. On the canvas, that is the same corner of the bed.
    </p>
    <Button
      size="sm"
      variant="outline"
      disabled={!control.idle}
      title={control.idle ? undefined : notReady}
      onclick={frameSquare}
    >
      Frame test square
    </Button>
    {#if frameError}
      {@render hint(frameError)}
    {/if}
    {#if framed}
      {@render question(
        `Did the head trace a square near the ${cornerName(profile.origin)} corner of the bed?`,
        (yes) => seen("frameTest", yes),
        frameSeen,
      )}
      {#if frameSeen === false}
        {@render hint(
          "Check the origin corner and bed size under Bed and origin, and the jog test: an inverted axis or the wrong corner puts jobs in the wrong place.",
        )}
      {/if}
    {/if}
  {/if}
{/snippet}

{#snippet fireBody()}
  <div class="space-y-1 rounded-md border border-destructive/50 bg-destructive/10 p-2">
    <p class="font-medium text-destructive">Before the laser fires:</p>
    <ul class="list-disc space-y-0.5 pl-4">
      {#each EYE_SAFETY_REMINDER as line (line)}
        <li>{line}</li>
      {/each}
    </ul>
    <label class="flex items-center gap-2 pt-1 font-medium">
      <input type="checkbox" bind:checked={safetyChecked} />
      Done: glasses on, bed clear, switch in reach
    </label>
  </div>
  <Button
    size="sm"
    variant="destructive"
    disabled={!safetyChecked || !control.idle}
    title={control.idle ? undefined : notReady}
    onclick={testFire}
  >
    {control.firing ? "Firing…" : `Test fire ${control.power}% for ${control.durationMs} ms`}
  </Button>
  <p class="text-muted-foreground">Power and time are set on the jog pad (at most 10% for 1 s).</p>
  {#if fired}
    {@render question(
      "Did you see a short, faint dot of light?",
      (yes) => seen("fireTest", yes),
      fireSeen,
    )}
    {#if fireSeen === false}
      {@render hint(
        "Check the laser module's power and any enable switch or key, then that $30 (maximum S) matches the controller. Try a little more power on the jog pad.",
      )}
    {/if}
  {/if}
{/snippet}

{#snippet handsOnBody()}
  {@render test("Jog test", "jogTest", jogBody)}
  {@render test("Frame a test square", "frameTest", frameBody)}
  {@render test("Low-power test fire", "fireTest", fireBody)}
  {#if control.error}
    <p role="alert" class="text-destructive">{control.error}</p>
  {/if}
{/snippet}

{#snippet airBody()}
  <p>
    Turn air assist on ({profile?.airAssist ?? "M8"}) and off again (M9). Skip this if the machine
    has none.
  </p>
  <Button
    size="sm"
    variant="outline"
    disabled={!control.idle}
    title={control.idle ? undefined : notReady}
    onclick={toggleAir}
  >
    {control.airAssist ? "Turn air off" : "Turn air on"}
  </Button>
  {#if airToggled}
    {@render question(
      "Did the air come on, and stop when turned off?",
      (yes) => seen("airAssist", yes),
      airSeen,
    )}
    {#if airSeen === false}
      {@render hint(
        "Check the pump's power and hose. Some controllers switch air on M7 rather than M8: change the air-assist command in the machine profile.",
      )}
    {/if}
  {/if}
  {#if control.error}
    <p role="alert" class="text-destructive">{control.error}</p>
  {/if}
{/snippet}

{#if checklist.open}
  <section aria-label="Set up this machine" class="space-y-2 rounded-lg border p-2">
    <div class="flex items-center gap-2 px-1">
      <ListChecks class="size-4 shrink-0" />
      <h3 class="flex-1 text-sm font-medium">Set up this machine</h3>
      <Badge variant="outline">{checklist.completed} of {CHECKLIST.length}</Badge>
      <Button
        variant="ghost"
        size="icon"
        class="size-6"
        aria-label="Close setup checklist"
        onclick={() => checklist.dismiss()}
      >
        <X />
      </Button>
    </div>
    <ol class="space-y-1">
      {#each CHECKLIST as step (step)}
        {@const status = checklist.status(step)}
        {@const expanded = checklist.expanded === step}
        <li class="rounded-md {expanded ? 'bg-muted/40' : ''}">
          <button
            type="button"
            class="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left text-sm hover:bg-muted"
            aria-expanded={expanded}
            onclick={() => checklist.toggle(step)}
          >
            {@render statusIcon(status, TITLES[step])}
            <span class="flex-1 {status === 'skipped' ? 'text-muted-foreground' : ''}"
              >{TITLES[step]}</span
            >
            {#if status === "skipped"}<span class="text-xs text-muted-foreground">Skipped</span
              >{/if}
            {#if expanded}<ChevronDown class="size-4" />{:else}<ChevronRight class="size-4" />{/if}
          </button>
          {#if expanded}
            <div class="space-y-2 px-1 pt-1 pb-2 pl-7 text-xs">
              {#if step === "connected"}
                {@render connectedBody()}
              {:else if step === "bed"}
                {@render bedBody()}
              {:else if step === "laserMode"}
                {@render laserModeBody()}
              {:else if step === "homing"}
                {@render homingBody()}
              {:else if step === "handsOn"}
                {@render handsOnBody()}
              {:else}
                {@render airBody()}
              {/if}
              {#if status === "todo" && step !== "connected"}
                <Button
                  size="sm"
                  variant="ghost"
                  class="h-7 px-2 text-xs text-muted-foreground"
                  onclick={() => checklist.skip(step)}
                >
                  Skip {step === "handsOn" ? "the remaining tests" : "this step"}
                </Button>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ol>
    {#if checklist.finished}
      <div class="flex items-center gap-2 px-1 text-xs">
        <span class="flex-1">All done: this machine is set up.</span>
        <Button size="sm" onclick={() => checklist.dismiss()}>Close</Button>
      </div>
    {/if}
    {#if machineSettings.error}
      <p role="alert" class="px-1 text-xs text-destructive">{machineSettings.error}</p>
    {/if}
  </section>
{:else if checklist.available}
  <Button
    variant="ghost"
    size="sm"
    class="h-7 w-full justify-start px-2 text-xs text-muted-foreground"
    onclick={() => checklist.reopen()}
  >
    <ListChecks />
    {checklist.finished ? "Machine set up · review" : "Set up this machine…"}
    <span class="ml-auto tabular-nums">{checklist.completed} of {CHECKLIST.length}</span>
  </Button>
{/if}
