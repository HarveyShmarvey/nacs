// Phase 17, lane B, slice 1 — the measurement (`DECISIONS.md` → "Phase 17
// opens…", fork B1). The editing workspace either leans on the dirty-
// tracking DAG (T5) or accepts full re-emission per edit, and the DRAFT
// says: measure before machinery. This harness prices the WINDOW's
// rebuild, not the terminal's — everything `frontend::build` derives
// synchronously from a loaded project (parse, the check pipeline, the
// finding rows, the op paths, the feature edges, the removal boundaries,
// the outline, the program view, the fixture boundaries), and then the
// two bakes the window runs on worker threads (the swept-stock surface at
// the cursor and the rest scan at the cursor), both priced at the cursor
// that costs most: every op run.
//
// It is `#[ignore]` because it is a measurement, not a pin — its numbers
// live in the ruling entry, and a timing assertion would be a flaky test
// with a number in it. Run it with:
//
//     cargo test --release --test rebuild_cost -- --ignored --nocapture
//
// Everything walked is std-only view-model code; the harness never opens
// a window.

use std::time::{Duration, Instant};

use nacs::{check_result, outline, parse, theme::Theme, view};

struct Row {
    name: String,
    ops: usize,
    /// Parse + the check pipeline + the synchronous view derivations —
    /// the part of a reload the window waits for before it can draw the
    /// program document, the findings and the paths.
    foreground: Duration,
    /// The swept-stock surface with every op run (`None` = refused or
    /// declined; the window shows the sentence instead).
    sim: Option<Duration>,
    /// The rest scan with every op run.
    rest: Option<Duration>,
    note: &'static str,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t0 = Instant::now();
    let v = f();
    (v, t0.elapsed())
}

fn rebuild(path: &str) -> Row {
    let name = path.trim_end_matches(".cam").to_string();
    let t0 = Instant::now();
    let part = match parse::load_project(path) {
        Ok(p) => p,
        Err(_) => {
            return Row { name, ops: 0, foreground: t0.elapsed(), sim: None, rest: None, note: "does not load" };
        }
    };
    let mut cr = check_result::check_result(&part);
    let theme = Theme::default();
    let (_rows, _annotations) = view::finding_rows(&cr, part.stock.z, &theme);
    let emitted: Vec<_> = cr.emitted_moves.take().unwrap_or_default();
    let paths = view::op_paths(&part, &emitted);
    let edges = view::feature_edges(&part);
    let k = part.operations.len();
    let _removal = view::removal_boundaries(&part, k);
    let _bb = view::edges_aabb(&edges, view::scene_aabb(&part.stock, &paths));
    let _outline = outline::outline(&part);
    let _gcode = view::gcode_view(&part);
    let _fixture = view::fixture_boundaries(&part);
    let foreground = t0.elapsed();
    if cr.refusal.is_some() {
        return Row { name, ops: k, foreground, sim: None, rest: None, note: "refused" };
    }
    let (sim, t_sim) = time(|| view::stock_surface_prefix(&part, &emitted, k, None, None));
    let (rest, t_rest) = time(|| view::rest_cells(&part, &emitted, k));
    let note = match (sim.is_ok(), rest.is_ok()) {
        (true, true) => "",
        (false, true) => "sim declined",
        (true, false) => "rest declined",
        (false, false) => "sim and rest declined",
    };
    Row { name, ops: k, foreground, sim: sim.ok().map(|_| t_sim), rest: rest.ok().map(|_| t_rest), note }
}

fn corpus() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(".")
        .expect("crate root")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".cam"))
        .collect();
    v.sort();
    v
}

#[test]
#[ignore]
fn measure_the_window_rebuild_over_the_corpus() {
    let names = corpus();
    assert!(!names.is_empty(), "run from the crate root, where the *.cam projects live");
    let rows: Vec<Row> = names.iter().map(|n| rebuild(n)).collect();

    println!();
    println!("{:<34} {:>4} {:>11} {:>11} {:>11}  note", "program", "ops", "foreground", "sim(k=n)", "rest(k=n)");
    let fmt = |d: Option<Duration>| d.map_or("—".to_string(), |d| format!("{:.1}ms", ms(d)));
    let mut by_fg: Vec<&Row> = rows.iter().collect();
    by_fg.sort_by(|a, b| b.foreground.cmp(&a.foreground));
    for r in &by_fg {
        println!("{:<34} {:>4} {:>11} {:>11} {:>11}  {}", r.name, r.ops, fmt(Some(r.foreground)), fmt(r.sim), fmt(r.rest), r.note);
    }

    let fg: Vec<f64> = rows.iter().map(|r| ms(r.foreground)).collect();
    let sims: Vec<f64> = rows.iter().filter_map(|r| r.sim.map(ms)).collect();
    let rests: Vec<f64> = rows.iter().filter_map(|r| r.rest.map(ms)).collect();
    let stats = |label: &str, v: &[f64]| {
        if v.is_empty() {
            println!("{label}: none");
            return;
        }
        let mut s = v.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let total: f64 = s.iter().sum();
        println!(
            "{label}: n={} total={:.0}ms max={:.1}ms p90={:.1}ms median={:.1}ms",
            s.len(),
            total,
            s[s.len() - 1],
            s[(s.len() * 9 / 10).min(s.len() - 1)],
            s[s.len() / 2]
        );
    };
    println!();
    stats("foreground (parse+check+view)", &fg);
    stats("sim bake at k=n", &sims);
    stats("rest scan at k=n", &rests);
    let over_frame = rows.iter().filter(|r| ms(r.foreground) > 16.7).count();
    let over_100 = rows.iter().filter(|r| ms(r.foreground) > 100.0).count();
    println!("foreground over one 60Hz frame (16.7ms): {over_frame} of {}; over 100ms: {over_100}", rows.len());
}
