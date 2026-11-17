<script lang="ts">
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import ShortcutKeys from "./ShortcutKeys.svelte";
  import { actions, groupActions } from "./registry.svelte.js";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  const groups = $derived(groupActions(actions.all.filter((action) => action.shortcut)));
</script>

<Dialog.Root bind:open>
  <Dialog.Content class="sm:max-w-xl">
    <Dialog.Header>
      <Dialog.Title>Keyboard shortcuts</Dialog.Title>
      <Dialog.Description>Shortcuts don’t fire while you’re typing in a field.</Dialog.Description>
    </Dialog.Header>
    <div class="grid max-h-[60vh] gap-x-8 gap-y-4 overflow-y-auto sm:grid-cols-2">
      {#each groups as [group, list] (group)}
        <section class="space-y-1.5">
          <h3 class="text-xs font-medium text-muted-foreground">{group}</h3>
          <dl class="space-y-1">
            {#each list as action (action.id)}
              <div class="flex items-center justify-between gap-4 text-sm">
                <dt>{action.title}</dt>
                <dd><ShortcutKeys shortcut={action.shortcut!} /></dd>
              </div>
            {/each}
          </dl>
        </section>
      {/each}
    </div>
  </Dialog.Content>
</Dialog.Root>
