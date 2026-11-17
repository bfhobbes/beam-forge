import { describe, expect, it } from "vite-plus/test";
import type { Toolpath } from "#lib/bindings/gcode/Toolpath.js";
import { emptyDocument } from "#lib/document/model.js";
import { DEFAULT_PLACEMENT } from "#lib/gcode/job-setup.svelte.js";
import { PREVIEW_NO_TOOLPATH, previewToolpath, type ToolpathAdapter } from "./adapter.js";
import { ToolpathPreview } from "./preview.svelte.js";

/** 10 mm at 600 mm/min: one second. */
const oneSecond = (layerId = "a"): Toolpath => ({
  rapidRate: 6000,
  plan: {
    start: { x: 0, y: 0 },
    layers: [
      {
        layerId,
        airAssist: false,
        moves: [{ kind: "cut", to: { x: 10, y: 0 }, power: 50, speed: 600 }],
      },
    ],
  },
});

function preview(adapter: ToolpathAdapter) {
  const requests: unknown[] = [];
  const store = new ToolpathPreview({
    adapter,
    request: () => {
      const request = { document: emptyDocument(), profile: null, placement: DEFAULT_PLACEMENT };
      requests.push(request);
      return request;
    },
  });
  return { store, requests };
}

const fixed = (toolpath: Toolpath): ToolpathAdapter => ({
  available: true,
  toolpath: async () => toolpath,
});

describe("toolpath preview", () => {
  it("plans the job and shows it finished", async () => {
    const { store, requests } = preview(fixed(oneSecond()));
    await store.refresh();
    expect(requests).toHaveLength(1);
    expect(store.problem).toBe("");
    expect(store.duration).toBeCloseTo(1);
    expect(store.atEnd).toBe(true);
    expect(store.progress?.completed).toBe(1);
  });

  it("plays from the start at the chosen speed and stops at the end", async () => {
    const { store } = preview(fixed(oneSecond()));
    await store.refresh();
    store.playPause();
    expect(store.playing).toBe(true);
    expect(store.time).toBe(0);
    store.setSpeed(2);
    store.tick(250);
    expect(store.time).toBeCloseTo(0.5);
    expect(store.progress?.head.x).toBeCloseTo(5);
    store.playPause();
    store.tick(250);
    expect(store.time).toBeCloseTo(0.5);
    store.playPause();
    store.tick(10_000);
    expect(store.atEnd).toBe(true);
    expect(store.playing).toBe(false);
  });

  it("scrubbing pauses and clamps to the job", async () => {
    const { store } = preview(fixed(oneSecond()));
    await store.refresh();
    store.playPause();
    store.scrub(5);
    expect(store.playing).toBe(false);
    expect(store.time).toBeCloseTo(1);
    store.scrub(-1);
    expect(store.time).toBe(0);
  });

  it("can follow a running job instead of the clock", async () => {
    const { store } = preview(fixed(oneSecond()));
    await store.refresh();
    store.scrub(0);
    store.external = { completed: 1 };
    expect(store.progress?.completed).toBe(1);
    expect(store.progress?.head).toEqual({ x: 10, y: 0 });
    store.external = { completed: 0, head: { x: 3, y: 0 } };
    expect(store.progress?.head).toEqual({ x: 3, y: 0 });
  });

  it("keeps the newest plan when refreshes overlap", async () => {
    const resolvers: ((toolpath: Toolpath) => void)[] = [];
    const { store } = preview({
      available: true,
      toolpath: () => new Promise((resolve) => resolvers.push(resolve)),
    });
    const older = store.refresh();
    const newer = store.refresh();
    resolvers[1]!(oneSecond("new"));
    resolvers[0]!(oneSecond("old"));
    await Promise.all([older, newer]);
    expect(store.timeline?.layers[0]?.layerId).toBe("new");
    expect(store.loading).toBe(false);
  });

  it("says why there's nothing to show", async () => {
    const { store } = preview(previewToolpath);
    expect(store.available).toBe(false);
    await store.refresh();
    expect(store.toolpath).toBeNull();
    expect(store.problem).toBe(PREVIEW_NO_TOOLPATH);
  });

  it("follows a running job's toolpath without re-planning, then goes back", async () => {
    const { store, requests } = preview(fixed(oneSecond("design")));
    const running = oneSecond("job");
    store.follow(running, { completed: 0, head: { x: 4, y: 0 } });
    expect(store.shown).toBe(true);
    expect(store.toolpath).toBe(running);
    expect(store.progress?.head).toEqual({ x: 4, y: 0 });

    // Edits to the design meanwhile don't replace the job's path.
    await store.refresh();
    expect(requests).toHaveLength(0);
    store.follow(running, { completed: 1 });
    expect(store.progress?.completed).toBe(1);

    store.unfollow();
    expect(store.external).toBeNull();
    await Promise.resolve();
    await Promise.resolve();
    expect(requests).toHaveLength(1);
    expect(store.toolpath?.plan.layers[0]!.layerId).toBe("design");
  });

  it("hiding the view stops playback", async () => {
    const { store } = preview(fixed(oneSecond()));
    await store.refresh();
    store.toggle(true);
    store.playPause();
    store.toggle();
    expect(store.shown).toBe(false);
    expect(store.playing).toBe(false);
  });
});
