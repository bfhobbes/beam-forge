<script lang="ts">
  import type { OriginCorner } from "#lib/bindings/OriginCorner.js";

  /** The bed seen from above with the front at the bottom: where 0,0 is, and which way +X and +Y go. */
  let { origin }: { origin: OriginCorner } = $props();

  const bed = { left: 14, top: 8, width: 92, height: 56 };
  const ARROW = 30;
  const right = $derived(origin.endsWith("Right"));
  const rear = $derived(origin.startsWith("rear"));
  // The origin corner, and which way each axis grows from it on screen.
  const corner = $derived({
    x: right ? bed.left + bed.width : bed.left,
    y: rear ? bed.top : bed.top + bed.height,
  });
  const dx = $derived(right ? -1 : 1);
  const dy = $derived(rear ? 1 : -1);
  const xEnd = $derived({ x: corner.x + dx * ARROW, y: corner.y });
  const yEnd = $derived({ x: corner.x, y: corner.y + dy * ARROW });
  const label = $derived(
    `Origin at the ${rear ? "rear" : "front"} ${right ? "right" : "left"} corner: ` +
      `+X goes ${right ? "left" : "right"}, +Y goes ${rear ? "to the front" : "to the back"}`,
  );
</script>

<svg
  viewBox="0 0 120 84"
  class="h-auto w-full max-w-48 text-muted-foreground"
  role="img"
  aria-label={label}
>
  <defs>
    <marker
      id="origin-arrow"
      viewBox="0 0 6 6"
      refX="5"
      refY="3"
      markerWidth="5"
      markerHeight="5"
      orient="auto"
    >
      <path d="M0,0 L6,3 L0,6 Z" class="fill-primary" />
    </marker>
  </defs>
  <rect
    x={bed.left}
    y={bed.top}
    width={bed.width}
    height={bed.height}
    rx="2"
    class="fill-muted stroke-current"
    stroke-width="1"
  />
  <text x="60" y="80" text-anchor="middle" class="fill-current text-[8px]">Front (you)</text>
  <line
    x1={corner.x}
    y1={corner.y}
    x2={xEnd.x}
    y2={xEnd.y}
    class="stroke-primary"
    stroke-width="2"
    marker-end="url(#origin-arrow)"
  />
  <line
    x1={corner.x}
    y1={corner.y}
    x2={yEnd.x}
    y2={yEnd.y}
    class="stroke-primary"
    stroke-width="2"
    marker-end="url(#origin-arrow)"
  />
  <text
    x={xEnd.x + dx * 2}
    y={xEnd.y + (rear ? 12 : -4)}
    text-anchor={right ? "end" : "start"}
    class="fill-primary text-[8px] font-medium">+X</text
  >
  <text
    x={yEnd.x + (right ? -4 : 4)}
    y={yEnd.y + (rear ? 8 : 0)}
    text-anchor={right ? "end" : "start"}
    class="fill-primary text-[8px] font-medium">+Y</text
  >
  <circle cx={corner.x} cy={corner.y} r="3" class="fill-primary" />
  <text
    x={corner.x + (right ? 4 : -4)}
    y={corner.y + (rear ? -2 : 10)}
    text-anchor={right ? "start" : "end"}
    class="fill-current text-[7px]">0,0</text
  >
</svg>
