<script lang="ts">
  import { Eraser, RotateCcw, SendHorizontal } from "@lucide/svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { Button } from "#lib/components/ui/button/index.js";
  import { Input } from "#lib/components/ui/input/index.js";
  import { Toggle } from "#lib/components/ui/toggle/index.js";
  import { QUICK_COMMANDS } from "./console-actions.js";
  import { explain, machineConsole, type ConsoleEntry } from "./console.svelte.js";
  import { machine } from "./connection.svelte.js";

  let input = $state("");
  let log = $state<HTMLElement>();
  // Follow new output unless the user scrolled up to read.
  let stick = true;

  $effect(() => {
    void machineConsole.entries;
    if (log && stick) log.scrollTop = log.scrollHeight;
  });

  function onscroll() {
    if (log) stick = log.scrollHeight - log.scrollTop - log.clientHeight < 16;
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const line = input;
    if (!line.trim()) return;
    input = "";
    stick = true;
    await machineConsole.send(line);
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "ArrowUp") {
      event.preventDefault();
      input = machineConsole.previous(input);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      input = machineConsole.next(input);
    }
  }

  function tone(entry: ConsoleEntry): string {
    switch (entry.kind) {
      case "sent":
        return "text-primary";
      case "status":
        return "text-muted-foreground";
      case "note":
        return entry.error ? "text-destructive" : "text-muted-foreground italic";
      case "received":
        switch (entry.reply.kind) {
          case "error":
          case "alarm":
            return "text-destructive";
          case "message":
            return "text-amber-600 dark:text-amber-400";
          case "ok":
            return "text-muted-foreground";
          default:
            return "";
        }
    }
  }
</script>

<section aria-label="Machine console" class="space-y-2">
  <div class="flex items-center justify-between gap-2">
    <h3 class="text-sm font-medium">Console</h3>
    <div class="flex items-center gap-1">
      <Toggle
        size="sm"
        aria-label="Show status reports"
        pressed={machineConsole.showStatus}
        onPressedChange={() => actions.run("console.toggle-status")}
      >
        Status
      </Toggle>
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label="Clear console"
        onclick={() => actions.run("console.clear")}
      >
        <Eraser />
      </Button>
    </div>
  </div>
  <div class="flex flex-wrap gap-1">
    {#each QUICK_COMMANDS as command (command.id)}
      <Button
        variant="outline"
        size="sm"
        class="font-mono"
        title={command.title}
        disabled={!machine.connected}
        onclick={() => actions.run(command.id)}
      >
        {command.line}
      </Button>
    {/each}
    <Button
      variant="outline"
      size="sm"
      title="Soft reset (Ctrl-X): aborts any motion"
      disabled={!machine.connected}
      onclick={() => actions.run("console.soft-reset")}
    >
      <RotateCcw />Reset
    </Button>
  </div>
  <div
    bind:this={log}
    {onscroll}
    role="log"
    aria-label="Console output"
    class="h-64 overflow-y-auto rounded-lg border bg-muted/30 p-2 font-mono text-xs leading-relaxed"
  >
    {#each machineConsole.entries as entry (entry.id)}
      <div class="break-all {tone(entry)}">
        {#if entry.kind === "sent"}<span aria-hidden="true">&gt; </span>{/if}{entry.text}
        {#if entry.kind === "received" && explain(entry.reply)}
          <div class="pl-3 font-sans">{explain(entry.reply)}</div>
        {/if}
      </div>
    {:else}
      <p class="font-sans text-muted-foreground">
        {machine.connected ? "Type a command, or try $I." : "Connect to see machine output."}
      </p>
    {/each}
  </div>
  <form class="flex gap-2" onsubmit={submit}>
    <Input
      aria-label="Command"
      class="h-8 font-mono text-xs"
      placeholder={machine.connected ? "G-code or $ command" : "Not connected"}
      autocomplete="off"
      spellcheck={false}
      disabled={!machine.connected}
      bind:value={input}
      {onkeydown}
    />
    <Button type="submit" size="icon" aria-label="Send" disabled={!machine.connected}>
      <SendHorizontal />
    </Button>
  </form>
</section>
