#!/usr/bin/env python3
"""Generate tests/fixtures/boss_chamfer.step — a round boss on a plate, its
top edge broken by a 1 mm bevel.

ROADMAP item 9. The bevel is a cone between the boss's cap and its wall; it
is the boss's `chamfer_depth`, as a strip is a profile's.

  plate   40 x 40 x 10
  boss    dia 16 at 20,20, 8 tall, 1 x 45 degrees on the top edge

Run from the repo root:

  sidecar/.venv/Scripts/python.exe sidecar/fixtures/make_boss_chamfer.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 9
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "boss_chamfer.step"


def build():
    plate = cq.Workplane("XY").box(40, 40, 10, centered=False)
    boss = (cq.Workplane("XY", origin=(20, 20, 10)).circle(8).extrude(8)
            .faces(">Z").edges().chamfer(1))
    return plate.union(boss)


def main():
    solid = build()
    n = len(solid.val().Faces())
    if n != EXPECTED_FACES:
        sys.exit(f"error: fixture has {n} faces, expected {EXPECTED_FACES} -- "
                 "the geometry (or the kernel's face merging) changed")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(solid, str(OUT))
    print(f"wrote {OUT} ({n} faces)")


if __name__ == "__main__":
    main()
