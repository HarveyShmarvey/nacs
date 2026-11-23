// The `Part` model: features, operations, the machine, stock and fixture, the header directives.

use super::*;

/// **Which leg of the chamfer the drawing dimensioned** — the drop down the
/// wall (`chamfer_depth`) or the run across the top face (`chamfer_width`).
/// Exactly one, never both, rejected at parse rather than checked for
/// consistency: the tool's angle already converts between them, and two
/// declarations of one quantity is a second place for it to be wrong
/// (DECISIONS.md → "the chamfer tool…"). The two legs are equal only at a
/// 90-degree included angle, and which one a print meant is a live shop
/// ambiguity that scraps parts — so it is named, not inferred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferLeg {
    Depth(f64),
    Width(f64),
}

/// A feature's declared chamfer: the one dimensioned leg, plus the face's
/// nominal INCLUDED angle when the drawing states one. An absent angle means
/// the tool's own angle defines the face (the common case — most chamfer
/// callouts dimension one leg and assume the tool); a present one must match
/// the tool's nominal angle exactly, because a form cutter cuts its own angle
/// or the wrong one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChamferSpec {
    pub leg: ChamferLeg,
    pub angle: Option<f64>,
}

/// A loop point's restatement of the chamfer's leg for the edges from it on
/// (ROADMAP item 1 slice 9): in force from point `at`'s outgoing edge until
/// a later point restates it, `None` where the point said `chamfer_depth=0`
/// and the edges are left unbroken. The feature's own leg holds from the
/// first point. The angle is the loop's: one tool cuts one angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChamferMark {
    pub at: usize,
    pub leg: Option<ChamferLeg>,
}

/// A hole's declared mouth cone — the same ground cone doing a different job
/// on a different feature (DECISIONS.md → "spot drilling and
/// countersinking…"). Two jobs, one geometry: a countersink (`csink_dia`) is
/// the conical seat a screw head sits in, dimensioned on the print as a top
/// diameter at an angle; a spot (`spot_dia`) is the positioning dimple a
/// drill follows. The job is carried by the VARIANT because the two pair
/// with different downstream checks (a seat must fit the following screw; a
/// dimple must stay narrower than the following drill's web — named linter
/// material, not built), so one field doing both would leave a future check
/// unable to tell which law applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CsinkSeat {
    Csink(f64),
    Spot(f64),
}

/// The mouth cone's declaration: the seat, plus the print's stated INCLUDED
/// angle when the drawing gives one — absent, the tool's own angle defines
/// the cone (the common callout); present, it must match the tool's nominal
/// angle exactly, the chamfer's pairing rule on the hole's axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CsinkSpec {
    pub seat: CsinkSeat,
    pub angle: Option<f64>,
}

impl CsinkSpec {
    /// The declared seat's top diameter, whichever job declared it.
    pub fn seat_dia(&self) -> f64 {
        match self.seat {
            CsinkSeat::Csink(d) | CsinkSeat::Spot(d) => d,
        }
    }

    /// The job's name, for headers and errors.
    pub fn job(&self) -> &'static str {
        match self.seat {
            CsinkSeat::Csink(_) => "csink",
            CsinkSeat::Spot(_) => "spot",
        }
    }
}

/// A hole's declared counterbore — the flat-floored cylindrical seat a
/// cap-screw head sinks into (DECISIONS.md → "the counterbore — a cylindrical
/// seat, and the tool's diameter becomes the pairing law"). Where the mouth
/// cone had one dimension and a derivable depth, the cylinder has two and
/// derives nothing: there is no angle to derive through, so `depth` is
/// DECLARED, and the structural trap moves from the depth math to the TOOL —
/// an `Endmill` plunging on the hole's axis leaves a seat exactly as wide as
/// itself, so `dia` must equal the tool's own diameter (the drill law, made a
/// declared pairing). Spotfacing is the same two fields at a smaller depth;
/// no third field pair exists.
///
/// The caveat the record cannot carry, named rather than guessed at: a
/// plunging endmill must be CENTER-CUTTING, and `Tool` has no bit that says
/// whether it is — a non-center-cutting endmill plunged on its axis rubs its
/// web exactly as a drill's chisel does. That stays the machinist's
/// discipline until the tool record grows the field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CboreSpec {
    /// The seat's diameter, off the print — and, by the pairing law, the
    /// tool's.
    pub dia: f64,
    /// The seat's depth below the part top, off the print. Declared, never
    /// derived: a cylinder has no apex to derive through.
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Feature {
    // `z` is the FINISHED face — the surface you want, not the one you started
    // from — so `Floor` simply cuts to it (DECISIONS.md "Vertical occupancy is
    // `extent`…"). A plane's own Z span is genuinely zero, which is why this is
    // the one feature with a datum and no `extent`.
    Plane { z: f64 },
    // The polyline prismatic pair (DECISIONS.md "Geometry-first fixture
    // naming…"): an authored arbitrary closed cross-section extruded down a
    // wall. Side-of-material is carried by the VARIANT, not by the data — the
    // row-2.4 law, finally applied to the polyline: `ProfilePocket` bounds a
    // void (the wall is cut from inside, the prismatic sibling of
    // `CylinderBore`/`ParaboloidBowl`), `ProfileBoss` bounds standing material
    // (cut from outside, the sibling of `CylinderBoss`). The winding is now
    // REDUNDANT data validated against the variant — CCW for a pocket, CW for
    // a boss, the blessed comp convention — and a mismatch is rejected pointing
    // at the other variant rather than silently reversed, because reversing
    // the traversal flips climb/conventional on the machine.
    //
    // "Profile" is deliberate shock-absorber vocabulary: not *Prism* (a prism
    // is strictly polygonal — the first filleted corner would falsify the
    // name) and not *Extrude* (a general extrude would subsume `CylinderBore`
    // and re-create two encodings of one shape). The word `Contour` names the
    // 3D-wire pair below, the feature it was reserved for — which is also why
    // `extent == 0` stays rejected HERE in `validate()`: a planar polyline
    // spans no Z of its own, so `extent` must be positive to leave any wall
    // at all, while the wire carries its own Z range and takes zero (total Z
    // span = the geometry's own Z span + extent, DECISIONS.md "Vertical
    // occupancy is `extent`…").
    //
    // `z` is the top datum (None = the stock top, matching `CylinderBore`'s
    // authored-top-down convention); `extent` is the feature's own vertical
    // occupancy, so the wall lives in [z - extent, z]. Downward is not an
    // orientation *choice* — a 3-axis prismatic wall is cut from its top.
    //
    // `edge_round` is the same optional field the circular pair carries — the
    // round on this feature's top edge, cut by a `CornerRounder` (DECISIONS.md
    // → "the corner rounder gets an operation", slice 2). The field is the
    // declaration; nothing about the polyline changes because of it.
    /// The bounded planar face (Phase 28 slice 1; `DECISIONS.md` → "Phase 28
    /// opens: the patterns DRAFT…", fork 1A): a height and a loop that can
    /// contain loops — `loops[0]` the outer boundary, wound CCW, the rest
    /// inner boundaries, wound CW, each strictly inside the outer and clear of
    /// the others. It is what STEP calls a face (a plane, one outer bound, any
    /// number of inner bounds) and what the offsetter calls a shape. The loops
    /// say WHERE the face is; what each stretch of boundary means to a tool is
    /// derived (`region::of`). `Plane` is its unbounded sibling.
    Face { z: f64, loops: Vec<Vec<Pt>> },
    ProfilePocket { points: Vec<Pt>, z: Option<f64>, extent: f64, edge_round: Option<f64>, chamfer: Option<ChamferSpec>, chamfer_marks: Vec<ChamferMark> },
    ProfileBoss { points: Vec<Pt>, z: Option<f64>, extent: f64, edge_round: Option<f64>, chamfer: Option<ChamferSpec>, chamfer_marks: Vec<ChamferMark> },
    // ---- The 3D-wire pair: the reserved word, landed (DECISIONS.md "the
    // 3D-wire Contour — the reserved word meets the row-2.4 law…", blessed
    // 2026-07-29). A closed wire whose points rise and fall in Z, sweeping a
    // non-planar prismatic WALL: at each point the wall spans
    // [z - extent, z], so `total Z span = the wire's own Z span + extent` —
    // the committed `extent` sum with both halves nonzero for the first time,
    // and the one place `extent == 0` is legal (no extrusion: the wall is the
    // wire itself, a trim edge).
    //
    // Side-of-material is carried by the VARIANT (the row-2.4 law), judged on
    // the XY projection: `ContourPocket` bounds a void (CCW), `ContourBoss`
    // standing material (CW), mismatches rejected pointing at the other
    // variant. There is no `z` datum field — the wire carries its own Z per
    // point, absolute, which is the entire reason the word was reserved.
    // Open wires are deliberately not this feature: they have no winding for
    // the law to validate, and arrive (if ever) as their own variant.
    ContourPocket { points: Vec<Pt3>, extent: f64 },
    ContourBoss { points: Vec<Pt3>, extent: f64 },
    // ---- The EXTRUSION features, and the rule that scopes the word
    // (DECISIONS.md "The `extent` vocabulary is scoped to the extrusion
    // features…"). `extent` names vertical occupancy on exactly those features
    // whose number is *pure occupancy* — sweep a planar cross-section up or
    // down and the shape is unchanged, only how much of it there is. The
    // circular pair (`CylinderBore`/`CylinderBoss`), the elliptic pair
    // (`EllipticBore`/`EllipticBoss`) and the polyline pair above are that set,
    // and they all read the committed semantic the same way: a planar section
    // spans no Z of its own, so total Z span = 0 + extent.
    //
    // What the word deliberately does NOT reach: the *shape parameters* below
    // — `Cone { height }`, `Ellipsoid { height }`, `Paraboloid { height }`,
    // `ParaboloidBowl { depth }`, `Ogive { height }`, `Vase { height }`. Those
    // numbers change the surface (a cone's flank angle, a spheroid's polar
    // semi-axis, an ogive's derived R_o), so they ARE the geometry's own Z
    // span rather than an extrusion added to it; spelling them `extent` would
    // assert their geometry spans zero. Orientation living in `height`/`depth`
    // is the accepted cost of that line — the alternative was a word that
    // contradicts its own definition on six variants.
    //
    // Direction still comes from the variant, never the field: a bore's wall
    // occupies [z_top - extent, z_top] from the stock top, a boss's rises to
    // `z + extent` from its base plane.
    //
    // `csink` is the hole's declared mouth cone (DECISIONS.md → "spot drilling
    // and countersinking…"): a countersunk seat or a spot dimple, cut by a
    // `Drill` run with a ChamferMill — the only feature that carries it,
    // because Drill is the only verb that visits it. `cbore` is its
    // cylindrical sibling (DECISIONS.md → "the counterbore…"): a flat-floored
    // seat cut by a `Drill` run with an `Endmill`, whose diameter the seat
    // must equal exactly. One mouth carries one declared treatment — the
    // parse-side guards reject any two of {csink, cbore, chamfer, edge_round}
    // together.
    //
    // `z` is the RIM — the top datum the bore is authored down from (Phase
    // 45 slice 1; DECISIONS.md → "DRAFT: Phase 45 opens…", fork 1A): `None`
    // is the stock top, exactly as `ProfilePocket`'s, and every reader goes
    // through the same one home, `derive_contour_top`. The wall lives in
    // [z − extent, z]; the mouth treatments (csink, cbore, round, chamfer)
    // sit at the rim and move with it. What stands ABOVE a sunken rim is
    // another feature's material: the entry restriction refuses a plunge
    // through it by name until an earlier op has removed it.
    CylinderBore { dia: f64, extent: f64, x: f64, y: f64, z: Option<f64>, edge_round: Option<f64>, chamfer: Option<ChamferSpec>, csink: Option<CsinkSpec>, cbore: Option<CboreSpec> },
    // ---- The intruder with an axis (Phase 41 slice 5, DECISIONS.md → "Phase
    // 41 slice 5…"): a cylindrical bore whose axis is NOT +Z. It is declared
    // so a boundary can be built against it, a region can go around it or
    // read it as a void, and the picture can show it — and for nothing else:
    // no op of this program cuts a tilted bore (this program never tilts a
    // tool), so every verb on it refuses by name at `validate`. It is cut on
    // another setup, or arrives from CAD.
    //
    // `x, y` is where the axis crosses the stock top (the tilted bore has no
    // `z` yet: it opens at the top face — Phase 45 gave the vertical bore a
    // rim, DECISIONS.md → "DRAFT: Phase 45 opens…" fork 3A, and this one
    // follows as an additive arm when a part asks); `tilt` is the axis's
    // angle from vertical in degrees, strictly inside (0, 90) — 0 IS a
    // `CylinderBore`, refused pointing at it; `heading` is the plan direction
    // the axis leans toward going DOWN, degrees CCW from +X, like every angle
    // the vocabulary carries (never unit-scaled). `extent` is vertical
    // occupancy, as it is everywhere ("Vertical occupancy is `extent`…"): the
    // bore is the cylinder of radius `dia/2` about the axis, clipped to the
    // slab between the stock top and `extent` below it — so both its ends are
    // horizontal sections, which is what makes its plan envelope two lines
    // and two half-ellipses (`derive_tilted_bore_envelope`).
    TiltedBore { dia: f64, extent: f64, x: f64, y: f64, tilt: f64, heading: f64 },
    // ---- Camp 2: closed-form quadric surfaces (DECISIONS.md "Feature geometry
    // is three camps"). These carry the *shape*; the toolpath that consumes them
    // stays pure arithmetic (the "it's a wall now" bet). Only `Sphere` has a
    // consumer today (`ops::finish`); `Cone`/`Torus` are vocabulary landed with
    // it and are inert until their finishing math arrives (ROADMAP Phase 3) —
    // any op that references one gets a clear "not yet" error from `validate`.
    //
    // `z` on all three is the surface's reference plane. For `Sphere` it is the
    // equator plane (the sphere centre height): the machinable upper hemisphere
    // — the "dome" — rises from `z` to `z + dia/2`. Below the equator is an
    // undercut, off-limits to the 3-axis path.
    Sphere { dia: f64, x: f64, y: f64, z: f64 },
    Cone { dia: f64, height: f64, x: f64, y: f64, z: f64 },
    Torus { major_dia: f64, minor_dia: f64, x: f64, y: f64, z: f64 },
    // ---- Camp 2b: R2 analytic surface (ROADMAP Phase 3.5; DECISIONS.md
    // "Implementation regimes"). An ellipsoid of revolution (spheroid) dome:
    // equatorial circle of radius `dia/2` in the `z` plane, polar semi-axis
    // `height` rising to the pole at `z + height`. Unlike the five R1 quadrics
    // above, a spheroid has **no elementary offset surface**, so its ball-nose
    // tool-centre path is computed *pointwise* from the closed-form normal
    // (`ops::finish::finish_ellipsoid`) rather than by offset-closure algebra.
    //
    // Deliberately an ellipsoid of *revolution*, not a full tri-axial one: the
    // rotational symmetry keeps every waterline pass a circle (so it reuses the
    // R1 ring machinery). A tri-axial dome (distinct x/y radii) turns each pass
    // into an ellipse and is deferred (DECISIONS.md "R2 first primitive…").
    // When `height == dia/2` this is exactly a `Sphere` — the correctness anchor
    // the finishing math is checked against.
    // ---- Camp 2b: R2 elliptic paraboloid dome (CHECKLIST row 3.10). A
    // paraboloid of *revolution* — same narrowing as `Ellipsoid` above, and for
    // the same reason: rotational symmetry keeps every waterline pass a circle.
    // Base circle of radius `dia/2` in the `z` plane, apex at `z + height`;
    // surface z(rho) = z + height·(1 − rho²/(dia/2)²). Authored base-up like
    // `Cone`/`CylinderBoss`/`Ellipsoid` (the outside-feature convention). Like the
    // spheroid it has no elementary offset, so the CL path is pointwise from
    // the closed-form normal (`ops::finish::finish_paraboloid`). Unlike the
    // spheroid it has no sphere limit — its correctness anchor is the direct
    // one-ball-radius-off-surface residual test, not a reduces-to-sphere test.
    // ---- Camp 2b: R2 elliptic paraboloid bowl (CHECKLIST row 3.9, the ⚠ row —
    // the first *concave* R2 surface). Same paraboloid of revolution as above,
    // opening upward: a top-open cavity authored top-down from the stock top by
    // `depth`, as the `CylinderBore` bore was until Phase 45 (no `z` field —
    // the mouth IS the stock top, so "top-open" holds by construction; the
    // bore now carries an optional rim, DECISIONS.md → "DRAFT: Phase 45
    // opens…" fork 3A, and this one follows as an additive arm when a part
    // asks). Mouth circle of radius `dia/2` at
    // stock.z, vertex at stock.z − depth; surface
    // z(rho) = z_vertex + depth·rho²/(dia/2)². Side-of-material is carried by
    // the variant, not a field (the row-2.4 rule): a bowl is a void, so its
    // finishing pass is concave and runs the shared gouge guard
    // (ops::finish::concave_gouge_check) against the tightest concave radius
    // of curvature, (dia/2)²/(2·depth) at the vertex umbilic.
    // ---- Camp 2b: tangent-ogive nose (CHECKLIST row 5.1), the first "analytic
    // surface of revolution" row (TAXONOMY §5). A nose cone whose meridian is a
    // circular arc of radius R_o = (a² + h²)/(2a) (a = dia/2, h = height;
    // derive_ogive_radius is the one home) centred IN the base plane at radial
    // offset a − R_o ≤ 0 — "tangent" because the arc meets the base plane with
    // a vertical tangent, blending smoothly into the wall below. Base circle of
    // radius `dia/2` in the `z` plane, sharp apex on the axis at `z + height`;
    // authored base-up like `Cone`/`CylinderBoss`/`Paraboloid` (the outside-feature
    // convention). Classed R2 by the taxonomy's revolved family, but the
    // circular meridian makes this member close in R1 form: the ball offset of
    // a circular arc is the concentric arc R_o + r_ball (a spindle-torus patch
    // is self-offsetting, like the torus), so both spacing modes are
    // closed-form — no Newton march (DECISIONS.md "Tangent ogive nose…").
    // Requires height ≥ dia/2: any shallower and the arc crests before
    // reaching the axis, so the nose would overhang — an undercut, made
    // unrepresentable rather than checked. At height == dia/2 exactly the
    // ogive IS a hemisphere — the reduces-to-sphere correctness anchor.
    //
    // `profile_radius`, when present, is the SECANT superset (DECISIONS.md
    // "Secant ogive…"): the meridian radius authored free instead of derived,
    // required strictly greater than the tangent radius R_o. The arc centre
    // then drops below the base plane (derive_secant_ogive_centre), the wall
    // meets the base at an angle instead of vertically, and the CL locus
    // gains a third piece — a ball roll around the base-rim corner, the apex
    // cap's move mirrored at the bottom. Radii below R_o crest before the
    // apex (an overhang — undercut, rejected); equal to R_o it IS the tangent
    // form (rejected pointing at omitting the field, the degenerate-pointer
    // move). Omitted, the tangent form derives R_o — one home, no free
    // parameter to mis-set.
    // ---- Camp 2b: catenoid neck (CHECKLIST row 5.3), the first *genuinely* R2
    // revolved surface — the cosh meridian is not an arc, so unlike the ogive
    // nothing closes in R1 form and the constant-scallop spacing is the Newton
    // march the ogive entry warned rows 5.2/5.3 would need (DECISIONS.md
    // "Tangent ogive nose…"). A soap-film neck: base circle of radius `dia/2`
    // in the `z` plane, narrowing along rho(z) = c·cosh((z_w − z)/c) to its
    // waist circle of radius c = `neck_dia/2` at the top, z_w = z + height.
    //
    // The *height is derived, not authored*: pinning the waist AT the top
    // makes h = c·acosh(a/c) (derive_catenoid_height, the one home). Two
    // reasons, both load-bearing: (1) everything above the waist widens again
    // — a 3-axis undercut — so stopping the surface exactly at the waist is
    // the non-undercut constructor rule (dome-from-equator, base-down cone);
    // (2) authoring dia+height instead would be the classic two-catenaries
    // soap-film problem (zero, one, or two waist solutions for the same rim
    // and height) — a free parameter to mis-set, rejected like the secant
    // ogive's free radius. Requires neck_dia strictly less than dia: at
    // equality the catenoid degenerates to a zero-height cylinder band (a
    // wall — declare a CylinderBoss).
    //
    // The first *anticlastic* surface in the vocabulary (a minimal surface:
    // the principal curvatures cancel everywhere): the hoop direction is
    // convex but the meridian is concave toward the tool, tightest radius
    // c·cosh²w = c at the waist, so finishing runs the shared gouge guard
    // (ops::finish::concave_gouge_check) against neck_dia/2.
    //
    // `top_dia`, when present, is the TRUNCATED superset (DECISIONS.md
    // "Truncated catenoid neck…"): the band stops at that circle short of
    // the waist, strictly between neck_dia and dia. `neck_dia` stays
    // authored as the surface's identity even though its circle is no longer
    // cut — it pins the catenary the band lives on (the pseudosphere's
    // dia-pins-the-tractrix move). The height is still derived (the same
    // meridian, stopped early: h = c·[acosh(a/c) − acosh(t/c)]), and the
    // gouge bound moves with the truncation — tightest meridian radius
    // c·cosh²(w_top) = (top_dia/2)²/c at the top circle, looser than the
    // full neck's c. top_dia == neck_dia is rejected pointing at omitting the
    // field (that IS the full neck — the degenerate-pointer move); below the
    // waist no such circle exists on the surface; at dia the band is zero
    // height. Omitted, the full neck-to-waist form is unchanged.
    // ---- Camp 2b: catenoid throat (CHECKLIST row 5.3, inside orientation) —
    // the same cosh meridian machined from the *void* side: the converging
    // half of a venturi. A top-open cavity, authored top-down with no `z`
    // field (the `ParaboloidBowl` precedent; the `CylinderBore` gained a rim
    // in Phase 45, fork 3A — this one follows when a part asks): rim circle of
    // radius `dia/2` in the stock-top plane, narrowing along
    // rho(z) = c·cosh((z − z_w)/c) to the waist (throat) circle of radius
    // c = `neck_dia/2` at the BOTTOM, z_w = stock top − depth.
    //
    // The *depth is derived, not authored* — the outside neck's waist-at-top
    // rule, mirrored: everything below a catenoid's waist widens again, which
    // for a cavity machined from above is an undercut, so the surface ends
    // exactly at the waist and h = c·acosh(a/c) — literally the same
    // derive_catenoid_height, one home serving both orientations. (The
    // venturi's diverging half is the same feature machined from the other
    // side after a flip — a second setup, not a new constructible.) Requires
    // neck_dia strictly less than dia: at equality the throat degenerates to
    // a straight cylinder bore wall — declare a CylinderBore.
    //
    // Side-of-material is carried by the variant, not a field an op could
    // misread (the CylinderBore/CylinderBoss and Paraboloid/ParaboloidBowl precedent).
    // Finishing runs the shared gouge guard against the *hoop* normal-section
    // radius c·cosh²w — tightest c at the waist, the SAME bound as the
    // outside neck's meridian: a minimal surface's two principal radii agree
    // in magnitude everywhere, so its guard bound is orientation-independent
    // (see ops::finish::finish_catenoid_throat).
    // ---- Camp 2b: pseudosphere band (CHECKLIST row 5.3, second member), the
    // tractrix meridian revolved — THE constant-negative-curvature surface
    // (K = −1/c² everywhere, c = dia/2) and the second anticlastic member
    // after the catenoid. Meridian, parametrized by u ≥ 0 from the cusp rim:
    // rho(u) = c·sech u, z(u) = c·(u − tanh u) above the cusp plane. The
    // identity sech²u + tanh²u = 1 is this row's analogue of the catenoid's
    // cosh identity: the unit tangent is (−sech u, tanh u) with no
    // normalization factor, so the CL offset stays closed-form per point.
    //
    // The first *forced-band* constructible: three authored diameters, because
    // — uniquely so far — BOTH canonical ends of the meridian are unusable.
    // The cusp rim (u = 0, diameter `dia`) is where the flare meets its base
    // plane tangentially, and the concave meridian radius of curvature
    // R_m(u) = c·sinh u vanishes there — the first surface point NO ball tool
    // of any radius can machine, so truncation above it is forced, not chosen
    // (validate() rejects base_dia == dia naming the cusp). The other end
    // narrows asymptotically toward the axis — the tip is at infinity, so the
    // top truncation is forced too (tip_dia > 0, strictly). `dia` itself (the
    // bell-mouth/cusp circle, 2c) stays authored because it is the surface's
    // identity — it pins the tractrix the band lives on; the band is
    // base_dia → tip_dia within it, base circle in the `z` plane, heights
    // derived via u(rho) = acosh(c/rho) (derive_pseudosphere_u /
    // derive_pseudosphere_height, the one home each).
    //
    // Non-undercut by construction: rho strictly shrinks going up for every
    // u > 0, so no band can express an overhang (the re-entrancy gate stays
    // vacuous). Finishing runs the shared gouge guard against the band's
    // tightest concave meridian radius, c·sinh(u_base) at the base circle.
    // ---- Camp 2b: pseudosphere funnel (CHECKLIST row 5.3, inside
    // orientation of the tractricoid) — the tractrix meridian machined from
    // the void side: a top-open cavity whose mouth circle (`mouth_dia`, in
    // the stock-top plane) narrows asymptotically toward the axis down to the
    // tip circle (`tip_dia`) at the derived depth
    // (derive_pseudosphere_height, the same one home as the outside band —
    // same meridian, same u(rho) = acosh(c/rho), c = dia/2). Authored
    // top-down with no `z` field, the bore precedent; field names follow the
    // orientation like ParaboloidBowl's depth (the outside band's `base_dia`
    // is a bottom circle; the funnel's wide circle is its top opening, so it
    // is `mouth_dia`).
    //
    // Still a band, but the forced-band reasoning is *asymmetric* in
    // orientation, said explicitly: the tip truncation stays forced (the tip
    // is at infinity, and from inside the concave hoop radius c/sinh u
    // vanishes toward it), but the cusp circle is only unmachinable from
    // OUTSIDE — from the void side the meridian is convex toward the tool,
    // so the full TANGENT-LIP form (mouth_dia == dia, the cavity blending
    // tangentially into the stock top face) is geometrically machinable and
    // is now supported (DECISIONS.md "Tangent-lip funnel…" — the deferral
    // recorded on landing the funnel, lifted). The one degeneracy is the
    // scallop march's start: the meridian parameter speed c·tanh u vanishes
    // at the cusp, so the first step's initial guess switches from the
    // arc-speed linearization to the closed-form arc-length inversion
    // (derive_pseudosphere_funnel_step_u); the waterline's closed-form
    // inversion needed nothing at all. Only mouth_dia > dia stays rejected —
    // the tractrix ends at the cusp circle, no surface exists beyond it.
    //
    // Non-undercut by construction: the cavity narrows strictly going down.
    // Finishing runs the shared gouge guard against the tightest concave
    // hoop radius, c/sinh(u_tip) at the tip circle — NOT the outside band's
    // bound or end: the pseudosphere is not a minimal surface, so the
    // catenoid's orientation-independent guard was that surface's privilege
    // (see ops::finish::finish_pseudosphere_funnel).
    // ---- Camp 2b: vase (CHECKLIST row 5.2), the last R2 row — and the one
    // whose meridian had to be *chosen*, not inherited: "vase / goblet" names
    // a shape family with no canonical equation, so the constructible is the
    // symmetric two-arc ogee settled in DECISIONS.md ("Vase (row 5.2)…",
    // Option B): a convex belly arc from the equator circle (radius
    // a = dia/2, vertical tangent, in the `z` plane) blending G1 at the
    // inflection into a concave neck arc ending at the waist circle (radius
    // c = neck_dia/2, vertical tangent) at z + height. Three authored fields
    // pin everything in closed form: half-angle tan(θ/2) = (a − c)/h, both
    // arc radii r = h/(2·sin θ) (derive_vase_arc_angle /
    // derive_vase_arc_radius, one home each); the symmetric split r1 = r2 is
    // the canonical choice — the inflection lands at the exact midpoint of
    // the wall — and an asymmetric split or authored inflection height is a
    // strict superset, deferred (the secant-ogive move).
    //
    // The machinable 3-axis scope is the equator-to-waist wall only: the
    // under-belly and any flaring lip are undercut and unrepresentable (the
    // dome-from-equator rule). Validation rejects h < a − c — past that the
    // belly arc would crest beyond vertical, an overhang — and neck_dia ==
    // dia (a straight wall: declare a CylinderBoss). Both arcs are *circles*, so the
    // whole surface closes in R1 form (concentric CL arcs, per-arc
    // closed-form scallop): the first mixed-curvature meridian — synclastic
    // belly, anticlastic neck through an inflection — with the gouge guard
    // applying to the neck arc only (r_ball ≤ r).
    //
    // `inflection`, when present, is the ASYMMETRIC superset (DECISIONS.md
    // "Asymmetric ogee…"): the G1 junction's height above the `z` plane,
    // authored free — the split's one degree of freedom. The turning angle θ
    // is pinned by (a, c, h) regardless of split, and the G1 conditions
    // become r1 = i/sin θ (belly), r2 = (h − i)/sin θ (neck)
    // (derive_vase_split_radii): the SUM r1 + r2 = h/sin θ is invariant,
    // the split slides the inflection along the wall. Required strictly
    // inside (0, height); == height/2 IS the symmetric ogee (rejected
    // pointing at omitting the field, the degenerate-pointer move). The
    // gouge bound follows the neck arc alone, so it MOVES with the split —
    // r_ball ≤ r2, looser for a low inflection, tighter for a high one.
    // ---- The circular outside boss (CHECKLIST row 2.4; DECISIONS.md "Row-2.4
    // `side` selector"). `CylinderBore` above is a *void* (a bore, authored top-down
    // by `extent`, tool inside); `CylinderBoss` is *material* — a standing cylinder whose
    // wall is finished from the outside, tool-centre at R + r_tool. One taxonomy
    // row = one constructible: side-of-material is carried by the variant, not a
    // field an op could misread, so Drill-on-CylinderBoss is a type error and the
    // convex/no-gouge-guard property holds by construction. Authored base-up
    // like `Cone`: `z` is the base plane, the wall rises to `z + extent` (the
    // boss top, typically the stock top it was cut down from). `Cone`'s
    // neighbouring `height` is NOT the same kind of number — see the extrusion
    // rule at `CylinderBore` above.
    CylinderBoss { dia: f64, extent: f64, x: f64, y: f64, z: f64, edge_round: Option<f64>, chamfer: Option<ChamferSpec> },
    // ---- Row 2.5's elliptic member (CHECKLIST "Conic-section cylinder"): the
    // elliptical bore — `CylinderBore`'s ellipse sibling, a void whose vertical
    // wall's cross-section is an axis-aligned ellipse (major axis along X,
    // semi-axes major_dia/2 ≥ minor_dia/2; a rotation field is a deferred
    // superset). Same bore semantics: authored top-down from the stock top by
    // `extent` (no `z` yet — the circular bore's rim is Phase 45's, fork 3A;
    // this one follows as an additive arm when a part asks), tool inside,
    // straight-flank cut with an endmill (`stepover` is
    // the axial Z stepdown; the wall is developable, so the flank contact is
    // exact). The ellipse is the row's *minimal honest constructible*: it is
    // the one conic whose profile is a closed curve — the parabolic and
    // hyperbolic members name open profiles that cannot stand alone as a wall
    // and stay named-open in the row. An ellipse swept down is pure occupancy,
    // so the vertical field is `extent` (the rule at `CylinderBore` above).
    //
    // What makes this R2 where the circular bore was R1: the inward parallel
    // of an ellipse is NOT an ellipse (no elementary offset), so the
    // tool-centre ring is built pointwise from the closed-form normal — the
    // normal-angle parametrization t = atan2(b·sin φ, a·cos φ) keeps every
    // point exact (ops::finish::ellipse_offset_pts). Requires major_dia
    // strictly greater than minor_dia: at equality the ellipse IS a circle —
    // declare a CylinderBore (whose R1 closed form this op must and does reduce
    // to, the test anchor). Finishing runs the shared gouge guard against the
    // wall's tightest concave radius of curvature, (minor/2)²/(major/2) at
    // the major-axis vertices — the evolute bound past which the inward
    // parallel self-intersects.
    //
    // `rotation`, when present, is the ROTATED superset (DECISIONS.md
    // "Rotated ellipse…"): the major axis turned `rotation` DEGREES
    // counterclockwise from +X about the feature centre. Angles are the
    // vocabulary's first non-length field — never unit-scaled. Required
    // strictly inside (0, 180): the ellipse's 180° symmetry makes that range
    // name every distinct orientation exactly once (0 IS the axis-aligned
    // form — rejected pointing at omitting the field, the degenerate-pointer
    // move; major > minor already guarantees the axes can't be authored
    // swapped). Rotation is a rigid motion, so every derived quantity the
    // ops use — the evolute gouge bound, the ring's chord flattening, both
    // windings — is unchanged; only the emitted coordinates turn.
    // ---- Row 2.5's elliptic member, outside: `CylinderBoss`'s ellipse sibling —
    // standing material whose wall is finished from the outside, the same
    // pointwise parallel with the offset sign flipped (tool-centre outside).
    // Convex everywhere, so no gouge guard (the absence is a stated fact, per
    // the guard roster in ops::finish). Same field rules as EllipticBore
    // (axis-aligned, major > minor strictly — at equality declare a CylinderBoss;
    // optional `rotation` in degrees CCW, strictly inside (0, 180), per the
    // bore's rotated-superset note); authored bottom-up like CylinderBoss (`z` is
    // the base plane, wall rises `extent` above it).
    // ---- Camp 3: full-3D tessellated mesh (ROADMAP Phase 4; DECISIONS.md
    // "Camp-3 slice 1: raster drop-cutter (z-map) mesh finishing"). The one
    // feature with no equation — a triangle soup read from an STL file at
    // parse time (mirroring how a tool's SVG profile is baked in). Machining
    // it accepts tessellation + sampling error, which no analytical camp does;
    // that acceptance is the variant's charter, stated where it's declared.
    //
    // STL vertex data is defined to be millimetres (STL is unitless; mm is
    // the convention) — under `units inch` the placement offsets `x`/`y`/`z`
    // scale at the parse boundary like every other length, the triangles
    // never do. `file` is kept for error messages only; the geometry the ops
    // consume is `triangles`, already loaded and validated non-empty.
    Mesh { triangles: Vec<crate::mesh::Tri>, x: f64, y: f64, z: f64, file: String },
    // ---- The analytic NURBS reader (ROADMAP Phase 4 follow-up; DECISIONS.md
    // "Analytic spline-of-revolution reader…"). A surface of revolution whose
    // meridian is an arbitrary (rational) b-spline curve — the freeform
    // revolved face `import_step_mesh.py` would tessellate, machined here with
    // NO stair-step error instead. It is the R2 revolved family (Catenoid /
    // Pseudosphere / Vase) with the named analytic meridian generalized to a
    // spline: contact is the pointwise ball offset along the curve's exact
    // normal, curvature comes from the exact second derivative, so the
    // finishing path is analytic, not sampled — the opposite trade to Mesh.
    //
    // `meridian` is a curve in the (r, z) half-plane, revolved about the
    // vertical axis through (x, y); the feature's `z` is the base offset added
    // to the meridian's z (world_z = z + meridian_z, world radius = meridian_r).
    // Like STL vertices (see the Mesh variant), the meridian's control-point
    // coordinates are defined to be millimetres — under `units inch` the
    // placement offsets x/y/z scale at the parse boundary, the meridian never
    // does. `file` is kept for error messages only; the geometry the ops
    // consume is `meridian`, already loaded and validated a machinable cap
    // (apex-first, no undercut) at parse time. Baked in from a `.spline` file
    // at parse time, exactly as a Mesh's STL or a FormTool's SVG profile is.
    SplineRevolve { meridian: crate::nurbs::BSplineCurve, x: f64, y: f64, z: f64, file: String },
    // The spline surface of revolution's INSIDE orientation (CHECKLIST row 5.3's
    // last member; DECISIONS.md "Spline funnel…") — a revolved-spline cavity,
    // machined from the void side, completing the both-orientations pattern the
    // catenoid (Catenoid/CatenoidThroat) and pseudosphere
    // (Pseudosphere/PseudosphereFunnel) set. A separate variant, not a `side`
    // field: side-of-material is carried by the type, so a cap op on a cavity
    // meridian is a type error, not a runtime surprise.
    //
    // No `z` field — the throat/bore precedent: a cavity is authored top-down,
    // its mouth circle in the stock-top plane, and its depth read off the
    // meridian's own z extent (world_z = stock.z + (meridian_z − z_mouth); only
    // z *differences* in the `.spline` matter). The meridian is stored
    // MOUTH-FIRST (mouth rim widest, at the top; z non-increasing, r
    // non-increasing — the mirror of the cap's apex-first envelope, validated
    // by nurbs::check_machinable_funnel at the same parse boundary). Control
    // points are mm by definition like the cap's; only x/y scale with project
    // units. `file` kept for error messages only, geometry baked in at parse.
    SplineRevolveFunnel { meridian: crate::nurbs::BSplineCurve, x: f64, y: f64, file: String },
    // ---- The general NURBS patch (DECISIONS.md "the general NURBS patch…",
    // slice 1): a tensor-product (rational) b-spline surface, camp 3's
    // ANALYTIC input — the exact surface under the mesh raster contract.
    // Where `Mesh` tessellates a freeform face and accepts the chord error,
    // this reads the patch itself; the raster drop-cutter's sampling error
    // remains (camp 3's charter), the chord term is gone. NOT an R2 family
    // member: a general patch has no meridian and no waterline circle, so it
    // machines through ops::finish::finish_nurbs_patch (the drop-cutter),
    // never the pointwise-offset marches.
    //
    // A single name, no <Geometry><Orientation> pair — recorded against the
    // row-2.4 law deliberately: the drop-cutter machines the reachable upper
    // envelope from above, so side-of-material is not a variant axis here and
    // there is nothing for winding to validate (Mesh and SplineRevolve stand
    // as precedent). Like STL vertices and the meridian's control points, the
    // patch's control net is mm by definition — under `units inch` only the
    // placement offsets x/y/z scale at the parse boundary. `file` is kept for
    // error messages only; the geometry the ops consume is `surface`, loaded
    // and validated at parse time from a `.patch` file.
    NurbsPatch { surface: crate::nurbs::BSplineSurface, x: f64, y: f64, z: f64, file: String },
}

impl Feature {
    /// The declaration's own spelling — the PascalCase word a `.part` file
    /// writes and a reader recognizes. Lives beside the enum because the name
    /// of a variant is a property of the enum, not of any consumer: the
    /// project outline (`outline.rs`) is the first caller and the Phase-6
    /// renderer will be the second, and neither should carry its own table of
    /// spellings to drift.
    ///
    /// `Debug` is deliberately not this function: `Mesh`, `SplineRevolve`,
    /// `SplineRevolveFunnel`, and `NurbsPatch` carry whole triangle lists and
    /// control nets, so a derived spelling would print a megabyte where a word
    /// belongs. `OpType` has no such payload and is read through `Debug`.
    pub fn kind(&self) -> &'static str {
        match self {
            Feature::Plane { .. } => "Plane",
            Feature::Face { .. } => "Face",
            Feature::ProfilePocket { .. } => "ProfilePocket",
            Feature::ProfileBoss { .. } => "ProfileBoss",
            Feature::ContourPocket { .. } => "ContourPocket",
            Feature::ContourBoss { .. } => "ContourBoss",
            Feature::CylinderBore { .. } => "CylinderBore",
            Feature::TiltedBore { .. } => "TiltedBore",
            Feature::CylinderBoss { .. } => "CylinderBoss",
            Feature::Sphere { .. } => "Sphere",
            Feature::Cone { .. } => "Cone",
            Feature::Torus { .. } => "Torus",
            Feature::Mesh { .. } => "Mesh",
            Feature::SplineRevolve { .. } => "SplineRevolve",
            Feature::SplineRevolveFunnel { .. } => "SplineRevolveFunnel",
            Feature::NurbsPatch { .. } => "NurbsPatch",
        }
    }

    /// The rotational axis this feature is declared about, where it has one
    /// (`DECISIONS.md` → "the multi-feature cavity…", fork 2A — the first
    /// consumer needs to know which declared features stand on ONE axis).
    ///
    /// `None` is not "at the origin": it is *this feature states no axis*.
    /// The polyline and wire pairs are polygons, a `Plane` is unbounded, and
    /// a `Mesh` or a `NurbsPatch` states a PLACEMENT — the point its data is
    /// hung from, which is not a line anything is symmetric about. A consumer
    /// asking "are these two coaxial?" must read `None` as "cannot say", never
    /// as an answer.
    ///
    /// Exhaustive on purpose, like `kind()` above: a new variant stops
    /// compiling here rather than silently claiming to stand nowhere.
    pub fn axis(&self) -> Option<Pt> {
        match self {
            Feature::Plane { .. }
            | Feature::Face { .. }
            | Feature::ProfilePocket { .. }
            | Feature::ProfileBoss { .. }
            | Feature::ContourPocket { .. }
            | Feature::ContourBoss { .. }
            | Feature::Mesh { .. }
            | Feature::NurbsPatch { .. } => None,
            // A tilted bore's axis is a LINE through (x, y) at the stock
            // top, not a vertical; nothing +Z-symmetric is coaxial with
            // it, and "cannot say" is the honest answer.
            Feature::TiltedBore { .. } => None,
            Feature::CylinderBore { x, y, .. }
            | Feature::CylinderBoss { x, y, .. }
            | Feature::Sphere { x, y, .. }
            | Feature::Cone { x, y, .. }
            | Feature::Torus { x, y, .. }
            | Feature::SplineRevolve { x, y, .. }
            | Feature::SplineRevolveFunnel { x, y, .. } => Some(Pt { x: *x, y: *y }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CycleType {
    Simple,
    Peck,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpType {
    // The tool's end on a floor (Phase 36 slice 2; né `Floor`, né `Facing`).
    // `floor` stays the noun: the surface the end makes.
    End,
    // Area clearance of an enclosed void (né `Pocket` — the noun is an
    // orientation word now, DECISIONS.md "Geometry-first fixture naming…").
    Clear,
    // `Drill` dissolved in Phase 36 slice 3: a visit to POINTS is a motion,
    // read off the reference key (`points=`), under `Form` or `End`.
    // The three verbs that FOLLOW A SURFACE, named for the part of the tool
    // that makes it (Phase 36 slice 1, DECISIONS.md → "DRAFT: Phase 36 opens
    // — OPS BY CONTACT…"). They were one verb, `Finish`, which dispatched on
    // the tool's form first and the feature second; the verb now says which.
    //
    // The tool's flank on a wall: the prismatic pairs, traced or ringed.
    Side,
    // A ball's nose on an in-between surface, at a point, in many passes.
    Copy,
    // A tool that carries the shape — a rounder, a chamfer mill, a drawn
    // profile — placed by the path and nothing more.
    Form,
}

/// A word behind its article — `an End`, `a Side` — so a sentence built
/// around an op word reads whichever word it is handed.
pub fn with_article(word: &str) -> String {
    let an = word.chars().next().is_some_and(|c| "AEIOUaeiou".contains(c));
    format!("{} {word}", if an { "an" } else { "a" })
}

impl OpType {
    /// The verb as the `.cam` file spells it — the one home for the word in
    /// a message, so three refusals cannot drift into three spellings.
    pub fn verb(self) -> &'static str {
        match self {
            OpType::End => "End",
            OpType::Clear => "Clear",
            OpType::Side => "Side",
            OpType::Copy => "Copy",
            OpType::Form => "Form",
        }
    }

    /// Does the op follow a surface (one increment) rather than consume a
    /// volume or visit points? The three contact verbs that were `Finish`.
    pub fn follows_surface(self) -> bool {
        matches!(self, OpType::Side | OpType::Copy | OpType::Form)
    }

    /// May the verb visit POINTS instead (`points=`)? The tool's form, or
    /// its end, plunged: the two that were `Drill`.
    pub fn may_visit_points(self) -> bool {
        matches!(self, OpType::Form | OpType::End)
    }
}

// Finishing compensation mode, per DECISIONS.md "Finishing comp strategy: a
// single per-op `comp` mode". Mutually exclusive by construction (one-of-N, not
// a pair of toggles) so the two illegal states of the Pre-Offset x G41/G42
// matrix — a one-radius gouge and a double-comp — are unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompMode {
    // Engine computes the tool-center offset path itself and emits plain
    // coordinates with no cutter comp active. The portable, self-contained
    // default.
    Engine,
    // Emit centerline = part edge and let the controller apply cutter radius
    // compensation via G41/G42 + the D register (derive_d), so wear/deflection
    // can be dialed in at the machine. Opt-in.
    Control,
}

// The voice of the engagement linter (`engagement_findings`). Advise is the
// default and matches every other lint: findings go to stderr, the program
// still comes out. Gate is the committed brake (ROADMAP "Engagement-driven
// stock/feed coupling": "the hard-gate form of the linter is required as at
// least an option") — the same findings reject the program at validate. The
// advisory-vs-gate *default* is a still-open question on that item; this enum
// keeps today's advisory behavior as the status quo rather than settling it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngagementMode {
    Advise,
    Gate,
}

// The voice of the edge-round deviation budget (`deviation_findings`,
// DECISIONS.md → "the corner rounder gets an operation"). Advise is the
// default, and deliberately so: every real ground arc trails off, so a gate by
// default would reject every real program, and "an unmachinable feature is a
// hard error" cannot be the law for a quantity that is never zero. What is not
// optional is that the number EXISTS — the gap this fills is not that
// traditional CAM tolerates the deviation, it is that it cannot tell you what
// it is. (Doc note: this comment and EntryMode's below were swapped for a
// while; the enums themselves were always wired correctly.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeviationMode {
    Advise,
    Gate,
}

// The voice of the plunge taxonomy's category-3 flag (`stock::entry_findings`,
// DECISIONS.md "the plunge taxonomy"). Same two-voice pattern as the
// engagement linter, its OWN directive: entry discipline and allowance
// pairing are different axes, and an author may want either without the
// other. Gate is enforced in `gcode::program`, not at validate — entries
// exist only once moves do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EntryMode {
    Advise,
    Gate,
}

/// The keep-out check's voice (DECISIONS.md → "V2, the fixture…", fork 6A).
/// Its own directive like its three siblings, because avoiding workholding is
/// a different axis of authoring choice from entry discipline or allowance
/// pairing. Enforced in `gcode::program` rather than at validate — a keep-out
/// is met by a MOVE, and moves exist only once emitted.
///
/// **Unlike body-foul, this one may gate**, and joins `apply_gate_floor`. The
/// standing constraint is that a gate must not promise what a probe cannot,
/// and it muzzles body-foul because a sampled lattice biased to silence can
/// MISS. This check is closed form and bounded from above: it may name a
/// clearance that was really there, and can never miss one that was not. An
/// over-claiming bound does not run into that constraint at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FixtureMode {
    Advise,
    Gate,
}

/// The reach certificate's voice — `reach advise|gate` (`DECISIONS.md` →
/// "`reach gate`, the fifth directive…"). Its own directive like its four
/// siblings: an assembly too short for the depth it works in is a different
/// axis of authoring choice from entry discipline, allowance pairing, or
/// workholding. Enforced in `gcode::program` rather than at validate, and that
/// is FORCED rather than chosen — this certificate reads the emitted moves, so
/// it has nothing to read until they exist.
///
/// **Why it may gate**, applying the family's own law rather than restating a
/// preference: a gate must not promise what a probe cannot, and that
/// constraint "is about probes that MISS" — which is what muzzles body-foul's
/// sampled lattice. The keep-out passes it by over-claiming. This one passes a
/// third way and a stronger one: it neither over- nor under-claims. `stickout`
/// is a declared number, the deepest tip is the emitted Z less
/// `Silhouette::drop()`, and the comparison is one subtraction — exact where it
/// speaks, and silent only where the declaration is absent, which is the
/// house's universal undeclared-means-today's-behaviour law and not a miss.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReachMode {
    Advise,
    Gate,
}

/// The envelope's voice — `envelope advise|gate`, the SIXTH directive
/// (Phase 15 slice 4, DECISIONS.md → "Phase 15 opens: the envelope
/// DRAFT…", fork 4, taken at the DRAFT's blessing). Its own directive like
/// its five siblings: a program the declared machine cannot run is a
/// different axis of authoring choice from any of theirs.
///
/// **Why it may gate**, by the family's own law rather than a preference:
/// a gate must not promise what a probe cannot, and every arm of this one
/// neither over- nor under-claims — a travel crossing is an emitted
/// coordinate plus a declared datum against a declared span (one addition,
/// one comparison), the ceiling is a resolved RPM against a declared max,
/// the changer a count against a count. The same third way `reach` passed.
/// The two parse-time arms gate at `validate` (and once more after the
/// derive words resolve, the engagement gate's pattern — a `spindle=derive`
/// op has no RPM until then); the travel arm gates in `gcode::program`,
/// where emitted moves exist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnvelopeMode {
    Advise,
    Gate,
}

/// A coolant KIND — semantic intent, never a controller word (DECISIONS.md →
/// "coolant as a modeled setting…", fork 1A as amended at blessing). Five
/// named kinds plus three opaque custom slots for machine-specific scenarios
/// (high-pressure through, shower, chip blast — whatever a shop's controller
/// has a word for). Which M-word a kind emits is the machine MENU's business
/// (`CoolantMenu`); the program text only ever states the kind. The custom
/// slots are menu-only by construction — no default word exists or ever
/// will — so a program carrying `coolant=custom1` is deliberately coupled to
/// its machine file; the semantic five are the portable spellings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoolantKind {
    Flood,
    Mist,
    Air,
    Through,
    Off,
    Custom1,
    Custom2,
    Custom3,
}

impl CoolantKind {
    /// The program-text spelling, one home for parse errors, validate
    /// messages, and the machine-file key suffix (`coolant_<name>`).
    pub fn name(self) -> &'static str {
        match self {
            CoolantKind::Flood => "flood",
            CoolantKind::Mist => "mist",
            CoolantKind::Air => "air",
            CoolantKind::Through => "through",
            CoolantKind::Off => "off",
            CoolantKind::Custom1 => "custom1",
            CoolantKind::Custom2 => "custom2",
            CoolantKind::Custom3 => "custom3",
        }
    }

    /// Parse a declared kind. The caller owns the error so the message can
    /// carry its line and its home (tool line vs run line).
    pub fn parse(s: &str) -> Option<CoolantKind> {
        Some(match s {
            "flood" => CoolantKind::Flood,
            "mist" => CoolantKind::Mist,
            "air" => CoolantKind::Air,
            "through" => CoolantKind::Through,
            "off" => CoolantKind::Off,
            "custom1" => CoolantKind::Custom1,
            "custom2" => CoolantKind::Custom2,
            "custom3" => CoolantKind::Custom3,
            _ => return None,
        })
    }

    pub const ALL_NAMES: &'static str = "flood, mist, air, through, off, custom1, custom2, custom3";
}

/// The machine's coolant menu (DECISIONS.md → "coolant as a modeled
/// setting…", fork 3A as amended): which M-word each kind emits on THIS
/// controller. Every field optional; absent fields fall back to the
/// undeclared dialect — flood → `M08`, off → `M09`, everything else
/// unsayable — so a declared machine with no coolant keys changes nothing
/// (the K1 default discipline), and the menu AUGMENTS the defaults rather
/// than replacing them (a machine that cannot flood at all is not
/// expressible in v1, priced in the design entry).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoolantMenu {
    pub flood: Option<String>,
    pub mist: Option<String>,
    pub air: Option<String>,
    pub through: Option<String>,
    pub off: Option<String>,
    pub custom1: Option<String>,
    pub custom2: Option<String>,
    pub custom3: Option<String>,
}

impl CoolantMenu {
    /// The declared slot for a kind, mutable — `parse_machine`'s assignment
    /// arm, one match instead of eight.
    pub(crate) fn slot(&mut self, k: CoolantKind) -> &mut Option<String> {
        match k {
            CoolantKind::Flood => &mut self.flood,
            CoolantKind::Mist => &mut self.mist,
            CoolantKind::Air => &mut self.air,
            CoolantKind::Through => &mut self.through,
            CoolantKind::Off => &mut self.off,
            CoolantKind::Custom1 => &mut self.custom1,
            CoolantKind::Custom2 => &mut self.custom2,
            CoolantKind::Custom3 => &mut self.custom3,
        }
    }

    /// The word a kind emits under this menu: the declared word, or the
    /// undeclared dialect's for the two kinds it has always said. `None` is
    /// the error validate turns into prose.
    pub fn word(&self, k: CoolantKind) -> Option<&str> {
        let declared = match k {
            CoolantKind::Flood => &self.flood,
            CoolantKind::Mist => &self.mist,
            CoolantKind::Air => &self.air,
            CoolantKind::Through => &self.through,
            CoolantKind::Off => &self.off,
            CoolantKind::Custom1 => &self.custom1,
            CoolantKind::Custom2 => &self.custom2,
            CoolantKind::Custom3 => &self.custom3,
        };
        match declared {
            Some(w) => Some(w),
            None => match k {
                CoolantKind::Flood => Some("M08"),
                CoolantKind::Off => Some("M09"),
                _ => None,
            },
        }
    }
}

/// The one home for "what word does this kind emit here" across a declared
/// and an undeclared machine — validate rejects through it and the emitter
/// spells through it, so the error and the program cannot disagree.
pub fn coolant_word(machine: Option<&Machine>, k: CoolantKind) -> Option<String> {
    match machine {
        Some(m) => m.coolant.word(k).map(str::to_string),
        None => CoolantMenu::default().word(k).map(str::to_string),
    }
}

/// The resolved coolant for an op: op override > tool fallback > flood —
/// the undeclared default said in one place, so a program that says nothing
/// emits today's bytes by construction (DECISIONS.md → "coolant as a
/// modeled setting…", fork 2A).
pub fn coolant_for(p: &Part, op: &Operation) -> CoolantKind {
    op.coolant
        .or_else(|| p.tools.get(&op.tool_num).and_then(|t| t.coolant))
        .unwrap_or(CoolantKind::Flood)
}

/// A work-offset SELECTION (DECISIONS.md → "work offsets…", fork 1A): which
/// pre-defined machine register the program's coordinate space rides —
/// tenet 5's own distinction, made a type. A selection never carries values
/// and never sets a datum (`G92`/`G10`/`G52` are rejected by name at parse);
/// where the register actually points is the operator's setup sheet, not
/// this program's claim.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkOffset {
    /// One of the six standard registers, `G54`–`G59` — the undeclared
    /// dialect: unlike the coolant words, they mean the same thing on every
    /// controller this project can name, so no menu gates them.
    Standard(u8),
    /// A register in the machine's declared EXTENDED family (fork 3A):
    /// `family` is the program-text token's word half (`"G54.1P"`,
    /// `"G154P"` — the vendor's spelling, which is exactly why it is menu
    /// territory), `p` the register number. Sayable only when the machine's
    /// `extended_offsets` menu names this family with at least `p` registers
    /// — checked at validate, spelled at emission, one home each.
    Extended { family: String, p: usize },
}

impl WorkOffset {
    /// The setup-line words this selection emits: `G55`, or the vendor's
    /// two-word spelling `G54.1 P7` (the family splits at its trailing `P`;
    /// program text keeps the one-token form because a directive value with
    /// interior spaces is a parsing ambush).
    pub fn words(&self) -> String {
        match self {
            WorkOffset::Standard(n) => format!("G{}", n),
            WorkOffset::Extended { family, p } => {
                format!("{} P{}", &family[..family.len() - 1], p)
            }
        }
    }
}

/// The gate floor (DECISIONS.md → "the agent surface — every verb is a flag
/// first…", fork 5A): escalate the parsed gate directives advise→gate, in
/// exactly one named place — six of them since the envelope directive joined
/// the reach certificate's five (`DECISIONS.md` → "the reach gate lands…",
/// then "Phase 15 opens: the envelope DRAFT…" fork 4). The policy home stays the program
/// text — this is an escalation applied to the same enums, never a parallel
/// policy
/// — and it is escalation-only by construction: a file-declared `gate` is
/// already at the floor, and nothing here can lower anything. Body-foul stays
/// advisory in every mode by its own standing rationale (a sampled probe
/// biased to silence must not gate). Exposed CLI-first as `--gates gate`; the
/// MCP server applies it to every verb unconditionally, with opt-down a
/// launch-time human act (`--gates declared`), never a per-call parameter.
///
/// The caller re-runs `validate` after this: the engagement and deviation
/// gates live there, and parse already ran it once at the declared modes —
/// the second pass is what surfaces the floor's errors.
pub fn apply_gate_floor(p: &mut Part) {
    p.engagement = EngagementMode::Gate;
    p.deviation = DeviationMode::Gate;
    p.entry = EntryMode::Gate;
    // Joins the floor, unlike body-foul: this probe over-claims, so escalating
    // it can only ever reject a program that might have been fine — never
    // bless one that crashes.
    p.fixture_mode = FixtureMode::Gate;
    // The fifth, and the one an unattended reader most needs escalated: a
    // reach shortfall is invisible in the G-code — every coordinate is legal
    // and the machine will run it — so a warning is exactly the form of this
    // finding that gets talked past.
    p.reach_mode = ReachMode::Gate;
    // The sixth (Phase 15 slice 4): a program the declared machine cannot
    // run. Every arm's probe is exact — the eligibility that let the
    // directive open at all — so escalation can only reject a program a
    // real axis, spindle, or changer would have rejected in metal.
    p.envelope_mode = EnvelopeMode::Gate;
}

// How the emitted S word spells a spindle speed (DECISIONS.md "K1, the
// machine…", fork 2A). `Decimal` is today's `{:.3}` — the S word wearing the
// coordinates' decimals, which is what every golden says and what an
// UNDECLARED machine must keep emitting byte for byte. `Integer` is the
// Fanuc-family spelling (`S3000`), the one word in the walked dialect most
// likely to be already wrong on a real controller — and the K1 acceptance
// word. A closed set matched arm-by-arm, like `comp`/`compress`: an unknown
// value rejects at the machine-file parse, never passes through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpindleWord {
    Integer,
    Decimal,
}

// The units the CONTROLLER reads (DECISIONS.md "K1 slice 1 — the formatting
// boundary…"). Deliberately the same word the `.part` grammar uses for its
// input declaration, because it is the same fact about a different party:
// `units` in the part file says what the AUTHOR's numbers mean (converted
// once, at the parse boundary), `units` in the machine file says what the
// controller's numbers must mean (converted once, at the formatting
// boundary). Internal model is mm, always, between the two. `Mm` is the
// undeclared default — today's G21 output, byte for byte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutputUnits {
    Mm,
    Inch,
}

// The declared machine (DECISIONS.md "K1, the machine…"): the controller
// dialect facts the writer may read, declared once in a machine file beside
// the project and referenced by a single `machine <file>` line — never
// inlined in `main.part`, because a machine is shop infrastructure shared by
// every part the shop cuts, and an inline copy is the founding stale-copy
// bug at shop scale (fork 1B). The undeclared machine lives in CODE, never
// in a default file: `Part::machine == None` means today's exact output,
// byte for byte, with no file read at all.
//
// v1 declared exactly two facts: `name` (identity only — it prints nothing,
// because emitting even an identifying comment would break byte-identical)
// and `spindle_word`. Slice 1 added the output-format class — `units`,
// `coord_decimals`, `feed_decimals` (DECISIONS.md "K1 slice 1 — the
// formatting boundary…"). Every later dialect fact (N-word policy, coolant
// coverage, the work-offset menu) is a new field with its own arm in
// `parse_machine` — growth is additive, and an unknown key already rejects
// loudly, so no field needs a reserved-value trick.
#[derive(Clone, Debug, PartialEq)]
pub struct Machine {
    pub name: Option<String>,
    pub spindle_word: SpindleWord,
    // The output-format class (DECISIONS.md "K1 slice 1 — the formatting
    // boundary…"): what units the controller reads, and how many decimals
    // each word class carries. `coord_decimals` governs X/Y/Z/I/J,
    // `feed_decimals` governs F; S is governed by `spindle_word` alone
    // (an RPM has no length unit and never converts). Undeclared decimals
    // resolve by units at parse: mm → 3 (today's spelling), inch → 4
    // (tenths of a thou, the ROADMAP's motivating pairing).
    pub units: OutputUnits,
    pub coord_decimals: usize,
    pub feed_decimals: usize,
    /// The coolant menu (DECISIONS.md → "coolant as a modeled setting…",
    /// fork 3A): which M-word each kind emits on this controller. Default
    /// (every slot empty) falls back to the undeclared dialect.
    pub coolant: CoolantMenu,
    /// The extended work-offset family this controller speaks (DECISIONS.md
    /// → "work offsets…", fork 3A): the word half of the register token
    /// (`"G54.1P"` Fanuc-style, `"G154P"` Haas-style — the spelling is the
    /// vendor's, which is exactly why it is menu territory) and how many
    /// registers exist. `None` = only `G54`–`G59` are sayable, the
    /// undeclared dialect.
    pub extended_offsets: Option<(String, usize)>,
    /// The machine's X travel span, in MACHINE coordinates and internal mm
    /// — `travel_x <min> <max>` (Phase 15, DECISIONS.md → "Phase 15 opens:
    /// the envelope DRAFT…", fork 1). A span is a fact about the AXIS, so
    /// it lives in the shop-infrastructure file; where the program's work
    /// zero sits on that axis is the part-side `datum` line's fact, and the
    /// travel check runs only when both are declared. `None` = no claim,
    /// today's behaviour. Values in an inch machine file are the
    /// controller's inches, converted here at the machine-file parse — the
    /// same one-boundary rule as every other length.
    pub travel_x: Option<(f64, f64)>,
    /// The Y span — `travel_y <min> <max>`, `travel_x`'s twin. There is
    /// deliberately no `travel_z`: the machine-frame Z of a comp'd move
    /// rides the controller's H register VALUE, a length this project does
    /// not model, so the key rejects by name rather than checking a guess
    /// (fork 1's Z ruling).
    pub travel_y: Option<(f64, f64)>,
    /// The spindle's ceiling in RPM — `spindle_max <rpm>` (Phase 15 slice
    /// 2, fork 2's blessing: one line, not a curve — the power/torque
    /// table's one customer is the feeds engine, which is not built, and a
    /// table nobody reads is the silence `reject_unknown_keys` forbids).
    /// An RPM has no length unit and never converts, the `spindle_word`
    /// rule. The check it feeds advises and NEVER clamps: a silently
    /// lowered S would change the chip load every derived feed was priced
    /// for — the clamp is the T3 coupling's move on the day it lands.
    pub spindle_max: Option<f64>,
    /// The changer's pocket count — `atc_pockets <count>` (Phase 15 slice
    /// 3, fork 3's blessing): how many distinct tools one program may ask
    /// for. A count against a count, exact both ways. Max tool
    /// diameter/length is deliberately NOT here: gauging an assembly
    /// against declared constraints is `--assembly-report`'s owned
    /// question, and a second home for the same comparison is the drift
    /// DECISIONS.md exists to prevent.
    pub atc_pockets: Option<usize>,
    /// The rapid traverse rates, per axis, in internal mm/min — `rapid_x`,
    /// `rapid_y`, `rapid_z` (Phase 16 slice 1, DECISIONS.md → "Phase 16
    /// opens: the clock-and-address DRAFT…", fork 1): what the spec sheet
    /// publishes, in the machine file's own units per minute, converting at
    /// this parse like every machine-file length. These are TIME inputs,
    /// never limits — the cut-time model's rapids read them, and a machine
    /// declaring none makes the report decline the rapid share by name
    /// rather than guess a traverse rate. (`rapid_z` is a rate, not a
    /// position: the travel-key Z ruling does not apply.)
    pub rapid_x: Option<f64>,
    pub rapid_y: Option<f64>,
    pub rapid_z: Option<f64>,
}

impl Default for Machine {
    fn default() -> Self {
        Machine {
            name: None,
            spindle_word: SpindleWord::Decimal,
            units: OutputUnits::Mm,
            coord_decimals: 3,
            feed_decimals: 3,
            coolant: CoolantMenu::default(),
            extended_offsets: None,
            travel_x: None,
            travel_y: None,
            spindle_max: None,
            atc_pockets: None,
            rapid_x: None,
            rapid_y: None,
            rapid_z: None,
        }
    }
}

impl Machine {
    /// True when the declared machine's output format is the undeclared
    /// default — metric words at 3 decimals. The formatting boundary's
    /// respell pass (gcode.rs) runs only when this is false, so a declared
    /// machine that states the defaults explicitly is byte-identical to no
    /// machine at all, by construction rather than by arithmetic.
    pub fn output_format_is_default(&self) -> bool {
        self.units == OutputUnits::Mm && self.coord_decimals == 3 && self.feed_decimals == 3
    }
}

// Per-operation output *representation*, per DECISIONS.md "Per-op compress:
// arcs-first toggle committed, force/spline reserved". Orthogonal to `comp`
// (which decides who applies the offset); this decides how the already-computed
// curve is written out. One-of-N like `comp`, so illegal combinations stay
// unrepresentable and the field never has to change shape as modes are added.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompressMode {
    // Flatten every curve to a G01 polyline within CHORD_TOL_MM. The default,
    // and byte-identical to what shipped before this field existed; the safe,
    // universal baseline since every controller reads G01.
    Chords,
    // Emit the exact circle as a true G02/G03 for every op that already *knows*
    // its curve is a circular arc (the finish waterline rings, the cylinder
    // bore wall, the boss wall). A fidelity gain, not an approximation — an arc
    // is geometrically exact, so CHORD_TOL_MM never enters. The elliptic
    // parallels are not circular arcs and stay chorded even here.
    Arcs,
    // Reserved vocabulary (ROADMAP.md "Per-operation output compression"): the
    // words are claimed now so the enum never changes shape when they land, but
    // the parser rejects them until each is designed and built. `Force` is
    // arc-fit of an arbitrary chord stream within a new tolerance; `Spline` is
    // polynomial/BSPLINE output. Constructed nowhere yet — hence the allow.
    #[allow(dead_code)]
    Force,
    #[allow(dead_code)]
    Spline,
}

/// Which way a pass with material on one side runs under M03 (ROADMAP item 2,
/// slice 2). Climb: the material on the tool's right, each tooth entering at
/// full chip — every pass the engine wrote before this key existed. The
/// engine's loops are wound to climb; `Conventional` runs each the other way
/// round from the same start, and writes G42 where control comp is asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    Climb,
    Conventional,
}

impl Direction {
    pub fn word(self) -> &'static str {
        match self {
            Direction::Climb => "climb",
            Direction::Conventional => "conventional",
        }
    }
}

// How a Clear picks its passes (DECISIONS.md "adaptive clearing — the field
// picks the passes…", slice 1). One-of-N like `comp` and `compress`, so
// illegal combinations stay unrepresentable. Absent means today's dense
// spiral, byte-identical. `Rest` is whole-loop selection against the voxel
// stock field: the field SELECTS, the analytic offset family CUTS — every
// emitted pass is the same family member it would be without the field, the
// field's authority is only "this loop still has work; this loop is air".
// `Regions` is region decomposition (DECISIONS.md "adaptive clearing, region
// decomposition…"): the field SHAPES the work-list — per-region offset
// spirals over the standing material a prior op left — while the declaration
// keeps the walls (every emitted point is clamped by declaration-exact
// offsets of the authored feature) and the family still cuts. A different
// promise than `Rest` (the field's material within the declaration, to the
// stated resolution — not the dense family's coverage), hence its own word.
// `adaptive` is RESERVED for the engagement-bounded trochoidal tier (the
// compress force/spline precedent: claim the word, reject it until designed).
/// How an op walks the region it works. `Offset` is loops parallel to every
/// wall (the family Phases 26–27 built); `Zigzag` is parallel passes clipped to
/// the region, alternating, joined ALONG its boundary, closed by one pass
/// along every wall; `Oneway` is the same passes all cut in one direction;
/// `Spine` (Phase 30) is rows cut only where they own floor, one shore pass
/// along what stands and only there, on a comb chosen by measuring it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pattern {
    Offset,
    Zigzag,
    Oneway,
    Spine,
    /// The first raster (Phase 1), named in Phase 35 slice 4: X-running
    /// passes over the whole BLANK, overhung by a radius. It reads no region,
    /// so it is legal on a bare `Plane` and nowhere else. It is what a
    /// `Floor` on a bare plane cut until the default moved, and what
    /// `nacs.py` ports.
    Raster,
    /// HELD under a ceiling on engagement (Phase 62 slice 1): spine's comb
    /// at the spacing the ceiling derives, `r(1 − cos θ)`, walked from the
    /// air, its links read through the walk as they are laid and lifted
    /// where one would exceed the ceiling, the places it cannot hold said
    /// by name. `engagement=<deg>` is its one key (90° without it) and
    /// `stepover=` is refused beside it.
    Held,
}

impl Pattern {
    pub const ALL: [Pattern; 5] = [Pattern::Offset, Pattern::Zigzag, Pattern::Oneway, Pattern::Spine, Pattern::Held];
    pub fn word(self) -> &'static str {
        match self {
            Pattern::Offset => "offset",
            Pattern::Zigzag => "zigzag",
            Pattern::Oneway => "oneway",
            Pattern::Spine => "spine",
            Pattern::Raster => "raster",
            Pattern::Held => "held",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClearStrategy {
    Rest,
    Regions,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    pub op_type: OpType,
    pub tool_num: usize,
    pub feature_name: String,
    // No `depth`. Vertical occupancy is the *feature's* geometry — `extent` on a
    // Contour, `depth` on a CylinderBore, a derived height on the analytic surfaces
    // — never an operation input (DECISIONS.md "Vertical occupancy is
    // `extent`…"). An op says which tool cuts what and how, not how far down.
    /// The spacing between adjacent passes. On a `Clear` it is the radial bite
    /// (ae). On a `Finish` it is measured along the target surface, and its
    /// direction is the surface's own, never declared: a surface has one free
    /// direction once the pass direction is chosen, so a `Finish` has exactly
    /// one increment — across a floor's raster, up a flank's rings, along a
    /// dome's meridian. The vertical case is still a stepover; nothing is
    /// stepped *through* (DECISIONS.md → "The increment intake…").
    pub stepover: Option<f64>,
    /// The axial bite (ap) of a `Clear` over standing material — a revolve or
    /// a boss — read as a MAXIMUM (DECISIONS.md → "roughing a convex
    /// revolve…", fork 2). A volume needs two increments to slice; a surface
    /// has room for one, so `stepdown` is refused on every other op rather
    /// than ridden ignored. This is not `depth` in disguise and does not
    /// breach "vertical occupancy is the feature's": the FLOOR is still the
    /// feature's own base plane, derived — this only says how many bites the
    /// tool takes getting there, exactly as `stepover` and `scallop` say how
    /// it spaces its passes.
    pub stepdown: Option<f64>,
    pub scallop: Option<f64>,
    pub feed: Option<f64>,
    pub plunge: Option<f64>,
    pub rapid_z: Option<f64>,
    pub clearance_z: Option<f64>,
    pub spindle: Option<f64>,
    // The `feed=derive` / `spindle=derive` sentinels (DECISIONS.md → "K3,
    // the material…", fork 4A): the author literally replaces the number
    // with the word, so one key cannot carry two sources and
    // declared-outranks-derived holds by construction. Resolution happens
    // once, at the end of parse (`resolve_derived_words`), writing a plain
    // number into `feed`/`spindle` above — so the `unwrap_or` sites and the
    // one S seam in gcode.rs never change and emission never learns the
    // table exists. Both flags false = today's behavior, byte for byte.
    pub feed_derive: bool,
    pub spindle_derive: bool,
    // The chip-thinning correction the derived feed actually rode, when one
    // was available (Phase 8 V1 slice 1 — `DECISIONS.md` → "V1 slice 1…").
    // Written by `resolve_derived_words` beside `feed`, and read only to
    // AUDIT: the op title states the factor and the engagement it came from,
    // because a feed raised by a computation the program text cannot show is
    // the silence this project keeps rejecting. `None` — no sentinel, or a
    // carrier whose engagement is not computable — emits nothing at all, so
    // every golden without it is byte-identical by construction.
    pub chip_thinning: Option<crate::engagement::ChipThinning>,
    // The feed word of EACH pass, in emission order, when the chip-thinned
    // feed rode a per-pass engagement (Phase 9 lane A, slice 1 —
    // `DECISIONS.md` → "Phase 9 lane A, slice 1…"): the slot at the un-thinned
    // identity, every later loop at its own measured factor. Written beside
    // `feed` (which stays the worst non-slot pass's word, the one number a
    // single-feed reader sees) and consumed by `ops::clear` per level. `None`
    // emits one word everywhere — every golden without it is byte-identical.
    pub pass_feeds: Option<Vec<f64>>,
    pub drill_points: Option<Vec<String>>,
    pub cycle: Option<CycleType>,
    pub peck_increment: Option<f64>,
    // Finishing comp mode. Only Contour reads it; other ops carry the default.
    pub comp: CompMode,
    // Output representation. Read by the finish ops (waterline rings, bore,
    // boss) whose curve is a known circle; other ops carry the default.
    pub compress: CompressMode,
    // Which way each pass with material on one side runs (ROADMAP item 2,
    // slice 2). `None` is unsaid: every pass climbs, as every program before
    // the key did. Read by Clear, End and Side; refused on the rest at parse.
    pub direction: Option<Direction>,
    // How every descent in the op is made (ROADMAP item 2, slice 4). `None`
    // is unsaid: each descent is the kind the plan gives it — a ramp into
    // standing stock, a plunge beside air, a lead from the air past the
    // blank. Said, every descent is that kind, and the op refuses where one
    // cannot be. Read by Clear, End and Side; refused on the rest at parse.
    pub entry: Option<EntryKind>,
    // The lead's shape (ROADMAP item 2, slice 5): how far from the start the
    // descent is made (mm; a diameter unsaid), which way from the start it
    // lies (degrees; 0 behind on the pass's own line, 90 off to the air side,
    // the unsaid default) and how steeply the flight comes down (degrees
    // below flat; 0 unsaid). Read where a lead is made; refused elsewhere.
    pub lead: Option<f64>,
    pub lead_angle: Option<f64>,
    pub lead_descent: Option<f64>,
    // Roughing/semi-finish stock allowance in mm. Its PRESENCE is the roughing
    // tag — there is no separate `mode` field, and there will not be one
    // (DECISIONS.md "Operation role is derived, not declared…", settling what
    // "stock_to_leave first slice…" left open): an op's role is DERIVED from
    // what the pass leaves, never declared, so a semi-finish pass stays
    // representable without a second spelling to keep in sync. Semantics are
    // max-not-min (DECISIONS.md "Stock-to-leave semantics…"): a
    // guarantee-ceiling on TOTAL remaining material including the pass's own
    // cusps, so the pass math budgets its cusp height inside this number. Read
    // by `Finish` (every arm) and `Clear`; `Floor` and `Drill` reject it.
    pub stock_to_leave: Option<f64>,
    // Pass selection for Clear; absent = the dense spiral. Only Clear reads
    // it; validate() rejects it anywhere else rather than letting it ride
    // ignored.
    pub strategy: Option<ClearStrategy>,
    // HOW a region is walked (Phase 28 slice 3; `DECISIONS.md` → "Phase 28
    // slice 3: the patterns…") — orthogonal to `strategy`, which says WHICH
    // MATERIAL a `Clear` works. Absent is the default every program written
    // before it cuts: a bare `Plane` the raster, everything else the offset
    // family. The pin the comparison report hands the author.
    pub pattern: Option<Pattern>,
    /// The ceiling `pattern=held` lays its rows under, in degrees of
    /// engagement (Phase 62 slice 1): `None` is the 90° line, said as the
    /// default. Refused beside any other pattern.
    pub engagement: Option<f64>,
    /// The rows' direction, in degrees from +X (ROADMAP item 2, slice 3):
    /// `None` and spine's comb chooses among the directions a person would
    /// name; said, the rows run at this angle and the comb chooses only the
    /// phase and the way. Read by the row patterns (spine, zigzag, oneway,
    /// held); refused on the rest.
    pub angle: Option<f64>,
    // The CONCESSION (Phase 32 slice 2; `DECISIONS.md` → "Phase 32 slice 2:
    // the concession…"): a floor with a stretch no pass can enter refuses,
    // unless the run line concedes it — `cover=96.9`, the least share of the
    // floor this op must reach. Bounded: it fails the day less is reachable.
    pub cover: Option<f64>,
    // The voxel field's finest-cell size (mm) for strategy=rest — REQUIRED
    // with it, meaningless without it, and deliberately explicit-only: a
    // field-selected path bakes the field's resolution into the emitted
    // G-code, so the resolution is program text. A derived default would let
    // an engine-side constant silently change which loops drop
    // (DECISIONS.md "adaptive clearing…", the resolution sub-fork).
    pub voxel_cell: Option<f64>,
    /// This op's coolant OVERRIDE (DECISIONS.md → "coolant as a modeled
    /// setting…", fork 2A): the odd op out — a facing pass run wet on an
    /// otherwise-dry tool. Resolution is op > tool > flood (`coolant_for`).
    pub coolant: Option<CoolantKind>,
    /// This op's rung on the restart ladder (DECISIONS.md → "N-words…",
    /// fork 1A): emitted as `N<label>` prefixing the FIRST line the op
    /// contributes — its own state re-establishment when it has one, its
    /// title otherwise — so the ladder points at the safest line to resume
    /// from by construction. Sparse and hand-placed, the wish's own law;
    /// duplicates reject at validate (an ambiguous ladder is no ladder);
    /// order is deliberately the author's.
    pub n: Option<usize>,
    // PROGRAMMER standoffs, one per orthogonal comp knob, and the ONLY home for
    // a deliberate offset on an edge-round pass (DECISIONS.md → "the corner
    // rounder gets an operation"). Deltas from the virtual sharp corner, so
    // `wall_standoff` backs the tool off the wall and `face_standoff` lifts it
    // off the face — each positive-away, each defaulting to zero.
    //
    // Manufacturer relief is NOT here: it lives in the tool record because it
    // is physical, and it is only ever reported. Two homes for one offset is
    // the silent-stacking failure this design exists to kill, so the split is
    // by ownership (who decided it) rather than by convenience.
    pub wall_standoff: f64,
    pub face_standoff: f64,
    // The chamfer pass's own knob, and the owner of the cone's placement
    // degree of freedom (DECISIONS.md → "the chamfer tool…", option B): how
    // far the relieved tip flat's edge clears the wall below the chamfer,
    // radially. Optional because it has a DERIVED default
    // (`derive_tip_clearance`) — the one standoff-family knob that does,
    // because zero is not a usable default here: zero IS the rejected
    // deepest-legal placement. Only a Form with a ChamferMill reads it;
    // rejected anywhere else rather than riding ignored.
    pub tip_clearance: Option<f64>,
}

/// The kinds a descent into the cut can be (ROADMAP item 2, slice 4): a
/// ramp cuts on the way down, a plunge goes straight down on the path, a lead
/// goes straight down in the air beside the path and feeds onto it flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Ramp,
    Plunge,
    Lead,
}

impl EntryKind {
    pub fn word(self) -> &'static str {
        match self {
            EntryKind::Ramp => "ramp",
            EntryKind::Plunge => "plunge",
            EntryKind::Lead => "lead",
        }
    }
}

impl Operation {
    /// Every pass with material on one side climbs — the default, and what
    /// `direction=climb` says outright. False under `direction=conventional`.
    pub fn climbs(&self) -> bool {
        self.direction != Some(Direction::Conventional)
    }

    /// The MOTION, read off the reference key (Phase 36 slice 3): does the
    /// op visit points (`points=`) rather than follow a surface or work a
    /// floor? One home — what `op_type == Drill` used to ask.
    pub fn at_points(&self) -> bool {
        self.drill_points.is_some()
    }

    /// Does the op follow a surface: a contact verb, and not at points.
    pub fn follows_surface(&self) -> bool {
        self.op_type.follows_surface() && !self.at_points()
    }

    // The wall-cutting op classes — the ones that machine the surfaces a
    // stock_to_leave allowance rides on, and therefore the only ops that can
    // consume an upstream allowance's skin. Floor cuts only its own floor and
    // Drill's diameter is the tool's own, so neither inherits a skin.
    pub fn cuts_walls(&self) -> bool {
        self.follows_surface() || self.op_type == OpType::Clear
    }
}

/// The blank's shape (Phase 46 slice 1, fork 1A; `DECISIONS.md` → "DRAFT:
/// Phase 46 opens…"): a `Box`, or a `Cylinder` standing on its axis. The
/// `w`/`l`/`x`/`y` on `Stock` are the plan BOUNDING BOX for every shape — a
/// Cylinder's `w` and `l` are its `dia`, its `x`/`y` the box's datum corner,
/// converted at parse from the AXIS the line declares — so every reader that
/// wants an extent (the raster's rows, the voxel domain, the camera's fit)
/// reads the box unchanged, and every reader that wants the FOOTPRINT reads
/// `derive_blank`, the one home. A square blank around a round part was
/// measured as a declaration the vocabulary does not mean (the DRAFT's
/// premise 7): the stock model held the corners as steel and refused the
/// first wall op near the rim by name.
#[derive(Clone, Debug, PartialEq)]
pub enum StockShape {
    Box,
    Cylinder { dia: f64 },
    /// A blank whose plan footprint is a closed loop of points (Phase 54
    /// slice 5a, fork 3A — the model-based stock's noun): `stock
    /// Profile(h=, z=)` with the points on the lines below, wound CCW as a
    /// pocket's are. The struct keeps the loop's bounding box in `w`, `l`,
    /// `x`, `y`, so every extent reader reads it unchanged; `derive_blank`
    /// answers the loop itself. The sidecar proposes one from a STEP
    /// (slice 5b); an author may type one.
    Profile { points: Vec<Pt> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stock {
    pub shape: StockShape,
    pub w: f64,
    pub l: f64,
    pub h: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// What the stock is made OF — the workpiece axis of the feeds-table
    /// lookup (DECISIONS.md → "K3, the material…", fork 2). Optional:
    /// undeclared means today's behavior byte for byte, and only an op that
    /// asks for a derived F/S word (`feed=derive` / `spindle=derive`) ever
    /// reads it. Names match table records exactly — no aliasing, no fuzz.
    pub material: Option<String>,
}

/// A declared **keep-out**: workholding the tool must avoid and no operation
/// ever cuts (ROADMAP T2 → "Workholding models for clearance checks";
/// `DECISIONS.md` → "V2, the fixture…").
///
/// Its nearest relative is the STOCK, not a `Feature` — like the stock it is a
/// passive body the program reasons about, and unlike every `Feature` no
/// removal law touches it. That is why it lives here beside `Stock` rather
/// than as an enum arm over there.
///
/// **A fixture may overlap the stock, and that is not a bug** (fork 7): a clamp
/// that touches nothing holds nothing. It takes no part in containment, in
/// `removals_for`, or in any accounting of what the stock still holds — it is
/// an obstacle for the TOOL and nothing else.
#[derive(Clone, Debug, PartialEq)]
pub struct Fixture {
    pub name: String,
    /// Datum corner (Box) or axis foot (Cylinder), in the program's frame —
    /// the same convention `stock Box` uses.
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub shape: FixtureShape,
}

/// v1's two shapes (fork 2A, the optimizer's own scope cut): a vise, a jaw, a
/// clamp foot, a standoff and a chuck are all boxes and cylinders. Meshes are
/// the named second slice — they would forfeit the closed form, and with it
/// the brake.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FixtureShape {
    /// Axis-aligned, like the stock.
    Box { w: f64, l: f64, h: f64 },
    /// Axis vertical, foot at `z`.
    Cylinder { dia: f64, h: f64 },
}

impl Fixture {
    /// The keep-out's vertical extent, `(z_lo, z_hi)`.
    pub fn z_span(&self) -> (f64, f64) {
        let h = match self.shape {
            FixtureShape::Box { h, .. } | FixtureShape::Cylinder { h, .. } => h,
        };
        (self.z, self.z + h)
    }

    /// The footprint's four plan corners (a box) or its centre and radius
    /// (a cylinder), for the exact segment-distance solve.
    pub fn plan_corners(&self) -> Option<[(f64, f64); 4]> {
        match self.shape {
            FixtureShape::Box { w, l, .. } => Some([
                (self.x, self.y),
                (self.x + w, self.y),
                (self.x + w, self.y + l),
                (self.x, self.y + l),
            ]),
            FixtureShape::Cylinder { .. } => None,
        }
    }

    /// Plan distance from `(px, py)` to the keep-out's footprint — zero inside
    /// it. Closed form for both shapes, which is what lets this check gate
    /// where a sampled probe never could.
    pub fn plan_distance(&self, px: f64, py: f64) -> f64 {
        match self.shape {
            FixtureShape::Box { w, l, .. } => {
                let dx = (self.x - px).max(px - (self.x + w)).max(0.0);
                let dy = (self.y - py).max(py - (self.y + l)).max(0.0);
                (dx * dx + dy * dy).sqrt()
            }
            FixtureShape::Cylinder { dia, .. } => {
                let d = ((px - self.x).powi(2) + (py - self.y).powi(2)).sqrt();
                (d - dia / 2.0).max(0.0)
            }
        }
    }
}

impl Default for Stock {
    fn default() -> Self {
        Stock { shape: StockShape::Box, w: 0.0, l: 0.0, h: 0.0, x: 0.0, y: 0.0, z: 0.0, material: None }
    }
}

impl Stock {
    /// The footprint's centre in plan — a Cylinder's axis, a Box's middle.
    pub fn axis(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.l / 2.0)
    }
}

/// Which way the part was turned between the earlier setup and this one —
/// the `up=` word of `given from` (Phase 48 slice 1). The words and their
/// meaning are the sidecar's own table, `import_step.py`'s `UP_ROTATIONS`:
/// which MODEL axis is up on the machine, as the one rotation about the
/// ORIGIN that carries it onto +Z. A deliberate twin across the language
/// boundary, like `CHORD_TOL_MM`: an author who imported the second setup
/// with `--up z-` says `up=z-` here and means the same frame.
///
/// Only the two turns that keep a column a column are carried: `z+` (no
/// turn — a second program on the same side) and `z-` (the part turned
/// over, 180° about X: (x, y, z) → (x, −y, −z)). The four quarter turns are
/// in the table and are declined by name: a quarter turn makes a vertical
/// void a horizontal one, which the stock model's section-between-heights
/// `Removal` cannot hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UpAxis {
    ZPlus,
    ZMinus,
}

impl UpAxis {
    pub fn from_word(word: &str) -> Result<UpAxis, String> {
        match word {
            "z+" => Ok(UpAxis::ZPlus),
            "z-" => Ok(UpAxis::ZMinus),
            "x+" | "x-" | "y+" | "y-" => Err(format!(
                "up={word} is a quarter turn — a vertical void turned a quarter becomes a horizontal one, which the stock model holds no section for; only up=z- (the part turned over) and up=z+ (the same side) are read"
            )),
            _ => Err(format!(
                "up={word} is not an axis word — the sidecar's own: z+, z- (and x+, x-, y+, y-, which a program cannot read yet)"
            )),
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            UpAxis::ZPlus => "z+",
            UpAxis::ZMinus => "z-",
        }
    }
}

/// The program before this one, as `given from <project> up=<axis>` says
/// it (Phase 48 slice 1): loaded by path through the one `load_project`
/// home at parse — and so parsed, converted to mm and validated as its own
/// program — and held whole, beside what this program reads from it: its
/// closed-form removals turned into this frame, `stock::turn_program`'s
/// answer, derived from the one home every reader of a program's
/// removals uses, ONCE, here (slice 6, ruling 2 of the Phase 48 CLOSE —
/// a stored derived value, asked and ruled: the `Part` it is derived
/// from is frozen in this same struct and nothing can move either).
#[derive(Clone, Debug, PartialEq)]
pub struct GivenFrom {
    /// The path as written, for every sentence that names the project.
    pub project: String,
    /// Where it was found — as written first, then against this project's
    /// directory, the `.spline` and `.patch` rule.
    pub resolved: std::path::PathBuf,
    pub up: UpAxis,
    /// Where the earlier program's zero sits in this program's frame once
    /// the part is turned, `at=x,y,z`: a point there is turned as `up` says
    /// and then moved by this. Zeros when the word is not written, which is
    /// the two setups sharing one origin.
    pub at: [f64; 3],
    pub part: Box<Part>,
    /// The earlier program's air in this frame, read by
    /// `stock::given_from_air` and never re-derived.
    pub air: crate::stock::GivenFromAir,
}

// How deep in `given from` the parser is: 0 for the program the user named,
// 1 while the earlier project loads. A chain — the earlier project itself
// saying `given from` — refuses at parse (DECISIONS.md → "Phase 48 slice
// 1…": a third setup is not in the phase, and a cycle A→B→A would otherwise
// load forever). A thread-local because `parse_into`'s signature is the
// grammar's one entry and gains no argument for a guard.
thread_local! {
    pub(super) static GIVEN_FROM_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The blank's plan footprint, the one home (Phase 46 slice 1): a Box's four
/// corners, CCW from the datum; a Cylinder's circle, chorded through the one
/// chord-tolerance home every drawn circle goes through. `grow` pushes the
/// footprint out by that much — the offset family's wall round a `Plane` is
/// the blank grown a diameter, and a round blank grown is a bigger circle,
/// never a box. Every reader of the footprint calls this; none restates the
/// four corners (the pre-slice state: six copies in six files).
pub fn derive_blank_grown(s: &Stock, grow: f64) -> Vec<Pt> {
    match s.shape {
        StockShape::Box => vec![
            Pt { x: s.x - grow, y: s.y - grow },
            Pt { x: s.x + s.w + grow, y: s.y - grow },
            Pt { x: s.x + s.w + grow, y: s.y + s.l + grow },
            Pt { x: s.x - grow, y: s.y + s.l + grow },
        ],
        StockShape::Cylinder { dia } => {
            let (cx, cy) = s.axis();
            crate::ops::finish::circle_pts(cx, cy, dia / 2.0 + grow).into_iter().map(|(x, y)| Pt { x, y }).collect()
        }
        // The loop as declared; grown, the offsetting crate's outer loop,
        // flattened through the one chord tolerance like every drawn arc.
        // The crate's convention is the pocket's: a positive offset of a
        // CCW loop moves INWARD (the tool into the void), so a blank grows
        // by the negative — found by the pin, which asked for more area
        // and got less.
        StockShape::Profile { ref points } => {
            if grow.abs() <= 1e-12 {
                return points.clone();
            }
            // Loops of one area: the first the offsetter gave.
            crate::tie::first_greatest(crate::offset::offset_closed_polyline_verts(points, -grow).into_iter().map(|lp| crate::offset::flatten_loop(&lp)), |lp| signed_area(lp).abs())
                .unwrap_or_else(|| points.clone())
        }
    }
}

/// The blank's plan footprint as declared — `derive_blank_grown` by nothing.
pub fn derive_blank(s: &Stock) -> Vec<Pt> {
    derive_blank_grown(s, 0.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    // Input-units flag, parser bookkeeping only. After parse_project() every
    // value in Part is millimeters (inch inputs are converted at the parse
    // boundary — see DECISIONS.md "Internal units are mm, always"), so this
    // is always true by the time an op or the writer sees the Part.
    pub units_mm: bool,
    pub global_feed: f64,
    pub global_plunge: f64,
    pub global_rapid_z: f64,
    pub global_clearance_z: f64,
    pub global_spindle: f64,
    pub stock: Stock,
    pub tools: std::collections::HashMap<usize, Tool>,
    pub features: std::collections::HashMap<String, Feature>,
    pub operations: Vec<Operation>,
    // How the engagement linter's findings are voiced: stderr advisories
    // (default) or an error at validate. Set by the top-level `engagement`
    // directive.
    pub engagement: EngagementMode,
    // How the plunge taxonomy's entry flag is voiced: stderr advisories
    // (default) or an error at generation. Set by the top-level `entry`
    // directive.
    pub entry: EntryMode,
    pub deviation: DeviationMode,
    /// The shop's feeds table, declared in program text via `feeds_table
    /// "<path>"` (DECISIONS.md → "K3, the material…", fork 1). A shop record
    /// shadows the shipped record WHOLE at (material × tool type)
    /// granularity — never field-merged — so what wins is readable by eye.
    /// None = the shipped `tables/starter.feeds` alone.
    pub shop_feeds: Option<FeedsTable>,
    /// How loudly the keep-out check speaks — `fixture advise|gate`.
    pub fixture_mode: FixtureMode,
    /// How loudly the reach certificate speaks — `reach advise|gate`.
    pub reach_mode: ReachMode,
    /// How loudly the machine envelope speaks — `envelope advise|gate`,
    /// the sixth directive (Phase 15 slice 4).
    pub envelope_mode: EnvelopeMode,
    /// Declared keep-outs (`DECISIONS.md` → "V2, the fixture…"). Empty is
    /// today's behaviour byte for byte: with nothing declared there is nothing
    /// to avoid and the check never speaks.
    pub fixtures: Vec<Fixture>,
    /// The voids that are there BEFORE this program starts — `given <name>`,
    /// one per line, in declaration order (Phase 42 slice 1; `DECISIONS.md`
    /// → "Phase 42 slice 1…"). A program fact beside `fixture`, not a key on
    /// the noun: a bore cut on another setup is air to this one, and the
    /// stock model subtracts it at prefix 0 so every certificate reads it
    /// from the one home. Empty is today's behaviour byte for byte.
    pub given: Vec<String>,
    /// The program BEFORE this one — `given from <project> up=<axis>`, at
    /// most once (Phase 48 slice 1; `DECISIONS.md` → "Phase 48 slice 1…",
    /// fork 1A of the DRAFT): the earlier setup's project, loaded by path
    /// and turned by the sidecar's own table, whose every closed-form
    /// removal is air at this program's prefix 0. `given`'s second arm, the
    /// `fixture advise|gate` precedent — one word, two jobs, told apart by
    /// a reserved word. `None` is today's behaviour byte for byte.
    pub given_from: Option<GivenFrom>,
    // The declared machine, from the one `machine <file>` line. `None` is the
    // undeclared machine — today's exact output, byte for byte (DECISIONS.md
    // "K1, the machine…"); only `gcode`'s per-word-class formatters read it.
    pub machine: Option<Machine>,
    /// The program's datum SELECTION (DECISIONS.md → "work offsets…",
    /// fork 1A): which pre-defined machine register the one coordinate
    /// space rides. It moves nothing — every certificate and stamp gauges
    /// the program's own space unchanged, and the emitted difference is one
    /// word on the setup line. `None` = `G54`, today's spelling.
    pub work_offset: Option<WorkOffset>,
    /// Where the program's one register POINTS — the machine coordinates of
    /// the work XY zero, `datum x= y=` (Phase 15 slice 1, DECISIONS.md →
    /// "Phase 15 opens: the envelope DRAFT…", fork 1). A declaration the
    /// certificates read, never a word the controller hears: `G92`/`G10`/
    /// `G52` stay rejected, and nothing about emission changes — this is
    /// the fixture declaration's precedent, setup geometry declared where
    /// the checks can see it. `None` = no claim, and the travel check that
    /// needs it declines by name rather than guessing. No z on purpose —
    /// fork 1's Z ruling; internal mm like every parsed length.
    pub datum: Option<(f64, f64)>,
    /// Every file this project was read from, in the order they were
    /// opened: the part file(s) first, then each external the parse
    /// resolved (a machine file, a feeds table, a tool profile). The window
    /// watches exactly this list to follow the file (`DESIGN.md` §1); a
    /// project parsed from text alone lists nothing.
    pub sources: Vec<std::path::PathBuf>,
    /// Where each PARSED operation's `run` line is: (index into `sources`
    /// — 0 the main text, 1 `geometry.part` — and the 0-based line), one
    /// entry per `run` statement in program order. A table beside
    /// `operations` rather than a field on `Operation`, because operations
    /// are also synthesized in code with no line to name: `op_lines.get(i)`
    /// is `None` for those, honestly. The editing workspace's writes read
    /// it (`DESIGN.md` §1, the one write).
    pub op_lines: Vec<(usize, usize)>,
    /// Each parsed `run` line's own text, parallel to `op_lines` — what the
    /// inspector's fields are read from (`DESIGN.md` §5.4), so no field
    /// is ever recovered from a rendered sentence (I7).
    pub op_texts: Vec<String>,
    /// Where each `feature` statement starts: (name, source index, 0-based
    /// line, that line's text). A block statement records its first line;
    /// its arguments span lines and the field editor refuses it by name.
    pub feature_lines: Vec<(String, usize, usize, String)>,
    /// Where each `tool` declaration is: (number, source index, line, text).
    pub tool_lines: Vec<(usize, usize, usize, String)>,
    /// Where the `stock` statement is: (source index, 0-based line, that
    /// line's text) — the last one parsed, which is the one `stock`
    /// holds. `None` for a project with no stock line (the default
    /// stock), which the window's form declines by name rather than
    /// inventing a line to edit (Phase 22 slice 1).
    pub stock_line: Option<(usize, usize, String)>,
    /// Where each header word's line is, for the window's form (Phase 50
    /// slice 3): `(key, file, line, text)`, the stock line's shape.
    pub header_lines: Vec<(String, usize, usize, String)>,
}

impl Default for Part {
    fn default() -> Self {
        Part {
            units_mm: true,
            global_feed: 0.0,
            global_plunge: 0.0,
            global_rapid_z: 0.0,
            global_clearance_z: 0.0,
            global_spindle: 0.0,
            stock: Stock::default(),
            tools: std::collections::HashMap::new(),
            features: std::collections::HashMap::new(),
            operations: Vec::new(),
            engagement: EngagementMode::Advise,
            entry: EntryMode::Advise,
            deviation: DeviationMode::Advise,
            shop_feeds: None,
            fixture_mode: FixtureMode::Advise,
            reach_mode: ReachMode::Advise,
            envelope_mode: EnvelopeMode::Advise,
            fixtures: Vec::new(),
            given: Vec::new(),
            given_from: None,
            machine: None,
            work_offset: None,
            datum: None,
            sources: Vec::new(),
            op_lines: Vec::new(),
            op_texts: Vec::new(),
            feature_lines: Vec::new(),
            tool_lines: Vec::new(),
            stock_line: None,
            header_lines: Vec::new(),
        }
    }
}
