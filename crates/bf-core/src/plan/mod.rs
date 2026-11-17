//! Toolpath planning: a design document in, the moves to make out.
//!
//! [`plan`] takes the document's objects (transforms applied, curves
//! flattened to polylines) and its cut layers, and returns a dialect-neutral
//! move list: rapids, and cuts at a power and speed. GCode generation turns it
//! into a dialect's commands; the toolpath preview draws it.
//!
//! Layers are planned in document order, each only if its output is on. Line
//! layers follow their outlines, inner shapes before the shapes around them
//! and otherwise nearest first, and repeat for each pass. Fill layers hatch
//! their closed shapes with scan lines ([`fill`]).

mod fill;
mod order;
mod outline;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::project::{CutLayer, DesignDocument, LayerMode, Point};

use outline::{Polyline, SAME_POINT, distance};

/// How far a flattened curve may stray from the true curve, mm.
pub const DEFAULT_TOLERANCE: f64 = 0.01;

/// Settings that aren't part of the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanOptions {
    /// Curve flattening tolerance, mm. Must be positive.
    pub tolerance: f64,
    /// Where the head is when the job starts.
    pub start: Point,
}

impl Default for PlanOptions {
    fn default() -> Self {
        PlanOptions {
            tolerance: DEFAULT_TOLERANCE,
            start: Point { x: 0.0, y: 0.0 },
        }
    }
}

/// One head movement, in document millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "plan/")]
pub enum Move {
    /// Travel with the laser off.
    Rapid { to: Point },
    /// Move in a straight line at a feed rate, lasing unless `power` is 0.
    /// Fills move at power 0 to overscan and to cross short gaps, so the
    /// head keeps its speed.
    Cut {
        to: Point,
        /// Percent of the machine's maximum.
        power: f64,
        /// Feed rate, mm/min.
        speed: f64,
    },
}

/// The moves for one cut layer, every pass included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "plan/")]
pub struct LayerPlan {
    pub layer_id: String,
    pub air_assist: bool,
    pub moves: Vec<Move>,
}

/// A planned job: the layers that have something to cut, in cut order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "plan/")]
pub struct Plan {
    /// Where the head is before the first move.
    pub start: Point,
    pub layers: Vec<LayerPlan>,
}

impl Plan {
    /// Every move in order.
    pub fn moves(&self) -> impl Iterator<Item = &Move> {
        self.layers.iter().flat_map(|layer| &layer.moves)
    }

    /// Total distance travelled with the laser off, mm, power-0 cuts
    /// included.
    pub fn travel_length(&self) -> f64 {
        self.lengths().0
    }

    /// Total distance lased, mm.
    pub fn cut_length(&self) -> f64 {
        self.lengths().1
    }

    fn lengths(&self) -> (f64, f64) {
        let mut at = self.start;
        let (mut travel, mut cut) = (0.0, 0.0);
        for step in self.moves() {
            match *step {
                Move::Rapid { to } => {
                    travel += distance(at, to);
                    at = to;
                }
                Move::Cut { to, power, .. } => {
                    if power > 0.0 {
                        cut += distance(at, to);
                    } else {
                        travel += distance(at, to);
                    }
                    at = to;
                }
            }
        }
        (travel, cut)
    }
}

/// Plans the document. Objects on layers the document lacks are skipped;
/// a document that passes [`DesignDocument::validate`] has none.
pub fn plan(document: &DesignDocument, options: &PlanOptions) -> Plan {
    let tolerance = options.tolerance.max(1e-6);
    let mut polylines = outline::polylines_by_layer(&document.objects, tolerance);
    let mut at = options.start;
    let mut layers = Vec::new();
    for layer in document.layers.iter().filter(|layer| layer.output) {
        let Some(paths) = polylines.remove(layer.id.as_str()) else {
            continue;
        };
        let moves = match layer.mode {
            LayerMode::Line => line_moves(layer, &paths, &mut at),
            LayerMode::Fill => fill::fill_moves(layer, &paths, &mut at),
        };
        if !moves.is_empty() {
            layers.push(LayerPlan {
                layer_id: layer.id.clone(),
                air_assist: layer.air_assist,
                moves,
            });
        }
    }
    Plan {
        start: options.start,
        layers,
    }
}

/// A line layer: its outlines in cut order, once per pass.
fn line_moves(layer: &CutLayer, paths: &[Polyline], at: &mut Point) -> Vec<Move> {
    let routes = order::order(paths, *at);
    let mut moves = Vec::new();
    for _ in 0..layer.passes {
        for route in &routes {
            let mut points = route.points(paths);
            let Some(first) = points.next() else {
                continue;
            };
            if distance(*at, first) > SAME_POINT {
                moves.push(Move::Rapid { to: first });
            }
            moves.extend(points.map(|to| Move::Cut {
                to,
                power: layer.max_power,
                speed: layer.speed,
            }));
            if let Some(Move::Cut { to, .. } | Move::Rapid { to }) = moves.last() {
                *at = *to;
            }
        }
    }
    moves
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::project::{SceneObject, Segment, Subpath, Transform};

    const IDENTITY: Transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    fn translate(x: f64, y: f64) -> Transform {
        [1.0, 0.0, 0.0, 1.0, x, y]
    }

    fn rect(id: &str, layer: &str, x: f64, y: f64, size: f64) -> SceneObject {
        SceneObject::Rect {
            id: id.into(),
            name: id.into(),
            layer_id: layer.into(),
            transform: translate(x, y),
            width: size,
            height: size,
            corner_radius: 0.0,
        }
    }

    fn circle(id: &str, layer: &str, x: f64, y: f64, r: f64) -> SceneObject {
        SceneObject::Ellipse {
            id: id.into(),
            name: id.into(),
            layer_id: layer.into(),
            transform: translate(x, y),
            rx: r,
            ry: r,
        }
    }

    fn document(layers: Vec<CutLayer>, objects: Vec<SceneObject>) -> DesignDocument {
        let document = DesignDocument {
            version: 1,
            units: "mm".into(),
            layers,
            objects,
        };
        document.validate().unwrap();
        document
    }

    fn layer(id: &str) -> CutLayer {
        CutLayer::new(id, id, "#0000ff")
    }

    /// Each run of cuts with the width of its bounding box, so tests can tell
    /// shapes apart.
    fn cut_widths(moves: &[Move]) -> Vec<f64> {
        let mut widths = Vec::new();
        let mut run: Vec<f64> = Vec::new();
        let mut last = None;
        let close = |run: &mut Vec<f64>, widths: &mut Vec<f64>| {
            if run.len() > 1 {
                let (min, max) = run
                    .iter()
                    .fold((f64::MAX, f64::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)));
                widths.push(((max - min) * 1000.0).round() / 1000.0);
            }
            run.clear();
        };
        for step in moves {
            match *step {
                Move::Rapid { to } => {
                    close(&mut run, &mut widths);
                    run.push(to.x);
                }
                Move::Cut { to, .. } => {
                    if run.is_empty()
                        && let Some(Point { x, .. }) = last
                    {
                        run.push(x);
                    }
                    run.push(to.x);
                }
            }
            last = Some(match *step {
                Move::Rapid { to } | Move::Cut { to, .. } => to,
            });
        }
        close(&mut run, &mut widths);
        widths
    }

    #[test]
    fn inner_shapes_are_cut_before_the_shapes_around_them() {
        // A frame at the origin (where the head starts) holding a part that
        // has a hole; a loose shape sits beside the frame.
        let doc = document(
            vec![layer("a")],
            vec![
                rect("frame", "a", 0.0, 0.0, 100.0),
                rect("part", "a", 20.0, 20.0, 60.0),
                circle("hole", "a", 50.0, 50.0, 10.0),
                rect("loose", "a", 110.0, 0.0, 5.0),
            ],
        );
        let plan = plan(&doc, &PlanOptions::default());
        let widths = cut_widths(&plan.layers[0].moves);
        let position = |width: f64| widths.iter().position(|&w| w == width).unwrap();
        assert_eq!(widths.len(), 4, "{widths:?}");
        assert!(position(20.0) < position(60.0), "{widths:?}");
        assert!(position(60.0) < position(100.0), "{widths:?}");
    }

    #[test]
    fn open_paths_inside_a_shape_are_cut_first_too() {
        let score = SceneObject::Path {
            id: "score".into(),
            name: "Score".into(),
            layer_id: "a".into(),
            transform: IDENTITY,
            subpaths: vec![Subpath {
                start: Point { x: 30.0, y: 50.0 },
                segments: vec![Segment::Line {
                    to: Point { x: 70.0, y: 50.0 },
                }],
                closed: false,
            }],
        };
        let doc = document(
            vec![layer("a")],
            vec![rect("frame", "a", 0.0, 0.0, 100.0), score],
        );
        let widths = cut_widths(&plan(&doc, &PlanOptions::default()).layers[0].moves);
        assert_eq!(widths, [40.0, 100.0]);
    }

    /// Document-order travel: each shape cut from its first point, in the order drawn.
    fn document_order_travel(doc: &DesignDocument) -> f64 {
        let mut at = Point { x: 0.0, y: 0.0 };
        let mut travel = 0.0;
        for object in &doc.objects {
            let layers =
                outline::polylines_by_layer(std::slice::from_ref(object), DEFAULT_TOLERANCE);
            for path in layers.values().flatten() {
                travel += distance(at, path.points[0]);
                at = if path.closed {
                    path.points[0]
                } else {
                    path.points[path.points.len() - 1]
                };
            }
        }
        travel
    }

    #[test]
    fn travel_is_shorter_than_document_order() {
        // A 6×6 grid of circles and squares, drawn in a scrambled order.
        let objects = (0..36)
            .map(|i| {
                let cell = (i * 23) % 36;
                let (x, y) = (f64::from(cell % 6) * 30.0, f64::from(cell / 6) * 30.0);
                if i % 2 == 0 {
                    circle(&format!("o{i}"), "a", x + 10.0, y + 10.0, 8.0)
                } else {
                    rect(&format!("o{i}"), "a", x, y, 20.0)
                }
            })
            .collect();
        let doc = document(vec![layer("a")], objects);
        let planned = plan(&doc, &PlanOptions::default()).travel_length();
        let naive = document_order_travel(&doc);
        assert!(
            planned < naive * 0.5,
            "planned {planned} mm vs document order {naive} mm"
        );
    }

    #[test]
    fn passes_repeat_the_layer() {
        let objects = vec![
            rect("r", "a", 10.0, 10.0, 20.0),
            circle("c", "a", 60.0, 20.0, 5.0),
        ];
        let once = plan(
            &document(vec![layer("a")], objects.clone()),
            &PlanOptions::default(),
        );
        let mut thrice_layer = layer("a");
        thrice_layer.passes = 3;
        let thrice = plan(
            &document(vec![thrice_layer], objects),
            &PlanOptions::default(),
        );
        let cuts = |plan: &Plan| {
            plan.moves()
                .filter(|m| matches!(m, Move::Cut { .. }))
                .copied()
                .collect::<Vec<_>>()
        };
        let (one, three) = (cuts(&once), cuts(&thrice));
        assert_eq!(three.len(), one.len() * 3);
        assert_eq!(three, [one.clone(), one.clone(), one].concat());
        assert!((thrice.cut_length() - 3.0 * once.cut_length()).abs() < 1e-9);
    }

    #[test]
    fn layers_follow_their_settings_and_order() {
        let mut first = layer("first");
        first.max_power = 80.0;
        first.min_power = 10.0;
        first.speed = 600.0;
        first.air_assist = false;
        let mut off = layer("off");
        off.output = false;
        let mut fill = layer("fill");
        fill.mode = LayerMode::Fill;
        let mut hidden = layer("hidden");
        hidden.shown = false;
        let group = SceneObject::Group {
            id: "g".into(),
            name: "Group".into(),
            layer_id: "off".into(),
            transform: translate(100.0, 0.0),
            children: vec![rect("in-group", "first", 0.0, 0.0, 10.0)],
        };
        let doc = document(
            vec![hidden, first, off, fill, layer("empty")],
            vec![
                rect("h", "hidden", 0.0, 0.0, 10.0),
                rect("o", "off", 0.0, 0.0, 10.0),
                rect("f", "fill", 0.0, 0.0, 10.0),
                group,
            ],
        );
        let plan = plan(&doc, &PlanOptions::default());
        let ids: Vec<_> = plan.layers.iter().map(|l| l.layer_id.as_str()).collect();
        // Hidden layers still cut; output-off and empty layers don't.
        assert_eq!(ids, ["hidden", "first", "fill"]);
        assert!(plan.layers[0].air_assist);
        let first = &plan.layers[1];
        assert!(!first.air_assist);
        for step in &first.moves {
            if let Move::Cut { to, power, speed } = *step {
                assert_eq!((power, speed), (80.0, 600.0));
                // The group's transform moved the rectangle.
                assert!((100.0..=110.0).contains(&to.x), "{to:?}");
            }
        }
    }

    #[test]
    fn the_plan_starts_from_the_given_point_and_serializes_for_the_ui() {
        let doc = document(vec![layer("a")], vec![rect("r", "a", 0.0, 0.0, 10.0)]);
        let options = PlanOptions {
            start: Point { x: 0.0, y: 0.0 },
            ..PlanOptions::default()
        };
        let plan = plan(&doc, &options);
        // Already at the rectangle's corner: no rapid, straight into the cut.
        assert!(matches!(plan.layers[0].moves[0], Move::Cut { .. }));
        assert_eq!(plan.layers[0].moves.len(), 4);
        assert_eq!(plan.travel_length(), 0.0);
        assert!((plan.cut_length() - 40.0).abs() < 1e-9);
        let json = serde_json::to_value(plan.layers[0].moves[0]).unwrap();
        assert_eq!(json["kind"], "cut");
        assert_eq!(json["power"], 50.0);
        assert!(serde_json::to_value(&plan).unwrap()["layers"][0]["layerId"] == "a");
    }

    #[test]
    fn planning_is_deterministic() {
        let objects = (0..50)
            .map(|i| {
                circle(
                    &format!("c{i}"),
                    "a",
                    f64::from(i % 7) * 15.0,
                    f64::from(i / 7) * 15.0,
                    5.0,
                )
            })
            .collect();
        let doc = document(vec![layer("a")], objects);
        assert_eq!(
            plan(&doc, &PlanOptions::default()),
            plan(&doc, &PlanOptions::default())
        );
    }

    /// Like the canvas benchmark (`benchmarkDocument`): zig-zag paths of 100
    /// line segments tiled over a 400 mm bed, across 8 layers.
    fn zig_zags(segments: usize) -> DesignDocument {
        let per_path = 100;
        let paths = segments.div_ceil(per_path);
        let columns = (paths as f64).sqrt().ceil() as usize;
        let cell = 400.0 / columns as f64;
        let layers: Vec<_> = (0..8).map(|i| layer(&format!("layer-{i}"))).collect();
        let objects = (0..paths)
            .map(|i| {
                let x0 = (i % columns) as f64 * cell;
                let y0 = (i / columns) as f64 * cell;
                SceneObject::Path {
                    id: format!("p{i}"),
                    name: format!("Benchmark {i}"),
                    layer_id: layers[i % layers.len()].id.clone(),
                    transform: IDENTITY,
                    subpaths: vec![Subpath {
                        start: Point { x: x0, y: y0 },
                        segments: (0..per_path)
                            .map(|s| Segment::Line {
                                to: Point {
                                    x: x0 + (s + 1) as f64 / per_path as f64 * cell * 0.9,
                                    y: y0 + if s % 2 == 0 { cell * 0.9 } else { 0.0 },
                                },
                            })
                            .collect(),
                        closed: false,
                    }],
                }
            })
            .collect();
        document(layers, objects)
    }

    /// Many small shapes: 2,500 squares (10k segments) with a hole in every
    /// tenth one, and 10k loose one-segment lines.
    fn small_shapes() -> [DesignDocument; 2] {
        let squares = (0..2500)
            .flat_map(|i| {
                let (x, y) = (f64::from(i % 50) * 8.0, f64::from(i / 50) * 8.0);
                let mut shapes = vec![rect(&format!("s{i}"), "a", x, y, 6.0)];
                if i % 10 == 0 {
                    shapes.push(rect(&format!("h{i}"), "a", x + 2.0, y + 2.0, 2.0));
                }
                shapes
            })
            .collect();
        let lines = (0..10_000)
            .map(|i| {
                let (x, y) = (f64::from((i * 37) % 400), f64::from((i * 91) % 400));
                SceneObject::Path {
                    id: format!("l{i}"),
                    name: "Line".into(),
                    layer_id: "a".into(),
                    transform: IDENTITY,
                    subpaths: vec![Subpath {
                        start: Point { x, y },
                        segments: vec![Segment::Line {
                            to: Point {
                                x: x + 0.5,
                                y: y + 0.5,
                            },
                        }],
                        closed: false,
                    }],
                }
            })
            .collect();
        [
            document(vec![layer("a")], squares),
            document(vec![layer("a")], lines),
        ]
    }

    #[test]
    fn ten_thousand_segments_plan_in_under_200_ms() {
        let [squares, lines] = small_shapes();
        for (name, doc) in [
            ("zig-zags", zig_zags(10_000)),
            ("squares", squares),
            ("lines", lines),
        ] {
            // Best of three, so a busy machine doesn't fail the test.
            let mut best = Duration::MAX;
            let mut planned = None;
            for _ in 0..3 {
                let started = Instant::now();
                planned = Some(plan(&doc, &PlanOptions::default()));
                best = best.min(started.elapsed());
            }
            let cuts = planned
                .unwrap()
                .moves()
                .filter(|m| matches!(m, Move::Cut { .. }))
                .count();
            assert!(cuts >= 10_000, "{name}: {cuts} cuts");
            // Debug builds are many times slower; the budget is for release
            // (`cargo test -p bf-core --release plan`).
            if !cfg!(debug_assertions) {
                assert!(best < Duration::from_millis(200), "{name}: {best:?}");
            }
        }
    }
}
