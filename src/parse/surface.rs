use crate::math::Portable as _;
// Surface finishing math: scallop and cusp heights, cone and spline meridians, the step sizes they give.

use super::*;

// Camp-3 raster sample spacing (DECISIONS.md "Camp-3 slice 1: raster
// drop-cutter…"). v1: an isotropic z-map grid — the along-row sample spacing
// equals the row stepover, so both directions share one error budget. One home
// so a finer along-row spacing later is a one-line change, not a hunt.
pub fn derive_mesh_sample_step(stepover: f64) -> f64 {
    stepover
}

// Tangent-ogive meridian radius (CHECKLIST row 5.1): the circular arc through
// the base rim (rho = a, the base plane) and the apex (rho = 0, height h up)
// whose centre lies IN the base plane — that centre placement is what makes
// the arc meet the base with a vertical tangent ("tangent" ogive). From
// (R_o - a)² + h² = R_o²:  R_o = (a² + h²)/(2a). One home per the derive_*()
// discipline: ops::finish and the tests both consume this, never restate it.
// At h == a this is exactly a (r_part = a) hemisphere.

// Secant-ogive meridian arc centre (row 5.1's superset, DECISIONS.md "Secant
// ogive…"): the circle of authored radius r through the base rim (a, 0) and
// the apex (0, h), centre on the chord's perpendicular bisector on the inner
// (material) side — (rho_c, z_c) relative to (axis, base plane):
//   q = sqrt(r² − (L/2)²), L = |chord| = sqrt(a² + h²)
//   rho_c = a/2 − q·h/L,   z_c = h/2 − q·a/L
// At r == R_o (tangent) this lands exactly on (a − R_o, 0); validate()
// enforces r > R_o, which pushes z_c strictly below the base plane — the
// arc's crest (its widest circle, at the centre height) leaves the wall, so
// rho is strictly decreasing base→apex: the non-undercut rule, again carried
// by the constructor. One home per the derive_*() discipline; the tangent
// form does NOT round-trip through this (its z_c ≡ 0 is an identity, not a
// residual of sqrt) — finish_ogive keeps the closed-form (a − R_o, 0).

// Catenoid height (CHECKLIST row 5.3): the waist sits AT the feature top by
// construction (the variant's non-undercut rule), which pins the height to
// h = c·acosh(a/c) — the meridian rho(z) = c·cosh((z_w − z)/c) must pass
// through the base rim, c·cosh(h/c) = a. One home per the derive_*()
// discipline: ops::finish and the tests both consume this, never restate it.
// Well-defined for every legal catenoid (validate() enforces neck_dia < dia,
// so a/c > 1 and acosh is real).

pub fn derive_scallop_angle(scallop_height: f64, r_ball: f64, r_part: f64) -> f64 {
    let r = r_part + r_ball;
    let d = r_part + scallop_height;
    let cos_half = ((r * r) + (d * d) - (r_ball * r_ball)) / (2.0 * r * d);
    let half_theta = cos_half.clamp(-1.0, 1.0).pacos();
    2.0 * half_theta
}

// The exact forward inverse of derive_scallop_angle: the cusp height left
// between two adjacent ball positions separated by angle `theta` about a
// convex surface of radius r_part. Same triangle read the other way — the
// cusp sits on the bisector where the two ball surfaces meet, at distance
// D = R'·cos(θ/2) − sqrt(r_ball² − R'²·sin²(θ/2)) from the centre (the law
// of cosines r_ball² = R'² + D² − 2R'D·cos(θ/2) solved for D, inner root —
// the outer root is the spheres' far intersection, outside the material),
// so h = D − r_part. When R'·sin(θ/2) >= r_ball the two ball surfaces no
// longer meet over the cusp at all — an uncut RIDGE, not a scallop —
// returned as INFINITY so a caller budgeting cusps rejects instead of
// pricing it. One home per the derive_*() discipline: the stock_to_leave
// budget (DECISIONS.md "stock_to_leave first slice…") consumes this, and
// the round-trip with derive_scallop_angle is pinned by a unit test.
pub fn derive_cusp_height(theta: f64, r_ball: f64, r_part: f64) -> f64 {
    let r = r_part + r_ball;
    let half = theta / 2.0;
    let sin_leg = r * half.psin();
    if sin_leg >= r_ball {
        return f64::INFINITY;
    }
    r * half.pcos() - (r_ball * r_ball - sin_leg * sin_leg).sqrt() - r_part
}

// The convex cusp triangle in the R → ∞ limit: two ball positions whose CL
// points sit a chord `d` apart over a FLAT meridian leave a cusp of
// r_ball − sqrt(r_ball² − (d/2)²). This is simultaneously (a) the exact price
// for the cone's flat flank, and (b) the universal convex CEILING: rewriting
// the triangle above at a fixed CL chord d gives
//   cusp(R) = sqrt((R + r_ball)² − (d/2)²) − sqrt(r_ball² − (d/2)²) − R
// (sin_leg = (R + r_ball)·sin(θ/2) = d/2 identically — the ridge condition
// d >= 2·r_ball never depends on R), whose derivative in R is
// (R + r_ball)/sqrt((R + r_ball)² − (d/2)²) − 1 > 0: strictly increasing,
// from 0 at R = 0 (a sharp corner — both ball surfaces pass through the
// corner point itself) to this flat limit. That chord-fixed monotonicity is
// the corner-roll derivation's load-bearing lemma (DECISIONS.md
// "stock_to_leave corner roll…"): it is why a sharp corner's pairs price
// near zero (and why flat-bounding them would over-reject by an order of
// magnitude), why finish_cone's stock-mode scallop re-grid — every chord
// held within the flat-scallop chord s — keeps every pair at or under the
// scallop on ANY convex piece, and the derivative fact behind the audit
// that repriced finish_sphere/finish_torus at full inflation. Ridge
// semantics match derive_cusp_height:
// balls that no longer intersect return INFINITY so a budgeting caller
// rejects instead of pricing.
pub fn derive_cusp_height_flat(d: f64, r_ball: f64) -> f64 {
    let half = d / 2.0;
    if half >= r_ball {
        return f64::INFINITY;
    }
    r_ball - (r_ball * r_ball - half * half).sqrt()
}

// `r_off` is the full normal-offset distance the CL point rides at: r_ball for
// a plain finishing pass, r_ball + a' for a roughing pass (the a'-inflated
// surface is itself a normal offset, so the two compose into one distance).
// `z_ref` is the caller's own z datum, added where each caller already added
// it (the marchers pass 0.0 — equator-relative, and 0.0 + x is exact — while
// ops::finish and stock::removals_for pass the equator plane). `r_off` is the
// full normal-offset distance: r_ball + a' for a tool centre, plain a' for the
// remaining-material boundary the stock model reads.

// Local meridian radius of curvature of the spheroid at parameter t —
// (a²cos²t + c²sin²t)^1.5 / (a·c), tightest a²/c at the pole of a prolate
// (c > a) spheroid and c²/a at the equator of an oblate one. One home: the
// scallop march and the stack pricer below both price the cusp triangle on
// this radius and must never disagree on it.

// `skin` is the roughing nominal offset a' (0.0 for a plain finishing pass —
// byte-identical output, x + 0.0 == x). A roughing pass machines the surface
// INFLATED by a', so the cusp triangle is cut by the physical ball on that
// inflated surface — local radius r_part + a' (a normal offset of a convex
// curve slackens it) — while the CL locus offsets the contact point by
// r_ball + a'. The substitutions stay distinct per the first slice's rule —
// no effective-tool shortcut (DECISIONS.md "stock_to_leave concave budget…").
//
// **This step is a SEED, not an emitted spacing** (2026-07-24, DECISIONS.md
// "Convex R2 march cap…"): the triangle reads the meridian radius at ONE
// point and this march walks a CONVEX meridian tight-end-first, so every step
// overstates its own allowance — measured +1.0% at a 0.5mm request on the
// prolate fixture. Both spacings now go through
// derive_ellipsoid_capped_step_t; what survives here is the cheap forward
// guess a bisection wants for its upper bracket.

// Distance from a meridian-half-plane point to the NOMINAL spheroid meridian
// (equator-plane-relative): the quarter ellipse (a·sin t, c·cos t),
// t ∈ [0, π/2], clamped at both ends — past the equator the nearest curve
// point is the equator rim itself, the corner-fan treatment the cone's
// distance gave its apex (the pole end is smooth, so its clamp never binds on
// a cusp tip). Dense-grid seed + Newton polish on the squared distance, the
// throat projection's shape on another closed-form meridian; like it the
// result can only OVERSTATE a leftover (a missed tighter minimum reads as more
// material, never less), the one-sidedness the ceiling rests on.

// Worst leftover-above-nominal of a spheroid pass stack given as its meridian
// parameter list, at inflation `skin` — the exact tip walk BOTH ellipsoid
// spacings verify against, and the convex-mirror sibling of
// derive_catenoid_throat_march_cusp (see it for the whole construction: CL
// points ride the inflated offset r_ball + a', tips come from the physical
// ball via meridian_pair_tips, and each tip prices against the clamped
// nominal projection minus the skin). Ridge (non-overlapping balls) surfaces
// as INFINITY.

// The spheroid scallop march's step, in BOTH modes: the convex chord seed
// above, CAPPED by the pair's own exact tip price — the family cap
// (DECISIONS.md "Family-wide march cap…") reaching the two R2 carriers whose
// pricers had to be derived first (DECISIONS.md "Convex R2 march cap…").
//
// Why this march drifts, in one line: at a fixed CL chord the convex cusp
// RISES with the local radius (derive_cusp_height_flat's chord-fixed
// monotonicity lemma), and every R2 march here walks its parameter from the
// tight end toward the slack one, so the radius read at the pass point
// under-reads the pair's own interval and the step comes out too long. The
// spheroid's meridian radius runs a²/c at the pole to c²/a at the equator,
// so on the prolate fixture it rises the whole way: measured +1.0% at a
// 0.5mm request, +0.7% at 0.2mm, +0.4% at 0.05mm. Second-order and small,
// but a scallop is a PROMISE — and in scallop+stock mode the same drift
// breaches the max-not-min CEILING outright (the emitted budget charges
// h = scallop by fiat, so a 0.5mm allowance measured 0.5014mm of real
// leftover before this cap). Fixed the way the family fixes it: fix the
// grid, not the guarantee.
//
// Inert by construction wherever the triangle is already accurate — the cap
// only ever SHORTENS a step and starts from the triangle's own chord — which
// is also why the three concave R2 marches (catenoid, pseudosphere,
// paraboloid bowl) take no cap: their mirror of the lemma runs the other way
// (derive_cusp_height_concave falls with the radius), so tight-end-first
// makes them conservative, measured −0.1% to −2.2% across the same requests.
//
// The bisection, the safe-side choice and the priced clamp all live in
// cap_step_by_tip_price; this function is the spheroid's seed plus its pricer.

// Worst-case cusp height of the ellipsoid's waterline pass stack — the
// stock_to_leave budget's stepover-mode price, the convex-only sibling of
// derive_paraboloid_stack_cusp below. The stack is the finisher's fixed grid
// of n+1 uniform-t passes (the grid depends only on a, c, stepover — never on
// the skin), and like the paraboloid there is no closed-form worst pair: the
// meridian radius varies pole-to-equator (either direction, prolate vs
// oblate) while the CL chord varies with it, so every adjacent pair is priced
// and the max returned. Each pair is priced on the local osculating circle —
// exact CL chord d at inflation `skin`, local radius R_eff = min(R(t_i),
// R(t_i+1)) + skin (min is conservative: a tighter surface cusps higher),
// CL circle R_cl = R_eff + r_ball, θ = 2·asin(d / 2R_cl), then the exact
// triangle inverse. Ridge (d >= 2·r_ball) surfaces as INFINITY through
// derive_cusp_height and propagates to the caller's error.
//
// The tight pricing extreme: the grid is fixed in the contact parameter t, so
// this is the paraboloid dome's coupling exactly — inflation slides every CL
// point outward along its own fixed normal, the chords GROW, and for a fixed
// chord the slacker inflated radius prices HIGHER too (the cusp triangle's
// flat ceiling is approached from below), so both effects run upward and the
// price maximizes at skin = allowance. Monotonicity over the operative range
// is pinned by a unit test (the honest-limit convention); the caller prices
// at the top and emits at a' = allowance − price, so the price one-sidedly
// bounds the emitted stack's true cusp.

// The mirrored cusp triangle for a *concave* surface (ball inside the curve):
// the CL circle has radius r_part - r_ball (the centre pulls toward the
// curvature centre) and the cusp sits at r_part - scallop (material is removed
// inward of the surface). Substituting (-scallop, -r_ball) into the convex
// formula produces exactly that triangle — r_ball enters the numerator only
// squared, so negation flips just the two radial sums. Named here so call
// sites say what they mean instead of passing bare negative arguments.
pub fn derive_scallop_angle_concave(scallop_height: f64, r_ball: f64, r_part: f64) -> f64 {
    derive_scallop_angle(-scallop_height, -r_ball, r_part)
}

// The concave mirror of derive_cusp_height: the cusp left between two adjacent
// ball positions separated by angle `theta` about a *concave* surface of local
// radius r_part (ball inside the curve). Unlike the scallop-angle pair this is
// NOT a sign-substitution of the convex body — the law-of-cosines root choice
// flips. The CL circle has radius R' = r_part − r_ball; the cusp sits on the
// bisector at distance D = R'·cos(θ/2) + sqrt(r_ball² − R'²·sin²(θ/2)) from
// the curvature centre (the OUTER root — material lies beyond the CL circle
// here, where for the convex triangle it lies inside), so h = r_part − D.
// Same ridge semantics as the convex form: when R'·sin(θ/2) >= r_ball the two
// ball surfaces no longer meet over the cusp — an uncut RIDGE, returned as
// INFINITY so a budgeting caller rejects instead of pricing it. Consumed by
// the stock_to_leave concave budget (DECISIONS.md "stock_to_leave concave
// budget…"); the round-trip with derive_scallop_angle_concave is pinned by a
// unit test.
pub fn derive_cusp_height_concave(theta: f64, r_ball: f64, r_part: f64) -> f64 {
    let r = r_part - r_ball;
    let half = theta / 2.0;
    let sin_leg = r * half.psin();
    if sin_leg >= r_ball {
        return f64::INFINITY;
    }
    r_part - (r * half.pcos() + (r_ball * r_ball - sin_leg * sin_leg).sqrt())
}

// Meridian CL point for a paraboloid of revolution with apex at the origin,
// z(rho) = -h·rho²/a² (apex-relative; callers add cz/z_v and flip for the
// bowl). `r_signed` is +r_ball for the dome (offset along the outward normal)
// and -r_ball for the bowl — the bowl's CL curve is the dome's mirrored in z,
// and mirroring preserves chord lengths, so one helper serves both marchers.

// R2 numeric constant-scallop spacing for the paraboloid pair, mirroring
// derive_ellipsoid_step_t above: local meridian radius of curvature
// R_part(rho) = (1 + s²)^1.5 · a²/(2h) (minimum a²/(2h) at the vertex,
// strictly increasing outward), target chord from the R1 cusp formula over a
// sphere of that radius (convex or mirrored-concave triangle per `concave`),
// then <=20 Newton iterations on the exact CL chord length. Always marches in
// increasing rho (apex/vertex -> rim/mouth); the bowl caller reverses the
// collected passes for mouth-first emission.
//
// **Which caller emits from this differs by sign** (2026-07-24, DECISIONS.md
// "Convex R2 march cap…"). The DOME's steps are a SEED only: on a convex
// meridian walked apex-first the one-point triangle overstates every step
// (measured +3.6% at a 0.5mm request, the largest drift the family has
// shown), so finish_paraboloid marches derive_paraboloid_capped_step_rho and
// this is just its upper bracket. The concave BOWL still emits from here:
// the mirrored triangle falls with the radius instead of rising, so
// vertex-first pricing is conservative — measured −0.5%/−0.3%/−0.1% at
// 0.5/0.2/0.05mm — and a cap that can only shorten steps would be inert.
//
// `skin` is the roughing nominal offset a' (0.0 for a plain finishing pass —
// byte-identical output, x ± 0.0 == x). A roughing pass machines the surface
// INFLATED by a', so the cusp triangle is cut by the physical ball on that
// inflated surface: its local radius is r_part + a' on the convex dome and
// r_part − a' on the concave bowl (an offset toward the curvature centre
// tightens the curve — the unsafe direction the first slice deferred), and
// the CL locus offsets the contact point by r_ball + a'. The three
// substitutions stay distinct per the first slice's rule — no effective-tool
// shortcut (DECISIONS.md "stock_to_leave concave budget…").

// Distance from a meridian-half-plane point to the NOMINAL paraboloid
// meridian, apex-relative in the same frame paraboloid_cl_point uses
// (r, −h·r²/a²), r ∈ [0, a], clamped at both ends — past the rim the nearest
// curve point is the rim itself (the cone's corner fan), and the apex end is
// smooth. Dense-grid seed + Newton polish on the squared distance; like every
// projection in this family the result can only OVERSTATE a leftover, the
// one-sidedness the ceiling rests on. The bowl is this curve mirrored in z and
// mirroring preserves distances, so a bowl caller could price here too — none
// does today, because its concave march needs no cap.

// Worst leftover-above-nominal of a paraboloid DOME pass stack given as its
// meridian parameter list, at inflation `skin` — the exact tip walk both dome
// spacings verify against (see derive_catenoid_throat_march_cusp for the
// construction). Dome-only by signature: r_signed is +(r_ball + skin), so the
// CL points ride the outward normal. Ridge (non-overlapping balls) surfaces as
// INFINITY.

// The paraboloid DOME's scallop march step, in both modes: the convex chord
// seed above, CAPPED by the pair's own exact tip price
// (cap_step_by_tip_price). Same scheme, same reason, and the same
// inertness-where-accurate as derive_ellipsoid_capped_step_t — read the drift
// argument there; this meridian just drifts hardest (+3.6% at 0.5mm, +2.6% at
// 0.2mm, +1.4% at 0.05mm, and 0.5049mm of real leftover against a 0.500mm
// scallop+stock ceiling), because its radius climbs L³·a²/(2h) from vertex to
// rim — the steepest rise of any carrier in the family.

// Worst-case cusp height of the paraboloid pair's waterline pass stack — the
// stock_to_leave budget's stepover-mode price (DECISIONS.md "stock_to_leave
// concave budget…"). The stack is the finisher's fixed grid of n+1 uniform-rho
// passes (the grid depends only on a, h, stepover — never on the skin), and
// unlike the sphere there is no closed-form worst pair: local radius grows
// rim-ward (slackening the cusp) while the CL chord grows rim-ward too
// (fattening it), so every adjacent pair is priced and the max returned.
//
// Each pair is priced on the local osculating circle — the same approximation
// class the scallop march itself uses: exact CL chord d between the pair's CL
// points at inflation `skin`, local radius R_eff = min(R(rho_i), R(rho_i+1))
// ∓ skin (min is conservative in both directions: a tighter surface cusps
// higher whether the ball rides outside or inside it), central angle
// θ = 2·asin(d / 2R_cl) on the CL circle R_cl = R_eff ± r_ball, then the
// exact triangle inverse (derive_cusp_height / _concave). By construction
// R_cl·sin(θ/2) = d/2, so the ridge condition surfaces as d >= 2·r_ball —
// adjacent balls that no longer intersect — and INFINITY propagates to the
// caller's error. A non-positive concave R_cl (ball at or past the inflated
// curvature radius) is also INFINITY here, defensively: the caller runs the
// stock-aware gouge guard first, which rejects the strict case with the
// better message.
//
// The skin to price at is the extreme that maximizes the cusp — and for THIS
// stack that extreme is decided by the grid–inflation coupling, not by
// convexity alone. The grid is fixed in the contact parameter rho, so
// inflation slides every CL point along its own fixed normal: on the convex
// dome the CL circle fattens, the chords GROW, and the growth outruns the
// flatter surface's conformity gain — the price maximizes at skin =
// allowance; on the concave bowl the CL circle tightens, the chords SHRINK,
// and the ball conforms better to the tighter surface — BOTH effects run
// downward, so the price maximizes at skin = 0 (nominal). Note this INVERTS
// the sphere slice's fixed-Z-grid intuition (there inflation compresses the
// convex apex angle, and the first slice's note warned the concave mirror
// would under-budget — true for a fixed-Z stack, not for this fixed-rho
// one). Monotonicity over the operative skin range is pinned by unit tests
// beside the round-trip tests below; the caller prices at the tight extreme
// and emits at a' = allowance − price <= allowance, so the price one-sidedly
// bounds the emitted stack's true cusp — stated conservatism of
// O(a'·∂h/∂skin), the same class as the sphere slice's.

// Meridian CL point for a catenoid neck, waist-relative: contact
// (rho, z) = (c·cosh w, −c·w) with w = 0 at the waist, growing toward the
// base (callers add z_w). The meridian tangent going up is (−sinh w, 1), so
// the outward normal is (1, sinh w)/L with L = sqrt(1 + sinh²w) = cosh w —
// the cosh identity collapses the normalization:
//   rho_cl = c·cosh w + r_off/cosh w
//   z_cl   = −c·w + r_off·tanh w
// `r_off` is the full normal-offset distance the CL point rides at: r_ball for
// a plain finishing pass, r_ball + a' for a roughing pass (the a'-inflated
// surface is itself a normal offset, so the two compose into one distance —
// the identity is linear in the offset, nothing breaks under inflation).
// `z_ref` as on the throat: the marchers pass 0.0 (waist-relative), ops::finish
// and stock::removals_for pass the waist plane z_w.

// R2 numeric constant-scallop spacing for the catenoid neck, mirroring
// derive_paraboloid_step_rho above: local meridian radius of curvature
// R_part(w) = c·cosh²w (tightest c at the waist, strictly increasing toward
// the base), and the meridian is *concave toward the tool* — the catenoid is
// a minimal surface, so the convex hoop curvature cancels it exactly — which
// makes the target chord the mirrored-concave cusp triangle over a circle of
// that radius (CL circle radius R_part − r_ball), then <=20 Newton iterations
// on the exact CL chord length. Marches in increasing w (waist → base), which
// is already the top-first emission order — no reversal at the call site. At
// the legal boundary r_ball == c the concave triangle degenerates at the
// waist (CL radius 0); the caller's forward-progress guard absorbs it, same
// as the bowl's vertex.
//
// `skin` is the roughing nominal offset a' (0.0 for a plain finishing pass —
// byte-identical output, x ± 0.0 == x). A roughing pass machines the surface
// INFLATED by a', so the cusp triangle is cut by the physical ball on that
// inflated surface: local radius r_part − a' (an offset toward the curvature
// centre tightens a concave curve), while the CL locus offsets the contact
// point by r_ball + a'. The three substitutions stay distinct per the first
// slice's rule — no effective-tool shortcut (DECISIONS.md "stock_to_leave
// concave budget…").

// One adjacent-pair price for a concave-meridian band stack: exact CL chord d
// at inflation `skin`, local osculating radius r_loc at the pair's tight
// member, r_eff = r_loc − skin (the a'-tightened surface), CL circle
// r_eff − r_ball, then the exact concave triangle inverse. A non-positive
// CL radius is INFINITY here, defensively: the caller runs the stock-aware
// gouge guard first, which rejects the strict case with the better message —
// at exact equality (r_ball == band minimum radius, legal for the guard) the
// tight pair collapses and the caller's ridge error reports it instead.
// Shared by the two band pricers below only; the paraboloid/ellipsoid pricers
// predate it and stay untouched.

// Worst-case cusp height of the catenoid neck's waterline pass stack — the
// stock_to_leave budget's stepover-mode price for the band (DECISIONS.md
// "stock_to_leave anticlastic bands…"). Mirrors derive_paraboloid_stack_cusp
// above (see its comment for the full pricing argument): the stack is the
// finisher's fixed grid of n+1 uniform-w passes over [w_t, w_b] (uniform in w
// IS the fixed-Z waterline — z is linear in w — and the grid depends only on
// the dims, never on the skin), every adjacent pair is priced on the local
// osculating circle at the pair's tight member (R_part = c·cosh²w is smallest
// at the lower w), and the max is returned. Taking the band endpoints
// directly lets the truncated form ride free — w_t is just where the grid
// starts.
//
// The grid is fixed in the contact parameter and the meridian is concave
// toward the tool, so this is the bowl's coupling class: inflation tightens
// the CL circle, shrinks the chords, and conforms the ball to the tighter
// surface — both effects run downward, the price maximizes at skin = 0
// (nominal). Monotonicity is pinned by a unit test; the caller prices at
// nominal and emits at a' = allowance − price.

// Meridian CL point for a catenoid throat, waist-relative: contact
// (rho, z) = (c·cosh w, c·w) with w = 0 at the waist, growing toward the rim
// (callers add z_w = stock top − depth). Same meridian as catenoid_cl_point
// above, offset along the *into-void* normal instead: the tangent going up is
// (sinh w, 1)/cosh w, so the void-side normal is (−1, sinh w)/cosh w and
//   rho_cl = c·cosh w − r_ball/cosh w
//   z_cl   = c·w + r_ball·tanh w
// `z_ref` is the caller's own z datum, added in the same order each caller
// used before this became shared: the marchers below pass 0.0 (this curve is
// waist-relative to them, and 0.0 + x is exact), ops::finish and
// stock::removals_for pass the waist plane z_w. `r_ball` is the full
// normal-offset distance — r_ball + a' for a tool centre, plain a' for the
// remaining-material boundary the stock model reads.

// R2 numeric constant-scallop spacing for the catenoid throat. Same meridian
// radius R_part(w) = c·cosh²w as the outside neck, but seen from the void
// side the meridian is *convex toward the tool* — so the target chord is the
// plain convex cusp triangle (CL circle radius R_part + r_ball; the ball
// centre sits on the far side of the meridian's curvature centre), then <=20
// Newton iterations on the exact CL chord length. Marches waist→rim in
// increasing w (every R2 marcher here walks its parameter up from the small
// end, evaluating R_part at the march point — first-order exact, the
// across-step drift of R is second-order). Unlike the concave neck/band
// marchers, whose tight-end-first order makes each step strictly
// conservative, the convex chord *shrinks* slowly with growing R_part
// (toward the flat-surface limit), so the current-point evaluation slightly
// overestimates each step's allowance — the same direction, and the same
// second-order magnitude, as the paraboloid dome's established march.
// Bottom-first here, reversed at the call site for top-first emission like
// the pseudosphere.
//
// `skin` is the roughing nominal offset a' (0.0 for a plain finishing pass —
// byte-identical, x + 0.0 == x): the triangle prices the PHYSICAL ball on the
// inflated surface (convex meridian, so the local radius grows to R + a' —
// the first slice's three-substitutions rule) and the CL rides at
// r_ball + a'. Under stock the march is only the SPACING heuristic: the
// convex current-point drift above means its cusp is not exact, so the
// finisher prices the emitted grid by exact tips
// (derive_catenoid_throat_march_cusp) and the guarantee rests there
// (DECISIONS.md "stock_to_leave inside funnels…").
//
// **This step is a SEED, not an emitted spacing** (2026-07-24, DECISIONS.md
// "Family-wide march cap…"). Nothing emits from it any more: the honest
// conservatism note above is a measured +2.1% at a 0.5mm request (0.5106mm
// of real ridge), and a `scallop` is a promise rather than an estimate, so
// both spacings go through derive_catenoid_throat_capped_step_w — which
// starts from this chord and caps it by the pair's own exact tip price. What
// survives here is the cheap forward guess, which is exactly what a
// bisection wants for its upper bracket.

// Distance from a meridian-half-plane point to the NOMINAL throat meridian
// (waist-relative): the catenary segment (c·cosh w, c·w), w ∈ [0, w_b],
// clamped at both ends — beyond the rim (or below the waist) the nearest
// curve point is the endpoint itself, the corner-fan treatment the cone's
// distance gave its apex. Dense-grid seed + Newton polish on the squared
// distance, the spline projection's shape on a closed-form curve; like it,
// the result can only OVERSTATE a leftover (a missed tighter minimum reads
// as more material, never less), the one-sidedness the ceiling rests on.

// Worst leftover-above-nominal of a throat pass stack given as its meridian
// parameter list, at inflation `skin` — the exact tip walk BOTH throat
// spacings verify against (DECISIONS.md "stock_to_leave inside funnels…").
// The throat's convex march carries the honest current-point drift, so under
// stock it is only the spacing heuristic and the ceiling rests here — the
// mixed-meridians rule extended to the closed-form funnels: exact tips
// everywhere, no osculating budget. CL points ride the inflated offset
// (r_ball + a'), tips come from the physical ball (meridian_pair_tips), and
// each tip prices against the clamped nominal projection minus the skin.
// Ridge (non-overlapping balls) surfaces as INFINITY.

// The throat scallop march's step, in BOTH modes: the convex chord seed above,
// CAPPED by the pair's own exact tip price. This is the tangent-lip funnel's
// fix carried across the family (DECISIONS.md "Family-wide march cap…"): the
// cusp triangle reads the local radius at ONE point, so on a convex meridian
// walked from the tight end it overstates every step's allowance, and the
// throat's measured overshoot is +2.1% at a 0.5mm request (0.5106mm of real
// ridge), +1.5% at 0.2mm. Second-order and small — nothing like the tangent
// lip's 12× — but a scallop is a PROMISE, and shipping 0.5106mm against a
// stated 0.500mm is the same category of wrong output at a gentler size.
// Fixed the same way: fix the grid, not the guarantee.
//
// Inert by construction wherever the triangle is already accurate — the cap
// only ever SHORTENS a step and it starts from the triangle's own chord — so
// the whole cost is the scallop goldens the drift had been holding still.
//
// Bisection on the single pair's price, returning the safe side (the funnel's
// scheme verbatim): the price falls continuously to zero as the step closes,
// because two CL points a hair apart put their cusp tip a hair off the
// surface, so the walk always converges to a forward step. A non-overlapping
// candidate prices INFINITY and shrinks like any other breach. The w_b clamp
// is priced too — hi_cand caps at the rim, so the final pass pair is capped
// like every other, not exempted by the clamp.

// Pseudosphere meridian parameter for a circle of radius rho on the tractrix
// with cusp radius c: rho(u) = c·sech u inverts to u = acosh(c/rho). One home
// per the derive_*() discipline — the band ends (u_base, u_tip), the height,
// the gouge-guard radius, and the tests all consume this, never restate it.
// Well-defined for every legal band (validate() enforces 0 < rho < c, i.e.
// 0 < tip_dia < base_dia < dia, so c/rho > 1 and acosh is real).

// Pseudosphere band height (CHECKLIST row 5.3, second member): with both
// truncations forced (see the variant's doc comment) the height is pinned by
// the meridian, h = c·[(u_t − tanh u_t) − (u_b − tanh u_b)] — z(u) =
// c·(u − tanh u) above the cusp plane, evaluated at the two authored circles.
// Depth of a tractrix circle below the cusp plane, z(u) = c·(u − tanh u) — the
// quantity derive_pseudosphere_height differences, and the `d_b` mouth datum
// ops::finish and stock::removals_for both need. One home for the point value;
// the height keeps its own factored form, whose association the goldens pin.

// Meridian CL point for a pseudosphere band, cusp-plane-relative: contact
// (rho, z) = (c·sech u, c·(u − tanh u)) with u = 0 at the cusp rim, growing
// toward the tip (callers subtract the base circle's height and add z0). The
// meridian tangent going up is (−sech u, tanh u) — already unit length, by
// sech²u + tanh²u = 1, this surface's analogue of the catenoid's cosh
// identity — so the outward normal is (tanh u, sech u) and
//   rho_cl = c·sech u + r_off·tanh u
//   z_cl   = c·(u − tanh u) + r_off·sech u
// `r_off` is the full normal-offset distance the CL point rides at: r_ball for
// a plain finishing pass, r_ball + a' for a roughing pass (the a'-inflated
// surface is itself a normal offset, so the two compose into one distance —
// the identity is linear in the offset, nothing breaks under inflation).
// `z_ref` as on the funnel: the marchers pass 0.0 (cusp-plane-relative),
// ops::finish and stock::removals_for pass the cusp plane.

// R2 numeric constant-scallop spacing for the pseudosphere band, mirroring
// derive_catenoid_step_w above: local meridian radius of curvature
// R_part(u) = c·sinh u (tightest at the BASE circle, strictly increasing
// toward the tip — the opposite end from the catenoid's waist), and the
// meridian is *concave toward the tool* — constant negative curvature: the
// convex hoop radius c/sinh u times this is always −c² — so the target chord
// is the mirrored-concave cusp triangle over a circle of that radius (CL
// circle radius R_part − r_ball), then <=20 Newton iterations on the exact CL
// chord length. Marches in increasing u (base → tip) so R_part at the march
// point is the tightest over each step's interval — the conservative bound,
// the same reason the catenoid marches waist-first. Here that is the
// *bottom*-first order, so the call site reverses the passes for the usual
// top-first emission — the first op where the two orders differ. At the legal
// boundary r_ball == c·sinh(u_base) the concave triangle degenerates at the
// base circle (CL radius 0); the caller's forward-progress guard absorbs it,
// same as the catenoid's waist.
//
// `skin` is the roughing nominal offset a' (0.0 for a plain finishing pass —
// byte-identical output, x ± 0.0 == x). A roughing pass machines the surface
// INFLATED by a', so the cusp triangle is cut by the physical ball on that
// inflated surface: local radius r_part − a' (an offset toward the curvature
// centre tightens a concave curve), while the CL locus offsets the contact
// point by r_ball + a'. The three substitutions stay distinct per the first
// slice's rule — no effective-tool shortcut (DECISIONS.md "stock_to_leave
// concave budget…").

// Worst-case cusp height of the pseudosphere band's waterline pass stack —
// the stepover-mode price, the tractrix sibling of derive_catenoid_stack_cusp
// above (see derive_paraboloid_stack_cusp for the full pricing argument). The
// finisher's grid is n+1 passes uniform in meridian arc length s (exact
// closed form: geometric in rho, u_i = acosh(cosh(u_b)·e^{s_i/c}) with
// s_total = c·ln(r_base/r_tip)) — reconstructed here verbatim, and dependent
// only on the dims, never the skin. R_part = c·sinh u is smallest at the
// lower u — the base circle, where the gouge guard also binds — so each pair
// prices at its lower-u member. Same concave fixed-parameter coupling as the
// catenoid: the price maximizes at skin = 0 (nominal), pinned by a unit test;
// the caller prices there and emits at a' = allowance − price.

// The leftover peak between one adjacent pass pair, constructed EXACTLY:
// the two ball circles (radius r_ball about each meridian CL point) cross at
// two points; the cusp tip is the crossing nearer the surface, and the pair's
// leftover-above-nominal is that tip's distance to the nominal meridian. The
// piecewise carriers (cone, ogive) price with this instead of an osculating
// triangle because a pair straddling a corner-roll seam touches two surface
// pieces and no single triangle prices it — the tip construction doesn't
// care. Returns the two candidate crossings; the caller picks by distance
// (the far crossing sits away from the material). None = the balls no longer
// cross: an uncut RIDGE, the caller's INFINITY.
// The exact-tip stack walk itself, one home for every carrier that owns one
// (2026-07-24, DECISIONS.md "Convex R2 march cap…"). Five carriers price their
// emitted grid this way — throat, tractrix funnel, spline cap, spline funnel,
// and the convex R2 pair — and the walk was identical in all five up to two
// functions: where a march parameter puts the CL point, and how far a
// half-plane point is from the nominal meridian. Those two are what each
// `derive_*_march_cusp` supplies; everything else — the inflated offset the CL
// rides (r_ball + a'), the physical-ball tip construction, the min-of-two-tips
// convention, the leftover − skin bookkeeping, and RIDGE as INFINITY — is the
// same guarantee everywhere and now lives once.
//
// The degenerate-chord skip is part of that guarantee, not an optimization: a
// pair whose CL points coincide (a clamped final pass landing on its
// predecessor) has no cusp between them, and meridian_pair_tips would divide
// by its zero chord.
pub(super) fn march_cusp_walk(
    params: &[f64],
    r_ball: f64,
    skin: f64,
    cl: impl Fn(f64) -> (f64, f64),
    dist: impl Fn(f64, f64) -> f64,
) -> f64 {
    let mut worst: f64 = 0.0;
    for pair in params.windows(2) {
        let (p1, p2) = (cl(pair[0]), cl(pair[1]));
        let d = ((p2.0 - p1.0).powi(2) + (p2.1 - p1.1).powi(2)).sqrt();
        if d < 1e-9 {
            continue;
        }
        match meridian_pair_tips(p1, p2, r_ball) {
            None => return f64::INFINITY,
            Some((t1, t2)) => {
                let leftover = dist(t1.0, t1.1).min(dist(t2.0, t2.1));
                worst = worst.max(leftover - skin);
            }
        }
    }
    worst
}

// The cap's policy, one home for the same five carriers: take the cusp
// triangle's forward guess as an upper bracket and shorten it until the pair's
// own exact tip price holds the request (DECISIONS.md "Family-wide march
// cap…", then "Convex R2 march cap…"). `hi` is the march's terminal parameter,
// so a seed that stalls, regresses or overruns falls back to the clamp — and
// the clamp is PRICED like any other candidate rather than exempted, which is
// what makes the final pass pair as honest as the rest.
//
// Bisection rather than Newton because the price is only piecewise-smooth (it
// switches which of the two tips is nearer the surface, and jumps to INFINITY
// when the balls stop overlapping); 60 iterations exhausts f64 on every
// parameter range in the family. Returning `lo` returns the safe side by
// construction: the price falls continuously to zero as the step closes, since
// two CL points a hair apart put their cusp tip a hair off the surface, so a
// forward step always exists to converge on.
pub(super) fn cap_step_by_tip_price(x: f64, hi: f64, seed: f64, scallop: f64, price: impl Fn(f64) -> f64) -> f64 {
    let hi_cand = if seed > x && seed <= hi { seed } else { hi };
    if price(hi_cand) <= scallop {
        return hi_cand;
    }
    let (mut lo, mut hi_b) = (x, hi_cand);
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi_b);
        if price(mid) <= scallop {
            lo = mid;
        } else {
            hi_b = mid;
        }
    }
    lo
}

pub(super) fn meridian_pair_tips(p1: (f64, f64), p2: (f64, f64), r_ball: f64) -> Option<((f64, f64), (f64, f64))> {
    let (dr, dz) = (p2.0 - p1.0, p2.1 - p1.1);
    let d = (dr * dr + dz * dz).sqrt();
    if d >= 2.0 * r_ball {
        return None;
    }
    let q = (r_ball * r_ball - (d / 2.0) * (d / 2.0)).sqrt();
    let (mr, mz) = ((p1.0 + p2.0) / 2.0, (p1.1 + p2.1) / 2.0);
    Some(((mr + q * dz / d, mz - q * dr / d), (mr - q * dz / d, mz + q * dr / d)))
}

// Distance from a meridian point to the NOMINAL cone (base-relative): the
// nearer of the two sharp corners (apex; base rim) and, where the foot of the
// perpendicular falls between them, the distance to the flank itself. The
// flank is a SEGMENT with a corner fan at each end, exactly the shape
// ogive_meridian_distance uses — so the min never reads the flank's extension
// through a corner.
//
// The rim fan is fork 2B's (DECISIONS.md → "the cone rolls its rim…"). This
// used to run the flank as a LINE past the base rim — the "flank-wall
// convention", wall-only skin at full depth — which existed to give the
// clamped bottom pass a surface to be an offset OF. That pass rolls its corner
// now, so the wall it was measured against is gone, and pricing a tip near the
// rim against the extension would under-report the leftover: the extension is
// always nearer than the corner it fans around.
pub(super) fn cone_meridian_distance(rho: f64, z: f64, r_part: f64, height: f64) -> f64 {
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

// The cone's inflated CL locus at height `z`, in three pieces: the apex corner
// sphere above the upper seam, the parallel flank between the seams, and the
// base-rim corner sphere below the lower one. `r_cap` is the whole stand-off
// (r_ball + skin); `cz` is the base plane.
//
// ONE home, read by both the emitter (finish_cone's three arms) and the pricer
// (derive_cone_stack_cusp), because the sweep-then-VERIFY guarantee rests on
// the pricer walking the stack that is actually emitted. It used to be
// reconstructed "verbatim" in the pricer, which is the same drift slice 0
// rejected when it put the tail in one place.
//
// The two corner pieces are the same construction at opposite ends, and the
// locus is continuous at both seams: at the lower one the flank gives
// r_cap·cosα + h·tanα = a + r_cap·cosα, which is what the roll gives too.
pub(crate) fn cone_cl_rho(z: f64, cz: f64, a: f64, h: f64, r_cap: f64) -> f64 {
    let l = (a * a + h * h).sqrt();
    let (sin_alpha, cos_alpha, tan_alpha) = (a / l, h / l, a / h);
    let z_apex = cz + h;
    let z_trans = z_apex + r_cap * sin_alpha;
    if z > z_trans {
        let dz = z - z_apex;
        (r_cap * r_cap - dz * dz).max(0.0).sqrt()
    } else if z < cz + r_cap * sin_alpha {
        let dz = z - cz;
        a + (r_cap * r_cap - dz * dz).max(0.0).sqrt()
    } else {
        r_cap * cos_alpha + (z_trans - z) * tan_alpha
    }
}

// Worst leftover-above-nominal of the cone's waterline pass stack at
// inflation `skin` — the stepover-mode price for the first sharp-apex
// carrier (DECISIONS.md "stock_to_leave corner roll…"), returned as a cusp
// height ABOVE the inflated surface (leftover − skin) so the caller's
// a' = allowance − price decomposition reads as everywhere else. The grid is
// the finisher's uniform-Z ladder over the INFLATED locus (top at
// z_apex + r_ball + skin — the stack must reach the inflated cap's pole, or
// the polar leftover alone can exceed the allowance), reconstructed here
// verbatim in base-relative z. Every pair is priced by the exact tip
// construction (meridian_pair_tips) against the exact nominal distance — no
// osculating bound anywhere, which matters twice at a sharp corner: corner
// pairs genuinely price near zero at skin = 0 and grow with the skin (the
// corner-roll finding — derive_cusp_height_flat's chord-fixed monotonicity
// lemma explains why any flat/slack bound would over-price them ~10×), and
// seam-straddling pairs, which touch two surface pieces, have no triangle
// at all. Ridge (non-overlapping balls) surfaces as INFINITY.
//
// The price is NOT monotone in the skin: the grid top moves with the
// inflation, so which pair lands astride the cap/flank seam — and how fat
// its chord is — shifts alignment as the skin grows (observed while pinning
// this slice: the fixture geometry prices 0.263 at skin 0, 0.298 at 0.4,
// 0.085 at 0.5). That killed the extreme-walk scheme every earlier carrier
// used: NO single walk one-sidedly bounds the emitted stack. The caller
// (finish_cone, finish_ogive alike) instead takes the max over a 5-point
// skin sweep {0, ¼, ½, ¾, 1}·allowance to choose a', then VERIFIES the
// actual emitted stack — price(a') — and rejects if the ceiling is breached,
// so the guarantee rests on the exact verification, never on a sweep
// having caught the worst alignment.
pub fn derive_cone_stack_cusp(r_part: f64, height: f64, r_ball: f64, skin: f64, stepdown: f64) -> f64 {
    let r_cap = r_ball + skin;
    let z_top = height + r_cap;

    let cl = |z: f64| -> (f64, f64) { (cone_cl_rho(z, 0.0, r_part, height, r_cap), z) };

    let n = (z_top / stepdown).ceil().max(1.0) as usize;
    let mut worst: f64 = 0.0;
    for i in 0..n {
        let za = (z_top - i as f64 * stepdown).max(0.0);
        let zb = (z_top - (i + 1) as f64 * stepdown).max(0.0);
        if (za - zb).abs() < 1e-12 {
            continue;
        }
        match meridian_pair_tips(cl(za), cl(zb), r_ball) {
            None => return f64::INFINITY,
            Some((t1, t2)) => {
                let leftover = cone_meridian_distance(t1.0, t1.1, r_part, height)
                    .min(cone_meridian_distance(t2.0, t2.1, r_part, height));
                worst = worst.max(leftover - skin);
            }
        }
    }
    worst
}

// Distance from a meridian point to the NOMINAL ogive (base-relative): the
// nearer of the two sharp corners (apex; base rim) and, where the point's
// bearing from the arc centre falls inside the arc's angular span, the
// distance to the arc itself. Points beyond either end of the arc bear
// outside the span (the corner fans), so the min never reads the arc's
// extension through a corner.

// Worst leftover-above-nominal of the ogive's waterline pass stack at
// inflation `skin`, returned as a cusp height above the inflated surface —
// derive_cone_stack_cusp's sibling for the arc-bodied sharp-apex carrier
// (see its comment for the whole pricing story: exact tip construction per
// pair, no osculating bounds, the non-monotone-in-skin finding, and the
// caller's sweep-then-VERIFY scheme that carries the guarantee). Same
// uniform-Z ladder as the finisher (top at z_apex + r_ball + skin, the
// inflated cap's pole), three CL pieces: apex corner sphere, concentric
// offset arc about the meridian centre, and (secant form) the base-rim
// corner sphere. At h == a (the hemisphere) the cap is empty, every tip
// prices against the plain arc, and the crown pair reproduces exactly the
// repriced finish_sphere's full-inflation crown price — the
// reduces-to-sphere anchor carries through the pricing.

// Distance from a meridian point to the NOMINAL vase ogee (base-relative):
// the nearer of the two arcs — neck (concave, about O2 = (c + r2, h)) and
// belly (convex, about O1 = (a − r1, 0)) — each read only where the point's
// bearing from its centre falls inside the arc's angular span, plus the two
// vertical end tangents as HALF-LINES (waist rho = c upward, equator rho = a
// downward — the flank-wall convention: wall-only skin, the wall priced over
// its whole span) and the G1 junction point itself, which closes the bearing
// gap a far-side point can fall into. The junction needs no corner fan: the
// ogee is tangent-continuous there, which is exactly why the vase has no
// corner-roll piece — its straddling pairs are priced by the exact tip
// construction alone.

// Worst leftover-above-nominal of the vase's stepover pass stack at inflation
// `skin`, returned as a cusp height above the inflated surface — the mixed-
// curvature heir of derive_cone_stack_cusp's pricing story (exact tip
// construction per pair, no osculating bounds, sweep-then-VERIFY at the
// caller). The grid is the finisher's uniform-arc ladder over the INFLATED
// ogee, reconstructed verbatim in base-relative z: offsetting the ogee by a'
// keeps both arcs concentric (neck r2 − a', belly r1 + a'; the total arc
// (r1 + r2)·θ is split-invariant AND skin-invariant, but the junction's
// arc-length position moves with the skin — which is why no extreme walk is
// claimed and the caller verifies the emitted stack). Pairs straddling the
// G1 junction touch both arcs and have no single osculating triangle; the
// tip construction doesn't care. Ridge (non-overlapping balls) surfaces as
// INFINITY.

// Distance from a meridian-plane point to the NOMINAL spline meridian,
// base-relative. A spline has no closed-form point-to-curve distance, so:
// dense seed per knot span (the guard scan's density discipline — a many-knot
// meridian cannot hide a near approach between samples of a fixed whole-domain
// count), then a Newton polish on the stationarity condition
// g(u) = (C(u) − P)·C'(u) with g'(u) = |C'|² + (C − P)·C'' (exact derivatives
// from de Boor). The returned value is the distance to an actual curve point,
// so it can only OVERestimate the true minimum — the safe side for a ceiling:
// a leftover read through this function is never understated, and the
// overstatement after the polish is far below emission resolution. This is
// what separates the spline from the Mesh refusal: the residual here is
// bounded and driven to machine precision, not an unbounded between-sample
// gap.
pub(crate) fn spline_meridian_distance(meridian: &crate::nurbs::BSplineCurve, rho: f64, z: f64) -> f64 {
    const SAMPLES_PER_SPAN: usize = 64;
    let knots = meridian.distinct_knots();
    let mut best_u = knots[0];
    let mut best_d2 = f64::INFINITY;
    for pair in knots.windows(2) {
        for k in 0..=SAMPLES_PER_SPAN {
            let u = pair[0] + (pair[1] - pair[0]) * (k as f64 / SAMPLES_PER_SPAN as f64);
            let (r, zz) = meridian.eval(u);
            let d2 = (r - rho).powi(2) + (zz - z).powi(2);
            if d2 < best_d2 {
                best_d2 = d2;
                best_u = u;
            }
        }
    }
    let (u0, u1) = meridian.domain();
    let mut u = best_u;
    for _ in 0..12 {
        let ((r, zz), (dr, dz), (ddr, ddz)) = meridian.eval_d2(u);
        let (er, ez) = (r - rho, zz - z);
        let g = er * dr + ez * dz;
        let gp = dr * dr + dz * dz + er * ddr + ez * ddz;
        if gp.abs() < 1e-12 {
            break;
        }
        let step = g / gp;
        u = (u - step).clamp(u0, u1);
        if step.abs() < 1e-12 {
            break;
        }
    }
    let (r, zz) = meridian.eval(u);
    let d_polished = ((r - rho).powi(2) + (zz - z).powi(2)).sqrt();
    d_polished.min(best_d2.sqrt())
}

// Worst leftover-above-nominal of a spline pass stack given as its meridian
// parameter list, at inflation `skin` — the ceiling walk BOTH spline spacings
// verify against (DECISIONS.md "stock_to_leave mixed meridians…"). The
// stepover pricer below hands it the uniform-arc grid; the scallop march
// hands it the parameters it actually emitted, because on a user-authored
// meridian the march's osculating triangle is a pointwise estimate whose
// across-step curvature drift is unbounded in principle — so the guarantee
// rests on this exact tip walk, never on the triangle. Pairs are priced by
// meridian_pair_tips against the numerically-projected nominal distance;
// ridge (non-overlapping balls) surfaces as INFINITY.
pub fn derive_spline_march_cusp(meridian: &crate::nurbs::BSplineCurve, r_ball: f64, skin: f64, us: &[f64]) -> f64 {
    march_cusp_walk(
        us,
        r_ball,
        skin,
        |u| spline_revolve_cl_point(meridian, r_ball + skin, u),
        |rho, z| spline_meridian_distance(meridian, rho, z),
    )
}

// The stepover-mode price: the uniform-arc waterline grid the finisher emits
// at inflation `skin`, priced by the exact tip walk above. The grid is
// rebuilt through the same one home the finisher reads
// (spline_uniform_arc_us), so pricer and emitter cannot drift; like the
// cone/ogive pricers the value is NOT claimed monotone in the skin — the
// caller sweeps to choose a' and verifies the emitted stack.
pub fn derive_spline_stack_cusp(meridian: &crate::nurbs::BSplineCurve, r_ball: f64, skin: f64, stepover: f64) -> f64 {
    derive_spline_march_cusp(meridian, r_ball, skin, &spline_uniform_arc_us(meridian, stepover, skin, false))
}

// The funnel's exact tip walk — derive_spline_march_cusp with the void-side
// CL home (spline_revolve_funnel_cl_point; the sides can't share a flip, see
// the two CL points' comments). The tip construction and the numeric
// projection are orientation-agnostic — meridian_pair_tips intersects two
// ball circles wherever they sit, and the min of the two tips' distances
// picks the material-side cusp on either side of the meridian — so only the
// CL home differs. Same one-sided honesty argument: the projection can only
// OVERSTATE a leftover, never hide one.
pub fn derive_spline_funnel_march_cusp(meridian: &crate::nurbs::BSplineCurve, r_ball: f64, skin: f64, us: &[f64]) -> f64 {
    march_cusp_walk(
        us,
        r_ball,
        skin,
        |u| spline_revolve_funnel_cl_point(meridian, r_ball + skin, u),
        |rho, z| spline_meridian_distance(meridian, rho, z),
    )
}

// The funnel's stepover-mode price, derive_spline_stack_cusp's void-side
// twin: the same one-home grid rebuild (into_void = true — the arc is
// measured along the funnel's inflated meridian), priced by the funnel tip
// walk. Not claimed monotone in the skin; the caller sweeps and verifies.
pub fn derive_spline_funnel_stack_cusp(meridian: &crate::nurbs::BSplineCurve, r_ball: f64, skin: f64, stepover: f64) -> f64 {
    derive_spline_funnel_march_cusp(meridian, r_ball, skin, &spline_uniform_arc_us(meridian, stepover, skin, true))
}

// Uniform-meridian-arc-length waterline parameters for a spline surface of
// revolution, emitted first-to-last in the curve's own parameter direction
// (apex→base for the cap, mouth→tip for the funnel — both top-first). The
// spline has no closed-form length, so build a dense cumulative table
// (deterministic, fixed sample count) and invert it for the pass parameters.
// One home for both orientations AND for the stack pricer above, so the
// waterlines and the price cannot drift into slightly-different numerics.
// `skin` measures the arc along the INFLATED meridian (the offset curve at
// a' along the machined side's normal — the surface the roughing pass
// actually machines, so "uniform stepover on the surface" keeps meaning the
// surface being cut); 0.0 takes the nominal eval path verbatim,
// byte-identical to the pre-stock table. `into_void` names which side the
// inflation offsets toward — false = the cap's up-normal, true = the
// funnel's into-void normal (the sides can't share a flip, see the two CL
// points' comments); at skin 0.0 the sides coincide on the nominal meridian
// and the flag is inert.
pub fn spline_uniform_arc_us(meridian: &crate::nurbs::BSplineCurve, stepover: f64, skin: f64, into_void: bool) -> Vec<f64> {
    let (u0, u1) = meridian.domain();
    const ARC_SAMPLES: usize = 2000;
    let at = |u: f64| -> (f64, f64) {
        if skin > 0.0 {
            if into_void {
                spline_revolve_funnel_cl_point(meridian, skin, u)
            } else {
                spline_revolve_cl_point(meridian, skin, u)
            }
        } else {
            meridian.eval(u)
        }
    };
    let mut us = Vec::with_capacity(ARC_SAMPLES + 1);
    let mut cum = Vec::with_capacity(ARC_SAMPLES + 1);
    let mut s = 0.0;
    let mut prev = at(u0);
    us.push(u0);
    cum.push(0.0);
    for k in 1..=ARC_SAMPLES {
        let u = u0 + (u1 - u0) * (k as f64 / ARC_SAMPLES as f64);
        let cur = at(u);
        s += ((cur.0 - prev.0).powi(2) + (cur.1 - prev.1).powi(2)).sqrt();
        us.push(u);
        cum.push(s);
        prev = cur;
    }
    let total = s;

    // Invert cumulative arc length s_target -> u by linear interpolation in
    // the table (monotonic in s, so a forward scan with a moving cursor
    // suffices).
    let u_at_arc = |s_target: f64, cursor: &mut usize| -> f64 {
        while *cursor + 1 < cum.len() && cum[*cursor + 1] < s_target {
            *cursor += 1;
        }
        if *cursor + 1 >= cum.len() {
            return u1;
        }
        let (s_lo, s_hi) = (cum[*cursor], cum[*cursor + 1]);
        let (u_lo, u_hi) = (us[*cursor], us[*cursor + 1]);
        if s_hi <= s_lo {
            return u_lo;
        }
        u_lo + (u_hi - u_lo) * (s_target - s_lo) / (s_hi - s_lo)
    };

    let n = (total / stepover).ceil().max(1.0) as usize;
    let mut out = Vec::with_capacity(n + 1);
    let mut cursor = 0usize;
    for i in 0..=n {
        let s_target = (i as f64 / n as f64) * total;
        out.push(u_at_arc(s_target, &mut cursor));
    }
    out
}

// Meridian CL point for a pseudosphere funnel, mouth-plane-relative: contact
// (rho, depth-below-mouth) = (c·sech u, c·[(u − tanh u) − (u_b − tanh u_b)])
// with u growing DOWNWARD from the mouth circle at u_b (callers pass the
// contact depth term relative to the mouth by subtracting d_b = u_b's
// meridian height; here the caller supplies d_b so this helper stays a pure
// function of u). Same tractrix as pseudosphere_cl_point above, offset along
// the *into-void* normal instead: the tangent going down is
// −(sech u, tanh u), so the void-side normal is (−tanh u, sech u) —
// sech²u + tanh²u = 1 once more — and
//   rho_cl = c·sech u − r_ball·tanh u   (= sech u·(c − r_ball·sinh u): the
//            ring radius hits zero exactly at the hoop-guard boundary)
//   z_cl   = −c·[(u − tanh u) − d_b/c] + r_ball·sech u   (below the mouth)
// `z_ref`, like the throat's: the marchers pass 0.0 (cusp-plane-relative, and
// 0.0 - x is exact), ops::finish and stock::removals_for pass the stock top.

// R2 numeric constant-scallop spacing for the pseudosphere funnel. Same
// meridian radius R_part(u) = c·sinh u as the outside band, but seen from
// the void side the meridian is *convex toward the tool* — so the target
// chord is the plain convex cusp triangle (CL circle radius R_part + r_ball),
// then <=20 Newton iterations on the exact CL chord length; the throat's
// honest conservatism note applies verbatim (current-point R evaluation on a
// convex triangle slightly overestimates a step's allowance — second-order,
// the paraboloid dome's established behavior). Marches in increasing u
// (mouth → tip), which — unlike the outside band and unlike the throat — is
// ALREADY the top-first emission order: a funnel deepens as it narrows, so
// no reversal at the call site. Near the mouth R_part can be small; the
// convex triangle handles r_part → 0 gracefully (the ogive apex precedent,
// near-180° steps).
//
// The TANGENT-LIP degenerate start (DECISIONS.md "Tangent-lip funnel…"): at
// mouth_dia == dia the march starts at the cusp itself, u = 0, where the
// meridian arc speed c·tanh u vanishes and the linearized initial guess
// u + d_req/(c·tanh u) divides by zero. The treatment is the arc-length
// inversion the waterline already uses closed-form — s(u) = c·ln(cosh u)
// from the cusp, so the u whose ON-SURFACE arc from the march point equals
// the target chord is acosh(cosh u · e^{d_req/c}), finite and positive for
// every u ≥ 0 — handed to the same Newton polish. Only the u == 0 start
// takes it: for u > 0 the linearized guess is kept verbatim, so existing
// truncated-mouth funnels converge from the same seed and their goldens are
// untouched (arc length ≥ chord, so the arc guess starts past the root and
// Newton walks back — same convergence, different seed only at the cusp).
//
// `skin` is the roughing nominal offset a' (0.0 for a plain finishing pass —
// byte-identical, x + 0.0 == x): the convex triangle prices the inflated
// surface's local radius R + a' and the CL rides at r_ball + a', the throat's
// substitutions verbatim.
//
// **This step is a SEED, not an emitted spacing** (2026-07-24, DECISIONS.md
// "Tangent-lip march fix…"). Nothing calls it directly any more: the
// current-point-R model is a one-point approximation that overshoots the true
// cusp — by ~2.6% on a truncated mouth and by ~12× at the tangent lip, where
// the corner form's chord goes scallop-insensitive — so both spacing modes go
// through derive_pseudosphere_funnel_capped_step_u, which starts from this
// chord and caps it by the pair's own exact tip price. What survives here is
// the cheap forward guess (and the cusp-start arc-length inversion above),
// which is exactly what a bisection wants for its upper bracket.

// Distance from a meridian-half-plane point to the NOMINAL funnel tractrix
// (cusp-plane-relative, the d_b = 0 frame pseudosphere_funnel_cl_point's
// chord solve already uses): the segment (c·sech u, −c·(u − tanh u)),
// u ∈ [u_b, u_t], clamped at the mouth and tip circles — the throat
// projection's corner-fan treatment on the other closed-form meridian. Same
// dense-grid seed + Newton polish, same one-sided honesty: a missed tighter
// minimum only overstates the leftover.

// The funnel scallop march's step, in BOTH modes: the chord seed above,
// CAPPED by the pair's own exact tip price. The cap exists because the
// current-point-R model FAILS at the tangent lip: the lip is an x^{3/2}
// cusp (R_part = c·sinh u → 0 but the meridian leaves horizontally, G1 with
// the stock top), so the corner-model step at R → 0 — a ~2·r_ball pivot
// chord, scallop-INSENSITIVE — assumes a surface that drops away like a
// cone corner while the tractrix hugs the horizontal: the first pair's true
// cusp prices ~2.5mm against a 0.2mm request. Since no scallop choice
// shrinks that chord, the roughing sweep-verify would reject EVERY
// tangent-lip allowance (the allowance-independent knife-edge the
// mixed-meridians slice named) and a plain finishing pass would silently
// ship a ridge 12× the promise. Both are dodged the way the cone dodged it:
// fix the grid, not the guarantee.
//
// Landed 2026-07-23 (DECISIONS.md "stock_to_leave inside funnels…")
// value-presence-gated, roughing only; the gate came OFF 2026-07-24
// (DECISIONS.md "Tangent-lip march fix…") once the nominal hole was the
// only thing it was protecting. One march home, one guarantee: the emitted
// spacing is priced by exact tips at every skin including 0, so no pass
// stack anywhere leans on the current-point-R model. The cost is the two
// scallop goldens the gate had been holding still (tangent-lip and
// truncated-mouth), churned on purpose.
//
// Bisection on the single pair's price (monotone-enough in the step; the
// price → 0 as the step closes, because two CL points a hair apart put
// their cusp tip a hair off the surface, so the walk always converges to a
// forward step), returning the safe side; a non-overlapping candidate
// prices INFINITY and shrinks like any other breach. The u_t clamp is
// priced too — hi_cand caps at the tip, so the final pass pair is capped
// like every other, not exempted by the clamp.

// Worst leftover-above-nominal of a tractrix-funnel pass stack given as its
// meridian parameter list, at inflation `skin` — the throat walk's sibling
// (see derive_catenoid_throat_march_cusp for the whole story: exact tips
// everywhere, the march demoted to spacing, the clamped one-sided
// projection). CL points live in the d_b = 0 frame the projection shares, so
// the tips and the nominal meridian never disagree about where the mouth is.

// Meridian CL point for the elliptic paraboloid bowl (row 3.9) in its op's own
// world frame: nominal surface z_v + h·rho²/a², offset along the outward normal
// whose slope is s = 2h·rho/a². `r_off` is the full normal-offset distance —
// `r_ball + a'` when the op asks where the tool centre goes, plain `a'` when
// `stock::removals_for` asks where the material stops (the offset meridian at
// the allowance IS the wall a roughing pass leaves standing, DECISIONS.md
// "Analytical op-history stock…"). It was the op's private closure until that
// second reader arrived; the arithmetic is verbatim, so goldens don't move.
//
// `paraboloid_cl_point` above is the same curve in the marchers' apex-relative,
// z-mirrored frame — a pre-existing split this move neither creates nor closes.
// Folding them would re-associate the sums and churn goldens for nothing.

// Meridian CL point for a spline surface of revolution, base-relative: the
// contact point C(u) offset one ball radius along the meridian's outward
// (up-facing) normal — the tangent rotated to (−z', r')/|C'|, flipped so
// nz >= 0 (the surface is machined from above). Returns (rho_cl, z_cl) in the
// meridian half-plane; callers add their own z reference. One home: the
// finishing pass emitter and the scallop march's Newton solve both read this,
// so the offset can't drift into two slightly-different versions.
//
// This is the CAP's home; the funnel's is spline_revolve_funnel_cl_point
// below. On a mouth-first cavity meridian (r' <= 0, z' <= 0) the nz >= 0
// flip here lands on the correct into-void normal wherever r' < 0 — but on
// a vertical wall stretch (r' == 0, legal in both envelopes) nz == 0 and
// the tie-break cannot tell the sides apart: it resolves outward, the cap's
// correct answer and a cavity gouge. So each orientation owns its rotation
// sense outright (the Catenoid/CatenoidThroat pattern) instead of sharing a
// flip that is right except on the walls.
pub(crate) fn spline_revolve_cl_point(
    meridian: &crate::nurbs::BSplineCurve,
    r_ball: f64,
    u: f64,
) -> (f64, f64) {
    let ((r, z), (dr, dz)) = meridian.eval_d1(u);
    let speed = (dr * dr + dz * dz).sqrt().max(f64::EPSILON);
    let (mut nr, mut nz) = (-dz / speed, dr / speed);
    if nz < 0.0 {
        nr = -nr;
        nz = -nz;
    }
    (r + r_ball * nr, z + r_ball * nz)
}

// The funnel's meridian CL point — the contact point offset one ball radius
// along the INTO-VOID normal, the tangent rotated the other way from the
// cap's: (z', −r')/|C'|. On the funnel envelope (r' <= 0, z' <= 0) this is
// up-and-inward with no flip needed — and it is exact on a vertical wall
// stretch (r' == 0), where it reads (−1, 0), straight toward the axis: the
// bore-wall answer the cap rule's tie-break would get wrong (see the comment
// above). Same (rho_cl, z_cl) contract, callers add their own z reference.
// One home for the funnel side: the pass emitter and the funnel march's
// Newton solve both read this.
pub(crate) fn spline_revolve_funnel_cl_point(
    meridian: &crate::nurbs::BSplineCurve,
    r_ball: f64,
    u: f64,
) -> (f64, f64) {
    let ((r, z), (dr, dz)) = meridian.eval_d1(u);
    let speed = (dr * dr + dz * dz).sqrt().max(f64::EPSILON);
    let (nr, nz) = (dz / speed, -dr / speed);
    (r + r_ball * nr, z + r_ball * nz)
}

/// A revolved CAVITY's meridian: its parameter range, mouth-first, and the
/// cutter-location point at a given ball radius (`DECISIONS.md` → "the
/// revolved cavities…", fork 3A).
///
/// **One home, and it is the third consumer that forced it.** The pairing of
/// "which parameter range" with "which `*_cl_point`" was already written twice
/// — once in `ops::finish`, once in `view.rs`'s wireframe arms — and the
/// clearance corridor would have been the third. `r_ball = 0` is the nominal
/// wall itself, which is exactly what the wireframe draws; a positive radius is
/// the ball centre's locus, which is exactly what the finishing pass rides.
/// The four variants here are the cavity-shaped revolves: the outside family
/// (domes, bosses, caps) is machined from without and has no corridor.
pub struct CavityMeridian<'a> {
    /// The parameter at the MOUTH — where the cavity meets the stock's top
    /// face, which is where all four of these variants are datumed.
    pub mouth: f64,
    /// The parameter at the deepest point.
    pub floor: f64,
    point: Box<dyn Fn(f64, f64) -> (f64, f64) + 'a>,
}

impl CavityMeridian<'_> {
    /// The meridian point at `t` for a ball of radius `r_ball`, as
    /// `(rho, z)` in absolute coordinates — `r_ball = 0` is the wall.
    pub fn at(&self, t: f64, r_ball: f64) -> (f64, f64) {
        (self.point)(t, r_ball)
    }

    /// `n` points from mouth to floor, both ends included. `n < 2` yields the
    /// two ends alone, because a meridian sampled at one point is not one.
    pub fn samples(&self, n: usize, r_ball: f64) -> Vec<(f64, f64)> {
        let n = n.max(2);
        (0..n)
            .map(|i| {
                let s = i as f64 / (n - 1) as f64;
                self.at(self.mouth + (self.floor - self.mouth) * s, r_ball)
            })
            .collect()
    }
}

/// The four cavity-shaped revolves, or `None` for everything else. Every
/// number comes from the variant's own homes — the same `derive_*` calls the
/// wireframe and the finishing pass make.
pub fn cavity_meridian(f: &Feature, stock_top: f64) -> Option<CavityMeridian<'_>> {
    match f {
        Feature::SplineRevolveFunnel { meridian, .. } => {
            let (u0, u1) = meridian.domain();
            // Only z DIFFERENCES carry meaning in an authored meridian; the
            // mouth maps to the stock top, exactly as the op places it.
            let z_shift = stock_top - meridian.eval(u0).1;
            Some(CavityMeridian {
                mouth: u0,
                floor: u1,
                point: Box::new(move |u, r| {
                    let (rho, z_rel) = spline_revolve_funnel_cl_point(meridian, r, u);
                    (rho, z_shift + z_rel)
                }),
            })
        }
        _ => None,
    }
}

// R2 numeric constant-scallop spacing for the spline surface of revolution:
// the family Newton march (derive_ellipsoid_step_t / derive_catenoid_step_w)
// generalized by POINTWISE curvature sign. A general spline meridian has no
// single convex-or-concave identity, so the cusp triangle is picked per march
// point from the exact signed curvature (nurbs::curvature_toward_up_normal,
// positive = concave toward the tool): convex → the plain triangle over
// R_part = 1/|κ| (CL circle R_part + r_ball), concave → the mirrored triangle
// (CL circle R_part − r_ball), and |κ| < 1e-6 /mm (R_part beyond a kilometre)
// → the cone's planar cusp chord 2·sqrt(2·r_ball·h − h²). The flat band is
// numerical, not cosmetic: for R_part that large the cusp triangle's
// cos(θ/2) rounds to 1.0 in f64 and θ collapses to 0 — and since both curved
// chords converge to the planar one as R_part → ∞, the band is continuous
// from either side, which also carries the march smoothly across an
// inflection (a curvature sign change — the vase's mixed-curvature situation
// without its closed-form arcs). Curvature is evaluated at the current march
// point like every sibling; the across-step drift is second-order (the
// catenoid-throat honesty note applies verbatim).
//
// Seeds from the meridian parameter speed like the family. A vanishing-speed
// start (a fully repeated pole) has no linearized guess and no closed-form
// arc length to invert (the tangent-lip funnel's escape), so it seeds from a
// deterministic per-knot-span grid walk instead: the first grid point whose
// CL chord reaches the target, handed to the same Newton polish. <=20 Newton
// iterations on the exact CL chord, the sibling loop shape byte for byte.
//
// `skin` is the roughing nominal offset a' (0.0 for a plain finishing pass —
// byte-identical output, x + 0.0 == x). The three substitutions stay
// distinct per the first slice's rule: the triangle prices the PHYSICAL
// ball on the inflated surface (local radius R + a' where the meridian is
// convex toward the tool, R − a' where concave — the paraboloid pair's
// signs, here applied per march point), the CL rides at r_ball + a', and
// the flat band's planar chord is skin-invariant (the corner-roll lemma's
// flat limit doesn't move with inflation). NOTE the across-step curvature
// drift makes this march an estimate on a user-authored meridian; the
// finisher verifies the emitted stack with derive_spline_march_cusp and the
// guarantee rests there, not here.
//
// **Both spline steps are SEEDS, not emitted spacings** (2026-07-24,
// DECISIONS.md "Family-wide march cap…"): nothing emits from them directly
// any more — derive_spline_revolve_capped_step_u and its funnel twin below
// start from this chord and cap it by the pair's own exact tip price, in
// both spacing modes. The estimate note above is exactly why.
pub fn derive_spline_revolve_step_u(
    scallop: f64,
    r_ball: f64,
    meridian: &crate::nurbs::BSplineCurve,
    u: f64,
    skin: f64,
) -> f64 {
    spline_scallop_step_u(scallop, r_ball, meridian, u, false, skin)
}

// The funnel's march — the same worker with the void-side pair of homes
// (spline_revolve_funnel_cl_point / curvature_toward_void_normal). The
// triangle logic, flat band, degenerate-start grid walk, and Newton loop are
// shared byte for byte; only which side of the meridian the CL and the
// curvature sign live on differs (see the two CL points' comments for why the
// sides can't share a flip).
pub fn derive_spline_revolve_funnel_step_u(
    scallop: f64,
    r_ball: f64,
    meridian: &crate::nurbs::BSplineCurve,
    u: f64,
    skin: f64,
) -> f64 {
    spline_scallop_step_u(scallop, r_ball, meridian, u, true, skin)
}

// The spline cap's step in BOTH modes, and its funnel twin below: the
// pointwise-curvature chord above as a SEED, capped by the pair's own exact
// tip price (DECISIONS.md "Family-wide march cap…" — the tangent-lip
// funnel's fix carried across the family). The spline is the carrier with
// the strongest claim on the cap: the mixed-meridians slice already REJECTED
// to let the osculating triangle be the roughing budget here, on the ground
// that across-step curvature drift on a user-authored meridian is unbounded
// in principle (measured +2.6% at a 0.2mm request on the fixture cap,
// +1.9% on the fixture funnel) — and if the drift is not trustworthy enough
// to price an allowance, it is not trustworthy enough to ship as a stated
// scallop either. Same argument, one slice later, applied to the promise
// instead of the ceiling.
//
// Mixed curvature costs the cap nothing: it only ever SHORTENS a step, so on
// the concave stretches — where the tight-end-first walk already makes each
// step conservative — it is inert, and it binds exactly where the triangle's
// one-point reading runs slack. Domain end taken from the curve itself
// (meridian.domain()), so the clamped final pair is priced like every other
// rather than exempted by the caller's forward-progress clamp.
pub fn derive_spline_revolve_capped_step_u(
    scallop: f64,
    r_ball: f64,
    meridian: &crate::nurbs::BSplineCurve,
    u: f64,
    skin: f64,
) -> f64 {
    spline_capped_step_u(scallop, r_ball, meridian, u, false, skin)
}

// The funnel's capped march — the same bisection over the void-side tip walk
// (derive_spline_funnel_march_cusp). Only which side the CL and the price
// live on differs, the plain marches' own split.
pub fn derive_spline_revolve_funnel_capped_step_u(
    scallop: f64,
    r_ball: f64,
    meridian: &crate::nurbs::BSplineCurve,
    u: f64,
    skin: f64,
) -> f64 {
    spline_capped_step_u(scallop, r_ball, meridian, u, true, skin)
}

// Bisection on the single pair's exact tip price, returning the safe side —
// the tangent-lip funnel's scheme (derive_pseudosphere_funnel_capped_step_u)
// with the two spline homes selected by `into_void`. The price falls
// continuously to zero as the step closes (two CL points a hair apart put
// their cusp tip a hair off the surface, and a sub-1e-9 chord is skipped
// outright), so the walk always converges to a forward step; a
// non-overlapping candidate prices INFINITY and shrinks like any other
// breach.
pub(super) fn spline_capped_step_u(
    scallop: f64,
    r_ball: f64,
    meridian: &crate::nurbs::BSplineCurve,
    u: f64,
    into_void: bool,
    skin: f64,
) -> f64 {
    let (_, u1) = meridian.domain();
    cap_step_by_tip_price(
        u,
        u1,
        spline_scallop_step_u(scallop, r_ball, meridian, u, into_void, skin),
        scallop,
        |un| {
            if into_void {
                derive_spline_funnel_march_cusp(meridian, r_ball, skin, &[u, un])
            } else {
                derive_spline_march_cusp(meridian, r_ball, skin, &[u, un])
            }
        },
    )
}

pub(super) fn spline_scallop_step_u(
    scallop: f64,
    r_ball: f64,
    meridian: &crate::nurbs::BSplineCurve,
    u: f64,
    into_void: bool,
    skin: f64,
) -> f64 {
    let cl_point = if into_void {
        spline_revolve_funnel_cl_point
    } else {
        spline_revolve_cl_point
    };
    let curvature = if into_void {
        crate::nurbs::BSplineCurve::curvature_toward_void_normal
    } else {
        crate::nurbs::BSplineCurve::curvature_toward_up_normal
    };
    let d_req = match curvature(meridian, u) {
        Some(kappa_tt) if kappa_tt.abs() >= 1e-6 => {
            let r_part = 1.0 / kappa_tt.abs();
            if kappa_tt > 0.0 {
                let theta = derive_scallop_angle_concave(scallop, r_ball, r_part - skin);
                2.0 * (r_part - r_ball - skin) * (theta / 2.0).psin()
            } else {
                let theta = derive_scallop_angle(scallop, r_ball, r_part + skin);
                2.0 * (r_part + r_ball + skin) * (theta / 2.0).psin()
            }
        }
        // Flat point (or a vanishing-speed one): the cone's planar cusp chord
        // (skin-invariant — a flat band's offset is the same flat band).
        _ => 2.0 * (2.0 * r_ball * scallop - scallop * scallop).sqrt(),
    };

    let cl_u = cl_point(meridian, r_ball + skin, u);
    let (_, (dr, dz)) = meridian.eval_d1(u);
    let speed = (dr * dr + dz * dz).sqrt();
    let (_, u1) = meridian.domain();

    let mut u_next = if speed > 1e-9 {
        u + d_req / speed
    } else {
        // Degenerate start: walk the per-span grid until the CL chord reaches
        // the target (deterministic, bounded by the domain), then Newton.
        const GRID_PER_SPAN: usize = 64;
        let mut seed = u1;
        'walk: for pair in meridian.distinct_knots().windows(2) {
            for k in 1..=GRID_PER_SPAN {
                let cand = pair[0] + (pair[1] - pair[0]) * (k as f64 / GRID_PER_SPAN as f64);
                if cand <= u {
                    continue;
                }
                let cl_c = cl_point(meridian, r_ball + skin, cand);
                let d = ((cl_c.0 - cl_u.0).powi(2) + (cl_c.1 - cl_u.1).powi(2)).sqrt();
                if d >= d_req {
                    seed = cand;
                    break 'walk;
                }
            }
        }
        seed
    };
    for _ in 0..20 {
        let cl_next = cl_point(meridian, r_ball + skin, u_next);
        let dr = cl_next.0 - cl_u.0;
        let dz = cl_next.1 - cl_u.1;
        let d = (dr * dr + dz * dz).sqrt();

        if (d - d_req).abs() < 1e-6 {
            break;
        }

        let cl_eps = cl_point(meridian, r_ball + skin, u_next + 1e-5);
        let dr_eps = cl_eps.0 - cl_u.0;
        let dz_eps = cl_eps.1 - cl_u.1;
        let d_eps = (dr_eps * dr_eps + dz_eps * dz_eps).sqrt();

        let deriv = (d_eps - d) / 1e-5;
        if deriv.abs() < 1e-9 { break; }

        u_next = u_next - (d - d_req) / deriv;
    }

    u_next
}

// Vase ogee half-geometry (CHECKLIST row 5.2): the symmetric two-arc S from
// the equator (radius a, vertical tangent) to the waist (radius c, vertical
// tangent) over height h is pinned entirely by the G1 junction conditions —
// radial span a − c = 2r·(1 − cos θ), height h = 2r·sin θ — whose ratio
// gives tan(θ/2) = (a − c)/h. One home each per the derive_*() discipline;
// well-defined for every legal vase (validate() enforces c < a and
// h >= a − c, so θ ∈ (0, π/2]).

// The asymmetric ogee's split radii (row 5.2's superset, DECISIONS.md
// "Asymmetric ogee…"): with the G1 junction authored at height `i` above the
// base plane, the turning angle θ is unchanged (it is pinned by a, c, h alone
// — both wall ends stay vertical, so both arcs turn through θ regardless of
// split) and the junction conditions give
//   r1 = i/sin θ  (belly),   r2 = (h − i)/sin θ  (neck)
// — the sum r1 + r2 = h/sin θ is split-invariant (twice the symmetric
// radius); the radial G1 condition a − c = (r1 + r2)(1 − cos θ) holds by θ's
// own definition, so the junction closes for every legal i. validate()
// enforces 0 < i < h (both radii positive) and i ≠ h/2 (the symmetric form —
// omit the field). One home per the derive_*() discipline; the symmetric
// form does NOT route through this (finish_vase keeps derive_vase_arc_radius
// verbatim, byte-identical).
