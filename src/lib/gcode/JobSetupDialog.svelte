<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { CircleCheck, Frame, OctagonX, TriangleAlert, X } from "@lucide/svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import type { JobAnchor } from "#lib/bindings/gcode/JobAnchor.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import { machine } from "#lib/machine/connection.svelte.js";
  import { machineJob } from "#lib/machine/job.svelte.js";
  import { blocked, preflightPrefs as prefs } from "#lib/machine/preflight.svelte.js";
  import { machineSettings } from "#lib/machine/settings.svelte.js";
  import { formatPosition, mm } from "#lib/machine/status.js";
  import { ANCHORS, START_FROM, frameLaserLabel, jobSetup as setup } from "./job-setup.svelte.js";

  /**
   * Mount once per window: the Export GCode and Start job actions, the job
   * dialog they open (start from, job origin, bounds check), and the export
   * notice.
   */
  onMount(() =>
    actions.register(
      {
        id: "file.export-gcode",
        title: "Export GCode…",
        group: "File",
        keywords: ["gcode", "nc", "save", "job", "grbl"],
        enabled: () => setup.available,
        hint: "GCode export needs the desktop app",
        run: () => setup.open("export"),
      },
      {
        id: "machine.run",
        title: "Start job…",
        group: "Machine",
        keywords: ["run", "send", "cut", "engrave", "gcode"],
        enabled: () => setup.available && machine.connected && !machineJob.active,
        hint: "Connect a machine, with no job running, to start a job",
        run: () => setup.open("run"),
      },
      {
        id: "machine.frame",
        title: "Frame job",
        group: "Machine",
        keywords: ["frame", "outline", "trace", "bounds", "preview", "position"],
        enabled: () => setup.canFrame,
        hint: "Connect a machine, with no job running, to frame the job",
        run: () => void setup.frame(),
      },
    ),
  );

  // Regenerate the summary when anything it depends on changes.
  const position = $derived(machine.status?.workPosition ?? null);
  const positionKey = $derived(position ? `${position.x},${position.y}` : "");
  $effect(() => {
    if (!setup.mode) return;
    void [editor.doc, machineSettings.profile, machine.connected, positionKey];
    untrack(() => void setup.refresh());
  });

  // Export notices fade on their own.
  $effect(() => {
    if (!setup.notice) return;
    const timer = setTimeout(() => (setup.notice = ""), 6000);
    return () => clearTimeout(timer);
  });

  const profile = $derived(machineSettings.profile);
  const summary = $derived(setup.summary);
  const exporting = $derived(setup.mode === "export");
  const checks = $derived(setup.checklist);
  const checksBlocked = $derived(checks !== null && blocked(checks));
  const checksWarn = $derived(checks?.some((item) => item.level === "warn") ?? false);
  const confirmLabel = $derived.by(() => {
    const anyway = summary && !summary.insideBed;
    if (exporting) return anyway ? "Export anyway…" : "Export…";
    return anyway ? "Start anyway" : "Start job";
  });

  const ANCHOR_LABELS: Record<JobAnchor, string> = {
    topLeft: "Top left",
    top: "Top",
    topRight: "Top right",
    left: "Left",
    center: "Center",
    right: "Right",
    bottomLeft: "Bottom left",
    bottom: "Bottom",
    bottomRight: "Bottom right",
  };
</script>

<Dialog.Root
  open={setup.mode !== null}
  onOpenChange={(open) => {
    if (!open) setup.close();
  }}
>
  <Dialog.Content class="sm:max-w-md">
    {#if checks}
      <Dialog.Header>
        <Dialog.Title>Before you start</Dialog.Title>
        <Dialog.Description>
          {checksBlocked
            ? "Fix what's marked before the job can start."
            : checksWarn
              ? "Check what's marked, then start the job."
              : "Everything checks out."}
        </Dialog.Description>
      </Dialog.Header>

      <ul aria-label="Pre-run checks" class="space-y-2 text-sm">
        {#each checks as item (item.id)}
          <li class="flex items-start gap-2">
            {#if item.level === "pass"}
              <CircleCheck
                class="mt-0.5 size-4 shrink-0 text-muted-foreground"
                aria-label="Passed"
              />
            {:else if item.level === "warn"}
              <TriangleAlert
                class="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-400"
                aria-label="Warning"
              />
            {:else}
              <OctagonX
                class="mt-0.5 size-4 shrink-0 text-destructive"
                aria-label="Blocks the job"
              />
            {/if}
            <div class="min-w-0 space-y-1">
              <p class={item.level === "pass" ? "text-muted-foreground" : "font-medium"}>
                {item.title}
              </p>
              {#if item.detail}
                <p class="text-xs text-muted-foreground">{item.detail}</p>
              {/if}
              {#if item.id === "framed" && item.level !== "pass"}
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!setup.canFrame}
                  title={frameLaserLabel(profile)}
                  onclick={() => void setup.frame()}
                >
                  <Frame />Frame now
                </Button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>

      <div class="space-y-1 text-xs text-muted-foreground">
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            class="size-3.5 accent-primary"
            checked={prefs.alwaysShow}
            onchange={(event) => prefs.setAlwaysShow(event.currentTarget.checked)}
          />
          Show this checklist even when everything passes
        </label>
        <label class="flex items-center gap-2">
          <input
            type="checkbox"
            class="size-3.5 accent-primary"
            checked={prefs.askFramed}
            onchange={(event) => prefs.setAskFramed(event.currentTarget.checked)}
          />
          Ask whether the job was framed
        </label>
      </div>
      {#if setup.error}
        <p role="alert" class="text-sm text-destructive">{setup.error}</p>
      {/if}

      <Dialog.Footer>
        <Button variant="outline" disabled={setup.busy} onclick={() => setup.back()}>Back</Button>
        <Button
          variant={checksWarn ? "destructive" : "default"}
          disabled={!setup.canConfirm}
          onclick={() => void setup.confirm()}
        >
          {checksWarn ? "Start anyway" : "Start job"}
        </Button>
      </Dialog.Footer>
    {:else}
      <Dialog.Header>
        <Dialog.Title>{exporting ? "Export GCode" : "Start job"}</Dialog.Title>
        <Dialog.Description>
          {#if profile}
            GCode for {profile.name}.
          {:else}
            No machine profile yet, so this uses GRBL defaults ($30=1000, 200 × 200 mm bed). Connect
            once to read the machine's settings.
          {/if}
        </Dialog.Description>
      </Dialog.Header>

      <div class="grid grid-cols-[1fr_auto] gap-4 text-sm">
        <fieldset class="space-y-1.5">
          <legend class="mb-1.5 font-medium">Start from</legend>
          {#each START_FROM as option (option.value)}
            {@const why = setup.unavailable(option.value)}
            <label class="flex items-start gap-2" title={why ?? undefined}>
              <input
                type="radio"
                name="start-from"
                class="mt-0.5 size-3.5 accent-primary"
                checked={setup.placement.startFrom === option.value}
                onchange={() => setup.setStartFrom(option.value)}
              />
              <span class={why ? "text-muted-foreground" : undefined}>{option.label}</span>
            </label>
          {/each}
        </fieldset>

        <fieldset class="space-y-1.5" disabled={!setup.anchored}>
          <legend class="mb-1.5 font-medium">Job origin</legend>
          <div
            class="grid w-fit grid-cols-3 gap-1"
            title={setup.anchored
              ? undefined
              : "Used when starting from the current position or user origin"}
          >
            {#each ANCHORS as anchor (anchor)}
              {@const selected = setup.placement.anchor === anchor}
              <button
                type="button"
                aria-label={ANCHOR_LABELS[anchor]}
                aria-pressed={selected}
                class={[
                  "grid size-6 place-items-center rounded-sm border transition-colors disabled:opacity-40",
                  selected ? "border-primary bg-primary" : "hover:bg-muted",
                ]}
                onclick={() => setup.setAnchor(anchor)}
              >
                <span
                  class={[
                    "size-1.5 rounded-full",
                    selected ? "bg-primary-foreground" : "bg-muted-foreground",
                  ]}
                ></span>
              </button>
            {/each}
          </div>
        </fieldset>
      </div>

      <div class="space-y-1 text-xs text-muted-foreground">
        <p>
          Head: {machine.connected ? formatPosition(position) : "not connected"}
        </p>
        <div class="flex items-center justify-between gap-2">
          <span>
            User origin: {setup.userOrigin
              ? `X ${mm(setup.userOrigin.x)} Y ${mm(setup.userOrigin.y)}`
              : "not set"}
          </span>
          <Button
            size="sm"
            variant="outline"
            disabled={!setup.canSetOrigin}
            title={setup.canSetOrigin ? undefined : "Connect to a machine to set the user origin."}
            onclick={() => void setup.setUserOrigin()}
          >
            Set user origin here
          </Button>
        </div>
      </div>

      {#if setup.problem}
        <p role="status" class="text-sm text-muted-foreground">{setup.problem}</p>
      {:else if summary}
        <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-sm tabular-nums">
          <dt class="text-muted-foreground">Bounds</dt>
          <dd>
            X {mm(summary.bounds.min.x)} Y {mm(summary.bounds.min.y)} to X {mm(
              summary.bounds.max.x,
            )}
            Y {mm(summary.bounds.max.y)}
          </dd>
          <dt class="text-muted-foreground">Size</dt>
          <dd>
            {mm(summary.bounds.max.x - summary.bounds.min.x)} × {mm(
              summary.bounds.max.y - summary.bounds.min.y,
            )} mm
          </dd>
          <dt class="text-muted-foreground">Cut</dt>
          <dd>{Math.round(summary.cutLength)} mm, {summary.lines} lines</dd>
        </dl>
        {#if summary.warning}
          <p
            role="alert"
            class="flex items-start gap-2 rounded-md border border-destructive/50 p-2 text-sm text-destructive"
          >
            <TriangleAlert class="mt-0.5 size-4 shrink-0" />{summary.warning}
          </p>
        {/if}
      {/if}

      {#if !exporting && !machine.connected}
        <p class="text-sm text-muted-foreground">Connect to a machine to start the job.</p>
      {:else if !exporting}
        <div class="space-y-1.5 rounded-md border p-2 text-sm">
          <div class="flex items-center gap-3">
            <Button
              size="sm"
              variant="outline"
              disabled={!setup.canFrame}
              title="Trace the job's outline on the bed, placed as above"
              onclick={() => void setup.frame()}
            >
              <Frame />Frame
            </Button>
            <label class="flex items-center gap-2">
              <input
                type="checkbox"
                class="size-3.5 accent-primary"
                checked={setup.frameShape === "rubberBand"}
                onchange={(event) =>
                  setup.setFrameShape(event.currentTarget.checked ? "rubberBand" : "box")}
              />
              Rubber band
            </label>
          </div>
          <p
            class={[
              "text-xs",
              profile && profile.framePower > 0
                ? "font-medium text-destructive"
                : "text-muted-foreground",
            ]}
          >
            {frameLaserLabel(profile)}. Change it in the machine profile.
          </p>
        </div>
      {/if}
      {#if setup.error}
        <p role="alert" class="text-sm text-destructive">{setup.error}</p>
      {/if}

      <Dialog.Footer>
        <Button variant="outline" disabled={setup.busy} onclick={() => setup.close()}>
          Cancel
        </Button>
        <Button
          variant={summary && !summary.insideBed ? "destructive" : "default"}
          disabled={!setup.canConfirm}
          onclick={() => void setup.confirm()}
        >
          {confirmLabel}
        </Button>
      </Dialog.Footer>
    {/if}
  </Dialog.Content>
</Dialog.Root>

{#if setup.notice}
  <div
    role="status"
    class="fixed bottom-10 left-16 z-40 flex max-w-md items-start gap-2 rounded-md border bg-popover px-3 py-2 text-sm text-popover-foreground shadow-md"
  >
    <span class="min-w-0 flex-1 break-all">{setup.notice}</span>
    <Button
      variant="ghost"
      size="icon-xs"
      aria-label="Dismiss"
      class="-mr-1"
      onclick={() => (setup.notice = "")}
    >
      <X />
    </Button>
  </div>
{/if}
