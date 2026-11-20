#!/usr/bin/env python3
"""Generate tests/fixtures/trim_wire.step — the 3D-wire trim slice's
committed fixture (DECISIONS.md "The 3D-wire Contour…", slice 3).

A crown-roofed plate — a part with NO flat +Z plane anywhere (z_top is None,
so the cap/floor-anchored passes have nothing to anchor on), exercising every
branch of the part-bottom-anchored trim-wire pass:

  crown    60 x 50 plate whose top is a ridge roof: edges at z=9 rising to a
           ridge at x=30, z=12 (slope 0.1). The bottom face's OUTER wire
           bounds four vertical silhouette walls -- two rectangles (x=0,
           x=60, horizontal top edges at z=9) and two gabled pentagons (y=0,
           y=50, TWO sloped top edges each, exercising the cross-gable
           chaining) -- proposing a CW 6-point ContourBoss, extent=12
  cutout   10 x 12 rectangular through hole (x 10..20, y 14..26) under the
           left roof plane: an inner wire of the bottom face, four vertical
           walls whose straight top edges ride the slope (z=10 at x=10,
           z=11 at x=20), proposing a CCW ContourPocket, extent=11
  filleted 12 x 12 through hole (x 36..48, y 14..26) with one vertical edge
           filleted r=3 -- DELIBERATE refusal: the fillet's cylindrical wall
           meets the sloped roof in a curved top edge, which cannot ride the
           wire grammar's straight x y z segments (tessellating a 3D curve
           would be a guess); the refusal lands on all five walls
  roofs    the two sloped planes are row-1.2 residue (oblique planar face),
           and the bottom face keeps its residue entry with the reason
           rewritten to name the trim wires anchored on it

The STEP file is committed once and regenerated only deliberately -- STEP
headers carry a timestamp, so a regeneration is a real reviewable diff, not
noise. Run from the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_trim_wire.py
"""

import sys
from pathlib import Path

import cadquery as cq

EXPECTED_FACES = 16
OUT = Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "trim_wire.step"


def build():
    crown = (cq.Workplane("XZ")
             .polyline([(0, 0), (60, 0), (60, 9), (30, 12), (0, 9)]).close()
             .extrude(-50))
    cutout = cq.Workplane("XY", origin=(10, 14, -1)).box(10, 12, 21, centered=False)
    filleted = (cq.Workplane("XY", origin=(36, 14, -1)).box(12, 12, 21, centered=False)
                .edges("|Z and <X and <Y").fillet(3))
    return crown.cut(cutout).cut(filleted)


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
