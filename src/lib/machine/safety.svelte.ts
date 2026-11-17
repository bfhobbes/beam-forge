import { safetyAdapter, type SafetyAdapter } from "./safety-adapter.js";

/**
 * The version of the safety notice in `SafetyNotice.svelte`. It must match
 * `NOTICE_VERSION` in `src-tauri/src/machine/safety.rs`: bump both when the
 * notice changes in a way users must read again.
 */
export const NOTICE_VERSION = 1;

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/**
 * The "Real laser" switch. While it is off the backend refuses to connect to
 * any port that may be a real laser (serial, or TCP to another computer);
 * turning it on needs the safety notice accepted.
 */
export class RealHardwareSwitch {
  enabled = $state(false);
  /** The last failed load or change, until the next one. */
  error = $state("");
  saving = $state(false);

  readonly #adapter: SafetyAdapter;

  constructor(adapter: SafetyAdapter) {
    this.#adapter = adapter;
  }

  async load(): Promise<void> {
    try {
      this.enabled = (await this.#adapter.realHardware()).enabled;
      this.error = "";
    } catch (cause) {
      this.error = message(cause);
    }
  }

  /** Turns it on, recording that the user accepted the notice. */
  accept(): Promise<boolean> {
    return this.#set(true, NOTICE_VERSION);
  }

  /** Turns it off; turning it on again shows the notice again. */
  turnOff(): Promise<boolean> {
    return this.#set(false);
  }

  async #set(enabled: boolean, acceptedNotice?: number): Promise<boolean> {
    this.saving = true;
    this.error = "";
    try {
      this.enabled = (await this.#adapter.setRealHardware(enabled, acceptedNotice)).enabled;
      return true;
    } catch (cause) {
      this.error = message(cause);
      return false;
    } finally {
      this.saving = false;
    }
  }
}

export const realHardware = new RealHardwareSwitch(safetyAdapter);
