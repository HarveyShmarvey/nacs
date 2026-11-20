#!/usr/bin/env python3
"""Generate tests/fixtures/pocket_plate.step — the pocket-recognition fixture.

One plate carrying four pockets, chosen so the pocket grouping pass exercises
both graduated cases and both new refusal checks (the slice's deliberate
residue, mirroring block_bore's boss and quadric_boss's bowl):

  plate 100 x 80 x 15 mm, corner at the origin
  A  sharp pocket     30 x 20, x 10..40, y 10..30, floor z=9 (depth 6) --
                      proposed, all-line boundary (CHECKLIST row 2.1)
  B  filleted pocket  35 x 25, x 55..90, y 10..35, corner r 6, floor z=10
                      (depth 5) -- proposed, arc-tessellated boundary with
                      partial-cylinder fillet walls (CHECKLIST row 2.7)
  C  tapered pocket   20 x 15 mouth centred (30, 57.5), 20 deg draft, floor
                      z=10 -- the verticality residue: oblique walls refuse
                      the floor (check 3)
  D  lipped pocket    cavity 16 x 12, z 4..9, centred (70, 60), under a
                      mouth 8 x 6, z 9..15, same centre -- the undercut
                      residue: the cavity's rim is capped by an overhanging
                      lip (check 5). Its mouth is OPEN over the floor
                      centroid, so a single-point probe alone would pass it.

Expected faces (EXPECTED_FACES below): plate 6 + A 5 (floor + 4 walls)
+ B 9 (floor + 4 planar walls + 4 fillet cylinders) + C 5 (floor + 4 oblique
walls) + D 10 (cavity floor + 4 cavity walls + annular lip underside +
4 mouth walls) = 35. All four cut the one plate top, which stays a single
face with four inner wires.

The STEP file is committed once and regenerated only deliberately -- STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_pocket_plate.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 35
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "pocket_plate.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 15, centered=False)
    sharp = (cq.Workplane("XY", origin=(10, 10, 15))
             .rect(30, 20, centered=False).extrude(-6))
    filleted = (cq.Workplane("XY", origin=(55, 10, 15))
                .rect(35, 25, centered=False).extrude(-5)
                .edges("|Z").fillet(6))
    # Tapered: extruding downward with draft shrinks toward the floor, so
    # the four walls are true oblique PLANEs (a loft would give BSplines).
    tapered = (cq.Workplane("XY", origin=(30, 57.5, 15))
               .rect(20, 15).extrude(-5, taper=20))
    mouth = (cq.Workplane("XY", origin=(70, 60, 15))
             .rect(8, 6).extrude(-6))
    cavity = (cq.Workplane("XY", origin=(70, 60, 9))
              .rect(16, 12).extrude(-5))
    return plate.cut(sharp).cut(filleted).cut(tapered).cut(mouth).cut(cavity)


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
