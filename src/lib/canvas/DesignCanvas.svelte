<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { actions } from "#lib/actions/registry.svelte.js";
  import { isTyping } from "#lib/actions/shortcut.js";
  import { addLayer, batch, insert } from "#lib/document/commands.js";
  import { drawingLayer } from "#lib/document/drawing-layer.svelte.js";
  import { editor } from "#lib/document/editor.svelte.js";
  import type { Point } from "#lib/document/model.js";
  import { selection } from "#lib/document/selection.svelte.js";
  import { arrowDirection, machineControl } from "#lib/machine/control.svelte.js";
  import { bedBounds, type Bed } from "./bed.js";
  import { drawFrame, readTheme, RULER_SIZE, type CanvasTheme } from "./render.js";
  import type { ShapeTool } from "./draw.js";
  import { DrawTool } from "./draw-tool.svelte.js";
  import { benchmarkDocument, buildScene } from "./scene.js";
  import { SelectTool } from "./select-tool.svelte.js";
  import { findPlacement } from "#lib/document/model.js";
  import { textEditing } from "#lib/text/text-editing.svelte.js";
  import type { ToolpathDrawing } from "#lib/toolpath/render.js";
  import { PICK_REACH } from "./select-tool.svelte.js";
  import {
    fitBounds,
    panBy,
    screenToWorld,
    worldToScreen,
    wheelGesture,
    zoomAt,
    type Bounds,
    type View,
  } from "./view.js";

  /**
   * The design canvas: renders the open document over the bed, with zoom at
   * the cursor, pan (space+drag, middle mouse, two-finger scroll) and fit.
   * With `selecting` on, the primary button selects and transforms;
   * with a `drawing` tool, it draws that shape and calls `ondrawn`.
   * `cursor` reports the pointer's world position in mm for the status bar.
   * With a `toolpath`, the canvas draws it instead of the design. With
   * `texting`, a click starts a new text there or edits the text under it;
   * double-clicking text with the select tool edits it too.
   */
  let {
    bed,
    selecting = true,
    drawing = null,
    ondrawn = () => {},
    cursor = $bindable(null),
    toolpath = null,
    texting = false,
    showFills = false,
  }: {
    /** The machine's work area: the active profile's bed. */
    bed: Bed;
    /** Whether the select tool is active. */
    selecting?: boolean;
    /** The drawing tool that is active, if any. */
    drawing?: ShapeTool | null;
    /** Runs after a drawing tool finishes a shape. */
    ondrawn?: () => void;
    cursor?: Point | null;
    /** The toolpath view: the planned moves and how far along they are. */
    toolpath?: ToolpathDrawing | null;
    /** Whether the text tool is active. */
    texting?: boolean;
    /** Shade the shapes on Fill layers. */
    showFills?: boolean;
  } = $props();

  const ZOOM_STEP = 1.25;
  const FIT_PADDING = 24;

  let canvas = $state<HTMLCanvasElement>();
  let size = $state.raw({ width: 0, height: 0, dpr: 1 });
  let view = $state.raw<View | null>(null);
  let theme = $state.raw<CanvasTheme | null>(null);
  let pointer = $state.raw<Point | null>(null);
  let spaceHeld = $state(false);
  let panning = $state(false);
  const selectTool = new SelectTool(editor, selection);
  const drawTool = new DrawTool(editor, selection, drawingLayer, () => ondrawn());

  // Switching tools abandons a half-drawn shape.
  $effect(() => {
    void drawing;
    drawTool.cancel();
  });

  const scene = $derived(buildScene(editor.doc));

  /** The canvas area right of and below the rulers, in CSS px. */
  const content = $derived({
    x: RULER_SIZE,
    y: RULER_SIZE,
    width: Math.max(1, size.width - RULER_SIZE),
    height: Math.max(1, size.height - RULER_SIZE),
  });

  function setView(next: View) {
    view = next;
    cursor = pointer ? screenToWorld(next, pointer) : null;
  }

  function fit(bounds: Bounds) {
    setView(fitBounds(bounds, content, FIT_PADDING));
  }

  // A different bed (another machine's profile, or the profile loading after
  // the first fit) is fitted again; the same size keeps the user's zoom.
  let fittedBed = "";
  $effect(() => {
    const key = `${bed.width}x${bed.height}`;
    untrack(() => {
      if (view && fittedBed && key !== fittedBed) fit(bedBounds(bed));
      if (view) fittedBed = key;
    });
  });

  function zoomAtCentre(factor: number) {
    if (!view) return;
    setView(
      zoomAt(view, { x: content.x + content.width / 2, y: content.y + content.height / 2 }, factor),
    );
  }

  const selectionBounds = $derived(scene.boundsOf(selection.ids));
  const overlay = $derived({
    selected: new Set(selection.ids),
    bounds: selectionBounds,
    handles: selecting && view ? selectTool.handles(view, scene) : [],
    marquee: selectTool.marquee,
    draft: drawTool.draft,
    text: textEditing.caret(),
  });
  const pointerCursor = $derived(
    panning
      ? "grabbing"
      : spaceHeld
        ? "grab"
        : selecting && view
          ? selectTool.cursor(pointer, view, scene)
          : "crosshair",
  );

  onMount(() => {
    const unregister = actions.register(
      {
        id: "view.zoom-in",
        title: "Zoom in",
        group: "View",
        run: () => zoomAtCentre(ZOOM_STEP),
      },
      {
        id: "view.zoom-out",
        title: "Zoom out",
        group: "View",
        run: () => zoomAtCentre(1 / ZOOM_STEP),
      },
      {
        id: "view.zoom-fit",
        title: "Zoom to bed",
        group: "View",
        keywords: ["fit", "frame", "work area", "reset view"],
        run: () => fit(bedBounds(bed)),
      },
      {
        id: "view.zoom-selection",
        title: "Zoom to selection",
        group: "View",
        keywords: ["fit", "frame", "focus"],
        enabled: () => selectionBounds !== null,
        hint: "Select something first",
        run: () => {
          if (selectionBounds) fit(selectionBounds);
        },
      },
    );
    // A stress test for the 60 fps target: 10k segments over the bed.
    const unregisterDebug = import.meta.env.DEV
      ? actions.register({
          id: "debug.benchmark-scene",
          title: "Add benchmark scene (10k segments)",
          group: "Debug",
          keywords: ["performance", "stress", "fps"],
          run: () => {
            const bench = benchmarkDocument(10_000);
            const have = new Set(editor.doc.layers.map((layer) => layer.id));
            const layers = bench.layers.filter((layer) => !have.has(layer.id));
            editor.execute(
              batch([...layers.map((layer) => addLayer(layer)), insert(bench.objects)]),
            );
          },
        })
      : () => {};

    // The canvas reads theme colours from CSS variables; re-read when `.dark` toggles.
    const root = document.documentElement;
    const refreshTheme = () => canvas && (theme = readTheme(canvas));
    refreshTheme();
    const themeWatch = new MutationObserver(refreshTheme);
    themeWatch.observe(root, { attributes: true, attributeFilter: ["class", "style"] });

    return () => {
      unregister();
      unregisterDebug();
      themeWatch.disconnect();
    };
  });

  // Repaint at most once per animation frame, whenever anything drawn changes.
  $effect(() => {
    if (!canvas || !view || !theme || size.width === 0) return;
    const target = canvas;
    const frame = { view, scene, bed, theme, overlay, toolpath, showFills, ...size };
    const id = requestAnimationFrame(() => {
      const width = Math.round(frame.width * frame.dpr);
      const height = Math.round(frame.height * frame.dpr);
      if (target.width !== width) target.width = width;
      if (target.height !== height) target.height = height;
      const ctx = target.getContext("2d");
      if (ctx) drawFrame(ctx, frame);
    });
    return () => cancelAnimationFrame(id);
  });

  function local(event: MouseEvent): Point {
    const rect = canvas!.getBoundingClientRect();
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function onwheel(event: WheelEvent) {
    if (!view) return;
    event.preventDefault();
    const gesture = wheelGesture(event);
    if (gesture.kind === "zoom") setView(zoomAt(view, local(event), gesture.factor));
    else setView(panBy(view, gesture.dx, gesture.dy));
  }

  const pointerInput = (event: PointerEvent) => ({
    at: local(event),
    shift: event.shiftKey,
    alt: event.altKey,
  });

  /** The text object under a canvas point, if any. */
  function textAt(at: Point): string | null {
    if (!view) return null;
    const id = scene.hitTest(screenToWorld(view, at), PICK_REACH / view.zoom);
    const object = id ? findPlacement(editor.doc.objects, id)?.object : undefined;
    return object?.type === "text" ? object.id : null;
  }

  function onpointerdown(event: PointerEvent) {
    if (event.button === 0 && !spaceHeld && texting && view) {
      event.preventDefault();
      const at = local(event);
      const id = textAt(at);
      if (id) textEditing.edit(id);
      else textEditing.create(screenToWorld(view, at));
      return;
    }
    const pan = event.button === 1 || (event.button === 0 && spaceHeld);
    const select = event.button === 0 && !spaceHeld && selecting && view;
    const draw = event.button === 0 && !spaceHeld && drawing && view;
    if (!pan && !select && !draw) return;
    event.preventDefault();
    // Clicking the canvas leaves any Inspector field, committing its edit
    // first, and gives the canvas the keyboard for arrow-key jogging.
    canvas!.focus({ preventScroll: true });
    canvas!.setPointerCapture(event.pointerId);
    pointer = local(event);
    if (pan) panning = true;
    else if (drawing) drawTool.down(pointerInput(event), view!, drawing);
    else selectTool.down(pointerInput(event), view!, scene);
  }

  function onpointermove(event: PointerEvent) {
    const at = local(event);
    if (panning && view && pointer) {
      // Deltas from client positions: movementX is in device pixels on some platforms.
      const from = pointer;
      pointer = at;
      setView(panBy(view, at.x - from.x, at.y - from.y));
    } else {
      pointer = at;
      if (view) {
        cursor = screenToWorld(view, at);
        if (drawing) drawTool.move(pointerInput(event), view);
        else selectTool.move(pointerInput(event), view);
      }
    }
  }

  function onpointerup(event: PointerEvent) {
    if (canvas!.hasPointerCapture(event.pointerId)) canvas!.releasePointerCapture(event.pointerId);
    if (panning) panning = false;
    else if (view && drawing) drawTool.up(view);
    else if (view) selectTool.up(view, scene);
  }

  function onpointerleave() {
    if (panning) return;
    pointer = null;
    cursor = null;
  }

  // Space is a held modifier, not a shortcut, so it stays out of the action
  // registry. It only arms panning while the pointer is over the canvas.
  function onkeydown(event: KeyboardEvent) {
    if (modifierKey(event)) return;
    if (event.key === "Escape") {
      selectTool.cancel();
      drawTool.cancel();
    }
    if (event.key === "Enter" && drawTool.drawingLine && !isTyping(event.target)) {
      event.preventDefault();
      drawTool.finish();
    }
    if (event.key !== " " || !pointer || isTyping(event.target)) return;
    event.preventDefault();
    spaceHeld = true;
  }

  function onkeyup(event: KeyboardEvent) {
    if (modifierKey(event)) return;
    if (event.key === " ") spaceHeld = false;
  }

  /**
   * Arrow keys jog the machine one step while the canvas has focus, nothing
   * is selected (else they nudge the selection) and the machine can jog.
   */
  function oncanvaskeydown(event: KeyboardEvent) {
    const direction = arrowDirection(event.key);
    if (!direction || event.repeat || event.shiftKey || event.altKey) return;
    if (event.ctrlKey || event.metaKey || selection.ids.length > 0) return;
    if (!machineControl.canJog) return;
    event.preventDefault();
    void machineControl.jog(direction);
  }

  /** Shift and alt change a drag as they go down or up, without the pointer moving. */
  function modifierKey(event: KeyboardEvent): boolean {
    if (event.key !== "Shift" && event.key !== "Alt") return false;
    if ((!selectTool.transforming && !drawTool.dragging) || !view) return false;
    // Stop alt focusing the window menu mid-drag.
    event.preventDefault();
    const modifiers = { shift: event.shiftKey, alt: event.altKey };
    if (drawTool.dragging) drawTool.modifiers(modifiers, view);
    else selectTool.modifiers(modifiers, view);
    return true;
  }

  /**
   * Tracks the element's CSS size and the display's pixel ratio (which changes
   * when the window moves between monitors). Fits the bed once a size is known.
   */
  function observeSize(element: HTMLDivElement) {
    const measure = () => {
      const { width, height } = element.getBoundingClientRect();
      size = { width, height, dpr: window.devicePixelRatio || 1 };
      if (!view && width > 0 && height > 0) {
        fit(bedBounds(bed));
        fittedBed = `${bed.width}x${bed.height}`;
      }
    };
    const resize = new ResizeObserver(measure);
    resize.observe(element);
    let dprQuery = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    const onDpr = () => {
      measure();
      dprQuery.removeEventListener("change", onDpr);
      dprQuery = matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
      dprQuery.addEventListener("change", onDpr);
    };
    dprQuery.addEventListener("change", onDpr);
    return () => {
      resize.disconnect();
      dprQuery.removeEventListener("change", onDpr);
    };
  }

  /** Where the hidden text box sits: at the caret, so an IME's popup appears beside it. */
  const caretSpot = $derived.by(() => {
    const at = overlay.text?.caret?.to;
    if (!view || !at) return "0px 0px";
    const { x, y } = worldToScreen(view, at);
    return `${x}px ${y}px`;
  });

  function syncCaret(event: Event) {
    const box = event.currentTarget as HTMLTextAreaElement | null;
    if (box) textEditing.select(box.selectionStart, box.selectionEnd);
  }

  /** Focuses the text box as editing starts, caret where the editor has it. */
  function typing(box: HTMLTextAreaElement) {
    void tick().then(() => {
      box.focus({ preventScroll: true });
      box.setSelectionRange(textEditing.selectionStart, textEditing.selectionEnd);
    });
  }

  /** Wheel listeners must be non-passive to stop the page zooming or scrolling. */
  function nonPassiveWheel(element: HTMLCanvasElement) {
    element.addEventListener("wheel", onwheel, { passive: false });
    return () => element.removeEventListener("wheel", onwheel);
  }
</script>

<svelte:window {onkeydown} {onkeyup} onblur={() => (spaceHeld = false)} />

<div {@attach observeSize} class="relative h-full w-full overflow-hidden">
  <canvas
    bind:this={canvas}
    {@attach nonPassiveWheel}
    aria-label="Design canvas"
    tabindex="0"
    class="absolute inset-0 h-full w-full touch-none outline-none select-none"
    style:cursor={pointerCursor}
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    onpointercancel={onpointerup}
    {onpointerleave}
    onkeydown={oncanvaskeydown}
    ondblclick={(event) => {
      if (drawTool.drawingLine) drawTool.finish();
      else if (selecting) {
        const id = textAt(local(event));
        if (id) textEditing.edit(id);
      }
    }}
    onauxclick={(event) => event.preventDefault()}
    onmousedown={(event) => {
      // Middle-drag pans; a text-tool click keeps the keyboard in the text being typed.
      if (event.button === 1 || (event.button === 0 && texting)) event.preventDefault();
    }}
  ></canvas>
  {#if textEditing.editing}
    {@const id = textEditing.editing}
    {#key id}
      <textarea
        {@attach typing}
        aria-label="Text"
        class="pointer-events-none absolute top-0 left-0 size-px resize-none opacity-0"
        style:translate={caretSpot}
        value={textEditing.value}
        autocomplete="off"
        spellcheck={false}
        oninput={(event) => {
          const box = event.currentTarget;
          void textEditing.input(box.value, box.selectionStart, box.selectionEnd);
        }}
        onselect={syncCaret}
        onkeyup={syncCaret}
        onmouseup={syncCaret}
        onkeydown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            textEditing.finish();
            canvas?.focus({ preventScroll: true });
          } else requestAnimationFrame(() => syncCaret(event));
        }}
        onblur={() => textEditing.editing === id && textEditing.finish()}></textarea>
    {/key}
  {/if}
</div>
