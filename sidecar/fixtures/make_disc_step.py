#!/usr/bin/env python3
"""Generate tests/fixtures/disc_step.step — the silhouette fixture.

Phase 46 slice 2: a floor whose wire runs out to the part's SILHOUETTE — a
full-revolution cylinder with material INSIDE it, spanning the floor's height
— used to refuse as `pocket_floor_of_bore` (the check never asked which side
the material was on). It proposes as a `ProfilePocket` carrying its true wire
now: the silhouette edge drawn a chord tolerance PAST the blank (the engine
clips it to the declared stock, so on a `stock Cylinder` of this diameter it
reads OPEN and on a larger Box it reads WALL), the step wall a wall, and the
rim of a bore that pierces the floor's edge from below a wall the Clear rides
to (the bore proposes beside it).

  disc   dia 120, 30 tall, axis at the origin, bottom at z=0 -> top at z=30
  step   the half x > 20 cut down to z=20 (a wall at x=20, 20..30)
  pad    a 20 x 20 boss on the step, 35..55 x -10..10, top at z=28
  bore   dia 12 at (20, -35), from the top down to z=12 -- straddling the
         step wall, so the step floor's outer wire runs along its rim

Expected faces: disc 3 (top, bottom, side) + step floor 1 + step wall 2 (the bore
splits it) + pad 5 (top + 4 sides) + bore 2 (wall + bottom) = 13.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_disc_step.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 13
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "disc_step.step"


def build():
    disc = cq.Workplane("XY").circle(60).extrude(30)
    step = cq.Workplane("XY", origin=(20, -70, 20)).box(60, 140, 10, centered=False)
    pad = cq.Workplane("XY", origin=(35, -10, 20)).box(20, 20, 8, centered=False)
    bore = cq.Workplane("XY", origin=(20, -35, 12)).circle(6).extrude(18)
    return disc.cut(step).union(pad).cut(bore)


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
