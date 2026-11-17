<script lang="ts">
  import ChevronsUpDown from "@lucide/svelte/icons/chevrons-up-down";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Command from "#lib/components/ui/command/index.js";
  import * as Popover from "#lib/components/ui/popover/index.js";
  import { fontList } from "#lib/text/fonts.svelte.js";

  /**
   * Picks a font family. Each name is drawn in its own font, so the list is
   * its own preview; typing filters it.
   */
  let { value, onpick }: { value: string; onpick: (family: string) => void } = $props();

  let open = $state(false);
  let query = $state("");

  const matches = $derived.by(() => {
    const needle = query.trim().toLowerCase();
    const all = fontList.families;
    return needle ? all.filter((family) => family.name.toLowerCase().includes(needle)) : all;
  });

  /** Quoted for CSS, so names with spaces or digits work. */
  const cssFamily = (name: string) => `"${name.replaceAll('"', '\\"')}", sans-serif`;

  function pick(family: string) {
    open = false;
    query = "";
    onpick(family);
  }
</script>

<Popover.Root bind:open onOpenChange={(next) => next && void fontList.load()}>
  <Popover.Trigger>
    {#snippet child({ props })}
      <Button
        {...props}
        variant="outline"
        size="sm"
        role="combobox"
        aria-expanded={open}
        aria-label="Font"
        class="h-7 min-w-0 flex-1 justify-between px-2 text-xs font-normal"
      >
        <span class="truncate" style:font-family={cssFamily(value)}>{value}</span>
        <ChevronsUpDown class="opacity-50" />
      </Button>
    {/snippet}
  </Popover.Trigger>
  <Popover.Content align="start" class="w-72 p-0">
    <Command.Root shouldFilter={false}>
      <Command.Input placeholder="Search fonts…" bind:value={query} />
      <Command.List class="max-h-80">
        {#if fontList.error}
          <p role="alert" class="p-3 text-xs text-destructive">
            Couldn’t list fonts: {fontList.error}
          </p>
        {:else if !fontList.loaded}
          <p role="status" class="p-3 text-xs text-muted-foreground">Loading fonts…</p>
        {:else}
          <Command.Empty>No font matches “{query}”.</Command.Empty>
          {#each matches as family (family.name)}
            <Command.Item value={family.name} onSelect={() => pick(family.name)}>
              <span class="truncate text-base" style:font-family={cssFamily(family.name)}>
                {family.name}
              </span>
              {#if family.styles.length > 1}
                <span class="ml-auto shrink-0 text-xs text-muted-foreground">
                  {family.styles.length}
                </span>
              {/if}
            </Command.Item>
          {/each}
        {/if}
      </Command.List>
    </Command.Root>
  </Popover.Content>
</Popover.Root>
