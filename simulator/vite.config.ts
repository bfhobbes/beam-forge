import tailwindcss from "@tailwindcss/vite";
import adapter from "@sveltejs/adapter-static";
import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vite-plus";

const generated = ["src-tauri/**", "build/**", ".svelte-kit/**", "src/lib/bindings/**"];

// Mirrors the main app's vite.config.ts; the dev server port differs so both apps can run at once.
export default defineConfig({
  // Instantiate eagerly: lazyPlugins stops svelte-kit sync from generating $app/tsconfig.
  plugins: [
    tailwindcss(),
    sveltekit({
      compilerOptions: {
        // Force runes mode for the project, except for libraries. Can be removed in svelte 6.
        runes: ({ filename }) =>
          filename.split(/[/\\]/).includes("node_modules") ? undefined : true,
      },
      adapter: adapter({ fallback: "index.html" }),
    }),
  ],
  // Tauri expects a fixed port and must not watch native build output.
  server: {
    port: 1421,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
  },
  clearScreen: false,
  fmt: {
    svelte: true,
    sortTailwindcss: { stylesheet: "./src/routes/layout.css" },
    ignorePatterns: generated,
  },
  // Oxlint covers TS/JS. It does not read Svelte markup, so ESLint lints .svelte files.
  // typeCheck stays off: it cannot resolve .svelte module exports; svelte-check owns types.
  lint: {
    plugins: ["oxc", "typescript", "unicorn"],
    jsPlugins: [{ name: "vite-plus", specifier: "vite-plus/oxlint-plugin" }],
    categories: { correctness: "error" },
    env: { builtin: true, browser: true, node: true },
    ignorePatterns: generated,
    rules: {
      "no-unused-vars": "error",
      "no-var": "error",
      "prefer-const": "error",
      "typescript/no-explicit-any": "error",
      "vite-plus/prefer-vite-plus-imports": "error",
    },
    options: { typeAware: true },
  },
  test: { include: ["src/**/*.test.ts"] },
});
