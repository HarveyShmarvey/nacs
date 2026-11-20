use crate::math::Portable as _;
// G-code writer/formatting: the shared number format and the program
// assembly that stitches the ops together with the surrounding boilerplate
// (program start/end, tool change, spindle, coolant).

use crate::ops::{clear::clear, drill::drill, floor::floor, finish::finish};
use crate::parse::{
    derive_d, derive_h, derive_t, Machine, OpType, Operation, OutputUnits, Part, SpindleWord,
    MM_PER_INCH,
};

// The one number format, split by WORD CLASS (DECISIONS.md "K1, the
// machine…", fork 2A): coordinates, feeds, and spindle speeds are three
// different words to a controller, even though on the UNDECLARED machine all
// three still spell `{:.3}` — which is why every existing golden is
// byte-identical. The classes exist so later machine fields (inch output,
// extended precision — T1) land as per-class facts on formatters that are
// already there, one formatting story instead of a post.
//
// `f()` stays as the coordinate/feed spelling every existing call site uses.
// The day a machine field split the classes arrived (units/decimals,
// DECISIONS.md "K1 slice 1 — the formatting boundary…") and the call sites
// still did not need to name their class, for a reason slice 0 couldn't see:
// the emitted word's LETTER already names it. X/Y/Z/I/J are coordinate
// words, F is a feed word, S is a speed word — so the split lands at the
// formatting boundary (`respell_line`, below), which classifies each word by
// its letter and respells it through the class's output formatter. The op
// emitters keep writing the INTERNAL dialect — mm at 3 decimals, the text
// every certificate reader (check_moves, the voxel stamp builder, the entry
// findings, the toolpath view) parses — and only the program text that
// leaves for the controller is converted, once. One conversion in (the
// parse boundary), one out (here), mm everywhere between.
pub fn f(v: f64) -> String { f_coord(v) }
pub fn f_coord(v: f64) -> String { format!("{:.3}", snap(v)) }
pub fn f_feed(v: f64) -> String { format!("{:.3}", snap(v)) }

/// A value on its way to print, rounded to a millionth of a micron first.
/// A ramp over sixteen equal steps computes Z = -0.5625 give or take the
/// last bit, and that bit chose between `-0.562` and `-0.563`: it differed
/// between a debug and an optimised build of the same source. Snapped, the
/// noise is gone before the three decimals are cut.
pub fn snap(v: f64) -> f64 { (v * 1e9).round() / 1e9 }

/// One output word in a dimension-carrying class: the mm-internal value,
/// spelled in the declared units at the declared decimals. The one home for
/// the mm→inch conversion on the way OUT, mirroring `parse.rs`'s
/// `MM_PER_INCH` scaling on the way in.
fn out_word(v_mm: f64, units: OutputUnits, decimals: usize) -> String {
    let v = match units {
        OutputUnits::Mm => v_mm,
        OutputUnits::Inch => v_mm / MM_PER_INCH,
    };
    format!("{:.*}", decimals, snap(v))
}

/// The coordinate class's OUTPUT spelling under a (possibly undeclared)
/// machine. Identical to `f_coord` for the undeclared machine and for any
/// declared machine whose output format is the default — by construction,
/// since mm at 3 decimals IS the default format.
pub fn f_coord_out(v: f64, machine: Option<&Machine>) -> String {
    match machine {
        Some(m) => out_word(v, m.units, m.coord_decimals),
        None => f_coord(v),
    }
}

/// The feed class's OUTPUT spelling — mm/min internally, the declared
/// units/min at `feed_decimals` on the way out.
pub fn f_feed_out(v: f64, machine: Option<&Machine>) -> String {
    match machine {
        Some(m) => out_word(v, m.units, m.feed_decimals),
        None => f_feed(v),
    }
}

/// Split a leading N-word off a program line: `Some(("N10", rest))` for the
/// canonical spelling this dialect writes (`N` + digits + one space, or a
/// bare `N<digits>` line), `None` otherwise. The reader half of the restart
/// ladder (DECISIONS.md → "N-words…", fork 4A): every reader of emitted text
/// strips through here so the label is invisible to certificates — the K1
/// design's "teach the certificate readers first", paid once. Without this,
/// an N-prefixed TITLE slips past the readers' `starts_with('(')` guards
/// into their word loops, where `split_at(1)` on a word beginning with a
/// non-ASCII character (feature names are free text) panics on the byte
/// boundary — the latent crash the design entry measured.
pub fn split_n_word(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('N')?;
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let after = &rest[digits..];
    if after.is_empty() {
        return Some((line, ""));
    }
    let body = after.strip_prefix(' ')?;
    Some((&line[..1 + digits], body))
}

/// The stripped view: the line as every certificate reads it.
pub fn strip_n_word(line: &str) -> &str {
    split_n_word(line).map_or(line, |(_, body)| body)
}

/// The formatting boundary's respell of one emitted line (DECISIONS.md "K1
/// slice 1 — the formatting boundary…"): each word is classified by its
/// letter and respelled through its class's output formatter — X/Y/Z/I/J
/// through the coordinate class, F through the feed class. `G21` becomes
/// `G20` in the same pass that converts the numbers, so the units word and
/// the words it governs cannot drift apart. Everything else is untouched:
/// S is already final (spelled machine-aware at its source, and an RPM has
/// no length unit), G/M/T/H/D are modes and registers, and comment lines
/// pass through whole — the dimension-carrying comments are either
/// converted at their source (the tool headers) or carry an explicit mm
/// label (the roughing audit trail, the regions residue).
fn respell_line(line: &str, m: &Machine) -> String {
    // An N word is a label, not a dimension: split it off, respell the body,
    // rejoin — which also keeps an N-prefixed title's comment immunity (a
    // feature name that LOOKS like a coordinate word must not respell).
    if let Some((n, body)) = split_n_word(line) {
        return format!("{} {}", n, respell_line(body, m));
    }
    if line.starts_with('(') {
        return line.to_string();
    }
    if line == "G21" {
        return match m.units {
            OutputUnits::Mm => line.to_string(),
            OutputUnits::Inch => "G20".to_string(),
        };
    }
    line.split(' ')
        .map(|word| {
            if word.len() < 2 || !word.is_ascii() {
                return word.to_string();
            }
            let (letter, rest) = word.split_at(1);
            let respelled = |dec: usize| {
                rest.parse::<f64>()
                    .map(|v| format!("{}{}", letter, out_word(v, m.units, dec)))
                    .unwrap_or_else(|_| word.to_string())
            };
            match letter {
                "X" | "Y" | "Z" | "I" | "J" => respelled(m.coord_decimals),
                "F" => respelled(m.feed_decimals),
                _ => word.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// The speed class is the one that reads the declared machine today:
// Fanuc-family controls read S as an integer, and `M03 S3000.000` — the S
// word wearing the coordinates' decimals — was the word in the walked
// dialect most likely to be already wrong on a real controller. `None`
// (the undeclared machine) keeps today's exact spelling, byte for byte.
pub fn f_speed(v: f64, machine: Option<&Machine>) -> String {
    match machine.map(|m| m.spindle_word) {
        Some(SpindleWord::Integer) => format!("{}", v.round() as i64),
        Some(SpindleWord::Decimal) | None => format!("{:.3}", v),
    }
}

/// Title-comment audit trail for a roughing op: states the allowance and how
/// much of it the cusp budget consumed, so the max-not-min decomposition
/// (DECISIONS.md "Stock-to-leave semantics…") is readable in the G-code
/// itself. Empty when the op is a plain finishing pass, so every existing
/// golden is byte-identical.
///
/// Lives here rather than in `ops::finish` because it is a *format*, and the
/// prismatic carriers (`ops::pocket`, `ops::contour`) emit it too.
pub fn stock_suffix(op: &Operation, cusp_budget: f64) -> String {
    match op.stock_to_leave {
        Some(allowance) => format!(
            " [stock_to_leave {:.3}mm, cusp budget {:.3}mm]",
            allowance, cusp_budget
        ),
        None => String::new(),
    }
}

/// Title-comment audit trail for a chip-thinned feed (Phase 8 V1 slice 1,
/// `DECISIONS.md` → "V1 slice 1…"). States the factor and the engagement it was
/// computed from, and says so explicitly when `CHIP_THINNING_CAP` bound — the
/// residue law's precedent: a reader sees the number the engine rejected to
/// exceed rather than silently receiving a clamped one.
///
/// A feed raised by a computation the program text cannot show is exactly the
/// silence this project keeps rejecting. Empty whenever no correction rode, so
/// every golden without one is byte-identical by construction.
pub fn thinning_suffix(op: &Operation) -> String {
    match op.chip_thinning {
        // A factor of exactly 1 is the identity — at or past half immersion
        // there is nothing to say, and saying it anyway would be noise on
        // every heavily-engaged pocket.
        Some(ct) if (ct.factor - 1.0).abs() > 1e-9 => {
            // Per-pass words (Phase 9 lane A, slice 1): the reader sees the
            // factor range the passes actually ride, the slot said out loud,
            // so every F word in the block can be cross-checked against the
            // identity the title implies.
            let per_pass = match &op.pass_feeds {
                Some(feeds) if feeds.len() > 1 => {
                    let base = feeds[0];
                    let factors: Vec<f64> = feeds.iter().map(|f| f / base).collect();
                    let lo = factors[1..].iter().cloned().fold(f64::INFINITY, f64::min);
                    let hi = factors[1..].iter().cloned().fold(0.0, f64::max);
                    format!(
                        "; per pass: x1.000 on the slot, then x{:.3}..x{:.3} over {} passes",
                        lo, hi, feeds.len() - 1
                    )
                }
                _ => String::new(),
            };
            format!(
                " [chip thinning x{:.3} at {:.1}° engagement{}{}]",
                ct.factor,
                ct.engagement.to_degrees(),
                if ct.capped { ", capped" } else { "" },
                per_pass
            )
        }
        _ => String::new(),
    }
}

/// A circular-arc cutting move in the active plane (G17/XY). `cw` picks the
/// direction word — `G02` clockwise, `G03` counter-clockwise — and `i`/`j` are
/// the arc center *relative to the arc start* (the incremental IJ convention,
/// the default for the controllers this targets and the standard CAM post
/// form). `x`/`y` are the absolute endpoint. Uses the same coordinate/feed
/// formatter classes as every other move so arc coordinates round
/// identically. This is the shared
/// arc primitive (DECISIONS.md "Arc emission…"): `ops::contour`'s engine mode is
/// the first consumer; the finish waterline rings are a later one.
pub fn arc(cw: bool, x: f64, y: f64, i: f64, j: f64, feed: f64) -> String {
    format!(
        "G0{} X{} Y{} I{} J{} F{}",
        if cw { 2 } else { 3 },
        f_coord(x), f_coord(y), f_coord(i), f_coord(j), f_feed(feed)
    )
}

/// The arc writer's reader (Phase 16 slice 1): the G02/G03 words of one
/// emitted line — direction and the center-relative I/J — for the one
/// consumer that needs an arc's true length rather than its endpoint (the
/// cut-time model; `apply_move` deliberately reads only the modal endpoint).
/// Lives here so the dialect's writer and reader cannot drift apart, the
/// `Motion` rule. `None` for any line that is not an arc block.
pub fn arc_ij(line: &str) -> Option<(bool, f64, f64)> {
    let line = strip_n_word(line);
    let (mut cw, mut i, mut j) = (None, None, None);
    for word in line.split_whitespace() {
        let (letter, rest) = word.split_at(1);
        match letter {
            "G" => match rest.parse::<f64>().ok().map(|v| v as i32) {
                Some(2) => cw = Some(true),
                Some(3) => cw = Some(false),
                _ => {}
            },
            "I" => i = rest.parse().ok(),
            "J" => j = rest.parse().ok(),
            _ => {}
        }
    }
    Some((cw?, i?, j?))
}

/// One operation's emitted cutting moves, as handed to the voxel stock
/// engine's stamp builder (DECISIONS.md "Phase 5's voxel stock engine…", the
/// emission emitted moves). The lines are the op's OWN moves only — the tool-change
/// boilerplate between ops never travels below the clearance plane, and every
/// op re-states its Z at clearance on entry, so the boilerplate carries no
/// stamp and is deliberately not on the emitted-move list. `op_idx` points back into
/// `Part::operations` so the consumer can read the op's comp mode and tool —
/// under control comp the emitted coordinates are the PART EDGE, not the tool
/// center (the `check_moves` rule), so a control op carries `resolved`: the
/// emitter-authored tool-center motion in the same dialect (DECISIONS.md "the
/// control-comp centerline resolve"), which is what the walker stamps. A
/// control op with no resolved channel keeps the named error.
pub struct OpEmission {
    pub op_idx: usize,
    pub moves: Vec<String>,
    pub resolved: Option<Vec<String>>,
    /// Which lines of the emitted program this op contributed, as a
    /// half-open range — the restart ladder's own anchor (`op_start`
    /// below, "whatever line this op contributes FIRST") through the last
    /// of its moves. So it covers the op's tool change, its bare S or
    /// coolant transition and its title as well as its motion, which is
    /// the same block the `n=` rung labels and the same block a restart
    /// from this op would replay.
    ///
    /// Carried by the EMITTED MOVES because it cannot be recovered from the text
    /// (Phase 13, slice 4 measured both ways it fails: a tool change
    /// between two ops means their moves are not one contiguous run, and
    /// the formatting boundary respells the text while the emitted moves stays in
    /// the internal dialect, so the strings do not even match). It is an
    /// OP→line correspondence; the finer one lives beside it as
    /// `moves_start` (Phase 16 slice 2 — the sentence that once said "its
    /// own item, and this is not half of it" is paid).
    ///
    /// Indices into `text.lines()` and stable under the respell, which
    /// rewrites each line in place and moves none of them.
    pub lines: std::ops::Range<usize>,
    /// The MOVE→line correspondence (Phase 16 slice 2, DECISIONS.md →
    /// "Phase 16 opens: the clock-and-address DRAFT…", fork 3): the op's
    /// k-th move IS program line `moves_start + k`. One number, not a
    /// table, because the assembly lands each op's moves with a single
    /// `g.extend` — contiguity is structural, and the corpus pin holds the
    /// invariant that the move block sits inside `lines`. Stable under the
    /// respell and the N-rung for the same reason `lines` is: both rewrite
    /// lines in place and move none. The block-grain scrub sync (slice 3)
    /// is its consumer; until then the pins are.
    pub moves_start: usize,
}

impl OpEmission {
    /// The program line the op's k-th move occupies.
    pub fn move_line(&self, k: usize) -> usize {
        self.moves_start + k
    }
}

/// An op emitter's two heads: `text` is the program (what stdout gets, byte
/// for byte what it always was), `resolved` is the engine-resolved tool-center
/// motion for a comp=control op — plain moves in the same dialect, written by
/// the same pass that wrote the text, never re-derived from it (DECISIONS.md
/// "the control-comp centerline resolve", fork A). `None` for every op whose
/// text already IS the tool center.
#[derive(Debug)]
pub struct OpMoves {
    pub text: Vec<String>,
    pub resolved: Option<Vec<String>>,
}

impl From<Vec<String>> for OpMoves {
    fn from(text: Vec<String>) -> Self {
        OpMoves { text, resolved: None }
    }
}

/// Modal motion state, in the one dialect this engine emits. Lives beside the
/// writer so the dialect's reader and writer cannot drift apart — the entry
/// restriction (`stock::check_moves`) and the voxel stamp builder (`vox`) both
/// read emitted moves through this one home.
#[derive(Clone, Copy)]
pub struct Motion {
    pub rapid: bool,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    /// The modal F word (Phase 16 slice 1): the commanded feed in force,
    /// carried because the dialect's F is modal — a bare `G01 Y6.000`
    /// inherits the last stated feed. Read by the cut-time model; every
    /// prior reader ignores it, which is why it rides `Motion` rather than
    /// a second walker.
    pub f: Option<f64>,
}

/// Apply one emitted line to the modal state. Returns false for anything that
/// is not motion (comments, tool changes, spindle, coolant).
pub fn apply_move(line: &str, m: &mut Motion) -> bool {
    let line = strip_n_word(line);
    if line.starts_with('(') {
        return false;
    }
    let mut moved = false;
    for word in line.split_whitespace() {
        let (letter, rest) = word.split_at(1);
        let value: f64 = match rest.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        match letter {
            // G43 rides along on a G00 line; only 0-3 are motion modes.
            "G" => match value as i32 {
                0 => { m.rapid = true; moved = true; }
                1 | 2 | 3 => { m.rapid = false; moved = true; }
                _ => {}
            },
            "X" => { m.x = Some(value); moved = true; }
            // Modal feed: recorded, but an F word alone is not motion.
            "F" => { m.f = Some(value); }
            "Y" => { m.y = Some(value); moved = true; }
            "Z" => { m.z = Some(value); moved = true; }
            _ => {}
        }
    }
    moved
}

/// The moves a consumer WALKS for an op: the resolved tool-center channel
/// when the emitter authored one, the text otherwise. One home for the
/// control-comp rule — "walk the resolved channel when it exists; it is the
/// motion the machine makes, the text is the motion the controller is told"
/// (DECISIONS.md "the control-comp centerline resolve"). Every reader of
/// emitted motion (the entry restriction, the voxel stamp builder, the body
/// foul lint, the entry findings, the toolpath view) goes through this
/// function, so the rule cannot drift per consumer.
pub fn resolved_moves(em: &OpEmission) -> &[String] {
    em.resolved.as_deref().unwrap_or(&em.moves)
}

/// Below this plan travel a move is vertical; below this Z change it is
/// horizontal. Purely a classification guard, orders under any emitted
/// coordinate — the writer (`f()`, above) rounds to 1e-3, and keeping the
/// guard beside the writer is exactly this file's charter: the dialect's
/// reader and writer cannot drift apart.
pub(crate) const PATH_EPS: f64 = 1e-9;

/// A G02/G03's geometry, derived from the emitted words: center (absolute),
/// radius, start angle, and SIGNED sweep in radians (+ccw; |sweep| == 2π is
/// the writer's full-circle convention — start == end within the guard).
///
/// One home because a sign error here doesn't fail loudly, it draws (or
/// stamps) the mirrored arc — and the two consumers (the voxel stamp builder
/// and the toolpath view) would each be confidently wrong in their own way.
/// A zero-length radius vector is the CALLER's judgment (against `PATH_EPS`),
/// not silently patched here: `a0`/`sweep` are still well-defined arithmetic
/// (atan2(0,0) = 0) but a zero-radius arc is not a move class anyone accepts.
pub struct ArcGeom {
    pub cx: f64,
    pub cy: f64,
    pub ra: f64,
    pub a0: f64,
    pub sweep: f64,
}

/// Derive an arc's geometry from its start point, end point, and the emitted
/// I/J/cw words (I/J are center-relative-to-start, the convention `arc()`
/// writes).
pub fn arc_geom(fx: f64, fy: f64, tx: f64, ty: f64, i: f64, j: f64, cw: bool) -> ArcGeom {
    let (cx, cy) = (fx + i, fy + j);
    let ra = i.phypot(j);
    let a0 = (fy - cy).patan2(fx - cx);
    let full = (fx - tx).phypot(fy - ty) <= PATH_EPS;
    let sweep = if full {
        if cw { -2.0 * std::f64::consts::PI } else { 2.0 * std::f64::consts::PI }
    } else {
        let a1 = (ty - cy).patan2(tx - cx);
        if cw {
            -((a0 - a1).rem_euclid(2.0 * std::f64::consts::PI))
        } else {
            (a1 - a0).rem_euclid(2.0 * std::f64::consts::PI)
        }
    };
    ArcGeom { cx, cy, ra, a0, sweep }
}

/// An emitted arc as the chords a reader walks: `from`, the points between
/// at the chord tolerance, `to`, every one at `to`'s Z (the dialect's arcs
/// are flat). The one chording every reader of an emitted arc shares — the
/// toolpath view, the pattern report's moves — so a ring written as one
/// full-circle block (`compress=arcs`) reads as the path the tool takes and
/// not as the zero-length move between its equal ends.
pub fn arc_chords(ag: &ArcGeom, from: [f64; 3], to: [f64; 3]) -> Vec<[f64; 3]> {
    // sagitta s = r(1 - cos(θ/2)) <= tol  =>  θ <= 2·acos(1 - tol/r)
    let step = if ag.ra <= crate::CHORD_TOL_MM {
        std::f64::consts::PI // degenerate-small arc: endpoints alone suffice
    } else {
        2.0 * (1.0 - crate::CHORD_TOL_MM / ag.ra).pacos()
    };
    let n = (ag.sweep.abs() / step).ceil().max(1.0) as usize;
    let mut pts = Vec::with_capacity(n + 1);
    pts.push(from);
    for k in 1..n {
        let a = ag.a0 + ag.sweep * (k as f64) / (n as f64);
        pts.push([ag.cx + ag.ra * a.pcos(), ag.cy + ag.ra * a.psin(), to[2]]);
    }
    pts.push(to);
    pts
}

/// The arc words of an emitted line, if it is a G02/G03: (i, j, cw). I/J are
/// the center relative to the arc START (the incremental convention `arc()`
/// writes). Returns None for every non-arc line, so a consumer can key on it
/// without re-stating the dialect.
pub fn arc_words(line: &str) -> Option<(f64, f64, bool)> {
    let line = strip_n_word(line);
    if line.starts_with('(') {
        return None;
    }
    let (mut i, mut j, mut cw) = (None, None, None);
    for word in line.split_whitespace() {
        let (letter, rest) = word.split_at(1);
        let value: f64 = match rest.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        match letter {
            "G" => match value as i32 {
                2 => cw = Some(true),
                3 => cw = Some(false),
                _ => {}
            },
            "I" => i = Some(value),
            "J" => j = Some(value),
            _ => {}
        }
    }
    match (i, j, cw) {
        (Some(i), Some(j), Some(cw)) => Some((i, j, cw)),
        _ => None,
    }
}

pub fn program(p: &Part) -> Result<String, String> {
    program_with_moves(p).map(|(text, _)| text)
}

/// `program()` plus the emission emitted moves: the same text, and each op's emitted
/// moves handed out structurally so the voxel stock engine can stamp the
/// swept volume from the engine's own emission rather than re-parsing stdout
/// (DECISIONS.md "Phase 5's voxel stock engine…", the fork: a stamp fed from
/// the text alone is wrong by construction under control comp). The emitted moves
/// changes nothing about the emitted program — `program()` is this function
/// with the emitted moves discarded.
pub fn program_with_moves(p: &Part) -> Result<(String, Vec<OpEmission>), String> {
    let mut g: Vec<String> = Vec::new();
    let mut emitted_moves: Vec<OpEmission> = Vec::new();
    g.push("%".into());
    g.push("(nacs — nacs output)".into());

    // Print all defined tools in comments. Their numbers are the one
    // dimensional content of the program the formatting-boundary respell
    // deliberately never touches (comments pass through whole), so the
    // lengths here spell through the coordinate class's OUTPUT formatter at
    // the source instead — these are the numbers an operator DIALS (the D
    // register values above all), and a mm register value inside a G20
    // program would be the trap the whole header exists to avoid. Angles
    // are degrees and never convert (the `rotation` rule).
    let fc = |v: f64| f_coord_out(v, p.machine.as_ref());
    let mut tool_nums: Vec<&usize> = p.tools.keys().collect();
    tool_nums.sort();
    for t_num in tool_nums {
        let t = p.tools.get(t_num).unwrap();
        // A corner rounder's `dia` is its BODY OD — clearance, and the one
        // number on the tool that must never reach a comp register. What the
        // operator dials is the diameter of consequence, `2(rc - R)`, plus any
        // standoff the program folded in (DECISIONS.md → "the corner rounder
        // gets an operation"). If this line printed the body OD or the pilot,
        // the program would spring the trap the whole design exists to avoid,
        // so it prints the effective number AND says what it is.
        match &t.rounder {
            Some(rd) => {
                // One register holds one number, and validate has already
                // rejected any program whose control passes disagree about it.
                let standoff = p
                    .operations
                    .iter()
                    .find(|op| op.tool_num == *t_num && op.comp == crate::parse::CompMode::Control)
                    .map(|op| op.wall_standoff)
                    .unwrap_or(0.0);
                let dia = crate::parse::derive_edge_round_comp_dia(rd, standoff);
                let note = if standoff > 0.0 {
                    format!(" = effective {} + 2 x standoff {}", fc(rd.effective_dia), fc(standoff))
                } else {
                    String::new()
                };
                g.push(format!(
                    "(tool T{} CornerRounder R{} body dia {} pilot {} -> comp register D{} = {}{})",
                    derive_t(*t_num), fc(rd.radius), fc(t.dia), fc(rd.pilot),
                    derive_d(*t_num), fc(dia), note
                ));
                g.push(format!(
                    "(  D{} is the EFFECTIVE diameter 2(rc-R), not the pilot: the pilot is relieved {} per side and nothing positions from it)",
                    derive_d(*t_num), fc(rd.relief())
                ));
            }
            // A chamfer mill's `dia` is likewise the BODY OD, and its D is a
            // function of the placement's resolved slide along the flank: the
            // cone's diameter at the contact height. The header states WHICH
            // height that diameter lives at, because a D register nobody can
            // check against the tool is a number nobody can verify — the
            // strongest argument for the declared tip_clearance over a
            // derived-and-invisible placement (DECISIONS.md → "the chamfer
            // tool…").
            None => match &t.chamfer {
                Some(ch) => {
                    // One register holds one number, rejected otherwise — as
                    // above. With no control op, the derived default is the
                    // number a control op WOULD get, printable tool-side
                    // because the default is a tool-only function.
                    let (tc, standoff) = p
                        .operations
                        .iter()
                        .find(|op| op.tool_num == *t_num && op.comp == crate::parse::CompMode::Control)
                        .map(|op| (crate::parse::derive_tip_clearance(op, ch), op.wall_standoff))
                        .unwrap_or((crate::parse::derive_default_tip_clearance(ch), 0.0));
                    let dia = crate::parse::derive_chamfer_comp_dia(ch, tc, standoff);
                    let cone = t.ground_cone().expect("a ChamferMill has a cone");
                    let contact_h = (dia / 2.0) / cone.phi_half.ptan();
                    g.push(format!(
                        "(tool T{} ChamferMill {}deg body dia {} tip {} -> comp register D{} = {})",
                        derive_t(*t_num), f(ch.angle), fc(t.dia), fc(ch.tip_dia),
                        derive_d(*t_num), fc(dia)
                    ));
                    g.push(format!(
                        "(  D{} is the cone diameter {} above the virtual apex, not the body OD: the tip is a relieved flat {} wide and nothing positions from it)",
                        derive_d(*t_num), fc(contact_h), fc(ch.tip_dia)
                    ));
                }
                None => g.push(format!("(tool T{} dia {} -> comp register D{})",
                                       derive_t(*t_num), fc(t.dia), derive_d(*t_num))),
            },
        }
    }

    // G21 in the INTERNAL dialect, always: Part values are mm by the time
    // they get here — inch inputs were converted at the parse boundary
    // (DECISIONS.md "Internal units are mm, always"). A machine that
    // declares `units inch` gets G20 from the formatting-boundary respell
    // below, in the same pass that converts every dimension word — the two
    // can never come apart, which is exactly what the old warning here
    // guarded ("G20 against mm numbers would be a 25.4x error": true then,
    // true now; the respell is the one licensed way to pair G20 with
    // inch-converted numbers).
    g.push("G21".into());
    // The datum slot (DECISIONS.md → "work offsets…", fork 4A): the one
    // word the selection changes — G54 undeclared, byte for byte, or the
    // selected register's spelling (the vendor's two-word form for an
    // extended family). Nothing else can move: the selection relabels
    // where the machine PUTS the program's space, never the space itself.
    match &p.work_offset {
        None => g.push("G90 G54 G17".into()),
        Some(wo) => g.push(format!("G90 {} G17", wo.words())),
    }

    let mut active_tool = None;
    let mut active_spindle = 0.0;
    let mut last_clearance_z = p.global_clearance_z;

    // The coolant state, beside the spindle's (DECISIONS.md → "coolant as a
    // modeled setting…", fork 4A): one state, explicit transitions. The
    // resolved kind's word comes from the same menu home validate rejected
    // through, so a kind that reaches this expect has a word by construction.
    let mut active_coolant = crate::parse::CoolantKind::Off;

    for (op_idx, op) in p.operations.iter().enumerate() {
        // The restart ladder's anchor (DECISIONS.md → "N-words…", fork 1A):
        // whatever line this op contributes FIRST — the leaving retract of
        // its tool change, its bare S or coolant transition, or its title —
        // is where its declared rung lands, below.
        let op_start = g.len();
        let tool_changed = active_tool != Some(op.tool_num);
        let spindle_speed = crate::parse::derive_spindle_speed(op, p.global_spindle);
        let spindle_changed = active_spindle != spindle_speed;
        let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);
        let coolant = crate::parse::coolant_for(p, op);
        let coolant_on_word = |k: crate::parse::CoolantKind| {
            crate::parse::coolant_word(p.machine.as_ref(), k)
                .expect("validate rejected any kind the menu cannot say")
        };

        if tool_changed {
            if active_tool.is_some() {
                g.push("M09".into());
                g.push("M05".into());
                g.push(format!("G00 Z{}", f(last_clearance_z)));
            }
            g.push(format!("M06 T{}", derive_t(op.tool_num)));
            g.push(format!("M03 S{}", f_speed(spindle_speed, p.machine.as_ref())));
            // The M08 slot, generalized: the resolved kind's word — and NO
            // word when the kind is `off` (the leaving half above already
            // said M09, and the first block never had coolant on).
            if coolant != crate::parse::CoolantKind::Off {
                g.push(coolant_on_word(coolant));
            }
            // Length comp for the new tool, applied on the move up to
            // clearance so it's active before any cutting move (the safety
            // lint enforces this ordering).
            g.push(format!("G00 G43 H{} Z{}", derive_h(op.tool_num), f(clearance_z)));
            active_tool = Some(op.tool_num);
            active_spindle = spindle_speed;
        } else {
            if spindle_changed {
                g.push(format!("S{}", f_speed(spindle_speed, p.machine.as_ref())));
                active_spindle = spindle_speed;
            }
            // A kind change without a tool change: off-then-on at the op
            // boundary — the tool-change block's own shape, on the bare-S
            // re-issue's precedent. On controllers where two coolant words
            // are independent channels, a bare swap would leave both
            // running; a one-state model closes the old channel first.
            if coolant != active_coolant {
                g.push("M09".into());
                if coolant != crate::parse::CoolantKind::Off {
                    g.push(coolant_on_word(coolant));
                }
            }
        }
        active_coolant = coolant;

        last_clearance_z = clearance_z;

        let (moves, resolved) = match op.op_type {
            // A visit to points is a motion, whatever the verb (Phase 36
            // slice 3): the plunge cycle that was `Drill`.
            _ if op.at_points() => (drill(p, op), None),
            // A face that goes around a standing boss can fail like any other
            // family (a web between two islands) — Phase 26 slice 3.
            OpType::End => (floor(p, op)?, None),
            // Contour, Pocket, and Finish can fail at generation time (an
            // engine-comp offset that doesn't fit, a feature too small for
            // the tool, or a torus bore too small for the ball) — that's a
            // hard error, never a silently skipped op (DECISIONS.md
            // "An unmachinable feature is a hard error").
            OpType::Clear => {
                // strategy=rest reads the stock as it stands BEFORE this op
                // (DECISIONS.md "adaptive clearing…", slice 1): the field is
                // built from the ops already emitted above — the emitted moves IS the
                // prefix — at the op's own explicit voxel_cell. The
                // incompleteness law is a hard error, not a quiet
                // degrade: an incomplete field over-states material, the op
                // would fall back to the dense spiral while claiming rest
                // semantics, and the error names the op to fix instead.
                let rest_field = match op.strategy {
                    None => None,
                    Some(s) => {
                        let word = match s {
                            crate::parse::ClearStrategy::Rest => "rest",
                            crate::parse::ClearStrategy::Regions => "regions",
                        };
                        let cell = op.voxel_cell.expect("validate: a field strategy requires voxel_cell");
                        let field =
                            crate::vox::VoxField::build_prefix_from_emissions(p, &emitted_moves, op_idx, Some(cell))?;
                        if !field.complete() {
                            return Err(format!(
                                "op [{}] (Clear on '{}') uses strategy={word}, which selects passes against the stock as it stands before this op — and that state is INCOMPLETE:\n{}\nfix the named op or drop strategy={word} (an incomplete field over-states material, so field selection would silently mis-state what stands while claiming its semantics)",
                                op_idx,
                                op.feature_name,
                                field
                                    .incomplete_reasons
                                    .iter()
                                    .map(|r| format!("  - {r}"))
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            ));
                        }
                        // The regions work-mask reads ONE layer above the
                        // floor under the upward-closure lemma ("every
                        // standing column shows at that layer"). A truncated
                        // silhouette in the prefix — a form cutter, or a rod
                        // with a declared flute_len (K2 slice 2) — breaks the
                        // lemma: material can stand above swept void and
                        // never show at the query plane, so the mask would
                        // silently miss work while claiming coverage. Reject
                        // by name, like the incompleteness law above; `rest`
                        // needs no twin error because its certifier already
                        // answers false per loop and the op honestly cuts
                        // everything instead.
                        if matches!(s, crate::parse::ClearStrategy::Regions)
                            && !field.upward_closed()
                        {
                            return Err(format!(
                                "op [{}] (Clear on '{}') uses strategy=regions, whose work-list reads one field layer under the upward-closure lemma — and a truncated silhouette stamped this prefix (a form cutter, or a rod with a declared flute_len), so standing material need not show at that layer. Rough the prefix with an unbounded tool, or drop strategy=regions (a blind work-mask would silently miss standing work while claiming its semantics)",
                                op_idx,
                                op.feature_name,
                            ));
                        }
                        Some(field)
                    }
                };
                (clear(p, op, rest_field.as_ref())?, None)
            }
            OpType::Side | OpType::Copy | OpType::Form => {
                let m = finish(p, op)?;
                (m.text, m.resolved)
            }
        };
        // The entry restriction (DECISIONS.md "The entry restriction refuses
        // the program…"): the op's moves are read against the analytical
        // op-history stock before they are stitched in, so a plunge or rapid
        // that drives the tool into material no op removes never reaches
        // stdout. It rejects; it never rewrites a move. It reads the
        // TOOL-CENTER head — the resolved channel when the emitter authored
        // one (under control comp the text is the part edge, which is not
        // where the tool goes; DECISIONS.md "the control-comp centerline
        // resolve", slice 2), the text otherwise.
        let mut em = OpEmission { op_idx, moves, resolved, lines: op_start..op_start, moves_start: 0 };
        crate::stock::check_moves(p, op_idx, resolved_moves(&em))?;
        // The address (Phase 16 slice 2): the one `extend` below is what
        // makes `moves_start + k` the k-th move's program line.
        em.moves_start = g.len();
        g.extend(em.moves.iter().cloned());
        // The rung lands on the PROGRAM TEXT alone: the emitted moves stays unlabeled
        // (walkers strip the word anyway, and the label is an operator-facing
        // fact about the text, not about the motion), so stamps and cache
        // hashes cannot move under renumbering.
        if let Some(n) = op.n {
            g[op_start] = format!("N{} {}", n, g[op_start]);
        }
        em.lines = op_start..g.len();
        emitted_moves.push(em);
    }

    g.push("M09".into());
    g.push("M05".into());
    g.push(format!("G00 Z{}", f(last_clearance_z)));
    g.push("M30".into());
    g.push("%".into());

    // The entry gate (DECISIONS.md "the plunge taxonomy"): opt-in via
    // `entry gate`, it rejects the program on the same findings the advisory
    // voice prints — one computation, two voices, the engagement gate's
    // pattern. Enforced here rather than at validate because entries exist
    // only once moves do; last, over the finished emitted moves, so a malformed op is
    // reported as its own hard error, not as an entry finding.
    if p.entry == crate::parse::EntryMode::Gate {
        let findings = crate::stock::entry_findings(p, &emitted_moves);
        if let Some(first) = findings.first() {
            return Err(match findings.len() {
                1 => format!("entry gate: {}", first),
                n => format!("entry gate: {} (and {} more finding{})",
                             first, n - 1, if n > 2 { "s" } else { "" }),
            });
        }
    }
    // The keep-out gate (DECISIONS.md → "V2, the fixture…", fork 6A). Same
    // shape as the entry gate above and for the same reason — a keep-out is
    // met by a MOVE — and same law: it rejects on exactly the findings the
    // advisory voice prints, one computation and two voices. It is the fourth
    // gate and the FIRST whose probe over-claims; that is precisely why it is
    // allowed to reject at all, where body-foul never will be.
    if p.fixture_mode == crate::parse::FixtureMode::Gate {
        let findings = crate::fixture::fixture_findings(p, &emitted_moves);
        if let Some(first) = findings.first() {
            return Err(match findings.len() {
                1 => format!("fixture gate: {}", first),
                n => format!("fixture gate: {} (and {} more finding{})",
                             first, n - 1, if n > 2 { "s" } else { "" }),
            });
        }
    }
    // The reach gate (DECISIONS.md → "`reach gate`, the fifth directive…").
    // Fifth and last of the family, in the same place and by the same law: it
    // rejects on exactly the findings the advisory voice prints, and it lives
    // here rather than at validate because the certificate reads the emitted
    // moves. What made it ELIGIBLE is one step past the keep-out's argument —
    // that one gates because an over-claiming bound never misses, and this one
    // neither over- nor under-claims: a declared `stickout` against the deepest
    // emitted tip is one subtraction.
    if p.reach_mode == crate::parse::ReachMode::Gate {
        let findings = crate::stock::reach_findings(p, &emitted_moves);
        if let Some(first) = findings.first() {
            return Err(match findings.len() {
                1 => format!("reach gate: {}", first),
                n => format!("reach gate: {} (and {} more finding{})",
                             first, n - 1, if n > 2 { "s" } else { "" }),
            });
        }
    }
    // The envelope gate's travel half (Phase 15 slice 4): sixth of the
    // family, same place and same law as its siblings — it rejects on
    // exactly the findings the advisory voice prints, and it lives here
    // because the travel arm reads emitted moves. The ceiling and changer
    // arms gate at validate, where their numbers already exist.
    if p.envelope_mode == crate::parse::EnvelopeMode::Gate {
        let findings = crate::stock::envelope_findings(p, &emitted_moves);
        if let Some(first) = findings.first() {
            return Err(match findings.len() {
                1 => format!("envelope gate: {}", first),
                n => format!("envelope gate: {} (and {} more finding{})",
                             first, n - 1, if n > 2 { "s" } else { "" }),
            });
        }
    }
    // The formatting boundary (DECISIONS.md "K1 slice 1 — the formatting
    // boundary…"): the one place the internal mm dialect becomes the
    // declared machine's output format. It runs LAST, after every reader —
    // check_moves read each op's moves above, the entry gate just read the
    // finished emitted moves, and the emitted moves itself stays in the internal dialect for
    // the downstream readers (the voxel stamp builder, the toolpath view,
    // the advisory findings) — so conversion teaches no reader anything.
    // Skipped entirely for the undeclared machine and for any declared
    // machine whose output format is the default: today's bytes never pass
    // through a parse-and-reformat they don't need.
    if let Some(m) = p.machine.as_ref().filter(|m| !m.output_format_is_default()) {
        for line in g.iter_mut() {
            *line = respell_line(line, m);
        }
    }
    Ok((g.join("\n"), emitted_moves))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reader-side home for the restart label (DECISIONS.md →
    /// "N-words…", fork 4A): only the canonical spelling this dialect
    /// writes is a label; everything else is somebody's content.
    #[test]
    fn the_n_word_strips_only_its_canonical_spelling() {
        assert_eq!(strip_n_word("N10 G01 X1.000"), "G01 X1.000");
        assert_eq!(strip_n_word("N7 (--- title ---)"), "(--- title ---)");
        assert_eq!(strip_n_word("N10"), "");
        assert_eq!(strip_n_word("G01 X1.000"), "G01 X1.000");
        assert_eq!(strip_n_word("N G01"), "N G01", "no digits, no label");
        assert_eq!(strip_n_word("N10X5"), "N10X5", "no separating space, not ours");
        assert_eq!(split_n_word("N42 M09"), Some(("N42", "M09")));
    }

    /// The latent crash the design entry measured: an N-prefixed TITLE
    /// slipped past the readers' `starts_with('(')` guards into their word
    /// loops, where `split_at(1)` on a word beginning with a non-ASCII
    /// character — feature names are free text — panicked on the byte
    /// boundary. Now the label strips first and a title is a title again.
    #[test]
    fn an_n_prefixed_title_is_still_a_comment_to_every_reader() {
        let title = "N10 (--- floor: Ölfläche with T1 ---)";
        let mut m = Motion { rapid: true, x: None, y: None, z: None, f: None };
        assert!(!apply_move(title, &mut m), "a title is not a move, labeled or not");
        assert_eq!(m.x, None);
        assert_eq!(arc_words(title), None);
        // And a labeled MOVE still moves.
        assert!(apply_move("N20 G01 X5.000 Y1.000 Z-2.000", &mut m));
        assert_eq!(m.x, Some(5.0));
    }

    /// The writer side: the label is not a dimension, so the formatting
    /// boundary respells the BODY and leaves the label alone — and an
    /// N-prefixed title keeps its comment immunity (a feature name that
    /// looks like a coordinate word must not respell).
    #[test]
    fn respell_splits_the_label_off_before_judging_the_line() {
        // Decimals stated explicitly: a hand-built Machine skips
        // parse_machine's by-units defaulting.
        let m = Machine {
            units: crate::parse::OutputUnits::Inch,
            coord_decimals: 4,
            feed_decimals: 2,
            ..Machine::default()
        };
        assert_eq!(respell_line("N5 G21", &m), "N5 G20");
        assert_eq!(respell_line("N5 G01 X25.400 F254.000", &m), "N5 G01 X1.0000 F10.00");
        assert_eq!(
            respell_line("N5 (--- floor: X12 with T1 ---)", &m),
            "N5 (--- floor: X12 with T1 ---)",
            "the comment's X12 is a NAME, not a coordinate"
        );
    }
}
