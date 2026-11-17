//! Object outlines in world millimetres: each leaf object's shape, its own
//! and its groups' transforms applied, flattened to polylines.
//!
//! The shapes match the canvas (`src/lib/canvas/scene.ts`), so what is cut is
//! what is drawn.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::project::{Point, SceneObject, Segment, Subpath, Transform};

/// A flattened subpath. A closed one does not repeat its first point at the
/// end; cutting it returns there.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Polyline {
    pub points: Vec<Point>,
    pub closed: bool,
}

const IDENTITY: Transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Kappa: control-point distance for a quarter circle drawn as one cubic.
const KAPPA: f64 = 0.552_284_749_8;

/// Most line pieces one curve is split into, whatever the tolerance.
const MAX_CURVE_STEPS: f64 = 10_000.0;

/// Points closer than this (mm) are the same point.
pub(super) const SAME_POINT: f64 = 1e-9;

/// Every leaf object's polylines, grouped by layer id, in document order.
/// A group's children keep their own layers.
pub(super) fn polylines_by_layer(
    objects: &[SceneObject],
    tolerance: f64,
) -> HashMap<&str, Vec<Polyline>> {
    let mut layers = HashMap::new();
    collect(objects, &IDENTITY, tolerance, &mut layers);
    layers
}

fn collect<'a>(
    objects: &'a [SceneObject],
    parent: &Transform,
    tolerance: f64,
    layers: &mut HashMap<&'a str, Vec<Polyline>>,
) {
    for object in objects {
        let (transform, subpaths) = match object {
            SceneObject::Group {
                transform,
                children,
                ..
            } => {
                collect(children, &multiply(parent, transform), tolerance, layers);
                continue;
            }
            SceneObject::Path {
                transform,
                subpaths,
                ..
            } => (transform, Cow::Borrowed(subpaths.as_slice())),
            SceneObject::Text {
                transform, outline, ..
            } => (transform, Cow::Borrowed(outline.as_slice())),
            SceneObject::Rect {
                transform,
                width,
                height,
                corner_radius,
                ..
            } => (
                transform,
                Cow::Owned(vec![rect(*width, *height, *corner_radius)]),
            ),
            SceneObject::Ellipse {
                transform, rx, ry, ..
            } => (transform, Cow::Owned(vec![ellipse(*rx, *ry)])),
            SceneObject::Polygon {
                transform,
                sides,
                radius,
                inner_radius,
                ..
            } => (
                transform,
                Cow::Owned(vec![polygon(*sides, *radius, *inner_radius)]),
            ),
        };
        let transform = multiply(parent, transform);
        let out = layers.entry(object.layer_id()).or_default();
        out.extend(
            subpaths
                .iter()
                .filter_map(|subpath| flatten(subpath, &transform, tolerance)),
        );
    }
}

/// The transform that applies `inner` first, then `outer`.
fn multiply(outer: &Transform, inner: &Transform) -> Transform {
    let [a1, b1, c1, d1, e1, f1] = *outer;
    let [a2, b2, c2, d2, e2, f2] = *inner;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

fn apply([a, b, c, d, e, f]: &Transform, Point { x, y }: Point) -> Point {
    Point {
        x: a * x + c * y + e,
        y: b * x + d * y + f,
    }
}

fn point(x: f64, y: f64) -> Point {
    Point { x, y }
}

fn line(x: f64, y: f64) -> Segment {
    Segment::Line { to: point(x, y) }
}

/// Origin at the bottom-left corner; rounded corners are quarter-circle cubics.
fn rect(width: f64, height: f64, corner_radius: f64) -> Subpath {
    let r = corner_radius.min(width / 2.0).min(height / 2.0).max(0.0);
    if r == 0.0 {
        return Subpath {
            start: point(0.0, 0.0),
            segments: vec![line(width, 0.0), line(width, height), line(0.0, height)],
            closed: true,
        };
    }
    let k = r * KAPPA;
    let corner = |c1: (f64, f64), to: (f64, f64), c2: (f64, f64)| Segment::Cubic {
        control1: point(c1.0, c1.1),
        control2: point(c2.0, c2.1),
        to: point(to.0, to.1),
    };
    Subpath {
        start: point(r, 0.0),
        segments: vec![
            line(width - r, 0.0),
            corner((width - r + k, 0.0), (width, r), (width, r - k)),
            line(width, height - r),
            corner(
                (width, height - r + k),
                (width - r, height),
                (width - r + k, height),
            ),
            line(r, height),
            corner((r - k, height), (0.0, height - r), (0.0, height - r + k)),
            line(0.0, r),
            corner((0.0, r - k), (r, 0.0), (r - k, 0.0)),
        ],
        closed: true,
    }
}

/// Centred on the origin, as four cubics.
fn ellipse(rx: f64, ry: f64) -> Subpath {
    let (kx, ky) = (rx * KAPPA, ry * KAPPA);
    let quarter = |c1: (f64, f64), c2: (f64, f64), to: (f64, f64)| Segment::Cubic {
        control1: point(c1.0, c1.1),
        control2: point(c2.0, c2.1),
        to: point(to.0, to.1),
    };
    Subpath {
        start: point(rx, 0.0),
        segments: vec![
            quarter((rx, ky), (kx, ry), (0.0, ry)),
            quarter((-kx, ry), (-rx, ky), (-rx, 0.0)),
            quarter((-rx, -ky), (-kx, -ry), (0.0, -ry)),
            quarter((kx, -ry), (rx, -ky), (rx, 0.0)),
        ],
        closed: true,
    }
}

/// First vertex points straight up; a star alternates outer and inner radii.
fn polygon(sides: u32, radius: f64, inner_radius: Option<f64>) -> Subpath {
    let count = if inner_radius.is_some() {
        sides * 2
    } else {
        sides
    };
    let vertex = |i: u32| {
        let r = match inner_radius {
            Some(inner) if i % 2 == 1 => inner,
            _ => radius,
        };
        let angle =
            std::f64::consts::FRAC_PI_2 + f64::from(i) * std::f64::consts::TAU / f64::from(count);
        point(r * angle.cos(), r * angle.sin())
    };
    Subpath {
        start: vertex(0),
        segments: (1..count)
            .map(|i| Segment::Line { to: vertex(i) })
            .collect(),
        closed: true,
    }
}

/// The subpath in world coordinates, curves split so no piece strays more
/// than `tolerance` mm from the curve. `None` when there is nothing to cut.
fn flatten(subpath: &Subpath, transform: &Transform, tolerance: f64) -> Option<Polyline> {
    let at = |p: Point| apply(transform, p);
    let mut points = Vec::with_capacity(subpath.segments.len() + 1);
    let mut push = |p: Point| {
        if points
            .last()
            .is_none_or(|last: &Point| distance(*last, p) > SAME_POINT)
        {
            points.push(p);
        }
    };
    let mut from = at(subpath.start);
    push(from);
    for segment in &subpath.segments {
        // Transforming control points is exact: affine maps keep Béziers
        // Béziers. Flattening afterwards keeps the tolerance in world mm.
        let to = match *segment {
            Segment::Line { to } => {
                let to = at(to);
                push(to);
                to
            }
            Segment::Quad { control, to } => {
                let (c, to) = (at(control), at(to));
                // Uniform steps stray at most |B''| / 8n², and |B''| = 2|P0 − 2P1 + P2|.
                let n = steps(length(second_difference(from, c, to)) / 4.0, tolerance);
                for step in 1..=n {
                    let t = step as f64 / n as f64;
                    let u = 1.0 - t;
                    let (a, b, d) = (u * u, 2.0 * u * t, t * t);
                    push(point(
                        a * from.x + b * c.x + d * to.x,
                        a * from.y + b * c.y + d * to.y,
                    ));
                }
                to
            }
            Segment::Cubic {
                control1,
                control2,
                to,
            } => {
                let (c1, c2, to) = (at(control1), at(control2), at(to));
                // |B''| ≤ 6·max|second differences|, so the bound is 3/4 of that over n².
                let bend = length(second_difference(from, c1, c2))
                    .max(length(second_difference(c1, c2, to)));
                let n = steps(bend * 0.75, tolerance);
                for step in 1..=n {
                    let t = step as f64 / n as f64;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    push(point(
                        a * from.x + b * c1.x + c * c2.x + d * to.x,
                        a * from.y + b * c1.y + c * c2.y + d * to.y,
                    ));
                }
                to
            }
        };
        from = to;
    }
    if subpath.closed
        && points.len() > 1
        && distance(points[0], points[points.len() - 1]) <= SAME_POINT
    {
        points.pop();
    }
    (points.len() > 1).then_some(Polyline {
        points,
        closed: subpath.closed,
    })
}

/// Uniform steps so that `bound / n²` is within `tolerance`.
fn steps(bound: f64, tolerance: f64) -> usize {
    (bound / tolerance)
        .sqrt()
        .ceil()
        .clamp(1.0, MAX_CURVE_STEPS) as usize
}

fn second_difference(a: Point, b: Point, c: Point) -> Point {
    point(a.x - 2.0 * b.x + c.x, a.y - 2.0 * b.y + c.y)
}

fn length(p: Point) -> f64 {
    p.x.hypot(p.y)
}

pub(super) fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_object(transform: Transform, corner_radius: f64) -> SceneObject {
        SceneObject::Rect {
            id: "r".into(),
            name: "Rectangle".into(),
            layer_id: "a".into(),
            transform,
            width: 20.0,
            height: 10.0,
            corner_radius,
        }
    }

    #[test]
    fn group_and_object_transforms_both_apply() {
        let group = SceneObject::Group {
            id: "g".into(),
            name: "Group".into(),
            layer_id: "ignored".into(),
            // Rotate a quarter turn anticlockwise, then move right 100 mm.
            transform: [0.0, 1.0, -1.0, 0.0, 100.0, 0.0],
            children: vec![rect_object([1.0, 0.0, 0.0, 1.0, 5.0, 0.0], 0.0)],
        };
        let layers = polylines_by_layer(std::slice::from_ref(&group), 0.01);
        assert!(!layers.contains_key("ignored"));
        let points: Vec<_> = layers["a"][0]
            .points
            .iter()
            .map(|p| (p.x.round(), p.y.round()))
            .collect();
        // Object space (5,0) (25,0) (25,10) (5,10), rotated, then moved.
        assert_eq!(
            points,
            [(100.0, 5.0), (100.0, 25.0), (90.0, 25.0), (90.0, 5.0)]
        );
        assert!(layers["a"][0].closed);
    }

    #[test]
    fn curves_flatten_within_the_tolerance() {
        for tolerance in [0.01, 0.1] {
            let circle = SceneObject::Ellipse {
                id: "e".into(),
                name: "Ellipse".into(),
                layer_id: "a".into(),
                transform: IDENTITY,
                rx: 50.0,
                ry: 50.0,
            };
            let layers = polylines_by_layer(std::slice::from_ref(&circle), tolerance);
            let points = &layers["a"][0].points;
            // Every chord's midpoint is within the tolerance of the (near-)circle:
            // the Kappa cubic itself is off a true circle by about 0.027%.
            for (i, a) in points.iter().enumerate() {
                let b = points[(i + 1) % points.len()];
                let mid = point((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                let off = 50.0 - length(mid);
                assert!(off < tolerance + 50.0 * 0.0003, "{off} at {tolerance}");
            }
            // Finer tolerance, more points; but not absurdly many.
            assert!(points.len() < 2000, "{}", points.len());
        }
        let rounded = [rect_object(IDENTITY, 3.0)];
        let coarse = polylines_by_layer(&rounded, 0.1);
        let fine = polylines_by_layer(&rounded, 0.001);
        assert!(fine["a"][0].points.len() > coarse["a"][0].points.len());
    }

    #[test]
    fn closed_paths_drop_a_repeated_end_point_and_empty_paths_vanish() {
        let path = SceneObject::Path {
            id: "p".into(),
            name: "Path".into(),
            layer_id: "a".into(),
            transform: IDENTITY,
            subpaths: vec![
                Subpath {
                    start: point(0.0, 0.0),
                    segments: vec![line(1.0, 0.0), line(1.0, 1.0), line(0.0, 0.0)],
                    closed: true,
                },
                Subpath {
                    start: point(5.0, 5.0),
                    segments: vec![line(5.0, 5.0)],
                    closed: false,
                },
            ],
        };
        let layers = polylines_by_layer(std::slice::from_ref(&path), 0.01);
        assert_eq!(layers["a"].len(), 1);
        assert_eq!(layers["a"][0].points.len(), 3);
    }

    #[test]
    fn text_cuts_its_stored_outline() {
        let text = SceneObject::Text {
            id: "t".into(),
            name: "Text".into(),
            layer_id: "a".into(),
            transform: [1.0, 0.0, 0.0, 1.0, 10.0, 0.0],
            text: "I".into(),
            font_family: "Not installed anywhere".into(),
            font_weight: 400,
            italic: false,
            height: 10.0,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align: crate::text::TextAlign::Left,
            outline: vec![Subpath {
                start: point(0.0, 0.0),
                segments: vec![line(1.0, 0.0), line(1.0, 10.0), line(0.0, 10.0)],
                closed: true,
            }],
        };
        let layers = polylines_by_layer(std::slice::from_ref(&text), 0.01);
        let points: Vec<_> = layers["a"][0].points.iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(
            points,
            [(10.0, 0.0), (11.0, 0.0), (11.0, 10.0), (10.0, 10.0)]
        );
    }

    #[test]
    fn stars_alternate_radii_from_the_top() {
        let star = polygon(5, 10.0, Some(4.0));
        assert_eq!(star.segments.len(), 9);
        assert!((star.start.y - 10.0).abs() < 1e-12 && star.start.x.abs() < 1e-12);
        let Segment::Line { to } = star.segments[0] else {
            panic!("polygons are straight lines");
        };
        assert!((length(to) - 4.0).abs() < 1e-12);
    }
}
