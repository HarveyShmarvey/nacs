use crate::math::Portable as _;
// The cutter: `Tool`, its ground forms (`Rounder`, `Chamfer`, cones and arcs), the body `Silhouette`, and the checks a declaration must pass.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

// A vertex of a 3D wire (the Contour pair): the one point type that carries
// its own Z, because the feature's geometry does (total Z span = the wire's
// own Z span + extent — DECISIONS.md "the 3D-wire Contour…").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Pt3 {
    // The XY shadow — winding, offsetting, and the centroid lead all read the
    // projection, because the wall is a vertical sweep and its plan view IS
    // the planar problem the existing machinery already solves.
    pub fn xy(&self) -> Pt {
        Pt { x: self.x, y: self.y }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub num: usize,
    pub name: String,
    pub dia: f64,
    pub profile: Option<Vec<Pt>>,
    /// Present only for `CornerRounder` — the ground arc plus the relieved
    /// clearance record (DECISIONS.md → "the corner rounder gets an
    /// operation"). `dia` above is the tool's BODY OD for a rounder, which is
    /// clearance; nothing positions from it.
    pub rounder: Option<Rounder>,
    /// Present only for `ChamferMill` — the ground cone plus the relieved tip
    /// flat (DECISIONS.md → "the chamfer tool — one tangent surface instead of
    /// two…"). `dia` above is the BODY OD here too, clearance again.
    pub chamfer: Option<Chamfer>,
    /// K2 body fields (DECISIONS.md → "K2, the tool body", slice 0): tier-2
    /// BODY data — declared in the program text, read by the certificates (the
    /// foul advisory, `check_moves`), and never positioned from — the same
    /// clearance-record class as `pilot` and `tip_dia`. All optional: an
    /// undeclared body means today's behavior, never a guess.
    ///
    /// Where the flutes end, above the tip, mm. Legal only on the rods
    /// (Endmill/Ballmill and kin): a drawn profile's top vertex and a ground
    /// form's derived run-out already own the cutting top, and two records
    /// describing one fact is the chamfer thread's error law.
    pub flute_len: Option<f64>,
    /// The shank's diameter, mm — above `flute_len` on a rod, above the drawn
    /// top vertex on a profile tool (where it replaces the `max()` guess and
    /// finally makes the narrower-than-the-flutes shank expressible).
    pub shank_dia: Option<f64>,
    /// A **relieved** section between the flutes and the shank — the long-reach
    /// cutter's ground-back waist (`DECISIONS.md` → "K2's neck fields…"). Both
    /// or neither, seated by construction at the flute top, and narrower than
    /// the cutting diameter because that is what "relieved" means.
    ///
    /// **What this field is NOT.** It does not declare a tool that can
    /// *undercut*. The undercut class is one whose **cutting envelope** narrows
    /// going up, and that is expressible only by drawing it (`profile=`), where
    /// `Silhouette::narrowing()` catches it and `vox` rejects to stamp it by
    /// name. A neck is above the cutting top and removes nothing — the same
    /// reasoning that already keeps a narrow `shank_dia` out of the fence.
    ///
    /// **The overlap with catalog language is real, and the model absorbs it.**
    /// A shelf calls a T-slot cutter, a Woodruff cutter and a long-reach endmill
    /// all "necked", and the first two genuinely cut undercuts. Declaring one as
    /// a rod plus a neck is nevertheless *honest*, because a declared
    /// `flute_len` already truncates the stamp at the cutting top and derives
    /// `upward_closed = false` (slice 2, fork 5A) — a disc on a neck is exactly
    /// what that models. The tools this cannot describe reject elsewhere and by
    /// name: a lollipop's ball would need flutes ending inside the ball, and a
    /// dovetail's cutting cone must be drawn, where the fence is waiting.
    pub neck_dia: Option<f64>,
    /// The relieved section's length, mm above the flute top.
    pub neck_len: Option<f64>,
    /// How far the tool sticks out of the holder, mm above the tip — where the
    /// tool ends and the holder's nose face sits. Parsed and claim-checked in
    /// slice 0; slice 1 gave it its first geometric consumer (the holder rungs
    /// seat on it). The reach check is still a later thread (fork 4c).
    pub stickout: Option<f64>,
    /// The holder, as PROVENANCE — a label for humans and future libraries,
    /// never resolved to geometry (fork 2A: a catalog record silently
    /// load-bearing for a hard error is the stale-derived-value bug one file
    /// over). The numeric fields below are the only holder geometry, and a
    /// library or wizard GENERATES them into the program text where the
    /// certificates and the diff can see them.
    pub holder: Option<String>,
    /// The holder nose's diameter, mm — the narrow end, whose face sits at the
    /// `stickout` plane. Requires `stickout`: a holder the ladder cannot place
    /// is a declaration that quietly does not exist.
    pub holder_nose_dia: Option<f64>,
    /// How far the nose runs up from its face, mm, to where the holder body
    /// begins. Paired with `holder_body_dia` by error, both ways: a nose top
    /// with nothing declared above it is a record nobody reads, and a body
    /// needs a bottom.
    pub holder_nose_len: Option<f64>,
    /// The holder body's diameter, mm, above the nose.
    pub holder_body_dia: Option<f64>,
    /// How many flutes the cutter has. Not geometry: a feeds-lookup axis,
    /// carried on the tool line because it is a fact about the cutter whoever
    /// consumes it (user ruling 2026-08-16 — see the K2 entry). Parsed and
    /// validated here; K3's chip-load→feed derivation is its first reader.
    pub flute_count: Option<usize>,
    /// The drawn profile's cutting band (DECISIONS.md → "K2, the tool body",
    /// fork 3A), heights above the tip, mm: where the cutting zone starts. A
    /// drawn form with a relieved pilot used to be assumed cutting over its
    /// whole extent — the standing form-tool gripe by construction. Contact
    /// math may only claim contact inside the band; a requested surface that
    /// needs contact outside it rejects with the shortfall as a number.
    /// `cut_bottom` bounds contact only — like the rounder's pilot, the solid
    /// below it still stamps (the tool occupies it; it just earns nothing).
    /// Defaults to 0: today, said out loud.
    pub cut_bottom: Option<f64>,
    /// Where the cutting zone ends, above the tip, mm — everything drawn
    /// above it is relieved body. Unlike `cut_bottom` this one truncates the
    /// cutting ENVELOPE too: the stamp stops here (the rounder's convention —
    /// relief removes nothing, and a field that recorded its removal would
    /// under-state leftovers, the one direction a stock model must never
    /// err). Defaults to the drawn top vertex: today, said out loud.
    pub cut_top: Option<f64>,
    /// The tool's FALLBACK coolant (DECISIONS.md → "coolant as a modeled
    /// setting…", fork 2A): how this cutter is run when the op says nothing —
    /// a through-coolant drill wants `through`, a graphite mill wants `air`,
    /// the `flute_count` precedent of a cutter fact parsed where it lives.
    /// An op's own `coolant=` overrides it; undeclared everywhere resolves
    /// to flood, today's behavior said out loud.
    pub coolant: Option<CoolantKind>,
}

/// A corner rounder as DECLARED: the ground arc, and beside it the relieved
/// surfaces that exist so the tool does not touch there.
///
/// The split is the point (DECISIONS.md → "the corner rounder gets an
/// operation"). `radius` is the arc's NOMINAL radius and is the tool's
/// identity — it is what pairs with a feature's `edge_round`, and an arc that
/// came off the grinder two hundredths small is not a different tool, so what
/// it measured rides as `radius_error`. `effective_dia` and `gauge_z` are
/// ACTUAL: they are the two numbers a presetter hands back and the two a setup
/// is made from, and nobody ever gauged from a catalogue version of them.
///
/// `pilot` is a clearance record and is NEVER read by positioning math. It is
/// here to make the manufacturer's relief a number (`relief()`) instead of a
/// gap nobody can see. Reading it as the offset is the classic defect this
/// type exists to prevent: on a catalogue drawing the arc is tangent to the
/// pilot, so `rc - R == pilot/2` exactly — true on paper, false in every ground
/// tool, and wrong in the direction that gouges the wall by the relief.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rounder {
    /// `R`, nominal. The tool's identity; pairs with `edge_round`.
    pub radius: f64,
    /// `2(rc - R)` — the diameter of consequence: the virtual cylinder the arc
    /// is tangent to, which is the wall the round lands on. Actual.
    pub effective_dia: f64,
    /// `zc + R` above the tip — the virtual plane the arc is tangent to, which
    /// is the top face the round lands on. Actual.
    pub gauge_z: f64,
    /// Clearance record: the relieved pilot's diameter. Never positioned from.
    pub pilot: f64,
    /// What the arc actually measured, against `radius`. A trail-off STEPS the
    /// blend; this RESIZES it.
    pub radius_error: f64,
    /// Degrees the arc falls short of tangency at the wall end. Residual blend
    /// deviation there is `R(1 - cos trail_wall)`.
    pub trail_wall: f64,
    /// Degrees the arc falls short of tangency at the face end.
    pub trail_face: f64,
    /// Whether these numbers came off a presetter or a catalogue — the
    /// tolerance state of the tool, which a model that cannot hold it is
    /// guessing about while sounding certain (`stock.rs`'s `complete` flag is
    /// the same instinct).
    pub measured: bool,
}

impl Rounder {
    /// The arc that actually cuts: nominal plus what it measured.
    pub fn arc_radius(&self) -> f64 {
        self.radius + self.radius_error
    }

    /// The virtual wall-tangent cylinder's radius, `rc - R`. **This** is the
    /// number the axis rides off the wall — not the pilot.
    pub fn effective_radius(&self) -> f64 {
        self.effective_dia / 2.0
    }

    /// The manufacturer's relief, made visible: how far the pilot is ground
    /// under the cylinder the arc is tangent to. Reported, never applied.
    pub fn relief(&self) -> f64 {
        self.effective_radius() - self.pilot / 2.0
    }

    /// Residual blend deviation at each end, `R(1 - cos trail)` — the step the
    /// finished part carries because the arc ran out before tangency.
    pub fn wall_step(&self) -> f64 {
        let r = self.arc_radius();
        r * (1.0 - self.trail_wall.to_radians().pcos())
    }
    pub fn face_step(&self) -> f64 {
        let r = self.arc_radius();
        r * (1.0 - self.trail_face.to_radians().pcos())
    }
}

/// A chamfer mill as DECLARED: the ground cone, and beside it the relieved tip
/// flat that exists so the tool does not touch there.
///
/// The same split as `Rounder`, one axis over (DECISIONS.md → "the chamfer
/// tool — one tangent surface instead of two…"). `angle` is the cone's NOMINAL
/// included angle and is the tool's identity — a 90-degree mill that grinds a
/// hundredth off is still a 90-degree mill, so what it measured rides as
/// `angle_error`; an angle error TILTS the face where a tip error SIZES it.
/// `gauge_dia` at `gauge_z` is one measured point ON the cone, ACTUAL — the
/// pair a presetter hands back — and together with the actual angle it pins the
/// cone in the tool's frame; everything else about the form is derived.
///
/// `tip_dia` is a clearance record and is NEVER read as a positioning datum. A
/// catalogue draws the cone coming to a sharp point; every ground tool has a
/// flat or broken corner there, so the theoretical apex sits
/// `(tip_dia/2)/tan(phi_half)` BELOW the physical tip, out in the air under the
/// tool. Set the length offset to the tip, do the math as if the point were
/// real, and the chamfer is wrong by exactly that — half the feature on a
/// 0.5mm chamfer with a 0.5mm flat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chamfer {
    /// The INCLUDED angle, nominal, degrees. The tool's identity; pairs with a
    /// feature's declared chamfer.
    pub angle: f64,
    /// What the ground cone actually measured, against `angle`, degrees.
    pub angle_error: f64,
    /// One measured point ON the cone: its diameter, `gauge_z` above the tip.
    /// Actual.
    pub gauge_dia: f64,
    /// Where that diameter was measured, above the tip. Actual.
    pub gauge_z: f64,
    /// Clearance record: the relieved/broken tip flat's diameter. Never
    /// positioned from — required rather than defaulted, because a zero
    /// default would make the catalogue's sharp-point lie the default.
    pub tip_dia: f64,
    /// Presetter or catalogue — the tolerance state of the tool, as on
    /// `Rounder`.
    pub measured: bool,
}

impl Chamfer {
    /// The `gauge_z` of a cone that ends at the tip flat: where a cone of
    /// this angle is `gauge_dia` wide, measured up from a flat `tip_dia`
    /// wide. What the form offers before a measured value is typed.
    pub fn gauge_z_at_flat(angle: f64, gauge_dia: f64, tip_dia: f64) -> Option<f64> {
        let ok = angle > 0.0 && angle < 180.0 && tip_dia > 0.0 && gauge_dia > tip_dia;
        ok.then(|| (gauge_dia - tip_dia) / 2.0 / (angle / 2.0).to_radians().ptan())
    }

    /// How far above the tip the cone starts: 0 on a cone that ends at
    /// its flat, more when a straight land runs below it.
    pub fn land(&self) -> f64 {
        let t = self.phi_half().ptan();
        self.gauge_z - (self.gauge_dia / 2.0) / t + (self.tip_dia / 2.0) / t
    }

    /// A declaration that reads like a slip: a land longer than the flat
    /// is wide is a pin under the cone, and the program plunges it into
    /// the work to bring the cone to the edge. Advice; the tool may be so.
    pub fn note(&self) -> Option<String> {
        let land = self.land();
        (land > self.tip_dia + 1e-9).then(|| {
            let fit = Self::gauge_z_at_flat(self.actual_angle(), self.gauge_dia, self.tip_dia).unwrap_or(0.0);
            format!(
                "its cone starts {land:.3}mm above the tip, on a {:.3}mm straight end: gauge_z ({:.3}) is how far above the tip the cone is gauge_dia ({:.3}) wide, not the tool's length. A cone that ends at the tip flat has gauge_z {fit:.3}",
                self.tip_dia, self.gauge_z, self.gauge_dia
            )
        })
    }

    /// The included angle that actually cuts: nominal plus what it measured.
    pub fn actual_angle(&self) -> f64 {
        self.angle + self.angle_error
    }

    /// Half-angle from the tool AXIS, radians — the flank's slope, from the
    /// actual angle.
    pub fn phi_half(&self) -> f64 {
        (self.actual_angle() / 2.0).to_radians()
    }
}

/// **The ground arc, and nothing else** — the only geometry positioning math
/// may reference (DECISIONS.md → "the corner rounder gets an operation").
///
/// This type is the form/clearance boundary made structural rather than
/// disciplinary: it is what `Tool::ground_arc` hands the gauging side, and it
/// has no pilot, no body OD, and no shank to read. A path generator holding one
/// of these *cannot* position from a relieved surface, because it cannot see
/// one. The body side keeps its own answer in `Silhouette`, which is the
/// tessellated solid and says nothing about where the tool goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundArc {
    /// The arc radius that actually cuts.
    pub r: f64,
    /// The arc centre, radially from the axis.
    pub rc: f64,
    /// The arc centre, above the tip.
    pub zc: f64,
    /// Degrees short of tangency at each end.
    pub trail_wall: f64,
    pub trail_face: f64,
}

impl GroundArc {
    /// The virtual wall-tangent cylinder's radius, `rc - r`: how far the axis
    /// rides off the finished wall.
    pub fn effective_radius(&self) -> f64 {
        self.rc - self.r
    }

    /// The virtual face-tangent plane, `zc + r` above the tip: the axial datum.
    pub fn gauge_z(&self) -> f64 {
        self.zc + self.r
    }

    /// **The virtual sharp corner**, `(effective_radius, gauge_z)` — where the
    /// two virtual tangent planes meet. This is the programming datum: it is
    /// the point driven along the model's edge, and every offset in this design
    /// is a delta from it.
    pub fn virtual_sharp_corner(&self) -> Pt {
        Pt { x: self.effective_radius(), y: self.gauge_z() }
    }

    /// Where the cutting edge actually stops, which is BELOW `gauge_z` by the
    /// face-end trail-off: the arc runs out at `zc + r·cos(trail_face)`.
    pub fn cutting_top(&self) -> f64 {
        self.zc + self.r * self.trail_face.to_radians().pcos()
    }

    /// Where the cutting edge actually starts, which is ABOVE the wall-tangent
    /// point by the wall-end trail-off.
    pub fn cutting_bottom(&self) -> f64 {
        self.zc + self.r * self.trail_wall.to_radians().psin()
    }

    /// A point on the arc at `phi` degrees from the wall-tangent end: `phi = 0`
    /// is vertical tangent at `(rc - r, zc)`, `phi = 90` is horizontal tangent
    /// at `(rc, zc + r)`. Returned as `(radius, height above tip)`.
    pub fn at(&self, phi_deg: f64) -> Pt {
        let phi = phi_deg.to_radians();
        Pt { x: self.rc - self.r * phi.pcos(), y: self.zc + self.r * phi.psin() }
    }
}

/// **The ground cone, and nothing else** — the chamfer mill's twin of
/// `GroundArc`: what `Tool::ground_cone` hands the gauging side. It carries the
/// flank's slope and the derived apex, plus the two heights that BOUND the
/// flank — where the relieved flat ends it below and where the body OD ends it
/// above. The bounds are read the way `GroundArc` reads its trail-offs: they
/// say where the form runs out, never where anything is positioned from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundCone {
    /// Half-angle from the axis, radians, from the ACTUAL angle.
    pub phi_half: f64,
    /// The theoretical apex, above the tip — negative for any real ground
    /// tool, because the relieved flat puts it `(tip_dia/2)/tan(phi_half)`
    /// below the physical tip, out in the air under the tool.
    pub z_apex: f64,
    /// The relieved flat's radius: where the flank starts, as a bound.
    pub tip_r: f64,
    /// The body OD's radius: where the flank runs out, as a bound.
    pub body_r: f64,
}

impl GroundCone {
    /// The cone's radius `h` above the tip.
    pub fn radius_at(&self, h: f64) -> f64 {
        (h - self.z_apex) * self.phi_half.ptan()
    }

    /// The height above the tip where the cone reaches radius `r` — the
    /// inverse of `radius_at`, and the conversion every placement runs.
    pub fn height_at_radius(&self, r: f64) -> f64 {
        self.z_apex + r / self.phi_half.ptan()
    }

    /// Where the cutting flank starts: the flat's edge. A bound read from a
    /// clearance record, like the pilot bounding the rounder's body — the
    /// flank exists above here, and nothing is positioned from it.
    pub fn cutting_bottom(&self) -> f64 {
        self.height_at_radius(self.tip_r)
    }

    /// Where the flank runs out into the body OD — the cutting top: above
    /// here the tool is shank and removes nothing.
    pub fn flank_top(&self) -> f64 {
        self.height_at_radius(self.body_r)
    }
}

impl Tool {
    /// The ground arc, for the gauging side. `None` for every tool that is not
    /// a corner rounder — which is every tool whose form is not an arc, so
    /// there is nothing to hand over.
    pub fn ground_arc(&self) -> Option<GroundArc> {
        let rd = self.rounder.as_ref()?;
        let r = rd.arc_radius();
        Some(GroundArc {
            r,
            rc: rd.effective_radius() + r,
            zc: rd.gauge_z - r,
            trail_wall: rd.trail_wall,
            trail_face: rd.trail_face,
        })
    }

    /// The ground cone, for the gauging side. `None` for every tool that is
    /// not a chamfer mill.
    pub fn ground_cone(&self) -> Option<GroundCone> {
        let ch = self.chamfer.as_ref()?;
        let phi_half = ch.phi_half();
        Some(GroundCone {
            phi_half,
            z_apex: ch.gauge_z - (ch.gauge_dia / 2.0) / phi_half.ptan(),
            tip_r: ch.tip_dia / 2.0,
            body_r: self.dia / 2.0,
        })
    }
}

/// **The declared body above a rod's flutes** — the K2 ladder
/// (DECISIONS.md → "K2, the tool body", fork 4A). It lives ON `Silhouette` as
/// data BESIDE the arms rather than folded into their points, because the
/// narrowing test is a question about the CUTTING envelope: a shank narrower
/// than the flutes removes nothing, so folding it into a polygon would trip
/// the undercut fence for a surface that never cuts. `radius_at` reads it,
/// and since slice 2 so does `cutting_top()` — a declared flute top IS where
/// cutting ends (fork 5A), so the stamp truncates there — while `narrowing()`
/// keeps reading the envelope alone. Slice 0 built the shank rung; slice 1
/// added the holder rungs above the stickout plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyLadder {
    /// The shank rung — where the flutes end and the shank radius above them.
    /// `None` when the program declares holder rungs without a `flute_len`:
    /// the rod stays its cutting radius all the way to the nose face, which is
    /// the rod's own assumption, not a guess this ladder adds.
    pub shank: Option<ShankRung>,
    /// The declared holder above the stickout plane (slice 1).
    pub holder: Option<HolderRungs>,
}

/// Where the flutes end and what stands above them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShankRung {
    /// Where the flutes end, above the tip — the rung's bottom.
    pub flute_top: f64,
    /// The shank's radius from there up. When the program declares a
    /// `flute_len` but no `shank_dia`, this is the cutting radius — the rod's
    /// own assumption, now bounded instead of infinite in kind.
    pub shank_r: f64,
    /// A relieved neck between the flutes and the shank, as `(top, radius)` —
    /// its BOTTOM is `flute_top` above and is never stored twice
    /// (`DECISIONS.md` → "K2's neck fields…", fork 3A). Living inside this
    /// struct is what makes "a neck at a height that is not the flute top" and
    /// "a neck with no flutes" unrepresentable rather than merely rejected: the
    /// `comp` enum's law, one rung further in.
    pub neck: Option<(f64, f64)>,
}

/// The declared holder: nose cylinder from its face at the stickout plane,
/// body above. Tier-2 BODY data like every K2 field — read by the
/// certificates, never positioned from. The `holder=` label is deliberately
/// not here: it is provenance, and nothing geometric ever resolves it.
///
/// **Its catalog twin, and which way the traffic runs** (Phase 8's serial
/// slice 0). `assembly::Holder` describes the same physical object in the
/// optimizer's own terms — unplaced, with `gauge_len`/`compliance`/`cost` that
/// no program declares, and with its projection as the search variable. Neither
/// type absorbs the other, because a search candidate and a placed body are
/// different kinds. What connects them is one conversion in one direction:
/// `assembly::assembly_ladder` builds this struct, and `assembly::tool_line`
/// emits the declaration an author reviews — the catalog law's own
/// requirement, since a catalog is "a search space, never a gauging source".
/// The fill-and-parse pin over there is what stops the two spellings drifting;
/// if you add a fact here, that test is where its absence over there will
/// surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HolderRungs {
    /// The nose face's height above the tip — the declared `stickout`, where
    /// the tool ends and the holder begins.
    pub nose_face: f64,
    /// The nose's radius from its face up.
    pub nose_r: f64,
    /// `(nose_top, body_r)`: where the nose ends and the body radius above it.
    /// Present together or not at all — the parse-time pairing error — so a
    /// declared nose length always has the body it exists to seat.
    pub body: Option<(f64, f64)>,
}

impl BodyLadder {
    /// The ladder's answer at `h` above the tip, or `None` below every rung —
    /// the cutting envelope's territory, which each rod arm answers itself.
    /// The holder is checked first: its rungs seat at the stickout plane,
    /// which the claim checks hold at or above the flute top, and the holder
    /// swallows whatever the tool is inside it.
    pub fn radius_at(&self, h: f64) -> Option<f64> {
        if let Some(hr) = &self.holder {
            if h >= hr.nose_face {
                return Some(match hr.body {
                    Some((nose_top, body_r)) if h >= nose_top => body_r,
                    _ => hr.nose_r,
                });
            }
        }
        match self.shank {
            // The neck first: it occupies `[flute_top, neck_top)`, and the
            // shank picks up above it. Answered at the DECLARED (narrower)
            // radius, which is what this ladder has always done for a body
            // field — `radius_at`'s own doc anticipated this tool. The drawn
            // profile's relieved zone deliberately answers its band's WIDEST
            // instead, and the two must stay different: a drawing above the
            // cutting band is inferred, a declared field is stated.
            Some(s) if h >= s.flute_top => Some(match s.neck {
                Some((neck_top, neck_r)) if h < neck_top => neck_r,
                _ => s.shank_r,
            }),
            _ => None,
        }
    }

    /// The heights at which this ladder's radius can change — its rungs'
    /// own bottoms. Published so a consumer that needs piecewise-constant
    /// BANDS (the keep-out check) can take the breakpoints from here instead
    /// of restating the ladder's layout in a second place.
    pub fn rung_heights(&self) -> Vec<f64> {
        let mut hs = Vec::new();
        if let Some(s) = self.shank {
            hs.push(s.flute_top);
            // The neck's TOP is a second breakpoint; its bottom is the flute
            // top already pushed. A consumer banding the body (the keep-out
            // check) therefore sees the narrow band without knowing a neck
            // exists.
            if let Some((neck_top, _)) = s.neck {
                hs.push(neck_top);
            }
        }
        if let Some(hr) = &self.holder {
            hs.push(hr.nose_face);
            if let Some((nose_top, _)) = hr.body {
                hs.push(nose_top);
            }
        }
        hs
    }

    /// The widest declared rung — what the ladder contributes to
    /// `Silhouette::max_radius`.
    pub fn max_radius(&self) -> f64 {
        // The neck is claim-checked narrower than the flutes, but not against
        // the shank — a reduced-shank cutter is odd hardware, not impossible
        // hardware — so it is taken into the max rather than assumed under it.
        let shank = self.shank.map_or(0.0, |s| {
            s.shank_r.max(s.neck.map_or(0.0, |(_, neck_r)| neck_r))
        });
        let holder = self.holder.map_or(0.0, |hr| {
            hr.body.map_or(hr.nose_r, |(_, body_r)| hr.nose_r.max(body_r))
        });
        shank.max(holder)
    }
}

/// **The one home for what a tool IS** — its shape in the meridian half-plane
/// (DECISIONS.md → "the profile silhouette…", slice 1). Every consumer that
/// asks how wide the tool is at a height, or how far its tip hangs below the
/// coordinates the emitter writes, derives it from here: the analytic foul
/// lint's lattice (`stock`), the voxel stamp (`vox`), and the tip convention
/// the entry restriction reads. They agreed by hand before this type existed,
/// which is exactly the drift the fold removes — an error and a flag must not
/// be able to disagree about the tool.
///
/// `h` is always measured **above the tip**, never from the emitted
/// coordinate; `drop()` is what separates the two.
#[derive(Clone, Debug, PartialEq)]
pub enum Silhouette {
    /// Flat-tipped rod, constant radius. Emitted coordinates are the tip.
    /// `body` is the declared K2 ladder above the flutes, when there is one.
    Flat { r: f64, body: Option<BodyLadder> },
    /// Ball-tipped rod: below the ball's equator the tool is narrower than its
    /// nominal radius, and treating it as a full cylinder there would invent
    /// contact the tool cannot make. Emitted coordinates are the ball CENTER
    /// (the emission convention for surface ops). `body` as on `Flat`.
    Ball { r: f64, body: Option<BodyLadder> },
    /// A custom profile — `(radius, height-above-tip)` points, sorted by height
    /// at parse time with straight segments between them (arcs and Beziers were
    /// flattened there). Authoritative where it reaches; above its top vertex
    /// the tool continues as its nominal `shank_r`. Emitted coordinates are the
    /// tip.
    Profile { pts: Vec<Pt>, shank_r: f64 },
    /// A corner rounder's BODY: relieved pilot, the ground arc tessellated,
    /// the shoulder, then the shank — carried in the same
    /// `(radius, height-above-tip)` form `Profile` uses, so every consumer that
    /// walks a meridian polygon works unchanged.
    ///
    /// This arm is deliberately the tool's SOLID and nothing more. The arc is
    /// exact where it cuts and tessellated where it is only a volume model, and
    /// this is the tessellated half: the exact half lives on `Tool::ground_arc`,
    /// where the pilot is not visible. `cut_top` is where the cutting edge
    /// actually runs out, which sits BELOW the emitted datum by the face-end
    /// trail-off. Emitted coordinates are the **virtual sharp corner**, so
    /// `drop()` is a whole `gauge_z` — the same rule as `Ball`, whose emitted
    /// coordinate is likewise its centre of curvature, with the centre moved
    /// off the axis.
    Rounder { pts: Vec<Pt>, shank_r: f64, cut_top: f64, gauge_z: f64 },
    /// A chamfer mill's BODY: the relieved tip flat, any tip land, then the
    /// straight flank to the body OD — derived from the declared numbers like
    /// the rounder's, and carried in the same `(radius, height-above-tip)`
    /// form. A cone flank is straight, so unlike the arc there is nothing to
    /// tessellate: the polygon is exact. `cut_top` is the flank run-out, above
    /// which the tool is shank and removes nothing. Emitted coordinates are
    /// the physical TIP — the surface a presetter actually finds — so `drop()`
    /// is zero; the trap this tool is famous for is doing APEX math from the
    /// tip, not referencing the tip, and the apex offset lives on `GroundCone`
    /// where the placement math reads it.
    Chamfer { pts: Vec<Pt>, shank_r: f64, cut_top: f64 },
}

impl Silhouette {
    /// Derive a tool's silhouette. A profile is authoritative when present; with
    /// none, the name decides — the same free-form compare
    /// `require_flat_bottom_tool` and the `Finish` arms use. Weak, but one
    /// convention beats two.
    pub fn of(tool: &Tool) -> Silhouette {
        let r = tool.dia / 2.0;
        // A corner rounder's body is DERIVED from its declared numbers, never
        // drawn — the entry's fork 2, where reading the arc back out of a
        // rendering was the rejected alternative.
        if let (Some(rd), Some(arc)) = (&tool.rounder, tool.ground_arc()) {
            let (pts, shank_r) = rounder_body(rd, &arc, r);
            return Silhouette::Rounder {
                // A declared shank on a form tool is rejected narrower than the
                // body OD at parse (no shoulder-length record to bound the
                // step down), so this override can only widen the body.
                pts,
                shank_r: tool.shank_dia.map_or(shank_r, |d| d / 2.0),
                cut_top: arc.cutting_top(),
                gauge_z: arc.gauge_z(),
            };
        }
        // A chamfer mill's body is likewise derived, and the cross-errors
        // guarantee it never coexists with a rounder record.
        if let Some(cone) = tool.ground_cone() {
            let (pts, shank_r) = chamfer_body(&cone);
            return Silhouette::Chamfer {
                pts,
                shank_r: tool.shank_dia.map_or(shank_r, |d| d / 2.0),
                cut_top: cone.flank_top(),
            };
        }
        match &tool.profile {
            // A declared `shank_dia` replaces the `max()` guess — the fix the
            // corner-rounder entry deferred "when it bites": a shank narrower
            // than the flutes is finally expressible, and it lives HERE rather
            // than in `pts` so `narrowing()`'s undercut fence never reads it
            // (a shank removes nothing, so it is not an undercut).
            //
            // A declared `cut_top` clips the polygon (slice 2, fork 3A):
            // `pts` is the CUTTING envelope — what the stamp may claim was
            // removed — which is the rounder's own convention, its relieved
            // shoulder likewise living outside `pts`. The drawn relief above
            // the band moves into `shank_r`, so `radius_at` still answers the
            // body there at the relief's widest (a declared shank narrower
            // than the relief was rejected at parse); a narrowing above the
            // band stops tripping the undercut fence, which is the fence's
            // own reasoning — relief removes nothing.
            Some(pts) => match tool.cut_top {
                Some(ct) if ct < pts[pts.len() - 1].y - 1e-12 => {
                    let relief_r = profile_max_radius_from(pts, ct);
                    Silhouette::Profile {
                        shank_r: tool.shank_dia.map_or(relief_r.max(r), |d| d / 2.0),
                        pts: clip_profile_at(pts, ct),
                    }
                }
                _ => Silhouette::Profile {
                    shank_r: tool.shank_dia.map_or(pts[pts.len() - 1].x.max(r), |d| d / 2.0),
                    pts: pts.clone(),
                },
            },
            None => {
                // The K2 ladder: only a declared `flute_len` gives the shank a
                // bottom, so `shank_dia` without one is rejected at parse and
                // an undeclared body stays exactly today's semi-infinite rod.
                let shank = tool.flute_len.map(|fl| ShankRung {
                    flute_top: fl,
                    shank_r: tool.shank_dia.map_or(r, |d| d / 2.0),
                    // The neck seats on the flute top by construction — the
                    // field pair states a length and a diameter, never a
                    // height, so there is no second place for its bottom to
                    // be wrong. Built defensively like the holder below: the
                    // claim checks reject half a neck and a neck without
                    // flutes, so through the parser both are always present
                    // together.
                    neck: match (tool.neck_dia, tool.neck_len) {
                        (Some(nd), Some(nl)) => Some((fl + nl, nd / 2.0)),
                        _ => None,
                    },
                });
                // The holder rungs seat at the stickout plane (slice 1). Built
                // defensively rather than by `expect`: the claim checks reject
                // a nose without a stickout and a lopsided nose-length/body
                // pair at parse, so through the parser these `match`es always
                // take the declared shape.
                let holder = tool.stickout.and_then(|so| {
                    tool.holder_nose_dia.map(|nd| HolderRungs {
                        nose_face: so,
                        nose_r: nd / 2.0,
                        body: match (tool.holder_nose_len, tool.holder_body_dia) {
                            (Some(nl), Some(bd)) => Some((so + nl, bd / 2.0)),
                            _ => None,
                        },
                    })
                });
                let body = (shank.is_some() || holder.is_some())
                    .then_some(BodyLadder { shank, holder });
                if tool.name == "Ballmill" {
                    Silhouette::Ball { r, body }
                } else {
                    Silhouette::Flat { r, body }
                }
            }
        }
    }

    /// Every height at which this silhouette's radius can change: the K2
    /// ladder's rungs, plus a drawn profile's own vertices. One home for the
    /// breakpoints, so a consumer banding the body does not re-derive where
    /// the steps are (`DECISIONS.md` → "V2, the fixture…").
    pub fn rung_heights(&self) -> Vec<f64> {
        match self {
            Silhouette::Flat { body, .. } | Silhouette::Ball { body, .. } => {
                body.as_ref().map(|b| b.rung_heights()).unwrap_or_default()
            }
            Silhouette::Profile { pts, .. } => pts.iter().map(|p| p.y).collect(),
            Silhouette::Rounder { pts, cut_top, .. } | Silhouette::Chamfer { pts, cut_top, .. } => {
                let mut hs: Vec<f64> = pts.iter().map(|p| p.y).collect();
                hs.push(*cut_top);
                hs
            }
        }
    }

    /// The tool's meridian polygon, for every consumer that walks one. `None`
    /// for the two rods, which have a closed form instead.
    pub fn body_pts(&self) -> Option<(&[Pt], f64)> {
        match self {
            Silhouette::Flat { .. } | Silhouette::Ball { .. } => None,
            Silhouette::Profile { pts, shank_r } => Some((pts, *shank_r)),
            Silhouette::Rounder { pts, shank_r, .. } => Some((pts, *shank_r)),
            Silhouette::Chamfer { pts, shank_r, .. } => Some((pts, *shank_r)),
        }
    }

    /// How wide the tool is `h` above its tip. On a rod with a declared body,
    /// the ladder answers above the flutes: at the flute top and up the tool
    /// is its shank — wider than the flutes on a stub cutter in a fat
    /// holder-side shank, narrower on a long-reach neck — and from the
    /// stickout plane up it is the declared holder, nose then body. The lint
    /// that samples this finally sees the body instead of a constant-radius
    /// guess.
    pub fn radius_at(&self, h: f64) -> f64 {
        match self {
            Silhouette::Flat { r, body } => body
                .as_ref()
                .and_then(|b| b.radius_at(h))
                .unwrap_or(*r),
            Silhouette::Ball { r, body } => {
                body.as_ref().and_then(|b| b.radius_at(h)).unwrap_or_else(|| {
                    if h < *r {
                        (r * r - (r - h) * (r - h)).sqrt()
                    } else {
                        *r
                    }
                })
            }
            _ => {
                let (pts, shank_r) = self.body_pts().expect("non-rod arms carry a meridian polygon");
                // Sorted by height with straight segments between, so
                // bracketing plus one lerp is exact.
                let first = pts[0];
                if h <= first.y {
                    return first.x;
                }
                // A relieved surface meets the arc in a radial STEP — two
                // vertices at one height — so more than one segment can contain
                // `h`, and the body is their UNION. Taking the first match
                // would answer with the relieved side of a step and quietly
                // under-state the solid, which is the wrong direction for a
                // foul lint. Sorted by height, so the containing segments are
                // contiguous and the walk still stops early.
                let mut widest: Option<f64> = None;
                for pair in pts.windows(2) {
                    let (lo, hi) = (pair[0], pair[1]);
                    if h < lo.y {
                        break;
                    }
                    if h > hi.y {
                        continue;
                    }
                    let r = if (hi.y - lo.y).abs() < 1e-12 {
                        lo.x.max(hi.x)
                    } else {
                        lo.x + (hi.x - lo.x) * (h - lo.y) / (hi.y - lo.y)
                    };
                    widest = Some(widest.map_or(r, |w: f64| w.max(r)));
                }
                widest.unwrap_or(shank_r)
            }
        }
    }

    /// The widest the tool ever gets — the swept radius the rod stamps and every
    /// bounding box pads by.
    pub fn max_radius(&self) -> f64 {
        match self {
            Silhouette::Flat { r, body } | Silhouette::Ball { r, body } => {
                body.map_or(*r, |b| r.max(b.max_radius()))
            }
            _ => {
                let (pts, shank_r) = self.body_pts().expect("non-rod arms carry a meridian polygon");
                pts.iter().map(|p| p.x).fold(shank_r, f64::max)
            }
        }
    }

    /// How far the tip hangs BELOW the emitted coordinate: a ball's lower
    /// hemisphere by a radius, a corner rounder's whole `gauge_z` — both for
    /// the same reason, that the emitted coordinate is the tool's centre of
    /// curvature rather than its tip — and everything else nothing.
    pub fn drop(&self) -> f64 {
        match self {
            Silhouette::Ball { r, .. } => *r,
            Silhouette::Rounder { gauge_z, .. } => *gauge_z,
            // A chamfer mill emits at its physical tip: the tip is what a
            // presetter hands back, and the apex the catalogue draws is a
            // derived fiction in the air below it — referenced only through
            // `GroundCone`, never as an emission datum.
            Silhouette::Flat { .. } | Silhouette::Profile { .. } | Silhouette::Chamfer { .. } => 0.0,
        }
    }

    /// Whether this is one of the two closed-form rod arms. Since slice 2 a
    /// rod is not necessarily semi-infinite — a declared `flute_len` caps its
    /// cutting span — so upward-closure is `cutting_top().is_none()`'s claim,
    /// not this one's: this asks about the SHAPE family (constant-radius
    /// closed forms vs a meridian polygon), nothing more.
    pub fn is_rod(&self) -> bool {
        matches!(self, Silhouette::Flat { .. } | Silhouette::Ball { .. })
    }

    /// How high above the tip the tool still CUTS. `None` for an undeclared
    /// rod — it cuts as high as it reaches, which is what makes its sweep
    /// upward-closed (DECISIONS.md → "the profile silhouette…"). `Some(h)`
    /// for a profile: at its top vertex the cutting edge ends and the shank
    /// begins, and the shank removes nothing, so the stamp stops there and
    /// the column above it becomes a question for the foul lint instead of
    /// an answer.
    ///
    /// A rod with a DECLARED `flute_len` answers it (K2 slice 2, fork 5A):
    /// the flutes are the cutting span, so the stamp truncates there, the
    /// field derives `upward_closed` false, `strategy=rest`/`regions`
    /// decline on such prefixes exactly as form-cutter programs do, and the
    /// rod gains the foul lint's field voice — the gate is a truncated
    /// silhouette. A ladder with holder rungs but no `flute_len` stays
    /// `None`: cutting to the nose face is the rod's own assumption, not a
    /// top this model adds.
    pub fn cutting_top(&self) -> Option<f64> {
        match self {
            Silhouette::Flat { body, .. } | Silhouette::Ball { body, .. } => {
                body.as_ref().and_then(|b| b.shank.map(|s| s.flute_top))
            }
            Silhouette::Profile { pts, .. } => Some(pts[pts.len() - 1].y),
            // A rounder's cutting edge runs out where the ARC does, which is
            // below its top vertex twice over: the shoulder above the arc is
            // relieved clearance, and the arc itself may trail off short of
            // tangency. Everything above this height removes nothing.
            Silhouette::Rounder { cut_top, .. } => Some(*cut_top),
            // A chamfer mill's flank runs out into the body OD; shank above.
            Silhouette::Chamfer { cut_top, .. } => Some(*cut_top),
        }
    }

    /// The first place the silhouette NARROWS going up, as
    /// `(height, radius below, radius above)`.
    ///
    /// A necked tool — T-slot, dovetail, Woodruff — is the one class that can
    /// cut an undercut, and its swept solid is not upward-closed even within a
    /// single stamp. `TAXONOMY.md` rows 2.10/2.11 call form cutters `context`
    /// for exactly that reason; the voxel walker rejects to stamp one by name
    /// rather than feeding a lemma a shape that violates it.
    /// Only the drawn profile is checked. A shank narrower than the cutting
    /// edge IS expressible now (K2's `shank_dia` replaced the
    /// `max(profile top, nominal)` guess) — and it is deliberately NOT read
    /// here: a shank removes nothing, so it is not an undercut, and that is
    /// exactly why the ladder lives beside the points instead of in them
    /// (DECISIONS.md → "K2, the tool body", fork 4A).
    ///
    /// **This is a question about the CUTTING envelope, and it used to ask the
    /// body.** A corner rounder's shoulder is relieved — ground back so it does
    /// not rub the finished face — which reads as a narrowing and would make
    /// the tool unstampable, the field incomplete, and `strategy=rest` reject.
    /// Relief removes nothing, so it is not an undercut: below `cutting_top()`
    /// a rounder is monotone (relieved pilot, then an arc flaring outward and
    /// up), and everything above is clearance the stamp already truncates away.
    /// So the `Rounder` arm answers `None` by construction rather than by
    /// inspecting a body it has no business reading (DECISIONS.md → "the corner
    /// rounder gets an operation").
    pub fn narrowing(&self) -> Option<(f64, f64, f64)> {
        let Silhouette::Profile { pts, .. } = self else {
            return None; // a ball widens then holds; a flat rod is constant;
                         // a rounder's cutting envelope is monotone (above);
                         // a chamfer mill is a flat then a widening flank —
                         // monotone by construction, same reasoning
        };
        pts.windows(2).find_map(|pair| {
            let (lo, hi) = (pair[0], pair[1]);
            (hi.x < lo.x - 1e-12).then_some((hi.y, lo.x, hi.x))
        })
    }
}

/// What a declared corner rounder has to satisfy before it is a tool at all.
///
/// Checked at parse rather than at validate because every one of these is a
/// claim about the TOOL, true or false with no operation in sight — the same
/// place an SVG profile's zero radius is caught.
pub(super) fn check_rounder(rd: &Rounder, dia: f64, num: usize, lineno: usize) -> Result<(), String> {
    let at = |m: String| format!("line {}: tool T{} {}", lineno + 1, num, m);
    if rd.radius <= 0.0 {
        return Err(at(format!("radius ({:.3}mm) must be positive — it is the arc the tool grinds into the part", rd.radius)));
    }
    if rd.effective_dia <= 0.0 {
        return Err(at(format!("effective_dia ({:.3}mm) must be positive", rd.effective_dia)));
    }
    if rd.pilot <= 0.0 {
        return Err(at(format!("pilot ({:.3}mm) must be positive", rd.pilot)));
    }
    if rd.radius_error.abs() >= rd.radius {
        return Err(at(format!(
            "radius_error ({:+.3}mm) is as large as the nominal radius ({:.3}mm) — a measurement that swallows the number it corrects is a transposition, not a tolerance",
            rd.radius_error, rd.radius
        )));
    }
    for (name, t) in [("trail_wall", rd.trail_wall), ("trail_face", rd.trail_face)] {
        if !(0.0..90.0).contains(&t) {
            return Err(at(format!(
                "{name} ({t:.3}deg) must be in [0, 90) — it is how far the ground arc falls SHORT of tangency, so 0 is a perfect blend and 90 is no arc at all"
            )));
        }
    }
    if rd.trail_wall + rd.trail_face >= 90.0 {
        return Err(at(format!(
            "trail_wall ({:.3}deg) + trail_face ({:.3}deg) leaves no arc between them: the quarter round is 90deg total",
            rd.trail_wall, rd.trail_face
        )));
    }
    // The relief is the whole reason `pilot` is declared, and a pilot that is
    // not relieved is either rubbing the finished wall or two numbers that got
    // swapped on the way in. This is the error that keeps the catalogue
    // identity `rc - R == pilot/2` from ever looking like a usable offset.
    if rd.pilot >= rd.effective_dia {
        return Err(at(format!(
            "pilot ({:.3}mm) is at or above effective_dia ({:.3}mm), so it is not relieved: the pilot must sit UNDER the virtual cylinder the arc is tangent to or it rubs the finished wall.\n  On a catalogue drawing these two are equal — that identity is exactly the trap, and it is why the axis rides effective_dia/2 off the wall and never the pilot.\n  If the tool really is ground this way, the likelier reading is that the two numbers were transposed",
            rd.pilot, rd.effective_dia
        )));
    }
    // `gauge_z` is measured above the TIP, so the arc centre `zc = gauge_z - R`
    // has to clear it — otherwise the arc starts below the end of the tool.
    let r = rd.arc_radius();
    if rd.gauge_z <= r {
        return Err(at(format!(
            "gauge_z ({:.3}mm) is at or below the arc radius ({:.3}mm), which puts the arc centre at or under the tip: there is no pilot left below the arc",
            rd.gauge_z, r
        )));
    }
    if dia < rd.effective_dia {
        return Err(at(format!(
            "dia ({:.3}mm) is the tool's BODY OD and is smaller than effective_dia ({:.3}mm) — the body cannot be narrower than the cylinder its own cutting arc is tangent to",
            dia, rd.effective_dia
        )));
    }
    Ok(())
}

/// A corner rounder's body as a meridian polygon, derived from the declared
/// numbers: relieved pilot, the ground arc in chords, the shoulder, the shank.
///
/// The chords are the TESSELLATED half of the entry's split — this polygon is
/// only ever a volume model (the foul lattice, the voxel stamp), and it is
/// walked at a tolerance well under a voxel cell. Nothing positions from it;
/// the exact arc is `Tool::ground_arc`.
///
/// The two radial STEPS are real and deliberate. At the arc's lower end the
/// pilot is relieved, so the body jumps outward from `pilot/2` to where the arc
/// actually starts — and that jump IS the relief plus the wall-end trail-off,
/// the thing traditional CAM cannot see. At the upper end the shoulder may sit
/// inboard of the arc's outer extent, which is relief again.
pub(super) fn rounder_body(rd: &Rounder, arc: &GroundArc, od_r: f64) -> (Vec<Pt>, f64) {
    let pilot_r = rd.pilot / 2.0;
    let start = arc.at(rd.trail_wall);
    let end = arc.at(90.0 - rd.trail_face);

    let mut pts = vec![Pt { x: pilot_r, y: 0.0 }];
    if start.y > 1e-12 {
        pts.push(Pt { x: pilot_r, y: start.y }); // the relieved pilot, full height
    }
    pts.push(start); // the step onto the arc

    // Chord the arc at a tolerance far below a cell: for a sagitta `t` on
    // radius `r` the step is `2·acos(1 - t/r)`, and one segment is the floor so
    // a hair of arc still produces a segment rather than a gap.
    let sweep = (90.0 - rd.trail_face - rd.trail_wall).max(0.0);
    let step = 2.0 * (1.0 - (crate::CHORD_TOL_MM / arc.r).min(1.0)).pacos().to_degrees();
    let n = if step > 1e-9 { (sweep / step).ceil().max(1.0) as usize } else { 1 };
    for i in 1..=n {
        pts.push(arc.at(rd.trail_wall + sweep * (i as f64) / (n as f64)));
    }

    // The shoulder is NOT a vertex here, and that is the `Profile` convention
    // rather than an omission: `pts` is the CUTTING envelope — what the stamp
    // may claim was removed — and `shank_r` is the body above it, which removes
    // nothing and is the foul lint's business instead. Planting the shoulder in
    // `pts` would stamp a full-OD disc at the arc's top and claim the relieved
    // shoulder cut the part.
    let _ = end;
    (pts, od_r)
}

/// What a declared chamfer mill has to satisfy before it is a tool at all.
/// Parse-time for the same reason `check_rounder` is: every one of these is a
/// claim about the TOOL, true or false with no operation in sight.
pub(super) fn check_chamfer(ch: &Chamfer, dia: f64, num: usize, lineno: usize) -> Result<(), String> {
    let at = |m: String| format!("line {}: tool T{} {}", lineno + 1, num, m);
    if !(0.0 < ch.angle && ch.angle < 180.0) {
        return Err(at(format!(
            "angle ({:.3}deg) must be strictly inside (0, 180) — the INCLUDED angle of the ground cone; 180 is a flat face and 0 is a needle",
            ch.angle
        )));
    }
    if ch.angle_error.abs() >= ch.angle {
        return Err(at(format!(
            "angle_error ({:+.3}deg) is as large as the nominal angle ({:.3}deg) — a measurement that swallows the number it corrects is a transposition, not a tolerance",
            ch.angle_error, ch.angle
        )));
    }
    let actual = ch.actual_angle();
    if !(0.0 < actual && actual < 180.0) {
        return Err(at(format!(
            "angle ({:.3}deg) + angle_error ({:+.3}deg) leaves no cone: the actual included angle must stay strictly inside (0, 180)",
            ch.angle, ch.angle_error
        )));
    }
    if ch.gauge_dia <= 0.0 {
        return Err(at(format!("gauge_dia ({:.3}mm) must be positive — it is a measured diameter ON the cone", ch.gauge_dia)));
    }
    if ch.gauge_z <= 0.0 {
        return Err(at(format!(
            "gauge_z ({:.3}mm) must be positive — the gauge point is measured above the TIP, and a point at or under the tip is not on the flank",
            ch.gauge_z
        )));
    }
    if ch.tip_dia <= 0.0 {
        return Err(at(format!(
            "tip_dia ({:.3}mm) must be positive — every ground cone has a flat or broken corner at the tip, and declaring it zero would make the catalogue's sharp point the declaration",
            ch.tip_dia
        )));
    }
    if ch.tip_dia >= ch.gauge_dia {
        return Err(at(format!(
            "tip_dia ({:.3}mm) is at or above gauge_dia ({:.3}mm) — the gauge point must sit on the cone ABOVE the relieved flat, or one of the two was measured on the wrong surface",
            ch.tip_dia, ch.gauge_dia
        )));
    }
    if dia < ch.gauge_dia {
        return Err(at(format!(
            "dia ({:.3}mm) is the tool's BODY OD and is smaller than gauge_dia ({:.3}mm) — the body cannot be narrower than a measured point on its own cone",
            dia, ch.gauge_dia
        )));
    }
    // The three records have to describe ONE tool: the cone pinned by the
    // gauge point and the angle must reach `tip_dia/2` at or above the
    // physical tip. Below it, the flank would already be wider than the flat
    // at the tip plane — the gauge point, the angle, and the flat cannot all
    // be true. Tolerance covers exact-consistency float noise only.
    let phi_half = ch.phi_half();
    let z_apex = ch.gauge_z - (ch.gauge_dia / 2.0) / phi_half.ptan();
    let cutting_bottom = z_apex + (ch.tip_dia / 2.0) / phi_half.ptan();
    if cutting_bottom < -1e-9 {
        return Err(at(format!(
            "the declared cone passes below the physical tip: from gauge_dia {:.3} at gauge_z {:.3} with an actual angle of {:.3}deg, the flank is already {:.3}mm wide at the tip plane, wider than tip_dia {:.3} — the gauge point, the angle, and the tip flat cannot all be true of one tool",
            ch.gauge_dia, ch.gauge_z, actual, 2.0 * (-z_apex) * phi_half.ptan(), ch.tip_dia
        )));
    }
    Ok(())
}

/// A chamfer mill's body as a meridian polygon, derived from the declared
/// numbers: the relieved tip flat, any tip land, then the straight flank to
/// the body OD. Exact rather than tessellated — a cone flank is a segment,
/// which is the one shape the polygon speaks natively.
///
/// Same CUTTING-envelope convention as `rounder_body`: `pts` stops at the
/// flank run-out and `shank_r` is the body above it, which removes nothing.
pub(super) fn chamfer_body(cone: &GroundCone) -> (Vec<Pt>, f64) {
    let mut pts = vec![Pt { x: cone.tip_r, y: 0.0 }];
    let cb = cone.cutting_bottom();
    if cb > 1e-12 {
        // A flat ground wider than the theoretical cone leaves a short land
        // before the flank starts — the flat's radius, carried up to where
        // the cone actually reaches it.
        pts.push(Pt { x: cone.tip_r, y: cb });
    }
    pts.push(Pt { x: cone.body_r, y: cone.flank_top() });
    (pts, cone.body_r)
}

/// What a declared K2 tool body has to satisfy before it is a body at all
/// (DECISIONS.md → "K2, the tool body", slice 0). Parse-time in the
/// `check_rounder` mold: every one of these is a claim about the TOOL, true or
/// false with no operation in sight. Composition with the existing records is
/// error-shaped where two records would describe one fact — the chamfer
/// thread's law.
pub(super) fn check_body(tool: &Tool, lineno: usize) -> Result<(), String> {
    let at = |m: String| format!("line {}: tool T{} {}", lineno + 1, tool.num, m);
    for (name, v) in [
        ("flute_len", tool.flute_len),
        ("shank_dia", tool.shank_dia),
        ("neck_dia", tool.neck_dia),
        ("neck_len", tool.neck_len),
        ("stickout", tool.stickout),
        ("holder_nose_dia", tool.holder_nose_dia),
        ("holder_nose_len", tool.holder_nose_len),
        ("holder_body_dia", tool.holder_body_dia),
    ] {
        if let Some(v) = v {
            if v <= 0.0 {
                return Err(at(format!("{name} ({v:.3}mm) must be positive")));
            }
        }
    }
    if let Some(fl) = tool.flute_len {
        // The cutting top already has exactly one owner on each of these: the
        // drawing's top vertex, or the ground form's derived run-out.
        if tool.profile.is_some() {
            return Err(at(format!(
                "draws a profile and declares flute_len ({fl:.3}mm) — the drawing's top vertex owns the cutting top, and two records describing one fact is a second place for it to be wrong. The drawn extent IS the flute length"
            )));
        }
        if tool.rounder.is_some() || tool.chamfer.is_some() {
            return Err(at(format!(
                "declares flute_len ({fl:.3}mm) but its cutting top is DERIVED from the declared form (the arc's or the flank's run-out) — two records describing one fact is a second place for it to be wrong"
            )));
        }
        // A ball is ground flutes down to its tip: flutes ending inside the
        // ball would put shank where the ball's own sphere still cuts.
        if tool.name == "Ballmill" && fl < tool.dia / 2.0 {
            return Err(at(format!(
                "flute_len ({:.3}mm) is shorter than the ball radius ({:.3}mm) — the flutes cannot end inside the ball itself",
                fl, tool.dia / 2.0
            )));
        }
        if let Some(so) = tool.stickout {
            if fl > so {
                return Err(at(format!(
                    "flute_len ({fl:.3}mm) exceeds stickout ({so:.3}mm) — the flutes cannot run past where the tool leaves the holder"
                )));
            }
        }
    }
    if let Some(sd) = tool.shank_dia {
        if tool.rounder.is_some() || tool.chamfer.is_some() {
            // `dia` is these tools' declared body OD. A WIDER shank is a real
            // body fact the ladder can hold; a narrower one needs the
            // shoulder's length to say where the step down happens, and that
            // record does not exist yet — silently erasing the shoulder would
            // under-state the solid, the one direction a foul model must
            // never err.
            if sd < tool.dia {
                return Err(at(format!(
                    "shank_dia ({:.3}mm) is narrower than the declared body OD dia ({:.3}mm) — where the shoulder ends and the narrower shank begins is not declarable yet (there is no shoulder-length record), so the model would have to erase the shoulder to honor it. Declare shank_dia at or above dia, or omit it",
                    sd, tool.dia
                )));
            }
        } else if tool.profile.is_none() && tool.flute_len.is_none() {
            // On a rod the drawn top vertex does not exist, so only flute_len
            // can give the shank a bottom — without one the declared shank
            // could not be placed, and a field nothing can read is a
            // declaration that quietly does not exist.
            return Err(at(format!(
                "declares shank_dia ({sd:.3}mm) without flute_len — a shank needs a bottom, and on a rod only flute_len says where the flutes end and the shank begins"
            )));
        }
    }
    // The relieved neck (the neck-fields slice). Same shape as the holder
    // pairing below: reject what cannot be placed rather than guess where it
    // goes, and reject a field that could not mean anything.
    match (tool.neck_dia, tool.neck_len) {
        (None, None) => {}
        (nd, nl) => {
            if nd.is_none() || nl.is_none() {
                let (have, want) = if nd.is_some() { ("neck_dia", "neck_len") } else { ("neck_len", "neck_dia") };
                return Err(at(format!(
                    "declares {have} without {want} — a relieved neck is a diameter over a length, and half of one places nothing. The catalog grammar rejects the same half-neck, one file over"
                )));
            }
            let (nd, nl) = (nd.unwrap(), nl.unwrap());
            // A drawn or ground tool owns its own body: two records
            // describing one section is the chamfer thread's error law,
            // and slice 1's holder message has the same shape.
            if tool.profile.is_some() || tool.rounder.is_some() || tool.chamfer.is_some() {
                return Err(at(
                    "declares a relieved neck, but on a profile or form tool the body is a drawn or derived polygon that already says where it narrows — two records describing one section is a second place for it to be wrong. Draw the neck, or declare it on a rod"
                        .to_string(),
                ));
            }
            // The neck's bottom IS the flute top; without one there is
            // nowhere to seat it, exactly as a shank needs a bottom.
            let Some(fl) = tool.flute_len else {
                return Err(at(format!(
                    "declares a relieved neck ({nd:.3}mm over {nl:.3}mm) without flute_len — a neck sits between the flutes and the shank, and only flute_len says where the flutes end"
                )));
            };
            // "Relieved" means ground back from the cutting diameter. A neck
            // at or above it is not relief, so the field would record nothing
            // — the silence the unknown-key error exists to make
            // unshippable. NOT checked against shank_dia: a reduced shank is
            // odd hardware, not impossible hardware, and rejecting it would be
            // a law about tools rather than about declarations.
            if nd >= tool.dia {
                return Err(at(format!(
                    "neck_dia ({:.3}mm) is not narrower than the cutting diameter ({:.3}mm) — a neck is a RELIEVED section, ground back so it clears; at or above the flutes it records nothing the shank does not already say",
                    nd, tool.dia
                )));
            }
            if let Some(so) = tool.stickout {
                // `>` and not `>=`, measured: the optimizer's own floor is
                // exactly flutes + neck, so at minimum projection the neck
                // top lands precisely on the nose face (shipped data:
                // `1/4-reach` in `slim-nose` ranks at 38.100 = 12.7 + 25.4).
                // A `>=` here would reject the recommendation this project
                // prints.
                if fl + nl > so + 1e-9 {
                    return Err(at(format!(
                        "flute_len ({:.3}mm) plus neck_len ({:.3}mm) runs past stickout ({:.3}mm) — the neck would end inside the holder, where the holder's own rungs already say what stands there",
                        fl, nl, so
                    )));
                }
            }
        }
    }
    // The holder rungs (slice 1). The `holder=` label is provenance and free
    // to stand alone; every check below is about the NUMERIC fields, which
    // claim geometry the ladder must be able to hold.
    let holder_geom = tool.holder_nose_dia.is_some()
        || tool.holder_nose_len.is_some()
        || tool.holder_body_dia.is_some();
    if holder_geom {
        // The ladder's holder rungs live on the rod arms today: a drawn
        // profile and the ground forms answer their body from a meridian
        // polygon that has no holder step yet. Accepting the fields there
        // would be a declaration nothing reads — the exact silence the
        // whitelist was built to reject — so they reject until that slice,
        // the same missing-record pattern as the shoulder-length error.
        if tool.profile.is_some() || tool.rounder.is_some() || tool.chamfer.is_some() {
            return Err(at(
                "declares holder geometry, but on a profile or form tool the body is a drawn/derived polygon that has no holder step yet — the holder rungs live on the rod ladder today, and a field nothing reads is a declaration that quietly does not exist. Declare the holder on a rod tool, or omit the numeric holder fields"
                    .to_string(),
            ));
        }
        // The nose face sits at the stickout plane — without a stickout the
        // ladder cannot place the holder at all.
        if tool.stickout.is_none() {
            return Err(at(
                "declares holder geometry without stickout — the nose face sits at the stickout plane, and a holder the ladder cannot place is a declaration that quietly does not exist"
                    .to_string(),
            ));
        }
        // A length for a cylinder with no diameter cannot be placed either.
        if tool.holder_nose_dia.is_none() {
            return Err(at(
                "declares holder geometry without holder_nose_dia — the nose is the holder's first rung, and a length or a body with no nose diameter under it cannot be placed"
                    .to_string(),
            ));
        }
        // nose_len and body_dia come together, both ways: the length exists
        // to say where the body starts, and the body needs that bottom.
        match (tool.holder_nose_len, tool.holder_body_dia) {
            (Some(nl), None) => {
                return Err(at(format!(
                    "declares holder_nose_len ({nl:.3}mm) without holder_body_dia — the nose length exists to say where the body begins, and a nose top with nothing declared above it is a record nobody reads. Declare the body, or omit the length and let the nose stand for the whole holder"
                )));
            }
            (None, Some(bd)) => {
                return Err(at(format!(
                    "declares holder_body_dia ({bd:.3}mm) without holder_nose_len — a body needs a bottom, and only the nose's length says where the nose ends and the body begins"
                )));
            }
            _ => {}
        }
        if let (Some(nd), Some(bd)) = (tool.holder_nose_dia, tool.holder_body_dia) {
            if nd > bd {
                return Err(at(format!(
                    "holder_nose_dia ({nd:.3}mm) is wider than holder_body_dia ({bd:.3}mm) — the nose is the holder's narrow end; for a straight-bodied holder declare the one diameter as the nose and omit the body"
                )));
            }
        }
        // The nose grips the shank: the tool runs INTO the holder above the
        // stickout plane, so a nose narrower than what it swallows would
        // under-state the solid there — the one direction a foul model must
        // never err. On a rod the in-holder diameter is known even when
        // undeclared: the shank, or the rod's own cutting diameter.
        if let Some(nd) = tool.holder_nose_dia {
            let gripped = tool.shank_dia.unwrap_or(tool.dia);
            if nd < gripped {
                return Err(at(format!(
                    "holder_nose_dia ({:.3}mm) is narrower than the shank it grips ({:.3}mm) — the shank runs into the holder above the stickout plane, so honoring this would erase gripped steel",
                    nd, gripped
                )));
            }
        }
    }
    // The cutting band (slice 2, fork 3A). Every check is a claim about the
    // TOOL: the band bounds where the drawn profile earns contact, so it must
    // name heights the drawing actually has.
    if tool.cut_bottom.is_some() || tool.cut_top.is_some() {
        let pts = match &tool.profile {
            Some(pts) => pts,
            None => {
                // The rods and the ground forms each already have one owner
                // for where cutting starts and stops — a second record is a
                // second place for it to be wrong (the chamfer thread's law).
                let owner = if tool.rounder.is_some() || tool.chamfer.is_some() {
                    "its cutting zone is DERIVED from the declared form (the arc's or the flank's run-out)"
                } else {
                    "on a rod, flute_len says where the flutes end"
                };
                return Err(at(format!(
                    "declares a cutting band (cut_bottom/cut_top) without a drawn profile — the band says which part of a DRAWING earns contact, and {owner}"
                )));
            }
        };
        let drawn_top = pts.last().expect("a parsed profile has points").y;
        if let Some(ct) = tool.cut_top {
            if ct <= 0.0 {
                return Err(at(format!("cut_top ({ct:.3}mm) must be positive — a band ending at or under the tip cuts nothing")));
            }
            if ct > drawn_top + 1e-9 {
                return Err(at(format!(
                    "cut_top ({:.3}mm) is above the drawn profile's top vertex ({:.3}mm) — the band cannot claim cutting on steel the drawing does not state",
                    ct, drawn_top
                )));
            }
        }
        let cb = tool.cut_bottom.unwrap_or(0.0);
        if cb < 0.0 {
            return Err(at(format!("cut_bottom ({cb:.3}mm) must not be negative — heights are measured above the tip")));
        }
        let ct_eff = tool.cut_top.unwrap_or(drawn_top);
        if cb >= ct_eff - 1e-9 {
            return Err(at(format!(
                "cut_bottom ({:.3}mm) is at or above the band's top ({:.3}mm) — a band with no height earns no contact anywhere, which is a tool that cannot cut",
                cb, ct_eff
            )));
        }
        // A declared cut_top below the drawn top makes everything above it
        // relieved BODY, which `radius_at` answers as the shank: a declared
        // shank narrower than that relief would erase drawn steel — the one
        // direction a foul model must never err — and where the relief steps
        // down to the narrower shank is not a record the drawing carries.
        if let (Some(ct), Some(sd)) = (tool.cut_top, tool.shank_dia) {
            let relief_max = profile_max_radius_from(pts, ct);
            if sd / 2.0 < relief_max - 1e-9 {
                return Err(at(format!(
                    "shank_dia ({:.3}mm) is narrower than the drawn profile above cut_top ({:.3}mm wide at its widest) — the drawing states that relieved steel, so honoring the shank would erase it. Declare shank_dia at or above the relief, or omit it",
                    sd, 2.0 * relief_max
                )));
            }
        }
    }
    Ok(())
}

/// The drawn profile's widest radius at or above height `h` — the interpolated
/// radius AT `h` joins the vertices above it, so a band top mid-segment still
/// answers with the segment's width there. Zero when `h` is above the drawing.
pub(super) fn profile_max_radius_from(pts: &[Pt], h: f64) -> f64 {
    let mut widest = 0.0f64;
    for p in pts.iter().filter(|p| p.y >= h) {
        widest = widest.max(p.x);
    }
    for pair in pts.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        if lo.y <= h && h <= hi.y && hi.y - lo.y > 1e-12 {
            widest = widest.max(lo.x + (hi.x - lo.x) * (h - lo.y) / (hi.y - lo.y));
        }
    }
    widest
}

/// The drawn profile clipped at a declared `cut_top` — the cutting envelope
/// under the band (slice 2, fork 3A). A band top landing mid-segment gains
/// an interpolated boundary vertex, so the envelope still states its width
/// where it ends; vertices exactly at the band top are kept as drawn.
pub(super) fn clip_profile_at(pts: &[Pt], cut_top: f64) -> Vec<Pt> {
    let mut out: Vec<Pt> = pts.iter().copied().filter(|p| p.y <= cut_top + 1e-12).collect();
    if let Some(last) = out.last().copied() {
        if last.y < cut_top - 1e-12 {
            if let Some(w) = pts.windows(2).find(|w| w[0].y <= cut_top && w[1].y > cut_top) {
                let (lo, hi) = (w[0], w[1]);
                let x = lo.x + (hi.x - lo.x) * (cut_top - lo.y) / (hi.y - lo.y);
                out.push(Pt { x, y: cut_top });
            }
        }
    }
    out
}
