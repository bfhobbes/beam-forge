import { textAdapter, type FontFamily, type TextAdapter } from "./adapter.js";

/** CSS weight names, as font menus show them. */
const WEIGHT_NAMES: Record<number, string> = {
  100: "Thin",
  200: "Extra Light",
  300: "Light",
  400: "Regular",
  500: "Medium",
  600: "Semibold",
  700: "Bold",
  800: "Extra Bold",
  900: "Black",
};

/** "Bold Italic", "Regular", or "Weight 450" for an unusual weight. */
export function styleName(weight: number, italic: boolean): string {
  const name = WEIGHT_NAMES[weight] ?? `Weight ${weight}`;
  if (!italic) return name;
  return weight === 400 ? "Italic" : `${name} Italic`;
}

/** The installed fonts, fetched once on first use. */
export class FontList {
  families = $state.raw<FontFamily[]>([]);
  loaded = $state(false);
  error = $state("");
  #loading: Promise<void> | null = null;
  readonly #adapter: TextAdapter;

  constructor(adapter: TextAdapter) {
    this.#adapter = adapter;
  }

  load(): Promise<void> {
    this.#loading ??= this.#adapter.fonts().then(
      (families) => {
        this.families = families;
        this.loaded = true;
      },
      (error: unknown) => {
        this.error = error instanceof Error ? error.message : String(error);
        this.#loading = null;
      },
    );
    return this.#loading;
  }

  find(name: string): FontFamily | undefined {
    const lower = name.toLowerCase();
    return this.families.find((family) => family.name.toLowerCase() === lower);
  }
}

export const fontList = new FontList(textAdapter);
