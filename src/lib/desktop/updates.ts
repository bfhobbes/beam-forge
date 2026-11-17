import { writable } from "svelte/store";
import type { DownloadEvent } from "@tauri-apps/plugin-updater";

export interface UpdatePackage {
  version: string;
  body?: string;
  download(progress: (event: DownloadEvent) => void, options: { timeout: number }): Promise<void>;
  install(): Promise<void>;
  close(): Promise<void>;
}
export interface UpdateProvider {
  check(): Promise<UpdatePackage | null>;
  relaunch(): Promise<void>;
}
export type UpdateStage =
  | "idle"
  | "checking"
  | "current"
  | "available"
  | "downloading"
  | "ready"
  | "installing"
  | "restart"
  | "error";
export interface UpdateState {
  stage: UpdateStage;
  version: string;
  notes: string;
  received: number;
  total?: number;
  error: string;
}

// UI-independent lifecycle: the application supplies its save/busy gate.
export function createUpdater(
  provider: UpdateProvider,
  acquireInstallLock: () => (() => void) | null,
) {
  let value: UpdateState = { stage: "idle", version: "", notes: "", received: 0, error: "" };
  const state = writable(value);
  let update: UpdatePackage | null = null;
  let disposed = false;
  let inFlight = false;
  const set = (patch: Partial<UpdateState>) => {
    value = { ...value, ...patch };
    state.set(value);
  };
  const release = async () => {
    const old = update;
    update = null;
    await old?.close();
  };
  async function check() {
    if (disposed || inFlight || update) return;
    inFlight = true;
    set({ stage: "checking", error: "" });
    try {
      update = await provider.check();
      if (disposed) {
        await release();
        return;
      }
      set(
        update
          ? { stage: "available", version: update.version, notes: update.body ?? "" }
          : { stage: "current" },
      );
    } catch (cause) {
      set({ stage: "error", error: String(cause) });
    } finally {
      inFlight = false;
    }
  }
  async function download() {
    if (disposed || inFlight || !update || value.stage !== "available") return;
    inFlight = true;
    set({ stage: "downloading", received: 0, total: undefined, error: "" });
    try {
      await update.download(
        (event) => {
          if (event.event === "Started") set({ total: event.data.contentLength });
          if (event.event === "Progress")
            set({ received: value.received + event.data.chunkLength });
        },
        { timeout: 300_000 },
      );
      // Finished means transfer complete; only resolution confirms signature verification.
      set({ stage: "ready" });
    } catch (cause) {
      set({ stage: "available", error: String(cause) });
    } finally {
      inFlight = false;
      if (disposed) await release();
    }
  }
  async function install() {
    if (disposed || inFlight || !update || value.stage !== "ready") return;
    const unlock = acquireInstallLock();
    if (!unlock) {
      set({ error: "Save your changes and wait for current work to finish before installing." });
      return;
    }
    inFlight = true;
    set({ stage: "installing", error: "" });
    let installed = false;
    try {
      await update.install();
      installed = true;
      set({ stage: "restart" });
      await provider.relaunch();
    } catch (cause) {
      set({ stage: installed ? "restart" : "ready", error: String(cause) });
    } finally {
      inFlight = false;
      unlock();
    }
  }
  return {
    state: { subscribe: state.subscribe },
    check,
    download,
    install,
    async restart() {
      if (inFlight || value.stage !== "restart") return;
      const unlock = acquireInstallLock();
      if (!unlock) return;
      inFlight = true;
      try {
        await provider.relaunch();
      } catch (cause) {
        set({ error: String(cause) });
      } finally {
        inFlight = false;
        unlock();
      }
    },
    async dispose() {
      disposed = true;
      if (!inFlight) await release();
    },
  };
}
