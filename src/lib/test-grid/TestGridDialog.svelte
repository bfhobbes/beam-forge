<script lang="ts">
  import { onMount, untrack } from "svelte";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import * as ToggleGroup from "#lib/components/ui/toggle-group/index.js";
  import { DocumentError, editLayer } from "#lib/document/commands.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import type { CutLayer, LayerMode } from "#lib/document/model.js";
  import { machineSettings } from "#lib/machine/settings.svelte.js";
  import { presetLibrary } from "#lib/presets/library.svelte.js";
  import { presetName, type MaterialPreset } from "#lib/presets/presets.js";
  import SavePresetDialog from "#lib/presets/SavePresetDialog.svelte";
  import NumberField from "#lib/shell/NumberField.svelte";
  import { textEditing } from "#lib/text/text-editing.svelte.js";
  import { testGrid } from "./generator.svelte.js";
  import type { TestGridSettings } from "./test-grid.js";

  /**
   * Mount once per window: the "Material test grid" actions and their
   * dialog. The settings view lays out a speed × power grid and adds it to
   * the document; the pick view turns one of the latest grid's cells into a
   * material preset.
   */
  onMount(() =>
    actions.register(
      {
        id: "machine.test-grid",
        title: "Material test grid…",
        group: "Machine",
        keywords: ["material", "test", "grid", "matrix", "calibrate", "speed", "power"],
        enabled: () => testGrid.available,
        hint: "Test grids need the desktop app",
        run: () => void testGrid.open(textEditing.defaults.fontFamily),
      },
      {
        id: "machine.test-grid-preset",
        title: "Save test-grid cell as preset…",
        group: "Machine",
        keywords: ["material", "test", "grid", "cell", "preset", "library"],
        enabled: () => testGrid.cells !== null,
        hint: "Add a material test grid first",
        run: () => testGrid.pick(),
      },
    ),
  );

  const MAX_STEPS = 20;
  const settings = $derived(testGrid.settings);
  const bed = $derived(machineSettings.bed);
  const cells = $derived(testGrid.view === "pick" ? testGrid.cells : null);

  // A different machine has a different bed: check the grid against it.
  $effect(() => {
    void bed;
    if (testGrid.view === "settings") untrack(() => void testGrid.refresh());
  });

  // Saving goes to the active machine's library.
  $effect(() => {
    void presetLibrary.follow(machineSettings.profile?.id ?? null);
  });

  let saving = $state<CutLayer | null>(null);
  let notice = $state("");

  function mm(value: number): string {
    return String(Number(value.toFixed(1)));
  }

  /** Accepts finite numbers; Rust says what's wrong with the rest. */
  const finite = (set: (value: number) => void) => (value: number) => {
    if (!Number.isFinite(value)) return false;
    set(value);
    return true;
  };

  /** Accepts whole numbers from 1 to `max`. */
  const whole =
    (set: (value: number) => void, max = Infinity) =>
    (value: number) => {
      if (!Number.isInteger(value) || value < 1 || value > max) return false;
      set(value);
      return true;
    };

  function change(changes: Partial<TestGridSettings>) {
    void testGrid.update(changes);
  }

  function pickCell(layer: CutLayer) {
    notice = "";
    saving = layer;
  }

  /** Links the cell's layer to the preset it was saved as. */
  function saved(layer: CutLayer, preset: MaterialPreset) {
    const link = { id: preset.id, name: presetName(preset), settings: preset.settings };
    try {
      editor.execute(editLayer(layer.id, { preset: link }));
    } catch (cause) {
      if (!(cause instanceof DocumentError)) throw cause;
    }
    notice = `Saved ${layer.name} as ${presetName(preset)}.`;
  }

  /** The speed of a row and power of a column, from any cell still there. */
  function rowSpeed(row: number): string {
    const layer = cells?.cells[row].find((cell) => cell !== null);
    return layer ? mm(layer.speed) : "";
  }

  function columnPower(column: number): string {
    const layer = cells?.cells.map((row) => row[column]).find((cell) => cell);
    return layer ? `${mm(layer.maxPower)}%` : "";
  }
</script>

<Dialog.Root
  open={testGrid.view !== null}
  onOpenChange={(open) => {
    if (!open) {
      testGrid.close();
      notice = "";
    }
  }}
>
  <Dialog.Content class="sm:max-w-2xl">
    {#if testGrid.view === "settings"}
      <Dialog.Header>
        <Dialog.Title>Material test grid</Dialog.Title>
        <Dialog.Description>
          A grid of squares, each on its own layer with its own speed and power, labelled on a
          low-power score layer. Burn it on a scrap of the material to find the settings that work.
        </Dialog.Description>
      </Dialog.Header>

      <form
        class="grid gap-x-6 gap-y-4 sm:grid-cols-2"
        aria-label="Test grid settings"
        onsubmit={(event) => {
          event.preventDefault();
          void testGrid.add();
        }}
      >
        <fieldset class="flex flex-col gap-1.5">
          <legend class="mb-1.5 text-sm font-medium">Speed, one row each</legend>
          <NumberField
            wide
            label="From"
            unit="mm/min"
            digits={0}
            value={settings.speed.from}
            oncommit={finite((from) => change({ speed: { ...settings.speed, from } }))}
          />
          <NumberField
            wide
            label="To"
            unit="mm/min"
            digits={0}
            value={settings.speed.to}
            oncommit={finite((to) => change({ speed: { ...settings.speed, to } }))}
          />
          <NumberField
            wide
            label="Steps"
            unit=""
            digits={0}
            value={settings.speed.steps}
            oncommit={whole((steps) => change({ speed: { ...settings.speed, steps } }), MAX_STEPS)}
          />
        </fieldset>

        <fieldset class="flex flex-col gap-1.5">
          <legend class="mb-1.5 text-sm font-medium">Power, one column each</legend>
          <NumberField
            wide
            label="From"
            unit="%"
            digits={1}
            value={settings.power.from}
            oncommit={finite((from) => change({ power: { ...settings.power, from } }))}
          />
          <NumberField
            wide
            label="To"
            unit="%"
            digits={1}
            value={settings.power.to}
            oncommit={finite((to) => change({ power: { ...settings.power, to } }))}
          />
          <NumberField
            wide
            label="Steps"
            unit=""
            digits={0}
            value={settings.power.steps}
            oncommit={whole((steps) => change({ power: { ...settings.power, steps } }), MAX_STEPS)}
          />
        </fieldset>

        <fieldset class="flex flex-col gap-1.5">
          <legend class="mb-1.5 text-sm font-medium">Cells</legend>
          <div class="flex items-center gap-1.5 text-xs">
            <span id="test-grid-mode" class="w-20 shrink-0 text-muted-foreground">Mode</span>
            <ToggleGroup.Root
              type="single"
              variant="outline"
              size="sm"
              class="flex-1"
              aria-labelledby="test-grid-mode"
              bind:value={
                () => settings.mode, (mode) => mode && change({ mode: mode as LayerMode })
              }
            >
              <ToggleGroup.Item value="fill" class="flex-1">Fill</ToggleGroup.Item>
              <ToggleGroup.Item value="line" class="flex-1">Line</ToggleGroup.Item>
            </ToggleGroup.Root>
          </div>
          <NumberField
            wide
            label="Size"
            unit="mm"
            value={settings.cellSize}
            oncommit={finite((cellSize) => change({ cellSize }))}
          />
          <NumberField
            wide
            label="Gap"
            unit="mm"
            value={settings.gap}
            oncommit={finite((gap) => change({ gap }))}
          />
          {#if settings.mode === "fill"}
            <NumberField
              wide
              label="Interval"
              unit="mm"
              value={settings.lineInterval}
              oncommit={finite((lineInterval) => change({ lineInterval }))}
            />
          {/if}
          <NumberField
            wide
            label="Passes"
            unit=""
            digits={0}
            value={settings.passes}
            oncommit={whole((passes) => change({ passes }))}
          />
          <label class="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              class="size-3.5 accent-primary"
              checked={settings.airAssist}
              onchange={(event) => change({ airAssist: event.currentTarget.checked })}
            />
            Air assist
          </label>
        </fieldset>

        <div class="flex flex-col gap-4">
          <fieldset class="flex flex-col gap-1.5">
            <legend class="mb-1.5 text-sm font-medium">Labels, scored first</legend>
            <NumberField
              wide
              label="Height"
              unit="mm"
              value={settings.labels.height}
              oncommit={finite((height) => change({ labels: { ...settings.labels, height } }))}
            />
            <NumberField
              wide
              label="Speed"
              unit="mm/min"
              digits={0}
              value={settings.labels.speed}
              oncommit={finite((speed) => change({ labels: { ...settings.labels, speed } }))}
            />
            <NumberField
              wide
              label="Power"
              unit="%"
              digits={1}
              value={settings.labels.power}
              oncommit={finite((power) => change({ labels: { ...settings.labels, power } }))}
            />
          </fieldset>
          <fieldset class="flex flex-col gap-1.5">
            <legend class="mb-1.5 text-sm font-medium">Bottom-left corner on the bed</legend>
            <NumberField
              wide
              label="X"
              unit="mm"
              value={settings.origin.x}
              oncommit={finite((x) => change({ origin: { ...settings.origin, x } }))}
            />
            <NumberField
              wide
              label="Y"
              unit="mm"
              value={settings.origin.y}
              oncommit={finite((y) => change({ origin: { ...settings.origin, y } }))}
            />
          </fieldset>
        </div>

        <div class="space-y-1 sm:col-span-2">
          {#if testGrid.problem}
            <p
              role="alert"
              class="flex items-start gap-2 rounded-md border border-destructive/50 p-2 text-sm text-destructive"
            >
              <TriangleAlert class="mt-0.5 size-4 shrink-0" />{testGrid.problem}
            </p>
          {:else if testGrid.grid}
            <p role="status" class="text-sm text-muted-foreground">
              {settings.speed.steps * settings.power.steps} cells and their labels, {mm(
                testGrid.grid.width,
              )} × {mm(testGrid.grid.height)} mm, fit on the {mm(bed.width)} × {mm(bed.height)} mm bed.
            </p>
            {#if testGrid.grid.labelFont !== settings.labels.fontFamily}
              <p class="text-xs text-muted-foreground">
                {settings.labels.fontFamily} isn't installed, so the labels use {testGrid.grid
                  .labelFont}.
              </p>
            {/if}
          {:else}
            <p role="status" class="text-sm text-muted-foreground">Laying out the grid…</p>
          {/if}
        </div>

        <Dialog.Footer class="sm:col-span-2">
          <Button type="button" variant="ghost" class="mr-auto" onclick={() => testGrid.reset()}>
            Reset
          </Button>
          <Button type="button" variant="outline" onclick={() => testGrid.close()}>Cancel</Button>
          <Button type="submit" disabled={!testGrid.grid}>Add to design</Button>
        </Dialog.Footer>
      </form>
    {:else if testGrid.view === "pick"}
      <Dialog.Header>
        <Dialog.Title>Save a test cell as a preset</Dialog.Title>
        <Dialog.Description>
          {testGrid.added
            ? "Added the grid. Burn it, then pick the cell that came out best: its settings become a material preset for this machine."
            : "Pick the cell that came out best: its settings become a material preset for this machine."}
        </Dialog.Description>
      </Dialog.Header>

      {#if cells}
        <div class="overflow-x-auto">
          <table class="mx-auto border-separate border-spacing-1 text-xs tabular-nums">
            <thead>
              <tr>
                <th scope="col" class="text-left font-normal text-muted-foreground">mm/min</th>
                {#each { length: cells.columns } as _, column (column)}
                  <th scope="col" class="font-normal text-muted-foreground">
                    {columnPower(column)}
                  </th>
                {/each}
              </tr>
            </thead>
            <tbody>
              {#each cells.cells as row, r (r)}
                <tr>
                  <th scope="row" class="pr-1 text-right font-normal text-muted-foreground">
                    {rowSpeed(r)}
                  </th>
                  {#each row as cell, column (column)}
                    <td>
                      {#if cell}
                        <button
                          type="button"
                          class="grid size-9 place-items-center rounded-sm border transition-colors hover:border-primary hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none disabled:opacity-40"
                          aria-label={`Save ${cell.name} as a preset`}
                          title={cell.preset
                            ? `${cell.name}: saved as ${cell.preset.name}`
                            : cell.name}
                          disabled={!presetLibrary.available}
                          onclick={() => pickCell(cell)}
                        >
                          <span
                            class={["size-3 rounded-full", cell.preset && "ring-2 ring-primary"]}
                            style:background-color={cell.color}
                          ></span>
                        </button>
                      {/if}
                    </td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {:else}
        <p role="status" class="text-sm text-muted-foreground">
          The design has no test grid. Add one first.
        </p>
      {/if}

      {#if !presetLibrary.available}
        <p class="text-sm text-muted-foreground">
          Browser preview: the material library is stored by the desktop app.
        </p>
      {:else if presetLibrary.error}
        <p role="alert" class="text-sm text-destructive">{presetLibrary.error}</p>
      {:else if notice}
        <p role="status" class="text-sm text-muted-foreground">{notice}</p>
      {/if}

      <Dialog.Footer>
        {#if testGrid.available}
          <Button
            variant="outline"
            class="mr-auto"
            onclick={() => void testGrid.open(textEditing.defaults.fontFamily)}
          >
            New grid…
          </Button>
        {/if}
        <Button onclick={() => testGrid.close()}>Done</Button>
      </Dialog.Footer>

      {#if saving}
        {@const layer = saving}
        <SavePresetDialog
          bind:open={() => saving !== null, (open) => !open && (saving = null)}
          {layer}
          onsaved={(preset) => saved(layer, preset)}
        />
      {/if}
    {/if}
  </Dialog.Content>
</Dialog.Root>
