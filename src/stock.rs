use crate::math::Portable as _;
// ---- stock: the analytical op-history remaining-stock model ------------------
//
// What is still in the stock at any point in the `run` sequence, derived from
// the DECLARED feature/op history rather than from a simulated cut (ROADMAP
// "Stock-aware planning for concave / form-profile cutters", level 1;
// DECISIONS.md "Analytical op-history stock…"). Level 2 — arbitrary stock state
// from a voxel model — is Phase 5 and is deliberately not this.
//
// The whole model rests on one rule, which is "it's a wall now" (README) applied
// to the stock: **after an op runs, the geometry its feature declares STANDS.**
// So a removal is not "the volume the tool swept" (path-dependent, and the thing
// voxels exist to answer) but "the footprint this feature occupies, from the
// stock's top down to the surface the feature declares" — a footprint plus a
// floor function, both closed-form. That is why this can precede Phase 5 at all.
//
// Every op either has such a removal or it does not, and the difference is
// tracked (`complete`) rather than papered over: the same total-accounting
// policy the sidecar uses for recognition residue. A consumer that finds
// `complete == false` must make no claim about what remains — a partial subtract
// over-states material, and an over-statement is a false alarm.

use std::f64::consts::PI;

use crate::parse::{
    derive_contour_top, derive_z_cut, Feature, OpType, Operation, Part, Pt, Silhouette, Stock, StockShape, Tool, UpAxis,
};

// Membership tolerance, in mm. Purely numerical, and it always leans the same
// way: **removals are generous, the stock is strict**, so geometry that touches
// exactly — a tool body riding the wall it just cut, a tip resting on its own
// floor — reads as contact and not as material. Tangency is the NORMAL case
// here (every finishing pass ends up exactly on the surface it cut), so a
// tolerance that leaned the other way would report a foul on every well-formed
// op. Deliberately NOT CHORD_TOL_MM: this is not a curve approximation budget,
// and borrowing that number would tie two unrelated errors together.
const EPS: f64 = 1e-6;

/// Half of the internal dialect's last place, in mm. `gcode::f_coord` writes
/// coordinates at three decimals and every certificate reads that text back,
/// so a depth measured off an EMISSION carries this much quantization against
/// the same depth derived from a declaration. The width of "these two agree".
pub(crate) const EMITTED_MM_HALF_ULP: f64 = 0.0005;

/// The XY footprint of a removal. Closed shapes only — a removal is a solid
/// column of plan area, never a curve or a swept path.
#[derive(Clone, Debug, PartialEq)]
pub enum Section {
    /// The whole stock in plan: what facing takes.
    All,
    Disc { x: f64, y: f64, r: f64 },
    /// A disc with its middle left alone — what a *band* declares. A truncated
    /// cap (a catenoid neck, a pseudosphere band, a vase, the whole of a torus)
    /// says nothing about the region inside its top circle, so that region is
    /// undeclared stock, not removed material.
    Annulus { x: f64, y: f64, r_inner: f64, r_outer: f64 },
    Ellipse { x: f64, y: f64, a: f64, b: f64, rot: f64 },
    /// `holes` are the islands the footprint goes around (Phase 26 slice 2):
    /// standing outlines inside `points` that this removal never claimed. Empty
    /// for every polygon that hosts none.
    Polygon { points: Vec<Pt>, holes: Vec<Vec<Pt>> },
    /// The band a distance `[d_inner, d_outer]` off a closed `path`, on the
    /// material side of it — the `Annulus`'s general form, and what a form tool
    /// swept along a polyline declares (DECISIONS.md → "the corner rounder gets
    /// an operation", slice 2).
    ///
    /// `material_inside` says which side of the path holds the material: inside
    /// for a boss, outside for a pocket. The distance bound does most of the
    /// work — a pocket's axis path is far from the far wall too — but the side
    /// test is what keeps the band from wrapping around onto the void side of a
    /// narrow web.
    PathBand { path: Vec<Pt>, material_inside: bool, d_inner: f64, d_outer: f64 },
    /// One stretch of a `PathBand` (ROADMAP item 1 slice 9): the band a
    /// distance `[d_inner, d_outer]` off an OPEN `chain` of the closed
    /// `outline`, on the outline's material side. A chamfer whose loop
    /// points restate the leg is one of these per stretch, each at its own
    /// depth. The band reaches a half-disc past the chain's ends, which is
    /// where the deeper flank goes round the corner.
    ChainBand { outline: Vec<Pt>, chain: Vec<Pt>, material_inside: bool, d_inner: f64, d_outer: f64 },
}

impl Section {
    /// Grown by `EPS + slack`, per the tolerance note above: the boundary
    /// belongs to the removal, so a tool riding its own wall is not standing in
    /// material.
    ///
    /// `slack` is the plan-direction half of what `Removal::removes` already
    /// allowed in Z, and it is here because EPS alone is smaller than the
    /// numbers' own representation. `check_moves` reads positions back out of
    /// EMITTED G-code, rounded to three decimals, so a pass placed exactly on
    /// its declared boundary comes back up to ~0.7µm off it — four orders above
    /// EPS. Z was given `MOTION_SLACK` for exactly this reason; plan was not,
    /// and the gap showed the first time an op's outermost pass was placed
    /// exactly at its own declared edge (DECISIONS.md → "roughing a convex
    /// revolve…", slice 1).
    pub(crate) fn contains(&self, x: f64, y: f64, slack: f64) -> bool {
        match self {
            Section::All => true,
            Section::Disc { x: cx, y: cy, r } => {
                let (dx, dy) = (x - cx, y - cy);
                let grown = r + EPS + slack;
                dx * dx + dy * dy < grown * grown
            }
            Section::Annulus { x: cx, y: cy, r_inner, r_outer } => {
                let (dx, dy) = (x - cx, y - cy);
                let rho = (dx * dx + dy * dy).sqrt();
                rho > r_inner - EPS - slack && rho < r_outer + EPS + slack
            }
            Section::Ellipse { x: cx, y: cy, a, b, rot } => {
                // Un-rotate into the ellipse's own frame, then the unit-circle test.
                let (dx, dy) = (x - cx, y - cy);
                let (s, c) = (-rot).psin_cos();
                let (u, v) = (dx * c - dy * s, dx * s + dy * c);
                let (ga, gb) = (a + EPS + slack, b + EPS + slack);
                (u / ga) * (u / ga) + (v / gb) * (v / gb) < 1.0
            }
            Section::Polygon { points, holes } => {
                // Inside the outline and inside no hole. Both boundaries belong
                // to the REMOVAL, the same lean at the island's wall as at the
                // pocket's: a tool riding the wall it just cut is not standing
                // in material, whichever wall it is.
                let in_outline = point_in_polygon(points, x, y) || dist_to_polygon(points, x, y) < EPS + slack;
                in_outline
                    && !holes.iter().any(|h| point_in_polygon(h, x, y) && dist_to_polygon(h, x, y) >= EPS + slack)
            }
            Section::PathBand { path, material_inside, d_inner, d_outer } => {
                if point_in_polygon(path, x, y) != *material_inside {
                    return false;
                }
                let d = dist_to_polygon(path, x, y);
                d > d_inner - EPS - slack && d < d_outer + EPS + slack
            }
            Section::ChainBand { outline, chain, material_inside, d_inner, d_outer } => {
                if point_in_polygon(outline, x, y) != *material_inside {
                    return false;
                }
                let d = dist_to_chain(chain, x, y);
                d > d_inner - EPS - slack && d < d_outer + EPS + slack
            }
        }
    }
}

/// `dist_to_polygon` for an open chain: no closing segment.
pub fn dist_to_chain(points: &[Pt], x: f64, y: f64) -> f64 {
    match points.len() {
        0 => f64::INFINITY,
        1 => (x - points[0].x).phypot(y - points[0].y),
        _ => points.windows(2).map(|w| dist_to_polygon(w, x, y)).fold(f64::INFINITY, f64::min),
    }
}

/// Even-odd ray cast along +X. Winding is irrelevant here — the variant already
/// carries side-of-material (the row-2.4 law), so this only answers "inside the
/// closed loop", which both windings agree on. A point ON an edge is decided
/// arbitrarily by this test, which is why `contains` pairs it with the distance
/// check rather than trusting it alone.
pub fn point_in_polygon(points: &[Pt], x: f64, y: f64) -> bool {
    let n = points.len();
    let mut inside = false;
    for i in 0..n {
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        if (p1.y > y) != (p2.y > y) {
            let t = (y - p1.y) / (p2.y - p1.y);
            if x < p1.x + t * (p2.x - p1.x) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Distance from a point to the closed polyline (its edges, not its interior).
pub fn dist_to_polygon(points: &[Pt], x: f64, y: f64) -> f64 {
    let n = points.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let (ex, ey) = (p2.x - p1.x, p2.y - p1.y);
        let len2 = ex * ex + ey * ey;
        let t = if len2 > 0.0 {
            (((x - p1.x) * ex + (y - p1.y) * ey) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (dx, dy) = (x - (p1.x + t * ex), y - (p1.y + t * ey));
        best = best.min((dx * dx + dy * dy).sqrt());
    }
    best
}

/// The bottom of a removal: the surface the feature declares, which is what the
/// stock is left standing on.
#[derive(Clone, Debug, PartialEq)]
pub enum Floor {
    Flat(f64),
    /// Upper half of a sphere of radius `r` centred on the equator plane `z`.
    Dome { x: f64, y: f64, z: f64, r: f64 },
    /// Any surface of revolution, as its meridian sampled into `(rho, z)` with
    /// rho ascending — the R2 cavities (bowl, throat, funnel, spline funnel),
    /// which share nothing but this shape. Each contributes only a sampler
    /// calling the same `parse::*_cl_point` home its own op cuts with, so the
    /// wall this model reports and the wall the machine leaves are one curve.
    Meridian { x: f64, y: f64, samples: Vec<(f64, f64)> },
    /// A form tool swept along a horizontal `path`: the surface as a sampled
    /// `(d, z)` table, `d` ascending, where `d` is the plan distance to the
    /// path. This is `Meridian`'s general form and `Meridian`'s own law — the
    /// tool is a solid of revolution, so its swept surface is a function of the
    /// distance to the curve its AXIS rode, and the meridian arm is the case
    /// where that curve happens to be a circle. The table is fed from the
    /// tool's CUTTING form (a rounder's `GroundArc` today, a chamfer mill's
    /// cone next), never from `Silhouette` body points — the body carries
    /// relieved clearance surfaces, which are exactly what was NOT removed.
    ///
    /// Measuring from the path rather than from the wall is what makes this
    /// safe at corners. At a plan corner tighter than the tool the axis pivots
    /// on the offset vertex and never reaches the perpendicular stand-off from
    /// the corner — a wall-relative surface would claim a full round there and
    /// be over-stating removal, which is the dangerous direction (`check_moves`
    /// reads this model to decide whether a plunge is into air).
    PathForm { path: Vec<Pt>, samples: Vec<(f64, f64)> },
    /// A surface that follows a 3D WIRE: at `(x, y)` the z of the wire's
    /// nearest point, `drop` below it — `ops::profile::wire_cut_z`, the one
    /// home the wire emitter itself reads, so the floor the model reports and
    /// the tip the machine runs are one function (Phase 10 lane A, slice 2).
    /// `PathForm` is a `(d, z)` table over distance FROM a path; this is the
    /// same idea turned sideways, a z ALONG one.
    AlongWire { wire: Vec<crate::parse::Pt3>, drop: f64 },
}

impl Floor {
    /// The deepest floor within `slack` of `(x, y)` in plan — measured in the
    /// table's own key, so no direction has to be guessed.
    ///
    /// On the continuous floors this is `z_at` (a flat floor is flat, and the
    /// dome is smooth, so probing a neighbourhood buys slope and costs a
    /// discontinuity nobody has). On a TABLE it matters, because a table is
    /// allowed to step: a revolve clear's terraced floor drops a whole level
    /// across one riser, so a sub-micron plan error there becomes a
    /// millimetres-tall Z error that no tolerance in Z can absorb. Probing the
    /// key ±slack and taking the deepest answer is the same "the boundary
    /// belongs to the removal" convention `Section::contains` applies in plan,
    /// carried to the one place the floor is allowed to jump. Three samples
    /// suffice: the table is piecewise linear, so its minimum over a short
    /// interval is at an end or at a riser, and a riser inside the interval is
    /// caught by the end beyond it.
    fn z_at_slack(&self, x: f64, y: f64, slack: f64) -> f64 {
        if slack <= 0.0 {
            return self.z_at(x, y);
        }
        let (samples, key) = match self {
            Floor::Flat(_) | Floor::Dome { .. } => return self.z_at(x, y),
            Floor::Meridian { x: cx, y: cy, samples } => {
                let (dx, dy) = (x - cx, y - cy);
                (samples, (dx * dx + dy * dy).sqrt())
            }
            Floor::PathForm { path, samples } => (samples, dist_to_polygon(path, x, y)),
            // A wire's z varies ALONG the path, not with distance from it, so
            // the neighbourhood is probed in plan: the deepest of the point and
            // its four `slack` neighbours.
            Floor::AlongWire { .. } => {
                return [(0.0, 0.0), (slack, 0.0), (-slack, 0.0), (0.0, slack), (0.0, -slack)]
                    .iter()
                    .map(|(ox, oy)| self.z_at(x + ox, y + oy))
                    .fold(f64::INFINITY, f64::min);
            }
        };
        interp_table(samples, key)
            .min(interp_table(samples, key + slack))
            .min(interp_table(samples, (key - slack).max(0.0)))
    }

    /// `z_at_slack`'s dual, for CEILINGS: the HIGHEST surface within `slack` of
    /// `(x, y)` in plan. The floor helper probes for the deepest value because
    /// that is the permissive direction below a removal; above one, permissive
    /// is the other way, and reusing the floor probe would let a ceiling's
    /// riser shave a legitimate contact (DECISIONS.md → "rim-area
    /// accounting…", slice 1 — built for the tube's sampled envelope, whose
    /// end caps are exactly where a ceiling is steep).
    fn z_at_slack_max(&self, x: f64, y: f64, slack: f64) -> f64 {
        if slack <= 0.0 {
            return self.z_at(x, y);
        }
        let (samples, key) = match self {
            Floor::Flat(_) | Floor::Dome { .. } => return self.z_at(x, y),
            Floor::Meridian { x: cx, y: cy, samples } => {
                let (dx, dy) = (x - cx, y - cy);
                (samples, (dx * dx + dy * dy).sqrt())
            }
            Floor::PathForm { path, samples } => (samples, dist_to_polygon(path, x, y)),
            Floor::AlongWire { .. } => {
                return [(0.0, 0.0), (slack, 0.0), (-slack, 0.0), (0.0, slack), (0.0, -slack)]
                    .iter()
                    .map(|(ox, oy)| self.z_at(x + ox, y + oy))
                    .fold(f64::NEG_INFINITY, f64::max);
            }
        };
        interp_table(samples, key)
            .max(interp_table(samples, key + slack))
            .max(interp_table(samples, (key - slack).max(0.0)))
    }

    fn z_at(&self, x: f64, y: f64) -> f64 {
        match self {
            Floor::Flat(z) => *z,
            Floor::Dome { x: cx, y: cy, z, r } => {
                let (dx, dy) = (x - cx, y - cy);
                let rho2 = dx * dx + dy * dy;
                // Outside the equator the dome has no height; the Section test
                // has already excluded those points, so the clamp is only
                // guarding the sqrt at the rim itself.
                z + (r * r - rho2).max(0.0).sqrt()
            }
            Floor::Meridian { x: cx, y: cy, samples } => {
                let (dx, dy) = (x - cx, y - cy);
                let rho = (dx * dx + dy * dy).sqrt();
                // Inside the innermost circle the meridian has no surface — a
                // throat's waist, a funnel's tip — and the cavity bottoms out
                // there, so the floor runs flat inward from that circle. Outside
                // the mouth is the Section's business, not this one.
                interp_table(samples, rho)
            }
            // The form's table, evaluated at the plan distance to the path —
            // the whole surface in one lookup, because a solid of revolution
            // swept along a curve knows nothing but that distance. Outside the
            // table's span the form has no surface; the Section test has
            // already excluded those points, so the end clamps only guard the
            // band's own rims.
            Floor::PathForm { path, samples } => {
                let d = dist_to_polygon(path, x, y);
                interp_table(samples, d)
            }
            Floor::AlongWire { wire, drop } => crate::ops::profile::wire_cut_z(x, y, wire, *drop),
        }
    }

    /// The narrowest circle the floor reaches: a band's top circle. Polar by
    /// nature, so the path arm has no answer — and needs none: every consumer is
    /// reached through `revolved_axis`, which a polyline feature never has.
    fn inner_radius(&self) -> f64 {
        match self {
            Floor::Flat(_) | Floor::Dome { .. } | Floor::PathForm { .. } | Floor::AlongWire { .. } => 0.0,
            Floor::Meridian { samples, .. } => samples[0].0,
        }
    }

    /// Does the meridian climb going outward at its inner end? True only where
    /// the inner rim is the bottom of an inward-facing bowl (a torus's hole),
    /// false where it is a truncation the surface falls away from.
    fn climbs_outward_at_inner_rim(&self) -> bool {
        match self {
            Floor::Flat(_) | Floor::Dome { .. } | Floor::PathForm { .. } | Floor::AlongWire { .. } => false,
            Floor::Meridian { samples, .. } => samples.len() > 1 && samples[1].1 > samples[0].1,
        }
    }

    /// The widest circle the floor reaches, which is the removal's own footprint
    /// for a cavity: its mouth.
    fn outer_radius(&self) -> f64 {
        match self {
            Floor::Flat(_) | Floor::PathForm { .. } | Floor::AlongWire { .. } => 0.0,
            Floor::Dome { r, .. } => *r,
            Floor::Meridian { samples, .. } => samples[samples.len() - 1].0,
        }
    }
}

/// One table lookup for both table-carrying floors: end-clamped, binary-search
/// bracketed, linearly interpolated. `Meridian` keys it by rho off an axis,
/// `PathForm` by plan distance off a path — same table, different distance, and
/// one interpolator so the two arms cannot drift apart.
///
/// Binary search, not a scan: the motion check asks this for every sampled
/// point of every entry move.
use crate::parse::{interp_table, sample_meridian};

/// One op's worth of material gone: everything inside `section`, between
/// `floor` and `z_top`.
#[derive(Clone, Debug, PartialEq)]
pub struct Removal {
    pub section: Section,
    pub z_top: f64,
    pub floor: Floor,
    /// The removal's UPPER surface, where one exists that `z_top`'s flat plane
    /// cannot state: the swept-tube ceiling beside a rim (DECISIONS.md →
    /// "rim-area accounting…", fork 1A). A ball finishing a revolve sweeps the
    /// shell between the surface at `skin` and the surface at `skin + dia`,
    /// whose top beside the rim is a curve — claiming a flat `z_top` there
    /// either truncates the tube (the 25 rim advisories) or over-claims air
    /// the tool never enters, which `check_moves` would then bless. `None`
    /// means the removal is the full column up to `z_top`, which every
    /// removal was before this field existed.
    pub ceiling: Option<Floor>,
}

impl Section {
    /// The section's plan rims, each a closed loop of points — what the
    /// declared removal LOOKS like from above (Phase 11, lane B). A disc is
    /// one circle, an annulus two, a polygon its own loop, a band the two
    /// parallels of its path on the material side, and `All` the stock's
    /// footprint. Circles are flattened through the one chord-tolerance home
    /// every drawn circle goes through. The band's side is found by the
    /// band's own containment test rather than by an offset-sign convention:
    /// the offsetter's sign follows the loop's winding, and a boss and a
    /// pocket wind opposite ways.
    pub fn outlines(&self, stock: &Stock) -> Vec<Vec<Pt>> {
        fn circle(x: f64, y: f64, r: f64) -> Vec<Pt> {
            crate::ops::finish::circle_pts(x, y, r).into_iter().map(|(px, py)| Pt { x: px, y: py }).collect()
        }
        match self {
            Section::All => vec![crate::parse::derive_blank(stock)],
            Section::Disc { x, y, r } => vec![circle(*x, *y, *r)],
            Section::Annulus { x, y, r_inner, r_outer } => {
                let mut out = vec![circle(*x, *y, *r_outer)];
                if *r_inner > EPS {
                    out.push(circle(*x, *y, *r_inner));
                }
                out
            }
            Section::Ellipse { x, y, a, b, rot } => {
                let n = crate::ops::finish::circle_pts(0.0, 0.0, a.max(*b)).len().max(8);
                let (c, s) = (rot.pcos(), rot.psin());
                vec![(0..n)
                    .map(|i| {
                        let t = 2.0 * PI * i as f64 / n as f64;
                        let (ex, ey) = (a * t.pcos(), b * t.psin());
                        Pt { x: x + ex * c - ey * s, y: y + ex * s + ey * c }
                    })
                    .collect()]
            }
            Section::Polygon { points, holes } => std::iter::once(points.clone()).chain(holes.iter().cloned()).collect(),
            Section::PathBand { path, material_inside, d_inner, d_outer } => {
                let mut out = Vec::new();
                for d in [*d_inner, *d_outer] {
                    if d <= EPS {
                        out.push(path.clone());
                        continue;
                    }
                    // Whichever sign lands on the material side is the rim.
                    // (Vertices only, by name: this is a DRAWN rim and the
                    // containment certificate's points, and a chord across a
                    // rounded corner lies inside the true rim — it can only
                    // under-claim how far the band reaches. Phase 37 slice 0.)
                    for signed in [d, -d] {
                        let loops = crate::offset::offset_closed_polyline(path, signed);
                        let on_side = loops.iter().flatten().next().map_or(false, |pt| {
                            point_in_polygon(path, pt.x, pt.y) == *material_inside
                        });
                        if on_side {
                            out.extend(loops);
                            break;
                        }
                    }
                }
                out
            }
            // The chain itself, drawn once: a stretch has no closed offset to
            // draw, and a drawn rim may only under-claim how far a band
            // reaches (the note above).
            Section::ChainBand { chain, .. } => vec![chain.clone()],
        }
    }

    /// The section in a few words — the `--outline` voice of a removal's
    /// footprint, which the window's removal wires are a view of.
    pub fn describe(&self) -> String {
        let f = crate::gcode::f;
        match self {
            Section::All => "the whole stock".to_string(),
            Section::Disc { r, .. } => format!("disc r {}", f(*r)),
            Section::Annulus { r_inner, r_outer, .. } => format!("annulus r {}..{}", f(*r_inner), f(*r_outer)),
            Section::Ellipse { a, b, .. } => format!("ellipse {}x{}", f(*a), f(*b)),
            Section::Polygon { points, holes } if holes.is_empty() => format!("polygon ({} pts)", points.len()),
            Section::Polygon { points, holes } => format!("polygon ({} pts) around {} island{}", points.len(), holes.len(), if holes.len() == 1 { "" } else { "s" }),
            Section::PathBand { path, material_inside, d_inner, d_outer } => format!(
                "band {}..{} off a {}-pt path, material {}",
                f(*d_inner),
                f(*d_outer),
                path.len(),
                if *material_inside { "inside" } else { "outside" }
            ),
            Section::ChainBand { outline, chain, material_inside, d_inner, d_outer } => format!(
                "band {}..{} off a {}-pt stretch of a {}-pt path, material {}",
                f(*d_inner),
                f(*d_outer),
                chain.len(),
                outline.len(),
                if *material_inside { "inside" } else { "outside" }
            ),
        }
    }
}

impl Removal {
    /// The floor's height under a plan point — the declared surface the
    /// stock is left standing on there. Public for the window's removal wires
    /// (Phase 11, lane B); the model's own consumers read the slack-aware
    /// forms above.
    pub fn floor_z(&self, x: f64, y: f64) -> f64 {
        self.floor.z_at(x, y)
    }

    /// The removal's upper surface over a plan point: its ceiling where one
    /// exists (the swept tube's), else the flat `z_top`.
    pub fn ceiling_z(&self, x: f64, y: f64) -> f64 {
        match &self.ceiling {
            Some(c) => c.z_at(x, y),
            None => self.z_top,
        }
    }

    /// The removal's z extent in two words: the lowest floor sample (or the
    /// flat floor, or a dome's base) up to `z_top`.
    pub fn z_span(&self) -> (f64, f64) {
        let lo = match &self.floor {
            Floor::Flat(z) => *z,
            Floor::Dome { z, .. } => *z,
            Floor::Meridian { samples, .. } | Floor::PathForm { samples, .. } => {
                samples.iter().map(|s| s.1).fold(f64::MAX, f64::min)
            }
            Floor::AlongWire { wire, drop } => wire.iter().map(|p| p.z - drop).fold(f64::MAX, f64::min),
        };
        (lo, self.z_top)
    }
    /// `slack` grows the removal, so a larger one means "only count material the
    /// tool is *well* inside". EPS is the numerical default; the motion check
    /// passes the geometric tolerance instead (see `check_moves`).
    fn removes(&self, x: f64, y: f64, z: f64, slack: f64) -> bool {
        // One rule, stated once: a point counts as removed if it lies inside
        // this removal allowing `slack` of error in Z **and in plan**. A tip
        // resting exactly on the floor it just cut is contact; so is a tool
        // edge riding the wall it just cut. The plan half reaches the FLOOR as
        // well as the section — see `Floor::z_at_slack` for why a terraced
        // floor makes that the difference between a program and an error.
        //
        // The ceiling reads `z_at_slack_max`, the floor probe's dual: within
        // `slack` in plan, the floor answers its deepest and the ceiling its
        // highest, so both err toward "removed" at their own boundary.
        z < self.z_top + slack
            && z > self.floor.z_at_slack(x, y, slack) - slack
            && self.ceiling.as_ref().is_none_or(|c| z < c.z_at_slack_max(x, y, slack) + slack)
            && self.section.contains(x, y, slack)
    }
}

/// The stock, minus what the ops so far have declared away.
#[derive(Clone, Debug, PartialEq)]
pub struct RemainingStock {
    pub stock: Stock,
    pub removals: Vec<Removal>,
    /// False when any op in the modeled prefix had no closed-form removal, in
    /// which case this model over-states material and no consumer may read it.
    /// Total accounting: the gap is reported, never absorbed.
    pub complete: bool,
}

impl RemainingStock {
    /// Is there material at this point? Ordering does not matter — every op
    /// subtracts and none adds, so the prefix is a plain set difference.
    pub fn material_at(&self, x: f64, y: f64, z: f64) -> bool {
        self.material_at_slack(x, y, z, EPS)
    }

    /// `material_at` with the removals grown by `slack` — "is the tool more than
    /// `slack` inside standing material", which is the only form a hard error
    /// can honestly use.
    pub fn material_at_slack(&self, x: f64, y: f64, z: f64, slack: f64) -> bool {
        let s = &self.stock;
        // The plan test is the shape's own (Phase 46 slice 1): the box's four
        // sides, or the round's radius — closed form either way, which is
        // what lets a hard error read it.
        let in_plan = match s.shape {
            crate::parse::StockShape::Box => x > s.x + EPS && x < s.x + s.w - EPS && y > s.y + EPS && y < s.y + s.l - EPS,
            crate::parse::StockShape::Cylinder { dia } => {
                let (cx, cy) = s.axis();
                (x - cx).phypot(y - cy) < dia / 2.0 - EPS
            }
            // The loop's own test (Phase 54 slice 5a): inside, and not on
            // the edge — the polygon helpers every island reads through.
            crate::parse::StockShape::Profile { ref points } => point_in_polygon(points, x, y) && dist_to_polygon(points, x, y) > EPS,
        };
        let in_blank = in_plan && z < s.z - EPS && z > s.z - s.h + EPS;
        if !in_blank {
            return false;
        }
        !self.removals.iter().any(|r| r.removes(x, y, z, slack))
    }
}

/// Who the DECLARED PART says stands at a point — the part being the material
/// a program promises will SURVIVE it (Phase 9 lane C, `DECISIONS.md` →
/// "Phase 9 lane C — over-cut of the declared part, in the field…").
///
/// **The part is not the stock minus its removals, and it is not the features
/// restated.** The DRAFT measured both: the end-state model says material in
/// 0 of the 9,516 cells of a post declared inside a cleared pocket, because
/// every op subtracts and none adds, so the pocket's footprint reads the post
/// away; and restating every cavity's void feature-by-feature (counterbores,
/// chamfer wedges, skins) would be the duplicated-derived-description bug the
/// removal model exists to prevent. So the part's VOIDS are read from the
/// end-state model — `removals_for`'s own geometry, skins included — and its
/// SOLIDS are the standing prismatic declarations re-asserted on top:
/// `CylinderBoss`, `EllipticBoss`, `ProfileBoss`, `ContourBoss`, each over its
/// own Z span read through the convention its emitter reads. The caps and
/// standing revolves need nothing added — their removal is the material ABOVE
/// the surface, so the model keeps the dome solid by itself.
///
/// **Solid wins over void where declarations overlap** (fork 3A): a declared
/// boss is the strongest statement a program makes about a region, and a
/// pocket's outline is the author's hand-traced outer boundary. What the
/// overlap MEANS — island, subtraction, mistake — is T5's profile-composition
/// question and is not decided here; this only makes the post reportable.
///
/// **Declines where the model declines.** An op with no closed-form removal
/// makes the model over-state material, and a part that over-states solid
/// would accuse the sweep falsely — so `complete()` is the model's own flag,
/// and a consumer that finds it false must make no claim (the law on
/// `RemainingStock`). A finished boss declines today for exactly this reason:
/// the boss finish's cleared surround is a path fact (`removals_for`'s `None`).
pub struct DeclaredPart<'a> {
    end: RemainingStock,
    stock_top: f64,
    /// The standing declarations, by name, sorted so attribution is
    /// deterministic whatever order the feature map iterates in.
    standing: Vec<(&'a str, &'a Feature)>,
    /// `own_void[i][j]`: removal `j` is a void nested inside standing feature
    /// `i`'s footprint — that solid's own hole. Judged once here, on the
    /// outlines, so `claim_at` stays a per-cell lookup.
    own_void: Vec<Vec<bool>>,
    /// What each op was PERMITTED to overhang (Phase 32 slice 3, fork D):
    /// its swath and its floor. Undeclared stock inside one is stock an op
    /// may sweep — the same rule `check_moves` reads, read here so the field
    /// does not call a permitted overhang a violation. A DECLARED feature
    /// standing there is untouched by this: it stays the part, and sweeping
    /// it stays a violation (Phase 29's ring).
    permitted: Vec<Overhang>,
}

/// Who claims a point of the part: a standing feature by name, or the stock's
/// own remainder — material no declaration gives away.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PartClaim<'a> {
    Feature(&'a str),
    Stock,
}

impl<'a> DeclaredPart<'a> {
    pub fn of(p: &'a Part) -> Self {
        let end = if p.operations.is_empty() {
            remaining_after(p, 0)
        } else {
            remaining_after(p, p.operations.len() - 1)
        };
        let mut standing: Vec<(&str, &Feature)> = p
            .features
            .iter()
            .filter(|(_, f)| {
                matches!(
                    f,
                    Feature::CylinderBoss { .. }
                        | Feature::ProfileBoss { .. }
                        | Feature::ContourBoss { .. }
                )
            })
            .map(|(n, f)| (n.as_str(), f))
            .collect();
        standing.sort_by(|a, b| a.0.cmp(b.0));
        let permitted: Vec<Overhang> = p.operations.iter().flat_map(|op| face_overhang(p, op)).collect();
        let own_void = standing
            .iter()
            .map(|(_, f)| end.removals.iter().map(|r| nested_in(&r.section, &p.stock, f, p.stock.z)).collect())
            .collect();
        DeclaredPart { end, stock_top: p.stock.z, standing, own_void, permitted }
    }

    /// The model's own total-accounting flag; false means no claim may be made.
    pub fn complete(&self) -> bool {
        self.end.complete
    }

    /// Who says material stands here, or None where the part is void. A
    /// standing feature answers first — solid wins over the void that
    /// CONTAINS it — unless the point was removed by a void NESTED inside the
    /// feature's own footprint, which is the feature's own (a hole down a
    /// post); then the model answers. The stock's remainder answers for
    /// everything the end-state model still holds.
    ///
    /// **The narrower declaration wins** (slice 1's correction to fork 3A).
    /// "Solid wins" was argued for a boss standing in a pocket's footprint and
    /// is right there; measured on a bore declared coaxially inside the boss
    /// and drilled — a hollow post, a real part — it accused the drill of
    /// 100.9 mm³ the declaration had given away. The geometry tells the two
    /// apart without a semantics: the pocket's section contains the boss, the
    /// bore's section lies within it. A removal is NESTED when its plan
    /// OUTLINE lies strictly inside the standing footprint (the footprint,
    /// not its box — Phase 26 slice 1) — generous on
    /// the removal's side, so a void that straddles the wall is not nested and
    /// the boss keeps its claim.
    pub fn claim_at(&self, x: f64, y: f64, z: f64) -> Option<PartClaim<'_>> {
        for (i, (name, f)) in self.standing.iter().enumerate() {
            if !standing_solid_at(f, self.stock_top, x, y, z) {
                continue;
            }
            let own_void = self
                .end
                .removals
                .iter()
                .zip(&self.own_void[i])
                .any(|(r, &nested)| nested && r.removes(x, y, z, EPS));
            if !own_void {
                return Some(PartClaim::Feature(name));
            }
        }
        if !self.end.material_at(x, y, z) {
            return None;
        }
        // Undeclared stock an op was permitted to overhang is not part it
        // violated by sweeping: the permission is a rule, and this is its
        // second reader (`check_moves` is the first).
        if self.permitted.iter().any(|o| o.permits(x, y, z)) {
            return None;
        }
        Some(PartClaim::Stock)
    }
}

/// Whether a removal's section lies within a standing feature's footprint:
/// every vertex of the section's own outline strictly inside it — the
/// FOOTPRINT, not the plan box lane C first tested (Phase 26 slice 1, the
/// correction lane D's ruling owed it: an L-shaped void's box can poke out of
/// a solid the void sits wholly inside). The whole-stock section (`Floor`)
/// is never nested. Read once per (standing, removal) pair when the declared
/// part is built, never per cell.
fn nested_in(section: &Section, stock: &crate::parse::Stock, f: &Feature, stock_top: f64) -> bool {
    if matches!(section, Section::All) {
        return false;
    }
    let outlines = section.outlines(stock);
    !outlines.is_empty()
        && outlines.iter().flatten().all(|q| standing_footprint_contains(f, stock_top, q.x, q.y))
}

/// Strictly inside a standing prismatic declaration's PLAN footprint — the
/// XY half of `standing_solid_at`, one home for both.
fn standing_footprint_contains(f: &Feature, _stock_top: f64, x: f64, y: f64) -> bool {
    match f {
        Feature::CylinderBoss { dia, x: cx, y: cy, .. } => {
            let (dx, dy) = (x - cx, y - cy);
            let r = dia / 2.0 - EPS;
            dx * dx + dy * dy < r * r
        }
        Feature::ProfileBoss { points, .. } => point_in_polygon(points, x, y) && dist_to_polygon(points, x, y) > EPS,
        Feature::ContourBoss { points, .. } => {
            let xy: Vec<Pt> = points.iter().map(|p| Pt { x: p.x, y: p.y }).collect();
            point_in_polygon(&xy, x, y) && dist_to_polygon(&xy, x, y) > EPS
        }
        _ => false,
    }
}

/// Strictly inside a standing prismatic declaration. STRICT on purpose — the
/// opposite lean from `Section::contains`, because this is the part's solid
/// and a point exactly on a boss's wall is the wall the finish rides, not
/// material the sweep took. The Z spans are each emitter's own convention:
/// the round and elliptic bosses stand UP from `z` (`finish_cylinder_boss`
/// reads `z` as the base), the polyline boss hangs DOWN from its contour top
/// (`derive_contour_top`, the one home), and the 3D wire's guaranteed core is
/// its lowest vertex down by `extent` — an under-claim of a sloped top, which
/// is the silent direction.
fn standing_solid_at(f: &Feature, stock_top: f64, x: f64, y: f64, z: f64) -> bool {
    let between = |lo: f64, hi: f64| z > lo + EPS && z < hi - EPS;
    let in_span = match f {
        Feature::CylinderBoss { extent, z: base, .. } => {
            between(*base, base + extent)
        }
        Feature::ProfileBoss { z: top, extent, .. } => {
            let top = derive_contour_top(stock_top, *top);
            between(top - extent, top)
        }
        Feature::ContourBoss { points, extent } => {
            let lowest = points.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
            between(lowest - extent, lowest)
        }
        _ => return false,
    };
    in_span && standing_footprint_contains(f, stock_top, x, y)
}

// ---- nesting: which declaration lies inside which ---------------------------
// Phase 26 slice 1 (`DECISIONS.md` → "Phase 10 lane D — overlap semantics…",
// forks 1A/2A/5A, and "Phase 26 opens: the island DRAFT…", forks 5b/6/7). A
// standing feature inside a cavity's footprint is an ISLAND of that cavity; a
// void inside a standing footprint is that solid's own HOLE; anything that
// crosses a wall has no meaning in this vocabulary and refuses by name. All of
// it is derived from the declarations — never from `run` order, so the part
// does not depend on the program — and this is the one home every consumer
// reads: `validate`, the emitters, the skeletons and the over-cut check.

/// How one closed plan outline lies against another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Nesting {
    Apart,
    /// The first outline lies wholly inside the second.
    Inside,
    /// The first outline wholly surrounds the second.
    Around,
    Straddles,
}

fn segments_cross(a1: Pt, a2: Pt, b1: Pt, b2: Pt) -> bool {
    let side = |p: Pt, q: Pt, r: Pt| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
    let (d1, d2) = (side(b1, b2, a1), side(b1, b2, a2));
    let (d3, d4) = (side(a1, a2, b1), side(a1, a2, b2));
    // PROPER crossings only: two outlines that merely touch (a boss whose wall
    // is tangent to a pocket's) are decided by their vertices, not refused for
    // a shared point the chord flattening may or may not reproduce.
    d1 * d2 < -EPS && d3 * d4 < -EPS
}

/// Where (a1, a2) properly crosses (b1, b2): the parameter along a, or None.
fn cross_param(a1: Pt, a2: Pt, b1: Pt, b2: Pt) -> Option<f64> {
    if !segments_cross(a1, a2, b1, b2) {
        return None;
    }
    let (dx, dy) = (a2.x - a1.x, a2.y - a1.y);
    let (ex, ey) = (b2.x - b1.x, b2.y - b1.y);
    let den = dx * ey - dy * ex;
    if den.abs() <= EPS {
        return None;
    }
    Some(((b1.x - a1.x) * ey - (b1.y - a1.y) * ex) / den)
}

/// Whether outline `a` reaches INTO `b` by more than `depth` where their
/// boundaries cross: at one of a's vertices inside b, or at the middle of a
/// stretch of one of a's edges between two crossings of b (a bar through a
/// bar, where no vertex of either lies inside the other).
fn reaches_into(a: &[Pt], b: &[Pt], depth: f64) -> bool {
    let deep = |q: Pt| point_in_polygon(b, q.x, q.y) && dist_to_polygon(b, q.x, q.y) > depth;
    if a.iter().any(|q| deep(*q)) {
        return true;
    }
    for i in 0..a.len() {
        let (a1, a2) = (a[i], a[(i + 1) % a.len()]);
        let mut ts: Vec<f64> = (0..b.len()).filter_map(|j| cross_param(a1, a2, b[j], b[(j + 1) % b.len()])).collect();
        ts.sort_by(|x, y| x.partial_cmp(y).unwrap());
        if ts.windows(2).any(|w| {
            let m = (w[0] + w[1]) / 2.0;
            deep(Pt { x: a1.x + (a2.x - a1.x) * m, y: a1.y + (a2.y - a1.y) * m })
        }) {
            return true;
        }
    }
    false
}

/// `a` against `b`, both closed outlines of either winding. The footprint
/// itself, never its bounding box: an L-shaped boss whose box pokes out of a
/// pocket it sits wholly inside is nested, and a box test says otherwise (the
/// correction lane D's ruling owed lane C's `nested_in`).
pub fn plan_nesting(a: &[Pt], b: &[Pt]) -> Nesting {
    if a.len() < 3 || b.len() < 3 {
        return Nesting::Apart;
    }
    // Two outlines whose boundaries run ALONG each other — a boss whose corner
    // is notched by a bore, its arc concentric with the bore's rim, or a bore
    // tangent to a boss's wall (the real part's central hole and the bores at
    // the heads' corners) — cross back and forth within the chord tolerance:
    // two chordings of one curve, or a chorded arc against the line it kisses.
    // That is a touch, not a straddle: the crossings count only where one
    // outline reaches into the other by more than two chord tolerances.
    let crossed = (0..a.len()).any(|i| {
        (0..b.len()).any(|j| segments_cross(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()]))
    });
    if crossed {
        let depth = 2.0 * crate::CHORD_TOL_MM;
        if reaches_into(a, b, depth) || reaches_into(b, a, depth) {
            return Nesting::Straddles;
        }
    }
    // No edge crosses, so each outline is wholly on one side of the other and
    // one vertex decides — taken OFF the other's boundary by more than the
    // chording where one exists, so a shared vertex (or a chord of a shared
    // arc) cannot flip the answer.
    let clear_of = |pts: &[Pt], other: &[Pt]| pts.iter().copied().find(|q| dist_to_polygon(other, q.x, q.y) > crate::CHORD_TOL_MM);
    if let Some(q) = clear_of(a, b) {
        if point_in_polygon(b, q.x, q.y) {
            return Nesting::Inside;
        }
    }
    if let Some(q) = clear_of(b, a) {
        if point_in_polygon(a, q.x, q.y) {
            return Nesting::Around;
        }
    }
    Nesting::Apart
}

fn ring(pts: Vec<(f64, f64)>) -> Vec<Pt> {
    pts.into_iter().map(|(x, y)| Pt { x, y }).collect()
}

/// A declaration's plan footprint as one closed outline, with the Z span it
/// occupies and whether it STANDS (a boss, a standing revolve) or is a void.
/// `None` for the kinds this judgment does not read — the `Plane` (it has no
/// footprint; `host_of` handles it by span alone), the revolve cavities, the
/// free surfaces — which is the under-claiming direction.
struct Occupancy {
    outline: Vec<Pt>,
    lo: f64,
    hi: f64,
    stands: bool,
}

/// The height a declared feature stands up to, for the schedule's top-face
/// rule: a boss's or a standing contour's top, a Plane's or Face's own
/// height. `None` for a void or a free surface, which nothing has to clear
/// above before it is cut.
pub fn standing_top(p: &Part, f: &Feature) -> Option<f64> {
    match f {
        Feature::Plane { z } | Feature::Face { z, .. } => Some(*z),
        _ => occupancy(p, f).filter(|o| o.stands).map(|o| o.hi),
    }
}

fn occupancy(p: &Part, f: &Feature) -> Option<Occupancy> {
    use crate::ops::finish::circle_pts;
    let top = p.stock.z;
    let xy = |pts: &[crate::parse::Pt3]| pts.iter().map(|q| Pt { x: q.x, y: q.y }).collect::<Vec<_>>();
    Some(match f {
        Feature::CylinderBoss { dia, extent, x, y, z, .. } => {
            Occupancy { outline: ring(circle_pts(*x, *y, dia / 2.0)), lo: *z, hi: z + extent, stands: true }
        }
        Feature::ProfileBoss { points, z, extent, .. } => {
            let t = derive_contour_top(top, *z);
            Occupancy { outline: points.clone(), lo: t - extent, hi: t, stands: true }
        }
        Feature::ContourBoss { points, extent } => {
            let lowest = points.iter().map(|q| q.z).fold(f64::INFINITY, f64::min);
            let highest = points.iter().map(|q| q.z).fold(f64::NEG_INFINITY, f64::max);
            Occupancy { outline: xy(points), lo: lowest - extent, hi: highest, stands: true }
        }
        Feature::ProfilePocket { points, z, extent, .. } => {
            let t = derive_contour_top(top, *z);
            Occupancy { outline: points.clone(), lo: t - extent, hi: t, stands: false }
        }
        // A rim WIRE declares a wall and nothing of its interior ("a rim
        // wire's interior is undeclared" — Phase 10 lane A, slice 2), so it is
        // no cavity a feature could stand in. The DRAFT's fork 7 listed it as
        // a host to refuse; three field-voice tests that sink a dome inside
        // one corrected that — the corpus's second correction to this slice.
        Feature::ContourPocket { .. } => return None,
        Feature::CylinderBore { dia, extent, x, y, z, .. } => {
            let t = derive_contour_top(top, *z);
            Occupancy { outline: ring(circle_pts(*x, *y, dia / 2.0)), lo: t - extent, hi: t, stands: false }
        }
        // The tilted bore's footprint is its plan ENVELOPE over the whole
        // slab it spans (Phase 41 slice 5) — not its section at any one
        // height, because the tool occupies the whole depth and the cylinder
        // leans. One home for that curve (`derive_tilted_bore_envelope`),
        // so the nesting tests, the bridged-void reading and the arc
        // construction all read the same loop.
        Feature::TiltedBore { dia, extent, x, y, tilt, heading } => Occupancy {
            outline: ring(
                crate::parse::derive_tilted_bore_envelope(*dia, *extent, *x, *y, *tilt, *heading)
                    .into_iter()
                    .map(|q| (q.x, q.y))
                    .collect(),
            ),
            lo: top - extent,
            hi: top,
            stands: false,
        },
        other => {
            // The standing revolves: the rim circle, from the base plane to the
            // crown — the offset meridian's own samples at no allowance.
            let (x, y, base) = crate::parse::derive_revolve_datum(other)?;
            let m = crate::parse::derive_revolve_offset_meridian(other, 0.0)?;
            let rim = m.samples.last()?.0;
            let crown = m.samples.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
            Occupancy { outline: ring(circle_pts(x, y, rim)), lo: base, hi: crown, stands: true }
        }
    })
}

/// The kinds a plan family can honestly go around: a footprint that is the
/// same at every height (fork 7).
fn island_kind(f: &Feature) -> bool {
    matches!(f, Feature::CylinderBoss { .. } | Feature::ProfileBoss { .. })
}

/// A declaration's footprint and the height material stands to inside it —
/// whichever kind of declaration it is (Phase 29 slice 2). Slice 1 read only
/// the FLOOR features, where the floor's own z is the top of what survives; a
/// wall or a fence is a BOSS, and the thing that stands beside a floor is
/// quite as often standing material as it is another floor. Both answer the
/// same question, so both answer here.
///
/// The `Plane` is skipped on purpose — it has no footprint, so it would cover
/// every point and close every edge — which is the under-claiming direction.
fn declared_standing(p: &Part, f: &Feature) -> Option<(Vec<Pt>, f64)> {
    if let Some((Some(outline), lo, _hi)) = cut_span(p, f) {
        return Some((outline, lo));
    }
    occupancy(p, f).filter(|o| o.stands).map(|o| (o.outline, o.hi))
}

/// The plan footprints of every OTHER footprinted floor-feature whose floor
/// stands strictly above `above` (Phase 29 slice 1). These are the neighbours
/// that close a region's edges, and they are the one thing a swath pushed out
/// across an OPEN edge must never swallow. Sorted by name so the clip order,
/// and therefore the emitted boundary, is deterministic whatever order the
/// feature map iterates in.
pub fn declared_neighbours_above<'a>(p: &'a Part, except: &str, above: f64) -> Vec<(&'a str, Vec<Pt>)> {
    let mut out: Vec<(&str, Vec<Pt>)> = p
        .features
        .iter()
        .filter(|(name, _)| name.as_str() != except)
        .filter_map(|(name, f)| {
            let (outline, top) = declared_standing(p, f)?;
            (top > above).then_some((name.as_str(), outline))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(b.0));
    out
}

/// The kinds that can HOST an island today (fork 7): the polyline pocket, and
/// the faced plane — a cavity whose wall is the blank.
fn host_kind(f: &Feature) -> bool {
    matches!(f, Feature::ProfilePocket { .. } | Feature::Plane { .. } | Feature::Face { .. })
}

/// A cavity's cut span and, for a footprinted one, its outline. The `Plane` is
/// the whole stock in plan from the stock's top down to itself.
fn cut_span(p: &Part, f: &Feature) -> Option<(Option<Vec<Pt>>, f64, f64)> {
    match f {
        Feature::Plane { z } => Some((None, *z, p.stock.z)),
        // A face is cut from the stock's top down to itself, inside its outer
        // loop (Phase 28).
        Feature::Face { z, loops } => Some((loops.first().cloned(), *z, p.stock.z)),
        _ => occupancy(p, f).filter(|o| !o.stands).map(|o| (Some(o.outline), o.lo, o.hi)),
    }
}

/// A straddle is symmetric, so the sentence has to choose its subject: the
/// SMALLER footprint crosses the larger one's wall (a post across a pocket's
/// wall, a hole across a post's) — whichever of the pair the walk met first.
fn straddle_sentence(a: &str, a_outline: &[Pt], b: &str, b_outline: &[Pt]) -> String {
    let area = |o: &[Pt]| (0..o.len()).map(|i| o[i].x * o[(i + 1) % o.len()].y - o[(i + 1) % o.len()].x * o[i].y).sum::<f64>().abs();
    let (small, big) = if area(a_outline) <= area(b_outline) { (a, b) } else { (b, a) };
    format!(
        "'{small}' straddles '{big}''s wall — a feature is inside another's footprint (an island, or a hole) or outside it; neither has a meaning in this vocabulary"
    )
}

/// What one standing feature is to one cavity.
enum Standing {
    /// Apart in plan, or their spans never share a height.
    Unrelated,
    Island,
    Refused(String),
}

/// The width of "these two heights agree", declaration against declaration.
/// Authored heights are three-decimal numbers, and one of a pair is often a
/// rounded transcendental — `catenoid_truncated.cam` faces its blank to
/// `z=6.238`, the truncated neck's own top to three places, and the meridian's
/// crown is 6.2380…-something. A face that MEETS a crown is not a face the
/// crown stands through (the corpus's first correction to this slice).
const Z_AGREE: f64 = 1e-3;

fn judge_standing(p: &Part, s_name: &str, s: &Feature, c_name: &str, c: &Feature) -> Standing {
    let (Some(so), Some((c_outline, c_lo, c_hi))) = (occupancy(p, s), cut_span(p, c)) else {
        return Standing::Unrelated;
    };
    if !so.stands || so.hi <= c_lo + Z_AGREE || so.lo >= c_hi - Z_AGREE {
        return Standing::Unrelated;
    }
    let nesting = match &c_outline {
        Some(outline) => plan_nesting(&so.outline, outline),
        None => Nesting::Inside,
    };
    let (s_kind, c_kind) = (crate::template::spec_for(s).variant, crate::template::spec_for(c).variant);
    match nesting {
        Nesting::Apart => return Standing::Unrelated,
        // A cavity wholly inside a standing footprint is that solid's own
        // hole — the void-side half of the ruling, judged by `judge_void`.
        Nesting::Around => return Standing::Unrelated,
        // A face's boundary is OPEN, not a wall: something standing across it
        // — a wall flush with the part's edge — is a bite out of the region
        // (`region::split_islands`), and means exactly what it looks like.
        Nesting::Straddles if matches!(c, Feature::Face { .. }) => {}
        // A cavity that leaves this solid on its way out of the blank is a
        // notch in the solid's wall, the same as a hole in it.
        Nesting::Straddles if c_outline.as_deref().is_some_and(|o| runs_out_to_air(p, o, &so.outline)) => return Standing::Unrelated,
        Nesting::Straddles => {
            return Standing::Refused(straddle_sentence(s_name, &so.outline, c_name, c_outline.as_deref().unwrap_or(&[])))
        }
        Nesting::Inside => {}
    }
    if !island_kind(s) {
        return Standing::Refused(format!(
            "'{s_name}' is a {s_kind} standing inside '{c_name}''s footprint — an island is cut around in plan, and a {s_kind}'s outline changes with height; only a CylinderBoss, an EllipticBoss or a ProfileBoss can be an island yet"
        ));
    }
    if !host_kind(c) {
        return Standing::Refused(format!(
            "'{s_name}' stands inside '{c_name}', a {c_kind} — only a ProfilePocket or a faced Plane can be cleared around an island yet"
        ));
    }
    if so.lo > c_lo + Z_AGREE {
        return Standing::Refused(format!(
            "'{s_name}' stands from z {} inside '{c_name}', whose floor is z {} — a boss floating above a cavity's floor is an undercut, and nothing in this vocabulary cuts beneath it",
            crate::gcode::f(so.lo),
            crate::gcode::f(c_lo)
        ));
    }
    // A boss taller than the cavity it stands in is a post through the
    // cavity's mouth: the cavity's island from its floor to its top, and
    // above that the island of whatever cuts down to the mouth around it.
    // Something has to: with nothing declared there the post's upper part
    // would stand in uncut material, which is a wall, not a post.
    if so.hi > c_hi + Z_AGREE && !column_declared_removed_above(p, &so, c_name, c_hi) {
        return Standing::Refused(format!(
            "'{s_name}' stands to z {} inside '{c_name}', whose top is z {} — a boss taller than the cavity it stands in needs the material round it above the cavity declared removed; face or pocket down to z {} around it by name, or shorten the boss",
            crate::gcode::f(so.hi),
            crate::gcode::f(c_hi),
            crate::gcode::f(c_hi)
        ));
    }
    if so.hi < c_hi - Z_AGREE && !column_declared_removed(p, &so, c_name, c_hi) {
        return Standing::Refused(format!(
            "'{s_name}' stands to z {} inside '{c_name}', which cuts from z {} — a boss shorter than the cut around it is a step in the floor (a terrace), and nothing declares the material above it removed; face or pocket that material away by name, or raise the boss",
            crate::gcode::f(so.hi),
            crate::gcode::f(c_hi)
        ));
    }
    Standing::Island
}

/// Fork 5b, order-free: the column above a short island, up to the cavity's
/// top, is some OTHER declaration's removal — read from the declarations, never
/// the `run` prefix. (A skim `Plane` over a boss, then a lower face around it.)
fn column_declared_removed(p: &Part, so: &Occupancy, host: &str, c_hi: f64) -> bool {
    p.features.iter().any(|(name, f)| {
        if name == host {
            return false;
        }
        let Some((outline, lo, hi)) = cut_span(p, f) else { return false };
        let covers = match &outline {
            Some(o) => plan_nesting(&so.outline, o) == Nesting::Inside,
            None => true,
        };
        covers && lo <= so.hi + Z_AGREE && hi >= c_hi - Z_AGREE
    })
}

/// The same question the other way up: round a boss taller than its cavity,
/// some other declaration cuts from the boss's top or above down to the
/// cavity's mouth. (A level faced round a post, then a pocket round its foot.)
fn column_declared_removed_above(p: &Part, so: &Occupancy, host: &str, c_hi: f64) -> bool {
    p.features.iter().any(|(name, f)| {
        if name == host {
            return false;
        }
        let Some((outline, lo, hi)) = cut_span(p, f) else { return false };
        let covers = match &outline {
            Some(o) => plan_nesting(&so.outline, o) == Nesting::Inside,
            None => true,
        };
        covers && (lo - c_hi).abs() <= Z_AGREE && hi >= so.hi - Z_AGREE
    })
}

/// The z a standing island sits on — the floor a Clear AROUND it is declared
/// at (Phase 61 slice 3, the *Clear around it…* door). `None` for a feature
/// that is not an island kind (a CylinderBoss, an EllipticBoss, a ProfileBoss).
pub fn standing_base_z(p: &Part, f: &Feature) -> Option<f64> {
    if !island_kind(f) {
        return None;
    }
    occupancy(p, f).filter(|o| o.stands).map(|o| o.lo)
}

/// The islands of a host cavity, by name, sorted — derived from containment,
/// never declared (lane D fork 2A). Empty for a feature that hosts none.
pub fn islands_of<'a>(p: &'a Part, host: &str) -> Vec<(&'a str, &'a Feature)> {
    let Some(c) = p.features.get(host) else { return Vec::new() };
    let mut found: Vec<(&str, &Feature)> = p
        .features
        .iter()
        .filter(|(n, _)| n.as_str() != host)
        .filter(|(n, s)| matches!(judge_standing(p, n, s, host, c), Standing::Island))
        .map(|(n, s)| (n.as_str(), s))
        .collect();
    found.sort_by(|a, b| a.0.cmp(b.0));
    found
}

/// The cavity a standing feature is an island of, if any — the first by name,
/// so the answer is deterministic whatever order the feature map iterates in.
pub fn host_of<'a>(p: &'a Part, island: &str) -> Option<&'a str> {
    let s = p.features.get(island)?;
    let mut hosts: Vec<&str> = p
        .features
        .iter()
        .filter(|(n, _)| n.as_str() != island)
        .filter(|(n, c)| matches!(judge_standing(p, island, s, n, c), Standing::Island))
        .map(|(n, _)| n.as_str())
        .collect();
    hosts.sort();
    hosts.first().copied()
}

/// Whether a declared VOID opens into a floor at height `z` inside `outer` —
/// its mouth is in that floor: wholly inside the region in plan, reaching up
/// to the floor or above it, and going deeper than it. (A void that ends above
/// the floor is air the region's own removal already covers; one that starts
/// below it is not in this floor at all.) Phase 27's BRIDGED, read by
/// `region::of`.
pub(crate) fn mouth_in_floor(p: &Part, f: &Feature, outer: &[Pt], z: f64) -> bool {
    let Some(o) = occupancy(p, f) else { return false };
    !o.stands && o.hi >= z - Z_AGREE && o.lo < z - Z_AGREE && plan_nesting(&o.outline, outer) == Nesting::Inside
}

/// A host's islands as the offsetter wants them: a round boss a true circle,
/// everything else its authored or flattened loop. Sorted by name, like
/// `islands_of`, so the family — and the golden — do not depend on the order
/// a map iterates in.
pub fn island_outlines<'a>(p: &'a Part, host: &str) -> Vec<(&'a str, crate::offset::Island)> {
    use crate::offset::Island;
    islands_of(p, host)
        .into_iter()
        .filter_map(|(name, f)| {
            let island = match f {
                Feature::CylinderBoss { dia, x, y, .. } => Island::Circle { x: *x, y: *y, r: dia / 2.0 },
                _ => Island::Loop(occupancy(p, f)?.outline),
            };
            Some((name, island))
        })
        .collect()
}

/// The same islands as plain outlines grown by `skin` — what a removal that
/// goes around them leaves standing.
pub fn island_holes(p: &Part, host: &str, skin: f64) -> Vec<Vec<Pt>> {
    use crate::offset::Island;
    island_outlines(p, host)
        .into_iter()
        .filter_map(|(_, island)| match island {
            Island::Circle { x, y, r } => Some(ring(crate::ops::finish::circle_pts(x, y, r + skin))),
            Island::Loop(pts) if skin <= 0.0 => Some(pts),
            Island::Loop(pts) => {
                let ccw = crate::ops::clear::make_ccw(&pts);
                let grown = crate::offset::offset_closed_polyline_verts(&ccw, -skin);
                grown.first().map(|l| crate::offset::flatten_loop(l))
            }
        })
        .collect()
}

/// A void against a standing solid: nested inside it is the solid's own hole
/// (the hollow post — silent), straddling its wall refuses (lane D fork 5A)
/// unless the void leaves the solid on its way out of the blank.
fn judge_void(p: &Part, v_name: &str, v: &Feature, s_name: &str, s: &Feature) -> Option<String> {
    let (vo, so) = (occupancy(p, v)?, occupancy(p, s)?);
    if vo.stands || !so.stands || vo.hi <= so.lo + Z_AGREE || vo.lo >= so.hi - Z_AGREE {
        return None;
    }
    (plan_nesting(&vo.outline, &so.outline) == Nesting::Straddles && !runs_out_to_air(p, &vo.outline, &so.outline))
        .then(|| straddle_sentence(v_name, &vo.outline, s_name, &so.outline))
}

/// Whether every part of `void` outside `solid` reaches past the blank. A
/// pocket drawn out past the blank is open on that side; where it crosses a
/// standing wall on the way out it is a notch in that wall, and what lies
/// between the wall and the blank's edge is stock the wall's own cut removes.
fn runs_out_to_air(p: &Part, void: &[Pt], solid: &[Pt]) -> bool {
    let blank = crate::region::blank_of(p);
    let outside = crate::offset::subtract_all(void, solid);
    !outside.is_empty()
        && outside.iter().all(|piece| {
            piece.iter().any(|q| !point_in_polygon(&blank, q.x, q.y) && dist_to_polygon(&blank, q.x, q.y) > crate::CHORD_TOL_MM)
        })
}

/// Every overlap the vocabulary cannot mean, refused by name with both
/// features in the sentence. Called from `validate`; pairs are walked in name
/// order so the first refusal is the same one on every run.
pub fn overlap_refusal(p: &Part) -> Result<(), String> {
    let mut names: Vec<&String> = p.features.keys().collect();
    names.sort();
    for a in &names {
        for b in &names {
            if a == b {
                continue;
            }
            let (fa, fb) = (&p.features[*a], &p.features[*b]);
            match judge_standing(p, a, fa, b, fb) {
                Standing::Refused(why) => return Err(why),
                _ => {}
            }
            if let Some(why) = judge_void(p, a, fa, b, fb) {
                return Err(why);
            }
        }
    }
    // Fork 6: an island's roughing IS its host's Clear. A moat aimed at the
    // island cuts two diameters outward with no idea a wall is there.
    //
    // …when the host is a POCKET (narrowed in Phase 28 slice 4). A boss standing
    // on a `Face` or through a `Plane` is an island of it too — an End goes
    // round it — but nothing obliges a program to FACE that floor, and where it
    // does not, the moat is how the boss is roughed. The sidecar made the case:
    // the moment it could propose a ledge as a `Face`, a boss standing on the
    // ledge became "an island", and an ordinary imported program — rough the
    // boss, finish the boss — refused for a face nobody had asked to cut.
    for op in &p.operations {
        if op.op_type == OpType::Clear {
            if let Some(host) = host_of(p, &op.feature_name).filter(|h| matches!(p.features.get(*h), Some(Feature::ProfilePocket { .. }))) {
                return Err(format!(
                    "'{}' is an island of '{host}' — that feature's own clearing op roughs around it; schedule the Clear on '{host}'",
                    op.feature_name
                ));
            }
        }
    }
    Ok(())
}

/// The ops this model has no closed-form removal for — the reason a prefix
/// reads incomplete, named per op for consumers that must decline with a why
/// (the rest-material report). Derived from the same `removals_for` that
/// `remaining_after` consults, so the list and the flag cannot drift apart.
/// The sentence a decline states for an op the model has no removal for —
/// one home for the two voices that print it (`untrusted_for`, the rest
/// report). Every feature kind reads "has no closed-form removal"; the two
/// camp-3 kinds say they ALWAYS will (Phase 10, fork 6A, ruled): a drop-cutter
/// over a mesh or a patch machines the upper envelope, so the swept field IS
/// the removal and no analytic section of it exists to write down. A decline
/// that reads as owed invites a phase to pay it; these two are paid.
pub fn incomplete_reason(p: &Part, op_idx: usize, feature: &str) -> String {
    let permanent = matches!(
        p.features.get(feature),
        Some(Feature::Mesh { .. }) | Some(Feature::NurbsPatch { .. })
    );
    // A rim wire's band is statable; the program that finishes a rim leads
    // into the rim's INTERIOR, which no declaration ever opens — a
    // non-planar-floored pocket the vocabulary cannot say (Phase 10 lane A,
    // slice 2, named to T5). Modelling the band would reject every such
    // program at its lead-in, so the decline stands, and says what pays it.
    if matches!(p.features.get(feature), Some(Feature::ContourPocket { .. })) {
        return format!(
            "op [{}] on '{}' has no closed-form removal — a rim wire's interior is undeclared, and until the vocabulary can say a non-planar-floored pocket to rough it (T5), a program that finishes the rim alone leads into standing stock; declined by name",
            op_idx, feature
        );
    }
    if permanent {
        format!(
            "op [{}] on '{}' has no closed-form removal, and never will — a drop-cutter over a mesh or patch sweeps the upper envelope, so the swept field IS its removal (permanent, by the Phase-10 ruling)",
            op_idx, feature
        )
    } else {
        format!("op [{}] on '{}' has no closed-form removal", op_idx, feature)
    }
}

pub fn incomplete_ops(p: &Part) -> Vec<(usize, String)> {
    p.operations
        .iter()
        .enumerate()
        .filter(|(_, op)| removals_for(p, op).is_none())
        .map(|(k, op)| (k, op.feature_name.clone()))
        .collect()
}

/// The stock as it stands once operations `0..=op_idx` have run.
pub fn remaining_after(p: &Part, op_idx: usize) -> RemainingStock {
    remaining_over(p, op_idx + 1)
}

/// The stock as it stands BEFORE operation `op_idx` runs — virgin for the
/// first op. This is the model the plunge taxonomy's category split reads
/// (DECISIONS.md "the plunge taxonomy"): void here is a licensed entry;
/// standing here but removed by the op's own declaration is the flagged one.
pub fn remaining_before(p: &Part, op_idx: usize) -> RemainingStock {
    remaining_over(p, op_idx)
}

fn remaining_over(p: &Part, n_ops: usize) -> RemainingStock {
    // What is GIVEN is air before the first op (Phase 42 slice 1): subtracted
    // at prefix 0, so the plunge taxonomy, `check_moves`, the immersion
    // field and the rest report all read it from this one home.
    let mut removals = given_removals(p);
    // The earlier program's removals that could not be turned (Phase 48
    // slice 1) are an over-statement of material at prefix 0 — the model is
    // incomplete, and every reader says so through `complete`, exactly as
    // for an op of this program with no closed form.
    let mut complete = given_from_declines(p).is_empty();
    for op in p.operations.iter().take(n_ops) {
        match removals_for(p, op) {
            Some(mut rs) => removals.append(&mut rs),
            None => complete = false,
        }
    }
    RemainingStock { stock: p.stock.clone(), removals, complete }
}

/// How far, in plan, a given tilted bore's section may drift across one slab
/// of its stack (Phase 42 slice 1): ten chord tolerances. A slab's section
/// is the LENS of its two end sections, so this is the width of the crescent
/// each slab leaves unclaimed — air called material, the direction that can
/// only under-claim. The fixture's bore (14 deep at 30°) is 81 slabs.
pub(crate) const GIVEN_SLAB_SHIFT_MM: f64 = 10.0 * crate::CHORD_TOL_MM;

/// What a GIVEN void takes out of the stock before the program starts
/// (Phase 42 slice 1; `DECISIONS.md` → "Phase 42 slice 1…"): its own
/// occupancy, as the removal the op that cut it on the other setup would
/// have declared. `None` for a kind that cannot be given — what stands, and
/// what this model has no closed form for — which is `validate`'s refusal,
/// read from here so the two cannot disagree.
///
/// The tilted bore is the one void that is not a column. It is cut as a
/// STACK of horizontal slabs, each slab's section the lens of the bore's
/// sections at its two faces (air at both, so air between, by convexity),
/// the slabs thin enough that the crescent each leaves unclaimed is a tenth
/// of a millimetre wide. A short approximation of a sheared cylinder in the
/// direction that can only under-claim — `check_moves` never blesses a move
/// into the tube wall over the bore, and a tool standing in the lens a body
/// can stand in (slice 2) is inside every slab's section by construction.
pub fn given_removal(p: &Part, f: &Feature) -> Option<Vec<Removal>> {
    let top = p.stock.z;
    match f {
        Feature::ProfilePocket { .. } => {
            let o = occupancy(p, f)?;
            Some(vec![Removal { section: Section::Polygon { points: o.outline, holes: Vec::new() }, z_top: o.hi, floor: Floor::Flat(o.lo), ceiling: None }])
        }
        Feature::CylinderBore { dia, extent, x, y, z, .. } => {
            let rim = derive_contour_top(top, *z);
            Some(vec![Removal {
                section: Section::Disc { x: *x, y: *y, r: dia / 2.0 },
                z_top: rim,
                floor: Floor::Flat(rim - extent),
                ceiling: None,
            }])
        }
        Feature::TiltedBore { dia, extent, x, y, tilt, heading } => {
            let shift = extent * tilt.to_radians().ptan();
            let slabs = (shift / GIVEN_SLAB_SHIFT_MM).ceil().max(1.0) as usize;
            let h = extent / slabs as f64;
            Some((0..slabs)
                .map(|k| {
                    let (d0, d1) = (k as f64 * h, (k + 1) as f64 * h);
                    let lens = crate::parse::derive_tilted_bore_lens(*dia, *x, *y, *tilt, *heading, d0, d1);
                    Removal { section: Section::Polygon { points: lens, holes: Vec::new() }, z_top: top - d0, floor: Floor::Flat(top - d1), ceiling: None }
                })
                .collect())
        }
        _ => None,
    }
}

/// Every given void's removal, in declaration order — what `remaining_over`
/// subtracts before the first op. A name `validate` would have refused (no
/// such feature, a kind that cannot be given) contributes nothing here.
pub fn given_removals(p: &Part) -> Vec<Removal> {
    let mut out: Vec<Removal> =
        p.given.iter().filter_map(|n| p.features.get(n)).filter_map(|f| given_removal(p, f)).flatten().collect();
    if let Some(air) = given_from_air(p) {
        out.extend(air.removals.iter().cloned());
    }
    out
}

// ---- given from: the program before this one, turned over (Phase 48 slice 1;
// DECISIONS.md → "Phase 48 slice 1…", fork 1A of the DRAFT)

/// A point turned as `up=` says: `z+` leaves it, `z-` is the part turned
/// over — 180° about the X axis through the ORIGIN, (x, y, z) → (x, −y, −z).
/// The sidecar's `UP_ROTATIONS["z-"]`, twinned (`parse::UpAxis`).
fn turn_pt(q: Pt, up: UpAxis) -> Pt {
    match up {
        UpAxis::ZPlus => q,
        UpAxis::ZMinus => Pt { x: q.x, y: -q.y },
    }
}

fn turn_z(z: f64, up: UpAxis) -> f64 {
    match up {
        UpAxis::ZPlus => z,
        UpAxis::ZMinus => -z,
    }
}

/// The earlier program's stock in THIS program's frame. A box or a round is
/// a box or a round turned over: the same w, l, h; the datum corner moves to
/// where the turn puts the opposite corner, and the top is the old bottom.
pub fn turn_stock(s: &Stock, up: UpAxis) -> Stock {
    match up {
        UpAxis::ZPlus => s.clone(),
        // `0.0 - v`, not `-v`: a corner at the origin turns to +0, so the
        // refusal that prints both stocks never shows a `-0`.
        UpAxis::ZMinus => Stock { y: 0.0 - (s.y + s.l), z: s.h - s.z, ..s.clone() },
    }
}

/// A stock moved by `at`, the translation `given from … at=` says: the
/// earlier program's blank, turned, where this program's origin sees it.
pub fn shift_stock(s: &Stock, at: [f64; 3]) -> Stock {
    let shape = match &s.shape {
        StockShape::Profile { points } => StockShape::Profile { points: points.iter().map(|q| Pt { x: q.x + at[0], y: q.y + at[1] }).collect() },
        other => other.clone(),
    };
    // `+ 0.0`: a corner that lands on the origin prints as 0, never -0.
    Stock { shape, x: s.x + at[0] + 0.0, y: s.y + at[1] + 0.0, z: s.z + at[2] + 0.0, ..s.clone() }
}

/// ` at=x,y,z` as the `given from` line spells it, or nothing for the two
/// setups sharing one origin.
pub fn at_word(at: [f64; 3]) -> String {
    if at == [0.0; 3] {
        return String::new();
    }
    format!(" at={},{},{}", at[0], at[1], at[2])
}

/// A removal moved by `at`: every plan point by its x and y, every height
/// by its z. The tables' keys (a distance from a centre or a path) do not
/// move.
fn shift_removal(r: &Removal, at: [f64; 3]) -> Removal {
    if at == [0.0; 3] {
        return r.clone();
    }
    let pt = |q: &Pt| Pt { x: q.x + at[0], y: q.y + at[1] };
    let pts = |v: &Vec<Pt>| v.iter().map(pt).collect::<Vec<Pt>>();
    let rise = |v: &Vec<(f64, f64)>| v.iter().map(|&(k, z)| (k, z + at[2])).collect::<Vec<(f64, f64)>>();
    let floor = |f: &Floor| match f {
        Floor::Flat(z) => Floor::Flat(z + at[2]),
        Floor::Dome { x, y, z, r } => Floor::Dome { x: x + at[0], y: y + at[1], z: z + at[2], r: *r },
        Floor::Meridian { x, y, samples } => Floor::Meridian { x: x + at[0], y: y + at[1], samples: rise(samples) },
        Floor::PathForm { path, samples } => Floor::PathForm { path: pts(path), samples: rise(samples) },
        Floor::AlongWire { wire, drop } => {
            Floor::AlongWire { wire: wire.iter().map(|w| crate::parse::Pt3 { x: w.x + at[0], y: w.y + at[1], z: w.z + at[2] }).collect(), drop: *drop }
        }
    };
    let section = match &r.section {
        Section::All => Section::All,
        Section::Disc { x, y, r } => Section::Disc { x: x + at[0], y: y + at[1], r: *r },
        Section::Annulus { x, y, r_inner, r_outer } => Section::Annulus { x: x + at[0], y: y + at[1], r_inner: *r_inner, r_outer: *r_outer },
        Section::Ellipse { x, y, a, b, rot } => Section::Ellipse { x: x + at[0], y: y + at[1], a: *a, b: *b, rot: *rot },
        Section::Polygon { points, holes } => Section::Polygon { points: pts(points), holes: holes.iter().map(pts).collect() },
        Section::PathBand { path, material_inside, d_inner, d_outer } => {
            Section::PathBand { path: pts(path), material_inside: *material_inside, d_inner: *d_inner, d_outer: *d_outer }
        }
        Section::ChainBand { outline, chain, material_inside, d_inner, d_outer } => {
            Section::ChainBand { outline: pts(outline), chain: pts(chain), material_inside: *material_inside, d_inner: *d_inner, d_outer: *d_outer }
        }
    };
    Removal { section, z_top: r.z_top + at[2], floor: floor(&r.floor), ceiling: r.ceiling.as_ref().map(floor) }
}

/// Whether two stocks are the same blank in the same place — shape and the
/// six numbers, to a micron. `material` is a feeds-table name and says
/// nothing about where the steel is, so it is not compared.
pub fn stocks_coincide(a: &Stock, b: &Stock) -> bool {
    a.shape == b.shape
        && [(a.w, b.w), (a.l, b.l), (a.h, b.h), (a.x, b.x), (a.y, b.y), (a.z, b.z)]
            .iter()
            .all(|(u, v)| (u - v).abs() < EPS)
}

/// A stock as the refusal prints it: the statement an author would write.
pub fn stock_words_row(s: &Stock) -> String {
    match s.shape {
        StockShape::Box => format!("Box(w={}, l={}, h={}, x={}, y={}, z={})", s.w, s.l, s.h, s.x, s.y, s.z),
        // The line says the AXIS; the struct keeps the bounding box (the
        // parser's own note on `StockShape`), so the row is read back
        // through `axis()` — never the corner the first draft printed.
        StockShape::Cylinder { dia } => {
            let (cx, cy) = s.axis();
            format!("Cylinder(dia={}, h={}, x={}, y={}, z={})", dia, s.h, cx + 0.0, cy + 0.0, s.z)
        }
        StockShape::Profile { ref points } => format!("Profile(h={}, z={}) over {} points", s.h, s.z, points.len()),
    }
}

/// One removal of the earlier program in this program's frame, or why it
/// cannot be said here. A section is a plan shape and turns in plan (a
/// mirror in y for `z-`: a disc stays a disc, a polygon's points move and
/// its winding flips, which no reader of a section minds). The two
/// surfaces swap roles: what the removal stood on it now hangs from. A
/// flat floor and a flat top are a flat top and a flat floor; a SHAPED
/// floor — the R2 cavities' meridian, a form's `(d, z)` table — is the
/// turned removal's `ceiling`, the same table over the same (turned) path
/// with its z negated, which `Removal` has held since the rim-area thread
/// and `Floor::z_at_slack_max` already probes (Phase 48 slice 5, ruling
/// 1 of the CLOSE; slices 1–4 declined every shaped floor by name, and
/// the real part's rib rounders are exactly this). A shaped
/// `ceiling` turns into a shaped floor the same way. What is declined BY
/// NAME, never flattened (a flat top over a bowl would claim its rim
/// material as air, the direction `check_moves` must never be handed): a
/// `Dome`, whose underside is not an upper half-sphere, and an
/// `AlongWire`, whose table keys a DROP below a wire that turned over is
/// a rise.
fn turn_removal(r: &Removal, up: UpAxis) -> Result<Removal, String> {
    if up == UpAxis::ZPlus {
        return Ok(r.clone());
    }
    let ceiling = match &r.floor {
        Floor::Flat(_) => None,
        f => Some(turn_floor(f, up).map_err(|why| format!("{} floor turned over is {why}", kind_with_article(f)))?),
    };
    let floor = match &r.ceiling {
        None => Floor::Flat(turn_z(r.z_top, up)),
        Some(c) => turn_floor(c, up).map_err(|why| format!("{} ceiling turned over is {why}", kind_with_article(c)))?,
    };
    // The turned removal's flat cap is where its ceiling peaks: the old
    // floor's lowest point, negated. For a flat floor that is the floor.
    let (lo, _) = r.z_span();
    Ok(Removal { section: turn_section(&r.section, up), z_top: turn_z(lo, up), floor, ceiling })
}

/// A surface turned over, in this frame: a flat plane's height negated; a
/// meridian's centre turned in plan and every sample's z negated over the
/// same rho; a form table's path turned and every sample's z negated over
/// the same d — the surface's shape is a function of a plan distance that
/// the turn preserves. `Err` says what the turned surface would be and why
/// the model holds no form for it.
fn turn_floor(f: &Floor, up: UpAxis) -> Result<Floor, String> {
    match f {
        Floor::Flat(z) => Ok(Floor::Flat(turn_z(*z, up))),
        Floor::Meridian { x, y, samples } => {
            let c = turn_pt(Pt { x: *x, y: *y }, up);
            Ok(Floor::Meridian { x: c.x, y: c.y, samples: samples.iter().map(|&(rho, z)| (rho, turn_z(z, up))).collect() })
        }
        Floor::PathForm { path, samples } => Ok(Floor::PathForm {
            path: path.iter().map(|q| turn_pt(*q, up)).collect(),
            samples: samples.iter().map(|&(d, z)| (d, turn_z(z, up))).collect(),
        }),
        Floor::Dome { .. } => Err("the underside of a sphere, which `Dome` (an upper half) holds no form for".to_string()),
        Floor::AlongWire { .. } => {
            Err("a rise above a wire, which the table keys as a drop below one — the model holds no form for it".to_string())
        }
    }
}

fn turn_section(s: &Section, up: UpAxis) -> Section {
    match s {
        Section::All => Section::All,
        Section::Disc { x, y, r } => Section::Disc { x: *x, y: -y, r: *r },
        Section::Annulus { x, y, r_inner, r_outer } => Section::Annulus { x: *x, y: -y, r_inner: *r_inner, r_outer: *r_outer },
        Section::Ellipse { x, y, a, b, rot } => Section::Ellipse { x: *x, y: -y, a: *a, b: *b, rot: -rot },
        Section::Polygon { points, holes } => Section::Polygon {
            points: points.iter().map(|q| turn_pt(*q, up)).collect(),
            holes: holes.iter().map(|h| h.iter().map(|q| turn_pt(*q, up)).collect()).collect(),
        },
        Section::PathBand { path, material_inside, d_inner, d_outer } => Section::PathBand {
            path: path.iter().map(|q| turn_pt(*q, up)).collect(),
            material_inside: *material_inside,
            d_inner: *d_inner,
            d_outer: *d_outer,
        },
        Section::ChainBand { outline, chain, material_inside, d_inner, d_outer } => Section::ChainBand {
            outline: outline.iter().map(|q| turn_pt(*q, up)).collect(),
            chain: chain.iter().map(|q| turn_pt(*q, up)).collect(),
            material_inside: *material_inside,
            d_inner: *d_inner,
            d_outer: *d_outer,
        },
    }
}

/// The kind with its article, for a sentence: "a Dome", "an AlongWire".
fn kind_with_article(f: &Floor) -> String {
    let kind = floor_kind(f);
    let article = if kind.starts_with(['A', 'E', 'I', 'O', 'U']) { "an" } else { "a" };
    format!("{article} {kind}")
}

pub(crate) fn floor_kind(f: &Floor) -> &'static str {
    match f {
        Floor::Flat(_) => "flat",
        Floor::Dome { .. } => "Dome",
        Floor::Meridian { .. } => "Meridian",
        Floor::PathForm { .. } => "PathForm",
        Floor::AlongWire { .. } => "AlongWire",
    }
}

/// What the program before this one left as air, in this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct GivenFromAir {
    /// The earlier program's closed-form removals, turned — what
    /// `given_removals` adds at prefix 0.
    pub removals: Vec<Removal>,
    /// How many of its ops contributed at least one removal.
    pub ops_read: usize,
    /// One sentence per removal the model could not carry, naming the
    /// earlier op. Non-empty makes the model INCOMPLETE.
    pub declined: Vec<String>,
}

/// The earlier program's air (Phase 48 slice 1): every removal its OWN ops
/// declare, turned into this frame — `removals_for` over its `run` order,
/// the same home every reader of a program's removals uses. Not its
/// `given`s: a void the earlier program was told about was cut on another
/// setup, which may be this one, and air claimed here for a hole this
/// program is about to cut would bless a plunge into steel. Not its
/// `given from` either: a chain refuses at parse.
///
/// Derived ONCE, when `given from` loads, and held in `parse::GivenFrom`
/// beside the earlier `Part` it is derived from (slice 6, ruling 2 of
/// the CLOSE): a stored derived value, the thing this project asks
/// before doing — asked, and ruled. It is not the hazard the DRY rule
/// names, a copy that drifts from its source: the earlier `Part` is
/// frozen in the same struct at the same moment, nothing in this program
/// can change either, and the two are born and die together. Slices 1–5
/// derived it on every ask of the stock model, and a program of
/// fifty-seven ops asked it so often that its second setup did not
/// generate in a sitting (the CLOSE's finding 2).
pub fn turn_program(earlier: &Part, up: UpAxis, at: [f64; 3], project: &str) -> GivenFromAir {
    let mut removals = Vec::new();
    let mut declined = Vec::new();
    let mut ops_read = 0;
    for (k, op) in earlier.operations.iter().enumerate() {
        let Some(rs) = removals_for(earlier, op) else {
            declined.push(format!(
                "op [{}] on '{}' of {} — {}",
                k,
                op.feature_name,
                project,
                incomplete_reason(earlier, k, &op.feature_name)
            ));
            continue;
        };
        let mut read_one = false;
        for r in &rs {
            match turn_removal(r, up) {
                Ok(t) => {
                    removals.push(shift_removal(&t, at));
                    read_one = true;
                }
                Err(why) => declined.push(format!("op [{}] on '{}' of {} — {}", k, op.feature_name, project, why)),
            }
        }
        if read_one {
            ops_read += 1;
        }
    }
    GivenFromAir { removals, ops_read, declined }
}

/// What the program before this one left as air, as `given from` holds
/// it — `turn_program`'s answer, read and never re-derived. `None` when
/// this program says no `given from`.
pub fn given_from_air(p: &Part) -> Option<&GivenFromAir> {
    p.given_from.as_ref().map(|gf| &gf.air)
}

/// The sentences for what `given from` could not carry — the one list
/// `remaining_over`'s `complete`, `untrusted_for`'s reason and the rest
/// report's "declared model" line all read. Empty without a `given from`.
pub fn given_from_declines(p: &Part) -> Vec<String> {
    given_from_air(p).map_or_else(Vec::new, |a| a.declined.clone())
}

// ---- the certificate (Phase 48 slice 2; DECISIONS.md → "Phase 48 slice 2…",
// fork 1A (iii) of the DRAFT): a `given <feature>` restated beside a `given
// from` must lie in the earlier program's air.

/// How far under the top and above the floor a sample is taken: inside the
/// void by a micron's thousand, so a column that reaches exactly this face
/// certifies and one that stops a hair short does not.
const CERT_DZ_MM: f64 = 1e-3;
/// The step of the descent that finds where the earlier air begins under a
/// standing sample — a twentieth of a millimetre, said in the sentence.
const CERT_PROBE_MM: f64 = 0.05;

/// A sample of a given void the earlier program's air does not hold.
#[derive(Clone, Debug, PartialEq)]
pub struct StandingSample {
    /// The sample, in this program's frame.
    pub here: [f64; 3],
    /// The same point in the earlier program's frame.
    pub there: [f64; 3],
    /// Under the sample's plan point, how far below this face the steel
    /// reaches before the earlier program's air begins — `None` when it
    /// stands through the whole of the void's depth.
    pub steel_to: Option<f64>,
    /// The void's depth under this face, for the sentence.
    pub depth: f64,
}

/// The certificate's answer for one given void.
#[derive(Clone, Debug, PartialEq)]
pub enum Certificate {
    /// Every sample is air after the earlier program: how many were asked.
    Certified { samples: usize },
    /// The first sample that stands.
    Standing(StandingSample),
    /// The earlier model is incomplete (a removal it could not carry), so
    /// the certificate did not read it — never a pass.
    Declined,
}

/// A point turned into the earlier program's frame: `at` taken off, then
/// the turn, which is its own inverse (none, or a half turn), so this is
/// `turn_pt` and `turn_z` again.
fn turn_point_back(q: [f64; 3], up: UpAxis, at: [f64; 3]) -> [f64; 3] {
    let t = turn_pt(Pt { x: q[0] - at[0], y: q[1] - at[1] }, up);
    [t.x, t.y, turn_z(q[2] - at[2], up)]
}

/// The samples the certificate asks of one given void: for every removal
/// the void subtracts, each rim vertex (the section's chorded rims, the
/// same points the containment certificate reads) at the top, the middle
/// and the floor of the removal's column, and each rim's centroid at the
/// floor — a proof by certificate over sampled points, which can only
/// under-claim: a void the earlier program cut narrower BETWEEN the rims
/// would pass, and a void it cut shorter or elsewhere does not.
fn certificate_samples(p: &Part, f: &Feature) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for r in given_removal(p, f).unwrap_or_default() {
        let Floor::Flat(floor) = r.floor else { continue };
        let (top, bottom) = (r.z_top - CERT_DZ_MM, floor + CERT_DZ_MM);
        let mid = (top + bottom) / 2.0;
        for lp in r.section.outlines(&p.stock) {
            if lp.is_empty() {
                continue;
            }
            for z in [top, mid, bottom] {
                out.extend(lp.iter().map(|q| [q.x, q.y, z]));
            }
            let n = lp.len() as f64;
            let (cx, cy) = lp.iter().fold((0.0, 0.0), |a, q| (a.0 + q.x / n, a.1 + q.y / n));
            out.push([cx, cy, bottom]);
        }
    }
    out
}

/// Certify one given void against the earlier program's air (Phase 48
/// slice 2). `None` when this program says no `given from` — the restated
/// void is then exactly Phase 42's, unread, as the DRAFT names it. The
/// earlier air is this stock less the turned removals — `given_from_air`'s
/// own list, so the certificate and the subtraction cannot disagree — and
/// the given's own removal plays no part in it (it is the thing being
/// certified). A standing sample names itself in both frames and says how
/// far the steel reaches under it: the descent from this face in
/// `CERT_PROBE_MM` steps to the first height the earlier air holds.
pub fn certify_given(p: &Part, name: &str) -> Option<Certificate> {
    let gf = p.given_from.as_ref()?;
    let air = given_from_air(p)?;
    if !air.declined.is_empty() {
        return Some(Certificate::Declined);
    }
    let Some(f) = p.features.get(name) else { return None };
    let earlier = RemainingStock { stock: p.stock.clone(), removals: air.removals.clone(), complete: true };
    let samples = certificate_samples(p, f);
    for s in &samples {
        if !earlier.material_at(s[0], s[1], s[2]) {
            continue;
        }
        let top = p.stock.z;
        let floor = given_removal(p, f).unwrap_or_default().iter().map(|r| r.z_span().0).fold(top, f64::min);
        let depth = top - floor;
        // The descent: steps of `CERT_PROBE_MM` to the first height the
        // earlier air holds, then a bisection between that and the last
        // standing height to a tenth of a micron, so the sentence says
        // 1.000 where the earlier column ends a millimetre short and not
        // the step it happened to land on.
        let mut steel_to = None;
        let mut stands_at = top - CERT_DZ_MM;
        let mut z = stands_at;
        while z > floor {
            if !earlier.material_at(s[0], s[1], z) {
                let (mut hi, mut lo) = (stands_at, z);
                while hi - lo > 1e-4 {
                    let m = (hi + lo) / 2.0;
                    if earlier.material_at(s[0], s[1], m) {
                        hi = m;
                    } else {
                        lo = m;
                    }
                }
                steel_to = Some(top - lo);
                break;
            }
            stands_at = z;
            z -= CERT_PROBE_MM;
        }
        return Some(Certificate::Standing(StandingSample { here: *s, there: turn_point_back(*s, gf.up, gf.at), steel_to, depth }));
    }
    Some(Certificate::Certified { samples: samples.len() })
}

/// The sentence a standing sample refuses with — `validate`'s, and the
/// test's pin.
pub fn standing_sentence(p: &Part, name: &str, s: &StandingSample) -> String {
    let gf = p.given_from.as_ref().expect("a standing sample exists only under a given from");
    let steel = match s.steel_to {
        Some(d) => format!(
            "steel from this face to {d:.3} below it, where the earlier program's air begins (probed every {CERT_PROBE_MM} mm)"
        ),
        None => format!("steel through the whole {:.3} of the void — the earlier program cut nothing there", s.depth),
    };
    format!(
        "given '{name}' is not air after {} turned {}: ({:.3}, {:.3}, {:.3}) here — ({:.3}, {:.3}, {:.3}) in its frame — stands; {steel}. Take it through there, or give less here",
        gf.project,
        gf.up.word(),
        s.here[0],
        s.here[1],
        s.here[2],
        s.there[0],
        s.there[1],
        s.there[2],
    )
}

/// The certificate's advisory voices (Phase 48 slice 2), for `parse::lint`:
/// a DECLINE per given void when the earlier model is incomplete — the
/// certificate did not read it, which is never a pass — and the REVERSE:
/// an earlier removal that reaches this face which no given names. The
/// reverse is advisory because the restriction already reads that air from
/// the model (a program that names nothing is exactly as safe as before);
/// what it loses is a noun — no op of this program can dress what it does
/// not name. Voiced under `Containment`, the declaration against the stock:
/// here the stock as it ARRIVED, which `given from` makes part of the
/// declaration; a family of its own would cost a theme slot for one
/// sentence.
pub fn given_from_findings(p: &Part) -> Vec<crate::parse::Finding> {
    use crate::parse::{Finding, FindingFamily};
    let mut out = Vec::new();
    let (Some(gf), Some(air)) = (&p.given_from, given_from_air(p)) else { return out };
    if !air.declined.is_empty() {
        for name in &p.given {
            out.push(Finding::feature(
                FindingFamily::Decline,
                name,
                format!(
                    "given '{name}' not certified against {} — the earlier program's model is incomplete ({}), so the certificate did not read it: a void restated a millimetre off would pass unseen here. Fix the named op there, or restate with care",
                    gf.project,
                    air.declined.join("; ")
                ),
            ));
        }
        return out;
    }
    // The reverse: a turned removal whose top is this face, judged at its
    // rims' centroid a hair under the face, against what this program's
    // own givens subtract.
    let named = RemainingStock { stock: p.stock.clone(), removals: p.given.iter().filter_map(|n| p.features.get(n)).filter_map(|f| given_removal(p, f)).flatten().collect(), complete: true };
    let top = p.stock.z;
    // Once per earlier FEATURE, at its first op that reaches: a bore the
    // earlier program cleared and then wall-finished is one void here.
    // A visit to points names no feature: each hole it drilled is a void of
    // its own, named by the point whose axis its removal stands on.
    let mut said: Vec<String> = Vec::new();
    for (k, op) in gf.part.operations.iter().enumerate() {
        let holes = if op.at_points() { subjects(&gf.part, op) } else { Vec::new() };
        for r in removals_for(&*gf.part, op).unwrap_or_default() {
            let name = if op.at_points() {
                let Some(lp) = r.section.outlines(&gf.part.stock).into_iter().find(|lp| !lp.is_empty()) else { continue };
                let n = lp.len() as f64;
                let (x, y) = lp.iter().fold((0.0, 0.0), |a, q| (a.0 + q.x / n, a.1 + q.y / n));
                let near = |s: &&Subject| s.at.is_some_and(|at| (at.x - x).abs() < 1e-6 && (at.y - y).abs() < 1e-6);
                let Some(hole) = holes.iter().find(near) else { continue };
                hole.name.as_str()
            } else {
                op.feature_name.as_str()
            };
            if said.iter().any(|s| s == name) {
                continue;
            }
            let Ok(t) = turn_removal(&r, gf.up).map(|t| shift_removal(&t, gf.at)) else { continue };
            if t.z_top < top - EPS || matches!(t.section, Section::All) {
                continue;
            }
            for lp in t.section.outlines(&p.stock) {
                if lp.is_empty() {
                    continue;
                }
                let n = lp.len() as f64;
                let (cx, cy) = lp.iter().fold((0.0, 0.0), |a, q| (a.0 + q.x / n, a.1 + q.y / n));
                let z = top - CERT_DZ_MM;
                // A rim's centroid can lie in steel (an annulus, a band): only
                // a point the turned removal itself holds is asked.
                if !t.removes(cx, cy, z, EPS) || !named.material_at(cx, cy, z) {
                    continue;
                }
                let feature = gf.part.features.get(name).map_or("?", |f| f.kind());
                let word = if op.at_points() { "at" } else { "on" };
                out.push(
                    Finding::feature(
                        FindingFamily::Containment,
                        name,
                        format!(
                            "{}'s op [{}] {} '{}' ({}) reaches this face at ({:.3}, {:.3}) and no `given` here names it — the stock model reads its air all the same, but no op of this program can dress a void it has no noun for; restate it in this frame and give it, or leave it",
                            gf.project, k, word, name, feature, cx, cy
                        ),
                    )
                    .with_place([cx, cy, top]),
                );
                said.push(name.to_string());
                break;
            }
        }
    }
    out
}

/// The air a tool BODY can stand in — one given void, one op's floor (Phase
/// 42 slice 2; `DECISIONS.md` → "Phase 42 slice 2…", fork 2 of the DRAFT).
/// The envelope is the union of a tilted bore's sections over its depth, the
/// right curve for a keep-out and the wrong one for a region the tool may
/// occupy: a tool spanning the floor's depth is in air at a plan point only
/// if that point is inside the void at EVERY height the body occupies. The
/// lens is that intersection, eroded by the body's radius at each height.
#[derive(Clone, Debug, PartialEq)]
pub struct Lens {
    /// The given void this is the lens of.
    pub void: String,
    /// Where the tool's CENTRE may stand, as CCW pieces: inside the void's
    /// section at every height the body occupies in the stock, each height's
    /// section eroded by the body's radius there. Empty with `declined` set.
    pub centre: Vec<Vec<Pt>>,
    /// The floor the op may occupy with its centre in `centre`: the centre
    /// pieces with the cutting radius put back on.
    pub floor: Vec<Vec<Pt>>,
    /// The body as judged, from the tip up to the stock's top: `(from, to,
    /// radius)` in height above the tip — K2's declared pieces, each
    /// constant over its stretch. One stretch at the cutting radius when no
    /// body is declared: the rod the stamp already is, K2's own law.
    pub stretches: Vec<(f64, f64, f64)>,
    /// Why `centre` is empty, when it is.
    pub declined: Option<String>,
}

impl Lens {
    /// The body in a few words — each stretch's radius and its heights.
    pub fn body_words(&self) -> String {
        let f = crate::gcode::f;
        self.stretches.iter().map(|(a, b, r)| format!("r {} from {} to {}", f(*r), f(*a), f(*b))).collect::<Vec<_>>().join(", ")
    }

    /// The outline's one line for this lens.
    pub fn line(&self) -> String {
        let f = crate::gcode::f;
        match &self.declined {
            Some(why) => format!("given air '{}': none — {}", self.void, why),
            None => {
                let area = |pieces: &[Vec<Pt>]| pieces.iter().map(|l| crate::parse::signed_area(l).abs() / 2.0).sum::<f64>();
                format!(
                    "given air '{}': the body ({}) stands in {} piece{} — centre {}mm², floor {}mm²",
                    self.void,
                    self.body_words(),
                    self.centre.len(),
                    if self.centre.len() == 1 { "" } else { "s" },
                    f(area(&self.centre)),
                    f(area(&self.floor))
                )
            }
        }
    }
}

/// The lens of every given void for the floor `op` works, in declaration
/// order — empty for an op that works no floor (a flank finish, a visit),
/// or a program that gives nothing. Body-aware through the one silhouette
/// home: the stretches are the K2 ladder's rungs clipped to the height the
/// body occupies in the stock, each judged at its widest.
///
/// A column void (a pocket, a vertical bore) is one section at every
/// height, so its lens is that section eroded by the body's widest radius —
/// when the void spans the whole height, floor to stock top; a given pocket
/// whose top is below the stock's leaves the body in material above it, and
/// says so. A tilted bore's lens is the intersection over the stretches of
/// `derive_tilted_bore_lens` at each stretch's two depths eroded by its
/// radius — erosion commutes with intersection, so the stretch's lens eroded
/// IS the intersection of its eroded sections — through the offsetter and
/// the crate's `And`, nothing sampled. A floor below the bore's bottom has
/// material under the tip and no lens at all.
pub fn given_air(p: &Part, op: &Operation) -> Vec<Lens> {
    let Some(region) = crate::region::of(p, op) else { return Vec::new() };
    given_air_at(p, op, region.z)
}

/// `given_air` for a caller that already knows the floor's height —
/// `region::of` itself (slice 3), which cannot ask for the region it is
/// building. The floor the lens reports is clipped to the void's sections
/// over the body's WHOLE height (their lens, `L`), not only the centre set
/// put back out by the cutting radius: a body stretch THINNER than the
/// flutes (a relieved shank) constrains the centre set less, so `C ⊕ r`
/// alone could reach a fringe outside a section the shank stands at — and
/// a floor the region grows into (slice 3) must hold every body point
/// thinner than the flutes in air, which `C ⊕ r ∩ L` does. For a rod the
/// clip changes nothing: `C ⊕ r ⊆ L` already.
pub fn given_air_at(p: &Part, op: &Operation, z: f64) -> Vec<Lens> {
    let f = crate::gcode::f;
    let Some(tool) = p.tools.get(&op.tool_num) else { return Vec::new() };
    let top = p.stock.z;
    let reach = top - z;
    if reach <= 0.0 {
        return Vec::new();
    }
    let sil = Silhouette::of(tool);
    let r_cut = tool.dia / 2.0;
    // The body's stretches in the stock: the ladder's rungs between the tip
    // and the top, each stretch answered at its widest (a drawn profile is
    // linear between its vertices, so its ends are its extremes).
    let mut rungs: Vec<f64> = sil.rung_heights().into_iter().filter(|h| *h > 1e-9 && *h < reach - 1e-9).collect();
    rungs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    rungs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let mut bounds = vec![0.0];
    bounds.extend(rungs);
    bounds.push(reach);
    let stretches: Vec<(f64, f64, f64)> = bounds
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let rad = sil.radius_at(a + 1e-9).max(sil.radius_at(b - 1e-9)).max(sil.radius_at((a + b) / 2.0));
            (a, b, rad)
        })
        .collect();
    let widest = stretches.iter().map(|s| s.2).fold(0.0, f64::max);
    let erode = |lp: &[Pt], by: f64| -> Vec<Vec<Pt>> {
        let ccw = crate::ops::clear::make_ccw(lp);
        // The crate's sign is inward-positive on a CCW loop.
        crate::offset::offset_closed_polyline(&ccw, by).into_iter().filter(|l| l.len() >= 3).collect()
    };
    let intersect = |a: &[Vec<Pt>], b: &[Vec<Pt>]| -> Vec<Vec<Pt>> {
        a.iter().flat_map(|x| b.iter().flat_map(move |y| crate::offset::clip_to(x, y))).collect()
    };
    let mut out = Vec::new();
    for name in &p.given {
        let Some(feat) = p.features.get(name) else { continue };
        let mut lens = Lens { void: name.clone(), centre: Vec::new(), floor: Vec::new(), stretches: stretches.clone(), declined: None };
        // The void's sections over the body's whole height, as one loop:
        // what the floor is clipped to.
        let mut whole: Vec<Pt> = Vec::new();
        let centre: Result<Vec<Vec<Pt>>, String> = match feat {
            Feature::TiltedBore { dia, extent, x, y, tilt, heading } => {
                if z < top - extent - Z_AGREE {
                    Err(format!("the floor (z {}) is below the bore's bottom (z {}): material under the tip", f(z), f(top - extent)))
                } else {
                    whole = crate::parse::derive_tilted_bore_lens(*dia, *x, *y, *tilt, *heading, 0.0, (top - z).min(*extent));
                    // The lens reads the SAME cylinder the certificate
                    // reads: the slab stack (`given_removal`) under-claims
                    // the true bore by up to one slab's drift, so every
                    // section here is eroded by that margin too — a body
                    // the lens says stands in air is one `check_moves`
                    // blesses, at every height, by construction (a convex
                    // set eroded by the drift lies inside every translate
                    // of itself by less). A column void has no drift and
                    // no margin.
                    whole = erode(&whole, GIVEN_SLAB_SHIFT_MM).into_iter().next().unwrap_or_default();
                    // Each stretch, as depths below the stock top: the
                    // stretch's bottom is the deeper face.
                    let mut acc: Option<Vec<Vec<Pt>>> = None;
                    for (a, b, rad) in &stretches {
                        let (d_hi, d_lo) = (top - (z + a), top - (z + b));
                        let slab = crate::parse::derive_tilted_bore_lens(*dia, *x, *y, *tilt, *heading, d_lo.max(0.0), d_hi.min(*extent));
                        let eroded = if slab.len() >= 3 { erode(&slab, *rad + GIVEN_SLAB_SHIFT_MM) } else { Vec::new() };
                        acc = Some(match acc {
                            None => eroded,
                            Some(so_far) => intersect(&so_far, &eroded),
                        });
                    }
                    Ok(acc.unwrap_or_default())
                }
            }
            other => match occupancy(p, other) {
                Some(o) if !o.stands => {
                    if o.hi < top - Z_AGREE {
                        Err(format!("'{name}' tops out at z {} below the stock top: material above it stands where the body is", f(o.hi)))
                    } else if o.lo > z + Z_AGREE {
                        Err(format!("the floor (z {}) is below '{name}''s bottom (z {}): material under the tip", f(z), f(o.lo)))
                    } else {
                        whole = o.outline.clone();
                        Ok(erode(&o.outline, widest))
                    }
                }
                _ => continue,
            },
        };
        match centre {
            Err(why) => lens.declined = Some(why),
            Ok(c) if c.is_empty() => {
                lens.declined = Some(format!(
                    "no plan point of '{name}' holds the body at every height from the floor (z {}) to the stock top — the widest stretch is r {}",
                    f(z),
                    f(widest)
                ))
            }
            Ok(c) => {
                lens.floor = c
                    .iter()
                    .flat_map(|piece| crate::offset::offset_closed_polyline(piece, -r_cut))
                    .filter(|l| l.len() >= 3)
                    .flat_map(|grown| crate::offset::clip_to(&grown, &whole))
                    .collect();
                lens.centre = c;
            }
        }
        out.push(lens);
    }
    out
}

/// What an op declares away, or `None` when this slice has no closed form for
/// it. `None` is a first-class answer, not a failure: it is what keeps
/// `complete` honest.
///
/// Slice 1 covers the ops whose removal is the feature's own occupancy with no
/// path knowledge needed — facing, the prismatic voids, drilling, and the
/// sphere dome (the one feature a shaped tool may cut today, so the one the
/// first consumer below actually needs). The standing prismatic pair
/// (`CylinderBoss`/`ProfileBoss`/the elliptic and polyline bosses) is
/// deliberately absent: how far out from a boss the surrounding material is
/// cleared is a property of the PATH, not of the declared feature, so there is
/// no closed form to write and pretending otherwise would over-remove.
/// The 3D-wire pair (`ContourPocket`/`ContourBoss`) is absent for both reasons
/// at once (DECISIONS.md "the 3D-wire Contour…"): a rim wire declares its
/// WALL — the region inside the projection has no declared floor (the rim only
/// pins the boundary), and how far out the boss side was cleared is the
/// path fact again. Both fall to the `None` arm and mark the model incomplete.
/// What an edge-round pass actually takes: the torus band the ground arc sweeps
/// — and **only the part of it the arc actually reaches**.
///
/// This is where the trail-off stops being a report and starts carrying load
/// (DECISIONS.md → "the corner rounder gets an operation"). An arc that runs out
/// short of tangency removes LESS than the nominal round: a residual ridge
/// stands at each end. Model the full band anyway and this file would believe
/// material is gone that is still there — and that is the dangerous direction,
/// because `check_moves` reads this model to decide whether a plunge is into
/// air. So the band is bounded by where the arc really cuts, and the ridges fall
/// outside the declared section, which is exactly how they survive.
///
/// For a circular edge that surface is a `Floor::Meridian` over an `Annulus`,
/// both of which this file already had — which is the reason slice 1 scoped
/// there. For a polyline edge it is a `Floor::PathForm` over a `PathBand`,
/// which is the same two objects with the plan circle replaced by the axis path
/// the op actually emits (slice 2). The round's centre of curvature comes from
/// `parse::EdgeRound` either way, so the wall this model reports and the wall
/// the machine leaves are one curve.
fn edge_round_removal(p: &Part, op: &Operation, tool: &Tool) -> Option<Vec<Removal>> {
    let feature = p.features.get(&op.feature_name)?;
    let er = crate::parse::derive_edge_round(p, feature)?;
    let arc = tool.ground_arc()?;
    let r = arc.r;
    let z_c = er.centre_z(op.face_standoff);

    // The arc cuts from `trail_wall` degrees off the wall tangency to
    // `trail_face` degrees off the face tangency, and the span it covers is a
    // span of DISTANCE FROM THE WALL — `d(psi) = (R - wall_standoff) - r·cos
    // psi` — before it is a span of anything else. Both arms below are that one
    // interval, read in the coordinate their own plan curve speaks.
    let end_wall = arc.trail_wall.to_radians();
    let end_face = (90.0 - arc.trail_face).to_radians();
    let d_c = er.radius - op.wall_standoff;
    let depth = |psi: f64| d_c - r * psi.pcos();

    if let crate::parse::EdgeLoop::Polyline { points } = &er.edge {
        return edge_round_removal_path(p, op, &er, &arc, points, depth(end_wall), depth(end_face), z_c);
    }

    let (rho_c, _) = er.centre(op.wall_standoff, op.face_standoff);
    let (_, _, wall_radius) = er.circle().expect("the polyline arm returned above");
    let inward = if er.boss { -1.0 } else { 1.0 };
    let at = |psi: f64| (rho_c - inward * r * psi.pcos(), z_c + r * psi.psin());
    let (rho_a, _) = at(end_wall);
    let (rho_b, _) = at(end_face);

    let (mut r_inner, mut r_outer) = (rho_a.min(rho_b), rho_a.max(rho_b));
    // A standoff can push the band past the wall into the void the feature's own
    // op already opened. Claiming void is harmless but untrue, so the band is
    // clipped to the material side of the wall it breaks.
    if er.boss {
        r_outer = r_outer.min(wall_radius);
    } else {
        r_inner = r_inner.max(wall_radius);
    }
    if r_outer - r_inner <= 1e-12 {
        return None; // no arc reaches the material: nothing declared, nothing claimed
    }

    // The band's surface, sampled with rho ASCENDING as `Floor::Meridian`
    // requires. Sampling rather than a closed form because that is the shape
    // this file's floor model speaks; the samples come off the same circle the
    // op positions by, never a second derivation of it.
    const SAMPLES: usize = 64;
    let mut samples = Vec::with_capacity(SAMPLES + 1);
    for i in 0..=SAMPLES {
        let t = i as f64 / SAMPLES as f64;
        let psi = end_wall + (end_face - end_wall) * t;
        let (rho, z) = at(psi);
        samples.push((rho, z));
    }
    samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    samples.retain(|(rho, _)| *rho >= r_inner - 1e-12 && *rho <= r_outer + 1e-12);
    if samples.len() < 2 {
        return None;
    }

    let (cx, cy, _) = er.circle().expect("the polyline arm returned above");
    Some(vec![Removal { ceiling: None,
        section: Section::Annulus { x: cx, y: cy, r_inner, r_outer },
        z_top: p.stock.z,
        floor: Floor::Meridian { x: cx, y: cy, samples },
    }])
}

/// The polyline arm of the band above, and the whole of what slice 2 adds to
/// this file.
///
/// The band is stated as distances from the AXIS PATH rather than from the wall,
/// and that is the load-bearing choice. A wall-relative band is the same set
/// wherever the tool rides right against the wall, and a WRONG one — wrong in
/// the over-claiming direction — everywhere it does not: at a plan corner
/// tighter than the tool, the axis pivots on its offset vertex and the arc never
/// reaches the corner, so a wall-relative model would report a round the part
/// does not have and licence a plunge into material that is still standing.
/// Measured from the path, the same geometry reports the shortfall for free,
/// because the path is what the tool actually rode.
///
/// `d_wall`/`d_face` arrive as distances INTO THE MATERIAL from the wall, the
/// same interval the circular arm reads; `+ reach` turns each into a distance
/// from the path, which is the only conversion the two arms do differently.
fn edge_round_removal_path(
    p: &Part,
    op: &Operation,
    er: &crate::parse::EdgeRound,
    arc: &crate::parse::GroundArc,
    points: &[Pt],
    d_wall: f64,
    d_face: f64,
    z_c: f64,
) -> Option<Vec<Removal>> {
    let reach = er.axis_offset(arc.effective_radius(), op.wall_standoff);
    let path = crate::parse::derive_edge_round_path(points, reach).ok()?;
    let flat = crate::parse::derive_edge_round_path_points(&path);

    // A standoff can push the band's inner rim past the wall into the void the
    // feature's own op already opened — the circular arm's clip, in the
    // distance-from-the-path coordinate: the wall itself sits at `reach`.
    let d_inner = (d_wall.min(d_face) + reach).max(reach);
    let d_outer = d_wall.max(d_face) + reach;
    if d_outer - d_inner <= 1e-12 {
        return None; // no arc reaches the material: nothing declared, nothing claimed
    }

    // The floor's `(d, z)` table, sampled off the quarter arc in the tool's own
    // frame: `d(psi) = rc - r·cos psi` is the surface's plan distance from the
    // path (`rc` is the arc centre's distance from the axis, so it is the arc
    // centre's distance from the path too — the standoffs cancel out of it
    // exactly as they must, since the tool's arc never moves relative to its
    // own axis). Sampled over the FULL quarter rather than the trail-clipped
    // cut span, because the floor states the form's ideal surface and the
    // Section above carries the clipping — the same division the closed form
    // had. Uniform in psi, which clusters samples in `d` at the wall end where
    // dz/dd runs vertical, exactly where clustering is needed.
    let samples = sample_meridian(0.0, std::f64::consts::FRAC_PI_2, |psi| {
        (arc.rc - arc.r * psi.pcos(), z_c + arc.r * psi.psin())
    });

    Some(vec![Removal { ceiling: None,
        section: Section::PathBand {
            path: flat.clone(),
            material_inside: er.boss,
            d_inner,
            d_outer,
        },
        z_top: p.stock.z,
        floor: Floor::PathForm { path: flat, samples },
    }])
}

/// What a chamfer pass takes: the conical band the flank sweeps — the edge
/// round's removal one form over, and SIMPLER, because a line interpolates
/// exactly: both floors are two-point tables, no sampling loop.
///
/// The same two safety choices carry over unchanged. The band is clipped to
/// the material side of the wall (a standoff pushes the face into the void the
/// feature's own op already opened; claiming void is harmless but untrue), and
/// the polyline arm measures from the AXIS PATH, so a plan corner the tool
/// cannot reach is reported as standing rather than claimed cut.
fn chamfer_removal(p: &Part, op: &Operation, tool: &Tool) -> Option<Vec<Removal>> {
    let feature = p.features.get(&op.feature_name)?;
    let ec = crate::parse::derive_chamfer(p, feature)?;
    let ch = tool.chamfer.as_ref()?;
    let cone = tool.ground_cone()?;
    let pl = crate::parse::derive_chamfer_placement(
        &ec,
        &cone,
        crate::parse::derive_tip_clearance(op, ch),
        op.wall_standoff,
        op.face_standoff,
    );
    // The face's two corners in part Z: bottom on the (shifted) wall, top at
    // the (lifted) face plane.
    let z_low = ec.z_top + op.face_standoff - pl.depth;
    let z_high = ec.z_top + op.face_standoff;

    if let crate::parse::EdgeLoop::Polyline { points } = &ec.edge {
        let path = crate::parse::derive_edge_round_path(points, pl.reach).ok()?;
        // A loop whose points restate the leg (slice 9) is one band per
        // stretch, each at its own depth along its stretch of the axis
        // path; a stretch left unbroken claims nothing.
        let runs = crate::parse::derive_chamfer_runs(&ec, &path);
        if !runs.is_empty() {
            let outline = crate::parse::derive_edge_round_path_points(&path);
            let mut out = Vec::new();
            for run in &runs {
                let Some(leg) = run.leg else { continue };
                let rp = crate::parse::derive_leg_placement(
                    leg, ec.z_top, &cone, crate::parse::derive_tip_clearance(op, ch), op.wall_standoff, op.face_standoff,
                );
                let d_lo = rp.r_contact;
                let d_hi = rp.r_contact + rp.width;
                let d_inner = d_lo.max(rp.reach);
                if d_hi - d_inner <= 1e-12 {
                    continue;
                }
                // The chain's last vertex carries no bulge, so flattening it
                // as a loop adds nothing past its end.
                let chain = crate::parse::derive_edge_round_path_points(&run.path);
                // There and back along the chain: a closed polygon whose
                // distance is the distance to the open chain, for the floor.
                let there_and_back: Vec<Pt> = chain.iter().chain(chain.iter().rev().skip(1)).copied().collect();
                out.push(Removal { ceiling: None,
                    section: Section::ChainBand {
                        outline: outline.clone(),
                        chain,
                        material_inside: ec.boss,
                        d_inner,
                        d_outer: d_hi,
                    },
                    z_top: p.stock.z,
                    floor: Floor::PathForm {
                        path: there_and_back,
                        samples: vec![(d_lo, z_high - rp.depth), (d_hi, z_high)],
                    },
                });
            }
            return Some(out);
        }
        let flat = crate::parse::derive_edge_round_path_points(&path);
        // In distance-from-the-path coordinates the authored wall sits at
        // `reach`; the face spans [reach − s_w, reach − s_w + w], clipped to
        // the wall — the circular arm's clip, one coordinate over.
        let d_lo = pl.r_contact;
        let d_hi = pl.r_contact + pl.width;
        let d_inner = d_lo.max(pl.reach);
        if d_hi - d_inner <= 1e-12 {
            return None; // the face never reaches the material
        }
        return Some(vec![Removal { ceiling: None,
            section: Section::PathBand {
                path: flat.clone(),
                material_inside: ec.boss,
                d_inner,
                d_outer: d_hi,
            },
            z_top: p.stock.z,
            floor: Floor::PathForm {
                path: flat,
                samples: vec![(d_lo, z_low), (d_hi, z_high)],
            },
        }]);
    }

    let (cx, cy, wall_radius) = ec.circle().expect("the polyline arm returned above");
    // The face in feature-radial rho: bottom corner `s_w` off the wall into
    // the void, top corner `width` further into the material.
    let inward = if ec.boss { -1.0 } else { 1.0 };
    let rho_low = wall_radius - inward * op.wall_standoff;
    let rho_high = rho_low + inward * pl.width;
    let (mut r_inner, mut r_outer) = (rho_low.min(rho_high), rho_low.max(rho_high));
    if ec.boss {
        r_outer = r_outer.min(wall_radius);
    } else {
        r_inner = r_inner.max(wall_radius);
    }
    if r_outer - r_inner <= 1e-12 {
        return None; // the face never reaches the material
    }
    let mut samples = vec![(rho_low, z_low), (rho_high, z_high)];
    samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    Some(vec![Removal { ceiling: None,
        section: Section::Annulus { x: cx, y: cy, r_inner, r_outer },
        z_top: p.stock.z,
        floor: Floor::Meridian { x: cx, y: cy, samples },
    }])
}

pub(crate) fn removals_for(p: &Part, op: &Operation) -> Option<Vec<Removal>> {
    // The surface the pass actually cuts to, not the ceiling it guarantees
    // (parse::derive_emitted_skin, the one home for that distinction).
    let skin = crate::parse::derive_emitted_skin(p, op)?;
    // The rim collar (see `rim_collar`) is the tool's, so the revolved arms need
    // it. An op with no tool declared is caught upstream by validate().
    let tool = p.tools.get(&op.tool_num);

    // The rest floor (Phase 33) removes what each of its floors removes, and
    // nothing where the earlier op already cut. No floor — the op refuses —
    // is no removal to claim.
    if crate::rest::is_rest(op) {
        let pieces = crate::rest::pieces(p, op).ok()?;
        let mut rs = Vec::new();
        for pc in pieces.iter().filter(|pc| pc.is_cut()) {
            rs.extend(removals_for(&pc.part, &pc.op)?);
        }
        return Some(rs);
    }

    // Drill is the one op that reads a list of features rather than one, and it
    // removes the DRILL's own circle, not the bore's — the hole a drill leaves
    // is the tool's diameter wherever the tip reached.
    if op.at_points() {
        let tool = p.tools.get(&op.tool_num)?;
        let cone = tool.ground_cone();
        let names = op.drill_points.as_ref()?;
        let mut rs = Vec::new();
        for name in names {
            match p.features.get(name)? {
                // The mouth-cone arm (DECISIONS.md → "spot drilling and
                // countersinking…"): the dimple is the tool's swept envelope —
                // a frustum, stated as the two-point table a line interpolates
                // exactly, with `interp_table`'s end clamp giving the relieved
                // flat's floor inside `tip_r` for free. Claiming the flat's
                // disc is the drill's own idealization one form over (a twist
                // drill's chisel web does not cut either, and its removal
                // claims the full circle): the envelope is what the plunge
                // sweeps, and it is what licenses a spot into solid stock —
                // material under the flat is gone from the model exactly as a
                // drilled hole's core is.
                Feature::CylinderBore { x, y, z, csink: Some(spec), .. } if cone.is_some() => {
                    let cone = cone.as_ref().expect("guarded by the arm");
                    let rim = derive_contour_top(p.stock.z, *z);
                    let seat_r = spec.seat_dia() / 2.0;
                    let z_tip = crate::parse::derive_csink_tip_z(rim, spec.seat_dia(), cone);
                    rs.push(Removal { ceiling: None,
                        section: Section::Disc { x: *x, y: *y, r: seat_r },
                        z_top: rim,
                        floor: Floor::Meridian {
                            x: *x,
                            y: *y,
                            samples: vec![(cone.tip_r, z_tip), (seat_r, rim)],
                        },
                    });
                }
                // A ChamferMill on a seatless hole is rejected by validate();
                // reaching it here means the model cannot speak for the op.
                Feature::CylinderBore { .. } if cone.is_some() => return None,
                // The counterbore arm (DECISIONS.md → "the counterbore…"):
                // the endmill really does cut a flat floor, so the flat claim
                // is TRUE here and the removal is one line — no meridian
                // table. The disc reads the DECLARED dia; validate() has
                // already held it equal to the tool's own, the drill-arm
                // idealization below made a declared pairing.
                Feature::CylinderBore { x, y, z, cbore: Some(cb), .. } if tool.name == "Endmill" => {
                    let rim = derive_contour_top(p.stock.z, *z);
                    rs.push(Removal { ceiling: None,
                        section: Section::Disc { x: *x, y: *y, r: cb.dia / 2.0 },
                        z_top: rim,
                        floor: Floor::Flat(derive_z_cut(rim, cb.depth)),
                    });
                }
                Feature::CylinderBore { extent, x, y, z, .. } => {
                    let rim = derive_contour_top(p.stock.z, *z);
                    rs.push(Removal { ceiling: None,
                        section: Section::Disc { x: *x, y: *y, r: tool.dia / 2.0 },
                        z_top: rim,
                        floor: Floor::Flat(derive_z_cut(rim, *extent)),
                    })
                }
                _ => return None,
            }
        }
        return Some(rs);
    }

    // The form passes are the `Finish`es whose removal is the TOOL's form
    // rather than the feature's wall, so they branch before the feature match
    // — the same shape as Drill above, and for the same reason.
    if op.follows_surface() {
        if let Some(t) = tool {
            if t.rounder.is_some() {
                return edge_round_removal(p, op, t);
            }
            if t.chamfer.is_some() {
                return chamfer_removal(p, op, t);
            }
        }
    }

    let feature = p.features.get(&op.feature_name)?;

    // A `Clear` over a standing revolve removes the MOAT it cut, which is not
    // the feature's own offset surface — that is what the arms below report,
    // and it is the FINISH's removal. Branching before the match is what keeps
    // the two apart (DECISIONS.md → "roughing a convex revolve…", fork 4A).
    if op.op_type == OpType::Clear {
        if let Some(rs) = revolve_clear_removal(p, op, feature, tool, skin) {
            return Some(rs);
        }
    }

    let one = |r: Removal| Some(vec![r]);
    match feature {
        // The Plane IS the finished face, so what is gone is everything above
        // it, across the whole stock (facing overhangs the ends by a radius).
        // A `Face` removes the column over its own outer loop, less what stands
        // in it (Phase 28) — never the blank beside it, which is why a face on
        // a bar does not claim the bar.
        //
        // "Its own outer loop" as the TOOL meets it: every edge of a face is
        // open, so the path overhangs the outline and what it takes is the
        // outline pushed out one tool diameter — `Region::family_wall`, the one
        // home the emitter erodes its family from, so the two cannot disagree
        // about where the cut ends. The same standing the moat has: a width
        // the op sweeps, stated once, read by the emitter and the removal. On
        // a bar that swath is scrap; if it were PART, something would be
        // declared standing there, and it would be an island or a bite.
        Feature::Face { z, .. } => {
            let region = crate::region::of(p, op)?;
            // What the PATTERN sweeps past an open edge — not what the op is
            // PERMITTED to sweep there (Phase 32 slice 3, fork D). The two
            // were one polygon until this slice, and could not be both sizes:
            // the permission is `face_overhang`, read by `check_moves` alone.
            let swept = region.family_wall(pattern_push(p, op, tool?))?;
            let points = crate::region::clipped_outline(p, &swept.wall).unwrap_or(swept.wall);
            // …and less the voids it goes AROUND (slice 2): nothing over an
            // open void is faced, so nothing over it is claimed removed. The
            // region shrinks each void by one tool DIAMETER, which is the
            // ring the offset family's outermost loop sweeps past the rim —
            // and which spine does not sweep either, its rows owning the floor
            // and stopping at it (Phase 32 slice 4; slice 3 said the same of
            // the outer edges and missed these). For spine the hole is the
            // void as DECLARED.
            // A Clear's allowance stands on every flank of what it goes
            // around, and its floor skin waits for the End that takes it
            // (Phase 61 slice 1): both are zero for an End.
            let hole_skin = if op.op_type == OpType::Clear { skin } else { 0.0 };
            let floor_z = *z + crate::parse::derive_floor_skin(p, op);
            let mut holes = island_holes(p, &op.feature_name, hole_skin);
            match (rows_only(p, op), p.features.get(&op.feature_name)) {
                // …every drawn void as declared — except a drawn loop that
                // is a declared void's MOUTH, which the rows run over bridged
                // (Phase 46 slice 3, the region's own test): the claim says
                // what the path did, or the void's own op is refused into a
                // column the model wrongly holds.
                (true, Some(Feature::Face { loops, .. })) => holes.extend(
                    loops[1..]
                        .iter()
                        .filter(|l| !p.features.iter().any(|(name, f)| name.as_str() != op.feature_name && mouth_in_floor(p, f, l, *z)))
                        .cloned(),
                ),
                _ => holes.extend(region.open_void_holes()),
            }
            holes.extend(conceded_holes(p, op));
            // A neighbour the swath closed over whole (Phase 32, after the close) stands in it.
            holes.extend(swept.enclosed);
            let removal = |points: Vec<Pt>, holes: Vec<Vec<Pt>>| Removal { ceiling: None, section: Section::Polygon { points, holes }, z_top: p.stock.z, floor: Floor::Flat(floor_z) };
            // …and of the SCRAP past the floor, only what the tool can REACH
            // (the nooks): a swath's inside corners past a wall chain's end,
            // and the strip behind a wall thinner than the push, are narrower
            // than the tool and no pass enters them. The FLOOR too is claimed
            // as reached (2026-11-10): the fillet at an inside corner no tool
            // enters is stock the model holds. `None` where the family itself
            // refuses — the op emits nothing then, and the claim stays the
            // swath, whole. A pattern that runs rows claims the floor, pushed
            // by nothing (`pattern_push`), so its swath IS its floor.
            if rows_only(p, op) {
                return Some(reached(p, op, tool, hole_skin, points, holes, &removal));
            }
            let Some(reach) = reachable(p, op, &region, tool?, hole_skin) else {
                return one(removal(points, holes));
            };
            let floor = region.family_wall(0.0)?;
            let floor_pts = crate::region::clipped_outline(p, &floor.wall).unwrap_or(floor.wall);
            let mut floor_holes = island_holes(p, &op.feature_name, hole_skin);
            if let Some(Feature::Face { loops, .. }) = p.features.get(&op.feature_name) {
                floor_holes.extend(loops[1..].iter().cloned());
            }
            floor_holes.extend(conceded_holes(p, op));
            floor_holes.extend(floor.enclosed);
            let mut out = clip_reached(&reach, floor_pts, floor_holes, &removal);
            for (outer, reach_holes) in reach {
                for piece in crate::offset::clip_to(&outer, &points) {
                    let mut hs = holes.clone();
                    hs.extend(reach_holes.iter().cloned());
                    out.push(removal(piece, hs));
                }
            }
            Some(out)
        }
        // …unless a boss stands THROUGH the face (Phase 26 slice 3): then the
        // face goes around it, and what is gone is the stock's own footprint
        // minus those islands. `All` stays the answer wherever nothing stands.
        Feature::Plane { z } => {
            // A Clear on the Plane (Phase 61 slice 1) removes the volume less
            // its allowance round everything that stands, down to the floor
            // an End that follows will take; an End removes it whole.
            let hole_skin = if op.op_type == OpType::Clear { skin } else { 0.0 };
            let floor_z = *z + crate::parse::derive_floor_skin(p, op);
            let mut holes = island_holes(p, &op.feature_name, hole_skin);
            holes.extend(crate::region::of(p, op).map(|r| r.open_void_holes()).unwrap_or_default());
            holes.extend(conceded_holes(p, op));
            if holes.is_empty() {
                return one(Removal { ceiling: None, section: Section::All, z_top: p.stock.z, floor: Floor::Flat(floor_z) });
            }
            // Something stands: the blank less what stands, as reached — an
            // island's notch keeps its fillet.
            let removal = |points: Vec<Pt>, holes: Vec<Vec<Pt>>| Removal { ceiling: None, section: Section::Polygon { points, holes }, z_top: p.stock.z, floor: Floor::Flat(floor_z) };
            Some(reached(p, op, tool, hole_skin, crate::parse::derive_blank(&p.stock), holes, &removal))
        }
        // The prismatic voids: footprint shrunk by the allowance, floor left
        // alone. `stock_to_leave` here is a RADIAL allowance and only that
        // (DECISIONS.md "stock_to_leave is a radial allowance…": the wall is the
        // only surface geometry defines, the floor is the feature's own
        // `extent`), so the pass cuts to full depth and a raised floor would
        // report a skin the machine never leaves.
        Feature::CylinderBore { dia, extent, x, y, z, .. } => {
            let rim = derive_contour_top(p.stock.z, *z);
            one(Removal { ceiling: None,
                section: Section::Disc { x: *x, y: *y, r: dia / 2.0 - skin },
                z_top: rim,
                floor: Floor::Flat(derive_z_cut(rim, *extent)),
            })
        }
        Feature::ProfilePocket { points, z, extent, .. } => {
            let z_top = derive_contour_top(p.stock.z, *z);
            // A rough that leaves its allowance on the floor (a later `Floor`
            // takes it — `parse::derive_floor_skin`) has not removed it.
            let floor = Floor::Flat(derive_z_cut(z_top, *extent) + crate::parse::derive_floor_skin(p, op));
            // A `Floor` on the pocket removes the skin the roughs left on its
            // floor and nothing above it — so the volume is still the Clear's
            // to open, and a floor pass over a pocket nobody cleared drives
            // into material that outlives it.
            let z_top = if op.op_type == OpType::End {
                derive_z_cut(z_top, *extent) + crate::parse::derive_floor_left(p, op)
            } else {
                z_top
            };
            // The islands this pocket goes around are not its removal (lane D
            // fork 4A): each is a hole, grown by the allowance exactly as the
            // wall is shrunk by it — the emitter stands off both by the skin.
            let mut holes = island_holes(p, &op.feature_name, skin);
            holes.extend(conceded_holes(p, op));
            holes.extend(crate::region::of(p, op).map(|r| r.open_void_holes()).unwrap_or_default());
            // A pocket drawn past the blank removes only what the blank had
            // (Phase 27): the outline clipped to the stock, the authored
            // points untouched wherever they already lie inside it.
            let clipped = crate::region::clipped_outline(p, points);
            let points = clipped.as_ref().unwrap_or(points);
            // As reached (2026-11-10): the pocket's inside corners keep the
            // fillet a round tool leaves.
            let removal = |points: Vec<Pt>, holes: Vec<Vec<Pt>>| Removal { ceiling: None, section: Section::Polygon { points, holes }, z_top, floor: floor.clone() };
            if skin <= 0.0 {
                return Some(reached(p, op, tool, 0.0, points.clone(), holes, &removal));
            }
            // With an allowance the void is the pocket eroded by the skin. An
            // inward offset of an arbitrary polyline can split, vanish, or
            // self-intersect — outcomes this model once said it had "nowhere
            // to put" and declined on. But the model is a LIST of removals and
            // the offsetter resolves exactly those outcomes into zero-or-more
            // clean loops (DECISIONS.md → the 2026-07-29 offset-decomposition
            // DRAFT, Finding 2; built as Phase 10 lane C): evaluate it ONCE,
            // through the same `offset` home and the same sign convention the
            // emitters use — a positive distance on the AUTHORED points is the
            // material side under the winding law, so no `to_ccw`, exactly as
            // `ops::profile` offsets its edge — and keep one `Polygon` per loop.
            // A split is several removals. A vanished offset leaves no program
            // to model — `ops::profile` rejects a degenerate edge and
            // `ops::clear` rejects a pocket too small for tool plus allowance —
            // so the answer cannot matter, and the model declines rather than
            // claim an empty void it never checked.
            // Flattened ALONG its arcs, as the emitter's own loops are (Phase
            // 37 slice 0): round a wall corner that points into the pocket the
            // skin is an arc about that corner, and its chord would claim the
            // sliver between them as removed — stock the wall pass leaves.
            let loops: Vec<Vec<Pt>> = crate::offset::offset_closed_polyline_verts(points, skin).iter().map(|lp| crate::offset::flatten_loop(lp)).collect();
            if loops.is_empty() || loops.iter().any(|l| l.len() < 3) {
                return None;
            }
            Some(
                loops
                    .into_iter()
                    .flat_map(|pts| {
                        // A split pocket: each piece keeps the islands inside it.
                        let mine = holes.iter().filter(|h| plan_nesting(h, &pts) == Nesting::Inside).cloned().collect();
                        reached(p, op, tool, skin, pts, mine, &removal)
                    })
                    .collect(),
            )
        }
        // The dome stands inside its own footprint; the allowance is a normal
        // offset, which on a sphere is a larger radius.
        Feature::Sphere { dia, x, y, z } => {
            let r = dia / 2.0 + skin;
            let mut out = vec![Removal { ceiling: None,
                section: Section::Disc { x: *x, y: *y, r },
                z_top: p.stock.z,
                floor: Floor::Dome { x: *x, y: *y, z: *z, r },
            }];
            // The dome's rim is its equator, and reaching it sweeps the same
            // collar every other surface's rim does — but on a sphere the
            // collar is only the below-rim slice of what the pass removes
            // beside the rim. The full claim is the swept TUBE (DECISIONS.md →
            // "rim-area accounting…", fork 1A): a ball riding the dome sweeps
            // the shell between the surface at `skin` and the surface at
            // `skin + dia`, and on a sphere both are concentric spheres, so
            // the shell's top beside the rim is exactly a `Dome` ceiling at
            // `r + dia` — 16.6mm above the equator at the rim on the shipped
            // fixture, where the flat collar stopped at 3. The end-cap
            // subtlety that guards the general carriers does not arise here: a
            // sphere's rim tangent is vertical, so the last CL sits AT the
            // equator plane and the shell above it is swept in full.
            // A FORM tool's claim beside the rim is its swept tube too
            // (Phase 10 lane B, slice 1): the cutting form revolved about
            // the axis at every ring the emitter rides — the same ring list,
            // from the same `parse` home — floor `tip + z_form(d)`, ceiling
            // `tip + cut_top`. The flat collar it replaces claimed a box
            // `rim ± widest` × `rim_z ± tip_to_widest`, whose inner-lower
            // corner is air a cone from a point never enters and stock that
            // stands after the pass: 1289 mm³ of it on the shipped fixture,
            // in the direction `check_moves` must never be handed. The form
            // is the CUTTING envelope (`Silhouette::Profile.pts`, clipped at
            // a declared `cut_top`), never the drawing: the relieved wall
            // above the band removes nothing. `cut_bottom` bounds contact
            // only — the emitter rejects a band that does not reach the tip
            // it rides — so the floor starts at the tip. No `stock_to_leave`
            // here by `validate`'s own error, so the rings sit on `dia/2`
            // exactly as emitted.
            if let (Some(t), Some(drawn), Some(stepover)) =
                (tool, tool.and_then(|t| t.profile.as_ref()), op.stepover)
            {
                if let (Silhouette::Profile { pts, .. }, true) = (Silhouette::of(t), op.follows_surface()) {
                    let cut_top = pts[pts.len() - 1].y;
                    let rings = crate::parse::sphere_profile_passes(drawn, dia / 2.0, *z, stepover);
                    let tips: Vec<(f64, f64)> = rings.iter().map(|&(z_tip, rho)| (rho, z_tip)).collect();
                    out.extend(swept_tube(*x, *y, &tips, CutForm::Profile { pts: &pts, cut_top }));
                    return Some(out);
                }
            }
            if let (Some(mut collar), Some(t)) =
                (rim_collar(*x, *y, r, *z, tool, false, tool.map_or(0.0, |t| t.dia)), tool)
            {
                // Ball geometry: the Dome ceiling is the tube's closed form
                // for a concentric shell. Any other tool keeps the flat
                // collar, the pre-tube claim.
                if t.name == "Ballmill" {
                    let r_shell = r + t.dia;
                    collar.z_top = *z + (r_shell * r_shell - r * r).sqrt();
                    collar.ceiling = Some(Floor::Dome { x: *x, y: *y, z: *z, r: r_shell });
                }
                out.push(collar);
            }
            Some(out)
        }
        // ---- the R2 cavities. Each is authored top-down with its mouth in the
        // stock-top plane (the bore precedent), so the removal is always "the
        // mouth disc, from the stock's top down to the meridian", and the only
        // per-feature work is walking that feature's own meridian.
        // ---- the standing surfaces of revolution. Each is authored base-up
        // from its own `z` datum (the outside-feature convention) and walks the
        // meridian its own op cuts. The ten of them differed ONLY in which
        // meridian that is, so the walk moved to `parse` when the revolve-boss
        // `Clear` became its second consumer (DECISIONS.md → "roughing a convex
        // revolve…", slice 0). The `Sphere` above is the one member that stays
        // here: its offset is a concentric sphere, which `Floor::Dome` reports
        // exactly rather than as a 256-point polyline.
        Feature::Cone { x, y, .. }
        | Feature::Torus { x, y, .. }
        | Feature::SplineRevolve { x, y, .. } => {
            let mer = crate::parse::derive_revolve_offset_meridian(feature, skin)?;
            // The tube is BALL geometry — centres at `skin + r_ball`, spherical
            // envelope — so it is gated on the tool being one. A profile tool's
            // sweep is its ground form dragged along a different centre path
            // entirely; claiming a ball's tube for it would be shaped-wrong in
            // both directions at once. Everything else keeps `cap()` — the flat
            // collar and its gated inner sibling — which is the pre-tube claim
            // and the honest reading of `tip_to_widest`'s profile arm.
            let ball = tool.filter(|t| t.name == "Ballmill");
            let Some(t) = ball else {
                return Some(cap(*x, *y, p.stock.z, mer.samples, mer.closes_on_axis, tool));
            };
            let r_ball = t.dia / 2.0;
            let floor = Floor::Meridian { x: *x, y: *y, samples: mer.samples };
            let r_out = floor.outer_radius();
            let section = if mer.closes_on_axis {
                Section::Disc { x: *x, y: *y, r: r_out }
            } else {
                Section::Annulus { x: *x, y: *y, r_inner: floor.inner_radius(), r_outer: r_out }
            };
            let mut out = vec![Removal { ceiling: None, section, z_top: p.stock.z, floor }];
            // The tube about the centre path — the same meridian home, priced
            // at the centre's own stand-off. Both rims are the tube's own ends,
            // so a band's top circle needs no gate — and neither does a torus's
            // hole, which is why the straddle collar retires for it too now
            // that the hole is opened rather than left buried (DECISIONS.md →
            // "the torus's annulus island…", fork 4). The tool works that rim
            // from inside, which is still what the PROBES ask about; that gate
            // is untouched.
            //
            // The centre path runs to the end of the EMITTER, not to the end of
            // the surface. Two carriers overrun their own meridian at the base
            // rim — the cone runs its offset line down to the base plane, the
            // secant ogive rolls the rim corner — and that run-out is the last
            // thing the flat `rim_collar` was still covering here. Handing it to
            // the tube retires the collar for the whole family: one envelope,
            // priced from the centres themselves, instead of a box drawn around
            // two different shapes (DECISIONS.md → "the emitter rim tails…").
            // `derive_revolve_emitter_tail` is empty for everything that stops
            // where its meridian does, so this is a no-op for the other seven.
            if let Some(centres) =
                crate::parse::derive_revolve_offset_meridian(feature, skin + r_ball)
            {
                let mut path = centres.samples;
                path.extend(crate::parse::derive_revolve_emitter_tail(feature, skin + r_ball));
                out.extend(swept_tube(*x, *y, &path, CutForm::Ball { r: r_ball }));
            }
            Some(out)
        }
        // The cylinder boss FINISH's removal (Phase 10 lane A, fork 4; the
        // keystone `None` arm, paid): the band a flat endmill sweeps riding the
        // wall at `R + skin` — an annulus one tool diameter wide, from the
        // boss top down its extent. Derived from the declaration through the
        // same numbers `finish_cylinder_boss` rides (`r_part + r_tool + skin`
        // on the centre circle), never read off the emitted moves. The top is capped at
        // the tool's cutting top above its lowest tip (the last level lands
        // on the base): a declared `flute_len` shorter than the boss is shank
        // above it, and shank removes nothing.
        Feature::CylinderBoss { dia, extent, x, y, z, .. } if op.follows_surface() => {
            let t = tool?;
            if t.name != "Endmill" {
                return None;
            }
            let r = t.dia / 2.0;
            let rr = dia / 2.0 + skin;
            let cut = Silhouette::of(t).cutting_top().map_or(*extent, |h| h.min(*extent));
            one(Removal {
                ceiling: None,
                section: Section::Annulus { x: *x, y: *y, r_inner: rr, r_outer: rr + 2.0 * r },
                z_top: z + cut,
                floor: Floor::Flat(*z),
            })
        }
        // The prismatic bosses (Phase 10 lane A, slice 1). A flat endmill
        // riding a loop sweeps everything within one tool radius of it on
        // BOTH sides — two `PathBand`s about the loop, `d ∈ [0, r]`, the
        // inner one ending exactly at the skin surface because the loop
        // stands `r + skin` off the wall. The loops are the emitter's own
        // (`parse::prismatic_boss_cl_loops`), so the removal and the path
        // cannot disagree; the FINISH's removal is the innermost loop's
        // swath, the moat `Clear`'s is every loop of the family. Top capped
        // at the tool's cutting top above the lowest tip; floor the base.
        Feature::ProfileBoss { .. } | Feature::ContourBoss { .. }
            if op.cuts_walls() =>
        {
            let t = tool?;
            if t.name != "Endmill" {
                return None;
            }
            let r = t.dia / 2.0;
            let (top, base) = crate::parse::prismatic_boss_z_span(feature, p.stock.z)?;
            // The wire boss's FINISH runs a tip that follows the wire, `extent`
            // below it (slice 2): its floor is `AlongWire`, and so is its
            // ceiling when a declared cutting top caps the band — tip plus
            // cutting top is the same function with a smaller drop. The moat
            // below is flat levels like any boss's.
            if let (Feature::ContourBoss { points, extent }, true) = (feature, op.follows_surface()) {
                let loops = crate::parse::prismatic_boss_cl_loops(feature, r + skin)?;
                let ceiling = Silhouette::of(t)
                    .cutting_top()
                    .map(|h| Floor::AlongWire { wire: points.clone(), drop: extent - h });
                let mut out = Vec::with_capacity(loops.len() * 2);
                for path in loops {
                    for material_inside in [true, false] {
                        out.push(Removal {
                            ceiling: ceiling.clone(),
                            section: Section::PathBand { path: path.clone(), material_inside, d_inner: 0.0, d_outer: r },
                            z_top: p.stock.z,
                            floor: Floor::AlongWire { wire: points.clone(), drop: *extent },
                        });
                    }
                }
                return Some(out);
            }
            let loops: Vec<Vec<Pt>> = match op.op_type {
                OpType::Side | OpType::Copy | OpType::Form => crate::parse::prismatic_boss_cl_loops(feature, r + skin)?,
                _ => {
                    let stepover = op.stepover?;
                    let width = crate::parse::derive_moat_width(p, op, feature, t.dia);
                    crate::parse::prismatic_boss_moat_family(feature, r, skin, stepover, width)?
                        .into_iter()
                        .flatten()
                        .collect()
                }
            };
            let cut = Silhouette::of(t).cutting_top().map_or(top - base, |h| h.min(top - base));
            let mut out = Vec::with_capacity(loops.len() * 2);
            for path in loops {
                for material_inside in [true, false] {
                    out.push(Removal {
                        ceiling: None,
                        section: Section::PathBand { path: path.clone(), material_inside, d_inner: 0.0, d_outer: r },
                        z_top: base + cut,
                        floor: Floor::Flat(base),
                    });
                }
            }
            Some(out)
        }
        Feature::SplineRevolveFunnel { meridian, x, y, .. } => {
            let (u0, u1) = meridian.domain();
            // Only the meridian's z differences carry meaning; its mouth pole
            // maps onto the stock top, exactly as the op places it.
            let z_shift = p.stock.z - meridian.eval(u0).1;
            Some(cavity(*x, *y, p.stock.z, sample_meridian(u0, u1, |u| {
                let (rho, z_rel) = crate::parse::spline_revolve_funnel_cl_point(meridian, skin, u);
                (rho, z_shift + z_rel)
            }), tool))
        }
        _ => None,
    }
}

/// A top-open cavity: the mouth circle in plan, everything from the stock's top
/// down to the meridian. Inside the meridian's innermost circle — a throat's
/// waist, a funnel's tip — the cavity bottoms out flat, which is what
/// `Floor::Meridian`'s inward clamp does.
fn cavity(x: f64, y: f64, z_top: f64, samples: Vec<(f64, f64)>, tool: Option<&Tool>) -> Vec<Removal> {
    let floor = Floor::Meridian { x, y, samples };
    let (r_in, r_out) = (floor.inner_radius(), floor.outer_radius());
    let mouth = rim_collar(x, y, r_out, floor.z_at(x + r_out, y), tool, true, tool.map_or(0.0, |t| t.dia));
    // The cavity's INNER end — a funnel's tip circle, a throat's waist — is a
    // rim too, and the tool bottoms out there: a ball tangent to the wall at
    // that circle has its lowest point below the circle's plane, dimpling the
    // middle. Same overrun as the mouth, at the other end, and it reaches
    // across the axis rather than around it.
    // Only where the surface ENDS at a circle. A bowl closes on its axis, and a
    // ball bottoming out at a smooth vertex touches it and no more — there is no
    // beyond to dimple.
    let tip = if r_in <= EPS { None } else { rim_collar(x, y, r_in, floor.z_at(x + r_in, y), tool, true, tool.map_or(0.0, |t| t.dia / 2.0)) }
        .map(|mut c| {
            if let Section::Annulus { r_outer, .. } = c.section {
                c.section = Section::Disc { x, y, r: r_outer };
            }
            c
        });
    let mut out = vec![Removal { ceiling: None, section: Section::Disc { x, y, r: r_out }, z_top, floor }];
    out.extend(mouth);
    out.extend(tip);
    out
}

/// The collar a tool must sweep to reach the EDGE of a declared surface.
///
/// The declared-geometry rule says what stands after an op; it does not say
/// that reaching the *rim* of that geometry puts the tool partly outside it.
/// It does, unavoidably: a ball contacting a rim whose tangent is vertical has
/// its centre a radius beyond the rim and its lowest point a radius below it, so
/// finishing any surface down to its edge cuts a collar around that edge. That
/// material is removed BY THIS OP — leaving it out is what made the model
/// report every correct ball-nose program as driving its tool into standing
/// stock (DECISIONS.md "The entry restriction refuses the program…").
///
/// Bounded by the tool's own extent about the contact rather than run to the
/// stock's top: one tool diameter wide, and the tool's tip-to-widest drop above
/// and below. A collar that ran to the top would swallow the buried-cap case,
/// which is the most valuable thing the lint says.
fn rim_collar(
    x: f64,
    y: f64,
    r_rim: f64,
    z_rim: f64,
    tool: Option<&Tool>,
    straddle: bool,
    reach: f64,
) -> Option<Removal> {
    let tool = tool?;
    let drop = tip_to_widest(tool);
    if drop <= 0.0 {
        return None;
    }
        // `straddle` is the cavity/cap difference, and it is about what lies on the
    // inward side of the rim. For a CAVITY the mouth's inward side is the wall
    // the tool grazes as it overruns the rim — the last pass runs a hair past
    // the declared edge, which on a sloped wall lands at radii slightly *inside*
    // the mouth as well as outside it. For a CAP the inward side is the part
    // itself, where the tool never goes, so the collar there is outward only.
    //
    // `reach` is how far past the rim the tool's cutting end gets. Contacting a
    // rim from OUTSIDE (a cap's base, a cavity's mouth) puts the centre a radius
    // beyond it, so the far side of the tool reaches a diameter. Bottoming out
    // at a cavity's inner circle puts the centre inside the void already, so the
    // reach is only a radius. Same shape, different arm's length.
    let r_inner = if straddle { (r_rim - reach).max(0.0) } else { r_rim };
    Some(Removal { ceiling: None,
        section: Section::Annulus { x, y, r_inner, r_outer: r_rim + reach },
        z_top: z_rim + drop,
        floor: Floor::Flat(z_rim - drop),
    })
}

/// How far above its tip a tool first reaches its full width — a ball's radius,
/// a flat tool's zero, a profile's top vertex.
fn tip_to_widest(tool: &Tool) -> f64 {
    let r = tool.dia / 2.0;
    if let Some(profile) = &tool.profile {
        return profile.iter().find(|pt| pt.x >= r - EPS).map_or(0.0, |pt| pt.y);
    }
    if tool.name == "Ballmill" {
        return r;
    }
    0.0
}

/// A standing surface of revolution: the same removal shape as a cavity — the
/// stock's top down to the declared surface, inside the feature's own footprint
/// — which is the whole reason one `Floor::Meridian` serves both orientations.
///
/// The difference is the middle. A cap that runs to an apex owns its axis, so
/// its footprint is a disc; a *band* stops at a top circle and says nothing
/// about the region inside it, so its footprint is an annulus and that region
/// stays untouched stock. `closes_on_axis` is the feature's own answer, not
/// something read off the samples: an allowance moves a sharp apex off the axis
/// by a hair (the offset of a corner is a sphere), and the inward clamp then
/// flattens that hair's spherical cap — a sub-allowance sliver, erring toward
/// removal, which is the quiet direction for every consumer.
/// What a `Clear` over a standing revolve actually took: the moat, as the
/// TERRACES a flat tool leaves in it (DECISIONS.md → "roughing a convex
/// revolve…", fork 4A).
///
/// Reporting the smooth offset surface instead would be the dangerous
/// direction. A Z-level rough leaves a staircase hugging the part from above,
/// and claiming the dome where the steps really are would read standing
/// material as air — which is exactly the question `check_moves` asks this
/// model before it lets the following finish pass plunge.
///
/// `None` for anything that is not a standing revolve, and for the `Finish`
/// that follows on the same feature (no `stepdown`), so both fall through to
/// the feature arms.
fn revolve_clear_removal(
    p: &Part,
    op: &Operation,
    feature: &Feature,
    tool: Option<&Tool>,
    skin: f64,
) -> Option<Vec<Removal>> {
    let (x, y, _) = crate::parse::derive_revolve_datum(feature)?;
    let tool = tool?;
    let stepdown = op.stepdown?;
    let stacks = crate::parse::derive_revolve_clear_stacks(feature, p.stock.z, stepdown, skin)?;
    // The pillar the emitter stands off, from the emitter's own home: the model
    // must not read its column as air, and the two cannot be allowed to size it
    // differently (DECISIONS.md → "the torus's annulus island…", fork 2).
    let pillar = stacks.pillar_r(tool.dia);
    let levels = stacks.moat;
    let first = levels.first()?;
    let rim = crate::parse::derive_revolve_rim_radius(feature, 0.0)? + skin;
    let r_outer = rim + crate::parse::derive_moat_width(p, op, feature, tool.dia);

    // The staircase, as a `(rho, z)` table: each level's floor holds from its
    // own island out to the NEXT level's island, and the last one out to the
    // moat's edge. The repeated rho is the vertical riser, which `interp_table`
    // reads as a step rather than a ramp — a ramp there would claim removal
    // across the riser's whole width, which is material still standing.
    //
    // The island is the emitter's, pillar and all: above a crown the stack says
    // nothing stands, and the pillar does.
    let island_at = |lv: &crate::parse::ClearLevel| lv.island_r.max(pillar);
    let mut samples = Vec::with_capacity(levels.len() * 2);
    for (i, level) in levels.iter().enumerate() {
        let inner = island_at(level);
        let outer = levels.get(i + 1).map_or(r_outer, island_at);
        samples.push((inner, level.z));
        samples.push((outer.max(inner), level.z));
    }

    // The hole, where the part crowns: the same staircase read in the mirror.
    // Each level's floor holds from its own ceiling INWARD to the next level's
    // ceiling — the hole narrows as it descends — and the deepest one holds in
    // to the pillar. Built outward from that floor so the table comes out
    // rho-ascending like every other meridian in this file.
    //
    // The riser convention lands on the other side here, and that is worth
    // stating: `interp_table` resolves a key landing exactly ON a repeated rho
    // to the LATER sample, which on the moat's outward staircase is the deeper
    // terrace ("the boundary belongs to the removal") and on this inward one is
    // the SHALLOWER. So a query exactly at a riser radius reads a hair less
    // removed than the pass really took — measure-zero, and the quiet direction
    // for every consumer, since claiming standing material as air is the only
    // dangerous way to be wrong.
    let mut hole_removal = None;
    if let Some(deepest) = stacks.hole.last() {
        let mut hole_samples = Vec::with_capacity(stacks.hole.len() * 2 + 2);
        for (i, level) in stacks.hole.iter().enumerate().rev() {
            let inner = stacks.hole.get(i + 1).map_or(pillar, |next| next.ceiling_r);
            hole_samples.push((inner.min(level.ceiling_r), level.z));
            hole_samples.push((level.ceiling_r, level.z));
        }
        let r_hole_outer = stacks.hole[0].ceiling_r;
        // From the stock's top, not from the crown: the moat's own above-crown
        // levels cleared this column on their way down (up there the hole and
        // the moat are one annulus), so everything over the staircase is gone.
        hole_removal = Some(Removal {
            ceiling: None,
            section: Section::Annulus { x, y, r_inner: pillar, r_outer: r_hole_outer },
            z_top: p.stock.z,
            floor: Floor::Meridian { x, y, samples: hole_samples },
        });
        debug_assert!(deepest.ceiling_r <= r_hole_outer + EPS, "the hole narrows downward");
    }

    // An `Annulus`, never a `Disc`: the column inside the topmost level's
    // island is the part's own cap plus whatever stock stands over it — or, in
    // a crowned part, the pillar — and this op did not touch a millimetre of it.
    Some(vec![Removal { ceiling: None,
        section: Section::Annulus { x, y, r_inner: island_at(first), r_outer },
        z_top: p.stock.z,
        floor: Floor::Meridian { x, y, samples },
    }]
    .into_iter()
    .chain(hole_removal)
    .collect())
}

/// The swept TUBE about a finish pass's centre path: the union of ball
/// positions along it, as a removal with a sampled floor AND ceiling envelope
/// (DECISIONS.md → "rim-area accounting…", fork 1A).
///
/// The centre path is the offset meridian at `skin + r_ball` — the same
/// `parse` home the clear levels and the main removal's floor read, priced at
/// the centre's own stand-off — so the tube and the toolpath cannot drift.
/// Its envelope is what the flat `rim_collar` under- and over-claimed at
/// once: beside a rim the swept shell rises to the `skin + dia` surface
/// (16.6mm above a 20mm sphere's equator, where the band claimed 3), and
/// below a stop-at-normal rim (paraboloid, catenoid) the band's flat box
/// claimed corners the last ball never reaches — standing material read as
/// air, the direction `check_moves` must never be handed.
///
/// Numeric on purpose. Per carrier the analytic boundary is "the offset at
/// `skin + dia` plus end-cap arcs about the last CL", but the end caps depend
/// on where each EMITTER stops, and nine carriers' worth of restated
/// end-geometry is drift waiting to happen. The envelope is computed from the
/// centre samples directly: per rho, the lowest and highest point of any ball
/// within reach. Piecewise-linear interpolation of those tables mis-states
/// the true envelope by at most the chord sagitta — sub-micron at this
/// density against a 3mm ball — and the sampling itself can only round the
/// tube's ends inward, the quiet direction.
///
/// Covers the WHOLE path, both ends: a band's top circle is the tube's other
/// end, not a special case (fork 2), which is what retires the
/// `climbs_outward_at_inner_rim` gate's collar-withholding half for every
/// carrier that comes this way (the torus keeps the gate and the old collar —
/// its hole is worked from inside on a different arm's length, and the
/// feature is out of the rim-area design's scope by name).
///
/// Generalised over the CUTTING FORM (Phase 10 lane B, slice 1, fork 3A): the
/// ball is the case where the form's floor is `r - sqrt(r² - d²)` about a
/// centre path and its ceiling the mirror; a profile tool's is its cutting
/// envelope about a TIP path and a flat ceiling at its cutting top. One
/// function for "what a solid of revolution sweeps along a meridian path",
/// so the ball's floor and ceiling reproduce byte for byte and the profile's
/// come from the same two loops.
fn swept_tube(x: f64, y: f64, path: &[(f64, f64)], form: CutForm) -> Option<Removal> {
    let reach = form.reach();
    if path.is_empty() || reach <= 0.0 {
        return None;
    }
    let rho_lo = path.iter().map(|c| c.0 - reach).fold(f64::MAX, f64::min).max(0.0);
    let rho_hi = path.iter().map(|c| c.0 + reach).fold(f64::MIN, f64::max);
    const TUBE_SAMPLES: usize = 512;
    let mut stations: Vec<f64> =
        (0..=TUBE_SAMPLES).map(|i| rho_lo + (rho_hi - rho_lo) * i as f64 / TUBE_SAMPLES as f64).collect();
    // A form with cliffs is stationed at each of them about every path point,
    // a hair to either side, so the table carries the wall as a wall. The
    // ball adds nothing here and keeps its uniform stations byte for byte.
    let cliffs = form.cliffs();
    if !cliffs.is_empty() {
        const HAIR: f64 = 1e-9;
        for c in path {
            for d in &cliffs {
                for edge in [c.0 - d, c.0 + d] {
                    for rho in [edge - HAIR, edge, edge + HAIR] {
                        if rho >= rho_lo && rho <= rho_hi {
                            stations.push(rho);
                        }
                    }
                }
            }
        }
        stations.sort_by(|a, b| a.partial_cmp(b).expect("finite stations"));
        stations.dedup();
    }
    let mut floor_s = Vec::with_capacity(stations.len());
    let mut ceil_s = Vec::with_capacity(stations.len());
    for rho in stations {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for c in path {
            if let Some((below, above)) = form.span(rho - c.0) {
                lo = lo.min(c.1 + below);
                hi = hi.max(c.1 + above);
            }
        }
        // A rho no ball reaches (a gap wider than the ball in the centre
        // table) contributes nothing; the envelope simply skips it and the
        // interpolation across the gap under-claims, which is the safe side.
        if lo <= hi {
            floor_s.push((rho, lo));
            ceil_s.push((rho, hi));
        }
    }
    if floor_s.len() < 2 {
        return None;
    }
    let z_top = ceil_s.iter().map(|s| s.1).fold(f64::MIN, f64::max);
    let section = if rho_lo <= EPS {
        Section::Disc { x, y, r: rho_hi }
    } else {
        Section::Annulus { x, y, r_inner: rho_lo, r_outer: rho_hi }
    };
    Some(Removal {
        section,
        z_top,
        floor: Floor::Meridian { x, y, samples: floor_s },
        ceiling: Some(Floor::Meridian { x, y, samples: ceil_s }),
    })
}

/// The cutting form a tube is swept with, in the meridian half-plane about
/// its own path point: what the tool occupies at plan distance `d` from the
/// coordinate the emitter wrote for it.
enum CutForm<'a> {
    /// A ball about its CENTRE (the surface-op emission convention): the
    /// sphere's own span, `±sqrt(r² - d²)`.
    Ball { r: f64 },
    /// A profile tool's cutting envelope about its TIP — `pts` as
    /// `(radius, height above tip)` sorted by height, the tip on the axis —
    /// from the envelope's height at `d` up to `cut_top`, flat over the
    /// widest radius: the form flares outward monotonically (the emitter
    /// rejects any other), so above its surface at `d` the body is at least
    /// as wide all the way to the cutting top, and above the cutting top it
    /// is shank and removes nothing (fork 4A — the truncated stamp's rule,
    /// so the analytic claim and the field's sweep agree by construction).
    Profile { pts: &'a [Pt], cut_top: f64 },
}

impl CutForm<'_> {
    /// The widest radius the form reaches. A profile's is widened by half an
    /// emitted ULP: the probes read the tool's position back off the emitted
    /// text, rounded, and a profile's edge is a WALL — 0.3µm outside the exact
    /// reach is the whole cutting top lower (measured: the contained twin's
    /// own `Z-0.286` ring rejected at its own wall, 0.0003mm past its exact
    /// radius). The ball needs none: its edge tapers to zero thickness, so a
    /// sub-micron excursion there is a sub-micron error in z.
    fn reach(&self) -> f64 {
        match self {
            CutForm::Ball { r } => *r,
            CutForm::Profile { pts, .. } => pts[pts.len() - 1].x + EMITTED_MM_HALF_ULP,
        }
    }

    /// Where the form's vertical span is DISCONTINUOUS in `d` — a profile's
    /// outer edge, where its wall stands `cut_top - z_form(reach)` tall. The
    /// ball has none: its span closes to zero at its edge. The tube stations
    /// these exactly (fork 9B, which the DRAFT rejected as unnecessary for the
    /// ball's smooth envelope and which a form with a wall needs): linear
    /// interpolation across a cliff reads the wall as a ramp one station wide,
    /// and at the tool's own ring that ramp is standing stock for the probe.
    fn cliffs(&self) -> Vec<f64> {
        match self {
            CutForm::Ball { .. } => Vec::new(),
            CutForm::Profile { .. } => vec![self.reach()],
        }
    }

    /// The vertical span `(below, above)` the form occupies at signed plan
    /// distance `d` from its path point, relative to that point's z; `None`
    /// where the form does not reach.
    fn span(&self, d: f64) -> Option<(f64, f64)> {
        match self {
            CutForm::Ball { r } => {
                if d.abs() <= *r {
                    let h = (r * r - d * d).sqrt();
                    Some((-h, h))
                } else {
                    None
                }
            }
            CutForm::Profile { pts, cut_top } => {
                let d = d.abs();
                if d > self.reach() {
                    return None;
                }
                Some((profile_height_at(pts, d.min(pts[pts.len() - 1].x)), *cut_top))
            }
        }
    }
}

/// The height above the tip at which a monotone-flaring profile first reaches
/// radius `d` — its cutting surface at that radius. A vertical segment is a
/// wall: the surface there is the wall's foot.
fn profile_height_at(pts: &[Pt], d: f64) -> f64 {
    if d <= pts[0].x {
        return pts[0].y;
    }
    for w in pts.windows(2) {
        if d <= w[1].x {
            let dx = w[1].x - w[0].x;
            return if dx <= EPS { w[0].y } else { w[0].y + (w[1].y - w[0].y) * (d - w[0].x) / dx };
        }
    }
    pts[pts.len() - 1].y
}

fn cap(
    x: f64,
    y: f64,
    z_top: f64,
    samples: Vec<(f64, f64)>,
    closes_on_axis: bool,
    tool: Option<&Tool>,
) -> Vec<Removal> {
    let floor = Floor::Meridian { x, y, samples };
    let (r_in, r_out) = (floor.inner_radius(), floor.outer_radius());
    let section = if closes_on_axis {
        Section::Disc { x, y, r: r_out }
    } else {
        Section::Annulus { x, y, r_inner: r_in, r_outer: r_out }
    };
    let mut out = Vec::new();
    out.extend(rim_collar(x, y, r_out, floor.z_at(x + r_out, y), tool, false, tool.map_or(0.0, |t| t.dia)));
    // A band's inner rim gets one too, but only where the tool has to work from
    // inside the middle — the same inward-facing test the probes use.
    if !closes_on_axis && floor.climbs_outward_at_inner_rim() {
        let inner_z = floor.z_at(x + r_in, y);
        // A torus's hole: the tool works it from inside, so this collar runs
        // inward from the rim exactly as the outer one runs outward.
        if let Some(mut collar) = rim_collar(x, y, r_in, inner_z, tool, true, tool.map_or(0.0, |t| t.dia)) {
            if let (Section::Annulus { r_outer, .. }, Some(_)) = (&mut collar.section, tool) {
                *r_outer = r_in;
            }
            out.push(collar);
        }
    }
    out.insert(0, Removal { ceiling: None, section, z_top, floor });
    out
}

// ---- first consumer: the tool body against material that OUTLIVES the op -----
//
// The rule (DECISIONS.md "Analytical op-history stock…"): a tool may cut
// whatever its own op removes, but no part of it may occupy material that is
// still there when the op finishes. Material the op removes is fair game;
// material it leaves — including the skin a roughing pass deliberately leaves —
// is not. That single comparison is what "the shoulder fouls even when the tip
// clears" means, and it needs no path, only the tool's own silhouette.

// How wide the tool is `h` above its tip is `parse::Silhouette`'s answer, not
// this file's — one home for what the tool IS, shared with the voxel stamp
// (DECISIONS.md → "the profile silhouette…", slice 1).

// Sampling of the probe. The tool body is a solid of revolution and the stock
// is a box with holes in it, so an exact intersection would be a small CSG
// engine — the thing this level explicitly is not. Instead the silhouette is
// sampled on a lattice, which can only ever MISS a foul thinner than the
// spacing, never invent one. A lint that stays quiet when unsure is the right
// failure direction; the hard version arrives with the voxel model.
const HEIGHT_SAMPLES: usize = 64;
const AZIMUTH_SAMPLES: usize = 24;
const AXIS_SAMPLES: usize = 24;

/// One place the tool axis provably has to be, how low its tip provably gets
/// there, and **what that place is about** — the declared feature whose
/// geometry forces it. For every op but `Drill` the subject is the op's own
/// feature; a `Drill` works a LIST, so each of its probes names the hole it
/// stands in (Phase 9 slice 0, `DECISIONS.md` → "the per-point reach
/// finding…", the two prose voices it left standing). A finding raised from a
/// probe is ABOUT the probe's subject, never about `op.feature_name` — which
/// for a drill is empty, and was read as the subject for as long as the probe
/// was a bare tuple.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    /// The axis position, and how far off it the tool may stand.
    pub x: f64,
    pub y: f64,
    pub axis_r: f64,
    /// How low the tip gets.
    pub tip_z: f64,
    /// The declared feature this probe is about.
    pub subject: String,
}

/// Where the tool axis provably has to be, and how low its tip provably gets,
/// for the op to machine the feature at all. One entry per such place: a band
/// can force the tool to two of them.
///
/// Every one is stated as the LEAST committal position — the axis no further
/// out (or in) and the tip no lower than the geometry forces — because each is a
/// claim the tool *cannot avoid*, and a claim it could avoid is a false alarm.
/// The positions are read off the removal the same op declares rather than
/// re-derived per feature, so a probe cannot drift from the geometry it probes.
///
/// The op's own allowance moves them with it: a roughing pass rides its offset
/// surface, never the nominal one, so reading the nominal wall would accuse
/// every roughing op of fouling the skin it was asked to leave.
///
/// **Ops with no probe, named rather than quietly skipped.** `Floor` is
/// provably inert — its removal is everything above the plane across the whole
/// stock, so nothing it leaves can be above its own tip. `Clear` on a polyline
/// pocket would need the pocket's maximum inscribed circle to place the axis
/// (an inward offset, and `cavalier_contours`' answer can split or vanish),
/// and a cylindrical body cannot foul a vertical wall it just cut — expensive
/// and inert at once. The standing prismatic trio and `Mesh` have no closed-form
/// removal at all, so `complete` already silences them upstream of here.
fn probes(p: &Part, op: &Operation, tool: &Tool) -> Vec<Probe> {
    // No probe where the model does not know the skin — a probe placed against a
    // guessed surface is a guess with a warning attached.
    let Some(skin) = crate::parse::derive_emitted_skin(p, op) else {
        return Vec::new();
    };

    // Drilling IS the plunge-entry case: tip at the bottom of each hole, axis on
    // that hole's own centre. The mouth-cone arm's tip stops at the derived
    // seat Z, not the hole's floor — read from the same home the emitter
    // reads, or this advisory would accuse the countersink of a plunge it
    // never makes.
    if op.at_points() {
        let names = match &op.drill_points {
            Some(n) => n,
            None => return Vec::new(),
        };
        let cone = tool.ground_cone();
        return names
            .iter()
            .filter_map(|name| match p.features.get(name) {
                Some(Feature::CylinderBore { extent, x, y, z, csink, cbore, .. }) => {
                    let rim = derive_contour_top(p.stock.z, *z);
                    let z_tip = match (&cone, csink) {
                        (Some(cone), Some(spec)) => {
                            crate::parse::derive_csink_tip_z(rim, spec.seat_dia(), cone)
                        }
                        // The counterbore arm's tip stops at the declared seat
                        // floor — read from the same home the emitter reads,
                        // the mouth-cone lesson applied before it can recur.
                        _ => match cbore {
                            Some(cb) if tool.name == "Endmill" => derive_z_cut(rim, cb.depth),
                            _ => derive_z_cut(rim, *extent),
                        },
                    };
                    Some(Probe { x: *x, y: *y, axis_r: 0.0, tip_z: z_tip, subject: name.clone() })
                }
                _ => None,
            })
            .collect();
    }

    let feature = match p.features.get(&op.feature_name) {
        Some(f) => f,
        None => return Vec::new(),
    };
    match feature {
        // Finishing a bore wall means putting the tool against it: the axis sits
        // one tool radius in from the wall, tip on the floor. On the ellipse the
        // ring rides the minor semi-axis, where it touches the offset path
        // exactly; everywhere else it sits deeper inside the void, which can
        // only under-claim.
        Feature::CylinderBore { dia, extent, x, y, z, .. } => vec![Probe {
            x: *x,
            y: *y,
            axis_r: (dia / 2.0 - skin - tool.dia / 2.0).max(0.0),
            tip_z: derive_z_cut(derive_contour_top(p.stock.z, *z), *extent),
            subject: op.feature_name.clone(),
        }],
        // A revolve-boss `Clear` has no place here, and the reason is about
        // what this probe MEANS: "the tip is on the surface, the rest of the
        // tool is not in air". It puts the tool against a declared surface at
        // its rim. A roughing moat's outer edge is not a declared surface — it
        // is where the removal stops — so the probe would stand the tool a full
        // radius out in the pedestal and report a foul the emitter never makes
        // (its outermost pass is a radius further in). The op's real contact is
        // the ISLAND, which the family stands off by construction and which
        // `check_moves` verifies against the emitted text rather than against a
        // guessed position.
        _ if op.op_type == OpType::Clear && crate::parse::derive_revolve_datum(feature).is_some() => {
            Vec::new()
        }
        _ => match revolved_axis(feature) {
            None => Vec::new(),
            Some((cx, cy)) => {
                // The FIRST removal is the surface itself; anything after it is
                // the rim collar, which is not what a probe reads.
                let removal = match removals_for(p, op).and_then(|rs| rs.into_iter().next()) {
                    Some(r) => r,
                    None => return Vec::new(),
                };
                revolved_probes(cx, cy, &removal, is_cavity(feature), tool)
                    .into_iter()
                    .map(|(x, y, axis_r, tip_z)| Probe { x, y, axis_r, tip_z, subject: op.feature_name.clone() })
                    .collect()
            }
        },
    }
}

/// The centre of a surface of revolution, or None if the feature is not one.
fn revolved_axis(feature: &Feature) -> Option<(f64, f64)> {
    match feature {
        Feature::Sphere { x, y, .. }
        | Feature::Cone { x, y, .. }
        | Feature::Torus { x, y, .. }
        | Feature::SplineRevolve { x, y, .. }
        | Feature::SplineRevolveFunnel { x, y, .. } => Some((*x, *y)),
        _ => None,
    }
}

fn is_cavity(feature: &Feature) -> bool {
    matches!(
        feature,
            Feature::SplineRevolveFunnel { .. }
    )
}

/// Where a surface of revolution forces the tool, read off its own removal.
///
/// A **cavity** is entered, so the binding place is its deepest circle with the
/// axis on the centreline — the most favourable spot the tool can be while
/// reaching it, furthest from every wall at once. A **cap** is walked around, so
/// the binding place is its base rim, with the axis no further out than the rim
/// itself. Both are the same question — "can the rest of the tool follow the
/// tip to the deepest declared point" — asked of the two geometries, and both
/// are independent of `concave_gouge_check`, which asks whether the cutting end
/// fits the curvature rather than whether the tool fits the space.
///
/// A band gets a second probe at its INNER rim, but only when the meridian
/// climbs going outward there — that is what makes the inner rim the bottom of
/// an inward-facing bowl (a torus's hole), where the tool has to work from
/// inside the middle column. Where the meridian falls away outward instead (a
/// catenoid waist, a vase neck, a pseudosphere tip: truncations, not rims) the
/// surface faces out, the tool never enters the middle, and a probe there would
/// claim a foul the tool avoids.
fn revolved_probes(cx: f64, cy: f64, removal: &Removal, cavity: bool, tool: &Tool) -> Vec<(f64, f64, f64, f64)> {
    if cavity {
        return vec![(cx, cy, 0.0, removal.floor.z_at(cx, cy))];
    }
    // The axis stands a tool RADIUS clear of the rim, not on it. That is where
    // the tool actually is when its tip touches the edge — the same sentence
    // `rim_collar` is built on ("a ball contacting a rim whose tangent is
    // vertical has its centre a radius beyond the rim") — and putting the axis
    // ON the rim instead reaches a radius INBOARD of it, into the part's own
    // body, which no tangent tool can ever occupy.
    //
    // That misplacement was unreachable until 2026-08-12: every convex-revolve
    // fixture declared a stock too short to hold its own part, so there was no
    // material beside a rim for the probe to find and it reported nothing
    // wherever it stood. The first stock that CONTAINED its part turned it into
    // a foul warning on all 25 of them at once, always just inside the rim and
    // just above the base plane (DECISIONS.md → "roughing a convex revolve…",
    // slice 2). Offsetting outward is also the conservative direction for what
    // this lint is FOR: a wide-bodied tool reaching out over the pedestal is
    // probed further out, not less.
    let r_tool = tool.dia / 2.0;
    let r_out = removal.floor.outer_radius();
    let mut out = vec![(cx, cy, r_out + r_tool, removal.floor.z_at(cx + r_out, cy))];
    if let Section::Annulus { r_inner, .. } = removal.section {
        if removal.floor.climbs_outward_at_inner_rim() {
            // A band's hole is worked from inside, so its clearance is inward.
            out.push((cx, cy, (r_inner - r_tool).max(0.0), removal.floor.z_at(cx + r_inner, cy)));
        }
    }
    out
}

/// Advisory warnings for tool bodies that reach into material the op leaves
/// behind. Called from `parse::lint` — advisory, on stderr, so no golden moves.
pub fn body_foul_warnings(p: &Part) -> Vec<crate::parse::Finding> {
    let mut warnings = Vec::new();
    // Read once: the fact is about the declaration, not about any op.
    let uncontained = uncontained(p);
    for (k, op) in p.operations.iter().enumerate() {
        let tool = match p.tools.get(&op.tool_num) {
            Some(t) => t,
            None => continue,
        };
        let places = probes(p, op, tool);
        if places.is_empty() {
            // No probe, no claim — and no decline either: the probe roster
            // says which ops it reads, and an op it never reads was never
            // handed a clean bill.
            continue;
        }
        let sil = Silhouette::of(tool);
        // Everything this op leaves standing, including its own allowance.
        let remaining = remaining_after(p, k);
        // The decline (Phase 9 lane B, fork 3A): a probe that would have read
        // this op but cannot trust the model says so instead of going quiet —
        // and a probe that read it, found nothing, and was reading a model
        // missing the very feature it probed says that too. A foul found on
        // an uncontained feature still stands: the stock it did find is real.
        let untrusted = untrusted_for(p, k, &uncontained);
        if !remaining.complete {
            if let Some(u) = untrusted {
                warnings.push(crate::parse::Finding::op(crate::parse::FindingFamily::Decline, k, &u.feature, format!(
                    "body-foul probe declined on '{}' ({} with T{}): {} — a foul read off an over-statement could accuse the tool of standing in air, so the probe did not read this op. Fix the named op to restore it",
                    u.feature, op_word(op), op.tool_num, u.reason
                )));
            }
            continue;
        }
        let mut fouled = false;
        for probe in places {
            // Only as far up as the stock goes: above the stock top there is
            // nothing to foul on.
            let reach = p.stock.z - probe.tip_z;
            if reach <= 0.0 {
                continue;
            }
            let (h, rb, px, py, pz) =
                match first_foul(&remaining, &sil, probe.x, probe.y, probe.axis_r, probe.tip_z, reach) {
                    Some(w) => w,
                    None => continue,
                };
            // ABOUT the probe's subject — the hole, for a drill — not the op's
            // feature name, which a drill does not have.
            let tip_x = probe.x + probe.axis_r * (py - probe.y).patan2(px - probe.x).pcos();
            let tip_y = probe.y + probe.axis_r * (py - probe.y).patan2(px - probe.x).psin();
            warnings.push(crate::parse::Finding::op(crate::parse::FindingFamily::BodyFoul, k, &probe.subject, format!(
                "T{}'s body {:.3}mm above the tip sweeps to r {:.3}mm and reaches material that outlives '{}' at ({:.3}, {:.3}, {:.3}) — the tip is on the surface, the rest of the tool is not in air; remove that stock in an earlier op, or declare the stock to what is actually on the machine — the tip at ({:.3}, {:.3}, {:.3})",
                op.tool_num, h, rb, probe.subject, px, py, pz, tip_x, tip_y, probe.tip_z
            )).with_place([px, py, pz]).with_tip([tip_x, tip_y, probe.tip_z], op.tool_num));
            // One foul per op is the actionable message; a second probe on the
            // same op would be the same stock, restated from another angle.
            fouled = true;
            break;
        }
        if !fouled {
            if let Some(u) = untrusted {
                warnings.push(crate::parse::Finding::op(crate::parse::FindingFamily::Decline, k, &u.feature, format!(
                    "body-foul probe declined on '{}' ({} with T{}): {} — the quiet it read here is not a claim: the body may stand in material the model never had. Declare the stock the part is actually made from, or move the feature's datum into it",
                    u.feature, op_word(op), op.tool_num, u.reason
                )));
            }
        }
    }
    warnings
}

/// How far above or below the stock a declared point must sit before the
/// containment lint speaks. A coincidence guard and nothing else: a bore's
/// mouth ring lands ON `stock.z` by construction (`derive_contour_top` copies
/// it), so exact equality has to read as inside. It is deliberately not a
/// machining tolerance — the overhang is reported as a number and the reader
/// judges whether it matters. The PLAN faces take a different guard, the
/// region's `ON_THE_BLANK` (Phase 47 slice 1, fork 2A): an authored loop
/// against the blank's edge is a declaration against a declaration, three
/// decimals, and a vertex drawn on a round to three decimals (the real
/// part's rim ribs: 0.0005 past the exact circle) is ON it, as the region
/// would read the edge it ends.
const CONTAINMENT_EPS: f64 = 1e-6;

/// Declared features standing outside the stock (advisory).
///
/// The stock is the material the program starts from, so a feature reaching
/// outside it cannot be cut out of it. Nothing REJECTS on this: a feature
/// legitimately stands proud while the `run` list is still being written, which
/// is exactly what the outline's "declared, never run" tail is for, and the
/// window's first-frame fit folds `wires_aabb` in for the same reason
/// (DECISIONS.md "Phase 6, slice 2…" — "there is no global containment
/// assertion"). What that decision settled was that containment cannot be a
/// GATE. It is still worth saying out loud, because the rest of the toolchain
/// quietly assumes it: `removals_for`'s dome arm models "the footprint, from
/// the stock top down to the dome", which goes silently inert when the stock
/// top IS the dome's own equator — the model and the declaration hold opposite
/// beliefs and neither notices.
///
/// **The certificate, and why it can only under-claim.** Every point
/// `view::feature_wires` emits is evaluated ON the declared feature (the
/// nominal `parse::*_cl_point` homes at `r_off = 0` — never restated math), so
/// a wire point outside the stock PROVES the feature leaves it. The converse
/// does not hold: wire density is a drawing choice (`WIRE_MERIDIANS` and
/// friends are presentation, not a precision claim) and a flattened circle is
/// inscribed, so the hull is an INNER bound — a feature can leave the stock
/// between samples and go unreported. This lint therefore misses quietly and
/// never cries wolf, which is the safe direction for an advisory, and it is why
/// the check borrows the drawing's one exhaustive per-variant home instead of
/// standing up a second one that would have to be kept in sync with it.
///
/// The price of that borrowing, stated rather than discovered: every `nacs` run
/// now builds the wires, which the plain CLI path did not before. Linear in the
/// declaration and unmeasurable on every fixture here, but `Feature::Mesh`'s arm
/// walks its triangles through a `HashSet`, so a million-triangle STL pays that
/// walk to emit a lint about its bounding box. If that ever bites, the fix is a
/// cheap `Mesh` arm here (vertex extremes are already the exact answer for a
/// mesh), not a second geometry home for the other twenty-four variants.
pub fn containment_findings(p: &Part) -> Vec<crate::parse::Finding> {
    uncontained(p)
        .into_iter()
        .map(|u| {
            let phrases: Vec<String> = u.faces.into_iter().map(|(_, s)| s).collect();
            crate::parse::Finding::feature(crate::parse::FindingFamily::Containment, &u.name, format!(
                "'{}' ({}) stands outside the stock: {} — the stock is the material the program starts from, so no pass can cut this surface out of it. Declare the stock the part is actually made from, or move the feature's datum into it. (Read from the DECLARATION alone: the run list is not consulted, so a feature nothing machines yet reads the same as one every op touches)",
                u.name,
                u.kind,
                phrases.join(", "),
            ))
        })
        .collect()
}

/// A declared feature standing outside the stock: the fact the containment
/// lint renders, and the fact the stock model's consumers decline on.
///
/// One derivation, two renderings (Phase 9 lane B, fork 1A): the lint above
/// used to compute this straight into a finding's prose, and a consumer that
/// needed to know whether the feature IT works is contained would have had to
/// parse names back out of sentences. `faces` is worst-first, each a phrase
/// the lint prints; `faces[0].1` is the one-line reason a decline quotes.
#[derive(Clone, Debug, PartialEq)]
pub struct Uncontained {
    pub name: String,
    pub kind: String,
    pub faces: Vec<(f64, String)>,
}

/// Every declared feature the stock does not contain, read from the
/// declaration alone — the certificate `containment_findings` documents, with
/// the same under-claim (the hull is the drawing's, an inner bound) and the
/// same coincidence guard.
pub fn uncontained(p: &Part) -> Vec<Uncontained> {
    let stock = crate::view::stock_aabb(&p.stock);
    // A stock with no volume is a project that has not declared one yet; every
    // feature would trip, and none of those findings would be about geometry.
    // `h` is the reachable case — `validate` already rejects a non-positive w
    // or l, and this does not restate that rule so much as decline to depend on
    // which side of it the lint runs.
    if p.stock.w <= 0.0 || p.stock.l <= 0.0 || p.stock.h <= 0.0 {
        return Vec::new();
    }
    let mut out_all = Vec::new();
    for wire in crate::view::feature_edges(p) {
        // The hull of points certainly ON the feature. Coordinate-wise
        // comparison only: each extreme of this box is achieved by a real
        // declared point, which is what makes an axis finding a proof. The box
        // as a SOLID is not claimed — its corners are air on any dome.
        let inf = f64::INFINITY;
        let bb =
            crate::view::edges_aabb(std::slice::from_ref(&wire), [inf, inf, inf, -inf, -inf, -inf]);
        if !bb.iter().all(|v| v.is_finite()) {
            continue; // a variant that draws nothing pins nothing
        }
        // (overhang, phrase) per face, worst first — one warning per feature
        // carries every face it leaves, because "raise the stock" and "widen
        // the stock" are different fixes and a reader needs both.
        let mut faces = vec![
            (bb[5] - stock[5], format!("{:.3}mm above the stock top z {:.3}", bb[5] - stock[5], stock[5])),
            (stock[2] - bb[2], format!("{:.3}mm below the stock's bottom z {:.3}", stock[2] - bb[2], stock[2])),
        ];
        // The plan faces are the shape's (Phase 46 slice 1): a box has four,
        // each an axis extreme a declared point achieves; a round has one,
        // the farthest declared point's reach past the radius — still a
        // proof by a point the author drew, never an estimate. The round is
        // measured as the ROUND, not as `derive_blank`'s chords (Phase 47
        // slice 1, the DRAFT's measure corrected on the way in): the chorded
        // footprint is inscribed, so a vertex ON the circle reads up to the
        // chord tolerance (0.010) outside it — ten times the guard below —
        // and the stock model's own `material_at` reads the exact radius.
        match p.stock.shape {
            crate::parse::StockShape::Box => faces.extend([
                (stock[0] - bb[0], format!("{:.3}mm past the stock's -x face at x {:.3}", stock[0] - bb[0], stock[0])),
                (bb[3] - stock[3], format!("{:.3}mm past the stock's +x face at x {:.3}", bb[3] - stock[3], stock[3])),
                (stock[1] - bb[1], format!("{:.3}mm past the stock's -y face at y {:.3}", stock[1] - bb[1], stock[1])),
                (bb[4] - stock[4], format!("{:.3}mm past the stock's +y face at y {:.3}", bb[4] - stock[4], stock[4])),
            ]),
            // A profile blank's outline (Phase 54 slice 5a): the farthest a
            // wire point stands OUTSIDE the loop, by the polygon distance.
            crate::parse::StockShape::Profile { ref points } => {
                let reach = wire
                    .polylines
                    .iter()
                    .flatten()
                    .filter(|q| !point_in_polygon(points, q[0], q[1]))
                    .map(|q| dist_to_polygon(points, q[0], q[1]))
                    .fold(0.0, f64::max);
                faces.push((reach, format!("{:.3}mm past the stock's outline of {} points", reach, points.len())));
            }
            crate::parse::StockShape::Cylinder { dia } => {
                let (cx, cy) = p.stock.axis();
                let reach = wire.polylines.iter().flatten().map(|q| (q[0] - cx).phypot(q[1] - cy)).fold(0.0, f64::max);
                faces.push((reach - dia / 2.0, format!("{:.3}mm past the stock's round of dia {:.3} about ({:.3}, {:.3})", reach - dia / 2.0, dia, cx, cy)));
            }
        }
        // A VOID past the blank in plan is air declared air, not a finding
        // (Phase 27, fork 2A): a step is a pocket whose outline runs off the
        // stock, and nothing out there needs cutting. Its Z faces still speak
        // — a pocket above the stock's top or through its bottom is a
        // different mistake — and a BOSS past the blank still speaks on every
        // face, because that is material the stock does not have. A `Face`
        // is the plan's void too (Phase 47 slice 1, fork 2A): its outline is
        // the part's silhouette, drawn a hair past the blank for the region
        // to clip (Phase 46's recognizer writes it two chord tolerances out),
        // and what it works past the blank is air by the same rule.
        let plan_void = matches!(wire.kind, "ProfilePocket" | "CylinderBore" | "Face");
        let mut out: Vec<(f64, String)> = faces
            .into_iter()
            .enumerate()
            .filter(|(i, (over, _))| {
                let plan = *i >= 2;
                let guard = if plan { crate::region::ON_THE_BLANK } else { CONTAINMENT_EPS };
                *over > guard && !(plan_void && plan)
            })
            .map(|(_, face)| face)
            .collect();
        if out.is_empty() {
            continue;
        }
        out.sort_by(|a, b| b.0.total_cmp(&a.0));
        out_all.push(Uncontained { name: wire.name.clone(), kind: wire.kind.to_string(), faces: out });
    }
    out_all
}

/// The one word an op's kind prints as in an error or a decline.
/// What a CONCESSION leaves standing (Phase 32 slice 2): the unreachable
/// stretches' outlines, held back from the op's removal so that no later op's
/// entry check believes a slot this one never entered is clear. Empty for an
/// op that conceded nothing. A rest op (Phase 33) concedes piece by piece:
/// what each of its floors held back.
pub fn conceded_holes(p: &Part, op: &Operation) -> Vec<Vec<Pt>> {
    if op.cover.is_none() {
        return Vec::new();
    }
    if crate::rest::is_rest(op) {
        let Ok(pieces) = crate::rest::pieces(p, op) else { return Vec::new() };
        return crate::rest::held_back(&pieces);
    }
    let (Some(region), Some(tool)) = (crate::region::of(p, op), p.tools.get(&op.tool_num)) else { return Vec::new() };
    let skin = if matches!(p.features.get(&op.feature_name), Some(Feature::ProfilePocket { .. } | Feature::Plane { .. } | Feature::Face { .. })) && op.op_type == crate::parse::OpType::Clear { op.stock_to_leave.unwrap_or(0.0) } else { 0.0 };
    crate::ops::clear::coverage(p, op, &region, tool.dia / 2.0, skin).map(|c| c.stretches.into_iter().map(|s| s.outline).collect()).unwrap_or_default()
}

/// How far past an OPEN edge a pattern's own passes GUARANTEE to sweep, in
/// the plan — the claim's size (Phase 32 slice 3, fork D). The offset family,
/// the zigzag and the oneway fit their passes to the allowed region, whose
/// outermost pass rides a radius outside the floor, so the tool's edge reaches
/// a DIAMETER past it, always.
///
/// **`spine` guarantees NOTHING past the edge**, and measuring said so: its rows
/// own the floor, the comb's phase is chosen per part, and its outermost row
/// lands where that phase puts it — on `lawn_fence_bed_spine` two and a half
/// millimetres inside the edge, so a claim of one radius left a 3mm strip of
/// scrap claimed and unswept. A rail pass down a rail narrower than the tool
/// reaches less still. What spine guarantees is the FLOOR, which the `uncut`
/// column holds it to; beyond the floor it claims nothing, and the overhang
/// it is permitted is `face_overhang`'s, not this.
/// Where a face op's tool can reach at all: every point some tool position in
/// the family's allowed region covers — that region (the permitted swath less
/// its islands, eroded by a radius: the emitter's own `allowed_loops`) grown
/// back out by the radius. A morphological OPENING of the swath, in closed
/// form from the offsetter: what it leaves out is exactly what no pass can
/// enter — an inside corner's fillet, a nook past a wall chain's end, a strip
/// behind a wall thinner than the push. Each piece comes with its own holes.
/// `None` where the family refuses.
fn reachable(p: &Part, op: &Operation, region: &crate::region::Region, tool: &Tool, skin: f64) -> Option<Vec<(Vec<Pt>, Vec<Vec<Pt>>)>> {
    let r = tool.dia / 2.0;
    let (ccw, cw) = crate::ops::clear::allowed_loops(p, op, region, r, skin).ok()?;
    let mut out = Vec::new();
    for piece in &ccw {
        let inside: Vec<crate::offset::Island> = cw
            .iter()
            .filter(|h| h.first().is_some_and(|q| point_in_polygon(piece, q.x, q.y)))
            .map(|h| crate::offset::Island::Loop(h.clone()))
            .collect();
        let grown = crate::offset::offset_shape(piece, &inside, -r);
        let holes: Vec<Vec<Pt>> = grown.cw.iter().map(|l| crate::offset::flatten_loop(l)).collect();
        for l in &grown.ccw {
            out.push((crate::offset::flatten_loop(l), holes.clone()));
        }
    }
    Some(out)
}

/// A floor claim as the tool REACHES it (2026-11-10): `points` less its
/// holes, clipped to `reachable`'s pieces, so an inside corner's fillet, a
/// nook and a web thinner than the tool are stock the model holds — where
/// the voxel field finds them, and where until now the air column, the
/// immersion map and the cuts-nothing check read air (the over-claim in
/// ROADMAP.md's debts). Whole where the op works no region, has no tool,
/// or the family refuses.
fn reached(p: &Part, op: &Operation, tool: Option<&Tool>, skin: f64, points: Vec<Pt>, holes: Vec<Vec<Pt>>, make: &dyn Fn(Vec<Pt>, Vec<Vec<Pt>>) -> Removal) -> Vec<Removal> {
    let reach = crate::region::of(p, op).zip(tool).and_then(|(region, tool)| reachable(p, op, &region, tool, skin));
    match reach {
        Some(reach) => clip_reached(&reach, points, holes, make),
        None => vec![make(points, holes)],
    }
}

/// `points` clipped to each reached piece. A piece's own hole is what the
/// tool leaves round a standing thing — the island grown by the allowance,
/// its notches filleted — so it stands in for the plain hole it covers;
/// every other hole is kept. The whole claim where nothing clips.
fn clip_reached(reach: &[(Vec<Pt>, Vec<Vec<Pt>>)], points: Vec<Pt>, holes: Vec<Vec<Pt>>, make: &dyn Fn(Vec<Pt>, Vec<Vec<Pt>>) -> Removal) -> Vec<Removal> {
    let mut out = Vec::new();
    for (outer, reach_holes) in reach {
        for piece in crate::offset::clip_to(outer, &points) {
            let mut hs = holes.clone();
            for rh in reach_holes {
                // Covered: all but a sliver of one hole lies inside the
                // other; the larger stands. (Not vertex by vertex — a corner
                // comes back from the offsetter's round trip a sagitta short.)
                let covers = |outer: &[Pt], inner: &[Pt]| crate::offset::clip_to(inner, outer).iter().map(|c| plan_area(c)).sum::<f64>() >= 0.99 * plan_area(inner);
                let same = |h: &[Pt]| covers(h, rh) || covers(rh, h);
                // An island's notch fillet is under a hundredth of the
                // island, so of two that match the LARGER stands, by area.
                if hs.iter().any(|h| same(h) && plan_area(h) >= plan_area(rh)) {
                    continue;
                }
                hs.retain(|h| !same(h));
                hs.push(rh.clone());
            }
            out.push(make(piece, hs));
        }
    }
    if out.is_empty() {
        out.push(make(points, holes));
    }
    out
}

/// A closed outline's area in plan, unsigned.
fn plan_area(pts: &[Pt]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum::<f64>().abs() / 2.0
}

/// How far past an OPEN edge a pattern's passes GUARANTEE to sweep — the
/// claim's push (Phase 32 slice 3, fork D). A DIAMETER for the offset family,
/// whose outermost loop rides a radius outside the floor all the way round.
/// NOTHING for a pattern that runs rows (`rows_only`): spine since Phase 32
/// slice 3, zigzag and oneway since Phase 40 slice 0. Their rows do end a
/// radius past the edge, but between two row ends there is a crescent the
/// disc never reaches (two 5mm discs five apart leave 0.67mm at the midline;
/// the zigzag's links sweep every other one, the oneway's none) and at an
/// open corner a nook. Until Phase 40 the parallel patterns' closing LAP of
/// the whole level swept those for them, which is why a diameter held; the
/// lap is gone (Phase 40 slice 1) and what the rows guarantee is the FLOOR,
/// which the `uncut` column holds them to — so that is what they claim.
fn pattern_push(p: &Part, op: &Operation, tool: &Tool) -> f64 {
    if rows_only(p, op) { 0.0 } else { tool.dia }
}

/// The patterns that cut a floor in ROWS and close along what stands, rather
/// than in loops parallel to every edge: what they sweep past an open edge is
/// not a promise, so the claim stops at the floor and the open void is the
/// void as DECLARED (Phase 32 slice 4 for spine; Phase 40 slice 0 for the
/// parallel passes).
fn rows_only(p: &Part, op: &Operation) -> bool {
    matches!(
        crate::parse::derive_pattern(p, op),
        Some(crate::parse::Pattern::Spine | crate::parse::Pattern::Zigzag | crate::parse::Pattern::Oneway | crate::parse::Pattern::Held)
    )
}

/// The op's OVERHANG ALLOWANCE: where this op's own tool may sweep beyond the
/// floor it cuts, which is one tool DIAMETER past every open edge — Phase
/// 29's sentence ("a face's job is to overhang scrap"), which until Phase 32
/// slice 3 was enforced by claiming that scrap removed. It is the swath the
/// removal used to be, clip and all, so a declared neighbour beyond an edge is
/// still not this op's to touch — nor one the swath closed over whole
/// (Phase 32, after the close), which comes back as a hole. `None` for an op that works no
/// floor. A rest op (Phase 33) is allowed each of its floors' own.
fn face_overhang(p: &Part, op: &Operation) -> Vec<Overhang> {
    if crate::rest::is_rest(op) {
        let pieces = crate::rest::pieces(p, op).unwrap_or_default();
        return pieces.iter().filter(|pc| pc.is_cut()).flat_map(|pc| face_overhang(&pc.part, &pc.op)).collect();
    }
    one_overhang(p, op).into_iter().collect()
}

fn one_overhang(p: &Part, op: &Operation) -> Option<Overhang> {
    let tool = p.tools.get(&op.tool_num)?;
    if !matches!(p.features.get(&op.feature_name), Some(Feature::Face { .. }) | Some(Feature::Plane { .. }) | Some(Feature::ProfilePocket { .. })) {
        return None;
    }
    let region = crate::region::of(p, op)?;
    if region.open_edges() == 0 {
        return None;
    }
    let swath = region.family_wall(tool.dia)?;
    let wall = crate::region::clipped_outline(p, &swath.wall).unwrap_or(swath.wall);
    Some(Overhang { wall, holes: swath.enclosed, floor_z: region.z })
}

/// Where an op may overhang: inside `wall`, outside every hole, at or above
/// its floor.
struct Overhang {
    wall: Vec<Pt>,
    holes: Vec<Vec<Pt>>,
    floor_z: f64,
}

impl Overhang {
    /// …or ON its edge, to `MOTION_SLACK` (Phase 35 slice 3). A row of spine
    /// ends a radius past the floor's open edge, so the tool's own edge lies
    /// exactly on the swath's — a diameter past. That is tangency, which the
    /// material side of this test has always let pass; the permission side
    /// asked a point-in-polygon of a point ON the polygon, and a plunge there
    /// was refused wherever the blank ran on past the swath.
    fn permits(&self, x: f64, y: f64, z: f64) -> bool {
        let within = point_in_polygon(&self.wall, x, y) || dist_to_polygon(&self.wall, x, y) <= MOTION_SLACK;
        z >= self.floor_z - Z_AGREE && within && !self.holes.iter().any(|h| point_in_polygon(h, x, y))
    }
}

pub(crate) fn op_word(op: &Operation) -> &'static str {
    op.op_type.verb()
}

/// Why a stock-model consumer cannot answer for this op — or `None` when the
/// model is trustworthy here and a clean bill IS a claim.
///
/// **Two kinds of untrustworthiness, opposite in sign** (`DECISIONS.md` →
/// "Phase 9 lane B — containment declines by name…"). `complete == false`
/// means an earlier op has no closed-form removal, so the model OVER-states
/// material: a finding could accuse a tool of standing in air, and the
/// consumers have gone silent on it since 2026-07-29. An uncontained feature
/// the op works means the model UNDER-states material — the part is there and
/// the stock is not — so a finding is still true but a clean bill is not.
/// Either way the consumer's silence was never a claim, and this is the one
/// place that says so, per OP (fork 2A): the lie is local to the feature the
/// stock is missing, and a `Clear` at the other end of the stock is judged
/// against real material. The subject is the feature the decline is about —
/// for a drill the first listed hole, through `subjects` — never the op's
/// bare `feature_name`.
pub struct Untrusted {
    /// The declared feature the decline is about.
    pub feature: String,
    /// The reason, as a clause a consumer's sentence can carry.
    pub reason: String,
    /// True when the model over-states (incomplete); false when it under-states
    /// (uncontained). The consumers phrase their declines by this sign.
    pub over_states: bool,
}

pub fn untrusted_for(p: &Part, op_idx: usize, uncontained: &[Uncontained]) -> Option<Untrusted> {
    let op = &p.operations[op_idx];
    if !remaining_after(p, op_idx).complete {
        let own = subjects(p, op).first().map_or(op.feature_name.clone(), |s| s.name.clone());
        // What `given from` could not carry is missing at prefix 0, so it is
        // missing before every op (Phase 48 slice 1).
        let mut missing: Vec<String> = given_from_declines(p);
        missing.extend(
            incomplete_ops(p)
                .into_iter()
                .filter(|(k, _)| *k <= op_idx)
                .map(|(k, f)| incomplete_reason(p, k, &f)),
        );
        return Some(Untrusted {
            feature: own,
            reason: format!("{}, so the model over-states material", missing.join("; ")),
            over_states: true,
        });
    }
    uncontained_for(p, op_idx, uncontained)
}

/// The uncontained half of `untrusted_for` on its own: the feature this op
/// works that the stock does not contain, or `None`. Split out in slice 1 for
/// the field voice, which runs ONLY where the analytic model is incomplete —
/// so asked through `untrusted_for` it would always hear the incomplete
/// reason, which is its charter to read past, never a reason to decline.
pub fn uncontained_for(p: &Part, op_idx: usize, uncontained: &[Uncontained]) -> Option<Untrusted> {
    let op = &p.operations[op_idx];
    let outside = subjects(p, op)
        .iter()
        .find_map(|s| uncontained.iter().find(|u| u.name == s.name).cloned())?;
    Some(Untrusted {
        feature: outside.name.clone(),
        reason: format!(
            "the stock does not contain '{}' ({}), so the model has no material where the part stands",
            outside.name,
            outside.faces.first().map_or(String::new(), |(_, s)| s.clone())
        ),
        over_states: false,
    })
}

/// Every uncontained feature some RUN op works, with the first op that works
/// it — the rest report's reason to decline (Phase 9 lane B, fork 2A: a
/// declared-but-unrun feature standing proud is the case Phase 6 protected,
/// and it declines nothing).
pub fn uncontained_worked(p: &Part) -> Vec<(Uncontained, usize)> {
    uncontained(p)
        .into_iter()
        .filter_map(|u| {
            let k = p
                .operations
                .iter()
                .enumerate()
                .find(|(_, op)| subjects(p, op).iter().any(|s| s.name == u.name))
                .map(|(k, _)| k)?;
            Some((u, k))
        })
        .collect()
}

/// The entry restriction's declines, voiced after generation (Phase 9 lane B,
/// fork 3A). `check_moves` is a `Result`, and a decline is not an error — so
/// the fact that it could not answer for an op travels the way every
/// post-generation finding does, read off the same emitted moves, derived from the same
/// predicate (`untrusted_for`) and nothing else. One finding per op: the
/// error's silence was per op, and so is the sentence that replaces it.
///
/// The two reasons read differently because they ARE different: on an
/// incomplete model `check_moves` returned before looking (an error could
/// accuse a tool of driving into air); on an uncontained feature it looked,
/// found nothing, and that nothing is not a claim.
pub fn move_declines(p: &Part, emissions: &[crate::gcode::OpEmission]) -> Vec<crate::parse::Finding> {
    let uncontained = uncontained(p);
    let mut out = Vec::new();
    for em in emissions {
        let Some(op) = p.operations.get(em.op_idx) else { continue };
        let Some(u) = untrusted_for(p, em.op_idx, &uncontained) else { continue };
        let message = if u.over_states {
            format!(
                "check_moves declined on '{}' ({} with T{}): {} — the entry restriction did not run for this op, because an error read off an over-statement could accuse the tool of driving into air. Fix the named op to restore the check",
                u.feature, op_word(op), op.tool_num, u.reason
            )
        } else {
            format!(
                "check_moves declined on '{}' ({} with T{}): {} — the pass it gave this op is not a claim: the tool may drive into material the model never had. Declare the stock the part is actually made from, or move the feature's datum into it",
                u.feature, op_word(op), op.tool_num, u.reason
            )
        };
        out.push(crate::parse::Finding::op(crate::parse::FindingFamily::Decline, em.op_idx, &u.feature, message));
    }
    out
}

/// **Reach shortfall** — the assembly is shorter than the hole it has to work
/// in, so the HOLDER has to enter the stock's envelope for the tip to get where
/// the program sends it (`DECISIONS.md` → "K2, the tool body", fork 4c, named
/// there as a follow-up and built as one).
///
/// **What makes this a new check rather than an existing lint seeing more.**
/// The two foul voices ask whether the body stands in MATERIAL, so they are
/// silent over a cavity that an earlier op already opened, and silent in air.
/// This one never consults the material model at all: `stickout` is where the
/// tool ends and the holder begins, so `stickout < reach` means the nose face
/// is below the stock's top face — a fact about the declaration and the
/// program, true whether or not anything is standing there. That is the
/// containment family's shape (a claim about the declaration against the
/// stock), which is why fork 4c filed it as kin, and why it lives beside
/// `containment_findings` rather than beside the foul probes.
///
/// **Why it reads EMISSIONS anyway, unlike its kin.** "The reach the op
/// demands" is how far below the stock top the tip actually goes, and the ops
/// are the only place that number exists. Deriving a second depth from the
/// feature declarations would be a duplicated derived value that could disagree
/// with the program it is judging — so it reads the emitted moves, through `gcode`'s own
/// `Motion`/`apply_move`/`resolved_moves` and `Silhouette::drop()`, and
/// restates no arithmetic.
///
/// **It can only under-claim.** An undeclared `stickout` is skipped entirely
/// (the house law: undeclared means today's behavior, never a guess), a move
/// with no established Z is skipped, and equality is legal — a nose face
/// exactly ON the stock's top face is touching, not entering.
///
/// **The one narrowing, and it is an exoneration only** (`DECISIONS.md` → "the
/// fifth certificate and the corridor…", forks 1A/3A). Since the recommender
/// learned to measure the ROOM in a cavity, this check and that advisory
/// disagreed about a nose in a mouth: every `[nose enters]` row it proposes is
/// by construction a stickout shorter than its reach, so a shop that turned
/// `reach gate` on and followed the report was rejected for taking the advice.
/// The finding is now withdrawn where `nose_has_room` can show the millimetres
/// — and only there. The envelope claim stands exactly as it did everywhere
/// the corridor cannot measure, which is why this is the same finding with a
/// better probe behind it rather than a new claim. (The family later opened
/// at six — `envelope`, Phase 15 fork 4, taken as a decide at the DRAFT's
/// blessing — but the point here stands: this narrowing added no directive
/// of its own.)
pub fn reach_findings(p: &Part, emissions: &[crate::gcode::OpEmission]) -> Vec<crate::parse::Finding> {
    let mut findings = Vec::new();
    let stock_top = p.stock.z;
    for em in emissions {
        let Some(op) = p.operations.get(em.op_idx) else { continue };
        let Some(tool) = p.tools.get(&op.tool_num) else { continue };
        // No declared stickout, nothing declared to be short.
        let Some(stickout) = tool.stickout else { continue };
        let sil = Silhouette::of(tool);

        // One pass over the op's moves, feeding every subject it works. The
        // modal state is advanced by EVERY line — the subject decides whether
        // a position COUNTS, never whether it is read, or a hole would inherit
        // the Z of the retract that took the tool to the next one.
        let subjects = subjects(p, op);
        let mut deepest: Vec<Option<f64>> = vec![None; subjects.len()];
        let mut m = crate::gcode::Motion { rapid: false, x: None, y: None, z: None, f: None };
        for line in crate::gcode::resolved_moves(em) {
            if !crate::gcode::apply_move(line, &mut m) {
                continue;
            }
            let Some(z) = m.z else { continue };
            let tip = z - sil.drop();
            for (i, s) in subjects.iter().enumerate() {
                if s.claims(m.x, m.y) {
                    deepest[i] = Some(deepest[i].map_or(tip, |d: f64| d.min(tip)));
                }
            }
        }

        for (i, s) in subjects.iter().enumerate() {
            let Some(tip_z) = deepest[i] else { continue };
            let reach = stock_top - tip_z;
            if reach <= stickout + EPS {
                continue;
            }
            if nose_has_room(p, em.op_idx, &s.name, tool, stickout, reach) {
                continue;
            }
            findings.push(crate::parse::Finding::op(
                crate::parse::FindingFamily::Reach,
                em.op_idx,
                &s.name,
                format!(
                    "T{}'s {:.3}mm stickout is short of the {:.3}mm its TIP reaches below the stock's top on '{}' — the tool ends {:.3}mm above its own tip, so the holder's nose face sits {:.3}mm inside the stock's envelope. Nothing standing there is required for this to be true: it is the assembly's own length against the depth, which is why the foul lints can be silent here. Declare more stickout, or ask `nacs --assembly-report` for an assembly that reaches — its `reach=` input is exactly this number",
                    op.tool_num, stickout, reach, s.name, stickout, reach - stickout
                ),
            ));
        }
    }
    findings
}

/// The travel half of the machine envelope (Phase 15 slice 1, DECISIONS.md →
/// "Phase 15 opens: the envelope DRAFT…", fork 1): every emitted position,
/// carried into MACHINE coordinates by the declared datum, against the
/// machine file's declared spans. The subject is the CONTROLLER-POSITIONED
/// point — a travel limit is a fact about the axes, and what the body around
/// that point hits is the keep-out's and the foul lints' jurisdiction, not a
/// span's.
///
/// **It runs only when both homes speak.** The spans live in the machine
/// file (an axis's range is shop infrastructure); where the program's one
/// register points lives on the part (`datum x= y=`, the setup sheet's
/// number made declarable). Either absent, this check never speaks — the
/// undeclared-means-today's-behaviour law. Where it does speak it is exact:
/// one addition and one comparison per axis, neither over- nor
/// under-claiming, and equality is legal — a move AT the limit is a position
/// the machine reaches.
///
/// **Every X/Y word the program emits is on this walk.** The per-op moves
/// are the whole X/Y story by construction — the inter-op boilerplate
/// (tool change, spindle, coolant, the final retract) never carries an X or
/// Y word — and under control comp the walk reads the emitter's own
/// resolved tool-center channel, the `check_moves` rule.
pub fn envelope_findings(p: &Part, emissions: &[crate::gcode::OpEmission]) -> Vec<crate::parse::Finding> {
    let mut findings = Vec::new();
    let Some(machine) = p.machine.as_ref() else { return findings };
    if machine.travel_x.is_none() && machine.travel_y.is_none() {
        return findings;
    }
    let Some((dx, dy)) = p.datum else { return findings };

    for em in emissions {
        let Some(op) = p.operations.get(em.op_idx) else { continue };
        // The finding names what it is about: the op's feature, or — for a
        // Drill, whose subject is a hole list — the first declared point.
        let about = if op.feature_name.is_empty() {
            subjects(p, op).first().map(|s| s.name.clone()).unwrap_or_default()
        } else {
            op.feature_name.clone()
        };
        // Worst crossing per axis over the op's moves: (overshoot, machine
        // coordinate, the work-frame position it happened at).
        let mut worst: [Option<(f64, f64, [f64; 3])>; 2] = [None, None];
        let mut m = crate::gcode::Motion { rapid: false, x: None, y: None, z: None, f: None };
        for line in crate::gcode::resolved_moves(em) {
            if !crate::gcode::apply_move(line, &mut m) {
                continue;
            }
            for (i, (val, datum)) in [(m.x, dx), (m.y, dy)].into_iter().enumerate() {
                let Some(v) = val else { continue };
                let Some((lo, hi)) = (if i == 0 { machine.travel_x } else { machine.travel_y }) else { continue };
                let mc = v + datum;
                let over = if mc < lo - EPS {
                    lo - mc
                } else if mc > hi + EPS {
                    mc - hi
                } else {
                    continue;
                };
                if worst[i].map_or(true, |(o, _, _)| over > o) {
                    let at = [m.x.unwrap_or(0.0), m.y.unwrap_or(0.0), m.z.unwrap_or(0.0)];
                    worst[i] = Some((over, mc, at));
                }
            }
        }
        for (i, w) in worst.into_iter().enumerate() {
            let Some((over, mc, at)) = w else { continue };
            let (axis, key, span, datum) = if i == 0 {
                ("X", "travel_x", machine.travel_x.unwrap(), dx)
            } else {
                ("Y", "travel_y", machine.travel_y.unwrap(), dy)
            };
            findings.push(
                crate::parse::Finding::op(
                    crate::parse::FindingFamily::Envelope,
                    em.op_idx,
                    &about,
                    format!(
                        "T{} drives {} to {:.3} in machine coordinates ({:.3} in the program + the datum's {:.3}) on '{}' — {:.3}mm past the declared {} {:.3} .. {:.3}. A travel span is the axis's own fact: no comp mode or body geometry moves it. Shift the setup (the datum), shrink the path, or correct the span if the sheet was misread",
                        op.tool_num, axis, mc, mc - datum, datum, about, over, key, span.0, span.1
                    ),
                )
                .with_place(at),
            );
        }
    }
    findings
}

/// One subject of a finding about an op: a declared name, and the axis its
/// moves have to stand on when the op works several of them.
///
/// **Why the subject is a thing at all** (`DECISIONS.md` → "the per-point reach
/// finding…", forks 1 and 3). Every op but `Drill` names exactly one feature,
/// so a certificate could ask its question per OPERATION and read the answer
/// off `op.feature_name`. A `Drill` names a LIST — `drill_points` — and carries
/// no `feature_name` at all, which made the reach finding say `on ''` and made
/// the narrowing's feature lookup miss every time. The subject is what a
/// finding is ABOUT, and for a drill that is a hole, not an operation.
///
/// Born as the reach certificate's own, and **promoted to the one home for
/// every voice that reads emitted positions** in Phase 9's slice 0: the
/// error (`check_moves`) and the field's foul voice
/// (`vox::body_foul_findings`) attribute each position through `claims`
/// exactly as the reach check does, so the three cannot disagree about which
/// hole a move belongs to. The general shape the per-point slice named —
/// *an op's `feature_name` is not the same thing as what a finding is about* —
/// now has one place to be true.
pub struct Subject {
    pub name: String,
    /// `None` = every move in the op belongs to this subject, which is the
    /// single-feature case and today's behavior exactly.
    at: Option<crate::parse::Pt>,
}

impl Subject {
    /// Does the tool, standing here, count against this subject?
    ///
    /// **The tolerance is the EMISSION's**, the same width the narrowing had to
    /// learn (`EMITTED_MM_HALF_ULP`, not `EPS`): both sides are one declared
    /// coordinate, one of them written out at three decimals and read back.
    /// A position with no XY established yet cannot be attributed and is
    /// SKIPPED — in a `Drill` that is only the opening retract to clearance,
    /// which is above the stock and could never bind, and skipping keeps the
    /// standing law that this check may only under-claim.
    pub fn claims(&self, x: Option<f64>, y: Option<f64>) -> bool {
        let Some(at) = self.at else { return true };
        matches!(
            (x, y),
            (Some(x), Some(y))
                if (x - at.x).abs() <= EMITTED_MM_HALF_ULP
                    && (y - at.y).abs() <= EMITTED_MM_HALF_ULP
        )
    }
}

/// What the op's reach claim is about, one entry per subject.
///
/// **An ATTRIBUTION, never a second derivation** (fork 2). This certificate
/// reads the emitted moves by law — deriving a depth from the declaration would be a
/// duplicated derived value that could disagree with the program it judges —
/// and the split respects it, because `ops::drill` emits every one of its Z
/// moves standing on a listed hole's own centre (rapid to (hx, hy), rapid Z,
/// then the cycle). So "the deepest tip at this point's XY" is read off the
/// same lines as before, with no arithmetic restated. `stock::probes` and
/// `removals_for` split the same op by DERIVING each hole's depth, and are
/// right to: they ask what the model removed, where this asks what the program
/// did.
///
/// A point whose feature is undeclared or states no axis is dropped rather
/// than guessed at — `validate` already guarantees a `Drill`'s points are
/// declared bores, so this is the under-claiming reading of a case that cannot
/// arise. Two points declared at one XY both claim the same moves and report
/// the same reach; that is a duplicate declaration's problem, not this
/// check's.
pub fn subjects(p: &Part, op: &Operation) -> Vec<Subject> {
    if op.at_points() {
        let Some(names) = op.drill_points.as_ref() else { return Vec::new() };
        return names
            .iter()
            .filter_map(|name| {
                let at = p.features.get(name)?.axis()?;
                Some(Subject { name: name.clone(), at: Some(at) })
            })
            .collect();
    }
    // Every other op works one declared feature, so the general form
    // degenerates to what this check has always done: one subject, no filter,
    // every move — which is why no existing program's findings move.
    vec![Subject { name: op.feature_name.clone(), at: None }]
}

/// The name a finding at this emitted position is about — the first subject
/// that claims it — falling back to the op's own feature name when none does.
/// The fallback is today's text exactly, so the attribution can only ever
/// replace an empty name with a declared one, never the reverse.
pub fn subject_at<'a>(subjects: &'a [Subject], op: &'a Operation, x: f64, y: f64) -> &'a str {
    subjects
        .iter()
        .find(|s| s.claims(Some(x), Some(y)))
        .map_or(op.feature_name.as_str(), |s| s.name.as_str())
}

/// **The exoneration, and the only thing the corridor is allowed to do to the
/// certificate above** (`DECISIONS.md` → "the fifth certificate and the
/// corridor…", fork 4A). True when the nose this program declares
/// demonstrably HAS the room it stands in — at which point the shortfall is a
/// statement about the stock's envelope and not about an interference, and the
/// recommender that proposed the assembly was right.
///
/// **All three or nothing.** (1) the op's feature is a cavity
/// `assembly::corridor_of` reads, (2) the declared tool is admissible in it —
/// a rod in a funnel is rejected by the pairing law `place` quotes, so it
/// cannot be exonerated by one — and (3) the room at the nose face's own depth
/// is at least the nose's own radius. Any of the three missing returns false
/// and today's finding stands, so **the narrowing can only ever silence a case
/// it can MEASURE**: a facing pass, a feature kind the corridor rejects, an
/// undeclared nose and a drawn profile are all untouched, and the check keeps
/// under-claiming in exactly the places it always did.
///
/// **No clearance, and equality is legal** (fork 5A). The recommender's
/// `clearance` is a machinist's declared radial comfort margin on a PROPOSAL;
/// a program states no such number, and a certificate is not the place to
/// invent a default for one. So what is asked here is the interference
/// question alone — is the nose inside the wall — which is the same call the
/// envelope claim makes about a nose face resting on the top face.
fn nose_has_room(
    p: &Part,
    op_idx: usize,
    feature_name: &str,
    tool: &Tool,
    stickout: f64,
    reach: f64,
) -> bool {
    // (3)'s measurand, asked first because it is the cheapest: a holder whose
    // width is undeclared cannot be exonerated by a width.
    let Some(nose_dia) = tool.holder_nose_dia else { return false };
    // (1) the feature is a cavity the corridor reads. The NAME is the caller's
    // (`DECISIONS.md` → "the per-point reach finding…", fork 4): reading
    // `op.feature_name` here is what made this unreachable for a `Drill`,
    // whose work is a list of points and whose op names no feature at all.
    // Nothing else about the three conditions moves — this is the same claim
    // with the right subject, not a new one.
    let Some(feature) = p.features.get(feature_name) else { return false };
    // `Opened::Before(op_idx)`: this door has a PROGRAM, and a program states
    // an ORDER (`DECISIONS.md` → "the multi-feature cavity…", fork 4A). A
    // coaxial neighbour widens the mouth only where an earlier op has already
    // opened it — a check that GATES may not silence a finding on room the
    // program has not made yet, and the entry gate's own coverage of that case
    // is not this family's to borrow: it goes silent on an incomplete removal
    // model by design.
    let opened = crate::assembly::Opened::Before(op_idx);
    let Ok(cavity) = crate::assembly::corridor_of(p, feature, opened) else { return false };
    // (2) the declared tool is admissible in it.
    let shape = crate::assembly::CutterShape::of_tool(tool);
    let Ok(placed) = cavity.corridor.place(&shape, reach) else { return false };

    // The corridor has to account for the depth the PROGRAM actually reaches.
    // If the tip goes deeper than the cavity the declaration states, the
    // corridor is not describing this move and has nothing to say about it —
    // which is the reading that leaves the finding standing.
    //
    // **The tolerance is the EMISSION's, not a fudge.** These are two
    // measurements of one depth by different routes: the corridor derives it
    // from the declaration in full precision, and this check reads it back off
    // text the emitter wrote in mm at three decimals. So they agree to within
    // half of that last place and no closer, and `EPS` here would reject an
    // assembly over a quarter of a micron of rounding — which it did, on the
    // shipped catalog's own top recommendation.
    if placed.reach + EMITTED_MM_HALF_ULP < reach {
        return false;
    }

    let nose_r = nose_dia / 2.0;
    // EVERY working position, not just the deepest. On a curved wall the tool
    // stands further OUT the higher it is, so the mouth is exactly where a
    // descended nose comes nearest the material — the same reason
    // `min_projection` walks the whole locus. Positions whose nose face is at
    // or above the stock's top face are not this claim's subject: nothing is
    // inside the envelope there.
    for (tip_depth, excursion) in &placed.locus {
        let face = tip_depth - stickout;
        if face <= EPS {
            continue;
        }
        if placed.corridor.room_at(face, shape.radius, *excursion) + EPS < nose_r {
            return false;
        }
    }
    true
}

/// The lowest point on the tool body standing at (ax, ay) with its TIP at
/// `tip_z` that occupies remaining material, or None. `slack` is how far inside
/// standing material the body must be before it counts.
fn body_hits(
    remaining: &RemainingStock,
    sil: &Silhouette,
    ax: f64,
    ay: f64,
    tip_z: f64,
    reach: f64,
    slack: f64,
) -> Option<(f64, f64, f64, f64, f64)> {
    body_hits_where(remaining, sil, ax, ay, tip_z, reach, slack, AZIMUTH_SAMPLES, &|_, _, _| false)
}

/// The same probe, less the points this op is PERMITTED to sweep (Phase 32
/// slice 3): a hit inside its own overhang allowance is not a foul, and the
/// probe keeps looking — a move may overhang scrap and still reach a
/// neighbour it must not touch.
#[allow(clippy::too_many_arguments)]
fn body_hits_where(
    remaining: &RemainingStock,
    sil: &Silhouette,
    ax: f64,
    ay: f64,
    tip_z: f64,
    reach: f64,
    slack: f64,
    azimuths: usize,
    permitted: &dyn Fn(f64, f64, f64) -> bool,
) -> Option<(f64, f64, f64, f64, f64)> {
    for i in 0..=HEIGHT_SAMPLES {
        let h = reach * i as f64 / HEIGHT_SAMPLES as f64;
        let rb = sil.radius_at(h);
        let z = tip_z + h;
        for b in 0..azimuths {
            let tb = 2.0 * PI * b as f64 / azimuths as f64;
            let (px, py) = (ax + rb * tb.pcos(), ay + rb * tb.psin());
            if remaining.material_at_slack(px, py, z, slack) && !permitted(px, py, z) {
                return Some((h, rb, px, py, z));
            }
        }
    }
    None
}

/// The first axis position on the probe ring whose body occupies remaining
/// material, at that position's lowest such height — or None. One foul per op is
/// the actionable message; the rest are the same wall, restated.
fn first_foul(
    remaining: &RemainingStock,
    sil: &Silhouette,
    cx: f64,
    cy: f64,
    axis_r: f64,
    tip_z: f64,
    reach: f64,
) -> Option<(f64, f64, f64, f64, f64)> {
    for a in 0..AXIS_SAMPLES {
        let ta = 2.0 * PI * a as f64 / AXIS_SAMPLES as f64;
        let (ax, ay) = (cx + axis_r * ta.pcos(), cy + axis_r * ta.psin());
        if let Some(hit) = body_hits(remaining, sil, ax, ay, tip_z, reach, EPS) {
            return Some(hit);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phase 10 lane B, slice 1 (fork 3A): the ball's form is the closed form
    /// the tube always used — so its floor and ceiling reproduce byte for
    /// byte, which every golden and rim-area pin then says at scale — and the
    /// profile's form is its cutting envelope about the tip with a WALL at its
    /// edge, widened by the emitted half-ULP and stationed exactly.
    #[test]
    fn the_ball_form_is_the_closed_form_and_the_profile_form_carries_its_wall() {
        let ball = CutForm::Ball { r: 3.0 };
        for d in [-3.0, -2.5, 0.0, 1.0, 2.9999, 3.0] {
            let h = (9.0f64 - d * d).sqrt();
            assert_eq!(ball.span(d), Some((-h, h)), "today's `c.z ∓ sqrt(r² - d²)` at d {d}");
        }
        assert_eq!(ball.span(3.0 + 1e-9), None);
        assert!(ball.cliffs().is_empty(), "a ball's edge tapers to nothing");

        // The fixture's drawing: tip (0,0), corner (5,5), wall to (5,10).
        let pts = vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 5.0, y: 5.0 }, Pt { x: 5.0, y: 10.0 }];
        let form = CutForm::Profile { pts: &pts, cut_top: 10.0 };
        assert_eq!(form.span(2.0), Some((2.0, 10.0)), "the 45° flank, the cutting top above it");
        assert_eq!(form.span(-5.0), Some((5.0, 10.0)), "the wall's foot, signed distance either side");
        assert_eq!(form.span(5.0 + EMITTED_MM_HALF_ULP), Some((5.0, 10.0)), "the emitted rounding ball is inside the form");
        assert_eq!(form.span(5.001), None, "past it the form is not there");
        let band = CutForm::Profile { pts: &pts[..2], cut_top: 5.0 };
        assert_eq!(band.span(5.0), Some((5.0, 5.0)), "a band ending at the corner has no wall");

        // Two of the shipped fixture's own rings (tip z, tip rho): the rim ring
        // and the `Z-0.286` ring whose wall the probe met. Between the second
        // ring's edge and a hair past it the ceiling drops the whole wall —
        // 9.714 to 5.000 — as a wall, not as a ramp one station wide.
        let rings = [(15.0, -5.0), (13.8187, -0.2857)];
        let r = swept_tube(0.0, 0.0, &rings, CutForm::Profile { pts: &pts, cut_top: 10.0 }).expect("a tube");
        let ceil = r.ceiling.as_ref().expect("the tube has a ceiling");
        assert!((r.floor.z_at(17.0, 0.0) - (-3.0)).abs() < 1e-9, "floor: the rim ring's flank, -5 + 2");
        let edge = 13.8187 + 5.0 + EMITTED_MM_HALF_ULP;
        assert!((ceil.z_at(edge - 1e-7, 0.0) - 9.7143).abs() < 1e-3, "inside the ring's edge the wall stands: {}", ceil.z_at(edge - 1e-7, 0.0));
        assert!((ceil.z_at(edge + 1e-7, 0.0) - 5.0).abs() < 1e-3, "a hair past it only the rim ring reaches: {}", ceil.z_at(edge + 1e-7, 0.0));
        match r.section {
            Section::Annulus { r_inner, r_outer, .. } => {
                assert!((r_inner - (13.8187 - 5.0 - EMITTED_MM_HALF_ULP)).abs() < 1e-9);
                assert!((r_outer - (20.0 + EMITTED_MM_HALF_ULP)).abs() < 1e-9);
            }
            other => panic!("an annulus about the axis, got {other:?}"),
        }
    }

    use crate::parse::{CompMode, CompressMode, OpType, Stock};

    // A minimal Part carrying one tool and one Copy op over a torus sunk into
    // the stock top (the same ring the probe test below reads).
    fn funnel_part(tool: Tool) -> Part {
        let mut p = Part::default();
        p.stock = Stock { shape: crate::parse::StockShape::Box, w: 100.0, l: 80.0, h: 40.0, x: 0.0, y: 0.0, z: 0.0, material: None };
        let tool_num = tool.num;
        p.tools.insert(tool_num, tool);
        p.features.insert(
            "Vortex".into(),
            Feature::Torus { major_dia: 50.0, minor_dia: 8.0, x: 50.0, y: 40.0, z: -4.0 },
        );
        p.operations.push(Operation {
            op_type: OpType::Copy,
            tool_num,
            feature_name: "Vortex".into(),
            stepover: Some(3.0),
            stepdown: None,
            scallop: None,
            feed: None,
            plunge: None,
            rapid_z: None,
            clearance_z: None,
            spindle: None,
            feed_derive: false,
            spindle_derive: false,
            chip_thinning: None,
            pass_feeds: None,
            drill_points: None,
            cycle: None,
            peck_increment: None,
            comp: CompMode::Engine,
            compress: CompressMode::Chords,
            direction: None, angle: None, entry: None, lead: None, lead_angle: None, lead_descent: None,
            stock_to_leave: None,
            strategy: None,
            pattern: None,
            engagement: None,
            cover: None,
            voxel_cell: None, coolant: None, n: None,
            wall_standoff: 0.0,
            face_standoff: 0.0,
            tip_clearance: None,
        });
        p
    }

    // A minimal Part carrying one Ballmill and one Finish op over `feature`,
    // sunk so the declared solid is actually inside the stock.

    // The same Part with `feature` in place of the torus.
    fn cap_part(name: &str, feature: Feature) -> Part {
        let mut p = funnel_part(Tool { num: 10, name: "Ballmill".into(), dia: 6.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        p.features.clear();
        p.features.insert(name.into(), feature);
        p.operations[0].feature_name = name.into();
        p
    }

    #[test]
    fn a_torus_is_probed_in_its_hole() {
        // A torus's inner equator is the bottom of an inward-facing bowl:
        // machining it puts the tool inside the middle column, so the hole is
        // a place the geometry forces it and gets its own probe. Integration
        // tests cannot see this (the outer rim reports first), so it is pinned here.
        let tool = Tool { num: 10, name: "Ballmill".into(), dia: 6.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None };

        let p = cap_part("Ring", Feature::Torus { major_dia: 50.0, minor_dia: 8.0, x: 50.0, y: 40.0, z: -4.0 });
        let torus = probes(&p, &p.operations[0], &tool);
        assert_eq!(torus.len(), 2, "a torus is probed at both rims, got {:?}", torus);
        assert!(torus[1].axis_r > 0.0 && torus[1].axis_r < torus[0].axis_r, "the second probe is the inner rim");
        assert!(torus.iter().all(|pr| pr.subject == "Ring"), "a cap's probes are about the cap: {:?}", torus);
    }

}

// ---- the entry restriction: reject the move, never rewrite it ---------------
//
// The last piece of ROADMAP level (1) — "restricts plunge/feed entry
// accordingly". `gcode::program` hands each op's finished moves here before
// stitching them in, so this reads what will actually be emitted rather than
// where the geometry forces the tool (`probes`, above). A violation is a hard
// error: the program either comes out as it is or does not come out at all
// (DECISIONS.md "The entry restriction refuses the program…"). Nothing here
// rewrites a move — converting a plunge into a ramp is a cutting-strategy
// decision this level has not made.

/// How far inside standing material a move must be before it is rejected. The
/// model's own meridians are 256-point polylines, whose chord error sits four
/// orders above `EPS`, so rejecting at `EPS` would reject tangency — and a
/// finishing pass IS tangency. `CHORD_TOL_MM` is the right number precisely
/// because the error being tolerated is a curve-flattening error: below the
/// project's one geometric tolerance, nothing here is certain enough to reject.
const MOTION_SLACK: f64 = crate::CHORD_TOL_MM;

/// Emitted Z is the ball CENTRE for the surface ops (DECISIONS.md "Camp-2
/// finishing…", `finish_sphere`'s convention) and the tool tip everywhere else,
/// so the body model — which measures upward from the tip — has to be told
/// which. That gap IS the silhouette's `drop()`, read from the one home rather
/// than re-testing the tool's name here (slice 1 of "the profile silhouette…").
fn tip_z_of(tool: &Tool, emitted_z: f64) -> f64 {
    emitted_z - Silhouette::of(tool).drop()
}

// The modal `Motion` reader lives with the writer in `gcode` (one home for
// the dialect); the entry restriction below and the voxel stamp builder both
// read emitted moves through it.
use crate::gcode::{apply_move, Motion};

/// **What counts as an entry, and why it is not "every descent".** An entry is
/// the first descent after a retract to the safe plane — the move that commits
/// the tool from clear air into the work — plus any rapid that travels in XY.
/// Those are the moves an author can *choose*: enter somewhere else, pre-drill,
/// ramp, reorder the ops.
///
/// A descent that continues an already-engaged pass sequence is not an entry,
/// and the reason is the discipline this whole file runs on: never claim what
/// the tool cannot avoid. Finishing any surface whose rim meets the stock's
/// top forces the tool below that plane at the last pass — a ball contacting
/// the rim has its lowest point a radius below the contact — so judging that
/// would judge every correct ball-nose program of that shape. It is real
/// contact with standing stock, and the advisory probes above are where it
/// belongs; it is not a *choice*, so it is not an entry.
///
/// A downward move needs only its destination checked: the body sweeps from the
/// tip up to the stock's top there, which contains everything the tool occupied
/// higher up the same column. Lateral *feed* moves are the op's cutting motion,
/// and the forced-position probes already speak for those.
///
/// One walk, two readers (DECISIONS.md "the plunge taxonomy"): `check_moves`
/// rejects these positions into material that outlives the program;
/// `entry_findings` flags them into material standing only until the op's own
/// declaration removes it. The entry definition lives here once so the error
/// and the flag can never disagree about what counts.
fn committing_moves<'a>(p: &Part, op: &Operation, moves: &'a [String]) -> Vec<(&'a str, f64, f64, f64)> {
    let safe_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let mut m = Motion { rapid: true, x: None, y: None, z: None, f: None };
    // The tool starts every op at clearance, so it starts retracted.
    let mut retracted = true;
    let mut out = Vec::new();
    for line in moves {
        let before = m;
        if !apply_move(line, &mut m) {
            continue;
        }
        let (x, y, z) = match (m.x, m.y, m.z) {
            (Some(x), Some(y), Some(z)) => (x, y, z),
            _ => continue,
        };
        let descending = before.z.map_or(true, |bz| z < bz);
        let entering = descending && retracted;
        let traversing = m.rapid && (before.x != m.x || before.y != m.y);
        // Back at the safe plane, the next descent is a fresh entry.
        if z >= safe_z {
            retracted = true;
        } else if descending {
            retracted = false;
        }
        if entering || traversing {
            out.push((line.as_str(), x, y, z));
        }
    }
    out
}

/// Reject an op whose emitted motion drives the tool body into material that
/// outlives it. Called from `gcode::program` with the op's TOOL-CENTER moves:
/// the text head for engine ops, the emitter-authored resolved channel for
/// control ops (DECISIONS.md "the control-comp centerline resolve"). The
/// control skip this function used to carry existed because reading part-edge
/// text as tool positions would accuse every comp=control op of straddling its
/// own wall; the resolved channel is where the tool actually goes, so the
/// false-accusation reason is gone and a control op answers for its entries
/// like any other.
pub fn check_moves(p: &Part, op_idx: usize, moves: &[String]) -> Result<(), String> {
    let op = &p.operations[op_idx];
    // Total accounting: an incomplete model over-states material, and rejecting a
    // program on an over-statement would be the worst outcome this file can
    // produce. This silence is VOICED after generation by `move_declines`
    // (Phase 9 lane B) — a wordless `Ok` was never a claim, and now says so.
    let room = match entry_room(p, op_idx) {
        Some(r) => r,
        None => return Ok(()),
    };
    let subjects = subjects(p, op);

    for (line, x, y, z) in committing_moves(p, op, moves) {
        if let Some((h, rb, px, py, pz)) = room.hit(x, y, z) {
            let tip = room.tip_of(z);
            // The error is ABOUT the hole the move stands on, attributed off
            // the same emitted XY the probe read — for a drill, `op.feature_name`
            // is empty and was printed as the subject until Phase 9's slice 0.
            return Err(format!(
                "{} on '{}' with T{}: the move `{}` drives the tool into material that outlives this operation — its body {:.3}mm above the tip sweeps to r {:.3}mm and reaches standing stock at ({:.3}, {:.3}, {:.3}). Remove that material in an earlier op, enter where the stock is already open, or declare the stock to what is actually on the machine — the tip at ({:.3}, {:.3}, {:.3})",
                op_word(op),
                subject_at(&subjects, op, x, y), op.tool_num, line.trim(), h, rb, px, py, pz, x, y, tip
            ));
        }
    }
    Ok(())
}

/// The entry restriction's question, held so it can be asked BEFORE a point is
/// chosen (Phase 47 slice 2, fork 1A — the wall trace's lead reads the room).
/// Built once per op from the same three things `check_moves` reads — the
/// model of what outlives the op, the tool's silhouette, the overhang the op
/// is permitted — so a point this says stands in air is a point the
/// restriction accepts, by construction and not by agreement. `entry_room`
/// is `None` when the model is incomplete: no claim, so no reader may act on
/// it, and the restriction's own silence (`move_declines`) is the voice.
pub struct EntryRoom {
    remaining: RemainingStock,
    sil: Silhouette,
    overhang: Vec<Overhang>,
    stock_top: f64,
}

pub fn entry_room(p: &Part, op_idx: usize) -> Option<EntryRoom> {
    let op = p.operations.get(op_idx)?;
    let tool = p.tools.get(&op.tool_num)?;
    let remaining = remaining_after(p, op_idx);
    if !remaining.complete {
        return None;
    }
    // The overhang this op is allowed (Phase 32 slice 3, fork D): material
    // standing above its own floor, inside its own swath, is scrap it may
    // sweep — the permission the swath used to carry by claiming the scrap
    // removed. Outside the swath, or below the floor, nothing changes.
    Some(EntryRoom { remaining, sil: Silhouette::of(tool), overhang: face_overhang(p, op), stock_top: p.stock.z })
}

impl EntryRoom {
    fn tip_of(&self, emitted_z: f64) -> f64 {
        emitted_z - self.sil.drop()
    }

    /// Where the body standing at the emitted (x, y, z) first meets material
    /// that outlives the op — `(height above the tip, body radius there, px,
    /// py, pz)` — or `None` when it stands in air, to the restriction's own
    /// slack (a chord tolerance: contact is not a gouge) and its own rim
    /// sampling.
    fn hit(&self, x: f64, y: f64, emitted_z: f64) -> Option<(f64, f64, f64, f64, f64)> {
        self.probe(x, y, emitted_z, MOTION_SLACK, AZIMUTH_SAMPLES)
    }

    fn probe(&self, x: f64, y: f64, emitted_z: f64, slack: f64, azimuths: usize) -> Option<(f64, f64, f64, f64, f64)> {
        let tip = self.tip_of(emitted_z);
        let reach = self.stock_top - tip;
        if reach <= 0.0 {
            return None;
        }
        let permitted = |px: f64, py: f64, pz: f64| self.overhang.iter().any(|o| o.permits(px, py, pz));
        body_hits_where(&self.remaining, &self.sil, x, y, tip, reach, slack, azimuths, &permitted)
    }

    /// Can the tool stand at this emitted position without its body in
    /// material that outlives the op? The lead's question, read with the
    /// restriction's slack (the tip stands ON the floor it cut, and contact
    /// is not a gouge) but the rim sampled every half degree, where the
    /// restriction samples every fifteen: at a slot's corner the coarse rim
    /// passed a body 0.14 into the far wall between two samples, and the
    /// fixture's one-millimetre slot read 1.180. A point clear here is a
    /// point the restriction accepts, its reading being the weaker.
    pub fn in_air(&self, x: f64, y: f64, emitted_z: f64) -> bool {
        self.probe(x, y, emitted_z, MOTION_SLACK, LEAD_AZIMUTHS).is_none()
    }

    /// The slack `in_air` forgives — what a shortened lead gives back, so the
    /// room it prints is the room there is and the body stands tangent to
    /// what stopped it, not a chord tolerance inside it.
    pub fn slack(&self) -> f64 {
        MOTION_SLACK
    }
}

/// The lead's rim sampling: every half degree, so the room it reads can be
/// over by at most r·(1 − cos 0.25°) — a fifth of a thou on a ⌀16.
const LEAD_AZIMUTHS: usize = 720;

/// The plunge taxonomy's category-3 flag (DECISIONS.md "the plunge taxonomy"):
/// a Finish entry into material that stands BEFORE the op runs but is void
/// after it — work only the op's own declaration accounts for, i.e. a wall
/// finish plunging a pocket nobody cleared. Advisory by charter: deliberate
/// plunge-entry is a real manual solution, so the finding is voiced, never a
/// silent rewrite and never an error — unless the author opts into the
/// `entry gate` directive, which rejects the program on these same findings.
///
/// Clear, Drill, and Floor are LICENSED to meet virgin stock — turning
/// declared stock into void is their charter, entry included — so only Finish
/// ops are read: every Finish arm documents the cleared-surroundings
/// assumption this flag makes enforceable. An incomplete before-model
/// declines silently (an over-stated model would fire false flags — the same
/// total-accounting law the error above stands on, and what keeps boss
/// finishes honest: their cleared surround is a path fact the model rejects
/// to guess at). One finding per op — the first flagged entry tells the
/// story; a ladder of Z levels repeating it is noise.
/// The skin a Finish inherits from its own rough — the engagement linter's
/// derived pairing, run from the consumer's side: the nearest EARLIER
/// wall-cutting op on the same feature is this op's producer, and the skin
/// handed down is its allowance minus this op's own. A wall op between with no
/// allowance breaks the chain (it consumed the skin itself), exactly as the
/// producer-side derivation would conclude. Zero means unpaired.
pub fn inherited_skin(p: &Part, k: usize) -> f64 {
    let op = &p.operations[k];
    let own = op.stock_to_leave.unwrap_or(0.0);
    p.operations[..k]
        .iter()
        .rev()
        .find(|j| j.feature_name == op.feature_name && j.cuts_walls())
        .and_then(|j| j.stock_to_leave)
        .map(|a| (a - own).max(0.0))
        .unwrap_or(0.0)
}

pub fn entry_findings(p: &Part, emissions: &[crate::gcode::OpEmission]) -> Vec<crate::parse::Finding> {
    let mut findings = Vec::new();
    for em in emissions {
        let op = &p.operations[em.op_idx];
        if !op.follows_surface() {
            continue;
        }
        let tool = match p.tools.get(&op.tool_num) {
            Some(t) => t,
            None => continue,
        };
        let before = remaining_before(p, em.op_idx);
        // The gate reads the AFTER-model's completeness, not the before's
        // (which it subsumes — same prefix plus this op), because that is
        // what makes a before-hit category 3 EXACTLY: `check_moves` already
        // passed these same positions against the after-model, so standing
        // before + complete after means the material is removed by this op's
        // own declaration and nothing else. It is also what keeps boss
        // finishes honest — their cleared surround is a path fact,
        // `removals_for`'s None arm — and what declines when any earlier op
        // is unmodeled (an over-stated model would fire false flags, the
        // same total-accounting law the error stands on).
        if !remaining_after(p, em.op_idx).complete {
            continue;
        }
        // The derived-pairing license (fork 3A of "rim-area accounting…",
        // blessed by name): a Finish PAIRED behind its own rough — the
        // engagement linter's consumer derivation, run from this side — is
        // licensed outright, because a category-3 hit's own preconditions
        // guarantee the entry cuts only what this op declares (standing
        // before + complete after + `check_moves` passed means the material
        // is removed by this op's own declaration and nothing else — a
        // neighbour's stock would have been a hard error, not a flag).
        //
        // A stricter per-sample bound — "within the inherited skin of the
        // op's own floor" — was built first and DIED ON MEASUREMENT: the
        // producer's apex-break level cuts a flat at the OFFSET apex, so the
        // residual cap the finish plunges is skin plus that flat's sagitta
        // over the dome (0.532mm against a 0.500 allowance at the flagship
        // fixture's first lattice sample, a stepdown's worth at the terrace
        // shoulder). The residue is the producer's structural leftover — part
        // of what the pairing MEANS — so the license line is the pairing, and
        // the voice survives exactly where the pairing is absent: an
        // uncleared pocket finish, a contained dome with no rough in front.
        if inherited_skin(p, em.op_idx) > 0.0 {
            continue;
        }
        let moves = crate::gcode::resolved_moves(em);
        let sil = Silhouette::of(tool);
        for (line, x, y, z) in committing_moves(p, op, moves) {
            let tip = tip_z_of(tool, z);
            let reach = p.stock.z - tip;
            if reach <= 0.0 {
                continue;
            }
            if let Some((_, _, px, py, pz)) = body_hits(&before, &sil, x, y, tip, reach, MOTION_SLACK) {
                findings.push(crate::parse::Finding::op(crate::parse::FindingFamily::Entry, em.op_idx, &op.feature_name, format!(
                    "Finish on '{}' with T{}: the entry `{}` meets standing material at ({:.3}, {:.3}, {:.3}) that no earlier op cleared — only this op's own declared removal accounts for it, so the tool cuts its way in (a manual plunge entry; the usual sequence clears the feature first). This is advice; an `entry gate` line in the header refuses the program on it — the tip at ({:.3}, {:.3}, {:.3})",
                    op.feature_name, op.tool_num, line.trim(), px, py, pz, x, y, tip
                )).with_place([px, py, pz]).with_tip([x, y, tip], op.tool_num));
                break;
            }
        }
    }
    findings
}

