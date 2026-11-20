//! The arc-measure engagement primitive — Phase 8's V1, slice 0
//! (`DECISIONS.md` → "V1, the engagement measure…").
//!
//! Engagement is the angular span, at one tool position, over which a flute is
//! actually in uncut material — entry to exit. It is the quantity the
//! `parse::engagement_findings` floor stands in for with a scalar
//! allowance-vs-diameter ratio, and the quantity the chip-thinning feed cannot
//! be computed without: the correction is `1 / sin(theta_e)`, so the angle is
//! not an input to the feed, it *is* the feed's argument.
//!
//! **Closed form, never sampled** (the constraint committed 2026-07-29): every
//! boundary a tool meets here is lines and arcs, so a crossing is a
//! circle-vs-line or circle-vs-circle solve. We use `cavalier_contours`' own
//! `line_circle_intr` / `circle_circle_intr` rather than hand-rolling a fourth
//! variant of a routine `parse.rs` already carries three of — no new
//! dependency, just more of a crate already in the annotated ledger.
//!
//! **What "material" is, and why no arc appears in the test.** After a pass
//! whose tool centers ride `d_prev` inside the authored wall, the material that
//! stands is the pocket *eroded* by `d_prev` — and erosion by a disc is exactly
//! a distance sublevel set. So `in_material` is "inside the authored polygon,
//! and at least `d` from its boundary": two exact, elementary tests over
//! straight segments. The eroded boundary's own arcs (which appear at reflex
//! vertices, where the nearest boundary feature is the vertex itself) are still
//! solved exactly for the *crossings* — they just never have to be represented.
//!
//! **The leading-half convention.** A point of the tool circle that lies in
//! material behind the tool has already been swept by this same pass, so the
//! engaged span is the material arc restricted to the leading half-circle. On a
//! locally straight path that is exact and reproduces the textbook
//! `theta_e = acos(1 - 2*a_e/D)` identically; where the path curves it stands in
//! for "not yet swept by this pass", which is the one modelling choice in here
//! and is named rather than hidden.

use crate::math::Portable as _;
use crate::offset::{arc_center, offset_closed_polyline_verts, OffsetVertex};
use crate::parse::{Finding, FindingFamily, Part, Pt};
use cavalier_contours::core::math::{
    circle_circle_intr, line_circle_intr, CircleCircleIntr, LineCircleIntr, Vector2,
};

/// Fuzzy budget handed to the crate's intersect routines, and the slack on the
/// "is this candidate actually on the eroded boundary" filter. Not a geometric
/// tolerance in the `CHORD_TOL_MM` sense — nothing is being approximated within
/// it; it only decides when two exactly-computed reals count as equal, so it
/// stays a constant here rather than becoming a named budget.
const EPS: f64 = 1e-9;

/// Slack on the distance filter. Looser than `EPS` because a candidate point is
/// the output of one intersect solve and its distance is a second, independent
/// computation — the two agree to roughly the conditioning of the geometry, not
/// to the last bit.
const DIST_TOL: f64 = 1e-7;

/// One tool position's engagement, and where it was measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Engagement {
    /// The engaged span in radians — entry to exit, the textbook `theta_e`.
    pub radians: f64,
    /// Tool-center position where it was measured.
    pub at: (f64, f64),
    /// Which clearing pass (0 = the boundary pass that rides the wall).
    pub pass: usize,
}

/// A pocket's engagement over its whole clearing sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct PocketEngagement {
    /// What the declared stepover implies on a straight run: `acos(1 - a_e/r)`.
    /// The number an author who reasons in stepover believes they asked for.
    pub nominal: f64,
    /// The worst engagement any pass AFTER the slot actually reaches.
    pub worst: Engagement,
    /// Every pass's worst, **in emission order** — pass 0 is the innermost
    /// loop, cut from solid (a full slot, `pi` by construction), and the last
    /// is the wall pass. One entry per offset level; a level the offsetter
    /// split into several loops reports the worst over all of them. This is
    /// what a per-pass feed reads (Phase 9 lane A, slice 1).
    pub passes: Vec<Engagement>,
}

// ---------------------------------------------------------------------------
// Chip thinning: the arc measure, spent.
// ---------------------------------------------------------------------------

/// Ceiling on the chip-thinning feed correction — a named budget with one
/// home, per the one-tolerance-one-home rule, not an inline literal.
///
/// `1/sin(theta_e)` diverges as engagement approaches zero: a whisper-thin cut
/// asks for an unbounded feed, and no table's chip load was measured out there.
/// 2.0 is the angle `theta_e = 30 degrees`, which is a radial engagement of
/// about 6.7% of diameter. It is chosen so the cap does **not** bind on the
/// workflow this item exists to serve: the author's own manual loop targets an
/// allowance near 10% of the finisher's diameter, which lands at
/// `theta_e = 36.9 degrees` and a factor of 1.667 — comfortably under. Below
/// 6.7% immersion a pass is a finishing wisp, and the honest answer there is
/// not a bigger multiplier but the **rubbing threshold**, which belongs to the
/// MRR band's thread and is deliberately not guessed at here.
///
/// The value is the movable part; that it is capped, named, and stated in the
/// program text when it binds is the part that is not.
pub const CHIP_THINNING_CAP: f64 = 2.0;

/// A feed correction, and enough of its provenance to state in program text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChipThinning {
    /// The multiplier applied to the chip-load feed identity.
    pub factor: f64,
    /// The engagement it was computed from, radians.
    pub engagement: f64,
    /// Whether `CHIP_THINNING_CAP` bound — stated out loud where it did, so a
    /// reader sees the number the engine rejected to exceed rather than
    /// silently receiving a clamped one (the residue law's precedent).
    pub capped: bool,
}

/// The radial chip-thinning correction for an engagement angle.
///
/// Substituting `cos(theta_e) = 1 - 2*a_e/D` into the standard correction
/// `D / (2*sqrt(a_e*(D - a_e)))` collapses it to exactly `1 / sin(theta_e)` —
/// so the arc measure is not an input to the feed, it *is* the feed's argument.
/// Verified against the standard form across the immersion range.
///
/// **The clamp is on the ANGLE, never on the factor's value.** Past half
/// immersion `1/sin` turns around and rises again by the sine's mirror — it
/// reads 1.0206 at both 40% and 60% immersion and runs to infinity at a full
/// slot. Clamping the value would let that mirror through as a legitimate
/// boost exactly as the tool buries itself, which is precisely backwards.
/// Clamping the angle at 90 degrees makes the factor 1 and keeps it there.
pub fn chip_thinning(theta_e: f64) -> ChipThinning {
    // No cut, no correction — a zero engagement is not an infinite feed.
    if theta_e <= EPS {
        return ChipThinning { factor: 1.0, engagement: theta_e, capped: false };
    }
    let raw = 1.0 / theta_e.min(std::f64::consts::FRAC_PI_2).psin();
    ChipThinning {
        factor: raw.min(CHIP_THINNING_CAP),
        engagement: theta_e,
        capped: raw > CHIP_THINNING_CAP,
    }
}

// ---------------------------------------------------------------------------
// The material region: a pocket eroded by a distance.
// ---------------------------------------------------------------------------

/// Distance from `q` to the closed polygon's boundary — the minimum over its
/// segments, each a point-to-segment distance with the projection clamped to
/// the segment (so a vertex answers for its own neighbourhood, which is exactly
/// the reflex-vertex arc of the eroded boundary).
fn dist_to_boundary(q: (f64, f64), poly: &[Pt]) -> f64 {
    let n = poly.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let a = &poly[i];
        let b = &poly[(i + 1) % n];
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 <= EPS {
            0.0
        } else {
            (((q.0 - a.x) * dx + (q.1 - a.y) * dy) / len2).clamp(0.0, 1.0)
        };
        let (px, py) = (a.x + t * dx, a.y + t * dy);
        let d = ((q.0 - px).powi(2) + (q.1 - py).powi(2)).sqrt();
        if d < best {
            best = d;
        }
    }
    best
}

/// Ray-cast containment for the authored polygon. Straight segments only —
/// the authored boundary of a `ProfilePocket` is a point list, so there is no
/// arc to handle here and none is invented.
fn inside(q: (f64, f64), poly: &[Pt]) -> bool {
    let n = poly.len();
    let mut winding = false;
    for i in 0..n {
        let a = &poly[i];
        let b = &poly[(i + 1) % n];
        if (a.y > q.1) != (b.y > q.1) {
            let x_cross = a.x + (q.1 - a.y) / (b.y - a.y) * (b.x - a.x);
            if q.0 < x_cross {
                winding = !winding;
            }
        }
    }
    winding
}

/// The material standing after a pass whose tool centers rode `d` inside the
/// wall: the pocket eroded by `d`. Erosion by a disc *is* the distance sublevel
/// set, which is why this needs no offset geometry to answer.
fn in_material(q: (f64, f64), poly: &[Pt], d: f64) -> bool {
    inside(q, poly) && dist_to_boundary(q, poly) >= d - DIST_TOL
}

// ---------------------------------------------------------------------------
// The swath: what the inner loops have already cut, exactly.
// ---------------------------------------------------------------------------
//
// A pocket is cleared INSIDE-OUT (`ops::clear` reverses its offset list and
// ramps into the innermost loop), so the material a pass meets is the pocket
// eroded by the skin MINUS everything the inner loops swept. That swept region
// is the region inside the previous (inner) level's loops, dilated by the tool
// radius: every point inside those loops was swept by a still-more-inner loop
// (consecutive levels stand `stepover < dia` apart and the island error
// guarantees the core is reached), and the band outside them is swept to one
// radius. It is NOT the pocket eroded by `offset - r` — the first build of
// this slice assumed so, and the L-shaped fixtures showed why that is wrong:
// where a narrow leg is thinner than the inner level's offset, that level has
// no loop there at all, the leg is still solid, and the next loop cuts it
// FROM SOLID (a 180° slot on an "interior" pass). A global erosion reads the
// leg's centre strip as already cut — 156.4° where the truth is 180°, and an
// under-read of engagement is the unsafe direction for a thinned feed.

/// The arc on the segment leaving `v1` toward `v2`: centre, radius, start
/// angle, signed sweep (positive = CCW).
fn arc_geom(v1: &OffsetVertex, v2: &OffsetVertex) -> ((f64, f64), f64, f64, f64) {
    let (cx, cy) = arc_center(v1, v2);
    let rad = ((v1.x - cx).powi(2) + (v1.y - cy).powi(2)).sqrt();
    let a1 = (v1.y - cy).patan2(v1.x - cx);
    let a2 = (v2.y - cy).patan2(v2.x - cx);
    let mut sweep = a2 - a1;
    if v1.bulge > 0.0 {
        while sweep <= 0.0 {
            sweep += std::f64::consts::TAU;
        }
    } else {
        while sweep >= 0.0 {
            sweep -= std::f64::consts::TAU;
        }
    }
    ((cx, cy), rad, a1, sweep)
}

/// Is angle `a` (about the arc's centre) within the arc's sweep from `a1`?
fn within_sweep(a: f64, a1: f64, sweep: f64) -> bool {
    let rel = norm(a - a1);
    if sweep >= 0.0 {
        rel <= sweep + EPS
    } else {
        rel >= std::f64::consts::TAU + sweep - EPS || rel <= EPS
    }
}

/// Distance from `q` to a closed offset loop — straight segments and bulge
/// arcs, each answering for its own neighbourhood.
fn dist_to_loop(q: (f64, f64), lp: &[OffsetVertex]) -> f64 {
    let n = lp.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let v1 = &lp[i];
        let v2 = &lp[(i + 1) % n];
        let d = if v1.bulge.abs() <= EPS {
            let (dx, dy) = (v2.x - v1.x, v2.y - v1.y);
            let len2 = dx * dx + dy * dy;
            let t = if len2 <= EPS {
                0.0
            } else {
                (((q.0 - v1.x) * dx + (q.1 - v1.y) * dy) / len2).clamp(0.0, 1.0)
            };
            ((q.0 - v1.x - t * dx).powi(2) + (q.1 - v1.y - t * dy).powi(2)).sqrt()
        } else {
            let ((cx, cy), rad, a1, sweep) = arc_geom(v1, v2);
            let a = (q.1 - cy).patan2(q.0 - cx);
            if within_sweep(a, a1, sweep) {
                (((q.0 - cx).powi(2) + (q.1 - cy).powi(2)).sqrt() - rad).abs()
            } else {
                let d1 = ((q.0 - v1.x).powi(2) + (q.1 - v1.y).powi(2)).sqrt();
                let d2 = ((q.0 - v2.x).powi(2) + (q.1 - v2.y).powi(2)).sqrt();
                d1.min(d2)
            }
        };
        if d < best {
            best = d;
        }
    }
    best
}

/// Exact containment in a closed offset loop with bulge arcs. The offsetting
/// crate's own winding number, through `offset.rs` (Phase 37 slice 1;
/// `DECISIONS.md` → "Phase 37 slice 1…"). The predicate that stood here was
/// hand-written — ray-cast the chords, flip for each arc's sliver — and chose
/// the CENTRE's side of the chord where the sliver is on the arc's.
fn inside_loop(q: (f64, f64), lp: &[OffsetVertex]) -> bool {
    crate::offset::loop_contains(lp, q)
}

/// Has `q` been swept by the loops of the inner level, with a tool of radius
/// `r`? Inside any of them (swept by a still-more-inner loop), or within `r`
/// of one (swept by this one).
/// Has `q` been swept by the loops of the deeper level, with a tool of radius
/// `r`? For a pocket with ISLANDS (Phase 26 slice 2) the deeper
/// level is then wall-parallel loops (`inner`, CCW) and island-parallel ones
/// (`around`, CW), and "deeper" lies inside the first kind and OUTSIDE the
/// second: a point between an island and the loop that circles it has not been
/// reached yet — it is the material the next island pass is for. Within `r` of
/// either kind is swept by that loop itself. `around` empty is the plain pocket.
fn swept_around(q: (f64, f64), inner: &[Vec<OffsetVertex>], around: &[Vec<OffsetVertex>], r: f64) -> bool {
    let near = |lp: &Vec<OffsetVertex>| dist_to_loop(q, lp) < r - DIST_TOL;
    inner.iter().chain(around).any(near)
        || (inner.iter().any(|lp| inside_loop(q, lp)) && !around.iter().any(|lp| inside_loop(q, lp)))
}

/// The material a pass meets: inside the pocket eroded by the skin, and not
/// yet swept by the deeper level's loops. Both lists empty is the deepest
/// loop, cut from solid.
fn in_band(q: (f64, f64), poly: &[Pt], skin: f64, inner: &[Vec<OffsetVertex>], around: &[Vec<OffsetVertex>], r: f64) -> bool {
    in_material(q, poly, skin) && !swept_around(q, inner, around, r)
}

/// Every angle (about the tool centre) at which the tool circle crosses the
/// swath's outer edge — the inner loops dilated by `r`. Candidates come from
/// the families that edge is made of (a parallel at `r` to each straight
/// segment, a circle of radius `r` about each vertex, a concentric circle at
/// `rad ± r` for each arc) and survive only if they really stand `r` from the
/// loops — the same enumerate-then-filter soundness `crossing_angles` uses.
fn swath_crossings(c: (f64, f64), r_tool: f64, inner: &[Vec<OffsetVertex>], r: f64) -> Vec<f64> {
    let center = Vector2::new(c.0, c.1);
    let mut out: Vec<f64> = Vec::new();
    let mut push = |x: f64, y: f64| {
        let d = inner.iter().map(|lp| dist_to_loop((x, y), lp)).fold(f64::INFINITY, f64::min);
        if (d - r).abs() <= DIST_TOL {
            out.push((y - c.1).patan2(x - c.0));
        }
    };
    for lp in inner {
        let n = lp.len();
        for i in 0..n {
            let v1 = &lp[i];
            let v2 = &lp[(i + 1) % n];
            if v1.bulge.abs() <= EPS {
                let (dx, dy) = (v2.x - v1.x, v2.y - v1.y);
                let len = (dx * dx + dy * dy).sqrt();
                if len <= EPS {
                    continue;
                }
                let (nx, ny) = (-dy / len, dx / len);
                for sgn in [1.0, -1.0] {
                    let p0 = Vector2::new(v1.x + sgn * r * nx, v1.y + sgn * r * ny);
                    let p1 = Vector2::new(v2.x + sgn * r * nx, v2.y + sgn * r * ny);
                    match line_circle_intr(p0, p1, r_tool, center, EPS) {
                        LineCircleIntr::NoIntersect => {}
                        LineCircleIntr::TangentIntersect { t0 } => {
                            push(p0.x + t0 * (p1.x - p0.x), p0.y + t0 * (p1.y - p0.y));
                        }
                        LineCircleIntr::TwoIntersects { t0, t1 } => {
                            for t in [t0, t1] {
                                push(p0.x + t * (p1.x - p0.x), p0.y + t * (p1.y - p0.y));
                            }
                        }
                    }
                }
            } else {
                let ((cx, cy), rad, _, _) = arc_geom(v1, v2);
                for rr in [rad + r, rad - r] {
                    if rr <= EPS {
                        continue;
                    }
                    match circle_circle_intr(r_tool, center, rr, Vector2::new(cx, cy), EPS) {
                        CircleCircleIntr::NoIntersect | CircleCircleIntr::Overlapping => {}
                        CircleCircleIntr::TangentIntersect { point } => push(point.x, point.y),
                        CircleCircleIntr::TwoIntersects { point1, point2 } => {
                            push(point1.x, point1.y);
                            push(point2.x, point2.y);
                        }
                    }
                }
            }
            // The vertex's own cap.
            match circle_circle_intr(r_tool, center, r, Vector2::new(v1.x, v1.y), EPS) {
                CircleCircleIntr::NoIntersect | CircleCircleIntr::Overlapping => {}
                CircleCircleIntr::TangentIntersect { point } => push(point.x, point.y),
                CircleCircleIntr::TwoIntersects { point1, point2 } => {
                    push(point1.x, point1.y);
                    push(point2.x, point2.y);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The crossings: circle vs. the eroded boundary, exactly.
// ---------------------------------------------------------------------------

/// Every angle (about the tool center) at which the tool circle crosses the
/// eroded boundary.
///
/// Candidates come from the two families the eroded boundary is made of — a
/// line parallel to each authored segment at distance `d` inward, and a circle
/// of radius `d` about each authored vertex — and are then filtered by the
/// exact distance test. That filter is what makes the enumeration sound without
/// having to classify vertices as convex or reflex first: a candidate survives
/// only if it really does stand `d` from the boundary, whichever feature is
/// nearest to it.
fn crossing_angles(c: (f64, f64), r: f64, poly: &[Pt], d: f64) -> Vec<f64> {
    let center = Vector2::new(c.0, c.1);
    let mut out: Vec<f64> = Vec::new();
    let mut push = |x: f64, y: f64| {
        if (dist_to_boundary((x, y), poly) - d).abs() <= DIST_TOL {
            out.push((y - c.1).patan2(x - c.0));
        }
    };

    let n = poly.len();
    for i in 0..n {
        let a = &poly[i];
        let b = &poly[(i + 1) % n];
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= EPS {
            continue;
        }
        // Inward normal of a CCW ring.
        let (nx, ny) = (-dy / len, dx / len);
        let p0 = Vector2::new(a.x + d * nx, a.y + d * ny);
        let p1 = Vector2::new(b.x + d * nx, b.y + d * ny);
        // `t` outside [0,1] is accepted on purpose: the segment's own parallel
        // may cross the tool circle past its ends, and whether that point is
        // really on the eroded boundary is the distance filter's question, not
        // the parameter range's.
        match line_circle_intr(p0, p1, r, center, EPS) {
            LineCircleIntr::NoIntersect => {}
            LineCircleIntr::TangentIntersect { t0 } => {
                push(p0.x + t0 * (p1.x - p0.x), p0.y + t0 * (p1.y - p0.y));
            }
            LineCircleIntr::TwoIntersects { t0, t1 } => {
                for t in [t0, t1] {
                    push(p0.x + t * (p1.x - p0.x), p0.y + t * (p1.y - p0.y));
                }
            }
        }
    }

    for v in poly.iter() {
        match circle_circle_intr(r, center, d, Vector2::new(v.x, v.y), EPS) {
            CircleCircleIntr::NoIntersect | CircleCircleIntr::Overlapping => {}
            CircleCircleIntr::TangentIntersect { point } => push(point.x, point.y),
            CircleCircleIntr::TwoIntersects { point1, point2 } => {
                push(point1.x, point1.y);
                push(point2.x, point2.y);
            }
        }
    }

    out
}

/// Normalize to `[0, 2*pi)`.
fn norm(a: f64) -> f64 {
    let t = a % std::f64::consts::TAU;
    if t < 0.0 {
        t + std::f64::consts::TAU
    } else {
        t
    }
}

/// The engaged span at one tool position, in radians.
///
/// `travel` is the unit direction of motion; `d` is how far inside the wall the
/// *previous* pass rode, so `poly` eroded by `d` is the material this position
/// is cutting into.
pub fn engagement_at(
    c: (f64, f64),
    r: f64,
    travel: (f64, f64),
    poly: &[Pt],
    d: f64,
) -> f64 {
    engagement_in(c, r, travel, poly, d, &[])
}

/// The engaged span at one tool position against the inside-out material —
/// the pocket eroded by `skin`, minus the swath of the `inner` level's loops
/// (`engagement_at` is the case with no inner loops). The crossings are the
/// union of the circle's crossings with the eroded boundary and with the
/// swath's edge; each interval between crossings is classified by its
/// midpoint, exactly as before.
pub fn engagement_in(
    c: (f64, f64),
    r: f64,
    travel: (f64, f64),
    poly: &[Pt],
    skin: f64,
    inner: &[Vec<OffsetVertex>],
) -> f64 {
    engagement_around(c, r, travel, poly, skin, inner, &[])
}

/// `engagement_in` for a pocket with islands: `around` is the deeper level's
/// island-parallel loops (`swept_around`). The islands themselves add no
/// crossing of their own — a pass rides `r + skin` off every wall, so the tool
/// circle reaches an island's skin boundary at a tangent and never crosses it;
/// what the islands change is WHERE the loops are and what they have swept.
#[allow(clippy::too_many_arguments)]
pub fn engagement_around(
    c: (f64, f64),
    r: f64,
    travel: (f64, f64),
    poly: &[Pt],
    skin: f64,
    inner: &[Vec<OffsetVertex>],
    around: &[Vec<OffsetVertex>],
) -> f64 {
    engagement_in_blank(c, r, travel, poly, skin, inner, around, None)
}

/// `engagement_around` for a region with OPEN edges (Phase 27 slice 2): `poly`
/// is then the region pushed out past the blank, and beyond the blank there is
/// nothing to cut — so the material is what the pocket holds AND the blank
/// has, and the blank's boundary is one more curve the tool circle can cross.
/// `blank` absent is `engagement_around`, exactly.
#[allow(clippy::too_many_arguments)]
pub fn engagement_in_blank(
    c: (f64, f64),
    r: f64,
    travel: (f64, f64),
    poly: &[Pt],
    skin: f64,
    inner: &[Vec<OffsetVertex>],
    around: &[Vec<OffsetVertex>],
    blank: Option<&[Pt]>,
) -> f64 {
    let mut angles: Vec<f64> = crossing_angles(c, r, poly, skin).into_iter().map(norm).collect();
    if let Some(b) = blank {
        angles.extend(crossing_angles(c, r, b, 0.0).into_iter().map(norm));
    }
    if around.is_empty() {
        angles.extend(swath_crossings(c, r, inner, r).into_iter().map(norm));
    } else {
        // The swath's edge is `r` off the NEAREST deeper loop of either kind,
        // so the candidates are filtered against both together.
        let all: Vec<Vec<OffsetVertex>> = inner.iter().chain(around).cloned().collect();
        angles.extend(swath_crossings(c, r, &all, r).into_iter().map(norm));
    }
    // The leading half-circle's own two boundaries: a point behind the tool is
    // material this pass has already swept, not material it is about to cut.
    let heading = travel.1.patan2(travel.0);
    angles.push(norm(heading + std::f64::consts::FRAC_PI_2));
    angles.push(norm(heading - std::f64::consts::FRAC_PI_2));

    angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
    angles.dedup_by(|a, b| (*a - *b).abs() <= EPS);
    if angles.is_empty() {
        return 0.0;
    }

    let mut total = 0.0;
    for i in 0..angles.len() {
        let a0 = angles[i];
        let a1 = if i + 1 < angles.len() {
            angles[i + 1]
        } else {
            angles[0] + std::f64::consts::TAU
        };
        let span = a1 - a0;
        if span <= EPS {
            continue;
        }
        let mid = a0 + span / 2.0;
        let q = (c.0 + r * mid.pcos(), c.1 + r * mid.psin());
        let leading = (q.0 - c.0) * travel.0 + (q.1 - c.1) * travel.1 >= 0.0;
        if leading && in_band(q, poly, skin, inner, around, r) && blank.map_or(true, |b| inside(q, b)) {
            total += span;
        }
    }
    total
}

// ---------------------------------------------------------------------------
// The pocket driver.
// ---------------------------------------------------------------------------

/// Positions to evaluate along one pass loop, each with its travel direction.
///
/// A pass loop is straight segments and arcs. On a straight run parallel to a
/// straight material boundary the engagement is constant, and on an arc
/// concentric with the boundary's arc it is likewise constant — so the extremes
/// live at the loop's own breakpoints. We evaluate at every vertex and at every
/// segment's midpoint: exhaustive over the geometry's breakpoints for a
/// polygonal pocket, rather than a sweep at some step.
fn pass_positions(loop_verts: &[OffsetVertex]) -> Vec<((f64, f64), (f64, f64))> {
    let n = loop_verts.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    for i in 0..n {
        let v1 = &loop_verts[i];
        let v2 = &loop_verts[(i + 1) % n];
        if v1.bulge.abs() <= EPS {
            let (dx, dy) = (v2.x - v1.x, v2.y - v1.y);
            let len = (dx * dx + dy * dy).sqrt();
            if len <= EPS {
                continue;
            }
            let dir = (dx / len, dy / len);
            out.push(((v1.x, v1.y), dir));
            out.push((((v1.x + v2.x) / 2.0, (v1.y + v2.y) / 2.0), dir));
        } else {
            let (cx, cy) = arc_center(v1, v2);
            let rad = ((v1.x - cx).powi(2) + (v1.y - cy).powi(2)).sqrt();
            if rad <= EPS {
                continue;
            }
            let ccw = v1.bulge > 0.0;
            let a1 = (v1.y - cy).patan2(v1.x - cx);
            let a2 = (v2.y - cy).patan2(v2.x - cx);
            let mut sweep = a2 - a1;
            if ccw {
                while sweep <= 0.0 {
                    sweep += std::f64::consts::TAU;
                }
            } else {
                while sweep >= 0.0 {
                    sweep -= std::f64::consts::TAU;
                }
            }
            // Tangent, oriented by the sweep's sense.
            let tangent = |a: f64| {
                if ccw {
                    (-a.psin(), a.pcos())
                } else {
                    (a.psin(), -a.pcos())
                }
            };
            out.push(((v1.x, v1.y), tangent(a1)));
            let am = a1 + sweep / 2.0;
            out.push(((cx + rad * am.pcos(), cy + rad * am.psin()), tangent(am)));
        }
    }
    out
}

/// The engagement a pocket's clearing sequence actually reaches, against the
/// engagement its declared stepover implies.
///
/// The pass sequence is re-derived here from declared fields rather than
/// observed from emitted moves: `ops::clear` walks the same offsets from the
/// same authored boundary, so this is the same geometry computed the same way,
/// and it stays callable before anything is emitted (which is what lets the
/// derived feed, resolved at parse time, ever consume it).
///
/// **The passes are walked in the order the emitter cuts them: inside-out**
/// (Phase 9 lane A, slice 1 — `DECISIONS.md` → "Phase 9 lane A, slice 1…").
/// The first build of this primitive walked the offsets outside-in, boundary
/// first, each pass meeting what the previous OUTER pass left; `ops::clear`
/// reverses its offset list and ramps into the innermost loop, so the slot is
/// the innermost loop and every later loop meets the band between the skin
/// and the edge the inner swaths already reached. On a straight run the two
/// orders agree exactly (the band is `stepover` wide either way); at a corner
/// they need not, and a per-pass feed has to price the loop the machine runs,
/// not its mirror.
///
/// Pass 0 — the slot — is judged for its CHIP (it is `pi` by construction and
/// the band's ceiling reads it) and deliberately **not** for the corner
/// advisory: clearing a pocket from solid means one full-width pass, a fact
/// about pockets rather than a finding, and `worst` skips it so that advisory
/// stays quiet on every ordinary pocket.
pub fn pocket_engagement(
    points: &[Pt],
    r_tool: f64,
    skin: f64,
    stepover: f64,
) -> Option<PocketEngagement> {
    if points.len() < 3 || r_tool <= 0.0 || stepover <= 0.0 {
        return None;
    }
    let nominal = (1.0 - stepover / r_tool).clamp(-1.0, 1.0).pacos();

    // The offset family exactly as the emitter builds it, outermost first…
    let mut levels: Vec<(f64, Vec<Vec<OffsetVertex>>)> = Vec::new();
    let mut offset_dist = r_tool + skin;
    loop {
        let loops = offset_closed_polyline_verts(points, offset_dist);
        if loops.is_empty() {
            break;
        }
        levels.push((offset_dist, loops));
        offset_dist += stepover;
    }
    if levels.is_empty() {
        return None;
    }
    // …then reversed, which is the emission order.
    levels.reverse();

    let mut passes: Vec<Engagement> = Vec::new();
    let mut worst: Option<Engagement> = None;
    let empty: Vec<Vec<OffsetVertex>> = Vec::new();
    for (pass, (_, loops)) in levels.iter().enumerate() {
        // What this pass meets: the material the previous (inner) level's
        // loops did not sweep. The innermost loop has no inner level — it is
        // cut from solid.
        let inner = if pass == 0 { &empty } else { &levels[pass - 1].1 };
        let mut level_worst: Option<Engagement> = None;
        for l in loops {
            for (c, travel) in pass_positions(l) {
                let e = engagement_in(c, r_tool, travel, points, skin, inner);
                if level_worst.as_ref().map_or(true, |w| e > w.radians) {
                    level_worst = Some(Engagement { radians: e, at: c, pass });
                }
            }
        }
        let Some(lw) = level_worst else { continue };
        if pass > 0 && worst.as_ref().map_or(true, |w| lw.radians > w.radians) {
            worst = Some(lw);
        }
        passes.push(lw);
    }

    worst.map(|worst| PocketEngagement { nominal, worst, passes })
}

/// `pocket_engagement` for a pocket that goes around islands (Phase 26 slice
/// 2): the same walk over the same family the emitter cuts —
/// `offset::shape_levels`, deepest level first, every loop of a level judged
/// and the level priced by its worst. Pass 0 is the deepest level, cut from
/// solid, exactly as the plain pocket's slot is.
pub fn islanded_pocket_engagement(
    points: &[Pt],
    islands: &[crate::offset::Island],
    r_tool: f64,
    skin: f64,
    stepover: f64,
) -> Option<PocketEngagement> {
    region_pocket_engagement(points, islands, None, r_tool, skin, stepover)
}

/// The same walk for any region the pocket arm cuts on the shape family:
/// `points` is the boundary the family erodes from (`Region::family_wall` —
/// pushed out past the blank across open edges), and `blank` is given exactly
/// when there are open edges, so a pass riding outside the stock is priced as
/// the air it is.
pub fn region_pocket_engagement(
    points: &[Pt],
    islands: &[crate::offset::Island],
    blank: Option<&[Pt]>,
    r_tool: f64,
    skin: f64,
    stepover: f64,
) -> Option<PocketEngagement> {
    if points.len() < 3 || r_tool <= 0.0 || stepover <= 0.0 {
        return None;
    }
    let nominal = (1.0 - stepover / r_tool).clamp(-1.0, 1.0).pacos();
    let mut levels = crate::offset::shape_levels(points, islands, r_tool + skin, stepover);
    if levels.is_empty() {
        return None;
    }
    levels.reverse();

    let mut passes: Vec<Engagement> = Vec::new();
    let mut worst: Option<Engagement> = None;
    let none = crate::offset::ShapeLevel::default();
    for (pass, (_, level)) in levels.iter().enumerate() {
        let deeper = if pass == 0 { &none } else { &levels[pass - 1].1 };
        let mut level_worst: Option<Engagement> = None;
        for l in level.ccw.iter().chain(&level.cw) {
            for (c, travel) in pass_positions(l) {
                let e = engagement_in_blank(c, r_tool, travel, points, skin, &deeper.ccw, &deeper.cw, blank);
                if level_worst.as_ref().map_or(true, |w| e > w.radians) {
                    level_worst = Some(Engagement { radians: e, at: c, pass });
                }
            }
        }
        let Some(lw) = level_worst else { continue };
        if pass > 0 && worst.as_ref().map_or(true, |w| lw.radians > w.radians) {
            worst = Some(lw);
        }
        passes.push(lw);
    }
    worst.map(|worst| PocketEngagement { nominal, worst, passes })
}

// ---------------------------------------------------------------------------
// The measure from the moves (Phase 34 slice 3).
// ---------------------------------------------------------------------------
//
// Everything above prices the OFFSET FAMILY: "already cut" is the region
// inside the deeper level's loops, dilated by the tool's radius, and that
// definition is the family's. Parallel rows, a rail pass, a shore pass have
// no deeper level. What every path has is the moves it already made, and the
// material a tool meets at one of its positions is the stock the op began
// with, less every earlier move of the same path swept by the tool's disc —
// a STADIUM per move: two parallels a radius off it and a cap at each end.
//
// The crossings are the same two solves as above, circle against line and
// circle against circle, so this stays closed form (the 2026-07-29
// constraint) and needs no crate. It enumerates candidates and classifies
// each interval between them by its midpoint, as `engagement_in_blank` does.
// A candidate that is not really on a boundary only splits an interval in
// two and changes nothing, so the candidates are never filtered; what must
// not happen is a MISSED boundary, which is why a stock model holding a
// footprint this cannot solve declines the whole measure (`Bounds::of`).
//
// It judges the PROGRAM, not a model of the pattern: a link between two
// loops is a move, and what it swept is swept.

/// The curves that bound the material an op began with, in plan: the blank,
/// and the footprint of every removal an earlier op declared. Straight edges
/// and whole circles — the two things the tool's circle can be solved against.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bounds {
    pub edges: Vec<(Pt, Pt)>,
    pub circles: Vec<((f64, f64), f64)>,
}

impl Bounds {
    /// `None` where an earlier removal's footprint is neither: an ellipse, or
    /// a band off a path. The measure declines there, by the rule above.
    pub fn of(before: &crate::stock::RemainingStock) -> Option<Bounds> {
        use crate::stock::Section;
        let s = &before.stock;
        let mut b = Bounds::default();
        let mut ring = |pts: &[Pt]| {
            for i in 0..pts.len() {
                b.edges.push((pts[i], pts[(i + 1) % pts.len()]));
            }
        };
        ring(&crate::parse::derive_blank(s));
        let mut circles = Vec::new();
        for r in &before.removals {
            match &r.section {
                Section::All => {}
                Section::Disc { x, y, r } => circles.push(((*x, *y), *r)),
                Section::Annulus { x, y, r_inner, r_outer } => {
                    circles.push(((*x, *y), *r_inner));
                    circles.push(((*x, *y), *r_outer));
                }
                Section::Polygon { points, holes } => {
                    ring(points);
                    for h in holes {
                        ring(h);
                    }
                }
                Section::Ellipse { .. } | Section::PathBand { .. } | Section::ChainBand { .. } => return None,
            }
        }
        b.circles = circles;
        Some(b)
    }
}

/// A path as it was run: its at-depth moves in order, indexed so a position
/// asks only the moves near it.
pub struct Walk<'a> {
    moves: &'a [(Pt, Pt)],
    r: f64,
    cell: f64,
    grid: std::collections::HashMap<(i64, i64), Vec<usize>>,
}

pub(crate) fn dist_to_segment(q: (f64, f64), a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= EPS { 0.0 } else { (((q.0 - a.x) * dx + (q.1 - a.y) * dy) / len2).clamp(0.0, 1.0) };
    (q.0 - a.x - t * dx).phypot(q.1 - a.y - t * dy)
}

impl<'a> Walk<'a> {
    pub fn of(moves: &'a [(Pt, Pt)], r: f64) -> Self {
        let cell = (4.0 * r).max(1e-3);
        let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
        for (k, (a, b)) in moves.iter().enumerate() {
            let (i0, i1) = ((a.x.min(b.x) / cell).floor() as i64, (a.x.max(b.x) / cell).floor() as i64);
            let (j0, j1) = ((a.y.min(b.y) / cell).floor() as i64, (a.y.max(b.y) / cell).floor() as i64);
            for i in i0..=i1 {
                for j in j0..=j1 {
                    grid.entry((i, j)).or_default().push(k);
                }
            }
        }
        Walk { moves, r, cell, grid }
    }

    /// The moves BEFORE move `k` that pass within two radii of `c` — near
    /// enough for their swath to reach the tool's circle.
    fn earlier_near(&self, c: (f64, f64), k: usize) -> Vec<usize> {
        let reach = 2.0 * self.r;
        let (i0, i1) = (((c.0 - reach) / self.cell).floor() as i64, ((c.0 + reach) / self.cell).floor() as i64);
        let (j0, j1) = (((c.1 - reach) / self.cell).floor() as i64, ((c.1 + reach) / self.cell).floor() as i64);
        let mut out: Vec<usize> = Vec::new();
        for i in i0..=i1 {
            for j in j0..=j1 {
                if let Some(ks) = self.grid.get(&(i, j)) {
                    out.extend(ks.iter().filter(|j| **j < k && dist_to_segment(c, self.moves[**j].0, self.moves[**j].1) <= reach + DIST_TOL));
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The engaged span, in radians, with the tool a share `t` of the way
    /// along move `k`. `material` is the stock the op began with.
    pub fn engagement(&self, k: usize, t: f64, bounds: &Bounds, material: &dyn Fn((f64, f64)) -> bool) -> f64 {
        let (a, b) = self.moves[k];
        let len = (b.x - a.x).phypot(b.y - a.y);
        if len <= EPS {
            return 0.0;
        }
        let travel = ((b.x - a.x) / len, (b.y - a.y) / len);
        let c = (a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
        let here = Pt { x: c.0, y: c.1 };
        let r = self.r;
        let center = Vector2::new(c.0, c.1);

        // What has swept: the earlier moves near enough to matter, and the
        // part of this move already behind the tool.
        let mut swaths: Vec<(Pt, Pt)> = self.earlier_near(c, k).into_iter().map(|j| self.moves[j]).collect();
        if t > 0.0 {
            swaths.push((a, here));
        }

        let heading = travel.1.patan2(travel.0);
        let mut angles: Vec<f64> = vec![norm(heading + std::f64::consts::FRAC_PI_2), norm(heading - std::f64::consts::FRAC_PI_2)];
        let mut at = |x: f64, y: f64| angles.push(norm((y - c.1).patan2(x - c.0)));
        let mut line = |p0: Vector2<f64>, p1: Vector2<f64>| match line_circle_intr(p0, p1, r, center, EPS) {
            LineCircleIntr::NoIntersect => {}
            LineCircleIntr::TangentIntersect { t0 } => at(p0.x + t0 * (p1.x - p0.x), p0.y + t0 * (p1.y - p0.y)),
            LineCircleIntr::TwoIntersects { t0, t1 } => {
                for t in [t0, t1] {
                    at(p0.x + t * (p1.x - p0.x), p0.y + t * (p1.y - p0.y));
                }
            }
        };
        for (p, q) in &bounds.edges {
            if dist_to_segment(c, *p, *q) <= r + DIST_TOL {
                line(Vector2::new(p.x, p.y), Vector2::new(q.x, q.y));
            }
        }
        for (p, q) in &swaths {
            let (dx, dy) = (q.x - p.x, q.y - p.y);
            let l = dx.phypot(dy);
            if l <= EPS {
                continue;
            }
            let (nx, ny) = (-dy / l * r, dx / l * r);
            for sgn in [1.0, -1.0] {
                line(Vector2::new(p.x + sgn * nx, p.y + sgn * ny), Vector2::new(q.x + sgn * nx, q.y + sgn * ny));
            }
        }
        let mut circle = |o: (f64, f64), rad: f64| match circle_circle_intr(r, center, rad, Vector2::new(o.0, o.1), EPS) {
            CircleCircleIntr::NoIntersect | CircleCircleIntr::Overlapping => {}
            CircleCircleIntr::TangentIntersect { point } => angles.push(norm((point.y - c.1).patan2(point.x - c.0))),
            CircleCircleIntr::TwoIntersects { point1, point2 } => {
                angles.push(norm((point1.y - c.1).patan2(point1.x - c.0)));
                angles.push(norm((point2.y - c.1).patan2(point2.x - c.0)));
            }
        };
        for (o, rad) in &bounds.circles {
            circle(*o, *rad);
        }
        for (p, q) in &swaths {
            circle((p.x, p.y), r);
            circle((q.x, q.y), r);
        }

        angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
        angles.dedup_by(|a, b| (*a - *b).abs() <= EPS);
        let swept = |q: (f64, f64)| swaths.iter().any(|(p0, p1)| dist_to_segment(q, *p0, *p1) < r - DIST_TOL);
        let mut total = 0.0;
        for i in 0..angles.len() {
            let a0 = angles[i];
            let a1 = if i + 1 < angles.len() { angles[i + 1] } else { angles[0] + std::f64::consts::TAU };
            let span = a1 - a0;
            if span <= EPS {
                continue;
            }
            let mid = a0 + span / 2.0;
            let q = (c.0 + r * mid.pcos(), c.1 + r * mid.psin());
            let leading = (q.0 - c.0) * travel.0 + (q.1 - c.1) * travel.1 >= 0.0;
            if leading && material(q) && !swept(q) {
                total += span;
            }
        }
        total
    }
}

// ---- the walk's arm of the engagement family ----------------------------------
// Phase 38 slice 2 (`DECISIONS.md` → "Phase 38 slice 2…", forks 3A, 4A, 5A).

/// How much of what the tool cut in material must be a full-width slot
/// before the op is MOSTLY A SLOT: more than half. A line with a meaning —
/// most of the cut — and still a chosen one, which is why it was a fork
/// (4A; the alternatives, any slot longer than the tool is wide and more
/// than half past 90°, fire on 47 and 29 of the 76 shipped ops).
pub const MOSTLY_A_SLOT: f64 = 0.5;

/// An op that is mostly a slot says so, under any pattern (fork 3A). Read
/// from the moves the program made (`compare::load_as_emitted`), so it
/// answers for every op that works a region; voiced only where the band
/// does not answer — the band's corner advisory already speaks for a
/// `Clear` on a pocket under the offset family, and one op gets one voice.
/// Advisory only (fork 5A): `engagement gate` escalates the band's findings
/// at validate, before a move exists, and not this arm — a full-width skim
/// half a millimetre deep and a full-width cut five deep read the same to
/// the walk, which is radial, and a gate that cannot tell them apart would
/// refuse the first. The sentence says the depth so the reader can.
pub fn slot_findings(p: &Part, emissions: &[crate::gcode::OpEmission]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for em in emissions {
        let idx = em.op_idx;
        let Some(op) = p.operations.get(idx) else { continue };
        let Some(tool) = p.tools.get(&op.tool_num) else { continue };
        // An op that cuts nothing says so (ROADMAP item 3 slice 6): its
        // moves at depth all run in air by the stock model before it — the
        // floor an earlier op already cut, or stock that never stood there.
        // Said before the band is asked, since the band prices the loops
        // and not what is under them.
        // Asked of a rest floor too since the model claims a floor only as
        // its tool reaches it (2026-11-10), so the stretches it takes stand.
        if let Some(region) = crate::region::of(p, op) {
            let floor_z = region.z + crate::parse::derive_floor_skin(p, op);
            let moves = crate::compare::at_depth(&crate::compare::op_moves(emissions, idx), floor_z);
            if let Some(mm) = cuts_nothing(p, idx, &moves, floor_z, tool.dia / 2.0) {
                findings.push(Finding::op(FindingFamily::Engagement, idx, &op.feature_name, format!(
                    "{} on '{}' with T{}: cuts nothing — its {:.0}mm of moves at depth all run in air by the stock model; the floor was already cut by an earlier operation, or no stock stood there. Suppress the operation, or give it a floor that still holds material",
                    op.op_type.verb(),
                    op.feature_name,
                    op.tool_num,
                    mm,
                )));
                continue;
            }
        }
        if !crate::rest::is_rest(op) && crate::parse::op_engagement(p, op).is_some() {
            continue;
        }
        let Some(load) = crate::compare::load_as_emitted(p, emissions, idx) else { continue };
        if load.engaged_mm <= 0.0 || load.slot_mm <= MOSTLY_A_SLOT * load.engaged_mm {
            continue;
        }
        let Some(region) = crate::region::of(p, op) else { continue };
        let floor_z = region.z + crate::parse::derive_floor_skin(p, op);
        let depth = depth_of_cut(p, idx, &crate::compare::at_depth(&crate::compare::op_moves(emissions, idx), floor_z), floor_z);
        findings.push(Finding::op(FindingFamily::Engagement, idx, &op.feature_name, format!(
            "{} on '{}' with T{}: mostly a slot — {:.0}mm of the {:.0}mm it cuts in material at depth ({:.0}%) is cut at the tool's full width ({}mm), {}mm deep. The walk measured it from the moves; `nacs --patterns` shows what each other pattern would meet on this floor",
            op.op_type.verb(),
            op.feature_name,
            op.tool_num,
            load.slot_mm,
            load.engaged_mm,
            load.slot_mm / load.engaged_mm * 100.0,
            crate::gcode::f(tool.dia),
            crate::gcode::f(depth),
        )));
    }
    findings
}

/// The length of the at-depth moves when none of them meets material
/// (ROADMAP item 3 slice 6): at one station a millimetre, the tool's
/// centre and eight points of its circle are asked of the stock model
/// before the op, a hair above the floor; `None` as soon as one answers
/// material, where the stock before the op is not modelled in closed form,
/// or where there are no moves at depth.
fn cuts_nothing(p: &Part, idx: usize, moves: &[(Pt, Pt)], floor_z: f64, r: f64) -> Option<f64> {
    if moves.is_empty() {
        return None;
    }
    let before = crate::stock::remaining_before(p, idx);
    if !before.complete {
        return None;
    }
    let z = floor_z + 0.01;
    let mut total = 0.0;
    for (a, b) in moves {
        let len = (b.x - a.x).phypot(b.y - a.y);
        total += len;
        let steps = (len.ceil() as usize).max(1);
        for s in 0..steps {
            let t = (s as f64 + 0.5) / steps as f64;
            let (cx, cy) = (a.x + t * (b.x - a.x), a.y + t * (b.y - a.y));
            if before.material_at(cx, cy, z) {
                return None;
            }
            for k in 0..8 {
                let ang = std::f64::consts::TAU * k as f64 / 8.0;
                if before.material_at(cx + r * ang.pcos(), cy + r * ang.psin(), z) {
                    return None;
                }
            }
        }
    }
    Some(total)
}

/// How deep the at-depth moves cut: the top of what stood above the floor
/// when the op began, read from the op-history stock at the moves' own
/// stations — the highest material found over them, less the floor — and
/// never more than a declared `stepdown`, which is the bite a `Clear` takes
/// on its last level. The walk is radial and cannot say this; the stock
/// model can, and this is the one number the sentence needs that the load
/// does not carry.
fn depth_of_cut(p: &Part, idx: usize, moves: &[(Pt, Pt)], floor_z: f64) -> f64 {
    let before = crate::stock::remaining_before(p, idx);
    let op = &p.operations[idx];
    let step = crate::CHORD_TOL_MM * 10.0;
    let mut top = floor_z;
    for (a, b) in moves.iter().step_by((moves.len() / 64).max(1)) {
        let (x, y) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let mut z = p.stock.z;
        while z > top && !before.material_at(x, y, z - step / 2.0) {
            z -= step;
        }
        top = top.max(z);
    }
    let depth = (top - floor_z).max(0.0);
    op.stepdown.map_or(depth, |s| s.min(depth))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `pocket_post.cam`'s own family (Phase 26 slice 2): three levels, the
    /// deepest cut from solid, and the two wall levels — pocket wall and post
    /// wall alike — riding a band one stepover wide, so neither strays far
    /// from the straight-run identity. A family walked through the post would
    /// read the island pass as air.
    #[test]
    fn a_post_in_a_pocket_is_priced_on_the_loops_that_go_around_it() {
        let wall: Vec<Pt> = [(20.0, 20.0), (80.0, 20.0), (80.0, 80.0), (20.0, 80.0)].iter().map(|&(x, y)| Pt { x, y }).collect();
        let post = [crate::offset::Island::Circle { x: 50.0, y: 50.0, r: 6.0 }];
        let e = islanded_pocket_engagement(&wall, &post, 5.0, 0.0, 4.0).expect("a family");
        assert_eq!(e.passes.len(), 3);
        assert!((e.passes[0].radians - std::f64::consts::PI).abs() < 1e-6, "the deepest level is a slot: {:?}", e.passes[0]);
        for p in &e.passes {
            eprintln!("pass {} {:.1}° at ({:.3}, {:.3})", p.pass, p.radians.to_degrees(), p.at.0, p.at.1);
        }
        let wall_level = e.passes[2].radians.to_degrees();
        assert!((78.0..=110.0).contains(&wall_level), "the wall level rides about a stepover: {wall_level}");
        // No island, no difference: the shape family IS the plain family.
        let plain = pocket_engagement(&wall, 5.0, 0.0, 4.0).unwrap();
        let shaped = islanded_pocket_engagement(&wall, &[], 5.0, 0.0, 4.0).unwrap();
        assert_eq!(plain.passes.len(), shaped.passes.len());
        for (a, b) in plain.passes.iter().zip(&shaped.passes) {
            assert!((a.radians - b.radians).abs() < 1e-6, "{a:?} vs {b:?}");
        }
    }

    fn square(side: f64) -> Vec<Pt> {
        vec![
            Pt { x: 0.0, y: 0.0 },
            Pt { x: side, y: 0.0 },
            Pt { x: side, y: side },
            Pt { x: 0.0, y: side },
        ]
    }

    // ---- the walk (Phase 34 slice 3) ------------------------------------------

    fn rows(ys: &[f64]) -> Vec<(Pt, Pt)> {
        ys.iter().map(|y| (Pt { x: 10.0, y: *y }, Pt { x: 90.0, y: *y })).collect()
    }

    fn in_square(q: (f64, f64)) -> bool {
        q.0 > 0.0 && q.0 < 100.0 && q.1 > 0.0 && q.1 < 100.0
    }

    fn square_bounds() -> Bounds {
        let sq = square(100.0);
        Bounds { edges: (0..4).map(|i| (sq[i], sq[(i + 1) % 4])).collect(), circles: Vec::new() }
    }

    /// The one number the walk must agree with the world about, as the band
    /// had to: a row one stepover beside the row before it is the textbook
    /// side cut, `acos(1 - a_e/r)`, and the first row into solid is a slot.
    #[test]
    fn the_walk_reproduces_the_textbook_identity_row_beside_row() {
        let r = 5.0;
        for stepover in [0.5, 1.0, 2.0, 4.0, 5.0, 6.0] {
            let moves = rows(&[50.0, 50.0 + stepover]);
            let walk = Walk::of(&moves, r);
            let slot = walk.engagement(0, 0.5, &square_bounds(), &in_square);
            assert!((slot - std::f64::consts::PI).abs() < 1e-9, "the first row is a slot: {slot}");
            let got = walk.engagement(1, 0.5, &square_bounds(), &in_square);
            let want = (1.0 - stepover / r).pacos();
            assert!((got - want).abs() < 1e-6, "stepover {stepover}: got {got:.6}, want {want:.6}");
        }
    }

    /// A row run twice cuts nothing the second time, and a row outside the
    /// blank cuts nothing at all — the blank's edge is a boundary like any.
    #[test]
    fn the_walk_reads_nothing_where_nothing_is_left() {
        let r = 5.0;
        let twice = rows(&[50.0, 50.0]);
        assert_eq!(Walk::of(&twice, r).engagement(1, 0.5, &square_bounds(), &in_square), 0.0);
        let outside = rows(&[-6.0]);
        assert_eq!(Walk::of(&outside, r).engagement(0, 0.5, &square_bounds(), &in_square), 0.0);
        // Half on, half off: the tool's centre on the blank's edge.
        let edge = rows(&[0.0]);
        let half = Walk::of(&edge, r).engagement(0, 0.5, &square_bounds(), &in_square);
        assert!((half - std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{half}");
    }

    /// What a link swept is swept. Two rows a slot apart and the move that
    /// joins their ends: the second row starts in the link's swath, and only
    /// reaches the full slot a radius along.
    #[test]
    fn the_walk_counts_the_link_between_two_rows() {
        let r = 5.0;
        let moves = vec![
            (Pt { x: 10.0, y: 30.0 }, Pt { x: 90.0, y: 30.0 }),
            (Pt { x: 90.0, y: 30.0 }, Pt { x: 90.0, y: 60.0 }),
            (Pt { x: 90.0, y: 60.0 }, Pt { x: 10.0, y: 60.0 }),
        ];
        let walk = Walk::of(&moves, r);
        let at = |t: f64| walk.engagement(2, t, &square_bounds(), &in_square);
        assert!(at(0.01) < std::f64::consts::PI - 0.5, "leaving the link: {}", at(0.01).to_degrees());
        assert!((at(0.5) - std::f64::consts::PI).abs() < 1e-9, "clear of it: {}", at(0.5).to_degrees());
    }

    /// Finding E, closed (Phase 37 slice 1). (60, 50) on `pocket_post` is 4mm
    /// from the post and 11mm from the nearest corner loop of the deepest
    /// level, and it is inside none of them. Each of those loops closes with
    /// an arc that curves round the post — concave to the loop — and the
    /// hand-written predicate that stood here flipped containment on the
    /// centre's side of that arc's chord: it read this point as inside all
    /// four, so as swept, and the band under-read beside the post.
    #[test]
    fn a_point_beside_the_post_is_inside_none_of_the_corner_loops() {
        let wall: Vec<Pt> = [(20.0, 20.0), (80.0, 20.0), (80.0, 80.0), (20.0, 80.0)].iter().map(|&(x, y)| Pt { x, y }).collect();
        let post = [crate::offset::Island::Circle { x: 50.0, y: 50.0, r: 6.0 }];
        let mut levels = crate::offset::shape_levels(&wall, &post, 5.0, 4.0);
        levels.reverse();
        let corners = &levels[0].1.ccw;
        assert_eq!(corners.len(), 4);
        let q = (60.0, 50.0);
        assert!(corners.iter().all(|l| dist_to_loop(q, l) > 10.0), "far from all four");
        assert!(!corners.iter().any(|l| inside_loop(q, l)), "and inside none of them");
        // …and a point that IS in a corner loop still reads so.
        assert_eq!(corners.iter().filter(|l| inside_loop((65.0, 65.0), l)).count(), 1);
    }

    /// The sliver between an arc and its chord, both ways round. A half-disc
    /// bulging right of its chord holds a point in the bulge; a square with
    /// one side bowed INWARD does not hold a point between that side's chord
    /// and its arc. The old predicate had these two the other way about, and
    /// a circle — two half turns, the chord a diameter — could not show it.
    #[test]
    fn containment_follows_the_arc_not_its_chord() {
        let v = |x: f64, y: f64, bulge: f64| OffsetVertex { x, y, bulge };
        // (0,-5) → (0,5) by a CCW half turn through (5,0), then straight back.
        let half_disc = [v(0.0, -5.0, 1.0), v(0.0, 5.0, 0.0)];
        assert!(inside_loop((3.0, 0.0), &half_disc), "in the bulge");
        assert!(!inside_loop((-3.0, 0.0), &half_disc), "past the chord");
        // A 10 x 10 square, CCW, whose right side bows in to (8, 5): a CW arc.
        let bowed = [v(0.0, 0.0, 0.0), v(10.0, 0.0, -(2.0_f64 / 5.0)), v(10.0, 10.0, 0.0), v(0.0, 10.0, 0.0)];
        assert!(!inside_loop((9.5, 5.0), &bowed), "between the chord and the arc that bows in");
        assert!(inside_loop((7.0, 5.0), &bowed), "past the arc");
        assert!(inside_loop((9.5, 0.5), &bowed), "in the corner the arc leaves");
    }

    #[test]
    fn a_straight_run_reproduces_the_textbook_identity() {
        // The one number this primitive must agree with the world about:
        // theta_e = acos(1 - a_e/r) for a side cut of radial depth a_e.
        let poly = square(100.0);
        let r = 5.0;
        for stepover in [0.5, 1.0, 2.0, 4.0] {
            // Pass 1 of a skinless pocket: the boundary pass rode `r` from the
            // wall, so it left material standing at `2r`; this pass's centers
            // ride `r + stepover`, putting the tool `r - stepover` short of that
            // edge and cutting `stepover` deep — the textbook side cut.
            let c = (50.0, r + stepover);
            let travel = (1.0, 0.0);
            let got = engagement_at(c, r, travel, &poly, 2.0 * r);
            let want = (1.0 - stepover / r).pacos();
            assert!(
                (got - want).abs() < 1e-6,
                "stepover {stepover}: got {:.6} rad, want {:.6} rad",
                got,
                want
            );
        }
    }

    #[test]
    fn half_immersion_is_ninety_degrees_and_a_slot_is_one_eighty() {
        let poly = square(100.0);
        let r = 5.0;
        // a_e == r is half immersion — the tool center sits exactly on the
        // material edge: theta_e == 90 degrees exactly, and the chip-thinning
        // factor 1/sin(theta_e) is exactly 1.
        let half = engagement_at((50.0, 2.0 * r), r, (1.0, 0.0), &poly, 2.0 * r);
        assert!(
            (half - std::f64::consts::FRAC_PI_2).abs() < 1e-6,
            "half immersion was {half:.6} rad"
        );
        // A tool standing in solid material with nothing cleared is a full-width
        // slot: the whole leading half-circle is engaged.
        let slot = engagement_at((50.0, 50.0), r, (1.0, 0.0), &poly, 0.0);
        assert!(
            (slot - std::f64::consts::PI).abs() < 1e-6,
            "slot was {slot:.6} rad"
        );
    }

    #[test]
    fn engagement_is_zero_in_air() {
        // A position whose whole circle stands inside already-cleared ground.
        let poly = square(100.0);
        let e = engagement_at((50.0, 50.0), 5.0, (1.0, 0.0), &poly, 60.0);
        assert_eq!(e, 0.0);
    }

    #[test]
    fn the_thinning_factor_is_the_reciprocal_sine_and_clamps_the_angle() {
        // Against the standard form D/(2*sqrt(a_e*(D - a_e))) — the identity
        // this whole slice rests on.
        let d: f64 = 1.0;
        for ratio in [0.10f64, 0.20, 0.40] {
            let ae = ratio * d;
            let theta = (1.0 - 2.0 * ae / d).pacos();
            let standard = d / (2.0 * (ae * (d - ae)).sqrt());
            let got = chip_thinning(theta);
            assert!(
                (got.factor - standard).abs() < 1e-12,
                "a_e/D {ratio}: got {} want {standard}",
                got.factor
            );
            assert!(!got.capped, "a_e/D {ratio} should not reach the cap");
        }

        // 5% immersion is BELOW the cap's 6.7% knee, so the identity is
        // still the identity but the shipped factor is the ceiling — the
        // boundary pinned rather than described.
        let five = chip_thinning((1.0 - 2.0 * 0.05_f64).pacos());
        assert!(five.capped, "5% immersion should reach the cap");
        assert_eq!(five.factor, CHIP_THINNING_CAP);

        // Half immersion: exactly 1, no correction, no case split.
        let half = chip_thinning(std::f64::consts::FRAC_PI_2);
        assert!((half.factor - 1.0).abs() < 1e-12);

        // THE MIRROR. Past half immersion 1/sin rises again — 60% immersion
        // reads the same 1.0206 as 40%. Clamping the ANGLE must flatten it to
        // 1.0; a naive clamp on the VALUE would let it through as a boost
        // exactly as the tool buries itself.
        let past = (1.0 - 2.0 * 0.60_f64).pacos(); // 101.5 degrees
        assert!(past > std::f64::consts::FRAC_PI_2);
        assert!(
            (chip_thinning(past).factor - 1.0).abs() < 1e-12,
            "the mirror leaked through: {}",
            chip_thinning(past).factor
        );
        // A full slot is the mirror's limit and must also be exactly 1.
        assert!((chip_thinning(std::f64::consts::PI).factor - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_cap_binds_only_below_the_workflow_it_serves() {
        // The author's own loop targets ~10% of the finisher's diameter.
        let ten_pct = (1.0 - 2.0 * 0.10_f64).pacos();
        let at_target = chip_thinning(ten_pct);
        assert!((at_target.factor - 1.0 / 0.6).abs() < 1e-9, "{}", at_target.factor);
        assert!(!at_target.capped, "the cap must not bind on the 10% target");

        // A whisper-thin cut is where it does bind, and says so.
        let wisp = chip_thinning(0.01);
        assert_eq!(wisp.factor, CHIP_THINNING_CAP);
        assert!(wisp.capped);

        // Zero engagement is not an infinite feed.
        assert_eq!(chip_thinning(0.0).factor, 1.0);
    }

    #[test]
    fn a_reflex_corner_spikes_past_the_straight_run() {
        // The L-pocket, the shape the corner arm exists for. Its inward offset
        // carries a real arc at the reflex vertex (the offsetter returns one
        // bulge-bearing segment), so this exercises the arc branch of the pass
        // walk as well as the spike itself.
        let l = vec![
            Pt { x: 0.0, y: 0.0 },
            Pt { x: 60.0, y: 0.0 },
            Pt { x: 60.0, y: 30.0 },
            Pt { x: 30.0, y: 30.0 },
            Pt { x: 30.0, y: 60.0 },
            Pt { x: 0.0, y: 60.0 },
        ];
        let r = 5.0;
        let stepover = 4.0;
        let pe = pocket_engagement(&l, r, 0.0, stepover).expect("passes");
        // The straight runs still hold the textbook value...
        let nominal = (1.0 - stepover / r).pacos();
        assert!((pe.nominal - nominal).abs() < 1e-12);
        // ...and the L drives past it, and past half immersion.
        assert!(
            pe.worst.radians > pe.nominal,
            "the L did not spike: worst {:.4} vs nominal {:.4}",
            pe.worst.radians.to_degrees(),
            pe.nominal.to_degrees()
        );
        assert!(
            pe.worst.radians > std::f64::consts::FRAC_PI_2,
            "spike stayed under half immersion: {:.2} deg",
            pe.worst.radians.to_degrees()
        );
        // WHERE it spikes moved when the primitive learned the emitter's
        // order (Phase 9 lane A, slice 1). Walked boundary-first, the spike
        // was 93.8° one pass offset from the reflex vertex (30,30) — the
        // mirror order's answer. Cut inside-out, the innermost loop is a
        // tiny ring at the L's ELBOW (the 30×30 corner square's inscribed
        // circle outruns the arms' 30mm width: offset 17 exists there and
        // nowhere else), and the next loop's arms run out from that elbow
        // INTO SOLID — a full slot on pass 1, far from the reflex vertex.
        assert!(
            (pe.worst.radians - std::f64::consts::PI).abs() < 1e-9,
            "the arms of the second loop cut from solid: {:.2} deg",
            pe.worst.radians.to_degrees()
        );
        assert_eq!(pe.worst.pass, 1);
        assert!(pe.passes.len() >= 4, "elbow ring, then three L-shaped loops: {:?}", pe.passes);
        assert!((pe.passes[0].radians - std::f64::consts::PI).abs() < 1e-9, "the elbow ring is the slot");
        // And the wall pass — the last one — is the textbook side cut again:
        // by then the inner swath is a stepover inside it everywhere.
        let wall = pe.passes.last().unwrap();
        assert!((wall.radians - nominal).abs() < 1e-4, "wall pass {:.4} vs nominal {:.4}", wall.radians, nominal);
    }

    #[test]
    fn the_worst_engagement_is_the_conservative_choice() {
        // Fork 3A, stated as the safety property it is rather than as a
        // preference. The thinning factor FALLS as engagement rises, so
        // pricing the feed at the worst engagement over the pass yields the
        // SMALLEST boost. A reflex corner must therefore make the feed more
        // conservative, never less — if we priced the nominal instead, the
        // corner would be fed as though it were a straight run.
        let square: Vec<Pt> = [(0.0, 0.0), (90.0, 0.0), (90.0, 90.0), (0.0, 90.0)]
            .iter()
            .map(|&(x, y)| Pt { x, y })
            .collect();
        let ell: Vec<Pt> = [
            (0.0, 0.0), (90.0, 0.0), (90.0, 45.0),
            (45.0, 45.0), (45.0, 90.0), (0.0, 90.0),
        ]
        .iter()
        .map(|&(x, y)| Pt { x, y })
        .collect();

        let sq = pocket_engagement(&square, 5.0, 0.0, 1.0).expect("square passes");
        let l = pocket_engagement(&ell, 5.0, 0.0, 1.0).expect("L passes");

        // Same declaration, so the same straight-run promise...
        assert!((sq.nominal - l.nominal).abs() < 1e-12);
        // ...but the L's reflex corner is measured well past it.
        assert!(sq.worst.radians < l.worst.radians,
            "the L must engage harder: square {:.2} deg vs L {:.2} deg",
            sq.worst.radians.to_degrees(), l.worst.radians.to_degrees());

        // And so it is fed more gently. This is the assertion that matters.
        let (fs, fl) = (chip_thinning(sq.worst.radians), chip_thinning(l.worst.radians));
        assert!(fl.factor < fs.factor,
            "the corner must lower the boost: square x{:.4} vs L x{:.4}",
            fs.factor, fl.factor);
    }

    #[test]
    fn a_square_pocket_holds_its_nominal_past_its_first_turns() {
        // A convex pocket has no reflex vertex, so no corner can spike — but
        // cut INSIDE-OUT its first turns run heavy (Phase 9 lane A, slice 1):
        // after a 2×2 innermost loop the swept region is a rounded 12×12
        // square, and the next loop's leading edge meets the round shoulder
        // of that swath out past a stepover's width. The mirror-order model
        // read the nominal on every pass; the machine does not cut that way.
        let poly = square(60.0);
        let pe = pocket_engagement(&poly, 5.0, 0.0, 2.0).expect("passes");
        let p = &pe.passes;
        assert!((p[0].radians - std::f64::consts::PI).abs() < 1e-9, "the slot");
        assert!(p[1].radians > pe.nominal + 0.3, "the first turn runs heavy: {:.2} deg", p[1].radians.to_degrees());
        assert!(p[2].radians > pe.nominal + 1e-4, "and the second, a little: {:.2} deg", p[2].radians.to_degrees());
        // From the third turn on — and on the wall pass — exactly the
        // straight-run identity, every pass.
        for e in &p[3..] {
            assert!((e.radians - pe.nominal).abs() < 1e-4, "pass {} {:.6} vs nominal {:.6}", e.pass, e.radians, pe.nominal);
        }
        assert_eq!(pe.worst.pass, 1, "the op-level worst is the first turn");
    }
}

// ---------------------------------------------------------------------------
// The immersion field: what the tool WOULD meet, before a path exists
// (Phase 41 slice 2; `DECISIONS.md` → "Phase 41 slice 2…").
// ---------------------------------------------------------------------------
//
// The band and the walk above both measure a PATH: a tool position with a
// direction of travel and the passes before it. The focused-feature brief asks
// to see where the tool will be overloaded while the boundary is still being
// drawn — before any pass exists to measure — so this is the direction-free
// quantity: at each position of the floor the tool could stand on, how much of
// its circle is in the material that stands when the op BEGINS (the op-history
// stock, `stock::remaining_before`). Half that arc is the engagement a pass
// through the point along any straight line would see with the trailing half
// already swept — on a straight run beside a previous pass it is the textbook
// `theta_e` exactly, and in solid it is π, the full slot. It has no direction
// and no previous pass of this op's own, so it is a CEILING on what any pattern
// meets there: a steering picture, never a certificate, and never read by a
// feed, a price or a pattern's choice.
//
// SAMPLED, on purpose, and said so: the circle is asked `material_at` at
// `IMMERSION_SAMPLES` angles, and the field is a grid of cells. The band is
// closed-form because a certificate must be; a picture whose cell is stated
// may count points. The sampling is the picture's own and never a toolpath's,
// which is what keeps this beside — not inside — the engagement linter's
// floor, under its standing order not to grow into a voxel engine.

/// Points asked around the tool's circle per cell. 64 puts the quantum at
/// π/64 ≈ 2.8°, under any band's width on the corpus.
pub const IMMERSION_SAMPLES: usize = 64;

/// The grid's cell as a share of the tool's DIAMETER: an eighth, so a tool
/// of any size is eight cells across and the picture's grain follows the
/// tool rather than the part. Stated in the line, so a reader knows what
/// one square means.
pub const IMMERSION_CELL_OF_DIA: f64 = 1.0 / 8.0;

/// How far above the floor the circle is asked: a hair, so material that
/// stands down to the floor counts and a floor already cut to this height
/// does not.
const ABOVE_THE_FLOOR: f64 = 1e-3;

/// Which band a cell's immersion falls in — three words the house already
/// uses, none of them new: WITHIN the stepover's own `acos(1 - a_e/r)`,
/// ABOVE it but under the 90° line, and PAST 90° (the tool more than half
/// buried — the line `parse::engagement_findings` and the report's
/// `past 90°` column stand on; a full slot is its far end).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Within,
    Above,
    Past90,
}

/// The immersion field of one op over the floor it works.
#[derive(Clone, Debug, PartialEq)]
pub struct Immersion {
    pub op_idx: usize,
    /// The grid's cell, mm.
    pub cell: f64,
    /// The lower-left corner of cell (0, 0).
    pub x0: f64,
    pub y0: f64,
    pub nx: usize,
    pub ny: usize,
    /// The floor's height — where the cells are drawn.
    pub z: f64,
    /// The tool's radius.
    pub r: f64,
    /// Radians per cell, row-major (`iy * nx + ix`); `None` where the tool
    /// cannot stand — outside the floor, nearer a WALL edge than its
    /// radius, or on what stands.
    pub theta: Vec<Option<f64>>,
    /// What the declared stepover implies on a straight run: `acos(1 - a_e/r)`;
    /// π when no stepover is declared (then nothing is "within").
    pub nominal: f64,
    /// The highest cell: `(x, y, theta)`.
    pub worst: Option<(f64, f64, f64)>,
}

impl Immersion {
    /// The band a value falls in. The sampling's quantum is the slack: a
    /// value within one sample of the nominal is within it.
    pub fn band(&self, theta: f64) -> Band {
        let q = std::f64::consts::PI / IMMERSION_SAMPLES as f64;
        if theta <= self.nominal + q {
            Band::Within
        } else if theta <= std::f64::consts::FRAC_PI_2 + q {
            Band::Above
        } else {
            Band::Past90
        }
    }

    /// The cells with a value: `(x, y, theta)` at cell centres.
    pub fn cells(&self) -> impl Iterator<Item = (f64, f64, f64)> + '_ {
        self.theta.iter().enumerate().filter_map(move |(i, t)| {
            t.map(|t| {
                let (ix, iy) = (i % self.nx, i / self.nx);
                (self.x0 + (ix as f64 + 0.5) * self.cell, self.y0 + (iy as f64 + 0.5) * self.cell, t)
            })
        })
    }

    /// The three bands' shares of the cells with a value, in `Band` order,
    /// and the share that is a FULL slot (every sample in material) — the
    /// far end of the third band, said on its own because it is the case
    /// the brief was written about.
    pub fn shares(&self) -> ([f64; 3], f64) {
        let mut n = [0usize; 3];
        let mut slot = 0usize;
        let mut total = 0usize;
        let q = std::f64::consts::PI / IMMERSION_SAMPLES as f64;
        for (_, _, t) in self.cells() {
            total += 1;
            n[match self.band(t) {
                Band::Within => 0,
                Band::Above => 1,
                Band::Past90 => 2,
            }] += 1;
            if t >= std::f64::consts::PI - q / 2.0 {
                slot += 1;
            }
        }
        if total == 0 {
            return ([0.0; 3], 0.0);
        }
        let share = |k: usize| k as f64 / total as f64;
        ([share(n[0]), share(n[1]), share(n[2])], share(slot))
    }

    /// The terminal's sentence: what was measured, at what grain, and how
    /// the floor divides — the one line the window's layer may say.
    pub fn line(&self, p: &Part) -> String {
        let op = &p.operations[self.op_idx];
        let count = self.theta.iter().filter(|t| t.is_some()).count();
        let ([within, above, past], slot) = self.shares();
        let deg = |rad: f64| rad.to_degrees();
        let nominal = if self.nominal >= std::f64::consts::PI {
            "no stepover declared".to_string()
        } else {
            format!("the stepover's {:.0}°", deg(self.nominal))
        };
        let worst = match self.worst {
            Some((x, y, t)) => format!("; worst {:.0}° at ({}, {})", deg(t), crate::gcode::f(x), crate::gcode::f(y)),
            None => String::new(),
        };
        format!(
            "immersion before {} on '{}' with T{} ({}mm cells, {} where the tool can stand): {:.0}% within {}, {:.0}% above it, {:.0}% past 90° ({:.0}% a full slot){} — a ceiling on what any pattern meets, from the stock as this op finds it",
            op.op_type.verb(),
            op.feature_name,
            op.tool_num,
            crate::gcode::f(self.cell),
            count,
            within * 100.0,
            nominal,
            above * 100.0,
            past * 100.0,
            slot * 100.0,
            worst
        )
    }

    /// The field as text, top row first (+Y up, as a plan reads): a space
    /// where the tool cannot stand, `.` within, `+` above, `#` past 90°.
    /// The picture's terminal voice, the same cells the window colours.
    pub fn map(&self) -> String {
        let mut out = String::new();
        for iy in (0..self.ny).rev() {
            let mut row = String::with_capacity(self.nx);
            for ix in 0..self.nx {
                row.push(match self.theta[iy * self.nx + ix] {
                    None => ' ',
                    Some(t) => match self.band(t) {
                        Band::Within => '.',
                        Band::Above => '+',
                        Band::Past90 => '#',
                    },
                });
            }
            out.push_str(row.trim_end());
            out.push('\n');
        }
        out
    }
}

/// The immersion field of op `op_idx`. Declines by name where it cannot
/// answer honestly: an op that works no floor region (a flank finish, a
/// drill, a rest floor — several floors, its own line's business), a tool
/// it cannot find, or a stock model that is INCOMPLETE before the op (an
/// earlier op with no closed-form removal over-states material, and a
/// ceiling read from an over-statement is not a ceiling).
pub fn immersion(p: &Part, op_idx: usize) -> Result<Immersion, String> {
    let op = p.operations.get(op_idx).ok_or_else(|| format!("no operation [{op_idx}] — nacs --outline lists them"))?;
    let tool = p.tools.get(&op.tool_num).ok_or_else(|| format!("operation [{op_idx}] names T{}, which is not declared", op.tool_num))?;
    if crate::rest::is_rest(op) {
        return Err(format!("operation [{op_idx}] is a rest floor — several floors, each read by its own line; the immersion field answers for one"));
    }
    let region = crate::region::of(p, op).ok_or_else(|| {
        format!("operation [{op_idx}] ({} on '{}') works no floor region — the immersion field is a floor's picture", op.op_type.verb(), op.feature_name)
    })?;
    let before = crate::stock::remaining_before(p, op_idx);
    if !before.complete {
        return Err(format!(
            "the stock before operation [{op_idx}] is not modelled in closed form (an earlier op has no declared removal), so the material it would meet cannot be said"
        ));
    }
    let r = tool.dia / 2.0;
    let cell = tool.dia * IMMERSION_CELL_OF_DIA;
    let z = region.z + crate::parse::derive_floor_skin(p, op) + ABOVE_THE_FLOOR;
    let (mut xa, mut ya, mut xb, mut yb) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for q in &region.outer {
        xa = xa.min(q.x);
        ya = ya.min(q.y);
        xb = xb.max(q.x);
        yb = yb.max(q.y);
    }
    if !(xa < xb && ya < yb) {
        return Err(format!("operation [{op_idx}] works a floor with no area"));
    }
    let nx = ((xb - xa) / cell).ceil() as usize;
    let ny = ((yb - ya) / cell).ceil() as usize;
    let standing = region.standing_polygons();
    let n = region.outer.len();
    let walls: Vec<(Pt, Pt)> = (0..n)
        .filter(|&i| region.edges[i] == crate::region::EdgeKind::Wall)
        .map(|i| (region.outer[i], region.outer[(i + 1) % n]))
        .collect();
    let nominal = match crate::parse::derive_stepover(p, op) {
        Some(a) if a > 0.0 => (1.0 - a / r).clamp(-1.0, 1.0).pacos(),
        _ => std::f64::consts::PI,
    };
    let mut theta = Vec::with_capacity(nx * ny);
    let mut worst: Option<(f64, f64, f64)> = None;
    for iy in 0..ny {
        for ix in 0..nx {
            let (cx, cy) = (xa + (ix as f64 + 0.5) * cell, ya + (iy as f64 + 0.5) * cell);
            // Where the tool can STAND: on the floor, a radius off every
            // wall edge (an open edge bounds nothing — the tool may run
            // off), and a radius off what stands.
            let on_floor = crate::stock::point_in_polygon(&region.outer, cx, cy)
                && walls.iter().all(|(a, b)| dist_to_segment((cx, cy), *a, *b) >= r - EPS)
                && standing
                    .iter()
                    .all(|lp| !crate::stock::point_in_polygon(lp, cx, cy) && crate::stock::dist_to_polygon(lp, cx, cy) >= r - EPS);
            if !on_floor {
                theta.push(None);
                continue;
            }
            let mut hits = 0usize;
            for k in 0..IMMERSION_SAMPLES {
                let a = std::f64::consts::TAU * k as f64 / IMMERSION_SAMPLES as f64;
                if before.material_at(cx + r * a.pcos(), cy + r * a.psin(), z) {
                    hits += 1;
                }
            }
            let t = std::f64::consts::PI * hits as f64 / IMMERSION_SAMPLES as f64;
            if worst.is_none_or(|w| t > w.2) {
                worst = Some((cx, cy, t));
            }
            theta.push(Some(t));
        }
    }
    Ok(Immersion { op_idx, cell, x0: xa, y0: ya, nx, ny, z: region.z, r, theta, nominal, worst })
}

/// The terminal's report: the line, then the map.
pub fn immersion_report(p: &Part, op_idx: usize) -> Result<String, String> {
    let f = immersion(p, op_idx)?;
    Ok(format!("{}\n{}", f.line(p), f.map()))
}
