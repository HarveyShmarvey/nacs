#!/usr/bin/env python3
"""Generate tests/fixtures/block_bore.step — the sidecar's committed fixture.

One plate, one blind bore, one boss, chosen so the walking skeleton's report
exercises every branch of the v1 contract:

  plate 100 x 80 x 15 mm, corner at the origin
  blind bore  dia 30, centred (35, 40), from the plate top (z=15) down 10
  boss        dia 20, centred (70, 40), rising 5 above the plate (top z=20)

Expected faces (EXPECTED_FACES below): plate bottom, 4 side walls, plate top,
bore wall, bore floor, boss wall, boss top = 10. The bore wall and the boss
top are the two proposable faces; the boss wall is the deliberate CHECKLIST
row 2.4 residue entry (the first residue item DECISIONS.md predicts real
parts will hit).

The STEP file is committed once and regenerated only deliberately — STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_block_bore.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 10
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "block_bore.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 15, centered=False)
    boss = cq.Workplane("XY", origin=(70, 40, 15)).circle(10).extrude(5)
    solid = plate.union(boss)
    bore = cq.Workplane("XY", origin=(35, 40, 15)).circle(15).extrude(-10)
    return solid.cut(bore)


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
