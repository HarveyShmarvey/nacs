use crate::math::Portable as _;
// Findings: advisory results from engagement, chip-load pricing, bands, envelope and deviation checks, and `lint`.

use super::*;

// Shared constructor rule for the elliptic pair's rotated superset
// (DECISIONS.md "Rotated ellipse…"): the angle must name its orientation
// canonically. The ellipse repeats every 180°, so (0, 180) exclusive covers
// every distinct orientation exactly once — 0 IS the axis-aligned form
// (rejected pointing at omitting the field, the degenerate-pointer move) and
// anything outside is a duplicate name for an orientation the range already
// has.

// ---- structured findings (the agent surface, fork 4A) -----------------------
//
// One advisory finding, structured (DECISIONS.md "the agent surface — every
// verb is a flag first…", fork 4A; PRINCIPLES.md tenet 7). The six findings
// families return these instead of bare strings so a tool can group, sort,
// and re-offer them — while `Display` renders exactly the prose the family
// always printed, so stderr and every golden are byte-identical and nothing
// outside the type signatures can tell the difference. One fact, two
// renderings, one home: the message is built once, at the site that knows
// the numbers, and every voice (the CLI's `warning:` lines, the gate
// directives' errors, `--check=json`'s document) reads this struct.
//
// Std-only on purpose: the library stays JSON-free — `--check=json`'s
// rendering is the CLI's, hand-rolled over std in `main.rs` (DECISIONS.md
// "--check=json — the check result as data…"), and `serde_json` stays deferred to
// the serve loop that first CONSUMES JSON (the agent surface's slice 2).
// Deliberately
// NOT carried yet, per the blessed entry's honest pricing: severity (every
// finding here is advisory by construction — escalation is the gate
// directives' act, not a field), per-finding codes, and the error envelope
// over the ~227 `Err(String)` sites (structured at the MCP boundary in a
// later slice, never per-site).

/// Which findings family voiced a finding — one variant per family function,
/// so the family is data rather than a substring match over prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingFamily {
    /// `parse::engagement_findings` — the engagement linter's scalar floor,
    /// which `engagement gate` escalates at validate — and, since Phase 38
    /// slice 2, `engagement::slot_findings`, the walk's arm: an op that is
    /// mostly a slot, read from the moves after generation, which the gate
    /// does not escalate (fork 5A). One family, two arms, as `Envelope` has
    /// its ceiling and its travel.
    Engagement,
    /// `parse::deviation_findings` — the edge-round/chamfer deviation budget.
    Deviation,
    /// `stock::body_foul_warnings` — the analytic body-foul probe.
    BodyFoul,
    /// `stock::containment_findings` — a declared feature outside the stock.
    Containment,
    /// `stock::entry_findings` — the plunge taxonomy's category-3 entry flag.
    Entry,
    /// `fixture::fixture_findings` — the tool body against a declared keep-out.
    Fixture,
    /// `vox::body_foul_findings` — the foul lint's swept-field voice.
    FieldBodyFoul,
    /// `stock::reach_findings` — the assembly shorter than the depth it works
    /// in (K2 fork 4c). Kin to `Containment`: a claim about the declaration
    /// against the stock, never about the material.
    Reach,
    /// `stock::envelope_findings` — the program against the declared
    /// machine's own limits (Phase 15, DECISIONS.md → "Phase 15 opens: the
    /// envelope DRAFT…"): an emitted position past a travel span. Exact
    /// where it speaks — a declared span against an emitted coordinate plus
    /// a declared datum is one addition and one comparison — and silent
    /// only where the declaration is absent, which is the undeclared-means-
    /// today's-behaviour law and not a miss.
    Envelope,
    /// A consumer of the stock model saying it could NOT answer for an op,
    /// and why — `stock::move_declines` for the entry restriction,
    /// `stock::body_foul_warnings` for the analytic probe (Phase 9 lane B,
    /// `DECISIONS.md` → "containment declines by name…", fork 4A). Not a
    /// defect and never a gate: the family's law muzzles probes that miss,
    /// and a decline is a probe saying it cannot, out loud. It exists as a
    /// finding so `--check=json` and the MCP `check` verb carry it — an
    /// agent reading `would_emit: true, findings: []` must not be reading a
    /// document three checks never ran for.
    Decline,
    /// `vox::part_violations` — the swept field read against the DECLARED
    /// PART: material a feature says must survive, swept away anyway (Phase 9
    /// lane C, the part-violation entry's "show all"). About the feature
    /// violated, like `Containment`; advisory and biased to silence, like
    /// `FieldBodyFoul`, because a gouge shallower than the field's band is a
    /// coin flip and is not counted.
    PartViolation,
}

/// What a finding is about: the op it judges, or — for the containment lint,
/// which reads the declaration alone and never consults the run list — the
/// declared feature by itself.
#[derive(Debug, Clone, PartialEq)]
pub enum FindingSubject {
    /// An operation, by its 0-based index in the `run` order, plus the
    /// feature it works (denormalized on purpose: the finding must stay
    /// readable without the `Part` in hand).
    Op { index: usize, feature: String },
    /// A declared feature alone.
    Feature { name: String },
}

/// One advisory finding: the family that voiced it, the op/feature it names,
/// and today's exact prose. `Display` is that prose, verbatim.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub family: FindingFamily,
    pub subject: FindingSubject,
    pub message: String,
    /// The place the finding is ABOUT, typed — the material point a body
    /// met, the worst corner, the lowest gouge — so a consumer annotates it
    /// without reading the sentence back (DESIGN.md I7). `None` for the
    /// families that name none. Not on the `--check=json` wire: the
    /// document is the three fields above, byte for byte as before.
    pub place: Option<[f64; 3]>,
    /// The tool tip the finding names, typed — where the tool stood when a
    /// body was accused — and the tool. `None` for a family about the
    /// declaration rather than a body.
    pub at: Option<[f64; 3]>,
    pub tool: Option<usize>,
    /// For a `Decline`: the family whose check could not run here (ROADMAP
    /// item 2, slice 6), so the account can say which check it was. `None`
    /// on every other family.
    pub about: Option<FindingFamily>,
}

impl Finding {
    /// A finding about an op (five of the six families).
    pub fn op(family: FindingFamily, index: usize, feature: &str, message: String) -> Finding {
        Finding { family, subject: FindingSubject::Op { index, feature: feature.to_string() }, message, place: None, at: None, tool: None, about: None }
    }
    /// A finding about a declared feature alone (the containment lint).
    pub fn feature(family: FindingFamily, name: &str, message: String) -> Finding {
        Finding { family, subject: FindingSubject::Feature { name: name.to_string() }, message, place: None, at: None, tool: None, about: None }
    }
    /// A decline, said to stand in for `family`'s check; any other finding
    /// passes through untouched. The collector applies it to each probe's
    /// voice, so a probe never names a family it is not.
    pub fn declining(mut self, family: FindingFamily) -> Finding {
        if self.family == FindingFamily::Decline {
            self.about = Some(family);
        }
        self
    }
    /// The place this finding is about, as the producer knows it.
    pub fn with_place(mut self, place: [f64; 3]) -> Finding {
        self.place = Some(place);
        self
    }
    /// `with_place`, when the producer may or may not have one.
    pub fn with_place_opt(mut self, place: Option<[f64; 3]>) -> Finding {
        self.place = place;
        self
    }
    /// The tool tip and tool this finding names, as the producer knows them.
    pub fn with_tip(mut self, at: [f64; 3], tool: usize) -> Finding {
        self.at = Some(at);
        self.tool = Some(tool);
        self
    }
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

// The engagement linter's buildable floor (ROADMAP "Engagement-driven
// stock/feed coupling", step 2; DECISIONS.md "The engagement linter's
// floor…"). Scalar allowance-vs-diameter checks only — the closed-form
// arc-measure engagement over the analytic boundary inventory stays committed
// for when a boundary-level quantity is needed (adaptive clearing), and the
// rubbing threshold ("barely cuts") is honestly gated on a feeds engine that
// does not exist yet, so thin-but-positive skins stay silent.
//
// The cross-op pairing is DERIVED, never declared — the same rule that keeps
// an op's role derived from what it leaves: an allowance's consumer is the
// next wall-cutting op (Finish/Clear) on the same feature, because that pass
// is the one that inherits the skin. An allowance with no downstream consumer
// is silent: a semi-finished part is representable by design.
//
// Four findings, biased to silence like every advisory here:
//  - own-tool heavy skin (the forerunner, previously the whole linter):
//    allowance at or above the op's own tool radius;
//  - cusp over-spend (fork 1A of "the roughing allowance has two arms…"): a
//    STEPOVER pass whose derived cusp takes more than half the allowance, so
//    the ceiling is met mostly by cusp and the ridden skin is a fraction of
//    the declared one — scallop mode is exempt because it declares its cusp,
//    the same deliberate-authoring silence the spring pass gets;
//  - over-engagement: the skin handed downstream (allowance minus the
//    consumer's own allowance) at or above the CONSUMER tool's radius — the
//    tool that must actually clear it, which the forerunner could never see;
//  - under-engagement's unambiguous core: the consumer's own allowance
//    EXCEEDS what it inherits, so its target surface stands outside the
//    material and the pass removes nothing. Strictly-exceeds: an equal
//    allowance is a spring pass at the allowance (the shipped two-comp-modes
//    fixtures author exactly this), deliberate rubbing, not an error.
pub fn engagement_findings(p: &Part) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (i, op) in p.operations.iter().enumerate() {
        let Some(allowance) = op.stock_to_leave else { continue };
        if let Some(tool) = p.tools.get(&op.tool_num) {
            let r_tool = tool.dia / 2.0;
            // An allowance at or above the tool's own radius is a heavy
            // roughing skin, not a semi-finish allowance: no same-class tool
            // can clear it in a single sub-radius-engagement pass.
            if allowance >= r_tool {
                findings.push(Finding::op(FindingFamily::Engagement, i, &op.feature_name, format!(
                    "stock_to_leave ({:.3}mm) on '{}' is at or above tool T{}'s radius ({:.3}mm) — a heavy roughing skin, not a semi-finish allowance: no same-class tool can clear it in a single sub-radius-engagement pass",
                    allowance, op.feature_name, op.tool_num, r_tool
                )));
            }
        }
        // The cusp-spend floor (fork 1A of "the roughing allowance has two
        // arms…", threshold blessed at 50% by name). Stepover arm ONLY: a
        // scallop pass DECLARES its cusp, so a heavy declared spend is the
        // spring-pass case — deliberate, visible in the file, silent by the
        // same bias every advisory here carries. The stepover arm derives its
        // cusp invisibly, and past ~50% the pass is riding a hair off the part
        // and relying on cusps to meet its ceiling: a roughing pass in name
        // only, and nothing in the declaration said so until this line.
        if op.scallop.is_none() {
            if let Some(skin) = derive_emitted_skin(p, op) {
                let cusp = allowance - skin;
                if cusp > 0.5 * allowance {
                    findings.push(Finding::op(FindingFamily::Engagement, i, &op.feature_name, format!(
                        "stock_to_leave ({:.3}mm) on '{}' spends {:.0}% of its budget on the pass's own cusps — it rides {:.3}mm off the part and meets its ceiling with cusp, a roughing pass in name only; tighten stepover until the cusp is the minority share, or declare the split explicitly with scallop mode",
                        allowance, op.feature_name, 100.0 * cusp / allowance, skin
                    )));
                }
            }
        }
        let Some(consumer) = p.operations[i + 1..]
            .iter()
            .find(|b| b.feature_name == op.feature_name && b.cuts_walls())
        else { continue };
        let consumer_leaves = consumer.stock_to_leave.unwrap_or(0.0);
        let skin = allowance - consumer_leaves;
        if skin < 0.0 {
            findings.push(Finding::op(FindingFamily::Engagement, i, &op.feature_name, format!(
                "stock_to_leave ({:.3}mm) on '{}' is below the {:.3}mm its downstream pass (T{}) is itself told to leave — allowances must shrink along a feature's op chain, so that pass inherits nothing to cut",
                allowance, op.feature_name, consumer_leaves, consumer.tool_num
            )));
        } else if let Some(consumer_tool) = p.tools.get(&consumer.tool_num) {
            let r_consumer = consumer_tool.dia / 2.0;
            if skin >= r_consumer {
                findings.push(Finding::op(FindingFamily::Engagement, i, &op.feature_name, format!(
                    "stock_to_leave ({:.3}mm) on '{}' hands its downstream pass (T{}, radius {:.3}mm) a {:.3}mm skin — at or above the radius of the tool that must clear it, so no single sub-radius-engagement pass can; for single-pass cleanup target an allowance near 10% of that tool's diameter ({:.3}mm)",
                    allowance, op.feature_name, consumer.tool_num, r_consumer, skin,
                    consumer_tool.dia * 0.1
                )));
            }
        }
    }
    findings.extend(corner_engagement_findings(p));
    findings.extend(band_findings(p));
    findings
}

/// The measured arm (Phase 8, V1 slice 0 — `DECISIONS.md` → "V1, the engagement
/// measure…"). Every finding above reasons about an allowance in millimetres;
/// this one reasons about the quantity the item is actually named for, in
/// degrees, computed closed-form by `crate::engagement` rather than inferred
/// from a stepover.
///
/// **What it says that stepover cannot.** A stepover fixes the engagement on a
/// straight run and says nothing about a corner, where the tool wraps standing
/// material and the load spikes — the "you can over-engage with no way to stop
/// it" failure this item's 2026-07-19 constraint was written against.
///
/// **Why 90 degrees is the line, and why it is not a knob.** Half immersion is
/// where a real transition happens: below it the chip is thinned and the feed
/// wants raising (the factor is exactly `1/sin(theta_e)`, so it is 1 at 90 and
/// rises only below), above it the tool is more than half buried and thinning
/// stops helping at all. So the finding is not "the corner is worse than the
/// straight run" — which is true of nearly every corner and would be chatter —
/// but "the corner crosses a line the declared stepover stays under." Measured
/// against the shipped corpus before it was chosen: of the six pocket-clearing
/// ops in the tree, four hold their nominal exactly and two reach 93.8 degrees
/// against a 78.5 degree nominal, at the reflex corner their own fixture header
/// documents.
/// The engagement of one op, or `None` where it is not computable.
///
/// **One computation, two voices** — the floor's own discipline, and the reason
/// this is a function rather than two lookalike blocks: the corner advisory and
/// the chip-thinning feed must never be able to disagree about what the tool is
/// doing. If they read different numbers, one of them is lying to the author.
///
/// Today the one carrier is `Clear` on a `ProfilePocket`, per fork 6A — the
/// pocket is where engagement means what the author's own manual loop means.
/// Every other carrier answers `None`, which costs nothing and claims nothing:
/// a feed with no correction is the chip-load identity K3 shipped, unchanged.
pub(crate) fn op_engagement(p: &Part, op: &Operation) -> Option<crate::engagement::PocketEngagement> {
    if op.op_type != OpType::Clear {
        return None;
    }
    let Some(Feature::ProfilePocket { points, .. }) = p.features.get(&op.feature_name) else {
        return None;
    };
    let tool = p.tools.get(&op.tool_num)?;
    let stepover = op.stepover?;
    let ccw = crate::ops::clear::make_ccw(points);
    // Parallel passes are not the offset family, and pricing them as if they
    // were would print that family's numbers under this op's name. `None`
    // costs nothing and claims nothing; the comparison report prices them.
    if derive_pattern(p, op) != Some(Pattern::Offset) {
        return None;
    }
    // A pocket that is more than a closed outline is priced on the family it
    // actually cuts (Phase 26: around islands; Phase 27: open edges) — the
    // same `region::of` and `offset::shape_levels` the emitter walks, never
    // the plain pocket's spiral through a post or out into air.
    if let Some(region) = crate::region::of(p, op).filter(|reg| !reg.islands().is_empty() || reg.open_edges() > 0) {
        let mut islands: Vec<crate::offset::Island> = region.islands().into_iter().map(|(_, i)| i.clone()).collect();
        let open = region.open_edges() > 0;
        let wall = if open {
            let swath = region.family_wall(tool.dia)?;
            islands.extend(swath.enclosed_islands());
            swath.wall
        } else {
            region.outer.clone()
        };
        let blank = crate::region::blank_of(p);
        return crate::engagement::region_pocket_engagement(
            &wall,
            &islands,
            open.then_some(blank.as_slice()),
            tool.dia / 2.0,
            op.stock_to_leave.unwrap_or(0.0),
            stepover,
        );
    }
    crate::engagement::pocket_engagement(
        &ccw,
        tool.dia / 2.0,
        op.stock_to_leave.unwrap_or(0.0),
        stepover,
    )
}

pub(super) fn corner_engagement_findings(p: &Part) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (i, op) in p.operations.iter().enumerate() {
        let Some(pe) = op_engagement(p, op) else { continue };
        let Some(stepover) = op.stepover else { continue };
        // Both conditions carry weight: past half immersion, and past it
        // because of the geometry rather than because the stepover already
        // asked for it. A stepover that is itself at or beyond half immersion
        // is the `stepover >= dia` error's and the heavy-skin arm's business,
        // not this one's — two voices for one fact is how a linter gets ignored.
        if pe.worst.radians > std::f64::consts::FRAC_PI_2
            && pe.worst.radians > pe.nominal + 1e-9
            && pe.nominal <= std::f64::consts::FRAC_PI_2
        {
            // Three shapes of the same fact, told apart so the advice is
            // actionable (Phase 9 lane A, slice 1 — the primitive walks the
            // emitter's inside-out order now, and the first two became
            // visible with it): a full slot on a later pass, where the inner
            // loop never reached (a leg narrower than its offset) and this
            // pass cuts from solid; an early pass of the spiral meeting more
            // than a stepover, because the swath around a tiny inner loop is
            // round where this loop is not; and the corner the advisory was
            // first written for.
            let deg = pe.worst.radians.to_degrees();
            let full_slot = pe.worst.radians >= std::f64::consts::PI - 1e-9;
            let (why, what) = if full_slot {
                ("a full slot — the inner loop never reached here, so this pass cuts from solid", "slot")
            } else if pe.worst.pass <= 2 {
                ("the inner loops' swath is rounder than this loop, so the pass meets more than its stepover — the first turns of an inside-out spiral run heavy", "heavy early pass")
            } else {
                ("that corner carries the load", "corner")
            };
            findings.push(Finding::op(FindingFamily::Engagement, i, &op.feature_name, format!(
                "Clear on '{}' rides a {:.3}mm stepover — {:.1}° of cutter engagement on a straight run — but at ({:.3}, {:.3}) on pass {} the tool actually reaches {:.1}°, past the 90° half-immersion line the stepover alone stays under: {}, and above 90° chip thinning no longer thins the chip. Reduce stepover, or take the {} knowingly",
                op.feature_name, stepover, pe.nominal.to_degrees(),
                pe.worst.at.0, pe.worst.at.1, pe.worst.pass, deg, why, what
            )).with_place([pe.worst.at.0, pe.worst.at.1, p.stock.z]));
        }
    }
    findings
}

// ---- the MRR band (Phase 9, lane A) ---------------------------------------
// DECISIONS.md → "Phase 9 — the band, and the silences around it…". The
// engagement linter's horizon, as the roadmap entry named it in 2026-08-01:
// every feed move PRICED, a rubbing floor and a too-heavy ceiling. Measured
// before it was designed, it is a CHIP-THICKNESS band rather than a volume
// band, because the one rating a shipped datum carries is per tooth — the
// `chipload` in the feeds table, the `f_z` of K3's identity `F = RPM · z · f_z`:
//
//   h = (F / (rpm · z)) · sin(min(theta_e, 90°))
//
//   floor    h < f_z / CHIP_THINNING_CAP   the chip is thinner than half the
//                                          rating — the edge rubs more than
//                                          it cuts (V1's own budget, read
//                                          from the other side; one home)
//   ceiling  h > f_z                        the cut is heavier than the
//                                          tool/material pair is rated for
//
// MRR (a_e · a_p · F) is PRINTED as the move's price and not thresholded: no
// datum rates a volume, and a threshold with no datum behind it is a guess.
// The machine-power ceiling (spindle power × specific cutting energy) is the
// band's named follow-up, gated on two fields nobody declares yet.
//
// Every input is declared or derived closed-form, so the reading neither
// over- nor under-claims, and every silence below is an ABSENCE OF DATA —
// never a sampled miss. That is what makes this arm gate-eligible under the
// family's law ("a gate must not promise what a probe cannot") where the
// body-foul probe is not, and why it speaks through `engagement advise|gate`
// rather than a sixth directive.
//
// The silence roster, written down because a silence with no name is what
// Phase 9 exists to end: no `material=` on the stock (no rating); no feeds
// record or band for the pair (the lookup's own error is for `derive`; a
// typed feed with no rating is simply unpriced); no `flute_count` (no chip);
// a `derive` op not yet resolved, either word (the gate's first call runs
// inside `validate`, before the words exist — pricing the global feed or
// spindle there would be a WRONG number wearing a finding's clothes, so it
// waits for the second call); a Finish with no rough in front of it (no inherited skin); carriers
// with no engagement home yet (`CylinderBore` clears, revolve finishes, the
// field-trimmed strategies, `Floor`, ball and form tools).

/// One priced pass of an op: its engagement, the chip it takes, and that chip
/// against the rating.
#[derive(Clone, Debug, PartialEq)]
pub struct PricedPass {
    /// Which pass, in the op's own words ("pass 0, the slot cut from solid").
    pub label: String,
    /// The engagement it runs at, radians.
    pub theta: f64,
    /// The chip it takes, mm per tooth.
    pub chip: f64,
    /// `chip / f_z` — 1.000 is exactly the rating.
    pub ratio: f64,
    /// Its price: `a_e · a_p · F`, mm³/min.
    pub mrr: f64,
    /// The feed word this pass actually rides — its own, when the op carries
    /// one per pass; the op's otherwise.
    pub feed: f64,
}

/// The rated chip load for this tool on this job — the feeds table's `f_z`
/// for (material × tool type) at the tool's diameter — and which table said
/// so. `None` on any miss: this is the advisory's reading of a rating the
/// `derive` path would reject on, and an absent rating prices nothing.
pub(super) fn rated_chipload(p: &Part, tool: &Tool) -> Option<(f64, &'static str)> {
    let material = p.stock.material.as_deref()?;
    let shipped = parse_feeds(STARTER_FEEDS).ok()?;
    let shop = p.shop_feeds.as_ref();
    let from_shop = shop.and_then(|t| t.record(material, &tool.name)).is_some();
    let rec = feeds_lookup(shop, &shipped, material, &tool.name).ok()?;
    let band = rec.chiploads.iter().find(|b| b.dia_lo <= tool.dia && tool.dia < b.dia_hi)?;
    Some((band.mm_per_tooth, if from_shop { "the shop feeds table" } else { "tables/starter.feeds" }))
}

/// The chip a tooth takes at this engagement from this feed per tooth:
/// `f · sin(theta_e)` up to half immersion, the full `f` past it — the mirror
/// V1's clamp already rejects to read as a boost.
pub(super) fn chip_at(f_per_tooth: f64, theta: f64) -> f64 {
    f_per_tooth * theta.min(std::f64::consts::FRAC_PI_2).psin()
}

/// Radial engagement of a wall pass riding `a_e` into the wall with a tool of
/// radius `r`: the textbook `acos(1 - a_e / r)`, the same form `engagement::
/// pocket_engagement` calls `nominal`.
pub(super) fn wall_engagement(a_e: f64, r: f64) -> f64 {
    (1.0 - a_e / r).clamp(-1.0, 1.0).pacos()
}

/// Does this feature present a vertical wall that a flat endmill engages
/// radially — the geometry the chip-thickness identity is written for? The
/// revolves are ball-tool work whose engagement is an axial question the
/// scallop machinery owns; they are silent here by name, not by accident.
pub(super) fn has_vertical_wall(f: &Feature) -> bool {
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

/// The priced passes of one op, or `None` where the band has nothing honest
/// to say (the silence roster above). The feed per tooth is read from the
/// op's RESOLVED words — the derived number when `derive` rode along, the
/// declared/global one otherwise — exactly as `resolve_derived_words` reads
/// the spindle, because the chip is what the machine will actually take.
pub fn priced_passes(p: &Part, op_idx: usize) -> Option<(Vec<PricedPass>, f64, &'static str)> {
    let op = p.operations.get(op_idx)?;
    let tool = p.tools.get(&op.tool_num)?;
    if tool.name != "Endmill" {
        return None;
    }
    // An unresolved `derive` — EITHER word: the number does not exist yet.
    // Pricing the global feed or spindle in its place would be a wrong number
    // with a finding's voice (the first build of this priced a typed F10000
    // at the global 3000rpm because only the feed was checked — 27.8× where
    // the resolved answer is 17.5×); `parse_project_with_base` asks again
    // once the words resolve.
    if (op.feed_derive && op.feed.is_none()) || (op.spindle_derive && op.spindle.is_none()) {
        return None;
    }
    let (f_z, table) = rated_chipload(p, tool)?;
    let z = tool.flute_count?;
    let feed = op.feed.unwrap_or(p.global_feed);
    let rpm = derive_spindle_speed(op, p.global_spindle);
    if feed <= 0.0 || rpm <= 0.0 || z == 0 {
        return None;
    }
    let per_tooth = |f: f64| f / (rpm * z as f64);
    let f_per_tooth = per_tooth(feed);
    let r = tool.dia / 2.0;
    let feature = p.features.get(&op.feature_name)?;

    let mut passes = Vec::new();
    match op.op_type {
        OpType::Clear => {
            // A field-trimmed family keeps a subset of the loops the
            // primitive walks, so its per-pass readings name loops the
            // program may not run — silent by name (the roster).
            if op.strategy.is_some() {
                return None;
            }
            // The pocket carrier, per pass in EMISSION order, through the
            // same `op_engagement` the corner advisory and the chip-thinning
            // feed read — so the three cannot disagree about what the tool is
            // doing. Each pass is priced at its OWN feed word when the op
            // carries one per pass (slice 1), the op's word otherwise.
            let pe = op_engagement(p, op)?;
            let stepover = op.stepover?;
            let Feature::ProfilePocket { extent, .. } = feature else { return None };
            let a_p = op.stepdown.map_or(*extent, |s| s.min(*extent));
            for (k, e) in pe.passes.iter().enumerate() {
                let f_k = op.pass_feeds.as_ref().and_then(|v| v.get(k).copied()).unwrap_or(feed);
                let label = if k == 0 {
                    "pass 0, the slot cut from solid".to_string()
                } else {
                    format!("pass {}, worst at ({:.3}, {:.3})", k, e.at.0, e.at.1)
                };
                let a_e = if e.radians >= std::f64::consts::FRAC_PI_2 { tool.dia } else { stepover };
                passes.push(PricedPass {
                    label,
                    theta: e.radians,
                    chip: chip_at(per_tooth(f_k), e.radians),
                    ratio: chip_at(per_tooth(f_k), e.radians) / f_z,
                    mrr: a_e * a_p * f_k,
                    feed: f_k,
                });
            }
        }
        OpType::Side | OpType::Copy | OpType::Form => {
            // The wall carrier: a finish paired behind its own rough inherits
            // a skin, and a flat endmill riding a vertical wall engages it
            // radially — the linter's derived pairing, run from the
            // consumer's side, one home in `stock`.
            if !has_vertical_wall(feature) {
                return None;
            }
            let skin = crate::stock::inherited_skin(p, op_idx);
            if skin <= 0.0 || skin >= r {
                // Unpaired, or the over-engagement finding's business.
                return None;
            }
            let theta = wall_engagement(skin, r);
            let a_p = op.stepover.unwrap_or(feature_z_span(feature));
            passes.push(PricedPass {
                label: format!("the wall pass, {:.3}mm of inherited skin", skin),
                theta,
                chip: chip_at(f_per_tooth, theta),
                ratio: chip_at(f_per_tooth, theta) / f_z,
                mrr: skin * a_p * feed,
                feed,
            });
        }
        _ => return None,
    }
    Some((passes, f_z, table))
}

/// The Z span a wall pass runs over when the op states no axial stepdown.
pub(super) fn feature_z_span(f: &Feature) -> f64 {
    match f {
        Feature::CylinderBore { extent, .. }
        | Feature::CylinderBoss { extent, .. }
        | Feature::ProfilePocket { extent, .. }
        | Feature::ProfileBoss { extent, .. }
        | Feature::ContourPocket { extent, .. }
        | Feature::ContourBoss { extent, .. } => *extent,
        _ => 0.0,
    }
}

/// Ratios within this of 1.000 (or of the floor) are AT the rating, not over
/// it: two exactly-computed reals being compared, not a geometric tolerance.
pub(super) const BAND_EPS: f64 = 1e-6;

/// The band's findings: one ceiling and/or one floor finding per op, each
/// naming the pass that sets it, the chip against the rating, and the price.
pub fn band_findings(p: &Part) -> Vec<Finding> {
    let mut findings = Vec::new();
    let floor = 1.0 / crate::engagement::CHIP_THINNING_CAP;
    for (i, op) in p.operations.iter().enumerate() {
        let Some((passes, f_z, table)) = priced_passes(p, i) else { continue };
        let tool = &p.tools[&op.tool_num];
        let rpm = derive_spindle_speed(op, p.global_spindle);
        let z = tool.flute_count.unwrap_or(0);
        let verb = op.op_type.verb();
        let material = p.stock.material.as_deref().unwrap_or("?");
        let Some(heaviest) = passes.iter().max_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap()) else { continue };
        if heaviest.ratio > 1.0 + BAND_EPS {
            let rest: Vec<String> = passes
                .iter()
                .filter(|q| !std::ptr::eq(*q, heaviest))
                .map(|q| format!("{} at {:.1}° takes {:.3}×", q.label, q.theta.to_degrees(), q.ratio))
                .collect();
            findings.push(Finding::op(FindingFamily::Engagement, i, &op.feature_name, format!(
                "{} on '{}' with T{} feeds F{:.3} at {:.0}rpm × {} flutes = {:.3}mm/tooth; on {} ({:.1}° of engagement) the chip is {:.3}mm — {:.3}× the {:.3}mm/tooth {} rates {}×{} at dia {:.3}, {:.0}mm³/min{}. Feed that pass at the rated chip, or take the heavier cut knowingly",
                verb, op.feature_name, op.tool_num, heaviest.feed, rpm, z, heaviest.feed / (rpm * z as f64),
                heaviest.label, heaviest.theta.to_degrees(), heaviest.chip, heaviest.ratio,
                f_z, table, material, tool.name, tool.dia, heaviest.mrr,
                if rest.is_empty() { String::new() } else { format!("; {}", rest.join("; ")) }
            )));
        }
        let Some(lightest) = passes.iter().min_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap()) else { continue };
        if lightest.ratio < floor - BAND_EPS {
            findings.push(Finding::op(FindingFamily::Engagement, i, &op.feature_name, format!(
                "{} on '{}' with T{} feeds F{:.3} at {:.0}rpm × {} flutes = {:.3}mm/tooth, but on {} ({:.1}° of engagement) the chip is only {:.3}mm — {:.3}× the {:.3}mm/tooth {} rates {}×{} at dia {:.3}, below the 1/{:.1} rubbing floor (CHIP_THINNING_CAP read from the other side), {:.0}mm³/min: the edge rubs more than it cuts. Leave a thicker skin, feed faster, or declare the spring pass with an equal allowance",
                verb, op.feature_name, op.tool_num, lightest.feed, rpm, z, lightest.feed / (rpm * z as f64),
                lightest.label, lightest.theta.to_degrees(), lightest.chip, lightest.ratio,
                f_z, table, material, tool.name, tool.dia,
                crate::engagement::CHIP_THINNING_CAP, lightest.mrr
            )));
        }
    }
    findings
}

// Advisory lints: conditions that are legal but worth a voice. Error
// (validate / the runtime gouge guards) is the only voice the emitted program
// has; lints are the advisory sibling, printed by the CLI to stderr so stdout
// — and therefore every golden — stays byte-identical. Two members: the
// engagement findings (which `engagement gate` escalates to an error — this
// advisory voice is then unreachable, the same findings speak from validate)
// and the body-foul probe, which stays advisory-only in every mode because
// its sampled lattice is biased to silence and can miss — a gate must not
// promise what a probe cannot.
/// What a tool's own declaration says about itself, by tool number: a
/// slip the parser accepts and an operation would trip on later. Not a
/// finding: it is about no operation and no feature.
pub fn tool_notes(p: &Part) -> Vec<(usize, String)> {
    let mut nums: Vec<usize> = p.tools.keys().copied().collect();
    nums.sort_unstable();
    nums.into_iter().filter_map(|n| p.tools[&n].chamfer.as_ref().and_then(|c| c.note()).map(|note| (n, format!("T{n}: {note}")))).collect()
}

pub fn lint(p: &Part) -> Vec<Finding> {
    let mut warnings = engagement_findings(p);
    warnings.extend(deviation_findings(p));
    // Tool bodies reaching into material the op leaves behind. Lives in
    // `stock` because it reads the op-history stock model rather than the op's
    // own fields, but it is voiced from here: one home for advisories.
    warnings.extend(crate::stock::body_foul_warnings(p).into_iter().map(|f| f.declining(FindingFamily::BodyFoul)));
    // A declared feature reaching outside the stock. Also voiced from here for
    // the same reason, though it reads neither the stock model nor the ops —
    // only the declaration against itself.
    warnings.extend(crate::stock::containment_findings(p));
    // The spindle ceiling (Phase 15 slice 2): every op's resolved RPM
    // against the machine's declared max. Parse-time by nature — the number
    // is the declaration's, resolved before any move exists — unlike the
    // travel arm, which reads emitted moves and is voiced post-generation.
    warnings.extend(envelope_ceiling_findings(p));
    // The changer (Phase 15 slice 3): the program's distinct tools against
    // the declared pockets. Also parse-time — a count reads the run list,
    // never a move.
    warnings.extend(envelope_changer_findings(p));
    // The certificate's two advisory voices (Phase 48 slice 2): a decline
    // per given void when the earlier program's model is incomplete, and an
    // earlier removal reaching this face that no given names.
    warnings.extend(crate::stock::given_from_findings(p).into_iter().map(|f| f.declining(FindingFamily::Entry)));
    warnings
}

/// The changer half of the machine envelope (Phase 15 slice 3, DECISIONS.md
/// → "Phase 15 opens: the envelope DRAFT…", fork 3): the count of DISTINCT
/// tools the run list asks for, against the machine file's `atc_pockets`.
/// A count against a count — exact both ways, the family's cheapest probe.
/// The subject is each op whose tool is new past the declared count, so the
/// finding lands where an author would edit. Max tool diameter/length is
/// deliberately not asked here: gauging an assembly is
/// `--assembly-report`'s owned question, one home.
pub fn envelope_changer_findings(p: &Part) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(pockets) = p.machine.as_ref().and_then(|m| m.atc_pockets) else { return findings };
    let mut seen: Vec<usize> = Vec::new();
    for (i, op) in p.operations.iter().enumerate() {
        if seen.contains(&op.tool_num) {
            continue;
        }
        seen.push(op.tool_num);
        if seen.len() <= pockets {
            continue;
        }
        findings.push(Finding::op(
            FindingFamily::Envelope,
            i,
            &op.feature_name,
            format!(
                "T{} brings this program's distinct-tool count to {}, first asked for at op [{}] — the declared changer holds {} pocket{}. A pocket count is a count of TOOLS, not of runs: re-using a tool is free, a new number is not. Re-tool an op onto a cutter already in the carousel, split the program at a tool break, or correct atc_pockets if the sheet was misread. (Whether each assembly FITS its pocket stays `--assembly-report`'s gauging question, one home.)",
                op.tool_num, seen.len(), i, pockets, if pockets == 1 { "" } else { "s" }
            ),
        ));
    }
    findings
}

/// The ceiling half of the machine envelope (Phase 15 slice 2, DECISIONS.md
/// → "Phase 15 opens: the envelope DRAFT…", fork 2): each op's RESOLVED
/// spindle speed — the same number the emitter prints as S, through the one
/// `derive_spindle_speed` home — against the machine file's `spindle_max`.
/// Exact both ways: a declared number against a declared ceiling is one
/// comparison, and an undeclared ceiling is silence, not a miss.
///
/// **It advises and never clamps.** A silently lowered S would change the
/// chip load every derived feed was priced for; the clamp is the
/// engagement/feeds coupling's move (T3) on the day it lands, made against
/// a ceiling this key will already carry.
pub fn envelope_ceiling_findings(p: &Part) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(max) = p.machine.as_ref().and_then(|m| m.spindle_max) else { return findings };
    for (i, op) in p.operations.iter().enumerate() {
        let rpm = derive_spindle_speed(op, p.global_spindle);
        if rpm <= max {
            continue;
        }
        let source = if op.spindle_derive {
            "the feeds table derived"
        } else {
            "the program declares"
        };
        findings.push(Finding::op(
            FindingFamily::Envelope,
            i,
            &op.feature_name,
            format!(
                "T{} asks the spindle for {:.0} RPM ({} it) on op [{}] — past the declared spindle_max {:.0} by {:.0}. The ceiling never clamps: a silently lowered S would change the chip load every feed number was priced for. Lower the op's spindle= (or the program's), pick a pairing the feeds table rates slower, or correct the ceiling if the sheet was misread",
                op.tool_num, rpm, source, i, max, rpm - max
            ),
        ));
    }
    findings
}

// The prismatic ops emit a flat floor and a full-height vertical wall from a
// single Z, and both claims are false for a round-bottomed tool: a ball leaves
// scallops between adjacent floor passes, and on the WALL it only cuts full
// width at z_cut + r_ball, leaving a fillet of up to r_ball above the floor.
// The second is the larger error and the one that matters for an allowance —
// it breaks the ceiling on the very surface the allowance is about.
//
// Always-on, not gated on stock_to_leave: `ops::pocket`'s own comment asserts
// "a flat bottom", which a ballmill makes false with or without an allowance
// (DECISIONS.md "stock_to_leave is a radial allowance…"). The `name` compare
// is the same one the Finish arm uses for its Ballmill requirement; it
// inherits that convention's weakness (a free-form field, so "BallMill" slips
// through) rather than inventing a second convention beside it.
pub(super) fn require_flat_bottom_tool(op_name: &str, tool: &Tool) -> Result<(), String> {
    // A corner rounder carries no `profile` — its body is derived, not drawn —
    // so it would otherwise walk straight through this rule looking like an
    // endmill, and cut a bore with a relieved pilot (DECISIONS.md → "the corner
    // rounder gets an operation").
    if let Some(rd) = &tool.rounder {
        return Err(format!(
            "{} operation requires a flat-bottomed tool, but T{} is a CornerRounder: below its arc it is a RELIEVED pilot ({:.3}mm), a clearance surface ground undersize so it does not touch — it has no flat floor to cut with and no full-width wall",
            op_name, tool.num, rd.pilot
        ));
    }
    // Same trap, one form over: a chamfer mill carries no `profile` either.
    if let Some(ch) = &tool.chamfer {
        return Err(format!(
            "{} operation requires a flat-bottomed tool, but T{} is a ChamferMill: its tip is a RELIEVED flat ({:.3}mm), a clearance surface that does not cut — it has no flat floor to cut with and its flank cuts at an angle, not full-width",
            op_name, tool.num, ch.tip_dia
        ));
    }
    if tool.profile.is_some() {
        return Err(format!(
            "{} operation requires a flat-bottomed tool, but T{} carries a custom profile: the emitted path assumes a flat floor and a full-height wall from one Z, which a shaped tool does not cut",
            op_name, tool.num
        ));
    }
    if tool.name == "Ballmill" {
        return Err(format!(
            "{} operation requires a flat-bottomed tool, but T{} is a Ballmill: a ball leaves scallops between adjacent floor passes, and on the wall it cuts full width only at r ({:.3}mm) above the floor, leaving a fillet the single-Z path never removes — use an Endmill",
            op_name, tool.num, tool.dia / 2.0
        ));
    }
    Ok(())
}

/// **The deviation budget** — what the finished blend will actually depart from
/// the round that was declared, by contributor, with the input behind each.
///
/// This is the gap the whole design exists to fill (DECISIONS.md → "the corner
/// rounder gets an operation"). Traditional CAM assumes perfect tangency, takes
/// the catalogue pilot as the comp diameter, and lets the resulting step be
/// something found with a fingernail. Here the three contributors are separate
/// numbers, never summed into an opaque total, because a step you can attribute
/// is a step you can fix:
///
///   * **wall step** — `R(1 - cos trail_wall)`, plus any wall standoff, on the
///     flank;
///   * **face step** — `R(1 - cos trail_face)`, plus any face standoff, on the
///     top;
///   * **radius error** — the arc ground off nominal, which RESIZES the blend
///     rather than stepping it;
///   * **corner shortfall** — the polyline pair's own contributor (slice 2), and
///     the one a circle structurally cannot have. Where a plan corner is tighter
///     than the tool, the axis path pulls away from the wall and the round is
///     simply not cut there. Traditional CAM leaves that to be found with a
///     fingernail on the part; here it is `dist(corner, path) - the offset the
///     axis rides`, in millimetres, per corner.
///
/// A catalogue tool with no trail-off and no standoff is silent, which is the
/// honest answer: nothing was declared that would move the blend.
pub fn deviation_findings(p: &Part) -> Vec<Finding> {
    let mut out = Vec::new();
    for (k, op) in p.operations.iter().enumerate() {
        if !op.follows_surface() {
            continue;
        }
        let Some(tool) = p.tools.get(&op.tool_num) else { continue };
        let Some(feature) = p.features.get(&op.feature_name) else { continue };

        // The chamfer's contributors, beside the round's: the standoffs (each
        // moves the face), the angle error (which TILTS the face — the
        // placement pins the wall corner, so the tilt accumulates to the face
        // corner as depth · |tan actual − tan nominal|), and the corner
        // shortfall the polyline pair inherits. The tip flat is deliberately
        // NOT a contributor: with a positive clearance the flat touches
        // nothing, so its provenance lives in the comp header and its failure
        // modes in the errors, not in an invented deviation number.
        if let Some(ch) = &tool.chamfer {
            let Some(ec) = derive_chamfer(p, feature) else { continue };
            let cone = tool.ground_cone().expect("a ChamferMill has a cone");
            let pl = derive_chamfer_placement(&ec, &cone, derive_tip_clearance(op, ch), op.wall_standoff, op.face_standoff);

            let tilt = if ch.angle_error != 0.0 {
                let nominal = (ch.angle / 2.0).to_radians().ptan();
                Some(pl.depth * (cone.phi_half.ptan() - nominal).abs())
            } else {
                None
            };
            let corners = chamfer_corner_finding(&ec, &pl);
            if op.wall_standoff <= 0.0 && op.face_standoff <= 0.0 && tilt.is_none() && corners.is_none() {
                continue;
            }

            let mut parts: Vec<String> = Vec::new();
            if op.wall_standoff > 0.0 {
                parts.push(format!("wall shift {:.4}mm (standoff)", op.wall_standoff));
            }
            if op.face_standoff > 0.0 {
                parts.push(format!("face shift {:.4}mm (standoff)", op.face_standoff));
            }
            if let Some(t) = tilt {
                parts.push(format!(
                    "face tilt {:.4}mm across the {:.3}mm leg (angle_error {:+.3}deg off the declared {:.3}deg)",
                    t, pl.depth, ch.angle_error, ch.angle
                ));
            }
            let place = corners.as_ref().map(|(_, at)| [at[0], at[1], p.stock.z]);
            if let Some((c, _)) = corners {
                parts.push(c);
            }
            out.push(Finding::op(FindingFamily::Deviation, k, &op.feature_name, format!(
                "op [{}] (chamfer on '{}', T{}{}): {}",
                k,
                op.feature_name,
                op.tool_num,
                if ch.measured { ", measured" } else { ", catalog nominal" },
                parts.join("; ")
            )).with_place_opt(place));
            continue;
        }

        let Some(rd) = &tool.rounder else { continue };
        let Some(er) = derive_edge_round(p, feature) else { continue };

        let wall = rd.wall_step() + op.wall_standoff;
        let face = rd.face_step() + op.face_standoff;
        let dr = rd.radius_error;
        let corners = edge_round_corner_finding(&er, tool, op.wall_standoff);
        if wall <= 0.0 && face <= 0.0 && dr == 0.0 && corners.is_none() {
            continue;
        }

        let mut parts: Vec<String> = Vec::new();
        if wall > 0.0 {
            parts.push(format!(
                "wall step {:.4}mm (trail_wall {:.2}deg -> {:.4}, standoff {:.4})",
                wall, rd.trail_wall, rd.wall_step(), op.wall_standoff
            ));
        }
        if face > 0.0 {
            parts.push(format!(
                "face step {:.4}mm (trail_face {:.2}deg -> {:.4}, standoff {:.4})",
                face, rd.trail_face, rd.face_step(), op.face_standoff
            ));
        }
        if dr != 0.0 {
            parts.push(format!("radius {:+.4}mm off the declared {:.3}mm", dr, er.radius));
        }
        let place = corners.as_ref().map(|(_, at)| [at[0], at[1], p.stock.z]);
        if let Some((c, _)) = corners {
            parts.push(c);
        }
        out.push(Finding::op(FindingFamily::Deviation, k, &op.feature_name, format!(
            "op [{}] (edge round on '{}', T{}{}): {}",
            k,
            op.feature_name,
            op.tool_num,
            if rd.measured { ", measured" } else { ", catalog nominal" },
            parts.join("; ")
        )).with_place_opt(place));
    }
    out
}
