import { describe, expect, it, vi } from "vite-plus/test";
import type { Project } from "#lib/bindings/project/Project.js";
import type { RecentProject } from "#lib/bindings/project/RecentProject.js";
import type { Recovery } from "#lib/bindings/project/Recovery.js";
import { Lifecycle } from "#lib/desktop/lifecycle.svelte.js";
import { insert } from "#lib/document/commands.js";
import { DocumentEditor } from "#lib/document/editor.svelte.js";
import type { SceneObject } from "#lib/document/model.js";
import { IDENTITY } from "#lib/document/transform.js";
import { previewProjects, PREVIEW_NO_FILES, type ProjectAdapter } from "./adapter.js";
import { FORMAT_VERSION, ProjectSession } from "./session.svelte.js";

function rect(id: string): SceneObject {
  return {
    type: "rect",
    id,
    name: id,
    layerId: "C00",
    transform: IDENTITY,
    width: 10,
    height: 5,
    cornerRadius: 1,
  };
}

function name(path: string): string {
  return path.replace(/^.*[\\/]/, "").replace(/\.bfp$/, "");
}

/** An in-memory file system standing in for the Rust commands, through JSON as over IPC. */
function fakeDisk() {
  const files = new Map<string, string>();
  let recent: RecentProject[] = [];
  let recovery: Recovery | null = null;
  let launch: string | null = null;
  const addRecent = (path: string) => {
    recent = [{ path, name: name(path) }, ...recent.filter((entry) => entry.path !== path)];
    return recent;
  };
  const read = (path: string) => {
    const text = files.get(path);
    if (text === undefined) throw new Error(`Couldn't read ${path}: not found`);
    try {
      return JSON.parse(text) as Project;
    } catch {
      throw new Error("This isn't a valid Beam Forge project: the JSON is malformed.");
    }
  };
  const adapter: ProjectAdapter & { dialogPath: string | null } = {
    available: true,
    dialogPath: null,
    open: async (path) => ({
      path,
      name: name(path),
      project: read(path),
      recent: addRecent(path),
    }),
    async openDialog() {
      return this.dialogPath ? this.open(this.dialogPath) : null;
    },
    save: async (project, path) => {
      files.set(path, JSON.stringify(project));
      return { path, name: name(path), recent: addRecent(path) };
    },
    async saveAs(project) {
      return this.dialogPath ? this.save(project, this.dialogPath) : null;
    },
    recent: async () => recent.filter((entry) => files.has(entry.path)),
    recovery: async () => recovery,
    saveRecovery: async (project, path) => {
      recovery = JSON.parse(
        JSON.stringify({ path, name: path ? name(path) : "Untitled", savedAt: 1, project }),
      );
    },
    clearRecovery: async () => {
      recovery = null;
    },
    launchFile: async () => {
      const path = launch;
      launch = null;
      return path;
    },
  };
  return {
    adapter,
    files,
    get recovery() {
      return recovery;
    },
    setLaunch(path: string) {
      launch = path;
    },
  };
}

function setup(disk = fakeDisk()) {
  const lifecycle = new Lifecycle();
  const editor = new DocumentEditor(lifecycle);
  const session = new ProjectSession({
    adapter: disk.adapter,
    editor,
    lifecycle,
    profile: () => ({ id: "serial-com3", name: "Phecda" }),
  });
  return { disk, lifecycle, editor, session };
}

describe("project session", () => {
  it("saves, then reopens an identical document", async () => {
    const { disk, editor, lifecycle, session } = setup();
    editor.execute(insert([rect("a"), { ...rect("b"), transform: [0.5, 0.25, -1, 2, 3.5, -7] }]));
    const original = editor.doc;
    disk.adapter.dialogPath = "C:/jobs/Coaster.bfp";

    expect(await session.save()).toBe(true);
    expect(session.path).toBe("C:/jobs/Coaster.bfp");
    expect(session.name).toBe("Coaster");
    expect(lifecycle.dirty).toBe(false);
    expect(session.title).toBe("Coaster – Beam Forge");
    expect(session.recent).toEqual([{ path: "C:/jobs/Coaster.bfp", name: "Coaster" }]);
    const saved = JSON.parse(disk.files.get("C:/jobs/Coaster.bfp")!);
    expect(saved.formatVersion).toBe(FORMAT_VERSION);
    expect(saved.machineProfile).toEqual({ id: "serial-com3", name: "Phecda" });

    await session.newProject();
    expect(editor.doc.objects).toEqual([]);
    expect(session.name).toBe("Untitled");

    expect(await session.open("C:/jobs/Coaster.bfp")).toBe(true);
    expect(editor.doc).toEqual(original);
    expect(lifecycle.dirty).toBe(false);
    expect(editor.canUndo).toBe(false);
  });

  it("saves to the same file without asking again", async () => {
    const { disk, editor, session } = setup();
    disk.adapter.dialogPath = "a.bfp";
    await session.save();
    disk.adapter.dialogPath = null;
    editor.execute(insert([rect("a")]));
    expect(session.title).toBe("● a – Beam Forge");
    expect(await session.save()).toBe(true);
    expect(JSON.parse(disk.files.get("a.bfp")!).document.objects).toHaveLength(1);
  });

  it("holds the app busy while saving and keeps edits made meanwhile unsaved", async () => {
    const { disk, editor, lifecycle, session } = setup();
    let finish!: () => void;
    const save = disk.adapter.save.bind(disk.adapter);
    disk.adapter.save = async (project, path) => {
      await new Promise<void>((resolve) => (finish = resolve));
      return save(project, path);
    };
    session.path = "a.bfp";
    editor.execute(insert([rect("a")]));
    const saving = session.save();
    expect(lifecycle.busy).toEqual(["Saving"]);
    editor.execute(insert([rect("b")]));
    finish();
    expect(await saving).toBe(true);
    expect(lifecycle.busy).toEqual([]);
    expect(lifecycle.dirty).toBe(true);
  });

  it("cancelling Save As changes nothing", async () => {
    const { editor, lifecycle, session } = setup();
    editor.execute(insert([rect("a")]));
    expect(await session.saveAs()).toBe(false);
    expect(lifecycle.dirty).toBe(true);
    expect(session.path).toBeNull();
  });

  it("asks before opening over unsaved changes", async () => {
    const { disk, editor, session } = setup();
    disk.files.set("other.bfp", JSON.stringify(session.snapshot()));
    editor.execute(insert([rect("a")]));

    const cancelled = session.open("other.bfp");
    await vi.waitFor(() => expect(session.unsaved).toEqual({ action: "opening another project" }));
    session.answerUnsaved("cancel");
    expect(await cancelled).toBe(false);
    expect(editor.doc.objects).toHaveLength(1);

    const discarded = session.open("other.bfp");
    await vi.waitFor(() => expect(session.unsaved).not.toBeNull());
    session.answerUnsaved("discard");
    expect(await discarded).toBe(true);
    expect(editor.doc.objects).toEqual([]);
    expect(session.name).toBe("other");
  });

  it("can save before starting a new project", async () => {
    const { disk, editor, session } = setup();
    disk.adapter.dialogPath = "kept.bfp";
    editor.execute(insert([rect("a")]));
    const started = session.newProject();
    await vi.waitFor(() => expect(session.unsaved).not.toBeNull());
    session.answerUnsaved("save");
    expect(await started).toBe(true);
    expect(JSON.parse(disk.files.get("kept.bfp")!).document.objects).toHaveLength(1);
    expect(editor.doc.objects).toEqual([]);
  });

  it("shows a clear error for a malformed file and keeps the current document", async () => {
    const { disk, editor, session } = setup();
    editor.execute(insert([rect("a")]));
    disk.files.set("broken.bfp", "{ not json");
    const opening = session.open("broken.bfp");
    await vi.waitFor(() => expect(session.unsaved).not.toBeNull());
    session.answerUnsaved("discard");
    expect(await opening).toBe(false);
    expect(session.error).toBe(
      "Couldn't open broken.bfp. This isn't a valid Beam Forge project: the JSON is malformed.",
    );
    expect(editor.doc.objects).toHaveLength(1);
  });

  it("copies unsaved work to the recovery file only when it changed", async () => {
    const { disk, editor, session } = setup();
    const saveRecovery = vi.spyOn(disk.adapter, "saveRecovery");
    await session.writeRecovery();
    expect(saveRecovery).not.toHaveBeenCalled();

    editor.execute(insert([rect("a")]));
    await session.writeRecovery();
    await session.writeRecovery();
    expect(saveRecovery).toHaveBeenCalledOnce();
    expect(disk.recovery?.project.document.objects).toHaveLength(1);

    disk.adapter.dialogPath = "a.bfp";
    await session.save();
    expect(disk.recovery).toBeNull();
  });

  it("offers recovered work at launch, then opens the launch file", async () => {
    const first = setup();
    first.editor.execute(insert([rect("a")]));
    await first.session.writeRecovery();

    const { disk, editor, lifecycle, session } = setup(first.disk);
    disk.files.set("launch.bfp", JSON.stringify(session.snapshot()));
    disk.setLaunch("launch.bfp");
    await session.start(60_000);
    expect(session.recovery?.name).toBe("Untitled");
    expect(editor.doc.objects).toEqual([]);

    // Restoring makes the work unsaved, so opening the launch file asks first.
    const restoring = session.restoreRecovery();
    await vi.waitFor(() => expect(session.unsaved).not.toBeNull());
    expect(editor.doc.objects).toHaveLength(1);
    expect(lifecycle.dirty).toBe(true);
    session.answerUnsaved("cancel");
    await restoring;
    expect(session.name).toBe("Untitled");
  });

  it("opens the file it was launched with", async () => {
    const { disk, session } = setup();
    disk.files.set("C:/a/Sign.bfp", JSON.stringify(session.snapshot()));
    disk.setLaunch("C:/a/Sign.bfp");
    const stop = await session.start(60_000);
    expect(session.name).toBe("Sign");
    stop();
  });
});

describe("browser preview projects", () => {
  it("says files need the desktop app", async () => {
    const lifecycle = new Lifecycle();
    const session = new ProjectSession({
      adapter: previewProjects,
      editor: new DocumentEditor(lifecycle),
      lifecycle,
      profile: () => null,
    });
    expect(session.available).toBe(false);
    expect(await session.saveAs()).toBe(false);
    expect(session.error).toBe(`Couldn't save Untitled. ${PREVIEW_NO_FILES}`);
  });
});
