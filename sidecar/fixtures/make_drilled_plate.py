#!/usr/bin/env python3
"""Generate tests/fixtures/drilled_plate.step — the hole-vs-bore fixture.

One plate, four cylindrical voids, one per bottom kind the recognition
distinguishes (DECISIONS.md -> "Hole-vs-bore recognition…"):

  plate 100 x 80 x 15 mm, corner at the origin
  A  through hole      dia 12 at (20, 20) — bottom opens through      -> _Hole
  B  drill-point hole  dia 10 at (50, 20), full-dia depth 8, point
     length 3 (included angle 2*atan(5/3) ~ 118.1 deg)                -> _Hole
     (wall + cone grouped; depth reaches the tip: 11)
  C  flat-bottom bore  dia 16 at (80, 20), depth 6                    -> _Bore
     (wall + floor grouped; a twist drill can't leave a flat bottom)
  D  ball-end bottom   dia 10 at (20, 60), wall from z=15 to z=8,
     hemispherical bottom r=5 — the deliberate fallback: wall-only
     _Bore, the spherical bottom refused as a row-3.1 bowl

Expected faces (EXPECTED_FACES below): plate bottom, 4 side walls, plate
top, A wall, B wall + cone, C wall + floor, D wall + sphere = 13.

The STEP file is committed once and regenerated only deliberately — STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_drilled_plate.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 13
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "drilled_plate.step"


def build():
    plate = cq.Workplane("XY").box(100, 80, 15, centered=False)

    # A -- through hole: the cutter overshoots both faces.
    a = cq.Workplane("XY", origin=(20, 20, -1)).circle(6).extrude(17)

    # B -- drill-point hole: cylinder to z=7, cone from there to the apex
    # at z=4 (makeCone puts the apex at its local origin, base at z=height).
    b = cq.Workplane("XY", origin=(50, 20, 15)).circle(5).extrude(-8)
    b = b.union(cq.Workplane(obj=cq.Solid.makeCone(0.0, 5.0, 3.0).translate(
        cq.Vector(50, 20, 4))))

    # C -- flat-bottom bore.
    c = cq.Workplane("XY", origin=(80, 20, 15)).circle(8).extrude(-6)

    # D -- ball-end-milled bottom: cylinder to z=8, hemisphere below it
    # (the sphere's upper half lies inside the cylinder's void). NB
    # makeSphere's default latitude range is 0..90 deg -- a dome, not a
    # ball -- so the full range is spelled out.
    d = cq.Workplane("XY", origin=(20, 60, 15)).circle(5).extrude(-7)
    d = d.union(cq.Workplane(obj=cq.Solid.makeSphere(
        5.0, cq.Vector(20, 60, 8), angleDegrees1=-90, angleDegrees2=90)))

    return plate.cut(a).cut(b).cut(c).cut(d)


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
