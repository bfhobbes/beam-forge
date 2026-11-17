import { describe, expect, it } from "vite-plus/test";
import { followSystemTheme, type ColorSchemeQuery } from "./theme.js";

function fakeRoot() {
  const classes = new Set<string>();
  return {
    classes,
    classList: {
      toggle(token: string, force: boolean) {
        if (force) classes.add(token);
        else classes.delete(token);
      },
    },
    style: { colorScheme: "" },
  };
}

function fakeQuery(matches: boolean) {
  const listeners = new Set<(event: { matches: boolean }) => void>();
  const query: ColorSchemeQuery & { set(dark: boolean): void; listeners: typeof listeners } = {
    matches,
    listeners,
    addEventListener: (_type, listener) => listeners.add(listener),
    removeEventListener: (_type, listener) => listeners.delete(listener),
    set(dark) {
      query.matches = dark;
      for (const listener of listeners) listener({ matches: dark });
    },
  };
  return query;
}

describe("system theme", () => {
  it("starts dark when the OS is dark", () => {
    const root = fakeRoot();
    followSystemTheme(root, fakeQuery(true));
    expect(root.classes.has("dark")).toBe(true);
    expect(root.style.colorScheme).toBe("dark");
  });

  it("starts light when the OS is light", () => {
    const root = fakeRoot();
    followSystemTheme(root, fakeQuery(false));
    expect(root.classes.has("dark")).toBe(false);
    expect(root.style.colorScheme).toBe("light");
  });

  it("follows OS changes live", () => {
    const root = fakeRoot();
    const query = fakeQuery(false);
    followSystemTheme(root, query);
    query.set(true);
    expect(root.classes.has("dark")).toBe(true);
    query.set(false);
    expect(root.classes.has("dark")).toBe(false);
  });

  it("stops following once disposed", () => {
    const root = fakeRoot();
    const query = fakeQuery(false);
    const stop = followSystemTheme(root, query);
    stop();
    expect(query.listeners.size).toBe(0);
    query.set(true);
    expect(root.classes.has("dark")).toBe(false);
  });
});
