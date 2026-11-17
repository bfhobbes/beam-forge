<script lang="ts">
  import { RefreshCw, Settings } from "@lucide/svelte";
  import { Button } from "#lib/components/ui/button/index.js";
  import * as Dialog from "#lib/components/ui/dialog/index.js";
  import * as Tabs from "#lib/components/ui/tabs/index.js";
  import LaserModeWarning from "./LaserModeWarning.svelte";
  import ProfileEditor from "./ProfileEditor.svelte";
  import SettingsTable from "./SettingsTable.svelte";
  import { machineSettings } from "./settings.svelte.js";

  let { open = $bindable(false) }: { open?: boolean } = $props();
  let tab = $state("profile");
  const connected = $derived(machineSettings.settings.length > 0);
  const selectClass = "h-8 min-w-0 rounded-lg border border-input bg-background px-2 text-sm";
</script>

<Dialog.Root bind:open>
  <Dialog.Trigger>
    {#snippet child({ props })}
      <Button variant="outline" class="w-full" {...props}><Settings />Machine settings…</Button>
    {/snippet}
  </Dialog.Trigger>
  <Dialog.Content class="sm:max-w-3xl">
    <Dialog.Header>
      <Dialog.Title>Machine settings</Dialog.Title>
      <Dialog.Description>
        The profile sets the bed, power range and speeds Beam Forge plans with. Connecting refreshes
        it from the controller's $ settings.
      </Dialog.Description>
    </Dialog.Header>
    <LaserModeWarning />
    <Tabs.Root bind:value={tab} class="min-h-0 gap-3">
      <Tabs.List>
        <Tabs.Trigger value="profile">Profile</Tabs.Trigger>
        <Tabs.Trigger value="settings">Controller ($$)</Tabs.Trigger>
      </Tabs.List>
      <Tabs.Content value="profile" class="max-h-[55vh] overflow-y-auto">
        {#if machineSettings.profiles.length > 1}
          <select
            aria-label="Machine profile"
            class="{selectClass} mb-3 w-full"
            value={machineSettings.profile?.id}
            disabled={connected}
            onchange={(event) => machineSettings.selectProfile(event.currentTarget.value)}
          >
            {#each machineSettings.profiles as profile (profile.id)}
              <option value={profile.id}>{profile.name}</option>
            {/each}
          </select>
        {/if}
        {#if machineSettings.profile}
          <ProfileEditor profile={machineSettings.profile} />
        {:else}
          <p class="text-sm text-muted-foreground">
            No machine profile yet. Connect to a machine to create one from its settings.
          </p>
        {/if}
      </Tabs.Content>
      <Tabs.Content value="settings" class="max-h-[55vh] overflow-y-auto">
        {#if connected}
          <div class="mb-2 flex justify-end">
            <Button
              size="sm"
              variant="outline"
              disabled={machineSettings.loading}
              onclick={() => machineSettings.read()}
            >
              <RefreshCw class={machineSettings.loading ? "animate-spin" : ""} />Read again
            </Button>
          </div>
          <SettingsTable />
        {:else}
          <p class="text-sm text-muted-foreground">
            {machineSettings.loading
              ? "Reading settings…"
              : "Connect to a machine to read and change its settings."}
          </p>
        {/if}
      </Tabs.Content>
    </Tabs.Root>
    {#if machineSettings.error}
      <p role="alert" class="text-sm text-destructive">{machineSettings.error}</p>
    {/if}
  </Dialog.Content>
</Dialog.Root>
