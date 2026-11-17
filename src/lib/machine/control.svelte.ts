import type { JogDirection } from "#lib/bindings/JogDirection.js";
import type { MachineCommand } from "#lib/bindings/MachineCommand.js";
import type { MachineProfile } from "#lib/bindings/MachineProfile.js";
import type { MachineSetting } from "#lib/bindings/MachineSetting.js";
import type { MachineStatus } from "#lib/bindings/MachineStatus.js";
import { lifecycle as appLifecycle, type Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { controlAdapter, type ControlAdapter } from "./control-adapter.js";
import { machine as appMachine } from "./connection.svelte.js";
import { machineJob } from "./job.svelte.js";
import { machineSettings } from "./settings.svelte.js";
import { airAssist } from "./status.js";

/** Jog step sizes in mm, matching `JOG_STEPS` in `bf-core`. */
export const JOG_STEPS = [0.1, 1, 10, 50] as const;
export const DEFAULT_JOG_FEED = 3000;
/** A press held this long jogs continuously; a shorter click jogs one step. */
export const HOLD_DELAY_MS = 300;
/** How often a held jog is renewed. Rust stops it ~400 ms after the last renewal. */
export const RENEW_MS = 100;

/** Test-fire caps, enforced again in Rust (`TEST_FIRE_MAX_*` in `bf-core`). */
export const TEST_FIRE_MAX_POWER = 10;
export const TEST_FIRE_MAX_MS = 1000;
export const DEFAULT_TEST_FIRE = { power: 1, durationMs: 100 } as const;

/** Why close and updates wait while the laser test fires. */
export const FIRING_REASON = "Laser test fire";

const STORAGE_KEY = "beam-forge.jog";

type Storage = Pick<globalThis.Storage, "getItem" | "setItem">;

/** What the jog pad reads from the connection, job and settings stores. */
export interface MachineView {
  readonly connected: boolean;
  readonly status: MachineStatus | null;
}
export interface JobView {
  readonly active: boolean;
}
export interface SettingsView {
  readonly settings: MachineSetting[];
  readonly profile: MachineProfile | null;
}

/** The jog pad's eight arrows, top row first. */
export const JOG_PAD: readonly (JogDirection & { label: string })[] = [
  { x: -1, y: 1, label: "Jog back left" },
  { x: 0, y: 1, label: "Jog back (Y+)" },
  { x: 1, y: 1, label: "Jog back right" },
  { x: -1, y: 0, label: "Jog left (X−)" },
  { x: 1, y: 0, label: "Jog right (X+)" },
  { x: -1, y: -1, label: "Jog front left" },
  { x: 0, y: -1, label: "Jog front (Y−)" },
  { x: 1, y: -1, label: "Jog front right" },
];

/** The jog direction for an arrow key, or null for any other key. */
export function arrowDirection(key: string): JogDirection | null {
  switch (key) {
    case "ArrowUp":
      return { x: 0, y: 1 };
    case "ArrowDown":
      return { x: 0, y: -1 };
    case "ArrowLeft":
      return { x: -1, y: 0 };
    case "ArrowRight":
      return { x: 1, y: 0 };
    default:
      return null;
  }
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

const positive = (value: number) => Number.isFinite(value) && value > 0;

/**
 * The jog pad: step and held jogs, homing, the work origin, air assist and
 * the laser test fire. Timing that keeps the machine safe (stopping a held
 * jog the UI stops renewing, turning the test fire off) runs in Rust; this
 * only renews, releases and shows state.
 */
export class MachineControl {
  step = $state<number>(1);
  feed = $state(DEFAULT_JOG_FEED);
  power = $state<number>(DEFAULT_TEST_FIRE.power);
  durationMs = $state<number>(DEFAULT_TEST_FIRE.durationMs);
  /** The held jog's direction while one runs. */
  holding = $state<JogDirection | null>(null);
  firing = $state(false);
  /** The last failed command, until the next one. */
  error = $state("");

  readonly #adapter: ControlAdapter;
  readonly #machine: MachineView;
  readonly #job: JobView;
  readonly #settings: SettingsView;
  readonly #lifecycle: Lifecycle;
  readonly #storage: Storage | undefined;
  #pressed: { direction: JogDirection; timer: ReturnType<typeof setTimeout> } | null = null;
  #renew: ReturnType<typeof setInterval> | null = null;

  constructor(
    adapter: ControlAdapter,
    machine: MachineView,
    job: JobView,
    settings: SettingsView,
    lifecycle: Lifecycle,
    storage?: Storage,
  ) {
    this.#adapter = adapter;
    this.#machine = machine;
    this.#job = job;
    this.#settings = settings;
    this.#lifecycle = lifecycle;
    this.#storage = storage;
    try {
      const saved = JSON.parse(storage?.getItem(STORAGE_KEY) ?? "{}");
      if (JOG_STEPS.includes(saved.step)) this.step = saved.step;
      if (positive(saved.feed)) this.feed = saved.feed;
      if (positive(saved.power) && saved.power <= TEST_FIRE_MAX_POWER) this.power = saved.power;
      if (Number.isInteger(saved.durationMs) && saved.durationMs > 0)
        this.durationMs = Math.min(saved.durationMs, TEST_FIRE_MAX_MS);
    } catch {
      // Unreadable settings: start from the defaults.
    }
  }

  get #state(): string | undefined {
    return this.#machine.connected ? this.#machine.status?.state.name : undefined;
  }

  /** Connected, no job running and the laser not test firing. */
  get #free(): boolean {
    return this.#machine.connected && !this.#job.active && !this.firing;
  }

  /** Idle and free: ready to set the origin, test fire or go to the origin. */
  get idle(): boolean {
    return this.#free && this.#state === "idle";
  }

  /** GRBL takes jogs while idle and while already jogging. */
  get canJog(): boolean {
    return this.#free && (this.#state === "idle" || this.#state === "jog");
  }

  /** Homing is configured (`$22=1`), from the settings table or the profile. */
  get homingEnabled(): boolean {
    const row = this.#settings.settings.find((setting) => setting.number === 22);
    if (row) return Number(row.value) === 1;
    return this.#settings.profile?.homing ?? false;
  }

  get canHome(): boolean {
    return this.homingEnabled && this.#free && (this.#state === "idle" || this.#state === "alarm");
  }

  get canUnlock(): boolean {
    return this.#free && this.#state === "alarm";
  }

  /** Air assist as the controller reports it, or null before it has. */
  get airAssist(): boolean | null {
    return this.#machine.connected ? airAssist(this.#machine.status?.accessories ?? null) : null;
  }

  // ---- jogging ----

  /** Jogs one step. */
  async jog({ x, y }: JogDirection): Promise<void> {
    await this.#run(() => this.#adapter.jog({ x, y }, this.step, this.feed));
  }

  /**
   * A jog button went down. Released within {@link HOLD_DELAY_MS} it jogs
   * one step; held longer it jogs until released.
   */
  press({ x, y }: JogDirection): void {
    const direction = { x, y };
    this.cancel();
    if (!this.canJog) return;
    const timer = setTimeout(() => {
      this.#pressed = null;
      void this.#startHold(direction);
    }, HOLD_DELAY_MS);
    this.#pressed = { direction, timer };
  }

  /** The jog button came up: step if it was a click, else stop the held jog. */
  release(): void {
    const pressed = this.#pressed;
    if (pressed) {
      clearTimeout(pressed.timer);
      this.#pressed = null;
      void this.jog(pressed.direction);
    } else {
      this.#stopHold();
    }
  }

  /** The press was lost (focus left, pointer cancelled): stop without stepping. */
  cancel(): void {
    if (this.#pressed) {
      clearTimeout(this.#pressed.timer);
      this.#pressed = null;
    }
    this.#stopHold();
  }

  async #startHold(direction: JogDirection): Promise<void> {
    this.holding = direction;
    const renew = () =>
      this.#adapter.jogHold(direction, this.feed).catch((cause) => {
        this.error = message(cause);
        this.#stopHold();
      });
    this.error = "";
    this.#renew = setInterval(() => void renew(), RENEW_MS);
    await renew();
  }

  #stopHold(): void {
    if (this.#renew !== null) {
      clearInterval(this.#renew);
      this.#renew = null;
    }
    if (this.holding) {
      this.holding = null;
      void this.#adapter.jogStop().catch((cause) => (this.error = message(cause)));
    }
  }

  // ---- settings ----

  setStep(step: number): void {
    if (!JOG_STEPS.includes(step as (typeof JOG_STEPS)[number])) return;
    this.step = step;
    this.#save();
  }

  /** Jog speed in mm/min; false (unchanged) if not a positive number. */
  setFeed(feed: number): boolean {
    if (!positive(feed)) return false;
    this.feed = feed;
    this.#save();
    return true;
  }

  /** Test-fire power in percent; refused above the cap. */
  setPower(power: number): boolean {
    if (!positive(power) || power > TEST_FIRE_MAX_POWER) return false;
    this.power = power;
    this.#save();
    return true;
  }

  /** Test-fire duration in ms; refused above the cap. */
  setDuration(durationMs: number): boolean {
    if (!positive(durationMs) || durationMs > TEST_FIRE_MAX_MS) return false;
    this.durationMs = Math.round(durationMs);
    this.#save();
    return true;
  }

  // ---- commands ----

  async command(command: MachineCommand): Promise<void> {
    await this.#run(() => this.#adapter.command(command, this.#settings.profile));
  }

  goToOrigin(): Promise<void> {
    return this.command({ command: "goToOrigin", feed: this.feed });
  }

  toggleAirAssist(): Promise<void> {
    return this.command({ command: "airAssist", on: !this.airAssist });
  }

  /**
   * Fires the laser. Rust turns it off after the duration; the app stays
   * busy (no close, no update) until then.
   */
  async testFire(): Promise<void> {
    if (this.firing) return;
    this.firing = true;
    const release = this.#lifecycle.beginBusy(FIRING_REASON);
    const done = () => {
      this.firing = false;
      release();
    };
    const fire = { power: this.power, durationMs: this.durationMs };
    if (!(await this.#run(() => this.#adapter.testFire(fire, this.#settings.profile)))) {
      done();
      return;
    }
    // Rust's clamp may have shortened it; the UI's own caps match.
    setTimeout(done, Math.min(fire.durationMs, TEST_FIRE_MAX_MS));
  }

  async #run(command: () => Promise<unknown>): Promise<boolean> {
    this.error = "";
    try {
      await command();
      return true;
    } catch (cause) {
      this.error = message(cause);
      return false;
    }
  }

  #save(): void {
    this.#storage?.setItem(
      STORAGE_KEY,
      JSON.stringify({
        step: this.step,
        feed: this.feed,
        power: this.power,
        durationMs: this.durationMs,
      }),
    );
  }
}

export const machineControl = new MachineControl(
  controlAdapter,
  appMachine,
  machineJob,
  machineSettings,
  appLifecycle,
  globalThis.window?.localStorage,
);
