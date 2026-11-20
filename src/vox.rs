use crate::math::Portable as _;
// ---- vox: the Phase-5 voxel stock engine, slice 1 ----------------------------
//
// A truncated-signed-distance narrow-band sparse octree over the stock,
// stamped by the swept volume of every emitted move (DECISIONS.md "Phase 5's
// voxel stock engine…", option B blessed 2026-07-31). Negative is material,
// positive is air, occupancy is the sign; a removal is
// `field = max(field, -d_swept)`.
//
// What this is NOT (the design entry's boundaries, restated where the code
// could drift): not a finishing surface — no finishing pass reads this field
// (the 2026-06 scoping decision, whose actual charter makes the field the
// source of truth for stock state, rest-machining detection, and roughing
// toolpaths, and reserves only FINISHING for the exact surface — so roughing
// selection from field boundaries, as `Clear(strategy=rest)` does, is what
// the engine is for); not a replacement for `stock.rs` — the analytic op-history model
// stays the exact model for declared prefixes, this field covers exactly what
// that model declines, and neither is consulted where the other is the
// authority; not a CSG engine.
//
// The stamp inventory is N + M, not N × M: every tool is a solid of
// revolution and every emitted move sweeps it along a path, so membership
// factors through (rho, z) — rho is the plan distance to the path (segment,
// arc, or point) and the tool is a silhouette in that half-plane. Slice 1
// carries the two rod silhouettes (Ballmill: ball-tipped semi-infinite rod,
// coordinates are the ball CENTER per the emission convention `tip_z_of`
// documents; everything else flat-tipped, coordinates are the tip — the same
// dichotomy `require_flat_bottom_tool` runs on). The rod is semi-infinite
// upward because everything a valid move's body sweeps is void by definition;
// a body standing in material is the foul lint's finding, and with this field
// that query becomes a distance lookup (slice 3).
//
// Exactness, stated honestly (the design entry's audit): occupancy's SIGN is
// exact for both silhouettes on every move class the engine emits —
// horizontal moves collapse exactly (any silhouette), vertical moves keep the
// revolution symmetry, and a sloped move's rod membership is the covering
// interval (one quadratic in the move parameter, tip Z linear, min at an
// endpoint — the two endpoints are the advancing edge and the dragging heel).
// The distance MAGNITUDE near a sloped move's dragged floor takes a stated
// safe-side bound instead of a closed form: the floor turns vertical at the
// covering-interval tangency seams, so a tilted-plane bound would over-claim
// exactly there (the heel shadow). Since the per-instance distance is
// Lipschitz in the move parameter, the sloped stamp takes a sampled min over
// instances with the sampling error subtracted on the outside branch —
// certified never to over-state on either side (over-stating |d| inside the
// sweep would over-claim clearance-to-material; over-stating outside would
// let the max claim false depth). Horizontal and vertical stamps are the
// exact closed forms. Every formula here is 1-Lipschitz, which is what the
// octree's collapse margins and the flat-vs-tree equality pin lean on.
//
// Total accounting (the `stock.rs` pattern, ported verbatim): anything the
// emitted moves cannot yet stamp — a control-comp op (emitted coordinates are the part
// edge, the `check_moves` rule), a `profile` tool (its revolved-polyline
// silhouette is the named upgrade path), a move the walker cannot place below
// the stock's top — marks the field `complete = false` and is never guessed
// at. A consumer that finds `complete == false` must make no claim about
// what remains.

use crate::gcode::{self, Motion, PATH_EPS};
use crate::parse::{CompMode, Part, Pt, Silhouette, Stock, StockShape};
use std::path::Path;

/// Cells along the stock's longest edge at the default resolution, the one
/// home for the derivation (the design entry's "longest edge over a stated
/// power of two"). 2^8 puts a 100mm part at ~0.4mm cells — coarse enough to
/// build fast, fine enough that roughing/verification queries resolve; the
/// 2026-06 decision says stair-step error is *chosen*, so tests and future
/// consumers pass an explicit cell instead when they mean something else.
const DEFAULT_CELLS: f64 = 256.0;

/// Truncation band, in finest cells. Two: one so every surface-adjacent cell
/// center holds a live distance rather than a clamp (the sign-crossing cell
/// and its neighbor), one spare so the collapse margins and the
/// piecewise-constant query stay conservative.
const TAU_CELLS: f64 = 2.0;

/// Instances sampled along a sloped move for the safe-side distance bound
/// (uniform, endpoints included; the covering-interval endpoints are appended
/// when real). The certified error is len3d / (2*(SLOPE_SAMPLES-1)), carried
/// per stamp and honored by the octree's culling margin.
const SLOPE_SAMPLES: usize = 9;

// The vertical/horizontal classification guard `PATH_EPS` lives in `gcode`
// now (one home beside the writer whose rounding justifies its magnitude);
// this file's stamp math shares it through the import above.

#[derive(Clone, Debug)]
enum PathK {
    /// Vertical or zero-length travel: the lowest instance contains every
    /// higher one (the rod is semi-infinite upward), so the union IS the tool
    /// at the lowest point — vertical moves keep the revolution symmetry.
    ///
    /// A TRUNCATED silhouette breaks that containment — a higher instance
    /// reaches higher — so `lift` carries the travel and the cross-section's
    /// top cap is extended by it. That is exact rather than conservative
    /// because the walker rejects a silhouette that narrows going up: with a
    /// non-decreasing profile the deepest instance is the widest at every
    /// height, and the union is its cross-section plus the extended cap.
    /// `lift` is 0 for a zero-length move and ignored by the rods.
    Point { x: f64, y: f64, z: f64, lift: f64 },
    /// Horizontal segment at constant coordinate height.
    HSeg { ax: f64, ay: f64, bx: f64, by: f64, z: f64 },
    /// Horizontal arc (the dialect's arcs are constant-Z: `gcode::arc` has no
    /// Z word). `sweep` is signed radians, +ccw; |sweep| == 2π is a full ring.
    HArc { cx: f64, cy: f64, ra: f64, a0: f64, sweep: f64, z: f64 },
    /// Sloped linear move — the one class where the sweep has a heel.
    Slope { a: [f64; 3], b: [f64; 3] },
}

struct Stamp {
    /// The op (0-based `run` index) whose move swept this. Carried so a
    /// violated cell can name the op that cut it by re-evaluating the built
    /// field's own stamps — never by a second build (Phase 9 lane C, slice 1,
    /// fork 2A). Derivable state like the stamps themselves, so it is not in
    /// the `.vox` cache either: a loaded field has no stamps to attribute from
    /// and says so.
    op: usize,
    sil: Silhouette,
    path: PathK,
    /// Swept-solid bounding box (z max is unbounded — the rod). Culling
    /// leans on `sweep ⊆ aabb`, so it is generous, never tight-and-wrong.
    aabb: [f64; 6],
    /// Certified under-claim of this stamp's distance formula versus the true
    /// distance for far points: formula >= dist(p, aabb) - eps. Zero for the
    /// exact arms; the sloped arm's sampling error otherwise.
    eps: f64,
}

// ---- exact plan/space distance primitives -----------------------------------

fn dist2d(px: f64, py: f64, qx: f64, qy: f64) -> f64 {
    (px - qx).phypot(py - qy)
}

pub(crate) fn dist2d_seg(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let (ex, ey) = (bx - ax, by - ay);
    let len2 = ex * ex + ey * ey;
    let t = if len2 > 0.0 {
        (((px - ax) * ex + (py - ay) * ey) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist2d(px, py, ax + t * ex, ay + t * ey)
}

fn dist3d_seg(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len2 = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * e[0] + (p[1] - a[1]) * e[1] + (p[2] - a[2]) * e[2]) / len2)
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q = [a[0] + t * e[0], a[1] + t * e[1], a[2] + t * e[2]];
    let d = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Exact 2D distance from a point to a circular arc (curve, not disc).
/// Exact everywhere — the design entry's eikonal degeneracy past an arc's
/// center is a property of the level sets in 3D composition, not of this
/// planar distance, which handles the center like any other point.
fn dist2d_arc(px: f64, py: f64, cx: f64, cy: f64, ra: f64, a0: f64, sweep: f64) -> f64 {
    let (dx, dy) = (px - cx, py - cy);
    let rho = dx.phypot(dy);
    if sweep.abs() >= 2.0 * std::f64::consts::PI - 1e-12 {
        return (rho - ra).abs();
    }
    let ang = dy.patan2(dx);
    // Angular travel from the start, measured in the sweep's own direction.
    let travel = if sweep >= 0.0 {
        (ang - a0).rem_euclid(2.0 * std::f64::consts::PI)
    } else {
        (a0 - ang).rem_euclid(2.0 * std::f64::consts::PI)
    };
    if travel <= sweep.abs() {
        (rho - ra).abs()
    } else {
        let a1 = a0 + sweep;
        let e0 = dist2d(px, py, cx + ra * a0.pcos(), cy + ra * a0.psin());
        let e1 = dist2d(px, py, cx + ra * a1.pcos(), cy + ra * a1.psin());
        e0.min(e1)
    }
}

// ---- the shaped silhouette in the meridian half-plane -------------------------
//
// A profile tool's swept cross-section is a POLYGON in (rho, h) — h measured
// above the tip — rather than the rods' half-infinite rectangle, and the two
// functions below are the whole of the difference (DECISIONS.md → "the profile
// silhouette…", slice 2). The composition around them is unchanged: rho is the
// plan distance to the path, h is the height above the tip, and membership
// factors through the pair exactly as the header describes.
//
// `lift` extends the top cap upward at the top radius. A vertical move's union
// of truncated instances is NOT the lowest instance any more — that collapse
// was the rod's semi-infinite property — but for a silhouette that never
// narrows going up (the fence the walker enforces) the union is exactly the
// lowest instance with its cap extended by the travel, which keeps the Point
// stamp one closed form instead of a sampled sweep.

/// How wide the swept cross-section is `h` above the tip: the silhouette,
/// truncated at its cutting top, with the top cap extended by `lift`.
fn meridian_r(pts: &[Pt], lift: f64, h: f64) -> f64 {
    let top = pts[pts.len() - 1];
    if h > top.y {
        return if h <= top.y + lift { top.x } else { 0.0 };
    }
    if h < 0.0 {
        return 0.0;
    }
    if h <= pts[0].y {
        return pts[0].x;
    }
    for pair in pts.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        if h <= hi.y {
            return lo.x + (hi.x - lo.x) * (h - lo.y) / (hi.y - lo.y);
        }
    }
    top.x
}

/// Signed distance from `(rho, h)` to that cross-section, negative inside.
/// Exact and 1-Lipschitz: the boundary is walked as the chain
/// axis → tip disc → the profile → the (lifted) top cap → axis, and the axis
/// edge itself is left out because it is interior to the solid of revolution,
/// not a wall of it.
fn meridian_d(pts: &[Pt], lift: f64, rho: f64, h: f64) -> f64 {
    let top = pts[pts.len() - 1];
    let cap_h = top.y + lift;
    let mut best = dist2d_seg(rho, h, 0.0, 0.0, pts[0].x, 0.0); // the tip disc
    let mut prev = (pts[0].x, 0.0);
    for p in pts.iter().filter(|p| p.y > 0.0) {
        best = best.min(dist2d_seg(rho, h, prev.0, prev.1, p.x, p.y));
        prev = (p.x, p.y);
    }
    if lift > 0.0 {
        best = best.min(dist2d_seg(rho, h, prev.0, prev.1, top.x, cap_h));
        prev = (top.x, cap_h);
    }
    best = best.min(dist2d_seg(rho, h, prev.0, prev.1, 0.0, cap_h)); // the cap
    let inside = h >= 0.0 && h <= cap_h && rho <= meridian_r(pts, lift, h);
    if inside {
        -best
    } else {
        best
    }
}

/// Is `p` inside the swept solid of a shaped silhouette along the sloped move
/// `a + t*e`, `t` in [0, 1]? Exact.
///
/// Band by band: over one profile segment the tip height `h(t) = pz - q_z(t)`
/// is linear in `t`, so the segment is active on a t-interval and its radius
/// `r(t) = alpha + beta*t` is linear there. Membership on the band is then
/// `|p_xy - q_xy(t)|² - r(t)² <= 0` — one quadratic, whose minimum over the
/// window is at an endpoint or (when it opens upward) its vertex. Radii are
/// non-negative by construction, so squaring loses no root. The rod's single
/// covering interval is this with one band and `beta = 0`.
fn shaped_slope_inside(px: f64, py: f64, pz: f64, a: [f64; 3], e: [f64; 3], pts: &[Pt]) -> bool {
    let (dx, dy) = (px - a[0], py - a[1]);
    let a_xy = e[0] * e[0] + e[1] * e[1];
    let b_xy = dx * e[0] + dy * e[1];
    let c_xy = dx * dx + dy * dy;
    let h0 = pz - a[2]; // height above the tip at t = 0
    let ez = e[2];

    let n = pts.len();
    let bottom = pts[0].y > 0.0; // a profile that starts above the tip plane
    let bands = if bottom { n } else { n - 1 };
    for k in 0..bands {
        let (h_lo, h_hi, r_lo, r_hi) = match (bottom, k) {
            // The tip disc's column: constant radius from the tip up to the
            // profile's first vertex.
            (true, 0) => (0.0, pts[0].y, pts[0].x, pts[0].x),
            (true, k) => (pts[k - 1].y, pts[k].y, pts[k - 1].x, pts[k].x),
            (false, k) => (pts[k].y, pts[k + 1].y, pts[k].x, pts[k + 1].x),
        };
        if h_hi <= h_lo {
            continue; // a horizontal step: measure zero in h, and its neighbours bound it
        }
        // The t-window on which this band is the active one.
        let (mut lo, mut hi) = if ez.abs() <= PATH_EPS {
            if h0 < h_lo || h0 > h_hi {
                continue;
            }
            (0.0, 1.0)
        } else {
            let (t_a, t_b) = ((h0 - h_lo) / ez, (h0 - h_hi) / ez);
            (t_a.min(t_b), t_a.max(t_b))
        };
        lo = lo.max(0.0);
        hi = hi.min(1.0);
        if lo > hi {
            continue;
        }
        let slope = (r_hi - r_lo) / (h_hi - h_lo);
        let alpha = r_lo + slope * (h0 - h_lo);
        let beta = -slope * ez;
        let (qp, qq, qr) = (
            a_xy - beta * beta,
            -2.0 * (b_xy + alpha * beta),
            c_xy - alpha * alpha,
        );
        let g = |t: f64| qp * t * t + qq * t + qr;
        if g(lo) <= 0.0 || g(hi) <= 0.0 {
            return true;
        }
        if qp > 0.0 {
            let vertex = -qq / (2.0 * qp);
            if vertex > lo && vertex < hi && g(vertex) <= 0.0 {
                return true;
            }
        }
    }
    false
}

/// Signed distance to `plan region x [z0, +inf)` given the signed plan
/// distance and `below = z0 - pz`. Exact for ANY plan region: the outside
/// corner is the plan rim at the floor height, hence the hypot.
fn rod_d(plan_d: f64, below: f64) -> f64 {
    if plan_d > 0.0 && below > 0.0 {
        plan_d.phypot(below)
    } else {
        plan_d.max(below)
    }
}

/// `rod_d` with a ceiling: signed distance to `plan region x [z0, z1]`, given
/// `below = z0 - pz` and `above = pz - z1` — the truncated rod's own form (K2
/// slice 2, fork 5A: a declared `flute_len` caps the sweep). Exact by the same
/// rim argument, top and bottom; `below` and `above` cannot both be positive
/// while `z0 < z1`, so the max picks the one live gap.
fn rod_d_capped(plan_d: f64, below: f64, above: f64) -> f64 {
    let dz = below.max(above);
    if plan_d > 0.0 && dz > 0.0 {
        plan_d.phypot(dz)
    } else {
        plan_d.max(dz)
    }
}

impl Stamp {
    /// Signed distance to the swept solid: negative inside (void), positive
    /// outside. Sign exact everywhere; magnitude exact except a sloped move's
    /// certified bound (see the header).
    fn d_swept(&self, px: f64, py: f64, pz: f64) -> f64 {
        let (r, ball, cap) = match self.rod() {
            Some(rod) => rod,
            None => return self.d_shaped(px, py, pz),
        };
        match &self.path {
            PathK::Point { x, y, z, lift } => {
                let plan = dist2d(px, py, *x, *y) - r;
                match cap {
                    None => {
                        let rod = rod_d(plan, z - pz);
                        if ball {
                            rod.min(dist3d_seg([px, py, pz], [*x, *y, *z], [*x, *y, *z]) - r)
                        } else {
                            rod
                        }
                    }
                    // A vertical move's truncated union is the lowest
                    // instance with its cap lifted by the travel — the same
                    // collapse the shaped Point stamp uses, exact here
                    // because a rod never narrows going up.
                    Some(c) => {
                        let cap_z = z + lift + c;
                        let rod = rod_d_capped(plan, z - pz, pz - cap_z);
                        if ball {
                            // The ball piece guarded at the cap: with flutes
                            // shorter than the ball's own diameter the sphere
                            // would poke above the cap, and an unguarded min
                            // would claim that steel as void. Below the cap
                            // the sphere is authoritative (its upper half
                            // there sits inside the cylinder piece anyway).
                            rod.min(
                                (dist3d_seg([px, py, pz], [*x, *y, *z], [*x, *y, *z]) - r)
                                    .max(pz - cap_z),
                            )
                        } else {
                            rod
                        }
                    }
                }
            }
            PathK::HSeg { ax, ay, bx, by, z } => {
                let plan = dist2d_seg(px, py, *ax, *ay, *bx, *by) - r;
                match cap {
                    None => {
                        let rod = rod_d(plan, z - pz);
                        if ball {
                            rod.min(dist3d_seg([px, py, pz], [*ax, *ay, *z], [*bx, *by, *z]) - r)
                        } else {
                            rod
                        }
                    }
                    Some(c) => {
                        let cap_z = z + c;
                        let rod = rod_d_capped(plan, z - pz, pz - cap_z);
                        if ball {
                            rod.min(
                                (dist3d_seg([px, py, pz], [*ax, *ay, *z], [*bx, *by, *z]) - r)
                                    .max(pz - cap_z),
                            )
                        } else {
                            rod
                        }
                    }
                }
            }
            PathK::HArc { cx, cy, ra, a0, sweep, z } => {
                let arc2d = dist2d_arc(px, py, *cx, *cy, *ra, *a0, *sweep);
                match cap {
                    None => {
                        let rod = rod_d(arc2d - r, z - pz);
                        if ball {
                            // The ball's sweep along a constant-Z arc is the exact
                            // arc-capsule: the 3D distance to a planar curve is the
                            // hypot of its plan distance and the height gap.
                            rod.min(arc2d.phypot(pz - z) - r)
                        } else {
                            rod
                        }
                    }
                    Some(c) => {
                        let cap_z = z + c;
                        let rod = rod_d_capped(arc2d - r, z - pz, pz - cap_z);
                        if ball {
                            rod.min((arc2d.phypot(pz - z) - r).max(pz - cap_z))
                        } else {
                            rod
                        }
                    }
                }
            }
            PathK::Slope { a, b } => self.d_slope(px, py, pz, *a, *b, r, ball, cap),
        }
    }

    /// The two closed-form rods, as `(radius, is_ball, cap)`. `None` for a
    /// shaped tool — a drawn profile or a ground form's derived body — which
    /// takes the shaped arm below. `cap` is the flute ceiling above the PATH
    /// coordinate (`cutting_top - drop`, so a ball's is measured from its
    /// emitted center): `Some` only when the program declared a `flute_len`
    /// (K2 slice 2, fork 5A), and `None` keeps the semi-infinite arms
    /// byte-for-byte — an undeclared body is today's behavior, never a guess.
    fn rod(&self) -> Option<(f64, bool, Option<f64>)> {
        let cap = self.sil.cutting_top().map(|t| t - self.sil.drop());
        match self.sil {
            Silhouette::Flat { r, .. } => Some((r, false, cap)),
            Silhouette::Ball { r, .. } => Some((r, true, cap)),
            Silhouette::Profile { .. } | Silhouette::Rounder { .. } | Silhouette::Chamfer { .. } => None,
        }
    }

    /// The shaped stamp: the same composition the rods use, with the
    /// half-infinite rectangle replaced by the profile's own cross-section.
    /// Plan distance is unchanged, so `Point` and `HSeg` stay exact and `HArc`
    /// inherits precisely the rod arm's status (the composition is a certified
    /// lower bound, exact away from the level-set degeneracy past an arc's
    /// centre that the rod arm already documents).
    fn d_shaped(&self, px: f64, py: f64, pz: f64) -> f64 {
        // One meridian polygon, whether it was drawn (a `FormTool`'s SVG) or
        // derived (a `CornerRounder`'s pilot-arc-shoulder body): the stamp does
        // not care which, and asking `Silhouette` for the polygon rather than
        // matching the arm is what keeps it that way.
        let Some((pts, _)) = self.sil.body_pts() else {
            return f64::INFINITY;
        };
        match &self.path {
            PathK::Point { x, y, z, lift } => {
                meridian_d(pts, *lift, dist2d(px, py, *x, *y), pz - z)
            }
            PathK::HSeg { ax, ay, bx, by, z } => {
                meridian_d(pts, 0.0, dist2d_seg(px, py, *ax, *ay, *bx, *by), pz - z)
            }
            PathK::HArc { cx, cy, ra, a0, sweep, z } => {
                meridian_d(pts, 0.0, dist2d_arc(px, py, *cx, *cy, *ra, *a0, *sweep), pz - z)
            }
            PathK::Slope { a, b } => self.d_shaped_slope(px, py, pz, *a, *b, pts),
        }
    }

    /// A sloped move under a shaped silhouette. Membership stays EXACT: the
    /// rod's single covering interval becomes one quadratic per profile band,
    /// because on a band the tool's radius is linear in the move parameter
    /// (the profile is piecewise linear and the tip's height is linear in t),
    /// so `|p_xy - q_xy(t)|² ≤ r(t)²` is again a quadratic — solved as a
    /// minimum over the band's own t-window rather than by finding its roots.
    /// The magnitude keeps the rod arm's certified sampling bound, with the
    /// per-instance distance now the cross-section's.
    fn d_shaped_slope(&self, px: f64, py: f64, pz: f64, a: [f64; 3], b: [f64; 3], pts: &[Pt]) -> f64 {
        let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len3 = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
        let inside = shaped_slope_inside(px, py, pz, a, e, pts);

        let inst = |t: f64| -> f64 {
            let q = [a[0] + t * e[0], a[1] + t * e[1], a[2] + t * e[2]];
            meridian_d(pts, 0.0, dist2d(px, py, q[0], q[1]), pz - q[2])
        };
        let mut best = f64::INFINITY;
        for i in 0..SLOPE_SAMPLES {
            best = best.min(inst(i as f64 / (SLOPE_SAMPLES - 1) as f64));
        }
        let eps = len3 / (2.0 * (SLOPE_SAMPLES - 1) as f64);

        if inside {
            best.min(-f64::EPSILON)
        } else {
            // The sweep sits inside the stadium of the tool's widest radius
            // crossed with [lowest tip, highest tip + cutting top]; the exact
            // distance to that superset is a certified lower bound.
            let top = pts[pts.len() - 1].y;
            let wide = pts.iter().map(|p| p.x).fold(0.0f64, f64::max);
            let plan = (dist2d_seg(px, py, a[0], a[1], b[0], b[1]) - wide).max(0.0);
            let z_gap = ((a[2].min(b[2])) - pz).max(pz - (a[2].max(b[2]) + top)).max(0.0);
            let hull = plan.phypot(z_gap);
            (best - eps).max(hull).max(f64::EPSILON)
        }
    }

    /// The sloped-move stamp. Membership (the sign) is exact via the covering
    /// interval; the magnitude is the sampled-instance bound, never
    /// over-stated on either side.
    fn d_slope(
        &self,
        px: f64,
        py: f64,
        pz: f64,
        a: [f64; 3],
        b: [f64; 3],
        r: f64,
        ball: bool,
        cap: Option<f64>,
    ) -> f64 {
        let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len3 = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();

        // --- exact membership: the covering interval -------------------------
        // |p_xy - q_xy(t)|^2 <= r^2 is one quadratic in t; the interval's two
        // endpoints are the advancing edge and the dragging heel, and the rod
        // floor over p is the lower of the two endpoint heights (tip Z is
        // linear in t, so the min over the interval sits at an endpoint).
        // Under a flute cap the instance is a slab [q_z, q_z + cap] instead of
        // a half-line, and existence over the interval reads both ends of the
        // path-height range — still endpoints, because q_z is linear in t.
        let (qa, qb, qc) = {
            let (dx, dy) = (px - a[0], py - a[1]);
            let aa = e[0] * e[0] + e[1] * e[1];
            let bb = -2.0 * (dx * e[0] + dy * e[1]);
            let cc = dx * dx + dy * dy - r * r;
            (aa, bb, cc)
        };
        let mut interval: Option<(f64, f64)> = None;
        let disc = qb * qb - 4.0 * qa * qc;
        if qa > 0.0 && disc >= 0.0 {
            let s = disc.sqrt();
            let t0 = ((-qb - s) / (2.0 * qa)).max(0.0);
            let t1 = ((-qb + s) / (2.0 * qa)).min(1.0);
            if t0 <= t1 {
                interval = Some((t0, t1));
            }
        }
        let inside_rod = match interval {
            Some((t0, t1)) => {
                let (za, zb) = (a[2] + t0 * e[2], a[2] + t1 * e[2]);
                let floor = za.min(zb);
                match cap {
                    None => pz >= floor,
                    // Some covering instance's slab holds pz iff the slabs'
                    // union over the interval does: [floor, ceil + cap].
                    Some(c) => pz >= floor && pz <= za.max(zb) + c,
                }
            }
            None => false,
        };
        let inside_ball = ball
            && match cap {
                None => dist3d_seg([px, py, pz], a, b) <= r,
                // A truncated ball's spherical piece is its LOWER half — the
                // upper half is the cylinder's business, and above the cap it
                // is not steel at all — so membership needs an instance that
                // both reaches p and sits at or above it: the t-window of
                // |p - q(t)| <= r is one quadratic, its center-height range
                // has its max at an endpoint (linear again).
                Some(_) => {
                    let (dx, dy, dz) = (px - a[0], py - a[1], pz - a[2]);
                    let aa = e[0] * e[0] + e[1] * e[1] + e[2] * e[2];
                    let bb = -2.0 * (dx * e[0] + dy * e[1] + dz * e[2]);
                    let cc = dx * dx + dy * dy + dz * dz - r * r;
                    let disc = bb * bb - 4.0 * aa * cc;
                    if aa > 0.0 && disc >= 0.0 {
                        let s = disc.sqrt();
                        let s0 = ((-bb - s) / (2.0 * aa)).max(0.0);
                        let s1 = ((-bb + s) / (2.0 * aa)).min(1.0);
                        s0 <= s1 && (a[2] + s0 * e[2]).max(a[2] + s1 * e[2]) >= pz
                    } else {
                        false
                    }
                }
            };
        let inside = inside_rod || inside_ball;

        // --- safe-side magnitude: sampled instances --------------------------
        // Per-instance signed distance is 1-Lipschitz in (t * len3), so a
        // uniform K-sample min misses at most len3/(2(K-1)); that error is
        // subtracted on the outside branch and needs no correction inside
        // (a missed deeper instance only makes the claim shallower — the
        // under-claiming direction). The covering-interval endpoints join the
        // samples so interior points always see a covering instance.
        let inst = |t: f64| -> f64 {
            let q = [a[0] + t * e[0], a[1] + t * e[1], a[2] + t * e[2]];
            let plan = dist2d(px, py, q[0], q[1]) - r;
            match cap {
                None => {
                    let rod = rod_d(plan, q[2] - pz);
                    if ball {
                        let d = [px - q[0], py - q[1], pz - q[2]];
                        rod.min((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - r)
                    } else {
                        rod
                    }
                }
                Some(c) => {
                    let rod = rod_d_capped(plan, q[2] - pz, pz - (q[2] + c));
                    if ball {
                        let d = [px - q[0], py - q[1], pz - q[2]];
                        rod.min(
                            ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - r)
                                .max(pz - (q[2] + c)),
                        )
                    } else {
                        rod
                    }
                }
            }
        };
        let mut best = f64::INFINITY;
        for i in 0..SLOPE_SAMPLES {
            best = best.min(inst(i as f64 / (SLOPE_SAMPLES - 1) as f64));
        }
        if let Some((t0, t1)) = interval {
            best = best.min(inst(t0)).min(inst(t1)).min(inst(0.5 * (t0 + t1)));
        }
        let eps = len3 / (2.0 * (SLOPE_SAMPLES - 1) as f64);

        if inside {
            // Under-claim the depth; a positive sampled min at a genuinely
            // interior point (possible only on the boundary's measure-zero
            // skin) still must not flip the sign.
            best.min(-f64::EPSILON)
        } else {
            // Certified lower bounds of the true distance: the sampled min
            // less its sampling error, and the stadium-times-slab hull (a
            // halfspace when no cap bounds the sweep above).
            let plan = dist2d_seg(px, py, a[0], a[1], b[0], b[1]) - r;
            let z_bottom = a[2].min(b[2]) - self.sil.drop();
            let hull = match cap {
                None => rod_d(plan.max(0.0), z_bottom - pz),
                Some(c) => rod_d_capped(plan.max(0.0), z_bottom - pz, pz - (a[2].max(b[2]) + c)),
            };
            (best - eps).max(hull).max(f64::EPSILON)
        }
    }

    fn intersects(&self, lo: [f64; 3], hi: [f64; 3], pad: f64) -> bool {
        self.aabb[0] - pad <= hi[0]
            && self.aabb[3] + pad >= lo[0]
            && self.aabb[1] - pad <= hi[1]
            && self.aabb[4] + pad >= lo[1]
            && self.aabb[2] - pad <= hi[2]
            && self.aabb[5] + pad >= lo[2]
    }
}

// ---- the field ---------------------------------------------------------------

enum Node {
    Air,
    Material,
    /// One 8x8x8 brick of finest cells, x-fastest.
    Leaf(Box<[f32; 512]>),
    Branch(Box<[Node; 8]>),
}

/// The machined stock's surface as a welded, indexed triangle mesh —
/// `VoxField::iso_surface`'s answer. World millimetres; `normals[i]` belongs
/// to `verts[i]` and points OUT of material; `tris` indexes both.
#[derive(Debug, Default)]
pub struct IsoSurface {
    pub verts: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub tris: Vec<[u32; 3]>,
}

/// Every leaf brick with the lattice index of its minimum corner. Collapsed
/// nodes are skipped, which is the whole speed of the walk: only bricks hold
/// a band, and only a band can hold a crossing.
fn each_leaf(node: &Node, min: [usize; 3], node_cells: usize, f: &mut impl FnMut([usize; 3], &[f32; 512])) {
    match node {
        Node::Leaf(brick) => f(min, brick),
        Node::Branch(ch) => {
            let half = node_cells / 2;
            for (oct, child) in ch.iter().enumerate() {
                let m = [
                    min[0] + if oct & 1 != 0 { half } else { 0 },
                    min[1] + if oct & 2 != 0 { half } else { 0 },
                    min[2] + if oct & 4 != 0 { half } else { 0 },
                ];
                each_leaf(child, m, half, f);
            }
        }
        Node::Air | Node::Material => {}
    }
}

pub struct VoxField {
    /// Root cube min corner; the cube's side is `cells * cell`.
    origin: [f64; 3],
    /// Finest cells per root edge (a power of two, >= 8).
    cells: usize,
    /// Finest cell side, mm.
    cell: f64,
    /// Truncation distance, mm; stored values live in [-tau, tau].
    tau: f64,
    root: Node,
    /// Present on a built field, absent on one loaded from a `.vox` cache —
    /// the stamps are derivable state and are deliberately not serialized, so
    /// a loaded field answers queries from its tree alone and `flat_value`
    /// honestly declines rather than evaluating against nothing.
    stamps: Option<Vec<Stamp>>,
    /// The air at prefix 0 that is not the blank's outside — what is GIVEN
    /// (Phase 42) and what the program before this one cut (`given from`,
    /// Phase 48 slice 1), as the stock model's own prefix-0 removals read
    /// once at build into columns the composite can take a signed distance
    /// from. Build-time state like `stamps`: derivable, never serialized —
    /// the cache key takes it in instead.
    prefix_air: Option<Vec<AirColumn>>,
    stock: Stock,
    /// How many leading ops this field has stamped: the field is the stock as
    /// it stands BEFORE op `prefix_ops` (0 = the untouched stock, the op
    /// count = the end state). DECISIONS.md "adaptive clearing — the field
    /// picks the passes…", slice 0: the before-this-op state is what
    /// `strategy=rest` will select against.
    prefix_ops: usize,
    /// The FNV-1a hash of everything the field is a function of (the resolved
    /// cell, the stock, the tool table, the stamped PREFIX — each op's moves
    /// and stamp bindings — and the format version) — computed at build,
    /// written into the cache, and the cache's whole validity story: on
    /// mismatch the file is recomputed, not trusted.
    input_hash: u64,
    /// Whether every stamp in this field reaches forever upward — true unless
    /// a truncated (form-cutter) silhouette contributed, since a shank removes
    /// nothing and its column is left standing (DECISIONS.md → "the profile
    /// silhouette…", slice 2). It rides the cache because a loaded field has no
    /// stamps to re-derive it from. The one consumer that leans on the lemma is
    /// `band_certified_void`; everything else walks its own ladder.
    upward_closed: bool,
    /// One entry per thing the walker could not stamp exactly — a control-comp
    /// op, a necked silhouette, a move it could not place. Empty means complete.
    /// The reasons are the flag (one value, not a bool beside a list that
    /// could drift — the DRY-for-derived-values rule), and they exist so a
    /// consumer that must decline can say WHY: total accounting reports the
    /// gap, and an actionable gap names itself.
    pub incomplete_reasons: Vec<String>,
}

impl VoxField {
    /// Build the field for a part at the derived default resolution.
    pub fn build(p: &Part) -> Result<VoxField, String> {
        Self::build_with_cell(p, None)
    }

    /// Build with an explicit finest-cell size (mm). The default derivation
    /// (one home): the stock's longest edge over 2^8, then rounded down to
    /// the power-of-two subdivision that fits it. The end state is the
    /// whole-program prefix — one build path, not two.
    pub fn build_with_cell(p: &Part, cell: Option<f64>) -> Result<VoxField, String> {
        Self::build_prefix(p, p.operations.len(), cell)
    }

    /// The stock as it stands BEFORE op `op_idx`: emissions `0..op_idx`
    /// stamped, everything after left standing (DECISIONS.md "adaptive
    /// clearing — the field picks the passes…", slice 0). `op_idx == 0` is
    /// the untouched stock; `op_idx == p.operations.len()` is the end state.
    /// The FULL program is still emitted and validated — a prefix is a view
    /// of a valid program, never a way to dodge a downstream error — and
    /// `incomplete_reasons` are the prefix's own: an unstampable op past the
    /// prefix does not poison the state before it (the incompleteness law
    /// slice 1's error reads is exactly this field's).
    pub fn build_prefix(p: &Part, op_idx: usize, cell: Option<f64>) -> Result<VoxField, String> {
        if op_idx > p.operations.len() {
            return Err(format!(
                "voxel field: prefix {op_idx} exceeds the program's {} ops",
                p.operations.len()
            ));
        }
        let (_, emissions) = gcode::program_with_moves(p)?;
        Self::build_prefix_from_emissions(p, &emissions, op_idx, cell)
    }

    /// `build_prefix` for a caller that already holds the emissions — the
    /// emission loop itself (`gcode::program_with_moves` builds the before-op-k
    /// field for a `strategy=rest` op from the k ops it has ALREADY emitted;
    /// re-emitting the program from inside its own emission would recurse).
    /// Emissions past `op_idx` are ignored, so handing the emitted moves-so-far and
    /// handing the full emitted moves are the same call.
    pub(crate) fn build_prefix_from_emissions(
        p: &Part,
        emissions: &[gcode::OpEmission],
        op_idx: usize,
        cell: Option<f64>,
    ) -> Result<VoxField, String> {
        Self::build_prefix_moves(p, emissions, op_idx, None, cell)
    }

    /// The prefix at MOVE grain (DESIGN.md §5.6, Phase 52 slice 6): ops
    /// `0..op_idx` stamped whole and, with `through = Some(b)`, op `op_idx`
    /// stamped through its move `b` — the stock as it stands while the
    /// block cursor is on that move. `None` is `build_prefix`. The field's
    /// `prefix_ops` stays `op_idx` (what stands BEFORE the op, plus the
    /// moves) and its hash takes the move count in, so a partial field can
    /// never be mistaken for a cached whole one. `through` indexes the
    /// channel the walker WALKS (`gcode::resolved_moves`): the text for an
    /// engine-comp op, the resolved centerline under control comp — where
    /// the caller turns the text's move into the resolved move its foot is
    /// on (`view::resolved_foot`, Phase 52 slice 10).
    pub fn build_prefix_moves(
        p: &Part,
        emissions: &[gcode::OpEmission],
        op_idx: usize,
        through: Option<usize>,
        cell: Option<f64>,
    ) -> Result<VoxField, String> {
        let s = &p.stock;
        let (cells, cell_mm) = resolve_cells(s, cell)?;
        let tau = TAU_CELLS * cell_mm;

        let (stamps, mut incomplete_reasons) = walk_emissions(p, emissions, op_idx, through);
        let mut input_hash = input_hash(p, cell_mm, emissions, op_idx);
        if let Some(b) = through {
            fnv1a(&mut input_hash, &(b as u64 + 1).to_le_bytes());
        }
        let (prefix_air, air_reasons) = prefix_air_columns(p);
        // Air at prefix 0 the field cannot measure is a gap before any op:
        // it goes first, and it is a reason like any stamp's (slice 5).
        incomplete_reasons.splice(0..0, air_reasons);
        // The column-contiguity lemma needs every air column to reach the
        // stock top as well: a void the earlier program cut from the far
        // side, turned over, is a cavity under this top (Phase 48 slice 1).
        let upward_closed = stamps.iter().all(|s| s.sil.cutting_top().is_none())
            && prefix_air.iter().all(|c| c.z_hi >= s.z - PATH_EPS);

        let mut field = VoxField {
            origin: [s.x, s.y, s.z - s.h],
            cells,
            cell: cell_mm,
            tau,
            root: Node::Air,
            stamps: Some(stamps),
            prefix_air: Some(prefix_air),
            stock: s.clone(),
            prefix_ops: op_idx,
            input_hash,
            upward_closed,
            incomplete_reasons,
        };
        let n = field.stamps.as_ref().map_or(0, |v| v.len());
        let idx: Vec<usize> = (0..n).collect();
        field.root = field.build_node(field.origin, field.cells, &idx);
        Ok(field)
    }

    /// The stock as material: the blank's signed distance, negative inside —
    /// a box's three axes, or a round's radius and height (Phase 46 slice 1:
    /// outside the round is air at prefix 0, which is the whole reason the
    /// shape exists).
    fn stock_sdf(&self, px: f64, py: f64, pz: f64) -> f64 {
        let s = &self.stock;
        let dz = ((s.z - s.h) - pz).max(pz - s.z);
        let ds: [f64; 3] = match s.shape {
            StockShape::Box => [(s.x - px).max(px - (s.x + s.w)), (s.y - py).max(py - (s.y + s.l)), dz],
            StockShape::Cylinder { dia } => {
                let (cx, cy) = s.axis();
                // One plan distance; the second axis never limits.
                [(px - cx).phypot(py - cy) - dia / 2.0, f64::NEG_INFINITY, dz]
            }
            // The loop's signed plan distance (Phase 54 slice 5a): negative
            // inside, positive outside, by the polygon helpers.
            StockShape::Profile { ref points } => {
                let d = crate::stock::dist_to_polygon(points, px, py);
                [if crate::stock::point_in_polygon(points, px, py) { -d } else { d }, f64::NEG_INFINITY, dz]
            }
        };
        if ds.iter().all(|d| *d <= 0.0) {
            ds.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        } else {
            ds.iter().map(|d| d.max(0.0).powi(2)).sum::<f64>().sqrt()
        }
    }

    /// The untruncated composite over a stamp subset, the air at prefix 0
    /// first. `max` is exact for the
    /// difference stock-minus-sweeps in the directions that matter: material
    /// is a subset of every sweep's complement, so each `-d` under-states the
    /// distance to material, and the max of under-statements still
    /// under-states (Finding 5's caveat, the safe direction).
    fn raw_value(&self, px: f64, py: f64, pz: f64, which: &[usize]) -> f64 {
        let stamps = self
            .stamps
            .as_deref()
            .expect("raw_value is the builder's and the pin's; loaded fields decline upstream");
        let mut f = self.stock_sdf(px, py, pz);
        // The air at prefix 0 composes exactly as a sweep does: material is
        // the blank minus every column, so `max(-d_column)` under-states
        // nothing and stays 1-Lipschitz (Phase 48 slice 1).
        if let Some(air) = self.prefix_air.as_deref() {
            f = f.max(-prefix_air_sdf(air, px, py, pz, self.tau));
        }
        for &i in which {
            f = f.max(-stamps[i].d_swept(px, py, pz));
        }
        f
    }

    /// Which move swept this point, and what kind of move it was: the stamp
    /// standing furthest INSIDE at the point — the argmax of the very
    /// `max(-d_swept)` that `raw_value` takes, so the op named is the one
    /// whose sweep the field's own value at that cell came from. `None` when
    /// no stamp reaches the point (the field reads air there for another
    /// reason, or the point is outside every sweep) and on a field loaded
    /// from a `.vox` cache, whose stamps are not serialized — an honest "not
    /// attributed", never a guess (Phase 9 lane C, slice 1).
    ///
    /// Culled by the stamp's own aabb with its certified eps, the same
    /// contract the octree build leans on, so the cost is the stamps that can
    /// reach the point rather than every move in the program.
    pub fn deepest_sweep_at(&self, px: f64, py: f64, pz: f64) -> Option<(usize, &'static str)> {
        let stamps = self.stamps.as_ref()?;
        let mut best: Option<(f64, &Stamp)> = None;
        for s in stamps {
            if !s.intersects([px, py, pz], [px, py, pz], s.eps) {
                continue;
            }
            let d = s.d_swept(px, py, pz);
            if d < 0.0 && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, s));
            }
        }
        best.map(|(_, s)| {
            let kind = match s.path {
                PathK::Point { lift, .. } if lift > PATH_EPS => "a plunge",
                PathK::Point { .. } => "a dwell",
                PathK::HSeg { .. } | PathK::HArc { .. } => "a level pass",
                PathK::Slope { .. } => "a ramp",
            };
            (s.op, kind)
        })
    }

    /// The unaccelerated evaluation the octree must match exactly: the
    /// truncated composite over ALL stamps — the brute-force pin's other
    /// side. `None` on a field loaded from a cache: its stamps were derivable
    /// state and were not serialized, and evaluating against nothing would be
    /// a silent lie rather than a cheap answer.
    pub fn flat_value(&self, px: f64, py: f64, pz: f64) -> Option<f32> {
        let stamps = self.stamps.as_ref()?;
        let idx: Vec<usize> = (0..stamps.len()).collect();
        Some(self.raw_value(px, py, pz, &idx).clamp(-self.tau, self.tau) as f32)
    }

    fn build_node(&self, min: [f64; 3], node_cells: usize, stamps: &[usize]) -> Node {
        let side = node_cells as f64 * self.cell;
        let half_diag = side * 0.5 * 3f64.sqrt();
        let hi = [min[0] + side, min[1] + side, min[2] + side];

        // Culling is exact under truncation: an excluded stamp satisfies
        // d > tau + half_diag + eps everywhere in the cube (sweep ⊆ aabb and
        // the formula under-claims by at most eps), so its `-d` can neither
        // cross the truncation floor nor win the max mid-band.
        let all = self
            .stamps
            .as_deref()
            .expect("build_node runs only during build, when the stamps exist");
        let local: Vec<usize> = stamps
            .iter()
            .copied()
            .filter(|&i| {
                let s = &all[i];
                s.intersects(min, hi, self.tau + half_diag + s.eps)
            })
            .collect();

        let c = [min[0] + side * 0.5, min[1] + side * 0.5, min[2] + side * 0.5];
        let fc = self.raw_value(c[0], c[1], c[2], &local);
        // The composite is 1-Lipschitz (every arm is — the header's audit),
        // so the center value plus the half-diagonal brackets the cube.
        if fc - half_diag > self.tau {
            return Node::Air;
        }
        if fc + half_diag < -self.tau {
            return Node::Material;
        }
        if node_cells == 8 {
            let mut brick = Box::new([0f32; 512]);
            for k in 0..8 {
                for j in 0..8 {
                    for i in 0..8 {
                        let p = [
                            min[0] + (i as f64 + 0.5) * self.cell,
                            min[1] + (j as f64 + 0.5) * self.cell,
                            min[2] + (k as f64 + 0.5) * self.cell,
                        ];
                        brick[(k * 8 + j) * 8 + i] = self
                            .raw_value(p[0], p[1], p[2], &local)
                            .clamp(-self.tau, self.tau) as f32;
                    }
                }
            }
            return Node::Leaf(brick);
        }
        let half_cells = node_cells / 2;
        let hs = half_cells as f64 * self.cell;
        let mut children: Vec<Node> = Vec::with_capacity(8);
        for oct in 0..8 {
            let omin = [
                min[0] + if oct & 1 != 0 { hs } else { 0.0 },
                min[1] + if oct & 2 != 0 { hs } else { 0.0 },
                min[2] + if oct & 4 != 0 { hs } else { 0.0 },
            ];
            children.push(self.build_node(omin, half_cells, &local));
        }
        let arr: [Node; 8] = match children.try_into() {
            Ok(a) => a,
            Err(_) => unreachable!("eight octants were just built"),
        };
        Node::Branch(Box::new(arr))
    }

    /// Finest cells per root edge.
    pub fn cells(&self) -> usize {
        self.cells
    }

    /// Finest cell side, mm.
    /// The hash of everything this field is a function of — the cache's
    /// validity story, promoted to a reader so a CONSUMER can tell one field
    /// from another. The GPU mesh keys its upload on it: two fields with the
    /// same hash extract the same surface, so re-uploading is provably
    /// pointless, and a changed hash is exactly when it is not.
    pub fn id(&self) -> u64 {
        self.input_hash
    }

    pub fn cell(&self) -> f64 {
        self.cell
    }

    /// The truncation distance, mm.
    pub fn tau(&self) -> f64 {
        self.tau
    }

    /// How many leading ops this field has stamped: the field is the stock
    /// BEFORE op `prefix_ops` (0 = the untouched stock, the program's op
    /// count = the end state).
    pub fn prefix_ops(&self) -> usize {
        self.prefix_ops
    }

    pub fn cell_center(&self, i: usize, j: usize, k: usize) -> (f64, f64, f64) {
        (
            self.origin[0] + (i as f64 + 0.5) * self.cell,
            self.origin[1] + (j as f64 + 0.5) * self.cell,
            self.origin[2] + (k as f64 + 0.5) * self.cell,
        )
    }

    /// The truncated field at the cell containing the point (piecewise
    /// constant at cell granularity; outside the root cube is air).
    pub fn value(&self, px: f64, py: f64, pz: f64) -> f32 {
        let to_idx = |v: f64, o: f64| -> Option<usize> {
            let t = (v - o) / self.cell;
            if t < 0.0 || t >= self.cells as f64 {
                None
            } else {
                Some(t as usize)
            }
        };
        let (i, j, k) = match (
            to_idx(px, self.origin[0]),
            to_idx(py, self.origin[1]),
            to_idx(pz, self.origin[2]),
        ) {
            (Some(i), Some(j), Some(k)) => (i, j, k),
            _ => return self.tau as f32,
        };
        let mut node = &self.root;
        let mut cells = self.cells;
        let (mut bi, mut bj, mut bk) = (i, j, k);
        loop {
            match node {
                Node::Air => return self.tau as f32,
                Node::Material => return -self.tau as f32,
                Node::Leaf(brick) => return brick[(bk * 8 + bj) * 8 + bi],
                Node::Branch(ch) => {
                    let half = cells / 2;
                    let oct = (bi >= half) as usize | (((bj >= half) as usize) << 1) | (((bk >= half) as usize) << 2);
                    bi %= half;
                    bj %= half;
                    bk %= half;
                    cells = half;
                    node = &ch[oct];
                }
            }
        }
    }

    /// Occupancy is the sign.
    pub fn material_at(&self, px: f64, py: f64, pz: f64) -> bool {
        self.value(px, py, pz) < 0.0
    }

    /// Whether every stamp reaches forever upward — the column-contiguity
    /// lemma: under it, each XY column's remaining material is one interval
    /// from the stock's bottom, so the machined stock is exactly a
    /// heightfield. `band_certified_void` leans on it internally; the sim
    /// view (Phase 6) is the external consumer that must decline without it,
    /// which is why the flag is readable and not just private state.
    pub fn upward_closed(&self) -> bool {
        self.upward_closed
    }

    /// True when every op and move was stamped exactly. Derived from the
    /// reasons list — one value, not a bool that could drift beside it. A
    /// consumer that finds this false must make no claim about what remains.
    /// The incompleteness reasons as one line, for a pin's message.
    pub fn incomplete_reasons_text(&self) -> String {
        self.incomplete_reasons.join("; ")
    }

    /// How many stamps the field holds — the walk's count, for the pins.
    pub fn stamp_count(&self) -> usize {
        self.stamps.as_ref().map_or(0, |v| v.len())
    }

    pub fn complete(&self) -> bool {
        self.incomplete_reasons.is_empty()
    }

    /// One sample of the lattice by INDEX, out-of-range reading as `+tau` —
    /// the same answer `value` gives outside the root, so the extraction's
    /// boundary needs no special case.
    fn sample(&self, i: isize, j: isize, k: isize) -> f32 {
        let n = self.cells as isize;
        if i < 0 || j < 0 || k < 0 || i >= n || j >= n || k >= n {
            return self.tau as f32;
        }
        self.value(
            self.origin[0] + (i as f64 + 0.5) * self.cell,
            self.origin[1] + (j as f64 + 0.5) * self.cell,
            self.origin[2] + (k as f64 + 0.5) * self.cell,
        )
    }

    /// The world point a lattice index names — cell CENTERS, which is where
    /// `build_node` sampled and therefore the only lattice the stored values
    /// are true at.
    fn lattice_point(&self, p: [f64; 3]) -> [f64; 3] {
        [
            self.origin[0] + (p[0] + 0.5) * self.cell,
            self.origin[1] + (p[1] + 0.5) * self.cell,
            self.origin[2] + (p[2] + 0.5) * self.cell,
        ]
    }

    /// The lattice block one leaf's extraction reads: its 8x8x8 samples plus
    /// the one-cell shell on every side, so both passes work from an array
    /// instead of descending the octree once per corner. Without it the
    /// extraction re-reads each sample about ten times — measured at ~2.0 s
    /// on sphere.cam, against ~0.4 s with it.
    fn leaf_block(&self, lmin: [usize; 3]) -> Box<[f32; 1000]> {
        let mut b = Box::new([0f32; 1000]);
        for dk in -1..9i32 {
            for dj in -1..9i32 {
                for di in -1..9i32 {
                    b[(((dk + 1) * 10 + (dj + 1)) * 10 + (di + 1)) as usize] = self.sample(
                        lmin[0] as isize + di as isize,
                        lmin[1] as isize + dj as isize,
                        lmin[2] as isize + dk as isize,
                    );
                }
            }
        }
        b
    }

    /// The machined stock's SURFACE, extracted as a welded triangle mesh —
    /// the undercut-capable reading of the same field the heightfield view
    /// can only take a lid off (DECISIONS.md → "the field is already
    /// undercut-capable…", slice 0).
    ///
    /// Surface Nets: one vertex per lattice cube whose eight corner samples
    /// do not agree in sign, placed at the centroid of that cube's zero
    /// crossings, and one quad per lattice EDGE that changes sign, joining
    /// the four cubes around it. The vertex is shared by every quad that
    /// touches its cube, so the mesh comes out indexed and welded rather than
    /// as a triangle soup.
    ///
    /// **Why walking the leaf bricks alone is sufficient, and not merely
    /// convenient.** A sign change on a lattice edge implies at least one of
    /// its endpoints is strictly inside the truncation band, so the surface
    /// cannot hide in a collapsed node: the composite is 1-Lipschitz and
    /// adjacent centers are `cell` apart, so two samples can differ by at most
    /// `cell` — while stepping from `+tau` to `-tau` demands `2*tau`, which is
    /// `4*cell` at `TAU_CELLS = 2`. An `Air`-beside-`Material` face with no
    /// band between them is arithmetically impossible, and every crossing
    /// therefore has a brick on at least one side.
    ///
    /// **The one place that argument needs a shell.** It holds for every
    /// crossing INSIDE the root, so both passes could walk brick interiors
    /// alone — except that the root cube's side is the stock's longest
    /// dimension, so a 100x100x20 stock fills the root exactly in x and y and
    /// its four side walls lie ON the boundary. Outside reads `+tau` by fiat
    /// rather than by measurement, so those crossings are real but have no
    /// brick on the far side. Both passes therefore reach one cube and one
    /// sample BEFORE each brick, which closes the mesh; the cost is that a
    /// wall standing exactly on the boundary is placed from a `+tau` that
    /// over-states the outside distance, landing it within about a third of a
    /// cell. That is inside what this surface claims (the field, at the
    /// field's resolution) and is the only place the extraction approximates
    /// for a reason other than resolution.
    ///
    /// This is GEOMETRY and makes no claim about trustworthiness: a field
    /// that is `!complete()` still extracts, because whether a picture may be
    /// DRAWN from it is the view's decision and is made in `view::sim_iso`
    /// where the wording for declining already lives.
    pub fn iso_surface(&self) -> IsoSurface {
        use std::collections::HashMap;

        // Pass 1 — place one vertex per straddling cube. Keyed by the cube's
        // minimum corner, which is what the quads look up by, and which makes
        // the weld automatic: two bricks naming the same boundary cube get
        // the same vertex rather than a crack.
        let mut vert_of: HashMap<[i32; 3], u32> = HashMap::new();
        let mut verts: Vec<[f64; 3]> = Vec::new();

        // The 12 edges of a cube as (corner, corner), corners indexed
        // x-fastest exactly as the bricks are.
        const EDGES: [(usize, usize); 12] = [
            (0, 1), (2, 3), (4, 5), (6, 7), // along x
            (0, 2), (1, 3), (4, 6), (5, 7), // along y
            (0, 4), (1, 5), (2, 6), (3, 7), // along z
        ];
        const CORNER: [[i32; 3]; 8] = [
            [0, 0, 0], [1, 0, 0], [0, 1, 0], [1, 1, 0],
            [0, 0, 1], [1, 0, 1], [0, 1, 1], [1, 1, 1],
        ];

        let mut leaves: Vec<[usize; 3]> = Vec::new();
        each_leaf(&self.root, [0; 3], self.cells, &mut |min, _brick| leaves.push(min));

        for lmin in &leaves {
            let blk = self.leaf_block(*lmin);
            let at = |di: i32, dj: i32, dk: i32| -> f32 {
                blk[(((dk + 1) * 10 + (dj + 1)) * 10 + (di + 1)) as usize]
            };
            for dk in -1..8i32 {
                for dj in -1..8i32 {
                    for di in -1..8i32 {
                        let c = [
                            lmin[0] as i32 + di,
                            lmin[1] as i32 + dj,
                            lmin[2] as i32 + dk,
                        ];
                        if vert_of.contains_key(&c) {
                            continue;
                        }
                        let mut v = [0f32; 8];
                        let mut neg = 0;
                        for (n, off) in CORNER.iter().enumerate() {
                            v[n] = at(di + off[0], dj + off[1], dk + off[2]);
                            if v[n] < 0.0 {
                                neg += 1;
                            }
                        }
                        if neg == 0 || neg == 8 {
                            continue;
                        }
                        // The centroid of this cube's crossings, in lattice
                        // coordinates relative to the cube's min corner.
                        let mut acc = [0f64; 3];
                        let mut n_cross = 0f64;
                        for (a, b) in EDGES {
                            let (va, vb) = (f64::from(v[a]), f64::from(v[b]));
                            if (va < 0.0) == (vb < 0.0) {
                                continue;
                            }
                            // va != vb is guaranteed: the signs differ.
                            let t = va / (va - vb);
                            for ax in 0..3 {
                                let (ca, cb) = (f64::from(CORNER[a][ax]), f64::from(CORNER[b][ax]));
                                acc[ax] += ca + t * (cb - ca);
                            }
                            n_cross += 1.0;
                        }
                        let p = [
                            f64::from(c[0]) + acc[0] / n_cross,
                            f64::from(c[1]) + acc[1] / n_cross,
                            f64::from(c[2]) + acc[2] / n_cross,
                        ];
                        vert_of.insert(c, verts.len() as u32);
                        verts.push(self.lattice_point(p));
                    }
                }
            }
        }

        // Pass 2 — one quad per sign-changing lattice edge, wound so the
        // normal points OUT of material (toward increasing value). The four
        // cubes around an axis-`a` edge sit at the four combinations of -1/0
        // in the other two axes; taking them in (b,c) order (-1,-1), (0,-1),
        // (0,0), (-1,0) traverses +b then +c then -b, whose first two edges
        // cross to +a — so that order IS the outward winding when the
        // material lies on the low side, and its reverse when it does not.
        let mut tris: Vec<[u32; 3]> = Vec::new();
        let mut seen_edge: HashMap<([i32; 3], u8), ()> = HashMap::new();
        for lmin in &leaves {
            let blk = self.leaf_block(*lmin);
            let at = |di: i32, dj: i32, dk: i32| -> f32 {
                blk[(((dk + 1) * 10 + (dj + 1)) * 10 + (di + 1)) as usize]
            };
            for dk in -1..8i32 {
                for dj in -1..8i32 {
                    for di in -1..8i32 {
                        let s = [
                            lmin[0] as i32 + di,
                            lmin[1] as i32 + dj,
                            lmin[2] as i32 + dk,
                        ];
                        for a in 0..3usize {
                            let mut d1 = [di, dj, dk];
                            d1[a] += 1;
                            let v0 = at(di, dj, dk);
                            let v1 = at(d1[0], d1[1], d1[2]);
                            // Sign FIRST, dedup second. The shell makes
                            // neighbouring bricks revisit each other's edges,
                            // so a dedup is required — but deduping every
                            // edge instead of every CROSSING edge means
                            // millions of map writes to keep a hundred
                            // thousand, and the overwhelming majority of
                            // lattice edges never cross anything.
                            if (v0 < 0.0) == (v1 < 0.0) {
                                continue;
                            }
                            if seen_edge.insert((s, a as u8), ()).is_some() {
                                continue;
                            }
                            let (b, c) = ((a + 1) % 3, (a + 2) % 3);
                            let mut cube = [[0i32; 3]; 4];
                            for (n, (ob, oc)) in [(-1, -1), (0, -1), (0, 0), (-1, 0)].iter().enumerate() {
                                let mut q = s;
                                q[b] += ob;
                                q[c] += oc;
                                cube[n] = q;
                            }
                            let idx: Option<Vec<u32>> =
                                cube.iter().map(|q| vert_of.get(q).copied()).collect();
                            // Every cube around a sign-changing edge straddles
                            // by construction, so a miss means the edge left
                            // the root — a boundary the stock should never
                            // reach, and a hole is the honest answer if it does.
                            let Some(q) = idx else { continue };
                            let (x, y, z, w) = if v0 < 0.0 {
                                (q[0], q[1], q[2], q[3])
                            } else {
                                (q[0], q[3], q[2], q[1])
                            };
                            tris.push([x, y, z]);
                            tris.push([x, z, w]);
                        }
                    }
                }
            }
        }

        // Normals from the field's own gradient, by central difference at one
        // cell — the surface's slope is the SDF's slope, so the shading is a
        // reading of the field rather than a reconstruction from the
        // triangles (which is what made the heightfield's terraces unshadeable).
        let h = self.cell;
        let normals: Vec<[f64; 3]> = verts
            .iter()
            .map(|p| {
                let g = [
                    f64::from(self.value(p[0] + h, p[1], p[2]) - self.value(p[0] - h, p[1], p[2])),
                    f64::from(self.value(p[0], p[1] + h, p[2]) - self.value(p[0], p[1] - h, p[2])),
                    f64::from(self.value(p[0], p[1], p[2] + h) - self.value(p[0], p[1], p[2] - h)),
                ];
                let n = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
                if n > 0.0 {
                    [g[0] / n, g[1] / n, g[2] / n]
                } else {
                    // A plateau in the truncated field (every neighbour
                    // clamped) has no slope to read; +Z is the drawing
                    // convention's up and never a silent NaN.
                    [0.0, 0.0, 1.0]
                }
            })
            .collect();

        IsoSurface { verts, normals, tris }
    }

    /// The field's form of the foul lint's body test: does a tool body
    /// standing at (ax, ay) with its tip at `tip_z` occupy field material
    /// anywhere up its reach?
    ///
    /// **The radial arm** (DECISIONS.md → "the profile silhouette…", slice 3,
    /// fork 3). Slice 3 of the voxel engine landed this reading only the axis
    /// column, which is why its own pin could call itself radius-independent —
    /// and why it could not see the case this lint exists for. A shank against
    /// a wall never buries the axis, and from an axis value in air no amount
    /// of Lipschitz reasoning certifies material nearby: the property gives a
    /// LOWER bound on neighbouring values, and a lower bound cannot prove a
    /// value is negative. So the body's rim is sampled where the analytic
    /// lint samples it — `BODY_AZIMUTH_SAMPLES` around `body_radius_at(h)`,
    /// the same lattice with a distance lookup where its point-in-solid test
    /// used to be. The two voices now differ in what they know, not in how
    /// finely they look.
    ///
    /// The check results stay certified or silent, never guessed:
    /// - `Foul` only where a queried point is certifiably inside material —
    ///   the stored value is at the containing cell's center and the composite
    ///   is 1-Lipschitz, so a value below −(cell·√3/2) puts the queried point
    ///   below zero regardless of where it sits in the cell. Sign is exact
    ///   only for a COMPLETE field (an incomplete one over-states material and
    ///   would cry foul at un-stamped sweeps), so an incomplete field answers
    ///   `Uncertain` unconditionally.
    /// - `Clear` only when every height certifies daylight: value ≥ body
    ///   radius + half a cell diagonal AT THE AXIS, which certifies the whole
    ///   disc at that height and is why the ring is skipped there — the
    ///   1-Lipschitz property bounds clearance downward, which is the
    ///   direction that arm needs. The field is truncated at τ, so clearance
    ///   beyond τ is not certifiable and wide bodies honestly land in
    ///   `Uncertain` — a caller that needs certified clearance for a fat body
    ///   passes a finer cell (τ scales with it) rather than reading
    ///   `Uncertain` as either answer.
    /// - `Uncertain` is the advisory bias to silence, ported: quiet when
    ///   unsure, in both directions. A rim sample can MISS a foul thinner than
    ///   its spacing; it can never invent one.
    pub fn body_clearance(
        &self,
        body_radius_at: &dyn Fn(f64) -> f64,
        ax: f64,
        ay: f64,
        tip_z: f64,
        reach: f64,
    ) -> BodyVerdict {
        if !self.complete() || reach <= 0.0 {
            return BodyVerdict::Uncertain;
        }
        let half_diag = self.cell * 3f64.sqrt() * 0.5;
        let mut all_clear = true;
        for i in 0..=BODY_HEIGHT_SAMPLES {
            let h = reach * i as f64 / BODY_HEIGHT_SAMPLES as f64;
            let z = tip_z + h;
            let v = self.value(ax, ay, z) as f64;
            if v < -half_diag {
                return BodyVerdict::Foul { h, x: ax, y: ay, z };
            }
            let r = body_radius_at(h);
            if v >= r + half_diag {
                continue; // the whole disc at this height is certified air
            }
            all_clear = false;
            if r <= 0.0 {
                continue; // an on-axis point IS the axis query above
            }
            for b in 0..BODY_AZIMUTH_SAMPLES {
                let t = 2.0 * std::f64::consts::PI * b as f64 / BODY_AZIMUTH_SAMPLES as f64;
                let (px, py) = (ax + r * t.pcos(), ay + r * t.psin());
                if (self.value(px, py, z) as f64) < -half_diag {
                    return BodyVerdict::Foul { h, x: px, y: py, z };
                }
            }
        }
        if all_clear {
            BodyVerdict::Clear
        } else {
            BodyVerdict::Uncertain
        }
    }

    /// Whole-loop rest certification (DECISIONS.md "adaptive clearing…",
    /// slice 1, option A): is the closed loop's swept band — the polyline
    /// fattened by the tool radius `r`, at cutting depth `z` and the whole
    /// column above it — CERTIFIED void? `true` lets a `strategy=rest` Clear
    /// drop the loop; anything less than certainty answers `false`, because
    /// cutting air is the safe direction and skipping material is the gouge
    /// of omission.
    ///
    /// Two facts carry the certification:
    /// - **The void set is upward-closed inside the stock**: every stamp is —
    ///   the rod by construction (semi-infinite upward), the ball because its
    ///   rod covers everything above the equator — so void at the cutting
    ///   depth is void all the way up, and the deepest level answers for the
    ///   column. One query plane, not a ladder. (Below the stock's bottom the
    ///   lemma has no footing — void under the box says nothing about the box
    ///   — so a cutting depth below the stock is never certified.)
    /// - **Each query certifies at most τ of clearance** (the
    ///   `body_clearance` lesson): the stored value sits at the containing
    ///   cell's center and the composite is 1-Lipschitz, so a query at q
    ///   certifies a void ball of radius `value − cell·√3/2` around q, capped
    ///   by the truncation. The band is therefore TILED with queries — a
    ///   lattice over every segment's stadium (spacing ≤ one cell along and
    ///   across, lateral endpoints clamped to ±r so every node is in-band)
    ///   plus polar tilings of the vertex discs — every band point sits
    ///   within `cell·√2/2` of some node, and a node certifies its
    ///   neighborhood only when `value > cell·(√3/2 + √2/2)` ≈ 1.573·cell,
    ///   which the τ = 2-cell truncation leaves honestly reachable.
    ///
    /// A consequence worth stating: TANGENCY IS NOT CERTIFIABLE. A loop whose
    /// band grazes standing material — the wall pass at exactly `r + skin`
    /// off the wall, or a re-run over a floor cut to exactly this depth —
    /// reads `false` and stays, as a spring pass. That is the τ-law working,
    /// not a miss: the field's boundary IS value zero, and a certification
    /// that could vault it would be the gouge this function exists to reject.
    pub fn band_certified_void(&self, pts: &[(f64, f64)], r: f64, z: f64) -> bool {
        // Belt and braces: an incomplete field over-states material, so its
        // void claims are actually sound — but the error law upstream has
        // already declined the op, and a certifier that answered anyway would
        // let a future caller skip that law.
        if !self.complete() || pts.is_empty() || r <= 0.0 {
            return false;
        }
        if !self.upward_closed {
            // A truncated silhouette stamped this field, so void at the cutting
            // depth no longer answers for the column above it and the one-plane
            // shortcut below would be certifying a lemma it does not have
            // (DECISIONS.md → "the profile silhouette…", slice 2). Declining
            // costs a form-cutter prefix its rest selection — it cuts the loop
            // instead — which is the direction this function always errs in.
            // The height-ladder upgrade lands when a program wants both.
            return false;
        }
        if z < self.origin[2] - PATH_EPS {
            return false; // below the stock: the upward-closure lemma has no footing
        }
        let g = self.cell;
        let need = g * (3f64.sqrt() / 2.0 + std::f64::consts::SQRT_2 / 2.0);
        let ok = |x: f64, y: f64| (self.value(x, y, z) as f64) > need;

        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            // The vertex disc: polar rings at radial spacing ≤ g (center and
            // rim included), each ring at arc spacing ≤ g.
            let n_rings = ((r / g).ceil() as usize).max(1);
            for ir in 0..=n_rings {
                let rho = r * ir as f64 / n_rings as f64;
                let n_arc = ((2.0 * std::f64::consts::PI * rho / g).ceil() as usize).max(1);
                for ia in 0..n_arc {
                    let ang = 2.0 * std::f64::consts::PI * ia as f64 / n_arc as f64;
                    if !ok(a.0 + rho * ang.pcos(), a.1 + rho * ang.psin()) {
                        return false;
                    }
                }
            }
            // The segment's stadium interior: a lattice at spacing ≤ g along
            // and across, lateral positions spanning [-r, +r] inclusive.
            let (ex, ey) = (b.0 - a.0, b.1 - a.1);
            let len = ex.phypot(ey);
            if len <= PATH_EPS {
                continue;
            }
            let (ux, uy) = (ex / len, ey / len);
            let (nx, ny) = (-uy, ux);
            let m_along = ((len / g).ceil() as usize).max(1);
            let m_lat = ((2.0 * r / g).ceil() as usize).max(1);
            for ia in 0..=m_along {
                let t = len * ia as f64 / m_along as f64;
                for il in 0..=m_lat {
                    let l = -r + 2.0 * r * il as f64 / m_lat as f64;
                    if !ok(a.0 + ux * t + nx * l, a.1 + uy * t + ny * l) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Rectilinear region boundaries of a boolean cell mask (DECISIONS.md
/// "adaptive clearing, region decomposition…", the polygonization sub-fork).
/// The mask is binary — a complete field's sign at cell centers is exact —
/// so marching squares collapses to the exact boundary of the union of cell
/// squares: directed grid edges with material on the LEFT, paired into loops.
/// Outer boundaries come out CCW and holes CW, so holes are dropped by their
/// area sign alone (a hole in a rest region is air the spiral may cross —
/// the DRAFT's wasteful-but-safe reading). At a saddle vertex the LEFT turn
/// is taken, which keeps material 4-connected: two regions touching at one
/// corner are two loops, not one pinched one. Consecutive collinear segments
/// merge (lossless); nothing else is smoothed — cell-scale roughness is
/// roughing vocabulary, and determinism is the derivation being closed under
/// (mask, origin, cell): loop starts are scan order, nothing iterates a map.
/// `(vx0, vy0)` is the world coordinate of grid vertex (0, 0) — the min
/// corner of cell (0, 0) — and `cell` the square side.
pub(crate) fn trace_regions(
    mask: &[bool],
    nx: usize,
    ny: usize,
    vx0: f64,
    vy0: f64,
    cell: f64,
) -> Vec<Vec<(f64, f64)>> {
    let at = |i: isize, j: isize| -> bool {
        i >= 0 && j >= 0 && (i as usize) < nx && (j as usize) < ny && mask[j as usize * nx + i as usize]
    };
    // Directions: 0 = +x, 1 = +y, 2 = −x, 3 = −y. An edge is (start vertex,
    // dir); vertices live on the (nx+1) × (ny+1) corner grid.
    let vw = nx + 1;
    let vid = |i: usize, j: usize| j * vw + i;
    let step = |v: usize, d: usize| -> usize {
        match d {
            0 => v + 1,
            1 => v + vw,
            2 => v - 1,
            _ => v - vw,
        }
    };
    let mut out = vec![[false; 4]; vw * (ny + 1)];
    for j in 0..ny as isize {
        for i in 0..nx as isize {
            if !at(i, j) {
                continue;
            }
            let (iu, ju) = (i as usize, j as usize);
            if !at(i, j - 1) {
                out[vid(iu, ju)][0] = true; // south side, +x
            }
            if !at(i + 1, j) {
                out[vid(iu + 1, ju)][1] = true; // east side, +y
            }
            if !at(i, j + 1) {
                out[vid(iu + 1, ju + 1)][2] = true; // north side, −x
            }
            if !at(i - 1, j) {
                out[vid(iu, ju + 1)][3] = true; // west side, −y
            }
        }
    }

    let mut regions = Vec::new();
    for sv in 0..out.len() {
        for sd in 0..4 {
            if !out[sv][sd] {
                continue;
            }
            // Walk the loop this edge belongs to: consume edges, preferring
            // the left turn at each vertex (then straight, then right).
            let mut verts: Vec<usize> = Vec::new();
            let (mut v, mut d) = (sv, sd);
            loop {
                out[v][d] = false;
                verts.push(v);
                let u = step(v, d);
                // The left-turn pairing is bijective at every vertex (at a
                // saddle each incoming's FIRST preference is its own region's
                // outgoing), so the preference scan only ever selects this
                // loop's next edge — or the loop's own start edge, which is
                // already consumed and must close the walk rather than let
                // the scan fall through onto another region's edges.
                let mut nd = None;
                for cand in [(d + 1) % 4, d, (d + 3) % 4] {
                    if u == sv && cand == sd {
                        break; // preference selected the start edge: closed
                    }
                    if out[u][cand] {
                        nd = Some(cand);
                        break;
                    }
                }
                match nd {
                    Some(next) => {
                        v = u;
                        d = next;
                    }
                    None => break,
                }
            }
            // Collinear merge: drop every vertex whose neighbors continue the
            // same direction. Work on coordinates only at the end.
            let n = verts.len();
            let mut pts: Vec<(f64, f64)> = Vec::new();
            for k in 0..n {
                let (prev, here, next) = (verts[(k + n - 1) % n], verts[k], verts[(k + 1) % n]);
                let (px, py) = ((prev % vw) as isize, (prev / vw) as isize);
                let (hx, hy) = ((here % vw) as isize, (here / vw) as isize);
                let (qx, qy) = ((next % vw) as isize, (next / vw) as isize);
                if (hx - px) * (qy - hy) - (hy - py) * (qx - hx) == 0 {
                    continue; // straight through
                }
                pts.push((vx0 + hx as f64 * cell, vy0 + hy as f64 * cell));
            }
            if pts.len() < 3 {
                continue;
            }
            // Holes trace clockwise under the material-on-left rule; the
            // signed area is the whole outer/hole distinction.
            let mut area = 0.0;
            for k in 0..pts.len() {
                let a = pts[k];
                let b = pts[(k + 1) % pts.len()];
                area += a.0 * b.1 - b.0 * a.1;
            }
            if area > 0.0 {
                regions.push(pts);
            }
        }
    }
    regions
}

/// `body_clearance`'s answer. `Foul` carries the height above the tip and the
/// world point that is certifiably buried — the axis itself, or a point on the
/// body's rim at that height (slice 3's radial arm), which is what makes the
/// message quotable in the same terms as the analytic lint's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyVerdict {
    Clear,
    Uncertain,
    Foul { h: f64, x: f64, y: f64, z: f64 },
}

/// The ladder and the ring `body_clearance` walks — the same counts the
/// analytic lint's lattice uses (`stock.rs`), so the two voices sample the
/// silhouette at the same granularity and a disagreement between them is
/// about what they KNOW, never about how finely they looked.
const BODY_HEIGHT_SAMPLES: usize = 64;
const BODY_AZIMUTH_SAMPLES: usize = 24;

// ---- the replay: emitted moves -> stamps -------------------------------------

/// Walk the leading `op_idx` ops' emitted moves (the `gcode::program_with_moves`
/// channel) into stamps, carrying the modal state across ops exactly as the
/// machine would. The tool-change boilerplate between ops is not walked: it
/// never travels below the clearance plane and every op re-states its Z at
/// clearance on entry, so no stamp is lost and no stale coordinate survives
/// an op boundary (each op re-establishes X/Y before descending) — which is
/// also why truncating the walk AT an op boundary is exact: the ops past the
/// prefix contribute no modal state the prefix's stamps depend on.
fn walk_emissions(
    p: &Part,
    emissions: &[gcode::OpEmission],
    op_idx: usize,
    through: Option<usize>,
) -> (Vec<Stamp>, Vec<String>) {
    let mut stamps: Vec<Stamp> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();
    let reject = |reasons: &mut Vec<String>, why: String| {
        if !reasons.contains(&why) {
            reasons.push(why);
        }
    };
    let top = p.stock.z;
    let mut m = Motion { rapid: true, x: None, y: None, z: None, f: None };

    // The op AT `op_idx` walks only at move grain (Phase 52 slice 6):
    // through move `through` of the channel walked below — the text's
    // move for an engine-comp op, the resolved move the text's foot is on
    // under control comp (the caller's conversion, slice 10).
    for em in emissions.iter().filter(|em| em.op_idx < op_idx || (em.op_idx == op_idx && through.is_some())) {
        let partial = if em.op_idx == op_idx { through } else { None };
        let op = &p.operations[em.op_idx];
        let tool = match p.tools.get(&op.tool_num) {
            Some(t) => t,
            None => {
                reject(&mut reasons, format!("op [{}] has no tool T{}", em.op_idx, op.tool_num));
                continue;
            }
        };
        // The named errors, never guesses: under control comp the emitted
        // TEXT is the part edge (the `check_moves` rule) — stamped anyway
        // when the emitter authored a resolved tool-center channel
        // (DECISIONS.md "the control-comp centerline resolve"); a control op
        // without one keeps the named error by construction. A `profile`
        // tool's revolved-polyline silhouette is still the other named
        // upgrade path.
        let sil = Silhouette::of(tool);
        let comp_ok = op.comp == CompMode::Engine || em.resolved.is_some();
        let shape_ok = sil.narrowing().is_none();
        let stampable = comp_ok && shape_ok;
        if !comp_ok {
            reject(
                &mut reasons,
                format!(
                    "op [{}] on '{}' uses control comp with no resolved centerline: the emitted coordinates are the part edge, not the tool center, so its sweep is not stamped",
                    em.op_idx,
                    op.feature_name
                ),
            );
        }
        if let Some((h, below, above)) = sil.narrowing() {
            reject(
                &mut reasons,
                format!(
                    "tool T{}'s profile narrows going up — r {:.3}mm below {:.3}mm above the tip, r {:.3}mm above it: a necked silhouette is the one tool class that can undercut, and its swept void is not upward-closed, so it is not stamped",
                    op.tool_num, below, h, above
                ),
            );
        }

        // `gcode::resolved_moves` — the one home for which channel a reader
        // walks. The modal-state carry across ops is exact either way,
        // because every op re-establishes its coordinates on entry (the
        // truncation argument above).
        let limit = partial.map_or(usize::MAX, |b| b + 1);
        for line in gcode::resolved_moves(em).iter().take(limit) {
            let before = m;
            let arc = gcode::arc_words(line);
            if !gcode::apply_move(line, &mut m) {
                continue;
            }
            if !stampable {
                continue; // modal state still tracked above
            }
            let to = (m.x, m.y, m.z);
            let (tx, ty, tz) = match to {
                (Some(x), Some(y), Some(z)) => (x, y, z),
                _ => {
                    // A move the walker cannot place. Skipping is honest only
                    // while every known height stays at or above the stock's
                    // top (the op-entry retracts and first rapids).
                    let known_min = [before.z, m.z].iter().flatten().fold(f64::INFINITY, |a, &b| a.min(b));
                    if known_min < top - PATH_EPS {
                        reject(
                            &mut reasons,
                            format!("op [{}]: a move below the stock top could not be placed", em.op_idx),
                        );
                    }
                    continue;
                }
            };
            let from = match (before.x, before.y, before.z) {
                (Some(x), Some(y), Some(z)) => Some((x, y, z)),
                _ => None,
            };
            let (fx, fy, fz) = match from {
                Some(f) => f,
                None => {
                    if tz < top - PATH_EPS || before.z.map_or(false, |z| z < top - PATH_EPS) {
                        reject(
                            &mut reasons,
                            format!("op [{}]: a move below the stock top could not be placed", em.op_idx),
                        );
                    }
                    continue;
                }
            };

            if let Some((i, j, cw)) = arc {
                // The dialect's arcs are constant-Z; a Z word on an arc line
                // would be a new move class, rejected rather than guessed.
                if (tz - fz).abs() > PATH_EPS {
                    reject(
                        &mut reasons,
                        format!("op [{}]: an arc carrying a Z word is not a stamped move class", em.op_idx),
                    );
                    continue;
                }
                // `gcode::arc_geom` — the center/sweep derivation's one home
                // (a sign error there stamps the mirrored arc); the
                // zero-radius error stays this walker's judgment.
                let ag = gcode::arc_geom(fx, fy, tx, ty, i, j, cw);
                if ag.ra <= PATH_EPS {
                    reject(
                        &mut reasons,
                        format!("op [{}]: an arc with a zero-length radius vector is not a stamped move class", em.op_idx),
                    );
                    continue;
                }
                push_stamp(
                    &mut stamps,
                    em.op_idx,
                    sil.clone(),
                    PathK::HArc { cx: ag.cx, cy: ag.cy, ra: ag.ra, a0: ag.a0, sweep: ag.sweep, z: tz },
                );
            } else {
                let plan = dist2d(fx, fy, tx, ty);
                if plan <= PATH_EPS {
                    push_stamp(
                        &mut stamps,
                        em.op_idx,
                        sil.clone(),
                        PathK::Point { x: tx, y: ty, z: fz.min(tz), lift: (tz - fz).abs() },
                    );
                } else if (tz - fz).abs() <= PATH_EPS {
                    push_stamp(&mut stamps, em.op_idx, sil.clone(), PathK::HSeg { ax: fx, ay: fy, bx: tx, by: ty, z: tz });
                } else {
                    push_stamp(&mut stamps, em.op_idx, sil.clone(), PathK::Slope { a: [fx, fy, fz], b: [tx, ty, tz] });
                }
            }
        }
    }
    (stamps, reasons)
}

/// The resolution derivation, one home for build and load alike: cells per
/// root edge is the power of two (floored at one brick) that brings the cell
/// to or under the target; the RESOLVED cell — not the request — is what the
/// cache hash carries, so two requests that round to the same subdivision
/// share a cache entry.
fn resolve_cells(s: &Stock, cell: Option<f64>) -> Result<(usize, f64), String> {
    if s.w <= 0.0 || s.l <= 0.0 || s.h <= 0.0 {
        return Err("voxel field: the stock has no volume".into());
    }
    let side = s.w.max(s.l).max(s.h);
    let target = match cell {
        Some(c) if c > 0.0 => c,
        Some(_) => return Err("voxel field: cell size must be positive".into()),
        None => side / DEFAULT_CELLS,
    };
    let mut cells: usize = 8;
    while side / cells as f64 > target && cells < (1 << 14) {
        cells *= 2;
    }
    Ok((cells, side / cells as f64))
}

// ---- the `.vox` cache --------------------------------------------------------
//
// A cache, never a source (the design entry's sub-fork): the repo's pitch is
// text that diffs, and a binary field would review as a hash — so `.vox`
// holds only derived state, is never authored, never a golden, and is
// gitignored. Its whole validity story is the input hash: everything the
// field is a function of — the resolved cell, the stock, the tool table, the
// stamped prefix (each op's emitted moves and stamp bindings), the format
// version — hashed at build and verified at load; on any mismatch the file
// is recomputed, not trusted. A field is a PREFIX field (the end state is
// the whole-program prefix), and the hash is computed over exactly the
// prefix the file represents. It fails
// closed: a truncated write, a flipped bit (the body checksum), a foreign
// file, all read as "stale", and `load_or_build` rebuilds and heals. A cache
// hit still re-emits the program — validation (the entry restriction, the
// lints) is never skipped for having a warm cache; what the cache saves is
// the octree build, which is the expensive part.

const VOX_MAGIC: &[u8; 8] = b"NACSVOX\0";
// v2 (slice 3): the incompleteness REASONS ride the file where v1 carried one
// boolean — a loaded field must be able to say why it declines, not just
// that it does. Old caches read as stale by the version check, exactly the
// self-invalidation the format was designed around.
// v3 (adaptive slice 0): a field is a PREFIX field — the file carries the op
// count it stamped, and the input hash is computed over that prefix (the
// format doc's "hash of the prefix it represents" finally earning the word).
// v4 (the control-comp centerline resolve): control ops stamp their resolved
// channel, and the input hash covers it — a v3 field of the same program
// stamped nothing for those ops, so it must read stale, not equal.
// v5 (the profile silhouette, slice 2): shaped tools stamp their meridian
// polygon truncated at the cutting top, where v4 rejected to stamp them.
// v6 (K2 slice 2): a rod with a declared flute_len stamps truncated at it,
// and a drawn profile's declared cut_top clips its stamped envelope — a v5
// field of such a program carries the semi-infinite/full-drawing sweeps, so
// it must read stale, not equal. (The two fields join the input hash for the
// same reason; the bump covers the algorithm change alone.)
// 8: the field holds the stock model's prefix-0 air — a given void, and
// what the program before this one cut (Phase 48 slice 1); a version-7
// field held both as material.
const VOX_FORMAT_VERSION: u32 = 8;

/// The stock's words in the hash and the file (Phase 46 slice 1 added the
/// shape, and bumped the format): the box, then a shape tag and the round's
/// diameter — zero for a box, so a box's words are its old six and two zeros.
const STOCK_WORDS: usize = 8;

fn stock_words(s: &Stock) -> [f64; STOCK_WORDS] {
    let (tag, dia) = match s.shape {
        StockShape::Box => (0.0, 0.0),
        StockShape::Cylinder { dia } => (1.0, dia),
        // A profile's points cannot ride eight words: the tag says so and
        // the eighth word is their hash, so a key never mistakes one
        // outline for another; the field itself is never cached (`save`).
        StockShape::Profile { ref points } => {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for q in points {
                fnv1a(&mut h, &q.x.to_bits().to_le_bytes());
                fnv1a(&mut h, &q.y.to_bits().to_le_bytes());
            }
            (2.0, f64::from_bits(h))
        }
    };
    [s.w, s.l, s.h, s.x, s.y, s.z, tag, dia]
}

fn fnv1a(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// A prefix-0 surface's words for the cache key: its kind, and for a table
/// its key origin (a meridian's centre, a form's path) and every sample.
fn hash_floor(h: &mut u64, f: &crate::stock::Floor) {
    use crate::stock::Floor;
    let mut num = |v: f64| fnv1a(h, &v.to_bits().to_le_bytes());
    match f {
        Floor::Flat(z) => {
            num(*z);
        }
        Floor::Meridian { x, y, samples } => {
            num(*x);
            num(*y);
            for &(rho, z) in samples {
                num(rho);
                num(z);
            }
        }
        Floor::PathForm { path, samples } => {
            for q in path {
                num(q.x);
                num(q.y);
            }
            for &(d, z) in samples {
                num(d);
                num(z);
            }
        }
        Floor::Dome { .. } | Floor::AlongWire { .. } => unreachable!("prefix_air_columns keeps these out of the field"),
    }
    fnv1a(h, crate::stock::floor_kind(f).as_bytes());
}

/// One column of air at prefix 0 (Phase 48 slice 1): a prefix-0 removal of
/// the stock model — a given void, or what the program before this one cut,
/// turned — as a plan section between two surfaces. A given void is a
/// flat-floored column with no ceiling (`stock::given_removal` makes
/// columns and slabs), and its signed distance is closed form: the
/// section's plan distance and the z interval's, combined as a product
/// set's is. The earlier program's removals turned over (slice 5) may hang
/// from a SHAPED ceiling — a bowl's meridian, a rounder's `(d, z)` table —
/// or, a ceiling it had, stand on a shaped floor; those read the table's
/// own signed distance in the `(distance-to-path, z)` half-plane, which is
/// a lower bound of the true distance (a plan distance is 1-Lipschitz, so
/// the half-plane's distance never exceeds space's) with the exact sign:
/// an under-statement, the composite's safe direction, and still
/// 1-Lipschitz, which keeps the octree's bracket honest.
struct AirColumn {
    section: crate::stock::Section,
    /// The section's rims, chorded through the one chord-tolerance home —
    /// the magnitude of the plan distance for the shapes without a closed
    /// form (a polygon, an ellipse, a band), and the cache key's words.
    loops: Vec<Vec<Pt>>,
    /// `Section::All`: the whole blank in plan, bounded by the stock alone.
    whole_plan: bool,
    /// The surfaces, as the stock model holds them: `Flat` or a table the
    /// field has a distance for (`shaped_column_sdf`); any other kind made
    /// the field incomplete at build and is not here.
    floor: crate::stock::Floor,
    ceiling: Option<crate::stock::Floor>,
    /// The column's z extent — the floor's lowest point, the flat cap —
    /// for the skips in `prefix_air_sdf` and the upward-closure check.
    z_lo: f64,
    z_hi: f64,
    /// Plan bounding box of the rims, for the skip in `prefix_air_sdf`.
    bb: [f64; 4],
}

impl AirColumn {
    /// Whether either surface is a table rather than a plane.
    fn shaped(&self) -> bool {
        self.ceiling.is_some() || !matches!(self.floor, crate::stock::Floor::Flat(_))
    }
}

/// The stock model's prefix-0 removals as columns — read from the one home
/// (`stock::remaining_before(p, 0)`), so the field and the model cannot
/// disagree about what is air before the first op — and, beside them, one
/// reason per removal the field has no distance for (a `Dome` or
/// `AlongWire` surface at prefix 0, which nothing makes today): the field
/// is then INCOMPLETE by that reason, never missing a column in silence.
fn prefix_air_columns(p: &Part) -> (Vec<AirColumn>, Vec<String>) {
    use crate::stock::Floor;
    let mut columns = Vec::new();
    let mut reasons = Vec::new();
    for r in &crate::stock::remaining_before(p, 0).removals {
        let readable = |f: &Floor| matches!(f, Floor::Flat(_) | Floor::Meridian { .. } | Floor::PathForm { .. });
        if !readable(&r.floor) || r.ceiling.as_ref().is_some_and(|c| !readable(c)) {
            reasons.push(format!(
                "a prefix-0 removal ({} floor{}) the swept field has no signed distance for — the air before the first op is under-read there",
                crate::stock::floor_kind(&r.floor),
                r.ceiling.as_ref().map_or(String::new(), |c| format!(", {} ceiling", crate::stock::floor_kind(c))),
            ));
            continue;
        }
        let loops = r.section.outlines(&p.stock);
        let bb = loops.iter().flatten().fold(
            [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY],
            |b, q| [b[0].min(q.x), b[1].min(q.y), b[2].max(q.x), b[3].max(q.y)],
        );
        let (z_lo, _) = r.z_span();
        columns.push(AirColumn {
            whole_plan: matches!(r.section, crate::stock::Section::All),
            section: r.section.clone(),
            loops,
            floor: r.floor.clone(),
            ceiling: r.ceiling.clone(),
            z_lo,
            z_hi: r.z_top,
            bb,
        });
    }
    (columns, reasons)
}

/// The signed distance, in the `(key, z)` half-plane, from `(key, z)` to a
/// table's piecewise-linear curve — positive ABOVE the curve, negative
/// below — with the curve's end clamps (`parse::interp_table`'s) drawn as
/// flat rays, so a point past the table's span measures to the clamped
/// height exactly as the model reads it.
fn profile_sdf(samples: &[(f64, f64)], key: f64, z: f64) -> f64 {
    let sign = if z > crate::parse::interp_table(samples, key) { 1.0 } else { -1.0 };
    let (first, last) = (samples[0], samples[samples.len() - 1]);
    let far = 1.0e6;
    let mut prev = (first.0 - far, first.1);
    let mut best = f64::INFINITY;
    for &s in samples.iter().chain(std::iter::once(&(last.0 + far, last.1))) {
        let (ex, ey) = (s.0 - prev.0, s.1 - prev.1);
        let len2 = ex * ex + ey * ey;
        let t = if len2 > 0.0 { (((key - prev.0) * ex + (z - prev.1) * ey) / len2).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min((key - (prev.0 + t * ex)).phypot(z - (prev.1 + t * ey)));
        prev = s;
    }
    sign * best
}

/// A shaped surface's signed distance at a point, as the column's floor
/// (`air_above`: air stands above it, so the sign flips) or its ceiling.
/// The surface's key is the plan distance the model itself reads it by —
/// a meridian's rho, a form table's distance to its path — so the field
/// and the model answer the same height at every plan point.
fn shaped_surface_sdf(f: &crate::stock::Floor, x: f64, y: f64, z: f64, air_above: bool) -> f64 {
    use crate::stock::Floor;
    let d = match f {
        Floor::Flat(h) => {
            return if air_above { h - z } else { z - h };
        }
        Floor::Meridian { x: cx, y: cy, samples } => profile_sdf(samples, (x - cx).phypot(y - cy), z),
        Floor::PathForm { path, samples } => profile_sdf(samples, crate::stock::dist_to_polygon(path, x, y), z),
        Floor::Dome { .. } | Floor::AlongWire { .. } => unreachable!("prefix_air_columns keeps these out of the field"),
    };
    if air_above {
        -d
    } else {
        d
    }
}

/// The column's signed plan distance, negative inside: closed form for a
/// disc and an annulus, the chorded rims' distance signed by the section's
/// own containment test for the rest.
fn air_plan_sdf(c: &AirColumn, x: f64, y: f64) -> f64 {
    use crate::stock::Section;
    match &c.section {
        Section::All => f64::NEG_INFINITY,
        Section::Disc { x: cx, y: cy, r } => (x - cx).phypot(y - cy) - r,
        Section::Annulus { x: cx, y: cy, r_inner, r_outer } => {
            let rho = (x - cx).phypot(y - cy);
            (r_inner - rho).max(rho - r_outer)
        }
        _ => {
            let d = c.loops.iter().map(|lp| crate::stock::dist_to_polygon(lp, x, y)).fold(f64::INFINITY, f64::min);
            if c.section.contains(x, y, 0.0) {
                -d
            } else {
                d
            }
        }
    }
}

/// The signed distance to the UNION of the prefix-0 air — the minimum over
/// the columns, each a product set's exact distance — stopping at the first
/// column the point is deeper than `tau` inside: the composite clamps there,
/// so a deeper answer is the same answer, and a column whose bounding box
/// alone puts it further than the best so far is not measured. Both skips
/// leave the value the composite reads unchanged wherever it is not clamped,
/// which is what keeps the octree's Lipschitz bracket honest.
fn prefix_air_sdf(air: &[AirColumn], x: f64, y: f64, z: f64, tau: f64) -> f64 {
    let mut best = f64::INFINITY;
    for c in air {
        // The z slab [z_lo, z_hi] holds the column whatever its surfaces,
        // so its distance bounds the column's from below — and bounds what
        // this function computes, which is what a skip must respect.
        let dz = (c.z_lo - z).max(z - c.z_hi);
        if dz >= best {
            continue;
        }
        let shaped = c.shaped();
        if !c.whole_plan {
            // Outside the rims' box in plan, the box's distance bounds the
            // plan distance from below; inside it, nothing is known yet. A
            // shaped column's value is a MAX of its parts, not a product
            // set's hypot, so its bound is the max too.
            let bx = (c.bb[0] - x).max(x - c.bb[2]).max(0.0);
            let by = (c.bb[1] - y).max(y - c.bb[3]).max(0.0);
            let outside = bx.phypot(by);
            if outside > 0.0 {
                let lower = if shaped {
                    outside.max(dz)
                } else if dz > 0.0 {
                    outside.phypot(dz)
                } else {
                    outside
                };
                if lower >= best {
                    continue;
                }
            }
        }
        let dp = air_plan_sdf(c, x, y);
        let d = if shaped {
            // Each term under-states the distance to its own surface with
            // the exact sign; the max of under-statements under-states the
            // distance to the nearest, with the exact sign (slice 5).
            let floor = shaped_surface_sdf(&c.floor, x, y, z, true);
            let ceiling = c.ceiling.as_ref().map_or(z - c.z_hi, |cl| shaped_surface_sdf(cl, x, y, z, false).max(z - c.z_hi));
            dp.max(floor).max(ceiling)
        } else if dp <= 0.0 && dz <= 0.0 {
            dp.max(dz)
        } else {
            dp.max(0.0).phypot(dz.max(0.0))
        };
        if d < best {
            best = d;
            if best < -tau {
                break;
            }
        }
    }
    best
}

/// Everything the field is a function of, hashed in a fixed order — computed
/// over the PREFIX the field represents, so each prefix keys its own cache
/// entry and no entry can answer for a state it doesn't hold. The recorded
/// moves stand in for the toolpaths (any engine change that alters an op's
/// emission alters its moves and invalidates every prefix containing it);
/// each op's stamp bindings ride alongside because the moves under-determine
/// them — the op→tool binding lives in the boilerplate M06 the emitted moves
/// deliberately omits, and two tools of one diameter but different
/// silhouettes (Endmill vs Ballmill) can emit identical moves that stamp
/// differently. The tool table adds the silhouette data itself; the format
/// version covers stamp-algorithm changes that leave the moves alone.
fn input_hash(p: &Part, cell_mm: f64, emissions: &[gcode::OpEmission], op_idx: usize) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    fnv1a(&mut h, &VOX_FORMAT_VERSION.to_le_bytes());
    fnv1a(&mut h, &cell_mm.to_bits().to_le_bytes());
    let s = &p.stock;
    for v in stock_words(s) {
        fnv1a(&mut h, &v.to_bits().to_le_bytes());
    }
    let mut nums: Vec<&usize> = p.tools.keys().collect();
    nums.sort();
    for num in nums {
        let t = &p.tools[num];
        fnv1a(&mut h, &(*num as u64).to_le_bytes());
        fnv1a(&mut h, &(t.name.len() as u64).to_le_bytes());
        fnv1a(&mut h, t.name.as_bytes());
        fnv1a(&mut h, &t.dia.to_bits().to_le_bytes());
        // The two body fields that move STAMPS (K2 slice 2): flute_len
        // truncates a rod's sweep, cut_top clips a drawn profile's. The rest
        // of the ladder (shank, holder) is body data no stamp reads, so it
        // stays out — hashing it would only churn caches for nothing.
        for v in [t.flute_len, t.cut_top] {
            fnv1a(&mut h, &v.map_or(u64::MAX, f64::to_bits).to_le_bytes());
        }
        match &t.profile {
            None => fnv1a(&mut h, &0u64.to_le_bytes()),
            Some(pts) => {
                fnv1a(&mut h, &(pts.len() as u64).to_le_bytes());
                for pt in pts {
                    fnv1a(&mut h, &pt.x.to_bits().to_le_bytes());
                    fnv1a(&mut h, &pt.y.to_bits().to_le_bytes());
                }
            }
        }
    }
    // The air at prefix 0 (Phase 48 slice 1): the columns themselves — the
    // field is a function of these, not of the `given` names or the earlier
    // project's text that derive them, so a change in how a removal is
    // stated upstream misses the cache the way a changed move does.
    let (air, air_reasons) = prefix_air_columns(p);
    fnv1a(&mut h, &(air_reasons.len() as u64).to_le_bytes());
    fnv1a(&mut h, &(air.len() as u64).to_le_bytes());
    for c in &air {
        for v in [c.z_lo, c.z_hi] {
            fnv1a(&mut h, &v.to_bits().to_le_bytes());
        }
        fnv1a(&mut h, &(c.loops.len() as u64).to_le_bytes());
        for lp in &c.loops {
            fnv1a(&mut h, &(lp.len() as u64).to_le_bytes());
            for q in lp {
                fnv1a(&mut h, &q.x.to_bits().to_le_bytes());
                fnv1a(&mut h, &q.y.to_bits().to_le_bytes());
            }
        }
        fnv1a(&mut h, &[c.whole_plan as u8]);
        // The surfaces (slice 5): a shaped one's table and its key's origin
        // are the column's shape, so they are the key's words too.
        hash_floor(&mut h, &c.floor);
        fnv1a(&mut h, &[c.ceiling.is_some() as u8]);
        if let Some(cl) = &c.ceiling {
            hash_floor(&mut h, cl);
        }
    }
    fnv1a(&mut h, &(op_idx as u64).to_le_bytes());
    for em in emissions.iter().filter(|em| em.op_idx < op_idx) {
        let op = &p.operations[em.op_idx];
        fnv1a(&mut h, &(em.op_idx as u64).to_le_bytes());
        fnv1a(&mut h, &(op.tool_num as u64).to_le_bytes());
        fnv1a(&mut h, &[matches!(op.comp, CompMode::Engine) as u8]);
        fnv1a(&mut h, &(op.feature_name.len() as u64).to_le_bytes());
        fnv1a(&mut h, op.feature_name.as_bytes());
        fnv1a(&mut h, &(em.moves.len() as u64).to_le_bytes());
        for line in &em.moves {
            fnv1a(&mut h, &(line.len() as u64).to_le_bytes());
            fnv1a(&mut h, line.as_bytes());
        }
        // The resolved channel is stamped input like the text (DECISIONS.md
        // "the control-comp centerline resolve" — cache honesty): a change in
        // resolved-generation must miss the cache, never stale-hit it.
        match &em.resolved {
            None => fnv1a(&mut h, &u64::MAX.to_le_bytes()),
            Some(lines) => {
                fnv1a(&mut h, &(lines.len() as u64).to_le_bytes());
                for line in lines {
                    fnv1a(&mut h, &(line.len() as u64).to_le_bytes());
                    fnv1a(&mut h, line.as_bytes());
                }
            }
        }
    }
    h
}

fn write_node(n: &Node, out: &mut Vec<u8>) {
    match n {
        Node::Air => out.push(0),
        Node::Material => out.push(1),
        Node::Leaf(brick) => {
            out.push(2);
            for v in brick.iter() {
                out.extend_from_slice(&v.to_bits().to_le_bytes());
            }
        }
        Node::Branch(ch) => {
            out.push(3);
            for c in ch.iter() {
                write_node(c, out);
            }
        }
    }
}

/// `None` on anything malformed — the caller treats it as stale.
fn read_node(bytes: &[u8], pos: &mut usize, cells: usize) -> Option<Node> {
    let tag = *bytes.get(*pos)?;
    *pos += 1;
    match tag {
        0 => Some(Node::Air),
        1 => Some(Node::Material),
        2 => {
            if cells != 8 {
                return None; // a brick where the tree still had to branch
            }
            let mut brick = Box::new([0f32; 512]);
            for v in brick.iter_mut() {
                let end = pos.checked_add(4)?;
                let raw: [u8; 4] = bytes.get(*pos..end)?.try_into().ok()?;
                *v = f32::from_bits(u32::from_le_bytes(raw));
                *pos = end;
            }
            Some(Node::Leaf(brick))
        }
        3 => {
            if cells <= 8 {
                return None; // branching below the brick level
            }
            let mut children: Vec<Node> = Vec::with_capacity(8);
            for _ in 0..8 {
                children.push(read_node(bytes, pos, cells / 2)?);
            }
            let arr: [Node; 8] = children.try_into().ok()?;
            Some(Node::Branch(Box::new(arr)))
        }
        _ => None,
    }
}

impl VoxField {
    fn to_bytes(&self) -> Vec<u8> {
        let mut body: Vec<u8> = Vec::new();
        body.extend_from_slice(&(self.incomplete_reasons.len() as u32).to_le_bytes());
        for r in &self.incomplete_reasons {
            body.extend_from_slice(&(r.len() as u32).to_le_bytes());
            body.extend_from_slice(r.as_bytes());
        }
        body.push(self.upward_closed as u8);
        body.extend_from_slice(&(self.prefix_ops as u32).to_le_bytes());
        body.extend_from_slice(&(self.cells as u32).to_le_bytes());
        body.extend_from_slice(&self.cell.to_bits().to_le_bytes());
        body.extend_from_slice(&self.tau.to_bits().to_le_bytes());
        for v in self.origin {
            body.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        for v in stock_words(&self.stock) {
            body.extend_from_slice(&v.to_bits().to_le_bytes());
        }
        write_node(&self.root, &mut body);

        let mut body_hash: u64 = 0xcbf2_9ce4_8422_2325;
        fnv1a(&mut body_hash, &body);

        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(VOX_MAGIC);
        out.extend_from_slice(&VOX_FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.input_hash.to_le_bytes());
        out.extend_from_slice(&body_hash.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn from_bytes(bytes: &[u8], expected_input_hash: u64) -> Option<VoxField> {
        let mut pos = 0usize;
        let take = |bytes: &[u8], pos: &mut usize, n: usize| -> Option<Vec<u8>> {
            let end = pos.checked_add(n)?;
            let out = bytes.get(*pos..end)?.to_vec();
            *pos = end;
            Some(out)
        };
        if take(bytes, &mut pos, 8)? != VOX_MAGIC {
            return None;
        }
        let version = u32::from_le_bytes(take(bytes, &mut pos, 4)?.try_into().ok()?);
        if version != VOX_FORMAT_VERSION {
            return None;
        }
        let input_hash = u64::from_le_bytes(take(bytes, &mut pos, 8)?.try_into().ok()?);
        if input_hash != expected_input_hash {
            return None;
        }
        let body_hash = u64::from_le_bytes(take(bytes, &mut pos, 8)?.try_into().ok()?);
        let body = &bytes[pos..];
        let mut check: u64 = 0xcbf2_9ce4_8422_2325;
        fnv1a(&mut check, body);
        if check != body_hash {
            return None;
        }

        let mut pos = 0usize;
        let n_reasons = u32::from_le_bytes(take(body, &mut pos, 4)?.try_into().ok()?) as usize;
        if n_reasons > 4096 {
            return None; // no honest field carries thousands of errors
        }
        let mut incomplete_reasons = Vec::with_capacity(n_reasons);
        for _ in 0..n_reasons {
            let len = u32::from_le_bytes(take(body, &mut pos, 4)?.try_into().ok()?) as usize;
            let raw = take(body, &mut pos, len)?;
            incomplete_reasons.push(String::from_utf8(raw).ok()?);
        }
        let upward_closed = take(body, &mut pos, 1)?[0] != 0;
        let prefix_ops = u32::from_le_bytes(take(body, &mut pos, 4)?.try_into().ok()?) as usize;
        let cells = u32::from_le_bytes(take(body, &mut pos, 4)?.try_into().ok()?) as usize;
        if cells < 8 || cells > (1 << 14) || !cells.is_power_of_two() {
            return None;
        }
        let f64s = |n: usize, pos: &mut usize| -> Option<Vec<f64>> {
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                let raw: [u8; 8] = take(body, pos, 8)?.try_into().ok()?;
                out.push(f64::from_bits(u64::from_le_bytes(raw)));
            }
            Some(out)
        };
        let cell = f64s(1, &mut pos)?[0];
        let tau = f64s(1, &mut pos)?[0];
        let o = f64s(3, &mut pos)?;
        let sv = f64s(STOCK_WORDS, &mut pos)?;
        let root = read_node(body, &mut pos, cells)?;
        if pos != body.len() {
            return None; // trailing garbage is corruption, not padding
        }
        Some(VoxField {
            origin: [o[0], o[1], o[2]],
            cells,
            cell,
            tau,
            root,
            stamps: None,
            prefix_air: None,
            stock: Stock {
                shape: if sv[6] > 0.5 { StockShape::Cylinder { dia: sv[7] } } else { StockShape::Box },
                w: sv[0],
                l: sv[1],
                h: sv[2],
                x: sv[3],
                y: sv[4],
                z: sv[5],
                material: None,
            },
            prefix_ops,
            input_hash,
            upward_closed,
            incomplete_reasons,
        })
    }

    /// Write the field to a `.vox` cache file. The write is plain (no temp
    /// and rename): a crash mid-write leaves a file the body checksum
    /// rejects, which reads as stale and rebuilds — the cache fails closed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        // A profile blank's field is not cached (Phase 54 slice 5a): the
        // file's eight stock words cannot carry its outline, and a loaded
        // field would stand on a blank it could not state. Rebuilt each
        // time, said here, never silently wrong.
        if matches!(self.stock.shape, StockShape::Profile { .. }) {
            return Err("voxel cache: a stock Profile's field is not cached — it is rebuilt each run".to_string());
        }
        std::fs::write(path, self.to_bytes())
            .map_err(|e| format!("voxel cache: failed to write {}: {e}", path.display()))
    }

    /// Load the end-state field for this part from a cache file — the
    /// whole-program prefix, one load path, not two.
    pub fn load(path: &Path, p: &Part, cell: Option<f64>) -> Result<Option<VoxField>, String> {
        Self::load_prefix(path, p, p.operations.len(), cell)
    }

    /// Load a prefix field from a cache file. `Ok(None)` — the one answer
    /// for a missing file, a stale hash, a foreign format, corruption, or a
    /// file holding a DIFFERENT prefix (the hash is computed over the prefix,
    /// so an entry can never answer for a state it doesn't hold) — means
    /// "rebuild"; it is never an error, because a cache must not be able to
    /// fail a run its absence would let succeed. The full program is
    /// re-emitted to compute the expected hash, so a cache hit never skips
    /// validation: what the hit saves is the octree build.
    pub fn load_prefix(
        path: &Path,
        p: &Part,
        op_idx: usize,
        cell: Option<f64>,
    ) -> Result<Option<VoxField>, String> {
        if op_idx > p.operations.len() {
            return Err(format!(
                "voxel field: prefix {op_idx} exceeds the program's {} ops",
                p.operations.len()
            ));
        }
        let (_, cell_mm) = resolve_cells(&p.stock, cell)?;
        let (_, emissions) = gcode::program_with_moves(p)?;
        let expected = input_hash(p, cell_mm, &emissions, op_idx);
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };
        Ok(VoxField::from_bytes(&bytes, expected))
    }

    /// The intended cache entry point: a valid cache loads, anything else
    /// rebuilds and rewrites — healing a stale or corrupt file in passing.
    pub fn load_or_build(p: &Part, path: &Path, cell: Option<f64>) -> Result<VoxField, String> {
        Self::load_or_build_prefix(p, p.operations.len(), path, cell)
    }

    /// `load_or_build` for a prefix field: a valid cache for THIS prefix
    /// loads, anything else rebuilds and rewrites.
    pub fn load_or_build_prefix(
        p: &Part,
        op_idx: usize,
        path: &Path,
        cell: Option<f64>,
    ) -> Result<VoxField, String> {
        if let Some(field) = Self::load_prefix(path, p, op_idx, cell)? {
            return Ok(field);
        }
        let field = Self::build_prefix(p, op_idx, cell)?;
        field.save(path)?;
        Ok(field)
    }
}

// ---- the rest-material report ------------------------------------------------

/// **The foul lint's field voice** (DECISIONS.md → "the profile silhouette…",
/// slice 3): a tool body standing in material that no pass removes, found at
/// the positions the tool ACTUALLY visits rather than the ones the declared
/// geometry forces. Advisory, one finding per op, on stderr beside the
/// analytic lint's — no directive and no error surface.
///
/// **Where it speaks, and why that is not most places.** Two gates, and each
/// one is a theorem rather than a preference:
///
/// - Only ops whose tool has a TRUNCATED silhouette. For a rod, the stamp IS
///   the body, so the end-state field is void up the whole body column at
///   every position the op visits — a rod can never foul its own program's
///   field (the voxel engine's slice-3 theorem, pinned by
///   `test_no_emitted_position_fouls_its_own_field`). Running the query there
///   would be paying for a guaranteed answer, so the gate is also what keeps
///   this lint free for every program without a form cutter.
/// - Only ops the ANALYTIC model declines (`stock::remaining_after` is
///   incomplete). Where that model is exact it is the authority and already
///   asks this question with the full silhouette; two voices on one op would
///   be one voice too many. The declines are not rare and not going away: a
///   boss's cleared surround is a fact about the PATH, permanently outside a
///   declaration-shaped model, and incompleteness is contagious forward — one
///   unmodeled op silences the analytic voice for everything after it. The
///   field, which is nothing but path truth, carries on.
///
/// The positions come from the emitted moves, so this reads the resolved centerline
/// where an emitter authored one — the same channel the stamps were built
/// from, which is the only way the query and the field can be talking about
/// the same tool.
pub fn body_foul_findings(p: &Part, emissions: &[gcode::OpEmission]) -> Vec<crate::parse::Finding> {
    let mut out = Vec::new();
    // Read once: a fact about the declaration, not about any op (Phase 9
    // lane B, slice 1 — the field voice's declines).
    let uncontained = crate::stock::uncontained(p);
    for (k, op) in p.operations.iter().enumerate() {
        let tool = match p.tools.get(&op.tool_num) {
            Some(t) => t,
            None => continue,
        };
        let sil = Silhouette::of(tool);
        if sil.cutting_top().is_none() {
            continue; // a rod cannot foul its own field: the theorem
        }
        if crate::stock::remaining_after(p, k).complete {
            continue; // the analytic voice is the authority here
        }
        let em = match emissions.iter().find(|e| e.op_idx == k) {
            Some(e) => e,
            None => continue,
        };
        // The field as it stands when this op finishes — the same "material
        // that outlives the op" the analytic rule names, read from the sweep.
        let field = match VoxField::build_prefix_from_emissions(p, emissions, k + 1, None) {
            Ok(f) => f,
            Err(_) => continue,
        };
        if !field.complete() {
            // An incomplete field over-states material; it may not accuse —
            // and since Phase 9 lane B it says so (fork 3A) instead of going
            // quiet: this is the one op the field voice was the authority on,
            // and a wordless skip here was a clean bill nobody earned.
            let about = crate::stock::subjects(p, op).first().map_or(op.feature_name.clone(), |s| s.name.clone());
            out.push(crate::parse::Finding::op(crate::parse::FindingFamily::Decline, k, &about, format!(
                "field body-foul probe declined on '{}' ({} with T{}): the swept field is incomplete — {} — so it over-states material, and a foul read off it could accuse the tool of standing in air. Fix the named reason to restore it",
                about, crate::stock::op_word(op), op.tool_num, field.incomplete_reasons.join("; ")
            )));
            continue;
        }

        // What a foul here is ABOUT: the hole under the tool for a drill, the
        // op's one feature otherwise — the same attribution the reach check and
        // the error make (`stock::subjects`), so three voices cannot name
        // three different things for one position.
        let subjects = crate::stock::subjects(p, op);
        let mut m = Motion { rapid: true, x: None, y: None, z: None, f: None };
        for line in gcode::resolved_moves(em) {
            if !gcode::apply_move(line, &mut m) {
                continue;
            }
            let (x, y, z) = match (m.x, m.y, m.z) {
                (Some(x), Some(y), Some(z)) => (x, y, z),
                _ => continue, // an unplaceable move is not a position to judge
            };
            let tip = z - sil.drop();
            // The scan starts at the CUTTING TOP, not the tip — which is what
            // this finding has always claimed ("above its cutting top the tool
            // removes nothing") and what it now actually does.
            //
            // Below that height the tool is cutting, and a form cutter's
            // cutting surface is TANGENT to the surface it leaves: that is what
            // a form cut is. Sampling the body there asks whether a point lying
            // exactly on the finished surface is inside material, which is a
            // coin flip at any cell size, in both directions. A corner rounder
            // made this unmissable — it is tangent along its whole arc — but the
            // fault was never the tool's (DECISIONS.md → "the corner rounder
            // gets an operation"). What genuinely cannot cut, and therefore
            // genuinely cannot have opened the column it stands in, is the shank
            // and shoulder above.
            let cut_top = sil.cutting_top().expect("the rods returned above");
            let base = tip + cut_top;
            let reach = p.stock.z - base;
            if reach <= 0.0 {
                continue; // above the stock there is nothing to foul on
            }
            if let BodyVerdict::Foul { h, x: fx, y: fy, z: fz } =
                field.body_clearance(&|h| sil.radius_at(cut_top + h), x, y, base, reach)
            {
                let about = crate::stock::subject_at(&subjects, op, x, y);
                out.push(crate::parse::Finding::op(crate::parse::FindingFamily::FieldBodyFoul, k, about, format!(
                    "T{}'s body {:.3}mm above the tip stands in material that outlives '{}' at ({:.3}, {:.3}, {:.3}) — above its cutting top the tool removes nothing, so no pass opened that column and the shank is rubbing. Rough the surrounding stock in an earlier op, or declare the stock to what is actually on the machine. (Read from the swept field: this operation's declared model is incomplete, so the analytic lint cannot see it) — the tip at ({:.3}, {:.3}, {:.3})",
                    op.tool_num, cut_top + h, about, fx, fy, fz, x, y, tip
                )).with_place([fx, fy, fz]).with_tip([x, y, tip], op.tool_num));
                break; // one foul per op is the actionable message
            }
        }
        // Nothing found — and if the op works a feature the stock does not
        // contain, that nothing is not a claim: the field was stamped from a
        // stock with no material where the part stands, so the sweep had
        // nothing to leave standing there (Phase 9 lane B, slice 1). A foul
        // FOUND beside such a feature stands: the stock it found is real.
        if !out.iter().any(|f| matches!(f.subject, crate::parse::FindingSubject::Op { index, .. } if index == k)) {
            if let Some(u) = crate::stock::uncontained_for(p, k, &uncontained) {
                out.push(crate::parse::Finding::op(crate::parse::FindingFamily::Decline, k, &u.feature, format!(
                    "field body-foul probe declined on '{}' ({} with T{}): {} — the quiet it read here is not a claim: the sweep never had material where the part stands, so the body may stand in stock the field never saw. Declare the stock the part is actually made from, or move the feature's datum into it",
                    u.feature, crate::stock::op_word(op), op.tool_num, u.reason
                )));
            }
        }
    }
    out
}

/// "Did we forget material" — the `complete` flags' first consumer, and the
/// place the two truths meet on purpose: the field is the SWEEP's truth, the
/// declared model is the DECLARATION's, and *forgotten* material is exactly
/// their honest divergence — cells the declarations claim removed that no
/// pass ever swept (a bore declared void whose core no op cleared).
///
/// Total accounting, both directions: if the field is incomplete the sweep
/// side over-states material, if the declared model is incomplete the
/// baseline over-states it — either way the comparison would accuse or
/// absolve wrongly, so the report DECLINES and says why (the reasons are in
/// the text, not swallowed into a boolean). A declining report is still a
/// report.
///
/// The counted set is banded the same way the test oracle is: a cell counts
/// only when the field says material at its center (the sign is exact there)
/// AND the declared model says void at the center and at ±band along every
/// axis — so scallop cusps, chord slop, and corner fillets within the band
/// of a declared surface stay out, and what remains is material a consumer
/// could actually go remove. The band is stated in the header.
pub fn rest_report(p: &Part, cell: Option<f64>) -> Result<String, String> {
    let field = VoxField::build_with_cell(p, cell)?;
    let model = if p.operations.is_empty() {
        crate::stock::remaining_after(p, 0)
    } else {
        crate::stock::remaining_after(p, p.operations.len() - 1)
    };
    let band = field.cell * 3f64.sqrt() + 0.03;

    let mut out = String::new();
    out.push_str(&format!(
        "rest-material report — {n}³ cells at {cell:.3}mm, band {band:.3}mm\n",
        n = field.cells,
        cell = field.cell,
    ));
    if field.complete() {
        out.push_str("swept field: complete\n");
    } else {
        out.push_str("swept field: INCOMPLETE —\n");
        for r in &field.incomplete_reasons {
            out.push_str(&format!("  - {r}\n"));
        }
    }
    let missing = crate::stock::incomplete_ops(p);
    // What the program before this one left that the model could not carry
    // (Phase 48 slice 1) is missing at prefix 0 — incomplete before any op.
    let declined = crate::stock::given_from_declines(p);
    if missing.is_empty() && declined.is_empty() {
        out.push_str("declared model: complete\n");
    } else {
        out.push_str("declared model: INCOMPLETE —\n");
        for line in &declined {
            out.push_str(&format!("  - given from: {line}\n"));
        }
        for (k, feature) in &missing {
            out.push_str(&format!("  - {}\n", crate::stock::incomplete_reason(p, *k, feature)));
        }
    }
    // The stock, judged apart from the model (Phase 9 lane B, slice 1): the
    // model above may be COMPLETE — every op has its closed form — and still
    // be measured down from a stock the part is not in. A feature a run op
    // works that the stock does not contain is stated here under its own
    // key, because it is a different fact with the opposite sign: the model
    // UNDER-states material, so every number below would be measured against
    // air where the part is. A declared-but-unrun feature standing proud is
    // the outline's "declared, never run" case and declines nothing.
    let worked = crate::stock::uncontained_worked(p);
    if !worked.is_empty() {
        out.push_str("stock: UNCONTAINED —\n");
        for (u, k) in &worked {
            out.push_str(&format!(
                "  - '{}' ({}) stands {} — and op [{}] works it\n",
                u.name,
                u.kind,
                u.faces.first().map_or(String::new(), |(_, s)| s.clone()),
                k
            ));
        }
    }
    if !field.complete() || !missing.is_empty() || !declined.is_empty() {
        out.push_str(
            "no claim: an incomplete side over-states material, and a comparison against an over-statement would accuse or absolve wrongly — the report declines\n",
        );
        return Ok(out);
    }
    if !worked.is_empty() {
        out.push_str(
            "no claim: the stock does not contain a feature the program works, so the model has no material where the part stands and every count below would be measured against air where the part is — the report declines\n",
        );
        return Ok(out);
    }

    let part = crate::stock::DeclaredPart::of(p);
    let scan = scan(p, &field, &model, &part, band);
    let cell_vol = field.cell * field.cell * field.cell;
    out.push_str(&format!(
        "standing material: {} cells (~{:.1} mm³)\n",
        scan.standing,
        scan.standing as f64 * cell_vol
    ));
    match scan.forgotten_bb {
        None => out.push_str(
            "forgotten (declared removed, never swept): none — every declared removal is swept to within the band\n",
        ),
        Some(b) => {
            out.push_str(&format!(
                "forgotten (declared removed, never swept): {} cells (~{:.1} mm³)\n",
                scan.forgotten,
                scan.forgotten as f64 * cell_vol
            ));
            out.push_str(&format!(
                "  bounding box: x {:.3}..{:.3}, y {:.3}..{:.3}, z {:.3}..{:.3}\n",
                b[0], b[3], b[1], b[4], b[2], b[5]
            ));
        }
    }
    // The other direction of the same comparison (Phase 9 lane C): cells the
    // DECLARED PART says stand that the sweep read as air. Same loop, same
    // band, opposite sign — and the text here IS the finding's text, so the
    // report and `part_violations` cannot say two things about one cell.
    let violations = violation_findings(&scan, p, &field, band);
    if violations.is_empty() {
        out.push_str(
            "violated (declared part, swept anyway): none — nothing the declaration says stands was swept deeper than the band\n",
        );
    } else {
        let cells: usize = scan.violated.values().map(|v| v.cells).sum();
        out.push_str(&format!(
            "violated (declared part, swept anyway): {} cells (~{:.1} mm³)\n",
            cells,
            cells as f64 * cell_vol
        ));
        for f in &violations {
            out.push_str(&format!("  {}\n", f.message));
        }
    }
    Ok(out)
}

/// The rest report's cells, for drawing (Phase 11, lane B): the same scan
/// `rest_report` counts from, run over the field after the first `k` ops
/// (the whole program when `k` is the op count) against the model after
/// the same `k` — so the window's cells at a scrub position are exactly the
/// report's classification, never a second reading. Declines where the
/// report declines, in the report's own words: an incomplete model, or a
/// feature the stock does not contain.
pub struct RestCells {
    pub cell: f64,
    pub forgotten: Vec<[f64; 3]>,
    pub violated: Vec<[f64; 3]>,
}

pub fn rest_cells(p: &Part, emissions: &[gcode::OpEmission], k: usize, cell: Option<f64>) -> Result<RestCells, String> {
    let k = k.min(p.operations.len());
    let model = crate::stock::remaining_before(p, k);
    if !model.complete {
        let missing: Vec<String> = crate::stock::incomplete_ops(p)
            .into_iter()
            .filter(|(i, _)| *i < k)
            .map(|(i, why)| format!("op [{}] — {}", i, why))
            .collect();
        if !missing.is_empty() {
            return Err(format!("the rest cells decline — declared model incomplete: {}", missing.join("; ")));
        }
    }
    if !crate::stock::uncontained_worked(p).is_empty() {
        return Err("the rest cells decline — the stock does not contain a feature the program works".to_string());
    }
    let field = VoxField::build_prefix_from_emissions(p, emissions, k, cell)?;
    if !field.complete() {
        return Err(format!("the rest cells decline — swept field incomplete: {}", field.incomplete_reasons.join("; ")));
    }
    let band = field.cell * 3f64.sqrt() + 0.03;
    let part = crate::stock::DeclaredPart::of(p);
    let scan = scan(p, &field, &model, &part, band);
    Ok(RestCells { cell: field.cell, forgotten: scan.forgotten_cells, violated: scan.violated_cells })
}

/// One pass over the cells: what stands, what was forgotten (declared void,
/// never swept), and what was violated (declared solid, swept anyway). Both
/// comparisons are banded the same way — the cell and its six ±band
/// neighbours must agree on the declaration's side — so the sign's sub-cell
/// slop at a tangent surface (8µm on a finished sphere, measured) counts for
/// neither. A violation is attributed to whoever claims the cell's CENTRE.
struct Scan {
    standing: usize,
    forgotten: usize,
    forgotten_bb: Option<[f64; 6]>,
    /// The cells themselves, kept so the window can draw what the report
    /// counts from the same pass (Phase 11, lane B): one centre per
    /// forgotten cell, one per violated cell.
    forgotten_cells: Vec<[f64; 3]>,
    violated_cells: Vec<[f64; 3]>,
    /// Keyed by the claimant's name (`stock` for the stock's remainder), in
    /// name order so the report is deterministic.
    violated: std::collections::BTreeMap<String, Violated>,
}

struct Violated {
    cells: usize,
    /// The lowest violated cell — the deepest the sweep went into the claim.
    lowest: (f64, f64, f64),
    /// Who cut the lowest cell, and with what kind of move (slice 1).
    lowest_by: Option<(usize, &'static str)>,
    bb: [f64; 6],
    /// Cells per op, read from the built field's own stamps; a cell no stamp
    /// reaches is counted under `unattributed` rather than guessed at.
    by_op: std::collections::BTreeMap<usize, usize>,
    unattributed: usize,
}

fn grow(bb: &mut [f64; 6], x: f64, y: f64, z: f64) {
    bb[0] = bb[0].min(x);
    bb[1] = bb[1].min(y);
    bb[2] = bb[2].min(z);
    bb[3] = bb[3].max(x);
    bb[4] = bb[4].max(y);
    bb[5] = bb[5].max(z);
}

fn scan(p: &Part, field: &VoxField, model: &crate::stock::RemainingStock, part: &crate::stock::DeclaredPart, band: f64) -> Scan {
    let n = field.cells;
    // What is GIVEN was never this program's to sweep (Phase 42 slice 1):
    // the field is the sweep's truth and still holds it as material, the
    // declared model subtracts it at prefix 0 — and that divergence is not
    // forgotten material, it is the other setup's work. Excluded by name
    // from the forgotten count, through the model's own prefix-0 answer.
    let given = crate::stock::remaining_before(p, 0);
    let mut scan = Scan {
        standing: 0,
        forgotten: 0,
        forgotten_bb: None,
        forgotten_cells: Vec::new(),
        violated_cells: Vec::new(),
        violated: Default::default(),
    };
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let (x, y, z) = field.cell_center(i, j, k);
                let ring = [
                    (x - band, y, z),
                    (x + band, y, z),
                    (x, y - band, z),
                    (x, y + band, z),
                    (x, y, z - band),
                    (x, y, z + band),
                ];
                if field.value(x, y, z) < 0.0 {
                    scan.standing += 1;
                    let deep_void = given.material_at(x, y, z)
                        && !model.material_at(x, y, z)
                        && ring.iter().all(|&(px, py, pz)| !model.material_at(px, py, pz));
                    // A declared void's edge cells abut the stock's own faces at
                    // the stock top; a probe past a face reads void trivially, so
                    // the erosion above never under-counts there — but the cell
                    // itself must still be a declared removal's interior, which
                    // `material_at` already answered at the center.
                    if deep_void {
                        scan.forgotten += 1;
                        scan.forgotten_cells.push([x, y, z]);
                        let b = scan.forgotten_bb.get_or_insert([x, y, z, x, y, z]);
                        grow(b, x, y, z);
                    }
                    continue;
                }
                // Air in the sweep. A violation only where the part is solid by
                // more than a band on every side — the mirror of the rule above.
                let Some(claim) = part.claim_at(x, y, z) else { continue };
                if !ring.iter().all(|&(px, py, pz)| part.claim_at(px, py, pz).is_some()) {
                    continue;
                }
                let key = match claim {
                    crate::stock::PartClaim::Feature(name) => name.to_string(),
                    crate::stock::PartClaim::Stock => "stock".to_string(),
                };
                scan.violated_cells.push([x, y, z]);
                // Who cut it: the stamp standing furthest inside at this cell,
                // from the field's own stamps — no second build (slice 1).
                let cut_by = field.deepest_sweep_at(x, y, z);
                let v = scan.violated.entry(key).or_insert(Violated {
                    cells: 0,
                    lowest: (x, y, z),
                    lowest_by: cut_by,
                    bb: [x, y, z, x, y, z],
                    by_op: Default::default(),
                    unattributed: 0,
                });
                v.cells += 1;
                if z < v.lowest.2 || v.cells == 1 {
                    v.lowest = (x, y, z);
                    v.lowest_by = cut_by;
                }
                match cut_by {
                    Some((op, _)) => *v.by_op.entry(op).or_insert(0) += 1,
                    None => v.unattributed += 1,
                }
                grow(&mut v.bb, x, y, z);
            }
        }
    }
    scan
}

fn violation_findings(scan: &Scan, p: &Part, field: &VoxField, band: f64) -> Vec<crate::parse::Finding> {
    let cell_vol = field.cell * field.cell * field.cell;
    scan.violated
        .iter()
        .map(|(name, v)| {
            let who = match p.features.get(name) {
                Some(f) => format!("'{}' ({}): the sweep removed material the declaration says stands", name, f.kind()),
                None => "the stock's declared remainder: the sweep removed material no declaration gives away".to_string(),
            };
            let (lx, ly, lz) = v.lowest;
            crate::parse::Finding::feature(crate::parse::FindingFamily::PartViolation, name, format!(
                "{who} — {} cells (~{:.1} mm³) more than {band:.3}mm inside it read air in the swept field, the lowest at ({lx:.3}, {ly:.3}, {lz:.3}), bounding box x {:.3}..{:.3}, y {:.3}..{:.3}, z {:.3}..{:.3}. {} Advisory and biased to silence: a gouge shallower than the band is not counted. A standing feature inside a cavity's footprint is that cavity's ISLAND, and the cavity's own ops owe it a berth (DECISIONS.md \"Phase 10 lane D — overlap semantics…\")",
                v.cells,
                v.cells as f64 * cell_vol,
                v.bb[0], v.bb[3], v.bb[1], v.bb[4], v.bb[2], v.bb[5],
                swept_by(p, v)
            )).with_place([lx, ly, lz])
        })
        .collect()
}

/// The sentence that names who cut a claimant's cells, from the field's own
/// stamps: every op with a share, largest first, and the move kind at the
/// lowest cell. Ops are numbered as the rest report numbers them (1-based,
/// the `run` order); a Drill has no feature name and is not given one.
fn swept_by(p: &Part, v: &Violated) -> String {
    let label = |op_idx: usize| -> String {
        let op = &p.operations[op_idx];
        let verb = op.op_type.verb();
        if op.feature_name.is_empty() {
            format!("{verb} (op [{}])", op_idx)
        } else {
            format!("{verb} on '{}' (op [{}])", op.feature_name, op_idx)
        }
    };
    let mut shares: Vec<(usize, usize)> = v.by_op.iter().map(|(&op, &n)| (op, n)).collect();
    shares.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut out = String::new();
    match shares.as_slice() {
        [] => out.push_str("Swept by no stamped move the field can name"),
        [(op, _)] if v.unattributed == 0 => out.push_str(&format!("Swept by {}: all of it", label(*op))),
        _ => {
            out.push_str("Swept by ");
            let parts: Vec<String> = shares.iter().map(|(op, n)| format!("{} ({n} cells)", label(*op))).collect();
            out.push_str(&parts.join(", "));
        }
    }
    if v.unattributed > 0 {
        out.push_str(&format!(", with {} cells no stamped move reaches", v.unattributed));
    }
    match v.lowest_by {
        Some((op, kind)) => out.push_str(&format!("; at its lowest by {kind} of op [{}].", op)),
        None => out.push('.'),
    }
    out
}

/// The part-violation findings alone — "show all", the T2 part-violation
/// entry's first flavour (Phase 9 lane C, `DECISIONS.md` → "Phase 9 lane C —
/// over-cut of the declared part, in the field…"): the swept field read
/// against `stock::DeclaredPart`, one finding per violated claimant. About the
/// FEATURE violated (`Finding::feature`, the containment lint's shape); the op
/// that cut it is slice 1's attribution and is not guessed at here.
///
/// Biased to silence, advisory forever (fork 1A): a truncated signed-distance
/// leaf is exact in sign at the cell centre, so a gouge deeper than one band
/// is certain and a shallower one is a coin flip — the finished spheres read
/// air in 35 cells 8µm inside their own surface and in 0 once banded. The
/// band makes this an UNDER-claim, which under the family's law ("a gate must
/// not promise what a probe cannot") disqualifies it from ever gating, exactly
/// as `body_foul_findings` is disqualified and for the same reason.
///
/// Declines to an empty list, never to a guess, when either side is
/// incomplete: an incomplete field over-states material and an incomplete
/// model over-states the part, and both would accuse wrongly. `rest_report`
/// says why; this function is for a caller that already holds the field.
pub fn part_violations(p: &Part, field: &VoxField) -> Vec<crate::parse::Finding> {
    let part = crate::stock::DeclaredPart::of(p);
    if !field.complete() || !part.complete() {
        return Vec::new();
    }
    let model = if p.operations.is_empty() {
        crate::stock::remaining_after(p, 0)
    } else {
        crate::stock::remaining_after(p, p.operations.len() - 1)
    };
    let band = field.cell * 3f64.sqrt() + 0.03;
    let scan = scan(p, field, &model, &part, band);
    violation_findings(&scan, p, field, band)
}

fn push_stamp(stamps: &mut Vec<Stamp>, op: usize, sil: Silhouette, path: PathK) {
    let r = sil.max_radius();
    let drop = sil.drop();
    // A rod reaches forever; a truncated silhouette stops a cutting top above
    // the highest tip it visits (plus a vertical move's lift, which raises the
    // cap by the travel). Generous, never tight-and-wrong, as the culling
    // contract requires.
    let ceiling = |highest_tip: f64, lift: f64| match sil.cutting_top() {
        None => f64::INFINITY,
        Some(top) => highest_tip + top + lift,
    };
    let (aabb, eps) = match &path {
        PathK::Point { x, y, z, lift } => {
            ([x - r, y - r, z - drop, x + r, y + r, ceiling(*z, *lift)], 0.0)
        }
        PathK::HSeg { ax, ay, bx, by, z } => (
            [
                ax.min(*bx) - r,
                ay.min(*by) - r,
                z - drop,
                ax.max(*bx) + r,
                ay.max(*by) + r,
                ceiling(*z, 0.0),
            ],
            0.0,
        ),
        PathK::HArc { cx, cy, ra, z, .. } => (
            // The full circle's box: generous for partial arcs, never wrong.
            [cx - ra - r, cy - ra - r, z - drop, cx + ra + r, cy + ra + r, ceiling(*z, 0.0)],
            0.0,
        ),
        PathK::Slope { a, b } => {
            let len3 = {
                let e = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt()
            };
            (
                [
                    a[0].min(b[0]) - r,
                    a[1].min(b[1]) - r,
                    a[2].min(b[2]) - drop,
                    a[0].max(b[0]) + r,
                    a[1].max(b[1]) + r,
                    ceiling(a[2].max(b[2]), 0.0),
                ],
                len3 / (2.0 * (SLOPE_SAMPLES - 1) as f64),
            )
        }
    };
    stamps.push(Stamp { op, sil, path, aabb, eps });
}

// ---- unit tests: the stamp math on its own terms -----------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // A deterministic LCG — tests need reproducible probes, not entropy.
    fn lcg(state: &mut u64) -> f64 {
        *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*state >> 11) as f64) / ((1u64 << 53) as f64)
    }

    /// Cache honesty (DECISIONS.md "the control-comp centerline resolve"):
    /// the input hash covers the resolved channel — same text, different
    /// resolved motion, different hash — so a resolved-generation change
    /// misses the cache, never stale-hits it.
    #[test]
    fn input_hash_covers_the_resolved_channel() {
        let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
            stock Box(w=10, l=10, h=5, x=0, y=0, z=0)\n\
            tool T5 Endmill(dia=2.0)\n\
            feature Top Plane(z=-1.0)\n\
            run End(tool=T5, surface=Top, stepover=1.0)\n";
        let p = crate::parse::parse(src).unwrap();
        let mk = |resolved: Option<Vec<String>>| {
            vec![gcode::OpEmission { op_idx: 0, moves: vec!["G01 X1 Y1 F800".into()], resolved, lines: 0..1, moves_start: 0 }]
        };
        let none = input_hash(&p, 1.0, &mk(None), 1);
        let a = input_hash(&p, 1.0, &mk(Some(vec!["G01 X2 Y2 F800".into()])), 1);
        let b = input_hash(&p, 1.0, &mk(Some(vec!["G01 X3 Y3 F800".into()])), 1);
        assert_ne!(none, a, "presence of the resolved channel must move the hash");
        assert_ne!(a, b, "the resolved lines themselves must move the hash");
    }

    /// The air at prefix 0 is stamped input (Phase 48 slice 1): a program
    /// that gives a void and the same program that does not build different
    /// fields from identical moves, so they must key different cache
    /// entries — or the cache is the stale thing.
    #[test]
    fn input_hash_covers_the_prefix_air() {
        let head = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
            stock Box(w=10, l=10, h=5, x=0, y=0, z=0)\n\
            tool T5 Endmill(dia=2.0)\n\
            feature Hole CylinderBore(dia=4.0, extent=5.0, x=5.0, y=5.0)\n";
        let plain = crate::parse::parse(head).unwrap();
        let given = crate::parse::parse(&format!("{head}given Hole\n")).unwrap();
        assert_ne!(input_hash(&plain, 1.0, &[], 0), input_hash(&given, 1.0, &[], 0), "a given void must move the hash");
    }

    /// **The fold's own pin** (DECISIONS.md → "the profile silhouette…", slice
    /// 1): the stamp's boundary IS the curve the analytic lint samples. Both
    /// voices now read `Silhouette`, so this asserts the thing that used to be
    /// true only by hand — at every height above the TIP, the swept solid's
    /// surface sits at exactly `radius_at(h)`, with `drop()` the one place the
    /// ball's center-vs-tip convention is spoken. A regression here is the two
    /// voices drifting apart, which is the whole reason the type exists; slice
    /// 2 extends this test to the `Profile` arm rather than replacing it.
    /// **The radial arm, isolated** (DECISIONS.md → "the profile silhouette…",
    /// slice 3, fork 3). A cleared bore, and an axis parked in the middle of
    /// the void: the axis is in air at every height, so the arm that reads only
    /// the axis column can never call this anything but `Uncertain` — and no
    /// Lipschitz argument rescues it, because an air value bounds neighbours
    /// from BELOW and a lower bound cannot prove a value negative. Widen the
    /// body past the bore wall and the ring finds the wall.
    ///
    /// The contrast is the pin: same field, same position, same heights — only
    /// the RADIUS differs. The voxel engine's slice 3 called its own body pin
    /// "radius-independent"; this check result is not, and that is the whole point.
    #[test]
    fn the_body_verdict_reads_the_wall_the_axis_cannot_see() {
        let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
            stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
            tool T5 Endmill(dia=10.0)\n\
            feature Bore CylinderBore(dia=40.0, extent=12.0, x=50.0, y=50.0)\n\
            run Clear(tool=T5, profile=Bore, stepover=4.0)\n";
        let p = crate::parse::parse(src).unwrap();
        let field = VoxField::build_with_cell(&p, Some(0.5)).unwrap();

        // A thin body down the cleared middle: never a foul. (Whether it is
        // `Clear` or `Uncertain` is a question about tau, not about the wall —
        // certified clearance stops at the truncation band, as documented — so
        // the pin is on the silence, which is what the lint acts on.)
        assert!(
            !matches!(
                field.body_clearance(&|_| 2.0, 50.0, 50.0, -10.0, 10.0),
                BodyVerdict::Foul { .. }
            ),
            "a thin body in the cleared void must not be accused"
        );
        // The same body at the same place, wide enough to reach past the
        // dia-40 wall: the ring lands in standing stock.
        match field.body_clearance(&|_| 25.0, 50.0, 50.0, -10.0, 10.0) {
            BodyVerdict::Foul { x, y, .. } => {
                let rho = (x - 50.0).phypot(y - 50.0);
                assert!(rho > 20.0, "the foul must sit outside the bore wall, got rho {rho}");
            }
            v => panic!("a body wider than the bore must foul on its wall, got {v:?}"),
        }
    }

    /// **What the truncated stamp costs** (DECISIONS.md → "the profile
    /// silhouette…", slice 2, fork 2): `band_certified_void` reads ONE plane
    /// and answers for the column above it, which is the upward-closure lemma
    /// every rod stamp gives it for free. A form cutter's stamp stops at its
    /// cutting top, so the lemma is gone and the certifier declines — the
    /// direction it always errs in (cut the loop; skipping material is the
    /// gouge of omission).
    ///
    /// White-box on purpose: the flag is flipped on an otherwise-certified
    /// band so the guard is isolated from the geometry, in the spirit of
    /// `stock::tests::cavity_probe_fires_when_the_shoulder_cannot_follow_the_tip`.
    #[test]
    fn a_truncated_stamp_costs_the_certifier_its_lemma() {
        let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
            stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
            tool T5 Endmill(dia=10.0)\n\
            feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
            run Clear(tool=T5, profile=Bore, stepover=4.0)\n";
        let p = crate::parse::parse(src).unwrap();
        let mut field = VoxField::build_with_cell(&p, Some(0.5)).unwrap();
        let band = [(46.0, 50.0), (54.0, 50.0)];
        assert!(
            field.band_certified_void(&band, 3.0, -6.0),
            "the cleared bore's middle must certify, or the pin is vacuous"
        );
        field.upward_closed = false;
        assert!(
            !field.band_certified_void(&band, 3.0, -6.0),
            "without the lemma the same band must not certify"
        );
    }

    /// **The fence, and the incompleteness law's remaining evidence**
    /// (DECISIONS.md → "the profile silhouette…", slice 2). A silhouette that
    /// narrows going up is not stamped: it is the one tool class that can
    /// undercut, and its swept void is not upward-closed even within a single
    /// stamp, so feeding it to the lemma would be feeding the lemma a shape
    /// that violates it. The field goes incomplete with the reason named, and
    /// every consumer that must decline can say why.
    ///
    /// This is reached with a hand-built emission (the `input_hash` test's
    /// precedent) because the front door is shut upstream: a profile tool can
    /// only reach `Finish` on a `Sphere`, whose contact math rejects a
    /// non-flaring profile first. The fence is local to the invariant it
    /// protects rather than borrowing a finishing-specific precondition — the
    /// day the corner-rounder gets its own operation, this is what still holds.
    #[test]
    fn a_necked_silhouette_refuses_by_name() {
        let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
            stock Box(w=40, l=40, h=10, x=0, y=0, z=0)\n\
            tool T5 Endmill(dia=10.0)\n\
            feature Top Plane(z=-1.0)\n\
            run End(tool=T5, surface=Top, stepover=4.0)\n";
        let mut p = crate::parse::parse(src).unwrap();
        let moves = vec!["G00 X5 Y5 Z2".to_string(), "G01 Z-1 F800".to_string()];
        let ems = vec![gcode::OpEmission { op_idx: 0, lines: 0..moves.len(), moves, resolved: None, moves_start: 0 }];

        // Widens to r 5 at 4mm, then necks back to r 2: a T-slot's shape.
        let necked = vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 5.0, y: 4.0 }, Pt { x: 2.0, y: 8.0 }];
        p.tools.get_mut(&5).unwrap().profile = Some(necked);
        let (stamps, reasons) = walk_emissions(&p, &ems, 1, None);
        assert!(stamps.is_empty(), "a necked tool must contribute no stamps");
        assert_eq!(reasons.len(), 1, "one reason, named: {reasons:?}");
        assert!(reasons[0].contains("narrows going up"), "got: {}", reasons[0]);

        // The positive control, so the fence is not just rejecting everything:
        // the same tool, monotone, stamps and completes.
        let flared = vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 5.0, y: 4.0 }, Pt { x: 5.0, y: 8.0 }];
        p.tools.get_mut(&5).unwrap().profile = Some(flared);
        let (stamps, reasons) = walk_emissions(&p, &ems, 1, None);
        assert!(reasons.is_empty(), "a flaring profile is stampable: {reasons:?}");
        assert!(!stamps.is_empty(), "and it actually stamps");
    }

    /// The fixture form cutter (`sphere_formtool.cam`, post-scale): a 45°
    /// cone from the tip to (5, 5), then a vertical wall to (5, 10). Monotone,
    /// so the walker stamps it.
    fn cone_tool() -> Silhouette {
        Silhouette::Profile {
            pts: vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 5.0, y: 5.0 }, Pt { x: 5.0, y: 10.0 }],
            shank_r: 5.0,
        }
    }

    /// **The split, pinned** (DECISIONS.md → "the profile silhouette…", slice
    /// 2): below the cutting top the stamp's boundary is the silhouette, and
    /// ABOVE it the stamp claims nothing while the body is still 5mm wide.
    /// That gap is the whole slice — the shank occupies without removing — and
    /// it is what gives the foul lint's field voice something to find.
    #[test]
    fn the_stamp_stops_at_the_cutting_top_while_the_body_does_not() {
        let sil = cone_tool();
        let top = sil.cutting_top().expect("a profile has a cutting top");
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, sil.clone(), PathK::Point { x: 0.0, y: 0.0, z: 0.0, lift: 0.0 });
        let stamp = &stamps[0];

        // Below the top: the swept boundary IS the silhouette, exactly.
        for i in 0..=40 {
            let h = top * i as f64 / 40.0;
            let d = stamp.d_swept(sil.radius_at(h), 0.0, h);
            assert!(d.abs() < 1e-9, "at h={h} the boundary must sit at r={}, got {d}", sil.radius_at(h));
        }
        // Above it: the body still measures 5mm (the shank the lint samples),
        // and the stamp reads that same point as untouched material.
        for h in [top + 0.5, top + 4.0] {
            assert_eq!(sil.radius_at(h), 5.0, "the body continues as its shank");
            assert!(
                stamp.d_swept(0.0, 0.0, h) > 0.0,
                "the shank's column at h={h} must not be claimed as swept"
            );
        }
    }

    /// A vertical move's `lift`: the union of truncated instances is the
    /// lowest one with its cap extended by the travel, exact because the
    /// silhouette never narrows. Checked against the union itself.
    #[test]
    fn a_vertical_move_lifts_the_cap_by_its_travel() {
        let sil = cone_tool();
        let (lo, hi) = (-4.0, 3.0);
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, sil.clone(), PathK::Point { x: 0.0, y: 0.0, z: lo, lift: hi - lo });
        let stamp = &stamps[0];
        let mut state = 7u64;
        for _ in 0..400 {
            let p = [
                (lcg(&mut state) - 0.5) * 20.0,
                (lcg(&mut state) - 0.5) * 20.0,
                (lcg(&mut state) - 0.5) * 40.0,
            ];
            let rho = p[0].phypot(p[1]);
            // The honest union: is SOME tip height in [lo, hi] covering p?
            let mut covered = false;
            for i in 0..=4000 {
                let tip = lo + (hi - lo) * i as f64 / 4000.0;
                let h = p[2] - tip;
                if h >= 0.0 && h <= 10.0 && rho <= sil.radius_at(h) {
                    covered = true;
                    break;
                }
            }
            let claimed = stamp.d_swept(p[0], p[1], p[2]) < 0.0;
            // The dense scan can miss a sliver thinner than its own step, so a
            // disagreement only counts away from the boundary.
            let d = stamp.d_swept(p[0], p[1], p[2]).abs();
            if d > 1e-3 {
                assert_eq!(claimed, covered, "lift membership at {p:?} (d={d})");
            }
        }
    }

    /// The shaped stamp against a dense union of instances, on the two move
    /// classes that carry a sweep: the horizontal segment (exact by the
    /// composition) and the slope (exact membership by the per-band quadratic,
    /// magnitude by the certified sampled bound — so the claim may only
    /// UNDER-state, never over-state, on either side).
    #[test]
    fn the_shaped_stamp_never_overclaims_against_dense_instances() {
        let sil = cone_tool();
        let cases: Vec<(PathK, [f64; 3], [f64; 3])> = vec![
            (
                PathK::HSeg { ax: 0.0, ay: 0.0, bx: 12.0, by: 4.0, z: -2.0 },
                [0.0, 0.0, -2.0],
                [12.0, 4.0, -2.0],
            ),
            (
                PathK::Slope { a: [0.0, 0.0, 6.0], b: [9.0, 3.0, -6.0] },
                [0.0, 0.0, 6.0],
                [9.0, 3.0, -6.0],
            ),
        ];
        for (path, a, b) in cases {
            let mut stamps = Vec::new();
            push_stamp(&mut stamps, 0, sil.clone(), path);
            let stamp = &stamps[0];
            let dense = |p: [f64; 3]| -> f64 {
                let mut d = f64::INFINITY;
                for i in 0..=20_000 {
                    let t = i as f64 / 20_000.0;
                    let q = [
                        a[0] + t * (b[0] - a[0]),
                        a[1] + t * (b[1] - a[1]),
                        a[2] + t * (b[2] - a[2]),
                    ];
                    let rho = dist2d(p[0], p[1], q[0], q[1]);
                    d = d.min(meridian_d(
                        &[Pt { x: 0.0, y: 0.0 }, Pt { x: 5.0, y: 5.0 }, Pt { x: 5.0, y: 10.0 }],
                        0.0,
                        rho,
                        p[2] - q[2],
                    ));
                }
                d
            };
            let mut state = 31u64;
            for _ in 0..250 {
                let p = [
                    (lcg(&mut state) - 0.5) * 30.0 + 5.0,
                    (lcg(&mut state) - 0.5) * 24.0 + 2.0,
                    (lcg(&mut state) - 0.5) * 34.0,
                ];
                let claimed = stamp.d_swept(p[0], p[1], p[2]);
                let truth = dense(p);
                if truth > 1e-6 {
                    assert!(claimed > 0.0, "sign flip outside at {p:?}: {claimed} vs {truth}");
                    assert!(claimed <= truth + 1e-6, "outside over-claim at {p:?}: {claimed} vs {truth}");
                } else if truth < -1e-6 {
                    assert!(claimed < 0.0, "sign flip inside at {p:?}: {claimed} vs {truth}");
                    assert!(claimed >= truth - 1e-6, "inside depth over-claim at {p:?}: {claimed} vs {truth}");
                }
            }
        }
    }

    /// A rod with a declared flute_len (K2 slice 2, fork 5A), as the ladder
    /// the parser would build: cutting radius all the way — the truncation is
    /// in HEIGHT, not width.
    fn flute_rod(r: f64, ball: bool, fl: f64) -> Silhouette {
        let body = Some(crate::parse::BodyLadder {
            shank: Some(crate::parse::ShankRung { flute_top: fl, shank_r: r, neck: None }),
            holder: None,
        });
        if ball {
            Silhouette::Ball { r, body }
        } else {
            Silhouette::Flat { r, body }
        }
    }

    /// **Fork 5's stamp, pinned in closed form**: below the flute top the
    /// boundary is the rod's own; at the cap it ends, and above it the stamp
    /// claims nothing — the column the field voice now polices. The ball's
    /// cap is measured from its TIP like every silhouette height, so with
    /// flutes no longer than the ball itself the spherical piece is its lower
    /// half and the old full-sphere min would over-claim steel as void.
    #[test]
    fn a_declared_flute_len_truncates_the_rod_stamp() {
        // Flat r 5, flutes 10: a capped cylinder, every distance exact.
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, flute_rod(5.0, false, 10.0), PathK::Point { x: 0.0, y: 0.0, z: 0.0, lift: 0.0 });
        let s = &stamps[0];
        for i in 0..=20 {
            let h = 10.0 * i as f64 / 20.0;
            assert!(s.d_swept(5.0, 0.0, h).abs() < 1e-9, "the wall must sit at r 5 up to the cap (h={h})");
        }
        assert!(s.d_swept(0.0, 0.0, 5.0) < 0.0, "inside the flutes");
        assert!((s.d_swept(0.0, 0.0, 10.5) - 0.5).abs() < 1e-9, "the cap ends the claim");
        assert!((s.d_swept(8.0, 0.0, 14.0) - 5.0).abs() < 1e-9, "the cap rim is a true corner (3-4-5)");
        // And the vertical travel lifts the cap by exactly itself.
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, flute_rod(5.0, false, 10.0), PathK::Point { x: 0.0, y: 0.0, z: 0.0, lift: 4.0 });
        let s = &stamps[0];
        assert!(s.d_swept(0.0, 0.0, 13.5) < 0.0);
        assert!((s.d_swept(0.0, 0.0, 14.5) - 0.5).abs() < 1e-9);

        // Ball r 3, flutes 4: cap 1.0 above the emitted CENTER (tip + 4).
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, flute_rod(3.0, true, 4.0), PathK::Point { x: 0.0, y: 0.0, z: 0.0, lift: 0.0 });
        let s = &stamps[0];
        assert!(s.d_swept(0.0, 0.0, -3.0).abs() < 1e-9, "the ball tip is untouched");
        assert!(s.d_swept(2.99, 0.0, 0.99) < 0.0, "the cylinder band below the cap");
        assert!((s.d_swept(0.0, 0.0, 1.5) - 0.5).abs() < 1e-9, "the cap ends the claim above the center");

        // Flutes exactly the ball radius: the solid is the lower half-ball,
        // and a point inside the SPHERE but above the cap is steel, not void.
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, flute_rod(3.0, true, 3.0), PathK::Point { x: 0.0, y: 0.0, z: 0.0, lift: 0.0 });
        let s = &stamps[0];
        assert!(s.d_swept(2.0, 0.0, -0.5) < 0.0, "the lower half cuts");
        assert!((s.d_swept(2.0, 0.0, 0.5) - 0.5).abs() < 1e-9, "the upper half does not (distance to the cap disc)");
    }

    /// The truncated rod against a dense union of its capped instances, on
    /// every path class that sweeps — membership is the safety property (a
    /// sign flip is a gouge claim in one direction and phantom stock in the
    /// other), so it is the thing brute-forced, both rod shapes.
    #[test]
    fn a_truncated_rod_never_disagrees_with_the_dense_union() {
        let cases: Vec<(PathK, [f64; 3], [f64; 3])> = vec![
            (PathK::Slope { a: [0.0, 0.0, 5.0], b: [9.0, 3.0, -6.0] }, [0.0, 0.0, 5.0], [9.0, 3.0, -6.0]),
            (PathK::Slope { a: [0.0, 0.0, -6.0], b: [7.0, -2.0, 4.0] }, [0.0, 0.0, -6.0], [7.0, -2.0, 4.0]),
            (PathK::HSeg { ax: 0.0, ay: 0.0, bx: 10.0, by: 4.0, z: -2.0 }, [0.0, 0.0, -2.0], [10.0, 4.0, -2.0]),
            (PathK::Point { x: 2.0, y: 1.0, z: -4.0, lift: 7.0 }, [2.0, 1.0, -4.0], [2.0, 1.0, 3.0]),
        ];
        for ball in [false, true] {
            let (r, fl) = if ball { (3.0, 4.0) } else { (4.0, 6.0) };
            let sil = flute_rod(r, ball, fl);
            for (path, a, b) in &cases {
                let mut stamps = Vec::new();
                push_stamp(&mut stamps, 0, sil.clone(), path.clone());
                let stamp = &stamps[0];
                // One capped instance's membership, stated independently of
                // the stamp's own composition: the lower half-ball, then the
                // cylinder band from the center (the tip, for a flat) up to
                // the flute cap.
                let covered = |p: [f64; 3]| -> bool {
                    for i in 0..=4000 {
                        let t = i as f64 / 4000.0;
                        let q = [
                            a[0] + t * (b[0] - a[0]),
                            a[1] + t * (b[1] - a[1]),
                            a[2] + t * (b[2] - a[2]),
                        ];
                        let rho = dist2d(p[0], p[1], q[0], q[1]);
                        let hc = p[2] - q[2];
                        let hit = if ball {
                            let d3 = ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt();
                            (d3 <= r && hc <= 0.0) || (rho <= r && (0.0..=fl - r).contains(&hc))
                        } else {
                            rho <= r && (0.0..=fl).contains(&hc)
                        };
                        if hit {
                            return true;
                        }
                    }
                    false
                };
                let mut state = 11u64;
                for _ in 0..300 {
                    let p = [
                        (lcg(&mut state) - 0.5) * 28.0 + 4.0,
                        (lcg(&mut state) - 0.5) * 20.0,
                        (lcg(&mut state) - 0.5) * 30.0,
                    ];
                    let d = stamp.d_swept(p[0], p[1], p[2]);
                    // The dense scan can miss a sliver thinner than its own
                    // step, so near-boundary disagreements are not check results.
                    if d.abs() > 1e-3 {
                        assert_eq!(
                            d < 0.0,
                            covered(p),
                            "membership at {p:?} (ball={ball}, path={path:?}, d={d})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_stamp_boundary_is_the_silhouette_the_lint_samples() {
        for sil in [Silhouette::Ball { r: 3.0, body: None }, Silhouette::Flat { r: 3.0, body: None }] {
            let (cx, cy, cz) = (2.0, -1.0, 7.0);
            let mut stamps = Vec::new();
            push_stamp(&mut stamps, 0, sil.clone(), PathK::Point { x: cx, y: cy, z: cz, lift: 0.0 });
            let stamp = &stamps[0];
            let tip = cz - sil.drop();
            for i in 0..=40 {
                let h = 8.0 * i as f64 / 40.0;
                let rb = sil.radius_at(h);
                let d = stamp.d_swept(cx + rb, cy, tip + h);
                assert!(
                    d.abs() < 1e-9,
                    "{sil:?}: the body at h={h} reaches r={rb}, but the stamp reads {d} there"
                );
            }
        }
    }

    #[test]
    fn rod_d_is_the_exact_prism_distance() {
        // Inside: nearer of wall and floor. Outside below the rim: the hypot.
        assert_eq!(rod_d(-3.0, -1.0), -1.0);
        assert_eq!(rod_d(-1.0, -3.0), -1.0);
        assert_eq!(rod_d(2.0, -1.0), 2.0);
        assert_eq!(rod_d(-1.0, 2.0), 2.0);
        assert!((rod_d(3.0, 4.0) - 5.0).abs() < 1e-12);
    }

    #[test]
    fn arc_distance_matches_dense_sampling() {
        // A three-quarter CCW arc; the exact formula against 100k chords.
        let (cx, cy, ra) = (10.0, -4.0, 7.0);
        let a0 = 0.3;
        let sweep = 1.5 * std::f64::consts::PI;
        let mut state = 7u64;
        for _ in 0..200 {
            let px = cx + (lcg(&mut state) - 0.5) * 40.0;
            let py = cy + (lcg(&mut state) - 0.5) * 40.0;
            let exact = dist2d_arc(px, py, cx, cy, ra, a0, sweep);
            let mut dense = f64::INFINITY;
            for i in 0..=100_000 {
                let a = a0 + sweep * i as f64 / 100_000.0;
                dense = dense.min(dist2d(px, py, cx + ra * a.pcos(), cy + ra * a.psin()));
            }
            assert!(
                (exact - dense).abs() < 1e-6,
                "arc distance {exact} vs dense {dense} at ({px}, {py})"
            );
        }
    }

    /// The design entry's exactness claim, checked from outside the formula:
    /// the horizontal ball stamp equals a dense min over instances.
    #[test]
    fn horizontal_ball_stamp_matches_dense_instances() {
        let sil = Silhouette::Ball { r: 3.0, body: None };
        let path = PathK::HSeg { ax: 0.0, ay: 0.0, bx: 20.0, by: 5.0, z: 4.0 };
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, sil.clone(), path);
        let stamp = &stamps[0];
        let mut state = 11u64;
        for _ in 0..300 {
            let p = [
                (lcg(&mut state) - 0.5) * 40.0 + 10.0,
                (lcg(&mut state) - 0.5) * 30.0,
                (lcg(&mut state) - 0.5) * 24.0 + 4.0,
            ];
            let claimed = stamp.d_swept(p[0], p[1], p[2]);
            let mut dense = f64::INFINITY;
            for i in 0..=20_000 {
                let t = i as f64 / 20_000.0;
                let q = [t * 20.0, t * 5.0, 4.0];
                let plan = dist2d(p[0], p[1], q[0], q[1]) - 3.0;
                let inst = rod_d(plan, q[2] - p[2]).min(dist3d_seg(p, q, q) - 3.0);
                dense = dense.min(inst);
            }
            assert!(
                (claimed - dense).abs() < 1e-3,
                "horizontal ball stamp {claimed} vs dense {dense} at {p:?}"
            );
        }
    }

    /// The safety pin on the sloped bound: the claim never over-states on
    /// either side — outside it stays at or under the dense truth, inside its
    /// claimed depth stays at or under the dense depth, and the sign always
    /// matches the exact membership. A steep move with a real heel shadow.
    #[test]
    fn sloped_stamp_bound_never_overclaims() {
        for sil in [Silhouette::Flat { r: 3.0, body: None }, Silhouette::Ball { r: 3.0, body: None }] {
            let a = [0.0, 0.0, 12.0];
            let b = [6.0, 2.0, 0.0]; // steeper than 60 degrees: the bad case
            let mut stamps = Vec::new();
            push_stamp(&mut stamps, 0, sil.clone(), PathK::Slope { a, b });
            let stamp = &stamps[0];
            let r = 3.0;
            let dense_d = |p: [f64; 3]| -> f64 {
                let mut d = f64::INFINITY;
                for i in 0..=50_000 {
                    let t = i as f64 / 50_000.0;
                    let q = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])];
                    let plan = dist2d(p[0], p[1], q[0], q[1]) - r;
                    let mut inst = rod_d(plan, q[2] - p[2]);
                    if matches!(sil, Silhouette::Ball { .. }) {
                        inst = inst.min(dist3d_seg(p, q, q) - r);
                    }
                    d = d.min(inst);
                }
                d
            };
            let mut state = 23u64;
            for _ in 0..250 {
                let p = [
                    (lcg(&mut state) - 0.5) * 24.0 + 3.0,
                    (lcg(&mut state) - 0.5) * 16.0 + 1.0,
                    (lcg(&mut state) - 0.5) * 30.0 + 6.0,
                ];
                let claimed = stamp.d_swept(p[0], p[1], p[2]);
                let dense = dense_d(p);
                if dense > 1e-6 {
                    // Truly outside: the claim must not over-state, and must
                    // agree on the sign.
                    assert!(claimed > 0.0, "sign flip outside at {p:?}: claimed {claimed}, dense {dense}");
                    assert!(
                        claimed <= dense + 1e-6,
                        "outside over-claim at {p:?}: claimed {claimed}, dense {dense} ({sil:?})"
                    );
                } else if dense < -1e-6 {
                    assert!(claimed < 0.0, "sign flip inside at {p:?}: claimed {claimed}, dense {dense}");
                    assert!(
                        claimed >= dense - 1e-6,
                        "inside depth over-claim at {p:?}: claimed {claimed}, dense {dense} ({sil:?})"
                    );
                }
            }
        }
    }

    /// The ellipsoid-reduces-to-sphere mold: a zero-length ball move is the
    /// exact sphere-plus-rod, checked against closed forms.
    #[test]
    fn point_ball_stamp_is_the_exact_ball_nose() {
        let mut stamps = Vec::new();
        push_stamp(&mut stamps, 0, Silhouette::Ball { r: 5.0, body: None }, PathK::Point { x: 0.0, y: 0.0, z: 10.0, lift: 0.0 });
        let s = &stamps[0];
        // Straight below the tip: sphere bottom at z = 5, so d = 5 - z.
        assert!((s.d_swept(0.0, 0.0, 3.0) - 2.0).abs() < 1e-12);
        // On the axis inside the ball: depth to the sphere's own boundary.
        assert!((s.d_swept(0.0, 0.0, 10.0) - (-5.0)).abs() < 1e-12);
        // Level with the center, outside: cylinder wall distance.
        assert!((s.d_swept(9.0, 0.0, 10.0) - 4.0).abs() < 1e-12);
        // High above: the semi-infinite rod, not the sphere.
        assert!((s.d_swept(9.0, 0.0, 300.0) - 4.0).abs() < 1e-12);
    }
}
