import type { CheckLevel } from "#lib/bindings/CheckLevel.js";
import type { Preflight } from "#lib/bindings/Preflight.js";
import type { PreflightCheckId } from "#lib/bindings/PreflightCheckId.js";

/**
 * The pre-run checklist shown before a job starts: the machine's checks
 * (`machine_preflight` in Rust) plus, if asked for, whether the job was
 * framed since it last changed. The dialog is skipped when everything
 * passes, unless the user asked to always see it.
 */

/** A checklist line: one of the machine's checks, or the framing question. */
export interface ChecklistItem {
  id: PreflightCheckId | "framed";
  level: CheckLevel;
  title: string;
  detail: string | null;
}

/** "Have you framed this job?", answered from whether it was framed since it last changed. */
export function framingCheck(framed: boolean): ChecklistItem {
  return framed
    ? { id: "framed", level: "pass", title: "Framed since the last change", detail: null }
    : {
        id: "framed",
        level: "warn",
        title: "Not framed",
        detail: "Frame the job to check where it lands on the material.",
      };
}

/**
 * The checklist for a job: the machine's checks, then the framing question
 * when `framed` is given (null when the user doesn't want to be asked).
 */
export function checklist(preflight: Preflight, framed: boolean | null): ChecklistItem[] {
  const items: ChecklistItem[] = [...preflight.checks];
  if (framed !== null) items.push(framingCheck(framed));
  return items;
}

/** The checklist with the framing question answered again. */
export function withFraming(items: readonly ChecklistItem[], framed: boolean): ChecklistItem[] {
  return items.map((item) => (item.id === "framed" ? framingCheck(framed) : item));
}

/** Whether to show the checklist before starting: something didn't pass, or it is always shown. */
export function checklistNeeded(items: readonly ChecklistItem[], alwaysShow: boolean): boolean {
  return alwaysShow || items.some((item) => item.level !== "pass");
}

/** Something must be fixed before the job can start. */
export function blocked(items: readonly ChecklistItem[]): boolean {
  return items.some((item) => item.level === "block");
}

/** The machine's warnings, which starting from the checklist acknowledges. */
export function acknowledgements(items: readonly ChecklistItem[]): PreflightCheckId[] {
  const ids: PreflightCheckId[] = [];
  for (const item of items) {
    if (item.level === "warn" && item.id !== "framed") ids.push(item.id);
  }
  return ids;
}

const ALWAYS_SHOW_KEY = "beam-forge.preflight.always-show";
const ASK_FRAMED_KEY = "beam-forge.preflight.ask-framed";

type Storage = Pick<globalThis.Storage, "getItem" | "setItem">;

/** How the checklist behaves, remembered across sessions. */
export class PreflightPrefs {
  /** Show the checklist even when everything passes. */
  alwaysShow = $state(false);
  /** Ask whether the job was framed since it last changed. */
  askFramed = $state(true);

  readonly #storage: Storage | undefined;

  constructor(storage?: Storage) {
    this.#storage = storage;
    this.alwaysShow = storage?.getItem(ALWAYS_SHOW_KEY) === "true";
    this.askFramed = storage?.getItem(ASK_FRAMED_KEY) !== "false";
  }

  setAlwaysShow(value: boolean): void {
    this.alwaysShow = value;
    this.#storage?.setItem(ALWAYS_SHOW_KEY, String(value));
  }

  setAskFramed(value: boolean): void {
    this.askFramed = value;
    this.#storage?.setItem(ASK_FRAMED_KEY, String(value));
  }
}

export const preflightPrefs = new PreflightPrefs(globalThis.window?.localStorage);
