#!/usr/bin/env python3
"""DXF tool profile -> the SVG the Rust core already reads (ROADMAP Phase 4,
"Custom tool/cutter profiles from DXF").

Reads a DXF drawing of a cutter's half-profile (r-z: x is radius from the
tool axis, y is height along it), chains the modelspace entities into ONE
open polyline, and writes it as a minimal SVG whose single <path> is the
exact form `tool T? FormTool(dia=..., profile="...")` consumes (src/parse/svg.rs).
No second profile format enters the Rust core or the `.part` grammar -- DXF
support is this conversion, nothing else (DECISIONS.md -> "DXF tool
profiles…").

The profile must be a single open chain -- the same shape the SVG consumer
expects (tip out to shank). Anything else refuses with the reason named,
never a guess:

  entity types      LINE, ARC, LWPOLYLINE (with bulge arcs). Any other
                    modelspace entity (SPLINE, ELLIPSE, CIRCLE, INSERT, …)
                    refuses naming the type -- no silent approximation.
  connectivity      endpoints must meet within LIN_TOL; a branch point,
                    a closed loop, or disconnected pieces each refuse.

Scale and orientation do NOT matter: the Rust parser rescales the profile to
the tool's declared dia and measures heights from the min-radius point (the
tip), so a profile drawn in inches, upside down, or off-origin converts the
same (DECISIONS.md -> "Custom SVG tool profile scaling…"). What the drawing
must get right is the *shape*: radius/height ratios, and one open chain.

Invocation (argv + files, no stdin protocol, no network -- CONTRACT.md):

  sidecar/.venv/bin/python sidecar/import_dxf_profile.py <profile.dxf> -o <outdir>/

Writes <outdir>/profile.svg. On any error: one-line stderr, exit 1, no
partial output files.
"""

import argparse
import math
import sys
from pathlib import Path

import ezdxf

# Linear tolerance (mm-scale drawing units) for "these endpoints meet" when
# chaining entities -- the same value as import_step.py's LIN_TOL.
LIN_TOL = 1e-3

# Max chord error when tessellating an arc to line segments. A DELIBERATE
# duplicate of the Rust core's CHORD_TOL_MM (src/lib.rs) across the language
# boundary, same as import_step.py's -- the committed golden pins any drift.
CHORD_TOL = 0.01


def fmt(v):
    """One number formatter for everything emitted (determinism rule):
    3 decimals max, trailing zeros stripped, always at least one decimal,
    and no negative zero."""
    v = round(v, 3) + 0.0  # + 0.0 turns -0.0 into 0.0
    s = f"{v:.3f}".rstrip("0")
    return s + "0" if s.endswith(".") else s


def arc_points(cx, cy, r, a0, sweep):
    """Points along a circular arc from angle a0 (radians) through the
    signed sweep (CCW positive), INCLUDING both ends, tessellated at
    CHORD_TOL -- the same chord-error formula as import_step.py's
    edge_polyline (sagitta r*(1 - cos(theta/2)))."""
    theta_max = 2.0 * math.acos(max(-1.0, 1.0 - CHORD_TOL / r))
    n = max(1, math.ceil(abs(sweep) / theta_max))
    return [(cx + r * math.cos(a0 + sweep * k / n),
             cy + r * math.sin(a0 + sweep * k / n)) for k in range(n + 1)]


def bulge_points(p0, p1, b):
    """An LWPOLYLINE bulge segment as tessellated points, both ends
    included. bulge = tan(sweep/4), CCW positive; the centre falls out of
    sweep and chord alone: it sits r*cos(|sweep|/2) from the chord midpoint,
    left of travel for a CCW minor arc, with the sign of the cosine flipping
    it for major arcs and the sign of the sweep flipping it for CW."""
    sweep = 4.0 * math.atan(b)
    (x0, y0), (x1, y1) = p0, p1
    chord = math.hypot(x1 - x0, y1 - y0)
    r = chord / (2.0 * math.sin(abs(sweep) / 2.0))
    h = math.copysign(r * math.cos(abs(sweep) / 2.0), sweep)
    ux, uy = (x1 - x0) / chord, (y1 - y0) / chord
    cx = (x0 + x1) / 2.0 - uy * h
    cy = (y0 + y1) / 2.0 + ux * h
    return arc_points(cx, cy, r, math.atan2(y0 - cy, x0 - cx), sweep)


def entity_polylines(msp):
    """Every supported modelspace entity as a tessellated point list
    (a 'segment' the chainer connects). Unsupported types refuse loudly --
    a silently dropped entity would change the cutter's shape."""
    segs = []
    for e in msp:
        kind = e.dxftype()
        if kind == "LINE":
            s, t = e.dxf.start, e.dxf.end
            segs.append([(s.x, s.y), (t.x, t.y)])
        elif kind == "ARC":
            # DXF arcs are always CCW from start_angle to end_angle (deg).
            a0 = math.radians(e.dxf.start_angle)
            a1 = math.radians(e.dxf.end_angle)
            if a1 <= a0 + LIN_TOL / max(e.dxf.radius, LIN_TOL):
                a1 += 2.0 * math.pi
            c = e.dxf.center
            segs.append(arc_points(c.x, c.y, e.dxf.radius, a0, a1 - a0))
        elif kind == "LWPOLYLINE":
            if e.closed:
                raise ValueError(
                    "closed LWPOLYLINE -- a tool profile is the OPEN "
                    "half-profile from tip to shank, not a closed outline")
            vs = list(e.get_points("xyb"))
            pts = [(vs[0][0], vs[0][1])]
            for (x0, y0, b), (x1, y1, _) in zip(vs, vs[1:]):
                if abs(b) < 1e-12:
                    pts.append((x1, y1))
                else:
                    pts += bulge_points((x0, y0), (x1, y1), b)[1:]
            segs.append(pts)
        else:
            raise ValueError(
                f"unsupported DXF entity {kind} -- the profile importer "
                "reads LINE, ARC, and LWPOLYLINE only (no silent "
                "approximation of splines/ellipses/blocks)")
    if not segs:
        raise ValueError("no modelspace entities -- an empty drawing has no "
                         "profile to convert")
    return segs


def chain(segs):
    """Connect the segments end-to-end into one open polyline, reversing
    segments as needed (entity order and direction in a DXF are arbitrary).
    The connectivity checks ARE the honest-refusal gate: a branch, a closed
    loop, or disconnected pieces each name themselves."""
    # Cluster segment endpoints into nodes (endpoints within LIN_TOL meet).
    nodes = []  # [(x, y, [(seg_idx, end_idx), ...])]

    def node_at(x, y):
        for nd in nodes:
            if math.hypot(nd[0] - x, nd[1] - y) <= LIN_TOL:
                return nd
        nd = (x, y, [])
        nodes.append(nd)
        return nd

    ends = []  # per segment: (start_node, end_node)
    for i, seg in enumerate(segs):
        n0 = node_at(*seg[0])
        n0[2].append((i, 0))
        n1 = node_at(*seg[-1])
        n1[2].append((i, 1))
        ends.append((n0, n1))

    for nd in nodes:
        if len(nd[2]) > 2:
            raise ValueError(
                f"profile branches at ({fmt(nd[0])}, {fmt(nd[1])}) -- "
                f"{len(nd[2])} entity ends meet there; a tool profile is "
                "one unbranched chain")
    open_ends = [nd for nd in nodes if len(nd[2]) == 1]
    if not open_ends:
        raise ValueError("profile is a closed loop -- draw the OPEN "
                         "half-profile from tip to shank, not a closed "
                         "outline")
    if len(open_ends) > 2:
        raise ValueError(
            f"profile is disconnected -- {len(open_ends)} loose ends "
            f"(gaps wider than {LIN_TOL}); a tool profile is one connected "
            "chain")

    # Walk from the deterministic start: the loose end with the smaller
    # rounded (x, y) -- entity order in the file must not matter.
    start = min(open_ends, key=lambda nd: (round(nd[0], 3), round(nd[1], 3)))
    pts = []
    node, used = start, set()
    while True:
        nxt = [(i, e) for (i, e) in node[2] if i not in used]
        if not nxt:
            break
        i, end_idx = nxt[0]
        used.add(i)
        seg = segs[i] if end_idx == 0 else list(reversed(segs[i]))
        pts += seg if not pts else seg[1:]  # the joint point is shared
        node = ends[i][1 - end_idx]
    if len(used) != len(segs):
        raise ValueError(
            f"profile is disconnected -- only {len(used)} of {len(segs)} "
            "entities form one chain (a separate piece or loop remains)")
    return pts


def build_svg(pts, source_name):
    """The emitted SVG: one <path> of line segments -- exactly what the Rust
    parser reads -- wrapped for human review in any viewer. The scale(1 -1)
    group is viewer-only (SVG's y points down, the drawing's points up); the
    engine reads only the d attribute, where y stays the drawing's."""
    d = "M " + " L ".join(f"{fmt(x)} {fmt(y)}" for x, y in pts)
    xs = [x for x, _ in pts]
    ys = [y for _, y in pts]
    view = (f"{fmt(min(xs))} {fmt(-max(ys))} "
            f"{fmt(max(xs) - min(xs))} {fmt(max(ys) - min(ys))}")
    return "\n".join([
        f"<!-- tool cutter profile converted from {source_name} by "
        "sidecar/import_dxf_profile.py -->",
        "<!-- review before use; consume with: "
        'tool T? FormTool(dia=..., profile="profile.svg") -->',
        "<!-- x is radius from the tool axis, y is height above the tip; "
        "the engine rescales to",
        "     the tool's dia and re-measures heights from the min-radius "
        "point, so the drawing's",
        "     absolute units and position do not matter -->",
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}">',
        '  <g transform="scale(1 -1)">',
        f'    <path fill="none" stroke="black" stroke-width="0.05" d="{d}"/>',
        "  </g>",
        "</svg>",
    ]) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("dxf", type=Path, help="input DXF drawing of the "
                    "cutter's open half-profile (r-z)")
    ap.add_argument("-o", "--outdir", type=Path, required=True,
                    help="directory for profile.svg")
    args = ap.parse_args()

    try:
        msp = ezdxf.readfile(args.dxf).modelspace()
        segs = entity_polylines(msp)
        pts = chain(segs)
        svg = build_svg(pts, args.dxf.name)
    except Exception as e:
        sys.exit(f"error: {args.dxf}: {e}")

    args.outdir.mkdir(parents=True, exist_ok=True)
    (args.outdir / "profile.svg").write_text(svg, encoding="utf-8", newline="\n")
    print(f"{len(segs)} entities -> 1 chain, {len(pts)} points: profile.svg")


if __name__ == "__main__":
    main()
