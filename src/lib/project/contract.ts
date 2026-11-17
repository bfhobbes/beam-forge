import type { DesignDocument as StoredDocument } from "#lib/bindings/project/DesignDocument.js";
import type { DesignDocument } from "#lib/document/model.js";

/**
 * Rust validates project files against its own copy of the document types
 * (`crates/bf-core/src/project/document.rs`). These lines stop svelte-check if
 * `model.ts` and the generated bindings drift apart: update both together.
 */
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;

export const documentMatchesProjectFile: Same<DesignDocument, StoredDocument> = true;
