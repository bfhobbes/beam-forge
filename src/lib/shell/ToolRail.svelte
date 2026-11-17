<script lang="ts">
  import { onMount } from "svelte";
  import * as ToggleGroup from "#lib/components/ui/toggle-group/index.js";
  import * as Tooltip from "#lib/components/ui/tooltip/index.js";
  import ShortcutKeys from "#lib/actions/ShortcutKeys.svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { tools, toolActionId, toolActions, type ToolId } from "./tools.js";

  let { tool = $bindable("select") }: { tool?: ToolId } = $props();

  onMount(() => actions.register(...toolActions((next) => (tool = next))));
</script>

<nav
  aria-label="Tools"
  class="flex w-11 shrink-0 flex-col items-center border-r bg-background py-2"
>
  <ToggleGroup.Root
    type="single"
    orientation="vertical"
    spacing={1}
    bind:value={() => tool, (value) => value && (tool = value as ToolId)}
  >
    {#each tools as item (item.id)}
      {@const shortcut = actions.get(toolActionId(item.id))?.shortcut}
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <ToggleGroup.Item {...props} value={item.id} aria-label={item.label} size="sm">
              <item.icon />
            </ToggleGroup.Item>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content side="right">
          {item.label}
          {#if shortcut}<ShortcutKeys {shortcut} class="ml-1" />{/if}
        </Tooltip.Content>
      </Tooltip.Root>
    {/each}
  </ToggleGroup.Root>
</nav>
