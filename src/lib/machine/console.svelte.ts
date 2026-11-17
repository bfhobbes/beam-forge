import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { Reply } from "#lib/bindings/Reply.js";
import { machineAdapter, type MachineAdapter } from "./adapter.js";
import { formatPosition, stateLabel } from "./status.js";

/** One scrollback line. `note` lines come from Beam Forge, not the controller. */
export type ConsoleEntry =
  | { id: number; kind: "sent"; text: string }
  | { id: number; kind: "received"; text: string; reply: Reply }
  | { id: number; kind: "status"; text: string }
  | { id: number; kind: "note"; text: string; error: boolean };

/** Lines kept in the scrollback; older ones drop off the top. */
export const SCROLLBACK = 1000;
/** Commands kept for up/down recall. */
export const HISTORY = 100;

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** A status report as one console line, e.g. `Idle · MPos X 0.00 Y 0.00 Z 0.00`. */
export function statusLine(status: MachineStatus): string {
  const parts = [stateLabel(status.state), `MPos ${formatPosition(status.machinePosition)}`];
  if (status.workOffset) parts.push(`WPos ${formatPosition(status.workPosition)}`);
  parts.push(`F ${status.feed} S ${status.spindle}`);
  return parts.join(" · ");
}

/** What a decoded reply adds to its raw line: the error or alarm text, if any. */
export function explain(reply: Reply): string | null {
  switch (reply.kind) {
    case "error":
    case "alarm":
      return `${reply.title}: ${reply.description}`;
    default:
      return null;
  }
}

/**
 * The machine console: scrollback of sent and received lines, command
 * history, and sending. Status reports are hidden unless `showStatus` is on.
 */
export class MachineConsole {
  entries = $state.raw<ConsoleEntry[]>([]);
  showStatus = $state(false);
  /** Sent commands, oldest first. */
  history = $state.raw<string[]>([]);

  readonly #adapter: MachineAdapter;
  #nextId = 0;
  /** Position while recalling history; `history.length` means the draft. */
  #cursor = 0;
  #draft = "";

  constructor(adapter: MachineAdapter) {
    this.#adapter = adapter;
  }

  /** Follows controller output. Returns the cleanup. */
  async start(): Promise<() => void> {
    const stopLine = await this.#adapter.onLine(({ line, reply }) =>
      this.#push({ kind: "received", text: line, reply }),
    );
    const stopStatus = await this.#adapter.onStatus((status) => {
      if (this.showStatus) this.#push({ kind: "status", text: statusLine(status) });
    });
    return () => {
      stopLine();
      stopStatus();
    };
  }

  /** Sends a command line typed or picked by the user. False if nothing went out. */
  async send(text: string): Promise<boolean> {
    const line = text.trim();
    if (!line) return false;
    if (this.history.at(-1) !== line) this.history = [...this.history, line].slice(-HISTORY);
    this.#cursor = this.history.length;
    this.#draft = "";
    this.#push({ kind: "sent", text: line });
    try {
      await this.#adapter.send(line);
      return true;
    } catch (cause) {
      this.note(message(cause), true);
      return false;
    }
  }

  async softReset(): Promise<void> {
    this.note("Soft reset");
    try {
      await this.#adapter.softReset();
    } catch (cause) {
      this.note(message(cause), true);
    }
  }

  /** The previous command (up arrow), keeping `current` as the draft to return to. */
  previous(current: string): string {
    if (this.#cursor >= this.history.length) {
      this.#cursor = this.history.length;
      this.#draft = current;
    }
    if (this.#cursor > 0) this.#cursor--;
    return this.history[this.#cursor] ?? current;
  }

  /** The next command (down arrow), ending at the draft. */
  next(current: string): string {
    if (this.#cursor >= this.history.length) return current;
    this.#cursor++;
    return this.#cursor === this.history.length ? this.#draft : this.history[this.#cursor];
  }

  toggleStatus(): void {
    this.showStatus = !this.showStatus;
    if (!this.showStatus) this.entries = this.entries.filter((entry) => entry.kind !== "status");
  }

  clear(): void {
    this.entries = [];
  }

  note(text: string, error = false): void {
    this.#push({ kind: "note", text, error });
  }

  #push(entry: DistributiveOmit<ConsoleEntry, "id">): void {
    const next = [...this.entries, { ...entry, id: this.#nextId++ } as ConsoleEntry];
    this.entries = next.length > SCROLLBACK ? next.slice(-SCROLLBACK) : next;
  }
}

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

export const machineConsole = new MachineConsole(machineAdapter);
