use crate::math::Portable as _;
// ---- the region: a floor, and what bounds it --------------------------------
// Phase 27 (`DECISIONS.md` → "Phase 27 opens: the region DRAFT…"). A FLOOR is
// a planar region at one height, and every stretch of its boundary is one of
// three kinds — WALL (material stands there; the tool stands off by its
// radius), OPEN (air; the tool may run off) or BRIDGED (a void in the floor
// the path runs over as if it were floor). `Floor` a `Plane` is the all-open
// case and a closed pocket's floor the all-wall case; a step, a shoulder, an
// open pocket and a floor with holes in it are the same region with the kinds
// mixed.
//
// Nothing here is declared. Every kind is DERIVED from what the project already
// says — the host feature, the blank, what stands in the region, what is sunk
// in it — and this is the one home the emitters, the stock model, the lints
// and `--outline` read. It adds no noun and no key to the grammar.

use crate::offset::Island;
use crate::parse::{derive_contour_top, derive_z_cut, signed_area, Feature, OpType, Operation, Part, Pt};
use crate::stock::{dist_to_polygon, island_outlines, plan_nesting, Nesting};

/// How one stretch of a region's OUTER boundary meets the tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeKind {
    /// The host's own wall: stand off by the tool's radius (plus allowance).
    Wall,
    /// On the blank's boundary: beyond it is air, and the tool may run off.
    Open,
}

/// What an inner boundary is to the path.
#[derive(Clone, Debug, PartialEq)]
pub enum Inner {
    /// Standing material the path goes around — Phase 26's island.
    Island { name: String, outline: Island },
    /// A declared `fixture` standing where the tool would cut (Phase 27 slice
    /// 3, fork 5A): steel the path goes around exactly as it goes around an
    /// island, its footprint grown by the keep-out margin. KEEP-OUT never
    /// needed a new word — `fixture` has been that word since V2 — it needed
    /// an emitter that read it.
    KeepOut { name: String, outline: Island },
    /// A void the tool can stand in clear of its rim (Phase 28 slice 2, fork
    /// 2A): an OPEN inner edge. `outline` is the void SHRUNK by one tool
    /// diameter — the mirror of an open outer edge pushed out by one — so the
    /// family, standing a radius off it, rides a radius INSIDE the rim: the
    /// tool overhangs the void and never crosses it at feed. "It doesn't need
    /// to go there."
    OpenVoid { name: String, outline: Island },
    /// A declared void whose mouth is in this floor, too narrow for that:
    /// shrunk by a tool diameter, nothing of it is left. The path runs straight
    /// over it, cut or not: uncut it is floor, and cut it costs air time and
    /// nothing else (fork 4A — what the engine always did, now said).
    Bridged { name: String },
}

/// The tightest place a tool must pass: between an island and a wall, or
/// between two islands. Open edges bound nothing — there is air to swing into.
#[derive(Clone, Debug, PartialEq)]
pub struct Passage {
    pub between: String,
    pub and: String,
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub host: String,
    /// The floor's height.
    pub z: f64,
    /// The outer boundary, CCW, clipped to the blank: an outline drawn 12mm
    /// past the stock and one drawn 100mm past it are the same region.
    pub outer: Vec<Pt>,
    /// One kind per edge of `outer` — edge `i` runs from vertex `i` to `i+1`.
    pub edges: Vec<EdgeKind>,
    pub inners: Vec<Inner>,
    pub narrowest: Option<Passage>,
    /// The plan footprints of the DECLARED NEIGHBOURS that closed this
    /// region's WALL edges (Phase 29 slice 1). The family's pushed boundary is
    /// clipped back out of these: a push across an open edge may cross scrap,
    /// never a neighbour.
    pub keep_clear: Vec<Vec<Pt>>,
    /// The names of those same neighbours, name-sorted as `keep_clear` is
    /// (Phase 37 slice 5): `--outline` says which feature closed an edge, so
    /// that a wall no op names is not listed as a feature nothing uses.
    pub closed_by: Vec<String>,
    /// The GIVEN voids this floor grew into (Phase 42 slice 3; fork 3A): a
    /// given void whose lens — the air a tool body can stand in,
    /// `stock::given_air_at` — straddles the outer boundary is unioned into
    /// it, the lens's edge a WALL (the body would touch the void's wall
    /// there), the authored outline's stretch inside the lens dropped, the
    /// junction a re-entrant corner the tool fillets. In declaration order.
    /// Empty for every program that gives nothing — byte for byte what the
    /// region was.
    pub grown: Vec<String>,
    /// The standing features BITTEN OUT of the outer boundary because they
    /// touch it (Phase 49 slice 1, fork 1A): a boss on the shore is the
    /// shore. Its flanks are WALL edges of `outer`; it is not in `inners`.
    /// Name-sorted, as `island_outlines` hands them over.
    pub on_the_shore: Vec<String>,
}

/// How near the blank's boundary an edge must lie to be ON it. An authored
/// coordinate against the stock's own: three decimals, like every other
/// declaration-against-declaration comparison (`stock::Z_AGREE`'s reason).
/// The containment lint's plan guard reads the same number (Phase 47 slice
/// 1): a vertex drawn on the blank's edge to three decimals is contained,
/// here and there, by the one tolerance.
pub(crate) const ON_THE_BLANK: f64 = 1e-3;

/// How far outside an edge to ask what stands there (Phase 29 slice 1). A
/// neighbour's footprint SHARES this boundary exactly — the raised ring's
/// circle IS the deck's arc — so the probe must step off it, and a hundredth
/// of a millimetre is past every coincident-edge tie while staying well inside
/// the neighbour it is asking about.
const BEYOND_THE_EDGE: f64 = 1e-2;

/// How far above this floor a neighbour's floor must sit before it counts as
/// standing: three decimals, the same declaration-against-declaration
/// comparison `ON_THE_BLANK` and `stock::Z_AGREE` make.
const STANDS_ABOVE: f64 = 1e-3;

/// A fixture's top against a floor: the same three decimals.
const KEEP_OUT_AGREE: f64 = 1e-3;

/// How far a pushed-out strip reaches back into its region so the two overlap.
const STRIP_OVERLAP: f64 = 0.1;

impl Region {
    pub fn open_edges(&self) -> usize {
        self.edges.iter().filter(|k| **k == EdgeKind::Open).count()
    }

    pub fn islands(&self) -> Vec<(&str, &Island)> {
        self.inners
            .iter()
            .filter_map(|i| match i {
                Inner::Island { name, outline } | Inner::KeepOut { name, outline } | Inner::OpenVoid { name, outline } => {
                    Some((name.as_str(), outline))
                }
                Inner::Bridged { .. } => None,
            })
            .collect()
    }

    /// What STANDS in the region — features and steel — without the voids the
    /// path merely declines to cross. A passage is between things that stand.
    pub fn standing(&self) -> Vec<(&str, &Island)> {
        self.inners
            .iter()
            .filter_map(|i| match i {
                Inner::Island { name, outline } | Inner::KeepOut { name, outline } => Some((name.as_str(), outline)),
                _ => None,
            })
            .collect()
    }

    /// What stands, as plain loops: what a row stops against and a shore
    /// pass runs along (Phase 30).
    pub fn standing_polygons(&self) -> Vec<Vec<Pt>> {
        self.standing().into_iter().map(|(_, i)| island_polygon(i)).collect()
    }

    pub fn open_voids(&self) -> Vec<&str> {
        self.inners
            .iter()
            .filter_map(|i| match i {
                Inner::OpenVoid { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The open voids' shrunk outlines as plain loops — what a removal that
    /// goes around them leaves standing (or leaves as the air it already is).
    pub fn open_void_holes(&self) -> Vec<Vec<Pt>> {
        self.inners
            .iter()
            .filter_map(|i| match i {
                Inner::OpenVoid { outline, .. } => Some(island_polygon(outline)),
                _ => None,
            })
            .collect()
    }

    pub fn keep_outs(&self) -> Vec<&str> {
        self.inners
            .iter()
            .filter_map(|i| match i {
                Inner::KeepOut { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The standing FEATURES alone — Phase 26's islands, without the steel.
    pub fn feature_islands(&self) -> Vec<&str> {
        self.inners
            .iter()
            .filter_map(|i| match i {
                Inner::Island { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn bridged(&self) -> Vec<&str> {
        self.inners
            .iter()
            .filter_map(|i| match i {
                Inner::Bridged { name } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }

    /// A bare face and a closed pocket with nothing in it are regions too, and
    /// say nothing a reader does not already know; everything else is worth a
    /// line.
    pub fn is_plain(&self) -> bool {
        self.inners.is_empty() && self.grown.is_empty() && self.on_the_shore.is_empty() && (self.open_edges() == 0 || self.open_edges() == self.edges.len())
    }

    /// The `--outline` line: the terminal's statement of what the emitters
    /// will read.
    pub fn line(&self, tool_dia: f64) -> String {
        let mut parts = vec![format!(
            "region: floor z {} — {} edge{}, {} open",
            crate::gcode::f(self.z),
            self.edges.len(),
            if self.edges.len() == 1 { "" } else { "s" },
            self.open_edges()
        )];
        let islands = self.feature_islands();
        if !islands.is_empty() {
            parts.push(format!("around {}", islands.join(", ")));
        }
        if !self.keep_outs().is_empty() {
            parts.push(format!("keeping out of {}", self.keep_outs().join(", ")));
        }
        if !self.open_voids().is_empty() {
            parts.push(format!("open onto {}", self.open_voids().join(", ")));
        }
        if !self.bridged().is_empty() {
            parts.push(format!("bridging {}", self.bridged().join(", ")));
        }
        if !self.grown.is_empty() {
            parts.push(format!("grown into the given air of {}", self.grown.join(", ")));
        }
        if !self.on_the_shore.is_empty() {
            parts.push(format!("on the shore {} (bitten out of the boundary)", self.on_the_shore.join(", ")));
        }
        if let Some(n) = &self.narrowest {
            parts.push(format!(
                "narrowest passage {}mm ({:.2} tool diameters) between {} and {}",
                crate::gcode::f(n.width),
                n.width / tool_dia,
                n.between,
                n.and
            ));
        }
        parts.join("; ")
    }
}

/// A cavity's outline as the blank sees it: the authored points where they lie
/// wholly inside the stock (every closed pocket — returned untouched), the
/// part inside the stock where they do not. One home for the clip, read by
/// `of` and by the stock model's removal.
pub fn clipped_outline(p: &Part, points: &[Pt]) -> Option<Vec<Pt>> {
    let blank = blank_of(p);
    match plan_nesting(points, &blank) {
        Nesting::Inside => None,
        _ => crate::offset::clip_to(points, &blank).into_iter().next(),
    }
}

impl Region {
    /// The boundary the path's family is eroded FROM: the region, pushed out
    /// across each OPEN edge by one tool diameter — so the first loop beside an
    /// open edge rides one radius outside the blank, the raster's own overhang
    /// — and left where it is along every WALL. Built from the clipped region,
    /// never from how far past the blank the outline happened to be drawn: 12mm
    /// past or 100mm past is the same family, the same program. `None` when the
    /// pieces do not union into one simple outline.
    ///
    /// A declared neighbour THINNER than the push lies wholly inside it (Phase
    /// 32, after the close): the clip would leave a hole, which is not a bite out of the
    /// boundary but something standing in it. It comes back in `enclosed`, and
    /// every reader takes it as what it is — an island the paths go around, a
    /// hole in what the op claims and in what it is permitted to sweep.
    pub fn family_wall(&self, tool_dia: f64) -> Option<Swath> {
        let n = self.outer.len();
        // Every edge open — a `Face` — is one outward offset of the whole
        // outline: a strip per edge cannot be unioned around a concavity
        // narrower than two diameters (a wrench's jaw throat closes into a
        // hole), and the offsetter resolves exactly that. The largest loop is
        // the boundary; what it closes over is air the family may cross.
        let whole = || {
            // Loops of one area: the first the offsetter gave.
            crate::tie::first_greatest(
                crate::offset::offset_closed_polyline_verts(&self.outer, -tool_dia).iter().map(|l| crate::offset::flatten_loop(l)).filter(|l| l.len() >= 3),
                |l| loop_area(l),
            )
        };
        if self.open_edges() == n {
            return whole().map(Swath::plain);
        }
        // **Mixed, with a declared neighbour** (Phase 29 slice 1, fork 2B).
        // Keep the one outward offset — it is what resolves a concavity
        // narrower than two diameters, and the wrench's jaw throat is exactly
        // that — then CLIP the result back out of every neighbour. A clip can
        // only REMOVE swath area, so where it is wrong the op refuses rather
        // than gouges. The strip branch below cannot serve here: it is what
        // the whole-outline offset was written to replace.
        if !self.keep_clear.is_empty() {
            let mut wall = whole()?;
            let mut enclosed = Vec::new();
            for hold in &self.keep_clear {
                match crate::offset::subtract(&wall, hold) {
                    Some(w) => wall = w,
                    None if matches!(plan_nesting(hold, &wall), Nesting::Inside) => enclosed.push(hold.clone()),
                    None => return None,
                }
            }
            return Some(Swath { wall, enclosed });
        }
        let mut wall = self.outer.clone();
        for (i, kind) in self.edges.iter().enumerate() {
            if *kind != EdgeKind::Open {
                continue;
            }
            let (a, b) = (self.outer[i], self.outer[(i + 1) % n]);
            let len = (b.x - a.x).phypot(b.y - a.y);
            if len <= 1e-9 {
                continue;
            }
            // `outer` is CCW, so the way OUT of it is to the right of travel.
            let (ux, uy) = ((b.y - a.y) / len, -(b.x - a.x) / len);
            let (nx, ny) = (ux * tool_dia, uy * tool_dia);
            // The strip starts a hair INSIDE the region: two polygons that
            // merely share an edge are not an overlap the boolean will join.
            let (ix, iy) = (ux * STRIP_OVERLAP, uy * STRIP_OVERLAP);
            let strip = [
                Pt { x: a.x - ix, y: a.y - iy },
                Pt { x: b.x - ix, y: b.y - iy },
                Pt { x: b.x + nx, y: b.y + ny },
                Pt { x: a.x + nx, y: a.y + ny },
            ];
            wall = crate::offset::union(&wall, &strip)?;
        }
        // The OPEN CORNER (Phase 58 slice 1, `DECISIONS.md` → "Phase 58 slice
        // 1…"): where two open edges meet, the
        // two strips leave the square between them unfilled, and the
        // blank's corner survives the union as a REFLEX vertex — which
        // erosion rounds, so the family's outermost level stood `r + skin`
        // off a point that is air on both sides, and a Clear with an
        // allowance left the skin on the corner for no later pass to take.
        // The fill is the sector the outward offset of a convex vertex is
        // (the `whole` branch's own rounding), chorded at `CHORD_TOL_MM`,
        // its rays turned a hair into each strip and its apex a hair inside
        // the region — two polygons that merely share an edge are not an
        // overlap the boolean will join.
        for i in 0..n {
            let prev = (i + n - 1) % n;
            if self.edges[prev] != EdgeKind::Open || self.edges[i] != EdgeKind::Open {
                continue;
            }
            let (a, v, b) = (self.outer[prev], self.outer[i], self.outer[(i + 1) % n]);
            let (l1, l2) = ((v.x - a.x).phypot(v.y - a.y), (b.x - v.x).phypot(b.y - v.y));
            if l1 <= 1e-9 || l2 <= 1e-9 {
                continue;
            }
            let (t1x, t1y) = ((v.x - a.x) / l1, (v.y - a.y) / l1);
            let (t2x, t2y) = ((b.x - v.x) / l2, (b.y - v.y) / l2);
            // A left turn is a convex vertex, where the gap is; collinear
            // or reflex, there is none.
            if t1x * t2y - t1y * t2x <= 1e-9 {
                continue;
            }
            // Outward normals — right of travel, `outer` CCW — as angles.
            let a1 = (-t1x).patan2(t1y);
            let mut a2 = (-t2x).patan2(t2y);
            while a2 < a1 {
                a2 += std::f64::consts::TAU;
            }
            let lean = STRIP_OVERLAP / tool_dia;
            let (a1, a2) = (a1 - lean, a2 + lean);
            let step = 2.0 * (1.0 - crate::CHORD_TOL_MM / tool_dia).pacos();
            let m = ((a2 - a1) / step).ceil().max(1.0) as usize;
            let mid = (a1 + a2) / 2.0;
            let mut fan = vec![Pt { x: v.x - mid.pcos() * STRIP_OVERLAP, y: v.y - mid.psin() * STRIP_OVERLAP }];
            for k in 0..=m {
                let ang = a1 + (a2 - a1) * k as f64 / m as f64;
                fan.push(Pt { x: v.x + ang.pcos() * tool_dia, y: v.y + ang.psin() * tool_dia });
            }
            wall = crate::offset::union(&wall, &fan)?;
        }
        Some(Swath::plain(wall))
    }
}

/// What `Region::family_wall` hands back: the boundary a family erodes from,
/// and the declared neighbours standing wholly inside it (a wall thinner than
/// the push) — islands of the swath, never part of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Swath {
    pub wall: Vec<Pt>,
    pub enclosed: Vec<Vec<Pt>>,
}

impl Swath {
    fn plain(wall: Vec<Pt>) -> Self {
        Swath { wall, enclosed: Vec::new() }
    }

    /// The enclosed neighbours as the offsetter's islands.
    pub fn enclosed_islands(&self) -> Vec<Island> {
        self.enclosed.iter().map(|l| Island::Loop(l.clone())).collect()
    }
}

/// The region an op works, or `None` for an op that works no floor: a `Floor`
/// (its `Plane`, its `Face`, or its `ProfilePocket`'s floor), and a `Clear` on
/// a `ProfilePocket` (that pocket's floor).
pub fn of(p: &Part, op: &Operation) -> Option<Region> {
    let host = p.features.get(&op.feature_name)?;
    let s = &p.stock;
    let blank = blank_of(p);
    let (z, outer) = match (op.op_type, host) {
        (OpType::End | OpType::Clear, Feature::Plane { z }) => (*z, blank),
        (OpType::Clear | OpType::End, Feature::ProfilePocket { points, z, extent, .. }) => {
            let floor = derive_z_cut(derive_contour_top(s.z, *z), *extent);
            let ccw = crate::ops::clear::make_ccw(points);
            // Wholly inside the blank — every closed pocket in the corpus — the
            // outline is returned as authored, untouched by the clipper.
            let outer = match plan_nesting(&ccw, &blank) {
                Nesting::Inside => ccw,
                _ => clipped_outline(p, &ccw)?,
            };
            (floor, outer)
        }
        // A `Face` (Phase 28): its own outer loop, clipped to the blank like any
        // outline that runs past it. Its edges are judged below like a
        // pocket's would be, and then opened: nothing about a face's boundary
        // is a wall unless something STANDS there, and what stands is an
        // island or a bite (`split_islands`), never an edge kind.
        (OpType::End | OpType::Clear, Feature::Face { z, loops }) => {
            let outer = match plan_nesting(&loops[0], &blank) {
                Nesting::Inside => loops[0].clone(),
                _ => clipped_outline(p, &loops[0])?,
            };
            (*z, outer)
        }
        _ => return None,
    };
    let is_face = matches!(host, Feature::Face { .. });
    // The tool decides what is a gap (fork 2A). No tool, no claim: bridged.
    let tool_dia = p.tools.get(&op.tool_num).map_or(f64::INFINITY, |t| t.dia);

    // **The floor grows into the air a tool body can stand in** (Phase 42
    // slice 3, fork 3A; `DECISIONS.md` → "Phase 42 slice 3…"). The user's
    // reframe: the pocket as intended does not exist; a larger void
    // containing it does. Derived, never drawn — "islands are never
    // declared" is the precedent: containment derived the island, and
    // straddling a given void derives this. The authored outline stays the
    // boundary that OWNS material; the lens of every GIVEN void whose floor
    // piece STRADDLES it (`stock::given_air_at`, the body-aware free region
    // at this floor) is unioned in (`offset::union`, Phase 27 slice 2's
    // helper). Wholly inside, the void stays BRIDGED below, unchanged;
    // apart, nothing; a void NOT given is a wall, as ever — its air is
    // forbidden until the program says it is there. A `Plane` has no outline
    // to grow (its region is the blank, every edge open).
    let mut outer = outer;
    let mut grown: Vec<String> = Vec::new();
    let mut lens_edges: Vec<Vec<Pt>> = Vec::new();
    if !p.given.is_empty() && !matches!(host, Feature::Plane { .. }) {
        for lens in crate::stock::given_air_at(p, op, z) {
            let mut took = false;
            for piece in &lens.floor {
                if plan_nesting(piece, &outer) != Nesting::Straddles {
                    continue;
                }
                if let Some(u) = crate::offset::union(&outer, piece) {
                    outer = u;
                    lens_edges.push(piece.clone());
                    took = true;
                }
            }
            if took {
                grown.push(lens.void.clone());
            }
        }
    }
    // **A boss standing ON the boundary is the shore, not an island** (Phase
    // 49 slice 1, fork 1A; `DECISIONS.md` → "Phase 49 slice 1…"). A rim boss
    // imported from a STEP sits on the blank's silhouette to half a micron.
    // Read as an island, its loop a radius OUT and the pushed open edge's
    // loop a radius IN land on one circle, and the offsetter hands the
    // outermost level back as one stub per chord (measured: 28 stubs, 72
    // lifts, 18 s on a ⌀100 fixture; 426 lifts a level on the real part).
    // The reading that already exists for a STRADDLER — a bite out of the
    // wall, `split_islands` — is the reading for a toucher: its outline is
    // subtracted from the boundary HERE, before the edges are judged, so its
    // flanks are closed by `closer` below like any neighbour's and the edge
    // it touched with stops being anything. Touching is within the one
    // geometric tolerance: the measured knife-edge is a hundredth either way.
    // A bite the boolean cannot take leaves the island reading as it was.
    let mut on_the_shore: Vec<String> = Vec::new();
    let mut islands_in: Vec<(&str, Island)> = island_outlines(p, &op.feature_name);
    islands_in.retain(|(name, island)| {
        let poly = island_polygon(island);
        if !poly.iter().any(|q| dist_to_polygon(&outer, q.x, q.y) <= crate::CHORD_TOL_MM) {
            return true;
        }
        match crate::offset::subtract(&outer, &poly) {
            Some(bitten) => {
                outer = bitten;
                on_the_shore.push(name.to_string());
                false
            }
            None => true,
        }
    });
    // An edge of the grown outline that lies on a lens's own boundary: the
    // body would touch the void's wall beyond it.
    let on_lens = |q: Pt| lens_edges.iter().any(|l| dist_to_polygon(l, q.x, q.y) <= ON_THE_BLANK);

    // **An edge is CLOSED when material that outlives this operation stands
    // immediately beyond it** (Phase 29 slice 1, fork 1). Probed at three
    // interior stations a hair outside the edge; the corners are skipped
    // because the outward normal is ambiguous there. ANY station finding a
    // neighbour closes the edge, which is the conservative direction: an edge
    // wrongly called a wall costs a pass, an edge wrongly called open costs
    // the part.
    //
    // Only a neighbour that actually CLOSES an edge is kept to clip against.
    // One standing INSIDE the region is an island, and the island machinery
    // already has it (slice 2's correction: collecting every standing
    // declaration sent a post inside an open pocket to the wrong branch).
    // A neighbour the ISLAND MACHINERY already has is not an edge-closer. A
    // boss inside this floor is its island; one straddling its boundary is a
    // BITE, which `split_islands` has made since Phase 26 and which is the
    // blessed reading of a wall flush with a face's edge. Only what lies
    // wholly BESIDE the region closes an edge — and note `plan_nesting` cannot
    // answer this on its own, because a wall sharing an edge with the face
    // never properly CROSSES it.
    // A neighbour that eats into this floor is not an edge-closer: a boss
    // inside it is its ISLAND, one straddling its boundary is a BITE, and
    // `split_islands` has made both since Phase 26 — a wall flush with a
    // face's edge is a bite by blessed reading, not a wall.
    //
    // The test is OVERLAP AREA, because neither nesting nor containment can
    // answer it here: a neighbour that shares this boundary exactly never
    // properly CROSSES it, and every interesting vertex lies ON an edge. What
    // separates the two cases is how much floor the neighbour covers. An
    // edge-closer covers only what the two outlines' chordings disagree
    // about, which is bounded by the perimeter times the one geometric
    // tolerance; a bite covers real area.
    let Judged { edges, closed_by, keep_clear } = judge_edges(p, &op.feature_name, z, &outer, is_face, &on_lens);

    let mut inners: Vec<Inner> = islands_in
        .into_iter()
        .map(|(name, outline)| Inner::Island { name: name.to_string(), outline })
        .collect();
    // Steel in the way: a fixture whose top is above this floor stands where
    // the tool's body will be, whatever height its foot is at, and one whose
    // footprint reaches the region is in THIS op's way. In declaration order.
    for fx in &p.fixtures {
        let (_, hi) = fx.z_span();
        if hi <= z + KEEP_OUT_AGREE {
            continue;
        }
        let outline = keep_out_outline(fx);
        let poly = match &outline {
            Island::Circle { x, y, r } => crate::ops::finish::circle_pts(*x, *y, *r).into_iter().map(|(x, y)| Pt { x, y }).collect(),
            Island::Loop(pts) => pts.clone(),
        };
        if !matches!(plan_nesting(&poly, &outer), Nesting::Apart) {
            inners.push(Inner::KeepOut { name: fx.name.clone(), outline });
        }
    }
    // A `Face`'s own inner loops with nothing standing in them are VOIDS —
    // "it doesn't need to go there". Until the tool decides otherwise (slice
    // 2) the path runs over them, and they are named by their place in the
    // feature: `loop 2` is the first inner boundary.
    if let Feature::Face { loops, .. } = host {
        for (i, inner) in loops.iter().enumerate().skip(1) {
            let occupied = inners.iter().any(|it| match it {
                Inner::Island { outline, .. } | Inner::KeepOut { outline, .. } => {
                    let poly: Vec<Pt> = match outline {
                        Island::Circle { x, y, r } => crate::ops::finish::circle_pts(*x, *y, *r).into_iter().map(|(x, y)| Pt { x, y }).collect(),
                        Island::Loop(pts) => pts.clone(),
                    };
                    // Something standing IN the loop — including a boss whose
                    // outline IS the loop, which no crossing test can see
                    // (coincident edges never properly cross).
                    poly.iter().any(|q| crate::stock::point_in_polygon(inner, q.x, q.y) || dist_to_polygon(inner, q.x, q.y) < 1e-4)
                }
                Inner::Bridged { .. } | Inner::OpenVoid { .. } => false,
            });
            // A declared void whose mouth lies in this loop is what the loop
            // IS (Phase 46 slice 3): its mouth — at a beveled edge a leg
            // wider than the void's own outline, which no coincidence test
            // can see. A declared void in the floor stays BRIDGED whatever
            // its size (Phase 27's rule, applied below to every sunk void);
            // judged by the tool instead, a wide mouth reads as a hole to go
            // round, and the column over the void stands for the void's own
            // op to be refused into.
            let voided = p.features.iter().any(|(name, f)| name.as_str() != op.feature_name && crate::stock::mouth_in_floor(p, f, inner, z));
            if !occupied && !voided {
                inners.extend(void_by_tool(format!("loop {}", i + 1), &Island::Loop(inner.clone()), tool_dia));
            }
        }
    }
    let mut sunk: Vec<&String> = p
        .features
        .iter()
        .filter(|(name, f)| name.as_str() != op.feature_name && crate::stock::mouth_in_floor(p, f, &outer, z))
        .map(|(name, _)| name)
        .collect();
    sunk.sort();
    // A declared void FEATURE in the floor stays BRIDGED whatever its size
    // (Phase 27's rule, kept on purpose — see `void_by_tool`): "there will be
    // a hole here" is not "do not go here", and the commonest program there is
    // — face the top, then bore it — must keep facing the top.
    inners.extend(sunk.into_iter().map(|name| Inner::Bridged { name: name.clone() }));

    // What closed the edges, kept for the boundary to clip against (fork 2B).
    // Empty where no neighbour closed anything — a closed pocket, or a step
    // whose walls are walls because they are not on the blank — so every
    // pre-Phase-29 family takes exactly the branch it always did.
    let mut region = Region { host: op.feature_name.clone(), z, outer, edges, inners, narrowest: None, keep_clear, closed_by, grown, on_the_shore };
    region.narrowest = narrowest_passage(&region);
    Some(region)
}

/// What `judge_edges` hands back: a kind per edge of the boundary, and the
/// declared neighbours that CLOSED one — their names and footprints.
struct Judged {
    edges: Vec<EdgeKind>,
    closed_by: Vec<String>,
    keep_clear: Vec<Vec<Pt>>,
}

/// The kind of every edge of a boundary at height `z` under `host` — the one
/// judgement `of` has made since Phase 27, in a home of its own (Phase 60
/// slice 1) so a wall trace reads the same kinds a floor does.
fn judge_edges(p: &Part, host: &str, z: f64, outer: &[Pt], is_face: bool, on_lens: &dyn Fn(Pt) -> bool) -> Judged {
    let blank = blank_of(p);
    let perimeter: f64 = (0..outer.len()).map(|i| {
        let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
        (b.x - a.x).phypot(b.y - a.y)
    }).sum();
    let chording_slack = perimeter * crate::CHORD_TOL_MM;
    let (neighbour_names, neighbours): (Vec<String>, Vec<Vec<Pt>>) = crate::stock::declared_neighbours_above(p, host, z + STANDS_ABOVE)
        .into_iter()
        .filter(|(_, o)| {
            let over: f64 = crate::offset::clip_to(o, outer).iter().map(|l| loop_area(l) / 2.0).sum();
            over <= chording_slack
        })
        .map(|(name, o)| (name.to_string(), o))
        .unzip();
    let mut closes: Vec<bool> = vec![false; neighbours.len()];
    let closer = |a: Pt, b: Pt| -> Option<usize> {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = dx.phypot(dy);
        if len <= 1e-9 {
            return None;
        }
        // `outer` is CCW, so the way OUT of it is to the right of travel.
        let (nx, ny) = (dy / len * BEYOND_THE_EDGE, -dx / len * BEYOND_THE_EDGE);
        [0.25f64, 0.5, 0.75].iter().find_map(|t| {
            let q = Pt { x: a.x + dx * t + nx, y: a.y + dy * t + ny };
            neighbours.iter().position(|o| crate::stock::point_in_polygon(o, q.x, q.y))
        })
    };
    let edges: Vec<EdgeKind> = (0..outer.len())
        .map(|i| {
            let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
            let mid = Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
            if edge_on_blank(&blank, a, b) {
                // Beyond the blank nothing stands. The old rule, unchanged,
                // and now the degenerate case of the one below it.
                EdgeKind::Open
            } else if on_lens(a) && on_lens(b) && on_lens(mid) {
                // The lens's edge (Phase 42 slice 3): the body would touch
                // the given void's wall beyond it — a WALL, on a face too.
                EdgeKind::Wall
            } else if let Some(k) = closer(a, b) {
                closes[k] = true;
                EdgeKind::Wall
            } else if is_face {
                // A face's outline IS the part's silhouette, so what lies
                // beyond it is scrap the op may overhang — unless a
                // declaration above said otherwise. Before Phase 29 this arm
                // ran FIRST and unconditionally, which is why a raised
                // neighbour could be machined away with the program exiting 0.
                EdgeKind::Open
            } else {
                // A cavity's outline: beyond it is its host.
                EdgeKind::Wall
            }
        })
        .collect();
    let closed_by: Vec<String> = neighbour_names.into_iter().zip(&closes).filter(|(_, c)| **c).map(|(n, _)| n).collect();
    let keep_clear: Vec<Vec<Pt>> = neighbours.into_iter().zip(&closes).filter(|(_, c)| **c).map(|(o, _)| o).collect();
    Judged { edges, closed_by, keep_clear }
}

/// The edges a WALL TRACE meets — a `Side` on a prismatic loop that runs on
/// or past the blank (Phase 60 slices 1 and 2): the authored loop clipped to
/// the blank, and a kind per edge — a pocket's judged as `of` judges a
/// floor's, a boss's by the blank rule alone. `None` for any other feature.
pub fn wall_edges(p: &Part, op: &Operation) -> Option<(Vec<Pt>, Vec<EdgeKind>)> {
    let host = p.features.get(&op.feature_name)?;
    match host {
        Feature::ProfilePocket { points, z, extent, .. } => {
            let floor = derive_z_cut(derive_contour_top(p.stock.z, *z), *extent);
            // Wholly inside the blank the loop stands as authored — and an
            // edge ON the blank is still judged open, as `of` judges it.
            let outer = clipped_outline(p, points).unwrap_or_else(|| points.clone());
            let j = judge_edges(p, &op.feature_name, floor, &outer, false, &|_| false);
            Some((outer, j.edges))
        }
        // A boss (Phase 60 slice 2, fork 2A): its loop is CW with the part
        // inside, and nothing stands beyond its edge but the floor it stands
        // on — so the blank rule alone judges it: an edge on or past the blank
        // is OPEN, every other edge is the boss's own wall. `judge_edges`'s
        // other arms (a closer, a lens, a face) are a floor's questions. The
        // clip hands the loop back in whatever winding the boolean chose; it
        // is held CW so the chains run in the boss's own direction.
        Feature::ProfileBoss { points, .. } => {
            let blank = blank_of(p);
            // A boss on the shore is drawn to the blank's TRUE outline — a rim
            // boss's arc at the blank's radius — and the blank is a chorded
            // polygon within `CHORD_TOL_MM` of it. Clipping such a loop saws
            // the shared rim into slivers alternately on and off the polygon
            // (nine chains for one rib of `disc_ribs.cam` when this arm was
            // first run), so the loop is clipped only where it truly
            // OVERHANGS — a vertex past the blank by more than the one
            // tolerance — and stands as authored otherwise.
            let overhangs = points
                .iter()
                .any(|q| !crate::stock::point_in_polygon(&blank, q.x, q.y) && dist_to_polygon(&blank, q.x, q.y) > crate::CHORD_TOL_MM);
            let mut outer = if overhangs { clipped_outline(p, points).unwrap_or_else(|| points.clone()) } else { points.clone() };
            if signed_area(&outer) > 0.0 {
                outer.reverse();
            }
            let n = outer.len();
            let perimeter: f64 = (0..n).map(|i| (outer[(i + 1) % n].x - outer[i].x).phypot(outer[(i + 1) % n].y - outer[i].y)).sum();
            let kinds = (0..n)
                .map(|i| if boss_edge_on_blank(&blank, outer[i], outer[(i + 1) % n], perimeter) { EdgeKind::Open } else { EdgeKind::Wall })
                .collect();
            Some((outer, kinds))
        }
        _ => None,
    }
}

/// Is the edge from `a` to `b` ON the blank — both ends and its midpoint
/// within `ON_THE_BLANK` of the boundary? The one rule that makes an edge
/// OPEN (Phase 27): beyond the blank nothing stands. `judge_edges` reads it
/// first for a floor's edge; `wall_edges` reads it alone for a boss's.
pub(crate) fn edge_on_blank(blank: &[Pt], a: Pt, b: Pt) -> bool {
    let mid = Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
    [a, b, mid].iter().all(|q| dist_to_polygon(blank, q.x, q.y) <= ON_THE_BLANK)
}

/// A BOSS's edge ON the blank, read with the shore's tolerance (Phase 49's
/// `CHORD_TOL_MM`, where a floor's clipped edge reads the exact
/// `ON_THE_BLANK`): both ends within it, and the lune between the edge and
/// the blank's boundary — two thirds of the chord times its midpoint's sag,
/// a chord's area against its arc to first order — within the CHORDING SLACK
/// `judge_edges` already allows a neighbour, the loop's perimeter times the
/// one tolerance. A rim arc chorded coarser than the blank's polygon sags a
/// twentieth of a millimetre and passes; a flat cut across a disc between
/// two rim points sags centimetres and is the wall it is.
fn boss_edge_on_blank(blank: &[Pt], a: Pt, b: Pt, perimeter: f64) -> bool {
    let near = |q: Pt| dist_to_polygon(blank, q.x, q.y) <= crate::CHORD_TOL_MM;
    let mid = Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
    let len = (b.x - a.x).phypot(b.y - a.y);
    let lune = 2.0 / 3.0 * len * dist_to_polygon(blank, mid.x, mid.y);
    near(a) && near(b) && lune <= perimeter * crate::CHORD_TOL_MM
}

fn loop_area(l: &[Pt]) -> f64 {
    (0..l.len()).map(|i| l[i].x * l[(i + 1) % l.len()].y - l[(i + 1) % l.len()].x * l[i].y).sum::<f64>().abs()
}

/// The blank's footprint as the region reads it — `parse::derive_blank`, the
/// one home (Phase 46 slice 1): a box's corners or a round's chords, and the
/// edge kinds fall out of it (a `Plane` on a round blank is a disc, open all
/// round).
pub(crate) fn blank_of(p: &Part) -> Vec<Pt> {
    crate::parse::derive_blank(&p.stock)
}

/// **The tool decides what is a gap** (Phase 28 slice 2, fork 2A) — for the
/// voids an author DREW: a `Face`'s own inner loops, which say "it doesn't need
/// to go there." A declared void FEATURE is a different statement and stays
/// bridged: the first build judged those by the tool too, and the most
/// ordinary program in the suite — face a plate at 60% stepover, then bore a
/// 30mm hole in it — stopped being a raster, went around a hole that did not
/// exist yet, and REFUSED under the ridge law. Nobody asked for that. A void is
/// shrunk by one tool diameter — the construction an open outer edge gets,
/// mirrored. Anything left: the tool can stand in the void clear of its rim,
/// so it is an OPEN inner edge, and what is left is what the family goes
/// around (a void that pinches may leave several pieces; each is one). Nothing
/// left: BRIDGED — the path runs over it as if it were floor. There is no
/// threshold to tune; it falls out of the push. It does mean a void between
/// one and two diameters wide is bridged — the tool would fit, but its two rim
/// passes would be the same pass — said here once so nobody reads it as a bug.
fn void_by_tool(name: String, outline: &Island, tool_dia: f64) -> Vec<Inner> {
    let left: Vec<Island> = match outline {
        Island::Circle { x, y, r } if *r > tool_dia + 1e-9 => vec![Island::Circle { x: *x, y: *y, r: r - tool_dia }],
        Island::Circle { .. } => Vec::new(),
        Island::Loop(pts) if tool_dia.is_finite() => {
            let ccw = crate::ops::clear::make_ccw(pts);
            crate::offset::offset_closed_polyline_verts(&ccw, tool_dia)
                .iter()
                .map(|l| crate::offset::flatten_loop(l))
                .filter(|l| l.len() >= 3)
                .map(Island::Loop)
                .collect()
        }
        Island::Loop(_) => Vec::new(),
    };
    if left.is_empty() {
        return vec![Inner::Bridged { name }];
    }
    left.into_iter().map(|outline| Inner::OpenVoid { name: name.clone(), outline }).collect()
}

fn island_polygon(island: &Island) -> Vec<Pt> {
    match island {
        Island::Circle { x, y, r } => crate::ops::finish::circle_pts(*x, *y, *r).into_iter().map(|(x, y)| Pt { x, y }).collect(),
        Island::Loop(pts) => pts.clone(),
    }
}

/// The floor's shape as two numbers (Phase 34 slice 0; `DECISIONS.md` →
/// "Phase 34 slice 0…"): how much floor there is for how much boundary, and
/// how much of that boundary is WALL. Facts about the floor, the same for
/// every pattern — the comparison report prints them and nothing reads them
/// to choose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub area: f64,
    pub perimeter: f64,
    /// The length of boundary something stands beyond: the outer WALL edges,
    /// the flank of every bite, and the outline of everything that stands in
    /// the floor. The rest of the perimeter is open — the blank's edge, scrap,
    /// a drawn void's rim.
    pub wall: f64,
}

impl Shape {
    /// `2·area / perimeter`: a long strip of width w reads w, a disc reads its
    /// radius. Not a width anyone measures with a caliper — the lands are
    /// those — but the one number that says rail or field.
    pub fn mean_width(&self) -> f64 {
        2.0 * self.area / self.perimeter
    }

    pub fn wall_share(&self) -> f64 {
        self.wall / self.perimeter
    }
}

/// The floor AS DECLARED, measured: the region's outer boundary with every
/// straddling neighbour bitten out of it (`split_islands` — the emitters' own
/// reading), less the loops a `Face` draws inside it, less what stands on the
/// floor itself. The tool is not asked: a drawn void too narrow to go around
/// is bridged by the PATH, and is still not floor. `None` where the bites
/// leave no one outline.
pub fn shape(p: &Part, op: &Operation, region: &Region) -> Option<Shape> {
    let drawn: &[Vec<Pt>] = match p.features.get(&op.feature_name) {
        Some(Feature::Face { loops, .. }) => &loops[1..],
        _ => &[],
    };
    // What stands IN a drawn loop (or is one — a boss whose outline is the
    // loop) took its area out with the loop; it is that loop's wall, below.
    let in_a_loop = |island: &Island| {
        let poly = island_polygon(island);
        drawn.iter().any(|l| poly.iter().all(|q| crate::stock::point_in_polygon(l, q.x, q.y) || dist_to_polygon(l, q.x, q.y) < 1e-4))
    };
    // What merely TOUCHES the boundary covers no floor and bites nothing: it
    // stands beside, and closed the edges it touches (`of`'s overlap-area
    // rule, and its slack — two chordings of one curve disagree by that much).
    let slack = loop_length(&region.outer) * crate::CHORD_TOL_MM;
    let covers_floor = |island: &Island| crate::offset::clip_to(&island_polygon(island), &region.outer).iter().map(|l| loop_area(l) / 2.0).sum::<f64>() > slack;
    let on_the_floor: Vec<(&str, &Island)> = region.standing().into_iter().filter(|(_, i)| !in_a_loop(i) && covers_floor(i)).collect();
    let (outline, standing) = split_islands(region.outer.clone(), &on_the_floor)?;
    let n = region.outer.len();
    let mut s = Shape { area: loop_area(&outline) / 2.0, perimeter: 0.0, wall: 0.0 };
    for i in 0..outline.len() {
        let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
        let len = (b.x - a.x).phypot(b.y - a.y);
        let (mx, my) = ((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        // An edge of the bitten outline that still lies ON an edge of the
        // region keeps that edge's kind; one that does not is a bite's flank,
        // and what bit is standing there.
        let kind = (0..n)
            .find(|e| dist_to_polygon(&[region.outer[*e], region.outer[(*e + 1) % n]], mx, my) <= ON_THE_BLANK)
            .map_or(EdgeKind::Wall, |e| region.edges[e]);
        s.perimeter += len;
        if kind == EdgeKind::Wall {
            s.wall += len;
        }
    }
    for island in &standing {
        // A round boss on its CIRCLE, never its chords (`gap_to_segment`'s rule).
        let (area, around) = match island {
            Island::Circle { r, .. } => (std::f64::consts::PI * r * r, std::f64::consts::TAU * r),
            Island::Loop(pts) => (loop_area(pts) / 2.0, loop_length(pts)),
        };
        s.area -= area;
        s.perimeter += around;
        s.wall += around;
    }
    // A drawn loop's edge is a wall where something stands a hair beyond it —
    // `of`'s own probe for an outer edge, at its own three stations.
    let stands: Vec<Vec<Pt>> = region.standing_polygons().into_iter().chain(region.keep_clear.iter().cloned()).collect();
    for l in drawn {
        s.area -= loop_area(l) / 2.0;
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.phypot(dy);
            if len <= 1e-9 {
                continue;
            }
            s.perimeter += len;
            let walled = [0.25f64, 0.5, 0.75].iter().any(|t| {
                [1.0, -1.0].iter().any(|side| {
                    let q = Pt { x: a.x + dx * t + dy / len * BEYOND_THE_EDGE * side, y: a.y + dy * t - dx / len * BEYOND_THE_EDGE * side };
                    crate::stock::point_in_polygon(l, q.x, q.y) && stands.iter().any(|o| crate::stock::point_in_polygon(o, q.x, q.y))
                })
            });
            if walled {
                s.wall += len;
            }
        }
    }
    (s.area > 0.0 && s.perimeter > 0.0).then_some(s)
}

fn loop_length(l: &[Pt]) -> f64 {
    (0..l.len()).map(|i| (l[(i + 1) % l.len()].x - l[i].x).phypot(l[(i + 1) % l.len()].y - l[i].y)).sum()
}

/// A fixture's plan footprint, grown by the margin a tool keeps from steel —
/// `assembly::DEFAULT_CLEARANCE_MM`, the one home for "the clearance a tool
/// keeps from what it must not touch".
fn keep_out_outline(fx: &crate::parse::Fixture) -> Island {
    let m = crate::assembly::DEFAULT_CLEARANCE_MM;
    match fx.shape {
        crate::parse::FixtureShape::Box { w, l, .. } => Island::Loop(vec![
            Pt { x: fx.x - m, y: fx.y - m },
            Pt { x: fx.x - m, y: fx.y + l + m },
            Pt { x: fx.x + w + m, y: fx.y + l + m },
            Pt { x: fx.x + w + m, y: fx.y - m },
        ]),
        crate::parse::FixtureShape::Cylinder { dia, .. } => Island::Circle { x: fx.x, y: fx.y, r: dia / 2.0 + m },
    }
}

/// What the path goes around, against the boundary its family is actually
/// eroded from: an outline wholly inside `wall` is an island of it; one that
/// STRADDLES it — a toe clamp over the stock's edge — is a bite out of it, and
/// one clear of it is nothing. Standing features are always the first kind
/// (`validate` refused the rest); fixtures can be any of the three.
pub fn split_islands(wall: Vec<Pt>, islands: &[(&str, &Island)]) -> Option<(Vec<Pt>, Vec<Island>)> {
    let mut wall = wall;
    let mut kept = Vec::new();
    for (_, island) in islands {
        let poly: Vec<Pt> = match island {
            Island::Circle { x, y, r } => crate::ops::finish::circle_pts(*x, *y, *r).into_iter().map(|(x, y)| Pt { x, y }).collect(),
            Island::Loop(pts) => pts.clone(),
        };
        match plan_nesting(&poly, &wall) {
            Nesting::Inside => kept.push((*island).clone()),
            Nesting::Straddles => wall = crate::offset::subtract(&wall, &poly)?,
            Nesting::Apart | Nesting::Around => {}
        }
    }
    Some((wall, kept))
}

/// The gap between an island and one straight stretch of boundary, and between
/// two islands — a round boss measured on its CIRCLE, centre distance less the
/// radius, never on its chords (Phase 26 slice 2: a post standing 8.000 off a
/// wall read 8.010 through them, and this is a number the author goes and
/// measures).
fn gap_to_segment(island: &Island, a: Pt, b: Pt) -> f64 {
    let seg = [a, b];
    match island {
        Island::Circle { x, y, r } => dist_to_polygon(&seg, *x, *y) - r,
        Island::Loop(pts) => pts.iter().map(|q| dist_to_polygon(&seg, q.x, q.y)).fold(f64::INFINITY, f64::min),
    }
}

pub(crate) fn gap_between(a: &Island, b: &Island) -> f64 {
    match (a, b) {
        (Island::Circle { x, y, r }, Island::Circle { x: x2, y: y2, r: r2 }) => (x - x2).phypot(y - y2) - r - r2,
        (Island::Loop(pts), other) | (other, Island::Loop(pts)) => match other {
            Island::Circle { x, y, r } => dist_to_polygon(pts, *x, *y) - r,
            Island::Loop(o) => {
                let one_way = |from: &[Pt], to: &[Pt]| from.iter().map(|q| dist_to_polygon(to, q.x, q.y)).fold(f64::INFINITY, f64::min);
                one_way(pts, o).min(one_way(o, pts))
            }
        },
    }
}

fn narrowest_passage(region: &Region) -> Option<Passage> {
    // Between things that STAND. An open void's rim is air, like an open edge:
    // it bounds nothing, and the ring of a wrench is not a web.
    let islands = region.standing();
    let mut best: Option<Passage> = None;
    let mut consider = |width: f64, between: &str, and: String| {
        if best.as_ref().map_or(true, |b| width < b.width) {
            best = Some(Passage { between: format!("'{between}'"), and, width });
        }
    };
    for (i, (name, island)) in islands.iter().enumerate() {
        for (e, kind) in region.edges.iter().enumerate() {
            if *kind == EdgeKind::Wall {
                let (a, b) = (region.outer[e], region.outer[(e + 1) % region.outer.len()]);
                consider(gap_to_segment(island, a, b), name, format!("'{}''s wall", region.host));
            }
        }
        for (other, o) in &islands[i + 1..] {
            consider(gap_between(island, o), name, format!("'{other}'"));
        }
    }
    best
}
