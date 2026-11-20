// Phase 34 slice 2 — the sweep (`DECISIONS.md` → "DRAFT: Phase 34 opens…",
// fork 4A; "Phase 34 slice 2…" for what it found). Every region fixture in
// the corpus runs ONE tool at ONE stepover. This walks each of them again
// under other tools and other stepovers, every pattern, through the real
// emitter, and asks only the gates: a row either refuses by name, or it
// emits a program that leaves no reachable floor uncut. A program the
// emitter hands back has already passed `stock::check_moves` — a move into
// material that outlives its op is a refusal, not a row.
//
// It is `#[ignore]` because it is a measurement, not a lock: a few thousand
// emissions, a minute and a half in release. What it finds is pinned as an
// ordinary test on the one case that found it, and named in `known` below.
// Run it with:
//
//     cargo test --release --test sweep -- --ignored --nocapture
//
// The edit is a TEXT edit — `view::set_key` on the tool line and the run
// line, the write the window makes — so every case goes through `parse` and
// `validate` like a project somebody typed.

use std::collections::BTreeMap;

use nacs::{compare, parse, view};

const DIAS: [f64; 4] = [4.0, 6.0, 10.0, 16.0];
/// Stepover as a share of the tool's diameter. 0.5 is the ridge law's edge
/// around islands; 0.7 is past it, and asks for the refusal.
const SHARES: [f64; 3] = [0.3, 0.5, 0.7];

/// The project with its region ops alone, their tools at `dia` and their
/// stepovers at `share` of it. Everything else the fixture runs — a wall
/// finish, a drill — is somebody else's gate and is dropped, so a refusal
/// here is about the floor.
fn recut(main: &str, dia: f64, share: f64) -> String {
    let region_op = |l: &str| l.starts_with("run End(") || l.starts_with("run Clear(");
    let tools: Vec<String> = main
        .lines()
        .filter(|l| region_op(l))
        .filter_map(|l| l.split("tool=").nth(1).map(|t| t.split(|c: char| c == ',' || c == ')').next().unwrap().trim().to_string()))
        .collect();
    let mut out = String::new();
    for line in main.lines() {
        let edited = if region_op(line) {
            let l = view::set_key(line, "stepover", Some(&format!("{:.3}", dia * share))).unwrap();
            // A declared concession was sized for the shipped tool.
            view::set_key(&l, "cover", None).unwrap_or(l)
        } else if line.starts_with("run ") {
            continue;
        } else if tools.iter().any(|t| line.starts_with(&format!("tool {t} "))) {
            view::set_key(line, "dia", Some(&format!("{dia:.1}"))).unwrap()
        } else {
            line.to_string()
        };
        out.push_str(&edited);
        out.push('\n');
    }
    out
}

/// What the sweep has already found and filed (`DECISIONS.md` → "Phase 34
/// slice 2…"), each pinned as an ordinary test on the one case that found
/// it. The sweep fails on anything that is NOT one of these — it is a
/// ratchet, not a list of excuses — and says how many of each it still sees,
/// so the day one is fixed its line here reads zero and is deleted.
fn known(dir: &str, dia: f64, _share: f64, pattern: &str, uncut: f64) -> Option<&'static str> {
    match (dir, pattern) {
        ("curved_rails_offset.cam", "spine") if dia == 4.0 && uncut < 0.1 => Some("D: one cell of the spiral under a 4mm tool"),
        _ => None,
    }
}

/// The same for a row its own gate stopped.
/// (C — spine's entry beside the lawn's neighbour — was the one class here,
/// and Phase 35 slice 3 closed it: the table is empty and stays, so the next
/// one has a home.)
fn known_stop(_dir: &str, _dia: f64, _pattern: &str) -> Option<&'static str> {
    None
}

#[derive(Default)]
struct Tally {
    cases: usize,
    rows: usize,
    cut: usize,
    declined: usize,
    refused: BTreeMap<String, usize>,
    findings: Vec<String>,
    /// Found before, filed, pinned: by class.
    known: BTreeMap<&'static str, usize>,
}

/// A refusal's sentence with its numbers and names taken out, so the tally
/// groups by WHAT was refused.
fn kind_of(why: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in why.chars() {
        match c {
            '\'' => {
                quoted = !quoted;
                if quoted {
                    out.push('…');
                }
            }
            _ if quoted => {}
            c if c.is_ascii_digit() || c == '.' && out.ends_with('#') => {
                if !out.ends_with('#') {
                    out.push('#');
                }
            }
            c => out.push(c),
        }
    }
    out.chars().take(110).collect()
}

#[test]
#[ignore]
fn sweep_every_region_fixture_over_tools_and_stepovers() {
    let mut dirs: Vec<String> = std::fs::read_dir(".")
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
        .filter(|n| n.ends_with(".cam"))
        .collect();
    dirs.sort();
    let mut t = Tally::default();
    let mut swept = 0;
    for dir in &dirs {
        let main = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
        // One fixture per floor: a sibling that differs by its pin is the
        // same sweep, because the report walks every pattern anyway. Since
        // Phase 35 slice 4 a floor's fixtures are all pinned and named for
        // the pin, so its representative is the unpinned one where there is
        // one and the `_offset` one where there is not. (Skipping every
        // pinned fixture, as this did, would have dropped eleven floors the
        // day they were pinned, and the sweep would have stayed green.)
        let other_pin = main.lines().any(|l| l.contains("pattern=") && !l.contains("pattern=offset") && !l.contains("pattern=raster"));
        if !main.lines().any(|l| l.starts_with("run End(") || l.starts_with("run Clear(")) || other_pin || dir.ends_with("_raster.cam") || main.contains("strategy=") {
            continue;
        }
        let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
        swept += 1;
        for dia in DIAS {
            for share in SHARES {
                let case = format!("{dir} dia {dia} stepover {:.0}%", share * 100.0);
                let src = recut(&main, dia, share);
                let part = match parse::parse_project_with_base(&src, geom.as_deref(), Some(std::path::Path::new(dir))) {
                    Ok(p) => p,
                    Err(why) => {
                        // Refused at validate: the whole project, by name.
                        *t.refused.entry(format!("validate: {}", kind_of(&why))).or_default() += 1;
                        t.cases += 1;
                        continue;
                    }
                };
                t.cases += 1;
                let reports = match std::panic::catch_unwind(|| compare::op_reports(&part)) {
                    Ok(r) => r,
                    Err(_) => {
                        t.findings.push(format!("{case}: PANICKED"));
                        continue;
                    }
                };
                for r in &reports {
                    for row in &r.rows {
                        t.rows += 1;
                        match &row.outcome {
                            Err(why) if why.trim().is_empty() => t.findings.push(format!("{case} {} {}: refused with no sentence", r.title, row.pattern.word())),
                            Err(why) => {
                                // The emitter planned a move and its own gate
                                // stopped it: a refusal by name, and a plan
                                // that should not have been made.
                                if why.contains("drives the tool into material") {
                                    match known_stop(dir, dia, row.pattern.word()) {
                                        Some(class) => *t.known.entry(class).or_default() += 1,
                                        None => t.findings.push(format!("{case} {} {}: planned a move check_moves stopped", r.title, row.pattern.word())),
                                    }
                                }
                                *t.refused.entry(kind_of(why)).or_default() += 1
                            }
                            Ok(st) => match st.uncut_mm2 {
                                None => t.declined += 1,
                                Some(a) if a == 0.0 => t.cut += 1,
                                Some(a) => match known(dir, dia, share, row.pattern.word(), a) {
                                    Some(class) => *t.known.entry(class).or_default() += 1,
                                    None => t.findings.push(format!("{case} {} {}: {a:.1}mm² of reachable floor uncut", r.title, row.pattern.word())),
                                },
                            },
                        }
                    }
                }
            }
        }
        eprintln!("swept {dir}: {} rows so far, {} findings", t.rows, t.findings.len());
    }
    println!("\nthe sweep: {swept} fixtures x {} tools x {} stepovers = {} cases, {} rows", DIAS.len(), SHARES.len(), t.cases, t.rows);
    println!("  cut, nothing uncut: {}", t.cut);
    println!("  cut, the stock model declines to say: {}", t.declined);
    println!("  refused by name: {}", t.refused.values().sum::<usize>());
    let mut kinds: Vec<(&String, &usize)> = t.refused.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1));
    for (kind, n) in kinds {
        println!("    {n:>5}  {kind}");
    }
    println!("  found before, filed and pinned: {}", t.known.values().sum::<usize>());
    for (class, n) in &t.known {
        println!("    {n:>5}  {class}");
    }
    println!("  NEW findings: {}", t.findings.len());
    for f in &t.findings {
        println!("    {f}");
    }
    assert!(t.findings.is_empty(), "{} rows cut and left floor, were stopped by their own gate, or fell over — and none of them is on file", t.findings.len());
}

// ---- Phase 38 slice 3: spine's comb, priced at the op's own removal rate ----------
// (`DECISIONS.md` → "DRAFT: Phase 38 opens…", fork 2A; "Phase 38 slice 3…".)
// Spine chooses its comb by length and lifts and cannot see load. The DRAFT
// asked whether a price WITH load in it would choose differently, and
// measured that it would not, by more than a few percent, on any floor in
// the corpus. This is that measurement as a pin: for every op that cuts
// spine today, every comb spine considers — each direction, four phases,
// both ways — is planned, its moves walked, and priced at the removal rate
// the op's own words declare: a millimetre in a full slot counted as
// `dia / stepover` millimetres, a millimetre past 90° and not in a slot as
// `1.5r / stepover`, the rest as itself, lifts as spine prices them. The
// comb spine picks must price within `SPINE_LOAD_MARGIN` of the best. The
// day a comb exists that slots far less for little clock, this fails, and
// fork 2 reopens with a number.
//
//     cargo test --release --test sweep -- --ignored --nocapture spine_comb

/// How much dearer, at the loaded price, the comb spine picks may be than
/// the cheapest single comb it considered. The DRAFT's number, 3%; measured
/// here at 0.7% at worst (`wrench_spine`, 2026-10-02), so the pin stands
/// four times off the worst it has seen and no further. (A negative row is
/// the chosen comb beating every single comb — it is a cross comb or an
/// anchored phase, which spine also considers and this does not enumerate.)
const SPINE_LOAD_MARGIN: f64 = 0.03;

/// The comb spine chose today, and the lightest comb it could have, by the
/// loaded price — one row per op.
#[test]
#[ignore]
fn sweep_spine_comb_against_the_loaded_price() {
    use nacs::parse::{Feature, OpType, Pattern};
    let mut dirs: Vec<String> = std::fs::read_dir(".").unwrap().filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string())).filter(|n| n.ends_with(".cam")).collect();
    dirs.sort();
    let mut rows: Vec<String> = Vec::new();
    let mut worst: (f64, String) = (0.0, String::new());
    let mut ops = 0;
    for dir in &dirs {
        let main = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
        let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
        let p = parse::parse_project_with_base(&main, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap();
        for (idx, op) in p.operations.iter().enumerate() {
            if parse::derive_pattern(&p, op) != Some(Pattern::Spine) || nacs::rest::is_rest(op) {
                continue;
            }
            let Some(region) = nacs::region::of(&p, op) else { continue };
            let tool = &p.tools[&op.tool_num];
            let (r, stepover) = (tool.dia / 2.0, op.stepover.unwrap());
            // What `pattern_pass` hands spine: a lift priced in millimetres
            // of feed, a ramp `RAMP_DEG` steep over the depth it loses.
            let host = p.features.get(&op.feature_name);
            let pocket = matches!(host, Some(Feature::ProfilePocket { .. }));
            let skin = if pocket { op.stock_to_leave.unwrap_or(0.0) } else { 0.0 };
            let z_cut = region.z;
            let z_from = if pocket && op.op_type == OpType::End { z_cut + parse::derive_floor_left(&p, op) } else { p.stock.z };
            let feed = op.feed.unwrap_or(p.global_feed);
            let plunge = op.plunge.unwrap_or(p.global_plunge);
            let lift = if region.keep_outs().is_empty() { op.rapid_z.unwrap_or(p.global_rapid_z) } else { op.clearance_z.unwrap_or(p.global_clearance_z) };
            let lift_cost = (lift - z_cut).max(0.0) * (1.0 + feed / plunge);
            let ramp_len = ((z_from - z_cut).max(0.0) / nacs::ops::clear::RAMP_DEG.to_radians().tan()).max(2.0 * r);
            let costs = (lift_cost, ramp_len);
            // The walk's material: the stock the op began with, at the floor.
            let before = nacs::stock::remaining_before(&p, idx);
            let Some(bounds) = nacs::engagement::Bounds::of(&before) else { continue };
            let floor_z = region.z + parse::derive_floor_skin(&p, op);
            let material = |q: (f64, f64)| before.material_at(q.0, q.1, floor_z + 0.01);
            let priced = |plan: &nacs::spine::Plan| -> (f64, compare::Load) {
                let moves: Vec<(parse::Pt, parse::Pt)> = plan.chains.iter().flat_map(|c| c.pts.windows(2).map(|w| (w[0], w[1]))).collect();
                let load = compare::load_along(&moves, r, &bounds, &material);
                let slot_rate = tool.dia / stepover;
                let heavy_rate = 1.5 * r / stepover;
                (plan.cost + load.slot_mm * (slot_rate - 1.0) + (load.past_half_mm - load.slot_mm) * (heavy_rate - 1.0), load)
            };
            let chosen = nacs::ops::clear::spine_plan(&p, op, &region, skin, costs, None).unwrap();
            let (chosen_price, chosen_load) = priced(&chosen);
            // The floor spine names its directions by: the region's outer
            // boundary, a Face's inner loops or what stands in it.
            let mut floor = vec![region.outer.clone()];
            match host {
                Some(Feature::Face { loops, .. }) => floor.extend(loops.iter().skip(1).cloned()),
                _ => floor.extend(region.standing_polygons()),
            }
            let mut best: Option<(f64, compare::Load, f64, f64, bool)> = None;
            for angle in nacs::spine::directions(&floor, tool.dia) {
                for phase in [0.0, 0.25, 0.5, 0.75] {
                    for descending in [false, true] {
                        let Ok(plan) = nacs::ops::clear::spine_plan(&p, op, &region, skin, costs, Some((angle, phase, descending))) else { continue };
                        let (price, load) = priced(&plan);
                        if best.as_ref().is_none_or(|b| price < b.0) {
                            best = Some((price, load, angle, phase, descending));
                        }
                    }
                }
            }
            let Some((best_price, best_load, angle, phase, descending)) = best else { continue };
            ops += 1;
            let over = chosen_price / best_price - 1.0;
            rows.push(format!(
                "{:<34} op {idx} {:<12} chosen {:>6.0} (slot {:>5.0}mm) at {:>3.0}deg; lightest {:>6.0} (slot {:>5.0}mm) at {:>3.0}deg ph {:.2} {} — {:+.1}%",
                dir, op.feature_name, chosen_price, chosen_load.slot_mm, chosen.angle_deg, best_price, best_load.slot_mm, angle, phase, if descending { "down" } else { "up" }, over * 100.0
            ));
            if over > worst.0 {
                worst = (over, format!("{dir} op {idx}"));
            }
        }
    }
    println!("\nspine's comb against the loaded price, {ops} ops:");
    for r in &rows {
        println!("  {r}");
    }
    println!("  worst: {:+.1}% on {}", worst.0 * 100.0, worst.1);
    assert!(worst.0 <= SPINE_LOAD_MARGIN, "a comb spine considers beats the one it picks by {:.1}% at the loaded price, on {} — fork 2 reopens with this number", worst.0 * 100.0, worst.1);
}
