import { describe, expect, it, vi } from "vite-plus/test";
import { ActionRegistry } from "#lib/actions/registry.svelte.js";
import type { MachineLine } from "#lib/bindings/MachineLine.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import { previewMachine, PREVIEW_UNAVAILABLE, type MachineAdapter } from "./adapter.js";
import { QUICK_COMMANDS, registerConsoleActions } from "./console-actions.js";
import { explain, HISTORY, MachineConsole, SCROLLBACK, statusLine } from "./console.svelte.js";

const idle: MachineStatus = {
  state: { name: "idle" },
  machinePosition: { x: 1, y: 2, z: 0 },
  workPosition: { x: 1, y: 2, z: 0 },
  workOffset: null,
  feed: 0,
  spindle: 0,
  overrides: null,
  buffer: null,
  pins: {
    limitX: false,
    limitY: false,
    limitZ: false,
    probe: false,
    door: false,
    hold: false,
    softReset: false,
    cycleStart: false,
  },
  accessories: null,
  line: null,
};

const error20: MachineLine = {
  line: "error:20",
  reply: {
    kind: "error",
    code: 20,
    title: "Unsupported command",
    description: "Unsupported or invalid G-code command found in block.",
  },
};

function fakeAdapter() {
  let line: (line: MachineLine) => void = () => {};
  let report: (status: MachineStatus) => void = () => {};
  const adapter = {
    ...previewMachine,
    available: true,
    send: vi.fn(async (_line: string) => {}),
    softReset: vi.fn(async () => {}),
    onLine: vi.fn(async (handler: (line: MachineLine) => void) => {
      line = handler;
      return () => {};
    }),
    onStatus: vi.fn(async (handler: (status: MachineStatus) => void) => {
      report = handler;
      return () => {};
    }),
  } satisfies MachineAdapter;
  return {
    adapter,
    receive: (value: MachineLine) => line(value),
    report: (status: MachineStatus) => report(status),
  };
}

const texts = (machineConsole: MachineConsole) => machineConsole.entries.map((e) => e.text);

describe("machine console", () => {
  it("records sent lines and decoded replies", async () => {
    const { adapter, receive } = fakeAdapter();
    const machineConsole = new MachineConsole(adapter);
    await machineConsole.start();

    expect(await machineConsole.send("  G99 ")).toBe(true);
    receive(error20);

    expect(adapter.send).toHaveBeenCalledWith("G99");
    expect(machineConsole.entries.map((e) => e.kind)).toEqual(["sent", "received"]);
    const reply = machineConsole.entries[1];
    expect(reply.kind === "received" && explain(reply.reply)).toBe(
      "Unsupported command: Unsupported or invalid G-code command found in block.",
    );
  });

  it("ignores blank input", async () => {
    const { adapter } = fakeAdapter();
    const machineConsole = new MachineConsole(adapter);
    expect(await machineConsole.send("   ")).toBe(false);
    expect(adapter.send).not.toHaveBeenCalled();
    expect(machineConsole.entries).toEqual([]);
  });

  it("hides status reports until asked to show them", async () => {
    const { adapter, report } = fakeAdapter();
    const machineConsole = new MachineConsole(adapter);
    await machineConsole.start();

    report(idle);
    expect(machineConsole.entries).toEqual([]);

    machineConsole.toggleStatus();
    report(idle);
    expect(texts(machineConsole)).toEqual(["Idle · MPos X 1.00 Y 2.00 Z 0.00 · F 0 S 0"]);

    machineConsole.toggleStatus();
    expect(machineConsole.entries).toEqual([]);
  });

  it("recalls history with up and down, returning to the draft", async () => {
    const { adapter } = fakeAdapter();
    const machineConsole = new MachineConsole(adapter);
    await machineConsole.send("$I");
    await machineConsole.send("$G");
    await machineConsole.send("$G");

    expect(machineConsole.history).toEqual(["$I", "$G"]);
    expect(machineConsole.previous("G0 X")).toBe("$G");
    expect(machineConsole.previous("$G")).toBe("$I");
    expect(machineConsole.previous("$I")).toBe("$I");
    expect(machineConsole.next("$I")).toBe("$G");
    expect(machineConsole.next("$G")).toBe("G0 X");
    expect(machineConsole.next("G0 X")).toBe("G0 X");
  });

  it("caps history and scrollback", async () => {
    const { adapter } = fakeAdapter();
    const machineConsole = new MachineConsole(adapter);
    for (let i = 0; i < SCROLLBACK + 5; i++) await machineConsole.send(`G0 X${i}`);
    expect(machineConsole.history).toHaveLength(HISTORY);
    expect(machineConsole.entries).toHaveLength(SCROLLBACK);
    expect(machineConsole.entries.at(-1)?.text).toBe(`G0 X${SCROLLBACK + 4}`);
  });

  it("notes a failed send", async () => {
    const machineConsole = new MachineConsole(previewMachine);
    expect(await machineConsole.send("$I")).toBe(false);
    expect(machineConsole.entries.at(-1)).toMatchObject({
      kind: "note",
      text: PREVIEW_UNAVAILABLE,
      error: true,
    });
  });

  it("formats work position when an offset is known", () => {
    const offset = {
      ...idle,
      workOffset: { x: 1, y: 0, z: 0 },
      workPosition: { x: 0, y: 2, z: 0 },
    };
    expect(statusLine(offset)).toBe(
      "Idle · MPos X 1.00 Y 2.00 Z 0.00 · WPos X 0.00 Y 2.00 Z 0.00 · F 0 S 0",
    );
  });
});

describe("console actions", () => {
  function setup(connected = true) {
    const { adapter } = fakeAdapter();
    const machineConsole = new MachineConsole(adapter);
    const registry = new ActionRegistry({ mac: false, storage: null });
    registerConsoleActions(machineConsole, () => connected, registry);
    return { adapter, machineConsole, registry };
  }

  it("sends each quick command", () => {
    const { adapter, registry } = setup();
    for (const command of QUICK_COMMANDS) expect(registry.run(command.id)).toBe(true);
    expect(adapter.send.mock.calls.map(([line]) => line)).toEqual(["$$", "$#", "$I", "$G", "$X"]);
  });

  it("soft-resets", () => {
    const { adapter, registry } = setup();
    registry.run("console.soft-reset");
    expect(adapter.softReset).toHaveBeenCalled();
  });

  it("disables machine commands while disconnected", () => {
    const { adapter, registry } = setup(false);
    expect(registry.run("console.build-info")).toBe(false);
    expect(registry.run("console.soft-reset")).toBe(false);
    expect(adapter.send).not.toHaveBeenCalled();
    expect(registry.run("console.toggle-status")).toBe(true);
  });
});
