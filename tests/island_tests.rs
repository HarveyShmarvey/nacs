// ---- islands, holes, and the overlaps that mean nothing ----------------------
// Phase 26 (`DECISIONS.md` → "Phase 10 lane D — overlap semantics…" for the
// ruling, "Phase 26 opens: the island DRAFT…" for the build). Slice 1 is the
// predicate and the refusals: `stock::plan_nesting` / `islands_of` /
// `host_of`, and `validate` naming both features of every overlap the
// vocabulary cannot mean.

use nacs::gcode;
use nacs::parse::{self, Pt};
use nacs::region;
use nacs::stock::{self, Nesting};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n";
const POCKET: &str = "feature Pocket ProfilePocket(extent=5.0\n    20 20\n    80 20\n    80 80\n    20 80\n)\n";

fn program(lines: &str) -> Result<parse::Part, String> {
    parse::parse(&format!("{HEAD}{lines}"))
}

fn pts(v: &[(f64, f64)]) -> Vec<Pt> {
    v.iter().map(|&(x, y)| Pt { x, y }).collect()
}

/// The footprint, not its box: an L-shaped boss sits wholly inside an L-shaped
/// pocket while its bounding box pokes out through the pocket's notch — the
/// case lane C's bbox test got wrong and lane D's ruling named.
#[test]
fn test_nesting_reads_the_footprint_not_its_box() {
    let pocket = pts(&[(0.0, 0.0), (60.0, 0.0), (60.0, 20.0), (20.0, 20.0), (20.0, 60.0), (0.0, 60.0)]);
    let boss = pts(&[(5.0, 5.0), (55.0, 5.0), (55.0, 15.0), (15.0, 15.0), (15.0, 55.0), (5.0, 55.0)]);
    // The boss's box corner (55, 55) is outside the pocket.
    assert!(!stock::point_in_polygon(&pocket, 55.0, 55.0));
    assert_eq!(stock::plan_nesting(&boss, &pocket), Nesting::Inside);
    assert_eq!(stock::plan_nesting(&pocket, &boss), Nesting::Around);
    // Either winding: side-of-material is not this test's business.
    let cw: Vec<Pt> = boss.iter().rev().copied().collect();
    assert_eq!(stock::plan_nesting(&cw, &pocket), Nesting::Inside);

    let apart = pts(&[(100.0, 100.0), (110.0, 100.0), (110.0, 110.0), (100.0, 110.0)]);
    assert_eq!(stock::plan_nesting(&apart, &pocket), Nesting::Apart);
    let across = pts(&[(50.0, 10.0), (70.0, 10.0), (70.0, 15.0), (50.0, 15.0)]);
    assert_eq!(stock::plan_nesting(&across, &pocket), Nesting::Straddles);
}

/// ROADMAP item 1, slice 6 (the real part): a boss whose corner is notched
/// by a bore, the notch's arc concentric with the bore's rim, and a bore
/// tangent to a boss's wall. Both are two chordings crossing back and forth
/// within the chord tolerance, and both are OUTSIDE each other -- a touch,
/// not a straddle. A bore cutting into the boss by more than the chording
/// still straddles.
#[test]
fn test_a_bore_notching_or_touching_a_boss_is_outside_it() {
    let circle = |cx: f64, cy: f64, r: f64, n: usize| -> Vec<Pt> {
        (0..n).map(|k| {
            let a = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            Pt { x: cx + r * a.cos(), y: cy + r * a.sin() }
        }).collect()
    };
    let bore = circle(0.0, 0.0, 15.0, 97);
    // The notched boss: a square whose corner nearest the bore follows the
    // bore's own circle, chorded at a different pitch.
    let mut notched = vec![Pt { x: 40.0, y: 10.0 }, Pt { x: 40.0, y: 40.0 }, Pt { x: 10.0, y: 40.0 }];
    let (a0, a1) = ((10.0f64 / 15.0).acos(), (10.0f64 / 15.0).asin());
    let steps = 24;
    for k in 0..=steps {
        let a = a0 + (a1 - a0) * k as f64 / steps as f64;
        notched.push(Pt { x: 15.0 * a.cos(), y: 15.0 * a.sin() });
    }
    assert_eq!(stock::plan_nesting(&bore, &notched), Nesting::Apart, "the notch runs along the bore's rim");
    assert_eq!(stock::plan_nesting(&notched, &bore), Nesting::Apart);
    // Tangent: the boss's wall at x = 15 kisses the bore.
    let tangent = pts(&[(15.0, -10.0), (40.0, -10.0), (40.0, 10.0), (15.0, 10.0)]);
    assert_eq!(stock::plan_nesting(&bore, &tangent), Nesting::Apart, "a wall tangent to the rim touches it");
    let kissing = pts(&[(14.996, -10.0), (40.0, -10.0), (40.0, 10.0), (14.996, 10.0)]);
    assert_eq!(stock::plan_nesting(&bore, &kissing), Nesting::Apart, "inside the chord tolerance is still a touch");
    // Cut into by more than the chording: a straddle, as before.
    let across = pts(&[(14.0, -10.0), (40.0, -10.0), (40.0, 10.0), (14.0, 10.0)]);
    assert_eq!(stock::plan_nesting(&bore, &across), Nesting::Straddles);

    // And the same in a program: the level, the notched head standing to it,
    // the bore beside it -- the containment judge lets it through, and the
    // Clear on the level derives the head as its island.
    let loop_text: String = notched.iter().rev().map(|p| format!("    {:.3} {:.3}\n", p.x, p.y)).collect();
    let part = program(&format!(
        "feature Floor Plane(z=-5.0)\nfeature Head ProfileBoss(z=0.0, extent=5.0\n{loop_text})\n\
         feature Centre CylinderBore(dia=30.0, extent=5.0, x=0.0, y=0.0)\n\
         run Clear(tool=T5, profile=Floor, stepover=3.0)\n"
    )).expect("a bore notching a head is outside it");
    let r = region::of(&part, &part.operations[0]).expect("the Clear on the level works a region");
    assert_eq!(r.feature_islands(), vec!["Head"]);
}

/// The island is DERIVED: no `islands=` word exists, and the shipped fixture
/// that draws a post in a pocket says so by containment alone.
#[test]
fn test_the_shipped_post_is_an_island_of_its_pocket() {
    let p = parse::parse(&std::fs::read_to_string("pocket_post.cam/main.part").unwrap()).expect("pocket_post.cam parses exactly as before");
    let islands: Vec<&str> = stock::islands_of(&p, "PocketProfile").into_iter().map(|(n, _)| n).collect();
    assert_eq!(islands, ["Post"]);
    assert_eq!(stock::host_of(&p, "Post"), Some("PocketProfile"));
    assert_eq!(stock::host_of(&p, "PocketProfile"), None);
    assert!(stock::islands_of(&p, "Post").is_empty());
}

#[test]
fn test_a_boss_across_a_pocket_wall_refuses_with_both_names() {
    let e = program(&format!("{POCKET}feature Post CylinderBoss(dia=12, extent=5, x=80, y=50, z=-5)\n")).unwrap_err();
    assert_eq!(
        e,
        "'Post' straddles 'Pocket''s wall — a feature is inside another's footprint (an island, or a hole) or outside it; neither has a meaning in this vocabulary"
    );
    // Beside the pocket, or under its floor: not this question at all.
    program(&format!("{POCKET}feature Post CylinderBoss(dia=12, extent=5, x=92, y=50, z=-5)\n")).expect("apart in plan");
    program(&format!("{POCKET}feature Post CylinderBoss(dia=12, extent=5, x=50, y=50, z=-10)\n")).expect("apart in height");
}

/// Fork 5b, order-free. A boss shorter than the cut around it is a terrace —
/// unless some OTHER declaration says the material above it is gone.
#[test]
fn test_a_short_boss_is_a_terrace_unless_the_column_above_it_is_declared_removed() {
    let short = "feature Post CylinderBoss(dia=12, extent=3, x=50, y=50, z=-5)\n";
    let e = program(&format!("{POCKET}{short}")).unwrap_err();
    assert!(e.contains("'Post'") && e.contains("'Pocket'") && e.contains("terrace"), "{e}");
    assert!(e.contains("z -2") && e.contains("z 0"), "the two heights are in the sentence: {e}");

    // A skim plane at the boss's own top declares the column above it removed.
    // Read from the DECLARATION — no op runs on the plane, and no op has to.
    let p = program(&format!("{POCKET}{short}feature Skim Plane(z=-2)\n")).expect("the skim declares the air above the post");
    assert_eq!(stock::host_of(&p, "Post"), Some("Pocket"));
}

#[test]
fn test_a_floating_boss_and_an_over_tall_one_refuse_by_name() {
    let e = program(&format!("{POCKET}feature Post CylinderBoss(dia=12, extent=3, x=50, y=50, z=-3)\n")).unwrap_err();
    assert!(e.contains("'Post'") && e.contains("'Pocket'") && e.contains("undercut"), "{e}");

    let sunk = "feature Pocket ProfilePocket(z=-5, extent=5.0\n    20 20\n    80 20\n    80 80\n    20 80\n)\n";
    let e = program(&format!("{sunk}feature Post CylinderBoss(dia=12, extent=10, x=50, y=50, z=-10)\n")).unwrap_err();
    assert!(e.contains("'Post'") && e.contains("'Pocket'") && e.contains("taller"), "{e}");
}

/// Fork 7: the kind is in the sentence, on both sides.
#[test]
fn test_the_kinds_that_cannot_stand_or_host_yet_say_so() {
    let e = program(&format!("{POCKET}feature Dome Sphere(dia=20, x=50, y=50, z=-5)\n")).unwrap_err();
    assert!(e.contains("'Dome' is a Sphere") && e.contains("'Pocket'"), "{e}");

    let e = program(
        "feature Bore CylinderBore(dia=60, extent=5, x=50, y=50)\nfeature Post CylinderBoss(dia=12, extent=5, x=50, y=50, z=-5)\n",
    )
    .unwrap_err();
    assert!(e.contains("'Post'") && e.contains("'Bore', a CylinderBore"), "{e}");

    // The void-side half is unchanged: a hole down a post is the post's own.
    program("feature Post CylinderBoss(dia=40, extent=5, x=50, y=50, z=-5)\nfeature Hole CylinderBore(dia=10, extent=5, x=50, y=50)\n")
        .expect("a hollow post is a part");
}

/// Fork 6: an island's roughing is its host's Clear.
#[test]
fn test_a_clear_aimed_at_an_island_is_sent_to_its_host() {
    let e = program(&format!(
        "{POCKET}feature Post CylinderBoss(dia=12, extent=5, x=50, y=50, z=-5)\nrun Clear(tool=T5, profile=Post, stepover=4.0, stepdown=2.5)\n"
    ))
    .unwrap_err();
    assert_eq!(e, "'Post' is an island of 'Pocket' — that feature's own clearing op roughs around it; schedule the Clear on 'Pocket'");
}

/// A face that MEETS a boss's top is every boss fixture in the corpus: no
/// island, the raster. A face BELOW it goes around the boss (slice 3 — slice 1
/// refused this program in the interim, and a refusal lifted can only shrink
/// the set of programs rejected).
#[test]
fn test_a_face_below_a_boss_top_goes_around_it() {
    let boss = "feature Post CylinderBoss(dia=12, extent=5, x=50, y=50, z=-5)\n";
    let meets = program(&format!("{boss}feature Top Plane(z=0)\nrun End(tool=T5, surface=Top, stepover=5.0)\n")).expect("the face meets the top");
    assert!(stock::islands_of(&meets, "Top").is_empty());
    assert!(!gcode::program(&meets).unwrap().contains("(islands:"), "nothing stands through it");

    let around = program(&format!("{boss}feature Deck Plane(z=-5)\nrun End(tool=T5, surface=Deck, stepover=5.0, pattern=offset)\n")).expect("the boss is the face's island");
    assert_eq!(stock::host_of(&around, "Post"), Some("Deck"));
    let g = gcode::program(&around).expect("generates");
    assert!(g.contains("(--- floor: Deck with T5 ---)\n(islands: Post)\n"), "{}", &g[..400.min(g.len())]);
    // Entered from air: the first loop rides one radius outside the blank and
    // takes a plain plunge to depth, no ramp.
    assert!(g.contains("G00 X-5.000 Y-5.000\nG01 Z-5.000 F300.000\n"), "{}", &g[..600.min(g.len())]);

    // The removal is the blank's footprint minus the boss.
    let removals = stock::remaining_after(&around, 0).removals;
    let stock::Section::Polygon { holes, .. } = &removals[0].section else { panic!("{:?}", removals[0].section) };
    assert_eq!(holes.len(), 1);
}

/// The ridge law, measured on the face (`face_boss_offset.cam` forgot 243.9 mm³ at an
/// 8mm stepover on a 12mm tool): a family that goes around islands takes a
/// stepover of at most the tool's radius, pocket and face alike.
#[test]
fn test_a_family_around_islands_takes_at_most_a_radius_of_stepover() {
    let boss = "feature Post CylinderBoss(dia=12, extent=5, x=50, y=50, z=-5)\n";
    let wide = program(&format!("{boss}feature Deck Plane(z=-5)\nrun End(tool=T5, surface=Deck, stepover=6.0, pattern=offset)\n")).unwrap();
    let e = gcode::program(&wide).unwrap_err();
    assert!(e.contains("'Deck' goes around islands") && e.contains("stepover (6.000) is more than T5's radius (5.000)"), "{e}");
    // The same face with nothing standing through it keeps the stepover it
    // always could: the law is about ridges, and a bare face has none.
    program("feature Deck Plane(z=-5)\nrun End(tool=T5, surface=Deck, stepover=8.0)\n").map(|p| gcode::program(&p).unwrap()).expect("a bare face");

    let pocket = program(&format!("{POCKET}{boss}run Clear(tool=T5, profile=Pocket, stepover=6.0)\n")).unwrap();
    let e = gcode::program(&pocket).unwrap_err();
    assert!(e.contains("pocket feature 'Pocket' goes around islands") && e.contains("reduce stepover to 5.000 or less"), "{e}");
}

/// Fork 5b on the face, order-free: the shipped fixture's bosses top out at
/// z=-1 under a stock whose top is z=0, and they are the Deck's islands only
/// because the Skim plane DECLARES the millimetre above them removed.
#[test]
fn test_the_shipped_face_reads_the_skim_as_the_air_above_its_bosses() {
    let src = std::fs::read_to_string("face_boss_offset.cam/main.part").unwrap();
    let p = parse::parse(&src).expect("face_boss_offset.cam parses");
    let names: Vec<&str> = stock::islands_of(&p, "Deck").into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Lobe", "Post"]);
    assert!(stock::islands_of(&p, "Skim").is_empty(), "the skim meets their tops");

    let e = parse::parse(&src.replace("feature Skim Plane(z=-1)", "feature Skim Plane(z=0)")).unwrap_err();
    assert!(e.contains("terrace") && e.contains("'Deck'"), "{e}");
    // Not run order: the skim scheduled AFTER the deck is the same part.
    let swapped = src
        .replace("run End(tool=T2, surface=Skim, stepover=8.0, pattern=raster)\n", "")
        .replace("run End(tool=T2, surface=Deck, stepover=6.0, pattern=offset)\n", "run End(tool=T2, surface=Deck, stepover=6.0, pattern=offset)\nrun End(tool=T2, surface=Skim, stepover=8.0, pattern=raster)\n");
    assert_ne!(swapped, src);
    let q = parse::parse(&swapped).expect("the declaration, not the program");
    assert_eq!(stock::islands_of(&q, "Deck").len(), 2);
}

// ---- Phase 32, after the close: a neighbour thinner than the push -----------

/// The walled slot's inner wall (7.5mm) and the thin wall (9mm) are both
/// thinner than the 10mm tool's push, so the swath pushed out from the face
/// closes all the way round them: each stands INSIDE the swath. Until then
/// the clip that takes a neighbour out of the swath made a hole there and the
/// op refused ("the region folds back on itself"). Now the neighbour is an
/// island of the swath — and no pass of any pattern, closing pass included,
/// may bring the tool's edge into it at depth (to within the one chord
/// tolerance: the loop round its corner is an arc, flattened).
#[test]
fn test_a_neighbour_thinner_than_the_push_is_an_island_of_the_swath() {
    for (dir, face, wall, z_cut) in [("walled_slot.cam", "Rail", "Inner", -4.0), ("thin_wall.cam", "Floor", "Wall", -4.0)] {
        let src = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
        for pattern in ["offset", "zigzag", "oneway", "spine"] {
            let run = src.lines().find(|l| l.starts_with("run ")).unwrap();
            let pinned = run.split(", pattern=").next().unwrap().to_string() + &format!(", pattern={pattern})");
            let part = parse::parse(&src.replace(run, &pinned)).unwrap_or_else(|e| panic!("{dir} {pattern}: {e}"));
            assert!(part.features.contains_key(face));
            let g = gcode::program(&part).unwrap_or_else(|e| panic!("{dir} {pattern} must emit: {e}"));
            let parse::Feature::ProfileBoss { points, .. } = &part.features[wall] else { panic!("{wall} is a ProfileBoss") };
            let cuts = cuts_at(&g, z_cut);
            assert!(!cuts.is_empty(), "{dir} {pattern}: cuts at depth");
            for (a, b) in cuts {
                let n = ((b.x - a.x).hypot(b.y - a.y) / 0.25).ceil().max(1.0) as usize;
                for i in 0..=n {
                    let t = i as f64 / n as f64;
                    let (x, y) = (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                    let clear = if stock::point_in_polygon(points, x, y) { 0.0 } else { stock::dist_to_polygon(points, x, y) };
                    assert!(clear >= 5.0 - nacs::CHORD_TOL_MM - 1e-3, "{dir} {pattern}: the tool centre passes {clear:.3}mm from '{wall}' at ({x:.3}, {y:.3})");
                }
            }
        }
    }
}

/// Feed moves at one depth, as (from, to) in plan.
fn cuts_at(g: &str, z_cut: f64) -> Vec<(Pt, Pt)> {
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut out = Vec::new();
    for line in g.lines().filter(|l| l.starts_with("G0")) {
        let (mut nx, mut ny, mut nz) = (x, y, z);
        for w in line.split_whitespace() {
            match w.as_bytes()[0] {
                b'X' => nx = w[1..].parse().unwrap_or(nx),
                b'Y' => ny = w[1..].parse().unwrap_or(ny),
                b'Z' => nz = w[1..].parse().unwrap_or(nz),
                _ => {}
            }
        }
        if line.starts_with("G01") && x.is_finite() && (z - z_cut).abs() < 1e-6 && (nz - z_cut).abs() < 1e-6 {
            out.push((Pt { x, y }, Pt { x: nx, y: ny }));
        }
        (x, y, z) = (nx, ny, nz);
    }
    out
}

/// The shipped fixtures, byte for byte.
#[test]
fn test_the_thin_neighbour_fixtures_are_golden() {
    for dir in ["walled_slot.cam", "thin_wall.cam"] {
        let src = std::fs::read_to_string(format!("{dir}/main.part")).unwrap();
        let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new(dir))).unwrap();
        let g = gcode::program(&part).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let golden = std::fs::read_to_string(format!("tests/{}.golden.gcode", dir.trim_end_matches(".cam"))).unwrap();
        assert_eq!(g.trim(), golden.trim(), "{dir}");
    }
}
