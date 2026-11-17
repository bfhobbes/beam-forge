import { defineConfig } from "eslint/config";
import svelte from "eslint-plugin-svelte";
import ts from "typescript-eslint";
import { plugin as shadcn } from "@shadcn/lint";

// ESLint only lints Svelte components: Oxlint (vp lint) does not read Svelte markup.
export default defineConfig(
  {
    ignores: [
      "node_modules/**",
      ".svelte-kit/**",
      "build/**",
      "src-tauri/**",
      "target/**",
      ".local/**",
    ],
  },
  ...svelte.configs.recommended.map((config) => ({ ...config, files: ["**/*.svelte"] })),
  {
    files: ["src/lib/components/ui/button/button.svelte"],
    // A generic primitive forwards URLs; callers own SvelteKit URL resolution.
    rules: { "svelte/no-navigation-without-resolve": "off" },
  },
  {
    files: ["**/*.svelte"],
    languageOptions: { parserOptions: { parser: ts.parser } },
    plugins: { shadcn },
    rules: {
      // Design-system rules are opt-in; scripts/prove-design-lint.mjs proves the plugin sees markup.
    },
  },
);
