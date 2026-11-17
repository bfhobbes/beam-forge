import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ConnectionState } from "#lib/bindings/ConnectionState.js";
import type { MachineInfo } from "#lib/bindings/MachineInfo.js";
import type { MachineLine } from "#lib/bindings/MachineLine.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { PortInfo } from "#lib/bindings/PortInfo.js";
import { desktop } from "#lib/desktop/platform.js";

/** Typed access to the machine commands and events in `src-tauri/src/machine`. */
export interface MachineAdapter {
  /** False in the browser preview, which can't open ports. */
  readonly available: boolean;
  ports(): Promise<PortInfo[]>;
  state(): Promise<ConnectionState>;
  connect(port: PortInfo, baud: number): Promise<MachineInfo>;
  disconnect(): Promise<void>;
  onState(handler: (state: ConnectionState) => void): Promise<() => void>;
  /** Sends one command line; the backend adds the line ending. */
  send(line: string): Promise<void>;
  /** Soft-resets the controller, aborting any motion. */
  softReset(): Promise<void>;
  /** Each line the controller sends, decoded (status reports come via `onStatus`). */
  onLine(handler: (line: MachineLine) => void): Promise<() => void>;
  /** Polled status reports, about 5 Hz while connected (10 Hz while moving). */
  onStatus(handler: (status: MachineStatus) => void): Promise<() => void>;
}

export const desktopMachine: MachineAdapter = {
  available: true,
  ports: () => invoke<PortInfo[]>("machine_ports"),
  state: () => invoke<ConnectionState>("machine_state"),
  connect: (port, baud) => invoke<MachineInfo>("machine_connect", { port, baud }),
  disconnect: () => invoke<void>("machine_disconnect"),
  onState: (handler) => listen<ConnectionState>("machine:state", (event) => handler(event.payload)),
  send: (line) => invoke<void>("machine_send", { line }),
  softReset: () => invoke<void>("machine_soft_reset"),
  onLine: (handler) => listen<MachineLine>("machine:line", (event) => handler(event.payload)),
  onStatus: (handler) => listen<MachineStatus>("machine:status", (event) => handler(event.payload)),
};

export const PREVIEW_UNAVAILABLE =
  "Browser preview: machine connection needs the desktop app (npm run desktop:dev).";

// The browser preview has no Rust side: no ports, and connecting says why.
export const previewMachine: MachineAdapter = {
  available: false,
  ports: async () => [],
  state: async () => ({ status: "disconnected", reason: null }),
  connect: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  disconnect: async () => {},
  send: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  softReset: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
  onState: async () => () => {},
  onLine: async () => () => {},
  onStatus: async () => () => {},
};

export const machineAdapter = desktop ? desktopMachine : previewMachine;
