// `validate`: everything a declared part must satisfy before any op emits, plus the header law and the gates.

use super::*;

/// **The corner shortfall** — where a polyline's round is not the round that was
/// declared, because the tool could not get in there.
///
/// A circle has no corners, so slice 1 had nothing to say here. A polyline does,
/// and at a plan corner tighter than the tool the axis path pivots on the offset
/// vertex and never reaches the perpendicular stand-off from the corner itself.
/// The corner comes out under-broken, or on a sharp enough one, not broken at
/// all — and it comes out that way in every CAM system, silently. The number is
/// `dist(corner, path) - the offset the axis rides`: how much further from the
/// path the corner sits than a wall point the tool actually rides against.
///
/// Only the WORST corner is named, with a count beside it. A finding nobody
/// reads is not a finding, and a twelve-sided pocket would otherwise report
/// twelve lines saying one thing.
pub(super) fn edge_round_corner_finding(er: &EdgeRound, tool: &Tool, wall_standoff: f64) -> Option<(String, [f64; 2])> {
    let EdgeLoop::Polyline { points } = &er.edge else { return None };
    let arc = tool.ground_arc()?;
    let reach = er.axis_offset(arc.effective_radius(), wall_standoff);
    let (gap, at, n) = polyline_corner_shortfall(points, reach)?;
    // Past the arc centre's own stand-off the arc never touches the corner at
    // all: what is left there is the sharp edge, not a shallower round.
    let unbroken = gap + reach >= arc.rc;
    Some((format!(
        "corner shortfall {:.4}mm at ({:.3}, {:.3}){} — {} corner{} the {:.3}mm round does not reach (the axis path pivots short of them)",
        gap, at.x, at.y,
        if unbroken { ", left sharp" } else { "" },
        n, if n == 1 { "" } else { "s" }, er.radius
    ), [at.x, at.y]))
}

/// The geometric half of the corner shortfall, shared by both forms: how much
/// further from the axis path each authored corner sits than a wall point the
/// tool actually rides against, as `(worst gap, its corner, count)`. Each form
/// formats its own message and judges its own "left sharp" bound, because
/// where the form runs out is the form's own business.
pub(super) fn polyline_corner_shortfall(points: &[Pt], reach: f64) -> Option<(f64, Pt, usize)> {
    let path = derive_edge_round_path(points, reach).ok()?;
    let flat = derive_edge_round_path_points(&path);

    let mut worst: Option<(f64, Pt)> = None;
    let mut n = 0usize;
    for v in points {
        let gap = crate::stock::dist_to_polygon(&flat, v.x, v.y) - reach;
        // Below a micron the corner is a corner the tool rides right into; the
        // tolerance is the chord budget the flattening above already spends.
        if gap <= crate::CHORD_TOL_MM {
            continue;
        }
        n += 1;
        if worst.map_or(true, |(g, _)| gap > g) {
            worst = Some((gap, *v));
        }
    }
    let (gap, at) = worst?;
    Some((gap, at, n))
}

/// The chamfer's corner shortfall — the round's contributor, one form over,
/// and arguably worse on a chamfer: an unbroken corner on a beveled edge is
/// more visible than on a blended one.
pub(super) fn chamfer_corner_finding(ec: &EdgeChamfer, pl: &ChamferPlacement) -> Option<(String, [f64; 2])> {
    let EdgeLoop::Polyline { points } = &ec.edge else { return None };
    let (gap, at, n) = polyline_corner_shortfall(points, pl.reach)?;
    // Past the flank's outer contact the cone never touches the corner at all.
    let unbroken = gap + pl.reach >= pl.r_contact + pl.width;
    Some((format!(
        "corner shortfall {:.4}mm at ({:.3}, {:.3}){} — {} corner{} the {:.3}mm chamfer does not reach (the axis path pivots short of them)",
        gap, at.x, at.y,
        if unbroken { ", left sharp" } else { "" },
        n, if n == 1 { "" } else { "s" }, pl.depth
    ), [at.x, at.y]))
}

/// Everything a `Finish` with a `CornerRounder` has to satisfy. The op's whole
/// rule set lives here, which is why the caller `continue`s past the generic
/// finishing checks: none of them are about this pass — there is no ball nose
/// to require, no scallop to space, and no wall to ladder down.
pub(super) fn check_edge_round_op(
    p: &Part,
    op: &Operation,
    feature: &Feature,
    tool: &Tool,
    rd: &Rounder,
) -> Result<(), String> {
    let f = &op.feature_name;
    // The prismatic pairs, circular and polyline, carry an edge round. What is
    // still out is every edge this op's placement cannot derive: a non-planar
    // one (the 3D wires — the derivation assumes ONE Z) and the curved surfaces,
    // which have no top edge of their own to break.
    let Some(er) = derive_edge_round(p, feature) else {
        return Err(match feature {
            Feature::CylinderBore { .. }
            | Feature::CylinderBoss { .. }
            | Feature::ProfilePocket { .. }
            | Feature::ProfileBoss { .. } => format!(
                "Form operation feature '{f}' is cut by T{}, a CornerRounder, but declares no edge_round: a corner rounder cuts a declared round, so add edge_round={:.3} to the feature (or use an endmill to cut the wall)",
                op.tool_num, rd.radius
            ),
            Feature::ContourPocket { .. } | Feature::ContourBoss { .. } => format!(
                "Form operation feature '{f}' is cut by T{}, a CornerRounder, but a Contour wire's edge is not planar — this op places the round at ONE Z, and a wire that rises and falls has a different one at every point",
                op.tool_num
            ),
            _ => format!(
                "Form operation feature '{f}' is cut by T{}, a CornerRounder, but only the prismatic pairs (CylinderBore/CylinderBoss, ProfilePocket/ProfileBoss) carry an edge_round — a curved surface has no top edge of its own to break",
                op.tool_num
            ),
        });
    };

    // The pairing error, and it reads the NOMINAL radius: a form cutter cuts
    // its own radius or the wrong one. `radius_error` is deliberately not in
    // play here — a 3mm rounder that ground two hundredths small is still a 3mm
    // rounder, and that difference is a deviation to report, not a mismatch to
    // refuse (DECISIONS.md → "the corner rounder gets an operation").
    if (rd.radius - er.radius).abs() > 1e-9 {
        return Err(format!(
            "Form operation feature '{f}' declares edge_round={:.3}mm but T{} grinds a {:.3}mm arc: a form cutter cuts its own radius or the wrong one, so there is no scaling and no closest fit — declare the round the tool actually has, or pick the tool the round actually needs",
            er.radius, op.tool_num, rd.radius
        ));
    }

    let extent = match feature {
        Feature::CylinderBore { extent, .. }
        | Feature::CylinderBoss { extent, .. }
        | Feature::ProfilePocket { extent, .. }
        | Feature::ProfileBoss { extent, .. } => *extent,
        _ => unreachable!("derive_edge_round answered for this feature"),
    };
    if er.radius >= extent {
        return Err(format!(
            "Form operation feature '{f}': the {:.3}mm edge round is as deep as the {:.3}mm wall it lives on — a round consumes its own radius of wall, so there would be nothing left of the flank below it",
            er.radius, extent
        ));
    }
    let reach = tool.ground_arc().expect("a rounder has an arc").effective_radius() + op.wall_standoff;
    match &er.edge {
        EdgeLoop::Circle { r: wall_radius, .. } => {
            if er.boss && er.radius >= *wall_radius {
                return Err(format!(
                    "Form operation feature '{f}': the {:.3}mm edge round is at or beyond the boss's own {:.3}mm radius — the round's centre of curvature would fall on or through the axis",
                    er.radius, wall_radius
                ));
            }
            // The tool has to fit down the hole it is breaking the lip of. Read
            // off the EFFECTIVE radius, because that is where the axis rides.
            if !er.boss && reach > *wall_radius {
                return Err(format!(
                    "Form operation feature '{f}': T{}'s effective radius ({:.3}mm) plus its wall standoff ({:.3}mm) exceeds the {:.3}mm bore — the axis would have to ride outside the hole it is cutting in",
                    op.tool_num, reach - op.wall_standoff, op.wall_standoff, wall_radius
                ));
            }
        }
        // The polyline's twin of both circular checks at once, and it is one
        // question rather than two because a polyline has no radius to compare
        // against: can the axis ride the void side at all? A pocket too narrow
        // for the tool and a boss whose plan corners are tighter than it both
        // come back as the offset failing, which is the same error the wall
        // trace already makes for the same geometry.
        EdgeLoop::Polyline { points } => {
            if let Err(why) = derive_edge_round_path(points, reach) {
                return Err(format!(
                    "Form operation feature '{f}': T{}'s axis path {} — the round cannot be placed where the tool cannot ride. Open the plan corners, or pick a rounder whose effective radius fits",
                    op.tool_num, why
                ));
            }
        }
    }

    // Nothing to space and nothing to ladder: the form is stamped, not
    // approximated. These reject rather than riding ignored.
    for (name, present) in [
        ("stepover", op.stepover.is_some()),
        ("scallop", op.scallop.is_some()),
    ] {
        if present {
            return Err(format!(
                "Form operation feature '{f}' with a CornerRounder takes no {name}: the round is stamped by the cutting edge in ONE pass, not approximated by spaced passes, so there is nothing to space"
            ));
        }
    }
    if op.stock_to_leave.is_some() {
        return Err(format!(
            "Form operation feature '{f}' with a CornerRounder takes no stock_to_leave: on a form cut an allowance means cutting a different radius than the one declared, and a deliberate offset belongs on wall_standoff/face_standoff — which are the single home for it, so that no offset can be applied twice"
        ));
    }
    for (name, v) in [("wall_standoff", op.wall_standoff), ("face_standoff", op.face_standoff)] {
        if v < 0.0 {
            return Err(format!(
                "Form operation feature '{f}': {name} ({v:+.3}mm) must not be negative — a standoff backs the tool AWAY from the surface it is not to touch; a negative one would drive it in"
            ));
        }
    }
    Ok(())
}

/// Everything a `Finish` with a `ChamferMill` has to satisfy — the chamfer's
/// twin of `check_edge_round_op`, and the same shape: the whole rule set in
/// one place, the caller `continue`s past every generic finishing check.
pub(super) fn check_chamfer_op(
    p: &Part,
    op: &Operation,
    feature: &Feature,
    tool: &Tool,
    ch: &Chamfer,
) -> Result<(), String> {
    let f = &op.feature_name;
    let Some(ec) = derive_chamfer(p, feature) else {
        return Err(match feature {
            Feature::CylinderBore { .. }
            | Feature::CylinderBoss { .. }
            | Feature::ProfilePocket { .. }
            | Feature::ProfileBoss { .. } => format!(
                "Form operation feature '{f}' is cut by T{}, a ChamferMill, but declares no chamfer: a chamfer mill cuts a declared break, so add chamfer_depth= (the drop down the wall) or chamfer_width= (the leg across the face) to the feature",
                op.tool_num
            ),
            Feature::ContourPocket { .. } | Feature::ContourBoss { .. } => format!(
                "Form operation feature '{f}' is cut by T{}, a ChamferMill, but a Contour wire's edge is not planar — this op places the form at ONE Z, and a wire that rises and falls has a different one at every point",
                op.tool_num
            ),
            _ => format!(
                "Form operation feature '{f}' is cut by T{}, a ChamferMill, but only the prismatic pairs (CylinderBore/CylinderBoss, ProfilePocket/ProfileBoss) carry a chamfer — a curved surface has no top edge of its own to break",
                op.tool_num
            ),
        });
    };

    // The pairing error reads the ANGLE, not the size — a form cutter cuts
    // its own angle or the wrong one — and it reads the NOMINAL angle, the
    // radius-pairing rule one axis over: a 90-degree mill that ground a
    // hundredth off is still a 90-degree mill, and that difference is a
    // deviation to report, not a mismatch to reject.
    if let Some(a) = ec.spec.angle {
        if (a - ch.angle).abs() > 1e-9 {
            return Err(format!(
                "Form operation feature '{f}' declares chamfer_angle={:.3}deg but T{} grinds a {:.3}deg cone: a form cutter cuts its own angle or the wrong one, so there is no scaling and no closest fit — declare the angle the tool actually has, or pick the tool the drawing actually needs",
                a, op.tool_num, ch.angle
            ));
        }
    }

    if let Some(tc) = op.tip_clearance {
        if tc <= 0.0 {
            return Err(format!(
                "Form operation feature '{f}': tip_clearance ({tc:+.3}mm) must be positive — zero is the deepest-legal placement, where a tip-flat measurement error puts the flat straight into the wall, and negative puts the tip IN the material by declaration. Omit the field for the derived default"
            ));
        }
    }

    let cone = tool.ground_cone().expect("a ChamferMill has a cone");
    let pl = derive_chamfer_placement(&ec, &cone, derive_tip_clearance(op, ch), op.wall_standoff, op.face_standoff);
    // The deepest leg on the loop governs the size checks below when the
    // points restate it (slice 9); without marks every edge is the feature's.
    let legs = derive_chamfer_legs(&ec);
    let pl = legs.iter().flatten()
        .map(|l| derive_leg_placement(*l, ec.z_top, &cone, derive_tip_clearance(op, ch), op.wall_standoff, op.face_standoff))
        .fold(pl, |a, b| if b.depth > a.depth { b } else { a });
    if op.comp == CompMode::Control && legs.iter().any(|l| l.is_none()) {
        return Err(format!(
            "Form operation feature '{f}': a loop point leaves its edges unbroken (chamfer_depth=0) under comp=control, where the compensated pass is one loop — give every edge a leg, or use engine comp, which lifts off the unbroken edges"
        ));
    }

    let extent = match feature {
        Feature::CylinderBore { extent, .. }
        | Feature::CylinderBoss { extent, .. }
        | Feature::ProfilePocket { extent, .. }
        | Feature::ProfileBoss { extent, .. } => *extent,
        _ => unreachable!("derive_chamfer answered for this feature"),
    };
    if pl.depth > extent + 1e-9 {
        return Err(format!(
            "Form operation feature '{f}': the {:.3}mm chamfer depth is deeper than the {:.3}mm wall it lives on — the bevel may take the whole wall, not more",
            pl.depth, extent
        ));
    }

    // The flank has to hold the whole face plus the clearance below it: past
    // the body OD the cone runs out into shank. A face wider than that is
    // cut in steps (`derive_chamfer_passes`), where steps can be laid out.
    let passes = derive_chamfer_passes(&ec, &cone, derive_tip_clearance(op, ch));
    if pl.r_contact >= cone.body_r {
        return Err(format!(
            "Form operation feature '{f}': tip flat {:.3}/2 + tip_clearance {:.3} puts the contact {:.3}mm off the axis, at or past T{}'s body radius {:.3}mm — no flank is left to cut with. Use a smaller tip_clearance",
            ch.tip_dia, pl.r_contact - ch.tip_dia / 2.0, pl.r_contact, op.tool_num, cone.body_r
        ));
    }
    if passes.len() > 1 {
        let wide = format!(
            "the face's outer contact ({:.3}mm off the axis — tip flat {:.3}/2 + tip_clearance {:.3} + width {:.3}) runs past T{}'s usable flank (body radius {:.3}mm), so the chamfer is cut in steps",
            pl.r_contact + pl.width, ch.tip_dia, pl.r_contact - ch.tip_dia / 2.0, pl.width, op.tool_num, cone.body_r
        );
        if op.comp == CompMode::Control {
            return Err(format!(
                "Form operation feature '{f}': {wide}, each riding its own offset, and under comp=control a register holds one number — use engine comp, a larger mill, or a smaller chamfer"
            ));
        }
        let cut: Vec<ChamferLeg> = legs.iter().flatten().copied().collect();
        if cut.windows(2).any(|w| w[0] != w[1]) {
            return Err(format!(
                "Form operation feature '{f}': {wide}, and the loop's points restate the leg to more than one size — steps are laid out for one leg on the edges that carry it, with 0 on the rest. Give the deep edge its own feature, or use a larger mill"
            ));
        }
    }

    match &ec.edge {
        EdgeLoop::Circle { r: wall_radius, .. } => {
            if ec.boss && pl.width >= *wall_radius {
                return Err(format!(
                    "Form operation feature '{f}': the {:.3}mm chamfer width is at or beyond the boss's own {:.3}mm radius — the face's top corner would fall on or through the axis",
                    pl.width, wall_radius
                ));
            }
            if !ec.boss && pl.reach > *wall_radius {
                return Err(format!(
                    "Form operation feature '{f}': T{}'s tip flat plus tip_clearance plus wall standoff ({:.3}mm) exceeds the {:.3}mm bore — the axis would have to ride outside the hole it is cutting in",
                    op.tool_num, pl.reach, wall_radius
                ));
            }
        }
        EdgeLoop::Polyline { points } => {
            // Every pass's own path: a stepped pass rides further in, over
            // the face it is cutting.
            for pass in &passes {
                if let Err(why) = derive_edge_round_path(points, pl.reach - pass.slide) {
                    return Err(format!(
                        "Form operation feature '{f}': T{}'s axis path {} — the chamfer cannot be placed where the tool cannot ride. Open the plan corners, or reduce the tip_clearance",
                        op.tool_num, why
                    ));
                }
            }
        }
    }

    for (name, present) in [
        ("stepover", op.stepover.is_some()),
        ("scallop", op.scallop.is_some()),
    ] {
        if present {
            return Err(format!(
                "Form operation feature '{f}' with a ChamferMill takes no {name}: the face is stamped by the cutting flank, in one pass or in the steps the flank's own length sets, not approximated by spaced passes, so there is nothing to space"
            ));
        }
    }
    if op.stock_to_leave.is_some() {
        return Err(format!(
            "Form operation feature '{f}' with a ChamferMill takes no stock_to_leave: on a form cut an allowance means cutting a different face than the one declared, and a deliberate offset belongs on wall_standoff/face_standoff — which are the single home for it, so that no offset can be applied twice"
        ));
    }
    for (name, v) in [("wall_standoff", op.wall_standoff), ("face_standoff", op.face_standoff)] {
        if v < 0.0 {
            return Err(format!(
                "Form operation feature '{f}': {name} ({v:+.3}mm) must not be negative — a standoff backs the tool AWAY from the surface it is not to touch; a negative one would drive it in"
            ));
        }
    }
    Ok(())
}

/// One register, one number. Under control comp the standoff folds into the D
/// value the operator dials, which is what keeps it from also living in the
/// path — but a register holds one number, so two control ops on the same
/// rounder asking for two different values is unsatisfiable, and saying so is
/// better than emitting a program whose header contradicts one of them.
pub(super) fn check_edge_round_registers(p: &Part) -> Result<(), String> {
    let mut seen: std::collections::BTreeMap<usize, (f64, String)> = Default::default();
    for op in &p.operations {
        if !op.follows_surface() || op.comp != CompMode::Control {
            continue;
        }
        let Some(tool) = p.tools.get(&op.tool_num) else { continue };
        // What folds into D differs per form — the rounder folds its wall
        // standoff, the chamfer folds tip_clearance + wall standoff (the
        // resolved slide along its own flank) — but the law is one law: a
        // register holds one number.
        let (folded, what) = if tool.rounder.is_some() {
            (op.wall_standoff, "wall standoff")
        } else if let Some(ch) = &tool.chamfer {
            (derive_tip_clearance(op, ch) + op.wall_standoff, "tip_clearance + wall standoff")
        } else {
            continue;
        };
        match seen.get(&op.tool_num) {
            None => {
                seen.insert(op.tool_num, (folded, op.feature_name.clone()));
            }
            Some((first, first_feature)) if (first - folded).abs() > 1e-9 => {
                return Err(format!(
                    "two comp=control form passes with T{} ask for different D{} values: '{}' folds {:.3}mm of {what} into the register and '{}' folds {:.3}mm — one register holds one number, so split the tool or match the offsets",
                    op.tool_num, derive_d(op.tool_num), first_feature, first, op.feature_name, folded
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// The D value an edge-round pass needs dialled in: the diameter of consequence
/// plus twice the programmer's standoff. **Not** the body OD and not the pilot.
/// One home, because the emitter prints it and the op positions by it.
pub fn derive_edge_round_comp_dia(rd: &Rounder, wall_standoff: f64) -> f64 {
    rd.effective_dia + 2.0 * wall_standoff
}

/// Field-shape (dimensional) invariants of the standing revolves — one home,
/// read by BOTH consuming ops. These lived inline in `validate`'s Finish arm
/// until the revolve `Clear` became the family's second validating consumer
/// (DECISIONS.md → "rim-area accounting…", slice 2): running FIRST in a
/// contained fixture's op list, the Clear derived a degenerate feature's
/// offset meridian before the Finish arm could reject it, and the NaN
/// meridian panicked the sort in `ascending` — a crash where a named error
/// belonged. Only the feature-against-itself checks live here; tool-fit
/// constraints stay with the op that owns them (the Finish arm's bore-vs-tool
/// check, the runtime gouge guards in `ops::finish`).
/// A `Face`'s loop law (Phase 28, fork 1A): at least an outer boundary; every
/// loop at least three points; the first wound CCW and the rest CW — refused,
/// never normalized, as every profile's winding is (the row-2.4 law: reversing
/// a traversal flips climb and conventional on the machine); and each inner
/// loop strictly inside the outer and clear of every other.
pub(super) fn validate_face_loops(name: &str, loops: &[Vec<Pt>]) -> Result<(), String> {
    let signed = |l: &[Pt]| (0..l.len()).map(|i| l[i].x * l[(i + 1) % l.len()].y - l[(i + 1) % l.len()].x * l[i].y).sum::<f64>();
    if loops.is_empty() {
        return Err(format!("Face feature '{name}' has no boundary — open its outer loop with a `loop` line and give it points"));
    }
    for (i, l) in loops.iter().enumerate() {
        if l.len() < 3 {
            return Err(format!("Face feature '{name}': loop {} has {} point{} — a boundary is at least three", i + 1, l.len(), if l.len() == 1 { "" } else { "s" }));
        }
        if let Some(a) = loop_repeated_vertex(l) {
            return Err(format!("Face feature '{name}': loop {} repeats a point — vertex {} and vertex {} are both ({}, {}), a zero-length edge the offsetter cannot index; drop the duplicate", i + 1, a + 1, (a + 1) % l.len() + 1, crate::gcode::f(l[a].x), crate::gcode::f(l[a].y)));
        }
        if let Some((a, b)) = loop_self_intersection(l) {
            return Err(format!("Face feature '{name}': loop {} crosses itself — the edge from vertex {} and the edge from vertex {} intersect, so it bounds no one region; untangle the points", i + 1, a + 1, b + 1));
        }
        let ccw = signed(l) > 0.0;
        if i == 0 && !ccw {
            return Err(format!("Face feature '{name}': its first loop is the OUTER boundary and winds counter-clockwise; this one winds clockwise. Reverse its points (the winding is never normalized — it is the traversal)"));
        }
        if i > 0 && ccw {
            return Err(format!("Face feature '{name}': loop {} is an INNER boundary and winds clockwise; this one winds counter-clockwise. Reverse its points, or if it is a second face, declare a second Face", i + 1));
        }
    }
    for i in 1..loops.len() {
        if crate::stock::plan_nesting(&loops[i], &loops[0]) != crate::stock::Nesting::Inside {
            return Err(format!("Face feature '{name}': loop {} is not inside the outer boundary — an inner loop is a boundary WITHIN the face", i + 1));
        }
        for j in i + 1..loops.len() {
            if crate::stock::plan_nesting(&loops[i], &loops[j]) != crate::stock::Nesting::Apart {
                return Err(format!("Face feature '{name}': loops {} and {} overlap or nest — inner boundaries stand clear of each other", i + 1, j + 1));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_revolve_dims(name: &str, f: &Feature) -> Result<(), String> {
    match f {
        Feature::Cone { height, .. } => {
            if *height <= 0.0 {
                return Err(format!("Cone feature '{}' height must be positive", name));
            }
        }
        Feature::Torus { major_dia, minor_dia, .. } => {
            if *minor_dia <= 0.0 {
                return Err(format!("Torus feature '{}' minor_dia must be positive", name));
            }
            if *major_dia <= 0.0 {
                return Err(format!("Torus feature '{}' major_dia must be positive", name));
            }
        }
        // The height >= dia/2 floor is the non-undercut constructor rule (see
        // the variant's doc comment): a tangent ogive shallower than its
        // hemisphere limit crests before the axis and the nose overhangs — a
        // 3-axis undercut, rejected here so it stays unrepresentable rather
        // than runtime-checked.
        // The neck_dia < dia floor is the same species of constructor rule as
        // the ogive's: the waist sits AT the top by construction, so the
        // widening-above-the-waist undercut is unrepresentable rather than
        // runtime-checked.
        // Pseudosphere band: the ordering base_dia < dia is not a style check
        // — at base_dia == dia the band would include the cusp rim, where the
        // concave meridian radius of curvature c·sinh(u) vanishes, a point no
        // ball tool of any radius can machine.
        // Vase (row 5.2): the two degenerate orderings and the overhang bound.
        _ => {}
    }
    Ok(())
}

/// What one declared feature must satisfy on its own terms, before any op
/// names it: a shape that can be machined.
fn validate_feature(name: &str, feature: &Feature) -> Result<(), String> {
    if let Feature::Face { loops, .. } = feature {
        validate_face_loops(name, loops)?;
    }
    // The tilted bore's own shape (Phase 41 slice 5), judged from the
    // declaration alone — no op may name it, so no op-time check would
    // ever reach it. `tilt` strictly inside (0, 90): 0 IS a vertical
    // bore and has a noun already (the degenerate-pointer move, as
    // `rotation` 0 points at omitting the field); 90 or more never
    // enters the stock from its top face.
    if let Feature::TiltedBore { dia, extent, tilt, heading, .. } = feature {
        if *dia <= 0.0 {
            return Err(format!("TiltedBore feature '{name}' dia must be positive"));
        }
        if *extent <= 0.0 {
            return Err(format!("TiltedBore feature '{name}' extent must be positive"));
        }
        if *tilt == 0.0 {
            return Err(format!(
                "TiltedBore feature '{name}' tilt (0) IS a vertical bore — declare a CylinderBore"
            ));
        }
        if *tilt < 0.0 || *tilt >= 90.0 {
            return Err(format!(
                "TiltedBore feature '{name}' tilt ({}) must be strictly inside (0, 90): the angle is measured from vertical, and a bore at 90 or more never enters the stock from its top face — the lean's direction is `heading`",
                crate::gcode::f(*tilt)
            ));
        }
        if !heading.is_finite() {
            return Err(format!("TiltedBore feature '{name}' heading must be a number of degrees"));
        }
    }
    if let Feature::ProfilePocket { points, extent, .. }
        | Feature::ProfileBoss { points, extent, .. } = feature
    {
        // Side-of-material is the variant's; the winding is redundant data
        // validated against it (CCW void / CW material, the blessed comp
        // convention). A mismatch is rejected pointing at the other variant
        // rather than silently reversed — reversing the traversal would
        // flip climb/conventional on the machine.
        let kind = if matches!(feature, Feature::ProfileBoss { .. }) { "ProfileBoss" } else { "ProfilePocket" };
        if points.len() >= 3 {
            let area = signed_area(points);
            let is_boss = matches!(feature, Feature::ProfileBoss { .. });
            if area == 0.0 {
                return Err(format!(
                    "{} feature '{}' has zero signed area: the loop is degenerate (collinear or self-cancelling) and bounds nothing",
                    kind, name
                ));
            }
            if let Some(i) = loop_repeated_vertex(points) {
                return Err(format!(
                    "{} feature '{}' repeats a point: vertex {} and vertex {} are both ({}, {}) — a zero-length edge bounds nothing and the offsetter cannot index it; drop the duplicate",
                    kind, name, i + 1, (i + 1) % points.len() + 1, crate::gcode::f(points[i].x), crate::gcode::f(points[i].y)
                ));
            }
            if let Some((i, j)) = loop_self_intersection(points) {
                return Err(format!(
                    "{} feature '{}' crosses itself: the edge from vertex {} and the edge from vertex {} intersect, so the loop bounds no one region — untangle the points (a loop is one simple closed traversal)",
                    kind, name, i + 1, j + 1
                ));
            }
            if is_boss && area > 0.0 {
                return Err(format!(
                    "ProfileBoss feature '{}' winds counter-clockwise, but a boss is standing material cut from outside, authored CW (the blessed winding convention). Wind the loop CW — or if it bounds a void, declare a ProfilePocket",
                    name
                ));
            }
            if !is_boss && area < 0.0 {
                return Err(format!(
                    "ProfilePocket feature '{}' winds clockwise, but a pocket is a void cut from inside, authored CCW (the blessed winding convention). Wind the loop CCW — or if it bounds standing material, declare a ProfileBoss",
                    name
                ));
            }
        }
        if *extent == 0.0 {
            return Err(format!(
                "{} feature '{}' has extent 0: a planar profile spans no Z of its own, so extent 0 leaves no wall to cut. Extent 0 is reserved for geometry that carries its own Z range (the 3D-wire ContourPocket/ContourBoss pair), which this feature is not — give it a positive extent",
                kind, name
            ));
        }
        if *extent < 0.0 {
            return Err(format!(
                "{} feature '{}' extent ({}) must be positive: extent is the feature's own vertical occupancy, not a direction — the wall runs downward from `z` by construction",
                kind, name, extent
            ));
        }
    }
    // The wire pair's own shape rules (DECISIONS.md "the 3D-wire
    // Contour…"): the row-2.4 winding law is judged on the XY projection —
    // the wall is a vertical sweep, so its plan view carries the same
    // side-of-material information a planar loop does, and Z plays no
    // part in it. `extent == 0` is LEGAL here and only here: the wire
    // carries its own Z range, so zero extrusion still leaves the wire
    // itself to cut (a trim edge).
    if let Feature::ContourPocket { points, extent }
        | Feature::ContourBoss { points, extent } = feature
    {
        let kind = if matches!(feature, Feature::ContourBoss { .. }) { "ContourBoss" } else { "ContourPocket" };
        if points.len() >= 3 {
            let proj: Vec<Pt> = points.iter().map(Pt3::xy).collect();
            let area = signed_area(&proj);
            let is_boss = matches!(feature, Feature::ContourBoss { .. });
            if area == 0.0 {
                return Err(format!(
                    "{} feature '{}' has zero signed area in plan view: the wire's XY projection is degenerate (collinear or self-cancelling) and bounds no wall",
                    kind, name
                ));
            }
            if let Some(i) = loop_repeated_vertex(&proj) {
                return Err(format!(
                    "{} feature '{}' repeats a point in plan view: vertex {} and vertex {} are both ({}, {}) in XY — a zero-length edge bounds no wall and the offsetter cannot index it; drop the duplicate",
                    kind, name, i + 1, (i + 1) % proj.len() + 1, crate::gcode::f(proj[i].x), crate::gcode::f(proj[i].y)
                ));
            }
            if let Some((i, j)) = loop_self_intersection(&proj) {
                return Err(format!(
                    "{} feature '{}' crosses itself in plan view: the edge from vertex {} and the edge from vertex {} intersect in XY, so the wire bounds no one wall — untangle the points",
                    kind, name, i + 1, j + 1
                ));
            }
            if is_boss && area > 0.0 {
                return Err(format!(
                    "ContourBoss feature '{}' winds counter-clockwise in plan view, but a boss is standing material cut from outside, authored CW (the blessed winding convention). Wind the wire CW — or if it rims a void, declare a ContourPocket",
                    name
                ));
            }
            if !is_boss && area < 0.0 {
                return Err(format!(
                    "ContourPocket feature '{}' winds clockwise in plan view, but a pocket is a void cut from inside, authored CCW (the blessed winding convention). Wind the wire CCW — or if it rims standing material, declare a ContourBoss",
                    name
                ));
            }
        }
        if *extent < 0.0 {
            return Err(format!(
                "{} feature '{}' extent ({}) must not be negative: extent is the feature's own vertical occupancy below the wire, not a direction — the wall runs downward from each point's own z by construction (0 is legal: no extrusion, the wire itself is the edge)",
                kind, name, extent
            ));
        }
    }
    Ok(())
}

/// An `End` on a surface: the floor the tool must reach and the tool it takes.
fn validate_end_op(p: &Part, op: &Operation, tool: &Tool, feature: &Feature) -> Result<(), String> {
    // The contact check (Phase 36 slice 2): the verb names
    // the tool's end, so the tool has to have one.
    let cuts = derive_tool_cuts(tool);
    if !cuts.contains(&Cut::End) {
        return Err(format!(
            "End operation feature '{}' is cut by T{}, a {} — the tool's end makes a floor, and this tool has no flat end (it cuts with its {})",
            op.feature_name,
            op.tool_num,
            tool.name,
            cuts.iter().map(|c| c.label()).collect::<Vec<_>>().join(" and ")
        ));
    }
    // `End` finishes a declared planar floor to its declared
    // height, and takes any feature that declares one (Phase
    // 27, fork 6A): a `Plane`, and a `ProfilePocket` — its
    // floor, inside its walls, around its islands.
    if !matches!(feature, Feature::Plane { .. } | Feature::Face { .. } | Feature::ProfilePocket { .. }) {
        return Err(format!(
            "End operation feature '{}' must declare a planar floor — a Plane, a Face, or a ProfilePocket (whose floor it finishes)",
            op.feature_name
        ));
    }
    let stepover = derive_stepover(p, op).unwrap_or(0.0);
    if stepover <= 0.0 {
        return Err("End operation requires a positive stepover".to_string());
    }
    // One ceiling for every op that works a floor (Phase 43
    // slice 4): the Clear's law, which the End lacked — a
    // stepover at or over the diameter leaves a strip of
    // floor between every pass that no pattern's claim
    // covers, and the handle could drag one there.
    if stepover >= tool.dia {
        return Err(format!(
            "End operation stepover ({:.3}mm) is at or above tool T{}'s diameter ({:.3}mm): adjacent passes would not overlap, leaving an uncut strip of floor between every pass — reduce stepover",
            stepover, op.tool_num, tool.dia
        ));
    }
    Ok(())
}

/// A `Clear`: which features it may empty, with what strategy, pattern and stepdown.
fn validate_clear_op(p: &Part, op: &Operation, tool: &Tool, feature: &Feature) -> Result<(), String> {
    match feature {
        Feature::ProfilePocket { points, .. } => {
            if points.len() < 3 {
                return Err(format!("Clear feature '{}' must have at least 3 points", op.feature_name));
            }
        }
        // The circular pocket (DECISIONS.md "adaptive
        // clearing…", slice 1's feature coverage): its offset
        // family is concentric circles. Size checks live with
        // the family generation in ops::clear, beside the
        // pocket's own too-small/island errors.
        Feature::CylinderBore { dia, .. } => {
            if *dia <= 0.0 {
                return Err(format!("Clear feature '{}' must have a positive diameter", op.feature_name));
            }
        }
        // The two prismatic bosses. These used to reject on a
        // PRINCIPLE — "clearing standing material is not a
        // smaller mistake, it is the opposite op" — and that
        // sentence is retired (DECISIONS.md → "roughing a
        // convex revolve…", fork 1A): `Clear` does clear around
        // standing material now, on the revolve family below.
        // What survives is a SCOPE note plus a typo catch worth
        // keeping on exactly this pair: `ProfileBoss` sits one
        // word from `ProfilePocket`, so an op aimed at the boss
        // is usually an op that meant the pocket. A `Sphere` has
        // no such twin, which is why the asymmetry is here and
        // not a rule.
        // The prismatic bosses (Phase 10 lane A, slice 1):
        // `Clear` roughs AROUND them — the pocket spiral run
        // outward, the boss as its island, the stock as the
        // one outer wall that needs no ruling — in Z levels
        // down to the base, as the revolve family does. The
        // typo catch this pair used to carry ("a slip for
        // the pocket") is gone with the refusal: the op is
        // real now, and a slip emits a moat rather than a
        // sentence, which the rest report then names.
        f @ (Feature::ProfileBoss { .. } | Feature::ContourBoss { .. }) => {
            let stepdown = op.stepdown.unwrap_or(0.0);
            if stepdown <= 0.0 {
                return Err(format!(
                    "Clear operation feature '{}' is a standing boss: roughing around one is a stack of Z levels, not a single pass, so it requires a positive stepdown (the maximum bite per level — the floor is the boss's own base plane)",
                    op.feature_name
                ));
            }
            if op.strategy.is_some() {
                return Err(format!(
                    "Clear operation feature '{}' is a standing boss: strategy selects passes inside a declared pocket, and this op's work-list is the boss's own outline offset outward. Drop the strategy",
                    op.feature_name
                ));
            }
            let (top, base) = prismatic_boss_z_span(f, p.stock.z).expect("the arm matched a boss");
            if top < p.stock.z - 1e-9 {
                return Err(format!(
                    "Clear operation feature '{}' stands with its top at z {:.3}, below the stock top z {:.3}: the column above a sunk prismatic boss is not modelled by this op yet (the cylinder boss takes it through the revolve family's crown break; the prismatic moat's own is a named remainder) — declare the boss top at the stock top, or Face the stock down to it first",
                    op.feature_name, top, p.stock.z
                ));
            }
            if p.stock.z - base <= 1e-9 {
                return Err(format!(
                    "Clear operation feature '{}' has nothing to rough: its base plane z {:.3} is at or above the stock top z {:.3}",
                    op.feature_name, base, p.stock.z
                ));
            }
            if let Feature::ProfileBoss { points, .. } = f {
                if prismatic_boss_cl_loops(f, 1e-6).is_none() || points.len() < 3 {
                    return Err(format!(
                        "Clear operation feature '{}' has a profile the offsetter cannot run outward — the boss outline is degenerate",
                        op.feature_name
                    ));
                }
            }
        }
        // A Plane or a Face is a floor whose wall is the stock
        // (Phase 61 slice 1, fork 1A): the VOLUME above it is
        // the Clear's, around whatever stands in it. The
        // region is `region::of`'s, the same one the End reads.
        Feature::Plane { .. } | Feature::Face { .. } => {
            if op.strategy.is_some() {
                return Err(format!(
                    "Clear operation feature '{}' is a {}: strategy selects passes inside a declared pocket against the voxel field, and a floor's volume has none declared — drop strategy=, or Clear a ProfilePocket",
                    op.feature_name, feature.kind()
                ));
            }
        }
        Feature::ContourPocket { .. } => {
            // A rim wire declares its WALL, not an interior —
            // the region inside the projection has no declared
            // floor for Clear to spiral down to (DECISIONS.md
            // "the 3D-wire Contour…": the rim declares no
            // interior). The wall is Finish's job.
            return Err(format!(
                "Clear operation feature '{}' is a ContourPocket rim — the wire declares a wall, not an interior with a floor to empty (the interior under a non-planar rim is a pocket the vocabulary cannot say yet, T5). Finish traces the wall; a region to clear is a ProfilePocket — the wire BOSS, whose surround is the stock, takes a Clear like any boss",
                op.feature_name
            ));
        }
        // The standing surfaces of revolution: `Clear` roughs
        // AROUND them, level by level, down to their own base
        // plane (DECISIONS.md → "roughing a convex revolve…").
        // The geometry checks live in
        // `derive_revolve_clear_levels`, the one home that also
        // computes the stack — so what can be machined and what
        // is emitted cannot disagree.
        f if derive_revolve_base_z(f).is_some() => {
            // The feature against itself, before anything
            // derives from it: a degenerate meridian must be
            // a named error here, not a NaN panic in the
            // level walk below.
            validate_revolve_dims(&op.feature_name, f)?;
            let stepdown = op.stepdown.unwrap_or(0.0);
            if stepdown <= 0.0 {
                return Err(format!(
                    "Clear operation feature '{}' is a standing surface of revolution: roughing around one is a stack of Z levels, not a single pass, so it requires a positive stepdown (the maximum bite per level — the floor is still the feature's own base plane)",
                    op.feature_name
                ));
            }
            if op.strategy.is_some() {
                return Err(format!(
                    "Clear operation feature '{}' is a standing surface of revolution: strategy selects passes inside a declared pocket, and this op's work-list comes from the feature's own silhouette at each level. Drop the strategy",
                    op.feature_name
                ));
            }
            let skin = op.stock_to_leave.unwrap_or(0.0);
            if derive_revolve_clear_stacks(f, p.stock.z, stepdown, skin).is_none() {
                // Two reachable causes, and the message names
                // both because the fix differs: a stock that
                // does not stand above the base plane (what the
                // containment lint reports), and a meridian
                // that crests more than once.
                let base = derive_revolve_base_z(f).expect("guarded by the arm");
                if p.stock.z - base <= 0.0 {
                    return Err(format!(
                        "Clear operation feature '{}' has nothing to rough: its base plane z {:.3} is at or above the stock top z {:.3}, so no material stands over it. Declare the stock the part is actually made from",
                        op.feature_name, base, p.stock.z
                    ));
                }
                return Err(format!(
                    "Clear operation feature '{}' has a meridian that crests more than once — the level stack breaks at ONE crown into an outer branch and an inner one, and a meridian that rises again inside the hole puts three radii at some height, which this op cannot state",
                    op.feature_name
                ));
            }
        }
        _ => {
            return Err(format!("Clear operation feature '{}' must be a ProfilePocket, a Plane, a Face, a CylinderBore, or a standing surface of revolution", op.feature_name));
        }
    }
    if derive_stepover(p, op).unwrap_or(0.0) <= 0.0 {
        return Err("Clear operation requires a positive stepover".to_string());
    }
    require_flat_bottom_tool("Clear", tool)?;
    // Adjacent spiral loops sit `stepover` apart at one Z, so a
    // flat tool covers the gap only while stepover <= dia. At or
    // past the diameter an uncut RIDGE stands between every pair
    // of loops — unbounded leftover, which no allowance can
    // ceiling and which the flat-bottom claim below the emitter
    // silently contradicts. The bound holds at sharp corners
    // too: on a square's corner bisector the worst uncovered
    // point sits stepover/2 from both loops, needing the same
    // stepover/2 <= r. `>=` matches derive_cusp_height_flat's
    // d/2 >= r_ball -> INFINITY convention.
    //
    // Always-on, not gated on stock_to_leave: the ridge is there
    // either way (DECISIONS.md "stock_to_leave is a radial
    // allowance…" — the always-on findings).
    let stepover = derive_stepover(p, op).unwrap_or(0.0);
    if stepover >= tool.dia {
        return Err(format!(
            "Clear operation stepover ({:.3}mm) is at or above tool T{}'s diameter ({:.3}mm): adjacent loops would not overlap, leaving an uncut ridge of full-depth stock between every pass — reduce stepover",
            stepover, op.tool_num, tool.dia
        ));
    }
    Ok(())
}

/// A `Side`, `Copy` or `Form` along a wall or over a surface: the contact the
/// tool makes with the feature, and every register and offset that rides on it.
fn validate_wall_op(p: &Part, op: &Operation, tool: &Tool, feature: &Feature) -> Result<(), String> {
    let verb = op.op_type.verb();
    // A floor is the end's, and no contact verb follows one.
    if matches!(feature, Feature::Plane { .. } | Feature::Face { .. }) {
        return Err(format!(
            "{verb} operation feature '{}' is a {}, a floor — the tool's end makes a floor. Write `run End(...)`",
            op.feature_name, feature.kind()
        ));
    }
    // A drill cuts by its form, at points: it follows nothing.
    if tool.name == "Drill" && tool.profile.is_none() {
        return Err(format!(
            "{verb} operation feature '{}' is cut by T{}, a Drill — a drill cuts by its form at points and follows no surface",
            op.feature_name, op.tool_num
        ));
    }
    // The contact check (Phase 36 slice 1): the verb names
    // the part of the tool that cuts, and the tool and the
    // feature already say which part that is. Validated
    // redundancy, the winding precedent — a mismatch refuses
    // and is never quietly corrected.
    let contact = derive_contact(tool, feature);
    if contact != op.op_type {
        return Err(format!(
            "{verb} operation feature '{}' with T{} is not a {} cut: it is a {} — write `run {}(...)`",
            op.feature_name, op.tool_num, verb.to_lowercase(), contact_reason(tool, feature), contact.verb()
        ));
    }
    // Checked before the per-feature geometry checks below so
    // "wrong tool for this feature entirely" is reported first
    // rather than being masked by an unrelated field error the
    // user would have to fix first to even discover it
    // (DECISIONS.md "Custom tool-profile finishing...").
    if tool.profile.is_some() && !matches!(feature, Feature::Sphere { .. }) {
        return Err(format!(
            "Form operation feature '{}' uses tool T{}'s custom profile, but only Sphere features support custom tool profiles today (Cone/Torus need their own extension of the contact math)",
            op.feature_name, op.tool_num
        ));
    }
    // Only Sphere has finishing math today; Cone/Torus parse but
    // have no consumer yet, so referencing one is a clear "not
    // yet" rather than a silent no-op (ROADMAP Phase 3).
    // The standing revolves' field-shape invariants live
    // in `validate_revolve_dims` — one home, shared with
    // the revolve `Clear` (which validates FIRST in a
    // contained fixture and used to panic in the meridian
    // walk on a degenerate feature).
    validate_revolve_dims(&op.feature_name, feature)?;
    match feature {
        Feature::Sphere { .. } => {}
        // The polyline pair: wall tracing is one op regardless
        // of cross-section, so the former Contour op's checks
        // live here now (exact flank, flat-bottomed tool).
        Feature::ProfilePocket { points, .. }
        | Feature::ProfileBoss { points, .. } => {
            if points.len() < 3 {
                return Err(format!("profile feature '{}' must have at least 3 points", op.feature_name));
            }
            // A form tool on this feature is not tracing the
            // wall — it is placing the declared edge break,
            // which wants no flat floor and no full-width
            // flank. Its whole rule set is its own check
            // below, so the flat-bottom rule steps aside
            // rather than rejecting with a sentence about the
            // wrong pass.
            if tool.rounder.is_none() && tool.chamfer.is_none() {
                require_flat_bottom_tool("Side", tool)?;
            }
        }
        // The wire pair: the same wall-tracing op with the Z
        // profile the wire carries — a vertical wall in any Z
        // profile is still a flank cut, so the flat-bottom
        // rule holds unchanged.
        Feature::ContourPocket { points, .. }
        | Feature::ContourBoss { points, .. } => {
            if points.len() < 3 {
                return Err(format!("contour wire feature '{}' must have at least 3 points", op.feature_name));
            }
            // Same deferral as the planar pair above: a form
            // tool here is rejected for having no planar edge
            // to break, which is the true reason, not for
            // lacking a flat bottom.
            if tool.rounder.is_none() && tool.chamfer.is_none() {
                require_flat_bottom_tool("Side", tool)?;
            }
        }
        Feature::Cone { .. } => {}
        Feature::Torus { .. } => {}
        // The bowl's tool-fit constraint (ball radius vs the
        // vertex radius of curvature) is the shared runtime
        // gouge guard in ops::finish, like the torus — only
        // field-shape checks live here.
        // The height >= dia/2 floor is the non-undercut
        // constructor rule (see the variant's doc comment): a
        // tangent ogive shallower than its hemisphere limit
        // crests before the axis and the nose overhangs — a
        // 3-axis undercut, rejected here so it stays
        // unrepresentable rather than runtime-checked.
        // The neck_dia < dia floor is the same species of
        // constructor rule as the ogive's: the waist sits AT
        // the top by construction (see the variant's doc
        // comment), so the widening-above-the-waist undercut
        // is unrepresentable rather than runtime-checked. The
        // tool-fit constraint (ball radius vs the waist's
        // concave meridian radius) is the shared runtime gouge
        // guard in ops::finish, like the bowl — only
        // field-shape checks live here.
        // Catenoid throat (row 5.3, inside orientation): same
        // field-shape checks as the outside neck — the waist
        // sits AT the bottom by construction (see the
        // variant's doc comment), so the widening-below-the-
        // waist undercut is unrepresentable rather than
        // runtime-checked. The tool-fit constraint (ball
        // radius vs the throat's concave hoop radius) is the
        // shared runtime gouge guard in ops::finish.
        // Pseudosphere band (row 5.3, second member): the
        // ordering base_dia < dia is not a style check — at
        // base_dia == dia the band would include the cusp rim,
        // where the concave meridian radius of curvature
        // c·sinh(u) vanishes, a point no ball tool of any
        // radius can machine (see the variant's doc comment).
        // Only field-shape checks live here; the tool-fit
        // constraint against the band's own tightest meridian
        // radius is the shared runtime gouge guard in
        // ops::finish, like the catenoid and the bowl.
        // Pseudosphere funnel (row 5.3, inside orientation):
        // same ordering shape as the outside band, but the
        // mouth_dia < dia error has a different *reason*
        // (see the variant's doc comment): the tangent-lip
        // full form is a deferred superset (the march's
        // parameter speed degenerates at the cusp), not the
        // outside band's no-tool-can-machine-it
        // impossibility. Tool-fit against the tip circle's
        // concave hoop radius is the shared runtime gouge
        // guard in ops::finish.
        // CylinderBore-as-surface: exact-flank finishing of the inside
        // bore wall (CHECKLIST row 2.3; ROADMAP Phase 3.5). The
        // developable wall flank-cuts with zero error, so the
        // tool-centre rides a coaxial circle of radius R - r_tool.
        // dia must exceed the tool strictly: dia == tool.dia leaves
        // a zero-radius (degenerate) path, dia < tool.dia can't fit.
        Feature::CylinderBore { dia, extent, .. } => {
            if *extent <= 0.0 {
                return Err(format!("CylinderBore feature '{}' extent must be positive", op.feature_name));
            }
            // The form tools are exempt, and the exemption is
            // the form/clearance split again: this rule reads
            // `tool.dia`, which for both is the BODY OD — and
            // a form tool's body sits ABOVE the top face, in
            // open air, where nothing constrains it. What has
            // to fit down the hole is the reach the axis
            // rides, and each form's own check says so in
            // those terms. A 12mm-OD tool breaking the lip of
            // a 10mm bore is ordinary work; rejecting it here
            // would be the body OD deciding a placement
            // question again.
            if tool.rounder.is_none() && tool.chamfer.is_none() && *dia <= tool.dia {
                return Err(format!("CylinderBore feature '{}' diameter ({}) must be larger than tool T{} diameter ({}) to leave a bore wall to finish", op.feature_name, dia, op.tool_num, tool.dia));
            }
        }
        // Vase (row 5.2): field-shape checks — the two
        // degenerate orderings and the overhang bound. The
        // tool-fit constraint (ball radius vs the neck arc's
        // concave radius r) is the shared runtime gouge guard
        // in ops::finish, like every curved surface.
        // Elliptic pair (row 2.5's elliptic member): field-
        // shape checks only — the tool-fit constraint (tool
        // radius vs the bore wall's tightest concave radius of
        // curvature, the evolute bound (minor/2)²/(major/2))
        // is the shared runtime gouge guard in ops::finish,
        // like every curved-wall op. major == minor is the
        // circle: rejected with a pointer at the feature that
        // IS that shape (CylinderBore / CylinderBoss), whose R1 closed
        // form the elliptic op reduces to (the test anchor).
        // CylinderBoss: the outside wall (CHECKLIST row 2.4). The tool
        // rides outside at R + r_tool, so unlike the bore there
        // is no fit constraint against the diameter — any tool
        // fits around a boss.
        Feature::CylinderBoss { dia, extent, .. } => {
            if *extent <= 0.0 {
                return Err(format!("CylinderBoss feature '{}' extent must be positive", op.feature_name));
            }
            if *dia <= 0.0 {
                return Err(format!("CylinderBoss feature '{}' dia must be positive", op.feature_name));
            }
        }
        // Camp 3: a sampled mesh (ROADMAP Phase 4). Shape
        // checks live at parse time (readable STL, non-empty);
        // spacing is stepover-only — a constant-scallop
        // formula for a sampled tessellation isn't derived,
        // so scallop is a clear "not yet", not a silent
        // approximation.
        Feature::Mesh { .. } => {
            if op.scallop.is_some() {
                return Err(format!(
                    "Copy operation on mesh feature '{}': constant-scallop spacing isn't derived for a sampled mesh; use stepover",
                    op.feature_name
                ));
            }
        }
        // The analytic NURBS reader (ROADMAP Phase 4 follow-up).
        // Geometry is validated here: the meridian must be a
        // machinable cap (apex-first, no undercut). Spacing is
        // the shared stepover-XOR-scallop contract below, same
        // as every revolved member — constant scallop marches
        // parse::derive_spline_revolve_step_u (DECISIONS.md
        // "Constant scallop over a spline meridian…").
        Feature::SplineRevolve { meridian, .. } => {
            meridian.check_machinable_cap(&op.feature_name)?;
        }
        // The inside orientation validates against the mirror
        // envelope (mouth-first, no widening below the mouth);
        // spacing is the same stepover-XOR-scallop contract.
        Feature::SplineRevolveFunnel { meridian, .. } => {
            meridian.check_machinable_funnel(&op.feature_name)?;
        }
        // The general NURBS patch (DECISIONS.md "the general
        // NURBS patch…"): the exact surface under the mesh
        // raster contract, so it takes the Mesh's
        // stepover-only rule — with the difference stated:
        // here the exact curvature EXISTS, so a scallop
        // derivation is a genuine follow-up, not a
        // permanent-by-nature error.
        Feature::NurbsPatch { .. } => {
            if op.scallop.is_some() {
                return Err(format!(
                    "Copy operation on patch feature '{}': constant-scallop spacing over a general patch isn't derived yet (a named follow-up — the exact curvature exists); use stepover",
                    op.feature_name
                ));
            }
        }
        _ => {
            return Err(format!("{verb} operation feature '{}' must be a Sphere, Cone, Torus, Ellipsoid, Paraboloid, ParaboloidBowl, Ogive, Catenoid, CatenoidThroat, Pseudosphere, PseudosphereFunnel, Vase, CylinderBore, CylinderBoss, EllipticBore, EllipticBoss, Mesh, SplineRevolve, SplineRevolveFunnel, or NurbsPatch", op.feature_name));
        }
    }
    // The waterline offset assumes a ball-nose (offset one ball
    // radius along the surface normal); a flat/other cutter would
    // gouge the sphere, so require it rather than emit a bad path
    // -- UNLESS the tool carries a custom profile (`ops::finish`'s
    // vertex-following contact math generalizes the ball offset
    // for an arbitrary profile), which is wired up for Sphere only
    // so far (DECISIONS.md "Custom tool-profile finishing...").
    // Whether a tool takes this path is decided by carrying a
    // profile, not by its (free-form, user-chosen) name -- a
    // tool named "Ballmill" that also carries a profile is
    // treated as a profile tool; the name is a label, not a type.
    // A corner rounder cuts a declared EDGE ROUND, which is a
    // surface no feature can carry yet (DECISIONS.md → "the
    // corner rounder gets an operation": the tool lands first,
    // the `edge_round` field and the placement follow). Rejected
    // by name rather than falling through to the ball-nose
    // requirement, which would blame the wrong thing.
    if let Some(rd) = &tool.rounder {
        check_edge_round_op(p, op, feature, tool, rd)?;
        return Ok(());
    }
    // The chamfer's whole rule set, like the rounder's above —
    // one home, and the generic finishing checks are not about
    // this pass (DECISIONS.md → "the chamfer tool…", slice 2).
    if let Some(ch) = &tool.chamfer {
        check_chamfer_op(p, op, feature, tool, ch)?;
        return Ok(());
    }
    if tool.profile.is_some() {
        if op.scallop.is_some() {
            return Err(format!(
                "Form operation feature '{}' uses tool T{}'s custom profile: constant-scallop spacing isn't derived for an arbitrary profile yet, use stepover",
                op.feature_name, op.tool_num
            ));
        }
    } else if tool.name != "Ballmill"
        && !matches!(
            feature,
            Feature::CylinderBore { .. }
                | Feature::CylinderBoss { .. }
                | Feature::ProfilePocket { .. }
                | Feature::ProfileBoss { .. }
                | Feature::ContourPocket { .. }
                | Feature::ContourBoss { .. }
        )
    {
        // A prismatic wall (circular, elliptic, or polyline) is a straight *flank*
        // cut, not a ball-nose scallop, so it is exempt from
        // the ball-nose requirement — it wants a straight-sided
        // endmill, not a ballmill.
        return Err(format!("Copy operation requires a Ballmill tool — a ball's nose is what follows a surface between wall and floor — but T{} is a {}", op.tool_num, tool.name));
    }
    if matches!(
        feature,
        Feature::ContourPocket { .. } | Feature::ContourBoss { .. }
    ) {
        // The wire wall is one full-height trace along the
        // wire's own Z profile — the same no-spacing rule as
        // the planar profile wall, stated in its own words so
        // the error names what the author actually declared.
        if op.scallop.is_some() {
            return Err("Side operation on a contour wire (ContourPocket, ContourBoss) is a full-height trace along the wire; scallop spacing does not apply".to_string());
        }
        if op.stepover.is_some() {
            return Err("Side operation on a contour wire (ContourPocket, ContourBoss) is one full-height trace along the wire and takes no stepover — omit the field".to_string());
        }
    } else if matches!(
        feature,
        Feature::ProfilePocket { .. } | Feature::ProfileBoss { .. }
    ) {
        // The polyline wall is one full-depth trace today (the
        // former Contour op): no spacing parameter applies, and
        // an authored one would be silently ignored — rejected
        // instead (an axial stepdown, when it lands, will take
        // `stepover` like the circular flanks).
        if op.scallop.is_some() {
            return Err("Side operation on a profile wall (ProfilePocket, ProfileBoss) is a full-depth flank trace; scallop spacing does not apply".to_string());
        }
        if op.stepover.is_some() {
            return Err("Side operation on a profile wall (ProfilePocket, ProfileBoss) is one full-depth trace today and takes no stepover — omit the field".to_string());
        }
    } else if matches!(
        feature,
        Feature::CylinderBore { .. }
            | Feature::CylinderBoss { .. }
    ) {
        // The flank has no scallop; `stepover` is the ring
        // spacing up the wall — the surface's one free direction
        // happens to be the axis (DECISIONS.md → "The increment
        // intake…") — and it is the only spacing that applies.
        if op.scallop.is_some() {
            return Err("Side operation on a flank wall (CylinderBore, CylinderBoss, EllipticBore, EllipticBoss) is stacked rings: stepover runs along the axis because that is the surface's one free direction; scallop spacing does not apply to a straight flank".to_string());
        }
        if !(op.stepover.unwrap_or(0.0) > 0.0) {
            return Err("Side operation on a flank wall (CylinderBore, CylinderBoss, EllipticBore, EllipticBoss) requires a positive stepover (the ring spacing, measured up the wall)".to_string());
        }
    } else {
        let has_stepover = op.stepover.unwrap_or(0.0) > 0.0;
        let has_scallop = op.scallop.unwrap_or(0.0) > 0.0;
        if has_stepover && has_scallop {
            return Err(format!("{verb} operation must specify either stepover or scallop, not both"));
        }
        if !has_stepover && !has_scallop {
            return Err(format!("{verb} operation requires either a positive stepover or scallop"));
        }
    }
    // stock_to_leave field-shape checks (DECISIONS.md
    // "stock_to_leave first slice…"). Only rules readable off
    // the fields live here; the geometry-dependent cusp budget
    // (stepover mode) is checked where the pass math lives, in
    // ops::finish beside concave_gouge_check — the same
    // validate-vs-runtime split every tool-fit check follows.
    // (The zero/negative shape checks are hoisted to the
    // carrier gate at the top of the op loop — they read the
    // same on every carrier, so they are written once.)
    if let Some(allowance) = op.stock_to_leave {
        if tool.profile.is_some() {
            return Err(format!(
                "Form operation feature '{}' uses tool T{}'s custom profile: stock_to_leave is not derived for a profile tool (its per-vertex composite locus is not a single normal offset the allowance could ride) — use a Ballmill for the roughing pass",
                op.feature_name, op.tool_num
            ));
        }
        match feature {
            // The polyline pair: the former Contour op's
            // carrier semantics — wall-only, allowance
            // engine-baked in both comp modes, cusp budget
            // structurally 0.000 (DECISIONS.md
            // "stock_to_leave is a radial allowance…").
            Feature::ProfilePocket { .. }
            | Feature::ProfileBoss { .. }
            | Feature::Sphere { .. }
            | Feature::Torus { .. }
            | Feature::CylinderBore { .. }
            | Feature::CylinderBoss { .. }
            | Feature::Cone { .. }
            | Feature::SplineRevolve { .. }
            | Feature::SplineRevolveFunnel { .. } => {}
            // The 3D-wire pair joins at its budget slice
            // (DECISIONS.md "the 3D-wire Contour…", slice 2):
            // the same wall-only radial allowance as the
            // planar profiles — one full-height flank pass,
            // so the cusp budget is structurally 0.000 — with
            // the offset's per-vertex Z riding the same
            // re-attachment engine comp committed in slice 1.
            Feature::ContourPocket { .. }
            | Feature::ContourBoss { .. } => {}
            // The drop-cutter's between-sample residual has no
            // analytic curvature bound (camp 3 is chartered to
            // accept sampling error), and a max-not-min
            // GUARANTEE cannot ride on an unbounded residual.
            Feature::Mesh { .. } => {
                return Err(format!(
                    "Copy operation feature '{}' is a Mesh: stock_to_leave is a guarantee-ceiling on remaining material, and the drop-cutter's between-sample residual has no analytic bound to budget against — mesh roughing waits for the stock-state work (ROADMAP Phase 5)",
                    op.feature_name
                ));
            }
            // The patch shares the Mesh's raster and so its
            // error — worded honestly for the analytic
            // input: the residual COULD be bounded from the
            // exact curvature, but a max-not-min guarantee on
            // a sampled pass is its own derivation, and
            // freeform roughing waits for Phase 5 regardless.
            Feature::NurbsPatch { .. } => {
                return Err(format!(
                    "Copy operation feature '{}' is a NurbsPatch: stock_to_leave is a guarantee-ceiling on remaining material, and the raster's between-sample residual budget isn't derived for the patch — freeform roughing waits for the stock-state work (ROADMAP Phase 5)",
                    op.feature_name
                ));
            }
            // The fail-safe arm for carriers whose budget is
            // not yet derived — unreachable today (every
            // Finish carrier is listed above or is the Mesh),
            // kept so the NEXT new feature rejects the field
            // until its ceiling-incl-cusps budget is derived,
            // instead of inheriting an allowance by default
            // (the committed new-carrier rule).
            _ => {
                return Err(format!(
                    "{verb} operation feature '{}' does not support stock_to_leave yet: the ceiling-incl-cusps budget must be derived for a new carrier before the field is accepted",
                    op.feature_name
                ));
            }
        }
        // Scallop mode's whole budget check is field
        // arithmetic: the march guarantees cusp == scallop, so
        // scallop >= allowance means the cusp alone spends the
        // ceiling before any nominal offset remains.
        if let Some(scallop) = op.scallop {
            if scallop >= allowance {
                return Err(format!(
                    "{verb} operation stock_to_leave ({:.3}mm) must exceed the scallop height ({:.3}mm): the allowance is a ceiling on total remaining material including the roughing cusps (max, not min), so the cusp alone already spends the whole budget — reduce scallop or increase stock_to_leave",
                    allowance, scallop
                ));
            }
        }
    }
    Ok(())
}

/// A verb at points (the drill family): the contact the tool makes at each
/// point, the cycle and its words.
fn validate_point_op(p: &Part, op: &Operation, tool: &Tool) -> Result<(), String> {
    let verb = op.op_type.verb();
    let drill_points = op.drill_points.as_ref().expect("at_points read them");
    // The contact check, at points (Phase 36 slice 3): a tool that
    // carries the shape forms the hole, a flat end seats it, and a
    // nose does neither.
    match derive_point_contact(tool) {
        Some(contact) if contact == op.op_type => {}
        Some(contact) => {
            return Err(format!(
                "{verb} operation at points with T{}: a {} cuts there with its {} — write `run {}(points=[...], ...)`",
                op.tool_num, tool.name, contact.verb().to_lowercase(), contact.verb()
            ));
        }
        None => {
            return Err(format!(
                "{verb} operation at points with T{}: a {} has neither a form nor a flat end to plunge — a hole is formed by a drill or a chamfer mill, and a seat is made by an endmill's end",
                op.tool_num, tool.name
            ));
        }
    }
    // The tools whose tip is not a drilling tip reject by name — the
    // require_flat_bottom_tool instinct, one motion family over: a
    // drill cycle plunges a tip to one Z, and these tips are not that.
    if let Some(rd) = &tool.rounder {
        return Err(format!(
            "{verb} operation tool T{} is a CornerRounder: below its arc it is a RELIEVED pilot ({:.3}mm), a clearance surface ground undersize so it does not touch — it drills nothing",
            op.tool_num, rd.pilot
        ));
    }
    if tool.profile.is_some() {
        return Err(format!(
            "{verb} operation tool T{} carries a custom profile: a drill cycle plunges one tip to one Z, and a shaped tool has no single tip diameter that plunge could mean",
            op.tool_num
        ));
    }
    // A ChamferMill in a Drill is the one relaxed pairing (DECISIONS.md
    // → "spot drilling and countersinking…"): the motion IS a drill
    // cycle, the cut is the hole's declared mouth cone, and the depth
    // derives through `GroundCone` — so the body-OD-vs-hole check
    // below does not apply (the body legitimately dwarfs the hole; the
    // seat-vs-flank and flat-vs-hole laws replace it).
    let cone = tool.ground_cone();
    for pt_name in drill_points {
        let feature = p.features.get(pt_name).ok_or_else(|| {
            format!("referenced feature '{}' not defined{} — declared: {}", pt_name, where_is(p, op), declared_features(p))
        })?;
        match feature {
            Feature::CylinderBore { dia, csink, cbore, .. } => {
                let Some(cone) = &cone else {
                    // The Endmill/counterbore pairing (DECISIONS.md →
                    // "the counterbore…"): a seat cut by a plunge is
                    // exactly as wide as the tool — no wider, no
                    // narrower — so the declared dia must EQUAL the
                    // tool's own, the drill law made a declared
                    // pairing. The body-vs-hole size check below does
                    // not apply: the seat legitimately outgrows its
                    // hole, and the parse-side seat-vs-hole laws
                    // replace it.
                    if tool.name == "Endmill" {
                        if let Some(cb) = cbore {
                            if (cb.dia - tool.dia).abs() > 1e-9 {
                                return Err(format!(
                                    "hole '{}' declares cbore_dia={} but T{}'s diameter is {}: a plunged seat is the tool's own circle, so the seat's diameter IS the endmill's — declare the endmill the print's seat calls for. A seat wider than the shop's biggest endmill is an orbited seat (helical entry + annular clearing), a different motion family and its own design when a real part needs it",
                                    pt_name, cb.dia, op.tool_num, tool.dia
                                ));
                            }
                            continue;
                        }
                    }
                    if *dia < tool.dia {
                        return Err(format!("tool T{} diameter ({}) is larger than cylinder hole '{}' diameter ({})", op.tool_num, tool.dia, pt_name, dia));
                    }
                    continue;
                };
                let ch = tool.chamfer.as_ref().expect("ground_cone implies chamfer");
                let Some(spec) = csink else {
                    return Err(format!(
                        "{verb} operation point '{}' is cut by T{}, a ChamferMill, but declares no mouth cone: the pairing cuts a declared seat, so add csink_dia= (a screw seat) or spot_dia= (a positioning dimple) to the hole — or drill it with a drill",
                        pt_name, op.tool_num
                    ));
                };
                // The pairing error reads the ANGLE, and the NOMINAL
                // one — the chamfer_angle rule on the hole's axis: a
                // ground hundredth is a deviation to report, not a
                // mismatch to reject.
                if let Some(a) = spec.angle {
                    if (a - ch.angle).abs() > 1e-9 {
                        return Err(format!(
                            "hole '{}' declares csink_angle={:.3}deg but T{} grinds a {:.3}deg cone: a form cutter cuts its own angle or the wrong one, so there is no scaling and no closest fit — declare the angle the tool actually has, or pick the tool the drawing actually needs",
                            pt_name, a, op.tool_num, ch.angle
                        ));
                    }
                }
                let seat_dia = spec.seat_dia();
                if seat_dia / 2.0 > cone.body_r {
                    return Err(format!(
                        "hole '{}' declares a {} seat of {} but T{}'s flank runs out into its body OD at {} — the seat is wider than the usable flank, so its rim would be cut by shank, which removes nothing",
                        pt_name, spec.job(), seat_dia, op.tool_num, cone.body_r * 2.0
                    ));
                }
                match spec.seat {
                    // The flat-vs-declared consistency, per job. A
                    // countersink's relieved flat must hang in the
                    // hole's own void — a flat as wide as the hole
                    // would press the seat floor with a surface that
                    // does not cut.
                    CsinkSeat::Csink(_) => {
                        if ch.tip_dia >= *dia {
                            return Err(format!(
                                "T{}'s relieved tip flat ({}) does not fit down hole '{}' ({}) — a countersink's flat hangs in the hole's void; this one would press the seat floor with a surface ground not to cut",
                                op.tool_num, ch.tip_dia, pt_name, dia
                            ));
                        }
                    }
                    // A spot goes into solid stock, so its law is the
                    // other bound: the dimple must outgrow the flat,
                    // or only the relieved flat would ever touch.
                    CsinkSeat::Spot(_) => {
                        if seat_dia <= ch.tip_dia {
                            return Err(format!(
                                "hole '{}' declares spot_dia={} but T{}'s relieved tip flat is {} — the dimple never outgrows the flat, so only a surface ground not to cut would touch the stock",
                                pt_name, seat_dia, op.tool_num, ch.tip_dia
                            ));
                        }
                    }
                }
            }
            _ => {
                return Err(format!("{verb} operation point '{}' must be a CylinderBore feature", pt_name));
            }
        }
    }
    if op.cycle == Some(CycleType::Peck) {
        let peck_val = op.peck_increment.unwrap_or(0.0);
        if peck_val <= 0.0 {
            return Err(format!("{verb} operation with Peck cycle requires a positive peck_increment"));
        }
    }
    Ok(())
}

/// What one `run` statement must satisfy: the tool exists and fits the
/// feature, every key is legal on this verb, every number is in range.
fn validate_op(p: &Part, op: &Operation) -> Result<(), String> {
    let tool = p.tools.get(&op.tool_num).ok_or_else(|| {
        format!("referenced tool T{} not defined{} — declared: {}", op.tool_num, where_is(p, op), declared_tools(p))
    })?;
    if tool.dia <= 0.0 {
        return Err(format!("tool T{} diameter must be positive", op.tool_num));
    }
    // No op of this program cuts a tilted bore (Phase 41 slice 5): it is
    // declared to be built against, gone around and read as a void —
    // this program never tilts a tool, and the bore is cut on another
    // setup. Refused by name whichever verb names it, as the surface it
    // works or as a point it visits, before any emitter could be asked.
    let tilted = std::iter::once(op.feature_name.as_str())
        .chain(op.drill_points.iter().flatten().map(String::as_str))
        .find(|n| matches!(p.features.get(*n), Some(Feature::TiltedBore { .. })));
    if let Some(n) = tilted {
        return Err(format!(
            "{} operation names '{n}', a TiltedBore — no op of this program cuts a tilted bore: a 3-axis program never tilts the tool, so the bore is cut on another setup and declared here only to be built against (nacs --arc), gone around and read as a void. Drop the run line",
            op.op_type.verb()
        ));
    }
    // A GIVEN void is air already (Phase 42 slice 1): a verb that would
    // REMOVE it — the volume (`Clear`), the floor (`End`), or a visit
    // that drills it (`points=`) — refuses by name; you cannot clear
    // what is already air. Dressing its wall (`Side`, `Copy`) is a pass
    // on air's rim, removes only its own skin, and stays legal.
    let removes_given = (op.op_type == OpType::Clear || (op.op_type == OpType::End && !op.at_points()))
        .then_some(op.feature_name.as_str())
        .into_iter()
        .chain(op.drill_points.iter().flatten().map(String::as_str))
        .find(|n| p.given.iter().any(|g| g == n));
    if let Some(n) = removes_given {
        return Err(format!(
            "{} operation names '{n}', which is given — it is air before this program starts (cut on another setup), and what is already air cannot be cleared, floored or drilled again. Drop the run line, or drop the `given {n}` line if this program does cut it",
            op.op_type.verb()
        ));
    }
    // The strategy field pair (DECISIONS.md "adaptive clearing…", slice 1):
    // rejected where unread rather than ridden ignored — a parameter that
    // silently does nothing is a program that lies about itself.
    // `stepdown` is the CLEAR's word (Phase 61 slice 2): the volume verb
    // removes a volume in a stack of Z levels, whatever the host — a
    // standing revolve or boss (required, Phase 10), a pocket, a bore, a
    // Plane or a Face (optional: absent is one level, the floor, as every
    // shipped golden cut). A surface verb — an End, a Side, a Copy, a Form
    // — follows ONE surface at one level and has one increment, so
    // `stepdown` there is rejected rather than ridden ignored, the
    // `strategy` / `voxel_cell` rule.
    if op.stepdown.is_some() {
        let feature = p.features.get(&op.feature_name);
        // A feature the program does not declare is the reference
        // check's to name (below): this sentence assumes the feature
        // exists and says what it is not — found by the tenth write,
        // whose delete of a boss under a Clear was told the Clear
        // "follows one surface" (Phase 22 slice 2).
        let undeclared = !op.at_points() && feature.is_none();
        if !undeclared && op.op_type != OpType::Clear {
            return Err(format!(
                "stepdown slices a stack of Z levels and belongs to Clear, the volume verb; a {} on '{}' follows one surface and has one increment, stepover, measured along that surface — here it would ride ignored",
                op.op_type.verb(),
                op.feature_name
            ));
        }
        // The rest selection certifies WHOLE loops void against the stock
        // at one cutting depth; a stack of levels has several. Not
        // designed, so refused by name rather than guessed.
        if !undeclared && op.strategy.is_some() && feature.is_some_and(|f| matches!(f, Feature::ProfilePocket { .. } | Feature::CylinderBore { .. } | Feature::Plane { .. } | Feature::Face { .. })) {
            return Err(format!(
                "stepdown on '{}' beside strategy=: the field selects loops certified void at one cutting depth, and a stack of levels has several — drop one of the two",
                op.feature_name
            ));
        }
        // The word is a MAXIMUM bite per level, so zero or less names no
        // stack at all — and `level_zs` would quietly cut one level, the
        // parameter that silently does nothing (Phase 61 nudge 1). The
        // standing hosts refuse theirs below by name; the optional hosts
        // refuse theirs here.
        if !undeclared && op.op_type == OpType::Clear && op.stepdown.is_some_and(|s| s <= 0.0) && feature.is_some_and(|f| matches!(f, Feature::ProfilePocket { .. } | Feature::CylinderBore { .. } | Feature::Plane { .. } | Feature::Face { .. })) {
            return Err(format!(
                "stepdown={} on '{}': a Clear's stepdown is the maximum bite per level and must be positive — drop the word for one level at the floor",
                op.stepdown.unwrap_or(0.0),
                op.feature_name
            ));
        }
    }
    // `strategy=rest` on an End (Phase 33 slice 1): the floor an earlier
    // op left standing, read from the ANALYTICAL stock model — so no field,
    // and no resolution to put in the program.
    let rest_floor = op.op_type == OpType::End && op.strategy == Some(ClearStrategy::Rest);
    if rest_floor {
        if op.voxel_cell.is_some() {
            return Err(format!(
                "voxel_cell on the End on '{}': strategy=rest on an End reads the floor an earlier op conceded from the analytical stock model, exactly — there is no field, so no resolution to set. Drop voxel_cell",
                op.feature_name
            ));
        }
        if !matches!(p.features.get(&op.feature_name), Some(Feature::Face { .. } | Feature::Plane { .. })) {
            return Err(format!(
                "strategy=rest on the End on '{}': the rest floor is built for a Face or a Plane — a pocket's floor has walls no declaration names, and is not yet designed",
                op.feature_name
            ));
        }
    } else if op.strategy.is_some() && op.op_type != OpType::Clear {
        return Err(format!(
            "strategy is a Clear parameter — {} does not select passes (an End takes one word of it: strategy=rest)",
            op.op_type.verb()
        ));
    }
    // Same rule for the chamfer's knob: only a Finish with a ChamferMill
    // reads tip_clearance, so anywhere else it would ride ignored.
    if op.tip_clearance.is_some() && !(op.follows_surface() && tool.chamfer.is_some()) {
        return Err(format!(
            "tip_clearance is the chamfer placement's knob — only a Form with a ChamferMill reads it, and op {} with T{} is not one",
            op.feature_name, op.tool_num
        ));
    }
    match (op.strategy, op.voxel_cell) {
        _ if rest_floor => {}
        (None, Some(_)) => {
            return Err(
                "voxel_cell bakes the stock field's resolution into the program, and only a field-selected strategy (rest, regions) reads that field — add one or omit voxel_cell"
                    .to_string(),
            );
        }
        (Some(s), None) => {
            // Deliberately no derived default — the one exception to the
            // derive-don't-restate habit, for a stated reason: an
            // engine-side default that moved would silently change which
            // loops drop (or where a region's boundary falls), churning
            // emitted programs invisibly. The resolution is program text.
            return Err(format!(
                "strategy={} selects passes against the voxel stock field, and the field's resolution becomes program text — declare voxel_cell=<mm> explicitly (there is deliberately no derived default)",
                match s {
                    ClearStrategy::Rest => "rest",
                    ClearStrategy::Regions => "regions",
                }
            ));
        }
        (Some(_), Some(cell)) if cell <= 0.0 => {
            return Err(format!("voxel_cell ({}) must be positive", cell));
        }
        _ => {}
    }
    // Which ops carry a stock_to_leave at all, plus the two field-shape
    // rules that read the same on every carrier (DECISIONS.md
    // "stock_to_leave is a radial allowance…"). The allowance offsets
    // surfaces the op defines by GEOMETRY — walls, from the feature
    // boundary — so an op whose only surface is a floor has nothing for it
    // to move.
    if let Some(allowance) = op.stock_to_leave {
        if op.at_points() {
            return Err(format!(
                "{} operation at points does not carry stock_to_leave: a plunged hole's diameter is the tool's own, not an offset the engine can move",
                op.op_type.verb()
            ));
        }
        match op.op_type {
            OpType::Side | OpType::Copy | OpType::Form | OpType::Clear => {}
            // On a `Plane`, that is. Floor a POCKET's floor (Phase 27
            // slice 4) has walls beside it, and there the allowance is
            // what it is everywhere else: the radial stand-off from them —
            // how the author keeps a floor pass off a finished wall.
            OpType::End if matches!(p.features.get(&op.feature_name), Some(Feature::ProfilePocket { .. })) => {}
            OpType::End => {
                return Err(
                    "End operation does not carry stock_to_leave: its only surface is the floor it cuts, and that floor IS the Plane's own `z` — author the plane where you want the material to end up".to_string()
                );
            }
        }
        if allowance == 0.0 {
            // The degenerate-pointer move (see validate_ellipse_rotation):
            // 0 names the pass the absent field already is.
            return Err("stock_to_leave (0) IS the finishing pass — omit the field".to_string());
        }
        if allowance < 0.0 {
            return Err(format!(
                "stock_to_leave ({}) must be positive: material cannot be left in negative amounts",
                allowance
            ));
        }
    }

    if !op.at_points() {
        let feature = p.features.get(&op.feature_name).ok_or_else(|| {
            format!("referenced feature '{}' not defined{} — declared: {}", op.feature_name, where_is(p, op), declared_features(p))
        })?;

        match op.op_type {
            OpType::End => validate_end_op(p, op, tool, feature)?,
            OpType::Clear => validate_clear_op(p, op, tool, feature)?,
            OpType::Side | OpType::Copy | OpType::Form => validate_wall_op(p, op, tool, feature)?,
        }
    } else {
        validate_point_op(p, op, tool)?;
    }
    Ok(())
}

pub fn validate(p: &Part) -> Result<(), String> {
    check_edge_round_registers(p)?;
    // A missing stock line is said as such (Phase 50 slice 7) — its size
    // was never the trouble.
    if p.stock_line.is_none() {
        return Err("no stock line — a program declares its blank first: `stock Box(w=, l=, h=)` or `stock Cylinder(dia=, h=)`".to_string());
    }
    if p.stock.w <= 0.0 || p.stock.l <= 0.0 {
        return Err(match p.stock.shape {
            StockShape::Box => "stock size w and l must be positive".to_string(),
            StockShape::Cylinder { .. } => "stock dia must be positive".to_string(),
            StockShape::Profile { .. } => "stock profile needs an outline with area".to_string(),
        });
    }
    // Every op's resolved coolant must have a word on this machine's menu
    // (DECISIONS.md → "coolant as a modeled setting…", fork 1A): the refusal
    // NAMES the menu key that would license the kind — the machine-config
    // gap converted into a pointer at the K1 file. Checked through the same
    // `coolant_word` home the emitter spells through, so the error and
    // the program cannot disagree.
    for (idx, op) in p.operations.iter().enumerate() {
        let kind = coolant_for(p, op);
        if coolant_word(p.machine.as_ref(), kind).is_none() {
            return Err(format!(
                "operation [{}] on '{}': coolant '{}' has no word on this machine — the undeclared dialect says only flood (M08) and off (M09); declare `coolant_{} <M-word>` in the machine file to license it",
                idx, op.feature_name, kind.name(), kind.name()
            ));
        }
    }
    // HELD (Phase 62 slice 1; `DECISIONS.md` → "Phase 62 slice 1…"): the
    // word takes a CEILING and derives its spacing from it, so a stepover
    // beside it is two governors; the ceiling is the word's alone; and a
    // ceiling past the 90° line is a slot by declaration. Before the
    // per-verb laws, which read the derived spacing and would otherwise
    // speak first about a stepover the author never meant to write.
    for op in &p.operations {
        // The rows' direction (ROADMAP item 2, slice 3) is a row pattern's
        // key: the offset family cuts loops and the raster's rows run along
        // X by construction, so beside either the key would govern nothing.
        if let Some(a) = op.angle {
            let takes_it = matches!(derive_pattern(p, op), Some(Pattern::Spine | Pattern::Zigzag | Pattern::Oneway | Pattern::Held));
            if !takes_it {
                let why = match derive_pattern(p, op) {
                    Some(Pattern::Raster) => "the raster's rows run along X and nothing turns them".to_string(),
                    Some(Pattern::Offset) if op.pattern.is_none() => "the offset family, which this op cuts unsaid, cuts loops, not rows".to_string(),
                    _ => "the offset family cuts loops, not rows".to_string(),
                };
                return Err(format!(
                    "angle={} on '{}': the rows' direction is a row pattern's key — pattern=spine, zigzag, oneway or held — and {}",
                    deg_word(a), op.feature_name, why
                ));
            }
        }
        let held = op.pattern == Some(Pattern::Held);
        if let Some(deg) = op.engagement {
            if !held {
                return Err(format!(
                    "engagement={} on '{}': the ceiling is `pattern=held`'s one key — its rows are laid under it — so pin the pattern, or drop the key",
                    deg_word(deg), op.feature_name
                ));
            }
            if !(deg > 0.0) {
                return Err(format!("engagement={} on '{}': the ceiling is an angle of engagement and must be positive", deg_word(deg), op.feature_name));
            }
            if deg > HELD_DEFAULT_DEG + 1e-9 {
                return Err(format!(
                    "engagement={} on '{}': past {}deg a pass is a slot by declaration, and the ceiling cannot be raised above the line it holds — leave the key off for {}deg, the most it takes",
                    deg_word(deg), op.feature_name, deg_word(HELD_DEFAULT_DEG), deg_word(HELD_DEFAULT_DEG)
                ));
            }
        }
        if let (true, Some(s)) = (held, op.stepover) {
            let spacing = p.tools.get(&op.tool_num).map(|t| held_spacing(t.dia / 2.0, derive_held_ceiling(op)));
            return Err(format!(
                "pattern=held with stepover={} on '{}': two governors — the ceiling derives the spacing, r times 1 - cos {}deg{}; drop the stepover",
                crate::gcode::f(s),
                op.feature_name,
                deg_word(derive_held_ceiling(op)),
                spacing.map(|a| format!(" = {} on T{}", crate::gcode::f(a), op.tool_num)).unwrap_or_default()
            ));
        }
    }
    // An extended work-offset selection is sayable exactly when the
    // machine's menu names its family with at least that many registers
    // (DECISIONS.md → "work offsets…", fork 3A) — the coolant refusal's
    // shape, third verse: the error names the menu key that would
    // license it.
    for wo in p.work_offset.iter().chain(&p.also_at) {
        let WorkOffset::Extended { family, p: reg } = wo else { continue };
        match p.machine.as_ref().and_then(|m| m.extended_offsets.as_ref()) {
            None => {
                return Err(format!(
                    "work_offset {}{}: the extended registers are the machine's menu — their spelling is the vendor's (G54.1P on one controller is G154P on another) — and no machine declares one here. Declare `extended_offsets {} <count>` in the machine file, or select G54–G59",
                    family, reg, family
                ));
            }
            Some((word, count)) => {
                if word != family {
                    return Err(format!(
                        "work_offset {}{}: this machine's extended family is spelled {} (declared `extended_offsets {} {}`) — a register in a family the controller does not speak is the wrong-dialect bet the menu exists to prevent",
                        family, reg, word, word, count
                    ));
                }
                if *reg < 1 || reg > count {
                    return Err(format!(
                        "work_offset {}{}: the declared family has registers 1–{} — a register past the menu is a register the machine does not have",
                        family, reg, count
                    ));
                }
            }
        }
    }
    // `datum` says where one register points, and the travel check reads
    // it; with several registers it would vouch for parts it cannot place.
    if p.datum.is_some() && !p.also_at.is_empty() {
        return Err("datum says where one register points, and this program runs at several (work_offset names more than one): drop the datum line, or run one register".to_string());
    }
    // The restart ladder must be unambiguous (DECISIONS.md → "N-words…",
    // fork 2A): two rungs with one label would make the operator's search
    // find the wrong op half the time. Order stays the author's — inserting
    // an op must not force renumbering the tail by hand.
    let mut rungs: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for (idx, op) in p.operations.iter().enumerate() {
        if let Some(n) = op.n {
            if let Some(prev) = rungs.insert(n, idx) {
                return Err(format!(
                    "operation [{}] on '{}' and operation [{}] on '{}' both carry n={} — two N{} lines make the restart search ambiguous, and the ladder exists to be unambiguous",
                    prev, p.operations[prev].feature_name, idx, op.feature_name, n, n
                ));
            }
        }
    }
    // What is GIVEN (Phase 42 slice 1): each name a declared VOID the stock
    // model has a closed form for — judged through the one home that
    // subtracts it, so the refusal and the subtraction cannot disagree. A
    // feature that stands is material, and material is never given; a kind
    // the model has no form for would be a guess, and is refused by name.
    for name in &p.given {
        let Some(f) = p.features.get(name) else {
            return Err(format!(
                "given '{name}': no feature named '{name}' — a given void is declared as a feature first, and the `given` line says it exists before the program starts"
            ));
        };
        if crate::stock::given_removal(p, f).is_none() {
            return Err(format!(
                "given '{name}': a {} cannot be given — only a void the stock model subtracts in closed form (a ProfilePocket, a CylinderBore, a TiltedBore) is air before this program starts; what stands is material, and material is never given",
                f.kind()
            ));
        }
    }
    // The program before this one (Phase 48 slice 1, fork 1A (i)): its
    // stock, turned as `up=` says, must BE this program's stock — the blank
    // turned over is the blank, and `given from` takes a turn about the
    // origin, never a hidden translation. A part whose two setups use two
    // origins asks first; the refusal prints both so the author sees which
    // number to move. Its own program already stood on its own at load
    // (parsed, in mm, validated) — what it removes was turned once at
    // load (`stock::turn_program`, held in `GivenFrom`), and a removal
    // that cannot be turned is declined there by name, never refused here.
    if let Some(gf) = &p.given_from {
        let turned = crate::stock::shift_stock(&crate::stock::turn_stock(&gf.part.stock, gf.up), gf.at);
        if !crate::stock::stocks_coincide(&turned, &p.stock) {
            // The same blank zeroed elsewhere is one word away: say it.
            let (a, b) = (&turned, &p.stock);
            let moved = crate::stock::shift_stock(a, [b.x - a.x, b.y - a.y, b.z - a.z]);
            let fix = if crate::stock::stocks_coincide(&moved, b) {
                format!(
                    "this is that blank zeroed elsewhere, which at={},{},{} on the `given from` line says",
                    b.x - a.x + gf.at[0] + 0.0,
                    b.y - a.y + gf.at[1] + 0.0,
                    b.z - a.z + gf.at[2] + 0.0
                )
            } else {
                "a second setup zeroed elsewhere says where the earlier program's zero sits with at=x,y,z on the `given from` line".to_string()
            };
            return Err(format!(
                "given from '{}' up={}{}: the earlier program's stock turned {}{} is {}, and this program's is {} — the blank turned over must be the blank; {fix}",
                gf.project,
                gf.up.word(),
                crate::stock::at_word(gf.at),
                match gf.up {
                    UpAxis::ZPlus => "not at all",
                    UpAxis::ZMinus => "over (y → −y, z → −z)",
                },
                if gf.at == [0.0; 3] { "" } else { " and moved" },
                crate::stock::stock_words_row(&turned),
                crate::stock::stock_words_row(&p.stock),
            ));
        }
        // The certificate (Phase 48 slice 2, fork 1A (iii)): every void
        // restated here must lie in the earlier program's air — a standing
        // sample is a hard error naming the point in both frames and the
        // depth of the steel. The earlier model incomplete is a decline,
        // voiced by `lint`, never a pass.
        for name in &p.given {
            if let Some(crate::stock::Certificate::Standing(s)) = crate::stock::certify_given(p, name) {
                return Err(crate::stock::standing_sentence(p, name, &s));
            }
        }
    }
    // Feature-shape rules that hold regardless of which op (if any) consumes
    // them — a feature that cannot describe a machinable shape is wrong on its
    // own terms, so it is checked once here rather than in every op arm.
    for (name, feature) in &p.features {
        validate_feature(name, feature)?;
    }
    for op in &p.operations {
        validate_op(p, op)?;
    }
    // `pattern=` says how a REGION is walked, so it belongs on the ops that
    // work one — a `Floor`, and a `Clear` on a `ProfilePocket` — and nowhere
    // else; and not beside `strategy=`, which selects passes of the offset
    // family against the voxel field (a field-selected parallel pass is a
    // combination nobody has designed).
    for op in &p.operations {
        let Some(pattern) = op.pattern else { continue };
        let on_region = op.op_type == OpType::End
            || (op.op_type == OpType::Clear && matches!(p.features.get(&op.feature_name), Some(Feature::ProfilePocket { .. } | Feature::Plane { .. } | Feature::Face { .. })));
        if !on_region {
            return Err(format!(
                "pattern={} on '{}': a pattern is how a floor's region is walked — it belongs on an End, or on a Clear of a ProfilePocket, a Plane or a Face",
                pattern.word(), op.feature_name
            ));
        }
        if pattern == Pattern::Raster && !raster_applies(p, op) {
            return Err(format!(
                "pattern=raster on '{}': the raster sweeps the whole blank and reads no region, so it belongs on an End of a bare Plane — nothing standing in it, no fixture on it, not the rest of an earlier op. Walk this floor in parallel passes with pattern=zigzag",
                op.feature_name
            ));
        }
        if op.strategy.is_some() && !crate::rest::is_rest(op) {
            return Err(format!(
                "pattern={} with strategy= on '{}': strategy selects passes of the offset family against the voxel field, and a field-selected parallel pass has not been designed — drop one",
                pattern.word(), op.feature_name
            ));
        }
    }
    // Overlap semantics (Phase 26 slice 1): an island and a hole are silent,
    // every other overlap refuses by name. Before the gates, which read
    // findings off a part they assume means something.
    crate::stock::overlap_refusal(p)?;
    // The engagement gate (the committed brake, ROADMAP "Engagement-driven
    // stock/feed coupling"): opt-in via `engagement gate`, it rejects the
    // program on the same findings the advisory voice would print — one
    // computation, two voices, so the two can never disagree about what
    // fires. Last so a malformed part is reported as malformed, not as an
    // engagement finding read off nonsense fields.
    // The deviation gate: opt-in via `deviation gate`, rejecting on the same
    // findings the advisory voice prints — the engagement gate's pattern, one
    // computation and two voices.
    if p.deviation == DeviationMode::Gate {
        let findings = deviation_findings(p);
        if let Some(first) = findings.first() {
            return Err(match findings.len() {
                1 => format!("deviation gate: {}", first),
                n => format!("deviation gate: {} (and {} more finding{})",
                             first, n - 1, if n > 2 { "s" } else { "" }),
            });
        }
    }
    engagement_gate(p)?;
    envelope_gate(p)?;
    header_law(p)
}

/// The header law (Phase 50 slice 1, `DECISIONS.md` → "Phase 50 slice 1…"):
/// the five program-wide words default to 0.0 in `Part`, and a project that
/// never declares them — exactly what the window's forms produce — used to
/// emit `M03 S0.000`, every cut at `F0.000` and every retract at the stock
/// top, with a green `--check`. A 0 nobody chose must not wear the look of a
/// chosen number (the `feed=derive` arm already refuses its own case by
/// name), so every op's RESOLVED feed, plunge and spindle must be positive,
/// and its resolved `rapid_z` and `clearance_z` must stand ABOVE the stock
/// top — every entry rapids to `rapid_z` before feeding, and the stock
/// model's entry restriction cannot catch a rapid into the op's OWN
/// material (that material does not outlive the op), which is how a shipped
/// fixture rapided seven millimetres into a plate for four days. Judged after
/// every other law so a structural error keeps its own sentence; the derive
/// sentinels are skipped here because `resolve_derived_words` judges them.
/// Where an operation's `run` line is, for a refusal that names it (Phase
/// 50 slice 7): " (main.part line 27)", or nothing for an operation parsed
/// from text or synthesized. The op is found by identity in the program's
/// list, so every site that holds an `&Operation` can say where it stands.
pub(super) fn where_is(p: &Part, op: &Operation) -> String {
    let Some(idx) = p.operations.iter().position(|o| std::ptr::eq(o, op)) else { return String::new() };
    match p.op_lines.get(idx) {
        Some((file, line)) => format!(" ({} line {})", if *file == 0 { "main.part" } else { "geometry.part" }, line + 1),
        None => String::new(),
    }
}

/// The tools a program declares, as a refusal lists them: `T1, T5`, or
/// `none — a tool line declares one`.
pub(super) fn declared_tools(p: &Part) -> String {
    let mut nums: Vec<usize> = p.tools.keys().copied().collect();
    nums.sort_unstable();
    if nums.is_empty() {
        return "none — a `tool T<n> <Kind>(dia=…)` line declares one".to_string();
    }
    nums.iter().map(|n| format!("T{n}")).collect::<Vec<_>>().join(", ")
}

/// The features a program declares, as a refusal lists them — the first
/// twelve by name, the rest counted.
pub(super) fn declared_features(p: &Part) -> String {
    let mut names: Vec<&String> = p.features.keys().collect();
    names.sort();
    if names.is_empty() {
        return "none — a `feature <name> <Kind>(…)` line declares one".to_string();
    }
    let shown: Vec<String> = names.iter().take(12).map(|n| format!("'{n}'")).collect();
    if names.len() > 12 {
        format!("{} and {} more", shown.join(", "), names.len() - 12)
    } else {
        shown.join(", ")
    }
}

pub(super) fn header_law(p: &Part) -> Result<(), String> {
    let top = p.stock.z;
    for (idx, op) in p.operations.iter().enumerate() {
        // A points visit names no feature; it is named by its points.
        let who = match &op.drill_points {
            Some(pts) if op.feature_name.is_empty() => format!("operation [{}] ({:?} at {})", idx, op.op_type, pts.join(", ")),
            _ => format!("operation [{}] ({:?} on '{}')", idx, op.op_type, op.feature_name),
        };
        if !op.feed_derive && op.feed.unwrap_or(p.global_feed) <= 0.0 {
            return Err(format!(
                "{who}: its feed is 0 — no `feed` line in the program and no feed= on the op. A cut at F0 is a number nobody chose; declare `feed <mm/min>` in the header"
            ));
        }
        if op.plunge.unwrap_or(p.global_plunge) <= 0.0 {
            return Err(format!(
                "{who}: its plunge feed is 0 — no `plunge` line in the program and no plunge= on the op. A descent at F0 is a number nobody chose; declare `plunge <mm/min>` in the header"
            ));
        }
        // `feed=derive` reads the resolved spindle itself and refuses a 0 with
        // its own, more specific sentence — that arm keeps the case.
        if !op.spindle_derive && !op.feed_derive && derive_spindle_speed(op, p.global_spindle) <= 0.0 {
            return Err(format!(
                "{who}: its spindle is 0 — no `spindle` line in the program and no spindle= on the op. M03 S0 is a spindle nobody started; declare `spindle <rpm>` in the header"
            ));
        }
        let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
        if rapid_z <= top {
            return Err(format!(
                "{who}: rapid_z {}mm is at or below the stock top ({}mm) — every entry rapids to it before feeding, which drives the tool into the blank at rapid; declare `rapid_z` above {}",
                crate::gcode::f(rapid_z), crate::gcode::f(top), crate::gcode::f(top)
            ));
        }
        let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
        if clearance_z <= top {
            return Err(format!(
                "{who}: clearance_z {}mm is at or below the stock top ({}mm) — the between-ops retract would rest the tool in the blank; declare `clearance_z` above {}",
                crate::gcode::f(clearance_z), crate::gcode::f(top), crate::gcode::f(top)
            ));
        }
    }
    Ok(())
}

/// The engagement gate's error — one function, two call sites. `validate`
/// calls it last (a malformed part is reported as malformed), and
/// `parse_project_with_base` calls it ONCE MORE after `resolve_derived_words`,
/// because the band (Phase 9 lane A) prices the op's resolved feed word and a
/// `feed=derive` op has none until then: its band findings decline at the
/// first call rather than price the global feed in the word's place, and the
/// second call is where the gate sees them. For a program with no `derive`
/// the two calls read identical findings and the first has already rejected.
pub fn engagement_gate(p: &Part) -> Result<(), String> {
    if p.engagement == EngagementMode::Gate {
        let findings = engagement_findings(p);
        if let Some(first) = findings.first() {
            return Err(match findings.len() {
                1 => format!("engagement gate: {}", first),
                n => format!("engagement gate: {} (and {} more finding{})",
                             first, n - 1, if n > 2 { "s" } else { "" }),
            });
        }
    }
    Ok(())
}

/// The envelope gate's parse-time half (Phase 15 slice 4): the ceiling and
/// the changer, the two arms whose findings exist before any move does. Same
/// one-computation-two-voices law as every sibling — it rejects on exactly
/// the findings the advisory voice prints — and the same two call sites as
/// the engagement gate, for the same reason: a `spindle=derive` op has no
/// RPM until the derive words resolve, so `validate`'s call reads the
/// declared numbers and the post-resolve call reads the derived ones. The
/// travel arm gates in `gcode::program`, where emitted moves exist.
pub fn envelope_gate(p: &Part) -> Result<(), String> {
    if p.envelope_mode == EnvelopeMode::Gate {
        let mut findings = envelope_ceiling_findings(p);
        findings.extend(envelope_changer_findings(p));
        if let Some(first) = findings.first() {
            return Err(match findings.len() {
                1 => format!("envelope gate: {}", first),
                n => format!("envelope gate: {} (and {} more finding{})",
                             first, n - 1, if n > 2 { "s" } else { "" }),
            });
        }
    }
    Ok(())
}
