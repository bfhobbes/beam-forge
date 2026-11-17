import assert from "node:assert/strict";
import { ESLint } from "eslint";

const eslint = new ESLint({
  overrideConfig: [{ files: ["**/*.svelte"], rules: { "shadcn/no-arbitrary-values": "error" } }],
});
const bad = await eslint.lintText('<div class="p-[13px]">Deliberate design violation</div>', {
  filePath: "src/design-proof.svelte",
});
assert.ok(
  bad[0].messages.some((message) => message.ruleId === "shadcn/no-arbitrary-values"),
  "Shadcn must inspect Svelte markup",
);
const good = await eslint.lintText('<div class="p-4">Token-based spacing</div>', {
  filePath: "src/design-proof.svelte",
});
assert.equal(good[0].errorCount, 0);
console.log("PASS: shadcn rejects arbitrary spacing in Svelte markup and accepts token spacing.");
