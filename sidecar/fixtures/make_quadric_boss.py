#!/usr/bin/env python3
"""Generate tests/fixtures/quadric_boss.step — the quadric-recognition fixture.

One plate carrying the three camp-2 quadric bosses that graduate in the
Phase-4 "quadric recognition breadth" slice, plus one deliberate residue
(mirroring block_bore's row-2.4 boss residue):

  plate 100 x 80 x 15 mm, corner at the origin
  dome   sphere r 12, centred (30, 25), equator on the plate top (z=15)
  cone   base r 10, height 12, base on the plate top, apex up, at (70, 25)
  torus  major r 12, minor r 4, centre plane on the plate top, at (65, 55)
  bowl   sphere r 10 CUT into the top at (25, 55) -- the residue case:
         a concave spherical face (CHECKLIST row 3.1, todo)

The dome, torus, and bowl are half-embedded so exactly the machinable half
(or, for the bowl, the unmachinable mirror of it) is the exposed face --
that is what the classifiers' equator checks key on.

Expected faces (EXPECTED_FACES below): plate bottom, 4 side walls, plate
top, dome, cone, torus, bowl, plus the disk of plate top *inside* the torus
ring = 11. (The half-embedded torus cuts an annulus out of the top plane, so
the disk in the donut hole is its own planar face — at the same z=15 as the
main top, so both faces propose as `Plane`; "topmost" is a height, not a
single face.)

The STEP file is committed once and regenerated only deliberately -- STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_quadric_boss.py
"""

import sys
from pathlib import Path

import cadquery as cq
from cadquery import Vector

EXPECTED_FACES = 11
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "quadric_boss.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 15, centered=False)
    dome = cq.Solid.makeSphere(
        12, Vector(30, 25, 15), angleDegrees1=-90, angleDegrees2=90
    )
    cone = cq.Solid.makeCone(10, 0, 12, Vector(70, 25, 15))
    torus = cq.Solid.makeTorus(12, 4, Vector(65, 55, 15))
    bowl = cq.Solid.makeSphere(
        10, Vector(25, 55, 15), angleDegrees1=-90, angleDegrees2=90
    )
    return plate.union(dome).union(cone).union(torus).cut(bowl)


def main():
    solid = build()
    n = len(solid.val().Faces())
    if n != EXPECTED_FACES:
        sys.exit(f"error: fixture has {n} faces, expected {EXPECTED_FACES} — "
                 "the geometry (or the kernel's face merging) changed")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(solid, str(OUT))
    print(f"wrote {OUT} ({n} faces)")


if __name__ == "__main__":
    main()
