use crate::math::Portable as _;
// ---- the schedule: run lines for the features no run names yet ---------------
//
// ROADMAP item 1, slice 10. After the review a project holds a stock, its
// tools and the accepted features, and the run lines were typed by hand:
// forty of them on the real part. `template::op_lines` is the menu for ONE
// feature, with `T?` where a tool would be a guess. A schedule has the
// project's own tool lines to pick from, so it picks and says how:
//
//   - a hole is drilled by the Drill of its own diameter; with none declared
//     it is milled like a bore by the largest endmill that fits down it:
//     cleared, then its wall traced, unless the wall pass alone sweeps the
//     bore out (the tool reaches the centre, or a hole through the bore's
//     floor takes what the pass leaves in the middle);
//   - a counterbore seat is plunged by the Endmill of the seat's diameter,
//     a countersink by a ChamferMill;
//   - floors, pockets and bosses take the largest declared Endmill; domes
//     and free surfaces the largest Ballmill;
//   - a chamfer takes the ChamferMill at the loop's angle (any ChamferMill
//     when the loop names none), an edge round the CornerRounder of that
//     radius;
//   - stepover and stepdown are half the tool's diameter, the number the
//     window's creation form already suggests;
//   - a bore's wall and floor, a round boss's wall and the face of a round
//     blank are written `compress=arcs`: a ring that chords a circle is one
//     G02/G03 block where the chords were hundreds of lines (slice 13), and a
//     loop that is no circle keeps its chords under the word.
//
// The order is a shop's: the top face first when the blank stands above
// it, then drills, floors from the highest down, pockets and milled bores,
// standing material, and every chamfer and round last. A Plane at the
// blank's top is the blank's own face and gets a note instead of a pass;
// Planes at one height share one Clear, since a Clear on a Plane works the
// blank's whole plan there. A round boss whose wall is the blank's own rim
// gets a note too, and one within a tool's width of the rim is swept by
// its wall pass alone. A boss standing on a floor that is cleared is cut
// around by that Clear (`stock::host_of`, the engine's own island rule) and
// gets only its wall pass. A feature declared `given` is air cut on the
// other setup; only its mouth chamfer or round, which is this setup's, gets
// a line. When the project says `given from`, a void the earlier program's
// air already holds whole (the certificate the validator runs on a given
// void, `stock::certify_given`) is written as `given <name>` rather than
// cut again, and its mouth is dressed like any given void's.
//
// Every line is parsed with the project, in order, before it is offered. A
// line the parser refuses becomes a note carrying the parser's own
// sentence, so what the schedule writes always parses and the refusal is
// still on the page. The CLI prints the same text the window appends
// (`nacs --schedule <project>`), so the picture says nothing the terminal
// cannot.

use crate::parse::{self, Feature, Part};
use std::path::Path;

/// The schedule: the run lines to append, and the notes about what got no
/// line and why. Both are `.part` text once the notes wear a `#`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Schedule {
    pub lines: Vec<String>,
    pub notes: Vec<String>,
}

/// Is `name` already named by a run, as its surface or one of its points?
pub fn consumed(p: &Part, name: &str) -> bool {
    p.operations.iter().any(|op| {
        op.feature_name == name || op.drill_points.as_ref().is_some_and(|pts| pts.iter().any(|q| q == name))
    })
}

/// The project's tools by number, lowest first, so every pick below is
/// deterministic whatever order the map iterates in.
fn tools_by_number(p: &Part) -> Vec<(usize, &parse::Tool)> {
    let mut v: Vec<(usize, &parse::Tool)> = p.tools.iter().map(|(n, t)| (*n, t)).collect();
    v.sort_by_key(|(n, _)| *n);
    v
}

/// The lowest-numbered Drill of exactly this diameter.
fn drill_for(p: &Part, dia: f64) -> Option<usize> {
    tools_by_number(p)
        .into_iter()
        .find(|(_, t)| t.name == "Drill" && t.profile.is_none() && (t.dia - dia).abs() < 1e-6)
        .map(|(n, _)| n)
}

/// The largest tool of a kind, below `under` when a bore bounds it; the
/// lowest number on a tie.
fn largest(p: &Part, kind: &str, under: Option<f64>) -> Option<usize> {
    tools_by_number(p)
        .into_iter()
        .filter(|(_, t)| t.name == kind && under.is_none_or(|u| t.dia < u - 1e-9))
        .max_by(|a, b| a.1.dia.partial_cmp(&b.1.dia).expect("a tool's diameter is finite"))
        .map(|(n, _)| n)
}

/// The Endmill of exactly this diameter (a plunged seat is the tool's own circle).
fn endmill_of(p: &Part, dia: f64) -> Option<usize> {
    tools_by_number(p)
        .into_iter()
        .find(|(_, t)| t.name == "Endmill" && (t.dia - dia).abs() < 1e-6)
        .map(|(n, _)| n)
}

/// The lowest-numbered ChamferMill at this angle, or any when none is asked.
fn chamfer_mill(p: &Part, angle: Option<f64>) -> Option<usize> {
    tools_by_number(p)
        .into_iter()
        .find(|(_, t)| t.chamfer.as_ref().is_some_and(|c| angle.is_none_or(|a| (c.angle - a).abs() < 1e-9)))
        .map(|(n, _)| n)
}

/// The lowest-numbered CornerRounder of this radius.
fn rounder_of(p: &Part, radius: f64) -> Option<usize> {
    tools_by_number(p)
        .into_iter()
        .find(|(_, t)| t.rounder.as_ref().is_some_and(|r| (r.radius - radius).abs() < 1e-6))
        .map(|(n, _)| n)
}

/// A millimetre or degree value in a note, without trailing zeros.
fn num(v: f64) -> String {
    crate::gcode::f(v).trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Half the tool's diameter, spelled as the window's form suggests it.
fn half(p: &Part, tool: usize) -> String {
    crate::template::suggested_stepover(p, &format!("T{tool}")).unwrap_or_else(|| "?".to_string())
}

/// One `points=[…]` line per tool, the points in declaration order.
fn point_lines(verb: &str, groups: &[(usize, String)]) -> Vec<String> {
    let mut tools: Vec<usize> = Vec::new();
    for (t, _) in groups {
        if !tools.contains(t) {
            tools.push(*t);
        }
    }
    tools
        .into_iter()
        .map(|t| {
            let names: Vec<&str> = groups.iter().filter(|(n, _)| *n == t).map(|(_, s)| s.as_str()).collect();
            format!("run {verb}(points=[{}], tool=T{t}, cycle=Simple)", names.join(", "))
        })
        .collect()
}

/// The lines and notes before the parser has judged them: the rules in the
/// header, applied to every declared feature no run names, in declaration
/// order within each bucket.
fn propose(p: &Part) -> Schedule {
    let mut out = Schedule::default();
    let top = p.stock.z;

    // Declaration order, each name once.
    let mut names: Vec<&str> = Vec::new();
    for (name, ..) in &p.feature_lines {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    for (name, _) in p.features.iter() {
        if !names.contains(&name.as_str()) {
            names.push(name);
        }
    }
    let pending: Vec<(&str, &Feature)> = names
        .into_iter()
        .filter(|n| !consumed(p, n))
        .filter_map(|n| p.features.get(n).map(|f| (n, f)))
        .collect();

    let mut drills: Vec<(usize, String)> = Vec::new();
    let mut csinks: Vec<(usize, String)> = Vec::new();
    let mut cbores: Vec<(usize, String)> = Vec::new();
    // Height, name, and whether it is a Plane (whose Clear is the whole plan).
    let mut floors: Vec<(f64, String, bool)> = Vec::new();
    let mut voids: Vec<String> = Vec::new();
    let mut standing: Vec<String> = Vec::new();
    let mut forms: Vec<String> = Vec::new();
    // Two notes many features share, said once each with the names listed.
    let mut at_top: Vec<&str> = Vec::new();
    let mut at_rim: Vec<&str> = Vec::new();
    let mut given: Vec<&str> = Vec::new();
    // Voids the earlier program already cut (slice 12): `given <name>`
    // lines, written before the runs.
    let mut given_lines: Vec<String> = Vec::new();
    let mut already_air: Vec<&str> = Vec::new();

    let endmill = largest(p, "Endmill", None);
    let ballmill = largest(p, "Ballmill", None);
    let no_endmill = |what: &str| format!("{what} waits for an Endmill: none is declared");
    let no_ballmill = |what: &str| format!("{what} waits for a Ballmill: none is declared");

    for (name, f) in &pending {
        let name = *name;
        let is_given = p.given.iter().any(|g| g == name);
        let earlier_air = !is_given
            && p.given_from.is_some()
            && matches!(crate::stock::certify_given(p, name), Some(crate::stock::Certificate::Certified { samples }) if samples > 0);
        if earlier_air {
            already_air.push(name);
            given_lines.push(format!("given {name}"));
        }
        if is_given || earlier_air {
            if is_given {
                given.push(name);
            }
            let (chamfer, round) = match f {
                Feature::CylinderBore { chamfer, edge_round, .. }
                | Feature::ProfilePocket { chamfer, edge_round, .. }
                | Feature::ProfileBoss { chamfer, edge_round, .. }
                | Feature::CylinderBoss { chamfer, edge_round, .. } => (chamfer.as_ref().map(|c| c.angle), *edge_round),
                _ => (None, None),
            };
            push_forms(p, name, chamfer, round, &mut forms, &mut out.notes);
            continue;
        }
        match f {
            Feature::CylinderBore { dia, csink, cbore, chamfer, edge_round, .. } => {
                match drill_for(p, *dia) {
                    Some(t) => drills.push((t, name.to_string())),
                    None => match largest(p, "Endmill", Some(*dia)) {
                        Some(t) => {
                            let s = half(p, t);
                            if !wall_pass_sweeps_the_bore(p, name, p.tools[&t].dia) {
                                voids.push(format!("run Clear(tool=T{t}, profile={name}, stepover={s}, compress=arcs)"));
                            }
                            voids.push(format!("run Side(tool=T{t}, surface={name}, stepover={s}, compress=arcs)"));
                        }
                        None => out.notes.push(format!(
                            "{name} waits for a tool: no Drill(dia={}) and no Endmill under {} is declared",
                            num(*dia),
                            num(*dia)
                        )),
                    },
                }
                if let Some(cs) = csink {
                    match chamfer_mill(p, cs.angle) {
                        Some(t) => csinks.push((t, name.to_string())),
                        None => out.notes.push(format!("{name}'s countersink waits for a ChamferMill: none at its angle is declared")),
                    }
                }
                if let Some(cb) = cbore {
                    match endmill_of(p, cb.dia) {
                        Some(t) => cbores.push((t, name.to_string())),
                        None => out.notes.push(format!(
                            "{name}'s counterbore waits for an Endmill(dia={}): a plunged seat is the tool's own circle",
                            num(cb.dia)
                        )),
                    }
                }
                push_forms(p, name, chamfer.as_ref().map(|c| c.angle), *edge_round, &mut forms, &mut out.notes);
            }
            Feature::Plane { z } => {
                if *z >= top - 1e-9 {
                    at_top.push(name);
                } else {
                    floors.push((*z, name.to_string(), true));
                }
            }
            Feature::Face { z, .. } => floors.push((*z, name.to_string(), false)),
            Feature::ProfilePocket { chamfer, edge_round, .. } => {
                match endmill {
                    Some(t) => {
                        let s = half(p, t);
                        voids.push(format!("run Clear(tool=T{t}, profile={name}, stepover={s})"));
                        voids.push(format!("run Side(tool=T{t}, surface={name})"));
                    }
                    None => out.notes.push(no_endmill(name)),
                }
                push_forms(p, name, chamfer.as_ref().map(|c| c.angle), *edge_round, &mut forms, &mut out.notes);
            }
            Feature::ContourPocket { .. } => match endmill {
                Some(t) => voids.push(format!("run Side(tool=T{t}, surface={name})")),
                None => out.notes.push(no_endmill(name)),
            },
            Feature::ProfileBoss { chamfer, edge_round, .. } => {
                match endmill {
                    Some(t) => {
                        let s = half(p, t);
                        if crate::stock::host_of(p, name).is_none() {
                            standing.push(format!("run Clear(tool=T{t}, profile={name}, stepover={s}, stepdown={s})"));
                        }
                        standing.push(format!("run Side(tool=T{t}, surface={name})"));
                    }
                    None => out.notes.push(no_endmill(name)),
                }
                push_forms(p, name, chamfer.as_ref().map(|c| c.angle), *edge_round, &mut forms, &mut out.notes);
            }
            Feature::CylinderBoss { dia, x, y, chamfer, edge_round, .. } => {
                let ring = rim_ring(p, *x, *y, *dia);
                if ring.is_some_and(|r| r <= 1e-6) {
                    at_rim.push(name);
                } else {
                    match endmill {
                        Some(t) => {
                            let s = half(p, t);
                            // The wall pass runs the tool's flank on the wall and
                            // sweeps a tool's width outside it: a boss that close to
                            // the blank's edge needs nothing cleared around it.
                            let swept = ring.is_some_and(|r| r <= p.tools[&t].dia);
                            if crate::stock::host_of(p, name).is_none() && !swept {
                                standing.push(format!("run Clear(tool=T{t}, profile={name}, stepover={s}, stepdown={s})"));
                            }
                            standing.push(format!("run Side(tool=T{t}, surface={name}, stepover={s}, compress=arcs)"));
                        }
                        None => out.notes.push(no_endmill(name)),
                    }
                }
                push_forms(p, name, chamfer.as_ref().map(|c| c.angle), *edge_round, &mut forms, &mut out.notes);
            }
            Feature::ContourBoss { .. } => match endmill {
                Some(t) => {
                    let s = half(p, t);
                    if crate::stock::host_of(p, name).is_none() {
                        standing.push(format!("run Clear(tool=T{t}, profile={name}, stepover={s}, stepdown={s})"));
                    }
                    standing.push(format!("run Side(tool=T{t}, surface={name})"));
                }
                None => out.notes.push(no_endmill(name)),
            },
            Feature::Sphere { .. } | Feature::Cone { .. } | Feature::Torus { .. } | Feature::SplineRevolve { .. } => {
                match (endmill, ballmill) {
                    (Some(e), Some(b)) => {
                        if crate::stock::host_of(p, name).is_none() {
                            let s = half(p, e);
                            standing.push(format!("run Clear(tool=T{e}, profile={name}, stepover={s}, stepdown={s})"));
                        }
                        standing.push(format!("run Copy(tool=T{b}, surface={name}, stepover={})", half(p, b)));
                    }
                    (None, _) => out.notes.push(no_endmill(name)),
                    (_, None) => out.notes.push(no_ballmill(name)),
                }
            }
            Feature::Mesh { .. } | Feature::SplineRevolveFunnel { .. } | Feature::NurbsPatch { .. } => match ballmill {
                Some(b) => standing.push(format!("run Copy(tool=T{b}, surface={name}, stepover={})", half(p, b))),
                None => out.notes.push(no_ballmill(name)),
            },
            Feature::TiltedBore { .. } => {
                out.notes.push(format!("{name} is a TiltedBore: cut on another setup, declared to be built against, nothing to run"));
            }
        }
    }

    // Floors from the highest down: the Clear on the upper level cuts the
    // material the lower one stands under. Planes at one height are one
    // floor — a Clear on a Plane works the blank's whole plan there
    // (`region::of`), so the second has nothing left — and the first
    // declared leads.
    floors.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("a floor's height is finite"));
    let mut led: Vec<(f64, String, bool, Vec<String>)> = Vec::new();
    for (z, name, is_plane) in &floors {
        if *is_plane {
            if let Some(leader) = led.iter_mut().find(|(lz, _, lp, _)| *lp && (lz - z).abs() <= 1e-6) {
                leader.3.push(name.clone());
                continue;
            }
        }
        led.push((*z, name.clone(), *is_plane, Vec::new()));
    }
    for (_, leader, _, followers) in &led {
        if !followers.is_empty() {
            let (verb, whom) = if followers.len() == 1 { ("lies", "both") } else { ("lie", "them all") };
            out.notes.push(format!("{} {verb} at {leader}'s height: {leader}'s Clear is the one pass over {whom}", followers.join(", ")));
        }
    }
    // The top face leads: the highest Plane under the blank's top that
    // nothing stands above is the first cut, the face every bore placed by
    // z= and every boss is measured from once the allowance is gone. A
    // Plane at the blank's top, or anything standing higher, says the part's
    // top is already there and the highest floor is a step, not the face.
    let highest_standing = pending
        .iter()
        .filter(|(n, _)| !floors.iter().any(|(_, f, _)| f == n))
        .filter_map(|(_, f)| crate::stock::standing_top(p, f))
        .fold(f64::NEG_INFINITY, f64::max);
    // A Clear on a Plane works the blank's whole plan: over a round blank its
    // rings are circles, written as arcs.
    let disc = if matches!(p.stock.shape, parse::StockShape::Cylinder { .. }) { ", compress=arcs" } else { "" };
    let mut lead_line = None;
    if let Some((z, name, true, _)) = led.first() {
        if *z >= highest_standing - 1e-6 {
            if let Some(t) = endmill {
                lead_line = Some(format!("run Clear(tool=T{t}, profile={name}, stepover={}{disc})", half(p, t)));
            }
        }
    }
    let mut floor_lines = Vec::new();
    for (_, name, is_plane, _) in &led {
        match endmill {
            Some(t) => {
                let line = format!("run Clear(tool=T{t}, profile={name}, stepover={}{})", half(p, t), if *is_plane { disc } else { "" });
                if lead_line.as_deref() != Some(line.as_str()) {
                    floor_lines.push(line);
                }
            }
            None => out.notes.push(no_endmill(name)),
        }
    }

    if !at_rim.is_empty() {
        let (verb, pronoun) = if at_rim.len() == 1 { ("stands", "it") } else { ("stand", "them") };
        out.notes.push(format!(
            "{} {verb} at the blank's rim: nothing to cut around {pronoun} until the blank carries an allowance outside",
            at_rim.join(", ")
        ));
    }
    if !at_top.is_empty() {
        let (verb, pronoun) = if at_top.len() == 1 { ("lies", "it") } else { ("lie", "them") };
        out.notes.push(format!(
            "{} {verb} at the blank's top: nothing to cut until the blank carries an allowance above {pronoun}",
            at_top.join(", ")
        ));
    }
    if !given.is_empty() {
        let (verb, pronoun) = if given.len() == 1 { ("is", "its") } else { ("are", "their") };
        out.notes.push(format!(
            "{} {verb} given: cut on the other setup and read as air here, so only {pronoun} mouth chamfer or round is this setup's",
            given.join(", ")
        ));
    }
    if let (false, Some(gf)) = (already_air.is_empty(), &p.given_from) {
        let (verb, pronoun) = if already_air.len() == 1 { ("lies", "its") } else { ("lie", "their") };
        out.notes.push(format!(
            "{} {verb} in the earlier program's air (given from {} up={}): written as given, not cut again, so only {pronoun} mouth chamfer or round is this setup's",
            already_air.join(", "),
            gf.project,
            gf.up.word()
        ));
    }
    out.lines.extend(given_lines);
    out.lines.extend(lead_line);
    out.lines.extend(point_lines("Form", &drills));
    out.lines.extend(point_lines("Form", &csinks));
    out.lines.extend(point_lines("End", &cbores));
    out.lines.extend(floor_lines);
    out.lines.extend(voids);
    out.lines.extend(standing);
    out.lines.extend(forms);
    out
}

/// How far the blank stands outside a round boss's wall, by the declared
/// stock: the ring a wall pass has to take, zero when the wall is the
/// blank's own rim. `None` on a Profile blank, whose edge is a loop this
/// rule does not read.
fn rim_ring(p: &Part, x: f64, y: f64, dia: f64) -> Option<f64> {
    let s = &p.stock;
    let r = dia / 2.0;
    match s.shape {
        // The parser keeps a round blank's low corner in `x`, `y`; its axis is half a diameter in.
        parse::StockShape::Cylinder { dia: d } => Some(d / 2.0 - (((x - s.x - d / 2.0).powi(2) + (y - s.y - d / 2.0).powi(2)).sqrt() + r)),
        parse::StockShape::Box => Some([x - r - s.x, s.x + s.w - (x + r), y - r - s.y, s.y + s.l - (y + r)].into_iter().fold(f64::INFINITY, f64::min)),
        _ => None,
    }
}

/// Does a wall pass with a tool `d` wide sweep bore `name` out by itself?
/// The pass runs the tool's flank on the wall, so it covers the ring from
/// the wall in to `R - d` of the centre. Nothing is left in the middle when
/// that reaches the centre, or when a hole through the bore's floor, as
/// deep or deeper, holds the disc that remains: the counterbore over a
/// drilled hole, which is the common case.
fn wall_pass_sweeps_the_bore(p: &Part, name: &str, d: f64) -> bool {
    let Some(Feature::CylinderBore { dia, extent, x, y, z, .. }) = p.features.get(name) else { return false };
    let left = dia / 2.0 - d;
    if left <= 1e-9 {
        return true;
    }
    let top = p.stock.z;
    let bottom = z.unwrap_or(top) - extent;
    p.features.iter().any(|(other, f)| {
        other != name
            && match f {
                Feature::CylinderBore { dia: d2, extent: e2, x: x2, y: y2, z: z2, .. } => {
                    let dist = (x - x2).phypot(y - y2);
                    dist + left <= d2 / 2.0 + 1e-6 && z2.unwrap_or(top) - e2 <= bottom + 1e-6
                }
                _ => false,
            }
    })
}

/// The chamfer's and the round's Form lines, or the note that the tool for
/// one is not declared.
fn push_forms(p: &Part, name: &str, chamfer: Option<Option<f64>>, round: Option<f64>, forms: &mut Vec<String>, notes: &mut Vec<String>) {
    if let Some(angle) = chamfer {
        match chamfer_mill(p, angle) {
            Some(t) => forms.push(format!("run Form(tool=T{t}, surface={name})")),
            None => notes.push(match angle {
                Some(a) => format!("{name}'s chamfer waits for a ChamferMill(angle={}): none is declared", num(a)),
                None => format!("{name}'s chamfer waits for a ChamferMill: none is declared"),
            }),
        }
    }
    if let Some(r) = round {
        match rounder_of(p, r) {
            Some(t) => forms.push(format!("run Form(tool=T{t}, surface={name})")),
            None => notes.push(format!("{name}'s edge round waits for a CornerRounder(radius={}): none is declared", num(r))),
        }
    }
}

/// The schedule, judged: every proposed line is parsed with the project
/// and the lines before it; one the parser refuses becomes a note with the
/// parser's sentence. `main_src` is the project's main text, `geom_src`
/// its `geometry.part` when it has one, `base` the folder relative paths
/// resolve against, the three the project loader takes.
pub fn derive_schedule(p: &Part, main_src: &str, geom_src: Option<&str>, base: Option<&Path>) -> Schedule {
    let proposed = propose(p);
    let mut out = Schedule { lines: Vec::new(), notes: proposed.notes };
    let mut text = main_src.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    for line in proposed.lines {
        let candidate = format!("{text}{line}\n");
        match parse::parse_project_with_base(&candidate, geom_src, base) {
            Ok(_) => {
                text = candidate;
                out.lines.push(line);
            }
            Err(e) => out.notes.push(format!("not written, `{line}`: {}", strip_location(&e))),
        }
    }
    out
}

/// The parser's sentence without the `file:line:` the candidate text owns.
fn strip_location(e: &str) -> String {
    let s = e.trim();
    // "main.part:12: ..." / "line 12: ..." prefixes point into text the
    // reader never sees; the sentence after them is the refusal.
    let mut rest = s;
    for _ in 0..2 {
        if let Some(i) = rest.find(": ") {
            let head = &rest[..i];
            if head.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == ' ' || c == '-') && head.chars().any(|c| c.is_ascii_digit()) {
                rest = &rest[i + 2..];
                continue;
            }
        }
        break;
    }
    rest.to_string()
}

/// The schedule as `.part` text: a comment saying where the lines came
/// from and how the tools were picked, the lines, then the notes as
/// comments. Empty when there is nothing to write and nothing to say.
pub fn render(s: &Schedule) -> String {
    if s.lines.is_empty() && s.notes.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    if !s.lines.is_empty() {
        out.push_str("# scheduled by nacs --schedule: one run per feature no run named, in shop order\n");
        out.push_str("# (a void the earlier program's air already holds is written as given instead);\n");
        out.push_str("# drills by hole diameter, the largest endmill otherwise, the chamfer mill at the\n");
        out.push_str("# loop's angle; stepover and stepdown half the tool; a ring round a round wall or\n");
        out.push_str("# over a round blank is an arc (compress=arcs). Every number is yours to edit.\n");
        for l in &s.lines {
            out.push_str(l);
            out.push('\n');
        }
    }
    for n in &s.notes {
        out.push_str("# not scheduled: ");
        out.push_str(n);
        out.push('\n');
    }
    out
}

/// The schedule of a project on disk, from its own files: `main.part` and
/// the `geometry.part` beside it when there is one. The window and the CLI
/// both read the project this way.
pub fn for_project(p: &Part) -> Result<Schedule, String> {
    let main = p.sources.first().ok_or_else(|| "this project has no file on disk".to_string())?;
    let main_src = std::fs::read_to_string(main).map_err(|e| format!("cannot read {}: {e}", main.display()))?;
    let geom_src = p
        .sources
        .get(1)
        .filter(|g| g.file_name().is_some_and(|n| n == "geometry.part"))
        .and_then(|g| std::fs::read_to_string(g).ok());
    let base = main.parent();
    Ok(derive_schedule(p, &main_src, geom_src.as_deref(), base))
}

/// `nacs --schedule`'s page: the rendered schedule, or one comment line
/// when every declared feature already has its run.
pub fn report(p: &Part) -> Result<String, String> {
    let s = for_project(p)?;
    let text = render(&s);
    if text.is_empty() {
        return Ok("# nothing to schedule: every declared feature is named by a run\n".to_string());
    }
    Ok(text)
}
