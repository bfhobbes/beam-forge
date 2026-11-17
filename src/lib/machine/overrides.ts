/**
 * Feed and laser power overrides as sliders. GRBL moves an override in 10%
 * and 1% steps within 10-200% (`machine_override` sends them); the slider
 * shows what was asked for until a status report shows the machine got
 * there, then what the machine reports.
 */
import type { OverrideKind } from "#lib/bindings/OverrideKind.js";
import type { Overrides } from "#lib/bindings/Overrides.js";

/** GRBL's override limits and default, percent. */
export const OVERRIDE_MIN = 10;
export const OVERRIDE_MAX = 200;
export const OVERRIDE_DEFAULT = 100;

export const OVERRIDE_LABELS: Record<OverrideKind, string> = {
  feed: "Feed",
  spindle: "Power",
};

export function clampOverride(percent: number): number {
  return Math.min(OVERRIDE_MAX, Math.max(OVERRIDE_MIN, Math.round(percent)));
}

/** What the machine last reported for `kind`, or null before any `Ov:` report. */
export function reportedOverride(
  overrides: Overrides | null | undefined,
  kind: OverrideKind,
): number | null {
  if (!overrides) return null;
  return kind === "feed" ? overrides.feed : overrides.spindle;
}

/** The slider's position: the target while the machine is on its way, else what it reports. */
export function sliderValue(target: number | undefined, reported: number | null): number {
  if (target !== undefined && target !== reported) return target;
  return reported ?? OVERRIDE_DEFAULT;
}
