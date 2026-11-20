use crate::math::Portable as _;
// ---- finish: 3-axis ball-nose waterline finishing of an analytical surface --
// Camp 2 (DECISIONS.md "Feature geometry is three camps"): a closed-form surface
// needs only arithmetic to finish, no geometry kernel — the "it's a wall now"
// bet. This op machines the upper hemisphere of a `Sphere` with constant-Z
// (waterline) passes computed straight from the sphere equation.
//
// The tool must be a ball-nose. For a sphere the surface normal is radial, so
// the tool-*centre* (ball centre) path is simply the surface offset outward by
// the ball radius: a larger concentric sphere of radius R' = R_part + R_ball.
// Holding the tool-centre Z fixed makes each pass a horizontal circle on R'.
//
// Scope of this version:
//   - Sphere, Cone, and Torus (R1 offset-closure), plus the R2 pointwise-offset
//     surfaces: Ellipsoid of revolution (ROADMAP Phase 3.5; see finish_ellipsoid)
//     and the elliptic paraboloid dome (CHECKLIST row 3.10; finish_paraboloid),
//     plus the tangent-ogive nose (row 5.1; finish_ogive — an "R2" row whose
//     circular meridian closes in R1 form) and the catenoid neck (row 5.3;
//     finish_catenoid — the first genuinely-R2 revolved surface, and the
//     first anticlastic one).
//   - Fixed Z stepdown (waterline) or constant-scallop passes; the R2 surfaces
//     use the numeric Newton march on the exact CL chord (DECISIONS.md "R2
//     numeric constant-scallop").
//   - Upper hemisphere/half only (from the equator up) — below it is an
//     undercut a 3-axis machine can't reach. There is deliberately *no* runtime
//     re-entrancy check here: every declarable surface is its non-undercut
//     TAXONOMY.md row by construction (dome-from-equator, base-down cone,
//     upper torus half), so an undercut is unrepresentable rather than
//     detected. That check becomes real — and lands with — the first primitive
//     that can express one (DECISIONS.md "Re-entrancy detection:
//     unrepresentable by construction").
//   - Concave passes run the shared ⚠ gouge guard (concave_gouge_check): a
//     torus whose central bore is too small for the tool is a hard error, not a
//     silently truncated (or silently gouged) inner-bowl pass. See that function
//     and finish_torus below.

use crate::gcode::{f, stock_suffix};
use crate::ops::clear::{comp_word, direction_note};
use crate::parse::{CompMode, CompressMode, Feature, Operation, Part, Pt, Tool};
use crate::CHORD_TOL_MM;
use std::f64::consts::PI;

// ---- ⚠ concave gouge guard (shared, first-class) ---------------------------
// TAXONOMY.md "⚠ is the silent-failure risk, not the crash risk": on a *concave*
// region the ball-nose's closed-form CL point gouges **silently** once the ball
// radius exceeds the local concave radius of curvature — the analytic offset
// self-intersects and no emitted coordinate ever looks wrong. So every finishing
// pass that machines a concave region must run this up front. It is the
// generalization (ROADMAP Phase 3.5 "the ⚠ gouge trim as a named, shared step")
// of the torus inner-bowl guard from one per-op `if` (DECISIONS.md "Self-audit
// corrections…: Torus finishing silently dropped unreachable inner-bowl passes")
// into a first-class test, matching "An unmachinable feature is a hard error,
// not a skipped op".
//
// Which passes call it, stated so the *absence* of a call is a fact, not a gap:
//   - Torus inner bowl — concave (curves around the central hole): **calls it.**
//   - Paraboloid bowl — concave everywhere (the first concave R2 surface):
//     **calls it**, against the tightest concave radius of curvature
//     a²/(2·depth) at the vertex umbilic (the meridian and circumferential
//     radii coincide there and both grow strictly outward, so the vertex is
//     the global worst case). See finish_paraboloid_bowl — the guard is also
//     exactly the condition that keeps its offset locus regular.
//   - Catenoid neck — anticlastic (the first): the hoop direction is convex
//     but the *meridian* is concave toward the tool, and a ball that can't
//     nestle into the meridian cove gouges it regardless of the convex hoop —
//     **calls it**, against the tightest concave meridian radius c·cosh²w = c
//     (= neck_dia/2) at the waist. See finish_catenoid — as with the bowl,
//     the guard is also exactly what keeps its offset locus regular.
//   - Catenoid throat (inside orientation) — the neck's anticlastic surface
//     from the void side: the meridian is now convex toward the tool, but the
//     *hoop* direction is concave (the wall curls around the tool, like the
//     bore) with normal-section radius c·cosh²w, tightest c (= neck_dia/2) at
//     the waist: **calls it**, against the SAME bound as the outside neck — a
//     minimal surface's principal radii agree in magnitude everywhere, so its
//     guard bound is orientation-independent. Unlike every consumer above,
//     here the guard is NOT what keeps the offset locus regular (that holds
//     unconditionally); it is what keeps the waist CL ring from collapsing
//     through the axis — the cylinder-bore failure shape. See
//     finish_catenoid_throat.
//   - Pseudosphere band — anticlastic (the second, constant negative
//     curvature): same shape as the catenoid — convex hoop, meridian concave
//     toward the tool — but the tight end is the *base* circle, where the
//     meridian radius c·sinh u is smallest on the band (it vanishes entirely
//     at the cusp rim the band is forced to truncate above): **calls it**,
//     against c·sinh(u_base). See finish_pseudosphere — once more the guard
//     is exactly what keeps its offset locus regular.
//   - Pseudosphere funnel (inside orientation) — the tractricoid from the
//     void side: meridian convex toward the tool, hoop concave (curls around
//     the tool) with normal-section radius c/sinh u, tightest at the TIP
//     circle: **calls it**, against c/sinh(u_tip). Note the contrast with the
//     catenoid pair: the pseudosphere is NOT minimal, so its guard bound and
//     binding end BOTH move with orientation (outside: meridian c·sinh u at
//     the base; inside: hoop c/sinh u at the tip) — the catenoid's
//     orientation-independent bound was that surface's privilege. Like the
//     throat, the guard is not the regularity condition (the funnel's locus
//     is regular unconditionally); it is exactly the tip CL ring's
//     not-through-the-axis condition (rho_cl = sech u·(c − r_ball·sinh u)).
//     See finish_pseudosphere_funnel.
//   - Vase neck arc — the first *mixed-curvature* meridian (synclastic belly
//     arc blending G1 into the anticlastic neck arc at the inflection): the
//     neck arc is concave toward the tool with constant radius r (the ogee
//     arc radius, derive_vase_arc_radius): **calls it**, against r. The
//     convex belly arc contributes no concave radius — the guard bound comes
//     from one arc of the meridian only, a situation no single-curvature
//     surface could produce. See finish_vase.
//   - CylinderBore inside bore wall — concave (curves around the tool): **calls it.**
//     A straight-flank pass, not ball-nose, but the same failure shape: the
//     coaxial tool-centre circle of radius R - r_tool self-intersects (collapses
//     through the axis) once r_tool > R, so the bore radius R is its concave
//     radius of curvature. See finish_cylinder_bore.
//   - Sphere / Cone / Ellipsoid / Paraboloid / Ogive upper domes and noses, and
//     the torus *outer* half — all convex; a tool offset outward from a convex
//     surface never self-intersects, so there is no concave radius to compare
//     and they correctly do not call it.
//   - CylinderBoss outside wall — convex by construction (material inside the circle,
//     tool outside at R + r_tool): **correctly does not call it.** The absence
//     is a stated fact, not a gap; see finish_cylinder_boss.
//   - EllipticBore inside wall — concave (curves around the tool) with a
//     radius of curvature that now *varies* along the ring: tightest
//     (minor/2)²/(major/2) at the major-axis vertices, the evolute bound past
//     which the inward parallel curve self-intersects: **calls it.** The
//     first flank consumer whose guard radius differs from its fit radius
//     (the circular bore's coincided at R); passing the guard implies the
//     tool also fits, since (minor/2)²/(major/2) <= minor/2. See
//     finish_elliptic_bore.
//   - EllipticBoss outside wall — convex by construction, like CylinderBoss:
//     **correctly does not call it**; an outward parallel of a convex curve
//     never self-intersects. See finish_elliptic_boss.
//   - Mesh raster drop-cutter (camp 3) — **correctly does not call it**, for a
//     different reason than convexity: the emitted height is the max of the
//     ball-drop contact over every triangle (the upper envelope), which keeps
//     the centre at least r from every sampled triangle by construction —
//     concavity is handled by max(), not by an offset that could
//     self-intersect. The residual error lives *between* samples (local
//     curvature x step^2/8, plus the mesh's own chord error) — exactly the
//     tessellation/sampling error camp 3 is chartered to accept and the
//     analytical camps reject. See finish_mesh.
//   - SplineRevolve cap — a general meridian can be concave toward the tool
//     in stretches (vase-neck-like coves): **calls it**, against the tightest
//     concave meridian radius from a dense per-knot-span scan of the exact
//     signed curvature. The HOOP direction on the cap envelope is convex
//     toward the tool everywhere — with z' <= 0 the outward normal's radial
//     component n_r = −z'/|C'| is >= 0, so the hoop's curvature centre (on
//     the axis) sits on the material side — and correctly contributes no
//     bound; a stated fact, not a gap. See finish_spline_revolve.
//   - SplineRevolveFunnel (inside orientation) — the first TWO-principal-
//     direction scan. A general cavity meridian can be concave toward the
//     tool in the MERIDIAN direction (a bowl bottom — there the guard is the
//     offset-regularity condition, the neck/bowl consumers' role) AND is
//     concave in the HOOP direction wherever the wall leans inward (n_r < 0;
//     normal-section radius rho/|n_r| by Meusnier — there the guard is the
//     CL-ring-not-through-the-axis condition, the throat/funnel consumers'
//     role). On a free meridian neither role dominates a priori — the named
//     precedents were each ONE of these, this op is both at once — so the
//     scan tracks the tightest bound over BOTH directions and **calls it**
//     once with the winner, naming the binding direction in the detail. See
//     finish_spline_revolve_funnel.
//
// `concave_radius` is the *tightest* (smallest) concave radius of curvature the
// pass will contact — the worst case, since the tool fits a valley iff it fits
// the tightest point of it. Rejects when the tool can't fit: r_tool >
// concave_radius. `detail` carries the feature-specific geometric explanation so
// the shared error still says *why* this particular surface can't take this
// tool (TAXONOMY.md "useful for explaining why a feature is unmachinable").
fn concave_gouge_check(
    feature_name: &str,
    tool_num: usize,
    r_tool: f64,
    concave_radius: f64,
    detail: &str,
) -> Result<(), String> {
    concave_gouge_check_with_stock(feature_name, tool_num, r_tool, 0.0, concave_radius, detail)
}

// The gouge guard's stock_to_leave-aware form (DECISIONS.md "stock_to_leave
// first slice…"): a roughing pass offsets the locus by r_tool + allowance, and
// it is that FATTER effective offset that self-intersects past the concave
// radius — so the allowance joins the tool radius on the left of the
// comparison. At allowance == 0 this is concave_gouge_check verbatim,
// message included (its text is load-bearing in tests and goldens-adjacent
// error assertions).
fn concave_gouge_check_with_stock(
    feature_name: &str,
    tool_num: usize,
    r_tool: f64,
    stock_allowance: f64,
    concave_radius: f64,
    detail: &str,
) -> Result<(), String> {
    if r_tool + stock_allowance > concave_radius {
        if stock_allowance > 0.0 {
            return Err(format!(
                "feature '{}' cannot be finished with T{}: {}. The tool radius ({:.3}mm) plus the stock_to_leave offset ({:.3}mm) exceeds the tightest concave radius of curvature ({:.3}mm), so the roughing offset self-intersects and the closed-form path would gouge silently — use a smaller tool or reduce stock_to_leave",
                feature_name, tool_num, detail, r_tool, stock_allowance, concave_radius
            ));
        }
        return Err(format!(
            "feature '{}' cannot be finished with T{}: {}. The tool radius ({:.3}mm) exceeds the tightest concave radius of curvature ({:.3}mm), so the tool offset self-intersects and the closed-form path would gouge silently — use a smaller tool",
            feature_name, tool_num, detail, r_tool, concave_radius
        ));
    }
    Ok(())
}

// The sweep-then-VERIFY budget chooser (DECISIONS.md "stock_to_leave corner
// roll…"), one home for every carrier whose exact-tip price has no
// monotonicity theorem in the skin: the budget h is the max of `price` over
// the 5-point skin sweep {0, ¼, ½, ¾, 1}·allowance (choosing a' =
// allowance − h), and the ACTUAL emitted stack is then re-priced at a' —
// the guarantee rests on that verification, never on the sweep having
// caught the worst alignment. Returns (a', h); h is what the title audits
// (the budget charged — the verified price never exceeds it, or the ceiling
// check here rejected). INFINITY from `price` is the uncut-ridge shape
// (non-overlapping passes), rejected via `ridge_err`; a breached ceiling via
// `exceed_err`, handed the worst price seen so the message can state it.
fn sweep_verify_budget(
    allowance: f64,
    price: &dyn Fn(f64) -> f64,
    ridge_err: &dyn Fn() -> String,
    exceed_err: &dyn Fn(f64) -> String,
) -> Result<(f64, f64), String> {
    let mut h: f64 = 0.0;
    for k in 0..=4 {
        h = h.max(price(allowance * k as f64 / 4.0));
    }
    if h.is_infinite() {
        return Err(ridge_err());
    }
    let verified = if h < allowance { price(allowance - h) } else { h };
    if verified.is_infinite() {
        return Err(ridge_err());
    }
    // The ceiling is asked with the same 1e-9 the sum below already carries:
    // at allowance == scallop the capped march prices EXACTLY the scallop, and
    // whether that lands a few ulps above or below the allowance is the
    // platform's libm (glibc refused it, MSVC's UCRT let it through —
    // DECISIONS.md, the 2026-10-03 native-Windows entry). A refusal must not
    // depend on which transcendental library rounded last.
    if h >= allowance - 1e-9 || (allowance - h) + verified > allowance + 1e-9 {
        return Err(exceed_err(h.max(verified)));
    }
    Ok((allowance - h, h))
}

/// The stepover-mode roughing split, from the one home in `parse`, with this
/// file's error wording wrapped around its two failure shapes. Carriers not
/// yet moved to that home keep their own local budget; this returns (0, 0) for
/// an op carrying no allowance, which is the finishing pass.
fn stepover_budget(
    feature: &Feature,
    tool: &Tool,
    op: &Operation,
    spacing: f64,
) -> Result<(f64, f64), String> {
    // No allowance is the finishing pass, and it rides the surface itself.
    let Some(allowance) = op.stock_to_leave else {
        return Ok((0.0, 0.0));
    };
    match crate::parse::derive_stepover_budget(feature, tool, op) {
        // Only carriers that HAVE been moved to the shared home call this, so
        // a missing arm is an internal invariant violation, not a zero skin.
        // Answering (0, 0) here would ride the nominal surface and cut the
        // whole allowance away — the first draft of this adapter did exactly
        // that, and three emitter unit tests caught it.
        None => unreachable!(
            "no shared budget arm for the carrier behind '{}', which reads this home",
            op.feature_name
        ),
        Some(Ok(split)) => Ok(split),
        Some(Err(crate::parse::BudgetFail::Ridge)) => Err(format!(
            "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: at a {:.3}mm stepover adjacent passes do not overlap — an uncut ridge remains, which no allowance can ceiling — reduce stepover or increase stock_to_leave",
            op.feature_name, allowance, op.tool_num, spacing
        )),
        Some(Err(crate::parse::BudgetFail::Exceeded(worst))) => Err(format!(
            "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm stepover leaves a worst-case cusp of {:.3}mm between adjacent passes, which meets or exceeds the allowance ceiling (cusps count against it — max, not min) — reduce stepover or increase stock_to_leave",
            op.feature_name, allowance, op.tool_num, spacing, worst
        )),
    }
}

// `OpMoves`: text plus, for the comp=control arms, the emitter-authored
// resolved tool-center channel (DECISIONS.md "the control-comp centerline
// resolve"). Every engine-mode arm wraps through `From<Vec<String>>`
// (resolved: None) — the emitted moves's shape is uniform, the channel is not.
pub fn finish(p: &Part, op: &Operation) -> Result<crate::gcode::OpMoves, String> {
    let feature = p.features.get(&op.feature_name).expect("feature must exist");
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    // The tool's form decides the path, not its name — the custom-tool-profile
    // precedent, and the reason the corner rounder needed no fifth verb. A
    // `CornerRounder` cuts the feature's declared `edge_round`; an endmill on
    // the same feature cuts its wall (DECISIONS.md → "the corner rounder gets
    // an operation").
    if tool.rounder.is_some() {
        return finish_edge_round(p, op, feature, tool);
    }
    // Same rule, one form over: a `ChamferMill` cuts the feature's declared
    // chamfer (DECISIONS.md → "the chamfer tool…", slice 2).
    if tool.chamfer.is_some() {
        return finish_chamfer(p, op, feature, tool);
    }
    if tool.profile.is_some() {
        return match feature {
            Feature::Sphere { .. } => finish_sphere_profile(p, op, feature, tool).map(Into::into),
            // validate() requires a profile tool's Finish to reference a
            // Sphere, so this is unreachable through the normal parse path --
            // same invariant, same failure mode as the panic below.
            _ => panic!("Finish operation with a custom tool profile requires a Sphere feature"),
        };
    }
    match feature {
        Feature::Sphere { .. } => finish_sphere(p, op, feature).map(Into::into),
        Feature::Cone { .. } => finish_cone(p, op, feature).map(Into::into),
        Feature::Torus { .. } => finish_torus(p, op, feature).map(Into::into),
        // The polyline pair routes to the profile module — the former
        // Contour op, `Finish`'s polyline arm since the op merge.
        Feature::ProfilePocket { .. } | Feature::ProfileBoss { .. } => crate::ops::profile::finish_profile_wall(p, op),
        // The 3D-wire pair routes to the same module: wall tracing is one
        // operation regardless of cross-section, and now regardless of the
        // top edge's Z profile (DECISIONS.md "the 3D-wire Contour…").
        Feature::ContourPocket { .. } | Feature::ContourBoss { .. } => crate::ops::profile::finish_contour_wire(p, op),
        Feature::CylinderBore { .. } => {
            ring_entry_kept(op)?;
            finish_cylinder_bore(p, op, feature)
        }
        Feature::CylinderBoss { .. } => {
            ring_entry_kept(op)?;
            finish_cylinder_boss(p, op, feature)
        }
        Feature::Mesh { .. } => Ok(finish_mesh(p, op, feature).into()),
        Feature::SplineRevolve { .. } => finish_spline_revolve(p, op, feature).map(Into::into),
        Feature::SplineRevolveFunnel { .. } => finish_spline_revolve_funnel(p, op, feature).map(Into::into),
        Feature::NurbsPatch { .. } => Ok(finish_nurbs_patch(p, op, feature).into()),
        _ => panic!("Finish operation requires a Sphere, Cone, Torus, Ellipsoid, Paraboloid, ParaboloidBowl, Ogive, Catenoid, CatenoidThroat, Pseudosphere, PseudosphereFunnel, Vase, CylinderBore, CylinderBoss, EllipticBore, EllipticBoss, Mesh, SplineRevolve, SplineRevolveFunnel, or NurbsPatch feature"),
    }
}

fn finish_sphere(p: &Part, op: &Operation, feature: &Feature) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_ball = tool.dia / 2.0;

    let (cx, cy, cz, r_part) = match feature {
        Feature::Sphere { dia, x, y, z } => (*x, *y, *z, dia / 2.0),
        _ => unreachable!(),
    };

    // stock_to_leave (DECISIONS.md "Stock-to-leave semantics…"): the stated
    // allowance is a ceiling on TOTAL remaining material including this pass's
    // own cusps, so the cusp height h of the chosen spacing is budgeted out of
    // it first and only a' = allowance − h becomes nominal offset. Roughing
    // the sphere is then finishing the inflated sphere r_part + a': the locus
    // stays a concentric sphere (r_off grows by a'), so generate_passes'
    // convex-transition proof carries unchanged. The `+ a` terms are appended
    // without re-associating the existing sums, so a' == 0 (field absent) is
    // byte-identical output (IEEE x + 0.0 == x) — pinned by every golden.
    let mut passes = Vec::new();

    let title = if let Some(scallop) = op.scallop {
        // Constant scallop: the march guarantees cusp == scallop on the
        // surface it runs against, so h = scallop exactly — known up front,
        // no circularity (validate() rejected scallop >= allowance). The cusp
        // triangle is cut by the PHYSICAL ball on the ROUGHED surface, so
        // r_part + a goes into the triangle while r_ball stays r_ball — a
        // single "effective tool radius" substitution here would misplace
        // the cusp by O(a/r) and breach the max-not-min guarantee.
        let a = op.stock_to_leave.map(|allowance| allowance - scallop).unwrap_or(0.0);
        let r_off = r_part + r_ball + a;
        let theta = crate::parse::derive_scallop_angle(scallop, r_ball, r_part + a);
        let n = ((PI / 2.0) / theta).ceil().max(1.0) as usize;
        for i in 0..=n {
            let phi = (i as f64 * theta).min(PI / 2.0);
            let dz = r_off * phi.pcos();
            let z = cz + dz;
            let rho = r_off * phi.psin();
            passes.push((z, rho));
        }
        format!("(--- copy surface: {} with T{} (ball-nose constant scallop){} ---)", op.feature_name, op.tool_num, stock_suffix(op, scallop))
    } else {
        let stepdown = op.stepover.expect("Finish requires a stepover or scallop");
        // Waterline: the worst adjacent pass pair is the first one at the
        // apex (acos is steepest at 1, so uniform dz steps give their widest
        // angle there). h is priced at FULL inflation — the fixed-Z convex
        // tight extreme. The first slice priced this pair at NOMINAL
        // geometry, arguing the inflated r_off compresses the apex angle
        // and slackens the triangle; the corner-roll slice's chord-fixed
        // monotonicity lemma (parse::derive_cusp_height_flat) showed that
        // argument backwards — the crown CHORD grows like sqrt(2·r_off·Δz),
        // faster than the angle compresses, and both triangle inputs (chord,
        // surface radius) run the price UP with the skin, so the nominal
        // price under-ran the emitted stack's true crown cusp by
        // O(a'·Δz/r_ball) — a small but real breach of the max-not-min
        // ceiling (DECISIONS.md "stock_to_leave corner roll…", the audit
        // finding; rise-with-skin pinned by the cusp-tip ceiling test). The
        // concave carriers correctly keep their NOMINAL extreme — offset
        // toward the curvature centre runs their prices DOWN with skin.
        let r_off_nom = r_part + r_ball;
        let (a, h) = if let Some(allowance) = op.stock_to_leave {
            let theta_max = (1.0 - stepdown / (r_off_nom + allowance)).max(0.0).pacos();
            let h = crate::parse::derive_cusp_height(theta_max, r_ball, r_part + allowance);
            if h.is_infinite() {
                return Err(format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: at a {:.3}mm stepdown adjacent passes do not overlap near the apex — an uncut ridge remains, which no allowance can ceiling — reduce stepover or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, stepdown
                ));
            }
            if h >= allowance {
                return Err(format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm stepdown leaves a worst-case cusp of {:.3}mm near the apex, which meets or exceeds the allowance ceiling (cusps count against it — max, not min) — reduce stepover or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, stepdown, h
                ));
            }
            (allowance - h, h)
        } else {
            (0.0, 0.0)
        };
        let r_off = r_off_nom + a;
        let z_top = cz + r_off;
        let n = (r_off / stepdown).ceil().max(1.0) as usize;
        for i in 0..=n {
            let z = (z_top - i as f64 * stepdown).max(cz);
            let dz = z - cz;
            let rho = (r_off * r_off - dz * dz).max(0.0).sqrt();
            passes.push((z, rho));
        }
        format!("(--- copy surface: {} with T{} (ball-nose waterline){} ---)", op.feature_name, op.tool_num, stock_suffix(op, h))
    };

    Ok(generate_passes(p, op, title, cx, cy, passes))
}

// ---- Ellipsoid (R2): pointwise ball offset from the closed-form normal ------
// Camp 2b / regime R2 (ROADMAP Phase 3.5; TAXONOMY.md "analytic per point,
// numeric per path"; DECISIONS.md "Implementation regimes"). A spheroid is the
// first surface whose offset is *not* the same species as itself — there is no
// larger concentric spheroid the ball-centre rides on. So instead of the
// offset-closure algebra finish_sphere/_cone/_torus use, the tool-centre (CL)
// path is built one point at a time from the surface's closed-form normal.
//
// Meridian parameter t: 0 at the pole, pi/2 at the equator. With equatorial
// radius a = dia/2 and polar semi-axis c = height, the contact point in a
// meridian half-plane is (r, z) = (a·sin t, c·cos t) about the centre. The
// implicit surface F = (r/a)^2 + ((z-cz)/c)^2 - 1 has gradient ∝ (r/a^2,
// (z-cz)/c^2), i.e. an outward normal direction (sin t / a, cos t / c) in
// (radial, axial) coordinates. Normalizing by L = sqrt(sin^2 t/a^2 + cos^2
// t/c^2) and pushing the contact point out one ball radius along it gives the
// CL point:
//   rho_cl = a·sin t + r_ball·(sin t / a)/L
//   z_cl   = cz + c·cos t + r_ball·(cos t / c)/L
// Sanity: with c == a (a true sphere) L = 1/a and both collapse to
// finish_sphere's (a+r_ball)·sin/cos phi — the exact R1 limit, which is the
// golden anchor test_ellipsoid_reduces_to_sphere checks.
//
// By rotational symmetry every point on one t-latitude shares rho_cl and z_cl,
// so each pass stays a horizontal circle and reuses generate_passes' proven
// convex-dome transition (the CL locus is the offset of a convex surface, hence
// itself convex; rho_cl increases and z_cl decreases monotonically pole→equator,
// so "feed radially out, then plunge" never cuts a chord through the interior).
//
// Spacing is uniform in t, sized so the on-surface step stays within stepover:
// the meridian arc element ds = sqrt(a^2 cos^2 t + c^2 sin^2 t)·dt <= max(a,c)·dt,
// so n = ceil(max(a,c)·(pi/2)/stepover) uniform t-steps bound it.
// Constant-scallop spacing is the numeric Newton march on the exact CL chord
// (parse::derive_ellipsoid_step_t; DECISIONS.md "R2 numeric constant-scallop").

// ---- Paraboloid (R2): pointwise ball offset, parametrized by radius ---------
// The elliptic paraboloid dome (CHECKLIST row 3.10), second R2 surface after
// the spheroid and the same species of math: no elementary offset, so the CL
// path is built one point at a time from the closed-form normal. Unlike the
// spheroid there is no natural closed angle range — the meridian is
// parametrized directly by radial distance rho in [0, a].
//
// With base radius a = dia/2 and apex height h above the base plane z0, the
// surface is z(rho) = z0 + h·(1 - rho²/a²). Slope s(rho) = 2h·rho/a², so the
// outward unit normal in (radial, axial) coordinates is (s, 1)/L with
// L = sqrt(1 + s²), and the CL point is
//   rho_cl = rho + r_ball·s/L
//   z_cl   = z0 + h·(1 - rho²/a²) + r_ball/L
// The meridian radius of curvature is R_part(rho) = L³·a²/(2h), minimum
// a²/(2h) at the apex and strictly increasing outward, so
//   drho_cl/drho = 1 + r_ball/R_part > 0,   dz_cl/drho = -s·(1 + r_ball/R_part) < 0
// — rho_cl grows and z_cl falls monotonically apex→rim, which is exactly the
// convexity generate_passes' feed-out-then-plunge transition needs (the same
// argument as finish_ellipsoid). Note the rim contact (rho = a) has a tilted
// normal — a paraboloid never reaches a vertical tangent — so the last CL
// point sits at rho > a and z > z0; like every finish op this assumes roughing
// cleared the surroundings to the base plane (the sphere/ellipsoid last pass
// equally dips below its equator plane).
//
// Waterline spacing is uniform in rho, sized by the meridian arc length
// (∫L·drho <= sqrt(a² + 4h²)) so the on-surface step stays within stepover.
// Constant scallop marches parse::derive_paraboloid_step_rho (the ellipsoid's
// Newton scheme with rho for t). No sphere limit exists to anchor against, so
// correctness is pinned by the direct off-surface residual tests below
// (every CL point exactly one ball radius from the surface along the normal).

// ---- Paraboloid bowl (R2 ⚠): the first concave R2 surface -------------------
// CHECKLIST row 3.9. The dome above, opened upward as a top-open cavity:
// mouth circle of radius a = dia/2 at the stock top, vertex at
// z_v = stock.z - depth, surface z(rho) = z_v + h·rho²/a² (h = depth).
// Authored top-down by `depth` like the CylinderBore bore, so "top-open" is
// structural, not checked.
//
// Concave everywhere, so the shared ⚠ gouge guard runs FIRST, against the
// tightest concave radius of curvature: a²/(2h) at the vertex (an umbilic —
// meridian radius R_part(rho) = L³·a²/(2h) and circumferential radius L·a²/(2h)
// coincide there and both grow strictly outward, so the vertex is the global
// worst case). The guard is load-bearing twice over: the CL point offsets
// *into* the bowl along the void-side normal (-s, 1)/L,
//   rho_cl = rho - r_ball·s/L
//   z_cl   = z_v + h·rho²/a² + r_ball/L
// and with r_ball <= a²/(2h) <= R_part(rho),
//   drho_cl/drho = 1 - r_ball/R_part >= 0,   dz_cl/drho = s·(1 - r_ball/R_part) >= 0
// — the offset locus is regular (no self-intersection), rho_cl stays >= 0, and
// passes ordered mouth→vertex descend monotonically in both z and rho. Under
// stock_to_leave the same algebra holds with r_ball + a' in place of r_ball
// (the roughing locus is the fatter offset), which is exactly what the
// _with_stock guard form enforces per mode below. That
// monotonicity is what makes generate_passes' transition safe here: feeding
// *inward* to the next (smaller) radius at the current height moves through
// open air above the locus, then the plunge lands on it — the exact pattern
// the torus inner bowl already rides through the same function. Unlike the
// torus there is no sibling pass that pre-cut the first ring, so the mouth
// ring is kept; the final pass is the on-axis vertex touch at z_v + r_ball
// (the ball fits — the guard said so).
//
// The first CL point sits at z_mouth + r_ball/L(a), *above* the stock top, so
// the initial rapid+plunge is trivially clear of material.

// ---- Ogive (row 5.1): the "R2" revolved row that closes in R1 form ----------
// The tangent-ogive nose (CHECKLIST row 5.1), first of TAXONOMY §5's analytic
// surfaces of revolution. Promoted expecting the R2 pointwise machinery, but
// the meridian is a *circular arc* — radius R_o = derive_ogive_radius(a, h),
// centre C = (rho_c, z0) in the base plane with rho_c = a − R_o ≤ 0 — and a
// circular arc is self-offsetting: the ball-centre locus is the concentric
// arc of radius R_o + r_ball about the same C (the surface is a spindle-torus
// patch, and the torus is R1). So no Newton march, no numeric spacing — both
// branches below are closed-form, the same algebra species as finish_sphere.
//
// The apex is a *sharp point* (the meridian meets the axis at elevation angle
// phi_apex < 90°, like the cone's tip), so the CL locus is piecewise, exactly
// finish_cone's cap-plus-body precedent:
//   - body (normal elevation psi in [psi_rim, phi_apex]): contact on the arc,
//       CL = C + (R_o + r_ball)·(cos psi, sin psi)
//   - cap (psi in [phi_apex, 90°]): contact pinned at the apex corner,
//       CL = apex + r_ball·(cos psi, sin psi)
//   - rim roll (psi in [0, psi_rim]): contact pinned at the base-rim corner,
//       CL = rim + r_ball·(cos psi, sin psi) — the cap's move mirrored at the
//       bottom. TANGENT form: the arc centre C sits in the base plane
//       (sin(psi_rim) = 0), the wall meets the base vertically, and this
//       piece is empty — the locus is the original two pieces. SECANT form
//       (profile_radius authored, DECISIONS.md "Secant ogive…"): C drops
//       below the base plane (derive_secant_ogive_centre), the wall meets
//       the base at an angle, and the ball rolls the rim corner down to the
//       horizontal-normal base pass.
// with sin(phi_apex) = (h − z_c)/R_o, cos(phi_apex) = −rho_c/R_o and
// sin(psi_rim) = −z_c/R_o, cos(psi_rim) = (a − rho_c)/R_o (z_c ≡ 0 tangent).
// Adjacent pieces meet tangentially (each seam evaluates equal on both
// formulas), and along the whole locus rho grows and z falls strictly as psi
// drops from 90° to 0 — the monotonicity generate_passes'
// feed-out-then-plunge transition needs, by the same convex argument as the
// sphere. validate() enforces h ≥ a and, for the secant form, R > R_o —
// together they keep the arc's crest below the base plane and the apex
// normal inside 90°, so the would-be undercut stays unrepresentable (per the
// variant's doc comment).
//
// Constant scallop is a constant angular step, closed-form like the sphere's:
// theta = derive_scallop_angle(scallop, r_ball, R_o) — the meridian curvature
// is R_o *everywhere* on the body, so the R1 cusp triangle is exact there.
// Across the cap the same theta is kept: contact sits at the apex corner
// (r_part → 0), where the cusp triangle degenerates and allows enormous
// steps, so reusing the body's theta is strictly conservative — over-sampled,
// never under-cut. Waterline spacing is a fixed Z stepdown over the full CL
// height, finish_cone's shape with the flank formula swapped for the arc.
//
// At h == a the ogive IS a hemisphere (R_o = a, C on the axis, empty cap):
// both branches reduce to finish_sphere's formulas exactly — the
// reduces-to-sphere anchor tests below. Convex everywhere in both forms (the
// arc is convex toward the tool and both corners are convex), so this
// correctly does not call concave_gouge_check (see the guard's roster above).
// stock_to_leave (DECISIONS.md "stock_to_leave corner roll…"): roughing the
// ogive is finishing the a'-inflated ogive — the concentric arc r_o + a'
// about the same centre plus a'-SPHERES about the two sharp corners (apex
// always; base rim in the secant form), so every piece keeps its formula
// with r_ball → r_ball + a' on the corner rolls and r_off → r_off + a' on
// the body, and the waterline stack climbs to the inflated cap's pole. The
// `+ skin` terms are appended without re-associating existing sums, so an
// absent field is byte-identical output (IEEE x + 0.0 == x) — pinned by the
// existing goldens. Scallop mode needs NO re-grid (unlike the cone): theta
// is recomputed on the inflated body and the same uniform-psi sweep covers
// all three pieces — corner pairs price cusp(theta, r_ball, a') <
// cusp(theta, r_ball, r_o + a') = scallop by the ANGLE-fixed monotonicity of
// the cusp triangle (the same derivative fact as the chord-fixed lemma on
// parse::derive_cusp_height_flat), which is exactly why "reusing the body's
// theta across the cap" stays conservative under stock; seam straddles
// price on the slacker piece (the arc) at a chord no fatter than the body's,
// strictly under the scallop — no knife-edge. Waterline mode prices the
// per-piece-extreme two-walk max (parse::derive_ogive_stack_cusp).

// ---- Catenoid (row 5.3): the first genuinely-R2 revolved surface ------------
// The catenoid neck (CHECKLIST row 5.3), second of TAXONOMY §5's analytic
// surfaces of revolution — and the one the ogive entry predicted: the cosh
// meridian is NOT an arc, so nothing closes in R1 form and this op is the
// paraboloid pattern (pointwise offset + Newton scallop march) generalized to
// a revolved meridian parametrized by w = (z_w − z)/c: contact
// (rho, z) = (c·cosh w, z_w − c·w), w = 0 at the waist (radius c = neck_dia/2,
// AT the feature top by construction), w_b = acosh(a/c) at the base rim
// (radius a = dia/2, height h = derive_catenoid_height below it).
//
// The meridian tangent going up is (−sinh w, 1), so the outward normal is
// (1, sinh w)/L — and L = sqrt(1 + sinh²w) = cosh w, the identity that keeps
// every line here closed-form:
//   rho_cl = c·cosh w + r_ball/cosh w
//   z_cl   = z_w − c·w + r_ball·tanh w
//
// The first *anticlastic* surface (a minimal surface: principal curvatures
// cancel everywhere): the hoop direction is convex, but the meridian is
// concave toward the tool with radius R_m(w) = c·cosh²w, tightest c at the
// waist — so the shared ⚠ gouge guard runs FIRST, against neck_dia/2. As with
// the bowl the guard is load-bearing twice: with r_ball <= c,
//   drho_cl/dw = sinh w·(c − r_ball/cosh²w) >= 0,   dz_cl/dw = −c + r_ball/cosh²w <= 0
// — the offset locus is regular and passes ordered waist→base grow in rho and
// fall in z monotonically (at the r_ball == c boundary the waist derivative
// vanishes only instantaneously; successive emitted passes still descend).
// That is exactly the shape generate_passes' feed-out-then-plunge transition
// needs, by the same argument as the convex domes: the outward normal's
// radial component (1/cosh w) is positive everywhere, so moving radially
// outward from a CL point at fixed z only gains clearance.
//
// Emission starts at the waist ring (rho = c + r_ball, z = z_w — the wall
// tangent is *vertical* at the waist, so the ball contacts at its own
// equator). The initial plunge to it is safe: descending at that radius, the
// nearest material is the waist rim itself, and the ball ends exactly r_ball
// from it having been strictly farther the whole way down. The base contact
// (w = w_b) has a tilted normal — the meridian never goes horizontal — so the
// last CL point sits beyond a and above the base plane; like the paraboloid
// rim this assumes roughing cleared the surroundings to the base plane.
//
// TRUNCATED form (top_dia authored, DECISIONS.md "Truncated catenoid
// neck…"): the band stops at w_t = acosh(t/c) > 0 instead of reaching the
// waist, so emission starts there and the guard bound moves with it — the
// band's tightest concave meridian radius is c·cosh²(w_t) = t²/c at the top
// circle, looser than the full neck's c (a ball too big for the waist can
// legally finish a band that stops short of it). Regularity and monotone
// descent hold under the moved bound by the same derivatives (both are
// signed by c − r_ball/cosh²w, nonnegative-in-rho / nonpositive-in-z for all
// w ≥ w_t exactly when r_ball ≤ c·cosh²(w_t)). The top contact has a tilted
// normal, so the top CL sits above the top plane by r_ball·tanh(w_t) and
// r_ball/cosh(w_t) outside it — the pseudosphere band's stated rim
// assumption, both ends: anything above the top plane is already cleared.
// The plunge to it is safe by the pseudosphere-tip argument (horizontal
// offset r_ball/cosh w_t, vertical r_ball·tanh w_t, and 1/cosh² + tanh² = 1
// puts the ball exactly r_ball from the top rim only at final depth).
//
// Waterline spacing is uniform in w — z is *linear* in w (z = z_w − c·w), so
// uniform-w passes are also a fixed Z stepdown — sized by the on-surface step
// c·cosh(w)·Δw, largest a·Δw at the base: n = ceil(a·(w_b − w_t)/stepover)
// bounds it within stepover (w_t = 0 for the full neck). Constant scallop
// marches parse::derive_catenoid_step_w (the paraboloid's Newton scheme with
// w for rho and the mirrored-*concave* cusp triangle over the meridian
// radius). No sphere limit exists to anchor against, so correctness is
// pinned by the direct off-surface residual tests below, plus the
// closed-form waist/base spot checks.
//
// stock_to_leave (DECISIONS.md "stock_to_leave anticlastic bands…"): the
// bowl's concave recipe on this band. The three substitutions stay distinct
// (CL locus at r_ball + a'; gouge guard compares r_ball + a'; cusp triangle
// prices the physical ball on the a'-tightened meridian r_part − a'), and
// the guard bound doubles as the INFLATED locus's regularity bound with no
// new derivation: both CL derivatives above are signed by c − r_off/cosh²w,
// one-signed on the whole band exactly when r_off = r_ball + a' ≤
// c·cosh²(w_t) — precisely what concave_gouge_check_with_stock enforces
// against the band's tightest meridian radius.

// ---- Catenoid throat (row 5.3, inside orientation) --------------------------
// The catenoid neck's cosh meridian machined from the void side: the
// converging half of a venturi, a top-open cavity narrowing from the rim
// (radius a = dia/2, in the stock-top plane) down to the waist/throat circle
// (radius c = neck_dia/2) at the bottom. The waist sits AT the bottom by the
// outside neck's rule, mirrored — below a catenoid's waist the surface widens
// again, an undercut for a cavity machined from above — so the depth is
// derived, not authored: h = c·acosh(a/c), literally the same
// parse::derive_catenoid_height (one home serving both orientations). The
// venturi's diverging half is this same feature machined from the other side
// after a flip — a second setup, not a new constructible.
//
// Same meridian, opposite offset: contact (rho, z) = (c·cosh w, z_w + c·w),
// w = 0 at the waist (z_w = stock top − h), w_b = acosh(a/c) at the rim. The
// tangent going up is (sinh w, 1)/cosh w, so the *into-void* normal is
// (−1, sinh w)/cosh w and
//   rho_cl = c·cosh w − r_ball/cosh w
//   z_cl   = z_w + c·w + r_ball·tanh w
//
// From the void side the anticlastic roles swap: the meridian is now *convex*
// toward the tool, and the concave direction is the HOOP — the wall curls
// around the tool like the cylinder bore, with normal-section radius
// c·cosh²w, tightest c at the waist. So the shared ⚠ gouge guard runs FIRST,
// against neck_dia/2 — the SAME bound as the outside neck's meridian check: a
// minimal surface's principal radii agree in magnitude everywhere, so its
// guard bound is orientation-independent. Two things make this consumer
// different from the bowl/neck/band precedents, said explicitly:
//   - The guard is NOT the offset-regularity condition this time. Both CL
//     derivatives are one-signed for every ball radius —
//       drho_cl/dw = sinh w·(c + r_ball/cosh²w) >= 0,   dz_cl/dw = c + r_ball/cosh²w > 0
//     — so the locus is regular unconditionally; what r_ball <= c protects is
//     the waist CL ring itself, whose radius c − r_ball would collapse
//     through the axis (the cylinder-bore failure shape, radial rather than
//     meridian). At the r_ball == c boundary the waist ring degenerates to
//     the on-axis point and generate_passes' sub-tolerance-ring skip absorbs
//     it, like the bowl's vertex.
//   - The through-throat assumption, stated not hidden: at the waist the wall
//     is vertical and the ball contacts at its own equator, so its lower
//     hemisphere dips below the waist plane. The op therefore assumes the
//     bore below the throat is already opened (drilled/roughed through, or
//     the diverging half already cut from the far side) — the mirror of the
//     outside ops' "roughing cleared the surroundings to the base plane".
//
// Emission is rim-first / z-descending, and passes SHRINK in rho going down —
// the first revolved op where they do. generate_passes' transition (feed to
// the next radius at the current height, then plunge) is safe mirrored: the
// into-void normal's radial component (−1/cosh w) is negative everywhere, so
// moving radially INWARD from a CL point at fixed z only gains clearance, and
// the plunge lands on the regular, monotone locus from above — the bowl rides
// the same function with the same shape. The first CL point sits at
// z = stock top + r_ball·tanh w_b, *above* the stock top and inside the mouth
// (rho = a − r_ball·c/a), so the initial rapid+plunge is trivially clear.
//
// Waterline spacing is uniform in w — z is linear in w again (z = z_w + c·w),
// so uniform-w passes are also a fixed Z stepdown — sized by the on-surface
// step c·cosh(w)·Δw, largest a·Δw at the rim: n = ceil(a·w_b/stepover), the
// outside neck's bound verbatim. Constant scallop marches
// parse::derive_catenoid_throat_capped_step_w — the plain convex cusp
// triangle as a SEED, capped by each pair's own exact tip price, because the
// triangle reads the local radius at ONE point and the resulting drift is a
// measured +2.1% of real ridge at a 0.5mm request (DECISIONS.md "Family-wide
// march cap…"; the tangent-lip funnel's fix at a gentler size). Both spacings build waist→rim and reverse — the pseudosphere's
// split-order pattern. No sphere limit exists, so
// correctness is pinned by the direct off-surface residual tests below, plus
// the closed-form rim/waist spot checks.

// ---- Vase (row 5.2): the symmetric two-arc ogee -----------------------------
// The last R2 row, and the one whose meridian was *chosen*, not inherited
// (DECISIONS.md "Vase (row 5.2)…", settled as Option B): "vase" names a shape
// family with no canonical equation, so the constructible is the smallest
// settled meridian — the symmetric two-arc ogee. A convex belly arc rises
// from the equator circle (radius a = dia/2, vertical tangent, in the z
// plane), blends G1 at the inflection into a concave neck arc, and ends at
// the waist circle (radius c = neck_dia/2, vertical tangent) at z + h. The
// G1 conditions pin everything closed-form: tan(θ/2) = (a − c)/h and both
// arc radii r = h/(2 sin θ) (parse::derive_vase_arc_angle / _radius); by the
// symmetric split the inflection contact is the exact midpoint
// ((a + c)/2, z + h/2).
//
// ASYMMETRIC form (`inflection` authored, DECISIONS.md "Asymmetric ogee…"):
// the junction height is the split's one degree of freedom. θ does not move
// (it is pinned by a, c, h alone — both wall ends stay vertical), the radii
// become r1 = i/sin θ (belly) and r2 = (h − i)/sin θ (neck)
// (parse::derive_vase_split_radii, sum invariant), and everything below
// generalizes by substitution: the CL arcs are r1 + r_ball about O1 =
// (a − r1, z) and r2 − r_ball about O2 = (c + r2, z + h), the gouge bound
// follows the NECK radius alone — so the bound moves with the split, looser
// for a low inflection, tighter for a high one — and the per-arc scallop
// triangles take their own radius each. The symmetric path keeps its
// original expressions verbatim (r1 = r2 = r, byte-identical goldens).
//
// Arc centres: belly O1 = (a − r, z), points (a − r + r·cos φ, z + r·sin φ),
// φ ∈ [0, θ] with outward normal (cos φ, sin φ); neck O2 = (c + r, z + h),
// points (c + r − r·cos ψ, z + h − r·sin ψ), ψ ∈ [0, θ] (ψ = 0 the waist),
// outward normal (cos ψ, sin ψ). Both arcs are circles, so — like the ogive
// and the torus — the ball offset closes in R1 form: the CL rides the
// concentric arcs r + r_ball about O1 and r − r_ball about O2, meeting G1 at
// the junction normal (the ogive entry's "no arc shortcut for 5.2" warning
// inverts here on purpose: with no intrinsic meridian to be faithful to,
// *choosing* arcs is the honest minimal constructible — see the DECISIONS
// entry).
//
// The first mixed-curvature meridian: synclastic belly (convex both ways, a
// dome band), anticlastic neck (convex hoop, meridian concave toward the
// tool with constant radius r) — so the shared ⚠ gouge guard runs FIRST,
// against r, and it is the *neck arc alone* that produces the bound. With
// r_ball <= r both CL arcs are regular and the emitted locus is monotone —
// waist→equator it grows in rho and falls in z on both arcs (dz flips sign
// only past θ = 90°, which validate() excludes as the squat-vase overhang) —
// exactly generate_passes' feed-out-then-plunge shape, with the outward
// normal's radial component cos(angle) >= 0 everywhere (zero only
// instantaneously at a θ = 90° junction, the catenoid-waist boundary
// argument). At r_ball == r the neck CL arc collapses to the single point
// O2 — the ball nestles into the cove and one waist position touches the
// whole neck arc; duplicate rings are emitted harmlessly and the scallop
// march's clamp jumps to the junction.
//
// Emission starts at the waist ring (rho = c + r_ball, z = z + h — vertical
// wall, ball-equator contact, the catenoid waist's plunge-safety argument
// verbatim) and descends to the equator ring (a + r_ball, exactly z: the
// normal is horizontal there, so the last CL sits AT the base plane —
// assumes roughing cleared the surroundings to it). Waterline spacing is
// uniform meridian arc length — the total is closed-form, s = 2·r·θ — so
// rings are exactly stepover-uniform on the surface. Constant scallop is
// per-arc closed form, no Newton anywhere: the concave cusp triangle's angle
// step on the neck arc (derive_scallop_angle_concave(…, r)), the convex
// triangle's on the belly arc (derive_scallop_angle(…, r)); the junction
// ring is always emitted and the march restarts there with the other arc's
// triangle — conservative, never undersampled. Correctness anchors: the
// off-meridian residual tests below plus closed-form waist/junction/equator
// spot checks.

// ---- SplineRevolve: the analytic NURBS reader -------------------------------
// A surface of revolution whose meridian is an arbitrary (rational) b-spline
// (ROADMAP Phase 4 follow-up; DECISIONS.md "Analytic spline-of-revolution
// reader…"). The same pointwise-offset regime as the catenoid / pseudosphere /
// vase, with the analytic meridian generalized: instead of a cosh or a
// tractrix or an ogee arc, the contact point, tangent, and curvature come from
// de Boor evaluation of the spline (crate::nurbs). Everything else is the
// revolved-cap machinery unchanged — the CL point is the surface offset one
// ball radius along the exact meridian normal, revolved into a waterline circle
// by generate_passes.
//
// This is the analytic counterpart to Mesh (finish_mesh): where the mesh path
// tessellates a freeform revolved face and accepts the stair-step error, this
// reads the same face's b-spline meridian exactly and leaves no tessellation
// error — the difference the follow-up exists to make.
//
// Two spacings, both apex→base so generate_passes' apex-plunge-then-widening-
// rings shape applies directly. Waterline (stepover): passes at uniform
// meridian arc length (numerically integrated — the spline has no closed-form
// length). Constant scallop: the family Newton march
// (parse::derive_spline_revolve_capped_step_u), the ellipsoid/catenoid scheme
// generalized by POINTWISE curvature sign — a general meridian has no single
// convex-or-concave identity, so the cusp triangle is picked per march point
// from the exact signed curvature, with a flat band reducing to the cone's
// planar chord (which also carries the march smoothly across an inflection) —
// and then CAPPED by each pair's own exact tip price (DECISIONS.md
// "Family-wide march cap…"). The cap is what makes the stated scallop a
// promise on a user-authored meridian: the same across-step curvature drift
// that the mixed-meridians slice rejected to let price an allowance (+2.6% on
// the fixture cap at 0.2mm) is not fit to ship as a scallop either.
//
// Gouge guard: a general meridian can be concave toward the tool in stretches
// (like the vase neck), so the shared concave_gouge_check runs against the
// tightest concave radius of curvature found along the curve. The curvature at
// each sample is exact (the spline's second derivative); the scan is dense PER
// KNOT SPAN (nurbs::distinct_knots), so a many-knot meridian cannot hide a
// concave wiggle between samples the way a fixed whole-domain count could. A
// meridian convex everywhere (a plain dome) has no concave point and correctly
// skips the guard, exactly as the sphere/ogive noses do. The guard runs before
// the spacing branch, so scallop and stepover share it by construction. The
// scan is meridian-only ON PURPOSE: on the cap envelope (z' <= 0) the outward
// normal's radial component n_r = −z'/|C'| is >= 0, so the hoop direction is
// convex toward the tool everywhere and contributes no bound — the funnel op
// below, where n_r goes negative, is where the hoop scan lives.
fn finish_spline_revolve(p: &Part, op: &Operation, feature: &Feature) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_ball = tool.dia / 2.0;

    let (meridian, cx, cy, z0) = match feature {
        Feature::SplineRevolve { meridian, x, y, z, .. } => (meridian, *x, *y, *z),
        _ => unreachable!(),
    };
    let (u0, u1) = meridian.domain();

    // Outward tool normal at parameter u: the meridian tangent rotated so it
    // points away from the material, N_z >= 0 (the cap is machined from above).
    // The offset itself lives in parse::spline_revolve_cl_point (one home, the
    // scallop march Newtons on the same formula); this closure just adds the
    // feature's base z and swaps into the (z_cl, rho_cl) order generate_passes
    // wants. `skin` is the roughing nominal offset a' (0.0 for a plain
    // finishing pass — byte-identical, x + 0.0 == x): the roughing CL is the
    // same pointwise normal offset at distance r_ball + a'.
    let cl_at = |u: f64, skin: f64| -> (f64, f64) {
        let (rho, z_rel) = crate::parse::spline_revolve_cl_point(meridian, r_ball + skin, u);
        (z0 + z_rel, rho)
    };

    // Gouge guard: scan curvature over the meridian; the tightest concave-
    // toward-tool radius (if any) is the bound the shared check compares the
    // ball radius against. Per knot span (see the header note); the sign
    // convention lives in nurbs::curvature_toward_up_normal (positive =
    // concave toward the tool). Convex-everywhere meridians produce no
    // candidate and skip the check (a stated fact, not a gap — see the guard
    // roster above).
    const GUARD_SAMPLES_PER_SPAN: usize = 100;
    let mut tightest_concave: Option<f64> = None;
    for pair in meridian.distinct_knots().windows(2) {
        for k in 0..=GUARD_SAMPLES_PER_SPAN {
            let u = pair[0] + (pair[1] - pair[0]) * (k as f64 / GUARD_SAMPLES_PER_SPAN as f64);
            let Some(toward_tool) = meridian.curvature_toward_up_normal(u) else {
                continue;
            };
            if toward_tool > 1e-12 {
                let radius = 1.0 / toward_tool;
                tightest_concave = Some(tightest_concave.map_or(radius, |m: f64| m.min(radius)));
            }
        }
    }
    if let Some(radius) = tightest_concave {
        concave_gouge_check(
            &op.feature_name,
            op.tool_num,
            r_ball,
            radius,
            &format!(
                "the spline meridian is concave toward the tool over part of its span, tightest radius of curvature {:.3}mm (from the curve's exact second derivative)",
                radius
            ),
        )?;
    }

    let mut passes = Vec::new();
    let title: String;

    // stock_to_leave (DECISIONS.md "stock_to_leave mixed meridians…"): the
    // spline is the one carrier whose meridian is user-authored, so neither
    // spacing gets to trust an osculating estimate under stock — BOTH modes
    // verify the actually-emitted pass stack by the exact tip construction
    // (parse::derive_spline_march_cusp) against the numerically-projected
    // nominal distance, and the ceiling guarantee rests on that walk. The
    // gouge guard's _with_stock form runs at the emitted skin against the
    // same scanned tightest radius (the nominal check above stays first —
    // byte-identical errors for a plain finishing pass).
    if let Some(scallop) = op.scallop {
        // scallop_height <= r_ball is the physical ceiling for the cusp-height
        // formula (2*r_ball*h - h*h stays real and the cusp can't exceed the
        // ball's own radius) — a request above it is a hard error, not a
        // silent clamp, the cone's rule verbatim.
        if scallop > r_ball {
            return Err(format!(
                "SplineRevolve feature '{}' scallop height ({:.3}mm) exceeds tool T{}'s ball radius ({:.3}mm); reduce scallop or use a larger tool",
                op.feature_name, scallop, op.tool_num, r_ball
            ));
        }
        // The family march, ellipsoid loop shape: emit, step, clamp the final
        // pass to the base rim. The forward-progress guard also absorbs a
        // stalled or NaN step (pathological input) and the degenerate
        // zero-chord step at an exact r_ball == concave-radius fit (allowed by
        // the guard as point contact — the catenoid-waist precedent): both
        // snap to u1 so the loop terminates instead of spinning.
        let march_us = |s: f64| -> Vec<f64> {
            let mut us = Vec::new();
            let mut u = u0;
            loop {
                us.push(u);
                if u >= u1 {
                    break;
                }
                let u_next = crate::parse::derive_spline_revolve_capped_step_u(scallop, r_ball, meridian, u, s);
                u = if u_next > u && u_next <= u1 { u_next } else { u1 };
            }
            us
        };
        // Under stock the march's osculating triangle is only the SPACING
        // heuristic, never the budget: on a user-authored meridian the
        // across-step curvature drift can push a pair's true cusp past the
        // scallop, and charging the drift against a scallop-sized budget
        // would reject every allowance (the overshoot is allowance-
        // independent — the knife-edge the cone dodged by re-gridding). So
        // scallop mode prices exactly like stepover mode: the budget h is
        // the exact tip walk over the march's own emitted grid, swept over
        // 5 skins to choose a', then VERIFIED at the emitted skin — cusp
        // budget = the verified price (≈ scallop plus the drift), not the
        // scallop by fiat.
        let (skin, cusp) = if let Some(allowance) = op.stock_to_leave {
            sweep_verify_budget(
                allowance,
                &|s| crate::parse::derive_spline_march_cusp(meridian, r_ball, s, &march_us(s)),
                &|| format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm scallop march leaves adjacent passes that do not overlap — an uncut ridge remains, which no allowance can ceiling — reduce scallop or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, scallop
                ),
                &|worst| format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm scallop march leaves a worst-case verified cusp of {:.3}mm between adjacent passes, which meets or exceeds the allowance ceiling (cusps count against it — max, not min) — reduce scallop or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, scallop, worst
                ),
            )?
        } else {
            (0.0, 0.0)
        };
        if let Some(radius) = tightest_concave {
            if skin > 0.0 {
                concave_gouge_check_with_stock(
                    &op.feature_name,
                    op.tool_num,
                    r_ball,
                    skin,
                    radius,
                    &format!(
                        "the spline meridian is concave toward the tool over part of its span, tightest radius of curvature {:.3}mm (from the curve's exact second derivative)",
                        radius
                    ),
                )?;
            }
        }
        let emitted = march_us(skin);
        // The plain finishing pass's own verify (DECISIONS.md "Family-wide
        // march cap…"), the tangent-lip funnel's arm: every step the cap
        // returns prices at or under the requested scallop, so only the
        // forward-progress clamp above can emit a pair the cap never
        // approved. Unreachable — the pair price falls continuously to zero
        // as the step closes, on either curvature sign — but the whole point
        // of this slice is that a stated scallop is a promise, so the promise
        // gets a structural check and not just a test.
        if op.stock_to_leave.is_none() {
            let worst = crate::parse::derive_spline_march_cusp(meridian, r_ball, 0.0, &emitted);
            if worst > scallop + 1e-9 {
                return Err(format!(
                    "feature '{}' cannot be finished to a {:.3}mm scallop with T{}: the emitted pass stack prices a worst-case cusp of {:.3}mm, because the march's forward-progress clamp landed a pass pair the tip cap never approved — reduce the tool diameter or increase the scallop",
                    op.feature_name, scallop, op.tool_num, worst
                ));
            }
        }
        for u in emitted {
            passes.push(cl_at(u, skin));
        }
        title = format!(
            "(--- copy surface: {} with T{} (ball-nose constant scallop, pointwise spline offset, analytic surface of revolution){} ---)",
            op.feature_name, op.tool_num, stock_suffix(op, cusp)
        );
    } else {
        // Uniform meridian arc length, apex -> base (parse::spline_uniform_arc_us,
        // the one home shared with the funnel AND the stack pricer). n passes
        // at <= stepover, measured along the surface the pass actually cuts
        // (the a'-inflated meridian under stock).
        let stepover = op.stepover.expect("Finish on a SplineRevolve requires a stepover or scallop");
        // Sweep-then-VERIFY (the cone/ogive scheme — parse::derive_cone_stack_cusp's
        // pricing story): the exact per-pair tip price has no monotonicity
        // theorem in the skin on a piecewise-polynomial meridian, so the
        // budget is the max over a 5-point skin sweep (choosing a') and the
        // guarantee is the verification walk at the emitted skin itself.
        // One home, shared with the stock model (DECISIONS.md → "the roughing
        // allowance has two arms…", fork 3A).
        let (skin, cusp) = stepover_budget(feature, tool, op, stepover)?;
        if let Some(radius) = tightest_concave {
            if skin > 0.0 {
                concave_gouge_check_with_stock(
                    &op.feature_name,
                    op.tool_num,
                    r_ball,
                    skin,
                    radius,
                    &format!(
                        "the spline meridian is concave toward the tool over part of its span, tightest radius of curvature {:.3}mm (from the curve's exact second derivative)",
                        radius
                    ),
                )?;
            }
        }
        for u in crate::parse::spline_uniform_arc_us(meridian, stepover, skin, false) {
            passes.push(cl_at(u, skin));
        }

        title = format!(
            "(--- copy surface: {} with T{} (ball-nose waterline, pointwise spline offset, analytic surface of revolution){} ---)",
            op.feature_name, op.tool_num, stock_suffix(op, cusp)
        );
    }
    Ok(generate_passes(p, op, title, cx, cy, passes))
}

// ---- SplineRevolveFunnel: the inside orientation ----------------------------
// The spline surface of revolution machined from the void side (CHECKLIST row
// 5.3's last member; DECISIONS.md "Spline funnel…"), completing the
// both-orientations pattern the catenoid and pseudosphere pairs set. The
// meridian is stored MOUTH-FIRST (mouth rim widest, at the top — the mirror of
// the cap's apex-first envelope, nurbs::check_machinable_funnel), the mouth
// circle sits in the stock-top plane, and depth is read off the meridian's own
// z extent: world_z = stock.z + (meridian_z − z_mouth).
//
// The offset, curvature, and scallop march are the cap's machinery with the
// rotation sense flipped — one home per orientation, not a shared flip. On a
// mouth-first meridian the cap's nz >= 0 rule lands on the correct
// up-and-inward into-void normal wherever r' < 0, but on a vertical wall
// stretch (r' == 0, legal in both envelopes — a counterbore-like cavity, or
// the hemisphere bowl's mouth) the tie-break resolves outward, the cap's
// answer and a cavity gouge; the curvature flip (`if r' < 0`) misses the
// same case. So the funnel reads its own pair —
// parse::spline_revolve_funnel_cl_point ((z', −r')/|C'|, exact on the whole
// envelope) and nurbs::curvature_toward_void_normal (plain signed curvature
// negated, no tie-break) — and the march
// derive_spline_revolve_funnel_capped_step_u is the shared worker handed that
// pair, tip-capped like the cap's (the Catenoid/CatenoidThroat pattern). The
// DECISIONS entry records why the sides can't share a flip.
//
// Gouge guard — the first TWO-principal-direction scan (see the roster above).
// Meridian: concave-toward-tool stretches (a bowl bottom) bound the ball
// exactly as on the cap; this is also the offset-regularity condition — under
// it the CL derivative factor (1 − r_ball·κ) stays non-negative, so rho_cl
// and z_cl are both non-increasing mouth→tip and the locus descends and
// narrows monotonically. Hoop: wherever the wall leans inward the into-void
// normal's radial component n_r goes negative and the parallel circle curls
// around the tool with normal-section radius rho/|n_r| (Meusnier) — the
// CL-ring-not-through-the-axis condition (rho_cl = rho − r_ball·|n_r| >= 0
// exactly at guard equality), the throat/funnel consumers' bound generalized
// pointwise. Tightest over both directions wins, and the error names which
// direction bound.
//
// Tip semantics, both legal, stated not hidden. An OPEN tip (r_tip > 0): the
// ball's below-tip cap dips under the tip circle, so the op assumes a tip
// relief below it is already opened — the throat's through-bore / the
// tractrix funnel's tip-relief assumption verbatim. A CLOSED bowl bottom
// (r_tip = 0 with horizontal end tangent): the final CL ring's radius
// collapses and generate_passes' sub-tolerance-ring skip absorbs it (the
// bowl-vertex precedent); the hoop radius rho/|n_r| limits to the finite
// meridian radius at the axis umbilic, which the meridian scan already
// covers. A conical V-pit (r_tip = 0 with a tilted end tangent) needs no
// special case: rho/|n_r| → 0 at the tip, so the hoop guard rejects every
// ball — correct physics, a ball-nose cannot finish a V-pit's bottom point.
//
// Mouth plunge safety, the tractrix funnel's lip argument generalized to any
// machinable meridian: the first CL point sits at rho_cl = r_mouth + r_ball·n_r
// (n_r <= 0) and z = z_top + r_ball·n_z, and the ball's below-stock-top cap is
// a disc of radius r_ball·|n_r| about the centre — outermost reach
// rho_cl + r_ball·|n_r| = r_mouth, exactly tangent to the lip circle. The
// initial plunge is clear and the first ring finishes the lip, for every
// meridian the funnel envelope admits.
//
// Both spacings march mouth→tip in increasing u, which for a cavity IS the
// top-first emission order — no reversal (the tractrix funnel's precedent;
// the outside band and the throat both reverse — the split-order question
// stays stated per-op). Waterline spacing shares spline_uniform_arc_us with
// the cap; constant scallop shares the family Newton march. No sphere limit
// is machinable as a cap here — correctness is pinned by the concentric
// INNER-sphere reduction (a rational quarter-circle bowl meridian must ride
// the sphere of radius R − r_ball) plus the direct anchors in the tests.
fn finish_spline_revolve_funnel(p: &Part, op: &Operation, feature: &Feature) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_ball = tool.dia / 2.0;

    let (meridian, cx, cy) = match feature {
        Feature::SplineRevolveFunnel { meridian, x, y, .. } => (meridian, *x, *y),
        _ => unreachable!(),
    };
    let (u0, u1) = meridian.domain();
    // Mouth in the stock-top plane: only the meridian's z *differences* carry
    // meaning, the mouth pole's z maps onto stock.z.
    let z_mouth = meridian.eval(u0).1;
    let z_shift = p.stock.z - z_mouth;

    // `skin` is the roughing nominal offset a' (0.0 for a plain finishing
    // pass — byte-identical, x + 0.0 == x): the roughing CL is the same
    // pointwise into-void offset at distance r_ball + a', the cap's rule on
    // the funnel's own normal home.
    let cl_at = |u: f64, skin: f64| -> (f64, f64) {
        let (rho, z_rel) = crate::parse::spline_revolve_funnel_cl_point(meridian, r_ball + skin, u);
        (z_shift + z_rel, rho)
    };

    // The two-direction guard scan (see the header and the roster). Same
    // per-knot-span density as the cap; the hoop candidate recomputes the
    // into-void normal exactly as spline_revolve_funnel_cl_point does, so the
    // bound and the offset can't disagree about which side the material is on.
    const GUARD_SAMPLES_PER_SPAN: usize = 100;
    let mut tightest: Option<(f64, bool)> = None; // (radius, binding_is_hoop)
    for pair in meridian.distinct_knots().windows(2) {
        for k in 0..=GUARD_SAMPLES_PER_SPAN {
            let u = pair[0] + (pair[1] - pair[0]) * (k as f64 / GUARD_SAMPLES_PER_SPAN as f64);
            if let Some(toward_tool) = meridian.curvature_toward_void_normal(u) {
                if toward_tool > 1e-12 {
                    let radius = 1.0 / toward_tool;
                    if tightest.map_or(true, |(m, _)| radius < m) {
                        tightest = Some((radius, false));
                    }
                }
            }
            let ((rho, _), (dr, dz)) = meridian.eval_d1(u);
            let speed = (dr * dr + dz * dz).sqrt();
            if speed < 1e-9 {
                continue;
            }
            // The into-void normal's radial component (the funnel rotation:
            // n_r = z'/|C'|; on a vertical wall stretch this is −1 and the
            // bound below is exactly the bore ring's rho).
            let nr = dz / speed;
            if nr < -1e-9 {
                let radius = rho / (-nr);
                if tightest.map_or(true, |(m, _)| radius < m) {
                    tightest = Some((radius, true));
                }
            }
        }
    }
    let guard_detail = |radius: f64, binding_is_hoop: bool| -> String {
        if binding_is_hoop {
            format!(
                "the funnel's hoop direction tightens to a {:.3}mm concave normal-section radius (rho/|n_r| from the exact meridian normal — the wall curls around the tool, tightest where the cavity is narrow and steep)",
                radius
            )
        } else {
            format!(
                "the funnel meridian is concave toward the tool over part of its span, tightest radius of curvature {:.3}mm (from the curve's exact second derivative)",
                radius
            )
        }
    };
    if let Some((radius, binding_is_hoop)) = tightest {
        concave_gouge_check(&op.feature_name, op.tool_num, r_ball, radius, &guard_detail(radius, binding_is_hoop))?;
    }

    let mut passes = Vec::new();
    let title: String;

    // stock_to_leave (DECISIONS.md "stock_to_leave inside funnels…"): the
    // cap's scheme on the void side — BOTH spacings price the actually-
    // emitted stack by the exact tip walk (parse::derive_spline_funnel_march_cusp,
    // the funnel-CL twin) under sweep-then-VERIFY, and the two-direction
    // guard scan reruns _with_stock at the emitted skin: whichever direction
    // bound tightest at nominal is the same bound the fatter roughing offset
    // must clear, hoop and meridian alike (the scanned radius is the
    // inflated-locus regularity/ring bound in both directions).
    if let Some(scallop) = op.scallop {
        // Same physical ceiling and same forward-progress clamp as the cap —
        // see finish_spline_revolve's scallop branch.
        if scallop > r_ball {
            return Err(format!(
                "SplineRevolveFunnel feature '{}' scallop height ({:.3}mm) exceeds tool T{}'s ball radius ({:.3}mm); reduce scallop or use a larger tool",
                op.feature_name, scallop, op.tool_num, r_ball
            ));
        }
        let march_us = |s: f64| -> Vec<f64> {
            let mut us = Vec::new();
            let mut u = u0;
            loop {
                us.push(u);
                if u >= u1 {
                    break;
                }
                let u_next = crate::parse::derive_spline_revolve_funnel_capped_step_u(scallop, r_ball, meridian, u, s);
                u = if u_next > u && u_next <= u1 { u_next } else { u1 };
            }
            us
        };
        let (skin, cusp) = if let Some(allowance) = op.stock_to_leave {
            sweep_verify_budget(
                allowance,
                &|s| crate::parse::derive_spline_funnel_march_cusp(meridian, r_ball, s, &march_us(s)),
                &|| format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm scallop march leaves adjacent passes that do not overlap — an uncut ridge remains, which no allowance can ceiling — reduce scallop or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, scallop
                ),
                &|worst| format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm scallop march leaves a worst-case verified cusp of {:.3}mm between adjacent passes, which meets or exceeds the allowance ceiling (cusps count against it — max, not min) — reduce scallop or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, scallop, worst
                ),
            )?
        } else {
            (0.0, 0.0)
        };
        if let Some((radius, binding_is_hoop)) = tightest {
            if skin > 0.0 {
                concave_gouge_check_with_stock(&op.feature_name, op.tool_num, r_ball, skin, radius, &guard_detail(radius, binding_is_hoop))?;
            }
        }
        let emitted = march_us(skin);
        // The finishing-side verify, the cap's arm on the void side (see
        // finish_spline_revolve's for the argument).
        if op.stock_to_leave.is_none() {
            let worst = crate::parse::derive_spline_funnel_march_cusp(meridian, r_ball, 0.0, &emitted);
            if worst > scallop + 1e-9 {
                return Err(format!(
                    "feature '{}' cannot be finished to a {:.3}mm scallop with T{}: the emitted pass stack prices a worst-case cusp of {:.3}mm, because the march's forward-progress clamp landed a pass pair the tip cap never approved — reduce the tool diameter or increase the scallop",
                    op.feature_name, scallop, op.tool_num, worst
                ));
            }
        }
        for u in emitted {
            passes.push(cl_at(u, skin));
        }
        title = format!(
            "(--- copy surface: {} with T{} (ball-nose constant scallop, pointwise spline offset, analytic funnel of revolution){} ---)",
            op.feature_name, op.tool_num, stock_suffix(op, cusp)
        );
    } else {
        let stepover = op.stepover.expect("Finish on a SplineRevolveFunnel requires a stepover or scallop");
        // One home, shared with the stock model (DECISIONS.md → "the roughing
        // allowance has two arms…", fork 3A).
        let (skin, cusp) = stepover_budget(feature, tool, op, stepover)?;
        if let Some((radius, binding_is_hoop)) = tightest {
            if skin > 0.0 {
                concave_gouge_check_with_stock(&op.feature_name, op.tool_num, r_ball, skin, radius, &guard_detail(radius, binding_is_hoop))?;
            }
        }
        for u in crate::parse::spline_uniform_arc_us(meridian, stepover, skin, true) {
            passes.push(cl_at(u, skin));
        }
        title = format!(
            "(--- copy surface: {} with T{} (ball-nose waterline, pointwise spline offset, analytic funnel of revolution){} ---)",
            op.feature_name, op.tool_num, stock_suffix(op, cusp)
        );
    }

    // Mouth→tip in increasing u IS top-first for a cavity — no reversal
    // (see the header).

    Ok(generate_passes(p, op, title, cx, cy, passes))
}

// ---- Pseudosphere (row 5.3, second member): the forced band -----------------
// The pseudosphere band (CHECKLIST row 5.3), third of TAXONOMY §5's analytic
// surfaces of revolution and the second genuinely-R2 one: the tractrix
// meridian is not an arc, so as with the catenoid nothing closes in R1 form —
// pointwise offset plus Newton scallop march. Meridian parametrized by u,
// growing from the cusp rim toward the tip: contact
// (rho, z) = (c·sech u, z_cusp + c·(u − tanh u)), band from
// u_b = acosh(c/r_base) at the authored base circle (in the z plane, so
// z_cusp = z − c·(u_b − tanh u_b) sits below it) to u_t = acosh(c/r_tip).
//
// The meridian tangent going up is (−sech u, tanh u) — unit length already,
// by sech²u + tanh²u = 1, this surface's analogue of the catenoid's cosh
// identity — so the outward normal is (tanh u, sech u) and every line here
// stays closed-form:
//   rho_cl = c·sech u + r_ball·tanh u
//   z_cl   = z_cusp + c·(u − tanh u) + r_ball·sech u
//
// The second *anticlastic* surface, and the extreme case: constant negative
// Gaussian curvature K = −1/c² everywhere (meridian radius c·sinh u, concave
// toward the tool, times hoop radius c/sinh u, convex — always c², opposite
// signs). Unlike the catenoid the tight end is the BASE circle: c·sinh u is
// smallest at u_b — and vanishes at the cusp rim itself, which is why
// validate() forces the band to truncate above it (no ball of any radius can
// machine the flare's last sliver; see the variant's doc comment). So the
// shared ⚠ gouge guard runs FIRST, against c·sinh(u_b). As with the bowl and
// the catenoid the guard is load-bearing twice: with r_ball <= c·sinh u,
//   drho_cl/du = sech²u·(r_ball − c·sinh u) <= 0,   dz_cl/du = tanh u·sech u·(c·sinh u − r_ball) >= 0
// — both derivatives flip sign exactly at guard equality, so on a guarded
// band the offset locus is regular and passes ordered tip→base grow in rho
// and fall in z monotonically (at the r_ball == c·sinh(u_b) boundary the base
// derivative vanishes only instantaneously; successive emitted passes still
// descend). That is the shape generate_passes' feed-out-then-plunge
// transition needs, by the same argument as the convex domes: the outward
// normal's radial component (tanh u) is positive everywhere on the band, so
// moving radially outward from a CL point at fixed z only gains clearance.
//
// Emission starts at the tip ring. The initial plunge to it is safe: the tip
// CL radius is the band's smallest, and descending at that radius the nearest
// material is the tip rim circle itself — the ball's distance to it reaches
// exactly r_ball precisely at the final plunge depth z_cl(u_t) (horizontal
// offset r_ball·tanh u_t, vertical r_ball·sech u_t, and tanh² + sech² = 1
// again) and is strictly greater the whole way down. Both band ends carry the
// usual rim assumption, stated not hidden: the normal is tilted everywhere
// (the meridian is never vertical or horizontal on the band), so the tip CL
// sits above the tip plane — assumes anything above it is already cleared —
// and the base CL sits beyond r_base and above the base plane — assumes
// roughing cleared the surroundings to the base plane, like the paraboloid
// rim. What the tractrix continues into below base_dia (the flare toward the
// cusp) is not this feature's declared surface.
//
// Waterline spacing exploits this row's tidy bonus: the tractrix arc length
// is closed-form, s(u) = c·ln(cosh u), so the band's total is
// c·ln(r_base/r_tip) (cosh u = c/rho) and stepover-uniform rings invert in
// closed form, u_i = acosh(cosh u_b · e^{s_i/c}) — equivalently a geometric
// progression in rho. Exact uniform on-surface spacing, no conservative
// bound needed (the analogue of the catenoid's z-linear-in-w bonus).
// Constant scallop marches parse::derive_pseudosphere_step_u (the catenoid's
// Newton scheme with u for w). Both spacings march base→tip — the
// tight-end-first order that keeps the concave cusp triangle conservative —
// while emission is top-first like every finishing op, so the passes are
// reversed once at the end: the first op where the two orders differ. No
// sphere limit exists to anchor against, so correctness is pinned by the
// direct off-surface residual tests below, plus the closed-form tip/base
// spot checks.
//
// stock_to_leave (DECISIONS.md "stock_to_leave anticlastic bands…"): the
// bowl's concave recipe on this band, same as the catenoid neck. The three
// substitutions stay distinct (CL locus at r_ball + a'; gouge guard compares
// r_ball + a'; cusp triangle prices the physical ball on the a'-tightened
// meridian r_part − a'), and the guard bound doubles as the INFLATED locus's
// regularity bound with no new derivation: both CL derivatives are signed by
// c·sinh u − r_off (dz_cl/du = tanh u·sech u·(c·sinh u − r_off), drho_cl/du =
// −sech²u·(c·sinh u − r_off)), one-signed on the whole band exactly when
// r_off = r_ball + a' ≤ c·sinh(u_b) — precisely what
// concave_gouge_check_with_stock enforces against the base circle's radius.

// ---- Pseudosphere funnel (row 5.3, inside orientation) ----------------------
// The tractricoid's tractrix meridian machined from the void side: a top-open
// cavity whose mouth circle (radius r_m = mouth_dia/2, in the stock-top
// plane) narrows down the tractrix toward the axis to the tip circle
// (radius r_t = tip_dia/2) at the derived depth — the same meridian, u(rho),
// and derive_pseudosphere_height as the outside band, one home each. Contact
// (rho, z) = (c·sech u, z_top − d(u)) with d(u) = c·[(u − tanh u) −
// (u_b − tanh u_b)], u_b = acosh(c/r_m) at the mouth, u_t = acosh(c/r_t) at
// the tip. The tangent going down is −(sech u, tanh u) — unit by
// sech²u + tanh²u = 1 — so the into-void normal is (−tanh u, sech u) and
//   rho_cl = c·sech u − r_ball·tanh u   (= sech u·(c − r_ball·sinh u))
//   z_cl   = z_top − d(u) + r_ball·sech u
//
// From the void side the anticlastic roles swap (the throat's move, on the
// non-minimal surface): the meridian turns convex toward the tool, and the
// concave direction is the HOOP — normal-section radius c/sinh u, which
// tightens with DEPTH toward the tip. So the shared ⚠ gouge guard runs
// FIRST, against c/sinh(u_t) at the tip circle — and the contrast with the
// catenoid pair is the point worth naming: the pseudosphere is not a minimal
// surface, so orientation moves both the concave direction and the binding
// end (outside: meridian c·sinh u, tightest at the BASE; inside: hoop
// c/sinh u, tightest at the TIP). The catenoid's same-bound-either-side
// guard was that surface's privilege, not the tier's.
//
// Like the throat, the guard is NOT the offset-regularity condition: both CL
// derivatives are one-signed for every ball radius —
//   drho_cl/du = −sech u·(c·tanh u + r_ball·sech u) < 0,   dz_cl/du = −tanh u·(c·tanh u + r_ball·sech u) < 0
// — the locus descends and narrows unconditionally; what r_ball <= c/sinh(u_t)
// protects is the tip CL ring itself, whose radius sech u·(c − r_ball·sinh u)
// hits zero exactly at guard equality (the cylinder-bore collapse shape). At
// the boundary the tip ring degenerates to the on-axis point and
// generate_passes' sub-tolerance-ring skip absorbs it, like the bowl vertex.
//
// Two boundary facts fall out of the same identity and are stated, not
// hidden. At the MOUTH: the first CL point sits at z_top + r_ball·sech u_b,
// above the stock top, and the ball's below-top-plane cap spans exactly
// rho <= r_m — tangent to the lip circle (the contact point itself, which
// lies IN the stock-top plane by construction) and strictly inside the mouth
// below it, so the initial plunge is clear and the first ring finishes the
// lip. At the TIP: the ball's below-tip-plane cap likewise spans exactly
// rho <= r_t, tangent at the tip circle, dipping r_ball·(1 − sech u_t) below
// the tip plane — so the op assumes a tip_dia relief below the tip circle is
// already opened (a tip_dia drill to at least that dip suffices; the
// undeclared tractrix continuation below the tip lies inside that relief).
// The mirror of the throat's through-bore assumption.
//
// TANGENT-LIP form (mouth_dia == dia, DECISIONS.md "Tangent-lip funnel…"):
// the mouth IS the cusp circle, u_b = 0, and every mouth formula above
// specializes continuously (sech u_b = 1, tanh u_b = 0): the first CL point
// is (c, z_top + r_ball) — the ball resting ON the lip circle, tangent to
// the stock top plane from above, its below-top cap shrunk to the single
// tangency point — so the first ring grazes the lip without cutting the top
// face, exactly the blend the form names. The initial plunge is trivially
// clear (the ball stays above the stock top). The scallop march's first step
// carried TWO problems here, and only the shallower one was seen at first:
// the parameter speed c·tanh u vanishes at u = 0, so the linearized Newton
// seed divides by zero (treated in derive_pseudosphere_funnel_step_u by
// seeding with the closed-form arc-length inversion instead). The deeper one
// is that the cusp triangle itself misreads the lip — an x^{3/2} cusp is
// infinitely curved yet leaves horizontally, and the triangle's corner form
// at R → 0 assumes a surface that drops away on both sides — so the seeded
// step converged nicely onto a chord ~12× too long. That is the tip cap's
// job, above. The waterline inversion below is exact at u_b = 0 as it stands.
//
// Waterline spacing reuses the outside band's tidy bonus verbatim: tractrix
// arc length is closed-form, total c·ln(r_m/r_t), so stepover-uniform rings
// invert exactly (u_i = acosh(cosh u_b · e^{s_i/c}), a geometric progression
// in rho). Constant scallop marches
// parse::derive_pseudosphere_funnel_capped_step_u — the convex cusp triangle
// as a seed, capped by each pair's own exact tip price, because the triangle
// alone reads the local radius at ONE point and overshoots the true cusp
// (~2.6% on a truncated mouth, ~12× at the tangent lip; DECISIONS.md
// "Tangent-lip march fix…"). Both spacings march mouth→tip in increasing u, which for a cavity
// IS the top-first emission order — no reversal, unlike both the outside
// band and the throat: the split-order question resolves differently on
// each of the four §5 ops, so it stays stated per-op rather than assumed.
// No sphere limit exists; correctness is pinned by the direct off-surface
// residual tests below, plus the closed-form mouth/tip spot checks.

// ---- Custom tool profile (FormTool) finishing -------------------------------
// A `tool.profile` (SVG-derived (radius, height-above-tip) points, ROADMAP
// Phase 3 "Custom tool/cutter profiles from SVG") generalizes the ball-nose
// offset above: instead of a single constant radius, the *contact point
// migrates* along the tool's own profile as the local part-surface slope
// changes (TAXONOMY.md "Tool-shape caveat on every closed-form claim" -- this
// is exactly the "small root-find for where on the tool edge contact occurs"
// it warns about). For a piecewise-*linear* profile that root-find collapses
// to a finite lookup rather than a numeric solve: each profile segment has a
// constant tangent angle, so as the desired part-normal angle phi sweeps from
// 0 (pole) to pi/2 (equator), contact is pinned to whichever profile *vertex*
// bracket phi falls into -- the same "sharp corner follows a curved wall"
// contact as a flat-bottom or bull-nose tool's outer edge.
//
// Derivation (verified against finish_sphere's closed-form ball case, which
// is the limit of this with infinitely many segments):
//   - Profile points are (r_t, z_t), tip-referenced (r_t=0, z_t=0 at the
//     point of minimum radius -- parse.rs's SVG normalization).
//   - A segment's tangent (dr, dz) has angle psi = atan2(dz, dr). The part's
//     own contact-point normal angle phi (0 = pole, pi/2 = equator, same
//     convention as finish_sphere) must equal a *matching* profile-segment
//     psi for the two surfaces to be tangent there (not merely touching) --
//     this is the standard envelope/rolling-contact condition, and psi = phi
//     at match (algebra: local tool normal transformed into world radial
//     coordinates is antiparallel to the part's outward normal iff the
//     tangent angles agree).
//   - Vertex k (between segment k-1 and segment k) is the active contact for
//     every phi strictly between psi[k-1] and psi[k] (the segment normal
//     angles bracketing it) -- a straight tool segment can only be tangent
//     to a curved part at one point, so for phi values the profile has no
//     segment angled at exactly, contact must sit at a vertex instead.
//   - Given the active vertex (r_v, z_v), the tool's tip-reference position
//     is simply the part contact point shifted by that vertex's own local
//     offset: rho_c = rho_p(phi) + r_v, z_c = z_p(phi) - z_v. (Sanity check:
//     for the ball-nose's quarter-circle profile this reduces exactly to
//     finish_sphere's r_off/dz formulas, offset by r_ball because that
//     function tracks the ball *center*, not the tip.)
//
// Gouge safety requires the profile be a simple outward flare from the tip:
// both r_t and the segment angle psi must be non-decreasing along the
// profile (validated below) -- a concave dip would let contact jump to an
// unintended point and cut too deep, silently. This is the same
// "unmachinable feature is a hard error" rule as the torus bore/cone scallop
// checks above, just applied to the tool instead of the part.
//
// Scope of this version: Sphere only (the one surface whose contact angle is
// globally 1:1, with no apex-vertex or inner-bore complication of its own --
// see finish_cone/finish_torus), and `stepover` only (a constant-scallop
// formula for an arbitrary profile isn't derived yet). Cone, Torus, and
// profile scallop are a documented gap (DECISIONS.md "Custom tool-profile
// finishing..."), not a silent one -- validate() rejects them by name.
fn finish_sphere_profile(p: &Part, op: &Operation, feature: &Feature, tool: &Tool) -> Result<Vec<String>, String> {
    let profile = tool.profile.as_ref().expect("caller checked tool.profile is Some");
    let (cx, cy, cz, r_part) = match feature {
        Feature::Sphere { dia, x, y, z } => (*x, *y, *z, dia / 2.0),
        _ => unreachable!("caller matched Sphere"),
    };

    // validate() already rejects op.scallop for a profile tool; stepover is
    // the only supported spacing here.
    let stepover = op.stepover.expect("Finish with a custom tool profile requires a stepover");

    if profile.len() < 2 {
        return Err(format!(
            "tool T{}'s profile has fewer than two points; a custom tool profile needs at least a tip and one more point to define a contact angle",
            op.tool_num
        ));
    }
    if profile[0].x.abs() > 1e-6 {
        return Err(format!(
            "tool T{}'s profile does not reach the tool axis (starts at radius {:.3}mm, not 0); a dome's pole can only be finished by a profile that comes to a point on-axis",
            op.tool_num, profile[0].x
        ));
    }

    for w in profile.windows(2) {
        if w[1].x - w[0].x < -1e-9 {
            return Err(format!(
                "tool T{}'s profile radius decreases from {:.3}mm to {:.3}mm; a custom tool profile must flare outward monotonically from the tip to be used for surface finishing",
                op.tool_num, w[0].x, w[1].x
            ));
        }
    }
    // psis[i] is the tangent angle of segment profile[i] -> profile[i+1] —
    // `parse::profile_segment_angles`, the home the stock model's tube reads
    // too (Phase 10 lane B, slice 1): the rings below, the brackets, and
    // this table are one derivation in one place.
    let psis = crate::parse::profile_segment_angles(profile);
    for w in psis.windows(2) {
        if w[1] < w[0] - 1e-9 {
            return Err(format!(
                "tool T{}'s profile is not outward-convex (its normal angle decreases along the profile); a concave dip could let contact migrate to an unintended point and gouge the part",
                op.tool_num
            ));
        }
    }
    let psi_max = *psis.last().expect("profile.len() >= 2, checked above");
    if psi_max < PI / 2.0 - 1e-9 {
        return Err(format!(
            "tool T{}'s profile only reaches a {:.1} deg normal angle; finishing a sphere's dome out to its equator needs a profile that reaches 90 deg (a vertical wall segment)",
            op.tool_num, psi_max.to_degrees()
        ));
    }

    // Bracket boundaries: vertex 0 (the tip) is active on [0, psis[0]);
    // vertex k+1 is active on [psis[k], psis[k+1]), extended to pi/2 at the
    // end (reachability already checked above). Entries beyond pi/2 are left
    // in rather than filtered here -- the phi_end<=phi_start check in the
    // pass loop below already discards every one of them as a zero-width
    // bracket, so a second filter here would just be the same guard twice.
    let breaks = crate::parse::sphere_profile_breaks(profile);

    // The declared cutting band gates contact (K2 slice 2, fork 3A): the
    // vertices this pass actually rides — every bracket of non-zero width —
    // must sit inside [cut_bottom, cut_top], because outside the band the
    // drawn surface is relieved and a relieved surface gauging the part is
    // the standing form-tool gripe. The error carries the shortfall as a
    // number (the deviation ethos); an undeclared band is the whole drawing,
    // which is today's behavior said out loud.
    let (mut used_lo, mut used_hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for (i, &(phi_start, vidx)) in breaks.iter().enumerate() {
        let phi_end = breaks.get(i + 1).map(|&(p, _)| p).unwrap_or(PI / 2.0).min(PI / 2.0);
        if phi_end <= phi_start + 1e-12 {
            continue;
        }
        used_lo = used_lo.min(profile[vidx].y);
        used_hi = used_hi.max(profile[vidx].y);
    }
    if let Some(cb) = tool.cut_bottom {
        if used_lo < cb - 1e-9 {
            return Err(format!(
                "Finish operation feature '{}': the dome's contact starts {:.3}mm above T{}'s tip, but the declared cutting band starts at cut_bottom {:.3}mm — {:.3}mm of the working profile below the band is relieved surface, and a relieved surface earns no contact. Lower cut_bottom, or use a tool whose band reaches its tip",
                op.feature_name, used_lo, op.tool_num, cb, cb - used_lo
            ));
        }
    }
    if let Some(ct) = tool.cut_top {
        if used_hi > ct + 1e-9 {
            return Err(format!(
                "Finish operation feature '{}': the dome's contact runs up to {:.3}mm above T{}'s tip, but the declared cutting band ends at cut_top {:.3}mm — the last {:.3}mm of contact lands on relieved surface, which earns none. Raise cut_top, or use a tool whose band covers the contact span",
                op.feature_name, used_hi, op.tool_num, ct, used_hi - ct
            ));
        }
    }

    // Equal-Z steps within each bracket (matching "stepover" = waterline Z
    // spacing elsewhere in this file), inverting z_c(phi) per step rather
    // than stepping phi linearly, since z_c is a cosine of phi, not linear
    // in it — `parse::sphere_profile_passes`, which the stock model's tube
    // reads for the same rings.
    let passes = crate::parse::sphere_profile_passes(profile, r_part, cz, stepover);

    let title = format!("(--- form surface: {} with T{} (custom tool profile, waterline) ---)", op.feature_name, op.tool_num);
    // A custom profile's tool-centre locus is a *composite* of one
    // translated-sphere arc per active vertex (see the derivation above), not
    // the single concentric-dilated-sphere locus finish_sphere's "feed out at
    // the current height, then plunge" transition was proven safe for
    // (DECISIONS.md "Camp-2 finishing..."). That proof doesn't automatically
    // carry over to a locus that jumps discontinuously at each vertex
    // boundary, and re-deriving (or numerically re-checking) gouge-safety for
    // every possible profile shape is exactly the kind of thing that must be
    // provably right, not just right on the fixtures tried so far. So this
    // path pays for a fully conservative transition instead: retract to
    // clearance_Z (unconditionally outside the stock/part by definition)
    // between every pass, rather than reusing generate_passes' faster
    // two-move shortcut. See DECISIONS.md "Custom tool-profile finishing...".
    Ok(generate_passes_retract_between(p, op, title, cx, cy, passes))
}

fn finish_cone(p: &Part, op: &Operation, feature: &Feature) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_ball = tool.dia / 2.0;

    let (cx, cy, cz, r_part, height) = match feature {
        Feature::Cone { dia, height, x, y, z } => (*x, *y, *z, dia / 2.0, *height),
        _ => unreachable!(),
    };

    let z_apex = cz + height;

    let l_part = (r_part * r_part + height * height).sqrt();
    let sin_alpha = r_part / l_part;
    let cos_alpha = height / l_part;

    // stock_to_leave (DECISIONS.md "stock_to_leave corner roll…"): roughing
    // the cone is finishing the a'-inflated cone — a parallel flank plus an
    // a'-SPHERE about the sharp apex (the offset of a corner), so the whole
    // CL locus is the nominal locus with r_ball → r_ball + a' and the stack
    // must climb to the inflated cap's pole z_apex + r_ball + a' (stopping at
    // the nominal top leaves a polar cusp that alone can exceed the
    // allowance). The `+ skin` terms are appended without re-associating the
    // existing sums, so an absent field is byte-identical output
    // (IEEE x + 0.0 == x) — pinned by the existing goldens. The cusp
    // triangle, per the three-substitution rule, prices the PHYSICAL ball on
    // the inflated surface: corner pairs at r_part = a' (the corner-roll
    // price, 0 at a' = 0 and growing with the skin — the opposite extreme
    // from a fixed-Z body), flank pairs by the flat-chord formula (exact and
    // skin-invariant), seam straddles by the slacker piece = the flat
    // ceiling (parse::derive_cusp_height_flat's chord-fixed monotonicity).
    if let Some(scallop) = op.scallop {
        // scallop_height <= r_ball is the physical ceiling for this cusp-height
        // formula (2*r_ball*h - h*h stays real and the cusp can't exceed the
        // ball's own radius) — a request above it is a hard error, not a
        // silent clamp (DECISIONS.md "An unmachinable feature is a hard error").
        if scallop > r_ball {
            return Err(format!(
                "cone feature '{}' scallop height ({:.3}mm) exceeds tool T{}'s ball radius ({:.3}mm); reduce scallop or use a larger tool",
                op.feature_name, scallop, op.tool_num, r_ball
            ));
        }
        let s = 2.0 * (2.0 * r_ball * scallop - scallop * scallop).sqrt();
        let stepdown = (s * cos_alpha).max(0.001);
        let title = format!("(--- copy surface: {} with T{} (ball-nose constant scallop){} ---)", op.feature_name, op.tool_num, stock_suffix(op, scallop));

        let mut passes = Vec::new();
        if let Some(allowance) = op.stock_to_leave {
            // The nominal grid's single uniform-Z ladder does NOT survive the
            // stock: a pair straddling the cap/flank seam would price at the
            // flat ceiling on a chord ≥ s — i.e. AT or above the scallop —
            // a structural knife-edge error. So under stock the grid gains
            // the seam itself as a pass (no straddling pair exists at all)
            // and the cap re-grids as a uniform-psi sweep sized so every cap
            // chord stays within s: cap pairs then price under the scallop by
            // the chord-fixed monotonicity lemma (a corner sphere is the
            // tightest surface a chord can ride), and flank pairs keep the
            // exact flat-chord price = scallop. Value-presence gates the
            // fork, so a plain finishing pass is structurally untouched.
            let skin = allowance - scallop;
            let r_cap = r_ball + skin;
            let alpha = sin_alpha.patan2(cos_alpha);
            let dpsi = 2.0 * ((s / (2.0 * r_cap)).min(1.0)).pasin();
            let span = PI / 2.0 - alpha;
            let n_cap = (span / dpsi).ceil().max(1.0) as usize;
            for i in 0..=n_cap {
                let psi = PI / 2.0 - (i as f64 / n_cap as f64) * span;
                passes.push((z_apex + r_cap * psi.psin(), r_cap * psi.pcos()));
            }
            let z_seam = z_apex + r_cap * sin_alpha;
            let n_f = ((z_seam - cz) / stepdown).ceil().max(1.0) as usize;
            for j in 1..=n_f {
                let z = (z_seam - j as f64 * stepdown).max(cz);
                passes.push((z, crate::parse::cone_cl_rho(z, cz, r_part, height, r_cap)));
            }
        } else {
            let z_top = z_apex + r_ball;
            let n = ((z_top - cz) / stepdown).ceil().max(1.0) as usize;
            for i in 0..=n {
                let z = (z_top - i as f64 * stepdown).max(cz);
                passes.push((z, crate::parse::cone_cl_rho(z, cz, r_part, height, r_ball)));
            }
        }
        Ok(generate_passes(p, op, title, cx, cy, passes))
    } else {
        let stepdown = op.stepover.expect("Finish requires a stepover or scallop");
        // Sweep-then-VERIFY (parse::derive_cone_stack_cusp's pricing story):
        // the exact per-pair tip price is NOT monotone in the skin — the
        // grid's alignment with the cap/flank seam shifts as the stack top
        // moves — so no single extreme walk bounds the emitted stack. The
        // budget h is the max over a 5-point skin sweep (choosing a'), and
        // the guarantee is the verification walk at the emitted skin
        // itself: if the actual stack at a' still breaches the ceiling, the
        // op rejects with the verified number rather than emitting.
        let (skin, cusp) = if let Some(allowance) = op.stock_to_leave {
            let price = |s: f64| crate::parse::derive_cone_stack_cusp(r_part, height, r_ball, s, stepdown);
            let mut h: f64 = 0.0;
            for k in 0..=4 {
                h = h.max(price(allowance * k as f64 / 4.0));
            }
            if h.is_infinite() {
                return Err(format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: at a {:.3}mm stepdown adjacent passes do not overlap — an uncut ridge remains, which no allowance can ceiling — reduce stepover or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, stepdown
                ));
            }
            let verified = if h < allowance { price(allowance - h) } else { h };
            if verified.is_infinite() {
                return Err(format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: at a {:.3}mm stepdown adjacent passes do not overlap — an uncut ridge remains, which no allowance can ceiling — reduce stepover or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, stepdown
                ));
            }
            if h >= allowance || (allowance - h) + verified > allowance + 1e-9 {
                let worst = h.max(verified);
                return Err(format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm stepdown leaves a worst-case cusp of {:.3}mm between adjacent passes, which meets or exceeds the allowance ceiling (cusps count against it — max, not min) — reduce stepover or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, stepdown, worst
                ));
            }
            (allowance - h, h)
        } else {
            (0.0, 0.0)
        };
        let r_cap = r_ball + skin;
        let z_top = z_apex + r_cap;
        let title = format!("(--- copy surface: {} with T{} (ball-nose waterline){} ---)", op.feature_name, op.tool_num, stock_suffix(op, cusp));

        let n = ((z_top - cz) / stepdown).ceil().max(1.0) as usize;
        let mut passes = Vec::new();
        for i in 0..=n {
            let z = (z_top - i as f64 * stepdown).max(cz);
            passes.push((z, crate::parse::cone_cl_rho(z, cz, r_part, height, r_cap)));
        }
        Ok(generate_passes(p, op, title, cx, cy, passes))
    }
}

fn finish_torus(p: &Part, op: &Operation, feature: &Feature) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_ball = tool.dia / 2.0;

    let (cx, cy, cz, major_dia, minor_dia) = match feature {
        Feature::Torus { major_dia, minor_dia, x, y, z } => (*x, *y, *z, *major_dia, *minor_dia),
        _ => unreachable!(),
    };

    let r_major = major_dia / 2.0;
    let r_part = minor_dia / 2.0;

    // The inner bowl is the sole concave pass in this file, so it runs the
    // shared ⚠ gouge guard (concave_gouge_check). Its tightest concave radius of
    // curvature is at the inner equator (phi=90 deg, z=cz): there the surface
    // normal is horizontal and points at the axis, so the circumferential radius
    // of curvature equals the contact circle's own radius, r_major - r_part
    // (higher inner points are slacker: R/sin(phi) - r_part grows as phi drops).
    // r_ball > (r_major - r_part) is algebraically the old r_off > r_major guard
    // — the ball would have to cross the central axis to keep tracing the bowl —
    // so this rejects exactly the same inputs, now through the first-class check.
    let concave_radius = r_major - r_part;
    concave_gouge_check(
        &op.feature_name,
        op.tool_num,
        r_ball,
        concave_radius,
        &format!(
            "the inner-bowl passes would cross the central axis (ball-nose offset radius {:.3}mm = tool radius {:.3}mm + tube radius {:.3}mm exceeds major radius {:.3}mm)",
            r_part + r_ball, r_ball, r_part, r_major
        ),
    )?;

    // stock_to_leave (DECISIONS.md "stock_to_leave convex mirrors…"): the
    // torus is a CONVEX-cusp carrier — ring-to-ring cusps live on the tube
    // meridian, which the ball rides convexly on BOTH halves (the concave
    // budget slice's reclassification) — and the tube meridian is a circle,
    // so the whole budget is the sphere's recipe with the tube radius for the
    // sphere's: roughing the torus is finishing the inflated torus (same
    // centre circle, tube r_part + a' — the offset torus is a torus, R1
    // self-offsetting), the cusp triangle prices the physical ball on the
    // inflated tube, and stepover mode prices at FULL inflation, the fixed-Z
    // convex tight extreme (the sphere's repriced argument verbatim: the
    // crown chord grows faster than the crown angle compresses — see
    // finish_sphere and DECISIONS.md "stock_to_leave corner roll…" for the
    // audit that reversed the original nominal-extreme claim). The inner
    // bowl's circumferential concavity
    // is the gouge guard's business, never the cusp triangle's: the roughing
    // locus rides the fatter offset, so the _with_stock form runs at the skin
    // each mode actually emits, after the nominal guard above kept plain-pass
    // errors byte-identical.
    let mut outer_passes = Vec::new();
    let mut inner_passes = Vec::new();
    let (skin, mut title) = if let Some(scallop) = op.scallop {
        // cusp == scallop on the inflated tube the march is spaced against,
        // so the budget is exact here: a' = allowance − scallop (validate()
        // rejected scallop >= allowance).
        let skin = op.stock_to_leave.map(|allowance| allowance - scallop).unwrap_or(0.0);
        (
            skin,
            format!("(--- copy surface: {} with T{} (ball-nose constant scallop){} ---)", op.feature_name, op.tool_num, stock_suffix(op, scallop))
        )
    } else {
        let stepdown = op.stepover.expect("Finish requires a stepover or scallop");
        // Waterline: the worst adjacent pass pair is the first one at the
        // crown ring (acos is steepest at 1, so uniform dz steps give their
        // widest angle there — finish_sphere's argument on the tube circle),
        // and h is priced at FULL inflation, the fixed-Z convex tight
        // extreme (the corner-roll slice's audit fix — see finish_sphere).
        let r_off_nom = r_part + r_ball;
        let (skin, h) = if let Some(allowance) = op.stock_to_leave {
            let theta_max = (1.0 - stepdown / (r_off_nom + allowance)).max(0.0).pacos();
            let h = crate::parse::derive_cusp_height(theta_max, r_ball, r_part + allowance);
            if h.is_infinite() {
                return Err(format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: at a {:.3}mm stepdown adjacent passes do not overlap near the tube's crown ring — an uncut ridge remains, which no allowance can ceiling — reduce stepover or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, stepdown
                ));
            }
            if h >= allowance {
                return Err(format!(
                    "feature '{}' cannot be roughed to stock_to_leave {:.3}mm with T{}: the {:.3}mm stepdown leaves a worst-case cusp of {:.3}mm near the tube's crown ring, which meets or exceeds the allowance ceiling (cusps count against it — max, not min) — reduce stepover or increase stock_to_leave",
                    op.feature_name, allowance, op.tool_num, stepdown, h
                ));
            }
            (allowance - h, h)
        } else {
            (0.0, 0.0)
        };
        (
            skin,
            format!("(--- copy surface: {} with T{} (ball-nose waterline){} ---)", op.feature_name, op.tool_num, stock_suffix(op, h))
        )
    };
    if skin > 0.0 {
        // The emitted roughing locus rides at inflation a': the inner-bowl
        // rings pull toward the axis by exactly that much more, so the
        // crossing condition is r_ball + a' against the same inner-equator
        // radius.
        concave_gouge_check_with_stock(
            &op.feature_name,
            op.tool_num,
            r_ball,
            skin,
            concave_radius,
            &format!(
                "the inner-bowl roughing passes would cross the central axis (roughing offset radius {:.3}mm = tool radius {:.3}mm + tube radius {:.3}mm + roughing skin {:.3}mm exceeds major radius {:.3}mm)",
                r_part + r_ball + skin, r_ball, r_part, skin, r_major
            ),
        )?;
    }

    let r_off = r_part + r_ball + skin;
    let z_top = cz + r_off;

    if let Some(scallop) = op.scallop {
        // Constant angular spacing on the inflated tube: the cusp triangle is
        // cut by the physical ball on the surface the pass machines, so
        // r_part + a' goes into the triangle while r_ball stays r_ball — the
        // first slice's three-substitution rule.
        let theta = crate::parse::derive_scallop_angle(scallop, r_ball, r_part + skin);
        let n = ((PI / 2.0) / theta).ceil().max(1.0) as usize;
        for i in 0..=n {
            let phi = (i as f64 * theta).min(PI / 2.0);
            let dz = r_off * phi.pcos();
            let z = cz + dz;
            let drho = r_off * phi.psin();
            // drho <= r_off <= r_major is guaranteed by the guard above (the
            // _with_stock form when roughing), so the inner-bowl radius never
            // goes negative here.
            outer_passes.push((z, r_major + drho));
            inner_passes.push((z, r_major - drho));
        }
    } else {
        let stepdown = op.stepover.expect("Finish requires a stepover or scallop");
        let n = (r_off / stepdown).ceil().max(1.0) as usize;
        for i in 0..=n {
            let z = (z_top - i as f64 * stepdown).max(cz);
            let dz = z - cz;
            let drho = (r_off * r_off - dz * dz).max(0.0).sqrt();
            outer_passes.push((z, r_major + drho));
            inner_passes.push((z, r_major - drho));
        }
    }

    let mut g = generate_passes(p, op, title.clone(), cx, cy, outer_passes);

    // Skip the apex ring (i=0) for the inner passes since it was already cut by the outer passes.
    if inner_passes.len() > 1 {
        let inner_tail: Vec<_> = inner_passes.into_iter().skip(1).collect();
        title.push_str(" [inner bowl]");
        g.extend(generate_passes(p, op, title, cx, cy, inner_tail));
    }

    Ok(g)
}

// ---- CylinderBore (R1): exact-flank finishing of the inside bore wall -----------
// CHECKLIST row 2.3 / ROADMAP Phase 3.5 "close out the R1 quadric surface-
// finishing set". A circular cylinder is *developable* (TAXONOMY.md): a
// straight-sided tool contacts the wall along a full vertical ruling with zero
// theoretical error — a flank/swarf cut, not a ball-nose scallop. It is also R1
// self-offsetting: offsetting the bore circle inward by the tool radius yields a
// coaxial circle, so the tool-centre path is closed-form, radius R - r_tool. No
// scallop, no sampling; `stepover` here is the *axial* Z stepdown, not a radial
// cusp spacing.
//
// Scope (settled with the user, DECISIONS.md): the **inside bore** wall only,
// leaning on `Feature::CylinderBore`'s existing bore semantics (authored top-down
// from the stock top by `depth`, tool fits inside — validate enforces
// dia > tool.dia). The outside boss is the same op with the offset sign flipped
// (R + r_tool) — see finish_cylinder_boss below, which landed with the `CylinderBoss` variant
// (CHECKLIST row 2.4). Emission is a chorded circle per level (reuses circle_pts, like
// every other finish op); true G02/G03 arc interpolation is its own future item.
// Both comp modes are supported, mirroring ops::contour.
fn finish_cylinder_bore(p: &Part, op: &Operation, feature: &Feature) -> Result<crate::gcode::OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_tool = tool.dia / 2.0;

    let (cx, cy, r_part, extent, rim) = match feature {
        Feature::CylinderBore { dia, extent, x, y, z, .. } => {
            (*x, *y, dia / 2.0, *extent, crate::parse::derive_contour_top(p.stock.z, *z))
        }
        _ => unreachable!(),
    };

    // stock_to_leave on a flank: the stepdown is purely axial on a vertical
    // wall, so adjacent rings leave NO cusp between them — the cusp budget is
    // exactly zero and the whole allowance becomes radial offset (DECISIONS.md
    // "stock_to_leave first slice…"). The wall is cut to FULL depth with the
    // radial skin intact: the floor's own skin belongs to the floor's op (the
    // op-granularity split, ROADMAP "Rough vs. finish operation mode").
    let skin = op.stock_to_leave.unwrap_or(0.0);

    // The bore wall is concave (curves around the tool), so it runs the shared ⚠
    // gouge guard like the torus inner bowl. Its concave radius of curvature is
    // the bore radius itself; r_tool > r_part means the tool can't fit and the
    // coaxial tool-centre circle R - r_tool would collapse through the axis.
    // validate() already enforces dia > tool.dia, so this is redundant-but-named
    // (same pattern as finish_torus), naming the check every concave pass runs —
    // except under stock_to_leave, where r_tool + skin can exceed r_part with a
    // physically-fitting tool, and this guard is the only brake.
    concave_gouge_check_with_stock(
        &op.feature_name,
        op.tool_num,
        r_tool,
        skin,
        r_part,
        &if skin > 0.0 {
            format!(
                "the roughing ring would collapse through the bore axis (tool radius {:.3}mm + stock_to_leave {:.3}mm vs bore radius {:.3}mm)",
                r_tool, skin, r_part
            )
        } else {
            format!(
                "the tool does not fit the bore (tool radius {:.3}mm exceeds bore radius {:.3}mm)",
                r_tool, r_part
            )
        },
    )?;

    let stepdown = op
        .stepover
        .expect("Finish on a CylinderBore requires a stepover (validate ensures this)");

    // Wall runs from the RIM down to rim - extent (same Z convention as the
    // plunge cycle: derive_z_cut from the one top home; the rim is the stock
    // top unless the bore says `z`). Tip Z steps down by `stepover` to the
    // floor; the last level lands exactly on the floor so the bottom corner is
    // reached. A stepover >= extent collapses to a single full-flute pass at
    // the floor.
    let z_bottom = crate::parse::derive_z_cut(rim, extent);
    let n = (extent / stepdown).ceil().max(1.0) as usize;
    let mut z_levels = Vec::with_capacity(n);
    for i in 1..=n {
        z_levels.push((rim - i as f64 * stepdown).max(z_bottom));
    }

    // Roughing shrinks the machined wall inward by `a` (leftover skin ON the
    // wall), so engine mode shrinks the tool-centre ring and control mode
    // shrinks the emitted part-edge circle — the D register stays the pure
    // tool radius, so the allowance is engine-baked in BOTH comp modes and a
    // double-comp is unrepresentable (DECISIONS.md "stock_to_leave first
    // slice…"). `- skin` appended, not re-associated: skin == 0 is
    // byte-identical.
    match op.comp {
        CompMode::Engine => cylinder_engine(p, op, cx, cy, r_part - r_tool - skin, &z_levels).map(Into::into),
        CompMode::Control => cylinder_control(p, op, cx, cy, r_part - skin, &z_levels),
    }
}

// Engine comp: emit the coaxial tool-centre circle (radius `rho` = R - r_tool)
// as plain coordinates, one full ring per Z level. Plunge down the cleared bore
// centre, feed out to the wall, cut the ring, retract before the next level (the
// unconditionally-safe transition — the bore interior above the floor is open).
/// The sentence before a ring op's descent at its lead point (ROADMAP item
/// 2 — every descent is said): a boss is entered a diameter outside its
/// wall, a bore down its axis, and the tool feeds from there onto the wall,
/// which is a lead (slice 4 named it so; slice 1 had called it a plunge).
/// Air once the wall's own clear ran; where it is not, the entry
/// restriction refuses the program.
fn ring_entry(cw: bool) -> &'static str {
    if cw {
        "(lead: from one diameter outside the wall, in the air its clear left)"
    } else {
        "(lead: from the bore's axis, in the air its clear left)"
    }
}

/// A ring's lead, shaped (ROADMAP item 2, slice 5). Unsaid, the point every
/// ring program before the keys used — the bore's axis, a diameter outside a
/// boss's wall — with its sentence; said, the keys' shape off the ring's start
/// on the +X ray, the air toward the axis for a bore and away from it for a
/// boss, and the stock model asked that the point stands in air.
struct RingLead {
    at: Pt,
    line: String,
    shape: crate::ops::lead::LeadShape,
}

fn ring_lead(p: &Part, op: &Operation, cx: f64, cy: f64, start_r: f64, cw_travel: bool, boss: bool, unsaid: (f64, &str), z: f64) -> Result<RingLead, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let shape = crate::ops::lead::LeadShape::of(op, tool.dia);
    if !shape.said {
        return Ok(RingLead { at: Pt { x: unsaid.0, y: cy }, line: unsaid.1.to_string(), shape });
    }
    let start = Pt { x: cx + start_r, y: cy };
    let tangent = if cw_travel { (0.0, -1.0) } else { (0.0, 1.0) };
    let air = if boss { (1.0, 0.0) } else { (-1.0, 0.0) };
    let at = shape.point(start, tangent, air);
    let words = shape.words(if boss { "the ring's radius, outward" } else { "the ring's radius, toward the axis" }, "the ring's tangent, back from its start");
    // The whole lead in air: the point and every half millimetre of the
    // flight to the start, by the stock model, at the deepest level.
    let room = p.operations.iter().position(|o| std::ptr::eq(o, op)).and_then(|i| crate::stock::entry_room(p, i));
    let d = (start.x - at.x).phypot(start.y - at.y);
    let n = ((d / 0.5).ceil() as usize).max(1);
    let clear = |r: &crate::stock::EntryRoom| (0..=n).all(|k| {
        let s = k as f64 / n as f64;
        r.in_air(at.x + s * (start.x - at.x), at.y + s * (start.y - at.y), z)
    });
    let line = match room {
        None => format!("(lead: {words} — the stock model has no claim here)"),
        Some(r) if clear(&r) => format!("(lead: {words}, in air)"),
        Some(_) => return Err(format!("lead on '{}': {words} — the stock model reads standing material under the lead from ({}, {}), and a lead that is asked for must be whole", op.feature_name, f(at.x), f(at.y))),
    };
    Ok(RingLead { at, line, shape })
}

/// A ring is entered on a lead and nothing else is built for it (ROADMAP
/// item 2, slice 4): `entry=plunge` or `entry=ramp` on one is refused.
fn ring_entry_kept(op: &Operation) -> Result<(), String> {
    match op.entry {
        None | Some(crate::parse::EntryKind::Lead) => Ok(()),
        Some(k) => Err(format!("entry={} on '{}': a ring is entered on a lead in the air its clear left — from the bore's axis, or a diameter outside the boss — and a {} at the wall is not built", k.word(), op.feature_name, k.word())),
    }
}

fn cylinder_engine(p: &Part, op: &Operation, cx: f64, cy: f64, rho: f64, z_levels: &[f64]) -> Result<Vec<String>, String> {
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
    let lead = ring_lead(p, op, cx, cy, rho, !op.climbs(), false, (cx, ring_entry(false)), z_levels.iter().cloned().fold(f64::INFINITY, f64::min))?;

    let mut g = Vec::new();
    g.push(format!("(--- side bore wall (engine): {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));

    for &z in z_levels {
        g.push(lead.line.clone());
        g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y))); // over the cleared bore centre, or where the keys say
        g.push(format!("G01 Z{} F{}", f(z + lead.shape.rise()), f(plunge))); // plunge down the centre
        g.push(format!("G01 X{} Y{}{} F{}", f(cx + rho), f(cy), lead.shape.z_word(z), f(feed))); // feed out to the wall
        g.extend(ring_moves(op.compress, !op.climbs(), cx, cy, rho, feed, true));
        g.push(format!("G00 Z{}", f(clearance_z))); // retract before the next level
    }

    Ok(g)
}

// Control comp: emit the centerline = the part-edge circle (radius R) and let
// the controller apply cutter radius comp inward. Travelling CCW (circle_pts'
// winding) the tool sits left of travel = toward the centre, so G41 puts it
// inside the bore. Lead on/off through a point one tool diameter inside the
// wall, ramping comp on/off, mirroring ops::contour's contour_control.
//
// Two heads in one pass (DECISIONS.md "the control-comp centerline resolve"):
// the resolved head is the tool-center motion the controller derives from the
// text — the same entries, the ring pulled inward by the tool radius (the
// engine sibling's rho), and the engage/disengage lines running to the
// compensated ring point (left of CCW travel at angle 0 is the −X normal, so
// the point sits at r_part − r on the +X ray). The straight center line on
// the ramp moves is the contract the one-diameter lead was designed around.
fn cylinder_control(p: &Part, op: &Operation, cx: f64, cy: f64, r_part: f64, z_levels: &[f64]) -> Result<crate::gcode::OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_tool = tool.dia / 2.0;
    let d = crate::parse::derive_d(op.tool_num);

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Start/close the ring at angle 0 (the edge point circle_pts also ends on);
    // lead point is one tool diameter inside it, in the cleared bore.
    let edge_x = cx + r_part;
    let lead_x = cx + r_part - tool.dia;
    let lead = ring_lead(p, op, cx, cy, r_part - r_tool, !op.climbs(), false, (lead_x, "(lead: from one diameter inside the wall, in the air the bore's clear left)"), z_levels.iter().cloned().fold(f64::INFINITY, f64::min))?;

    let mut g = Vec::new();
    let mut c = Vec::new();
    g.push(format!("(--- side bore wall (control): {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.push(direction_note(op));
    c.push(format!("(--- side bore wall (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));

    for &z in z_levels {
        g.push(lead.line.clone());
        g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        g.push(format!("G00 Z{}", f(rapid_z)));
        g.push(format!("G01 Z{} F{}", f(z + lead.shape.rise()), f(plunge)));
        c.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        c.push(format!("G00 Z{}", f(rapid_z)));
        c.push(format!("G01 Z{} F{}", f(z + lead.shape.rise()), f(plunge)));
        // comp ON, ramped along the lead-in move onto the wall edge:
        g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(edge_x), f(cy), lead.shape.z_word(z), f(feed)));
        c.push(format!("G01 X{} Y{}{} F{}", f(edge_x - r_tool), f(cy), lead.shape.z_word(z), f(feed)));
        g.extend(ring_moves(op.compress, !op.climbs(), cx, cy, r_part, feed, false));
        c.extend(ring_moves(op.compress, !op.climbs(), cx, cy, r_part - r_tool, feed, false));
        // comp OFF, ramped along the lead-out back into the cleared bore:
        g.push(format!("G01 G40 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        c.push(format!("G01 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        g.push(format!("G00 Z{}", f(clearance_z)));
        c.push(format!("G00 Z{}", f(clearance_z)));
    }

    Ok(crate::gcode::OpMoves { text: g, resolved: Some(c) })
}

// ---- CylinderBoss (R1): exact-flank finishing of the circular outside boss wall -----
// CHECKLIST row 2.4 — finish_cylinder_bore's mirror (DECISIONS.md "Row-2.4 `side`
// selector"): the same developable flank cut with the offset sign flipped, the
// tool-centre riding a coaxial circle of radius R + r_tool *outside* the wall.
// Convex by construction (Feature::CylinderBoss is material inside the circle), so
// there is no concave radius of curvature and the shared concave_gouge_check is
// correctly not called — a tool offset outward from a convex wall cannot
// self-intersect. `stepover` is the axial Z stepdown, exactly as for the bore.
//
// The wall runs from the boss top (z + height) down to the base plane z; each
// level assumes the surrounding material at that depth was already cleared by
// roughing — the same assumption every finishing pass in this file makes.
//
// Cut direction: the boss rings are cut CW (the bore's are CCW via circle_pts).
// Two reasons, recorded per "an operation's cutting strategy is a decision":
// (1) with the spindle's standard M03 rotation, CW travel around an outside
// wall is climb milling, matching the bore's CCW-inside; (2) in control mode
// the blessed winding convention (CW = tool outside under G41) makes CW the
// only winding that puts G41's left-of-travel offset on the outside — engine
// mode follows so both comp modes cut the same direction.
fn finish_cylinder_boss(p: &Part, op: &Operation, feature: &Feature) -> Result<crate::gcode::OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_tool = tool.dia / 2.0;

    let (cx, cy, r_part, extent, z_base) = match feature {
        Feature::CylinderBoss { dia, extent, x, y, z, .. } => (*x, *y, dia / 2.0, *extent, *z),
        _ => unreachable!(),
    };

    // Tip Z steps down from the boss top to the base plane; the last level lands
    // exactly on the base so the bottom corner is reached. A stepover >= extent
    // collapses to a single full-flute pass at the base.
    let z_top = z_base + extent;
    let stepdown = op
        .stepover
        .expect("Finish on a CylinderBoss requires a stepover (validate ensures this)");
    let n = (extent / stepdown).ceil().max(1.0) as usize;
    let mut z_levels = Vec::with_capacity(n);
    for i in 1..=n {
        z_levels.push((z_top - i as f64 * stepdown).max(z_base));
    }

    // stock_to_leave on the boss flank: axial stepdown leaves no cusp (budget
    // exactly zero, the bore's argument mirrored), and the machined wall moves
    // OUTWARD by the allowance — skin on the boss. Convex, so no gouge guard
    // exists to widen; a fatter outward parallel can never self-intersect.
    // `+ skin` appended, not re-associated: skin == 0 is byte-identical.
    let skin = op.stock_to_leave.unwrap_or(0.0);

    match op.comp {
        CompMode::Engine => boss_engine(p, op, cx, cy, r_part + r_tool + skin, &z_levels).map(Into::into),
        CompMode::Control => boss_control(p, op, cx, cy, r_part + skin, &z_levels),
    }
}

// Engine comp: emit the coaxial tool-centre circle (radius `rho` = R + r_tool)
// as plain coordinates, one full CW ring per Z level. Plunge in cleared air one
// tool diameter outside the ring, feed in to the wall, cut the ring, retract
// before the next level.
fn boss_engine(p: &Part, op: &Operation, cx: f64, cy: f64, rho: f64, z_levels: &[f64]) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let lead_x = cx + rho + tool.dia;
    let lead = ring_lead(p, op, cx, cy, rho, op.climbs(), true, (lead_x, ring_entry(true)), z_levels.iter().cloned().fold(f64::INFINITY, f64::min))?;

    let mut g = Vec::new();
    g.push(format!("(--- side boss wall (engine): {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));

    for &z in z_levels {
        g.push(lead.line.clone());
        g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y))); // outside the wall, in cleared air
        g.push(format!("G01 Z{} F{}", f(z + lead.shape.rise()), f(plunge))); // plunge clear of the boss
        g.push(format!("G01 X{} Y{}{} F{}", f(cx + rho), f(cy), lead.shape.z_word(z), f(feed))); // feed in to the wall
        g.extend(ring_moves(op.compress, op.climbs(), cx, cy, rho, feed, true));
        g.push(format!("G00 Z{}", f(clearance_z))); // retract before the next level
    }

    Ok(g)
}

// Control comp: emit the centerline = the part-edge circle (radius R) and let
// the controller apply cutter radius comp outward. Travelling CW the tool sits
// left of travel = away from the centre, so G41 puts it outside the boss — the
// blessed winding convention (DECISIONS.md "Row-2.4 `side` selector"). Lead
// on/off through a point one tool diameter outside the wall, ramping comp
// on/off, mirroring cylinder_control.
//
// Two heads in one pass (DECISIONS.md "the control-comp centerline resolve"):
// the ring pushed outward by the tool radius (the engine sibling's rho), the
// engage/disengage lines running to the compensated ring point — left of CW
// travel at angle 0 is the +X normal, so it sits at r_part + r on the +X ray.
fn boss_control(p: &Part, op: &Operation, cx: f64, cy: f64, r_part: f64, z_levels: &[f64]) -> Result<crate::gcode::OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_tool = tool.dia / 2.0;
    let d = crate::parse::derive_d(op.tool_num);

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Start/close the ring at angle 0 (the edge point circle_pts_cw also ends
    // on); lead point is one tool diameter outside it, in cleared air.
    let edge_x = cx + r_part;
    let lead_x = cx + r_part + tool.dia;
    let lead = ring_lead(p, op, cx, cy, r_part + r_tool, op.climbs(), true, (lead_x, ring_entry(true)), z_levels.iter().cloned().fold(f64::INFINITY, f64::min))?;

    let mut g = Vec::new();
    let mut c = Vec::new();
    g.push(format!("(--- side boss wall (control): {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.push(direction_note(op));
    c.push(format!("(--- side boss wall (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));

    for &z in z_levels {
        g.push(lead.line.clone());
        g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        g.push(format!("G00 Z{}", f(rapid_z)));
        g.push(format!("G01 Z{} F{}", f(z + lead.shape.rise()), f(plunge)));
        c.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        c.push(format!("G00 Z{}", f(rapid_z)));
        c.push(format!("G01 Z{} F{}", f(z + lead.shape.rise()), f(plunge)));
        // comp ON, ramped along the lead-in move onto the wall edge:
        g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(edge_x), f(cy), lead.shape.z_word(z), f(feed)));
        c.push(format!("G01 X{} Y{}{} F{}", f(edge_x + r_tool), f(cy), lead.shape.z_word(z), f(feed)));
        g.extend(ring_moves(op.compress, op.climbs(), cx, cy, r_part, feed, false));
        c.extend(ring_moves(op.compress, op.climbs(), cx, cy, r_part + r_tool, feed, false));
        // comp OFF, ramped along the lead-out back into cleared air:
        g.push(format!("G01 G40 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        c.push(format!("G01 X{} Y{}", f(lead.at.x), f(lead.at.y)));
        g.push(format!("G00 Z{}", f(clearance_z)));
        c.push(format!("G00 Z{}", f(clearance_z)));
    }

    Ok(crate::gcode::OpMoves { text: g, resolved: Some(c) })
}

// ---- The edge round (TAXONOMY row: convex round, external edge blend) -------
// DECISIONS.md → "the corner rounder gets an operation". The part's round IS
// the tool's ground arc, so this op does not approximate a surface — it places
// one. That is why there is no stepover, no scallop and no pass ladder here:
// ONE pass, at one Z, and the form is stamped by the cutting edge.
//
// The whole placement is two lines, and both come off `parse::EdgeRound`:
//
//   * the tool's arc centre must land on the round's own centre of curvature,
//     which puts the AXIS one effective radius off the wall — `rc - R`, the
//     diameter of consequence, and never the relieved pilot;
//   * the emitted coordinate IS the virtual sharp corner (`Silhouette::drop()`
//     is a whole `gauge_z` for this tool), so the emitted Z is simply the
//     model's edge, and the tip lands `gauge_z` below it without anyone
//     computing where the tip goes.
//
// Winding follows the wall its feature already cuts — bore CCW, boss CW — so
// climb milling and the blessed control-comp convention both carry over
// unchanged from finish_cylinder_bore / finish_cylinder_boss.
fn finish_edge_round(
    p: &Part,
    op: &Operation,
    feature: &Feature,
    tool: &crate::parse::Tool,
) -> Result<crate::gcode::OpMoves, String> {
    let er = crate::parse::derive_edge_round(p, feature)
        .expect("validate: a CornerRounder Finish requires a feature with edge_round");
    let arc = tool.ground_arc().expect("a CornerRounder hands over its arc");

    // The Z of the virtual sharp corner. `face_standoff` lifts the datum off the
    // face; `wall_standoff` backs the axis off the wall. Each is positive-away,
    // and each has exactly one home.
    let z_corner = er.z_top + op.face_standoff;

    // Only the plan curve differs between the two pairs, and it differs in
    // exactly one way: a circle turns "one effective radius into the void" into
    // a signed radius, a polyline hands the same number to the offsetter.
    let (cx, cy, wall_radius) = match er.circle() {
        Some(c) => c,
        None => return crate::ops::profile::finish_profile_edge_round(p, op, &er, &arc, z_corner),
    };
    let rho = er.axis_radius(arc.effective_radius(), op.wall_standoff);
    let cw = er.boss;

    match op.comp {
        CompMode::Engine => edge_round_engine(p, op, cx, cy, rho, z_corner, cw).map(Into::into),
        // Control comp: the emitted coordinates are the model's own edge circle
        // and the controller offsets by D/2. That is not a second convention —
        // driving the virtual sharp corner along the model edge is exactly what
        // control comp already means here, which is why the datum and the comp
        // mode turn out to be the same object.
        CompMode::Control => edge_round_control(p, op, cx, cy, wall_radius, rho, z_corner, cw),
    }
}

// Engine comp: the axis ring as plain coordinates. Lead in from clear air one
// body diameter outside the ring — `tool.dia` is the BODY OD here, which is the
// one question a clearance surface is legitimately asked.
fn edge_round_engine(
    p: &Part, op: &Operation, cx: f64, cy: f64, rho: f64, z_corner: f64, cw: bool,
) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Approach from outside for a boss, from the bore's own axis for a bore:
    // either way the lead point is in air the wall's own op already opened.
    let lead_x = if cw { cx + rho + tool.dia } else { cx };
    let lead = ring_lead(p, op, cx, cy, rho, cw == op.climbs(), cw, (lead_x, ring_entry(cw)), z_corner)?;

    let mut g = Vec::new();
    g.push(format!("(--- break edge (engine): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(direction_note(op));
    g.push(lead.line.clone());
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    g.push(format!("G01 Z{} F{}", f(z_corner + lead.shape.rise()), f(plunge)));
    g.push(format!("G01 X{} Y{}{} F{}", f(cx + rho), f(cy), lead.shape.z_word(z_corner), f(feed)));
    g.extend(ring_moves(op.compress, cw == op.climbs(), cx, cy, rho, feed, true));
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

// Control comp: centerline = the part edge, offset register D from the tool —
// and for this tool D is the EFFECTIVE diameter plus twice the standoff, which
// `gcode.rs` states in the header. The standoff folds into that one number and
// never into the path, so there is no second place for it to also be applied.
fn edge_round_control(
    p: &Part, op: &Operation, cx: f64, cy: f64, r_edge: f64, rho: f64, z_corner: f64, cw: bool,
) -> Result<crate::gcode::OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let d = crate::parse::derive_d(op.tool_num);
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let edge_x = cx + r_edge;
    let lead_x = if cw { cx + r_edge + tool.dia } else { cx };
    let lead = ring_lead(p, op, cx, cy, rho, cw == op.climbs(), cw, (lead_x, ring_entry(cw)), z_corner)?;

    let mut g = Vec::new();
    let mut c = Vec::new();
    g.push(format!("(--- break edge (control): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(direction_note(op));
    c.push(format!("(--- break edge (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));
    g.push(lead.line.clone());
    g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    c.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    g.push(format!("G00 Z{}", f(rapid_z)));
    c.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(z_corner + lead.shape.rise()), f(plunge)));
    c.push(format!("G01 Z{} F{}", f(z_corner + lead.shape.rise()), f(plunge)));
    // A boss is cut CW so the tool sits left of travel = outside: G41. A bore is
    // cut CCW so left of travel is again the material side, inward: G41 too —
    // the same register, the same convention the wall ops already carry.
    g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(edge_x), f(cy), lead.shape.z_word(z_corner), f(feed)));
    c.push(format!("G01 X{} Y{}{} F{}", f(cx + rho), f(cy), lead.shape.z_word(z_corner), f(feed)));
    g.extend(ring_moves(op.compress, cw == op.climbs(), cx, cy, r_edge, feed, false));
    c.extend(ring_moves(op.compress, cw == op.climbs(), cx, cy, rho, feed, false));
    g.push(format!("G01 G40 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    c.push(format!("G01 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));

    Ok(crate::gcode::OpMoves { text: g, resolved: Some(c) })
}

// ---- The chamfer (the edge round's conical sibling) -------------------------
// DECISIONS.md → "the chamfer tool — one tangent surface instead of two…". The
// same one-pass form placement as the edge round, with one structural change:
// the cone is tangent to ONE surface, so the placement carries a degree of
// freedom the arc did not — resolved by `parse::derive_chamfer_placement` from
// the op's `tip_clearance` (option B, as blessed). The emitted Z is the
// physical TIP (`Silhouette::drop()` is zero for this tool), solved so the
// flank passes through the face's bottom corner; the axis rides `reach` off
// the wall, which under control comp is exactly D/2.
fn finish_chamfer(
    p: &Part,
    op: &Operation,
    feature: &Feature,
    tool: &crate::parse::Tool,
) -> Result<crate::gcode::OpMoves, String> {
    let ec = crate::parse::derive_chamfer(p, feature)
        .expect("validate: a ChamferMill Finish requires a feature with a chamfer");
    let ch = tool.chamfer.as_ref().expect("a ChamferMill carries its record");
    let cone = tool.ground_cone().expect("a ChamferMill hands over its cone");
    let pl = crate::parse::derive_chamfer_placement(
        &ec,
        &cone,
        crate::parse::derive_tip_clearance(op, ch),
        op.wall_standoff,
        op.face_standoff,
    );

    let (cx, cy, wall_radius) = match ec.circle() {
        Some(c) => c,
        None => {
            // Each run of a loop whose points restate the leg (slice 9) has
            // its own tip Z; the profile emitter asks for it by leg.
            let tip_clearance = crate::parse::derive_tip_clearance(op, ch);
            let z_tip = |leg: crate::parse::ChamferLeg| {
                crate::parse::derive_leg_placement(leg, ec.z_top, &cone, tip_clearance, op.wall_standoff, op.face_standoff).z_tip
            };
            return crate::ops::profile::finish_profile_chamfer(p, op, &ec, &pl, &z_tip);
        }
    };
    let rho = ec.axis_radius(pl.reach);
    let cw = ec.boss;

    match op.comp {
        CompMode::Engine => chamfer_engine(p, op, cx, cy, rho, pl.z_tip, cw).map(Into::into),
        CompMode::Control => chamfer_control(p, op, cx, cy, wall_radius, rho, pl.z_tip, cw),
    }
}

// Engine comp: the axis ring as plain coordinates — `edge_round_engine` with
// the emitted Z the placement's tip Z instead of the corner datum.
fn chamfer_engine(
    p: &Part, op: &Operation, cx: f64, cy: f64, rho: f64, z_tip: f64, cw: bool,
) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Approach from outside for a boss, from the bore's own axis for a bore:
    // either way the lead point is in air the wall's own op already opened.
    let lead_x = if cw { cx + rho + tool.dia } else { cx };
    let lead = ring_lead(p, op, cx, cy, rho, cw == op.climbs(), cw, (lead_x, ring_entry(cw)), z_tip)?;

    let mut g = Vec::new();
    g.push(format!("(--- chamfer edge (engine): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(direction_note(op));
    g.push(lead.line.clone());
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    g.push(format!("G01 Z{} F{}", f(z_tip + lead.shape.rise()), f(plunge)));
    g.push(format!("G01 X{} Y{}{} F{}", f(cx + rho), f(cy), lead.shape.z_word(z_tip), f(feed)));
    g.extend(ring_moves(op.compress, cw == op.climbs(), cx, cy, rho, feed, true));
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

// Control comp: centerline = the part edge, offset register D from the
// placement — the cone's diameter at the contact height, which the header
// states so the operator can check it. The tip_clearance and standoff fold
// into that one number and never into the path.
fn chamfer_control(
    p: &Part, op: &Operation, cx: f64, cy: f64, r_edge: f64, rho: f64, z_tip: f64, cw: bool,
) -> Result<crate::gcode::OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let d = crate::parse::derive_d(op.tool_num);
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let edge_x = cx + r_edge;
    let lead_x = if cw { cx + r_edge + tool.dia } else { cx };
    let lead = ring_lead(p, op, cx, cy, rho, cw == op.climbs(), cw, (lead_x, ring_entry(cw)), z_tip)?;

    let mut g = Vec::new();
    let mut c = Vec::new();
    g.push(format!("(--- chamfer edge (control): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(direction_note(op));
    c.push(format!("(--- chamfer edge (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));
    g.push(lead.line.clone());
    g.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    c.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    g.push(format!("G00 Z{}", f(rapid_z)));
    c.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(z_tip + lead.shape.rise()), f(plunge)));
    c.push(format!("G01 Z{} F{}", f(z_tip + lead.shape.rise()), f(plunge)));
    // Bore CCW, boss CW, left of travel the material side both ways: G41, the
    // same register convention every comp'd wall already carries.
    g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(edge_x), f(cy), lead.shape.z_word(z_tip), f(feed)));
    c.push(format!("G01 X{} Y{}{} F{}", f(cx + rho), f(cy), lead.shape.z_word(z_tip), f(feed)));
    g.extend(ring_moves(op.compress, cw == op.climbs(), cx, cy, r_edge, feed, false));
    c.extend(ring_moves(op.compress, cw == op.climbs(), cx, cy, rho, feed, false));
    g.push(format!("G01 G40 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    c.push(format!("G01 X{} Y{}", f(lead.at.x), f(lead.at.y)));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));

    Ok(crate::gcode::OpMoves { text: g, resolved: Some(c) })
}

// ---- Elliptic pair (row 2.5): pointwise-parallel flank walls ----------------
// CHECKLIST row 2.5's elliptic member, in and out: the vertical wall whose
// cross-section is an axis-aligned ellipse (semi-axes a = major_dia/2 along X,
// b = minor_dia/2 along Y). Structurally these are finish_cylinder_bore /
// finish_cylinder_boss with the ring generalized — the wall is still developable, so
// the flank contact is still exact, `stepover` is still the axial Z stepdown,
// and both comp modes mirror the circular pair line for line. What changed is
// the R-regime of the ring itself: the parallel curve of an ellipse is NOT an
// ellipse (there is no elementary offset — this is what made row 2.5 R2 while
// rows 2.3/2.4 were R1), so the tool-centre ring is built pointwise from the
// closed-form normal: for normal angle phi, the ellipse point is
// t = atan2(b·sin phi, a·cos phi) and the CL point is P(t) ± r_tool·(cos phi,
// sin phi) — ellipse_offset_pts below, chord-flattened like every ring in
// this file. At a == b every formula collapses to the circular pair's, and
// the reduces-to tests pin g-code equality with finish_cylinder_bore/finish_cylinder_boss —
// the same anchor move as test_ellipsoid_reduces_to_sphere.
//
// The bore's inward parallel self-intersects (cusps at the evolute) once
// r_tool exceeds the wall's tightest concave radius of curvature — b²/a, at
// the major-axis vertices — so the shared ⚠ gouge guard runs FIRST against
// b²/a: the first flank consumer whose guard radius differs from its fit
// radius (the circular bore's coincided at R). Passing it implies the tool
// also fits the bore, since b²/a <= b. The boss's outward parallel is convex
// and never self-intersects: no guard, the absence stated in the roster.
//
// Deliberate scope, recorded in DECISIONS ("Row 2.5, elliptic member…"): the
// ellipse is the one conic whose profile is a *closed* curve, so it is the
// row's minimal honest constructible; the parabolic/hyperbolic members name
// open profiles that cannot stand alone as a wall and stay named-open. And
// the degenerate circle (major == minor) is rejected at validation pointing
// at CylinderBore/CylinderBoss, the features that ARE that shape.
//
// The ROTATED superset (DECISIONS.md "Rotated ellipse…"): the optional
// `rotation` field turns the major axis CCW from +X about the feature
// centre. A rotation is a rigid motion, so nothing the math depends on
// changes — the evolute gouge bound b²/a, the chord flattening (turning
// angles are rotation-invariant), the windings, both comp modes — only the
// emitted coordinates turn: every ring point and every entry/lead point is
// the axis-aligned one rotated about (cx, cy). The absent case is threaded
// as Option::None (not Some(0°)) so the axis-aligned path keeps its original
// floating-point expressions verbatim — the reduces-to-CylinderBore/CylinderBoss anchors
// pin exact g-code equality, which a re-associated sum could break by an ulp.


// Engine comp: emit the inward-parallel tool-centre ring as plain coordinates,
// one full CCW ring per Z level — cylinder_engine with the ring generalized.
// Plunge down the cleared bore centre, feed out along +X to the phi = 0 CL
// point (a − r_off, the major vertex, where the parallel's own point is
// exactly on the X axis), cut the ring, retract before the next level.
// `r_off` is the inward offset distance: the tool radius, plus the
// stock_to_leave skin when the op is a rougher.

// Control comp: emit the centerline = the part-edge ellipse and let the
// controller apply cutter radius comp inward — cylinder_control with the ring
// generalized (the controller offsets the chords; its polyline parallel stays
// within the same chord tolerance the ring was flattened to). CCW + G41 puts
// the tool inside, the same winding rule as the circular bore.
//
// Two heads in one pass (DECISIONS.md "the control-comp centerline resolve"):
// the resolved head emits the TRUE inward parallel at skin + r (the engine
// sibling's ring) — within the chord tolerance of the controller's polyline
// parallel, the same equivalence the emission above already leans on. The
// engage/disengage target is the parallel's own major-vertex point, which
// sits on the same axis ray as the lead.

// finish_cylinder_boss's ellipse sibling: the outward pointwise parallel, convex by
// construction (no guard — see the roster), CW rings for the same two
// reasons recorded on finish_cylinder_boss (climb under M03 outside; CW is the
// winding that puts G41's left-of-travel offset outside in control mode).

// Engine comp: emit the outward-parallel tool-centre ring as plain
// coordinates, one full CW ring per Z level — boss_engine with the ring
// generalized. Plunge in cleared air one tool diameter outside the +X CL
// point, feed in to the wall, cut the ring, retract before the next level.
// `r_off` is the outward offset distance: the tool radius, plus the
// stock_to_leave skin when the op is a rougher.

// Control comp: emit the centerline = the part-edge ellipse CW and let the
// controller comp outward — boss_control with the ring generalized.
// Two heads in one pass (DECISIONS.md "the control-comp centerline resolve"):
// the resolved head emits the TRUE outward parallel at skin + r (the engine
// sibling's ring) — the chord-tolerance equivalence stated on the bore arm.

// ---- Mesh (camp 3): raster drop-cutter (z-map) finishing --------------------
// The full-3D escape hatch (ROADMAP Phase 4; DECISIONS.md "Camp-3 slice 1:
// raster drop-cutter (z-map) mesh finishing"). No equation, so no closed-form
// path: serpentine XY passes over the mesh's footprint, and at every sample the
// ball-nose centre height is the *upper envelope* — the max of
// mesh::drop_ball_z over all triangles. Like every finish pass here the emitted
// Z is the ball CENTRE (finish_sphere's convention).
//
// This is the one finishing path that accepts sampling error: between samples
// the true surface can rise by ~curvature x step^2/8 plus the mesh's own chord
// error. The spacing has one home, parse::derive_mesh_sample_step (= stepover
// in v1, isotropic in X and Y). CHORD_TOL_MM is deliberately NOT reused — it
// bounds circle flattening, a different error class.
//
// Undercuts: Feature::Mesh is the first declaration that can express one. A
// vertical drop-cutter machines the upper envelope — precisely the surface a
// 3-axis machine can reach — so material under an overhang is *left*, never
// gouged; silently-correct by construction, no check needed (DECISIONS.md).
//
// Raster extent is the mesh's XY bounding box (after placement) expanded by
// r_ball on all sides, so the ball rolls over boundary edges and vertices.
// Samples with no triangle contact clamp to z_floor = mesh z_min + r_ball (the
// tip never goes below the mesh's lowest point); the same clamp catches
// boundary-skirt contacts that would carry the centre below the mesh bottom.
//
// Row-to-row transitions are a single 3D G01 to the next row's first sample —
// both endpoints are on the envelope, so the straight move between adjacent
// samples is the same error class as along-row motion, not a special case.
//
// The triangle scan is linear per sample — O(rows x cols x tris). Fine for the
// fixture scale this slice ships at; a uniform grid / BVH is the named next
// step when a real scanned mesh makes it hurt, and wgpu for this exact loop
// stays an open ROADMAP consideration (this CPU path is its future oracle).
fn finish_mesh(p: &Part, op: &Operation, feature: &Feature) -> Vec<String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_ball = tool.dia / 2.0;

    let (triangles, ox, oy, oz) = match feature {
        Feature::Mesh { triangles, x, y, z, .. } => (triangles, *x, *y, *z),
        _ => unreachable!(),
    };
    assert!(!triangles.is_empty(), "mesh features are non-empty (parse ensures this)");

    // Shift the triangles by the placement offset once, up front.
    let tris: Vec<crate::mesh::Tri> = triangles
        .iter()
        .map(|t| {
            let mut v = t.v;
            for vert in &mut v {
                vert[0] += ox;
                vert[1] += oy;
                vert[2] += oz;
            }
            crate::mesh::Tri { v }
        })
        .collect();

    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for t in &tris {
        for vert in &t.v {
            for i in 0..3 {
                min[i] = min[i].min(vert[i]);
                max[i] = max[i].max(vert[i]);
            }
        }
    }
    let (x0, x1) = (min[0] - r_ball, max[0] + r_ball);
    let (y0, y1) = (min[1] - r_ball, max[1] + r_ball);
    let z_floor = min[2] + r_ball;

    let step = crate::parse::derive_mesh_sample_step(
        op.stepover.expect("Finish on a Mesh requires a stepover (validate ensures this)"),
    );

    // Sample coordinates, guaranteed to cover [lo, hi] with the last sample
    // clamped onto the far edge (facing's ys convention).
    let coords = |lo: f64, hi: f64| {
        let n = ((hi - lo) / step).ceil() as usize;
        (0..=n).map(|i| (lo + i as f64 * step).min(hi)).collect::<Vec<f64>>()
    };
    let xs = coords(x0, x1);
    let ys = coords(y0, y1);

    let drop = |px: f64, py: f64| {
        tris.iter()
            .filter_map(|t| crate::mesh::drop_ball_z(t, px, py, r_ball))
            .fold(f64::NEG_INFINITY, f64::max)
            .max(z_floor)
    };

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let mut g = Vec::new();
    g.push(format!(
        "(--- copy surface: {} with T{} (mesh raster drop-cutter) ---)",
        op.feature_name, op.tool_num
    ));
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(xs[0]), f(ys[0])));
    g.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(drop(xs[0], ys[0])), f(plunge)));

    for (row, &y) in ys.iter().enumerate() {
        // Serpentine: even rows left->right, odd rows right->left.
        let row_xs: Vec<f64> = if row % 2 == 0 {
            xs.clone()
        } else {
            xs.iter().rev().cloned().collect()
        };
        for (col, &x) in row_xs.iter().enumerate() {
            if row == 0 && col == 0 {
                continue; // already plunged onto the first sample
            }
            // The row transition (col == 0) is the same single 3D cut move as
            // any other sample — see the header comment.
            g.push(format!("G01 X{} Y{} Z{} F{}", f(x), f(y), f(drop(x, y)), f(feed)));
        }
    }
    g.push(format!("G00 Z{}", f(clearance_z)));
    g
}

// ---- the general NURBS patch: analytic raster drop-cutter -------------------
// (DECISIONS.md "the general NURBS patch…", slice 1.) The exact surface under
// the mesh raster contract: the same serpentine passes, the same isotropic
// sample spacing (parse::derive_mesh_sample_step — one home for the camp-3
// raster's error budget, shared deliberately: the two paths accept the same
// sampling term), the same upper-envelope ball drop — but the envelope is
// computed against the exact patch, so the mesh's chord term is gone.
//
// The drop at a sample (px, py) maximizes
//     f(u, v) = S_z + sqrt(r² − (S_x−px)² − (S_y−py)²)
// over the closed domain rectangle: the centre height at which a ball over
// (px, py) touches S(u, v). Every f value at ANY (u, v) is a height the ball
// can occupy while touching the surface — never above the true envelope — so
// every candidate is safe by construction and the only failure mode is
// MISSING the global max, which errs low: a gouge, the direction this house
// never ships (the mesh's max-over-all-triangles is total and cannot miss).
// That is why the seed grid is capture-bounded, not guessed: seeds are laid
// per knot span with spacing (r/2) / deriv_bound, where deriv_bound is the
// control net's convex-hull derivative bound (nurbs::deriv_bound_u/_v), so
// adjacent seeds' surface points are at most r/2 apart in space and the raw
// seed maximum alone is already a dense-sample envelope. Newton then polishes
// every in-disc seed's basin exact — interior stationary points in 2D, the
// four boundary iso-curves in 1D, the corners as plain values: the smooth
// mirror of drop_ball_z's face/edge/vertex triple. A derived-density dense
// check plus exact polish, not a proof (the machinable-cap tier-2 honesty);
// the brute-force ceiling pin in the tests is the enforcement.
//
// Raster extent is the control net's bounding box + r_ball: the hull CONTAINS
// the surface (positive weights), so the raster over-covers, never under.
// Samples with no contact clamp to z_floor = (seed-grid surface minimum) +
// r_ball — the mesh's clamp with the exact minimum replaced by a dense-scan
// minimum, which errs a hair HIGH (clipping the skirt roll-over marginally,
// the leave-material side), never low. Contacts that graze the ball's horizon
// near the patch rim sit just above the rim height and fall to the same clamp
// whenever the rim is within a ball radius of the patch minimum — the mesh's
// own skirt reasoning, stated rather than discovered.
//
// Undercuts: silently-correct by construction, the mesh's argument verbatim —
// the vertical drop machines the upper envelope, an overhang is left standing.
fn finish_nurbs_patch(p: &Part, op: &Operation, feature: &Feature) -> Vec<String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r_ball = tool.dia / 2.0;

    let (surface, ox, oy, oz) = match feature {
        Feature::NurbsPatch { surface, x, y, z, .. } => (surface, *x, *y, *z),
        _ => unreachable!(),
    };

    // Shift the control net by the placement offset once, up front (the
    // mesh's triangle-shift precedent): everything downstream is stock coords.
    let mut surf = surface.clone();
    for pl in &mut surf.poles {
        pl[0] += ox;
        pl[1] += oy;
        pl[2] += oz;
    }

    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for pl in &surf.poles {
        for i in 0..3 {
            min[i] = min[i].min(pl[i]);
            max[i] = max[i].max(pl[i]);
        }
    }
    let (x0, x1) = (min[0] - r_ball, max[0] + r_ball);
    let (y0, y1) = (min[1] - r_ball, max[1] + r_ball);

    let seeds = PatchSeeds::new(&surf, r_ball);
    let z_floor = seeds.z_min + r_ball;

    let step = crate::parse::derive_mesh_sample_step(
        op.stepover.expect("Finish on a NurbsPatch requires a stepover (validate ensures this)"),
    );

    // Sample coordinates, guaranteed to cover [lo, hi] with the last sample
    // clamped onto the far edge (the mesh raster's convention).
    let coords = |lo: f64, hi: f64| {
        let n = ((hi - lo) / step).ceil() as usize;
        (0..=n).map(|i| (lo + i as f64 * step).min(hi)).collect::<Vec<f64>>()
    };
    let xs = coords(x0, x1);
    let ys = coords(y0, y1);

    let drop = |px: f64, py: f64| {
        patch_drop(&surf, &seeds, px, py, r_ball).unwrap_or(f64::NEG_INFINITY).max(z_floor)
    };

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let mut g = Vec::new();
    g.push(format!(
        "(--- copy surface: {} with T{} (nurbs patch raster drop-cutter) ---)",
        op.feature_name, op.tool_num
    ));
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(xs[0]), f(ys[0])));
    g.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(drop(xs[0], ys[0])), f(plunge)));

    for (row, &y) in ys.iter().enumerate() {
        // Serpentine: even rows left->right, odd rows right->left; the row
        // transition is a single 3D cut move, the mesh's argument (both
        // endpoints are on the envelope, so it is the same error class).
        let row_xs: Vec<f64> = if row % 2 == 0 {
            xs.clone()
        } else {
            xs.iter().rev().cloned().collect()
        };
        for (col, &x) in row_xs.iter().enumerate() {
            if row == 0 && col == 0 {
                continue; // already plunged onto the first sample
            }
            g.push(format!("G01 X{} Y{} Z{} F{}", f(x), f(y), f(drop(x, y)), f(feed)));
        }
    }
    g.push(format!("G00 Z{}", f(clearance_z)));
    g
}

// The capture-bounded seed grid, laid once per op and reused for every raster
// sample: parameters per direction (per knot span, spacing (r/2)/deriv_bound
// so adjacent seeds' surface points move at most r/2 in space — see
// finish_nurbs_patch's header), plus every seed pair's surface point evaluated
// up front (the raw-seed envelope pass then needs no surface evaluation at
// all). Cost scales with the patch's own slope-complexity — a steep or
// many-span patch buys more seeds; no silent cap.
struct PatchSeeds {
    us: Vec<f64>,
    vs: Vec<f64>,
    /// points[iu * vs.len() + iv] = S(us[iu], vs[iv]).
    points: Vec<[f64; 3]>,
    z_min: f64,
}

impl PatchSeeds {
    fn new(surf: &crate::nurbs::BSplineSurface, r_ball: f64) -> Self {
        let capture = r_ball / 2.0;
        let lay = |spans: Vec<f64>, bound: f64| {
            let mut out = vec![spans[0]];
            for pair in spans.windows(2) {
                let n = (((pair[1] - pair[0]) * bound / capture).ceil() as usize).max(1);
                for k in 1..=n {
                    out.push(pair[0] + (pair[1] - pair[0]) * k as f64 / n as f64);
                }
            }
            out
        };
        let us = lay(surf.distinct_knots_u(), surf.deriv_bound_u());
        let vs = lay(surf.distinct_knots_v(), surf.deriv_bound_v());
        let mut points = Vec::with_capacity(us.len() * vs.len());
        let mut z_min = f64::INFINITY;
        for &u in &us {
            for &v in &vs {
                let s = surf.eval(u, v);
                z_min = z_min.min(s[2]);
                points.push(s);
            }
        }
        PatchSeeds { us, vs, points, z_min }
    }
}

// The ball-drop upper envelope against the exact patch at one sample — see
// finish_nurbs_patch's header for the argument. None when no candidate
// touches (the sample is beyond the ball's reach of the whole seed grid).
fn patch_drop(
    surf: &crate::nurbs::BSplineSurface,
    seeds: &PatchSeeds,
    px: f64,
    py: f64,
    r: f64,
) -> Option<f64> {
    let r2 = r * r;
    let (u0, u1) = surf.domain_u();
    let (v0, v1) = surf.domain_v();
    let mut best: Option<f64> = None;
    let mut keep = |cand: Option<f64>| {
        if let Some(z) = cand {
            if z.is_finite() {
                best = Some(best.map_or(z, |b: f64| b.max(z)));
            }
        }
    };

    // The centre height touching S(u, v), where the ball reaches it at all.
    let f_at = |u: f64, v: f64| -> Option<f64> {
        let s = surf.eval(u, v);
        let q = (s[0] - px).powi(2) + (s[1] - py).powi(2);
        if q > r2 {
            None
        } else {
            Some(s[2] + (r2 - q).sqrt())
        }
    };

    // 2D Newton on the stationarity of f from one seed, iterates clamped to
    // the domain. The result (converged or not) is itself a touching height,
    // so it joins the max unconditionally — a wandering Newton can only ever
    // fail to IMPROVE the envelope, never corrupt it.
    let newton2 = |mut u: f64, mut v: f64| -> Option<f64> {
        for _ in 0..15 {
            let (s, su, sv, suu, suv, svv) = surf.eval_d2(u, v);
            let (dx, dy) = (s[0] - px, s[1] - py);
            let g2 = r2 - (dx * dx + dy * dy);
            if g2 <= 1e-9 * r2 {
                break; // at the contact horizon: the value outside decides
            }
            let g = g2.sqrt();
            let qu = 2.0 * (dx * su[0] + dy * su[1]);
            let qv = 2.0 * (dx * sv[0] + dy * sv[1]);
            let quu = 2.0 * (su[0] * su[0] + dx * suu[0] + su[1] * su[1] + dy * suu[1]);
            let qvv = 2.0 * (sv[0] * sv[0] + dx * svv[0] + sv[1] * sv[1] + dy * svv[1]);
            let quv = 2.0 * (su[0] * sv[0] + dx * suv[0] + su[1] * sv[1] + dy * suv[1]);
            let fu = su[2] - qu / (2.0 * g);
            let fv = sv[2] - qv / (2.0 * g);
            let fuu = suu[2] - quu / (2.0 * g) - qu * qu / (4.0 * g * g * g);
            let fvv = svv[2] - qvv / (2.0 * g) - qv * qv / (4.0 * g * g * g);
            let fuv = suv[2] - quv / (2.0 * g) - qu * qv / (4.0 * g * g * g);
            let det = fuu * fvv - fuv * fuv;
            if det.abs() < 1e-14 {
                break;
            }
            let du = -(fu * fvv - fv * fuv) / det;
            let dv = -(fv * fuu - fu * fuv) / det;
            u = (u + du).clamp(u0, u1);
            v = (v + dv).clamp(v0, v1);
            if du.abs() < 1e-12 * (u1 - u0) && dv.abs() < 1e-12 * (v1 - v0) {
                break;
            }
        }
        f_at(u, v)
    };

    // 1D Newton along one boundary iso-curve (u free at fixed v, or v free at
    // fixed u) — the boundary of the closed rectangle owns the maxima the
    // interior stationarity misses, exactly as drop_ball_z's edge case owns
    // the silhouettes its face case skips.
    let newton1 = |along_u: bool, fixed: f64, mut t: f64| -> Option<f64> {
        let (lo, hi) = if along_u { (u0, u1) } else { (v0, v1) };
        for _ in 0..15 {
            let (u, v) = if along_u { (t, fixed) } else { (fixed, t) };
            let (s, su, sv, suu, _suv, svv) = surf.eval_d2(u, v);
            let (st, stt) = if along_u { (su, suu) } else { (sv, svv) };
            let (dx, dy) = (s[0] - px, s[1] - py);
            let g2 = r2 - (dx * dx + dy * dy);
            if g2 <= 1e-9 * r2 {
                break;
            }
            let g = g2.sqrt();
            let qt = 2.0 * (dx * st[0] + dy * st[1]);
            let qtt = 2.0 * (st[0] * st[0] + dx * stt[0] + st[1] * st[1] + dy * stt[1]);
            let ft = st[2] - qt / (2.0 * g);
            let ftt = stt[2] - qtt / (2.0 * g) - qt * qt / (4.0 * g * g * g);
            if ftt.abs() < 1e-14 {
                break;
            }
            let dt = -ft / ftt;
            t = (t + dt).clamp(lo, hi);
            if dt.abs() < 1e-12 * (hi - lo) {
                break;
            }
        }
        if along_u {
            f_at(t, fixed)
        } else {
            f_at(fixed, t)
        }
    };

    // Pass 1: the raw seed envelope — every precomputed seed point's touching
    // height, no surface evaluation needed. Remember the in-disc seeds for
    // the polish passes.
    let n_v = seeds.vs.len();
    let mut in_disc: Vec<(usize, usize)> = Vec::new();
    for iu in 0..seeds.us.len() {
        for iv in 0..n_v {
            let s = seeds.points[iu * n_v + iv];
            let q = (s[0] - px).powi(2) + (s[1] - py).powi(2);
            if q <= r2 {
                keep(Some(s[2] + (r2 - q).sqrt()));
                in_disc.push((iu, iv));
            }
        }
    }

    // Pass 2: polish — 2D from every in-disc seed; 1D along a boundary edge
    // from its in-disc seeds; corners are already raw seed values.
    for &(iu, iv) in &in_disc {
        let (u, v) = (seeds.us[iu], seeds.vs[iv]);
        keep(newton2(u, v));
        if iv == 0 {
            keep(newton1(true, v0, u));
        }
        if iv == n_v - 1 {
            keep(newton1(true, v1, u));
        }
        if iu == 0 {
            keep(newton1(false, u0, v));
        }
        if iu == seeds.us.len() - 1 {
            keep(newton1(false, u1, v));
        }
    }

    best
}

fn generate_passes(p: &Part, op: &Operation, title: String, cx: f64, cy: f64, passes: Vec<(f64, f64)>) -> Vec<String> {
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let mut g = Vec::new();
    g.push(title);
    g.push(format!("G00 Z{}", f(clearance_z)));

    for (i, &(z, rho)) in passes.iter().enumerate() {
        let start_x = cx + rho;

        if i == 0 {
            // Apex (rho ~ 0): rapid over the top, then plunge to the apex point.
            g.push(format!("G00 X{} Y{}", f(start_x), f(cy)));
            g.push(format!("G01 Z{} F{}", f(z), f(plunge)));
        } else {
            // Safe transition down to the next, wider level without cutting a
            // chord through the convex interior (which would gouge): first feed
            // radially OUT to the new radius at the current height — into clear
            // air outside the surface — then plunge straight down onto the level.
            g.push(format!("G01 X{} Y{} F{}", f(start_x), f(cy), f(feed)));
            g.push(format!("G01 Z{} F{}", f(z), f(plunge)));
        }

        // Cut the full waterline circle. Below one chord-tolerance of radius the
        // ring collapses to the point we already plunged to, so skip it.
        if rho > CHORD_TOL_MM {
            g.extend(ring_moves(op.compress, false, cx, cy, rho, feed, true));
        }
    }

    g.push(format!("G00 Z{}", f(clearance_z)));
    g
}

// Same ring-per-pass shape as generate_passes above, but every pass retracts
// to clearance_Z first instead of feeding directly to the next radius at the
// current height. generate_passes' faster transition is only proven
// gouge-free for a single concentric-dilated-sphere offset locus (the
// ball-nose case); finish_sphere_profile's composite, per-vertex locus isn't
// that shape, so it uses this conservative sibling instead of that unproven
// shortcut (see the call site in finish_sphere_profile for the full
// rationale). Retracting to clearance_Z is safe unconditionally, for any
// tool shape, because clearance_Z is defined to be clear of the stock/part.
fn generate_passes_retract_between(p: &Part, op: &Operation, title: String, cx: f64, cy: f64, passes: Vec<(f64, f64)>) -> Vec<String> {
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let mut g = Vec::new();
    g.push(title);
    g.push(format!("G00 Z{}", f(clearance_z)));

    for &(z, rho) in passes.iter() {
        let start_x = cx + rho;
        g.push(format!("G00 X{} Y{}", f(start_x), f(cy)));
        g.push(format!("G01 Z{} F{}", f(z), f(plunge)));

        if rho > CHORD_TOL_MM {
            g.extend(ring_moves(op.compress, false, cx, cy, rho, feed, true));
        }

        g.push(format!("G00 Z{}", f(clearance_z)));
    }

    g
}

// One full waterline ring, emitted as chords or a single exact-circle arc per
// the op's `compress` mode (DECISIONS.md "Per-op compress…"). The tool is
// assumed to already sit at the ring's start point (cx + rho, cy) — angle 0,
// the +X vertex — exactly as circle_pts/circle_pts_cw assume, so this is a
// drop-in for the `for (x, y) in circle_pts…` loops. `cw` picks the winding:
// false = CCW (the circle_pts rings — waterlines, bore), true = CW
// (circle_pts_cw — boss). `chord_feed` writes the F word on each chorded point
// (the engine-mode rings) or omits it (the control-comp rings, where feed is
// modal from the lead-in); the arc always carries F via the shared primitive.
//
// Arcs mode emits ONE full-circle G02/G03 block: start == end == (cx + rho, cy),
// so the centre is I = -rho, J = 0 relative to it. Chosen over quadrant or
// semicircle splits because it is minimal, exact, and — decisively — keeps the
// single existing seam witness point at angle 0 rather than multiplying it ×4
// (deferring cleanly to ROADMAP.md "Waterline seam scattering"). The IJ
// full-circle form is read by the mainstream targets (Fanuc/Haas/LinuxCNC/Grbl);
// the split forms are the portability fallback if a controller rejects it, left
// unbuilt. Callers keep the `rho > CHORD_TOL_MM` guard, so no degenerate ring
// reaches here.
fn ring_moves(compress: CompressMode, cw: bool, cx: f64, cy: f64, rho: f64, feed: f64, chord_feed: bool) -> Vec<String> {
    match compress {
        CompressMode::Arcs => vec![crate::gcode::arc(cw, cx + rho, cy, -rho, 0.0, feed)],
        CompressMode::Chords => {
            let pts = if cw { circle_pts_cw(cx, cy, rho) } else { circle_pts(cx, cy, rho) };
            pts.into_iter()
                .map(|(x, y)| {
                    if chord_feed {
                        format!("G01 X{} Y{} F{}", f(x), f(y), f(feed))
                    } else {
                        format!("G01 X{} Y{}", f(x), f(y))
                    }
                })
                .collect()
        }
        // Parse rejects force/spline before an Operation is built, so an
        // Operation carrying them can never reach emission.
        CompressMode::Force | CompressMode::Spline => {
            unreachable!("force/spline are reserved and rejected at parse")
        }
    }
}

// Points around a full circle, flattened so each chord's sagitta stays within
// CHORD_TOL_MM. The tool is assumed to already sit at angle 0 (start_x, cy), so
// this emits angles from the first step through 2*pi (== start), closing the
// loop and finishing back at angle 0.
pub(crate) fn circle_pts(cx: f64, cy: f64, rho: f64) -> Vec<(f64, f64)> {
    let m = circle_segments(rho);
    let mut v = Vec::with_capacity(m);
    for k in 1..=m {
        let a = 2.0 * PI * (k as f64) / (m as f64);
        v.push((cx + rho * a.pcos(), cy + rho * a.psin()));
    }
    v
}

// CW sibling of circle_pts, for the outside-boss passes (see finish_cylinder_boss's
// cut-direction note): same segment count and same start/end point at angle 0,
// traversed in the opposite direction.
pub(crate) fn circle_pts_cw(cx: f64, cy: f64, rho: f64) -> Vec<(f64, f64)> {
    let m = circle_segments(rho);
    let mut v = Vec::with_capacity(m);
    for k in 1..=m {
        let a = -2.0 * PI * (k as f64) / (m as f64);
        v.push((cx + rho * a.pcos(), cy + rho * a.psin()));
    }
    v
}

// One point of the axis-aligned ellipse parallel at signed offset `r_signed`
// (+ outward, − inward, 0 the ellipse itself), by normal angle `phi`: the
// ellipse point whose outward normal is (cos phi, sin phi) is t = atan2(
// b·sin phi, a·cos phi) (from tan t = (b/a)·tan phi, quadrant-correct), and
// the parallel point is P(t) + r_signed·(cos phi, sin phi) — closed-form per
// point, which is all R2 promises. At a == b, t == phi and this is the circle
// point at radius a + r_signed.
// `rot` is the rotated superset's (cos θ, sin θ); None is the axis-aligned
// form, kept as its own arm (not Some((1, 0))) so the original expressions —
// and therefore the reduces-to anchors' exact g-code equality — survive
// unchanged.
pub(crate) fn ellipse_offset_pt(cx: f64, cy: f64, a: f64, b: f64, r_signed: f64, phi: f64, rot: Option<(f64, f64)>) -> (f64, f64) {
    let t = (b * phi.psin()).patan2(a * phi.pcos());
    match rot {
        None => (
            cx + a * t.pcos() + r_signed * phi.pcos(),
            cy + b * t.psin() + r_signed * phi.psin(),
        ),
        Some((cos_r, sin_r)) => {
            let px = a * t.pcos() + r_signed * phi.pcos();
            let py = b * t.psin() + r_signed * phi.psin();
            (cx + px * cos_r - py * sin_r, cy + px * sin_r + py * cos_r)
        }
    }
}

// Points around a full ellipse parallel, flattened within CHORD_TOL_MM —
// circle_pts' contract (start assumed at angle 0, the +X major vertex; emit
// from the first step through 2*pi, closing the loop). Uniform steps of the
// NORMAL angle: a uniform normal-angle step is a uniform turning angle, so
// each chord's sagitta is R_local·(1 − cos(step/2)) with R_local the
// parallel's own radius of curvature (the ellipse's, a²/b at the slackest
// vertices to b²/a at the tightest, shifted by r_signed — parallels shift
// curvature radii exactly). Sizing the step by the SLACKEST radius
// (a²/b + r_signed) through circle_segments bounds every sagitta by the
// tolerance, at the cost of oversampling the tight major-axis vertices by up
// to ~(a/b)³ — deliberately the simple conservative version, revisit only if
// point counts hurt. At a == b this emits exactly circle_pts(a + r_signed),
// the reduces-to-cylinder/boss anchor.
// pub(crate) since Phase 6 slice 2: the feature wireframe (view::feature_wires)
// is the second consumer, at r_signed = 0 — the nominal ellipse wall.
pub(crate) fn ellipse_offset_pts(cx: f64, cy: f64, a: f64, b: f64, r_signed: f64, rot: Option<(f64, f64)>) -> Vec<(f64, f64)> {
    let m = circle_segments(a * a / b + r_signed);
    let mut v = Vec::with_capacity(m);
    for k in 1..=m {
        let phi = 2.0 * PI * (k as f64) / (m as f64);
        v.push(ellipse_offset_pt(cx, cy, a, b, r_signed, phi, rot));
    }
    v
}

// CW sibling of ellipse_offset_pts, for the outside-boss rings (see
// finish_cylinder_boss's cut-direction note): same segment count and same start/end
// point at the +X major vertex, traversed in the opposite direction.

// Segment count for a circle of radius `rho` at the shared chord tolerance.
// Sagitta rho*(1 - cos(theta/2)) <= tol  =>  theta <= 2*acos(1 - tol/rho).
pub(crate) fn circle_segments(rho: f64) -> usize {
    let ratio = (1.0 - CHORD_TOL_MM / rho).clamp(-1.0, 1.0);
    let theta = 2.0 * ratio.pacos();
    ((2.0 * PI / theta).ceil() as usize).max(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Feature, Operation, Part, Tool, CompMode, CompressMode};

    // A minimal Part carrying one Ballmill + one Finish op, so finish_ellipsoid
    // can be exercised directly (the g-code writer only exposes flattened text).

    // A minimal Part carrying one Ballmill and one Copy op over a sphere of
    // the given diameter, for the revolve tests that build their own feature
    // from the returned Part and Operation (the second argument is unused and
    // kept so the callers read as before).
    fn revolve_part(dia: f64, _height: f64, r_ball: f64) -> (Part, Operation, Feature) {
        let mut p = Part::default();
        p.tools.insert(9, Tool { num: 9, name: "Ballmill".into(), dia: r_ball * 2.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        let feature = Feature::Sphere { dia, x: 50.0, y: 40.0, z: 0.0 };
        let op = Operation {
            op_type: crate::parse::OpType::Copy,
            tool_num: 9,
            feature_name: "Dome".into(),
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
        };
        (p, op, feature)
    }

    // Recover the (z, rho) pass table finish_ellipsoid built, by reading back
    // the plunge Z and the ring radius (start_x - cx) from the emitted g-code.
    // Every pass is "G01 Z{z} F{plunge}" followed by a ring whose first point
    // is at (cx + rho, cy); the apex ring collapses so we read rho from the
    // G00 X move that precedes each plunge instead.
    fn passes_from_gcode(g: &[String], cx: f64) -> Vec<(f64, f64)> {
        // Each pass sets its radius with an X move (apex: "G00 X"; later passes
        // feed out with "G01 X … F") immediately followed by "G01 Z{z}" plunge.
        // Track the most recent X move and commit it when the plunge lands.
        let mut out = Vec::new();
        let mut pending_rho: Option<f64> = None;
        for line in g {
            let x_move = line.strip_prefix("G00 X").or_else(|| line.strip_prefix("G01 X"));
            if let Some(rest) = x_move {
                let x: f64 = rest.split_whitespace().next().unwrap().parse().unwrap();
                pending_rho = Some(x - cx);
            } else if let Some(rest) = line.strip_prefix("G01 Z") {
                let z: f64 = rest.split_whitespace().next().unwrap().parse().unwrap();
                if let Some(rho) = pending_rho.take() {
                    out.push((z, rho));
                }
            }
        }
        out
    }

    // The load-bearing correctness anchor (documented on finish_ellipsoid and
    // the Feature::Ellipsoid variant): when the polar semi-axis equals the
    // equatorial radius the spheroid IS a sphere, so every ball-centre point
    // must land on the concentric sphere of radius r_part + r_ball that
    // finish_sphere rides — proving the pointwise-normal offset reduces to the
    // R1 offset-closure result in the limit.

    // Build a minimal Part carrying one Ballmill + one Finish(stepover) op over
    // a SplineRevolve of the given meridian, so finish_spline_revolve can be
    // exercised directly (mirrors ellipsoid_part).
    fn spline_revolve_part(
        meridian: crate::nurbs::BSplineCurve,
        r_ball: f64,
    ) -> (Part, Operation, Feature) {
        let mut p = Part::default();
        p.global_feed = 600.0;
        p.global_plunge = 200.0;
        p.global_clearance_z = 40.0;
        p.tools.insert(10, Tool { num: 10, name: "Ballmill".into(), dia: 2.0 * r_ball, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        let feature = Feature::SplineRevolve { meridian, x: 50.0, y: 40.0, z: 0.0, file: "test".into() };
        p.features.insert("Cap".into(), feature.clone());
        let op = Operation {
            op_type: crate::parse::OpType::Copy,
            tool_num: 10,
            feature_name: "Cap".into(),
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
        };
        (p, op, feature)
    }

    // The load-bearing correctness anchor, the SplineRevolve twin of
    // test_ellipsoid_reduces_to_sphere: when the meridian is a circular arc
    // (here the exact rational quarter circle of radius R), the surface IS a
    // sphere, so every ball-centre point must ride the concentric sphere of
    // radius R + r_ball. This pins the pointwise spline offset to the R1
    // offset-closure result the same way the ellipsoid test does — an
    // independent check the self-generated golden can't give.
    #[test]
    fn test_spline_revolve_reduces_to_sphere() {
        let (r, r_ball) = (20.0, 3.0);
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        // Apex-first quarter circle centred at the meridian origin: apex (0, R)
        // to base (R, 0), so r ↑ and z ↓ down the cap.
        let meridian = crate::nurbs::BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(0.0, r), (r, r), (r, 0.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        let (p, op, feature) = spline_revolve_part(meridian, r_ball);
        let g = finish_spline_revolve(&p, &op, &feature).unwrap();
        let r_off = r + r_ball;
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 2, "expected several passes, got {}", passes.len());
        for (z, rho) in passes {
            // z0 = 0, so the CL point (rho, z) must satisfy rho² + z² = r_off².
            let residual = (rho * rho + z * z).sqrt() - r_off;
            assert!(residual.abs() < 2e-3, "CL point ({rho}, {z}) off the concentric sphere by {residual}");
        }
    }

    // The gouge guard end to end: a meridian that is a circular arc concave
    // toward the tool (an outward flare) of radius 10 — a ball within it
    // machines, a ball larger than it is rejected before any g-code, exactly
    // like the vase neck / paraboloid bowl guards. The concave radius comes from
    // the spline's exact second derivative, not a sampled estimate.
    #[test]
    fn test_spline_revolve_concave_gouge_guard() {
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        // Arc centred at (20, 20), radius 10, from (10, 20) down to (20, 10):
        // r ↑, z ↓, and the tool sits on the centre side, so it is concave
        // toward the tool with radius 10.
        let make = || {
            crate::nurbs::BSplineCurve::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![(10.0, 20.0), (10.0, 10.0), (20.0, 10.0)],
                Some(vec![1.0, w, 1.0]),
            )
            .unwrap()
        };
        // A 3mm-radius ball fits the 10mm cove.
        let (p, op, feature) = spline_revolve_part(make(), 3.0);
        assert!(finish_spline_revolve(&p, &op, &feature).is_ok(), "a ball within the concave radius must machine");
        // A 12mm-radius ball cannot — the offset arc would self-intersect.
        let (p, op, feature) = spline_revolve_part(make(), 12.0);
        let err = finish_spline_revolve(&p, &op, &feature).unwrap_err();
        assert!(err.contains("gouge"), "an oversized ball must be rejected naming the gouge: {err}");
        assert!(err.contains("concave"), "the error should name the concave meridian: {err}");
    }

    // The load-bearing anchor for the *constant-scallop* branch, the
    // SplineRevolve twin of test_ellipsoid_scallop_reduces_to_sphere: when the
    // meridian is the exact rational quarter circle of radius R, the surface IS
    // the sphere, so (a) every CL point the march lands on must ride the
    // concentric sphere of radius R + r_ball, and (b) — the spacing half, which
    // (a) alone can't pin — consecutive CL points must subtend the closed-form
    // R1 cusp angle derive_scallop_angle(h, r_ball, R) about the centre, every
    // step at the full angle except the final clamp to the base rim.
    #[test]
    fn test_spline_revolve_scallop_reduces_to_sphere() {
        let (r, r_ball) = (20.0, 3.0);
        let scallop = 0.5;
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        let meridian = crate::nurbs::BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(0.0, r), (r, r), (r, 0.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        let (p, mut op, feature) = spline_revolve_part(meridian, r_ball);
        op.stepover = None;
        op.scallop = Some(scallop);
        let g = finish_spline_revolve(&p, &op, &feature).unwrap();
        let r_off = r + r_ball;
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 2, "expected several passes, got {}", passes.len());
        for &(z, rho) in &passes {
            let residual = (rho * rho + z * z).sqrt() - r_off;
            assert!(residual.abs() < 2e-3, "scallop CL point ({rho}, {z}) off the concentric sphere by {residual}");
        }
        // The spacing pin: polar angle apex -> rim, deltas == the R1 angle.
        let theta = crate::parse::derive_scallop_angle(scallop, r_ball, r);
        let angles: Vec<f64> = passes.iter().map(|&(z, rho)| rho.patan2(z)).collect();
        for pair in angles.windows(2) {
            let d = pair[1] - pair[0];
            assert!(d > 0.0, "march must advance apex -> rim, got step {d}");
            assert!(d <= theta + 1e-3, "step {d} exceeds the R1 cusp angle {theta}");
        }
        for pair in angles[..angles.len() - 1].windows(2) {
            let d = pair[1] - pair[0];
            assert!((d - theta).abs() < 1e-3, "interior step {d} must be the full R1 cusp angle {theta}");
        }
    }

    // Mixed curvature end to end: a bell-shaped meridian — convex dome at the
    // apex, inflecting into a concave outward flare at the base, still a
    // machinable cap (r up, z down) — has no closed-form spacing to pin, so pin
    // the *contract* instead: the march completes apex to rim, and between every
    // adjacent pass pair the actual cusp left standing (the intersection of the
    // two ball circles nearer the surface) is within budget of the requested
    // scallop. Tolerance 1.2x covers the documented second-order drift of
    // current-point curvature evaluation (the family behavior, not a bug).
    #[test]
    fn test_spline_revolve_scallop_mixed_curvature_marches_through_inflection() {
        let scallop = 0.2;
        let r_ball = 1.5;
        let make = || {
            crate::nurbs::BSplineCurve::new(
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
                vec![(0.0, 20.0), (2.0, 19.0), (4.0, 10.0), (6.0, 3.0), (10.0, 1.0), (14.0, 0.0)],
                None,
            )
            .unwrap()
        };
        // The fixture must actually cross an inflection, or the test pins less
        // than it claims.
        let meridian = make();
        let (u0, u1) = meridian.domain();
        let (mut saw_convex, mut saw_concave) = (false, false);
        for k in 0..=200 {
            let u = u0 + (u1 - u0) * (k as f64 / 200.0);
            if let Some(kappa) = meridian.curvature_toward_up_normal(u) {
                if kappa < -1e-6 { saw_convex = true; }
                if kappa > 1e-6 { saw_concave = true; }
            }
        }
        assert!(saw_convex && saw_concave, "bell meridian must carry both curvature signs");

        let (p, mut op, feature) = spline_revolve_part(make(), r_ball);
        op.stepover = None;
        op.scallop = Some(scallop);
        let g = finish_spline_revolve(&p, &op, &feature).unwrap();
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 5, "expected a real march, got {} passes", passes.len());

        // First pass at the apex CL, last at the base-rim CL (the u1 clamp).
        let meridian = make();
        let cl = |u: f64| crate::parse::spline_revolve_cl_point(&meridian, r_ball, u);
        let (rho_a, z_a) = cl(u0);
        assert!((passes[0].1 - rho_a).abs() < 2e-3 && (passes[0].0 - z_a).abs() < 2e-3, "first pass must sit at the apex CL");
        let (rho_b, z_b) = cl(u1);
        let last = passes[passes.len() - 1];
        assert!((last.1 - rho_b).abs() < 2e-3 && (last.0 - z_b).abs() < 2e-3, "last pass must clamp to the base-rim CL");

        // Cusp budget per adjacent pair: intersect the two r_ball circles about
        // the CL points in the meridian plane, take the intersection nearer the
        // surface, and measure its distance to the meridian by dense sampling.
        let surf_dist = |pt: (f64, f64)| -> f64 {
            let mut best = f64::INFINITY;
            for k in 0..=2000 {
                let u = u0 + (u1 - u0) * (k as f64 / 2000.0);
                let (r, z) = meridian.eval(u);
                let d = ((pt.0 - r).powi(2) + (pt.1 - z).powi(2)).sqrt();
                best = best.min(d);
            }
            best
        };
        for pair in passes.windows(2) {
            let (c1, c2) = ((pair[0].1, pair[0].0), (pair[1].1, pair[1].0));
            let d = ((c2.0 - c1.0).powi(2) + (c2.1 - c1.1).powi(2)).sqrt();
            if d < 1e-6 {
                continue; // duplicate final pass — no cusp between coincident rings
            }
            assert!(d <= 2.0 * r_ball + 1e-6, "adjacent CL rings too far apart to leave a cusp: {d}");
            let mid = ((c1.0 + c2.0) / 2.0, (c1.1 + c2.1) / 2.0);
            let half = (r_ball * r_ball - (d / 2.0) * (d / 2.0)).max(0.0).sqrt();
            let perp = (-(c2.1 - c1.1) / d, (c2.0 - c1.0) / d);
            let p1 = (mid.0 + half * perp.0, mid.1 + half * perp.1);
            let p2 = (mid.0 - half * perp.0, mid.1 - half * perp.1);
            // The cusp ridge peaks at the circle intersection nearer the
            // surface; its height IS that point's distance to the surface
            // (in the sphere limit exactly the R1 construction's r_part +
            // scallop locus).
            let cusp = surf_dist(p1).min(surf_dist(p2));
            assert!(
                cusp <= 1.2 * scallop + 1e-3,
                "cusp {cusp} exceeds the scallop budget {scallop} between ({}, {}) and ({}, {})",
                c1.0, c1.1, c2.0, c2.1
            );
        }
    }

    // The physical ceiling, the cone's rule verbatim: a scallop request above
    // the ball radius has no real cusp geometry and is a hard error naming
    // feature, tool, and both numbers — not a silent clamp.
    #[test]
    fn test_spline_revolve_scallop_exceeding_ball_radius_is_refused() {
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        let meridian = crate::nurbs::BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(0.0, 20.0), (20.0, 20.0), (20.0, 0.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        let (p, mut op, feature) = spline_revolve_part(meridian, 3.0);
        op.stepover = None;
        op.scallop = Some(3.5);
        let err = finish_spline_revolve(&p, &op, &feature).unwrap_err();
        assert!(err.contains("exceeds") && err.contains("ball radius"), "the error must name the ceiling: {err}");
        assert!(err.contains("3.500") && err.contains("3.000"), "the error must carry both numbers: {err}");
    }

    // The freeform cubic the spline_cap fixtures read from disk, rebuilt
    // inline: apex (0, 20) on the axis, base rim (13, 0), monotone both ways.
    fn freeform_cap_meridian() -> crate::nurbs::BSplineCurve {
        crate::nurbs::BSplineCurve::new(
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
            vec![(0.0, 20.0), (3.0, 19.0), (7.0, 16.0), (10.0, 11.0), (12.0, 5.0), (13.0, 0.0)],
            None,
        )
        .unwrap()
    }

    // Brute-force distance to the nominal spline meridian (dense sampling —
    // independent of parse::spline_meridian_distance's seeded Newton, which
    // is part of the machinery under test).
    fn spline_meridian_distance_dense(meridian: &crate::nurbs::BSplineCurve, rho: f64, z: f64) -> f64 {
        let (u0, u1) = meridian.domain();
        let n = 20_000;
        (0..=n)
            .map(|i| {
                let (r, zz) = meridian.eval(u0 + (u1 - u0) * (i as f64 / n as f64));
                ((r - rho).powi(2) + (zz - z).powi(2)).sqrt()
            })
            .fold(f64::MAX, f64::min)
    }

    // The NOMINAL scallop promise on the user-authored meridian (DECISIONS.md
    // "Family-wide march cap…") — the carrier with the strongest claim on the
    // cap, since the mixed-meridians slice had already rejected to let this
    // same osculating triangle price an allowance. Before the cap the fixture
    // cap priced 0.2053mm on a 0.200mm request (+2.6%), 0.0507 on 0.050,
    // 0.0202 on 0.020; measured here against the dense brute force, not
    // against the triangle.
    #[test]
    fn test_spline_nominal_scallop_march_honours_its_promise() {
        let r_ball = 3.0;
        let meridian = freeform_cap_meridian();
        for scallop in [0.2, 0.05, 0.02] {
            let (p, mut op, feature) = spline_revolve_part(meridian.clone(), r_ball);
            op.stepover = None;
            op.scallop = Some(scallop);
            let g = finish_spline_revolve(&p, &op, &feature)
                .unwrap_or_else(|e| panic!("nominal finish must succeed (scallop {scallop}): {e}"));
            let passes = passes_from_gcode(&g, 50.0);
            assert_cusp_tips_within_allowance(&passes, r_ball, scallop, |rho, z| {
                spline_meridian_distance_dense(&meridian, rho, z)
            });
        }
    }

    // The cap's worth on the spline cap, in numbers — the throat's pin on the
    // user-authored meridian, and stack-level for the same reason: the
    // pointwise-curvature triangle's FIRST step actually prices 0.1979 on a
    // 0.200 request (under, not over), and the 0.2053 overshoot appears
    // further down the flare where curvature drifts hardest across a step. A
    // first-pair assertion would have passed while the part shipped a
    // 0.205mm ridge, which is the whole failure mode this slice removes.
    #[test]
    fn test_spline_seed_stack_overshoots_and_the_cap_holds() {
        let r_ball = 3.0;
        let meridian = freeform_cap_meridian();
        let (u0, u1) = meridian.domain();
        let walk = |scallop: f64, capped: bool| -> Vec<f64> {
            let mut us = vec![u0];
            let mut u = u0;
            while u < u1 {
                let un = if capped {
                    crate::parse::derive_spline_revolve_capped_step_u(scallop, r_ball, &meridian, u, 0.0)
                } else {
                    crate::parse::derive_spline_revolve_step_u(scallop, r_ball, &meridian, u, 0.0)
                };
                u = if un > u && un <= u1 { un } else { u1 };
                us.push(u);
            }
            us
        };
        for (scallop, was) in [(0.2_f64, 0.2053_f64), (0.05, 0.0507)] {
            let seed_price = crate::parse::derive_spline_march_cusp(&meridian, r_ball, 0.0, &walk(scallop, false));
            let capped_price = crate::parse::derive_spline_march_cusp(&meridian, r_ball, 0.0, &walk(scallop, true));
            assert!(
                (seed_price - was).abs() < 5e-4,
                "the uncapped stack's measured overshoot should still be {was}, got {seed_price}"
            );
            assert!(capped_price <= scallop + 1e-9, "capped stack price {capped_price} must hold {scallop}");
        }
    }

    // The finisher's own budget arithmetic, reconstructed: sweep the exact
    // tip price of the march grid over 5 skins, exactly as the scallop arm
    // does (the stepover twin below sweeps derive_spline_stack_cusp).
    fn spline_scallop_swept_budget(meridian: &crate::nurbs::BSplineCurve, r_ball: f64, scallop: f64, allowance: f64) -> f64 {
        let (u0, u1) = meridian.domain();
        let march = |s: f64| -> Vec<f64> {
            let mut us = vec![u0];
            let mut u = u0;
            while u < u1 {
                let u_next = crate::parse::derive_spline_revolve_capped_step_u(scallop, r_ball, meridian, u, s);
                u = if u_next > u && u_next <= u1 { u_next } else { u1 };
                us.push(u);
            }
            us
        };
        let mut h: f64 = 0.0;
        for k in 0..=4 {
            let s = allowance * k as f64 / 4.0;
            h = h.max(crate::parse::derive_spline_march_cusp(meridian, r_ball, s, &march(s)));
        }
        h
    }

    // Scallop-mode roughing on the user-authored meridian: the march's
    // osculating triangle is only the spacing heuristic — the emitted stack
    // must ride the inflated offset exactly AND keep every actual cusp tip
    // within the allowance, with the budget being the VERIFIED tip price,
    // never the scallop by fiat. Since the tip cap landed (DECISIONS.md
    // "Family-wide march cap…") that verified price no longer PAYS for the
    // drift: the march grid is capped at the scallop, so h lands at or just
    // under 0.2 where it used to price 0.205, and the extra 0.005 goes back
    // to the nominal skin. The check still reads the price rather than
    // assuming it — the budget is whatever the walk verifies.
    #[test]
    fn test_spline_rough_scallop_rides_inflated_offset_and_verifies_tips() {
        let (r_ball, scallop, allowance) = (3.0, 0.2, 0.5);
        let meridian = freeform_cap_meridian();
        let (p, mut op, feature) = spline_revolve_part(meridian.clone(), r_ball);
        op.stepover = None;
        op.scallop = Some(scallop);
        op.stock_to_leave = Some(allowance);
        let h = spline_scallop_swept_budget(&meridian, r_ball, scallop, allowance);
        assert!(h > 0.0 && h <= scallop + 1e-9, "the capped march's verified budget must hold the scallop, got {h}");
        let r_off = r_ball + (allowance - h);
        let g = finish_spline_revolve(&p, &op, &feature).expect("spline roughing must succeed");
        let passes = passes_from_gcode(&g, 50.0);
        for &(z, rho) in &passes {
            let d = spline_meridian_distance_dense(&meridian, rho, z);
            assert!((d - r_off).abs() < 2e-3, "CL point ({rho}, {z}) is {d}mm off the meridian, expected {r_off}");
        }
        assert_cusp_tips_within_allowance(&passes, r_ball, allowance, |rho, z| {
            spline_meridian_distance_dense(&meridian, rho, z)
        });
    }

    // Waterline roughing on the same meridian: uniform arc on the INFLATED
    // curve, sweep-then-verified through derive_spline_stack_cusp.
    #[test]
    fn test_spline_rough_stepover_rides_inflated_offset_and_verifies_tips() {
        let (r_ball, stepover, allowance) = (3.0, 3.0, 0.5);
        let meridian = freeform_cap_meridian();
        let (p, mut op, feature) = spline_revolve_part(meridian.clone(), r_ball);
        op.stepover = Some(stepover);
        op.stock_to_leave = Some(allowance);
        let mut h: f64 = 0.0;
        for k in 0..=4 {
            h = h.max(crate::parse::derive_spline_stack_cusp(&meridian, r_ball, allowance * k as f64 / 4.0, stepover));
        }
        assert!(h.is_finite() && h > 0.0 && h < allowance, "fixture price must spend part of the ceiling, got {h}");
        let r_off = r_ball + (allowance - h);
        let g = finish_spline_revolve(&p, &op, &feature).expect("spline roughing must succeed");
        let passes = passes_from_gcode(&g, 50.0);
        for &(z, rho) in &passes {
            let d = spline_meridian_distance_dense(&meridian, rho, z);
            assert!((d - r_off).abs() < 2e-3, "CL point ({rho}, {z}) is {d}mm off the meridian, expected {r_off}");
        }
        assert_cusp_tips_within_allowance(&passes, r_ball, allowance, |rho, z| {
            spline_meridian_distance_dense(&meridian, rho, z)
        });
    }

    // The R1 anchor under stock: a rational quarter-circle meridian roughed
    // at allowance a' must ride the concentric sphere R + r_ball + a' — the
    // offset-closure reduction that pins every spline formula, now with the
    // skin composed into the same single normal offset.
    #[test]
    fn test_spline_rough_reduces_to_concentric_sphere_with_skin() {
        let (r, r_ball, stepover, allowance) = (20.0, 3.0, 3.0, 0.5);
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        let meridian = crate::nurbs::BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(0.0, r), (r, r), (r, 0.0)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap();
        let mut h: f64 = 0.0;
        for k in 0..=4 {
            h = h.max(crate::parse::derive_spline_stack_cusp(&meridian, r_ball, allowance * k as f64 / 4.0, stepover));
        }
        assert!(h.is_finite() && h > 0.0 && h < allowance, "the sphere grid must price part of the ceiling, got {h}");
        let (p, mut op, feature) = spline_revolve_part(meridian, r_ball);
        op.stock_to_leave = Some(allowance);
        let g = finish_spline_revolve(&p, &op, &feature).expect("spline roughing must succeed");
        let r_off = r + r_ball + (allowance - h);
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 2, "expected several passes, got {}", passes.len());
        for (z, rho) in passes {
            let residual = (rho * rho + z * z).sqrt() - r_off;
            assert!(residual.abs() < 2e-3, "CL point ({rho}, {z}) off the concentric sphere by {residual}");
        }
    }

    // A stepover past the ball diameter leaves adjacent balls that never
    // intersect: an uncut RIDGE no allowance can ceiling — the INFINITY
    // path through the tip construction.
    #[test]
    fn test_spline_rough_stepover_ridge_is_refused() {
        let (p, mut op, feature) = spline_revolve_part(freeform_cap_meridian(), 3.0);
        op.stepover = Some(6.5);
        op.stock_to_leave = Some(0.5);
        let err = finish_spline_revolve(&p, &op, &feature).unwrap_err();
        assert!(err.contains("do not overlap") && err.contains("uncut ridge"), "the error must name the ridge: {err}");
    }

    // The knife-edge the verified budget exists for, and the one place this
    // slice MOVED it (DECISIONS.md "Family-wide march cap…"). Before the tip
    // cap, an allowance of 0.201 against a 0.2 scallop rejected: the drift
    // priced the grid at 0.205, past the ceiling. Capped, the grid prices
    // 0.200 and that request is legal — see the test below, which pins it.
    // What still rejects is an allowance the capped cusp alone consumes: at
    // allowance == scallop there is nothing left for a nominal skin, and the
    // error must carry the verified number rather than breach silently.
    // (parse::validate rejects this shape upstream with its own message; this
    // arm is the fail-safe underneath, and the direct call is how it's
    // reached.)
    #[test]
    fn test_spline_rough_scallop_verified_cusp_above_allowance_is_refused() {
        let (p, mut op, feature) = spline_revolve_part(freeform_cap_meridian(), 3.0);
        op.stepover = None;
        op.scallop = Some(0.2);
        op.stock_to_leave = Some(0.2);
        let err = finish_spline_revolve(&p, &op, &feature).unwrap_err();
        assert!(err.contains("verified cusp"), "the error must carry the verified price: {err}");
        assert!(err.contains("max, not min"), "the error must restate the semantics: {err}");
    }

    // The other side of that moved knife-edge, pinned so the gain is not
    // just a churned golden: the capped march spends exactly its scallop, so
    // a 0.201mm allowance against a 0.2mm scallop now buys a real (if thin)
    // nominal skin where the drift used to eat the whole ceiling. The title
    // audits both halves.
    #[test]
    fn test_spline_rough_scallop_capped_budget_frees_the_drift() {
        let (p, mut op, feature) = spline_revolve_part(freeform_cap_meridian(), 3.0);
        op.stepover = None;
        op.scallop = Some(0.2);
        op.stock_to_leave = Some(0.201);
        let g = finish_spline_revolve(&p, &op, &feature)
            .expect("the capped march holds 0.2, so a 0.201 ceiling fits");
        assert!(
            g.iter().any(|l| l.contains("[stock_to_leave 0.201mm, cusp budget 0.200mm]")),
            "title must audit a cusp budget at the scallop, not above it: {:?}",
            g.first()
        );
    }

    fn spline_revolve_funnel_part(
        meridian: crate::nurbs::BSplineCurve,
        r_ball: f64,
    ) -> (Part, Operation, Feature) {
        let mut p = Part::default();
        p.global_feed = 600.0;
        p.global_plunge = 200.0;
        p.global_clearance_z = 40.0;
        // stock.z stays the default 0, so the mouth plane is z = 0 and the
        // meridian's own z values read directly as world coordinates.
        p.tools.insert(10, Tool { num: 10, name: "Ballmill".into(), dia: 2.0 * r_ball, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        let feature = Feature::SplineRevolveFunnel { meridian, x: 50.0, y: 40.0, file: "test".into() };
        p.features.insert("Funnel".into(), feature.clone());
        let op = Operation {
            op_type: crate::parse::OpType::Copy,
            tool_num: 10,
            feature_name: "Funnel".into(),
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
        };
        (p, op, feature)
    }

    // A mouth-first quarter circle centred at the meridian origin: mouth
    // (R, 0) down to the bottom pole (0, −R) — the exact hemispherical BOWL
    // of radius R.
    fn bowl_meridian(r: f64) -> crate::nurbs::BSplineCurve {
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        crate::nurbs::BSplineCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![(r, 0.0), (r, -r), (0.0, -r)],
            Some(vec![1.0, w, 1.0]),
        )
        .unwrap()
    }

    // The funnel's load-bearing correctness anchor — the concave twin of
    // test_spline_revolve_reduces_to_sphere, and deliberately independent of
    // the (unimplemented) hemisphere-bowl op: when the meridian is the exact
    // rational quarter circle of radius R stored mouth-first, the cavity IS
    // the hemispherical bowl, so every ball-centre point must ride the
    // concentric INNER sphere of radius R − r_ball (the R1 offset-closure
    // result with the ball radius SUBTRACTED — the genuinely new closed form
    // the outside anchor can't pin). The mouth also pins the vertical-wall
    // tie-break fix: at the equator the tangent is exactly vertical, and the
    // into-void CL must sit at rho = R − r_ball (the cap rule's flip would
    // put it at R + r_ball, inside the material).
    #[test]
    fn test_spline_revolve_funnel_reduces_to_hemispherical_bowl() {
        let (r, r_ball) = (20.0, 3.0);
        let (p, op, feature) = spline_revolve_funnel_part(bowl_meridian(r), r_ball);
        let g = finish_spline_revolve_funnel(&p, &op, &feature).unwrap();
        let r_in = r - r_ball;
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 2, "expected several passes, got {}", passes.len());
        for &(z, rho) in &passes {
            // Mouth plane z = 0 is the sphere centre's height, so the CL point
            // (rho, z) must satisfy rho² + z² = (R − r_ball)².
            let residual = (rho * rho + z * z).sqrt() - r_in;
            assert!(residual.abs() < 2e-3, "CL point ({rho}, {z}) off the concentric inner sphere by {residual}");
        }
        // The mouth CL, exactly: (R − r_ball, 0) — the vertical-tangent pin.
        assert!((passes[0].1 - r_in).abs() < 2e-3 && passes[0].0.abs() < 2e-3,
            "mouth CL must sit one ball radius INSIDE the rim, got ({}, {})", passes[0].1, passes[0].0);
        // Mouth→tip IS top-first for a cavity: passes descend and narrow.
        for pair in passes.windows(2) {
            assert!(pair[1].0 <= pair[0].0 + 1e-9, "passes must descend: {:?}", pair);
            assert!(pair[1].1 <= pair[0].1 + 1e-9, "passes must narrow: {:?}", pair);
        }
    }

    // The scallop twin: same bowl, constant-scallop branch — (a) every march
    // CL point rides the inner sphere, (b) the spacing half: consecutive CL
    // points subtend the closed-form CONCAVE cusp angle
    // derive_scallop_angle_concave about the centre on the radius-(R − r_ball)
    // CL circle, every interior step at the full angle, final clamped.
    #[test]
    fn test_spline_revolve_funnel_scallop_reduces_to_bowl() {
        let (r, r_ball) = (20.0, 3.0);
        let scallop = 0.5;
        let (p, mut op, feature) = spline_revolve_funnel_part(bowl_meridian(r), r_ball);
        op.stepover = None;
        op.scallop = Some(scallop);
        let g = finish_spline_revolve_funnel(&p, &op, &feature).unwrap();
        let r_in = r - r_ball;
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 2, "expected several passes, got {}", passes.len());
        for &(z, rho) in &passes {
            let residual = (rho * rho + z * z).sqrt() - r_in;
            assert!(residual.abs() < 2e-3, "scallop CL point ({rho}, {z}) off the concentric inner sphere by {residual}");
        }
        // Spacing pin: polar angle from the mouth rim (φ = 0) down to the
        // bottom pole (φ = π/2), deltas == the concave R1 angle.
        let theta = crate::parse::derive_scallop_angle_concave(scallop, r_ball, r);
        let angles: Vec<f64> = passes.iter().map(|&(z, rho)| (-z).patan2(rho)).collect();
        for pair in angles.windows(2) {
            let d = pair[1] - pair[0];
            assert!(d > 0.0, "march must advance mouth -> tip, got step {d}");
            assert!(d <= theta + 1e-3, "step {d} exceeds the concave R1 cusp angle {theta}");
        }
        for pair in angles[..angles.len() - 1].windows(2) {
            let d = pair[1] - pair[0];
            assert!((d - theta).abs() < 1e-3, "interior step {d} must be the full concave R1 cusp angle {theta}");
        }
    }

    // The R1 anchor under stock, void side: the hemispherical bowl roughed at
    // allowance a' must ride the concentric INNER sphere R − r_ball − a' —
    // the funnel's offset-closure reduction with the skin composed into the
    // same single into-void offset (the cap anchor ADDS where this
    // subtracts: an inside carrier's inflation moves toward the axis).
    #[test]
    fn test_spline_funnel_rough_reduces_to_concentric_inner_sphere_with_skin() {
        let (r, r_ball, stepover, allowance) = (20.0, 3.0, 3.0, 0.5);
        let meridian = bowl_meridian(r);
        let mut h: f64 = 0.0;
        for k in 0..=4 {
            h = h.max(crate::parse::derive_spline_funnel_stack_cusp(&meridian, r_ball, allowance * k as f64 / 4.0, stepover));
        }
        assert!(h.is_finite() && h > 0.0 && h < allowance, "the bowl grid must price part of the ceiling, got {h}");
        let (p, mut op, feature) = spline_revolve_funnel_part(meridian, r_ball);
        op.stock_to_leave = Some(allowance);
        let g = finish_spline_revolve_funnel(&p, &op, &feature).expect("funnel roughing must succeed");
        assert!(g.iter().any(|l| l.contains("[stock_to_leave 0.500mm, cusp budget ")), "title must audit the budget split");
        let r_in = r - r_ball - (allowance - h);
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 2, "expected several passes, got {}", passes.len());
        for &(z, rho) in &passes {
            let residual = (rho * rho + z * z).sqrt() - r_in;
            assert!(residual.abs() < 2e-3, "CL point ({rho}, {z}) off the concentric inner sphere by {residual}");
        }
        // The mouth CL, exactly: one ball radius plus the skin INSIDE the rim.
        assert!((passes[0].1 - r_in).abs() < 2e-3 && passes[0].0.abs() < 2e-3,
            "mouth CL must sit r_ball + skin inside the rim, got ({}, {})", passes[0].1, passes[0].0);
    }

    // The NOMINAL scallop promise on the void side (DECISIONS.md "Family-wide
    // march cap…"), the cap's twin: measured against the dense brute force on
    // the mixed-curvature bell bowl, so both triangle signs are marched. The
    // fixture funnel priced +1.9% at 0.200mm before the cap.
    #[test]
    fn test_spline_funnel_nominal_scallop_march_honours_its_promise() {
        let r_ball = 1.5;
        let make = || {
            crate::nurbs::BSplineCurve::new(
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
                vec![(14.0, 0.0), (10.0, 0.0), (8.0, -4.0), (6.0, -10.0), (3.0, -12.0), (0.0, -12.0)],
                None,
            )
            .unwrap()
        };
        for scallop in [0.2, 0.05, 0.02] {
            let meridian = make();
            let (p, mut op, feature) = spline_revolve_funnel_part(make(), r_ball);
            op.stepover = None;
            op.scallop = Some(scallop);
            let g = finish_spline_revolve_funnel(&p, &op, &feature)
                .unwrap_or_else(|e| panic!("nominal finish must succeed (scallop {scallop}): {e}"));
            let passes = passes_from_gcode(&g, 50.0);
            assert_cusp_tips_within_allowance(&passes, r_ball, scallop, |rho, z| {
                spline_meridian_distance_dense(&meridian, rho, z)
            });
        }
    }

    // The user-authored cavity meridian end to end, both spacings: the
    // emitted stack rides the inflated into-void offset exactly and every
    // actual cusp tip stays within the allowance — the cap's verified-tips
    // contract on the funnel's own CL home (the mixed-curvature bell bowl
    // from the inflection test, so both triangle signs march under skin).
    #[test]
    fn test_spline_funnel_rough_rides_inflated_offset_and_verifies_tips() {
        let r_ball = 1.5;
        let allowance = 0.5;
        let make = || {
            crate::nurbs::BSplineCurve::new(
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
                vec![(14.0, 0.0), (10.0, 0.0), (8.0, -4.0), (6.0, -10.0), (3.0, -12.0), (0.0, -12.0)],
                None,
            )
            .unwrap()
        };
        for scallop in [None, Some(0.2)] {
            let meridian = make();
            let (u0, u1) = meridian.domain();
            let (p, mut op, feature) = spline_revolve_funnel_part(make(), r_ball);
            op.stock_to_leave = Some(allowance);
            let h = if let Some(scallop) = scallop {
                op.stepover = None;
                op.scallop = Some(scallop);
                let march = |s: f64| -> Vec<f64> {
                    let mut us = vec![u0];
                    let mut u = u0;
                    while u < u1 {
                        let u_next = crate::parse::derive_spline_revolve_funnel_capped_step_u(scallop, r_ball, &meridian, u, s);
                        u = if u_next > u && u_next <= u1 { u_next } else { u1 };
                        us.push(u);
                    }
                    us
                };
                let mut h: f64 = 0.0;
                for k in 0..=4 {
                    let s = allowance * k as f64 / 4.0;
                    h = h.max(crate::parse::derive_spline_funnel_march_cusp(&meridian, r_ball, s, &march(s)));
                }
                h
            } else {
                let stepover = 1.5;
                op.stepover = Some(stepover);
                let mut h: f64 = 0.0;
                for k in 0..=4 {
                    h = h.max(crate::parse::derive_spline_funnel_stack_cusp(&meridian, r_ball, allowance * k as f64 / 4.0, stepover));
                }
                h
            };
            assert!(h.is_finite() && h > 0.0 && h < allowance, "fixture price must spend part of the ceiling, got {h}");
            let r_off = r_ball + (allowance - h);
            let g = finish_spline_revolve_funnel(&p, &op, &feature).expect("funnel roughing must succeed");
            let passes = passes_from_gcode(&g, 50.0);
            for &(z, rho) in &passes {
                let d = spline_meridian_distance_dense(&meridian, rho, z);
                assert!((d - r_off).abs() < 2e-3, "CL point ({rho}, {z}) is {d}mm off the meridian, expected {r_off}");
            }
            assert_cusp_tips_within_allowance(&passes, r_ball, allowance, |rho, z| {
                spline_meridian_distance_dense(&meridian, rho, z)
            });
        }
    }

    // The two-direction guard's stock-aware form: the chamfer wall's hoop
    // bound (≈ 9mm at the tip) is unchanged, the roughing offset must clear
    // it — whichever direction bound tightest at nominal is what the fatter
    // offset reruns against.
    #[test]
    fn test_spline_funnel_rough_gouge_tightens_with_stock() {
        let make = || {
            crate::nurbs::BSplineCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![(10.0, 0.0), (9.0, -20.0)],
                None,
            )
            .unwrap()
        };
        let (p, mut op, feature) = spline_revolve_funnel_part(make(), 8.7);
        op.stepover = None;
        op.scallop = Some(0.1);
        assert!(finish_spline_revolve_funnel(&p, &op, &feature).is_ok(), "an 8.7mm ball fits the nominal wall");
        op.stock_to_leave = Some(0.5); // skin ≈ 0.4: 9.1 > ~9
        let err = finish_spline_revolve_funnel(&p, &op, &feature).unwrap_err();
        assert!(err.contains("stock_to_leave offset"), "error should name the roughing offset: {err}");
        assert!(err.contains("hoop"), "error should name the binding direction: {err}");
    }

    // The two-direction guard, direction by direction.
    //
    // Hoop-binding: a steep narrow degree-1 chamfer wall (10,0) -> (9,−20)
    // has zero meridian curvature everywhere (no meridian candidate at all),
    // but the wall curls around the tool with hoop radius rho/|n_r| ≈ rho —
    // tightest ≈ 9 at the tip. A 3mm ball machines; a 12mm ball is rejected
    // naming the hoop.
    #[test]
    fn test_spline_revolve_funnel_hoop_gouge_guard() {
        let make = || {
            crate::nurbs::BSplineCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![(10.0, 0.0), (9.0, -20.0)],
                None,
            )
            .unwrap()
        };
        let (p, op, feature) = spline_revolve_funnel_part(make(), 3.0);
        assert!(finish_spline_revolve_funnel(&p, &op, &feature).is_ok(), "a ball within the hoop radius must machine");
        let (p, op, feature) = spline_revolve_funnel_part(make(), 12.0);
        let err = finish_spline_revolve_funnel(&p, &op, &feature).unwrap_err();
        assert!(err.contains("gouge"), "an oversized ball must be rejected naming the gouge: {err}");
        assert!(err.contains("hoop"), "the error should name the binding hoop direction: {err}");
    }

    // Meridian-binding: a 10mm-radius cove set far from the axis (arc centred
    // at (20, 0) from (30, 0) down to (20, −10)) — every hoop radius is >= 20
    // (rho >= 20 and |n_r| <= 1), so the meridian's 10mm concave radius binds,
    // exactly as on the cap.
    #[test]
    fn test_spline_revolve_funnel_meridian_gouge_guard() {
        let w = (std::f64::consts::FRAC_PI_4).pcos();
        let make = || {
            crate::nurbs::BSplineCurve::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                vec![(30.0, 0.0), (30.0, -10.0), (20.0, -10.0)],
                Some(vec![1.0, w, 1.0]),
            )
            .unwrap()
        };
        let (p, op, feature) = spline_revolve_funnel_part(make(), 3.0);
        assert!(finish_spline_revolve_funnel(&p, &op, &feature).is_ok(), "a ball within the concave radius must machine");
        let (p, op, feature) = spline_revolve_funnel_part(make(), 12.0);
        let err = finish_spline_revolve_funnel(&p, &op, &feature).unwrap_err();
        assert!(err.contains("gouge"), "an oversized ball must be rejected naming the gouge: {err}");
        assert!(err.contains("meridian is concave"), "the error should name the binding meridian direction: {err}");
    }

    // A conical V-pit (r_tip = 0 with a tilted end tangent) needs no special
    // validation: the hoop radius rho/|n_r| → 0 at the tip, so the guard
    // rejects every ball — a ball-nose cannot finish a V-pit's bottom point.
    #[test]
    fn test_spline_revolve_funnel_conical_pit_is_refused_by_the_hoop_guard() {
        let meridian = crate::nurbs::BSplineCurve::new(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![(10.0, 0.0), (0.0, -10.0)],
            None,
        )
        .unwrap();
        let (p, op, feature) = spline_revolve_funnel_part(meridian, 0.5);
        let err = finish_spline_revolve_funnel(&p, &op, &feature).unwrap_err();
        assert!(err.contains("gouge") && err.contains("hoop"), "even a tiny ball must be rejected at a V-pit tip: {err}");
    }

    // Mixed curvature end to end, the cap's bell test mirrored into a cavity:
    // a wall that bulges into the void (convex toward the tool) inflecting
    // into a bowl bottom (concave), ending ON the axis with a horizontal
    // tangent — the closed-bowl-bottom tip case. Pin the contract: the march
    // completes mouth to tip and every adjacent-pass cusp is within the same
    // 1.2x budget (second-order current-point drift, the family behavior).
    #[test]
    fn test_spline_revolve_funnel_scallop_mixed_curvature_marches_through_inflection() {
        let scallop = 0.2;
        let r_ball = 1.5;
        let make = || {
            crate::nurbs::BSplineCurve::new(
                3,
                vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 1.0, 1.0],
                vec![(14.0, 0.0), (10.0, 0.0), (8.0, -4.0), (6.0, -10.0), (3.0, -12.0), (0.0, -12.0)],
                None,
            )
            .unwrap()
        };
        // The fixture must actually cross an inflection (both signs seen
        // against the VOID normal), or the test pins less than it claims.
        let meridian = make();
        let (u0, u1) = meridian.domain();
        let (mut saw_convex, mut saw_concave) = (false, false);
        for k in 0..=200 {
            let u = u0 + (u1 - u0) * (k as f64 / 200.0);
            if let Some(kappa) = meridian.curvature_toward_void_normal(u) {
                if kappa < -1e-6 { saw_convex = true; }
                if kappa > 1e-6 { saw_concave = true; }
            }
        }
        assert!(saw_convex && saw_concave, "bell cavity meridian must carry both curvature signs");

        let (p, mut op, feature) = spline_revolve_funnel_part(make(), r_ball);
        op.stepover = None;
        op.scallop = Some(scallop);
        let g = finish_spline_revolve_funnel(&p, &op, &feature).unwrap();
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 5, "expected a real march, got {} passes", passes.len());

        // First pass at the mouth CL, last at the tip CL (the u1 clamp; the
        // tip ring collapses on the axis and generate_passes' sub-tolerance
        // skip may absorb the ring, but the emitted locus still ends there).
        let meridian = make();
        let cl = |u: f64| crate::parse::spline_revolve_funnel_cl_point(&meridian, r_ball, u);
        let (rho_m, z_m) = cl(u0);
        assert!((passes[0].1 - rho_m).abs() < 2e-3 && (passes[0].0 - z_m).abs() < 2e-3, "first pass must sit at the mouth CL");

        // Cusp budget per adjacent pair — the cap test's construction verbatim.
        let surf_dist = |pt: (f64, f64)| -> f64 {
            let mut best = f64::INFINITY;
            for k in 0..=2000 {
                let u = u0 + (u1 - u0) * (k as f64 / 2000.0);
                let (r, z) = meridian.eval(u);
                let d = ((pt.0 - r).powi(2) + (pt.1 - z).powi(2)).sqrt();
                best = best.min(d);
            }
            best
        };
        for pair in passes.windows(2) {
            let (c1, c2) = ((pair[0].1, pair[0].0), (pair[1].1, pair[1].0));
            let d = ((c2.0 - c1.0).powi(2) + (c2.1 - c1.1).powi(2)).sqrt();
            if d < 1e-6 {
                continue;
            }
            assert!(d <= 2.0 * r_ball + 1e-6, "adjacent CL rings too far apart to leave a cusp: {d}");
            let mid = ((c1.0 + c2.0) / 2.0, (c1.1 + c2.1) / 2.0);
            let half = (r_ball * r_ball - (d / 2.0) * (d / 2.0)).max(0.0).sqrt();
            let perp = (-(c2.1 - c1.1) / d, (c2.0 - c1.0) / d);
            let p1 = (mid.0 + half * perp.0, mid.1 + half * perp.1);
            let p2 = (mid.0 - half * perp.0, mid.1 - half * perp.1);
            let cusp = surf_dist(p1).min(surf_dist(p2));
            assert!(
                cusp <= 1.2 * scallop + 1e-3,
                "cusp {cusp} exceeds the scallop budget {scallop} between ({}, {}) and ({}, {})",
                c1.0, c1.1, c2.0, c2.1
            );
        }
    }

    // The physical ceiling, the funnel's copy of the cap's rule: hard error,
    // both numbers named.
    #[test]
    fn test_spline_revolve_funnel_scallop_exceeding_ball_radius_is_refused() {
        let (p, mut op, feature) = spline_revolve_funnel_part(bowl_meridian(20.0), 3.0);
        op.stepover = None;
        op.scallop = Some(3.5);
        let err = finish_spline_revolve_funnel(&p, &op, &feature).unwrap_err();
        assert!(err.contains("exceeds") && err.contains("ball radius"), "the error must name the ceiling: {err}");
        assert!(err.contains("3.500") && err.contains("3.000"), "the error must carry both numbers: {err}");
    }

    // Same load-bearing anchor for the *constant-scallop* branch: the numeric
    // Newton march (derive_ellipsoid_step_t) sizes the spacing, but in the sphere
    // limit (polar semi-axis == equatorial radius) every ball-centre point it
    // lands on must still ride the concentric sphere of radius r_part + r_ball.
    // This is the independent check the self-generated golden can't give — it
    // pins the scallop spacing's geometry to the R1 offset-closure result.

    // The shared ⚠ gouge guard's contract (ROADMAP Phase 3.5): a ball that fits
    // the tightest concave radius exactly is allowed (a degenerate point/cusp
    // contact, not a gouge); any larger ball self-intersects and is rejected.
    // This pins the boundary the torus inner-bowl guard now delegates to.
    #[test]
    fn test_concave_gouge_check_boundary() {
        // r_ball == concave_radius: fits exactly, no gouge.
        assert!(concave_gouge_check("F", 1, 5.0, 5.0, "detail").is_ok());
        // r_ball just under: fits with room.
        assert!(concave_gouge_check("F", 1, 4.999, 5.0, "detail").is_ok());
        // r_ball over: self-intersection, hard error carrying the detail clause.
        let err = concave_gouge_check("F", 1, 5.001, 5.0, "some geometry reason").unwrap_err();
        assert!(err.contains("some geometry reason"), "error should carry the detail: {err}");
        assert!(err.contains("gouge"), "error should name the failure: {err}");
    }

    // A genuine spheroid (prolate: taller than wide) has NO concentric-sphere
    // CL locus; verify instead that each emitted CL point is exactly one ball
    // radius outward-normal from some contact point on the surface — the
    // defining property of the pointwise offset. We recover the contact point
    // by inverting the CL formula per pass and check it lies on the ellipsoid.

    // Same minimal-Part builder for the paraboloid dome.

    // Recover the contact point behind an emitted paraboloid CL point (z_cl is
    // strictly monotone in rho, so a bisection pins it), then assert the two
    // defining properties of the pointwise offset: the contact lies on the
    // surface (implicit-equation residual ~ 0) and the CL point is exactly one
    // ball radius away. This residual test IS the paraboloid's correctness
    // anchor — unlike the spheroid it has no sphere limit to reduce to (see
    // test_ellipsoid_reduces_to_sphere), so the defining property is checked
    // directly, like the prolate test above.

    // Same anchor for the constant-scallop branch — pins the Newton march
    // (derive_paraboloid_step_rho) to the exact offset geometry, independent of
    // the self-generated golden.

    // Closed-form spot checks the residual test's tolerance can't miss a sign
    // error in: at the apex the normal is vertical, so the first CL point is
    // exactly (z0 + h + r_ball, 0); at the rim (rho = a) the CL point follows
    // the tilted normal (s, 1)/L.

    // The degenerate-limit anchor (the closest analogue this surface has to the
    // sphere limit): as h -> 0 the dome flattens to the base plane, so every CL
    // point must ride the R1 plane offset — constant z = z0 + r_ball, rho ~ rho_c.

    // Same minimal-Part builder for the bowl. Part::default()'s stock top is
    // z = 0, so the vertex sits at -depth.

    // The bowl's off-surface anchor, mirroring the dome's: recover the contact
    // point behind each CL point (z_cl is strictly *increasing* in rho for the
    // bowl, so the bisection flips) and assert the defining offset property.
    // Also pins rho_cl >= 0 — the guard's regularity guarantee.

    // The invariant generate_passes' transition safety rests on for a concave
    // bowl: emission is mouth-first with z strictly decreasing and rho
    // non-increasing, ending at the on-axis vertex touch at z_v + r_ball.

    // The ⚠ contract at the op level (the torus test's analogue): a ball larger
    // than the vertex radius of curvature a²/(2·depth) is rejected up front —
    // hard error, no g-code — and one that fits is accepted.

    // stock_to_leave (DECISIONS.md "stock_to_leave concave budget…"): the
    // roughing locus is the (r_ball + a')-offset of the NOMINAL surface — the
    // same defining property the plain residual tests pin, with the skin
    // joining the ball radius. The off-surface helpers take the offset
    // distance as their r_ball argument, so passing r_ball + a' asserts
    // exactly that.

    // The ⚠ contract's stock-aware form: a ball that fits the nominal bowl
    // can still gouge ROUGHING it — the roughing offset r_ball + a' is what
    // must clear the vertex radius (concave_gouge_check_with_stock).

    // Dense brute-force distance to a nominal paraboloid meridian — the dome
    // form z0 + h·(1 − rho²/a²), with `z0` placing the base plane. Serves the
    // bowl too by passing z0 = −h, which is that same curve mirrored in z (the
    // frame parse::paraboloid_cl_point works in); mirroring preserves both
    // chords and distances. Independent of
    // parse::paraboloid_meridian_distance's seeded Newton, which is part of the
    // machinery under test.

    // Dense brute-force distance to a nominal catenoid neck meridian
    // (c·cosh w, z_w − c·w) over the band [w_t, w_b], clamped at both ends.

    // The NOMINAL scallop promise on the paraboloid dome — the meridian where
    // the one-point triangle drifted WORST in the whole family (DECISIONS.md
    // "Convex R2 march cap…"): 0.5180mm on a 0.500mm request (+3.6%), 0.2052
    // on 0.200, 0.0507 on 0.050. Its radius climbs L³·a²/(2h) vertex-to-rim,
    // the steepest rise of any carrier, and the drift tracks that rise.

    // The cap's worth on the dome, stack-level and per-request — the twin of
    // the spheroid's pin.

    // The dome's roughing ceiling, the sharper half of its defect: 0.5049mm of
    // real leftover against a 0.500mm allowance before the cap, 0.3018 against
    // 0.300. Priced at the march level for the resolution reason spelled out on
    // the spheroid's twin of this test.

    // The other half of the slice's finding, and the reason only TWO of the
    // five nominated marches took a cap (DECISIONS.md "Convex R2 march cap…"):
    // the drift's SIGN is the product of two facts. The cusp's monotonicity in
    // the local radius at a fixed CL chord runs UP for a convex meridian
    // (derive_cusp_height_flat's lemma) and DOWN for a concave one
    // (derive_cusp_height_concave, its mirror), while every R2 march here
    // walks its parameter from the tight end toward the slack one. Convex
    // therefore overshoots; concave undershoots, and a cap that can only
    // shorten steps would be inert. Measured margins at a 0.5mm request:
    // bowl −0.5%, catenoid −1.8%, truncated catenoid −2.2%, pseudosphere
    // −1.2%. Pinned on both sides — under the request (the promise) and not
    // absurdly under it (a march that collapsed to hairline steps would pass
    // the first assert while quietly tripling the cycle time).

    // Same minimal-Part builder for the torus ring.
    fn torus_part(major_dia: f64, minor_dia: f64, r_ball: f64) -> (Part, Operation, Feature) {
        let (p, op, _) = revolve_part(major_dia, minor_dia, r_ball);
        let feature = Feature::Torus { major_dia, minor_dia, x: 50.0, y: 40.0, z: 0.0 };
        (p, op, feature)
    }

    // stock_to_leave on the torus (DECISIONS.md "stock_to_leave convex
    // mirrors…"): the roughing locus is the inflated torus's offset — every
    // CL ring exactly r_part + r_ball + a' off the tube's centre circle, on
    // both the outer and inner halves. The defining property, checked
    // directly like the paraboloid's inflated-offset tests.
    #[test]
    fn test_torus_rough_scallop_rides_inflated_offset() {
        let (r_major, r_part, r_ball) = (25.0, 4.0, 3.0);
        let (p, mut op, feature) = torus_part(2.0 * r_major, 2.0 * r_part, r_ball);
        op.stepover = None;
        op.scallop = Some(0.2);
        op.stock_to_leave = Some(0.5);
        // Scallop mode's budget is exact: skin = 0.5 − 0.2.
        let skin = 0.3;
        let r_off = r_part + r_ball + skin;
        let g = finish_torus(&p, &op, &feature).expect("torus roughing must succeed");
        let passes = passes_from_gcode(&g, 50.0);
        assert!(passes.len() >= 6, "expected outer and inner rings, got {}", passes.len());
        for (z, rho) in passes {
            // Both halves: distance from the tube centre circle (rho−r_major, z=0).
            let dist = ((rho - r_major).powi(2) + z * z).sqrt();
            assert!((dist - r_off).abs() < 2e-3, "CL ring ({rho}, {z}) is {dist}mm off the tube centre circle, expected {r_off}");
        }
    }

    #[test]
    fn test_torus_rough_stepdown_budgets_at_full_inflation_and_rides_the_skin() {
        // Waterline: a 1.0mm stepdown prices the crown pair at FULL
        // inflation — acos(1 − 1/7.5) on the 7.5mm fully-inflated offset
        // circle, cusp triangle at tube 4.5 — 0.454mm, leaving 0.046mm of
        // skin. (The original slice priced this at nominal geometry, 0.400;
        // the corner-roll slice's audit showed that under-ran the emitted
        // stack's true crown cusp — the chord grows faster than the angle
        // compresses — so the price moved to the top of the range.)
        let (r_major, r_part, r_ball) = (25.0, 4.0, 3.0);
        let (p, mut op, feature) = torus_part(2.0 * r_major, 2.0 * r_part, r_ball);
        op.stepover = Some(1.0);
        op.stock_to_leave = Some(0.5);
        let g = finish_torus(&p, &op, &feature).expect("torus roughing must succeed");
        assert!(g.iter().any(|l| l.contains("[stock_to_leave 0.500mm, cusp budget 0.454mm]")), "title must audit the full-inflation budget");
        let theta_max = (1.0_f64 - 1.0 / (r_part + r_ball + 0.5)).pacos();
        let h = crate::parse::derive_cusp_height(theta_max, r_ball, r_part + 0.5);
        let r_off = r_part + r_ball + (0.5 - h);
        for (z, rho) in passes_from_gcode(&g, 50.0) {
            let dist = ((rho - r_major).powi(2) + z * z).sqrt();
            assert!((dist - r_off).abs() < 2e-3, "CL ring ({rho}, {z}) is {dist}mm off the tube centre circle, expected {r_off}");
        }
    }

    // The ⚠ contract's stock-aware form on the torus inner bowl: a ball that
    // fits the nominal bowl can still gouge ROUGHING it — the roughing
    // offset r_ball + a' is what must stay inside the inner-equator radius.
    #[test]
    fn test_torus_rough_gouge_tightens_with_stock() {
        let (r_major, r_part) = (25.0, 4.0); // inner-equator radius = 21mm
        let (p, mut op, feature) = torus_part(2.0 * r_major, 2.0 * r_part, 20.8);
        op.stepover = None;
        op.scallop = Some(0.1);
        assert!(finish_torus(&p, &op, &feature).is_ok(), "20.8mm ball fits the nominal inner bowl");
        op.stock_to_leave = Some(0.5); // skin 0.4: 21.2 > 21
        let err = finish_torus(&p, &op, &feature).unwrap_err();
        assert!(err.contains("stock_to_leave offset"), "error should name the roughing offset: {err}");
        assert!(err.contains("central axis"), "error should carry the geometric detail: {err}");
    }

    // The spheroid mirror of test_paraboloid_dome_rough_rides_inflated_offset:
    // the roughing locus is the (r_ball + a')-offset of the NOMINAL surface,
    // with a' = allowance − price and the price taken at full inflation (the
    // fixed-t grid's convex tight extreme).

    // Dense brute-force distance to the nominal spheroid meridian in world
    // coordinates — independent of parse::ellipsoid_meridian_distance's seeded
    // Newton, which is part of the machinery under test.

    // The NOMINAL scallop promise on the spheroid (DECISIONS.md "Convex R2
    // march cap…"), the throat's test carried to the R2 pair: a plain
    // finishing pass states its scallop as a guarantee, so the emitted stack
    // is measured against the dense brute-force surface distance rather than
    // against the cusp triangle that generated it. Before the cap this
    // fixture priced 0.5051mm on a 0.500mm request (+1.0%), 0.2015 on 0.200,
    // 0.0502 on 0.050 — the drift shrinking with the request, which is why no
    // single scallop could have caught it.

    // What the cap is worth on this meridian, in numbers rather than in prose
    // — and, as on the throat, a claim about the STACK rather than about any
    // one pair (the pole pairs are nearly flat-limit accurate; the drift is
    // spent up the flank toward the equator, where the meridian radius has
    // grown from a²/c to c²/a). Pinned per request because the overshoot is a
    // PERCENTAGE, so a coarse-only check would look nearly clean.

    // The roughing consequence of the same drift, and the sharper half of the
    // defect: scallop+stock mode charges h = scallop into the max-not-min
    // ceiling, so an overshooting march does not merely miss a finish target —
    // it breaks the stated guarantee that TOTAL leftover <= allowance. Measured
    // before the cap: 0.5014mm against a 0.500mm allowance (skin 0.3 + scallop
    // 0.2), 0.3005 against 0.300.
    //
    // Deliberately priced at the MARCH level rather than end-to-end from the
    // G-code, because the breach is micron-scale and the end-to-end helper
    // cannot resolve it: passes read back from 3-decimal G-code carry 2e-3 of
    // slack (scallop-mode pairs sit exactly at the ceiling, so the slack has to
    // be two-sided). An end-to-end version of this test passed happily while
    // the uncapped march was over the ceiling — the green-but-blind shape
    // DECISIONS.md "Family-wide march cap…" warned about, met again in a
    // different guise. The emission path is covered by the nominal-promise test
    // above; what is checked here is the arithmetic of the ceiling itself.

    // Same minimal-Part builder for the tangent-ogive nose.

    // And the secant sibling: the same builder with the meridian radius
    // authored free (validate() has already enforced r > R_o by the time
    // finish_ogive runs; these tests pass known-legal radii directly).

    // The ogive's primary correctness anchor (the same load-bearing move as
    // test_ellipsoid_reduces_to_sphere): at height == dia/2 the tangent ogive
    // IS a hemisphere — arc centre on the axis, empty apex cap — so every
    // emitted CL point must ride finish_sphere's concentric sphere of radius
    // r_part + r_ball.

    // Same anchor for the constant-scallop branch: theta comes from
    // derive_scallop_angle with r_part = R_o = a, exactly the sphere's own
    // spacing, so the scallop CL points must ride the same concentric sphere.

    // A genuinely pointed ogive (h > a) has a piecewise CL locus; assert the
    // defining property of each piece directly: body CL points sit exactly
    // R_o + r_ball from the meridian arc centre (rho_c, 0) — the offset of a
    // circular arc is the concentric arc — and cap CL points sit exactly
    // r_ball from the sharp apex (0, h) they pivot around. The seam z_trans
    // decides which contract applies; both tolerances are the g-code writer's
    // 3-decimal rounding.

    // Closed-form endpoint spot checks the locus test's tolerance can't catch
    // a sign error in: the first pass is the on-axis point above the sharp
    // apex (z = h + r_ball, rho = 0 — the cap's psi = 90° end), and the last
    // pass is the base ring at the base plane where the tangent-ogive wall is
    // vertical, so the CL sits at exactly a + r_ball (like the sphere's
    // equator pass).

    // The secant form's CL locus is three pieces (cap / body / base-rim
    // roll); assert each piece's defining distance directly, like the tangent
    // helper above. The worked numbers are the ogive_secant.cam fixture's:
    // a=15, h=20, r=32.5 → centre (−16.5, −8) exactly (see
    // test_derive_secant_ogive_centre). Points landing within tolerance of a
    // seam satisfy both adjacent contracts (the pieces meet tangentially), so
    // the seam band accepts either.

    // Endpoint spot checks for the secant form: the locus still starts on the
    // axis above the sharp apex, and — although the wall no longer meets the
    // base vertically — the rim roll ends at the same horizontal-normal base
    // CL (a + r_ball, z0) the tangent form ends at, because the roll's psi = 0
    // end is one ball radius straight out from the rim corner.

    // Same minimal-Part builder for the cone (base plane z = 0, sharp apex
    // at z = height).
    fn cone_part(dia: f64, height: f64, r_ball: f64) -> (Part, Operation, Feature) {
        let (p, op, _) = revolve_part(dia, height, r_ball);
        let feature = Feature::Cone { dia, height, x: 50.0, y: 40.0, z: 0.0 };
        (p, op, feature)
    }

    // Distance from a meridian point to the NOMINAL cone surface (base-plane
    // relative): the nearer of the two sharp corners (apex; base rim) and,
    // where the foot of the perpendicular falls between them, the distance to
    // the flank. A SEGMENT with a corner fan at each end — the shape
    // ogive_surface_distance already used, adopted here when the clamped
    // bottom pass started rolling its corner (fork 2B). The old flank-wall
    // convention ran the flank as a line past the rim, which is what let the
    // bottom pass sit 4.243 from the corner and still measure as an offset.
    fn cone_surface_distance(rho: f64, z: f64, r_part: f64, height: f64) -> f64 {
        let l = (r_part * r_part + height * height).sqrt();
        let t = rho * (r_part / l) + (z - height) * (-height / l);
        if t < 0.0 {
            (rho * rho + (z - height) * (z - height)).sqrt()
        } else if t > l {
            ((rho - r_part) * (rho - r_part) + z * z).sqrt()
        } else {
            rho * (height / l) + (z - height) * (r_part / l)
        }
    }

    // Distance from a meridian point to the NOMINAL ogive surface: the
    // nearer of the two sharp corners (apex; base rim) and, where the point's
    // bearing from the arc centre falls inside the arc's angular span, the
    // distance to the arc itself. Points beyond either end of the arc bear
    // outside the span (the corner fans), so the min never reads the arc's
    // extension through a corner.

    // The end-to-end max-not-min ceiling, checked as geometry instead of as
    // pricing: for every adjacent pass pair, construct the actual cusp tip —
    // the lower intersection point of the two ball circles in the meridian
    // plane — and assert its height above the NOMINAL surface stays within
    // the allowance. This is the pin for the corner-roll slice's seam bound
    // (the slacker-piece price has no per-pair exactness theorem, only the
    // chord-fixed monotonicity lemma; this test is what makes the bound
    // honest rather than claimed).
    fn assert_cusp_tips_within_allowance(
        passes: &[(f64, f64)],
        r_ball: f64,
        allowance: f64,
        dist: impl Fn(f64, f64) -> f64,
    ) {
        assert!(passes.len() >= 2, "expected several passes, got {}", passes.len());
        for w in passes.windows(2) {
            let ((z1, r1), (z2, r2)) = (w[0], w[1]);
            let d = ((r1 - r2).powi(2) + (z1 - z2).powi(2)).sqrt();
            if d < 1e-9 {
                continue;
            }
            assert!(d / 2.0 < r_ball, "adjacent balls must overlap (chord {d})");
            let q = (r_ball * r_ball - (d / 2.0) * (d / 2.0)).sqrt();
            let (mx, mz) = ((r1 + r2) / 2.0, (z1 + z2) / 2.0);
            let (ux, uz) = ((r2 - r1) / d, (z2 - z1) / d);
            // The tip is whichever chord-perpendicular intersection sits
            // nearer the surface (the other is the balls' far crossing).
            let tip_a = dist(mx + q * uz, mz - q * ux);
            let tip_b = dist(mx - q * uz, mz + q * ux);
            let leftover = tip_a.min(tip_b);
            // 2e-3 slack: the passes are parsed back from 3-decimal G-code,
            // and scallop mode's flank/body pairs sit EXACTLY at the ceiling
            // (cusp == scallop, skin + scallop == allowance), so emission
            // rounding wiggles the tip both ways around it.
            assert!(
                leftover <= allowance + 2e-3,
                "cusp tip {leftover}mm above nominal exceeds the {allowance}mm ceiling (pair at z {z1:.3}/{z2:.3})"
            );
        }
    }

    // Scallop-mode roughing on the cone: the one grid change of the
    // corner-roll slice. The cap re-grids as a uniform-psi sweep and the
    // cap/flank seam becomes a pass of its own, so every pair lives on one
    // piece and prices at or under the scallop (chord-fixed monotonicity).
    #[test]
    fn test_cone_rough_scallop_rides_inflated_offset_and_grids_the_seam() {
        let (r_part, height, r_ball) = (20.0, 20.0, 3.0);
        let (p, mut op, feature) = cone_part(2.0 * r_part, height, r_ball);
        op.stepover = None;
        op.scallop = Some(0.2);
        op.stock_to_leave = Some(0.5);
        let r_off = r_ball + 0.3;
        let g = finish_cone(&p, &op, &feature).expect("cone roughing must succeed");
        let passes = passes_from_gcode(&g, 50.0);
        // Pole and seam anchors: the sweep starts dead on the axis above the
        // inflated corner sphere and lands a pass exactly at the seam.
        let &(z_first, rho_first) = passes.first().unwrap();
        assert!((z_first - (height + r_off)).abs() < 2e-3, "pole CL z {z_first}, expected {}", height + r_off);
        assert!(rho_first.abs() < 2e-3, "pole CL rho {rho_first}, expected 0");
        let l = (r_part * r_part + height * height).sqrt();
        let (sin_a, cos_a) = (r_part / l, height / l);
        let z_seam = height + r_off * sin_a;
        assert!(
            passes.iter().any(|&(z, rho)| (z - z_seam).abs() < 2e-3 && (rho - r_off * cos_a).abs() < 2e-3),
            "the cap/flank seam must be a grid point"
        );
        for &(z, rho) in &passes {
            let d = cone_surface_distance(rho, z, r_part, height);
            assert!((d - r_off).abs() < 2e-3, "CL point ({rho}, {z}) is {d}mm off the cone, expected {r_off}");
        }
        assert_cusp_tips_within_allowance(&passes, r_ball, 0.5, |rho, z| cone_surface_distance(rho, z, r_part, height));
    }

    // Waterline roughing on the cone: the stack climbs to the inflated
    // cap's pole (stopping at the nominal top leaves a polar cusp that can
    // outspend the whole allowance) and the budget prices in ONE walk at
    // full inflation — flat flank pairs never move with the skin, corner
    // and seam pairs price highest there.
    #[test]
    fn test_cone_rough_stepdown_budgets_at_full_inflation_and_rides_the_skin() {
        let (r_part, height, r_ball) = (20.0, 20.0, 3.0);
        let (p, mut op, feature) = cone_part(2.0 * r_part, height, r_ball);
        op.stepover = Some(1.0);
        op.stock_to_leave = Some(0.5);
        let cusp = crate::parse::derive_cone_stack_cusp(r_part, height, r_ball, 0.5, 1.0);
        assert!(cusp.is_finite() && cusp > 0.0 && cusp < 0.5, "fixture price must spend part of the ceiling, got {cusp}");
        let r_off = r_ball + (0.5 - cusp);
        let g = finish_cone(&p, &op, &feature).expect("cone roughing must succeed");
        let passes = passes_from_gcode(&g, 50.0);
        let &(z_first, rho_first) = passes.first().unwrap();
        assert!((z_first - (height + r_off)).abs() < 2e-3, "pole CL z {z_first}, expected {}", height + r_off);
        assert!(rho_first.abs() < 2e-3, "pole CL rho {rho_first}, expected 0");
        for &(z, rho) in &passes {
            let d = cone_surface_distance(rho, z, r_part, height);
            assert!((d - r_off).abs() < 2e-3, "CL point ({rho}, {z}) is {d}mm off the cone, expected {r_off}");
        }
        assert_cusp_tips_within_allowance(&passes, r_ball, 0.5, |rho, z| cone_surface_distance(rho, z, r_part, height));
    }

    // Waterline roughing on the secant ogive: all three inflated pieces
    // (corner spheres about BOTH sharp corners, concentric arc between), the
    // per-piece-extreme two-walk budget, and the seam bound made honest by
    // the cusp-tip ceiling check.

    // Scallop-mode roughing on the secant ogive needs NO re-grid: theta is
    // recomputed on the inflated body and the same uniform-psi sweep covers
    // all three pieces — corner pairs price under the scallop by the
    // angle-fixed monotonicity of the cusp triangle, seam straddles by the
    // slacker-piece bound. The tip check pins both.

    // The reduces-to-sphere anchor carries through the PRICING: at h == a the
    // tangent ogive is a hemisphere with an empty apex cap, so the two-walk
    // max reduces to the full-inflation walk's crown price — exactly the
    // repriced finish_sphere's. (This anchor is what caught the original
    // sphere pricing breach: the ogive's full-inflation walk priced the
    // crown ABOVE the sphere's then-nominal budget, and the disagreement
    // was the sphere's to fix — DECISIONS.md "stock_to_leave corner roll…".)

    // The sweep-then-verify guarantee, exercised end to end: over a grid of
    // stepdowns and allowances, a roughing request either rejects or every
    // actual cusp tip of the emitted stack stays within the allowance of
    // the nominal surface — including the pairs astride the corner-roll
    // seams, whose alignment shifts with the skin (the non-monotonicity
    // that retired the extreme-walk scheme for these carriers).

    // The audit fix's own pin, on the sphere directly: with the crown pair
    // priced at full inflation, every actual cusp tip of the emitted stack
    // stays within the allowance of the NOMINAL sphere. The original
    // nominal-extreme pricing fails exactly this test (its worst tip lands
    // ~0.003mm above the ceiling at this geometry) — the corner-roll
    // slice's lemma turned into a regression guard.
    #[test]
    fn test_sphere_rough_stepdown_cusp_tips_stay_within_allowance() {
        let (r_part, r_ball) = (20.0, 3.0);
        let (p, mut op, _) = revolve_part(2.0 * r_part, r_part, r_ball);
        let feature = Feature::Sphere { dia: 2.0 * r_part, x: 50.0, y: 40.0, z: 0.0 };
        op.stepover = Some(0.1);
        op.stock_to_leave = Some(0.5);
        let g = finish_sphere(&p, &op, &feature).expect("sphere roughing must succeed");
        let passes = passes_from_gcode(&g, 50.0);
        assert_cusp_tips_within_allowance(&passes, r_ball, 0.5, |rho, z| (rho * rho + z * z).sqrt() - r_part);
    }


    // Same minimal-Part builder for the catenoid neck (base plane z = 0, so
    // the waist ring sits at derive_catenoid_height above it).

    // And the truncated sibling: the band stops at the authored top circle
    // short of the waist (validate() has already enforced
    // neck_dia < top_dia < dia by the time finish_catenoid runs).

    // The catenoid's off-surface anchor, mirroring the paraboloid's: recover
    // the contact parameter behind each CL point (z_cl is strictly decreasing
    // in w for r_ball < c, so a bisection pins it), then assert the CL point
    // is exactly one ball radius from the surface point. This residual test IS
    // the catenoid's correctness anchor — like the paraboloid it has no sphere
    // limit to reduce to, so the defining offset property is checked directly.

    // Same anchor for the constant-scallop branch — pins the Newton march
    // (derive_catenoid_step_w) to the exact offset geometry, independent of
    // the self-generated golden.

    // Closed-form spot checks the residual tolerance can't miss a sign error
    // in: at the waist the wall tangent is vertical (the ball contacts at its
    // own equator), so the first CL point is exactly (z0 + h, c + r_ball); at
    // the base rim the CL follows the tilted normal (1, sinh w_b)/cosh w_b,
    // i.e. rho = a + r_ball·c/a and z = z0 + r_ball·sqrt(a² − c²)/a.

    // The regular-offset-locus claim made in finish_catenoid's comment,
    // checked on the emitted passes for both spacing modes: z strictly falls
    // and rho never shrinks waist→base, which is what makes generate_passes'
    // feed-out-then-plunge transition safe here.

    // The ⚠ anticlastic contract: the convex hoop direction does NOT excuse
    // the concave meridian — a ball wider than the waist's meridian radius of
    // curvature (c = neck_dia/2) must be rejected by the shared guard, not
    // silently gouged.

    // The truncated band rides the same catenary, so the same off-surface
    // residual anchor applies — the band's w range is a sub-interval of the
    // full neck's, and the bisection recovers contacts on it unchanged.

    // Closed-form spot checks for the truncated form (the fixture's worked
    // numbers: t/c = 1.25 → w_t = ln 2, cosh w_t = 1.25, tanh w_t = 0.6):
    // the top contact has a tilted normal, so the first CL point sits
    // r_ball/cosh w_t outside the top circle and r_ball·tanh w_t above the
    // top plane z_top = c·(w_b − w_t); the base end is the full neck's,
    // unchanged by truncation.

    // The moved guard bound, exercised from both sides: a ball too big for
    // the waist (r_ball > c) is rejected on the full neck but accepted on a
    // band that stops short of it (r_ball ≤ t²/c = 15.625 here), and a ball
    // beyond even the top circle's meridian radius is still rejected.

    // The monotone-descent claim under the moved bound: with c < r_ball ≤
    // t²/c the full neck's derivative argument no longer covers w < w_t, but
    // on the band it must still hold — both spacing modes.

    // The pseudosphere's off-surface anchor, mirroring the catenoid's: recover
    // the contact parameter behind each CL point (z_cl is strictly increasing
    // in u for r_ball < c·sinh(u_b), so a bisection pins it), then assert the
    // CL point is exactly one ball radius from the surface point. Like the
    // catenoid there is no sphere limit to reduce to, so the defining offset
    // property is checked directly.

    // Same anchor for the constant-scallop branch — pins the Newton march
    // (derive_pseudosphere_step_u) to the exact offset geometry, independent
    // of the self-generated golden.

    // Closed-form spot checks the residual tolerance can't miss a sign error
    // in: the normal at a band circle of radius rho is
    // (tanh u, sech u) = (sqrt(c² − rho²)/c, rho/c), so the tip CL (first
    // pass, top-first emission) is (rho, z) =
    // (r_tip + r_ball·sqrt(c² − r_tip²)/c, z0 + h + r_ball·r_tip/c) and the
    // base CL (last pass) is
    // (r_base + r_ball·sqrt(c² − r_base²)/c, z0 + r_ball·r_base/c).

    // The regular-offset-locus claim made in finish_pseudosphere's comment,
    // checked on the emitted passes for both spacing modes: z strictly falls
    // and rho never shrinks tip→base — the base→tip march is reversed before
    // emission (the first op where the two orders differ), and this is the
    // shape that makes generate_passes' feed-out-then-plunge transition safe.

    // The ⚠ anticlastic contract again, at the opposite end from the
    // catenoid: the band's meridian is tightest at the BASE circle
    // (c·sinh(u_b) = 15mm for these numbers — it would vanish at the cusp rim
    // the band truncates above), so a wider ball must be rejected by the
    // shared guard, not silently gouged.

    // stock_to_leave on the anticlastic bands (DECISIONS.md "stock_to_leave
    // anticlastic bands…"): the roughing locus is the (r_ball + a')-offset of
    // the NOMINAL surface — the off-surface helpers take the offset distance
    // as their r_ball argument, so passing r_ball + a' asserts exactly that,
    // the bowl's pattern on the concave-meridian bands.

    // The truncated band rides along free: the pricer and the march scan the
    // actual [w_t, w_b] grid, so the same inflated-offset property holds.

    // The ⚠ contract's stock-aware form on the neck: a ball that fits the
    // nominal waist can still gouge ROUGHING it — the roughing offset
    // r_ball + a' is what must stay inside the waist's meridian radius, and
    // it doubles as the inflated locus's regularity bound.

    // And under truncation the stock-aware bound is the MOVED bound: the top
    // circle's t²/c, not the waist's c.

    // The ⚠ contract's stock-aware form at the opposite end: on the tractrix
    // band the bound is the BASE circle's meridian radius.

    // The throat's off-surface anchor, mirroring the outside neck's: recover
    // the contact parameter behind each CL point (z_cl is strictly increasing
    // in w unconditionally — the throat's offset locus is regular for every
    // ball, see finish_catenoid_throat — so a bisection pins it), then assert
    // the CL point is exactly one ball radius from the surface point.

    // Same anchor for the constant-scallop branch — pins the Newton march
    // (derive_catenoid_throat_capped_step_w, the convex-triangle sibling) to
    // the exact offset geometry, independent of the self-generated golden.

    // Closed-form spot checks the residual tolerance can't miss a sign error
    // in: at the rim the CL follows the tilted into-void normal
    // (−1, sinh w_b)/cosh w_b, i.e. rho = a − r_ball·c/a and
    // z = stock top + r_ball·sqrt(a² − c²)/a (above the stock top — the ball
    // rides partly out of the mouth); at the waist the wall is vertical (the
    // ball contacts at its own equator), so the last CL point is exactly
    // (rho, z) = (c − r_ball, stock top − h).

    // The regular-offset-locus claim made in finish_catenoid_throat's
    // comment, checked on the emitted passes for both spacing modes: z
    // strictly falls and rho never GROWS rim→waist — the first revolved op
    // whose passes shrink inward going down (a cavity narrowing to its
    // throat), which is the mirrored shape that keeps generate_passes'
    // feed-then-plunge transition safe (moving inward gains clearance).

    // The ⚠ contract from the void side: the guard bound is the SAME
    // c = neck_dia/2 as the outside neck (a minimal surface's principal radii
    // agree in magnitude), but the concave direction is now the hoop — a ball
    // wider than the throat must be rejected by the shared guard, not jammed
    // through a CL ring that collapses past the axis.

    // Dense brute-force distance to the throat's nominal catenary in world
    // coordinates — independent of parse::catenoid_throat_meridian_distance's
    // seeded Newton, which is part of the machinery under test.

    // The NOMINAL scallop promise on the throat (DECISIONS.md "Family-wide
    // march cap…"), the tangent-lip funnel's test carried across: a plain
    // finishing pass claims its scallop as a guarantee, so the emitted stack
    // is measured against the dense brute-force surface distance rather than
    // against the cusp triangle that generated it. Before the cap this
    // fixture priced 0.5106mm on a 0.500mm request (+2.1%), 0.2030 on 0.200,
    // 0.0504 on 0.050 — the drift shrinking with the request, which is
    // exactly why a single scallop could not have caught it.

    // What the cap is worth on this meridian, in numbers rather than in
    // prose. The claim is about the STACK, not about any one pair: the
    // triangle is accurate enough at the waist start (its first step prices
    // just UNDER a 0.5mm request) and runs slack further up the flare, so a
    // first-pair test would have looked clean — the drift has to be measured
    // where the march actually spends it. Pinned per request because the
    // overshoot is a PERCENTAGE here, unlike the tangent lip's absolute
    // scallop-insensitive 2.3mm, so it shrinks with the request and a
    // coarse-only check would look nearly clean too.

    // The throat's roughing contract, stepover mode: the emitted stack rides
    // the a'-inflated cavity exactly (CL at r_ball + a' off the nominal
    // catenary) AND every actual cusp tip stays within the allowance — the
    // exact-tip walk's end-to-end pin, checked as geometry instead of as
    // pricing (the inside-funnels slice prices by exact tips everywhere, so
    // there is no per-pair exactness theorem to lean on, only this).

    // The scallop twin: the convex march is only the spacing heuristic under
    // stock (its current-point drift means cusp != scallop exactly), so the
    // budget is the verified exact-tip price of the march's own grid and the
    // same geometric ceiling must hold. Since the tip cap landed the march is
    // itself capped, so that verified price no longer pays for the drift —
    // but the test still reads the price rather than assuming a value.

    // The ⚠ contract's stock-aware form through the throat: the waist bound
    // c is unchanged (the hoop is the concave direction and the inflation
    // rides INTO the cavity), but the roughing offset r_ball + a' is what
    // must now clear it.

    // A stepover past the ball diameter leaves adjacent balls that never
    // intersect — the INFINITY path through the throat's tip walk.

    // The funnel's off-surface anchor, mirroring the outside band's: recover
    // the contact parameter behind each CL point (z_cl is strictly decreasing
    // in u unconditionally — the funnel's locus is regular for every ball,
    // see finish_pseudosphere_funnel — so a bisection pins it), then assert
    // the CL point is exactly one ball radius from the surface point.

    // Same anchor for the constant-scallop branch — pins the Newton march
    // (derive_pseudosphere_funnel_step_u, the convex-triangle sibling) to the
    // exact offset geometry, independent of the self-generated golden.

    // Closed-form spot checks the residual tolerance can't miss a sign error
    // in: the into-void normal at a circle of radius rho is
    // (−tanh u, sech u) = (−sqrt(c² − rho²)/c, rho/c), so the mouth CL
    // (first pass) is (rho, z) = (r_m − r_ball·sqrt(c² − r_m²)/c,
    // stock top + r_ball·r_m/c) — above the stock top, the ball tangent to
    // the lip — and the tip CL (last pass) is
    // (r_t − r_ball·sqrt(c² − r_t²)/c, stock top − h + r_ball·r_t/c).

    // The regular-offset-locus claim made in finish_pseudosphere_funnel's
    // comment, checked on the emitted passes for both spacing modes: z
    // strictly falls and rho never grows mouth→tip — and unlike the outside
    // band and the throat, this order is the march order itself (no
    // reversal), so this test also pins that the op emits top-first.

    // The ⚠ contract on the non-minimal surface: the binding bound is the
    // TIP circle's hoop radius c/sinh(u_t) (≈ 8.729mm for these numbers) —
    // not the outside band's meridian bound at its base — so a ball wider
    // than the tip's hoop must be rejected by the shared guard.

    // The TANGENT-LIP form (mouth_dia == dia, the lifted deferral): the same
    // off-surface anchor holds with the march starting at the cusp itself —
    // u_b = 0 is a legal bisection bracket, and the residual check is what
    // pins the degenerate-start scallop treatment to the exact geometry.

    // Closed-form spot checks at the lip: the general mouth formulas
    // specialize continuously at u_b = 0 (sech = 1, tanh = 0), so the first
    // CL point is exactly (c, stock top + r_ball) — the ball resting ON the
    // lip circle, tangent to the stock top plane from above — and the tip
    // end is the truncated-mouth funnel's, unchanged. Both spacing modes,
    // pinning that neither march skips or distorts the degenerate first ring.

    // The monotone-locus claim from the cusp start, both spacing modes: the
    // scallop march's first step comes from the arc-length seed (the
    // linearized seed divides by zero at u = 0), and every subsequent ring
    // must still strictly descend and narrow.

    // The degenerate-start SEED itself, at the unit level: from u = 0 the
    // seed must return a finite, strictly positive parameter (the linearized
    // form would be inf/NaN), and its CL chord must land within Newton's
    // tolerance of the chord the cusp triangle asked for. Note what this test
    // does NOT claim, and never did: that the chord is the right one. It is
    // the corner model's chord, which at the lip is ~12× too long — hence the
    // tip cap, pinned by the promise test below (DECISIONS.md "Tangent-lip
    // march fix…"). Solving the stated equation correctly and stating the
    // right equation are separate obligations; this test is the first.

    // Dense brute-force distance to the funnel's nominal tractrix in world
    // coordinates — independent of parse::pseudosphere_funnel_meridian_distance's
    // seeded Newton, which is part of the machinery under test.

    // The NOMINAL scallop promise, both funnel forms (DECISIONS.md
    // "Tangent-lip march fix…") — this slice's reason to exist. A plain
    // finishing pass claims its scallop as a guarantee, so the emitted stack
    // is measured against the dense brute-force surface distance, not against
    // the cusp triangle that generated it. Before the tip cap came off its
    // roughing gate, the tangent lip's first pair priced 2.33mm on this
    // 0.5mm request and 2.48mm on a 0.2mm one; the truncated mouth overshot
    // by ~2.6%. Both forms and a spread of scallops, because the lip's
    // failure was scallop-INSENSITIVE — a single request could not have
    // caught it.

    // The cap binds hardest at the lip, and the numbers are worth pinning
    // rather than describing: the corner-model seed's first step, priced by
    // the exact tip walk, leaves ~2.3–2.5mm whatever the request — 4.7× on a
    // 0.5mm scallop, 12× on a 0.2mm one, the ratio growing only because the
    // chord itself is deaf to the request. That deafness is the point: no
    // scallop choice could have escaped it.

    // The funnel's roughing contract, stepover mode: the fixed closed-form
    // arc grid rides the a'-inflated cavity exactly and every actual cusp
    // tip stays within the allowance — the throat test's twin on the other
    // closed-form meridian.

    // The scallop twin: the funnel's convex march becomes the spacing
    // heuristic under stock and the verified exact-tip price is the budget.

    // The tangent-lip form under stock: the march starts AT the lip cusp
    // (u = 0, where the inflated local radius is the skin itself) and the
    // clamped projection treats leftovers above the lip as the corner fan —
    // the whole stack must still ride the inflated offset and hold the
    // ceiling.

    // The ⚠ contract's stock-aware form at the funnel's tip circle: the
    // hoop bound c/sinh(u_t) is unchanged, the roughing offset must clear it.

    // Distance from a point to the ellipse (a·cos t + cx, b·sin t + cy):
    // coarse parameter scan, then a ternary-search refine on the locally
    // unimodal squared distance. Independent of the normal-angle
    // parametrization the op uses — that independence is what makes the
    // parallel-residual tests below an anchor rather than a tautology.
    fn dist_to_ellipse(px: f64, py: f64, cx: f64, cy: f64, a: f64, b: f64) -> f64 {
        let d2 = |t: f64| -> f64 {
            let dx = px - (cx + a * t.pcos());
            let dy = py - (cy + b * t.psin());
            dx * dx + dy * dy
        };
        let n = 4096;
        let (mut best_t, mut best) = (0.0_f64, f64::MAX);
        for i in 0..n {
            let t = 2.0 * PI * (i as f64) / (n as f64);
            let d = d2(t);
            if d < best {
                best = d;
                best_t = t;
            }
        }
        let (mut lo, mut hi) = (best_t - 2.0 * PI / n as f64, best_t + 2.0 * PI / n as f64);
        for _ in 0..100 {
            let m1 = lo + (hi - lo) / 3.0;
            let m2 = hi - (hi - lo) / 3.0;
            if d2(m1) < d2(m2) { hi = m2 } else { lo = m1 }
        }
        d2(0.5 * (lo + hi)).sqrt()
    }

    // The elliptic rings' defining property, checked independently of the
    // normal-angle math that generates them: every emitted point sits exactly
    // |r_signed| off the ellipse, and every chord midpoint stays within the
    // shared chord tolerance of that parallel — the R2-in-the-plane analogue
    // of the §5 off-surface residual anchors.
    #[test]
    fn test_ellipse_offset_pts_ride_the_parallel() {
        let (cx, cy, a, b) = (50.0, 50.0, 25.0, 15.0);
        for (pts, r) in [
            (ellipse_offset_pts(cx, cy, a, b, -5.0, None), 5.0),
            (ellipse_offset_pts(cx, cy, a, b, 0.0, None), 0.0),
        ] {
            assert!(pts.len() >= 16, "expected a well-sampled ring, got {}", pts.len());
            for w in pts.windows(2) {
                let (x0, y0) = w[0];
                let (x1, y1) = w[1];
                let d = dist_to_ellipse(x0, y0, cx, cy, a, b);
                assert!((d - r).abs() < 1e-6, "ring point {d}mm from ellipse, expected {r}");
                let dm = dist_to_ellipse(0.5 * (x0 + x1), 0.5 * (y0 + y1), cx, cy, a, b);
                assert!((dm - r).abs() < crate::CHORD_TOL_MM + 1e-6, "chord midpoint {dm}mm from ellipse, expected {r} within the chord tolerance");
            }
        }
    }

    // The load-bearing R1 anchor (the flank-pair analogue of
    // test_ellipsoid_reduces_to_sphere): at major == minor the ellipse IS a
    // circle, so both comp modes must emit finish_cylinder_bore's g-code exactly
    // (titles aside). The parse path rejects that degenerate input (it points
    // at CylinderBore), so the anchor calls the ops directly.

    // The circular pair's twin pin (DECISIONS.md "the control-comp centerline
    // resolve", pin 1): the resolved head's cut ring is `ring_moves` at the
    // engine sibling's rho — same generator, same radius, checked as a
    // contiguous run of identical lines. A skin is present so the claim being
    // exercised is the composition (edge ⊖ skin) ⊖ r == edge ⊖ (skin + r),
    // not just the skinless radius arithmetic.
    #[test]
    fn test_control_resolved_ring_is_the_engine_ring() {
        fn contains_run(hay: &[String], needle: &[String]) -> bool {
            !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
        }
        let mut p = Part::default();
        p.tools.insert(5, Tool { num: 5, name: "Endmill".into(), dia: 10.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        let mut op = Operation {
            op_type: crate::parse::OpType::Side,
            tool_num: 5,
            feature_name: "Bore".into(),
            stepover: Some(5.0),
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
            comp: CompMode::Control,
            compress: CompressMode::Chords,
            direction: None, angle: None, entry: None, lead: None, lead_angle: None, lead_descent: None,
            stock_to_leave: Some(0.5),
            strategy: None,
            pattern: None,
            engagement: None,
            cover: None,
            voxel_cell: None, coolant: None, n: None,
            wall_standoff: 0.0,
            face_standoff: 0.0,
            tip_clearance: None,
        };
        let bore = Feature::CylinderBore { dia: 40.0, extent: 12.0, x: 50.0, y: 50.0, z: None, edge_round: None, chamfer: None, csink: None, cbore: None };
        let m = finish_cylinder_bore(&p, &op, &bore).expect("guard passes");
        let resolved = m.resolved.expect("a control op carries the resolved channel");
        // The engine sibling's rho: R − r_tool − skin = 20 − 5 − 0.5.
        let ring = ring_moves(op.compress, false, 50.0, 50.0, 20.0 - 5.0 - 0.5, p.global_feed, false);
        assert!(
            contains_run(&resolved, &ring),
            "the bore's resolved ring must be the engine ring at rho 14.5"
        );

        op.feature_name = "Post".into();
        let boss = Feature::CylinderBoss { dia: 40.0, extent: 12.0, x: 50.0, y: 50.0, z: -12.0, edge_round: None, chamfer: None };
        let m = finish_cylinder_boss(&p, &op, &boss).expect("the boss finishes");
        let resolved = m.resolved.expect("a control op carries the resolved channel");
        let ring = ring_moves(op.compress, true, 50.0, 50.0, 20.0 + 5.0 + 0.5, p.global_feed, false);
        assert!(
            contains_run(&resolved, &ring),
            "the boss's resolved ring must be the engine ring at rho 25.5"
        );
    }

    // The ⚠ flank contract: the wall's tightest concave radius is the evolute
    // bound b²/a = 9 for these numbers — smaller than the minor semi-axis 15
    // the tool trivially fits inside — so a tool radius between the two must
    // be rejected by the shared guard (the first flank consumer whose guard
    // radius differs from its fit radius).

    // The rotated superset's defining property at the ring level: the rotated
    // ring is the axis-aligned ring rigidly rotated about the centre — same
    // segment count (turning angles are rotation-invariant), each point
    // turned by exactly θ.

    // The same property for the whole emitted op, both features, both comp
    // modes: every XY move (ring points AND entry/lead/edge points) is the
    // axis-aligned move rotated about the feature centre — a rotation is a
    // rigid motion, so nothing else about the program may change.

    // The guard is rotation-invariant (the evolute bound is intrinsic to the
    // ellipse): the same oversized tool is rejected with rotation present.

    // And the asymmetric sibling: the G1 junction height authored free
    // (validate() has already enforced 0 < inflection < height, != height/2
    // by the time finish_vase runs).

    // The vase's off-meridian anchor: every CL point is exactly one ball
    // radius from the two-arc ogee, measured against a densely sampled
    // meridian — independent of the concentric-arc math that generates the
    // passes, like every residual anchor in this module. `inflection` picks
    // the split (None = symmetric): the meridian sampling uses the same
    // split radii the op derives, but the distance check is what anchors.

    // Closed-form spot checks: the waist CL is (c + r_ball, z0 + h) (vertical
    // wall, ball-equator contact — the catenoid waist's shape); the equator
    // CL is (a + r_ball, z0) exactly (horizontal normal); and in scallop mode
    // the junction ring — always emitted, where the march switches triangles
    // — is the inflection contact ((a+c)/2, z0+h/2) offset along the shared
    // normal (cos θ, sin θ). Fixture numbers make these exact: r = 25,
    // sin θ = 0.6, cos θ = 0.8 → junction CL (17.4, 16.8).

    // The regular-locus claim across the inflection, both spacing modes: z
    // strictly falls and rho never shrinks waist→equator — one monotone
    // sequence spanning both arcs, which is what makes generate_passes'
    // transition safe over the whole mixed-curvature meridian.

    // The ⚠ mixed-curvature contract: the bound comes from the neck arc
    // alone (constant radius r = 25 for these numbers) — a ball wider than
    // it must be rejected by the shared guard even though the belly arc would
    // take it happily.

    // The asymmetric ogee rides the same anchor, with the split radii in the
    // sampled meridian (fixture numbers: inflection 12 of 30 → r1 = 20,
    // r2 = 30).

    // Closed-form spot checks for the asymmetric form: both wall ends are
    // unchanged by the split (waist CL (c + r_ball, h), equator CL
    // (a + r_ball, 0)), and the junction ring — the authored part — is the
    // junction contact (a − r1(1 − cos θ), inflection) offset along the
    // shared normal (cos θ, sin θ). Fixture numbers make these exact:
    // sin θ = 0.6, cos θ = 0.8, r1 = 20 → contact (16, 12), CL (18.4, 13.8).

    // The monotone-locus claim holds for every legal split (same per-arc
    // arguments, θ ≤ 90° unchanged) — both spacing modes, low and high
    // inflection.

    // The moved gouge bound, exercised from both sides: the bound is the
    // neck radius r2 = (h − i)/sin θ, so the SAME ball is rejected on the
    // symmetric vase (r = 25) but accepted on a low-inflection split
    // (i = 12 → r2 = 30), and a high-inflection split tightens the bound
    // (i = 24 → r2 = 10) and rejects a ball the symmetric vase takes.

    // Brute-force distance to the nominal ogee — the dense meridian sample
    // plus the two vertical end tangents as half-lines (the flank-wall
    // convention), independent of parse::vase_meridian_distance's clamped-arc
    // closed form, which is part of the machinery under test.

    // Waterline roughing on the first mixed-curvature carrier: the grid is
    // uniform arc on the INFLATED ogee (junction moving with the skin), every
    // CL rides the inflated offset exactly, and every actual cusp tip —
    // G1-junction straddles included — stays within the allowance.

    // Scallop-mode roughing is EXACT on the vase: each march lives on one
    // constant-radius arc of the inflated ogee and the junction ring is
    // emitted by construction, so cusp == scallop pair by pair and the whole
    // remaining skin is a' = allowance − scallop. The asymmetric split rides
    // the same recipe with its own radii.

    // The ceiling refusal: a 3.5mm stepover's flat-class cusp on these arcs
    // already outspends a 0.2mm allowance.

    // The gouge guard's _with_stock form on the moved bound: a ball that
    // fits the high-inflection neck (r2 = 10) nominally is rejected once the
    // roughing skin joins the offset.

    // The camp-3 anchor (the mesh analogue of test_ellipsoid_reduces_to_sphere,
    // independent of the self-generated golden): over a flat horizontal plate
    // at z = h every emitted ball-centre must be exactly h + r_ball. Interior
    // samples prove the face-contact envelope; the boundary skirt (the raster
    // extends r_ball past the plate) proves the z_floor clamp — an edge/vertex
    // contact there would carry the centre below h + r_ball, and the clamp
    // holds it at the floor. One flat plate exercises both.
    #[test]
    fn test_mesh_flat_plate_is_constant_height() {
        let (h, r_ball) = (7.0, 3.0);
        let plate = vec![
            crate::mesh::Tri { v: [[0.0, 0.0, h], [20.0, 0.0, h], [20.0, 20.0, h]] },
            crate::mesh::Tri { v: [[0.0, 0.0, h], [20.0, 20.0, h], [0.0, 20.0, h]] },
        ];
        let mut p = Part::default();
        p.tools.insert(9, Tool { num: 9, name: "Ballmill".into(), dia: r_ball * 2.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        let feature = Feature::Mesh { triangles: plate, x: 0.0, y: 0.0, z: 0.0, file: "plate".into() };
        let op = Operation {
            op_type: crate::parse::OpType::Copy,
            tool_num: 9,
            feature_name: "Plate".into(),
            stepover: Some(2.0),
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
        };
        let g = finish_mesh(&p, &op, &feature);

        // Every cutting move (the plunge and each raster sample) carries a Z.
        let mut cut_zs = 0;
        for line in &g {
            if !line.starts_with("G01") {
                continue;
            }
            let z_tok = line
                .split_whitespace()
                .find_map(|t| t.strip_prefix('Z'))
                .expect("every G01 in the raster carries an explicit Z");
            let z: f64 = z_tok.parse().unwrap();
            assert!(
                (z - (h + r_ball)).abs() < 5e-4,
                "flat plate: centre at {z}, expected {}",
                h + r_ball
            );
            cut_zs += 1;
        }
        // (20 + 2*3) / 2 + 1 = 14 samples per row, 14 rows.
        assert_eq!(cut_zs, 14 * 14, "expected every grid sample to be emitted");
    }

    // ---- the general NURBS patch --------------------------------------------

    // The slice's fixture surface, hand-checkable end to end: a bicubic Bézier
    // sheet over a regular 24x24 net (x, y poles at 0/8/16/24), perimeter
    // poles at z = 0 and the inner 2x2 at z = 16. Clamped single-span knots
    // make it a pure Bézier patch, so the centre is the weighted pole average
    // with weights (1,3,3,1)/8 per direction: z(0.5, 0.5) = 4·(3·3·16)/64 =
    // 9.0 exactly, at x = y = (0 + 8·3 + 16·3 + 24)/8 = 12 — the apex, by
    // symmetry, with a vertical normal. The boundary curves lie at z = 0
    // (their poles all do), so the surface minimum is 0 exactly.
    fn bezier_cap() -> crate::nurbs::BSplineSurface {
        let grid = [0.0, 8.0, 16.0, 24.0];
        let mut poles = Vec::new();
        for iu in 0..4 {
            for iv in 0..4 {
                let inner = (1..=2).contains(&iu) && (1..=2).contains(&iv);
                poles.push([grid[iu], grid[iv], if inner { 16.0 } else { 0.0 }]);
            }
        }
        crate::nurbs::BSplineSurface::new(
            3,
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            poles,
            None,
        )
        .unwrap()
    }

    // The exact sphere octant (the nurbs unit tests' anchor surface, rebuilt
    // here for the drop): radius R about the origin, x, y, z >= 0.
    fn sphere_octant_patch(r: f64) -> crate::nurbs::BSplineSurface {
        let c = (std::f64::consts::FRAC_PI_4).pcos();
        let xs = [r, r, 0.0];
        let zs = [0.0, r, r];
        let w = [1.0, c, 1.0];
        let mut poles = Vec::new();
        let mut weights = Vec::new();
        for i in 0..3 {
            let ring = [[xs[i], 0.0, zs[i]], [xs[i], xs[i], zs[i]], [0.0, xs[i], zs[i]]];
            for j in 0..3 {
                poles.push(ring[j]);
                weights.push(w[i] * w[j]);
            }
        }
        crate::nurbs::BSplineSurface::new(
            2,
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            poles,
            Some(weights),
        )
        .unwrap()
    }

    // The patch analogue of test_mesh_flat_plate_is_constant_height: over a
    // flat bicubic sheet at z = h every emitted centre is h + r_ball — the
    // interior proves the contact envelope, the skirt proves the floor clamp
    // (a flat sheet's dense-scan minimum is h, so the floor is h + r_ball
    // too; one sheet exercises both).
    #[test]
    fn test_patch_flat_sheet_is_constant_height() {
        let (h, r_ball) = (7.0, 3.0);
        let grid = [0.0, 20.0 / 3.0, 40.0 / 3.0, 20.0];
        let mut poles = Vec::new();
        for iu in 0..4 {
            for iv in 0..4 {
                poles.push([grid[iu], grid[iv], h]);
            }
        }
        let surface = crate::nurbs::BSplineSurface::new(
            3,
            3,
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            poles,
            None,
        )
        .unwrap();
        let mut p = Part::default();
        p.tools.insert(9, Tool { num: 9, name: "Ballmill".into(), dia: r_ball * 2.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        let feature =
            Feature::NurbsPatch { surface, x: 0.0, y: 0.0, z: 0.0, file: "sheet".into() };
        let op = Operation {
            op_type: crate::parse::OpType::Copy,
            tool_num: 9,
            feature_name: "Sheet".into(),
            stepover: Some(2.0),
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
        };
        let g = finish_nurbs_patch(&p, &op, &feature);

        let mut cut_zs = 0;
        for line in &g {
            if !line.starts_with("G01") {
                continue;
            }
            let z_tok = line
                .split_whitespace()
                .find_map(|t| t.strip_prefix('Z'))
                .expect("every G01 in the raster carries an explicit Z");
            let z: f64 = z_tok.parse().unwrap();
            assert!(
                (z - (h + r_ball)).abs() < 5e-4,
                "flat sheet: centre at {z}, expected {}",
                h + r_ball
            );
            cut_zs += 1;
        }
        assert_eq!(cut_zs, 14 * 14, "expected every grid sample to be emitted");
    }

    // The load-bearing exactness anchor, the patch's counterpart of
    // test_ellipsoid_reduces_to_sphere: dropping on the exact rational sphere
    // octant must land the centre on the concentric R + r sphere —
    // sqrt((R+r)² − px² − py²) in closed form — wherever the true contact
    // lies strictly inside the octant. This pins the whole chain (tensor
    // evaluation, seed capture, Newton polish) against an independent
    // hand formula, not a golden.
    #[test]
    fn test_patch_drop_reduces_to_the_sphere() {
        let (r_part, r_ball) = (9.0, 3.0);
        let surf = sphere_octant_patch(r_part);
        let seeds = PatchSeeds::new(&surf, r_ball);
        let reach = r_part + r_ball;
        for &(px, py) in
            &[(1.0, 1.0), (3.0, 4.0), (5.0, 2.0), (0.5, 6.0), (6.5, 0.5), (4.0, 4.0)]
        {
            let z = patch_drop(&surf, &seeds, px, py, r_ball)
                .expect("a sample over the octant must find contact");
            let expect = (reach * reach - px * px - py * py).sqrt();
            assert!(
                (z - expect).abs() < 1e-6,
                "octant drop at ({px}, {py}): centre {z}, closed form {expect}"
            );
        }
    }

    // The fixture's hand anchor: over the Bézier cap's apex the ball rests on
    // the apex point itself (vertical normal there), so the centre is exactly
    // apex + r = 9 + 4 = 13.
    #[test]
    fn test_patch_drop_apex_is_exact() {
        let surf = bezier_cap();
        let seeds = PatchSeeds::new(&surf, 4.0);
        let z = patch_drop(&surf, &seeds, 12.0, 12.0, 4.0).expect("apex sample must touch");
        assert!((z - 13.0).abs() < 1e-9, "apex centre {z}, expected 13.0");
    }

    // The ceiling pin — the enforcement half of the capture argument
    // (DECISIONS.md "the general NURBS patch…"): an independent brute force
    // (a 640x640 = 409,600-point dense sweep of the surface, the march-cap
    // thread's 400k precedent) puts a floor under the true envelope at every
    // probed sample, and the emitted drop must never sit BELOW it (below =
    // a missed contact basin = a gouge). Tightness is pinned from the other
    // side at the dense grid's own resolution, so the polish must actually
    // polish — a drop that merely echoed the raw seed grid would fail it.
    #[test]
    fn test_patch_drop_never_dips_below_brute_force() {
        let r_ball = 4.0;
        let surf = bezier_cap();
        let seeds = PatchSeeds::new(&surf, r_ball);

        const N: usize = 640;
        let mut dense = Vec::with_capacity((N + 1) * (N + 1));
        for iu in 0..=N {
            for iv in 0..=N {
                dense.push(surf.eval(iu as f64 / N as f64, iv as f64 / N as f64));
            }
        }

        let r2 = r_ball * r_ball;
        let brute = |px: f64, py: f64| -> Option<f64> {
            let mut best: Option<f64> = None;
            for s in &dense {
                let q = (s[0] - px).powi(2) + (s[1] - py).powi(2);
                if q <= r2 {
                    let z = s[2] + (r2 - q).sqrt();
                    best = Some(best.map_or(z, |b: f64| b.max(z)));
                }
            }
            best
        };

        // Probe an 8x8 grid across the footprint plus skirt corners.
        let mut probed = 0;
        for ix in 0..8 {
            for iy in 0..8 {
                let (px, py) = (-2.0 + 28.0 * ix as f64 / 7.0, -2.0 + 28.0 * iy as f64 / 7.0);
                let (b, d) = (brute(px, py), patch_drop(&surf, &seeds, px, py, r_ball));
                match (b, d) {
                    (None, _) => continue, // beyond the ball's reach of the sheet
                    (Some(b), Some(d)) => {
                        assert!(
                            d >= b - 1e-9,
                            "drop at ({px}, {py}) sits {b:.6} - {d:.6} = {:.2e} BELOW the brute envelope: a missed basin, the gouge direction",
                            b - d
                        );
                        assert!(
                            d <= b + 2e-3,
                            "drop at ({px}, {py}) sits {:.2e} above the brute envelope — beyond the dense grid's own resolution",
                            d - b
                        );
                        probed += 1;
                    }
                    (Some(_), None) => {
                        panic!("drop at ({px}, {py}) found no contact where the brute force did")
                    }
                }
            }
        }
        assert!(probed > 40, "the probe grid must actually exercise the surface ({probed})");
    }
}
