import { describe, expect, it, vi } from "vite-plus/test";
import type { ConnectionState } from "#lib/bindings/ConnectionState.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { PortInfo } from "#lib/bindings/PortInfo.js";
import { PREVIEW_UNAVAILABLE, previewMachine, type MachineAdapter } from "./adapter.js";
import { DEFAULT_BAUD, MachineConnection } from "./connection.svelte.js";

const simulator: PortInfo = {
  key: "tcp:127.0.0.1:23232",
  address: { kind: "tcp", host: "127.0.0.1", port: 23232 },
  label: "Simulator (127.0.0.1:23232)",
  description: null,
  vid: null,
  pid: null,
};
const phecda: PortInfo = {
  key: "serial:COM3",
  address: { kind: "serial", path: "COM3" },
  label: "COM3 · USB-SERIAL CH340",
  description: "USB-SERIAL CH340",
  vid: 0x1a86,
  pid: 0x7523,
};

function status(x: number): MachineStatus {
  return {
    state: { name: "run" },
    machinePosition: { x, y: 0, z: 0 },
    workPosition: { x, y: 0, z: 0 },
    workOffset: { x: 0, y: 0, z: 0 },
    feed: 600,
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
}

function memory(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    values,
  };
}

function fakeAdapter(ports: PortInfo[] = [phecda, simulator]) {
  let emit: (state: ConnectionState) => void = () => {};
  let report: (status: MachineStatus) => void = () => {};
  const adapter = {
    available: true,
    ports: vi.fn(async () => ports),
    state: vi.fn(async (): Promise<ConnectionState> => ({ status: "disconnected", reason: null })),
    connect: vi.fn(async (port: PortInfo) => {
      const machine = { firmware: "grbl", name: "Grbl", version: "1.1h", banner: "" } as const;
      emit({ status: "connected", port, machine });
      return machine;
    }),
    disconnect: vi.fn(async () => emit({ status: "disconnected", reason: null })),
    onState: vi.fn(async (handler: (state: ConnectionState) => void) => {
      emit = handler;
      return () => {};
    }),
    send: vi.fn(async () => {}),
    softReset: vi.fn(async () => {}),
    onLine: vi.fn(async () => () => {}),
    onStatus: vi.fn(async (handler: (status: MachineStatus) => void) => {
      report = handler;
      return () => {};
    }),
  } satisfies MachineAdapter;
  return {
    adapter,
    emit: (state: ConnectionState) => emit(state),
    report: (status: MachineStatus) => report(status),
  };
}

describe("machine connection", () => {
  it("lists ports and selects the first when nothing is remembered", async () => {
    const { adapter } = fakeAdapter();
    const machine = new MachineConnection(adapter, memory());
    await machine.start();
    expect(machine.ports.map((p) => p.key)).toEqual([phecda.key, simulator.key]);
    expect(machine.portKey).toBe(phecda.key);
    expect(machine.baud).toBe(DEFAULT_BAUD);
    expect(machine.statusText).toBe("Disconnected");
  });

  it("remembers the last port and baud rate", async () => {
    const settings = memory();
    const { adapter } = fakeAdapter();
    const first = new MachineConnection(adapter, settings);
    await first.start();
    first.select(simulator.key);
    first.setBaud(250000);

    const next = new MachineConnection(adapter, settings);
    await next.start();
    expect(next.portKey).toBe(simulator.key);
    expect(next.baud).toBe(250000);
  });

  it("connects to the chosen port and reports the firmware", async () => {
    const { adapter } = fakeAdapter();
    const machine = new MachineConnection(adapter, memory());
    await machine.start();
    machine.setBaud(57600);
    await machine.connect();
    expect(adapter.connect).toHaveBeenCalledWith(phecda, 57600);
    expect(machine.connected).toBe(true);
    expect(machine.statusText).toBe("Connected · Grbl 1.1h");
    await machine.disconnect();
    expect(machine.statusText).toBe("Disconnected");
    expect(machine.problem).toBe("");
  });

  it("shows a failed connect as a problem", async () => {
    const { adapter } = fakeAdapter();
    adapter.connect.mockRejectedValueOnce("Couldn't open COM3: Access is denied.");
    const machine = new MachineConnection(adapter, memory());
    await machine.start();
    await machine.connect();
    expect(machine.problem).toBe("Couldn't open COM3: Access is denied.");
  });

  it("surfaces an unexpected disconnect from the backend", async () => {
    const { adapter, emit } = fakeAdapter();
    const machine = new MachineConnection(adapter, memory());
    await machine.start();
    await machine.connect();
    emit({ status: "disconnected", reason: "Connection lost: COM3 disappeared" });
    expect(machine.statusText).toBe("Disconnected");
    expect(machine.problem).toBe("Connection lost: COM3 disappeared");
  });

  it("follows live status while connected and clears it on disconnect", async () => {
    const { adapter, emit, report } = fakeAdapter();
    const machine = new MachineConnection(adapter, memory());
    await machine.start();
    report(status(1));
    expect(machine.status).toBeNull();

    await machine.connect();
    report(status(1));
    report(status(2.5));
    expect(machine.status?.workPosition?.x).toBe(2.5);

    emit({ status: "disconnected", reason: "Connection lost" });
    expect(machine.status).toBeNull();
  });

  it("asks for a port when none is available", async () => {
    const { adapter } = fakeAdapter([]);
    const machine = new MachineConnection(adapter, memory());
    await machine.start();
    await machine.connect();
    expect(adapter.connect).not.toHaveBeenCalled();
    expect(machine.problem).toBe("Choose a port first.");
  });

  it("says plainly that the browser preview can't connect", async () => {
    const machine = new MachineConnection(previewMachine, memory());
    await machine.start();
    expect(machine.available).toBe(false);
    expect(machine.ports).toEqual([]);
    await machine.connect();
    expect(machine.problem).toBe(PREVIEW_UNAVAILABLE);
  });

  it("shows a port listing failure", async () => {
    const { adapter } = fakeAdapter();
    adapter.ports.mockRejectedValueOnce(new Error("Couldn't list serial ports: denied"));
    const machine = new MachineConnection(adapter, memory());
    await machine.start();
    expect(machine.ports).toEqual([]);
    expect(machine.problem).toBe("Couldn't list serial ports: denied");
  });
});
