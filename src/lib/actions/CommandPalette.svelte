<script lang="ts">
  import * as Command from "#lib/components/ui/command/index.js";
  import PaletteResults from "./PaletteResults.svelte";
  import { actions } from "./registry.svelte.js";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  function choose(id: string) {
    open = false;
    // Let the dialog close and return focus before the action opens its own UI.
    queueMicrotask(() => actions.run(id));
  }
</script>

<Command.Dialog
  bind:open
  shouldFilter={false}
  title="Command palette"
  description="Search for an action to run"
>
  <PaletteResults onchoose={choose} />
</Command.Dialog>
