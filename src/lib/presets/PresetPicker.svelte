<script lang="ts">
  import BookmarkPlusIcon from "@lucide/svelte/icons/bookmark-plus";
  import ChevronsUpDown from "@lucide/svelte/icons/chevrons-up-down";
  import DownloadIcon from "@lucide/svelte/icons/download";
  import SaveIcon from "@lucide/svelte/icons/save";
  import Trash2Icon from "@lucide/svelte/icons/trash-2";
  import UploadIcon from "@lucide/svelte/icons/upload";
  import { Badge } from "#lib/components/ui/badge/index.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Command from "#lib/components/ui/command/index.js";
  import * as Popover from "#lib/components/ui/popover/index.js";
  import type { LayerChanges } from "#lib/document/layers.js";
  import type { CutLayer } from "#lib/document/model.js";
  import { machineSettings } from "#lib/machine/settings.svelte.js";
  import { presetLibrary } from "./library.svelte.js";
  import {
    applyPreset,
    defaultOperation,
    layerSettings,
    presetModified,
    presetName,
    searchPresets,
    settingsSummary,
    type MaterialPreset,
  } from "./presets.js";
  import SavePresetDialog from "./SavePresetDialog.svelte";

  /**
   * The layer's material preset: a searchable list of the machine's library
   * to apply from, which preset the layer came from and whether it has been
   * changed since, and the library actions (save the layer as a preset,
   * update its preset, export and import).
   * `onedit` makes an undoable layer edit and says whether it was made.
   */
  let { layer, onedit }: { layer: CutLayer; onedit: (changes: LayerChanges) => boolean } = $props();

  let open = $state(false);
  let saving = $state(false);
  let query = $state("");
  let notice = $state("");

  const profile = $derived(machineSettings.profile);
  $effect(() => {
    void presetLibrary.follow(profile?.id ?? null);
  });

  const matches = $derived(searchPresets(presetLibrary.presets, query));
  const modified = $derived(presetModified(layer));
  const linked = $derived(presetLibrary.find(layer.preset?.id));
  const starter = $derived(linked?.starter === true && !modified);
  const modifiedHint = $derived(
    layer.preset
      ? `Changed since ${layer.preset.name} was applied (${settingsSummary(layer.preset.settings)})`
      : "",
  );

  function close() {
    open = false;
    query = "";
  }

  function saveAs() {
    close();
    saving = true;
  }

  function apply(preset: MaterialPreset) {
    close();
    if (onedit(applyPreset(preset))) notice = "";
  }

  /** Links the layer to `preset` as saved, without changing its settings. */
  function link(preset: MaterialPreset) {
    onedit({ preset: { id: preset.id, name: presetName(preset), settings: preset.settings } });
  }

  async function update() {
    close();
    if (!linked) return;
    const saved = await presetLibrary.save({
      ...linked,
      settings: layerSettings(layer, defaultOperation(layer)),
      starter: false,
    });
    if (saved) {
      link(saved);
      notice = `Updated ${presetName(saved)}.`;
    }
  }

  async function remove() {
    close();
    if (!linked) return;
    const name = presetName(linked);
    if (await presetLibrary.remove(linked.id)) notice = `Deleted ${name} from the library.`;
  }

  async function exportLibrary() {
    close();
    const name = `${profile?.name ?? "Beam Forge"} presets`;
    const path = await presetLibrary.exportLibrary(name);
    if (path) notice = `Exported the library to ${path}.`;
  }

  async function importLibrary() {
    close();
    const result = await presetLibrary.importLibrary();
    if (!result) return;
    const plural = (count: number) => `${count} preset${count === 1 ? "" : "s"}`;
    notice = `Imported ${plural(result.added)} and replaced ${plural(result.replaced)}.`;
  }
</script>

<section aria-label="Material preset" class="flex flex-col gap-1.5">
  <div class="flex items-center gap-1">
    <Popover.Root bind:open onOpenChange={(next) => !next && (query = "")}>
      <Popover.Trigger>
        {#snippet child({ props })}
          <Button
            {...props}
            variant="outline"
            size="sm"
            role="combobox"
            aria-expanded={open}
            aria-label="Material preset"
            class="h-7 min-w-0 flex-1 justify-between px-2 text-xs font-normal"
          >
            <span class={["truncate", !layer.preset && "text-muted-foreground"]}>
              {layer.preset?.name ?? "Apply a material preset…"}
            </span>
            <ChevronsUpDown class="opacity-50" />
          </Button>
        {/snippet}
      </Popover.Trigger>
      <Popover.Content align="start" class="w-80 p-0">
        <Command.Root shouldFilter={false}>
          <Command.Input placeholder="Search presets…" bind:value={query} />
          <Command.List class="max-h-96">
            {#if !presetLibrary.available}
              <p role="status" class="p-3 text-xs text-muted-foreground">
                Browser preview: the material library is stored by the desktop app.
              </p>
            {:else if !presetLibrary.loaded && !presetLibrary.error}
              <p role="status" class="p-3 text-xs text-muted-foreground">Loading presets…</p>
            {:else}
              <Command.Empty>No preset matches “{query}”.</Command.Empty>
              <Command.Group heading={profile ? `Presets for ${profile.name}` : "Presets"}>
                {#each matches as preset (preset.id)}
                  <Command.Item value={preset.id} onSelect={() => apply(preset)}>
                    <span class="flex min-w-0 flex-1 flex-col">
                      <span class="truncate">{presetName(preset)}</span>
                      <span class="truncate text-xs text-muted-foreground">
                        {settingsSummary(preset.settings)}
                      </span>
                    </span>
                    {#if preset.starter}
                      <Badge variant="outline" title={preset.notes}>Verify</Badge>
                    {/if}
                  </Command.Item>
                {/each}
              </Command.Group>
            {/if}
            {#if presetLibrary.available}
              <Command.Separator />
              <Command.Group heading="Library">
                <Command.Item value="save-as" onSelect={saveAs}>
                  <BookmarkPlusIcon />Save layer as preset…
                </Command.Item>
                {#if linked && modified}
                  <Command.Item value="update" onSelect={update}>
                    <SaveIcon />
                    <span class="truncate">Update {presetName(linked)} from this layer</span>
                  </Command.Item>
                {/if}
                {#if linked}
                  <Command.Item value="delete" onSelect={remove}>
                    <Trash2Icon />
                    <span class="truncate">Delete {presetName(linked)}</span>
                  </Command.Item>
                {/if}
                <Command.Item value="export" onSelect={exportLibrary}>
                  <DownloadIcon />Export library…
                </Command.Item>
                <Command.Item value="import" onSelect={importLibrary}>
                  <UploadIcon />Import library…
                </Command.Item>
              </Command.Group>
            {/if}
          </Command.List>
        </Command.Root>
      </Popover.Content>
    </Popover.Root>
    {#if modified}
      <Badge variant="secondary" title={modifiedHint}>Modified</Badge>
    {/if}
  </div>
  {#if starter}
    <p class="text-xs text-muted-foreground">
      Starter preset: a starting point for a 20 W diode, not a tested value. Run a test cut first.
    </p>
  {/if}
  {#if presetLibrary.error}
    <p role="alert" class="text-xs text-destructive">{presetLibrary.error}</p>
  {:else if notice}
    <p role="status" class="text-xs text-muted-foreground">{notice}</p>
  {/if}
</section>

<SavePresetDialog bind:open={saving} {layer} onsaved={link} />
