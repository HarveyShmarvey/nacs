use crate::math::Portable as _;
// ---- the cut-time model: how long the program runs, stated as a floor ----
//
// Phase 16 slice 1 (DECISIONS.md → "Phase 16 opens: the clock-and-address
// DRAFT…", forks 1–2). The model is deliberately what its honesty sentence
// says and nothing more: **path length over commanded feed** for cutting
// moves, per-axis distance over declared rapid rate for rapids (the slowest
// axis dominates — rapid axes run to their own caps), arc blocks paying
// their true length (r·θ off the I/J center, a full circle 2πr). No
// acceleration, no corner slowdown, no tool-change time; under control comp
// the controller's path differs from the text within a tool radius at
// corners. Every number is a floor on reality and the report's header says
// so.
//
// **The subject is the emitted program text** — the lines the controller
// reads — walked through the one dialect reader (`gcode::apply_move`, which
// carries the modal F for exactly this consumer; `gcode::arc_ij` for the
// arc words). Each line's time lands in the op whose `OpEmission.lines`
// range holds it; what no op claims (tool changes, the setup block, the
// final retract) is the named between-ops bucket, never silently dropped.
//
// **Undeclared rapid rates get the Phase 9 voice.** The rapid DISTANCE is
// measurable from the text alone and always stated; the rapid TIME needs
// the machine file's `rapid_x`/`rapid_y`/`rapid_z`, so a program whose
// machine declares none (or moves an axis whose rate is missing) has its
// rapid share DECLINED by name — and the total declines with it, because a
// total that omitted the rapids would wear the look of a complete number.
// Cutting time is exact either way.

use crate::gcode;
use crate::parse::Part;

/// One row of the report: an op's share of the program's time, split into
/// its two kinds of motion. Distances in mm, times in seconds.
pub struct TimeRow {
    /// `None` is the between-ops bucket — boilerplate no op claims.
    pub op_idx: Option<usize>,
    /// The op's one-line identity, prebuilt so the renderer restates
    /// nothing: "op 3 End on 'TopFace' (T5)".
    pub title: String,
    pub cutting_mm: f64,
    pub cutting_s: f64,
    pub rapid_mm: f64,
    pub rapid_s: f64,
    /// True when some rapid in this row moved an axis whose rate is
    /// undeclared: `rapid_s` then under-states and the renderer declines
    /// the row's rapid time by name instead of printing it.
    pub rapid_declined: bool,
}

/// The whole program's clock: per-op rows in run order, the between-ops
/// bucket last.
pub struct TimeReport {
    pub rows: Vec<TimeRow>,
}

impl TimeReport {
    pub fn cutting_s(&self) -> f64 {
        self.rows.iter().map(|r| r.cutting_s).sum()
    }
    pub fn cutting_mm(&self) -> f64 {
        self.rows.iter().map(|r| r.cutting_mm).sum()
    }
    pub fn rapid_mm(&self) -> f64 {
        self.rows.iter().map(|r| r.rapid_mm).sum()
    }
    pub fn rapid_s(&self) -> f64 {
        self.rows.iter().map(|r| r.rapid_s).sum()
    }
    /// Any row's rapids declined — the total then declines too.
    pub fn declined(&self) -> bool {
        self.rows.iter().any(|r| r.rapid_declined)
    }
}

/// Walk the emitted program and price every motion line. Fails exactly when
/// emission fails — a rejected program has no clock.
pub fn time_report(p: &Part) -> Result<TimeReport, String> {
    let (text, emissions) = gcode::program_with_moves(p)?;
    time_report_of(p, &text, &emissions)
}

/// The clock of a program already emitted — for a caller that has the
/// emission in hand and would otherwise emit it twice (`compare`'s rows).
pub fn time_report_of(p: &Part, text: &str, emissions: &[gcode::OpEmission]) -> Result<TimeReport, String> {
    let (text, emissions) = (text.to_string(), emissions);
    let rates = [
        p.machine.as_ref().and_then(|m| m.rapid_x),
        p.machine.as_ref().and_then(|m| m.rapid_y),
        p.machine.as_ref().and_then(|m| m.rapid_z),
    ];

    let mut rows: Vec<TimeRow> = p
        .operations
        .iter()
        .enumerate()
        .map(|(i, op)| TimeRow {
            op_idx: Some(i),
            title: format!("op [{}] {:?} on '{}' (T{})", i, op.op_type, op.feature_name, op.tool_num),
            cutting_mm: 0.0,
            cutting_s: 0.0,
            rapid_mm: 0.0,
            rapid_s: 0.0,
            rapid_declined: false,
        })
        .collect();
    let between = rows.len();
    rows.push(TimeRow {
        op_idx: None,
        title: "between ops (tool changes, setup, the final retract)".into(),
        cutting_mm: 0.0,
        cutting_s: 0.0,
        rapid_mm: 0.0,
        rapid_s: 0.0,
        rapid_declined: false,
    });

    let mut m = gcode::Motion { rapid: true, x: None, y: None, z: None, f: None };
    for (lineno, line) in text.lines().enumerate() {
        let prev = (m.x, m.y, m.z);
        if !gcode::apply_move(line, &mut m) {
            continue;
        }
        // Distance per axis from the last ESTABLISHED position; a word that
        // first establishes an axis is a claim about where the machine is,
        // not a commanded travel, so it contributes zero — the program's
        // opening block, priced honestly.
        let d = [
            delta(prev.0, m.x),
            delta(prev.1, m.y),
            delta(prev.2, m.z),
        ];
        let chord = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();

        let row = emissions
            .iter()
            .find(|em| em.owns_line(lineno))
            .map(|em| em.op_idx)
            .unwrap_or(between);
        let row = &mut rows[row];

        if m.rapid {
            row.rapid_mm += chord;
            // The slowest axis dominates: each rapid axis runs to its own
            // cap, so the block takes as long as its longest component at
            // that component's rate. An axis moved without a declared rate
            // declines the row — never a guessed traverse.
            let mut secs: f64 = 0.0;
            let mut declined = false;
            for (dist, rate) in d.iter().zip(rates.iter()) {
                if dist.abs() <= 0.0 {
                    continue;
                }
                match rate {
                    Some(v) => secs = secs.max(dist.abs() / v * 60.0),
                    None => declined = true,
                }
            }
            if declined {
                row.rapid_declined = true;
            } else {
                row.rapid_s += secs;
            }
        } else {
            // An arc block's true length: r·θ off the center the I/J words
            // carry (center-relative from the start point), a full circle
            // 2πr — the chord alone would price a `compress=arcs` ring at
            // zero.
            let len = match (gcode::arc_ij(line), prev.0, prev.1) {
                (Some((cw, i, j)), Some(px), Some(py)) => {
                    arc_len(px, py, i, j, m.x.unwrap_or(px), m.y.unwrap_or(py), cw)
                }
                _ => chord,
            };
            row.cutting_mm += len;
            // The emitters state F on every cutting block's family and the
            // word is modal in between; a cutting move before any F would
            // be a dialect bug, priced at zero rather than invented.
            if let Some(f) = m.f {
                if f > 0.0 {
                    row.cutting_s += len / f * 60.0;
                }
            }
        }
    }
    Ok(TimeReport { rows })
}

fn delta(prev: Option<f64>, now: Option<f64>) -> f64 {
    match (prev, now) {
        (Some(a), Some(b)) => b - a,
        _ => 0.0,
    }
}

/// Arc length from start, center-relative I/J, and endpoint: r·θ with the
/// sweep read in the commanded direction, and start == end the full circle
/// (the `compress=arcs` ring's one-block spelling).
pub fn arc_len(px: f64, py: f64, i: f64, j: f64, x: f64, y: f64, cw: bool) -> f64 {
    let (cx, cy) = (px + i, py + j);
    let r = (i * i + j * j).sqrt();
    if r <= 0.0 {
        return 0.0;
    }
    let a0 = (py - cy).patan2(px - cx);
    let a1 = (y - cy).patan2(x - cx);
    let mut sweep = if cw { a0 - a1 } else { a1 - a0 };
    while sweep <= 1e-9 {
        sweep += std::f64::consts::TAU;
    }
    r * sweep
}

/// The report's text — header (the honesty sentence), one line per row that
/// moved, the totals. Seconds at one decimal, millimetres at one: a clock,
/// not a coordinate.
pub fn render(r: &TimeReport) -> String {
    let mut out = String::new();
    out.push_str(
        "cut time — path length over commanded feed: no acceleration, no corner slowdown, no tool-change time; under control comp the controller's path differs within a tool radius at corners. A floor on reality, not a promise.\n",
    );
    for row in &r.rows {
        if row.cutting_mm <= 0.0 && row.rapid_mm <= 0.0 {
            continue;
        }
        let rapid = if row.rapid_declined {
            format!(
                "rapids {:.1}mm, time declined — an axis moved whose rate the machine file does not declare (rapid_x / rapid_y / rapid_z)",
                row.rapid_mm
            )
        } else {
            format!("rapids {:.1}s over {:.1}mm", row.rapid_s, row.rapid_mm)
        };
        out.push_str(&format!(
            "{}: cutting {:.1}s over {:.1}mm, {}\n",
            row.title, row.cutting_s, row.cutting_mm, rapid
        ));
    }
    out.push_str(&format!("cutting total: {:.1}s over {:.1}mm\n", r.cutting_s(), r.cutting_mm()));
    if r.declined() {
        out.push_str(&format!(
            "total: declined — {:.1}mm of rapids are uncounted, and a total that omitted them would wear the look of a complete number. Declare the machine file's rapid rates to count them\n",
            r.rapid_mm()
        ));
    } else {
        out.push_str(&format!("total: {:.1}s\n", r.cutting_s() + r.rapid_s()));
    }
    out
}
