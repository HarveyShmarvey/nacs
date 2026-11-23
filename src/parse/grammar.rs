use crate::math::Portable as _;
// The `.part` grammar: statement parsing, the key tables, `parse_into`, the machine file, and the inch-to-mm conversion.

use super::*;

// ---- parser helpers ---------------------------------------------------------

pub(super) fn parse_args(s: &str, lineno: usize) -> Result<(String, std::collections::HashMap<String, String>), String> {
    let open_idx = s.find('(').ok_or_else(|| format!("line {}: missing '(' in: {}", lineno + 1, s))?;
    let close_idx = s.rfind(')').ok_or_else(|| format!("line {}: missing ')' in: {}", lineno + 1, s))?;
    if close_idx < open_idx {
        return Err(format!("line {}: invalid parentheses in: {}", lineno + 1, s));
    }
    let name = s[..open_idx].trim().to_string();
    let args_str = &s[open_idx + 1..close_idx];
    
    let mut map = std::collections::HashMap::new();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut bracket_depth = 0;
    for c in args_str.chars() {
        match c {
            '[' => {
                bracket_depth += 1;
                current.push(c);
            }
            ']' => {
                bracket_depth -= 1;
                current.push(c);
            }
            ',' if bracket_depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => {
                current.push(c);
            }
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    for part in parts {
        let part = part.trim();
        if part.is_empty() { continue; }
        let eq_idx = part.find('=').ok_or_else(|| format!("line {}: missing '=' in argument: {}", lineno + 1, part))?;
        let key = part[..eq_idx].trim().to_string();
        let val = part[eq_idx + 1..].trim().to_string();
        map.insert(key, val);
    }
    Ok((name, map))
}

pub(super) fn num(s: &str, lineno: usize) -> Result<f64, String> {
    s.parse::<f64>().map_err(|_| format!("line {}: '{}' is not a number", lineno + 1, s))
}

pub(super) fn get_num(map: &std::collections::HashMap<String, String>, key: &str, lineno: usize) -> Result<f64, String> {
    let val_str = map.get(key).ok_or_else(|| format!("line {}: missing key '{}'", lineno + 1, key))?;
    num(val_str, lineno)
}

// `extent` on the extrusion features, with the retired spelling rejected rather
// than reported as a missing key (DECISIONS.md "The `extent` vocabulary is
// scoped to the extrusion features…"). A `.part` written before the rename is
// a real file a real person has, and "missing key 'extent'" names the new word
// without ever naming the old one — the same rejected-never-ignored rule the
// operation-level `depth` retirement followed.
pub(super) fn get_extent(
    map: &std::collections::HashMap<String, String>,
    retired: &str,
    f_type: &str,
    lineno: usize,
) -> Result<f64, String> {
    if !map.contains_key("extent") && map.contains_key(retired) {
        return Err(format!(
            "line {}: `{}` on {} is now `extent` — one word for vertical occupancy on every feature that is a swept cross-section, with direction coming from the variant. The shape parameters (Cone's `height`) are not reached",
            lineno + 1,
            retired,
            f_type
        ));
    }
    // Both spellings at once is two records for one fact — rejected rather
    // than letting the retired one ride unread past the whitelist
    // (DECISIONS.md → "the unread-key silences…").
    if map.contains_key("extent") && map.contains_key(retired) {
        return Err(format!(
            "line {}: {} carries both `extent` and the retired `{}` — one word for vertical occupancy; drop `{}`",
            lineno + 1,
            f_type,
            retired,
            retired
        ));
    }
    get_num(map, "extent", lineno)
}

/// The whitelist law, reaching the arg-list lines that skipped it
/// (DECISIONS.md → "the unread-key silences…"): a field nobody reads is a
/// declaration that quietly does not exist — the tool line's own words (K2
/// slice 0) — so the stock line, every feature arm, and the run line reject
/// what they do not know. `known` is exactly what the line's parse arm
/// reads, listed in the error; `tolerated` is the retired spellings whose
/// pointed migration messages fire in the arm's own read phase (`depth` on a
/// bore, `height` on a boss) — checked here so they never hit the generic
/// error, never listed as if they were fields to write.
// ---- the whitelists, one home each (Phase 21 slice 1) -------------------
//
// The three key lists the parser rejects against are `pub` so the window's
// form (`view::statement_form`) can OFFER exactly what the parser ACCEPTS —
// read here, never restated, and pinned mirror-to-mirror against
// `template::TOOL_SPECS` / `FEATURE_SPECS` and the run skeletons
// (`tests/wizard_tests.rs`). Growth stays additive: a new field is a new
// entry beside its own read, and the pin says when a spec forgot it.

/// Every key a `run` line may carry — ONE set, the union of every key any
/// op reads, because the parser reads them all into one `Operation`
/// regardless of verb (DECISIONS.md → "the unread-key silences…", fork
/// 2A). Whether a verb CONSUMES a passed key is the semantic matrix,
/// deferred by name there.
pub const RUN_KEYS: [&str; 33] = [
    "tool", "surface", "profile", "points", "cycle", "peck_increment",
    "stepover", "stepdown", "scallop", "feed", "plunge", "rapid_z",
    "clearance_z", "spindle", "comp", "compress", "stock_to_leave",
    "wall_standoff", "face_standoff", "tip_clearance", "strategy",
    "voxel_cell", "coolant", "n", "pattern", "engagement", "cover",
    "direction", "angle", "entry", "lead", "lead_angle", "lead_descent",
];

/// The one reference key a verb READS (Phase 22 slice 3, the reference
/// rule): a `Floor` or a `Finish` follows a `surface=`, a `Clear` empties
/// or roughs around a `profile=`, and a `Form` or an `End` that visits
/// holes names them in `points=` (which is what makes it a visit).
/// The run arm refuses any other reference key on the line by name — the
/// first row of the per-verb semantic matrix the whitelist law deferred.
pub fn run_reference_key(op: OpType, at_points: bool) -> &'static str {
    match op {
        OpType::End | OpType::Form if at_points => "points",
        OpType::End | OpType::Side | OpType::Copy | OpType::Form => "surface",
        OpType::Clear => "profile",
    }
}

/// Every key the `stock Box` line may carry (Phase 22 slice 1, the stock
/// as a statement): the three sides, the origin and the top datum, and
/// the material the feeds lookup reads. `w`, `l`, `h` are the line's
/// identity — the stock has no size without them — and the parser's
/// `get_num` says so; the rest default. The window's form offers exactly
/// this list (`view::statement_form`), read and never restated.
/// The header's six words (Phase 50 slice 3), in the order a file reads
/// them: the form's fields, `--init`'s lines, and the keys whose lines
/// `Part::header_lines` records.
pub const HEADER_KEYS: [&str; 6] = ["units", "feed", "plunge", "spindle", "rapid_z", "clearance_z"];

pub const STOCK_KEYS: [&str; 7] = ["w", "l", "h", "x", "y", "z", "material"];

/// Every key the `stock Cylinder` line may carry (Phase 46 slice 1): the
/// diameter and the height are the line's identity; `x`, `y` are the AXIS
/// (the fixture Cylinder's convention), `z` the top datum, `material` as the
/// Box's. The window's form offers this list for a round blank.
pub const STOCK_CYLINDER_KEYS: [&str; 6] = ["dia", "h", "x", "y", "z", "material"];

/// Every key a `stock Profile` opener may carry (Phase 54 slice 5a): the
/// height is the line's identity, `z` the top datum, `material` as the
/// Box's; the footprint is the points below, not a key.
pub const STOCK_PROFILE_KEYS: [&str; 3] = ["h", "z", "material"];

/// Every kind word a `tool` line may open with (Phase 36 slice 0), in
/// `template::TOOL_SPECS`'s order — pinned equal to it by test.
pub const TOOL_KINDS: [&str; 6] = ["Endmill", "Ballmill", "Drill", "FormTool", "CornerRounder", "ChamferMill"];

/// Every key a `tool` line may carry, every kind together: the kind's own
/// keys and the body ladder (K2 slice 0, DECISIONS.md → "K2, the tool
/// body"). Keys of the WRONG tool family pass this list so the
/// cross-errors in the tool arm can name the family instead.
pub const TOOL_KEYS: [&str; 27] = [
    "dia", "profile", "source",
    "radius", "effective_dia", "gauge_z", "pilot",
    "radius_error", "trail_wall", "trail_face",
    "angle", "angle_error", "gauge_dia", "tip_dia",
    "flute_len", "shank_dia", "neck_dia", "neck_len", "stickout", "flute_count",
    "holder", "holder_nose_dia", "holder_nose_len", "holder_body_dia",
    "cut_bottom", "cut_top", "coolant",
];

/// The whitelist law, per feature variant (DECISIONS.md → "the unread-key
/// silences…", fork 2A): `(variants, known, tolerated)` — each `known` set
/// is exactly what the variant's arm reads; the `tolerated` half is the
/// retired spellings whose migration messages fire in the arm itself
/// (`get_extent`). The block variants are here too: their opener is
/// judged against the same table.
#[rustfmt::skip]
pub const FEATURE_KEYS: &[(&[&str], &[&str], &[&str])] = &[
    (&["Plane"], &["z"], &[]),
    (&["Face"], &["z"], &[]),
    (&["ProfilePocket", "ProfileBoss"], &["z", "extent", "edge_round", "chamfer_depth", "chamfer_width", "chamfer_angle"], &[]),
    // `z` never reaches the check: a Contour wire's pointed error fires first.
    (&["ContourPocket", "ContourBoss"], &["extent"], &[]),
    (&["CylinderBore"], &["dia", "extent", "x", "y", "z", "edge_round", "chamfer_depth", "chamfer_width", "chamfer_angle", "csink_dia", "spot_dia", "csink_angle", "cbore_dia", "cbore_depth"], &["depth"]),
    (&["TiltedBore"], &["dia", "extent", "x", "y", "tilt", "heading"], &["depth"]),
    (&["Sphere"], &["dia", "x", "y", "z"], &[]),
    (&["Cone"], &["dia", "height", "x", "y", "z"], &[]),
    (&["Torus"], &["major_dia", "minor_dia", "x", "y", "z"], &[]),
    (&["CylinderBoss"], &["dia", "extent", "x", "y", "z", "edge_round", "chamfer_depth", "chamfer_width", "chamfer_angle"], &["height"]),
    (&["Mesh", "SplineRevolve", "NurbsPatch"], &["file", "x", "y", "z"], &[]),
    (&["SplineRevolveFunnel"], &["file", "x", "y"], &[]),
];

/// A variant's `(known, tolerated)` key sets, or `None` for a word the
/// table does not name — an unknown VARIANT skips the check and falls to
/// the dispatch's own error-by-name.
pub fn feature_keys(variant: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    FEATURE_KEYS.iter().find(|(vs, ..)| vs.contains(&variant)).map(|(_, known, tolerated)| (*known, *tolerated))
}

pub(super) fn reject_unknown_keys(
    map: &std::collections::HashMap<String, String>,
    known: &[&str],
    tolerated: &[&str],
    what: &str,
    lineno: usize,
) -> Result<(), String> {
    let mut unknown: Vec<&str> = map
        .keys()
        .map(String::as_str)
        .filter(|k| !known.contains(k) && !tolerated.contains(k))
        .collect();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return Err(format!(
            "line {}: {} has no field named '{}' — a field nobody reads is a declaration that quietly does not exist, so the line rejects what it does not know. Known fields: {}",
            lineno + 1,
            what,
            unknown.join("', '"),
            known.join(", ")
        ));
    }
    Ok(())
}

pub(super) fn get_num_opt(map: &std::collections::HashMap<String, String>, key: &str, lineno: usize) -> Result<Option<f64>, String> {
    if let Some(val_str) = map.get(key) {
        let v = num(val_str, lineno)?;
        Ok(Some(v))
    } else {
        Ok(None)
    }
}

// `feed` / `spindle`, which accept the `derive` sentinel in place of a number
// (DECISIONS.md → "K3, the material…", fork 4A). Returns the declared number
// (if any) and whether the sentinel was written; the two are mutually
// exclusive by construction since both ride the same key.
pub(super) fn get_num_or_derive(
    map: &std::collections::HashMap<String, String>,
    key: &str,
    lineno: usize,
) -> Result<(Option<f64>, bool), String> {
    match map.get(key).map(|s| s.as_str()) {
        None => Ok((None, false)),
        Some("derive") => Ok((None, true)),
        Some(val_str) => Ok((Some(num(val_str, lineno)?), false)),
    }
}

/// The feature-side chamfer declaration, shared by all four prismatic parse
/// arms. The three words are PICK TWO (Phase 46 slice 3): one leg and the
/// tool's angle, one leg and a declared angle, or both legs with the angle
/// derived from them — and all three only where they agree. Three errors
/// live here so they cannot drift apart: three words that disagree, an angle
/// with no leg (a slope with no size dimensions nothing), and a chamfer
/// beside an `edge_round` (one top edge, one declared break — a corner is
/// rounded or beveled, not both).
pub(super) fn get_chamfer_spec(
    map: &std::collections::HashMap<String, String>,
    lineno: usize,
) -> Result<Option<ChamferSpec>, String> {
    let depth = get_num_opt(map, "chamfer_depth", lineno)?;
    let width = get_num_opt(map, "chamfer_width", lineno)?;
    let mut angle = get_num_opt(map, "chamfer_angle", lineno)?;
    let leg = match (depth, width) {
        // PICK TWO (Phase 46 slice 3, the fork-2 ruling): two legs imply the
        // included angle, and that implied angle meets the check a declared
        // one meets — it must be the cone the tool grinds. Three words must
        // agree, or the line refuses naming all three: a print says
        // "0.5 X 45 DEG", a model measures 0.48 at 44°, and the feature line
        // is where the reviewer types which one is true.
        (Some(d), Some(w)) => {
            if d <= 0.0 || w <= 0.0 {
                return Err(format!("line {}: chamfer_depth and chamfer_width must be positive", lineno + 1));
            }
            let implied = 2.0 * (w / d).patan().to_degrees();
            if let Some(a) = angle {
                if (a - implied).abs() > 1e-6 {
                    return Err(format!(
                        "line {}: chamfer_depth={} and chamfer_width={} imply a {:.3}deg chamfer, but chamfer_angle={} — the three words disagree; any two dimension the chamfer and the third is derived, so drop the one the drawing does not say",
                        lineno + 1, d, w, implied, a
                    ));
                }
            }
            angle = Some(implied);
            Some(ChamferLeg::Depth(d))
        }
        (Some(d), None) => Some(ChamferLeg::Depth(d)),
        (None, Some(w)) => Some(ChamferLeg::Width(w)),
        (None, None) => None,
    };
    let Some(leg) = leg else {
        if angle.is_some() {
            return Err(format!(
                "line {}: chamfer_angle without chamfer_depth or chamfer_width — an angle with no leg dimensions nothing; declare the leg the drawing dimensions",
                lineno + 1
            ));
        }
        return Ok(None);
    };
    if map.contains_key("edge_round") {
        return Err(format!(
            "line {}: edge_round and a chamfer on one feature — a top edge is rounded or beveled, not both; declare the break the drawing calls for",
            lineno + 1
        ));
    }
    Ok(Some(ChamferSpec { leg, angle }))
}

/// The hole's mouth-cone declaration (DECISIONS.md → "spot drilling and
/// countersinking…"), read only by the CylinderBore arm because Drill is the
/// only verb that visits it. The feature-local errors live here so they
/// cannot drift: both jobs at once (one mouth, one declared cone), an angle
/// with no seat, a seat beside an edge break (a mouth cone IS the edge's
/// treatment), and the two size laws that need no tool to check — a seat no
/// wider than its hole leaves no seat, and a dimple as wide as its hole
/// survives the drill, which makes it a countersink misdeclared.
pub(super) fn get_csink_spec(
    map: &std::collections::HashMap<String, String>,
    dia: f64,
    lineno: usize,
) -> Result<Option<CsinkSpec>, String> {
    let csink = get_num_opt(map, "csink_dia", lineno)?;
    let spot = get_num_opt(map, "spot_dia", lineno)?;
    let angle = get_num_opt(map, "csink_angle", lineno)?;
    let seat = match (csink, spot) {
        (Some(_), Some(_)) => {
            return Err(format!(
                "line {}: csink_dia and spot_dia on one hole — one mouth carries one declared cone: a seat for a screw head or a dimple for a following drill. Declare the job the print calls for",
                lineno + 1
            ));
        }
        (Some(d), None) => Some(CsinkSeat::Csink(d)),
        (None, Some(d)) => Some(CsinkSeat::Spot(d)),
        (None, None) => None,
    };
    let Some(seat) = seat else {
        if angle.is_some() {
            return Err(format!(
                "line {}: csink_angle without csink_dia or spot_dia — an angle with no seat dimensions nothing; declare the diameter the drawing dimensions",
                lineno + 1
            ));
        }
        return Ok(None);
    };
    // One top edge, one declared treatment: a mouth cone already IS the
    // hole's edge break, so a chamfer or round beside it declares the same
    // edge twice (the edge_round/chamfer law, extended to the mouth).
    for key in ["chamfer_depth", "chamfer_width", "edge_round"] {
        if map.contains_key(key) {
            return Err(format!(
                "line {}: {} and a mouth cone (csink_dia/spot_dia) on one hole — the cone already breaks the mouth's edge, so a second declared break describes the same edge twice",
                lineno + 1, key
            ));
        }
    }
    let d = match seat {
        CsinkSeat::Csink(d) | CsinkSeat::Spot(d) => d,
    };
    if d <= 0.0 {
        return Err(format!(
            "line {}: a mouth cone's diameter ({}) must be positive",
            lineno + 1, d
        ));
    }
    match seat {
        CsinkSeat::Csink(_) if d <= dia => {
            return Err(format!(
                "line {}: csink_dia ({}) must exceed the hole's own diameter ({}) — at or under it the cone never emerges from the hole's void and no seat remains. A dimple narrower than the hole is spot_dia's job",
                lineno + 1, d, dia
            ));
        }
        CsinkSeat::Spot(_) if d >= dia => {
            return Err(format!(
                "line {}: spot_dia ({}) must stay under the hole's own diameter ({}) — a dimple as wide as its hole survives the drill, and the ring it leaves is a countersink: declare csink_dia",
                lineno + 1, d, dia
            ));
        }
        _ => {}
    }
    Ok(Some(CsinkSpec { seat, angle }))
}

/// The hole's counterbore declaration (DECISIONS.md → "the counterbore…"),
/// read only by the CylinderBore arm because Drill is the only verb that
/// visits it. The feature-local errors live here so they cannot drift: the
/// paired-fields rule (a dia with no depth dimensions nothing; a depth with
/// no dia likewise — the dangling-angle rule with two limbs), the
/// one-edge-one-break law's third instance (a seat beside a mouth cone, a
/// chamfer, or a round describes the mouth twice), and the two size laws that
/// need no tool to check — a seat no wider than its hole leaves no seat, and
/// a seat as deep as its hole consumes it. The tool-side pairing law reads
/// the tool and waits for validate().
pub(super) fn get_cbore_spec(
    map: &std::collections::HashMap<String, String>,
    dia: f64,
    extent: f64,
    lineno: usize,
) -> Result<Option<CboreSpec>, String> {
    let cb_dia = get_num_opt(map, "cbore_dia", lineno)?;
    let cb_depth = get_num_opt(map, "cbore_depth", lineno)?;
    let (cb_dia, cb_depth) = match (cb_dia, cb_depth) {
        (None, None) => return Ok(None),
        (Some(_), None) => {
            return Err(format!(
                "line {}: cbore_dia without cbore_depth — a cylinder derives nothing: there is no angle to derive a depth through, so a dia with no depth dimensions nothing. Declare both, off the print",
                lineno + 1
            ));
        }
        (None, Some(_)) => {
            return Err(format!(
                "line {}: cbore_depth without cbore_dia — a depth with no dia dimensions nothing. Declare both, off the print",
                lineno + 1
            ));
        }
        (Some(d), Some(h)) => (d, h),
    };
    // One top edge, one declared treatment — the edge_round/chamfer law's
    // third instance, extended across the mouth vocabulary: a counterbore
    // beside a mouth cone, a chamfer, or a round describes the same mouth
    // twice.
    for key in ["csink_dia", "spot_dia", "chamfer_depth", "chamfer_width", "edge_round"] {
        if map.contains_key(key) {
            return Err(format!(
                "line {}: {} and a counterbore (cbore_dia/cbore_depth) on one hole — the seat already treats the mouth, so a second declared treatment describes the same edge twice",
                lineno + 1, key
            ));
        }
    }
    if cb_dia <= 0.0 || cb_depth <= 0.0 {
        return Err(format!(
            "line {}: a counterbore's dimensions (dia {}, depth {}) must be positive",
            lineno + 1, cb_dia, cb_depth
        ));
    }
    if cb_dia <= dia {
        return Err(format!(
            "line {}: cbore_dia ({}) must exceed the hole's own diameter ({}) — at or under it the seat never emerges from the hole's void and no seat remains",
            lineno + 1, cb_dia, dia
        ));
    }
    if cb_depth >= extent {
        return Err(format!(
            "line {}: cbore_depth ({}) must stay under the hole's own extent ({}) — a seat as deep as its hole consumes it, leaving a wider hole misdeclared as a seated one",
            lineno + 1, cb_depth, extent
        ));
    }
    Ok(Some(CboreSpec { dia: cb_dia, depth: cb_depth }))
}

// ---- main parser logic ------------------------------------------------------

pub fn parse(src: &str) -> Result<Part, String> {
    parse_project_with_base(src, None, None)
}

/// Whether `statement` is exactly one `feature` statement of the current
/// `.part` grammar — judged by the ONE parser (`parse_into`), never a
/// second grammar. `Err` carries the parser's own sentence. Born as
/// `review::check_statement` (Phase 17 lane A); moved core-side in Phase
/// 19 slice 1 when the wizard became its third consumer — the lean build
/// has no `review`, but a statement's legality is the parser's fact in
/// every build, so `review::check_statement` now re-exports this one home.
pub fn check_feature_statement(statement: &str) -> Result<(), String> {
    let first = statement.split_whitespace().next().unwrap_or("");
    if first != "feature" {
        return Err(format!(
            "not a feature statement (it opens with `{first}`, and a review accepts `feature` lines only)"
        ));
    }
    let mut p = Part::default();
    parse_into(statement, &mut p, None)?;
    match p.features.len() {
        1 => Ok(()),
        // Unreachable on today's grammar (an unclosed block is the parser's
        // own error, "end of file reached while reading contour profile
        // points"), kept so a future statement form that parses to nothing
        // is refused rather than accepted as an empty line.
        0 => Err("parses as no feature statement".to_string()),
        n => Err(format!("parses as {n} features — one statement per answer")),
    }
}

/// A `tool` statement's legality from nothing (Phase 20 slice 2) — the
/// same shape as `check_feature_statement`: the one parser, alone. The
/// kind word is NOT judged here (the parser accepts any word), which is
/// why the wizard's kind menu refuses it first.
pub fn check_tool_statement(statement: &str) -> Result<(), String> {
    let first = statement.split_whitespace().next().unwrap_or("");
    if first != "tool" {
        return Err(format!("not a tool statement (it opens with `{first}`)"));
    }
    let mut p = Part::default();
    parse_into(statement, &mut p, None)?;
    match p.tools.len() {
        1 => Ok(()),
        n => Err(format!("parses as {n} tool statements — one statement per answer")),
    }
}

/// How a project failed to load, split where every caller needs it split: an
/// unreadable path has no check result to render (I/O stays prose in every mode),
/// while a parse error IS a check result.
///
/// It lives here rather than in the binary because a LIBRARY function needs
/// it now (`assembly::report_from_args`'s `part=`), and the binary's copy
/// could not be called from one. Rendering the two classes stays CLI policy
/// and stays in the binary.
#[derive(Debug)]
pub enum LoadError {
    Io(String),
    Parse(String),
}

/// **The one home for "a path argument becomes a `Part`"** — the directory
/// (`main.part` + optional `geometry.part`) versus bare-file split, shared by
/// the default CLI flow, every MCP verb, and the assembly report's `part=`.
///
/// **It said that before it was true.** This comment used to promise the CLI
/// and the MCP verbs "cannot drift", while `--assembly-report`'s own `part=`
/// read one file and passed `None` for `geometry.part` — so
/// `part=plate.cam/main.part` failed with "referenced feature 'Profile1' not
/// defined" on a two-file project every other mode reads. Moving the home
/// into the library is what let the third caller join it (`DECISIONS.md` →
/// "`--assembly-report` as the sixth verb…", forks 4A/5A).
pub fn load_project(path_arg: &str) -> Result<Part, LoadError> {
    let path = std::path::Path::new(path_arg);
    if path.is_dir() {
        let main_path = path.join("main.part");
        let geom_path = path.join("geometry.part");
        let main_src = std::fs::read_to_string(&main_path)
            .map_err(|e| LoadError::Io(format!("cannot read {}: {}", main_path.display(), e)))?;
        let geom_src = if geom_path.exists() {
            Some(std::fs::read_to_string(&geom_path).map_err(|e| {
                LoadError::Io(format!("cannot read {}: {}", geom_path.display(), e))
            })?)
        } else {
            None
        };
        let mut p = parse_project_with_base(&main_src, geom_src.as_deref(), Some(path)).map_err(LoadError::Parse)?;
        // The part files lead the source list; the externals the parse
        // resolved already follow, in the order it opened them.
        let mut lead = vec![main_path];
        if geom_src.is_some() {
            lead.push(geom_path);
        }
        lead.append(&mut p.sources);
        p.sources = lead;
        Ok(p)
    } else {
        let src = std::fs::read_to_string(path)
            .map_err(|e| LoadError::Io(format!("cannot read {}: {}", path.display(), e)))?;
        let mut p = parse_project_with_base(&src, None, path.parent()).map_err(LoadError::Parse)?;
        p.sources.insert(0, path.to_path_buf());
        Ok(p)
    }
}

pub fn parse_project(main_src: &str, geom_src: Option<&str>) -> Result<Part, String> {
    parse_project_with_base(main_src, geom_src, None)
}

pub fn parse_project_with_base(
    main_src: &str,
    geom_src: Option<&str>,
    base_dir: Option<&std::path::Path>,
) -> Result<Part, String> {
    let mut p = Part::default();
    // Errors carry a line number but that's ambiguous once a project has two
    // source files (main.part / geometry.part) — prefix with the filename in
    // that case. A bare single-file `.part` (geom_src == None) has only one
    // file, so "line N" alone is already unambiguous there.
    let is_project = geom_src.is_some();
    parse_into(main_src, &mut p, base_dir)
        .map_err(|e| if is_project { format!("main.part: {}", e) } else { e })?;
    if let Some(src) = geom_src {
        let (n_main, n_feat, n_tool) = (p.op_lines.len(), p.feature_lines.len(), p.tool_lines.len());
        let stock_before = p.stock_line.clone();
        parse_into(src, &mut p, base_dir)
            .map_err(|e| format!("geometry.part: {}", e))?;
        // A stock line in the second file is the second file's — the one
        // `stock` holds is the last parsed, so a change here means it
        // came from geometry.part.
        if p.stock_line != stock_before {
            if let Some(entry) = p.stock_line.as_mut() {
                entry.0 = 1;
            }
        }
        // The second file's statements belong to it: `parse_into` records
        // every line against file 0, and only the caller knows the file.
        for entry in p.op_lines.iter_mut().skip(n_main) {
            entry.0 = 1;
        }
        for entry in p.feature_lines.iter_mut().skip(n_feat) {
            entry.1 = 1;
        }
        for entry in p.tool_lines.iter_mut().skip(n_tool) {
            entry.1 = 1;
        }
        // The 2026-06 layout, made a rule (Phase 20, fork 7): the
        // operational script — tools and run lines — lives in main.part;
        // geometry.part holds the coordinate-heavy declarations. One
        // grammar still runs over both files, so this is the one place
        // that knows which file a line came from. It retires every
        // "which file would the act edit" refusal in the window at once.
        if let Some((_, line)) = p.op_lines.iter().find(|(file, _)| *file == 1) {
            return Err(format!(
                "geometry.part: line {}: a run line belongs in main.part — geometry.part holds the geometry (the coordinate-heavy feature declarations), and the operational script stays in one file so one edit can find it",
                line + 1
            ));
        }
        if let Some((num, _, line, _)) = p.tool_lines.iter().find(|(_, file, _, _)| *file == 1) {
            return Err(format!(
                "geometry.part: line {}: tool T{} belongs in main.part — geometry.part holds the geometry (the coordinate-heavy feature declarations), and the tool table stays in one file so one edit can find it",
                line + 1, num
            ));
        }
    }
    if !p.units_mm {
        convert_to_mm(&mut p);
    }
    validate(&p)?;
    // The `feed=derive` / `spindle=derive` sentinels resolve here — after
    // validate (tools exist) and the mm boundary (everything metric), before
    // any G-code exists. An error here is a hard error like any other:
    // nonzero exit, no partial program.
    resolve_derived_words(&mut p)?;
    // The gate's second reading, for the findings that could only exist once
    // the derived words did (see `engagement_gate`).
    engagement_gate(&p)?;
    envelope_gate(&p)?;
    Ok(p)
}

// One conversion in (here, at the parse boundary), one out (gcode.rs's
// formatting boundary, for a machine that declares `units inch`) — mm
// everywhere between. `pub(crate)` so the two boundaries share the one
// constant and cannot drift.
pub(crate) const MM_PER_INCH: f64 = 25.4;

/// The chamfer declaration's lengths, at the conversion boundary: the leg is a
/// length; `chamfer_angle` is degrees and never scales (the `rotation` rule).
pub(super) fn scale_chamfer_spec(spec: &mut Option<ChamferSpec>, s: f64) {
    if let Some(spec) = spec {
        match &mut spec.leg {
            ChamferLeg::Depth(d) => *d *= s,
            ChamferLeg::Width(w) => *w *= s,
        }
    }
}

// The one conversion boundary: after this, everything downstream (ops, writer,
// tolerances) may assume mm without checking. Spindle is RPM, not a length,
// so it is deliberately not scaled.
pub(super) fn convert_to_mm(p: &mut Part) {
    let s = MM_PER_INCH;
    p.global_feed *= s;
    p.global_plunge *= s;
    p.global_rapid_z *= s;
    p.global_clearance_z *= s;
    // The datum is a pair of machine-coordinate lengths, authored in the
    // part's units like everything else on the part side. (The machine
    // FILE's travel spans convert at their own parse, by ITS units.)
    if let Some((ref mut dx, ref mut dy)) = p.datum {
        *dx *= s;
        *dy *= s;
    }
    p.stock.w *= s;
    p.stock.l *= s;
    p.stock.h *= s;
    p.stock.x *= s;
    p.stock.y *= s;
    p.stock.z *= s;
    match p.stock.shape {
        StockShape::Cylinder { ref mut dia } => *dia *= s,
        StockShape::Profile { ref mut points } => {
            for q in points.iter_mut() {
                q.x *= s;
                q.y *= s;
            }
        }
        StockShape::Box => {}
    }
    for tool in p.tools.values_mut() {
        tool.dia *= s;
        // The K2 body fields are lengths — all but `flute_count`, a count,
        // and the `holder` label, provenance — which follow the non-length
        // rule the angles set.
        if let Some(ref mut fl) = tool.flute_len { *fl *= s; }
        if let Some(ref mut sd) = tool.shank_dia { *sd *= s; }
        if let Some(ref mut nd) = tool.neck_dia { *nd *= s; }
        if let Some(ref mut nl) = tool.neck_len { *nl *= s; }
        if let Some(ref mut so) = tool.stickout { *so *= s; }
        if let Some(ref mut nd) = tool.holder_nose_dia { *nd *= s; }
        if let Some(ref mut nl) = tool.holder_nose_len { *nl *= s; }
        if let Some(ref mut bd) = tool.holder_body_dia { *bd *= s; }
        if let Some(ref mut profile) = tool.profile {
            for pt in profile {
                pt.x *= s;
                pt.y *= s;
            }
        }
        // The form-tool records are lengths too — all but their angles, which
        // follow the `rotation` rule (the vocabulary's non-length fields are
        // never unit-scaled). The rounder's block was missing entirely until
        // the chamfer's went in beside it: an inch program with a
        // CornerRounder got unscaled arc geometry (found 2026-08-08, no inch
        // fixture ever existed to catch it).
        if let Some(ref mut rd) = tool.rounder {
            rd.radius *= s;
            rd.effective_dia *= s;
            rd.gauge_z *= s;
            rd.pilot *= s;
            rd.radius_error *= s;
            // trail_wall / trail_face are degrees: not scaled.
        }
        if let Some(ref mut ch) = tool.chamfer {
            ch.gauge_dia *= s;
            ch.gauge_z *= s;
            ch.tip_dia *= s;
            // angle / angle_error are degrees: not scaled.
        }
    }
    for feature in p.features.values_mut() {
        match feature {
            Feature::Plane { z } => *z *= s,
            Feature::Face { z, loops } => {
                *z *= s;
                for pt in loops.iter_mut().flatten() {
                    pt.x *= s;
                    pt.y *= s;
                }
            }
            Feature::ProfilePocket { points, z, extent, edge_round, chamfer, chamfer_marks }
            | Feature::ProfileBoss { points, z, extent, edge_round, chamfer, chamfer_marks } => {
                for pt in points.iter_mut() {
                    pt.x *= s;
                    pt.y *= s;
                }
                if let Some(z) = z.as_mut() {
                    *z *= s;
                }
                *extent *= s;
                if let Some(e) = edge_round { *e *= s; }
                scale_chamfer_spec(chamfer, s);
                for m in chamfer_marks.iter_mut() {
                    match m.leg.as_mut() {
                        Some(ChamferLeg::Depth(d)) => *d *= s,
                        Some(ChamferLeg::Width(w)) => *w *= s,
                        None => {}
                    }
                }
            }
            // The wire pair: every coordinate is a length, including the Z the
            // points carry themselves.
            Feature::ContourPocket { points, extent }
            | Feature::ContourBoss { points, extent } => {
                for pt in points.iter_mut() {
                    pt.x *= s;
                    pt.y *= s;
                    pt.z *= s;
                }
                *extent *= s;
            }
            // The tilted bore's lengths scale; `tilt` and `heading` are
            // degrees and never do (the `rotation` rule).
            Feature::TiltedBore { dia, extent, x, y, .. } => {
                *dia *= s;
                *extent *= s;
                *x *= s;
                *y *= s;
            }
            Feature::CylinderBore { dia, extent, x, y, z, edge_round, chamfer, csink, cbore } => {
                *dia *= s;
                *extent *= s;
                *x *= s;
                *y *= s;
                if let Some(z) = z.as_mut() { *z *= s; }
                if let Some(e) = edge_round { *e *= s; }
                scale_chamfer_spec(chamfer, s);
                // The mouth cone's seat is a length; `csink_angle` is degrees
                // and never scales (the `rotation` rule).
                if let Some(cs) = csink {
                    match &mut cs.seat {
                        CsinkSeat::Csink(d) | CsinkSeat::Spot(d) => *d *= s,
                    }
                }
                // Both counterbore fields are lengths; both scale, so the
                // pairing law's dia-vs-tool comparison stays scale-invariant.
                if let Some(cb) = cbore {
                    cb.dia *= s;
                    cb.depth *= s;
                }
            }
            // Quadrics: every field is a length, so all scale (no angles here).
            Feature::Sphere { dia, x, y, z } => {
                *dia *= s;
                *x *= s;
                *y *= s;
                *z *= s;
            }
            Feature::Cone { dia, height, x, y, z } => {
                *dia *= s;
                *height *= s;
                *x *= s;
                *y *= s;
                *z *= s;
            }
            Feature::Torus { major_dia, minor_dia, x, y, z } => {
                *major_dia *= s;
                *minor_dia *= s;
                *x *= s;
                *y *= s;
                *z *= s;
            }
            Feature::CylinderBoss { dia, extent, x, y, z, edge_round, chamfer } => {
                *dia *= s;
                *extent *= s;
                *x *= s;
                *y *= s;
                *z *= s;
                if let Some(e) = edge_round { *e *= s; }
                scale_chamfer_spec(chamfer, s);
            }
            // `rotation` on the elliptic pair is an ANGLE (degrees), the
            // vocabulary's first non-length field — deliberately not scaled.
            // Camp 3: STL vertex data is defined to be mm (see the variant's
            // doc comment), so the triangles never scale — only the placement
            // offsets are project units.
            Feature::Mesh { x, y, z, .. } => {
                *x *= s;
                *y *= s;
                *z *= s;
            }
            // Like the mesh, the meridian's (r, z) control points are defined to
            // be mm (see the variant's doc comment), so the curve never scales —
            // only the placement offsets are project units.
            Feature::SplineRevolve { x, y, z, .. } => {
                *x *= s;
                *y *= s;
                *z *= s;
            }
            // Same mm-by-definition meridian; the funnel has no z to scale
            // (its mouth is the stock top by construction).
            Feature::SplineRevolveFunnel { x, y, .. } => {
                *x *= s;
                *y *= s;
            }
            // Like the mesh and the meridian, the patch's control net is
            // defined to be mm (see the variant's doc comment) — only the
            // placement offsets are project units.
            Feature::NurbsPatch { x, y, z, .. } => {
                *x *= s;
                *y *= s;
                *z *= s;
            }
        }
    }
    for op in &mut p.operations {
        op.wall_standoff *= s;
        op.face_standoff *= s;
        for v in [&mut op.stepover, &mut op.stepdown, &mut op.scallop, &mut op.feed, &mut op.plunge, &mut op.rapid_z, &mut op.clearance_z, &mut op.peck_increment, &mut op.stock_to_leave, &mut op.tip_clearance, &mut op.lead] {
            if let Some(v) = v.as_mut() {
                *v *= s;
            }
        }
    }
    p.units_mm = true;
}

/// One `tool T<num> <Kind>(…)` statement, checked and inserted into the part.
fn parse_tool_statement(p: &mut Part, rest: &str, raw: &str, lineno: usize, base_dir: Option<&std::path::Path>) -> Result<(), String> {
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(format!("line {}: invalid tool declaration, expected 'tool T<num> <type>(...)'", lineno + 1));
    }
    let t_name = parts[0];
    if !t_name.starts_with('T') {
        return Err(format!("line {}: tool identifier must start with 'T', found '{}'", lineno + 1, t_name));
    }
    let tool_num = t_name[1..].parse::<usize>().map_err(|_| format!("line {}: invalid tool number '{}'", lineno + 1, &t_name[1..]))?;
    if p.tools.contains_key(&tool_num) {
        return Err(format!(
            "line {}: tool T{} is already declared — one number names one tool, and a second declaration would silently shadow the first. Give one of them its own number",
            lineno + 1, tool_num
        ));
    }
    p.tool_lines.push((tool_num, 0, lineno, raw.to_string()));

    let remaining = parts[1..].join(" ");
    let (tool_type, map) = parse_args(&remaining, lineno)?;
    // The kind word is judged (Phase 36 slice 0): `derive_tool_cuts`
    // reads it, and a word nobody knows used to parse as a flat rod.
    if !TOOL_KINDS.contains(&tool_type.as_str()) {
        return Err(format!(
            "line {}: tool T{} is a '{}', which is not a tool kind — the kind says which of the tool's surfaces cut, so a word nobody knows cannot be checked against an operation. Kinds: {}",
            lineno + 1, tool_num, tool_type, TOOL_KINDS.join(", ")
        ));
    }
    // The tool line rejects unknown keys, the catalog grammar's
    // rule (K2 slice 0, DECISIONS.md → "K2, the tool body"). A
    // blessed behavior change: unknown keys used to vanish
    // silently, which is exactly how a declared body would
    // quietly not exist. Keys of the WRONG tool family pass here
    // so the cross-errors below can name the family instead.
    let mut unknown: Vec<&str> =
        map.keys().map(|k| k.as_str()).filter(|k| !TOOL_KEYS.contains(k)).collect();
    if !unknown.is_empty() {
        unknown.sort();
        return Err(format!(
            "line {}: tool T{} has no field named '{}' — a tool field nobody reads is a declaration that quietly does not exist, so the tool line rejects what it does not know. Known fields: {}",
            lineno + 1, tool_num, unknown.join("', '"), TOOL_KEYS.join(", ")
        ));
    }
    let dia = get_num(&map, "dia", lineno)?;

    let mut profile = None;
    if let Some(profile_filename) = map.get("profile") {
        let clean_filename = profile_filename.trim_matches('"').trim_matches('\'');
        let mut resolved_path = std::path::PathBuf::from(clean_filename);
        if !resolved_path.exists() {
            if let Some(base) = base_dir {
                let temp = base.join(clean_filename);
                if temp.exists() {
                    resolved_path = temp;
                }
            }
        }

        let svg_content = std::fs::read_to_string(&resolved_path)
            .map_err(|e| format!("line {}: failed to read tool profile '{}': {}", lineno + 1, clean_filename, e))?;
        p.sources.push(resolved_path.clone());

        let d_str = extract_svg_paths(&svg_content)
            .map_err(|e| format!("line {}: failed to extract SVG paths from '{}': {}", lineno + 1, clean_filename, e))?;

        let tokens = tokenize_svg_d(&d_str);
        let raw_points = parse_svg_path_tokens(&tokens, crate::CHORD_TOL_MM)
            .map_err(|e| format!("line {}: failed to parse SVG path in '{}': {}", lineno + 1, clean_filename, e))?;

        if raw_points.is_empty() {
            return Err(format!("line {}: SVG tool profile '{}' has no points", lineno + 1, clean_filename));
        }

        let mut min_r_idx = 0;
        let mut min_r = raw_points[0].x.abs();
        for (idx, p) in raw_points.iter().enumerate() {
            let r = p.x.abs();
            if r < min_r {
                min_r = r;
                min_r_idx = idx;
            }
        }
        let y_tip = raw_points[min_r_idx].y;

        let mut rz_points: Vec<Pt> = raw_points.iter().map(|p| {
            Pt {
                x: p.x.abs(),
                y: (p.y - y_tip).abs(),
            }
        }).collect();

        rz_points.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
        rz_points.dedup_by(|a, b| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);

        let max_r = rz_points.iter().map(|p| p.x).fold(0.0f64, |a, b| a.max(b));
        if max_r < 1e-9 {
            return Err(format!("line {}: SVG tool profile '{}' has zero radius", lineno + 1, clean_filename));
        }
        let scale_factor = (dia / 2.0) / max_r;
        for p in &mut rz_points {
            p.x *= scale_factor;
            p.y *= scale_factor;
        }

        profile = Some(rz_points);
    }

    // A corner rounder declares its GROUND ARC, not its catalogue
    // line item (DECISIONS.md → "the corner rounder gets an
    // operation"). `dia` is its body OD here — clearance, like
    // `pilot`, and nothing positions from either.
    // The tolerance state of a tool is a fact about that tool, so
    // it is declared rather than assumed — shared by both form
    // tools.
    let measured = match map.get("source").map(|s| s.trim_matches('"').trim_matches('\'')) {
        None | Some("catalog") => false,
        Some("measured") => true,
        Some(other) => return Err(format!(
            "line {}: tool T{} source='{}' — expected 'catalog' (the default) or 'measured'; the tolerance state of a tool is a fact about that tool, so it is declared rather than assumed",
            lineno + 1, tool_num, other
        )),
    };

    let rounder = if tool_type == "CornerRounder" {
        for key in ["angle", "angle_error", "gauge_dia", "tip_dia"] {
            if map.contains_key(key) {
                return Err(format!(
                    "line {}: tool T{} is a CornerRounder but carries chamfer-mill geometry ({}) — a ground cone belongs to a ChamferMill, whose form IS that cone",
                    lineno + 1, tool_num, key
                ));
            }
        }
        let rd = Rounder {
            radius: get_num(&map, "radius", lineno)?,
            effective_dia: get_num(&map, "effective_dia", lineno)?,
            gauge_z: get_num(&map, "gauge_z", lineno)?,
            pilot: get_num(&map, "pilot", lineno)?,
            radius_error: get_num_opt(&map, "radius_error", lineno)?.unwrap_or(0.0),
            trail_wall: get_num_opt(&map, "trail_wall", lineno)?.unwrap_or(0.0),
            trail_face: get_num_opt(&map, "trail_face", lineno)?.unwrap_or(0.0),
            measured,
        };
        check_rounder(&rd, dia, tool_num, lineno)?;
        Some(rd)
    } else {
        if map.contains_key("radius") || map.contains_key("effective_dia") || map.contains_key("pilot") {
            return Err(format!(
                "line {}: tool T{} is a {} but carries corner-rounder geometry (radius/effective_dia/pilot) — a ground arc belongs to a CornerRounder, whose form IS that arc",
                lineno + 1, tool_num, tool_type
            ));
        }
        None
    };

    // A chamfer mill declares its GROUND CONE — one measured point
    // on the flank plus the actual angle — never the catalogue's
    // sharp point (DECISIONS.md → "the chamfer tool…"). `dia` is
    // its body OD, clearance, like the rounder's.
    let chamfer = if tool_type == "ChamferMill" {
        for key in ["radius_error", "trail_wall", "trail_face"] {
            if map.contains_key(key) {
                return Err(format!(
                    "line {}: tool T{} is a ChamferMill but carries corner-rounder geometry ({}) — an arc's measurement record belongs to a CornerRounder",
                    lineno + 1, tool_num, key
                ));
            }
        }
        if !map.contains_key("tip_dia") {
            return Err(format!(
                "line {}: tool T{} declares no tip_dia — a catalogue draws the cone coming to a sharp point, but every ground tool has a flat or broken corner there, so the theoretical apex sits (tip_dia/2)/tan(angle/2) BELOW the physical tip. The flat is a clearance record and nothing positions from it, but the record must exist: measure the flat and declare it",
                lineno + 1, tool_num
            ));
        }
        let ch = Chamfer {
            angle: get_num(&map, "angle", lineno)?,
            angle_error: get_num_opt(&map, "angle_error", lineno)?.unwrap_or(0.0),
            gauge_dia: get_num(&map, "gauge_dia", lineno)?,
            gauge_z: get_num(&map, "gauge_z", lineno)?,
            tip_dia: get_num(&map, "tip_dia", lineno)?,
            measured,
        };
        check_chamfer(&ch, dia, tool_num, lineno)?;
        Some(ch)
    } else {
        if tool_type != "CornerRounder" {
            for key in ["angle", "angle_error", "gauge_dia", "tip_dia"] {
                if map.contains_key(key) {
                    return Err(format!(
                        "line {}: tool T{} is a {} but carries chamfer-mill geometry ({}) — a ground cone belongs to a ChamferMill, whose form IS that cone",
                        lineno + 1, tool_num, tool_type, key
                    ));
                }
            }
            // `gauge_z` is shared by both form tools — a gauge
            // datum names a declared form, so a tool with neither
            // record has no surface for it to be a datum OF.
            if map.contains_key("gauge_z") {
                return Err(format!(
                    "line {}: tool T{} is a {} but carries a gauge_z — a gauge datum belongs to a declared form (a CornerRounder's arc or a ChamferMill's cone), and this tool declares neither",
                    lineno + 1, tool_num, tool_type
                ));
            }
        }
        None
    };

    // The K2 body fields (DECISIONS.md → "K2, the tool body",
    // slice 0): flat keyword-value clearance records, absolute mm,
    // heights above the tip like every silhouette `h`.
    let flute_len = get_num_opt(&map, "flute_len", lineno)?;
    let shank_dia = get_num_opt(&map, "shank_dia", lineno)?;
    let neck_dia = get_num_opt(&map, "neck_dia", lineno)?;
    let neck_len = get_num_opt(&map, "neck_len", lineno)?;
    let stickout = get_num_opt(&map, "stickout", lineno)?;
    // The holder label (slice 1, fork 2A): provenance for humans
    // and future libraries, never resolved to geometry — so it is
    // stored as text and nothing numeric is derived from it.
    let holder = match map.get("holder") {
        None => None,
        Some(s) => {
            let label = s.trim_matches('"').trim_matches('\'').trim();
            if label.is_empty() {
                return Err(format!(
                    "line {}: tool T{} holder label is empty — the label is provenance, and a stock record of provenance is a record of nothing",
                    lineno + 1, tool_num
                ));
            }
            Some(label.to_string())
        }
    };
    let holder_nose_dia = get_num_opt(&map, "holder_nose_dia", lineno)?;
    let holder_nose_len = get_num_opt(&map, "holder_nose_len", lineno)?;
    let holder_body_dia = get_num_opt(&map, "holder_body_dia", lineno)?;
    let flute_count = match map.get("flute_count") {
        None => None,
        Some(s) => match s.parse::<usize>() {
            Ok(n) if n >= 1 => Some(n),
            _ => {
                return Err(format!(
                    "line {}: tool T{} flute_count '{}' must be a positive whole number — it counts the cutter's flutes (K3's chip-load lookup is its first reader)",
                    lineno + 1, tool_num, s
                ));
            }
        },
    };
    // The drawn profile's cutting band (fork 3A): absolute mm
    // above the tip like every other silhouette height — NOT in
    // the SVG's own units, because the drawing is scaled to `dia`
    // at parse and a band that rescaled with it would make two
    // fields state one number.
    let cut_bottom = get_num_opt(&map, "cut_bottom", lineno)?;
    let cut_top = get_num_opt(&map, "cut_top", lineno)?;
    // The fallback coolant (DECISIONS.md → "coolant as a modeled
    // setting…", fork 2A): a KIND, never a controller word — the
    // machine's menu owns the M-words, and validate rejects a
    // kind the resolved menu cannot say.
    let coolant = match map.get("coolant") {
        None => None,
        Some(s) => match CoolantKind::parse(s) {
            Some(k) => Some(k),
            None => {
                return Err(format!(
                    "line {}: tool T{} coolant '{}' is not a kind — coolant is stated as intent ({}), and the machine file's menu owns the M-words",
                    lineno + 1, tool_num, s, CoolantKind::ALL_NAMES
                ));
            }
        },
    };
    let tool = Tool {
        num: tool_num,
        name: tool_type,
        dia,
        profile,
        rounder,
        chamfer,
        flute_len,
        shank_dia,
        neck_dia,
        neck_len,
        stickout,
        holder,
        holder_nose_dia,
        holder_nose_len,
        holder_body_dia,
        flute_count,
        cut_bottom,
        cut_top,
        coolant,
    };
    check_body(&tool, lineno)?;
    p.tools.insert(tool_num, tool);
    Ok(())
}

/// One `run <Verb>(…)` statement, checked and appended to the part's operations.
fn parse_run_statement(p: &mut Part, rest: &str, raw: &str, lineno: usize) -> Result<(), String> {
    let (op_name, map) = parse_args(rest, lineno)?;
    let op_type = match op_name.as_str() {
        "End" => OpType::End,
        // Retired in Phase 36: the verb names the part of the
        // tool that cuts. `floor` stays the noun — the surface.
        "Floor" => return Err(format!(
            "line {}: the Floor op is renamed End — a floor is the surface, and the tool's end is what makes it. Write `run End(...)`",
            lineno + 1
        )),
        "Clear" => OpType::Clear,
        // Retired in Phase 36: `Drill` named a motion and a
        // tool at once. The motion is `points=`; the verb is
        // the part of the tool that cuts there.
        "Drill" => {
            let tool = map.get("tool").and_then(|t| t.strip_prefix('T')).and_then(|n| n.parse::<usize>().ok()).and_then(|n| p.tools.get(&n));
            let which = match tool.and_then(derive_point_contact) {
                Some(v) => format!(
                    " {}, which cuts with its {} — write `run {}(points=[...], ...)`",
                    tool.map_or(String::new(), |t| format!("T{} is {}", t.num, with_article(&t.name))), v.verb().to_lowercase(), v.verb()
                ),
                None => String::new(),
            };
            return Err(format!(
                "line {}: the Drill op is retired — visiting points is a motion, said by `points=`, and the verb names the part of the tool that cuts there: Form (a drill, a chamfer mill: the tool carries the shape) or End (an endmill plunged, its end makes the seat).{}",
                lineno + 1, which
            ));
        }
        "Side" => OpType::Side,
        "Copy" => OpType::Copy,
        "Form" => OpType::Form,
        // Reserved (Phase 36, fork 6A): the fifth contact word,
        // the back of a necked tool on a ceiling. No feature can
        // declare a ceiling yet, so the word refuses by name
        // rather than parsing into a verb with no arm.
        "Under" => return Err(format!(
            "line {}: Under is reserved — it names the back of a necked tool cutting a ceiling (a surface that faces away from the spindle), and no feature declares a ceiling yet. Every undercut row in CHECKLIST.md is unrepresentable by construction; the word lands with the first ceiling",
            lineno + 1
        )),
        // Retired in Phase 36: `Finish` was three operations
        // under one name. The refusal says which one this line
        // is, when its tool and feature are already declared.
        "Finish" => {
            let tool = map.get("tool").and_then(|t| t.strip_prefix('T')).and_then(|n| n.parse::<usize>().ok()).and_then(|n| p.tools.get(&n));
            let feature = map.get("surface").and_then(|f| p.features.get(f));
            let which = match (tool, feature) {
                (Some(t), Some(f)) => format!(
                    " This line is a {} — write `run {}(...)`",
                    contact_reason(t, f), derive_contact(t, f).verb()
                ),
                _ => String::new(),
            };
            return Err(format!(
                "line {}: the Finish op is retired — it was three operations under one name, and the verb now names the part of the tool that cuts: Side (the flank, on a wall), Copy (a ball's nose, on a surface between wall and floor), Form (a tool that carries the shape).{}",
                lineno + 1, which
            ));
        }
        // Retired in Phase 35: the word carried "raster" with
        // it, and the op works a floor however it is walked.
        "Facing" => return Err(format!(
            "line {}: the Facing op is retired — it works a floor, whatever pattern walks it, and the tool's end is what makes a floor. Write `run End(...)`",
            lineno + 1
        )),
        // The two retired op words teach their replacements
        // rather than failing as unknown (DECISIONS.md
        // "Geometry-first fixture naming…").
        "Contour" => return Err(format!(
            "line {}: the Contour op is retired — tracing a wall is side cutting, whatever the wall's cross-section. Write `run Side(tool=..., surface=<profile feature>, ...)`",
            lineno + 1
        )),
        "Pocket" => return Err(format!(
            "line {}: the Pocket op is renamed Clear — 'pocket' is an orientation word (the ProfilePocket feature), the verb is Clear. Write `run Clear(...)`",
            lineno + 1
        )),
        other => return Err(format!("line {}: unknown operation '{}'", lineno + 1, other)),
    };

    let tool_str = map.get("tool").ok_or_else(|| format!("line {}: missing 'tool' in run statement", lineno + 1))?;
    if !tool_str.starts_with('T') {
        return Err(format!("line {}: tool reference must start with 'T', found '{}'", lineno + 1, tool_str));
    }
    let tool_num = tool_str[1..].parse::<usize>().map_err(|_| format!("line {}: invalid tool reference '{}'", lineno + 1, tool_str))?;

    let mut feature_name = String::new();
    let mut drill_points = None;
    let mut cycle = None;
    let mut peck_increment = None;

    // `points=` is what makes a Form or an End a visit, so beside
    // a `surface=` the line says two motions and means one.
    if op_type.may_visit_points() && map.contains_key("points") && map.contains_key("surface") {
        return Err(format!(
            "line {}: {} follows a surface (surface=) or visits points (points=), and this line names both — the reference key is the motion, so keep the one it is",
            lineno + 1,
            with_article(&op_name)
        ));
    }
    let at_points = op_type.may_visit_points() && map.contains_key("points");
    if at_points {
        let pts_str = map.get("points").expect("at_points read the key");
        if !pts_str.starts_with('[') || !pts_str.ends_with(']') {
            return Err(format!("line {}: invalid points format, expected '[...]', found '{}'", lineno + 1, pts_str));
        }
        let inside = &pts_str[1..pts_str.len() - 1];
        let mut pts = Vec::new();
        for pt in inside.split(',') {
            let pt = pt.trim();
            if !pt.is_empty() {
                pts.push(pt.to_string());
            }
        }
        if pts.is_empty() {
            return Err(format!("line {}: {} operation requires at least one point in 'points'", lineno + 1, op_name));
        }
        drill_points = Some(pts);

        let cycle_str = map.get("cycle").ok_or_else(|| format!("line {}: missing 'cycle' in a points= run statement — a visit to points is a plunge cycle, Simple or Peck", lineno + 1))?;
        let cycle_type = match cycle_str.as_str() {
            "Simple" => CycleType::Simple,
            "Peck" => CycleType::Peck,
            other => return Err(format!("line {}: unknown cycle type '{}'", lineno + 1, other)),
        };
        cycle = Some(cycle_type);
        peck_increment = get_num_opt(&map, "peck_increment", lineno)?;
    } else {
        let feature_key = run_reference_key(op_type, at_points);
        feature_name = map.get(feature_key).ok_or_else(|| format!("line {}: missing feature reference '{}' in run statement", lineno + 1, feature_key))?.clone();
    }
    // The reference rule (Phase 22 slice 3, DECISIONS.md →
    // "Phase 22 slice 3: the reference rule…"): a run line
    // carries only the reference key its verb READS. The
    // whitelist law put every run key in one set and
    // deferred the per-verb matrix by name; this is its
    // first row, filled because a live run produced a
    // `Drill(points=[…], surface=…, profile=…)` the parser
    // accepted and nothing read — an operation aimed at one
    // feature that named three.
    for key in ["surface", "profile", "points"] {
        let reads = run_reference_key(op_type, at_points);
        if key != reads && map.contains_key(key) {
            return Err(format!(
                "line {}: {} names its feature{} in {}=; {}= is not read",
                lineno + 1,
                with_article(&op_name),
                if at_points { "s" } else { "" },
                reads,
                key
            ));
        }
    }
    // `depth` is retired as an operation input (DECISIONS.md
    // "Vertical occupancy is `extent`…"). It is rejected rather than
    // ignored: silently dropping it would emit a program cutting to
    // a different Z than the author wrote.
    if map.contains_key("depth") {
        return Err(format!(
            "line {}: `depth` is not an operation input — vertical occupancy belongs to the feature. Author `extent` on the feature — the Profile pair, the CylinderBore — or move the Plane's `z` to the finished face",
            lineno + 1
        ));
    }

    let stepover = get_num_opt(&map, "stepover", lineno)?;
    let stepdown = get_num_opt(&map, "stepdown", lineno)?;
    let scallop = get_num_opt(&map, "scallop", lineno)?;
    let (feed, feed_derive) = get_num_or_derive(&map, "feed", lineno)?;
    let plunge = get_num_opt(&map, "plunge", lineno)?;
    let rapid_z = get_num_opt(&map, "rapid_z", lineno)?;
    let clearance_z = get_num_opt(&map, "clearance_z", lineno)?;
    let (spindle, spindle_derive) = get_num_or_derive(&map, "spindle", lineno)?;

    // Finishing comp mode; engine-computed offsets are the default
    // (DECISIONS.md), control-side G41/G42 is the opt-in.
    let comp = match map.get("comp").map(|s| s.as_str()) {
        None | Some("engine") => CompMode::Engine,
        Some("control") => CompMode::Control,
        Some(other) => return Err(format!("line {}: unknown comp mode '{}' (expected 'engine' or 'control')", lineno + 1, other)),
    };

    // Output representation (DECISIONS.md "Per-op compress…"):
    // `chords` (default, unchanged output) or `arcs` (exact G02/G03
    // for circular curves). `force`/`spline` are reserved — the
    // words are recognized so the error names them as not-yet-built,
    // distinct from a genuine typo.
    let compress = match map.get("compress").map(|s| s.as_str()) {
        None | Some("chords") => CompressMode::Chords,
        Some("arcs") => CompressMode::Arcs,
        Some(m @ ("force" | "spline")) => return Err(format!("line {}: compress mode '{}' is reserved but not yet implemented (use 'chords' or 'arcs')", lineno + 1, m)),
        Some(other) => return Err(format!("line {}: unknown compress mode '{}' (expected 'chords' or 'arcs')", lineno + 1, other)),
    };

    // The cut's direction (ROADMAP item 2, slice 2). Unsaid, every pass
    // climbs, as it always has; said, the Clear, End or Side follows it.
    // A Copy or a Form has no side to say, and neither has a visit to
    // points, so the key is refused there rather than read by nothing.
    let direction = match map.get("direction").map(|s| s.as_str()) {
        None => None,
        Some("climb") => Some(Direction::Climb),
        Some("conventional") => Some(Direction::Conventional),
        Some(other) => return Err(format!("line {}: unknown direction '{}' (expected 'climb' or 'conventional')", lineno + 1, other)),
    };
    if direction.is_some() && (at_points || !matches!(op_type, OpType::Clear | OpType::End | OpType::Side)) {
        return Err(format!(
            "line {}: direction= is read by a Clear, an End or a Side, whose passes have the material on one side — {}{} has no side to say",
            lineno + 1,
            with_article(&op_name),
            if at_points { " at points" } else { "" }
        ));
    }

    // How every descent is made (ROADMAP item 2, slice 4). Unsaid, each
    // descent is the kind the plan gives it; said, every one is this kind.
    // A Copy's and a Form's descent is the operation, and so is a visit to
    // points, so the key is refused there rather than read by nothing.
    let entry = match map.get("entry").map(|s| s.as_str()) {
        None => None,
        Some("ramp") => Some(EntryKind::Ramp),
        Some("plunge") => Some(EntryKind::Plunge),
        Some("lead") => Some(EntryKind::Lead),
        Some(other) => return Err(format!("line {}: unknown entry '{}' (expected 'ramp', 'plunge' or 'lead')", lineno + 1, other)),
    };
    if entry.is_some() && (at_points || !matches!(op_type, OpType::Clear | OpType::End | OpType::Side)) {
        return Err(format!(
            "line {}: entry= names how a Clear, an End or a Side descends into its cut — {}{} has no entry to name, since its descent is the operation",
            lineno + 1,
            with_article(&op_name),
            if at_points { " at points" } else { "" }
        ));
    }

    // The lead's shape (ROADMAP item 2, slice 5). A length, and two angles
    // that cross no unit boundary. A plunge or a ramp has no lead to shape,
    // so the keys are refused beside entry=plunge or entry=ramp; a Copy, a
    // Form and a visit to points make no lead at all.
    let lead = get_num_opt(&map, "lead", lineno)?;
    let lead_angle = get_num_opt(&map, "lead_angle", lineno)?;
    let lead_descent = get_num_opt(&map, "lead_descent", lineno)?;
    if let Some(l) = lead {
        if !(l > 0.0) || !l.is_finite() {
            return Err(format!("line {}: lead={} — how far from the start the descent is made, and must be more than nothing", lineno + 1, l));
        }
    }
    if let Some(a) = lead_angle {
        if !(0.0..=90.0).contains(&a) {
            return Err(format!("line {}: lead_angle={} — 0 lies behind the start on the pass's own line, 90 off to the air side; between them is the whole range", lineno + 1, a));
        }
    }
    if let Some(d) = lead_descent {
        if !(0.0..90.0).contains(&d) {
            return Err(format!("line {}: lead_descent={} — degrees below flat, 0 up to but not including 90; a lead that comes straight down is a plunge", lineno + 1, d));
        }
    }
    if lead.is_some() || lead_angle.is_some() || lead_descent.is_some() {
        if at_points || !matches!(op_type, OpType::Clear | OpType::End | OpType::Side) {
            return Err(format!(
                "line {}: lead=, lead_angle= and lead_descent= shape a lead, which a Clear, an End or a Side makes — {}{} makes none, since its descent is the operation",
                lineno + 1,
                with_article(&op_name),
                if at_points { " at points" } else { "" }
            ));
        }
        if let Some(k @ (EntryKind::Plunge | EntryKind::Ramp)) = entry {
            return Err(format!("line {}: entry={} beside a lead key — a {} has no lead to shape; entry=lead, or drop the lead keys", lineno + 1, k.word(), k.word()));
        }
    }

    // The rows' direction (ROADMAP item 2, slice 3): degrees from +X, so
    // it crosses no unit boundary. Rows have no sense — 135deg lays the
    // same rows as -45 — so the half-turn is the whole range, and a number
    // outside it is refused rather than folded in silence. Which patterns
    // take it is `validate`'s law; here only the verb: a Side, a Copy, a
    // Form and a visit to points cut no rows.
    let angle = get_num_opt(&map, "angle", lineno)?;
    if let Some(a) = angle {
        if !(0.0..180.0).contains(&a) || !a.is_finite() {
            return Err(format!("line {}: angle={} — the rows' direction in degrees from +X, 0 up to but not including 180 (rows have no sense: 135 lays the rows -45 would)", lineno + 1, a));
        }
        if at_points || !matches!(op_type, OpType::Clear | OpType::End) {
            return Err(format!(
                "line {}: angle= is read by a Clear or an End cutting rows — {}{} cuts no rows",
                lineno + 1,
                with_article(&op_name),
                if at_points { " at points" } else { "" }
            ));
        }
    }

    // Roughing/semi-finish allowance (DECISIONS.md "stock_to_leave
    // first slice…"): presence tags the op as leaving stock. Only
    // field-shape rules live in validate(); the ceiling-incl-cusps
    // budget is computed where each surface's pass math lives.
    let stock_to_leave = get_num_opt(&map, "stock_to_leave", lineno)?;
    let wall_standoff = get_num_opt(&map, "wall_standoff", lineno)?.unwrap_or(0.0);
    let face_standoff = get_num_opt(&map, "face_standoff", lineno)?.unwrap_or(0.0);
    let tip_clearance = get_num_opt(&map, "tip_clearance", lineno)?;

    // Pass selection (DECISIONS.md "adaptive clearing…", slice 1):
    // `rest` is built; `adaptive` is reserved for the
    // engagement-bounded trochoidal tier — recognized so the error
    // names it as not-yet-built, distinct from a typo (the
    // compress force/spline precedent).
    let strategy = match map.get("strategy").map(|s| s.as_str()) {
        None => None,
        Some("rest") => Some(ClearStrategy::Rest),
        Some("regions") => Some(ClearStrategy::Regions),
        Some(m @ "adaptive") => return Err(format!(
            "line {}: strategy '{}' is reserved for the engagement-bounded trochoidal tier but not yet implemented (use 'rest' or 'regions')",
            lineno + 1, m
        )),
        Some(other) => return Err(format!(
            "line {}: unknown strategy '{}' (expected 'rest' or 'regions')",
            lineno + 1, other
        )),
    };
    let pattern = match map.get("pattern").map(|s| s.as_str()) {
        None => None,
        Some("offset") => Some(Pattern::Offset),
        Some("zigzag") => Some(Pattern::Zigzag),
        Some("oneway") => Some(Pattern::Oneway),
        Some("spine") => Some(Pattern::Spine),
        Some("raster") => Some(Pattern::Raster),
        Some("held") => Some(Pattern::Held),
        // The word it was born under (Phase 30), retired in
        // Phase 35: refused by name, never read as an alias — one
        // pattern, one word.
        Some("mow") => return Err(format!(
            "line {}: pattern=mow is now pattern=spine — the same pattern under the name it kept",
            lineno + 1
        )),
        Some(other) => return Err(format!(
            "line {}: unknown pattern '{}' (expected 'offset', 'zigzag', 'oneway', 'spine', 'held' or 'raster' — `nacs --patterns` lays them side by side)",
            lineno + 1, other
        )),
    };
    // The held ceiling (Phase 62 slice 1): degrees, so it crosses
    // no unit boundary; its laws are `validate`'s.
    let engagement = get_num_opt(&map, "engagement", lineno)?;
    let voxel_cell = get_num_opt(&map, "voxel_cell", lineno)?;
    let cover = get_num_opt(&map, "cover", lineno)?;
    if let Some(c) = cover {
        if !(0.0..=100.0).contains(&c) {
            return Err(format!("line {}: cover={} — the least share of the floor this op must reach, in percent, between 0 and 100", lineno + 1, c));
        }
    }

    // The restart ladder's rung (DECISIONS.md → "N-words…",
    // fork 2A): a positive whole number, the flute_count mold.
    let n = match map.get("n") {
        None => None,
        Some(s) => match s.parse::<usize>() {
            Ok(v) if v >= 1 => Some(v),
            _ => {
                return Err(format!(
                    "line {}: n '{}' must be a positive whole number — it is the restart ladder's label, emitted as N{} on this op's first line",
                    lineno + 1, s, s
                ));
            }
        },
    };
    // The per-op coolant override (DECISIONS.md → "coolant as a
    // modeled setting…", fork 2A) — a KIND, like the tool's
    // fallback; the machine's menu owns the words.
    let coolant = match map.get("coolant") {
        None => None,
        Some(s) => match CoolantKind::parse(s) {
            Some(k) => Some(k),
            None => {
                return Err(format!(
                    "line {}: coolant '{}' is not a kind — coolant is stated as intent ({}), and the machine file's menu owns the M-words",
                    lineno + 1, s, CoolantKind::ALL_NAMES
                ));
            }
        },
    };

    // The whitelist law, the run line's turn (DECISIONS.md →
    // "the unread-key silences…", fork 2A): ONE set, the union
    // of every key any op reads, because the parser reads them
    // all into one `Operation` regardless of verb — the found
    // hazard this thread exists for was `stok_to_leave=0.5`
    // parsing clean and cutting to size with no allowance.
    // Whether a verb CONSUMES a passed key is the semantic
    // matrix, deferred by name in the design entry; `depth`'s
    // pointed error fired above.
    reject_unknown_keys(&map, &RUN_KEYS, &[], &format!("run {}", op_name), lineno)?;

    p.op_lines.push((0, lineno));
    p.op_texts.push(raw.to_string());
    p.operations.push(Operation {
        op_type,
        tool_num,
        feature_name,
        stepover,
        stepdown,
        scallop,
        feed,
        plunge,
        rapid_z,
        clearance_z,
        spindle,
        feed_derive,
        spindle_derive,
        // Resolved later, in `resolve_derived_words`.
        chip_thinning: None,
        pass_feeds: None,
        drill_points,
        cycle,
        peck_increment,
        comp,
        compress,
        direction,
        stock_to_leave,
        wall_standoff,
        face_standoff,
        tip_clearance,
        strategy,
        pattern,
        engagement,
        angle,
        entry,
        lead,
        lead_angle,
        lead_descent,
        cover,
        voxel_cell,
        coolant,
        n,
    });
    Ok(())
}

/// One single-line `feature <Name> <Kind>(…)` statement. The multi-line kinds
/// (a Face, a Profile or a Contour with its points on the lines after) open
/// a reading in `parse_into` instead.
fn parse_feature_statement(p: &mut Part, f_name: String, remaining: &str, lineno: usize, base_dir: Option<&std::path::Path>) -> Result<(), String> {
    let (f_type, map) = parse_args(&remaining, lineno)?;
    // The whitelist law, per variant: the one table above
    // (`FEATURE_KEYS`), read here and offered by the
    // window's form; an unknown VARIANT skips the check and
    // falls to the dispatch's own error-by-name.
    let sets = feature_keys(&f_type);
    if let Some((known, tolerated)) = sets {
        reject_unknown_keys(&map, known, tolerated, &format!("feature '{}' ({})", f_name, f_type), lineno)?;
    }
    match f_type.as_str() {
        "Plane" => {
            let z = get_num(&map, "z", lineno)?;
            p.features.insert(f_name, Feature::Plane { z });
        }
        "CylinderBore" => {
            let dia = get_num(&map, "dia", lineno)?;
            let extent = get_extent(&map, "depth", "CylinderBore", lineno)?;
            let x = get_num(&map, "x", lineno)?;
            let y = get_num(&map, "y", lineno)?;
            // The rim (Phase 45): omitted, the stock top —
            // `derive_contour_top` resolves it, never here.
            let z = get_num_opt(&map, "z", lineno)?;
            let edge_round = get_num_opt(&map, "edge_round", lineno)?;
            let chamfer = get_chamfer_spec(&map, lineno)?;
            let csink = get_csink_spec(&map, dia, lineno)?;
            let cbore = get_cbore_spec(&map, dia, extent, lineno)?;
            p.features.insert(f_name, Feature::CylinderBore { dia, extent, x, y, z, edge_round, chamfer, csink, cbore });
        }
        "TiltedBore" => {
            let dia = get_num(&map, "dia", lineno)?;
            let extent = get_extent(&map, "depth", "TiltedBore", lineno)?;
            let x = get_num(&map, "x", lineno)?;
            let y = get_num(&map, "y", lineno)?;
            // Both angles in degrees, like `rotation` and
            // `chamfer_angle` — never unit-scaled. The
            // ranges are validate()'s.
            let tilt = get_num(&map, "tilt", lineno)?;
            let heading = get_num(&map, "heading", lineno)?;
            p.features.insert(f_name, Feature::TiltedBore { dia, extent, x, y, tilt, heading });
        }
        "Sphere" => {
            let dia = get_num(&map, "dia", lineno)?;
            let x = get_num(&map, "x", lineno)?;
            let y = get_num(&map, "y", lineno)?;
            let z = get_num(&map, "z", lineno)?;
            p.features.insert(f_name, Feature::Sphere { dia, x, y, z });
        }
        "Cone" => {
            let dia = get_num(&map, "dia", lineno)?;
            let height = get_num(&map, "height", lineno)?;
            let x = get_num(&map, "x", lineno)?;
            let y = get_num(&map, "y", lineno)?;
            let z = get_num(&map, "z", lineno)?;
            p.features.insert(f_name, Feature::Cone { dia, height, x, y, z });
        }
        "Torus" => {
            let major_dia = get_num(&map, "major_dia", lineno)?;
            let minor_dia = get_num(&map, "minor_dia", lineno)?;
            let x = get_num(&map, "x", lineno)?;
            let y = get_num(&map, "y", lineno)?;
            let z = get_num(&map, "z", lineno)?;
            p.features.insert(f_name, Feature::Torus { major_dia, minor_dia, x, y, z });
        }
        "CylinderBoss" => {
            let dia = get_num(&map, "dia", lineno)?;
            let extent = get_extent(&map, "height", "CylinderBoss", lineno)?;
            let x = get_num(&map, "x", lineno)?;
            let y = get_num(&map, "y", lineno)?;
            let z = get_num(&map, "z", lineno)?;
            let edge_round = get_num_opt(&map, "edge_round", lineno)?;
            let chamfer = get_chamfer_spec(&map, lineno)?;
            p.features.insert(f_name, Feature::CylinderBoss { dia, extent, x, y, z, edge_round, chamfer });
        }
        "Mesh" => {
            // The first string-valued feature argument; the
            // path resolves like a tool's `profile` SVG above
            // (as-given first, then against the project dir).
            let file_raw = map.get("file").ok_or_else(|| {
                format!("line {}: Mesh feature requires a 'file' argument (an STL path)", lineno + 1)
            })?;
            let clean = file_raw.trim_matches('"').trim_matches('\'').to_string();
            let mut resolved = std::path::PathBuf::from(&clean);
            if !resolved.exists() {
                if let Some(base) = base_dir {
                    let temp = base.join(&clean);
                    if temp.exists() {
                        resolved = temp;
                    }
                }
            }
            let triangles = crate::mesh::read_stl(&resolved)
                .map_err(|e| format!("line {}: failed to read mesh '{}': {}", lineno + 1, clean, e))?;
            if triangles.is_empty() {
                return Err(format!("line {}: mesh '{}' contains no triangles", lineno + 1, clean));
            }
            let x = get_num_opt(&map, "x", lineno)?.unwrap_or(0.0);
            let y = get_num_opt(&map, "y", lineno)?.unwrap_or(0.0);
            let z = get_num_opt(&map, "z", lineno)?.unwrap_or(0.0);
            p.features.insert(f_name, Feature::Mesh { triangles, x, y, z, file: clean });
        }
        "SplineRevolve" => {
            // A string-valued feature argument like Mesh's; the
            // `.spline` path resolves the same way (as-given
            // first, then against the project dir).
            let file_raw = map.get("file").ok_or_else(|| {
                format!("line {}: SplineRevolve feature requires a 'file' argument (a .spline meridian path)", lineno + 1)
            })?;
            let clean = file_raw.trim_matches('"').trim_matches('\'').to_string();
            let mut resolved = std::path::PathBuf::from(&clean);
            if !resolved.exists() {
                if let Some(base) = base_dir {
                    let temp = base.join(&clean);
                    if temp.exists() {
                        resolved = temp;
                    }
                }
            }
            let meridian = crate::nurbs::read_spline(&resolved)
                .map_err(|e| format!("line {}: failed to read spline meridian '{}': {}", lineno + 1, clean, e))?;
            let x = get_num_opt(&map, "x", lineno)?.unwrap_or(0.0);
            let y = get_num_opt(&map, "y", lineno)?.unwrap_or(0.0);
            let z = get_num_opt(&map, "z", lineno)?.unwrap_or(0.0);
            p.features.insert(f_name, Feature::SplineRevolve { meridian, x, y, z, file: clean });
        }
        "SplineRevolveFunnel" => {
            // The cap's parse arm minus `z`: a cavity's mouth
            // sits in the stock-top plane by construction, so
            // there is no base offset to author (the
            // throat/funnel precedent).
            let file_raw = map.get("file").ok_or_else(|| {
                format!("line {}: SplineRevolveFunnel feature requires a 'file' argument (a .spline meridian path)", lineno + 1)
            })?;
            let clean = file_raw.trim_matches('"').trim_matches('\'').to_string();
            let mut resolved = std::path::PathBuf::from(&clean);
            if !resolved.exists() {
                if let Some(base) = base_dir {
                    let temp = base.join(&clean);
                    if temp.exists() {
                        resolved = temp;
                    }
                }
            }
            let meridian = crate::nurbs::read_spline(&resolved)
                .map_err(|e| format!("line {}: failed to read spline meridian '{}': {}", lineno + 1, clean, e))?;
            let x = get_num_opt(&map, "x", lineno)?.unwrap_or(0.0);
            let y = get_num_opt(&map, "y", lineno)?.unwrap_or(0.0);
            p.features.insert(f_name, Feature::SplineRevolveFunnel { meridian, x, y, file: clean });
        }
        "NurbsPatch" => {
            // A string-valued feature argument like Mesh's; the
            // `.patch` path resolves the same way (as-given
            // first, then against the project dir).
            let file_raw = map.get("file").ok_or_else(|| {
                format!("line {}: NurbsPatch feature requires a 'file' argument (a .patch surface path)", lineno + 1)
            })?;
            let clean = file_raw.trim_matches('"').trim_matches('\'').to_string();
            let mut resolved = std::path::PathBuf::from(&clean);
            if !resolved.exists() {
                if let Some(base) = base_dir {
                    let temp = base.join(&clean);
                    if temp.exists() {
                        resolved = temp;
                    }
                }
            }
            let surface = crate::nurbs::read_patch(&resolved)
                .map_err(|e| format!("line {}: failed to read patch surface '{}': {}", lineno + 1, clean, e))?;
            let x = get_num_opt(&map, "x", lineno)?.unwrap_or(0.0);
            let y = get_num_opt(&map, "y", lineno)?.unwrap_or(0.0);
            let z = get_num_opt(&map, "z", lineno)?.unwrap_or(0.0);
            p.features.insert(f_name, Feature::NurbsPatch { surface, x, y, z, file: clean });
        }
        other => {
            return Err(format!("line {}: unknown feature type '{}'", lineno + 1, other));
        }
    }
    Ok(())
}

pub fn parse_into(src: &str, p: &mut Part, base_dir: Option<&std::path::Path>) -> Result<(), String> {
    let mut reading_contour_name: Option<String> = None;
    // A `stock Profile(` block in progress (Phase 54 slice 5a): the opener's
    // keys, then its points until the `)`.
    let mut reading_stock: Option<(f64, f64, Option<String>, usize)> = None;
    let mut stock_points: Vec<Pt> = Vec::new();
    let mut contour_points = Vec::new();
    // The 3D-wire pair collects here instead: same block form, one more
    // number per line (`x y z` — the wire carries its own Z range).
    let mut wire_points: Vec<Pt3> = Vec::new();
    // The opener's args, held until the block's ')' closes the feature.
    let mut contour_z: Option<f64> = None;
    let mut contour_extent: f64 = 0.0;
    let mut contour_edge_round: Option<f64> = None;
    let mut contour_chamfer: Option<ChamferSpec> = None;
    // The points' own restatements of the leg (slice 9), by point index.
    let mut contour_marks: Vec<ChamferMark> = Vec::new();
    let mut contour_is_boss = false;
    let mut contour_is_wire = false;
    // A `Face` block: `loop` opens each boundary, points follow (Phase 28).
    let mut face_z: Option<f64> = None;
    let mut face_loops: Vec<Vec<Pt>> = Vec::new();

    for (lineno, raw) in src.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() { continue; }

        if let Some((h, z, ref material, opened)) = reading_stock {
            let closes = line.contains(')');
            let clean = line.replace(')', "");
            let tok: Vec<&str> = clean.split_whitespace().collect();
            if !tok.is_empty() {
                if tok.len() != 2 {
                    return Err(format!("line {}: a stock Profile point needs 2 numbers (x y)", lineno + 1));
                }
                stock_points.push(Pt { x: num(tok[0], lineno)?, y: num(tok[1], lineno)? });
            }
            if closes {
                let points = std::mem::take(&mut stock_points);
                if points.len() < 3 {
                    return Err(format!("line {}: stock Profile needs at least 3 points", opened + 1));
                }
                // CCW as a pocket's loop (the blessed convention), so a grown
                // blank grows: the offsetting crate reads the orientation.
                if signed_area(&points) <= 0.0 {
                    return Err(format!("line {}: stock Profile's outline winds clockwise (or has no area) — wind it counter-clockwise, as a pocket's loop", opened + 1));
                }
                let (x0, y0) = points.iter().fold((f64::INFINITY, f64::INFINITY), |(a, b), q| (a.min(q.x), b.min(q.y)));
                let (x1, y1) = points.iter().fold((f64::NEG_INFINITY, f64::NEG_INFINITY), |(a, b), q| (a.max(q.x), b.max(q.y)));
                p.stock = Stock { shape: StockShape::Profile { points }, w: x1 - x0, l: y1 - y0, h, x: x0, y: y0, z, material: material.clone() };
                reading_stock = None;
            }
            continue;
        }

        if let Some(ref name) = reading_contour_name {
            // One point-line rule for both block kinds: the planar profiles
            // take `x y`, the 3D-wire pair takes `x y z` — each rejects the
            // other's arity so a missing (or stray) Z is loud, never a shifted
            // coordinate.
            let mut push_point = |tok: &[&str]| -> Result<(), String> {
                if face_z.is_some() {
                    if tok == ["loop"] {
                        face_loops.push(Vec::new());
                        return Ok(());
                    }
                    if tok.len() != 2 {
                        return Err(format!("line {}: a Face's loop point needs 2 numbers (x y), or the word `loop` to open the next boundary", lineno + 1));
                    }
                    let Some(current) = face_loops.last_mut() else {
                        return Err(format!("line {}: a Face's points belong to a loop — open the outer boundary with a `loop` line first", lineno + 1));
                    };
                    current.push(Pt { x: num(tok[0], lineno)?, y: num(tok[1], lineno)? });
                    return Ok(());
                }
                if contour_is_wire {
                    if tok.len() != 3 {
                        return Err(format!("line {}: contour wire point needs 3 numbers (x y z — the wire carries its own Z range)", lineno + 1));
                    }
                    wire_points.push(Pt3 { x: num(tok[0], lineno)?, y: num(tok[1], lineno)?, z: num(tok[2], lineno)? });
                } else {
                    if tok.len() < 2 {
                        return Err(format!("line {}: contour point needs 2 numbers", lineno + 1));
                    }
                    // A planar loop point may restate the chamfer's leg for
                    // the edges from it on (slice 9): `chamfer_depth=` or
                    // `chamfer_width=`, 0 for edges left unbroken. The angle
                    // is the loop's, so a point cannot carry one.
                    if let Some(word) = tok.get(2) {
                        if tok.len() > 3 {
                            return Err(format!("line {}: a loop point is `x y`, with at most one restated leg after it (chamfer_depth= or chamfer_width=) — found {} words", lineno + 1, tok.len()));
                        }
                        let (key, value) = word.split_once('=').ok_or_else(|| format!("line {}: a planar loop point needs 2 numbers (x y), followed at most by chamfer_depth= or chamfer_width= for the edges from it on; `{word}` is neither", lineno + 1))?;
                        let v = num(value, lineno)?;
                        let leg = match key {
                            "chamfer_depth" | "chamfer_width" if v < 0.0 => {
                                return Err(format!("line {}: {key}={v} on a loop point — a leg is positive, or 0 to leave the edges from this point unbroken", lineno + 1));
                            }
                            "chamfer_depth" if v == 0.0 => None,
                            "chamfer_width" if v == 0.0 => None,
                            "chamfer_depth" => Some(ChamferLeg::Depth(v)),
                            "chamfer_width" => Some(ChamferLeg::Width(v)),
                            "chamfer_angle" => {
                                return Err(format!("line {}: chamfer_angle on a loop point — the angle is the loop's, one tool cuts one angle; a point restates a leg (chamfer_depth= or chamfer_width=) or nothing", lineno + 1));
                            }
                            _ => {
                                return Err(format!("line {}: a loop point is `x y`, optionally followed by chamfer_depth= or chamfer_width= for the edges from it on; `{key}=` is neither", lineno + 1));
                            }
                        };
                        if contour_chamfer.is_none() {
                            return Err(format!("line {}: this loop point restates a chamfer the feature does not declare — put chamfer_depth= (or chamfer_width=) on the feature line as the loop's own leg, and 0 on the points whose edges stay unbroken", lineno + 1));
                        }
                        contour_marks.push(ChamferMark { at: contour_points.len(), leg });
                    }
                    contour_points.push(Pt { x: num(tok[0], lineno)?, y: num(tok[1], lineno)? });
                }
                Ok(())
            };
            if line.contains(')') {
                let clean_line = line.replace(')', "").trim().to_string();
                if !clean_line.is_empty() {
                    let tok: Vec<&str> = clean_line.split_whitespace().collect();
                    push_point(&tok)?;
                }
                p.features.insert(name.clone(), if let Some(z) = face_z.take() {
                    Feature::Face { z, loops: std::mem::take(&mut face_loops) }
                } else if contour_is_wire {
                    let taken = std::mem::take(&mut wire_points);
                    if contour_is_boss {
                        Feature::ContourBoss { points: taken, extent: contour_extent }
                    } else {
                        Feature::ContourPocket { points: taken, extent: contour_extent }
                    }
                } else {
                    let taken = std::mem::take(&mut contour_points);
                    let marks = std::mem::take(&mut contour_marks);
                    if contour_is_boss {
                        Feature::ProfileBoss { points: taken, z: contour_z, extent: contour_extent, edge_round: contour_edge_round, chamfer: contour_chamfer, chamfer_marks: marks }
                    } else {
                        Feature::ProfilePocket { points: taken, z: contour_z, extent: contour_extent, edge_round: contour_edge_round, chamfer: contour_chamfer, chamfer_marks: marks }
                    }
                });
                reading_contour_name = None;
            } else {
                let tok: Vec<&str> = line.split_whitespace().collect();
                push_point(&tok)?;
            }
            continue;
        }

        let first_space = line.find(' ').unwrap_or(line.len());
        let keyword = &line[..first_space];
        let rest = line[first_space..].trim();

        match keyword {
            "units" => {
                p.header_lines.push(("units".to_string(), 0, lineno, raw.to_string()));
                match rest {
                    "mm" => p.units_mm = true,
                    "inch" => p.units_mm = false,
                    other => {
                        return Err(format!("line {}: unknown units '{}' (expected 'mm' or 'inch')", lineno + 1, other));
                    }
                }
            }
            "feed" => {
                p.header_lines.push(("feed".to_string(), 0, lineno, raw.to_string()));
                p.global_feed = num(rest, lineno)?;
            }
            "plunge" => {
                p.header_lines.push(("plunge".to_string(), 0, lineno, raw.to_string()));
                p.global_plunge = num(rest, lineno)?;
            }
            "rapid_z" => {
                p.header_lines.push(("rapid_z".to_string(), 0, lineno, raw.to_string()));
                p.global_rapid_z = num(rest, lineno)?;
            }
            "clearance_z" => {
                p.header_lines.push(("clearance_z".to_string(), 0, lineno, raw.to_string()));
                p.global_clearance_z = num(rest, lineno)?;
            }
            "spindle" => {
                p.header_lines.push(("spindle".to_string(), 0, lineno, raw.to_string()));
                p.global_spindle = num(rest, lineno)?;
            }
            "engagement" => match rest {
                "advise" => p.engagement = EngagementMode::Advise,
                "gate" => p.engagement = EngagementMode::Gate,
                other => {
                    return Err(format!(
                        "line {}: unknown engagement mode '{}' (expected 'advise' or 'gate': advise voices the engagement linter's findings as warnings, gate rejects the program on them)",
                        lineno + 1, other
                    ));
                }
            },
            // The fifth gate directive. `reach` was free in this grammar —
            // the only other `reach=` in the codebase is an
            // `--assembly-report` CLI input, a different surface with no
            // shared parser — so the word that names the finding family is
            // also the word that governs it.
            "reach" => match rest {
                "advise" => p.reach_mode = ReachMode::Advise,
                "gate" => p.reach_mode = ReachMode::Gate,
                other => {
                    return Err(format!(
                        "line {}: unknown reach mode '{}' (expected 'advise' or 'gate': advise voices the reach certificate as warnings, gate rejects the program on the same findings)",
                        lineno + 1, other
                    ));
                }
            },
            // The sixth gate directive (Phase 15 slice 4, fork 4 taken at
            // the DRAFT's blessing): the machine envelope's two voices.
            "envelope" => match rest {
                "advise" => p.envelope_mode = EnvelopeMode::Advise,
                "gate" => p.envelope_mode = EnvelopeMode::Gate,
                other => {
                    return Err(format!(
                        "line {}: unknown envelope mode '{}' (expected 'advise' or 'gate': advise voices the machine-envelope findings as warnings, gate rejects the program on the same findings)",
                        lineno + 1, other
                    ));
                }
            },
            "deviation" => match rest {
                "advise" => p.deviation = DeviationMode::Advise,
                "gate" => p.deviation = DeviationMode::Gate,
                other => return Err(format!("line {}: unknown deviation mode '{}' (expected 'advise' or 'gate')", lineno + 1, other)),
            },
            "entry" => match rest {
                "advise" => p.entry = EntryMode::Advise,
                "gate" => p.entry = EntryMode::Gate,
                other => {
                    return Err(format!(
                        "line {}: unknown entry mode '{}' (expected 'advise' or 'gate': advise voices the plunge-entry flag as warnings, gate rejects the program on the same findings)",
                        lineno + 1, other
                    ));
                }
            },
            "work_offset" => {
                // The datum SELECTION (DECISIONS.md → "work offsets…",
                // forks 1A/2A): one register, one per program. The
                // datum-SETTING spellings reject by name — that act is work
                // positioning wearing offset clothes, tenet 5's exact
                // conflation — and G53 is machine coordinates, not a work
                // offset at all. Extended-family tokens parse by SHAPE here;
                // whether the machine's menu says them is validate's check.
                if p.work_offset.is_some() {
                    return Err(format!("line {}: duplicate 'work_offset' line — the program's one coordinate space rides one register (per-op selection is gated on the multi-datum stock model, by design)", lineno + 1));
                }
                let reg = rest.trim();
                p.work_offset = Some(match reg {
                    "" => return Err(format!("line {}: 'work_offset' needs a register, e.g. 'work_offset G55'", lineno + 1)),
                    "G54" | "G55" | "G56" | "G57" | "G58" | "G59" => {
                        WorkOffset::Standard(reg[1..].parse().expect("matched literal digits"))
                    }
                    "G92" | "G10" | "G52" => {
                        return Err(format!(
                            "line {}: {} SETS a datum — that is work positioning wearing offset clothes, and where a register points is the operator's setup sheet, never this program's claim. work_offset SELECTS a pre-defined register: G54–G59, or the machine's declared extended family",
                            lineno + 1, reg
                        ));
                    }
                    "G53" => {
                        return Err(format!(
                            "line {}: G53 is the MACHINE coordinate system, not a work offset — there is no register to select. Pick G54–G59, or the machine's declared extended family",
                            lineno + 1
                        ));
                    }
                    _ => {
                        // The extended shape: <word ending in P><number>,
                        // e.g. G54.1P7, G154P12.
                        let split = reg.rfind('P').map(|i| reg.split_at(i + 1));
                        match split {
                            Some((family, num)) if family.starts_with('G') && !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) => {
                                WorkOffset::Extended { family: family.to_string(), p: num.parse().map_err(|_| format!("line {}: work_offset register number '{}' is out of range", lineno + 1, num))? }
                            }
                            _ => {
                                return Err(format!(
                                    "line {}: work_offset '{}' is not a register — say G54–G59, or an extended register like G54.1P7 (sayable when the machine file declares its family via `extended_offsets`)",
                                    lineno + 1, reg
                                ));
                            }
                        }
                    }
                });
            }
            // Where the selected register POINTS, declared for the
            // certificates (Phase 15 slice 1, DECISIONS.md → "Phase 15
            // opens: the envelope DRAFT…", fork 1): the machine coordinates
            // of the work XY zero. Declaring is not SETTING — the G92/G10/
            // G52 rejections above stand, nothing new is emitted, and the
            // travel check is the one reader. One per program, like the one
            // register it locates; no z, by fork 1's Z ruling.
            "datum" => {
                if p.datum.is_some() {
                    return Err(format!("line {}: duplicate 'datum' line — the program's one register points one place", lineno + 1));
                }
                let (mut x, mut y): (Option<f64>, Option<f64>) = (None, None);
                for tok in rest.split_whitespace() {
                    if let Some(v) = tok.strip_prefix("x=") {
                        x = Some(v.parse().map_err(|_| format!("line {}: datum x '{}' is not a number", lineno + 1, v))?);
                    } else if let Some(v) = tok.strip_prefix("y=") {
                        y = Some(v.parse().map_err(|_| format!("line {}: datum y '{}' is not a number", lineno + 1, v))?);
                    } else if tok.starts_with("z=") {
                        return Err(format!(
                            "line {}: datum takes no z — the machine-frame Z of an emitted move rides the controller's H VALUE, a length this project does not model, so a declared z would feed no check (DECISIONS.md → 'Phase 15 opens: the envelope DRAFT…')",
                            lineno + 1
                        ));
                    } else {
                        return Err(format!("line {}: unknown datum field '{}' — datum takes x= and y=, the machine coordinates of the work XY zero", lineno + 1, tok));
                    }
                }
                match (x, y) {
                    (Some(x), Some(y)) => p.datum = Some((x, y)),
                    _ => {
                        return Err(format!(
                            "line {}: datum needs both axes, e.g. `datum x=-500.0 y=-200.0` — the machine coordinates of the work XY zero, off the operator's setup sheet",
                            lineno + 1
                        ));
                    }
                }
            }
            "machine" => {
                // One `machine <file>` line, resolved like the SVG tool
                // profile (literal path, then base_dir join). A program
                // declares N tools and runs on exactly ONE machine, so a
                // second line rejects; a missing file is a hard error, never
                // a silent fall-through to the undeclared machine — an
                // author who declared a machine meant it (DECISIONS.md "K1,
                // the machine…", fork 1B).
                if p.machine.is_some() {
                    return Err(format!("line {}: duplicate 'machine' line — a program runs on exactly one machine", lineno + 1));
                }
                if rest.is_empty() {
                    return Err(format!("line {}: 'machine' needs a file, e.g. 'machine shop.machine'", lineno + 1));
                }
                let clean_filename = rest.trim_matches('"').trim_matches('\'');
                let mut resolved_path = std::path::PathBuf::from(clean_filename);
                if !resolved_path.exists() {
                    if let Some(base) = base_dir {
                        let temp = base.join(clean_filename);
                        if temp.exists() {
                            resolved_path = temp;
                        }
                    }
                }
                let machine_src = std::fs::read_to_string(&resolved_path)
                    .map_err(|e| format!("line {}: failed to read machine file '{}': {}", lineno + 1, clean_filename, e))?;
                p.sources.push(resolved_path.clone());
                let machine = parse_machine(&machine_src)
                    .map_err(|e| format!("line {}: machine file '{}': {}", lineno + 1, clean_filename, e))?;
                p.machine = Some(machine);
            }
            "stock" => {
                // The profile blank (Phase 54 slice 5a): a block, as a
                // ProfilePocket is — the opener's `)` is on a later line.
                if rest.trim_start().starts_with("Profile(") && !rest.contains(')') {
                    let (_, map) = parse_args(&format!("{})", rest.trim()), lineno)?;
                    reject_unknown_keys(&map, &STOCK_PROFILE_KEYS, &[], "stock Profile", lineno)?;
                    let material = match map.get("material") {
                        None => None,
                        Some(m) => {
                            let clean = m.trim_matches('"').trim_matches('\'');
                            if clean.is_empty() {
                                return Err(format!("line {}: stock material= needs a name", lineno + 1));
                            }
                            Some(clean.to_string())
                        }
                    };
                    p.stock_line = Some((0, lineno, raw.to_string()));
                    reading_stock = Some((get_num(&map, "h", lineno)?, get_num_opt(&map, "z", lineno)?.unwrap_or(0.0), material, lineno));
                    stock_points.clear();
                    continue;
                }
                let (name, map) = parse_args(rest, lineno)?;
                if name != "Box" && name != "Cylinder" {
                    return Err(format!("line {}: unknown stock type '{}', only 'Box' and 'Cylinder' are supported", lineno + 1, name));
                }
                // `material=` names what the stock is made of — the workpiece
                // axis of the feeds lookup (DECISIONS.md → "K3, the
                // material…"). Optional; only a derived F/S word reads it.
                let material = match map.get("material") {
                    None => None,
                    Some(m) => {
                        let clean = m.trim_matches('"').trim_matches('\'');
                        if clean.is_empty() {
                            return Err(format!("line {}: stock material= needs a name", lineno + 1));
                        }
                        Some(clean.to_string())
                    }
                };
                // Where the statement is, for the window's form (DESIGN.md
                // §5.4): the stock is one line, edited like a tool's.
                p.stock_line = Some((0, lineno, raw.to_string()));
                p.stock = if name == "Box" {
                    reject_unknown_keys(&map, &STOCK_KEYS, &[], "stock Box", lineno)?;
                    Stock {
                        shape: StockShape::Box,
                        w: get_num(&map, "w", lineno)?,
                        l: get_num(&map, "l", lineno)?,
                        h: get_num(&map, "h", lineno)?,
                        x: get_num_opt(&map, "x", lineno)?.unwrap_or(0.0),
                        y: get_num_opt(&map, "y", lineno)?.unwrap_or(0.0),
                        z: get_num_opt(&map, "z", lineno)?.unwrap_or(0.0),
                        material,
                    }
                } else {
                    // A round blank (Phase 46 slice 1): the line says the
                    // AXIS, the struct keeps the bounding box — `StockShape`
                    // says why.
                    reject_unknown_keys(&map, &STOCK_CYLINDER_KEYS, &[], "stock Cylinder", lineno)?;
                    let dia = get_num(&map, "dia", lineno)?;
                    let cx = get_num_opt(&map, "x", lineno)?.unwrap_or(0.0);
                    let cy = get_num_opt(&map, "y", lineno)?.unwrap_or(0.0);
                    Stock {
                        shape: StockShape::Cylinder { dia },
                        w: dia,
                        l: dia,
                        h: get_num(&map, "h", lineno)?,
                        x: cx - dia / 2.0,
                        y: cy - dia / 2.0,
                        z: get_num_opt(&map, "z", lineno)?.unwrap_or(0.0),
                        material,
                    }
                };
            }
            // A keep-out (DECISIONS.md → "V2, the fixture…", fork 1A):
            // declared INLINE beside the stock, because a fixture is the same
            // kind of fact — the world the part sits in for this setup — and
            // the stock is inline. A shop LIBRARY of reusable setups is a
            // different question and belongs to V4's declaration-source-vs-
            // clipboard fork; taking a file path here would answer that fork
            // by accident, for one file kind, without the design.
            "fixture" => {
                // ONE WORD, TWO JOBS, disambiguated by a reserved name rather
                // than by a second keyword. `fixture advise|gate` is the
                // check's directive; anything else is a keep-out declaration.
                // The alternative — a separate word like `fixture_check` —
                // spends a second noun on one concept; the alternative to
                // THAT, telling them apart by token count, is the
                // silent-misparse hazard the `feeds_table` naming note warns
                // about. So the two mode words are simply not legal fixture
                // names, rejected below by name.
                let bare = rest.trim();
                if bare == "advise" || bare == "gate" {
                    p.fixture_mode = if bare == "gate" { FixtureMode::Gate } else { FixtureMode::Advise };
                    continue;
                }
                let mut it = rest.splitn(2, char::is_whitespace);
                let fixture_name = it.next().unwrap_or("").trim().to_string();
                let decl = it.next().unwrap_or("").trim();
                if fixture_name.is_empty() || decl.is_empty() {
                    return Err(format!(
                        "line {}: fixture needs a name and a shape, e.g. `fixture Vise Box(w=.., l=.., h=.., x=.., y=.., z=..)`",
                        lineno + 1
                    ));
                }
                if fixture_name == "advise" || fixture_name == "gate" {
                    return Err(format!(
                        "line {}: a fixture may not be named '{}' — those two words are the check's own directive (`fixture advise|gate`), so a keep-out called '{}' could not be told from it",
                        lineno + 1, fixture_name, fixture_name
                    ));
                }
                if p.fixtures.iter().any(|f| f.name == fixture_name) {
                    return Err(format!(
                        "line {}: fixture '{}' is declared twice — each keep-out needs its own name so a finding can say which one it hit",
                        lineno + 1, fixture_name
                    ));
                }
                let (shape_name, map) = parse_args(decl, lineno)?;
                let shape = match shape_name.as_str() {
                    "Box" => {
                        reject_unknown_keys(&map, &["w", "l", "h", "x", "y", "z"], &[], "fixture Box", lineno)?;
                        FixtureShape::Box {
                            w: get_num(&map, "w", lineno)?,
                            l: get_num(&map, "l", lineno)?,
                            h: get_num(&map, "h", lineno)?,
                        }
                    }
                    "Cylinder" => {
                        reject_unknown_keys(&map, &["dia", "h", "x", "y", "z"], &[], "fixture Cylinder", lineno)?;
                        FixtureShape::Cylinder {
                            dia: get_num(&map, "dia", lineno)?,
                            h: get_num(&map, "h", lineno)?,
                        }
                    }
                    other => {
                        return Err(format!(
                            "line {}: unknown fixture shape '{}' — v1 models keep-outs as 'Box' or 'Cylinder' (a mesh fixture is a named later slice)",
                            lineno + 1, other
                        ))
                    }
                };
                // A keep-out with no extent keeps nothing out, and a reader
                // would take the silence for clearance.
                let bad = match shape {
                    FixtureShape::Box { w, l, h } => (w <= 0.0) || (l <= 0.0) || (h <= 0.0),
                    FixtureShape::Cylinder { dia, h } => (dia <= 0.0) || (h <= 0.0),
                };
                if bad {
                    return Err(format!(
                        "line {}: fixture '{}' has a non-positive dimension — a keep-out with no extent keeps nothing out",
                        lineno + 1, fixture_name
                    ));
                }
                p.fixtures.push(Fixture {
                    name: fixture_name,
                    x: get_num_opt(&map, "x", lineno)?.unwrap_or(0.0),
                    y: get_num_opt(&map, "y", lineno)?.unwrap_or(0.0),
                    z: get_num_opt(&map, "z", lineno)?.unwrap_or(0.0),
                    shape,
                });
            }
            // A void that exists BEFORE this program starts (Phase 42 slice
            // 1; DECISIONS.md → "Phase 42 slice 1…", fork 1A): cut on
            // another setup, so the stock model reads it as air from prefix
            // 0. A statement beside `fixture` rather than a key on the void's
            // own line, because "already there" is a fact about this
            // program's place in the part's routing, not about the geometry
            // — exactly as a clamp is a `fixture` statement and not a
            // property of the stock. Which kinds may be given is
            // `validate`'s to judge, through the one home that subtracts
            // them (`stock::given_removal`), so the parser only records.
            "given" => {
                // ONE WORD, TWO JOBS (the `fixture advise|gate` precedent):
                // `given from <project> up=<axis>` is the earlier program,
                // anything else is a feature name. `from` is therefore not a
                // legal feature name on this line, by the reserved word.
                if let Some(from) = rest.trim().strip_prefix("from ").or((rest.trim() == "from").then_some("")) {
                    if p.given_from.is_some() {
                        return Err(format!(
                            "line {}: given from is declared twice — one program before this one; a chain of setups is not in the phase",
                            lineno + 1
                        ));
                    }
                    if GIVEN_FROM_DEPTH.with(|d| d.get()) > 0 {
                        return Err(format!(
                            "line {}: this project itself says `given from` — a chain of setups (a third setup) is not in the phase; only the program directly before this one is read",
                            lineno + 1
                        ));
                    }
                    let mut words = from.split_whitespace();
                    let project = words.next().unwrap_or("").trim_matches('"').trim_matches('\'').to_string();
                    let up_word = words.next();
                    let at_word = words.next();
                    let extra = words.next();
                    if project.is_empty() || up_word.is_none() || extra.is_some() {
                        return Err(format!(
                            "line {}: given from names the earlier project and the turn, e.g. `given from ../plate_top.cam up=z-` — which way the part was turned is the one fact the statement exists to say, so up= is not defaulted",
                            lineno + 1
                        ));
                    }
                    // `at=x,y,z`: where the earlier program's zero sits here
                    // once the part is turned. Unwritten, the two setups
                    // share one origin.
                    let mut at = [0.0; 3];
                    if let Some(word) = at_word {
                        let nums: Option<Vec<f64>> = word.strip_prefix("at=").map(|w| w.split(',').map(|n| n.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect()).unwrap_or(None);
                        match nums.as_deref() {
                            Some([x, y, z]) => at = [*x, *y, *z],
                            _ => {
                                return Err(format!(
                                    "line {}: given from's third word is at=x,y,z — where the earlier program's zero sits in this one once the part is turned, e.g. at=0,0,12 — not '{}'",
                                    lineno + 1, word
                                ));
                            }
                        }
                    }
                    let Some(axis_word) = up_word.and_then(|w| w.strip_prefix("up=")) else {
                        return Err(format!(
                            "line {}: given from's second word is up=<axis> (the sidecar's own word: z+ or z-), not '{}'",
                            lineno + 1, up_word.unwrap_or("")
                        ));
                    };
                    let up = UpAxis::from_word(axis_word).map_err(|e| format!("line {}: {}", lineno + 1, e))?;
                    // Resolved like a `.spline` path: as written, then against
                    // this project's directory.
                    let mut resolved = std::path::PathBuf::from(&project);
                    if !resolved.exists() {
                        if let Some(base) = base_dir {
                            let temp = base.join(&project);
                            if temp.exists() {
                                resolved = temp;
                            }
                        }
                    }
                    if !resolved.exists() {
                        return Err(format!(
                            "line {}: given from '{}': no such project — looked as written and against this project's directory",
                            lineno + 1, project
                        ));
                    }
                    GIVEN_FROM_DEPTH.with(|d| d.set(d.get() + 1));
                    let loaded = load_project(&resolved.to_string_lossy());
                    GIVEN_FROM_DEPTH.with(|d| d.set(d.get() - 1));
                    let part = match loaded {
                        Ok(part) => part,
                        Err(LoadError::Io(e)) | Err(LoadError::Parse(e)) => {
                            return Err(format!(
                                "line {}: given from '{}': the earlier program does not stand on its own — {}",
                                lineno + 1, project, e
                            ));
                        }
                    };
                    let air = crate::stock::turn_program(&part, up, at, &project);
                    p.given_from = Some(GivenFrom { project, resolved, up, at, part: Box::new(part), air });
                    continue;
                }
                let name = rest.trim();
                if name.is_empty() || name.split_whitespace().count() != 1 {
                    return Err(format!(
                        "line {}: given names exactly one feature, e.g. `given Drain` — the void is declared as a feature, and this line says it is already there when the program starts",
                        lineno + 1
                    ));
                }
                if p.given.iter().any(|g| g == name) {
                    return Err(format!(
                        "line {}: '{}' is given twice — once says it",
                        lineno + 1, name
                    ));
                }
                p.given.push(name.to_string());
            }
            // The shop's feeds table, declared IN PROGRAM TEXT so a review of
            // a `.cam` change can see which numbers it runs under — never an
            // implicit search path (DECISIONS.md → "K3, the material…",
            // fork 1). Spelled `feeds_table`, not `feeds`: one letter from
            // the existing `feed` directive is a silent-misparse hazard.
            "feeds_table" => {
                if p.shop_feeds.is_some() {
                    return Err(format!(
                        "line {}: feeds_table already declared — one shop table per program; a shop record shadows the shipped record whole, so put the shop's opinion in one file",
                        lineno + 1
                    ));
                }
                let clean = rest.trim().trim_matches('"').trim_matches('\'');
                if clean.is_empty() {
                    return Err(format!("line {}: feeds_table needs a path", lineno + 1));
                }
                // Resolved against the program directory, like tool-profile
                // SVGs.
                let mut resolved_path = std::path::PathBuf::from(clean);
                if !resolved_path.exists() {
                    if let Some(base) = base_dir {
                        let temp = base.join(clean);
                        if temp.exists() {
                            resolved_path = temp;
                        }
                    }
                }
                let text = std::fs::read_to_string(&resolved_path)
                    .map_err(|e| format!("line {}: cannot read feeds table '{}': {}", lineno + 1, clean, e))?;
                p.sources.push(resolved_path.clone());
                let table = parse_feeds(&text)
                    .map_err(|e| format!("line {}: feeds table '{}': {}", lineno + 1, clean, e))?;
                p.shop_feeds = Some(table);
            }
            "tool" => parse_tool_statement(p, rest, raw, lineno, base_dir)?,
            "feature" => {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.is_empty() {
                    return Err(format!("line {}: invalid feature declaration", lineno + 1));
                }
                let f_name = parts[0].to_string();
                p.feature_lines.push((f_name.clone(), 0, lineno, raw.to_string()));
                let remaining = parts[1..].join(" ");
                
                if remaining.starts_with("Face(") {
                    // The bounded planar face (Phase 28): the opener carries
                    // its height, and each boundary follows as a `loop` line
                    // and its points — the first the outer, the rest inner.
                    let (_, map) = parse_args(&format!("{})", remaining), lineno)?;
                    let (known, tolerated) = feature_keys("Face").expect("the table names the block variants");
                    reject_unknown_keys(&map, known, tolerated, &format!("feature '{}' (Face)", f_name), lineno)?;
                    face_z = Some(get_num(&map, "z", lineno)?);
                    face_loops.clear();
                    contour_is_wire = false;
                    reading_contour_name = Some(f_name);
                } else if remaining.starts_with("ProfilePocket(") || remaining.starts_with("ProfileBoss(") {
                    // The opener carries the feature's own geometry (`z`,
                    // `extent`) and the points follow on their own lines, so
                    // the ')' is on a later line — synthesize one so the shared
                    // `parse_args` can read the arg list.
                    let (_, map) = parse_args(&format!("{})", remaining), lineno)?;
                    contour_z = get_num_opt(&map, "z", lineno)?;
                    contour_extent = get_num(&map, "extent", lineno)?;
                    contour_edge_round = get_num_opt(&map, "edge_round", lineno)?;
                    contour_chamfer = get_chamfer_spec(&map, lineno)?;
                    let (known, tolerated) = feature_keys("ProfilePocket").expect("the table names the block variants");
                    reject_unknown_keys(
                        &map,
                        known,
                        tolerated,
                        &format!("feature '{}' ({})", f_name, if remaining.starts_with("ProfileBoss(") { "ProfileBoss" } else { "ProfilePocket" }),
                        lineno,
                    )?;
                    contour_is_boss = remaining.starts_with("ProfileBoss(");
                    contour_is_wire = false;
                    reading_contour_name = Some(f_name);
                    contour_points.clear();
                } else if remaining.starts_with("ContourPocket(") || remaining.starts_with("ContourBoss(") {
                    // The 3D-wire pair (DECISIONS.md "the 3D-wire Contour…"):
                    // same block form as the planar profiles, but the points
                    // carry their own Z and there is no datum to author.
                    let (_, map) = parse_args(&format!("{})", remaining), lineno)?;
                    if map.contains_key("z") {
                        return Err(format!(
                            "line {}: a Contour wire takes no z datum — its points carry their own Z (x y z per line, absolute), which is the geometry the word was reserved for",
                            lineno + 1
                        ));
                    }
                    contour_extent = get_num(&map, "extent", lineno)?;
                    contour_edge_round = None; // a wire has no planar top edge to break
                    contour_chamfer = None; // same reason: this op places the form at ONE Z
                    let (known, tolerated) = feature_keys("ContourPocket").expect("the table names the block variants");
                    reject_unknown_keys(
                        &map,
                        known,
                        tolerated,
                        &format!("feature '{}' ({})", f_name, if remaining.starts_with("ContourBoss(") { "ContourBoss" } else { "ContourPocket" }),
                        lineno,
                    )?;
                    contour_is_boss = remaining.starts_with("ContourBoss(");
                    contour_is_wire = true;
                    reading_contour_name = Some(f_name);
                    wire_points.clear();
                } else if remaining.starts_with("Contour(") {
                    // The migration refusal (DECISIONS.md "Geometry-first
                    // fixture naming…"), updated now the reserved word exists:
                    // side-of-material belongs to the variant, not to the
                    // winding, for wires exactly as for planar loops.
                    return Err(format!(
                        "line {}: the Contour feature is retired — side-of-material is carried by the variant now: declare ProfilePocket (a void, wound CCW) or ProfileBoss (standing material, wound CW) for a planar loop, or ContourPocket/ContourBoss for a 3D wire (x y z points — the feature the word was reserved for, landed)",
                        lineno + 1
                    ));
                } else {
                    parse_feature_statement(p, f_name, &remaining, lineno, base_dir)?;
                }
            }
            "run" => parse_run_statement(p, rest, raw, lineno)?,
            other => {
                return Err(format!("line {}: unknown key '{}'", lineno + 1, other));
            }
        }
    }
    
    if reading_contour_name.is_some() {
        return Err("end of file reached while reading contour profile points (missing closing parenthesis)".to_string());
    }
    if reading_stock.is_some() {
        return Err("end of file reached while reading the stock Profile's points (missing closing parenthesis)".to_string());
    }
    Ok(())
}

// The machine file's grammar (DECISIONS.md "K1, the machine…", fork 3B): the
// `.part` global-directive shape — one named directive per line, `#`
// comments — parsed line by line like `parse_into` itself. NAMED directives
// with unknown keys rejected: an open key-value dictionary of dialect facts
// would be a post config wearing a new extension, and posts are where
// misunderstanding accumulates. Closed sets (`spindle_word`) are enums
// matched arm-by-arm, the `comp`/`compress` precedent. Errors carry the
// machine file's own line number; the `machine` directive arm wraps them
// with the referencing file's context.
/// One decimals directive's value: an integer in the 1..=6 band. 0 rejects
/// (a dimension word with no decimals cannot carry the model's micrometers)
/// and >6 rejects (it outresolves any controller word in this dialect's
/// family) — the absurdity band, before the units-dependent honesty ceiling
/// that `parse_machine` applies once the whole file is read.
pub(super) fn parse_decimals(key: &str, rest: &str, lineno: usize) -> Result<usize, String> {
    let n: usize = rest.parse().map_err(|_| {
        format!("line {}: {} needs a whole number of decimals, e.g. '{} 4'", lineno + 1, key, key)
    })?;
    if n < 1 || n > 6 {
        return Err(format!(
            "line {}: {} {} is outside 1..=6 — 0 decimals cannot carry the model's micrometers, and more than 6 outresolves any controller word",
            lineno + 1, key, n
        ));
    }
    Ok(n)
}

pub fn parse_machine(src: &str) -> Result<Machine, String> {
    let mut m = Machine::default();
    // Decimals resolve AFTER the loop: their default depends on the declared
    // units (mm → 3, today's spelling; inch → 4, tenths of a thou), and so
    // does the honesty ceiling — and a machine file may state its lines in
    // any order, so neither can be judged at the line.
    let mut declared_coord: Option<(usize, usize)> = None; // (decimals, lineno)
    let mut declared_feed: Option<(usize, usize)> = None;
    // Travel spans are LENGTHS, so like the decimals they resolve after the
    // loop: an inch machine file states its spec sheet in the controller's
    // inches, and the file may declare `units` on any line.
    let mut declared_travel_x: Option<(f64, f64)> = None;
    let mut declared_travel_y: Option<(f64, f64)> = None;
    // Rapid rates are lengths-per-minute, so they resolve with the spans.
    let mut declared_rapid: [Option<f64>; 3] = [None, None, None];
    for (lineno, raw) in src.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() { continue; }

        let first_space = line.find(' ').unwrap_or(line.len());
        let keyword = &line[..first_space];
        let rest = line[first_space..].trim();

        match keyword {
            // Identity only. It prints nothing: emitting even an identifying
            // comment into the program would break the byte-identical promise
            // for a machine whose every field is the default.
            "name" => {
                if rest.is_empty() {
                    return Err(format!("line {}: 'name' needs a value", lineno + 1));
                }
                m.name = Some(rest.to_string());
            }
            "spindle_word" => match rest {
                "integer" => m.spindle_word = SpindleWord::Integer,
                "decimal" => m.spindle_word = SpindleWord::Decimal,
                other => {
                    return Err(format!(
                        "line {}: unknown spindle_word '{}' (expected 'integer' or 'decimal': integer emits S3000, decimal emits today's S3000.000)",
                        lineno + 1, other
                    ));
                }
            },
            // The controller's units — the output half of the `.part`
            // grammar's `units` line, same word on purpose (see OutputUnits).
            // An inch machine gets G20 and inch-converted dimension words,
            // spelled once at the formatting boundary (gcode.rs).
            "units" => match rest {
                "mm" => m.units = OutputUnits::Mm,
                "inch" => m.units = OutputUnits::Inch,
                other => {
                    return Err(format!("line {}: unknown units '{}' (expected 'mm' or 'inch')", lineno + 1, other));
                }
            },
            "coord_decimals" => declared_coord = Some((parse_decimals("coord_decimals", rest, lineno)?, lineno)),
            "feed_decimals" => declared_feed = Some((parse_decimals("feed_decimals", rest, lineno)?, lineno)),
            // The envelope's travel spans (Phase 15 slice 1, DECISIONS.md →
            // "Phase 15 opens: the envelope DRAFT…", fork 1): the axis's
            // reachable range in MACHINE coordinates, two numbers, min
            // strictly below max. The check they feed runs only when the
            // part also declares its `datum` — a span alone claims nothing.
            "travel_x" => declared_travel_x = Some(parse_travel_span("travel_x", rest, lineno)?),
            "travel_y" => declared_travel_y = Some(parse_travel_span("travel_y", rest, lineno)?),
            // The spindle ceiling (Phase 15 slice 2, fork 2): one RPM, the
            // fastest this spindle turns. An RPM never converts (the
            // `spindle_word` rule), so unlike the spans it resolves at the
            // line.
            "spindle_max" => {
                let rpm: f64 = rest.parse().map_err(|_| {
                    format!("line {}: spindle_max '{}' is not a number — the spindle's ceiling in RPM, e.g. `spindle_max 12000`", lineno + 1, rest)
                })?;
                if rpm <= 0.0 {
                    return Err(format!("line {}: spindle_max {} names no spindle — the ceiling must be positive", lineno + 1, rpm));
                }
                m.spindle_max = Some(rpm);
            }
            // The changer's pockets (Phase 15 slice 3, fork 3): a count,
            // resolved at the line like the ceiling — nothing about it
            // converts.
            "atc_pockets" => {
                let n: usize = match rest.parse() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        return Err(format!(
                            "line {}: atc_pockets '{}' is not a pocket count — a positive whole number, how many tools the changer holds, e.g. `atc_pockets 24`",
                            lineno + 1, rest
                        ));
                    }
                };
                m.atc_pockets = Some(n);
            }
            // The rapid traverse rates (Phase 16 slice 1, fork 1): the spec
            // sheet's numbers, per axis, in this file's units per minute —
            // lengths, so they resolve after the loop with the spans.
            "rapid_x" | "rapid_y" | "rapid_z" => {
                let rate: f64 = rest.parse().map_err(|_| {
                    format!("line {}: {} '{}' is not a number — the axis's rapid traverse rate in the file's units per minute, e.g. `rapid_x 36000`", lineno + 1, keyword, rest)
                })?;
                if rate <= 0.0 {
                    return Err(format!("line {}: {} {} names no traverse — the rate must be positive", lineno + 1, keyword, rate));
                }
                let slot = match keyword { "rapid_x" => 0, "rapid_y" => 1, _ => 2 };
                declared_rapid[slot] = Some(rate);
            }
            // Declined by name, not silently unknown (fork 1's Z ruling):
            // the machine-frame Z of a comp'd move rides the H register's
            // VALUE — the tool's gauge length — which this project models as
            // a register number, never as millimetres, so a Z span would be
            // checked against a guess. The revisit trigger is a declarable
            // gauge length on the assembly records.
            "travel_z" => {
                return Err(format!(
                    "line {}: travel_z is declined by name — with G43 H active the machine-frame Z of an emitted move rides the controller's H VALUE, a length this project does not model, so a Z span would gauge a guess. X and Y are the checkable axes today (DECISIONS.md → 'Phase 15 opens: the envelope DRAFT…')",
                    lineno + 1
                ));
            }
            // The coolant menu (DECISIONS.md → "coolant as a modeled
            // setting…", fork 3A as amended): eight optional named keys,
            // `coolant_<kind> <M-word>` — a kind is sayable exactly when its
            // machine names its word. The value must SPELL an M-word: the
            // menu maps intent to the controller's own vocabulary, and a
            // word the controller does not speak is a typo, not a dialect.
            // The extended work-offset family (DECISIONS.md → "work
            // offsets…", fork 3A): the vendor's register word plus the
            // register count — `extended_offsets G54.1P 48`. The word must
            // spell G + digits (with an optional dot group) + a trailing P,
            // because emission splits it there into the two-word spelling.
            "extended_offsets" => {
                let mut it = rest.split_whitespace();
                let (word, count) = match (it.next(), it.next(), it.next()) {
                    (Some(w), Some(c), None) => (w, c),
                    _ => {
                        return Err(format!(
                            "line {}: extended_offsets takes a register word and a count, e.g. `extended_offsets G54.1P 48`",
                            lineno + 1
                        ));
                    }
                };
                let body = word.strip_prefix('G').and_then(|b| b.strip_suffix('P'));
                let shape_ok = body.is_some_and(|b| {
                    !b.is_empty()
                        && b.bytes().all(|c| c.is_ascii_digit() || c == b'.')
                        && !b.starts_with('.')
                        && !b.ends_with('.')
                });
                if !shape_ok {
                    return Err(format!(
                        "line {}: extended_offsets word '{}' must spell G + digits + a trailing P (G54.1P, G154P) — emission splits the word there into the controller's two-word spelling",
                        lineno + 1, word
                    ));
                }
                let n: usize = match count.parse() {
                    Ok(n) if n >= 1 => n,
                    _ => {
                        return Err(format!(
                            "line {}: extended_offsets count '{}' must be a positive whole number — how many registers the family has",
                            lineno + 1, count
                        ));
                    }
                };
                m.extended_offsets = Some((word.to_string(), n));
            }
            key if key.starts_with("coolant_") => {
                let kind = match CoolantKind::parse(&key["coolant_".len()..]) {
                    Some(k) => k,
                    None => {
                        return Err(format!(
                            "line {}: unknown machine key '{}' — the coolant menu's keys are coolant_<kind> for {}",
                            lineno + 1, key, CoolantKind::ALL_NAMES
                        ));
                    }
                };
                let is_m_word = rest.len() >= 2
                    && rest.starts_with('M')
                    && rest[1..].bytes().all(|b| b.is_ascii_digit());
                if !is_m_word {
                    return Err(format!(
                        "line {}: {} '{}' is not an M-word — the menu maps a kind to the controller's M-code (e.g. `coolant_mist M07`)",
                        lineno + 1, key, rest
                    ));
                }
                *m.coolant.slot(kind) = Some(rest.to_string());
            }
            other => {
                return Err(format!("line {}: unknown machine key '{}'", lineno + 1, other));
            }
        }
    }
    // Undeclared decimals default by units: mm → 3 (the undeclared
    // spelling), inch → 4 (tenths of a thou — the ROADMAP's motivating
    // pairing for inch output).
    let default_decimals = match m.units {
        OutputUnits::Mm => 3,
        OutputUnits::Inch => 4,
    };
    // The honesty ceiling, and the cross-line fact this parser's validate
    // step exists for: the op emitters spell the internal dialect at 3 mm
    // decimals (a 1 micrometer grid), and the formatting boundary respells
    // those words — so a declaration finer than the grid would print
    // quantization noise dressed as position. mm carries at most 3 honest
    // decimals, inch at most 4 (one inch ulp at 4 decimals is 2.54
    // micrometers, still coarser than the grid). Lifting the ceiling is the
    // open remainder of ROADMAP "Extended precision for high-end
    // controllers": the internal spelling itself must widen first.
    let ceiling = match m.units {
        OutputUnits::Mm => 3,
        OutputUnits::Inch => 4,
    };
    for (key, declared) in [("coord_decimals", &declared_coord), ("feed_decimals", &declared_feed)] {
        if let Some((n, lineno)) = declared {
            if *n > ceiling {
                return Err(format!(
                    "line {}: {} {} outresolves the internal dialect: the emitter spells mm at 3 decimals (a 1 micrometer grid), which honestly carries at most {} {} decimals — extra digits would be quantization noise, not position (ROADMAP 'Extended precision for high-end controllers' is the remainder)",
                    lineno + 1, key, n, ceiling,
                    match m.units { OutputUnits::Mm => "mm", OutputUnits::Inch => "inch" },
                ));
            }
        }
    }
    m.coord_decimals = declared_coord.map(|(n, _)| n).unwrap_or(default_decimals);
    m.feed_decimals = declared_feed.map(|(n, _)| n).unwrap_or(default_decimals);
    // Travel spans convert at this boundary like every other length: an
    // inch machine file's spec sheet is the controller's inches, and
    // everything downstream of a parser is mm.
    let travel_scale = match m.units {
        OutputUnits::Mm => 1.0,
        OutputUnits::Inch => MM_PER_INCH,
    };
    m.travel_x = declared_travel_x.map(|(lo, hi)| (lo * travel_scale, hi * travel_scale));
    m.travel_y = declared_travel_y.map(|(lo, hi)| (lo * travel_scale, hi * travel_scale));
    m.rapid_x = declared_rapid[0].map(|r| r * travel_scale);
    m.rapid_y = declared_rapid[1].map(|r| r * travel_scale);
    m.rapid_z = declared_rapid[2].map(|r| r * travel_scale);
    Ok(m)
}

/// One travel span: two numbers, min strictly below max — a zero-width or
/// inverted span names no reachable axis and is a typo, not a machine.
pub(super) fn parse_travel_span(key: &str, rest: &str, lineno: usize) -> Result<(f64, f64), String> {
    let mut it = rest.split_whitespace();
    let (lo, hi) = match (it.next(), it.next(), it.next()) {
        (Some(a), Some(b), None) => (a, b),
        _ => {
            return Err(format!(
                "line {}: {} takes the axis's machine-coordinate range as two numbers, min then max, e.g. `{} -762 0`",
                lineno + 1, key, key
            ));
        }
    };
    let lo: f64 = lo.parse().map_err(|_| format!("line {}: {} min '{}' is not a number", lineno + 1, key, lo))?;
    let hi: f64 = hi.parse().map_err(|_| format!("line {}: {} max '{}' is not a number", lineno + 1, key, hi))?;
    if !(lo < hi) {
        return Err(format!(
            "line {}: {} range {} .. {} is empty — min must sit strictly below max",
            lineno + 1, key, lo, hi
        ));
    }
    Ok((lo, hi))
}
