#!/usr/bin/env python3
# Line-for-line port of main.rs so we can RUN it offline and prove the output.
# The Rust file is the real deliverable; this just demonstrates the same math.
import math
import os
import sys


def parse_args(s, lineno):
    open_idx = s.find('(')
    close_idx = s.rfind(')')
    if open_idx == -1 or close_idx == -1 or close_idx < open_idx:
        raise ValueError(f"line {lineno + 1}: missing '(' or ')' in: {s}")
    name = s[:open_idx].strip()
    args_str = s[open_idx + 1:close_idx]

    arg_map = {}
    parts = []
    current = []
    bracket_depth = 0
    for c in args_str:
        if c == '[':
            bracket_depth += 1
            current.append(c)
        elif c == ']':
            bracket_depth -= 1
            current.append(c)
        elif c == ',' and bracket_depth == 0:
            parts.append("".join(current).strip())
            current = []
        else:
            current.append(c)
    if current:
        parts.append("".join(current).strip())

    for part in parts:
        part = part.strip()
        if not part:
            continue
        eq_idx = part.find('=')
        if eq_idx == -1:
            raise ValueError(f"line {lineno + 1}: missing '=' in argument: {part}")
        key = part[:eq_idx].strip()
        val = part[eq_idx + 1:].strip()
        arg_map[key] = val
    return name, arg_map


def num(s, lineno):
    try:
        return float(s)
    except ValueError:
        raise ValueError(f"line {lineno + 1}: '{s}' is not a number")


def get_num(arg_map, key, lineno):
    if key not in arg_map:
        raise ValueError(f"line {lineno + 1}: missing key '{key}'")
    return num(arg_map[key], lineno)


def get_num_opt(arg_map, key, lineno):
    if key in arg_map:
        return num(arg_map[key], lineno)
    return None


def parse_into(src, p):
    reading_contour_name = None
    contour_points = []

    for lineno, raw in enumerate(src.splitlines()):
        line = raw.split('#', 1)[0].strip()
        if not line:
            continue

        if reading_contour_name is not None:
            if ')' in line:
                clean_line = line.replace(')', '').strip()
                if clean_line:
                    tok = clean_line.split()
                    if len(tok) != 2:
                        raise ValueError(f"line {lineno + 1}: contour point needs 2 numbers")
                    contour_points.append((num(tok[0], lineno), num(tok[1], lineno)))
                p["features"][reading_contour_name] = (
                    "Contour",
                    {"points": list(contour_points), "z": contour_z, "extent": contour_extent},
                )
                reading_contour_name = None
            else:
                tok = line.split()
                if len(tok) != 2:
                    raise ValueError(f"line {lineno + 1}: contour point needs 2 numbers")
                contour_points.append((num(tok[0], lineno), num(tok[1], lineno)))
            continue

        first_space = line.find(' ')
        if first_space == -1:
            keyword = line
            rest = ""
        else:
            keyword = line[:first_space].strip()
            rest = line[first_space:].strip()

        if keyword == "units":
            if rest == "mm":
                p["units_mm"] = True
            elif rest == "inch":
                p["units_mm"] = False
            else:
                raise ValueError(f"line {lineno + 1}: unknown units '{rest}' (expected 'mm' or 'inch')")
        elif keyword == "feed":
            p["global_feed"] = num(rest, lineno)
        elif keyword == "plunge":
            p["global_plunge"] = num(rest, lineno)
        elif keyword == "rapid_z":
            p["global_rapid_z"] = num(rest, lineno)
        elif keyword == "clearance_z":
            p["global_clearance_z"] = num(rest, lineno)
        elif keyword == "spindle":
            p["global_spindle"] = num(rest, lineno)
        elif keyword == "stock":
            name, arg_map = parse_args(rest, lineno)
            if name != "Box":
                raise ValueError(f"line {lineno + 1}: unknown stock type '{name}', only 'Box' supported")
            p["stock"] = {
                "w": get_num(arg_map, "w", lineno),
                "l": get_num(arg_map, "l", lineno),
                "h": get_num(arg_map, "h", lineno),
                "x": get_num_opt(arg_map, "x", lineno) or 0.0,
                "y": get_num_opt(arg_map, "y", lineno) or 0.0,
                "z": get_num_opt(arg_map, "z", lineno) or 0.0,
            }
        elif keyword == "tool":
            parts = rest.split(None, 1)
            if len(parts) < 2:
                raise ValueError(f"line {lineno + 1}: invalid tool declaration")
            t_name = parts[0]
            if not t_name.startswith('T'):
                raise ValueError(f"line {lineno + 1}: tool identifier must start with 'T'")
            tool_num = int(t_name[1:])
            tool_type, arg_map = parse_args(parts[1], lineno)
            dia = get_num(arg_map, "dia", lineno)
            p["tools"][tool_num] = {"num": tool_num, "name": tool_type, "dia": dia}
        elif keyword == "feature":
            parts = rest.split(None, 1)
            if not parts:
                raise ValueError(f"line {lineno + 1}: invalid feature declaration")
            f_name = parts[0]
            remaining = parts[1].strip() if len(parts) > 1 else ""

            if remaining.startswith("ProfilePocket(") or remaining.startswith("ProfileBoss("):
                # The polyline prismatic pair (side-of-material by variant; the
                # oracle keeps one internal tag since its only consumer is the
                # control-comp contour path). The opener carries the feature's
                # own geometry (`z`, `extent`) and the points follow on their
                # own lines, so the ')' is on a later line — synthesize one for
                # the shared arg parser.
                _, c_map = parse_args(remaining + ")", lineno)
                contour_z = get_num_opt(c_map, "z", lineno)
                contour_extent = get_num(c_map, "extent", lineno)
                reading_contour_name = f_name
                contour_points = []
            else:
                f_type, arg_map = parse_args(remaining, lineno)
                if f_type == "Plane":
                    z = get_num(arg_map, "z", lineno)
                    p["features"][f_name] = ("Plane", z)
                elif f_type == "CylinderBore":
                    dia = get_num(arg_map, "dia", lineno)
                    # `extent` is the vertical field on every extrusion feature
                    # (DECISIONS.md "The `extent` vocabulary is scoped to the
                    # extrusion features…"); the retired spelling is refused
                    # with a pointer, not reported as a missing key.
                    if "extent" not in arg_map and "depth" in arg_map:
                        raise ValueError(
                            f"line {lineno + 1}: `depth` on CylinderBore is now `extent` — "
                            "one word for vertical occupancy on every feature that is a "
                            "swept cross-section, with direction coming from the variant. "
                            "The shape parameters (Cone's `height`) are not reached"
                        )
                    extent = get_num(arg_map, "extent", lineno)
                    x = get_num(arg_map, "x", lineno)
                    y = get_num(arg_map, "y", lineno)
                    p["features"][f_name] = ("CylinderBore", {"dia": dia, "extent": extent, "x": x, "y": y})
                else:
                    raise ValueError(f"line {lineno + 1}: unknown feature type '{f_type}'")
        elif keyword == "run":
            op_name, arg_map = parse_args(rest, lineno)
            if op_name == "Copy" or (op_name == "Form" and "points" not in arg_map):
                raise ValueError(
                    f"line {lineno + 1}: operation '{op_name}' is Rust-only per DECISIONS.md "
                    "\"The parity oracle is frozen at Phase 1 scope\" — the surface and form "
                    "contact math was never ported, so it is not supported in nacs.py"
                )
            if op_name == "Clear":
                raise ValueError(
                    f"line {lineno + 1}: operation '{op_name}' depends on a geometry crate "
                    "(cavalier_contours) and is Rust-only per DECISIONS.md \"The parity "
                    "oracle is frozen at Phase 1 scope\" — not supported in nacs.py"
                )
            if op_name not in ("End", "Side", "Form"):
                raise ValueError(f"line {lineno + 1}: unknown operation '{op_name}'")
            # An End cuts spine unless pinned (Phase 35 slice 4), and spine
            # stands on the offsetting crate. This oracle ports the first
            # raster and nothing else, so the pin must say so.
            if op_name == "End" and arg_map.get("pattern") != "raster":
                raise ValueError(
                    f"line {lineno + 1}: an End without pattern=raster cuts a pattern that depends on "
                    "a geometry crate (cavalier_contours) and is Rust-only per DECISIONS.md \"The "
                    "parity oracle is frozen at Phase 1 scope\" — not supported in nacs.py"
                )
            # This oracle ports the profile wall's control-side path only (the
            # former Contour op, now Side), and never ported the
            # offsetting crate or the quadric finishing math. A stock_to_leave
            # here is baked into the emitted edge by main.rs
            # (cavalier_contours), so silently dropping it would emit a
            # plausible-looking program cutting to the wrong size — and
            # plate.cam runs this path in the parity roster, so the exposure
            # is live, not theoretical.
            if "stock_to_leave" in arg_map:
                raise ValueError(
                    f"line {lineno + 1}: stock_to_leave on '{op_name}' bakes an offset through "
                    "the geometry crate, which nacs.py never ported (DECISIONS.md \"The parity "
                    "oracle is frozen at Phase 1 scope\") — not supported in nacs.py"
                )
            tool_str = arg_map.get("tool")
            if not tool_str or not tool_str.startswith('T'):
                raise ValueError(f"line {lineno + 1}: invalid tool reference")
            tool_num = int(tool_str[1:])

            drill_points = None
            cycle = None
            peck_increment = None
            depth = None
            feature_name = ""

            if op_name == "Form":
                pts_str = arg_map.get("points")
                if not pts_str or not pts_str.startswith('[') or not pts_str.endswith(']'):
                    raise ValueError(f"line {lineno + 1}: invalid points format, expected '[...]', found '{pts_str}'")
                inside = pts_str[1:-1]
                pts = [pt.strip() for pt in inside.split(',') if pt.strip()]
                if not pts:
                    raise ValueError(f"line {lineno + 1}: Form operation requires at least one point in 'points'")
                drill_points = pts
                
                cycle_str = arg_map.get("cycle")
                if cycle_str not in ("Simple", "Peck"):
                    raise ValueError(f"line {lineno + 1}: unknown cycle type '{cycle_str}'")
                cycle = cycle_str
                peck_increment = get_num_opt(arg_map, "peck_increment", lineno)
            else:
                feature_key = "profile" if op_name == "Clear" else "surface"
                if feature_key not in arg_map:
                    raise ValueError(f"line {lineno + 1}: missing feature reference '{feature_key}'")
                feature_name = arg_map[feature_key]
            # `depth` is retired as an operation input (DECISIONS.md "Vertical
            # occupancy is `extent`…"); refused, never ignored.
            if "depth" in arg_map:
                raise ValueError(
                    f"line {lineno + 1}: `depth` is not an operation input — vertical "
                    "occupancy belongs to the feature. Author `extent` on the feature — "
                    "the Profile pair, the CylinderBore — or move the Plane's `z` to the "
                    "finished face"
                )

            stepover = get_num_opt(arg_map, "stepover", lineno)
            feed = get_num_opt(arg_map, "feed", lineno)
            plunge = get_num_opt(arg_map, "plunge", lineno)
            rapid_z = get_num_opt(arg_map, "rapid_z", lineno)
            clearance_z = get_num_opt(arg_map, "clearance_z", lineno)
            spindle = get_num_opt(arg_map, "spindle", lineno)
            comp = arg_map.get("comp")

            p["operations"].append({
                "op_type": op_name,
                "tool_num": tool_num,
                "feature_name": feature_name,
                "stepover": stepover,
                "feed": feed,
                "plunge": plunge,
                "rapid_z": rapid_z,
                "clearance_z": clearance_z,
                "spindle": spindle,
                "drill_points": drill_points,
                "cycle": cycle,
                "peck_increment": peck_increment,
                "comp": comp,
            })
        else:
            raise ValueError(f"line {lineno + 1}: unknown key '{keyword}'")

    if reading_contour_name is not None:
        raise ValueError("end of file reached while reading contour profile points")


def validate(p):
    if p["stock"]["w"] <= 0.0 or p["stock"]["l"] <= 0.0:
        raise ValueError("stock size w and l must be positive")
    # Feature-shape rules that hold regardless of which op (if any) consumes
    # them — mirrors parse::validate()'s feature loop.
    for name, item in p["features"].items():
        if item[0] == "Contour":
            extent = item[1]["extent"]
            if extent == 0.0:
                raise ValueError(
                    f"Contour feature '{name}' has extent 0: a planar contour spans no Z of "
                    "its own, so extent 0 leaves no wall to cut. Extent 0 is reserved for "
                    "geometry that carries its own Z range (a 3D wire), which this feature "
                    "is not — give it a positive extent"
                )
            if extent < 0.0:
                raise ValueError(
                    f"Contour feature '{name}' extent ({extent}) must be positive: extent is "
                    "the feature's own vertical occupancy, not a direction — the wall runs "
                    "downward from `z` by construction"
                )
    for op in p["operations"]:
        if op["tool_num"] not in p["tools"]:
            raise ValueError(f"referenced tool T{op['tool_num']} not defined")
        tool = p["tools"][op["tool_num"]]
        if tool["dia"] <= 0.0:
            raise ValueError(f"tool T{op['tool_num']} diameter must be positive")

        if op["op_type"] != "Form":
            if op["feature_name"] not in p["features"]:
                raise ValueError(f"referenced feature '{op['feature_name']}' not defined")
            f_type, f_val = p["features"][op["feature_name"]]

            if op["op_type"] == "End":
                if f_type != "Plane":
                    raise ValueError(f"End operation feature '{op['feature_name']}' must be a Plane")
                if op["stepover"] is None or op["stepover"] <= 0.0:
                    raise ValueError("End operation requires a positive stepover")
            elif op["op_type"] == "Side":
                if f_type != "Contour":
                    raise ValueError(
                        f"Side operation feature '{op['feature_name']}': only the profile-wall "
                        "control path is ported (the quadric finishing math is Rust-only per "
                        "DECISIONS.md \"The parity oracle is frozen at Phase 1 scope\")"
                    )
                if len(f_val["points"]) < 3:
                    raise ValueError(f"profile feature '{op['feature_name']}' must have at least 3 points")
                # main.rs defaults comp to "engine" (cavalier_contours offset
                # math), which this oracle never ported (DECISIONS.md "The
                # parity oracle is frozen at Phase 1 scope"). contour() below
                # only ever emits the control-side G41/G42 path, so silently
                # running an engine-mode (or unset) op here would produce
                # plausible-looking but wrong G-code — require it explicit.
                if op["comp"] != "control":
                    raise ValueError(
                        f"Contour operation comp mode '{op['comp']}' not supported in nacs.py "
                        "(only comp=control is ported; comp=engine is Rust-only)"
                    )
        else:
            drill_points = op["drill_points"]
            if not drill_points:
                raise ValueError("Form operation requires points")
            for pt_name in drill_points:
                if pt_name not in p["features"]:
                    raise ValueError(f"referenced feature '{pt_name}' not defined")
                f_type, f_val = p["features"][pt_name]
                if f_type != "CylinderBore":
                    raise ValueError(f"Form operation point '{pt_name}' must be a CylinderBore feature")
                if f_val["dia"] < tool["dia"]:
                    raise ValueError(f"tool T{op['tool_num']} diameter ({tool['dia']}) is larger than cylinder hole '{pt_name}' diameter ({f_val['dia']})")
            if op["cycle"] == "Peck":
                if op["peck_increment"] is None or op["peck_increment"] <= 0.0:
                    raise ValueError("Form operation with Peck cycle requires a positive peck_increment")


def parse(src):
    return parse_project(src, None)


def parse_project(main_src, geom_src):
    p = {
        "units_mm": True,
        "global_feed": 0.0,
        "global_plunge": 0.0,
        "global_rapid_z": 0.0,
        "global_clearance_z": 0.0,
        "global_spindle": 0.0,
        "stock": {"w": 0.0, "l": 0.0, "h": 0.0, "x": 0.0, "y": 0.0, "z": 0.0},
        "tools": {},
        "features": {},
        "operations": [],
    }
    parse_into(main_src, p)
    if geom_src is not None:
        parse_into(geom_src, p)
    if not p["units_mm"]:
        convert_to_mm(p)
    validate(p)
    return p


MM_PER_INCH = 25.4


def convert_to_mm(p):
    # The one conversion boundary: after this everything downstream assumes mm.
    # Spindle is RPM, not a length, so it is deliberately not scaled.
    s = MM_PER_INCH
    for key in ("global_feed", "global_plunge", "global_rapid_z", "global_clearance_z"):
        p[key] *= s
    for key in ("w", "l", "h", "x", "y", "z"):
        p["stock"][key] *= s
    for tool in p["tools"].values():
        tool["dia"] *= s
    for name, item in p["features"].items():
        f_type = item[0]
        if f_type == "Plane":
            p["features"][name] = ("Plane", item[1] * s)
        elif f_type == "CylinderBore":
            cyl = item[1]
            p["features"][name] = ("CylinderBore", {
                "dia": cyl["dia"] * s,
                "extent": cyl["extent"] * s,
                "x": cyl["x"] * s,
                "y": cyl["y"] * s,
            })
        else:
            c = item[1]
            p["features"][name] = ("Contour", {
                "points": [(x * s, y * s) for x, y in c["points"]],
                "z": None if c["z"] is None else c["z"] * s,
                "extent": c["extent"] * s,
            })
    for op in p["operations"]:
        for key in ("stepover", "feed", "plunge", "rapid_z", "clearance_z", "peck_increment"):
            if op.get(key) is not None:
                op[key] *= s
    p["units_mm"] = True


def f(v):
    return f"{v:.3f}"


def derive_t(tool_num):
    return tool_num


def derive_h(tool_num):
    return tool_num


def derive_d(tool_num):
    return tool_num


def derive_z_cut(z_top, extent):
    return z_top - extent


def derive_contour_top(stock_top, z):
    return stock_top if z is None else z


def floor_op(p, op):
    tool = p["tools"][op["tool_num"]]
    r = tool["dia"] / 2.0
    # The Plane IS the finished face, so facing cuts straight to it.
    f_type, z_cut = p["features"][op["feature_name"]]
    stepover = op["stepover"]

    feed = op["feed"] if op["feed"] is not None else p["global_feed"]
    plunge = op["plunge"] if op["plunge"] is not None else p["global_plunge"]
    rapid_z = op["rapid_z"] if op["rapid_z"] is not None else p["global_rapid_z"]
    clearance_z = op["clearance_z"] if op["clearance_z"] is not None else p["global_clearance_z"]

    x0 = p["stock"]["x"] - r
    x1 = p["stock"]["x"] + p["stock"]["w"] + r

    n = math.ceil(p["stock"]["l"] / stepover)
    ys = [p["stock"]["y"] + min(i * stepover, p["stock"]["l"]) for i in range(n + 1)]

    g = [
        f"(--- floor: {op['feature_name']} with T{op['tool_num']} ---)",
        "(direction: the rows run both ways, climb and conventional by turns)",
        "(plunge: in air off the blank's edge)",
        f"G00 Z{f(clearance_z)}",
        f"G00 X{f(x0)} Y{f(ys[0])}",
        f"G00 Z{f(rapid_z)}",
        f"G01 Z{f(z_cut)} F{f(plunge)}",
    ]
    at_x0 = True
    for i, y in enumerate(ys):
        target = x1 if at_x0 else x0
        if i == 0:
            g.append(f"G01 X{f(target)} F{f(feed)}")
        else:
            g.append(f"G01 Y{f(y)}")
            g.append(f"G01 X{f(target)}")
        at_x0 = not at_x0
    g.append(f"G00 Z{f(clearance_z)}")
    return g


# Phase 60 slice 1 — the wall trace follows the wall. The engine judges every
# edge of a Side's loop against the blank (region::wall_edges: on or past the
# blank, OPEN; else a wall) and traces the WALL CHAINS only, each entered one
# diameter before its start along its own line and left one diameter past its
# end. The oracle ports the box-blank case its fixture is: an edge ON the box's
# boundary is open (both ends and the midpoint within 1e-3 of it, the engine's
# `region::ON_THE_BLANK`); a loop that runs PAST the box needs the engine's
# polygon clip and is refused by name. A CW loop (a boss) holds its void
# OUTSIDE, which is still left of travel, so the same arithmetic serves both
# kinds (slice 2); the oracle's parity fixture is the pocket.
ON_THE_BLANK = 1e-3


def on_box(p, q):
    st = p["stock"]
    x0, x1 = st["x"], st["x"] + st["w"]
    y0, y1 = st["y"], st["y"] + st["l"]
    inside = x0 - ON_THE_BLANK <= q[0] <= x1 + ON_THE_BLANK and y0 - ON_THE_BLANK <= q[1] <= y1 + ON_THE_BLANK
    on_edge = min(abs(q[0] - x0), abs(q[0] - x1), abs(q[1] - y0), abs(q[1] - y1)) <= ON_THE_BLANK
    return inside and on_edge


def past_box(p, q):
    st = p["stock"]
    return (q[0] < st["x"] - ON_THE_BLANK or q[0] > st["x"] + st["w"] + ON_THE_BLANK
            or q[1] < st["y"] - ON_THE_BLANK or q[1] > st["y"] + st["l"] + ON_THE_BLANK)


def wall_chains(p, op, points):
    """The wall stretches of a loop with open edges, each a chain of consecutive
    wall edges in the loop's own direction — None when every edge is a wall
    (the closed trace, as before)."""
    n = len(points)
    for q in points:
        if past_box(p, q):
            raise ValueError(
                f"Side operation feature '{op['feature_name']}': its loop runs past the blank at "
                f"({f(q[0])}, {f(q[1])}); the engine clips the loop to the blank, which this oracle "
                "does not port — draw the loop on the blank's edge or inside it"
            )
    kinds = []
    for i in range(n):
        a, b = points[i], points[(i + 1) % n]
        mid = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0)
        kinds.append("open" if on_box(p, a) and on_box(p, b) and on_box(p, mid) else "wall")
    if "open" not in kinds:
        return None
    starts = [i for i in range(n) if kinds[i] == "wall" and kinds[(i + n - 1) % n] == "open"]
    if not starts:
        raise ValueError(
            f"Side on '{op['feature_name']}' with T{op['tool_num']}: every edge of its loop lies on or "
            "past the blank — there is no wall to trace"
        )
    chains, chain = [], []
    for k in range(n):
        i = (starts[0] + k) % n
        if kinds[i] == "wall":
            if not chain:
                chain.append(points[i])
            chain.append(points[(i + 1) % n])
        elif chain:
            chains.append(chain)
            chain = []
    if chain:
        chains.append(chain)
    return chains


def contour_chains(p, op, chains, d, z_cut, feed, plunge, rapid_z, clearance_z):
    tool = p["tools"][op["tool_num"]]
    r = tool["dia"] / 2.0
    g = [
        f"(--- side contour: {op['feature_name']} with T{op['tool_num']} ---)",
        "(direction: climb — the material on the tool's right under M03)",
    ]
    for k, chain in enumerate(chains):
        # The void is LEFT of travel for both windings (a CCW pocket holds it
        # inside, a CW boss outside), so the tool-centre path is the chain a
        # radius to the left; the lead is one diameter before
        # the compensated start along the first segment's line, the lead-out
        # one diameter past the compensated end along the last segment's.
        def unit(a, b):
            dx, dy = b[0] - a[0], b[1] - a[1]
            ln = max(math.hypot(dx, dy), 1e-9)
            return (dx / ln, dy / ln)
        t0 = unit(chain[0], chain[1])
        t1 = unit(chain[-2], chain[-1])
        c0 = (chain[0][0] - r * t0[1], chain[0][1] + r * t0[0])
        c1 = (chain[-1][0] - r * t1[1], chain[-1][1] + r * t1[0])
        lead = (c0[0] - tool["dia"] * t0[0], c0[1] - tool["dia"] * t0[1])
        out = (c1[0] + tool["dia"] * t1[0], c1[1] + tool["dia"] * t1[1])
        edges = len(chain) - 1
        g.append(f"(wall {k + 1} of {len(chains)}: {edges} edge{'' if edges == 1 else 's'}, open at both ends)")
        g.append(f"(lead: {f(tool['dia'])}mm along the path's own line, back from its start, in air)")
        g.append(f"G00 Z{f(clearance_z)}")
        g.append(f"G00 X{f(lead[0])} Y{f(lead[1])}")
        g.append(f"G00 Z{f(rapid_z)}")
        g.append(f"G01 Z{f(z_cut)} F{f(plunge)}")
        g.append(f"G01 G41 D{d} X{f(chain[0][0])} Y{f(chain[0][1])} F{f(feed)}")
        for q in chain[1:]:
            g.append(f"G01 X{f(q[0])} Y{f(q[1])}")
        g.append(f"G01 G40 X{f(out[0])} Y{f(out[1])}")
        g.append(f"G00 Z{f(clearance_z)}")
    return g


def contour(p, op):
    # Feature type/point-count were checked by validate(); no fallback needed.
    f_type, c = p["features"][op["feature_name"]]
    points = c["points"]
    tool = p["tools"][op["tool_num"]]
    d = derive_d(op["tool_num"])
    z_cut = derive_z_cut(derive_contour_top(p["stock"]["z"], c["z"]), c["extent"])

    feed = op["feed"] if op["feed"] is not None else p["global_feed"]
    plunge = op["plunge"] if op["plunge"] is not None else p["global_plunge"]
    rapid_z = op["rapid_z"] if op["rapid_z"] is not None else p["global_rapid_z"]
    clearance_z = op["clearance_z"] if op["clearance_z"] is not None else p["global_clearance_z"]

    chains = wall_chains(p, op, points)
    if chains is not None:
        return contour_chains(p, op, chains, d, z_cut, feed, plunge, rapid_z, clearance_z)

    p0, p1, pl = points[0], points[1], points[-1]
    # The lead reads the room (src/ops/profile.rs, "The lead: where a wall
    # trace plunges", Phase 47 slice 2): off the COMPENSATED start — the
    # parallel's first vertex, a radius along both of the corner's edges —
    # along the corner's VOID-SIDE bisector (left of travel for both windings,
    # since a CCW loop holds its void inside and a CW boss holds it outside),
    # one diameter where the stock model says a tool standing there is in
    # air. The oracle has no stock model and ports the full diameter alone;
    # the fixture it is held to (plate_raster.cam) leads into a whole diameter
    # of air, and says so in the one sentence the engine writes for it. The
    # parallel's vertex is the authored corner pushed a radius along each
    # edge's normal — the corner's offset at a right angle, where the engine's
    # offsetter and this arithmetic agree.
    r = tool["dia"] / 2.0
    def left_normal(a, b):
        dx, dy = b[0] - a[0], b[1] - a[1]
        ln = max(math.hypot(dx, dy), 1e-9)
        return (-dy / ln, dx / ln)
    n_out = left_normal(p0, p1)
    n_in = left_normal(pl, p0)
    c0 = (p0[0] + r * (n_out[0] + n_in[0]), p0[1] + r * (n_out[1] + n_in[1]))
    bx, by = n_out[0] + n_in[0], n_out[1] + n_in[1]
    bl = max(math.hypot(bx, by), 1e-9)
    lp = (c0[0] + bx / bl * tool["dia"], c0[1] + by / bl * tool["dia"])

    g = [
        f"(--- side contour: {op['feature_name']} with T{op['tool_num']} ---)",
        "(direction: climb — the material on the tool's right under M03)",
        f"(lead: {f(tool['dia'])}mm along the path's normal, in air)",
        f"G00 Z{f(clearance_z)}",
        f"G00 X{f(lp[0])} Y{f(lp[1])}",
        f"G00 Z{f(rapid_z)}",
        f"G01 Z{f(z_cut)} F{f(plunge)}",
        f"G01 G41 D{d} X{f(p0[0])} Y{f(p0[1])} F{f(feed)}",
    ]
    for q in points[1:]:
        g.append(f"G01 X{f(q[0])} Y{f(q[1])}")
    g.append(f"G01 X{f(p0[0])} Y{f(p0[1])}")
    g.append(f"G01 G40 X{f(lp[0])} Y{f(lp[1])}")
    g.append(f"G00 Z{f(clearance_z)}")
    return g


def drill(p, op):
    g = []
    drill_points = op["drill_points"]
    cycle = op["cycle"]
    feed = op["feed"] if op["feed"] is not None else p["global_feed"]
    plunge = op["plunge"] if op["plunge"] is not None else p["global_plunge"]
    rapid_z = op["rapid_z"] if op["rapid_z"] is not None else p["global_rapid_z"]
    clearance_z = op["clearance_z"] if op["clearance_z"] is not None else p["global_clearance_z"]

    g.append(f"(--- form points: {', '.join(drill_points)} with T{op['tool_num']} ---)")
    g.append(f"G00 Z{f(clearance_z)}")

    for pt_name in drill_points:
        f_type, f_val = p["features"][pt_name]
        # A Form at points is the one consumer that spends the bore's `extent` as a
        # travel rather than reading a wall off it (see src/ops/drill.rs).
        hx, hy, h_depth = f_val["x"], f_val["y"], f_val["extent"]
        z_cut = derive_z_cut(p["stock"]["z"], h_depth)

        g.append(f"G00 X{f(hx)} Y{f(hy)}")
        g.append(f"G00 Z{f(rapid_z)}")

        if cycle == "Simple":
            g.append(f"G01 Z{f(z_cut)} F{f(plunge)}")
            g.append(f"G00 Z{f(clearance_z)}")
        elif cycle == "Peck":
            peck_increment = op["peck_increment"]
            current_peck_z = p["stock"]["z"]
            peck_idx = 0
            while current_peck_z > z_cut:
                next_peck_z = max(current_peck_z - peck_increment, z_cut)
                if peck_idx > 0:
                    entry_z = min(current_peck_z + 0.5, rapid_z)
                    g.append(f"G00 Z{f(entry_z)}")
                g.append(f"G01 Z{f(next_peck_z)} F{f(plunge)}")
                g.append(f"G00 Z{f(rapid_z)}")
                current_peck_z = next_peck_z
                peck_idx += 1
            g.append(f"G00 Z{f(clearance_z)}")
    return g


def program(p):
    g = [
        "%",
        "(nacs — nacs output)",
    ]

    for t_num in sorted(p["tools"].keys()):
        t = p["tools"][t_num]
        g.append(f"(tool T{derive_t(t_num)} dia {f(t['dia'])} -> comp register D{derive_d(t_num)})")

    # Always G21: values are mm by here (inch inputs converted at the parse
    # boundary — DECISIONS.md "Internal units are mm, always").
    g.append("G21")
    g.append("G90 G54 G17")

    active_tool = None
    active_spindle = 0.0
    last_clearance_z = p["global_clearance_z"]

    for op in p["operations"]:
        tool_changed = active_tool != op["tool_num"]
        spindle_speed = op["spindle"] if op["spindle"] is not None else p["global_spindle"]
        spindle_changed = active_spindle != spindle_speed
        clearance_z = op["clearance_z"] if op["clearance_z"] is not None else p["global_clearance_z"]

        if tool_changed:
            if active_tool is not None:
                g.append("M09")
                g.append("M05")
                g.append(f"G00 Z{f(last_clearance_z)}")
            g.append(f"M06 T{derive_t(op['tool_num'])}")
            g.append(f"M03 S{f(spindle_speed)}")
            g.append("M08")
            # Length comp for the new tool, applied on the move up to clearance
            # so it's active before any cutting move (the safety lint enforces this).
            g.append(f"G00 G43 H{derive_h(op['tool_num'])} Z{f(clearance_z)}")
            active_tool = op["tool_num"]
            active_spindle = spindle_speed
        elif spindle_changed:
            g.append(f"S{f(spindle_speed)}")
            active_spindle = spindle_speed

        last_clearance_z = clearance_z

        if op["op_type"] == "End":
            g.extend(floor_op(p, op))
        elif op["op_type"] == "Side":
            g.extend(contour(p, op))
        elif op["op_type"] == "Form":
            g.extend(drill(p, op))

    g.append("M09")
    g.append("M05")
    g.append(f"G00 Z{f(last_clearance_z)}")
    g.append("M30")
    g.append("%")
    return "\n".join(g)


if __name__ == "__main__":
    path_arg = sys.argv[1]
    if os.path.isdir(path_arg):
        main_path = os.path.join(path_arg, "main.part")
        geom_path = os.path.join(path_arg, "geometry.part")
        with open(main_path, "r") as f_in:
            main_src = f_in.read()
        geom_src = None
        if os.path.exists(geom_path):
            with open(geom_path, "r") as f_in:
                geom_src = f_in.read()
        p = parse_project(main_src, geom_src)
    else:
        with open(path_arg, "r") as f_in:
            src = f_in.read()
        p = parse(src)

    print(program(p))
