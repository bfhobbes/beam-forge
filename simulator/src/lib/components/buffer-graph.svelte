<script lang="ts">
  interface Props {
    label: string;
    unit: string;
    /** Fill per sample, oldest first. */
    values: number[];
    capacity: number;
    /** Samples during which the buffer overflowed, flagged in red. */
    overflowed?: boolean[];
    /** How many samples the graph spans. */
    span: number;
  }

  let { label, unit, values, capacity, overflowed = [], span }: Props = $props();

  const width = 200;
  const height = 40;
  const step = $derived(width / Math.max(1, span - 1));
  const offset = $derived(Math.max(0, span - values.length));
  const y = (value: number) =>
    height - (Math.min(value, capacity) / Math.max(1, capacity)) * height;
  const points = $derived(
    values.map((value, index) => `${(offset + index) * step},${y(value)}`).join(" "),
  );
  const current = $derived(values.at(-1) ?? 0);
  const full = $derived(capacity > 0 && current >= capacity);
</script>

<figure class="space-y-1">
  <figcaption class="flex justify-between text-xs">
    <span class="text-muted-foreground">{label}</span>
    <span class={["font-mono", full && "text-destructive"]}>{current}/{capacity} {unit}</span>
  </figcaption>
  <svg
    viewBox="0 0 {width} {height}"
    preserveAspectRatio="none"
    class="h-10 w-full rounded-md border bg-muted/40"
    role="img"
    aria-label="{label}: {current} of {capacity} {unit}"
  >
    {#each overflowed as flag, index (index)}
      {#if flag}
        <rect x={(offset + index) * step - 1} y="0" width="2" {height} class="fill-destructive" />
      {/if}
    {/each}
    {#if values.length}
      <polyline
        {points}
        fill="none"
        stroke-width="1.5"
        vector-effect="non-scaling-stroke"
        class="stroke-foreground"
      />
    {/if}
  </svg>
</figure>
