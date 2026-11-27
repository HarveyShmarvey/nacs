# Roadmap

One page. What is built, what is next, and the debts we know about.
`CHECKLIST.md` says which surfaces the engine handles today, row by row.

## Built (as of 2026-10-28)

- **The pipeline.** Text in, toolpath computed, G-code out. Every number is
  derived from the project; inches convert once at parse.
- **2.5D.** Facing, pocket clearing with five patterns, wall tracing with
  engine or controller comp, drilling with seats, chamfers and corner rounds,
  islands and keep-outs by containment, open edges read off the blank,
  allowances, rest clearing, a second setup that knows what the first cut.
- **Analytic surfaces.** Waterline and constant-scallop finishing of spheres,
  cones, tori and spline-revolved surfaces, exact bore and boss walls,
  form tools from an SVG profile, a revolved NURBS meridian read exactly.
- **Mesh.** A raster drop-cutter over STL, the one op that samples.
- **Stock.** The declared op-history model and the voxel swept field, composed;
  the entry restriction, the body-foul check, the rest report.
- **Checks.** Containment, engagement (three measures), reach, envelope,
  fixture, the safety lint, the header law. Each advises or gates, and says
  by name when it cannot look.
- **Import.** STEP through the Python sidecar: a report that accounts for every
  face, a proposed `.part` to review, a stock proposal, chamfers and far-mouth
  bevels recognised.
- **The window.** A thin viewer and editor over the same view models the CLI
  prints: outline, program, viewport with layers, forms with grips, patterns
  table, findings, a bottom pane with the house's own verbs, themes, its own
  frame. Driven headless by 48 shell tests.
- **For agents.** `--check=json` and `nacs --mcp`, every verb a CLI flag first.

## Next, in order

1. **STEP is the front door.** Today the project is programmed from 140
   hand-written fixtures and STEP import is a side door with 21 test models.
   Make the ordinary job: open the window, import a STEP, accept a stock
   proposal, run recognition, review each proposed feature (accept, edit,
   reject, each one visible as the text it will write), schedule operations,
   write the program. Measure it on a real part that is not a fixture. The
   fixtures become what they should have been: tests.

   Slice 1 (2026-10-28): every import door also runs the stock proposal;
   the Review lists the stock first with the text it will write, and
   Accept makes `main.part` from it when the folder is fresh. Measured on
   the real part (a ribbed disc of a few hundred faces): the import takes 2
   s, proposes
   the bolt holes and counterbores and the top plane, declines the
   stock (not a prism) and leaves most faces unsupported, most of them the
   ribs' chamfers and drafts.

   Slice 2 (2026-10-29): a model that is not a prism gets the smallest
   blank it fits, a `stock Cylinder` when one round face bounds the plan
   and a `stock Box` otherwise, the comment above the line saying it is
   not the model's own outline and the allowance is still to add. 18 of
   the 21 fixture models declined a stock before; none does now. The real
   part proposes its bounding cylinder (it was made on a slightly larger blank).

   Slice 3 (2026-10-29): edit and reject on a row. The card's statement is
   a text field, and Accept writes the edited text (Accept all too). Reject
   appends a `# rejected F7` line to `main.part`, a comment the parser
   ignores and the Review reads back: the row and its group-mates say
   rejected, Accept all skips them, `--review` prints the same mark beside
   the row, and the card takes the line out again.

   Slice 4 (2026-10-29): the up axis and the solid in the import field,
   kept for the next import; an assembly refused for want of a solid
   keeps its path in the field with the solids listed in the note; the
   Review's head says which solid and which way was up; `--solid` on the
   CLI and `solid=` on the Output line beside `--up`/`up=`.

   Slice 5 (2026-10-29): the recognizer's yield on the real part, the
   ribs. Each rib stood behind four refusals in turn: its top edges are
   beveled 1.0 mm on some edges and 1.5 mm on others; its outline runs
   out to the disc's rim, whose wall reaches the part bottom; its walls
   end on the head's own underside; and a bevel meets the rounded end
   along a curve that is neither line nor arc. Each now proposes with a
   note instead of refusing: no chamfer and the strips left as residue
   naming the bevels; the base at the highest wall foot with the deeper
   wall named and left unclaimed; the overhang named as the other setup's,
   unless a proposed floor below hosts it, which is the engine's own
   undercut rule applied in the recognizer so the proposals parse
   together; the curve chorded on the wall's own circle. The part went from 38 to
   130 faces proposed, all the rib tops as ProfileBoss
   outlines. Next on that part: the rib walls standing between floors
   with no cap, the drafts, the 1 mm bevels as deep as their wall (28
   faces plus the four ribs they refuse).

   Slice 6 (2026-10-30): what the program makes of a head on a neck, seen
   from above. Nothing reaches what is under the head, so the recognizer
   walks down from each head through the faces under its outline to the
   floor it stands over and proposes the head to that floor; the neck,
   the underside, the pad and the pad's foot bevel become residue that
   says so, the bevel's entry adding that the floor's clearing cuts it
   away. A floor whose every neighbour is the blank's rim, a proposed
   feature or such a face proposes as the level, one `Plane` for every
   floor at that height, once nothing stands above it outside a proposed
   feature. The containment judge in the engine reads two chordings of
   one curve, or a bore tangent to a wall, as a touch and not a straddle,
   which the real part's bores at the heads' corners needed (one engine
   golden, `deck_rest_offset`, loses a shore note; its moves are the
   same). On that part
   146 faces propose, and every face a tool from above can reach
   now does: what remains is 136 faces under the heads, 60 uneven top
   bevels to break by hand, 4 bore floors, the rim and the bottom. A
   project of every proposal passes `--check`, and with one Clear on the
   level and a Side on each head it writes the Up program, 2,359 lines
   in 19 s debug, nothing on stderr.

   Slice 7 (2026-10-30): the other setup. Imported from the back (`--up
   z-`), the real part was 324 refusals under a dozen names, every one a
   face lying under the part's own body, and four proposals for
   counterbores that open under a ceiling. The recognizer now casts a ray
   straight up from each face, from up to five points on it nudged off
   into the void; a face the part's own material covers every time is
   `other_setup`, its entry naming the face over it and the `--up` that
   turns the model over, and a proposal whose primary face is covered is
   withdrawn whole. A downward face, a face under a head and a trim wire's
   bottom anchor keep their own words. A hole's far rim is read as the
   chain of edges touching the wall's bottom, so a hole that breaks out
   across a step onto several faces, tilted or vertical ones among them,
   is a through hole when air lies past it on the axis. From the back the
   real part now proposes 20 faces: the bottom, the rim and the through
   holes with their mouth chamfers; 264 faces say they are the other
   setup's, 52 face down, and 12 are the heads' corner arcs that a tool
   could reach up the holes but nothing claims. From the front nothing
   moved. The two programs write: the front drills every hole and
   counterbore first from the blank top and then clears the level and
   traces the heads (2,395 lines), and the back, `given from` the front,
   faces the bottom and chamfers their mouths (10,101 lines, `--check`
   clean), each in 14 s debug with nothing on stderr. One finding for the
   next slice: the sidecar proposes a hole's rim where the model's wall
   face ends (the pad top) although the column above it is open to
   the blank top, so the front program had to restate the rims by hand
   before the drills' bodies cleared the standing material. Fixture
   `step_hole.step`, a stepped plate with
   a hole across the step and a counterbore, read from the back, and
   `rib_disc` read from the back; `pocket_plate`'s lipped cavity walls,
   `mushroom`'s neck and `through_chamfer_back`'s blind bore now say they
   are covered instead of unanchored or capped.

   Slice 8 (2026-10-30): a hole under open air proposes from the top. A
   drilled hole with a plain mouth whose rim lies below the stock top, with
   nothing of the part straight above it, now proposes from the top with
   its extent the whole way down; the check is rays straight up from
   twenty-five points just inside the mouth. A hole with a mouth chamfer or
   round keeps its rim, which is where that mouth is, and a milled bore
   keeps its rim, where its wall finish starts. On the real part the holes
   the front program had restated by hand now propose from the model's top
   as
   written, the counterbores stay at their pad tops, and the
   programs compose straight from the proposals: the front drills the
   holes through the blank first, clears the level, traces the heads and
   finishes the counterbores (3,449 lines, 14 s debug; the counterbore
   finish warns four times that the ⌀10 endmill's body grazes the heads'
   corner arcs, which continue the counterbore walls upward); the back is
   unchanged (10,101 lines, `--check` clean). Fixture `step_hole` read
   from the front, whose hole under a counterbore is the real part's case
   in small; `rib_disc`'s through hole on the stepped base moves from
   z=20 to the top. Nothing else moved.

   Slice 9 (2026-11-01): a loop point may restate the chamfer's leg. The
   real part's heads carry a 1.5 bevel on their straight edges and a
   1.0 bevel on the concave arc where a bolt hole notches the corner, and
   one feature carried one chamfer, so the heads proposed without one and
   60 strips stayed refused. Now a `ProfilePocket` or `ProfileBoss` point
   line may carry `chamfer_depth=` or `chamfer_width=` after its
   coordinates, in force from that point's outgoing edge until a later
   point restates it, 0 for edges left unbroken; the feature's own keys
   hold from the first point and the angle is the loop's, since one tool
   cuts one angle. The pass runs the loop in stretches of one leg, moving
   the tip at the point where the leg changes and lifting over unbroken
   stretches; a deeper stretch reaches about a reach's width round the
   corner into a shallower one, and the program's comment says so. The
   removal model claims one band per stretch, each at its own depth, so
   the checks after a bevel keep running (without that the voxel probe
   rebuilt the whole swept field for every bevel, seven seconds a head).
   The sidecar writes the marks: the feature carries the leg that runs the
   longest way round the loop and a point restates it where it changes,
   0 where a wall rises to the cap unbeveled; only strips at two angles
   stay `chamfer_uneven`. On the real part the 60 strips are members of
   the heads (206 faces propose; 142 remain, 136 of them
   under a head), and the programs compose straight from the proposals:
   the front, with a bevel pass on every head (4,023 lines, 16 s debug),
   warns where a pass comes back down onto a deeper stretch after an
   unbroken one, since the plunge there meets the corner the pass itself
   cuts; the back is unchanged (10,101 lines, 13 s, `--check` clean).
   Fixture `chamfer_marks.cam` with a golden; `uneven_bevels` proposes
   with marks. Debt: a lead per stretch would spare those plunges.

   Slice 10 (2026-11-01): the operations scheduled. After the review a
   project held its stock, its tools and the accepted features, and the
   run lines were typed by hand, forty of them on the real part. `nacs
   --schedule <project>` now writes a run line for every declared feature
   no run names yet, in shop order, each tool picked from the project's
   own tool lines by a stated rule: a hole is drilled by the Drill of its
   diameter or milled by the largest endmill that fits, with a Clear
   before the wall pass unless that pass sweeps the bore out by itself;
   floors, pockets and bosses take the largest Endmill, a boss standing
   on a cleared floor only its wall pass; a chamfer the ChamferMill at
   the loop's angle; stepover and stepdown half the tool. A Plane lying
   at the blank's top, a given feature and a feature no declared tool
   fits get a note instead of a line, and every line is parsed with the
   project before it is offered, so a refused one lands as a note with
   the parser's sentence and what is written always parses. The window's
   Review has the same act, *Schedule operations*, one write, undoable;
   the MCP serves it as `schedule`. On the real part the front schedule is
   the forty lines the hand program had, differing only in the wall passes'
   comp (the engine's default where the hand wrote control), and the
   program composes from it in 11 s with the same advisories; the
   back schedules the mouth chamfers and nothing else, since the
   holes are given and the bottom face lies at the proposed blank's top
   (682 lines, `--check` clean, where the hand program had faced the
   bottom with a blank that had no allowance to face). Tests in
   `tests/schedule_tests.rs`; no golden moved.

   Slice 11 (2026-11-02): the blank carries an allowance. Every import
   door takes a third word beside `up=` and `solid=`: `allowance=`, 1 mm
   unless typed (`nacs --import --allowance <mm>`, `allowance=` on the
   Output line, a field under the import path), and the sidecar's own
   default stays 0 so its goldens stand. A bounding blank grows by it on
   the sides and the top (the real part proposes the blank it was made on);
   a prism, its own blank, is left alone and says so. The
   recognizer reads the model unchanged and places what the blank changes:
   a plain drilled hole at the model's top is drilled from the blank top
   (the blank's whole height on the real part's), a bore with a mouth chamfer or
   round and a milled bore are placed at their rim by `z=`, and the
   cylindrical face that bounds the plan — the blank's rim — proposes, from
   the stretch that runs the whole way round up from the part bottom, as
   `F1_Rim CylinderBoss(dia=300, extent=34, z=0)`, the boss whose wall pass
   brings the blank to size; the same stretch seen from the other side does
   not stand on the part bottom and is refused by name. The schedule faces
   first: the highest Plane under the blank's top that nothing stands above
   leads, Planes at one height share its one Clear (a Clear on a Plane
   works the whole plan, and the real part's top had one per rib, each a
   pass over the same disc, 178,000 lines), a round boss whose
   wall is the blank's own rim gets a note, and one within a tool's width
   of the rim is swept by its wall pass alone. On the real part the front
   is 42 lines (the facing, the drills, the floor, the bores, the heads, the
   rim, the chamfers), 17,107 lines of program in 31 s — the 1 mm face over
   the disc is 9,700 of them as chorded rings, the rim's seven rings
   2,000 — with the same advisories and `--check` exit 0. The back
   is the mouth chamfers again (682 lines, check clean): the engine's
   law for the second setup is the same blank turned over, so its stock is
   the same cylinder with its top at z=0, the face the up setup did not touch,
   and the whole-height boss the recognizer makes of the rim from that
   side is rejected as before. Debts: the second setup's stock proposal
   should come from the `given from` project, not the STEP (the parser's
   sentence names the right line today); a Box blank's sides are cut only
   when the silhouette proposes as a ProfileBoss; the facing's rings are
   chords where arcs would do.

   Slice 12 (2026-11-02): the second setup from the given project. Every
   import door takes a fourth word, `given=<project>`, the earlier
   program's project as the `given from` line will name it from the new
   folder (`nacs --import --given`, `given=` on the Output line, a field
   under the import path). The engine loads that project through the one
   `load_project` home, turns its stock as `up=` says with the validator's
   own `turn_stock` (z+ or z-; a quarter turn is refused by the grammar's
   own sentence, a Profile blank by name), and hands the words to the
   sidecar as `--blank`; the sidecar proposes that stock with the
   `given from … up=…` line beside it, measures the blank's margins
   instead of taking an allowance (the model outside the blank on any side
   is an error naming the side), places the bores by the margin above the
   top, and withdraws the plan-bounding rim by name (`rim_given`): the
   earlier setup's to bring to size, standing as it left it. Accepting the
   proposal into a fresh folder writes both lines, parsed where they will
   live. The schedule reads `given from` too: a void the earlier program's
   air already holds whole — the validator's own certificate,
   `certify_given` — is written `given <name>` instead of cut again, before
   the runs, and only its mouth chamfer or round gets a line; a void cut
   short (the web) keeps its own run. On the real part the back import
   takes 5 s and proposes 19 faces (18 bores, the bottom); the rim boss the
   old rule made of F1 is gone, refused as the front's; the stock proposal
   is a `Cylinder` at z=0 with `given from ../real_part_up.cam
   up=z-` under it; the schedule writes the `given` lines and the mouth
   chamfers; the program is 682 lines, nothing on stderr, `--check` exit 0
   — slice 11's hand-composed back program, with nothing typed but the
   tools. Debt: a Profile blank turned over wants its outline mirrored.

   Slice 13 (2026-11-02): the rings as arcs. The per-op word `compress=arcs`
   already wrote a bore's or a round boss's wall rings as one G02/G03 each;
   a Clear's rings were chords whatever the word said. Now a Clear under the
   word traces a loop that chords a circle — every vertex and every chord's
   middle within two chord tolerances of the least-squares circle through
   its vertices (`ops::clear::loop_circle`) — as one full-circle block, and
   a loop that is no circle keeps its chords, so the word is never wrong; a
   ramp's laps stay chords. The schedule writes the word on a bore's wall
   and floor, a round boss's wall and the face of a round blank (a Clear on
   a Plane works the blank's whole plan). The pattern report read an arc as
   the straight move between its ends, which for a full circle is nothing
   cut; it reads an arc as its chords now (`gcode::arc_chords`, the one
   chording the toolpath view shares), so the as-emitted walk of a facing
   written as arcs is the same walk. On the real part the front's 42 run
   lines are the same lines with the word on seven of them; the program is
   4,083 lines where it was 17,107 — the 1 mm face 141 where it was 9,696,
   the rim's seven rings 37 where they were 1,969, each bore wall 22 where
   it was 270 — with the same advisories, `--check` exit 0, and the
   report's row for the facing unchanged (0.0 mm² uncut, 15,937 mm at
   depth). No golden moved: the default stays chords byte for byte. Debt:
   the move lint judges entries and traverses, so it never read a ring, as
   an arc or as chords; the floor between the ribs has one round ring in
   twenty-six (its outer one) and the rest stay chords, rightly.

   Item 1 is done: the window opens, a STEP imports, a stock proposal is
   accepted, recognition runs, each proposed feature is reviewed as the text
   it will write, the operations are scheduled and the program written, and
   the real part's front and back are made with nothing typed but the tool
   lines. The fixtures-become-tests clause is housekeeping and waits.

2. **The human owns the program.** Every choice the engine makes on the
   person's behalf (pattern, entry, direction, lift, plunge) must be visible
   in the program as a plain sentence and overridable as a key in the `.part`
   text. Nothing automatic that cannot be seen and undone. The empty Problems
   pane is not a certificate; the window should say what was checked, not
   only what was found.

   The phase, seven slices, agreed whole on 2026-11-02. (1) Every descent in
   a Clear, End or Side block is said before it is made. (2) The direction
   is said and `direction=climb|conventional` overrides it: the engine has
   always climbed (G41 under M03, never G42) and never said so. (3) `angle=`
   pins the row direction the comb would choose. (4) `entry=ramp|plunge|lead`
   makes every descent in an op that kind, an error where it cannot be. (5)
   The lead's shape: `lead=` (mm, the sphere's radius about the start
   point), `lead_angle=` (0 tangent from behind the cut, 90 perpendicular
   from the air side, today's default, oblique between), `lead_descent=`
   (degrees below horizontal, 0 flat); the engine places the start point so
   the whole lead lies in air, and refuses where none does; a lead's descent
   is never called a ramp, since a ramp cuts and a lead flies. (6) `--check`
   prints an account of what each family judged, declined or could not
   apply to, on stdout, which is empty today; `--check=json` carries it. (7)
   The Problems pane shows that account. Not planned: a `start=` key, changes
   to the schedule, sentences on a Form's or a Copy's descents (there the
   descent is the operation).

   Slice 1 (2026-11-02): every descent said. One comment line stands before
   each straight descent at a stated feed in a Clear, End or Side block —
   `(plunge: …)`, `(ramp: …)` or `(lead: …)` with what licensed it there: the
   air an earlier loop or stroke cut and how far off it, air past the blank
   or in a given void, a loop or stroke too short to ramp, a lead's length and
   its room. The sentence reports the plan's own reading (a row chain now
   carries whether its start stood in air by the planner's predicate) and
   never judges again. The sentences that lived in op headers moved to the
   descent they explain; a ramp that needed one lap, a lead with a whole
   diameter of air and every plunge beside cut air, all silent before, are
   said. No motion moved: every one of the 115 fixtures' programs is byte for
   byte what it was with the comments stripped, so the 113 goldens moved by
   comment lines alone (`tests/descent_tests.rs` walks every fixture and
   requires a sentence within four lines above each descent, one per
   descent). On the real part's front, 158 descents have 149 sentences; the
   nine unsaid are the drills' visits to holes, which are the operation.

   Slice 2 (2026-11-03): the direction said and set. Every Clear, End and
   Side block carries one `(direction: …)` line, after its notes on the
   region and before its first move. A pass with the material
   on one side — a loop, a ring, a wall trace — says `climb — the material on
   the tool's right under M03` (what every program always did and never
   said) or `conventional — the material on the tool's left`. A block of rows
   says the rows run both ways by turns, since spine's strokes and the zigzag
   take whichever way is nearest, and names the passes along the walls
   separately; the one-way rows say which side the uncut floor stands on.
   `direction=climb|conventional` on a Clear, End or Side turns every pass
   that has a side: each loop the other way round from the same start (a
   bulge path moves its arcs to the segments' other ends, since `.reverse()`
   alone would bow them the wrong way), each ring CW for CCW, and G42 for
   G41 under control comp. Said where nothing takes it — a raster, a face
   with alternating rows and no wall — the op refuses and names the patterns
   that do; on a Copy, a Form or a visit to points it is refused at parse.
   Unsaid, nothing moves: 115 fixtures byte-identical with the comments
   stripped, 105 goldens by one line per block (and the nine wall-chain lead
   lines reworded). `plate_conventional.cam` is the plate cut the other way;
   `tests/direction_tests.rs` turns a bore ring, a boss ring and the one-way
   rows and reads the side off the motion. The
   lead sentence on an open wall chain now says `along the path's own line`,
   which it is; slice 1 had called it the normal.

   Slice 3 (2026-11-03): the rows' direction set. `angle=` on a Clear or an
   End cutting rows — spine, zigzag, oneway or held — pins the direction, in
   degrees from +X, 0 up to but not including 180, since rows have no sense.
   Spine's comb then chooses only its phase and its way, and tries no pair
   of combs, since a cross row runs at right angles to the direction set;
   the parallel rows plan at that one angle. The pattern sentence says `the
   angle set` after the angle, so a reader of the program alone can tell the
   comb's choice from the author's; pinned to the angle the comb chose, the
   program moves by those three words and nothing else. Refused at parse on
   a Side, a Copy, a Form or a visit to points, and beside the offset family
   or the raster, which cut no rows that turn. `plate_angled.cam` is the
   plate with its face rowed along Y and its notch zigzagged at 45 degrees;
   `tests/angle_tests.rs` reads the rows' direction off the motion.

   Slice 4 (2026-11-03): every descent may be made one kind. `entry=ramp`,
   `entry=plunge` or `entry=lead` on a Clear, an End or a Side makes each
   descent in the op that kind, and the op refuses where one cannot be. A
   ramp: every row and every shore pass ramps down its first stroke by the
   law the plan's own ramps follow, every loop of the offset family laps
   down its own profile; a stroke or a loop under a diameter has no room
   and refuses. A plunge: a row or a loop the plan would ramp plunges at its
   start, into standing stock, said so; a wall trace drops its lead and
   plunges on the path. A lead: kept where the plan makes one — a wall
   trace, a ring, a loop with air in reach past the blank — and refused
   where it does not. The sentence before a descent the key changed ends
   `as entry=… asks`; one the plan would have made the same is worded as
   before. A ring's descent at its lead point in the air its clear left is
   now called a lead, which it is; slice 1 had called it a plunge, and the
   ring goldens move by that word. Not built, and refused by name: a ramp
   down a wall or round a ring, a plunge at a ring's wall, a lead into a row
   or into a loop whose only air is what an earlier loop cut (slice 5 reads
   that air when it shapes the lead), anything but a plunge on the raster.
   Refused at parse on a Copy, a Form or a visit to points, whose descent is
   the operation. `pocket_post_plunged.cam` is the post pocket with its first
   stroke plunged into standing stock where the plan would ramp it;
   `tests/entry_tests.rs` reads each descent off the motion.

   Slice 5 (2026-11-03): the lead's shape. `lead=` says how far from the
   start the descent is made (mm; a diameter unsaid), `lead_angle=` which
   way from the start it lies (0 behind the start on the pass's own line, 90
   off to the air side, the unsaid default, oblique between) and
   `lead_descent=` how steeply the flight comes down (degrees below flat; 0
   unsaid). Said, the lead is whole and in air or the op refuses: a closed
   wall trace turns its start round the path to the first vertex whose lead
   lies in air by the stock model, the control-comp edge following the
   parallel; a ring's lead is read off its start on the +X ray, the air
   toward the axis for a bore and outward for a boss; the offset family's
   side entry turns its start round the loop until the whole lead lies in
   air past the blank, in a given void, or on the path of a loop cut before
   it, and under `entry=lead` every loop is entered so. The sentence names
   the length, the line and the slope. The flight onto the start carries the
   cut's Z where it descends, so an unsaid program keeps its bytes; no golden
   moves. Unsaid, a wall trace's lead is still shortened to the room there
   is, as before. Not built, and refused by name: an oblique lead on an open
   wall, which has no air side; a sloping lead onto a chamfer whose legs are
   restated; any lead key on rows or the raster, which make none. The lead
   keys beside `entry=plunge` or `entry=ramp` are refused at parse.
   `pocket_wall_lead.cam` is the cavity's wall led into four
   millimetres off, at 45 degrees, descending 10; `tests/lead_shape_tests.rs`
   reads each lead off the motion.

   Slice 6 (2026-11-03): the check's account. `--check` prints on stdout,
   which it left empty, one line per finding family after a summary: a
   family that ran says what it found and how many operations it could not
   judge; one with nothing to judge says why, read off the declaration the
   family itself reads — no machine declared, no keep-out, no tool with a
   stickout, no Side running a chamfer tool or a corner rounder, the program
   refused before any move. A decline now says which family's check it stood
   in for (`Finding::about`), so it is counted against that family. The
   account lives beside the check result in `check_result.rs`, the one home
   the CLI, the MCP verb and the window read; `--check=json` carries it as
   `checked`, one object per family. Stderr and the exit codes are as they
   were; the prose mode now prints its post-generation findings from the one
   pipeline instead of a second list of calls. The swept field's reading of
   the part is not in the check pipeline and the account says so rather than
   counting it as run. `tests/check_account_tests.rs`.

   Slice 7 (2026-11-03): the Problems pane shows the account. Its own line
   reads `no problems · 6 checks ran, 0 could not`, the "could not" the
   operations some check declined to judge; under the rows, and under "No
   problems found", the pane lists the lines `--check` prints. Derived once
   beside the rows (`view::problems_line`, `check_result::account`); the
   window only paints it. `tests/view_tests.rs`.

   Item 2 is done: every choice the engine makes on the person's behalf is
   a sentence in the program — the descent, the direction, the pattern's
   angle, the lead's shape — and a key on the run line overrides each
   (`direction=`, `angle=`, `entry=`, `lead=`, `lead_angle=`,
   `lead_descent=`), refused by name where it cannot be kept; and the check
   gives an account of what it judged, on stdout, in JSON and in the pane.
   Left for later, each named where it was met: a ramp down a wall or round a
   ring, a plunge at a ring's wall, a lead into a row, a sloping lead onto a
   chamfer whose legs are restated, an oblique lead on an open wall, the
   lead's arc (`lead_sweep=`), and the swept field's reading of the part in
   the check pipeline.

3. **The usability pass.** The findings already in hand: the creation form
   that sticks, the grip's 0.001 floor, the crawl under a fine stepover,
   nothing drawn on a path to tell five patterns apart, the agent's own seat
   (pixel-level shell tests so the agent can see what it ships).

   The phase, seven slices, agreed whole on 2026-11-07. (0) The agent's
   seat: a shell test renders the real window to a PNG the agent reads, and
   every later slice ships a picture. (1) The creation form closes after
   Create; the new feature is selected with its card editable; the follow-on
   op is a button on the card. (2) The stepover grip's floor is a fraction of
   the tool's diameter, derived in one home, said on the card when a drag
   hits it; a generation that outlasts a frame says it is computing. (3) The
   crawl under a fine stepover: measure which layer costs the frame, cap the
   engagement cells by coarsening, build the path mesh once per selection.
   (4) The selected path coloured by its engagement (past the ceiling, the
   slot, each entry), the same stretches printed by `--patterns`. (5) The op
   cursor debt: verify what Phase 61 built, finish what remains, strike it.
   (6) Small sayings: a Clear whose floor is already air says so; one-line
   findings from the walks. Not planned: new UI crates beyond the harness's
   own feature, a redesign of the panes, the capture box.

   Slice 0 (2026-11-07): the agent's seat. `egui_kittest`'s `wgpu` feature
   under `shell-tests` renders the frames the shell tests drive, on a CPU
   adapter where there is one, and `image` writes them as PNGs under
   `target/shots/`, never in the tree. `tests/shell_tests.rs` opens the
   plate, writes `plate-opened.png`, selects the Clear and writes
   `plate-clear-selected.png`, and asserts the frame is the window's size,
   many colours, and changed by the selection. The live window is untouched.

   Slice 1 (2026-11-07): the creation form closes on Create. What the write
   made is selected once its reload arrives — a feature by name, an
   operation by the index it landed at, a tool by number — and its card,
   with its fields, is the next thing seen; a feature's card already offers
   *New operation…* and the skeleton menu. Until now a feature with another
   skeleton reopened the form on the next one (a Plane: Clear, then End),
   *Clear around it…* reopened it on the Plane, and only Esc closed it, so
   a feature made in the window could not be edited. `tests/shell_tests.rs`
   makes a Plane from *New feature…* and from *Clear around it…* and an
   operation after the cursor, and finds each selected with the form gone;
   `target/shots/feature-created-selected.png` is the card.

   Slice 2 (2026-11-07): the grip's floor, and the frame it froze. The
   stepover grip stops at a twentieth of the tool's diameter
   (`view::stepover_floor`, the one home; it was 0.001 mm) and the status
   line says so the way it says the ceiling; a finer value may still be
   typed. Measured on plate_floated's Clear under a ⌀10, debug build: 0.5 mm
   plans in 2 s, 0.25 in 9 s, 0.1 in 66 s, and until now every second of
   that was spent on the frame, the window read as crashed. The reload —
   the load and the check, which is the program's whole planning — now
   runs on a worker; the status line says `Reloading — planning the
   program…` and the Output pane logs it; a file that changes while one
   runs is reloaded by the poll after. `tests/view_tests.rs` pins the floor
   and its sentence; the shell tests, which write and wait for the reload
   in nine places, drive the worker.

   Slice 3 (2026-11-07): the crawl, measured and cured. Headless, debug
   build, the plate's face selected under a ⌀2: a frame with nothing
   drawn is 8 ms, with the toolpaths 9 ms, with the Engagement layer
   101 ms — the path's stroke was never the cost; the layer painted a
   polygon per cell, 153,600 of them, every frame. The field is now one
   mesh, drawn within `view::IMMERSION_DRAW_BUDGET` (20,000 squares) by
   taking `step` × `step` cells a square, the worst band of each
   (`view::immersion_blocks`, built once per field), and the layer's line
   says `drawn 3×3 cells a square, the worst of each` when it is coarser
   than read; what `--engagement` reads is untouched. After: 18 ms. The
   frame that selects the op still builds the field on the frame (254 ms
   here); left. `tests/view_tests.rs` pins the step, the budget and the
   worst-of-each rule; a shell test reads the words off the status line
   (`target/shots/engagement-fine-plate.png`); the measurement runs under
   `NACS_SHELL_BENCH=1` and prints.

   Slice 4 (2026-11-07): the walk drawn on the path. The selected
   operation's path as emitted is read at the walk's own stations, one a
   millimetre, into stretches of one kind — air, cutting, past half
   immersion, a slot — merged along a move and across moves that meet
   (`compare::walk_as_emitted`, `compare::Stretch`); the stretches past
   90° and the slots are drawn over the path in the engagement family's
   colour at two brightnesses, and a dot marks each entry, where a run of
   descending feed moves arrives. The card says, under the removal line,
   `as cut: 81.0 mm past 90° in 6 stretches, 55.0 mm slot in 1, 1 entry —
   drawn on the path` for the plate's Clear (its first stroke along the
   notch's wall is the slot), the stretches and entries on hover, measured
   on a worker; `--patterns`
   lists the same lines under each op's table (`compare::walk_lines`, one
   home). The stretches sum to the load the report already read.
   `tests/pattern_tests.rs` pins the sums, the merge and the listing; a
   shell test reads the card and shoots `target/shots/walk-on-path.png`.

   Slice 5 (2026-11-07): the op cursor debt, struck. Intake note #611
   (2026-10-25) asked that a new operation land below the selected
   operation instead of at the end of the file. Phase 61 slice 3 built it
   and this slice drove it: with the cursor after the plate's End,
   *Create operation* on the Notch writes the Clear as the second run
   line, the cursor moves onto it, and since slice 1 it is selected with
   its card open (`target/shots/new-op-under-cursor.png`). The note's
   other half, placing an operation before it is written, is not built:
   the cursor is the place, and moving it is one click on a line.

   Slice 6 (2026-11-07): small sayings. An operation whose moves at depth
   all run in air by the stock model before it — the floor an earlier
   operation already cut, or stock that never stood there — is said by the
   check, an advisory in the engagement family: `Clear on 'Notch' with T5:
   cuts nothing — its 638mm of moves at depth all run in air…`
   (`engagement::slot_findings`, asked at one station a millimetre of the
   tool's centre and eight points of its circle; not asked of a
   field-selected work-list, `strategy=regions` or a rest floor, where the
   declared model's over-claim at inside corners would say air over the
   very corners it cuts; `tests/pattern_tests.rs`). Over the corpus it
   spoke on two: `floor_travel`'s face stood at the blank's top and skimmed
   nothing, so the fixture's face is half a millimetre down and its golden
   moves one line (`Z0.000` to `Z-0.500` on the plunge); `pocket_levels`'
   End is a floor finish after a Clear that leaves it no skin, said as it
   is and pinned by name in `tests/check_tests.rs`.
   What the slices' own pictures found: at the opening size the Program
   pane's *Write program* was clipped — the row's label now gives way
   before the controls do, and the gutter toggle reads `numbers`; the
   feature card's third button ran off the sidebar as "Delete featu…" —
   the row wraps. A shell test measures both and shoots
   `target/shots/feature-card-buttons-wrapped.png`. Seen and left, each a
   line: the long comment lines of the program run off the pane (the
   pane scrolls sideways); the status line's immersion sentence ends in an
   ellipsis at the opening width (the Output pane has it whole); the Output
   pane opens with the Engagement layer's "select an operation" note posted
   three times at 0:00.

   Item 3 closed 2026-11-09 on the user's walk, seven items passed. Two
   items were re-pointed on the way: a feature cannot be made from the
   window on `plate.cam`, whose declarations live in `geometry.part` (the
   form's own refusal since Phase 50), so that item walked on
   `cylinder_boss.cam`; and the walk on the path needs nothing set, since
   the Clear on the Notch already runs past 90°.

4. **Code debridement.** Done (2026-10-28): the eleven feature kinds no
   recognizer could propose went, with their emitters, 29 fixtures and 39
   goldens; `parse.rs` is now `src/parse/`, eleven files by topic, and its
   two 1,400-line functions are a dispatch each (`parse_into` keeps the
   multi-line reading of contour and face points, 700 lines, and wants a
   struct for the open reading before it shrinks further). The cutter-holder
   optimizer (`assembly.rs`) and the theme checker (`theme.rs`) stay as they
   are, ruled 2026-10-28: the first backs the body-foul check, the second is
   a feature in use.

5. **The quiet pass.** An audit of the window on 2026-11-12 found it saying
   the same thing in several places and its own bookkeeping in the user's
   way. Six slices, one PR each.

   Slice 1 (2026-11-12): the Output log is one line an event. Each source
   (a write, a refusal, the reload, the theme, a run) posts its own sentence
   once; the joined status line used to be posted whole, so a standing note
   was logged again with every change beside it (ten lines for two events
   on `spline_funnel_holder`). A background job is one line with how long
   it took. A refusal is drawn in the refusal colour, stands eight seconds
   on the status line and stays in the log. A layer's readout or failure
   stands under its row in the Layers list, not on the status line. What
   the launch printed to stderr (theme, surface, blur, frame) is in the log
   too. *Copy* and *Save* on the dock write the log as text, `<name>.log`
   beside the project. `3` opens the dock without taking the keyboard.

   Slice 2 (2026-11-12): one name for an operation. The Outline and the
   CLI flags name an operation `[0]`; errors, findings, report titles and
   the window's notes named the same one "op 1". They now say `op [0]`.
   A count ("3 of 3 run") is still a count. Left as they were, because
   they are G-code text and the goldens pin them: the two rest-floor
   comments, `the rest: 1 floor op 1 left standing` and `(the rest: op 2
   takes it with T6)`.

   Slice 3 (2026-11-12): one home for the cursor. The transport bar says
   it, once: `Shown through [2] Side on 'Notch' with T5 — 3 of 3 run`.
   "Shown through", because a selected operation's card may be open on a
   different one, and the old "After operation 3 of 3: Side" beside a
   Clear's card read as a disagreement. The sidebar's line and the
   Program's row no longer repeat it (the Program's copy was clipped to
   "97 l…" at the opening size). The move row says `[2]: 13 moves`; how to
   name a move is the slider's hover.

   Slice 4 (2026-11-12): the Problems pane says a thing once, in its own
   words. "Nothing found" is the band's line and no longer two more lines
   in the pane; the family-by-family account is folded under *What was
   checked* and names each family as the rows do ("Holder collision", not
   `body_foul`); so does a finding's mark in the viewport. A row carries
   the gist of its sentence, the card the whole. A row's head is plain,
   red only for a rejection and weak for a check that could not run; the
   family's colour is a swatch that matches the mark. Two findings lost a
   note to self: the body foul's pointer to a ROADMAP heading, and the
   entry advisory's "`entry gate` rejects on this instead", which now says
   an `entry gate` line in the header refuses the program on it.

   Slice 5 (2026-11-12): a card opens on the keys its line carries. The
   plate's Clear shows `tool`, `profile`, `stepover` and `pattern`; the
   other 27 keys a run takes are under *More keys (27)*, where each can
   still be set. Nothing is tagged "optional" or "the only one declared".
   What the op removes and how it cuts are read at body size, and with the
   card this short the patterns table is in view without a scroll. The
   form does not judge which keys a verb reads: that is the parser's.

   Slice 6 (2026-11-12): short labels. The palette lists an act by the
   label its menu row wears, the key in a column beside it, the sentence
   on hover; it listed whole sentences that wrapped to three lines.
   Typing matches the label too. The hovers on the buttons that write say
   what the button does and that `z` undoes it, without the account of
   the parser and the reload. The help opens wide enough for its columns.

   Seen and left: the Program row's `97 lines` is still clipped at the
   opening size by its four controls; an operation still stands twice in
   the Outline, under its feature and in the run order.

6. **Both platforms (2026-11-14).** Windows and Linux write the same program
   for every fixture, and CI checks it on every push: the lock's three legs on
   each platform and an `agree` job that compares every fixture's output. The
   engine's math goes through `libm`, printed numbers are snapped before they
   are cut to their decimals, and Stop kills a run's whole process group on
   Unix. The close calls inside the engine (which vertex starts a loop,
   whether a link takes the arc) were decided by a last bit; since 2026-11-20
   they follow one written rule, `src/tie.rs`: candidates within 1e-6 mm are
   tied, the first in a stated order wins, and a loop starts at the tied point
   nearest the tool.

7. **The agent's seat (2026-11-15).** A session on any platform starts ready
   to work: `setup.sh` prepares a Linux box (a cloud session runs it at
   start), `.claude/settings.json` lets cargo, git and the PR commands run
   without asking and refuses a merge, and `shots.sh` puts a window PR's
   before and after pictures in the PR itself.

8. **The job has an order (2026-11-21).** A new user could not tell what to
   do first: the acts existed but nothing said their order. The sidebar now
   carries a job guide until the operations are scheduled: Model, Origin, Stock,
   Features, Tools, Operations, Program, the next step with one sentence
   and its button. `nacs --job` prints the same.
   Still missing, in the order a job meets them: no act moves the part
   inside the stock (one allowance, all sides); Tools has no starter
   cutter; the Review lists every face, not the features.
   Second pass, from the user's first run on a part of their own
   (2026-11-21): the Stock step accepts the proposed blank in one click;
   Accept all waits for the stock; the Tools step stays open until every
   feature has a cutter and names the ones missing; scheduling puts the
   Outline in front; the guide stays through Write program and says when
   the file is written.
   Third pass (2026-11-21): a form with more than four optional answers
   puts Create under the required ones, so the tool form's button is
   never under the fold. A chamfer or edge round whose tool is not
   declared is still asked for after its feature is cut: the Tools step
   names the tool and keeps its button, and once the tool is declared
   Operations opens again for the one line.
   Fourth pass (2026-11-22): `nacs --view` with nothing to reopen opens
   a window with no project, the guide at step 1; importing a STEP there
   starts `<model>.cam` beside the model. What an import proposes is
   drawn before any accept: the proposed blank, and the waiting features
   in dim lines. The empty viewport's sentence wraps.
   Fifth pass (2026-11-22): the origin is chosen at import. The form
   beside the path offers X0 Y0 (the stock's center, a corner, as
   modeled) and Z0 (the stock's top, the part's top, the bottom, as
   modeled), and takes any point as typed words; unless told it is the
   stock's center and top. The guide has an Origin step that says it
   back and, until something is accepted, a button to change it. The
   viewport marks 0,0,0. `nacs --import --origin` is the same.
   Sixth pass (2026-11-22): the part is placed in the stock. Under the
   origin the form has a field per side of the blank (top, bottom, x-,
   x+, y-, y+); an empty one takes the allowance, the bottom 0. It is
   one spelling, `--allowance 1,bottom=3,x+=10`, and the origin's words
   read the blank it gives. The guide's Origin step names the register
   the program selects (G54 unless `work_offset` says).
   Seventh pass (2026-11-22): a flipped setup has its own zero. An
   import with `given=` takes the origin choice too, read on the turned
   blank (its centre and new top unless told), and writes `given from
   <project> up=z- at=x,y,z`; the engine moves the first program's cuts
   by `at` after the turn. Before this a flip of a project imported at
   the default origin was refused as outside its blank.
   Eighth pass (2026-11-22): one program at several work offsets.
   `work_offset G54 G55 G56` cuts a part in each register, every tool
   finishing all of them before the next tool change. The Origin step
   and the cycle time count the parts.
   Ninth pass (2026-11-23): the register is chosen in the window. The
   stock's card has a `work_offset` field at the top of the header (G54
   until a line says otherwise; several registers run a part in each;
   emptied, the line is removed). File > Work offset… opens it, and the
   guide's Program step names the register before the program is written.
   Eleventh pass (2026-11-23): New tool… opens on the tool the job
   waits for, its kind chosen and the one size the scheduler names
   filled (a ChamferMill at 90, a Drill of 3), not on an empty Endmill.
   Twelfth pass (2026-11-23): the Review lists features, not faces. A
   proposal is one row, its name and kind ("F7_Bore  CylinderBore · 2
   faces"); a face that is only a member of one has no row. The faces
   nothing was proposed for sit under a shut fold for each reason.
   Thirteenth pass (2026-11-23): the job guide is an option (File >
   Options…): shown, folded to its one line, or hidden. Every act it
   offers is in the menu, so a user who knows the order loses nothing.
   Fourteenth pass (2026-11-24): a setup sheet. A finished guide shows
   what to take to the machine: the program file, the blank, the origin
   and its register, the tools an operation runs, the run time. File >
   Copy setup sheet copies it; `nacs --sheet` prints it; Write program
   writes it beside the `.nc` as `<name>.setup.txt`.
   Fifteenth pass (2026-11-24): the Review counts features too. The tab
   says how many proposed features wait ("Review (4)"), not faces, and
   a feature's row is no longer marked "same as" one of its own faces.
   Sixteenth pass (2026-11-24): the import form is a form. It sits in
   the form's dock, a labelled row a question: the file, which way is
   up, the origin, the allowance, the stock beyond the part; the solid,
   the given program and the origin's words under a More fold that
   opens when an import is refused. Browse… fills the path and no
   longer imports at once.
   Seventeenth pass (2026-11-24): the chamfer mill's form explains
   itself. Each measured key says what it is; gauge_z is calculated
   from the angle, gauge_dia and tip_dia until a measured value is
   typed. A tool whose cone starts far up a straight end (gauge_z read
   as the tool's length) is noted on its card, at the head of the
   Problems, in `--outline` and as a warning on stderr.
   Not built: moving the origin after features are accepted (import
   again); a margin on a blank that is the model's own outline (a plain
   prism); a third setup; a quarter turn between setups; a front and its flipped back in one program
   (fronts on G54, backs on G55); a different part in each register.
   Idea, not built: orientation advice at import. Recognize from all six
   directions, then rank them: the one that reaches the most features,
   and the fewest directions that reach all. It cannot know workholding.
   Tenth pass (2026-11-23): an options page, File > Options…. First on
   it, the camera: the orbit style (free turntable, where the tilt goes
   on over the top, or a turntable that stops straight down and straight
   up) and what a drag turns the view about (the part's center, the
   viewport's middle, or the point under the cursor). The first of each
   is the default. A pick is saved in the workspace file at once;
   `nacs --options` prints the same page. Not built: a trackball orbit
   (the camera holds no roll).

9. **A second part of the user's own (2026-11-26).** A block from FreeCAD
   with a groove round three sides, a bevel, rounds and a pocket on each
   face. First pass: a hole the groove crosses proposed twice, once for
   each stretch of wall, and was drilled twice; it proposes once now, from
   either side.
   Second pass: the top pocket proposes. Its floor fillet is read through
   to the walls (no floor radius in the grammar: a flat endmill cuts the
   corner square, and the note says so), its rounded mouth is `edge_round`,
   and the side that runs out onto the bevel is open, drawn out past the
   blank so the clearing enters there in air.
   Third pass: the boss in that pocket stands taller than the block, which
   the engine refused; it reads it now as the island of the pocket and of
   the level above. The block's top, below the boss's, proposes as that
   level: a `Plane` the whole blank clears to (`block_level` fixture).
   Still refused on that part, in the order to take them: the outline of
   the block, the bevel itself, the boss's top chamfer, a counterbore's
   floor over a through hole. The groove and a slot in a
   side face wait on a quarter turn. Not built: an endmill with a corner
   radius, which is what leaves a floor fillet.

## Debts

- **The capture box** on the acrylic window waits on an egui release carrying
  `with_no_redirection_bitmap` (upstream PR #8749).

Paid in November 2026: the unbounded chord (a stretch open to the air says
so and bounds no tool); the re-entry plunge (`entry=ramp` was already the
cure, and the default stays a plunge said by its sentence, ruled 2026-11-10);
the inside-corner over-claim (the declared model claims a floor only as its
tool reaches it, `DECISIONS.md`); the advisory on an earlier program's void
no `given` names, which said `on '' (?)` for a drill visiting points and
spoke of its first hole only, and now names each hole.

## Not planned

Multi-axis. Per-machine posts. Telemetry. A second project format.
