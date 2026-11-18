//! The material test-grid command over `bf_core::material_test`. Labels are
//! laid out with the system's fonts, off the main thread like the text tool.

use bf_core::machine::BedSize;
use bf_core::material_test::{self, TestGrid, TestGridSettings};
use bf_core::text;

use crate::text::off_main;

/// A speed × power test grid with ids made from `id`, or why it can't be
/// made (including that it doesn't fit on `bed`).
#[tauri::command]
pub async fn test_grid_generate(
    id: String,
    settings: TestGridSettings,
    bed: BedSize,
) -> Result<TestGrid, String> {
    off_main(move || material_test::test_grid(text::system_fonts(), &id, &settings, bed))
        .await?
        .map_err(|e| e.to_string())
}
