import { invoke } from "@tauri-apps/api/core";
import type { RealHardware } from "#lib/bindings/RealHardware.js";
import { desktop } from "#lib/desktop/platform.js";
import { PREVIEW_UNAVAILABLE } from "./adapter.js";

/** Typed access to the real-hardware gate in `src-tauri/src/machine/safety.rs`. */
export interface SafetyAdapter {
  realHardware(): Promise<RealHardware>;
  /** Switches real hardware on (needs the notice version accepted) or off. */
  setRealHardware(enabled: boolean, acceptedNotice?: number): Promise<RealHardware>;
}

export const desktopSafety: SafetyAdapter = {
  realHardware: () => invoke<RealHardware>("machine_real_hardware"),
  setRealHardware: (enabled, acceptedNotice) =>
    invoke<RealHardware>("machine_set_real_hardware", {
      enabled,
      acceptedNotice: acceptedNotice ?? null,
    }),
};

// The browser preview can't reach a laser, so real hardware stays off.
export const previewSafety: SafetyAdapter = {
  realHardware: async () => ({ enabled: false, noticeVersion: 0 }),
  setRealHardware: async () => {
    throw new Error(PREVIEW_UNAVAILABLE);
  },
};

export const safetyAdapter = desktop ? desktopSafety : previewSafety;
