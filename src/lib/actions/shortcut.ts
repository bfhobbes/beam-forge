/**
 * Keyboard shortcuts are written as `+`-joined parts, modifiers first:
 * `"V"`, `"Mod+D"`, `"Mod+Shift+Z"`, `"Mod+/"`, `"Delete"`. `Mod` is Ctrl on
 * Windows and Linux and Cmd on macOS, so one binding serves every platform.
 */
export type Shortcut = string;

/** The slice of `KeyboardEvent` shortcut matching reads, so tests need no DOM. */
export interface KeyPress {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

interface Parsed {
  mod: boolean;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  key: string;
}

function parse(shortcut: Shortcut): Parsed {
  // Split on "+" that joins parts, so "Mod++" keeps "+" as the key.
  const parts = shortcut.split(/\+(?!$)/);
  const key = parts.pop() ?? "";
  const has = (name: string) => parts.includes(name);
  return { mod: has("Mod"), ctrl: has("Ctrl"), alt: has("Alt"), shift: has("Shift"), key };
}

/** True when the press is exactly this shortcut: same key, no extra modifiers. */
export function matchesShortcut(shortcut: Shortcut, press: KeyPress, mac: boolean): boolean {
  const want = parse(shortcut);
  const ctrl = want.ctrl || (want.mod && !mac);
  const meta = want.mod && mac;
  return (
    press.key.toLowerCase() === want.key.toLowerCase() &&
    press.ctrlKey === ctrl &&
    press.metaKey === meta &&
    press.altKey === want.alt &&
    press.shiftKey === want.shift
  );
}

const keyLabels: Record<string, string> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Escape: "Esc",
};

const macSymbols: Record<string, string> = { Mod: "⌘", Ctrl: "⌃", Alt: "⌥", Shift: "⇧" };

/** Key caps to show for a shortcut, in platform conventions. */
export function formatShortcut(shortcut: Shortcut, mac: boolean): string[] {
  const { mod, ctrl, alt, shift, key } = parse(shortcut);
  const modifiers = [ctrl && "Ctrl", mod && "Mod", alt && "Alt", shift && "Shift"].filter(
    (name): name is string => Boolean(name),
  );
  const label = keyLabels[key] ?? (key.length === 1 ? key.toUpperCase() : key);
  return [
    ...modifiers.map((name) => (mac ? macSymbols[name] : name === "Mod" ? "Ctrl" : name)),
    label,
  ];
}

/** True when key presses aimed at this target are text entry, not commands. */
export function isTyping(target: EventTarget | null): boolean {
  if (!target || typeof target !== "object" || !("tagName" in target)) return false;
  const element = target as { tagName: string; isContentEditable?: boolean };
  return (
    ["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName) || element.isContentEditable === true
  );
}

/** Whether this browser runs on macOS, where `Mod` means Cmd. */
export function onMac(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
}
