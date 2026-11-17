// The protocol inspector's log: raw wire events from the emulator turned into
// lines paired with their `ok`/`error:N`, named realtime commands and
// controller messages. Pairing follows GRBL: every `\n` or `\r` the RX buffer
// accepts ends a line (an empty one too), and each line gets exactly one
// `ok` or `error:N`, in order.

import type { WireEvent } from "#lib/bindings/WireEvent.js";
import { stamp } from "#lib/format.js";

export type EntryKind = "line" | "realtime" | "status" | "message" | "overflow" | "event";

export interface Entry {
  readonly id: number;
  readonly kind: EntryKind;
  /** Simulated clock in ms when the request, message or event happened. */
  readonly at: number;
  /** What crossed the wire, made printable (`\n`, `\x85`). */
  readonly text: string;
  /** Realtime command name, e.g. `Feed hold`. */
  readonly label?: string;
  /** The `ok`, `error:N` or status report that answered it. */
  readonly response?: string;
  readonly respondedAt?: number;
  readonly error: boolean;
  /** For a line: its number, counting line ends accepted since the app started. */
  readonly line?: number;
}

// GRBL 1.1 realtime commands (config.h), acted on as they arrive and never
// buffered. Other bytes from 0x80 up are dropped by GRBL's serial ISR.
const REALTIME = new Map<number, string>([
  [0x3f, "Status report"],
  [0x21, "Feed hold"],
  [0x7e, "Cycle start"],
  [0x18, "Soft reset"],
  [0x84, "Safety door"],
  [0x85, "Jog cancel"],
  [0x90, "Feed 100%"],
  [0x91, "Feed +10%"],
  [0x92, "Feed −10%"],
  [0x93, "Feed +1%"],
  [0x94, "Feed −1%"],
  [0x95, "Rapid 100%"],
  [0x96, "Rapid 50%"],
  [0x97, "Rapid 25%"],
  [0x99, "Spindle 100%"],
  [0x9a, "Spindle +10%"],
  [0x9b, "Spindle −10%"],
  [0x9c, "Spindle +1%"],
  [0x9d, "Spindle −1%"],
  [0x9e, "Spindle stop"],
  [0xa0, "Flood toggle"],
  [0xa1, "Mist toggle"],
]);

const hex = (byte: number) => byte.toString(16).toUpperCase().padStart(2, "0");

/**
 * The name of a byte GRBL handles outside the RX buffer, or undefined for a
 * byte that is buffered as part of a line.
 */
export function realtimeName(byte: number): string | undefined {
  return REALTIME.get(byte) ?? (byte >= 0x80 ? `Ignored 0x${hex(byte)}` : undefined);
}

/** Bytes as text, with control and non-ASCII bytes escaped. */
export function printable(bytes: Iterable<number>): string {
  let text = "";
  for (const byte of bytes) {
    if (byte === 0x0a) text += "\\n";
    else if (byte === 0x0d) text += "\\r";
    else if (byte === 0x09) text += "\\t";
    else if (byte === 0x5c) text += "\\\\";
    else if (byte >= 0x20 && byte < 0x7f) text += String.fromCharCode(byte);
    else text += `\\x${hex(byte)}`;
  }
  return text;
}

const isResponse = (line: string) => line === "ok" || /^error:\d+$/.test(line);
const isStatusReport = (line: string) => line.startsWith("<") && line.endsWith(">");
const isBanner = (line: string) => line.startsWith("Grbl ");

export class ProtocolLog {
  /** Oldest first, at most `capacity`. Entries are replaced, never mutated. */
  readonly entries: Entry[] = [];
  private nextId = 0;
  /** The line GRBL is buffering: accepted bytes since the last `\n`/`\r`. */
  private line: number[] = [];
  private lineAt = 0;
  /** Controller output not yet ended by `\n`. */
  private output: number[] = [];
  /** Lines and `?`s waiting for an answer, oldest first. */
  private waitingLines: number[] = [];
  private waitingStatus: number[] = [];

  constructor(readonly capacity = 2000) {}

  apply(events: readonly WireEvent[]): void {
    for (const { at, event } of events) {
      switch (event.kind) {
        case "received":
          this.received(at, event.bytes, event.dropped, event.linesBefore);
          break;
        case "sent":
          this.sent(at, event.bytes);
          break;
        case "state":
          this.add({ kind: "event", at, text: `State ${event.from} → ${event.to}` });
          break;
        case "note":
          this.add({ kind: "event", at, text: event.text });
          break;
        case "fault":
          this.add({ kind: "event", at, text: `Fault injected: ${event.text}`, error: true });
          break;
      }
    }
  }

  /** Events were lost between batches, so pairing starts over. */
  gap(at: number): void {
    this.forget();
    this.add({ kind: "event", at, text: "Log fell behind; earlier events were dropped" });
  }

  /** Empties the log; lines still waiting for an answer stay paired. */
  clear(): void {
    this.entries.length = 0;
  }

  private received(at: number, bytes: number[], dropped: number, linesBefore: number) {
    let line = linesBefore;
    // The RX buffer filled during this chunk, so its last `dropped` buffered
    // bytes never reached GRBL.
    const kept = bytes.filter((byte) => realtimeName(byte) === undefined).length - dropped;
    const lost: number[] = [];
    let buffered = 0;
    for (const byte of bytes) {
      const label = realtimeName(byte);
      if (label !== undefined) {
        const entry = this.add({
          kind: byte === 0x3f ? "status" : "realtime",
          at,
          text: printable([byte]),
          label,
        });
        if (byte === 0x3f) this.waitingStatus.push(entry.id);
      } else if (buffered++ >= kept) {
        lost.push(byte);
      } else if (byte === 0x0a || byte === 0x0d) {
        const text = printable([...this.line, byte]);
        const entry = this.add({
          kind: "line",
          at: this.line.length ? this.lineAt : at,
          text,
          line: ++line,
        });
        this.waitingLines.push(entry.id);
        this.line = [];
      } else {
        if (!this.line.length) this.lineAt = at;
        this.line.push(byte);
      }
    }
    if (lost.length) {
      this.add({
        kind: "overflow",
        at,
        text: `RX buffer overflow: ${lost.length} bytes lost: ${printable(lost)}`,
        error: true,
      });
    }
  }

  private sent(at: number, bytes: number[]) {
    for (const byte of bytes) {
      if (byte !== 0x0a) {
        this.output.push(byte);
        continue;
      }
      if (this.output.at(-1) === 0x0d) this.output.pop();
      const line = printable(this.output);
      this.output = [];
      if (line) this.message(at, line);
    }
  }

  private message(at: number, line: string) {
    if (isResponse(line)) {
      const id = this.waitingLines.shift();
      const error = line !== "ok";
      if (id === undefined) {
        this.add({ kind: "message", at, text: line, label: "No line waiting", error: true });
      } else if (!this.respond(id, at, line, error)) {
        // Its line was cleared or scrolled out of the log.
        this.add({ kind: "message", at, text: line, error });
      }
    } else if (isStatusReport(line)) {
      const waiting = this.waitingStatus;
      this.waitingStatus = [];
      // GRBL answers several `?`s sent before its report with one report.
      const answered = waiting.filter((id) => this.respond(id, at, line, false));
      if (!answered.length) this.add({ kind: "status", at, text: "", response: line });
    } else {
      if (isBanner(line)) {
        // A reset or power-up empties GRBL's buffers: those lines never get an answer.
        for (const id of this.waitingLines) this.respond(id, at, "(flushed by reset)", false);
        this.forget();
      }
      this.add({ kind: "message", at, text: line, error: line.startsWith("ALARM:") });
    }
  }

  private forget() {
    this.line = [];
    this.waitingLines = [];
    this.waitingStatus = [];
  }

  private add(entry: Omit<Entry, "id" | "error"> & { error?: boolean }): Entry {
    const added: Entry = { error: false, ...entry, id: this.nextId++ };
    this.entries.push(added);
    if (this.entries.length > this.capacity)
      this.entries.splice(0, this.entries.length - this.capacity);
    return added;
  }

  /** Records an answer; false if the entry has already scrolled out of the log. */
  private respond(id: number, at: number, response: string, error: boolean): boolean {
    const index = id - (this.entries[0]?.id ?? 0);
    const entry = this.entries[index];
    if (entry?.id !== id) return false;
    this.entries[index] = { ...entry, response, respondedAt: at, error };
    return true;
  }
}

export type Tone = "time" | "out" | "in" | "realtime" | "error" | "muted" | "event";

export interface Segment {
  readonly text: string;
  readonly tone: Tone;
}

/** One entry as the log shows it; export uses the same text. */
export function segments(entry: Entry): Segment[] {
  const parts: Segment[] = [{ text: `${stamp(entry.at)}  `, tone: "time" }];
  const answer = (tone: Tone = entry.error ? "error" : "in") => {
    if (entry.response === undefined) {
      parts.push({ text: "  … waiting", tone: "muted" });
      return;
    }
    parts.push({ text: "  ← ", tone: "muted" }, { text: entry.response, tone });
    if (entry.respondedAt !== undefined && entry.respondedAt >= entry.at) {
      parts.push({ text: ` (${Math.round(entry.respondedAt - entry.at)} ms)`, tone: "muted" });
    }
  };
  switch (entry.kind) {
    case "line":
      parts.push({ text: "→ ", tone: "muted" }, { text: entry.text, tone: "out" });
      answer();
      break;
    case "realtime":
      parts.push(
        { text: "→ ", tone: "muted" },
        { text: `[${entry.label}] ${entry.text}`, tone: "realtime" },
      );
      break;
    case "status":
      if (entry.text) {
        parts.push(
          { text: "→ ", tone: "muted" },
          { text: `[${entry.label}] ${entry.text}`, tone: "realtime" },
        );
        answer("muted");
      } else {
        parts.push({ text: "← ", tone: "muted" }, { text: entry.response ?? "", tone: "muted" });
      }
      break;
    case "message":
      parts.push(
        { text: "← ", tone: "muted" },
        { text: entry.text, tone: entry.error ? "error" : "in" },
      );
      if (entry.label) parts.push({ text: `  (${entry.label})`, tone: "error" });
      break;
    case "overflow":
      parts.push({ text: "✕ ", tone: "error" }, { text: entry.text, tone: "error" });
      break;
    case "event":
      parts.push({ text: "• ", tone: "event" }, { text: entry.text, tone: "event" });
      break;
  }
  return parts;
}

export function lineText(entry: Entry): string {
  return segments(entry)
    .map((segment) => segment.text)
    .join("");
}

export type FilterKind = "lines" | "realtime" | "status" | "messages" | "events";

export const filterKinds: { id: FilterKind; label: string }[] = [
  { id: "lines", label: "Lines" },
  { id: "realtime", label: "Realtime" },
  { id: "status", label: "Status reports" },
  { id: "messages", label: "Messages" },
  { id: "events", label: "Events" },
];

export interface Filter {
  readonly kinds: Readonly<Record<FilterKind, boolean>>;
  readonly search: string;
}

/** Status reports are noise most of the time, so they start hidden. */
export const defaultFilter: Filter = {
  kinds: { lines: true, realtime: true, status: false, messages: true, events: true },
  search: "",
};

const filterKind: Record<EntryKind, FilterKind | null> = {
  line: "lines",
  realtime: "realtime",
  status: "status",
  message: "messages",
  event: "events",
  // An overflow is always an error worth seeing.
  overflow: null,
};

export function visible(entries: readonly Entry[], filter: Filter): Entry[] {
  const search = filter.search.trim().toLowerCase();
  return entries.filter((entry) => {
    const kind = filterKind[entry.kind];
    if (kind && !filter.kinds[kind]) return false;
    return !search || lineText(entry).toLowerCase().includes(search);
  });
}

/** The log as text, one line per entry shown. */
export function exportText(entries: readonly Entry[]): string {
  return entries.map((entry) => `${lineText(entry)}\n`).join("");
}
