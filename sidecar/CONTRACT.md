# The Rust↔sidecar contract (v1)

This file is the canonical definition of the seam between the Python
(CadQuery / OCP) sidecar and everything else — the Rust core, the human, or
an agent reviewing proposals. ROADMAP Phase 4 names the constraints: plain
text / JSON over a subprocess, diffable and inspectable, no binary blobs, no
network, no phone-home. The *reasoning* lives in `DECISIONS.md` ("Rust↔sidecar
contract v1…"); this file is the spec.

## Invocation

```
sidecar/.venv/bin/python sidecar/import_step.py <model.step> -o <outdir>/
    [--solid N] [--up {x+,x-,y+,y-,z+,z-}] [--allowance MM]
    [--origin X,Y,Z] [--given PROJECT --blank WORDS]
```

argv + files, nothing else. No stdin protocol, no long-running process, no
sockets. The sidecar is a batch tool invoked per import; a future `nacs
import` subcommand is a thin wrapper around this exact invocation.

The two optional flags are **setup declarations** (the import-breadth slice;
`DECISIONS.md` → "Import breadth…") — facts about how the part sits on the
machine that only the human knows, so they are declared, never guessed:

- `--solid N` — which solid is the part, for a multi-solid STEP (an
  assembly). Solids are numbered `S1…SM` in the kernel's deterministic
  import order. Without it, a multi-solid file **rejects** with a one-line
  per-solid inventory (id, face count, bounding box) on stderr — never a
  guess, never a merge. An out-of-range `N` rejects too.
- `--up A` — which **model** axis points up on the machine (default `z+`).
  The sidecar rotates that axis onto machine +Z *before* recognition, so a
  Y-up CAD export reads exactly like its Z-up twin; every reported
  coordinate and proposed feature is post-rotation (machine orientation).
  The six values cover axis-aligned setups; an arbitrary-angle tilt still
  lands in the residue face by face, with reasons (honest error).
- `--allowance MM` (ROADMAP item 1, slice 11) — how far the blank stands
  above the model's top and outside its plan (default `0`: a blank cut to
  size; the engine's import doors pass `1` unless told otherwise). The
  recognizer reads the model unchanged and places what the blank changes:
  a bore whose rim is the model's top is below the blank's top, so a plain
  drilled `_Hole` proposes from the blank top with `extent` grown by the
  allowance, and a bore with a mouth chamfer or round, or a milled `_Bore`,
  is placed by `z=` at its rim; the vertical cylindrical face that bounds
  the plan on every side (the blank's own rim) proposes, from the stretch
  that runs the whole way round up from the part bottom, as
  `F<n>_Rim CylinderBoss(…)`, the boss whose wall pass brings the blank to
  the part's diameter (a stretch that does not stand on the part bottom
  is the other setup's and lands in the residue as `rim_not_on_bottom`).
  The header of `proposed.part` and the report's `allowance` field say
  the import was made with it; without it both files are byte for byte
  what they were.
- `--origin X,Y,Z` — the point that becomes 0,0,0. The sidecar moves the
  model there after the `--up` turn and before recognition, so every
  reported coordinate and proposed feature is measured from it. Each
  component is `model` (that axis keeps the model's own zero), a number (a
  model coordinate, after `--up`), `min`, `mid` or `max` of the blank (the
  model's box grown by `--allowance` on the sides and the top; a prism is
  its own blank and is not grown), or `part-min`, `part-mid` or `part-max`
  of the model. `mid,mid,max` is the blank's centre and its top. Without
  the flag nothing moves and both files are byte for byte what they were.
  The report carries `origin` (the words) and `origin_at` (the model point);
  the headers of `proposed.part` and `proposed_stock.part` say both.
  `import_stock.py` takes the same flag and must be given the same words.
  Beside `--blank` it is an error unless every component is `model`: the
  second setup keeps the origin its blank is written in.
- `--given PROJECT --blank WORDS` (ROADMAP item 1, slice 12) — the second
  setup. The engine, which has the earlier program, computes its blank
  turned as `--up` says (`z+` or `z-`; a quarter turn is refused, since a
  program reads no other turn between setups) and hands it over as the
  stock statement's words, `Cylinder(dia=, h=, x=, y=, z=)` or
  `Box(w=, l=, h=, x=, y=, z=)`; `PROJECT` is the path as the `given from`
  line will name it. The sidecar reads the model unchanged and measures the
  blank's margins instead of taking an allowance (`--allowance` beside
  `--blank` is an error, and so is one flag without the other): the margin
  above the model's top places the bores as `--allowance` would, and the
  plan-bounding cylinder — the blank's rim — proposes nothing and lands in
  the residue as `rim_given`, the earlier setup's to bring to size; a
  whole-height boss the outer-profile pass made of it is withdrawn. A model
  standing outside the blank on any side is an error naming the side: the
  earlier blank turned over does not hold it, so the turn or the origin
  differs. The report carries `given` and `blank`; the header of
  `proposed.part` says both.

Outputs, written into `<outdir>/`:

| File            | When                | What                                        |
|-----------------|---------------------|---------------------------------------------|
| `report.json`   | always on success   | the total-accounting recognition report     |
| `proposed.part` | when ≥ 1 proposal   | `feature` lines ready for human review      |

stdout: a one-line summary (`N faces: P proposed, U unsupported`).
Errors: one-line stderr, nonzero exit, **no partial output files** — the same
convention as the Rust CLI (never emit half a program).

## `report.json` — schema v1

```json
{
  "schema": "nacs-recognition-report",
  "version": 1,
  "source": "block_bore.step",
  "units": "mm",
  "faces_total": 10,
  "faces": [
    {
      "id": "F1",
      "surface": "plane",
      "status": "proposed",
      "checklist_row": "1.1",
      "part": "feature F1_Top Plane(z=15.0)",
      "why": "planar, normal +Z, topmost planar face"
    },
    {
      "id": "F6",
      "surface": "cylinder",
      "status": "unsupported",
      "checklist_row": "2.4",
      "code": "cylinder_boss_uncapped",
      "reason": "cylindrical face with material inside (outside boss) not anchored by a flat +Z top cap -- a capped, domed, or stepped boss cannot graduate to Feature::Boss (CHECKLIST row 2.4)"
    },
    {
      "id": "F19",
      "surface": "plane",
      "status": "unsupported",
      "checklist_row": "1.2",
      "code": "outer_profile_candidate_wall/outer_profile_wall_not_vertical",
      "reason": "wall of refused outer-profile candidate under cap F23: outer-profile wall F16 is not vertical -- a tilted/draft wall (CHECKLIST row 1.2) or an undercut relative to the tool axis",
      "refs": ["F23", "F16"]
    }
  ]
}
```

Field meanings:

- `schema` / `version` — always `"nacs-recognition-report"` / the integer
  schema version. Consumers must check both.
- `source` — the input's **basename** (never an absolute path; see
  determinism below).
- `units` — always `"mm"` in v1: the sidecar converts at the boundary, the
  same rule as the Rust parser's mm conversion boundary.
- `faces_total` — the body's face count from an independent traversal.
- `faces` — one entry per face, in deterministic traversal order, `id`
  assigned `F1…FN` in that order.
- `solid` / `solids_total` / `up` — optional top-level setup declarations
  (added additively under v1, the import-breadth slice), present **only
  when the import needed one**: `solid` (e.g. `"S1"`) and `solids_total`
  appear together when the file held more than one solid; `up` (e.g.
  `"y+"`) appears when a non-default `--up` was declared. Single-solid,
  Z-up imports — and their committed goldens — are byte-unchanged.

Per-face fields:

- `surface` — the underlying surface type as reported by the kernel
  (`plane`, `cylinder`, `cone`, `sphere`, `torus`, `other` in v1; new values
  are additive).
- `status` — exactly `"proposed"` or `"unsupported"`. No third value; no
  entry may be omitted.
- `checklist_row` — the `CHECKLIST.md` row ID this face maps to (e.g.
  `"2.3"`), or `"no row"` when no taxonomy row fits. Required on every entry.
- `proposed` entries carry `part` (exactly one parseable `feature`
  *statement* — the same text that lands in `proposed.part`; a `Contour`
  block spans several physical lines, carried as one JSON string) and `why`
  (one line: the orientation/reachability evidence for the proposal).
- `unsupported` entries carry `reason` (one line naming *why* — including,
  when it applies, that the face is undercut/re-entrant relative to the tool
  axis) and, since the Phase 17 review slice, `code` — the refusing check's
  name as a stable word string (never a number), enumerated under "Residue
  codes" below, so a consumer groups the residue by the check and never by
  the sentence — plus `refs`, the face ids (`"F16"`) or feature names
  (`"F6_Profile"`) the reason names, in the order the sentence names them,
  **present only when there is at least one** (never an empty list). Both
  are built from the values the sentence is formatted from, never recovered
  from it; the prose stays the human voice, unchanged.
- **Grouped proposals** (added additively under v1, the pocket slice): one
  recognized feature can be owned by several faces — a pocket is its floor
  plus its wall loop; since the hole-vs-bore slice, a drilled hole is its
  wall plus its drill-point cone, and a flat-bottom bore its wall plus its
  floor. Every member face is `proposed`, repeats the shared `part`
  statement **verbatim**, and carries the optional `feature` field naming
  the group (e.g. `"F27_Pocket"`). `feature` is absent on single-face
  proposals. Repetition is deliberate: it keeps "every proposed entry
  carries `part`" true per face (totality point 4 below), and a naive
  consumer that collects `part` statements still gets a parseable result —
  identical `feature` blocks under one name are idempotent in the `.part`
  grammar. `proposed.part` emits the block once, at the primary face's
  traversal position (the pocket's floor; the hole/bore's wall). The report
  carries no `primary` field: **the primary is the member whose id is the
  group name's `F<n>_` prefix** (`F27_Pocket` is floor F27's, `F8_Hole`
  wall F8's) — not the first member in traversal order, which on
  `pocket_plate` is a wall. A consumer that must place the block reads the
  prefix; `tests/review_tests.rs` pins that every group on every golden
  names exactly one member this way.

## The totality invariant (total accounting)

From the settled decision (`DECISIONS.md` → "Recognition residue policy is
total accounting…"):

1. `len(faces) == faces_total ==` the actual face count of the imported body.
2. Every entry is `proposed` or `unsupported` — **no face may be silently
   dropped**, and there is no "skipped" status.
3. Every `unsupported` entry names its `checklist_row` (or `"no row"`), a
   `reason`, and a `code` from the enumeration below; `refs`, when present,
   is non-empty and every id in it is a face of the same report or a
   feature name a `proposed` entry of the same report carries.
4. Every `proposed` entry's `part` line must parse as a `feature` statement
   of the current `.part` grammar (`src/parse/grammar.rs`).

The sidecar asserts (1)–(3) itself before writing any output; (4) is pinned
by the Rust round-trip test (`tests/sidecar_tests.rs`), and (3)'s code and
ref clauses by `test_sidecar_residue_codes` there, on the committed goldens.

## Residue codes

A `code` is the name of the branch in `sidecar/import_step.py` that refused
the face — one word string per refusing check, derived from the check and
never from the sentence (the house precedent is `check_result::family_word`:
words a reader can read, never a `CAM0142` a reader has to look up —
and spelled the way that precedent and this report's own keys are,
`snake_case`). Its
shape is `check`, or `role/check` with exactly one slash: an outer-profile
or trim-wire candidate that fails a graduation check lands that refusal on
*every wall* of the candidate (and, for a below-top cap, on the cap), and
each such face carries the **role** it played prefixed to the inner
**check** — `outer_profile_candidate_wall/outer_profile_wall_not_vertical`
is a wall of a refused outer profile, refused because a (possibly other)
wall was not vertical. A pocket candidate's refusal stays on its floor
alone as a bare `check` (its walls keep their own classifications). Roles
never nest. The one home is `residue()` /
`Refusal.rewrap()` in the sidecar; the test pins that every `code=` and
`role=` spelled there is listed here, and vice versa, so a new check cannot
ship without a documented code.

Roles (the prefix, when present):

| Role | The face is… |
|------|--------------|
| `outer_profile_candidate_wall` | a wall of a refused outer-profile candidate; `refs` leads with the cap |
| `outer_profile_candidate_cap` | the below-top cap of a refused outer-profile candidate |
| `trim_wire_candidate_wall` | a wall of a refused trim-wire candidate; `refs` leads with the part-bottom anchor |

Checks (the code, or the part after the slash), by the classifier that owns
them:

| Code | Row | The check that refused |
|------|-----|------------------------|
| `plane_downward` | no row | planar, normal −Z: unreachable from above |
| `plane_vertical_unanchored` | 2.2 | vertical planar wall bounding no recognized pocket floor or cap loop |
| `plane_oblique` | 1.2 | planar, neither vertical nor ±Z |
| `cylinder_axis_tilted` | 2.20 | cylinder axis not parallel to +Z |
| `cylinder_partial_unanchored` | 2.8 | partial-revolution cylinder bounding no recognized floor or cap loop |
| `cylinder_boss_uncapped` | 2.4 | material inside (a boss) with no flat +Z top cap anchoring it |
| `rim_not_on_bottom` | 2.4 | the plan-bounding cylinder (with `--allowance`) whose full-revolution stretch does not stand on the part bottom — the other setup's rim |
| `rim_given` | 2.4 | the plan-bounding cylinder on a blank given from the earlier program (`--given`): that setup's rim, standing as it left it |
| `cylinder_bore_capped` | no row | a bore whose axis probe above the rim is inside material |
| `sphere_partial` | no row | partial-revolution sphere (a blend/fillet) |
| `sphere_concave` | 3.1 | material outside (a bowl cavity) |
| `sphere_below_equator` | 3.3 | convex dome reaching below its equator |
| `cone_axis_tilted` | 2.20 | cone axis not parallel to +Z |
| `cone_partial` | 2.8 | partial-revolution cone |
| `cone_cavity_point_down` | 2.14 | internal cone narrowing downward (a point-down cavity) |
| `cone_cavity_undercut` | no row | internal cone widening downward |
| `cone_boss_undercut` | no row | conical boss widening upward (apex below its base) |
| `cone_frustum` | 2.12 | conical boss stopping short of the apex |
| `torus_axis_tilted` | no row | torus axis not parallel to +Z |
| `torus_partial` | no row | partial-revolution torus (a blend/fillet) |
| `torus_lip_blend` | 3.14 | full ring sharing an edge with a cylinder — a lip round on a wall, not a standing ring |
| `torus_internal` | no row | material outside the tube (an internal groove/fillet) |
| `torus_below_equator` | 3.13 | ring reaching below its equator plane |
| `edge_not_line_or_arc` | 2.5 | a boundary edge that is neither a line nor a circular arc (pocket floor or outer-profile cap loop). A wall's top boundary is the exception since 2026-10-29: the curve where a bevel's plane meets a cylindrical wall lies on the wall's own circle and is chorded at `CHORD_TOL` like an arc |
| `pocket_edge_unwalled` | no row | a floor boundary edge with no adjacent wall face |
| `pocket_wall_not_vertical` | 1.2 | a planar wall whose normal is not horizontal; `refs` the wall |
| `pocket_wall_cylinder_tilted` | 1.2 | a cylindrical wall whose axis is not +Z; `refs` the wall |
| `pocket_floor_of_bore` | 2.3 | a full-revolution bore wall whose BOTTOM is this floor (the bore proposes on its own; the part's silhouette and a bore from below are the floor's edges, not its walls — Phase 46 slice 2); `refs` the wall |
| `pocket_wall_not_plane_or_cylinder` | no row | a wall of another surface type; `refs` the wall |
| `pocket_walls_stepped` | no row | wall tops at more than one height |
| `pocket_walls_descend` | no row | the loop's mouth is not above the floor (a plateau top) |
| `pocket_mouth_lipped` | no row | a face across a mouth-rim edge is not an upward plane (the load-bearing undercut check) |
| `pocket_capped` | no row | the probe above the mouth over the floor centroid is inside material |
| `outer_profile_wall_not_vertical` | 1.2 | a planar wall whose normal is not horizontal; `refs` the wall |
| `outer_profile_wall_cylinder_tilted` | 1.2 | a cylindrical wall whose axis is not +Z; `refs` the wall |
| `outer_profile_wall_not_plane_or_cylinder` | no row | a wall of another surface type; `refs` the wall |
| `outer_profile_wall_inverted` | no row | a wall whose outward normal faces the loop interior; `refs` the wall |
| `outer_profile_degenerate_rim` | no row | the walls have no height below the cap |
| `outer_profile_base_rim_undercut` | no row | a face across a base-rim edge is neither an upward ledge, the part bottom, nor the wall's own underside; or the wall tops a head on a narrower neck (its own underside under its feet) that the walk down could not place and that floats above a PROPOSED floor hosting it — the engine's own undercut rule, applied here so the report's proposals parse together; `refs` that floor. A head on a neck proposes down to the floor it stands over (what is under it is `under_overhang`); one whose neck stands on the part bottom proposes to its own base, its underside named as the other setup's |
| `trim_wire_edge_not_line` | no row | a top edge of the loop that is not a straight line; `refs` the wall |
| `trim_wire_open_loop` | no row | the top edges do not chain into one closed loop |
| `trim_wire_wall_inverted` | no row | a wall with material on the wrong side of its loop; `refs` the wall |
| `trim_wire_anchor_bottom` | no row | not a refusal: the part bottom on which trim wire(s) proposed — it anchors, it is never a member; `refs` the feature(s) |
| `chamfer_consumes_wall` | 1.2 | an oblique face from a floor to the surrounding plane with nothing vertical under it — a draft wall or a bevel as deep as its wall (the engine's chamfer needs a flank); `refs` the face |
| `under_overhang` | no row | a face under a head that overhangs it — a wall of the neck, the head's underside, the pad the neck stands on, the pad's foot bevel: nothing from above reaches it. The head is proposed down to the floor it stands over and the program leaves the material under it standing; a foot bevel standing outside the head's outline is cleared away to the outline, and its entry says so. `refs` the head, then the floor |
| `level_obstructed` | 1.1 | a floor that is the level settled heads stand over, refused the `Plane` because faces stand above that height outside every proposed feature, and a Plane would clear through them; `refs` those faces |
| `other_setup` | no row | a face with the part's own material straight above it: from up to five points on the face, nudged off it into the void, a ray straight up meets the part every time. Nothing on this setup's tool axis reaches it, whatever a classifier made of it; the entry names the nearest face over it and the `--up` that turns the model over to look from the other side (a hidden cavity is covered from both). A proposal whose primary face is covered (a counterbore seen from the back, a void under a ceiling) is withdrawn whole, every member carrying this code. A downward plane, a face under a head and a trim wire's bottom anchor keep their own codes, which already say as much; `refs` the covering face |
| `chamfer_at_foot` | 1.2 | a bevel at a wall's FOOT — the engine's chamfer is a top-edge break and has no key for it; `refs` the strip |
| `chamfer_uneven` | no row | the strips round one cap carry more than one angle (one tool cuts one angle), or the strips round one mouth do not carry one leg and one angle; `refs` the strips. Round a boss's cap the profile still proposes without a chamfer and each strip carries this entry (`refs` the other strips and the profile's primary wall); round a pocket's mouth the pocket refuses |
| `chamfer_wall_loop_open` | no row | the walls under a beveled cap do not chain into one closed loop at their tops; `refs` the strips |
| `surface_not_graduated` | no row | a surface type no classifier reads yet |

The row column is the `checklist_row` the code lands with today (a role
keeps the inner check's row); it is documentation, not a second key — the
entry's own `checklist_row` is authoritative.

## The undercut co-requisite (graduation rule)

A surface type may graduate from residue to proposable **only together with
its orientation/undercut check** (ROADMAP Phase 4, "Undercut / re-entrancy
detection lands with recognition"). Two types graduated with the v1 walking
skeleton; the quadric slice (2026-07) added three more; the pocket slice
(2026-07) added the first grouped feature; the hole-vs-bore slice (2026-07)
refined the cylinder's semantics by its bottom face:

- **plane** → `Plane(z=…)`: planar faces with normal +Z at the topmost
  planar height — a *height*, not a single face: a part whose top is split
  into disconnected regions (e.g. by a half-embedded ring) proposes each of
  them. Any other orientation (vertical wall, oblique, downward) is residue.
  A +Z plane BELOW the topmost height proposes as a `Plane` too when it is
  the LEVEL that heads on necks stand over (ROADMAP item 1 slice 6): every
  neighbour the blank's silhouette, a proposed feature or a face under a
  head, and nothing standing above that height outside a proposed feature
  — one `Plane(z=…)` for every such floor at the height, the whole blank
  cleared to it, the engine deriving islands and voids from containment.
  Other below-top +Z planes are pocket floors, bore floors or ledges
  (`Face`), each by its own pass.
- **cylinder** → `CylinderBore(dia=…, extent=…, x=…, y=…)`: only a full-revolution
  face with axis parallel to +Z, opening at the top, **material outside the
  surface** (a bore). A boss (material inside), tilted axis, or partial face
  is residue with the reason saying which check failed. For a graduated
  wall, **the bottom face decides hole-vs-bore semantics** (the hole-vs-bore
  slice): air on the axis past the far rim, and no upward plane across
  it, means a *through hole* (`_Hole`, consume with `Form` at points) —
  the rim may break out across a step onto several faces, tilted or
  vertical ones among them, and the entry lists them; an on-axis, apex-down, full-revolution
  internal cone spanning the full mouth means a *drilled blind hole*
  (`_Hole`, grouped with the cone, `extent` reaching the **tip** — what
  a `Form` at points sends there — with the included point angle reported); an
  island-free upward plane means a *milled flat-bottom bore* (`_Bore`,
  grouped with the floor, consume with `Side` — a twist drill cannot
  leave a flat bottom, so no drill run is suggested); any other bottom
  falls back to a wall-only `_Bore`, the bottom face keeping its own
  classification. A bore whose rim lies below the stock top is placed by
  `z=` at its rim, except a drilled `_Hole` with a plain mouth and open air
  straight above it to the top: that one proposes from the top, its
  `extent` the whole way down, since a drill through the blank starts
  there and the step above its rim is another feature's to cut. A hole
  with a mouth chamfer or round keeps its rim, where that mouth is; a
  milled `_Bore` keeps its rim, where its wall finish starts.
- **sphere** → `Sphere(dia=…, x=…, y=…, z=…)`: only a full-revolution convex
  face (material inside — a dome/boss) entirely at/above its equator. A
  concave bowl or a face dipping below the equator (the undercut sub-equator
  band) is residue.
- **cone** → `Cone(dia=…, height=…, x=…, y=…, z=…)`: only a full-revolution,
  axis-parallel-+Z boss (material inside), base down, with the face reaching
  the apex. Internal walls, widening-upward faces (undercut), frustums, and
  tilted axes are residue.
- **torus** → `Torus(major_dia=…, minor_dia=…, x=…, y=…, z=…)`: only a
  full-revolution, axis-parallel-+Z ring (material inside the tube) entirely
  at/above its equator plane. Internal grooves/fillets and faces reaching
  below the equator (the undercut underside) are residue.
- **face** (a below-top planar +Z face capping a recognized outer profile —
  a ledge, a step's top) → a multi-line `Face(z=…` block: one `loop` line
  and its `x y` points per wire, the OUTER wire first (wound CCW), then
  every inner wire (wound CW) — STEP's own statement of the face, carried
  over. Every edge curve is accepted and chorded at `CHORD_TOL`
  (`edge_not_line_or_arc` does not apply to a face's boundary: it is
  overhung by a tool radius or stood off from, never the surface). Until
  Phase 28 this face was the `outer_profile_anchor_cap` residue, for want
  of a word; that code is retired (see Versioning). What stands on an inner
  wire proposes through its own classifier — or stays residue on its own
  faces, for the reviewer to declare.
- **chamfer** (Phase 46 slice 3, absorbed, never a noun): a planar strip
  that is neither vertical nor ±Z, or a cone with its axis ∥ +Z, bounded
  above by a +Z plane and below by a vertical wall is that wall's edge
  break — a member of the wall's group, written as the engine's own keys
  `chamfer_depth=` (the strip's height) and `chamfer_angle=` (the included
  angle, twice the flank's angle from the vertical: the ChamferMill's
  convention), on the boss whose cap it rings, the pocket whose mouth it
  rims, or the bore whose mouth cone it is. A boss's loop is then read at
  the WALLS (their top edges, chained), never at the cap's wire; a pocket's
  mouth is the bevel's top. Round a boss's cap the legs may differ edge to
  edge, and a wall may rise to the cap unbeveled (ROADMAP item 1 slice 9):
  the feature carries the leg that runs the longest way round the loop as
  `chamfer_depth=`, and a point line carries `chamfer_depth=<leg>` where
  the leg changes from the edge before it, `chamfer_depth=0.0` where the
  edge is unbroken; every strip is a member. Strips at two angles are two
  tools' work and stay `chamfer_uneven`. The sidecar writes what the model measures and
  the engine's three chamfer words are pick-two, so a print that says
  otherwise is typed on the feature line. A bevel that is a wall's whole
  height and a bevel at a wall's foot refuse by name (the table above).
  Strips round a boss's cap that disagree (1.0 on some edges, 1.5 on
  others — the real part's ribs) do not refuse the boss: the profile
  proposes without a chamfer, its loop still read at the walls, and each
  strip stays residue naming the bevels it saw, for the hand to break.
  Round a pocket's mouth they still refuse.
- **far mouth** (Phase 48 slice 3, absorbed, never a noun): a cone with
  its axis on a graduated bore's, void inside, whose NARROW rim is the
  bore's bottom rim and whose WIDE rim lies in a downward plane — the far
  face — is that hole's mouth bevel seen from the wrong side: the chamfer
  the OTHER setup cuts. The hole proposes THROUGH to the far face (`_Hole`,
  its `extent` the rim to the far face, as a plain through hole's is) and
  the cone is a member of its group, its `why` naming the other setup; no
  `chamfer_depth=` is written, because this setup cannot cut it. Imported
  turned over (`--up z-`) the same cone is the near mouth's bevel and IS
  the bore's `chamfer_depth=`, so the two imports agree on the hole's
  column — which the engine's `given from` certificate reads. A cone
  widening downward that is not a graduated bore's far mouth stays
  `cone_cavity_undercut`.
- **pocket** (grouped: floor + wall loop) → a multi-line `ProfilePocket(…)` block:
  only a below-top planar +Z floor — interior wires allowed since Phase 26:
  what stands on one proposes beside the pocket through its own classifier,
  and the engine derives it as the pocket's island from containment — whose
  boundary
  is lines/circular arcs, bounded by **vertical** walls (planes with
  horizontal normal, or partial-revolution cylinders with axis ∥ +Z) rising
  to a single mouth height, where the mouth rim adjoins upward-facing planar
  faces (**the load-bearing undercut check** — an overhanging lip puts a
  downward face at the rim, and a single-point probe alone would miss it
  when the lipped mouth is open over the floor's centroid), plus the bore's
  solid-classifier probe just above the mouth as belt and suspenders. A
  tilted wall, stepped walls, a plateau (walls descending), or a
  lipped rim all reject the *floor* into the residue naming the failed
  check; the wall faces keep their own per-face classifications. Since
  Phase 46 slice 2 a face on the floor's wire is first read for what it
  IS: a full-revolution cylinder with material INSIDE it spanning the
  floor's height is the part's **silhouette** — the blank's edge, open
  beyond; the loop keeps its arc, drawn two chord tolerances PAST the
  circle (the engine clips every outline to the declared stock, so on a
  `stock Cylinder` of the part's diameter the edge reads OPEN and on a
  larger Box it reads WALL — and an edge drawn ON the circle would clip
  into slivers against the engine's own chords of it); a full-revolution
  bore whose bottom is BELOW the floor is a **void the edge runs along**
  — the loop keeps its rim arc, a wall the Clear rides to, and the bore
  proposes beside the pocket. Neither is a member of the group and neither
  is subject to the wall checks; the `why` names both. A floor with no
  standing wall at all — every edge the silhouette or a void's rim — is a
  bounded `Face` (the ledge's noun), open all round, with what stands on
  its inner wires proposing beside it.

The checks are read off each `Feature`'s constructed semantics, so
recognition can only propose what the finishing math already handles
(`DECISIONS.md` → "Quadric recognition…").

An undercut face is therefore rejected at the recognition boundary by landing
in the residue with an explicit reason — never proposed, never dropped.

## `proposed.part`

Plain `.part` feature text, one `feature` statement per proposal, each
preceded by a `#` provenance comment naming the source file, face id, and
why. It is a **fragment for human review** — it has no stock/tool/run
preamble and is not runnable as-is; a human or agent accepts lines into a
real project file. This is deliberate: recognition proposes, it does not
machine.

Its shape, stated so a consumer can reproduce it: the `#` header block, a
blank line, then per proposal its `#` provenance line (`# F1: <why>
(CHECKLIST row <row>)`) and any generated `#` suggestion lines, then the
statement — proposals in the report's traversal order, a grouped block
once at its primary face — with **one blank line between proposals, a
trailing newline, and no trailing blank line**. The `#` lines are the
sidecar's voice and are NOT in the report (the report carries `part` and
`why`, never the suggestions); what the report does carry is the statement
text exactly, so a consumer's accept-all reproduces this file's statement
bytes and nothing else (`tests/review_tests.rs`, the accept-all pin).

When a setup declaration shaped the import, the file header says so (a
`# solid S1 of 2 …` and/or `# imported with --up y+ …` line) — a reviewer
must see the setup the proposed coordinates assume.

Recognized **contour** proposals (pockets, since the pocket slice) propose
`comp=control` on any consuming `Side` run they suggest, per the settled
decision — engine comp hard-errors on rounded-corner offsets until true arc
emission lands. The requirement is carried as generated `#` comment lines on
every profile proposal (suggested `run Clear(…)` / `run Side(…,
comp=control)` invocations plus the one-line why; the op words were
`Pocket` / `Contour`, then `Clear` / `Finish`, and are `Clear` / `Side`
since Phase 36), emitted unconditionally —
even a sharp-cornered boundary gets them, so the suggestion never depends on
tessellation details.

The suggested-run comments are advice to a reader; the run lines themselves
are written by the engine, `nacs --schedule <project>` (ROADMAP item 1,
slice 10), from the accepted features and the project's own tool lines. The
sidecar never names a tool.

Recognized **cylinder** proposals (since the hole-vs-bore slice) carry the
same style of suggested-run comments, keyed to what the bottom face proved:
`run Form(points=[…])` on `_Hole` proposals (with the tip-vs-full-diameter
depth caveat on drill-point bottoms and the breakthrough note on through
holes) and `run Side(…)` on `_Bore` proposals (with the one-line why no
drill run is suggested). Depth caveats matter because a visit to points
measures `depth`
from the **stock top** (`derive_z_cut`), the same convention the pocket
comments already state.

## Stock proposal — `import_stock.py`

The blank for a model, as one `.part` statement the Rust core already
reads, and a comment saying which of two answers it is:

```
python sidecar/import_stock.py <model.step> -o <outdir> [--up z-] [--solid N] [--allowance 1]
                                                       [--given <project> --blank <words>]
```

writes `<outdir>/proposed_stock.part`:

```
# Proposed stock from l_plate.step by sidecar/import_stock.py
# (Phase 54 slice 5b): the topmost flat face's outer wire as the
# plan outline, the solid's Z extent as the height, the top as z.
stock Profile(h=20.0, z=20.0
    0.0 0.0
    120.0 0.0
    ...
)
```

The solid is selected and up-rotated exactly as `import_step.py` does,
so a stock and a target proposed from one file share a frame.

A **prism** (every face flat on top, flat on the bottom or vertical, and
the topmost face spanning the solid's whole XY extent) is its own blank:
the `stock Profile` above, whose outline is the topmost flat (+Z) face's
OUTER wire, wound CCW, circular edges chorded at `CHORD_TOL` as every
loop here is; the height is the solid's Z extent; `z` its top.

Anything else gets **the smallest blank the model fits**, one line, with
the comment above it saying so and why the model is not a prism:

```
# Proposed stock from disc_step.step by sidecar/import_stock.py:
# the smallest blank the model fits, not the model's own outline --
# its topmost flat face (z 30.0) spans ... so something stands on its
# top. Add the allowance you will cut off.
stock Cylinder(dia=120.0, h=30.0, x=0.0, y=0.0, z=30.0)
```

It is a `stock Cylinder` when one vertical cylindrical face bounds the
plan on all four sides (a disc, a ring, a round casting; `x`, `y` the
axis), a `stock Box` otherwise (`x`, `y` the low corner); `h` and `z` as
the Profile's. The size is the model's exact extent unless `--allowance`
says how far the blank stands outside it: a bounding blank then grows by
it on the sides and the top (`dia + 2a`, `h + a`, `z + a`; a Box's low
corner moves out by `a` too), the comment says so, and a prism — its own
blank, cut to size — is left alone with a comment saying the allowance was
not added. The engine's import doors pass `--allowance 1` unless the person
types another number (`allowance=` on the Output line, `--allowance` on
`nacs --import`, the field under the import path), so the proposed blank is
the one the shop saws, and the top face and the rim become passes
`nacs --schedule` writes. The stdout line names the
kind and the size. Nothing is written only when the solid cannot be
chosen (an assembly with no `--solid`), which is an error, not a
proposal. The window reviews the file like every other proposal: the
Review lists it first, Accept writes it.

A **second setup** (`--given <project> --blank <words>`, slice 12) is the
third answer: the blank is the earlier program's turned over, as the engine
computed it from that program's own stock, and the proposal is two
statements — the stock, and the `given from <project> up=<axis>` line the
engine's validator demands beside it (the blank turned over must be the
blank; the earlier program's cuts are air here):

```
# Proposed stock by sidecar/import_stock.py from ../front.cam, the program
# before this one (--given): its blank turned over (up=z-), not
# measured from the model -- ... It stands 1.0 outside the model's
# plan and 0.0 above its top.
stock Cylinder(dia=120.0, h=30.0, x=0.0, y=0.0, z=0.0)
given from ../front.cam up=z-
```

The model is only checked to lie inside the blank; outside it on any side
is an error. The engine's import doors send both flags when the person
names the earlier project (`given=` on the Output line, `--given` on
`nacs --import`, the field under the import path) and drop `--allowance`
beside them. Accepting the proposal into a fresh folder writes both lines,
so the second setup's project knows the first without a line typed.

## DXF tool profiles — `import_dxf_profile.py`

The sidecar's second tool (ROADMAP Phase 4, "Custom tool/cutter profiles
from DXF"), under the same ground rules — argv + files, batch, diffable
text out, one-line stderr + nonzero exit + **no partial output files** on
error:

```
sidecar/.venv/bin/python sidecar/import_dxf_profile.py <profile.dxf> -o <outdir>/
```

| File          | When       | What                                          |
|---------------|------------|-----------------------------------------------|
| `profile.svg` | on success | the cutter's half-profile as one SVG `<path>` |

The output is **not a new seam**: it is exactly the SVG form the Rust
parser's `tool T? FormTool(dia=..., profile="…")` already reads, so the
Rust core and the `.part` grammar are unchanged — DXF support *is* this
conversion (`DECISIONS.md` → "DXF tool profiles…"). The SVG carries XML
provenance comments (source basename, the consume-with tool line) and a
viewer-only `scale(1 -1)` group so it reviews upright in any SVG viewer;
the engine reads only the path's `d` attribute.

Input rules (each violation rejects with the reason named, never a guess):

- entities: LINE, ARC, LWPOLYLINE (bulge arcs included) — any other
  modelspace entity rejects naming its type; no silent spline/ellipse
  approximation;
- connectivity: the entities must chain (endpoints within the linear
  tolerance) into exactly **one open polyline** — a branch point, a closed
  loop, or disconnected pieces each reject. Entity order and direction in
  the file do not matter.

Scale, position, and orientation of the drawing do not matter: the Rust
parser rescales the profile to the tool's declared `dia` and re-measures
heights from the min-radius point (the tip).

Determinism matches the report's rules (fixed traversal, one number
formatter, arcs tessellated at the same chord tolerance, no timestamps or
absolute paths); the committed golden
(`tests/fixtures/form_tool.profile.golden.svg`) is the normative example.

## STEP → mesh export — `import_step_mesh.py`

The sidecar's third tool (ROADMAP Phase 4, the camp-3 follow-up "sidecar
STEP→mesh export for freeform faces"). It is the complement of recognition:
`import_step.py` proposes clean camp-1/2 primitives for every face it can read
as an equation; this tool tessellates the **freeform** tail it cannot — the
b-spline, bézier, surface-of-revolution/extrusion, and offset faces no
equation captures — into the STL that `Feature::Mesh` already reads (finished
by the raster drop-cutter, `DECISIONS.md` → "Camp-3 slice 1…"). Same ground
rules — argv + files, batch, diffable text out, one-line stderr + nonzero exit
+ **no partial output files** on error:

```
sidecar/.venv/bin/python sidecar/import_step_mesh.py <model.step> -o <outdir>/
    [--solid N] [--up {x+,x-,y+,y-,z+,z-}]
```

| File            | When                 | What                                          |
|-----------------|----------------------|-----------------------------------------------|
| `mesh.stl`      | on success           | the freeform faces as an ASCII STL triangle soup |
| `proposed.part` | on success           | a reviewable `feature … Mesh(file="mesh.stl")` line |

stdout: a one-line summary (`N faces: F freeform tessellated (T triangles),
A analytic skipped for recognition`).

`--solid` / `--up` are **the same setup declarations** as `import_step.py`
(same `select_solid` guard, same `UP_ROTATIONS`): a multi-solid STEP rejects
unless `--solid` picks the part, and `--up` names which model axis is up and is
rotated onto machine +Z **before** tessellation — so the drop-cutter's
"up-facing" reachability is decided in machine orientation, exactly as
recognition's checks are.

The split is by surface type: a face is analytic (left for recognition) iff its
`geomType()` is `PLANE`/`CYLINDER`/`CONE`/`SPHERE`/`TORUS`; everything else is
freeform and tessellated here. **A part with no freeform face rejects** — there
is nothing to tessellate and an empty STL is never written (the error points
back at `import_step.py`). This is *not* a total-accounting report: the analytic
faces are deliberately, not silently, skipped (recognition owns them), and the
`proposed.part` header states the freeform/analytic counts so the division of
labour is on the page.

**Not a new seam.** Like the DXF tool, the output is exactly the form the Rust
core already consumes — `src/mesh.rs::read_stl` reads the STL, the unchanged
`.part` grammar reads the `Mesh` line — so mesh export adds nothing to the Rust
core. Undercut freeform faces need no check here: the drop-cutter machines the
reachable *upper envelope*, leaving overhangs standing (the camp-3 re-entrancy
discharge-by-construction, `DECISIONS.md` → "Camp-3 slice 1…").

Determinism: `BRepMesh_IncrementalMesh` at a fixed linear + angular deflection
(`MESH_DEFLECTION_MM`, a *separate* error class from `CHORD_TOL` and from the
finisher's `stepover` sample budget — the mesh's own chord error), facets in
kernel face-traversal order, triangles in triangulation order, every number
through the shared `fmt()`. The committed goldens
(`tests/fixtures/freeform_cap.mesh.golden.stl` +
`tests/fixtures/freeform_cap.proposed.golden.part`) are the normative example.

## Analytic spline-of-revolution — `import_step_nurbs.py`

The sidecar's fourth tool (ROADMAP Phase 4 follow-up "an *analytic* NURBS
reader — machine the patch without tessellating"; `DECISIONS.md` → "Analytic
spline-of-revolution reader…"). It is the **analytic counterpart** to mesh
export: where `import_step_mesh.py` tessellates a freeform revolved face and the
drop-cutter accepts the stair-step error, this reads the **same kind of face** —
a `SURFACE_OF_REVOLUTION` whose basis curve is a b-spline — and writes its
meridian out as an *exact* `.spline` curve, so the Rust core's
`Feature::SplineRevolve` machines the surface with **no tessellation error**
(`ops::finish::finish_spline_revolve`, the R2 pointwise-offset machinery, the
same regime as the catenoid/pseudosphere/vase). Same ground rules — argv +
files, batch, diffable text out, one-line stderr + nonzero exit + **no partial
output files** on error:

```
sidecar/.venv/bin/python sidecar/import_step_nurbs.py <model.step> -o <outdir>/
    [--solid N] [--up {x+,x-,y+,y-,z+,z-}]
```

| File               | When                 | What                                          |
|--------------------|----------------------|-----------------------------------------------|
| `meridian.spline`  | on success           | the meridian b-spline (`meridian_2.spline`, … for more than one such face) |
| `surface.patch`    | on success           | an untrimmed general b-spline patch's control net (`surface_2.patch`, …) — see "The general patch" below |
| `proposed.part`    | on success           | reviewable `feature … SplineRevolve(…)` / `SplineRevolveFunnel(…)` / `NurbsPatch(…)` line(s) |

stdout: a one-line summary (`N faces: M analytic spline-of-revolution
meridian(s) read`, extended with `+ K untrimmed b-spline patch(es)` when
patches were read).

`--solid` / `--up` are **the same setup declarations** as the other tools (same
`select_solid` guard, same `UP_ROTATIONS`): a multi-solid STEP rejects unless
`--solid` picks the part, and `--up` rotates the declared up axis onto machine
+Z **before** the meridian is read — so the finisher's top-down reachability is
decided in machine orientation.

It is deliberately narrow, and every miss is an honest error (never a guess,
never a partial output): a face qualifies only if it is a **full-revolution**
`SURFACE_OF_REVOLUTION`, its basis curve is a **b-spline** (a line basis is a
cone, a circle a sphere/torus — recognition's job), and its **axis is parallel
to +Z**. A partial revolution, a tilted axis, a non-b-spline basis, or a part
with no such face all reject with the reason named — the no-such-face error
points at the other two tools. This is *not* total accounting: like mesh export,
it reads only the faces it can take and leaves the rest.

**Both orientations read** (`DECISIONS.md` → "Spline funnel…"). The face's
oriented outward normal (the shared `face_normal`, the bore-wall material-side
precedent) tells them apart by its **radial component** at mid-parameters:
positive ⇒ material inside, a **cap**, proposed as `SplineRevolve(file=…, x, y,
z)` with the meridian stored apex-first; negative ⇒ the normal points into the
void, a **cavity**, proposed as `SplineRevolveFunnel(file=…, x, y)` with the
meridian stored **mouth-first** and **no z field** — the Rust variant puts the
mouth circle in the stock-top plane by construction, so the model's mouth
height is reported in a comment for the reviewer to set the stock by. A
near-zero radial component (an axial normal — a wall that could be either) is
rejected by name, not guessed. Feature names suffix `_Cap` / `_Funnel`.

**The general patch** (`DECISIONS.md` → "the general NURBS patch…", slice 1).
The same tool also reads a general `B_SPLINE_SURFACE` face — but **only over
its full natural domain**: the face's UV box, wire count, and area must all
match the untrimmed surface's own. A **trimmed** patch is never read as its
full rectangle (that would invent surface the part does not have, and
tessellating the trim boundary is a guess, not a reading) — it stays with
`import_step_mesh.py`, and the nothing-read error counts such faces by name.
A qualifying face must also be machinable from above (outward normal with an
upward component at mid-parameters — a downward or vertical patch is skipped
like any other non-qualifying face). The control net goes out **exactly** as
`surface.patch` (row-major `pole x y z` lines, mm, model coords — placement
`x`/`y`/`z` on the proposal default 0), consumed by `Feature::NurbsPatch`, the
raster drop-cutter against the exact surface. Feature names suffix `_Patch`.

**Not a new seam.** Like the DXF and mesh tools, the output is exactly the form
the Rust core already reads — `src/nurbs.rs::read_spline` reads the `.spline`,
the unchanged `.part` grammar reads the `SplineRevolve` line — so the analytic
reader adds nothing to the Rust core's seam. The `.spline` grammar (line
oriented, `#` comments ignored):

```text
degree 3
knots 0 0 0 0 4.123 9.123 14.954 19.226 22.267 22.267 22.267 22.267
pole 0 16
pole 1.412 15.942
...
weights 1 1 1 …        # optional; absent => non-rational
```

The `(r, z)` control points are the meridian half-plane in **millimetres by
definition** (the STL rule — `units inch` scales only the placement offsets,
never the curve), stored **top-first**: apex-first for a cap (apex on the axis
at the top, base rim at the bottom), mouth-first for a cavity (mouth rim
widest, at the top, tip at the bottom) — each envelope's canonical order
starts at its top, so one normalization serves both. The Rust validators
enforce the machinable envelopes — the cap's radius non-decreasing / height
non-increasing (`check_machinable_cap`), the funnel's mirror plus open-mouth
and depth preconditions (`check_machinable_funnel`) — so an undercut meridian
is rejected, not machined into a gouging path; `freeform_cap` (whose meridian
bulges under) stays on the mesh path for that reason.

Determinism: fixed face-traversal order, the flattened knot sequence, every
number through the shared `fmt()`. The committed goldens
(`tests/fixtures/spline_cap.meridian.golden.spline` +
`tests/fixtures/spline_cap.proposed.golden.part`, from
`sidecar/fixtures/make_spline_cap_step.py`; the cavity branch's
`tests/fixtures/spline_funnel.meridian.golden.spline` +
`tests/fixtures/spline_funnel.proposed.golden.part`, from
`sidecar/fixtures/make_spline_funnel_step.py`) are the normative examples.

## Determinism

Both outputs must be byte-identical across runs on the same input, so they
can be goldens:

- fixed face traversal order (the kernel's `TopExp_Explorer` order), ids
  assigned in that order;
- JSON with sorted keys, `indent=2`, trailing newline;
- no timestamps, no absolute paths, no environment details in either output;
- all numbers formatted through one formatting function (fixed decimals,
  no `1e-16` noise).

## Versioning

v1 grows **additively only**: new optional fields and new `surface` values
may appear under `"version": 1`. Removing, renaming, or repurposing a field
revs the version — so does renaming a residue code, which a consumer keys
on; adding one is additive. A code a GRADUATION retires simply stops
appearing and is never reused for another meaning: `pocket_islands` (a
pocket floor with interior wires) retired 2026-09-19 when the engine
learned to clear around islands, and `outer_profile_anchor_cap` (a
below-top cap of a recognized outer profile) retired 2026-09-19 when the
bounded `Face` gave it something to propose as, and
`outer_profile_walls_stepped` (wall bases at more than one height)
retired 2026-10-29 when a loop learned to propose to its highest wall foot
and name the walls that run on below it — a consumer keyed on any of
them sees fewer refusals, never a different one. The committed goldens
(`tests/fixtures/block_bore.report.golden.json`,
`tests/fixtures/quadric_boss.report.golden.json`,
`tests/fixtures/pocket_plate.report.golden.json` — the normative example of
grouped proposals — `tests/fixtures/drilled_plate.report.golden.json`, the
normative example of hole-vs-bore semantics, and
`tests/fixtures/assembly_pair.report.golden.json` /
`tests/fixtures/tilted_plate.report.golden.json`, the normative examples of
the setup-declaration fields) are the normative examples of this schema.

## What the Rust core does (and does not) consume

In this slice the Rust core consumes only `proposed.part` — its native text
format, via the unchanged parser. `report.json` is human/agent-facing; no
JSON parser exists in the Rust core, on purpose (the lean-core rule). When
Rust first needs to consume the report (e.g. an `nacs import` wrapper),
`serde_json` is the pre-blessed path — see the DECISIONS entry. That day
came with Phase 17 lane A: `src/review.rs` is the Rust reader of this
report (`nacs --review` / `--accept`), behind the `frontend`/`agent`
features that own `serde_json`, so the lean core is still JSON-free.
