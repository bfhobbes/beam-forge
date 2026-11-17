import type { ConnectionState } from "#lib/bindings/ConnectionState.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import type { PortInfo } from "#lib/bindings/PortInfo.js";
import { machineAdapter, PREVIEW_UNAVAILABLE, type MachineAdapter } from "./adapter.js";

export const BAUD_RATES = [9600, 19200, 38400, 57600, 115200, 230400, 250000] as const;
export const DEFAULT_BAUD = 115200;

const STORAGE_KEY = "beam-forge.machine";

type Settings = Pick<Storage, "getItem" | "setItem">;

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The machine connection as the UI sees it: the port list, the chosen port and
 * baud rate (remembered between sessions), and the live connection state.
 * The status bar reads `state`, `statusText` and the live `status`.
 */
export class MachineConnection {
  ports = $state<PortInfo[]>([]);
  portKey = $state("");
  baud = $state(DEFAULT_BAUD);
  state = $state<ConnectionState>({ status: "disconnected", reason: null });
  /** The latest polled status report while connected, else null. */
  status = $state.raw<MachineStatus | null>(null);
  /** The last failed action (listing ports, connecting), until the next one. */
  error = $state("");
  refreshing = $state(false);

  readonly #adapter: MachineAdapter;
  readonly #settings: Settings | undefined;

  constructor(adapter: MachineAdapter, settings?: Settings) {
    this.#adapter = adapter;
    this.#settings = settings;
    try {
      const saved = JSON.parse(settings?.getItem(STORAGE_KEY) ?? "{}");
      if (typeof saved.portKey === "string") this.portKey = saved.portKey;
      if (BAUD_RATES.includes(saved.baud)) this.baud = saved.baud;
    } catch {
      // Unreadable settings: start from the defaults.
    }
  }

  get available(): boolean {
    return this.#adapter.available;
  }

  get selectedPort(): PortInfo | undefined {
    return this.ports.find((port) => port.key === this.portKey);
  }

  get connected(): boolean {
    return this.state.status === "connected";
  }

  get statusText(): string {
    switch (this.state.status) {
      case "connected":
        return `Connected · ${this.state.machine.name} ${this.state.machine.version}`;
      case "connecting":
        return `Connecting to ${this.state.port.label}…`;
      case "disconnected":
        return "Disconnected";
    }
  }

  /** Why the last attempt or connection ended, if it wasn't the user's choice. */
  get problem(): string {
    if (this.error) return this.error;
    return this.state.status === "disconnected" ? (this.state.reason ?? "") : "";
  }

  /** Follows backend state and loads the port list. Returns the cleanup. */
  async start(): Promise<() => void> {
    const stopState = await this.#adapter.onState((state) => this.#setState(state));
    const stopStatus = await this.#adapter.onStatus((status) => {
      if (this.connected) this.status = status;
    });
    this.#setState(await this.#adapter.state());
    await this.refresh();
    return () => {
      stopState();
      stopStatus();
    };
  }

  async refresh(): Promise<void> {
    this.refreshing = true;
    this.error = "";
    try {
      this.ports = await this.#adapter.ports();
      if (!this.selectedPort && this.ports.length > 0) {
        // Keep the remembered port in settings until the user picks another.
        this.portKey = this.ports[0].key;
      }
    } catch (cause) {
      this.ports = [];
      this.error = message(cause);
    } finally {
      this.refreshing = false;
    }
  }

  select(portKey: string): void {
    this.portKey = portKey;
    this.#save();
  }

  setBaud(baud: number): void {
    this.baud = baud;
    this.#save();
  }

  async connect(): Promise<void> {
    this.error = "";
    const port = this.selectedPort;
    if (!port) {
      this.error = this.#adapter.available ? "Choose a port first." : PREVIEW_UNAVAILABLE;
      return;
    }
    this.#save();
    try {
      await this.#adapter.connect(port, this.baud);
    } catch (cause) {
      this.error = message(cause);
    }
  }

  async disconnect(): Promise<void> {
    this.error = "";
    try {
      await this.#adapter.disconnect();
    } catch (cause) {
      this.error = message(cause);
    }
  }

  #setState(state: ConnectionState): void {
    this.state = state;
    if (state.status !== "connected") this.status = null;
  }

  #save(): void {
    this.#settings?.setItem(
      STORAGE_KEY,
      JSON.stringify({ portKey: this.portKey, baud: this.baud }),
    );
  }
}

export const machine = new MachineConnection(machineAdapter, globalThis.window?.localStorage);
