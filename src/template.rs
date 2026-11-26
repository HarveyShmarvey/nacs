use crate::math::Portable as _;
// ---- template: click-a-face's text, CLI-side ---------------------------------
//
// Phase 6, slice 5 (ROADMAP item 5): "click a face -> generate an
// operation/feature template in text". This module is the TEXT — the
// window's template panel and `nacs --template <feature>` print the same
// lines, so the picture cannot say anything the CLI cannot (the phase's
// standing rule, applied once more).
//
// A template is a SKELETON, deliberately not-yet-legal: every number the
// machinist owns is a `?` placeholder, and the tool is `T?` always — a
// template that guessed a tool would guess wrong exactly when it matters
// (the drill line handed the profile tool, the ball surface handed the
// flat endmill). The template's whole claim is WHICH ops consume this
// feature kind and HOW each names it (`surface=` / `profile=` /
// `points=[...]`), and that claim is locked to validate()'s own acceptance
// by the fill-and-validate pins in `tests/template_tests.rs` — filled
// templates must PARSE — rather than by restating the acceptance rules
// here in a second home.
//
// Deliberately NOT here, named: the feature-declaration half of the
// roadmap item's "operation/feature template". A feature template needs
// dimensions and placement only CAD (or the machinist) knows, and the
// sidecar is already the feature-PROPOSING path — it reads real geometry
// and emits reviewable `.part` text. A click on an already-declared wire
// has nothing new to offer a feature declaration.

use crate::parse::{Feature, Part};

/// The suggested `run` line(s) for one declared feature, in the order a
/// shop would run them (drill before clearing, clearing before the finish
/// pass). One arm per `Feature` variant, exhaustive on purpose — a new
/// variant stops compiling here instead of silently having no template
/// (the `role_color` rule, applied to text).
pub fn op_lines(p: &Part, name: &str) -> Result<Vec<String>, String> {
    let feature = match p.features.get(name) {
        Some(f) => f,
        None => {
            let mut known: Vec<&str> = p.features.keys().map(|s| s.as_str()).collect();
            known.sort_unstable();
            return Err(format!(
                "feature '{}' is not declared — declared features: {}",
                name,
                known.join(", ")
            ));
        }
    };
    // An ISLAND's roughing is its host's Clear (Phase 26, fork 6A): a boss
    // standing inside a pocket, or through a faced plane, is cut around by
    // THAT feature's op, and a moat aimed at the island refuses at validate —
    // so its skeleton is its own lines less the Clear. One home for "is it an
    // island" (`stock::host_of`), read here, by validate and by the emitters.
    let lines = own_lines(feature, name);
    Ok(match crate::stock::host_of(p, name) {
        Some(_) => lines.into_iter().filter(|l| !l.starts_with("run Clear(")).collect(),
        None => lines,
    })
}

/// The op that takes what a concession leaves (Phase 33 slice 2, fork 5): a
/// rest `Floor` on the same floor. `T?` like every skeleton — the tool is
/// the machinist's, and the refusal says how big it may be.
pub fn rest_line(name: &str) -> String {
    format!("run End(tool=T?, surface={name}, stepover=?, strategy=rest)")
}

fn own_lines(feature: &Feature, name: &str) -> Vec<String> {
    match feature {
        // A floor whose wall is the stock: the VOLUME above it first, around
        // whatever stands (Phase 61 slice 1), then the floor itself. The
        // Clear's key is `profile=` — one reference key per verb. Delete the
        // End where nothing about the floor serves the part.
        Feature::Plane { .. } | Feature::Face { .. } => vec![
            format!("run Clear(tool=T?, profile={name}, stepover=?)"),
            format!("run End(tool=T?, surface={name}, stepover=?)"),
        ],
        // The enclosed voids Clear can empty, then the FLOOR's own pass
        // (Phase 27 slice 4: `Floor` takes a pocket's floor — with no
        // allowance on the Clear it is a spring pass, and the line is there
        // to be deleted), then the wall pass. Each verb names a different
        // surface of the one feature: the volume, the floor, the wall.
        Feature::ProfilePocket { .. } => vec![
            format!("run Clear(tool=T?, profile={name}, stepover=?)"),
            format!("run End(tool=T?, surface={name}, stepover=?)"),
            format!("run Side(tool=T?, surface={name})"),
        ],
        // The hole, plus the SEAT its own declaration asks for. A seat is
        // a second run at the same points with a second tool — the shop order the
        // fixtures spell (`cbore.cam`, `csink.cam`): a spot cone before
        // the drill, a countersink or counterbore cone after it. Omitting
        // it would print a skeleton that, filled and run, never cuts a
        // declared feature — the silent kind of wrong this module exists
        // to avoid. Which tool each line wants stays the machinist's (the
        // seat laws in validate() judge the pairing).
        Feature::CylinderBore { csink, cbore, .. } => {
            let mut lines = vec![format!("run Form(points=[{name}], tool=T?, cycle=Simple)")];
            // A mouth cone is formed by the chamfer mill that carries it;
            // a counterbore's floor is made by an endmill's end.
            if csink.is_some() {
                lines.push(format!("run Form(points=[{name}], tool=T?, cycle=Simple)  # the seat"));
            }
            if cbore.is_some() {
                lines.push(format!("run End(points=[{name}], tool=T?, cycle=Simple)  # the seat"));
            }
            lines.push(format!("run Clear(tool=T?, profile={name}, stepover=?)"));
            lines.push(format!("run Side(tool=T?, surface={name}, stepover=?)"));
            lines
        }
        // No op of this program cuts a tilted bore (Phase 41 slice 5): it
        // is declared to be built against, gone around and read as a void,
        // and validate refuses every verb on it by name — so the skeleton
        // is honestly empty rather than a line that could never run.
        Feature::TiltedBore { .. } => Vec::new(),
        // The wall traces with no interior to empty: the wire pocket
        // full-depth (a Clear over a rim is refused: the wire declares a
        // wall, not an interior), the elliptic bore likewise.
        Feature::ContourPocket { .. } => {
            vec![format!("run Side(tool=T?, surface={name})")]
        }
        // STANDING material — the prismatic bosses and the standing
        // revolves: the moat roughed around it first, in a stack of Z
        // levels (`stepdown=?` — validate REQUIRES it here; on a pocket, a
        // bore, a Plane or a Face it is optional since Phase 61 slice 2 and
        // the skeleton stays silent, absent being one level; the increment intake found no skeleton spelled it, so the
        // window could not schedule this op), then the wall or the dome
        // finished (the shop order every boss and revolve fixture spells:
        // `cylinder_boss.cam`, `sphere.cam`, `contour_boss.cam`).
        Feature::ProfileBoss { .. } | Feature::ContourBoss { .. } => vec![
            format!("run Clear(tool=T?, profile={name}, stepover=?, stepdown=?)"),
            format!("run Side(tool=T?, surface={name})"),
        ],
        // A flank is the tool's side; a dome is a ball's nose (Phase 36).
        Feature::CylinderBoss { .. } => vec![
            format!("run Clear(tool=T?, profile={name}, stepover=?, stepdown=?)"),
            format!("run Side(tool=T?, surface={name}, stepover=?)"),
        ],
        Feature::Sphere { .. }
        | Feature::Cone { .. }
        | Feature::Torus { .. }
        | Feature::SplineRevolve { .. } => vec![
            format!("run Clear(tool=T?, profile={name}, stepover=?, stepdown=?)"),
            format!("run Copy(tool=T?, surface={name}, stepover=?)"),
        ],
        // The bowls, the funnels and the free surfaces: nothing stands
        // over their base for a Clear to rough (validate refuses one), so
        // one finishing pass — stepover (or scallop, the machinist's
        // trade, not the template's).
        Feature::Mesh { .. }
        | Feature::SplineRevolveFunnel { .. }
        | Feature::NurbsPatch { .. } => {
            vec![format!("run Copy(tool=T?, surface={name}, stepover=?)")]
        }
    }
}

/// The `--template` report: the lines, one per row, newline-terminated —
/// ready to paste into `main.part` once the `?`s are the machinist's
/// numbers.
pub fn report(p: &Part, name: &str) -> Result<String, String> {
    let lines = op_lines(p, name)?;
    // A feature no op cuts has an empty skeleton (the tilted bore, Phase 41
    // slice 5); the terminal says so rather than printing nothing — as a
    // comment, so the answer is still text the file would accept.
    if lines.is_empty() {
        if let Some(Feature::TiltedBore { .. }) = p.features.get(name) {
            return Ok(format!(
                "# '{name}' is a TiltedBore — no op of this program cuts a tilted bore (a 3-axis program never tilts the tool; it is cut on another setup). It is declared to be built against (nacs --arc), gone around and read as a void, so there is no run line to write\n"
            ));
        }
    }
    Ok(lines.into_iter().map(|l| l + "\n").collect())
}

// ---- the feature half: creation from nothing (Phase 19) ----------------
//
// The half the header names out ("a click on an already-declared wire has
// nothing new to offer") is built here now that someone has the
// dimensions: an ANSWER SET is where they come from. The claim stays as
// narrow as op_lines': WHICH keys each variant takes and HOW the
// statement is spelled — a single line, or an opener with point lines.
// Whether the VALUES make a legal feature is never judged here: the
// rendered text goes to the one parser (`review::check_statement` for the
// statement alone; a whole-project re-parse where a project exists), and
// the parser's sentence is the refusal. Locked by the fill-and-validate
// pins in `tests/wizard_tests.rs`, the same way `template_tests.rs` locks
// op_lines — filled answer sets must PARSE, and byte-equal the corpus's
// own hand-typed lines. This module refuses only what the parser cannot
// catch loudly: a name or value that would silently change the statement's
// shape ('#' opens a comment anywhere), an answer given twice (the parser
// keeps the last one silently), an empty point (a blank line inside a
// block is skipped, and the point would vanish).
//
// The menu below is a MIRROR of the vocabulary, on purpose: a shape it
// cannot offer is a roadmap item, not a wizard feature (the entry's
// forcing-function law — helices are a motion family, not a statement).

/// How a variant's geometry rides the statement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Points {
    /// A single line, every argument a `key=value`.
    None,
    /// A planar loop: `x y` per line between the opener and its `)`.
    Planar,
    /// A 3D wire: `x y z` per line — the points carry their own Z.
    Wire,
    /// A `Face`'s boundaries: a `loop` line opening each, `x y` per line
    /// under it — the first the outer (CCW), the rest inner (CW).
    Loops,
}

/// One key of a variant: its spelling, the kind-word a refusal names, and
/// whether the statement is legal without it. The slice order below is the
/// CANONICAL spelling order — shape keys first (optional shape keys in
/// their shape slot: `rotation`, `top_dia`, `profile_radius`,
/// `inflection`), placement after, the finishing and seat keys last — the
/// order the corpus itself writes.
pub struct Key {
    pub name: &'static str,
    pub kind: &'static str,
    pub required: bool,
}

const fn req(name: &'static str) -> Key {
    Key { name, kind: "a number", required: true }
}
const fn opt(name: &'static str) -> Key {
    Key { name, kind: "a number", required: false }
}
const fn opt_deg(name: &'static str) -> Key {
    Key { name, kind: "degrees", required: false }
}
const fn req_deg(name: &'static str) -> Key {
    Key { name, kind: "degrees", required: true }
}
const FILE: Key = Key { name: "file", kind: "a file path", required: true };

/// What the planar/wire `points` answer reads as, named in refusals.
const PLANAR_POINTS: &str = "x,y pairs, ';' between points";
const WIRE_POINTS: &str = "x,y,z triples, ';' between points";
const LOOP_POINTS: &str = "x,y pairs, ';' between points, '|' between loops — the outer first";

pub struct FeatureSpec {
    pub variant: &'static str,
    pub points: Points,
    pub keys: &'static [Key],
}

/// Every variant the language can say — `Feature::kind()`'s 26, in the
/// enum's own order.
pub const FEATURE_SPECS: &[FeatureSpec] = &[
    FeatureSpec { variant: "Plane", points: Points::None, keys: &[req("z")] },
    FeatureSpec { variant: "Face", points: Points::Loops, keys: &[req("z")] },
    FeatureSpec {
        variant: "ProfilePocket",
        points: Points::Planar,
        keys: &[
            opt("z"),
            req("extent"),
            opt("edge_round"),
            opt("chamfer_depth"),
            opt("chamfer_width"),
            opt_deg("chamfer_angle"),
        ],
    },
    FeatureSpec {
        variant: "ProfileBoss",
        points: Points::Planar,
        keys: &[
            opt("z"),
            req("extent"),
            opt("edge_round"),
            opt("chamfer_depth"),
            opt("chamfer_width"),
            opt_deg("chamfer_angle"),
        ],
    },
    FeatureSpec { variant: "ContourPocket", points: Points::Wire, keys: &[req("extent")] },
    FeatureSpec { variant: "ContourBoss", points: Points::Wire, keys: &[req("extent")] },
    FeatureSpec {
        variant: "CylinderBore",
        points: Points::None,
        keys: &[
            req("dia"),
            req("extent"),
            req("x"),
            req("y"),
            // The rim (Phase 45): omitted, the stock top.
            opt("z"),
            opt("edge_round"),
            opt("chamfer_depth"),
            opt("chamfer_width"),
            opt_deg("chamfer_angle"),
            opt("csink_dia"),
            opt("spot_dia"),
            opt_deg("csink_angle"),
            opt("cbore_dia"),
            opt("cbore_depth"),
        ],
    },
    // The intruder with an axis (Phase 41 slice 5): the vertical bore's
    // four keys plus its two angles, both required — a tilt of 0 is a
    // `CylinderBore` and refuses pointing at it, so there is no default to
    // offer.
    FeatureSpec {
        variant: "TiltedBore",
        points: Points::None,
        keys: &[req("dia"), req("extent"), req("x"), req("y"), req_deg("tilt"), req_deg("heading")],
    },
    FeatureSpec {
        variant: "CylinderBoss",
        points: Points::None,
        keys: &[
            req("dia"),
            req("extent"),
            req("x"),
            req("y"),
            req("z"),
            opt("edge_round"),
            opt("chamfer_depth"),
            opt("chamfer_width"),
            opt_deg("chamfer_angle"),
        ],
    },
    FeatureSpec { variant: "Sphere", points: Points::None, keys: &[req("dia"), req("x"), req("y"), req("z")] },
    FeatureSpec {
        variant: "Cone",
        points: Points::None,
        keys: &[req("dia"), req("height"), req("x"), req("y"), req("z")],
    },
    FeatureSpec {
        variant: "Torus",
        points: Points::None,
        keys: &[req("major_dia"), req("minor_dia"), req("x"), req("y"), req("z")],
    },
    FeatureSpec { variant: "Mesh", points: Points::None, keys: &[FILE, opt("x"), opt("y"), opt("z")] },
    FeatureSpec {
        variant: "SplineRevolve",
        points: Points::None,
        keys: &[FILE, opt("x"), opt("y"), opt("z")],
    },
    FeatureSpec {
        variant: "SplineRevolveFunnel",
        points: Points::None,
        keys: &[FILE, opt("x"), opt("y")],
    },
    FeatureSpec { variant: "NurbsPatch", points: Points::None, keys: &[FILE, opt("x"), opt("y"), opt("z")] },
];

/// The spec by its `.cam` spelling; an unknown spelling lists the whole
/// vocabulary, the way op_lines' unknown-name error lists the declared.
pub fn feature_spec(variant: &str) -> Result<&'static FeatureSpec, String> {
    FEATURE_SPECS.iter().find(|s| s.variant == variant).ok_or_else(|| {
        let mut known: Vec<&str> = FEATURE_SPECS.iter().map(|s| s.variant).collect();
        known.sort_unstable();
        format!("'{variant}' is not a feature variant — the vocabulary: {}", known.join(", "))
    })
}

/// The spec of a parsed feature. One arm per `Feature` variant,
/// exhaustive on purpose — a new variant stops compiling here instead of
/// silently having no questions (the op_lines rule, applied once more).
pub fn spec_for(f: &Feature) -> &'static FeatureSpec {
    let variant = match f {
        Feature::Plane { .. } => "Plane",
        Feature::Face { .. } => "Face",
        Feature::ProfilePocket { .. } => "ProfilePocket",
        Feature::ProfileBoss { .. } => "ProfileBoss",
        Feature::ContourPocket { .. } => "ContourPocket",
        Feature::ContourBoss { .. } => "ContourBoss",
        Feature::CylinderBore { .. } => "CylinderBore",
        Feature::TiltedBore { .. } => "TiltedBore",
        Feature::CylinderBoss { .. } => "CylinderBoss",
        Feature::Sphere { .. } => "Sphere",
        Feature::Cone { .. } => "Cone",
        Feature::Torus { .. } => "Torus",
        Feature::Mesh { .. } => "Mesh",
        Feature::SplineRevolve { .. } => "SplineRevolve",
        Feature::SplineRevolveFunnel { .. } => "SplineRevolveFunnel",
        Feature::NurbsPatch { .. } => "NurbsPatch",
    };
    feature_spec(variant).expect("the spec table covers every variant the match names")
}

// ---- the tool half (Phase 20, slice 2) --------------------------------------
//
// The same precedent applied to the `tool` statement: ONE spec per KIND,
// whose claim is which keys the kind takes and how the line is spelled,
// locked to the parser by fill-and-parse pins. The parser keeps one key
// list for every kind (`TOOL_KEYS`) and names the wrong family loudly.
// It judges the kind word too, since Phase 36 slice 0, against
// `parse::TOOL_KINDS` — this table's kinds in this table's order, pinned
// equal by test. The key order is the corpus's: a form tool's ground form
// before its body OD (`dia` is clearance on a rounder or a chamfer
// mill), the body ladder after, provenance last.

const fn count(name: &'static str) -> Key {
    Key { name, kind: "a whole number", required: false }
}
const fn text(name: &'static str, kind: &'static str) -> Key {
    Key { name, kind, required: false }
}

/// The body ladder every kind may carry (K2), in the corpus's order.
const BODY: [Key; 11] = [
    opt("flute_len"),
    opt("shank_dia"),
    opt("neck_dia"),
    opt("neck_len"),
    opt("stickout"),
    count("flute_count"),
    text("holder", "a label"),
    opt("holder_nose_dia"),
    opt("holder_nose_len"),
    opt("holder_body_dia"),
    text("coolant", "a coolant kind"),
];
const SOURCE: Key = Key { name: "source", kind: "catalog or measured", required: false };
const PROFILE: Key = Key { name: "profile", kind: "an SVG file path", required: true };

pub struct ToolSpec {
    pub kind: &'static str,
    /// The kind's own keys, before the body ladder.
    pub keys: &'static [Key],
}

/// Every kind the parser names — the corpus's six. The order here is the
/// menu's order.
pub const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec { kind: "Endmill", keys: &[req("dia")] },
    ToolSpec { kind: "Ballmill", keys: &[req("dia")] },
    ToolSpec { kind: "Drill", keys: &[req("dia")] },
    ToolSpec { kind: "FormTool", keys: &[req("dia"), PROFILE, opt("cut_bottom"), opt("cut_top"), SOURCE] },
    ToolSpec {
        kind: "CornerRounder",
        keys: &[
            req("radius"),
            req("effective_dia"),
            req("gauge_z"),
            req("dia"),
            req("pilot"),
            opt("radius_error"),
            opt("trail_wall"),
            opt("trail_face"),
            SOURCE,
        ],
    },
    ToolSpec {
        kind: "ChamferMill",
        keys: &[req("angle"), req("gauge_dia"), req("gauge_z"), req("dia"), req("tip_dia"), opt_deg("angle_error"), SOURCE],
    },
];

/// The spec by its kind word; an unknown word lists the whole menu.
pub fn tool_spec(kind: &str) -> Result<&'static ToolSpec, String> {
    TOOL_SPECS.iter().find(|s| s.kind == kind).ok_or_else(|| {
        let known: Vec<&str> = TOOL_SPECS.iter().map(|s| s.kind).collect();
        format!(
            "'{kind}' is not a tool kind — the parser would accept the word and no operation could use the tool; the kinds: {}",
            known.join(", ")
        )
    })
}

/// What a tool's key means, where its name does not say: shown beside
/// the field and printed in the reference.
pub fn tool_key_says(kind: &str, key: &str) -> Option<&'static str> {
    Some(match (kind, key) {
        ("ChamferMill", "angle") => "the included angle of the cone, as sold: 90 cuts a 45° chamfer",
        ("ChamferMill", "gauge_dia") => "a diameter measured on the cone",
        ("ChamferMill", "gauge_z") => "height above the tip of gauge_dia; not the tool's length",
        ("ChamferMill", "dia") => "the body's outside diameter, above the cone",
        ("ChamferMill", "tip_dia") => "the flat at the tip; no ground cone comes to a point",
        ("CornerRounder", "radius") => "the arc's radius, as sold",
        ("CornerRounder", "effective_dia") => "the diameter the arc meets the wall at",
        ("CornerRounder", "gauge_z") => "height above the tip of the arc's top; not the tool's length",
        ("CornerRounder", "dia") => "the body's outside diameter, above the arc",
        ("CornerRounder", "pilot") => "the diameter of the end below the arc",
        _ => return None,
    })
}

/// A kind's whole key list — its own keys, then the body ladder.
pub fn tool_keys(spec: &ToolSpec) -> Vec<&'static Key> {
    spec.keys.iter().chain(BODY.iter()).collect()
}

/// The number slot's reading: `T3` or `3`, one positive whole number.
pub fn parse_tool_number(text: &str) -> Result<usize, String> {
    let digits = text.trim().trim_start_matches('T');
    match digits.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(format!("'{}' is not a tool number — a tool is named T<number>, a positive whole number", text.trim())),
    }
}

/// The lowest tool number the project does not declare — tenet 2's smart
/// default for the creation form, derived from the declared tools and
/// never remembered.
pub fn lowest_undeclared_tool(p: &Part) -> usize {
    (1..).find(|n| !p.tools.contains_key(n)).expect("the numbers do not run out")
}

/// The answers' own silent wrongs, shared by both statement kinds: a key
/// answered twice (the parser's map would keep the last one silently) and
/// a value a '#' would truncate or a line break would end.
fn check_answers(answers: &[(String, String)]) -> Result<(), String> {
    for (i, (k, v)) in answers.iter().enumerate() {
        if answers[..i].iter().any(|(prev, _)| prev == k) {
            return Err(format!("'{k}' is answered twice — an ambiguous answer is no answer"));
        }
        if v.contains('#') || v.chars().any(char::is_control) {
            return Err(format!(
                "the value of '{k}' cannot carry '#' or a line break — '#' opens a comment anywhere \
                 on a line and a break ends the statement, so the text would silently change meaning"
            ));
        }
    }
    Ok(())
}

/// One `tool` statement from an answer set — the wizard's tool half, the
/// pure function both voices drive. Values pass through verbatim, a key
/// the spec does not know passes through to the parser's own sentence,
/// and the statement differs by zero bytes from the corpus's hand-typed
/// line.
pub fn tool_statement(kind: &str, number: usize, answers: &[(String, String)]) -> Result<String, String> {
    let spec = tool_spec(kind)?;
    if number == 0 {
        return Err("a tool number is a positive whole number — T0 names nothing".to_string());
    }
    check_answers(answers)?;
    let keys = tool_keys(spec);
    let missing: Vec<String> = keys
        .iter()
        .filter(|k| k.required && !answers.iter().any(|(a, _)| a == k.name))
        .map(|k| format!("{} ({})", k.name, k.kind))
        .collect();
    if !missing.is_empty() {
        let required: Vec<&str> = keys.iter().filter(|k| k.required).map(|k| k.name).collect();
        let optional: Vec<&str> = keys.iter().filter(|k| !k.required).map(|k| k.name).collect();
        return Err(format!(
            "a {kind} still needs: {} — it takes {}; optionally {}",
            missing.join(", "),
            required.join(", "),
            optional.join(", ")
        ));
    }
    let mut args: Vec<String> = Vec::new();
    for key in &keys {
        if let Some((_, v)) = answers.iter().find(|(a, _)| a == key.name) {
            args.push(format!("{}={v}", key.name));
        }
    }
    for (k, v) in answers {
        if !keys.iter().any(|key| key.name == k) {
            args.push(format!("{k}={v}"));
        }
    }
    Ok(format!("tool T{number} {kind}({})", args.join(", ")))
}

// ---- the run half: the skeleton is the form (Phase 20, slice 3) -------------
//
// No third spec table. `op_lines` already says which keys a `run` line
// takes for a feature kind and how the reference is spelled — the
// skeleton with its `T?` and `?` blanks — and `view::set_key` rewrites
// one key on one line. So a created run line is the skeleton with its
// blanks filled: the blanks are the questions, `set_key` is the fill,
// and a second list of run keys would be exactly the drift the feature
// half's fork 1 forbade.

/// The op words the grammar has — the run head's vocabulary.
pub const OP_WORDS: [&str; 5] = ["End", "Clear", "Side", "Copy", "Form"];

/// A skeleton's blanks: each placeholder key with the kind-word a refusal
/// names. `tool=T?` is the tool; every `?` is a number.
pub fn skeleton_blanks(skeleton: &str) -> Vec<(String, &'static str)> {
    crate::view::fields_of(skeleton)
        .into_iter()
        .filter_map(|(k, v)| match v.as_str() {
            "T?" => Some((k, "a declared tool, T<n>")),
            "?" => Some((k, "a number")),
            _ => None,
        })
        .collect()
}

/// One `run` statement from a skeleton and an answer set — the wizard's
/// run half. Every blank must be answered (the refusal names the next);
/// a key the skeleton spells for the feature (`surface=`, `profile=`,
/// `points=`) may not be answered — the line is created ON the feature
/// that offered it, and a redirect is the silent wrong the parser would
/// accept; any other stranger is appended by `set_key` on its way to the
/// parser's own sentence.
pub fn fill_skeleton(skeleton: &str, answers: &[(String, String)]) -> Result<String, String> {
    check_answers(answers)?;
    let fields = crate::view::fields_of(skeleton);
    let blanks = skeleton_blanks(skeleton);
    for (k, _) in answers {
        if let Some((_, v)) = fields.iter().find(|(f, _)| f == k) {
            if v != "?" && v != "T?" {
                return Err(format!(
                    "'{k}' is the skeleton's ({k}={v}) — a run line is created on the feature that offered it, so that key is not an answer"
                ));
            }
        }
    }
    let missing: Vec<String> = blanks
        .iter()
        .filter(|(k, _)| !answers.iter().any(|(a, _)| a == k))
        .map(|(k, kind)| format!("{k} ({kind})"))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "the line still needs: {} — the skeleton: {}",
            missing.join(", "),
            skeleton.trim()
        ));
    }
    let mut line = skeleton.to_string();
    for (k, v) in answers {
        line = crate::view::set_key(&line, k, Some(v))?;
    }
    Ok(line)
}

/// The skeletons a feature offers for one op word — the CLI voice's
/// reading (`--new 'Floor Top …'`): the FIRST skeleton with that word
/// (a bore's csink seat line is its second Form and spells the same text).
pub fn run_statement(p: &Part, op_word: &str, feature: &str, answers: &[(String, String)]) -> Result<String, String> {
    if !OP_WORDS.contains(&op_word) {
        return Err(format!("'{op_word}' is not an operation — the op words: {}", OP_WORDS.join(", ")));
    }
    let lines = op_lines(p, feature)?;
    let head = format!("run {op_word}(");
    let skeleton = lines.iter().find(|l| l.starts_with(&head)).ok_or_else(|| {
        let offered: Vec<&str> = lines.iter().filter_map(|l| l.split_whitespace().nth(1)).map(|w| w.split('(').next().unwrap_or(w)).collect();
        format!("a {op_word} is not an operation the template offers for '{feature}' — its skeleton offers: {}", offered.join(", "))
    })?;
    fill_skeleton(skeleton, answers)
}

/// Which statement an answer set's head word asks for. The feature
/// vocabulary is disjoint from the other two (pinned); the tool kinds and
/// the op words share exactly one word — `Drill` is a tool and an
/// operation — so where the head word is in both, the NAME slot decides:
/// `T<n>` is a tool, anything else a feature the run line is created on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    Feature,
    Tool,
    Run,
}

/// The head word's vocabulary, or a refusal naming all three.
pub fn head_of(word: &str, name: &str) -> Result<Head, String> {
    if FEATURE_SPECS.iter().any(|s| s.variant == word) {
        return Ok(Head::Feature);
    }
    let kind = TOOL_SPECS.iter().any(|s| s.kind == word);
    let op = OP_WORDS.contains(&word);
    match (kind, op) {
        (true, false) => return Ok(Head::Tool),
        (false, true) => return Ok(Head::Run),
        (true, true) => return Ok(if parse_tool_number(name).is_ok() { Head::Tool } else { Head::Run }),
        (false, false) => {}
    }
    let mut variants: Vec<&str> = FEATURE_SPECS.iter().map(|s| s.variant).collect();
    variants.sort_unstable();
    let kinds: Vec<&str> = TOOL_SPECS.iter().map(|s| s.kind).collect();
    Err(format!(
        "'{word}' is not a feature variant, a tool kind or an operation — the feature vocabulary: {}; the tool kinds: {}; the operations: {}",
        variants.join(", "),
        kinds.join(", "),
        OP_WORDS.join(", ")
    ))
}

/// The CLI voice's reading of one answer-set argument —
/// `<Variant> <name> key=value …`, whitespace-split, values verbatim.
/// Only the SHAPE of the argument is judged here; the keys and values
/// belong to `feature_statement` and the parser. (The window pane never
/// spells this string — its form hands `feature_statement` the answers
/// directly, so this reader stays the terminal's alone.)
pub fn parse_answer_set(spec: &str) -> Result<(String, String, Vec<(String, String)>), String> {
    let mut toks = spec.split_whitespace();
    let variant = toks
        .next()
        .ok_or("an empty answer set — the shape: <Variant> <name> key=value ..., <Kind> T<n> key=value ..., or <Op> <feature> key=value ...")?;
    let name = toks.next().ok_or_else(|| {
        format!("'{spec}' names nothing — the shape: <Variant> <name> key=value ..., <Kind> T<n> key=value ..., or <Op> <feature> key=value ...")
    })?;
    let mut answers = Vec::new();
    for tok in toks {
        let Some(eq) = tok.find('=') else {
            return Err(format!("'{tok}' is not an answer — every answer is key=value"));
        };
        answers.push((tok[..eq].to_string(), tok[eq + 1..].to_string()));
    }
    Ok((variant.to_string(), name.to_string(), answers))
}

/// One `feature` statement from an answer set — the wizard proper, the
/// pure function both voices drive. Values pass through VERBATIM (the
/// statement differs by zero bytes from the hand-typed one), and an
/// answer key the spec does not know passes through too, so the parser's
/// own `has no field named` sentence is the refusal — never a second
/// spelling of the whitelist here.
pub fn feature_statement(
    variant: &str,
    name: &str,
    answers: &[(String, String)],
) -> Result<String, String> {
    let spec = feature_spec(variant)?;

    if name.is_empty() {
        return Err("a feature needs a name — feature <name> <Variant>(…)".to_string());
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_whitespace() || c.is_control() || matches!(c, '#' | '(' | ')' | '[' | ']' | ',' | '='))
    {
        return Err(format!(
            "'{name}' cannot name a feature — '{}' cannot be spelled into a statement or referenced \
             from a run line (one word, without '#', '(', ')', '[', ']', ',' or '=')",
            c.escape_default()
        ));
    }

    check_answers(answers)?;

    // The block variants read their geometry from the `points` answer;
    // on a single-line variant the same key is nothing special and falls
    // through to the parser's whitelist like any other stranger.
    let points_kind = match spec.points {
        Points::None => None,
        Points::Planar => Some(PLANAR_POINTS),
        Points::Wire => Some(WIRE_POINTS),
        Points::Loops => Some(LOOP_POINTS),
    };
    let points_answer = points_kind.and_then(|_| answers.iter().find(|(k, _)| k == "points"));

    let mut missing: Vec<String> = spec
        .keys
        .iter()
        .filter(|k| k.required && !answers.iter().any(|(a, _)| a == k.name))
        .map(|k| format!("{} ({})", k.name, k.kind))
        .collect();
    if let (Some(kind), None) = (points_kind, points_answer) {
        missing.push(format!("points ({kind})"));
    }
    if !missing.is_empty() {
        let required: Vec<&str> =
            spec.keys.iter().filter(|k| k.required).map(|k| k.name).collect();
        let optional: Vec<&str> =
            spec.keys.iter().filter(|k| !k.required).map(|k| k.name).collect();
        let mut takes = required.join(", ");
        if points_kind.is_some() {
            takes.push_str(", points");
        }
        if !optional.is_empty() {
            takes.push_str(&format!("; optionally {}", optional.join(", ")));
        }
        return Err(format!("a {variant} still needs: {} — it takes {takes}", missing.join(", ")));
    }

    // Canonical order: the spec's slots first, every stranger after them
    // as given, on its way to the parser's refusal.
    let mut args: Vec<String> = Vec::new();
    for key in spec.keys {
        if let Some((_, v)) = answers.iter().find(|(a, _)| a == key.name) {
            args.push(format!("{}={v}", key.name));
        }
    }
    for (k, v) in answers {
        if spec.keys.iter().any(|key| key.name == k) {
            continue;
        }
        if points_answer.is_some() && k == "points" {
            continue;
        }
        args.push(format!("{k}={v}"));
    }

    match points_answer {
        None => Ok(format!("feature {name} {variant}({})", args.join(", "))),
        Some((_, raw)) if spec.points == Points::Loops => {
            let mut lines: Vec<String> = Vec::new();
            for (l, lp) in raw.split('|').enumerate() {
                lines.push("    loop".to_string());
                for (i, seg) in lp.split(';').enumerate() {
                    let coords: Vec<&str> = seg.split(',').map(str::trim).collect();
                    if coords.iter().any(|c| c.is_empty()) {
                        return Err(format!("point {} of loop {} of 'points' is empty ({LOOP_POINTS})", i + 1, l + 1));
                    }
                    lines.push(format!("        {}", coords.join(" ")));
                }
            }
            Ok(format!("feature {name} {variant}({}\n{}\n)", args.join(", "), lines.join("\n")))
        }
        Some((_, raw)) => {
            let mut lines: Vec<String> = Vec::new();
            for (i, seg) in raw.split(';').enumerate() {
                let coords: Vec<&str> = seg.split(',').map(str::trim).collect();
                if coords.iter().any(|c| c.is_empty()) {
                    return Err(format!(
                        "point {} of 'points' is empty — a blank point line inside the block is \
                         skipped by the parser and the point would silently vanish ({})",
                        i + 1,
                        points_kind.expect("a points answer implies a block variant")
                    ));
                }
                lines.push(format!("    {}", coords.join(" ")));
            }
            Ok(format!("feature {name} {variant}({}\n{}\n)", args.join(", "), lines.join("\n")))
        }
    }
}

// ---- the project's first file (Phase 50 slice 3, the way in) ----------------

/// The stock kinds a new project can open on, in the menu's order.
pub const PROJECT_STOCK_KINDS: [&str; 2] = ["Box", "Cylinder"];

/// The keys a new project asks, per stock kind: the stock's own first (its
/// required ones leading), then the header's six, each with the default it
/// takes when left blank — tenet 2's smart default is the house's own
/// numbers (feed 800, plunge 300, spindle 3000 in mm; the planes five and
/// twenty-five above the stock top), SHOWN in the form and in the file,
/// never a last-used value. `(key, required, default-as-shown)`.
pub fn project_keys(kind: &str) -> Result<Vec<(&'static str, bool, String)>, String> {
    let stock: &[(&str, bool)] = match kind {
        "Box" => &[("w", true), ("l", true), ("h", true), ("x", false), ("y", false), ("z", false)],
        "Cylinder" => &[("dia", true), ("h", true), ("x", false), ("y", false), ("z", false)],
        other => return Err(format!("`{other}` is not a stock kind — Box or Cylinder")),
    };
    let mut keys: Vec<(&'static str, bool, String)> = stock.iter().map(|(k, r)| (*k, *r, String::new())).collect();
    for (k, d) in [("units", "mm"), ("feed", "800"), ("plunge", "300"), ("spindle", "3000"), ("rapid_z", "top + 5"), ("clearance_z", "top + 25")] {
        keys.push((k, false, d.to_string()));
    }
    Ok(keys)
}

/// `--init`'s one argument: `<Box|Cylinder> key=value …`, whitespace-split,
/// values verbatim — the shape alone is judged here, as `parse_answer_set`
/// does for `--new`.
pub fn parse_init_spec(spec: &str) -> Result<(String, Vec<(String, String)>), String> {
    let mut toks = spec.split_whitespace();
    let kind = toks
        .next()
        .ok_or_else(|| "the spec is empty — nacs --init 'Box w=100 l=60 h=12' <dir>".to_string())?
        .to_string();
    let mut answers = Vec::new();
    for t in toks {
        let (k, v) = t.split_once('=').ok_or_else(|| format!("`{t}` is not key=value"))?;
        answers.push((k.to_string(), v.to_string()));
    }
    Ok((kind, answers))
}

/// A plane's value: a number as given, or `top + n` (the form's own
/// default spelling, legal on the command line too) read against the stock
/// top. Three decimals, `gcode::f`'s, so the file reads as a number.
fn plane_value(key: &str, given: Option<String>, top: f64, default_above: f64) -> Result<String, String> {
    let Some(v) = given else { return Ok(crate::gcode::f(top + default_above)) };
    let compact: String = v.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some(rest) = compact.strip_prefix("top") {
        let n: f64 = match rest {
            "" => 0.0,
            r => r.parse().map_err(|_| format!("{key} `{v}` is not a number nor `top + n`"))?,
        };
        return Ok(crate::gcode::f(top + n));
    }
    v.parse::<f64>().map_err(|_| format!("{key} `{v}` is not a number nor `top + n`"))?;
    Ok(v)
}

/// The text of a new `main.part` (Phase 50 slice 3): the header, every
/// word with a visible value, then the stock. One home — `nacs --init` and
/// the window's New project both render here, and the one parser judges
/// the text after, so the header law (slice 1) can never refuse what this
/// writes. An unknown key, a missing required stock key, a unit that is
/// neither, or a plane that is not a number refuse by name.
pub fn project_text(kind: &str, answers: &[(String, String)]) -> Result<String, String> {
    let keys = project_keys(kind)?;
    for (k, _) in answers {
        if !keys.iter().any(|(n, _, _)| n == k) {
            return Err(format!(
                "`{k}` is not a key a new {kind} project takes — {}",
                keys.iter().map(|(n, _, _)| *n).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    let get = |k: &str| answers.iter().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
    for (k, required, _) in &keys {
        if *required && get(k).is_none() {
            return Err(format!("a {kind} stock needs {k}"));
        }
    }
    let units = get("units").unwrap_or_else(|| "mm".to_string());
    if units != "mm" && units != "inch" {
        return Err(format!("units is mm or inch, not `{units}`"));
    }
    let top: f64 = match get("z") {
        Some(z) => z.parse().map_err(|_| format!("z `{z}` is not a number"))?,
        None => 0.0,
    };
    let header = header_text(&units, top, get("feed"), get("plunge"), get("spindle"), get("rapid_z"), get("clearance_z"))?;
    let stock_keys: Vec<String> = keys
        .iter()
        .filter(|(k, _, _)| !crate::parse::HEADER_KEYS.contains(k))
        .filter_map(|(k, _, _)| get(k).map(|v| format!("{k}={v}")))
        .collect();
    Ok(format!("{header}\nstock {kind}({})\n", stock_keys.join(", ")))
}

/// The header alone — the comment and the six words, every default
/// visible, the planes read against `top` — one home for `project_text`
/// and `project_text_from_proposal` (Phase 56 slice 4).
fn header_text(
    units: &str,
    top: f64,
    feed: Option<String>,
    plunge: Option<String>,
    spindle: Option<String>,
    rapid_z: Option<String>,
    clearance_z: Option<String>,
) -> Result<String, String> {
    let mm = units == "mm";
    let feed = feed.unwrap_or_else(|| if mm { "800" } else { "32" }.to_string());
    let plunge = plunge.unwrap_or_else(|| if mm { "300" } else { "12" }.to_string());
    let spindle = spindle.unwrap_or_else(|| "3000".to_string());
    let rapid_z = plane_value("rapid_z", rapid_z, top, if mm { 5.0 } else { 0.2 })?;
    let clearance_z = plane_value("clearance_z", clearance_z, top, if mm { 25.0 } else { 1.0 })?;
    Ok(format!(
        "# written by nacs --init / the window's New project: every value here was shown before it was written — change any of them.\n\
         units {units}\nfeed {feed}\nplunge {plunge}\nspindle {spindle}\nrapid_z {rapid_z}\nclearance_z {clearance_z}\n"
    ))
}

/// A new `main.part` from a PROPOSED stock (Phase 56 slice 4, fork 4A):
/// the header's mm defaults read against the proposal's top — its `z=`,
/// the model's frame, so the planes stand above the blank the model
/// gave — then the proposal's lines as the sidecar wrote them, comments
/// and all. The one parser judges the text after, as it judges
/// `project_text`'s. The way in and the stock from the model in one act.
pub fn project_text_from_proposal(proposal: &str) -> Result<String, String> {
    let first = proposal
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .find(|l| !l.is_empty())
        .ok_or_else(|| "the proposal carries no stock statement".to_string())?;
    if !first.starts_with("stock ") {
        return Err(format!("the proposal's first statement is not a stock: {first}"));
    }
    let top = first
        .split("z=")
        .nth(1)
        .and_then(|r| r.split(|c: char| c == ',' || c == ')' || c.is_whitespace()).next())
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let header = header_text("mm", top, None, None, None, None, None)?;
    Ok(format!("{header}\n{}\n", proposal.trim_end()))
}

// ---- the forms speak machining (Phase 50 slice 5) ----------------------------

/// The feature kinds grouped by what a machinist is making (DESIGN.md
/// §5.4, 2026-10-15): the creation form's menu shows these headings over
/// the variant words. Every `FEATURE_SPECS` variant appears exactly once,
/// pinned.
pub const FEATURE_GROUPS: &[(&str, &[&str])] = &[
    ("holes", &["CylinderBore", "TiltedBore"]),
    ("pockets and faces", &["ProfilePocket", "Face", "Plane", "ContourPocket"]),
    ("bosses and outside profiles", &["ProfileBoss", "CylinderBoss", "ContourBoss"]),
    (
        "3D surfaces",
        &[
            "Sphere", "Cone", "Torus", 
            "SplineRevolve", "SplineRevolveFunnel", "NurbsPatch", "Mesh",
        ],
    ),
];

/// A name for a new feature nobody has to invent: the variant's plain word
/// (`Pocket`, `Hole`, `Boss`, `Post`, `Top`, `Face`, else the variant
/// itself) with the lowest number the project does not already use —
/// tenet 2's smart default, derived from the declared features, shown in
/// the name field and editable.
pub fn suggested_name(p: &Part, variant: &str) -> String {
    let base = match variant {
        "ProfilePocket" | "ContourPocket" => "Pocket",
        "ProfileBoss" | "ContourBoss" => "Boss",
        "CylinderBore" | "TiltedBore" => "Hole",
        "CylinderBoss" => "Post",
        // The first Plane is the skim at the top; a second is the floor of
        // something (Phase 61 slice 3 — a Plane is the floor whose wall is
        // the stock, and the name says what it will be used as).
        "Plane" => {
            if p.features.values().any(|f| matches!(f, Feature::Plane { .. })) {
                "Floor"
            } else {
                "Top"
            }
        }
        other => other,
    };
    suggested_name_from(p, base)
}

/// The lowest-numbered `<base>N` the project does not declare — the numbering
/// half of `suggested_name`, for a caller that knows what the feature IS
/// better than its kind alone says (Phase 61 nudge 1: the *Clear around it…*
/// door's Plane is a FLOOR by construction, whatever the project's other
/// Planes are called; `suggested_name`'s first-Plane-is-the-skim guess is for
/// the form, where no z is in hand yet).
pub fn suggested_name_from(p: &Part, base: &str) -> String {
    (1..)
        .map(|n| format!("{base}{n}"))
        .find(|name| !p.features.contains_key(name))
        .expect("the integers do not run out")
}

/// A stepover nobody has to guess: half the tool's diameter, the house's
/// own rule of thumb (`profile_pocket_clear.cam` runs a ⌀10 at 4, the
/// corpus between a third and a half). Shown as a draft the user commits,
/// never silently written (tenet 2; the Phase 50 DRAFT's fork 5A). `None`
/// for a tool the project does not declare.
pub fn suggested_stepover(p: &Part, tool: &str) -> Option<String> {
    let n = parse_tool_number(tool).ok()?;
    let t = p.tools.get(&n)?;
    Some(crate::gcode::f(t.dia / 2.0).trim_end_matches('0').trim_end_matches('.').to_string())
}

// ---- block features from numbers (Phase 50 slice 6) --------------------------

/// Which way a planar kind's loop winds (the row-2.4 law the parser
/// refuses against): a pocket CCW, a boss CW; `None` for a kind whose
/// points are not a planar loop.
pub fn winding_ccw(variant: &str) -> Option<bool> {
    match variant {
        "ProfilePocket" => Some(true),
        "ProfileBoss" => Some(false),
        _ => None,
    }
}

fn number(what: &str, text: &str, default: Option<f64>) -> Result<f64, String> {
    let t = text.trim();
    if t.is_empty() {
        return default.ok_or_else(|| format!("{what} is needed"));
    }
    t.parse::<f64>().map_err(|_| format!("{what} `{t}` is not a number"))
}

fn points_text(pts: &[(f64, f64)], ccw: bool) -> String {
    let fmt = |v: f64| crate::gcode::f(v).trim_end_matches('0').trim_end_matches('.').to_string();
    let mut pts: Vec<(f64, f64)> = pts.to_vec();
    if !ccw {
        pts.reverse();
    }
    pts.iter().map(|(x, y)| format!("{},{}", fmt(*x), fmt(*y))).collect::<Vec<_>>().join(";")
}

/// A rectangle as points text (`x,y;…`): `w` by `l` with its corner at
/// `x, y` (0, 0 when blank), wound CCW or CW as the kind wants — the
/// creation form's quick-start, so the persona's common loop is four
/// numbers and not a typed list. The text lands through the same
/// `points` answer a typed list does.
pub fn rectangle_points(w: &str, l: &str, x: &str, y: &str, ccw: bool) -> Result<String, String> {
    let (w, l) = (number("w", w, None)?, number("l", l, None)?);
    if w <= 0.0 || l <= 0.0 {
        return Err("a rectangle's w and l are positive".to_string());
    }
    let (x, y) = (number("x", x, Some(0.0))?, number("y", y, Some(0.0))?);
    Ok(points_text(&[(x, y), (x + w, y), (x + w, y + l), (x, y + l)], ccw))
}

/// A circle as points text: `dia` centred at `x, y`, `n` chords, wound as
/// the kind wants. A chorded circle is what the sidecar writes too.
pub fn circle_points(dia: &str, x: &str, y: &str, n: usize, ccw: bool) -> Result<String, String> {
    let dia = number("dia", dia, None)?;
    if dia <= 0.0 {
        return Err("a circle's dia is positive".to_string());
    }
    let n = n.max(8);
    let (x, y) = (number("x", x, Some(0.0))?, number("y", y, Some(0.0))?);
    let r = dia / 2.0;
    let pts: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            (x + r * a.pcos(), y + r * a.psin())
        })
        .collect();
    Ok(points_text(&pts, ccw))
}
