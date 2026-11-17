<script lang="ts">
  import * as Command from "#lib/components/ui/command/index.js";
  import ShortcutKeys from "./ShortcutKeys.svelte";
  import { actions, groupActions, type RegisteredAction } from "./registry.svelte.js";

  /** The palette's search and results. Mounted per opening, so each starts with an empty query. */
  let { onchoose }: { onchoose: (id: string) => void } = $props();

  let query = $state("");
  const results = $derived(actions.search(query));
  // Recent actions lead an empty palette; a query ranks everything instead.
  const sections = $derived<[string, RegisteredAction[]][]>(
    query.trim()
      ? [["Results", results]]
      : [
          ...(actions.recent.length
            ? [["Recent", actions.recent] as [string, RegisteredAction[]]]
            : []),
          ...groupActions(results),
        ],
  );
</script>

<Command.Input placeholder="Search actions…" bind:value={query} />
<Command.List>
  <Command.Empty>No matching actions.</Command.Empty>
  {#each sections as [heading, list] (heading)}
    <Command.Group {heading}>
      {#each list as action (action.id)}
        {@const enabled = actions.enabled(action)}
        <Command.Item
          value={`${heading}:${action.id}`}
          disabled={!enabled}
          onSelect={() => onchoose(action.id)}
        >
          <span class="truncate">{action.title}</span>
          {#if heading === "Recent" || heading === "Results"}
            <span class="text-xs text-muted-foreground">{action.group}</span>
          {/if}
          {#if !enabled && action.hint}
            <span class="truncate text-xs text-muted-foreground">· {action.hint}</span>
          {/if}
          {#if action.shortcut}
            <ShortcutKeys shortcut={action.shortcut} class="ml-auto" />
          {/if}
        </Command.Item>
      {/each}
    </Command.Group>
  {/each}
</Command.List>
