#!/usr/bin/env python3
"""Generate tests/fixtures/step_hole.step — a stepped plate with two holes,
meant to be imported from the back (`--up z-`).

ROADMAP item 1, slice 7. Seen from the back, a hole that comes out across
the step breaks out onto three faces at once (the upper top, the step's
floor and the step's wall it notches) and is still a through hole; a
counterbore is a void under a ceiling, the other setup's, while the hole
through its floor is a through hole from this side.

  plate       60 x 40, z 0..10, the half x > 30 stepped down to z=6
  step hole   r 4 through the plate at (30, 12), straddling the step's edge
  counterbore r 6, z 6..10 at (15, 28), over a hole r 3 through the plate

Expected faces: plate 9 (bottom, upper top, step floor, the step wall in
two pieces -- the hole notches it through its whole height -- and 4 sides)
+ step hole 1 + counterbore 2 (wall, floor) + hole 1 = 13.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_step_hole.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 13
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "step_hole.step"


def build():
    plate = cq.Workplane("XY").box(60, 40, 10, centered=False)
    step = cq.Workplane("XY", origin=(30, 0, 6)).box(30, 40, 4, centered=False)
    step_hole = cq.Workplane("XY", origin=(30, 12, -1)).circle(4).extrude(12)
    counterbore = cq.Workplane("XY", origin=(15, 28, 6)).circle(6).extrude(5)
    hole = cq.Workplane("XY", origin=(15, 28, -1)).circle(3).extrude(12)
    return plate.cut(step).cut(step_hole).cut(counterbore).cut(hole)


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
