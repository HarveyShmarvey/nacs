// ---- check result: the check pipeline's answer, as one value ---------------------
//
// The one home the three renderings read — `--check=json` (stdout), the MCP
// `check` verb (the same document on a socket), and since Phase 11 the
// window's findings pane. Before this module the collector lived in the
// binary beside its JSON renderer, which the window (a lib module) could not
// reach; a second collector there would have been the two-sources drift
// tenet 7 forbids. Nothing here decides anything: it runs the pipeline the
// `--check` modes always ran — parse-time lint, then generation (the gates
// and the entry restriction inside it), then the post-generation families
// — and hands back what it said.

use crate::gcode::{self, OpEmission};
use crate::parse::{self, Finding, FindingFamily, Part};
use crate::{fixture, stock, vox};

/// What the pipeline said about a project: the error if it rejected (the
/// one-line message the prose modes print, `error: ` prefix included), every
/// finding in the order the families produce them, and the emission emitted moves when
/// there was one — `None` exactly when `refusal` is `Some`, because a rejected
/// program emits nothing and the two post-generation families never come to
/// exist for it.
pub struct CheckResult {
    pub refusal: Option<String>,
    pub findings: Vec<Finding>,
    pub emitted_moves: Option<Vec<OpEmission>>,
}

impl CheckResult {
    pub fn would_emit(&self) -> bool {
        self.refusal.is_none()
    }
}

/// Run the check pipeline on a parsed project.
pub fn check_result(part: &Part) -> CheckResult {
    let mut findings = parse::lint(part);
    match gcode::program_with_moves(part) {
        Ok((_, emitted_moves)) => {
            findings.extend(stock::entry_findings(part, &emitted_moves));
            findings.extend(stock::reach_findings(part, &emitted_moves));
            findings.extend(stock::envelope_findings(part, &emitted_moves));
            findings.extend(fixture::fixture_findings(part, &emitted_moves));
            findings.extend(vox::body_foul_findings(part, &emitted_moves).into_iter().map(|f| f.declining(FindingFamily::FieldBodyFoul)));
            // The walk's arm of the engagement family (Phase 38 slice 2):
            // an op that is mostly a slot, read from the moves it made.
            findings.extend(crate::engagement::slot_findings(part, &emitted_moves));
            // The entry restriction's declines (Phase 9 lane B): last, so the
            // order of every finding that existed before them is unchanged.
            findings.extend(stock::move_declines(part, &emitted_moves).into_iter().map(|f| f.declining(FindingFamily::Entry)));
            CheckResult { refusal: None, findings, emitted_moves: Some(emitted_moves) }
        }
        Err(e) => CheckResult { refusal: Some(format!("error: {}", e)), findings, emitted_moves: None },
    }
}

/// What one family did on this project (ROADMAP item 2, slice 6): the
/// account the empty Problems pane was never able to give. A family that
/// ran judged what it reads and voiced `found` findings; where a probe said
/// it could not judge an operation, that is a `declined`; a family with
/// nothing to judge says why in `idle`, read off the same declaration the
/// family reads.
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyAccount {
    pub family: FindingFamily,
    pub found: usize,
    pub declined: usize,
    pub idle: Option<String>,
}

impl FamilyAccount {
    pub fn ran(&self) -> bool {
        self.idle.is_none()
    }
}

/// The account of every family but `Decline`, in the window's order; the
/// declines are counted against the families they stood in for.
pub fn account(part: &Part, v: &CheckResult) -> Vec<FamilyAccount> {
    FAMILY_ORDER
        .iter()
        .filter(|f| **f != FindingFamily::Decline)
        .map(|&family| FamilyAccount {
            family,
            found: v.findings.iter().filter(|f| f.family == family).count(),
            declined: v.findings.iter().filter(|f| f.family == FindingFamily::Decline && f.about == Some(family)).count(),
            idle: idle_reason(part, v, family),
        })
        .collect()
}

/// Why a family had nothing to judge, where it had none: the declaration's
/// own facts, the ones the family reads before it looks at an operation.
fn idle_reason(part: &Part, v: &CheckResult, family: FindingFamily) -> Option<String> {
    let no_ops = part.operations.is_empty();
    // The post-generation families read moves; a refused program made none.
    let refused = v.refusal.is_some();
    let after_moves = |part: &Part| -> Option<String> {
        if no_ops {
            Some("no operations".to_string())
        } else if refused {
            Some("the program was refused before any move".to_string())
        } else {
            let _ = part;
            None
        }
    };
    match family {
        FindingFamily::Engagement | FindingFamily::BodyFoul => no_ops.then(|| "no operations".to_string()),
        FindingFamily::Deviation => {
            let forms = part.operations.iter().any(|op| op.follows_surface() && part.tools.get(&op.tool_num).is_some_and(|t| t.chamfer.is_some() || t.rounder.is_some()));
            (!forms).then(|| "no Side runs a chamfer tool or a corner rounder".to_string())
        }
        FindingFamily::Containment => part.features.is_empty().then(|| "no features are declared".to_string()),
        FindingFamily::Entry | FindingFamily::FieldBodyFoul => after_moves(part),
        FindingFamily::Fixture => after_moves(part).or_else(|| part.fixtures.is_empty().then(|| "no keep-out is declared".to_string())),
        FindingFamily::Reach => after_moves(part).or_else(|| (!part.tools.values().any(|t| t.stickout.is_some())).then(|| "no tool declares its stickout".to_string())),
        FindingFamily::Envelope => part.machine.is_none().then(|| "no machine is declared".to_string()),
        FindingFamily::PartViolation => Some("not run by the check: the swept field's reading of the part is the rest report's".to_string()),
        FindingFamily::Decline => None,
    }
}

/// How many families ran, how many had nothing to judge, and how many
/// operations some check could not judge.
pub fn account_counts(acc: &[FamilyAccount]) -> (usize, usize, usize) {
    let ran = acc.iter().filter(|a| a.ran()).count();
    (ran, acc.len() - ran, acc.iter().map(|a| a.declined).sum())
}

/// The account as prose, one line per family after a summary: what
/// `--check` prints on stdout, and the Problems pane's heading reads.
pub fn account_words(acc: &[FamilyAccount]) -> Vec<String> {
    let (ran, idle, declined) = account_counts(acc);
    let mut out = vec![format!(
        "checked: {ran} {} ran, {idle} had nothing to judge{}",
        if ran == 1 { "check" } else { "checks" },
        match declined {
            0 => String::new(),
            1 => ", 1 operation could not be judged".to_string(),
            n => format!(", {n} operations could not be judged"),
        }
    )];
    for a in acc {
        let word = family_word(a.family);
        let line = match &a.idle {
            Some(why) => format!("{word}: nothing to judge — {why}"),
            None => {
                let found = match a.found {
                    0 => "nothing found".to_string(),
                    1 => "1 finding".to_string(),
                    n => format!("{n} findings"),
                };
                let declined = match a.declined {
                    0 => String::new(),
                    1 => "; could not judge 1 operation".to_string(),
                    n => format!("; could not judge {n} operations"),
                };
                format!("{word}: ran — {found}{declined}")
            }
        };
        out.push(line);
    }
    out
}

/// The family's one word — the `--check=json` wire's spelling, which the
/// window's pane shows unchanged so the two never name one family two ways.
pub fn family_word(family: FindingFamily) -> &'static str {
    match family {
        FindingFamily::Engagement => "engagement",
        FindingFamily::Deviation => "deviation",
        FindingFamily::BodyFoul => "body_foul",
        FindingFamily::Containment => "containment",
        FindingFamily::Entry => "entry",
        FindingFamily::FieldBodyFoul => "field_body_foul",
        FindingFamily::Fixture => "fixture",
        FindingFamily::Reach => "reach",
        FindingFamily::Envelope => "envelope",
        FindingFamily::Decline => "decline",
        FindingFamily::PartViolation => "part_violation",
    }
}

/// Every family, in the order the window lists them: the six gates in the
/// order the floor runs them, then the advisories, then the declines — a
/// finding that can reject above one that can only advise, and a probe
/// saying it could not look last. Envelope is the sixth gate (Phase 15's
/// fork 4, taken at the DRAFT's blessing), standing where its eligibility
/// already placed it. Exhaustive by the pin in `view_tests`: a family this list forgets is a
/// family the pane never shows.
pub const FAMILY_ORDER: [FindingFamily; 11] = [
    FindingFamily::Engagement,
    FindingFamily::Deviation,
    FindingFamily::Entry,
    FindingFamily::Fixture,
    FindingFamily::Reach,
    FindingFamily::Envelope,
    FindingFamily::Containment,
    FindingFamily::BodyFoul,
    FindingFamily::FieldBodyFoul,
    FindingFamily::PartViolation,
    FindingFamily::Decline,
];
