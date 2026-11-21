use crate::math::Portable as _;
// ---- spine: the pattern born between planing and the offset family -------------
// Phase 30 (`DECISIONS.md` → "Phase 30: mow…", its name until Phase 35). PLANING is parallel passes and
// knows nothing of walls; the OFFSET family is wall-parallel loops and drags
// every wall's shape across the whole floor. A real floor is mixed — open here,
// walled there — and spine gives each stretch of boundary the pass it wants:
//
//   ROWS    straight parallel passes, each cut only where it OWNS floor;
//   SHORE   one pass along what STANDS, and only along what stands;
//   LINKS   whichever of "along the boundary", "straight across" and "lift"
//           is shortest.
//
// With nothing standing there is no shore and spine is planing. Std only, pure
// geometry: it is handed where the tool's centre may be, the floor, and what
// stands, and hands back chains of points. It knows nothing of G-code.

use crate::compare::LOAD_AGREE;
use crate::engagement::{Bounds, Walk};
use crate::parse::Pt;
use crate::pattern::{len, path_len, price, spans, Chain, Frame, LINK_STEPS, ON_LOOP, SCAN_NUDGE};
use crate::stock::{dist_to_polygon, point_in_polygon};

pub struct Ask<'a> {
    /// Where the tool's centre may be: the offset family's outermost level.
    pub ccw: &'a [Vec<Pt>],
    pub cw: &'a [Vec<Pt>],
    /// The floor itself — outer loop first, then everything in it that is not
    /// floor (even-odd).
    pub floor: &'a [Vec<Pt>],
    /// What STANDS beside the floor, as segments: its wall edges, its islands.
    pub walls: &'a [(Pt, Pt)],
    pub r: f64,
    /// How far the allowed region stands off a wall: `r` plus any allowance.
    pub stand_off: f64,
    pub stepover: f64,
    /// What one lift costs, in millimetres of cutting — a link longer than
    /// this is a lift.
    pub lift_cost: f64,
    /// How long a ramp into solid stock is: the depth of the cut over the
    /// tangent of the ramp's slope. A ramped chain descends along the first
    /// `ramp_len` of its own first stroke, backwards, and then cuts it forwards.
    pub ramp_len: f64,
    /// The whole tool clears the blank here: a plunge needs nothing beside it.
    /// `Sync` because the comb's candidates are planned on every core
    /// (Phase 49 slice 2) and each asks it.
    pub in_air: &'a (dyn Fn(Pt) -> bool + Sync),
    /// The floor eroded by `level_d` from every edge at once — the HAIRLINE
    /// (Phase 31): where the floor is a rail, the level comes back as a pair
    /// of loops a hair apart, and their midline is the rail's own centreline.
    /// Empty where no rail is narrow enough to want one.
    pub level: &'a [Vec<Pt>],
    pub level_d: f64,
    /// The rows' direction pinned by `angle=` (ROADMAP item 2, slice 3):
    /// the comb then chooses only its phase and its way, and no pair of
    /// combs is tried, since a cross row runs at right angles to the
    /// direction the person set. `None` and the comb chooses the direction
    /// too.
    pub angle: Option<f64>,
    /// HELD under a ceiling on engagement (Phase 62 slice 1); `None` for
    /// spine itself.
    pub held: Option<Held<'a>>,
    /// `direction=conventional` (ROADMAP item 2, slice 2): every shore pass
    /// runs the other way round, against the loop's own winding.
    pub conventional: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The rows' direction, degrees CCW from +X, in [0, 180).
    pub angle_deg: f64,
    pub rows: usize,
    /// Rows of the comb at right angles, where the plan is a pair (slice 4).
    pub cross_rows: usize,
    /// Passes along a rail's centreline (Phase 31), and their length.
    pub rails: usize,
    pub rail_mm: f64,
    pub shores: usize,
    /// The shore passes' own length, leads included.
    pub shore_mm: f64,
    pub chains: Vec<Chain>,
    /// Cutting length plus `lift_cost` per lift: what the comb was chosen by.
    pub cost: f64,
    /// What the held read found (Phase 62 slice 1); `None` for spine.
    pub held: Option<HeldRead>,
}

/// A plan HELD under a ceiling on engagement (Phase 62 slice 1;
/// `DECISIONS.md` → "Phase 62 slice 1…"). The comb is walked from the air
/// (`ways`); every LINK is read through the walk as it is laid, over
/// everything laid before it, and a link that would exceed the ceiling is
/// not a link — the lift is; once the plan stands it is read back and the
/// places over the ceiling are said by what carried them. `walk` is `None`
/// where the stock model declines before the op or holds a footprint the
/// walk cannot solve: the spacing alone holds then, and the read says so.
pub struct Held<'a> {
    /// The ceiling, in radians.
    pub ceiling: f64,
    /// The stock the op began with, as the walk reads it: its bounds, and
    /// whether a point is in material.
    pub walk: Option<(&'a Bounds, &'a (dyn Fn(Pt) -> bool + Sync))>,
}

/// The held read of a plan: whether the walk was read at all, how many
/// links were refused for a lift, and the millimetres over the ceiling by
/// what carried them — in `HELD_KINDS`' order, a kind with none left out.
#[derive(Clone, Debug, PartialEq)]
pub struct HeldRead {
    pub read: bool,
    pub lifts: usize,
    pub over: Vec<(&'static str, f64)>,
}

/// What a segment of a held plan is, as its read names it.
pub const HELD_KINDS: [&str; 5] = ["the first stroke", "the rails", "the rows", "the shores", "the links"];
const FIRST: &str = HELD_KINDS[0];
const RAILS: &str = HELD_KINDS[1];
const ROWS: &str = HELD_KINDS[2];
const SHORES: &str = HELD_KINDS[3];
const LINKS: &str = HELD_KINDS[4];

/// How much further than `stand_off` a stretch of the allowed boundary may be
/// from a wall and still be running ALONG it: the flattened arcs of an offset
/// loop sit a chord's sagitta off the true offset.
const ALONG_WALL: f64 = 0.05;

/// The boundary is classified wall / not-wall at points this far apart, so a
/// shore pass begins within this of where its wall does.
const SHORE_SAMPLE: f64 = 0.5;


const BAND_HAIR: f64 = 1e-6;

/// The hairline's two loops sit `2 * level_d` apart across a rail exactly as
/// wide as they were made for, a hair more where the rail is wider; a stretch
/// whose partner is further than a single pass covers — twice the tool's
/// radius less the level's depth on each side — is an EDGE of something wide,
/// and belongs to the rows. `RAIL_SAMPLE` is how finely the level is walked,
/// `RAIL_SAME` how near a centreline point must be to one already kept to be
/// the same rail seen from its other side, `RAIL_SLACK` how much of the tool's
/// radius a rail pass is trusted to cover when rows are told what is done, and
/// `RAIL_STATION` how finely a row's owned run is asked.
const RAIL_SAMPLE: f64 = 0.5;
const RAIL_SAME: f64 = 0.4;
const RAIL_SLACK: f64 = 0.3;
const RAIL_STATION: f64 = 0.25;
/// A partner runs the other way: its tangent within sixty degrees of opposed.
const RAIL_OPPOSED: f64 = 0.5;

// (Between ROWS a link may run `LINK_STEPS` stepovers before a lift is the
// honest move — the zigzag's own number, from its home. Between SHORE passes
// the floor is already cut, and only the lift's cost bounds it.)

/// A row this close to a shore pass, running along it, IS that shore pass.
const SAME_PASS: f64 = 0.02;

type Span = (f64, f64);

// (The scan frame — rows run along +u — is `pattern::Frame` since Phase 38
// slice 0: one frame for spine's rows and the parallel passes.)

fn merge(mut v: Vec<Span>, gap: f64) -> Vec<Span> {
    v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut out: Vec<Span> = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 + gap => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// The u-extent of the floor inside the band `lo..hi` of v — EXACTLY: a
/// connected piece of floor-in-band projects onto the same interval its
/// boundary does, and its boundary is floor edges clipped to the band plus the
/// floor's spans along the band's two lines.
fn owned(floor: &[Vec<Pt>], lo: f64, hi: f64) -> Vec<Span> {
    let mut out = spans(floor, lo);
    out.extend(spans(floor, hi));
    for l in floor {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            if a.y.max(b.y) < lo || a.y.min(b.y) > hi {
                continue;
            }
            let at = |v: f64| if (b.y - a.y).abs() < 1e-12 { a.x } else { a.x + (v.clamp(a.y.min(b.y), a.y.max(b.y)) - a.y) / (b.y - a.y) * (b.x - a.x) };
            let (u0, u1) = if (b.y - a.y).abs() < 1e-12 { (a.x, b.x) } else { (at(lo), at(hi)) };
            out.push((u0.min(u1), u0.max(u1)));
        }
    }
    merge(out, 0.0)
}

/// One row pass, in the PRIMARY comb's frame: a cross comb's strokes are
/// turned into it, where they run along v.
#[derive(Clone, Copy, Debug)]
struct Stroke {
    row: usize,
    a: Pt,
    b: Pt,
}

/// A rectangle of the scan frame, `(u-range, v-range)`: floor one comb hands
/// to the other.
type Window = (Span, Span);

/// The rows, in cutting order. Row `k` OWNS the band of floor its advancing
/// edge uncovers — the top `stepover` of its swath, going up — and the bands
/// tile the floor exactly, so every point of floor has one row responsible for
/// it and a row is cut only where it owns something (run a radius past each
/// end of that, which is the tool leaving the floor it owns). It follows that
/// no row ever takes more than a stepover of new material: the first row to
/// touch a stretch of floor is the one that owns its edge.
///
/// Two things make a comb one of a PAIR (slice 4). With `hand_over`, an owned
/// run shorter than the tool is wide — a TOOTH: the row crossing a rail at
/// right angles — is not cut; it is returned as a window instead, for the comb
/// at right angles, along which that rail is long. With `only`, the comb owns
/// nothing but the floor inside those windows. Every point of floor still has
/// exactly one row of one comb responsible for it.
fn rows(ask: &Ask, allowed: &[Vec<Pt>], floor: &[Vec<Pt>], shores: &[Vec<Pt>], done: Option<&dyn Fn(Pt) -> bool>, phase: f64, descending: bool, hand_over: bool, only: Option<&[Window]>) -> (Vec<Stroke>, Vec<Window>) {
    let (lo, hi) = allowed.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.y), hi.max(q.y)));
    let s = ask.stepover;
    let (k0, k1) = (((lo - phase) / s).ceil() as i64, ((hi - phase) / s).floor() as i64);
    let mut ks: Vec<i64> = (k0..=k1).collect();
    if descending {
        ks.reverse();
    }
    let (mut out, mut teeth) = (Vec::new(), Vec::new());
    for (row, k) in ks.into_iter().enumerate() {
        let v = phase + k as f64 * s;
        // (Open at both ends by a hair: a floor whose edge lies exactly ON a
        // band's line is owned by the band it has area in, not by both.)
        let band = if descending { (v - ask.r + BAND_HAIR, v - ask.r + s - BAND_HAIR) } else { (v + ask.r - s + BAND_HAIR, v + ask.r - BAND_HAIR) };
        let mut mine: Vec<Span> = match only {
            None => owned(floor, band.0, band.1),
            Some(windows) => merge(
                windows
                    .iter()
                    .filter_map(|((u0, u1), (v0, v1))| {
                        let (lo, hi) = (band.0.max(*v0), band.1.min(*v1));
                        (hi - lo > 2.0 * BAND_HAIR).then(|| owned(floor, lo, hi).into_iter().map(|(a, b)| (a.max(*u0), b.min(*u1))).filter(|(a, b)| b > a).collect::<Vec<_>>())
                    })
                    .flatten()
                    .collect(),
                0.0,
            ),
        };
        // Floor the rail passes already cover is not a row's to own: each
        // owned run is asked at stations along it, every point of floor across
        // the band at each, and the covered stations dropped. (The mask can
        // only under-claim, so this can only leave a row MORE floor.)
        if let Some(done) = done {
            let across = (ask.stepover / COVER_CELL).ceil().max(2.0) as usize;
            let covered = |u: f64| (0..=across).all(|k| {
                let v = band.0 + (band.1 - band.0) * k as f64 / across as f64;
                let q = Pt { x: u, y: v };
                // (The mask first: it is a lookup, and the floor is hundreds of
                // edges.)
                done(q) || !inside(floor, q)
            });
            let mut split: Vec<Span> = Vec::new();
            for (a, b) in mine {
                let n = ((b - a) / RAIL_STATION).ceil().max(1.0) as usize;
                let mut open: Option<f64> = None;
                for k in 0..=n {
                    let u = a + (b - a) * k as f64 / n as f64;
                    match (covered(u), open) {
                        (false, None) => open = Some(u),
                        (true, Some(u0)) => {
                            split.push((u0, u));
                            open = None;
                        }
                        _ => {}
                    }
                }
                if let Some(u0) = open {
                    split.push((u0, b));
                }
            }
            mine = split.into_iter().filter(|(a, b)| b - a > RAIL_STATION).collect();
        }
        if hand_over {
            let (short, long): (Vec<Span>, Vec<Span>) = mine.into_iter().partition(|(a, b)| b - a < 2.0 * ask.r);
            teeth.extend(short.into_iter().map(|span| (span, band)));
            mine = long;
        }
        let need: Vec<Span> = mine.into_iter().map(|(a, b)| (a - ask.r, b + ask.r)).collect();
        // Two stretches of one row closer than a lift costs are one stretch.
        let need = merge(need, ask.lift_cost);
        // (A stretch shorter than the tool's radius is not a pass: it is the
        // allowed region pinched against a wall, and what it would reach is
        // the shore's.)
        // Where a shore pass runs along this very line, the row is the shore's
        // — cut once, last, by the pass that finishes the wall.
        let taken: Vec<Span> = shores
            .iter()
            .flat_map(|sh| sh.windows(2))
            .filter(|w| (w[0].y - v).abs() < SAME_PASS && (w[1].y - v).abs() < SAME_PASS)
            .map(|w| (w[0].x.min(w[1].x), w[0].x.max(w[1].x)))
            .collect();
        let taken = merge(taken, SAME_PASS);
        for (a0, a1) in spans(allowed, v) {
            for (n0, n1) in &need {
                let mut pieces = vec![(a0.max(*n0), a1.min(*n1))];
                for (t0, t1) in &taken {
                    pieces = pieces.into_iter().flat_map(|(u0, u1)| [(u0, u1.min(*t0)), (u0.max(*t1), u1)]).filter(|(u0, u1)| u1 > u0).collect();
                }
                // (…unless rail passes have taken the rest of the row: what
                // they leave beside a junction is short and is floor.)
                let least = if done.is_none() { ask.r } else { RAIL_STATION };
                out.extend(pieces.into_iter().filter(|(u0, u1)| u1 - u0 > least).map(|(u0, u1)| Stroke { row, a: Pt { x: u0, y: v }, b: Pt { x: u1, y: v } }));
            }
        }
    }
    (out, teeth)
}

fn crosses(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let side = |p: Pt, q: Pt, r: Pt| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0
}

fn inside(loops: &[Vec<Pt>], q: Pt) -> bool {
    loops.iter().filter(|l| point_in_polygon(l, q.x, q.y)).count() % 2 == 1
}

/// From `a` to `b` along loop `l`, both on it, the shorter way round: the
/// points strictly between.
fn along(l: &[Pt], a: Pt, b: Pt) -> Option<Vec<Pt>> {
    crate::pattern::along_boundary(l, a, b).map(|(_, via)| via)
}

/// Whether the straight move from `a` to `b` keeps the tool's centre where it
/// may be: it crosses no boundary, and nine points along it are inside the
/// allowed region or on its edge (a move that slips out through a VERTEX
/// crosses nothing properly, and is caught here).
fn straight(allowed: &[Vec<Pt>], a: Pt, b: Pt) -> bool {
    let crossing = allowed.iter().any(|l| (0..l.len()).any(|i| crosses(a, b, l[i], l[(i + 1) % l.len()])));
    let may_be = |q: Pt| inside(allowed, q) || allowed.iter().any(|l| dist_to_polygon(l, q.x, q.y) < ON_LOOP);
    !crossing && (1..10).all(|k| may_be(Pt { x: a.x + (b.x - a.x) * k as f64 / 10.0, y: a.y + (b.y - a.y) * k as f64 / 10.0 }))
}

/// The shortest cutting move from `a` to `b` that keeps the tool's centre
/// where it may be: straight across, or along the boundary.
fn link(allowed: &[Vec<Pt>], a: Pt, b: Pt) -> Option<(f64, Vec<Pt>)> {
    let across = straight(allowed, a, b).then(|| (len(a, b), Vec::new()));
    let round = allowed.iter().filter_map(|l| along(l, a, b)).map(|via| (path_len(&[&[a], &via[..], &[b]].concat()), via));
    // Tied ways: straight across, then the boundaries as listed.
    crate::tie::first_least(across.into_iter().chain(round), |x| x.0)
}

/// The stretches of the allowed boundary that run ALONG something standing, in
/// the loop's own direction (CCW round the outside, CW round an island: climb
/// milling both ways, as the closing loops always were). A loop that stands
/// all the way round comes back closed (first point repeated last).
pub fn shores(ask: &Ask) -> Vec<Vec<Pt>> {
    let allowed: Vec<Vec<Pt>> = ask.ccw.iter().chain(ask.cw).cloned().collect();
    let (allowed, walls) = (&allowed[..], ask.walls);
    // ALONG a wall means beside its length, or round a corner where two walls
    // meet. Past the END of a wall chain the boundary is someone else's — air,
    // an open edge — and a wall-following pass must never wrap round it: the
    // bare end of a chain therefore counts for nothing.
    let joined = |q: Pt| walls.iter().filter(|(a, b)| len(*a, q) < 1e-6 || len(*b, q) < 1e-6).count() >= 2;
    let by_wall = |q: Pt| {
        walls.iter().any(|(a, b)| {
            let (ex, ey) = (b.x - a.x, b.y - a.y);
            let t = ((q.x - a.x) * ex + (q.y - a.y) * ey) / (ex * ex + ey * ey).max(1e-18);
            let near = if t < 0.0 { *a } else if t > 1.0 { *b } else { Pt { x: a.x + t * ex, y: a.y + t * ey } };
            len(near, q) <= ask.stand_off + ALONG_WALL && ((0.0..=1.0).contains(&t) || joined(near))
        })
    };
    // …and ALONG A FLOOR. A wall is a region whose one edge touches this floor
    // and whose others do not: the allowed boundary wraps it all the way
    // round, and the far side has no floor beside it to clean. A shore pass
    // there would cut scrap for the length of the wall (Phase 32 slice 4).
    // (Whether a shore beside a rail the rails already cut is REDUNDANT was
    // tried and reverted: dropping those runs bought 8s on the walled plate
    // and left 2.1mm² of floor uncut — the `uncut` column caught it. A shore
    // is cheap and a missed sliver is not; the redundancy stands, named.)
    let by_floor = |q: Pt| {
        [0.0, 0.25, 0.5, 0.75, 1.0].iter().any(|t| {
            let d = ask.r * t;
            [(d, 0.0), (-d, 0.0), (0.0, d), (0.0, -d), (d * 0.7, d * 0.7), (-d * 0.7, d * 0.7), (d * 0.7, -d * 0.7), (-d * 0.7, -d * 0.7)]
                .iter()
                .any(|(dx, dy)| inside(ask.floor, Pt { x: q.x + dx, y: q.y + dy }))
        })
    };
    let by_wall = |q: Pt| by_wall(q) && by_floor(q);
    let mut out = Vec::new();
    for l in allowed {
        // The loop, resampled: (point, is it one of the loop's own vertices).
        let mut pts: Vec<(Pt, bool)> = Vec::new();
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let n = (len(a, b) / SHORE_SAMPLE).ceil().max(1.0) as usize;
            pts.extend((0..n).map(|k| (Pt { x: a.x + (b.x - a.x) * k as f64 / n as f64, y: a.y + (b.y - a.y) * k as f64 / n as f64 }, k == 0)));
        }
        let flags: Vec<bool> = pts.iter().map(|(q, _)| by_wall(*q)).collect();
        let n = pts.len();
        if flags.iter().all(|f| *f) {
            let mut lp = l.clone();
            lp.push(l[0]);
            out.push(lp);
            continue;
        }
        // Where the wall begins or ends between two samples, to a micron.
        let edge_of = |on: Pt, off: Pt| {
            let (mut on, mut off) = (on, off);
            for _ in 0..24 {
                let mid = Pt { x: (on.x + off.x) / 2.0, y: (on.y + off.y) / 2.0 };
                if by_wall(mid) { on = mid } else { off = mid }
            }
            on
        };
        // Begin at a not-wall sample so no run wraps the seam.
        let Some(start) = flags.iter().position(|f| !*f) else { continue };
        let mut run: Vec<Pt> = Vec::new();
        for k in 1..=n {
            let (i, before) = ((start + k) % n, (start + k - 1) % n);
            if flags[i] {
                if run.is_empty() {
                    run.push(edge_of(pts[i].0, pts[before].0));
                }
                if pts[i].1 {
                    run.push(pts[i].0);
                }
            } else if !run.is_empty() {
                run.push(edge_of(pts[before].0, pts[i].0));
                if path_len(&run) > ask.r {
                    out.push(led(allowed, dedup(std::mem::take(&mut run)), ask.r));
                }
                run.clear();
            }
        }
    }
    // Conventional: each shore the other way round. A closed shore keeps its
    // first point, which is also its last; an open one starts at its far end.
    if ask.conventional {
        for sh in &mut out {
            sh.reverse();
        }
    }
    out
}

/// A shore pass with a straight lead on each end, a radius long where the
/// allowed region has room for it (half that, or none, where it has not): the
/// pass enters and leaves PAST the wall's end, so neither the plunge nor the
/// lift leaves its witness on the wall.
fn led(allowed: &[Vec<Pt>], mut run: Vec<Pt>, r: f64) -> Vec<Pt> {
    // Straight on past `from`, away from `toward`.
    let reach = |from: Pt, toward: Pt| {
        let d = len(from, toward).max(1e-12);
        [r, r / 2.0].into_iter().map(|k| Pt { x: from.x + (from.x - toward.x) / d * k, y: from.y + (from.y - toward.y) / d * k }).find(|q| straight(allowed, from, *q))
    };
    if run.len() >= 2 {
        if let Some(q) = reach(run[0], run[1]) {
            run.insert(0, q);
        }
        let n = run.len();
        if let Some(q) = reach(run[n - 1], run[n - 2]) {
            run.push(q);
        }
    }
    run
}

/// What the rail passes have covered, as a mask (Phase 31): every point within
/// `r − RAIL_SLACK` of a pass, on a grid of `COVER_CELL`, stamped once in the
/// world frame and read from every comb. A cell counts as covered only when
/// its CENTRE is — so the mask can only under-claim, and a row can only be
/// left more floor, never less.
pub struct Cover {
    x0: f64,
    y0: f64,
    nx: usize,
    ny: usize,
    bits: Vec<bool>,
}

const COVER_CELL: f64 = 0.25;

impl Cover {
    pub fn of(rails: &[Vec<Pt>], r: f64) -> Cover {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for q in rails.iter().flatten() {
            (x0, y0, x1, y1) = (x0.min(q.x - r), y0.min(q.y - r), x1.max(q.x + r), y1.max(q.y + r));
        }
        if !x0.is_finite() {
            return Cover { x0: 0.0, y0: 0.0, nx: 0, ny: 0, bits: Vec::new() };
        }
        let (nx, ny) = (((x1 - x0) / COVER_CELL).ceil() as usize + 1, ((y1 - y0) / COVER_CELL).ceil() as usize + 1);
        let mut bits = vec![false; nx * ny];
        let reach = r - RAIL_SLACK;
        let cells = (reach / COVER_CELL).ceil() as i64;
        for rl in rails {
            for w in rl.windows(2) {
                let n = (len(w[0], w[1]) / (COVER_CELL / 2.0)).ceil().max(1.0) as usize;
                for k in 0..=n {
                    let t = k as f64 / n as f64;
                    let q = Pt { x: w[0].x + (w[1].x - w[0].x) * t, y: w[0].y + (w[1].y - w[0].y) * t };
                    let (ci, cj) = (((q.x - x0) / COVER_CELL) as i64, ((q.y - y0) / COVER_CELL) as i64);
                    for j in (cj - cells).max(0)..=(cj + cells).min(ny as i64 - 1) {
                        for i in (ci - cells).max(0)..=(ci + cells).min(nx as i64 - 1) {
                            let idx = j as usize * nx + i as usize;
                            if !bits[idx] {
                                let c = Pt { x: x0 + (i as f64 + 0.5) * COVER_CELL, y: y0 + (j as f64 + 0.5) * COVER_CELL };
                                if len(c, q) <= reach {
                                    bits[idx] = true;
                                }
                            }
                        }
                    }
                }
            }
        }
        Cover { x0, y0, nx, ny, bits }
    }

    pub fn at(&self, q: Pt) -> bool {
        if self.nx == 0 {
            return false;
        }
        let (i, j) = (((q.x - self.x0) / COVER_CELL).floor(), ((q.y - self.y0) / COVER_CELL).floor());
        if i < 0.0 || j < 0.0 || i >= self.nx as f64 || j >= self.ny as f64 {
            return false;
        }
        self.bits[j as usize * self.nx + i as usize]
    }
}

/// A set of passes that are polylines — rail passes, shore passes — walked
/// nearest first, each joined to the chain in hand where a link is cheaper
/// than a lift. An open pass may be cut from either end (`either_way`); a
/// closed one may begin anywhere.
#[allow(clippy::too_many_arguments)]
fn polylines(
    ask: &Ask,
    air: &dyn Fn(Pt) -> bool,
    allowed: &[Vec<Pt>],
    mut set: Vec<Vec<Pt>>,
    kind: &'static str,
    holds: &dyn Fn(&[(bool, Vec<Pt>)], Pt, &[Pt], Pt) -> bool,
    chains: &mut Vec<(bool, Vec<Pt>)>,
    kinds: &mut Vec<Vec<&'static str>>,
    airs: &mut Vec<bool>,
    cut: &mut Vec<(Pt, Pt)>,
    lifts_held: &mut usize,
    either_way: bool,
) {
    while !set.is_empty() {
        let from = chains.last().and_then(|(_, pts)| pts.last().copied());
        // (start index or, for an open pass, usize::MAX for "reversed"; distance)
        let start_of = |sh: &Vec<Pt>, from: Option<Pt>| -> (usize, f64) {
            let closed = sh.len() > 2 && len(sh[0], *sh.last().unwrap()) < ON_LOOP;
            match (closed, from) {
                // Tied vertices: the first from the pass's own start.
                (true, Some(f)) => crate::tie::first_least((0..sh.len() - 1).map(|i| (i, len(f, sh[i]))), |x| x.1).unwrap(),
                (false, Some(f)) if either_way && crate::tie::clearly_less(len(f, *sh.last().unwrap()), len(f, sh[0])) => (usize::MAX, len(f, *sh.last().unwrap())),
                (_, Some(f)) => (0, len(f, sh[0])),
                _ => (0, 0.0),
            }
        };
        // Tied passes: the first as the set lists them.
        let (i, (k, _)) = crate::tie::first_least(set.iter().enumerate().map(|(i, sh)| (i, start_of(sh, from))), |x| x.1 .1).unwrap();
        let mut sh = set.remove(i);
        if k == usize::MAX {
            sh.reverse();
        } else if k > 0 {
            sh.pop();
            sh.rotate_left(k);
            sh.push(sh[0]);
        }
        let joined = from.and_then(|f| link(allowed, f, sh[0])).filter(|(d, _)| crate::tie::within(*d, ask.lift_cost));
        // Under a ceiling a link that would exceed it is not a link (Phase
        // 62 slice 1): the pass is lifted to instead.
        let joined = match joined {
            Some((d, via)) if holds(chains, from.expect("a link has a chain in hand"), &via, sh[0]) => Some((d, via)),
            Some(_) => {
                *lifts_held += 1;
                None
            }
            None => None,
        };
        match joined {
            Some((_, via)) => {
                let chain = &mut chains.last_mut().unwrap().1;
                let ks = kinds.last_mut().unwrap();
                ks.extend(std::iter::repeat_n(LINKS, via.len() + 1));
                chain.extend(via);
                chain.extend(sh.iter().copied());
                ks.extend(std::iter::repeat_n(kind, sh.len() - 1));
            }
            None => {
                let in_air = air(sh[0]);
                chains.push((!(in_air || beside(cut, sh[0], ask.stepover)), sh.clone()));
                kinds.push(vec![kind; sh.len() - 1]);
                airs.push(in_air);
            }
        }
        cut.extend(sh.windows(2).map(|w| (w[0], w[1])));
    }
}

/// The rail passes (Phase 31): one pass down the centreline of every stretch
/// of floor a single pass covers. The floor's hairline level is walked; each
/// sample's PARTNER is the nearest point of the level that is not its own
/// neighbourhood along the loop; where the partner is within reach the
/// midpoint is on the centreline, and runs of such midpoints are the passes —
/// each rail found from both its sides and kept once. A pass whose end is a
/// dead end runs a radius past it, as a row runs past what it owns; at a
/// junction it stops, the crossing rail's pass covering the rest.
pub fn rails(ask: &Ask) -> Vec<Vec<Pt>> {
    let allowed: Vec<Vec<Pt>> = ask.ccw.iter().chain(ask.cw).cloned().collect();
    let (allowed, floor) = (&allowed[..], ask.floor);
    let level = ask.level;
    if level.is_empty() {
        return Vec::new();
    }
    // Every level segment.
    let segs: Vec<(usize, f64, Pt, Pt)> = level.iter().enumerate().flat_map(|(li, l)| (0..l.len()).map(move |i| (li, 0.0, l[i], l[(i + 1) % l.len()]))).collect();
    let reach = 2.0 * (ask.r - ask.level_d) - RAIL_SLACK;
    let mut kept: Vec<Vec<Pt>> = Vec::new();
    for l in level {
        let mut run: Vec<Pt> = Vec::new();
        // A run shorter than the tool's radius is the apex of a wide region's
        // rounded end, not a rail: the rows own it.
        let close = |run: &mut Vec<Pt>, kept: &mut Vec<Vec<Pt>>| {
            if path_len(run) >= ask.r {
                kept.push(std::mem::take(run));
            }
            run.clear();
        };
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let n = (len(a, b) / RAIL_SAMPLE).ceil().max(1.0) as usize;
            let (tx, ty) = ((b.x - a.x) / len(a, b).max(1e-9), (b.y - a.y) / len(a, b).max(1e-9));
            for k in 0..n {
                let t = k as f64 / n as f64;
                let q = Pt { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
                // The partner is the nearest point of the level on a stretch
                // running the OTHER way: the two sides of a rail are traversed
                // in opposite directions, across a dead end's tip included,
                // while a wide region's own corner, just round the bend, runs
                // at right angles and is nobody's partner.
                // Tied partners: the first segment as listed.
                let facing_points = segs
                    .iter()
                    .filter_map(|(_, _, c, d)| {
                        let (ex, ey) = (d.x - c.x, d.y - c.y);
                        let seg = (ex * ex + ey * ey).max(1e-18);
                        let facing = (ex * tx + ey * ty) / seg.sqrt() <= -RAIL_OPPOSED;
                        let tt = (((q.x - c.x) * ex + (q.y - c.y) * ey) / seg).clamp(0.0, 1.0);
                        facing.then_some(Pt { x: c.x + ex * tt, y: c.y + ey * tt })
                    });
                let partner = crate::tie::first_least(facing_points, |x| len(q, *x));
                let centre = partner.filter(|p| len(q, *p) <= reach).map(|p| Pt { x: (q.x + p.x) / 2.0, y: (q.y + p.y) / 2.0 });
                let centre = centre.filter(|c| inside(allowed, *c) && !kept.iter().any(|rl| dist_to_polygon(rl, c.x, c.y) < RAIL_SAME));
                match centre {
                    Some(c) => run.push(c),
                    None => close(&mut run, &mut kept),
                }
            }
        }
        close(&mut run, &mut kept);
    }
    // Stitch runs whose ends meet (the loop's seam), then run each dead end
    // out a radius.
    let mut out: Vec<Vec<Pt>> = Vec::new();
    for mut rl in kept {
        if let Some(prev) = out.iter_mut().find(|o| len(*o.last().unwrap(), rl[0]) < RAIL_SAMPLE) {
            prev.extend(rl.drain(..));
            continue;
        }
        out.push(rl);
    }
    for rl in out.iter_mut() {
        let n = rl.len();
        if n < 2 {
            continue;
        }
        let dead = |end: Pt, before: Pt| {
            let d = len(end, before).max(1e-9);
            let ahead = Pt { x: end.x + (end.x - before.x) / d * ask.r / 2.0, y: end.y + (end.y - before.y) / d * ask.r / 2.0 };
            !inside(floor, ahead)
        };
        let extend = |end: Pt, before: Pt| {
            let d = len(end, before).max(1e-9);
            Pt { x: end.x + (end.x - before.x) / d * ask.r, y: end.y + (end.y - before.y) / d * ask.r }
        };
        if dead(rl[n - 1], rl[n - 2]) && straight(allowed, rl[n - 1], extend(rl[n - 1], rl[n - 2])) {
            let q = extend(rl[n - 1], rl[n - 2]);
            rl.push(q);
        }
        if dead(rl[0], rl[1]) && straight(allowed, rl[0], extend(rl[0], rl[1])) {
            let q = extend(rl[0], rl[1]);
            rl.insert(0, q);
        }
    }
    out.into_iter().filter(|rl| rl.len() >= 2).collect()
}

fn beside(cut: &[(Pt, Pt)], q: Pt, within: f64) -> bool {
    cut.iter().any(|(a, b)| dist_to_polygon(&[*a, *b], q.x, q.y) <= within + ON_LOOP)
}

/// The rows and shores as chains, in the scan frame. Greedy, and deliberately
/// so: from where the tool is, take the cheapest LINK to a stroke of the next
/// row up (or this one); when there is none, lift to the lowest stroke left —
/// so a floor that splits (a jaw, an island's two sides) is walked one side at
/// a time, bottom to top, and each stroke still has its lower neighbour cut.
fn walk(ask: &Ask, frame: Frame, allowed: &[Vec<Pt>], rails: Vec<Vec<Pt>>, strokes: Vec<Stroke>, shores: Vec<Vec<Pt>>) -> (Vec<Chain>, f64, Option<HeldRead>) {
    let air = |q: Pt| (ask.in_air)(frame.from(q));
    let mut left = strokes;
    let mut cut: Vec<(Pt, Pt)> = Vec::new();
    let mut chains: Vec<(bool, Vec<Pt>)> = Vec::new();
    // What carried each segment of each chain — one word per segment,
    // parallel to `chains` — for the held read (Phase 62 slice 1).
    let mut kinds: Vec<Vec<&'static str>> = Vec::new();
    // Whether each chain's start stands in `air` — parallel to `chains`, the
    // fact the emitter's plunge sentence reports.
    let mut airs: Vec<bool> = Vec::new();
    let mut lifts_held = 0usize;
    // Whether a link laid NOW — from the chain in hand, through `via`, to
    // `to` — holds under the ceiling (Phase 62 slice 1): each of its
    // segments read through the walk over everything laid before it, at
    // the report's own stations, against the report's own line. Always,
    // for spine itself and where the walk declined.
    let holds = |chains: &[(bool, Vec<Pt>)], from: Pt, via: &[Pt], to: Pt| -> bool {
        let Some(held) = &ask.held else { return true };
        let Some((bounds, material)) = held.walk else { return true };
        let mut moves = world_segments(chains, frame);
        let n0 = moves.len();
        let pts: Vec<Pt> = std::iter::once(from).chain(via.iter().copied()).chain(std::iter::once(to)).map(|q| frame.from(q)).collect();
        moves.extend(pts.windows(2).filter(|w| len(w[0], w[1]) > 1e-9).map(|w| (w[0], w[1])));
        let walk = Walk::of(&moves, ask.r);
        let material = |q: (f64, f64)| material(Pt { x: q.0, y: q.1 });
        (n0..moves.len()).all(|k| stations(moves[k]).all(|t| walk.engagement(k, t, bounds, &material) <= held.ceiling + LOAD_AGREE))
    };
    let mut at_row = 0usize; // the row of the stroke last cut
    let ends = |s: &Stroke, flip: bool| if flip { (s.b, s.a) } else { (s.a, s.b) };
    // The rail passes FIRST: they are most of the work on a floor that has
    // them, and the rows fill in what is wide.
    polylines(ask, &air, allowed, rails, RAILS, &holds, &mut chains, &mut kinds, &mut airs, &mut cut, &mut lifts_held, true);
    let mut first_row = true;
    while !left.is_empty() {
        let here = chains.last().and_then(|(_, pts)| pts.last().copied());
        // The links from the chain in hand: to the same row or the next,
        // never dearer than the lift it would save — the cheapest taken, as
        // ever; under a ceiling the cheapest that HOLDS (the sort is stable,
        // so a tie still goes to the earlier candidate and the goldens
        // stand).
        let mut cands: Vec<(f64, usize, bool, Vec<Pt>)> = Vec::new();
        if let Some(from) = here {
            for (i, s) in left.iter().enumerate() {
                if s.row > at_row + 1 {
                    continue;
                }
                for flip in [false, true] {
                    if let Some((d, via)) = link(allowed, from, ends(s, flip).0) {
                        if crate::tie::within(d, ask.lift_cost.min(LINK_STEPS * ask.stepover)) {
                            cands.push((d, i, flip, via));
                        }
                    }
                }
            }
        }
        // The shortest link that holds. Tied links: the first as gathered,
        // which is stroke order and each stroke's own direction first.
        let had = !cands.is_empty();
        let mut best = None;
        while let Some(k) = crate::tie::first_least(0..cands.len(), |k| cands[*k].0) {
            let c = cands.remove(k);
            if holds(&chains, here.expect("a candidate link has a chain in hand"), &c.3, ends(&left[c.1], c.2).0) {
                best = Some(c);
                break;
            }
        }
        let kind = if first_row { FIRST } else { ROWS };
        match best {
            Some((_, i, flip, via)) => {
                let s = left.remove(i);
                at_row = s.row;
                let (a, b) = ends(&s, flip);
                let chain = &mut chains.last_mut().unwrap().1;
                let ks = kinds.last_mut().unwrap();
                ks.extend(std::iter::repeat_n(LINKS, via.len() + 1));
                chain.extend(via);
                chain.push(a);
                chain.push(b);
                ks.push(kind);
                cut.push((a, b));
            }
            None => {
                if had {
                    lifts_held += 1;
                }
                // A lift, to the lowest stroke left — entered from AIR where
                // either end is in it, else from beside something cut.
                let row = left.iter().map(|s| s.row).min().unwrap();
                let i = left.iter().position(|s| s.row == row).unwrap();
                let s = left.remove(i);
                at_row = s.row;
                let open_at = |q: Pt| air(q) || beside(&cut, q, ask.stepover);
                let flip = !open_at(ends(&s, false).0) && open_at(ends(&s, true).0);
                let (a, b) = ends(&s, flip);
                chains.push((!open_at(a), vec![a, b]));
                kinds.push(vec![kind]);
                airs.push(air(a));
                cut.push((a, b));
            }
        }
        first_row = false;
    }
    // The shores, last — so a wall is met by a pass a scallop wide, never by a
    // slot.
    polylines(ask, &air, allowed, shores, SHORES, &holds, &mut chains, &mut kinds, &mut airs, &mut cut, &mut lifts_held, false);
    // A chain that RAMPS descends along the start of its own first stroke,
    // backwards, and then cuts it forwards: the emitter ramps from a chain's
    // second point to its first, so the ramp's far end is MADE the second
    // point — a ramp's length of stroke is paid for twice, not the whole pass.
    // A chain whose first segment is a CHORD — a rail pass, a shore — gets a
    // straight lead a ramp long, back along its first tangent, where the
    // centre may be there; half that where it may not; the chord otherwise.
    // (The kinds follow: a split or a lead is the first segment's kind.)
    for (c, (_, pts)) in chains.iter_mut().enumerate().filter(|(_, (ramp, pts))| *ramp && pts.len() >= 2) {
        let (a, b) = (pts[0], pts[1]);
        let first = len(a, b);
        if first > ask.ramp_len + 1e-6 {
            let t = ask.ramp_len / first;
            pts.insert(1, Pt { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });
            let k0 = kinds[c][0];
            kinds[c].insert(0, k0);
        } else if first < ask.ramp_len - 1e-6 {
            let back = |k: f64| Pt { x: a.x + (a.x - b.x) / first.max(1e-9) * k, y: a.y + (a.y - b.y) / first.max(1e-9) * k };
            if let Some(q) = [ask.ramp_len, ask.ramp_len / 2.0, ask.ramp_len / 4.0].into_iter().filter(|k| *k > first).map(back).find(|q| straight(allowed, a, *q)) {
                pts.insert(0, q);
                let k0 = kinds[c][0];
                kinds[c].insert(0, k0);
            }
        }
    }
    // The held read (Phase 62 slice 1), on the chains as they will be cut.
    let held = ask.held.as_ref().map(|h| read_back(h, &chains, &kinds, frame, ask.r, lifts_held));
    // Priced in the frame, before the points are turned back and deduped:
    // the one price (`pattern::price`), on exactly the chains it walked.
    let chains: Vec<Chain> = chains.into_iter().zip(airs).map(|((ramp, pts), in_air)| Chain { ramp, in_air, pts }).collect();
    let cost = price(&chains, ask.lift_cost);
    let chains = chains.into_iter().map(|c| Chain { ramp: c.ramp, in_air: c.in_air, pts: dedup(c.pts.into_iter().map(|q| frame.from(q)).collect()) }).collect();
    (chains, cost, held)
}

/// Every at-depth segment of the chains so far, in cutting order, in the
/// WORLD frame — the moves the walk reads.
fn world_segments(chains: &[(bool, Vec<Pt>)], frame: Frame) -> Vec<(Pt, Pt)> {
    chains
        .iter()
        .flat_map(|(_, pts)| pts.windows(2).map(|w| (frame.from(w[0]), frame.from(w[1]))).collect::<Vec<_>>())
        .filter(|(a, b)| len(*a, *b) > 1e-9)
        .collect()
}

/// The report's stations along a move (`compare::load_along`): one a
/// millimetre, at the middle of its millimetre.
fn stations((a, b): (Pt, Pt)) -> impl Iterator<Item = f64> {
    let steps = len(a, b).ceil().max(1.0) as usize;
    (0..steps).map(move |s| (s as f64 + 0.5) / steps as f64)
}

/// The plan read back through the walk (Phase 62 slice 1): every segment as
/// it will be cut, in order, at the report's stations; the length over the
/// ceiling put to what carried it.
fn read_back(held: &Held, chains: &[(bool, Vec<Pt>)], kinds: &[Vec<&'static str>], frame: Frame, r: f64, lifts: usize) -> HeldRead {
    let Some((bounds, material)) = held.walk else { return HeldRead { read: false, lifts, over: Vec::new() } };
    let (mut segs, mut tags) = (Vec::new(), Vec::new());
    for ((_, pts), ks) in chains.iter().zip(kinds) {
        debug_assert_eq!(pts.len().saturating_sub(1), ks.len(), "one kind per segment");
        for (w, k) in pts.windows(2).zip(ks) {
            let (a, b) = (frame.from(w[0]), frame.from(w[1]));
            if len(a, b) > 1e-9 {
                segs.push((a, b));
                tags.push(*k);
            }
        }
    }
    let walk = Walk::of(&segs, r);
    let material = |q: (f64, f64)| material(Pt { x: q.0, y: q.1 });
    let mut over: Vec<(&'static str, f64)> = HELD_KINDS.iter().map(|k| (*k, 0.0)).collect();
    for (k, seg) in segs.iter().enumerate() {
        let share = len(seg.0, seg.1) / stations(*seg).count() as f64;
        let slot = over.iter().position(|(name, _)| *name == tags[k]).expect("a kind from the list");
        for t in stations(*seg) {
            if walk.engagement(k, t, bounds, &material) > held.ceiling + LOAD_AGREE {
                over[slot].1 += share;
            }
        }
    }
    HeldRead { read: true, lifts, over: over.into_iter().filter(|(_, mm)| *mm > 0.0).collect() }
}

fn dedup(pts: Vec<Pt>) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::new();
    for q in pts {
        if out.last().is_none_or(|l| len(*l, q) > 1e-9) {
            out.push(q);
        }
    }
    out
}

/// One comb: rows at `angle_deg`, the first at `phase` (a fraction of the
/// stepover), walked up the frame or down it.
pub fn plan_at(ask: &Ask, angle_deg: f64, phase: f64, descending: bool) -> Option<Plan> {
    let (rail, shore) = (rails(ask), shores(ask));
    let cover = Cover::of(&[rail.clone(), shore.clone()].concat(), ask.r);
    plan_pair(ask, &rail, &shore, &cover, angle_deg, (phase, descending), None)
}

/// A PAIR of combs at right angles (slice 4): the primary hands its teeth to
/// the cross comb, which cuts only those.
pub fn plan_crossed(ask: &Ask, angle_deg: f64, primary: (f64, bool), cross: (f64, bool)) -> Option<Plan> {
    let (rail, shore) = (rails(ask), shores(ask));
    let cover = Cover::of(&[rail.clone(), shore.clone()].concat(), ask.r);
    plan_pair(ask, &rail, &shore, &cover, angle_deg, primary, Some(cross))
}

#[allow(clippy::too_many_arguments)]
fn plan_pair(ask: &Ask, rail_world: &[Vec<Pt>], shore_world: &[Vec<Pt>], cover: &Cover, angle_deg: f64, primary: (f64, bool), cross: Option<(f64, bool)>) -> Option<Plan> {
    let frame = Frame::new(angle_deg);
    let turn = |f: Frame, ls: &[Vec<Pt>]| ls.iter().map(|l| l.iter().map(|q| f.to(*q)).collect::<Vec<_>>()).collect::<Vec<_>>();
    let allowed: Vec<Vec<Pt>> = turn(frame, ask.ccw).into_iter().chain(turn(frame, ask.cw)).collect();
    let floor = turn(frame, ask.floor);
    let shore: Vec<Vec<Pt>> = turn(frame, shore_world);
    let rail: Vec<Vec<Pt>> = turn(frame, rail_world);
    // What the rails AND the shores cover is not a row's to own (slice 4): a
    // shore pass is a pass, and beside a curved wall it covers everything a
    // row would have cut there — the double cut this phase was opened on.
    let done = |q: Pt| cover.at(frame.from(q));
    let done: Option<&dyn Fn(Pt) -> bool> = (!rail.is_empty() || !shore.is_empty()).then_some(&done);
    let (mut strokes, teeth) = rows(ask, &allowed, &floor, &shore, done, primary.0 * ask.stepover, primary.1, cross.is_some(), None);
    let n_rows = strokes.iter().map(|s| s.row).collect::<std::collections::BTreeSet<_>>().len();
    let mut n_cross = 0;
    if let Some((phase, descending)) = cross {
        if teeth.is_empty() {
            return None; // nothing to hand over: this is the single comb, already priced
        }
        // The cross frame is this one turned a quarter: (u, v) -> (v, -u).
        let quarter = |q: Pt| Pt { x: q.y, y: -q.x };
        let back = |q: Pt| Pt { x: -q.y, y: q.x };
        let turned = |ls: &[Vec<Pt>]| ls.iter().map(|l| l.iter().map(|q| quarter(*q)).collect::<Vec<_>>()).collect::<Vec<_>>();
        let windows: Vec<Window> = teeth.iter().map(|((u0, u1), (v0, v1))| ((*v0, *v1), (-u1, -u0))).collect();
        let done_x = |q: Pt| cover.at(frame.from(back(q)));
        let done_x: Option<&dyn Fn(Pt) -> bool> = (!rail.is_empty()).then_some(&done_x);
        let (crossed, _) = rows(ask, &turned(&allowed), &turned(&floor), &turned(&shore), done_x, phase * ask.stepover, descending, false, Some(&windows));
        n_cross = crossed.iter().map(|s| s.row).collect::<std::collections::BTreeSet<_>>().len();
        // Numbered after the primary's rows: the walk finishes one comb, then
        // the other.
        let base = strokes.iter().map(|s| s.row + 2).max().unwrap_or(0);
        strokes.extend(crossed.into_iter().map(|s| Stroke { row: base + s.row, a: back(s.a), b: back(s.b) }));
    }
    if strokes.is_empty() && shore.is_empty() && rail.is_empty() {
        return None;
    }
    let (n_shores, shore_mm) = (shore.len(), shore.iter().map(|sh| path_len(sh)).sum());
    let (n_rails, rail_mm) = (rail.len(), rail.iter().map(|rl| path_len(rl)).sum());
    let (chains, cost, held) = walk(ask, frame, &allowed, rail, strokes, shore);
    Some(Plan { angle_deg, rows: n_rows, cross_rows: n_cross, rails: n_rails, rail_mm, shores: n_shores, shore_mm, chains, cost, held })
}

/// A pure function over an index range, run on every core and collected IN
/// ORDER (Phase 49 slice 2; `DECISIONS.md` → "Phase 49 slice 2…"): the comb's
/// candidates are independent, and the choice among them is a fold in the
/// order the one-thread walk took, so the winner — and every golden — is
/// byte-identical. `std::thread::scope`, no crate; one worker per core, each
/// taking the next index. A single index, or a single core, runs inline.
fn par_map<T: Send, F: Fn(usize) -> T + Sync>(n: usize, f: F) -> Vec<T> {
    let workers = std::thread::available_parallelism().map_or(1, |w| w.get()).min(n);
    if workers <= 1 {
        return (0..n).map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<std::sync::Mutex<Option<T>>> = (0..n).map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let out = f(i);
                *slots[i].lock().expect("a slot is never poisoned: the work happens before the lock") = Some(out);
            });
        }
    });
    slots.into_iter().map(|m| m.into_inner().expect("unpoisoned").expect("every index was planned")).collect()
}

/// The comb, CHOSEN: every candidate direction, a handful of phases, walked up
/// and down, each planned in full and the cheapest kept — cutting length plus
/// lifts, measured on the plan itself and on nothing else. The directions are
/// the ones a person would name: along X, along Y, and along the floor's own
/// longest edges.
///
/// Planned on every core since Phase 49 slice 2 — the single combs of every
/// direction at once, then the crossed pairs of every direction at once — and
/// FOLDED in exactly the order the one-thread walk took (per direction: the
/// cheapest single, kept; then each crossed pair, kept), with the same strict
/// `<` and the same margin, so a tie still goes to the earlier candidate and
/// the goldens do not move.
pub fn plan(ask: &Ask) -> Option<Plan> {
    let (rail, shore) = (rails(ask), shores(ask));
    let cover = Cover::of(&[rail.clone(), shore.clone()].concat(), ask.r);
    let cheaper = |p: &Plan, b: &Option<Plan>| b.as_ref().is_none_or(|b| crate::tie::clearly_less(p.cost, b.cost));
    // The single combs: every direction, every phase, both ways — or the
    // one direction the person set.
    let dirs = match ask.angle {
        Some(a) => vec![a],
        None => directions(ask.floor, 2.0 * ask.r),
    };
    let per_dir: Vec<Vec<(f64, bool)>> = dirs
        .iter()
        .map(|&a| {
            let ways = ways(ask, a);
            phases(ask, a).into_iter().flat_map(|ph| ways.iter().map(move |w| (ph, *w))).collect()
        })
        .collect();
    let flat: Vec<(f64, (f64, bool))> = dirs.iter().zip(&per_dir).flat_map(|(a, ps)| ps.iter().map(move |p| (*a, *p))).collect();
    let mut planned = par_map(flat.len(), |i| plan_pair(ask, &rail, &shore, &cover, flat[i].0, flat[i].1, None)).into_iter();
    let mut primaries: Vec<Option<(Plan, (f64, bool))>> = Vec::with_capacity(dirs.len());
    for ps in &per_dir {
        let mut single: Option<(Plan, (f64, bool))> = None;
        for p in ps {
            if let Some(pl) = planned.next().expect("one plan per candidate") {
                if single.as_ref().is_none_or(|(b, _)| crate::tie::clearly_less(pl.cost, b.cost)) {
                    single = Some((pl, *p));
                }
            }
        }
        primaries.push(single);
    }
    // The pairs: the best single of each direction as the primary, its teeth
    // handed to a comb at right angles, that comb's phase and way chosen too.
    // Never under a ceiling (Phase 62 slice 2): a tooth is floor the shores'
    // cover left a row, and the cross comb cuts it in windows BEFORE the
    // shores run — a cross row is a slot through uncut floor by construction
    // (on the post pocket 111 mm of slot against the single comb's 50, which
    // is the first stroke). The single comb cuts its teeth as short rows.
    let crosses: Vec<Vec<(f64, bool)>> = dirs.iter().zip(&primaries).map(|(&a, pr)| if pr.is_some() && ask.held.is_none() && ask.angle.is_none() { phases(ask, a + 90.0).into_iter().flat_map(|ph| [(ph, false), (ph, true)]).collect() } else { Vec::new() }).collect();
    let flat_x: Vec<(f64, (f64, bool), (f64, bool))> = dirs.iter().zip(&primaries).zip(&crosses).flat_map(|((a, pr), xs)| xs.iter().map(move |x| (*a, pr.as_ref().expect("a crossed list exists only under a primary").1, *x))).collect();
    let mut crossed = par_map(flat_x.len(), |i| plan_pair(ask, &rail, &shore, &cover, flat_x[i].0, flat_x[i].1, Some(flat_x[i].2))).into_iter();
    let mut best: Option<Plan> = None;
    for (pr, xs) in primaries.into_iter().zip(&crosses) {
        let Some((plan, _)) = pr else { continue };
        if cheaper(&plan, &best) {
            best = Some(plan);
        }
        for _ in xs {
            if let Some(pl) = crossed.next().expect("one plan per crossed candidate") {
                if cheaper(&pl, &best) {
                    best = Some(pl);
                }
            }
        }
    }
    best
}

/// Which ways a direction's comb may be walked: up the frame and down it —
/// or, HELD (Phase 62 slice 1), from the AIR alone where exactly one side of
/// the allowed region is in it, as the parallel passes' rows start
/// (`pattern::plan`): the first row then rides the open edge a spacing deep
/// and never cuts a slot through solid beside the wall it could have met
/// last. Both ways where both sides are in air, or neither (a closed floor's
/// first stroke is a slot, said by name).
fn ways(ask: &Ask, angle_deg: f64) -> Vec<bool> {
    if ask.held.is_none() {
        return vec![false, true];
    }
    let frame = Frame::new(angle_deg);
    let loops: Vec<Vec<Pt>> = ask.ccw.iter().chain(ask.cw).map(|l| l.iter().map(|q| frame.to(*q)).collect()).collect();
    let (lo, hi) = loops.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.y), hi.max(q.y)));
    let in_air = |v: f64| spans(&loops, v).iter().filter(|(a, b)| b - a > 1e-6).any(|(u0, u1)| (ask.in_air)(frame.from(Pt { x: (u0 + u1) / 2.0, y: v })));
    match (in_air(lo + SCAN_NUDGE), in_air(hi - SCAN_NUDGE)) {
        (true, false) => vec![false],
        (false, true) => vec![true],
        _ => vec![false, true],
    }
}

/// The phases tried in one direction, as fractions of the stepover: an even
/// handful, plus the two that ANCHOR the comb — a row exactly on the allowed
/// region's lowest line, or on its highest. Where that line is a wall's, the
/// anchored row is the shore pass itself, and the comb meets the wall in whole
/// stepovers with nothing left over.
fn phases(ask: &Ask, angle_deg: f64) -> Vec<f64> {
    let frame = Frame::new(angle_deg);
    let (lo, hi) = ask.ccw.iter().chain(ask.cw).flatten().map(|q| frame.to(*q).y).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let mut out: Vec<f64> = (0..PHASES).map(|k| k as f64 / PHASES as f64).collect();
    out.extend([lo, hi].into_iter().filter(|v| v.is_finite()).map(|v| (v / ask.stepover).rem_euclid(1.0)));
    out
}

/// How many phases of the comb are tried per direction.
const PHASES: usize = 4;

/// X, Y, and the directions of the floor's longest straight runs: edges at
/// least two tool diameters long (a chord of a flattened arc is not a
/// direction anyone would name), binned to the degree by length; the top three
/// that are not already X or Y.
pub fn directions(floor: &[Vec<Pt>], tool_dia: f64) -> Vec<f64> {
    let mut bins = [0.0f64; 180];
    for l in floor {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            if len(a, b) >= 2.0 * tool_dia {
                let deg = (b.y - a.y).patan2(b.x - a.x).to_degrees().rem_euclid(180.0);
                bins[(deg.round() as usize) % 180] += len(a, b);
            }
        }
    }
    let mut ranked: Vec<usize> = (0..180).filter(|d| *d != 0 && *d != 90 && bins[*d] > 0.0).collect();
    // The three longest. Tied directions: the smaller angle.
    let mut top: Vec<usize> = Vec::new();
    while top.len() < 3 {
        let Some(k) = crate::tie::first_greatest(0..ranked.len(), |k| bins[ranked[*k]]) else { break };
        top.push(ranked.remove(k));
    }
    [0.0, 90.0].into_iter().chain(top.into_iter().map(|d| d as f64)).collect()
}
