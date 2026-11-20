use crate::math::Portable as _;
// ---- the comparison report: the engine shows its options --------------------
// Phase 28 slice 4 (`DECISIONS.md` → "Phase 28 slice 4: the report…", fork 4A).
// For every op that works a region, each pattern valid on it — as a ROW OF
// THINGS ALREADY MEASURED, never a score: "fastest isn't always best, and the
// math can't always know." The engine shows; the author pins, with one word of
// program text the report prints ready to paste.
//
// Every number has an older home and is read from it: the clock
// (`cut_time::time_report`), the op-history stock (`stock::remaining_before`),
// the band (`parse::op_engagement`), the region (`region::of`). The report
// re-runs the real emitter once per candidate — a row is the program that
// pattern WOULD emit, judged, not a model of it.

use crate::parse::{Feature, Operation, Part, Pattern, Pt};
use crate::stock::point_in_polygon;

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    /// Every feed move of the op — the at-depth passes AND the entries that
    /// reach them (ramps, plunges, the descents). The clock's own number.
    pub cutting_mm: f64,
    /// The length cut AT DEPTH on the op's floor: the flat feed moves with
    /// the tool at `floor_z` before and after. Since Phase 39 slice 0 the ONE
    /// base every `%` in the row stands on — `on floor`, `other stock`,
    /// `air`, `past 90°` and `slot` are each a share of this — and the first
    /// column of the row, so the base is in sight. `cutting_mm` less this is
    /// the entries.
    pub at_depth_mm: f64,
    pub cutting_s: f64,
    pub rapid_mm: f64,
    /// Of the cutting length: with the tool over this op's own floor, over
    /// other stock (scrap beside a face, the blank beside a step), and over
    /// nothing at all. `None` where the stock model declines before this op.
    pub over: Option<(f64, f64, f64)>,
    pub plunges: usize,
    pub ramps: usize,
    pub lifts: usize,
    pub turns: usize,
    /// The band's worst engagement, where the band answers for this pattern.
    pub worst_deg: Option<f64>,
    /// How many passes cross each of the region's narrow lands, in their order.
    pub land_passes: Vec<usize>,
    /// Floor a tool this size could have reached, still holding stock when the
    /// op began, that no at-depth move swept — in mm², on the `UNCUT_CELL`
    /// grid. `None` where the stock model declines or the allowed region does.
    pub uncut_mm2: Option<f64>,
    /// What the tool met along the way, measured from the moves.
    pub load: Option<Load>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub pattern: Pattern,
    /// This is what the op cuts today — pinned, or the default.
    pub current: bool,
    pub outcome: Result<Stats, String>,
}

/// The narrowest stretch of floor between two of its own boundaries.
#[derive(Clone, Debug, PartialEq)]
pub struct Land {
    pub width: f64,
    pub from: Pt,
    pub to: Pt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpReport {
    pub op_idx: usize,
    pub title: String,
    pub tool_dia: f64,
    /// The share of the floor this tool can reach at all, where it is less
    /// than all of it (Phase 32 slice 2) — the same number for every pattern.
    pub reach_pct: Option<f64>,
    /// The floor's shape (Phase 34 slice 0) — a rest op's is its pieces'
    /// together. The same for every pattern, and nothing reads it to choose.
    pub shape: Option<crate::region::Shape>,
    pub lands: Vec<Land>,
    pub rows: Vec<Row>,
}

/// The floor's own boundary loops: a `Face`'s are authored; every other host's
/// is its region's outer loop.
fn floor_loops(p: &Part, op: &Operation, region: &crate::region::Region) -> Vec<Vec<Pt>> {
    match p.features.get(&op.feature_name) {
        Some(Feature::Face { loops, .. }) => loops.clone(),
        _ => vec![region.outer.clone()],
    }
}

/// The floor's narrow LANDS, narrowest first, by casting from the middle of
/// every boundary edge INTO the floor to the next boundary it meets. Not a
/// medial axis — that was named out in Phase 27 and still is — but the numbers
/// an author chooses a tool by: where the floor is thin, and how thin. Kept:
/// the narrowest, and every other distinct width under two tool diameters —
/// one-pass-or-two territory — up to four. (Distinct by WIDTH: a ring's wall
/// is measured from every chord of it, a symmetric part measures everything
/// twice, and a land that widens along its length is one land.)
pub fn lands(loops: &[Vec<Pt>], tool_dia: f64) -> Vec<Land> {
    let inside = |q: Pt| loops.first().is_some_and(|o| point_in_polygon(o, q.x, q.y)) && !loops[1..].iter().any(|h| point_in_polygon(h, q.x, q.y));
    let mut all: Vec<Land> = Vec::new();
    for l in loops {
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let len = (b.x - a.x).phypot(b.y - a.y);
            if len < 1e-6 {
                continue;
            }
            let m = Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
            for sign in [1.0, -1.0] {
                let n = (-(b.y - a.y) / len * sign, (b.x - a.x) / len * sign);
                if !inside(Pt { x: m.x + n.0 * 1e-3, y: m.y + n.1 * 1e-3 }) {
                    continue;
                }
                // The nearest boundary crossing along the ray.
                let mut t_hit = f64::INFINITY;
                let mut hit_alongside = false;
                for other in loops {
                    for j in 0..other.len() {
                        let (c, d) = (other[j], other[(j + 1) % other.len()]);
                        let (ex, ey) = (d.x - c.x, d.y - c.y);
                        let den = n.0 * ey - n.1 * ex;
                        if den.abs() < 1e-12 {
                            continue;
                        }
                        let t = ((c.x - m.x) * ey - (c.y - m.y) * ex) / den;
                        let u = ((c.x - m.x) * n.1 - (c.y - m.y) * n.0) / den;
                        // A LAND lies between boundaries that run alongside
                        // each other. A ray that lands on an edge slanting
                        // away is measuring a TAPER — a jaw's tip, a corner —
                        // which narrows to nothing and is not a width anyone
                        // chooses a tool by.
                        let alongside = ((ex * (b.x - a.x) + ey * (b.y - a.y)) / (ex.phypot(ey) * len)).abs() > LAND_PARALLEL;
                        if t > 1e-6 && (-1e-9..=1.0 + 1e-9).contains(&u) && t < t_hit {
                            t_hit = t;
                            hit_alongside = alongside;
                        }
                    }
                }
                if t_hit.is_finite() && hit_alongside {
                    all.push(Land { width: t_hit, from: m, to: Pt { x: m.x + n.0 * t_hit, y: m.y + n.1 * t_hit } });
                }
            }
        }
    }
    all.sort_by(|a, b| a.width.partial_cmp(&b.width).unwrap());
    let mut kept: Vec<Land> = Vec::new();
    for l in all {
        let narrow = l.width < 2.0 * tool_dia;
        let mid = |w: &Land| ((w.from.x + w.to.x) / 2.0, (w.from.y + w.to.y) / 2.0);
        // The same land again: measured from another chord of it (same width),
        // or a little further along it where it is a little wider (same place).
        let seen = kept.iter().any(|k| {
            let ((ax, ay), (bx, by)) = (mid(k), mid(&l));
            (k.width - l.width).abs() < LAND_DISTINCT || (ax - bx).phypot(ay - by) < 2.0 * tool_dia
        });
        if kept.len() < 4 && (kept.is_empty() || narrow) && !seen {
            kept.push(l);
        }
    }
    kept
}

/// Two lands are the same land when their widths agree this closely: a chorded
/// ring measures 7.92, 7.95, 7.97 round its wall.
const LAND_DISTINCT: f64 = 0.5;

/// Two boundaries run "alongside" within about twenty degrees of parallel.
const LAND_PARALLEL: f64 = 0.94;

fn crosses(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let side = |p: Pt, q: Pt, r: Pt| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    side(c, d, a) * side(c, d, b) < 0.0 && side(a, b, c) * side(a, b, d) < 0.0
}

/// One candidate: the project with this op pinned to `pattern`, emitted for
/// real, and its own block of the program measured.
fn measure(p: &Part, idx: usize, pattern: Pattern, lands: &[Land]) -> Result<Stats, String> {
    measure_with_moves(p, idx, pattern, lands).map(|(stats, _)| stats)
}

/// The moves a pattern's program makes AT DEPTH on this op's floor, in the
/// order it makes them — what every column of its row was measured from
/// (Phase 34 slice 3: and what `engagement::Walk` measures load from).
pub fn floor_moves(p: &Part, idx: usize, pattern: Pattern) -> Result<Vec<(Pt, Pt)>, String> {
    measure_with_moves(p, idx, pattern, &[]).map(|(_, moves)| moves)
}

/// One move of an op's own block, with the modal state the program carried
/// into it: where the tool was, where the line sends it, and whether it gets
/// there at feed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Move {
    pub from: (f64, f64, f64),
    pub to: (f64, f64, f64),
    pub feed: bool,
}

/// The moves of op `idx`, read from the emitted program with the modal X, Y
/// and Z carried across every op before it — an op's first words may name
/// only Z, and where the tool stood is the previous op's business. The one
/// reading of the emitted text every column of the report, and since Phase
/// 38 slice 1 the walk's load of an op as emitted, is measured from. A move
/// is a motion line (`G00` to `G03`) whose start is known; an arc is read
/// as its chords (`gcode::arc_chords`), since a ring written as one
/// full-circle block under `compress=arcs` starts where it ends and the
/// straight move between its ends would be nothing cut.
pub fn op_moves(emissions: &[crate::gcode::OpEmission], idx: usize) -> Vec<Move> {
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut out = Vec::new();
    for e in emissions {
        for line in &e.moves {
            if !line.starts_with("G0") {
                continue;
            }
            let (mut nx, mut ny, mut nz) = (x, y, z);
            for w in line.split_whitespace() {
                match w.as_bytes()[0] {
                    b'X' => nx = w[1..].parse().unwrap_or(nx),
                    b'Y' => ny = w[1..].parse().unwrap_or(ny),
                    b'Z' => nz = w[1..].parse().unwrap_or(nz),
                    _ => {}
                }
            }
            if e.op_idx == idx && x.is_finite() && y.is_finite() && z.is_finite() {
                let feed = !line.starts_with("G00");
                let arc = crate::gcode::arc_words(line).filter(|(i, j, _)| i.phypot(*j) > crate::gcode::PATH_EPS && (nz - z).abs() <= crate::gcode::PATH_EPS);
                match arc {
                    Some((i, j, cw)) => {
                        let ag = crate::gcode::arc_geom(x, y, nx, ny, i, j, cw);
                        for w in crate::gcode::arc_chords(&ag, [x, y, z], [nx, ny, nz]).windows(2) {
                            out.push(Move { from: (w[0][0], w[0][1], w[0][2]), to: (w[1][0], w[1][1], w[1][2]), feed });
                        }
                    }
                    None => out.push(Move { from: (x, y, z), to: (nx, ny, nz), feed }),
                }
            }
            (x, y, z) = (nx, ny, nz);
        }
    }
    out
}

/// Of an op's moves, the ones AT DEPTH on its floor — cut at feed with the
/// tool at `floor_z` before and after — as flat segments. What the `uncut`
/// column, the load columns and the walk read.
pub fn at_depth(moves: &[Move], floor_z: f64) -> Vec<(Pt, Pt)> {
    moves
        .iter()
        .filter(|m| m.feed && m.to.2 <= floor_z + 1e-6 && m.from.2 <= floor_z + 1e-6)
        .filter(|m| (m.to.0 - m.from.0).phypot(m.to.1 - m.from.1) > 1e-9)
        .map(|m| (Pt { x: m.from.0, y: m.from.1 }, Pt { x: m.to.0, y: m.to.1 }))
        .collect()
}

/// The walk's load of an op AS EMITTED (Phase 38 slice 1; `DECISIONS.md` →
/// "Phase 38 slice 1…"): what the tool met along the moves THIS program made
/// on the op's floor — the report's `past 90°` and `slot` for the row the op
/// actually cuts, read from the emission in hand rather than from a
/// re-emission. The home the advisory calls; the report keeps its own rows.
/// `None` where the op works no region, where the stock model declines
/// before it, or where the measure cannot solve its footprint — the same
/// silences as the report's columns.
pub fn load_as_emitted(p: &Part, emissions: &[crate::gcode::OpEmission], idx: usize) -> Option<Load> {
    let op = p.operations.get(idx)?;
    let region = crate::region::of(p, op)?;
    let floor_z = region.z + crate::parse::derive_floor_skin(p, op);
    let moves = at_depth(&op_moves(emissions, idx), floor_z);
    load_of(p, idx, &moves, floor_z)
}

fn measure_with_moves(p: &Part, idx: usize, pattern: Pattern, lands: &[Land]) -> Result<(Stats, Vec<(Pt, Pt)>), String> {
    let mut candidate = p.clone();
    candidate.operations[idx].pattern = Some(pattern);
    // HELD (Phase 62 slice 1): the held row cuts at the spacing its ceiling
    // derives and refuses an authored stepover, so its candidate carries
    // none; every other row of a held op cuts at that same spacing, as if
    // authored — one spacing across the table, the word the only difference.
    candidate.operations[idx].stepover = if pattern == Pattern::Held { None } else { crate::parse::derive_stepover(p, &p.operations[idx]) };
    let op = &candidate.operations[idx];
    let tool = candidate.tools.get(&op.tool_num).ok_or("the op's tool is not declared")?;
    let r = tool.dia / 2.0;
    // Emitted ONCE, and the clock reads that emission.
    let (text, emissions) = crate::gcode::program_with_moves(&candidate)?;
    let clock = crate::cut_time::time_report_of(&candidate, &text, &emissions)?;
    let row = clock.rows.iter().find(|row| row.op_idx == Some(idx)).ok_or("the clock has no row for this op")?;
    let region = crate::region::of(&candidate, op).ok_or("the op works no region")?;
    let floor = floor_loops(&candidate, op, &region);
    let before = crate::stock::remaining_before(&candidate, idx);
    let on_floor = |q: Pt| point_in_polygon(&floor[0], q.x, q.y) && !floor[1..].iter().any(|h| point_in_polygon(h, q.x, q.y));
    // The tool's disc, sampled: its centre, a ring near its edge, a ring halfway.
    let disc: Vec<(f64, f64)> = std::iter::once((0.0, 0.0))
        .chain((0..12).map(|k| (k as f64 * std::f64::consts::PI / 6.0, 0.98)).chain((0..6).map(|k| (k as f64 * std::f64::consts::PI / 3.0, 0.5))).map(|(a, s)| (r * s * a.pcos(), r * s * a.psin())))
        .collect();
    // A probe across each land, reaching a radius past each side: a tool whose
    // centre is off the land can still be cutting it.
    let probes: Vec<(Pt, Pt)> = lands
        .iter()
        .map(|l| {
            let (dx, dy) = ((l.to.x - l.from.x) / l.width, (l.to.y - l.from.y) / l.width);
            (Pt { x: l.from.x - dx * r, y: l.from.y - dy * r }, Pt { x: l.to.x + dx * r, y: l.to.y + dy * r })
        })
        .collect();
    let mut land_passes = vec![0usize; lands.len()];

    let (mut over, mut plunges, mut ramps, mut lifts, mut turns) = ((0.0, 0.0, 0.0), 0, 0, 0, 0);
    let mut heading: Option<f64> = None;
    let mut was_ramping = false;
    let floor_z = region.z + crate::parse::derive_floor_skin(&candidate, op);
    // One reading of the text (`op_moves`); the at-depth moves every column
    // is measured from are `at_depth` of it, the same home the walk reads.
    let moves = op_moves(&emissions, idx);
    let swept = at_depth(&moves, floor_z);
    for m in &moves {
        let ((x, y, z), (nx, ny, nz), feed) = (m.from, m.to, m.feed);
        let moved = (nx - x).phypot(ny - y);
        // Entries, not segments: a ramp down a chorded loop is dozens of
        // descending moves and one entry.
        let descending = feed && nz < z - 1e-9;
        if descending && moved < 1e-9 {
            plunges += 1;
        } else if descending && !was_ramping {
            ramps += 1;
        }
        was_ramping = descending && moved >= 1e-9;
        if !feed && nz > z + 1e-9 {
            lifts += 1;
        }
        let at_depth = feed && nz <= floor_z + 1e-6 && z <= floor_z + 1e-6;
        if at_depth && moved > 1e-9 {
            let steps = (moved.ceil() as usize).max(1);
            for k in 0..steps {
                let t = (k as f64 + 0.5) / steps as f64;
                let (cx, cy) = (x + t * (nx - x), y + t * (ny - y));
                let touches = |pred: &dyn Fn(Pt) -> bool| disc.iter().any(|(dx, dy)| pred(Pt { x: cx + dx, y: cy + dy }));
                let stock = |q: Pt| before.material_at(q.x, q.y, floor_z + 0.01);
                let share = moved / steps as f64;
                if touches(&|q| on_floor(q) && stock(q)) {
                    over.0 += share;
                } else if touches(&stock) {
                    over.1 += share;
                } else {
                    over.2 += share;
                }
            }
            let h = (ny - y).patan2(nx - x);
            if let Some(prev) = heading {
                let d = ((h - prev + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI).abs();
                turns += usize::from(d > 30f64.to_radians());
            }
            heading = Some(h);
            for (k, (pa, pb)) in probes.iter().enumerate() {
                land_passes[k] += usize::from(crosses(Pt { x, y }, Pt { x: nx, y: ny }, *pa, *pb));
            }
        } else {
            heading = None;
        }
    }
    // The allowance the family stood off by: a pocket's, and since Phase 61 a
    // Clear's on a Plane or a Face — the report's allowed region must be the
    // one the path was cut inside, or the skin round an island reads as floor.
    let skin = if matches!(candidate.features.get(&op.feature_name), Some(Feature::ProfilePocket { .. }))
        || (op.op_type == crate::parse::OpType::Clear && matches!(candidate.features.get(&op.feature_name), Some(Feature::Plane { .. } | Feature::Face { .. })))
    {
        op.stock_to_leave.unwrap_or(0.0)
    } else {
        0.0
    };
    let stock_at = |q: Pt| before.material_at(q.x, q.y, floor_z + 0.01);
    let uncut_mm2 = if !before.complete {
        None
    } else if crate::rest::is_rest(op) {
        // The rest floor (Phase 33 slice 4) is counted piece by piece: each
        // piece's own floor against where the centre may be IN it — the
        // host's whole allowed region would call a pinch the piece cannot
        // enter reachable.
        crate::rest::pieces(&candidate, op).ok().and_then(|pieces| {
            let mut total = 0.0;
            for pc in pieces.iter().filter(|pc| pc.is_cut()) {
                let reg = crate::region::of(&pc.part, &pc.op)?;
                let (ccw, cw) = crate::ops::clear::allowed_loops(&pc.part, &pc.op, &reg, r, 0.0).ok()?;
                total += uncut(&floor_loops(&pc.part, &pc.op, &reg), &ccw, &cw, &swept, r, &stock_at);
            }
            Some(total)
        })
    } else {
        match crate::ops::clear::allowed_loops(&candidate, op, &region, r, skin) {
            Ok((ccw, cw)) => Some(uncut(&floor, &ccw, &cw, &swept, r, &stock_at)),
            _ => None,
        }
    };
    let load = load_of(&candidate, idx, &swept, floor_z);
    let at_depth_mm: f64 = swept.iter().map(|(a, b)| (b.x - a.x).phypot(b.y - a.y)).sum();
    let stats = Stats {
        cutting_mm: row.cutting_mm,
        at_depth_mm,
        cutting_s: row.cutting_s,
        rapid_mm: row.rapid_mm,
        over: before.complete.then_some(over),
        plunges,
        ramps,
        lifts,
        turns,
        // The band prices the host's whole family; a rest op cuts pieces of it.
        worst_deg: if crate::rest::is_rest(op) { None } else { crate::parse::op_engagement(&candidate, op).map(|e| e.worst.radians.to_degrees()) },
        land_passes,
        uncut_mm2,
        load,
    };
    Ok((stats, swept))
}

/// What the tool met along the way (Phase 34 slices 3–4): of the length cut
/// at depth, how much was cut past half immersion and how much in a full
/// slot, and the most it ever met. Lengths, because every pattern that
/// starts in solid slots SOMEWHERE — a worst that counts the slot reads 180°
/// on every row, and one that skips it needs a "first pass" only the offset
/// family has. How MUCH of the path is heavy is what differs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Load {
    /// At-depth length with the tool in material at all.
    pub engaged_mm: f64,
    /// …past half immersion, the slot included.
    pub past_half_mm: f64,
    /// …in a full-width slot.
    pub slot_mm: f64,
    pub worst: f64,
}

/// A degree: inside it two engagements are the same engagement. A row at a
/// stepover of exactly the radius rides exactly 90°, and must not flicker
/// across the line.
pub const LOAD_AGREE: f64 = std::f64::consts::PI / 180.0;

/// One station a millimetre, at the middle of its millimetre — the stations
/// the `on floor` shares are counted at.
pub fn load_along(moves: &[(Pt, Pt)], r: f64, bounds: &crate::engagement::Bounds, material: &dyn Fn((f64, f64)) -> bool) -> Load {
    let walk = crate::engagement::Walk::of(moves, r);
    let mut load = Load { engaged_mm: 0.0, past_half_mm: 0.0, slot_mm: 0.0, worst: 0.0 };
    for (k, (a, b)) in moves.iter().enumerate() {
        let len = (b.x - a.x).phypot(b.y - a.y);
        let steps = (len.ceil() as usize).max(1);
        for s in 0..steps {
            let e = walk.engagement(k, (s as f64 + 0.5) / steps as f64, bounds, material);
            let share = len / steps as f64;
            if e > LOAD_AGREE {
                load.engaged_mm += share;
            }
            if e > std::f64::consts::FRAC_PI_2 + LOAD_AGREE {
                load.past_half_mm += share;
            }
            if e > std::f64::consts::PI - LOAD_AGREE {
                load.slot_mm += share;
            }
            load.worst = load.worst.max(e);
        }
    }
    load
}

/// What one station of the walk is (ROADMAP item 3 slice 4): the three
/// thresholds `load_along` counts by, as kinds — air (the tool meets
/// nothing), cutting, past half immersion, a full-width slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Station {
    Air,
    Cutting,
    PastHalf,
    Slot,
}

/// A stretch of the at-depth path of one kind: the stations it spans, as
/// the points they lie between, merged along a move and across moves that
/// meet end to start. What the window draws over the selected path and
/// `--patterns` lists.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretch {
    pub kind: Station,
    pub pts: Vec<Pt>,
    pub mm: f64,
}

/// The stretches of `moves`, by the same stations `load_along` counts: one a
/// millimetre, at the middle of its millimetre.
pub fn stretches_along(moves: &[(Pt, Pt)], r: f64, bounds: &crate::engagement::Bounds, material: &dyn Fn((f64, f64)) -> bool) -> Vec<Stretch> {
    let walk = crate::engagement::Walk::of(moves, r);
    let mut out: Vec<Stretch> = Vec::new();
    for (k, (a, b)) in moves.iter().enumerate() {
        let len = (b.x - a.x).phypot(b.y - a.y);
        let steps = (len.ceil() as usize).max(1);
        let share = len / steps as f64;
        for s in 0..steps {
            let e = walk.engagement(k, (s as f64 + 0.5) / steps as f64, bounds, material);
            let kind = if e > std::f64::consts::PI - LOAD_AGREE {
                Station::Slot
            } else if e > std::f64::consts::FRAC_PI_2 + LOAD_AGREE {
                Station::PastHalf
            } else if e > LOAD_AGREE {
                Station::Cutting
            } else {
                Station::Air
            };
            let at = |t: f64| Pt { x: a.x + t * (b.x - a.x), y: a.y + t * (b.y - a.y) };
            let (p0, p1) = (at(s as f64 / steps as f64), at((s as f64 + 1.0) / steps as f64));
            match out.last_mut() {
                Some(last) if last.kind == kind && last.pts.last().is_some_and(|q| (q.x - p0.x).phypot(q.y - p0.y) <= 1e-6) => {
                    last.pts.push(p1);
                    last.mm += share;
                }
                _ => out.push(Stretch { kind, pts: vec![p0, p1], mm: share }),
            }
        }
    }
    out
}

/// The walk of an op AS EMITTED, drawn and listed (ROADMAP item 3 slice
/// 4): its stretches on the floor at `z`, its entries — where each descent
/// at feed arrives, the last point of a run of descending feed moves — and
/// the load the stretches sum to. `None` exactly where `load_as_emitted` is.
#[derive(Clone, Debug, PartialEq)]
pub struct Walked {
    pub z: f64,
    pub stretches: Vec<Stretch>,
    pub entries: Vec<(f64, f64, f64)>,
    pub load: Load,
}

pub fn walk_as_emitted(p: &Part, emissions: &[crate::gcode::OpEmission], idx: usize) -> Option<Walked> {
    let op = p.operations.get(idx)?;
    let region = crate::region::of(p, op)?;
    let floor_z = region.z + crate::parse::derive_floor_skin(p, op);
    let all = op_moves(emissions, idx);
    let moves = at_depth(&all, floor_z);
    let before = crate::stock::remaining_before(p, idx);
    if !before.complete {
        return None;
    }
    let bounds = crate::engagement::Bounds::of(&before)?;
    let r = p.tools.get(&op.tool_num)?.dia / 2.0;
    let material = |q: (f64, f64)| before.material_at(q.0, q.1, floor_z + 0.01);
    let stretches = stretches_along(&moves, r, &bounds, &material);
    let load = load_along(&moves, r, &bounds, &material);
    let mut entries = Vec::new();
    let mut descending = false;
    for m in &all {
        let down = m.feed && m.to.2 < m.from.2 - 1e-9;
        if down {
            if descending {
                *entries.last_mut().expect("a run has its entry") = m.to;
            } else {
                entries.push(m.to);
            }
        }
        descending = down;
    }
    Some(Walked { z: floor_z, stretches, entries, load })
}

/// The walk in words (one home for the card and `--patterns`): the summary
/// line, then one line per stretch past half immersion or in a slot, and
/// one per entry.
pub fn walk_lines(w: &Walked) -> Vec<String> {
    let past: Vec<&Stretch> = w.stretches.iter().filter(|s| s.kind == Station::PastHalf).collect();
    let slot: Vec<&Stretch> = w.stretches.iter().filter(|s| s.kind == Station::Slot).collect();
    let mut out = vec![format!(
        "as cut: {:.1} mm past 90° in {} stretch{}, {:.1} mm slot in {}, {} entr{}",
        w.load.past_half_mm,
        past.len() + slot.len(),
        if past.len() + slot.len() == 1 { "" } else { "es" },
        w.load.slot_mm,
        slot.len(),
        w.entries.len(),
        if w.entries.len() == 1 { "y" } else { "ies" }
    )];
    let ends = |s: &Stretch| {
        let (a, b) = (s.pts[0], s.pts[s.pts.len() - 1]);
        format!("({:.1}, {:.1}) to ({:.1}, {:.1})  {:.1} mm", a.x, a.y, b.x, b.y, s.mm)
    };
    for s in w.stretches.iter().filter(|s| s.kind != Station::Air && s.kind != Station::Cutting) {
        out.push(format!("  {}  {}", if s.kind == Station::Slot { "slot     " } else { "past 90° " }, ends(s)));
    }
    for (x, y, z) in &w.entries {
        out.push(format!("  entry at ({x:.1}, {y:.1}) z{z:.3}"));
    }
    out
}

/// `None` where the stock model declines before the op, or holds a footprint
/// the measure cannot solve against (`engagement::Bounds::of`).
fn load_of(p: &Part, idx: usize, moves: &[(Pt, Pt)], floor_z: f64) -> Option<Load> {
    let before = crate::stock::remaining_before(p, idx);
    if !before.complete {
        return None;
    }
    let bounds = crate::engagement::Bounds::of(&before)?;
    let r = p.tools.get(&p.operations[idx].tool_num)?.dia / 2.0;
    Some(load_along(moves, r, &bounds, &|q| before.material_at(q.0, q.1, floor_z + 0.01)))
}

/// The grid the `uncut` column is counted on. A fifth of a millimetre: a
/// crescent a pass leaves against a wall is millimetres across, and a cell is
/// 0.04mm² — the column's resolution, said in its own heading.
pub const UNCUT_CELL: f64 = 0.2;

/// Floor nothing swept: cells of the floor that held stock, that a tool of
/// radius `r` could have reached (within `r` of where its centre may be — the
/// inside corner of two walls is nobody's to cut, and is not counted), and
/// that lie within `r` of no at-depth move. It can only UNDER-claim: a cell
/// counts as reachable only when it is reachable with a whole cell to spare.
pub fn uncut(floor: &[Vec<Pt>], ccw: &[Vec<Pt>], cw: &[Vec<Pt>], swept: &[(Pt, Pt)], r: f64, stock: &dyn Fn(Pt) -> bool) -> f64 {
    let h = UNCUT_CELL;
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for q in floor.iter().flatten() {
        (x0, y0, x1, y1) = (x0.min(q.x), y0.min(q.y), x1.max(q.x), y1.max(q.y));
    }
    if !x0.is_finite() {
        return 0.0;
    }
    let (nx, ny) = (((x1 - x0) / h).ceil() as usize + 1, ((y1 - y0) / h).ceil() as usize + 1);
    let centre = |i: usize, j: usize| Pt { x: x0 + (i as f64 + 0.5) * h, y: y0 + (j as f64 + 0.5) * h };
    let mut hit = vec![false; nx * ny];
    for (a, b) in swept {
        let cell = |v: f64, o: f64, n: usize| (((v - o) / h).floor().max(0.0) as usize).min(n - 1);
        let (i0, i1) = (cell(a.x.min(b.x) - r, x0, nx), cell(a.x.max(b.x) + r, x0, nx));
        let (j0, j1) = (cell(a.y.min(b.y) - r, y0, ny), cell(a.y.max(b.y) + r, y0, ny));
        for j in j0..=j1 {
            for i in i0..=i1 {
                let c = centre(i, j);
                if !hit[j * nx + i] && crate::stock::dist_to_polygon(&[*a, *b], c.x, c.y) <= r + 1e-6 {
                    hit[j * nx + i] = true;
                }
            }
        }
    }
    // (By row and by bucket — `cover`'s two helpers — rather than a polygon
    // test per cell: the same answer, a few hundred times cheaper.)
    let segs: Vec<(Pt, Pt)> = ccw.iter().chain(cw).flat_map(|l| (0..l.len()).map(move |k| (l[k], l[(k + 1) % l.len()]))).collect();
    let near = crate::cover::Buckets::of(&segs, r.max(h));
    let xs: Vec<f64> = (0..nx).map(|i| x0 + (i as f64 + 0.5) * h).collect();
    let mut cells = 0usize;
    for j in 0..ny {
        let y = y0 + (j as f64 + 0.5) * h;
        let floor_row = crate::cover::and_not(&crate::cover::row_inside(&floor[..1], y, &xs), &crate::cover::row_inside(&floor[1..], y, &xs));
        let allowed_row = crate::cover::and_not(&crate::cover::row_inside(ccw, y, &xs), &crate::cover::row_inside(cw, y, &xs));
        for i in 0..nx {
            let c = centre(i, j);
            if !hit[j * nx + i] && floor_row[i] && (allowed_row[i] || near.any_within(c, r - h)) && stock(c) {
                cells += 1;
            }
        }
    }
    cells as f64 * h * h
}

/// Every op that works a region, every pattern, measured.
pub fn op_reports(p: &Part) -> Vec<OpReport> {
    (0..p.operations.len()).filter_map(|idx| op_report(p, idx)).collect()
}

/// The op, its region and its tool, where a pattern could walk it at all —
/// the one test of whether an op has a report.
fn walked(p: &Part, idx: usize) -> Option<(&Operation, crate::region::Region, &crate::parse::Tool)> {
    let op = p.operations.get(idx)?;
    let region = crate::region::of(p, op)?;
    if op.strategy.is_some() && !crate::rest::is_rest(op) {
        return None; // a field-selected work-list is not a pattern's to walk
    }
    Some((op, region, p.tools.get(&op.tool_num)?))
}

/// The one row of an op's report marked `*` — what the op actually cuts —
/// measured alone (Phase 37 slice 4). The corpus gate asks every op whether
/// it leaves floor under its OWN pattern; the report's other rows are an
/// emitter run each that gate has no use for. Its `passes across each land`
/// is empty: no lands are probed. `None` exactly where `op_report` is.
pub fn current_row(p: &Part, idx: usize) -> Option<Row> {
    let (op, ..) = walked(p, idx)?;
    let pattern = crate::parse::derive_pattern(p, op)?;
    Some(Row { pattern, current: true, outcome: measure(p, idx, pattern, &[]) })
}

/// One op's report, or `None` where the op works no region a pattern could
/// walk. The window's inspector asks for the selected op alone (it used to
/// take every op's report and keep one — on a three-op rest-floor program
/// that was three times the emitter runs for one table).
pub fn op_report(p: &Part, idx: usize) -> Option<OpReport> {
    let (op, region, tool) = walked(p, idx)?;
    {
        let rest = crate::rest::is_rest(op);
        // A rest op's floor is its pieces (Phase 33 slice 4): their lands, the
        // narrowest of each, narrowest first — and none under the report's own
        // grid: a stretch between two curves is a lens, and its narrowest
        // "land" is the point it tapers to.
        let pieces = if rest { crate::rest::pieces(p, op).ok() } else { None };
        let lands = match &pieces {
            Some(pieces) => {
                let mut ls: Vec<Land> = pieces
                    .iter()
                    .filter(|pc| pc.is_cut())
                    .filter_map(|pc| crate::region::of(&pc.part, &pc.op).and_then(|reg| lands(&floor_loops(&pc.part, &pc.op, &reg), tool.dia).into_iter().find(|l| l.width >= UNCUT_CELL)))
                    .collect();
                ls.sort_by(|a, b| a.width.partial_cmp(&b.width).unwrap());
                ls.truncate(4);
                ls
            }
            None => lands(&floor_loops(p, op, &region), tool.dia),
        };
        // What the op cuts today: its pin, or the default.
        let current = crate::parse::derive_pattern(p, op);
        // The raster's row only where the raster is legal: a bare plane.
        let rows = Pattern::ALL
            .iter()
            .chain(crate::parse::raster_applies(p, op).then_some(&Pattern::Raster))
            .map(|k| Row { pattern: *k, current: current == Some(*k), outcome: measure(p, idx, *k, &lands) })
            .collect();
        let skin = if op.op_type == crate::parse::OpType::Clear { op.stock_to_leave.unwrap_or(0.0) } else { 0.0 };
        let coverage = match &pieces {
            Some(pieces) => Some(crate::rest::coverage(pieces)),
            None if rest => None,
            None => crate::ops::clear::coverage(p, op, &region, tool.dia / 2.0, skin).ok(),
        };
        let shape = match &pieces {
            Some(pieces) => pieces
                .iter()
                .filter(|pc| pc.is_cut())
                .map(|pc| crate::region::of(&pc.part, &pc.op).and_then(|reg| crate::region::shape(&pc.part, &pc.op, &reg)))
                .try_fold(crate::region::Shape { area: 0.0, perimeter: 0.0, wall: 0.0 }, |sum, s| {
                    s.map(|s| crate::region::Shape { area: sum.area + s.area, perimeter: sum.perimeter + s.perimeter, wall: sum.wall + s.wall })
                })
                .filter(|s| s.perimeter > 0.0),
            None => crate::region::shape(p, op, &region),
        };
        let reach_pct = coverage.filter(|c| !c.stretches.is_empty()).map(|c| c.reachable_pct());
        Some(OpReport {
            op_idx: idx,
            reach_pct,
            shape,
            title: format!(
                "op [{}] {} on '{}' (T{} dia {}){}",
                idx,
                crate::stock::op_word(op),
                op.feature_name,
                op.tool_num,
                crate::gcode::f(tool.dia),
                if rest { format!(" — {}", crate::rest::line(p, op)) } else { String::new() }
            ),
            tool_dia: tool.dia,
            lands,
            rows,
        })
    }
}

/// The floor's shape, as the report says it: how much floor, how wide it is
/// on the whole, and how much of its boundary something stands beyond.
pub fn shape_line(s: &crate::region::Shape, tool_dia: f64) -> String {
    format!(
        "floor: {:.0}mm², mean width {}mm ({:.2} tool diameters), {:.0}% wall",
        s.area,
        crate::gcode::f(s.mean_width()),
        s.mean_width() / tool_dia,
        100.0 * s.wall_share()
    )
}

/// One land, as the report says it.
pub fn land_line(k: usize, l: &Land, tool_dia: f64) -> String {
    format!(
        "land {}: {}mm across ({:.2} tool diameters) at ({}, {})",
        k + 1,
        crate::gcode::f(l.width),
        l.width / tool_dia,
        crate::gcode::f((l.from.x + l.to.x) / 2.0),
        crate::gcode::f((l.from.y + l.to.y) / 2.0)
    )
}

/// One column of the table: its group, its label, the width its cells are
/// padded to in the terminal, and which edge they sit on. The header's
/// labels, the group line, `row_line` and (Phase 39 slice 2) the window's
/// grid all read this one list, so a column is added or moved here and
/// nowhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub group: &'static str,
    pub title: &'static str,
    pub width: usize,
    pub left: bool,
}

/// The columns, in the order the questions are asked (Phase 39 slice 1,
/// fork 2A; `DECISIONS.md` → "Phase 39 slice 1…"): COVER first because it
/// disqualifies — `uncut` is 0.0 or the row is not a candidate — then what
/// the row COSTS, then WHERE the tool cut, then what LOAD it met, last
/// because Phase 34 found it is what decides between patterns that cover.
/// The widths are the terminal's; `passes across each land` is as wide as
/// its label, which holds four lands of two-digit counts.
pub const COLUMNS: [Column; 16] = [
    Column { group: "", title: "pattern", width: 8, left: true },
    Column { group: "COVER", title: "uncut", width: 9, left: false },
    Column { group: "COVER", title: "passes across each land", width: 23, left: true },
    Column { group: "COST", title: "at depth", width: 9, left: false },
    Column { group: "COST", title: "entries", width: 7, left: false },
    Column { group: "COST", title: "time", width: 7, left: false },
    Column { group: "COST", title: "plunge", width: 6, left: false },
    Column { group: "COST", title: "ramp", width: 4, left: false },
    Column { group: "COST", title: "lift", width: 4, left: false },
    Column { group: "COST", title: "turns", width: 6, left: false },
    Column { group: "WHERE", title: "on floor", width: 12, left: false },
    Column { group: "WHERE", title: "other stock", width: 12, left: false },
    Column { group: "WHERE", title: "air", width: 12, left: false },
    Column { group: "LOAD", title: "past 90°", width: 13, left: false },
    Column { group: "LOAD", title: "slot", width: 12, left: false },
    Column { group: "LOAD", title: "worst (band)", width: 12, left: false },
];

/// One cell of a row: its words, and how many columns it spans — one, save
/// a refusal (which spans the row) and the stock model's decline (which
/// spans the three WHERE columns).
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub text: String,
    pub span: usize,
}

impl Cell {
    fn one(text: String) -> Cell {
        Cell { text, span: 1 }
    }
}

/// The base, said once, above the header (Phase 39 slice 0; `DECISIONS.md` →
/// "Phase 39 slice 0…"): every percentage in a row is a share of the length
/// cut at depth. Before it, three columns stood on three different lengths
/// and the header named none of them.
pub const BASE_NOTE: &str = "every % is a share of the length cut at depth (`at depth`); `entries` is the rest of the feed length — the ramps, plunges and descents that reach it";

/// Columns within a group sit one space apart, groups two.
fn gap(k: usize) -> &'static str {
    if k == 0 {
        ""
    } else if COLUMNS[k].group != COLUMNS[k - 1].group {
        "  "
    } else {
        " "
    }
}

fn pad(text: &str, width: usize, left: bool) -> String {
    let n = text.chars().count();
    let fill = " ".repeat(width.saturating_sub(n));
    if left { format!("{text}{fill}") } else { format!("{fill}{text}") }
}

/// The width a run of columns takes on the line, gaps included.
fn span_width(from: usize, span: usize) -> usize {
    (from..from + span).map(|k| COLUMNS[k].width + if k > from { gap(k).len() } else { 0 }).sum()
}

/// The group line: each group's name centred over its columns. Two leading
/// spaces stand in for the row's mark.
pub fn group_line() -> String {
    let mut out = String::from("  ");
    let mut k = 0;
    while k < COLUMNS.len() {
        let group = COLUMNS[k].group;
        let span = COLUMNS[k..].iter().take_while(|c| c.group == group).count();
        let w = span_width(k, span);
        out.push_str(gap(k));
        let n = group.chars().count();
        let before = w.saturating_sub(n) / 2;
        out.push_str(&" ".repeat(before));
        out.push_str(group);
        out.push_str(&" ".repeat(w - before - n));
        k += span;
    }
    out.trim_end().to_string()
}

/// The column header. Its labels sit over the edge of each cell in
/// `row_line` — the same list, the same widths. Two leading spaces stand in
/// for the row's mark.
pub fn header() -> String {
    let mut out = String::from("  ");
    for (k, c) in COLUMNS.iter().enumerate() {
        out.push_str(gap(k));
        out.push_str(&pad(c.title, c.width, c.left));
    }
    out
}

/// A row as CELLS, one per column (a spanning cell stands for several) —
/// what the terminal joins and the window lays out. Every `%` is a share of
/// `at depth`, the one base (slice 0).
pub fn cells(row: &Row) -> Vec<Cell> {
    let word = Cell::one(row.pattern.word().to_string());
    let st = match &row.outcome {
        Err(why) => return vec![word, Cell { text: format!("refused — {why}"), span: COLUMNS.len() - 1 }],
        Ok(st) => st,
    };
    let base = st.at_depth_mm.max(1e-9);
    // A share cell is `NNNmm PPP%`, its two numbers each right-aligned, so the
    // percentages line up down a column as the lengths do.
    let share = |v: f64| Cell::one(format!("{:>5.0}mm {:>3.0}%", v, 100.0 * v / base));
    let mut out = vec![
        word,
        // COVER: reachable floor nothing swept, in mm² on the `UNCUT_CELL`
        // grid; and how many passes cross each narrow land.
        Cell::one(st.uncut_mm2.map_or("—".to_string(), |a| format!("{a:.1}mm²"))),
        Cell::one(st.land_passes.iter().enumerate().map(|(k, n)| format!("{}:{n}", k + 1)).collect::<Vec<_>>().join("  ")),
        // COST: the length at depth (the base), the entries that reach it,
        // the clock, and the entries and turns by kind.
        Cell::one(format!("{:.0}mm", st.at_depth_mm)),
        Cell::one(format!("{:.0}mm", (st.cutting_mm - st.at_depth_mm).max(0.0))),
        Cell::one(format!("{:.0}s", st.cutting_s)),
        Cell::one(st.plunges.to_string()),
        Cell::one(st.ramps.to_string()),
        Cell::one(st.lifts.to_string()),
        Cell::one(st.turns.to_string()),
    ];
    // WHERE: over the floor, over other stock, over air — the three add to
    // the base, so their shares add to a hundred. Or one sentence across
    // the three, where the stock model declines before this op.
    match st.over {
        Some((a, b, c)) => out.extend([share(a), share(b), share(c)]),
        None => out.push(Cell { text: "— the stock model declines here —".to_string(), span: 3 }),
    }
    // LOAD (Phase 34 slice 4): the length cut past half immersion and the
    // length cut in a full slot, each with its share of the base, measured
    // from the moves; and the band's worst, where the band answers.
    match st.load {
        Some(l) => out.extend([share(l.past_half_mm), share(l.slot_mm)]),
        None => out.extend([Cell::one("—".to_string()), Cell::one("—".to_string())]),
    }
    out.push(Cell::one(st.worst_deg.map_or("—".to_string(), |d| format!("{d:.0}°"))));
    out
}

/// One row, as the report says it — the terminal and the window print the
/// same words, because both read `cells`. A spanning cell is centred over
/// the columns it stands for.
pub fn row_line(row: &Row) -> String {
    let mut out = String::from(if row.current { "*" } else { " " });
    out.push(' ');
    let mut k = 0;
    for cell in cells(row) {
        if k > 0 {
            out.push_str(gap(k));
        }
        if cell.span == 1 {
            out.push_str(&pad(&cell.text, COLUMNS[k].width, COLUMNS[k].left));
        } else if k + cell.span == COLUMNS.len() && k == 1 {
            // A refusal: the sentence runs on from the word, unpadded.
            out.push_str(&cell.text);
        } else {
            let w = span_width(k, cell.span);
            let n = cell.text.chars().count();
            let before = w.saturating_sub(n) / 2;
            out.push_str(&" ".repeat(before));
            out.push_str(&cell.text);
            out.push_str(&" ".repeat(w.saturating_sub(before + n)));
        }
        k += cell.span;
    }
    out.trim_end().to_string()
}

/// The `--patterns` report.
pub fn render(p: &Part) -> String {
    let reports = op_reports(p);
    let emitted = crate::gcode::program_with_moves(p).ok().map(|(_, e)| e);
    if reports.is_empty() {
        return "patterns: no op in this project works a region — a pattern is how a floor is walked (an End, or a Clear on a ProfilePocket)\n".to_string();
    }
    let mut s = String::from("patterns — what each would cut, measured; nothing here is a score, and the engine does not choose\n");
    for r in &reports {
        s.push_str(&format!("\n{}\n", r.title));
        if let Some(pct) = r.reach_pct {
            s.push_str(&match p.operations[r.op_idx].cover {
                Some(c) => format!("  reaches {pct:.1}% of its floor — the run line concedes the rest (`cover={}`)\n", crate::gcode::f(c).trim_end_matches('0').trim_end_matches('.')),
                None => format!("  reaches {pct:.1}% of its floor — every row below is refused unless the run line concedes the rest (`cover={:.1}`)\n", (pct * 10.0).floor() / 10.0),
            });
        }
        if let Some(shape) = &r.shape {
            s.push_str(&format!("  {}\n", shape_line(shape, r.tool_dia)));
        }
        for (k, l) in r.lands.iter().enumerate() {
            s.push_str(&format!("  {}\n", land_line(k, l, r.tool_dia)));
        }
        s.push_str(&format!("  {BASE_NOTE}\n  {}\n  {}\n", group_line(), header()));
        for row in &r.rows {
            s.push_str(&format!("  {}\n", row_line(row)));
        }
        s.push_str("    (* what this op cuts today. To pin another, add its word to the run line: `pattern=zigzag`.)\n");
        // The walk of the `*` row as drawn in the window (ROADMAP item 3
        // slice 4): the same stretches, listed.
        if let Some(w) = emitted.as_ref().and_then(|e| walk_as_emitted(p, e, r.op_idx)) {
            for l in walk_lines(&w) {
                s.push_str(&format!("  {l}\n"));
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The walk at the band's own worst position: the point of a declared
    /// loop, found again on the emitted path.
    fn both(dir: &str) -> (f64, f64) {
        let p = crate::parse::load_project(dir).unwrap_or_else(|_| panic!("{dir} loads"));
        let op = &p.operations[0];
        let band = crate::parse::op_engagement(&p, op).expect("the band answers for an offset Clear").worst;
        let moves = floor_moves(&p, 0, Pattern::Offset).unwrap();
        let r = p.tools[&op.tool_num].dia / 2.0;
        let before = crate::stock::remaining_before(&p, 0);
        let bounds = crate::engagement::Bounds::of(&before).unwrap();
        let floor_z = crate::region::of(&p, op).unwrap().z;
        let walk = crate::engagement::Walk::of(&moves, r);
        // Emitted coordinates are rounded to three decimals.
        let mut best: Option<f64> = None;
        for (k, (a, b)) in moves.iter().enumerate() {
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let t = (((band.at.0 - a.x) * dx + (band.at.1 - a.y) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            if (a.x + t * dx - band.at.0).phypot(a.y + t * dy - band.at.1) < 2e-3 {
                let e = walk.engagement(k, t, &bounds, &|q| before.material_at(q.0, q.1, floor_z + 0.01));
                if best.map_or(true, |b| (e - band.radians).abs() < (b - band.radians).abs()) {
                    best = Some(e);
                }
            }
        }
        let walk = best.unwrap_or_else(|| panic!("{dir}: the path never passes ({:.3}, {:.3})", band.at.0, band.at.1));
        (walk.to_degrees(), band.radians.to_degrees())
    }

    /// Where both can answer, they agree (Phase 34 slice 3, fork 2A's pin).
    /// The band prices the offset family from its declared loops; the walk
    /// measures the program those loops became. Six of the eight offset
    /// `Clear`s in the corpus read the same engagement both ways, to a
    /// hundredth of a degree — `pocket_post` among them since Phase 37 slice
    /// 1, where the band read 94.97° until its containment test was replaced.
    #[test]
    fn the_walk_agrees_with_the_band_at_the_bands_own_worst() {
        for (dir, want) in [
            ("profile_pocket_clear.cam", 100.31),
            ("profile_pocket_clear_rough.cam", 106.65),
            ("profile_pocket_regions.cam", 90.0),
            ("pocket_floor_offset.cam", 180.0),
            ("pocket_islands.cam", 180.0),
            ("pocket_post.cam", 139.46),
        ] {
            let (walk, band) = both(dir);
            assert!((band - want).abs() < 0.01, "{dir}: the band reads {band:.2}");
            assert!((walk - band).abs() < 0.01, "{dir}: the walk reads {walk:.2}° where the band reads {band:.2}°");
        }
    }

    /// Where they differ, and why. On the step the band reads 113.6° and the
    /// walk 79.9° at the same place, the second level's corner at (85, 11):
    /// the band prices a level against the level DEEPER — here a segment
    /// whose swath ends in a stadium — and charges the sharp corner the whole
    /// crescent; the walk measures what the stadium actually left. (Until
    /// Phase 58 slice 1 the step's levels were arcs about the blank's open
    /// corners, the deepest shorter than the next, and the band read a slot
    /// where a loop's last edge met what its first edge swept: 180° to the
    /// walk's 148.9°.) The band is the cautious one, which is the safe way
    /// to be wrong for a feed.
    #[test]
    fn a_loop_that_closes_on_itself_meets_its_own_beginning() {
        let (walk, band) = both("step.cam");
        assert!((band - 113.58).abs() < 0.01 && (walk - 79.91).abs() < 0.01, "walk {walk:.2}, band {band:.2}");
    }

    /// The same difference among twelve posts, seen once the band could see
    /// it (Phase 37 slice 1). Between the outer posts and the wall the
    /// second-deepest level is a loop smaller than the tool, and the deepest
    /// level never entered that lane: the loop is cut from solid, the band
    /// says 180° and it is right about the LOOP. Its first move reads 180° on
    /// the walk too. By the vertex the band happens to name, the loop has
    /// swept its own beginning and the walk reads 24.44° there. (Before the
    /// containment fix the band took this lane for floor already cut, and
    /// named its first slot a level later.)
    #[test]
    fn a_loop_smaller_than_the_tool_is_a_slot_the_band_names_late() {
        let (walk, band) = both("post_field.cam");
        assert!((band - 180.0).abs() < 0.01 && (walk - 24.44).abs() < 0.01, "walk {walk:.2}, band {band:.2}");
        let p = crate::parse::load_project("post_field.cam").unwrap();
        let op = &p.operations[0];
        let at = crate::parse::op_engagement(&p, op).unwrap().worst.at;
        let moves = floor_moves(&p, 0, Pattern::Offset).unwrap();
        let before = crate::stock::remaining_before(&p, 0);
        let bounds = crate::engagement::Bounds::of(&before).unwrap();
        let floor_z = crate::region::of(&p, op).unwrap().z;
        let walk = crate::engagement::Walk::of(&moves, p.tools[&op.tool_num].dia / 2.0);
        // The same loop, a tool's width round the point the band named.
        let on_the_loop = moves.iter().enumerate().filter(|(_, (a, _))| (a.x - at.0).phypot(a.y - at.1) < 6.0);
        let worst = on_the_loop.map(|(k, _)| walk.engagement(k, 0.5, &bounds, &|q| before.material_at(q.0, q.1, floor_z + 0.01))).fold(0.0, f64::max);
        assert!((worst.to_degrees() - 180.0).abs() < 0.01, "the walk's worst on that loop: {:.2}", worst.to_degrees());
    }

    /// Finding E, closed (`DECISIONS.md` → "Phase 34 slice 3…" and "Phase 37
    /// slice 1…"): beside a post the band reads what the arithmetic says. At
    /// (71, 50) on `pocket_post` the tool runs up the wall-parallel loop with
    /// one of the deepest level's corner loops ahead of it and one behind.
    /// The corner ahead ends at (67, 58.485), and its swath takes an arc out
    /// of the leading half that is plain trigonometry: the points of the
    /// tool's circle within a radius of that corner. 180° less that arc is
    /// 139.46°. The walk reads it, and so does the band, which read 94.97°.
    #[test]
    fn the_band_and_the_walk_read_the_arithmetic_beside_a_post() {
        let (walk, band) = both("pocket_post.cam");
        // |q - corner|^2 < r^2 on the circle about (71, 50):
        //   113 + 40 cos(a) - 84.85 sin(a) < 25, corner at (-4, +8.485).
        let (dx, dy, r) = (67.0 - 71.0_f64, 58.485 - 50.0_f64, 5.0_f64);
        let half = ((dx * dx + dy * dy) / (2.0 * r * dx.phypot(dy))).pacos();
        let by_hand = 180.0 - 2.0 * half.to_degrees();
        assert!((walk - by_hand).abs() < 0.05, "the walk {walk:.2} against the arithmetic {by_hand:.2}");
        assert!((band - by_hand).abs() < 0.05, "the band {band:.2} against the arithmetic {by_hand:.2}");
    }
}
