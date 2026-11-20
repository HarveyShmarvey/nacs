#!/usr/bin/env python3
"""Generate the DXF tool-profile fixtures in tests/fixtures/ (ROADMAP
Phase 4, "Custom tool/cutter profiles from DXF").

form_tool.dxf — the good fixture: a corner-radius form cutter's open
half-profile (r-z), built to satisfy every constraint the Rust consumer
(ops::finish::finish_sphere_profile) enforces — starts on-axis, flares
outward monotonically, outward-convex, ends in a vertical wall (90 deg):

  tip (0,0) -> LINE to (3,3)          the 45 deg flank (drawn REVERSED,
                                      shank->tip, so chaining must flip it)
  -> ARC r=2, -45..-22.5 deg          first half of the tangent blend
  -> LWPOLYLINE [bulge arc            second half of the blend, then the
     -22.5..0 deg; line to y=8]       vertical wall to the shank

One entity of each supported type, written in scrambled order and mixed
directions, so the converter's chaining — not the file's entity order —
determines the profile. The blend arc is tangent-continuous with the flank
and the wall, keeping the chained tangent angles non-decreasing (the
consumer's convexity check) through fmt()'s 3-decimal rounding.

spline_profile.dxf — refusal fixture: flank + SPLINE shank; the importer
must refuse naming the entity type, never approximate the spline.

split_profile.dxf — refusal fixture: flank + wall with a 0.5 gap between
them; the importer must refuse as disconnected, never bridge the gap.

DXF headers carry timestamps, so the files are committed once and
regenerated only deliberately — same rule as the STEP fixtures. Run from
the repo root:

  sidecar/.venv/bin/python sidecar/fixtures/make_form_tool_dxf.py
"""

import math
from pathlib import Path

import ezdxf

OUTDIR = Path(__file__).resolve().parents[2] / "tests" / "fixtures"

# The blend arc: r=2, tangent to the 45 deg flank at (3,3) and to the
# vertical wall at its right edge.
R = 2.0
CX = 3.0 - R * math.sin(math.pi / 4)
CY = 3.0 + R * math.cos(math.pi / 4)
WALL_X = CX + R
MID = (CX + R * math.cos(math.radians(-22.5)),
       CY + R * math.sin(math.radians(-22.5)))


def make_form_tool():
    doc = ezdxf.new("R2000")
    msp = doc.modelspace()
    # Scrambled order + a reversed entity, deliberately.
    msp.add_lwpolyline(
        [(*MID, math.tan(math.radians(22.5) / 4.0)),  # bulge arc -22.5 -> 0 deg
         (WALL_X, CY, 0.0),
         (WALL_X, 8.0, 0.0)],
        format="xyb")
    msp.add_line((3.0, 3.0), (0.0, 0.0))  # the flank, drawn shank->tip
    msp.add_arc(center=(CX, CY), radius=R, start_angle=-45.0, end_angle=-22.5)
    doc.saveas(OUTDIR / "form_tool.dxf")


def make_spline_profile():
    doc = ezdxf.new("R2000")
    msp = doc.modelspace()
    msp.add_line((0.0, 0.0), (3.0, 3.0))
    msp.add_spline(fit_points=[(3.0, 3.0), (4.0, 5.0), (4.0, 8.0)])
    doc.saveas(OUTDIR / "spline_profile.dxf")


def make_split_profile():
    doc = ezdxf.new("R2000")
    msp = doc.modelspace()
    msp.add_line((0.0, 0.0), (3.0, 3.0))
    msp.add_line((3.5, 3.0), (3.5, 8.0))  # 0.5 gap from the flank's end
    doc.saveas(OUTDIR / "split_profile.dxf")


def main():
    OUTDIR.mkdir(parents=True, exist_ok=True)
    make_form_tool()
    make_spline_profile()
    make_split_profile()
    print(f"wrote form_tool.dxf, spline_profile.dxf, split_profile.dxf "
          f"in {OUTDIR}")


if __name__ == "__main__":
    main()
