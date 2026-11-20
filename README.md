# nacs

Code-first CAM. A toolpath generator where a project is a folder of plain
text, so it diffs, reviews and edits like code, by a person or by an agent.

This is a solo, for-fun, learn-as-we-go project. It is small on purpose.
Where a commercial CAM package reaches for a 40-page algorithm, nacs
is allowed to ship a 10-line approximation with an honest comment about the
limitation.

## Try it in sixty seconds

```sh
cargo build
./target/debug/nacs --view plate.cam        # the sample: a plate, a face, a notch
```

Click an operation in the Outline; its toolpath lights up and its card opens.
The Program pane is the G-code; **Write program** puts it beside the project
as `plate.nc`. `nacs --view` alone reopens the last project. `nacs --init
'Box w=100 l=60 h=12' job.cam` starts a new one. `GETTING_STARTED.md` walks a
part from nothing; `REFERENCE.md` is every word of the language.

## The idea

1. **Code-first.** No proprietary binary project file, ever. A part is text.
   An AI that can write text can program the machine; a reviewer can read
   exactly what changed.

2. **"It's a wall now."** Imported CAD is a close-enough canvas. You declare
   perfect analytical primitives over it (a plane, a wall, a bore, a boss) and
   the engine cuts the ideal shape. The feature is what you say it is. This
   keeps the toolpath math closed-form and the output predictable.

3. **Lean on math and existing libraries.** Because features are analytical, a
   toolpath is mostly arithmetic on known shapes. Where real geometry work is
   unavoidable (polygon offsetting, STEP import) we use a proven library.

4. **The post is a formatter, never a fixer.** The text the checks gauged is
   the text that ships. Dialect lives in a declared `machine`, not in a script
   that patches the output.

5. **A "that's it?" feeling.** If a feature needs a pipeline to produce a cut,
   it is too complicated.

Two more that the last four months taught us, written down in
`PRINCIPLES.md`: the person owns the program, so every automatic choice must
be visible and overridable; and an empty problems list is not a certificate.

## What a project looks like

`plate.cam/main.part`:

```text
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000

stock Box(w=120, l=80, h=20, x=0, y=0, z=0)

tool T5 Endmill(dia=10.0)

# The Plane is the FINISHED face: stock top is z=0, a 0.5 mm skim is z=-0.5.
feature TopFace Plane(z=-0.5)

run End(tool=T5, surface=TopFace, stepover=6.0)
# The notch is the void; the L plate is what remains. Two of its edges are
# drawn past the blank, so they are open and the Clear enters from that air.
run Clear(tool=T5, profile=Notch, stepover=4.0, pattern=spine)
run Side(tool=T5, surface=Notch, comp=control)
```

`plate.cam/geometry.part`:

```text
feature Notch ProfilePocket(extent=20
    -5 40
    60 40
    60 85
    -5 85
)
```

`cargo run -- plate.cam` prints a 91-line program.

## What works today

| Area | What |
|---|---|
| 2.5D | Facing, pocket clearing (five patterns), wall tracing with engine or controller comp, drilling with seats, chamfers and corner rounds, islands and keep-outs by containment, open edges, allowances, rest clearing, a second setup that knows what the first cut |
| Analytic surfaces | Waterline and constant-scallop finishing of spheres, cones, tori and spline-revolved surfaces; exact bore and boss walls; SVG form tools; revolved NURBS read exactly |
| Mesh | A raster drop-cutter over STL, the one op that samples |
| Stock | A declared op-history model and a voxel swept field, composed; the rest report says where they disagree |
| Checks | Containment, engagement, reach, envelope, fixture, the safety lint. Each advises, or refuses when gated, and says by name when it cannot look |
| Import | STEP through the Python sidecar: every face accounted for, a proposed `.part` to review |
| Window | A thin viewer and editor over the same view models the CLI prints |
| Agents | `--check=json` and `nacs --mcp`; every verb is a CLI flag first |

The verbs name the part of the tool that cuts: `End` (flat end on a floor),
`Side` (flank on a wall), `Copy` (ball nose on a surface), `Form` (a tool
that carries its shape), `Clear` (volume removal). `points=[...]` on a `Form`
or `End` is a visit, which is what drilling is. `stepover` is the spacing
between passes; `stepdown` is a `Clear`'s axial bite. `CHECKLIST.md` has the
row-by-row status; `TAXONOMY.md` is the shape reference it indexes.

## What this is not

- **Not multi-axis.** Curved surfaces yes, rotary axes no.
- **Not a mesh reverse-engineer.** Recognition proposes a primitive; a person
  accepts it; the engine machines the primitive.
- **No per-machine posts, no telemetry, no network.**

## Layout

```
src/            the engine, the view models, the window
ops/            emitters by surface math (floor, profile, clear, drill, finish)
sidecar/        Python CadQuery/OCP STEP import; CONTRACT.md is the seam
*.cam/          fixture projects, each with a golden in tests/
tests/          integration tests, goldens, the safety lint
check.sh        the quick loop        lock.sh   the merge gate
```

`nacs.py` is a Python port frozen at the first phase's scope, kept so the
pipeline can be demonstrated without a Rust toolchain. If it and `src/`
disagree, `src/` is right.

## Where to look next

- `GETTING_STARTED.md`: for a person. The window, a part from nothing.
- `REFERENCE.md`: every word of the language and every act of the window.
- `AGENTS.md`: for an agent. The map and the rules.
- `DECISIONS.md`: what is settled and why, short.
- `ROADMAP.md`: what is next.

## License

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`)
or the MIT license (`LICENSE-MIT`), at your option. The program writes
G-code that moves a machine: it comes with no warranty, and you are
responsible for proving a program before you run it.
