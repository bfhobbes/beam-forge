<script lang="ts">
  import { Input } from "#lib/components/ui/input/index.js";
  import { evaluate } from "./expression.js";

  /**
   * A numeric text field that accepts arithmetic (`120/2`). Enter or leaving
   * the field commits; Escape reverts. `oncommit` returns false to refuse a
   * value, which marks the field invalid until it is fixed or abandoned.
   */
  let {
    label,
    value,
    unit,
    digits = 3,
    wide = false,
    oncommit,
  }: {
    label: string;
    value: number;
    unit: string;
    /** Decimal places shown. */
    digits?: number;
    /** Room for word labels and units such as "Speed" and "mm/min". */
    wide?: boolean;
    oncommit: (value: number) => boolean;
  } = $props();

  let draft = $state<string | null>(null);
  let invalid = $state(false);

  // `|| 0` turns -0 into 0.
  const shown = $derived(draft ?? String(Number(value.toFixed(digits)) || 0));

  function commit(): boolean {
    if (draft === null) return true;
    const parsed = evaluate(draft);
    if (parsed === null || !oncommit(parsed)) {
      invalid = true;
      return false;
    }
    draft = null;
    invalid = false;
    return true;
  }

  function revert() {
    draft = null;
    invalid = false;
  }

  function onkeydown(event: KeyboardEvent & { currentTarget: HTMLInputElement }) {
    if (event.key === "Enter") {
      event.preventDefault();
      if (commit()) event.currentTarget.select();
    } else if (event.key === "Escape") {
      event.preventDefault();
      revert();
      event.currentTarget.blur();
    }
  }
</script>

<label class="flex items-center gap-1.5 text-xs">
  <span class={[wide ? "w-20" : "w-6", "shrink-0 text-muted-foreground"]}>{label}</span>
  <Input
    class="h-7 px-2 text-right tabular-nums md:text-xs"
    value={shown}
    aria-invalid={invalid || undefined}
    inputmode="decimal"
    autocomplete="off"
    spellcheck={false}
    oninput={(event) => (draft = event.currentTarget.value)}
    onfocus={(event) => event.currentTarget.select()}
    onblur={() => commit() || revert()}
    {onkeydown}
  />
  <span class={[wide ? "w-12" : "w-6", "shrink-0 text-muted-foreground"]}>{unit}</span>
</label>
