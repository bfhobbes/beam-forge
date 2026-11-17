//! B-splines to Béziers. A polynomial spline of degree 1-3 splits exactly
//! into lines, quadratics or cubics, one per knot span. Rational splines and
//! higher degrees are sampled into short lines instead.

use super::super::curve::{Point, pt};
use super::super::{ImportedSegment, ImportedSubpath};

/// Lines per knot span when a spline has to be sampled.
const SAMPLES_PER_SPAN: usize = 16;

pub(crate) struct Spline<'a> {
    pub degree: usize,
    pub knots: &'a [f64],
    pub control: &'a [Point],
    /// Empty, or one per control point.
    pub weights: &'a [f64],
}

/// The spline as one subpath, or None when its data doesn't describe a curve.
pub(crate) fn spline_to_subpath(spline: &Spline) -> Option<ImportedSubpath> {
    let p = spline.degree;
    let n = spline.control.len();
    if p == 0 || n <= p {
        return None;
    }
    let generated;
    let knots = if spline.knots.len() == n + p + 1 {
        spline.knots
    } else if spline.knots.is_empty() {
        generated = clamped_uniform(n, p);
        &generated
    } else {
        return None;
    };
    if knots.windows(2).any(|w| w[1] < w[0]) {
        return None;
    }
    let rational = spline.weights.len() == n
        && spline
            .weights
            .iter()
            .any(|w| (w - spline.weights[0]).abs() > 1e-12);
    if rational && spline.weights.iter().any(|w| *w <= 0.0) {
        return None;
    }
    // Homogeneous control points: (x w, y w, w).
    let points: Vec<[f64; 3]> = spline
        .control
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let w = if rational { spline.weights[i] } else { 1.0 };
            [c.x * w, c.y * w, w]
        })
        .collect();

    let spans: Vec<usize> = (p..n).filter(|&k| knots[k + 1] > knots[k]).collect();
    let first = *spans.first()?;
    let start = project(blossom(knots, &points, p, first, &vec![knots[first]; p]));
    let mut segments = Vec::new();
    for k in spans {
        let (a, b) = (knots[k], knots[k + 1]);
        if rational || p > 3 {
            for i in 1..=SAMPLES_PER_SPAN {
                let t = a + (b - a) * i as f64 / SAMPLES_PER_SPAN as f64;
                let to = project(blossom(knots, &points, p, k, &vec![t; p]));
                segments.push(ImportedSegment::Line { to });
            }
            continue;
        }
        // Bézier control point j of this span is the blossom at
        // (a × (p - j), b × j).
        let bezier: Vec<Point> = (0..=p)
            .map(|j| {
                let mut args = vec![a; p - j];
                args.resize(p, b);
                project(blossom(knots, &points, p, k, &args))
            })
            .collect();
        segments.push(match p {
            1 => ImportedSegment::Line { to: bezier[1] },
            2 => ImportedSegment::Quad {
                control: bezier[1],
                to: bezier[2],
            },
            _ => ImportedSegment::Cubic {
                control1: bezier[1],
                control2: bezier[2],
                to: bezier[3],
            },
        });
    }
    Some(ImportedSubpath {
        start,
        segments,
        closed: false,
    })
}

/// de Boor's algorithm with a different parameter at each level: the
/// spline's blossom over span `k`. With every argument equal it is the
/// point on the curve.
fn blossom(knots: &[f64], points: &[[f64; 3]], p: usize, k: usize, args: &[f64]) -> [f64; 3] {
    let mut d: Vec<[f64; 3]> = points[k - p..=k].to_vec();
    for r in 1..=p {
        let t = args[r - 1];
        for j in (r..=p).rev() {
            let i = j + k - p;
            let denominator = knots[i + 1 + p - r] - knots[i];
            let alpha = if denominator == 0.0 {
                0.0
            } else {
                (t - knots[i]) / denominator
            };
            let previous = d[j - 1];
            for (value, before) in d[j].iter_mut().zip(previous) {
                *value = (1.0 - alpha) * before + alpha * *value;
            }
        }
    }
    d[p]
}

fn project([x, y, w]: [f64; 3]) -> Point {
    pt(x / w, y / w)
}

/// Knots for `n` control points that start and end on the end points.
fn clamped_uniform(n: usize, p: usize) -> Vec<f64> {
    let inner = n - p;
    (0..n + p + 1)
        .map(|i| (i.saturating_sub(p)).min(inner) as f64)
        .collect()
}

/// A smooth curve through `fit` points (Catmull-Rom as cubics), for splines
/// stored only by the points they pass through.
pub(crate) fn through_points(fit: &[Point], closed: bool) -> Option<ImportedSubpath> {
    let n = fit.len();
    if n < 2 {
        return None;
    }
    let at = |i: isize| -> Point {
        if closed {
            fit[i.rem_euclid(n as isize) as usize]
        } else {
            fit[i.clamp(0, n as isize - 1) as usize]
        }
    };
    let count = if closed { n } else { n - 1 };
    let segments = (0..count as isize)
        .map(|i| {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            ImportedSegment::Cubic {
                control1: pt(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0),
                control2: pt(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0),
                to: p2,
            }
        })
        .collect();
    Some(ImportedSubpath {
        start: fit[0],
        segments,
        closed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    }

    #[test]
    fn a_single_span_cubic_is_its_own_bezier() {
        let control = [pt(0.0, 0.0), pt(1.0, 2.0), pt(3.0, 2.0), pt(4.0, 0.0)];
        let subpath = spline_to_subpath(&Spline {
            degree: 3,
            knots: &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            control: &control,
            weights: &[],
        })
        .unwrap();
        assert!(near(subpath.start, control[0]));
        assert_eq!(
            subpath.segments,
            [ImportedSegment::Cubic {
                control1: control[1],
                control2: control[2],
                to: control[3],
            }]
        );
    }

    #[test]
    fn interior_knots_split_into_continuous_beziers() {
        let control = [
            pt(0.0, 0.0),
            pt(1.0, 3.0),
            pt(3.0, 3.0),
            pt(4.0, 0.0),
            pt(6.0, -2.0),
        ];
        let subpath = spline_to_subpath(&Spline {
            degree: 3,
            knots: &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
            control: &control,
            weights: &[],
        })
        .unwrap();
        assert_eq!(subpath.segments.len(), 2);
        assert!(near(subpath.segments[1].to(), control[4]));
        // C1 at the join: the controls either side are collinear with it.
        let (ImportedSegment::Cubic { control2, to, .. }, ImportedSegment::Cubic { control1, .. }) =
            (subpath.segments[0], subpath.segments[1])
        else {
            panic!()
        };
        let (ax, ay) = (to.x - control2.x, to.y - control2.y);
        let (bx, by) = (control1.x - to.x, control1.y - to.y);
        assert!((ax * by - ay * bx).abs() < 1e-9);
    }

    #[test]
    fn degree_one_splines_are_polylines_and_missing_knots_are_clamped() {
        let control = [pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0)];
        let subpath = spline_to_subpath(&Spline {
            degree: 1,
            knots: &[],
            control: &control,
            weights: &[],
        })
        .unwrap();
        let ends: Vec<_> = subpath.segments.iter().map(|s| s.to()).collect();
        assert_eq!(ends, [control[1], control[2]]);
    }

    #[test]
    fn rational_quarter_circle_is_sampled_on_the_circle() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let control = [pt(1.0, 0.0), pt(1.0, 1.0), pt(0.0, 1.0)];
        let subpath = spline_to_subpath(&Spline {
            degree: 2,
            knots: &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control: &control,
            weights: &[1.0, w, 1.0],
        })
        .unwrap();
        assert_eq!(subpath.segments.len(), SAMPLES_PER_SPAN);
        for segment in &subpath.segments {
            let p = segment.to();
            assert!((p.x.hypot(p.y) - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn bad_knots_are_rejected() {
        let control = [pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0)];
        let spline = |knots: &'static [f64]| Spline {
            degree: 2,
            knots,
            control: &control,
            weights: &[],
        };
        assert!(spline_to_subpath(&spline(&[0.0, 1.0])).is_none());
        assert!(spline_to_subpath(&spline(&[0.0, 0.0, 1.0, 0.5, 1.0, 1.0])).is_none());
    }

    #[test]
    fn fit_points_curve_passes_through_them() {
        let fit = [pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 0.0)];
        let subpath = through_points(&fit, false).unwrap();
        assert_eq!(subpath.segments.len(), 2);
        assert!(near(subpath.segments[0].to(), fit[1]));
        assert!(near(subpath.segments[1].to(), fit[2]));
    }
}
