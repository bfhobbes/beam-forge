// The fault panel's vocabulary. Faults travel to the emulator in
// bf-sim's text form (`alarm 1`, `error 20 line 3`, `stall 2s`), the same
// text a scenario script uses, so the panel and Rust tests share one parser.

export interface Code {
  readonly code: number;
  readonly name: string;
}

/** GRBL 1.1 alarm codes (gnea/grbl doc/csv/alarm_codes_en_US.csv). */
export const alarms: readonly Code[] = [
  { code: 1, name: "Hard limit" },
  { code: 2, name: "Soft limit" },
  { code: 3, name: "Abort during cycle" },
  { code: 4, name: "Probe fail: initial state" },
  { code: 5, name: "Probe fail: no contact" },
  { code: 6, name: "Homing fail: reset" },
  { code: 7, name: "Homing fail: door" },
  { code: 8, name: "Homing fail: pull-off" },
  { code: 9, name: "Homing fail: no switch" },
];

/** GRBL 1.1 error codes (gnea/grbl doc/csv/error_codes_en_US.csv). 18 and 19 are unused. */
export const errors: readonly Code[] = [
  { code: 1, name: "Expected command letter" },
  { code: 2, name: "Bad number format" },
  { code: 3, name: "Invalid statement" },
  { code: 4, name: "Value < 0" },
  { code: 5, name: "Setting disabled" },
  { code: 6, name: "Value < 3 µs" },
  { code: 7, name: "EEPROM read fail" },
  { code: 8, name: "Not idle" },
  { code: 9, name: "G-code lock" },
  { code: 10, name: "Homing not enabled" },
  { code: 11, name: "Line overflow" },
  { code: 12, name: "Step rate > 30 kHz" },
  { code: 13, name: "Check door" },
  { code: 14, name: "Line length exceeded" },
  { code: 15, name: "Travel exceeded" },
  { code: 16, name: "Invalid jog command" },
  { code: 17, name: "Laser mode needs PWM" },
  { code: 20, name: "Unsupported command" },
  { code: 21, name: "Modal group violation" },
  { code: 22, name: "Undefined feed rate" },
  { code: 23, name: "Command value not integer" },
  { code: 24, name: "Axis command conflict" },
  { code: 25, name: "Word repeated" },
  { code: 26, name: "No axis words" },
  { code: 27, name: "Invalid line number" },
  { code: 28, name: "Value word missing" },
  { code: 29, name: "G59.x not supported" },
  { code: 30, name: "G53 needs G0 or G1" },
  { code: 31, name: "Axis words unused" },
  { code: 32, name: "G2/G3 need in-plane axis" },
  { code: 33, name: "Invalid motion target" },
  { code: 34, name: "Invalid arc radius" },
  { code: 35, name: "G2/G3 need offset words" },
  { code: 36, name: "Value words unused" },
  { code: 37, name: "G43.1 offset not on tool axis" },
  { code: 38, name: "Tool number > max" },
];

/** A starting point for the scenario editor; also documents the syntax. */
export const exampleScenario = `# Lines count from when the scenario starts.
# "after N:" fires once the host's Nth line has been answered;
# a fault without it fires straight away.
latency 10ms
after 3: error 20   # the 4th line gets error:20 instead of running
after 4: alarm 1    # then a hard limit trips
`;

/** A whole, non-negative number typed by the user, or null. */
export function wholeNumber(text: string): number | null {
  const trimmed = text.trim();
  return /^\d+$/.test(trimmed) ? Number(trimmed) : null;
}
