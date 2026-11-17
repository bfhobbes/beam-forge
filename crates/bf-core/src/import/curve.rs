//! Geometry shared by the importers: affine transforms, arcs as cubic
//! Béziers, reversing subpaths and tight bounds.

use std::f64::consts::{FRAC_PI_2, TAU};

use super::{ImportedPoint, ImportedSegment, ImportedSubpath};

pub(crate) type Point = ImportedPoint;

pub(crate) fn pt(x: f64, y: f64) -> Point {
    Point { x, y }
}

/// `x' = a x + c y + e`, `y' = b x + d y + f`, as in SVG's `matrix()`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(x: f64, y: f64) -> Affine {
        Affine {
            e: x,
            f: y,
            ..Affine::IDENTITY
        }
    }

    pub fn scale(x: f64, y: f64) -> Affine {
        Affine {
            a: x,
            d: y,
            ..Affine::IDENTITY
        }
    }

    pub fn rotate(radians: f64) -> Affine {
        let (sin, cos) = radians.sin_cos();
        Affine {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            ..Affine::IDENTITY
        }
    }

    /// `self` first, then `outer`.
    pub fn then(self, outer: Affine) -> Affine {
        Affine {
            a: outer.a * self.a + outer.c * self.b,
            b: outer.b * self.a + outer.d * self.b,
            c: outer.a * self.c + outer.c * self.d,
            d: outer.b * self.c + outer.d * self.d,
            e: outer.a * self.e + outer.c * self.f + outer.e,
            f: outer.b * self.e + outer.d * self.f + outer.f,
        }
    }

    pub fn apply(&self, p: Point) -> Point {
        pt(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    pub fn apply_segment(&self, segment: ImportedSegment) -> ImportedSegment {
        match segment {
            ImportedSegment::Line { to } => ImportedSegment::Line { to: self.apply(to) },
            ImportedSegment::Quad { control, to } => ImportedSegment::Quad {
                control: self.apply(control),
                to: self.apply(to),
            },
            ImportedSegment::Cubic {
                control1,
                control2,
                to,
            } => ImportedSegment::Cubic {
                control1: self.apply(control1),
                control2: self.apply(control2),
                to: self.apply(to),
            },
        }
    }

    pub fn apply_subpath(&self, subpath: &mut ImportedSubpath) {
        subpath.start = self.apply(subpath.start);
        for segment in &mut subpath.segments {
            *segment = self.apply_segment(*segment);
        }
    }
}

pub(crate) fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// An elliptical arc `center + u cos t + v sin t` for `t` from `t0` through
/// `sweep` radians (negative runs clockwise), as cubics of at most 90° each.
/// Exact for circles up to the usual 0.03% cubic error, and for ellipses,
/// which are affine images of circles.
pub(crate) fn arc(center: Point, u: Point, v: Point, t0: f64, sweep: f64) -> ImportedSubpath {
    let at = |t: f64| {
        let (sin, cos) = t.sin_cos();
        pt(
            center.x + u.x * cos + v.x * sin,
            center.y + u.y * cos + v.y * sin,
        )
    };
    // Derivative direction, unscaled.
    let tangent = |t: f64| {
        let (sin, cos) = t.sin_cos();
        pt(-u.x * sin + v.x * cos, -u.y * sin + v.y * cos)
    };
    let pieces = (sweep.abs() / FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
    let step = sweep / pieces as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut segments = Vec::with_capacity(pieces);
    for i in 0..pieces {
        let a = t0 + step * i as f64;
        let b = a + step;
        let (p0, p1) = (at(a), at(b));
        let (d0, d1) = (tangent(a), tangent(b));
        segments.push(ImportedSegment::Cubic {
            control1: pt(p0.x + k * d0.x, p0.y + k * d0.y),
            control2: pt(p1.x - k * d1.x, p1.y - k * d1.y),
            to: p1,
        });
    }
    let closed = (sweep.abs() - TAU).abs() < 1e-9;
    ImportedSubpath {
        start: at(t0),
        segments,
        closed,
    }
}

/// The last point of a subpath.
pub(crate) fn end(subpath: &ImportedSubpath) -> Point {
    subpath.segments.last().map_or(subpath.start, |s| s.to())
}

/// The same curve traced the other way.
pub(crate) fn reverse(subpath: &ImportedSubpath) -> ImportedSubpath {
    let mut froms = Vec::with_capacity(subpath.segments.len());
    let mut from = subpath.start;
    for segment in &subpath.segments {
        froms.push(from);
        from = segment.to();
    }
    let segments = subpath
        .segments
        .iter()
        .zip(froms)
        .rev()
        .map(|(segment, from)| match *segment {
            ImportedSegment::Line { .. } => ImportedSegment::Line { to: from },
            ImportedSegment::Quad { control, .. } => ImportedSegment::Quad { control, to: from },
            ImportedSegment::Cubic {
                control1, control2, ..
            } => ImportedSegment::Cubic {
                control1: control2,
                control2: control1,
                to: from,
            },
        })
        .collect();
    ImportedSubpath {
        start: end(subpath),
        segments,
        closed: subpath.closed,
    }
}

/// Tight bounds `[min x, min y, max x, max y]`, curve extrema included but
/// control points that lie off the curve left out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Bounds(pub [f64; 4]);

impl Bounds {
    pub const EMPTY: Bounds = Bounds([f64::MAX, f64::MAX, f64::MIN, f64::MIN]);

    pub fn is_empty(&self) -> bool {
        self.0[0] > self.0[2]
    }

    pub fn width(&self) -> f64 {
        self.0[2] - self.0[0]
    }

    pub fn height(&self) -> f64 {
        self.0[3] - self.0[1]
    }

    pub fn add(&mut self, p: Point) {
        let b = &mut self.0;
        *b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
    }

    pub fn add_subpath(&mut self, subpath: &ImportedSubpath) {
        self.add(subpath.start);
        let mut from = subpath.start;
        for segment in &subpath.segments {
            match *segment {
                ImportedSegment::Line { to } => self.add(to),
                ImportedSegment::Quad { control, to } => {
                    self.add(to);
                    for t in quad_extrema(from, control, to) {
                        self.add(quad_at(from, control, to, t));
                    }
                }
                ImportedSegment::Cubic {
                    control1,
                    control2,
                    to,
                } => {
                    self.add(to);
                    for t in cubic_extrema(from, control1, control2, to) {
                        self.add(cubic_at(from, control1, control2, to, t));
                    }
                }
            }
            from = segment.to();
        }
    }
}

fn quad_at(p0: Point, p1: Point, p2: Point, t: f64) -> Point {
    let s = 1.0 - t;
    pt(
        s * s * p0.x + 2.0 * s * t * p1.x + t * t * p2.x,
        s * s * p0.y + 2.0 * s * t * p1.y + t * t * p2.y,
    )
}

fn cubic_at(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let s = 1.0 - t;
    let (a, b, c, d) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
    pt(
        a * p0.x + b * p1.x + c * p2.x + d * p3.x,
        a * p0.y + b * p1.y + c * p2.y + d * p3.y,
    )
}

fn quad_extrema(p0: Point, p1: Point, p2: Point) -> Vec<f64> {
    let root = |a: f64, b: f64, c: f64| {
        let denominator = a - 2.0 * b + c;
        (denominator.abs() > 1e-12).then(|| (a - b) / denominator)
    };
    [root(p0.x, p1.x, p2.x), root(p0.y, p1.y, p2.y)]
        .into_iter()
        .flatten()
        .filter(|t| *t > 0.0 && *t < 1.0)
        .collect()
}

fn cubic_extrema(p0: Point, p1: Point, p2: Point, p3: Point) -> Vec<f64> {
    let mut ts = Vec::new();
    for (a, b, c, d) in [(p0.x, p1.x, p2.x, p3.x), (p0.y, p1.y, p2.y, p3.y)] {
        // The derivative's coefficients, divided by 3.
        let qa = -a + 3.0 * b - 3.0 * c + d;
        let qb = 2.0 * (a - 2.0 * b + c);
        let qc = b - a;
        if qa.abs() < 1e-12 {
            if qb.abs() > 1e-12 {
                ts.push(-qc / qb);
            }
            continue;
        }
        let discriminant = qb * qb - 4.0 * qa * qc;
        if discriminant >= 0.0 {
            let root = discriminant.sqrt();
            ts.push((-qb + root) / (2.0 * qa));
            ts.push((-qb - root) / (2.0 * qa));
        }
    }
    ts.retain(|t| *t > 0.0 && *t < 1.0);
    ts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Point, b: Point) -> bool {
        distance(a, b) < 1e-9
    }

    #[test]
    fn quarter_arcs_end_where_the_circle_says() {
        let half = arc(
            pt(0.0, 0.0),
            pt(2.0, 0.0),
            pt(0.0, 2.0),
            0.0,
            std::f64::consts::PI,
        );
        assert_eq!(half.segments.len(), 2);
        assert!(near(half.start, pt(2.0, 0.0)));
        assert!(near(half.segments[0].to(), pt(0.0, 2.0)));
        assert!(near(end(&half), pt(-2.0, 0.0)));
        assert!(!half.closed);

        let full = arc(pt(1.0, 1.0), pt(1.0, 0.0), pt(0.0, 1.0), 0.0, TAU);
        assert!(full.closed && full.segments.len() == 4);
        let mut bounds = Bounds::EMPTY;
        bounds.add_subpath(&full);
        for (a, b) in bounds.0.iter().zip([0.0, 0.0, 2.0, 2.0]) {
            assert!((a - b).abs() < 1e-9, "{bounds:?}");
        }
    }

    #[test]
    fn clockwise_sweeps_go_the_other_way() {
        let cw = arc(pt(0.0, 0.0), pt(1.0, 0.0), pt(0.0, 1.0), 0.0, -FRAC_PI_2);
        assert!(near(end(&cw), pt(0.0, -1.0)));
    }

    #[test]
    fn bounds_of_a_45_degree_arc_skip_its_control_points() {
        let a = arc(
            pt(0.0, 0.0),
            pt(1.0, 0.0),
            pt(0.0, 1.0),
            -std::f64::consts::FRAC_PI_4,
            FRAC_PI_2,
        );
        let mut bounds = Bounds::EMPTY;
        bounds.add_subpath(&a);
        // The arc peaks at x = 1, between its endpoints.
        assert!((bounds.0[2] - 1.0).abs() < 1e-3, "{bounds:?}");
    }

    #[test]
    fn reversing_twice_is_the_identity() {
        let path = ImportedSubpath {
            start: pt(0.0, 0.0),
            segments: vec![
                ImportedSegment::Line { to: pt(1.0, 0.0) },
                ImportedSegment::Cubic {
                    control1: pt(2.0, 0.0),
                    control2: pt(2.0, 1.0),
                    to: pt(1.0, 1.0),
                },
            ],
            closed: false,
        };
        let back = reverse(&path);
        assert_eq!(back.start, pt(1.0, 1.0));
        assert_eq!(
            back.segments[0],
            ImportedSegment::Cubic {
                control1: pt(2.0, 1.0),
                control2: pt(2.0, 0.0),
                to: pt(1.0, 0.0),
            }
        );
        assert_eq!(reverse(&back), path);
    }

    #[test]
    fn transforms_compose_in_order() {
        let m = Affine::scale(2.0, 2.0).then(Affine::translate(1.0, 0.0));
        assert!(near(m.apply(pt(1.0, 1.0)), pt(3.0, 2.0)));
        let r = Affine::rotate(FRAC_PI_2);
        assert!(near(r.apply(pt(1.0, 0.0)), pt(0.0, 1.0)));
    }
}
