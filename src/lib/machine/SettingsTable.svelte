<script lang="ts">
  import type { MachineSetting } from "#lib/bindings/MachineSetting.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import { machineSettings } from "./settings.svelte.js";

  /** Edited values not yet written, by setting number. */
  let drafts = $state<Record<number, string>>({});
  /** The setting whose write is waiting for confirmation. */
  let confirming = $state<number | null>(null);

  const edited = (setting: MachineSetting) =>
    drafts[setting.number] !== undefined && drafts[setting.number].trim() !== setting.value;

  function discard(number: number) {
    delete drafts[number];
    if (confirming === number) confirming = null;
  }

  async function write(setting: MachineSetting) {
    if (await machineSettings.write(setting.number, drafts[setting.number])) {
      discard(setting.number);
    }
  }
</script>

<table class="w-full text-sm">
  <thead class="text-left text-xs text-muted-foreground">
    <tr>
      <th class="w-12 py-1 font-normal">$</th>
      <th class="py-1 font-normal">Setting</th>
      <th class="w-32 py-1 font-normal">Value</th>
      <th class="w-16 py-1 font-normal">Unit</th>
      <th class="w-20 py-1"><span class="sr-only">Actions</span></th>
    </tr>
  </thead>
  <tbody>
    {#each machineSettings.settings as setting (setting.number)}
      {@const label = setting.name ?? `Setting ${setting.number}`}
      <tr class="border-t align-top">
        <td class="py-1.5 font-mono text-xs text-muted-foreground">${setting.number}</td>
        <td class="py-1.5 pr-2">
          <div class={setting.number === 32 && setting.value !== "1" ? "text-destructive" : ""}>
            {label}
          </div>
          {#if setting.description}
            <div class="text-xs text-muted-foreground">{setting.description}</div>
          {/if}
        </td>
        <td class="py-1">
          <Input
            aria-label="${setting.number} {label}"
            class="h-7 font-mono tabular-nums"
            inputmode="decimal"
            value={drafts[setting.number] ?? setting.value}
            disabled={machineSettings.writing !== null}
            oninput={(event) => (drafts[setting.number] = event.currentTarget.value)}
            onkeydown={(event) => {
              if (event.key === "Enter" && edited(setting)) confirming = setting.number;
              if (event.key === "Escape") discard(setting.number);
            }}
          />
        </td>
        <td class="py-1.5 pl-2 text-xs text-muted-foreground">{setting.unit ?? ""}</td>
        <td class="py-1 text-right">
          {#if edited(setting) && confirming !== setting.number}
            <Button size="sm" variant="outline" onclick={() => (confirming = setting.number)}>
              Apply
            </Button>
          {/if}
        </td>
      </tr>
      {#if confirming === setting.number}
        <tr>
          <td></td>
          <td colspan="4" class="pb-2">
            <div class="flex flex-wrap items-center gap-2 rounded-lg bg-muted p-2 text-xs">
              <span class="flex-1">
                Change ${setting.number} ({label}) from <code>{setting.value}</code> to
                <code>{drafts[setting.number].trim()}</code>? The controller keeps it after a
                restart.
              </span>
              <Button
                size="sm"
                disabled={machineSettings.writing !== null}
                onclick={() => write(setting)}
              >
                {machineSettings.writing === setting.number ? "Writing…" : "Write"}
              </Button>
              <Button size="sm" variant="outline" onclick={() => discard(setting.number)}>
                Cancel
              </Button>
            </div>
          </td>
        </tr>
      {/if}
    {/each}
  </tbody>
</table>
