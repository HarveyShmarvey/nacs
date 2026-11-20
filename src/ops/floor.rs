// ---- floor: one pass at a floor's depth, walked by a pattern -------------------
// The default is spine (Phase 35 slice 4). `floor_raster` below is the first
// raster, reached by `pattern=raster`: tool centre sweeps in X, stepping over
// in Y, overhanging the X ends by one radius.

use crate::gcode::f;
use crate::parse::{Feature, Operation, Part, Pt};

pub fn floor(p: &Part, op: &Operation) -> Result<Vec<String>, String> {
    // A boss standing THROUGH the face is an island of it (Phase 26 slice 3;
    // `DECISIONS.md` → "Phase 26 slice 3: the face…"): the raster below would
    // sweep it, so the face is cut around it instead. With nothing standing,
    // this is the raster it always was, byte for byte.
    // …and so is a declared FIXTURE standing on it (Phase 27 slice 3): the
    // raster used to warn "steel meeting steel" and drive through the clamp
    // anyway. Both come from the one derived home, `region::of`.
    // The rest floor (Phase 33): only what an earlier op left standing.
    if crate::rest::is_rest(op) {
        return rest_pass(p, op);
    }
    match p.features.get(&op.feature_name) {
        Some(Feature::ProfilePocket { .. }) => return crate::ops::clear::pocket_floor_pass(p, op),
        Some(Feature::Face { .. }) => return crate::ops::clear::face_pass(p, op),
        _ => {}
    }
    // What the op cuts has one home, `derive_pattern`: the pin, or spine
    // (Phase 35 slice 4). `raster` is the one pattern that reads no region.
    let pattern = crate::parse::derive_pattern(p, op).expect("an End works a region");
    if pattern == crate::parse::Pattern::Raster {
        return floor_raster(p, op);
    }
    let Some(Feature::Plane { z }) = p.features.get(&op.feature_name) else { unreachable!("the other floors returned above") };
    let region = crate::region::of(p, op).ok_or_else(|| format!("End operation feature '{}' lies outside the stock", op.feature_name))?;
    if pattern == crate::parse::Pattern::Offset {
        return floor_around(p, op, &region);
    }
    let mut g = vec![format!("(--- floor: {} with T{} ---)", op.feature_name, op.tool_num)];
    g.extend(crate::ops::clear::region_notes(&region, false));
    g.extend(crate::ops::clear::bridged_note(p, op));
    g.extend(crate::ops::clear::pattern_pass(p, op, &region, pattern, 0.0, *z, p.stock.z, true)?);
    Ok(g)
}

/// `strategy=rest` (Phase 33 slice 1; `DECISIONS.md` → "Phase 33 slice 1…"):
/// each floor the earlier op left standing faced as the `Face` it is, in the
/// order the concession found them (largest first) — the simplest order, not
/// the shortest; nearest-next is also viable and not yet measured.
fn rest_pass(p: &Part, op: &Operation) -> Result<Vec<String>, String> {
    let (k, _) = crate::rest::standing(p, op)?;
    let pieces = crate::rest::pieces(p, op)?;
    let mut g = vec![format!(
        "(--- floor: {} with T{} — the rest: {} floor{} op {} left standing ---)",
        op.feature_name,
        op.tool_num,
        pieces.len(),
        if pieces.len() == 1 { "" } else { "s" },
        k + 1
    )];
    // Judged once, over the whole rest floor (slice 3): the op's refusal or
    // its note, never a piece's.
    g.extend(crate::rest::concession(p, op, &pieces)?);
    for (i, pc) in pieces.iter().enumerate() {
        let (cx, cy, a) = centroid(pc.outline());
        if !pc.is_cut() {
            g.push(format!("(rest {} of {}: {:.1}mm² near ({}, {}) — no pass of T{} reaches it, conceded whole)", i + 1, pieces.len(), a, f(cx), f(cy), op.tool_num));
            continue;
        }
        g.push(format!("(rest {} of {}: {:.1}mm² near ({}, {}))", i + 1, pieces.len(), a, f(cx), f(cy)));
        // Each floor's own body, without its title (the op has one) and
        // without the piece's own concession lines (the op said them above).
        let body = crate::ops::clear::face_pass(&pc.part, &pc.op)?;
        // The direction is the op's: the first cut piece's line says it for
        // the block, the later pieces' are dropped.
        let mut said = g.iter().any(|l| l.starts_with("(direction:"));
        g.extend(body.into_iter().skip(1).filter(|l| {
            if l.starts_with("(direction:") {
                let first = !said;
                said = true;
                return first;
            }
            !l.starts_with("(unreachable:") && !l.starts_with("(the rest:")
        }));
    }
    Ok(g)
}

fn centroid(l: &[Pt]) -> (f64, f64, f64) {
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..l.len() {
        let (p0, p1) = (l[i], l[(i + 1) % l.len()]);
        let c = p0.x * p1.y - p1.x * p0.y;
        a += c;
        cx += (p0.x + p1.x) * c;
        cy += (p0.y + p1.y) * c;
    }
    (cx / (3.0 * a), cy / (3.0 * a), a.abs() / 2.0)
}

fn floor_raster(p: &Part, op: &Operation) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;

    let feature = p.features.get(&op.feature_name).expect("feature must exist");
    // The Plane IS the finished face, so facing cuts straight to it — there is
    // no travel to derive (DECISIONS.md "Vertical occupancy is `extent`…"). A
    // plane's own Z span is zero, which is why it needs no `extent` to go with
    // its datum.
    let z_cut = match feature {
        Feature::Plane { z } => *z,
        _ => panic!("End operation requires a Plane feature"),
    };

    let stepover = op.stepover.expect("End operation requires stepover");

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let x0 = p.stock.x - r;
    let x1 = p.stock.x + p.stock.w + r;

    // pass centers in Y, guaranteed to cover [oy, oy+L]
    let mut ys = Vec::new();
    let n = (p.stock.l / stepover).ceil() as usize;
    for i in 0..=n {
        ys.push(p.stock.y + (i as f64 * stepover).min(p.stock.l));
    }

    // The rows alternate and nothing else is cut: a direction said here has
    // nothing to govern (ROADMAP item 2, slice 2).
    if op.direction.is_some() {
        return Err(crate::ops::clear::nothing_to_govern(op, "the raster's rows run both ways by turns and nothing else is cut"));
    }
    // The raster's one descent is a plunge in air off the blank's edge, and
    // nothing else is built for it (slice 4).
    if let Some(k) = op.entry.filter(|k| *k != crate::parse::EntryKind::Plunge) {
        return Err(format!("entry={} on '{}': the raster plunges in air off the blank's edge, and nothing else is built for it", k.word(), op.feature_name));
    }
    if crate::ops::lead::LeadShape::of(op, tool.dia).said {
        return Err(crate::ops::lead::LeadShape::nothing_to_shape(op, "the raster plunges in air off the blank's edge"));
    }
    let mut g = Vec::new();
    g.push(format!("(--- floor: {} with T{} ---)", op.feature_name, op.tool_num));
    g.extend(crate::ops::clear::bridged_note(p, op));
    g.push(crate::ops::clear::rows_direction_note(None, None));
    g.push("(plunge: in air off the blank's edge)".to_string());
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(x0), f(ys[0])));
    g.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));

    let mut at_x0 = true; // current X end the tool sits at
    for (i, &y) in ys.iter().enumerate() {
        let target = if at_x0 { x1 } else { x0 };
        if i == 0 {
            g.push(format!("G01 X{} F{}", f(target), f(feed)));
        } else {
            g.push(format!("G01 Y{}", f(y)));        // step over at feed
            g.push(format!("G01 X{}", f(target)));   // cut the pass
        }
        at_x0 = !at_x0;
    }
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

/// The face around standing bosses: the pocket's island family
/// (`offset::shape_levels`) with the BLANK as the outer wall — lane A's moat
/// "run outward to the blank's edge", seen from the other wall.
///
/// **The outer wall is the blank grown by one tool diameter**, so the first
/// wall-parallel loop rides one radius outside the stock — the raster's own
/// overhang — and the op ENTERS FROM AIR with a plain plunge.
///
/// **Order: outside-in, island passes last.** The wall-parallel loops from
/// the outermost in (each beside the air the last one cut), then the
/// island-parallel loops from the deepest level back out to the island's own
/// wall. It is the mirror of the closed pocket's order for the mirrored
/// reason: there the air is nowhere and the op ramps into the middle; here the
/// air is all around. A loop plunges where it starts beside air already cut —
/// started at its point nearest that air — and ramps from the stock top where
/// it does not (a pad of stock ringed by islands).
///
/// One pass to depth, as the raster is: a `Plane` IS the finished face and
/// facing cuts straight to it.
fn floor_around(p: &Part, op: &Operation, region: &crate::region::Region) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let Some(Feature::Plane { z: z_cut }) = p.features.get(&op.feature_name) else {
        panic!("End operation requires a Plane feature");
    };
    let stepover = op.stepover.expect("End operation requires stepover");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    let grow = tool.dia;
    let grown = crate::parse::derive_blank_grown(&p.stock, grow);
    // A keep-out that straddles even the grown blank — a long clamp over the
    // stock's edge — is a bite out of the boundary, not an island in it.
    let (wall, outlines) = crate::region::split_islands(grown, &region.islands()).ok_or_else(|| format!(
        "End operation feature '{}': a keep-out across the stock's edge leaves no simple region to face",
        op.feature_name
    ))?;

    let levels = crate::offset::shape_levels(&wall, &outlines, r, stepover);
    assert!(!levels.is_empty(), "a blank grown by a diameter always takes the first loop");
    // The web (Phase 26 fork 4A) — every stretch of floor this tool cannot
    // enter — is the CONCESSION's to judge, as it has been on a pocket and a
    // `Face` since Phase 32 slice 2; the plane was missed then (Phase 33
    // slice 5). It refuses naming every stretch, the `cover=` that would
    // concede them and the rest op that would take them — or carries the note.
    let conceded = crate::ops::clear::concession(p, op, region, r, 0.0)?;
    // The ridge law is about ridges, and a face with nothing in it has none:
    // `pattern=offset` on a bare plane is loops round an empty rectangle. But
    // a ridge is not the only way to leave floor (Phase 37 slice 2): the
    // family stops when the next offset prunes empty, which can be short of
    // the middle, and two loops far enough apart strand a corner. The bare
    // plane refuses both in the sentences the pocket uses — its reach said
    // from the stock's edge, a tool's width inside the boundary it erodes.
    let what = format!("End operation feature '{}'", op.feature_name);
    if !outlines.is_empty() {
        crate::ops::clear::island_stepover_law(&what, op.tool_num, r, stepover)?;
    } else {
        // (A keep-out long enough to bite even the grown blank is in `wall`,
        // not in `outlines`, and the boundary turns in round it.)
        crate::ops::clear::inward_wall_stepover_law(&what, op.tool_num, &wall, r, stepover)?;
        let last_d = levels.last().expect("asserted non-empty above").0;
        if !crate::offset::offset_shape(&wall, &outlines, last_d + r + crate::ops::clear::MIDDLE_HAIR).is_empty() {
            return Err(crate::ops::clear::uncut_middle(&what, op.tool_num, last_d + r - grow, "the stock's edge"));
        }
        // (Judged on the blank's own corners — the angles are the grown
        // boundary's, and the corner the refusal names is one the author drew.)
        crate::ops::clear::corner_stepover_law(&what, op.tool_num, &crate::region::blank_of(p), r, stepover)?;
    }

    let flatten = |lp: &Vec<crate::offset::OffsetVertex>| crate::offset::flatten_loop(lp);
    let ordered: Vec<Vec<Pt>> = levels
        .iter()
        .flat_map(|(_, l)| l.ccw.iter().map(flatten))
        .chain(levels.iter().rev().flat_map(|(_, l)| l.cw.iter().map(flatten)))
        .collect();
    let (loops, ramps) = crate::ops::clear::enter_beside_cut_air(ordered, stepover);

    let mut g = Vec::new();
    g.push(format!("(--- floor: {} with T{} ---)", op.feature_name, op.tool_num));
    if !region.feature_islands().is_empty() {
        g.push(format!("(islands: {})", region.feature_islands().join(", ")));
    }
    g.extend(crate::ops::clear::shore_note(region));
    if !region.keep_outs().is_empty() {
        g.push(format!("(keep-outs: {})", region.keep_outs().join(", ")));
    }
    g.extend(crate::ops::clear::open_void_note(region));
    g.extend(crate::ops::clear::bridged_note(p, op));
    g.extend(conceded);
    g.push(crate::ops::clear::direction_note(op));
    g.push(format!("G00 Z{}", f(clearance_z)));
    // Between loops the tool lifts to `rapid_z` — a few millimetres over the
    // stock, which is over a boss and UNDER a clamp. Where steel stands in the
    // region every lift goes to the clearance plane instead; whether THAT
    // clears the clamp is the keep-out check's question (V2), and it asks it.
    let lift = if region.keep_outs().is_empty() { rapid_z } else { clearance_z };
    crate::ops::clear::emit_loop_blocks_ramped(&mut g, op, &loops, Some(&ramps), *z_cut, p.stock.z, &vec![feed; loops.len()], plunge, lift, tool.dia, op.compress == crate::parse::CompressMode::Arcs, !op.climbs())?;
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}
