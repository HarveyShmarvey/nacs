// ---- outline: the project's navigable tree, CLI-side -----------------------
//
// Phase 6, slice 0 (DECISIONS.md → "Phase 6 opens…"). `ROADMAP.md`'s Phase-6
// item 4 is a VS-Code-style **outline + map**, and the roadmap itself splits
// it: "the outline *tree* alone is text-representable and could exist CLI-side
// earlier; the color-coded map is the part that needs the frontend." This file
// is that half — `stock -> features -> operations`, with each operation's role
// DERIVED (`parse::derive_op_role`), which is exactly the tag the map view
// will color.
//
// The point is the SEAM, not the text. `ProjectOutline` is a view model over
// `Part`; `render()` is one presentation of it. The frontend (`frontend.rs`,
// slice 1) consumes the same struct rather than walking `Part` a second
// time — the house rule that a renderer is a thin consumer, never a second
// engine. This
// file therefore DERIVES nothing geometric of its own: every number it prints
// comes from a `parse::derive_*()` or from `Part` directly.
//
// Deliberately NOT here (scope, named rather than discovered): per-feature
// dimensions and placement. Printing those means one match arm per `Feature`
// variant restating the parser's fields, and the wireframe render needs that
// same per-variant geometry — so it belongs to the slice that draws the part,
// written once, not sketched twice. The tree carries each feature's KIND
// (`Feature::kind`), which is what a navigator needs to navigate.

use crate::gcode::f;
use crate::parse::{derive_op_role, derive_t, OpRole, OpType, Operation, Part, Tool};

/// One operation, as the outline sees it.
#[derive(Clone, Copy)]
pub struct OpNode {
    /// Index into `Part::operations` — and therefore the SAME index
    /// `gcode::OpEmission::op_idx` carries. That is the join key: a renderer
    /// holding this outline and the emission emitted moves can put a toolpath under the
    /// right tree node without matching on names.
    pub idx: usize,
    pub op_type: OpType,
    pub tool_num: usize,
    pub role: OpRole,
    /// The allowance whose PRESENCE made the role `Roughing`, carried so the
    /// outline can show the number without re-reading the tag and risking a
    /// disagreement with `derive_op_role`.
    pub allowance: Option<f64>,
    /// The skin the pass actually RIDES — `parse::derive_emitted_skin`'s
    /// answer, carried beside the allowance because the two can differ by 80%
    /// of the budget and the allowance alone was the only number this tree
    /// showed (DECISIONS.md → "the roughing allowance has two arms…", fork 2A).
    /// `None` is the model declining (a carrier with no budget arm), which the
    /// line states rather than hides.
    pub rides: Option<f64>,
}

/// One feature and the operations that consume it.
pub struct FeatureNode {
    pub name: String,
    pub kind: &'static str,
    pub ops: Vec<OpNode>,
}

/// How a feature no operation names still shapes what one cuts (Phase 37
/// slice 5). Each is read from a home that already names it — `region::of`'s
/// islands, voids and bridges, and the neighbours that closed its edges —
/// and none is derived here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shaping {
    /// Standing in the op's floor: the path goes around it.
    Island,
    /// A void in the op's floor the tool overhangs and does not cross.
    OpenVoid,
    /// A void in the op's floor the path runs straight over.
    Bridged,
    /// Standing beyond an edge of the op's floor, and so making it a wall.
    ClosesEdge,
    /// A given void whose air the op's floor grew into (Phase 42 slice 3).
    GrownInto,
}

impl Shaping {
    fn words(self) -> &'static str {
        match self {
            Shaping::Island => "island of",
            Shaping::OpenVoid => "open void of",
            Shaping::Bridged => "bridged by",
            Shaping::ClosesEdge => "closes an edge of",
            Shaping::GrownInto => "given air grown into by",
        }
    }
}

/// A feature no operation names, and the operations it shapes.
pub struct ShapingNode {
    pub name: String,
    pub kind: &'static str,
    /// `(Part::operations` index, how), in `run` order.
    pub uses: Vec<(usize, Shaping)>,
}

/// One indirectly-used feature's row, shared verbatim by the text tree and
/// the window: its kind, then each way it is used with the operations that
/// use it so — `island of [0] [2]; closes an edge of [1]`.
pub fn shaping_row(node: &ShapingNode) -> String {
    let mut parts: Vec<String> = Vec::new();
    for how in [Shaping::Island, Shaping::OpenVoid, Shaping::Bridged, Shaping::ClosesEdge, Shaping::GrownInto] {
        let ops: Vec<String> = node.uses.iter().filter(|(_, h)| *h == how).map(|(i, _)| format!("[{i}]")).collect();
        if !ops.is_empty() {
            parts.push(format!("{} {}", how.words(), ops.join(" ")));
        }
    }
    format!("{}  {}  {}", node.name, node.kind, parts.join("; "))
}

/// One declared tool and the operations that run it (Phase 20 slice 1,
/// DESIGN.md §5.2's Tools heading): the declaration's own row, so a
/// `tool` line no operation runs has a place in the tree instead of
/// being invisible — the window's tool card hangs off an operation, and
/// the op lines above name a tool only where one is used.
pub struct ToolNode {
    pub num: usize,
    /// `tool_label`'s text — the same words the op lines use.
    pub label: String,
    /// Indices into `Part::operations` that run this tool, in `run` order.
    pub ops: Vec<usize>,
}

/// The whole project, in `run` order.
pub struct ProjectOutline {
    pub stock: crate::parse::Stock,
    /// Features that at least one operation consumes, in FIRST-USE order down
    /// the `run` sequence.
    pub features: Vec<FeatureNode>,
    /// Features no operation names that still shape what one cuts, name-sorted
    /// (Phase 37 slice 5; the user's observation, 2026-09-22). An island the
    /// path goes around, a wall that closes an edge: none of these is a
    /// mistake, and until this list existed they were printed as one, two
    /// lines under the op that named them.
    pub shaping: Vec<ShapingNode>,
    /// Features nothing in the engine reads, name-sorted: no operation names
    /// them and none is shaped by them. Kept rather than dropped: a feature
    /// nothing cuts is usually a mistake worth seeing, and a tree that
    /// silently omits it hides the mistake. The only list that can be one.
    pub unused: Vec<FeatureNode>,
    /// The voids declared GIVEN — air before this program starts, cut on
    /// another setup (Phase 42 slice 1) — in declaration order, each with
    /// its kind. A given void is read by the stock model whether or not an
    /// op names it, so it is never `unused`; it may still appear above as a
    /// feature an op dresses or under `shaping` as one a floor bridges.
    pub given: Vec<FeatureNode>,
    /// Every declared tool in NUMBER order — never `HashMap` order — each
    /// with the operations that run it (empty for a tool nothing runs).
    pub tools: Vec<ToolNode>,
    /// What the stock model says each op removes — `removal_line`, taken ONCE
    /// with the rest of the outline, indexed like `Part::operations`. A
    /// renderer reads it here and must not ask per frame: the model's answer
    /// can cost a coverage grid (Phase 32: 124 ms a frame on the walled rail
    /// plate, a slideshow at 7 fps).
    pub removals: Vec<String>,
    /// The air a tool body can stand in, per op — `stock::given_air`'s
    /// lines, one per given void, indexed like `Part::operations` (Phase 42
    /// slice 2). Taken ONCE here for the same reason `removals` is: a lens
    /// costs offsets and booleans, and a renderer must not ask per frame.
    /// Empty for an op that works no floor, and for a program that gives
    /// nothing — which is every program before this phase.
    pub given_air: Vec<Vec<String>>,
    /// The program before this one, as `given from <project> up=<axis>`
    /// says it (Phase 48 slice 1): one line for the statement — the project,
    /// the turn, how many of its ops the stock model read as air at prefix 0
    /// — then one line per removal it could not carry, each naming the
    /// earlier op (`stock::given_from_air`'s own sentences). Empty for a
    /// program that says no `given from`, which is every program before this
    /// phase. Taken ONCE here like `removals`: the earlier program's
    /// removals are derived on demand and cost what its ops cost.
    pub given_from: Vec<String>,
}

/// Which features an operation consumes.
///
/// One home, because the answer is not uniform and the non-uniform case is the
/// one that bites: a `Drill` op's `feature_name` is EMPTY — its holes live in
/// `drill_points`, and it may name several at once (`run Form(points=[Hole1,
/// Hole2], ...)`). Reading `feature_name` alone would file every drill cycle
/// under a phantom "" feature and leave the holes looking never-machined,
/// which is precisely the kind of confident-but-wrong picture this tree exists
/// to avoid.
pub fn op_features(op: &Operation) -> Vec<&str> {
    match &op.drill_points {
        Some(points) => points.iter().map(|s| s.as_str()).collect(),
        None => vec![op.feature_name.as_str()],
    }
}

/// Build the tree. Ordering is `run` order then sorted names — never
/// `HashMap` iteration order, which is nondeterministic and would make this
/// output differ run to run.
pub fn outline(p: &Part) -> ProjectOutline {
    let mut features: Vec<FeatureNode> = Vec::new();

    for (idx, op) in p.operations.iter().enumerate() {
        let node = OpNode {
            idx,
            op_type: op.op_type,
            tool_num: op.tool_num,
            role: derive_op_role(op),
            allowance: op.stock_to_leave,
            rides: crate::parse::derive_emitted_skin(p, op),
        };
        for name in op_features(op) {
            // validate() guarantees the feature exists for every op that names
            // one (`referenced feature '...' not defined`, and the Drill arm's
            // own points check), so a lookup miss is not a case to invent
            // behaviour for — and not one the window can reach either: it
            // opens only a parsed AND validated project, and a reload that
            // fails changes nothing on screen (DESIGN.md §1). The "rejected"
            // project the window knows is an EMISSION refusal after
            // validate, whose every operation is here. (Phase 23's fork 6
            // proposed a "Not resolvable" heading for the dropped ops; the
            // build measured that there are none to list.)
            let kind = match p.features.get(name) {
                Some(f) => f.kind(),
                None => continue,
            };
            match features.iter_mut().find(|n| n.name == name) {
                Some(existing) => existing.ops.push(node),
                None => features.push(FeatureNode {
                    name: name.to_string(),
                    kind,
                    ops: vec![node],
                }),
            }
        }
    }

    // What each op's region reads, by name: the one home that already knows
    // what an op goes around, runs over and is walled by.
    let mut shaped: Vec<(String, usize, Shaping)> = Vec::new();
    for (idx, op) in p.operations.iter().enumerate() {
        let Some(region) = crate::region::of(p, op) else { continue };
        let named = |names: Vec<&str>, how: Shaping| names.into_iter().map(move |n| (n.to_string(), idx, how)).collect::<Vec<_>>();
        shaped.extend(named(region.feature_islands(), Shaping::Island));
        shaped.extend(named(region.open_voids(), Shaping::OpenVoid));
        shaped.extend(named(region.bridged(), Shaping::Bridged));
        shaped.extend(named(region.closed_by.iter().map(|n| n.as_str()).collect(), Shaping::ClosesEdge));
        shaped.extend(named(region.grown.iter().map(|n| n.as_str()).collect(), Shaping::GrownInto));
    }

    let mut shaping: Vec<ShapingNode> = Vec::new();
    let mut unused: Vec<FeatureNode> = Vec::new();
    for (name, feat) in p.features.iter().filter(|(name, _)| !features.iter().any(|n| &n.name == *name)) {
        let mut uses: Vec<(usize, Shaping)> = shaped.iter().filter(|(n, ..)| n == name).map(|(_, idx, how)| (*idx, *how)).collect();
        uses.dedup();
        if uses.is_empty() {
            // A given void is read — by the stock model, at prefix 0 — so
            // it is listed under its own heading below, never as unused.
            if !p.given.contains(name) {
                unused.push(FeatureNode { name: name.clone(), kind: feat.kind(), ops: Vec::new() });
            }
        } else {
            shaping.push(ShapingNode { name: name.clone(), kind: feat.kind(), uses });
        }
    }
    shaping.sort_by(|a, b| a.name.cmp(&b.name));
    unused.sort_by(|a, b| a.name.cmp(&b.name));
    let given: Vec<FeatureNode> = p
        .given
        .iter()
        .filter_map(|name| p.features.get(name).map(|f| FeatureNode { name: name.clone(), kind: f.kind(), ops: Vec::new() }))
        .collect();

    let mut nums: Vec<usize> = p.tools.keys().copied().collect();
    nums.sort_unstable();
    let tools: Vec<ToolNode> = nums
        .into_iter()
        .map(|num| ToolNode {
            num,
            label: tool_label(num, p.tools.get(&num)),
            ops: p.operations.iter().enumerate().filter(|(_, op)| op.tool_num == num).map(|(i, _)| i).collect(),
        })
        .collect();

    let removals = (0..p.operations.len()).map(|i| removal_line(p, i)).collect();
    let given_air = p.operations.iter().map(|op| crate::stock::given_air(p, op).iter().map(|l| l.line()).collect()).collect();
    let given_from = given_from_lines(p);
    ProjectOutline { stock: p.stock.clone(), features, shaping, unused, given, tools, removals, given_air, given_from }
}

/// The `given from` lines (Phase 48 slice 1): the statement with what the
/// stock model read of the earlier program, then each decline by name.
fn given_from_lines(p: &Part) -> Vec<String> {
    let (Some(gf), Some(air)) = (&p.given_from, crate::stock::given_from_air(p)) else {
        return Vec::new();
    };
    let turn = match gf.up {
        crate::parse::UpAxis::ZPlus => "the same side up (no turn)",
        crate::parse::UpAxis::ZMinus => "turned over (y → −y, z → −z)",
    };
    let mut out = vec![format!(
        "given from {} up={}: {} removal(s) from {} of its {} op(s), {} — air at prefix 0{}",
        gf.project,
        gf.up.word(),
        air.removals.len(),
        air.ops_read,
        gf.part.operations.len(),
        turn,
        if air.declined.is_empty() { "" } else { "; the model is INCOMPLETE for what follows" }
    )];
    out.extend(air.declined.iter().map(|d| format!("declined: {d}")));
    // Each restated void's verdict (Phase 48 slice 2). A standing sample
    // never reaches here — it refused at validate — so the verdicts are the
    // certificate's pass, with its count, or its decline.
    for name in &p.given {
        match crate::stock::certify_given(p, name) {
            Some(crate::stock::Certificate::Certified { samples }) => {
                out.push(format!("certified: {name} — {samples} sample(s) in the earlier program's air"))
            }
            Some(crate::stock::Certificate::Declined) => {
                out.push(format!("not certified: {name} — the earlier program's model is incomplete, so the certificate did not read it"))
            }
            Some(crate::stock::Certificate::Standing(_)) | None => {}
        }
    }
    out
}

/// One tool's row, shared verbatim by the text tree and the window's Tools
/// heading (DESIGN.md §5.2) — the label and the operations that run it, or
/// `never run`, which is the fact the row exists to show.
pub fn tool_row(node: &ToolNode) -> String {
    if node.ops.is_empty() {
        return format!("{}  never run", node.label);
    }
    let runs: Vec<String> = node.ops.iter().map(|i| format!("[{i}]")).collect();
    format!("{}  runs {}", node.label, runs.join(" "))
}

/// How a tool is named in the tree.
///
/// A `CornerRounder` and a `ChamferMill` carry their BODY OD in `dia`, which is
/// clearance — "the one number on the tool that must never reach a comp
/// register" (DECISIONS.md → "the corner rounder gets an operation"). The
/// G-code header already rejects to print it bare for exactly that reason, and
/// this tree makes the same refusal: a rounder is named by its ground arc, a
/// chamfer mill by its included angle. A picture that labels a rounder with its
/// pilot-adjacent body diameter is the original trap, drawn.
pub fn tool_label(num: usize, tool: Option<&Tool>) -> String {
    let t = match tool {
        Some(t) => t,
        // validate() pins every op's tool, so this is a formatting fallback,
        // not a machining decision.
        None => return format!("T{}", derive_t(num)),
    };
    if let Some(rd) = &t.rounder {
        return format!("T{} {} R{}", derive_t(num), t.name, f(rd.radius));
    }
    if let Some(ch) = &t.chamfer {
        return format!("T{} {} {}deg", derive_t(num), t.name, f(ch.angle));
    }
    format!("T{} {} dia {}", derive_t(num), t.name, f(t.dia))
}

/// The program's operations in RUN order — the other axis of the tree
/// (DESIGN.md §5.2, Sequence): every `OpNode` once, by index, gathered
/// from the feature nodes so the two presentations share one node. A
/// Drill over several holes is filed under each of them in the tree and
/// appears once here — it is one operation, run once.
pub fn sequence(o: &ProjectOutline) -> Vec<OpNode> {
    let mut ops: Vec<OpNode> = Vec::new();
    for node in &o.features {
        for op in &node.ops {
            if !ops.iter().any(|seen| seen.idx == op.idx) {
                ops.push(*op);
            }
        }
    }
    ops.sort_by_key(|op| op.idx);
    ops
}

/// One op's line, shared verbatim by the text tree and the window's outline
/// panel — one formatter, so the two presentations cannot drift (and the
/// rounder/chamfer body-OD error in `tool_label` holds in both).
pub fn op_line(p: &Part, op: &OpNode) -> String {
    let allowance = match (op.role, op.allowance) {
        // `leaves` is the ceiling the pass GUARANTEES; `rides` is the surface
        // it actually cuts to, `allowance − cusp` on both cusp-spending arms.
        // Printed only when the two differ: on a prismatic flank the skin IS
        // the allowance and a second copy of the number would be noise. When
        // the model declines the line says so — the one user-facing summary
        // stating the ceiling as if it were the leave is what this line
        // existed to stop doing.
        (OpRole::Roughing, Some(a)) => match op.rides {
            Some(s) if s < a => format!("  leaves {} (rides {})", f(a), f(s)),
            Some(_) => format!("  leaves {}", f(a)),
            None => format!("  leaves {} (rides unmodelled)", f(a)),
        },
        _ => String::new(),
    };
    format!(
        "[{}] {:?}  {}  {}{}",
        op.idx,
        op.op_type,
        tool_label(op.tool_num, p.tools.get(&op.tool_num)),
        op.role.label(),
        allowance
    )
}

/// What the stock model says an op removes — one line, the `--outline`
/// voice of `stock::removals_for` (Phase 11, lane B), so the window's
/// removal wires are a view of something the terminal prints. Each removal
/// as its section in a few words and its z span; an op the model has no
/// closed form for says so in the same words its consumers decline with.
pub fn removal_line(p: &Part, op_idx: usize) -> String {
    let op = match p.operations.get(op_idx) {
        Some(op) => op,
        None => return String::new(),
    };
    match crate::stock::removals_for(p, op) {
        None => "removes: no closed form — the model declines this op".to_string(),
        Some(rs) if rs.is_empty() => "removes: nothing the model can state".to_string(),
        Some(rs) => {
            let parts: Vec<String> = rs
                .iter()
                .map(|r| {
                    let (lo, hi) = r.z_span();
                    format!("{} z {}..{}", r.section.describe(), f(lo), f(hi))
                })
                .collect();
            format!("removes: {}", parts.join("; "))
        }
    }
}

fn render_feature(out: &mut String, p: &Part, node: &FeatureNode, given_air: &[Vec<String>]) {
    out.push_str(&format!("{}  {}\n", node.name, node.kind));
    for op in &node.ops {
        out.push_str(&format!("  {}\n", op_line(p, op)));
        out.push_str(&format!("      {}\n", removal_line(p, op.idx)));
        if let Some(line) = region_line(p, op.idx) {
            out.push_str(&format!("      {}\n", line));
        }
        // The air a body can stand in (Phase 42 slice 2), one line per
        // given void, voiced here before any emitter reads it.
        for line in given_air.get(op.idx).into_iter().flatten() {
            out.push_str(&format!("      {}\n", line));
        }
    }
}

/// The op's floor region, when it has one worth a line (Phase 27): what it
/// goes around, what it bridges, which of its edges are open, and the
/// narrowest place the tool must pass. A bare face and an empty closed pocket
/// print nothing — they are what every reader already assumes.
pub fn region_line(p: &Part, op_idx: usize) -> Option<String> {
    let op = p.operations.get(op_idx)?;
    // The rest floor (Phase 33) is not the host's whole region, and saying so
    // is the line: which floors, whose, how much.
    if crate::rest::is_rest(op) {
        return Some(crate::rest::line(p, op));
    }
    let region = crate::region::of(p, op)?;
    // **Through** (Phase 61 slice 3, fork 6A) is DERIVED, never declared: a
    // floor at or below the blank's bottom is a cut that goes through it, and
    // the line says so. What stands under the blank — the table, a fixture —
    // is the machinist's to know; a floor strictly BELOW the bottom is the
    // containment lint's advisory ("N mm below the stock's bottom"), which
    // already exists. No `through` key: the z answers it.
    let through = region.z <= p.stock.z - p.stock.h + 1e-6;
    if region.is_plain() && !through {
        return None;
    }
    let dia = p.tools.get(&op.tool_num)?.dia;
    let line = region.line(dia);
    Some(if through { format!("{line}; through the blank") } else { line })
}

/// The stock's row, shared verbatim by the text tree and the window's
/// Outline (Phase 22 slice 1: the row that selects the stock is a view of
/// what the terminal prints). Every number here is mm: inch inputs were
/// converted at the parse boundary (DECISIONS.md "Internal units are mm,
/// always"), and `stock.z` is the TOP datum — the stock hangs below it.
pub fn stock_row(s: &crate::parse::Stock) -> String {
    match s.shape {
        crate::parse::StockShape::Box => format!(
            "stock  {} x {} x {} mm  origin ({}, {})  top z {}",
            f(s.w), f(s.l), f(s.h), f(s.x), f(s.y), f(s.z)
        ),
        // A round blank says its axis, as its line does (Phase 46 slice 1).
        crate::parse::StockShape::Cylinder { dia } => {
            let (cx, cy) = s.axis();
            format!("stock  dia {} x {} mm  axis ({}, {})  top z {}", f(dia), f(s.h), f(cx), f(cy), f(s.z))
        }
        // A profile blank says its outline's count and the box it fits
        // (Phase 54 slice 5a).
        crate::parse::StockShape::Profile { ref points } => format!(
            "stock  profile of {} points within {} x {} x {} mm  origin ({}, {})  top z {}",
            points.len(), f(s.w), f(s.l), f(s.h), f(s.x), f(s.y), f(s.z)
        ),
    }
}

/// The text presentation of the tree — one rendering of `ProjectOutline`, not
/// the thing itself.
pub fn render(p: &Part, o: &ProjectOutline) -> String {
    let mut out = String::new();
    out.push_str(&stock_row(&o.stock));
    out.push('\n');
    out.push_str(&format!(
        "{} feature(s), {} operation(s), {} tool(s)\n",
        o.features.len() + o.shaping.len() + o.unused.len(),
        p.operations.len(),
        o.tools.len()
    ));

    for node in &o.features {
        out.push('\n');
        render_feature(&mut out, p, node, &o.given_air);
    }

    // The declarations (Phase 20 slice 1): every tool in number order,
    // each with the operations that run it — the terminal voice the
    // window's Tools heading is a view of.
    if !o.tools.is_empty() {
        out.push_str("\ntools:\n");
        for node in &o.tools {
            out.push_str(&format!("  {}\n", tool_row(node)));
        }
    }

    // "declared, never run" was three things under one heading (Phase 37
    // slice 5): a feature an op names is above; one that shapes what an op
    // cuts is here, with how; and only what is left is unused.
    if !o.shaping.is_empty() {
        out.push_str("\nindirectly used (no operation names it; it shapes what one cuts):\n");
        for node in &o.shaping {
            out.push_str(&format!("  {}\n", shaping_row(node)));
        }
    }
    if !o.unused.is_empty() {
        out.push_str("\nunused (no operation names it or is shaped by it):\n");
        for node in &o.unused {
            out.push_str(&format!("  {}  {}\n", node.name, node.kind));
        }
    }
    // What is already air (Phase 42 slice 1): the stock model subtracts
    // these before the first op, so a reader of the tree sees what the
    // program counts on having been cut elsewhere.
    if !o.given.is_empty() || !o.given_from.is_empty() {
        out.push_str("\ngiven (air before this program starts — cut on another setup):\n");
        for node in &o.given {
            out.push_str(&format!("  {}  {}\n", node.name, node.kind));
        }
        // The program before this one (Phase 48 slice 1): what the stock
        // model read of it, and what it could not carry, by name.
        for line in &o.given_from {
            out.push_str(&format!("  {line}\n"));
        }
    }

    out
}

/// The `--outline` report: build the tree and render it.
pub fn report(p: &Part) -> String {
    render(p, &outline(p))
}
