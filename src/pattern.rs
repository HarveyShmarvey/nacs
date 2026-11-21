use crate::math::Portable as _;
// ---- patterns: how a region is walked ---------------------------------------
// Phase 28 slice 3 (`DECISIONS.md` → "Phase 28 slice 3: the patterns…"). The
// offset family walks a region in loops parallel to every wall. This is the
// other native answer: PARALLEL PASSES, clipped to where the tool's centre may
// be, either alternating and joined along the region's boundary (`zigzag`) or
// all cut one way with a lift between (`oneway`), and closed by one pass along
// every wall — because a parallel pass ends against a wall and leaves a
// scallop there.
//
// Pure geometry, std only: it is handed the ALLOWED region — the offset
// family's outermost level, which already knows every wall, island, keep-out
// and open edge — and hands back chains of points. It knows nothing of G-code.

use crate::parse::Pt;
use crate::stock::dist_to_polygon;

/// One continuous cut at depth. `ramp` says its first stroke stands in solid
/// stock with no cut air beside it: the emitter descends along that stroke
/// instead of plunging at its start.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub ramp: bool,
    /// The start stands in air by the plan's own reading — past the blank,
    /// or in a given void's lens. A chain that plunges without it stands
    /// beside a stroke already cut; the emitter's sentence says which.
    pub in_air: bool,
    pub pts: Vec<Pt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The passes' direction, degrees CCW from +X, in [0, 180) — chosen by
    /// the emitter from the same candidates spine offers, by the same price
    /// (Phase 38 slice 0); 0 is `along X`, 90 `along Y`.
    pub angle_deg: f64,
    pub passes: usize,
    /// The spacing actually used: `stepover` as a MAXIMUM, so the passes land
    /// exactly on both sides of the region.
    pub step: f64,
    /// Whether every row climbs — the uncut floor on the tool's right under
    /// M03 — for `oneway`, whose rows all run the same way; `None` for the
    /// zigzag, whose rows alternate and have no one direction.
    pub rows_climb: Option<bool>,
    pub chains: Vec<Chain>,
}

/// How far a scan line is nudged off the region's own extreme, where it would
/// otherwise lie ALONG a boundary edge and cross nothing. A tenth of a micron:
/// the program's words carry three decimals.
pub(crate) const SCAN_NUDGE: f64 = 1e-4;

/// A link between passes may follow the boundary this far, in stepovers, before
/// a lift is the shorter honest move.
pub(crate) const LINK_STEPS: f64 = 4.0;

/// The scan frame: passes run along +u. Spine's frame since Phase 30, and
/// the one home for it since Phase 38 slice 0 — the parallel passes and
/// spine's rows turn the floor the same way, so a direction means one thing.
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    c: f64,
    s: f64,
}

impl Frame {
    pub(crate) fn new(angle_deg: f64) -> Frame {
        let a = angle_deg.to_radians();
        // Exactly axis-aligned at 0 and 90, so an X-running spine's words are the
        // zigzag's words and not 1e-17 off them.
        let snap = |v: f64| if v.abs() < 1e-12 { 0.0 } else if (v.abs() - 1.0).abs() < 1e-12 { v.signum() } else { v };
        Frame { c: snap(a.pcos()), s: snap(a.psin()) }
    }
    pub(crate) fn to(&self, q: Pt) -> Pt {
        Pt { x: q.x * self.c + q.y * self.s, y: -q.x * self.s + q.y * self.c }
    }
    pub(crate) fn from(&self, q: Pt) -> Pt {
        Pt { x: q.x * self.c - q.y * self.s, y: q.x * self.s + q.y * self.c }
    }
}

pub(crate) fn len(a: Pt, b: Pt) -> f64 {
    (b.x - a.x).phypot(b.y - a.y)
}

pub(crate) fn path_len(pts: &[Pt]) -> f64 {
    pts.windows(2).map(|w| len(w[0], w[1])).sum()
}

/// What a plan costs, in millimetres of cutting: the chains' length, a
/// ramped chain's first stroke counted again (the emitter descends along it
/// and then cuts it flat — spine makes its ramp that stroke, the parallel
/// passes ramp down their whole first one), and one lift per chain at
/// `lift_cost`. Spine's price since Phase 30, and since Phase 38 slice 0 the
/// ONE price every pattern that chooses anything chooses by: zigzag and
/// oneway pick their direction with it, spine its comb. Length and lifts,
/// measured on the plan itself and on nothing else — what the tool meets is
/// measured afterwards, from the moves, and never priced here
/// (`DECISIONS.md` → "Phase 38 slice 0…").
pub fn price(chains: &[Chain], lift_cost: f64) -> f64 {
    chains.iter().map(|c| path_len(&c.pts) + if c.ramp && c.pts.len() >= 2 { len(c.pts[0], c.pts[1]) } else { 0.0 }).sum::<f64>() + lift_cost * chains.len() as f64
}

/// A point this close to a loop is ON it (the program's words carry three
/// decimals; the offsetter's own vertices are exact).
pub(crate) const ON_LOOP: f64 = 1e-3;

/// Where the line `v = const` crosses into and out of the loops: sorted
/// `(u0, u1)` intervals, even-odd over every loop (holes are loops too). The
/// one scan line every parallel pass in the engine is cut from (Phase 35
/// slice 0) — the patterns here and spine's rows both.
pub(crate) fn spans(loops: &[Vec<Pt>], v: f64) -> Vec<(f64, f64)> {
    let mut xs: Vec<f64> = Vec::new();
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            if (a.y > v) != (b.y > v) {
                xs.push(a.x + (v - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// The edge of loop `l` that `q` lies on, if it lies on one.
pub(crate) fn locate(l: &[Pt], q: Pt) -> Option<usize> {
    // A point at a vertex lies on two edges: the first from the loop's start.
    crate::tie::first_least((0..l.len()).map(|i| (dist_to_polygon(&[l[i], l[(i + 1) % l.len()]], q.x, q.y), i)), |e| e.0)
        .filter(|(d, _)| *d < ON_LOOP)
        .map(|(_, i)| i)
}

/// The shorter way round a boundary loop from `a` to `b`, both ON it: its
/// length, and the points strictly between them — or `None` if either is not
/// on this loop. (Both on one edge is the straight line between them: the
/// whole loop the wrong way round is not a link.)
pub(crate) fn along_boundary(l: &[Pt], a: Pt, b: Pt) -> Option<(f64, Vec<Pt>)> {
    let (ia, ib, n) = (locate(l, a)?, locate(l, b)?, l.len());
    let total = |via: &[Pt]| -> f64 {
        let mut len = 0.0;
        let mut prev = a;
        for q in via.iter().chain(std::iter::once(&b)) {
            len += (q.x - prev.x).phypot(q.y - prev.y);
            prev = *q;
        }
        len
    };
    if ia == ib {
        return Some((total(&[]), Vec::new()));
    }
    let fwd: Vec<Pt> = (1..=(ib + n - ia) % n).map(|k| l[(ia + k) % n]).collect();
    let back: Vec<Pt> = (0..(ia + n - ib) % n).map(|k| l[(ia + n - k) % n]).collect();
    let (f, r) = (total(&fwd), total(&back));
    // Two ways round of one length: the loop's own direction.
    Some(if crate::tie::within(f, r) { (f, fwd) } else { (r, back) })
}

/// A pass shorter than this is not a pass: two crossings of one scan line a
/// micron apart are a vertex the line grazed.
const SLIVER: f64 = 1e-6;

/// Parallel passes over the allowed region, running at `angle_deg`. `ccw`
/// and `cw` are the offset family's outermost level (wall-parallel loops and
/// island-parallel ones); `in_air` says whether the whole tool clears the
/// blank at a point, which is the one place a plunge needs nothing cut
/// beside it. One direction, planned; the emitter asks for each candidate
/// and keeps the cheapest by `price`.
pub fn plan(zigzag: bool, angle_deg: f64, ccw: &[Vec<Pt>], cw: &[Vec<Pt>], stepover: f64, in_air: &dyn Fn(Pt) -> bool, climb: Option<bool>) -> Option<Plan> {
    // Work in a frame where passes run along +u; the plan is the same
    // geometry turned on the way in and turned back on the way out.
    let fr = Frame::new(angle_deg);
    let loops: Vec<Vec<Pt>> = ccw.iter().chain(cw).map(|l| l.iter().map(|q| fr.to(*q)).collect()).collect();
    let (lo, hi) = loops.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.y), hi.max(q.y)));
    if !lo.is_finite() || stepover <= 0.0 {
        return None;
    }
    let span = (hi - lo - 2.0 * SCAN_NUDGE).max(0.0);
    let n = (span / stepover).ceil() as usize;
    let step = if n == 0 { 0.0 } else { span / n as f64 };
    let unframe = |q: Pt| fr.from(q);

    let mut chains: Vec<Chain> = Vec::new();
    let mut cut: Vec<(Pt, Pt)> = Vec::new(); // strokes already cut, in the working frame
    let mut open: Option<Vec<Pt>> = None; // the chain being grown, working frame
    let beside_cut = |q: Pt, cut: &[(Pt, Pt)]| cut.iter().any(|(a, b)| dist_to_polygon(&[*a, *b], q.x, q.y) <= stepover + 1e-3);

    // Start on the side that is in AIR, where there is one: a step is walked
    // from its open edge toward its wall, so the wall gets the LAST pass — a
    // stepover wide — and never the first, a full slot at depth. (Both sides in
    // air, or neither: the low side first, as given.)
    // A row is in air when the MIDDLE of one of its spans is: its ends may hang
    // off the blank's far sides whichever row it is.
    let row_in_air = |i: usize| {
        let v = lo + SCAN_NUDGE + step * i as f64;
        spans(&loops, v).iter().filter(|(a, b)| b - a > SLIVER).any(|(u0, u1)| in_air(unframe(Pt { x: (u0 + u1) / 2.0, y: v })))
    };
    let from_far_side = !row_in_air(0) && row_in_air(n);
    // The first stroke runs FROM the end that is in air when only its far
    // end is (Phase 44 slice 3): a given void's lens at a row's end is an
    // entry at depth with no ramp, as the blank's air is. One direction
    // choice for the whole plan — the zigzag's parity turns over and every
    // oneway stroke turns with it — so the serpentine and the one-way rule
    // both hold; a later chain's own start is judged as before.
    let first = {
        let i = if from_far_side { n } else { 0 };
        let v = lo + SCAN_NUDGE + step * i as f64;
        spans(&loops, v).into_iter().find(|(a, b)| b - a > SLIVER).map(|(u0, u1)| (unframe(Pt { x: u0, y: v }), unframe(Pt { x: u1, y: v })))
    };
    // In the frame the rows run along +u and the uncut rows lie toward the
    // side they step to: +v (the left of +u) stepping up, −v stepping down.
    // Which side the uncut floor stands on is therefore the flip against the
    // stepping side — and `direction=` on the one-way rows (ROADMAP item 2,
    // slice 2) sets the flip instead of the air rule, so every stroke runs
    // with the uncut floor on the side asked.
    let flip = match climb {
        Some(climb) if !zigzag => from_far_side != climb,
        _ => first.is_some_and(|(a, b)| !in_air(a) && in_air(b)),
    };
    let rows_climb = (!zigzag).then_some(flip != from_far_side);
    for k in 0..=n {
        let i = if from_far_side { n - k } else { k };
        let v = lo + SCAN_NUDGE + step * i as f64;
        let mut row: Vec<(f64, f64)> = spans(&loops, v).into_iter().filter(|(a, b)| b - a > SLIVER).collect();
        let leftward = (zigzag && k % 2 == 1) ^ flip;
        if leftward {
            row.reverse();
        }
        for (u0, u1) in row {
            let (a, b) = if leftward { (Pt { x: u1, y: v }, Pt { x: u0, y: v }) } else { (Pt { x: u0, y: v }, Pt { x: u1, y: v }) };
            // Join to the chain in hand by following the boundary, when the
            // way round is short; otherwise close it and start another.
            let joined = match (&mut open, zigzag) {
                (Some(chain), true) => {
                    let from = *chain.last().unwrap();
                    // Tied boundaries: the first loop as listed.
                    let link = crate::tie::first_least(loops.iter().filter_map(|l| along_boundary(l, from, a)), |x| x.0);
                    match link {
                        Some((len, via)) if crate::tie::within(len, LINK_STEPS * stepover.max(step)) => {
                            chain.extend(via);
                            chain.push(a);
                            chain.push(b);
                            true
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
            if !joined {
                if let Some(done) = open.take() {
                    chains.push(Chain { ramp: false, in_air: false, pts: done });
                }
                let in_air_here = in_air(unframe(a));
                let start_open = in_air_here || beside_cut(a, &cut);
                // `ramp` rides on the chain we are about to open; stash it by
                // pushing a marker chain now and growing it in place.
                chains.push(Chain { ramp: !start_open, in_air: in_air_here, pts: Vec::new() });
                open = Some(vec![a, b]);
            }
            cut.push((a, b));
        }
    }
    if let Some(done) = open.take() {
        chains.push(Chain { ramp: false, in_air: false, pts: done });
    }
    // Fold each marker (an empty chain carrying the entry) into the chain that
    // follows it.
    let mut folded: Vec<Chain> = Vec::new();
    let mut pending: Option<(bool, bool)> = None;
    for c in chains {
        if c.pts.is_empty() {
            pending = Some((c.ramp, c.in_air));
        } else {
            let (ramp, in_air) = pending.take().unwrap_or((false, false));
            folded.push(Chain { ramp, in_air, pts: c.pts.into_iter().map(unframe).collect() });
        }
    }
    Some(Plan { angle_deg, passes: n + 1, step, rows_climb, chains: folded })
}
