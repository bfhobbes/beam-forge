import type { Accessories } from "#lib/bindings/Accessories.js";
import type { MachineState } from "#lib/bindings/MachineState.js";
import type { Position } from "#lib/bindings/Position.js";

const HOLD = ["Hold · ready", "Hold · stopping"];
const DOOR = ["Door · ready", "Door open", "Door · parking", "Door · restoring"];

/** GRBL's machine state, as the status bar and Machine panel show it. */
export function stateLabel(state: MachineState): string {
  switch (state.name) {
    case "idle":
      return "Idle";
    case "run":
      return "Running";
    case "hold":
      return HOLD[state.code] ?? `Hold:${state.code}`;
    case "jog":
      return "Jogging";
    case "alarm":
      return "Alarm";
    case "door":
      return DOOR[state.code] ?? `Door:${state.code}`;
    case "check":
      return "Check mode";
    case "home":
      return "Homing";
    case "sleep":
      return "Sleep";
  }
}

/** States that need the user's attention before the machine will run. */
export function isAlert(state: MachineState): boolean {
  return state.name === "alarm" || state.name === "door";
}

export const mm = (value: number) => value.toFixed(2);

/** `X 12.50 Y 3.00 Z 0.00`, or a dash before the position is known. */
export function formatPosition(position: Position | null | undefined): string {
  if (!position) return "—";
  return `X ${mm(position.x)} Y ${mm(position.y)} Z ${mm(position.z)}`;
}

/** Air assist is wired to flood (M8) or mist (M7) on diode lasers. */
export function airAssist(accessories: Accessories | null): boolean | null {
  return accessories ? accessories.flood || accessories.mist : null;
}
