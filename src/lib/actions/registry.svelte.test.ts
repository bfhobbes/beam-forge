import { describe, expect, it, vi } from "vite-plus/test";
import { ActionRegistry, groupActions, type RecentStorage } from "./registry.svelte.js";
import type { KeyPress } from "./shortcut.js";

const press = (key: string, extra: Partial<KeyPress & { target: EventTarget | null }> = {}) => ({
  key,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  target: null,
  ...extra,
});

const input = { tagName: "INPUT" } as unknown as EventTarget;

function memoryStorage(initial: Record<string, string> = {}): RecentStorage & {
  data: Record<string, string>;
} {
  const data = { ...initial };
  return {
    data,
    getItem: (key) => data[key] ?? null,
    setItem: (key, value) => void (data[key] = value),
  };
}

const registry = (storage: RecentStorage = memoryStorage()) =>
  new ActionRegistry({ mac: false, storage });

describe("action registry", () => {
  it("lists registered actions and forgets them when unregistered", () => {
    const actions = registry();
    const off = actions.register({ id: "a", title: "A", group: "Edit", run() {} });
    expect(actions.all.map((action) => action.id)).toEqual(["a"]);
    off();
    expect(actions.all).toEqual([]);
  });

  it("refuses duplicate ids", () => {
    const actions = registry();
    actions.register({ id: "a", title: "A", group: "Edit", run() {} });
    expect(() => actions.register({ id: "a", title: "Again", group: "Edit", run() {} })).toThrow(
      /already registered/,
    );
  });

  it("only unregisters the registration it was returned from", () => {
    const actions = registry();
    const off = actions.register({ id: "a", title: "First", group: "Edit", run() {} });
    off();
    actions.register({ id: "a", title: "Second", group: "Edit", run() {} });
    off();
    expect(actions.get("a")?.title).toBe("Second");
  });

  it("registers several actions at once", () => {
    const actions = registry();
    const off = actions.register(
      { id: "a", title: "A", group: "Edit", run() {} },
      { id: "b", title: "B", group: "Edit", run() {} },
    );
    expect(actions.all).toHaveLength(2);
    off();
    expect(actions.all).toHaveLength(0);
  });

  it("takes the default binding for its id unless the action names one", () => {
    const actions = registry();
    actions.register(
      { id: "edit.undo", title: "Undo", group: "Edit", run() {} },
      { id: "edit.redo", title: "Redo", group: "Edit", shortcut: "Mod+Y", run() {} },
      { id: "custom", title: "Custom", group: "Edit", run() {} },
    );
    expect(actions.get("edit.undo")?.shortcut).toBe("Mod+Z");
    expect(actions.get("edit.redo")?.shortcut).toBe("Mod+Y");
    expect(actions.get("custom")?.shortcut).toBeUndefined();
  });

  it("runs enabled actions only", () => {
    const actions = registry();
    const run = vi.fn();
    let enabled = false;
    actions.register({ id: "a", title: "A", group: "Edit", enabled: () => enabled, run });
    expect(actions.run("a")).toBe(false);
    expect(run).not.toHaveBeenCalled();
    enabled = true;
    expect(actions.run("a")).toBe(true);
    expect(run).toHaveBeenCalledOnce();
    expect(actions.run("missing")).toBe(false);
  });
});

describe("keyboard dispatch", () => {
  it("runs the action bound to a key press", () => {
    const actions = registry();
    const run = vi.fn();
    actions.register({ id: "tool.rectangle", title: "Rectangle", group: "Tools", run });
    expect(actions.handleKey(press("r"))).toBe(true);
    expect(run).toHaveBeenCalledOnce();
    expect(actions.handleKey(press("q"))).toBe(false);
  });

  it("does not fire shortcuts while typing in a field", () => {
    const actions = registry();
    const bare = vi.fn();
    const modified = vi.fn();
    actions.register(
      { id: "tool.rectangle", title: "Rectangle", group: "Tools", run: bare },
      { id: "edit.duplicate", title: "Duplicate", group: "Edit", run: modified },
    );
    expect(actions.handleKey(press("r", { target: input }))).toBe(false);
    expect(actions.handleKey(press("d", { ctrlKey: true, target: input }))).toBe(false);
    expect(bare).not.toHaveBeenCalled();
    expect(modified).not.toHaveBeenCalled();
  });

  it("lets actions opt in to firing while typing", () => {
    const actions = registry();
    const run = vi.fn();
    actions.register({
      id: "app.palette",
      title: "Palette",
      group: "Help",
      whileTyping: true,
      run,
    });
    expect(actions.handleKey(press("k", { ctrlKey: true, target: input }))).toBe(true);
    expect(run).toHaveBeenCalledOnce();
  });

  it("leaves disabled shortcuts to the browser", () => {
    const actions = registry();
    actions.register({
      id: "edit.duplicate",
      title: "D",
      group: "Edit",
      enabled: () => false,
      run() {},
    });
    expect(actions.handleKey(press("d", { ctrlKey: true }))).toBe(false);
  });
});

describe("recent actions", () => {
  it("records runs most-recent first without repeats, capped", () => {
    const actions = registry();
    for (const id of ["a", "b", "c", "d", "e", "f"])
      actions.register({ id, title: id, group: "Edit", run() {} });
    for (const id of ["a", "b", "a", "c", "d", "e", "f"]) actions.run(id);
    expect(actions.recent.map((action) => action.id)).toEqual(["f", "e", "d", "c", "a"]);
  });

  it("persists across sessions and hides unregistered ids", () => {
    const storage = memoryStorage();
    const first = registry(storage);
    first.register(
      { id: "a", title: "A", group: "Edit", run() {} },
      { id: "b", title: "B", group: "Edit", run() {} },
    );
    first.run("a");
    first.run("b");

    const second = registry(storage);
    second.register({ id: "a", title: "A", group: "Edit", run() {} });
    expect(second.recent.map((action) => action.id)).toEqual(["a"]);
  });

  it("ignores corrupt storage", () => {
    const actions = registry(memoryStorage({ "beam-forge:recent-actions": "{nope" }));
    expect(actions.recent).toEqual([]);
  });
});

describe("palette search", () => {
  it("returns every enabled and disabled action for an empty query", () => {
    const actions = registry();
    actions.register(
      { id: "a", title: "Alpha", group: "Edit", run() {} },
      { id: "b", title: "Beta", group: "Edit", enabled: () => false, run() {} },
    );
    expect(actions.search("").map((action) => action.id)).toEqual(["a", "b"]);
  });

  it("ranks by fuzzy score over title, group and keywords", () => {
    const actions = registry();
    actions.register(
      { id: "group", title: "Group shapes", group: "Edit", run() {} },
      { id: "updates", title: "Check for updates", group: "App", run() {} },
      {
        id: "frame",
        title: "Frame job",
        group: "Machine",
        keywords: ["outline", "trace"],
        run() {},
      },
    );
    expect(actions.search("upd").map((action) => action.id)).toEqual(["updates"]);
    expect(actions.search("trace").map((action) => action.id)).toEqual(["frame"]);
    expect(actions.search("machine").map((action) => action.id)).toEqual(["frame"]);
  });
});

describe("grouping", () => {
  it("groups actions by heading in first-seen order", () => {
    const action = (id: string, group: string) => ({ id, title: id, group, run() {} });
    const grouped = groupActions([action("a", "Tools"), action("b", "Edit"), action("c", "Tools")]);
    expect(grouped.map(([group, list]) => [group, list.map((item) => item.id)])).toEqual([
      ["Tools", ["a", "c"]],
      ["Edit", ["b"]],
    ]);
  });
});
