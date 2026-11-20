use crate::math::Portable as _;
// ---- cover: the floor an op can reach, and the stretches it cannot -----------
// Phase 32 slice 2 (`DECISIONS.md` → "Phase 32 slice 2: the concession…").
// An op whose floor has a stretch no pass can enter refuses — the law since
// Phase 9 — and this is the one home that says WHAT it cannot reach: every
// stretch, with its area, where it is, how wide, what stands either side of
// it, and the largest tool that would fit it; then the share of the floor
// that IS reachable, which is the number the `cover=` directive on the run
// line concedes. Std only; the same grid `compare::uncut` counts on.

use crate::parse::{Operation, Pt};
use crate::region::Region;
use crate::stock::{dist_to_polygon, point_in_polygon};

/// One connected stretch of floor no pass reaches.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretch {
    pub area_mm2: f64,
    /// Its widest point — where a tool would have to stand.
    pub at: Pt,
    /// The floor's width there: twice the clearance to what stands.
    pub width: f64,
    /// What stands either side of it, nearest first — feature names, or the
    /// host's own wall.
    pub between: Vec<String>,
    /// Its outline: the stretch's cells grown by one cell and traced, so it
    /// can only over-claim what is left standing. The stock model holds this
    /// back from the op's removal; it is the REST FLOOR a follow-up op takes.
    pub outline: Vec<Pt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Coverage {
    pub floor_mm2: f64,
    pub reachable_mm2: f64,
    pub stretches: Vec<Stretch>,
}

impl Coverage {
    pub fn reachable_pct(&self) -> f64 {
        if self.floor_mm2 <= 0.0 {
            100.0
        } else {
            100.0 * self.reachable_mm2 / self.floor_mm2
        }
    }

    /// The percentage a `cover=` line may ask for and pass: this coverage,
    /// rounded DOWN to one decimal.
    pub fn concession(&self) -> f64 {
        (self.reachable_pct() * 10.0).floor() / 10.0
    }
}

/// The grid's cell: `compare::UNCUT_CELL`'s own.
const CELL: f64 = crate::compare::UNCUT_CELL;

/// A stretch smaller than this is chording disagreement at a concave arc,
/// not floor a tool cannot reach: half a square millimetre, twelve cells.
pub(crate) const LEAST_STRETCH: f64 = 0.5;

/// The floor's coverage under this op's tool. The floor is the region's own
/// loops (a `Face`'s authored ones; everything else's outer); a cell is
/// REACHABLE when it lies within the tool's radius of where the tool's centre
/// may be — the offset family's outermost level, measured to the edges that
/// actually bound it.
pub fn of(region: &Region, floor: &[Vec<Pt>], ccw: &[Vec<Pt>], cw: &[Vec<Pt>], r: f64) -> Coverage {
    let h = CELL;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for q in floor.iter().flatten() {
        (x0, y0, x1, y1) = (x0.min(q.x), y0.min(q.y), x1.max(q.x), y1.max(q.y));
    }
    if !x0.is_finite() {
        return Coverage { floor_mm2: 0.0, reachable_mm2: 0.0, stretches: Vec::new() };
    }
    let (nx, ny) = (((x1 - x0) / h).ceil() as usize + 1, ((y1 - y0) / h).ceil() as usize + 1);
    let centre = |i: usize, j: usize| Pt { x: x0 + (i as f64 + 0.5) * h, y: y0 + (j as f64 + 0.5) * h };
    let on_floor = |q: Pt| point_in_polygon(&floor[0], q.x, q.y) && !floor[1..].iter().any(|l| point_in_polygon(l, q.x, q.y));
    let in_allowed = |q: Pt| ccw.iter().any(|l| point_in_polygon(l, q.x, q.y)) && !cw.iter().any(|l| point_in_polygon(l, q.x, q.y));
    // The allowed region's BOUNDARY: the offsetter can hand an island's loop
    // back overlapping the wall's instead of merged into it (an allowance on a
    // web does), and a stretch of loop lying outside the region bounds
    // nothing — near it is not near where the centre may be.
    let mut boundary: Vec<(Pt, Pt)> = Vec::new();
    for l in ccw {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let m = Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
            if !cw.iter().any(|c| point_in_polygon(c, m.x, m.y)) {
                boundary.push((a, b));
            }
        }
    }
    for l in cw {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let m = Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
            if ccw.iter().any(|c| point_in_polygon(c, m.x, m.y)) && !cw.iter().any(|c| !std::ptr::eq(c, l) && point_in_polygon(c, m.x, m.y)) {
                boundary.push((a, b));
            }
        }
    }
    let to_boundary = |q: Pt| boundary.iter().map(|(a, b)| dist_to_polygon(&[*a, *b], q.x, q.y)).fold(f64::INFINITY, f64::min);
    // The same question, fast, for the whole grid: "within r of the
    // boundary?" asks only the segments bucketed near the point.
    let near_boundary = Buckets::of(&boundary, r.max(h));
    let within_r = |q: Pt| near_boundary.any_within(q, r);
    // Exactly the radius: a margin here once made a strip down the middle of
    // a web read reachable and split one stretch in two. Corners, which the
    // margin was for, are told apart below by what stands, not by distance.
    let reach = |q: Pt| in_allowed(q) || to_boundary(q) <= r;
    let dist_to_allowed = |q: Pt| if in_allowed(q) { 0.0 } else { to_boundary(q) };
    // 0 = not floor, 1 = reachable floor, 2 = unreachable floor.
    let mut kind = vec![0u8; nx * ny];
    let (mut floor_cells, mut reach_cells) = (0usize, 0usize);
    // Inside/outside by ROW, not by cell: each row's crossings with each loop
    // once, cells marked by parity — the same answer `point_in_polygon` gives
    // at every cell centre, a few hundred times cheaper.
    let xs: Vec<f64> = (0..nx).map(|i| x0 + (i as f64 + 0.5) * h).collect();
    for j in 0..ny {
        let y = y0 + (j as f64 + 0.5) * h;
        let floor_row = and_not(&row_inside(&floor[..1], y, &xs), &row_inside(&floor[1..], y, &xs));
        let allowed_row = and_not(&row_inside(ccw, y, &xs), &row_inside(cw, y, &xs));
        for i in 0..nx {
            let c = centre(i, j);
            if floor_row[i] {
                floor_cells += 1;
                if allowed_row[i] || within_r(c) {
                    reach_cells += 1;
                    kind[j * nx + i] = 1;
                } else {
                    kind[j * nx + i] = 2;
                }
            }
        }
    }
    // What stands: the islands, and the host's own wall edges.
    // (A circle is measured to the circle, not its chords: a chord sits
    // inside its arc, and a width read to it would say a slot is wider than
    // it is — the one direction "a tool up to X would reach it" cannot err.)
    let circles: Vec<Option<(f64, f64, f64)>> = region.standing().into_iter().map(|(_, i)| match i { crate::offset::Island::Circle { x, y, r } => Some((*x, *y, *r)), _ => None }).collect();
    let mut standing: Vec<(String, Vec<Pt>)> = region.standing().into_iter().map(|(n, i)| (n.to_string(), island_pts(i))).collect();
    let n = region.outer.len();
    for e in 0..n {
        if region.edges[e] == crate::region::EdgeKind::Wall {
            standing.push((format!("the wall of '{}'", region.host), vec![region.outer[e], region.outer[(e + 1) % n]]));
        }
    }
    // The floor's OPEN edges: beyond them is air past the blank, where
    // nothing stands. A chord that exits through one is no pinch — the tool
    // reaches that floor from the air side — so it reads unbounded, and a
    // stretch with no bounded chord at all says so and names the edge.
    let open_edges: Vec<(Pt, Pt)> = (0..n).filter(|e| region.edges[*e] == crate::region::EdgeKind::Open).map(|e| (region.outer[e], region.outer[(e + 1) % n])).collect();
    let clearance = |q: Pt| -> Vec<(f64, usize)> {
        let mut d: Vec<(f64, usize)> = standing
            .iter()
            .enumerate()
            .map(|(k, (_, pts))| match circles.get(k).copied().flatten() {
                Some((cx, cy, cr)) => (((q.x - cx).phypot(q.y - cy) - cr).abs(), k),
                None => (dist_to_polygon(pts, q.x, q.y), k),
            })
            .collect();
        d.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        d
    };
    // The chord across the floor at `q`: from the nearest standing point,
    // through `q`, to the first standing thing beyond.
    let chord_full = |q: Pt| -> (f64, Pt, (f64, f64), bool) {
        let near = |k: usize| -> Pt {
            match circles.get(k).copied().flatten() {
                Some((cx, cy, cr)) => {
                    let d = (q.x - cx).phypot(q.y - cy).max(1e-12);
                    Pt { x: cx + (q.x - cx) / d * cr, y: cy + (q.y - cy) / d * cr }
                }
                None => nearest_on(&standing[k].1, q),
            }
        };
        let Some((_, k1)) = clearance(q).first().copied() else { return (f64::INFINITY, q, (0.0, 0.0), false) };
        let p1 = near(k1);
        let (ux, uy) = (q.x - p1.x, q.y - p1.y);
        let len = ux.phypot(uy);
        if len < 1e-9 {
            return (f64::INFINITY, q, (0.0, 0.0), false);
        }
        let (ux, uy) = (ux / len, uy / len);
        let mut t_hit = f64::INFINITY;
        for (k, (_, pts)) in standing.iter().enumerate() {
            match circles.get(k).copied().flatten() {
                Some((cx, cy, cr)) => {
                    // |p1 + t u - c|² = cr²
                    let (fx, fy) = (p1.x - cx, p1.y - cy);
                    let b = fx * ux + fy * uy;
                    let disc = b * b - (fx * fx + fy * fy - cr * cr);
                    if disc >= 0.0 {
                        for t in [-b - disc.sqrt(), -b + disc.sqrt()] {
                            if t > 1e-6 && t < t_hit {
                                t_hit = t;
                            }
                        }
                    }
                }
                None => {
                    let n = pts.len();
                    let edges = if n == 2 { 1 } else { n };
                    for e in 0..edges {
                        let (a, b) = (pts[e], pts[(e + 1) % n]);
                        let (ex, ey) = (b.x - a.x, b.y - a.y);
                        let den = ux * ey - uy * ex;
                        if den.abs() < 1e-12 {
                            continue;
                        }
                        let t = ((a.x - p1.x) * ey - (a.y - p1.y) * ex) / den;
                        let v = ((a.x - p1.x) * uy - (a.y - p1.y) * ux) / den;
                        if t > 1e-6 && (0.0..=1.0).contains(&v) && t < t_hit {
                            t_hit = t;
                        }
                    }
                }
            }
        }
        for (a, b) in &open_edges {
            let (ex, ey) = (b.x - a.x, b.y - a.y);
            let den = ux * ey - uy * ex;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = ((a.x - p1.x) * ey - (a.y - p1.y) * ex) / den;
            let v = ((a.x - p1.x) * uy - (a.y - p1.y) * ux) / den;
            if t > 1e-6 && (0.0..=1.0).contains(&v) && t < t_hit {
                return (f64::INFINITY, p1, (ux, uy), true);
            }
        }
        (t_hit, p1, (ux, uy), false)
    };
    let chord = |q: Pt| chord_full(q).0;
    // Flood the unreachable cells into stretches.
    let mut seen = vec![false; nx * ny];
    let mut stretches = Vec::new();
    for start in 0..nx * ny {
        if kind[start] != 2 || seen[start] {
            continue;
        }
        let mut stack = vec![start];
        seen[start] = true;
        let mut cells: Vec<usize> = Vec::new();
        while let Some(k) = stack.pop() {
            cells.push(k);
            let (i, j) = (k % nx, k / nx);
            for (di, dj) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (ii, jj) = (i as i64 + di, j as i64 + dj);
                if ii < 0 || jj < 0 || ii >= nx as i64 || jj >= ny as i64 {
                    continue;
                }
                let kk = jj as usize * nx + ii as usize;
                if kind[kk] == 2 && !seen[kk] {
                    seen[kk] = true;
                    stack.push(kk);
                }
            }
        }
        let area = cells.len() as f64 * h * h;
        if area < LEAST_STRETCH {
            reach_cells += cells.len(); // noise, not floor anyone failed to reach
            continue;
        }
        // The PINCH: the stretch's width is the narrowest chord across the
        // floor through it — from a point's nearest standing point, through the
        // point, to the first standing thing beyond. A tool that fits the pinch
        // fits the stretch; a lens-shaped web between a post and a wall is
        // widest at its tips and narrowest on the line of centres, and it is
        // the narrowest that is the web's width.
        // Seeded from the stretch's own first cell, never the grid's corner
        // (Phase 47 slice 1, found on the way): on a floor whose every edge
        // is OPEN — a Face run out to a round blank — the chord from a notch
        // cell escapes past the blank and meets nothing standing, so every
        // width is infinite and nothing below would move `best` off a
        // point outside the floor; the corner test then read what stood
        // nearest the grid's corner and called an L-island's own notch a
        // stretch between two features.
        let first = centre(cells[0] % nx, cells[0] / nx);
        let mut best: (f64, Pt, Vec<(f64, usize)>) = (chord(first), first, clearance(first));
        for k in &cells[1..] {
            let c = centre(k % nx, k / nx);
            let w = chord(c);
            if w < best.0 {
                best = (w, c, clearance(c));
            }
        }
        // Down to the true minimum off the grid.
        let (mut at, mut width) = (best.1, best.0);
        let mut step = h / 2.0;
        while step > 1e-4 && width.is_finite() {
            let moved = [(step, 0.0), (-step, 0.0), (0.0, step), (0.0, -step)]
                .into_iter()
                .map(|(dx, dy)| Pt { x: at.x + dx, y: at.y + dy })
                .filter(|q| on_floor(*q) && !reach(*q))
                .map(|q| (chord(q), q))
                .find(|(w, _)| *w < width - 1e-9);
            match moved {
                Some((w, q)) => (width, at) = (w, q),
                None => step /= 2.0,
            }
        }
        best.2 = clearance(at);
        // Said at the pinch's MIDDLE — every cell of a parallel slot ties on
        // width, and the one found first may lie on a wall's face.
        let (t, p1, (ux, uy), open_hit) = chord_full(at);
        best.1 = if t.is_finite() { Pt { x: p1.x + ux * t / 2.0, y: p1.y + uy * t / 2.0 } } else { at };
        // No chord through the stretch ends on anything standing: it is open
        // to the air past the blank, no width bounds a tool there, and it is
        // said at its middle. (Until 2026-11-09 it read 0.000mm wide, named
        // whatever stood next-nearest, and with one thing standing was
        // dropped as a corner: the unbounded chord of ROADMAP.md's debts.)
        if !width.is_finite() {
            let n = cells.len() as f64;
            at = Pt { x: cells.iter().map(|k| centre(k % nx, k / nx).x).sum::<f64>() / n, y: cells.iter().map(|k| centre(k % nx, k / nx).y).sum::<f64>() / n };
        }
        // A CORNER is not a stretch: where the two nearest standing things at
        // its widest point are one island's own concave vertex, or two wall
        // edges that meet, the floor there is the inside corner no round tool
        // reaches — nobody's to cut, as it has been since Phase 30. A stretch
        // is between two things that stand APART.
        // The island arm reads the island's own edges (Phase 47 slice 1,
        // found on the way): `standing` lists an island ONCE, so "the two
        // nearest things are the same island" could never be true, and a
        // sharp L-boss's notch beside a second boss was refused as a stretch
        // between the two. An inside corner is where the pinch point projects
        // onto the interiors of two adjacent edges of the nearest thing.
        let near: Vec<usize> = best.2.iter().take(2).map(|(_, k)| *k).collect();
        let corner = match near.as_slice() {
            [a, b] => {
                let (pa, pb) = (&standing[*a].1, &standing[*b].1);
                inside_corner_of(pa, at) || (pa.len() == 2 && pb.len() == 2 && pa.iter().any(|q| pb.iter().any(|w| (q.x - w.x).phypot(q.y - w.y) < 1e-6)))
            }
            // One thing stands and the chord exits through an open edge: the
            // stretch lies between that thing and air, a corner only in the
            // thing's own notch.
            [a] if open_hit => inside_corner_of(&standing[*a].1, at),
            _ => true,
        };
        if corner {
            reach_cells += cells.len();
            continue;
        }
        let mut between: Vec<String> = Vec::new();
        for (_, k) in best.2.iter().take(if open_hit { 1 } else { 2 }) {
            let name = standing[*k].0.clone();
            if !between.contains(&name) {
                between.push(name);
            }
        }
        if open_hit {
            between.push("the floor's open edge".to_string());
        }
        let outline = trace(&cells, nx, ny, h, Pt { x: x0, y: y0 }, &dist_to_allowed, r);
        stretches.push(Stretch { area_mm2: area, at: best.1, width, between, outline });
    }
    stretches.sort_by(|a, b| b.area_mm2.partial_cmp(&a.area_mm2).unwrap());
    Coverage { floor_mm2: floor_cells as f64 * h * h, reachable_mm2: reach_cells as f64 * h * h, stretches }
}

/// A stretch's OUTLINE: the curve where the distance to where the tool's
/// centre may be equals the tool's radius — the exact edge of what the tool
/// reaches — traced by marching squares over the stretch's cells grown by two,
/// with the field forced outside at the edge of that mask (so the curve closes
/// through standing material, which is standing either way). Linear
/// interpolation on a 0.2mm grid of a distance field: the edge is exact to
/// about a micron, so the entry check never sees the tool touch it, and the
/// stock model never claims material the tool did not reach.
fn trace(cells: &[usize], nx: usize, ny: usize, h: f64, origin: Pt, dist: &dyn Fn(Pt) -> f64, r: f64) -> Vec<Pt> {
    let mut mask = vec![false; nx * ny];
    for k in cells {
        let (i, j) = ((k % nx) as i64, (k / nx) as i64);
        for dj in -2..=2 {
            for di in -2..=2 {
                let (ii, jj) = (i + di, j + dj);
                if ii >= 0 && jj >= 0 && ii < nx as i64 && jj < ny as i64 {
                    mask[jj as usize * nx + ii as usize] = true;
                }
            }
        }
    }
    let masked = |i: i64, j: i64| i >= 0 && j >= 0 && i < nx as i64 && j < ny as i64 && mask[j as usize * nx + i as usize];
    let corner = |i: i64, j: i64| Pt { x: origin.x + i as f64 * h, y: origin.y + j as f64 * h };
    // The field at a corner: inside (positive) past the tool's reach, and
    // only where every cell round the corner is in the mask.
    let mut field: std::collections::HashMap<(i64, i64), f64> = Default::default();
    let mut value = |i: i64, j: i64| -> f64 {
        *field.entry((i, j)).or_insert_with(|| if masked(i - 1, j - 1) && masked(i, j - 1) && masked(i - 1, j) && masked(i, j) { dist(corner(i, j)) - r } else { -1.0 })
    };
    // Edges: (horizontal?, i, j) — a horizontal edge runs (i,j)->(i+1,j).
    type Edge = (bool, i64, i64);
    let mut links: std::collections::HashMap<Edge, Vec<Edge>> = Default::default();
    let mut at: std::collections::HashMap<Edge, Pt> = Default::default();
    let (mut i0, mut j0, mut i1, mut j1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for (k, m) in mask.iter().enumerate() {
        if *m {
            let (i, j) = ((k % nx) as i64, (k / nx) as i64);
            (i0, j0, i1, j1) = (i0.min(i), j0.min(j), i1.max(i), j1.max(j));
        }
    }
    for j in j0 - 1..=j1 + 1 {
        for i in i0 - 1..=i1 + 1 {
            let (a, b, c, d) = (value(i, j), value(i + 1, j), value(i + 1, j + 1), value(i, j + 1));
            let bottom = (true, i, j);
            let right = (false, i + 1, j);
            let top = (true, i, j + 1);
            let left = (false, i, j);
            let mut cross = |e: Edge, p: f64, q: f64, from: Pt, to: Pt| {
                let t = p / (p - q);
                at.entry(e).or_insert(Pt { x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t });
            };
            let (pa, pb, pc, pd) = (corner(i, j), corner(i + 1, j), corner(i + 1, j + 1), corner(i, j + 1));
            let mut hits: Vec<Edge> = Vec::new();
            if (a > 0.0) != (b > 0.0) {
                cross(bottom, a, b, pa, pb);
                hits.push(bottom);
            }
            if (b > 0.0) != (c > 0.0) {
                cross(right, b, c, pb, pc);
                hits.push(right);
            }
            if (c > 0.0) != (d > 0.0) {
                cross(top, c, d, pc, pd);
                hits.push(top);
            }
            if (d > 0.0) != (a > 0.0) {
                cross(left, d, a, pd, pa);
                hits.push(left);
            }
            let pairs: Vec<(Edge, Edge)> = match hits.len() {
                2 => vec![(hits[0], hits[1])],
                4 => {
                    // The saddle: the centre decides which diagonal joins.
                    if ((a + b + c + d) / 4.0 > 0.0) == (a > 0.0) {
                        vec![(bottom, right), (top, left)]
                    } else {
                        vec![(bottom, left), (top, right)]
                    }
                }
                _ => Vec::new(),
            };
            for (e, f) in pairs {
                links.entry(e).or_default().push(f);
                links.entry(f).or_default().push(e);
            }
        }
    }
    // Chain the edges into loops; the largest is the outline.
    let mut used: std::collections::HashSet<Edge> = Default::default();
    let mut best: Vec<Pt> = Vec::new();
    let area = |l: &[Pt]| (0..l.len()).map(|k| { let (a, b) = (l[k], l[(k + 1) % l.len()]); a.x * b.y - b.x * a.y }).sum::<f64>().abs() / 2.0;
    let starts: Vec<Edge> = links.keys().copied().collect();
    for start in starts {
        if used.contains(&start) {
            continue;
        }
        let mut lp = Vec::new();
        let (mut prev, mut here) = (None, start);
        loop {
            used.insert(here);
            lp.push(at[&here]);
            let next = links[&here].iter().copied().find(|e| Some(*e) != prev && !used.contains(e));
            match next {
                Some(e) => (prev, here) = (Some(here), e),
                None => break,
            }
        }
        if lp.len() >= 3 && area(&lp) > area(&best) {
            best = lp;
        }
    }
    best
}

/// Cells of one grid row (their centres `xs`, at height `y`) inside ANY of
/// `loops`, each loop by its own even-odd crossing count — `point_in_polygon`'s
/// answer at every centre.
pub(crate) fn row_inside(loops: &[Vec<Pt>], y: f64, xs: &[f64]) -> Vec<bool> {
    let mut out = vec![false; xs.len()];
    for l in loops {
        let mut cross: Vec<f64> = Vec::new();
        for k in 0..l.len() {
            let (a, b) = (l[k], l[(k + 1) % l.len()]);
            if (a.y > y) != (b.y > y) {
                cross.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        cross.sort_by(|a, b| a.partial_cmp(b).unwrap());
        for pair in cross.chunks_exact(2) {
            let (lo, hi) = (pair[0], pair[1]);
            // [lo, hi): `point_in_polygon`'s own convention for a centre that
            // lies exactly on an edge (a 0.2mm grid meets x=90.000 exactly).
            let first = xs.partition_point(|x| *x < lo);
            for (i, x) in xs.iter().enumerate().skip(first) {
                if *x >= hi {
                    break;
                }
                out[i] = true;
            }
        }
    }
    out
}

pub(crate) fn and_not(a: &[bool], b: &[bool]) -> Vec<bool> {
    a.iter().zip(b).map(|(x, y)| *x && !*y).collect()
}

/// Segments bucketed on a square grid of side `size`, for "is any segment
/// within `d` of this point?" with `d` ≤ `size`: only the 3x3 buckets round
/// the point can hold one.
pub(crate) struct Buckets<'a> {
    size: f64,
    map: std::collections::HashMap<(i64, i64), Vec<&'a (Pt, Pt)>>,
}

impl<'a> Buckets<'a> {
    pub(crate) fn of(segs: &'a [(Pt, Pt)], size: f64) -> Buckets<'a> {
        let mut map: std::collections::HashMap<(i64, i64), Vec<&(Pt, Pt)>> = Default::default();
        for s in segs {
            let (x0, x1) = (s.0.x.min(s.1.x), s.0.x.max(s.1.x));
            let (y0, y1) = (s.0.y.min(s.1.y), s.0.y.max(s.1.y));
            for j in (y0 / size).floor() as i64..=(y1 / size).floor() as i64 {
                for i in (x0 / size).floor() as i64..=(x1 / size).floor() as i64 {
                    map.entry((i, j)).or_default().push(s);
                }
            }
        }
        Buckets { size, map }
    }

    pub(crate) fn any_within(&self, q: Pt, d: f64) -> bool {
        let (ci, cj) = ((q.x / self.size).floor() as i64, (q.y / self.size).floor() as i64);
        (cj - 1..=cj + 1).any(|j| (ci - 1..=ci + 1).any(|i| self.map.get(&(i, j)).is_some_and(|v| v.iter().any(|(a, b)| dist_to_polygon(&[*a, *b], q.x, q.y) <= d))))
    }
}

/// The point of polyline-or-polygon `pts` nearest `q` (two points are a
/// segment; more are a closed loop).
fn nearest_on(pts: &[Pt], q: Pt) -> Pt {
    let n = pts.len();
    let edges = if n == 2 { 1 } else { n };
    let mut best = (f64::INFINITY, pts[0]);
    for e in 0..edges {
        let (a, b) = (pts[e], pts[(e + 1) % n]);
        let (ex, ey) = (b.x - a.x, b.y - a.y);
        let t = (((q.x - a.x) * ex + (q.y - a.y) * ey) / (ex * ex + ey * ey).max(1e-18)).clamp(0.0, 1.0);
        let p = Pt { x: a.x + ex * t, y: a.y + ey * t };
        let d = (p.x - q.x).phypot(p.y - q.y);
        if d < best.0 {
            best = (d, p);
        }
    }
    best.1
}

/// Is `q` in the inside corner of this loop — the floor's angle under 180°
/// at one of the loop's own vertices, where no round tool reaches? True when
/// the loop's two nearest edges to `q` are adjacent and meet at a vertex
/// where the loop turns INWARD (its own interior angle over 180°: an L's
/// notch), read against its winding so either winding answers alike. A
/// point beside a convex vertex is never in a corner of the floor, and a
/// two-point wall edge has no corner of its own (the wall arm pairs two).
/// Nearest EDGES, not a projection test: the pinch point is a cell centre
/// and lands on a leg's own line as readily as beside it.
fn inside_corner_of(pts: &[Pt], q: Pt) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut edges: Vec<(f64, usize)> = (0..n)
        .map(|e| {
            let (a, b) = (pts[e], pts[(e + 1) % n]);
            let (ex, ey) = (b.x - a.x, b.y - a.y);
            let t = (((q.x - a.x) * ex + (q.y - a.y) * ey) / (ex * ex + ey * ey).max(1e-18)).clamp(0.0, 1.0);
            (((a.x + ex * t) - q.x).phypot((a.y + ey * t) - q.y), e)
        })
        .collect();
    edges.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    let ((_, e1), (_, e2)) = (edges[0], edges[1]);
    let v = if (e1 + 1) % n == e2 {
        e2
    } else if (e2 + 1) % n == e1 {
        e1
    } else {
        return false;
    };
    let winding: f64 = (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum();
    let (p, c, nx) = (pts[(v + n - 1) % n], pts[v], pts[(v + 1) % n]);
    let turn = (c.x - p.x) * (nx.y - c.y) - (c.y - p.y) * (nx.x - c.x);
    turn * winding < 0.0
}

fn island_pts(i: &crate::offset::Island) -> Vec<Pt> {
    match i {
        crate::offset::Island::Circle { x, y, r } => crate::ops::finish::circle_pts(*x, *y, *r).into_iter().map(|(x, y)| Pt { x, y }).collect(),
        crate::offset::Island::Loop(pts) => pts.clone(),
    }
}

/// One stretch, as the refusal and the program say it.
pub fn stretch_line(s: &Stretch) -> String {
    let f = crate::gcode::f;
    let between = match s.between.len() {
        0 => String::new(),
        1 => format!(" beside {}", s.between[0]),
        _ => format!(" between {} and {}", quote(&s.between[0]), quote(&s.between[1])),
    };
    if !s.width.is_finite() {
        return format!("{:.1}mm²{} near ({}, {}), open to the air past the blank — no width bounds a tool there", s.area_mm2, between, f(s.at.x), f(s.at.y));
    }
    format!(
        "{:.1}mm²{} near ({}, {}), {}mm wide — a tool up to {}mm would reach it",
        s.area_mm2,
        between,
        f(s.at.x),
        f(s.at.y),
        f(s.width),
        f((s.width * 10.0).floor() / 10.0)
    )
}

fn quote(name: &str) -> String {
    if name.starts_with("the ") {
        name.to_string()
    } else {
        format!("'{name}'")
    }
}

/// What comes next for a floor this op cannot finish (Phase 33 slice 2, fork
/// 5): a rest `Floor`, when the floor is one the rest floor is built for,
/// and the largest tool that reaches EVERY stretch — the narrowest one's
/// width, rounded down to a tenth as each stretch's own line rounds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Next {
    /// The skeleton to paste — `template::rest_line`.
    pub line: String,
    pub up_to: f64,
    pub stretches: usize,
    /// The op already in the program that takes it, when there is one.
    pub taken_by: Option<(usize, usize)>,
}

pub fn next_step(p: &crate::parse::Part, op: &Operation, c: &Coverage) -> Option<Next> {
    use crate::parse::Feature;
    if op.op_type != crate::parse::OpType::End || c.stretches.is_empty() {
        return None;
    }
    if !matches!(p.features.get(&op.feature_name), Some(Feature::Face { .. } | Feature::Plane { .. })) {
        return None;
    }
    // A stretch open to the air bounds no tool; infinite when none does.
    let up_to = c.stretches.iter().map(|s| (s.width * 10.0).floor() / 10.0).fold(f64::INFINITY, f64::min);
    // The NEXT op on this floor, if it is a rest op: that is who takes it.
    let idx = crate::rest::index_of(p, op)?;
    let taken_by = p.operations[idx + 1..]
        .iter()
        .enumerate()
        .find(|(_, o)| o.feature_name == op.feature_name && crate::region::of(p, o).is_some())
        .filter(|(_, o)| crate::rest::is_rest(o))
        .map(|(k, o)| (idx + 1 + k, o.tool_num));
    Some(Next { line: crate::template::rest_line(&op.feature_name), up_to, stretches: c.stretches.len(), taken_by })
}

impl Next {
    fn reach(&self) -> String {
        if !self.up_to.is_finite() {
            return format!("no stretch bounds the tool that takes {}", if self.stretches == 1 { "it" } else { "them" });
        }
        match self.stretches {
            1 => format!("a tool up to {}mm reaches it", crate::gcode::f(self.up_to)),
            2 => format!("a tool up to {}mm reaches both", crate::gcode::f(self.up_to)),
            n => format!("a tool up to {}mm reaches all {n}", crate::gcode::f(self.up_to)),
        }
    }

    /// The refusal's last clause.
    fn refusal_clause(&self) -> String {
        format!(", then take what it leaves with `{}` — {}", self.line, self.reach())
    }

    /// The program's comment line: no parentheses inside, a G-code comment
    /// cannot hold them.
    pub fn note(&self) -> String {
        match self.taken_by {
            Some((k, t)) => format!("(the rest: op {} takes it with T{t})", k + 1),
            None => format!("(the rest: an End with strategy=rest takes it — {})", self.reach()),
        }
    }
}

/// The refusal, actionable: every stretch, the share that is reachable, and
/// the one word of program text that would emit the rest.
pub fn refusal(op: &Operation, tool_dia: f64, c: &Coverage, next: Option<&Next>) -> String {
    let lines: Vec<String> = c.stretches.iter().map(stretch_line).collect();
    format!(
        "{} on '{}' with T{} ({}mm) cannot reach {} stretch{} of its floor, {:.1}mm² of {:.1}mm² ({:.1}% reachable): {}. To cut what it can reach and leave those, add cover={} to its run line{}",
        crate::stock::op_word(op),
        op.feature_name,
        op.tool_num,
        crate::gcode::f(tool_dia),
        c.stretches.len(),
        if c.stretches.len() == 1 { "" } else { "es" },
        c.floor_mm2 - c.reachable_mm2,
        c.floor_mm2,
        c.reachable_pct(),
        lines.join("; "),
        c.concession(),
        next.map(Next::refusal_clause).unwrap_or_default()
    )
}

/// The program's note under a concession, and the check that the concession
/// still holds: `cover=` asked for at least this much, and the floor gives it.
pub fn concession_note(op: &Operation, asked: f64, c: &Coverage) -> Result<String, String> {
    if c.reachable_pct() + 1e-9 < asked {
        return Err(format!(
            "{} on '{}': cover={} was conceded, but only {:.1}% of the floor is reachable now — {}. Concede cover={} if that is intended",
            crate::stock::op_word(op),
            op.feature_name,
            asked,
            c.reachable_pct(),
            c.stretches.iter().map(stretch_line).collect::<Vec<_>>().join("; "),
            c.concession()
        ));
    }
    Ok(format!(
        "(unreachable: {} stretch{}, {:.1}mm² of {:.1}mm² — {:.1}% covered, cover={} conceded; {})",
        c.stretches.len(),
        if c.stretches.len() == 1 { "" } else { "es" },
        c.floor_mm2 - c.reachable_mm2,
        c.floor_mm2,
        c.reachable_pct(),
        asked,
        c.stretches.iter().map(stretch_line).collect::<Vec<_>>().join("; ")
    ))
}
