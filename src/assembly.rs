// The cutter/holder optimizer (DECISIONS.md → "the cutter/holder optimizer —
// the holder nose is a search variable, and the answer is a ranked report").
//
// An ADVISORY recommender, kin to the linter family: numbers, not vibes, and
// nothing here feeds toolpath math. The catalog is a search space, never a
// gauging source — a chosen assembly's actuals still arrive by declaration in
// the .cam program (the corner-rounder law: nobody ever gauged from a
// catalogue). v1 was manual inputs, closed-form interference, the stepped
// cantilever, and a ranked report where a near-tie is a tie by name.
//
// WHAT THE CLEARANCE PROFILE CHANGED, and it is the whole geometry of this
// file (`DECISIONS.md` → "the optimizer's clearance profile…"). v1 compared
// one scalar opening against the holder's nose, which is the geometry of a
// tool that FILLS the hole — a drill, a plunge, a boring pass. A milling
// cutter orbits, so its axis stands `r_cav − r_tool` off the cavity axis and
// carries the nose out there with it, and the room left is the CUTTER's own
// radius rather than the cavity's. `Corridor` is that model: the room at each
// depth, from which v1's comparison falls out as the corner where the
// excursion is zero. The cutter is a participant in the interference question
// now, not only a stiffness number — which is why a cutter too big for the
// opening is an error here rather than a top-ranked recommendation.
//
// Scope still stated rather than hidden: the corridor is read from ONE
// declared feature and its own steps, so a cavity built from several features
// under-claims as the named one (the feature-only removal path is its own
// filed item); and no bisection is deferred any more — both sides of the
// comparison are piecewise constant, so the answer is a finite max.

use std::collections::HashSet;

/// One cutter record, absolute mm (the catalog states real hardware — unlike
/// an SVG profile, which carries shape and takes its size from `dia`).
#[derive(Clone, Debug, PartialEq)]
pub struct Cutter {
    pub name: String,
    pub dia: f64,
    pub flute_len: f64,
    pub shank_dia: f64,
    pub overall_len: f64,
    /// A relieved neck between flutes and shank, both-or-neither.
    pub neck_dia: Option<f64>,
    pub neck_len: Option<f64>,
    /// Young's modulus, GPa — declared, not baked (carbide ≈ 600, HSS ≈ 200
    /// is catalog data). Not a length: never unit-scaled.
    pub modulus: f64,
    /// Which feeds-table row this cutter looks up — the lookup's second axis
    /// (`DECISIONS.md` → "V3, the assembly's remainders…", fork 2A). A
    /// property of the hardware, so it lives on the hardware. Defaults to
    /// `Endmill`, which is what the stepped-cantilever model actually
    /// describes; declaring it is how a shop with drills on the wall stops
    /// that assumption being a guess. `tool_line` now emits it — until V4's
    /// slice it read this field nowhere and printed `Endmill(` regardless,
    /// which made one field's two consumers disagree.
    pub tool_type: String,
    /// How many flutes the cutter has (`DECISIONS.md` → "V4, tool and holder
    /// libraries…", fork 3 — the ONE field measured missing between a catalog
    /// record and a complete `tool` line). A count, not a length: it never
    /// scales, and it is whole. `Tool` has carried it since K2, and K3's
    /// chip-load identity (F = RPM × z × f_z) cannot be evaluated without it,
    /// so a record lacking it can still be declared — the declaration just
    /// cannot ask for `feed=derive`, and says so by rejecting there rather
    /// than here. **Optional**, the house law: an undeclared count emits no
    /// field and every existing catalog stays byte-identical.
    pub flute_count: Option<usize>,
    pub cost: Option<f64>,
}

/// One holder record, absolute mm.
#[derive(Clone, Debug, PartialEq)]
pub struct Holder {
    pub name: String,
    /// The nose: the narrow front section whose diameter decides whether the
    /// holder may follow the tool into an opening — the search variable this
    /// item exists for.
    pub nose_dia: f64,
    pub nose_len: f64,
    pub body_dia: f64,
    pub gauge_len: f64,
    /// Tip-translation compliance at the nose, mm per kN, added in series
    /// with the tool's own bending. Declared per record so a measured holder
    /// carries its own number; a constant understates a long nose at extreme
    /// stickout, which the report says out loud.
    pub compliance: f64,
    /// The shank diameters this holder can actually hold, `(lo, hi)` mm —
    /// an ER collet's range, or a shrink-fit's one size as `grip d..d`.
    /// **Optional**: an undeclared holder considers every cutter exactly as
    /// v1 did, which is the house law that undeclared means today's
    /// behaviour byte for byte.
    pub grip: Option<(f64, f64)>,
    pub cost: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub cutters: Vec<Cutter>,
    pub holders: Vec<Holder>,
}

/// A shared floor for the comparisons in this file. Every one of them is a
/// declared millimetre against a declared millimetre, and EQUALITY IS LEGAL —
/// a body exactly filling the room it has is touching, not interfering, the
/// same call the neck fields and the reach certificate already made.
const EPS: f64 = 1e-9;

/// The corridor an assembly must descend: how much room there is around the
/// TOOL AXIS at each depth below the blank top (`DECISIONS.md` → "the
/// optimizer's clearance profile…").
///
/// **The one formula this type exists to answer**, with the tip working at
/// depth `D` and a cutter of radius `r_tool`:
///
/// ```text
/// room(z) = r_cav(z) − r_cav(D) + r_tool
/// ```
///
/// The middle term is the axis excursion the job demands: a tool has to reach
/// the wall it is cutting, so its axis stands `r_cav(D) − r_tool` off the
/// cavity's axis and carries the holder nose out there with it. Three readings
/// fall out of the one line, and v1's blessed comparison is the first of them:
///
/// * `r_tool = r_cav(D)` — a drill, a plunge, a boring pass. The tool fills
///   the hole, the excursion is zero, and `room = r_cav(z)`: exactly
///   `nose_dia + 2·clearance ≤ opening`, v1's arithmetic, now holding for a
///   stated reason rather than by assumption.
/// * `r_cav(z) = r_cav(D)` — any straight wall. `room = r_tool` whatever the
///   cross-section, which is why `Unmeasured` below can answer without a
///   number: a pocket's opening is direction-dependent and was never the
///   binding quantity.
/// * `r_cav(z) > r_cav(D)` — a STEP. The cavity's extra width at that depth is
///   what the assembly may spend, and it is the only place a profile earns its
///   existence.
#[derive(Clone, Debug, PartialEq)]
pub enum Corridor {
    /// A staircase of declared cylindrical steps: `(depth of the band's top
    /// below the stock top, radius)`, shallowest first, radius NON-INCREASING
    /// — which is not a sort but a property of the model, since a cavity that
    /// widened with depth would be re-entrant and is unrepresentable by
    /// construction (`DECISIONS.md` → "Re-entrancy detection…").
    Steps(Vec<(f64, f64)>),
    /// A straight-walled cavity whose crossing this mode does not measure — a
    /// pocket, where the widest circle that fits is a polygon question and the
    /// answer would not be used anyway: the room is the cutter's own radius at
    /// every depth. `mouth` is how far below the stock top the cavity opens,
    /// and everything above it reads as SOLID — which under-claims a feature
    /// whose datum sits below the top face instead of guessing what opened it.
    Unmeasured { mouth: f64 },
    /// A revolved cavity — a bowl, a throat, a funnel (`DECISIONS.md` → "the
    /// revolved cavities…"). It is the one corridor that cannot be stated
    /// without a cutter: on a curved wall the ball's own radius decides both
    /// where its axis stands and how far its tip hangs below the contact, so
    /// this arm holds the DECLARATION and is placed per row by `place`.
    Revolved { feature: Box<crate::parse::Feature>, top: f64, depth: f64 },
}

/// What one cutter faces: the corridor as a placed staircase, the working
/// positions it must clear from, and how deep its own tip goes.
///
/// **The locus is the generalisation this type exists for.** v1 had a single
/// working depth; the clearance profile kept it; a curved wall does not have
/// one, because the tool works its way down and stands further out the higher
/// it is. So the working position becomes a LIST of `(tip depth, axis
/// excursion)` pairs — and a vertical-walled cavity is that list with exactly
/// one entry, which is why nothing about the prismatic answer moved.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub corridor: Corridor,
    pub locus: Vec<(f64, f64)>,
    pub reach: f64,
}

/// How many points a revolved meridian is sampled at. A CONSERVATISM knob,
/// not a precision claim: every band takes its own narrowest radius, so the
/// staircase is an inner bound of the wall at any count, and refining it can
/// only ever admit an assembly it previously rejected — never the reverse.
/// Deliberately not `view::WIRE_MERIDIAN_PTS`: that one is a drawing choice,
/// and a number that picks a tool must not be a function of one.
pub const CORRIDOR_SAMPLES: usize = 128;

/// What placing a cutter in a corridor actually needs to know about it: a
/// radius, and whether it is a ball (`DECISIONS.md` → "the fifth certificate
/// and the corridor…", fork 2A).
///
/// **Why this exists rather than a `&Cutter`.** Placement has two callers now
/// and they hold different records — the recommender has a catalog `Cutter`,
/// and `stock::reach_findings` has a program's declared `parse::Tool`. A
/// second placement path for declared tools would be the duplicated derived
/// description this file was refactored once already to avoid, so the
/// signature states what the arithmetic actually reads and lets each record be
/// one caller of it.
#[derive(Clone, Debug, PartialEq)]
pub struct CutterShape {
    /// The cutting radius: where the tool touches a straight wall, and the
    /// ball radius the meridian home is sampled at on a curved one.
    pub radius: f64,
    /// Whether the cutting end is a BALL — the only other thing placement
    /// reads, because a curved wall has no contact model for anything else.
    pub ball: bool,
    /// What an error calls this cutter, and what that cutter declared itself
    /// to be. Read by messages only, never by the arithmetic.
    pub name: String,
    pub kind: String,
}

impl CutterShape {
    /// A catalog record — the recommender's caller, and the shape this
    /// signature used to take whole.
    pub fn of_record(c: &Cutter) -> CutterShape {
        CutterShape {
            radius: c.dia / 2.0,
            ball: c.tool_type == "Ballmill",
            name: c.name.clone(),
            kind: c.tool_type.clone(),
        }
    }

    /// A program's declared tool — the certificate's caller.
    ///
    /// **A drawn profile is not a ball here**, even though `validate` admits
    /// one on a curved wall: the corridor's contact model is `*_cl_point` at a
    /// BALL radius, and a custom profile touches where its own drawn points
    /// say it does. So a profile tool is simply not placeable in a revolved
    /// cavity by this mode, which leaves the certificate's finding standing —
    /// the under-claiming side, as everywhere else in this narrowing.
    pub fn of_tool(t: &crate::parse::Tool) -> CutterShape {
        CutterShape {
            radius: t.dia / 2.0,
            ball: t.name == "Ballmill" && t.profile.is_none(),
            name: format!("T{}", t.num),
            kind: t.name.clone(),
        }
    }
}

impl Corridor {
    /// The single band a typed `opening=` states — v1's whole geometry, said
    /// in this type so that one model stands behind both doors of the mode.
    pub fn straight(radius: f64) -> Corridor {
        Corridor::Steps(vec![(0.0, radius)])
    }

    /// The cavity's own crossing RADIUS at `depth`, where the declaration
    /// states one. `None` is not "no room": it is "this mode did not measure a
    /// crossing", which the room formula does not need.
    pub fn crossing_at(&self, depth: f64) -> Option<f64> {
        match self {
            // Non-increasing, so the last band that has started is the
            // narrowest one reached — and a band boundary answers with the
            // NARROWER of its two, which is the under-claiming side.
            Corridor::Steps(bands) => {
                bands.iter().filter(|(top, _)| *top <= depth + EPS).map(|(_, r)| *r).last()
            }
            Corridor::Unmeasured { .. } => None,
            // A revolved cavity states no single crossing until it is placed:
            // its mouth is one number and its floor another, and which one the
            // reader wants depends on the ball. `place` answers instead.
            Corridor::Revolved { .. } => None,
        }
    }

    /// The room around the tool axis at `depth`, for a cutter of radius
    /// `r_tool` whose axis stands `excursion` off the cavity's own axis.
    ///
    /// **The excursion is an argument rather than a derivation now**, and that
    /// is the whole of what the revolved cavities changed: on a vertical wall
    /// it is `r_cav(D) − r_tool` and the caller works it out once, while on a
    /// curved wall it is the ball's own CL locus and there is a different one
    /// at every height. The formula above them both is unchanged.
    pub fn room_at(&self, depth: f64, r_tool: f64, excursion: f64) -> f64 {
        match self {
            Corridor::Steps(_) => self.crossing_at(depth).unwrap_or(0.0) - excursion,
            // Above the mouth is material, and material is not room. Below it
            // the wall is straight, so the extra width is nil and the room is
            // the cutter's own radius — no crossing needed.
            Corridor::Unmeasured { mouth } => {
                if depth + EPS < *mouth { 0.0 } else { r_tool }
            }
            // Placed corridors only: a `Revolved` is a declaration, and
            // `place` is what turns it into the staircase this answers from.
            Corridor::Revolved { .. } => 0.0,
        }
    }

    /// The deepest depth at which a body of `radius` still has room, for a
    /// cutter of radius `r_tool` reaching `work`. Zero means it never enters —
    /// v1's "nose blocked at the opening", said generally.
    ///
    /// Closed form, and the reason no bisection appears anywhere in this file:
    /// the room is a staircase, so the answer is the top of the first band
    /// that cannot take the body — a finite walk of the steps the declaration
    /// itself states.
    pub fn admits_to(&self, radius: f64, r_tool: f64, excursion: f64, work: f64) -> f64 {
        for top in self.band_tops() {
            if top >= work {
                break;
            }
            if self.room_at(top, r_tool, excursion) + EPS < radius {
                return top;
            }
        }
        work.max(0.0)
    }

    /// The depths at which the room can change — the bands' own tops, which is
    /// exactly why a bisection is unnecessary. The mirror of
    /// `BodyLadder::rung_heights` on the other side of the comparison: a
    /// consumer that needs the breakpoints takes them from the owner rather
    /// than restating the layout in a second place.
    fn band_tops(&self) -> Vec<f64> {
        match self {
            Corridor::Steps(bands) => bands.iter().map(|(top, _)| *top).collect(),
            // The stock's own top face is a breakpoint here even when the
            // cavity opens lower: a solid cap has to be walked into to be
            // rejected.
            Corridor::Unmeasured { mouth } => vec![0.0, *mouth],
            Corridor::Revolved { .. } => Vec::new(),
        }
    }

    /// What the header adds about the corridor, beyond the crossing the first
    /// line already prints. `None` when there is nothing more to say.
    pub fn described(&self) -> Option<String> {
        match self {
            Corridor::Steps(bands) if bands.len() > 1 => {
                let mut parts = Vec::new();
                for (i, (top, r)) in bands.iter().enumerate() {
                    parts.push(match bands.get(i + 1) {
                        Some((next, _)) => format!("{}mm across to {}mm deep", f3(r * 2.0), f3(*next)),
                        None => format!("{}mm across below that", f3(r * 2.0)),
                    });
                    let _ = top;
                }
                Some(parts.join(", then "))
            }
            Corridor::Steps(_) => None,
            Corridor::Unmeasured { mouth } if *mouth > EPS => Some(format!(
                "straight walls, opening {}mm below the stock top and solid above that",
                f3(*mouth)
            )),
            Corridor::Unmeasured { .. } => Some(
                "straight walls; no crossing is measured, because a straight wall leaves the cutter's own radius whatever its shape".to_string(),
            ),
            Corridor::Revolved { feature, depth, .. } => Some(format!(
                "the declared {} meridian, {}mm deep, sampled at {} points and every band taken at its own narrowest — so the staircase is an inner bound of the wall, and each ball's own tip depth is in its row",
                feature.kind(), f3(*depth), CORRIDOR_SAMPLES
            )),
        }
    }

    /// Place this corridor for one cutter: the staircase it answers from, the
    /// working locus, and how deep that cutter's tip goes.
    ///
    /// `declared_reach` is the reach a VERTICAL-walled cavity states about
    /// itself — a declaration fact, the same for every cutter. A revolved
    /// cavity has no such number and ignores it: how deep the tip goes depends
    /// on the ball, because a ball contacting a slope hangs below its own
    /// contact point.
    pub fn place(&self, c: &CutterShape, declared_reach: f64) -> Result<Placed, String> {
        let r_tool = c.radius;
        match self {
            Corridor::Revolved { feature, top, .. } => {
                // The pairing law is not invented here: `validate` rejects a
                // Copy on any non-prismatic surface unless the tool is a
                // Ballmill or carries a custom profile, so a rod cannot work
                // this cavity in a program either.
                if !c.ball {
                    return Err(format!(
                        "cutter '{}' declares type {} — a curved wall is finished by a BALL, and the program that would use this assembly rejects otherwise ('Copy operation requires a Ballmill tool')",
                        c.name, c.kind
                    ));
                }
                let m = crate::parse::cavity_meridian(feature, *top)
                    .ok_or_else(|| format!("feature is a {}, which states no meridian", feature.kind()))?;
                let wall = m.samples(CORRIDOR_SAMPLES, 0.0);
                let cl = m.samples(CORRIDOR_SAMPLES, r_tool);

                // The staircase: each band spans to the next sample and takes
                // the NARROWER of the two radii, which is the under-claiming
                // reading of a wall between two measured points.
                let mut bands: Vec<(f64, f64)> = Vec::with_capacity(wall.len());
                for (i, (rho, z)) in wall.iter().enumerate() {
                    let depth = (top - z).max(0.0);
                    let r = match wall.get(i + 1) {
                        Some((next_rho, _)) => rho.min(*next_rho),
                        None => *rho,
                    };
                    bands.push((if i == 0 { 0.0 } else { depth }, r.max(0.0)));
                }

                // The locus: one working position per sample — the ball's own
                // tip depth there, and how far its axis stands off the cavity's.
                let mut locus = Vec::with_capacity(cl.len());
                for (rho_cl, z_cl) in &cl {
                    if *rho_cl < -EPS {
                        // The finishing pass's own gouge guard, in the one form
                        // this mode can state cheaply: a ball whose centre
                        // would cross the axis cannot work the tip at all.
                        return Err(format!(
                            "the {}mm ball cannot work this cavity's floor — its centre would cross the axis, which is the gouge the finishing pass rejects",
                            f3(2.0 * c.radius)
                        ));
                    }
                    locus.push(((top - (z_cl - r_tool)).max(0.0), rho_cl.max(0.0)));
                }
                let reach = locus.iter().map(|(d, _)| *d).fold(0.0, f64::max);
                Ok(Placed { corridor: Corridor::Steps(bands), locus, reach })
            }
            // A vertical wall states its own reach, and the axis stands at one
            // place: as far out as the working cross-section makes it.
            _ => {
                if let Some(r_work) = self.crossing_at(declared_reach) {
                    if r_tool > r_work + EPS {
                        return Err(format!(
                            "the {}mm cutter cannot enter a {}mm opening — it is the tool that has to fit the cavity first, whatever holder is behind it",
                            f3(2.0 * c.radius), f3(r_work * 2.0)
                        ));
                    }
                }
                let excursion = self.crossing_at(declared_reach).map_or(0.0, |r| (r - r_tool).max(0.0));
                Ok(Placed {
                    corridor: self.clone(),
                    locus: vec![(declared_reach, excursion)],
                    reach: declared_reach,
                })
            }
        }
    }
}

/// The job: the reach the tip must make below the part top, the corridor it
/// makes that reach through, and the cavity's own depth. `clearance` is radial at the nose; `load` is the
/// representative tip load the report prices deflection at — declared, not
/// derived, because there is no feeds engine to derive one from.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    /// The workpiece — the feeds lookup's first axis, and a fact about the
    /// JOB rather than the shelf: the same cutter runs in 6061 on Monday and
    /// 304 on Tuesday. Mirrors `material=` living on the `stock`, never on
    /// the `tool`. Absent means the report keeps its honest omission line.
    pub material: Option<String>,
    /// The shop's own feeds table, when one is passed. **The shop's proven
    /// numbers outrank the shipped ones** — K3's founding law, and the reason
    /// shipping timid starter values is free. Resolution is
    /// `parse::feeds_lookup`'s, not a second rule: shop row wins, shipped row
    /// fills the gap, and a miss lists both inventories.
    ///
    /// It lives on the JOB rather than beside the catalog because a table is
    /// what the *work* runs at, not what is on the shelf — the same reasoning
    /// that put `material` here and `type` on the cutter (V3, fork 2A). This
    /// mode has no `.cam` program, so it takes the table as an input where a
    /// program would say `feeds_table "file"`.
    pub shop_feeds: Option<crate::parse::FeedsTable>,
    /// Where `reach`/`opening`/`depth` came from, when they were READ off a
    /// declared feature instead of typed (the STEP slice, fork 3A). `None`
    /// means the three numbers are the caller's own, which is v1's whole
    /// input model and stays exactly as it was.
    ///
    /// The report header echoes this for the same reason it echoes every
    /// other number governing the rows: a derived input whose provenance the
    /// reader cannot see is a number they cannot check.
    pub derived_from: Option<String>,
    /// The coaxial neighbours the corridor was composed with, by name
    /// (`DECISIONS.md` → "the multi-feature cavity…", fork 6A). Empty for a
    /// typed job and for a lone feature. The header echoes it for the same
    /// reason it echoes the corridor, the derived numbers and the feeds
    /// table's name: a reader who cannot see which features were read cannot
    /// check the descent — and it is real advice, because it says which cut
    /// has to come first for the recommended assembly to fit.
    pub composed: Vec<String>,
    pub reach: f64,
    /// The corridor the assembly descends (`DECISIONS.md` → "the optimizer's
    /// clearance profile…"). It REPLACES v1's `opening` scalar rather than
    /// sitting beside it: a typed `opening=` builds `Corridor::straight`, a
    /// declared feature builds its steps, and the crossing the header prints
    /// is read back off this one field. Two spellings of one crossing is the
    /// duplicated derived value this project was started over.
    pub corridor: Corridor,
    pub depth: f64,
    pub clearance: f64,
    pub load: f64,
}

impl Job {
    /// The crossing the header echoes — the cavity's narrowest measured width
    /// at the working depth, where the corridor states one.
    pub fn crossing(&self) -> Option<f64> {
        self.corridor.crossing_at(self.reach).map(|r| r * 2.0)
    }
}

/// What a declared cavity states about the job — the STEP slice's derivation
/// (`DECISIONS.md` → "the optimizer's STEP slice…", fork 1A), grown a corridor
/// by its successor.
///
/// **Why this reads a PROGRAM and not a bare `proposed.part`.** The STEP
/// slice's DRAFT said `part=<proposed.part>`; measured, a `proposed.part` is a
/// *fragment* — the sidecar writes `feature` lines and nothing else, so it does
/// not parse on its own ("stock size w and l must be positive") and, more
/// importantly, it does not carry the stock. Every number here is measured DOWN
/// FROM THE STOCK'S TOP, because `CylinderBore` has no `z`: a bore opens at the
/// top face and runs `extent` down. So the input is the program a human landed
/// the proposal into, which is the workflow the sidecar contract already
/// describes — recognition proposes text, a human reviews it, the program
/// declares it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cavity {
    pub reach: f64,
    pub depth: f64,
    pub corridor: Corridor,
    /// The coaxial neighbours whose own walls were folded into `corridor`,
    /// by name and sorted (`DECISIONS.md` → "the multi-feature cavity…",
    /// fork 6A). Empty when the cavity is the one feature that was named —
    /// and a neighbour that widened nothing is not listed, because the header
    /// states what governs the rows rather than what was looked at.
    pub composed: Vec<String>,
}

impl Cavity {
    /// One declared feature's own reading, before any neighbour is folded in.
    fn of(reach: f64, depth: f64, corridor: Corridor) -> Cavity {
        Cavity { reach, depth, corridor, composed: Vec::new() }
    }
}

/// Which coaxial neighbours a composition may count as OPEN
/// (`DECISIONS.md` → "the multi-feature cavity…", fork 4A). One law, one home,
/// and **the caller states which truth it means** — because the two consumers
/// are asking about different things:
///
/// - the recommender is handed a PART and asked what assembly makes a feature
///   in it. A landed proposal often declares no operations at all, so there is
///   no order to be aware of: it reads the declaration, and the report header
///   names the neighbours it composed and the condition it composed under.
/// - a certificate is handed a PROGRAM, which states an order. A check that
///   GATES may not silence a finding on room the program has not opened yet,
///   so it counts only what an earlier op has already opened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Opened {
    /// Every coaxial cavity the part declares, whether or not anything cuts it.
    Declared,
    /// Only what an operation BEFORE this index left open.
    Before(usize),
}

impl Opened {
    /// Does this neighbour count as open?
    ///
    /// **A neighbour cut with SKIN contributes nothing** (fork 4A's sub-fork):
    /// a roughing pass leaves material standing on the wall it was aimed at,
    /// so its room is narrower than its declaration by an amount this reader
    /// would have to model. One exact comparison instead — the op that opened
    /// it left none — which under-claims, and which rough-then-finish satisfies
    /// at the finish. Shrinking the radius by `derive_emitted_skin` is the
    /// refinement the entry names and does not build.
    ///
    /// A `Drill` states a LIST of points and carries no `feature_name`, so it
    /// never admits anything here — which is the honest reading twice over: a
    /// drill leaves the declared wall standing at its own diameter.
    fn admits(&self, p: &crate::parse::Part, name: &str) -> bool {
        match self {
            Opened::Declared => true,
            Opened::Before(idx) => p.operations.iter().take(*idx).any(|op| {
                op.feature_name == name
                    && crate::parse::derive_emitted_skin(p, op).is_some_and(|skin| skin <= EPS)
            }),
        }
    }
}

/// Read a declared cavity as a corridor — the named feature and every coaxial
/// neighbour that is open around it — or say why it is not a cavity a rod
/// descends into.
///
/// **A neighbour is ROOM, never WORK** (`DECISIONS.md` → "the multi-feature
/// cavity…", fork 3A). The job is still to cut the feature that was named, so
/// `reach` and `depth` are its own; what the neighbours change is the width
/// the assembly descends through on the way. At each depth the room is the
/// WIDEST neighbour open there — a max of staircases — and every contributing
/// variant opens at the stock's top, so a max of non-increasing staircases is
/// itself non-increasing and `crossing_at`'s "narrowest band reached" reading
/// survives untouched.
///
/// **One home, not two.** The clearance-profile DRAFT proposed a `corridor_of`
/// *beside* `opening_of`; building it, two functions reading one declaration
/// for two answers is the shape this file already rejects everywhere else, so
/// the crossing is read back off the corridor instead. The same argument is
/// why the composition is HERE rather than in a `cavity_of` beside it: the
/// signature already held the part, and used it for exactly one number.
pub fn corridor_of(
    p: &crate::parse::Part,
    f: &crate::parse::Feature,
    opened: Opened,
) -> Result<Cavity, String> {
    use crate::parse::Feature;
    let own = one_feature(p, f)?;
    // Composition is the VERTICAL-WALLED AXISYMMETRIC family's, and only that
    // family's (fork 2A). A feature with no axis states nothing to be coaxial
    // WITH; a revolved cavity states no staircase until a ball is placed in
    // it; an unmeasured wall states no crossing to widen. Each is left exactly
    // as it read before, which is the under-claiming side.
    let Some(axis) = f.axis() else { return Ok(own) };
    let own_bands = match &own.corridor {
        Corridor::Steps(bands) => Some(bands.clone()),
        _ => None,
    };
    let Some(own_bands) = own_bands else { return Ok(own) };

    // Sorted by name, because the answer may not depend on a hash map's
    // iteration order — the merge below is order-independent arithmetic, but
    // the NAMES the header prints and the error that names one are not.
    let mut names: Vec<&String> = p.features.keys().collect();
    names.sort();
    let mut open_walls: Vec<(&String, Vec<(f64, f64)>, f64)> = Vec::new();
    let mut standing: Vec<(&String, &Feature)> = Vec::new();
    for name in names {
        let other = &p.features[name];
        // The named feature is already in hand as `own`; it contributes
        // whatever the map holds it under, and it is never subject to the
        // `Opened` filter — it is the feature being CUT, not one being
        // descended past.
        if std::ptr::eq(other, f) {
            continue;
        }
        let Some(other_axis) = other.axis() else { continue };
        if (other_axis.x - axis.x).abs() > EPS || (other_axis.y - axis.y).abs() > EPS {
            continue;
        }
        match one_feature(p, other) {
            Ok(n) => match n.corridor {
                Corridor::Steps(bands) => {
                    if opened.admits(p, name) {
                        open_walls.push((name, bands, n.reach));
                    }
                }
                // A coaxial revolved cavity REMOVES material, and this reader
                // does not fold its wall in: ignoring a removal under-claims
                // the room, which is the safe side of the same line fork 2A
                // drew around the pocket.
                _ => {}
            },
            // Not a cavity at all: whatever it is, it STANDS material on this
            // cavity's own axis. Fork 5A below.
            Err(_) => standing.push((name, other)),
        }
    }

    let mut bands = own_bands;
    let mut composed: Vec<String> = Vec::new();
    for (name, other_bands, other_reach) in &open_walls {
        let merged = merge_bands(&bands, other_bands, *other_reach, own.reach);
        if merged != bands {
            composed.push((*name).clone());
            bands = merged;
        }
    }

    // **Fork 5A, the island — the one neighbour that NARROWS.** A boss
    // standing on this axis is material inside the mouth, and today it is
    // ignored, which is an over-claim: the report would hand a holder to a
    // hole with a post in it. It was ignored honestly while the mode read one
    // feature and could not see it; the moment this reads the neighbours, that
    // excuse expires. **Once you read the neighbours you own them** — so a
    // standing form that could be inside the corridor rejects the cavity
    // rather than being measured around.
    //
    // The extrusion bosses state a radius this reader can compare, so the
    // common and legal shape — a hole down the middle of a post, where the
    // boss SURROUNDS the corridor — passes. Anything else standing on the axis
    // is rejected by kind, because a dome's or a horn's own radius at each
    // depth is a contact question this advisory does not import.
    let widest = bands.iter().map(|(_, r)| *r).fold(0.0, f64::max);
    for (name, other) in standing {
        let surrounds = match other {
            Feature::CylinderBoss { dia, .. } => *dia / 2.0 + EPS >= widest,
            _ => false,
        };
        if !surrounds {
            return Err(match other {
                Feature::CylinderBoss { dia, .. } => format!(
                    "feature '{}' is a {}mm {} standing on this cavity's own axis, inside a corridor {}mm across — an island in the mouth is material this reader would otherwise hand a holder to",
                    name, f3(*dia), other.kind(), f3(2.0 * widest)
                ),
                _ => format!(
                    "feature '{}' is a {} standing on this cavity's own axis — this mode measures room ABOUT that axis, and a standing form on it is material it cannot account for at any depth",
                    name, other.kind()
                ),
            });
        }
    }

    Ok(Cavity { reach: own.reach, depth: own.depth, corridor: Corridor::Steps(bands), composed })
}

/// Two staircases, one staircase: the room at each depth is the wider of the
/// two, over `[0, limit]`.
///
/// The neighbour is TRUNCATED at its own reach — below its floor it removed
/// nothing, so it offers no room there. That truncation is what keeps the
/// result non-increasing without a second pass: each side is non-increasing
/// and drops to nothing at its own floor, and a max of non-increasing
/// staircases is non-increasing.
fn merge_bands(
    own: &[(f64, f64)],
    other: &[(f64, f64)],
    other_reach: f64,
    limit: f64,
) -> Vec<(f64, f64)> {
    let at = |bands: &[(f64, f64)], d: f64| {
        bands.iter().filter(|(top, _)| *top <= d + EPS).map(|(_, r)| *r).last().unwrap_or(0.0)
    };
    let mut tops: Vec<f64> = vec![0.0];
    tops.extend(own.iter().map(|(top, _)| *top));
    tops.extend(other.iter().map(|(top, _)| *top));
    tops.push(other_reach);
    tops.retain(|top| *top >= -EPS && *top <= limit + EPS);
    tops.sort_by(|a, b| a.partial_cmp(b).expect("band tops are finite"));
    tops.dedup_by(|a, b| (*a - *b).abs() <= EPS);

    let mut out: Vec<(f64, f64)> = Vec::new();
    for top in tops {
        let room = at(own, top).max(if top + EPS < other_reach { at(other, top) } else { 0.0 });
        match out.last() {
            Some((_, prev)) if (*prev - room).abs() <= EPS => {}
            _ => out.push((top, room)),
        }
    }
    out
}

/// Read ONE declared feature as a corridor, or say why it is not a cavity a
/// rod descends into.
///
/// **Types graduate one at a time, with their reason** — the sidecar's own
/// standing rule, applied on this side of the seam. What graduates here is the
/// VERTICAL-WALLED family, because on a straight wall the room is the cutter's
/// own radius and the arithmetic needs no contact model. On a sloped or curved
/// wall it does: where the tool touches decides the working radius, and this
/// catalog states rods. That is the finishing ops' question, and importing it
/// into an advisory would put a second contact model in the codebase.
fn one_feature(p: &crate::parse::Part, f: &crate::parse::Feature) -> Result<Cavity, String> {
    use crate::parse::Feature;
    let top = p.stock.z;
    match f {
        // Reach and depth are the same number for a bore, and that is not a
        // coincidence to paper over: the cavity IS the opening, so the tip's
        // reach below the top face and the opening's own depth are one
        // measurement. A counterbore is where they stop being one shape —
        // and it is the only mouth decoration that states a STEP. A csink or
        // spot is a cone whose depth the TOOL derives (`GroundCone`, and this
        // mode has no tool for it); an `edge_round` or `chamfer` is a break.
        // A monotone staircase can only read a taper at its narrow end, which
        // is the bore itself — so all three are read as nothing, which
        // under-claims rather than offering a nose room it cannot use.
        //
        // A rim below the top face (Phase 45) is where the two numbers part:
        // the reach grows by the mouth's depth, the opening's own depth does
        // not, and the staircase starts at the mouth — above it `crossing_at`
        // answers nothing, which reads as SOLID, the pocket's under-claiming
        // reading of a datum below the top rather than a guess at what
        // opened the step.
        Feature::CylinderBore { dia, extent, z, cbore, .. } => {
            let mouth = top - crate::parse::derive_contour_top(top, *z);
            let mut bands = Vec::new();
            if let Some(cb) = cbore {
                bands.push((mouth, cb.dia / 2.0));
                bands.push((mouth + cb.depth, *dia / 2.0));
            } else {
                bands.push((mouth, *dia / 2.0));
            }
            Ok(Cavity::of(mouth + *extent, *extent, Corridor::Steps(bands)))
        }
        // An ellipse states its crossing as the MINOR axis, because the holder
        // nose is a circle and the narrowest crossing is what it has to pass;
        // the major axis would be the flattering number and the wrong one.
        // The pocket, and the finding that shrank it: its opening IS
        // direction-dependent, and the direction-dependence is not the binding
        // number — a straight wall leaves the cutter's own radius whichever way
        // it runs. So the polygon is never measured, and what the declaration
        // is read for is its DEPTH.
        Feature::ProfilePocket { z, extent, .. } => {
            let mouth = top - crate::parse::derive_contour_top(top, *z);
            Ok(Cavity::of(mouth + *extent, *extent, Corridor::Unmeasured { mouth: mouth.max(0.0) }))
        }
        // The 3D-wire pocket: its mouth is not planar, so the corridor opens
        // at the DEEPEST mouth point — everything above that is read as solid,
        // which is the under-claiming reading of a wire this mode cannot see
        // the top of.
        Feature::ContourPocket { points, extent } => {
            let lowest = points.iter().map(|p3| p3.z).fold(f64::INFINITY, f64::min);
            if !lowest.is_finite() {
                return Err("the contour declares no points, so it states no mouth to descend".to_string());
            }
            let mouth = (top - lowest).max(0.0);
            Ok(Cavity::of(mouth + *extent, *extent, Corridor::Unmeasured { mouth }))
        }
        // The revolved cavities (`DECISIONS.md` → "the revolved cavities…").
        // The corridor cannot be stated here, because on a curved wall the
        // BALL decides both where its axis stands and how deep its tip hangs —
        // so this arm carries the declaration and `Corridor::place` reads it
        // per row, through the meridian home the finishing pass and the
        // wireframe already share.
        other if crate::parse::cavity_meridian(other, top).is_some() => {
            let m = crate::parse::cavity_meridian(other, top).expect("just checked");
            let (_, z_floor) = m.at(m.floor, 0.0);
            let depth = (top - z_floor).max(0.0);
            // The cavity's own depth to its deepest SURFACE point. Every
            // row's reach is deeper than this by its own ball's hang, and says
            // so in its row.
            Ok(Cavity::of(
                depth,
                depth,
                Corridor::Revolved { feature: Box::new(other.clone()), top, depth },
            ))
        }
        other => Err(format!(
            "feature is a {} — this mode reads a CAVITY a tool descends into: a round or elliptic bore, a pocket, or a revolved bowl, throat or funnel. A boss, a dome or a plane is not a cavity at all",
            other.kind()
        )),
    }
}

pub const DEFAULT_CLEARANCE_MM: f64 = 0.5;
pub const DEFAULT_LOAD_N: f64 = 10.0;
/// Minimum shank the holder must keep hold of, as a multiple of shank dia —
/// an advisory floor, stated in the report header.
pub const MIN_GRIP_SHANK_DIAS: f64 = 1.5;
/// L/D beyond this is flagged as chatter RISK — a flag, never a chatter model.
pub const LD_RISK: f64 = 4.0;
/// Deflection within this factor of the leader's reports as an explicit tie.
pub const TIE_FACTOR: f64 = 1.10;

// ---- catalog parsing --------------------------------------------------------

/// Read a catalog file. The `read_`/`parse_` split is the `.patch` reader's,
/// so the grammar is unit-testable without a file.
pub fn read_catalog(path: &str) -> Result<Catalog, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read catalog '{}': {}", path, e))?;
    parse_catalog(&text)
}

/// The catalog grammar: line-oriented, `#` comments, keyword-value lines,
/// unknown keywords reject with their line number. What is new against the
/// `.patch` grammar is plurality: `cutter <name>` / `holder <name>` lines open
/// a record, fields follow one per line, and the record closes at the next
/// opener or EOF. Duplicate names reject; a field before any opener rejects.
pub fn parse_catalog(text: &str) -> Result<Catalog, String> {
    // A record under construction: which opener, its name, its fields so far.
    struct Open {
        kind: &'static str,
        name: String,
        lineno: usize,
        fields: Vec<(String, f64, usize)>,
        /// Fields whose value is not a number. Kept in their own bucket so a
        /// typo in a numeric field still rejects with "is not a number"
        /// rather than being silently accepted as text.
        texts: Vec<(String, String, usize)>,
    }

    /// The keys whose values are words or ranges, not numbers.
    const TEXT_FIELDS: &[&str] = &["type", "grip"];

    let mut catalog = Catalog::default();
    let mut names: HashSet<String> = HashSet::new();
    let mut open: Option<Open> = None;

    let close = |open: Option<Open>, catalog: &mut Catalog| -> Result<(), String> {
        let Some(rec) = open else { return Ok(()) };
        let get = |key: &str| rec.fields.iter().find(|(k, _, _)| k == key).map(|(_, v, _)| *v);
        let text = |key: &str| rec.texts.iter().find(|(k, _, _)| k == key).map(|(_, v, _)| v.clone());
        let req = |key: &str| {
            get(key).ok_or_else(|| {
                format!("line {}: {} '{}' is missing a '{}' line", rec.lineno + 1, rec.kind, rec.name, key)
            })
        };
        let positive = |key: &str, v: f64, lineno: usize| {
            if v <= 0.0 {
                return Err(format!("line {}: {} '{}' {} ({}) must be positive", lineno + 1, rec.kind, rec.name, key, v));
            }
            Ok(())
        };
        for (k, v, lineno) in &rec.fields {
            if k != "cost" {
                positive(k, *v, *lineno)?;
            }
        }
        match rec.kind {
            "cutter" => {
                let allowed = ["dia", "flute_len", "shank_dia", "overall_len", "neck_dia", "neck_len", "modulus", "type", "flute_count", "cost"];
                for (k, _, lineno) in rec.fields.iter().map(|(k, _, l)| (k, (), *l)).chain(rec.texts.iter().map(|(k, _, l)| (k, (), *l))) {
                    if !allowed.contains(&k.as_str()) {
                        return Err(format!("line {}: unknown cutter field '{}'", lineno + 1, k));
                    }
                }
                // A count is a number that must be WHOLE — `parse.rs`'s
                // flute_count mold, spelled again here because the catalog's
                // numeric bucket is `f64` and the positive check above cannot
                // see the difference between 4 and 4.5 flutes.
                let flute_count = match rec.fields.iter().find(|(k, _, _)| k == "flute_count") {
                    None => None,
                    Some((_, v, ln)) => {
                        if v.fract() != 0.0 {
                            return Err(format!(
                                "line {}: cutter '{}' flute_count ({}) must be a whole number — it counts the cutter's flutes",
                                ln + 1, rec.name, v
                            ));
                        }
                        Some(*v as usize)
                    }
                };
                let c = Cutter {
                    name: rec.name.clone(),
                    dia: req("dia")?,
                    flute_len: req("flute_len")?,
                    shank_dia: req("shank_dia")?,
                    overall_len: req("overall_len")?,
                    neck_dia: get("neck_dia"),
                    neck_len: get("neck_len"),
                    modulus: req("modulus")?,
                    tool_type: text("type").unwrap_or_else(|| "Endmill".to_string()),
                    flute_count,
                    cost: get("cost"),
                };
                if c.neck_dia.is_some() != c.neck_len.is_some() {
                    return Err(format!(
                        "line {}: cutter '{}' declares half a neck — neck_dia and neck_len come together or not at all",
                        rec.lineno + 1, c.name
                    ));
                }
                if c.flute_len + c.neck_len.unwrap_or(0.0) >= c.overall_len {
                    return Err(format!(
                        "line {}: cutter '{}' has no shank left to grip — flute_len {} + neck {} is not under overall_len {}",
                        rec.lineno + 1, c.name, c.flute_len, c.neck_len.unwrap_or(0.0), c.overall_len
                    ));
                }
                catalog.cutters.push(c);
            }
            _ => {
                let allowed = ["nose_dia", "nose_len", "body_dia", "gauge_len", "compliance", "grip", "cost"];
                for (k, _, lineno) in rec.fields.iter().map(|(k, _, l)| (k, (), *l)).chain(rec.texts.iter().map(|(k, _, l)| (k, (), *l))) {
                    if !allowed.contains(&k.as_str()) {
                        return Err(format!("line {}: unknown holder field '{}'", lineno + 1, k));
                    }
                }
                // `grip lo..hi` borrows the .feeds chipload band's spelling —
                // one range syntax in this codebase, not two.
                let grip = match rec.texts.iter().find(|(k, _, _)| k == "grip") {
                    None => None,
                    Some((_, v, ln)) => {
                        let Some((lo, hi)) = v.split_once("..") else {
                            return Err(format!("line {}: holder '{}' grip '{}' is not a range — spell it lo..hi (a shrink-fit's one size is d..d)", ln + 1, rec.name, v));
                        };
                        let lo: f64 = lo.trim().parse().map_err(|_| format!("line {}: holder '{}' grip low '{}' is not a number", ln + 1, rec.name, lo))?;
                        let hi: f64 = hi.trim().parse().map_err(|_| format!("line {}: holder '{}' grip high '{}' is not a number", ln + 1, rec.name, hi))?;
                        if lo <= 0.0 || hi < lo {
                            return Err(format!("line {}: holder '{}' grip {}..{} is not a range a shank can be inside", ln + 1, rec.name, lo, hi));
                        }
                        Some((lo, hi))
                    }
                };
                let h = Holder {
                    name: rec.name.clone(),
                    nose_dia: req("nose_dia")?,
                    nose_len: req("nose_len")?,
                    body_dia: req("body_dia")?,
                    gauge_len: req("gauge_len")?,
                    compliance: req("compliance")?,
                    grip,
                    cost: get("cost"),
                };
                if h.nose_dia > h.body_dia {
                    return Err(format!(
                        "line {}: holder '{}' nose_dia ({}) is wider than its body_dia ({}) — a nose is the narrow front section",
                        rec.lineno + 1, h.name, h.nose_dia, h.body_dia
                    ));
                }
                catalog.holders.push(h);
            }
        }
        Ok(())
    };

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let mut tok = line.split_whitespace();
        let key = tok.next().unwrap();
        match key {
            "cutter" | "holder" => {
                let name = tok.next().ok_or_else(|| {
                    format!("line {}: '{}' needs a name", lineno + 1, key)
                })?;
                if tok.next().is_some() {
                    return Err(format!("line {}: '{}' takes one name (no spaces)", lineno + 1, key));
                }
                if !names.insert(name.to_string()) {
                    return Err(format!("line {}: duplicate record name '{}'", lineno + 1, name));
                }
                close(open.take(), &mut catalog)?;
                open = Some(Open {
                    kind: if key == "cutter" { "cutter" } else { "holder" },
                    name: name.to_string(),
                    lineno,
                    fields: Vec::new(),
                    texts: Vec::new(),
                });
            }
            _ => {
                let Some(rec) = open.as_mut() else {
                    return Err(format!(
                        "line {}: field '{}' before any 'cutter <name>' or 'holder <name>' opener",
                        lineno + 1, key
                    ));
                };
                let val = tok.next().ok_or_else(|| {
                    format!("line {}: '{}' needs a value", lineno + 1, key)
                })?;
                if tok.next().is_some() {
                    return Err(format!("line {}: '{}' takes one value", lineno + 1, key));
                }
                if TEXT_FIELDS.contains(&key) {
                    if rec.texts.iter().any(|(k, _, _)| k == key) {
                        return Err(format!("line {}: duplicate field '{}'", lineno + 1, key));
                    }
                    rec.texts.push((key.to_string(), val.to_string(), lineno));
                    continue;
                }
                let v: f64 = val.parse().map_err(|_| {
                    format!("line {}: '{}' value '{}' is not a number", lineno + 1, key, val)
                })?;
                if rec.fields.iter().any(|(k, _, _)| k == key) {
                    return Err(format!("line {}: duplicate field '{}'", lineno + 1, key));
                }
                rec.fields.push((key.to_string(), v, lineno));
            }
        }
    }
    close(open.take(), &mut catalog)?;
    if catalog.cutters.is_empty() || catalog.holders.is_empty() {
        return Err("a catalog needs at least one cutter and one holder to have anything to pair".to_string());
    }
    Ok(catalog)
}

// ---- the stickout solve (closed form, v1) -----------------------------------

/// One assembly's check result.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Fit {
        projection: f64,
        l_over_d: f64,
        /// Tip deflection at the job's load, mm.
        deflection: f64,
        nose_enters: bool,
        /// How deep THIS cutter's tip goes. Equal to the job's stated reach on
        /// a vertical wall; on a curved one it is the ball's own, because a
        /// ball contacting a slope hangs below its contact point.
        reach: f64,
    },
    Infeasible(String),
}

/// The projections one cutter can physically be held at, `(floor, ceiling)`
/// mm: at least the flutes (plus any neck) the holder cannot swallow, at most
/// what leaves `MIN_GRIP_SHANK_DIAS` of shank inside. A property of the cutter
/// alone — no holder, no job.
///
/// **One home on purpose** (V4's slice). `min_projection` DERIVES a number and
/// must land inside this band; `--tool-line` is HANDED one and has to check
/// it. Two spellings of the same arithmetic is how a derived value drifts from
/// its own law, which is the bug this file was already refactored once to
/// avoid.
pub fn projection_band(c: &Cutter) -> (f64, f64) {
    (
        c.flute_len + c.neck_len.unwrap_or(0.0),
        c.overall_len - MIN_GRIP_SHANK_DIAS * c.shank_dia,
    )
}

/// Minimum projection for one (cutter, holder) pair, closed form — the foul
/// question inverted, and still arithmetic rather than a search.
///
/// **v1 asked it of the nose alone against one scalar opening. This asks it of
/// every rung against the corridor**, which is the same question with the
/// participants it always had: the cutter (whose own radius IS the room a
/// straight wall leaves, and whose shank stands where the flutes stop), the
/// holder's nose, and the body behind the nose. Each rung is checked at the
/// deepest depth it would stand, and the projection is the shallowest one that
/// clears them all — then floored by the flutes+neck the holder cannot swallow
/// and ceilinged by the shank grip it must keep.
///
/// **No bisection, and none is deferred either** (the item has carried one
/// since 2026-08-08). Both sides are piecewise constant — the corridor's bands
/// and the assembly's rungs — so the least clearing projection is a max over a
/// finite set of comparisons, and iterating toward it would be approximating an
/// answer that can be written down.
pub fn min_projection(c: &Cutter, h: &Holder, job: &Job) -> Result<(f64, bool, f64), String> {
    let r_tool = c.dia / 2.0;
    let placed = job.corridor.place(&CutterShape::of_record(c), job.reach)?;
    let corridor = &placed.corridor;
    let (exposed, max_projection) = projection_band(c);

    let mut projection: f64 = exposed;
    let mut nose_enters = false;
    let mut blocked_at: Option<f64> = None;
    // Every position the tool works from. A vertical wall has one; a curved
    // one has as many as the meridian was sampled at, and the assembly has to
    // clear the cavity from ALL of them — the tool higher up stands further
    // out, which is exactly where a mouth-parked nose is nearest the wall.
    for (d, excursion) in &placed.locus {
        let (d, excursion) = (*d, *excursion);

        // The cutter's OWN body, above the flutes. Its heights are fixed
        // relative to the tip, so a failure here is a failure at every
        // projection — which is why reach tools are ground with a relieved
        // neck. Checked at each rung's own bottom, the deepest it stands, and
        // WITHOUT the nose clearance: `clearance` is declared as the NOSE's
        // radial allowance, and a straight shank sits exactly in the room the
        // cutter itself makes. Refusing every straight-shank endmill in a deep
        // cavity would be this mode restating the flute-length law, which
        // belongs to the engagement family.
        let mut rung_bottom = c.flute_len;
        if let (Some(nd), Some(nl)) = (c.neck_dia, c.neck_len) {
            if rung_bottom < d && corridor.room_at(d - rung_bottom, r_tool, excursion) + EPS < nd / 2.0 {
                return Err(format!(
                    "the {}mm neck stands {}mm above the tip, where the cavity leaves {}mm of room — the relieved section is not relieved enough for this cavity",
                    f3(nd), f3(rung_bottom), f3(2.0 * corridor.room_at(d - rung_bottom, r_tool, excursion))
                ));
            }
            rung_bottom += nl;
        }
        if rung_bottom < d && corridor.room_at(d - rung_bottom, r_tool, excursion) + EPS < c.shank_dia / 2.0 {
            return Err(format!(
                "the {}mm shank stands {}mm above the tip, where the cavity leaves {}mm of room — the shank enters the cavity before the flutes have cleared it",
                f3(c.shank_dia), f3(rung_bottom), f3(2.0 * corridor.room_at(d - rung_bottom, r_tool, excursion))
            ));
        }

        // The holder, both rungs, each at the deepest depth it would stand.
        // The nose's limit is where the projection is solved from; the BODY's
        // limit is what v1 spelled as `nose_len` and is now the same
        // comparison one rung up — the nose stops descending where the body
        // behind it runs out of room.
        let z_nose = corridor.admits_to(h.nose_dia / 2.0 + job.clearance, r_tool, excursion, d);
        let z_body = corridor.admits_to(h.body_dia / 2.0 + job.clearance, r_tool, excursion, d);
        let here = (d - z_nose).max(d - h.nose_len - z_body).max(0.0);
        if here > projection {
            projection = here;
        }
        if z_nose > EPS {
            nose_enters = true;
        } else if blocked_at.is_none() {
            blocked_at = Some(2.0 * corridor.room_at(0.0, r_tool, excursion));
        }
    }

    if projection > max_projection {
        let why = match blocked_at {
            None => format!(
                "needs {}mm of projection (reach {} less {}mm of nose descent), past the {}mm the {}mm shank grip leaves",
                f3(projection), f3(placed.reach), f3(placed.reach - projection), f3(max_projection),
                f3(MIN_GRIP_SHANK_DIAS * c.shank_dia)
            ),
            Some(room) => format!(
                "nose blocked at the mouth ({}mm across plus 2x{}mm clearance, against {}mm of room), so the full {}mm reach must project — past the {}mm the shank grip leaves",
                f3(h.nose_dia), f3(job.clearance), f3(room), f3(projection), f3(max_projection)
            ),
        };
        return Err(why);
    }
    Ok((projection, nose_enters, placed.reach))
}

/// Tip deflection per unit load of a stepped cantilever, δ/F = Σ (L³out −
/// L³in) / (3·E·I) with I = πd⁴/64 — segments given tip-first as (length,
/// dia). Split out so the stiffness-ladder pins can test a bare rod.
pub fn cantilever_deflection_per_n(segments: &[(f64, f64)], modulus_gpa: f64) -> f64 {
    let e = modulus_gpa * 1000.0; // GPa -> N/mm^2
    let mut from_tip = 0.0;
    let mut sum = 0.0;
    for (len, dia) in segments {
        let (l_in, l_out) = (from_tip, from_tip + len);
        let i = std::f64::consts::PI * dia.powi(4) / 64.0;
        sum += (l_out.powi(3) - l_in.powi(3)) / (3.0 * e * i);
        from_tip = l_out;
    }
    sum
}

/// The whole assembly's tip deflection at `load` newtons: the cutter's
/// exposed sections bending (flute core at 0.78·dia effective, neck, shank to
/// the nose), plus the holder's declared compliance in series.
pub fn assembly_deflection(c: &Cutter, h: &Holder, projection: f64, load: f64) -> f64 {
    let neck_len = c.neck_len.unwrap_or(0.0);
    let shank_exposed = (projection - c.flute_len - neck_len).max(0.0);
    let mut segments = vec![(c.flute_len, 0.78 * c.dia)];
    if let (Some(nd), Some(nl)) = (c.neck_dia, c.neck_len) {
        segments.push((nl, nd));
    }
    segments.push((shank_exposed, c.shank_dia));
    let bend = cantilever_deflection_per_n(&segments, c.modulus) * load;
    let holder = h.compliance * load / 1000.0; // mm/kN, load in N
    bend + holder
}

/// Judge one pair.
pub fn judge(c: &Cutter, h: &Holder, job: &Job) -> Verdict {
    // Grip first: a holder that cannot hold this shank is not a candidate at
    // any projection, so asking for one would be arithmetic about an assembly
    // that cannot exist. Reported as an Infeasible ROW rather than filtered,
    // per the report's own law — a search space that silently shrinks reads
    // as coverage it does not have.
    if let Some((lo, hi)) = h.grip {
        if c.shank_dia < lo || c.shank_dia > hi {
            return Verdict::Infeasible(format!(
                "the {:.3}mm shank is outside the holder's {:.3}..{:.3}mm grip range — this holder cannot hold this cutter at any projection",
                c.shank_dia, lo, hi
            ));
        }
    }
    match min_projection(c, h, job) {
        Err(why) => Verdict::Infeasible(why),
        Ok((projection, nose_enters, reach)) => Verdict::Fit {
            projection,
            l_over_d: projection / c.dia,
            deflection: assembly_deflection(c, h, projection, job.load),
            nose_enters,
            reach,
        },
    }
}

// ---- the declaration hand-off (Phase 8 slice 0) ------------------------------
//
// TWO SPELLINGS OF ONE HOLDER, AND WHY NEITHER ABSORBS THE OTHER.
//
// `Holder` here and `parse::HolderRungs` there describe the same physical
// object and were written independently — this file's records landed with the
// optimizer (2026-08-08), K2 slice 1 grew the rungs on `parse::Silhouette`'s
// `BodyLadder` (2026-08-16), and nothing connected them. That is a duplicated
// derived description in a codebase whose founding bug was a duplicated
// derived value, so it gets a conversion and a test rather than a shrug.
//
// It does NOT get a merge, because the two are genuinely different kinds:
//
//   * a catalog record is UNPLACED and carries facts a program never
//     declares — `gauge_len`, `compliance`, `modulus`, `cost` — and its
//     `projection` is the search variable, the whole point of the optimizer;
//   * a `BodyLadder` is PLACED on one tool at one declared `stickout`, in
//     heights above the tip, and is what every certificate reads.
//
// So the honest relation is a direction of travel, and the catalog law fixes
// which way it runs: the catalog is "a search space, never a gauging source",
// therefore a chosen assembly must leave here as TEXT an author reviews and
// declares, never as geometry something gauges behind their back. `tool_line`
// is that hand-off and `assembly_ladder` is the one place that knows how a
// record's diameters and lengths become a ladder's radii and heights.
//
// This is not an answer to V4's declaration-source-vs-clipboard fork. That
// fork is about LIBRARIES; the catalog's own law was settled at its birth,
// and clipboard is what it already said.

/// The `BodyLadder` a chosen assembly would have once declared — the one
/// place that converts catalog diameters and lengths into ladder radii and
/// heights above the tip. Mirrors `Silhouette::of`'s construction exactly;
/// the fill-and-parse pin in the tests is what keeps the mirror honest.
pub fn assembly_ladder(c: &Cutter, h: &Holder, projection: f64) -> crate::parse::BodyLadder {
    crate::parse::BodyLadder {
        shank: Some(crate::parse::ShankRung {
            flute_top: c.flute_len,
            shank_r: c.shank_dia / 2.0,
            // The record's neck, finally declarable (`DECISIONS.md` → "K2's
            // neck fields…"). Both-or-neither on both sides of the hand-off,
            // so the `match` can only take the declared shape — and the
            // fill-and-parse pin is what would have caught this rung going
            // missing, which is exactly what it did.
            neck: match (c.neck_dia, c.neck_len) {
                (Some(nd), Some(nl)) => Some((c.flute_len + nl, nd / 2.0)),
                _ => None,
            },
        }),
        holder: Some(crate::parse::HolderRungs {
            nose_face: projection,
            nose_r: h.nose_dia / 2.0,
            body: Some((projection + h.nose_len, h.body_dia / 2.0)),
        }),
    }
}

/// How the emitted line places the cutter. **One-of-N so the combination the
/// parser rejects cannot be built here at all** (`parse.rs`'s `comp` law):
/// holder geometry without a stickout is rejected downstream — "the nose face
/// sits at the stickout plane, and a holder the ladder cannot place is a
/// declaration that quietly does not exist" — so a holder and its projection
/// travel together or not at all.
#[derive(Clone, Copy, Debug)]
pub enum Placement<'a> {
    /// No holder chosen: the cutter's own facts, and a stickout only if the
    /// caller has one to state. The shop wall's record of one cutter, which
    /// is a complete declarable thing on its own.
    Loose { stickout: Option<f64> },
    /// The assembly: a holder, and the projection its nose face sits at.
    InHolder { holder: &'a Holder, projection: f64 },
}

/// The `tool` line an author would write for a chosen assembly — the catalog's
/// only legal exit. `T?` is a placeholder because which slot a tool lives in
/// is the program's business, never the recommender's (`template.rs`'s law,
/// and the same reason it never guesses a tool).
///
/// **The kind is the record's own** (V4's slice). This printed `Endmill(`
/// unconditionally until V4, including after V3 gave the record a `tool_type`
/// the feeds lookup honors — one field, two consumers, and only one of them
/// had stopped guessing. The kind is free text in the parser (`Ballmill` and
/// `Endmill` carry meaning at specific sites; nothing else is enumerated), so
/// a shop's drill declares itself a drill and the line still parses.
///
/// **The neck was a named loss and is now a declared one.** A catalog cutter
/// may carry `neck_dia`/`neck_len`, and until 2026-08-18 `parse::Tool` had no
/// such fields — so a necked cutter could not be declared in full and this line
/// carried a NOTE saying the emitted body was WIDER than the real one between
/// flutes and shank. That note was the trigger K2's design named ("the first
/// real necked-tool program"), it fired on shipped data every time
/// `sample.catalog`'s `1/4-reach` ranked, and it is gone: the fields exist, the
/// neck rides the line, and the ladder either side of the hand-off agrees.
///
/// **An absent `flute_count` gets NO note, deliberately.** The neck note
/// exists because the emitted body is WIDER than the real one — a silent
/// understatement of the thing about to be gauged. A missing flute count
/// understates nothing: it is a field the line simply does not carry, and
/// `feed=derive` already rejects by name and says which field is missing.
/// A note here would duplicate an error that is better placed.
pub fn tool_line(c: &Cutter, placement: Placement) -> String {
    let mut fields = format!(
        "dia={}, flute_len={}, shank_dia={}",
        f3(c.dia), f3(c.flute_len), f3(c.shank_dia)
    );
    // The neck rides between the shank and the placement, which is where it
    // sits on the tool: a cutter fact, above the flutes, below whatever holds
    // it. Both-or-neither is the catalog's own law, so one `if` covers it.
    if let (Some(nd), Some(nl)) = (c.neck_dia, c.neck_len) {
        fields.push_str(&format!(", neck_dia={}, neck_len={}", f3(nd), f3(nl)));
    }
    if let Some(z) = c.flute_count {
        fields.push_str(&format!(", flute_count={}", z));
    }
    match placement {
        Placement::Loose { stickout } => {
            if let Some(s) = stickout {
                fields.push_str(&format!(", stickout={}", f3(s)));
            }
        }
        Placement::InHolder { holder: h, projection } => {
            fields.push_str(&format!(
                ", stickout={}, holder=\"{}\", holder_nose_dia={}, holder_nose_len={}, holder_body_dia={}",
                f3(projection), h.name, f3(h.nose_dia), f3(h.nose_len), f3(h.body_dia)
            ));
        }
    }
    format!("tool T? {}({})", c.tool_type, fields)
}

// ---- the report -------------------------------------------------------------

fn f3(v: f64) -> String {
    format!("{:.3}", v)
}

/// The spindle speed the shipped feeds table recommends for one cutter in the
/// job's material, or a reason it cannot be had.
///
/// Derived through `parse::derive_spindle_rpm` — the RPM identity's one home,
/// never restated here — and looked up on the two axes the design put in two
/// different places: the material is the JOB's, the tool type the CUTTER's.
/// **The shop's table outranks the shipped one**, which was V3's named
/// remainder and is now `feeds=<file>` on this mode. Resolution is
/// `parse::feeds_lookup`'s and nothing here re-decides it: a shop row wins, a
/// shipped row fills the gap, and a miss lists both inventories in file order.
/// A `.cam` program says `feeds_table "file"`; this mode has no program, so it
/// takes the same table as an input.
fn cutter_rpm(c: &Cutter, job: &Job) -> Result<f64, String> {
    let Some(material) = job.material.as_deref() else {
        return Err("no material input".to_string());
    };
    let shipped = crate::parse::parse_feeds(crate::parse::STARTER_FEEDS)
        .map_err(|e| format!("shipped feeds table: {e}"))?;
    let rec = crate::parse::feeds_lookup(job.shop_feeds.as_ref(), &shipped, material, &c.tool_type)?;
    Ok(crate::parse::derive_spindle_rpm(rec.surface_speed, c.dia))
}

/// The ranked report — `vox::rest_report`'s conventions: a header echoing
/// every number that governs the rows, explicit ties with tiebreaker data (a
/// near-tie silently ranked is a deviation hidden), infeasible combos as rows
/// with reasons (a search space that silently shrinks reads as coverage it
/// does not have), and an honest omission line where a number would have had
/// to be invented.
pub fn assembly_report(catalog: &Catalog, job: &Job) -> Result<String, String> {
    if job.reach <= 0.0 || job.depth <= 0.0 || job.crossing().is_some_and(|o| o <= 0.0) {
        return Err("reach, opening, and depth must all be positive".to_string());
    }
    if job.clearance < 0.0 {
        return Err("clearance must not be negative".to_string());
    }
    if job.load <= 0.0 {
        return Err("load must be positive".to_string());
    }

    let mut fits: Vec<(usize, usize, f64, f64, f64, bool, f64)> = Vec::new();
    let mut infeasible: Vec<(usize, usize, String)> = Vec::new();
    for (ci, c) in catalog.cutters.iter().enumerate() {
        for (hi, h) in catalog.holders.iter().enumerate() {
            match judge(c, h, job) {
                Verdict::Fit { projection, l_over_d, deflection, nose_enters, reach } => {
                    fits.push((ci, hi, projection, l_over_d, deflection, nose_enters, reach));
                }
                Verdict::Infeasible(why) => infeasible.push((ci, hi, why)),
            }
        }
    }
    fits.sort_by(|a, b| a.4.partial_cmp(&b.4).unwrap());

    let mut out = String::new();
    // The first line stays v1's word for word wherever the cavity states a
    // crossing — a pocket is the one that cannot, and says so rather than
    // printing a number nobody declared.
    out.push_str(&match (job.crossing(), &job.corridor) {
        // A revolved cavity has no ONE reach to print: how deep a tip goes
        // depends on the ball, so the header states the cavity and each row
        // states its own.
        (_, Corridor::Revolved { depth, .. }) => format!(
            "assembly report — a revolved cavity {}mm deep, each ball's own reach in its row\n",
            f3(*depth)
        ),
        (Some(o), _) => format!(
            "assembly report — reach {}mm through a {}mm opening, {}mm deep\n",
            f3(job.reach), f3(o), f3(job.depth)
        ),
        (None, _) => format!(
            "assembly report — reach {}mm into a cavity that states no crossing, {}mm deep\n",
            f3(job.reach), f3(job.depth)
        ),
    });
    // The law that decides every row, said once where the reader can check the
    // rows against it (`DECISIONS.md` → "the optimizer's clearance profile…").
    out.push_str("  the room a nose gets is the cavity's extra width at that depth PLUS the cutter's own radius — a tool must reach the wall it cuts, so its axis stands off the cavity axis and takes the nose with it\n");
    if let Some(shape) = job.corridor.described() {
        out.push_str(&format!("  corridor: {}\n", shape));
    }
    if !job.composed.is_empty() {
        out.push_str(&format!(
            "  composed with {} — coaxial {} this part declares, so the room above is there once {} open, and not before\n",
            job.composed.iter().map(|n| format!("'{}'", n)).collect::<Vec<_>>().join(", "),
            if job.composed.len() == 1 { "feature" } else { "features" },
            if job.composed.len() == 1 { "it is" } else { "they are" },
        ));
    }
    out.push_str(&format!(
        "  nose clearance {}mm radial; deflection at {:.1}N tip load (declared: the feeds table gives speeds and chip loads, not cutting forces)\n",
        f3(job.clearance), job.load
    ));
    out.push_str(&format!(
        "  grip floor {} x shank dia; L/D past {:.1} flagged as chatter risk (a flag, not a chatter model)\n",
        MIN_GRIP_SHANK_DIAS, LD_RISK
    ));
    out.push_str("  holder compliance is a per-record constant in series — it understates a long nose at extreme stickout\n");
    // Fork 3A: a derived input whose provenance the reader cannot see is a
    // number they cannot check, and the header's whole job is to carry
    // everything that governs the rows.
    if let Some(src) = &job.derived_from {
        out.push_str(&format!(
            "  reach/{}depth READ from {}, not typed\n",
            if job.crossing().is_some() { "opening/" } else { "" },
            src
        ));
    }
    match &job.material {
        // The header echoes every number that governs the rows, so it has to
        // echo WHICH TABLE they came from: a reader who cannot tell the
        // shipped starting points from their own shop's proven numbers cannot
        // tell how much to trust the column.
        Some(m) => out.push_str(&format!(
            "  rpm: from {}, {} x each cutter's own type, at its own diameter\n",
            match &job.shop_feeds {
                Some(t) => format!(
                    "the shop feeds table ({} row{}, shipped table behind it for anything it does not hold)",
                    t.records.len(),
                    if t.records.len() == 1 { "" } else { "s" }
                ),
                None => "the shipped feeds table".to_string(),
            },
            m
        )),
        // The honest omission, shortened to the half still true: K3 built the
        // engine, so what is missing now is only the input.
        None => out.push_str("  rpm: omitted — no material input (pass material=<name>)\n"),
    }
    out.push_str("  each row carries the `tool` line to declare it: the catalog proposes text, the program declares it, the certificates gauge the declaration\n");

    if fits.is_empty() {
        out.push_str("\nno feasible assembly: every pair is listed below with its reason\n");
    } else {
        let leader = fits[0].4;
        let tie_count = fits.iter().take_while(|row| row.4 <= leader * TIE_FACTOR).count();
        out.push('\n');
        let per_row_reach = matches!(job.corridor, Corridor::Revolved { .. });
        for (idx, (ci, hi, projection, ld, defl, nose_enters, reach)) in fits.iter().enumerate() {
            let c = &catalog.cutters[*ci];
            let h = &catalog.holders[*hi];
            let rank = if idx < tie_count && tie_count > 1 {
                "  =1.".to_string()
            } else {
                format!("  {}.", idx + 1)
            };
            let mut flags = Vec::new();
            if *ld > LD_RISK {
                flags.push(format!("L/D {:.1} chatter risk", ld));
            }
            if *nose_enters {
                flags.push("nose enters the opening".to_string());
            }
            let rpm_s = match cutter_rpm(c, job) {
                Ok(rpm) => format!(", {} rpm", f3(rpm)),
                // A row that cannot price its own speed says so in its own
                // line rather than letting the header's promise stand for it.
                Err(_) if job.material.is_some() => ", rpm n/a".to_string(),
                Err(_) => String::new(),
            };
            let cost: f64 = c.cost.unwrap_or(0.0) + h.cost.unwrap_or(0.0);
            let cost_s = if c.cost.is_some() || h.cost.is_some() {
                format!(", cost {:.2}", cost)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "{} cutter '{}' in holder '{}' — {}projection {}mm, L/D {:.1}, deflection {}mm{}{}{}\n",
                rank, c.name, h.name,
                if per_row_reach { format!("reach {}mm, ", f3(*reach)) } else { String::new() },
                f3(*projection), ld, f3(*defl), rpm_s, cost_s,
                if flags.is_empty() { String::new() } else { format!("  [{}]", flags.join("; ")) }
            ));
            // The catalog's only legal exit: the text an author reviews and
            // declares, so the certificates gauge a DECLARATION rather than a
            // catalogue. Emitted per row, not just for the leader, because a
            // tie is decided by the reader and they need both lines to take.
            out.push_str(&format!(
                "       {}\n",
                tool_line(c, Placement::InHolder { holder: h, projection: *projection })
            ));
        }
        if tie_count > 1 {
            out.push_str(&format!(
                "  the top {} are within {:.0}% of the leader — a tie, decided by the tiebreaker data above (L/D, cost), not by this ranking\n",
                tie_count, (TIE_FACTOR - 1.0) * 100.0
            ));
        }
        // The disagreement this note used to report is SETTLED (`DECISIONS.md`
        // → "the fifth certificate and the corridor…"): the certificate reads
        // this same corridor now and withdraws where the room is there. What
        // survives is the one thing a reader still has to do about it — the
        // certificate can only measure a nose the PROGRAM declares, so a row
        // taken as a bare `stickout=` is a row it must keep rejecting.
        if fits.iter().any(|row| row.5) {
            out.push_str("  NOTE a row that says `nose enters` proposes a stickout SHORTER than its reach. The reach certificate does not reject that on sight any more — it reads this same corridor and withdraws where the room is measurable — but it can only measure a nose the PROGRAM declares, so carry the holder fields into the tool line beside the stickout (`nacs --tool-line` writes them), or the envelope claim stands\n");
        }
    }
    if !infeasible.is_empty() {
        out.push_str("\ninfeasible:\n");
        for (ci, hi, why) in &infeasible {
            out.push_str(&format!(
                "  - cutter '{}' in holder '{}': {}\n",
                catalog.cutters[*ci].name, catalog.holders[*hi].name, why
            ));
        }
    }
    Ok(out)
}

// ---- the clipboard (V4) ------------------------------------------------------
//
// V4 decided the library fork as CLIPBOARD (`DECISIONS.md` → "V4, tool and
// holder libraries…", fork 1A): a library GENERATES the same `tool` line a
// human would type, and the program keeps it. Not because a program must be
// self-contained — the language already has three blessed external file
// references — but because **an error has to be reproducible from the
// artifact under review**. The certificates reject programs *using tool
// numbers*, so a gauged number that could change outside the diff would
// weaken the claim exactly where this project spends its effort.
//
// Fork 4A picked the invocation: a CLI mode, no grammar change at all. The
// `.part` language gains NOTHING here, which is the strongest available form
// of "the CLI is sufficient" — and the emission stays `tool_line`'s, the one
// home the report already prints from.

/// The `--tool-line` mode's arguments: a catalog path, a cutter name, then
/// optional `holder=<name>` and `stickout=<mm>`. Split out from `main.rs` for
/// the same reason `report_from_args` is — the errors are the interesting
/// part and they should be testable without a shell.
pub fn tool_line_from_args(args: &[String]) -> Result<String, String> {
    const USAGE: &str = "usage: nacs --tool-line <catalog> <cutter> [holder=<name>] [stickout=<mm>]";
    let (Some(catalog_path), Some(cutter_name)) = (args.first(), args.get(1)) else {
        return Err(USAGE.to_string());
    };
    if cutter_name.contains('=') {
        // A key=value in the cutter slot means the name was left out; blaming
        // "no such cutter 'holder=x'" would send the reader to their catalog
        // for a command-line mistake.
        return Err(format!("expected a cutter name, got the input '{}'\n{}", cutter_name, USAGE));
    }
    let catalog = read_catalog(catalog_path)?;

    let mut holder_name: Option<String> = None;
    let mut stickout: Option<f64> = None;
    for a in &args[2..] {
        let Some((k, v)) = a.split_once('=') else {
            return Err(format!("expected key=value, got '{}'\n{}", a, USAGE));
        };
        match k {
            "holder" => {
                if v.trim().is_empty() {
                    return Err("holder= needs a name".to_string());
                }
                holder_name = Some(v.trim().to_string());
            }
            "stickout" => {
                let s: f64 = v.parse().map_err(|_| format!("stickout value '{}' is not a number", v))?;
                if s <= 0.0 {
                    return Err(format!("stickout ({}) must be positive — it is how far the tool stands out of the holder", s));
                }
                stickout = Some(s);
            }
            other => return Err(format!("unknown input '{}' (expected holder/stickout)", other)),
        }
    }

    // A name that is not on the wall names the wall's contents back: the
    // report's own law that a search space which silently shrinks reads as
    // coverage it does not have, applied to a lookup of one.
    let named = |kind: &str, want: &str, have: Vec<&str>| {
        format!("no {} named '{}' in '{}' — the catalog holds: {}", kind, want, catalog_path, have.join(", "))
    };
    let c = catalog
        .cutters
        .iter()
        .find(|c| c.name == *cutter_name)
        .ok_or_else(|| named("cutter", cutter_name, catalog.cutters.iter().map(|c| c.name.as_str()).collect()))?;

    // The stickout the caller HANDS us gets the same band the optimizer
    // derives inside — one home (`projection_band`), so the clipboard cannot
    // propose an assembly the report would have called infeasible.
    if let Some(s) = stickout {
        let (floor, ceiling) = projection_band(c);
        if s < floor {
            return Err(format!(
                "stickout {:.3}mm would bury {:.3}mm of cutting length in the holder — cutter '{}' has {:.3}mm of flutes{} that must stand clear",
                s, floor - s, c.name, c.flute_len,
                if c.neck_len.is_some() { format!(" plus a {:.3}mm neck", c.neck_len.unwrap_or(0.0)) } else { String::new() }
            ));
        }
        if s > ceiling {
            return Err(format!(
                "stickout {:.3}mm leaves under {:.1}x{:.3}mm of shank in the holder — cutter '{}' is {:.3}mm overall, so {:.3}mm is the most it can stand out",
                s, MIN_GRIP_SHANK_DIAS, c.shank_dia, c.name, c.overall_len, ceiling
            ));
        }
    }

    let placement = match holder_name {
        None => Placement::Loose { stickout },
        Some(name) => {
            let h = catalog
                .holders
                .iter()
                .find(|h| h.name == name)
                .ok_or_else(|| named("holder", &name, catalog.holders.iter().map(|h| h.name.as_str()).collect()))?;
            // The grip range, reused from `judge` rather than restated: a
            // holder that cannot hold this shank is not a candidate at any
            // projection, so proposing the line would be text about an
            // assembly that cannot exist.
            if let Some((lo, hi)) = h.grip {
                if c.shank_dia < lo || c.shank_dia > hi {
                    return Err(format!(
                        "holder '{}' cannot hold cutter '{}': the {:.3}mm shank is outside its {:.3}..{:.3}mm grip range",
                        h.name, c.name, c.shank_dia, lo, hi
                    ));
                }
            }
            // The parser's own law, rejected here where the reader can still
            // do something about it: holder geometry needs the stickout plane
            // its nose face sits on, and this mode has no job to derive one
            // from — deriving is `--assembly-report`'s whole purpose.
            let Some(projection) = stickout else {
                return Err(format!(
                    "holder '{}' needs a stickout= — the nose face sits at the stickout plane, so a holder with no projection is a body nothing can place. Pass stickout=<mm>, or ask --assembly-report to derive one from a job",
                    h.name
                ));
            };
            Placement::InHolder { holder: h, projection }
        }
    };
    Ok(format!("{}\n", tool_line(c, placement)))
}

/// The CLI arguments after `--assembly-report`: a catalog path, then
/// `key=value` inputs. Split out so the shell in `main.rs` stays thin and the
/// errors are testable.
/// One report request, from either door — the typed shape both the CLI's
/// `key=value` argv and the MCP verb's JSON arguments build
/// (`DECISIONS.md` → "`--assembly-report` as the sixth verb…", fork 5A).
///
/// **Why the split exists.** v1 had one door and read its own file; the
/// verb is the second door, and a second file-reading path would have been a
/// second answer to "what does a path mean" — the drift `parse::load_project`
/// was declared to prevent and had already suffered. So the part arrives here
/// ALREADY LOADED, and everything downstream of the load is one road.
pub struct ReportRequest {
    /// The declared job: a loaded project, the feature in it to reach into,
    /// and how the caller spelled the path (the header echoes provenance, so
    /// a derived input whose source the reader cannot see is a number they
    /// cannot check).
    pub declared: Option<DeclaredJob>,
    pub reach: Option<f64>,
    pub opening: Option<f64>,
    pub depth: Option<f64>,
    pub clearance: f64,
    pub load: f64,
    pub material: Option<String>,
    pub shop_feeds: Option<crate::parse::FeedsTable>,
}

/// The STEP slice's input, with the reading already done.
pub struct DeclaredJob {
    pub part: crate::parse::Part,
    pub feature: String,
    /// The path as the caller spelled it — for the header only; nothing
    /// resolves from it a second time.
    pub shown_as: String,
}

impl Default for ReportRequest {
    fn default() -> Self {
        ReportRequest {
            declared: None,
            reach: None,
            opening: None,
            depth: None,
            clearance: DEFAULT_CLEARANCE_MM,
            load: DEFAULT_LOAD_N,
            material: None,
            shop_feeds: None,
        }
    }
}

/// The report one request states — **and every error about the INPUTS**, so
/// the two doors cannot answer differently from each other. The mutual
/// exclusions live here rather than in either caller for the same reason the
/// arithmetic does: a second copy of "one fact in two places" would be the bug
/// it describes.
pub fn report_for(catalog: &Catalog, req: ReportRequest) -> Result<String, String> {
    let ReportRequest {
        declared,
        mut reach,
        opening,
        mut depth,
        clearance,
        load,
        material,
        shop_feeds,
    } = req;

    // The STEP slice: the three job numbers READ off a declared feature
    // instead of typed (`DECISIONS.md` → "the optimizer's STEP slice…").
    let mut derived_from: Option<String> = None;
    let mut composed: Vec<String> = Vec::new();
    let mut corridor: Option<Corridor> = None;
    if let Some(d) = declared {
        // Two records describing one fact is this codebase's oldest error
        // shape: if the numbers are in the part, passing them too gives the
        // reader two places for them to disagree.
        let typed: Vec<&str> = [("reach", reach), ("opening", opening), ("depth", depth)]
            .iter()
            .filter(|(_, v)| v.is_some())
            .map(|(k, _)| *k)
            .collect();
        if !typed.is_empty() {
            return Err(format!(
                "the part reads {} off the declaration, so passing {} too would put one fact in two places. Pass the part, or pass the numbers",
                "reach/opening/depth", typed.join(" and ")
            ));
        }
        let feature = d.part.features.get(&d.feature).ok_or_else(|| {
            let mut held: Vec<String> =
                d.part.features.iter().map(|(n, f)| format!("{} ({})", n, f.kind())).collect();
            held.sort();
            format!(
                "no feature named '{}' in '{}' — the part declares: {}",
                d.feature, d.shown_as,
                if held.is_empty() { "nothing".to_string() } else { held.join(", ") }
            )
        })?;
        // `Opened::Declared`: this door has a PART and no order — a landed
        // proposal often declares no operations at all — so it reads every
        // coaxial neighbour the part declares and the header states the
        // condition (`DECISIONS.md` → "the multi-feature cavity…", fork 4A).
        let cavity = corridor_of(&d.part, feature, Opened::Declared)
            .map_err(|why| format!("feature '{}' in '{}': {}", d.feature, d.shown_as, why))?;
        derived_from = Some(format!(
            "feature '{}' ({}) in {}",
            d.feature, feature.kind(), d.shown_as
        ));
        reach = Some(cavity.reach);
        depth = Some(cavity.depth);
        composed = cavity.composed;
        corridor = Some(cavity.corridor);
    }

    let missing = |what: &str| {
        format!("missing required input '{}=<mm>' (or pass part=<file.part> feature=<name> and let the declaration state it)", what)
    };
    // A table with no material to look up in it is a table nobody reads — the
    // silence `reject_unknown_keys` exists to make unshippable, said here
    // where the reader can still fix the command line.
    if shop_feeds.is_some() && material.is_none() {
        return Err("feeds= was given but material= was not — a feeds table is looked up BY material, so without one the table is read by nothing".to_string());
    }
    let job = Job {
        material,
        shop_feeds,
        derived_from,
        composed,
        reach: reach.ok_or_else(|| missing("reach"))?,
        // Both doors build the SAME model: a typed `opening=` is the cavity's
        // crossing said by hand, so it becomes the one-band corridor rather
        // than a second kind of input (`DECISIONS.md` → "the optimizer's
        // clearance profile…", fork 4A).
        corridor: match corridor {
            Some(c) => c,
            None => Corridor::straight(opening.ok_or_else(|| missing("opening"))? / 2.0),
        },
        depth: depth.ok_or_else(|| missing("depth"))?,
        clearance,
        load,
    };
    assembly_report(catalog, &job)
}

/// The CLI door: `nacs --assembly-report <catalog> key=value...`. Argv in, a
/// `ReportRequest` out, and not one number of arithmetic — everything past the
/// spelling is `report_for`'s.
pub fn report_from_args(args: &[String]) -> Result<String, String> {
    let Some(catalog_path) = args.first() else {
        return Err("usage: nacs --assembly-report <catalog> (reach=<mm> opening=<mm> depth=<mm> | part=<file.part> feature=<name>) [clearance=<mm>] [load=<N>] [material=<name>] [feeds=<file>]".to_string());
    };
    let catalog = read_catalog(catalog_path)?;
    let mut req = ReportRequest::default();
    let mut part_path: Option<String> = None;
    let mut feature_name: Option<String> = None;
    for a in &args[1..] {
        let Some((k, v)) = a.split_once('=') else {
            return Err(format!("expected key=value, got '{}'", a));
        };
        // The two inputs that are NAMES, not lengths — handled before the
        // numeric parse so neither has to look like a number.
        if k == "material" {
            if v.trim().is_empty() {
                return Err("material= needs a name".to_string());
            }
            req.material = Some(v.trim().to_string());
            continue;
        }
        // The declared part and the feature in it to reach into — the STEP
        // slice's inputs (fork 1A). Names, not lengths, so they are handled
        // before the numeric parse like `material` above.
        if k == "part" || k == "feature" {
            let v = v.trim();
            if v.is_empty() {
                return Err(format!("{k}= needs a {}", if k == "part" { "path" } else { "name" }));
            }
            if k == "part" { part_path = Some(v.to_string()) } else { feature_name = Some(v.to_string()) }
            continue;
        }
        if k == "feeds" {
            // The shop's own table, the `.cam` language's `feeds_table "file"`
            // as a mode input. Read and PARSED here rather than carried as a
            // path, so a malformed table is an error with the table's own
            // line number instead of a surprise at the first rpm.
            let path = v.trim();
            if path.is_empty() {
                return Err("feeds= needs a path".to_string());
            }
            req.shop_feeds = Some(read_feeds(path)?);
            continue;
        }
        let v: f64 = v.parse().map_err(|_| format!("'{}' value '{}' is not a number", k, v))?;
        match k {
            "reach" => req.reach = Some(v),
            "opening" => req.opening = Some(v),
            "depth" => req.depth = Some(v),
            "clearance" => req.clearance = v,
            "load" => req.load = v,
            other => return Err(format!("unknown input '{}' (expected reach/opening/depth/part/feature/clearance/load/material/feeds)", other)),
        }
    }
    match (part_path, feature_name) {
        (None, None) => {}
        (Some(_), None) | (None, Some(_)) => {
            return Err("part= and feature= come together — a part with no feature named says which file to read and not what to reach into, and a feature with no part says the opposite".to_string());
        }
        (Some(path), Some(name)) => {
            req.declared = Some(DeclaredJob { part: load_declared(&path)?, feature: name, shown_as: path });
        }
    }
    report_for(&catalog, req)
}

/// The shop's own feeds table, read and PARSED from a path — so a malformed
/// table is an error carrying the table's own line number instead of a
/// surprise at the first rpm. One home, because both doors take the path.
pub fn read_feeds(path: &str) -> Result<crate::parse::FeedsTable, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read feeds table '{}': {}", path, e))?;
    crate::parse::parse_feeds(&text).map_err(|e| format!("feeds table '{}': {}", path, e))
}

/// A path becomes the project it names, through the one home
/// (`parse::load_project`) — **which this input did not use until 2026-08-20**,
/// and so could not read a two-file project at all. Both error classes flatten
/// to prose here: this mode has no check result document to put a stage in.
fn load_declared(path: &str) -> Result<crate::parse::Part, String> {
    crate::parse::load_project(path).map_err(|e| match e {
        crate::parse::LoadError::Io(m) => m,
        crate::parse::LoadError::Parse(m) => format!("part '{}': {}", path, m),
    })
}

// ---- tests ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const CAT: &str = "\
# a small shop wall
cutter eighth
dia 3.175
flute_len 12.7
shank_dia 3.175
overall_len 38.1
modulus 600
cost 18

cutter half
dia 12.7
flute_len 25.4
shank_dia 12.7
overall_len 76.2
modulus 600
cost 45

holder stubby
nose_dia 40.0
nose_len 20.0
body_dia 63.0
gauge_len 60.0
compliance 2.0
cost 120

holder slim
nose_dia 14.0
nose_len 45.0
body_dia 50.0
gauge_len 90.0
compliance 3.5
cost 260
";

    /// A one-feature program, through the real parser — this mode measures
    /// every depth down from the STOCK's top face, so a fixture without a
    /// stock line would be testing a cavity that is not standing anywhere.
    fn part_with(lines: &str) -> crate::parse::Part {
        let src = format!(
            "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
             stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n{}\n",
            lines
        );
        crate::parse::parse(&src).unwrap_or_else(|e| panic!("the test program must parse: {e}"))
    }

    fn job() -> Job {
        Job { material: None, shop_feeds: None, derived_from: None, composed: Vec::new(), reach: 30.0, corridor: Corridor::straight(10.0), depth: 35.0, clearance: DEFAULT_CLEARANCE_MM, load: DEFAULT_LOAD_N }
    }

    #[test]
    fn the_catalog_parses_and_its_refusals_hold() {
        let cat = parse_catalog(CAT).unwrap();
        assert_eq!(cat.cutters.len(), 2);
        assert_eq!(cat.holders.len(), 2);
        assert_eq!(cat.cutters[0].name, "eighth");
        assert_eq!(cat.holders[1].compliance, 3.5);

        let bad = [
            ("dia 3.0\n", "before any"),
            ("cutter a\ndia 3.0\nwat 5\nflute_len 10\nshank_dia 3\noverall_len 40\nmodulus 600\nholder h\nnose_dia 10\nnose_len 10\nbody_dia 40\ngauge_len 50\ncompliance 2\n", "unknown cutter field 'wat'"),
            ("cutter a\ndia 3.0\nflute_len 10\nshank_dia 3\noverall_len 40\nmodulus 600\ncutter a\ndia 4\nflute_len 10\nshank_dia 4\noverall_len 40\nmodulus 600\nholder h\nnose_dia 10\nnose_len 10\nbody_dia 40\ngauge_len 50\ncompliance 2\n", "duplicate record name"),
            ("cutter a\ndia 3.0\nflute_len 10\nshank_dia 3\noverall_len 40\nneck_dia 2.5\nmodulus 600\nholder h\nnose_dia 10\nnose_len 10\nbody_dia 40\ngauge_len 50\ncompliance 2\n", "half a neck"),
            ("cutter a\ndia 3.0\nflute_len 40\nshank_dia 3\noverall_len 40\nmodulus 600\nholder h\nnose_dia 10\nnose_len 10\nbody_dia 40\ngauge_len 50\ncompliance 2\n", "no shank left to grip"),
            ("cutter a\ndia 3.0\nflute_len 10\nshank_dia 3\noverall_len 40\nmodulus 600\nholder h\nnose_dia 50\nnose_len 10\nbody_dia 40\ngauge_len 50\ncompliance 2\n", "narrow front section"),
            ("cutter a\ndia 3.0\nflute_len 10\nshank_dia 3\noverall_len 40\nmodulus 600\n", "at least one cutter and one holder"),
            ("cutter a\ndia -3.0\nflute_len 10\nshank_dia 3\noverall_len 40\nmodulus 600\nholder h\nnose_dia 10\nnose_len 10\nbody_dia 40\ngauge_len 50\ncompliance 2\n", "must be positive"),
        ];
        for (src, needle) in bad {
            let err = parse_catalog(src).unwrap_err();
            assert!(err.contains(needle), "wanted '{needle}', got: {err}");
        }
    }

    #[test]
    fn the_rpm_column_reads_the_shipped_table_on_both_axes() {
        // V3's acceptance, half one: an RPM from tables/starter.feeds, on the
        // SHIPPED sample catalog. Checked against the identity by hand rather
        // than against itself — 6061 x Endmill is 150 m/min, so a 9.525mm
        // cutter turns 1000 x 150 / (pi x 9.525) = 5012.75.
        let cat = read_catalog("tests/fixtures/sample.catalog").expect("shipped catalog");
        let job = Job {
            material: Some("6061".into()),
            shop_feeds: None,
            derived_from: None,
            composed: Vec::new(),
            reach: 45.0, corridor: Corridor::straight(15.0), depth: 40.0,
            clearance: DEFAULT_CLEARANCE_MM, load: DEFAULT_LOAD_N,
        };
        let three_eighths = cat.cutters.iter().find(|c| c.name == "3/8-carbide").unwrap();
        let rpm = cutter_rpm(three_eighths, &job).expect("6061 x Endmill ships");
        assert!((rpm - 5012.754).abs() < 1e-2, "rpm was {rpm}");

        let report = assembly_report(&cat, &job).expect("report");
        assert!(report.contains("5012.754 rpm"), "the row must carry it: {report}");
        assert!(report.contains("rpm: from the shipped feeds table, 6061"), "{report}");
        assert!(!report.contains("rpm: omitted"), "the omission line must go: {report}");

        // The TYPE axis is real, not decorative: the same cutter declared a
        // drill looks up a different row and turns a different speed.
        let mut as_drill = three_eighths.clone();
        as_drill.tool_type = "Drill".into();
        let drill_rpm = cutter_rpm(&as_drill, &job).expect("6061 x Drill ships");
        assert!((drill_rpm - rpm).abs() > 1.0,
            "a drill must not inherit an end mill's surface speed ({drill_rpm} vs {rpm})");

        // And with no material the honest omission stands, shortened to the
        // half still true — K3 built the engine, so only the input is missing.
        let bare = Job { material: None, ..job.clone() };
        let r = assembly_report(&cat, &bare).expect("report");
        assert!(r.contains("rpm: omitted — no material input"), "{r}");
        assert!(!r.contains("no feeds engine"), "that half stopped being true: {r}");
        assert!(!r.contains(" rpm,") && !r.contains(" rpm\n"), "no column without an input: {r}");
    }

    #[test]
    fn the_shop_table_outranks_the_shipped_one_and_the_header_says_which_was_read() {
        // V3's named remainder, paid: this mode had no `.cam` program and so
        // no `feeds_table "file"` directive, and read the shipped table only.
        // K3's founding law is that a shop's proven numbers outrank ours —
        // which is exactly why shipping timid starter values is free — so the
        // report has to be able to read theirs.
        let cat = read_catalog("tests/fixtures/sample.catalog").expect("shipped catalog");
        let shop_text = std::fs::read_to_string("tests/fixtures/shop.feeds").expect("shop table");
        let shop = crate::parse::parse_feeds(&shop_text).expect("the shop table parses");
        let base = Job {
            material: Some("6061".into()),
            shop_feeds: None,
            derived_from: None,
            composed: Vec::new(),
            reach: 45.0, corridor: Corridor::straight(15.0), depth: 40.0,
            clearance: DEFAULT_CLEARANCE_MM, load: DEFAULT_LOAD_N,
        };
        let c = cat.cutters.iter().find(|c| c.name == "3/8-carbide").expect("shipped cutter");

        // HALF ONE — SHADOWING. The shipped row says 150 m/min, the shop's
        // says 260, and the shop's is what the column turns. Both numbers are
        // the identity's, restated from its own home rather than pasted.
        let shipped_rpm = cutter_rpm(c, &base).expect("6061 x Endmill ships");
        let shop_job = Job { shop_feeds: Some(shop.clone()), ..base.clone() };
        let shop_rpm = cutter_rpm(c, &shop_job).expect("the shop row answers");
        assert!((shipped_rpm - crate::parse::derive_spindle_rpm(150.0, c.dia)).abs() < 1e-9);
        assert!((shop_rpm - crate::parse::derive_spindle_rpm(260.0, c.dia)).abs() < 1e-9);
        assert!(shop_rpm > shipped_rpm, "the shop's proven number must win: {shop_rpm} vs {shipped_rpm}");

        // HALF TWO — a material the shipped table does not hold AT ALL. This
        // is the other reason a shop table exists: a miss rejects by name
        // rather than guessing, and the shop is where the answer can come
        // from. Without the table the row cannot price itself; with it, it can.
        let exotic = Job { material: Some("7075".into()), ..base.clone() };
        assert!(cutter_rpm(c, &exotic).is_err(), "the shipped table must not invent 7075");
        let exotic_shop = Job { shop_feeds: Some(shop.clone()), ..exotic.clone() };
        assert!(cutter_rpm(c, &exotic_shop).is_ok(), "the shop table holds it");

        // And a lookup the shop DOESN'T shadow still falls through, which is
        // what "shipped table behind it" in the header means.
        let mut as_drill = c.clone();
        as_drill.tool_type = "Drill".into();
        let fell_through = cutter_rpm(&as_drill, &shop_job).expect("6061 x Drill still ships");
        assert!((fell_through - crate::parse::derive_spindle_rpm(90.0, c.dia)).abs() < 1e-9,
            "an unshadowed row is the shipped one, unchanged");

        // THE HEADER NAMES ITS SOURCE, because a reader who cannot tell the
        // shipped starting points from their own shop's numbers cannot tell
        // how much to trust the column.
        let plain = assembly_report(&cat, &base).expect("report");
        assert!(plain.contains("rpm: from the shipped feeds table, 6061"), "{plain}");
        let shopped = assembly_report(&cat, &shop_job).expect("report");
        assert!(shopped.contains("rpm: from the shop feeds table (2 rows"), "{shopped}");
        assert!(shopped.contains("shipped table behind it"), "{shopped}");
    }

    /// The stack from the shipped corpus, read whole (`DECISIONS.md` → "the
    /// multi-feature cavity…"). One cavity is DECLARED as two coaxial bores,
    /// because two ops with two strategies cut it — and until this landed, the
    /// mode read whichever one was named and called the other one nothing.
    fn stack() -> crate::parse::Part {
        part_with(
            "tool T5 Endmill(dia=10.0)\n\
             feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
             feature Core CylinderBore(dia=30.0, extent=14.0, x=50.0, y=50.0)\n\
             run Clear(tool=T5, profile=Bore, stepover=4.0)\n\
             run Clear(tool=T5, profile=Core, stepover=4.0)",
        )
    }

    #[test]
    fn a_stack_of_coaxial_bores_reads_as_one_cavity_and_the_neighbour_is_only_room() {
        let p = stack();
        let c = corridor_of(&p, &p.features["Core"], Opened::Declared).expect("a bore is a cavity");
        assert_eq!(
            c.corridor,
            Corridor::Steps(vec![(0.0, 30.0), (12.0, 15.0)]),
            "60mm across to the counterbore's floor, then the core's own 30"
        );
        assert_eq!(c.composed, vec!["Bore".to_string()], "and the header must be able to name it");

        // **A neighbour is ROOM, never WORK** (fork 3A). The job is still to
        // cut the feature that was named, so both numbers are the CORE's — the
        // wide bore above it does not make this job shallower, and the fact
        // that they are equal for a plain bore is the reading that must survive.
        assert_eq!((c.reach, c.depth), (14.0, 14.0));

        // The same law from the other end: asking about the WIDE bore composes
        // nothing at all. The core is deeper and NARROWER, so it widens no
        // depth the wide bore's own job reaches — and a neighbour that widened
        // nothing is not named, because the header states what governs the
        // rows rather than what was looked at.
        let c = corridor_of(&p, &p.features["Bore"], Opened::Declared).expect("a cavity");
        assert_eq!(c.corridor, Corridor::Steps(vec![(0.0, 30.0)]));
        assert!(c.composed.is_empty(), "a deeper narrow neighbour is not room");
        assert_eq!((c.reach, c.depth), (12.0, 12.0));
    }

    #[test]
    fn the_program_order_decides_what_a_certificate_may_count_as_open() {
        // Fork 4A, at the level it lives. The DECLARATION is the same part in
        // both readings below; what changes is which question is being asked.
        let p = stack();
        let core = &p.features["Core"];

        // The wide bore is cleared at op 0, so by op 1 its room is REAL.
        let c = corridor_of(&p, core, Opened::Before(1)).expect("a cavity");
        assert_eq!(c.corridor, Corridor::Steps(vec![(0.0, 30.0), (12.0, 15.0)]));
        assert_eq!(c.composed, vec!["Bore".to_string()]);

        // At op 0 nothing has been opened yet, and the counterbore that is
        // going to be there is not room a tool can descend through today. This
        // is the whole of what stops a gating check from silencing a finding
        // on material the program has not made: the corpus's own stepped
        // cavity (`cylinder_bore_rest_flutes.cam`) cuts the CORE first.
        let c = corridor_of(&p, core, Opened::Before(0)).expect("a cavity");
        assert_eq!(c.corridor, Corridor::Steps(vec![(0.0, 15.0)]), "the core alone, as before");
        assert!(c.composed.is_empty());

        // And a neighbour nothing ever cuts is never open, however many ops
        // run — a declared feature is a statement about the finished part, not
        // about the material standing while it is made.
        let unmachined = part_with(
            "tool T5 Endmill(dia=10.0)\n\
             feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
             feature Core CylinderBore(dia=30.0, extent=14.0, x=50.0, y=50.0)\n\
             run Clear(tool=T5, profile=Core, stepover=4.0)",
        );
        let c = corridor_of(&unmachined, &unmachined.features["Core"], Opened::Before(1))
            .expect("a cavity");
        assert_eq!(c.corridor, Corridor::Steps(vec![(0.0, 15.0)]));
    }

    #[test]
    fn the_report_names_the_neighbours_it_composed_and_the_condition_it_composed_under() {
        // The corpus half of the acceptance, on the one stepped cavity the
        // tree actually ships (fork 6A). Before this landed, asking about
        // `Core` reported a 30mm hole 14mm deep and said nothing about the
        // 60mm counterbore standing open above it.
        let out = report_from_args(&[
            "tests/fixtures/sample.catalog".to_string(),
            "part=cylinder_bore_rest_flutes.cam/main.part".to_string(),
            "feature=Core".to_string(),
        ])
        .expect("the shipped catalog reports on the shipped stack");
        assert!(
            out.contains("corridor: 60.000mm across to 12.000mm deep, then 30.000mm across below that"),
            "the staircase the two features make together: {out}"
        );
        assert!(out.contains("composed with 'Bore'"), "and which feature it read: {out}");
        assert!(
            out.contains("once it is open, and not before"),
            "with the condition, which is also the advice — that cut comes first: {out}"
        );
        // The job's own numbers do not move: the crossing the first line
        // prints is the width at the depth the WORK reaches, which is the
        // core's, and a neighbour is room rather than work.
        assert!(
            out.contains("assembly report — reach 14.000mm through a 30.000mm opening, 14.000mm deep"),
            "the named feature still states the job: {out}"
        );
    }

    /// THE ANTI-DRIFT PIN, and the property that says the composition is right
    /// rather than merely different: **a counterbore IS a stack of two coaxial
    /// bores in one feature**, so the same cavity spelled either way must
    /// report the same thing. It is how the DRAFT measured the answer before
    /// the code existed, and it is how the two spellings are kept from drifting
    /// apart now that both roads reach it.
    #[test]
    fn one_cavity_spelled_two_ways_reports_the_same_assembly() {
        let cat = read_catalog("tests/fixtures/sample.catalog").expect("the shipped catalog");
        let report_on = |part: crate::parse::Part, feature: &str| {
            let req = ReportRequest {
                declared: Some(DeclaredJob {
                    part,
                    feature: feature.to_string(),
                    shown_as: "<test>".to_string(),
                }),
                ..ReportRequest::default()
            };
            report_for(&cat, req).expect("the report comes out")
        };

        let composed = report_on(stack(), "Core");
        let one_feature = report_on(
            part_with(
                "feature Stack CylinderBore(dia=30.0, extent=14.0, x=50.0, y=50.0, cbore_dia=60.0, cbore_depth=12.0)",
            ),
            "Stack",
        );

        // Everything but the two lines that are ABOUT the spelling: which
        // feature was read, and which neighbours were folded into it.
        let rows = |out: &str| -> Vec<String> {
            out.lines()
                .filter(|l| !l.contains("READ from") && !l.contains("composed with"))
                .map(|l| l.to_string())
                .collect()
        };
        assert_eq!(
            rows(&composed),
            rows(&one_feature),
            "two spellings of one cavity, two different reports"
        );
    }

    #[test]
    fn a_neighbour_left_with_skin_widens_nothing_until_a_pass_leaves_none() {
        // Fork 4A's sub-fork. A roughing pass leaves material standing on the
        // wall it was aimed at, so its declared width is not the width that is
        // there — and rather than modelling the skin, the neighbour simply
        // does not count until an op leaves none.
        let roughed = part_with(
            "tool T5 Endmill(dia=10.0)\n\
             feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
             feature Core CylinderBore(dia=30.0, extent=14.0, x=50.0, y=50.0)\n\
             run Clear(tool=T5, profile=Bore, stepover=4.0, stock_to_leave=0.5)\n\
             run Clear(tool=T5, profile=Core, stepover=4.0)",
        );
        let c = corridor_of(&roughed, &roughed.features["Core"], Opened::Before(1)).expect("a cavity");
        assert_eq!(
            c.corridor,
            Corridor::Steps(vec![(0.0, 15.0)]),
            "0.5mm of skin is 0.5mm this reader cannot account for, so the band does not count"
        );

        // Rough-then-finish satisfies it at the finish, which is the ordinary
        // shape of the work and the reason the rule costs nothing real.
        let finished = part_with(
            "tool T5 Endmill(dia=10.0)\n\
             feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
             feature Core CylinderBore(dia=30.0, extent=14.0, x=50.0, y=50.0)\n\
             run Clear(tool=T5, profile=Bore, stepover=4.0, stock_to_leave=0.5)\n\
             run Side(tool=T5, surface=Bore, stepover=5.0)\n\
             run Clear(tool=T5, profile=Core, stepover=4.0)",
        );
        let c = corridor_of(&finished, &finished.features["Core"], Opened::Before(2)).expect("a cavity");
        assert_eq!(c.corridor, Corridor::Steps(vec![(0.0, 30.0), (12.0, 15.0)]));
        assert_eq!(c.composed, vec!["Bore".to_string()]);
    }

    #[test]
    fn an_island_on_the_axis_refuses_and_a_post_the_bore_runs_through_does_not() {
        // Fork 5A. A boss standing on the cavity's own axis is material inside
        // the mouth — ignoring it is an over-claim, and the excuse for
        // ignoring it (the mode read one feature and could not see it) expires
        // the moment the neighbours are read at all.
        //
        // Phase 26 slice 1: `validate` now refuses this program first — a boss
        // inside a bore is an island the bore cannot host yet — so the corridor's
        // own arm is reached here by standing the post in AFTER the parse. The
        // arm stays: it is this reader's answer, and it must not depend on
        // another gate having run.
        const BORE: &str = "tool T5 Endmill(dia=10.0)\nfeature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)";
        let stood_in = |line: &str, name: &str| {
            let refused = crate::parse::parse(&format!(
                "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\nstock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n{BORE}\n{line}\n"
            ))
            .expect_err("validate refuses an island a bore cannot host");
            assert!(refused.contains(&format!("'{name}'")) && refused.contains("'Bore'"), "both features named: {refused}");
            let mut part = part_with(BORE);
            let alone = part_with(&format!("tool T5 Endmill(dia=10.0)\n{line}"));
            part.features.insert(name.to_string(), alone.features[name].clone());
            part
        };
        let island = stood_in("feature Post CylinderBoss(dia=20.0, extent=12.0, x=50.0, y=50.0, z=-12.0)", "Post");
        let e = corridor_of(&island, &island.features["Bore"], Opened::Declared)
            .expect_err("an island in the mouth is rejected, not measured around");
        assert!(e.contains("'Post'") && e.contains("20.000mm"), "the error must name it: {e}");

        // The legal and common shape passes: a hole down the middle of a post,
        // where the boss SURROUNDS the corridor rather than standing in it.
        let through = part_with(
            "tool T5 Endmill(dia=10.0)\n\
             feature Hole CylinderBore(dia=10.0, extent=12.0, x=50.0, y=50.0)\n\
             feature Post CylinderBoss(dia=40.0, extent=12.0, x=50.0, y=50.0, z=-12.0)",
        );
        let c = corridor_of(&through, &through.features["Hole"], Opened::Declared)
            .expect("a bore through a post is still a bore");
        assert_eq!(c.corridor, Corridor::Steps(vec![(0.0, 5.0)]));

        // A standing form whose own radius at each depth is a contact question
        // is rejected BY KIND rather than measured — the same line fork 2A drew
        // around what may be composed, drawn again around what may be ignored.
        let dome = stood_in("feature Dome Sphere(dia=40.0, x=50.0, y=50.0, z=-12.0)", "Dome");
        let e = corridor_of(&dome, &dome.features["Bore"], Opened::Declared)
            .expect_err("a horn on the axis is not something this reader can descend past");
        assert!(e.contains("'Dome'") && e.contains("Sphere"), "the error must name the kind: {e}");

        // And an off-axis neighbour is not this question at all: two holes in
        // one plate compose nothing, whatever their sizes.
        let apart = part_with(
            "tool T5 Endmill(dia=10.0)\n\
             feature A CylinderBore(dia=60.0, extent=12.0, x=20.0, y=50.0)\n\
             feature B CylinderBore(dia=30.0, extent=14.0, x=70.0, y=50.0)",
        );
        let c = corridor_of(&apart, &apart.features["B"], Opened::Declared).expect("a cavity");
        assert_eq!(c.corridor, Corridor::Steps(vec![(0.0, 15.0)]));
        assert!(c.composed.is_empty());
    }

    #[test]
    fn the_room_formula_holds_its_three_readings_and_v1_is_one_of_them() {
        // `room(z) = r_cav(z) − r_cav(D) + r_tool`, checked at each of the
        // three readings the type's doc names.
        let stepped = Corridor::Steps(vec![(0.0, 8.0), (6.0, 3.0)]);

        // (1) The tool FILLS the hole — a drill, a plunge, a boring pass. The
        // excursion is zero and the room is the cavity's own radius: v1's
        // arithmetic exactly, which is why v1 was never wrong about the case it
        // modelled.
        // The excursion is the argument now — `place` is what works it out
        // from the cavity for a vertical wall, and from the ball's own CL
        // locus for a curved one.
        assert_eq!(stepped.room_at(0.0, 3.0, 0.0), 8.0);
        assert_eq!(stepped.room_at(15.0, 3.0, 0.0), 3.0);

        // (2) A STRAIGHT wall — the room is the cutter's own radius, whatever
        // the crossing is, and whatever shape it has.
        let straight = Corridor::straight(15.0);
        assert!((straight.room_at(2.0, 3.175, 15.0 - 3.175) - 3.175).abs() < EPS);
        assert_eq!(Corridor::Unmeasured { mouth: 0.0 }.room_at(2.0, 3.175, 0.0), 3.175);

        // (3) A STEP — the extra width is what the assembly may spend, and it
        // is the only place a profile earns its existence. A 3.175mm cutter in
        // the 6mm hole stands 1.4125mm off the axis, so the 16mm seat leaves
        // 8 − 1.4125 of room, not 8.
        assert_eq!(stepped.room_at(0.0, 1.5875, 3.0 - 1.5875), 8.0 - 1.4125);

        // And the descent that falls out of it: a body small enough for the
        // seat stops at the seat's floor, one too big never enters, and
        // equality is legal.
        let e = 3.0 - 1.5875;
        assert_eq!(stepped.admits_to(6.0, 1.5875, e, 15.0), 6.0);
        assert_eq!(stepped.admits_to(8.0, 1.5875, e, 15.0), 0.0);
        assert_eq!(stepped.admits_to(8.0 - 1.4125, 1.5875, e, 15.0), 6.0, "equality is legal");
        // Solid above a pocket's mouth is not room, so nothing descends at all.
        assert_eq!(Corridor::Unmeasured { mouth: 4.0 }.admits_to(0.1, 5.0, 0.0, 10.0), 0.0);
    }

    #[test]
    fn the_nose_follows_the_cutter_only_as_far_as_the_cavity_is_wider_than_the_cut() {
        // The descent law where it can actually bite: a deep 12mm hole under a
        // wide 40mm seat, and a slim nose that can live in the seat.
        let cat = parse_catalog(CAT).expect("catalog parses");
        let eighth = &cat.cutters[0]; // dia 3.175, flute 12.7, overall 38.1
        let slim = &cat.holders[1]; // nose 14.0 x 45.0, body 50.0
        let seated = Job {
            material: None, shop_feeds: None, derived_from: None, composed: Vec::new(),
            reach: 30.0,
            corridor: Corridor::Steps(vec![(0.0, 20.0), (10.0, 6.0)]),
            depth: 30.0, clearance: DEFAULT_CLEARANCE_MM, load: DEFAULT_LOAD_N,
        };
        // Room in the seat: 20 − (6 − 1.5875) = 15.5875, against a 7.5mm nose
        // (7.0 + clearance). It enters, and stops at the seat's floor —
        // 10.000mm down, so 20.000mm of the reach must project.
        let (projection, enters, _) = min_projection(eighth, slim, &seated).expect("it fits");
        assert!(enters);
        assert_eq!(projection, 20.0);

        // The same hole with no seat: the wall is straight, so the room is the
        // cutter's 1.5875mm and a 7.0mm nose has no business in there. The full
        // reach projects — v1's "nose blocked", reached generally.
        let straight = Job { corridor: Corridor::straight(6.0), ..seated.clone() };
        let (projection, enters, _) = min_projection(eighth, slim, &straight).expect("it fits");
        assert!(!enters);
        assert_eq!(projection, 30.0);

        // And v1's own reading survives inside the same function: a tool that
        // FILLS the hole faces no excursion, so the nose enters exactly as
        // `nose_dia + 2·clearance <= opening` always said — and is stopped by
        // the BODY behind it, which is the comparison v1 spelled as `nose_len`.
        let half = &cat.cutters[1]; // dia 12.7, so it fills a 12.7mm hole
        let short_nose = Holder {
            name: "short-nose".to_string(), nose_dia: 11.0, nose_len: 10.0, body_dia: 50.0,
            gauge_len: 60.0, compliance: 2.0, grip: None, cost: None,
        };
        let filled = Job {
            reach: 40.0, corridor: Corridor::straight(6.35), depth: 40.0, ..seated.clone()
        };
        let (projection, enters, _) = min_projection(half, &short_nose, &filled).expect("it fits");
        assert!(enters, "11.0 + 2x0.5 <= 12.7: v1's comparison, unchanged");
        assert_eq!(projection, 30.0, "reach 40 less the nose's own 10mm of descent");
    }

    #[test]
    fn the_part_input_refuses_every_way_it_can_be_asked_wrong() {
        let cat = "tests/fixtures/sample.catalog";
        let prog = "cylinder_bore.cam/main.part";
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for (args, needle) in [
            // The pairing: neither half says anything useful alone.
            (a(&[cat, &format!("part={prog}")]), "come together"),
            (a(&[cat, "feature=Bore"]), "come together"),
            (a(&[cat, "part=", "feature=Bore"]), "part= needs a path"),
            (a(&[cat, &format!("part={prog}"), "feature="]), "feature= needs a name"),
            // Two records describing one fact — this codebase's oldest shape.
            (a(&[cat, &format!("part={prog}"), "feature=Bore", "reach=40"]), "one fact in two places"),
            (a(&[cat, &format!("part={prog}"), "feature=Bore", "depth=5", "opening=9"]), "opening and depth"),
            // The path, and what is in it. The unreadable-path message is the
            // LOADER's now, the same words `nacs nope.part` prints — one home
            // for resolving a path is one message for failing to.
            (a(&[cat, "part=nope.part", "feature=Bore"]), "cannot read nope.part"),
            (a(&[cat, &format!("part={cat}"), "feature=Bore"]), "part 'tests/fixtures/sample.catalog'"),
            (a(&[cat, &format!("part={prog}"), "feature=Absent"]), "no feature named 'Absent'"),
        ] {
            let e = report_from_args(&args).unwrap_err();
            assert!(e.contains(needle), "wanted '{needle}', got: {e}");
        }
        // A missing name lists what the part DOES declare, kind and all — the
        // report's own law that a search space which silently shrinks reads as
        // coverage it does not have, applied to a lookup of one.
        let e = report_from_args(&a(&[cat, &format!("part={prog}"), "feature=Absent"])).unwrap_err();
        assert!(e.contains("Bore (CylinderBore)"), "got: {e}");

        // And the happy path off a SHIPPED program: 40mm bore, 12mm deep.
        let r = report_from_args(&a(&[cat, &format!("part={prog}"), "feature=Bore"]))
            .expect("a shipped program's bore states its own job");
        assert!(r.starts_with("assembly report — reach 12.000mm through a 40.000mm opening, 12.000mm deep"), "{r}");
        assert!(r.contains("READ from feature 'Bore' (CylinderBore)"), "{r}");
    }

    /// **A TWO-FILE PROJECT, which this input could not read at all until it
    /// joined the loader's home** (`DECISIONS.md` → "`--assembly-report` as
    /// the sixth verb…", fork 4A). `plate.cam` declares its features in
    /// `geometry.part`; `part=plate.cam` used to answer "cannot read
    /// 'plate.cam': Is a directory", because this door read a FILE and
    /// nothing else.
    ///
    /// The bare-file spelling still reads one file, and that is not the
    /// defect: `nacs plate.cam/main.part` says the same thing everywhere in
    /// the CLI, because a main.part without its geometry.part genuinely does not
    /// declare those features. What was missing was the PROJECT spelling.
    #[test]
    fn the_part_input_reads_a_project_directory_the_way_every_other_mode_does() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let cat = "tests/fixtures/sample.catalog";
        let r = report_from_args(&a(&[cat, "part=plate.cam", "feature=Notch"]))
            .expect("a directory is a project, here as everywhere else");
        assert!(r.contains("READ from feature 'Notch' (ProfilePocket) in plate.cam"), "{r}");

        // The bare file, unchanged and correct: geometry.part is not part of
        // that spelling, so the feature genuinely is not declared.
        let e = report_from_args(&a(&[cat, "part=plate.cam/main.part", "feature=Notch"]))
            .expect_err("a bare main.part declares no Notch");
        assert!(e.contains("referenced feature 'Notch' not defined"), "{e}");
    }

    #[test]
    fn the_shipped_counterbore_stops_recommending_a_cutter_that_cannot_enter_it() {
        // THE ACCEPTANCE, on shipped data with no new fixture: the catch this
        // slice was built on, closed where it was found. Before it, this
        // command's number-one recommendation for a 6.000mm hole was a 9.525mm
        // cutter, because `opening` was only ever compared against `nose_dia`.
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let r = report_from_args(&a(&[
            "tests/fixtures/sample.catalog",
            "part=cbore.cam/main.part",
            "feature=Hole1",
        ]))
        .expect("a shipped counterbored hole states its own corridor");

        // The step the scalar could not hold, read off the declaration and
        // stated in the header — a corridor the reader cannot see is one they
        // cannot check the rows against.
        assert!(
            r.contains("corridor: 16.000mm across to 6.000mm deep, then 6.000mm across below that"),
            "{r}"
        );
        // Every cutter too big for the hole is an infeasible ROW naming both
        // diameters, not a silent drop: a search space that shrinks quietly
        // reads as coverage it does not have.
        assert!(r.contains("the 9.525mm cutter cannot enter a 6.000mm opening"), "{r}");
        assert!(r.contains("the 6.350mm cutter cannot enter a 6.000mm opening"), "{r}");
        // And what is left ranked is the one cutter that fits, which used to be
        // fourth.
        let ranked: Vec<&str> = r
            .lines()
            .take_while(|l| !l.starts_with("infeasible:"))
            .filter(|l| l.contains("cutter '"))
            .collect();
        assert_eq!(ranked.len(), 2, "only the 1/8 pairs survive: {ranked:?}");
        assert!(ranked.iter().all(|l| l.contains("'1/8-carbide'")), "{ranked:?}");
        assert!(ranked[0].trim_start().starts_with("1."), "and it leads: {:?}", ranked[0]);

        // The seat still buys this assembly nothing, and that is a MEASUREMENT
        // rather than an oversight: 16mm across leaves 6.588mm of room once the
        // cutter's excursion is taken out, against a 7.500mm nose. The report
        // says so by not claiming a descent.
        assert!(!r.contains("nose enters"), "{r}");
    }

    #[test]
    fn a_pocket_reads_now_and_a_straight_wall_leaves_the_cutter_its_own_radius() {
        // The kind that used to reject by name. Its opening IS
        // direction-dependent — and that was never the binding number, so the
        // polygon is not measured and the declaration is read for its depth.
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let r = report_from_args(&a(&[
            "tests/fixtures/sample.catalog",
            "part=profile_pocket.cam/main.part",
            "feature=Window",
        ]))
        .expect("a pocket is a cavity a rod descends into");
        assert!(r.starts_with("assembly report — reach 2.000mm into a cavity that states no crossing"), "{r}");
        assert!(r.contains("reach/depth READ from"), "no crossing was read, so none is claimed: {r}");
        assert!(!r.contains("nose enters"), "a straight wall leaves the cutter's own radius: {r}");

        // The same law on the shipped BORE, which is where the over-claim was
        // visible: a 40mm bore finished by a 6.350mm cutter leaves that cutter's
        // 3.175mm of room, so no nose on the shelf follows it in. v1 said three
        // of these rows entered, by comparing 14.000 against 40.000 and never
        // asking where the tool was.
        let r = report_from_args(&a(&[
            "tests/fixtures/sample.catalog",
            "part=cylinder_bore.cam/main.part",
            "feature=Bore",
        ]))
        .expect("report");
        assert!(r.contains("through a 40.000mm opening"), "{r}");
        assert!(!r.contains("nose enters"), "{r}");
    }

    #[test]
    fn the_feeds_input_refuses_what_it_cannot_read() {
        let cat = "tests/fixtures/sample.catalog";
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let job = ["reach=45", "opening=30", "depth=40"];
        let with = |extra: &[&str]| {
            let mut v: Vec<&str> = vec![cat];
            v.extend(job.iter());
            v.extend(extra.iter());
            a(&v)
        };
        // A path that is not there, and a file that is not a feeds table:
        // both name the file, because "which file" is the first thing the
        // reader needs and the second error carries the table's own line.
        let e = report_from_args(&with(&["material=6061", "feeds=nope.feeds"])).unwrap_err();
        assert!(e.contains("cannot read feeds table 'nope.feeds'"), "{e}");
        let e = report_from_args(&with(&["material=6061", "feeds=tests/fixtures/sample.catalog"])).unwrap_err();
        assert!(e.contains("feeds table 'tests/fixtures/sample.catalog'"), "{e}");
        let e = report_from_args(&with(&["feeds="])).unwrap_err();
        assert!(e.contains("feeds= needs a path"), "{e}");

        // A table with no material to look it up by is a table nobody reads —
        // said on the command line, where it can still be fixed, rather than
        // silently ignored.
        let e = report_from_args(&with(&["feeds=tests/fixtures/shop.feeds"])).unwrap_err();
        assert!(e.contains("but material= was not"), "{e}");

        // And the happy path, end to end through the arg parser.
        let r = report_from_args(&with(&["material=6061", "feeds=tests/fixtures/shop.feeds"]))
            .expect("the shop table reaches the report");
        assert!(r.contains("rpm: from the shop feeds table"), "{r}");
    }

    #[test]
    fn a_shank_outside_the_grip_is_an_infeasible_row_not_a_silent_drop() {
        // V3's acceptance, half two — and the report's own law: a search space
        // that silently shrinks reads as coverage it does not have. So the
        // pair appears, saying why, with both numbers.
        let cat = read_catalog("tests/fixtures/sample.catalog").expect("shipped catalog");
        let job = Job {
            material: None, shop_feeds: None, derived_from: None, composed: Vec::new(), reach: 45.0, corridor: Corridor::straight(15.0), depth: 40.0,
            clearance: DEFAULT_CLEARANCE_MM, load: DEFAULT_LOAD_N,
        };
        let report = assembly_report(&cat, &job).expect("report");
        assert!(
            report.contains("the 9.525mm shank is outside the holder's 3.000..8.000mm grip range"),
            "the grip error must name both numbers: {report}"
        );
        // It is rejected, not ranked: the pair appears under infeasible only.
        let ranked: Vec<&str> = report.lines().filter(|l| l.contains("'3/8-carbide' in holder 'slim-nose'")).collect();
        assert_eq!(ranked.len(), 1, "exactly one mention, in infeasible: {ranked:?}");
        assert!(ranked[0].trim_start().starts_with("- cutter"), "{:?}", ranked[0]);

        // An UNDECLARED grip is today's behaviour byte for byte: er32-standard
        // declares none and still pairs with every cutter it physically can.
        assert!(cat.holders.iter().find(|h| h.name == "er32-standard").unwrap().grip.is_none());
        assert!(report.contains("'3/8-carbide' in holder 'er32-standard'"),
            "an ungripped holder must still consider everything: {report}");
    }

    #[test]
    fn the_two_new_catalog_fields_refuse_what_they_cannot_mean() {
        let base = "cutter c\ndia 6\nflute_len 20\nshank_dia 6\noverall_len 60\nmodulus 600\nholder h\nnose_dia 30\nnose_len 20\nbody_dia 50\ngauge_len 60\ncompliance 2\n";
        // A grip that is not a range says so, and points at the shrink-fit
        // spelling rather than leaving the author to guess it.
        let e = parse_catalog(&format!("{base}grip 6\n")).expect_err("must reject");
        assert!(e.contains("is not a range") && e.contains("d..d"), "{e}");
        let e = parse_catalog(&format!("{base}grip 8..3\n")).expect_err("must reject");
        assert!(e.contains("not a range a shank can be inside"), "{e}");
        // The text bucket must not swallow a typo in a NUMERIC field.
        let e = parse_catalog("cutter c\ndia six\n").expect_err("must reject");
        assert!(e.contains("is not a number"), "{e}");
        // Unknown fields still reject now that two of them are text.
        let e = parse_catalog(&format!("{base}colour 5\n")).expect_err("must reject");
        assert!(e.contains("unknown holder field 'colour'"), "{e}");
        // And a NON-numeric value in a field that is not one of the two text
        // keys still rejects as "not a number" — the text bucket is a
        // whitelist, not a hole in the grammar.
        let e = parse_catalog(&format!("{base}colour red\n")).expect_err("must reject");
        assert!(e.contains("is not a number"), "{e}");
        // An undeclared type is Endmill — the assumption tool_line already
        // made, now the documented default rather than a silent guess.
        let cat = parse_catalog(base).expect("parses");
        assert_eq!(cat.cutters[0].tool_type, "Endmill");

        // V4's field is a COUNT, and the catalog's numeric bucket is f64 —
        // so wholeness is checked here or nowhere. Positivity comes free from
        // the bucket's own law; both errors are the `parse.rs` mold. (The
        // count is a CUTTER field, so it takes its own base: `base` above
        // leaves the holder record open.)
        let cutter = "cutter c\ndia 6\nflute_len 20\nshank_dia 6\noverall_len 60\nmodulus 600\n";
        let wall = "holder h\nnose_dia 30\nnose_len 20\nbody_dia 50\ngauge_len 60\ncompliance 2\n";
        let cat = parse_catalog(&format!("{cutter}flute_count 3\n{wall}")).expect("parses");
        assert_eq!(cat.cutters[0].flute_count, Some(3));
        assert_eq!(parse_catalog(base).expect("parses").cutters[0].flute_count, None,
            "an undeclared count is absent, never a guessed default");
        let e = parse_catalog(&format!("{cutter}flute_count 3.5\n{wall}")).expect_err("must reject");
        assert!(e.contains("must be a whole number"), "{e}");
        let e = parse_catalog(&format!("{cutter}flute_count 0\n{wall}")).expect_err("must reject");
        assert!(e.contains("must be positive"), "{e}");
    }

    #[test]
    fn the_catalog_and_the_declaration_cannot_drift_apart() {
        // The anti-drift certificate for Phase 8's serial slice 0. Two
        // spellings of one holder existed with nothing connecting them; this
        // is what now connects them, and it is a FILL-AND-PARSE pin rather
        // than a field-by-field comparison on purpose (template.rs's pattern):
        // it goes out through the emitted text and back in through the real
        // parser, so it fails if the line stops parsing, if a field is
        // renamed, or if either spelling grows a fact the other does not.
        let cat = parse_catalog(CAT).expect("catalog parses");
        let c = &cat.cutters[1]; // half
        let h = &cat.holders[1]; // slim
        let projection = 61.0;

        // Out: the catalog's own conversion.
        let from_catalog = assembly_ladder(c, h, projection);

        // ...and back in: the line it proposes, filled with a real slot and
        // put through the parser every program uses.
        let line = tool_line(c, Placement::InHolder { holder: h, projection }).replace("T?", "T1");
        let src = format!("units mm\nstock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n{line}\n");
        let part = crate::parse::parse(&src)
            .unwrap_or_else(|e| panic!("the proposed tool line must parse: {e}\n{line}"));
        let tool = part.tools.get(&1).expect("T1 declared");
        let from_declaration = match crate::parse::Silhouette::of(tool) {
            crate::parse::Silhouette::Flat { body, .. } => body.expect("a declared body"),
            other => panic!("expected a flat endmill silhouette, got {other:?}"),
        };

        assert_eq!(from_catalog, from_declaration,
            "the catalog's ladder and the declared one must be the same body");

        // And the ladder actually answers where the holder is, which is the
        // question V2's fixture check will ask of it.
        assert_eq!(from_declaration.radius_at(projection + 1.0), Some(h.nose_dia / 2.0));
        assert_eq!(
            from_declaration.radius_at(projection + h.nose_len + 1.0),
            Some(h.body_dia / 2.0)
        );

        // V4's two fields ride the SAME pin rather than getting a new one —
        // the entry's own acceptance. A record that states its kind and its
        // flute count must find both facts intact on the far side of the
        // parser, because both are now emitted and neither is a length the
        // ladder above would have noticed going missing.
        let mut drill = c.clone();
        drill.tool_type = "Drill".to_string();
        drill.flute_count = Some(2);
        let line = tool_line(&drill, Placement::InHolder { holder: h, projection }).replace("T?", "T1");
        let src = format!("units mm\nstock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n{line}\n");
        let part = crate::parse::parse(&src)
            .unwrap_or_else(|e| panic!("a drill's proposed line must parse too: {e}\n{line}"));
        let tool = part.tools.get(&1).expect("T1 declared");
        assert_eq!(tool.name, "Drill",
            "the line must declare the kind the RECORD states — printing Endmill regardless is the guess V4 found: {line}");
        assert_eq!(tool.flute_count, Some(2), "the flute count must survive the round trip: {line}");
        assert_eq!(
            assembly_ladder(&drill, h, projection),
            match crate::parse::Silhouette::of(tool) {
                crate::parse::Silhouette::Flat { body, .. } => body.expect("a declared body"),
                other => panic!("expected a flat silhouette, got {other:?}"),
            },
            "a kind that is not Endmill must not move the body"
        );
    }

    #[test]
    fn the_shipped_catalog_now_declares_a_tool_that_can_ask_for_its_own_feed() {
        // V4'S ACCEPTANCE, ON THE SHIPPED CATALOG. `flute_count` is the one
        // field fork 3 measured missing, and "missing" was never abstract:
        // K3's chip-load identity is F = RPM x z x f_z, so without z the
        // emitted declaration could carry a whole body and still not ask for
        // a feed. This is that gap closing, in the only way that counts — the
        // same line, before and after.
        let cat = read_catalog("tests/fixtures/sample.catalog").expect("shipped catalog");
        let c = cat.cutters.iter().find(|c| c.name == "3/8-carbide").expect("shipped cutter");
        let h = cat.holders.iter().find(|h| h.name == "er32-standard").expect("shipped holder");
        let line = tool_line(c, Placement::InHolder { holder: h, projection: 45.0 }).replace("T?", "T5");
        assert!(line.contains("flute_count=4"), "the shipped record's count must reach the line: {line}");

        let program = |tool_line: &str| {
            format!(
                "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
                 stock Box(w=120, l=80, h=20, x=0, y=0, z=0, material=6061)\n\
                 {tool_line}\n\
                 feature TopFace Plane(z=-0.5)\n\
                 run End(tool=T5, surface=TopFace, stepover=6.0, spindle=derive, feed=derive)\n"
            )
        };
        let part = crate::parse::parse(&program(&line))
            .unwrap_or_else(|e| panic!("the shipped record's line must carry a derivable feed: {e}"));

        // The identity, restated from its own homes rather than pasted as a
        // literal — 150 m/min and 0.040 mm/tooth are the shipped table's
        // 6061xEndmill row for a 9.525mm cutter.
        let rpm = crate::parse::derive_spindle_rpm(150.0, c.dia);
        let expected = crate::parse::derive_feed_mm_min(rpm, 4, 0.040);
        assert!((rpm - 5012.754).abs() < 1e-3, "rpm was {rpm}");
        assert!((part.operations[0].feed.unwrap() - expected).abs() < 1e-9,
            "derived F was {:?}, identity says {expected}", part.operations[0].feed);
        assert!((expected - 802.040).abs() < 1e-3, "the derived feed was {expected}");

        // And the before: the same line with the count taken back out is the
        // line this catalog emitted yesterday, and it rejects BY NAME rather
        // than guessing a tooth count — which is why an absent `flute_count`
        // needs no note on the line itself.
        let without = line.replace(", flute_count=4", "");
        let err = crate::parse::parse(&program(&without)).unwrap_err();
        assert!(err.contains("declares no flute count"), "got: {err}");
    }

    #[test]
    fn the_neck_the_tool_line_could_only_name_is_now_declared() {
        // THIS PIN USED TO ASSERT THE LOSS. Until 2026-08-18 `parse::Tool` had
        // no neck fields, so a necked cutter could not be declared in full and
        // the line carried a NOTE saying the emitted body was WIDER than the
        // real one — and this test asserted the note was there. K2's design
        // named the trigger for closing that gap ("the first real necked-tool
        // program"), the catalog's own `1/4-reach` became it, and the fields
        // landed. So the pin now asserts the landed truth, which is the same
        // work it was always doing.
        let cat = parse_catalog(CAT).expect("catalog parses");
        let plain = &cat.cutters[1];
        let mut necked = plain.clone();
        necked.neck_dia = Some(8.0);
        necked.neck_len = Some(10.0);
        let h = &cat.holders[1];

        let at = |c: &Cutter| tool_line(c, Placement::InHolder { holder: h, projection: 61.0 });
        assert!(!at(plain).contains("neck"),
            "an unnecked cutter declares no neck: {}", at(plain));
        let line = at(&necked);
        assert!(line.contains("neck_dia=8.000, neck_len=10.000"), "got: {line}");
        assert!(!line.contains("NOTE"), "there is no loss left to name: {line}");

        // Out through the text and back in, the slice-0 way: the neck must
        // survive the real parser, and the two ladders must agree about it.
        let src = format!(
            "units mm\nstock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n{}\n",
            line.replace("T?", "T1")
        );
        let part = crate::parse::parse(&src)
            .unwrap_or_else(|e| panic!("a necked cutter's line must parse: {e}\n{line}"));
        let tool = part.tools.get(&1).expect("T1 declared");
        assert_eq!((tool.neck_dia, tool.neck_len), (Some(8.0), Some(10.0)));
        assert_eq!(
            assembly_ladder(&necked, h, 61.0),
            match crate::parse::Silhouette::of(tool) {
                crate::parse::Silhouette::Flat { body, .. } => body.expect("a declared body"),
                other => panic!("expected a flat silhouette, got {other:?}"),
            },
            "the catalog's ladder and the declared one must agree about the neck too"
        );

        // And the point of declaring it: the body is NARROWER in the neck band
        // than the shank the line used to imply, which is the understatement
        // the old NOTE existed to confess.
        let sil = crate::parse::Silhouette::of(tool);
        let (flute_top, neck_top) = (plain.flute_len, plain.flute_len + 10.0);
        assert_eq!(sil.radius_at(flute_top + 1.0), 4.0, "inside the neck, the neck's radius");
        assert_eq!(sil.radius_at(neck_top + 1.0), plain.shank_dia / 2.0, "above it, the shank's");
    }

    #[test]
fn the_nose_is_a_search_variable_not_a_table_row() {
        // The item's founding claim, as arithmetic — and now with the
        // condition it always had: the nose descends where the CAVITY leaves
        // it room, which means a step. A 30mm seat 15mm deep over a 14mm hole,
        // reached to 30.
        let cat = parse_catalog(CAT).unwrap();
        let eighth = &cat.cutters[0];
        let seated = Job {
            corridor: Corridor::Steps(vec![(0.0, 15.0), (15.0, 7.0)]),
            ..job()
        };
        let (p_stubby, enters_stubby, _) = min_projection(eighth, &cat.holders[0], &seated).unwrap();
        let (p_slim, enters_slim, _) = min_projection(eighth, &cat.holders[1], &seated).unwrap();
        assert!(!enters_stubby && enters_slim);
        assert!((p_stubby - 30.0).abs() < 1e-9, "blocked nose: full reach projects");
        assert!((p_slim - 15.0).abs() < 1e-9, "descent to the seat's floor: {p_slim}");
        let d_stubby = assembly_deflection(eighth, &cat.holders[0], p_stubby, 10.0);
        let d_slim = assembly_deflection(eighth, &cat.holders[1], p_slim, 10.0);
        assert!(d_slim < d_stubby, "shorter stickout wins: {d_slim} vs {d_stubby}");

        // And the honest counterpoint, which is why this is a solver and not
        // a rule: the fat half-inch barely bends at either projection, so for
        // IT the stiffer stubby holder wins outright — the ranking is
        // computed per pair, never assumed from a slogan.
        let half = &cat.cutters[1];
        let (ps, _, _) = min_projection(half, &cat.holders[0], &seated).unwrap();
        let (pl, _, _) = min_projection(half, &cat.holders[1], &seated).unwrap();
        let ds = assembly_deflection(half, &cat.holders[0], ps, 10.0);
        let dl = assembly_deflection(half, &cat.holders[1], pl, 10.0);
        assert!(ds < dl, "the fat cutter prefers the stiff holder: {ds} vs {dl}");

        // THE CORRECTION THIS SLICE MADE, pinned where a reader meets the
        // claim: on the STRAIGHT opening `job()` states, the same slim nose no
        // longer descends at all. A 3.175mm cutter in a 20mm bore orbits, so
        // its axis stands 8.4125mm off centre and leaves the nose 1.5875mm of
        // room — the cutter's own radius. v1 said this nose entered because it
        // compared 14mm against 20mm and never asked where the tool was.
        let (p_straight, enters_straight, _) =
            min_projection(eighth, &cat.holders[1], &job()).unwrap();
        assert!(!enters_straight, "a straight wall leaves the cutter's own radius, and no more");
        assert!((p_straight - 30.0).abs() < 1e-9, "so the full reach projects: {p_straight}");
    }

    #[test]
    fn the_grip_ceiling_is_an_infeasibility_with_a_reason_not_an_error() {
        let cat = parse_catalog(CAT).unwrap();
        let eighth = &cat.cutters[0];
        // The eighth-inch tool cannot make a 36mm reach in the blocked
        // holder: 38.1 overall less 1.5x3.175 grip leaves 33.3mm.
        let deep = Job { reach: 36.0, ..job() };
        let err = min_projection(eighth, &cat.holders[0], &deep).unwrap_err();
        assert!(err.contains("nose blocked"), "{err}");
        assert!(err.contains("shank grip"), "{err}");
        // And the report carries it as a row, not a process failure.
        let report = assembly_report(&cat, &deep).unwrap();
        assert!(report.contains("infeasible:"), "{report}");
        assert!(report.contains("cutter 'eighth' in holder 'stubby'"), "{report}");
    }

    #[test]
    fn the_stiffness_ladder_pins_hold_in_metric() {
        // The roadmap's ladder, ×25.4: single-section rods at the four
        // (dia, stickout) pairs. Deflection goes as L³/d⁴, so the ladder's
        // d⁴/L³ values (3.8, 9.3, 19.8, 18.5 ×10⁻³ in⁴/in³) invert into
        // deflection ratios this test reads directly.
        let rods: [(f64, f64); 4] = [
            (0.125 * 25.4, 0.4 * 25.4),
            (0.25 * 25.4, 0.75 * 25.4),
            (0.375 * 25.4, 1.0 * 25.4),
            (0.5 * 25.4, 1.5 * 25.4),
        ];
        let defl: Vec<f64> = rods
            .iter()
            .map(|(d, l)| cantilever_deflection_per_n(&[(*l, *d)], 600.0))
            .collect();
        let ladder = [3.8147e-3, 9.2593e-3, 19.775e-3, 18.519e-3]; // d⁴/L³, inches (the roadmap's 3.8/9.3/19.8/18.5, exact)
        for pair in [(0usize, 1usize), (1, 2), (2, 3)] {
            let expect = ladder[pair.0] / ladder[pair.1]; // stiffer -> less deflection
            let got = defl[pair.1] / defl[pair.0];
            assert!(
                (got / expect - 1.0).abs() < 1e-3,
                "ladder ratio {:?}: got {got}, want {expect}", pair
            );
        }
        // 3/8 at 1.0" is genuinely STIFFER than 1/2 at 1.5" — the ~7% tie the
        // ranker must surface rather than silently ordering.
        assert!(defl[2] < defl[3]);
        assert!(defl[3] / defl[2] < TIE_FACTOR, "the pair is inside the tie window");
    }

    #[test]
    fn a_near_tie_is_reported_as_a_tie_with_the_tiebreakers() {
        // The ladder's own 3/8-at-1.0" vs 1/2-at-1.5" pair, as catalog rows:
        // all-flute cutters (the common 0.78 core factor cancels from the
        // ratio), flute floors forcing exactly the ladder's stickouts, and a
        // near-zero-compliance holder so the ranking IS the ladder. ~7%
        // apart: the tie window must catch it rather than silently ordering.
        let cat = parse_catalog(
            "cutter three-eighths\ndia 9.525\nflute_len 25.4\nshank_dia 9.525\noverall_len 45.0\nmodulus 600\ncost 30\n\
             cutter half\ndia 12.7\nflute_len 38.1\nshank_dia 12.7\noverall_len 60.0\nmodulus 600\ncost 45\n\
             holder plain\nnose_dia 30.0\nnose_len 20.0\nbody_dia 60.0\ngauge_len 70.0\ncompliance 0.001\n",
        )
        .unwrap();
        // The opening is too small for the nose; the flute floors set the
        // projections at 25.4 and 38.1 — the ladder's inch stickouts.
        let j = Job { material: None, shop_feeds: None, derived_from: None, composed: Vec::new(), reach: 20.0, corridor: Corridor::straight(8.0), depth: 30.0, clearance: 0.5, load: 10.0 };
        let report = assembly_report(&cat, &j).unwrap();
        assert!(report.contains("=1."), "the tie is explicit: {report}");
        assert!(report.contains("within 10% of the leader"), "{report}");
        assert!(report.contains("decided by the tiebreaker data"), "{report}");
        // And the 3/8 leads it — genuinely stiffer, as the ladder says.
        let first = report.lines().find(|l| l.contains("=1.")).unwrap();
        assert!(first.contains("three-eighths"), "{report}");
    }

    #[test]
    fn the_report_states_its_inputs_and_omits_what_it_cannot_derive() {
        let cat = parse_catalog(CAT).unwrap();
        let report = assembly_report(&cat, &job()).unwrap();
        assert!(report.starts_with("assembly report — reach 30.000mm"), "{report}");
        // The load stays DECLARED, but the reason it is had to change: K3
        // built the feeds engine this line used to blame, and a table of
        // speeds and chip loads still does not yield a cutting FORCE.
        assert!(report.contains("not cutting forces"), "{report}");
        assert!(report.contains("rpm: omitted"), "{report}");
        assert!(report.contains("understates a long nose"), "{report}");
        // Every feasible pair is a row; nothing silently dropped.
        assert_eq!(report.matches("cutter '").count(), 4, "{report}");
    }

    #[test]
    fn the_cli_arg_refusals_hold() {
        for (args, needle) in [
            (vec![], "usage:"),
            (vec!["nope.catalog".to_string()], "cannot read catalog"),
        ] {
            let err = report_from_args(&args).unwrap_err();
            assert!(err.contains(needle), "wanted '{needle}', got {err}");
        }
    }

    #[test]
    fn the_clipboard_refuses_every_line_it_cannot_honestly_print() {
        // `--tool-line` prints a DECLARATION, and a declaration the parser or
        // the optimizer would have rejected is worse than no line at all — the
        // reader pastes it and finds out later. So every error here is one
        // the rest of the file already owns, reached from a new door: the
        // grip range is `judge`'s, the projection band is `min_projection`'s
        // (through the one home both now read), and the missing stickout is
        // the parser's own placement law.
        let cat = "tests/fixtures/sample.catalog";
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for (args, needle) in [
            (a(&[]), "usage:"),
            (a(&[cat]), "usage:"),
            (a(&["nope.catalog", "x"]), "cannot read catalog"),
            // A key=value where the name belongs is a command-line mistake,
            // and blaming the catalog for it sends the reader to the wrong file.
            (a(&[cat, "holder=er32-standard"]), "expected a cutter name"),
            (a(&[cat, "1/2-carbide"]), "no cutter named '1/2-carbide'"),
            (a(&[cat, "3/8-carbide", "holder=er40-fantasy", "stickout=45"]), "no holder named"),
            (a(&[cat, "3/8-carbide", "spindle=9000"]), "unknown input 'spindle'"),
            (a(&[cat, "3/8-carbide", "stickout=deep"]), "is not a number"),
            (a(&[cat, "3/8-carbide", "stickout=0"]), "must be positive"),
            // The band, both ends: flutes buried in the holder, and shank
            // pulled out past what the grip law leaves.
            (a(&[cat, "3/8-carbide", "stickout=20"]), "would bury 5.400mm of cutting length"),
            (a(&[cat, "3/8-carbide", "stickout=70"]), "leaves under 1.5x9.525mm of shank"),
            // The neck counts as length that must stand clear, so the necked
            // reach tool's floor is higher than its flute length alone.
            (a(&[cat, "1/4-reach", "stickout=30"]), "plus a 25.400mm neck"),
            (a(&[cat, "3/8-carbide", "holder=slim-nose", "stickout=45"]), "outside its 3.000..8.000mm grip range"),
            (a(&[cat, "3/8-carbide", "holder=er32-standard"]), "needs a stickout="),
        ] {
            let err = tool_line_from_args(&args).unwrap_err();
            assert!(err.contains(needle), "wanted '{needle}', got: {err}");
        }
    }

    #[test]
    fn the_clipboard_prints_a_line_a_program_can_keep() {
        // The mode's whole contract: what it prints is what an author would
        // have typed, so it has to PARSE — with a holder and without one. The
        // loose form is not a lesser line, it is the shop wall's record of one
        // cutter, which is a complete declarable thing before a holder is
        // chosen.
        let cat = "tests/fixtures/sample.catalog";
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let parses = |line: &str| {
            let src = format!(
                "units mm\nstock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n{}\n",
                line.trim().replace("T?", "T7")
            );
            crate::parse::parse(&src)
                .unwrap_or_else(|e| panic!("the printed line must parse: {e}\n{line}"))
        };

        let mounted = tool_line_from_args(&a(&[cat, "3/8-carbide", "holder=er32-standard", "stickout=45"])).unwrap();
        let part = parses(&mounted);
        let tool = part.tools.get(&7).expect("T7 declared");
        assert_eq!(tool.stickout, Some(45.0));
        assert_eq!(tool.holder.as_deref(), Some("er32-standard"));
        assert_eq!(tool.flute_count, Some(4));

        let loose = tool_line_from_args(&a(&[cat, "3/8-carbide"])).unwrap();
        assert!(!loose.contains("holder"), "no holder was chosen, so none may be declared: {loose}");
        assert!(!loose.contains("stickout"), "an unstated stickout is not a guessable one: {loose}");
        let part = parses(&loose);
        let tool = part.tools.get(&7).expect("T7 declared");
        assert_eq!((tool.dia, tool.flute_count), (9.525, Some(4)));
        assert_eq!(tool.stickout, None);

        // A stickout with no holder is legal and useful — it is how far the
        // tool stands out, a fact the cutter has without a holder chosen.
        let bare = tool_line_from_args(&a(&[cat, "3/8-carbide", "stickout=45"])).unwrap();
        assert!(bare.contains("stickout=45.000") && !bare.contains("holder"), "got: {bare}");
        assert_eq!(parses(&bare).tools.get(&7).unwrap().stickout, Some(45.0));
    }
}
