import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { MachineSetup } from "#lib/bindings/MachineSetup.js";
import type { OriginCorner } from "#lib/bindings/OriginCorner.js";
import type { SetupStep } from "#lib/bindings/SetupStep.js";
import type { StepOutcome } from "#lib/bindings/StepOutcome.js";
import { newLayer } from "#lib/document/layers.js";
import type { DesignDocument } from "#lib/document/model.js";
import { machineSettings } from "./settings.svelte.js";

export type { MachineSetup } from "#lib/bindings/MachineSetup.js";
export type { SetupStep } from "#lib/bindings/SetupStep.js";

/** The checklist's six steps, in order. `handsOn` is the three hands-on tests. */
export const CHECKLIST = [
  "connected",
  "bed",
  "laserMode",
  "homing",
  "handsOn",
  "airAssist",
] as const;
export type ChecklistStep = (typeof CHECKLIST)[number];

/** The hands-on tests, which make up the `handsOn` step. */
export const HANDS_ON = ["jogTest", "frameTest", "fireTest"] as const satisfies SetupStep[];

export type StepStatus = StepOutcome | "todo";

/** Shown before the test fire, and ticked off before it can fire. */
export const EYE_SAFETY_REMINDER = [
  "Put on laser safety glasses rated for this laser.",
  "Keep anything reflective off the bed.",
  "Keep the machine's power switch within reach.",
] as const;

/** The framed test square: its side, and how far in from the origin corner it sits, in mm. */
export const TEST_SQUARE = { size: 20, inset: 10 } as const;

/** How far the jog test moves each axis, in mm. */
export const JOG_TEST_MM = 10;

function members(step: ChecklistStep): readonly SetupStep[] {
  return step === "handsOn" ? HANDS_ON : [step];
}

/** One saved step (or hands-on test): done, skipped or still to do. */
export function outcome(setup: MachineSetup | null, step: SetupStep): StepStatus {
  return setup?.steps[step] ?? "todo";
}

/**
 * A checklist step's status. The hands-on step is done when all three tests
 * are, skipped once each is done or skipped, and to do until then.
 */
export function stepStatus(setup: MachineSetup | null, step: ChecklistStep): StepStatus {
  const outcomes = members(step).map((member) => outcome(setup, member));
  if (outcomes.includes("todo")) return "todo";
  return outcomes.every((each) => each === "done") ? "done" : "skipped";
}

/** Steps done or skipped, out of six. */
export function completedSteps(setup: MachineSetup | null): number {
  return CHECKLIST.filter((step) => stepStatus(setup, step) !== "todo").length;
}

export function setupFinished(setup: MachineSetup | null): boolean {
  return completedSteps(setup) === CHECKLIST.length;
}

/** Which way the head should go on +X and +Y, seen from the front, for the origin corner. */
export function jogExpectation(origin: OriginCorner): { x: string; y: string } {
  const right = origin === "frontRight" || origin === "rearRight";
  const rear = origin === "rearLeft" || origin === "rearRight";
  return {
    x: right ? "left" : "right",
    y: rear ? "towards you (to the front)" : "away from you (to the back)",
  };
}

/** The origin corner in words, e.g. "front-left". */
export function cornerName(origin: OriginCorner): string {
  const names: Record<OriginCorner, string> = {
    frontLeft: "front-left",
    frontRight: "front-right",
    rearLeft: "rear-left",
    rearRight: "rear-right",
  };
  return names[origin];
}

/**
 * A design holding just the test square, {@link TEST_SQUARE}.inset mm in
 * from the profile's origin corner on both axes. Design coordinates measure
 * from the bed's front-left corner, so a square by another corner sits
 * against the far side of the bed.
 */
export function testSquare(profile: Pick<MachineProfile, "bed" | "origin">): DesignDocument {
  const { size, inset } = TEST_SQUARE;
  const far = (length: number) => length - inset - size;
  const x = profile.origin.endsWith("Right") ? far(profile.bed.width) : inset;
  const y = profile.origin.startsWith("rear") ? far(profile.bed.height) : inset;
  const layer = { ...newLayer(0, "setup-square"), name: "Test square", airAssist: false };
  return {
    version: 1,
    units: "mm",
    layers: [layer],
    objects: [
      {
        type: "rect",
        id: "setup-square",
        name: "Test square",
        layerId: layer.id,
        transform: [1, 0, 0, 1, x, y],
        width: size,
        height: size,
        cornerRadius: 0,
      },
    ],
  };
}

/** What the checklist needs from the settings store: the active profile, and saving it. */
export interface ProfileView {
  readonly profile: MachineProfile | null;
  saveProfile(changes: Partial<MachineProfile>): Promise<boolean>;
}

/**
 * The "Set up this machine" checklist: which steps are done or skipped, and
 * whether it shows. Progress lives in the machine profile (`setup`), so it
 * resumes on the next connect. It opens by itself when a machine whose
 * checklist isn't finished or dismissed connects; a new machine's profile
 * starts with an empty checklist, while profiles from before the checklist
 * have none and only get it when the user asks.
 */
export class SetupChecklist {
  /** Showing in full, rather than as one line. */
  open = $state(false);
  /** The step showing its details. */
  expanded = $state<ChecklistStep | null>(null);

  readonly #profiles: ProfileView;
  /** The connected machine's port, or null. */
  #port: string | null = null;
  /** The profile already offered on this connection. */
  #offeredFor: string | null = null;
  /** Saves run in order, each on top of the last. */
  #saving: Promise<unknown> = Promise.resolve();

  constructor(profiles: ProfileView) {
    this.#profiles = profiles;
  }

  get setup(): MachineSetup | null {
    return this.#profiles.profile?.setup ?? null;
  }

  /** There is a machine profile to set up. */
  get available(): boolean {
    return this.#profiles.profile !== null;
  }

  get completed(): number {
    return completedSteps(this.setup);
  }

  get finished(): boolean {
    return setupFinished(this.setup);
  }

  status(step: ChecklistStep): StepStatus {
    return stepStatus(this.setup, step);
  }

  outcome(step: SetupStep): StepStatus {
    return outcome(this.setup, step);
  }

  /**
   * Follows the connection (`port`, the connected port's key, or null) and
   * the active profile. Once per connection, when the profile read for that
   * machine arrives, it opens the checklist if it is still to do and ticks
   * off "Connected".
   */
  follow(port: string | null, profile: MachineProfile | null): void {
    this.#port = port;
    if (port === null) {
      this.#offeredFor = null;
      return;
    }
    // Until the settings are read, the profile may be another machine's.
    if (!profile || profile.portKey !== port || this.#offeredFor === profile.id) return;
    this.#offeredFor = profile.id;
    const setup = profile.setup;
    if (!setup) return;
    if (!setup.dismissed && !setupFinished(setup)) {
      this.open = true;
      this.expanded = this.#nextToDo(setup);
    }
    if (outcome(setup, "connected") === "todo") void this.resolve("connected", "done");
  }

  /** Records a step (or hands-on test) as done or skipped, and moves on to the next step to do. */
  resolve(step: SetupStep, result: StepOutcome): Promise<boolean> {
    return this.#update((setup) => ({ ...setup, steps: { ...setup.steps, [step]: result } }));
  }

  /** Skips a step: for the hands-on step, every test not yet done or skipped. */
  skip(step: ChecklistStep): Promise<boolean> {
    return this.#update((setup) => {
      const steps = { ...setup.steps };
      for (const member of members(step)) steps[member] ??= "skipped";
      return { ...setup, steps };
    });
  }

  /** Shows the checklist again, from the Machine tab or the command palette. */
  async reopen(): Promise<void> {
    this.open = true;
    this.expanded = this.#nextToDo(this.setup);
    const connected = this.#port !== null && this.#profiles.profile?.portKey === this.#port;
    await this.#update((setup) => {
      const steps = { ...setup.steps };
      if (connected) steps.connected ??= "done";
      return { steps, dismissed: false };
    }, false);
  }

  /** Closes the checklist; it isn't offered on connect again until reopened. */
  async dismiss(): Promise<void> {
    this.open = false;
    await this.#update((setup) => ({ ...setup, dismissed: true }), false);
  }

  toggle(step: ChecklistStep): void {
    this.expanded = this.expanded === step ? null : step;
  }

  #nextToDo(setup: MachineSetup | null): ChecklistStep | null {
    return CHECKLIST.find((step) => stepStatus(setup, step) === "todo") ?? null;
  }

  /** Saves `change` applied to the latest setup; `advance` moves to the next step to do. */
  #update(change: (setup: MachineSetup) => MachineSetup, advance = true): Promise<boolean> {
    const saved = this.#saving.then(async () => {
      const profile = this.#profiles.profile;
      if (!profile) return false;
      const setup = change(profile.setup ?? { steps: {}, dismissed: false });
      const ok = await this.#profiles.saveProfile({ setup });
      if (
        ok &&
        advance &&
        (this.expanded === null || stepStatus(setup, this.expanded) !== "todo")
      ) {
        this.expanded = this.#nextToDo(setup);
      }
      return ok;
    });
    this.#saving = saved;
    return saved;
  }
}

export const machineSetup = new SetupChecklist(machineSettings);
