#!/usr/bin/env python3
"""Generate tests/fixtures/freeform_cap.step — the STEP->mesh export fixture.

One plate carrying a single *freeform* cap that no camp-1/2 equation captures,
so it is exactly what the mesh-export tool (sidecar/import_step_mesh.py) is for:

  plate  50 x 50 x 6 mm, centred on the origin, sitting on z=0
  cap    a spline profile revolved 360 deg about Z -> a surface of revolution
         (a non-analytic face), sat on the plate top (z=6), peak at z=18

The point of the fixture is the *split*: the six plate faces are analytic
(plane) and stay recognition's job (import_step.py); the one revolved cap face
is the freeform tail that gets tessellated to STL and machined by
Feature::Mesh (ROADMAP Phase 4, camp 3). make_* scripts pin the geometry so a
kernel change that alters the face split is a loud test failure, not silent
drift.

The STEP file is committed once and regenerated only deliberately — STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_freeform_cap.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 7
EXPECTED_FREEFORM = 1  # the revolved cap; the other six are analytic planes
ANALYTIC = {"PLANE", "CYLINDER", "CONE", "SPHERE", "TORUS"}
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "freeform_cap.step"


def build():
    # A spline profile in the XZ plane, from the axis (r=0) out and back to the
    # axis, revolved about Z into a smooth dome-with-a-waist — a genuine
    # surface of revolution, not a sphere/cone/torus.
    prof = (
        cq.Workplane("XZ")
        .moveTo(0, 12)
        .spline([(0, 12), (6, 11), (11, 8), (13, 3), (12, 0)])
        .lineTo(0, 0)
        .close()
    )
    cap = prof.revolve(360, (0, 0, 0), (0, 1, 0))
    plate = cq.Workplane("XY").box(50, 50, 6, centered=(True, True, False))
    return plate.union(cap.translate((0, 0, 6)))


def main():
    solid = build()
    faces = solid.val().Faces()
    n = len(faces)
    freeform = [f for f in faces if f.geomType() not in ANALYTIC]
    if n != EXPECTED_FACES or len(freeform) != EXPECTED_FREEFORM:
        sys.exit(
            f"error: fixture has {n} faces ({len(freeform)} freeform), expected "
            f"{EXPECTED_FACES} ({EXPECTED_FREEFORM} freeform) — the geometry "
            "(or the kernel's face split) changed"
        )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(solid, str(OUT))
    print(f"wrote {OUT} ({n} faces, {len(freeform)} freeform)")


if __name__ == "__main__":
    main()
