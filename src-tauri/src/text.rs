//! Text commands over `bf_core::text`: the system's fonts and laying text
//! out as outlines. Both run off the main thread; the font list loads once.

use bf_core::text::{self, FontFamily, TextLayout, TextStyle};

/// Starts loading the system's fonts in the background, so the text tool is
/// ready by the time it is first used.
pub fn warm() {
    std::thread::spawn(|| {
        text::system_fonts();
    });
}

pub(crate) async fn off_main<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())
}

/// The installed font families, sorted by name.
#[tauri::command]
pub async fn text_fonts() -> Result<Vec<FontFamily>, String> {
    off_main(|| text::fonts(text::system_fonts())).await
}

/// `text` set in `style`: its outline and caret positions.
#[tauri::command]
pub async fn text_layout(text: String, style: TextStyle) -> Result<TextLayout, String> {
    off_main(move || text::layout(text::system_fonts(), &text, &style))
        .await?
        .map_err(|e| e.to_string())
}
