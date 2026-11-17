import { describe, expect, it } from "vite-plus/test";
import { formatShortcut, isTyping, matchesShortcut, type KeyPress } from "./shortcut.js";

const press = (key: string, extra: Partial<KeyPress> = {}): KeyPress => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...extra,
});

const element = (fields: object) => fields as unknown as EventTarget;

describe("shortcut matching", () => {
  it("matches bare letters, ignoring case and Shift-free caps lock", () => {
    expect(matchesShortcut("V", press("v"), false)).toBe(true);
    expect(matchesShortcut("V", press("V"), false)).toBe(true);
    expect(matchesShortcut("V", press("r"), false)).toBe(false);
  });

  it("requires the exact modifiers", () => {
    expect(matchesShortcut("V", press("v", { ctrlKey: true }), false)).toBe(false);
    expect(matchesShortcut("Mod+D", press("d"), false)).toBe(false);
    expect(matchesShortcut("Mod+D", press("d", { ctrlKey: true }), false)).toBe(true);
    expect(matchesShortcut("Mod+D", press("d", { ctrlKey: true, shiftKey: true }), false)).toBe(
      false,
    );
    expect(
      matchesShortcut("Mod+Shift+G", press("G", { ctrlKey: true, shiftKey: true }), false),
    ).toBe(true);
  });

  it("maps Mod to Ctrl off macOS and Cmd on macOS", () => {
    expect(matchesShortcut("Mod+K", press("k", { metaKey: true }), false)).toBe(false);
    expect(matchesShortcut("Mod+K", press("k", { metaKey: true }), true)).toBe(true);
    expect(matchesShortcut("Mod+K", press("k", { ctrlKey: true }), true)).toBe(false);
  });

  it("matches punctuation and named keys", () => {
    expect(matchesShortcut("Mod+/", press("/", { ctrlKey: true }), false)).toBe(true);
    expect(matchesShortcut("Delete", press("Delete"), false)).toBe(true);
    expect(matchesShortcut("Escape", press("Escape"), false)).toBe(true);
  });
});

describe("shortcut labels", () => {
  it("spells modifiers out off macOS", () => {
    expect(formatShortcut("Mod+Shift+Z", false)).toEqual(["Ctrl", "Shift", "Z"]);
    expect(formatShortcut("V", false)).toEqual(["V"]);
    expect(formatShortcut("Shift+ArrowLeft", false)).toEqual(["Shift", "←"]);
    expect(formatShortcut("Escape", false)).toEqual(["Esc"]);
  });

  it("uses symbols on macOS", () => {
    expect(formatShortcut("Mod+Shift+Z", true)).toEqual(["⌘", "⇧", "Z"]);
    expect(formatShortcut("Alt+X", true)).toEqual(["⌥", "X"]);
  });
});

describe("typing detection", () => {
  it("treats text fields and editable content as typing", () => {
    expect(isTyping(element({ tagName: "INPUT" }))).toBe(true);
    expect(isTyping(element({ tagName: "TEXTAREA" }))).toBe(true);
    expect(isTyping(element({ tagName: "SELECT" }))).toBe(true);
    expect(isTyping(element({ tagName: "DIV", isContentEditable: true }))).toBe(true);
  });

  it("lets everything else through", () => {
    expect(isTyping(element({ tagName: "DIV" }))).toBe(false);
    expect(isTyping(element({ tagName: "BUTTON" }))).toBe(false);
    expect(isTyping(null)).toBe(false);
  });
});
