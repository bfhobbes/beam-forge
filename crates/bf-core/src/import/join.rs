//! Joins open pieces whose ends meet into longer chains, and closes chains
//! that come back to where they began. CAD exports often write a rectangle
//! as four loose lines; cutting it as one closed path keeps the laser moving
//! and lets the planner treat it as a shape with an inside.

use std::collections::HashMap;

use super::ImportedSubpath;
use super::curve::{Point, distance, end, reverse};

/// One open or closed piece and the key it may only join within, e.g. its
/// layer and colour.
pub(crate) struct Piece<K> {
    pub key: K,
    pub subpath: ImportedSubpath,
}

/// Chains the open pieces end to end where their ends lie within
/// `tolerance` of each other, keeping the first piece's direction where it
/// can. Pieces only join others with the same key. Closed pieces pass
/// through. The result keeps the input order of each chain's first piece.
pub(crate) fn join<K: PartialEq>(pieces: Vec<Piece<K>>, tolerance: f64) -> Vec<Piece<K>> {
    let cell = |p: Point| {
        (
            (p.x / tolerance).floor() as i64,
            (p.y / tolerance).floor() as i64,
        )
    };
    // Open pieces' ends by grid cell. Entries go stale as pieces join; every
    // match is checked against the piece's current ends.
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (index, piece) in pieces.iter().enumerate() {
        if !piece.subpath.closed {
            for p in [piece.subpath.start, end(&piece.subpath)] {
                grid.entry(cell(p)).or_default().push(index);
            }
        }
    }
    let mut slots: Vec<Option<Piece<K>>> = pieces.into_iter().map(Some).collect();

    // Another open piece with `key` and an end within tolerance of `p`, and
    // whether that end is its start.
    let find = |slots: &[Option<Piece<K>>], me: usize, key: &K, p: Point| {
        let (cx, cy) = cell(p);
        let mut best: Option<(usize, bool, f64)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &other in grid.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    let Some(piece) = slots[other].as_ref() else {
                        continue;
                    };
                    if other == me || piece.subpath.closed || piece.key != *key {
                        continue;
                    }
                    for (at_start, q) in [(true, piece.subpath.start), (false, end(&piece.subpath))]
                    {
                        let d = distance(p, q);
                        let better = best.is_none_or(|(i, s, bd)| {
                            d < bd || (d == bd && (other, !at_start) < (i, !s))
                        });
                        if d <= tolerance && better {
                            best = Some((other, at_start, d));
                        }
                    }
                }
            }
        }
        best.map(|(index, at_start, _)| (index, at_start))
    };

    for me in 0..slots.len() {
        let Some(mut chain) = slots[me].take() else {
            continue;
        };
        if chain.subpath.closed {
            slots[me] = Some(chain);
            continue;
        }
        // Grow forwards from the end, then backwards from the start.
        while let Some((other, at_start)) = find(&slots, me, &chain.key, end(&chain.subpath)) {
            let piece = slots[other]
                .take()
                .expect("found pieces are present")
                .subpath;
            let next = if at_start { piece } else { reverse(&piece) };
            chain.subpath.segments.extend(next.segments);
            if closes(&chain.subpath, tolerance) {
                break;
            }
        }
        while !closes(&chain.subpath, tolerance)
            && let Some((other, at_start)) = find(&slots, me, &chain.key, chain.subpath.start)
        {
            let piece = slots[other]
                .take()
                .expect("found pieces are present")
                .subpath;
            let mut before = if at_start { reverse(&piece) } else { piece };
            before
                .segments
                .extend(std::mem::take(&mut chain.subpath.segments));
            chain.subpath = before;
        }
        if closes(&chain.subpath, tolerance) {
            close(&mut chain.subpath);
        }
        slots[me] = Some(chain);
    }
    slots.into_iter().flatten().collect()
}

/// Whether an open subpath ends where it starts and encloses something.
fn closes(subpath: &ImportedSubpath, tolerance: f64) -> bool {
    let curved = subpath
        .segments
        .iter()
        .any(|s| !matches!(s, super::ImportedSegment::Line { .. }));
    (subpath.segments.len() >= 3 || (curved && !subpath.segments.is_empty()))
        && distance(subpath.start, end(subpath)) <= tolerance
}

/// Marks a subpath closed and moves its last point onto its start, so the
/// gap that was within tolerance is gone.
fn close(subpath: &mut ImportedSubpath) {
    let start = subpath.start;
    if let Some(last) = subpath.segments.last_mut() {
        match last {
            super::ImportedSegment::Line { to }
            | super::ImportedSegment::Quad { to, .. }
            | super::ImportedSegment::Cubic { to, .. } => *to = start,
        }
    }
    subpath.closed = true;
}

#[cfg(test)]
mod tests {
    use super::super::ImportedSegment;
    use super::super::curve::pt;
    use super::*;

    fn line(key: u8, from: (f64, f64), to: (f64, f64)) -> Piece<u8> {
        Piece {
            key,
            subpath: ImportedSubpath {
                start: pt(from.0, from.1),
                segments: vec![ImportedSegment::Line { to: pt(to.0, to.1) }],
                closed: false,
            },
        }
    }

    fn points(subpath: &ImportedSubpath) -> Vec<(f64, f64)> {
        std::iter::once(subpath.start)
            .chain(subpath.segments.iter().map(|s| s.to()))
            .map(|p| (p.x, p.y))
            .collect()
    }

    #[test]
    fn four_loose_lines_become_one_closed_rectangle() {
        // Out of order, one reversed, and one end 0.005 off.
        let joined = join(
            vec![
                line(0, (10.0, 0.0), (10.0, 5.0)),
                line(0, (0.0, 5.0), (0.0, 0.0)),
                line(0, (10.0, 5.0), (0.0, 5.0)),
                line(0, (10.0, 0.0), (0.005, 0.0)),
            ],
            0.01,
        );
        assert_eq!(joined.len(), 1);
        let rect = &joined[0].subpath;
        assert!(rect.closed);
        assert_eq!(
            points(rect),
            [
                (10.0, 0.0),
                (10.0, 5.0),
                (0.0, 5.0),
                (0.0, 0.0),
                (10.0, 0.0)
            ]
        );
    }

    #[test]
    fn ends_further_apart_than_the_tolerance_stay_apart() {
        let joined = join(
            vec![
                line(0, (0.0, 0.0), (1.0, 0.0)),
                line(0, (1.02, 0.0), (2.0, 0.0)),
            ],
            0.01,
        );
        assert_eq!(joined.len(), 2);
    }

    #[test]
    fn pieces_only_join_within_their_key() {
        let joined = join(
            vec![
                line(0, (0.0, 0.0), (1.0, 0.0)),
                line(1, (1.0, 0.0), (2.0, 0.0)),
                line(0, (1.0, 0.0), (1.0, 1.0)),
            ],
            0.01,
        );
        assert_eq!(joined.len(), 2);
        assert_eq!(
            points(&joined[0].subpath),
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]
        );
        assert_eq!(joined[1].key, 1);
    }

    #[test]
    fn open_chains_grow_from_both_ends() {
        let joined = join(
            vec![
                line(0, (1.0, 0.0), (2.0, 0.0)),
                line(0, (0.0, 0.0), (1.0, 0.0)),
                line(0, (3.0, 0.0), (2.0, 0.0)),
            ],
            0.01,
        );
        assert_eq!(joined.len(), 1);
        assert!(!joined[0].subpath.closed);
        assert_eq!(
            points(&joined[0].subpath),
            [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]
        );
    }

    #[test]
    fn a_line_out_and_back_is_not_a_closed_shape() {
        let joined = join(
            vec![
                line(0, (0.0, 0.0), (1.0, 0.0)),
                line(0, (1.0, 0.0), (0.0, 0.0)),
            ],
            0.01,
        );
        assert_eq!(joined.len(), 1);
        assert!(!joined[0].subpath.closed);
    }
}
