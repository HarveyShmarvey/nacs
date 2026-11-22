#!/usr/bin/env python3
"""Propose the stock for a STEP solid, as one `.part` statement.

Two answers, and the file says which it gave. A PRISM (every face flat on
top, flat on the bottom or vertical, and the topmost face spanning the
whole plan) IS its own blank: a `stock Profile(h=, z=)` block whose
outline is the topmost face's outer wire and whose height is the solid's
Z extent. Anything else gets the smallest blank the model fits, declared
as such in the comment above the line: a `stock Cylinder` when one
vertical cylindrical face bounds the plan on all four sides (a disc, a
ring, a round-ribbed casting), a `stock Box` otherwise. The bounding
blank is the model's exact extent unless `--allowance` says how far the
blank stands outside it, on the sides and the top (the default is none).

A third answer when the part has a program already (`--given <project>
--blank <words>`): the blank is that program's turned over, as the engine
computed it, not measured from the model, and the proposal carries the
`given from <project> up=<axis>` line beside the stock, since the blank
turned over must be the blank and that program's cuts are air here.

Writes `proposed_stock.part` in the output directory and says one line.
Nothing is written when the solid cannot be chosen (an assembly with no
--solid); that is an error, not a proposal.

    python import_stock.py <model.step> -o <outdir> [--up z-] [--solid N] [--allowance 1]

Reads the solid as import_step.py does (the same selection, the same
up-rotation), so a stock and a target imported from one file agree on
the frame.
"""
import argparse
import sys
import textwrap
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from import_step import (UP_ROTATIONS, blank_margins, blank_words,  # noqa: E402
                         fmt, origin_note, parse_blank, parse_origin, place_origin,
                         plan_bounding_cylinder, prism_outline, select_solid)

def propose_stock(step_path, solid_sel=None, up="z+", allowance=0.0, blank=None, given=None, origin=None):
    """The proposal's text, the statement's kind word and one phrase for the
    summary line."""
    solid, solid_sel, n_solids = select_solid(step_path, solid_sel)
    if UP_ROTATIONS[up] is not None:
        axis, degrees = UP_ROTATIONS[up]
        solid = solid.rotate((0, 0, 0), axis, degrees)
    origin_at = None
    if origin is not None:
        solid, origin_at = place_origin(solid, origin, allowance)
    bb = solid.BoundingBox()
    faces = solid.Faces()
    h = bb.zmax - bb.zmin
    outer, why_not = prism_outline(faces, bb)
    if blank is not None:
        # The second setup (slice 12): the blank is the earlier program's
        # turned over, read as the engine computed it; the model is only
        # checked to lie inside it.
        kind, vals = blank
        side, top = blank_margins(blank, faces, bb)
        lines = [f"# Proposed stock by sidecar/import_stock.py from {given}, the program"]
        lines += textwrap.wrap(
            f"before this one (--given): its blank turned over (up={up}), not measured "
            f"from the model -- the blank turned over must be the blank, and "
            f"`given from` reads that program's cuts as air here. It stands "
            f"{fmt(side)} outside the model's plan and {fmt(top)} above its top.",
            width=68, initial_indent="# ", subsequent_indent="# ")
        about = (f"the blank of {given} turned over, " +
                 (f"dia {fmt(vals['dia'])} h {fmt(vals['h'])}" if kind == "Cylinder"
                  else f"{fmt(vals['w'])} x {fmt(vals['l'])} x {fmt(vals['h'])}"))
        statement = [f"stock {blank_words(kind, vals)}", f"given from {given} up={up}"]
    elif outer is not None:
        lines = [
            f"# Proposed stock from {step_path.name} by sidecar/import_stock.py",
            "# (Phase 54 slice 5b): the topmost flat face's outer wire as the",
            "# plan outline, the solid's Z extent as the height, the top as z.",
        ]
        kind, about = "Profile", f"{len(outer)} points"
        if allowance > 0.0:
            lines.append(f"# a prism is its own blank, cut to size: the allowance "
                         f"{fmt(allowance)} is not added")
            about += ", its own blank (no allowance added)"
        statement = [f"stock Profile(h={fmt(h)}, z={fmt(bb.zmax)}"]
        statement += [f"    {fmt(x)} {fmt(y)}" for x, y in outer]
        statement.append(")")
    else:
        lines = [f"# Proposed stock from {step_path.name} by sidecar/import_stock.py:"]
        a = allowance
        tail = (f"Plus an allowance of {fmt(a)} on the sides and the top (--allowance): "
                "the top face and the rim are this blank's to cut."
                if a > 0.0 else "Add the allowance you will cut off.")
        lines += textwrap.wrap(
            f"the smallest blank the model fits, not the model's own outline -- "
            f"{why_not}. {tail}",
            width=68, initial_indent="# ", subsequent_indent="# ")
        fits = "the smallest blank the model fits" + (f" plus {fmt(a)} allowance" if a > 0.0 else "")
        round_ = plan_bounding_cylinder(faces, bb)
        if round_ is not None:
            dia, cx, cy, _ = round_
            kind, about = "Cylinder", f"dia {fmt(dia + 2 * a)} h {fmt(h + a)}, {fits}"
            statement = [f"stock Cylinder(dia={fmt(dia + 2 * a)}, h={fmt(h + a)}, x={fmt(cx)}, y={fmt(cy)}, z={fmt(bb.zmax + a)})"]
        else:
            w, l = bb.xmax - bb.xmin + 2 * a, bb.ymax - bb.ymin + 2 * a
            kind, about = "Box", f"{fmt(w)} x {fmt(l)} x {fmt(h + a)}, {fits}"
            statement = [f"stock Box(w={fmt(w)}, l={fmt(l)}, h={fmt(h + a)}, x={fmt(bb.xmin - a)}, y={fmt(bb.ymin - a)}, z={fmt(bb.zmax + a)})"]
    if n_solids > 1:
        lines.append(f"# solid S{solid_sel} of {n_solids} in the file (--solid {solid_sel})")
    if UP_ROTATIONS[up] is not None:
        lines.append(f"# imported with --up {up} -- coordinates are post-rotation")
    if origin is not None:
        lines.append(origin_note(origin, origin_at))
    return "\n".join(lines + statement) + "\n", kind, about


def up_turn(up):
    """The turn a program reads between two setups: z+ (none) or z- (turned
    over); a quarter turn is no word the grammar has."""
    return up if up in ("z+", "z-") else None


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("step", type=Path, help="input STEP file")
    ap.add_argument("-o", "--outdir", type=Path, required=True, help="where proposed_stock.part is written")
    ap.add_argument("--solid", type=int, default=None, metavar="N", help="which solid, 1-based, when the file holds several")
    ap.add_argument("--up", choices=sorted(UP_ROTATIONS), default="z+", help="which MODEL axis points up on the machine (default z+)")
    ap.add_argument("--allowance", type=float, default=0.0, metavar="MM", help="how far the blank stands outside the model, on the sides and the top (default 0: the model's exact extent)")
    ap.add_argument("--blank", metavar="WORDS", default=None, help="the blank as given, the earlier program's turned over: Cylinder(dia=, h=, x=, y=, z=) or Box(w=, l=, h=, x=, y=, z=); with --given")
    ap.add_argument("--given", metavar="PROJECT", default=None, help="the earlier program's project as `given from` will name it; with --blank")
    ap.add_argument("--origin", metavar="X,Y,Z", default=None, help="the point that becomes 0,0,0, as import_step.py takes it")
    args = ap.parse_args()
    origin = None
    if args.origin is not None:
        try:
            origin = parse_origin(args.origin)
        except ValueError as e:
            sys.exit(f"error: {e}")
        if args.blank is not None and origin != ["model"] * 3:
            sys.exit("error: --origin with --blank: the second setup keeps the origin the given blank is written in")
    if args.allowance < 0.0:
        sys.exit(f"error: --allowance {args.allowance} is below zero")
    blank = None
    if (args.blank is None) != (args.given is None):
        sys.exit("error: --blank and --given go together: the earlier program's blank turned over, and the project it came from")
    if args.blank is not None:
        if args.allowance > 0.0:
            sys.exit("error: --allowance with --blank: the given blank says how far it stands outside the model")
        if up_turn(args.up) is None:
            sys.exit(f"error: --given with --up {args.up}: a program reads only z+ (the same side) and z- (the part turned over) for the turn between two setups")
        try:
            blank = parse_blank(args.blank)
        except ValueError as e:
            sys.exit(f"error: {e}")
    try:
        text, kind, about = propose_stock(args.step, args.solid, args.up, args.allowance, blank, args.given, origin)
    except SystemExit:
        raise
    except Exception as e:  # noqa: BLE001 -- the sidecar's one error shape
        sys.exit(f"error: {args.step}: {e}")
    args.outdir.mkdir(parents=True, exist_ok=True)
    (args.outdir / "proposed_stock.part").write_text(text, encoding="utf-8", newline="\n")
    print(f"stock {kind} proposed from {args.step.name}: {about} -- proposed_stock.part written")


if __name__ == "__main__":
    main()
