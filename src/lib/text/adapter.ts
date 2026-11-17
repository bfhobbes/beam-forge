import { invoke } from "@tauri-apps/api/core";
import type { FontFamily } from "#lib/bindings/text/FontFamily.js";
import type { FontStyle } from "#lib/bindings/text/FontStyle.js";
import type { TextLayout } from "#lib/bindings/text/TextLayout.js";
import type { TextLine } from "#lib/bindings/text/TextLine.js";
import type { TextStyle } from "#lib/bindings/text/TextStyle.js";
import { desktop } from "#lib/desktop/platform.js";
import type { Subpath, TextObject } from "#lib/document/model.js";

export type { FontFamily, FontStyle, TextLayout, TextLine, TextStyle };

/** Typed access to the text commands in `src-tauri/src/text.rs`. */
export interface TextAdapter {
  /** False in the browser preview, which draws boxes instead of letters. */
  readonly available: boolean;
  /** Installed font families, sorted by name. */
  fonts(): Promise<FontFamily[]>;
  /** `text` set in `style`: its outline and caret positions. */
  layout(text: string, style: TextStyle): Promise<TextLayout>;
}

/** The part of a text object that decides how it is laid out. */
export function styleOf(text: TextObject): TextStyle {
  const { fontFamily, fontWeight, italic, height, letterSpacing, lineSpacing, align } = text;
  return { fontFamily, fontWeight, italic, height, letterSpacing, lineSpacing, align };
}

export const desktopText: TextAdapter = {
  available: true,
  fonts: () => invoke<FontFamily[]>("text_fonts"),
  layout: (text, style) => invoke<TextLayout>("text_layout", { text, style }),
};

export const PREVIEW_FONT = "Browser preview (boxes)";

/**
 * The browser preview has no fonts: each character is a box the size of a
 * capital, so the text tool can be tried, and nobody mistakes it for type.
 */
export function previewLayout(text: string, style: TextStyle): TextLayout {
  const advance = style.height * 0.6;
  const lineHeight = style.height * 1.4 * style.lineSpacing;
  const outline: Subpath[] = [];
  const lines: TextLine[] = text.split("\n").map((line, row) => {
    const baseline = -row * lineHeight;
    const width =
      line.length === 0 ? 0 : line.length * (advance + style.letterSpacing) - style.letterSpacing;
    const shift = style.align === "left" ? 0 : style.align === "center" ? -width / 2 : -width;
    const carets = Array.from(
      { length: line.length + 1 },
      (_, i) => shift + (i === line.length ? width : i * (advance + style.letterSpacing)),
    );
    // One box per UTF-16 unit, matching the carets.
    for (let i = 0; i < line.length; i++) {
      if (line[i].trim() === "") continue;
      const x = carets[i] + advance * 0.1;
      const w = advance * 0.8;
      outline.push({
        start: { x, y: baseline },
        segments: [
          { kind: "line", to: { x: x + w, y: baseline } },
          { kind: "line", to: { x: x + w, y: baseline + style.height } },
          { kind: "line", to: { x, y: baseline + style.height } },
        ],
        closed: true,
      });
    }
    return { baseline, capHeight: style.height, carets };
  });
  return {
    outline,
    lines,
    fontFamily: PREVIEW_FONT,
    substituted: style.fontFamily !== PREVIEW_FONT,
  };
}

export const previewText: TextAdapter = {
  available: false,
  fonts: () => Promise.resolve([{ name: PREVIEW_FONT, styles: [{ weight: 400, italic: false }] }]),
  layout: (text, style) => Promise.resolve(previewLayout(text, style)),
};

export const textAdapter = desktop ? desktopText : previewText;
