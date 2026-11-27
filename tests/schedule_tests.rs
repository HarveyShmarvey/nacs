// ---- the schedule's pins (ROADMAP item 1, slice 10) --------------------------
//
// The schedule writes a run line for every declared feature no run names,
// with the tool picked from the project's own tool lines. The claim under
// test: WHICH lines, in WHICH order, with WHICH tool — and that what it
// writes always parses, since every line is judged by the parser before it
// is offered and a refused one becomes a note.

use nacs::{gcode, parse, schedule};

const HEADER: &str = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=200, l=120, h=30, x=0, y=0, z=0)\n";

const TOOLS: &str = "\
tool T1 Endmill(dia=10.0)\n\
tool T2 Drill(dia=6.0)\n\
tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n\
tool T4 Ballmill(dia=8.0)\n";

const FEATURES: &str = "\
feature Top Plane(z=0)\n\
feature Big CylinderBore(dia=20.0, extent=10.0, x=150, y=60)\n\
feature Hole1 CylinderBore(dia=6.0, extent=30.0, x=30, y=30)\n\
feature Pad ProfileBoss(extent=10.0, chamfer_depth=1.0, chamfer_angle=90.0\n    20 100\n    60 100\n    60 60\n    20 60\n)\n\
feature Shelf Plane(z=-5)\n\
feature Hole2 CylinderBore(dia=6.0, extent=30.0, x=60, y=30)\n\
feature Floor Plane(z=-10)\n";

fn schedule_of(src: &str) -> (parse::Part, schedule::Schedule) {
    let p = parse::parse(src).unwrap_or_else(|e| panic!("the project must parse: {e}"));
    let s = schedule::derive_schedule(&p, src, None, None);
    (p, s)
}

#[test]
fn test_the_schedule_runs_the_shop_order_with_the_projects_own_tools_and_the_result_emits() {
    let src = format!("{HEADER}{TOOLS}{FEATURES}");
    let (_, s) = schedule_of(&src);
    assert_eq!(
        s.lines,
        vec![
            // Both holes on the one drill of their diameter, one line.
            "run Form(points=[Hole1, Hole2], tool=T2, cycle=Simple)",
            // Floors from the highest down, whatever order they were declared in.
            "run Clear(tool=T1, profile=Shelf, stepover=5)",
            "run Clear(tool=T1, profile=Floor, stepover=5)",
            // A bore no drill fits is milled; the ⌀10 tool's wall pass reaches
            // the ⌀20 bore's centre, so it needs no Clear before it. Its rings
            // are circles, written as arcs.
            "run Side(tool=T1, surface=Big, stepover=5, compress=arcs)",
            // The boss stands on a floor that is cleared, so only its wall pass.
            "run Side(tool=T1, surface=Pad)",
            // Every chamfer last, on the mill at the loop's angle.
            "run Form(tool=T3, surface=Pad)",
        ],
        "notes: {:?}",
        s.notes
    );
    assert_eq!(s.notes, vec!["Top lies at the blank's top: nothing to cut until the blank carries an allowance above it"]);
    // What it writes parses and emits — the whole point of judging each line.
    let full = format!("{src}\n{}", schedule::render(&s));
    let p = parse::parse(&full).unwrap_or_else(|e| panic!("the scheduled project must parse: {e}\n{full}"));
    gcode::program(&p).unwrap_or_else(|e| panic!("the scheduled project must emit: {e}"));
    // The rendered text names its source and carries the note as a comment.
    let text = schedule::render(&s);
    assert!(text.starts_with("# scheduled by nacs --schedule"), "{text}");
    assert!(text.contains("\n# not scheduled: Top lies at the blank's top: nothing to cut until the blank carries an allowance above it\n"), "{text}");
}

/// A drilled hole deeper than five diameters pecks, a diameter a peck; one
/// at five diameters or under is a single plunge, on its own line.
/// A boss in a pocket with a 9 mm gap on one side: the 10 mm endmill, the
/// largest, cannot pass, and the engine refuses its wall pass. The schedule
/// gives that one line the next endmill down and says so; the result runs.
#[test]
fn test_a_wall_pass_the_largest_endmill_cannot_make_takes_the_next_one_down() {
    let src = format!(
        "{HEADER}{TOOLS}tool T5 Endmill(dia=4.0)\n\
         feature Well ProfilePocket(extent=5.0\n    30 36\n    77 36\n    77 84\n    30 84\n)\n\
         feature Post CylinderBoss(dia=25.0, extent=5.0, x=55.5, y=60.0, z=-5.0)\n"
    );
    let (_, s) = schedule_of(&src);
    assert!(s.lines.contains(&"run Side(tool=T5, surface=Post, stepover=2, compress=arcs)".to_string()), "{:?}", s.lines);
    assert!(s.lines.contains(&"run Side(tool=T1, surface=Well)".to_string()), "only the line that failed changes: {:?}", s.lines);
    assert!(s.notes.iter().any(|n| n.contains("Side on Post takes T5, the next endmill down: with T1,")), "{:?}", s.notes);
    let whole = format!("{src}{}\n", s.lines.join("\n"));
    let part = parse::parse(&whole).expect("the schedule parses");
    nacs::gcode::program_with_moves(&part).expect("and the program it makes runs");
}

#[test]
fn test_a_deep_hole_pecks() {
    let src = format!("{HEADER}{TOOLS}        feature Deep CylinderBore(dia=6.0, extent=30.5, x=30, y=30)
        feature Plain CylinderBore(dia=6.0, extent=30.0, x=60, y=30)
");
    let (_, s) = schedule_of(&src);
    assert_eq!(
        s.lines,
        vec!["run Form(points=[Plain], tool=T2, cycle=Simple)", "run Form(points=[Deep], tool=T2, cycle=Peck, peck_increment=6)"],
        "notes: {:?}",
        s.notes
    );
    let full = format!("{src}
{}", schedule::render(&s));
    let p = parse::parse(&full).unwrap_or_else(|e| panic!("the scheduled project must parse: {e}
{full}"));
    let g = gcode::program(&p).unwrap_or_else(|e| panic!("the scheduled project must emit: {e}"));
    assert!(g.contains("Z-6.000") && g.contains("Z-12.000"), "the pecks step down a diameter at a time: {g}");
}

#[test]
fn test_a_feature_a_run_already_names_is_left_alone() {
    let src = format!(
        "{HEADER}{TOOLS}{FEATURES}run Form(points=[Hole1], tool=T2, cycle=Simple)\nrun Clear(tool=T1, profile=Floor, stepover=4.0)\n"
    );
    let (_, s) = schedule_of(&src);
    assert!(s.lines.contains(&"run Form(points=[Hole2], tool=T2, cycle=Simple)".to_string()), "{:?}", s.lines);
    assert!(!s.lines.iter().any(|l| l.contains("Hole1") || l.contains("profile=Floor")), "{:?}", s.lines);
}

#[test]
fn test_without_tools_every_feature_waits_and_the_note_names_the_tool() {
    let src = format!("{HEADER}{FEATURES}");
    let (_, s) = schedule_of(&src);
    assert!(s.lines.is_empty(), "{:?}", s.lines);
    assert!(s.notes.contains(&"Hole1 waits for a tool: no Drill(dia=6) and no Endmill under 6 is declared".to_string()), "{:?}", s.notes);
    assert!(s.notes.contains(&"Pad waits for an Endmill: none is declared".to_string()), "{:?}", s.notes);
    assert!(s.notes.contains(&"Pad's chamfer waits for a ChamferMill(angle=90): none is declared".to_string()), "{:?}", s.notes);
    assert!(s.notes.contains(&"Floor waits for an Endmill: none is declared".to_string()), "{:?}", s.notes);
    // Nothing to write, something to say: the text is the notes alone.
    assert!(!schedule::render(&s).contains("scheduled by"), "{}", schedule::render(&s));
}

#[test]
fn test_a_line_the_parser_refuses_becomes_a_note_with_the_parsers_sentence() {
    // A 1.5 chamfer on a 1.0 wall: validate refuses the Form by name.
    let src = format!(
        "{HEADER}{TOOLS}feature Thin ProfileBoss(extent=1.0, chamfer_depth=1.5, chamfer_angle=90.0\n    20 100\n    60 100\n    60 60\n    20 60\n)\n"
    );
    let (_, s) = schedule_of(&src);
    // Standing on nothing cleared, the boss is roughed around in levels first.
    assert_eq!(
        s.lines,
        vec!["run Clear(tool=T1, profile=Thin, stepover=5, stepdown=5)", "run Side(tool=T1, surface=Thin)"],
        "notes: {:?}",
        s.notes
    );
    assert!(!s.lines.iter().any(|l| l.starts_with("run Form(")), "{:?}", s.lines);
    let note = s.notes.iter().find(|n| n.starts_with("not written, `run Form(tool=T3, surface=Thin)`: ")).unwrap_or_else(|| panic!("{:?}", s.notes));
    assert!(note.contains("deeper than the 1.000mm wall"), "{note}");
    // Still a project that parses with the schedule appended.
    let full = format!("{src}\n{}", schedule::render(&s));
    parse::parse(&full).unwrap_or_else(|e| panic!("{e}\n{full}"));
}

#[test]
fn test_a_given_hole_gets_only_its_mouth_chamfer() {
    let src = format!(
        "{HEADER}{TOOLS}feature Floor Plane(z=-10)\nfeature Left CylinderBore(dia=6.0, extent=30.0, x=30, y=30, chamfer_depth=1.0, chamfer_angle=90.0)\nfeature Right CylinderBore(dia=6.0, extent=30.0, x=60, y=30)\ngiven Left\ngiven Right\n"
    );
    let (_, s) = schedule_of(&src);
    assert_eq!(s.lines, vec!["run Clear(tool=T1, profile=Floor, stepover=5)", "run Form(tool=T3, surface=Left)"]);
    assert_eq!(
        s.notes,
        vec!["Left, Right are given: cut on the other setup and read as air here, so only their mouth chamfer or round is this setup's"]
    );
}

#[test]
fn test_a_chamfer_left_for_want_of_its_tool_is_asked_for_until_it_is_run() {
    let pad = "feature Pad ProfileBoss(extent=10.0, chamfer_depth=1.0, chamfer_angle=90.0
    20 100
    60 100
    60 60
    20 60
)
";
    let note = "Pad's chamfer waits for a ChamferMill(angle=90): none is declared";
    // No chamfer mill: the wall is scheduled and the chamfer noted.
    let src = format!("{HEADER}tool T1 Endmill(dia=10.0)
{pad}");
    let (_, s) = schedule_of(&src);
    assert!(!s.lines.is_empty() && s.notes.iter().any(|n| n == note), "{s:?}");
    // With the wall's runs written the note still stands.
    let cut = format!("{src}{}
", s.lines.join("
"));
    let (_, s) = schedule_of(&cut);
    assert_eq!((s.lines.len(), s.notes.iter().any(|n| n == note)), (0, true), "{s:?}");
    // The tool declared later gets the one line, and then nothing is left.
    let tooled = format!("{cut}tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)
");
    let (_, s) = schedule_of(&tooled);
    assert_eq!(s.lines, vec!["run Form(tool=T3, surface=Pad)"], "notes: {:?}", s.notes);
    let (_, s) = schedule_of(&format!("{tooled}{}
", s.lines[0]));
    assert!(s.lines.is_empty() && s.notes.is_empty(), "{s:?}");
}

#[test]
fn test_a_counterbore_over_a_drilled_hole_takes_its_wall_pass_alone() {
    // The ⌀10 endmill's wall pass covers the ring from ⌀26 in to ⌀6; the
    // ⌀18 hole below holds the rest. A blind ⌀26 bore keeps its Clear.
    let src = format!(
        "{HEADER}tool T1 Endmill(dia=10.0)\ntool T2 Drill(dia=18.0)\n\
         feature Through CylinderBore(dia=18.0, extent=30.0, x=50, y=60)\n\
         feature Seat CylinderBore(dia=26.0, extent=8.0, x=50, y=60)\n\
         feature Blind CylinderBore(dia=26.0, extent=8.0, x=150, y=60)\n"
    );
    let (_, s) = schedule_of(&src);
    assert_eq!(
        s.lines,
        vec![
            "run Form(points=[Through], tool=T2, cycle=Simple)",
            "run Side(tool=T1, surface=Seat, stepover=5, compress=arcs)",
            "run Clear(tool=T1, profile=Blind, stepover=5, compress=arcs)",
            "run Side(tool=T1, surface=Blind, stepover=5, compress=arcs)",
        ],
        "notes: {:?}",
        s.notes
    );
    let full = format!("{src}\n{}", schedule::render(&s));
    let p = parse::parse(&full).unwrap_or_else(|e| panic!("{e}\n{full}"));
    gcode::program(&p).unwrap_or_else(|e| panic!("{e}"));
}

/// The blank 1 mm above the part (slice 11): the top face leads, Planes at
/// one height share its one Clear, the bores placed at the part's top by z=
/// follow it, and the top Plane of the slice-10 plate, which lies at the
/// blank's top, stays a note — the rule reads the blank, not the plane.
#[test]
fn test_the_top_face_leads_and_planes_at_one_height_share_one_clear() {
    let src = format!(
        "{}{TOOLS}feature TopA Plane(z=0)\nfeature TopB Plane(z=0)\nfeature Hole1 CylinderBore(dia=6.0, extent=30.0, x=30, y=30, z=0.0)\nfeature Pad ProfileBoss(z=0.0, extent=10.0\n    20 100\n    60 100\n    60 60\n    20 60\n)\nfeature Floor Plane(z=-10)\n",
        HEADER.replace("stock Box(w=200, l=120, h=30, x=0, y=0, z=0)", "stock Box(w=200, l=120, h=31, x=0, y=0, z=1)")
    );
    let (_, s) = schedule_of(&src);
    assert_eq!(
        s.lines,
        vec![
            "run Clear(tool=T1, profile=TopA, stepover=5)",
            "run Form(points=[Hole1], tool=T2, cycle=Simple)",
            "run Clear(tool=T1, profile=Floor, stepover=5)",
            "run Side(tool=T1, surface=Pad)",
        ],
        "notes: {:?}",
        s.notes
    );
    assert!(s.notes.contains(&"TopB lies at TopA's height: TopA's Clear is the one pass over both".to_string()), "{:?}", s.notes);
    // A boss standing above the highest floor makes that floor a step, not
    // the top face: the drills keep the lead.
    let src = format!("{HEADER}{TOOLS}feature Hole1 CylinderBore(dia=6.0, extent=30.0, x=30, y=30)\nfeature Pad ProfileBoss(extent=10.0\n    20 100\n    60 100\n    60 60\n    20 60\n)\nfeature Shelf Plane(z=-5)\n");
    let (_, s) = schedule_of(&src);
    assert_eq!(s.lines[0], "run Form(points=[Hole1], tool=T2, cycle=Simple)", "{:?}", s.lines);
}

/// The blank's rim (slice 11): a round boss whose wall is the blank's own
/// edge has nothing around it to cut and gets a note; one standing within a
/// tool's width of the edge is swept by its wall pass alone, no Clear; one
/// further in is cleared around in levels first, as before.
#[test]
fn test_a_boss_at_the_blanks_rim_waits_and_one_inside_the_allowance_takes_its_wall_pass_alone() {
    // The blank's top at z=20 wants the rapids above it.
    let head = HEADER.replace("stock Box(w=200, l=120, h=30, x=0, y=0, z=0)", "stock Cylinder(dia=100, h=20, x=0, y=0, z=20)").replace("rapid_z 5\nclearance_z 25", "rapid_z 30\nclearance_z 40");
    let rim = "feature Rim CylinderBoss(dia=100.0, extent=20.0, x=0, y=0, z=0)\n";
    let (_, s) = schedule_of(&format!("{head}{TOOLS}{rim}"));
    assert!(s.lines.is_empty(), "{:?}", s.lines);
    assert_eq!(s.notes, vec!["Rim stands at the blank's rim: nothing to cut around it until the blank carries an allowance outside"]);
    let (_, s) = schedule_of(&format!("{}{TOOLS}{rim}", head.replace("dia=100", "dia=102")));
    assert_eq!(s.lines, vec!["run Side(tool=T1, surface=Rim, stepover=5, compress=arcs)"], "notes: {:?}", s.notes);
    let (_, s) = schedule_of(&format!("{}{TOOLS}{rim}", head.replace("dia=100", "dia=140")));
    assert_eq!(s.lines, vec!["run Clear(tool=T1, profile=Rim, stepover=5, stepdown=5)", "run Side(tool=T1, surface=Rim, stepover=5, compress=arcs)"], "notes: {:?}", s.notes);
}

/// The rings as arcs (slice 13): a Clear on a Plane works the blank's whole
/// plan, so over a round blank its rings are circles and the line carries
/// `compress=arcs`; over a box blank it does not. A Face's floor is its own
/// outline and keeps its chords either way.
#[test]
fn test_the_face_of_a_round_blank_is_written_as_arcs_and_a_box_blanks_is_not() {
    let head = HEADER.replace("stock Box(w=200, l=120, h=30, x=0, y=0, z=0)", "stock Cylinder(dia=100, h=20, x=0, y=0, z=20)").replace("rapid_z 5\nclearance_z 25", "rapid_z 30\nclearance_z 40");
    let src = format!("{head}{TOOLS}feature Top Plane(z=19)\n");
    let (_, s) = schedule_of(&src);
    assert_eq!(s.lines, vec!["run Clear(tool=T1, profile=Top, stepover=5, compress=arcs)"], "notes: {:?}", s.notes);
    let full = format!("{src}\n{}", schedule::render(&s));
    let p = parse::parse(&full).unwrap_or_else(|e| panic!("the schedule must parse with its project: {e}"));
    let g = gcode::program(&p).unwrap_or_else(|e| panic!("the scheduled project must emit: {e}"));
    assert!(g.lines().any(|l| l.starts_with("G02") || l.starts_with("G03")), "the facing's rings are arcs");
    let (_, s) = schedule_of(&format!("{HEADER}{TOOLS}feature Top Plane(z=-1)\n"));
    assert_eq!(s.lines, vec!["run Clear(tool=T1, profile=Top, stepover=5)"], "notes: {:?}", s.notes);
    let (_, s) = schedule_of(&format!("{head}{TOOLS}feature Top Face(z=19\n    loop\n        -20 -20\n    20 -20\n    20 20\n    -20 20\n)\n"));
    assert_eq!(s.lines, vec!["run Clear(tool=T1, profile=Top, stepover=5)"], "notes: {:?}", s.notes);
}

#[test]
fn test_the_cli_prints_the_schedule_and_says_when_there_is_nothing_to_write() {
    use std::process::Command;
    let dir = std::env::temp_dir().join("nacs_schedule_cli_test.cam");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.part"), format!("{HEADER}{TOOLS}{FEATURES}")).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--schedule", dir.to_str().unwrap()]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("run Form(points=[Hole1, Hole2], tool=T2, cycle=Simple)\n"), "{text}");
    assert!(text.ends_with("# not scheduled: Top lies at the blank's top: nothing to cut until the blank carries an allowance above it\n"), "{text}");
    // Appended, the project is scheduled through; the page then says so.
    let mut main = std::fs::read_to_string(dir.join("main.part")).unwrap();
    main.push('\n');
    main.push_str(&text);
    std::fs::write(dir.join("main.part"), main).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--schedule", dir.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    // The one feature with nothing to cut is still said; no line is offered twice.
    assert_eq!(text, "# not scheduled: Top lies at the blank's top: nothing to cut until the blank carries an allowance above it\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A second setup that says `given from` (slice 12): a void the earlier
/// program's air already holds whole is written `given`, not cut again, and
/// only its mouth chamfer gets a run; a void it cut short (the web) is not
/// given and takes its own line. The given lines come first, and the whole
/// text parses with the project and emits.
#[test]
fn test_a_void_the_earlier_program_cut_is_written_given_and_only_its_mouth_is_scheduled() {
    let front = std::fs::canonicalize("through_plate.cam").unwrap();
    let dir = std::env::temp_dir().join(format!("nacs_schedule_given_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=90, l=50, h=12, x=-45, y=-25, z=0)\n\
         given from {} up=z-\n\
         tool T1 Endmill(dia=8.0)\n\
         tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n\
         feature Left CylinderBore(dia=14.0, extent=12.0, x=-25.0, y=-6.0, chamfer_depth=1.0)\n\
         feature Mid CylinderBore(dia=14.0, extent=12.0, x=0.0, y=-6.0, chamfer_depth=1.0)\n\
         feature Short CylinderBore(dia=14.0, extent=12.0, x=25.0, y=-6.0, chamfer_depth=1.0)\n",
        front.to_string_lossy().replace('\\', "/")
    );
    std::fs::write(dir.join("main.part"), &src).unwrap();
    let p = parse::parse_project_with_base(&src, None, Some(&dir)).unwrap_or_else(|e| panic!("the project must parse: {e}"));
    let s = schedule::derive_schedule(&p, &src, None, Some(&dir));
    assert_eq!(
        s.lines,
        vec![
            "given Left",
            "given Mid",
            // The web under Short is steel to this program: the bore is milled
            // through it and chamfered like the others.
            "run Side(tool=T1, surface=Short, stepover=4, compress=arcs)",
            "run Form(tool=T3, surface=Left)",
            "run Form(tool=T3, surface=Mid)",
            "run Form(tool=T3, surface=Short)",
        ],
        "notes: {:?}",
        s.notes
    );
    assert!(
        s.notes.iter().any(|n| n.starts_with("Left, Mid lie in the earlier program's air (given from ") && n.ends_with("up=z-): written as given, not cut again, so only their mouth chamfer or round is this setup's")),
        "{:?}",
        s.notes
    );
    let full = format!("{src}\n{}", schedule::render(&s));
    let p = parse::parse_project_with_base(&full, None, Some(&dir)).unwrap_or_else(|e| panic!("the scheduled project must parse: {e}\n{full}"));
    assert_eq!(p.given, ["Left", "Mid"]);
    gcode::program(&p).unwrap_or_else(|e| panic!("the scheduled project must emit: {e}"));
    let _ = std::fs::remove_dir_all(&dir);
}
