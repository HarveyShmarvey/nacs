# The Python (CadQuery / OCP) sidecar

CAD import and feature recognition for nacs (ROADMAP Phase 4). The
sidecar reads a STEP file and emits (a) a total-accounting recognition report
and (b) proposed `.part` feature text for a human or agent to review. It never
machines anything — recognition proposes, the Rust core machines.

**Cost, stated up front:** the virtualenv is **~1.5 GB installed** (CadQuery +
OCP wrap the full OCCT kernel). It is by far the heaviest entry in the
project's dependency surface, and it is a required runtime **only** for
import / recognition / (future) camp-3 work. The Rust core builds, runs, and
tests without it — `cargo test` skips the sidecar tests when the venv is
absent.

## Setup

Once: `sh sidecar/setup.sh` (or `powershell -ExecutionPolicy Bypass -File sidecar\setup.ps1`
on Windows) finds Python 3.11, makes the venv and installs the stack. By hand
it is the two lines below. After that the window imports a STEP file dropped
on it, and `nacs --import <model.step> <project.cam>` does the same from a
terminal (Phase 50 slice 10).

Python **3.11** (the pinned stack installs from wheels there — no compiler):

```
python3.11 -m venv sidecar/.venv
sidecar/.venv/bin/pip install -r sidecar/requirements.txt
```

On Windows the venv puts its binaries under `Scripts\` instead of `bin/`
(`sidecar\.venv\Scripts\python.exe`); every `sidecar/.venv/bin/python` below
reads that way there, and the Rust tests look in the same place.

`pip install` is setup-time network use by you, once. The sidecar scripts
themselves never touch the network (the project's no-phone-home rule).
`sidecar/.venv/` is gitignored; `sidecar/requirements.txt` is the Python-side
dependency ledger, same annotation rule as `Cargo.toml`'s.

## Use

```
sidecar/.venv/bin/python sidecar/import_step.py <model.step> -o <outdir>/
    [--solid N] [--up {x+,x-,y+,y-,z+,z-}] [--allowance MM]
    [--origin X,Y,Z] [--given PROJECT --blank WORDS]
```

Writes `<outdir>/report.json` (every face accounted for: proposed primitive
or explicit `unsupported` + `CHECKLIST.md` row) and `<outdir>/proposed.part`
(feature lines to review and paste into a project). A multi-solid STEP (an
assembly) rejects with a per-solid inventory unless `--solid` names the
part; `--up` declares which model axis is up on the machine (a Y-up CAD
export imports with `--up y+`); `--allowance` says how far the blank stands
outside the model, so bores at the model's top are placed and the rim is
proposed as a boss to bring to size (the engine passes 1 unless told),
and a side can have its own margin after the number (`1,bottom=3,x+=10`);
`--origin` names the point that becomes 0,0,0 (the engine passes
`mid,mid,max`, the blank's centre and top, unless told; `CONTRACT.md`);
`--given` with `--blank` is the second setup: the engine hands over the
earlier program's blank turned over as the stock words, the sidecar measures
the blank's margins instead of taking an allowance, proposes the stock with
the `given from` line beside it, and leaves the plan-bounding rim to the
earlier setup (`rim_given`); `--origin` there reads the given blank, and
the line says where the earlier zero went (`at=`). The
full contract — schema, invariants,
determinism rules — is `sidecar/CONTRACT.md`.

Try it on the committed fixture:

```
sidecar/.venv/bin/python sidecar/import_step.py tests/fixtures/block_bore.step -o /tmp/out
cat /tmp/out/proposed.part
```

### DXF tool profiles

```
sidecar/.venv/bin/python sidecar/import_dxf_profile.py <profile.dxf> -o <outdir>/
```

Converts a DXF drawing of a cutter's open half-profile (r-z) into
`<outdir>/profile.svg` — the SVG that `tool T? FormTool(dia=...,
profile="...")` already consumes, so DXF support adds nothing to the Rust
core or the `.part` grammar. Supported entities: LINE, ARC, LWPOLYLINE;
anything else — or a branched, closed, or disconnected drawing — rejects
with the reason named (`DECISIONS.md` → "DXF tool profiles…").

### STEP → mesh export

```
sidecar/.venv/bin/python sidecar/import_step_mesh.py <model.step> -o <outdir>/
    [--solid N] [--up {x+,x-,y+,y-,z+,z-}]
```

The complement of recognition: recognition proposes primitives for the faces
it can read as equations; this tool tessellates the **freeform** tail it can't
(b-spline, bézier, surface of revolution/extrusion, offset) into
`<outdir>/mesh.stl` — the STL that `Feature::Mesh` already reads — plus a
reviewable `<outdir>/proposed.part` `Mesh(…)` line. The five analytic surface
types are left for `import_step.py`; a part with **no** freeform face rejects
(nothing to tessellate). `--solid` / `--up` are the same setup declarations as
recognition. Adds nothing to the Rust core (`DECISIONS.md` → "STEP→mesh
export…"; full contract in `sidecar/CONTRACT.md`).

### STEP → analytic NURBS (spline-of-revolution + general patch)

```
sidecar/.venv/bin/python sidecar/import_step_nurbs.py <model.step> -o <outdir>/
    [--solid N] [--up {x+,x-,y+,y-,z+,z-}]
```

The *analytic* counterpart to mesh export: where that tool tessellates a
freeform face and accepts the stair-step error, this reads the same faces
**exactly**. A full-revolution `SURFACE_OF_REVOLUTION` with a b-spline basis
curve and a +Z axis goes out as `<outdir>/meridian.spline`
(`Feature::SplineRevolve`, no tessellation error); an **untrimmed** general
`B_SPLINE_SURFACE` face goes out as `<outdir>/surface.patch`
(`Feature::NurbsPatch`, the raster drop-cutter against the exact surface —
`DECISIONS.md` → "the general NURBS patch…"), plus a reviewable
`<outdir>/proposed.part`. Narrow and honest: a partial revolution, tilted axis,
non-b-spline basis, a TRIMMED patch (it stays with the mesh path — reading its
full rectangle would invent surface), or a part with no such face all reject with the reason named
(the last pointing at the other two tools). `--solid` / `--up` are the same
setup declarations. Adds nothing to the Rust core (`DECISIONS.md` → "Analytic
spline-of-revolution reader…"; full contract in `sidecar/CONTRACT.md`).

## Fixtures

Each generator in `sidecar/fixtures/` writes a committed STEP file into
`tests/fixtures/`, paired with two committed goldens (`*.report.golden.json`
+ `*.proposed.golden.part`). The STEP files are regenerated only
deliberately: STEP headers carry a timestamp, so regeneration is a real
diff, not noise. Run from the repo root:

```
sidecar/.venv/bin/python sidecar/fixtures/make_block_bore.py
```

- `make_block_bore.py` → `block_bore.step` — the slice-1 walking skeleton: a
  plate with a bore and a boss (one proposable plane, one proposable bore;
  the boss began as the deliberate row-2.4 residue and graduated to a
  `Boss` proposal in the outer-profile slice, along with the plate's
  silhouette contour).
- `make_quadric_boss.py` → `quadric_boss.step` — the slice-2 quadric
  breadth: dome, apex cone, and half-embedded torus (proposed), plus a
  hemispherical bowl as the deliberate row-3.1 residue.
- `make_pocket_plate.py` → `pocket_plate.step` — the slice-3 grouped pocket
  recognition: a sharp and a filleted pocket (proposed as multi-face
  `Contour` groups), plus a tapered pocket (wall-verticality error) and a
  lipped pocket (mouth-rim undercut error) as the deliberate residues.
- `make_drilled_plate.py` → `drilled_plate.step` — the slice-4 hole-vs-bore
  semantics: one cylindrical void per bottom kind — through (`_Hole`),
  drill-point (`_Hole`, cone absorbed), flat floor (`_Bore`, floor
  absorbed), and a ball-end bottom as the deliberate wall-only fallback
  with a row-3.1 residue.
- `make_assembly_pair.py` → `assembly_pair.step` — the slice-5 multi-solid
  fixture: a recognizable plate plus a bystander block; rejects without
  `--solid`, recognizes the plate with `--solid 1`.
- `make_tilted_plate.py` → `tilted_plate.step` — the slice-5 tilted-part
  fixture: a drilled plate exported Y-up; `--up y+` recovers its Z-up
  twin's proposals exactly (the generator applies the inverse rotation, so
  the goldens' numbers are the human-readable Z-up ones).
- `make_outer_profile.py` → `outer_profile.step` — the slice-7
  outer-profile recognition: a plate whose below-top top anchors the part's
  silhouette contour (CW, material inside), a circular boss proposing the
  analytic `Feature::Boss` (row 2.4), a fillet-cornered plateau proposing a
  mixed line/arc contour, plus a tapered boss (wall-verticality error)
  and a mushroom (base-rim overhang error; its neck is the
  not-under-a-flat-cap row-2.4 residue) as the deliberate residues.
- `make_form_tool_dxf.py` → `form_tool.dxf` (+ golden
  `form_tool.profile.golden.svg`) — the DXF tool-profile fixture: a
  corner-radius form cutter's half-profile as one LINE + ARC + LWPOLYLINE
  chain in scrambled order and mixed directions, plus the two error
  fixtures `spline_profile.dxf` (unsupported entity) and
  `split_profile.dxf` (disconnected chain).
- `make_freeform_cap.py` → `freeform_cap.step` (+ goldens
  `freeform_cap.mesh.golden.stl` + `freeform_cap.proposed.golden.part`) —
  the STEP→mesh export fixture: a plate (six analytic planes, left for
  recognition) carrying one revolved spline cap (a surface of revolution —
  the freeform face tessellated to STL). `block_bore.step` doubles as the
  all-analytic error case (no freeform face). Its meridian deliberately
  bulges under (a slight undercut), which is why it stays on the mesh path.
- `make_spline_cap_step.py` → `spline_cap.step` (+ goldens
  `spline_cap.meridian.golden.spline` + `spline_cap.proposed.golden.part`) —
  the analytic spline-of-revolution fixture: the same kind of plate + revolved
  spline cap, but shaped as a MACHINABLE cap (monotone meridian, no undercut),
  so `import_step_nurbs.py` reads its b-spline meridian exactly and the Rust
  finisher machines it end to end. `block_bore.step` doubles as the
  no-spline-of-revolution error case.
- `make_patch_cap_step.py` → `patch_cap.step` (+ goldens
  `patch_cap.surface.golden.patch` + `patch_cap.proposed.golden.part`) — the
  general-NURBS-patch fixture: the `nurbs_patch.cam` Bézier net extruded down
  into a solid, so the untrimmed top face is read exactly and the downward
  bottom face pins the orientation skip.
- `make_patch_trimmed_step.py` → `patch_trimmed.step` — the same surface
  trimmed to a sub-domain: the trimmed-patch error case (a trimmed patch is
  never read as its full rectangle; it stays with the mesh path).
