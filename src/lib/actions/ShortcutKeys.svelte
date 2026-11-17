<script lang="ts">
  import * as Kbd from "#lib/components/ui/kbd/index.js";
  import { actions } from "./registry.svelte.js";
  import { formatShortcut, type Shortcut } from "./shortcut.js";

  let { shortcut, class: className }: { shortcut: Shortcut; class?: string } = $props();
</script>

<Kbd.Group class={className}>
  {#each formatShortcut(shortcut, actions.mac) as key, index (index)}
    <Kbd.Root>{key}</Kbd.Root>
  {/each}
</Kbd.Group>
