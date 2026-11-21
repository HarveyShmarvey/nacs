use crate::math::Portable as _;
use crate::gcode::{f, stock_suffix, thinning_suffix};
use crate::parse::{derive_contour_top, derive_z_cut, CompressMode, Direction, EntryKind, Feature, Operation, Part, Pt};
use crate::offset::offset_closed_polyline;
use crate::ops::lead::LeadShape;
use crate::vox::VoxField;

/// `rest` is the before-this-op voxel stock field, present exactly when the
/// op carries `strategy=rest` (DECISIONS.md "adaptive clearing…", slice 1:
/// the field SELECTS, the analytic family CUTS). The family below is
/// generated identically either way; the field's only authority is to drop a
/// loop WHOLE when its entire swept band is certified void — uncertainty
/// keeps the loop, and the ramp entry rides the innermost KEPT loop.
pub fn clear(p: &Part, op: &Operation, rest: Option<&VoxField>) -> Result<Vec<String>, String> {
    // Region decomposition is its own generation path (DECISIONS.md
    // "adaptive clearing, region decomposition…"): the field SHAPES the
    // work-list rather than filtering the dense family, so it shares the
    // emission machinery below but not the family.
    if op.strategy == Some(crate::parse::ClearStrategy::Regions) {
        let field = rest.expect("gcode built the before-op field for strategy=regions");
        return clear_regions(p, op, field);
    }
    let mut g = Vec::new();
    // Feature existence/type/point-count and stepover were checked by
    // parse::validate(); violations here are internal invariant bugs.
    let feature = p.features.get(&op.feature_name).expect("feature must exist");

    // Clearing AROUND standing material is its own generation path: the bound
    // is an island rather than a wall, it grows as the tool descends, and the
    // outer edge is derived rather than declared (DECISIONS.md → "roughing a
    // convex revolve…", forks 1A and 3A). It shares the emission machinery
    // below and nothing else.
    if crate::parse::derive_revolve_datum(feature).is_some() {
        return clear_revolve(p, op, feature);
    }
    // A floor whose wall is the stock (Phase 61 slice 1): the volume above a
    // Plane or a Face, around whatever stands in it.
    if matches!(feature, Feature::Plane { .. } | Feature::Face { .. }) {
        return volume_pass(p, op);
    }
    if matches!(feature, Feature::ProfileBoss { .. } | Feature::ContourBoss { .. }) {
        return clear_prismatic_boss(p, op, feature);
    }

    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("Clear operation requires stepover");

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Roughing allowance (DECISIONS.md "stock_to_leave is a radial allowance…").
    // A pocket's wall is the only surface here defined by geometry — the floor
    // is the feature's own `extent` — so the whole allowance becomes radial and
    // the cusp budget is structurally zero: every loop sits at one Z, so no
    // adjacent PAIR leaves a cusp, and a flat tool (required by validate) leaves
    // no floor scallop either. Same shape as the flank carriers' zero budget.
    let skin = op.stock_to_leave.unwrap_or(0.0);

    // ---- the offset family, per feature arm: `loops` is the flattened pass
    // list in execution order (innermost-out), each a closed CCW polyline.
    // An islanded pocket's two extra facts (Phase 26 slice 2): the comment
    // line naming what it goes around, and which loops RAMP — every other
    // family ramps its first loop and plunges the rest.
    // A pocket walked in parallel passes (Phase 28 slice 3) shares nothing with
    // the families below but its region, its floor and its title.
    if let (Some(pattern), Feature::ProfilePocket { z, extent, .. }) = (parallel_pattern(p, op), feature) {
        let region = crate::region::of(p, op).ok_or_else(|| format!("pocket feature '{}' lies outside the stock", op.feature_name))?;
        let floor_skin = crate::parse::derive_floor_skin(p, op);
        let z_cut = derive_z_cut(derive_contour_top(p.stock.z, *z), *extent) + floor_skin;
        let mut g = vec![format!("(--- pocket clear: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0))];
        g.extend(region_notes(&region, true));
        g.extend(bridged_note(p, op));
        if floor_skin > 0.0 {
            g.push(format!("(floor: {} left for the End op that follows)", f(floor_skin)));
        }
        // The rows descend from the pocket's rim, as the offset family does.
        let top = derive_contour_top(p.stock.z, *z);
        let zs = level_zs(top, z_cut, op.stepdown);
        let mut ceiling = top;
        for z_k in &zs {
            if zs.len() > 1 {
                g.push(format!("(level z{})", f(*z_k)));
            }
            g.extend(pattern_pass(p, op, &region, pattern, skin, *z_k, ceiling, ceiling == top)?);
            ceiling = *z_k;
        }
        return Ok(g);
    }

    // The ONE allowance, left on the floor too when a later `Floor` on this
    // pocket will take it (`parse::derive_floor_skin`); zero otherwise, and a
    // floor plus zero is the floor every golden already cut to.
    let floor_skin = crate::parse::derive_floor_skin(p, op);
    let mut island_plan: Option<(Vec<String>, Vec<Entry>)> = None;
    let (title, z_cut, loops, feeds) = match feature {
        // The pocket that is more than a closed outline (Phase 26: it goes
        // around islands; Phase 27: some of its edges are OPEN) is cut on the
        // shape family. A closed, empty pocket — every other pocket shipped —
        // takes the arm below, byte for byte what it always emitted.
        // A pocket whose floor GREW into given air (Phase 42 slice 3) is
        // cut on the same family: its outline is the region's, not the
        // authored points. So is one with given air IN PLAY at all (slice
        // 4) — a bridged given void is an entry too, and only the shape
        // family asks for air.
        Feature::ProfilePocket { z, extent, .. }
            if crate::region::of(p, op).is_some_and(|reg| !reg.islands().is_empty() || reg.open_edges() > 0 || given_in_play(p, op, &reg)) =>
        {
            let z_top = derive_contour_top(p.stock.z, *z);
            let z_cut = derive_z_cut(z_top, *extent) + floor_skin;
            let region = crate::region::of(p, op).expect("guarded");
            let family = region_pocket_family(p, op, &region, r, skin, stepover)?;
            let level_feed = |k: usize| op.pass_feeds.as_ref().and_then(|v| v.get(k).copied()).unwrap_or(feed);
            let feeds: Vec<f64> = family.level_of.iter().map(|&k| level_feed(k)).collect();
            let mut notes = Vec::new();
            let names = region.feature_islands();
            if !names.is_empty() {
                notes.push(format!("(islands: {})", names.join(", ")));
            }
            if !region.keep_outs().is_empty() {
                notes.push(format!("(keep-outs: {})", region.keep_outs().join(", ")));
            }
            if region.open_edges() > 0 {
                notes.push(format!("(open edges: {} of {} — entered from air)", region.open_edges(), region.edges.len()));
            }
            notes.extend(grown_note(&region));
            notes.extend(shore_note(&region));
            notes.extend(family.entered.iter().cloned());
            notes.extend(family.conceded.iter().cloned());
            island_plan = Some((notes, family.ramps));
            let title = format!("(--- pocket clear: {} with T{}{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0), thinning_suffix(op));
            (title, z_cut, family.loops, feeds)
        }
        Feature::ProfilePocket { points, z, extent, .. } => {
            let z_top = derive_contour_top(p.stock.z, *z);
            let z_cut = derive_z_cut(z_top, *extent) + floor_skin;

            let all_loops = plain_pocket_levels(op, points, r, skin, stepover)?;
            // The feed word of each emitted loop: its level's own word when
            // the op carries one per pass (a chip-thinned feed riding a
            // per-pass engagement — Phase 9 lane A, slice 1), the op's word
            // otherwise. A level the offsetter split into several loops
            // shares one word: the engagement was read as the worst over
            // the level.
            let level_feed = |k: usize| op.pass_feeds.as_ref().and_then(|v| v.get(k).copied()).unwrap_or(feed);
            let feeds: Vec<f64> = all_loops
                .iter()
                .enumerate()
                .flat_map(|(k, level)| std::iter::repeat(level_feed(k)).take(level.len()))
                .collect();
            let flat: Vec<Vec<Pt>> = all_loops.into_iter().flatten().collect();

            let title = format!("(--- pocket clear: {} with T{}{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0), thinning_suffix(op));
            (title, z_cut, flat, feeds)
        }
        // The circular pocket (DECISIONS.md "adaptive clearing…", slice 1's
        // feature coverage): the offset family is concentric circles riding
        // the declared boundary — radii from `R − r − skin` inward by
        // `stepover` while one still fits — chord-flattened through the same
        // `circle_pts` home the finish waterline rings use, so the emitted
        // loop and a finish ring at the same radius are one curve. Emission
        // then runs through the identical innermost-out machinery as the
        // polyline pocket: same ramp entry, same per-loop blocks.
        Feature::CylinderBore { dia, extent, x, y, z, .. } => {
            let z_cut = derive_z_cut(derive_contour_top(p.stock.z, *z), *extent);
            let r_wall = dia / 2.0 - r - skin;
            if r_wall <= 0.0 {
                if skin > 0.0 {
                    return Err(format!(
                        "bore feature '{}' is too small for tool T{} plus the stock_to_leave offset ({:.3}mm): no pass fits inside it — reduce stock_to_leave or use a smaller tool",
                        op.feature_name, op.tool_num, skin
                    ));
                }
                return Err(format!(
                    "bore feature '{}' is too small for tool T{}: no pass fits inside it",
                    op.feature_name, op.tool_num
                ));
            }
            let mut radii = Vec::new();
            let mut rho = r_wall;
            while rho > 0.0 {
                radii.push(rho);
                rho -= stepover;
            }
            // The pocket's island error on the circular family: the
            // innermost pass clears inward to `r0 − r`, and material inside
            // that circle is a full-depth core no pass touches.
            let r0 = *radii.last().expect("r_wall > 0 pushed at least one radius");
            if r0 - r > 0.0 {
                return Err(format!(
                    "bore feature '{}' leaves an uncut core: tool T{}'s innermost pass reaches {:.3}mm from the axis but material remains inside it — reduce stepover (no allowance can ceiling stock a pass never touches)",
                    op.feature_name, op.tool_num, r0 - r
                ));
            }
            let flat: Vec<Vec<Pt>> = radii
                .iter()
                .rev() // innermost-out
                .map(|&rho| {
                    let mut poly = vec![Pt { x: x + rho, y: *y }];
                    let ring = crate::ops::finish::circle_pts(*x, *y, rho);
                    poly.extend(ring[..ring.len() - 1].iter().map(|&(px, py)| Pt { x: px, y: py }));
                    poly
                })
                .collect();
            let title = format!("(--- bore clear: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0));
            let feeds = vec![feed; flat.len()];
            (title, z_cut, flat, feeds)
        }
        _ => panic!("Clear operation requires a ProfilePocket or CylinderBore feature"),
    };

    // ---- rest selection (whole loops, nothing else): a loop goes only when
    // its entire swept band at the cutting depth is certified void by the
    // before-this-op field. The certifier's τ-law means tangency never
    // certifies — a wall pass grazing its own wall stays, as a spring pass.
    let total = loops.len();
    // Loops and their words travel together through the rest filter, so a
    // dropped loop drops its own word and nothing shifts.
    let keep: Vec<bool> = match rest {
        None => vec![true; loops.len()],
        Some(field) => loops
            .iter()
            .map(|poly| {
                let pts: Vec<(f64, f64)> = poly.iter().map(|pt| (pt.x, pt.y)).collect();
                !field.band_certified_void(&pts, r, z_cut)
            })
            .collect(),
    };
    let kept_ramps: Option<Vec<Entry>> =
        island_plan.as_ref().map(|(_, ramps)| ramps.iter().zip(&keep).filter(|(_, &k)| k).map(|(&ramp, _)| ramp).collect());
    let (kept, kept_feeds): (Vec<Vec<Pt>>, Vec<f64>) =
        loops.into_iter().zip(feeds).zip(&keep).filter(|(_, &k)| k).map(|(pair, _)| pair).unzip();

    g.push(title);
    if let Some((notes, _)) = &island_plan {
        g.extend(notes.iter().cloned());
    }
    g.extend(crate::region::of(p, op).as_ref().and_then(open_void_note));
    g.extend(bridged_note(p, op));
    if floor_skin > 0.0 {
        g.push(format!("(floor: {} left for the End op that follows)", f(floor_skin)));
    }
    if let Some(field) = rest {
        if kept.is_empty() {
            // "Nothing stands" is the truthful output of a rest op — an
            // answer, not an error, and distinct from the unmachinable-feature
            // hard errors above. Stated in the header so the G-code reads
            // as a decision rather than an omission; no cutting moves follow.
            g.push(format!(
                "(rest: nothing stands — all {} loops of the family certified void against the stock before this op at resolved cell {}mm; no cutting moves)",
                total,
                f(field.cell()),
            ));
            return Ok(g);
        }
        // The audit line: how much of the family the field kept, so a
        // dropped-loop program says so in its own text.
        g.push(format!(
            "(rest: kept {} of {} loops against the stock before this op at resolved cell {}mm)",
            kept.len(),
            total,
            f(field.cell()),
        ));
    }
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));

    // Steel in the region: lift to the clearance plane between loops, never
    // to `rapid_z` — that is over the stock and under a clamp (`ops::floor`'s
    // own note). No keep-out, no change: every golden shipped keeps its lifts.
    let lift = match crate::region::of(p, op) {
        Some(region) if !region.keep_outs().is_empty() => clearance_z,
        _ => rapid_z,
    };
    // The descent starts at the feature's RIM (Phase 45): a void authored
    // from below the stock top has air — or another op's removal — above
    // it, and a ramp priced from the stock top would lap through that air
    // and hold its angle over a depth the material does not have. With no
    // `z` the rim IS the stock top, so nothing shipped moves.
    let z_from = match feature {
        Feature::ProfilePocket { z, .. } | Feature::CylinderBore { z, .. } => derive_contour_top(p.stock.z, *z),
        _ => p.stock.z,
    };
    // One block per level (Phase 61 slice 2): each level is the family again
    // at its own z, its first loop descending from the level above. With no
    // `stepdown` the stack is the floor alone and the text is what it was.
    let zs = level_zs(z_from, z_cut, op.stepdown);
    let mut ceiling = z_from;
    for z in &zs {
        if zs.len() > 1 {
            g.push(format!("(level z{} — {} pass{})", f(*z), kept.len(), if kept.len() == 1 { "" } else { "es" }));
        }
        emit_loop_blocks_ramped(&mut g, op, &kept, kept_ramps.as_deref(), *z, ceiling, &kept_feeds, plunge, lift, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;
        ceiling = *z;
    }

    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// **A family that goes around islands takes a stepover of at most the tool's
/// RADIUS** (Phase 26 slice 3, measured on the face; `DECISIONS.md` → "Phase 26
/// slice 3: the face…"). Between an island and the wall beside it runs a RIDGE
/// — the points equidistant from both — and the levels cross it at isolated
/// points, not along it. A ridge point `r + ε` deeper than level `k` is out of
/// that level's reach, and level `k+1` meets the ridge somewhere else along
/// it; with `stepover > r` there is room between them, and a fin stands that
/// no pass touches (243.9 mm³ on `face_boss_offset.cam` at 8mm on a 12mm tool).
/// With `stepover <= r` there is none: every point deeper than a level's
/// reach is already inside the next level's region, by induction down to a
/// last level whose reach covers what is left. That induction is also why the
/// plain pocket's "leaves an uncut island" check has no twin here — under this
/// law it cannot fire.
pub(crate) fn island_stepover_law(what: &str, tool_num: usize, r: f64, stepover: f64) -> Result<(), String> {
    if stepover > r + 1e-9 {
        return Err(format!(
            "{what} goes around islands, and its stepover ({}) is more than T{}'s radius ({}): between an island and the wall beside it the passes would leave a fin no pass touches — reduce stepover to {} or less, or walk it in parallel passes (pattern=zigzag), which have no ridge between them",
            f(stepover),
            tool_num,
            f(r),
            f(r)
        ));
    }
    Ok(())
}

/// **…and so does a family whose own WALL points into the floor** (Phase 37,
/// ruled 2026-10-01; `DECISIONS.md` → "Phase 37 rulings…"). A wall corner
/// that turns into the floor — an L-pocket's inside corner, a bed bulging
/// into a lawn — pinches the family exactly as an island does: past the
/// pinch the next level is further off than its stepover, and with
/// `stepover > r` floor between one level's reach and the next level is
/// stranded (1.3mm² at the mouth of an L's arm, 7mm on a 10mm tool). The
/// island law's induction never mentioned islands: every point deeper than a
/// level's reach is inside the next level's region whenever `stepover <= r`,
/// for any shape. So the same law, asked of the family's own boundary, and
/// naming where that boundary turns in most sharply.
pub(crate) fn inward_wall_stepover_law(what: &str, tool_num: usize, wall: &[Pt], r: f64, stepover: f64) -> Result<(), String> {
    if stepover <= r + 1e-9 {
        return Ok(());
    }
    let n = wall.len();
    let area: f64 = (0..n).map(|i| wall[i].x * wall[(i + 1) % n].y - wall[(i + 1) % n].x * wall[i].y).sum();
    let mut sharpest: Option<(f64, Pt)> = None;
    for i in 0..n {
        let (a, v, b) = (wall[(i + n - 1) % n], wall[i], wall[(i + 1) % n]);
        let (ux, uy, wx, wy) = (v.x - a.x, v.y - a.y, b.x - v.x, b.y - v.y);
        let (lu, lw) = (ux.phypot(uy), wx.phypot(wy));
        if lu < 1e-9 || lw < 1e-9 {
            continue;
        }
        // Against the way the wall winds: the corner points INTO the floor.
        // (A millionth of a radian is a straight wall drawn with a vertex on it.)
        let turn = ((ux * wy - uy * wx) * area.signum()).patan2(ux * wx + uy * wy);
        if turn < -1e-6 && sharpest.map_or(true, |(t, _)| turn < t) {
            sharpest = Some((turn, v));
        }
    }
    let Some((_, at)) = sharpest else { return Ok(()) };
    Err(format!(
        "{what}: its boundary turns into the floor at ({}, {}), and its stepover ({}) is more than T{}'s radius ({}): past that corner the passes would strand floor no pass touches — reduce stepover to {} or less, or walk it in parallel passes (pattern=zigzag), which have no ridge between them",
        f(at.x),
        f(at.y),
        f(stepover),
        tool_num,
        f(r),
        f(r)
    ))
}

/// **Where nothing stands, the offset family can still leave floor in two
/// places, and refuses both by name** (Phase 37 slice 2; `DECISIONS.md` →
/// "Phase 37 slice 2…"). The family steps inward until the next offset prunes
/// empty, so its last loop may stop short of the MIDDLE: this is that
/// refusal's one sentence, said by the closed pocket, the islandless region
/// and — since Phase 37 — the bare plane, which had no such check and left a
/// 4 x 4mm square in the middle of a 60mm face at 7mm on a 10mm tool.
///
/// Every arm asks whether floor remains a hair BEYOND the reach
/// (`MIDDLE_HAIR`). Asked at the reach itself, a last pass that lands exactly
/// on the floor's centreline is refused for the centreline — a strip with no
/// width, which the offsetter still hands back as a loop: `face_boss_offset`'s
/// skim, 8mm on a 12mm tool across an 80mm blank, reaches 40.000 of 40. (The
/// two pocket arms asked at the reach itself until the inward-wall law
/// landed: one such refusal stood in front of floor stranded on a ridge, and
/// the hair could only go in together with the law that refuses the ridge.)
pub(crate) const MIDDLE_HAIR: f64 = crate::stock::EMITTED_MM_HALF_ULP;

pub(crate) fn uncut_middle(what: &str, tool_num: usize, reach: f64, from: &str) -> String {
    format!(
        "{what} leaves an uncut island: tool T{tool_num}'s last pass reaches {reach:.3}mm from {from} but material remains beyond it — reduce stepover (no allowance can ceiling stock a pass never touches)"
    )
}

/// …and the CORNER. Two loops a stepover apart round a corner of half-angle
/// `a` leave floor on its bisector exactly when `stepover > r(1 + sin a)`:
/// the outer loop reaches `r` past itself, the inner loop's own corner
/// reaches `r` back toward it, and the corners are `stepover / sin a` apart.
/// A square corner gives `1.707r`, 85% of the tool; a straight wall gives
/// `2r`, the diameter `validate` already refuses; and as the corner closes
/// to nothing it gives `r` — the ridge law, of which this is the wide-open
/// end. Judged on the family's own wall, at its sharpest corner.
///
/// (A corner so short-sided that the first offset swallows it is judged as
/// if it survived: the law can refuse a stepover over the radius that would
/// have covered, never pass one that does not.)
pub(crate) fn corner_stepover_law(what: &str, tool_num: usize, wall: &[Pt], r: f64, stepover: f64) -> Result<(), String> {
    let n = wall.len();
    let area: f64 = (0..n).map(|i| wall[i].x * wall[(i + 1) % n].y - wall[(i + 1) % n].x * wall[i].y).sum();
    let mut sharpest: Option<(f64, Pt)> = None;
    for i in 0..n {
        let (a, v, b) = (wall[(i + n - 1) % n], wall[i], wall[(i + 1) % n]);
        let (ux, uy, wx, wy) = (v.x - a.x, v.y - a.y, b.x - v.x, b.y - v.y);
        let (lu, lw) = (ux.phypot(uy), wx.phypot(wy));
        // A corner of the floor turns the way the wall winds; one that turns
        // the other way points INTO the floor and has no bisector to strand.
        if lu < 1e-9 || lw < 1e-9 || (ux * wy - uy * wx) * area <= 0.0 {
            continue;
        }
        let turn = ((ux * wx + uy * wy) / (lu * lw)).clamp(-1.0, 1.0).pacos();
        let sin_half = ((std::f64::consts::PI - turn) / 2.0).psin();
        if sharpest.map_or(true, |(s, _)| sin_half < s) {
            sharpest = Some((sin_half, v));
        }
    }
    let Some((sin_half, at)) = sharpest else { return Ok(()) };
    let most = r * (1.0 + sin_half);
    if stepover > most + 1e-9 {
        return Err(format!(
            "{what}: its stepover ({}) is more than T{}'s passes can close round the corner at ({}, {}): two loops that far apart leave floor between them on the corner's bisector — reduce stepover to {} or less",
            f(stepover),
            tool_num,
            f(at.x),
            f(at.y),
            f((most * 1000.0).floor() / 1000.0)
        ));
    }
    Ok(())
}

/// The op's statement of what it goes AROUND without cutting it (Phase 28
/// slice 2): a void the tool can stand in clear of its rim is an open inner
/// edge — the path overhangs the rim and never crosses the void at feed.
pub(crate) fn open_void_note(region: &crate::region::Region) -> Option<String> {
    let mut names = region.open_voids();
    names.dedup();
    (!names.is_empty()).then(|| format!("(open onto: {})", names.join(", ")))
}

/// The op's statement of what it runs OVER (Phase 27, fork 4A): every declared
/// void whose mouth is in this floor is BRIDGED — the path crosses it as if it
/// were floor, cut or not — and the program says so, beside `(islands: …)`.
/// It is what the engine always did; nothing traces a gap's edge. Empty for a
/// floor with no void in it, which is every golden shipped before this line.
pub(crate) fn bridged_note(p: &Part, op: &Operation) -> Option<String> {
    let region = crate::region::of(p, op)?;
    let bridged = region.bridged();
    (!bridged.is_empty()).then(|| format!("(bridged: {})", bridged.join(", ")))
}

/// `Floor` on a `ProfilePocket`: the pocket's FLOOR finished to its declared
/// height (Phase 27 slice 4, fork 6A). It is the pocket's own family at the
/// true floor — the plain spiral for a closed, empty pocket; the region family
/// around islands, keep-outs and open edges otherwise — so the floor pass goes
/// exactly where the rough went, and `stock_to_leave` on it is what it is on
/// every op: the radial stand-off from the walls. It descends to the height the
/// rough left (`z_from`), never through air from the stock's top, and takes
/// what is there; with no allowance left it is a spring pass.
pub(crate) fn pocket_floor_pass(p: &Part, op: &Operation) -> Result<Vec<String>, String> {
    let Some(Feature::ProfilePocket { points, z, extent, .. }) = p.features.get(&op.feature_name) else {
        panic!("validate: an End's floor is a Plane's or a ProfilePocket's");
    };
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("End operation requires stepover");
    let skin = op.stock_to_leave.unwrap_or(0.0);
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
    let z_cut = derive_z_cut(derive_contour_top(p.stock.z, *z), *extent);
    let left = crate::parse::derive_floor_left(p, op);

    let region = crate::region::of(p, op).expect("an End on a pocket works its floor");
    if let Some(pattern) = parallel_pattern(p, op) {
        let mut g = vec![format!("(--- floor: {} floor with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0))];
        g.extend(region_notes(&region, true));
        g.extend(bridged_note(p, op));
        g.push(format!("(floor: takes the {} the rough left)", f(left)));
        g.extend(pattern_pass(p, op, &region, pattern, skin, z_cut, z_cut + left, true)?);
        return Ok(g);
    }
    let (loops, entries, notes) = if region.islands().is_empty() && region.open_edges() == 0 && !given_in_play(p, op, &region) {
        let flat: Vec<Vec<Pt>> = plain_pocket_levels(op, points, r, skin, stepover)?.into_iter().flatten().collect();
        (flat, None, Vec::new())
    } else {
        let family = region_pocket_family(p, op, &region, r, skin, stepover)?;
        let mut notes = Vec::new();
        if !region.feature_islands().is_empty() {
            notes.push(format!("(islands: {})", region.feature_islands().join(", ")));
        }
        if !region.keep_outs().is_empty() {
            notes.push(format!("(keep-outs: {})", region.keep_outs().join(", ")));
        }
        if region.open_edges() > 0 {
            notes.push(format!("(open edges: {} of {} — entered from air)", region.open_edges(), region.edges.len()));
        }
        notes.extend(grown_note(&region));
        notes.extend(shore_note(&region));
        notes.extend(family.entered.iter().cloned());
        notes.extend(family.conceded.iter().cloned());
        (family.loops, Some(family.ramps), notes)
    };

    let mut g = Vec::new();
    g.push(format!("(--- floor: {} floor with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.extend(notes);
    g.extend(open_void_note(&region));
    g.extend(bridged_note(p, op));
    g.push(format!("(floor: takes the {} the rough left)", f(left)));
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));
    let lift = if region.keep_outs().is_empty() { rapid_z } else { clearance_z };
    emit_loop_blocks_ramped(&mut g, op, &loops, entries.as_deref(), z_cut, z_cut + left, &vec![feed; loops.len()], plunge, lift, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// `Floor` on a `Face` (Phase 28 slice 1; `DECISIONS.md` → "Phase 28 slice 1:
/// the Face…"): the bounded planar face, cut on the same region family as a
/// pocket — its outline pushed out one tool diameter all round, because every
/// edge of a face is open; deepest level first; islands and keep-outs gone
/// around; a lead-in from air where the blank's edge is in reach and a ramp ON
/// the face where it is not (the stock beside a face on a bar is not this
/// op's to plunge into — it removes the column over its own loops and nothing
/// else). One pass to depth, as every `Floor` is.
pub(crate) fn face_pass(p: &Part, op: &Operation) -> Result<Vec<String>, String> {
    let Some(Feature::Face { z, .. }) = p.features.get(&op.feature_name) else {
        panic!("validate: face_pass is a Face's");
    };
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("End operation requires stepover");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
    let region = crate::region::of(p, op).ok_or_else(|| format!(
        "Face feature '{}' lies outside the stock — there is nothing of it to face",
        op.feature_name
    ))?;
    if let Some(pattern) = parallel_pattern(p, op) {
        let mut g = vec![format!("(--- floor: {} with T{} ---)", op.feature_name, op.tool_num)];
        g.extend(region_notes(&region, false));
        g.extend(bridged_note(p, op));
        g.extend(pattern_pass(p, op, &region, pattern, 0.0, *z, p.stock.z, true)?);
        return Ok(g);
    }
    let family = region_pocket_family(p, op, &region, r, 0.0, stepover)?;

    let mut g = Vec::new();
    g.push(format!("(--- floor: {} with T{} ---)", op.feature_name, op.tool_num));
    if !region.feature_islands().is_empty() {
        g.push(format!("(islands: {})", region.feature_islands().join(", ")));
    }
    g.extend(shore_note(&region));
    if !region.keep_outs().is_empty() {
        g.push(format!("(keep-outs: {})", region.keep_outs().join(", ")));
    }
    g.extend(open_void_note(&region));
    g.extend(bridged_note(p, op));
    g.extend(family.conceded.iter().cloned());
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));
    let lift = if region.keep_outs().is_empty() { rapid_z } else { clearance_z };
    emit_loop_blocks_ramped(&mut g, op, &family.loops, Some(&family.ramps), *z, p.stock.z, &vec![feed; family.loops.len()], plunge, lift, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// `Clear` on a `Plane` or a `Face` (Phase 61 slice 1; `DECISIONS.md` →
/// "Phase 61 slice 1…"): the VOLUME above a floor whose wall is the stock,
/// roughed down to the floor's z — one level until slice 2 — around whatever
/// stands in it. The region is `region::of`'s (the blank, or the face's outer
/// loop, every edge open; a standing feature an island, a boss on the shore a
/// bite), the cut is the shape family the End's floor uses, and the one
/// allowance stands on every flank of what it goes around — the island's skin
/// the next `Side` consumes. The FLOOR is left its allowance only when an End
/// on the same feature follows (`parse::derive_floor_skin`): nothing declares
/// a floor that serves the part, so nothing is left on one.
pub(crate) fn volume_pass(p: &Part, op: &Operation) -> Result<Vec<String>, String> {
    let (z, noun) = match p.features.get(&op.feature_name) {
        Some(Feature::Plane { z }) => (*z, "Plane"),
        Some(Feature::Face { z, .. }) => (*z, "Face"),
        _ => panic!("validate: volume_pass is a Plane's or a Face's"),
    };
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("Clear operation requires stepover");
    let skin = op.stock_to_leave.unwrap_or(0.0);
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
    let floor_skin = crate::parse::derive_floor_skin(p, op);
    let z_cut = z + floor_skin;
    let region = crate::region::of(p, op).ok_or_else(|| format!(
        "{noun} feature '{}' lies outside the stock — there is no volume of it to clear",
        op.feature_name
    ))?;
    let title = format!("(--- volume clear: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0));
    if let Some(pattern) = parallel_pattern(p, op) {
        let mut g = vec![title];
        g.extend(region_notes(&region, false));
        g.extend(bridged_note(p, op));
        if floor_skin > 0.0 {
            g.push(format!("(floor: {} left for the End op that follows)", f(floor_skin)));
        }
        let zs = level_zs(p.stock.z, z_cut, op.stepdown);
        let mut ceiling = p.stock.z;
        for z_k in &zs {
            if zs.len() > 1 {
                g.push(format!("(level z{})", f(*z_k)));
            }
            g.extend(pattern_pass(p, op, &region, pattern, skin, *z_k, ceiling, ceiling == p.stock.z)?);
            ceiling = *z_k;
        }
        return Ok(g);
    }
    let family = region_pocket_family(p, op, &region, r, skin, stepover)?;
    let mut g = vec![title];
    if !region.feature_islands().is_empty() {
        g.push(format!("(islands: {})", region.feature_islands().join(", ")));
    }
    g.extend(shore_note(&region));
    if !region.keep_outs().is_empty() {
        g.push(format!("(keep-outs: {})", region.keep_outs().join(", ")));
    }
    g.extend(open_void_note(&region));
    g.extend(bridged_note(p, op));
    g.extend(family.conceded.iter().cloned());
    if floor_skin > 0.0 {
        g.push(format!("(floor: {} left for the End op that follows)", f(floor_skin)));
    }
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));
    let lift = if region.keep_outs().is_empty() { rapid_z } else { clearance_z };
    let zs = level_zs(p.stock.z, z_cut, op.stepdown);
    let mut ceiling = p.stock.z;
    for z_k in &zs {
        if zs.len() > 1 {
            g.push(format!("(level z{} — {} pass{})", f(*z_k), family.loops.len(), if family.loops.len() == 1 { "" } else { "es" }));
        }
        emit_loop_blocks_ramped(&mut g, op, &family.loops, Some(&family.ramps), *z_k, ceiling, &vec![feed; family.loops.len()], plunge, lift, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;
        ceiling = *z_k;
    }
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// The boundary a region's paths are laid inside, and what they go around —
/// one home for the offset family and the parallel patterns alike: the blank
/// grown a tool diameter for a `Plane`; the region pushed out across its open
/// edges otherwise (all of them, for a `Face`); a keep-out that straddles the
/// result a bite out of it, everything else inside it an island.
///
/// The third element is what the swath closed over whole (Phase 32, after the close): already
/// among the islands, and handed back apart because it STANDS — a pass rebuilt
/// from `region.standing()` alone would run straight through it.
// **The allowance belongs to walls and islands, never to an open edge**
// (Phase 61 slice 1, found by the Plane Clear). The family erodes the
// boundary by `r + skin`; an open edge is pushed out a diameter so the
// outermost loop rides a radius beyond it — and with the allowance eroded
// off the same push, the tool stopped `skin` short of the face's own edge,
// leaving a strip along every open edge that nothing stands in and nothing
// will ever take. The push carries the allowance (`2r + skin`), so it
// cancels on an open edge and stands on a wall.
fn family_boundary(p: &Part, op: &Operation, region: &crate::region::Region, r: f64, skin: f64) -> Result<(Vec<Pt>, Vec<crate::offset::Island>, Vec<crate::offset::Island>), String> {
    let mut enclosed = Vec::new();
    // A Plane's floor is the blank and every edge of it is open, so its
    // family boundary is the blank grown a diameter — UNLESS a boss standing
    // on the shore was bitten out of it (Phase 49 slice 1), whose flanks are
    // WALLS the region already carries. Until Phase 60 slice 2 this branch
    // grew the whole blank regardless, and the rows ran a radius INTO the
    // boss along every flank perpendicular to them (`plate_boss.cam`:
    // 6 300 mm³ of the plate swept, the rest report's `violated`). A Plane
    // with a wall takes the region's own family wall, as a pocket does.
    let host_is_plane = matches!(p.features.get(&op.feature_name), Some(Feature::Plane { .. }));
    let wall = if host_is_plane && region.open_edges() == region.edges.len() {
        crate::parse::derive_blank_grown(&p.stock, 2.0 * r + skin)
    } else if region.open_edges() > 0 {
        let swath = region.family_wall(2.0 * r + skin).ok_or_else(|| format!(
            "feature '{}': its open edges could not be pushed out past the blank into one outline — the region folds back on itself at the stock's edge",
            op.feature_name
        ))?;
        enclosed = swath.enclosed_islands();
        swath.wall
    } else {
        region.outer.clone()
    };
    let (wall, mut islands) = crate::region::split_islands(wall, &region.islands())
        .ok_or_else(|| format!("feature '{}': a keep-out across its boundary leaves no simple region to cut", op.feature_name))?;
    islands.extend(enclosed.iter().cloned());
    Ok((wall, islands, enclosed))
}

/// Where the tool's CENTRE may be while it works this region: the family's
/// outermost level, as flat loops (`ccw` bound it, `cw` are what it goes
/// around). One home — the parallel patterns clip their passes to it, and the
/// comparison report's `uncut` column asks it what a tool this size could have
/// reached at all (Phase 30 slice 1).
pub fn allowed_loops(p: &Part, op: &Operation, region: &crate::region::Region, r: f64, skin: f64) -> Result<(Vec<Vec<Pt>>, Vec<Vec<Pt>>), String> {
    let (wall, islands, _) = family_boundary(p, op, region, r, skin)?;
    let level = crate::offset::offset_shape(&wall, &islands, r + skin);
    let flat = |ls: &[Vec<crate::offset::OffsetVertex>]| ls.iter().map(|l| crate::offset::flatten_loop(l)).collect::<Vec<_>>();
    Ok((flat(&level.ccw), flat(&level.cw)))
}

/// How steep ANY entry into solid stock may be (Phase 44 slice 1; born
/// `SPINE_RAMP_DEG` in Phase 30 slice 4, spine's alone): three degrees, a
/// slope any end mill takes in any metal. A CEILING, not a target — a first
/// loop or stroke longer than the ramp needs descends over its whole length
/// as it always did, shallower than this; one too short LAPS until the
/// angle is met (`emit_loop_blocks_ramped`); one under a diameter cannot
/// ramp at all and plunges at the plunge feed, said by name. The zigzag
/// ramps along its WHOLE first stroke and then cuts it again; on a floor of
/// long thin passes that doubles the longest pass in the program (slice 2).
pub const RAMP_DEG: f64 = 3.0;

/// The room a ramp needs to lose `depth` at `RAMP_DEG`.
pub fn ramp_room(depth: f64) -> f64 {
    depth.max(0.0) / RAMP_DEG.to_radians().ptan()
}

/// How far under half the narrowest land the hairline is eroded (Phase 31):
/// two hundredths, `CHORD_TOL_MM` twice over, so a rail's two sides come back
/// as two loops and never as one collapsed one.
const HAIR: f64 = 0.02;

/// `pattern=spine` (Phase 30; `DECISIONS.md` → "Phase 30 slice 2: mow…", its name until Phase 35): what
/// `spine::plan` is asked, gathered from the one home each fact already has —
/// where the centre may be (`allowed_loops`), the floor and what stands beside
/// it (`region::of`), air (the blank). `at` pins one comb — direction, phase,
/// walked down — instead of choosing; the tests sweep with it.
pub fn spine_plan(p: &Part, op: &Operation, region: &crate::region::Region, skin: f64, (lift_cost, ramp_len): (f64, f64), at: Option<(f64, f64, bool)>) -> Result<crate::spine::Plan, String> {
    spine_plan_with(p, op, region, skin, (lift_cost, ramp_len), at.map(|(a, ph, d)| (a, (ph, d), None)), None)
}

/// One PAIR of combs pinned: the primary, and the comb at right angles.
pub fn spine_plan_crossed(p: &Part, op: &Operation, region: &crate::region::Region, skin: f64, costs: (f64, f64), angle: f64, primary: (f64, bool), cross: (f64, bool)) -> Result<crate::spine::Plan, String> {
    spine_plan_with(p, op, region, skin, costs, Some((angle, primary, Some(cross))), None)
}

/// The plan `pattern=held` cuts at the level `z` (Phase 62 slice 1): spine's
/// comb at the spacing the op's ceiling derives, walked from the air, its
/// links read through the walk against the stock before the op at that
/// level, the places over the ceiling read back and carried in `Plan::held`.
pub fn held_plan(p: &Part, op: &Operation, region: &crate::region::Region, skin: f64, costs: (f64, f64), z: f64) -> Result<crate::spine::Plan, String> {
    spine_plan_with(p, op, region, skin, costs, None, Some(z))
}

/// The floor spine and the parallel passes name their directions by: the
/// region's outer boundary, less a `Face`'s own inner loops (its voids are
/// not floor), less everything standing in it.
/// What STANDS beside a floor, as segments: the region's wall edges, and
/// every standing polygon in it — an island, a keep-out, a clamp biting the
/// blank. The one list a shore pass is asked to run along (`spine::shores`),
/// for spine's plan and the parallel passes' closing pass alike (Phase 40
/// slice 1).
fn standing_segments(region: &crate::region::Region) -> Vec<(Pt, Pt)> {
    let n = region.outer.len();
    let mut walls: Vec<(Pt, Pt)> = (0..n).filter(|i| region.edges[*i] == crate::region::EdgeKind::Wall).map(|i| (region.outer[i], region.outer[(i + 1) % n])).collect();
    for poly in region.standing_polygons() {
        walls.extend((0..poly.len()).map(|i| (poly[i], poly[(i + 1) % poly.len()])));
    }
    walls
}

fn floor_of(p: &Part, op: &Operation, region: &crate::region::Region) -> Vec<Vec<Pt>> {
    let mut floor = vec![region.outer.clone()];
    match p.features.get(&op.feature_name) {
        Some(Feature::Face { loops, .. }) => floor.extend(loops.iter().skip(1).cloned()),
        _ => floor.extend(region.standing_polygons()),
    }
    floor
}

#[allow(clippy::type_complexity)]
fn spine_plan_with(p: &Part, op: &Operation, region: &crate::region::Region, skin: f64, (lift_cost, ramp_len): (f64, f64), at: Option<(f64, (f64, bool), Option<(f64, bool)>)>, held_z: Option<f64>) -> Result<crate::spine::Plan, String> {
    let tool = p.tools.get(&op.tool_num).ok_or("the op's tool is not declared")?;
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).ok_or("a pattern needs a stepover")?;
    let (ccw, cw) = allowed_loops(p, op, region, r, skin)?;
    if ccw.is_empty() {
        return Err(format!("feature '{}' is too small for tool T{}: no pass fits inside it", op.feature_name, op.tool_num));
    }
    let floor = floor_of(p, op, region);
    let walls = standing_segments(region);
    let blank = crate::region::blank_of(p);
    // "Air where it is in reach" for the rows (Phase 44 slice 3, the pool
    // item Phase 42 left): beyond the blank by a radius, OR inside a given
    // void's lens — the centre set the whole body can stand in
    // (`stock::given_air_at`), where the before-state is void and the plunge
    // taxonomy licenses an entry at depth; a point ON the lens's edge is in
    // it (the extreme row's stroke rides that edge, and the certificate
    // blesses it). The one predicate every row pattern reads for its start
    // side, its ramp flags and its shore leads.
    let given_air: Vec<Vec<Pt>> = crate::stock::given_air_at(p, op, region.z).into_iter().flat_map(|l| l.centre).collect();
    // HELD reads the clearance to the chord's tolerance (Phase 62 slice 1):
    // a row's end on the chorded arc of the blank's corner sits a sagitta
    // inside a radius past the edge, and read to a micron it ramped into
    // the air it stood in. Spine and the parallel passes keep the micron:
    // read to the chord, two shipped goldens on round and profile blanks
    // move, and no golden moves in this phase.
    let slack = if held_z.is_some() { crate::CHORD_TOL_MM } else { 1e-3 };
    let in_air = |q: Pt| {
        (!crate::stock::point_in_polygon(&blank, q.x, q.y) && crate::stock::dist_to_polygon(&blank, q.x, q.y) >= r - slack)
            || given_air.iter().any(|piece| crate::stock::point_in_polygon(piece, q.x, q.y) || crate::stock::dist_to_polygon(piece, q.x, q.y) <= crate::pattern::ON_LOOP)
    };
    // The hairline (Phase 31): the floor eroded to a hair under half its
    // narrowest land, where that land is narrow enough for one pass to cover.
    // Read from the same `lands` the report prints — the number an author
    // chooses a tool by is the number a rail pass is made for.
    let narrowest = crate::compare::lands(&floor, tool.dia).first().map(|l| l.width);
    let (level, level_d) = match narrowest.filter(|w| *w < 2.0 * r - 2.0 * HAIR) {
        Some(w) => {
            let d = w / 2.0 - HAIR;
            let islands: Vec<crate::offset::Island> = floor[1..].iter().map(|l| crate::offset::Island::Loop(l.clone())).collect();
            let lvl = crate::offset::offset_shape(&floor[0], &islands, d);
            let flat = |ls: &[Vec<crate::offset::OffsetVertex>]| ls.iter().map(|l| crate::offset::flatten_loop(l)).collect::<Vec<_>>();
            (flat(&lvl.ccw).into_iter().chain(flat(&lvl.cw)).collect::<Vec<_>>(), d)
        }
        None => (Vec::new(), 0.0),
    };
    // HELD (Phase 62 slice 1): the walk's stock — the model before this op,
    // its bounds, its material just above this level — for the links to be
    // read against as they are laid. No walk where the model declines or
    // holds a footprint the walk cannot solve: the spacing alone, said.
    let before = held_z.and_then(|_| p.operations.iter().position(|o| std::ptr::eq(o, op)).map(|i| crate::stock::remaining_before(p, i)));
    let bounds = before.as_ref().filter(|b| b.complete).and_then(crate::engagement::Bounds::of);
    let z_read = held_z.unwrap_or(0.0) + 0.01;
    let material = |q: Pt| before.as_ref().is_some_and(|b| b.material_at(q.x, q.y, z_read));
    let held = held_z.map(|_| crate::spine::Held { ceiling: crate::parse::derive_held_ceiling(op).to_radians(), walk: bounds.as_ref().map(|b| (b, &material as &(dyn Fn(Pt) -> bool + Sync))) });
    let ask = crate::spine::Ask { ccw: &ccw, cw: &cw, floor: &floor, walls: &walls, r, stand_off: r + skin, stepover, lift_cost, ramp_len, in_air: &in_air, level: &level, level_d, held, conventional: !op.climbs(), angle: op.angle };
    match at {
        Some((angle, (phase, descending), None)) => crate::spine::plan_at(&ask, angle, phase, descending),
        Some((angle, primary, Some(cross))) => crate::spine::plan_crossed(&ask, angle, primary, cross),
        None => crate::spine::plan(&ask),
    }
    .ok_or_else(|| format!("feature '{}': no pass fits inside it", op.feature_name))
}

/// The floor's coverage under this op's tool (Phase 32 slice 2): the region's
/// own loops against where the centre may be.
pub fn coverage(p: &Part, op: &Operation, region: &crate::region::Region, r: f64, skin: f64) -> Result<crate::cover::Coverage, String> {
    let (ccw, cw) = allowed_loops(p, op, region, r, skin)?;
    // The floor: its own loops, less everything standing in it — a post's
    // interior is not floor a tool failed to reach.
    let mut floor = match p.features.get(&op.feature_name) {
        Some(Feature::Face { loops, .. }) => loops.clone(),
        _ => vec![region.outer.clone()],
    };
    floor.extend(region.standing_polygons());
    // Judged at the radius PLUS the allowance: the band a rough leaves on
    // purpose beside every wall is intent, not a stretch it could not reach.
    Ok(crate::cover::of(region, &floor, &ccw, &cw, r + skin))
}

/// The CONCESSION (Phase 32 slice 2; `DECISIONS.md` → "Phase 32 slice 2: the
/// concession…"). A floor with a stretch no pass can enter refuses — every
/// stretch named, with its width and the tool that would fit it, and the one
/// word of program text that would emit the rest — unless the run line
/// concedes it with `cover=`, in which case the program carries the note and
/// the concession is checked: it fails the day less is reachable.
pub(crate) fn concession(p: &Part, op: &Operation, region: &crate::region::Region, r: f64, skin: f64) -> Result<Vec<String>, String> {
    let c = coverage(p, op, region, r, skin)?;
    if c.stretches.is_empty() {
        return Ok(Vec::new());
    }
    let next = crate::cover::next_step(p, op, &c);
    match op.cover {
        None => Err(crate::cover::refusal(op, 2.0 * r, &c, next.as_ref())),
        Some(asked) => {
            let mut notes = vec![crate::cover::concession_note(op, asked, &c)?];
            notes.extend(next.map(|n| n.note()));
            Ok(notes)
        }
    }
}

/// `pattern=zigzag|oneway` on any region (Phase 28 slice 3; `DECISIONS.md` →
/// "Phase 28 slice 3: the patterns…"): parallel passes in the direction that
/// prices cheapest of the ones spine names (Phase 38 slice 0; before it, the
/// region's longer side), clipped to the offset family's OUTERMOST level — where the
/// tool's centre may be, a radius (plus allowance) off every wall, island and
/// keep-out, a radius past every open edge — then the SHORE: one pass along
/// each stretch of that level that runs beside something STANDING, and only
/// there (`spine::shores`, Phase 40 slice 1). Returns the op's note line and
/// its motion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pattern_pass(
    p: &Part,
    op: &Operation,
    region: &crate::region::Region,
    pattern: crate::parse::Pattern,
    skin: f64,
    z_cut: f64,
    z_from: f64,
    // The op's first level, which says the direction once for the block.
    first: bool,
) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("validate requires a stepover");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
    let lift = if region.keep_outs().is_empty() { rapid_z } else { clearance_z };

    let (wall, islands, _) = family_boundary(p, op, region, r, skin)?;
    let level = crate::offset::offset_shape(&wall, &islands, r + skin);
    if level.is_empty() {
        return Err(format!("feature '{}' is too small for tool T{}: no pass fits inside it", op.feature_name, op.tool_num));
    }
    let conceded = concession(p, op, region, r, skin)?;
    let flat = |ls: &[Vec<crate::offset::OffsetVertex>]| ls.iter().map(|l| crate::offset::flatten_loop(l)).collect::<Vec<_>>();
    let (ccw, cw) = (flat(&level.ccw), flat(&level.cw));
    let blank = crate::region::blank_of(p);
    // (A scan line is nudged a tenth of a micron off the region's extreme, so
    // "the whole tool clears the blank" is read to a micron, not to 1e-6.)
    // "Air where it is in reach" for the rows (Phase 44 slice 3, the pool
    // item Phase 42 left): beyond the blank by a radius, OR inside a given
    // void's lens — the centre set the whole body can stand in
    // (`stock::given_air_at`), where the before-state is void and the plunge
    // taxonomy licenses an entry at depth; a point ON the lens's edge is in
    // it (the extreme row's stroke rides that edge, and the certificate
    // blesses it). The one predicate every row pattern reads for its start
    // side, its ramp flags and its shore leads.
    let lenses = crate::stock::given_air_at(p, op, region.z);
    let given_air: Vec<Vec<Pt>> = lenses.iter().flat_map(|l| l.centre.iter().cloned()).collect();
    let in_air = |q: Pt| {
        (!crate::stock::point_in_polygon(&blank, q.x, q.y) && crate::stock::dist_to_polygon(&blank, q.x, q.y) >= r - 1e-3)
            || given_air.iter().any(|piece| crate::stock::point_in_polygon(piece, q.x, q.y) || crate::stock::dist_to_polygon(piece, q.x, q.y) <= crate::pattern::ON_LOOP)
    };
    let spine = pattern == crate::parse::Pattern::Spine;
    let held = pattern == crate::parse::Pattern::Held;
    // A lift, priced in millimetres of feed: up is a rapid, down is the
    // plunge rate over the same height. The one price's one parameter
    // (`pattern::price`), for spine's comb and the parallel passes' direction
    // alike.
    let lift_cost = (lift - z_cut).max(0.0) * (1.0 + feed / plunge);
    let (plan_chains, closing, stretches, note, said) = if spine || held {
        // A ramp into solid stock, `RAMP_DEG` steep over the depth it has
        // to lose — never shorter than the tool is wide.
        let ramp_len = ramp_room(z_from - z_cut).max(2.0 * r);
        let plan = if held { held_plan(p, op, region, skin, (lift_cost, ramp_len), z_cut)? } else { spine_plan(p, op, region, skin, (lift_cost, ramp_len), None)? };
        let rows = format!("{} row{} at {}{}", plan.rows, if plan.rows == 1 { "" } else { "s" }, f(stepover), if plan.cross_rows == 0 { String::new() } else { format!(" and {} across", plan.cross_rows) });
        let rails = if plan.rails == 0 { String::new() } else { format!(", {} pass{} along the rails", plan.rails, if plan.rails == 1 { "" } else { "es" }) };
        let shores = if plan.shores == 0 { String::new() } else { format!(", then {} shore pass{} along what stands", plan.shores, if plan.shores == 1 { "" } else { "es" }) };
        let note = if held { held_note(op, &plan, &rows, &rails, &shores) } else { format!("(pattern: spine at {}deg{}, {}{}{})", f(plan.angle_deg), angle_set(op), rows, rails, shores) };
        // The rows run whichever way the shortest link takes them, so they
        // have no one direction; the shore passes have, and take the key.
        if op.direction.is_some() && plan.shores == 0 {
            return Err(nothing_to_govern(op, &format!("the {} rows run both ways by turns and no pass runs along a wall", pattern.word())));
        }
        let said = rows_direction_note(None, (plan.shores > 0).then_some(op.climbs()));
        (plan.chains, Vec::new(), Vec::new(), note, said)
    } else {
        // The direction, CHOSEN as spine chooses its comb (Phase 38 slice 0;
        // `DECISIONS.md` → "Phase 38 slice 0…"): every direction a person
        // would name on this floor (`spine::directions` — X, Y, its longest
        // edges), each planned in full, the cheapest kept by the one price.
        // A tie keeps the first, X before Y: the rule this replaced took X
        // on a square, and a program that keeps its direction keeps its
        // bytes.
        let zigzag = pattern == crate::parse::Pattern::Zigzag;
        let mut best: Option<(crate::pattern::Plan, f64)> = None;
        // Or the one direction the person set (ROADMAP item 2, slice 3).
        let dirs = match op.angle {
            Some(a) => vec![a],
            None => crate::spine::directions(&floor_of(p, op, region), tool.dia),
        };
        for angle in dirs {
            if let Some(cand) = crate::pattern::plan(zigzag, angle, &ccw, &cw, stepover, &in_air, op.direction.map(|d| d == Direction::Climb)) {
                let cost = crate::pattern::price(&cand.chains, lift_cost);
                if best.as_ref().is_none_or(|(_, c)| cost < c - 1e-6) {
                    best = Some((cand, cost));
                }
            }
        }
        let plan = best.map(|(plan, _)| plan).ok_or_else(|| match op.angle {
            Some(a) => format!("feature '{}': no parallel pass fits inside it at angle={}", op.feature_name, crate::parse::deg_word(a)),
            None => format!("feature '{}': no parallel pass fits inside it", op.feature_name),
        })?;

        // The closing pass is a SHORE (Phase 40 slice 1; `DECISIONS.md` →
        // "Phase 40 slice 1…"): the stretches of the allowed boundary that run
        // ALONG something standing — a wall edge, an island, a keep-out, a clamp
        // biting the blank — and only those, with a straight lead a radius long
        // past each end, from the one home spine's shores come from. A shore
        // that stands all the way round (an island; a pocket's whole wall) comes
        // back closed and is cut as a loop; an open one is a pass. Nothing along
        // an open edge, which the rows already overhang, and nothing round the
        // end of a wall chain: the lap of the WHOLE standing level — Phase 28
        // slice 3's "known cost, stated" — is what this retires. (Phase 37 slice
        // 3's stretch beside a clamp that bites the edge is the same shore, byte
        // for byte: the clamp's sides are standing edges.)
        let floor = floor_of(p, op, region);
        let walls = standing_segments(region);
        let level: Vec<Vec<Pt>> = Vec::new();
        let ask = crate::spine::Ask { ccw: &ccw, cw: &cw, floor: &floor, walls: &walls, r, stand_off: r + skin, stepover, lift_cost, ramp_len: 0.0, in_air: &in_air, level: &level, level_d: 0.0, held: None, conventional: !op.climbs(), angle: op.angle };
        let (mut closing, mut stretches): (Vec<Vec<Pt>>, Vec<Vec<Pt>>) = (Vec::new(), Vec::new());
        for mut shore in crate::spine::shores(&ask) {
            if shore.len() >= 2 && crate::pattern::len(shore[0], shore[shore.len() - 1]) < crate::pattern::ON_LOOP {
                shore.pop();
                closing.push(shore);
            } else {
                stretches.push(shore);
            }
        }
        let n_shores = closing.len() + stretches.len();
        // `along X` / `along Y` where the direction is an axis — the words
        // every program before Phase 38 carried — and the angle otherwise.
        let direction = match plan.angle_deg {
            a if a == 0.0 => "along X".to_string(),
            a if a == 90.0 => "along Y".to_string(),
            a => format!("at {}deg", f(a)),
        };
        let note = format!(
            "(pattern: {} {}{}, {} pass{} at {}{})",
            pattern.word(),
            direction,
            angle_set(op),
            plan.passes,
            if plan.passes == 1 { "" } else { "es" },
            f(plan.step),
            if n_shores == 0 { String::new() } else { format!(", then {} shore pass{} along what stands", n_shores, if n_shores == 1 { "" } else { "es" }) }
        );
        // The zigzag's rows alternate; the one-way rows all run as the plan
        // says, and under the key as the key says.
        if op.direction.is_some() && plan.rows_climb.is_none() && n_shores == 0 {
            return Err(nothing_to_govern(op, "the zigzag rows run both ways by turns and no pass runs along a wall"));
        }
        let said = rows_direction_note(plan.rows_climb, (n_shores > 0).then_some(op.climbs()));
        (plan.chains, closing, stretches, note, said)
    };

    let mut g = conceded;
    g.push(note);
    // Which given voids' air the rows enter from, said as the offset family
    // says it: a chain that starts without a ramp because its start stands
    // in a lens (slice 3).
    let entered_from: Vec<String> = {
        let mut names: Vec<String> = plan_chains
            .iter()
            .filter(|ch| !ch.ramp && !ch.pts.is_empty())
            .filter_map(|ch| lenses.iter().find(|l| l.centre.iter().any(|piece| crate::stock::point_in_polygon(piece, ch.pts[0].x, ch.pts[0].y) || crate::stock::dist_to_polygon(piece, ch.pts[0].x, ch.pts[0].y) <= crate::pattern::ON_LOOP)).map(|l| l.void.clone()))
            .collect();
        names.sort();
        names.dedup();
        names
    };
    if !entered_from.is_empty() {
        g.push(format!("(rows entered from given air: {})", entered_from.join(", ")));
    }
    if first {
        g.push(said);
    }
    g.push(format!("G00 Z{}", f(clearance_z)));
    // Every descent is said before it is made (ROADMAP item 2): a chain that
    // plunges says where its start stands, by the plan's own reading — in a
    // given void's air, in air past the blank, or beside a stroke already
    // cut and how far off it (the plan licensed the plunge within a stepover
    // of one).
    let mut cut: Vec<(Pt, Pt)> = Vec::new();
    let row_plunge = |q: Pt, in_air_start: bool, cut: &[(Pt, Pt)]| -> String {
        if in_air_start {
            if let Some(l) = lenses.iter().find(|l| l.centre.iter().any(|piece| crate::stock::point_in_polygon(piece, q.x, q.y) || crate::stock::dist_to_polygon(piece, q.x, q.y) <= crate::pattern::ON_LOOP)) {
                return format!("(plunge: in the given air of {})", l.void);
            }
            return "(plunge: in air outside the blank)".to_string();
        }
        match cut.iter().map(|(a, b)| crate::engagement::dist_to_segment((q.x, q.y), *a, *b)).min_by(|x, y| x.partial_cmp(y).unwrap()) {
            Some(d) => format!("(plunge: beside a stroke already cut, {}mm off it)", f(d)),
            None => "(plunge: at the stroke's start)".to_string(),
        }
    };
    // A row is entered down its own line (ROADMAP item 2, slice 4): a ramp
    // along its first stroke or a plunge at its start. A lead from the air
    // beside it is not built, so the key cannot be kept here.
    if op.entry == Some(EntryKind::Lead) {
        return Err(format!("entry=lead on '{}': a row is entered down its own line, by a ramp along its first stroke or a plunge at its start; a lead from the air beside it is not built", op.feature_name));
    }
    if LeadShape::of(op, tool.dia).said {
        return Err(LeadShape::nothing_to_shape(op, "a row is entered down its own line, by a ramp along its first stroke or a plunge at its start"));
    }
    for chain in &plan_chains {
        let pts = &chain.pts;
        // The plan ramps where the start stands in stock and plunges where
        // it stands in air or beside a stroke already cut; the key makes
        // every descent one kind, and the sentence names the key where it
        // changed one.
        let planned = chain.ramp && pts.len() >= 2;
        let ramp = match op.entry {
            Some(EntryKind::Ramp) => pts.len() >= 2,
            Some(EntryKind::Plunge) => false,
            _ => planned,
        };
        let asked = if ramp != planned { asks(op) } else { String::new() };
        // A plunged entry leaves the plunge feed modal: the first flat move
        // must state the cutting feed, as it does after a plain plunge.
        let mut plunged = !ramp;
        if ramp {
            plunged = ramp_down_first_stroke(&mut g, pts, z_from, z_cut, lift, feed, plunge, tool.dia, &asked, op)?;
        } else {
            g.push(if asked.is_empty() { row_plunge(pts[0], chain.in_air, &cut) } else { format!("(plunge: at the stroke's start, into standing stock{asked})") });
            g.push(format!("G00 X{} Y{}", f(pts[0].x), f(pts[0].y)));
            g.push(format!("G00 Z{}", f(lift)));
            g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));
        }
        for (i, q) in pts.iter().enumerate().skip(1) {
            g.push(if i == 1 && plunged { format!("G01 X{} Y{} F{}", f(q.x), f(q.y), f(feed)) } else { format!("G01 X{} Y{}", f(q.x), f(q.y)) });
        }
        g.push(format!("G00 Z{}", f(lift)));
        cut.extend(pts.windows(2).map(|w| (w[0], w[1])));
    }
    // A shore plunges beside floor the rows cut — or ramps down its first
    // stroke where `entry=ramp` asks (slice 4).
    let asked = asks(op);
    for lp in &closing {
        if lp.len() < 2 {
            continue;
        }
        let plunged = if op.entry == Some(EntryKind::Ramp) {
            ramp_down_first_stroke(&mut g, lp, z_from, z_cut, lift, feed, plunge, tool.dia, &asked, op)?
        } else {
            g.push("(plunge: at the shore's start, beside floor the rows cut)".to_string());
            g.push(format!("G00 X{} Y{}", f(lp[0].x), f(lp[0].y)));
            g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));
            true
        };
        trace_loop(&mut g, lp, feed, plunged, op.compress == CompressMode::Arcs);
        g.push(format!("G00 Z{}", f(lift)));
    }
    // An open shore: a pass with a lead each end, entered at the lead's start,
    // which is beside floor the rows already cut.
    for chain in stretches.iter().filter(|c| c.len() >= 2) {
        if op.entry == Some(EntryKind::Ramp) {
            ramp_down_first_stroke(&mut g, chain, z_from, z_cut, lift, feed, plunge, tool.dia, &asked, op)?;
        } else {
            g.push("(plunge: at the lead's start, beside floor the rows cut)".to_string());
            g.push(format!("G00 X{} Y{}", f(chain[0].x), f(chain[0].y)));
            g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));
        }
        for (i, q) in chain.iter().enumerate().skip(1) {
            g.push(if i == 1 { format!("G01 X{} Y{} F{}", f(q.x), f(q.y), f(feed)) } else { format!("G01 X{} Y{}", f(q.x), f(q.y)) });
        }
        g.push(format!("G00 Z{}", f(lift)));
    }
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// What a held op says of its plan (Phase 62 slice 1): the ceiling and
/// whether it is the default, the spacing it derived and on which tool,
/// spine's comb under it, the lifts taken where a link would have exceeded
/// the ceiling, and — read back through the walk — what ran over it and by
/// how much, by kind; or that the walk declined before this op, so the
/// spacing alone holds it.
fn held_note(op: &Operation, plan: &crate::spine::Plan, rows: &str, rails: &str, shores: &str) -> String {
    let deg = crate::parse::deg_word(crate::parse::derive_held_ceiling(op));
    let default = if op.engagement.is_none() { ", the default" } else { "" };
    let read = plan.held.as_ref().expect("a held plan carries its read");
    let lifts = match read.lifts {
        0 => String::new(),
        1 => "; 1 lift where a link would exceed the ceiling".to_string(),
        n => format!("; {n} lifts where a link would exceed the ceiling"),
    };
    let over = if !read.read {
        "; the walk declined before this op, so the spacing alone holds it".to_string()
    } else if read.over.is_empty() {
        "; nothing over it".to_string()
    } else {
        format!("; over it: {}", read.over.iter().map(|(kind, mm)| format!("{}mm on {kind}", f(*mm))).collect::<Vec<_>>().join(", "))
    };
    format!(
        "(pattern: held under {deg}deg{default}: {rows}, the spacing the ceiling derives on T{}, on spine's comb at {}deg{}{rails}{shores}{lifts}{over})",
        op.tool_num,
        f(plan.angle_deg),
        angle_set(op)
    )
}

/// The words that follow the rows' angle where the person set it
/// (ROADMAP item 2, slice 3), so a reader of the program alone can tell
/// the comb's choice from the author's.
fn angle_set(op: &Operation) -> &'static str {
    if op.angle.is_some() { ", the angle set" } else { "" }
}

/// What a region-working op says about its region, in one order everywhere:
/// what it goes around, what it keeps out of, which edges are open, which
/// voids it is open onto.
pub(crate) fn region_notes(region: &crate::region::Region, say_open_edges: bool) -> Vec<String> {
    let mut notes = Vec::new();
    if !region.feature_islands().is_empty() {
        notes.push(format!("(islands: {})", region.feature_islands().join(", ")));
    }
    if !region.keep_outs().is_empty() {
        notes.push(format!("(keep-outs: {})", region.keep_outs().join(", ")));
    }
    if say_open_edges && region.open_edges() > 0 {
        notes.push(format!("(open edges: {} of {} — entered from air)", region.open_edges(), region.edges.len()));
    }
    notes.extend(grown_note(region));
    notes.extend(shore_note(region));
    notes.extend(open_void_note(region));
    notes
}

/// Whether given air bears on this op's floor (Phase 42 slices 3–4): the
/// floor grew into a lens, or a lens exists for it at all (an entry, in reach
/// or not). Routes a closed pocket to the shape family, the one that reads
/// the region's outline and asks for air; a program that gives nothing takes
/// the plain arm it always did.
pub(crate) fn given_in_play(p: &Part, op: &Operation, region: &crate::region::Region) -> bool {
    !region.grown.is_empty() || (!p.given.is_empty() && !crate::stock::given_air_at(p, op, region.z).is_empty())
}

/// The op's statement that its floor GREW into given air (Phase 42 slice 3):
/// which voids' lenses the region unioned in, beside `(islands: …)`. Empty
/// for every program that gives nothing.
pub(crate) fn grown_note(region: &crate::region::Region) -> Option<String> {
    (!region.grown.is_empty()).then(|| format!("(grown into given air: {})", region.grown.join(", ")))
}

/// The program's note for what stands ON the boundary (Phase 49 slice 1):
/// bitten out of it, not gone around.
pub(crate) fn shore_note(region: &crate::region::Region) -> Option<String> {
    (!region.on_the_shore.is_empty()).then(|| format!("(on the shore: {})", region.on_the_shore.join(", ")))
}

/// Whether the op asked for parallel passes.
pub(crate) fn parallel_pattern(p: &Part, op: &Operation) -> Option<crate::parse::Pattern> {
    crate::parse::derive_pattern(p, op).filter(|k| *k != crate::parse::Pattern::Offset)
}

/// The closed, empty pocket's family, INNERMOST FIRST: one level per offset
/// from `r + skin` inward in `stepover` steps, refused where no pass fits or
/// the last one leaves a core. One home since Phase 27 slice 4 — `Clear` cuts
/// it to rough the pocket and `Floor` cuts it to finish the floor — lifted out
/// of `clear` verbatim, so every golden that walked it still does.
pub(crate) fn plain_pocket_levels(op: &Operation, points: &[Pt], r: f64, skin: f64, stepover: f64) -> Result<Vec<Vec<Vec<Pt>>>, String> {
    // Make a CCW copy of the points
    let ccw_points = make_ccw(points);

    // Offset inward until empty. The skin shifts the WHOLE spiral inward, not
    // just the boundary loop: inter-loop spacing then stays exactly `stepover`
    // with no special case, and the outermost loop — the wall pass — rides the
    // skin parallel. (Shifting only the boundary and leaving the interior at
    // nominal is the viable alternative; it buys nothing and costs a
    // discontinuity at the first interior loop. DECISIONS.md records both.)
    // `+ skin` is appended rather than re-associated, so skin == 0 is
    // byte-identical output.
    let mut offset_dist = r + skin;
    let mut all_loops = Vec::new();
    let mut last_offset_dist = 0.0;
    loop {
        // Flattened ALONG its arcs, never across them (Phase 37 slice 0). An
        // inward offset has an arc in one place — round a wall corner that
        // points INTO the pocket, centred on that corner — and its chord
        // passes NEARER the corner than the arc: at the wall level that is a
        // cut into the part, `r(1 - cos(turn/2))` deep. A pocket with no such
        // corner has no arcs, and its loops are the vertices they always were.
        let step: Vec<Vec<Pt>> = crate::offset::offset_closed_polyline_verts(&ccw_points, offset_dist).iter().map(|lp| crate::offset::flatten_loop(lp)).collect();
        if step.is_empty() {
            break;
        }
        all_loops.push(step);
        last_offset_dist = offset_dist;
        offset_dist += stepover;
    }

    if all_loops.is_empty() {
        // Geometry failure, not a note: a program that quietly omits an op
        // the author asked for is worse than no program.
        if skin > 0.0 {
            // The tool may well fit — it is tool + allowance that doesn't, and
            // saying "too small for the tool" would send the author after the
            // wrong knob.
            return Err(format!(
                "pocket feature '{}' is too small for tool T{} plus the stock_to_leave offset ({:.3}mm): no offset pass fits inside it — reduce stock_to_leave or use a smaller tool",
                op.feature_name, op.tool_num, skin
            ));
        }
        return Err(format!(
            "pocket feature '{}' is too small for tool T{}: no offset pass fits inside it",
            op.feature_name, op.tool_num
        ));
    }

    // The last pass clears material out to `last_offset_dist + r` from the
    // boundary; anything the offsetter still returns beyond that is stock no
    // pass reaches — a full-depth island standing at the pocket centre. It is a
    // distinct failure from the stepover >= dia ridge validate() rejects:
    // stepover can be under the diameter and still outrun the shape, because
    // the loop stops when the NEXT offset prunes empty, not when coverage runs
    // out (14x14 pocket, dia 6, stepover 5 leaves a 1mm island). Always-on: the
    // island is there with or without an allowance.
    let what = format!("pocket feature '{}'", op.feature_name);
    inward_wall_stepover_law(&what, op.tool_num, &ccw_points, r, stepover)?;
    if !offset_closed_polyline(&ccw_points, last_offset_dist + r + MIDDLE_HAIR).is_empty() {
        return Err(uncut_middle(&what, op.tool_num, last_offset_dist + r, "the boundary"));
    }
    corner_stepover_law(&what, op.tool_num, &ccw_points, r, stepover)?;

    // Inside-out: reverse the offset steps so we process innermost loops first
    all_loops.reverse();
    Ok(all_loops)
}

/// An islanded pocket's pass list, in emission order, with each loop's level
/// (for its feed word) and its entry.
struct IslandedFamily {
    loops: Vec<Vec<Pt>>,
    level_of: Vec<usize>,
    ramps: Vec<Entry>,
    /// The concession's note, where the run line conceded a stretch.
    conceded: Vec<String>,
    /// The entry's note where given air was in play (Phase 42 slice 4):
    /// which voids' air a loop was entered from, or that none reached.
    entered: Vec<String>,
}

/// The pocket arm's family when standing features are inside it (Phase 26
/// slice 2; `DECISIONS.md` → "Phase 26 slice 2: the pocket…"). The same
/// machine as the plain pocket — erode from `r + skin` in `stepover` steps
/// until nothing is left, cut the
/// deepest level first — with the region between the wall AND the islands as
/// what erodes, so every wall, whoever's it is, gets the last pass beside it.
///
/// **Order** (fork 2A): deepest level first; within a level the wall-parallel
/// loops (CCW) then the island-parallel ones (CW), as the offsetter hands them
/// back — each climbing against the wall it defines with M03.
///
/// **Entry** (fork 3A): a loop PLUNGES only where its start lands beside air a
/// deeper loop already cut — it is rotated to start at its vertex nearest a
/// deeper loop, and that vertex must be within one stepover of it. Otherwise it
/// stands in solid stock and RAMPS, however many such loops a level has.
///
/// **The web** (fork 4A): at the first level every island must still have its
/// own loop. One missing means a loop merged into a neighbour's — the tool
/// cannot pass between them — and the op refuses, naming the tightest pair.
fn region_pocket_family(
    p: &Part,
    op: &Operation,
    region: &crate::region::Region,
    r: f64,
    skin: f64,
    stepover: f64,
) -> Result<IslandedFamily, String> {
    // The family is the same for a pocket's volume, a pocket's floor and a
    // `Face`; only the noun in a refusal differs.
    let what = match p.features.get(&op.feature_name) {
        Some(Feature::Face { .. }) => format!("Face feature '{}'", op.feature_name),
        Some(Feature::Plane { .. }) => format!("Plane feature '{}'", op.feature_name),
        _ => format!("pocket feature '{}'", op.feature_name),
    };
    let islands = region.islands();
    // The boundary the family erodes from: the region itself when every edge
    // is a wall (the authored outline, untouched), pushed out one tool diameter
    // across the open ones otherwise.
    let pushed = if region.open_edges() > 0 {
        Some(region.family_wall(2.0 * r + skin).ok_or_else(|| format!(
            "{}: its open edges could not be pushed out past the blank into one outline — the region folds back on itself at the stock's edge",
            what
        ))?)
    } else {
        None
    };
    let (pushed, enclosed) = match pushed {
        Some(swath) => {
            let enclosed = swath.enclosed_islands();
            (swath.wall, enclosed)
        }
        None => (region.outer.clone(), Vec::new()),
    };
    let (wall, mut outlines) = crate::region::split_islands(pushed, &islands).ok_or_else(|| format!(
        "{}: a keep-out across its boundary leaves no simple region to cut",
        what
    ))?;
    outlines.extend(enclosed);
    let wall: &[Pt] = &wall;
    let levels = crate::offset::shape_levels(wall, &outlines, r + skin, stepover);
    let Some((last_d, _)) = levels.last() else {
        return Err(format!(
            "{} is too small for tool T{}{}: no offset pass fits between its wall and its island{}",
            what,
            op.tool_num,
            if skin > 0.0 { format!(" plus the stock_to_leave offset ({:.3}mm)", skin) } else { String::new() },
            if islands.len() == 1 { "" } else { "s" }
        ));
    };
    // The web (Phase 26 fork 4A) — and every other stretch of floor this tool
    // cannot enter — is the concession's to judge since Phase 32 slice 2: it
    // refuses naming every one, or carries the note the run line conceded.
    let conceded = concession(p, op, region, r, skin)?;
    if outlines.is_empty() {
        // No island, no ridge: the plain pocket's own coverage check.
        inward_wall_stepover_law(&what, op.tool_num, wall, r, stepover)?;
        if !crate::offset::offset_shape(wall, &outlines, last_d + r + MIDDLE_HAIR).is_empty() {
            return Err(uncut_middle(&what, op.tool_num, last_d + r, "the boundary"));
        }
        corner_stepover_law(&what, op.tool_num, wall, r, stepover)?;
    } else {
        island_stepover_law(&what, op.tool_num, r, stepover)?;
    }

    // Deepest first. Each level flattened ALONG its arcs: an island-parallel
    // loop is an outward offset, whose chord would cut inside the arc toward
    // the island (`offset::flatten_loop`'s own reason).
    let flat: Vec<Vec<Vec<Pt>>> = levels
        .iter()
        .rev()
        .map(|(_, l)| l.ccw.iter().chain(&l.cw).map(|lp| crate::offset::flatten_loop(lp)).collect())
        .collect();
    let mut out = IslandedFamily { loops: Vec::new(), level_of: Vec::new(), ramps: Vec::new(), conceded, entered: Vec::new() };
    for (k, level) in flat.iter().enumerate() {
        for lp in level {
            // The point of this loop nearest anything the deeper level cut —
            // a point, not a vertex: a square's four corners are each further
            // from the loop inside it than its edges are.
            let nearest = (k > 0)
                .then(|| {
                    // Tied deeper loops: the one whose point is nearest the tool.
                    let tool = out.loops.last().map(|l: &Vec<Pt>| l[0]);
                    crate::tie::first_least_then(flat[k - 1].iter().map(|deep| nearest_between(lp, deep, tool)), |n| n.0, |n| from_tool(tool, n.2))
                })
                .flatten();
            let beside_air = nearest.filter(|&(d, _, _)| d <= stepover + PLUNGE_BESIDE_AIR_TOL);
            let lp = match beside_air {
                Some((_, edge, at)) => start_loop_at(lp.clone(), edge, at),
                None => lp.clone(),
            };
            out.loops.push(lp);
            out.level_of.push(k);
            out.ramps.push(match beside_air { None => Entry::Ramp, Some((d, _, _)) => Entry::Plunge(Some(d)) });
        }
    }
    // "Air where it is in reach" (Phase 27 slice 2) means two things since
    // Phase 42 slice 4: beyond the blank, or in a GIVEN void's lens — the
    // centre set the whole body can stand in (`stock::given_air_at`). The
    // before-state is void there, so the plunge is a licensed entry.
    let given: Vec<(String, Vec<Pt>)> = crate::stock::given_air_at(p, op, region.z)
        .into_iter()
        .flat_map(|l| l.centre.into_iter().map(move |c| (l.void.clone(), c)))
        .collect();
    if region.open_edges() > 0 || !given.is_empty() {
        let used = lead_in_from_air(op, &mut out, &crate::region::blank_of(p), &given, flat.last().map(Vec::as_slice).unwrap_or(&[]), r)?;
        if !used.is_empty() {
            out.entered.push(format!("(entered from given air: {})", used.join(", ")));
        } else if !given.is_empty() && out.ramps.contains(&Entry::Ramp) {
            let mut names: Vec<&str> = given.iter().map(|(n, _)| n.as_str()).collect();
            names.dedup();
            out.entered.push(format!("(given air of {} out of reach of every pass — ramped)", names.join(", ")));
        }
    }
    Ok(out)
}

/// Every loop that would RAMP takes a side entry instead where air is in
/// reach (Phase 27 slice 2): plunge outside the blank, where the whole tool is
/// in air, and feed straight in to the loop's nearest point. The lead must
/// stay inside the family's outermost level the whole way — so it never comes
/// within a radius (plus allowance) of a wall or an island — or the loop keeps
/// its ramp.
///
/// Since Phase 42 slice 4 the air is also a GIVEN void's lens (`given`: the
/// centre pieces the whole body can stand in, by name): a plunge INTO it at
/// depth, then the same straight lead. No preference between the two airs
/// — the nearest air point wins, as it always did between two open edges.
/// Returns the given voids whose air a loop was entered from, deduplicated,
/// in the order they were first used.
fn lead_in_from_air(op: &Operation, family: &mut IslandedFamily, blank: &[Pt], given: &[(String, Vec<Pt>)], outermost: &[Vec<Pt>], r: f64) -> Result<Vec<String>, String> {
    let in_blank_air = |q: Pt| !crate::stock::point_in_polygon(blank, q.x, q.y) && crate::stock::dist_to_polygon(blank, q.x, q.y) >= r - 1e-6;
    let in_given = |q: Pt| given.iter().find(|(_, c)| crate::stock::point_in_polygon(c, q.x, q.y)).map(|(n, _)| n.as_str());
    let in_air = |q: Pt| in_blank_air(q) || in_given(q).is_some();
    let mut used: Vec<String> = Vec::new();
    let shape = LeadShape::of(op, 2.0 * r);
    let keyed = shape.said || op.entry == Some(EntryKind::Lead);
    // Where a tool may stand in air: along every loop of the family, a
    // millimetre apart, wherever the whole tool clears the blank.
    let mut air: Vec<Pt> = Vec::new();
    for lp in &family.loops {
        for i in 0..lp.len() {
            let (a, b) = (lp[i], lp[(i + 1) % lp.len()]);
            let steps = ((b.x - a.x).phypot(b.y - a.y).ceil() as usize).max(1);
            air.extend((0..steps).map(|k| k as f64 / steps as f64).map(|t| Pt { x: a.x + t * (b.x - a.x), y: a.y + t * (b.y - a.y) }).filter(|q| in_air(*q)));
        }
    }
    // Inside the outermost level: within one of its wall-parallel loops and
    // outside every island-parallel one. Winding tells them apart.
    let signed = |lp: &[Pt]| (0..lp.len()).map(|i| lp[i].x * lp[(i + 1) % lp.len()].y - lp[(i + 1) % lp.len()].x * lp[i].y).sum::<f64>();
    let allowed = |q: Pt| {
        let on = outermost.iter().any(|lp| crate::stock::dist_to_polygon(lp, q.x, q.y) <= 1e-6);
        let inside = outermost.iter().filter(|lp| signed(lp) > 0.0).any(|lp| crate::stock::point_in_polygon(lp, q.x, q.y));
        let in_island_zone = outermost.iter().filter(|lp| signed(lp) < 0.0).any(|lp| crate::stock::point_in_polygon(lp, q.x, q.y));
        on || (inside && !in_island_zone)
    };
    // The lead the keys shape (ROADMAP item 2, slice 5), or every loop's
    // under entry=lead: the start turned round the loop until the whole
    // lead lies in air — past the blank, in a given void, or on the path of
    // a loop cut before this one, where the body stood already — and runs
    // nowhere a radius from a wall; none, the op refuses.
    if keyed {
        for i in 0..family.loops.len() {
            if family.ramps[i] != Entry::Ramp && op.entry != Some(EntryKind::Lead) {
                continue;
            }
            let lp = family.loops[i].clone();
            let earlier: Vec<&[Pt]> = family.loops[..i].iter().map(Vec::as_slice).collect();
            let air_here = |q: Pt| in_air(q) || earlier.iter().any(|l| crate::stock::dist_to_polygon(l, q.x, q.y) <= crate::pattern::ON_LOOP);
            // Inside the level, or past the blank's outline — the body may
            // overlap the edge there, as the pass it leads to does at an
            // open edge — or in a given void.
            let clear = |q: Pt| allowed(q) || !crate::stock::point_in_polygon(blank, q.x, q.y) || in_given(q).is_some();
            let mut found = None;
            'turn: for e in 0..lp.len() {
                let (a, b) = (lp[e], lp[(e + 1) % lp.len()]);
                let len = (b.x - a.x).phypot(b.y - a.y);
                if len < 1e-9 {
                    continue;
                }
                let tangent = ((b.x - a.x) / len, (b.y - a.y) / len);
                let steps = (len.ceil() as usize).max(1);
                for k in 0..steps {
                    let s = k as f64 / steps as f64;
                    let at = Pt { x: a.x + s * (b.x - a.x), y: a.y + s * (b.y - a.y) };
                    for side in [(-tangent.1, tangent.0), (tangent.1, -tangent.0)] {
                        let q = shape.point(at, tangent, side);
                        let d = (q.x - at.x).phypot(q.y - at.y);
                        let n_s = ((d / 0.5).ceil() as usize).max(1);
                        let whole = (0..=n_s).all(|j| {
                            let tt = j as f64 / n_s as f64;
                            clear(Pt { x: q.x + tt * (at.x - q.x), y: q.y + tt * (at.y - q.y) })
                        });
                        if air_here(q) && whole {
                            found = Some((e, at, q));
                            break 'turn;
                        }
                    }
                }
            }
            let Some((e, at, q)) = found else {
                return Err(format!(
                    "lead on '{}': {} — no start on loop {} of {} has the whole lead in air past the blank, in a given void, or on a path cut before it",
                    op.feature_name, shape.words("the loop's normal", "the loop's own line, back from its start"), i + 1, family.loops.len()
                ));
            };
            family.loops[i] = start_loop_at(lp, e, at);
            family.ramps[i] = Entry::Lead(q);
            if !in_blank_air(q) {
                if let Some(name) = in_given(q) {
                    if !used.iter().any(|u| u == name) {
                        used.push(name.to_string());
                    }
                }
            }
        }
        return Ok(used);
    }
    for i in 0..family.loops.len() {
        if family.ramps[i] != Entry::Ramp {
            continue;
        }
        // Tied air points: the first as `air` lists them.
        let reachable = air.iter().map(|a| (*a, nearest_on_loop(&family.loops[i], *a, None))).filter(|(a, (d, _, at))| {
            let steps = ((d / 0.5).ceil() as usize).max(1);
            (0..=steps).map(|k| k as f64 / steps as f64).all(|t| allowed(Pt { x: a.x + t * (at.x - a.x), y: a.y + t * (at.y - a.y) }))
        });
        let best = crate::tie::first_least(reachable, |c| c.1 .0);
        if let Some((from, (_, edge, at))) = best {
            family.loops[i] = start_loop_at(family.loops[i].clone(), edge, at);
            family.ramps[i] = Entry::Lead(from);
            if !in_blank_air(from) {
                if let Some(name) = in_given(from) {
                    if !used.iter().any(|u| u == name) {
                        used.push(name.to_string());
                    }
                }
            }
        }
    }
    Ok(used)
}

/// The entry rule for a family walked in an order where air accumulates (the
/// islanded FACE — Phase 26 slice 3): the first loop stands in air outside the
/// blank and plunges; every later loop plunges where some EARLIER loop passes
/// within a stepover of it — started at its point nearest that loop — and
/// ramps where none does. Returns the loops, re-started, with their flags.
pub(crate) fn enter_beside_cut_air(ordered: Vec<Vec<Pt>>, stepover: f64) -> (Vec<Vec<Pt>>, Vec<Entry>) {
    let mut loops: Vec<Vec<Pt>> = Vec::with_capacity(ordered.len());
    let mut ramps = Vec::with_capacity(ordered.len());
    for (i, lp) in ordered.into_iter().enumerate() {
        if i == 0 {
            loops.push(lp);
            ramps.push(Entry::Plunge(None));
            continue;
        }
        // Tied earlier loops: the one whose point is nearest the tool.
        let tool = loops.last().map(|l| l[0]);
        let nearest = crate::tie::first_least_then(loops.iter().map(|earlier| nearest_between(&lp, earlier, tool)), |n| n.0, |n| from_tool(tool, n.2));
        let beside_air = nearest.filter(|&(d, _, _)| d <= stepover + PLUNGE_BESIDE_AIR_TOL);
        ramps.push(match beside_air { None => Entry::Ramp, Some((d, _, _)) => Entry::Plunge(Some(d)) });
        loops.push(match beside_air {
            Some((_, edge, at)) => start_loop_at(lp, edge, at),
            None => lp,
        });
    }
    (loops, ramps)
}

/// How far `q` is from where the tool stands; nothing to choose by when the
/// tool has no position yet.
fn from_tool(tool: Option<Pt>, q: Pt) -> f64 {
    tool.map_or(0.0, |t| (q.x - t.x).phypot(q.y - t.y))
}

/// `lp` re-started at `at`, a point on the edge leaving vertex `edge`: the
/// edge is split there unless the point is already one of its ends.
fn start_loop_at(mut lp: Vec<Pt>, edge: usize, at: Pt) -> Vec<Pt> {
    lp.rotate_left(edge);
    let near = |a: Pt, b: Pt| (a.x - b.x).phypot(a.y - b.y) < 1e-6;
    if near(lp[0], at) {
    } else if near(lp[1 % lp.len()], at) {
        lp.rotate_left(1);
    } else {
        lp.rotate_left(1);
        lp.insert(0, at);
    }
    lp
}

/// Where `lp` comes nearest to `other`, as a point ON `lp`. Both directions are
/// searched — `other`'s vertices dropped onto `lp`'s edges, and `lp`'s own
/// vertices measured against `other`'s edges — because either loop may be the
/// sparse one: a square's corners are each √2 stepovers from the square inside
/// it (the pocket's first build), and from the square OUTSIDE it (the face's).
///
/// Parallel loops are equally near all along a straight run, so the point is
/// a tie many times over. Tied points: the one nearest `tool`, where the tool
/// stands after the loop before (a closed loop ends where it starts), so the
/// move between loops is the short one; with no tool position, or tied on
/// that too, the first by vertex order.
fn nearest_between(lp: &[Pt], other: &[Pt], tool: Option<Pt>) -> (f64, usize, Pt) {
    let dropped = crate::tie::first_least_then(other.iter().map(|q| nearest_on_loop(lp, *q, tool)), |n| n.0, |n| from_tool(tool, n.2)).expect("loops have vertices");
    let own = crate::tie::first_least_then(lp.iter().enumerate().map(|(i, q)| (nearest_on_loop(other, *q, None).0, i, *q)), |n| n.0, |n| from_tool(tool, n.2)).expect("loops have vertices");
    // Parallel loops tie all along their length — to the last float bit on a
    // straight run, to a chord's sagitta around an arc — so the dropped point
    // wins anything closer than the flattening can tell apart, and the start
    // does not wander with the noise.
    if own.0 < dropped.0 - PLUNGE_BESIDE_AIR_TOL { own } else { dropped }
}

/// The closest point of a closed loop to `q`: its distance, the index of the
/// edge it lies on (from that vertex to the next), and the point.
fn nearest_on_loop(lp: &[Pt], q: Pt, tool: Option<Pt>) -> (f64, usize, Pt) {
    let n = lp.len();
    // Tied edges (a corner's two sides, or one vertex two edges share): the
    // point nearest the tool, then the first edge from the loop's start.
    let edges = (0..n)
        .map(|i| {
            let (a, b) = (lp[i], lp[(i + 1) % n]);
            let (ex, ey) = (b.x - a.x, b.y - a.y);
            let len2 = ex * ex + ey * ey;
            let t = if len2 > 0.0 { (((q.x - a.x) * ex + (q.y - a.y) * ey) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let at = Pt { x: a.x + t * ex, y: a.y + t * ey };
            ((q.x - at.x).phypot(q.y - at.y), i, at)
        });
    crate::tie::first_least_then(edges, |e| e.0, |e| from_tool(tool, e.2)).expect("a loop has edges")
}

/// A deeper loop stands exactly one stepover from the loop outside it, as
/// curves; as flattened polylines each is off its curve by at most the chord
/// tolerance. This is that, twice, with room.
const PLUNGE_BESIDE_AIR_TOL: f64 = 4.0 * crate::CHORD_TOL_MM;

/// Which pair is too close, and by how much — the sentence fork 4A owes, read
/// off the region's own narrowest passage (`region::of`, the one home for the
/// number since Phase 27; measured on a round boss's circle, never its chords).

/// `Clear` over a standing surface of revolution: the moat around it, taken in
/// Z levels down to its own base plane — and, where the surface crowns, the
/// HOLE inside it taken in the same levels (DECISIONS.md → "roughing a convex
/// revolve…" and "the torus's annulus island…", fork 3).
///
/// **Outermost-in, which is the pocket's rule mirrored.** The dense pocket
/// spiral runs innermost-out so the WALL pass — the one that defines the
/// surface anyone measures — comes last. Here the surface anyone measures is
/// the ISLAND, so the island-adjacent pass comes last and the family runs
/// outermost-in. It also puts the ramp entry out at the stock's edge, in virgin
/// stock, rather than against the part.
///
/// **The hole mirrors that mirror, so it is the pocket's rule the right way up
/// again.** Inside a donut hole the surface anyone measures is the part's inner
/// FLANK — the ceiling bounding the work from outside — so the hole family runs
/// innermost-out, ceiling-adjacent pass last, and its ramp entry lands beside
/// the pillar, in the scrap column, rather than against the part. Same reason,
/// opposite geometry.
///
/// **Cut direction CW around the island, CCW inside the hole**, for the reasons
/// already recorded on `finish_cylinder_boss` and the bore family: with M03, CW
/// travel around an outside wall is climb milling and CCW inside a wall that
/// surrounds you is the same. Each family climbs against the surface it
/// defines.
///
/// **Each level ramps from the level above it**, not from the stock top: the
/// annulus overhead was cleared by the previous level, so a ramp anchored at
/// the stock's top would spend most of its length descending through air it
/// already owns.
fn clear_revolve(p: &Part, op: &Operation, feature: &Feature) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("Clear operation requires stepover");
    let stepdown = op.stepdown.expect("validate requires stepdown on a revolve");
    let skin = op.stock_to_leave.unwrap_or(0.0);

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let (cx, cy, _) = crate::parse::derive_revolve_datum(feature).expect("guarded by the caller");
    let stacks = crate::parse::derive_revolve_clear_stacks(feature, p.stock.z, stepdown, skin)
        .expect("validate proved the stack exists");
    // The pillar the hole leaves in the middle, and the reason the moat needs
    // it: above the crown the stack's island is nothing (no part stands that
    // high), but the pillar does, and the moat's own passes run right across
    // that height. Both families stand off the same number, from opposite
    // sides. Zero when nothing opens inside the part, which is nine of the ten.
    let pillar = stacks.pillar_r(tool.dia);
    let levels = stacks.moat;
    let holes = stacks.hole;
    // Fork 3A: the moat is one tool diameter of MATERIAL beyond what stands —
    // the rim plus its allowance — which is `rim_collar`'s own law, the width a
    // tool must sweep to reach an edge at all. Nothing declares that the stock
    // outside it should go, so nothing takes it: what is left is a pedestal,
    // and a program that wants the whole block down says so with an End.
    let rim = crate::parse::derive_revolve_rim_radius(feature, 0.0)
        .expect("guarded by the caller") + skin;
    // The width is the kind's, not a constant: one diameter for a revolve,
    // two for a cylinder boss whose finish leads in from a diameter out
    // (`derive_moat_width`, the one home the removal reads too).
    let moat_width = crate::parse::derive_moat_width(p, op, feature, tool.dia);
    let r_outer_centre = rim + moat_width - r;

    let mut g = Vec::new();
    g.push(format!(
        "(--- revolve clear: {} with T{}{} ---)",
        op.feature_name,
        op.tool_num,
        stock_suffix(op, 0.0)
    ));
    // Spelled in words so every revolve golden that says "one tool diameter"
    // stays byte-identical; the cylinder boss is the only two.
    let width_words = if (moat_width / tool.dia - 1.0).abs() < 1e-9 {
        "one tool diameter".to_string()
    } else {
        format!("{} tool diameters — the finish's own and the one its lead-in stands out", f(moat_width / tool.dia))
    };
    g.push(format!(
        "(moat to r {} — the rim {} plus {}; stock outside it is declared by nothing and stays)",
        f(rim + moat_width),
        f(rim),
        width_words
    ));
    if let Some(first) = holes.last() {
        // Read at the hole's FLOOR, where it is narrowest — the same place the
        // pillar is sized from.
        g.push(format!(
            "(hole open to r {} — the inner rim {} less one tool diameter; the column inside it stands as a pillar)",
            f(pillar),
            f(first.ceiling_r)
        ));
    }
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));

    // The hole's levels are the tail of the moat's — one crown break, one
    // division — so a level is a single trip down for both work regions.
    let below_crown = levels.len() - holes.len();
    let mut ceiling = p.stock.z;
    for (i, level) in levels.iter().enumerate() {
        // Above the crown the part declares no island, but the pillar stands
        // there all the same.
        let island = level.island_r.max(pillar);
        let r_inner_centre = island + r;
        if r_inner_centre > r_outer_centre + 1e-9 {
            // The island has grown past the moat: below this height the tool
            // cannot stand between the part and the outer bound at all. It is
            // reachable only by an allowance thicker than the moat, which
            // validate cannot see without the tool, so it rejects here.
            return Err(format!(
                "Clear operation feature '{}': at z {} the island stands to r {} and the moat's outer edge is r {}, so tool T{} (dia {}) does not fit between them — reduce stock_to_leave, or use a smaller tool",
                op.feature_name,
                f(level.z),
                f(island),
                f(rim + moat_width),
                op.tool_num,
                f(tool.dia)
            ));
        }
        // Equal radial bites, the axial law applied sideways: `stepover` is a
        // maximum, and the family lands EXACTLY on the island-adjacent pass
        // rather than stopping short of it or crossing it. The bore arm above
        // steps by a fixed `stepover` and rejects a leftover core instead;
        // there is no core here — the island is the bound — so the family can
        // be spaced exactly rather than truncated.
        let loops = ring_family(cx, cy, r_outer_centre, r_inner_centre, stepover, true);
        g.push(format!(
            "(level z{} — island r {}, {} pass{})",
            f(level.z),
            f(island),
            loops.len(),
            if loops.len() == 1 { "" } else { "es" }
        ));
        emit_loop_blocks(&mut g, op, &loops, level.z, ceiling, &vec![feed; loops.len()], plunge, rapid_z, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;

        // The hole, at the same depth: the same annulus machinery read in the
        // mirror — island = the pillar, outer edge = this level's ceiling.
        if i >= below_crown {
            let hole = &holes[i - below_crown];
            debug_assert!((hole.z - level.z).abs() < 1e-9, "the stacks share their levels");
            let r_ceiling_centre = hole.ceiling_r - r;
            let r_pillar_centre = pillar + r;
            if r_pillar_centre > r_ceiling_centre + 1e-9 {
                // Only reachable where the hole is narrower than the tool
                // itself: the pillar has already gone to nothing (it is the
                // rim less a diameter, floored at zero) and the tool still
                // cannot stand inside the ceiling.
                return Err(format!(
                    "Clear operation feature '{}': at z {} the hole's ceiling stands at r {} and tool T{} (dia {}) cannot reach inside it — the hole is narrower than the tool that must open it, so use a smaller one",
                    op.feature_name,
                    f(level.z),
                    f(hole.ceiling_r),
                    op.tool_num,
                    f(tool.dia)
                ));
            }
            let loops = ring_family(cx, cy, r_pillar_centre, r_ceiling_centre, stepover, false);
            g.push(format!(
                "(hole z{} — ceiling r {}, pillar r {}, {} pass{})",
                f(hole.z),
                f(hole.ceiling_r),
                f(pillar),
                loops.len(),
                if loops.len() == 1 { "" } else { "es" }
            ));
            emit_loop_blocks(&mut g, op, &loops, level.z, ceiling, &vec![feed; loops.len()], plunge, rapid_z, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;
        }
        ceiling = level.z;
    }

    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// One level's worth of concentric passes, from `r_first` to `r_last` (both
/// tool-centre radii, either order), spaced at `stepover` as a MAXIMUM so the
/// family lands exactly on both ends. `cw` is the winding: CW around an island,
/// CCW inside a hole — each climbing against the wall it defines.
///
/// The first ring is the one that gets the ramping entry, so the caller's
/// direction choice is also its entry choice.
/// `Clear` around a standing PRISMATIC boss (Phase 10 lane A, slice 1;
/// `DECISIONS.md` → "Phase 10 lane A — the boss finish's removal refuses ten
/// shipped fixtures…", fork 3A): the pocket spiral run OUTWARD, the boss as
/// its island, in Z levels down to the base. The stock is the one outer wall
/// that needs no ruling, so the family runs from the moat's edge —
/// `derive_moat_width`, what the finish's lead-in needs — in to the finish's
/// own tool-centre loop, outermost first, so the island-adjacent pass comes
/// last (the revolve moat's rule, for the same reason). Every loop climbs CW
/// around the island; each level ramps from the level above.
///
/// A DECLARED outer wall — a boss inside a pocket, lane D's island — is this
/// same family with the wall clipping it, which is where that build starts:
/// `parse::prismatic_boss_moat_family` is the one home, and the wall is the
/// parameter it does not take yet.
fn clear_prismatic_boss(p: &Part, op: &Operation, feature: &Feature) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("Clear operation requires stepover");
    let stepdown = op.stepdown.expect("validate requires stepdown on a boss");
    let skin = op.stock_to_leave.unwrap_or(0.0);
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let moat_width = crate::parse::derive_moat_width(p, op, feature, tool.dia);
    let (top, base) = crate::parse::prismatic_boss_z_span(feature, p.stock.z).expect("guarded by validate");
    let family = crate::parse::prismatic_boss_moat_family(feature, r, skin, stepover, moat_width)
        .ok_or_else(|| format!(
            "Clear operation feature '{}': the moat family could not be built for tool T{} (dia {}) — the boss outline does not offset outward to {:.3}mm",
            op.feature_name, op.tool_num, f(tool.dia), skin + moat_width
        ))?;
    // Outermost-in, each loop CW around the island (climb with M03).
    let loops: Vec<Vec<Pt>> = family
        .into_iter()
        .flatten()
        .map(|mut l| {
            l.reverse();
            l
        })
        .collect();

    // Equal bites from the top down to the base, `stepdown` a maximum.
    let levels = level_zs(top, base, Some(stepdown));

    let mut g = Vec::new();
    g.push(format!("(--- boss clear: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.push(format!(
        "(moat to {} off the wall — the allowance {} plus {} tool diameters, the finish's own and the one its lead-in stands out; stock outside it is declared by nothing and stays)",
        f(skin + moat_width),
        f(skin),
        f(moat_width / tool.dia)
    ));
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));
    let mut ceiling = top;
    for z in levels {
        g.push(format!("(level z{} — {} pass{})", f(z), loops.len(), if loops.len() == 1 { "" } else { "es" }));
        emit_loop_blocks(&mut g, op, &loops, z, ceiling, &vec![feed; loops.len()], plunge, rapid_z, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;
        ceiling = z;
    }
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// The levels of a Clear, deepest LAST: equal bites from `top` down to
/// `floor`, `stepdown` a maximum (Phase 61 slice 2 — born in the moat's arm,
/// where it was the whole stack, and lifted so every Clear cuts the same
/// way). No `stepdown` is ONE level, the floor: what every Clear without the
/// word has always cut, byte for byte.
pub(crate) fn level_zs(top: f64, floor: f64, stepdown: Option<f64>) -> Vec<f64> {
    let span = top - floor;
    let n = match stepdown {
        Some(s) if s > 0.0 && span > 0.0 => ((span / s).ceil() as usize).max(1),
        _ => 1,
    };
    (1..=n).map(|i| top - span * i as f64 / n as f64).collect()
}

fn ring_family(cx: f64, cy: f64, r_first: f64, r_last: f64, stepover: f64, cw: bool) -> Vec<Vec<Pt>> {
    let span = (r_last - r_first).abs();
    let n = if span <= 1e-9 { 0 } else { (span / stepover).ceil() as usize };
    let step = if n == 0 { 0.0 } else { (r_last - r_first) / n as f64 };
    (0..=n)
        .map(|k| {
            let rho = r_first + step * k as f64;
            let mut poly = vec![Pt { x: cx + rho, y: cy }];
            let ring = if cw {
                crate::ops::finish::circle_pts_cw(cx, cy, rho)
            } else {
                crate::ops::finish::circle_pts(cx, cy, rho)
            };
            poly.extend(ring[..ring.len() - 1].iter().map(|&(px, py)| Pt { x: px, y: py }));
            poly
        })
        .collect()
}

/// One entry's worth of loop blocks, innermost-out: the FIRST loop gets the
/// ramping entry (descend along its own profile from the stock top, then a
/// flat re-trace), every later loop a plunge block at its start. The pocket,
/// the bore family, and each rest REGION (which needs its own entry — its
/// material is standing) all emit through this one home.
/// The circle a closed loop chords, if it chords one: the centre and radius
/// of the least-squares circle through its vertices, when every vertex and
/// every chord's middle lie within two chord tolerances of it. A square's
/// corners lie on a circle and its sides do not, so a square is not one. The
/// rings of a Clear over a round blank or down a round bore are offsets of a
/// chorded circle and pass; a vertex the entry split onto a chord is within
/// the tolerance too.
pub(crate) fn loop_circle(lp: &[Pt]) -> Option<(f64, f64, f64)> {
    if lp.len() < 8 {
        return None;
    }
    // x² + y² + d·x + e·y + g = 0 is linear in d, e, g: the normal equations
    // of that fit, solved by Cramer's rule.
    let n = lp.len() as f64;
    let (mut sx, mut sy, mut sxx, mut sxy, mut syy, mut sxw, mut syw, mut sw) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for q in lp {
        let w = q.x * q.x + q.y * q.y;
        sx += q.x;
        sy += q.y;
        sxx += q.x * q.x;
        sxy += q.x * q.y;
        syy += q.y * q.y;
        sxw += q.x * w;
        syw += q.y * w;
        sw += w;
    }
    let a = [[sxx, sxy, sx], [sxy, syy, sy], [sx, sy, n]];
    let b = [-sxw, -syw, -sw];
    let det = |m: &[[f64; 3]; 3]| m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let d0 = det(&a);
    if d0.abs() < 1e-12 {
        return None;
    }
    let solve = |k: usize| {
        let mut m = a;
        for (row, bv) in m.iter_mut().zip(b.iter()) {
            row[k] = *bv;
        }
        det(&m) / d0
    };
    let (d, e, g) = (solve(0), solve(1), solve(2));
    let (cx, cy) = (-d / 2.0, -e / 2.0);
    let r2 = cx * cx + cy * cy - g;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();
    let tol = 2.0 * crate::CHORD_TOL_MM;
    for i in 0..lp.len() {
        let (p, q) = (lp[i], lp[(i + 1) % lp.len()]);
        let mid = Pt { x: (p.x + q.x) / 2.0, y: (p.y + q.y) / 2.0 };
        if ((p.x - cx).phypot(p.y - cy) - r).abs() > tol || ((mid.x - cx).phypot(mid.y - cy) - r).abs() > tol {
            return None;
        }
    }
    Some((cx, cy, r))
}

/// A closed loop's flat trace, from its first vertex round to it again: one
/// full-circle G02/G03 when the op writes `compress=arcs` and the loop chords
/// a circle (`loop_circle`), the chords otherwise. `state_feed` writes F on
/// the first chord, as the move after a plunge must; the arc always carries
/// it. The winding picks the direction word.
/// The direction said (ROADMAP item 2, slice 2), for a block whose every pass
/// has the material on one side — loops, rings, wall traces. Climb: under
/// M03 the material stands on the tool's right and each tooth enters at full
/// chip; conventional: on its left. Every loop the engine builds is wound to
/// climb; `direction=conventional` runs each the other way round.
/// The words a descent sentence ends with where `entry=` made it what it
/// is (ROADMAP item 2, slice 4); empty where the plan would have made the
/// same descent unasked.
pub(crate) fn asks(op: &Operation) -> String {
    op.entry.map_or_else(String::new, |k| format!(", as entry={} asks", k.word()))
}

/// A descent down a chain's first stroke by the ramp law (`RAMP_DEG`), the
/// row patterns' own (Phase 44 slice 2): backwards along the stroke toward
/// its start, so the pass cuts it flat going the way it was going anyway,
/// over the room the law needs and never under a diameter; a stroke too
/// short for that room is ramped in legs back and forth along it, and one
/// under a diameter cannot be ramped at all and plunges, said by name — or,
/// where the ramp was asked for, refuses. Leaves the tool at the chain's
/// start at depth; returns whether it plunged (the plunge feed stays modal).
#[allow(clippy::too_many_arguments)]
fn ramp_down_first_stroke(g: &mut Vec<String>, pts: &[Pt], z_from: f64, z_cut: f64, lift: f64, feed: f64, plunge: f64, dia: f64, asked: &str, op: &Operation) -> Result<bool, String> {
    let (a, b) = (pts[0], pts[1]);
    let stroke = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    let depth = (z_from - z_cut).max(0.0);
    let need = ramp_room(depth).max(dia);
    if stroke + 1e-9 < dia {
        if op.entry == Some(EntryKind::Ramp) {
            return Err(format!("entry=ramp on '{}': a pass's first stroke is {}mm long, under one diameter, and has no room to ramp", op.feature_name, f(stroke)));
        }
        g.push(format!("(plunge: the first stroke is {}mm long, under one diameter — no room to ramp)", f(stroke)));
        g.push(format!("G00 X{} Y{}", f(a.x), f(a.y)));
        g.push(format!("G00 Z{}", f(lift)));
        g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));
        return Ok(true);
    }
    if stroke + 1e-9 >= need {
        g.push(format!("(ramp: down the first stroke, {}mm of it, to hold {}deg{asked})", f(need), f(RAMP_DEG)));
        let t = need / stroke;
        let q = Pt { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t };
        g.push(format!("G00 X{} Y{}", f(q.x), f(q.y)));
        g.push(format!("G00 Z{}", f(lift)));
        g.push(format!("G01 Z{} F{}", f(z_from), f(plunge)));
        g.push(format!("G01 X{} Y{} Z{} F{}", f(a.x), f(a.y), f(z_cut), f(feed)));
    } else {
        let legs = (need / stroke).ceil().max(2.0) as usize;
        // The last leg ends at the start; legs alternate, so the first
        // begins at the start for an even count and at the far end for
        // an odd one.
        let at = |k: usize| if (legs - k) % 2 == 0 { a } else { b };
        g.push(format!("(ramp: {} legs along the first stroke, {}mm long, to hold {}deg{asked})", legs, f(stroke), f(RAMP_DEG)));
        g.push(format!("G00 X{} Y{}", f(at(0).x), f(at(0).y)));
        g.push(format!("G00 Z{}", f(lift)));
        g.push(format!("G01 Z{} F{}", f(z_from), f(plunge)));
        for k in 1..=legs {
            let z = z_from - depth * k as f64 / legs as f64;
            g.push(format!("G01 X{} Y{} Z{} F{}", f(at(k).x), f(at(k).y), f(z), f(feed)));
        }
    }
    Ok(false)
}

pub(crate) fn direction_note(op: &Operation) -> String {
    format!("(direction: {})", direction_words(op.climbs()))
}

fn direction_words(climb: bool) -> &'static str {
    if climb {
        "climb — the material on the tool's right under M03"
    } else {
        "conventional — the material on the tool's left under M03"
    }
}

/// The same for a block of rows. Rows that alternate have no one direction
/// and say so; rows cut one way say which side the uncut floor stands on;
/// the passes along the walls, which always have a side, say theirs.
pub(crate) fn rows_direction_note(rows: Option<bool>, walls: Option<bool>) -> String {
    let verb = |climb: bool| if climb { "climb" } else { "cut conventional" };
    let mut s = match rows {
        None => "the rows run both ways, climb and conventional by turns".to_string(),
        Some(climb) => format!("the rows {}, the uncut floor on the tool's {} under M03, every row the same way", verb(climb), if climb { "right" } else { "left" }),
    };
    if let Some(climb) = walls {
        s.push_str(&format!("; the passes along the walls {}", verb(climb)));
    }
    format!("(direction: {s})")
}

/// `direction=` said on an op whose rows alternate and whose plan has no pass
/// along a wall: nothing takes it, so the op refuses rather than read it
/// into nothing.
pub(crate) fn nothing_to_govern(op: &Operation, rows: &str) -> String {
    format!(
        "{} on '{}' with T{}: direction={} has nothing to govern — {}; pattern=oneway cuts every row one way, pattern=offset cuts loops",
        op.op_type.verb(),
        op.feature_name,
        op.tool_num,
        op.direction.map_or("", |d| d.word()),
        rows
    )
}

/// A loop run the other way round from the same start: the first point
/// stays, the rest reverse. For `direction=conventional`.
pub(crate) fn turned(l: &[Pt]) -> Vec<Pt> {
    let mut out = l.to_vec();
    if out.len() > 2 {
        out[1..].reverse();
    }
    out
}

/// The cutter-comp word for the side the tool stands on: G41 (left of the
/// programmed edge) climbing, G42 conventional — the same side of the same
/// edge, run the other way.
pub(crate) fn comp_word(op: &Operation) -> &'static str {
    if op.climbs() { "G41" } else { "G42" }
}

fn trace_loop(g: &mut Vec<String>, poly: &[Pt], feed: f64, state_feed: bool, arcs: bool) {
    let p0 = poly[0];
    if arcs {
        if let Some((cx, cy, _)) = loop_circle(poly) {
            let area: f64 = (0..poly.len())
                .map(|i| {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    a.x * b.y - b.x * a.y
                })
                .sum();
            g.push(crate::gcode::arc(area < 0.0, p0.x, p0.y, cx - p0.x, cy - p0.y, feed));
            return;
        }
    }
    for (k, q) in poly.iter().enumerate().skip(1) {
        g.push(if k == 1 && state_feed { format!("G01 X{} Y{} F{}", f(q.x), f(q.y), f(feed)) } else { format!("G01 X{} Y{}", f(q.x), f(q.y)) });
    }
    g.push(format!("G01 X{} Y{}", f(p0.x), f(p0.y)));
}

#[allow(clippy::too_many_arguments)]
fn emit_loop_blocks(g: &mut Vec<String>, op: &Operation, loops: &[Vec<Pt>], z_cut: f64, z_from: f64, feeds: &[f64], plunge: f64, rapid_z: f64, dia: f64, arcs: bool, conventional: bool) -> Result<(), String> {
    emit_loop_blocks_ramped(g, op, loops, None, z_cut, z_from, feeds, plunge, rapid_z, dia, arcs, conventional)
}

/// The same blocks with the entry stated per loop. `ramps` absent is the rule
/// above — the first loop ramps, the rest plunge beside air it cut. An
/// islanded pocket states it (Phase 26 fork 3A): its deepest level is
/// routinely SEVERAL disconnected loops (a round post in a square pocket ends
/// in four corners), and each of those stands in solid stock.
/// How one loop is entered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Entry {
    /// Down the loop's own profile from `z_from` — round it as many times as
    /// `RAMP_DEG` needs (Phase 44 slice 1), or a plunge said by name where
    /// the loop is under a diameter round — then a flat re-trace: the entry
    /// for a loop standing in solid stock with no air in reach.
    Ramp,
    /// Straight down at the loop's start: `Some(d)` beside air an earlier
    /// loop cut, its path `d` mm off; `None` in air outright — a face's first
    /// loop, a radius outside the blank.
    Plunge(Option<f64>),
    /// Plunge IN AIR at this point, outside the blank, then feed straight in
    /// to the loop's start at depth (Phase 27 slice 2): the side entry a
    /// machinist takes whenever a region has an open edge. It is a full-width
    /// cut, exactly as heavy as the loop it leads to — a family's first loop is
    /// a slot by construction — and it needs no ramp.
    Lead(Pt),
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_loop_blocks_ramped(
    g: &mut Vec<String>,
    op: &Operation,
    loops: &[Vec<Pt>],
    ramps: Option<&[Entry]>,
    z_cut: f64,
    // The height the ramp starts from. For a pocket that is the stock's top;
    // for one level of a revolve clear it is the level above, whose annulus is
    // already open (a ramp anchored at the stock's top would descend through
    // air it already owns).
    z_from: f64,
    // One feed word per loop, parallel to `loops` (Phase 9 lane A, slice 1):
    // a chip-thinned pocket rides a per-pass word, everything else repeats
    // the op's one word.
    feeds: &[f64],
    plunge: f64,
    rapid_z: f64,
    // The tool's diameter: a loop shorter round than this cannot be ramped
    // round at all (the tool would wind on the spot) and plunges instead.
    dia: f64,
    // The op writes `compress=arcs`: a loop that chords a circle is traced as
    // one G02/G03 (`trace_loop`). A ramp's laps stay chords; a helix is a
    // word this dialect does not write.
    arcs: bool,
    // `direction=conventional`: every loop the other way round from the same
    // start (ROADMAP item 2, slice 2). The entries were judged on the loops'
    // starts and their air, which turning does not move.
    conventional: bool,
) -> Result<(), String> {
    let turned_loops: Vec<Vec<Pt>>;
    let loops: &[Vec<Pt>] = if conventional {
        turned_loops = loops.iter().map(|l| turned(l)).collect();
        &turned_loops
    } else {
        loops
    };
    // The lead keys shape a lead, and a family with no loop entered from the
    // air makes none (slice 5).
    if LeadShape::of(op, dia).said && !ramps.is_some_and(|e| e.iter().any(|x| matches!(x, Entry::Lead(_)))) {
        return Err(LeadShape::nothing_to_shape(op, "no loop here is entered from the air"));
    }
    let mut is_first_loop = true;
    // The loop traced last: under the rule above, a plunge's sentence says
    // how far off the air that loop cut this one's start stands.
    let mut prev: Option<&[Pt]> = None;
    debug_assert_eq!(loops.len(), feeds.len(), "one feed word per loop");

    for (i, (poly, &feed)) in loops.iter().zip(feeds).enumerate() {
        // A family that STATES its entries counts on every loop being cut:
        // a later loop's plunge is licensed by the air an earlier one
        // opened. So its two-point loops are cut too — a region that erodes
        // to a line is a slot, out and back (Phase 27 slice 2: a step's
        // deepest level is exactly that, and skipping it sent the next
        // loop plunging into solid). The legacy rule skips them as it
        // always did, and ramps whichever loop comes first.
        if poly.len() < if ramps.is_some() { 2 } else { 3 } {
            continue;
        }
        let p0 = poly[0];

        let planned = match ramps {
            Some(e) => e[i],
            None if is_first_loop => Entry::Ramp,
            None => Entry::Plunge(prev.map(|pl| nearest_between(poly, pl, None).0)),
        };
        // The key makes every entry one kind (ROADMAP item 2, slice 4); the
        // sentence names it where it changed one. A lead needs air in reach,
        // and the air an earlier loop cut is not read for one yet, so a loop
        // the plan would ramp or plunge cannot be led into.
        let asked = asks(op);
        let shape = LeadShape::of(op, dia);
        let (entry, said) = match (op.entry, planned) {
            (Some(EntryKind::Plunge), Entry::Ramp) => (Entry::Plunge(None), Some(format!("(plunge: at the loop's start, into standing stock{asked})"))),
            (Some(EntryKind::Plunge), Entry::Lead(from)) => (Entry::Plunge(None), Some(format!("(plunge: at the loop's start, {}mm from the air at ({}, {}){asked})", f((p0.x - from.x).phypot(p0.y - from.y)), f(from.x), f(from.y)))),
            (Some(EntryKind::Ramp), Entry::Plunge(_) | Entry::Lead(_)) => (Entry::Ramp, None),
            (Some(EntryKind::Lead), Entry::Ramp | Entry::Plunge(_)) => {
                return Err(format!("entry=lead on '{}': a loop stands with no air in reach for a lead — the engine leads in from the air past the blank or in a given void, and the air an earlier loop cut is not read for one", op.feature_name));
            }
            (_, e) => (e, None),
        };
        let asked = if entry == planned { String::new() } else { asked };
        // Every descent is said before it is made (ROADMAP item 2): which of
        // the three entries this is, and what licensed it here.
        match entry {
            Entry::Lead(from) => {
                let down = if shape.descent_deg > 0.0 { format!(", descending {}deg", f(shape.descent_deg)) } else { String::new() };
                g.push(format!("(lead: from the air at ({}, {}), {}mm to the loop{down})", f(from.x), f(from.y), f((p0.x - from.x).phypot(p0.y - from.y))));
                g.push(format!("G00 X{} Y{}", f(from.x), f(from.y)));
                g.push(format!("G00 Z{}", f(rapid_z)));
                g.push(format!("G01 Z{} F{}", f(z_cut + shape.rise()), f(plunge)));
                g.push(format!("G01 X{} Y{}{} F{}", f(p0.x), f(p0.y), shape.z_word(z_cut), f(feed)));
                trace_loop(g, poly, feed, false, arcs);
                g.push(format!("G00 Z{}", f(rapid_z)));
            }
            Entry::Ramp => {
                g.push(format!("G00 X{} Y{}", f(p0.x), f(p0.y)));
                g.push(format!("G00 Z{}", f(rapid_z)));
                let n = poly.len();
                let segment_lengths: Vec<f64> = (0..n)
                    .map(|i| {
                        let (pa, pb) = (poly[i], poly[(i + 1) % n]);
                        (pb.x - pa.x).phypot(pb.y - pa.y)
                    })
                    .collect();
                let total_length: f64 = segment_lengths.iter().sum();
                let depth = (z_from - z_cut).max(0.0);
                // A plunged entry leaves the plunge feed modal: the first
                // flat move must state the cutting feed.
                let plunged = total_length + 1e-9 < dia;
                if plunged && op.entry == Some(EntryKind::Ramp) {
                    return Err(format!("entry=ramp on '{}': a loop is {}mm round, under one diameter, and has no room to ramp", op.feature_name, f(total_length)));
                }
                if plunged {
                    // Under one diameter round, the tool would wind on the
                    // spot: a plunge, at the plunge feed, said by name — never
                    // a descent at the cutting feed wearing a ramp's words
                    // (Phase 44 slice 1; the wrench's innermost loop was 0.05mm
                    // round and took 4mm at F800).
                    g.push(format!("(plunge: this loop is {}mm round, under one diameter — no room to ramp)", f(total_length)));
                    g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));
                } else {
                    // Plunge to the height the ramp starts from, then ramp down
                    // along the profile of the loop — as many LAPS as the law
                    // needs (`RAMP_DEG`): one for a loop with the room, the
                    // descent spread evenly over all of them otherwise, so no
                    // line is steeper than the law. A ceiling, not a target: a
                    // loop longer than the room descends over one lap.
                    let laps = (ramp_room(depth) / total_length).ceil().max(1.0) as usize;
                    g.push(format!("(ramp: {} lap{} of this loop, {}mm round, to hold {}deg{asked})", laps, if laps == 1 { "" } else { "s" }, f(total_length), f(RAMP_DEG)));
                    g.push(format!("G01 Z{} F{}", f(z_from), f(plunge)));
                    let total = laps as f64 * total_length;
                    let mut cum_dist = 0.0;
                    for _ in 0..laps {
                        for i in 0..n {
                            let next_pt = poly[(i + 1) % n];
                            cum_dist += segment_lengths[i];
                            let ratio = (cum_dist / total).min(1.0);
                            let z_curr = z_from - ratio * depth;
                            g.push(format!("G01 X{} Y{} Z{} F{}", f(next_pt.x), f(next_pt.y), f(z_curr), f(feed)));
                        }
                    }
                }
                // Trace the loop once more at full depth to ensure a flat bottom
                trace_loop(g, poly, feed, plunged, arcs);
                g.push(format!("G00 Z{}", f(rapid_z)));
            }
            Entry::Plunge(beside) => {
                g.push(said.unwrap_or_else(|| match beside {
                    Some(d) => format!("(plunge: beside air an earlier loop cut, {}mm off its path)", f(d)),
                    None => "(plunge: in air outside the blank)".to_string(),
                }));
                g.push(format!("G00 X{} Y{}", f(p0.x), f(p0.y)));
                g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));
                trace_loop(g, poly, feed, true, arcs);
                g.push(format!("G00 Z{}", f(rapid_z)));
            }
        }
        is_first_loop = false;
        prev = Some(poly);
    }
    Ok(())
}

/// `strategy=regions` (DECISIONS.md "adaptive clearing, region
/// decomposition…", slice 2): per-region offset spirals over the standing
/// material a prior op left inside the declared pocket. Three authorities,
/// one job each — the FIELD shapes the work-list (the mask below), the
/// DECLARATION keeps the walls (the legal set is an exact erosion of the
/// authored polygon, computed from authored geometry and never from the
/// field), and the FAMILY still cuts (the offsetter's loops, emitted through
/// the same machinery as the pocket). The gouge-safety law falls out of the
/// clamp: a field error can only move passes within authorized air.
///
/// The masks, all on the field's own cell centers at the pinned resolution:
/// - W (work): field material at the first cell layer above the cutting
///   depth — material is DOWNWARD-closed inside the stock (void is
///   upward-closed, its contrapositive), so every standing column shows at
///   that one layer — and inside the authored polygon.
/// - L (legal): inside the polygon by at least r + skin + one cell. Region
///   boundary vertices sit on cell-square corners of L-true cells (≤
///   cell·√2/2 from the certifying center), so every emitted point keeps
///   r + skin of declaration-exact clearance.
/// - C (the tool-center work-list): L, with some W within the tool radius.
///
/// Work no legal center reaches is the RESIDUE LAW's subject: counted,
/// stated in the op's own text as a smaller tool's work, standing in the
/// rest-material report — never a hard error (a derived region is nobody's
/// authored intent; the only knob is a smaller tool, which is exactly the
/// next regions op) and never silent.
fn clear_regions(p: &Part, op: &Operation, field: &VoxField) -> Result<Vec<String>, String> {
    let mut g = Vec::new();
    let feature = p.features.get(&op.feature_name).expect("feature must exist");
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let stepover = crate::parse::derive_stepover(p, op).expect("Clear operation requires stepover");
    let skin = op.stock_to_leave.unwrap_or(0.0);

    // The one feature-specific fact in this path: the declared walls, asked
    // one question — how far a plan point sits INSIDE them (≤ 0 = outside).
    // Everything downstream — masks, tracer, spirals, residue — is shared.
    let (walls, z_from, z_cut, title) = match feature {
        Feature::ProfilePocket { points, z, extent, .. } => (
            // An island's wall is a declared wall too (Phase 26 slice 2): a
            // second clamp, the mask machinery untouched. With none, the
            // plain polygon — byte for byte the walls every regions golden
            // was cut against.
            match crate::stock::island_holes(p, &op.feature_name, 0.0) {
                holes if holes.is_empty() => DeclaredWalls::Polygon(make_ccw(points)),
                holes => DeclaredWalls::Islanded { outer: make_ccw(points), holes },
            },
            derive_contour_top(p.stock.z, *z),
            derive_z_cut(derive_contour_top(p.stock.z, *z), *extent) + crate::parse::derive_floor_skin(p, op),
            format!("(--- pocket regions: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)),
        ),
        // The bore arm (the DRAFT's named machinery-sharing follow-up): the
        // declared wall is a circle, clearance is R − ρ, and nothing else
        // changes — the field still shapes the work-list, so the standing
        // material need not be concentric (the dense bore family is), and a
        // lune left by an overlapping prior clear gets exactly its own spiral.
        Feature::CylinderBore { dia, extent, x, y, z, .. } => (
            DeclaredWalls::Circle { cx: *x, cy: *y, radius: dia / 2.0 },
            derive_contour_top(p.stock.z, *z),
            derive_z_cut(derive_contour_top(p.stock.z, *z), *extent),
            format!("(--- bore regions: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)),
        ),
        _ => panic!("validate: strategy=regions covers ProfilePocket and CylinderBore"),
    };

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let cell = field.cell();

    // The local grid: the declaration's bbox in field cell indices, one false
    // ring around it so region boundaries at the edge trace closed.
    let (c0x, c0y, _) = field.cell_center(0, 0, 0);
    let (ox, oy) = (c0x - cell / 2.0, c0y - cell / 2.0);
    let (bx0, by0, bx1, by1) = walls.bbox();
    let cells = field.cells() as isize;
    let clampc = |v: isize| v.clamp(0, cells - 1) as usize;
    let i0 = clampc(((bx0 - ox) / cell).floor() as isize - 1);
    let j0 = clampc(((by0 - oy) / cell).floor() as isize - 1);
    let i1 = clampc(((bx1 - ox) / cell).ceil() as isize + 1);
    let j1 = clampc(((by1 - oy) / cell).ceil() as isize + 1);
    let (nx, ny) = (i1 - i0 + 1, j1 - j0 + 1);

    // Work is read one layer above the floor: the floor cell itself straddles
    // z_cut, and downward-closed material makes the layer above answer for
    // every column that stands anywhere in (z_cut, top].
    let z_q = z_cut + 0.5 * cell;
    let mut work = vec![false; nx * ny];
    let mut legal = vec![false; nx * ny];
    for j in 0..ny {
        for i in 0..nx {
            let (x, y, _) = field.cell_center(i0 + i, j0 + j, 0);
            let clr = walls.clearance(x, y);
            if clr <= 0.0 {
                continue;
            }
            work[j * nx + i] = field.material_at(x, y, z_q);
            legal[j * nx + i] = clr >= r + skin + cell;
        }
    }
    // C = legal with work within the tool radius; reach = C dilated back by
    // the same radius. Both dilations use the same disc so the residue answer
    // is the complement of exactly what the passes can cover.
    let rc = (r / cell).ceil() as isize;
    let disc: Vec<(isize, isize)> = (-rc..=rc)
        .flat_map(|di| (-rc..=rc).map(move |dj| (di, dj)))
        .filter(|&(di, dj)| ((di * di + dj * dj) as f64) * cell * cell <= r * r)
        .collect();
    let dilate = |src: &[bool]| -> Vec<bool> {
        let mut out = vec![false; nx * ny];
        for j in 0..ny as isize {
            for i in 0..nx as isize {
                if !src[j as usize * nx + i as usize] {
                    continue;
                }
                for &(di, dj) in &disc {
                    let (a, b) = (i + di, j + dj);
                    if a >= 0 && b >= 0 && (a as usize) < nx && (b as usize) < ny {
                        out[b as usize * nx + a as usize] = true;
                    }
                }
            }
        }
        out
    };
    let near_work = dilate(&work);
    let c_mask: Vec<bool> = (0..nx * ny).map(|k| legal[k] && near_work[k]).collect();
    let reach = dilate(&c_mask);
    let residue = (0..nx * ny).filter(|&k| work[k] && !reach[k]).count();

    let regions = crate::vox::trace_regions(
        &c_mask,
        nx,
        ny,
        ox + i0 as f64 * cell,
        oy + j0 as f64 * cell,
        cell,
    );

    g.push(title);
    let residue_line = |g: &mut Vec<String>| {
        if residue > 0 {
            g.push(format!(
                "(regions residue: {} cells of standing work no legal tool position reaches (~{:.1} mm2 at the cutting level) — a smaller tool's work)",
                residue,
                residue as f64 * cell * cell,
            ));
        }
    };
    if regions.is_empty() {
        // An answer, not an error — and WHICH answer is stated: work that
        // stands unreachable is the residue law's case, not "nothing stands".
        if residue > 0 {
            g.push(format!(
                "(regions: no reachable work at resolved cell {}mm; no cutting moves)",
                f(cell),
            ));
            residue_line(&mut g);
        } else {
            g.push(format!(
                "(regions: nothing stands — no work within the declaration at resolved cell {}mm; no cutting moves)",
                f(cell),
            ));
        }
        return Ok(g);
    }
    g.push(format!(
        "(regions: {} standing region{} at resolved cell {}mm)",
        regions.len(),
        if regions.len() == 1 { "" } else { "s" },
        f(cell),
    ));
    residue_line(&mut g);
    g.push(direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));

    for region in &regions {
        let boundary: Vec<Pt> = region.iter().map(|&(x, y)| Pt { x, y }).collect();
        // The region's family: its own boundary is the outermost pass (the
        // tool-center work-list's rim), then inward offsets by stepover.
        let mut steps: Vec<Vec<Vec<Pt>>> = vec![vec![boundary.clone()]];
        let mut dist = stepover;
        let mut last = 0.0;
        loop {
            // (Vertices only, by name — Phase 37 slice 0. The boundary is the
            // tool-centre work-list itself, so every point inside it is a
            // legal centre: a chord across an inner loop's arc stays inside
            // it and can cost coverage, never a wall. The plain pocket's
            // boundary is the WALL, which is why that family follows its arcs.)
            let s = offset_closed_polyline(&boundary, dist);
            if s.is_empty() {
                break;
            }
            steps.push(s);
            last = dist;
            dist += stepover;
        }
        // The pocket's island law, per region: the middle of a region is
        // REACHABLE by construction (C is inside the legal erosion), so a
        // spiral that outruns it is a stepover problem with an authored knob
        // — a hard error, unlike the residue.
        if !offset_closed_polyline(&boundary, last + r).is_empty() {
            return Err(format!(
                "regions clear on '{}' leaves an uncut middle in a standing region: tool T{}'s last pass reaches {:.3}mm from the region boundary but the region runs deeper — reduce stepover",
                op.feature_name, op.tool_num, last + r
            ));
        }
        steps.reverse();
        let flat: Vec<Vec<Pt>> = steps.into_iter().flatten().collect();
        // Every region is standing material: each gets its own ramp entry,
        // descending from the feature's rim (Phase 45).
        emit_loop_blocks(&mut g, op, &flat, z_cut, z_from, &vec![feed; flat.len()], plunge, rapid_z, tool.dia, op.compress == CompressMode::Arcs, !op.climbs())?;
    }

    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// The declared walls a regions op is clamped by, reduced to the one
/// question the masks ask: signed clearance from a plan point to the wall,
/// positive inside. The clamp stays declaration-exact — clearance is computed
/// from the authored geometry (polygon or circle), never from the field.
enum DeclaredWalls {
    Polygon(Vec<Pt>),
    /// A polygon with standing outlines inside it: the clearance is to the
    /// NEAREST wall, the pocket's or an island's, and inside an island is
    /// outside the walls.
    Islanded { outer: Vec<Pt>, holes: Vec<Vec<Pt>> },
    Circle { cx: f64, cy: f64, radius: f64 },
}

impl DeclaredWalls {
    fn clearance(&self, x: f64, y: f64) -> f64 {
        match self {
            DeclaredWalls::Polygon(pts) => {
                if crate::stock::point_in_polygon(pts, x, y) {
                    crate::stock::dist_to_polygon(pts, x, y)
                } else {
                    // Outside is outside; the magnitude is never read.
                    -1.0
                }
            }
            DeclaredWalls::Islanded { outer, holes } => {
                if !crate::stock::point_in_polygon(outer, x, y) || holes.iter().any(|h| crate::stock::point_in_polygon(h, x, y)) {
                    return -1.0;
                }
                holes.iter().map(|h| crate::stock::dist_to_polygon(h, x, y)).fold(crate::stock::dist_to_polygon(outer, x, y), f64::min)
            }
            DeclaredWalls::Circle { cx, cy, radius } => radius - ((x - cx).powi(2) + (y - cy).powi(2)).sqrt(),
        }
    }

    fn bbox(&self) -> (f64, f64, f64, f64) {
        match self {
            DeclaredWalls::Polygon(pts) | DeclaredWalls::Islanded { outer: pts, .. } => {
                let (mut bx0, mut by0, mut bx1, mut by1) =
                    (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for pt in pts {
                    bx0 = bx0.min(pt.x);
                    by0 = by0.min(pt.y);
                    bx1 = bx1.max(pt.x);
                    by1 = by1.max(pt.y);
                }
                (bx0, by0, bx1, by1)
            }
            DeclaredWalls::Circle { cx, cy, radius } => (cx - radius, cy - radius, cx + radius, cy + radius),
        }
    }
}

pub(crate) fn make_ccw(points: &[Pt]) -> Vec<Pt> {
    let mut pts = points.to_vec();
    if pts.len() > 1 && pts.first() == pts.last() {
        pts.pop();
    }
    if pts.len() < 3 {
        return pts;
    }
    let mut area = 0.0;
    let n = pts.len();
    for i in 0..n {
        let p1 = &pts[i];
        let p2 = &pts[(i + 1) % n];
        area += p1.x * p2.y - p2.x * p1.y;
    }
    if area < 0.0 {
        pts.reverse();
    }
    pts
}

#[cfg(test)]
mod tests {
    use crate::{gcode, parse};

    // The guarantee, re-derived independently of any derive_*: for a roughing
    // pocket the wall pass must sit exactly `skin` off the authored boundary,
    // everywhere. This is the prismatic analogue of ops::finish's
    // assert_cusp_tips_within_allowance — it measures the emitted G-code
    // against the authored geometry rather than trusting the offset call.
    //
    // The wall pass is the LAST loop emitted (pocket machines innermost-out),
    // so the check is: every point of the final loop is >= r + skin from the
    // boundary, and the closest one is exactly r + skin. A skin that leaked
    // into the wrong loop, or a sign error, breaks the minimum.
    fn wall_clearance(src: &str) -> Vec<(f64, f64)> {
        let part = parse::parse(src).expect("fixture must parse");
        let g = gcode::program(&part).expect("fixture must generate");
        // Collect XY pairs from the tail of the program: the last cutting loop
        // before the closing retract is the wall pass.
        let mut pts = Vec::new();
        for line in g.lines() {
            if !line.starts_with("G01 X") && !line.starts_with("G00 X") { continue; }
            let (mut x, mut y) = (None, None);
            for tok in line.split_whitespace() {
                if let Some(v) = tok.strip_prefix('X') { x = v.parse::<f64>().ok(); }
                if let Some(v) = tok.strip_prefix('Y') { y = v.parse::<f64>().ok(); }
            }
            if let (Some(x), Some(y)) = (x, y) { pts.push((x, y)); }
        }
        pts
    }

    fn dist_to_square_boundary(x: f64, y: f64, lo: f64, hi: f64) -> f64 {
        // Inside an axis-aligned square: distance to the nearest of four edges.
        (x - lo).min(hi - x).min(y - lo).min(hi - y)
    }

    #[test]
    fn test_pocket_wall_pass_rides_the_skin_parallel() {
        let src = std::fs::read_to_string("profile_pocket_clear_rough.cam/main.part").expect("read profile_pocket_clear_rough.cam");
        let pts = wall_clearance(&src);
        // r = 5, skin = 0.5, boundary 20..80.
        let closest = pts
            .iter()
            .map(|&(x, y)| dist_to_square_boundary(x, y, 20.0, 80.0))
            .fold(f64::INFINITY, f64::min);
        assert!(
            (closest - 5.5).abs() < 1e-6,
            "the wall pass must sit exactly r + skin = 5.5 off the boundary, measured {closest}"
        );
    }

    #[test]
    fn test_pocket_without_an_allowance_rides_the_tool_radius() {
        // The same measurement at skin 0, so the test above cannot pass by the
        // measurement itself being off by a constant.
        let src = std::fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
        let pts = wall_clearance(&src);
        let closest = pts
            .iter()
            .map(|&(x, y)| dist_to_square_boundary(x, y, 20.0, 80.0))
            .fold(f64::INFINITY, f64::min);
        assert!(
            (closest - 5.0).abs() < 1e-6,
            "a finishing pocket must sit exactly r = 5.0 off the boundary, measured {closest}"
        );
    }

    // A square inside a square is equally near it at every point of four
    // straight runs. The start is the tied point nearest the tool, wherever
    // the tool is, and the first by vertex order when the tool has no place.
    #[test]
    fn a_loop_starts_at_the_tied_point_nearest_the_tool() {
        use crate::parse::Pt;
        let square = |h: f64| vec![Pt { x: -h, y: -h }, Pt { x: h, y: -h }, Pt { x: h, y: h }, Pt { x: -h, y: h }];
        let (outer, inner) = (square(20.0), square(15.0));
        let start = |tool: Option<Pt>| {
            let (d, _, at) = super::nearest_between(&outer, &inner, tool);
            assert!((d - 5.0).abs() < 1e-9, "the loops stand 5 apart, measured {d}");
            (at.x, at.y)
        };
        assert_eq!(start(Some(Pt { x: 30.0, y: 14.0 })), (20.0, 15.0));
        assert_eq!(start(Some(Pt { x: -30.0, y: -14.0 })), (-20.0, -15.0));
        assert_eq!(start(Some(Pt { x: -14.0, y: 30.0 })), (-15.0, 20.0));
        assert_eq!(start(None), (-15.0, -20.0));
    }
}
