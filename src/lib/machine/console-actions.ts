import { actions as appActions, type ActionRegistry } from "#lib/actions/registry.svelte.js";
import type { MachineConsole } from "./console.svelte.js";

/** The console's quick commands, in toolbar order. */
export const QUICK_COMMANDS = [
  { id: "console.settings", line: "$$", title: "Show GRBL settings", keywords: ["$$"] },
  { id: "console.coordinates", line: "$#", title: "Show coordinate offsets", keywords: ["$#"] },
  { id: "console.build-info", line: "$I", title: "Show build info", keywords: ["$I", "version"] },
  { id: "console.parser-state", line: "$G", title: "Show G-code parser state", keywords: ["$G"] },
  { id: "console.unlock", line: "$X", title: "Unlock (clear alarm)", keywords: ["$X", "alarm"] },
] as const;

const NOT_CONNECTED = "Connect to a machine first.";

/** Registers the console actions. Returns the cleanup. */
export function registerConsoleActions(
  machineConsole: MachineConsole,
  connected: () => boolean,
  registry: ActionRegistry = appActions,
): () => void {
  return registry.register(
    ...QUICK_COMMANDS.map(({ id, line, title, keywords }) => ({
      id,
      title,
      group: "Machine",
      keywords: ["console", ...keywords],
      enabled: connected,
      hint: NOT_CONNECTED,
      run: () => void machineConsole.send(line),
    })),
    {
      id: "console.soft-reset",
      title: "Soft reset",
      group: "Machine",
      keywords: ["console", "ctrl-x", "abort", "stop"],
      enabled: connected,
      hint: NOT_CONNECTED,
      run: () => void machineConsole.softReset(),
    },
    {
      id: "console.toggle-status",
      title: "Show status reports in console",
      group: "Machine",
      keywords: ["console", "status", "?"],
      run: () => machineConsole.toggleStatus(),
    },
    {
      id: "console.clear",
      title: "Clear console",
      group: "Machine",
      keywords: ["console", "scrollback"],
      run: () => machineConsole.clear(),
    },
  );
}

/**
 * Starts the app-wide console: it records output even while the Machine tab
 * is closed. Returns the cleanup.
 */
export function startConsole(machineConsole: MachineConsole, connected: () => boolean): () => void {
  const unregister = registerConsoleActions(machineConsole, connected);
  const started = machineConsole.start();
  return () => {
    unregister();
    void started.then((stop) => stop());
  };
}
