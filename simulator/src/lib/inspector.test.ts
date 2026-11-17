import { describe, expect, it } from "vite-plus/test";
import type { Wire } from "#lib/bindings/Wire.js";
import type { WireEvent } from "#lib/bindings/WireEvent.js";
import {
  defaultFilter,
  exportText,
  lineText,
  ProtocolLog,
  realtimeName,
  segments,
  visible,
  type Entry,
} from "./inspector.js";

const bytes = (text: string) => Array.from(text, (c) => c.charCodeAt(0));
let seq = 0;
const at = (ms: number, event: Wire): WireEvent => ({ seq: seq++, at: ms, event });
const rx = (ms: number, text: string | number[], dropped = 0, linesBefore = 0) =>
  at(ms, {
    kind: "received",
    bytes: typeof text === "string" ? bytes(text) : text,
    dropped,
    linesBefore,
  });
const tx = (ms: number, text: string) => at(ms, { kind: "sent", bytes: bytes(text) });

function log(...events: WireEvent[]): Entry[] {
  const protocol = new ProtocolLog();
  protocol.apply(events);
  return protocol.entries;
}

const pick = (entries: Entry[]) =>
  entries.map(({ kind, text, label, response, error }) => ({ kind, text, label, response, error }));

describe("request/response pairing", () => {
  it("pairs lines with their answers in order, however the bytes are split", () => {
    const entries = log(
      rx(0, "G1X1F600\nG1"),
      rx(1, "X2\n$Q\n"),
      tx(5, "ok\r\nok"),
      tx(6, "\r\nerror:3\r\n"),
    );
    expect(pick(entries)).toEqual([
      { kind: "line", text: "G1X1F600\\n", label: undefined, response: "ok", error: false },
      { kind: "line", text: "G1X2\\n", label: undefined, response: "ok", error: false },
      { kind: "line", text: "$Q\\n", label: undefined, response: "error:3", error: true },
    ]);
    expect(entries[1].at).toBe(0);
    expect(lineText(entries[0])).toBe("0:00:00.000  → G1X1F600\\n  ← ok (5 ms)");
  });

  it("treats \\r\\n as two lines, as GRBL does", () => {
    const entries = log(rx(0, "G0X1\r\n"), tx(1, "ok\r\nok\r\n"));
    expect(entries.map((entry) => [entry.text, entry.response])).toEqual([
      ["G0X1\\r", "ok"],
      ["\\n", "ok"],
    ]);
  });

  it("numbers lines like the burn trace, from the host's count", () => {
    const entries = log(rx(0, "G0X1\r\nG1", 0, 40), rx(1, "X2\n?$G\n", 0, 42));
    expect(entries.filter((entry) => entry.kind === "line").map((entry) => entry.line)).toEqual([
      41, 42, 43, 44,
    ]);
  });

  it("keeps a line waiting until its answer arrives", () => {
    const entries = log(rx(0, "G4P1\n"));
    expect(entries[0].response).toBeUndefined();
    expect(lineText(entries[0])).toContain("… waiting");
  });

  it("flags an answer with no line waiting", () => {
    const [entry] = log(tx(0, "ok\r\n"));
    expect(entry).toMatchObject({
      kind: "message",
      text: "ok",
      error: true,
      label: "No line waiting",
    });
  });

  it("pairs status reports with ? and keeps controller messages apart", () => {
    const entries = log(
      rx(0, "$I\n?"),
      tx(2, "[VER:1.1h.20190825:]\r\n<Idle|MPos:0.000,0.000,0.000>\r\nok\r\n"),
    );
    expect(pick(entries)).toEqual([
      { kind: "line", text: "$I\\n", label: undefined, response: "ok", error: false },
      {
        kind: "status",
        text: "?",
        label: "Status report",
        response: "<Idle|MPos:0.000,0.000,0.000>",
        error: false,
      },
      {
        kind: "message",
        text: "[VER:1.1h.20190825:]",
        label: undefined,
        response: undefined,
        error: false,
      },
    ]);
  });

  it("drops waiting lines when a reset prints the banner", () => {
    const entries = log(rx(0, "G1X5\n\x18"), tx(1, "\r\nGrbl 1.1h ['$' for help]\r\nok\r\n"));
    expect(entries[0].response).toBe("(flushed by reset)");
    expect(entries.at(-1)).toMatchObject({ text: "ok", label: "No line waiting" });
  });

  it("highlights alarms", () => {
    const [entry] = log(tx(0, "ALARM:2\r\n"));
    expect(entry.error).toBe(true);
    expect(segments(entry).some((segment) => segment.tone === "error")).toBe(true);
  });
});

describe("realtime bytes", () => {
  it("names GRBL's realtime commands", () => {
    expect(realtimeName(0x3f)).toBe("Status report");
    expect(realtimeName(0x21)).toBe("Feed hold");
    expect(realtimeName(0x7e)).toBe("Cycle start");
    expect(realtimeName(0x18)).toBe("Soft reset");
    expect(realtimeName(0x85)).toBe("Jog cancel");
    expect(realtimeName(0x91)).toBe("Feed +10%");
    expect(realtimeName(0x97)).toBe("Rapid 25%");
    expect(realtimeName(0x9e)).toBe("Spindle stop");
    expect(realtimeName(0xa0)).toBe("Flood toggle");
    expect(realtimeName(0x98)).toBe("Ignored 0x98");
    expect(realtimeName(0x47)).toBeUndefined();
    expect(realtimeName(0x0a)).toBeUndefined();
  });

  it("shows realtime bytes inline without breaking the line they interrupt", () => {
    const entries = log(rx(0, [...bytes("G1X"), 0x21, 0x85, ...bytes("1\n")]), tx(1, "ok\r\n"));
    expect(pick(entries)).toEqual([
      { kind: "realtime", text: "!", label: "Feed hold", response: undefined, error: false },
      { kind: "realtime", text: "\\x85", label: "Jog cancel", response: undefined, error: false },
      { kind: "line", text: "G1X1\\n", label: undefined, response: "ok", error: false },
    ]);
    expect(lineText(entries[1])).toBe("0:00:00.000  → [Jog cancel] \\x85");
  });
});

describe("RX overflow", () => {
  it("shows an overflow from the emulator as an error, and pairs what GRBL kept", () => {
    // The last 6 buffered bytes were lost; the realtime `?` was not.
    const entries = log(rx(0, "G1X1\nG1?X2\nG1X3\n", 6), tx(1, "ok\r\n"));
    expect(pick(entries)).toEqual([
      { kind: "line", text: "G1X1\\n", label: undefined, response: "ok", error: false },
      { kind: "status", text: "?", label: "Status report", response: undefined, error: false },
      {
        kind: "overflow",
        text: "RX buffer overflow: 6 bytes lost: \\nG1X3\\n",
        label: undefined,
        response: undefined,
        error: true,
      },
    ]);
    // GRBL's line ended up as `G1X2`, losing its newline, so it carries on
    // with the next bytes it accepts.
    const protocol = new ProtocolLog();
    protocol.apply([rx(0, "G1X2\nG1X3\n", 6), rx(1, "Y\n")]);
    expect(protocol.entries.map((entry) => entry.text)).toEqual([
      "RX buffer overflow: 6 bytes lost: \\nG1X3\\n",
      "G1X2Y\\n",
    ]);
  });
});

describe("filters and export", () => {
  const entries = log(
    at(0, { kind: "note", text: "Client 127.0.0.1:5000 connected" }),
    rx(1, "?G0X1\n"),
    tx(2, "<Idle|MPos:0.000,0.000,0.000>\r\nok\r\n"),
    at(3, { kind: "state", from: "Idle", to: "Run" }),
    rx(4, "G0X2\n", 5),
  );

  it("hides status reports by default and filters by type and search", () => {
    const shown = visible(entries, defaultFilter);
    expect(shown.map((entry) => entry.kind)).toEqual(["event", "line", "event", "overflow"]);
    const linesOnly = visible(entries, {
      kinds: { lines: true, realtime: false, status: false, messages: false, events: false },
      search: "",
    });
    // Overflows always show: they are errors.
    expect(linesOnly.map((entry) => entry.kind)).toEqual(["line", "overflow"]);
    expect(visible(entries, { ...defaultFilter, search: "idle → run" }).map(lineText)).toEqual([
      "0:00:00.003  • State Idle → Run",
    ]);
  });

  it("exports exactly the lines shown", () => {
    const shown = visible(entries, defaultFilter);
    expect(exportText(shown).split("\n").slice(0, -1)).toEqual(shown.map(lineText));
    expect(exportText(shown)).toBe(
      [
        "0:00:00.000  • Client 127.0.0.1:5000 connected",
        "0:00:00.001  → G0X1\\n  ← ok (1 ms)",
        "0:00:00.003  • State Idle → Run",
        "0:00:00.004  ✕ RX buffer overflow: 5 bytes lost: G0X2\\n",
        "",
      ].join("\n"),
    );
  });
});

describe("faults", () => {
  it("shows injected faults as error events", () => {
    const entries = log(at(7, { kind: "fault", text: "alarm 1" }));
    expect(pick(entries)).toEqual([
      {
        kind: "event",
        text: "Fault injected: alarm 1",
        label: undefined,
        response: undefined,
        error: true,
      },
    ]);
    expect(lineText(entries[0])).toBe("0:00:00.007  • Fault injected: alarm 1");
  });
});

describe("log capacity", () => {
  it("keeps the newest entries and shows answers to lines that scrolled out", () => {
    const protocol = new ProtocolLog(2);
    protocol.apply([rx(0, "A\nB\nC\n"), tx(1, "ok\r\n")]);
    expect(pick(protocol.entries)).toEqual([
      { kind: "line", text: "C\\n", label: undefined, response: undefined, error: false },
      { kind: "message", text: "ok", label: undefined, response: undefined, error: false },
    ]);
  });
});
