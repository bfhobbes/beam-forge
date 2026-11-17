//! `.bfp` project files: JSON holding a `formatVersion`, the design document
//! (objects and cut layers) and a reference to the machine profile it was
//! made for. Loading validates
//! the file and migrates older formats to the current one.
//!
//! Also the app-data stores that go with projects: the recent-files list
//! ([`RecentStore`]) and the crash-recovery copy ([`RecoveryStore`]).

mod document;
mod layer;
mod store;

pub use document::{DesignDocument, Point, SceneObject, Segment, Subpath, Transform};
pub use layer::{CutLayer, LayerMode, LayerPreset};
pub use store::{RecentProject, RecentStore, Recovery, RecoveryStore, project_name};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

/// The format this build writes. Bump it with a migration in [`migrate`].
pub const FORMAT_VERSION: u32 = 2;

/// The file extension, without the dot.
pub const EXTENSION: &str = "bfp";

/// The machine profile a project was made for. The name is kept so the UI can
/// say which machine it means even when that profile doesn't exist here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "project/")]
pub struct ProfileRef {
    pub id: String,
    pub name: String,
}

/// The contents of a `.bfp` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "project/")]
pub struct Project {
    pub format_version: u32,
    pub document: DesignDocument,
    pub machine_profile: Option<ProfileRef>,
}

/// Why a project couldn't be read or written, worded for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectError(String);

impl ProjectError {
    fn invalid(detail: String) -> Self {
        ProjectError(format!("This isn't a valid Beam Forge project: {detail}."))
    }
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProjectError {}

impl From<std::io::Error> for ProjectError {
    fn from(error: std::io::Error) -> Self {
        ProjectError(error.to_string())
    }
}

impl Project {
    pub fn new(document: DesignDocument, machine_profile: Option<ProfileRef>) -> Self {
        Project {
            format_version: FORMAT_VERSION,
            document,
            machine_profile,
        }
    }

    /// Reads a project file's text: parses, migrates and validates it.
    pub fn from_json(json: &str) -> Result<Project, ProjectError> {
        let value: Value = serde_json::from_str(json)
            .map_err(|error| ProjectError::invalid(format!("the JSON is malformed ({error})")))?;
        Project::from_value(value)
    }

    /// Migrates and validates an already-parsed project.
    pub fn from_value(value: Value) -> Result<Project, ProjectError> {
        let project: Project = serde_json::from_value(migrate(value)?)
            .map_err(|error| ProjectError::invalid(error.to_string()))?;
        project.validate()?;
        Ok(project)
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.format_version != FORMAT_VERSION {
            return Err(ProjectError::invalid(format!(
                "expected format version {FORMAT_VERSION}, not {}",
                self.format_version
            )));
        }
        self.document.validate()
    }

    /// The file's text: pretty JSON, so diffs and hand repairs are possible.
    pub fn to_json(&self) -> Result<String, ProjectError> {
        self.validate()?;
        serde_json::to_string_pretty(self).map_err(|error| ProjectError(error.to_string()))
    }
}

/// Upgrades a parsed file to [`FORMAT_VERSION`], one version at a time.
/// A format change adds an arm here that rewrites the value to the next
/// version.
fn migrate(mut value: Value) -> Result<Value, ProjectError> {
    let Some(version) = value.get("formatVersion") else {
        return Err(ProjectError::invalid("it has no formatVersion".to_string()));
    };
    let Some(version) = version.as_u64() else {
        return Err(ProjectError::invalid(format!(
            "formatVersion {version} isn't a version number"
        )));
    };
    if version > u64::from(FORMAT_VERSION) {
        return Err(ProjectError(format!(
            "This project was saved by a newer Beam Forge (format {version}). \
             Update Beam Forge to open it."
        )));
    }
    match version {
        1 => {
            add_layers(&mut value);
            value["formatVersion"] = Value::from(2);
            migrate(value)
        }
        2 => Ok(value),
        _ => Err(ProjectError::invalid(format!(
            "format version {version} doesn't exist"
        ))),
    }
}

/// Format 1 to 2: format 1 had layer ids on objects but no layers. Each id
/// in use becomes a layer with default settings, in the order the ids first
/// appear, coloured as format 1 drew it.
fn add_layers(value: &mut Value) {
    fn collect<'a>(objects: &'a Value, ids: &mut Vec<&'a str>) {
        for object in objects.as_array().into_iter().flatten() {
            if let Some(id) = object.get("layerId").and_then(Value::as_str)
                && !ids.contains(&id)
            {
                ids.push(id);
            }
            if let Some(children) = object.get("children") {
                collect(children, ids);
            }
        }
    }
    let Some(document) = value.get_mut("document").filter(|d| d.is_object()) else {
        // Not a document: deserializing reports it.
        return;
    };
    let mut ids = Vec::new();
    if let Some(objects) = document.get("objects") {
        collect(objects, &mut ids);
    }
    let layers: Vec<Value> = ids
        .iter()
        .map(|&id| {
            let name = match id.strip_prefix("layer-") {
                Some(number) if number.parse::<u32>().is_ok() => format!("Layer {number}"),
                _ => id.to_string(),
            };
            let layer = CutLayer::new(id, name, layer::format_1_color(id));
            serde_json::to_value(layer).unwrap_or_default()
        })
        .collect();
    document["layers"] = Value::from(layers);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        let identity = json!([1, 0, 0, 1, 0, 0]);
        let mut sample = json!({
            "formatVersion": 1,
            "machineProfile": { "id": "serial-com3", "name": "Phecda" },
            "document": {
                "version": 1,
                "units": "mm",
                "objects": [
                    {
                        "type": "rect", "id": "r", "name": "Rectangle", "layerId": "C00",
                        "transform": [1, 0, 0, 1, 10.5, 20.25],
                        "width": 30, "height": 20, "cornerRadius": 2
                    },
                    {
                        "type": "group", "id": "g", "name": "Group", "layerId": "C00",
                        "transform": identity,
                        "children": [
                            { "type": "ellipse", "id": "e", "name": "Ellipse", "layerId": "C01",
                              "transform": identity, "rx": 5, "ry": 3 },
                            { "type": "polygon", "id": "p", "name": "Star", "layerId": "C01",
                              "transform": [0.5, 0.866, -0.866, 0.5, 0, 0],
                              "sides": 5, "radius": 10, "innerRadius": null },
                            { "type": "path", "id": "a", "name": "Path", "layerId": "C00",
                              "transform": identity,
                              "subpaths": [{
                                  "start": { "x": 0, "y": 0 },
                                  "closed": true,
                                  "segments": [
                                      { "kind": "line", "to": { "x": 1, "y": 0 } },
                                      { "kind": "quad", "control": { "x": 1, "y": 1 }, "to": { "x": 0, "y": 1 } },
                                      { "kind": "cubic", "control1": { "x": -1, "y": 1 },
                                        "control2": { "x": -1, "y": 0 }, "to": { "x": 0.1, "y": 0.2 } }
                                  ]
                              }] }
                        ]
                    }
                ]
            }
        });
        let text = json!({
            "type": "text", "id": "t", "name": "Text", "layerId": "C00",
            "transform": identity, "text": "Hi
        there",
            "fontFamily": "Arial", "fontWeight": 700, "italic": true,
            "height": 8, "letterSpacing": 0.5, "lineSpacing": 1.2, "align": "center",
            "outline": [{
                "start": { "x": 0, "y": 0 },
                "closed": true,
                "segments": [{ "kind": "line", "to": { "x": 1, "y": 0 } }]
            }]
        });
        sample["document"]["objects"][1]["children"]
            .as_array_mut()
            .unwrap()
            .push(text);
        sample
    }

    #[test]
    fn round_trips_every_object_kind_exactly() {
        let project = Project::from_value(sample()).unwrap();
        let json = project.to_json().unwrap();
        let again = Project::from_json(&json).unwrap();
        assert_eq!(again, project);
        // Numbers compare as floats: 1 and 1.0 are the same document.
        let written: Value = serde_json::from_str(&json).unwrap();
        let expected = serde_json::to_value(Project::from_value(sample()).unwrap()).unwrap();
        assert_eq!(written, expected);
        assert_eq!(
            written["document"]["objects"][1]["children"][1]["innerRadius"],
            Value::Null
        );
    }

    #[test]
    fn text_needs_a_height_line_spacing_and_weight() {
        for (field, value) in [
            ("height", json!(0)),
            ("lineSpacing", json!(-1)),
            ("fontWeight", json!(0)),
        ] {
            let mut value_doc = sample();
            value_doc["document"]["objects"][1]["children"][3][field] = value;
            let error = Project::from_value(value_doc).unwrap_err();
            assert!(error.to_string().contains("text t"), "{field}: {error}");
        }
    }

    #[test]
    fn malformed_json_is_a_clear_error() {
        let error = Project::from_json("{ \"formatVersion\": 1, ").unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("This isn't a valid Beam Forge project: the JSON is malformed"),
            "{error}"
        );
    }

    #[test]
    fn a_file_without_a_format_version_is_refused() {
        let error = Project::from_json("{}").unwrap_err();
        assert_eq!(
            error.to_string(),
            "This isn't a valid Beam Forge project: it has no formatVersion."
        );
        let error = Project::from_json("[1, 2]").unwrap_err();
        assert!(error.to_string().contains("no formatVersion"), "{error}");
    }

    #[test]
    fn a_newer_format_asks_for_an_update() {
        let mut value = sample();
        value["formatVersion"] = json!(FORMAT_VERSION + 1);
        let error = Project::from_value(value).unwrap_err();
        assert!(error.to_string().contains("newer Beam Forge"), "{error}");
    }

    #[test]
    fn missing_and_mistyped_fields_name_the_problem() {
        let mut value = sample();
        value["document"]["objects"][0]
            .as_object_mut()
            .unwrap()
            .remove("width");
        let error = Project::from_value(value).unwrap_err();
        assert!(
            error.to_string().contains("missing field `width`"),
            "{error}"
        );

        let mut value = sample();
        value["document"]["objects"][0]["type"] = json!("hexagon");
        let error = Project::from_value(value).unwrap_err();
        assert!(error.to_string().contains("hexagon"), "{error}");
    }

    #[test]
    fn validation_rejects_duplicate_ids_and_impossible_shapes() {
        let mut value = sample();
        value["document"]["objects"][1]["children"][0]["id"] = json!("r");
        let error = Project::from_value(value).unwrap_err();
        assert!(
            error.to_string().contains("object id r is used twice"),
            "{error}"
        );

        let mut value = sample();
        value["document"]["objects"][0]["width"] = json!(-1);
        let error = Project::from_value(value).unwrap_err();
        assert!(error.to_string().contains("negative width"), "{error}");

        let mut value = sample();
        value["document"]["objects"][1]["children"][1]["sides"] = json!(2);
        let error = Project::from_value(value).unwrap_err();
        assert!(error.to_string().contains("fewer than 3 sides"), "{error}");

        let mut value = sample();
        value["document"]["units"] = json!("in");
        assert!(Project::from_value(value).is_err());
    }

    #[test]
    fn format_1_files_gain_a_layer_per_layer_id_in_use() {
        let project = Project::from_value(sample()).unwrap();
        assert_eq!(project.format_version, FORMAT_VERSION);
        let layers: Vec<_> = project
            .document
            .layers
            .iter()
            .map(|layer| (layer.id.as_str(), layer.name.as_str(), layer.color.as_str()))
            .collect();
        // First-use order, groups before their children; colours as format 1 drew them.
        assert_eq!(
            layers,
            [("C00", "C00", "#808080"), ("C01", "C01", "#0000ff")]
        );
        assert_eq!(
            project.document.layers[0],
            CutLayer::new("C00", "C00", "#808080")
        );

        let mut value = sample();
        value["document"]["objects"][0]["layerId"] = json!("layer-3");
        let project = Project::from_value(value).unwrap();
        assert_eq!(project.document.layers[0].name, "Layer 3");
        assert_eq!(project.document.layers[0].color, "#00e000");

        let mut value = sample();
        value["document"]["objects"] = json!([]);
        let project = Project::from_value(value).unwrap();
        assert!(project.document.layers.is_empty());
    }

    #[test]
    fn layer_settings_save_and_reload() {
        let mut project = Project::from_value(sample()).unwrap();
        let layer = &mut project.document.layers[1];
        layer.mode = LayerMode::Fill;
        layer.speed = 3000.0;
        layer.max_power = 80.0;
        layer.min_power = 10.0;
        layer.passes = 2;
        layer.air_assist = false;
        layer.output = false;
        layer.shown = false;
        layer.line_interval = 0.08;
        layer.scan_angle = 45.0;
        layer.bidirectional = false;
        layer.overscan = 0.0;
        layer.fill_gap = 0.5;
        project.document.layers.swap(0, 1);
        let json = project.to_json().unwrap();
        assert!(json.contains("\"formatVersion\": 2"), "{json}");
        assert!(json.contains("\"mode\": \"fill\""), "{json}");
        assert_eq!(Project::from_json(&json).unwrap(), project);
    }

    #[test]
    fn layers_saved_before_the_fill_gap_get_the_default() {
        let project = Project::from_value(sample()).unwrap();
        let mut value = serde_json::to_value(&project).unwrap();
        for layer in value["document"]["layers"].as_array_mut().unwrap() {
            layer.as_object_mut().unwrap().remove("fillGap").unwrap();
        }
        let reloaded = Project::from_value(value).unwrap();
        assert_eq!(reloaded.document.layers[0].fill_gap, 1.0);
        assert_eq!(reloaded, project);
    }

    #[test]
    fn a_layers_preset_saves_and_reloads_and_older_layers_have_none() {
        use crate::presets::PresetLibrary;

        let mut project = Project::from_value(sample()).unwrap();
        let plywood = PresetLibrary::starters().presets[0].clone();
        project.document.layers[0].preset = Some(LayerPreset {
            id: plywood.id.clone(),
            name: "3 mm Plywood · Cut".into(),
            settings: plywood.settings,
        });
        let json = project.to_json().unwrap();
        assert!(json.contains("\"preset\": {"), "{json}");
        let reloaded = Project::from_json(&json).unwrap();
        assert_eq!(reloaded, project);
        assert_eq!(reloaded.document.layers[1].preset, None);

        // A layer without a preset writes no field, as files did before presets.
        let value: Value = serde_json::from_str(&json).unwrap();
        assert!(value["document"]["layers"][1].get("preset").is_none());

        project.document.layers[0]
            .preset
            .as_mut()
            .unwrap()
            .settings
            .passes = 0;
        let error = project.to_json().unwrap_err();
        assert!(
            error.to_string().contains("a preset with no passes"),
            "{error}"
        );
    }

    #[test]
    fn objects_must_be_on_a_layer_the_document_has() {
        let mut project = Project::from_value(sample()).unwrap();
        project.document.layers.pop();
        let error = project.to_json().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("object e is on layer C01, which the document doesn't have"),
            "{error}"
        );

        let mut project = Project::from_value(sample()).unwrap();
        project.document.layers[1].id = "C00".to_string();
        let error = project.to_json().unwrap_err();
        assert!(
            error.to_string().contains("layer id C00 is used twice"),
            "{error}"
        );

        let mut project = Project::from_value(sample()).unwrap();
        project.document.layers[0].passes = 0;
        assert!(project.to_json().is_err());
    }

    #[test]
    fn writing_refuses_an_invalid_project() {
        let mut project = Project::from_value(sample()).unwrap();
        project.document.version = 2;
        assert!(project.to_json().is_err());
    }
}
