# Decisions

What is settled, and why, in plain English. Read before changing how a cut is
made, what the `.part` grammar means, or what the program depends on.

Rules for this file:

- An entry is **under ten lines**. If it needs more, the code comment or a test
  is the better home.
- An entry exists only when the call **changes the metal, the grammar, the
  G-code, or a dependency**. How the code is arranged is a comment, not a
  decision.
- Entries are grouped by topic, not by date. Overturn one by editing it and
  saying when.
- The long history of how we got here, June to October 2026, is not in this
  repository. A code comment that cites a `DECISIONS.md` entry by a title
  not found here, or `DESIGN.md`, a phase or a slice, refers to it.

---

## The project

**A project is a folder of text.** `job.cam/main.part` holds the stock, the
tools, the header and the `run` lines; `geometry.part` holds coordinate
lists. No binary project format, ever. Agents and humans edit the same file.

**`run` order is execution order.** No dependency graph between operations.
An op may read what earlier ops left (the stock model does), but nothing is
wired op to op.

**Internal units are millimetres.** `units inch` converts every linear value
once, at the end of parsing. The program always emits `G21`.

**One geometric tolerance.** `CHORD_TOL_MM` in `lib.rs` is the only chord and
approximation tolerance. Never an inline epsilon in an op.

**Derived values have one home.** Anything computed from `Part` fields is a
`derive_*()` function called wherever it is needed, never stored or restated.
The tool-number to D/H/T register drift this prevents is the bug the project
was started over.

**The name is nacs; the binary is nacs.**

**Dependencies are a short list with a reason each.** Engine: `cavalier_contours`
(2D polyline offsetting with arcs, pure Rust, chosen over Clipper2's FFI).
Window, optional: `eframe`, `serde_json` (also the MCP reader), `windows-sys`
(one blur call, one file dialog), `egui_kittest` and `image` (tests only; since
2026-11-07 the harness also renders the frame to a PNG, so the agent can look at
the window it drives). STEP import:
a Python CadQuery/OCP sidecar, 1.5 GB, optional. Adding a crate is a decision
recorded here first.

## Geometry

**One math library on every platform (2026-11-14).** `sin`, `cos`, `atan2`, `hypot`
and kin go through the pure-Rust `libm` crate (`src/math.rs`, written `x.psin()`),
never the std methods, which call the operating system's C library and differ
in the last bit between Windows and Linux. A last bit flipped ties and
thresholds, so the two platforms could not share goldens. Seven goldens moved
when this landed: a loop started at another vertex, a link took the arc, a Z
moved 0.001. The window's own drawing (`frontend.rs`) stays on std.

**A printed number is snapped before it is cut to its decimals (2026-11-14).**
`gcode::snap` rounds to nine decimals first. A ramp over sixteen equal steps
computes Z = -0.5625 give or take the last bit, and that bit chose `-0.562` or
`-0.563`: a debug and a release build of the same source disagreed. Five
goldens moved by a last digit. CI's `agree` job fails if Windows and Linux
write a different program for any fixture.

**A close call has a written rule (2026-11-20).** Where the engine picks the
nearest, shortest or cheapest of several candidates, any within 1e-6 mm of the
best are tied and the first in a stated order wins; a length within 1e-6 mm of
a limit is within it (`src/tie.rs`, each caller names its order). A loop's
start is the exception with a reason: parallel loops are equally near all
along a straight run, so among tied start points the one nearest where the
tool already stands wins. Cutting length, descents and lifts are unchanged on
every fixture; 32 goldens moved by where loops start, and rapid travel over
the corpus fell 8%.

**"It's a wall now."** Features are declared as exact analytical primitives.
Imported CAD is a canvas; recognition proposes primitives and never machines
a mesh. The declared feature is the authority, the part model is context.

**Three camps, three axes.** Camp 1 is 2.5D (profile plus depth). Camp 2 is
analytic surfaces (quadrics and revolves), machined closed-form. Camp 3 is
mesh and NURBS, the escape hatch that accepts tessellation error. All of it is
3-axis. Multi-axis is out of scope.

**A feature owns its vertical span.** `extent` on the feature, from its top
datum, is the depth. `depth` is not an operation input.

**`stock_to_leave` is one radial allowance.** It offsets the surfaces the op
defines. A pocket's floor keeps the allowance only when a later `End` takes
that floor.

**Islands and keep-outs are derived, never declared.** A feature standing in
another's footprint is an island by containment; a `fixture` is a keep-out.
Overlaps the grammar cannot mean are refused at validate with both names.

**A boss may stand taller than the cavity it is in (2026-11-26).** It is that
cavity's island from floor to mouth. It was refused outright; now it is
refused only when no other feature is declared to cut from the boss's top
down to the cavity's mouth round it, since without that its upper part
stands in uncut material. That other feature takes it as its island too.

**A pocket that runs out of the blank may notch a boss (2026-11-26).** A
cavity crossing a standing wall was refused whatever else was true. It is
allowed now when every part of the cavity outside the boss reaches past the
blank: the pocket is open on that side, and the stock between the wall and
the blank's edge is the wall's own cut to remove. A crossing that stays
inside the blank is still refused.

**An op works one planar region.** Its edges are read off the blank: wall,
open, or bridged. A step is a pocket drawn past the stock. A boss touching the
boundary is part of the shore, not an island.

**A loop's winding is its side.** Outer loops run counter-clockwise, inner
loops clockwise; the wrong winding is refused, as are repeated vertices and
self-intersections. The window offers to reverse a loop; the parser's law
stands.

**A `Face` is loops within loops.** An inner loop with nothing standing in it
is judged by the tool: shrunk by one diameter, either something is left (go
around it) or nothing (bridge it).

**`given` names air cut on another setup.** `given <feature>` subtracts the
void before the first op. `given from <project> up=<axis>` loads the earlier
program and turns its removals into this frame; anything it cannot turn is
declined by name and the model is said to be incomplete.

**Stock is Box, Cylinder or Profile.** Every closed-form reader matches on the
shape; the bounding box is kept for all three.

**The feature vocabulary is what the sidecar can propose plus what a person
would type.** (2026-10-28.) Eleven kinds that existed only because the shape
taxonomy had a row (Ellipsoid, EllipticBore, EllipticBoss, Paraboloid,
ParaboloidBowl, Ogive, Catenoid, CatenoidThroat, Pseudosphere,
PseudosphereFunnel, Vase) were deleted with their emitters, fixtures and
goldens: no recognizer can name them, nobody types them, and a face of any of
those shapes arrives from STEP as a `SplineRevolve` or a `Mesh`, which the
engine already machines. Sphere, Cone, Torus and Cylinder stay because the
sidecar names them and their offsets are exact; `TiltedBore` stays because a
person declares it.

## Operations

**Verbs name the part of the tool that cuts.** `End` (flat end on a floor),
`Side` (flank on a wall), `Copy` (ball nose on a surface), `Form` (a tool that
carries the shape), `Clear` (volume removal). `points=[...]` on a `Form` or
`End` is a visit, which is what drilling is. The verb is checked against tool
and feature and never corrected silently.

**A run line names one feature.** `surface=` for End/Side/Copy/Form,
`profile=` for Clear, `points=` for a visit. A second reference key is
refused.

**Unmachinable is a hard error.** A pocket too small for its tool, a loop the
offsetter cannot take, a move into material that outlives its op: the program
is refused with one line on stderr and nothing on stdout. Never a skipped op,
never a rewritten move.

**Offset clearing runs inside out** with a retract between loops, and enters
by ramping along the first loop. Every entry into solid obeys one ceiling,
`RAMP_DEG` (3°): a short first stroke laps until the angle is met, and a
stroke under one diameter plunges at the plunge feed and the program says so.

**Five floor patterns, chosen by one word.** `pattern=offset|zigzag|oneway|
spine|held`. `spine` is the default: rows cut only where they own floor, one
shore pass along what stands, a rail pass where one pass covers the width, the
comb chosen by length and lifts alone. `held` is spine's comb at the spacing a
stated engagement ceiling gives, lifted where a link would exceed it.
`nacs --patterns` measures all five through the real emitter; no score picks.

**Rows close along what stands.** Every row pattern ends with the shore pass
and claims only the floor it cut. The offset family alone claims one diameter
past an open edge.

**`Clear` takes a pocket, a `Plane` or a `Face`.** `stepdown` is its axial
bite. `stepover` for any floor op is read through `derive_stepover`.

**Drilling is unrolled `G00`/`G01`.** No canned cycles; portable everywhere.

**Wall finishing has one `comp` mode per op.** Engine-computed offsets by
default; `comp=control` opts into `G41`/`G42` with the `D` register.

**Camp-2 surfaces are machined by waterline or constant scallop.** Natural
quadrics offset themselves exactly; the rest use pointwise contact with a
numeric scallop march. A concave gouge check guards both.

**Camp 3 is a raster drop-cutter over STL.** It is the one op that accepts
sampling error, and it is the only one. A revolved NURBS meridian is read
analytically instead.

**Edge breaks are declared on the feature they break.** Chamfer and corner
round are cut by the tool the print implies, positioned from its ground arc,
never from the relieved pilot.

**A loop point may restate the chamfer's leg.** On a `ProfilePocket` or
`ProfileBoss` a point line may carry `chamfer_depth=` or `chamfer_width=`
after its coordinates, in force from that point's outgoing edge until a later
point restates it; `0` leaves the edges unbroken and the pass lifts off them.
The feature's own keys hold from the first point. The angle is the loop's,
since one tool cuts one angle. The pass runs the loop in stretches of one leg,
moving the tip at the point where the leg changes, so a deeper stretch reaches
about a reach's width round the corner into a shallower one; the removal model
claims one band per stretch, each at its own depth.
(2026-11-01: the real part's rib heads, 1.5 on the straight edges and 1.0 on
the arc a bolt hole notches.)

## The program

**Four safety rules, one test.** Units before motion, `G43 H` before a tool's
first cut, retract to `clearance_z` between ops, spindle on before any feed.
Every fixture is in the list of `test_gcode_safety_lint`. The header law
refuses zero feeds and safe heights below the stock top.

**The post is a formatter, never a fixer.** The program is emitted in one
dialect keyed off a declared `machine`. Every check gauges the emitted text,
so nothing may change it afterwards. A user post is the escape hatch for a
controller we do not model; its ideal size is zero.

**The check gives an account.** `--check` prints on stdout what each
finding family judged, found and could not judge, and why one had nothing
to judge; `--check=json` carries the same as `checked`. A decline names the
family it stood in for. One home, `check_result::account`, read by the CLI,
the MCP verb and the window. Stderr and exit codes unchanged. Since
2026-11-03 (ROADMAP item 2, slice 6).

**The lead has a shape.** `lead=` (mm from the start), `lead_angle=` (0
behind the start on the pass's own line, 90 off to the air side, the
default) and `lead_descent=` (degrees below flat) shape every lead an op
makes. Said, the lead is whole and in air or the op refuses: a closed path
turns its start to where it is. The sentence names length, line and slope;
the flight onto the start carries Z where it descends. Refused beside
entry=plunge|ramp, on rows and the raster, and as an oblique lead on an open
wall. Since 2026-11-03 (ROADMAP item 2, slice 5).

**Every descent may be made one kind.** `entry=ramp|plunge|lead` on a
Clear, an End or a Side makes each descent in the op that kind: rows,
shores and loops ramp down their own first stroke or lap; a wall trace
plunges on its path for its lead; a lead is kept where the plan makes one.
Where a descent cannot be that kind the op refuses by name — a ramp down a
wall or a ring, a lead into a row, a stroke under a diameter. The sentence
before a changed descent ends `as entry=… asks`. A ring's descent at its
lead point is a lead, and says so. Since 2026-11-03 (ROADMAP item 2, slice 4).

**The rows' direction may be set.** `angle=` on a Clear or an End cutting
rows pins the direction spine's comb and the parallel rows would choose:
degrees from +X, 0 up to but not including 180, since rows have no sense.
The comb then chooses only its phase and its way, and tries no pair of
combs. The pattern sentence says `the angle set` after the angle. Refused
where no rows turn: a Side, a Copy, a Form, a visit to points, the offset
family, the raster. Since 2026-11-03 (ROADMAP item 2, slice 3).

**The direction is said, and may be set.** Every Clear, End and Side block
carries one `(direction: …)` line before its first move. The engine's passes
climb — the material on the
tool's right under M03, G41 under control comp — and now say so; rows that
alternate say they run both ways. `direction=climb|conventional` on those
three verbs turns every pass with a side: each loop and ring the other way
round from the same start, G42 for G41. Where nothing takes it the op
refuses; on a Copy, a Form or a visit to points the key is refused at parse.
Since 2026-11-03 (ROADMAP item 2, slice 2).

**Every descent is said.** Before each straight descent in a Clear, End or
Side block the program carries one comment — `(plunge: …)`, `(ramp: …)` or
`(lead: …)` — with what licensed it there: the air an earlier loop or stroke
cut and how far off, air past the blank or in a given void, a loop too
short to ramp, a lead's length and its room. The sentence reports the plan's
own reading, never a second judgement. A Form's visit to points and a
Copy's passes are the operation itself and say nothing extra. Since
2026-11-02 (ROADMAP item 2, slice 1).

**A ring is an arc where the op says so.** `compress=arcs` writes a circle
the op knows (a bore's or a boss's wall ring) and, since 2026-11-02, a
Clear's loop that chords one — every vertex and chord middle within two
chord tolerances of one circle — as a single full-circle G02/G03; any other
loop keeps its chords under the word. The default is chords, byte for byte.
The schedule writes the word on round walls and on a round blank's face.

**Two stock truths, composed.** The declared truth is the analytical
op-history model (footprint plus floor per removal). The swept truth is the
voxel field, a cache derived from the emitted moves and keyed by an input
hash. `--rest-report` prints where they disagree. No finishing op ever reads
the voxel field.

**The declared model claims a floor only as its tool reaches it** (2026-11-10).
A Clear's or End's footprint is the floor clipped to the family's allowed
region grown back out by the tool's radius: an inside corner's fillet, a nook
and a web thinner than the tool are stock, as the voxel field finds them.
Until now the floor was claimed whole, and the air column, the immersion map
and the cuts-nothing check read air there. The claim stays whole where the
family refuses.

**Checks advise, gates refuse, silence is forbidden.** Findings go to stderr;
a `gate` directive makes a family an error. A check that cannot look at a
case says so by name; it never hands back a clean bill.

**Engagement is measured three ways and feeds nothing.** Per pass (the band),
from the moves (the walk), and before any path exists (immersion). None of
them sets a feed or picks a pattern.

## Import

**STEP goes through the Python sidecar.** It writes a versioned `report.json`
that accounts for every face and a `proposed.part` to review. Rust reads the
report; the only place Rust runs Python is `nacs --import`. The engine builds
and tests with the sidecar absent.

**The origin is chosen at import (2026-11-22).** The sidecar moves the model
so the chosen point is 0,0,0 before it reads anything (`--origin x,y,z`),
so the numbers in the file are the numbers on the machine and the grammar
and the G-code are unchanged. Unless told it is the blank's centre and its
top; any point can be typed, and `model` keeps the CAD origin. A project
has no origin statement. One that shifts the program at emit time stays
open for setups this does not cover.

**A second setup has its own zero (2026-11-22).** `given from <project>
up=<axis> at=x,y,z`: the earlier program is turned as `up` says and then
moved by `at`, which is where its zero sits in this program. Unwritten,
`at` is 0,0,0 and every existing project reads as before. The stocks
must still coincide, now after the move, and the refusal names the `at`
that would make them. A flipped import defaults to the turned blank's
centre and new top and writes the word. Each setup is its own project,
so each already has its own `work_offset`.

**One program at several work offsets (2026-11-22).** `work_offset G54
G55 G56` runs the program at each register, one part in each. Each tool
cuts every part before the next tool change: the ops between two tool
changes are written out once per register, with a retract to
`clearance_z` and the register's word between. No subprograms. An `n=`
label stays on the first part's copy. One register emits what it did,
byte for byte. Every check reads one part; `datum` with several
registers is refused, since it places only one.

**Recognition proposes, a person accepts.** Unsupported faces are named with
their `CHECKLIST.md` row. Nothing recognised is machined until it is written
into the project as text.

## The window

**The window is a thin consumer.** It draws view models from `view.rs` and
`outline.rs`, never walks `Part` or the program itself. Anything it shows is
printable from the CLI; a rejected program gets no window. The engine builds
without it (`--no-default-features`) and always will.

**The text file is the source of truth.** Every edit in the window is one
atomic write of `.part` text with one undo step; the window then reparses.

**Themes are Zed-shaped JSON with named colour slots.** Role colours are
checked for contrast and for colour-vision confusability before use.

## Process (reset of 2026-10-28)

**The ceremony is gone.** No DRAFT, forks, blessing, CLOSE or walk for solo
work. Branch, PR, merge. Propose first only for a change to G-code semantics,
the grammar, or a dependency.

**Two test speeds.** `cargo test` is the loop. `./lock.sh` (both feature
configs, shell tests, smoke run) is run before a merge, not after a nudge.

**Prose is plain.** Commit messages are one line and at most five more. No
coined noun without its definition in the sentence.
