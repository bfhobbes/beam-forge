import { Circle, Hexagon, MousePointer2, Spline, Square, Type } from "@lucide/svelte";
import type { Action } from "#lib/actions/registry.svelte.js";
import type { ShapeTool } from "#lib/canvas/draw.js";

/** Design tools on the left rail. */
export const tools = [
  { id: "select", label: "Select", icon: MousePointer2 },
  { id: "rectangle", label: "Rectangle", icon: Square },
  { id: "ellipse", label: "Ellipse", icon: Circle },
  { id: "line", label: "Line", icon: Spline },
  { id: "polygon", label: "Polygon", icon: Hexagon },
  { id: "text", label: "Text", icon: Type },
] as const;

export type ToolId = (typeof tools)[number]["id"];

const SHAPE_TOOLS: readonly ToolId[] = ["rectangle", "ellipse", "line", "polygon"];

/** The shape-drawing tool `tool` is, or null for select and text. */
export function shapeTool(tool: ToolId): ShapeTool | null {
  return SHAPE_TOOLS.includes(tool) ? (tool as ShapeTool) : null;
}

/** Registry id for picking a tool; its shortcut comes from the default keymap. */
export const toolActionId = (tool: ToolId) => `tool.${tool}`;

/** One "pick this tool" action per tool, for the palette and shortcuts. */
export function toolActions(select: (tool: ToolId) => void): Action[] {
  return tools.map((tool) => ({
    id: toolActionId(tool.id),
    title: `${tool.label} tool`,
    group: "Tools",
    run: () => select(tool.id),
  }));
}
