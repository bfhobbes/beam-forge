//! Fill layers: the layer's closed shapes hatched with parallel scan lines.
//!
//! All the layer's closed shapes are filled together by the even-odd rule,
//! so a shape inside another (the counter of an O, the hole in a ring) stays
//! empty. Open paths aren't filled. Scan lines run at the layer's scan angle,
//! `line_interval` apart, and rows with nothing to burn are skipped.
//!
//! Each burn along a line starts and ends `overscan` beyond the shape with
//! the laser off (a cut at power 0, so at cutting speed), so the head is up
//! to speed before it burns. A gap along a line is crossed the same way when
//! it is shorter than the layer's fill gap, or than the two overscans a
//! separate rapid would need: one steady move costs less than slowing down,
//! jumping and speeding up again.

use super::Move;
use super::outline::{Polyline, SAME_POINT, distance};
use crate::project::{CutLayer, Point};

/// Burns shorter than this, mm, are left out: they are where a scan line
/// grazes a vertex.
const MIN_SPAN: f64 = 1e-6;

/// The moves that fill `paths` with `layer`'s settings, every pass included,
/// from where the head is (`at`, which is left where the moves end).
pub(super) fn fill_moves(layer: &CutLayer, paths: &[Polyline], at: &mut Point) -> Vec<Move> {
    let frame = ScanFrame::new(layer.scan_angle);
    let mut rows = rows(paths, layer.line_interval, &frame);
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Vec::new();
    };
    // Start at whichever end row is nearer the head, from its nearer end.
    let head = frame.to_scan(*at);
    if (head.y - last.v).abs() < (head.y - first.v).abs() {
        rows.reverse();
    }
    let mut forward = !layer.bidirectional || {
        let spans = &rows[0].spans;
        (head.x - spans[0].0).abs() <= (head.x - spans[spans.len() - 1].1).abs()
    };
    let mut out = Out {
        frame,
        moves: Vec::new(),
        at: *at,
        power: layer.max_power,
        speed: layer.speed,
        overscan: layer.overscan,
        bridge: layer.fill_gap.max(2.0 * layer.overscan),
    };
    for pass in 0..layer.passes {
        // Each pass sweeps back the way the last one came.
        let order: Box<dyn Iterator<Item = &Row>> = if pass % 2 == 0 {
            Box::new(rows.iter())
        } else {
            Box::new(rows.iter().rev())
        };
        for row in order {
            out.row(row, forward);
            if layer.bidirectional {
                forward = !forward;
            }
        }
    }
    *at = out.at;
    out.moves
}

/// Coordinates with the scan lines along x: world points turned by minus the
/// scan angle.
#[derive(Debug, Clone, Copy)]
struct ScanFrame {
    sin: f64,
    cos: f64,
}

impl ScanFrame {
    fn new(degrees: f64) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        ScanFrame { sin, cos }
    }

    fn to_scan(self, p: Point) -> Point {
        Point {
            x: p.x * self.cos + p.y * self.sin,
            y: -p.x * self.sin + p.y * self.cos,
        }
    }

    fn to_world(self, x: f64, y: f64) -> Point {
        Point {
            x: x * self.cos - y * self.sin,
            y: x * self.sin + y * self.cos,
        }
    }
}

/// One scan line with something to burn, in scan coordinates.
#[derive(Debug)]
struct Row {
    /// Where the line is across the scan direction.
    v: f64,
    /// What to burn, `(from, to)` along the line, left to right, not touching.
    spans: Vec<(f64, f64)>,
}

/// Row `k` is centred in its band of the grid: `(k + 0.5) * interval`.
fn row_v(k: i64, interval: f64) -> f64 {
    (k as f64 + 0.5) * interval
}

/// The rows that cross the closed `paths`, bottom to top in scan
/// coordinates. Every edge holds the rows in `[low end, high end)`, so a
/// line through a vertex counts it once where the outline passes through
/// and zero or two times where it turns back: pairing crossings left to
/// right is then the even-odd rule.
fn rows(paths: &[Polyline], interval: f64, frame: &ScanFrame) -> Vec<Row> {
    let mut hits: Vec<(i64, f64)> = Vec::new();
    let mut points = Vec::new();
    for path in paths.iter().filter(|p| p.closed && p.points.len() > 2) {
        points.clear();
        points.extend(path.points.iter().map(|&p| frame.to_scan(p)));
        let mut previous = points[points.len() - 1];
        for &p in &points {
            let (low, high) = if previous.y < p.y {
                (previous, p)
            } else {
                (p, previous)
            };
            previous = p;
            if low.y == high.y {
                continue;
            }
            // One row early, in case rounding put the first row past `low`.
            let mut k = (low.y / interval - 0.5).ceil() as i64 - 1;
            loop {
                let v = row_v(k, interval);
                if v >= high.y {
                    break;
                }
                if v >= low.y {
                    let x = low.x + (v - low.y) / (high.y - low.y) * (high.x - low.x);
                    hits.push((k, x));
                }
                k += 1;
            }
        }
    }
    hits.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    hits.chunk_by(|a, b| a.0 == b.0)
        .filter_map(|row| {
            let spans: Vec<_> = row
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[from, to]| (from.1, to.1))
                .filter(|(from, to)| to - from > MIN_SPAN)
                .collect();
            (!spans.is_empty()).then(|| Row {
                v: row_v(row[0].0, interval),
                spans,
            })
        })
        .collect()
}

/// Writes moves, tracking where the head is.
struct Out {
    frame: ScanFrame,
    moves: Vec<Move>,
    at: Point,
    power: f64,
    speed: f64,
    overscan: f64,
    /// Gaps along a line up to this long are crossed with the laser off.
    bridge: f64,
}

impl Out {
    /// One row, left to right (`forward`) or right to left.
    fn row(&mut self, row: &Row, forward: bool) {
        let dir = if forward { 1.0 } else { -1.0 };
        let spans: Vec<(f64, f64)> = if forward {
            row.spans.clone()
        } else {
            row.spans.iter().rev().map(|&(a, b)| (b, a)).collect()
        };
        let v = row.v;
        let mut i = 0;
        while i < spans.len() {
            let start = spans[i].0;
            self.travel(start - dir * self.overscan, v);
            if self.overscan > 0.0 {
                self.cut(start, v, 0.0);
            }
            let end = loop {
                let end = spans[i].1;
                self.cut(end, v, self.power);
                match spans.get(i + 1) {
                    Some(&(next, _)) if (next - end) * dir <= self.bridge => {
                        self.cut(next, v, 0.0);
                        i += 1;
                    }
                    _ => break end,
                }
            };
            if self.overscan > 0.0 {
                self.cut(end + dir * self.overscan, v, 0.0);
            }
            i += 1;
        }
    }

    fn travel(&mut self, x: f64, y: f64) {
        let to = self.frame.to_world(x, y);
        if distance(self.at, to) > SAME_POINT {
            self.moves.push(Move::Rapid { to });
            self.at = to;
        }
    }

    fn cut(&mut self, x: f64, y: f64, power: f64) {
        let to = self.frame.to_world(x, y);
        if distance(self.at, to) > SAME_POINT {
            self.moves.push(Move::Cut {
                to,
                power,
                speed: self.speed,
            });
            self.at = to;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::import::svg::import_svg;
    use crate::import::{ImportedPoint, ImportedSegment};
    use crate::plan::outline::polylines_by_layer;
    use crate::project::{SceneObject, Segment, Subpath};

    const TOLERANCE: f64 = 0.01;

    fn layer() -> CutLayer {
        let mut layer = CutLayer::new("a", "a", "#000000");
        layer.mode = crate::project::LayerMode::Fill;
        layer
    }

    fn translate(x: f64, y: f64) -> [f64; 6] {
        [1.0, 0.0, 0.0, 1.0, x, y]
    }

    fn circle(x: f64, y: f64, r: f64) -> SceneObject {
        SceneObject::Ellipse {
            id: "c".into(),
            name: "c".into(),
            layer_id: "a".into(),
            transform: translate(x, y),
            rx: r,
            ry: r,
        }
    }

    fn square(x: f64, y: f64, size: f64) -> SceneObject {
        SceneObject::Rect {
            id: "r".into(),
            name: "r".into(),
            layer_id: "a".into(),
            transform: translate(x, y),
            width: size,
            height: size,
            corner_radius: 0.0,
        }
    }

    fn polylines(objects: &[SceneObject]) -> Vec<Polyline> {
        polylines_by_layer(objects, TOLERANCE)
            .remove("a")
            .unwrap_or_default()
    }

    /// `fill-text.svg`: four lines of Geist glyph outlines in a 50 mm
    /// square, counters and all.
    fn text() -> Vec<Polyline> {
        polylines(&text_objects())
    }

    fn text_objects() -> Vec<SceneObject> {
        let svg = include_bytes!("../../tests/fixtures/svg/fill-text.svg");
        let point = |p: ImportedPoint| Point { x: p.x, y: p.y };
        import_svg(svg)
            .unwrap()
            .paths
            .iter()
            .enumerate()
            .map(|(i, path)| SceneObject::Path {
                id: format!("t{i}"),
                name: "Text".into(),
                layer_id: "a".into(),
                transform: translate(0.0, 0.0),
                subpaths: path
                    .subpaths
                    .iter()
                    .map(|subpath| Subpath {
                        start: point(subpath.start),
                        segments: subpath
                            .segments
                            .iter()
                            .map(|segment| match *segment {
                                ImportedSegment::Line { to } => Segment::Line { to: point(to) },
                                ImportedSegment::Quad { control, to } => Segment::Quad {
                                    control: point(control),
                                    to: point(to),
                                },
                                ImportedSegment::Cubic {
                                    control1,
                                    control2,
                                    to,
                                } => Segment::Cubic {
                                    control1: point(control1),
                                    control2: point(control2),
                                    to: point(to),
                                },
                            })
                            .collect(),
                        closed: subpath.closed,
                    })
                    .collect(),
            })
            .collect()
    }

    /// A straight move: where it starts and ends, and its power (`None` for a
    /// rapid).
    #[derive(Debug, Clone, Copy)]
    struct Stroke {
        from: Point,
        to: Point,
        power: Option<f64>,
    }

    impl Stroke {
        fn burns(&self) -> bool {
            self.power.is_some_and(|power| power > 0.0)
        }

        fn length(&self) -> f64 {
            distance(self.from, self.to)
        }

        /// Points spread along the stroke, ends excluded.
        fn samples(&self, n: usize) -> impl Iterator<Item = Point> + '_ {
            (1..n).map(move |i| {
                let t = i as f64 / n as f64;
                Point {
                    x: self.from.x + t * (self.to.x - self.from.x),
                    y: self.from.y + t * (self.to.y - self.from.y),
                }
            })
        }
    }

    fn strokes(start: Point, moves: &[Move]) -> Vec<Stroke> {
        let mut at = start;
        moves
            .iter()
            .map(|step| {
                let (to, power) = match *step {
                    Move::Rapid { to } => (to, None),
                    Move::Cut { to, power, .. } => (to, Some(power)),
                };
                let stroke = Stroke {
                    from: at,
                    to,
                    power,
                };
                at = to;
                stroke
            })
            .collect()
    }

    fn fill(layer: &CutLayer, paths: &[Polyline]) -> Vec<Stroke> {
        let start = Point { x: 0.0, y: 0.0 };
        let mut at = start;
        let moves = fill_moves(layer, paths, &mut at);
        strokes(start, &moves)
    }

    /// Even-odd: inside when a ray to the right crosses an odd number of edges.
    fn inside(paths: &[Polyline], p: Point) -> bool {
        let mut crossings = 0;
        for path in paths.iter().filter(|path| path.closed) {
            let n = path.points.len();
            for i in 0..n {
                let (a, b) = (path.points[i], path.points[(i + 1) % n]);
                if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x)
                {
                    crossings += 1;
                }
            }
        }
        crossings % 2 == 1
    }

    /// Area by the even-odd rule, for outlines that never cross: holes wind
    /// the other way, so it is the signed areas' sum.
    fn area(paths: &[Polyline]) -> f64 {
        paths
            .iter()
            .map(|path| {
                let n = path.points.len();
                (0..n)
                    .map(|i| {
                        let (a, b) = (path.points[i], path.points[(i + 1) % n]);
                        a.x * b.y - b.x * a.y
                    })
                    .sum::<f64>()
                    / 2.0
            })
            .sum::<f64>()
            .abs()
    }

    fn burned(strokes: &[Stroke]) -> f64 {
        strokes
            .iter()
            .filter(|s| s.burns())
            .map(Stroke::length)
            .sum()
    }

    #[test]
    fn a_ring_is_filled_and_its_hole_stays_empty() {
        let ring = polylines(&[circle(30.0, 30.0, 10.0), circle(30.0, 30.0, 5.0)]);
        let strokes = fill(&layer(), &ring);
        let centre = Point { x: 30.0, y: 30.0 };
        for stroke in strokes.iter().filter(|s| s.burns()) {
            for p in stroke.samples(20).chain([stroke.from, stroke.to]) {
                let r = distance(p, centre);
                // The flattened circles are within the tolerance of true ones.
                assert!(
                    (5.0 - TOLERANCE..=10.0 + TOLERANCE).contains(&r),
                    "burns at r = {r}: {stroke:?}"
                );
            }
        }
        // Burned length times the interval is the area: every row is filled.
        let expected = PI * (100.0 - 25.0) / 0.1;
        let burned = burned(&strokes);
        assert!(
            (burned - expected).abs() < expected * 0.01,
            "{burned} vs {expected}"
        );
        // Every burn is preceded and followed by its overscan.
        for (i, stroke) in strokes.iter().enumerate() {
            if stroke.burns() && !strokes[i - 1].burns() {
                assert_eq!(strokes[i - 1].power, Some(0.0));
            }
        }
    }

    #[test]
    fn text_is_filled_inside_its_outlines_only() {
        let text = text();
        assert!(text.len() > 30, "{} outlines", text.len());
        let strokes = fill(&layer(), &text);
        for stroke in strokes.iter().filter(|s| s.burns()) {
            for p in stroke.samples(8) {
                assert!(inside(&text, p), "burns outside the glyphs at {p:?}");
            }
        }
        // And nothing inside is missed: rows are filled edge to edge.
        let expected = area(&text) / 0.1;
        let burned = burned(&strokes);
        assert!(
            (burned - expected).abs() < expected * 0.02,
            "{burned} vs {expected}"
        );
    }

    #[test]
    fn a_45_degree_fill_runs_along_its_angle_one_interval_apart() {
        let mut layer = layer();
        layer.scan_angle = 45.0;
        layer.line_interval = 0.5;
        let square = polylines(&[square(10.0, 10.0, 20.0)]);
        let strokes = fill(&layer, &square);
        let along = 45f64.to_radians();
        let mut rows = Vec::new();
        for stroke in strokes.iter().filter(|s| s.power.is_some()) {
            // Burns and overscans alike run at 45° one way or the other.
            let (dx, dy) = (stroke.to.x - stroke.from.x, stroke.to.y - stroke.from.y);
            let cross = dx * along.sin() - dy * along.cos();
            assert!(cross.abs() < 1e-9, "{stroke:?}");
            if stroke.burns() {
                for p in stroke.samples(4) {
                    assert!(inside(&square, p), "{p:?}");
                }
                // Distance across the scan direction identifies the row.
                rows.push(stroke.from.y * along.cos() - stroke.from.x * along.sin());
            }
        }
        rows.sort_by(f64::total_cmp);
        rows.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        for pair in rows.windows(2) {
            assert!((pair[1] - pair[0] - 0.5).abs() < 1e-9, "{pair:?}");
        }
        // The square's diagonal is 20√2 mm across the scan lines.
        let across = 20.0 * 2f64.sqrt() / 0.5;
        assert!((rows.len() as f64 - across).abs() < 1.0, "{}", rows.len());
        let burned = burned(&strokes);
        assert!(
            (burned - 400.0 / 0.5).abs() < 400.0 / 0.5 * 0.01,
            "{burned}"
        );
    }

    /// The scan direction (+1 or -1 along x) of each row's burns.
    fn directions(strokes: &[Stroke]) -> Vec<f64> {
        let mut directions: Vec<(f64, f64)> = Vec::new();
        for stroke in strokes.iter().filter(|s| s.burns()) {
            let dir = (stroke.to.x - stroke.from.x).signum();
            match directions.last() {
                Some(&(y, _)) if (y - stroke.from.y).abs() < 1e-9 => {}
                _ => directions.push((stroke.from.y, dir)),
            }
        }
        directions.into_iter().map(|(_, dir)| dir).collect()
    }

    #[test]
    fn bidirectional_fills_alternate_and_unidirectional_ones_do_not() {
        let square = polylines(&[square(0.0, 0.0, 5.0)]);
        let both = directions(&fill(&layer(), &square));
        assert_eq!(both.len(), 50);
        assert!(both.windows(2).all(|pair| pair[0] == -pair[1]), "{both:?}");
        let mut one_way = layer();
        one_way.bidirectional = false;
        let strokes = fill(&one_way, &square);
        assert!(directions(&strokes).iter().all(|&dir| dir == 1.0));
        // Every row starts with a rapid back to its overscan.
        assert_eq!(strokes.iter().filter(|s| s.power.is_none()).count(), 50);
    }

    /// Rapids that stay on one scan row: jumps across a gap.
    fn jumps(strokes: &[Stroke]) -> usize {
        strokes
            .iter()
            .filter(|s| s.power.is_none() && (s.from.y - s.to.y).abs() < 1e-9)
            .count()
    }

    #[test]
    fn short_gaps_are_crossed_with_the_laser_off_and_long_ones_jumped() {
        let pair = |gap: f64| polylines(&[square(0.0, 0.0, 5.0), square(5.0 + gap, 0.0, 5.0)]);
        let mut layer = layer();
        layer.overscan = 0.0;
        // Under the fill gap: one steady move per row.
        let strokes = fill(&layer, &pair(0.8));
        assert_eq!(jumps(&strokes), 0);
        let crossings = strokes
            .iter()
            .filter(|s| s.power == Some(0.0) && (s.length() - 0.8).abs() < 1e-9)
            .count();
        assert_eq!(crossings, 50);
        // Over it: a rapid across each row's gap.
        assert_eq!(jumps(&fill(&layer, &pair(3.0))), 50);
        // A wider fill gap crosses it again...
        layer.fill_gap = 4.0;
        assert_eq!(jumps(&fill(&layer, &pair(3.0))), 0);
        // ...and so does overscan, when the gap is shorter than two of them.
        layer.fill_gap = 1.0;
        layer.overscan = 2.0;
        assert_eq!(jumps(&fill(&layer, &pair(3.0))), 0);
        assert_eq!(jumps(&fill(&layer, &pair(5.0))), 50);
    }

    #[test]
    fn empty_rows_are_skipped() {
        let stacked = polylines(&[square(0.0, 0.0, 5.0), square(0.0, 15.0, 5.0)]);
        let strokes = fill(&layer(), &stacked);
        // Nothing stops between the squares: one rapid straight across.
        for stroke in &strokes {
            assert!(!(5.0..15.0).contains(&stroke.to.y), "{stroke:?}");
        }
        let across = strokes
            .iter()
            .filter(|s| s.from.y < 5.0 && s.to.y > 15.0)
            .collect::<Vec<_>>();
        assert_eq!(across.len(), 1);
        assert!(across[0].power.is_none());
    }

    #[test]
    fn open_paths_and_empty_layers_fill_nothing() {
        let open = Polyline {
            points: vec![Point { x: 0.0, y: 0.0 }, Point { x: 10.0, y: 10.0 }],
            closed: false,
        };
        let mut at = Point { x: 1.0, y: 2.0 };
        assert!(fill_moves(&layer(), &[open], &mut at).is_empty());
        assert!(fill_moves(&layer(), &[], &mut at).is_empty());
        assert_eq!(at, Point { x: 1.0, y: 2.0 });
    }

    #[test]
    fn filling_starts_at_the_corner_nearest_the_head() {
        let square = polylines(&[square(0.0, 0.0, 10.0)]);
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0)] {
            let mut at = Point { x, y };
            let moves = fill_moves(&layer(), &square, &mut at);
            let Move::Rapid { to } = moves[0] else {
                panic!("starts with a rapid");
            };
            // The overscan is 2.5 mm out; the first row is half an interval in.
            assert!((to.x - x).abs() <= 2.5 + 1e-9, "{to:?} from {x}, {y}");
            assert!((to.y - y).abs() <= 0.05 + 1e-9, "{to:?} from {x}, {y}");
        }
    }

    #[test]
    fn filled_50_mm_text_plans_in_under_500_ms() {
        let doc = crate::project::DesignDocument {
            version: 1,
            units: "mm".into(),
            layers: vec![layer()],
            objects: text_objects(),
        };
        doc.validate().unwrap();
        // Best of three, so a busy machine doesn't fail the test.
        let mut best = Duration::MAX;
        let mut planned = None;
        for _ in 0..3 {
            let started = Instant::now();
            planned = Some(crate::plan::plan(
                &doc,
                &crate::plan::PlanOptions::default(),
            ));
            best = best.min(started.elapsed());
        }
        let burns = planned
            .unwrap()
            .moves()
            .filter(|m| matches!(m, Move::Cut { power, .. } if *power > 0.0))
            .count();
        assert!(burns > 1000, "{burns} burns");
        // Debug builds are many times slower; the budget is for release
        // (`cargo test -p bf-core --release fill`).
        if !cfg!(debug_assertions) {
            assert!(best < Duration::from_millis(500), "{best:?}");
        }
    }

    #[test]
    fn passes_sweep_back_and_forth() {
        let square = polylines(&[square(0.0, 0.0, 5.0)]);
        let mut twice = layer();
        twice.passes = 2;
        let once = burned(&fill(&layer(), &square));
        let strokes = fill(&twice, &square);
        assert!((burned(&strokes) - 2.0 * once).abs() < 1e-6);
        // The second pass starts where the first ended: no long rapid back.
        let longest = strokes
            .iter()
            .filter(|s| s.power.is_none())
            .map(Stroke::length)
            .skip(1)
            .fold(0.0, f64::max);
        assert!(longest < 1.0, "{longest}");
    }
}
