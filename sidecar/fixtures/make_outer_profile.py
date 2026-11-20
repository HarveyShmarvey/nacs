#!/usr/bin/env python3
"""Generate tests/fixtures/outer_profile.step — the outer-profile slice's
committed fixture (DECISIONS.md "Outer-profile recognition…").

One plate carrying four features, one per branch of the cap-anchored pass:

  plate    60 x 50 x 10 mm, corner at the origin -- its top (z=10, below the
           topmost height) anchors the part's outer SILHOUETTE contour, a
           through profile down to the part bottom
  boss     dia 16, centred (14, 14), rising 6 above the plate (top z=16) --
           a full-revolution wall under a flat cap: proposes Feature::CylinderBoss
           (CHECKLIST row 2.4)
  plateau  20 x 14 with one vertical edge filleted r=4, rising 6 (top z=16)
           -- a mixed plane/arc wall loop: proposes a CW Contour (row 2.8)
  frustum  12 x 12 base, planar draft walls tapering inward ~21.8 deg,
           rising 5 (top z=15) -- DELIBERATE refusal: the walls descend from
           the cap but are not vertical (row 1.2). Built with a tapered
           extrude, NOT loft -- loft emits bspline walls, which hit the
           pass's silent-skip filter instead of the verticality refusal
  mushroom neck dia 8 (z=10..13) under a cap dia 14 (z=13..16) -- DELIBERATE
           refusal: the cap-edge wall's base rim adjoins the cap's downward
           underside above the part bottom (an overhang), and the neck wall
           is the reworded row-2.4 residue (a boss not under a flat cap)

The three flat tops at z=16 (boss, plateau, mushroom cap) are all topmost
planes ("topmost is a height, not a single face") and keep their Floor
proposals; the plate top and frustum top are below-top anchor caps, one
proposing, one refused -- exercising both cap_note paths.

The STEP file is committed once and regenerated only deliberately -- STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_outer_profile.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 23
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "outer_profile.step"


def build():
    plate = cq.Workplane("XY").box(60, 50, 10, centered=False)
    boss = cq.Workplane("XY", origin=(14, 14, 10)).circle(8).extrude(6)
    plateau = (cq.Workplane("XY", origin=(30, 6, 10)).box(20, 14, 6, centered=False)
               .edges("|Z and >X and >Y").fillet(4))
    frustum = (cq.Workplane("XY", origin=(14, 38, 10)).rect(12, 12)
               .extrude(5, taper=21.8))
    neck = cq.Workplane("XY", origin=(42, 36, 10)).circle(4).extrude(3)
    cap = cq.Workplane("XY", origin=(42, 36, 13)).circle(7).extrude(3)
    return plate.union(boss).union(plateau).union(frustum).union(neck).union(cap)


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
