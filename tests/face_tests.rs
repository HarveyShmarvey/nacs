// ---- the Face: a loop that can contain loops -----------------------------------
// Phase 28 slice 1 (`DECISIONS.md` → "Phase 28 opens: the patterns DRAFT…", fork
// 1A as revised on review: "there is no boss here… i just want a face"). The
// bounded planar face is a feature kind of its own — a height and a list of
// closed loops, the first the outer boundary (CCW), the rest inner (CW) — which
// is what STEP calls a face and what the offsetter calls a shape. `Floor`
// takes it. Authored geometry, derived kinds.

use nacs::region;
use nacs::{gcode, outline, parse, stock, template};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n";
const OUTER: &str = "    loop\n        20 20\n        80 20\n        80 60\n        20 60\n";
const INNER: &str = "    loop\n        40 30\n        40 50\n        60 50\n        60 30\n";

fn face(loops: &str) -> Result<parse::Part, String> {
    parse::parse(&format!("{HEAD}feature Deck Face(z=-2\n{loops})\nrun End(tool=T5, surface=Deck, stepover=5.0)\n"))
}

#[test]
fn test_a_face_is_a_height_and_a_loop_that_can_contain_loops() {
    let p = face(&format!("{OUTER}{INNER}")).expect("a face with one inner boundary");
    let Some(parse::Feature::Face { z, loops }) = p.features.get("Deck") else { panic!("{:?}", p.features.get("Deck")) };
    assert_eq!((*z, loops.len(), loops[0].len(), loops[1].len()), (-2.0, 2, 4, 4));
    assert_eq!(p.features["Deck"].kind(), "Face");
    // The volume, then the floor (Phase 61 slice 1): a Face takes the Clear.
    assert_eq!(template::op_lines(&p, "Deck").unwrap(), ["run Clear(tool=T?, profile=Deck, stepover=?)", "run End(tool=T?, surface=Deck, stepover=?)"]);

    // Inch input is converted at the parse boundary, loops and all.
    let inch = parse::parse(
        "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\nstock Box(w=4, l=3, h=1, x=0, y=0, z=0)\n\
         tool T5 Endmill(dia=0.375)\nfeature Deck Face(z=-0.1\n    loop\n        1 1\n        3 1\n        3 2\n        1 2\n)\n",
    )
    .unwrap();
    let Some(parse::Feature::Face { z, loops }) = inch.features.get("Deck") else { panic!() };
    assert!((*z + 2.54).abs() < 1e-9 && (loops[0][1].x - 76.2).abs() < 1e-9);
}

/// The loop law: refused by name, never normalized — a reversed loop is a
/// reversed traversal, and that is climb turned into conventional.
#[test]
fn test_the_loop_law_refuses_by_name() {
    let cw_outer = "    loop\n        20 20\n        20 60\n        80 60\n        80 20\n";
    let ccw_inner = "    loop\n        40 30\n        60 30\n        60 50\n        40 50\n";
    let e = face(cw_outer).unwrap_err();
    assert!(e.contains("'Deck'") && e.contains("OUTER boundary and winds counter-clockwise"), "{e}");
    let e = face(&format!("{OUTER}{ccw_inner}")).unwrap_err();
    assert!(e.contains("loop 2 is an INNER boundary and winds clockwise"), "{e}");
    let outside = "    loop\n        90 30\n        90 50\n        95 50\n        95 30\n";
    let e = face(&format!("{OUTER}{outside}")).unwrap_err();
    assert!(e.contains("loop 2 is not inside the outer boundary"), "{e}");
    let overlapping = "    loop\n        50 35\n        50 55\n        70 55\n        70 35\n";
    let e = face(&format!("{OUTER}{INNER}{overlapping}")).unwrap_err();
    assert!(e.contains("loops 2 and 3 overlap or nest"), "{e}");
    let e = face("        20 20\n        80 20\n        80 60\n").unwrap_err();
    assert!(e.contains("open the outer boundary with a `loop` line first"), "{e}");
    let e = face("    loop\n        20 20\n        80 20\n").unwrap_err();
    assert!(e.contains("loop 1 has 2 points"), "{e}");
    let e = parse::parse(&format!("{HEAD}feature Deck Face(z=-2, extent=3\n{OUTER})\n")).unwrap_err();
    assert!(e.contains("extent"), "a face has a height and nothing else: {e}");
}

/// Authored geometry, DERIVED kinds. Every edge of a face is open; a boss
/// standing on it is its island; an inner loop with nothing standing in it is
/// a void the path (for now) runs over, named by its place in the feature.
#[test]
fn test_a_faces_kinds_are_derived_not_authored() {
    let p = face(&format!("{OUTER}{INNER}")).unwrap();
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!((r.z, r.edges.len(), r.open_edges()), (-2.0, 4, 4));
    // 20mm across, a 10mm tool: exactly two diameters — shrunk by one there is
    // nothing left, so it is bridged (slice 2's rule; the boundary case).
    assert_eq!(r.bridged(), ["loop 2"]);
    assert!(gcode::program(&p).unwrap().contains("(--- floor: Deck with T5 ---)\n(bridged: loop 2)\n"));

    // The same inner loop with a post standing in it: the post is the island,
    // and the loop is only where its foot is.
    let with_post = parse::parse(&format!(
        "{HEAD}feature Skim Plane(z=0)\nfeature Deck Face(z=-2\n{OUTER}{INNER})\n\
         feature Post ProfileBoss(z=0, extent=2\n    40 30\n    40 50\n    60 50\n    60 30\n)\n\
         run End(tool=T5, surface=Deck, stepover=5.0)\n"
    ))
    .expect("a boss standing on a face");
    let r = region::of(&with_post, &with_post.operations[0]).unwrap();
    assert_eq!(r.feature_islands(), ["Post"]);
    assert!(r.bridged().is_empty(), "the loop is the post's foot, not a void: {:?}", r.bridged());
    assert_eq!(stock::host_of(&with_post, "Post"), Some("Deck"));

    // A wall flush with the face's edge STRADDLES its boundary — refused for a
    // pocket, whose boundary is a wall; legal for a face, whose boundary is
    // open. It is a bite out of the region.
    let flush = parse::parse(&format!(
        "{HEAD}feature Deck Face(z=-2\n{OUTER})\n\
         feature Wall ProfileBoss(z=0, extent=2\n    14 20\n    14 60\n    26 60\n    26 20\n)\n\
         run End(tool=T5, surface=Deck, stepover=5.0)\n"
    ))
    .expect("a wall across a face's open edge is legal");
    gcode::program(&flush).expect("and the face is cut up to it");
}

/// The wrench: ONE feature — its silhouette and its box opening — faced, not
/// the bar. Golden-pinned; no cutting position further than a tool radius
/// outside the wrench's outline; the removal is the swath the open edges are
/// pushed out to (one home with the emitter); the rest report clean both ways.
#[test]
fn test_wrench_golden_faces_the_wrench_not_the_bar() {
    let src = std::fs::read_to_string("wrench_offset.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("wrench_offset.cam/geometry.part").unwrap();
    let p = parse::parse_project(&src, Some(&geom)).expect("wrench_offset.cam parses");
    let g = gcode::program(&p).expect("generates under check_moves");
    assert_eq!(g.trim(), std::fs::read_to_string("tests/wrench_offset.golden.gcode").unwrap().trim());

    let Some(parse::Feature::Face { loops, .. }) = p.features.get("Top") else { panic!() };
    assert_eq!(loops.len(), 2, "the silhouette and the box opening");
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let (mut cutting, mut furthest) = (0, 0.0_f64);
    for line in g.lines().filter(|l| l.starts_with('G')) {
        for word in line.split_whitespace() {
            let value = || word[1..].parse::<f64>().expect("a number");
            match word.as_bytes()[0] {
                b'X' => x = value(),
                b'Y' => y = value(),
                b'Z' => z = value(),
                _ => {}
            }
        }
        if z <= 1e-9 && !stock::point_in_polygon(&loops[0], x, y) {
            furthest = furthest.max(stock::dist_to_polygon(&loops[0], x, y));
            cutting += 1;
        }
    }
    assert!(cutting > 50, "the family does overhang the outline ({cutting})");
    // A radius outside along every straight and convex stretch. Where the
    // outline turns INWARD — the shank meeting the ring and the head — pushing
    // out a diameter and coming back a radius is a closing, not an offset, and
    // the loop bridges the corner a little further out (5.62mm here). Over
    // scrap, inside the swath the op declares, and never near a diameter.
    assert!(furthest > 5.0 && furthest <= 6.0, "about a radius outside the wrench and no further: {furthest:.3}");

    // Slice 2: the 24mm box opening is 2.4 tool diameters — an open inner edge.
    assert_eq!(outline::region_line(&p, 0).unwrap(), "region: floor z 0.000 — 83 edges, 83 open; open onto loop 2");
    assert!(g.contains("(--- floor: Top with T5 ---)\n(open onto: loop 2)\n"));
    assert!(parse::lint(&p).iter().all(|f| !f.message.contains("declined") && !f.message.contains("stands outside")));
}

// ---- slice 2: the tool decides what is a gap -------------------------------------

fn words(g: &str) -> Vec<(f64, f64, f64)> {
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut out = Vec::new();
    for line in g.lines().filter(|l| l.starts_with('G')) {
        for word in line.split_whitespace() {
            let value = || word[1..].parse::<f64>().expect("a number");
            match word.as_bytes()[0] {
                b'X' => x = value(),
                b'Y' => y = value(),
                b'Z' => z = value(),
                _ => {}
            }
        }
        out.push((x, y, z));
    }
    out
}

/// A void shrunk by one tool diameter: anything left and it is an OPEN inner
/// edge — the family rides a radius inside its rim and never crosses it at
/// feed; nothing left and it is BRIDGED. No threshold: the same push an open
/// outer edge gets, mirrored. Between one and two diameters the tool would fit
/// and the void is bridged all the same — on purpose.
#[test]
fn test_the_tool_decides_what_is_a_gap() {
    let square = |half: f64| format!(
        "    loop\n        {a} {c}\n        {a} {d}\n        {b} {d}\n        {b} {c}\n",
        a = 50.0 - half, b = 50.0 + half, c = 40.0 - half, d = 40.0 + half
    );
    let big_outer = "    loop\n        5 5\n        95 5\n        95 75\n        5 75\n";
    // 30 across with a 10mm tool: shrunk by 10 a 10mm square is left.
    let p = face(&format!("{big_outer}{}", square(15.0))).unwrap();
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!((r.open_voids(), r.bridged()), (vec!["loop 2"], Vec::<&str>::new()));
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(open onto: loop 2)"));
    // The tool's centre never goes deeper into the void than a radius past its
    // rim — so it never crosses it.
    let mut rim_passes = 0;
    for (x, y, z) in words(&g) {
        if z < -1.0 {
            // What is left of the void, shrunk by a diameter, is x 45..55, y
            // 35..45; the tool's centre stands a radius off THAT (rounding its
            // corners), which is a radius inside the rim.
            let (dx, dy) = (((x - 50.0).abs() - 5.0).max(0.0), ((y - 40.0).abs() - 5.0).max(0.0));
            let off = dx.hypot(dy);
            assert!(off >= 5.0 - nacs::CHORD_TOL_MM - 1e-3, "({x}, {y}) is {off:.3}mm from what is left of the void");
            rim_passes += usize::from((off - 5.0).abs() < 0.02);
        }
    }
    assert!(rim_passes > 4, "the pass that overhangs the rim exists ({rim_passes})");
    // Nothing over the void is claimed removed.
    let stock::Section::Polygon { holes, .. } = &stock::remaining_after(&p, 0).removals[0].section else { panic!() };
    assert_eq!(holes.len(), 1);

    // 15 across (1.5 diameters): the tool would fit, and it is bridged anyway.
    let p = face(&format!("{big_outer}{}", square(7.5))).unwrap();
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!((r.open_voids(), r.bridged()), (Vec::<&str>::new(), vec!["loop 2"]));
    // A smaller tool opens the same void: it is the TOOL's decision.
    let small = parse::parse(&format!(
        "{HEAD}tool T3 Endmill(dia=6.0)\nfeature Deck Face(z=-2\n{big_outer}{})\nrun End(tool=T3, surface=Deck, stepover=3.0)\n",
        square(7.5)
    ))
    .unwrap();
    assert_eq!(region::of(&small, &small.operations[0]).unwrap().open_voids(), ["loop 2"]);
}

/// A declared void FEATURE is a different statement from a drawn inner loop,
/// and stays BRIDGED whatever its size: "there will be a hole here" is not "do
/// not go here". The first build of this slice judged features by the tool
/// too, and the most ordinary program there is — face a plate, then bore it —
/// stopped being a raster and refused under the ridge law. The loop is how an
/// author says not-here; and an open void bounds no passage — the ring of a
/// wrench is a land, not a web.
#[test]
fn test_a_declared_bore_stays_bridged_and_a_drawn_loop_is_how_you_say_not_here() {
    let faced = parse::parse(&format!(
        "{HEAD}feature Deck Plane(z=-2)\nfeature Hole CylinderBore(dia=30, extent=10, x=50, y=40)\n\
         run End(tool=T5, surface=Deck, stepover=6.0)\nrun Clear(tool=T5, profile=Hole, stepover=4.0)\n"
    ))
    .unwrap();
    let r = region::of(&faced, &faced.operations[0]).unwrap();
    assert_eq!((r.open_voids(), r.bridged()), (Vec::<&str>::new(), vec!["Hole"]));
    let g = gcode::program(&faced).expect("face, then bore: still a raster at 60% stepover");
    let bare = parse::parse(&format!("{HEAD}feature Deck Plane(z=-2)\nrun End(tool=T5, surface=Deck, stepover=6.0)\n")).unwrap();
    // The facing op's own motion: from its title to the last G-word before the
    // next op's tool change or the program's footer.
    let facing = |g: &str| -> Vec<String> {
        g.split("(--- floor:").nth(1).unwrap().lines().skip(1).filter(|l| *l != "(bridged: Hole)").take_while(|l| l.starts_with('G')).map(String::from).collect()
    };
    assert_eq!(facing(&g), facing(&gcode::program(&bare).unwrap()), "the bore changes nothing about the face but its one comment");

    // The wrench's ring: 8mm of metal between an open outer edge and an open
    // inner one. Nothing STANDS in the region, so nothing is a passage.
    let src = std::fs::read_to_string("wrench_offset.cam/main.part").unwrap();
    let geom = std::fs::read_to_string("wrench_offset.cam/geometry.part").unwrap();
    let w = parse::parse_project(&src, Some(&geom)).unwrap();
    assert_eq!(region::of(&w, &w.operations[0]).unwrap().narrowest, None);
}

/// Phase 29 slice 1. The stepped wrench: the box end raised to z=3, the shank
/// and open end faced at z=0, so the deck has ONE closed edge — the arc where
/// it butts the ring — and fifty-one open ones. Golden-pinned on all three
/// patterns. Before this slice every one of them emitted exit 0 and machined
/// better than a quarter of the ring away.
#[test]
fn test_the_stepped_wrench_leaves_its_raised_neighbour_standing() {
    for (dir, golden) in [
        ("wrench_step_offset.cam", "tests/wrench_step_offset.golden.gcode"),
        ("wrench_step_zigzag.cam", "tests/wrench_step_zigzag.golden.gcode"),
        ("wrench_step_oneway.cam", "tests/wrench_step_oneway.golden.gcode"),
    ] {
        let p = parse::load_project(dir).expect("the project loads");
        let g = gcode::program(&p).expect("generates under check_moves");
        assert_eq!(g.trim(), std::fs::read_to_string(golden).unwrap().trim(), "{dir}");
        // Two faces, two heights, and the ring's own top is faced first.
        assert!(
            g.find("(--- floor: BoxTop").unwrap() < g.find("(--- floor: Deck").unwrap(),
            "{dir}: the raised top is faced before the deck beside it"
        );
    }
}

/// Phase 29 slice 2. The three shapes the corpus had never had, golden-pinned,
/// and the one assertion that matters on each: no cutting position comes
/// within a tool radius of anything DECLARED to stand beside the floor.
#[test]
fn test_a_floor_leaves_every_standing_neighbour_alone() {
    let cases: [(&str, &str, &str, &[&str]); 3] = [
        ("cottage_interior_offset.cam", "tests/cottage_interior_offset.golden.gcode", "Floor", &["WallE", "WallN"]),
        ("lawn_fence_bed_offset.cam", "tests/lawn_fence_bed_offset.golden.gcode", "Lawn", &["Fence", "Bed"]),
        ("wrench_thick_offset.cam", "tests/wrench_thick_offset.golden.gcode", "Deck", &["BoxTop", "JawTop"]),
    ];
    for (dir, golden, floor, neighbours) in cases {
        let p = parse::load_project(dir).expect("the project loads");
        let g = gcode::program(&p).unwrap_or_else(|e| panic!("{dir} must emit: {e}"));
        assert_eq!(g.trim(), std::fs::read_to_string(golden).unwrap().trim(), "{dir}");
        // Only the FLOOR's own moves: a neighbour's own facing op of course
        // runs on that neighbour's outline.
        let block = g.split(&format!("(--- floor: {floor} ")).nth(1).unwrap_or_else(|| panic!("{dir}: {floor} is emitted"));
        let block = block.split("(--- ").next().unwrap();
        for name in neighbours {
            let outline: Vec<parse::Pt> = match p.features.get(*name).expect(name) {
                parse::Feature::Face { loops, .. } => loops[0].clone(),
                parse::Feature::ProfileBoss { points, .. } => points.clone(),
                other => panic!("{dir}: {name} is {other:?}"),
            };
            let (mut x, mut y) = (f64::NAN, f64::NAN);
            let mut worst = f64::INFINITY;
            for line in block.lines().filter(|l| l.starts_with("G0")) {
                for (key, slot) in [('X', &mut x), ('Y', &mut y)] {
                    if let Some(v) = line.split_whitespace().find_map(|t| t.strip_prefix(key)) {
                        *slot = v.parse().unwrap();
                    }
                }
                if x.is_nan() || y.is_nan() || line.starts_with("G00") {
                    continue;
                }
                worst = worst.min(stock::dist_to_polygon(&outline, x, y));
            }
            assert!(
                worst >= 5.0 - nacs::CHORD_TOL_MM,
                "{dir}: a cutting position came {worst} from '{name}', inside the tool radius"
            );
        }
    }
}
