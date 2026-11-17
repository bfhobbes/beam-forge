<script lang="ts">
  import { onMount } from "svelte";
  import CommandPalette from "./CommandPalette.svelte";
  import ShortcutSheet from "./ShortcutSheet.svelte";
  import { actions } from "./registry.svelte.js";

  /** Mount once per window: owns the palette, the cheat sheet and shortcut dispatch. */
  let palette = $state(false);
  let sheet = $state(false);

  onMount(() =>
    actions.register(
      {
        id: "app.palette",
        title: "Command palette",
        group: "Help",
        keywords: ["search", "actions", "commands"],
        whileTyping: true,
        run: () => {
          sheet = false;
          palette = !palette;
        },
      },
      {
        id: "app.shortcuts",
        title: "Keyboard shortcuts",
        group: "Help",
        keywords: ["cheat sheet", "keys", "bindings"],
        run: () => {
          palette = false;
          sheet = !sheet;
        },
      },
    ),
  );

  function onkeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing) return;
    if (actions.handleKey(event)) event.preventDefault();
  }
</script>

<svelte:window {onkeydown} />
<CommandPalette bind:open={palette} />
<ShortcutSheet bind:open={sheet} />
