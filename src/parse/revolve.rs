use crate::math::Portable as _;
// Revolved features: the offset meridian, the emitter tail, datums and rims, boss loops, the clear stacks.

use super::*;

// ---- the standing surfaces of revolution, as ONE offset meridian -----------
//
// Ten features declare a surface of revolution that STANDS on its own base
// plane, as against the cavities that hang from the stock top. Every one is
// machined from a meridian evaluated at a normal offset by its own
// `*_cl_point` home, and `stock::removals_for` walked all ten inline because
// it was the only consumer. It is not any more: the revolve-boss `Clear` needs
// the same curve to know what island it must stand off (DECISIONS.md →
// "roughing a convex revolve…", slice 0). So the walk lives here once, beside
// the homes it calls, and both consumers read it rather than restating it —
// which is what makes "the island the clear stands off and the wall the finish
// leaves are one curve" a fact about the code and not a hope.

/// The degeneracy tolerance for meridian work: how close to zero a span, a
/// radius or a monotonicity slip has to be before it counts as none. This is
/// NOT `stock::EPS` (which decides contact-versus-material for a tool body) and
/// NOT `CHORD_TOL_MM` (a curve approximation budget) — it happens to share the
/// former's magnitude because both are "this number is indistinguishable from
/// zero at machine scale", and it is written separately so changing one reason
/// never silently moves the other.
pub(crate) const REVOLVE_EPS: f64 = 1e-6;

/// How finely a meridian is sampled before it becomes a polyline. The chord
/// error falls off as 1/N², so at this count it is orders below the lattice
/// spacing the foul probe already samples the tool body at — this is not the
/// binding approximation, and pretending otherwise by making it adaptive would
/// add machinery no consumer can feel.
pub(crate) const MERIDIAN_SAMPLES: usize = 256;

pub(crate) fn sample_meridian(t0: f64, t1: f64, point: impl Fn(f64) -> (f64, f64)) -> Vec<(f64, f64)> {
    let samples: Vec<(f64, f64)> = (0..=MERIDIAN_SAMPLES)
        .map(|i| point(t0 + (t1 - t0) * i as f64 / MERIDIAN_SAMPLES as f64))
        .collect();
    ascending(samples)
}

/// Normalize a sampled meridian to rho-ascending. Every surface here is
/// non-undercut by construction, so rho is monotone along the meridian and this
/// is just a direction fix — except on the vase, whose meridian arrives as two
/// arcs, which is why it sorts rather than reverses.
pub(crate) fn ascending(mut samples: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    samples.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
    samples
}

/// A standing revolve's OFFSET meridian: `(rho, z)` in absolute coordinates,
/// rho-ascending, at a normal offset of `skin` (0 for the nominal surface).
pub(crate) struct RevolveMeridian {
    pub samples: Vec<(f64, f64)>,
    /// False for the BANDS — a catenoid neck, a pseudosphere band, a vase and
    /// a torus all stop at a top circle and declare nothing inside it.
    pub closes_on_axis: bool,
}

/// The offset meridian of a standing surface of revolution, or `None` for a
/// feature that is not one (and for the vase whose allowance eats its neck arc
/// whole, which has no offset surface to report).
pub(crate) fn derive_revolve_offset_meridian(feature: &Feature, skin: f64) -> Option<RevolveMeridian> {
    let closed = |samples| Some(RevolveMeridian { samples, closes_on_axis: true });
    let band = |samples| Some(RevolveMeridian { samples, closes_on_axis: false });
    match feature {
        // A cylinder boss: the vertical line at `R + skin` from its top to its
        // base — two samples, equal rho, so `ascending`'s stable sort keeps the
        // top first and the stack's crown lands at index 0, where a flat top
        // belongs (Phase 10 lane A). Closes on the axis: the top is a solid
        // disc, the part stands under all of it.
        Feature::CylinderBoss { dia, extent, z, .. } => {
            let r = dia / 2.0 + skin;
            closed(vec![(r, z + extent), (r, *z)])
        }
        // The sphere's offset is a concentric sphere. `removals_for` reports it
        // EXACTLY, as `Floor::Dome`, rather than as this polyline — so this arm
        // has one consumer (the clear's island) and the two cannot drift,
        // because both are the same `dia / 2 + skin`.
        Feature::Sphere { dia, z, .. } => {
            let r = dia / 2.0 + skin;
            closed(sample_meridian(0.0, std::f64::consts::FRAC_PI_2, |t| {
                (r * t.psin(), z + r * t.pcos())
            }))
        }
        Feature::Cone { dia, height, z, .. } => {
            let (a, h) = (dia / 2.0, *height);
            // The one meridian here that is a straight line, so its offset is
            // the parallel line one `skin` out along the constant normal
            // (h, a)/l — no CL home to borrow, and none worth inventing for a
            // formula this size.
            let l = (a * a + h * h).sqrt();
            closed(sample_meridian(0.0, a, |rho| {
                (rho + skin * h / l, z + h * (1.0 - rho / a) + skin * a / l)
            }))
        }
        Feature::Torus { major_dia, minor_dia, z, .. } => {
            let (r_major, r_tube) = (major_dia / 2.0, minor_dia / 2.0);
            // The offset of a torus is a torus (R1 self-offsetting): same
            // centre circle, fatter tube. phi runs outer equator -> crown ->
            // inner equator, which is the machined upper half.
            let r_off = r_tube + skin;
            band(sample_meridian(0.0, std::f64::consts::PI, |phi| {
                (r_major + r_off * phi.pcos(), z + r_off * phi.psin())
            }))
        }
        Feature::SplineRevolve { meridian, z, .. } => {
            let (u0, u1) = meridian.domain();
            // Apex-first envelope: whether it actually reaches the axis is the
            // authored curve's business, so ask it rather than assume.
            let closes = meridian.eval(u0).0 <= REVOLVE_EPS;
            let samples = sample_meridian(u0, u1, |u| {
                let (rho, z_rel) = spline_revolve_cl_point(meridian, skin, u);
                (rho, z + z_rel)
            });
            Some(RevolveMeridian { samples, closes_on_axis: closes })
        }
        _ => None,
    }
}

/// How finely an emitter's rim TAIL is sampled. Deliberately not
/// [`MERIDIAN_SAMPLES`]: the tail is stationed by its own parameter (arc angle
/// for the ogive's roll, line length for the cone's), and the piece it covers
/// is short in rho while carrying the whole below-base claim — the secant
/// ogive's roll spans 0.092mm of rho against a meridian running 0 → 18. A
/// consumer that stations by rho would resolve the tail at two or three
/// stations if it inherited the meridian's spacing over the meridian's span,
/// so the tail brings its own density rather than borrowing one.
pub(crate) const TAIL_SAMPLES: usize = 64;

/// **The profile sphere finish's contact, in one home** (Phase 10 lane B, slice
/// 1, fork 5A). `finish_sphere_profile` rides a custom profile's VERTICES round
/// a sphere — vertex k is the active contact for every polar angle strictly
/// between the tangent angles of the segments bracketing it — and emits one
/// ring per equal-Z step inside each bracket. The stock model's tube for that
/// op is the union of the cutting form over exactly those rings, so the ring
/// list lives here and both read it: pinning the tube against the emitted
/// positions and against this home is the same pin (the `Floor::Meridian`
/// law — the wall the model reports and the wall the machine leaves are one
/// curve). The emitter's validation stays with the emitter; these three take
/// a profile it has already accepted (on-axis tip, monotone flare,
/// outward-convex, reaching 90°).
///
/// The tangent angle of each segment `profile[i] -> profile[i+1]`.
pub fn profile_segment_angles(profile: &[Pt]) -> Vec<f64> {
    profile.windows(2).map(|w| (w[1].y - w[0].y).patan2(w[1].x - w[0].x)).collect()
}

/// Bracket boundaries as `(phi_start, active vertex)`: vertex 0 (the tip) is
/// active on `[0, psi_0)`, vertex k+1 on `[psi_k, psi_{k+1})`, the last
/// extended to π/2 by the consumer. Entries beyond π/2 are left in rather
/// than filtered — a consumer's `phi_end <= phi_start` check discards each as
/// a zero-width bracket, so a filter here would be the same guard twice.
pub fn sphere_profile_breaks(profile: &[Pt]) -> Vec<(f64, usize)> {
    let mut breaks: Vec<(f64, usize)> = vec![(0.0, 0)];
    for (k, &psi) in profile_segment_angles(profile).iter().enumerate() {
        breaks.push((psi.min(std::f64::consts::FRAC_PI_2), k + 1));
    }
    breaks
}

/// The rings, as `(tip z, tip rho)` in emission order: pole to equator, one
/// bracket at a time, equal-Z steps of `stepover` within each bracket (the
/// waterline convention), each ring's tip at the part contact shifted by the
/// active vertex's own offset — `rho = rho_p + r_v`, `z = z_p - z_v`. rho is
/// recovered as `sqrt(R² - dz²)` directly rather than through an `acos`.
pub fn sphere_profile_passes(profile: &[Pt], r_part: f64, cz: f64, stepover: f64) -> Vec<(f64, f64)> {
    let breaks = sphere_profile_breaks(profile);
    let mut passes: Vec<(f64, f64)> = Vec::new();
    for (i, &(phi_start, vidx)) in breaks.iter().enumerate() {
        let phi_end = breaks
            .get(i + 1)
            .map(|&(p, _)| p)
            .unwrap_or(std::f64::consts::FRAC_PI_2)
            .min(std::f64::consts::FRAC_PI_2);
        if phi_end <= phi_start + 1e-12 {
            continue;
        }
        let (r_v, z_v) = (profile[vidx].x, profile[vidx].y);
        let z_c = |phi: f64| cz + r_part * phi.pcos() - z_v;
        let (z_start, z_end) = (z_c(phi_start), z_c(phi_end));

        let n = ((z_start - z_end).abs() / stepover).ceil().max(1.0) as usize;
        for step in 0..=n {
            let target_z = z_start + (z_end - z_start) * (step as f64 / n as f64);
            let dz = target_z - cz + z_v;
            let rho_c = (r_part * r_part - dz * dz).max(0.0).sqrt() + r_v;
            passes.push((target_z, rho_c));
        }
    }
    passes
}

/// The run-out a ball-nose emitter walks PAST the offset meridian's natural
/// end at the base rim: `(rho, z)` in absolute coordinates, rho-ascending,
/// continuing from [`derive_revolve_offset_meridian`]'s last sample at the same
/// `offset` (which is the CL stand-off, `skin + r_ball`, not the surface skin).
/// The first point of the run-out is the meridian's own last sample, so it is
/// omitted and these append cleanly.
///
/// **Empty for everything that has no tail**, which is most of the family: the
/// seven carriers slice 1 of the rim-area thread measured ending at the
/// meridian's natural end (DECISIONS.md → "the tube replaces the collar…"), and
/// the TANGENT ogive, whose rim normal is horizontal — `finish_ogive`'s rim
/// piece is empty by construction, and its extreme CL is already `a + r_ball`.
/// Emptiness is the honest answer AND the gate: a consumer asks this rather
/// than testing the enum variant, so no carrier can acquire a rim collar by
/// being the wrong shape of feature (DECISIONS.md → "the emitter rim tails…",
/// forks 1B and 3).
///
/// Both tails are the SAME shape — a roll about the base-rim corner — and both
/// are true offsets. That is fork 2B's payoff: the cone's arm used to run its
/// offset LINE past the normal foot to the base plane, a locus that is not an
/// offset of anything, and it only did so because `finish_cone`'s ladder
/// clamped its last level and priced it on the flank branch. With the emitter
/// rolling its corner (DECISIONS.md → "the cone rolls its rim…"), the two arms
/// differ only in where the rim normal points.
pub fn derive_revolve_emitter_tail(feature: &Feature, offset: f64) -> Vec<(f64, f64)> {
    if offset <= REVOLVE_EPS {
        // At zero stand-off the CL locus IS the surface, which ends at the rim
        // by definition. Both tails collapse to their own start point.
        return Vec::new();
    }
    match feature {
        Feature::Cone { dia, height, z, .. } => {
            let (a, h) = (dia / 2.0, *height);
            if (a * a + h * h).sqrt() <= REVOLVE_EPS || h <= REVOLVE_EPS {
                return Vec::new();
            }
            // The flank's outward normal is (cosα, sinα), so the rim-normal
            // seat sits α above the horizontal — the roll spans exactly the
            // cone's own half-angle.
            rim_corner_roll(a, *z, offset, a.patan2(h))
        }
        _ => Vec::new(),
    }
}

/// The CL arc a ball traces rolling on the sharp base-rim corner `(a, z0)`:
/// radius `offset` about that corner, from the rim-normal seat at `psi_rim`
/// down to the horizontal, rho-ascending, the seat itself omitted so it
/// appends to a meridian that already ends there. Ending horizontal is what
/// makes the last sample `(a + offset, z0)` on every carrier — a roll finishes
/// when the ball leaves the corner, and there is nothing outboard of the rim to
/// hold it higher.
///
/// Empty at a horizontal rim normal: there is no corner to roll, and the
/// meridian's natural end is already the emitter's.
pub(super) fn rim_corner_roll(a: f64, z0: f64, offset: f64, psi_rim: f64) -> Vec<(f64, f64)> {
    if psi_rim <= REVOLVE_EPS {
        return Vec::new();
    }
    (1..=TAIL_SAMPLES)
        .map(|i| {
            let psi = psi_rim * (1.0 - i as f64 / TAIL_SAMPLES as f64);
            (a + offset * psi.pcos(), z0 + offset * psi.psin())
        })
        .collect()
}

/// A standing revolve's axis and base plane: `(x, y, z)`, where `z` is the
/// datum its own declaration is built from and the floor a roughing pass cuts
/// to. A named home rather than a field read, so the clear and the removal
/// model cannot disagree about which datum "the bottom" means — and so that
/// asking "is this a standing revolve?" has exactly one answer.
pub(crate) fn derive_revolve_datum(feature: &Feature) -> Option<(f64, f64, f64)> {
    match feature {
        Feature::Sphere { x, y, z, .. }
        | Feature::Cone { x, y, z, .. }
        | Feature::Torus { x, y, z, .. }
        | Feature::SplineRevolve { x, y, z, .. } => Some((*x, *y, *z)),
        // The simplest standing revolve of all, and the last to join (Phase 10
        // lane A, `DECISIONS.md` → "Phase 10 lane A — the boss finish's
        // removal rejects ten shipped fixtures…", fork 2A): a cylinder's
        // meridian is a vertical line, single-valued in z by construction, so
        // the moat `Clear`, its level stack, its removal and every consumer of
        // them come for free. The FINISH stays the prismatic emitter it is —
        // `removals_for`'s revolve finish arms match by kind, not by datum.
        Feature::CylinderBoss { x, y, z, .. } => Some((*x, *y, *z)),
        _ => None,
    }
}

/// The widest the declared surface gets — its own rim, at a `skin` normal
/// offset. The meridian is rho-ascending, so this is its last sample.
pub(crate) fn derive_revolve_rim_radius(feature: &Feature, skin: f64) -> Option<f64> {
    Some(derive_revolve_offset_meridian(feature, skin)?.samples.last()?.0)
}

/// How far beyond the rim (plus allowance) a moat `Clear` sweeps, in tool
/// diameters — read by the emitter and the removal alike, so the two cannot
/// disagree about where the moat ends.
///
/// The revolve family's law is ONE diameter: "the width a tool must sweep to
/// reach an edge at all" (`rim_collar`'s own sentence), because a revolve's
/// finish rings sit on the surface and enter from above. **A cylinder boss's
/// finish does not**: `boss_engine` plunges its lead-in one tool diameter
/// OUTSIDE the tool-centre circle ("in cleared air") and feeds in, so the lead
/// body stands at `rim + 2·dia` — a one-diameter moat leaves it in standing
/// stock and `check_moves` rejects the program the moat was cut for (Phase 10
/// lane A's measurement, at (90, 50, −5) on `cylinder_boss.cam`). The moat is
/// therefore what the next op needs to ENTER, stated per kind here rather than
/// guessed at the emitter: two diameters for the prismatic boss — its own on
/// the centre circle, and the one its lead stands out.
pub(crate) fn derive_moat_width(p: &Part, op: &Operation, feature: &Feature, tool_dia: f64) -> f64 {
    match feature {
        Feature::CylinderBoss { .. }
        | Feature::ProfileBoss { .. }
        | Feature::ContourBoss { .. } => {
            // "What the next op needs to enter" means every later op that
            // finishes this boss, and the widest of them sets the moat: a
            // corner rounder's 12mm body leads in one of ITS diameters past
            // its path, which a moat cut for a 10mm endmill's lead left
            // 0.3mm short (`edge_round_profile.cam`, rejected at (54.808,
            // 16.746) — lane A slice 1's measurement). The Clear's own tool
            // is the floor of the max, so a boss nobody finishes still gets
            // the two diameters its own cutter swept.
            let op_idx = p.operations.iter().position(|o| std::ptr::eq(o, op)).unwrap_or(0);
            let widest = p.operations[op_idx..]
                .iter()
                .filter(|o| o.feature_name == op.feature_name)
                .filter_map(|o| p.tools.get(&o.tool_num).map(|t| t.dia))
                .fold(tool_dia, f64::max);
            2.0 * widest
        }
        _ => tool_dia,
    }
}

/// The skin a pocket's `Clear` leaves on the FLOOR (Phase 27, fork 6A —
/// `DECISIONS.md` → "Phase 27 opens: the region DRAFT…"). `stock_to_leave`
/// stays ONE number: it is the radial allowance it always was, and it is left
/// on the floor as well exactly when a later `Floor` on the same pocket is
/// there to take it. No consumer, no floor skin — the floor goes to size, which
/// is what every program written before this rule still does. Derived from the
/// `run` list, like the engagement linter's consumer and `derive_moat_width`:
/// an allowance is a promise to a later op, and this reads whether one exists.
pub fn derive_floor_skin(p: &Part, op: &Operation) -> f64 {
    let Some(skin) = op.stock_to_leave.filter(|s| *s > 0.0) else { return 0.0 };
    if op.op_type != OpType::Clear || !matches!(p.features.get(&op.feature_name), Some(Feature::ProfilePocket { .. } | Feature::Plane { .. } | Feature::Face { .. })) {
        return 0.0;
    }
    let idx = p.operations.iter().position(|o| std::ptr::eq(o, op)).unwrap_or(usize::MAX);
    let taken = p
        .operations
        .iter()
        .enumerate()
        .any(|(i, o)| i > idx && o.op_type == OpType::End && o.feature_name == op.feature_name);
    if taken { skin } else { 0.0 }
}

/// What the roughs before a pocket's `Floor` left on its floor — the most any
/// of them did, since each cut every loop at its own one height. The floor
/// pass descends to this, takes this, and REMOVES only this: read by the
/// emitter and the stock model alike, so a floor pass scheduled over a pocket
/// nobody cleared is a plunge into material that outlives it, and refuses.
pub fn derive_floor_left(p: &Part, op: &Operation) -> f64 {
    let idx = p.operations.iter().position(|o| std::ptr::eq(o, op)).unwrap_or(0);
    p.operations[..idx]
        .iter()
        .filter(|o| o.feature_name == op.feature_name)
        .map(|o| derive_floor_skin(p, o))
        .fold(0.0, f64::max)
}

/// The tool-centre loops a flat endmill rides `d` outside a prismatic boss's
/// wall — the ONE home the boss finish, the moat `Clear` and both their
/// removals read (Phase 10 lane A, slice 1; `DECISIONS.md` → "Phase 10 lane
/// A, slice 1…"). An ellipse offsets along its normals through the emitter's
/// own `ellipse_offset_pts`; a polygon through the offsetter, normalised CCW
/// and flattened ALONG its fillet arcs (`offset::flatten_loop`), never across
/// them. CCW; an emitter that climbs CW reverses. `None` for any other kind.
pub fn prismatic_boss_cl_loops(feature: &Feature, d: f64) -> Option<Vec<Vec<Pt>>> {
    match feature {
        Feature::ProfileBoss { points, .. } => {
            let ccw = crate::ops::clear::make_ccw(points);
            let loops = crate::offset::offset_closed_polyline_verts(&ccw, -d);
            if loops.is_empty() {
                return None;
            }
            Some(loops.iter().map(|l| crate::offset::flatten_loop(l)).collect())
        }
        // The wire boss's plan view IS the planar problem (the wire thread's
        // own sentence); its Z rides `Floor::AlongWire`, never the loop.
        Feature::ContourBoss { points, .. } => {
            let proj: Vec<Pt> = points.iter().map(Pt3::xy).collect();
            let ccw = crate::ops::clear::make_ccw(&proj);
            let loops = crate::offset::offset_closed_polyline_verts(&ccw, -d);
            if loops.is_empty() {
                return None;
            }
            Some(loops.iter().map(|l| crate::offset::flatten_loop(l)).collect())
        }
        _ => None,
    }
}

/// The moat family around a prismatic boss, OUTERMOST FIRST (emission
/// order): the tool-centre loops from `skin + moat_width − r` down to `r +
/// skin`, in equal bites no larger than `stepover` (max-not-min, the revolve
/// stack's law). The innermost is the finish's own circle, so the moat ends
/// exactly where the finish begins.
pub fn prismatic_boss_moat_family(
    feature: &Feature,
    r_tool: f64,
    skin: f64,
    stepover: f64,
    moat_width: f64,
) -> Option<Vec<Vec<Vec<Pt>>>> {
    if stepover <= 0.0 || r_tool <= 0.0 {
        return None;
    }
    let d_in = r_tool + skin;
    let d_out = skin + moat_width - r_tool;
    if d_out < d_in - 1e-9 {
        return None;
    }
    let span = d_out - d_in;
    let n = if span <= 1e-9 { 0 } else { (span / stepover).ceil() as usize };
    let step = if n == 0 { 0.0 } else { span / n as f64 };
    let mut family = Vec::with_capacity(n + 1);
    for k in (0..=n).rev() {
        family.push(prismatic_boss_cl_loops(feature, d_in + step * k as f64)?);
    }
    Some(family)
}

/// A prismatic boss's top and base planes.
pub fn prismatic_boss_z_span(feature: &Feature, stock_top: f64) -> Option<(f64, f64)> {
    match feature {
        Feature::ProfileBoss { z, extent, .. } => {
            let top = derive_contour_top(stock_top, *z);
            Some((top, top - extent))
        }
        // A wire declares a WALL from its lowest point down; the surround
        // above it is stock like any other, so the moat runs from the stock
        // top to the wall's base. (The island's own column above the wire is
        // never cut by the moat, whatever stands there.)
        Feature::ContourBoss { points, extent } => {
            let lowest = points.iter().map(|q| q.z).fold(f64::INFINITY, f64::min);
            Some((stock_top, lowest - extent))
        }
        _ => None,
    }
}

/// A standing revolve's base plane. See [`derive_revolve_datum`].
pub(crate) fn derive_revolve_base_z(feature: &Feature) -> Option<f64> {
    derive_revolve_datum(feature).map(|(_, _, z)| z)
}

/// Linear interpolation over a `(key, value)` table with the ends clamped.
///
/// Duplicate keys are how a vertical STEP is written, and a key landing exactly
/// ON one resolves to the LATER value — the bracket is found with `<=`, not
/// `<`. On a terraced floor that means the riser belongs to the deeper terrace,
/// which is the same "the boundary belongs to the removal" convention
/// `Section::contains` applies in plan: the tool whose edge rides the riser cut
/// to the lower level, so that is the floor there. On a strictly-ascending
/// table — every meridian in this file — `<=` and `<` return the same value at
/// every key, so this is a rule about steps only.
pub(crate) fn interp_table(samples: &[(f64, f64)], key: f64) -> f64 {
    let (first, last) = (samples[0], samples[samples.len() - 1]);
    if key <= first.0 {
        return first.1;
    }
    if key >= last.0 {
        return last.1;
    }
    let i = samples.partition_point(|s| s.0 <= key).max(1).min(samples.len() - 1);
    let (lo, hi) = (samples[i - 1], samples[i]);
    if (hi.0 - lo.0).abs() < f64::EPSILON {
        return hi.1;
    }
    lo.1 + (hi.1 - lo.1) * (key - lo.0) / (hi.0 - lo.0)
}

/// One Z level of a revolve-boss `Clear`'s MOAT: the depth the pass cuts to,
/// and the radius of the island standing there that it must not cut into.
///
/// Above the crown of a stack that has a hole (see [`RevolveClearStacks`]) the
/// island reads ZERO — nothing the part declares stands that high — and what
/// actually stands there is the pillar the hole leaves in the middle. Only a
/// tool sizes a pillar, so it is [`RevolveClearStacks::pillar_r`]'s number and
/// not this one's, and both consumers stand off the wider of the two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClearLevel {
    pub z: f64,
    pub island_r: f64,
}

/// One Z level of the HOLE inside a crowned revolve — a torus's donut hole —
/// where the declared wall bounds the work from OUTSIDE instead of from
/// inside: the depth the pass cuts to, and the radius the part's inner flank
/// stands at there. The work at that level is the annulus between the pillar
/// and this ceiling (DECISIONS.md → "the torus's annulus island…", forks 1–3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoleLevel {
    pub z: f64,
    pub ceiling_r: f64,
}

/// The level stacks a revolve-boss `Clear` cuts: the moat outside the part,
/// and — where the meridian crowns — the hole inside it.
///
/// `hole` is empty for nine of the ten standing revolves, and for those this
/// is exactly the single stack it has always been.
pub struct RevolveClearStacks {
    pub moat: Vec<ClearLevel>,
    pub hole: Vec<HoleLevel>,
}

impl RevolveClearStacks {
    /// The pillar left standing in the middle of the hole: one tool DIAMETER
    /// inward from the narrowest the hole gets, which is the moat's own law
    /// read in the mirror ("stock a tool diameter past the rim is declared by
    /// nothing and stays"). Zero when there is no hole, and zero when the hole
    /// is too narrow to leave one — which is the same degenerate the
    /// above-crown levels already emit, a full disc with no island at all.
    ///
    /// One home because two consumers must agree on it exactly: the emitter
    /// stands its spiral off the pillar, and the removal model must not claim
    /// the pillar's column as air.
    pub fn pillar_r(&self, tool_dia: f64) -> f64 {
        let narrowest = self.hole.iter().map(|l| l.ceiling_r).fold(f64::MAX, f64::min);
        if narrowest == f64::MAX {
            return 0.0;
        }
        (narrowest - tool_dia).max(0.0)
    }
}

/// The level stacks a revolve-boss `Clear` cuts, top-down, with the island
/// radius at each moat level and the ceiling radius at each hole level — the
/// feature plus its `skin` allowance (DECISIONS.md → "roughing a convex
/// revolve…", slice 0, and "the torus's annulus island…", fork 1).
///
/// **The crown splits the meridian, and it is one rule for the whole family.**
/// A meridian sampled rho-ascending has one greatest height. Above it there is
/// no part at any radius; below it the branches on either side are each
/// single-valued in z, so each feeds its own stack. Nine of the ten standing
/// revolves put that crown at their FIRST sample — a cap's apex on the axis, a
/// band's own top circle — so their inner branch is empty, there is no hole,
/// and this reads exactly as it did when the moat was the only stack. The
/// torus is the member whose crown sits in the middle of its meridian, and its
/// inner branch is the donut hole.
///
/// `None` when the feature is not a standing revolve, when a branch is not
/// monotone in z (a meridian that crests more than once puts three radii at
/// some height, and no stack of discs and annuli can state that), or when the
/// stock does not stand above the feature's base plane at all — which is
/// exactly the too-short-stock case the containment lint reports.
///
/// **Equal steps, not a short last one.** `stepdown` is a MAXIMUM: the span is
/// divided into `ceil(span / stepdown)` equal cuts, so every pass takes the
/// same bite and none of them is a sliver. The viable alternative is exact
/// `stepdown` steps with a remainder cut at the bottom, which is what a
/// hand-written program usually does; it is rejected because the remainder pass
/// can be arbitrarily thin, and a spring pass that thin at the FLOOR of the
/// roughing stack is where a rougher chatters. Max-not-min also matches how
/// `stock_to_leave` and `scallop` already read.
pub fn derive_revolve_clear_stacks(
    feature: &Feature,
    stock_top: f64,
    stepdown: f64,
    skin: f64,
) -> Option<RevolveClearStacks> {
    if stepdown <= 0.0 {
        return None;
    }
    let base_z = derive_revolve_base_z(feature)?;
    let span = stock_top - base_z;
    if span <= REVOLVE_EPS {
        return None;
    }
    let mer = derive_revolve_offset_meridian(feature, skin)?;

    // The crown, taken at the FIRST sample reaching the greatest height: a
    // flat top then belongs to the outer branch whole, rather than being split
    // down its middle by a tie.
    let mut crown_i = 0;
    for (i, s) in mer.samples.iter().enumerate() {
        if s.1 > mer.samples[crown_i].1 + REVOLVE_EPS {
            crown_i = i;
        }
    }
    let crown_z = mer.samples[crown_i].1;
    let (inner, outer) = (&mer.samples[..=crown_i], &mer.samples[crown_i..]);
    // Each branch, in its own direction. A branch that turns back is a second
    // crest, which this shape cannot state — the error the torus used to
    // take, now narrowed to the meridians that really are unsayable.
    if outer.windows(2).any(|w| w[1].1 > w[0].1 + REVOLVE_EPS)
        || inner.windows(2).any(|w| w[1].1 < w[0].1 - REVOLVE_EPS)
    {
        return None;
    }

    // rho-ascending in, z-ascending out. On the outer branch z falls as rho
    // climbs, so the reversal is the whole inversion; the inner branch already
    // rises with rho and is inverted in place.
    let mut moat_table: Vec<(f64, f64)> = outer.iter().rev().map(|&(rho, z)| (z, rho)).collect();
    let hole_table: Vec<(f64, f64)> =
        if crown_i == 0 { Vec::new() } else { inner.iter().map(|&(rho, z)| (z, rho)).collect() };

    // The floor gets a RADIAL allowance, not a normal one. Below the surface's
    // own rim there is no declared surface to offset along a normal — the part
    // ends at the rim circle — and `Clear`'s standing law is that the wall
    // takes the allowance and the floor is the feature's own datum. So the
    // stack's bottom sample is the nominal rim grown radially, which is also
    // never SMALLER than the offset meridian's own rim (the normal offset there
    // leans partly upward), so the table stays monotone by construction.
    //
    // The hole needs no mirror of this. Its ceiling is NARROWEST at its own
    // bottom sample, so `interp_table`'s clamp below that carries the smallest
    // radius downward — the least the pass may cut, which is the quiet
    // direction on the inward side.
    let rim_nominal = derive_revolve_rim_radius(feature, 0.0)?;
    if moat_table[0].0 > base_z + REVOLVE_EPS {
        moat_table.insert(0, (base_z, rim_nominal + skin));
    }

    // The stack is TWO equal-bite runs, broken at the surface's own crown, and
    // the break is not cosmetic. Above the crown there is no part at all, so a
    // level landing there clears the whole working region; a stack that merely
    // divided `stock_top → base` would straddle the crown and leave everything
    // above it standing inside the topmost island — a PLUG of pure scrap
    // sitting on the part, which no later waterline pass can reach around.
    // Each run is divided separately so `stepdown` stays a maximum in both.
    let crown_cut = crown_z.min(stock_top);
    let level_zs = |from: f64, to: f64| -> Vec<f64> {
        let span = from - to;
        if span <= REVOLVE_EPS {
            return Vec::new();
        }
        let n = (span / stepdown).ceil().max(1.0);
        let step = span / n;
        (1..=n as usize).map(|k| from - step * k as f64).collect()
    };
    // Read at the level's FLOOR, never its top: the island is wider lower down,
    // and the standoff has to clear the worst of what the flute passes.
    //
    // Interpolated, not read conservatively from the wider bracketing sample.
    // The conservative read was tried and measured: on `cone.cam` it stands the
    // island 0.078mm proud — one whole rho step of a 256-sample meridian —
    // where interpolation is EXACT on that straight meridian and errs by the
    // chord's own ~1e-4mm on a sphere. It is also what keeps this island and
    // `stock::Floor::z_at`'s floor the same curve, since that reads the same
    // samples the same way; an island on a different curve than the removal
    // model is a worse bug than a tenth of a micron of allowance.
    let mut moat: Vec<ClearLevel> = Vec::new();
    for z in level_zs(stock_top, crown_cut) {
        // Above the crown of a hole-bearing shape the moat run reaches all the
        // way in: the column that would have stood there is the hole's own, and
        // hole and moat are one annulus up here. Where no hole opens under it
        // that column STANDS — a band declares nothing inside its top circle —
        // and the clamped table says so by carrying the crown's own radius up.
        let island_r = if hole_table.is_empty() { interp_table(&moat_table, z) } else { 0.0 };
        moat.push(ClearLevel { z, island_r });
    }
    let below = level_zs(crown_cut, base_z);
    moat.extend(
        below.iter().map(|&z| ClearLevel { z, island_r: interp_table(&moat_table, z) }),
    );
    // The hole's levels are the SAME cuts as the moat's below the crown — one
    // break, one division — so a level is one trip down for both work regions.
    let hole: Vec<HoleLevel> = if hole_table.is_empty() {
        Vec::new()
    } else {
        below.iter().map(|&z| HoleLevel { z, ceiling_r: interp_table(&hole_table, z) }).collect()
    };
    (!moat.is_empty()).then_some(RevolveClearStacks { moat, hole })
}
