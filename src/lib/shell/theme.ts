/** The slice of `MediaQueryList` the theme needs, so tests can fake the OS setting. */
export interface ColorSchemeQuery {
  matches: boolean;
  addEventListener(type: "change", listener: (event: { matches: boolean }) => void): void;
  removeEventListener(type: "change", listener: (event: { matches: boolean }) => void): void;
}

/** The slice of `<html>` the theme writes to. */
export interface ThemeRoot {
  classList: { toggle(token: string, force: boolean): unknown };
  style: { colorScheme: string };
}

export const darkSchemeQuery = "(prefers-color-scheme: dark)";

/**
 * Toggles `.dark` on `root` to match the OS colour scheme and keeps it in sync
 * as the setting changes. Returns the function that stops following.
 */
export function followSystemTheme(root: ThemeRoot, query: ColorSchemeQuery): () => void {
  const apply = (dark: boolean) => {
    root.classList.toggle("dark", dark);
    // Native scrollbars and form controls follow too.
    root.style.colorScheme = dark ? "dark" : "light";
  };
  const onChange = (event: { matches: boolean }) => apply(event.matches);
  apply(query.matches);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}
