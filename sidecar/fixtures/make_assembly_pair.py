#!/usr/bin/env python3
"""Generate tests/fixtures/assembly_pair.step — the multi-solid fixture.

Two disjoint solids in one STEP file (an assembly, as exported by most CAD
when the user forgets to isolate a part). The sidecar must never guess which
one is the part on the machine (DECISIONS.md -> "Import breadth…"):

  S1  plate 60 x 40 x 10 mm at the origin, with a flat-bottom bore
      dia 16, depth 5 at (20, 20) — real recognizable content, so the
      --solid 1 goldens prove recognition runs unchanged on a selected
      solid (top plane proposed, wall + floor grouped as a _Bore)
  S2  plain block 20 x 20 x 10 mm at (80, 0, 0) — the bystander solid
      (a fixture, a second part) that makes the file an assembly

Without --solid the import refuses with a one-line per-solid inventory;
with --solid 1 it recognizes S1 exactly as if it were alone in the file.

Expected faces: S1 = 6 plate + bore wall + bore floor = 8; S2 = 6.

The STEP file is committed once and regenerated only deliberately — STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_assembly_pair.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = (8, 6)  # (S1, S2)
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "assembly_pair.step"


def build():
    plate = cq.Workplane("XY").box(60, 40, 10, centered=False)
    bore = cq.Workplane("XY", origin=(20, 20, 10)).circle(8).extrude(-5)
    s1 = plate.cut(bore).val()
    s2 = cq.Workplane("XY", origin=(80, 0, 0)).box(20, 20, 10, centered=False).val()
    return s1, s2


def main():
    s1, s2 = build()
    counts = (len(s1.Faces()), len(s2.Faces()))
    if counts != EXPECTED_FACES:
        sys.exit(f"error: fixture has {counts} faces, expected {EXPECTED_FACES} — "
                 "the geometry (or the kernel's face merging) changed")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(cq.Compound.makeCompound([s1, s2]), str(OUT))
    print(f"wrote {OUT} ({counts[0]} + {counts[1]} faces in 2 solids)")


if __name__ == "__main__":
    main()
