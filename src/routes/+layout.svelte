<script lang="ts">
  import "./layout.css";
  import { onMount } from "svelte";
  import favicon from "#lib/assets/favicon.svg";
  import CloseGuard from "#lib/desktop/CloseGuard.svelte";
  import { darkSchemeQuery, followSystemTheme } from "#lib/shell/theme.js";
  import type { LayoutProps } from "./$types";

  let { children }: LayoutProps = $props();

  // The CSP forbids inline scripts, so the theme is applied here rather than in app.html.
  onMount(() => followSystemTheme(document.documentElement, matchMedia(darkSchemeQuery)));
</script>

<svelte:head>
  <link rel="icon" href={favicon} />
  <title>Beam Forge</title>
</svelte:head>

{@render children()}
<CloseGuard />
