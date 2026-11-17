import { untrack } from "svelte";
import { SvelteMap } from "svelte/reactivity";
import { fuzzyScore } from "./fuzzy.js";
import { defaultKeymap } from "./keymap.js";
import { isTyping, matchesShortcut, onMac, type KeyPress, type Shortcut } from "./shortcut.js";

/**
 * Something the user can do: from the command palette, a shortcut, a menu or
 * a toolbar button. Ids are dotted (`edit.undo`, `tool.rectangle`) and pick up
 * a default binding from `defaultKeymap`.
 */
export interface Action {
  id: string;
  title: string;
  /** Palette and cheat-sheet heading, e.g. "Edit" or "Tools". */
  group: string;
  /** Overrides the default binding for this id. */
  shortcut?: Shortcut;
  /** Extra words the palette matches on. */
  keywords?: string[];
  /** Read reactively, so it may depend on `$state`. Defaults to always enabled. */
  enabled?: () => boolean;
  /** Why the action is unavailable, shown while it is disabled. */
  hint?: string;
  /** Fire the shortcut even from a text field (the palette's own toggle, say). */
  whileTyping?: boolean;
  run: () => void;
}

/** A registered action with its binding resolved. */
export type RegisteredAction = Action & { shortcut?: Shortcut };

/** The slice of `Storage` recent actions need, so tests can use an in-memory map. */
export interface RecentStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const recentKey = "beam-forge:recent-actions";
const recentLimit = 5;

function browserStorage(): RecentStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null; // Storage can be disabled; recents then reset each launch.
  }
}

function readRecent(storage: RecentStorage | null): string[] {
  try {
    const value: unknown = JSON.parse(storage?.getItem(recentKey) ?? "[]");
    return Array.isArray(value) ? value.filter((id) => typeof id === "string") : [];
  } catch {
    return [];
  }
}

/** Every action the app offers. Components register on mount and unregister on destroy. */
export class ActionRegistry {
  readonly mac: boolean;
  #storage: RecentStorage | null;
  #actions = new SvelteMap<string, RegisteredAction>();
  #recent = $state<string[]>([]);

  constructor(options: { mac?: boolean; storage?: RecentStorage | null } = {}) {
    this.mac = options.mac ?? onMac();
    this.#storage = options.storage === undefined ? browserStorage() : options.storage;
    this.#recent = readRecent(this.#storage);
  }

  /**
   * Add actions; the returned function removes exactly these registrations.
   * Untracked, so registering from an effect doesn't subscribe it to the registry.
   */
  register(...actions: Action[]): () => void {
    return untrack(() => this.#register(actions));
  }

  #register(actions: Action[]): () => void {
    const added: RegisteredAction[] = [];
    for (const action of actions) {
      if (this.#actions.has(action.id)) {
        for (const done of added) this.#actions.delete(done.id);
        throw new Error(`Action "${action.id}" is already registered`);
      }
      const registered = { ...action, shortcut: action.shortcut ?? defaultKeymap[action.id] };
      this.#actions.set(action.id, registered);
      added.push(registered);
    }
    return () =>
      untrack(() => {
        for (const action of added)
          if (this.#actions.get(action.id) === action) this.#actions.delete(action.id);
      });
  }

  get(id: string): RegisteredAction | undefined {
    return this.#actions.get(id);
  }

  get all(): RegisteredAction[] {
    return [...this.#actions.values()];
  }

  enabled(action: Action): boolean {
    return action.enabled?.() ?? true;
  }

  /** Run an action if it exists and is enabled; true if it ran. */
  run(id: string): boolean {
    const action = this.#actions.get(id);
    if (!action || !this.enabled(action)) return false;
    this.#recent = [id, ...this.#recent.filter((other) => other !== id)].slice(0, recentLimit);
    try {
      this.#storage?.setItem(recentKey, JSON.stringify(this.#recent));
    } catch {
      // Full or disabled storage only costs the recents list.
    }
    action.run();
    return true;
  }

  /**
   * Run the action bound to a key press. True if one ran, so the caller can
   * prevent the browser default. Typing in a field never triggers shortcuts
   * unless the action opts in with `whileTyping`.
   */
  handleKey(press: KeyPress & { target: EventTarget | null }): boolean {
    const typing = isTyping(press.target);
    const action = this.all.find(
      (candidate) =>
        candidate.shortcut &&
        (!typing || candidate.whileTyping) &&
        this.enabled(candidate) &&
        matchesShortcut(candidate.shortcut, press, this.mac),
    );
    return action ? this.run(action.id) : false;
  }

  /** Recently run actions that are still registered, most recent first. */
  get recent(): RegisteredAction[] {
    return this.#recent.flatMap((id) => this.#actions.get(id) ?? []);
  }

  /** Actions matching a palette query, best first; all actions for an empty query. */
  search(query: string): RegisteredAction[] {
    if (!query.trim()) return this.all;
    return this.all
      .map((action) => {
        const fields = [action.title, action.group, ...(action.keywords ?? [])];
        const scores = fields
          .map((field) => fuzzyScore(query, field))
          .filter((score) => score !== null);
        return { action, score: scores.length ? Math.max(...scores) : null };
      })
      .filter(
        (result): result is { action: RegisteredAction; score: number } => result.score !== null,
      )
      .sort((a, b) => b.score - a.score)
      .map((result) => result.action);
  }
}

/** The app's action registry. */
export const actions = new ActionRegistry();

/** Actions grouped under their headings, groups in first-seen order. */
export function groupActions<T extends Pick<Action, "group">>(list: T[]): [string, T[]][] {
  const groups = new Map<string, T[]>();
  for (const action of list)
    groups.set(action.group, [...(groups.get(action.group) ?? []), action]);
  return [...groups];
}
