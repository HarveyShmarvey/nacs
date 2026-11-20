#!/usr/bin/env python3
"""Generate tests/fixtures/tilted_plate.step — the tilted-part fixture.

A drilled plate modeled the way a Y-up CAD package exports it: the part's
"up" is the model's +Y axis, so imported as-is every face refuses into the
residue (vertical walls where tops should be, oblique nothing anywhere — an
honest but useless report). Imported with `--up y+` the sidecar rotates the
model's +Y onto the machine's +Z before recognition, and the part reads
exactly like its Z-up twin (DECISIONS.md -> "Import breadth…"):

  plate 100 x 80 x 15 mm (post-rotation: corner at the origin, top at z=15)
  A  through hole     dia 12 at (20, 20)                       -> _Hole
  B  flat-bottom bore dia 16, depth 6 at (80, 20)              -> _Bore
     (wall + floor grouped)

The generator builds the part Z-up in exactly those coordinates, then
applies the INVERSE of the import rotation (-90 deg about X) before export
— so the committed goldens' numbers are the human-readable Z-up ones, and
`--up y+` provably recovers them.

Expected faces: 6 plate + A wall + B wall + B floor = 9.

The STEP file is committed once and regenerated only deliberately — STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_tilted_plate.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 9
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "tilted_plate.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 15, centered=False)
    a = cq.Workplane("XY", origin=(20, 20, -1)).circle(6).extrude(17)
    b = cq.Workplane("XY", origin=(80, 20, 15)).circle(8).extrude(-6)
    solid = plate.cut(a).cut(b).val()
    # Tip the part over: model +Z (the machined top) lands on +Y, the
    # inverse of import_step.py's `--up y+` rotation (+90 deg about X).
    return solid.rotate((0, 0, 0), (1, 0, 0), -90)


def main():
    solid = build()
    n = len(solid.Faces())
    if n != EXPECTED_FACES:
        sys.exit(f"error: fixture has {n} faces, expected {EXPECTED_FACES} — "
                 "the geometry (or the kernel's face merging) changed")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    cq.exporters.export(solid, str(OUT))
    print(f"wrote {OUT} ({n} faces, Y-up)")


if __name__ == "__main__":
    main()
