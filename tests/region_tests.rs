// ---- the region: a floor, and what bounds it ---------------------------------
// Phase 27 (`DECISIONS.md` → "Phase 27 opens: the region DRAFT…"). Slice 1 is
// the region as TRUTH: `region::of` derives the floor an op works — its outer
// boundary clipped to the blank, a kind per edge (wall / open), what it goes
// around (islands), what it runs over (bridged), and the narrowest place the
// tool must pass — and the outline, the program's comments, the containment
// lint, `check_moves` and the removal all read it. No path moves in slice 1.

use nacs::region::{self, EdgeKind};
use nacs::{gcode, outline, parse, stock};

const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n";

fn program(lines: &str) -> parse::Part {
    parse::parse(&format!("{HEAD}{lines}")).unwrap_or_else(|e| panic!("must parse: {e}"))
}

/// A step, drawn the obvious way: a pocket whose outline runs past the blank
/// on three sides. Before this slice it had three wrong voices — the
/// containment lint said the void "stands outside the stock", `check_moves`
/// DECLINED on the op, and the removal claimed air beyond the blank.
const STEP: &str = "feature Step ProfilePocket(extent=5.0\n    60 -12\n    112 -12\n    112 92\n    60 92\n)\n\
run Clear(tool=T5, profile=Step, stepover=4.0)\n";

#[test]
fn test_a_step_is_a_region_with_three_open_edges_and_one_wall() {
    let p = program(STEP);
    let r = region::of(&p, &p.operations[0]).expect("a Clear on a pocket works a floor");
    assert_eq!(r.z, -5.0);
    // Clipped to the blank: x 60..100, y 0..80 — however far past it was drawn.
    let (xs, ys): (Vec<f64>, Vec<f64>) = r.outer.iter().map(|q| (q.x, q.y)).unzip();
    let span = |v: &[f64]| (v.iter().cloned().fold(f64::INFINITY, f64::min), v.iter().cloned().fold(f64::NEG_INFINITY, f64::max));
    assert_eq!((span(&xs), span(&ys)), ((60.0, 100.0), (0.0, 80.0)));
    assert_eq!(r.edges.len(), 4);
    assert_eq!(r.open_edges(), 3);
    // The one wall is the step's own face, at x = 60.
    let wall = r.edges.iter().position(|k| *k == EdgeKind::Wall).unwrap();
    let (a, b) = (r.outer[wall], r.outer[(wall + 1) % 4]);
    assert!((a.x - 60.0).abs() < 1e-9 && (b.x - 60.0).abs() < 1e-9, "{a:?} {b:?}");

    // Drawn 12mm past the blank or 100mm past it: the same region.
    let far = program(&STEP.replace("112", "200").replace("-12", "-100").replace("92", "180"));
    let rf = region::of(&far, &far.operations[0]).unwrap();
    assert_eq!(rf.edges, r.edges);
    for (a, b) in rf.outer.iter().zip(&r.outer) {
        assert!((a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9, "{a:?} vs {b:?}");
    }
}

#[test]
fn test_a_void_past_the_blank_is_air_declared_air_and_a_boss_past_it_still_speaks() {
    let p = program(STEP);
    let said: Vec<String> = parse::lint(&p).into_iter().map(|f| f.message).collect();
    assert!(!said.iter().any(|m| m.contains("stands outside the stock")), "{said:?}");
    assert!(!said.iter().any(|m| m.contains("check_moves declined")), "the entry restriction is ON for the step: {said:?}");
    assert!(stock::uncontained(&p).is_empty());
    gcode::program(&p).expect("and the program generates under check_moves");

    // The removal is what the blank had: the clipped outline, not air beyond it.
    let removal = &stock::remaining_after(&p, 0).removals[0];
    let stock::Section::Polygon { points, .. } = &removal.section else { panic!("{:?}", removal.section) };
    assert!(points.iter().all(|q| q.x <= 100.0 + 1e-9 && q.y >= -1e-9 && q.y <= 80.0 + 1e-9), "{points:?}");
    assert!(outline::removal_line(&p, 0).starts_with("removes: polygon (4 pts)"), "{}", outline::removal_line(&p, 0));

    // A BOSS past the blank is material the stock does not have.
    let boss = program("feature Pad ProfileBoss(extent=5.0\n    60 -12\n    60 92\n    112 92\n    112 -12\n)\n");
    let said: Vec<String> = parse::lint(&boss).into_iter().map(|f| f.message).collect();
    assert!(said.iter().any(|m| m.contains("'Pad' (ProfileBoss) stands outside the stock")), "{said:?}");
    // And a void through the stock's BOTTOM is a different mistake, still said.
    let deep = program("feature Through ProfilePocket(extent=25.0\n    60 -12\n    112 -12\n    112 92\n    60 92\n)\n");
    let said: Vec<String> = parse::lint(&deep).into_iter().map(|f| f.message).collect();
    assert!(said.iter().any(|m| m.contains("below the stock's bottom") && !m.contains("past the stock's")), "{said:?}");
}

/// BRIDGED is what the engine always did — the raster faces straight over a
/// bore, a pocket clears straight over a hole in its floor — and now it says
/// so. Cut or not, no order consulted: uncut it is floor, cut it is air time.
#[test]
fn test_a_void_in_a_floor_is_bridged_and_the_program_says_so() {
    let faced = program(
        "feature Deck Plane(z=-2)\nfeature Hole CylinderBore(dia=12, extent=10, x=50, y=40)\n\
         run End(tool=T5, surface=Deck, stepover=6.0)\n",
    );
    let r = region::of(&faced, &faced.operations[0]).unwrap();
    assert_eq!(r.bridged(), ["Hole"]);
    assert_eq!(r.open_edges(), 4);
    let g = gcode::program(&faced).unwrap();
    assert!(g.contains("(--- floor: Deck with T5 ---)\n(bridged: Hole)\n"), "{}", &g[..400]);
    // The path itself is the raster it always was: the note is the only line.
    let bare = program("feature Deck Plane(z=-2)\nrun End(tool=T5, surface=Deck, stepover=6.0)\n");
    assert_eq!(g.replace("(bridged: Hole)\n", ""), gcode::program(&bare).unwrap());

    let pocket = program(
        "feature Cavity ProfilePocket(extent=5.0\n    20 20\n    80 20\n    80 60\n    20 60\n)\n\
         feature Hole CylinderBore(dia=8, extent=12, x=50, y=40)\n\
         feature Shallow CylinderBore(dia=8, extent=3, x=30, y=30)\n\
         feature Outside CylinderBore(dia=8, extent=12, x=90, y=70)\n\
         run Clear(tool=T5, profile=Cavity, stepover=4.0)\n",
    );
    let r = region::of(&pocket, &pocket.operations[0]).unwrap();
    // `Shallow` ends above the floor — air the pocket already declares;
    // `Outside` is not in this floor at all.
    assert_eq!(r.bridged(), ["Hole"]);
    assert!(gcode::program(&pocket).unwrap().contains("(--- pocket clear: Cavity with T5 ---)\n(bridged: Hole)\n"));
}

/// The strip number (fork 7): the narrowest place the tool must pass, between
/// which two boundaries, in millimetres and tool diameters — the thing found
/// by trying tools, stated. It is the web refusal's own number (one home), and
/// an OPEN edge bounds nothing: there is air to swing into.
#[test]
fn test_the_narrowest_passage_is_a_number_and_open_edges_bound_nothing() {
    let src = std::fs::read_to_string("pocket_islands.cam/main.part").unwrap();
    let p = parse::parse(&src).unwrap();
    let r = region::of(&p, &p.operations[0]).unwrap();
    let n = r.narrowest.as_ref().expect("islands have passages");
    // PostB (r 7 at y 40) stands 13mm off the pocket's y=20 wall.
    assert_eq!((n.between.as_str(), n.and.as_str()), ("'PostB'", "'Cavity''s wall"));
    assert!((n.width - 13.0).abs() < 1e-9, "{}", n.width);
    let line = outline::region_line(&p, 0).unwrap();
    assert_eq!(
        line,
        "region: floor z -6.000 — 4 edges, 0 open; around Pad, PostA, PostB; narrowest passage 13.000mm (1.30 tool diameters) between 'PostB' and 'Cavity''s wall"
    );
    // A bare face says nothing; the notch's Clear says its two open edges
    // (Phase 53 slice 1); a Side works no region.
    let plate = parse::parse_project(&std::fs::read_to_string("plate.cam/main.part").unwrap(), std::fs::read_to_string("plate.cam/geometry.part").ok().as_deref()).unwrap();
    assert!(outline::region_line(&plate, 0).is_none() && outline::region_line(&plate, 2).is_none());
    assert_eq!(outline::region_line(&plate, 1).as_deref(), Some("region: floor z -20.000 — 4 edges, 2 open; through the blank"));
    // (…through the blank: the notch's floor is the blank's bottom, derived — Phase 61 slice 3.)

    // A boss 3mm from the blank's edge, through a face: the edge is OPEN, so
    // it is no passage at all.
    let faced = program(
        "feature Deck Plane(z=-5)\nfeature Post CylinderBoss(dia=12, extent=5, x=9, y=40, z=-5)\n\
         run End(tool=T5, surface=Deck, stepover=5.0)\n",
    );
    let r = region::of(&faced, &faced.operations[0]).unwrap();
    assert_eq!(r.islands().len(), 1);
    assert_eq!(r.narrowest, None);
    gcode::program(&faced).expect("and it faces: the tool passes outside the blank");
}

/// The region and Phase 26's derivation agree on every shipped fixture that
/// has an island — one fact, two readers.
#[test]
fn test_the_region_and_islands_of_agree_on_the_corpus() {
    let mut hosts = 0;
    for entry in std::fs::read_dir(".").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path.join("main.part")) else { continue };
        let geom = std::fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(p) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        for op in &p.operations {
            let Some(r) = region::of(&p, op) else { continue };
            let derived: Vec<&str> = stock::islands_of(&p, &op.feature_name).into_iter().map(|(n, _)| n).collect();
            // The standing FEATURES: a keep-out is steel, not part (slice 3).
            // Since Phase 49 slice 1 one standing feature can be read two
            // ways — an island of the floor, or bitten out of its boundary
            // because it touches it — and containment derives both.
            let mut seen: Vec<&str> = r.feature_islands();
            seen.extend(r.on_the_shore.iter().map(|s| s.as_str()));
            seen.sort();
            assert_eq!(seen, derived, "{}: {}", path.display(), op.feature_name);
            hosts += usize::from(!derived.is_empty());
        }
    }
    assert!(hosts >= 3, "pocket_post, pocket_islands, face_boss at least ({hosts})");
}

// ---- slice 2: open edges in the path ------------------------------------------

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

/// The shipped step: golden-pinned, entered from AIR, never ramped, never
/// further than one tool diameter past the blank, the wall respected, and the
/// rest report clean in both directions.
#[test]
fn test_step_golden_enters_from_air_and_stays_within_a_diameter_of_the_blank() {
    let src = std::fs::read_to_string("step.cam/main.part").unwrap();
    let p = parse::parse(&src).expect("step.cam parses");
    let g = gcode::program(&p).expect("generates under check_moves");
    assert_eq!(g.trim(), std::fs::read_to_string("tests/step.golden.gcode").unwrap().trim());
    assert!(g.contains("(--- pocket clear: Step with T5 ---)\n(open edges: 3 of 4 — entered from air)\n"));
    assert!(!g.contains("G01 Z0.000"), "no ramp anywhere: air is in reach");
    // The first entry: a plunge to depth OUTSIDE the blank (x=105 is a radius
    // past the x=100 face), then a feed straight in to the deepest level —
    // a 50 mm segment since Phase 58 slice 1 (the open corners filled, the
    // levels a stepover apart), where it was 40 under the corners' arcs.
    assert!(g.contains("G00 X105.000 Y15.124\nG00 Z5.000\nG01 Z-5.000 F300.000\nG01 X85.000 Y15.124 F800.000\n"), "{}", &g[..700]);

    let cut: Vec<_> = words(&g).into_iter().filter(|w| w.2 < -1e-9).collect();
    assert!(cut.len() > 100);
    for (x, y, _) in &cut {
        assert!(*x >= 65.0 - 1e-3, "the wall at x=60 keeps its radius: x {x}");
        assert!(*x <= 105.0 + 1e-3 && *y >= -5.0 - 1e-3 && *y <= 85.0 + 1e-3, "one radius past the blank and no further: ({x}, {y})");
    }
    assert_eq!(outline::region_line(&p, 0).unwrap(), "region: floor z -5.000 — 4 edges, 3 open");
    assert!(parse::lint(&p).iter().all(|f| !f.message.contains("stands outside") && !f.message.contains("declined")));
}

/// Fork 2A's promise, as bytes: the region is built from the outline CLIPPED
/// to the blank, so how far past the stock it was drawn cannot matter.
#[test]
fn test_a_step_drawn_twelve_or_a_hundred_past_the_blank_is_the_same_program() {
    let src = std::fs::read_to_string("step.cam/main.part").unwrap();
    let far = src.replace("112", "200").replace("-12", "-100").replace(" 92", " 180");
    assert_ne!(far, src);
    let (a, b) = (gcode::program(&parse::parse(&src).unwrap()).unwrap(), gcode::program(&parse::parse(&far).unwrap()).unwrap());
    assert_eq!(a, b);
    // Drawn exactly TO the blank's edge is open too ("on or beyond"), and the
    // same program again.
    let flush = src.replace("112", "100").replace("-12", "0").replace(" 92", " 80");
    assert_eq!(gcode::program(&parse::parse(&flush).unwrap()).unwrap(), a);
}

/// A closed pocket is untouched: no open edge, no island, the plain arm, the
/// golden it always had. And an open pocket with an island in it is both
/// phases at once — around the post, in from the air.
#[test]
fn test_closed_pockets_keep_their_arm_and_an_open_one_can_hold_an_island() {
    let closed = parse::parse(&std::fs::read_to_string("profile_pocket_clear.cam/main.part").unwrap()).unwrap();
    let r = region::of(&closed, &closed.operations[0]).unwrap();
    assert!(r.is_plain() && r.open_edges() == 0);
    assert_eq!(
        gcode::program(&closed).unwrap().trim(),
        std::fs::read_to_string("tests/profile_pocket_clear.golden.gcode").unwrap().trim()
    );

    let src = std::fs::read_to_string("step.cam/main.part").unwrap();
    let both = src.replace("run Clear", "feature Post CylinderBoss(dia=12, extent=5, x=82, y=40, z=-5)\nrun Clear");
    let p = parse::parse(&both).expect("an island in an open region");
    let g = gcode::program(&p).expect("generates");
    assert!(g.contains("(islands: Post)\n(open edges: 3 of 4 — entered from air)\n"), "{}", &g[..500]);
    assert!(!g.contains("G01 Z0.000"), "still no ramp");
    for (x, y, z) in words(&g) {
        if z < -1e-9 {
            let rho = ((x - 82.0).powi(2) + (y - 40.0).powi(2)).sqrt();
            assert!(rho >= 11.0 - nacs::CHORD_TOL_MM - 1e-3, "({x}, {y}) is {rho:.3} from the post's axis");
        }
    }
    // The narrowest passage is to the WALL (x=60): 82 - 6 - 60 = 16. The open
    // +x edge, 12mm the other way, is no passage at all.
    let n = region::of(&p, &p.operations[0]).unwrap().narrowest.unwrap();
    assert!((n.width - 16.0).abs() < 1e-9 && n.and == "'Step''s wall", "{n:?}");
}

// ---- slice 3: the keep-out ------------------------------------------------------

/// A pocket goes around a clamp standing in it just as a face does; a fixture
/// whose top is at or below the floor is not in the way; and one clear of the
/// region is not this op's business.
#[test]
fn test_a_fixture_in_a_pocket_is_kept_out_of_and_one_below_the_floor_is_not() {
    let pocket = "feature Cavity ProfilePocket(extent=5.0\n    10 10\n    90 10\n    90 70\n    10 70\n)\n";
    let run = "run Clear(tool=T5, profile=Cavity, stepover=4.0)\n";
    let p = program(&format!("fixture Pin Cylinder(dia=10, h=30, x=50, y=40, z=-5)\n{pocket}{run}"));
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!(r.keep_outs(), ["Pin"]);
    assert!(r.feature_islands().is_empty());
    let g = gcode::program(&p).expect("generates");
    assert!(g.contains("(--- pocket clear: Cavity with T5 ---)\n(keep-outs: Pin)\n"), "{}", &g[..400]);
    assert!(!g.contains("G00 Z5.000"), "lifts go to the clearance plane where steel stands");
    for (x, y, z) in words(&g) {
        if z < -1e-9 {
            // Pin r 5 + margin 0.5 + tool r 5.
            let rho = ((x - 50.0).powi(2) + (y - 40.0).powi(2)).sqrt();
            assert!(rho >= 10.5 - nacs::CHORD_TOL_MM - 1e-3, "({x}, {y}) is {rho:.3} from the pin");
        }
    }
    assert_eq!(
        outline::region_line(&p, 0).unwrap(),
        "region: floor z -5.000 — 4 edges, 0 open; keeping out of Pin; narrowest passage 24.500mm (2.45 tool diameters) between 'Pin' and 'Cavity''s wall"
    );

    // A locating pin whose top is AT the floor stands in nobody's way…
    let low = program(&format!("fixture Pin Cylinder(dia=10, h=5, x=50, y=40, z=-10)\n{pocket}{run}"));
    assert!(region::of(&low, &low.operations[0]).unwrap().keep_outs().is_empty());
    // …and neither does a clamp beside the pocket.
    let beside = program(&format!("fixture Clamp Box(w=6, l=6, h=30, x=92, y=72, z=0)\n{pocket}{run}"));
    assert!(region::of(&beside, &beside.operations[0]).unwrap().is_plain());
    assert_eq!(
        gcode::program(&beside).unwrap(),
        gcode::program(&program(&format!("{pocket}{run}"))).unwrap(),
        "a fixture out of the region changes nothing"
    );
}

/// A keep-out too close to a wall for the tool to pass IS a web — between
/// steel and a wall there is material, and no way to it.
#[test]
fn test_a_keep_out_too_near_a_wall_is_a_web() {
    let p = program(
        "fixture Pin Cylinder(dia=10, h=30, x=22, y=40, z=-5)\n\
         feature Cavity ProfilePocket(extent=5.0\n    10 10\n    90 10\n    90 70\n    10 70\n)\n\
         run Clear(tool=T5, profile=Cavity, stepover=4.0)\n",
    );
    let e = gcode::program(&p).unwrap_err();
    // Since Phase 32 slice 2: one stretch the op cannot reach, said with the
    // tool that would fit it and the word that concedes it.
    assert!(e.contains("cannot reach 1 stretch of its floor") && e.contains("between 'Pin' and the wall of 'Cavity'") && e.contains("6.500mm wide — a tool up to 6.500mm would reach it"), "{e}");
}

// ---- slice 4: the floor's verb ----------------------------------------------------

/// `Floor` finishes a declared planar floor, and a pocket declares one. The
/// shipped fixture roughs with ONE allowance, faces the floor, finishes the
/// walls: golden-pinned, the rough a half-millimetre high, the floor pass at
/// size and half a millimetre further off the walls, and the rest report clean.
#[test]
fn test_pocket_floor_golden_roughs_high_and_faces_to_size() {
    let src = std::fs::read_to_string("pocket_floor_offset.cam/main.part").unwrap();
    let p = parse::parse(&src).expect("pocket_floor_offset.cam parses");
    let g = gcode::program(&p).expect("generates under check_moves");
    assert_eq!(g.trim(), std::fs::read_to_string("tests/pocket_floor_offset.golden.gcode").unwrap().trim());

    let rough = g.split("(--- pocket clear: Cavity").nth(1).unwrap().split("(---").next().unwrap();
    assert!(rough.contains("(floor: 0.500 left for the End op that follows)"));
    assert!(rough.contains("Z-5.500") && !rough.contains("Z-6.000"), "every loop of the rough rides 0.5 above the floor, at ONE level");
    let face = g.split("(--- floor: Cavity floor").nth(1).unwrap().split("(---").next().unwrap();
    assert!(face.contains("(islands: Post)\n(floor: takes the 0.500 the rough left)"), "{}", &face[..200]);
    assert!(face.contains("Z-6.000"));
    // It descends to where the rough stopped, never from the stock's top.
    assert!(!face.contains("G01 Z0.000"), "no ramp from z=0 through air the rough already opened");
    // Its own allowance is its stand-off from the walls: x 15..85 less r 5
    // less 0.5 — and the same from the post.
    for (x, y, z) in words(&format!("G00 X50 Y40 Z25\n{face}")).into_iter().skip(1) {
        if z < -5.75 {
            assert!(x >= 20.5 - 1e-3 && x <= 79.5 + 1e-3 && y >= 20.5 - 1e-3 && y <= 59.5 + 1e-3, "({x}, {y})");
            let rho = ((x - 50.0).powi(2) + (y - 40.0).powi(2)).sqrt();
            assert!(rho >= 12.5 - nacs::CHORD_TOL_MM - 1e-3, "({x}, {y}) is {rho:.3} from the post");
        }
    }
    assert_eq!(parse::derive_floor_skin(&p, &p.operations[0]), 0.5);
}

/// STILL ONE ALLOWANCE (fork 6A, revised on review): the floor half is derived
/// from whether a later `Floor` on the same pocket is there to take it. No
/// consumer, no floor skin — byte for byte what the Clear always cut.
#[test]
fn test_the_floor_skin_exists_exactly_when_a_later_floor_takes_it() {
    let src = std::fs::read_to_string("pocket_floor_offset.cam/main.part").unwrap();
    let no_face = src.replace("run End(tool=T5, surface=Cavity, stepover=4.0, stock_to_leave=0.5, pattern=offset)\n", "");
    assert_ne!(no_face, src);
    let p = parse::parse(&no_face).unwrap();
    assert_eq!(parse::derive_floor_skin(&p, &p.operations[0]), 0.0);
    let g = gcode::program(&p).unwrap();
    assert!(!g.contains("(floor:") && !g.contains("Z-5.500"), "the floor goes to size");
    // An End BEFORE the Clear takes nothing from it.
    let early = src
        .replace("run End(tool=T5, surface=Cavity, stepover=4.0, stock_to_leave=0.5, pattern=offset)\n", "")
        .replace("run Clear(", "run End(tool=T5, surface=Cavity, stepover=4.0)\nrun Clear(");
    let q = parse::parse(&early).unwrap();
    assert_eq!(parse::derive_floor_skin(&q, &q.operations[1]), 0.0);
    // The shipped rough that leaves an allowance and never faces: unchanged.
    let shipped = parse::parse(&std::fs::read_to_string("profile_pocket_clear_rough.cam/main.part").unwrap()).unwrap();
    assert_eq!(
        gcode::program(&shipped).unwrap().trim(),
        std::fs::read_to_string("tests/profile_pocket_clear_rough.golden.gcode").unwrap().trim()
    );
}

/// When is each verb allowed: `Floor` wherever a floor is declared. A closed
/// empty pocket takes the plain spiral; a `Plane` still refuses an allowance
/// (it has no wall to stand off); and a feature with no planar floor refuses.
#[test]
fn test_floor_takes_a_declared_floor_and_nothing_else() {
    let pocket = "feature Cavity ProfilePocket(extent=5.0\n    20 20\n    80 20\n    80 60\n    20 60\n)\n";
    let p = program(&format!("{pocket}run Clear(tool=T5, profile=Cavity, stepover=4.0)\nrun End(tool=T5, surface=Cavity, stepover=4.0)\n"));
    let g = gcode::program(&p).expect("a spring pass over a floor already at size");
    assert!(g.contains("(--- floor: Cavity floor with T5 ---)\n(floor: takes the 0.000 the rough left)"), "{g}");
    // Never cleared: the floor pass would plunge into the pocket's own stock.
    let bare = program(&format!("{pocket}run End(tool=T5, surface=Cavity, stepover=4.0)\n"));
    assert!(gcode::program(&bare).unwrap_err().contains("drives the tool into material"));

    let e = parse::parse(&format!("{HEAD}feature Deck Plane(z=-2)\nrun End(tool=T5, surface=Deck, stepover=6.0, stock_to_leave=0.5)\n")).unwrap_err();
    assert!(e.contains("does not carry stock_to_leave") && e.contains("Plane"), "{e}");
    let e = parse::parse(&format!("{HEAD}feature Post CylinderBoss(dia=12, extent=5, x=50, y=40, z=-5)\nrun End(tool=T5, surface=Post, stepover=4.0)\n")).unwrap_err();
    assert!(e.contains("must declare a planar floor — a Plane, a Face, or a ProfilePocket"), "{e}");
}

// ---- Phase 29 slice 1: the headland -----------------------------------------
// `DECISIONS.md` → "Phase 29 slice 1: the headland…". An edge is CLOSED when a
// DECLARED neighbour stands above this floor just beyond it. Before this slice
// a `Face` short-circuited every edge to Open, and the stepped wrench emitted
// exit 0 while machining a quarter of its raised ring away.

/// The rule, both ways round on one part: the deck's arc against the raised
/// ring is a WALL because BoxTop declares a floor above it; the shank's long
/// edges stay OPEN because nothing is declared beyond them, though the bar
/// they overhang is solid material. This is why the probe reads DECLARATIONS
/// and never the remaining stock — stock cannot tell a neighbour from scrap.
#[test]
fn test_a_declared_neighbour_closes_an_edge_and_bare_scrap_does_not() {
    let p = parse::load_project("wrench_step_offset.cam").unwrap();
    let deck = p.operations.iter().find(|o| o.feature_name == "Deck").expect("the deck is faced");
    let r = region::of(&p, deck).expect("an End on a Face works a floor");
    let walls = r.edges.iter().filter(|k| **k == EdgeKind::Wall).count();
    let opens = r.edges.iter().filter(|k| **k == EdgeKind::Open).count();
    assert_eq!((walls, opens), (8, 51), "the arc against the ring, and nothing else");
    // Every WALL edge lies on the ring's side of the deck; every OPEN one does not.
    for (i, kind) in r.edges.iter().enumerate() {
        let (a, b) = (r.outer[i], r.outer[(i + 1) % r.outer.len()]);
        let mid_x = (a.x + b.x) / 2.0;
        match kind {
            EdgeKind::Wall => assert!(mid_x < 50.0, "wall edge {i} at x={mid_x} is not against the ring"),
            EdgeKind::Open => assert!(mid_x > 46.0, "open edge {i} at x={mid_x} is against the ring"),
        }
    }
    // The ring's own top has nothing standing beside it: all open.
    let top = p.operations.iter().find(|o| o.feature_name == "BoxTop").unwrap();
    let rt = region::of(&p, top).unwrap();
    assert!(rt.edges.iter().all(|k| *k == EdgeKind::Open), "nothing stands above the ring's top");
    assert!(rt.keep_clear.is_empty(), "nothing to clip against");
}

/// Fork 2B: the swath keeps the whole-outline offset (the jaw throat closes
/// into a hole under per-edge strips) and is clipped back out of the
/// neighbour. The test is the one that matters — no tool-centre position may
/// come within a radius of the DECLARED ring, on any pattern.
#[test]
fn test_the_swath_is_clipped_out_of_the_neighbour_on_every_pattern() {
    let ring: Vec<parse::Pt> = {
        let p = parse::load_project("wrench_step_offset.cam").unwrap();
        match p.features.get("BoxTop").unwrap() {
            parse::Feature::Face { loops, .. } => loops[0].clone(),
            _ => unreachable!("BoxTop is a Face"),
        }
    };
    for dir in ["wrench_step_offset.cam", "wrench_step_zigzag.cam", "wrench_step_oneway.cam"] {
        let p = parse::load_project(dir).unwrap();
        let deck = p.operations.iter().find(|o| o.feature_name == "Deck").unwrap();
        let r = region::of(&p, deck).unwrap();
        assert_eq!(r.keep_clear.len(), 1, "{dir}: the ring is the one thing to keep clear of");
        let g = gcode::program(&p).unwrap_or_else(|e| panic!("{dir} must emit: {e}"));
        let deck_block = g.split("(--- floor: Deck").nth(1).expect("the deck is emitted");
        let (mut x, mut y) = (f64::NAN, f64::NAN);
        let mut worst = f64::INFINITY;
        for line in deck_block.lines().filter(|l| l.starts_with("G01") || l.starts_with("G00")) {
            for (key, slot) in [('X', &mut x), ('Y', &mut y)] {
                if let Some(v) = line.split_whitespace().find_map(|t| t.strip_prefix(key)) {
                    *slot = v.parse().unwrap();
                }
            }
            if x.is_nan() || y.is_nan() || line.starts_with("G00") {
                continue;
            }
            worst = worst.min(stock::dist_to_polygon(&ring, x, y));
        }
        // 5.0 is the tool radius; CHORD_TOL_MM is what a flattened curve may be out by.
        assert!(
            worst >= 5.0 - nacs::CHORD_TOL_MM,
            "{dir}: a cutting position came {worst} from the declared ring — inside the tool radius"
        );
    }
}

/// Phase 29 slice 2. A wall is quite as often STANDING MATERIAL as it is
/// another floor, so the probe reads a boss's top as readily as a face's
/// floor. Three shapes the corpus had never had: two walls meeting at an
/// interior 90° corner; a wall chain that terminates mid-side, twice, with
/// free boundary past each end; and a deck walled at BOTH ends.
#[test]
fn test_a_boss_beside_a_floor_closes_its_edge_and_a_chain_may_end_mid_side() {
    // The room: two of four edges closed, and they meet at the corner.
    let p = parse::load_project("cottage_interior_offset.cam").unwrap();
    let r = region::of(&p, &p.operations[0]).unwrap();
    assert_eq!(r.edges.iter().filter(|k| **k == EdgeKind::Wall).count(), 2);
    assert_eq!(r.keep_clear.len(), 2, "both walls are kept clear of");
    // The two WALL edges are adjacent — that is what makes the corner.
    let walls: Vec<usize> = (0..r.edges.len()).filter(|i| r.edges[*i] == EdgeKind::Wall).collect();
    assert_eq!(walls.len(), 2);
    assert_eq!((walls[1] - walls[0]) % r.edges.len(), 1, "the walls meet at a corner");

    // The lawn: the bed's chain ENDS mid-side, and the boundary past each end
    // is free — an open gap to the fence above, and to the road below.
    let q = parse::load_project("lawn_fence_bed_offset.cam").unwrap();
    let rq = region::of(&q, &q.operations[0]).unwrap();
    let opens = rq.edges.iter().filter(|k| **k == EdgeKind::Open).count();
    assert_eq!((rq.edges.len(), opens), (17, 4), "road, driveway, and a gap each side of the bed");
    // Walk the ring: the closed run must break, i.e. there are two separate
    // chains (the fence, and the bed), not one.
    let n = rq.edges.len();
    let breaks = (0..n).filter(|i| rq.edges[*i] == EdgeKind::Wall && rq.edges[(i + n - 1) % n] == EdgeKind::Open).count();
    assert_eq!(breaks, 2, "two wall chains, each starting after an open edge");

    // The thickened wrench: the deck is walled at both ends.
    let w = parse::load_project("wrench_thick_offset.cam").unwrap();
    let deck = w.operations.iter().find(|o| o.feature_name == "Deck").unwrap();
    let rw = region::of(&w, deck).unwrap();
    assert_eq!(rw.edges.iter().filter(|k| **k == EdgeKind::Open).count(), 2, "the two long sides");
    assert_eq!(rw.keep_clear.len(), 2, "the ring and the jaw");
}

// ---- Phase 34 slice 0: the floor's shape as two numbers ------------------------

fn shape_of(p: &parse::Part, i: usize) -> region::Shape {
    let op = &p.operations[i];
    region::shape(p, op, &region::of(p, op).unwrap()).expect("the floor has a shape")
}

/// Mean width is `2·area / perimeter` and wall share is the length of boundary
/// something stands beyond. Both are read off the region `of` derived, by hand
/// here: the step is 40 x 80 with its one wall 80 long, a closed pocket is all
/// wall, and a faced plane is the blank with none.
#[test]
fn test_the_floors_shape_is_its_mean_width_and_its_wall_share() {
    let step = shape_of(&program(STEP), 0);
    assert!((step.area - 3200.0).abs() < 1e-6 && (step.perimeter - 240.0).abs() < 1e-6, "{step:?}");
    assert!((step.mean_width() - 3200.0 * 2.0 / 240.0).abs() < 1e-9);
    assert!((step.wall_share() - 80.0 / 240.0).abs() < 1e-9, "one wall of four edges, by LENGTH: {step:?}");

    let plane = shape_of(&program("feature Top Plane(z=-2.0)\nrun End(tool=T5, surface=Top, stepover=5.0)\n"), 0);
    assert_eq!((plane.area, plane.perimeter, plane.wall), (8000.0, 360.0, 0.0));

    let closed = shape_of(&parse::load_project("profile_pocket_clear.cam").unwrap(), 0);
    assert!((closed.wall_share() - 1.0).abs() < 1e-9, "{closed:?}");
}

/// What stands on the floor is not floor, and its outline is wall — a round
/// post on its CIRCLE, not its chords. The post makes the pocket NARROWER by
/// this measure, which is the point: the same outline with something standing
/// in it is less of a field.
#[test]
fn test_what_stands_on_the_floor_takes_its_area_and_adds_its_wall() {
    let p = parse::load_project("pocket_post.cam").unwrap();
    let with = shape_of(&p, 0);
    let r = region::of(&p, &p.operations[0]).unwrap();
    let outer_area: f64 = (0..r.outer.len()).map(|i| r.outer[i].x * r.outer[(i + 1) % r.outer.len()].y - r.outer[(i + 1) % r.outer.len()].x * r.outer[i].y).sum::<f64>().abs() / 2.0;
    let nacs::offset::Island::Circle { r: post, .. } = r.standing()[0].1 else { panic!("the post is round") };
    assert!((with.area - (outer_area - std::f64::consts::PI * post * post)).abs() < 1e-6, "{with:?}");
    assert!((with.wall_share() - 1.0).abs() < 1e-9, "a pocket's wall and a post: all wall");
    assert!(with.mean_width() < 2.0 * outer_area / (with.perimeter - std::f64::consts::TAU * post));
}

/// A `Face`'s floor is what its loops draw — the walled plate's land is its
/// outline less all eight inner loops, whatever stands in them — and an inner
/// loop's edge is a wall exactly where something stands beyond it. The rail
/// plate draws the same kind of land with nothing raised: no wall at all.
#[test]
fn test_a_faces_floor_is_its_drawn_loops_and_a_loop_is_walled_where_something_stands() {
    let p = parse::load_project("walled_rails_offset.cam").unwrap();
    let Some(parse::Feature::Face { loops, .. }) = p.features.get("Land") else { panic!("the land is a Face") };
    let area = |l: &[parse::Pt]| (parse::signed_area(l) / 2.0).abs();
    let drawn = area(&loops[0]) - loops[1..].iter().map(|l| area(l)).sum::<f64>();
    let land = shape_of(&p, 0);
    assert!((land.area - drawn).abs() < 1e-6, "{} vs {drawn}", land.area);
    assert!(land.wall > 0.0 && land.wall < land.perimeter, "walled in places: {land:?}");
    assert!(land.mean_width() < 5.0, "rails: {}", land.mean_width());

    let bare = parse::load_project("rail_plate_offset.cam").unwrap();
    assert_eq!(shape_of(&bare, 0).wall, 0.0);
}

/// A neighbour that only TOUCHES the floor's boundary bites nothing out of it
/// (`deck_rest`'s web lies between two posts whose circles are its ends): the
/// floor keeps its area, and the touched edges are the wall.
#[test]
fn test_a_neighbour_that_touches_the_boundary_is_wall_and_not_a_bite() {
    let p = parse::load_project("deck_rest_offset.cam").unwrap();
    let pieces = nacs::rest::pieces(&p, &p.operations[1]).unwrap();
    let pc = pieces.iter().find(|pc| pc.is_cut()).unwrap();
    let s = region::shape(&pc.part, &pc.op, &region::of(&pc.part, &pc.op).unwrap()).expect("measured, not declined");
    assert!((s.area - 50.0).abs() < 1.0, "the web the rest line names: {s:?}");
    assert!(s.wall_share() > 0.4 && s.wall_share() < 0.6, "the two posts' arcs: {s:?}");
}

/// Phase 47 slice 1, found on the way to the round blank: an L-shaped
/// island's NOTCH is an inside corner no round tool reaches — nobody's to
/// cut — and it stays one beside a second island and on a floor whose every
/// edge is OPEN. Before: `cover::of` listed each island once, so "the two
/// nearest standing things are the same island" never held, and on an
/// all-open floor every chord through the notch escaped past the blank, so
/// the pinch was judged at the grid's corner; the End refused "5.9mm²
/// between 'LW' and 'LE' … 0.000mm wide" for a fillet in LW's own notch.
#[test]
fn test_an_l_islands_notch_is_an_inside_corner_not_a_stretch_between_two_islands() {
    let two_ls = "feature Level Face(z=-5.0\n    loop\n        -5 -5\n        105 -5\n        105 85\n        -5 85\n)\n\
feature LW ProfileBoss(extent=5.0\n    44.5 60\n    44.5 20\n    12 20\n    12 32\n    29.5 32\n    29.5 60\n)\n\
feature LE ProfileBoss(extent=5.0\n    55.5 20\n    55.5 60\n    70.5 60\n    70.5 32\n    88 32\n    88 20\n)\n\
run End(tool=T5, surface=Level, stepover=4.0)\n";
    let p = program(two_ls);
    let r = region::of(&p, &p.operations[0]).expect("an End on a Face works a floor");
    assert_eq!(r.open_edges(), r.edges.len(), "a Face drawn past the blank: every edge open");
    assert_eq!(r.standing().len(), 2);
    let cov = nacs::ops::clear::coverage(&p, &p.operations[0], &r, 5.0, 0.0).expect("coverage");
    assert!(
        cov.stretches.is_empty(),
        "the notches are corners, not stretches: {:?}",
        cov.stretches.iter().map(|s| (s.area_mm2, s.width, s.between.clone())).collect::<Vec<_>>()
    );
    gcode::program(&p).expect("and the End generates");
}

/// Phase 49 slice 1 (fork 1A): a boss standing ON the boundary is the shore,
/// not an island. `rim_boss_offset.cam`'s two ribs sit on the blank's
/// silhouette to the fourth decimal; the region bites them out of its outer
/// boundary — their flanks and inner arcs are WALL edges of it, they are in
/// no island list, and `--outline` says so. A rib a millimetre inside the
/// edge is an island as it always was, and nothing is on the shore.
#[test]
fn test_a_boss_on_the_silhouette_is_bitten_out_of_the_boundary_not_an_island() {
    let p = parse::load_project("rim_boss_offset.cam").unwrap();
    let r = region::of(&p, &p.operations[0]).expect("a Clear on the level works a floor");
    assert_eq!(r.on_the_shore, vec!["RimE".to_string(), "RimW".to_string()]);
    assert!(r.feature_islands().is_empty(), "nothing stands INSIDE the level: {:?}", r.feature_islands());
    let walls = r.edges.iter().filter(|k| **k == EdgeKind::Wall).count();
    // Each bitten rib leaves its two flanks and four inner chords in the
    // boundary: twelve wall edges between two ribs, the rest the blank's.
    assert_eq!(walls, 12, "the ribs' flanks and inner arcs are the walls: {} of {}", walls, r.edges.len());
    assert!(r.open_edges() > 100, "the blank's edge stays open: {}", r.open_edges());
    assert!(r.line(8.0).contains("on the shore RimE, RimW (bitten out of the boundary)"), "{}", r.line(8.0));
    assert!(!r.is_plain());
    // Every edge the bite made lies on a rib: no stray chord of the boolean.
    let ribs: Vec<Vec<parse::Pt>> = ["RimE", "RimW"].iter().map(|n| match &p.features[*n] {
        parse::Feature::ProfileBoss { points, .. } => points.clone(),
        _ => unreachable!(),
    }).collect();
    for (i, k) in r.edges.iter().enumerate() {
        if *k != EdgeKind::Wall { continue; }
        let (a, b) = (r.outer[i], r.outer[(i + 1) % r.outer.len()]);
        let mid = parse::Pt { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 };
        assert!(ribs.iter().any(|rib| stock::dist_to_polygon(rib, mid.x, mid.y) <= 1e-3), "wall edge {i} at {mid:?} is on no rib");
    }

    // The same reading on a square blank: a boss whose edge LIES ON its
    // pocket's wall is bitten out of the boundary; the same boss a millimetre
    // off the wall is an island, as ever.
    let pocket = "feature Pocket ProfilePocket(z=0.0, extent=5.0
    20 20
    80 20
    80 60
    20 60
)
";
    let flush = program(&format!("{pocket}feature Post ProfileBoss(z=0.0, extent=5.0
    20 45
    30 45
    30 35
    20 35
)
run Clear(tool=T5, profile=Pocket, stepover=4.0)
"));
    let rf = region::of(&flush, &flush.operations[0]).unwrap();
    assert_eq!(rf.on_the_shore, vec!["Post".to_string()]);
    assert!(rf.feature_islands().is_empty());
    assert_eq!(rf.open_edges(), 0, "a closed pocket: every edge a wall, the bite's included");
    assert_eq!(rf.edges.len(), 8, "four walls, less the stretch the post stands on, plus its three free sides: {}", rf.edges.len());
    let off = program(&format!("{pocket}feature Post ProfileBoss(z=0.0, extent=5.0
    21 45
    31 45
    31 35
    21 35
)
run Clear(tool=T5, profile=Pocket, stepover=4.0)
"));
    let ro = region::of(&off, &off.operations[0]).unwrap();
    assert!(ro.on_the_shore.is_empty());
    assert_eq!(ro.feature_islands(), vec!["Post"]);
    assert_eq!(ro.edges.len(), 4);
}

/// Phase 58 slice 1 (`DECISIONS.md` → "Phase 58 slice 1…"): the OPEN CORNER.
/// Where two open edges meet, the family wall used to leave the square
/// between their strips unfilled, so the blank's corner survived the union
/// as a reflex vertex the erosion rounded, and every pattern stood `r + skin`
/// off a point that is air on both sides. Now the corner is filled with the
/// sector the outward offset would give: the family wall contains a point a
/// radius diagonally outside the plate's open corner, and the allowed region
/// at a skin `s` passes within `r − s` of that corner, so the cutter covers
/// it by the skin.
#[test]
fn test_the_open_corner_is_filled_and_the_cutter_covers_it_by_the_skin() {
    let p = parse::load_project("plate.cam").unwrap();
    let r = region::of(&p, &p.operations[1]).unwrap();
    assert_eq!(r.open_edges(), 2, "the notch's two open edges");
    let swath = r.family_wall(10.0).unwrap();
    assert!(stock::point_in_polygon(&swath.wall, -3.5, 83.5), "the square beyond the corner (0, 80) is swath");
    let src = std::fs::read_to_string("plate.cam/main.part").unwrap().replace("pattern=spine)", "pattern=spine, stock_to_leave=0.5)");
    assert!(src.contains("stock_to_leave=0.5"));
    let geom = std::fs::read_to_string("plate.cam/geometry.part").unwrap();
    let p = parse::parse_project_with_base(&src, Some(&geom), Some(std::path::Path::new("plate.cam"))).unwrap();
    let op = &p.operations[1];
    let (ccw, _) = nacs::ops::clear::allowed_loops(&p, op, &region::of(&p, op).unwrap(), 5.0, 0.5).unwrap();
    let near = ccw.iter().map(|l| stock::dist_to_polygon(l, 0.0, 80.0)).fold(f64::INFINITY, f64::min);
    // Phase 61 slice 1 moved this from `r − s` (4.5) to `r` (5.0): the push now
    // carries the allowance (`2r + skin`), so on an open-open vertex the
    // erosion by `r + skin` leaves the centre exactly a radius off the corner
    // and the cutter's edge ON it — covered, with no skin left on air. The
    // old figure was the leak itself: the tool stopped `s` short of the
    // stock's edge all along every open edge (DECISIONS → "Phase 61 slice 1").
    assert!(near <= 5.0 + nacs::CHORD_TOL_MM, "the allowed region stands {near:.3} off the open corner; r is 5.0");
}

/// Phase 58 slice 1, the second face of the open corner: on a CHORDED open
/// edge the reflex vertices sat a chord apart, their exclusion discs
/// overlapped, and the allowed region's whole outer boundary was pulled a
/// radius INSIDE the blank. A half-plane pocket on a round blank — the chord
/// x=0 its wall, the arc its open edge — rides a radius outside it.
#[test]
fn test_a_pocket_open_along_a_chorded_silhouette_rides_a_radius_outside_the_blank() {
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Cylinder(dia=120, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=8.0)\n\
feature Half ProfilePocket(extent=5.0\n    0 -100\n    100 -100\n    100 100\n    0 100\n)\n\
run Clear(tool=T5, profile=Half, stepover=3.0)\n";
    let p = parse::parse(src).unwrap_or_else(|e| panic!("must parse: {e}"));
    let op = &p.operations[0];
    let r = region::of(&p, op).unwrap();
    assert!(r.open_edges() > 40 && r.open_edges() < r.edges.len(), "the arc's chords open, the chord x=0 a wall: {} of {}", r.open_edges(), r.edges.len());
    let (ccw, _) = nacs::ops::clear::allowed_loops(&p, op, &r, 4.0, 0.0).unwrap();
    // Past the wall's own stand-off (x = 4) every vertex is the outer arc's.
    let least = ccw.iter().flatten().filter(|q| q.x > 4.1).map(|q| q.x.hypot(q.y)).fold(f64::INFINITY, f64::min);
    assert!(least >= 64.0 - 0.05, "the allowed region's outer boundary is at r {least:.3}; a radius outside the blank is r 64");
    gcode::program(&p).unwrap_or_else(|e| panic!("the half-plane pocket generates: {e}"));
}

/// Phase 58 slice 1: the allowance is a skin on WALLS alone. A step cleared
/// with `stock_to_leave` and then sided forgets nothing along its open
/// edges at a cell fine enough to see half a millimetre — the open corners
/// included, where the skin used to be left for no pass to take.
#[test]
fn test_a_skinned_step_forgets_nothing_along_its_open_edges() {
    let p = program(
        "feature Step ProfilePocket(extent=5.0\n    60 -12\n    112 -12\n    112 92\n    60 92\n)\n\
run Clear(tool=T5, profile=Step, stepover=4.0, stock_to_leave=0.5)\nrun Side(tool=T5, surface=Step)\n",
    );
    let (_, emitted) = gcode::program_with_moves(&p).unwrap();
    let cells = nacs::vox::rest_cells(&p, &emitted, p.operations.len(), Some(0.2)).unwrap();
    // Away from the wall at x 60: the open edges and their two corners.
    let on_open: Vec<_> = cells.forgotten.iter().filter(|c| c[0] > 61.0).collect();
    assert!(on_open.is_empty(), "{} cells forgotten along the open edges, first at {:?}", on_open.len(), on_open[0]);
}

/// The unbounded chord (a ROADMAP debt, found 2026-10-11 by Phase 47 slice
/// 1): a stretch beside the floor's OPEN edge had nothing standing beyond
/// it, so its chord ran to infinity, read "0.000mm wide", and with one
/// standing thing it was dropped as a corner. A chord out through an open
/// edge is no pinch — the tool reaches that floor from the air side — so
/// such a stretch now says it is open to the air, names the edge, and
/// bounds no tool. Read with no passes at all, so the whole floor is the one
/// stretch, and every chord through it from the post runs out to air.
#[test]
fn test_a_stretch_open_to_the_air_bounds_no_tool_and_names_the_edge() {
    let src = "feature Level Face(z=-5.0\n    loop\n        0 0\n        100 0\n        100 80\n        0 80\n)\n\
feature Post ProfileBoss(extent=5.0\n    95 50\n    95 30\n    80 30\n    80 50\n)\n\
run End(tool=T5, surface=Level, stepover=4.0)\n";
    let p = program(src);
    let r = region::of(&p, &p.operations[0]).expect("an End on a Face works a floor");
    assert_eq!(r.open_edges(), r.edges.len(), "a Face at the blank's edge: every edge open");
    let mut floor = vec![r.outer.clone()];
    floor.extend(r.standing_polygons());
    let cov = nacs::cover::of(&r, &floor, &[], &[], 5.0);
    assert_eq!(
        cov.stretches.len(),
        1,
        "nothing cut: the whole floor is one stretch, not a corner: {:?}",
        cov.stretches.iter().map(|s| (s.area_mm2, s.width, s.between.clone())).collect::<Vec<_>>()
    );
    let s = &cov.stretches[0];
    assert!(s.width.is_infinite(), "no chord ends on anything standing: {}", s.width);
    assert_eq!(s.between, vec!["Post".to_string(), "the floor's open edge".to_string()]);
    assert!(s.at.x > 0.0 && s.at.x < 100.0 && s.at.y > 0.0 && s.at.y < 80.0, "said at its middle, on the floor: {:?}", s.at);
    let line = nacs::cover::stretch_line(s);
    assert!(line.contains("between 'Post' and the floor's open edge") && line.contains("open to the air past the blank — no width bounds a tool there"), "{line}");
}
