#!/usr/bin/env python3
"""Generate tests/fixtures/sunken_bore.step — the sunken-bore fixture.

The first real part (an outside STEP, 2026-10-09) had counterbores on a step
30 mm below its top, and the importer proposed them as `CylinderBore`s —
whose top is always the stock top — so they would have been cut 30 mm too
high, with no error. For a day this fixture held the refusal
(`cylinder_bore_sunken`); since Phase 45 slice 3 it holds the PLACEMENT:

  plate   100 x 80 x 20, corner at the origin           -> top at z=20
  step    x 50..100 cut down to z=10 (open on three sides)
  bore A  dia 16, centred (25, 40), from the top (z=20) down 8 -> proposes,
          no `z` (the rim is the stock top)
  bore B  dia 16, centred (75, 40), from the step (z=10) down 6 -> proposes
          with `z=10.0`, its rim, and a reason naming the step

Expected faces: bottom 1 + plate walls 4 + riser 1 + step 1 + top 1 + two
bore walls + two bore floors = 12.

Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_sunken_bore.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 12
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "sunken_bore.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 20, centered=False)
    step = cq.Workplane("XY", origin=(50, 0, 10)).box(50, 80, 10, centered=False)
    bore_a = cq.Workplane("XY", origin=(25, 40, 20)).circle(8).extrude(-8)
    bore_b = cq.Workplane("XY", origin=(75, 40, 10)).circle(8).extrude(-6)
    return plate.cut(step).cut(bore_a).cut(bore_b)


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
