//! Native file dialogs, opened from Rust so the window never gets general file access.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;

/// A file type the dialog offers, e.g. `("SVG", &["svg", "svgz"])`.
pub type Filter<'a> = (&'a str, &'a [&'a str]);

/// Asks the user for one file to open. None when they cancel. Blocks, so
/// call it from an async command, never the main thread.
pub fn pick_file<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    filters: &[Filter],
) -> Option<PathBuf> {
    let mut dialog = app.dialog().file().set_title(title);
    for (name, extensions) in filters {
        dialog = dialog.add_filter(*name, extensions);
    }
    dialog.blocking_pick_file()?.into_path().ok()
}

/// Asks the user where to save one file, starting from `suggested` (a full
/// path or just a file name). None when they cancel. The first filter's first
/// extension is added when the user leaves it off. Blocks, like [`pick_file`].
pub fn pick_save_file<R: Runtime>(
    app: &AppHandle<R>,
    title: &str,
    filters: &[Filter],
    suggested: &Path,
) -> Option<PathBuf> {
    let mut dialog = app.dialog().file().set_title(title);
    for (name, extensions) in filters {
        dialog = dialog.add_filter(*name, extensions);
    }
    if let Some(dir) = suggested.parent().filter(|dir| dir.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(name) = suggested.file_name() {
        dialog = dialog.set_file_name(name.to_string_lossy());
    }
    let mut path = dialog.blocking_save_file()?.into_path().ok()?;
    if let Some(extension) = filters
        .first()
        .and_then(|(_, extensions)| extensions.first())
        && !has_extension(&path, filters)
    {
        let mut name = path.file_name().unwrap_or_default().to_owned();
        name.push(format!(".{extension}"));
        path.set_file_name(name);
    }
    Some(path)
}

/// True if `path` ends in one of the filters' extensions, ignoring case.
pub fn has_extension(path: &Path, filters: &[Filter]) -> bool {
    path.extension().is_some_and(|extension| {
        filters
            .iter()
            .flat_map(|(_, extensions)| extensions.iter())
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    })
}

/// The file's name without its extension, for naming what it brings in.
pub fn display_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}
