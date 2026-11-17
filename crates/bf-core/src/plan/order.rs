//! Cut order within a layer: every shape inside a closed shape is cut before
//! it, so a part doesn't drop out of the stock before its holes are cut. Among
//! the shapes that are free to cut, the head goes to the nearest one next.
//! A closed shape can start at any of its vertices; an open one at either end.

use crate::project::Point;

use super::outline::{Polyline, distance};

/// One path's place in the order: where to start it and which way to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Route {
    pub path: usize,
    /// The vertex the cut starts at.
    pub start: usize,
    /// Open paths only: cut from the last point back to the first.
    pub reversed: bool,
}

impl Route {
    /// The points to visit, in order. A closed path ends back at its start.
    pub fn points<'a>(&self, paths: &'a [Polyline]) -> impl Iterator<Item = Point> + 'a {
        let path = &paths[self.path];
        let n = path.points.len();
        let (start, reversed, closed) = (self.start, self.reversed, path.closed);
        let count = if closed { n + 1 } else { n };
        (0..count).map(move |i| {
            let index = if closed {
                (start + i) % n
            } else if reversed {
                n - 1 - i
            } else {
                i
            };
            path.points[index]
        })
    }

    fn end(&self, paths: &[Polyline]) -> Point {
        let path = &paths[self.path];
        match (path.closed, self.reversed) {
            (true, _) => path.points[self.start],
            (false, true) => path.points[0],
            (false, false) => path.points[path.points.len() - 1],
        }
    }
}

/// The order to cut `paths` in, starting with the head at `from`.
pub(super) fn order(paths: &[Polyline], from: Point) -> Vec<Route> {
    let boxes: Vec<Bounds> = paths.iter().map(|path| Bounds::of(&path.points)).collect();
    let containers = containers(paths, &boxes);
    // How many shapes inside each path are still uncut.
    let mut blockers = vec![0usize; paths.len()];
    for outer in containers.iter().flatten() {
        blockers[*outer] += 1;
    }
    let mut done = vec![false; paths.len()];
    let mut grid = Grid::new(paths, &done);
    let mut routes = Vec::with_capacity(paths.len());
    let mut at = from;
    while routes.len() < paths.len() {
        let free = |path: usize| !done[path] && blockers[path] == 0;
        let route = match grid.nearest(paths, at, free) {
            Some(route) => route,
            // Can't happen (containment needs a strictly larger area, so it
            // has no cycles), but never loop forever or drop a shape.
            None => {
                let path = (0..paths.len()).find(|&p| !done[p]).unwrap_or_default();
                Route {
                    path,
                    start: 0,
                    reversed: false,
                }
            }
        };
        done[route.path] = true;
        for &outer in &containers[route.path] {
            blockers[outer] -= 1;
        }
        grid.remove(paths, route.path, &done);
        at = route.end(paths);
        routes.push(route);
    }
    routes
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn of(points: &[Point]) -> Bounds {
        points.iter().fold(
            Bounds {
                min_x: f64::INFINITY,
                min_y: f64::INFINITY,
                max_x: f64::NEG_INFINITY,
                max_y: f64::NEG_INFINITY,
            },
            |b, p| Bounds {
                min_x: b.min_x.min(p.x),
                min_y: b.min_y.min(p.y),
                max_x: b.max_x.max(p.x),
                max_y: b.max_y.max(p.y),
            },
        )
    }

    fn contains(&self, inner: &Bounds) -> bool {
        self.min_x <= inner.min_x
            && self.min_y <= inner.min_y
            && self.max_x >= inner.max_x
            && self.max_y >= inner.max_y
    }
}

/// Polygon vertices tested to decide whether one path is inside another.
const CONTAINMENT_SAMPLES: usize = 8;

/// For each path, the closed paths it lies inside. A path is inside a closed
/// shape when that shape's bounds hold its bounds, it encloses a sample of the
/// path's vertices, and (so two copies of a shape don't wait on each other)
/// its area is strictly larger.
fn containers(paths: &[Polyline], boxes: &[Bounds]) -> Vec<Vec<usize>> {
    let areas: Vec<f64> = paths
        .iter()
        .map(|path| if path.closed { area(&path.points) } else { 0.0 })
        .collect();
    // Closed paths by left edge, so each path only checks those starting left of it.
    let mut closed: Vec<usize> = (0..paths.len()).filter(|&p| paths[p].closed).collect();
    closed.sort_by(|&a, &b| boxes[a].min_x.total_cmp(&boxes[b].min_x));
    (0..paths.len())
        .map(|inner| {
            let reach = closed.partition_point(|&outer| boxes[outer].min_x <= boxes[inner].min_x);
            closed[..reach]
                .iter()
                .copied()
                .filter(|&outer| {
                    outer != inner
                        && areas[outer] > areas[inner]
                        && boxes[outer].contains(&boxes[inner])
                        && encloses(&paths[outer].points, &paths[inner].points)
                })
                .collect()
        })
        .collect()
}

fn area(points: &[Point]) -> f64 {
    let n = points.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    twice.abs() / 2.0
}

/// Whether the polygon `outer` holds evenly spaced samples of `inner`.
fn encloses(outer: &[Point], inner: &[Point]) -> bool {
    let step = inner.len().div_ceil(CONTAINMENT_SAMPLES).max(1);
    inner
        .iter()
        .step_by(step)
        .all(|&point| inside(outer, point))
}

/// Even-odd ray cast.
fn inside(polygon: &[Point], p: Point) -> bool {
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for (i, a) in polygon.iter().enumerate() {
        let b = polygon[j];
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// A place a path can start: its index and the vertex.
#[derive(Debug, Clone, Copy)]
struct Entry {
    path: u32,
    vertex: u32,
}

/// A uniform grid of the uncut paths' start points, for nearest-neighbour
/// search. Cut paths' entries are skipped and, once most of the grid is
/// stale, dropped by rebuilding it.
struct Grid {
    min_x: f64,
    min_y: f64,
    cell: f64,
    columns: usize,
    rows: usize,
    cells: Vec<Vec<Entry>>,
    live: usize,
    stored: usize,
}

/// About this many entries share a cell.
const ENTRIES_PER_CELL: f64 = 2.0;

fn entries(paths: &[Polyline], path: usize) -> impl Iterator<Item = Entry> + '_ {
    let polyline = &paths[path];
    let n = polyline.points.len();
    let ends = [0, n - 1];
    let vertices: Box<dyn Iterator<Item = usize>> = if polyline.closed {
        Box::new(0..n)
    } else {
        Box::new(ends.into_iter())
    };
    vertices.map(move |vertex| Entry {
        path: path as u32,
        vertex: vertex as u32,
    })
}

impl Grid {
    fn new(paths: &[Polyline], done: &[bool]) -> Grid {
        let all: Vec<Entry> = (0..paths.len())
            .filter(|&p| !done[p])
            .flat_map(|p| entries(paths, p))
            .collect();
        let point = |e: &Entry| paths[e.path as usize].points[e.vertex as usize];
        let bounds = Bounds::of(&all.iter().map(point).collect::<Vec<_>>());
        let (width, height) = (
            (bounds.max_x - bounds.min_x).max(0.0),
            (bounds.max_y - bounds.min_y).max(0.0),
        );
        let target = (all.len() as f64 / ENTRIES_PER_CELL).max(1.0);
        let cell = ((width * height / target).sqrt())
            .max(width.max(height) / target)
            .max(1e-6);
        let columns = ((width / cell) as usize + 1).max(1);
        let rows = ((height / cell) as usize + 1).max(1);
        let mut grid = Grid {
            min_x: if all.is_empty() { 0.0 } else { bounds.min_x },
            min_y: if all.is_empty() { 0.0 } else { bounds.min_y },
            cell,
            columns,
            rows,
            cells: vec![Vec::new(); columns * rows],
            live: all.len(),
            stored: all.len(),
        };
        for entry in all {
            let (column, row) = grid.cell_of(point(&entry));
            grid.cells[row * grid.columns + column].push(entry);
        }
        grid
    }

    fn cell_of(&self, p: Point) -> (usize, usize) {
        let column = ((p.x - self.min_x) / self.cell).floor().max(0.0) as usize;
        let row = ((p.y - self.min_y) / self.cell).floor().max(0.0) as usize;
        (column.min(self.columns - 1), row.min(self.rows - 1))
    }

    /// Notes that `path` is cut; rebuilds once three quarters are stale.
    fn remove(&mut self, paths: &[Polyline], path: usize, done: &[bool]) {
        self.live -= entries(paths, path).count();
        if self.stored > 64 && self.live * 4 < self.stored {
            *self = Grid::new(paths, done);
        }
    }

    /// The route to the closest start point of a path that `free` allows.
    fn nearest(
        &self,
        paths: &[Polyline],
        from: Point,
        free: impl Fn(usize) -> bool,
    ) -> Option<Route> {
        // How far `from` is outside the grid. Every grid point is at least
        // √(outside² + ring²) away, since the grid is a convex box.
        let clamped = Point {
            x: from
                .x
                .clamp(self.min_x, self.min_x + self.columns as f64 * self.cell),
            y: from
                .y
                .clamp(self.min_y, self.min_y + self.rows as f64 * self.cell),
        };
        let outside = distance(from, clamped);
        let (cx, cy) = self.cell_of(clamped);
        let mut best: Option<(f64, Entry)> = None;
        for ring in 0..=self.columns.max(self.rows) {
            let reach = (ring.saturating_sub(1) as f64) * self.cell;
            if best.is_some_and(|(d, _)| outside.hypot(reach) > d) {
                break;
            }
            let (x0, x1) = (cx as isize - ring as isize, cx as isize + ring as isize);
            let (y0, y1) = (cy as isize - ring as isize, cy as isize + ring as isize);
            let mut visit = |x: isize, y: isize| {
                if x < 0 || y < 0 || x >= self.columns as isize || y >= self.rows as isize {
                    return;
                }
                for entry in &self.cells[y as usize * self.columns + x as usize] {
                    if !free(entry.path as usize) {
                        continue;
                    }
                    let d = distance(
                        from,
                        paths[entry.path as usize].points[entry.vertex as usize],
                    );
                    if best.is_none_or(|(b, _)| d < b) {
                        best = Some((d, *entry));
                    }
                }
            };
            // The ring's top and bottom rows, then its two sides.
            let (columns, rows) = (self.columns as isize, self.rows as isize);
            for x in x0.max(0)..=x1.min(columns - 1) {
                visit(x, y0);
                if y1 != y0 {
                    visit(x, y1);
                }
            }
            for y in (y0 + 1).max(0)..y1.min(rows) {
                visit(x0, y);
                visit(x1, y);
            }
        }
        best.map(|(_, entry)| {
            let path = entry.path as usize;
            let vertex = entry.vertex as usize;
            let polyline = &paths[path];
            let reversed = !polyline.closed && vertex != 0;
            Route {
                path,
                start: if polyline.closed { vertex } else { 0 },
                reversed,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, size: f64) -> Polyline {
        Polyline {
            points: vec![
                Point { x, y },
                Point { x: x + size, y },
                Point {
                    x: x + size,
                    y: y + size,
                },
                Point { x, y: y + size },
            ],
            closed: true,
        }
    }

    fn origin() -> Point {
        Point { x: 0.0, y: 0.0 }
    }

    #[test]
    fn closed_paths_start_at_the_nearest_vertex_and_return_to_it() {
        let paths = [square(10.0, 10.0, 5.0)];
        let routes = order(&paths, Point { x: 20.0, y: 20.0 });
        assert_eq!(routes[0].start, 2);
        let points: Vec<_> = routes[0].points(&paths).map(|p| (p.x, p.y)).collect();
        assert_eq!(
            points,
            [
                (15.0, 15.0),
                (10.0, 15.0),
                (10.0, 10.0),
                (15.0, 10.0),
                (15.0, 15.0)
            ]
        );
    }

    #[test]
    fn open_paths_start_from_their_nearer_end() {
        let line = Polyline {
            points: vec![Point { x: 10.0, y: 0.0 }, Point { x: 1.0, y: 0.0 }],
            closed: false,
        };
        let paths = [line];
        let routes = order(&paths, origin());
        assert!(routes[0].reversed);
        let points: Vec<_> = routes[0].points(&paths).map(|p| p.x).collect();
        assert_eq!(points, [1.0, 10.0]);
    }

    #[test]
    fn identical_shapes_do_not_wait_on_each_other() {
        let paths = [square(0.0, 0.0, 10.0), square(0.0, 0.0, 10.0)];
        assert_eq!(order(&paths, origin()).len(), 2);
    }

    #[test]
    fn the_nearest_neighbour_holds_on_a_sparse_scattered_grid() {
        // Far-apart clusters with lots of rebuilds: compare with brute force.
        let mut paths = Vec::new();
        for i in 0..300 {
            let x = f64::from((i * 7919) % 997) * if i % 3 == 0 { 10.0 } else { 0.1 };
            let y = f64::from((i * 104_729) % 991) * 0.5;
            paths.push(square(x, y, 0.5));
        }
        let routes = order(&paths, Point { x: -50.0, y: 900.0 });
        let mut at = Point { x: -50.0, y: 900.0 };
        let mut done = vec![false; paths.len()];
        for route in routes {
            let best = (0..paths.len())
                .filter(|&p| !done[p])
                .flat_map(|p| paths[p].points.iter().map(move |q| distance(at, *q)))
                .fold(f64::INFINITY, f64::min);
            let chosen = distance(at, paths[route.path].points[route.start]);
            assert!((chosen - best).abs() < 1e-9, "{chosen} vs {best}");
            done[route.path] = true;
            at = route.end(&paths);
        }
    }
}
