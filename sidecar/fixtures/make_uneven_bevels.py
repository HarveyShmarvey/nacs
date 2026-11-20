#!/usr/bin/env python3
"""Generate tests/fixtures/uneven_bevels.step — a boss whose top edges carry
two different chamfers.

ROADMAP item 1, slice 5: the real part's ribs are beveled 1.0 mm on some
edges and 1.5 mm on others, and the outer-profile pass used to refuse the
whole rib for it ("one feature carries one chamfer"). Now the profile
proposes without a chamfer, its loop read at the walls, and each strip
stays residue saying which bevels it saw — the edge is the hand's to break.

  plate   100 x 80 x 20, corner at the origin                    -> top at z=20
  boss    30 x 20, 8 tall, at 10..40 x 10..30
          its two long top edges chamfered 1.0 mm, its two short ones 1.5 mm

Expected faces: plate 6 + boss 9 (top, 4 walls, 4 strips) = 15.

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_uneven_bevels.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 15
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "uneven_bevels.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 20, centered=False)
    boss = cq.Workplane("XY", origin=(10, 10, 20)).box(30, 20, 8, centered=False)
    boss = boss.edges("|X and >Z").chamfer(1.0)
    boss = boss.edges("|Y and >Z").chamfer(1.5)
    return plate.union(boss)


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
