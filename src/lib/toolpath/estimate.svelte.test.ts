import { describe, expect, it } from "vite-plus/test";
import type { JobEstimate } from "#lib/bindings/plan/JobEstimate.js";
import { emptyDocument } from "#lib/document/model.js";
import { DEFAULT_PLACEMENT } from "#lib/gcode/job-setup.svelte.js";
import { PREVIEW_NO_TOOLPATH, previewEstimate, type EstimateAdapter } from "./adapter.js";
import { JobTime } from "./estimate.svelte.js";

const estimate = (seconds: number): JobEstimate => ({
  seconds,
  layers: [{ layerId: "a", seconds }],
});

function jobTime(adapter: EstimateAdapter) {
  return new JobTime({
    adapter,
    request: () => ({ document: emptyDocument(), profile: null, placement: DEFAULT_PLACEMENT }),
  });
}

describe("job time estimate", () => {
  it("keeps the newest estimate when refreshes overlap", async () => {
    const resolvers: ((estimate: JobEstimate) => void)[] = [];
    const store = jobTime({
      available: true,
      estimate: () => new Promise((resolve) => resolvers.push(resolve)),
    });
    const older = store.refresh();
    const newer = store.refresh();
    resolvers[1]!(estimate(20));
    resolvers[0]!(estimate(10));
    await Promise.all([older, newer]);
    expect(store.estimate?.seconds).toBe(20);
    expect(store.problem).toBe("");
  });

  it("says why there's no estimate, and clears the last one", async () => {
    let fail = false;
    const store = jobTime({
      available: true,
      estimate: async () => {
        if (fail) throw new Error("There is nothing to cut");
        return estimate(5);
      },
    });
    await store.refresh();
    expect(store.estimate?.layers).toEqual([{ layerId: "a", seconds: 5 }]);
    fail = true;
    await store.refresh();
    expect(store.estimate).toBeNull();
    expect(store.problem).toBe("There is nothing to cut");
  });

  it("is unavailable in the browser preview", async () => {
    const store = jobTime(previewEstimate);
    expect(store.available).toBe(false);
    await store.refresh();
    expect(store.problem).toBe(PREVIEW_NO_TOOLPATH);
  });
});
