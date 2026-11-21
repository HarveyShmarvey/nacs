use crate::math::Portable as _;
// The small `derive_*()` homes: registers, stepover, roles, contact, tilted bores, loops, edge rounds and chamfers.

use super::*;

// The `feed=derive` / `spindle=derive` resolution — the sentinel's one
// consumer (fork 4A). Runs once at the end of parse_project_with_base, after
// validate (so tools exist) and after the mm conversion (so every number it
// reads and writes is metric), writing plain numbers into the ops' own
// `feed`/`spindle` Options. Emission never learns the table exists. A program
// with no sentinel never touches this path — byte-for-byte today's behavior.
pub(super) fn resolve_derived_words(p: &mut Part) -> Result<(), String> {
    if !p.operations.iter().any(|o| o.feed_derive || o.spindle_derive) {
        return Ok(());
    }
    let shipped = parse_feeds(STARTER_FEEDS)
        .map_err(|e| format!("shipped feeds table (tables/starter.feeds): {}", e))?;
    let shop = p.shop_feeds.as_ref();
    // Measured before the mutable walk, because the engagement of an op is a
    // fact about the whole `Part` (its features and tools) and the walk below
    // borrows the operations mutably. Same call the corner advisory makes, so
    // the two cannot drift.
    let engagements: Vec<Option<crate::engagement::PocketEngagement>> =
        p.operations.iter().map(|op| op_engagement(p, op)).collect();
    let tools = &p.tools;
    let stock_material = p.stock.material.as_deref();
    let global_spindle = p.global_spindle;
    for (op_idx, op) in p.operations.iter_mut().enumerate() {
        if !(op.feed_derive || op.spindle_derive) {
            continue;
        }
        let which = if op.feed_derive && op.spindle_derive {
            "feed=derive / spindle=derive"
        } else if op.feed_derive {
            "feed=derive"
        } else {
            "spindle=derive"
        };
        // validate() ran first, so the tool exists.
        let tool = &tools[&op.tool_num];
        // The form tools' `dia` is BODY clearance, not a cutting diameter —
        // nothing feeds-shaped can honestly be computed from it.
        if matches!(tool.name.as_str(), "FormTool" | "CornerRounder" | "ChamferMill") {
            return Err(format!(
                "op [{}] ({:?}): {} with tool T{} ({}) — a form tool's dia is body clearance, not a cutting diameter, so feeds derivation rejects it",
                op_idx, op.op_type, which, op.tool_num, tool.name
            ));
        }
        if tool.dia <= 0.0 {
            return Err(format!(
                "op [{}] ({:?}): {} with tool T{} dia {} — derivation needs a positive cutting diameter",
                op_idx, op.op_type, which, op.tool_num, tool.dia
            ));
        }
        let Some(material) = stock_material else {
            return Err(format!(
                "op [{}] ({:?}): {} but the stock declares no material — the lookup needs the workpiece axis; declare material=<name> on the stock Box",
                op_idx, op.op_type, which
            ));
        };
        let rec = feeds_lookup(shop, &shipped, material, &tool.name)
            .map_err(|e| format!("op [{}] ({:?}): {}: {}", op_idx, op.op_type, which, e))?;
        if op.spindle_derive {
            op.spindle = Some(derive_spindle_rpm(rec.surface_speed, tool.dia));
        }
        if op.feed_derive {
            // F = RPM × z × f_z. The chip load is ours; the flute count z is
            // the TOOL's fact (K2's field, landed). Both halves must exist
            // honestly before a feed is written, so the band is checked
            // first — its miss names itself — and then the flute count
            // rejects by name, the slice-0 ordering kept: this slice only
            // replaced the final error with the second identity.
            let band = rec.chiploads.iter().find(|b| b.dia_lo <= tool.dia && tool.dia < b.dia_hi);
            let Some(band) = band else {
                let held: Vec<String> = rec
                    .chiploads
                    .iter()
                    .map(|b| format!("{}..{}", b.dia_lo, b.dia_hi))
                    .collect();
                return Err(format!(
                    "op [{}] ({:?}): feed=derive: no chipload band covering dia {} for {}\u{d7}{}; the record holds bands: {}",
                    op_idx, op.op_type, tool.dia, rec.material, rec.tool_type,
                    if held.is_empty() { "(none)".to_string() } else { held.join(", ") }
                ));
            };
            let Some(flute_count) = tool.flute_count else {
                return Err(format!(
                    "op [{}] ({:?}): feed=derive: tool T{} declares no flute count — the chip-load\u{2192}feed conversion (F = RPM \u{d7} z \u{d7} f_z) needs the tool's flute count, a tool-body field this tool line does not carry",
                    op_idx, op.op_type, op.tool_num
                ));
            };
            // The RPM that feeds the identity is the op's RESOLVED spindle —
            // the just-derived number when `spindle=derive` rode along, the
            // declared/global number otherwise — because the chip thins by
            // what the spindle actually turns, never by what the table would
            // have recommended for a spindle the author overrode. A resolved
            // spindle of 0 (nothing declared anywhere) would make F = 0 wear
            // the look of a chosen number, fork 2B's exact failure mode — so
            // it rejects by name instead.
            let rpm = derive_spindle_speed(op, global_spindle);
            if rpm <= 0.0 {
                return Err(format!(
                    "op [{}] ({:?}): feed=derive but the resolved spindle is {} — the chip-load identity multiplies by the RPM the spindle actually turns; declare spindle= on the op or the program, or ask spindle=derive",
                    op_idx, op.op_type, rpm
                ));
            }
            let base = derive_feed_mm_min(rpm, flute_count, band.mm_per_tooth);
            // The chip-thinning correction (Phase 8 V1 slice 1). The chip-load
            // identity assumes the programmed feed per tooth IS the chip; below
            // half immersion it is not, and the correction that restores it is
            // exactly `1/sin(theta_e)` — so the measured engagement is not an
            // adjustment bolted onto the feed, it is the term the identity was
            // missing. Fork 3A picks the WORST engagement over the pass rather
            // than a steady-state one: the factor falls as engagement rises, so
            // pricing at the maximum yields the SMALLEST boost — conservative,
            // and the same number the corner advisory reads.
            //
            // No engagement, no correction: a carrier this cannot measure keeps
            // K3's identity untouched rather than being guessed at.
            match &engagements[op_idx] {
                Some(pe) => {
                    let ct = crate::engagement::chip_thinning(pe.worst.radians);
                    op.feed = Some(base * ct.factor);
                    op.chip_thinning = Some(ct);
                    // Per pass, in emission order: the slot reads pi and takes
                    // the identity unchanged; every later loop takes the
                    // factor its OWN worst position earns, never the op's.
                    // Only for the plain strategy — a field-trimmed family
                    // (`rest`/`regions`) keeps a subset of these loops, and a
                    // per-level list would index the wrong one.
                    if op.strategy.is_none() {
                        op.pass_feeds = Some(
                            pe.passes
                                .iter()
                                .map(|e| base * crate::engagement::chip_thinning(e.radians).factor)
                                .collect(),
                        );
                    }
                }
                None => op.feed = Some(base),
            }
        }
    }
    Ok(())
}

// ---- derived values: one source, computed on demand (DRY) ------------------
pub fn derive_t(tool_num: usize) -> usize {
    tool_num
}

pub fn derive_h(tool_num: usize) -> usize {
    tool_num
}

pub fn derive_d(tool_num: usize) -> usize {
    tool_num
}

/// The RPM an op's spindle actually turns: the op's own `spindle=` (the
/// derive sentinel already resolved into it), the program's global
/// otherwise. This existed as `op.spindle.unwrap_or(p.global_spindle)`
/// restated at four sites — the emitter, the chip-load pricer, the
/// engagement prose, the feed derivation — which is exactly the duplicated
/// derived value the DRY rule exists for; the ceiling check (Phase 15
/// slice 2) made it a fifth and became the reason to name it once. Takes
/// the global as a value so the derive-resolution loop, which holds `&mut`
/// ops, can call it too.
pub fn derive_spindle_speed(op: &Operation, global_spindle: f64) -> f64 {
    op.spindle.unwrap_or(global_spindle)
}

// The floor of a feature's vertical occupancy: its top datum less its extent
// (DECISIONS.md "Vertical occupancy is `extent`…"). Both arguments are
// feature-owned geometry — `CylinderBore`'s authored `depth`, a `Contour`'s
// `extent`, an analytic surface's derived height — never an operation input.
/// Why a stepover-mode roughing budget could not be chosen. The emitter turns
/// these into its own carrier-worded error; the shapes live here so the
/// arithmetic that finds them does not have to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BudgetFail {
    /// Adjacent passes do not overlap — an uncut ridge, which no allowance can
    /// ceiling.
    Ridge,
    /// The worst cusp meets or exceeds the allowance. Carries the worst price
    /// seen so the message can state it.
    Exceeded(f64),
}

/// The sweep-then-VERIFY budget chooser (DECISIONS.md "stock_to_leave corner
/// roll…"), for every carrier whose exact-tip price has no monotonicity theorem
/// in the skin: the budget `h` is the max of `price` over the 5-point skin
/// sweep {0, ¼, ½, ¾, 1}·allowance (choosing a' = allowance − h), and the
/// ACTUAL emitted stack is then re-priced at a' — the guarantee rests on that
/// verification, never on the sweep having caught the worst alignment.
pub(crate) fn sweep_verify_budget(
    allowance: f64,
    price: &dyn Fn(f64) -> f64,
) -> Result<(f64, f64), BudgetFail> {
    let mut h: f64 = 0.0;
    for k in 0..=4 {
        h = h.max(price(allowance * k as f64 / 4.0));
    }
    if h.is_infinite() {
        return Err(BudgetFail::Ridge);
    }
    let verified = if h < allowance { price(allowance - h) } else { h };
    if verified.is_infinite() {
        return Err(BudgetFail::Ridge);
    }
    if h >= allowance || (allowance - h) + verified > allowance + 1e-9 {
        return Err(BudgetFail::Exceeded(h.max(verified)));
    }
    Ok((allowance - h, h))
}

/// The `(skin, cusp)` split a STEPOVER-mode roughing pass rides — one home,
/// read by the emitter that cuts it and by the stock model that places it
/// (DECISIONS.md → "the roughing allowance has two arms…", fork 3A).
///
/// `None` means this function has no arm for the op: no allowance, scallop mode
/// (whose split is the trivial `allowance − scallop`), or a carrier not yet
/// moved here. A `None` carrier keeps the emitter's own local budget and the
/// model keeps declining for it, which is where every carrier started.
/// The feature and tool arrive by REFERENCE rather than by name lookup: the
/// emitters are handed both already, and a by-name lookup here silently
/// answered `None` for every caller whose `Part` did not also carry the feature
/// under that name — which the emitter unit tests do not, and which turned a
/// missing arm into a zero skin instead of a loud failure.
pub fn derive_stepover_budget(
    feature: &Feature,
    tool: &Tool,
    op: &Operation,
) -> Option<Result<(f64, f64), BudgetFail>> {
    let allowance = op.stock_to_leave?;
    if op.scallop.is_some() {
        return None;
    }
    let stepover = op.stepover?;
    let r_ball = tool.dia / 2.0;
    match feature {
        // Priced ONCE at the allowance rather than swept: the ellipsoid's
        // fixed-Z stack has its cusp bounded at the top, so pricing there
        // bounds the emitted stack (see derive_ellipsoid_stack_cusp).
        // Same shape, priced at the allowance: the meridian's cusp is bounded
        // by the pricing point and does not move with the skin.
        // Priced at skin ZERO, which is the emitter's own choice on both
        // tractrix carriers: the band's cusp is skin-independent there, so the
        // nominal pricing bounds every inflation of it.
        // Swept: the arc's cusp moves with the skin, so no extreme walk is
        // claimed and the emitted stack is re-priced at the chosen a'.
        // Swept over a grid rebuilt from `stepover` — the same uniform contact
        // grid the emitter walks, sized by the nominal rim arc.
        // Swept, and pure — the spline pricers take the meridian and the
        // spacing and build nothing.
        Feature::SplineRevolve { meridian, .. } => Some(sweep_verify_budget(allowance, &|s| {
            derive_spline_stack_cusp(meridian, r_ball, s, stepover)
        })),
        Feature::SplineRevolveFunnel { meridian, .. } => {
            Some(sweep_verify_budget(allowance, &|s| {
                derive_spline_funnel_stack_cusp(meridian, r_ball, s, stepover)
            }))
        }
        // Swept: the uniform-arc grid's junction alignment moves with the skin
        // (the inflated neck arc shortens as the belly's lengthens), so no
        // extreme walk is claimed.
        _ => None,
    }
}

/// The skin a pass actually CUTS to, as opposed to the ceiling it guarantees —
/// or `None` when this model cannot know it.
///
/// `stock_to_leave` is max-not-min (DECISIONS.md "Stock-to-leave semantics…"): a
/// bound on total remaining material INCLUDING the pass's own cusps. A
/// scallop-mode pass therefore rides `allowance − scallop` and lets its cusps
/// rise into the rest of the budget. **A stepover-mode pass on a curved surface
/// does the same thing** — it rides `allowance − cusp`, where the cusp is chosen
/// by the emitter's sweep-then-verify and stated in the program's own header
/// ("cusp budget 0.173mm"). This function cannot reconstruct that number: the
/// pricers take march grids built inside the emitter, so the choice is not a
/// function of `Part` alone.
///
/// It used to return the allowance there, and the paragraph above this line
/// said why that was wrong before the code did it anyway — *modelling the
/// ceiling instead puts material where the tool legitimately is*. It did:
/// `removals_for` placed `vase_rough`'s surface 0.173mm outside where the ball
/// actually went, and `check_moves` rejected a correct program on it
/// (DECISIONS.md → "the vase's roughing pass rides what it promises…").
///
/// So it declines instead, and `None` is a first-class answer for the reason
/// `RemainingStock::complete` already gives: an incomplete model over-states
/// material, and rejecting a program on an over-statement is the worst outcome
/// the stock model can produce. Declining says "I do not know" where returning
/// the ceiling said something false.
pub fn derive_emitted_skin(p: &Part, op: &Operation) -> Option<f64> {
    match (op.stock_to_leave, op.scallop) {
        (None, _) => Some(0.0),
        // Exactly the emitter's own arithmetic, on every carrier.
        (Some(allowance), Some(scallop)) => Some(allowance - scallop),
        (Some(allowance), None) => {
            let spends_a_cusp = p
                .features
                .get(&op.feature_name)
                .is_some_and(|f| op.follows_surface() && surface_spends_a_cusp(f));
            if !spends_a_cusp {
                return Some(allowance);
            }
            // A carrier that has been moved to the shared budget home answers
            // exactly what the emitter rides; one that has not still declines.
            let feature = p.features.get(&op.feature_name)?;
            let tool = p.tools.get(&op.tool_num)?;
            match derive_stepover_budget(feature, tool, op) {
                Some(Ok((skin, _))) => Some(skin),
                // A budget the emitter will reject outright leaves no program
                // to model, so the answer does not matter — decline rather
                // than invent one.
                Some(Err(_)) => None,
                None => None,
            }
        }
    }
}

/// Whether a stepover pass over this feature spends part of its allowance on
/// its own cusps. True for every CURVED carrier — adjacent passes on a curve
/// leave a scallop between them, which the budget has to cover. False for the
/// prismatic flanks (an exact wall has no cusp) and for the pocket floors,
/// whose cusp budget `ops::clear` calls structurally zero for the same reason.
pub(super) fn surface_spends_a_cusp(feature: &Feature) -> bool {
    matches!(
        feature,
        Feature::Sphere { .. }
            | Feature::Cone { .. }
            | Feature::Torus { .. }
            | Feature::SplineRevolve { .. }
            | Feature::SplineRevolveFunnel { .. }
            | Feature::Mesh { .. }
            | Feature::NurbsPatch { .. }
    )
}

/// An operation's role, DERIVED — never declared (DECISIONS.md → "Operation
/// role is derived, not declared…"). There is no `mode` field on `Operation`
/// and there will not be one; the tag is `stock_to_leave`'s PRESENCE, which is
/// why this is a function and not a stored value.
///
/// Three arms, not two, and the third is the honest one. `validate()` rejects
/// `stock_to_leave` on `Floor` and `Drill` outright — a faced floor IS the
/// Plane's own `z`, and a drilled hole's diameter is the tool's own — so those
/// ops cannot carry the axis at all. Collapsing them into `Finishing` would
/// make the Phase-6 map view color a drill cycle "finishing" with full
/// confidence, which is a lie a picture tells better than text does. They read
/// `Untagged`: the role axis does not apply, which is a different statement
/// from "this pass leaves nothing behind".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpRole {
    /// The pass leaves an allowance for a later one. `stock_to_leave` is
    /// present, and validate() has already rejected a zero (0 IS the finishing
    /// pass), so this arm always means a real skin.
    Roughing,
    /// The pass cuts to the declared surface: `Finish` or `Clear` with no
    /// allowance.
    Finishing,
    /// The op has no role axis — `Floor` and `Drill`, which reject the tag.
    Untagged,
}

impl OpRole {
    /// The word the outline and any later renderer print. One home so the two
    /// cannot disagree about what to call an arm.
    pub fn label(&self) -> &'static str {
        match self {
            OpRole::Roughing => "roughing",
            OpRole::Finishing => "finishing",
            OpRole::Untagged => "untagged",
        }
    }
}

/// The one home for the role derivation — of the TAG, and the tag is all
/// there is. The derived role *scale* this comment once promised ("the map
/// view's two colors become a gradient") was closed by ruling 2026-08-21
/// (`DECISIONS.md` → "Phase 9 lane D — the operation-role scale does not earn
/// its place…"): what a pass leaves is `outline::op_line`'s `leaves a (rides
/// s)`, what it removes is `band_findings`' per-pass reading, and neither is
/// a number per op to colour by. Three values, one home, no gradient.
pub fn derive_op_role(op: &Operation) -> OpRole {
    match op.op_type {
        // These two reject the tag at validate(), so there is nothing to read.
        _ if op.at_points() => OpRole::Untagged,
        OpType::End => OpRole::Untagged,
        OpType::Clear | OpType::Side | OpType::Copy | OpType::Form => match op.stock_to_leave {
            Some(_) => OpRole::Roughing,
            None => OpRole::Finishing,
        },
    }
}

/// A cutting surface of a tool — the part of it that can make a surface
/// (`DECISIONS.md` → "DRAFT: Phase 36 opens — OPS BY CONTACT…", fork 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    /// The flat end: it makes a floor.
    End,
    /// The flank: it makes a wall.
    Side,
    /// A ball's nose: it touches an in-between surface at a point.
    Nose,
    /// A ground form: the tool carries the shape, the path only places it.
    Form,
    /// The back of a cutting envelope that narrows going up: it makes a ceiling.
    Back,
}

impl Cut {
    pub fn label(&self) -> &'static str {
        match self {
            Cut::End => "end",
            Cut::Side => "side",
            Cut::Nose => "nose",
            Cut::Form => "form",
            Cut::Back => "back",
        }
    }
}

/// The one home for WHICH OF A TOOL'S SURFACES CUT (Phase 36 slice 0).
/// Derived, never declared: the records present decide first, as they do in
/// `Silhouette::of`, and the kind word decides between the rods. A `Drill`
/// is a form tool — its diameter is the wall and its ground point the cone —
/// and it cuts nowhere else.
pub fn derive_tool_cuts(tool: &Tool) -> Vec<Cut> {
    let sil = Silhouette::of(tool);
    match sil {
        Silhouette::Profile { .. } | Silhouette::Rounder { .. } | Silhouette::Chamfer { .. } => {
            let mut cuts = vec![Cut::Form];
            if sil.narrowing().is_some() {
                cuts.push(Cut::Back);
            }
            cuts
        }
        Silhouette::Ball { .. } => vec![Cut::Nose, Cut::Side],
        Silhouette::Flat { .. } if tool.name == "Drill" => vec![Cut::Form],
        Silhouette::Flat { .. } => vec![Cut::End, Cut::Side],
    }
}

/// Is the feature a WALL — a prismatic pair, whose surface the tool's flank
/// makes? The eight that are exempt from the ball-nose requirement.
pub fn is_wall(f: &Feature) -> bool {
    matches!(
        f,
        Feature::CylinderBore { .. }
            | Feature::CylinderBoss { .. }
            | Feature::ProfilePocket { .. }
            | Feature::ProfileBoss { .. }
            | Feature::ContourPocket { .. }
            | Feature::ContourBoss { .. }
    )
}

/// The one home for WHICH CONTACT VERB a tool on a surface is (Phase 36
/// slice 1): a tool that carries the shape forms; otherwise a wall is the
/// flank's and anything between wall and floor is the nose's. The verb the
/// user typed is checked against this and never replaced by it.
pub fn derive_contact(tool: &Tool, feature: &Feature) -> OpType {
    if derive_tool_cuts(tool).contains(&Cut::Form) {
        OpType::Form
    } else if is_wall(feature) {
        OpType::Side
    } else {
        OpType::Copy
    }
}

/// The contact verb of a tool AT POINTS (Phase 36 slice 3): a tool that
/// carries the shape forms, a flat end seats, and anything else has no
/// business plunging.
pub fn derive_point_contact(tool: &Tool) -> Option<OpType> {
    let cuts = derive_tool_cuts(tool);
    if cuts.contains(&Cut::Form) {
        Some(OpType::Form)
    } else if cuts.contains(&Cut::End) {
        Some(OpType::End)
    } else {
        None
    }
}

/// Why `derive_contact` answered as it did, for a refusal to say.
pub fn contact_reason(tool: &Tool, feature: &Feature) -> String {
    match derive_contact(tool, feature) {
        OpType::Form => format!("form cut (T{} is a {}, a tool that carries the shape)", tool.num, tool.name),
        OpType::Side => format!("side cut (a {} is a wall, and the tool's flank makes it)", feature.kind()),
        _ => format!("copy cut (a {} lies between wall and floor, where a ball's nose follows it)", feature.kind()),
    }
}

/// The one home for WHAT PATTERN AN OP CUTS (Phase 35 slice 0): its pin, or
/// the default. A `Floor` defaults to `spine` (Phase 35 slice 4) and a
/// `Clear` to the offset family — the family the band, `strategy=` and
/// `stepdown` are built on, and the one that holds in a closed pocket. An op
/// that works no region reads `None`. The emitters, the stock model's claim
/// and the comparison report all ask here.
pub fn derive_pattern(_p: &Part, op: &Operation) -> Option<Pattern> {
    match op.op_type {
        OpType::End if op.at_points() => None,
        OpType::End => Some(op.pattern.unwrap_or(Pattern::Spine)),
        OpType::Clear => Some(op.pattern.unwrap_or(Pattern::Offset)),
        OpType::Side | OpType::Copy | OpType::Form => None,
    }
}

/// The line a held op holds by default: 90°, the linter's own
/// (`engagement::Band::Past90`) and the ridge law's edge (`a_e = r`).
pub const HELD_DEFAULT_DEG: f64 = 90.0;

/// The ceiling a `pattern=held` op lays its rows under, in degrees: its
/// `engagement=`, or the 90° line (Phase 62 slice 1).
pub fn derive_held_ceiling(op: &Operation) -> f64 {
    op.engagement.unwrap_or(HELD_DEFAULT_DEG)
}

/// The spacing a ceiling derives on a tool of radius `r`: a stroke beside a
/// previous pass at radial depth `a_e` engages `acos(1 − a_e/r)`, so the
/// depth that engages exactly `deg` is `r(1 − cos deg)` — the radius itself
/// at 90°.
pub fn held_spacing(r: f64, deg: f64) -> f64 {
    // (Exactly the radius at 90°: `cos(π/2)` is 6e-17, and the spacing the
    // ridge law and the goldens read must be `r`, not `r` less a hair.)
    let c = deg.to_radians().pcos();
    r * (1.0 - if c.abs() < 1e-12 { 0.0 } else { c })
}

/// The one home for an op's STEPOVER (Phase 62 slice 1): its authored
/// `stepover=`, or — under `pattern=held`, which refuses one — the spacing
/// its ceiling derives on its tool. Every emitter and every law that read
/// `op.stepover` for a floor op reads here.
pub fn derive_stepover(p: &Part, op: &Operation) -> Option<f64> {
    op.stepover.or_else(|| {
        if op.pattern != Some(Pattern::Held) {
            return None;
        }
        p.tools.get(&op.tool_num).map(|t| held_spacing(t.dia / 2.0, derive_held_ceiling(op)))
    })
}

/// An angle in degrees as a program says it: three decimals, the trailing
/// zeros dropped — `90`, `72.5`.
pub fn deg_word(deg: f64) -> String {
    let s = crate::gcode::f(deg);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Where `pattern=raster` is legal: a `Floor` on a `Plane` with nothing
/// standing in it and no fixture on it, that is not a rest op. One home —
/// validate refuses by it and the comparison report shows the row by it.
pub fn raster_applies(p: &Part, op: &Operation) -> bool {
    op.op_type == OpType::End
        && !crate::rest::is_rest(op)
        && matches!(p.features.get(&op.feature_name), Some(Feature::Plane { .. }))
        && crate::region::of(p, op).is_none_or(|region| region.islands().is_empty())
}

pub fn derive_z_cut(z_top: f64, extent: f64) -> f64 {
    z_top - extent
}

/// A tilted bore's axis direction going DOWN into the stock, as a unit
/// vector, from its two declared angles: `tilt` from vertical, `heading` the
/// plan direction it leans toward (degrees CCW from +X). One home, read by
/// the envelope below, the stock model and the wireframe.
pub fn derive_tilted_axis(tilt_deg: f64, heading_deg: f64) -> [f64; 3] {
    let (st, ct) = tilt_deg.to_radians().psin_cos();
    let (sh, ch) = heading_deg.to_radians().psin_cos();
    [st * ch, st * sh, -ct]
}

/// A tilted bore's plan ENVELOPE — what the tool, which occupies the whole
/// depth the bore spans, can meet of it looking down the tool axis (Phase 41
/// slice 5; `DECISIONS.md` → "DRAFT: Phase 41 slice 5…", "what the footprint
/// is, geometrically"). The bore is a cylinder clipped to the slab between
/// the stock top and `extent` below it, so each end is a horizontal section:
/// an ellipse with semi-axis `r / cos(tilt)` along the heading and `r`
/// across it, the lower one displaced `extent · tan(tilt)` along the
/// heading. The envelope is the convex hull of the two — the two tangent
/// lines parallel to the heading, a radius either side of the axis's plan
/// line, joined by the far half of each ellipse — returned as one CCW loop
/// chorded at the shared chord tolerance (the ellipse sampler's own
/// conservative step), so every consumer that reads loops (the offsetter,
/// the nesting tests, the arc construction) reads it as it reads a
/// profile. The section at any ONE height is the wrong curve here: the
/// cylinder leans, and above the floor it overhangs what the floor section
/// reaches.
pub fn derive_tilted_bore_envelope(dia: f64, extent: f64, x: f64, y: f64, tilt_deg: f64, heading_deg: f64) -> Vec<Pt> {
    use std::f64::consts::PI;
    let r = dia / 2.0;
    let t = tilt_deg.to_radians();
    let (a, b) = (r / t.pcos(), r);
    let (sh, ch) = heading_deg.to_radians().psin_cos();
    // The lower section's centre: where the axis is `extent` down.
    let shift = extent * t.ptan();
    let (x1, y1) = (x + shift * ch, y + shift * sh);
    // Parametric point on the ellipse centred at (cx, cy): `phi` measured
    // from the heading direction, CCW.
    let at = |cx: f64, cy: f64, phi: f64| {
        let (u, v) = (a * phi.pcos(), b * phi.psin());
        Pt { x: cx + u * ch - v * sh, y: cy + u * sh + v * ch }
    };
    // Half an ellipse in `n` chords, the sagitta bounded by the slackest
    // curvature radius a²/b — the same conservative sizing
    // `ellipse_offset_pts` uses, through the one segment-count home.
    let n = circle_segments_for_ellipse(a, b);
    let mut out = Vec::with_capacity(2 * n + 2);
    // The lower ellipse's near half (phi from −90° through 0 to +90°: the
    // side the lean points to), then the upper ellipse's far half (+90°
    // through 180° to 270°). The two straight runs between them fall out of
    // the join: at ±90° both ellipses sit a radius off the axis's plan line
    // with their tangents along the heading.
    for k in 0..=n {
        out.push(at(x1, y1, -PI / 2.0 + PI * k as f64 / n as f64));
    }
    for k in 0..=n {
        out.push(at(x, y, PI / 2.0 + PI * k as f64 / n as f64));
    }
    out
}

/// A tilted bore's horizontal SECTION `depth` below the stock top (Phase 42
/// slice 1): the congruent ellipse the envelope's two ends are — semi-axis
/// `r / cos(tilt)` along the heading, `r` across it — centred where the axis
/// is `depth` down, `depth · tan(tilt)` along the heading from the mouth. One
/// CCW loop, chorded as the envelope's halves are. The envelope is the hull
/// of the sections at depths 0 and `extent`; this is the curve at any one
/// height between, which is what a slab of the void is.
pub fn derive_tilted_bore_section(dia: f64, x: f64, y: f64, tilt_deg: f64, heading_deg: f64, depth: f64) -> Vec<Pt> {
    use std::f64::consts::PI;
    let r = dia / 2.0;
    let t = tilt_deg.to_radians();
    let (a, b) = (r / t.pcos(), r);
    let (sh, ch) = heading_deg.to_radians().psin_cos();
    let shift = depth * t.ptan();
    let (cx, cy) = (x + shift * ch, y + shift * sh);
    let n = 2 * circle_segments_for_ellipse(a, b);
    (0..n)
        .map(|k| {
            let phi = 2.0 * PI * k as f64 / n as f64;
            let (u, v) = (a * phi.pcos(), b * phi.psin());
            Pt { x: cx + u * ch - v * sh, y: cy + u * sh + v * ch }
        })
        .collect()
}

/// The INTERSECTION of a tilted bore's sections at two depths (Phase 42
/// slice 1): where the bore is air at BOTH heights — and so at every height
/// between, by convexity (a point in two translates of a convex set lies in
/// every translate along the segment joining them). Closed form: in the
/// frame that scales the section to the unit circle the two sections are
/// unit circles `δ = shift / a` apart along the heading, and their lens is
/// two arcs, each the part of one circle inside the other — the first
/// circle within `acos(δ/2)` of the direction toward the second, and its
/// mirror on the second. Empty when the shift is two semi-axes or more: the
/// sections clear each other. CCW, chorded as the sections are; at equal
/// depths it is the section itself.
pub fn derive_tilted_bore_lens(dia: f64, x: f64, y: f64, tilt_deg: f64, heading_deg: f64, depth_lo: f64, depth_hi: f64) -> Vec<Pt> {
    use std::f64::consts::PI;
    let r = dia / 2.0;
    let t = tilt_deg.to_radians();
    let (a, b) = (r / t.pcos(), r);
    let (sh, ch) = heading_deg.to_radians().psin_cos();
    let (lo, hi) = (depth_lo.min(depth_hi), depth_lo.max(depth_hi));
    let shift0 = lo * t.ptan();
    let (cx, cy) = (x + shift0 * ch, y + shift0 * sh);
    // The second section's centre in the scaled frame: `delta` unit radii
    // along the heading.
    let delta = (hi - lo) * t.ptan() / a;
    if delta >= 2.0 {
        return Vec::new();
    }
    let theta = (delta / 2.0).pacos();
    let n = ((2.0 * circle_segments_for_ellipse(a, b) as f64) * (theta / PI)).ceil().max(2.0) as usize;
    let back = |u: f64, v: f64| Pt { x: cx + a * u * ch - b * v * sh, y: cy + a * u * sh + b * v * ch };
    let mut out = Vec::with_capacity(2 * n);
    // The first circle's arc toward the second, −theta through +theta…
    for k in 0..=n {
        let phi = -theta + 2.0 * theta * k as f64 / n as f64;
        out.push(back(phi.pcos(), phi.psin()));
    }
    // …then the second circle's arc back toward the first, from π − theta
    // through π + theta, its ends the two crossing points already pushed.
    for k in 1..n {
        let phi = PI - theta + 2.0 * theta * k as f64 / n as f64;
        out.push(back(delta + phi.pcos(), phi.psin()));
    }
    out
}

/// How many chords a full ellipse with these semi-axes takes at the shared
/// tolerance — the sagitta bounded by its slackest curvature radius `a²/b`,
/// through the one segment-count home, the envelope's own sizing.
pub(super) fn circle_segments_for_ellipse(a: f64, b: f64) -> usize {
    crate::ops::finish::circle_segments(a * a / b).div_ceil(2).max(2)
}

// A `Contour`'s top datum: its authored `z`, or the stock top when the field is
// absent (`CylinderBore`'s authored-top-down convention). One home so the default
// cannot drift between the two ops that consume a Contour.
// Twice the signed area (shoelace); sign is CCW-positive, magnitude unused.
// One home (used by validate()'s winding-vs-variant rule and by
// ops::contour's offset-side selection).
pub fn signed_area(points: &[Pt]) -> f64 {
    let n = points.len();
    let mut a = 0.0;
    for i in 0..n {
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        a += p1.x * p2.y - p2.x * p1.y;
    }
    a
}

/// Whether a closed loop crosses itself (Phase 50 slice 1, B2): the first
/// pair of NON-ADJACENT edges that properly cross, overlap along a line, or
/// meet at a point interior to one of them, as `(i, j)` — the indices of the
/// vertices each edge starts from. A pair sharing a vertex coordinate is
/// skipped (a repeated vertex is a different degeneracy, not a crossing).
/// O(n²), which a chorded circle of a few hundred points pays once at parse.
/// Before this law a bowtie reached the offsetting crate, whose spatial index
/// asserted `min_x <= max_x` and took the process down — a panic where a
/// named refusal belonged.
/// The other degenerate edge (Phase 50 slice 1, B2): two consecutive vertices
/// at the same point — the closing pair included — make a zero-length edge,
/// which the offsetting crate's spatial index asserts on. The audit's own
/// repro was this, not a crossing: a square with one vertex typed twice.
/// Returns the index of the first vertex of the pair.
pub fn loop_repeated_vertex(points: &[Pt]) -> Option<usize> {
    let n = points.len();
    (0..n).find(|&i| {
        let (a, b) = (points[i], points[(i + 1) % n]);
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
    })
}

pub fn loop_self_intersection(points: &[Pt]) -> Option<(usize, usize)> {
    let n = points.len();
    if n < 4 {
        return None;
    }
    let orient = |a: Pt, b: Pt, c: Pt| (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    let on_segment = |a: Pt, b: Pt, c: Pt| {
        c.x >= a.x.min(b.x) - 1e-9 && c.x <= a.x.max(b.x) + 1e-9 && c.y >= a.y.min(b.y) - 1e-9 && c.y <= a.y.max(b.y) + 1e-9
    };
    let same = |a: Pt, b: Pt| (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9;
    for i in 0..n {
        let (a, b) = (points[i], points[(i + 1) % n]);
        for j in (i + 2)..n {
            // The closing edge (j == n-1) is adjacent to edge 0.
            if i == 0 && j == n - 1 {
                continue;
            }
            let (c, d) = (points[j], points[(j + 1) % n]);
            if same(a, c) || same(a, d) || same(b, c) || same(b, d) {
                continue;
            }
            let o1 = orient(a, b, c);
            let o2 = orient(a, b, d);
            let o3 = orient(c, d, a);
            let o4 = orient(c, d, b);
            let proper = ((o1 > 1e-9 && o2 < -1e-9) || (o1 < -1e-9 && o2 > 1e-9))
                && ((o3 > 1e-9 && o4 < -1e-9) || (o3 < -1e-9 && o4 > 1e-9));
            let touch = (o1.abs() <= 1e-9 && on_segment(a, b, c))
                || (o2.abs() <= 1e-9 && on_segment(a, b, d))
                || (o3.abs() <= 1e-9 && on_segment(c, d, a))
                || (o4.abs() <= 1e-9 && on_segment(c, d, b));
            if proper || touch {
                return Some((i, j));
            }
        }
    }
    None
}

pub fn derive_contour_top(stock_top: f64, z: Option<f64>) -> f64 {
    z.unwrap_or(stock_top)
}

/// A declared edge round, resolved against the feature that owns it: the round
/// itself, and the circle in plan whose top edge it breaks
/// (`(radius, z_top, wall_radius, x, y, outside)`).
///
/// `outside` is where the MATERIAL is — true for a boss (material inside the
/// circle, tool outside), false for a bore. That one bit decides every sign
/// below it, which is why it is read off the feature VARIANT rather than
/// inferred from a winding or a diameter comparison: the row-2.4 law, applied
/// to the edge (DECISIONS.md → "the corner rounder gets an operation").
///
/// One home, because the op, the removal model and the deviation lint all need
/// exactly this and must not each re-derive it.
pub fn derive_edge_round(p: &Part, feature: &Feature) -> Option<EdgeRound> {
    match feature {
        Feature::CylinderBore { dia, x, y, z, edge_round, .. } => Some(EdgeRound {
            radius: (*edge_round)?,
            // A bore is authored top-down from its rim, so the rim IS the
            // edge's plane — the stock top unless `z` says otherwise.
            z_top: derive_contour_top(p.stock.z, *z),
            boss: false,
            edge: EdgeLoop::Circle { x: *x, y: *y, r: dia / 2.0 },
        }),
        Feature::CylinderBoss { dia, extent, x, y, z, edge_round, .. } => Some(EdgeRound {
            radius: (*edge_round)?,
            z_top: z + extent,
            boss: true,
            edge: EdgeLoop::Circle { x: *x, y: *y, r: dia / 2.0 },
        }),
        // The polyline pair (slice 2). Same three answers, off the same fields:
        // a pocket's top edge sits at its own datum (the stock top by default,
        // the bore's convention), a boss's at its datum too — `z` is the TOP of
        // a profile feature in both variants, so unlike the circular pair there
        // is no `z + extent` to do.
        Feature::ProfilePocket { points, z, edge_round, .. } => Some(EdgeRound {
            radius: (*edge_round)?,
            z_top: derive_contour_top(p.stock.z, *z),
            boss: false,
            edge: EdgeLoop::Polyline { points: points.clone() },
        }),
        Feature::ProfileBoss { points, z, edge_round, .. } => Some(EdgeRound {
            radius: (*edge_round)?,
            z_top: derive_contour_top(p.stock.z, *z),
            boss: true,
            edge: EdgeLoop::Polyline { points: points.clone() },
        }),
        _ => None,
    }
}

/// A round on a planar top edge, in the part's frame.
///
/// The three fields above `edge` are everything the placement needs, and none of
/// them knows what shape the edge is: the round is `radius` into the material
/// from the wall and `radius` below `z_top` **whatever the wall's plan curve**,
/// which is why the circular and polyline pairs share one derivation and differ
/// only in how a distance from the wall is turned into a coordinate.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeRound {
    pub radius: f64,
    pub z_top: f64,
    /// Material on the inside of the loop (a boss) rather than outside it (a
    /// bore or pocket). The one bit that drives every sign below it, read off
    /// the feature VARIANT rather than inferred.
    pub boss: bool,
    pub edge: EdgeLoop,
}

/// The plan curve whose top edge is being broken.
#[derive(Clone, Debug, PartialEq)]
pub enum EdgeLoop {
    Circle { x: f64, y: f64, r: f64 },
    Polyline { points: Vec<Pt> },
}

impl EdgeRound {
    /// The circle, when the edge is one. The circular arms position in polar
    /// coordinates about it; the polyline arms have no such centre and never
    /// call this.
    pub fn circle(&self) -> Option<(f64, f64, f64)> {
        match &self.edge {
            EdgeLoop::Circle { x, y, r } => Some((*x, *y, *r)),
            EdgeLoop::Polyline { .. } => None,
        }
    }

    /// The round's own centre of curvature, as `(radius from the axis, height)`:
    /// `radius` into the material from the wall, and `radius` below the top
    /// face. This is the point the tool's ground arc centre has to land on, and
    /// it is the whole placement derivation in one line.
    pub fn centre(&self, wall_standoff: f64, face_standoff: f64) -> (f64, f64) {
        let (_, _, wall_radius) = self.circle().expect("a polar centre needs a circular edge");
        let inward = if self.boss { -1.0 } else { 1.0 };
        (
            wall_radius + inward * (self.radius - wall_standoff),
            self.centre_z(face_standoff),
        )
    }

    /// The height of the round's centre of curvature, which is the one half of
    /// the placement that never depended on the edge's shape.
    pub fn centre_z(&self, face_standoff: f64) -> f64 {
        self.z_top - self.radius + face_standoff
    }

    /// How far the tool's AXIS rides off the wall, into the VOID: one effective
    /// radius, backed off by any standoff. Never the pilot. Which direction
    /// "into the void" points is the edge's business, not this number's — for a
    /// circle it is a signed radius (`axis_radius`), for a polyline it is the
    /// distance handed to the offsetter (`derive_edge_round_path`).
    pub fn axis_offset(&self, effective_radius: f64, wall_standoff: f64) -> f64 {
        effective_radius + wall_standoff
    }

    /// Where the tool's AXIS rides on a circular edge: one effective radius off
    /// the wall, backed off by any standoff — outward for a boss, inward for a
    /// bore.
    pub fn axis_radius(&self, effective_radius: f64, wall_standoff: f64) -> f64 {
        let (_, _, wall_radius) = self.circle().expect("a polar axis ring needs a circular edge");
        let outward = if self.boss { 1.0 } else { -1.0 };
        wall_radius + outward * self.axis_offset(effective_radius, wall_standoff)
    }
}

/// A declared chamfer, resolved against the feature that owns it — the
/// `EdgeRound` derivation with the round swapped for the one dimensioned leg
/// (plus the drawing's angle, when stated). Same one-home rule: the op, the
/// removal model, the deviation lint and the register check all read exactly
/// this and must not each re-derive it.
pub fn derive_chamfer(p: &Part, feature: &Feature) -> Option<EdgeChamfer> {
    match feature {
        Feature::CylinderBore { dia, x, y, z, chamfer, .. } => Some(EdgeChamfer {
            spec: (*chamfer)?,
            z_top: derive_contour_top(p.stock.z, *z),
            boss: false,
            edge: EdgeLoop::Circle { x: *x, y: *y, r: dia / 2.0 },
            marks: Vec::new(),
        }),
        Feature::CylinderBoss { dia, extent, x, y, z, chamfer, .. } => Some(EdgeChamfer {
            spec: (*chamfer)?,
            z_top: z + extent,
            boss: true,
            edge: EdgeLoop::Circle { x: *x, y: *y, r: dia / 2.0 },
            marks: Vec::new(),
        }),
        Feature::ProfilePocket { points, z, chamfer, chamfer_marks, .. } => Some(EdgeChamfer {
            spec: (*chamfer)?,
            z_top: derive_contour_top(p.stock.z, *z),
            boss: false,
            edge: EdgeLoop::Polyline { points: points.clone() },
            marks: chamfer_marks.clone(),
        }),
        Feature::ProfileBoss { points, z, chamfer, chamfer_marks, .. } => Some(EdgeChamfer {
            spec: (*chamfer)?,
            z_top: derive_contour_top(p.stock.z, *z),
            boss: true,
            edge: EdgeLoop::Polyline { points: points.clone() },
            marks: chamfer_marks.clone(),
        }),
        _ => None,
    }
}

/// The leg each edge of a polyline chamfer is cut at, edge `i` running from
/// point `i` to the next (ROADMAP item 1 slice 9): the feature's own leg
/// from the first point on, restated wherever a point says so, `None` on
/// the edges a point left unbroken. Empty for a circle.
pub fn derive_chamfer_legs(ec: &EdgeChamfer) -> Vec<Option<ChamferLeg>> {
    let EdgeLoop::Polyline { points } = &ec.edge else { return Vec::new() };
    let mut legs = vec![Some(ec.spec.leg); points.len()];
    let mut marks = ec.marks.clone();
    marks.sort_by_key(|m| m.at);
    for (k, m) in marks.iter().enumerate() {
        let end = marks.get(k + 1).map_or(points.len(), |next| next.at);
        for leg in legs.iter_mut().take(end).skip(m.at) {
            *leg = m.leg;
        }
    }
    legs
}

/// One stretch of a polyline chamfer cut at one leg: the axis path it rides
/// (an open chain, arcs kept, `start` its first vertex's index on the loop's
/// path) and the authored points under it.
#[derive(Clone, Debug, PartialEq)]
pub struct ChamferRun {
    pub leg: Option<ChamferLeg>,
    pub start: usize,
    pub path: Vec<crate::offset::OffsetVertex>,
    pub edge: Vec<Pt>,
}

/// Split a loop's axis path into runs where the leg changes. Empty when no
/// point restates the leg: the loop is one closed pass and the emitter keeps
/// its closed form, so a program without marks reads as it always did. The
/// offsetter keeps no vertex correspondence (a corner becomes an arc or a
/// mitre), so each boundary point is matched to the path vertex nearest it,
/// the lowest-indexed on a tie, and the depth changes there: a deeper run
/// then reaches about a reach's width round the corner into its shallower
/// neighbour. That is the approximation this takes, and the program says
/// so in its comment.
pub fn derive_chamfer_runs(ec: &EdgeChamfer, path: &[crate::offset::OffsetVertex]) -> Vec<ChamferRun> {
    let EdgeLoop::Polyline { points } = &ec.edge else { return Vec::new() };
    if ec.marks.is_empty() || points.is_empty() || path.is_empty() {
        return Vec::new();
    }
    let legs = derive_chamfer_legs(ec);
    let n = points.len();
    let mut bounds: Vec<usize> = (0..n).filter(|&i| legs[i] != legs[(i + n - 1) % n]).collect();
    if bounds.is_empty() {
        bounds.push(0); // every edge restated to one leg: one run, the whole loop
    }
    // Start on a cut stretch when there is one, so the lead-in serves a
    // cut and the pass does not open with a lift.
    if let Some(k) = bounds.iter().position(|&b| legs[b].is_some()) {
        bounds.rotate_left(k);
    }
    let one_run = bounds.len() == 1;
    let nearest = |q: Pt| -> usize {
        // Tied vertices: the first from the path's start.
        crate::tie::first_least(path.iter().enumerate().map(|(k, v)| (k, (v.x - q.x).phypot(v.y - q.y))), |c| c.1).map_or(0, |(k, _)| k)
    };
    // The indices from `from` to `to` round a ring of `len`, inclusive; a
    // single run goes all the way round and closes on its own start.
    let walk = |from: usize, to: usize, len: usize| -> Vec<usize> {
        let mut idx = vec![from];
        let mut i = from;
        while i != to || (one_run && idx.len() == 1) {
            i = (i + 1) % len;
            idx.push(i);
        }
        idx
    };
    bounds
        .iter()
        .enumerate()
        .map(|(k, &b)| {
            let e = bounds[(k + 1) % bounds.len()];
            let (pb, pe) = (nearest(points[b]), nearest(points[e]));
            let mut chain: Vec<crate::offset::OffsetVertex> = walk(pb, pe, path.len()).into_iter().map(|i| path[i]).collect();
            if let Some(last) = chain.last_mut() {
                last.bulge = 0.0;
            }
            let edge: Vec<Pt> = walk(b, e, n).into_iter().map(|i| points[i]).collect();
            ChamferRun { leg: legs[b], start: pb, path: chain, edge }
        })
        .collect()
}

/// The mouth cone's plunge Z, and the one home for the depth law
/// (DECISIONS.md → "spot drilling and countersinking…"): the declared seat
/// diameter must land at the part's top surface, so the tip goes to the top
/// minus the height above the tip where the ground cone reaches the seat's
/// radius. `height_at_radius` carries the apex the relieved flat hides below
/// the tool, which is the whole point — the shop-floor derivation from the
/// catalogue's sharp point (`seat_r/tan` measured from the tip) is
/// structurally unwritable here, because no sharp-point cone exists to read.
pub fn derive_csink_tip_z(stock_z: f64, seat_dia: f64, cone: &GroundCone) -> f64 {
    stock_z - cone.height_at_radius(seat_dia / 2.0)
}

/// A chamfer on a planar top edge, in the part's frame — `EdgeRound`'s shape
/// with the round swapped for the declared `ChamferSpec`. The edge, the top
/// plane, and the material side answer identically; only what is stamped into
/// the corner differs.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeChamfer {
    pub spec: ChamferSpec,
    pub z_top: f64,
    pub boss: bool,
    pub edge: EdgeLoop,
    /// The points' restatements of the leg (slice 9); empty for a circle and
    /// for a loop cut at one leg all round.
    pub marks: Vec<ChamferMark>,
}

impl EdgeChamfer {
    /// The circle, when the edge is one — `EdgeRound::circle`, verbatim.
    pub fn circle(&self) -> Option<(f64, f64, f64)> {
        match &self.edge {
            EdgeLoop::Circle { x, y, r } => Some((*x, *y, *r)),
            EdgeLoop::Polyline { .. } => None,
        }
    }

    /// Where the tool's AXIS rides on a circular edge — outward for a boss,
    /// inward for a bore, the `EdgeRound` sign convention with the reach
    /// resolved by the placement rather than by the tool alone.
    pub fn axis_radius(&self, reach: f64) -> f64 {
        let (_, _, wall_radius) = self.circle().expect("a polar axis ring needs a circular edge");
        let outward = if self.boss { 1.0 } else { -1.0 };
        wall_radius + outward * reach
    }
}

/// **The resolved placement** — the cone's degree of freedom, answered, and the
/// one home every consumer reads it from (DECISIONS.md → "the chamfer tool…",
/// option B as blessed).
///
/// The structure worth naming: `tip_clearance` slides the tool along its OWN
/// FLANK — rho and Z move together, the cut face is invariant, and only the D
/// register changes — while the standoffs MOVE THE FACE (each is a deviation
/// contributor). The two knob families are orthogonal, which is what makes the
/// freedom resolvable without touching the declared geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChamferPlacement {
    /// The leg down the wall, after normalizing whichever leg was declared.
    pub depth: f64,
    /// The leg across the face, `depth · tan(phi_half)`.
    pub width: f64,
    /// The flank radius at the face's bottom corner: `tip_r + tip_clearance`.
    /// Where the cutting starts, and the invariant the slide preserves.
    pub r_contact: f64,
    /// The axis's offset from the AUTHORED wall, into the void:
    /// `r_contact + wall_standoff`. What D/2 must equal under control comp.
    pub reach: f64,
    /// The emitted Z — the physical tip (`Silhouette::drop()` is zero), solved
    /// so the flank passes through the face's bottom corner.
    pub z_tip: f64,
}

/// Resolve the placement. The face's bottom corner sits `depth` below the
/// (face-standoff-lifted) top plane, `wall_standoff` off the authored wall into
/// the void; the flank must pass through it at radius `r_contact`, which pins
/// the tip Z through the apex the relieved flat hides below the tool.
pub fn derive_chamfer_placement(
    ec: &EdgeChamfer,
    cone: &GroundCone,
    tip_clearance: f64,
    wall_standoff: f64,
    face_standoff: f64,
) -> ChamferPlacement {
    derive_leg_placement(ec.spec.leg, ec.z_top, cone, tip_clearance, wall_standoff, face_standoff)
}

/// The placement for one leg at one top height: what a run that restates
/// the leg reads (slice 9). The loop's own placement is this at the
/// feature's leg; only `depth`, `width` and `z_tip` move with the leg, the
/// reach is the tool's and the same for every run.
pub fn derive_leg_placement(
    leg: ChamferLeg,
    z_top: f64,
    cone: &GroundCone,
    tip_clearance: f64,
    wall_standoff: f64,
    face_standoff: f64,
) -> ChamferPlacement {
    let t = cone.phi_half.ptan();
    let depth = match leg {
        ChamferLeg::Depth(d) => d,
        ChamferLeg::Width(w) => w / t,
    };
    let width = depth * t;
    let r_contact = cone.tip_r + tip_clearance;
    ChamferPlacement {
        depth,
        width,
        r_contact,
        reach: r_contact + wall_standoff,
        z_tip: z_top + face_standoff - depth - cone.height_at_radius(r_contact),
    }
}

/// The tip clearance a chamfer pass runs at: the authored value, or the
/// derived default `max(0.5mm, tip_dia/2)` — non-zero in every limit (zero IS
/// the rejected deepest-legal placement), growing with the very flat whose
/// mis-measurement it exists to guard, and floored at the machinist's
/// half-millimetre instinct. One home; the emitter, the header, the register
/// check and the removal model all resolve it here.
pub fn derive_tip_clearance(op: &Operation, ch: &Chamfer) -> f64 {
    op.tip_clearance.unwrap_or_else(|| derive_default_tip_clearance(ch))
}

/// The default alone, for consumers with no op in hand (the header's
/// no-control-op fallback) — one home for the number either way.
pub fn derive_default_tip_clearance(ch: &Chamfer) -> f64 {
    (ch.tip_dia / 2.0).max(0.5)
}

/// The D value a chamfer pass needs dialled in: twice the axis's offset from
/// the wall — equivalently, the cone's diameter at the contact height, which
/// is what the header prints so the operator can check the number against the
/// tool. **Not** the body OD and not the tip flat.
pub fn derive_chamfer_comp_dia(ch: &Chamfer, tip_clearance: f64, wall_standoff: f64) -> f64 {
    ch.tip_dia + 2.0 * (tip_clearance + wall_standoff)
}

/// **The axis path for a polyline edge round** — the offset loop the tool rides,
/// and the one home both the emitter and the removal model read it from.
///
/// A positive distance on the AUTHORED points is "left of travel", which under
/// the winding convention is the VOID side for both variants (a CCW pocket
/// shrinks, a CW boss grows) — the same claim `ops/profile.rs` pins for the wall
/// trace, reused rather than re-derived, and for the same reason: reversing an
/// offset result is not `.reverse()`, and getting the side wrong is a silent
/// gouge.
///
/// The error is where slice 1's plan-corner rule actually lives. The design
/// wrote it as "corners tighter than the tool" and slice 1 sharpened *which*
/// radius that reads — the EFFECTIVE one, because a rounder's body sits above
/// the top face in open air. Here it needs no restatement at all: the corner
/// law IS this offset succeeding, because the path the corner would collapse is
/// the path the op emits.
pub fn derive_edge_round_path(
    points: &[Pt],
    axis_offset: f64,
) -> Result<Vec<crate::offset::OffsetVertex>, String> {
    let loops = crate::offset::offset_closed_polyline_verts(points, axis_offset);
    if loops.len() != 1 {
        return Err(format!(
            "offsets to {} loop(s) at the effective radius ({:.3}mm)",
            loops.len(), axis_offset
        ));
    }
    let one = loops.into_iter().next().unwrap();
    if one.len() < 3 {
        return Err(format!(
            "collapses to a degenerate path at the effective radius ({:.3}mm)",
            axis_offset
        ));
    }
    Ok(one)
}

/// The axis path as plain points, arcs flattened within `CHORD_TOL_MM` — what a
/// consumer measuring a DISTANCE to the path needs (`stock.rs`'s removal band).
/// Dropping the bulges instead would put a chord where a fillet is and quietly
/// under-state the round at every rounded corner.
pub fn derive_edge_round_path_points(path: &[crate::offset::OffsetVertex]) -> Vec<Pt> {
    let n = path.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let from = path[i];
        let to = path[(i + 1) % n];
        out.push(Pt { x: from.x, y: from.y });
        if from.bulge.abs() <= 1e-9 {
            continue;
        }
        let (cx, cy) = crate::offset::arc_center(&from, &to);
        let (r, a0) = {
            let (dx, dy) = (from.x - cx, from.y - cy);
            ((dx * dx + dy * dy).sqrt(), dy.patan2(dx))
        };
        // bulge = tan(theta/4), signed by direction (positive is CCW).
        let sweep = 4.0 * from.bulge.patan();
        if r <= 0.0 {
            continue;
        }
        let step = 2.0 * (1.0 - (crate::CHORD_TOL_MM / r).min(1.0)).pacos();
        let n_seg = if step > 0.0 { (sweep.abs() / step).ceil().max(1.0) as usize } else { 1 };
        for k in 1..n_seg {
            let a = a0 + sweep * (k as f64 / n_seg as f64);
            out.push(Pt { x: cx + r * a.pcos(), y: cy + r * a.psin() });
        }
    }
    out
}
