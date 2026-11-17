import type { Speed } from "#lib/bindings/Speed.js";

export const speeds: { id: Speed; label: string }[] = [
  { id: "x1", label: "1x" },
  { id: "x10", label: "10x" },
  { id: "x100", label: "100x" },
  { id: "instant", label: "Instant" },
];

/** A coordinate as GRBL reports it, e.g. `12.500`. */
export function coordinate(value: number): string {
  return (Object.is(value, -0) ? 0 : value).toFixed(3);
}

/** Simulated clock as `h:mm:ss.s`. */
export function clock(ms: number): string {
  const tenths = Math.floor(ms / 100);
  const hours = Math.floor(tenths / 36000);
  const minutes = Math.floor(tenths / 600) % 60;
  const seconds = ((tenths % 600) / 10).toFixed(1).padStart(4, "0");
  return `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`;
}

/** Simulated clock to the millisecond, as `h:mm:ss.mmm`. */
export function stamp(ms: number): string {
  const whole = Math.max(0, Math.floor(ms));
  const hours = Math.floor(whole / 3_600_000);
  const minutes = Math.floor(whole / 60_000) % 60;
  const seconds = Math.floor(whole / 1000) % 60;
  const millis = whole % 1000;
  return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${String(millis).padStart(3, "0")}`;
}

/** Parses a TCP port typed by the user, or returns null if it isn't one. */
export function parsePort(text: string): number | null {
  if (!/^\d+$/.test(text.trim())) return null;
  const port = Number(text);
  return port >= 1 && port <= 65535 ? port : null;
}
