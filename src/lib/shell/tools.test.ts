import { describe, expect, it, vi } from "vite-plus/test";
import { ActionRegistry } from "#lib/actions/registry.svelte.js";
import { toolActions } from "./tools.js";

const press = (key: string) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  target: null,
});

describe("tool actions", () => {
  it("binds every tool to a single-letter shortcut", () => {
    const actions = new ActionRegistry({ mac: false, storage: null });
    actions.register(...toolActions(() => {}));
    expect(actions.all.map((action) => action.shortcut)).toEqual(["V", "R", "E", "L", "P", "T"]);
  });

  it("selects the tool for its key", () => {
    const actions = new ActionRegistry({ mac: false, storage: null });
    const select = vi.fn();
    actions.register(...toolActions(select));
    actions.handleKey(press("r"));
    expect(select).toHaveBeenCalledWith("rectangle");
  });
});
