<script lang="ts">
  import type { Snippet } from "svelte";
  import * as Tabs from "#lib/components/ui/tabs/index.js";
  import LayersPanel from "./LayersPanel.svelte";
  import SelectionProperties from "./SelectionProperties.svelte";

  /**
   * `machine` fills the Machine tab: the connection panel and machine
   * controls. `tab` is the open tab, bindable so actions can switch to one.
   */
  let { machine, tab = $bindable("properties") }: { machine: Snippet; tab?: string } = $props();
</script>

<aside aria-label="Inspector" class="flex h-full flex-col bg-background">
  <Tabs.Root bind:value={tab} class="flex min-h-0 flex-1 flex-col gap-0">
    <div class="border-b p-1.5">
      <Tabs.List class="w-full">
        <Tabs.Trigger value="properties">Properties</Tabs.Trigger>
        <Tabs.Trigger value="layers">Layers</Tabs.Trigger>
        <Tabs.Trigger value="machine">Machine</Tabs.Trigger>
      </Tabs.List>
    </div>
    <Tabs.Content value="properties" class="min-h-0 overflow-auto p-3">
      <SelectionProperties />
    </Tabs.Content>
    <Tabs.Content value="layers" class="min-h-0 overflow-auto p-3">
      <LayersPanel />
    </Tabs.Content>
    <Tabs.Content value="machine" class="min-h-0 overflow-auto p-3">
      {@render machine()}
    </Tabs.Content>
  </Tabs.Root>
</aside>
