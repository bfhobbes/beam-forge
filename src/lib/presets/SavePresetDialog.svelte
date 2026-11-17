<script lang="ts">
  import { untrack } from "svelte";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import * as ToggleGroup from "#lib/components/ui/toggle-group/index.js";
  import type { CutLayer } from "#lib/document/model.js";
  import { presetLibrary } from "./library.svelte.js";
  import {
    defaultOperation,
    layerSettings,
    OPERATION_LABELS,
    operationsFor,
    presetName,
    settingsSummary,
    type MaterialPreset,
    type Operation,
  } from "./presets.js";

  /**
   * "Save layer as preset": names the layer's settings by material, thickness
   * and operation and adds them to the machine's library. `onsaved` links the
   * layer to the new preset.
   */
  let {
    open = $bindable(false),
    layer,
    onsaved,
  }: {
    open?: boolean;
    layer: CutLayer;
    onsaved: (preset: MaterialPreset) => void;
  } = $props();

  let material = $state("");
  let thickness = $state("");
  let operation = $state<Operation>("cut");
  let saving = $state(false);
  let problem = $state("");

  const operations = $derived(operationsFor(layer.mode));
  const thicknessValue = $derived(thickness.trim() === "" ? null : Number(thickness));
  const draft = $derived<MaterialPreset>({
    id: "",
    material: material.trim(),
    thickness: thicknessValue,
    settings: layerSettings(layer, operation),
    starter: false,
    notes: "",
  });

  /** Starts from the layer's current preset, if it has one in the library. */
  function reset() {
    const current = presetLibrary.find(layer.preset?.id);
    material = current?.material ?? "";
    thickness = current?.thickness === null || !current ? "" : String(current.thickness);
    operation = defaultOperation(layer);
    problem = "";
  }

  // Each opening starts afresh from the layer.
  $effect(() => {
    if (open) untrack(reset);
  });

  async function save(event: SubmitEvent) {
    event.preventDefault();
    if (!draft.material) {
      problem = "Name the material";
      return;
    }
    if (thicknessValue !== null && !(Number.isFinite(thicknessValue) && thicknessValue > 0)) {
      problem = "Thickness is a number of millimetres above zero, or empty";
      return;
    }
    saving = true;
    const saved = await presetLibrary.save(draft);
    saving = false;
    if (!saved) {
      problem = presetLibrary.error;
      return;
    }
    onsaved(saved);
    open = false;
  }
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>Save layer as preset</Dialog.Title>
      <Dialog.Description>
        Adds {layer.name}'s settings to this machine's material library.
      </Dialog.Description>
    </Dialog.Header>
    <form class="flex flex-col gap-3" onsubmit={save}>
      <label class="flex flex-col gap-1 text-sm">
        Material
        <Input bind:value={material} placeholder="Plywood" autocomplete="off" />
      </label>
      <label class="flex flex-col gap-1 text-sm">
        Thickness (mm, optional)
        <Input bind:value={thickness} inputmode="decimal" placeholder="3" autocomplete="off" />
      </label>
      <div class="flex flex-col gap-1 text-sm">
        <span id="preset-operation">Operation</span>
        <ToggleGroup.Root
          type="single"
          variant="outline"
          size="sm"
          class="w-full"
          aria-labelledby="preset-operation"
          bind:value={() => operation, (next) => next && (operation = next as Operation)}
        >
          {#each operations as option (option)}
            <ToggleGroup.Item value={option} class="flex-1">
              {OPERATION_LABELS[option]}
            </ToggleGroup.Item>
          {/each}
        </ToggleGroup.Root>
      </div>
      <p class="text-xs text-muted-foreground">
        {draft.material ? presetName(draft) : "New preset"}: {settingsSummary(draft.settings)}
      </p>
      {#if problem}<p role="alert" class="text-sm text-destructive">{problem}</p>{/if}
      <Dialog.Footer>
        <Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
        <Button type="submit" disabled={saving}>{saving ? "Saving…" : "Save preset"}</Button>
      </Dialog.Footer>
    </form>
  </Dialog.Content>
</Dialog.Root>
