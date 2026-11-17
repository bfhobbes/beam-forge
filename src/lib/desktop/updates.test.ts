import { describe, expect, it, vi } from "vite-plus/test";
import { get } from "svelte/store";
import { createUpdater, type UpdatePackage } from "./updates.js";

function fixture() {
  const update = {
    version: "0.2.1",
    body: "Improvements",
    download: vi.fn<UpdatePackage["download"]>(async (progress) => {
      progress({ event: "Started", data: { contentLength: 100 } });
      progress({ event: "Progress", data: { chunkLength: 100 } });
      progress({ event: "Finished" });
    }),
    install: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
  };
  const provider = {
    check: vi.fn(async (): Promise<UpdatePackage | null> => update),
    relaunch: vi.fn(async () => {}),
  };
  const unlock = vi.fn();
  let safe = true;
  const updater = createUpdater(provider, () => (safe ? unlock : null));
  return {
    update,
    provider,
    updater,
    unlock,
    unsafe: () => {
      safe = false;
    },
  };
}
describe("desktop update lifecycle", () => {
  it("checks and downloads without installing until explicitly requested", async () => {
    const f = fixture();
    await f.updater.check();
    expect(get(f.updater.state).stage).toBe("available");
    await f.updater.download();
    expect(get(f.updater.state)).toMatchObject({ stage: "ready", received: 100, total: 100 });
    expect(f.update.install).not.toHaveBeenCalled();
    await f.updater.install();
    expect(f.update.install).toHaveBeenCalledOnce();
    expect(f.provider.relaunch).toHaveBeenCalledOnce();
    expect(f.unlock).toHaveBeenCalledOnce();
  });
  it("rechecks unsaved/busy state at install time", async () => {
    const f = fixture();
    await f.updater.check();
    await f.updater.download();
    f.unsafe();
    await f.updater.install();
    expect(f.update.install).not.toHaveBeenCalled();
    expect(get(f.updater.state).error).toContain("Save your changes");
  });
  it("never treats a completed transfer with a bad signature as installable", async () => {
    const f = fixture();
    vi.mocked(f.update.download).mockImplementation(async (progress) => {
      progress({ event: "Finished" });
      throw new Error("Signature verification failed");
    });
    await f.updater.check();
    await f.updater.download();
    await f.updater.install();
    expect(get(f.updater.state).stage).toBe("available");
    expect(f.update.install).not.toHaveBeenCalled();
    expect(get(f.updater.state).error).toContain("Signature");
  });
  it("deduplicates checks and disposes a late response", async () => {
    const f = fixture();
    let resolve!: (value: UpdatePackage) => void;
    f.provider.check.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const pending = f.updater.check();
    await f.updater.check();
    await f.updater.dispose();
    resolve(f.update);
    await pending;
    expect(f.provider.check).toHaveBeenCalledOnce();
    expect(f.update.close).toHaveBeenCalledOnce();
  });
  it("handles no update and recovers from offline checks", async () => {
    const f = fixture();
    f.provider.check.mockRejectedValueOnce(new Error("Offline")).mockResolvedValueOnce(null);
    await f.updater.check();
    expect(get(f.updater.state).stage).toBe("error");
    await f.updater.check();
    expect(get(f.updater.state).stage).toBe("current");
  });
  it("retries restart without reinstalling after a relaunch failure", async () => {
    const f = fixture();
    f.provider.relaunch.mockRejectedValueOnce(new Error("Restart failed"));
    await f.updater.check();
    await f.updater.download();
    await f.updater.install();
    expect(get(f.updater.state).stage).toBe("restart");
    await f.updater.restart();
    expect(f.update.install).toHaveBeenCalledOnce();
    expect(f.provider.relaunch).toHaveBeenCalledTimes(2);
  });
});
