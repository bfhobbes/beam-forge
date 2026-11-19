//! Import commands over `bf_core::import`. Parsing runs off the main thread,
//! so a large file doesn't freeze the window.

use std::path::{Path, PathBuf};

use bf_core::import::{ImportError, ImportedDrawing, LengthUnit, dxf, svg};
use serde::Serialize;
use ts_rs::TS;

use crate::files;

/// A drawing read from a file, with a name for the objects it brings in.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportedFile {
    pub name: String,
    pub drawing: ImportedDrawing,
}

/// What reading a file came to.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum ImportOutcome {
    Imported {
        file: ImportedFile,
    },
    /// A DXF without units: ask the user, then call `import_file` again with
    /// `units`. `width` and `height` are the drawing's size in its own units.
    #[serde(rename_all = "camelCase")]
    NeedsUnits {
        #[ts(type = "string")]
        path: PathBuf,
        name: String,
        width: f64,
        height: f64,
    },
}

const SVG: files::Filter = ("SVG", &["svg", "svgz"]);
const DXF: files::Filter = ("DXF", &["dxf"]);
const PICK_FILTERS: &[files::Filter] = &[("SVG or DXF", &["svg", "svgz", "dxf"]), SVG, DXF];

async fn off_main<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ImportError> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn read(path: PathBuf, units: Option<LengthUnit>) -> Result<ImportOutcome, ImportError> {
    let data = std::fs::read(&path).map_err(|e| ImportError::Read(e.to_string()))?;
    let name = files::display_name(&path);
    let drawing = if files::has_extension(&path, &[DXF]) {
        match dxf::import_dxf(&data, units, bf_core::text::system_fonts()) {
            Err(ImportError::NeedsUnits { width, height }) => {
                return Ok(ImportOutcome::NeedsUnits {
                    path,
                    name,
                    width,
                    height,
                });
            }
            result => result?,
        }
    } else {
        svg::import_svg(&data)?
    };
    Ok(ImportOutcome::Imported {
        file: ImportedFile { name, drawing },
    })
}

fn check_importable(path: &Path) -> Result<(), String> {
    if files::has_extension(path, &[SVG, DXF]) {
        Ok(())
    } else {
        Err(format!(
            "{} is not an SVG or DXF file.",
            files::display_name(path)
        ))
    }
}

/// Asks for an SVG or DXF file and reads it. None when the user cancels.
#[tauri::command]
pub async fn import_pick<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<Option<ImportOutcome>, String> {
    off_main(move || {
        files::pick_file(&app, "Import", PICK_FILTERS)
            .map(|path| read(path, None))
            .transpose()
    })
    .await
}

/// Reads an SVG or DXF file by path, e.g. one dropped on the window, or a
/// DXF again once the user has chosen its `units`.
#[tauri::command]
pub async fn import_file(
    path: PathBuf,
    units: Option<LengthUnit>,
) -> Result<ImportOutcome, String> {
    check_importable(&path)?;
    off_main(move || read(path, units)).await
}

/// Reads SVG markup, e.g. pasted from the clipboard.
#[tauri::command]
pub async fn import_svg_text(text: String) -> Result<ImportedDrawing, String> {
    off_main(move || svg::import_svg_str(&text)).await
}
