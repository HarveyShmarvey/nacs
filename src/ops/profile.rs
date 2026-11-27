use crate::math::Portable as _;
// ---- profile: exact-flank finishing of the polyline prismatic walls --------
// The former Contour op's machinery, reached through `Finish` since the op
// merge (DECISIONS.md "Geometry-first fixture naming…"): wall tracing is one
// operation regardless of cross-section, so this module is `Finish`'s polyline
// arm the way finish_cylinder_bore/finish_cylinder_boss are its circular ones.
// Two comp modes (DECISIONS.md "Finishing comp strategy: a single per-op `comp`
// mode"):
//   - control:      emit centerline = part edge with G41 + the D register and
//                   let the controller apply cutter radius comp (opt-in, so
//                   wear/deflection can be dialed in at the machine).
//   - engine (default): compute the tool-center offset path here and emit plain
//                   coordinates with no comp active — portable, self-contained.
// Both lead in/out radially through the centroid so the approach ramps on and
// off without gouging the start point.

use crate::gcode::{arc, f, stock_suffix, OpMoves};
use crate::ops::clear::{comp_word, direction_note};
use crate::offset::{arc_center, offset_closed_polyline_verts, OffsetVertex};
use crate::ops::lead::LeadShape;
use crate::parse::{derive_contour_top, derive_d, derive_z_cut, signed_area, CompMode, Feature, Operation, Part, Pt, Pt3};
use crate::stock::EntryRoom;

pub fn finish_profile_wall(p: &Part, op: &Operation) -> Result<OpMoves, String> {
    // Feature existence/type/point-count were checked by parse::validate();
    // violations here are internal invariant bugs.
    let feature = p.features.get(&op.feature_name).expect("feature must exist");
    let (points, z_cut) = match feature {
        Feature::ProfilePocket { points, z, extent, .. }
        | Feature::ProfileBoss { points, z, extent, .. } => {
            (points, derive_z_cut(derive_contour_top(p.stock.z, *z), *extent))
        }
        _ => panic!("finish_profile_wall requires a ProfilePocket or ProfileBoss feature"),
    };
    // Phase 60 (slice 1 the pocket, slice 2 the boss): a loop that runs on or
    // past the blank has OPEN edges — beyond them is air — and the trace
    // follows its WALL stretches only, each chain entered and left along its
    // own line.
    if let Some((outer, kinds)) = crate::region::wall_edges(p, op) {
        if kinds.iter().any(|k| *k == crate::region::EdgeKind::Open) {
            return contour_chains(p, op, &outer, &kinds, z_cut);
        }
    }
    match op.comp {
        CompMode::Control => contour_control(p, op, points, z_cut),
        CompMode::Engine => contour_engine(p, op, points, z_cut).map(Into::into),
    }
}

/// The wall stretches of a loop with open edges, each a CHAIN of consecutive
/// wall edges in the loop's own direction, from the vertex after an open edge
/// to the vertex before the next.
fn wall_chains(outer: &[Pt], kinds: &[crate::region::EdgeKind]) -> Vec<Vec<Pt>> {
    use crate::region::EdgeKind;
    let n = outer.len();
    let Some(start) = (0..n).find(|&i| kinds[i] == EdgeKind::Wall && kinds[(i + n - 1) % n] == EdgeKind::Open) else { return Vec::new() };
    let mut chains: Vec<Vec<Pt>> = Vec::new();
    let mut chain: Vec<Pt> = Vec::new();
    for k in 0..n {
        let i = (start + k) % n;
        if kinds[i] == EdgeKind::Wall {
            if chain.is_empty() {
                chain.push(outer[i]);
            }
            chain.push(outer[(i + 1) % n]);
        } else if !chain.is_empty() {
            chains.push(std::mem::take(&mut chain));
        }
    }
    if !chain.is_empty() {
        chains.push(chain);
    }
    chains
}

/// A bulge path run the other way (ROADMAP item 2, slice 2). The bulge rides
/// the vertex a segment leaves (offset.rs), so a turned path moves each bulge
/// to the segment's other end and flips its sign — `.reverse()` alone would
/// bow every arc the wrong way, a silent gouge. A closed loop keeps its first
/// vertex, the start the lead was placed at; an open path starts at its far
/// end.
pub(crate) fn turned_verts(path: &[OffsetVertex], closed: bool) -> Vec<OffsetVertex> {
    let n = path.len();
    if n < 2 {
        return path.to_vec();
    }
    (0..n)
        .map(|i| {
            // The old index of the new vertex, and the bulge of the old
            // segment that arrived at it, now leaving it the other way.
            let j = if closed { (n - i) % n } else { n - 1 - i };
            let bulge = if closed {
                -path[(j + n - 1) % n].bulge
            } else if i + 1 < n {
                -path[j - 1].bulge
            } else {
                0.0
            };
            OffsetVertex { x: path[j].x, y: path[j].y, bulge }
        })
        .collect()
}

/// The path as the op runs it: as built, climbing; turned, conventional.
fn run_way(op: &Operation, path: Vec<OffsetVertex>, closed: bool) -> Vec<OffsetVertex> {
    if op.climbs() { path } else { turned_verts(&path, closed) }
}

/// An open path's segments, the loop emitter's walk without the closing one.
fn emit_offset_open(path: &[OffsetVertex], feed: f64) -> Vec<String> {
    let mut g = Vec::with_capacity(path.len());
    for w in path.windows(2) {
        let (from, to) = (w[0], w[1]);
        if from.bulge.abs() <= 1e-9 {
            g.push(format!("G01 X{} Y{}", f(to.x), f(to.y)));
        } else {
            let (acx, acy) = arc_center(&from, &to);
            g.push(arc(from.bulge < 0.0, to.x, to.y, acx - from.x, acy - from.y, feed));
        }
    }
    g
}

/// A wall traced along its WALL chains only (Phase 60). The void is LEFT of
/// travel for both windings — a CCW pocket holds it inside, a CW boss outside
/// (the winding law) — so the tool-centre path is each chain offset to the
/// LEFT by `r + skin` in the loop's own direction; both comp modes, the
/// control text the chain itself (offset by the skin) under G41/G40. Each
/// chain is entered along its own line one diameter before its start and left
/// one diameter past its end — past an open edge, which is past the blank —
/// the lead home asked for the room all the same.
fn contour_chains(p: &Part, op: &Operation, outer: &[Pt], kinds: &[crate::region::EdgeKind], z_cut: f64) -> Result<OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let d = derive_d(op.tool_num);
    let skin = op.stock_to_leave.unwrap_or(0.0);
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
    let control = op.comp == CompMode::Control;
    let chains = wall_chains(outer, kinds);
    if chains.is_empty() {
        return Err(format!(
            "Side on '{}' with T{}: every edge of its loop lies on or past the blank — there is no wall to trace",
            op.feature_name, op.tool_num
        ));
    }
    let room = room_for(p, op);
    let total = chains.len();
    let one = |loops: Vec<Vec<OffsetVertex>>, what: &str, k: usize| -> Result<Vec<OffsetVertex>, String> {
        if loops.len() != 1 || loops[0].len() < 2 {
            return Err(format!(
                "Side on '{}' with T{}: wall {} of {} offsets to {} path(s) for its {} — the chain may be too tight for this tool{}",
                op.feature_name, op.tool_num, k + 1, total, loops.len(), what, skin_knob(skin)
            ));
        }
        Ok(loops.into_iter().next().unwrap())
    };
    let mut g = Vec::new();
    let mut c = Vec::new();
    if control {
        g.push(format!("(--- side contour: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
        c.push(format!("(--- side contour (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
    } else {
        g.push(format!("(--- side contour (engine): {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    }
    g.push(direction_note(op));
    for (k, chain) in chains.iter().enumerate() {
        let center = one(crate::offset::offset_open_polyline_verts(chain, r + skin), "tool-centre path", k)?;
        let edge: Vec<OffsetVertex> = if skin > 0.0 {
            one(crate::offset::offset_open_polyline_verts(chain, skin), "allowance", k)?
        } else {
            chain.iter().map(|q| OffsetVertex { x: q.x, y: q.y, bulge: 0.0 }).collect()
        };
        let center = run_way(op, center, false);
        let edge = run_way(op, edge, false);
        let m = center.len();
        let (tx, ty) = segment_tangent(center[0], center[1], true);
        let lead = entry_lead(op, room.as_ref(), Pt { x: center[0].x, y: center[0].y }, (tx, ty), (-tx, -ty), tool.dia, z_cut, BACK, true)?;
        let (ex, ey) = segment_tangent(center[m - 2], center[m - 1], false);
        let out = lead_in(room.as_ref(), Pt { x: center[m - 1].x, y: center[m - 1].y }, (ex, ey), tool.dia, z_cut, "the path's own line, on past its end");
        let note = format!("(wall {} of {}: {} edge{}, open at both ends)", k + 1, total, chain.len() - 1, if chain.len() == 2 { "" } else { "s" });
        g.push(note.clone());
        c.push(note);
        g.push(lead.line.clone());
        if out.short {
            g.push(format!("(lead out: {}", out.line.split_once(": ").map_or("", |(_, rest)| rest)));
        }
        for ch in [&mut g, &mut c] {
            ch.push(format!("G00 Z{}", f(clearance_z)));
            ch.push(format!("G00 X{} Y{}", f(lead.at.x), f(lead.at.y)));
            ch.push(format!("G00 Z{}", f(rapid_z)));
            ch.push(format!("G01 Z{} F{}", f(z_cut + lead.rise), f(plunge)));
        }
        if control {
            g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(edge[0].x), f(edge[0].y), LeadShape::of(op, tool.dia).z_word(z_cut), f(feed)));
            g.extend(emit_offset_open(&edge, feed));
            g.push(format!("G01 G40 X{} Y{}", f(out.at.x), f(out.at.y)));
        } else {
            g.push(format!("G01 X{} Y{}{} F{}", f(center[0].x), f(center[0].y), LeadShape::of(op, tool.dia).z_word(z_cut), f(feed)));
            g.extend(emit_offset_open(&center, feed));
            g.push(format!("G01 X{} Y{}", f(out.at.x), f(out.at.y)));
        }
        c.push(format!("G01 X{} Y{} F{}", f(center[0].x), f(center[0].y), f(feed)));
        c.extend(emit_offset_open(&center, feed));
        c.push(format!("G01 X{} Y{}", f(out.at.x), f(out.at.y)));
        g.push(format!("G00 Z{}", f(clearance_z)));
        c.push(format!("G00 Z{}", f(clearance_z)));
    }
    Ok(OpMoves { text: g, resolved: control.then_some(c) })
}

// Control-side comp: centerline = part edge, offset register D from the tool.
//
// stock_to_leave here follows finish_cylinder's precedent exactly: the D
// register stays the PURE tool radius and the allowance is engine-baked into
// the emitted edge, so a double-comp is unrepresentable. Composition is exact —
// the controller's G41 rounds outside corners the same way `parallel_offset`
// does, so (edge ⊖ skin) ⊖ r == edge ⊖ (skin + r) wherever both are regular.
//
// The cost is that a polyline's "shrink" is not a scalar the way a circle's
// r_part - skin was: it needs the offsetter, which can round corners into
// bulges the straight-only emission could not represent, and can prune. Hence
// the shared bulge walk and the fallible signature. At skin == 0 the verbatim
// path is kept, so every existing golden is byte-identical.
fn contour_control(p: &Part, op: &Operation, points: &[Pt], z_cut: f64) -> Result<OpMoves, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;
    let d = derive_d(op.tool_num);
    let skin = op.stock_to_leave.unwrap_or(0.0);

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // A POSITIVE distance on the AUTHORED polyline is "left of travel", which is
    // G41's side and the material side under the winding convention — so no
    // to_ccw and, critically, no reversal of the result (bulges live on the
    // segment leaving a vertex, so reversing is not `.reverse()`; getting it
    // wrong is a silent gouge). Pinned by test_positive_offset_on_authored_
    // points_is_g41s_side below.
    let edge: Vec<OffsetVertex> = if skin > 0.0 {
        let loops = offset_closed_polyline_verts(points, skin);
        if loops.len() != 1 {
            return Err(format!(
                "contour feature '{}' offsets to {} loop(s) under a stock_to_leave of {:.3}mm; finishing needs exactly one (the profile may be too small or self-intersecting for this allowance) — reduce stock_to_leave or simplify the profile",
                op.feature_name, loops.len(), skin
            ));
        }
        if loops[0].len() < 3 {
            return Err(format!(
                "contour feature '{}' produced a degenerate offset under a stock_to_leave of {:.3}mm — reduce stock_to_leave",
                op.feature_name, skin
            ));
        }
        loops.into_iter().next().unwrap()
    } else {
        points.iter().map(|q| OffsetVertex { x: q.x, y: q.y, bulge: 0.0 }).collect()
    };

    // The resolved head's cut loop: the tool-center parallel at skin + r, one
    // offset call on the authored points by the blessed composition law
    // ((edge ⊖ skin) ⊖ r == edge ⊖ (skin + r) wherever both are regular) —
    // same positive-is-G41's-side convention as the edge. Machinability
    // parity (DECISIONS.md "the control-comp centerline resolve"): where this
    // parallel degenerates, the controller's comp degenerates with it — an
    // interference alarm mid-program or a gouge — so the error the engine
    // sibling always made now holds in both comp modes.
    let cl_loops = offset_closed_polyline_verts(points, skin + r);
    if cl_loops.len() != 1 {
        return Err(format!(
            "contour feature '{}' with comp=control has a tool-center parallel that offsets to {} loop(s) for tool T{}{}; the controller's comp would collapse or split the same way (the profile may be too small or self-intersecting for this tool) — reduce the tool or simplify the profile{}",
            op.feature_name, cl_loops.len(), op.tool_num,
            skin_note(skin), skin_knob(skin)
        ));
    }
    let room = room_for(p, op);
    let center_run = turned(op, room.as_ref(), run_way(op, cl_loops[0].clone(), true), signed_area(points) >= 0.0, tool.dia, z_cut)?;
    let center = &center_run;
    if center.len() < 3 {
        return Err(format!(
            "contour feature '{}' with comp=control produced a degenerate tool-center parallel for tool T{}{}",
            op.feature_name, op.tool_num, skin_note(skin)
        ));
    }

    let mut edge = run_way(op, edge, true);
    follow(&mut edge, Pt { x: center[0].x, y: center[0].y });
    let p0 = edge[0];
    let c0 = center[0];
    // The lead reads the room (the home at the foot of this file): off the
    // compensated start — the parallel's own first vertex, which is where the
    // resolved head says the tool is when comp is on — along the parallel's
    // void-side normal there. Not `r` off the authored vertex: at a pocket's
    // corner the parallel's vertex is a radius along BOTH edges, and a point a
    // radius along one stands half a tool in the other wall.
    let n = void_normal(center, signed_area(points) >= 0.0);
    let lead = entry_lead(op, room.as_ref(), Pt { x: c0.x, y: c0.y }, segment_tangent(center[center.len() - 1], center[0], false), n, tool.dia, z_cut, "the path's normal", false)?;
    let lead_pt = lead.at;

    let mut g = Vec::new();
    let mut c = Vec::new();
    g.push(format!("(--- side contour: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    c.push(format!("(--- side contour (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(direction_note(op));
    g.push(lead.line.clone());
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    g.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(z_cut + lead.rise), f(plunge)));
    c.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    c.push(format!("G00 Z{}", f(rapid_z)));
    c.push(format!("G01 Z{} F{}", f(z_cut + lead.rise), f(plunge)));
    // comp ON, ramped along the lead-in move into the first profile point —
    // the resolved head runs the same line to the compensated start, the
    // parallel's own first vertex:
    g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(p0.x), f(p0.y), lead.z_word(z_cut), f(feed)));
    c.push(format!("G01 X{} Y{}{} F{}", f(c0.x), f(c0.y), lead.z_word(z_cut), f(feed)));
    g.extend(emit_offset_loop(&edge, feed));
    c.extend(emit_offset_loop(center, feed));
    // comp OFF, ramped along the lead-out back to the safe lead point:
    g.push(format!("G01 G40 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    c.push(format!("G01 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));
    Ok(OpMoves { text: g, resolved: Some(c) })
}

// Engine-computed comp: the engine offsets the profile by the tool radius and
// emits plain coordinates with no cutter comp active.
fn contour_engine(p: &Part, op: &Operation, points: &[Pt], z_cut: f64) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Offset toward the same side control-mode's G41 (left of travel) would put
    // the tool: interior for CCW authoring, exterior for CW. The offset routine
    // treats a positive distance as "inward for a CCW polygon", so normalise the
    // profile to CCW and pick the sign from the authored winding.
    // The allowance rides with the tool radius: roughing the wall is finishing
    // the wall of a part fattened by the skin, which for a prismatic profile is
    // the same offset one skin further out (DECISIONS.md "stock_to_leave is a
    // radial allowance…"). Corner fillets grow r -> r + skin with it, which
    // needs no new handling — the emitter reads the bulge and never hardcodes a
    // radius. `+ skin` keeps skin == 0 byte-identical.
    let skin = op.stock_to_leave.unwrap_or(0.0);
    let area = signed_area(points);
    let ccw = to_ccw(points, area);
    let dist = if area >= 0.0 { r + skin } else { -(r + skin) };
    let loops = offset_closed_polyline_verts(&ccw, dist);

    if loops.len() != 1 {
        // A program that quietly drops or fudges the op the author asked for is
        // worse than no program (DECISIONS.md "unmachinable feature = hard error").
        return Err(format!(
            "contour feature '{}' with comp=engine offsets to {} loop(s) for tool T{}{}; engine finishing needs exactly one (the profile may be too small or self-intersecting for this tool) — reduce the tool, simplify the profile{}, or use comp=control",
            op.feature_name, loops.len(), op.tool_num,
            skin_note(skin), skin_knob(skin)
        ));
    }
    let room = room_for(p, op);
    let offset_run = turned(op, room.as_ref(), run_way(op, loops[0].clone(), true), area >= 0.0, tool.dia, z_cut)?;
    let offset = &offset_run;

    // A corner the offset rounds into a fillet comes back as a nonzero-bulge
    // segment; those are emitted as true G02/G03 arcs below, so nothing about a
    // rounded offset is rejected here anymore (DECISIONS.md "Arc emission…").
    if offset.len() < 3 {
        return Err(format!(
            "contour feature '{}' with comp=engine produced a degenerate offset for tool T{}{}",
            op.feature_name, op.tool_num, skin_note(skin)
        ));
    }

    // Lead in off the offset loop ON THE VOID SIDE — along the path's own
    // normal at its first vertex, as far as the stock model says there is room
    // (the home at the foot of this file) — so the plunge lands in cleared
    // air, not on the finished wall and not in the part, then feed to the
    // offset path. Until Phase 10 lane A this arm led toward the centroid for
    // BOTH windings: on a boss that is one diameter INTO the part, a plunge
    // the stock model rejected the moment the boss finish had a removal to be
    // read against. Until Phase 47 it led along the centroid's ray for one
    // diameter whatever stood there: on an L-shaped rib across a slot, into
    // the neighbouring rib (DECISIONS.md "DRAFT: Phase 47 opens…", premise 4).
    let o0 = offset[0];
    let n = void_normal(offset, area >= 0.0);
    let lead = entry_lead(op, room.as_ref(), Pt { x: o0.x, y: o0.y }, segment_tangent(offset[offset.len() - 1], offset[0], false), n, tool.dia, z_cut, "the path's normal", false)?;
    let lead_pt = lead.at;

    let mut g = Vec::new();
    g.push(format!("(--- side contour (engine): {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.push(direction_note(op));
    g.push(lead.line.clone());
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    g.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(z_cut + lead.rise), f(plunge)));
    g.push(format!("G01 X{} Y{}{} F{}", f(o0.x), f(o0.y), lead.z_word(z_cut), f(feed)));
    // Walk the closed loop segment by segment. The bulge lives on the vertex a
    // segment leaves (offset.rs), so segment offset[i] -> offset[(i+1)%n] carries
    // offset[i].bulge; iterating 0..n covers the closing segment back to o0 too.
    // A straight segment stays a modal G01 (no repeated F, so the arc-free case
    // is byte-identical to the old straight-only emission); a rounded one becomes
    // an exact G02/G03 with the center as incremental IJ from the segment start.
    g.extend(emit_offset_loop(offset, feed));
    g.push(format!("G01 X{} Y{}", f(lead_pt.x), f(lead_pt.y))); // lead out
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

// ---- The edge round on a polyline edge (DECISIONS.md "the corner rounder gets
// an operation", slice 2) ----------------------------------------------------
// `ops/finish.rs`'s edge-round arm, with the plan curve swapped: the circular
// pair's axis RING becomes an axis PATH, and every other line of the derivation
// is untouched, because the round is `radius` into the material and `radius`
// below the top face whatever shape the wall's plan curve is.
//
// One pass, at one Z, no stepover and no ladder — for the same reason as the
// circular pair: the form is stamped by the cutting edge, not approximated.
// What is genuinely new is that the round is no longer CONSTANT along the edge:
// where a plan corner is tighter than the tool, the path pulls away and the
// round is not cut there. That is not modelled away — it is reported by
// `parse::deviation_findings` and honoured by `stock.rs`'s removal, which
// measures from this path rather than from the wall.
pub fn finish_profile_edge_round(
    p: &Part,
    op: &Operation,
    er: &crate::parse::EdgeRound,
    arc: &crate::parse::GroundArc,
    z_corner: f64,
) -> Result<OpMoves, String> {
    let crate::parse::EdgeLoop::Polyline { points } = &er.edge else {
        panic!("finish_profile_edge_round requires a polyline edge");
    };
    let reach = er.axis_offset(arc.effective_radius(), op.wall_standoff);
    // The same offset both `validate()` and the removal model read, from the one
    // home that owns it — so the wall this program cuts and the wall the model
    // reports cannot be two different curves.
    let path = crate::parse::derive_edge_round_path(points, reach).map_err(|why| format!(
        "edge round on '{}' with T{}: the axis path {}",
        op.feature_name, op.tool_num, why
    ))?;
    let path = run_way(op, path, true);

    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Back off INTO the void, which side the winding names — the wall trace's
    // own rule (see contour_control), and here it is load-bearing rather than
    // tidy: the tip hangs a whole gauge_z below the emitted datum, so a lead
    // point on the material side would plunge the arc into standing stock.
    //
    // Which curve it is measured FROM is the standoff law again. Under engine
    // comp the standoff is in the coordinates, so the lead rides off the path
    // with them; under control comp it is in D and nowhere else, so the lead
    // rides off the AUTHORED edge and lands the same distance clear of the wall
    // by the tool's nominal reach. Otherwise a standoff would move a second
    // number in the emitted text, and one offset having two homes is the whole
    // failure this design exists to kill.
    // One lead for both arms, off an axis path's first vertex along its
    // void-side normal: the path itself under engine comp; under control the
    // same path WITHOUT the standoff — the lead is text, the standoff is D —
    // which is the physical spot engine comp leads from when none is asked.
    // Under the lead keys the anchor is turned to where the lead lies in air
    // and the other loop follows it (slice 5).
    let room = room_for(p, op);
    let void_inside = signed_area(points) >= 0.0;
    let mut path = path;
    let bare: Option<Vec<OffsetVertex>> = match op.comp {
        CompMode::Engine => {
            path = turned(op, room.as_ref(), path, void_inside, tool.dia, z_corner)?;
            None
        }
        CompMode::Control => {
            let b = turned(op, room.as_ref(), run_way(op, crate::parse::derive_edge_round_path(points, arc.effective_radius()).map_err(|why| format!(
                "edge round on '{}' with T{}: the axis path {}",
                op.feature_name, op.tool_num, why
            ))?, true), void_inside, tool.dia, z_corner)?;
            follow(&mut path, Pt { x: b[0].x, y: b[0].y });
            Some(b)
        }
    };
    let a0 = path[0];
    let anchor: &[OffsetVertex] = bare.as_deref().unwrap_or(&path);
    let n = void_normal(anchor, void_inside);
    let lead = entry_lead(op, room.as_ref(), Pt { x: anchor[0].x, y: anchor[0].y }, segment_tangent(anchor[anchor.len() - 1], anchor[0], false), n, tool.dia, z_corner, "the path's normal", false)?;
    let at = lead.at;

    let mut g = Vec::new();
    match op.comp {
        // Engine comp: the axis path as plain coordinates, arcs and all — a
        // corner the offset rounds is a real arc the tool really rides.
        CompMode::Engine => {
            g.push(format!("(--- break edge (engine): {} with T{} ---)", op.feature_name, op.tool_num));
            g.push(direction_note(op));
            g.push(lead.line.clone());
            g.push(format!("G00 Z{}", f(clearance_z)));
            g.push(format!("G00 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(rapid_z)));
            g.push(format!("G01 Z{} F{}", f(z_corner + lead.rise), f(plunge)));
            g.push(format!("G01 X{} Y{}{} F{}", f(a0.x), f(a0.y), lead.z_word(z_corner), f(feed)));
            g.extend(emit_offset_loop(&path, feed));
            g.push(format!("G01 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(clearance_z)));
            Ok(g.into())
        }
        // Control comp: the emitted coordinates are the model's own edge — which
        // for this op is what driving the virtual sharp corner along the edge
        // MEANS, so the datum and the comp mode are the same object (slice 1's
        // finding, and it carries over verbatim). The standoff folds into D and
        // never into the path, so the emitted loop is the authored polyline
        // itself and exactly one number reaches the machine.
        CompMode::Control => {
            let d = derive_d(op.tool_num);
            let edge: Vec<OffsetVertex> =
                run_way(op, points.iter().map(|q| OffsetVertex { x: q.x, y: q.y, bulge: 0.0 }).collect(), true);
            let p0 = edge[0];
            let mut c = Vec::new();
            g.push(format!("(--- break edge (control): {} with T{} ---)", op.feature_name, op.tool_num));
            c.push(format!("(--- break edge (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
            g.push(direction_note(op));
            g.push(lead.line.clone());
            for (h, at) in [(&mut g, at), (&mut c, at)] {
                h.push(format!("G00 Z{}", f(clearance_z)));
                h.push(format!("G00 X{} Y{}", f(at.x), f(at.y)));
                h.push(format!("G00 Z{}", f(rapid_z)));
                h.push(format!("G01 Z{} F{}", f(z_corner + lead.rise), f(plunge)));
            }
            // Left of travel is the void side under the winding convention, for
            // both variants — the same register and the same G41 the wall trace
            // and the circular lips already carry.
            g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(p0.x), f(p0.y), lead.z_word(z_corner), f(feed)));
            c.push(format!("G01 X{} Y{}{} F{}", f(a0.x), f(a0.y), lead.z_word(z_corner), f(feed)));
            g.extend(emit_offset_loop(&edge, feed));
            c.extend(emit_offset_loop(&path, feed));
            g.push(format!("G01 G40 X{} Y{}", f(at.x), f(at.y)));
            c.push(format!("G01 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(clearance_z)));
            c.push(format!("G00 Z{}", f(clearance_z)));
            Ok(OpMoves { text: g, resolved: Some(c) })
        }
    }
}

// ---- The chamfer on a polyline edge (DECISIONS.md "the chamfer tool…") ------
// `finish_profile_edge_round` with the form swapped: the axis path is the same
// offset loop machinery at the placement's resolved reach, the emitted Z is
// the placement's tip Z, and the corner-shortfall story transfers as-is —
// arguably worse, because an unbroken corner on a beveled edge is more
// visible. One pass, one Z, the face stamped by the flank.
pub fn finish_profile_chamfer(
    p: &Part,
    op: &Operation,
    ec: &crate::parse::EdgeChamfer,
    pl: &crate::parse::ChamferPlacement,
    z_tip_for: &dyn Fn(crate::parse::ChamferLeg) -> f64,
    slide: f64,
) -> Result<OpMoves, String> {
    let crate::parse::EdgeLoop::Polyline { points } = &ec.edge else {
        panic!("finish_profile_chamfer requires a polyline edge");
    };
    // The same offset both `validate()` and the removal model read, from the
    // one home that owns it.
    let mut path = crate::parse::derive_edge_round_path(points, pl.reach).map_err(|why| format!(
        "chamfer on '{}' with T{}: the axis path {}",
        op.feature_name, op.tool_num, why
    ))?;
    // A loop whose points restate the leg (slice 9) is cut in runs, one
    // depth each, starting where the first run starts: the path is turned
    // so that vertex leads and the lead-in below is computed there.
    let mut runs = crate::parse::derive_chamfer_runs(ec, &path);
    if let Some(first) = runs.first() {
        path.rotate_left(first.start);
        runs = crate::parse::derive_chamfer_runs(ec, &path);
    }
    if !runs.is_empty() {
        // A stepped pass (`slide` up the face) rides in over the face, and
        // its path turns the loop's corners that much early. Where a run
        // ends against an unbroken edge the bevel runs out square through
        // that edge's wall, so the run is carried on by the slide at that
        // end, out to where the unslid pass ends.
        if slide > 0.0 && runs.len() > 1 {
            let n = runs.len();
            let cut: Vec<bool> = runs.iter().map(|r| r.leg.is_some()).collect();
            for i in 0..n {
                if !cut[i] || runs[i].path.len() < 2 {
                    continue;
                }
                if !cut[(i + n - 1) % n] {
                    let (a, b) = (runs[i].path[0], runs[i].path[1]);
                    let t = segment_tangent(a, b, true);
                    runs[i].path.insert(0, OffsetVertex { x: a.x - t.0 * slide, y: a.y - t.1 * slide, bulge: 0.0 });
                }
                if !cut[(i + 1) % n] {
                    let m = runs[i].path.len();
                    let (a, b) = (runs[i].path[m - 2], runs[i].path[m - 1]);
                    let t = segment_tangent(a, b, false);
                    runs[i].path[m - 1].bulge = 0.0;
                    runs[i].path.push(OffsetVertex { x: b.x + t.0 * slide, y: b.y + t.1 * slide, bulge: 0.0 });
                }
            }
        }
        return finish_profile_chamfer_runs(p, op, ec, pl, &path, &runs, z_tip_for);
    }
    let path = run_way(op, path, true);

    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Back off INTO the void, which side the winding names — and load-bearing
    // for the same reason as the round: the tip rides below the face's bottom
    // corner, so a lead point on the material side would plunge the flank into
    // standing stock. Under engine comp the lead rides off the path (the
    // offsets are in the coordinates); under control comp off the AUTHORED
    // edge by the nominal reach, so a standoff moves nothing but D.
    // One lead for both arms, off an axis path's first vertex (the round's
    // note above): the path under engine comp, the standoff-free path under
    // control; under the lead keys the anchor turned and the other following.
    let room = room_for(p, op);
    let void_inside = signed_area(points) >= 0.0;
    let mut path = path;
    let bare: Option<Vec<OffsetVertex>> = match op.comp {
        CompMode::Engine => {
            path = turned(op, room.as_ref(), path, void_inside, tool.dia, pl.z_tip)?;
            None
        }
        CompMode::Control => {
            let b = turned(op, room.as_ref(), run_way(op, crate::parse::derive_edge_round_path(points, pl.r_contact).map_err(|why| format!(
                "chamfer on '{}' with T{}: the axis path {}",
                op.feature_name, op.tool_num, why
            ))?, true), void_inside, tool.dia, pl.z_tip)?;
            follow(&mut path, Pt { x: b[0].x, y: b[0].y });
            Some(b)
        }
    };
    let a0 = path[0];
    let anchor: &[OffsetVertex] = bare.as_deref().unwrap_or(&path);
    let n = void_normal(anchor, void_inside);
    let lead = entry_lead(op, room.as_ref(), Pt { x: anchor[0].x, y: anchor[0].y }, segment_tangent(anchor[anchor.len() - 1], anchor[0], false), n, tool.dia, pl.z_tip, "the path's normal", false)?;
    let at = lead.at;

    let mut g = Vec::new();
    match op.comp {
        CompMode::Engine => {
            g.push(format!("(--- chamfer edge (engine): {} with T{} ---)", op.feature_name, op.tool_num));
            g.push(direction_note(op));
            g.push(lead.line.clone());
            g.push(format!("G00 Z{}", f(clearance_z)));
            g.push(format!("G00 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(rapid_z)));
            g.push(format!("G01 Z{} F{}", f(pl.z_tip + lead.rise), f(plunge)));
            g.push(format!("G01 X{} Y{}{} F{}", f(a0.x), f(a0.y), lead.z_word(pl.z_tip), f(feed)));
            g.extend(emit_offset_loop(&path, feed));
            g.push(format!("G01 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(clearance_z)));
            Ok(g.into())
        }
        CompMode::Control => {
            let d = derive_d(op.tool_num);
            let edge: Vec<OffsetVertex> =
                run_way(op, points.iter().map(|q| OffsetVertex { x: q.x, y: q.y, bulge: 0.0 }).collect(), true);
            let p0 = edge[0];
            let mut c = Vec::new();
            g.push(format!("(--- chamfer edge (control): {} with T{} ---)", op.feature_name, op.tool_num));
            c.push(format!("(--- chamfer edge (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
            g.push(direction_note(op));
            g.push(lead.line.clone());
            for (h, at) in [(&mut g, at), (&mut c, at)] {
                h.push(format!("G00 Z{}", f(clearance_z)));
                h.push(format!("G00 X{} Y{}", f(at.x), f(at.y)));
                h.push(format!("G00 Z{}", f(rapid_z)));
                h.push(format!("G01 Z{} F{}", f(pl.z_tip + lead.rise), f(plunge)));
            }
            g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(p0.x), f(p0.y), lead.z_word(pl.z_tip), f(feed)));
            c.push(format!("G01 X{} Y{}{} F{}", f(a0.x), f(a0.y), lead.z_word(pl.z_tip), f(feed)));
            g.extend(emit_offset_loop(&edge, feed));
            c.extend(emit_offset_loop(&path, feed));
            g.push(format!("G01 G40 X{} Y{}", f(at.x), f(at.y)));
            c.push(format!("G01 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(clearance_z)));
            c.push(format!("G00 Z{}", f(clearance_z)));
            Ok(OpMoves { text: g, resolved: Some(c) })
        }
    }
}

/// The chamfer pass on a loop whose points restate the leg (ROADMAP item 1
/// slice 9): one lead-in, then the runs in order round the loop, the tip
/// moving to each run's own Z at the vertex where the leg changes. A run left
/// unbroken is passed over at `rapid_z`, the tool coming back down at the
/// next run's start. Under control comp the compensated pass is one loop and
/// the Z moves happen with comp on; an unbroken run under control comp is
/// refused by `validate`. Where a deeper run meets a shallower one the deeper
/// flank reaches about a reach's width round the corner, which the comment
/// says.
fn finish_profile_chamfer_runs(
    p: &Part,
    op: &Operation,
    ec: &crate::parse::EdgeChamfer,
    pl: &crate::parse::ChamferPlacement,
    path: &[OffsetVertex],
    runs: &[crate::parse::ChamferRun],
    z_tip_for: &dyn Fn(crate::parse::ChamferLeg) -> f64,
) -> Result<OpMoves, String> {
    let crate::parse::EdgeLoop::Polyline { points } = &ec.edge else {
        panic!("finish_profile_chamfer_runs requires a polyline edge");
    };
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // `direction=conventional` (ROADMAP item 2, slice 2): the runs in the
    // other order, each the other way, every leg staying with its stretch
    // (the legs were read off the path as authored); the path the lead
    // reads turned with them. The last run ends where the first began, so
    // the first vertex is the same point either way.
    let (turned_runs, turned_path);
    let (runs, path): (&[crate::parse::ChamferRun], &[OffsetVertex]) = if op.climbs() {
        (runs, path)
    } else {
        turned_runs = runs
            .iter()
            .rev()
            .map(|r| crate::parse::ChamferRun { leg: r.leg, start: r.start, path: turned_verts(&r.path, false), edge: r.edge.iter().rev().copied().collect() })
            .collect::<Vec<_>>();
        turned_path = turned_verts(path, true);
        (&turned_runs, &turned_path)
    };

    // The lead, off the first run's first vertex — the path was turned so
    // that vertex is the path's first (see the caller).
    let bare;
    let anchor: &[OffsetVertex] = match op.comp {
        CompMode::Engine => path,
        CompMode::Control => {
            let mut b = crate::parse::derive_edge_round_path(points, pl.r_contact).map_err(|why| format!(
                "chamfer on '{}' with T{}: the axis path {}",
                op.feature_name, op.tool_num, why
            ))?;
            let q = runs[0].edge[0];
            // Tied vertices: the first from the path's start.
            let k = crate::tie::first_least(b.iter().enumerate().map(|(k, v)| (k, (v.x - q.x).phypot(v.y - q.y))), |c| c.1).map_or(0, |(k, _)| k);
            b.rotate_left(k);
            bare = run_way(op, b, true);
            &bare
        }
    };
    let n = void_normal(anchor, signed_area(points) >= 0.0);
    if LeadShape::of(op, tool.dia).descent_deg > 0.0 {
        return Err(format!("lead_descent on '{}': a chamfer whose legs are restated comes down to each run's own depth at the run's start, and a sloping lead onto the first is not built", op.feature_name));
    }
    let lead = entry_lead(op, room_for(p, op).as_ref(), Pt { x: anchor[0].x, y: anchor[0].y }, segment_tangent(anchor[anchor.len() - 1], anchor[0], false), n, tool.dia, pl.z_tip, "the path's normal", false)?;
    let at = lead.at;
    let legs: Vec<String> = runs.iter().map(|r| match r.leg {
        Some(crate::parse::ChamferLeg::Depth(d)) => format!("{} deep", f(d)),
        Some(crate::parse::ChamferLeg::Width(w)) => format!("{} wide", f(w)),
        None => "unbroken".to_string(),
    }).collect();
    let about = format!(
        "legs restated on the loop: {} runs ({}); a deeper run reaches about {} round the corner into a shallower one",
        runs.len(), legs.join(", "), f(pl.reach)
    );

    // One chain of moves along a run's path (or its authored edge), from its
    // first vertex, which the tool already stands on.
    let along = |chain: &[OffsetVertex], h: &mut Vec<String>| {
        for i in 0..chain.len().saturating_sub(1) {
            let (from, to) = (chain[i], chain[i + 1]);
            if from.bulge.abs() <= 1e-9 {
                // The first move carries the feed: the tool arrives at a
                // run's start on a plunge move or a rapid.
                if i == 0 {
                    h.push(format!("G01 X{} Y{} F{}", f(to.x), f(to.y), f(feed)));
                } else {
                    h.push(format!("G01 X{} Y{}", f(to.x), f(to.y)));
                }
            } else {
                let (acx, acy) = arc_center(&from, &to);
                h.push(arc(from.bulge < 0.0, to.x, to.y, acx - from.x, acy - from.y, feed));
            }
        }
    };
    // The runs, emitted over `chains` (the path's runs for the axis
    // program, the authored edges' for control comp's compensated one).
    let emit_runs = |chains: &[Vec<OffsetVertex>], h: &mut Vec<String>, comp_on: Option<String>| {
        let mut up = true; // the tool is at rapid_z before the first run
        let mut first = true;
        let mut descended = false;
        for (r, chain) in runs.iter().zip(chains) {
            match r.leg {
                Some(leg) => {
                    let z = z_tip_for(leg);
                    // The first descent is the lead's, said above; a run after
                    // an unbroken one descends where the tool stands.
                    if descended {
                        h.push("(plunge: at the run's start, on the path — the leg is cut on the way down)".to_string());
                    }
                    descended = true;
                    h.push(format!("G01 Z{} F{}", f(z), f(plunge)));
                    if first {
                        // Onto the path from the lead point, comp coming on
                        // here under control.
                        if let Some(on) = &comp_on {
                            h.push(format!("{on} X{} Y{} F{}", f(chain[0].x), f(chain[0].y), f(feed)));
                        } else {
                            h.push(format!("G01 X{} Y{} F{}", f(chain[0].x), f(chain[0].y), f(feed)));
                        }
                    }
                    along(chain, h);
                    up = false;
                }
                None => {
                    if !up {
                        h.push(format!("G00 Z{}", f(rapid_z)));
                    }
                    if let Some(last) = chain.last() {
                        h.push(format!("G00 X{} Y{}", f(last.x), f(last.y)));
                    }
                    up = true;
                }
            }
            first = false;
        }
        up
    };

    let mut g = Vec::new();
    match op.comp {
        CompMode::Engine => {
            g.push(format!("(--- chamfer edge (engine): {} with T{}; {} ---)", op.feature_name, op.tool_num, about));
            g.push(direction_note(op));
            g.push(lead.line.clone());
            g.push(format!("G00 Z{}", f(clearance_z)));
            g.push(format!("G00 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(rapid_z)));
            let chains: Vec<Vec<OffsetVertex>> = runs.iter().map(|r| r.path.clone()).collect();
            let up = emit_runs(&chains, &mut g, None);
            if !up {
                g.push(format!("G01 X{} Y{}", f(at.x), f(at.y)));
            }
            g.push(format!("G00 Z{}", f(clearance_z)));
            Ok(g.into())
        }
        CompMode::Control => {
            let d = derive_d(op.tool_num);
            let edges: Vec<Vec<OffsetVertex>> = runs.iter()
                .map(|r| r.edge.iter().map(|q| OffsetVertex { x: q.x, y: q.y, bulge: 0.0 }).collect())
                .collect();
            let chains: Vec<Vec<OffsetVertex>> = runs.iter().map(|r| r.path.clone()).collect();
            let mut c = Vec::new();
            g.push(format!("(--- chamfer edge (control): {} with T{}; {} ---)", op.feature_name, op.tool_num, about));
            c.push(format!("(--- chamfer edge (control, resolved centerline): {} with T{}; {} ---)", op.feature_name, op.tool_num, about));
            g.push(direction_note(op));
            g.push(lead.line.clone());
            for h in [&mut g, &mut c] {
                h.push(format!("G00 Z{}", f(clearance_z)));
                h.push(format!("G00 X{} Y{}", f(at.x), f(at.y)));
                h.push(format!("G00 Z{}", f(rapid_z)));
            }
            emit_runs(&edges, &mut g, Some(format!("G01 {} D{d}", comp_word(op))));
            emit_runs(&chains, &mut c, None);
            g.push(format!("G01 G40 X{} Y{}", f(at.x), f(at.y)));
            c.push(format!("G01 X{} Y{}", f(at.x), f(at.y)));
            g.push(format!("G00 Z{}", f(clearance_z)));
            c.push(format!("G00 Z{}", f(clearance_z)));
            Ok(OpMoves { text: g, resolved: Some(c) })
        }
    }
}

// ---- The 3D-wire pair (DECISIONS.md "the 3D-wire Contour…") ---------------
// The same wall trace as the planar profiles, along a top edge that rises and
// falls in Z: one full-height pass riding the wire displaced down by `extent`
// (at extent == 0, the wire itself — the reserved semantic), XYZ-interpolated
// between vertices. Descending segments are ramp cuts with the tip engaged;
// every on-wire move runs at `feed` — plunge is entry-only, as everywhere.
//
// Side-of-material reads exactly as the planar worker reads it: the wall is a
// vertical sweep, so the XY projection carries the whole plan problem, and the
// variant/winding contract was validated at parse. What is new is that Z must
// SURVIVE THE OFFSET: the offsetter is strictly 2D, so engine comp offsets
// the projection and re-attaches each result vertex's Z from the nearest
// authored segment (linear in the segment parameter — exact for a vertical
// sweep). An inserted corner fillet sits nearest the two segments that meet
// at its corner, whose shared vertex has one Z, so fillet arcs come back
// constant-Z by construction — guarded below rather than assumed, because a
// wire tight enough for the offset to re-attach across the loop is a real
// authoring possibility and a silent helical arc would cut a wall that was
// never declared.
pub fn finish_contour_wire(p: &Part, op: &Operation) -> Result<OpMoves, String> {
    let feature = p.features.get(&op.feature_name).expect("feature must exist");
    let (points, extent) = match feature {
        Feature::ContourPocket { points, extent }
        | Feature::ContourBoss { points, extent } => (points, *extent),
        _ => panic!("finish_contour_wire requires a ContourPocket or ContourBoss feature"),
    };
    match op.comp {
        CompMode::Control => wire_control(p, op, points, extent),
        CompMode::Engine => wire_engine(p, op, points, extent).map(Into::into),
    }
}

// Control-side comp: centerline = part edge with a Z word per vertex, G41 +
// the pure-radius D register, the controller rounding corners at whatever Z
// the adjacent moves put it — one point, one Z, so its arc is constant-Z
// like engine mode's.
//
// stock_to_leave follows the planar wall's precedent exactly (DECISIONS.md
// "the 3D-wire Contour…", slice 2): the D register stays the PURE tool
// radius and the allowance is engine-baked into the emitted edge, so a
// double-comp is unrepresentable. What is new against the planar worker is
// that the baked edge is an OFFSET result, so its vertices ride the same Z
// re-attachment engine comp uses — including the constant-Z arc guard,
// because a boss's skin offset rounds outward corners into fillets exactly
// as the tool offset does. At skin == 0 the authored wire is emitted
// verbatim, so the slice-1 goldens hold byte-identical.
fn wire_control(p: &Part, op: &Operation, points: &[Pt3], extent: f64) -> Result<OpMoves, String> {
    let dia = p.tools.get(&op.tool_num).expect("tool must exist").dia;
    let d = derive_d(op.tool_num);
    let skin = op.stock_to_leave.unwrap_or(0.0);

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // A POSITIVE distance on the AUTHORED projection is "left of travel" —
    // G41's side under the winding convention — so no to_ccw and no reversal,
    // same as the planar control worker (and the same silent-gouge reason to
    // never reverse an offset result).
    let edge: Vec<OffsetVertex> = if skin > 0.0 {
        let proj: Vec<Pt> = points.iter().map(Pt3::xy).collect();
        let loops = offset_closed_polyline_verts(&proj, skin);
        if loops.len() != 1 {
            return Err(format!(
                "contour wire '{}' offsets to {} loop(s) under a stock_to_leave of {:.3}mm; finishing needs exactly one (the wire's plan view may be too small or self-intersecting for this allowance) — reduce stock_to_leave or simplify the wire",
                op.feature_name, loops.len(), skin
            ));
        }
        if loops[0].len() < 3 {
            return Err(format!(
                "contour wire '{}' produced a degenerate offset under a stock_to_leave of {:.3}mm — reduce stock_to_leave",
                op.feature_name, skin
            ));
        }
        loops.into_iter().next().unwrap()
    } else {
        points.iter().map(|q| OffsetVertex { x: q.x, y: q.y, bulge: 0.0 }).collect()
    };

    let edge = run_way(op, edge, true);
    let zs = wire_offset_zs(&edge, points, extent).map_err(|(za, zb)| format!(
        "contour wire '{}' rounds a corner whose ends re-attach at different heights ({:.3} vs {:.3}) under a stock_to_leave of {:.3}mm — the plan view is too tight for this allowance; reduce stock_to_leave or open the wire's corners",
        op.feature_name, za, zb, skin
    ))?;

    // The resolved head's cut loop: the tool-center parallel at skin + r on
    // the same authored projection, its Z re-attached by the same rule the
    // machine follows (Z is linear along programmed blocks; the perpendicular
    // foot of a parallel vertex reads the same authored parameter at every
    // offset distance, so the constant-Z arc guard holds or fails for both
    // heads together). Machinability parity (DECISIONS.md "the control-comp
    // centerline resolve"): where this parallel degenerates or re-attaches
    // across the loop, the controller's comp is in the same trouble — the
    // engine sibling's errors now hold in both comp modes.
    let r = p.tools.get(&op.tool_num).expect("tool must exist").dia / 2.0;
    let proj: Vec<Pt> = points.iter().map(Pt3::xy).collect();
    let cl_loops = offset_closed_polyline_verts(&proj, skin + r);
    if cl_loops.len() != 1 {
        return Err(format!(
            "contour wire '{}' with comp=control has a tool-center parallel that offsets to {} loop(s) for tool T{}{}; the controller's comp would collapse or split the same way (the wire's plan view may be too small or self-intersecting for this tool) — reduce the tool or simplify the wire{}",
            op.feature_name, cl_loops.len(), op.tool_num,
            skin_note(skin), skin_knob(skin)
        ));
    }
    let room = room_for(p, op);
    let deepest = points.iter().map(|q| q.z).fold(f64::INFINITY, f64::min);
    let center_run = turned(op, room.as_ref(), run_way(op, cl_loops[0].clone(), true), signed_area(&proj) >= 0.0, dia, deepest)?;
    let center = &center_run;
    if center.len() < 3 {
        return Err(format!(
            "contour wire '{}' with comp=control produced a degenerate tool-center parallel for tool T{}{}",
            op.feature_name, op.tool_num, skin_note(skin)
        ));
    }
    let zs_c = wire_offset_zs(center, points, extent).map_err(|(za, zb)| format!(
        "contour wire '{}' with comp=control rounds a corner whose ends re-attach at different heights ({:.3} vs {:.3}) at the tool-center parallel — the plan view is too tight for tool T{}{}; reduce the tool{}, or open the wire's corners",
        op.feature_name, za, zb, op.tool_num, skin_note(skin), skin_knob(skin)
    ))?;

    // The authored edge and its heights follow the parallel's start.
    let (mut edge, mut zs) = (edge, zs);
    let k = follow(&mut edge, Pt { x: center[0].x, y: center[0].y });
    zs.rotate_left(k);
    let p0 = edge[0];
    let c0 = center[0];
    // Into the void off the parallel's first vertex along its normal, as far
    // as there is room — the planar worker's rule (see contour_control).
    let n = void_normal(center, signed_area(&proj) >= 0.0);
    let lead = entry_lead(op, room.as_ref(), Pt { x: c0.x, y: c0.y }, segment_tangent(center[center.len() - 1], center[0], false), n, dia, zs_c[0], "the path's normal", false)?;
    let lead_pt = lead.at;

    let mut g = Vec::new();
    let mut c = Vec::new();
    g.push(format!("(--- side contour wire: {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    c.push(format!("(--- side contour wire (control, resolved centerline): {} with T{} ---)", op.feature_name, op.tool_num));
    g.push(direction_note(op));
    g.push(lead.line.clone());
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    g.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(zs[0] + lead.rise), f(plunge)));
    c.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    c.push(format!("G00 Z{}", f(rapid_z)));
    c.push(format!("G01 Z{} F{}", f(zs_c[0] + lead.rise), f(plunge)));
    // comp ON, ramped along the lead-in move into the first edge point — the
    // resolved head runs the same line to the compensated start, the
    // parallel's own first vertex:
    g.push(format!("G01 {} D{} X{} Y{}{} F{}", comp_word(op), d, f(p0.x), f(p0.y), lead.z_word(zs[0]), f(feed)));
    c.push(format!("G01 X{} Y{}{} F{}", f(c0.x), f(c0.y), lead.z_word(zs_c[0]), f(feed)));
    g.extend(emit_wire_loop(&edge, &zs, feed));
    c.extend(emit_wire_loop(center, &zs_c, feed));
    // comp OFF, ramped along the lead-out back to the safe lead point:
    g.push(format!("G01 G40 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    c.push(format!("G01 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    g.push(format!("G00 Z{}", f(clearance_z)));
    c.push(format!("G00 Z{}", f(clearance_z)));
    Ok(OpMoves { text: g, resolved: Some(c) })
}

// Engine-computed comp: offset the XY projection by the tool radius (same
// winding normalization and sign dance as the planar worker), then re-attach
// Z to every offset vertex before emitting.
fn wire_engine(p: &Part, op: &Operation, points: &[Pt3], extent: f64) -> Result<Vec<String>, String> {
    let tool = p.tools.get(&op.tool_num).expect("tool must exist");
    let r = tool.dia / 2.0;

    let feed = op.feed.unwrap_or(p.global_feed);
    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // The allowance rides with the tool radius, the planar engine worker's
    // rule verbatim: roughing the wall is finishing the wall of a part
    // fattened by the skin, which for a vertical sweep is the same offset one
    // skin further out. Corner fillets grow r -> r + skin with it — no new
    // handling, the emitter reads the bulge. `+ skin` keeps skin == 0
    // byte-identical.
    let skin = op.stock_to_leave.unwrap_or(0.0);
    let proj: Vec<Pt> = points.iter().map(Pt3::xy).collect();
    let area = signed_area(&proj);
    let ccw = to_ccw(&proj, area);
    let dist = if area >= 0.0 { r + skin } else { -(r + skin) };
    let loops = offset_closed_polyline_verts(&ccw, dist);

    if loops.len() != 1 {
        return Err(format!(
            "contour wire '{}' with comp=engine offsets to {} loop(s) for tool T{}{}; engine finishing needs exactly one (the wire's plan view may be too small or self-intersecting for this tool) — reduce the tool, simplify the wire{}, or use comp=control",
            op.feature_name, loops.len(), op.tool_num,
            skin_note(skin), skin_knob(skin)
        ));
    }
    let room = room_for(p, op);
    let deepest = points.iter().map(|q| q.z).fold(f64::INFINITY, f64::min);
    let offset_run = turned(op, room.as_ref(), run_way(op, loops[0].clone(), true), signed_area(&proj) >= 0.0, tool.dia, deepest)?;
    let offset = &offset_run;
    if offset.len() < 3 {
        return Err(format!(
            "contour wire '{}' with comp=engine produced a degenerate offset for tool T{}{}",
            op.feature_name, op.tool_num, skin_note(skin)
        ));
    }

    let zs = wire_offset_zs(offset, points, extent).map_err(|(za, zb)| format!(
        "contour wire '{}' with comp=engine rounds a corner whose ends re-attach at different heights ({:.3} vs {:.3}) — the plan view is too tight for tool T{}{}; reduce the tool{}, or open the wire's corners",
        op.feature_name, za, zb, op.tool_num, skin_note(skin), skin_knob(skin)
    ))?;

    // Lead in off the offset loop ON THE VOID SIDE, along the path's normal
    // at its first vertex and as far as there is room (Phase 10 lane A, slice
    // 2 learned the sides; Phase 47 slice 2 the direction and the length —
    // `contour_engine`'s note).
    let o0 = offset[0];
    let n = void_normal(offset, signed_area(&proj) >= 0.0);
    let lead = entry_lead(op, room.as_ref(), Pt { x: o0.x, y: o0.y }, segment_tangent(offset[offset.len() - 1], offset[0], false), n, tool.dia, zs[0], "the path's normal", false)?;
    let lead_pt = lead.at;

    let mut g = Vec::new();
    g.push(format!("(--- side contour wire (engine): {} with T{}{} ---)", op.feature_name, op.tool_num, stock_suffix(op, 0.0)));
    g.push(direction_note(op));
    g.push(lead.line.clone());
    g.push(format!("G00 Z{}", f(clearance_z)));
    g.push(format!("G00 X{} Y{}", f(lead_pt.x), f(lead_pt.y)));
    g.push(format!("G00 Z{}", f(rapid_z)));
    g.push(format!("G01 Z{} F{}", f(zs[0] + lead.rise), f(plunge)));
    g.push(format!("G01 X{} Y{}{} F{}", f(o0.x), f(o0.y), lead.z_word(zs[0]), f(feed)));
    g.extend(emit_wire_loop(offset, &zs, feed));
    g.push(format!("G01 X{} Y{}", f(lead_pt.x), f(lead_pt.y))); // lead out
    g.push(format!("G00 Z{}", f(clearance_z)));
    Ok(g)
}

// The cut height at an offset-path point: nearest authored segment in plan
// view, Z linear in that segment's parameter, minus the extrusion. Clamped
// projection so a fillet vertex past a segment's end reads the CORNER's Z —
// which is exactly the constant-Z-arc property the caller guards.
//
// Ties are real, not a corner case: an INWARD offset corner is equidistant
// from both adjacent segments by construction (that is what an offset corner
// is), and when the two walls carry different heights there, one tool
// position must serve both contacts. The deeper of the tied heights wins:
// the wall promise (full height cut) then holds on both walls, at the cost
// of up to r x |slope difference| of extra depth into the undeclared
// interior — the region the model already reports unmodeled. Picking by
// strict nearest would make the choice fall to iteration order instead.
pub(crate) fn wire_cut_z(x: f64, y: f64, wire: &[Pt3], extent: f64) -> f64 {
    let n = wire.len();
    let mut best_d2 = f64::INFINITY;
    let mut best_z = wire[0].z;
    for i in 0..n {
        let a = wire[i];
        let b = wire[(i + 1) % n];
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 <= 0.0 {
            0.0
        } else {
            (((x - a.x) * dx + (y - a.y) * dy) / len2).clamp(0.0, 1.0)
        };
        let (px, py) = (a.x + t * dx, a.y + t * dy);
        let d2 = (x - px) * (x - px) + (y - py) * (y - py);
        let z = a.z + t * (b.z - a.z);
        // Strictly closer wins; a tie (within squared-distance noise) keeps
        // the deeper height.
        if d2 < best_d2 - TIE_EPS_2 {
            best_d2 = d2;
            best_z = z;
        } else if d2 <= best_d2 + TIE_EPS_2 {
            best_d2 = best_d2.min(d2);
            best_z = best_z.min(z);
        }
    }
    best_z - extent
}

// Squared-distance tolerance for the tie test above: 1e-6 mm in distance is
// far below any machining tolerance and far above f64 noise at part scale.
const TIE_EPS_2: f64 = 1e-12;

// Z per offset vertex, plus the constant-Z arc guard — shared by both comp
// modes since the budget slice gave control mode an offset edge of its own.
// Each vertex takes the cut Z of the nearest authored segment in plan view
// (nearest-by-XY is order-independent, so winding normalization never
// desynchronizes the heights). A fillet's two ends re-attach through the two
// segments meeting at its corner, one shared Z — if they disagree, some
// vertex re-attached across the loop (a plan view too tight for this offset
// distance) and emitting the arc would improvise a helix: the offending pair
// comes back as the error for the caller to phrase in its own mode's words.
fn wire_offset_zs(offset: &[OffsetVertex], wire: &[Pt3], extent: f64) -> Result<Vec<f64>, (f64, f64)> {
    let zs: Vec<f64> = offset
        .iter()
        .map(|v| wire_cut_z(v.x, v.y, wire, extent))
        .collect();
    let n = offset.len();
    for i in 0..n {
        if offset[i].bulge.abs() > 1e-9 && (zs[i] - zs[(i + 1) % n]).abs() > 1e-6 {
            return Err((zs[i], zs[(i + 1) % n]));
        }
    }
    Ok(zs)
}

// The Z-carrying sibling of emit_offset_loop, one per vertex height: straight
// segments interpolate the wall's top edge exactly (linear in the segment
// parameter both sides of the offset); arcs are constant-Z by the guard in
// wire_offset_zs, so the shared arc primitive emits them unchanged (modal Z
// holds through the arc). Shared by both comp modes for the same reason as
// emit_offset_loop: two copies of a bulge walk is where a silent gouge hides.
fn emit_wire_loop(offset: &[OffsetVertex], zs: &[f64], feed: f64) -> Vec<String> {
    let n = offset.len();
    let mut g = Vec::with_capacity(n);
    for i in 0..n {
        let from = offset[i];
        let to = offset[(i + 1) % n];
        if from.bulge.abs() <= 1e-9 {
            g.push(format!("G01 X{} Y{} Z{}", f(to.x), f(to.y), f(zs[(i + 1) % n])));
        } else {
            let (acx, acy) = arc_center(&from, &to);
            g.push(arc(from.bulge < 0.0, to.x, to.y, acx - from.x, acy - from.y, feed));
        }
    }
    g
}

// Walk a closed offset loop from its first vertex all the way back around,
// assuming the tool already sits on that vertex. The bulge lives on the vertex
// a segment LEAVES (offset.rs), so segment offset[i] -> offset[(i+1)%n] carries
// offset[i].bulge, and iterating 0..n covers the closing segment too. A
// straight segment stays a modal G01 (no repeated F, so the arc-free case is
// byte-identical to the old straight-only emission); a rounded one becomes an
// exact G02/G03 with the center as incremental IJ from the segment start.
//
// Shared by both comp modes: control mode grew an offset path of its own when
// stock_to_leave landed, and two copies of a bulge walk is exactly the kind of
// duplicate a silent gouge hides in.
fn emit_offset_loop(offset: &[OffsetVertex], feed: f64) -> Vec<String> {
    let n = offset.len();
    let mut g = Vec::with_capacity(n);
    for i in 0..n {
        let from = offset[i];
        let to = offset[(i + 1) % n];
        if from.bulge.abs() <= 1e-9 {
            g.push(format!("G01 X{} Y{}", f(to.x), f(to.y)));
        } else {
            let (acx, acy) = arc_center(&from, &to);
            // G17 direction: positive bulge is a CCW arc (G03), negative CW (G02).
            g.push(arc(from.bulge < 0.0, to.x, to.y, acx - from.x, acy - from.y, feed));
        }
    }
    g
}

// Two message fragments so an error under an allowance names the allowance —
// otherwise the author is told to reach for the tool when the skin is what
// stopped the offset fitting.
fn skin_note(skin: f64) -> String {
    if skin > 0.0 { format!(" plus the stock_to_leave offset ({:.3}mm)", skin) } else { String::new() }
}

fn skin_knob(skin: f64) -> &'static str {
    if skin > 0.0 { ", reduce stock_to_leave" } else { "" }
}

// Copy of the points wound counter-clockwise (reverse when the authored area is
// clockwise), matching the offset routine's "positive = inward for CCW" convention.
fn to_ccw(points: &[Pt], area: f64) -> Vec<Pt> {
    let mut pts = points.to_vec();
    if area < 0.0 {
        pts.reverse();
    }
    pts
}

// ---- The lead: where a wall trace plunges (Phase 47 slice 2, fork 1A) ------
// One construction for every wall trace in this file — both comp arms of the
// polyline and wire `Side`s, and the `Form`'s two closures. DIRECTION: the
// void-side normal of the loop the arm drives, at its first vertex — the one
// direction that never points into the material its first segment stands
// against, where the ray from a vertex-average centroid can (a C-shaped boss;
// the real part's L-shaped ribs, DECISIONS.md "DRAFT: Phase 47 opens…",
// premise 4). LENGTH: one diameter beyond the path point where the stock
// model says a tool standing there is in air (`stock::EntryRoom` — the entry
// restriction's own question, asked before the point is chosen), otherwise
// the longest shorter back-off that is, to three decimals, down to zero — a
// plunge ON the path — and the program says which. Where the model has no
// claim the lead is the diameter it always was and says nothing: the
// restriction's own silence (`move_declines`) tells the reader the op went
// unread, and a lead chosen on no claim must not sound like one chosen on
// one.
//
// Every arm reads the TOOL-CENTRE loop it drives — the offset loop under
// engine comp, the resolved head's parallel under control (its first vertex
// is the compensated start, a radius along both edges at a pocket's corner,
// so a point a radius along one edge alone would stand half a tool in the
// other wall), the axis path for a Form. Both control heads carry one lead
// XY, and at zero room the comp-on move still runs the whole radius it ramps
// on.

pub(crate) struct Lead {
    pub at: Pt,
    /// The program's sentence before the descent at `at`: how long the lead
    /// is and what licensed it (ROADMAP item 2 — every descent is said).
    pub line: String,
    /// The lead is shorter than the diameter asked; the lead out says so too.
    pub short: bool,
    /// How far above the cut the plunge stops (ROADMAP item 2, slice 5):
    /// the flight onto the start descends the rest. Zero for a flat lead.
    pub rise: f64,
}

impl Lead {
    /// The Z word the flight onto the start carries where the lead descends;
    /// empty for a flat lead, so an unsaid program keeps its bytes.
    fn z_word(&self, z_cut: f64) -> String {
        if self.rise > 0.0 { format!(" Z{}", f(z_cut)) } else { String::new() }
    }
}

/// The void-side unit normal at a loop's first VERTEX: the bisector of the
/// normals of the two segments that meet there — the one the loop leaves
/// along and the one it closes along — which is what a normal at a vertex
/// means. On a straight run the two agree. At a corner the bisector points
/// away from BOTH walls, where the first segment's normal alone would run
/// beside the other one: a pocket's trace starts at its corner's offset, a
/// radius from each wall, and a lead along one wall's normal plunges with
/// the body rubbing the other (and a chamfer cone cutting its own bevel) —
/// the first cut at this slice did exactly that, and the entry taxonomy
/// flagged it. A tangent-continuous start (a corner arc closing onto the
/// first segment) bisects to the segment's own normal. `void_inside` says
/// whether the loop's interior is the void (a pocket) or the material (a
/// boss); the loop's own winding, as emitted, says which side of travel the
/// interior is.
fn void_normal(path: &[OffsetVertex], void_inside: bool) -> (f64, f64) {
    let n = path.len();
    let pts: Vec<Pt> = path.iter().map(|v| Pt { x: v.x, y: v.y }).collect();
    let loop_ccw = signed_area(&pts) >= 0.0;
    // Left of travel is the interior of a CCW loop; the void is the interior
    // of a pocket and the exterior of a boss.
    let void_side = |tx: f64, ty: f64| if loop_ccw == void_inside { (-ty, tx) } else { (ty, -tx) };
    let (out_x, out_y) = segment_tangent(path[0], path[1 % n], true);
    let (in_x, in_y) = segment_tangent(path[n - 1], path[0], false);
    let (ax, ay) = void_side(out_x, out_y);
    let (bx, by) = void_side(in_x, in_y);
    let (sx, sy) = (ax + bx, ay + by);
    let len = (sx * sx + sy * sy).sqrt();
    // A spike — the loop turns straight back — has no bisector; the
    // segment's own normal is as good a direction as any there.
    if len < 1e-9 { (ax, ay) } else { (sx / len, sy / len) }
}

/// A segment's unit tangent at its start (`at_start`) or its end. An arc's
/// tangent leaves the chord by half the included angle: a CCW arc (positive
/// bulge) bows right of its chord, so it starts heading right of it and ends
/// heading left — rotate by ∓2·atan(bulge).
fn segment_tangent(a: OffsetVertex, b: OffsetVertex, at_start: bool) -> (f64, f64) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt().max(1e-9);
    let (tx, ty) = (dx / len, dy / len);
    if a.bulge == 0.0 {
        return (tx, ty);
    }
    let th = if at_start { -2.0 * a.bulge.patan() } else { 2.0 * a.bulge.patan() };
    let (c, s) = (th.pcos(), th.psin());
    (tx * c - ty * s, tx * s + ty * c)
}

/// The entry a wall trace makes, under the key (ROADMAP item 2, slice 4):
/// unsaid or `entry=lead`, the lead `lead_in` finds — refused under the key
/// where it finds no room, since a plunge on the path is not a lead;
/// `entry=plunge`, straight down on the path; `entry=ramp`, refused, since a
/// ramp down a wall is not built.
/// The pass's own line behind its start, in the lead's sentence.
const BACK: &str = "the path's own line, back from its start";

fn entry_lead(op: &Operation, room: Option<&EntryRoom>, path_pt: Pt, t: (f64, f64), n: (f64, f64), dia: f64, z: f64, along: &str, open: bool) -> Result<Lead, String> {
    match op.entry {
        Some(crate::parse::EntryKind::Plunge) => Ok(Lead { at: path_pt, line: format!("(plunge: on the path{})", crate::ops::clear::asks(op)), short: true, rise: 0.0 }),
        Some(crate::parse::EntryKind::Ramp) => Err(format!("entry=ramp on '{}': a wall is entered on a lead from the air beside it, or by a plunge on the path; a ramp down the wall is not built", op.feature_name)),
        kind => {
            // The shape said (slice 5): the lead is whole and in air, or the
            // op refuses; an open wall has no air side to turn it toward.
            let shape = LeadShape::of(op, dia);
            if shape.said {
                if open && shape.angle_deg != 90.0 {
                    return Err(format!("lead_angle={} on '{}': an open wall is entered along its own line, and has no air side to turn the lead toward", f(shape.angle_deg), op.feature_name));
                }
                let at = shape.point(path_pt, t, if open { n } else { air_side(t, n) });
                let words = shape.words(along, BACK);
                return match room {
                    None => Ok(Lead { at, line: format!("(lead: {words} — the stock model has no claim here)"), short: false, rise: shape.rise() }),
                    Some(r) if lead_clear(r, at, path_pt, z) => Ok(Lead { at, line: format!("(lead: {words}, in air)"), short: false, rise: shape.rise() }),
                    Some(_) => Err(format!("lead on '{}': {words} — the stock model reads standing material under the lead from ({}, {}), and a lead that is asked for must be whole", op.feature_name, f(at.x), f(at.y))),
                };
            }
            let lead = lead_in(room, path_pt, n, dia, z, along);
            if kind.is_some() && lead.line.starts_with("(plunge:") {
                return Err(format!("entry=lead on '{}': no room for a lead along {along} — the stock model reads standing material there", op.feature_name));
            }
            Ok(lead)
        }
    }
}

/// The whole lead in air by the stock model (ROADMAP item 2, slice 5): the
/// point the tool descends at and every half millimetre of the flight from
/// it to the start. The material the op itself removes is not standing, so
/// the flight may cross the skin the pass takes and nothing else.
fn lead_clear(room: &EntryRoom, at: Pt, start: Pt, z: f64) -> bool {
    let d = (start.x - at.x).phypot(start.y - at.y);
    let n = ((d / 0.5).ceil() as usize).max(1);
    (0..=n).all(|k| {
        let s = k as f64 / n as f64;
        room.in_air(at.x + s * (start.x - at.x), at.y + s * (start.y - at.y), z)
    })
}

/// The air side of a path's incoming line at its start: the perpendicular to
/// that line, on the side the void-side normal `n` leans toward. At a corner
/// the normal is the bisector, which is not square to either line, and the
/// keys' angle is counted off the line itself, as the sentence says.
fn air_side(t: (f64, f64), n: (f64, f64)) -> (f64, f64) {
    let left = (-t.1, t.0);
    if left.0 * n.0 + left.1 * n.1 >= 0.0 { left } else { (t.1, -t.0) }
}

/// A closed path turned so the lead the keys shape lies in air (ROADMAP
/// item 2, slice 5): each vertex tried as the start, in order, the first
/// with the whole lead in air kept; none, the op refuses. Unsaid, or with no
/// stock model to ask, the path keeps its start.
fn turned(op: &Operation, room: Option<&EntryRoom>, path: Vec<OffsetVertex>, void_inside: bool, dia: f64, z: f64) -> Result<Vec<OffsetVertex>, String> {
    let shape = LeadShape::of(op, dia);
    if !shape.said || matches!(op.entry, Some(crate::parse::EntryKind::Plunge | crate::parse::EntryKind::Ramp)) {
        return Ok(path);
    }
    let Some(room) = room else { return Ok(path) };
    let n = path.len();
    for k in 0..n {
        let mut rot = path.clone();
        rot.rotate_left(k);
        // Behind a closed path's start is its incoming line: the lead
        // then lies on the path itself, which the pass cuts last.
        let t = segment_tangent(rot[n - 1], rot[0], false);
        let nn = air_side(t, void_normal(&rot, void_inside));
        let start = Pt { x: rot[0].x, y: rot[0].y };
        let q = shape.point(start, t, nn);
        if lead_clear(room, q, start, z) {
            return Ok(rot);
        }
    }
    Err(format!("lead on '{}': {} — no start on the path has the whole lead in air", op.feature_name, shape.words("the path's normal", BACK)))
}

/// A sibling loop turned to begin at its vertex nearest `at`, so the two
/// heads of a control-comp pass keep one start; returns how far it turned.
fn follow(v: &mut [OffsetVertex], at: Pt) -> usize {
    // Tied vertices: the first from the loop's start.
    let k = crate::tie::first_least(0..v.len(), |&k| (v[k].x - at.x).phypot(v[k].y - at.y)).unwrap_or(0);
    v.rotate_left(k);
    k
}

/// The lead point for a wall trace whose tool-centre path starts at `path_pt`
/// with `n` its void-side normal there, plunging to the emitted `z`; `room` is
/// the stock model's reading when it has a claim; `along` names `n` in the
/// sentence — the path's normal for a loop, its own line for an open chain.
fn lead_in(room: Option<&EntryRoom>, path_pt: Pt, n: (f64, f64), dia: f64, z: f64, along: &str) -> Lead {
    let at = |d: f64| Pt { x: path_pt.x + n.0 * d, y: path_pt.y + n.1 * d };
    let Some(room) = room else {
        return Lead { at: at(dia), line: format!("(lead: {}mm along {along} — the stock model has no claim here)", f(dia)), short: false, rise: 0.0 };
    };
    let clear = |d: f64| {
        let q = at(d);
        room.in_air(q.x, q.y, z)
    };
    if clear(dia) {
        return Lead { at: at(dia), line: format!("(lead: {}mm along {along}, in air)", f(dia)), short: false, rise: 0.0 };
    }
    // The longest shorter back-off, to three decimals: a coarse scan down from
    // the diameter finds the first step that stands in air, and a bisection
    // between it and the step above sharpens the edge. Every candidate is
    // probed and only a clear one is ever taken, so what is emitted is in air
    // by the model whatever the scan stepped over.
    const STEPS: usize = 16;
    let snap = |d: f64| (d * 1000.0).floor() / 1000.0;
    let mut hi = dia;
    let mut found = None;
    for k in 1..=STEPS {
        let d = snap(dia * (STEPS - k) as f64 / STEPS as f64);
        if clear(d) {
            found = Some(d);
            break;
        }
        hi = d;
    }
    let mut lo = found.unwrap_or(0.0);
    if found.is_some() {
        loop {
            let mid = snap((lo + hi) / 2.0);
            if mid <= lo || mid >= hi {
                break;
            }
            if clear(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
    }
    // The probe forgives the restriction's slack; a shortened lead gives it
    // back, so the body stands tangent to what stopped it and the number
    // printed is the room there is.
    // Room within the one chord tolerance is no room: the model's edge is
    // chorded to it, and a lead that short would be the chording's, not the
    // slot's.
    let lo = snap((lo - room.slack()).max(0.0));
    let lo = if lo > crate::CHORD_TOL_MM { lo } else { 0.0 };
    let line = if lo > 0.0 {
        format!("(lead: {:.3}mm of room along {along} — one diameter asked)", lo)
    } else {
        format!("(plunge: on the path — 0.000mm of room along {along})")
    };
    Lead { at: at(lo), line, short: true, rise: 0.0 }
}

/// The room the stock model reads for this op, when it has a claim. The op is
/// found by identity in the part's `run` list — where every op an emitter is
/// handed lives; one that is not there (a test's detached op) is unread.
fn room_for(p: &Part, op: &Operation) -> Option<EntryRoom> {
    let idx = p.operations.iter().position(|o| std::ptr::eq(o, op))?;
    crate::stock::entry_room(p, idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The lead home's two readings (Phase 47 slice 2): an arc's tangent at
    // either end, and the void-side bisector at a loop's first vertex.
    #[test]
    fn test_an_arcs_tangent_leaves_its_chord_by_half_the_included_angle() {
        // A CCW quarter arc on the unit circle from (1, 0) to (0, 1): bulge
        // tan(22.5°). It starts heading +Y and ends heading -X.
        let a = OffsetVertex { x: 1.0, y: 0.0, bulge: (std::f64::consts::PI / 8.0).ptan() };
        let b = OffsetVertex { x: 0.0, y: 1.0, bulge: 0.0 };
        let (sx, sy) = segment_tangent(a, b, true);
        assert!((sx).abs() < 1e-9 && (sy - 1.0).abs() < 1e-9, "start tangent: ({sx}, {sy})");
        let (ex, ey) = segment_tangent(a, b, false);
        assert!((ex + 1.0).abs() < 1e-9 && (ey).abs() < 1e-9, "end tangent: ({ex}, {ey})");
    }

    #[test]
    fn test_the_void_normal_at_a_corner_is_the_bisector_and_a_tangent_start_is_the_segments_own() {
        let v = |x: f64, y: f64, bulge: f64| OffsetVertex { x, y, bulge };
        // A CCW square pocket starting at its corner: the void is inside, the
        // first segment heads +X (normal +Y), the closing one arrives along
        // -Y (normal +X): the bisector is the diagonal.
        let sq = [v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0), v(0.0, 10.0, 0.0)];
        let (nx, ny) = void_normal(&sq, true);
        let d = std::f64::consts::FRAC_1_SQRT_2;
        assert!((nx - d).abs() < 1e-9 && (ny - d).abs() < 1e-9, "({nx}, {ny})");
        // The same square as a CCW BOSS loop (void outside): the diagonal
        // points the other way.
        let (nx, ny) = void_normal(&sq, false);
        assert!((nx + d).abs() < 1e-9 && (ny + d).abs() < 1e-9, "({nx}, {ny})");
        // A boss loop whose closing segment is a quarter arc landing tangent
        // onto the first segment (a rounded corner at the start): the
        // bisector is the first segment's own normal, outward.
        let r = (std::f64::consts::PI / 8.0).ptan();
        let rounded = [v(0.0, 0.0, 0.0), v(10.0, 0.0, 0.0), v(10.0, 10.0, 0.0), v(-5.0, 10.0, 0.0), v(-5.0, 5.0, r)];
        // ...the closing arc runs (-5, 5) -> (0, 0) CCW about (0, 5), ending along +X.
        let (nx, ny) = void_normal(&rounded, false);
        assert!((nx).abs() < 1e-9 && (ny + 1.0).abs() < 1e-9, "({nx}, {ny})");
    }
    #[test]
    fn test_a_turned_path_rides_the_same_arcs_the_other_way() {
        let v = |x: f64, y: f64, bulge: f64| OffsetVertex { x, y, bulge };
        let r = (std::f64::consts::PI / 8.0).ptan();
        // The arcs of a path, as (centre to the micron, CCW): what a turned
        // path must keep, every sense flipped.
        let arcs = |segs: Vec<(OffsetVertex, OffsetVertex)>| -> Vec<(i64, i64, bool)> {
            let mut out: Vec<(i64, i64, bool)> = segs
                .into_iter()
                .filter(|(a, _)| a.bulge.abs() > 1e-9)
                .map(|(a, b)| {
                    let (cx, cy) = arc_center(&a, &b);
                    ((cx * 1000.0).round() as i64, (cy * 1000.0).round() as i64, a.bulge > 0.0)
                })
                .collect();
            out.sort();
            out
        };
        let flipped = |a: &[(i64, i64, bool)]| a.iter().map(|c| (c.0, c.1, !c.2)).collect::<Vec<_>>();
        let closed_segs = |p: &[OffsetVertex]| (0..p.len()).map(|i| (p[i], p[(i + 1) % p.len()])).collect::<Vec<_>>();
        let open_segs = |p: &[OffsetVertex]| p.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>();

        // A loop with a quarter arc in its middle and one closing onto its
        // start: turned, it keeps its first vertex and both arcs.
        let lp = [v(0.0, 0.0, 0.0), v(10.0, 0.0, r), v(15.0, 5.0, 0.0), v(15.0, 10.0, 0.0), v(-5.0, 10.0, 0.0), v(-5.0, 5.0, r)];
        let t = turned_verts(&lp, true);
        assert_eq!((t[0].x, t[0].y), (0.0, 0.0));
        assert_eq!(turned_verts(&t, true), lp.to_vec(), "turning twice is the path itself");
        let a = arcs(closed_segs(&lp));
        assert_eq!(a.len(), 2);
        assert_eq!(arcs(closed_segs(&t)), flipped(&a));

        // An open path starts at its far end; its arc keeps its centre.
        let open = &lp[..4];
        let t = turned_verts(open, false);
        assert_eq!((t[0].x, t[0].y), (15.0, 10.0));
        assert_eq!(t.last().unwrap().bulge, 0.0);
        assert_eq!(turned_verts(&t, false), open.to_vec());
        let a = arcs(open_segs(open));
        assert_eq!(a.len(), 1);
        assert_eq!(arcs(open_segs(&t)), flipped(&a));
    }

    use crate::offset::offset_closed_polyline_verts;

    // The load-bearing sign claim behind control-mode's skin (DECISIONS.md
    // "stock_to_leave is a radial allowance…"): `parallel_offset` is relative to
    // travel direction, so a POSITIVE distance on the AUTHORED polyline is
    // always "left of travel" — which is exactly G41's side and exactly the
    // material side under the winding convention. That means control mode can
    // offset the authored points directly, with no to_ccw and (critically) no
    // reversal of the result: bulges live on the segment LEAVING a vertex, so
    // reversing a bulge-carrying loop is not `.reverse()` and getting it wrong
    // is a silent gouge. Pinned on both windings against engine mode's
    // normalize-then-sign dance, which is the behavior already in the goldens.
    fn square(ccw: bool) -> Vec<Pt> {
        let s = [(20.0, 20.0), (80.0, 20.0), (80.0, 80.0), (20.0, 80.0)];
        let mut v: Vec<Pt> = s.iter().map(|&(x, y)| Pt { x, y }).collect();
        if !ccw { v.reverse(); }
        v
    }

    fn offset_via_normalize(points: &[Pt], d: f64) -> Vec<Vec<OffsetVertex>> {
        let area = signed_area(points);
        let ccw = to_ccw(points, area);
        offset_closed_polyline_verts(&ccw, if area >= 0.0 { d } else { -d })
    }

    fn extent_of(loops: &[Vec<OffsetVertex>]) -> (f64, f64) {
        let vs = &loops[0];
        let lo = vs.iter().fold(f64::INFINITY, |m, v| m.min(v.x));
        let hi = vs.iter().fold(f64::NEG_INFINITY, |m, v| m.max(v.x));
        (lo, hi)
    }

    fn wall_part(comp: CompMode) -> (Part, Operation) {
        let mut p = Part::default();
        p.tools.insert(5, crate::parse::Tool { num: 5, name: "Endmill".into(), dia: 10.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
        let op = Operation {
            op_type: crate::parse::OpType::Side,
            tool_num: 5,
            feature_name: "Wall".into(),
            stepover: None,
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
            comp,
            compress: crate::parse::CompressMode::Chords,
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
        (p, op)
    }

    fn contains_run(hay: &[String], needle: &[String]) -> bool {
        !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
    }

    /// The polyline twin pin (DECISIONS.md "the control-comp centerline
    /// resolve", pin 1): the control head's resolved cut loop is the engine
    /// sibling's offset loop, line for line. Not a tautology — the two modes
    /// reach the parallel through different sign dances (control offsets the
    /// AUTHORED points by a positive skin + r; engine normalizes to CCW and
    /// signs the distance by the winding), so equality is the winding
    /// convention holding at the resolved channel, with a skin present so
    /// the composition claim is the thing exercised.
    #[test]
    fn test_control_resolved_loop_is_the_engine_loop() {
        let (mut p, mut op) = wall_part(CompMode::Control);
        p.features.insert(
            "Wall".into(),
            Feature::ProfilePocket { points: square(true), z: None, extent: 5.0, edge_round: None, chamfer: None, chamfer_marks: Vec::new() },
        );
        let control = finish_profile_wall(&p, &op).expect("offsets fit");
        let resolved = control.resolved.expect("a control op carries the resolved channel");
        op.comp = CompMode::Engine;
        let engine = finish_profile_wall(&p, &op).expect("offsets fit");
        assert!(engine.resolved.is_none(), "engine text already is the tool center");
        // The engine sibling's loop, built by its own dance.
        let loops = offset_via_normalize(&square(true), 5.0 + 0.5);
        assert_eq!(loops.len(), 1);
        let expected = emit_offset_loop(&loops[0], p.global_feed);
        assert!(
            contains_run(&engine.text, &expected),
            "the engine text must carry its own loop (the pin's control side is meaningless otherwise)"
        );
        assert!(
            contains_run(&resolved, &expected),
            "the control head's resolved loop must be the engine loop, line for line"
        );
    }

    /// The wire arm's twin pin: same claim one Z word later — the resolved
    /// loop is the engine loop with the same re-attached heights.
    #[test]
    fn test_control_resolved_wire_loop_is_the_engine_loop() {
        let wire: Vec<Pt3> = [
            (20.0, 20.0, 0.0),
            (80.0, 20.0, -2.0),
            (80.0, 80.0, 0.0),
            (20.0, 80.0, -2.0),
        ]
        .iter()
        .map(|&(x, y, z)| Pt3 { x, y, z })
        .collect();
        let (mut p, mut op) = wall_part(CompMode::Control);
        p.features.insert(
            "Wall".into(),
            Feature::ContourPocket { points: wire.clone(), extent: 3.0 },
        );
        let control = finish_contour_wire(&p, &op).expect("offsets fit");
        let resolved = control.resolved.expect("a control op carries the resolved channel");
        op.comp = CompMode::Engine;
        let engine = finish_contour_wire(&p, &op).expect("offsets fit");
        // The engine sibling's loop, built by its own dance.
        let proj: Vec<Pt> = wire.iter().map(Pt3::xy).collect();
        let loops = offset_via_normalize(&proj, 5.0 + 0.5);
        assert_eq!(loops.len(), 1);
        let zs = wire_offset_zs(&loops[0], &wire, 3.0).expect("constant-Z corners");
        let expected = emit_wire_loop(&loops[0], &zs, p.global_feed);
        assert!(
            contains_run(&engine.text, &expected),
            "the engine text must carry its own wire loop"
        );
        assert!(
            contains_run(&resolved, &expected),
            "the control head's resolved wire loop must be the engine loop, Z words included"
        );
    }

    /// Machinability parity (DECISIONS.md "the control-comp centerline
    /// resolve"): a profile whose tool-center parallel degenerates was
    /// already rejected under engine comp; under control comp it emitted and
    /// left the collapse to the controller's comp — an alarm mid-program or
    /// a gouge. Both modes now reject the same geometry, each naming its
    /// own mechanism.
    #[test]
    fn test_control_refuses_where_the_engine_refuses() {
        // A 60 × 4 slot: the interior parallel at r + skin = 5.5 collapses.
        let narrow: Vec<Pt> = [(20.0, 20.0), (80.0, 20.0), (80.0, 24.0), (20.0, 24.0)]
            .iter()
            .map(|&(x, y)| Pt { x, y })
            .collect();
        let (mut p, mut op) = wall_part(CompMode::Engine);
        p.features.insert(
            "Wall".into(),
            Feature::ProfilePocket { points: narrow, z: None, extent: 5.0, edge_round: None, chamfer: None, chamfer_marks: Vec::new() },
        );
        let err = finish_profile_wall(&p, &op).expect_err("engine rejects the collapse");
        assert!(err.contains("comp=engine"), "got: {err}");
        op.comp = CompMode::Control;
        let err = finish_profile_wall(&p, &op).expect_err("control now rejects the same geometry");
        assert!(
            err.contains("tool-center parallel"),
            "the control error must name the parallel: {err}"
        );
    }

    #[test]
    fn test_positive_offset_on_authored_points_is_g41s_side() {
        for ccw in [true, false] {
            let pts = square(ccw);
            let direct = offset_closed_polyline_verts(&pts, 2.0);
            let normalized = offset_via_normalize(&pts, 2.0);
            assert_eq!(direct.len(), 1, "ccw={ccw}: one loop");
            assert_eq!(normalized.len(), 1, "ccw={ccw}: one loop");
            assert_eq!(
                extent_of(&direct), extent_of(&normalized),
                "ccw={ccw}: a positive offset on the authored points must land where \
                 the normalize-then-sign path lands"
            );
        }
    }

    #[test]
    fn test_the_two_windings_offset_to_opposite_sides() {
        // The convention itself, so the test above can't pass by both paths
        // being wrong in the same direction: CCW authoring shrinks (tool
        // inside), CW authoring grows (tool outside).
        let (lo_in, hi_in) = extent_of(&offset_closed_polyline_verts(&square(true), 2.0));
        let (lo_out, hi_out) = extent_of(&offset_closed_polyline_verts(&square(false), 2.0));
        assert!(lo_in > 20.0 && hi_in < 80.0, "CCW must shrink, got {lo_in}..{hi_in}");
        assert!(lo_out < 20.0 && hi_out > 80.0, "CW must grow, got {lo_out}..{hi_out}");
    }
}
