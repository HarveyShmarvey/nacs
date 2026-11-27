use std::fs;
use std::process::Command;

// The oracle's interpreter: `python3` on Linux and macOS; `python` from the
// python.org installer on Windows, where a `python3.exe` ALSO exists as the
// Microsoft Store's install-prompt stub — it spawns fine and exits nonzero
// (the 2026-10-03 move to native Windows: the parity test FAILED instead of
// skipping). So a candidate counts only when `--version` succeeds, and the
// skip stays a skip on a box with no Python at all.
fn python() -> Option<&'static str> {
    ["python3", "python"]
        .into_iter()
        .find(|p| Command::new(p).arg("--version").output().map(|o| o.status.success()).unwrap_or(false))
}
use nacs::{parse, gcode, ops, stock};

#[test]
fn test_golden() {
    let main_src = fs::read_to_string("plate.cam/main.part").expect("failed to read main.part");
    let geom_src = fs::read_to_string("plate.cam/geometry.part").expect("failed to read geometry.part");
    let part = parse::parse_project(&main_src, Some(&geom_src)).expect("failed to parse plate.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/plate.golden.gcode").expect("failed to read golden G-code");

    // Compare trimmed outputs to ignore whitespace/newline differences at the end
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

/// The profile blank's golden (Phase 54 slice 5a): the End's rows over
/// the L and nothing else.
#[test]
fn test_stock_profile_golden() {
    let src = fs::read_to_string("stock_profile.cam/main.part").unwrap();
    let part = nacs::parse::parse_project_with_base(&src, None, Some(std::path::Path::new("stock_profile.cam"))).unwrap();
    let generated_gcode = nacs::gcode::program(&part).unwrap();
    let golden_gcode = fs::read_to_string("tests/stock_profile.golden.gcode").expect("failed to read stock_profile golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_floor_derive_golden() {
    // The K3 acceptance (DECISIONS.md → "K3, the material…", fork 4A): one
    // derived S word on one real op, behind the `spindle=derive` opt-in —
    // the shipped table's 6061×Endmill surface speed resolved to RPM at
    // parse, emitted through the unchanged S seam as M03 S4774.648.
    let main_src = fs::read_to_string("floor_derive.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse floor_derive.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/floor_derive.golden.gcode").expect("failed to read floor_derive golden G-code");

    assert!(generated_gcode.contains("M03 S4774.648"), "the derived S word must be emitted");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_floor_derive_feed_golden() {
    // The K3 slice-1 acceptance (DECISIONS.md → "K3, the material…",
    // slice 1): BOTH derived words on one real op — the tool carries K2's
    // flute_count, so the F half closes: F = RPM × z × f_z =
    // 4774.648… × 3 × 0.040 = 572.958, emitted through the unchanged F
    // seam. The sibling floor_derive.cam keeps deriving only S.
    let main_src = fs::read_to_string("floor_derive_feed.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse floor_derive_feed.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/floor_derive_feed.golden.gcode").expect("failed to read floor_derive_feed golden G-code");

    assert!(generated_gcode.contains("M03 S4774.648"), "the derived S word must be emitted");
    assert!(generated_gcode.contains("F572.958"), "the derived F word must be emitted");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_python_parity() {
    // No interpreter that runs: skip, not fail (see python() above).
    let Some(py) = python() else {
        println!("Skipping python parity test: no Python interpreter available");
        return;
    };

    let fixtures: &[(&str, Option<&str>)] = &[
        // The oracle ports the first raster, so it is held to the fixture
        // pinned to it (Phase 35 slice 4); `plate.cam` itself cuts the default.
        ("plate_raster.cam/main.part", Some("plate_raster.cam/geometry.part")),
        ("cylinder_bore_form_points.cam/main.part", None),
    ];

    for &(main_path, geom_path) in fixtures {
        let main_src = fs::read_to_string(main_path).expect("failed to read main.part");
        let geom_src = geom_path.map(|p| fs::read_to_string(p).expect("failed to read geometry"));
        let rust_part = parse::parse_project(&main_src, geom_src.as_deref()).expect("failed to parse");
        let rust_gcode = gcode::program(&rust_part).expect("failed to generate program");

        let project_dir = main_path.split('/').next().unwrap();
        // The oracle's TEXT is what is compared, not its platform's
        // encoding of it: a Windows Python writes a piped stdout in the
        // console code page (the header's em dash came back as cp1252's
        // 0x97) and ends every line CRLF — neither is a divergence of the
        // program, so UTF-8 mode is asked for and the line ends normalised
        // before the strings meet (the native-Windows move, 2026-10-04).
        let py_output = Command::new(py)
            .args(&["nacs.py", project_dir])
            .env("PYTHONUTF8", "1")
            .output()
            .expect("failed to execute python3 nacs.py");

        assert!(py_output.status.success(), "python3 nacs.py failed to execute");
        let py_gcode = String::from_utf8(py_output.stdout).expect("invalid utf8 from python output").replace("\r\n", "\n");

        assert_eq!(rust_gcode.trim(), py_gcode.trim(), "Parity mismatch for {}", project_dir);
    }
}

#[test]
fn test_python_oracle_rejects_out_of_scope_ops() {
    // The frozen oracle (DECISIONS.md "The parity oracle is frozen at Phase 1
    // scope") must hard-error on ops it can't correctly emit, rather than
    // silently print a plausible-looking but wrong/incomplete program.
    let Some(py) = python() else {
        println!("Skipping python oracle scope test: no Python interpreter available");
        return;
    };

    // Pocket depends on cavalier_contours, never ported to Python — must
    // error, not silently emit a program missing the pocket op.
    let pocket_output = Command::new(py)
        .args(&["nacs.py", "profile_pocket_clear.cam"])
        .output()
        .expect("failed to execute python3 nacs.py");
    assert!(!pocket_output.status.success(), "nacs.py must reject an unported Pocket op");

    // profile_pocket.cam uses the default comp=engine mode, which the oracle never
    // implemented (it only ever emits G41/G42) — must error, not silently
    // emit control-side G-code for an engine-mode request.
    let engine_comp_output = Command::new(py)
        .args(&["nacs.py", "profile_pocket.cam"])
        .output()
        .expect("failed to execute python3 nacs.py");
    assert!(!engine_comp_output.status.success(), "nacs.py must reject an unported comp=engine op");

    // mesh.cam's Mesh feature (camp 3) postdates the frozen oracle — it must
    // reject the unknown feature type, not emit a program missing the surface.
    let mesh_output = Command::new(py)
        .args(&["nacs.py", "mesh.cam"])
        .output()
        .expect("failed to execute python3 nacs.py");
    assert!(!mesh_output.status.success(), "nacs.py must reject an unported Mesh feature");

    // Same for the NurbsPatch (the analytic camp-3 sibling): unknown to the
    // frozen oracle, must reject rather than emit a program missing it.
    let patch_output = Command::new(py)
        .args(&["nacs.py", "nurbs_patch.cam"])
        .output()
        .expect("failed to execute python3 nacs.py");
    assert!(!patch_output.status.success(), "nacs.py must reject an unported NurbsPatch feature");

    // profile_pocket_rough.cam bakes a stock_to_leave into the emitted edge through
    // the offsetting crate. This is the one carrier the oracle can otherwise
    // reach — it accepts Contour, and plate.cam is a Contour in the parity
    // roster above — so without an explicit guard it would silently emit the
    // un-skinned path at the wrong size.
    let rough_output = Command::new(py)
        .args(&["nacs.py", "profile_pocket_rough.cam"])
        .output()
        .expect("failed to execute python3 nacs.py");
    assert!(!rough_output.status.success(), "nacs.py must reject a stock_to_leave it cannot bake");
    let stderr = String::from_utf8_lossy(&rough_output.stderr);
    assert!(
        stderr.contains("stock_to_leave"),
        "the error should name the field, got: {}", stderr
    );
}

#[test]
fn test_dry_renumbering() {
    let main_src = fs::read_to_string("plate.cam/main.part").expect("failed to read main.part");
    let geom_src = fs::read_to_string("plate.cam/geometry.part").expect("failed to read geometry.part");

    // DRY (derive_d, derive_h, derive_t: tool-derived values have one source)
    // Renumber tool 5 to tool 7 and assert every derived register moves with it.
    assert!(main_src.contains("T5"));
    let modified_src = main_src.replace("T5", "T7");

    let part = parse::parse_project(&modified_src, Some(&geom_src)).expect("failed to parse modified part");
    let gcode_output = gcode::program(&part).expect("failed to generate program");

    assert!(!gcode_output.contains("T5"), "stale T5 after rename");
    assert!(!gcode_output.contains("D5"), "stale D5 after rename (stale register bug)");
    assert!(gcode_output.contains("D7"), "D did not follow the tool to D7");
    assert!(!gcode_output.contains("H5"), "stale H5 after rename (stale register bug)");
    assert!(gcode_output.contains("H7"), "H did not follow the tool to H7");
}

#[test]
fn test_profile_pocket_clear_golden() {
    let main_src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse profile_pocket_clear.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/profile_pocket_clear.golden.gcode").expect("failed to read profile_pocket_clear golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_profile_wall_engine_golden() {
    // Engine comp (the default): the engine offsets the profile by the tool
    // radius and emits plain coordinates, no G41/G42.
    let main_src = fs::read_to_string("profile_pocket.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse profile_pocket.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/profile_pocket.golden.gcode").expect("failed to read profile_pocket golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    // Guard the mode's defining property: no controller-side cutter comp.
    assert!(!generated_gcode.contains("G41"), "engine comp must not emit G41");
    assert!(!generated_gcode.contains("G42"), "engine comp must not emit G42");
}

#[test]
fn test_profile_wall_engine_emits_arc() {
    // A concave profile's inward offset rounds the reflex corner into an arc.
    // Engine comp now emits a true G02/G03 for it (DECISIONS.md "Arc
    // emission…") instead of the earlier hard error — the physically correct
    // fillet a round tool leaves, not a gouging chord. Still no controller-side
    // comp (the mode's defining property).
    let main_src = fs::read_to_string("profile_pocket_arc.cam/main.part").expect("failed to read profile_pocket_arc.cam");
    let part = parse::parse(&main_src).expect("failed to parse");
    let g = gcode::program(&part).expect("engine comp must now emit the arc, not error");
    assert!(g.contains("G02") || g.contains("G03"), "a rounded offset must emit an arc move");
    assert!(g.contains(" I") && g.contains(" J"), "arc must carry incremental IJ center offsets");
    assert!(!g.contains("G41") && !g.contains("G42"), "engine comp must not emit controller-side comp");
}

#[test]
fn test_profile_pocket_arc_golden() {
    // Inside/concave case: the reflex corner rounds into one G02 fillet arc of
    // radius = tool radius, centered on the part corner (hand-verified in the
    // golden), the rest straight G01.
    let main_src = fs::read_to_string("profile_pocket_arc.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse profile_pocket_arc.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/profile_pocket_arc.golden.gcode").expect("failed to read profile_pocket_arc golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_profile_boss_arc_golden() {
    // Outside/convex case: the four corners round into four G03 quarter-circle
    // arcs of radius = tool radius, centered on the part corners — the opposite
    // direction word from the concave L-profile, so the pair pins both G02 and
    // G03.
    let main_src = fs::read_to_string("profile_boss_arc.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse profile_boss_arc.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/profile_boss_arc.golden.gcode").expect("failed to read profile_boss_arc golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_profile_wall_engine_untooled_is_an_error() {
    // A tool too large to leave any offset loop inside the profile can't be
    // finished — a hard error, never a silently skipped op.
    let main_src = fs::read_to_string("profile_pocket.cam/main.part").expect("failed to read main.part");
    let huge_tool = main_src.replace("dia=10.0", "dia=90.0");
    let part = parse::parse(&huge_tool).expect("failed to parse modified profile_pocket.cam");
    let result = gcode::program(&part);
    assert!(result.is_err(), "too-large tool must be a hard error, not a skipped op");
    assert!(result.unwrap_err().contains("engine finishing needs exactly one"), "error should say why");
}

#[test]
fn test_unmachinable_pocket_is_an_error() {
    // A pocket smaller than the tool must fail the whole program (nonzero
    // exit at the CLI), never emit a program with the op silently skipped —
    // DECISIONS.md "An unmachinable feature is a hard error".
    let main_src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("failed to read main.part");
    let huge_tool = main_src.replace("dia=10.0", "dia=90.0");
    let part = parse::parse(&huge_tool).expect("failed to parse modified profile_pocket_clear.cam");
    let result = gcode::program(&part);
    assert!(result.is_err(), "too-small pocket must be a hard error, not a skipped op");
    assert!(result.unwrap_err().contains("too small"), "error should say why");
}

// Phase 50 slice 1 (DECISIONS.md → "Phase 50 slice 1…"): the audit's two
// engine bugs, pinned. A project with no header — what the window's forms
// produce — is refused by name, field by field; a loop that crosses itself is
// refused at parse instead of panicking inside the offsetting crate; and
// `--help` is help.
const HEADERLESS_PLATE: &str = "stock Box(w=100, l=60, h=12)
tool T1 Endmill(dia=6)
tool T2 Drill(dia=6)
feature Pocket1 ProfilePocket(extent=6
    30 20
    70 20
    70 40
    30 40
)
feature H1 CylinderBore(dia=6, extent=12, x=10, y=10)
run Clear(tool=T1, profile=Pocket1, stepover=3)
run Form(points=[H1], tool=T2, cycle=Simple)
";

const PLATE_HEADER: &str = "units mm
feed 800
plunge 300
spindle 3000
rapid_z 5
clearance_z 25
";

#[test]
fn test_headerless_project_is_refused_field_by_field() {
    // Nothing declared: the feed is the first word the law reads.
    let err = parse::parse(HEADERLESS_PLATE).unwrap_err();
    assert!(err.contains("its feed is 0") && err.contains("declare `feed <mm/min>`"), "got: {err}");
    // Each word named in turn as the ones before it are declared.
    let with = |header: &str| parse::parse(&format!("{header}{HEADERLESS_PLATE}"));
    let err = with("feed 800\n").unwrap_err();
    assert!(err.contains("its plunge feed is 0"), "got: {err}");
    let err = with("feed 800\nplunge 300\n").unwrap_err();
    assert!(err.contains("its spindle is 0") && err.contains("M03 S0"), "got: {err}");
    let err = with("feed 800\nplunge 300\nspindle 3000\n").unwrap_err();
    assert!(err.contains("rapid_z 0.000mm is at or below the stock top (0.000mm)"), "got: {err}");
    let err = with("feed 800\nplunge 300\nspindle 3000\nrapid_z 5\n").unwrap_err();
    assert!(err.contains("clearance_z 0.000mm is at or below the stock top (0.000mm)"), "got: {err}");
    // The whole header: the program parses, and a spindle override of 0 on
    // one op is still that op's refusal.
    let part = with(PLATE_HEADER).expect("a full header parses");
    assert_eq!(part.operations.len(), 2);
    let zero_on_op = format!("{PLATE_HEADER}{HEADERLESS_PLATE}").replace("cycle=Simple)", "cycle=Simple, spindle=0)");
    let err = parse::parse(&zero_on_op).unwrap_err();
    assert!(err.starts_with("operation [1] (Form at H1): its spindle is 0"), "got: {err}");
}

#[test]
fn test_rapid_below_the_stock_top_is_refused() {
    // The shipped `through_plate.cam` carried `rapid_z 5` on a stock whose
    // top is z=12 for four days, and its golden rapided seven millimetres
    // into the plate before feeding UP to the top — the entry restriction
    // cannot see a rapid into the op's own material. The law names the top.
    let src = fs::read_to_string("through_plate.cam/main.part").expect("read");
    let bad = src.replace("rapid_z 17", "rapid_z 5");
    assert_ne!(src, bad, "the fixture declares rapid_z 17 now");
    let err = parse::parse(&bad).unwrap_err();
    assert!(err.contains("rapid_z 5.000mm is at or below the stock top (12.000mm)"), "got: {err}");
    // A per-op clearance below the top is the same refusal, on that op.
    let bad = src.replace("stepover=4.0)", "stepover=4.0, clearance_z=12)");
    let err = parse::parse(&bad).unwrap_err();
    assert!(err.starts_with("operation [0] (Clear on 'Left'): clearance_z 12.000mm is at or below the stock top (12.000mm)"), "got: {err}");
}

#[test]
fn test_degenerate_loops_are_refused_not_a_panic() {
    // The audit's own repro: a square with one vertex typed twice — a
    // zero-length edge. It passed validate (its area is a triangle's) and
    // reached the engagement lint's offsetter, whose spatial index asserted.
    let doubled = format!("{PLATE_HEADER}{HEADERLESS_PLATE}").replace("    70 40\n", "    70 20\n");
    let err = parse::parse(&doubled).unwrap_err();
    assert!(err.contains("ProfilePocket feature 'Pocket1' repeats a point: vertex 2 and vertex 3 are both (70.000, 20.000)"), "got: {err}");
    // Two vertices swapped make a bowtie (asymmetric, so its area is not 0 —
    // the symmetric bowtie is the zero-area refusal that already existed). Before the law this reached
    // `offset_shape` and the crate's spatial index asserted
    // `min_x <= max_x` — a panic, exit 101, where a sentence belonged.
    let bowtie = format!("{PLATE_HEADER}{HEADERLESS_PLATE}").replace("    70 20\n    70 40\n", "    70 45\n    70 20\n");
    let err = parse::parse(&bowtie).unwrap_err();
    assert!(err.contains("ProfilePocket feature 'Pocket1' crosses itself") && err.contains("vertex 1") && err.contains("vertex 3"), "got: {err}");
    // The same loop as a Face, and as a Contour in plan view.
    let face = format!("{PLATE_HEADER}stock Box(w=100, l=60, h=12)
tool T1 Endmill(dia=6)
feature F Face(z=0
loop
    30 20
    70 45
    70 20
    30 40
)
run End(tool=T1, surface=F, stepover=3)
");
    let err = parse::parse(&face).unwrap_err();
    assert!(err.contains("Face feature 'F': loop 1 crosses itself"), "got: {err}");
    let contour = format!("{PLATE_HEADER}stock Box(w=100, l=60, h=12)
tool T1 Endmill(dia=6)
feature C ContourPocket(extent=6
    30 20 0
    70 45 0
    70 20 0
    30 40 0
)
run Side(tool=T1, surface=C)
");
    let err = parse::parse(&contour).unwrap_err();
    assert!(err.contains("ContourPocket feature 'C' crosses itself in plan view"), "got: {err}");
    // A touch is a crossing too: a vertex of one edge resting on another.
    let touch = format!("{PLATE_HEADER}{HEADERLESS_PLATE}").replace("    70 40\n    30 40\n", "    70 40\n    50 20\n");
    let err = parse::parse(&touch).unwrap_err();
    assert!(err.contains("crosses itself"), "got: {err}");
}

#[test]
fn test_help_is_help() {
    // `--help` used to reach the path reader and be blamed as a missing
    // file. Now it is the usage on stdout, exit 0; the no-argument refusal
    // is the same text on stderr, exit 2.
    for flag in ["--help", "-h"] {
        let out = Command::new(env!("CARGO_BIN_EXE_nacs")).arg(flag).output().expect("run");
        assert!(out.status.success(), "{flag} should exit 0");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.starts_with("usage: nacs"), "got: {text}");
        assert!(text.contains("--view") && text.contains("--check") && text.contains("plate.nc"), "got: {text}");
    }
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).output().expect("run");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("usage: nacs"));
}

#[test]
fn test_dash_o_writes_exactly_what_stdout_carries() {
    // Phase 50 slice 2, the way out: `-o <file>` is stdout's bytes in a
    // file, through the atomic rename; stdout stays empty. A rejected
    // program writes nothing — no file appears — and `-o` beside a mode
    // that emits no program is a usage error.
    let dir = std::env::temp_dir().join(format!("nacs_dash_o_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    let out_file = dir.join("plate.nc");
    let plain = Command::new(env!("CARGO_BIN_EXE_nacs")).arg("plate.cam").output().expect("run");
    assert!(plain.status.success());
    let written = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["plate.cam", "-o", out_file.to_str().unwrap()]).output().expect("run");
    assert!(written.status.success(), "{}", String::from_utf8_lossy(&written.stderr));
    assert!(written.stdout.is_empty(), "-o leaves stdout empty");
    assert_eq!(fs::read(&out_file).expect("the file"), plain.stdout, "the file is stdout's bytes");
    // `--output` is the long spelling.
    let long = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--output", out_file.to_str().unwrap(), "plate.cam"]).output().expect("run");
    assert!(long.status.success());
    // A rejected program: nothing written, the error on stderr, exit 1.
    let bad = dir.join("headerless");
    fs::create_dir_all(&bad).expect("scratch project");
    fs::write(bad.join("main.part"), HEADERLESS_PLATE).expect("write");
    let refused_file = dir.join("refused.nc");
    let refused = Command::new(env!("CARGO_BIN_EXE_nacs")).args([bad.to_str().unwrap(), "-o", refused_file.to_str().unwrap()]).output().expect("run");
    assert_eq!(refused.status.code(), Some(1));
    assert!(!refused_file.exists(), "a refused program leaves no file");
    assert!(String::from_utf8_lossy(&refused.stderr).contains("its feed is 0"));
    // `-o` with a mode: a usage error, nothing written.
    let mixed = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--check", "plate.cam", "-o", dir.join("mixed.nc").to_str().unwrap()]).output().expect("run");
    assert_eq!(mixed.status.code(), Some(2));
    assert!(!dir.join("mixed.nc").exists());
    let missing = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["plate.cam", "-o"]).output().expect("run");
    assert_eq!(missing.status.code(), Some(2));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_init_writes_a_project_that_parses_and_never_writes_over_one() {
    // Phase 50 slice 3, the way in: `nacs --init '<spec>' <dir>` writes a
    // main.part with every value shown (stdout carries the text), creates
    // the directory, refuses an existing main.part by name, and refuses a
    // malformed spec as a usage error.
    let dir = std::env::temp_dir().join(format!("nacs_init_{}.cam", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--init", "Box w=100 l=60 h=12", dir.to_str().unwrap()]).output().expect("run");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = fs::read_to_string(dir.join("main.part")).expect("main.part written");
    assert_eq!(String::from_utf8_lossy(&out.stdout), text, "stdout shows exactly what was written");
    assert!(text.contains("rapid_z 5.000\nclearance_z 25.000\n\nstock Box(w=100, l=60, h=12)\n"));
    // It is a program the engine accepts, with nothing to cut yet.
    let check = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--check", dir.to_str().unwrap()]).output().expect("run");
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    // Never over an existing one.
    let again = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--init", "Box w=1 l=1 h=1", dir.to_str().unwrap()]).output().expect("run");
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("exists"));
    assert_eq!(fs::read_to_string(dir.join("main.part")).unwrap(), text, "untouched");
    // A malformed spec is a usage error; a refused value a refusal.
    let shape = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--init", "Box w", dir.to_str().unwrap()]).output().expect("run");
    assert_eq!(shape.status.code(), Some(2));
    let other = std::env::temp_dir().join(format!("nacs_init2_{}", std::process::id()));
    let _ = fs::remove_dir_all(&other);
    let value = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--init", "Box w=1 l=1", other.to_str().unwrap()]).output().expect("run");
    assert_eq!(value.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&value.stderr).contains("needs h"));
    assert!(!other.exists(), "a refused init creates nothing");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_reference_errors_say_where_and_what_is_declared() {
    // Phase 50 slice 7: the three reference refusals carry the run line
    // and the declared names; a missing stock line says so instead of
    // blaming its size.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read");
    let tool = parse::parse(&src.replace("tool=T5", "tool=T9")).unwrap_err();
    assert!(tool.starts_with("referenced tool T9 not defined (main.part line "), "{tool}");
    assert!(tool.ends_with("— declared: T5"), "{tool}");
    let feature = parse::parse(&src.replace("profile=PocketProfile", "profile=Nope")).unwrap_err();
    assert!(feature.starts_with("referenced feature 'Nope' not defined (main.part line "), "{feature}");
    assert!(feature.contains("— declared: 'PocketProfile'"), "{feature}");
    let visit = parse::parse(&format!("{PLATE_HEADER}{HEADERLESS_PLATE}").replace("points=[H1]", "points=[H9]")).unwrap_err();
    assert!(visit.starts_with("referenced feature 'H9' not defined (main.part line "), "{visit}");
    assert!(visit.contains("declared: 'H1', 'Pocket1'"), "{visit}");
    let no_tools = parse::parse(&src.replace("tool T5 Endmill(dia=10.0)", "")).unwrap_err();
    assert!(no_tools.contains("declared: none — a `tool T<n> <Kind>(dia=…)` line declares one"), "{no_tools}");
    let stockless: String = src.lines().filter(|l| !l.starts_with("stock ")).collect::<Vec<_>>().join("\n");
    let e = parse::parse(&stockless).unwrap_err();
    assert!(e.starts_with("no stock line"), "{e}");
    assert!(e.contains("stock Box(w=, l=, h=)"), "{e}");
}

#[test]
fn test_reference_prints_the_checked_in_file() {
    // Phase 50 slice 8: `nacs --reference` needs no project and prints the
    // generated reference; the checked-in REFERENCE.md is that text.
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).arg("--reference").output().expect("run");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(text.starts_with("# The `.part` language"));
    let file = fs::read_to_string("REFERENCE.md").expect("checked in").replace("\r\n", "\n");
    assert_eq!(text, file);
}

#[test]
fn test_import_runs_the_sidecar_or_names_the_setup() {
    // Phase 50 slice 10: `nacs --import <step> <dir>` runs the sidecar's
    // venv by its known path and writes report.json beside the project;
    // with no sidecar in reach it says so, exit 1, before anything runs.
    let scratch = std::env::temp_dir().join(format!("nacs_import_cli_{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();
    let step = fs::canonicalize("tests/fixtures/block_bore.step").expect("the sidecar's own fixture");
    let dir = scratch.join("block.cam");
    // Nowhere near a sidecar: the sentence names it.
    let none = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--import", step.to_str().unwrap(), dir.to_str().unwrap()])
        .current_dir(&scratch)
        .env_remove("NACS_SIDECAR")
        .output()
        .expect("run");
    assert_eq!(none.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&none.stderr).contains("no sidecar"), "{}", String::from_utf8_lossy(&none.stderr));
    // With the venv installed (the sidecar tests' own gate), it runs.
    let venv = if cfg!(windows) { "sidecar/.venv/Scripts/python.exe" } else { "sidecar/.venv/bin/python" };
    if std::path::Path::new(venv).is_file() {
        let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--import", step.to_str().unwrap(), dir.to_str().unwrap()]).output().expect("run");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(dir.join("report.json").is_file(), "the report beside the project");
        assert!(String::from_utf8_lossy(&out.stderr).contains("no main.part yet"), "a fresh directory is told what it still needs");
    }
    let bad = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--import", "tests/fixtures/block_bore.step", "--up", "sideways", dir.to_str().unwrap()]).output().expect("run");
    assert_eq!(bad.status.code(), Some(2));
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn test_cylinder_bore_form_points_golden() {
    let main_src = fs::read_to_string("cylinder_bore_form_points.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_bore_form_points.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore_form_points.golden.gcode").expect("failed to read drill golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_cylinder_bore_machine_golden() {
    // K1 acceptance (DECISIONS.md "K1, the machine…", fork 2A): a declared
    // machine with `spindle_word integer` changes exactly the S words —
    // `M03 S3000` at the tool change, bare `S2400` at the mid-run re-issue.
    // base_dir is the fixture dir so `machine fanuc.machine` resolves, same
    // as the SVG tool profile path.
    let main_src = fs::read_to_string("cylinder_bore_machine.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("cylinder_bore_machine.cam")))
        .expect("failed to parse cylinder_bore_machine.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore_machine.golden.gcode").expect("failed to read machine golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_undeclared_machine_twin_differs_only_in_the_s_words() {
    // The acceptance's other half, and the standing promise's sharpest pin:
    // the UNDECLARED twin — the same source minus its one `machine` line — is
    // byte-identical everywhere EXCEPT the spindle words, and those differ
    // only in spelling (S3000 vs S3000.000). This is the "everything else
    // byte-identical" clause of the ROADMAP K1 acceptance test, asserted
    // line by line rather than eyeballed.
    let main_src = fs::read_to_string("cylinder_bore_machine.cam/main.part").expect("failed to read main.part");
    let declared_part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("cylinder_bore_machine.cam")))
        .expect("failed to parse declared fixture");
    let declared = gcode::program(&declared_part).expect("failed to generate declared program");

    let twin_src: String = main_src
        .lines()
        .filter(|l| !l.trim_start().starts_with("machine "))
        .collect::<Vec<_>>()
        .join("\n");
    let twin_part = parse::parse(&twin_src).expect("failed to parse undeclared twin");
    assert!(twin_part.machine.is_none(), "the twin must be the undeclared machine");
    let undeclared = gcode::program(&twin_part).expect("failed to generate undeclared program");

    let d: Vec<&str> = declared.lines().collect();
    let u: Vec<&str> = undeclared.lines().collect();
    assert_eq!(d.len(), u.len(), "the declared machine must not add or drop a line");

    let mut s_diffs = 0;
    for (dl, ul) in d.iter().zip(u.iter()) {
        if dl == ul {
            continue;
        }
        // Every differing line is a spindle word — the tool-change `M03 S`
        // or the bare re-issue `S` — and the difference is the format alone:
        // the undeclared spelling is the declared one plus `.000`.
        assert!(
            ul.starts_with("M03 S") || ul.starts_with('S'),
            "a non-spindle line moved under the declared machine: '{}' vs '{}'",
            dl, ul
        );
        assert_eq!(
            format!("{}.000", dl), *ul,
            "the S word must differ in spelling only: '{}' vs '{}'",
            dl, ul
        );
        s_diffs += 1;
    }
    // Both emission sites: the tool-change block and the speed re-issue.
    assert_eq!(s_diffs, 2, "exactly the two spindle-word sites change");
    assert!(declared.lines().any(|l| l == "M03 S3000"), "the acceptance word, verbatim");
    assert!(undeclared.lines().any(|l| l == "M03 S3000.000"), "the undeclared twin's spelling");
}

#[test]
fn test_speed_class_formatter() {
    // The per-word-class split's speed class, pinned directly: `Integer`
    // rounds to the nearest whole RPM (2999.6 is 3000, never a truncated
    // 2999), and both `Decimal` and the undeclared machine keep today's
    // `{:.3}` spelling byte for byte.
    let fanuc = parse::Machine { spindle_word: parse::SpindleWord::Integer, ..parse::Machine::default() };
    assert_eq!(gcode::f_speed(3000.0, Some(&fanuc)), "3000");
    assert_eq!(gcode::f_speed(2999.6, Some(&fanuc)), "3000");
    assert_eq!(gcode::f_speed(3000.0, Some(&parse::Machine::default())), "3000.000");
    assert_eq!(gcode::f_speed(3000.0, None), "3000.000");
}

#[test]
fn test_output_class_formatters() {
    // The coordinate and feed classes' OUTPUT spellings (DECISIONS.md "K1
    // slice 1 — the formatting boundary…"), pinned directly: the undeclared
    // machine and the all-defaults declared machine both keep today's mm
    // `{:.3}` byte for byte, and an inch machine converts by exactly
    // MM_PER_INCH at its declared decimals — 25.4mm is 1.0000in, the pin
    // that would have caught the 25.4x error the old G21 comment guarded.
    let default = parse::Machine::default();
    assert_eq!(gcode::f_coord_out(50.0, None), "50.000");
    assert_eq!(gcode::f_coord_out(50.0, Some(&default)), "50.000");
    assert_eq!(gcode::f_feed_out(800.0, Some(&default)), "800.000");

    let inch = parse::parse_machine("units inch\nfeed_decimals 2\n").unwrap();
    assert_eq!(inch.coord_decimals, 4, "inch coordinates default to tenths of a thou");
    assert_eq!(gcode::f_coord_out(25.4, Some(&inch)), "1.0000");
    assert_eq!(gcode::f_coord_out(50.0, Some(&inch)), "1.9685");
    assert_eq!(gcode::f_feed_out(800.0, Some(&inch)), "31.50");

    // The classes genuinely split: the same value spells differently as a
    // coordinate and as a feed once the per-class decimals differ.
    assert_eq!(gcode::f_coord_out(800.0, Some(&inch)), "31.4961");
}

#[test]
fn test_cylinder_bore_inch_golden() {
    // K1 slice 1 acceptance (DECISIONS.md "K1 slice 1 — the formatting
    // boundary…"): a declared `units inch` machine emits a G20 program whose
    // every dimension word is inch at the declared precision. The anchors
    // are hand-derived, independent of the golden bytes: the bore's engine
    // CL flank 65mm → X2.5591, feed 800mm/min at the declared 2 decimals →
    // F31.50, the full-circle arc's I word -15mm → I-0.5906 (the IJ words
    // convert with the coordinates they steer), and the tool header's
    // D-register value — the number the operator DIALS — reads 0.3937, not
    // a mm value smuggled into an inch program.
    let main_src = fs::read_to_string("cylinder_bore_inch.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("cylinder_bore_inch.cam")))
        .expect("failed to parse cylinder_bore_inch.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore_inch.golden.gcode").expect("failed to read inch golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    assert!(generated_gcode.lines().any(|l| l == "G20"), "an inch machine's units word is G20");
    assert!(!generated_gcode.contains("G21"), "no metric units word survives");
    assert!(generated_gcode.contains("G01 X2.5591 Y1.9685 F31.50"), "coordinate at 4 inch decimals, feed at the declared 2");
    assert!(generated_gcode.contains("I-0.5906"), "the arc IJ words convert with their coordinates");
    assert!(generated_gcode.contains("(tool T5 dia 0.3937 -> comp register D5)"), "the operator-dialed register value speaks inch");
    assert!(generated_gcode.contains("M03 S3000.000"), "an RPM has no length unit: the S word is untouched by units");
}

#[test]
fn test_inch_machine_twin_converts_numbers_never_structure() {
    // The slice-0 twin discipline, extended to the formatting class: the
    // UNDECLARED twin (the same source minus its `machine` line) emits a
    // program with the same line count and, line for line, the same word
    // skeleton — every word's letter and every non-numeric word identical,
    // only the numerals differing. Conversion changes numbers, never
    // structure. The S words are byte-identical between the two (this
    // machine leaves `spindle_word` at its default), pinning that the
    // respell classifies by letter and never touches a unit-free word.
    let skeleton = |line: &str| -> String {
        line.split(' ')
            .map(|w| {
                if w.parse::<f64>().is_ok() {
                    "#".to_string()
                } else if w.is_ascii() && w.len() >= 2 && w[1..].parse::<f64>().is_ok() {
                    w[..1].to_string()
                } else {
                    w.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };

    let main_src = fs::read_to_string("cylinder_bore_inch.cam/main.part").expect("failed to read main.part");
    let declared_part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("cylinder_bore_inch.cam")))
        .expect("failed to parse declared fixture");
    let declared = gcode::program(&declared_part).expect("failed to generate declared program");

    let twin_src: String = main_src
        .lines()
        .filter(|l| !l.trim_start().starts_with("machine "))
        .collect::<Vec<_>>()
        .join("\n");
    let twin_part = parse::parse(&twin_src).expect("failed to parse undeclared twin");
    assert!(twin_part.machine.is_none(), "the twin must be the undeclared machine");
    let undeclared = gcode::program(&twin_part).expect("failed to generate undeclared program");

    let d: Vec<&str> = declared.lines().collect();
    let u: Vec<&str> = undeclared.lines().collect();
    assert_eq!(d.len(), u.len(), "the inch machine must not add or drop a line");
    for (dl, ul) in d.iter().zip(u.iter()) {
        if *ul == "G21" {
            assert_eq!(*dl, "G20", "the units word flips in place");
            continue;
        }
        assert_eq!(skeleton(dl), skeleton(ul), "conversion must not change a line's structure: '{}' vs '{}'", dl, ul);
        if ul.starts_with("M03 S") || ul.starts_with('S') {
            assert_eq!(dl, ul, "the S word has no length unit and must be byte-identical");
        }
    }
}

#[test]
fn test_cylinder_bore_inch_input_golden() {
    // The inch INPUT half of the one-in-one-out law, end to end — the golden
    // the ROADMAP's inch entry said the input path was one away from. An
    // inch-authored program (`units inch`, no machine) converts once at the
    // parse boundary and emits plain metric G21: the 0.5in tool prints as
    // dia 12.700, the 1.5in bore at (2in, 2in) finishes its engine CL flank
    // at X63.500, and the 32 in/min feed rides as F812.800.
    let main_src = fs::read_to_string("cylinder_bore_inch_input.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_bore_inch_input.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore_inch_input.golden.gcode").expect("failed to read inch-input golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    assert!(generated_gcode.lines().any(|l| l == "G21"), "an undeclared machine's output stays metric");
    assert!(generated_gcode.contains("(tool T5 dia 12.700"), "0.5in tool converts at the parse boundary");
    assert!(generated_gcode.contains("G01 X63.500 Y50.800 F812.800"), "50.8 + 38.1/2 - 12.7/2, at 32in/min");
}

#[test]
fn test_sphere_golden() {
    // Camp-2 ball-nose waterline finishing of a Sphere (ROADMAP Phase 3).
    let main_src = fs::read_to_string("sphere.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse sphere.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/sphere.golden.gcode").expect("failed to read sphere golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_sphere_scallop_golden() {
    let main_src = fs::read_to_string("sphere_scallop.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse sphere_scallop.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/sphere_scallop.golden.gcode").expect("failed to read sphere_scallop golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_cone_golden() {
    let main_src = fs::read_to_string("cone.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cone.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cone.golden.gcode").expect("failed to read cone golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_cone_scallop_golden() {
    let main_src = fs::read_to_string("cone_scallop.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cone_scallop.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cone_scallop.golden.gcode").expect("failed to read cone_scallop golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_finish_requires_ball_nose() {
    // The waterline math offsets one ball radius along the surface normal, so a
    // flat endmill would gouge the sphere — reject it rather than emit a bad path.
    let main_src = fs::read_to_string("sphere.cam/main.part").expect("failed to read sphere.cam");
    let flat = main_src.replace("Ballmill", "Endmill");
    let result = parse::parse(&flat);
    assert!(result.is_err(), "Finish with a non-ball tool must be a hard error");
    assert!(result.unwrap_err().contains("Ballmill"), "error should say why");
}

#[test]
fn test_sphere_formtool_golden() {
    let main_src = fs::read_to_string("sphere_formtool.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("sphere_formtool.cam")))
        .expect("failed to parse sphere_formtool.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/sphere_formtool.golden.gcode").expect("failed to read sphere_formtool golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_sphere_formtool_band_golden() {
    // K2 slice 2, fork 3A on disk: the same dome finish with the cutting
    // band declared covering the contact span — the default said out loud,
    // pinned as byte-identical to the undeclared twin's golden both by its
    // own golden file and by direct equality below.
    let main_src = fs::read_to_string("sphere_formtool_band.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("sphere_formtool_band.cam")))
        .expect("failed to parse sphere_formtool_band.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/sphere_formtool_band.golden.gcode").expect("failed to read band golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    let base_golden = fs::read_to_string("tests/sphere_formtool.golden.gcode").expect("failed to read base golden");
    assert_eq!(golden_gcode, base_golden, "a covering band is the default said out loud — the twins' goldens are one file's bytes twice");
}

#[test]
fn test_cylinder_bore_rest_flutes_golden() {
    // K2 slice 2, fork 5A on disk: the rest-selection family under a prefix
    // whose rod declares where its flutes end. The certifier declines every
    // loop — the audit line says so in program text — where the undeclared
    // twin (the same source minus flute_len) certifies and drops two.
    let main_src = fs::read_to_string("cylinder_bore_rest_flutes.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_bore_rest_flutes.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore_rest_flutes.golden.gcode").expect("failed to read rest-flutes golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert!(generated_gcode.contains("(rest: kept 7 of 7 loops"), "the decline must be stated in program text");

    let twin = parse::parse(&main_src.replace(", flute_len=12.0", "")).expect("the undeclared twin parses");
    let twin_gcode = gcode::program(&twin).expect("the twin generates");
    assert!(
        twin_gcode.contains("(rest: kept 5 of 7 loops"),
        "the undeclared twin must still certify and drop — or this fixture demonstrates nothing"
    );
}

#[test]
fn test_torus_golden() {
    let main_src = fs::read_to_string("torus.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse torus.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/torus.golden.gcode").expect("failed to read torus golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_torus_scallop_golden() {
    let main_src = fs::read_to_string("torus_scallop.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse torus_scallop.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/torus_scallop.golden.gcode").expect("failed to read torus_scallop golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_cylinder_bore_golden() {
    // Exact-flank finishing of a cylindrical bore wall (CHECKLIST 2.3, ROADMAP
    // Phase 3.5): coaxial tool-centre circle per Z stepdown, both comp modes.
    let main_src = fs::read_to_string("cylinder_bore.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_bore.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore.golden.gcode").expect("failed to read cylinder golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_cylinder_wall_takes_endmill_not_ballmill() {
    // The flank cut wants a straight-sided endmill, so CylinderBore is exempt from
    // the ball-nose requirement the curved quadrics enforce — an Endmill parses.
    let main_src = fs::read_to_string("cylinder_bore.cam/main.part").expect("failed to read cylinder_bore.cam");
    parse::parse(&main_src).expect("Finish on a CylinderBore with an Endmill must be accepted");
}

#[test]
fn test_cylinder_wall_rejects_scallop() {
    // A straight flank has no scallop; stepover here is the axial stepdown, and
    // asking for scallop spacing is a hard error, not a silent reinterpretation.
    let main_src = fs::read_to_string("cylinder_bore.cam/main.part").expect("failed to read cylinder_bore.cam");
    let scalloped = main_src.replace("stepover=5.0", "scallop=0.5");
    let result = parse::parse(&scalloped);
    assert!(result.is_err(), "scallop on a CylinderBore flag wall must be a hard error");
    assert!(result.unwrap_err().contains("scallop"), "error should say why");
}

#[test]
fn test_cylinder_wall_tool_too_large_is_an_error() {
    // dia must exceed the tool strictly: a tool as wide as (or wider than) the
    // bore leaves a zero-radius or self-intersecting tool-centre path — a hard
    // error, not a degenerate/gouging pass.
    let main_src = fs::read_to_string("cylinder_bore.cam/main.part").expect("failed to read cylinder_bore.cam");
    let big_tool = main_src.replace("dia=10.0", "dia=40.0"); // tool dia == bore dia
    let result = parse::parse(&big_tool);
    assert!(result.is_err(), "tool as wide as the bore must be a hard error");
    assert!(result.unwrap_err().contains("larger than tool"), "error should say why");
}

#[test]
fn test_cylinder_boss_golden() {
    // The circular outside boss (CHECKLIST 2.4, DECISIONS.md "Row-2.4 `side`
    // selector"): finish_cylinder's mirror — tool-centre at R + r_tool, cut CW,
    // both comp modes — plus the first committed CW/outside Contour pass.
    let main_src = fs::read_to_string("cylinder_boss.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_boss.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_boss.golden.gcode").expect("failed to read boss golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    // The engine-comp section must carry no controller comp; the control
    // sections put the tool outside via G41 + CW travel (the blessed winding
    // rule), never G42.
    let engine_section = generated_gcode.split("(control)").next().unwrap();
    assert!(!engine_section.contains("G41"), "engine comp must not emit G41");
    assert!(!generated_gcode.contains("G42"), "side is selected by winding under G41, not by switching to G42");
}

// ---- compress=arcs: exact-circle output for circular rings -------------------
// DECISIONS.md "Per-op compress: arcs-first toggle committed". Each fixture is
// its chorded sibling re-emitted under `compress=arcs`, so the ring polylines
// collapse to single exact G02/G03 full-circle blocks. The default (`chords`)
// is byte-identical to before the field existed — pinned implicitly by every
// existing golden staying green, and explicitly by test_compress_chords_default.

#[test]
fn test_sphere_arcs_golden() {
    // The waterline path (generate_passes): every constant-Z ring of the
    // hemisphere is a circle, so each becomes one G03 full-circle block
    // (start == end, centre I = -rho J = 0). CCW, so G03 and never G02.
    let main_src = fs::read_to_string("sphere_arcs.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse sphere_arcs.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/sphere_arcs.golden.gcode").expect("failed to read sphere_arcs golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_cylinder_bore_arcs_golden() {
    // The bore wall (cylinder_engine + cylinder_control): coaxial rings emit as
    // G03 (CCW, inside), under both comp modes.
    let main_src = fs::read_to_string("cylinder_bore_arcs.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_bore_arcs.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore_arcs.golden.gcode").expect("failed to read cylinder_bore_arcs golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_cylinder_boss_arcs_golden() {
    // The boss wall (boss_engine + boss_control): coaxial rings emit as G02
    // (CW, outside — the boss's blessed winding), the opposite direction word
    // from the bore, under both comp modes.
    let main_src = fs::read_to_string("cylinder_boss_arcs.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_boss_arcs.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_boss_arcs.golden.gcode").expect("failed to read cylinder_boss_arcs golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

#[test]
fn test_compress_arcs_emits_exact_full_circle() {
    // The defining behaviour: a circular ring becomes one true arc carrying
    // incremental IJ centre offsets — not a chord polyline — and the winding
    // word matches the cut direction (bore CCW = G03, boss CW = G02). The arc
    // is exact, so the full circle is one block (start == end), and there is one
    // per Z level.
    let bore_src = fs::read_to_string("cylinder_bore_arcs.cam/main.part").expect("read");
    let bore = gcode::program(&parse::parse(&bore_src).expect("parse")).expect("gen");
    assert!(bore.contains(" I") && bore.contains(" J"), "arc must carry incremental IJ centre offsets");
    assert!(bore.contains("G03 X65.000 Y50.000 I-15.000 J0.000"), "engine bore ring must be one CCW full-circle arc at rho = R - r_tool");
    assert!(!bore.contains("G02"), "an inside bore is cut CCW — never G02");

    let boss_src = fs::read_to_string("cylinder_boss_arcs.cam/main.part").expect("read");
    let boss = gcode::program(&parse::parse(&boss_src).expect("parse")).expect("gen");
    assert!(boss.contains("G02 X75.000 Y50.000 I-25.000 J0.000"), "engine boss ring must be one CW full-circle arc at rho = R + r_tool");
    assert!(!boss.contains("G03"), "an outside boss is cut CW — never G03");
}

#[test]
fn test_compress_chords_default_is_unchanged() {
    // Omitting `compress` and writing `compress=chords` must both reproduce the
    // committed chorded golden byte-for-byte: the default emits what shipped
    // before the field existed.
    let default_src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let default_g = gcode::program(&parse::parse(&default_src).expect("parse")).expect("gen");
    let explicit_src = default_src.replace("stepover=3.0)", "stepover=3.0, compress=chords)");
    let explicit_g = gcode::program(&parse::parse(&explicit_src).expect("parse")).expect("gen");
    assert_eq!(default_g, explicit_g, "compress=chords must equal the implicit default");
    assert!(!default_g.contains("G02") && !default_g.contains("G03"), "the chorded default emits no arcs");
}

#[test]
fn test_compress_reserved_modes_are_rejected() {
    // `force` and `spline` are reserved vocabulary: recognized so the error
    // names them as not-yet-built, but selecting them is a hard error.
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    for mode in ["force", "spline"] {
        let bad = src.replace("stepover=3.0)", &format!("stepover=3.0, compress={})", mode));
        let err = parse::parse(&bad).expect_err("reserved compress mode must be rejected");
        assert!(err.contains("reserved"), "error must name it reserved, got: {}", err);
        assert!(err.contains(mode), "error should echo the mode, got: {}", err);
    }
}

#[test]
fn test_compress_unknown_mode_is_rejected() {
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let bad = src.replace("stepover=3.0)", "stepover=3.0, compress=bezier)");
    let err = parse::parse(&bad).expect_err("unknown compress mode must be rejected");
    assert!(err.contains("unknown compress mode"), "error should say why, got: {}", err);
}

// ---- stock_to_leave: the rough/finish first slice ---------------------------
// (DECISIONS.md "stock_to_leave first slice…"): presence of the value is the
// roughing tag; the allowance is a guarantee-ceiling on TOTAL remaining
// material including the pass's own cusps (max, not min), so the cusp budget
// is spent out of the number before any nominal offset remains.

#[test]
fn test_sphere_rough_golden() {
    // Scallop-mode roughing of the sphere_scallop sphere: cusp budget = the
    // scallop (0.2 of the 0.5 allowance), nominal skin 0.3 — the pass
    // machines the inflated sphere r=20.3, apex CL at Z = 20.3 + 3 = 23.3.
    let main_src = fs::read_to_string("sphere_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse sphere_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    // Hand-checked anchors: the apex plunge and the audited budget split.
    assert!(generated.contains("G01 Z23.300 F200.000"), "apex CL must sit at 20.3 + r_ball");
    assert!(generated.contains("[stock_to_leave 0.500mm, cusp budget 0.200mm]"), "title must audit the budget split");
    let golden = fs::read_to_string("tests/sphere_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

// ---- the prismatic allowance carriers ---------------------------------------
// DECISIONS.md "stock_to_leave is a radial allowance…". These two carriers are
// the first with NO new cusp math: the budget is structurally zero on both, so
// what needs pinning is the offset arithmetic, the D-register discipline, and
// the errors.

#[test]
fn test_profile_pocket_clear_rough_golden() {
    // The whole allowance is radial: the spiral starts at r + skin = 5.5, so
    // the wall pass (emitted LAST — pocket machines innermost-out) runs
    // 25.5..74.5 against profile_pocket_clear.cam's 25..75. The floor is untouched at
    // Z-5.000: `extent` is the depth, and a radial allowance never moves it.
    let main_src = fs::read_to_string("profile_pocket_clear_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse profile_pocket_clear_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    assert!(generated.contains("G01 X74.500 Y25.500 F800.000"), "the wall pass must ride the skin parallel");
    assert!(generated.contains("G00 X49.500 Y49.500"), "the innermost loop shifts with the whole spiral");
    assert!(generated.contains("G01 Z-5.000 F300.000"), "the floor stays at the feature's own extent");
    assert!(generated.contains("[stock_to_leave 0.500mm, cusp budget 0.000mm]"), "one Z and a flat tool: no cusp to budget");
    let golden = fs::read_to_string("tests/profile_pocket_clear_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

#[test]
fn test_profile_pocket_rough_golden() {
    // Both comp modes on one CCW (inside) profile. Engine emits the tool centre
    // at r + skin = 5.5 inward; control emits the PART EDGE shrunk by the skin
    // alone, with D still the pure tool radius — the finish_cylinder precedent,
    // which is what makes a double-comp unrepresentable.
    let main_src = fs::read_to_string("profile_pocket_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse profile_pocket_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    assert!(generated.contains("G01 X25.500 Y25.500 F800.000"), "engine path must carry r + skin");
    assert!(generated.contains("G01 G41 D5 X20.500 Y20.500 F800.000"), "control edge carries the skin; D stays pure");
    assert!(!generated.contains("D6"), "the allowance must never reach the D register");
    let golden = fs::read_to_string("tests/profile_pocket_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

#[test]
fn test_profile_rough_outside_shifts_outward() {
    // The sign, on the other winding: profile_boss.cam's Outline is a ProfileBoss, so the
    // tool cuts OUTSIDE it and leaving stock moves the edge AWAY from centre —
    // the opposite direction from profile_pocket_rough.cam's CCW window. A sign error
    // here would be a gouge, not a size error, so it gets its own pin.
    let src = fs::read_to_string("profile_boss.cam/main.part").expect("read profile_boss.cam");
    let rough = src.replace("surface=Outline, comp=control)", "surface=Outline, comp=control, stock_to_leave=0.5)");
    let part = parse::parse(&rough).expect("must parse");
    let g = gcode::program(&part).expect("must generate");
    // Authored 10..90; the material is inside, so the edge grows to 9.5..90.5.
    assert!(g.contains("X9.500"), "a CW profile's edge must grow outward, got:\n{}", g);
    assert!(g.contains("X90.500"), "a CW profile's edge must grow outward, got:\n{}", g);
}

#[test]
fn test_profile_rough_arc_radii_grow_with_the_skin() {
    // Corner fillets are r + skin, not r. Nothing in the emitter hardcodes a
    // radius — it reads the bulge — so this is really a pin that the fattened
    // offset flows through arc emission unchanged: profile_boss_arc.cam's
    // four quarter-circles go from IJ magnitude 5.0 to 5.5.
    let src = fs::read_to_string("profile_boss_arc.cam/main.part").expect("read fixture");
    let plain = gcode::program(&parse::parse(&src).unwrap()).expect("must generate");
    let rough_src = src.replace("surface=Pad)", "surface=Pad, stock_to_leave=0.5)");
    let rough = gcode::program(&parse::parse(&rough_src).unwrap()).expect("must generate");
    assert!(plain.contains("I5.000") || plain.contains("J5.000"), "baseline fillet is the tool radius");
    assert!(rough.contains("I5.500") || rough.contains("J5.500"), "roughed fillet is r + skin, got:\n{}", rough);
}

#[test]
fn test_pocket_rough_too_small_for_tool_plus_skin_is_refused() {
    // The tool fits; tool + allowance does not. Saying "too small for the tool"
    // would send the author after the wrong knob.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    // A 6mm-wide pocket takes the 10mm tool's radius but not radius + 8.
    let tight = src
        .replace("    20 20\n    80 20\n    80 80\n    20 80", "    20 20\n    26 20\n    26 26\n    20 26")
        .replace("stepover=4.0)", "stepover=4.0, stock_to_leave=8.0)");
    let part = parse::parse(&tight).expect("must parse");
    let err = gcode::program(&part).expect_err("must reject");
    assert!(err.contains("plus the stock_to_leave offset"), "error must name the allowance, got: {}", err);
    assert!(err.contains("reduce stock_to_leave"), "error must offer the right knob, got: {}", err);
}

#[test]
fn test_pocket_stepover_at_or_over_diameter_is_refused() {
    // Adjacent loops would not overlap: an uncut ridge of full-depth stock
    // between every pass. Always-on — the ridge is there with or without an
    // allowance, and the emitter's flat-floor claim is false either way.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let bad = src.replace("stepover=4.0)", "stepover=10.0)");
    let err = parse::parse(&bad).expect_err("stepover >= dia must be rejected");
    assert!(err.contains("uncut ridge"), "error should name the failure, got: {}", err);
    assert!(err.contains("reduce stepover"), "error should offer the knob, got: {}", err);
}

/// The End's ceiling is the Clear's (Phase 43 slice 4): a stepover at or
/// over the tool's diameter is refused at validate for every op that
/// works a floor, not only the offset family at emit.
#[test]
fn test_end_stepover_at_or_over_diameter_is_refused() {
    let src = |stepover: &str| {
        format!(
            "units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
             stock Box(w=100, l=80, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=10.0)
             feature Deck Plane(z=-2)
run End(tool=T5, surface=Deck, stepover={stepover}, pattern=zigzag)
"
        )
    };
    let err = parse::parse(&src("10.0")).expect_err("an End's stepover >= dia must be rejected");
    assert!(err.contains("End operation stepover") && err.contains("uncut strip"), "error should name the failure, got: {}", err);
    assert!(err.contains("reduce stepover"), "error should offer the knob, got: {}", err);
    assert!(parse::parse(&src("9.999")).is_ok(), "a hair under the diameter is accepted");
}

/// The ramp's law (Phase 44 slices 1 and 2): over every fixture whose ops
/// are all floor ops, under any pattern, no cutting move that descends is
/// steeper than `RAMP_DEG`, and every pure descent is at the plunge feed.
/// The corpus used to carry 32° (the plain pocket's 8mm first loop), 89°
/// (the wrench's 0.05mm innermost loop, three lines at the cutting feed;
/// the zigzag's 0.01mm first stroke, one line) and 87° (the rails' chord
/// fallbacks under spine).
#[test]
fn test_no_entry_into_solid_is_steeper_than_the_ramp_law() {
    let word = |line: &str, w: char| -> Option<f64> {
        line.split('(').next().unwrap_or("").split_whitespace().find_map(|t| t.strip_prefix(w).and_then(|v| v.parse().ok()))
    };
    let mut checked = 0;
    for entry in fs::read_dir(".").expect("read repo root") {
        let path = entry.expect("dir entry").path();
        if !path.extension().map(|e| e == "cam").unwrap_or(false) {
            continue;
        }
        let src = fs::read_to_string(path.join("main.part")).expect("read fixture");
        let geom = fs::read_to_string(path.join("geometry.part")).ok();
        let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)).unwrap_or_else(|e| panic!("{}: parse failed: {}", path.display(), e));
        // Every op that works a floor under any pattern (slice 2 widened
        // it from the offset family); a fixture with a contact verb or a
        // visit is out, since those descend along surfaces by design.
        let in_scope = part.operations.iter().all(|op| parse::derive_pattern(&part, op).is_some());
        if !in_scope {
            continue;
        }
        let program = gcode::program(&part).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        let (mut x, mut y, mut z): (Option<f64>, Option<f64>, Option<f64>) = (None, None, None);
        for (n, line) in program.lines().enumerate() {
            let code = line.split('(').next().unwrap_or("").trim();
            if code.is_empty() {
                continue;
            }
            let (nx, ny, nz) = (word(code, 'X').or(x), word(code, 'Y').or(y), word(code, 'Z').or(z));
            if code.starts_with("G01") {
                if let (Some(x0), Some(y0), Some(z0), Some(x1), Some(y1), Some(z1)) = (x, y, z, nx, ny, nz) {
                    if z1 < z0 - 1e-9 {
                        let dxy = (x1 - x0).hypot(y1 - y0);
                        if dxy > 1e-9 {
                            // Judged in millimetres, with one quantum of the
                            // three-decimal Z word allowed: a 0.09mm chord
                            // drops 0.005mm at the law, and `f` rounds that.
                            let allowed = dxy * ops::clear::RAMP_DEG.to_radians().tan() + 0.0015;
                            let deg = (z0 - z1).atan2(dxy).to_degrees();
                            assert!(z0 - z1 <= allowed, "{} line {}: a descent at {deg:.1}deg over {dxy:.3}mm — steeper than the law ({}deg): {line}", path.display(), n + 1, ops::clear::RAMP_DEG);
                        } else {
                            let feed = word(code, 'F').expect("a pure descent carries its feed word");
                            assert!((feed - part.global_plunge).abs() < 1e-9 || part.operations.iter().any(|op| op.plunge.is_some_and(|pl| (pl - feed).abs() < 1e-9)), "{} line {}: a pure descent at the cutting feed: {line}", path.display(), n + 1);
                        }
                    }
                }
            }
            (x, y, z) = (nx, ny, nz);
        }
        checked += 1;
    }
    assert!(checked >= 20, "the offset family's corpus is in scope: {checked}");
}

#[test]
fn test_pocket_uncut_island_is_refused() {
    // Distinct from the ridge above: stepover (5) is UNDER the diameter (6),
    // yet the spiral still outruns the shape, because it stops when the next
    // offset prunes empty rather than when coverage runs out. Loops land at
    // offset 3 only; reach is 3 + 3 = 6 against a 7mm inradius, so a 1mm-wide
    // full-depth island stands at the centre. This shipped silently before.
    let src = "\
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=40, l=40, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=6.0)
feature Island ProfilePocket(extent=5.0
    10 10
    24 10
    24 24
    10 24
)
run Clear(tool=T5, profile=Island, stepover=5.0)
";
    // (Until Phase 27 this pocket was drawn at the blank's own corner, 0..14.
    // Two of its edges lay ON the stock's boundary, which the region now
    // reads as OPEN — a corner notch, entered from air, a different shape with
    // a different family. The CLOSED pocket this test is about sits inside.)
    let part = parse::parse(src).expect("must parse");
    let err = gcode::program(&part).expect_err("an uncut island must be rejected");
    assert!(err.contains("uncut island"), "error should name the failure, got: {}", err);
    assert!(err.contains("reduce stepover"), "error should offer the knob, got: {}", err);
}

#[test]
fn test_prismatic_ops_require_a_flat_bottom_tool() {
    // Always-on, both carriers. The floor scallop is the smaller half: a ball
    // on a VERTICAL WALL at a single Z cuts full width only at r above the
    // floor, leaving a fillet the single-Z path never removes — which breaks
    // the ceiling on the surface an allowance is actually about.
    for (fixture, run_from, run_to) in [
        ("profile_pocket_clear.cam/main.part", "tool=T5,", "tool=T9,"),
        ("profile_pocket.cam/main.part", "tool=T5,", "tool=T9,"),
    ] {
        let src = fs::read_to_string(fixture).expect("read fixture");
        let bad = src
            .replace("tool T5 Endmill(dia=10.0)", "tool T5 Endmill(dia=10.0)\ntool T9 Ballmill(dia=6.0)")
            .replace(run_from, run_to);
        let err = parse::parse(&bad).expect_err("a Ballmill on a prismatic op must be rejected");
        assert!(err.contains("flat-bottomed tool"), "{fixture}: error should say what is needed, got: {}", err);
        assert!(err.contains("fillet"), "{fixture}: error should name the wall failure, got: {}", err);
    }
}

#[test]
fn test_stock_to_leave_lint_fires_on_a_pocket() {
    // parse::lint was always op-agnostic; opening the gate makes it reachable
    // from the prismatic carriers for the first time. Pinned so that reads as
    // deliberate rather than as an accident of where the check happened to sit.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let heavy = src.replace("stepover=4.0)", "stepover=4.0, stock_to_leave=5.0)");
    let part = parse::parse(&heavy).expect("a heavy skin is legal, just loud");
    let warnings = parse::lint(&part);
    assert!(
        warnings.iter().any(|w| w.message.contains("heavy roughing skin")),
        "the heavy-skin lint should reach a Pocket, got: {:?}", warnings
    );
}

#[test]
fn test_cylinder_bore_rough_golden() {
    // Flank roughing: axial stepdown leaves no cusp, so the budget is exactly
    // zero and the whole 0.5 allowance is radial skin. Engine ring
    // 20 − 5 − 0.5 = 14.5 (X64.5); control emits the ROUGHED wall (19.5,
    // X69.5) as its part edge with the D register still the pure tool radius.
    let main_src = fs::read_to_string("cylinder_bore_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_bore_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    assert!(generated.contains("G01 X64.500 Y50.000 F800.000"), "engine ring must shrink by the skin");
    assert!(generated.contains("G01 G41 D5 X69.500 Y50.000 F800.000"), "control edge must be the roughed wall, D untouched");
    assert!(generated.contains("[stock_to_leave 0.500mm, cusp budget 0.000mm]"), "flank cusp budget is exactly zero, stated");
    let golden = fs::read_to_string("tests/cylinder_bore_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

#[test]
fn test_torus_rough_golden() {
    // Scallop-mode roughing of the first convex-mirror carrier (DECISIONS.md
    // "stock_to_leave convex mirrors…"): the torus budget is the sphere
    // recipe on the tube meridian. Cusp budget = the scallop (0.2 of 0.5),
    // nominal skin 0.3, every CL ring 7.3mm (= 4 + 3 + 0.3) off the tube's
    // centre circle — crown ring at z=7.300, radius 25 (X75.000),
    // hand-checked.
    let main_src = fs::read_to_string("torus_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse torus_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    assert!(generated.contains("G00 X75.000 Y40.000"), "crown ring must sit at the major radius");
    assert!(generated.contains("G01 Z7.300 F200.000"), "crown plunge must ride the inflated tube");
    assert!(generated.contains("[stock_to_leave 0.500mm, cusp budget 0.200mm]"), "title must audit the budget split");
    let golden = fs::read_to_string("tests/torus_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

#[test]
fn test_torus_rough_stepdown_ridge_is_refused() {
    // torus.cam's own 3.0mm stepdown against the 7mm nominal offset circle:
    // fine as a finishing pass, but as a roughing pass the crown pair's
    // half-chord (7·sin(θ_max/2) = 3.24) exceeds the 3mm ball radius —
    // adjacent balls never intersect, an uncut RIDGE no allowance can
    // ceiling. The INFINITY path through derive_cusp_height.
    let src = fs::read_to_string("torus.cam/main.part").expect("read torus.cam");
    let bad = src.replace("stepover=3.0)", "stepover=3.0, stock_to_leave=0.5)");
    let part = parse::parse(&bad).expect("parse");
    let err = gcode::program(&part).expect_err("an uncut ridge must reject");
    assert!(err.contains("uncut ridge"), "error should name the ridge, got: {}", err);
    assert!(err.contains("crown ring"), "error should place the worst pair, got: {}", err);
    assert!(err.contains("reduce stepover or increase stock_to_leave"), "error should be actionable, got: {}", err);
}

#[test]
fn test_cone_rough_golden() {
    // Scallop-mode roughing of the first sharp-apex carrier (DECISIONS.md
    // "stock_to_leave corner roll…"): cusp budget = the scallop (0.2 of
    // 0.5), nominal skin 0.3, every CL point r_ball + 0.3 = 3.3mm off the
    // nominal cone. Under stock the grid gains the cap/flank seam as a pass
    // and re-grids the cap by psi: pole pass dead on the axis at
    // z = 20 + 3.3 = 23.300, seam pass at z = 20 + 3.3·sin45° = 22.333 with
    // X = 50 + 3.3·cos45° = 52.333 — the hand-checked anchors.
    let main_src = fs::read_to_string("cone_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cone_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    assert!(generated.contains("G00 X50.000 Y40.000"), "the sweep must start on the axis");
    assert!(generated.contains("G01 Z23.300 F200.000"), "pole plunge must ride the inflated corner sphere");
    assert!(generated.contains("G01 Z22.333 F200.000"), "the cap/flank seam must be its own pass");
    assert!(generated.contains("[stock_to_leave 0.500mm, cusp budget 0.200mm]"), "title must audit the budget split");
    let golden = fs::read_to_string("tests/cone_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

#[test]
fn test_spline_cap_rough_golden() {
    // Scallop-mode roughing of the USER-AUTHORED meridian (DECISIONS.md
    // "stock_to_leave mixed meridians…"): the march's osculating triangle is
    // only the spacing heuristic — the budget is the exact tip walk over the
    // march's own grid, never the scallop by fiat.
    //
    // The number this pins moved with the tip cap (DECISIONS.md "Family-wide
    // march cap…"): the walk used to price 0.205mm, the 0.2 scallop target
    // PLUS the across-step curvature drift the triangle couldn't see, and the
    // roughing pass paid for that drift out of its ceiling. The capped march
    // holds 0.200, so the 0.005 goes back to the nominal skin — a' = 0.300
    // where it was 0.295, and the part is cut 5µm closer to size for the same
    // stated allowance. First CL point on the tilted apex normal (1,3)/√10 at
    // r_ball + a' = 3.300 → (1.044, 23.131), hand-checked.
    let main_src = fs::read_to_string("spline_cap_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("spline_cap_rough.cam")))
        .expect("failed to parse spline_cap_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    assert!(generated.contains("G00 X51.044 Y40.000"), "first CL must ride the tilted apex normal at the skin");
    assert!(generated.contains("G01 Z23.131 F200.000"), "apex plunge must ride the skin up");
    assert!(generated.contains("[stock_to_leave 0.500mm, cusp budget 0.200mm]"), "title must audit the VERIFIED budget — capped at the scallop, no drift surcharge");
    let golden = fs::read_to_string("tests/spline_cap_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

#[test]
fn test_spline_funnel_rough_golden() {
    // Stepover-mode roughing down the spline funnel — the LAST axisymmetric
    // refusal closed (DECISIONS.md "stock_to_leave inside funnels…"): the
    // uniform-arc grid measured on the a'-INFLATED meridian (into-void
    // side), priced by the funnel exact tip walk, guarded by the
    // two-direction scan's _with_stock form. Budget 0.402mm of 0.500 at a
    // 3.0mm stepover, skin a' = 0.098: mouth CL on the end-tangent
    // into-void normal (−3,1)/√10 at r_ball + a' = 3.098 → (10.061, 0.980)
    // (X60.061 / Z0.980), tip CL (2.061, −17.020) — hand-checked.
    let main_src = fs::read_to_string("spline_funnel_rough.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("spline_funnel_rough.cam")))
        .expect("failed to parse spline_funnel_rough.cam");
    let generated = gcode::program(&part).expect("failed to generate program");
    assert!(generated.contains("G00 X60.061 Y40.000"), "mouth ring CL must ride the end-tangent normal at the skin");
    assert!(generated.contains("G01 Z0.980"), "mouth plunge must ride the inflated offset");
    assert!(generated.contains("X52.061") && generated.contains("Z-17.020"), "final ring must clamp to the inflated tip CL");
    assert!(generated.contains("[stock_to_leave 0.500mm, cusp budget 0.402mm]"), "title must audit the VERIFIED budget");
    let golden = fs::read_to_string("tests/spline_funnel_rough.golden.gcode").expect("failed to read golden G-code");
    assert_eq!(generated.trim(), golden.trim());
}

#[test]
fn test_cone_rough_stepdown_ridge_is_refused() {
    // A 4.5mm stepdown on the 45° flank spaces adjacent CL points
    // 4.5/cos45° = 6.36mm apart — beyond the 6mm ball diameter, so adjacent
    // balls never intersect: an uncut RIDGE no allowance can ceiling. The
    // INFINITY path through derive_cusp_height_flat.
    let src = fs::read_to_string("cone.cam/main.part").expect("read cone.cam");
    let bad = src.replace("stepover=3.0)", "stepover=4.5, stock_to_leave=0.5)");
    let part = parse::parse(&bad).expect("parse");
    let err = gcode::program(&part).expect_err("an uncut ridge must reject");
    assert!(err.contains("uncut ridge"), "error should name the ridge, got: {}", err);
    assert!(err.contains("reduce stepover or increase stock_to_leave"), "error should be actionable, got: {}", err);
}

#[test]
fn test_cone_rough_cusp_exceeding_allowance_is_refused() {
    // Max-not-min on the flat flank: cone.cam's 3.0mm stepdown leaves an
    // exact flat-chord cusp of 3 − sqrt(9 − (3/cos45°/2)²) = 0.879mm on the
    // 45° wall — skin-independent, and alone outspending a 0.5mm ceiling.
    let src = fs::read_to_string("cone.cam/main.part").expect("read cone.cam");
    let bad = src.replace("stepover=3.0)", "stepover=3.0, stock_to_leave=0.5)");
    let part = parse::parse(&bad).expect("parse");
    let err = gcode::program(&part).expect_err("a cusp above the ceiling must reject");
    assert!(err.contains("worst-case cusp"), "error should price the cusp, got: {}", err);
    assert!(err.contains("max, not min"), "error should restate the semantics, got: {}", err);
}

#[test]
fn test_cusp_height_flat_is_the_convex_ceiling() {
    // The corner-roll slice's load-bearing lemma, pinned both ways: at a
    // fixed CL chord the convex cusp price rises with the surface radius,
    // so the flat limit ceilings every convex pair (and a sharp corner's
    // pairs price near zero — the reason flat-bounding them over-rejects);
    // and the flat price round-trips the flat-scallop chord exactly
    // (s = 2·sqrt(2·r·h − h²) ⟹ flat cusp = h), the identity finish_cone's
    // stock-mode scallop re-grid leans on.
    let (r_ball, d) = (3.0, 2.0);
    let flat = parse::derive_cusp_height_flat(d, r_ball);
    let mut last = 0.0;
    for r_part in [0.0, 0.5, 2.0, 10.0, 100.0] {
        let theta = 2.0 * (d / (2.0 * (r_part + r_ball))).asin();
        let h = parse::derive_cusp_height(theta, r_ball, r_part);
        assert!(h >= last - 1e-12, "price must rise with radius at fixed chord: {h} after {last}");
        assert!(h <= flat + 1e-12, "the flat price must ceiling radius {r_part}: {h} vs {flat}");
        last = h;
    }
    let scallop = 0.2;
    let s = 2.0 * (2.0 * r_ball * scallop - scallop * scallop).sqrt();
    assert!((parse::derive_cusp_height_flat(s, r_ball) - scallop).abs() < 1e-12, "flat-scallop chord must round-trip");
    assert!(parse::derive_cusp_height_flat(2.0 * r_ball, r_ball).is_infinite(), "non-overlapping balls must be a ridge");
}

// ---- `extent`: vertical occupancy belongs to the feature ---------------------
// DECISIONS.md "Vertical occupancy is `extent`…". The five tests below pin the
// whole vocabulary: the default datum, both errors on the field itself, an
// authored datum, and the retirement of `depth` as an operation input.

#[test]
fn test_profile_extent_defaults_its_datum_to_the_stock_top() {
    // The migration's byte-identity claim, stated as an equation rather than
    // left to the goldens: an absent `z` means the stock top, so a 5mm extent
    // on a stock topped at z=0 cuts at -5 — exactly where `depth=5.0` used to.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let part = parse::parse(&src).expect("profile_pocket_clear.cam must parse");
    let g = gcode::program(&part).expect("profile_pocket_clear.cam must generate");
    assert!(g.contains("Z-5.000"), "the floor should sit at the stock top less the extent");
}

#[test]
fn test_profile_extent_zero_is_rejected_and_names_the_reserved_meaning() {
    // 0 is not "no wall" — it is reserved for geometry that carries its own Z
    // range (a 3D wire). Refusing it now keeps that door open: the day such a
    // feature exists, `extent=0` means "add no extrusion", not "cut nothing".
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let bad = src.replace("ProfilePocket(extent=5.0", "ProfilePocket(extent=0");
    let err = parse::parse(&bad).expect_err("extent=0 must be rejected");
    assert!(err.contains("reserved"), "error should name the reserved meaning, got: {}", err);
    assert!(err.contains("3D-wire"), "error should name what reserves it, got: {}", err);
}

#[test]
fn test_profile_extent_negative_is_rejected() {
    // Extent is occupancy, not direction: a wall runs downward from its datum
    // by construction, so a negative one is a category error, not a flip.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let bad = src.replace("ProfilePocket(extent=5.0", "ProfilePocket(extent=-5.0");
    let err = parse::parse(&bad).expect_err("negative extent must be rejected");
    assert!(err.contains("must be positive"), "error should say why, got: {}", err);
}

#[test]
fn test_profile_authored_z_moves_the_wall_off_the_stock_top() {
    // The datum is authorable, which is the half a `depth` measured from the
    // stock top could never express: a pocket whose mouth is already 3mm down.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let moved = src
        .replace("ProfilePocket(extent=5.0", "ProfilePocket(z=-3.0, extent=5.0")
        // A mouth 3mm down has 3mm of stock above it, and the entry restriction
        // rejects a program that plunges through material no op removes
        // (DECISIONS.md "The entry restriction refuses the program…") — so the
        // authored datum needs the deck taken down to meet it. That is the
        // check working, not a workaround: without this the tool would drill
        // its way in through solid stock.
        .replace("feature PocketProfile", "feature Deck Plane(z=-3.0)\nfeature PocketProfile")
        .replace("run Clear", "run End(tool=T5, surface=Deck, stepover=6.0)\nrun Clear");
    let part = parse::parse(&moved).expect("an authored datum must parse");
    let g = gcode::program(&part).expect("must generate");
    assert!(g.contains("Z-8.000"), "floor should be the authored datum less the extent");
    assert!(!g.contains("Z-5.000"), "the stock-top default must not survive an authored z");
}

#[test]
fn test_profile_winding_mismatch_is_refused_pointing_at_the_other_variant() {
    // Side-of-material is the variant's; the winding is redundant data
    // validated against it. A mismatch is rejected, never silently reversed —
    // reversing the traversal would flip climb/conventional on the machine.
    // Relabeling a fixture's variant leaves its loop wound for the other side,
    // which is exactly the mismatch: a CCW loop declared as a boss…
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let ccw_boss = src.replace("ProfilePocket(", "ProfileBoss(");
    let err = parse::parse(&ccw_boss).expect_err("a CCW ProfileBoss must be rejected");
    assert!(err.contains("winds counter-clockwise"), "error should name the winding, got: {}", err);
    assert!(err.contains("ProfilePocket"), "error should point at the other variant, got: {}", err);

    // …and a CW loop declared as a pocket.
    let boss_src = fs::read_to_string("profile_boss_arc.cam/main.part").expect("read profile_boss_arc.cam");
    let cw_pocket = boss_src.replace("ProfileBoss(", "ProfilePocket(");
    let err = parse::parse(&cw_pocket).expect_err("a CW ProfilePocket must be rejected");
    assert!(err.contains("winds clockwise"), "error should name the winding, got: {}", err);
    assert!(err.contains("ProfileBoss"), "error should point at the other variant, got: {}", err);
}

#[test]
fn test_retired_op_words_are_refused_with_teaching_pointers() {
    // The op verbs settled with the vocabulary migration: Contour is absorbed
    // into Finish, Pocket is renamed Clear. Both retired words reject with
    // their replacement rather than failing as unknown.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let contour_op = src.replace("run Side(", "run Contour(");
    let err = parse::parse(&contour_op).expect_err("the retired Contour op must be rejected");
    assert!(err.contains("tracing a wall is side cutting") && err.contains("run Side("), "error should teach the merge, got: {}", err);

    let clear_src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let pocket_op = clear_src.replace("run Clear(", "run Pocket(");
    let err = parse::parse(&pocket_op).expect_err("the retired Pocket op must be rejected");
    assert!(err.contains("renamed Clear"), "error should teach the rename, got: {}", err);
    assert!(err.contains("orientation word"), "error should say why, got: {}", err);
}

#[test]
fn test_profile_wall_finish_refuses_spacing_params() {
    // The polyline wall is one full-depth trace today: an authored spacing
    // would be silently ignored, so it is rejected instead — stepover pointing
    // at omission, scallop at the flank's nature.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let stepped = src.replace("surface=Window)", "surface=Window, stepover=3.0)");
    let err = parse::parse(&stepped).expect_err("stepover on a profile wall must be rejected");
    assert!(err.contains("full-depth trace"), "error should say why, got: {}", err);

    let scalloped = src.replace("surface=Window)", "surface=Window, scallop=0.5)");
    let err = parse::parse(&scalloped).expect_err("scallop on a profile wall must be rejected");
    assert!(err.contains("scallop spacing does not apply"), "error should say why, got: {}", err);
}

#[test]
fn test_profile_zero_area_loop_is_refused() {
    // A degenerate (collinear) loop bounds nothing — rejected at validation
    // regardless of variant.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let flat = src
        .replace("    80 80", "    80 20")
        .replace("    20 80", "    20 20");
    let err = parse::parse(&flat).expect_err("a zero-area loop must be rejected");
    assert!(err.contains("zero signed area"), "error should name the degeneracy, got: {}", err);
}

#[test]
fn test_contour_wire_goldens() {
    // The 3D-wire pair, slice 1 (DECISIONS.md "the 3D-wire Contour…"): both
    // comp modes per fixture, hand-checked anchors in the fixture comments.
    // The boss golden also pins the constant-Z fillet arcs (a corner's arc at
    // its corner's own Z) and extent=0 — the reserved semantic, reachable.
    // The rough golden pins slice 2: the allowance through the projection
    // offset in both comp modes, riding the same Z re-attachment.
    for fixture in ["contour_pocket", "contour_boss", "contour_pocket_rough"] {
        let src = fs::read_to_string(format!("{fixture}.cam/main.part")).expect("read fixture");
        let part = parse::parse(&src).expect("wire fixture must parse");
        let generated = gcode::program(&part).expect("wire fixture must generate");
        let golden = fs::read_to_string(format!("tests/{fixture}.golden.gcode")).expect("read golden");
        assert_eq!(generated.trim(), golden.trim(), "{fixture} golden drifted");
    }
}

#[test]
fn test_contour_wire_engine_reattaches_z_through_the_offset() {
    // The slice's technical core, asserted directly rather than only through
    // the golden bytes: the offsetter is 2D, so engine comp must re-attach Z.
    let src = fs::read_to_string("contour_pocket.cam/main.part").expect("read contour_pocket.cam");
    let part = parse::parse(&src).expect("parse");
    let g = gcode::program(&part).expect("generate");
    // Inward offset corner 5mm along both walls from the wire corner: the
    // corner's z -/+ 5·(2/60), minus extent 3 (the fixture's anchor math).
    assert!(g.contains("G01 X25.000 Y25.000 F800.000"), "engine offset lands on the sharp inner square");
    assert!(g.contains("X75.000 Y25.000 Z-4.833"), "the offset corner near (80,20,-2) re-attaches 1/6 above its corner");
    assert!(g.contains("X25.000 Y25.000 Z-3.167"), "the offset corner near (20,20,0) re-attaches 1/6 below its corner");
    // Control mode needs no re-attachment: the authored edge with Z words.
    assert!(g.contains("G01 G41 D5 X20.000 Y20.000 F800.000"), "control comp rides the part edge");
    assert!(g.contains("X80.000 Y20.000 Z-5.000"), "control comp carries the wire's own z minus extent");
}

#[test]
fn test_contour_wire_boss_fillet_arcs_are_constant_z_at_the_corner() {
    // An outward offset rounds each corner into an arc whose BOTH endpoints
    // re-attach through the two segments meeting at that corner — one shared
    // vertex, one Z — so the arc emits with no Z word (modal Z holds) at the
    // corner's own height. extent=0: the pass rides the wire itself.
    let src = fs::read_to_string("contour_boss.cam/main.part").expect("read contour_boss.cam");
    let part = parse::parse(&src).expect("parse");
    let g = gcode::program(&part).expect("generate");
    // The (70,30,z=-3) corner: enter at Z-3.000, arc at constant Z.
    assert!(g.contains("G01 X70.000 Y25.000 Z-3.000"), "the straight side ends at the corner's own z");
    assert!(g.contains("G03 X75.000 Y30.000 I0.000 J5.000"), "the corner rounds into a true arc");
    // A side between corners at different heights interpolates linearly —
    // the wall's own top edge, exactly (linear in the segment parameter).
    assert!(g.contains("G01 X75.000 Y70.000 Z-1.000"), "the side climbs to the next corner's z");
}

#[test]
fn test_contour_wire_winding_law_and_point_arity() {
    let src = fs::read_to_string("contour_pocket.cam/main.part").expect("read contour_pocket.cam");
    // The row-2.4 law on the projection: a CW wire declared as a pocket is
    // rejected pointing at the other variant, never silently reversed.
    let cw = src
        .replace("    80 20 -2", "    20 80 -2")
        .replace("    20 80 -2\n)", "    80 20 -2\n)");
    // (swap two vertices to flip the winding; the first replace hits the
    // earlier line, the second restores the pair in reversed order)
    let err = parse::parse(&cw).expect_err("a CW pocket wire must be rejected");
    assert!(err.contains("ContourBoss"), "the error points at the other variant, got: {}", err);
    // A wire point without its Z is a loud arity error, not a shifted parse.
    let flat = src.replace("    20 20 0", "    20 20");
    let err = parse::parse(&flat).expect_err("a 2-number wire point must be rejected");
    assert!(err.contains("3 numbers"), "the error names the arity, got: {}", err);
    // And a planar profile point with a stray third number stays rejected.
    let profile_src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let stray = profile_src.replace("    20 20", "    20 20 0");
    let err = parse::parse(&stray).expect_err("a 3-number profile point must be rejected");
    assert!(err.contains("2 numbers"), "the error names the arity, got: {}", err);
}

#[test]
fn test_contour_wire_extent_zero_is_legal_and_negative_is_not() {
    // extent=0 is THE reserved semantic, legal exactly here (contour_boss.cam
    // ships it); negative stays a category error.
    let src = fs::read_to_string("contour_boss.cam/main.part").expect("read contour_boss.cam");
    let bad = src.replace("ContourBoss(extent=0", "ContourBoss(extent=-1");
    let err = parse::parse(&bad).expect_err("negative wire extent must be rejected");
    assert!(err.contains("must not be negative"), "got: {}", err);
    // A z datum on the opener contradicts the feature's whole point.
    let datum = src.replace("ContourBoss(extent=0", "ContourBoss(z=-1, extent=0");
    let err = parse::parse(&datum).expect_err("a z datum on a wire must be rejected");
    assert!(err.contains("no z datum"), "got: {}", err);
}

#[test]
fn test_contour_wire_op_arms_refuse_what_does_not_apply() {
    let src = fs::read_to_string("contour_pocket.cam/main.part").expect("read contour_pocket.cam");
    // Spacing does not apply to a full-height trace.
    let spaced = src.replacen("run Side(tool=T5, surface=Rim)", "run Side(tool=T5, surface=Rim, stepover=2.0)", 1);
    let err = parse::parse(&spaced).expect_err("stepover on a wire must be rejected");
    assert!(err.contains("takes no stepover"), "got: {}", err);
    // A ball would leave a fillet the full-height wall claim contradicts.
    let ball = src.replace("tool T5 Endmill(dia=10.0)", "tool T5 Ballmill(dia=10.0)");
    let err = parse::parse(&ball).expect_err("a Ballmill on a wire wall must be rejected");
    assert!(err.contains("use an Endmill"), "got: {}", err);
    // Clear has no interior to empty — the rim declares a wall.
    let clear = src.replacen("run Side(tool=T5, surface=Rim)", "run Clear(tool=T5, profile=Rim, stepover=4.0)", 1);
    let err = parse::parse(&clear).expect_err("Clear on a wire must be rejected");
    assert!(err.contains("declares a wall, not an interior"), "got: {}", err);
}

#[test]
fn test_contour_wire_rough_rides_the_z_reattachment() {
    // The budget slice (DECISIONS.md "the 3D-wire Contour…", slice 2): the
    // allowance is radial (cusp budget structurally 0.000, the planar wall's
    // budget) and the edge it fattens re-attaches Z at the OFFSET vertex's
    // own plan position — so every emitted Z moves with the offset distance
    // (the fixture's hand-checked anchors, both comp modes).
    let src = fs::read_to_string("contour_pocket_rough.cam/main.part").expect("read fixture");
    let part = parse::parse(&src).expect("parse");
    let g = gcode::program(&part).expect("generate");
    // Engine: r + skin = 5.5 inward; every corner is 5.5mm along both walls
    // from its wire corner, so its top edge sits 11/60 from that corner's z.
    assert!(g.contains("G01 X25.500 Y25.500 F800.000"), "engine path must carry r + skin");
    assert!(g.contains("X74.500 Y25.500 Z-4.817"), "the deep corner re-attaches 11/60 above its own z");
    assert!(g.contains("X25.500 Y25.500 Z-3.183"), "the shallow corner re-attaches 11/60 below its own z");
    // Control: the part edge shrunk by the skin alone, D still the pure
    // radius; its Z re-attaches at the SHRUNK corner (1/60 from the wire
    // corner) — no longer the authored point's own z, which is what the
    // zero-skin path emits.
    assert!(g.contains("G01 G41 D5 X20.500 Y20.500 F800.000"), "control edge carries the skin; D stays pure");
    assert!(g.contains("X79.500 Y20.500 Z-4.983"), "the control edge's Z rides the re-attachment too");
    assert!(!g.contains("D6"), "the allowance must never reach the D register");
}

#[test]
fn test_contour_wire_boss_control_skin_rounds_corners_constant_z() {
    // A boss's skin offset rounds outward corners into fillets of radius =
    // skin — the first control-mode edge with arcs baked in — so pin that
    // they emit constant-Z at their corner's own z (modal Z holds through
    // the arc, the engine mode's guarantee now holding on the skinned edge).
    let src = fs::read_to_string("contour_boss.cam/main.part").expect("read contour_boss.cam");
    let rough = src.replace("surface=Crest, comp=control)", "surface=Crest, comp=control, stock_to_leave=0.5)");
    let part = parse::parse(&rough).expect("parse");
    let g = gcode::program(&part).expect("generate");
    // The (30,70,z=-3) corner: the side arrives at the corner's own z on the
    // skinned edge, then the fillet (radius 0.5, centred on the wire corner)
    // turns with no Z word.
    assert!(g.contains("G01 X29.500 Y70.000 Z-3.000"), "the side ends at the corner's own z on the skinned edge, got:\n{}", g);
    assert!(g.contains("G02 X30.000 Y70.500 I0.500 J0.000"), "the skin fillet is a true arc about the wire corner, got:\n{}", g);
    assert!(g.contains("G01 G41 D5 X29.500 Y30.000 F800.000"), "D stays the pure radius under a skinned edge");
}

#[test]
fn test_contour_wire_rough_offset_collapse_names_the_allowance() {
    // A wire too small for tool + skin must send the author to the right
    // knob: the error names the allowance, not just the tool.
    let src = fs::read_to_string("contour_pocket_rough.cam/main.part").expect("read fixture");
    let small = src
        .replace("    80 20 -2", "    26 20 -2")
        .replace("    80 80 0", "    26 26 0")
        .replace("    20 80 -2", "    20 26 -2");
    let part = parse::parse(&small).expect("a small wire still parses");
    let err = gcode::program(&part).expect_err("tool + skin cannot fit a 6mm rim");
    assert!(err.contains("stock_to_leave offset (0.500mm)"), "the error must name the allowance, got: {}", err);
}

#[test]
fn test_contour_wire_marks_the_stock_model_incomplete() {
    // A rim wire declares its wall, not the interior below it — the model
    // reports itself incomplete rather than guessing (DECISIONS.md "the
    // 3D-wire Contour…"), which silences the entry restriction and the
    // body-foul probe for the program by design.
    let src = fs::read_to_string("contour_pocket.cam/main.part").expect("read contour_pocket.cam");
    let part = parse::parse(&src).expect("parse");
    let remaining = nacs::stock::remaining_after(&part, part.operations.len() - 1);
    assert!(!remaining.complete, "a wire op has no closed-form removal to declare");
}

#[test]
fn test_retired_contour_feature_word_is_refused_with_the_migration_pointer() {
    // The word is reserved for a future 3D-wire feature; the error teaches
    // the split rather than failing as an unknown type.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let old = src.replace("ProfilePocket(extent=5.0", "Contour(extent=5.0");
    let err = parse::parse(&old).expect_err("the retired Contour feature must be rejected");
    assert!(err.contains("retired"), "error should say the word is retired, got: {}", err);
    assert!(err.contains("ProfilePocket") && err.contains("ProfileBoss"), "error should teach the pair, got: {}", err);
    assert!(err.contains("reserved"), "error should name the reservation, got: {}", err);
}

#[test]
fn test_depth_on_an_operation_is_refused_not_ignored() {
    // Retirement has to be loud. Silently dropping `depth` would emit a program
    // cutting to a different Z than the author wrote — the one outcome worse
    // than an error. Checked on a Drill too, where `depth` was already
    // vestigial (drill.rs reads the CylinderBore's, never the op's).
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let bad = src.replace("stepover=4.0)", "stepover=4.0, depth=5.0)");
    let err = parse::parse(&bad).expect_err("`depth` on an op must be rejected");
    assert!(err.contains("not an operation input"), "error should say what it is, got: {}", err);
    assert!(err.contains("extent"), "error should point at the replacement, got: {}", err);
}

#[test]
fn test_retired_extrusion_field_spellings_are_refused_with_the_migration_pointer() {
    // The rename's whole cost falls on `.part` files that already exist, so the
    // retired spelling has to be rejected BY NAME rather than surfacing as
    // "missing key 'extent'" — which names the new word without ever naming the
    // old one. All four extrusion features, both orientations.
    let cases = [
        ("cylinder_bore.cam/main.part", "extent=12.0", "depth=12.0", "CylinderBore"),
        ("cylinder_boss.cam/main.part", "extent=12.0", "height=12.0", "CylinderBoss"),
    ];
    for (fixture, new, old, variant) in cases {
        let src = fs::read_to_string(fixture).unwrap_or_else(|_| panic!("read {}", fixture));
        assert!(src.contains(new), "{} should author {}", fixture, new);
        let bad = src.replace(new, old);
        let err = match parse::parse(&bad) {
            Ok(_) => panic!("the retired spelling on {} must be rejected", variant),
            Err(e) => e,
        };
        assert!(err.contains(variant), "error should name the feature, got: {}", err);
        assert!(err.contains("is now `extent`"), "error should point at the replacement, got: {}", err);
        assert!(
            err.contains("Cone"),
            "error should scope the rule by naming the shape parameters it does NOT reach, got: {}",
            err
        );
    }
}

#[test]
fn test_shape_parameters_keep_height_and_depth() {
    // The other half of the rule, pinned so a later "consistency" sweep can't
    // quietly extend `extent` onto the shape parameters: those numbers ARE the
    // geometry's own Z span, not an extrusion added to it, so `extent` would
    // assert a cone spans zero. Authoring `extent` on one must fail.
    let cases = [
        ("cone.cam/main.part", "height=20", "Cone"),
    ];
    for (fixture, field, variant) in cases {
        let src = fs::read_to_string(fixture).unwrap_or_else(|_| panic!("read {}", fixture));
        assert!(src.contains(field), "{} should still author {}", fixture, field);
        parse::parse(&src).unwrap_or_else(|e| panic!("{} must still parse: {}", variant, e));

        let key = field.split('=').next().expect("field name");
        let swapped = src.replace(field, &field.replace(key, "extent"));
        assert!(
            parse::parse(&swapped).is_err(),
            "`extent` must NOT be accepted on {} — it is a shape parameter, not occupancy",
            variant
        );
    }
}

#[test]
fn test_stock_to_leave_zero_is_rejected() {
    // The degenerate-pointer move (rotation-0 precedent): 0 names the pass
    // the absent field already is.
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let bad = src.replace("stepover=3.0)", "stepover=3.0, stock_to_leave=0)");
    let err = parse::parse(&bad).expect_err("stock_to_leave=0 must be rejected");
    assert!(err.contains("omit the field"), "error should point at omitting the field, got: {}", err);
}

#[test]
fn test_stock_to_leave_negative_is_rejected() {
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let bad = src.replace("stepover=3.0)", "stepover=3.0, stock_to_leave=-0.5)");
    let err = parse::parse(&bad).expect_err("negative stock_to_leave must be rejected");
    assert!(err.contains("must be positive"), "error should say why, got: {}", err);
}

#[test]
fn test_stock_to_leave_scallop_at_or_over_allowance_is_rejected() {
    // Scallop mode's whole budget check is field arithmetic: cusp == scallop,
    // so scallop >= allowance spends the ceiling before any offset remains.
    let src = fs::read_to_string("sphere_scallop.cam/main.part").expect("read sphere_scallop.cam");
    for allowance in ["0.4", "0.5"] {
        let bad = src.replace("scallop=0.5)", &format!("scallop=0.5, stock_to_leave={})", allowance));
        let err = parse::parse(&bad).expect_err("scallop >= allowance must be rejected");
        assert!(err.contains("must exceed the scallop height"), "error should say why, got: {}", err);
    }
}

#[test]
fn test_stock_to_leave_stepover_ridge_is_rejected() {
    // sphere.cam's 3.0 stepdown against r_off = 23: adjacent balls near the
    // apex no longer overlap — an uncut RIDGE, which no allowance can
    // ceiling. Geometry-dependent, so it surfaces at generation, not parse.
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let bad = src.replace("stepover=3.0)", "stepover=3.0, stock_to_leave=0.5)");
    let part = parse::parse(&bad).expect("parses; the error is geometric");
    let err = gcode::program(&part).expect_err("non-overlapping roughing passes must be rejected");
    assert!(err.contains("uncut ridge"), "error should name the ridge, got: {}", err);
}

#[test]
fn test_stock_to_leave_stepover_cusp_budget() {
    // Fine 0.5 stepdown on the dome, priced at FULL inflation (the
    // corner-roll slice's audit fix — the original nominal price 1.072
    // under-ran the emitted stack's true crown cusp). The price now reads
    // the allowance: at 1.0 the crown pair on the fully-inflated sphere
    // prices 1.143mm — rejected with the number; at 1.2 it prices 1.157mm
    // and proceeds, auditing the budget.
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");

    let rejected = src.replace("stepover=3.0)", "stepover=0.5, stock_to_leave=1.0)");
    let part = parse::parse(&rejected).expect("parses; the error is geometric");
    let err = gcode::program(&part).expect_err("cusp >= allowance must be rejected");
    assert!(err.contains("worst-case cusp of 1.143mm"), "error should state the cusp, got: {}", err);
    assert!(err.contains("reduce stepover or increase stock_to_leave"), "error should be actionable, got: {}", err);

    let accepted = src.replace("stepover=3.0)", "stepover=0.5, stock_to_leave=1.2)");
    let part = parse::parse(&accepted).expect("parse");
    let g = gcode::program(&part).expect("a ceiling above the cusp must proceed");
    assert!(g.contains("[stock_to_leave 1.200mm, cusp budget 1.157mm]"), "title must audit the budget split");
    // Nominal skin 1.2 − 1.1571 = 0.0429: apex CL at 20 + 3 + 0.0429 = 23.043.
    assert!(g.contains("G01 Z23.043"), "apex CL must rise by exactly the tightened skin");
}

#[test]
fn test_stock_to_leave_on_mesh_is_rejected() {
    // Camp 3 accepts sampling error; a max-not-min GUARANTEE cannot ride on
    // an unbounded between-sample residual. Stated, not degraded.
    let src = fs::read_to_string("mesh.cam/main.part").expect("read mesh.cam");
    let bad = src.replace("stepover=2.0)", "stepover=2.0, stock_to_leave=0.5)");
    // base_dir so the fixture's STL resolves; the error under test is
    // validate's, which runs after the mesh loads.
    let base_dir = std::path::Path::new("mesh.cam");
    let err = parse::parse_project_with_base(&bad, None, Some(base_dir))
        .expect_err("stock_to_leave on a Mesh must be rejected");
    assert!(err.contains("between-sample residual has no analytic bound"), "error should say why, got: {}", err);
}

#[test]
fn test_stock_to_leave_on_spline_funnel_graduated() {
    // The last seat of the old unsupported-surface error test: the torus
    // and ellipsoid graduated out when the convex-mirrors slice landed, the
    // cone and ogive with corner roll, the vase and spline cap with mixed
    // meridians, and the spline funnel — the final axisymmetric error —
    // with the inside-funnels slice. Every non-Mesh Finish carrier now
    // accepts the field, so there is no fixture left that can reach the
    // catch-all error (it survives in validate as a fail-safe for future
    // carriers). The pin flips to the graduation: the funnel roughs, audits
    // the budget, and its verified exact-tip price stays under the ceiling.
    let src = fs::read_to_string("spline_funnel.cam/main.part").expect("read spline_funnel.cam");
    let rough_src = src.replace("stepover=3.0)", "stepover=3.0, stock_to_leave=0.5)");
    let base = Some(std::path::Path::new("spline_funnel.cam"));
    let plain = parse::parse_project_with_base(&src, None, base).expect("parse plain");
    let rough = parse::parse_project_with_base(&rough_src, None, base)
        .unwrap_or_else(|e| panic!("stock_to_leave on the spline funnel must parse now, got: {}", e));
    let g_plain = gcode::program(&plain).expect("plain pass generates");
    let g_rough = gcode::program(&rough).expect("roughing the spline funnel must generate");
    assert!(g_rough.contains("[stock_to_leave 0.500mm, cusp budget"), "title must audit the budget split");
    assert_ne!(g_plain, g_rough, "the roughing pass must actually move off the finishing locus");
}

#[test]
fn test_stock_to_leave_on_floor_points_at_the_plane() {
    // The allowance offsets surfaces the op defines by GEOMETRY. Floor's only
    // surface is a floor, and that floor IS the Plane's authored `z` — so there
    // is nothing for an allowance to move, and the error has to say where the
    // control actually lives rather than just "no".
    let src = fs::read_to_string("plate.cam/main.part").expect("read plate.cam");
    let geom = fs::read_to_string("plate.cam/geometry.part").expect("read plate geometry");
    let bad = src.replace("stepover=6.0)", "stepover=6.0, stock_to_leave=0.5)");
    let err = parse::parse_project(&bad, Some(&geom))
        .expect_err("stock_to_leave on an End must be rejected");
    assert!(err.contains("only surface is the floor"), "error should say why, got: {}", err);
    assert!(err.contains("author the plane"), "error should point at the Plane's z, got: {}", err);
}

#[test]
fn test_stock_to_leave_on_drill_is_rejected() {
    // A drilled hole's diameter is the tool's own; there is no offsettable
    // surface for an allowance to ride.
    let src = fs::read_to_string("cylinder_bore_form_points.cam/main.part").expect("read cylinder_bore_form_points.cam");
    let bad = src.replace("cycle=Simple)", "cycle=Simple, stock_to_leave=0.5)");
    let err = parse::parse(&bad).expect_err("stock_to_leave on a Drill must be rejected");
    assert!(err.contains("diameter is the tool's own"), "error should say why, got: {}", err);
}

#[test]
fn test_stock_to_leave_composes_with_compress_arcs() {
    // Orthogonal per-op fields compose: the roughing ring is still a true
    // circle, so arcs mode emits the same one-block full circle at the
    // shrunken radius.
    let src = fs::read_to_string("cylinder_bore_arcs.cam/main.part").expect("read cylinder_bore_arcs.cam");
    let roughed = src.replace("compress=arcs)", "compress=arcs, stock_to_leave=0.5)");
    let part = parse::parse(&roughed).expect("parse");
    let g = gcode::program(&part).expect("gen");
    assert!(g.contains("G03 X64.500 Y50.000 I-14.500 J0.000"), "engine arc ring must shrink by the skin, got none");
    assert!(g.contains("G03 X69.500 Y50.000 I-19.500 J0.000"), "control arc ring must ride the roughed wall");
}

#[test]
fn test_stock_to_leave_widens_gouge_guard() {
    // A physically-fitting tool whose ROUGHING offset collapses through the
    // bore axis: r_tool 19.5 fits the 20 bore, r_tool + 1.0 does not. The
    // widened guard is the only brake (validate's dia > tool.dia passes).
    let src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read cylinder_bore.cam");
    let bad = src
        .replace("dia=10.0", "dia=39.0")
        .replacen("stepover=5.0)", "stepover=5.0, stock_to_leave=1.0)", 1);
    let part = parse::parse(&bad).expect("parses; the error is geometric");
    let err = gcode::program(&part).expect_err("roughing offset past the bore radius must be rejected");
    assert!(err.contains("plus the stock_to_leave offset (1.000mm)"), "error should show the widened left side, got: {}", err);
    assert!(err.contains("reduce stock_to_leave"), "error should be actionable, got: {}", err);
}

#[test]
fn test_derive_cusp_height_is_the_exact_inverse() {
    // Round trip with derive_scallop_angle: the same triangle read both ways.
    let theta = parse::derive_scallop_angle(0.2, 3.0, 20.0);
    let h = parse::derive_cusp_height(theta, 3.0, 20.0);
    assert!((h - 0.2).abs() < 1e-9, "round trip must recover the scallop, got {}", h);
    // Zero separation leaves zero cusp, exactly.
    assert_eq!(parse::derive_cusp_height(0.0, 3.0, 20.0), 0.0);
    // Past ball overlap there is no cusp to price — an uncut ridge.
    assert!(parse::derive_cusp_height(1.0, 3.0, 20.0).is_infinite());
}

#[test]
fn test_derive_cusp_height_concave_is_the_exact_inverse() {
    // Round trip with derive_scallop_angle_concave: the mirrored triangle
    // read both ways. Not a sign-substitution of the convex inverse — the
    // law-of-cosines root flips (see the fn comment) — so the round trip is
    // the whole anchor.
    let theta = parse::derive_scallop_angle_concave(0.2, 3.0, 20.0);
    let h = parse::derive_cusp_height_concave(theta, 3.0, 20.0);
    assert!((h - 0.2).abs() < 1e-9, "round trip must recover the scallop, got {}", h);
    // Zero separation leaves zero cusp, exactly.
    assert_eq!(parse::derive_cusp_height_concave(0.0, 3.0, 20.0), 0.0);
    // Past ball overlap there is no cusp to price — an uncut ridge.
    assert!(parse::derive_cusp_height_concave(1.0, 3.0, 20.0).is_infinite());
}

#[test]
fn test_lint_warns_on_heavy_allowance() {
    // Allowance at or above the tool radius: legal, but a heavy roughing
    // skin, not a semi-finish — the advisory voice, not an error. Since the
    // engagement floor landed this authoring mistake draws TWO findings, one
    // per claim: the rougher's own pass is heavy, and the downstream pass
    // (run 2, same tool) inherits a skin at its radius.
    let src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read cylinder_bore.cam");
    let heavy = src.replacen("stepover=5.0)", "stepover=5.0, stock_to_leave=5.0)", 1);
    let part = parse::parse(&heavy).expect("parse");
    let warnings = parse::lint(&part);
    assert_eq!(warnings.len(), 2, "one heavy allowance, two claims: own pass and downstream pass");
    assert!(warnings[0].message.contains("heavy roughing skin"), "warning should say why, got: {}", warnings[0]);
    assert!(warnings[1].message.contains("hands its downstream pass"), "second warning is the cross-op claim, got: {}", warnings[1]);

    let light = src.replacen("stepover=5.0)", "stepover=5.0, stock_to_leave=0.5)", 1);
    let part = parse::parse(&light).expect("parse");
    assert!(parse::lint(&part).is_empty(), "a sub-radius allowance is quiet");
}

#[test]
fn test_engagement_lint_reads_the_downstream_tool() {
    // The floor's novelty over the heavy-skin forerunner: the skin is judged
    // against the tool that must CLEAR it, not the tool that left it. A 3mm
    // allowance is comfortable for the dia-10 rougher (sub-radius, the
    // forerunner is silent) but at or above the dia-4 finisher's radius —
    // only the cross-op check can see that.
    let src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read cylinder_bore.cam");
    let chained = src
        .replace("tool T5 Endmill(dia=10.0)", "tool T5 Endmill(dia=10.0)\ntool T6 Endmill(dia=4.0)")
        .replacen("stepover=5.0)", "stepover=5.0, stock_to_leave=3.0)", 1)
        .replace("run Side(tool=T5, surface=Bore, stepover=5.0, comp=control)",
                 "run Side(tool=T6, surface=Bore, stepover=5.0, comp=control)");
    let part = parse::parse(&chained).expect("parse");
    let warnings = parse::lint(&part);
    assert_eq!(warnings.len(), 1, "the rougher's own check is silent; the downstream tool's is not");
    assert!(warnings[0].message.contains("T6") && warnings[0].message.contains("hands its downstream pass"),
        "the finding names the downstream tool, got: {}", warnings[0]);
    // Advice, not just accusation: the message carries the single-pass target
    // (10% of the finisher's diameter).
    assert!(warnings[0].message.contains("0.400"), "the 10%-of-diameter target is stated, got: {}", warnings[0]);

    // 0.3mm on the same chain: 7.5% of the finisher's diameter, inside the
    // silent zone between the heuristic target and the radius ceiling — the
    // bias-to-silence pin (no "you are not at exactly 10%" nagging).
    let quiet = chained.replace("stock_to_leave=3.0", "stock_to_leave=0.3");
    let part = parse::parse(&quiet).expect("parse");
    assert!(parse::lint(&part).is_empty(), "a sub-radius skin for the downstream tool is quiet");
}

#[test]
fn test_engagement_lint_under_fires_only_when_allowances_grow() {
    // Under-engagement's unambiguous core: a later pass told to leave MORE
    // than it inherits has its target surface standing outside the material —
    // it removes nothing. Growth fires; equality is a spring pass at the
    // allowance (the two-comp-modes fixtures author exactly this) and must
    // stay silent — the rubbing threshold for thin-but-positive skins is
    // gated on a feeds engine and deliberately not guessed at.
    let src = fs::read_to_string("cylinder_bore_rough.cam/main.part").expect("read cylinder_bore_rough.cam");
    let growing = src.replacen("stock_to_leave=0.5)", "stock_to_leave=0.3)", 1);
    let part = parse::parse(&growing).expect("parse");
    let warnings = parse::lint(&part);
    assert_eq!(warnings.len(), 1, "a growing allowance chain is one finding");
    assert!(warnings[0].message.contains("inherits nothing to cut"), "the finding says what goes wrong, got: {}", warnings[0]);

    // The shipped fixture itself: equal allowances, both comp modes — quiet.
    let part = parse::parse(&src).expect("parse");
    assert!(parse::lint(&part).is_empty(), "an equal-allowance spring pass is quiet");
}

#[test]
fn test_engagement_gate_is_an_option() {
    // The committed brake (ROADMAP "Engagement-driven stock/feed coupling"):
    // `engagement gate` escalates the same findings to an error at validate.
    // The default stays advisory — the advisory-vs-gate default is a still-
    // open question, so the directive changes the voice, never the findings.
    let src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read cylinder_bore.cam");
    let heavy = src.replacen("stepover=5.0)", "stepover=5.0, stock_to_leave=5.0)", 1);

    let gated = heavy.replace("units mm", "units mm\nengagement gate");
    let err = parse::parse(&gated).expect_err("the gate must reject what the advisory only voices");
    assert!(err.contains("engagement gate:") && err.contains("heavy roughing skin"),
        "the error carries the finding, got: {}", err);
    assert!(err.contains("and 1 more finding"), "every finding is accounted for, got: {}", err);

    let advised = heavy.replace("units mm", "units mm\nengagement advise");
    parse::parse(&advised).expect("advise (the explicit default) must not reject");

    let unknown = heavy.replace("units mm", "units mm\nengagement loud");
    let err = parse::parse(&unknown).expect_err("an unknown mode is rejected at parse");
    assert!(err.contains("unknown engagement mode"), "got: {}", err);

    // A gated part with no findings comes out untouched.
    let clean = src.replace("units mm", "units mm\nengagement gate");
    parse::parse(&clean).expect("the gate is inert when nothing fires");
}

#[test]
fn test_engagement_lint_is_quiet_on_every_shipped_fixture() {
    // The floor's noise pin, the body-foul sweep's sibling: every shipped
    // fixture is well-formed, so the engagement findings must be empty on all
    // of them — including the equal-allowance two-comp-modes roughing
    // fixtures the spring-pass silence exists for.
    //
    // DELIBERATE exception, named fixture by fixture (fork 1A of "the roughing
    // allowance has two arms…", DECISIONS.md): the ≥50% cusp-spend advisory
    // names the corpus's two heavy spenders, and the design's own cost list
    // said it would — "that is the advisory working, and the fixtures may want
    // their stepovers revisited, but that is a fixture change with golden
    // churn and NOT part of this thread". An allowlist keeps the pin exact:
    // one finding each, the spend finding, nowhere else.
    //
    // TWO MORE, added 2026-08-17 with V1 slice 0 (DECISIONS.md → "V1, the
    // engagement measure…"): the corner arm's first catch, and it is a TRUE
    // POSITIVE in-house — the same shape K2's whitelist thread hit. Both
    // fixtures clear an L-shaped `Cavity` whose own header documents "the
    // reflex corner at (30,50)", and the measured worst engagement lands at
    // (30, 41): exactly one pass offset (r + stepover = 9mm) below that corner,
    // the tool pivoting around it. The advisory is saying something true that
    // no scalar check could have seen. Their stepovers may want revisiting —
    // that is a fixture change with golden churn, and deliberately not this
    // slice, exactly as the spend advisory's two were left.
    //
    // RE-MEASURED 2026-08-21 with Phase 9 lane A slice 1 (DECISIONS.md →
    // "Phase 9 lane A, slice 1…"), when the primitive learned the emitter's
    // INSIDE-OUT order. V1 walked the offsets boundary-first and read 93.8° at
    // the reflex corner; cut inside-out, the first loop never enters the
    // L-cavity's 20mm leg at all (the offset vanishes there), so the second
    // loop cuts the leg FROM SOLID — 180.0°, a slot on an "interior" pass.
    // And two fixtures that were silent under the mirror order speak now:
    // the first turns of an inside-out spiral around a tiny innermost loop
    // meet more than a stepover, because the swath is round where the loop
    // is square — 100.3° and 106.6° on pass 1, at a side midpoint, true
    // positives a machinist would recognise. Their stepovers may want
    // revisiting; that is a fixture change with golden churn, not this slice.
    // The band's slice-0 line on the thinned fixture is GONE: every pass now
    // rides its own word and takes exactly the rated chip.
    let expected: &[(&str, &str)] = &[
        ("./spline_funnel_rough.cam", "spends 80% of its budget"),
        ("./chamfer_profile.cam", "the tool actually reaches 180.0°"),
        ("./edge_round_profile.cam", "the tool actually reaches 180.0°"),
        ("./profile_pocket_clear.cam", "the tool actually reaches 100.3°"),
        ("./profile_pocket_clear_rough.cam", "the tool actually reaches 106.6°"),
        // Phase 26 slice 2: the band prices the family that goes AROUND the
        // post now. The first wall-parallel level meets the four corners the
        // deepest level opened and solid stock between them — 139.5° mid-edge.
        // (It read 95.0° until Phase 37 slice 1: the band's containment test
        // took the floor beside the post for the inside of all four corner
        // loops, and so for floor already cut.)
        ("./pocket_post.cam", "the tool actually reaches 139.5°"),
        // Three islands leave gaps the deepest level never entered, so a pass
        // of the next level cuts one from solid: a true slot, said so.
        ("./pocket_islands.cam", "the tool actually reaches 180.0°"),
        // Phase 27 slice 2: the step's family erodes to a LINE at its deepest
        // level. Until Phase 58 slice 1 that line was shorter than the level
        // outside it (the blank's open corners stood in the family as reflex
        // vertices) and the level's end turns cut from solid — a slot. With
        // the corners filled the levels are a stepover apart everywhere, and
        // what is left is the corner bite: the second level's sharp corner
        // against the segment's stadium end, the sentence
        // `profile_pocket_clear.cam` has always carried.
        ("./step.cam", "the tool actually reaches 113.6°"),
        // Phase 27 slice 4: a post in a pocket again — level 1 meets the gap
        // the deepest level never entered.
        ("./pocket_floor_offset.cam", "the tool actually reaches 180.0°"),
        ("./pocket_wall_lead.cam", "the tool actually reaches 180.0°"),
        // Phase 34 slice 1: twelve posts — the deepest level is a loop round
        // each, and the level outside them cuts the lanes between from solid.
        ("./post_field.cam", "the tool actually reaches 180.0°"),
        // Phase 41 slice 5: the pocket built against the tilted bore — its
        // third level's end turns under the constructed arc cut from solid,
        // the offset family's own slot on any pocket of this size.
        ("./tilted_bore.cam", "the tool actually reaches 180.0°"),
        // Phase 49 slice 1: a level that is the whole round, two rim bosses
        // bitten out of its edge — the deepest level is two lenses between
        // the bites' erosions, and the level outside them cuts from solid
        // where the lenses end: the offset family's own slot again.
        ("./rim_boss_offset.cam", "the tool actually reaches 180.0°"),
    ];
    for entry in fs::read_dir(".").expect("read repo root") {
        let path = entry.expect("dir entry").path();
        if path.extension().map(|e| e == "cam").unwrap_or(false) {
            let src = fs::read_to_string(path.join("main.part")).expect("read fixture");
            let geom = fs::read_to_string(path.join("geometry.part")).ok();
            let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path))
                .unwrap_or_else(|e| panic!("{}: parse failed: {}", path.display(), e));
            let findings = parse::engagement_findings(&part);
            // By file NAME: read_dir hands back `.\x.cam` on Windows and `./x.cam`
            // on Linux, and a string match on the latter made every expected
            // finding read as noise on the former (2026-10-03).
            match expected.iter().find(|(d, _)| std::path::Path::new(d).file_name() == path.file_name()) {
                Some((_, spend)) => {
                    assert_eq!(findings.len(), 1, "{}: exactly the spend finding, got: {:?}", path.display(), findings);
                    assert!(findings[0].message.contains(spend), "{}: expected '{}', got: {}", path.display(), spend, findings[0]);
                }
                None => assert!(findings.is_empty(), "{}: expected silence, got: {:?}", path.display(), findings),
            }
        }
    }
}

/// Phase 9 lane A — the band's slice-0 first catch, and slice 1's remedy
/// (DECISIONS.md → "Phase 9 lane A, slice 0…" and "…slice 1…"). Slice 0
/// caught the shipped thinned feed `F954.930` riding every loop of
/// `profile_pocket_thinned.cam`, the slot cut from solid included (1.667× the
/// rated chip there). Slice 1 gives every pass its own word — and, having
/// taught the primitive the emitter's inside-out order on the way, found the
/// interior passes were wrong too: the true worst early-pass engagement is
/// 53.1°, not the 36.9° nominal V1's mirror-order model read, so V1's x1.667
/// over-fed pass 1 by 1.33× at (30, 28). Now every pass takes EXACTLY the
/// rated chip, and the band is silent on the fixture.
#[test]
fn test_the_band_is_quiet_once_every_pass_rides_its_own_word() {
    let src = fs::read_to_string("profile_pocket_thinned.cam/main.part").expect("read");
    let part = parse::parse(&src).expect("parse");
    assert!(parse::engagement_findings(&part).is_empty(), "every pass at the rating: {:?}", parse::engagement_findings(&part));

    let (passes, f_z, table) = parse::priced_passes(&part, 0).expect("priced");
    assert_eq!(table, "tables/starter.feeds");
    assert!((f_z - 0.040).abs() < 1e-12);
    assert_eq!(passes.len(), 10, "ten levels, innermost first");
    for q in &passes {
        assert!((q.ratio - 1.0).abs() < 1e-9, "{}: {:.6}× is not the rating", q.label, q.ratio);
    }
    // The slot is pi by construction and rides the un-thinned identity.
    assert!((passes[0].theta - std::f64::consts::PI).abs() < 1e-9);
    assert!((passes[0].feed - 572.958).abs() < 5e-4, "{}", passes[0].feed);
    // Pass 1 is the heavy early turn: 53.1°, not the 36.9° nominal.
    assert!((passes[1].theta.to_degrees() - 53.1).abs() < 0.05, "{}", passes[1].theta.to_degrees());
    // Every later word is the identity times the cap-bounded factor of its
    // OWN worst engagement — read through the one home, so the F words and
    // the primitive cannot drift apart.
    let feeds = part.operations[0].pass_feeds.as_ref().expect("per-pass words");
    assert_eq!(feeds.len(), passes.len());
    for (k, q) in passes.iter().enumerate() {
        let want = feeds[0] * nacs::engagement::chip_thinning(q.theta).factor;
        assert!((feeds[k] - want).abs() < 1e-9, "pass {k}: {} vs {}", feeds[k], want);
    }
}

/// The DRAFT's second measurement: a typed feed was never read against the
/// rating sitting in the table. `feed 10000` typed, `material=6061` declared —
/// 13 × F10000.000, exit 0, silent. Now priced, and gate-eligible.
#[test]
fn test_a_typed_feed_is_priced_against_the_rating_it_never_read() {
    let src = fs::read_to_string("profile_pocket_thinned.cam/main.part").expect("read");
    let fast = src
        .replace("feed 800", "feed 10000")
        .replace("spindle=derive, feed=derive", "spindle=derive");
    assert_ne!(fast, src);
    let part = parse::parse(&fast).expect("a heavy feed is legal");
    let found = parse::engagement_findings(&part);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("F10000.000"), "{}", found[0].message);
    assert!(found[0].message.contains("17.453× the 0.040mm/tooth"), "{}", found[0].message);

    // The gate sees it: every input is declared or derived closed-form, so the
    // band may reject where body-foul never could. And it rejects on the
    // RESOLVED spindle — this program still says `spindle=derive`, so the
    // gate's first call (inside `validate`) has no rpm yet and the band must
    // decline there rather than price 4775rpm's chip at the global 3000
    // (27.8× — the first build of this test caught exactly that).
    let gated = fast.replace("units mm", "units mm\nengagement gate");
    let e = parse::parse(&gated).expect_err("the band rejects under the gate");
    assert!(e.starts_with("engagement gate: Clear on 'PocketProfile'"), "{e}");
    assert!(e.contains("at 4775rpm"), "the derived spindle, never the global one: {e}");
    assert!(e.contains("17.453×"), "{e}");

    // THE SILENCE ROSTER, two lines of it: no rating without a material, no
    // chip without a flute count. Each is an absence of data, never a guess.
    // (`spindle=derive` rejects by name without a material — K3's own rule —
    // so the control types the spindle; what is being measured is the band.)
    let no_material = fast.replace(", material=6061", "").replace(", spindle=derive", "");
    assert_ne!(no_material, fast);
    let part = parse::parse(&no_material).expect("parse");
    assert!(parse::engagement_findings(&part).is_empty(), "no material, no rating, no finding");
    let no_flutes = fast.replace("Endmill(dia=10.0, flute_count=3)", "Endmill(dia=10.0)");
    assert_ne!(no_flutes, fast);
    let part = parse::parse(&no_flutes).expect("parse");
    assert!(parse::engagement_findings(&part).is_empty(), "no flute count, no chip, no finding");
}

/// The floor (fork 3A): `1 / CHIP_THINNING_CAP`, one home, read from the
/// other side. V1 chose the cap so that below ~6.7% immersion the engine stops
/// raising the feed and says the honest answer is the rubbing threshold; this
/// is that threshold, and it is not a new number.
#[test]
fn test_the_rubbing_floor_reads_the_cap_from_the_other_side() {
    let src = fs::read_to_string("profile_pocket_clear_rough.cam/main.part").expect("read");
    let paired = src
        .replace("stock Box(w=100, l=100, h=20, x=0, y=0, z=0)", "stock Box(w=100, l=100, h=20, x=0, y=0, z=0, material=6061)")
        .replace("tool T5 Endmill(dia=10.0)", "tool T5 Endmill(dia=10.0, flute_count=3)");
    let thin = paired.replace("stock_to_leave=0.5)", "stock_to_leave=0.1)\nrun Side(tool=T5, surface=PocketProfile)");
    assert_ne!(thin, paired);
    let part = parse::parse(&thin).expect("parse");
    let finish: Vec<_> = parse::engagement_findings(&part)
        .into_iter()
        .filter(|f| matches!(&f.subject, parse::FindingSubject::Op { index: 1, .. }))
        .collect();
    assert_eq!(finish.len(), 1, "the finish's one floor finding: {finish:?}");
    let m = &finish[0].message;
    assert!(m.contains("0.100mm of inherited skin (11.5° of engagement)"), "{m}");
    assert!(m.contains("0.442× the 0.040mm/tooth"), "{m}");
    assert!(m.contains("below the 1/2.0 rubbing floor (CHIP_THINNING_CAP read from the other side)"), "{m}");

    // By hand: f = 800 / (3000 · 3); theta = acos(1 − 0.1/5); h = f · sin(theta).
    let f = 800.0 / 9000.0;
    let theta = (1.0_f64 - 0.1 / 5.0).acos();
    let (passes, f_z, _) = parse::priced_passes(&part, 1).expect("priced");
    assert!((passes[0].ratio - f * theta.sin() / f_z).abs() < 1e-12);
    assert!(passes[0].ratio < 1.0 / nacs::engagement::CHIP_THINNING_CAP, "below the floor, by the cap's own value");

    // THE CONTROL: the shipped 0.500mm skin engages 25.8° and takes 0.969× —
    // under the rating and above the floor, so the finish is silent.
    let fair = paired.replace("stock_to_leave=0.5)", "stock_to_leave=0.5)\nrun Side(tool=T5, surface=PocketProfile)");
    let part = parse::parse(&fair).expect("parse");
    let (passes, _, _) = parse::priced_passes(&part, 1).expect("priced");
    assert!(passes[0].ratio > 0.5 && passes[0].ratio < 1.0, "{}", passes[0].ratio);
    assert!(
        parse::engagement_findings(&part).iter().all(|f| !matches!(&f.subject, parse::FindingSubject::Op { index: 1, .. })),
        "a fair skin is silent"
    );
}

/// The gate had to be asked twice. An in-text `engagement gate` is checked in
/// `validate`, which runs BEFORE `resolve_derived_words`; a `feed=derive` op
/// has no feed word there, and pricing the global `feed 800` in its place
/// would be a wrong number wearing a finding's voice. The band declines at
/// the first call, and `parse_project_with_base` asks the gate once more
/// after the words resolve — where it rejects on the number the machine will
/// actually be given.
///
/// With every pass riding its own word a derived feed can miss the rating
/// only where the cap binds: below ~6.7% immersion `1/sin` wants more than
/// `CHIP_THINNING_CAP` and V1 rejects to give it — "the honest answer there is
/// the rubbing threshold", its own doc says — so the band's floor is where
/// that sentence finally has a voice. `stepover=0.1` on a 10mm cutter is 1%
/// immersion, 11.5°: the capped word takes 0.398× the rated chip.
#[test]
fn test_the_gate_reads_the_band_after_the_derived_words_resolve() {
    let src = fs::read_to_string("profile_pocket_thinned.cam/main.part").expect("read");
    let wisp = src.replace("stepover=1.0", "stepover=0.1");
    assert_ne!(wisp, src);
    let part = parse::parse(&wisp).expect("a whisper-thin stepover is legal");
    let found = parse::engagement_findings(&part);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("(11.5° of engagement) the chip is only 0.016mm — 0.398× the 0.040mm/tooth"), "{}", found[0].message);
    assert!(found[0].message.contains("feeds F1145.916"), "the capped pass's OWN word, not the op's: {}", found[0].message);
    assert!(found[0].message.contains("rubbing floor"), "{}", found[0].message);

    let gated = wisp.replace("units mm", "units mm\nengagement gate");
    assert_ne!(gated, wisp);
    let e = parse::parse(&gated).expect_err("the capped wisp is rejected under the gate");
    assert!(e.starts_with("engagement gate: Clear on 'PocketProfile' with T5 feeds F1145.916 at 4775rpm"), "the RESOLVED words, never the global ones: {e}");
    assert!(!e.contains("F800") && !e.contains("3000rpm"), "{e}");
}

#[test]
fn test_the_chip_thinned_feed_golden() {
    // V1 slice 1's acceptance (DECISIONS.md → "V1 slice 1…"): one op's derived
    // feed changes BECAUSE OF a measured engagement. The chain, every link
    // checkable by hand:
    //
    //   stepover 1.0 on r 5.0  ->  a_e/D = 10%, the author's own loop target
    //   theta_e = acos(1 - a_e/r) = 36.87 degrees        (the primitive)
    //   factor  = 1/sin(theta_e) = 1.666667             (the identity)
    //   F       = 572.958 x 1.666667 = 954.930          (the feed)
    //
    // 572.958 is the un-thinned identity the same declaration yields with no
    // engagement to read — floor_derive_feed.cam emits exactly that.
    //
    // PER PASS since Phase 9 lane A slice 1 (DECISIONS.md → "…slice 1…"): the
    // slot (innermost loop, emitted first) rides the identity itself; pass 1
    // — the heavy first turn of an inside-out spiral, 53.1° — rides x1.250;
    // pass 2 x1.573; every later pass has relaxed to the 36.9° nominal and
    // rides the x1.667 above. The op's one-number word is the worst non-slot
    // pass's, x1.250.
    let main_src = fs::read_to_string("profile_pocket_thinned.cam/main.part").expect("read main.part");
    let part = parse::parse(&main_src).expect("parse profile_pocket_thinned.cam");
    let g = gcode::program(&part).expect("generate");

    assert!(g.contains("M03 S4774.648"), "the derived S word must survive unchanged");
    assert!(g.contains("F954.930"), "the THINNED feed must be emitted on the relaxed passes, got: {g}");
    // (Since Phase 44 slice 1 the slot — 8mm round on a 10mm tool, under a
    // diameter — is entered by a plunge at the plunge feed, said by name, so
    // the identity rides its first flat move rather than a ramp line.)
    assert!(g.contains("(plunge: this loop is 8.000mm round, under one diameter — no room to ramp)"), "{g}");
    assert!(g.contains("G01 X31.000 Y29.000 F572.958"), "the slot rides the un-thinned identity on its first flat move: {g}");
    assert!(g.contains("F716.197"), "pass 1 at x1.250: {g}");
    assert!(g.contains("F901.033"), "pass 2 at x1.573: {g}");
    // The correction is auditable in the program text, not just in the number:
    // a feed raised by a computation the text cannot show is the silence this
    // project rejects — and with one word per pass, the title states the range.
    assert!(g.contains("[chip thinning x1.250 at 53.1° engagement; per pass: x1.000 on the slot, then x1.250..x1.667 over 9 passes]"),
        "the audit trail must state factor, engagement and the per-pass range, got: {g}");

    let golden = fs::read_to_string("tests/profile_pocket_thinned.golden.gcode").expect("read golden");
    assert_eq!(g.trim(), golden.trim());
}

#[test]
fn test_the_audit_trail_and_the_feed_cannot_disagree() {
    // Two emitted artifacts, cross-checked against each other: the F word the
    // controller obeys, and the title comment the operator reads. If the audit
    // says x1.667 at 36.9 degrees, then F must be the un-thinned identity times
    // exactly that factor, and that factor must be the reciprocal sine of that
    // angle. A comment that flatters a number it did not produce is worse than
    // no comment.
    //
    // (Fork 3A's own property — that the WORST engagement is what gets priced,
    // so a reflex corner lowers the boost rather than raising it — is pinned at
    // the unit level in `engagement.rs`, where the two shapes can be stated
    // directly instead of edited into a fixture.)
    let main_src = fs::read_to_string("profile_pocket_thinned.cam/main.part").expect("read");
    let part = parse::parse(&main_src).expect("parse");
    let g = gcode::program(&part).expect("generate");

    let ct = part.operations[0].chip_thinning.expect("the op carries its correction");
    let title = g.lines().find(|l| l.contains("chip thinning")).expect("audit line");

    // The audit's own two numbers agree with each other.
    assert!(title.contains(&format!("x{:.3}", ct.factor)), "factor mismatch: {title}");
    assert!(title.contains(&format!("{:.1}°", ct.engagement.to_degrees())), "angle mismatch: {title}");
    assert!((ct.factor - 1.0 / ct.engagement.sin()).abs() < 1e-12,
        "the audited factor is not the reciprocal sine of the audited angle");

    // And the feed the controller obeys is the identity times that same factor.
    const UNTHINNED: f64 = 572.9577951308232; // F = RPM x z x f_z, K3's identity
    let feed = part.operations[0].feed.expect("derived feed");
    assert!((feed - UNTHINNED * ct.factor).abs() < 1e-9,
        "emitted feed {feed} is not the identity times the audited factor");
    assert!(g.contains(&format!("F{:.3}", feed)), "the audited feed must be the emitted one");
}

#[test]
fn test_a_declared_keep_out_is_seen_and_moves_no_metal() {
    // V2 slice 0's acceptance (DECISIONS.md → "V2, the fixture…"): one
    // declared clamp turns a currently-green program into a named advisory.
    //
    // The accident is deliberately NOT a cutting mistake. Every move in this
    // program is legal against the material — it faces the top and nothing
    // else — so the material-facing checks stay silent and only the keep-out
    // speaks. What is wrong is the clearance plane, which was chosen for a
    // stock and says nothing about a clamp standing proud of it.
    //
    // Phase 27 slice 3 changed what the SHIPPED program does — the face goes
    // around the clamp now (pinned in the next test) — so the accident is
    // rebuilt here as a device: the raster's moves, from the program with no
    // fixture line at the stepover it shipped with, judged against the part
    // that declares the clamp. The check is what is under test, and it must
    // not lean on the emitter being careless.
    let path = std::path::Path::new("./floor_clamped_offset.cam");
    let src = fs::read_to_string(path.join("main.part")).expect("read fixture");
    let part = parse::parse(&src).expect("parse");
    let without: String = src
        .replace("stepover=5.0", "stepover=6.0")
        .replace("pattern=offset", "pattern=raster")
        .lines()
        .filter(|l| !l.trim_start().starts_with("fixture "))
        .collect::<Vec<_>>()
        .join("\n");
    let bare = parse::parse(&without).expect("parse without the clamp");
    let (_, raster_moves) = gcode::program_with_moves(&bare).expect("generate");

    let findings = nacs::fixture::fixture_findings(&part, &raster_moves);
    assert_eq!(findings.len(), 1, "expected exactly the clamp, got: {findings:?}");
    let m = &findings[0].message;
    // The claim is the geometry, so the assertions are the numbers. The tip
    // rides 0.5mm below the datum and the clamp's foot sits on it, so the body
    // meets steel 0.5mm above the tip; the pass runs along the clamp's near
    // edge, so the overlap is the full tool radius, at its near corner.
    assert!(m.contains("body 0.500mm above the tip"), "wrong height: {m}");
    assert!(m.contains("reaches fixture 'ToeClamp' by 5.000mm"), "wrong depth: {m}");
    assert!(m.contains("near (48.000, 18.000)"), "wrong witness: {m}");
    assert!(nacs::fixture::fixture_findings(&bare, &raster_moves).is_empty(),
        "no fixture declared, nothing to say");
}

/// The corpus's keep-out finding, since Phase 27 slice 3: `floor_traverse_offset.cam`
/// is gone around by every cutting pass and crossed by one LIFT — the clearance
/// plane is 25mm and the riser block stands 40 — which is the check's real
/// subject (V2: "the first check whose subject is the rapid"), and nothing an
/// emitter can fix. Golden-pinned; one finding; the gate refuses it.
#[test]
fn test_a_traverse_across_a_riser_is_the_keep_out_checks_real_subject() {
    let src = fs::read_to_string("floor_traverse_offset.cam/main.part").expect("read fixture");
    let part = parse::parse(&src).expect("parse");
    let (g, emitted_moves) = gcode::program_with_moves(&part).expect("generate");
    assert_eq!(g.trim(), fs::read_to_string("tests/floor_traverse_offset.golden.gcode").unwrap().trim());
    assert!(g.contains("(keep-outs: Riser)"));
    let findings = nacs::fixture::fixture_findings(&part, &emitted_moves);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].message.contains("reaches fixture 'Riser'"), "{}", findings[0].message);
    // The move it names is a traverse at the clearance plane, not a cut.
    assert!(findings[0].message.contains(", 25.000) as the move ends"), "{}", findings[0].message);
    let gated = parse::parse(&format!("fixture gate\n{src}")).unwrap();
    assert!(gcode::program(&gated).unwrap_err().starts_with("fixture gate:"));
}

/// Phase 27 slice 3 (`DECISIONS.md` → "Phase 27 slice 3: the keep-out…"): the
/// EMITTER reads the fixture now. V2's law was that the ADVISORY moves no
/// metal — a lint must never be a silent rewrite — and it still does not: what
/// changed the path is the declaration, read by the op, and the program says
/// so in its own text. The shipped clamp is gone around, the check has nothing
/// left to say about it, and the lifts between loops go to the clearance
/// plane.
#[test]
fn test_a_face_goes_around_a_declared_clamp_and_says_so() {
    let src = fs::read_to_string("floor_clamped_offset.cam/main.part").expect("read fixture");
    let part = parse::parse(&src).expect("parse");
    let (g, emitted_moves) = gcode::program_with_moves(&part).expect("generate");
    let golden = fs::read_to_string("tests/floor_clamped_offset.golden.gcode").expect("read golden");
    assert_eq!(g.trim(), golden.trim());
    assert!(g.contains("(--- floor: Top with T1 ---)\n(keep-outs: ToeClamp)\n"), "{}", &g[..400]);
    assert!(nacs::fixture::fixture_findings(&part, &emitted_moves).is_empty(), "nothing left to say");

    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut cutting = 0;
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
        if z < 0.0 {
            // Distance from the tool's centre to the clamp's footprint grown
            // by the 0.5mm margin (x 47.5..62.5, y 17.5..42.5): never under the
            // tool's radius, less a chord's sagitta on the rounded corners.
            let (dx, dy) = ((47.5 - x).max(x - 62.5).max(0.0), (17.5 - y).max(y - 42.5).max(0.0));
            let clear = dx.hypot(dy);
            assert!(clear >= 5.0 - nacs::CHORD_TOL_MM - 1e-3, "`{line}` is {clear:.3}mm from the clamp's keep-out");
            cutting += 1;
        }
        // Every lift is to the clearance plane: `rapid_z` (5) is under the clamp.
        assert!(!line.starts_with("G00 Z5.000"), "a lift to rapid_z would traverse under the clamp's top");
    }
    assert!(cutting > 50);

    // The declaration is what moved the path — delete it and the raster is back.
    let without: String = src.lines().filter(|l| !l.trim_start().starts_with("fixture ")).collect::<Vec<_>>().join("\n");
    let bare = gcode::program(&parse::parse(&without).unwrap()).unwrap();
    assert!(!bare.contains("keep-outs") && bare != g);

    // A clamp hanging over the stock's edge is not a web: its loop merges into
    // the boundary across nothing but air, and the narrowest passage — which
    // is what the web is read from — has no wall there to measure to.
    let region = nacs::region::of(&part, &part.operations[0]).unwrap();
    assert_eq!(region.keep_outs(), ["ToeClamp"]);
    assert_eq!(region.narrowest, None);
}

#[test]
fn test_the_keep_out_gate_is_an_option_and_the_default_does_not_move() {
    // V2 slice 1 (DECISIONS.md → "V2, the fixture…", fork 6A): the fourth
    // gate, and the first whose probe OVER-claims — which is exactly why it
    // may reject where body-foul never will. One computation, two voices: it
    // rejects on the same finding the advisory prints.
    //
    // Since Phase 27 slice 3 the shipped program goes AROUND its clamp, so the
    // gate has nothing to reject there. Its subject was always the traverse
    // (V2: "the first check whose subject is the rapid"), and that is what
    // this program restores: a clamp in the middle of the face, taller than
    // the clearance plane the lifts between loops go to.
    let src = fs::read_to_string("floor_clamped_offset.cam/main.part").expect("read")
        .replace("h=25, x=48, y=18", "h=40, x=20, y=18");

    // Undeclared: the program still comes out. The default did not move.
    let part = parse::parse(&src).expect("parse");
    assert!(gcode::program(&part).is_ok(), "advisory by default");

    // Declared: it rejects, carrying the measurement into the error.
    let gated = parse::parse(&format!("fixture gate\n{src}")).expect("parse gated");
    let e = gcode::program(&gated).expect_err("fixture gate must reject");
    assert!(e.starts_with("fixture gate:"), "not the gate's voice: {e}");
    assert!(e.contains("reaches fixture 'ToeClamp' by 5.000mm"),
        "the error must carry the measure: {e}");

    // And the CLI floor escalates it, unlike body-foul which is excluded from
    // that floor forever because a sampled probe biased to silence may miss.
    let mut floored = parse::parse(&src).expect("parse");
    parse::apply_gate_floor(&mut floored);
    assert!(gcode::program(&floored).is_err(), "--gates gate must escalate the keep-out");

    // An explicit `fixture advise` is inert, and a clean program is inert
    // under the gate — the brake rejects findings, not fixtures.
    let advised = parse::parse(&format!("fixture advise\n{src}")).expect("parse");
    assert!(gcode::program(&advised).is_ok());
    let clean: String = src.lines().filter(|l| !l.trim_start().starts_with("fixture ")).collect::<Vec<_>>().join("\n");
    let mut clean = parse::parse(&format!("fixture gate\n{clean}")).expect("parse");
    parse::apply_gate_floor(&mut clean);
    assert!(gcode::program(&clean).is_ok(), "a gated program with no keep-out hit must pass");
}

#[test]
fn test_the_optimizer_and_the_reach_certificate_reach_the_same_number() {
    // THE ACCEPTANCE for the revolved cavities (DECISIONS.md → "the revolved
    // cavities…", fork 6A). Two checks, two code paths, one number.
    //
    // The reach certificate reads the EMITTED moves of a program and reports
    // how deep the tool's tip goes. The optimizer reads the DECLARATION alone
    // — no ops, no emission — and derives the same depth from the cavity's
    // meridian and the ball's own radius. If those two ever disagree, one of
    // them is describing a tool that is not there.
    let src = fs::read_to_string("spline_funnel_holder.cam/main.part").expect("read");
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");

    // The certificate's number, off the emission emitted moves.
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("the fixture emits");
    let findings = nacs::stock::reach_findings(&part, &emitted_moves);
    assert_eq!(findings.len(), 1, "one program, one shortfall");
    let voiced = findings[0].to_string();
    assert!(voiced.contains("20.051mm"), "the certificate's own number: {voiced}");

    // The optimizer's, from the declaration and the fixture's own 6mm ball.
    // The catalog is inline because the point is the BALL's radius, not the
    // shop wall: this is the same cutter the program declares.
    let cat = nacs::assembly::parse_catalog(
        "cutter six-ball\ndia 6.0\ntype Ballmill\nflute_len 3.0\nshank_dia 10.0\noverall_len 60.0\nmodulus 600\n\
         holder plain\nnose_dia 30.0\nnose_len 20.0\nbody_dia 50.0\ngauge_len 60.0\ncompliance 2.0\n",
    )
    .expect("catalog parses");
    let cavity = nacs::assembly::corridor_of(&part, &part.features["Vortex"], nacs::assembly::Opened::Declared)
        .expect("a funnel is a cavity");
    let shape = nacs::assembly::CutterShape::of_record(&cat.cutters[0]);
    let placed = cavity.corridor.place(&shape, cavity.reach).expect("a ball works it");
    assert_eq!(format!("{:.3}mm", placed.reach), "20.051mm",
        "the optimizer must derive the certificate's own reach");

    // And the two numbers are NOT the same measurement made twice: the
    // cavity's own deepest surface point is 18.000mm (the meridian's last
    // pole). The extra 2.051mm is the ball hanging below the point it
    // touches, which is exactly why the certificate measures the TOOL and
    // not the feature.
    assert_eq!(format!("{:.3}", cavity.depth), "18.000");
    assert!(placed.reach > cavity.depth, "the tip hangs below the surface it touches");
}

#[test]
fn test_the_reach_gate_is_an_option_and_the_default_does_not_move() {
    // THE ACCEPTANCE (DECISIONS.md → "`reach gate`, the fifth directive…",
    // fork 5A): the fifth gate, on the one program in the corpus that fires
    // this certificate — a fixture whose header wrote the finding in prose
    // months before the check existed.
    //
    // The directive is added HERE and never to the fixture, and that is the
    // trap the DRAFT named: a gated program emits nothing, so declaring
    // `reach gate` in `spline_funnel_holder.cam` would delete its own
    // golden. The corpus keeps the advisory, the test owns the gate.
    let src = fs::read_to_string("spline_funnel_holder.cam/main.part").expect("read");

    // Undeclared: the program still comes out, byte for byte — the default
    // did not move, and the golden proves it in its own test.
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    assert!(gcode::program(&part).is_ok(), "advisory by default");

    // Declared: it rejects, carrying the same two numbers the advisory prints.
    // One computation, two voices — the family's own law, and the reason the
    // gate reads `stock::reach_findings` rather than restating its arithmetic.
    let gated = parse::parse_project_with_base(&format!("reach gate\n{src}"), None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse gated");
    let e = gcode::program(&gated).expect_err("reach gate must reject");
    assert!(e.starts_with("reach gate:"), "not the gate's voice: {e}");
    assert!(
        e.contains("8.000mm stickout is short of the 20.051mm"),
        "the error must carry the measurement: {e}"
    );

    // The floor escalates it (fork 2A) — this is the check an unattended
    // reader most needs escalated, because a shortfall is invisible in the
    // G-code: every coordinate is legal and the machine will run it. Asserted
    // at the ONE home rather than through this program's output, because the
    // gates are ORDERED and the entry gate speaks first here: this fixture
    // plunges as well as coming up short, so under the floor the reader is
    // told about the entry it meets before the assembly that cannot be there.
    // Both are true; the first error wins, which is the family's shape and
    // not this check's business.
    let mut floored = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    parse::apply_gate_floor(&mut floored);
    assert_eq!(floored.reach_mode, parse::ReachMode::Gate,
        "--gates gate must escalate the reach certificate");
    assert!(gcode::program(&floored).is_err(), "and the floored program rejects");

    // An explicit `reach advise` is inert, and the gate rejects FINDINGS
    // rather than stickouts: the same program with a stickout that reaches
    // passes under the gate and under the floor.
    let advised = parse::parse_project_with_base(&format!("reach advise\n{src}"), None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    assert!(gcode::program(&advised).is_ok());
    let long = src.replace("stickout=8.0", "stickout=40.0");
    assert_ne!(long, src, "the fixture must still declare the stickout this test lengthens");
    let long = parse::parse_project_with_base(&format!("reach gate\n{long}"), None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    assert!(
        gcode::program(&long).is_ok(),
        "an assembly that reaches must pass the gate it just failed"
    );

    // An unknown mode word is a directive missing its shape, and says so with
    // both spellings — the four siblings' error, one more time.
    let e = parse::parse(&format!("reach loud\n{src}")).expect_err("an unknown mode must reject");
    assert!(e.contains("unknown reach mode 'loud'") && e.contains("'advise' or 'gate'"), "{e}");

    // And an undeclared stickout stays silent under the gate — the universal
    // law, and the distinction the DRAFT rested the eligibility argument on:
    // saying nothing where the declaration is absent is not the same as
    // MISSING something it claimed to look at.
    // Truncating the tool line at `stickout=` drops the holder with it, and
    // that is the parser's own placement law rather than a convenience here: a
    // holder the ladder cannot place is a declaration that quietly does not
    // exist, so it rejects. Which means the silent case is the honest one —
    // nothing is declared about where this tool ends.
    let mute: String = src
        .lines()
        .map(|l| match l.find(", stickout=") {
            Some(i) if l.starts_with("tool ") => format!("{})", &l[..i]),
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!mute.contains("stickout="), "the stickout must actually be gone");
    let mute = parse::parse_project_with_base(&format!("reach gate\n{mute}"), None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    assert!(gcode::program(&mute).is_ok(), "undeclared means today's behaviour, even under the gate");
}

#[test]
fn test_the_mode_words_are_reserved_fixture_names() {
    // One word doing two jobs, disambiguated by a reserved name rather than by
    // token count — which would have been the silent-misparse hazard the
    // `feeds_table` naming note warns about. So the collision rejects, loudly.
    let src = fs::read_to_string("floor_clamped_offset.cam/main.part").expect("read");
    let e = parse::parse(&src.replace("fixture ToeClamp Box", "fixture gate Box"))
        .expect_err("a fixture named 'gate' must reject");
    assert!(e.contains("may not be named 'gate'") && e.contains("fixture advise|gate"),
        "the error must explain the collision: {e}");
    // An unknown mode word is a declaration missing its shape, and says so.
    assert!(parse::parse(&format!("fixture loud\n{src}")).is_err());
}

#[test]
fn test_the_keep_out_grammar_refuses_what_it_cannot_mean() {
    let base = fs::read_to_string("floor_clamped_offset.cam/main.part").expect("read");
    let swap = |from: &str, to: &str| base.replace(from, to);

    // A keep-out with no extent keeps nothing out, and a reader would take
    // the silence for clearance.
    let e = parse::parse(&swap("h=25, x=48", "h=0, x=48")).expect_err("zero height must reject");
    assert!(e.contains("keeps nothing out"), "{e}");

    // Unknown keys reject, per the whitelist law.
    let e = parse::parse(&swap("w=14,", "w=14, dia=3,")).expect_err("unknown key must reject");
    assert!(e.contains("dia"), "{e}");

    // Two keep-outs cannot share a name — a finding has to be able to say
    // which one it hit.
    let dup = base.replace(
        "fixture ToeClamp Box(w=14, l=24, h=25, x=48, y=18, z=0)",
        "fixture ToeClamp Box(w=14, l=24, h=25, x=48, y=18, z=0)\nfixture ToeClamp Box(w=4, l=4, h=4, x=0, y=0, z=0)",
    );
    let e = parse::parse(&dup).expect_err("duplicate name must reject");
    assert!(e.contains("declared twice"), "{e}");

    // A shape v1 does not model rejects by name and points at what it does.
    let e = parse::parse(&swap("Box(w=14, l=24, h=25", "Wedge(w=14, l=24, h=25"))
        .expect_err("unknown shape must reject");
    assert!(e.contains("'Box' or 'Cylinder'"), "{e}");
}

#[test]
fn test_the_corner_arm_cites_a_measured_angle_not_a_ratio() {
    // V1 slice 0's acceptance (DECISIONS.md → "V1, the engagement measure…"):
    // one finding cites a MEASURED angle, where every finding before it cited a
    // millimetre allowance against a tool radius. The claim under test is the
    // number, not the prose — so the assertions are on the angle, the corner
    // coordinate, and the pass index, each of which only a boundary-level
    // computation can produce.
    let path = std::path::Path::new("./chamfer_profile.cam");
    let src = fs::read_to_string(path.join("main.part")).expect("read fixture");
    let geom = fs::read_to_string(path.join("geometry.part")).ok();
    let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(path)).expect("parse");
    let findings = parse::engagement_findings(&part);
    let corner = findings
        .iter()
        .find(|f| f.message.contains("half-immersion"))
        .unwrap_or_else(|| panic!("no corner finding, got: {findings:?}"));

    // The straight-run nominal is the stepover's own promise: acos(1 - 4/5).
    assert!(corner.message.contains("78.5° of cutter engagement on a straight run"),
        "nominal angle missing: {corner}");
    // The measured worst, past half immersion — the fact the stepover cannot
    // state. RE-MEASURED with Phase 9 lane A slice 1, when the primitive
    // learned the emitter's inside-out order: V1's mirror-order walk read 93.8°
    // at the reflex corner; cut innermost-first, the first loop never enters
    // the 20mm leg (the offset vanishes there), so the second loop cuts the
    // leg FROM SOLID — a full slot, and the advisory says so in those words.
    assert!(corner.message.contains("the tool actually reaches 180.0°"),
        "measured angle missing: {corner}");
    assert!(corner.message.contains("a full slot — the inner loop never reached here"),
        "the slot's own reason: {corner}");
    // Located, not merely asserted: at the mouth of the leg, on the second
    // emitted loop — the first point of that loop the deeper one's swath does
    // not reach. (Until Phase 37 slice 1 this read (21.000, 60.500), halfway
    // up the leg: the band's containment test took the mouth for the inside
    // of the deeper loop, whose arc curves round the corner at (30, 50).)
    assert!(corner.message.contains("at (21.000, 50.000) on pass 1"),
        "position missing: {corner}");

    // Silence where the geometry is kind: a pocket whose early turns stay
    // under half immersion says nothing. (The plain 60x60 fixtures are NOT
    // that control any more — at stepover 4 on r 5 their first turn meets
    // 100° around a 2mm innermost loop, a true positive; the thinned fixture
    // at stepover 1 peaks at 53.1°.)
    let quiet = std::path::Path::new("./profile_pocket_thinned.cam");
    let qsrc = fs::read_to_string(quiet.join("main.part")).expect("read fixture");
    let qgeom = fs::read_to_string(quiet.join("geometry.part")).ok();
    let qpart =
        parse::parse_project_with_base(&qsrc, qgeom.as_deref(), Some(quiet)).expect("parse");
    assert!(parse::engagement_findings(&qpart).is_empty(),
        "a convex pocket must stay silent");
}

#[test]
fn test_the_corner_arm_escalates_through_the_existing_gate() {
    // Fork 5A: the measure is closed-form over an exact inventory, not a
    // sampled probe, so it MAY gate — and it does so through the directive
    // that already exists rather than growing a fourth one. The advisory
    // default is untouched: the same fixture without the directive only warns.
    let path = std::path::Path::new("./chamfer_profile.cam");
    let src = fs::read_to_string(path.join("main.part")).expect("read fixture");
    let geom = fs::read_to_string(path.join("geometry.part")).ok();
    let gated = format!("engagement gate\n{src}");
    let err = parse::parse_project_with_base(&gated, geom.as_deref(), Some(path))
        .expect_err("engagement gate must reject the corner finding");
    assert!(err.contains("engagement gate:"), "not the gate's voice: {err}");
    assert!(err.contains("180.0°"), "the error must carry the measure: {err}");

    // Undeclared: parses, and the finding is only a finding.
    let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(path))
        .expect("advisory by default");
    assert!(!parse::engagement_findings(&part).is_empty());
}

#[test]
fn test_the_spend_advisory_escalates_through_the_gate() {
    // The promotion path the design chose over a cold refusal: the directive
    // that already exists turns the advisory into a validate-time error,
    // same findings, louder voice.
    let src = fs::read_to_string("spline_funnel_rough.cam/main.part").expect("read");
    let gated = src.replace("units mm", "units mm\nengagement gate");
    let err = parse::parse_project_with_base(&gated, None, Some(std::path::Path::new("spline_funnel_rough.cam")))
        .expect_err("the gate must reject an 80% spend");
    assert!(
        err.contains("engagement gate:") && err.contains("spends 80% of its budget"),
        "the error speaks the finding: {}",
        err
    );
}

#[test]
fn test_boss_wall_takes_endmill_not_ballmill() {
    // Like the bore, the boss wall is a straight flank cut — an Endmill parses.
    let main_src = fs::read_to_string("cylinder_boss.cam/main.part").expect("failed to read cylinder_boss.cam");
    parse::parse(&main_src).expect("Finish on a CylinderBoss with an Endmill must be accepted");
}

#[test]
fn test_boss_wall_rejects_scallop() {
    // Same rule as the bore flank: no scallop on a straight wall.
    let main_src = fs::read_to_string("cylinder_boss.cam/main.part").expect("failed to read cylinder_boss.cam");
    let scalloped = main_src.replace("stepover=5.0", "scallop=0.5");
    let result = parse::parse(&scalloped);
    assert!(result.is_err(), "scallop on a CylinderBoss flank wall must be a hard error");
    assert!(result.unwrap_err().contains("scallop"), "error should say why");
}

#[test]
fn test_boss_has_no_tool_fit_constraint() {
    // The tool rides *outside* the boss, so unlike the bore there is no
    // dia > tool.dia requirement — a tool wider than the boss is fine.
    let main_src = fs::read_to_string("cylinder_boss.cam/main.part").expect("failed to read cylinder_boss.cam");
    let wide_tool = main_src.replace("dia=10.0", "dia=45.0"); // wider than the 40mm boss
    let part = parse::parse(&wide_tool).expect("a tool wider than the boss must be accepted");
    gcode::program(&part).expect("boss finishing with a wide tool must emit");
}

#[test]
fn test_drill_on_boss_is_an_error() {
    // The variant carries the material side, so the illegal state is
    // unrepresentable: a CylinderBoss is material, and drilling material that the
    // feature says is *kept* is a type error (DECISIONS.md "Row-2.4 `side`
    // selector" — the argument against a side= field an op could misread).
    let src = "\
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)
tool T3 Drill(dia=8.0)
feature Post CylinderBoss(dia=40.0, extent=12.0, x=50.0, y=50.0, z=-12.0)
run Form(tool=T3, points=[Post], cycle=Simple)
";
    let result = parse::parse(src);
    assert!(result.is_err(), "Drill on a CylinderBoss must be a hard error");
    assert!(result.unwrap_err().contains("must be a CylinderBore"), "error should say why");
}

#[test]
fn test_mesh_golden() {
    // Camp 3 (ROADMAP Phase 4): raster drop-cutter (z-map) finishing of an STL
    // triangle mesh — the one finishing path that accepts tessellation +
    // sampling error. The fixture pyramid's anchor values are hand-derived
    // (apex sample Z = 14.000 = apex 10 + r_ball 4; out-of-reach grid corners
    // clamp to the floor Z = 4.000 = mesh bottom 0 + r_ball).
    let main_src = fs::read_to_string("mesh.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("mesh.cam")))
        .expect("failed to parse mesh.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/mesh.golden.gcode").expect("failed to read mesh golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    // The hand-derived anchors, pinned independently of the golden bytes:
    assert!(generated_gcode.contains("G01 X30.000 Y30.000 Z14.000"), "apex sample must ride the vertex contact");
    assert!(generated_gcode.contains("G01 X8.000 Y6.000 Z4.000"), "out-of-reach corner must clamp to the floor");
}

#[test]
fn test_nurbs_patch_golden() {
    // The general NURBS patch (DECISIONS.md "the general NURBS patch…",
    // slice 1): the analytic camp-3 path — the mesh raster contract against
    // the exact tensor-product surface. The fixture cap's anchors are
    // hand-derived (apex sample Z = 13.000 = Bézier centre 9 + r_ball 4, the
    // normal vertical there; out-of-reach grid corners clamp to the floor
    // Z = 4.000 = boundary plane 0 + r_ball).
    let main_src = fs::read_to_string("nurbs_patch.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("nurbs_patch.cam")))
        .expect("failed to parse nurbs_patch.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/nurbs_patch.golden.gcode").expect("failed to read nurbs_patch golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    // The hand-derived anchors, pinned independently of the golden bytes:
    assert!(generated_gcode.contains("G01 X30.000 Y30.000 Z13.000"), "apex sample must rest on the exact apex contact");
    assert!(generated_gcode.contains("G01 X46.000 Y14.000 Z4.000"), "out-of-reach corner must clamp to the floor");
}

#[test]
fn test_nurbs_patch_requires_ball_nose_and_refuses_scallop() {
    // The same ball gate as the mesh (the drop rests a ball on the surface),
    // and the scallop "not yet" — worded as a follow-up, not a permanent
    // error, because the patch's exact curvature exists.
    let main_src = fs::read_to_string("nurbs_patch.cam/main.part").expect("failed to read nurbs_patch.cam");
    let flat = main_src.replace("Ballmill", "Endmill");
    let result = parse::parse_project_with_base(&flat, None, Some(std::path::Path::new("nurbs_patch.cam")));
    assert!(result.is_err(), "Finish on a NurbsPatch with a non-ball tool must be a hard error");
    assert!(result.unwrap_err().contains("Ballmill"), "error should say why");

    let scallop = main_src.replace("stepover=2.0", "scallop=0.05");
    let result = parse::parse_project_with_base(&scallop, None, Some(std::path::Path::new("nurbs_patch.cam")));
    assert!(result.is_err(), "scallop on a NurbsPatch must be a hard error");
    let err = result.unwrap_err();
    assert!(
        err.contains("isn't derived yet") && err.contains("stepover"),
        "the error must name the not-yet and the alternative: {err}"
    );
}

#[test]
fn test_stock_to_leave_on_nurbs_patch_is_rejected() {
    // The Mesh's roughing error, patch edition: a guarantee-ceiling cannot
    // ride the raster's underived between-sample budget; freeform roughing
    // waits for Phase 5 (DECISIONS.md "the general NURBS patch…").
    let main_src = fs::read_to_string("nurbs_patch.cam/main.part").expect("failed to read nurbs_patch.cam");
    let rough = main_src.replace("stepover=2.0", "stepover=2.0, stock_to_leave=0.3");
    let result = parse::parse_project_with_base(&rough, None, Some(std::path::Path::new("nurbs_patch.cam")));
    assert!(result.is_err(), "stock_to_leave on a NurbsPatch must be a hard error");
    let err = result.unwrap_err();
    assert!(
        err.contains("NurbsPatch") && err.contains("Phase 5"),
        "the error must name the carrier and where roughing waits: {err}"
    );
}

#[test]
fn test_mesh_requires_ball_nose() {
    // The drop math rests a ball on the triangles; a flat endmill would gouge
    // every slope — same gate as the quadric surfaces, unlike the flank walls.
    let main_src = fs::read_to_string("mesh.cam/main.part").expect("failed to read mesh.cam");
    let flat = main_src.replace("Ballmill", "Endmill");
    let result = parse::parse_project_with_base(&flat, None, Some(std::path::Path::new("mesh.cam")));
    assert!(result.is_err(), "Finish on a Mesh with a non-ball tool must be a hard error");
    assert!(result.unwrap_err().contains("Ballmill"), "error should say why");
}

#[test]
fn test_mesh_rejects_scallop() {
    // Constant-scallop spacing isn't derived for a sampled tessellation — a
    // clear "not yet", not a silently wrong cusp height.
    let main_src = fs::read_to_string("mesh.cam/main.part").expect("failed to read mesh.cam");
    let scalloped = main_src.replace("stepover=2.0", "scallop=0.5");
    let result = parse::parse_project_with_base(&scalloped, None, Some(std::path::Path::new("mesh.cam")));
    assert!(result.is_err(), "scallop on a Mesh must be a hard error");
    assert!(result.unwrap_err().contains("sampled mesh"), "error should say why");
}

#[test]
fn test_mesh_missing_stl_is_an_error() {
    // A feature whose geometry file can't be read must fail at parse with the
    // file named — not surface later as an empty toolpath.
    let main_src = fs::read_to_string("mesh.cam/main.part").expect("failed to read mesh.cam");
    let missing = main_src.replace("pyramid.stl", "missing.stl");
    let result = parse::parse_project_with_base(&missing, None, Some(std::path::Path::new("mesh.cam")));
    assert!(result.is_err(), "a missing STL must be a hard error");
    let err = result.unwrap_err();
    assert!(err.contains("failed to read mesh") && err.contains("missing.stl"), "error should name the file: {err}");
}

#[test]
fn test_spline_cap_golden() {
    // The analytic NURBS reader (ROADMAP Phase 4 follow-up): ball-nose waterline
    // finishing of a surface of revolution whose meridian is an arbitrary cubic
    // b-spline (spline_cap.cam/meridian.spline). The analytic counterpart to
    // mesh.cam — the same kind of revolved-spline face the freeform_cap STEP
    // fixture tessellates, machined here with NO stair-step error: the contact
    // point, normal, and curvature all come from de Boor evaluation of the
    // spline (see ops::finish::finish_spline_revolve). The apex ring's CL point
    // is hand-derivable from the clamped-spline end tangent ∝ (3,-1): outward
    // normal (1,3)/√10, so CL = (0.949, 22.846) over the (0,20) apex.
    let main_src = fs::read_to_string("spline_cap.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("spline_cap.cam")))
        .expect("failed to parse spline_cap.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/spline_cap.golden.gcode").expect("failed to read spline_cap golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    // The hand-derived apex anchor, pinned independently of the golden bytes.
    assert!(generated_gcode.contains("G00 X50.949 Y40.000"), "apex ring radius must ride the end-tangent normal");
    assert!(generated_gcode.contains("G01 Z22.846"), "apex plunge Z must ride the end-tangent normal");
}

#[test]
fn test_spline_cap_requires_ball_nose() {
    // Same ball-nose requirement as every curved surface: the CL locus is one
    // ball radius along the meridian normal, so a flat endmill would gouge.
    let main_src = fs::read_to_string("spline_cap.cam/main.part").expect("failed to read spline_cap.cam");
    let flat = main_src.replace("Ballmill", "Endmill");
    let result = parse::parse_project_with_base(&flat, None, Some(std::path::Path::new("spline_cap.cam")));
    assert!(result.is_err(), "Finish on a SplineRevolve with a non-ball tool must be a hard error");
    assert!(result.unwrap_err().contains("Ballmill"), "error should say why");
}

#[test]
fn test_spline_cap_scallop_golden() {
    // Constant scallop over the same spline cap (DECISIONS.md "Constant scallop
    // over a spline meridian…"): the family Newton march with the cusp triangle
    // picked per march point from the exact signed curvature, replacing the
    // slice-1 error this test used to pin. The apex ring is scallop-invariant
    // — the march starts at u0 with the same end-tangent CL point the waterline
    // golden hand-derives (normal (1,3)/√10 over the (0,20) apex) — so the same
    // X50.949 / Z22.846 anchor is pinned independently of the golden bytes.
    let main_src = fs::read_to_string("spline_cap_scallop.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("spline_cap_scallop.cam")))
        .expect("failed to parse spline_cap_scallop.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/spline_cap_scallop.golden.gcode").expect("failed to read spline_cap_scallop golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert!(generated_gcode.contains("constant scallop"), "title must name the spacing mode");
    assert!(generated_gcode.contains("G00 X50.949 Y40.000"), "apex ring radius must ride the end-tangent normal");
    assert!(generated_gcode.contains("G01 Z22.846"), "apex plunge Z must ride the end-tangent normal");
}

#[test]
fn test_spline_cap_missing_spline_is_an_error() {
    // A feature whose geometry file can't be read must fail at parse with the
    // file named — not surface later as an empty toolpath (the Mesh rule).
    let main_src = fs::read_to_string("spline_cap.cam/main.part").expect("failed to read spline_cap.cam");
    let missing = main_src.replace("meridian.spline", "missing.spline");
    let result = parse::parse_project_with_base(&missing, None, Some(std::path::Path::new("spline_cap.cam")));
    assert!(result.is_err(), "a missing .spline must be a hard error");
    let err = result.unwrap_err();
    assert!(err.contains("failed to read spline meridian") && err.contains("missing.spline"), "error should name the file: {err}");
}

#[test]
fn test_spline_funnel_golden() {
    // The spline surface of revolution's INSIDE orientation (CHECKLIST row
    // 5.3's last member; DECISIONS.md "Spline funnel…"): ball-nose waterline
    // finishing of a revolved-spline cavity, mouth in the stock-top plane,
    // depth read off the meridian's own z extent. The mouth ring's CL point is
    // hand-derivable from the clamped-spline end tangent ∝ (−1,−3): into-void
    // normal (−3,1)/√10, so CL = (13 − 9/√10, 3/√10) = (10.154, 0.949) off the
    // (13, 0) mouth rim — one ball radius INSIDE it (the cap rule's flip would
    // put it outside, in the material: the vertical-wall tie-break the funnel's
    // own offset home exists for). The tip CL (2.154, −17.051) is the same
    // derivation at the (5, −18) tip pole.
    let main_src = fs::read_to_string("spline_funnel.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("spline_funnel.cam")))
        .expect("failed to parse spline_funnel.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/spline_funnel.golden.gcode").expect("failed to read spline_funnel golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    // The hand-derived mouth and tip anchors, pinned independently of the
    // golden bytes.
    assert!(generated_gcode.contains("G00 X60.154 Y40.000"), "mouth ring radius must ride the end-tangent into-void normal");
    assert!(generated_gcode.contains("G01 Z0.949"), "mouth plunge Z must ride the end-tangent into-void normal");
    assert!(generated_gcode.contains("X52.154") && generated_gcode.contains("Z-17.051"), "final ring must clamp to the tip CL");
}

#[test]
fn test_spline_funnel_scallop_golden() {
    // Constant scallop over the same cavity: the family Newton march handed
    // the void-side pair of homes (spline_revolve_funnel_cl_point /
    // curvature_toward_void_normal). The mouth ring is scallop-invariant (the
    // march starts at u0 with the same end-tangent CL point the waterline
    // golden hand-derives), so the same X60.154 / Z0.949 anchor is pinned
    // independently of the golden bytes.
    let main_src = fs::read_to_string("spline_funnel_scallop.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("spline_funnel_scallop.cam")))
        .expect("failed to parse spline_funnel_scallop.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/spline_funnel_scallop.golden.gcode").expect("failed to read spline_funnel_scallop golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert!(generated_gcode.contains("constant scallop"), "title must name the spacing mode");
    assert!(generated_gcode.contains("G00 X60.154 Y40.000"), "mouth ring radius must ride the end-tangent into-void normal");
    assert!(generated_gcode.contains("G01 Z0.949"), "mouth plunge Z must ride the end-tangent into-void normal");
}

#[test]
fn test_spline_funnel_requires_ball_nose() {
    // The same ball-nose gate as every curved surface — the funnel is not a
    // straight flank, so it takes no exemption.
    let main_src = fs::read_to_string("spline_funnel.cam/main.part").expect("failed to read spline_funnel.cam");
    let flat = main_src.replace("Ballmill", "Endmill");
    let result = parse::parse_project_with_base(&flat, None, Some(std::path::Path::new("spline_funnel.cam")));
    assert!(result.is_err(), "Finish on a SplineRevolveFunnel with a non-ball tool must be a hard error");
    assert!(result.unwrap_err().contains("Ballmill"), "error should say why");
}

#[test]
fn test_spline_funnel_widening_meridian_is_hard_error() {
    // The cavity's undercut, end to end through the project parse: a meridian
    // that widens below its mouth is rejected at validation with the envelope
    // named — never machined into a shadowed wall.
    let widening = "\
degree 1
knots 0 0 0.5 1 1
pole 13 0
pole 9 -8
pole 12 -16
";
    let dir = std::env::temp_dir().join(format!("spline_funnel_widen_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("meridian.spline"), widening).unwrap();
    let main_src = fs::read_to_string("spline_funnel.cam/main.part").expect("failed to read spline_funnel.cam");
    let result = parse::parse_project_with_base(&main_src, None, Some(dir.as_path()));
    fs::remove_dir_all(&dir).ok();
    assert!(result.is_err(), "a widening cavity meridian must be a hard error");
    let err = result.unwrap_err();
    assert!(err.contains("undercut") && err.contains("mouth"), "error should name the cavity envelope: {err}");
}

#[test]
fn test_spline_funnel_oversized_ball_is_refused_naming_the_hoop() {
    // The two-direction gouge guard end to end: this fixture's tightest bound
    // is the HOOP normal-section radius near the r=5 tip circle (≈ 5.3mm), so
    // an 11mm-radius ball is rejected naming that direction — before any
    // g-code is emitted.
    let main_src = fs::read_to_string("spline_funnel.cam/main.part").expect("failed to read spline_funnel.cam");
    let big = main_src.replace("dia=6.0", "dia=22.0");
    let part = parse::parse_project_with_base(&big, None, Some(std::path::Path::new("spline_funnel.cam")))
        .expect("failed to parse modified spline_funnel.cam");
    let result = gcode::program(&part);
    assert!(result.is_err(), "an oversized ball must be a hard error");
    let err = result.unwrap_err();
    assert!(err.contains("gouge") && err.contains("hoop"), "the error should name the binding hoop direction: {err}");
}

#[test]
fn test_torus_bore_too_small_for_tool_is_an_error() {
    // If the ball-nose offset radius (tool radius + tube radius) exceeds the
    // torus's major radius, the inner-bowl passes would have to cross the
    // central axis — geometrically impossible. Must be a hard error, not a
    // silently truncated inner bowl (DECISIONS.md "An unmachinable feature
    // is a hard error, not a skipped op").
    let main_src = fs::read_to_string("torus.cam/main.part").expect("failed to read torus.cam");
    let huge_tool = main_src.replace("dia=6.0", "dia=90.0");
    let part = parse::parse(&huge_tool).expect("failed to parse modified torus.cam");
    let result = gcode::program(&part);
    assert!(result.is_err(), "torus bore smaller than the ball offset must be a hard error");
    assert!(result.unwrap_err().contains("cross the central axis"), "error should say why");
}

#[test]
fn test_cone_scallop_exceeding_ball_radius_is_an_error() {
    // The constant-scallop cusp-height formula is only valid for
    // scallop <= r_ball; above that it must be a hard error, not a silent
    // clamp to the max value.
    let main_src = fs::read_to_string("cone_scallop.cam/main.part").expect("failed to read cone_scallop.cam");
    let oversized_scallop = main_src.replace("scallop=0.5", "scallop=5.0");
    let part = parse::parse(&oversized_scallop).expect("failed to parse modified cone_scallop.cam");
    let result = gcode::program(&part);
    assert!(result.is_err(), "scallop larger than the ball radius must be a hard error");
    assert!(result.unwrap_err().contains("exceeds tool"), "error should say why");
}

#[test]
fn test_unmachinable_drill_is_an_error() {
    let main_src = fs::read_to_string("cylinder_bore_form_points.cam/main.part").expect("failed to read main.part");
    let huge_tool = main_src.replace("dia=8.0", "dia=12.0");
    let result = parse::parse(&huge_tool);
    assert!(result.is_err(), "too-small hole must be a hard error, not a skipped or gouging op");
    assert!(result.unwrap_err().contains("larger than cylinder hole"), "error should say why");
}


// ---- G-code safety lint -----------------------------------------------------
// The one home for the emitted-program safety rules (AGENTS.md): units set
// before any motion, G43 length comp active after every tool change before
// that tool's first cutting move, every op starting with a retract, and the
// spindle on before any feed move. New ops must pass this lint by being added
// to `lint_fixtures` — not by restating the rules in their own tests.
//
// The retract check is structural (first motion after an op header must be a
// pure Z rapid); it does not verify the Z value equals the op's clearance_z,
// since per-op overrides aren't recoverable from the text alone.

fn lint_gcode(program: &str, stock_top: f64) -> Result<(), String> {
    let is_motion = |l: &str| ["G00", "G01", "G02", "G03"].iter().any(|p| l.starts_with(p));
    let is_cutting = |l: &str| ["G01", "G02", "G03"].iter().any(|p| l.starts_with(p));

    let mut units_lines = 0;
    let mut motion_seen = false;
    let mut spindle_on = false;
    let mut comp_pending = false; // set by M06, cleared by G43
    let mut expect_retract = false; // set by an op header comment
    let mut last_motion = String::new();

    for line in program.lines() {
        // The restart ladder's label is invisible to certificates: strip it
        // through the one reader-side home (DECISIONS.md → "N-words…",
        // fork 4A) — a literal-prefix lint that kept the label would go
        // green-but-blind on every N-numbered line.
        let l = gcode::strip_n_word(line.trim());
        if l == "G20" || l == "G21" {
            units_lines += 1;
            if motion_seen {
                return Err(format!("units set after a motion command: {}", l));
            }
        }
        if l.starts_with("M06") {
            comp_pending = true;
        }
        // A program at several work offsets changes register between parts,
        // and only with the tool up: the motion before it is a Z-only rapid.
        if matches!(l, "G54" | "G55" | "G56" | "G57" | "G58" | "G59") {
            if !(last_motion.starts_with("G00") && last_motion.contains('Z') && !last_motion.contains('X') && !last_motion.contains('Y')) {
                return Err(format!("register {} is selected after '{}', not after a retract", l, last_motion));
            }
            let z: f64 = last_motion.split_whitespace().find_map(|w| w.strip_prefix('Z')).and_then(|v| v.parse().ok()).unwrap_or(f64::MIN);
            if z <= stock_top {
                return Err(format!("register {} is selected at Z{}, not above the stock top {}", l, z, stock_top));
            }
        }
        if l.contains("G43") {
            comp_pending = false;
        }
        if l.starts_with("M03") {
            spindle_on = true;
            // A spindle nobody started (Phase 50 slice 1, B1).
            if l.split_whitespace().any(|w| w == "S0.000") {
                return Err(format!("spindle started at S0: {}", l));
            }
        }
        if l.starts_with("M05") {
            spindle_on = false;
        }
        if l.starts_with("(---") {
            expect_retract = true;
            continue;
        }
        if is_motion(l) {
            last_motion = l.to_string();
            if units_lines == 0 {
                return Err(format!("motion before units were set: {}", l));
            }
            motion_seen = true;
            if expect_retract {
                if !(l.starts_with("G00") && l.contains('Z') && !l.contains('X') && !l.contains('Y')) {
                    return Err(format!("op does not start with a Z retract: {}", l));
                }
                // A retract is ABOVE the stock (Phase 50 slice 1): a
                // `clearance_z` of 0 on a stock whose top is 0 passed this
                // lint as a retract to the material's surface.
                let z: f64 = l.split_whitespace().find_map(|w| w.strip_prefix('Z')).and_then(|v| v.parse().ok()).ok_or_else(|| format!("retract carries no Z number: {}", l))?;
                if z <= stock_top {
                    return Err(format!("op's retract Z{} is not above the stock top {}: {}", z, stock_top, l));
                }
                expect_retract = false;
            }
        }
        if is_cutting(l) {
            if comp_pending {
                return Err(format!("cutting move before G43 length comp: {}", l));
            }
            if !spindle_on {
                return Err(format!("feed move with the spindle off: {}", l));
            }
            // A feed nobody chose (Phase 50 slice 1, B1).
            if l.split_whitespace().any(|w| w == "F0.000") {
                return Err(format!("cutting move at F0: {}", l));
            }
        }
    }
    if units_lines != 1 {
        return Err(format!("expected exactly one G20/G21 line, found {}", units_lines));
    }
    Ok(())
}

#[test]
fn test_gcode_safety_lint() {
    let lint_fixtures: &[(&str, Option<&str>)] = &[
        ("plate.cam/main.part", Some("plate.cam/geometry.part")),
        ("plate_boss.cam/main.part", None),
        ("plate_floated.cam/main.part", None),
        ("face_clear.cam/main.part", None),
        ("pocket_levels.cam/main.part", None),
        // The N-numbered ladder: the taught lint must examine a labeled
        // program, not go green-but-blind past it (DECISIONS.md →
        // "N-words…", fork 4A).
        ("floor_restart.cam/main.part", None),
        ("stock_profile.cam/main.part", None),
        ("profile_pocket_clear.cam/main.part", None),
        ("profile_pocket_thinned.cam/main.part", None),
        ("floor_clamped_offset.cam/main.part", None),
        ("profile_pocket_clear_rough.cam/main.part", None),
        ("cylinder_bore_form_points.cam/main.part", None),
        ("csink.cam/main.part", None),
        ("cbore.cam/main.part", None),
        ("tilted_bore.cam/main.part", None),
        ("step_bore.cam/main.part", None),
        ("disc_step.cam/main.part", None),
        ("disc_ribs.cam/main.part", Some("disc_ribs.cam/geometry.part")),
        ("rim_boss_offset.cam/main.part", Some("rim_boss_offset.cam/geometry.part")),
        ("tube_pocket.cam/main.part", None),
        ("tube_pocket_spine.cam/main.part", None),
        ("profile_pocket.cam/main.part", None),
        ("profile_pocket_rough.cam/main.part", None),
        ("profile_pocket_arc.cam/main.part", None),
        ("profile_boss_arc.cam/main.part", None),
        ("contour_pocket.cam/main.part", None),
        ("contour_boss.cam/main.part", None),
        ("contour_pocket_rough.cam/main.part", None),
        ("sphere.cam/main.part", None),
        ("sphere_clear.cam/main.part", None),
        ("sphere_scallop.cam/main.part", None),
        ("cone.cam/main.part", None),
        ("cone_scallop.cam/main.part", None),
        ("torus.cam/main.part", None),
        ("torus_scallop.cam/main.part", None),
        ("sphere_formtool.cam/main.part", None),
        ("cylinder_bore.cam/main.part", None),
        ("cylinder_bore_arcs.cam/main.part", None),
        ("cylinder_boss.cam/main.part", None),
        ("cylinder_boss_arcs.cam/main.part", None),
        ("sphere_arcs.cam/main.part", None),
        ("mesh.cam/main.part", None),
        ("nurbs_patch.cam/main.part", None),
        ("spline_cap.cam/main.part", None),
        ("spline_cap_scallop.cam/main.part", None),
        ("spline_funnel.cam/main.part", None),
        ("spline_funnel_shank.cam/main.part", None),
        ("spline_funnel_holder.cam/main.part", None),
        ("spline_funnel_scallop.cam/main.part", None),
        ("sphere_rough.cam/main.part", None),
        ("cylinder_bore_rough.cam/main.part", None),
        ("torus_rough.cam/main.part", None),
        ("cone_rough.cam/main.part", None),
        ("spline_cap_rough.cam/main.part", None),
        ("spline_funnel_rough.cam/main.part", None),
        // The form-tool passes. The edge_round pair is a BACKFILL — the
        // corner-rounder thread landed without registering them here (found
        // 2026-08-08); the chamfer pair lands with its own slice, as the rule
        // requires.
        ("edge_round.cam/main.part", None),
        ("edge_round_profile.cam/main.part", None),
        ("chamfer.cam/main.part", None),
        ("chamfer_profile.cam/main.part", None),
        ("chamfer_marks.cam/main.part", None),
        ("chamfer_deep.cam/main.part", None),
        // K3's acceptance fixture: the derived S word must satisfy the same
        // spindle-before-feed ordering as any declared one.
        ("floor_derive.cam/main.part", None),
        // K3 slice 1: both derived words (S and F) must satisfy the same
        // rules as declared ones.
        ("floor_derive_feed.cam/main.part", None),
        // The K1 declared-machine fixture: the integer S word must satisfy
        // the same spindle-before-feed rule as the decimal spelling.
        ("cylinder_bore_machine.cam/main.part", None),
        ("floor_travel.cam/main.part", None),
        // K1 slice 1: the inch-output program must satisfy every rule under
        // its G20 spelling — the lint already reads G20 as the units word
        // (its `l == "G20" || l == "G21"` arm predates this fixture), so
        // units-before-motion is checked, not skipped, on an inch program.
        // The inch-INPUT fixture rides through as plain metric output.
        ("cylinder_bore_inch.cam/main.part", None),
        ("cylinder_bore_inch_input.cam/main.part", None),
        // Phase 26 slice 2: the pocket that goes around islands — several
        // ramps in one op, and island finishes entering through its annulus.
        ("pocket_post.cam/main.part", None),
        ("pocket_islands.cam/main.part", None),
        // Slice 3: a face entered from air outside the blank, around two bosses.
        ("face_boss_offset.cam/main.part", None),
        // Phase 27 slice 2: a pocket with open edges, entered by a lead-in
        // from air outside the blank.
        ("step.cam/main.part", None),
        // Phase 27 slice 3: a face around a riser block, lifts at the
        // clearance plane.
        ("floor_traverse_offset.cam/main.part", None),
        // Slice 4: rough with an allowance, face the floor, finish the walls.
        ("pocket_floor_offset.cam/main.part", None),
        // Phase 28 slice 1: a `Face` — the wrench's top, not the bar's.
        ("wrench_offset.cam/main.part", Some("wrench_offset.cam/geometry.part")),
        // Slice 3: the pinned patterns — ramps along a stroke, boundary links.
        ("wrench_zigzag.cam/main.part", Some("wrench_zigzag.cam/geometry.part")),
        ("wrench_oneway.cam/main.part", Some("wrench_oneway.cam/geometry.part")),
        // Phase 29 slice 1: the headland — a face with a raised neighbour, so
        // one of its edges is CLOSED and the swath is clipped back out of it.
        ("wrench_step_offset.cam/main.part", Some("wrench_step_offset.cam/geometry.part")),
        ("wrench_step_zigzag.cam/main.part", Some("wrench_step_zigzag.cam/geometry.part")),
        ("wrench_step_oneway.cam/main.part", Some("wrench_step_oneway.cam/geometry.part")),
        // Phase 29 slice 2: a wall that is a BOSS (the room's two walls, the
        // fence, the flowerbed), a wall chain terminating mid-side, and a deck
        // walled at both ends.
        ("cottage_interior_offset.cam/main.part", None),
        ("lawn_fence_bed_offset.cam/main.part", None),
        ("wrench_thick_offset.cam/main.part", Some("wrench_thick_offset.cam/geometry.part")),
        ("step_zigzag.cam/main.part", None),
        // Phase 30: spine — rows at any named angle, straight links, shore
        // passes with a lead on each end.
        ("wrench_spine.cam/main.part", Some("wrench_spine.cam/geometry.part")),
        ("wrench_step_spine.cam/main.part", Some("wrench_step_spine.cam/geometry.part")),
        ("lawn_fence_bed_spine.cam/main.part", None),
        ("pocket_post_spine.cam/main.part", None),
        // Phase 62 slice 1: held — spine's comb from the air at the spacing
        // the ceiling derives, a lift where a link would exceed it.
        ("plate_held.cam/main.part", Some("plate_held.cam/geometry.part")),
        ("pocket_post_held.cam/main.part", None),
        // ROADMAP item 2 slice 2: the plate cut conventional — the loops the
        // other way round, G42 under control comp.
        ("plate_conventional.cam/main.part", Some("plate_conventional.cam/geometry.part")),
        ("plate_angled.cam/main.part", Some("plate_angled.cam/geometry.part")),
        ("pocket_post_plunged.cam/main.part", None),
        ("pocket_tall_post.cam/main.part", None),
        ("pocket_wall_lead.cam/main.part", None),
        // Phase 30 slice 3: the rail plate — three faces, dozens of lifts.
        ("rail_plate_offset.cam/main.part", Some("rail_plate_offset.cam/geometry.part")),
        ("rail_plate_spine.cam/main.part", Some("rail_plate_spine.cam/geometry.part")),
        // Phase 31: the curved rail plate.
        ("curved_rails_offset.cam/main.part", Some("curved_rails_offset.cam/geometry.part")),
        ("curved_rails_spine.cam/main.part", Some("curved_rails_spine.cam/geometry.part")),
        // Phase 32, after the close: a neighbour thinner than the push, an island of the swath.
        ("walled_slot.cam/main.part", None),
        ("thin_wall.cam/main.part", None),
        // Phase 33 slice 1: the rest floor — three small floors, each entered
        // by its own ramp, after the 10mm tool's concession.
        ("walled_rails_rest_offset.cam/main.part", Some("walled_rails_rest_offset.cam/geometry.part")),
        // Slice 3: the ladder — a rest op that concedes, and one after it.
        ("walled_rails_ladder_offset.cam/main.part", Some("walled_rails_ladder_offset.cam/geometry.part")),
        // Slice 4: the rest floor walked by spine.
        ("walled_rails_rest_spine.cam/main.part", Some("walled_rails_rest_spine.cam/geometry.part")),
        // Slice 5: a Plane concedes its web, and a rest op takes it.
        ("deck_rest_offset.cam/main.part", None),
        // Phase 34 slice 1: the wide floors — a 6mm tool on a field twenty
        // diameters across, and a pocket of twelve posts (a ramp per nest).
        ("open_field.cam/main.part", None),
        ("post_field.cam/main.part", None),
        // Phase 48 slice 1: the second setup — a plate bored from the top,
        // then turned over and chamfered, the back reading the front's air
        // through `given from`.
        ("through_plate.cam/main.part", None),
        ("through_plate_back.cam/main.part", None),
        // Three parts a run, one in each of three registers.
        ("several_offsets.cam/main.part", None),
    ];
    // (Phase 32's `walled_rails_offset.cam` and `_open` refuse at validate by the
    // passage law, so they emit no program to lint — pinned in spine_tests.)
    for (main_path, geom_path) in lint_fixtures {
        let main_src = fs::read_to_string(main_path).expect("failed to read fixture");
        let geom_src = geom_path.map(|p| fs::read_to_string(p).expect("failed to read geometry"));
        // base_dir is the fixture's own directory (matching main.rs's
        // directory-parse path) so a fixture with an on-disk relative asset,
        // like sphere_formtool.cam's tool_profile.svg, resolves correctly. A
        // no-op for every other fixture here, none of which reference one.
        let base_dir = std::path::Path::new(main_path).parent();
        let part = parse::parse_project_with_base(&main_src, geom_src.as_deref(), base_dir).expect("failed to parse fixture");
        let program = gcode::program(&part).expect("failed to generate program");
        if let Err(e) = lint_gcode(&program, part.stock.z) {
            panic!("safety lint failed for {}: {}", main_path, e);
        }
    }
}

#[test]
fn test_svg_profile_parses_via_project_file() {
    // Exercises SVG profile *parsing* through a full project file (tool line
    // -> SVG file read -> scaled `tool.profile` points). Consumption is
    // covered separately: `ops::finish::finish_sphere_profile` reads
    // `tool.profile` (test_sphere_formtool_golden and the
    // test_form_tool_profile_* error tests below). The End op here is only
    // a vehicle to make the project parse; it does not read the profile.
    // (Historical note: this test was once named
    // ..._but_has_no_consumer_yet, from the era DECISIONS.md's "Self-audit
    // corrections" entry records, before the consumer landed.)
    let svg_content = r#"<svg>
  <path d="M 0,0 L 2,0 C 3,1 4,2 5,3 Z"/>
</svg>"#;
    let svg_path = "test_integration_profile.svg";
    fs::write(svg_path, svg_content).unwrap();

    let main_src = r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=100, l=100, h=20)
tool T1 FormTool(dia=8.0, profile="test_integration_profile.svg")
tool T2 Endmill(dia=8.0)
feature Top Plane(z=-0.5)
run End(tool=T2, surface=Top, stepover=4.0)
"#;
    
    let part = parse::parse_project(&main_src, None);
    let _ = fs::remove_file(svg_path);
    
    let part = part.expect("failed to parse");
    let tool = part.tools.get(&1).expect("T1 not found");
    assert_eq!(tool.name, "FormTool");
    assert_eq!(tool.dia, 8.0);
    
    let profile = tool.profile.as_ref().expect("profile not found");
    assert!(!profile.is_empty());
    
    let max_r = profile.iter().map(|p| p.x).fold(0.0f64, |a, b| a.max(b));
    assert!((max_r - 4.0).abs() < 1e-9);
}

// ---- Custom tool profile (FormTool) finishing a Sphere ---------------------
// ops::finish's vertex-following contact math closes the gap the tests above
// deliberately left open. The happy path is covered by test_sphere_formtool_golden
// (an exact-match golden fixture, per this file's usual convention for a new
// finishing op) and by sphere_formtool.cam's entry in test_gcode_safety_lint's
// fixture list; the tests below cover the hard-error branches.
//
// Shared fixture profile (pre-scale, `dia=10` -> scale factor 5): tip (0,0)
// -> corner (1,1) -> wall-top (1,2), which scales to (0,0) -> (5,5) ->
// (5,10) -- a 45 deg segment (tip->corner) followed by a vertical (90 deg)
// wall segment, chosen so the two segment angles are hand-computable and the
// corner sits exactly at the sphere's midway angle (see sphere_formtool.cam's
// header comment for the worked numbers).

fn write_sphere_form_tool_svg(path: &str, d: &str) {
    std::fs::write(path, format!("<svg>\n  <path d=\"{}\"/>\n</svg>", d)).unwrap();
}

// Every error case below is the same program with exactly one thing wrong
// (the SVG profile shape, or the feature/run line) -- one shared template
// with two substitution points, matching this file's usual fixture+replace
// convention (e.g. test_torus_bore_too_small_for_tool_is_an_error) as
// closely as a per-test SVG shape allows.
fn form_tool_main_src(svg_path: &str, feature_and_run: &str) -> String {
    format!(
        r#"
units mm
feed 600
plunge 200
rapid_z 5
clearance_z 30
spindle 4000
stock Box(w=100, l=80, h=25, x=0, y=0, z=0)
tool T1 FormTool(dia=10.0, profile="{svg_path}")
{feature_and_run}
"#
    )
}

const DOME_FEATURE_AND_RUN: &str = "feature Dome Sphere(dia=20, x=30, y=20, z=0)\nrun Form(tool=T1, surface=Dome, stepover=3.0)";

#[test]
fn test_form_tool_profile_scallop_is_an_error() {
    // Constant-scallop spacing isn't derived for an arbitrary profile (only
    // the closed-form ball case has one) -- must be a clear "not yet", not a
    // silently wrong cusp height.
    let svg_path = "test_sphere_profile_scallop.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1 L 1,2");

    let main_src = form_tool_main_src(svg_path, "feature Dome Sphere(dia=20, x=30, y=20, z=0)\nrun Form(tool=T1, surface=Dome, scallop=0.5)");
    let result = parse::parse_project(&main_src, None);
    let _ = fs::remove_file(svg_path);
    assert!(result.is_err(), "scallop with a custom tool profile must be a hard error");
    assert!(result.unwrap_err().contains("scallop"), "error should say why");
}

#[test]
fn test_form_tool_profile_requires_sphere_feature() {
    // Cone's apex vertex and Torus's inner bowl need their own extension of
    // the contact math (not derived here) -- referencing one with a profile
    // tool must be a clear "not yet", not a silently wrong path.
    let svg_path = "test_sphere_profile_cone.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1 L 1,2");

    let main_src = form_tool_main_src(svg_path, "feature Spike Cone(dia=20, height=15, x=30, y=20, z=0)\nrun Form(tool=T1, surface=Spike, stepover=3.0)");
    let result = parse::parse_project(&main_src, None);
    let _ = fs::remove_file(svg_path);
    assert!(result.is_err(), "a custom tool profile on a Cone must be a hard error");
    assert!(result.unwrap_err().contains("only Sphere features"), "error should say why");
}

#[test]
fn test_form_tool_profile_on_mesh_is_an_error() {
    // The vertex-following contact math is derived for Sphere only; dropping
    // an arbitrary profile onto an arbitrary triangle is its own extension.
    // Pins that the existing Sphere-only guard covers the camp-3 Mesh too.
    let svg_path = "test_sphere_profile_mesh.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1 L 1,2");
    let stl_path = "test_form_tool_mesh.stl";
    fs::write(
        stl_path,
        "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 10 0 0\nvertex 10 5 5\nendloop\nendfacet\nendsolid t\n",
    )
    .expect("failed to write test STL");

    let main_src = form_tool_main_src(
        svg_path,
        "feature Blob Mesh(file=\"test_form_tool_mesh.stl\")\nrun Form(tool=T1, surface=Blob, stepover=3.0)",
    );
    let result = parse::parse_project(&main_src, None);
    let _ = fs::remove_file(svg_path);
    let _ = fs::remove_file(stl_path);
    assert!(result.is_err(), "a custom tool profile on a Mesh must be a hard error");
    assert!(result.unwrap_err().contains("only Sphere features"), "error should say why");
}

#[test]
fn test_form_tool_profile_must_reach_tool_axis() {
    // A profile whose tip isn't at r=0 can never touch a dome's pole -- must
    // be a hard error, not a path that quietly skips the pole.
    let svg_path = "test_sphere_profile_offset_tip.svg";
    write_sphere_form_tool_svg(svg_path, "M 1,0 L 2,1 L 2,2");

    let main_src = form_tool_main_src(svg_path, DOME_FEATURE_AND_RUN);
    let part = parse::parse_project(&main_src, None).expect("failed to parse");
    let _ = fs::remove_file(svg_path);
    let result = gcode::program(&part);
    assert!(result.is_err(), "a profile that doesn't reach the tool axis must be a hard error");
    assert!(result.unwrap_err().contains("does not reach the tool axis"), "error should say why");
}

#[test]
fn test_form_tool_profile_must_reach_equator() {
    // A profile that never reaches a 90 deg normal angle can't finish a
    // dome out to its equator -- must be a hard error, not a truncated dome.
    let svg_path = "test_sphere_profile_shallow.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1");

    let main_src = form_tool_main_src(svg_path, DOME_FEATURE_AND_RUN);
    let part = parse::parse_project(&main_src, None).expect("failed to parse");
    let _ = fs::remove_file(svg_path);
    let result = gcode::program(&part);
    assert!(result.is_err(), "a profile that never reaches 90 deg must be a hard error");
    assert!(result.unwrap_err().contains("only reaches a"), "error should say why");
}

/// The band's tool line (K2 slice 2, fork 3A): the shared 45°+wall profile
/// with a declared cutting band appended, everything else the error-template
/// program. Post-scale the contact rides vertex (0,0) at the pole and vertex
/// (5,5) from the dome's midway angle out, so the used span is [0, 5] mm and
/// every shortfall below is hand-checkable.
fn form_tool_band_src(svg_path: &str, band: &str) -> String {
    form_tool_main_src(svg_path, DOME_FEATURE_AND_RUN).replace(
        &format!("profile=\"{svg_path}\")"),
        &format!("profile=\"{svg_path}\"{band})"),
    )
}

#[test]
fn test_form_tool_band_refuses_contact_above_cut_top() {
    // The dome needs contact up to the 5mm corner vertex; a band ending at
    // 4mm makes that last millimetre relieved surface — rejected with the
    // shortfall as a number, not cut with a surface the tool cannot gauge.
    let svg_path = "test_sphere_profile_band_top.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1 L 1,2");
    let main_src = form_tool_band_src(svg_path, ", cut_top=4.0");
    let part = parse::parse_project(&main_src, None).expect("failed to parse");
    let _ = fs::remove_file(svg_path);
    let err = gcode::program(&part).expect_err("contact outside the band must be a hard error");
    assert!(err.contains("cut_top 4.000"), "the error must quote the band: {err}");
    assert!(err.contains("up to 5.000mm"), "the error must quote the contact span: {err}");
    assert!(err.contains("1.000mm of contact"), "the shortfall is a number: {err}");
}

#[test]
fn test_form_tool_band_refuses_contact_below_cut_bottom() {
    // The dome's pole is finished by the tip vertex itself, so any positive
    // cut_bottom puts the pole's contact on relieved surface.
    let svg_path = "test_sphere_profile_band_bottom.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1 L 1,2");
    let main_src = form_tool_band_src(svg_path, ", cut_bottom=0.5");
    let part = parse::parse_project(&main_src, None).expect("failed to parse");
    let _ = fs::remove_file(svg_path);
    let err = gcode::program(&part).expect_err("pole contact under the band must be a hard error");
    assert!(err.contains("cut_bottom 0.500"), "the error must quote the band: {err}");
    assert!(err.contains("starts 0.000mm"), "the error must quote the contact: {err}");
}

#[test]
fn test_form_tool_band_covering_the_contact_changes_nothing() {
    // A band that covers the used span exactly is the default said out loud:
    // the program must emit byte for byte what the undeclared twin emits.
    let svg_path = "test_sphere_profile_band_cover.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1 L 1,2");
    let bare = parse::parse_project(&form_tool_main_src(svg_path, DOME_FEATURE_AND_RUN), None)
        .expect("failed to parse");
    let banded =
        parse::parse_project(&form_tool_band_src(svg_path, ", cut_bottom=0.0, cut_top=5.0"), None)
            .expect("failed to parse");
    let _ = fs::remove_file(svg_path);
    let g_bare = gcode::program(&bare).expect("the undeclared twin emits");
    let g_band = gcode::program(&banded).expect("a covering band emits");
    assert_eq!(g_bare, g_band, "a covering band must change nothing");
}

#[test]
fn test_form_tool_profile_radius_must_not_decrease() {
    // A profile whose radius decreases isn't a simple outward flare from the
    // tip, so the vertex-following contact math can't reason about it safely.
    let svg_path = "test_sphere_profile_radius_dip.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 2,0 L 1,1");

    let main_src = form_tool_main_src(svg_path, DOME_FEATURE_AND_RUN);
    let part = parse::parse_project(&main_src, None).expect("failed to parse");
    let _ = fs::remove_file(svg_path);
    let result = gcode::program(&part);
    assert!(result.is_err(), "a profile radius decrease must be a hard error");
    assert!(result.unwrap_err().contains("radius decreases"), "error should say why");
}

#[test]
fn test_form_tool_profile_must_be_outward_convex() {
    // A profile whose normal angle decreases along its length (a concave dip)
    // could let contact migrate to an unintended vertex and gouge -- must be
    // a hard error, not a silently wrong path.
    let svg_path = "test_sphere_profile_concave.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,0 L 1,2 L 2,2.01");

    let main_src = form_tool_main_src(svg_path, DOME_FEATURE_AND_RUN);
    let part = parse::parse_project(&main_src, None).expect("failed to parse");
    let _ = fs::remove_file(svg_path);
    let result = gcode::program(&part);
    assert!(result.is_err(), "a concave (non-outward-convex) profile must be a hard error");
    assert!(result.unwrap_err().contains("not outward-convex"), "error should say why");
}

// ---- analytical op-history stock: the model and its first consumer ----------
// (DECISIONS.md "Analytical op-history stock…", ROADMAP "Stock-aware planning
// for concave / form-profile cutters" level 1.) The model answers what the
// stock still holds after a prefix of the run sequence; the lint asks whether
// the tool's body stands in material the op does not remove.

#[test]
fn test_stock_model_subtracts_the_void_a_bore_declares() {
    // The rule the whole model rests on: after the op, the declared geometry
    // stands. So the bore's cylinder is gone, and only it.
    let src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read cylinder_bore.cam");
    let part = parse::parse(&src).expect("failed to parse cylinder_bore.cam");
    let remaining = nacs::stock::remaining_after(&part, part.operations.len() - 1);

    assert!(remaining.complete, "every op here has a closed-form removal");
    // Bore dia 40 at (50,50), extent 12 into a 20mm-thick stock topped at z=0.
    assert!(!remaining.material_at(50.0, 50.0, -6.0), "inside the bore is void");
    assert!(remaining.material_at(50.0, 50.0, -15.0), "below the bore floor is material");
    assert!(remaining.material_at(90.0, 50.0, -6.0), "outside the bore is material");
    assert!(!remaining.material_at(50.0, 50.0, 5.0), "above the stock is air");
}

#[test]
fn test_stock_model_reports_itself_incomplete_rather_than_guessing() {
    // How far out from a boss the surrounding material was cleared is a fact
    // about the PATH, not about the declared feature — so there is no closed
    // form, and total accounting means saying so out loud. As of Phase 10
    // lane A the CYLINDER boss states its band (and the moat in front of it),
    // so the model is complete there; the profile boss is still the example.
    let src = fs::read_to_string("cylinder_boss.cam/main.part").expect("read cylinder_boss.cam");
    let part = parse::parse(&src).expect("failed to parse cylinder_boss.cam");
    let remaining = nacs::stock::remaining_after(&part, part.operations.len() - 1);
    assert!(remaining.complete, "the cylinder boss finish has its band since Phase 10 lane A");

    let src = fs::read_to_string("contour_pocket.cam/main.part").expect("read contour_pocket.cam");
    let part = parse::parse(&src).expect("parse");
    let remaining = nacs::stock::remaining_after(&part, part.operations.len() - 1);
    assert!(!remaining.complete, "a rim wire's interior is undeclared, so its finish declines by name (T5's vocabulary)");
}

#[test]
fn test_body_foul_lint_is_quiet_on_every_shipped_fixture() {
    // The lint's failure direction is "quiet when unsure", so a fixture set
    // that is already well-formed must stay silent — this is the noise pin.
    // The R2 cavities are here for a second reason: a ball's shank is exactly
    // as wide as its ball and a non-undercut cavity only widens going up, so
    // the probe cannot fire for a ball that concave_gouge_check already passed.
    let quiet = [
        "sphere_formtool.cam", "sphere.cam", "cylinder_bore.cam",
        "spline_funnel.cam",
        // Shipped WITH a rim foul from its first day — the advisory its header
        // used to call inherent. The swept-tube ceiling made it a defect and
        // then fixed it (DECISIONS.md → "rim-area accounting…", slice 0), so
        // the first contained-stock fixture now holds the line the 26 stocks
        // will be held to.
        "sphere_clear.cam",
        // The last three to be contained, and the only ones whose Clear opens
        // a hole as well as a moat (DECISIONS.md → "the torus's annulus
        // island…"): a ball working a rim from INSIDE is where a mis-shaped
        // claim would show first.
        "torus.cam", "torus_rough.cam", "torus_scallop.cam",
    ];
    for fixture in quiet {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
        let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new(fixture)))
            .expect("failed to parse fixture");
        let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
        assert!(fouls.is_empty(), "{fixture}: expected no foul warning, got: {:?}", fouls);
    }
}

#[test]
fn test_body_foul_lint_reads_the_allowance_not_the_nominal_wall() {
    // A roughing pass rides its own offset surface. Reading the nominal wall
    // would accuse it of fouling the very skin it was told to leave.
    let src = fs::read_to_string("cylinder_bore_rough.cam/main.part").expect("read cylinder_bore_rough.cam");
    let part = parse::parse(&src).expect("failed to parse cylinder_bore_rough.cam");
    let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
    assert!(fouls.is_empty(), "the allowance is remaining material the op keeps clear of, got: {:?}", fouls);
}

// ---- K2, the tool body — slice 0: the wide-shank acceptance fixture
// (DECISIONS.md "K2, the tool body"; ROADMAP Phase 7 K2) ----------------------

/// The phase item's acceptance, met: one EXISTING lint — the body-foul
/// advisory — sees a body it was blind to, with no new lint code. The fixture
/// is stock.rs's staged unit pin promoted through the parser: the funnel's
/// tip circle is r 8, the declared bar above 3mm of ball flutes is r 10, so
/// the tip reaches the floor and the bar stands inside the tractrix wall.
#[test]
fn test_the_body_foul_advisory_sees_a_declared_wide_shank() {
    let src = fs::read_to_string("spline_funnel_shank.cam/main.part")
        .expect("read spline_funnel_shank.cam");
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_shank.cam"))).expect("the body declaration parses");
    let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
    assert_eq!(fouls.len(), 1, "the declared bar is buried in the wall, got: {:?}", fouls);
    assert!(
        fouls[0].message.contains("Vortex") && fouls[0].message.contains("r 10.000"),
        "the warning names the feature and the body radius the rod could not say: {}",
        fouls[0]
    );

    // The control: strip the body fields and the tool is today's rod again —
    // constant-radius as high as it reaches, and the advisory has nothing to
    // see. This IS the blindness the fixture closes, run both ways.
    let blind = src.replace(
        "tool T10 Ballmill(dia=6.0, flute_len=3.0, shank_dia=20.0, stickout=30.0, flute_count=2)",
        "tool T10 Ballmill(dia=6.0)",
    );
    let part = parse::parse_project_with_base(&blind, None, Some(std::path::Path::new("spline_funnel_shank.cam"))).expect("the undeclared rod parses");
    let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
    assert!(fouls.is_empty(), "an undeclared body means today's behavior: {:?}", fouls);
}

/// Slice 0 moves no G-code: the advisory is stderr, and the emitted program is
/// byte-identical to spline_funnel.cam's own golden — the same funnel,
/// the same ball, the body fields changing nothing but what the certificates
/// can see. This equality is the fixture's golden.
#[test]
fn test_the_declared_body_moves_no_gcode() {
    let src = fs::read_to_string("spline_funnel_shank.cam/main.part")
        .expect("read spline_funnel_shank.cam");
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_shank.cam"))).expect("parse");
    let g = gcode::program(&part).expect("the advisory does not reject the program");
    let golden = fs::read_to_string("tests/spline_funnel.golden.gcode")
        .expect("read spline_funnel golden");
    assert_eq!(g.trim(), golden.trim(), "the body fields must not move a byte of output");
}

// ---- K2, the tool body — slice 1: the holder-nose acceptance fixture
// (DECISIONS.md "K2, the tool body", slice 1) ---------------------------------

/// The holder-nose foul, through the parser, with zero new lint code — the
/// deep-cavity case the design's fork 2C named as what this item exists for.
/// The tool itself is honest (the r 5 shank clears the tractrix wall by
/// 4.45mm at its closest), but the 8mm stickout puts the holder's nose face
/// below the stock top, and the r 16 nose stands against a wall of r 12.76
/// there — so the advisory's report must carry the NOSE's radius.
#[test]
fn test_the_body_foul_advisory_sees_a_declared_holder_nose() {
    let src = fs::read_to_string("spline_funnel_holder.cam/main.part")
        .expect("read spline_funnel_holder.cam");
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("the holder declaration parses");
    let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
    assert_eq!(fouls.len(), 1, "the nose is buried in the wall, got: {:?}", fouls);
    assert!(
        fouls[0].message.contains("Vortex") && fouls[0].message.contains("r 16.000"),
        "the warning names the feature and the holder radius the tool alone could not reach: {}",
        fouls[0]
    );

    // The control: strip the holder and the honest tool underneath returns
    // to silence — the blindness this fixture closes, run both ways.
    let blind = src.replace(
        ", holder=\"ER32 slim nose\", holder_nose_dia=32.0, holder_nose_len=40.0, holder_body_dia=50.0)",
        ")",
    );
    assert_ne!(blind, src, "the strip must actually strip");
    let part = parse::parse_project_with_base(&blind, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("the stripped tool still parses");
    let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
    assert!(fouls.is_empty(), "an undeclared holder means today's behavior: {:?}", fouls);
}

/// Slice 1 moves no G-code either: the advisory is stderr, and the emitted
/// program is byte-identical to spline_funnel.cam's own golden — the
/// same funnel, the same ball, the holder changing nothing but what the
/// certificates can see. This equality is the fixture's golden.
#[test]
fn test_the_declared_holder_moves_no_gcode() {
    let src = fs::read_to_string("spline_funnel_holder.cam/main.part")
        .expect("read spline_funnel_holder.cam");
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    let g = gcode::program(&part).expect("the advisory does not reject the program");
    let golden = fs::read_to_string("tests/spline_funnel.golden.gcode")
        .expect("read spline_funnel golden");
    assert_eq!(g.trim(), golden.trim(), "the holder fields must not move a byte of output");
}

// ---- rim-area accounting: the swept-tube ceiling (DECISIONS.md → "rim-area
// accounting — the collar is a band but the tool sweeps a tube…", slice 0) ----

/// The ceiling's boundary, pinned at the model level — the march-cap lesson
/// applied here from day one: an end-to-end lint check is blind by resolution,
/// so the pin asks `material_at` about three points that straddle the claim.
/// The sphere's finish sweeps the shell between the surface (r 20) and the
/// surface plus a tool diameter (r 26); its top beside the rim is the r-26
/// dome, 16.49mm up at rho 20.1, where the flat collar used to stop at 3.
#[test]
fn test_the_swept_tube_ceiling_bounds_the_rim_claim_exactly() {
    // The straddle points ask the FINISH-ONLY program: on the shipped
    // two-op fixture the Clear has already swept everything above the
    // terraces, so no point beside the rim distinguishes the ceiling from
    // the stock top there. Alone, the finish's tube is the only claim.
    let src = fs::read_to_string("sphere_clear.cam/main.part").expect("read sphere_clear.cam");
    let alone = src.replace("run Clear(tool=T1, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n", "");
    let part = parse::parse(&alone).expect("parse");
    let rem = stock::remaining_after(&part, 0);
    assert!(rem.complete, "the finish models closed-form");

    // The exact point the shipped advisory reported (rho 20.46, z 3.09): 20.69mm
    // from the sphere's centre, inside the [20, 26] shell — removed by the
    // finish, and the collar's flat top used to deny it.
    assert!(!rem.material_at(70.457, 40.0, 3.094), "the old foul point is swept by the tube");
    // Just under the ceiling at rho 20.1 (dome top sqrt(26^2 - 20.1^2) = 16.492).
    assert!(!rem.material_at(70.1, 40.0, 16.3), "just under the ceiling is swept");
    // Just over it: unswept, standing.
    assert!(rem.material_at(70.1, 40.0, 16.7), "just over the ceiling still stands");
    // Mid-shell, where only the CEILING draws the line — at rho 23 the dome
    // ceiling sits at 12.124, well under the collar's flat z_top (16.61), so
    // this pair is the one a flat raise (the design's rejected fork 1C) would
    // get wrong: it would read the standing point as air, and check_moves
    // would bless a crash through it.
    assert!(!rem.material_at(73.0, 40.0, 11.9), "under the ceiling mid-shell is swept");
    assert!(rem.material_at(73.0, 40.0, 12.3), "over the ceiling mid-shell still stands");
}

/// The buried-cap sentence survives the fix — the bound the old collar comment
/// defended ("a collar that ran to the top would swallow the buried-cap case")
/// now lives in the ceiling: material ABOVE the `skin + dia` shell still
/// stands, so a Finish with no Clear in front of it still fouls on the stock
/// its body crosses beside the rim.
#[test]
fn test_a_finish_only_contained_dome_still_fouls() {
    let src = fs::read_to_string("sphere_clear.cam/main.part").expect("read sphere_clear.cam");
    let alone = src.replace("run Clear(tool=T1, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n", "");
    let part = parse::parse(&alone).expect("a finish-only program still parses");
    let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
    assert_eq!(fouls.len(), 1, "the uncleared stock beside the rim must still be named, got: {:?}", fouls);
}

/// Fork 1A's own pin: the ceiling asserts what the emitter DOES, not what the
/// geometry would allow. Every cutting move of the finish rides the offset
/// sphere (centre distance r + r_ball), so the ceiling — that path plus a ball
/// radius — must equal the emitted maximum plus r_ball, read from the emitted moves.
#[test]
fn test_the_ceiling_is_the_emitted_path_plus_a_ball_radius() {
    let src = fs::read_to_string("sphere_clear.cam/main.part").expect("read sphere_clear.cam");
    let part = parse::parse(&src).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("generates");
    let finish = emitted_moves.iter().find(|e| e.op_idx == 1).expect("the finish op's emitted_moves");

    let coord = |line: &str, axis: char| -> Option<f64> {
        line.split_whitespace()
            .find(|w| w.starts_with(axis))
            .and_then(|w| w[1..].parse().ok())
    };
    // Emitted XYZ is the ball's CENTRE (the apex plunge Z23 = apex 20 + r_ball,
    // the hand-checked anchor's convention). Rings ride the centre-path sphere
    // at exactly 23; ring-to-ring LINKS travel outside it, through air above
    // the dome — so the pin is one-sided where the path is (nothing ever
    // inside the centre-path sphere) and exact where the surface is touched.
    let (mut x, mut y, mut z) = (50.0_f64, 40.0_f64, 40.0_f64);
    let (mut min_d, mut rim_ring) = (f64::MAX, false);
    for line in &finish.moves {
        let cut = line.starts_with("G01") || line.starts_with("G02") || line.starts_with("G03");
        x = coord(line, 'X').unwrap_or(x);
        y = coord(line, 'Y').unwrap_or(y);
        z = coord(line, 'Z').unwrap_or(z);
        if cut && z <= 23.01 {
            let d = ((x - 50.0).powi(2) + (y - 40.0).powi(2) + z.powi(2)).sqrt();
            min_d = min_d.min(d);
            // The equator ring: surface contact at the rim itself, which is
            // what carries the tube all the way down to the base plane.
            rim_ring |= (d - 23.0).abs() < 5e-3 && z.abs() < 5e-3;
        }
    }
    // No centre ever inside the offset sphere (gouge-freedom, and the tube's
    // inner tangent), the rim ring exists (the tube runs the full meridian),
    // and the model's ceiling is that path plus a ball radius — 26, the
    // r + dia dome the removal claims. 3-decimal G-code sets the tolerance.
    assert!((min_d - 23.0).abs() < 5e-3, "the closest CL is the offset sphere itself, got {min_d:.4}");
    assert!(rim_ring, "the emitted path must ride down to the equator ring");
    assert!((26.0 - (min_d + 3.0)).abs() < 5e-3, "the ceiling is the emitted path plus a ball radius");
}

/// The slice-2 measurement, inverted into a pin: a contained-stock
/// Clear+Finish on a cap, a mixed band and a concave band comes out with NO
/// body-foul advisory — the tube accounts for the rim area the flat collar
/// truncated. These are the three scratch fixtures the design DRAFT measured
/// fouling (one signature, stepdown-insensitive on the vase); the 26 stocks
/// land against exactly this bar in slice 2.
#[test]
fn test_a_contained_convex_revolve_builds_silent() {
    // Header per fixture: (rapid_z, clearance_z) sit above each stock's top.
    let contained = |rapid: f64, clear: f64, stock: &str, feature: &str, name: &str| {
        format!(
            "units mm\nfeed 600\nplunge 200\nrapid_z {rapid}\nclearance_z {clear}\nspindle 4000\n\
             {stock}\ntool T1 Endmill(dia=12.0)\ntool T9 Ballmill(dia=6.0)\n{feature}\n\
             run Clear(tool=T1, profile={name}, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n\
             run Copy(tool=T9, surface={name}, stepover=3.0)\n"
        )
    };
    let fixtures = [
        ("cone", contained(25.0, 40.0,
            "stock Box(w=100, l=80, h=47, x=0, y=0, z=22)",
            "feature Point Cone(dia=40, height=20, x=50, y=40, z=0)", "Point")),
    ];
    for (name, src) in &fixtures {
        let part = parse::parse(src).unwrap_or_else(|e| panic!("{name}: parse: {e}"));
        assert!(gcode::program(&part).is_ok(), "{name}: the contained program builds");
        let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
        assert!(fouls.is_empty(), "{name}: the tube must silence the rim foul, got: {:?}", fouls);
    }
}

// ---- the emitter rim tails ---------------------------------------------------
//
// The run-out past the offset meridian's natural end (DECISIONS.md → "the
// emitter rim tails…"). Slice 0 builds the home and makes it the collar's gate;
// the tube reads it in slice 1.

/// The finish op's extreme CL, read off the emitted moves: `(rho, z)` at the
/// largest rho any cutting move reaches. Ball-nose emitters write the ball
/// CENTRE (the convention `test_the_tube_ends_where_the_emitter_does` wrote
/// down), so this is directly comparable to the tail home's own samples.
///
/// The z is the modally-active one at that move, which is a ring's own z for
/// every fixture here — but the rho is what the collar and the tube are sized
/// from, so the pins below lead with it.
fn emitted_extreme_cl(fixture: &str, cx: f64, cy: f64) -> (f64, f64) {
    let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
    let part = parse::parse(&src).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("generates");
    let finish = emitted_moves.last().expect("the finish op");
    let coord = |line: &str, axis: char| -> Option<f64> {
        line.split_whitespace().find(|w| w.starts_with(axis)).and_then(|w| w[1..].parse().ok())
    };
    let (mut x, mut y, mut z) = (cx, cy, 99.0_f64);
    let mut seat = (0.0_f64, 0.0_f64);
    for line in &finish.moves {
        let cut = line.starts_with("G01") || line.starts_with("G02") || line.starts_with("G03");
        x = coord(line, 'X').unwrap_or(x);
        y = coord(line, 'Y').unwrap_or(y);
        z = coord(line, 'Z').unwrap_or(z);
        if cut && z <= 30.0 {
            let rho = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            if rho > seat.0 {
                seat = (rho, z);
            }
        }
    }
    seat
}

/// The op's own CL stand-off, which is what the tail home is parameterised by:
/// the emitted skin plus the ball's radius — the same number `removals_for`
/// hands `derive_revolve_offset_meridian` for the tube's centre path.
fn finish_stand_off(fixture: &str) -> (parse::Part, f64) {
    let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
    let part = parse::parse(&src).expect("parse");
    let op = part.operations.last().expect("the finish op").clone();
    let skin = parse::derive_emitted_skin(&part, &op).expect("these carriers all model their skin");
    let r_ball = part.tools.get(&op.tool_num).expect("tool").dia / 2.0;
    (part, skin + r_ball)
}

/// The home lands exactly where the emitter lands, on every carrier that has a
/// tail and in both spacing modes. This is the pin that stops the two
/// constructions drifting — slice 1 of the rim-area thread rejected to restate
/// nine carriers' end-geometry for exactly this reason, and a tail restated in
/// `stock.rs` would have been the same mistake one carrier at a time.
///
/// The closed form is stated here rather than recomputed, because a pin that
/// derives its expectation the way the code does cannot catch the code being
/// wrong. Since fork 2B there is only ONE form to state: every roll about the
/// rim corner finishes horizontal, so every tail ends at `a + offset` in the
/// base plane, whatever the rim normal was at the seat. The cone used to need
/// its own — `offset·(cosα + sinα·tanα)` outboard of `a` — and that it no
/// longer does is the whole point of the fork.
///
/// Which makes this pin weaker than it was, in one specific way: `a + offset`
/// is now shared, so a cone that wrongly took the OGIVE's arm would still pass
/// here. The stand-off assertion and the emitter cross-check below are what
/// keep it honest, and `cone_rough` (a + 3.300 vs the ogive's a + 3.363) is
/// the case where a swapped arm would show.
#[test]
fn test_the_tail_home_lands_where_the_emitter_lands() {
    // (fixture, stand-off, expected tail end (rho, z))
    let cases: &[(&str, f64, (f64, f64))] = &[
        ("cone.cam", 3.000, (23.000, 0.0)),
        ("cone_rough.cam", 3.300, (23.300, 0.0)),
        ("cone_scallop.cam", 3.000, (23.000, 0.0)),
    ];
    for (fixture, stand_off_want, (rho_want, z_want)) in cases {
        let (part, stand_off) = finish_stand_off(fixture);
        assert!(
            (stand_off - stand_off_want).abs() < 1e-3,
            "{fixture}: stand-off moved — expected {stand_off_want}, got {stand_off:.3}"
        );
        let feature = part.features.values().next().expect("one feature");
        let tail = parse::derive_revolve_emitter_tail(feature, stand_off);
        assert!(!tail.is_empty(), "{fixture}: this carrier has a tail");
        let end = *tail.last().expect("non-empty");
        assert!(
            (end.0 - rho_want).abs() < 2e-3 && (end.1 - z_want).abs() < 1e-9,
            "{fixture}: tail ends ({:.3}, {:.3}), closed form says ({rho_want}, {z_want})",
            end.0,
            end.1
        );
        // ...and that end is the emitter's own, not merely a plausible number.
        let (rho_emitted, _) = emitted_extreme_cl(fixture, 50.0, 40.0);
        assert!(
            (end.0 - rho_emitted).abs() < 2e-3,
            "{fixture}: the home stops at rho {:.3} but the emitter reaches {rho_emitted:.3} — \
             the tail must move with the emitter",
            end.0
        );
    }
}

/// Distance from a meridian point to the NOMINAL cone: the flank as a SEGMENT
/// with a corner fan at each end. Written out here rather than called out of
/// `parse`, because a pin that borrows the code's own notion of "the surface"
/// cannot catch the code measuring against the wrong one — which is exactly
/// how the flank-wall convention kept an air-cutting ring looking like a
/// legitimate offset for as long as it did. Under that convention the ring at
/// (24.243, 0) measured 3.000 from "the surface"; against the real cone it is
/// 4.243 from the nearest point of it.
fn cone_nominal_distance(rho: f64, z: f64, a: f64, h: f64) -> f64 {
    let l = (a * a + h * h).sqrt();
    let corner = (rho * rho + (z - h) * (z - h)).sqrt().min(((rho - a).powi(2) + z * z).sqrt());
    let t = rho * (a / l) + (z - h) * (-h / l);
    if (0.0..=l).contains(&t) {
        return corner.min(rho * (h / l) + (z - h) * (a / l));
    }
    corner
}

/// Each emitted finish ring as `(rho, z)`: group the cutting points by level
/// and take the SMALLEST radius at each. Two readings that look simpler are
/// both wrong here, and each was tried:
///
/// - "widest point at each Z" reads the radial lead INTO the next ring, which
///   is emitted at the previous ring's Z — every ring's radius attributed to
///   the one above it.
/// - "first point after each plunge" reads that same lead whenever a level has
///   no circle of its own, which is exactly what the degenerate pole ring at
///   `z_apex + r_cap` is. That one reports the pole as `(4.243, 23.000)`.
///
/// The lead always runs outward (the stack widens on the way down), so the
/// minimum at a level is the ring; levels carrying fewer than three points
/// have no circle at all and are dropped.
fn emitted_finish_rings(fixture: &str, cx: f64, cy: f64) -> Vec<(f64, f64)> {
    let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
    let part = parse::parse(&src).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("generates");
    let finish = emitted_moves.last().expect("the finish op");
    let coord = |line: &str, axis: char| -> Option<f64> {
        line.split_whitespace().find(|w| w.starts_with(axis)).and_then(|w| w[1..].parse().ok())
    };
    let (mut x, mut y, mut z) = (cx, cy, 99.0_f64);
    let mut levels: Vec<(f64, f64, usize)> = Vec::new();
    for line in &finish.moves {
        if !line.starts_with("G01") {
            continue;
        }
        let moved_xy = coord(line, 'X').is_some() || coord(line, 'Y').is_some();
        x = coord(line, 'X').unwrap_or(x);
        y = coord(line, 'Y').unwrap_or(y);
        z = coord(line, 'Z').unwrap_or(z);
        if !moved_xy {
            continue;
        }
        let rho = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        match levels.iter_mut().find(|(lz, _, _)| (*lz - z).abs() < 1e-9) {
            Some((_, min_rho, n)) => {
                *min_rho = min_rho.min(rho);
                *n += 1;
            }
            None => levels.push((z, rho, 1)),
        }
    }
    levels.iter().filter(|(_, _, n)| *n >= 3).map(|(z, rho, _)| (*rho, *z)).collect()
}

/// Fork 2B's payoff, stated as the thing that was wrong: **no cone finish ring
/// cuts air.** Every emitted CL stands exactly the op's stand-off from the
/// nominal cone — apex sphere, flank, or rim corner, whichever is nearest —
/// so every ring's ball is in contact with something that is really there.
///
/// Before this slice the bottom ring sat 4.243 from the nearest surface point
/// with a 3.000 stand-off, and `cone_rough`'s next ring up sat 3.816. Both
/// measured correct against the flank-wall convention, which ran the flank on
/// past the rim as a surface to be an offset of. Deleting that wall is what
/// makes this pin able to fail.
///
/// The band is asserted to be non-empty first. A contact pin that runs over
/// rings which do not exist is green and blind — and the rim band is exactly
/// where the ladder's `.max(cz)` clamp puts rings that no stepdown asked for.
#[test]
fn test_no_cone_finish_ring_cuts_air() {
    // (fixture, stand-off, rings expected in the rim corner band)
    let cases: &[(&str, f64, usize)] =
        &[("cone.cam", 3.000, 2), ("cone_rough.cam", 3.300, 2), ("cone_scallop.cam", 3.000, 2)];
    let (a, h) = (20.0_f64, 20.0_f64);
    for (fixture, stand_off_want, band_want) in cases {
        let (_, stand_off) = finish_stand_off(fixture);
        assert!(
            (stand_off - stand_off_want).abs() < 1e-3,
            "{fixture}: stand-off moved — expected {stand_off_want}, got {stand_off:.3}"
        );
        let rings = emitted_finish_rings(fixture, 50.0, 40.0);
        assert!(rings.len() > 3, "{fixture}: only {} rings read — the reader is broken", rings.len());

        // The rim band is everything below the flank/roll seam at r_cap·sinα.
        let z_rim = stand_off * (a / (a * a + h * h).sqrt());
        let band = rings.iter().filter(|(_, z)| *z < z_rim).count();
        assert_eq!(band, *band_want, "{fixture}: {band} rings in the rim band, expected {band_want}");

        for (rho, z) in &rings {
            let d = cone_nominal_distance(*rho, *z, a, h);
            assert!(
                (d - stand_off).abs() < 2e-3,
                "{fixture}: ring CL ({rho:.3}, {z:.3}) stands {d:.3} off the cone, but the ball \
                 is {stand_off:.3} — it is cutting air by {:.3}",
                d - stand_off
            );
        }
    }
}

/// The bottom ring specifically, as a closed form and on the corner it rolls.
/// A roll finishes horizontal, so the last CL is `(a + stand_off, z0)` and its
/// ball touches the rim corner exactly — the one contact the old locus missed
/// while claiming to be an offset of the surface it had run off the end of.
#[test]
fn test_the_cone_bottom_ring_touches_the_rim_corner() {
    for (fixture, stand_off) in [("cone.cam", 3.000), ("cone_rough.cam", 3.300), ("cone_scallop.cam", 3.000)] {
        let (rho, z) = emitted_extreme_cl(fixture, 50.0, 40.0);
        assert!(
            (rho - (20.0 + stand_off)).abs() < 2e-3 && z.abs() < 2e-3,
            "{fixture}: bottom ring at ({rho:.3}, {z:.3}), the roll's closed form says ({:.3}, 0)",
            20.0 + stand_off
        );
        let to_corner = ((rho - 20.0).powi(2) + z * z).sqrt();
        assert!(
            (to_corner - stand_off).abs() < 2e-3,
            "{fixture}: bottom ball sits {to_corner:.3} from the rim corner, not {stand_off:.3} — \
             it is not in contact"
        );
    }
}

/// Emptiness is the honest answer for most of the family, and it is also the
/// gate — so what reports a tail is pinned as a closed list, the way the
/// containment remainder is. The TANGENT ogive is the interesting member: it
/// sits in the same enum variant as the secant form and has no tail at all.
#[test]
fn test_only_the_two_run_out_carriers_report_a_tail() {
    let with_tail = ["cone.cam", "cone_rough.cam", "cone_scallop.cam"];
    let mut found: Vec<String> = Vec::new();
    for entry in fs::read_dir(".").expect("read repo root") {
        let path = entry.expect("dir entry").path();
        if !path.extension().map(|e| e == "cam").unwrap_or(false) {
            continue;
        }
        let src = fs::read_to_string(path.join("main.part")).expect("read fixture");
        let geom = fs::read_to_string(path.join("geometry.part")).ok();
        let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path))
            .unwrap_or_else(|e| panic!("{}: parse failed: {}", path.display(), e));
        // Ask at a generous stand-off, so a fixture is on this list because of
        // its GEOMETRY and not because its own tool happens to be small.
        let any = part
            .features
            .values()
            .any(|f| !parse::derive_revolve_emitter_tail(f, 3.0).is_empty());
        if any {
            found.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    found.sort();
    assert_eq!(found, with_tail, "the set of carriers with a rim tail moved");
}

/// Slice 1's payoff, on the whole affected corpus and in BOTH directions. The
/// tube now runs to the end of the emitter, so the flat collar is gone from
/// this arm entirely — and a retired collar has to be checked twice: that the
/// over-claim it carried is gone (the point stands), and that nothing it was
/// legitimately covering went with it (the point beside the tail's last ball is
/// still swept). A model that simply claims less would pass the first half.
///
/// Below the base plane again, for the reason slice 0 wrote down: above it the
/// contained stock's `Clear` removes the moat and every probe reads air whatever
/// the model does.
#[test]
fn test_the_tube_retires_the_last_two_collars() {
    // (fixture, a point the collar over-claimed, a point the tail really sweeps)
    let cases: &[(&str, (f64, f64), (f64, f64))] = &[
        ("cone.cam", (20.5, -2.5), (23.000, -2.9)),
        ("cone_rough.cam", (20.5, -2.5), (23.300, -2.9)),
        ("cone_scallop.cam", (20.5, -2.5), (23.000, -2.9)),
    ];
    for (fixture, over, swept) in cases {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
        let part = parse::parse(&src).expect("parse");
        let rem = stock::remaining_after(&part, part.operations.len() - 1);
        assert!(rem.complete, "{fixture}: the model accounts for every op");
        assert!(
            rem.material_at(50.0 + over.0, 40.0, over.1),
            "{fixture}: ({}, {}) is stock no ball reaches — the collar's over-claim must be gone",
            over.0,
            over.1
        );
        assert!(
            !rem.material_at(50.0 + swept.0, 40.0, swept.1),
            "{fixture}: ({}, {}) sits under the tail's last ball — retiring the collar must not \
             have taken this claim with it",
            swept.0,
            swept.1
        );
    }
}

/// The tube's outer edge is the tail's last ball, sharply. Pinned either side of
/// it on the cone, whose run-out reaches furthest: the last CL is (23.000, 0),
/// so at z −0.1 the ball reaches 23 + √(3² − 0.1²) = 25.998, and the model must
/// not be soft about which side of that a point falls on.
///
/// The boundary moved inboard 1.243mm with fork 2B, which is the honest number:
/// the tube used to reach 27.243 because the emitter used to go there, and it
/// went there without touching anything.
#[test]
fn test_the_tube_ends_at_the_tails_last_ball() {
    let src = fs::read_to_string("cone.cam/main.part").expect("read cone.cam");
    let part = parse::parse(&src).expect("parse");
    let rem = stock::remaining_after(&part, part.operations.len() - 1);
    assert!(!rem.material_at(50.0 + 25.95, 40.0, -0.1), "just inside the last ball: swept");
    assert!(rem.material_at(50.0 + 26.05, 40.0, -0.1), "just beyond the last ball: stands");
}

/// At zero stand-off the CL locus IS the surface, which ends at the rim by
/// definition — so the tail vanishes. That matters beyond tidiness: two
/// consumers ask `derive_revolve_rim_radius(feature, 0.0)` for the NOMINAL rim
/// (`ops::clear`, and the clear's own removal), and a tail that reported
/// anything at zero would move the moat radius on every cone in the corpus.
#[test]
fn test_the_tail_vanishes_at_zero_stand_off() {
    for fixture in ["cone.cam"] {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
        let part = parse::parse(&src).expect("parse");
        let feature = part.features.values().next().expect("one feature");
        assert!(
            parse::derive_revolve_emitter_tail(feature, 0.0).is_empty(),
            "{fixture}: a zero-offset tail would move the nominal rim"
        );
    }
}

/// The form-tool pair's `Clear` line, verbatim (Phase 10 lane B, slice 0: the
/// pair ships contained and paired). Pins whose subject is the finish ALONE
/// strip it from the fixture's text, and say so where they do.
const FORM_TOOL_CLEAR_LINE: &str =
    "run Clear(tool=T2, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n";

/// The pair's stock before lane B contained it, as a substitution on the
/// shipped fixture's text: the same program with its dome standing 10mm proud
/// of the stock top. The twin every "uncontained" pin is built from.
fn lower_the_form_tool_blank(src: &str) -> String {
    let lowered = src.replace(
        "stock Box(w=100, l=80, h=35, x=0, y=0, z=10)",
        "stock Box(w=100, l=80, h=25, x=0, y=0, z=0)",
    );
    assert_ne!(lowered, src, "the substitution must actually substitute");
    lowered
}

fn strip_the_form_tool_clear(src: &str) -> String {
    let stripped = src.replace(FORM_TOOL_CLEAR_LINE, "");
    assert_ne!(stripped, src, "the fixture must still pair the Clear this strips");
    stripped
}

#[test]
fn test_body_foul_lint_fires_when_a_flaring_tool_is_sunk_in_the_blank() {
    // The shipped fixture roughs a moat around its dome, so the flaring
    // tool's shoulder swings through cleared air. Strip the Clear and drop
    // the equator 10mm: the same tool, on the same path, drags its 45-degree
    // flank through solid stock that no op removes — the tip still clears
    // the dome perfectly.
    let src = fs::read_to_string("sphere_formtool.cam/main.part").expect("read sphere_formtool.cam");
    let sunk = strip_the_form_tool_clear(&src).replace("Sphere(dia=20, x=30, y=20, z=0)", "Sphere(dia=20, x=30, y=20, z=-10)");
    let part = parse::parse_project_with_base(&sunk, None, Some(std::path::Path::new("sphere_formtool.cam")))
        .expect("an embedded dome is legal, just loud");
    let warnings = parse::lint(&part);
    assert!(
        warnings.iter().any(|w| w.message.contains("outlives") && w.message.contains("TopDome") && w.message.contains("T1")),
        "the body foul should name the tool and the feature, got: {:?}", warnings
    );
}

// ---- the standing surfaces of revolution -----------------------------------
// Every cap fixture authors its base plane AT the stock top, so the declared
// solid stands entirely clear of the stock and removes nothing — correct, and
// blind. Each case below sinks the feature into the stock first (the same move
// the sunk-dome foul test makes), then straddles the solid using only the
// feature's own AUTHORED circles: no meridian formula is restated here, so a
// wrong datum or a flipped frame in the model cannot agree with the test by
// construction.
fn sunk(fixture: &str, from: &str, to: &str) -> nacs::stock::RemainingStock {
    let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
    let sunk = src.replace(from, to);
    assert!(sunk != src, "{fixture}: the sink replacement did not apply");
    let part = parse::parse_project_with_base(&sunk, None, Some(std::path::Path::new(fixture)))
        .expect("a sunk feature is legal");
    let n = part.operations.len() - 1;
    nacs::stock::remaining_after(&part, n)
}

#[test]
fn test_stock_model_covers_the_standing_revolved_caps() {
    // The probes pin ORIENTATION, not just presence: a cap is wide near its
    // base and narrow near its top, so a point out near the base radius but
    // halfway up must be clear of the solid, and a point near the axis but well
    // down from the top must be inside it. A flipped or mis-datumed meridian
    // fails one or both — checked by mutation, not assumed.
    //
    // (fixture, authored z -> sunk z, wide radius, z halfway up, narrow radius,
    //  z below the top)
    let cases: [(&str, &str, &str, f64, f64, f64, f64); 2] = [
        // Cone: base circle r=20 at the base plane, apex on the axis 20 up.
        ("cone.cam", "Cone(dia=40, height=20, x=50, y=40, z=0)", "Cone(dia=40, height=20, x=50, y=40, z=-20)", 18.0, -10.0, 2.0, -7.5),
        // Spline cap: meridian.spline runs apex (0,20) to base rim (13,0).
        ("spline_cap.cam", "x=50, y=40, z=0)", "x=50, y=40, z=-20)", 11.7, -10.0, 1.3, -6.0),
    ];
    for (fixture, from, to, rho_wide, z_mid, rho_narrow, z_high) in cases {
        let r = sunk(fixture, from, to);
        assert!(r.complete, "{fixture}: the cap must have a closed-form removal");
        assert!(!r.material_at(50.0 + rho_wide, 40.0, z_mid), "{fixture}: out near the base radius, halfway up, is clear of the solid");
        assert!(r.material_at(50.0 + rho_narrow, 40.0, z_high), "{fixture}: near the axis, below the top, is inside the solid");
        assert!(r.material_at(90.0, 40.0, z_mid), "{fixture}: the stock outside the footprint stands");
    }
}

#[test]
fn test_stock_model_leaves_the_torus_hole_alone() {
    // The torus is the band case at its most obvious: an annulus from the inner
    // equator (r=21) to the outer (r=29), with the crown at r=25, and a hole
    // down the middle no op declares as a surface.
    //
    // The fixture is CONTAINED now, so the last op's remaining stock also
    // carries the Clear's moat and hole. That moves the outer probe — the stock
    // beyond the moat is a pedestal, at r 43 rather than r 40 — and leaves the
    // middle one exactly where it was, because what stands there is now the
    // PILLAR: the Clear opened the hole around it and left the column, which is
    // the same answer for the same reason (nothing declares it should go).
    let r = sunk("torus.cam", "Torus(major_dia=50, minor_dia=8, x=50, y=40, z=0)", "Torus(major_dia=50, minor_dia=8, x=50, y=40, z=-4)");
    assert!(r.complete, "the torus must have a closed-form removal");
    assert!(r.material_at(77.0, 40.0, -3.9), "just above the equator plane is inside the tube");
    assert!(!r.material_at(77.0, 40.0, -0.1), "near the crown height, off the crown radius, is clear of the tube");
    assert!(r.material_at(50.0, 40.0, -0.1), "the pillar down the middle is undeclared stock");
    assert!(r.material_at(93.0, 40.0, -0.1), "outside the moat the stock stands as a pedestal");
}

// ---- the probes: where the geometry forces the tool ------------------------

#[test]
fn test_body_foul_lint_catches_a_cap_still_buried_in_the_blank() {
    // The reachable case for a plain ball tool, and the one worth having: a cap
    // sunk into the stock with nothing declared to remove the material around
    // it. The tip rides the surface perfectly; the shank is in solid stock the
    // whole way down. Each fixture authors its base plane AT the stock top —
    // standing clear, so quiet — until it is sunk.
    let cases: [(&str, &str, &str, &str); 2] = [
        ("cone.cam", "Cone(dia=40, height=20, x=50, y=40, z=0)", "Cone(dia=40, height=20, x=50, y=40, z=-20)", "Point"),
        ("torus.cam", "Torus(major_dia=50, minor_dia=8, x=50, y=40, z=0)", "Torus(major_dia=50, minor_dia=8, x=50, y=40, z=-4)", "T"),
    ];
    for (fixture, from, to, feature) in cases {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
        // The premise is "nothing declared to remove the material around it",
        // so the contained fixtures' Clear (and the truncated family's Floor)
        // comes out — with it in, the moat opens and the cap is honestly
        // reachable, which is the op working, not the lint failing.
        let sunk = src
            .lines()
            .filter(|l| !l.starts_with("run Clear") && !l.starts_with("run End"))
            .collect::<Vec<_>>()
            .join("\n")
            .replace(from, to);
        let part = parse::parse_project_with_base(&sunk, None, Some(std::path::Path::new(fixture)))
            .expect("a sunk cap is legal, just loud");
        let warnings = parse::lint(&part);
        assert!(
            warnings.iter().any(|w| w.message.contains("outlives") && w.message.contains(feature)),
            "{fixture}: a buried cap must be reported, got: {:?}", warnings
        );
    }
}

#[test]
fn test_body_foul_lint_reads_the_op_history_not_just_the_feature() {
    // The same sunk cone, with an End op ahead of it that decks the stock down
    // to the cone's own CROWN: the stock the shank would have plowed is
    // gone by the time the Finish runs, so the warning must disappear. This is
    // the whole point of an op-HISTORY model — a per-feature check could not
    // tell these two programs apart.
    //
    // (Until Phase 26 slice 1 the deck was the cone's BASE plane, z=-20 — a
    // whole-stock raster 20mm below the crown, i.e. a program that faces the
    // cone away. Nothing said so; `validate` now refuses it by name, pinned
    // below, and the legal deck meets the crown.)
    let src = fs::read_to_string("cone.cam/main.part").expect("read cone.cam");
    let sunk = src
        .replace("Cone(dia=40, height=20, x=50, y=40, z=0)", "Cone(dia=40, height=20, x=50, y=40, z=-20)")
        .replace("tool T9 Ballmill(dia=6.0)", "tool T9 Ballmill(dia=6.0)\ntool T2 Endmill(dia=12.0)")
        .replace("feature Point", "feature Deck Plane(z=0)\nfeature Point")
        .replace("run Finish", "run End(tool=T2, surface=Deck, stepover=8.0)\nrun Finish");
    let swept = parse::parse(&sunk.replace("Plane(z=0)", "Plane(z=-20)")).expect_err("a face below the crown would sweep the cone");
    assert!(swept.contains("'Point'") && swept.contains("'Deck'"), "both features named: {swept}");
    let part = parse::parse(&sunk).expect("facing then finishing is legal");
    let fouls: Vec<_> = parse::lint(&part).into_iter().filter(|w| w.message.contains("outlives")).collect();
    assert!(fouls.is_empty(), "the facing op removed the stock the shank would foul, got: {:?}", fouls);
}

// ---- the entry restriction --------------------------------------------------
// (DECISIONS.md "The entry restriction refuses the program…") The stock model
// finally changes an outcome rather than a message: a program whose entry moves
// drive the tool into material no op removes does not come out at all.

#[test]
fn test_entry_into_unremoved_stock_is_refused() {
    // A pocket whose mouth is authored 3mm below the stock top has 3mm of stock
    // over it. Nothing declares that away, so the plunge drills its way in
    // through solid material — rejected, with the move quoted.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let sunk = src.replace("ProfilePocket(extent=5.0", "ProfilePocket(z=-3.0, extent=5.0");
    let part = parse::parse(&sunk).expect("an authored datum parses");
    let err = gcode::program(&part).expect_err("a plunge through unremoved stock must be rejected");
    assert!(err.contains("outlives this operation"), "error should say what it hit, got: {}", err);
    assert!(err.contains("G01 Z-8.000"), "error should quote the offending move, got: {}", err);
    assert!(err.contains("Remove that material in an earlier op"), "error should offer the fix, got: {}", err);
}

#[test]
fn test_the_same_program_generates_once_the_material_is_declared_away() {
    // The op-history half, at the level that matters most: deck the stock down
    // to the pocket's own mouth and the identical Clear op is fine. Refusing is
    // a statement about the PROGRAM, not about the feature.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let decked = src
        .replace("ProfilePocket(extent=5.0", "ProfilePocket(z=-3.0, extent=5.0")
        .replace("feature PocketProfile", "feature Deck Plane(z=-3.0)\nfeature PocketProfile")
        .replace("run Clear", "run End(tool=T5, surface=Deck, stepover=6.0)\nrun Clear");
    let part = parse::parse(&decked).expect("facing then clearing parses");
    let g = gcode::program(&part).expect("the same op is fine once the stock above it is gone");
    assert!(g.contains("Z-8.000"), "the pocket still cuts to its authored floor");
}

#[test]
fn test_a_rapid_plane_inside_the_blank_is_refused() {
    // The other half of "entry": a G00 that traverses below the top of the
    // stock is a crash, not a cut. Nothing checked this before the stock model
    // existed — it is what the sidecar round-trip wrappers were doing.
    // A rapid plane BELOW the feature's own floor: the tool rapids past the
    // bottom of the pocket it is about to cut, into stock the pocket never
    // reaches. This is what the sidecar round-trip wrappers were doing (a
    // rapid_z of 5 under a stock topped at 15) and nothing caught it.
    let src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read profile_pocket_clear.cam");
    let buried = src.replace("rapid_z 5", "rapid_z -8");
    // Phase 50 slice 1: the header law refuses this at PARSE now, naming the
    // top. The entry restriction could never see a rapid into the op's OWN
    // column (that material does not outlive the op), so a rapid plane in
    // the blank is the header's law; the restriction's own sentence stays
    // pinned by the tests around this one.
    let err = parse::parse(&buried).expect_err("a rapid plane inside the blank is refused at parse");
    assert!(err.contains("rapid_z -8.000mm is at or below the stock top (0.000mm)"), "got: {err}");
}

// ---- the entry restriction, control comp (DECISIONS.md "the control-comp
// centerline resolve", slice 2): the skip retires ------------------------------
// Before the resolve, check_moves skipped comp=control ops wholesale — reading
// part-edge text as tool positions would have accused every one of straddling
// its own wall. The resolved channel is the motion the machine actually makes,
// so a control op now answers for its entries like any other op.

#[test]
fn test_a_control_entry_into_unremoved_stock_is_refused() {
    // The sunken-mouth scenario, both comp modes: a pocket authored 3mm below
    // the stock top has 3mm of stock over it that nothing declares away, so
    // the plunge drills in through solid material. Engine comp always rejected
    // this; control comp sailed through on the skip and the program came out.
    // Same geometry, same check result, either comp mode — that is the seat.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let sunk = src.replace("ProfilePocket(extent=2.0", "ProfilePocket(z=-3.0, extent=2.0");

    let engine = parse::parse(&sunk).expect("an authored datum parses");
    let err = gcode::program(&engine).expect_err("engine comp: a plunge through unremoved stock is rejected");
    assert!(err.contains("outlives this operation"), "engine error should say what it hit, got: {}", err);

    let ctl = sunk.replace(
        "run Side(tool=T5, surface=Window)",
        "run Side(tool=T5, surface=Window, comp=control)",
    );
    let part = parse::parse(&ctl).expect("a comp switch parses");
    let err = gcode::program(&part).expect_err("control comp: the same plunge must now be rejected, not skipped");
    assert!(err.contains("outlives this operation"), "control error should say what it hit, got: {}", err);
    assert!(err.contains("Side on 'Window'"), "control error should name the op, got: {}", err);
    assert!(err.contains("G01 Z-5.000"), "control error should quote the resolved plunge, got: {}", err);
}

#[test]
fn test_the_control_program_generates_once_the_material_is_declared_away() {
    // The op-history half, control edition: deck the stock down to the sunken
    // mouth and the identical control op is fine. Refusing is a statement
    // about the PROGRAM, not about the comp mode.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let decked = src
        .replace("ProfilePocket(extent=2.0", "ProfilePocket(z=-3.0, extent=2.0")
        .replace("feature Window", "feature Deck Plane(z=-3.0)\nfeature Window")
        .replace(
            "run Side(tool=T5, surface=Window)",
            "run End(tool=T5, surface=Deck, stepover=6.0)\nrun Side(tool=T5, surface=Window, comp=control)",
        );
    let part = parse::parse(&decked).expect("facing then control finishing parses");
    let g = gcode::program(&part).expect("the same control op is fine once the stock above it is gone");
    assert!(g.contains("G41"), "the control op still emits controller comp");
    assert!(g.contains("Z-5.000"), "the pocket still cuts to its authored floor");
}

// ---- the plunge taxonomy: the category-3 entry flag (DECISIONS.md "the
// plunge taxonomy") ------------------------------------------------------------
// Rejected entries (material nothing removes) are check_moves, above. The flag
// is the middle category: a Finish entering material that stands only until
// its own declaration removes it — allowed as a manual solution, voiced as an
// advisory, rejected only under the opt-in `entry gate`.

fn entry_findings_of(src: &str) -> Vec<String> {
    let part = parse::parse(src).expect("fixture parses");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("fixture generates");
    nacs::stock::entry_findings(&part, &emitted_moves).into_iter().map(|f| f.message).collect()
}

#[test]
fn test_the_entry_flag_fires_on_an_uncleared_pocket_finish() {
    // The machinist's law, encoded: wall-finishing a pocket nobody cleared is
    // a plunge cut through the pocket's own standing material. Both comp
    // modes read the same tool-center motion, so both fire the same flag.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let findings = entry_findings_of(&src);
    assert_eq!(findings.len(), 1, "the uncleared window finish flags once, got: {:?}", findings);
    assert!(
        findings[0].contains("no earlier op cleared") && findings[0].contains("'Window'"),
        "the finding names the category and the feature, got: {}",
        findings[0]
    );

    let ctl = src.replace(
        "run Side(tool=T5, surface=Window)",
        "run Side(tool=T5, surface=Window, comp=control)",
    );
    let ctl_findings = entry_findings_of(&ctl);
    assert_eq!(ctl_findings.len(), 1, "control comp flags the same entry, got: {:?}", ctl_findings);
}

/// The structured half of a finding (DECISIONS.md "the agent surface…", fork
/// 4A): family and subject are DATA, not substrings of the prose, and
/// `Display` is that prose verbatim — one fact, two renderings, one home.
/// Every other test in this file reads `.message` or the rendered string, so
/// this is the one place the fields themselves are pinned: an op-shaped
/// finding carries its run index and feature, and the containment lint — which
/// never consults the run list — carries the feature alone.
#[test]
fn test_findings_carry_their_family_and_subject() {
    use parse::{FindingFamily, FindingSubject};

    // An entry finding names the op it judges: profile_pocket.cam's one
    // Finish (run index 0) on 'Window'.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let part = parse::parse(&src).expect("fixture parses");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("fixture generates");
    let findings = nacs::stock::entry_findings(&part, &emitted_moves);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].family, FindingFamily::Entry);
    assert_eq!(
        findings[0].subject,
        FindingSubject::Op { index: 0, feature: "Window".into() },
        "the subject is the op the prose names"
    );
    assert_eq!(findings[0].to_string(), findings[0].message, "Display IS the prose");

    // A containment finding is about the declaration alone — a feature
    // subject, no op index to name. The shipped pair is contained now, so
    // the subject is its lowered, unpaired twin (a Clear over a dome
    // standing in air rejects at parse: nothing to rough).
    let src = fs::read_to_string("sphere_formtool.cam/main.part").expect("read sphere_formtool.cam");
    let src = lower_the_form_tool_blank(&strip_the_form_tool_clear(&src));
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("sphere_formtool.cam")))
        .expect("parse");
    let containment: Vec<_> = parse::lint(&part)
        .into_iter()
        .filter(|f| f.family == parse::FindingFamily::Containment)
        .collect();
    assert_eq!(containment.len(), 1, "{containment:?}");
    assert_eq!(
        containment[0].subject,
        FindingSubject::Feature { name: "TopDome".into() },
        "containment reads the declaration alone, so its subject is the feature"
    );
}

#[test]
fn test_the_entry_flag_is_quiet_after_clearing() {
    // The licensed case: the same finish behind a Clear enters cleared air.
    // The Clear's own first plunge is category 3 by construction and is the
    // normal start of all work — licensed by charter, so no finding from it
    // either.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let cleared = src.replace(
        "run Side(tool=T5, surface=Window)",
        "run Clear(tool=T5, profile=Window, stepover=4.0)\nrun Side(tool=T5, surface=Window)",
    );
    let findings = entry_findings_of(&cleared);
    assert!(findings.is_empty(), "a cleared pocket's finish is licensed, got: {:?}", findings);
}

#[test]
fn test_the_entry_flag_declines_on_an_unmodeled_removal() {
    // A boss finish's cleared surround is a path fact — removals_for's None
    // arm — so the flag declines exactly as check_moves does, rather than
    // firing a finding whose "own declaration accounts for it" claim would be
    // false. Total accounting: no flag is better than a wrong one.
    let src = fs::read_to_string("profile_boss.cam/main.part").expect("read profile_boss.cam");
    let findings = entry_findings_of(&src);
    assert!(findings.is_empty(), "an unmodeled surround declines silently, got: {:?}", findings);
}

#[test]
fn test_the_entry_flag_is_quiet_on_plate() {
    // The honestly-sequenced demo: Floor decks the top, and the contour's
    // entry lands exactly on the floor it decked — licensed, the good plunge.
    let main_src = fs::read_to_string("plate.cam/main.part").expect("read main.part");
    let geom_src = fs::read_to_string("plate.cam/geometry.part").expect("read geometry.part");
    let part = parse::parse_project(&main_src, Some(&geom_src)).expect("parse plate.cam");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("plate generates");
    let findings = nacs::stock::entry_findings(&part, &emitted_moves);
    assert!(findings.is_empty(), "plate.cam's sequence is licensed end to end, got: {:?}", findings);
}

/// Fork 3A (blessed by name): the derived pairing licenses a Finish's
/// category-3 entries — behind its own rough, an entry can only cut what the
/// op declares (standing before + complete after + check_moves passed), and
/// naming that on every contained fixture would name nothing. The voice
/// survives exactly where the pairing is absent: the same program minus its
/// Clear meets the same material and flags.
#[test]
fn test_the_entry_license_is_the_pairing() {
    let src = fs::read_to_string("sphere_clear.cam/main.part").expect("read sphere_clear.cam");
    let (_, emitted_moves) = gcode::program_with_moves(&parse::parse(&src).expect("parse")).expect("generates");
    let part = parse::parse(&src).expect("parse");
    assert!(
        nacs::stock::entry_findings(&part, &emitted_moves).is_empty(),
        "a finish behind its own rough is the normal start of work"
    );

    let alone = src.replace("run Clear(tool=T1, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n", "");
    let part = parse::parse(&alone).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("generates");
    let findings = nacs::stock::entry_findings(&part, &emitted_moves);
    assert_eq!(findings.len(), 1, "unpaired, the same entry keeps its voice: {:?}", findings);
    assert!(findings[0].message.contains("no earlier op cleared"), "{}", findings[0]);
}

/// The license holds under the gate too: `entry gate` escalates FINDINGS, and
/// a licensed entry produces none, so the gated contained fixture builds.
#[test]
fn test_the_entry_gate_passes_a_licensed_pairing() {
    let src = fs::read_to_string("sphere_clear.cam/main.part").expect("read sphere_clear.cam");
    let gated = format!("entry gate\n{}", src);
    let part = parse::parse(&gated).expect("parse");
    assert!(gcode::program(&part).is_ok(), "the gate rejects findings, and a licensed entry has none");
}

/// What remains of the containment corpus, pinned by name. 26 of the 31
/// too-short stocks were contained by the rim-area thread, the three torus
/// fixtures joined them once the annulus island could be said (DECISIONS.md →
/// "the torus's annulus island…", slice 2), and the form-tool pair joined on
/// Phase 10 lane B's first slice (DECISIONS.md → "Phase 10 lane B, slice 0…"):
/// the belief that a contained stock under the flat collar would ship a
/// permanent foul warning was the ball's experience, never measured for the
/// form tool, and a rough paired in front quiets every lint without a line of
/// tube code. **31 → 2.** What is left is the two camp-3 fixtures, ruled
/// permanently uncontained by the Phase-10 DRAFT (a mesh rougher is the
/// field's job). The lint keeps saying why for each; this pin keeps the list
/// from growing back silently.
#[test]
fn test_the_too_short_blanks_that_remain_are_exactly_the_named_ones() {
    let expected = ["mesh.cam", "nurbs_patch.cam"];
    let mut fouling: Vec<String> = Vec::new();
    for entry in fs::read_dir(".").expect("read repo root") {
        let path = entry.expect("dir entry").path();
        if path.extension().map(|e| e == "cam").unwrap_or(false) {
            let src = fs::read_to_string(path.join("main.part")).expect("read fixture");
            let geom = fs::read_to_string(path.join("geometry.part")).ok();
            let part = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path))
                .unwrap_or_else(|e| panic!("{}: parse failed: {}", path.display(), e));
            if !containment_warnings(&part).is_empty() {
                fouling.push(path.file_name().unwrap().to_string_lossy().into_owned());
            }
        }
    }
    fouling.sort();
    assert_eq!(fouling, expected, "the containment remainder moved — a contained fixture regressed, or a new one shipped too short");
}

#[test]
fn test_the_entry_gate_refuses_on_the_same_findings() {
    // One computation, two voices: the opt-in gate turns the advisory into a
    // error, and the grammar rejects a mode it doesn't know.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let gated = format!("entry gate\n{}", src);
    let part = parse::parse(&gated).expect("the entry directive parses");
    let err = gcode::program(&part).expect_err("the gate rejects the uncleared finish");
    assert!(err.starts_with("entry gate:"), "the error names its gate, got: {}", err);
    assert!(err.contains("'Window'"), "the error carries the finding, got: {}", err);

    let ungated = parse::parse(&src).expect("the fixture parses");
    assert!(gcode::program(&ungated).is_ok(), "advisory mode still emits the program");

    let err = parse::parse(&format!("entry maybe\n{}", src)).expect_err("an unknown mode rejects");
    assert!(err.contains("unknown entry mode"), "got: {}", err);
}

#[test]
fn test_the_control_lead_backs_into_the_void() {
    // The companion the seat forced (DECISIONS.md "the control-comp centerline
    // resolve", slice 2): the control arm used to back its lead point off one
    // diameter OUTSIDE the loop regardless of orientation — for a pocket that
    // is standing material, and the machine plunged there (the sidecar's
    // proposed pockets did exactly this). The lead now backs into the void the
    // winding names, so the plain window fixture generates under control comp
    // with its lead inside the loop, mirroring the engine arm's entry.
    let src = fs::read_to_string("profile_pocket.cam/main.part").expect("read profile_pocket.cam");
    let ctl = src.replace(
        "run Side(tool=T5, surface=Window)",
        "run Side(tool=T5, surface=Window, comp=control)",
    );
    let part = parse::parse(&ctl).expect("a comp switch parses");
    let g = gcode::program(&part).expect("a control pocket wall finish is machinable as declared");
    // Phase 47 slice 2: the lead rides off the COMPENSATED start — the
    // parallel's first vertex, a radius along both edges from the corner,
    // (25, 25) — one diameter along the corner's bisector, (32.071, 32.071).
    // Until then it rode the centroid's ray from the AUTHORED corner, which
    // on a square is the same diagonal a radius sooner, (27.071, 27.071).
    assert!(
        g.contains("G00 X32.071 Y32.071"),
        "the lead point sits one diameter inside the window off the compensated start, not outside it: {g}"
    );
    assert!(!g.contains("X12.929"), "the outward lead point is gone");
}

// ---- adaptive clearing, slice 1: strategy=rest (DECISIONS.md "adaptive
// clearing — the field picks the passes…") ------------------------------------

/// The slice-1 demo pair as a golden: the wall-finished bore whose core the
/// rest-material report sizes, then the rest Clear that cuts it. Every loop of
/// the family touches the standing core or grazes its own wall (tangency never
/// certifies), so the audit line reads kept 7 of 7 — and the program's own
/// text says so.
#[test]
fn test_cylinder_bore_rest_golden() {
    let main_src = fs::read_to_string("cylinder_bore_rest.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse cylinder_bore_rest.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/cylinder_bore_rest.golden.gcode").expect("failed to read cylinder_bore_rest golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

fn rest_src(strategy_params: &str) -> String {
    format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
         tool T5 Endmill(dia=10.0)\n\
         feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
         run Clear(tool=T5, profile=Bore, stepover=4.0{strategy_params})\n"
    )
}

/// The strategy grammar, error by error. Each names its rule: `adaptive`
/// is reserved vocabulary (the compress force/spline precedent), rest without
/// voxel_cell rejects because the resolution is program text with
/// deliberately no derived default, voxel_cell without rest is a parameter
/// nothing reads, and strategy on a non-Clear op is rejected rather than
/// ridden ignored.
#[test]
fn test_rest_strategy_grammar_refusals() {
    let err = parse::parse(&rest_src(", strategy=adaptive, voxel_cell=1.0")).expect_err("adaptive is reserved");
    assert!(err.contains("reserved"), "adaptive must be named reserved, got: {err}");
    assert!(err.contains("trochoidal"), "the reservation names its tier, got: {err}");

    let err = parse::parse(&rest_src(", strategy=fastest, voxel_cell=1.0")).expect_err("unknown strategy");
    assert!(err.contains("unknown strategy"), "got: {err}");

    let err = parse::parse(&rest_src(", strategy=rest")).expect_err("rest without voxel_cell");
    assert!(
        err.contains("voxel_cell") && err.contains("no derived default"),
        "the error must say the resolution is explicit-only, got: {err}"
    );

    let err = parse::parse(&rest_src(", voxel_cell=1.0")).expect_err("voxel_cell without strategy");
    assert!(err.contains("field-selected strategy"), "got: {err}");

    let err = parse::parse(&rest_src(", strategy=rest, voxel_cell=0")).expect_err("zero cell");
    assert!(err.contains("must be positive"), "got: {err}");

    let facing = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Top Plane(z=-2.0)\n\
        run End(tool=T5, surface=Top, stepover=6.0, strategy=rest, voxel_cell=1.0)\n";
    // Since Phase 33 an End takes ONE word of strategy — `rest`, read from
    // the analytical stock model — so a voxel cell beside it is refused as
    // a field that does not exist, and every other word as a Clear's.
    let err = parse::parse(facing).expect_err("voxel_cell on a rest Floor");
    assert!(err.contains("no field"), "got: {err}");
    let err = parse::parse(&facing.replace("strategy=rest", "strategy=regions")).expect_err("regions on Floor");
    assert!(err.contains("strategy is a Clear parameter"), "got: {err}");
}

/// Clear's CylinderBore arm carries the pocket's whole error family: a boss
/// is the opposite op (side-of-material as a type error), a bore the tool
/// cannot enter is unmachinable, and a stepover that outruns the shape leaves
/// an uncut core at the axis — a hard error, never a silently thinner op.
#[test]
fn test_clear_bore_arm_refusals() {
    let boss = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Post CylinderBoss(dia=30.0, extent=12.0, x=50.0, y=50.0, z=-12.0)\n\
        run Clear(tool=T5, profile=Post, stepover=4.0)\n";
    // Since Phase 10 lane A the cylinder boss IS a standing revolve, so a
    // Clear on it is the moat — and without a stepdown it rejects as every
    // revolve does, not as a side-of-material type error.
    let err = parse::parse(boss).expect_err("Clear on a boss without a stepdown");
    assert!(err.contains("requires a positive stepdown"), "got: {err}");

    let tiny = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Bore CylinderBore(dia=8.0, extent=12.0, x=50.0, y=50.0)\n\
        run Clear(tool=T5, profile=Bore, stepover=4.0)\n";
    let part = parse::parse(tiny).expect("a too-small bore parses; generation rejects");
    let err = gcode::program(&part).expect_err("no pass fits");
    assert!(err.contains("too small for tool"), "got: {err}");

    // dia 44 (r_wall 19), tool dia 6, stepover 5: radii 19, 14, 9, 4 — the
    // innermost pass reaches 1mm from the axis and a 1mm core stands.
    let core = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=6.0)\n\
        feature Bore CylinderBore(dia=44.0, extent=12.0, x=50.0, y=50.0)\n\
        run Clear(tool=T5, profile=Bore, stepover=5.0)\n";
    let part = parse::parse(core).expect("parses; generation rejects");
    let err = gcode::program(&part).expect_err("uncut core");
    assert!(err.contains("uncut core"), "got: {err}");
}

/// Both of the walker's front-door errors, now retired, asserting
/// ACCEPTANCE end to end — the shape a retired error's test should take
/// (DECISIONS.md "the profile silhouette…", slice 2, and "the control-comp
/// centerline resolve", slice 1). A `strategy=rest` Clear downstream of a
/// FORM-CUTTER Finish used to reject, because a profile tool had no stamp and
/// the before-op field came back incomplete; the shaped stamp completes it and
/// the rest op runs. Same for a control-comp Finish upstream, whose resolved
/// channel completes the field the same way.
///
/// The incompleteness LAW those errors used to demonstrate is unchanged and
/// still enforced in `gcode::program` — with no front-door source of an
/// incomplete field left, its evidence is
/// `vox::tests::a_necked_silhouette_refuses_by_name`, which hands the walker
/// the one shape it still declines.
#[test]
fn test_rest_accepts_what_the_walker_now_stamps() {
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T1 FormTool(dia=10.0, profile=\"tool_profile.svg\")\n\
        tool T5 Endmill(dia=10.0)\n\
        feature TopDome Sphere(dia=20, x=80, y=80, z=0)\n\
        feature Bore CylinderBore(dia=60.0, extent=12.0, x=40.0, y=40.0)\n\
        run Form(tool=T1, surface=TopDome, stepover=3.0)\n\
        run Clear(tool=T5, profile=Bore, stepover=4.0, strategy=rest, voxel_cell=1.0)\n";
    let part = parse::parse_project_with_base(
        src,
        None,
        Some(std::path::Path::new("sphere_formtool.cam")),
    )
    .expect("the program parses");
    let g = gcode::program(&part).expect("a form-cutter upstream no longer blocks rest");
    assert!(g.contains("(rest: "), "the rest op must have run and said so: {g}");

    // The same shape with a shape with a
    // control-comp Finish upstream now emits — the resolved channel
    // completes the before-op field and the rest op runs against it.
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
        run Side(tool=T5, surface=Bore, stepover=5.0, comp=control)\n\
        run Clear(tool=T5, profile=Bore, stepover=4.0, strategy=rest, voxel_cell=1.0)\n";
    let part = parse::parse(src).expect("parses");
    let g = gcode::program(&part).expect("a control-comp upstream no longer blocks rest");
    assert!(g.contains("(rest: "), "the rest op must have run and said so: {g}");
}

/// The comp-twin flagship (DECISIONS.md "the control-comp centerline
/// resolve", pin 4): two programs identical except the upstream Finish's
/// comp mode, feeding the same strategy=rest Clear, must emit BYTE-IDENTICAL
/// moves for the rest op — the twins' fields differ only in entry sweeps
/// through space the upstream Clear already voided, so the selection cannot
/// tell them apart. The scenario keeps some loops and drops others (a
/// selection with nothing at stake pins nothing): a dia-20 pre-clear voids
/// the middle DEEPER than the rest op's floor (the τ-law's tangency finding:
/// prior work certifies only past ~1.6 cells, in Z as much as in plan), the
/// wall finish grazes at the shared depth and correctly stays spring-passed,
/// and the dia-10 rest clear drops the loop riding the deep-cleared middle.
#[test]
fn test_rest_selection_is_identical_across_comp_twins() {
    let src = |comp: &str| {
        format!(
            "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
            stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
            tool T5 Endmill(dia=10.0)\n\
            tool T9 Endmill(dia=16.0)\n\
            feature Middle CylinderBore(dia=20.0, extent=14.0, x=50.0, y=50.0)\n\
            feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
            run Clear(tool=T5, profile=Middle, stepover=4.0)\n\
            run Side(tool=T9, surface=Bore, stepover=5.0{comp})\n\
            run Clear(tool=T5, profile=Bore, stepover=4.0, strategy=rest, voxel_cell=0.5)\n"
        )
    };
    let engine = gcode::program(&parse::parse(&src("")).unwrap()).unwrap();
    let control = gcode::program(&parse::parse(&src(", comp=control")).unwrap()).unwrap();
    assert_ne!(engine, control, "the twins must differ upstream or the pin is vacuous");

    // The rest op's own section: from its header comment to the end of the
    // program (the trailer is shared, so including it costs nothing).
    let tail = |g: &str| {
        let i = g.find("(--- bore clear: Bore").expect("the rest op's header must be present");
        g[i..].to_string()
    };
    let (e_tail, c_tail) = (tail(&engine), tail(&control));
    assert_eq!(e_tail, c_tail, "the rest selection must not be able to tell the comp twins apart");

    // And the selection is a real one: some loops dropped, some kept.
    let audit = e_tail
        .lines()
        .find(|l| l.starts_with("(rest: kept "))
        .expect("the audit line must be present — an all-void or all-kept scenario pins nothing");
    let nums: Vec<usize> = audit
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    let (kept, total) = (nums[0], nums[1]);
    assert!(kept > 0 && kept < total, "a meaningful selection, got {audit}");
}

/// An empty rest-clear is an answer, not an error: when every loop certifies
/// void — here the whole pocket region was faced away deeper than the pocket
/// floor before the op ran — the op emits its header comment and no cutting
/// moves, and the header says why. Distinct from the unmachinable-feature
/// hard error, and stated so the G-code reads as a decision, not an omission.
#[test]
fn test_empty_rest_clear_is_an_answer() {
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Top Plane(z=-10.0)\n\
        feature Pocket ProfilePocket(extent=6.0\n    20 20\n    60 20\n    60 60\n    20 60\n)\n\
        run End(tool=T5, surface=Top, stepover=6.0)\n\
        run Clear(tool=T5, profile=Pocket, stepover=4.0, strategy=rest, voxel_cell=1.0)\n";
    let part = parse::parse(src).expect("parses");
    let g = gcode::program(&part).expect("an all-void rest op must not fail the program");
    assert!(g.contains("(rest: nothing stands"), "the header must state the decision: {g}");
    // No motion after the empty op's header: the section between the pocket
    // title and the program footer must be comments only.
    let tail = g.split("(--- pocket clear:").nth(1).expect("the op title must still be emitted");
    let cutting: Vec<&str> = tail
        .lines()
        .filter(|l| l.starts_with("G01") || l.starts_with("G02") || l.starts_with("G03"))
        .collect();
    assert!(cutting.is_empty(), "an empty rest op must emit no cutting moves, found {cutting:?}");
    // And the dense sibling — same program without the strategy — does cut,
    // so the emptiness above is the field's decision, not a broken emitter.
    let dense_src = src.replace(", strategy=rest, voxel_cell=1.0", "");
    let dense = gcode::program(&parse::parse(&dense_src).unwrap()).unwrap();
    assert!(
        dense.split("(--- pocket clear:").nth(1).unwrap().lines().any(|l| l.starts_with("G01")),
        "the dense sibling must still cut"
    );
}

// ---- adaptive clearing, slice 2: strategy=regions (DECISIONS.md "adaptive
// clearing, region decomposition…") -------------------------------------------

/// The slice-2 demo as a golden: the dia-10 dense clear leaves its four
/// corner fillets, the dia-3 regions op cuts four little spirals over them,
/// and the residue law states the small tool's own corner residue in the
/// program's own text.
#[test]
fn test_profile_pocket_regions_golden() {
    let main_src = fs::read_to_string("profile_pocket_regions.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse profile_pocket_regions.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/profile_pocket_regions.golden.gcode").expect("failed to read profile_pocket_regions golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

/// The regions grammar: the bore arm ACCEPTS (the not-yet-built error
/// retired when the arm landed — DECISIONS.md "adaptive clearing, the regions
/// bore arm…"), and the explicit-resolution law reads the same as rest's.
#[test]
fn test_regions_grammar_refusals() {
    let bore = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
        run Clear(tool=T5, profile=Bore, stepover=4.0, strategy=regions, voxel_cell=1.0)\n";
    parse::parse(bore).expect("the bore arm is built; the reserved error retired with it");

    let pocket = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=60, l=60, h=15, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Pocket ProfilePocket(extent=6.0\n    15 15\n    45 15\n    45 45\n    15 45\n)\n\
        run Clear(tool=T5, profile=Pocket, stepover=4.0, strategy=regions)\n";
    let err = parse::parse(pocket).expect_err("regions without voxel_cell");
    assert!(
        err.contains("strategy=regions") && err.contains("no derived default"),
        "got: {err}"
    );
}

/// The residue law's message fork, both arms. Re-running the SAME tool as a
/// regions op finds the fillets standing but no legal center within reach of
/// them — "no reachable work", with the residue stated, and no cutting moves.
/// A pocket faced away deeper than its own floor has no work at all —
/// "nothing stands". The two answers are different facts and say so.
#[test]
fn test_regions_answers_without_cutting() {
    let base = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=60, l=60, h=15, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Pocket ProfilePocket(extent=6.0\n    15 15\n    45 15\n    45 45\n    15 45\n)\n\
        run Clear(tool=T5, profile=Pocket, stepover=4.0)\n\
        run Clear(tool=T5, profile=Pocket, stepover=4.0, strategy=regions, voxel_cell=0.25)\n";
    let g = gcode::program(&parse::parse(base).unwrap()).unwrap();
    assert!(g.contains("(regions: no reachable work"), "{g}");
    assert!(g.contains("(regions residue:"), "the standing fillets must be counted: {g}");
    let tail = g.split("(--- pocket regions:").nth(1).unwrap();
    assert!(
        !tail.lines().any(|l| l.starts_with("G01")),
        "no reachable work must emit no cutting moves"
    );

    let clean = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=60, l=60, h=15, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Top Plane(z=-10.0)\n\
        feature Pocket ProfilePocket(extent=6.0\n    15 15\n    45 15\n    45 45\n    15 45\n)\n\
        run End(tool=T5, surface=Top, stepover=6.0)\n\
        run Clear(tool=T5, profile=Pocket, stepover=4.0, strategy=regions, voxel_cell=0.25)\n";
    let g = gcode::program(&parse::parse(clean).unwrap()).unwrap();
    assert!(g.contains("(regions: nothing stands"), "{g}");
    assert!(!g.contains("(regions residue:"), "no work means no residue line: {g}");
}

/// Determinism, the DRAFT's obligation: the derivation is closed under
/// (program text, voxel_cell) — two independent parses and emissions of the
/// fixture produce one byte-stream.
#[test]
fn test_regions_emission_is_deterministic() {
    let main_src = fs::read_to_string("profile_pocket_regions.cam/main.part").unwrap();
    let a = gcode::program(&parse::parse(&main_src).unwrap()).unwrap();
    let b = gcode::program(&parse::parse(&main_src).unwrap()).unwrap();
    assert_eq!(a, b);
}

// ---- adaptive clearing, the regions bore arm (DECISIONS.md "adaptive
// clearing, the regions bore arm…") -------------------------------------------

/// The bore-arm demo as a golden: two declared bores overlap, the dia-10
/// dense clear empties the first, and the dia-3 regions op cuts one spiral
/// over the LUNE standing in the second — off-center work no concentric
/// family can aim at — with the cusp slivers stated by the residue law.
#[test]
fn test_bore_regions_golden() {
    let main_src = fs::read_to_string("bore_regions.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse bore_regions.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/bore_regions.golden.gcode").expect("failed to read bore_regions golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    // The demo's shape, pinned in its own text: one region (the lune is one
    // connected standing set), and its residue named.
    assert!(generated_gcode.contains("(regions: 1 standing region"), "{generated_gcode}");
    assert!(generated_gcode.contains("(regions residue:"), "{generated_gcode}");
}

// ---- the corner rounder's operation (DECISIONS.md "the corner rounder gets an
// operation", slice 1) --------------------------------------------------------
// The part's round IS the tool's ground arc, so this op places a form rather
// than approximating a surface. Everything below is a claim about WHERE it
// places it, and every one of them is a claim about the arc — never the pilot.

const ER_PREAMBLE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\nstock Box(w=120, l=100, h=20, x=0, y=0, z=0)\ntool T5 Endmill(dia=10.0)\n";
// Nominal 3mm rounder: rc = 6, zc = 3, pilot relieved 0.03/side under the 6.00
// cylinder the arc is tangent to.
const ER_T2: &str = "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94)\n";

// The lip is broken AFTER the feature is opened, exactly as a real program
// orders it — otherwise the entry restriction rejects the plunge, and rightly:
// a rounder entering solid stock is a crash, not a test fixture.
fn er_part(tool: &str, feature: &str, run: &str) -> Result<parse::Part, String> {
    let open = if feature.starts_with("CylinderBore") {
        "run Clear(tool=T5, profile=F, stepover=4.0)\n"
    } else {
        "run Side(tool=T5, surface=F, stepover=5.0)\n"
    };
    parse::parse(&format!("{ER_PREAMBLE}{tool}feature F {feature}\n{open}{run}\n"))
}

#[test]
fn test_edge_round_golden() {
    // The whole thread end to end: both walls, then both lips — the bore's in
    // engine comp, the boss's in control.
    let main_src = fs::read_to_string("edge_round.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse edge_round.cam");
    parse::validate(&part).expect("edge_round.cam must validate");
    let generated = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/edge_round.golden.gcode").expect("failed to read edge_round golden");
    assert_eq!(generated.trim(), golden.trim());

    // The header is where this design either helps the operator or springs the
    // trap on them: D must carry the EFFECTIVE diameter, and must say so. The
    // body OD (12.0) and the pilot (5.94) both appear as labelled clearance,
    // and neither is the register value.
    assert!(generated.contains("comp register D2 = 6.000"), "D2 must be the effective diameter");
    assert!(generated.contains("not the pilot"), "the header must name what D is NOT");
    assert!(generated.contains("relieved 0.030 per side"), "the relief must be a number in the program");
    // One pass per lip: no ladder, because the form is stamped, not approximated.
    assert_eq!(generated.matches("break edge").count(), 2, "two lips, one pass each");
}

#[test]
fn test_edge_round_places_the_arc_centre_not_the_pilot() {
    // A bore of radius 20 with a 3mm round. The tool's arc centre must land on
    // the round's own centre of curvature: 3mm into the material from the wall
    // (rho = 23) and 3mm below the top face (z = -3).
    let p = er_part(ER_T2, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                    "run Form(tool=T2, surface=F)").unwrap();
    parse::validate(&p).unwrap();
    let g = gcode::program(&p).unwrap();

    // The axis rides effective_dia/2 = 3.000 INSIDE the wall: 20 - 3 = 17, so
    // the ring sits at x = 30 + 17 = 47. Were it comped off the pilot (2.970)
    // the ring would be at 47.030 and the arc would cut 0.030 into the wall —
    // the gouge this whole design exists to prevent.
    assert!(g.contains("G01 X47.000 Y50.000"), "the axis rides the effective radius:\n{g}");
    assert!(!g.contains("X47.030"), "positioning from the relieved pilot is the defect");

    // The emitted Z is the VIRTUAL SHARP CORNER, which lands on the model's
    // edge — the top face at z = 0. The tip goes a whole gauge_z below it, and
    // nobody had to compute where the tip goes.
    let seg = g.split("break edge").nth(1).unwrap();
    assert!(seg.contains("G01 Z0.000 F300.000"), "the datum lands on the edge:\n{seg}");
}

#[test]
fn test_edge_round_boss_mirrors_the_sign() {
    // Material inside the circle, so the axis rides OUTSIDE by the same number.
    let p = er_part(ER_T2, "CylinderBoss(dia=40.0, extent=12.0, x=90.0, y=50.0, z=-12.0, edge_round=3.0)",
                    "run Form(tool=T2, surface=F)").unwrap();
    parse::validate(&p).unwrap();
    let g = gcode::program(&p).unwrap();
    // 20 + 3 = 23 -> x = 113, and the datum lands on the boss top (z = 0).
    assert!(g.contains("G01 X113.000 Y50.000"), "{g}");
}

#[test]
fn test_edge_round_control_comp_emits_the_model_edge_and_folds_the_standoff_into_d() {
    // Under control comp the coordinates ARE the model's edge circle — which is
    // exactly what driving the virtual sharp corner along that edge means, so
    // the datum and the comp mode turn out to be the same object.
    let p = er_part(ER_T2, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                    "run Form(tool=T2, surface=F, comp=control)").unwrap();
    parse::validate(&p).unwrap();
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("G01 G41 D2 X50.000 Y50.000"), "the path is the part edge (30 + 20):\n{g}");
    assert!(g.contains("comp register D2 = 6.000"));

    // Now with a standoff. The PATH must not move — it is still the model edge —
    // and the standoff must appear in the one number that reaches the machine.
    // Two homes for one offset is the silent stacking this design exists to kill.
    let p = er_part(ER_T2, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                    "run Form(tool=T2, surface=F, comp=control, wall_standoff=0.05)").unwrap();
    parse::validate(&p).unwrap();
    let g2 = gcode::program(&p).unwrap();
    assert!(g2.contains("G01 G41 D2 X50.000 Y50.000"), "the standoff must NOT move the path:\n{g2}");
    assert!(g2.contains("comp register D2 = 6.100"), "0.05 per side folds into D:\n{g2}");
    assert!(g2.contains("= effective 6.000 + 2 x standoff 0.050"), "the header shows its work:\n{g2}");

    // Under ENGINE comp the same standoff goes into the coordinates instead,
    // and D is not in play at all — the other half of "exactly one home".
    let p = er_part(ER_T2, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                    "run Form(tool=T2, surface=F, wall_standoff=0.05)").unwrap();
    let g3 = gcode::program(&p).unwrap();
    assert!(g3.contains("G01 X46.950 Y50.000"), "engine comp backs the axis off the wall:\n{g3}");
}

#[test]
fn test_edge_round_one_register_holds_one_number() {
    let src = format!(
        "{ER_PREAMBLE}{ER_T2}feature A CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)\n\
         feature B CylinderBore(dia=40.0, extent=12.0, x=90.0, y=50.0, edge_round=3.0)\n\
         run Form(tool=T2, surface=A, comp=control, wall_standoff=0.05)\n\
         run Form(tool=T2, surface=B, comp=control, wall_standoff=0.10)\n"
    );
    let err = parse::parse(&src)
        .and_then(|p| parse::validate(&p).map(|_| p))
        .unwrap_err();
    assert!(err.contains("different D2 values"), "{err}");
}

#[test]
fn test_edge_round_deviation_is_a_number_and_advise_is_the_default() {
    // A catalogue tool with a perfect arc and no standoff says nothing: there
    // is no deviation to report, and inventing one would be noise.
    let p = er_part(ER_T2, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                    "run Form(tool=T2, surface=F)").unwrap();
    assert!(parse::deviation_findings(&p).is_empty(), "a nominal tool has nothing to declare");

    // A measured one does, and every contributor is named separately with the
    // input behind it — a step you can attribute is a step you can fix.
    let measured = "tool T2 CornerRounder(radius=3.0, effective_dia=6.01, gauge_z=5.99, dia=12.0, pilot=5.94, radius_error=-0.02, trail_wall=4.0, trail_face=1.5, source=measured)\n";
    let p = er_part(measured, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                    "run Form(tool=T2, surface=F, face_standoff=0.02)").unwrap();
    parse::validate(&p).expect("a measured arc is not a mismatch — it is a deviation");
    let f = parse::deviation_findings(&p);
    assert_eq!(f.len(), 1, "{f:?}");
    let f = &f[0];
    assert!(f.message.contains("measured"), "provenance is part of the finding: {f}");
    // R(1 - cos 4deg) on a 2.98mm arc = 0.00726.
    assert!(f.message.contains("wall step 0.0073"), "{f}");
    // R(1 - cos 1.5deg) = 0.00102, plus the 0.02 standoff.
    assert!(f.message.contains("face step 0.0210"), "{f}");
    assert!(f.message.contains("radius -0.0200mm off the declared 3.000mm"), "{f}");

    // The program still comes out — advise is the default, and it has to be:
    // every real arc trails off, so a gate by default rejects every real
    // program. Opting in rejects on the very same finding.
    assert!(gcode::program(&p).is_ok(), "advisory findings never withhold the program");
    let err = parse::parse(&format!("deviation gate\n{ER_PREAMBLE}{measured}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T2, surface=F, face_standoff=0.02)\n"))
        .map(|_| ())
        .unwrap_err();
    assert!(err.starts_with("deviation gate:"), "{err}");
    assert!(err.contains("wall step"), "the gate rejects on the advisory's own words: {err}");
}

#[test]
fn test_a_short_arc_leaves_its_ridge_standing_in_the_model() {
    // The trail-off carrying load, which is what stops it being decoration. An
    // arc that runs out short removes LESS than the nominal band, and the model
    // has to know: over-stating removal is the dangerous direction, because
    // `check_moves` reads this model to decide whether a plunge is into air.
    let nominal = er_part(ER_T2, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                          "run Form(tool=T2, surface=F)").unwrap();
    // 20deg short of tangency at the wall end: the cut stops R(1-cos20) =
    // 0.181mm out from the wall, so a ridge stands against the flank.
    let trailed_tool = "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, trail_wall=20.0, source=measured)\n";
    let trailed = er_part(trailed_tool, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)",
                          "run Form(tool=T2, surface=F)").unwrap();

    let after_nominal = nacs::stock::remaining_after(&nominal, 2);
    let after_trailed = nacs::stock::remaining_after(&trailed, 2);
    assert!(after_nominal.complete && after_trailed.complete, "both models account for themselves");

    // A point just inside the ridge: 0.05mm out from the wall, and 0.05mm below
    // where the nominal round's surface would be there.
    let (x, y) = (30.0 + 20.05, 50.0);
    let z = -3.0 + (9.0f64 - (23.0 - 20.05f64).powi(2)).sqrt() + 0.05;
    assert!(!after_nominal.material_at(x, y, z), "the full arc reaches the wall and takes this");
    assert!(after_trailed.material_at(x, y, z), "a 20deg trail-off leaves it standing");
}

#[test]
fn test_edge_round_refusals() {
    let bore = "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)";
    let cases: Vec<(String, &str)> = vec![
        // A form cutter cuts its own radius or the wrong one.
        (format!("{ER_T2}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=5.0)\nrun Form(tool=T2, surface=F)"),
         "grinds a 3.000mm arc"),
        // Declared round, no field on the feature.
        (format!("{ER_T2}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0)\nrun Form(tool=T2, surface=F)"),
         "declares no edge_round"),
        // Nothing to space, nothing to ladder.
        (format!("{ER_T2}feature F {bore}\nrun Form(tool=T2, surface=F, stepover=1.0)"), "takes no stepover"),
        (format!("{ER_T2}feature F {bore}\nrun Form(tool=T2, surface=F, scallop=0.01)"), "takes no scallop"),
        // An allowance on a form cut means cutting a different radius.
        (format!("{ER_T2}feature F {bore}\nrun Form(tool=T2, surface=F, stock_to_leave=0.2)"), "takes no stock_to_leave"),
        // A standoff backs the tool AWAY; a negative one drives it in.
        (format!("{ER_T2}feature F {bore}\nrun Form(tool=T2, surface=F, wall_standoff=-0.05)"), "must not be negative"),
        // A round deeper than the wall it lives on.
        (format!("{ER_T2}feature F CylinderBore(dia=40.0, extent=2.0, x=30.0, y=50.0, edge_round=3.0)\nrun Form(tool=T2, surface=F)"),
         "as deep as the"),
        // The tool has to fit down the hole whose lip it is breaking.
        (format!("{ER_T2}feature F CylinderBore(dia=5.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0)\nrun Form(tool=T2, surface=F)"),
         "ride outside the hole"),
        // A curved surface has no top edge of its own to break.
        (format!("{ER_T2}feature F Sphere(dia=40.0, x=30.0, y=50.0, z=-10.0)\nrun Form(tool=T2, surface=F)"),
         "only the prismatic pairs"),
        // The placement puts the round at ONE Z, which a wire does not have.
        (format!("{ER_T2}feature F ContourPocket(extent=3.0\n 20 20 0\n 80 20 -2\n 80 80 0\n 20 80 -2\n)\nrun Form(tool=T2, surface=F)"),
         "not planar"),
    ];
    for (body, needle) in cases {
        let src = format!("{ER_PREAMBLE}{body}\n");
        let err = parse::parse(&src)
            .and_then(|p| parse::validate(&p).map(|_| p))
            .err()
            .unwrap_or_else(|| panic!("expected an error containing `{needle}`"));
        assert!(err.contains(needle), "expected `{needle}`, got: {err}");
    }
}

// ---- the polyline pair (DECISIONS.md "the corner rounder gets an operation",
// slice 2) --------------------------------------------------------------------
// The placement is the circular pair's, with the plan curve swapped. What is
// new is that the round stops being CONSTANT along the edge, and every claim
// below is about the project rejecting to pretend otherwise.

// The L-pocket of edge_round_profile.cam: five sharp corners the tool cannot
// reach into, and one reflex corner at (30,50) — a convex MATERIAL corner —
// that it wraps.
const ER_L: &str = "feature F ProfilePocket(extent=12.0, edge_round=3.0\n 10 20\n 50 20\n 50 50\n 30 50\n 30 80\n 10 80\n)\n";
// The same round on standing material: a CW square boss, every corner outside.
const ER_PAD: &str = "feature F ProfileBoss(extent=12.0, edge_round=3.0\n 70 30\n 70 70\n 110 70\n 110 30\n)\n";

fn er_profile_part(feature: &str, run: &str) -> parse::Part {
    // A boss is roughed before it is finished (Phase 10 lane A): the wall
    // finish leads in "in cleared air" one diameter out, and since its band
    // joined the stock model an unroughed boss rejects at that plunge. So the
    // pad variant carries two ops before `run`: the moat, then the wall.
    let open = if feature.contains("ProfilePocket") {
        "run Clear(tool=T5, profile=F, stepover=4.0)\n"
    } else {
        "run Clear(tool=T5, profile=F, stepover=4.0, stepdown=4.0)\nrun Side(tool=T5, surface=F)\n"
    };
    let p = parse::parse(&format!("{ER_PREAMBLE}{ER_T2}{feature}{open}{run}\n"))
        .expect("the polyline edge round must parse");
    parse::validate(&p).expect("the polyline edge round must validate");
    p
}

#[test]
fn test_edge_round_profile_golden() {
    let main_src = fs::read_to_string("edge_round_profile.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse edge_round_profile.cam");
    parse::validate(&part).expect("edge_round_profile.cam must validate");
    let generated = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/edge_round_profile.golden.gcode").expect("failed to read golden");
    assert_eq!(generated.trim(), golden.trim());
    assert_eq!(generated.matches("break edge").count(), 2, "two lips, one pass each");
}

#[test]
fn test_edge_round_profile_rides_the_offset_path_at_the_effective_radius() {
    // The pocket wall at x = 10 with a 3mm round: the axis rides 3mm INTO the
    // void, so the path runs at x = 13 — the effective radius, never the pilot
    // (which would put it at 12.970 and cut the finished wall).
    let p = er_profile_part(ER_L, "run Form(tool=T2, surface=F)");
    let g = gcode::program(&p).unwrap();
    let seg = g.split("break edge").nth(1).unwrap();
    assert!(seg.contains("G01 X13.000 Y23.000"), "the axis rides the effective radius:\n{seg}");
    assert!(!seg.contains("X12.970"), "positioning from the relieved pilot is the defect");
    // The emitted Z is the virtual sharp corner, which lands on the model's own
    // top face — the tip goes a whole gauge_z below without anyone deriving it.
    assert!(seg.contains("G01 Z0.000 F300.000"), "the datum lands on the edge:\n{seg}");

    // The one reflex corner is a convex MATERIAL corner, so the tool wraps it —
    // a real arc of the effective radius, not a pivot. That is the corner the
    // round DOES reach, and it is emitted as an arc rather than approximated.
    assert!(seg.contains("G02 X27.000 Y50.000 I0.000 J3.000"), "the tool wraps the material corner:\n{seg}");
}

#[test]
fn test_edge_round_profile_control_comp_emits_the_authored_edge() {
    // Control comp on a polyline is slice 1's finding one shape later: the
    // emitted coordinates ARE the model's edge, because driving the virtual
    // sharp corner along that edge is what control comp already means here.
    let p = er_profile_part(ER_PAD, "run Form(tool=T2, surface=F, comp=control, wall_standoff=0.05)");
    let g = gcode::program(&p).unwrap();
    let seg = g.split("break edge").nth(1).unwrap();
    assert!(seg.contains("G01 G41 D2 X70.000 Y30.000"), "the path is the authored edge:\n{seg}");
    assert!(g.contains("comp register D2 = 6.100"), "0.05 per side folds into D, not into the path:\n{g}");
    // The standoff moved the register and nothing else: the emitted loop is
    // byte-identical to the one without it, so exactly one number reaches the
    // machine and stacking is unrepresentable.
    let plain = er_profile_part(ER_PAD, "run Form(tool=T2, surface=F, comp=control)");
    let g0 = gcode::program(&plain).unwrap();
    let path_of = |text: &str| -> Vec<String> {
        text.split("break edge (control):").nth(1).unwrap()
            .lines().take_while(|l| !l.contains("break edge (control, resolved"))
            .filter(|l| l.starts_with("G01") || l.starts_with("G02") || l.starts_with("G03"))
            .map(|l| l.to_string()).collect()
    };
    assert_eq!(path_of(&g), path_of(&g0), "a wall standoff must not move the emitted path");
}

#[test]
fn test_the_round_a_sharp_corner_never_gets_is_a_number() {
    // The polyline pair's own deviation contributor, and the one a circle
    // structurally cannot have. At a sharp plan corner the axis pivots on its
    // offset vertex and the arc never reaches the corner — every corner rounder
    // ever run into a sharp pocket corner does this, silently. Here it is
    // 3(sqrt2 - 1) = 1.2426mm, before the part is cut.
    let p = er_profile_part(ER_L, "run Form(tool=T2, surface=F)");
    let f = parse::deviation_findings(&p);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].message.contains("corner shortfall 1.2426mm"), "{}", f[0]);
    assert!(f[0].message.contains("5 corners"), "the L's five sharp corners, not its reflex one: {}", f[0]);

    // The boss is the control: every corner of a CW square is an OUTSIDE
    // corner, the tool wraps all four, and a nominal tool has nothing to
    // declare. A finding that fired on both shapes would be measuring nothing.
    let pad = er_profile_part(ER_PAD, "run Form(tool=T2, surface=F)");
    assert!(parse::deviation_findings(&pad).is_empty(), "an outside corner is fully broken");
}

#[test]
fn test_the_polyline_round_is_measured_from_the_path_not_the_wall() {
    // The load-bearing claim of slice 2's removal model. A wall-relative band
    // would report a full round in the pocket's sharp corner; the tool never
    // gets there, so that would be over-stating removal — the dangerous
    // direction, because `check_moves` reads this model to decide whether a
    // plunge is into air.
    let p = er_profile_part(ER_L, "run Form(tool=T2, surface=F)");
    let after = nacs::stock::remaining_after(&p, 2);
    assert!(after.complete, "the polyline round accounts for itself — nothing goes contagious");

    // A point on the corner bisector 0.5mm into the material at (10,20). The
    // two models disagree about it by more than a millimetre of depth:
    //   * measured from the WALL it is 0.354mm out, so the round's surface
    //     would sit at -3 + sqrt(9 - (3 - 0.354)^2) = -1.587 and everything
    //     above that is claimed gone;
    //   * measured from the PATH it is 4.743mm out — the axis pivots on
    //     (13,23) and never comes closer — so the surface sits at -0.276 and
    //     the millimetre below it is still standing.
    // z is picked strictly between the two, which is the only place the claim
    // can be tested at all.
    let s = 0.5f64;
    let (x, y) = (10.0 - s / 2f64.sqrt(), 20.0 - s / 2f64.sqrt());
    assert!(
        after.material_at(x, y, -1.0),
        "the corner the tool cannot reach still stands — a wall-relative band would have called this gone"
    );

    // The same round at the middle of a straight run IS cut, so the test above
    // is not passing by the band being empty: 0.5mm out from the x = 10 wall,
    // half way up the leg.
    let z_mid = -3.0 + (9.0f64 - (3.0 - 0.5f64).powi(2)).sqrt() + 0.05;
    assert!(!after.material_at(10.0 - 0.5, 50.0, z_mid), "the straight run gets the full round");
}

#[test]
fn test_the_polyline_band_carries_the_trail_off_and_both_variants_account_for_themselves() {
    // The trail-off carries the same load one shape later: an arc that runs out
    // short removes LESS, and the band's inner rim is where the arc really
    // cuts, not where the wall is. 20deg short of tangency stops the cut
    // R(1-cos20) = 0.181mm out from the flank, so a ridge stands against it.
    let trailed = "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, trail_wall=20.0, source=measured)\n";
    let src = |t: &str| format!("{ER_PREAMBLE}{t}{ER_L}run Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T2, surface=F)\n");
    let nominal = parse::parse(&src(ER_T2)).unwrap();
    let short = parse::parse(&src(trailed)).unwrap();

    // Mid-leg on the x = 10 wall, 0.05mm out and 0.05mm below the nominal
    // round's surface there — inside the ridge the trail-off leaves.
    let (x, y) = (10.0 - 0.05, 50.0);
    let z = -3.0 + (9.0f64 - (3.0 - 0.05f64).powi(2)).sqrt() + 0.05;
    assert!(!nacs::stock::remaining_after(&nominal, 2).material_at(x, y, z), "the full arc takes this");
    assert!(nacs::stock::remaining_after(&short, 2).material_at(x, y, z), "a 20deg trail-off leaves it standing");

    // And the standing variant accounts for itself too — the band is inside the
    // path loop rather than outside it, which is the one bit that flips. When
    // this landed the boss's WALL was unmodeled ("how far out a boss was
    // cleared is a path fact") and the round declared its own removal beside
    // that gap; since Phase 10 lane A the wall has its band and the moat in
    // front of it, so the pad program is complete end to end — moat (op 0),
    // wall (op 1), round (op 2).
    let pad = er_profile_part(ER_PAD, "run Form(tool=T2, surface=F)");
    let incomplete = nacs::stock::incomplete_ops(&pad);
    assert!(incomplete.is_empty(), "every op of a roughed, finished, rounded boss has a removal: {incomplete:?}");

    // 0.5mm inside the x = 70 wall, where the round's surface sits at
    // -3 + sqrt(9 - (3 - 0.5)^2) = -1.342.
    let after = nacs::stock::remaining_after(&pad, 2);
    assert!(!after.material_at(70.5, 50.0, -1.30), "above the round's surface is gone");
    assert!(after.material_at(70.5, 50.0, -1.40), "below it the boss still stands");
}

// ---- The chamfer (DECISIONS.md "the chamfer tool — one tangent surface
// instead of two…", slice 2) — the edge round's conical sibling. ------------

// The self-consistent 90-degree mill from the fixtures: apex 0.25 below the
// tip ((tip_dia/2)/tan45), derived default tip_clearance max(0.5, 0.25) = 0.5.
const CH_T3: &str = "tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n";

// Same ordering rule as er_part: the lip is broken AFTER the feature is
// opened, or the entry restriction rightly rejects the plunge.
fn ch_part(tool: &str, feature: &str, run: &str) -> Result<parse::Part, String> {
    let open = if feature.starts_with("CylinderBore") {
        "run Clear(tool=T5, profile=F, stepover=4.0)\n"
    } else {
        "run Side(tool=T5, surface=F, stepover=5.0)\n"
    };
    parse::parse(&format!("{ER_PREAMBLE}{tool}feature F {feature}\n{open}{run}\n"))
}

#[test]
fn test_chamfer_golden() {
    for (dir, golden_path) in [
        ("chamfer.cam", "tests/chamfer.golden.gcode"),
        ("chamfer_profile.cam", "tests/chamfer_profile.golden.gcode"),
        ("chamfer_marks.cam", "tests/chamfer_marks.golden.gcode"),
    ] {
        let main_src = fs::read_to_string(format!("{dir}/main.part")).expect("failed to read main.part");
        let part = parse::parse(&main_src).expect("failed to parse fixture");
        parse::validate(&part).expect("fixture must validate");
        let generated = gcode::program(&part).expect("failed to generate program");
        let golden = fs::read_to_string(golden_path).expect("failed to read golden");
        assert_eq!(generated.trim(), golden.trim(), "{dir} diverged from its golden");

        // The header either helps the operator or springs the trap: D is the
        // cone diameter at the contact height — tip 0.5 + 2 x (0.6 + 0) for
        // the control op — and the header says WHICH height that diameter
        // lives at, because a D nobody can check against the tool is a number
        // nobody can verify.
        assert!(generated.contains("comp register D3 = 1.700"), "{dir}: D3 must be the contact diameter");
        assert!(generated.contains("cone diameter 0.850 above the virtual apex"), "{dir}: the header names the contact height");
        assert!(generated.contains("not the body OD"), "{dir}: the header must name what D is NOT");
        // One pass per lip: the face is stamped, not approximated.
        assert_eq!(generated.matches("chamfer edge").count(), 2, "{dir}: two lips, one pass each");
    }
}

/// A chamfer wider than the cutter's flank (`chamfer_deep.cam`): T3 holds
/// 5.25 of face above its contact radius, the Pad's bevel is 10 wide and the
/// Post's 8. Each is cut in seven passes. No pass goes below the declared
/// placement's tip, the last pass is that placement, and a pass slid up the
/// face rides in over the boss, carried out past the walls at the bevel's
/// ends.
#[test]
fn test_a_chamfer_wider_than_the_flank_is_cut_in_steps() {
    let main_src = fs::read_to_string("chamfer_deep.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse fixture");
    parse::validate(&part).expect("fixture must validate");
    let generated = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/chamfer_deep.golden.gcode").expect("failed to read golden");
    assert_eq!(generated.trim(), golden.trim(), "chamfer_deep.cam diverged from its golden");

    let pad = generated.split("(--- chamfer edge (engine): Pad with T3 ---)").nth(1).expect("the Pad's chamfer op")
        .split("(--- ").next().unwrap();
    assert_eq!(pad.matches("(pass ").count(), 7, "{pad}");
    let plunges: Vec<f64> = pad.lines()
        .filter_map(|l| l.strip_prefix("G01 Z"))
        .map(|rest| rest.split_whitespace().next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(plunges.len(), 7, "one descent a pass: {pad}");
    // The declared placement: 10 deep, the tip 0.5 below the face's foot.
    assert!(plunges.iter().all(|z| *z >= -10.5 - 1e-9), "no pass below the declared tip: {plunges:?}");
    assert_eq!(*plunges.last().unwrap(), -10.5, "the last pass is the declared placement");
    // The top band of the last layer: slid 4.75 up the face, so the axis is
    // 4.0 inside the wall at x=20, and the run goes on to 0.75 past each end
    // wall (y=20 and y=60), where the unslid pass's offset would end.
    let top = pad.split("(pass 5 of 7").nth(1).unwrap().split("(pass 6").next().unwrap();
    assert!(top.contains("G01 X24.000 Y19.250") && top.contains("G01 X24.000 Y60.750"), "{top}");
}

#[test]
fn test_chamfer_places_the_flank_not_the_flat() {
    // A bore of radius 20, chamfer_depth 1.0, derived tip_clearance 0.5. The
    // flank must pass through the face's bottom corner at r_contact = 0.75
    // off the axis, which pins the tip Z through the apex the flat hides:
    // z_tip = -depth - (z_apex + r_contact/tan45) = -1.0 - 0.5 = -1.5, and
    // the axis ring rides at 20 - 0.75 = 19.25.
    let p = ch_part(CH_T3, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=1.0)",
                    "run Form(tool=T3, surface=F)").unwrap();
    parse::validate(&p).unwrap();
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("G01 Z-1.500"), "the tip Z is solved through the apex: {g}");
    assert!(g.contains("G01 X49.250 Y50.000"), "the axis rides r_contact inside the wall: {g}");

    // Naive tip math — placing the face as if the sharp point were real —
    // would land the tool 0.25 higher (the whole apex offset) and cut half
    // the chamfer. That Z must not appear as the plunge.
    assert!(!g.contains("G01 Z-1.250 F300.000"), "the catalogue's sharp point must not position the pass");
}

#[test]
fn test_chamfer_tip_clearance_slides_along_the_flank() {
    // The degree of freedom, pinned as a STRUCTURE: authoring a deeper
    // tip_clearance moves rho and Z together along the 45-degree flank — the
    // ring shrinks by 0.5 exactly as the tip drops by 0.5 — and the cut face
    // is invariant, which is why the knob can exist at all.
    let bore = "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=1.0)";
    let a = ch_part(CH_T3, bore, "run Form(tool=T3, surface=F)").unwrap();
    let b = ch_part(CH_T3, bore, "run Form(tool=T3, surface=F, tip_clearance=1.0)").unwrap();
    let (ga, gb) = (gcode::program(&a).unwrap(), gcode::program(&b).unwrap());
    assert!(ga.contains("G01 Z-1.500") && ga.contains("X49.250"), "{ga}");
    assert!(gb.contains("G01 Z-2.000") && gb.contains("X48.750"), "{gb}");
}

#[test]
fn test_chamfer_control_comp_emits_the_model_edge_and_folds_the_slide_into_d() {
    // Under control comp the emitted loop is the authored edge and BOTH knobs
    // that move the axis — tip_clearance and wall_standoff — fold into D and
    // never into the path. Same cut loop, different register.
    let boss = "CylinderBoss(dia=40.0, extent=12.0, x=90.0, y=50.0, z=-12.0, chamfer_width=0.8)";
    let plain = ch_part(CH_T3, boss, "run Form(tool=T3, surface=F, comp=control, tip_clearance=0.6)").unwrap();
    let stood = ch_part(CH_T3, boss, "run Form(tool=T3, surface=F, comp=control, tip_clearance=0.6, wall_standoff=0.5)").unwrap();
    let (gp, gs) = (gcode::program(&plain).unwrap(), gcode::program(&stood).unwrap());

    // The model edge, verbatim, in both.
    for g in [&gp, &gs] {
        assert!(g.contains("G01 G41 D3 X110.000 Y50.000"), "the emitted loop is the authored edge: {g}");
    }
    // D carries the slide: tip 0.5 + 2x0.6 = 1.7 plain, + 2x0.5 standoff = 2.7.
    assert!(gp.contains("comp register D3 = 1.700"), "{gp}");
    assert!(gs.contains("comp register D3 = 2.700"), "{gs}");
    // The emitted cut ring is identical: the standoff moved no coordinate.
    let ring = |g: &str| {
        g.lines()
            .skip_while(|l| !l.contains("chamfer edge (control)"))
            .filter(|l| l.starts_with("G0") && l.contains('X'))
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    // The face standoff moves Z, the wall standoff moves nothing — but the
    // placement Z also folds nothing of the wall standoff, so the two column
    // sets match line for line.
    assert_eq!(ring(&gp), ring(&gs), "a wall standoff must move only D");
}

#[test]
fn test_chamfer_one_register_holds_one_number() {
    // Two control passes with one tool asking for two different slides is
    // unsatisfiable: one register holds one number.
    let src = format!(
        "{ER_PREAMBLE}{CH_T3}feature A CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=1.0)\nfeature B CylinderBoss(dia=30.0, extent=12.0, x=90.0, y=50.0, z=-12.0, chamfer_depth=1.0)\nrun Clear(tool=T5, profile=A, stepover=4.0)\nrun Side(tool=T5, surface=B, stepover=5.0)\nrun Form(tool=T3, surface=A, comp=control, tip_clearance=0.6)\nrun Form(tool=T3, surface=B, comp=control, tip_clearance=0.8)\n"
    );
    let err = parse::parse(&src).unwrap_err();
    assert!(err.contains("one register holds one number"), "{err}");
}

#[test]
fn test_chamfer_refusals() {
    let bore = "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=1.0)";
    let cases: Vec<(String, &str)> = vec![
        // The pairing error reads the ANGLE: a form cutter cuts its own
        // angle or the wrong one.
        (format!("{ER_PREAMBLE}{CH_T3}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=1.0, chamfer_angle=30.0)\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F)\n"),
         "cuts its own angle or the wrong one"),
        // Two legs derive the angle since Phase 46 slice 3 (pick two); three
        // words that disagree are rejected at parse, naming all three.
        (format!("{ER_PREAMBLE}{CH_T3}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=1.0, chamfer_width=1.0, chamfer_angle=60)\n"),
         "the three words disagree"),
        // An angle with no leg dimensions nothing.
        (format!("{ER_PREAMBLE}{CH_T3}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_angle=45.0)\n"),
         "an angle with no leg dimensions nothing"),
        // One top edge, one declared break.
        (format!("{ER_PREAMBLE}{CH_T3}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, edge_round=3.0, chamfer_depth=1.0)\n"),
         "rounded or beveled, not both"),
        // The leg consumes the wall.
        (format!("{ER_PREAMBLE}{CH_T3}feature F CylinderBore(dia=40.0, extent=1.0, x=30.0, y=50.0, chamfer_depth=1.5)\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F)\n"),
         "deeper than the"),
        // The face runs off the usable flank into the body: cut in steps,
        // which control comp's one register cannot hold.
        (format!("{ER_PREAMBLE}{CH_T3}feature F CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=5.5)\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F, comp=control)\n"),
         "runs past T3's usable flank"),
        // The axis must fit down the hole.
        (format!("{ER_PREAMBLE}{CH_T3}feature F CylinderBore(dia=1.2, extent=12.0, x=30.0, y=50.0, chamfer_depth=0.1)\nrun Form(tool=T3, surface=F)\n"),
         "outside the hole it is cutting in"),
        // Zero IS the rejected deepest-legal placement.
        (format!("{ER_PREAMBLE}{CH_T3}feature F {bore}\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F, tip_clearance=0.0)\n"),
         "must be positive"),
        // Nothing to space, nothing to ladder, no second home for an offset.
        (format!("{ER_PREAMBLE}{CH_T3}feature F {bore}\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F, stepover=2.0)\n"),
         "takes no stepover"),
        (format!("{ER_PREAMBLE}{CH_T3}feature F {bore}\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F, scallop=0.1)\n"),
         "takes no scallop"),
        (format!("{ER_PREAMBLE}{CH_T3}feature F {bore}\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F, stock_to_leave=0.3)\n"),
         "takes no stock_to_leave"),
        (format!("{ER_PREAMBLE}{CH_T3}feature F {bore}\nrun Clear(tool=T5, profile=F, stepover=4.0)\nrun Form(tool=T3, surface=F, wall_standoff=-0.1)\n"),
         "must not be negative"),
        // tip_clearance read nowhere else: rejected where unread.
        (format!("{ER_PREAMBLE}{CH_T3}feature F {bore}\nrun Clear(tool=T5, profile=F, stepover=4.0, tip_clearance=0.5)\n"),
         "only a Form with a ChamferMill reads it"),
        // A curved surface has no top edge of its own to break.
        (format!("{ER_PREAMBLE}{CH_T3}feature F Sphere(dia=30.0, x=60.0, y=50.0, z=0.0)\nrun Form(tool=T3, surface=F)\n"),
         "only the prismatic pairs"),
    ];
    for (src, needle) in cases {
        let err = parse::parse(&src)
            .and_then(|p| parse::validate(&p).map(|_| p))
            .err()
            .unwrap_or_else(|| panic!("expected an error containing '{needle}'"));
        assert!(err.contains(needle), "wanted '{needle}', got: {err}");
    }
}

#[test]
fn test_the_bevel_a_sharp_corner_never_gets_is_a_number() {
    // The corner story transfers from the round, and it is worse here: an
    // unbroken corner on a beveled edge is more visible. The L-pocket's five
    // sharp corners sit 0.75(sqrt2 - 1) = 0.3107mm past the axis path's reach.
    let main_src = fs::read_to_string("chamfer_profile.cam/main.part").unwrap();
    let part = parse::parse(&main_src).unwrap();
    let findings = parse::deviation_findings(&part);
    let corner = findings.iter().find(|w| w.message.contains("corner shortfall")).expect("the shortfall is a finding");
    assert!(corner.message.contains("0.3107mm"), "{corner}");
    assert!(corner.message.contains("5 corner"), "{corner}");
    // The boss is the control: every corner is an outside corner the tool
    // wraps, so the control pass reports nothing.
    assert_eq!(findings.len(), 1, "only the pocket reports: {findings:?}");

    // An angle error TILTS the face, and the budget prices the tilt at the
    // face corner: depth x |tan(45.5deg) - tan(45deg)| = 0.0176mm.
    let tilted = CH_T3.replace("tip_dia=0.5", "tip_dia=0.5, angle_error=1.0, source=measured");
    let p = ch_part(&tilted, "CylinderBore(dia=40.0, extent=12.0, x=30.0, y=50.0, chamfer_depth=1.0)",
                    "run Form(tool=T3, surface=F)").unwrap();
    let findings = parse::deviation_findings(&p);
    assert!(findings.iter().any(|w| w.message.contains("face tilt 0.0176mm") && w.message.contains("measured")), "{findings:?}");

    // And the gate is the same computation with a second voice.
    let gated = format!("deviation gate\n{main_src}");
    let err = parse::parse(&gated).unwrap_err();
    assert!(err.contains("deviation gate: ") && err.contains("corner shortfall"), "{err}");
}

#[test]
fn test_the_polyline_bevel_is_measured_from_the_path_not_the_wall() {
    // The removal is a distance field to the axis PATH: at a sharp corner the
    // path pivots on its offset vertex, so the bevel runs out and the model
    // reports the corner region as STANDING where a wall-relative band would
    // claim it gone — the dangerous direction, because check_moves reads this.
    let main_src = fs::read_to_string("chamfer_profile.cam/main.part").unwrap();
    let part = parse::parse(&main_src).unwrap();
    // Op 4 since Phase 10 lane A: the Pad's moat `Clear` sits two ops earlier.
    let after = nacs::stock::remaining_after(&part, 4);

    // On the bisector 0.15mm past the corner (10,20), into the material. The
    // path model puts the face's surface at z = -0.539 there; a wall-relative
    // model would put it at -0.894. z = -0.7 sits strictly between, so the
    // claim is testable at all: standing under the path model, gone under the
    // wall-relative one.
    let (x, y) = (10.0 - 0.15 / 2.0f64.sqrt(), 20.0 - 0.15 / 2.0f64.sqrt());
    assert!(after.material_at(x, y, -0.7), "the corner the tool cannot reach still stands");

    // Mid-run the band claims exactly the face: at (30, 19.5) the path sits
    // 1.25 away, the face's surface at z = -0.5.
    assert!(!after.material_at(30.0, 19.5, -0.2), "above the face is gone");
    assert!(after.material_at(30.0, 19.5, -0.8), "below the face still stands");
}

// ---- spot drilling and countersinking (DECISIONS.md → "spot drilling and
// countersinking…"): the same ground cone on a hole's mouth, cut by Drill ----

#[test]
fn test_csink_golden() {
    let main_src = fs::read_to_string("csink.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse fixture");
    parse::validate(&part).expect("fixture must validate");
    let generated = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/csink.golden.gcode").expect("failed to read golden");
    assert_eq!(generated.trim(), golden.trim(), "csink.cam diverged from its golden");

    // The depth is derived through the apex the relieved flat hides below the
    // tool: seat Z-5.250 and spot Z-1.250 on T3's 0.25mm apex offset. The
    // shop-floor sharp-point derivation — seat_r/tan from the tip, the flat
    // forgotten — would plunge to -5.500 / -1.500; those Z's must not exist.
    assert!(generated.contains("G01 Z-5.250 F300.000"), "the csink plunges to the derived seat: {generated}");
    assert!(generated.contains("G01 Z-1.250 F300.000"), "the spot plunges to the derived dimple: {generated}");
    assert!(!generated.contains("Z-5.500"), "the catalogue's sharp point must not position the seat");
    assert!(!generated.contains("Z-1.500"), "the catalogue's sharp point must not position the dimple");
    // The header names the job and the declared seat, so the operator can
    // check the plunge against the print's callout.
    assert!(generated.contains("Hole1 (csink 11.000)"), "{generated}");
    assert!(generated.contains("Hole2 (spot 3.000)"), "{generated}");
}

#[test]
fn test_csink_depth_reads_the_actual_angle() {
    // The pairing error reads the NOMINAL angle; the math reads the ACTUAL
    // one — the identity/measurement split. A 90-degree mill ground 2 degrees
    // over cuts a 92-degree cone, and the dimple's Z must move with it:
    // independently, z_apex = 3.75 - 4/tan46 and Z = -(z_apex + 1.5/tan46).
    let tool = "tool T3 ChamferMill(angle=90.0, angle_error=2.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5, source=measured)\n";
    let src = format!(
        "{ER_PREAMBLE}{tool}feature H CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, spot_dia=3.0)\nrun Form(points=[H], tool=T3, cycle=Simple)\n"
    );
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).unwrap();
    let tan = (46.0f64).to_radians().tan();
    let z = -((3.75 - 4.0 / tan) + 1.5 / tan);
    assert!(g.contains(&format!("G01 Z{} F300.000", gcode::f(z))), "the actual cone derives the depth: {g}");
    assert!(!g.contains("G01 Z-1.250 F300.000"), "the nominal cone must not position the pass");
}

#[test]
fn test_csink_peck_reaches_the_derived_seat() {
    // The mouth cone rides the same drill cycles the holes ride — Peck
    // included: the increments walk down, the final peck lands on the derived
    // Z, never past it.
    let src = format!(
        "{ER_PREAMBLE}{CH_T3}feature H CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, spot_dia=3.0)\nrun Form(points=[H], tool=T3, cycle=Peck, peck_increment=0.5)\n"
    );
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("G01 Z-0.500 F300.000"), "{g}");
    assert!(g.contains("G01 Z-1.000 F300.000"), "{g}");
    assert!(g.contains("G01 Z-1.250 F300.000"), "{g}");
    assert!(!g.contains("Z-1.500"), "{g}");
}

#[test]
fn test_csink_removal_is_the_swept_frustum() {
    // A spot into solid stock: after the op the frustum the plunge sweeps is
    // void — the relieved flat's floor included, the drill's chisel-web
    // idealization one form over — and the flank line is the boundary.
    // Samples [(0.25, -1.25), (1.5, 0)]: at rho = 0.875 the line gives
    // z = -0.625, and the end clamp gives the flat's -1.25 on the axis.
    let src = format!(
        "{ER_PREAMBLE}{CH_T3}feature H CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, spot_dia=3.0)\nrun Form(points=[H], tool=T3, cycle=Simple)\n"
    );
    let p = parse::parse(&src).unwrap();
    let after = nacs::stock::remaining_after(&p, 0);
    assert!(after.complete, "the dimple has a closed form");
    assert!(!after.material_at(30.0, 40.0, -1.2), "above the flat's floor is gone");
    assert!(after.material_at(30.0, 40.0, -1.3), "below the flat's floor still stands");
    assert!(!after.material_at(30.875, 40.0, -0.6), "above the flank is gone");
    assert!(after.material_at(30.875, 40.0, -0.65), "below the flank still stands");
    assert!(after.material_at(31.6, 40.0, -0.01), "outside the rim the top face stands");
}

#[test]
fn test_csink_refusals() {
    let hole = "CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0";
    let run = "run Form(points=[H], tool=T3, cycle=Simple)\n";
    let cases: Vec<(String, &str)> = vec![
        // One mouth, one declared cone: a seat or a dimple, never both.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_dia=11.0, spot_dia=3.0)\n"),
         "one mouth carries one declared cone"),
        // An angle with no seat dimensions nothing.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_angle=90.0)\n"),
         "an angle with no seat dimensions nothing"),
        // The mouth cone IS the edge's break — a second break describes the
        // same edge twice, in either vocabulary.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_dia=11.0, chamfer_depth=1.0)\n"),
         "describes the same edge twice"),
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_dia=11.0, edge_round=1.0)\n"),
         "describes the same edge twice"),
        // A seat no wider than its hole leaves no seat.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_dia=6.0)\n"),
         "no seat remains"),
        // A dimple as wide as its hole survives the drill: that is a
        // countersink misdeclared.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, spot_dia=6.0)\n"),
         "declare csink_dia"),
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_dia=-1.0)\n"),
         "must be positive"),
        // A ChamferMill in a Drill cuts a DECLARED seat.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole})\n{run}"),
         "declares no mouth cone"),
        // The pairing error reads the angle — nominal, exactly.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_dia=11.0, csink_angle=82.0)\n{run}"),
         "cuts its own angle or the wrong one"),
        // The seat must stay on the usable flank, not run out into shank.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, csink_dia=13.0)\n{run}"),
         "wider than the usable flank"),
        // A countersink's relieved flat hangs in the hole's void — or rejects.
        (format!("{ER_PREAMBLE}{CH_T3}feature H CylinderBore(dia=0.4, extent=15.0, x=30.0, y=40.0, csink_dia=2.0)\n{run}"),
         "does not fit down hole"),
        // A dimple that never outgrows the flat is cut by nothing.
        (format!("{ER_PREAMBLE}{CH_T3}feature H {hole}, spot_dia=0.4)\n{run}"),
         "never outgrows the flat"),
        // A corner rounder's tip is a relieved pilot: it drills nothing.
        (format!("{ER_PREAMBLE}{ER_T2}feature H {hole})\nrun Form(points=[H], tool=T2, cycle=Simple)\n"),
         "it drills nothing"),
    ];
    for (src, needle) in cases {
        let err = parse::parse(&src)
            .and_then(|p| parse::validate(&p).map(|_| p))
            .err()
            .unwrap_or_else(|| panic!("expected an error containing '{needle}'"));
        assert!(err.contains(needle), "wanted '{needle}', got: {err}");
    }
}

// ---- the counterbore (DECISIONS.md → "the counterbore — a cylindrical seat,
// and the tool's diameter becomes the pairing law"): a flat-floored seat on a
// hole's mouth, cut by Drill with an Endmill ----

const CB_T4: &str = "tool T4 Endmill(dia=16.0)\n";

#[test]
fn test_cbore_golden() {
    let main_src = fs::read_to_string("cbore.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse fixture");
    parse::validate(&part).expect("fixture must validate");
    let generated = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/cbore.golden.gcode").expect("failed to read golden");
    assert_eq!(generated.trim(), golden.trim(), "cbore.cam diverged from its golden");

    // The depth is DECLARED — a cylinder has no apex to derive through — so
    // the seat plunges to exactly the print's number below the top: the
    // cap-screw seat to -6.0, the spotface (the same two fields at a smaller
    // depth) to -0.8. The holes themselves still go to the extent's -15.0.
    assert!(generated.contains("G01 Z-6.000 F300.000"), "the seat plunges to the declared depth: {generated}");
    assert!(generated.contains("G01 Z-0.800 F300.000"), "the spotface plunges to the declared depth: {generated}");
    assert!(generated.contains("G01 Z-15.000 F300.000"), "the drill still opens the holes to the extent: {generated}");
    // The header names the seat per point, so the operator can check the
    // plunge against the print's callout.
    assert!(generated.contains("Hole1 (cbore 16.000)"), "{generated}");
    assert!(generated.contains("Hole2 (cbore 16.000)"), "{generated}");
    // The drill's own pass names no seat — it is not the seat's tool.
    assert!(generated.contains("(--- form points: Hole1, Hole2 with T7 ---)"), "{generated}");
}

#[test]
fn test_cbore_peck_reaches_the_declared_seat() {
    // The seat rides the same drill cycles the holes ride — Peck included:
    // the increments walk down, the final peck lands on the declared depth,
    // never past it.
    let src = format!(
        "{ER_PREAMBLE}{CB_T4}feature H CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, cbore_dia=16.0, cbore_depth=1.2)\nrun End(points=[H], tool=T4, cycle=Peck, peck_increment=0.5)\n"
    );
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("G01 Z-0.500 F300.000"), "{g}");
    assert!(g.contains("G01 Z-1.000 F300.000"), "{g}");
    assert!(g.contains("G01 Z-1.200 F300.000"), "{g}");
    assert!(!g.contains("Z-15.000"), "the seat's tool never travels the hole's extent: {g}");
}

#[test]
fn test_cbore_removal_is_the_flat_seat() {
    // The flat claim is TRUE here — the endmill really does cut a flat floor
    // — so the removal is one disc to one Z: void above the declared depth
    // across the seat's radius, material below it, the rim's top face
    // standing just outside.
    let src = format!(
        "{ER_PREAMBLE}{CB_T4}feature H CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, cbore_dia=16.0, cbore_depth=6.0)\nrun End(points=[H], tool=T4, cycle=Simple)\n"
    );
    let p = parse::parse(&src).unwrap();
    let after = nacs::stock::remaining_after(&p, 0);
    assert!(after.complete, "the seat has a closed form");
    assert!(!after.material_at(30.0, 40.0, -5.9), "above the seat floor is gone");
    assert!(after.material_at(30.0, 40.0, -6.1), "below the seat floor still stands");
    assert!(!after.material_at(37.9, 40.0, -0.1), "the seat reaches its declared radius");
    assert!(after.material_at(38.1, 40.0, -0.01), "outside the rim the top face stands");
}

#[test]
fn test_cbore_refusals() {
    let hole = "CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0";
    let run = "run End(points=[H], tool=T4, cycle=Simple)\n";
    let cases: Vec<(String, &str)> = vec![
        // The paired-fields rule — the dangling-angle rule with two limbs: a
        // cylinder derives nothing, so either field alone dimensions nothing.
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=16.0)\n"),
         "cbore_dia without cbore_depth"),
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_depth=6.0)\n"),
         "cbore_depth without cbore_dia"),
        // One mouth, one declared treatment — the one-edge-one-break law's
        // third instance, across the whole mouth vocabulary.
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=16.0, cbore_depth=6.0, csink_dia=11.0)\n"),
         "describes the same edge twice"),
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=16.0, cbore_depth=6.0, spot_dia=3.0)\n"),
         "describes the same edge twice"),
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=16.0, cbore_depth=6.0, chamfer_depth=1.0)\n"),
         "describes the same edge twice"),
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=16.0, cbore_depth=6.0, edge_round=1.0)\n"),
         "describes the same edge twice"),
        // A seat no wider than its hole leaves no seat — the csink's law,
        // cylindrical.
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=6.0, cbore_depth=6.0)\n"),
         "no seat remains"),
        // A seat as deep as its hole consumes it.
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=16.0, cbore_depth=15.0)\n"),
         "consumes it"),
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=-16.0, cbore_depth=6.0)\n"),
         "must be positive"),
        // The pairing law: a plunged seat is exactly the tool's own circle,
        // so a dia the tool does not have rejects — naming the orbited-seat
        // fork the road not taken.
        (format!("{ER_PREAMBLE}{CB_T4}feature H {hole}, cbore_dia=20.0, cbore_depth=6.0)\n{run}"),
         "orbited seat"),
        (format!("{ER_PREAMBLE}feature H {hole}, cbore_dia=12.0, cbore_depth=6.0)\nrun End(points=[H], tool=T5, cycle=Simple)\n"),
         "the seat's diameter IS the endmill's"),
    ];
    for (src, needle) in cases {
        let err = parse::parse(&src)
            .and_then(|p| parse::validate(&p).map(|_| p))
            .err()
            .unwrap_or_else(|| panic!("expected an error containing '{needle}'"));
        assert!(err.contains(needle), "wanted '{needle}', got: {err}");
    }
}

#[test]
fn test_cbore_hole_still_drills_with_a_drill() {
    // A declared seat does not capture the hole: a twist drill visiting the
    // same feature still drills the hole to its extent — the shop order is
    // drill first, then seat, and each tool reads its own arm.
    let src = format!(
        "{ER_PREAMBLE}tool T7 Drill(dia=6.0)\nfeature H CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, cbore_dia=16.0, cbore_depth=6.0)\nrun Form(points=[H], tool=T7, cycle=Simple)\n"
    );
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("G01 Z-15.000 F300.000"), "the drill travels the extent: {g}");
    assert!(!g.contains("Z-6.000"), "the declared seat is not the drill's business: {g}");
    assert!(g.contains("(--- form points: H with T7 ---)"), "no seat named on the drill's pass: {g}");
}

#[test]
fn test_a_shaped_tool_cannot_drill() {
    // A custom profile has no single tip diameter a drill plunge could mean.
    let svg_path = "test_csink_drill_profile.svg";
    write_sphere_form_tool_svg(svg_path, "M 0,0 L 1,1 L 1,2");
    let main_src = form_tool_main_src(
        svg_path,
        "feature H CylinderBore(dia=12.0, extent=10.0, x=30.0, y=20.0)\nrun Form(points=[H], tool=T1, cycle=Simple)",
    );
    let result = parse::parse_project(&main_src, None);
    let _ = fs::remove_file(svg_path);
    let err = result.err().expect("a shaped tool must reject to drill");
    assert!(err.contains("no single tip diameter"), "{err}");
}

// ---- the containment lint ---------------------------------------------------
//
// A declared feature reaching outside the stock (`stock::containment_findings`,
// voiced through `parse::lint`). Advisory by construction — DECISIONS.md
// "Phase 6, slice 2…" settled that containment cannot be a gate — so these pins
// are about what it SAYS, and about the two ways an advisory goes wrong: crying
// wolf on geometry that is flush with a stock face by construction, and staying
// silent on a real overhang.

/// The containment findings alone, filtered out of the shared advisory voice.
fn containment_warnings(part: &parse::Part) -> Vec<String> {
    parse::lint(part).into_iter().filter(|w| w.message.contains("stands outside the stock")).map(|w| w.message).collect()
}

#[test]
fn test_containment_lint_catches_a_dome_above_the_blank() {
    // The headline case, and the reason the lint exists — now HISTORICAL: for
    // two months every convex-revolve fixture put its feature datum AT the
    // stock top, so the whole dome stood in air, and this lint is what found
    // all 31 of them. The corpus was contained in the rim-area thread's slice
    // 2, so the too-short authoring is reconstructed here: sphere.cam's stock
    // shortened back to its old line, the Clear dropped (it rejects a stock
    // with nothing standing over the base — its own, different, message).
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let short = src
        .replace("stock Box(w=100, l=80, h=47, x=0, y=0, z=22)", "stock Box(w=100, l=80, h=25, x=0, y=0, z=0)")
        .replace("rapid_z 25", "rapid_z 5")
        .replace("clearance_z 40", "clearance_z 30")
        .lines()
        .filter(|l| !l.starts_with("run Clear"))
        .collect::<Vec<_>>()
        .join("\n");
    let part = parse::parse(&short).expect("parse");
    let found = containment_warnings(&part);
    assert_eq!(found.len(), 1, "one feature, one finding: {:?}", found);
    assert!(found[0].contains("'TopDome' (Sphere)"), "names the feature and its kind: {}", found[0]);
    assert!(
        found[0].contains("20.000mm above the stock top z 0.000"),
        "states the overhang as a number: {}",
        found[0]
    );

    // The fix the message asks for is what the fixture now IS: a stock that
    // contains the part. Nothing about the feature differs — only the material
    // it is declared to come out of.
    let part = parse::parse(&src).expect("parse");
    assert!(containment_warnings(&part).is_empty(), "a stock that contains the dome is quiet");
}

#[test]
fn test_containment_lint_is_quiet_where_geometry_is_flush_by_construction() {
    // The cry-wolf direction. Every top-down cavity has its mouth ring exactly
    // ON `stock.z` (`derive_contour_top` copies it) and a `Plane`'s wire is
    // drawn across the stock's own XY faces, so both of these sit on a
    // stock face at exact equality and neither is outside it.
    for fixture in ["cylinder_bore.cam", "plate.cam"] {
        let path = format!("{}/main.part", fixture);
        let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path, e));
        let geom = fs::read_to_string(format!("{}/geometry.part", fixture)).ok();
        let part = parse::parse_project(&src, geom.as_deref()).expect("parse");
        assert!(
            containment_warnings(&part).is_empty(),
            "{} is flush with the stock, not outside it: {:?}",
            fixture,
            containment_warnings(&part)
        );
    }
}

#[test]
fn test_containment_lint_names_every_face_the_feature_leaves() {
    // "Raise the stock" and "widen the stock" are different fixes, so one
    // feature's finding carries every face it leaves — worst overhang first.
    // Reconstructed on the too-short authoring (the fixture itself is
    // contained now): stock shortened back, Clear dropped, dome slid to the
    // +x edge.
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let off_edge = src
        .replace("stock Box(w=100, l=80, h=47, x=0, y=0, z=22)", "stock Box(w=100, l=80, h=25, x=0, y=0, z=0)")
        .replace("rapid_z 25", "rapid_z 5")
        .replace("clearance_z 40", "clearance_z 30")
        .lines()
        .filter(|l| !l.starts_with("run Clear"))
        .collect::<Vec<_>>()
        .join("\n")
        .replace("Sphere(dia=40, x=50, y=40, z=0)", "Sphere(dia=40, x=95, y=40, z=0)");
    let part = parse::parse(&off_edge).expect("parse");
    let found = containment_warnings(&part);
    assert_eq!(found.len(), 1, "still one feature: {:?}", found);
    assert!(found[0].contains("20.000mm above the stock top z 0.000"), "{}", found[0]);
    assert!(found[0].contains("past the stock's +x face at x 100.000"), "{}", found[0]);
    let z_at = found[0].find("above the stock top").expect("z clause");
    let x_at = found[0].find("past the stock's +x face").expect("x clause");
    assert!(z_at < x_at, "the worse overhang leads: {}", found[0]);

    // The certificate is an INNER bound built from points evaluated on the
    // feature, so the reported x overhang is the flattened circle's, which is
    // inscribed and therefore never larger than the true 15.000mm.
    let over: f64 = found[0]
        .split("mm past the stock's +x face")
        .next()
        .and_then(|s| s.rsplit(", ").next())
        .and_then(|s| s.parse().ok())
        .expect("the x overhang parses");
    assert!(over > 14.9 && over <= 15.0, "an inner bound, tight but never over-claimed: {}", over);
}

#[test]
fn test_containment_lint_reads_the_blank_bottom_too() {
    // The other vertical direction, and a real authoring mistake rather than a
    // synthetic one: a bore declared deeper than the stock is thick.
    let src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read cylinder_bore.cam");
    let deep = src.replace("extent=12.0", "extent=26.0");
    let part = parse::parse(&deep).expect("parse");
    let found = containment_warnings(&part);
    assert_eq!(found.len(), 1, "{:?}", found);
    assert!(
        found[0].contains("6.000mm below the stock's bottom z -20.000"),
        "the through-hole overshoot, as a number: {}",
        found[0]
    );
}

#[test]
fn test_containment_lint_says_nothing_about_a_blank_with_no_volume() {
    // Every feature would trip, and not one of those findings would be about
    // geometry — it would be about a project that has not declared its stock.
    // A zero HEIGHT is the reachable form of that: `validate` already rejects a
    // non-positive w or l, so those never get this far.
    let src = fs::read_to_string("sphere.cam/main.part").expect("read sphere.cam");
    let no_stock = src.replace("stock Box(w=100, l=80, h=25, x=0, y=0, z=0)", "stock Box(w=100, l=80, h=0, x=0, y=0, z=0)");
    let part = parse::parse(&no_stock).expect("parse");
    assert!(containment_warnings(&part).is_empty(), "no stock declared, no containment claim");
}

// ---- the revolve-boss Clear's level stack (DECISIONS.md → "roughing a convex
// revolve…", slice 0). Geometry only: these pin what the op will stand off,
// before any op exists to stand off it.

fn feature_of(fixture: &str, name: &str) -> parse::Feature {
    let src = fs::read_to_string(format!("{}/main.part", fixture)).expect("read fixture");
    // Through the base-dir door, so a fixture whose meridian lives in a
    // sidecar file (the spline revolves) reaches it the same way its own
    // golden test does.
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new(fixture)))
        .expect("parse");
    part.features.get(name).expect("feature").clone()
}

#[test]
fn test_revolve_clear_levels_stand_off_the_sphere_at_its_closed_form() {
    // sphere.cam: dia=40 (r=20), equator at z=0, so the dome's own silhouette
    // at height z is the circle sqrt(r² − z²) — the closed form this stack has
    // to agree with.
    //
    // Read ONE-SIDED, in the measured direction. rho(z) is concave, so a chord
    // of the sampled meridian lies inside the arc and the island always comes
    // out at or under the true radius, never over: swept across 199 stepdowns
    // the worst understatement is 9.3e-4mm, an order below the project's own
    // CHORD_TOL_MM (0.01mm) and three below the allowance it sits inside. The
    // pin is written this way so an overshoot — the direction that would leave
    // the finish pass a ridge it did not budget for — fails loudly.
    let dome = feature_of("sphere.cam", "TopDome");
    let levels = parse::derive_revolve_clear_stacks(&dome, 20.0, 4.0, 0.0).expect("a cap stack").moat;
    assert_eq!(levels.len(), 5, "20mm of span in 4mm bites: {:?}", levels);
    for lv in &levels {
        let exact = (400.0 - lv.z * lv.z).sqrt();
        assert!(
            lv.island_r <= exact + 1e-12 && lv.island_r > exact - 1e-3,
            "level z={:.3} island {:.6} vs closed form {:.6}",
            lv.z, lv.island_r, exact
        );
    }
    // The equator is a meridian sample, not a bracket, so the bottom level is
    // the closed form exactly — and it is the same `dia/2 + skin` that
    // `Floor::Dome` reports the removal with.
    let bottom = levels.last().unwrap();
    assert!((bottom.z - 0.0).abs() < 1e-12, "the stack lands ON the base plane: {:?}", bottom);
    assert!((bottom.island_r - 20.0).abs() < 1e-12, "the equator is r exactly: {:?}", bottom);
}

#[test]
fn test_revolve_clear_stepdown_is_a_maximum_and_the_bites_are_equal() {
    // 20mm of span with a 6mm stepdown is FOUR 5mm cuts, not three 6s and a 2.
    // `stepdown` reads as a ceiling the way `stock_to_leave` and `scallop` do,
    // and no pass is left a sliver (DECISIONS.md, the strategy note on the
    // derive).
    let dome = feature_of("sphere.cam", "TopDome");
    let levels = parse::derive_revolve_clear_stacks(&dome, 20.0, 6.0, 0.0).expect("a cap stack").moat;
    let zs: Vec<f64> = levels.iter().map(|l| l.z).collect();
    assert_eq!(zs.len(), 4, "ceil(20/6) = 4 cuts: {:?}", zs);
    for (i, z) in zs.iter().enumerate() {
        let want = 20.0 - 5.0 * (i as f64 + 1.0);
        assert!((z - want).abs() < 1e-12, "equal 5mm bites: {:?}", zs);
    }
}

#[test]
fn test_revolve_clear_island_on_a_cone_is_a_straight_line_in_z() {
    // cone.cam: dia=40, height=20, base z=0. The nominal silhouette is the
    // straight line a(1 − z/h), and a straight meridian has no chord error at
    // all — so this one is exact at every level, bracket or not.
    let cone = feature_of("cone.cam", "Point");
    let levels = parse::derive_revolve_clear_stacks(&cone, 20.0, 2.5, 0.0).expect("a cap stack").moat;
    assert_eq!(levels.len(), 8);
    for lv in &levels {
        let exact = 20.0 * (1.0 - lv.z / 20.0);
        assert!(
            (lv.island_r - exact).abs() < 1e-9,
            "level z={:.3} island {:.6} vs the line {:.6}",
            lv.z, lv.island_r, exact
        );
    }
}

#[test]
fn test_revolve_clear_floor_takes_a_radial_allowance_not_a_normal_one() {
    // The bottom level sits ON the base plane, where the declared surface has
    // ENDED — the part's boundary there is the rim circle and nothing else. So
    // the allowance is radial (rim + skin), matching `Clear`'s standing law
    // that the wall takes the allowance and the floor is the feature's own
    // datum. On a 45° cone the NORMAL offset at the rim leans upward and would
    // read 20.354, which is 0.15mm INSIDE the material the finish pass is owed.
    let cone = feature_of("cone.cam", "Point");
    let levels = parse::derive_revolve_clear_stacks(&cone, 20.0, 5.0, 0.5).expect("a cap stack").moat;
    let bottom = levels.last().unwrap();
    assert!((bottom.z - 0.0).abs() < 1e-12);
    assert!(
        (bottom.island_r - 20.5).abs() < 1e-9,
        "rim 20 + 0.5mm radial, not the normal offset's 20.354: {:?}",
        bottom
    );
    // Above the rim the allowance is the NORMAL offset again. A normal offset
    // of `skin` off a flank shows up in PLAN as skin·l/h of radius — 0.707 on
    // this 45° cone, not 0.5 — because the offset surface is displaced sideways
    // and downward at once. Getting this wrong in the other direction is how a
    // roughing pass quietly eats a flank allowance.
    let l = (800.0f64).sqrt();
    let mid = levels.iter().find(|lv| (lv.z - 10.0).abs() < 1e-9).expect("a level at z=10");
    assert!(
        (mid.island_r - (10.0 + 0.5 * l / 20.0)).abs() < 1e-9,
        "normal offset off the flank reads {:.6} in plan, want {:.6}: {:?}",
        mid.island_r, 10.0 + 0.5 * l / 20.0, mid
    );
}

#[test]
fn test_the_torus_crown_splits_into_two_branches_each_at_its_closed_form() {
    // What the old error called unsayable, said. torus.cam: R 25, minor r 4,
    // base z 0, so at height z the OFFSET body's cross-section is the annulus
    // between 25 ∓ sqrt((4+skin)² − z²) — one closed form, read in the mirror
    // on each side of the crown. The moat's island is the outer branch; the
    // hole's ceiling is the inner one.
    let torus = feature_of("torus.cam", "T");
    for skin in [0.0f64, 0.5] {
        let r_off = 4.0 + skin;
        let s = parse::derive_revolve_clear_stacks(&torus, 6.5, 0.25, skin).expect("both stacks");
        assert!(!s.hole.is_empty(), "a crown opens a hole under it");
        for lv in s.moat.iter().filter(|l| l.z < r_off - 1e-9) {
            let exact = 25.0 + (r_off * r_off - lv.z * lv.z).sqrt();
            // The sphere's direction, for the sphere's reason: rho(z) is
            // concave on this branch, so a chord of the sampled meridian lies
            // inside the arc and the island comes out at or UNDER the true
            // radius, never over. Worst measured understatement 2.2e-4mm.
            assert!(
                lv.island_r <= exact + 1e-12 && lv.island_r > exact - 1e-3,
                "moat z={:.3} island {:.6} vs closed form {:.6}",
                lv.z, lv.island_r, exact
            );
        }
        for lv in &s.hole {
            let exact = 25.0 - (r_off * r_off - lv.z * lv.z).sqrt();
            // And the mirror puts the chord error on the other side: the inner
            // branch is convex, so the ceiling reads at or OVER the closed form
            // by the same 2.2e-4mm. Both are a fifth of a micron against an
            // allowance three orders larger, and both are the same table the
            // removal model reads, which is what the derive's interpolation
            // note is really protecting.
            assert!(
                lv.ceiling_r >= exact - 1e-12 && lv.ceiling_r < exact + 1e-3,
                "hole z={:.3} ceiling {:.6} vs closed form {:.6}",
                lv.z, lv.ceiling_r, exact
            );
        }
        // Both stacks land ON the base plane, where the two branches are the
        // equators themselves: 25 ± (4 + skin), exactly.
        let (bm, bh) = (s.moat.last().unwrap(), s.hole.last().unwrap());
        assert!((bm.z).abs() < 1e-12 && (bh.z).abs() < 1e-12, "{:?} {:?}", bm, bh);
        assert!((bm.island_r - (25.0 + r_off)).abs() < 1e-12, "{:?}", bm);
        assert!((bh.ceiling_r - (25.0 - r_off)).abs() < 1e-12, "{:?}", bh);
    }
}

#[test]
fn test_the_level_stack_breaks_at_the_crown_so_no_ring_stands_over_the_torus() {
    // The apex-break pin's torus sibling, and the defect it forecloses is the
    // same one: a run that straddled the crown would leave a RING of scrap
    // standing over the part inside the topmost island, which no later pass can
    // reach around. The break puts a level exactly at the crown, and above it
    // the moat reaches all the way in — hole and moat are one annulus up there,
    // so the island the PART declares is nothing at all.
    let torus = feature_of("torus.cam", "T"); // crown at z 4 nominal, 4.5 at skin 0.5
    let s = parse::derive_revolve_clear_stacks(&torus, 6.5, 0.75, 0.5).expect("both stacks");
    assert!(
        s.moat.iter().any(|l| (l.z - 4.5).abs() < 1e-9),
        "a level lands ON the crown: {:?}",
        s.moat.iter().map(|l| l.z).collect::<Vec<_>>()
    );
    for l in s.moat.iter().filter(|l| l.z >= 4.5 - 1e-9) {
        assert!(l.island_r.abs() < 1e-9, "nothing of the part stands at or above the crown: {:?}", l);
    }
    // Nothing straddles it from the other side either: the hole exists only
    // BELOW the crown, and its levels are the moat's own — one break, one
    // division, so a level is a single trip down for both work regions.
    assert!(s.hole.iter().all(|l| l.z < 4.5 - 1e-9), "no hole at or above the crown: {:?}", s.hole);
    let below: Vec<f64> = s.moat.iter().filter(|l| l.z < 4.5 - 1e-9).map(|l| l.z).collect();
    let hole_zs: Vec<f64> = s.hole.iter().map(|l| l.z).collect();
    assert_eq!(below.len(), hole_zs.len(), "{:?} vs {:?}", below, hole_zs);
    for (a, b) in below.iter().zip(&hole_zs) {
        assert!((a - b).abs() < 1e-12, "{:?} vs {:?}", below, hole_zs);
    }
    // And `stepdown` is still a maximum in BOTH runs: 2mm of scrap over the
    // crown in 0.75 bites is three, 4.5mm under it is six.
    assert_eq!(s.moat.len() - below.len(), 3, "{:?}", s.moat);
    assert_eq!(below.len(), 6, "{:?}", below);
}

#[test]
fn test_only_the_crowned_member_of_the_family_opens_a_hole() {
    // The whole point of splitting at the crown rather than writing a torus
    // arm: nine of the ten put their crown at the meridian's FIRST sample — a
    // cap's apex on the axis, a band's own top circle — so the inner branch is
    // empty, no hole opens, and the moat stack is what it always was. This pin
    // is the one that would fail if the split ever started firing on a shape
    // whose middle is not a hole at all.
    for (fixture, name, top) in [
        ("sphere.cam", "TopDome", 22.0),
        ("cone.cam", "Point", 22.0),
        ("spline_cap.cam", "Cap", 32.0),
    ] {
        let f = feature_of(fixture, name);
        let s = parse::derive_revolve_clear_stacks(&f, top, 3.0, 0.5)
            .unwrap_or_else(|| panic!("{} should produce a stack", fixture));
        assert!(s.hole.is_empty(), "{}: nothing opens inside this one", fixture);
        assert!(s.pillar_r(12.0).abs() < 1e-12, "{}: no hole, no pillar", fixture);
    }
    let torus = feature_of("torus.cam", "T");
    let s = parse::derive_revolve_clear_stacks(&torus, 6.5, 3.0, 0.5).expect("both stacks");
    assert!(!s.hole.is_empty(), "and exactly one of the ten does");
}

#[test]
fn test_the_pillar_stands_one_tool_diameter_inside_the_narrowest_the_hole_gets() {
    // The moat law read in the mirror. The hole is narrowest at its floor,
    // where the offset inner equator stands at 25 − (4 + 0.5) = 20.5; a 12mm
    // rougher reaches 12mm in from there and stops, and the column inside —
    // r 8.5 on the shipped fixture — survives exactly as the part survives on
    // its pedestal. Stock nothing declares is stock nothing takes.
    let torus = feature_of("torus.cam", "T");
    let s = parse::derive_revolve_clear_stacks(&torus, 6.5, 0.75, 0.5).expect("both stacks");
    let narrowest = s.hole.iter().map(|l| l.ceiling_r).fold(f64::MAX, f64::min);
    assert!((narrowest - 20.5).abs() < 1e-12, "the hole's floor is its narrowest: {}", narrowest);
    assert!((s.pillar_r(12.0) - 8.5).abs() < 1e-12, "pillar {}", s.pillar_r(12.0));
    // A smaller tool leaves a fatter pillar, and a tool wide enough to swallow
    // the hole leaves none: the degenerate is a full-disc clear with no island
    // at all, which is what the above-crown levels already emit.
    assert!((s.pillar_r(6.0) - 14.5).abs() < 1e-12, "pillar {}", s.pillar_r(6.0));
    assert!(s.pillar_r(25.0).abs() < 1e-12, "no pillar, not a negative one");
}

#[test]
fn test_revolve_clear_has_no_stack_when_the_blank_does_not_stand_above_the_base() {
    // The too-short-stock case the containment lint reports, asked of the
    // geometry directly: with the datum AT the stock top there is no span to
    // divide, and the honest answer is that there is no stack — not a stack of
    // zero cuts, and not one cut of zero depth.
    let dome = feature_of("sphere.cam", "TopDome");
    assert!(parse::derive_revolve_clear_stacks(&dome, 0.0, 4.0, 0.0).is_none());
    assert!(parse::derive_revolve_clear_stacks(&dome, -5.0, 4.0, 0.0).is_none());
    // And a non-positive stepdown divides nothing.
    assert!(parse::derive_revolve_clear_stacks(&dome, 20.0, 0.0, 0.0).is_none());
}

#[test]
fn test_revolve_clear_declines_a_feature_that_is_not_a_standing_revolve() {
    let bore = feature_of("cylinder_bore.cam", "Bore");
    assert!(parse::derive_revolve_clear_stacks(&bore, 20.0, 4.0, 0.0).is_none());
}

#[test]
fn test_sphere_clear_golden() {
    let main_src = fs::read_to_string("sphere_clear.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse sphere_clear.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode =
        fs::read_to_string("tests/sphere_clear.golden.gcode").expect("failed to read sphere_clear golden G-code");

    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
}

// ---- the revolve-boss Clear itself (DECISIONS.md → "roughing a convex
// revolve…", slice 1).

fn clear_src(edit: impl Fn(String) -> String) -> String {
    edit(fs::read_to_string("sphere_clear.cam/main.part").expect("read sphere_clear.cam"))
}

#[test]
fn test_the_clear_opens_the_material_the_finish_pass_enters() {
    // The whole claim of the slice, as one before/after — asserted on the STOCK
    // MODEL, not on whether `check_moves` happens to reject.
    //
    // That distinction is the point. `check_moves` finds fouls by sampling the
    // tool body on a fixed grid whose spacing is `reach / HEIGHT_SAMPLES`, and
    // `reach` moves with the stock's own height — so raising a stock by 2mm can
    // step the sampler over a thin standing region and turn an error into
    // silence. Pinning the error would pin the sampler's alignment. Pinning
    // the model pins the geometry.
    //
    // The point is one the finishing tool's SHANK really crosses and its ball
    // never cuts. It moved once, deliberately: the original (70.178, 40,
    // 11.703) sat 23.33mm from the sphere's centre — inside the [20, 26] shell
    // the ball provably sweeps — so it was pinning the collar's under-claim,
    // the exact defect the swept-tube ceiling fixed (DECISIONS.md → "rim-area
    // accounting…", slice 0). This one is ABOVE the ceiling (16.492 at
    // rho 20.1): the equator ring's shank passes through it on the way down,
    // the tube never claims it, and only the Clear opens it.
    const HIT: (f64, f64, f64) = (70.1, 40.0, 16.7);

    let without = clear_src(|s| {
        s.lines().filter(|l| !l.trim_start().starts_with("run Clear")).collect::<Vec<_>>().join("\n")
    });
    let part = parse::parse(&without).expect("parse");
    let rem = stock::remaining_after(&part, 0);
    assert!(rem.complete);
    assert!(
        rem.material_at_slack(HIT.0, HIT.1, HIT.2, 0.0),
        "with no Clear, the stock still stands where the ball's body goes"
    );

    let part = parse::parse(&clear_src(|s| s)).expect("parse");
    assert!(gcode::program(&part).is_ok(), "the fixture builds");
    let rem = stock::remaining_after(&part, 0);
    assert!(rem.complete, "and the Clear has a closed-form removal, so the model is total");
    assert!(
        !rem.material_at_slack(HIT.0, HIT.1, HIT.2, 0.0),
        "the Clear opened it — that is what the slice bought"
    );
}

#[test]
fn test_the_clear_removes_the_terraces_it_cut_not_the_dome() {
    // Fork 4A, pinned where it can actually be seen: the Clear's removal is an
    // ANNULUS with a stepped floor, not the feature's own offset dome. Reading
    // the dome here would claim the terraces are gone — standing material as
    // air — which is the direction that lets a finish pass plunge into a step.
    let part = parse::parse(&clear_src(|s| s)).expect("parse");
    let rem = stock::remaining_after(&part, 0);
    assert!(rem.complete, "the clear has a closed-form removal");
    assert_eq!(rem.removals.len(), 1);
    match (&rem.removals[0].section, &rem.removals[0].floor) {
        (stock::Section::Annulus { r_inner, r_outer, .. }, stock::Floor::Meridian { samples, .. }) => {
            // Inside the topmost island nothing was touched, and the moat stops
            // one tool diameter past the rim-plus-allowance.
            // The topmost level sits AT the offset apex, where the island is
            // nothing at all — so the moat's inner edge is the axis and the
            // scrap above the part comes off across the whole disc.
            assert!(r_inner.abs() < 1e-9, "topmost island is the apex: {}", r_inner);
            assert!((r_outer - 32.5).abs() < 1e-9, "20.5 rim + 12mm tool: {}", r_outer);
            // A staircase, not a curve: every rho appears twice — once ending a
            // terrace and once starting the riser below it.
            assert_eq!(samples.len(), 12, "six levels, two samples each: {:?}", samples);
            for w in samples.windows(2) {
                assert!(w[1].0 >= w[0].0 - 1e-9, "rho ascending: {:?}", samples);
                assert!(w[1].1 <= w[0].1 + 1e-9, "floor descending outward: {:?}", samples);
            }
            assert!((samples.last().unwrap().1 - 0.0).abs() < 1e-9, "the moat floor is the base plane");
        }
        other => panic!("expected a terraced annulus, got {:?}", other),
    }
}

#[test]
fn test_the_clear_family_runs_outermost_in_and_lands_on_the_island() {
    // The pocket's rule mirrored: the pass that defines the measured surface
    // comes LAST, and here that surface is the island. The family therefore
    // starts at the moat's outer edge and ends exactly tangent to the island +
    // allowance, rather than stopping short of it or crossing it.
    let part = parse::parse(&clear_src(|s| s)).expect("parse");
    let g = gcode::program(&part).expect("program");
    // The first level: three passes, from r_outer_centre = 26.5 inward.
    let starts: Vec<f64> = g
        .lines()
        .skip_while(|l| !l.contains("level z16.400"))
        .take_while(|l| !l.contains("level z12.300"))
        .filter_map(|l| {
            let x = l.strip_prefix("G00 X")?.split(" Y").next()?.parse::<f64>().ok()?;
            l.contains("Y40.000").then_some(x - 50.0)
        })
        .collect();
    assert_eq!(starts.len(), 3, "three passes on that level: {:?}", starts);
    assert!((starts[0] - 26.5).abs() < 1e-3, "starts at the moat's outer edge: {:?}", starts);
    assert!(starts[1] < starts[0] && starts[2] < starts[1], "outermost-in: {:?}", starts);
    // island 12.300 + tool radius 6
    assert!((starts[2] - 18.300).abs() < 1e-3, "lands tangent to the island: {:?}", starts);
}

#[test]
fn test_the_clear_rings_are_cut_clockwise_like_every_other_outside_pass() {
    // The reasons are recorded on finish_cylinder_boss: CW around an outside
    // wall is climb milling under M03. A boss rough that spiralled the other
    // way would conventional-cut the one surface anyone measures.
    let part = parse::parse(&clear_src(|s| s)).expect("parse");
    let g = gcode::program(&part).expect("program");
    let after = g
        .lines()
        .skip_while(|l| !l.contains("level z0.000"))
        .filter_map(|l| {
            let rest = l.strip_prefix("G01 X")?;
            let (x, tail) = rest.split_once(" Y")?;
            let y = tail.split_whitespace().next()?.parse::<f64>().ok()?;
            Some((x.parse::<f64>().ok()?, y))
        })
        .nth(1)
        .expect("a cutting move on the bottom level");
    assert!(after.1 < 40.0, "CW from the +X start means Y falls first: {:?}", after);
}

#[test]
fn test_stepdown_is_required_on_a_revolve_clear_and_refused_everywhere_else() {
    // The `strategy` / `voxel_cell` rule: a parameter that silently does
    // nothing is a program that lies about itself.
    let no_stepdown = clear_src(|s| s.replace(", stepdown=5.0", ""));
    let err = parse::parse(&no_stepdown).expect_err("must reject");
    assert!(err.contains("requires a positive stepdown"), "{}", err);

    let on_a_finish = clear_src(|s| {
        s.replace("run Copy(tool=T9, surface=TopDome, stepover=3.0)",
                  "run Copy(tool=T9, surface=TopDome, stepover=3.0, stepdown=2.0)")
    });
    let err = parse::parse(&on_a_finish).expect_err("must reject");
    assert!(err.contains("stepdown slices a stack of Z levels and belongs to Clear, the volume verb"), "{}", err);
    assert!(err.contains("a Copy on 'TopDome' follows one surface and has one increment, stepover"), "{}", err);
}

#[test]
fn test_the_clear_refuses_a_meridian_that_crests_more_than_once() {
    // What the "not single-valued in z" error narrowed DOWN to. One crown
    // splits a meridian into two branches and each is sayable; a second crest
    // puts three radii at some height, and no stack of discs and annuli states
    // that. Only the freeform member can be authored that way, so the pin is a
    // hand-written wavy meridian — and it proves the error is reachable
    // rather than a message written for a case that cannot happen.
    let wavy = "\
degree 1
knots 0 0 0.25 0.5 0.75 1 1
pole 0 20
pole 4 12
pole 7 18
pole 10 6
pole 13 0
";
    let dir = std::env::temp_dir().join(format!("spline_cap_two_crest_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("meridian.spline"), wavy).unwrap();
    let main_src = fs::read_to_string("spline_cap.cam/main.part").expect("read spline_cap.cam");
    let result = parse::parse_project_with_base(&main_src, None, Some(dir.as_path()));
    fs::remove_dir_all(&dir).ok();
    let err = result.expect_err("two crests cannot be stated");
    assert!(err.contains("crests more than once"), "{}", err);
    assert!(err.contains("three radii at some height"), "{}", err);
}

#[test]
fn test_the_clear_refuses_a_blank_that_does_not_stand_over_the_feature() {
    // The containment lint's finding, met at the op: with the datum AT the
    // stock top there is nothing over the dome to rough, and the fix is the
    // stock rather than the parameters — so the message says so.
    let src = clear_src(|s| s.replace("stock Box(w=100, l=80, h=47, x=0, y=0, z=22)",
                                      "stock Box(w=100, l=80, h=25, x=0, y=0, z=0)"));
    let err = parse::parse(&src).expect_err("must reject");
    assert!(err.contains("has nothing to rough"), "{}", err);
    assert!(err.contains("Declare the stock"), "{}", err);
}

#[test]
fn test_clear_on_a_prismatic_boss_still_refuses_but_no_longer_on_a_principle() {
    // Fork 1A's cost, pinned so it cannot rot back into the old sentence. The
    // error survives as a TYPO catch on the one pair that has a confusable
    // sibling — and it must no longer claim that clearing around standing
    // material is the opposite op, because the revolve family now does exactly
    // that. Since Phase 10 lane A the CYLINDER boss has left the pair (it is a
    // revolve, and takes the moat), so the profile boss is the one that
    // still rejects.
    let src = fs::read_to_string("profile_boss.cam/main.part").expect("read profile_boss.cam");
    let boss = src
        .lines()
        .find_map(|l| l.strip_prefix("feature ").map(|r| r.split_whitespace().next().unwrap().to_string()))
        .expect("a feature");
    let tool = src
        .lines()
        .find_map(|l| l.strip_prefix("tool T").map(|r| r.split_whitespace().next().unwrap().to_string()))
        .expect("a tool");
    // Since Phase 10 lane A slice 1 the profile boss takes the moat too, so
    // the typo catch is gone with the refusal: a Clear aimed at the boss is a
    // real op, rejected only for what every standing-material Clear needs — a
    // stepdown — and emitted with one.
    let with_clear = format!("{}\nrun Clear(tool=T{}, profile={}, stepover=2.0)\n", src, tool, boss);
    let err = parse::parse(&with_clear).expect_err("no stepdown, no stack");
    assert!(err.contains("is a standing boss") && err.contains("requires a positive stepdown"), "{}", err);
    assert!(!err.contains("it is the opposite op"), "the retired principle must not survive: {}", err);
    let moated = format!("{}\nrun Clear(tool=T{}, profile={}, stepover=2.0, stepdown=2.0)\n", src, tool, boss);
    let part = parse::parse(&moated).expect("a moat around a profile boss is a real op now");
    let g = gcode::program(&part).expect("and it emits");
    assert!(g.contains("(--- boss clear: Outline with T5 ---)"), "{g}");
}

/// Phase 10 lane A, slice 0 (DECISIONS.md → "Phase 10 lane A, slice 0…"): the
/// cylinder boss joins the revolve moat family — `derive_revolve_datum` knows
/// it, its meridian is the vertical line at `R + skin`, and the stack is a
/// plain run of equal levels from the stock top to its base with the island at
/// `R + skin` on every one and no hole.
#[test]
fn test_the_cylinder_boss_joins_the_revolve_moat_family() {
    let post = feature_of("cylinder_boss.cam", "Post"); // R 20, base z -12, top z 0
    let stacks = parse::derive_revolve_clear_stacks(&post, 0.0, 4.0, 0.0).expect("a stack");
    assert_eq!(stacks.moat.len(), 3, "12mm at a 4mm maximum bite: three equal levels");
    for (i, l) in stacks.moat.iter().enumerate() {
        assert!((l.z - (-4.0 * (i as f64 + 1.0))).abs() < 1e-9, "level {i} at {}", l.z);
        assert!((l.island_r - 20.0).abs() < 1e-9, "the island is the boss itself, every level: {}", l.island_r);
    }
    assert!(stacks.hole.is_empty(), "a cylinder crowns nowhere and opens nothing inside");
    let skinned = parse::derive_revolve_clear_stacks(&post, 0.0, 4.0, 0.5).expect("a stack");
    assert!(skinned.moat.iter().all(|l| (l.island_r - 20.5).abs() < 1e-9), "the island carries the allowance");

    // THE MOAT IS WHAT THE FINISH NEEDS TO ENTER: two tool diameters, not the
    // revolve family's one, because `boss_engine` leads in from a diameter
    // outside the tool-centre circle. The title says so, and the lead-in's
    // body — x 85 + 5 on this fixture — stands in cleared air.
    let src = fs::read_to_string("cylinder_boss.cam/main.part").expect("read");
    let part = parse::parse(&src).expect("parse");
    let g = gcode::program(&part).expect("emits — the finish has its moat");
    assert!(g.contains("(moat to r 40.000 — the rim 20.000 plus 2.000 tool diameters — the finish's own and the one its lead-in stands out;"), "{}", g.lines().find(|l| l.contains("moat to r")).unwrap_or(""));
    let after_clear = stock::remaining_after(&part, 0);
    assert!(after_clear.complete);
    assert!(!after_clear.material_at(90.0, 50.0, -5.0), "the lead-in's outer body edge is in the moat's air");
    assert!(after_clear.material_at(91.0, 50.0, -5.0), "and the moat ends where it says: stock beyond stays");
}

/// The control the DRAFT named (fork 8, slice 0): the shipped one-`Finish`
/// program — the cylinder boss with no rough in front — is REJECTED by name now
/// that the band exists, at the lead-in plunge the measurement found. That
/// program emitted until today only because the model declined to look.
#[test]
fn test_the_one_finish_boss_program_refuses_once_the_band_exists() {
    let src = fs::read_to_string("cylinder_boss.cam/main.part").expect("read");
    let bare: String = src.lines().filter(|l| !l.starts_with("run Clear")).map(|l| format!("{l}\n")).collect();
    assert_ne!(bare, src, "the Clear must actually be stripped");
    let part = parse::parse(&bare).expect("parses; the error is the entry restriction's");
    let e = gcode::program(&part).expect_err("a boss finish with no moat leads into standing stock");
    assert!(e.starts_with("Side on 'Post' with T5: the move `G01 Z-5.000 F300.000`"), "{e}");
    assert!(e.contains("standing stock at (90.000, 50.000, -5.000)"), "the lead-in, one diameter out: {e}");

    // And the band is the finish's own circle: on that bare program, after
    // the finish, the annulus it rode is void, the boss inside it stands, and
    // the stock beyond it stands — read from the declaration, not the emitted moves.
    let after = stock::remaining_after(&part, 0);
    assert!(after.complete, "the cylinder boss finish has a removal");
    assert!(!after.material_at(75.0, 50.0, -6.0), "on the tool-centre circle (rho 25): swept");
    assert!(!after.material_at(70.5, 50.0, -6.0), "at the wall (rho 20.5): swept");
    assert!(!after.material_at(79.5, 50.0, -6.0), "one diameter out (rho 29.5): swept");
    assert!(after.material_at(60.0, 50.0, -6.0), "inside the boss: standing");
    assert!(after.material_at(82.0, 50.0, -6.0), "beyond the band (rho 32): standing");
    assert!(after.material_at(75.0, 50.0, -13.0), "below the base: standing");
}

/// A declared `flute_len` shorter than the boss is shank above it, and shank
/// removes nothing: the band's top is the cutting top above the lowest tip.
#[test]
fn test_the_boss_band_stops_at_the_tool_s_cutting_top() {
    let src = fs::read_to_string("cylinder_boss.cam/main.part").expect("read");
    let bare: String = src.lines().filter(|l| !l.starts_with("run Clear")).map(|l| format!("{l}\n")).collect();
    let short = bare.replace("tool T5 Endmill(dia=10.0)", "tool T5 Endmill(dia=10.0, flute_len=6.0)");
    assert_ne!(short, bare);
    let part = parse::parse(&short).expect("parse");
    let after = stock::remaining_after(&part, 0);
    assert!(after.complete);
    assert!(!after.material_at(75.0, 50.0, -9.0), "within 6mm of the base (the lowest tip): swept");
    assert!(after.material_at(75.0, 50.0, -3.0), "above the cutting top: the shank, nothing removed");
}

/// The rest report makes a CLAIM on the boss fixtures now — no `no claim`,
/// and nothing forgotten: the moat and the band are swept exactly as declared.
#[test]
fn test_the_rest_report_claims_on_the_cylinder_boss() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--rest-report", "cylinder_boss.cam"])
        .output()
        .expect("run nacs");
    let report = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(report.contains("forgotten (declared removed, never swept): none"), "{report}");
    assert!(!report.contains("no claim"), "{report}");
}

/// Phase 10 lane A, slice 2: the wire boss's floor FOLLOWS THE WIRE. Its
/// finish runs a tip at the wire's z less `extent`, interpolated along each
/// edge (`contour_boss.cam`'s bottom edge runs from (70,30,−3) to (30,30,−1)),
/// and `Floor::AlongWire` reads that tip through the emitter's own
/// `wire_cut_z` — so at a mid-edge point the model says "gone" just above the
/// tip and "standing" just below it, at the height the machine actually cut.
#[test]
fn test_the_wire_boss_floor_follows_the_wire() {
    let src = fs::read_to_string("contour_boss.cam/main.part").expect("read");
    // The floor is read on the finish ALONE (the shipped program's moat in
    // front of it floors the whole surround to the wall's base, which is
    // right, and hides exactly what this pins). The CL rides 5mm south of the
    // bottom edge at y = 25; the wire's z there is −2 at x = 50 and −2.5 at
    // x = 60 (extent 0, so the tip IS the wire's z).
    let bare: String = src.lines().filter(|l| !l.starts_with("run Clear")).map(|l| format!("{l}\n")).collect();
    let part = parse::parse(&bare).expect("parse");
    let after = stock::remaining_after(&part, 0);
    assert!(after.complete, "the wire boss finish has its band");
    assert!(!after.material_at(50.0, 25.0, -1.5), "above the tip at x 50: swept");
    assert!(after.material_at(50.0, 25.0, -2.5), "below it: standing");
    assert!(!after.material_at(60.0, 25.0, -2.3), "above the tip at x 60 (wire −2.5): swept");
    assert!(after.material_at(60.0, 25.0, -2.7), "below it: standing");
    // And the band is a band: one diameter south of the CL the tool never went.
    assert!(after.material_at(50.0, 14.0, -1.5), "beyond the band: standing");
    // With the moat in front (the shipped program) the whole surround is
    // gone to the wall's base, which is the moat's job.
    let part = parse::parse(&src).expect("parse");
    assert!(stock::remaining_after(&part, 1).complete, "moat and band: complete");
    assert!(!stock::remaining_after(&part, 1).material_at(50.0, 25.0, -2.5), "the moat floors the surround to the base");

    // The rest report claims, and nothing is forgotten.
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--rest-report", "contour_boss.cam"]).output().expect("run");
    let report = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    assert!(report.contains("declared model: complete") && report.contains("forgotten (declared removed, never swept): none"), "{report}");

    // THE CONTROL: the wire boss finished with no moat rejects by name at its
    // lead-in — which stands on the VOID side (outward for a CW boss; the
    // engine wire arm led toward the centroid, into the part, until this
    // slice, as `contour_engine` had until slice 1). Since Phase 47 slice 2
    // the lead reads the room, and an uncut surround has none: it plunges ON
    // the path by name — the path's first vertex, (75, 30) — and the
    // restriction refuses that plunge, the body in the surround beside it.
    let part = parse::parse(&bare).expect("parse");
    let e = gcode::program(&part).expect_err("an unroughed wire boss leads into standing stock");
    assert!(e.starts_with("Side on 'Crest' with T5:"), "{e}");
    assert!(e.contains("standing stock at (79.830, 31.294,"), "outside the boss, in the stock it never cleared: {e}");
    assert!(e.contains("the tip at (75.000, 30.000,"), "refused at the path's first vertex, where the lead plunged for want of room: {e}");
}

/// The rim wire's decline says what pays it. Its band is statable; the
/// program that finishes a rim alone leads into the rim's INTERIOR, which no
/// declaration opens and no vocabulary can yet rough (T5) — so the model
/// declines by name rather than reject every such program at its lead-in.
#[test]
fn test_the_rim_wire_declines_and_names_the_vocabulary_that_pays_it() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--check=json", "contour_pocket.cam"]).output().expect("run");
    let doc = String::from_utf8_lossy(&out.stdout);
    assert!(doc.contains("a rim wire's interior is undeclared"), "{doc}");
    assert!(doc.contains("non-planar-floored pocket"), "{doc}");
}

/// Fork 6A of the Phase-10 DRAFT, ruled: the two camp-3 kinds decline
/// PERMANENTLY, and the decline's own text says so rather than reading as a
/// removal somebody still owes.
#[test]
fn test_mesh_and_patch_decline_permanently_in_their_own_words() {
    for fixture in ["mesh.cam", "nurbs_patch.cam"] {
        let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
            .args(["--check=json", fixture])
            .output()
            .expect("run nacs");
        let doc = String::from_utf8_lossy(&out.stdout);
        assert!(doc.contains("and never will"), "{fixture}: {doc}");
        assert!(doc.contains("the swept field IS its removal (permanent, by the Phase-10 ruling)"), "{fixture}: {doc}");
    }
}

#[test]
fn test_the_level_stack_breaks_at_the_apex_so_no_plug_stands_over_the_part() {
    // A stack that merely divided `stock_top → base` would straddle the apex,
    // and everything above it would stand inside the topmost island: a PLUG of
    // pure scrap sitting on the part, which no later waterline pass can reach
    // around. Breaking the stack at the apex puts a level exactly there, where
    // the island is nothing and the whole disc comes off.
    let dome = feature_of("sphere.cam", "TopDome"); // r 20, base z 0, apex z 20
    let levels = parse::derive_revolve_clear_stacks(&dome, 26.0, 5.0, 0.0).expect("a stack").moat;
    let apex = levels.iter().find(|l| (l.z - 20.0).abs() < 1e-9).expect("a level AT the apex");
    assert!(apex.island_r.abs() < 1e-9, "nothing stands at the apex: {:?}", apex);
    // The run above it still respects stepdown as a maximum: 6mm of scrap over
    // the apex is two 3mm bites (one landing ON the apex), not one 6mm one.
    let above: Vec<f64> = levels.iter().filter(|l| l.z > 20.0 + 1e-9).map(|l| l.z).collect();
    assert_eq!(above, vec![23.0], "6mm above the apex in <=5mm bites: {:?}", above);
    for l in levels.iter().filter(|l| l.z >= 20.0 - 1e-9) {
        assert!(l.island_r.abs() < 1e-9, "no island above the apex: {:?}", l);
    }
}

#[test]
fn test_derive_emitted_skin_answers_what_each_carrier_actually_rides() {
    // `derive_emitted_skin` is "the surface the pass actually cuts to, not the
    // ceiling it guarantees". It returned the ceiling on stepover-mode surface
    // roughing until 2026-08-12, then declined, and now answers — one carrier
    // at a time, from the same home the emitter reads
    // (DECISIONS.md → "the roughing allowance has two arms…", fork 3A).
    //
    // (The stepover-mode carriers that pinned numbers here were the revolved
    // kinds deleted on 2026-10-28; the scallop arm below is what remains.)

    // Scallop mode is the emitter's own arithmetic and still answers directly.
    let sphere = parse::parse(&fs::read_to_string("sphere_rough.cam/main.part").expect("read"))
        .expect("parse");
    let op = sphere.operations.last().expect("the finish op");
    assert_eq!(parse::derive_emitted_skin(&sphere, op), Some(0.5 - 0.2));

    // And a PRISMATIC flank has no cusp to spend, so the allowance is the skin.
    let bore = parse::parse(&fs::read_to_string("cylinder_bore_rough.cam/main.part").expect("read"))
        .expect("parse");
    let op = bore.operations.iter().find(|o| o.stock_to_leave.is_some()).expect("a rough");
    assert_eq!(parse::derive_emitted_skin(&bore, op), op.stock_to_leave);
}

#[test]
fn test_every_shipped_roughing_pass_has_a_closed_form_removal_again() {
    // The coverage #112 gave up, fully recovered. Declining kept the model
    // honest but stood `check_moves` and the foul lint down on seven carriers;
    // with every arm in place there is nothing left standing down.
    for fixture in [
        "spline_funnel_rough.cam",
    ] {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read");
        let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new(fixture)))
            .expect("parse");
        let rem = stock::remaining_after(&part, part.operations.len() - 1);
        assert!(rem.complete, "{fixture}: the removal is closed-form again");
        assert!(stock::incomplete_ops(&part).is_empty(), "{fixture}: nothing unaccounted");
    }
}

#[test]
fn test_the_models_skin_is_what_the_program_says_it_spent() {
    // The cross-check that would have caught the whole thread in one line.
    //
    // Two things compute the skin a roughing pass rides. The EMITTER chooses it
    // (`allowance − cusp`, the cusp from its own sweep-then-verify) and prints
    // the cusp in the program header. The MODEL re-derives it in
    // `parse::derive_emitted_skin` so `stock::removals_for` can place the
    // surface. Those are the same number by definition, and they drifted:
    // the model returned the CEILING on stepover-mode surface roughing, so it
    // put `vase_rough`'s surface 0.173mm outside where the ball went and
    // `check_moves` refused a correct program (DECISIONS.md → "the vase's
    // roughing pass rides what it promises…").
    //
    // This asserts the two agree — reading the emitter's answer out of the
    // program TEXT rather than restating its math, so the pin cannot drift with
    // the thing it checks. A model that cannot know the number must say so;
    // what it must never do is answer something else.
    let mut dirs: Vec<String> = fs::read_dir(".")
        .expect("read cwd")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().into_string().expect("utf8"))
        .filter(|n| n.ends_with(".cam"))
        .collect();
    dirs.sort();

    let mut checked = 0usize;
    let mut declined = 0usize;
    for dir in dirs {
        let path = format!("{dir}/main.part");
        let Ok(src) = fs::read_to_string(&path) else { continue };
        let Ok(part) = parse::parse_project_with_base(&src, None, Some(std::path::Path::new(&dir)))
        else {
            continue;
        };
        let Ok(g) = gcode::program(&part) else { continue };
        for op in part.operations.iter() {
            let Some(allowance) = op.stock_to_leave else { continue };
            // The emitter's own answer, read out of the text it wrote.
            // Matched by feature AND tool: since the corpus was contained, a
            // feature's Clear and its Finish both print a cusp-budget header,
            // and the Clear's structural 0.000 read as the finish's spend
            // would accuse a correct pass of riding the ceiling.
            let Some(line) = g
                .lines()
                .find(|l| {
                    l.contains("cusp budget")
                        && l.contains(&op.feature_name)
                        && l.contains(&format!("with T{}", op.tool_num))
                })
            else {
                continue;
            };
            let cusp: f64 = line
                .split("cusp budget ")
                .nth(1)
                .and_then(|t| t.split("mm").next())
                .and_then(|t| t.parse().ok())
                .expect("a cusp budget reads as a number");
            let emitted = allowance - cusp;
            match parse::derive_emitted_skin(&part, op) {
                None => {
                    declined += 1;
                    // Declining is only honest where the number really is the
                    // emitter's private choice — a stepover pass over a curve.
                    assert!(
                        op.scallop.is_none(),
                        "{dir}/{}: declined on a SCALLOP pass, whose skin is allowance − scallop and is derivable",
                        op.feature_name
                    );
                }
                Some(model) => {
                    checked += 1;
                    assert!(
                        (model - emitted).abs() < 5e-4,
                        "{dir}/{}: the program spent a {cusp:.3}mm cusp so it rides {emitted:.3}mm, but the model places the surface at {model:.3}mm — that difference is material the model puts where the tool legitimately is",
                        op.feature_name
                    );
                }
            }
        }
    }
    // A glob that matched nothing would pass every assertion above.
    assert!(checked >= 10, "only {checked} roughing passes cross-checked — did the fixture scan break?");
    // Every shipped roughing pass is now derivable — the decline path still
    // exists for a carrier with no arm, but no fixture reaches it. Pinned as an
    // equality so ADDING a carrier without its budget arm fails here loudly
    // rather than quietly dropping that program's stock model on the floor.
    assert_eq!(
        declined, 0,
        "{declined} roughing pass(es) have no shared budget arm — add one in parse::derive_stepover_budget, or change this pin deliberately"
    );
}

/// A torus with NO rough behind it: the scratch source the tube pins need,
/// because every shipped torus fixture now has a Clear in front of its finish.
fn torus_finish_only() -> String {
    "units mm\nfeed 600\nplunge 200\nrapid_z 9\nclearance_z 25\nspindle 4000\n\
stock Box(w=100, l=100, h=31, x=0, y=-10, z=6)\ntool T9 Ballmill(dia=6.0)\n\
feature T Torus(major_dia=50, minor_dia=8, x=50, y=40, z=0)\n\
run Copy(tool=T9, surface=T, stepover=3.0)\n"
        .to_string()
}

#[test]
fn test_the_shipped_torus_family_is_contained_and_silent() {
    // Slice 2's payoff, and the bar the corpus campaign set: all three torus
    // fixtures declare stocks their donut fits in, each with a Clear opening
    // the moat AND the hole before its finish, and each is silent on every
    // voice — nothing reaching outside the stock, no tool body in material that
    // outlives its op, no unlicensed entry.
    for fixture in ["torus.cam", "torus_rough.cam", "torus_scallop.cam"] {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read fixture");
        let part = parse::parse(&src).unwrap_or_else(|e| panic!("{fixture}: parse: {e}"));
        let g = gcode::program(&part).unwrap_or_else(|e| panic!("{fixture}: {e}"));
        assert!(parse::lint(&part).is_empty(), "{fixture}: {:?}", parse::lint(&part));
        assert!(
            stock::containment_findings(&part).is_empty(),
            "{fixture}: {:?}", stock::containment_findings(&part)
        );
        // And the program says both work regions, level by level.
        assert!(g.contains("(hole open to r"), "{fixture}: the hole is opened by name");
        assert!(g.lines().any(|l| l.starts_with("(hole z")), "{fixture}: and level by level");
    }
}

#[test]
fn test_the_clear_opens_the_torus_hole_and_leaves_the_pillar_standing() {
    // What the removal model claims after torus.cam's Clear, read at eight
    // places. The hole is a moat: cleared to the base plane, with a column
    // standing in the middle because nothing declares it should go.
    let src = fs::read_to_string("torus.cam/main.part").expect("read torus.cam");
    let part = parse::parse(&src).expect("parse");
    let after = stock::remaining_after(&part, 0);
    assert!(after.complete);
    let at = |rho: f64, z: f64| after.material_at(50.0 + rho, 40.0, z);
    // The pillar, top to bottom — r 8.5, the inner rim less one tool diameter.
    assert!(at(4.0, 5.0), "the pillar stands over the crown's height");
    assert!(at(8.0, -0.5), "and below the base plane, on its own footing");
    // Beside it the hole is open all the way down: the bottom level's single
    // pass sweeps the whole remaining annulus, so the floor there is z 0.
    assert!(!at(10.0, 4.4), "just outside the pillar: cleared to the base plane");
    assert!(!at(14.0, 2.9), "and mid-hole likewise");
    // The staircase's inward terraces: at rho 21 only the top hole level
    // reached, so material stands from its floor down — and that material is
    // the skin the finish is owed, not a leftover.
    assert!(!at(21.0, 3.5), "above the top hole terrace: cleared");
    assert!(at(21.5, 2.9), "below it the offset flank's allowance stands");
    // The moat and the pedestal, unchanged by any of this.
    assert!(!at(35.0, 0.5), "the moat is open to the base plane");
    assert!(at(35.0, -0.5), "and stops there");
    assert!(at(43.0, 5.0), "beyond the moat the stock stands as a pedestal");
}

#[test]
fn test_the_clear_never_crosses_the_pillar_it_leaves() {
    // Cost 4 of the design, pinned: the pillar is new geometry for the Clear's
    // own spiral to respect, and it is respected by BOTH families at once —
    // the hole's innermost pass and the above-crown moat pass are the same
    // circle, tool-centre r 14.5, one tool radius off a pillar of r 8.5. The
    // op never puts a centre inside that, so nothing touches the column.
    let src = fs::read_to_string("torus.cam/main.part").expect("read torus.cam");
    let part = parse::parse(&src).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("generates");
    let clear = emitted_moves.first().expect("the clear op");
    let coord = |line: &str, axis: char| -> Option<f64> {
        line.split_whitespace().find(|w| w.starts_with(axis)).and_then(|w| w[1..].parse().ok())
    };
    let (mut x, mut y) = (f64::NAN, f64::NAN);
    let mut closest = f64::INFINITY;
    for line in &clear.moves {
        x = coord(line, 'X').unwrap_or(x);
        y = coord(line, 'Y').unwrap_or(y);
        // Z-only blocks before the first positioning move say nothing about
        // where the tool is in plan.
        if x.is_nan() || y.is_nan() {
            continue;
        }
        closest = closest.min(((x - 50.0).powi(2) + (y - 40.0).powi(2)).sqrt());
    }
    assert!(
        (closest - 14.5).abs() < 1e-3,
        "the innermost tool centre should ride 14.5 (pillar 8.5 + tool radius 6), got {closest:.4}"
    );
}

#[test]
fn test_the_torus_joins_the_tube_family_and_its_straddle_collar_retires() {
    // Fork 4. The flat inner collar claimed a BOX at the hole's rim —
    // [rim − dia, rim] × ±drop, so rho 15..21 by z ±3 — and the ball only ever
    // reaches the innermost of that at one height. Now that the hole is opened
    // rather than left buried, the torus takes the swept tube like the rest of
    // the family, and the box's corners go back to standing material.
    let part = parse::parse(&torus_finish_only()).expect("parse");
    let rem = stock::remaining_after(&part, 0);
    assert!(rem.complete);
    let at = |rho: f64, z: f64| rem.material_at(50.0 + rho, 40.0, z);
    assert!(at(15.0, 2.0), "the old box's inner corner is material the ball never reaches");
    assert!(!at(15.0, 0.0), "at the base plane the last inner ring does reach it");
    assert!(!at(16.0, -2.0), "and dips below the base plane, as a ball on that rim must");
    // The outer rim's end-cap, the same claim the other nine already get.
    assert!(!at(33.0, 0.0), "the outer end-cap is swept");
    assert!(at(35.5, 0.0), "beyond the last ball, nothing is claimed");
}

// ---- `--check`, the validate-only mode (DECISIONS.md "the agent surface —
// every verb is a flag first…", slice 0 of the CLI half) ----------------------
//
// The mode has no golden by definition — its artifact is the check result, so the
// pins are behavioral: the exit code and stderr must be the DEFAULT mode's,
// byte for byte, with stdout empty. Anything looser and `--check` could
// drift into a cheaper approximation of the pipeline it claims to speak for.

/// A clean project: exit 0, no G-code on stdout, nothing on stderr — the
/// silence IS the report, same as a lint-free default run.
#[test]
fn test_check_on_a_clean_project_is_silent_and_emits_nothing() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check", "plate.cam"])
        .output()
        .expect("run nacs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // Stdout is the account (ROADMAP item 2, slice 6), never the program.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.starts_with("checked: ") && !stdout.lines().any(|l| l.starts_with('G') || l.starts_with('%')), "--check must not emit G-code: {stdout}");
    assert!(out.stderr.is_empty(), "a clean project has nothing to say");
}

/// A rejecting project: same nonzero exit, same one-line stderr error the
/// default mode gives, and still nothing on stdout (never partial output —
/// here "no output at all" is the whole mode). No shipped fixture rejects
/// (every one carries a golden, so every one emits), so the error is the
/// suite's standing one built in place: profile_pocket_clear's pocket with a
/// tool too large for it, `test_unmachinable_pocket_is_an_error`'s case run
/// through the CLI.
#[test]
fn test_check_refuses_exactly_as_the_default_mode_would() {
    let main_src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read main.part");
    let huge_tool = main_src.replace("dia=10.0", "dia=90.0");
    let path = std::env::temp_dir().join(format!("nacs_check_refuse_{}.part", std::process::id()));
    fs::write(&path, huge_tool).expect("write temp .part");

    let checked = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check"])
        .arg(&path)
        .output()
        .expect("run nacs --check");
    let emitted = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .arg(&path)
        .output()
        .expect("run nacs");
    fs::remove_file(&path).ok();

    assert!(!checked.status.success(), "a rejecting project must exit nonzero under --check");
    assert!(checked.stdout.is_empty(), "no partial artifact on stdout");
    assert!(!emitted.status.success());
    assert_eq!(
        checked.stderr, emitted.stderr,
        "--check's error must be the default mode's error, byte for byte"
    );
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("too small"),
        "the error says why: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
}

/// An advisory-carrying project: exit 0, the advisory voiced. The fixture is
/// cylinder_bore.cam, whose finding is `stock::entry_findings` — a
/// POST-generation family — so this pin also proves --check runs the whole
/// pipeline rather than stopping at validate (a check that skipped
/// generation would be green-but-blind on exactly this warning). Stderr is
/// compared byte-for-byte against the default mode's, which covers the
/// parse-time families in the same breath.
#[test]
fn test_check_voices_the_advisories_the_default_mode_voices() {
    let checked = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check", "cylinder_bore.cam"])
        .output()
        .expect("run nacs --check");
    let emitted = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .arg("cylinder_bore.cam")
        .output()
        .expect("run nacs");

    assert!(checked.status.success(), "an advisory is not a rejection");
    let account = String::from_utf8_lossy(&checked.stdout);
    assert!(account.starts_with("checked: ") && !account.contains("G01"), "--check must not emit G-code: {account}");
    assert!(!emitted.stdout.is_empty(), "the default mode does emit — the two differ only there");
    assert_eq!(
        checked.stderr, emitted.stderr,
        "--check's advisories must be the default mode's, byte for byte"
    );
    assert!(
        String::from_utf8_lossy(&checked.stderr).contains("meets standing material"),
        "the entry advisory is voiced: {}",
        String::from_utf8_lossy(&checked.stderr)
    );
}

// ---- `--check=json`, the structured rendering (DECISIONS.md "--check=json —
// the check result as data…") ------------------------------------------------------
//
// The document is the structs rendered plus the check result, never a third
// source; the JSON is the artifact, so stderr stays silent in this rendering
// and the exit codes are `--check`'s. Deterministic byte for byte on the
// same project: fixed field order, finding order = the order the families
// already produce.

/// A minimal JSON well-formedness checker — enough to prove a document
/// PARSES, with no serde anywhere in the tree (the crate is JSON-free on
/// purpose and the test tree follows it). Panics with position context on
/// the first violation; trailing content after the one document is one.
fn assert_valid_json(s: &str) {
    fn ws(b: &[char], i: &mut usize) {
        while *i < b.len() && matches!(b[*i], ' ' | '\t' | '\n' | '\r') {
            *i += 1;
        }
    }
    fn string(b: &[char], i: &mut usize) {
        assert_eq!(b.get(*i), Some(&'"'), "expected a string at {}", i);
        *i += 1;
        loop {
            match b.get(*i) {
                Some('"') => {
                    *i += 1;
                    return;
                }
                Some('\\') => {
                    *i += 1;
                    match b.get(*i) {
                        Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't') => *i += 1,
                        Some('u') => {
                            for k in 1..=4 {
                                assert!(
                                    b.get(*i + k).is_some_and(|c| c.is_ascii_hexdigit()),
                                    "bad \\u escape at {}",
                                    i
                                );
                            }
                            *i += 5;
                        }
                        c => panic!("bad escape {:?} at {}", c, i),
                    }
                }
                Some(c) if (*c as u32) >= 0x20 => *i += 1,
                c => panic!("bad string char {:?} at {}", c, i),
            }
        }
    }
    fn lit(b: &[char], i: &mut usize, l: &str) {
        for c in l.chars() {
            assert_eq!(b.get(*i), Some(&c), "bad literal at {}", i);
            *i += 1;
        }
    }
    fn number(b: &[char], i: &mut usize) {
        if b.get(*i) == Some(&'-') {
            *i += 1;
        }
        let start = *i;
        while b
            .get(*i)
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        {
            *i += 1;
        }
        assert!(*i > start, "empty number at {}", i);
    }
    fn value(b: &[char], i: &mut usize) {
        ws(b, i);
        match b.get(*i) {
            Some('{') => {
                *i += 1;
                ws(b, i);
                if b.get(*i) == Some(&'}') {
                    *i += 1;
                    return;
                }
                loop {
                    ws(b, i);
                    string(b, i);
                    ws(b, i);
                    assert_eq!(b.get(*i), Some(&':'), "expected ':' at {}", i);
                    *i += 1;
                    value(b, i);
                    ws(b, i);
                    match b.get(*i) {
                        Some(',') => *i += 1,
                        Some('}') => {
                            *i += 1;
                            return;
                        }
                        c => panic!("expected ',' or '}}' at {}: {:?}", i, c),
                    }
                }
            }
            Some('[') => {
                *i += 1;
                ws(b, i);
                if b.get(*i) == Some(&']') {
                    *i += 1;
                    return;
                }
                loop {
                    value(b, i);
                    ws(b, i);
                    match b.get(*i) {
                        Some(',') => *i += 1,
                        Some(']') => {
                            *i += 1;
                            return;
                        }
                        c => panic!("expected ',' or ']' at {}: {:?}", i, c),
                    }
                }
            }
            Some('"') => string(b, i),
            Some('t') => lit(b, i, "true"),
            Some('f') => lit(b, i, "false"),
            Some('n') => lit(b, i, "null"),
            Some(c) if *c == '-' || c.is_ascii_digit() => number(b, i),
            c => panic!("unexpected {:?} at {}", c, i),
        }
    }
    let b: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    value(&b, &mut i);
    ws(&b, &mut i);
    assert_eq!(i, b.len(), "trailing content after the one document at {}", i);
}

/// A clean project: exit 0, stderr silent, and the document is exactly the
/// two-field shape. The byte-pin IS the contract for the empty case — a
/// fixed field order is the deterministic-output promise made testable.
#[test]
fn test_check_json_on_a_clean_project_is_the_two_field_document() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check=json", "plate.cam"])
        .output()
        .expect("run nacs --check=json");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
        out.stderr.is_empty(),
        "the document is the whole report — stderr stays silent in the JSON rendering"
    );
    // The document opens as it always did; the account follows the empty
    // findings (ROADMAP item 2, slice 6), one object per family.
    let doc = String::from_utf8_lossy(&out.stdout);
    assert!(doc.starts_with("{\n  \"would_emit\": true,\n  \"findings\": [],\n  \"checked\": [\n"), "{doc}");
    assert!(doc.ends_with("  ]\n}\n"), "{doc}");
    assert_eq!(doc.matches("\"family\": ").count(), 10, "every family but decline, once: {doc}");
    assert_valid_json(&String::from_utf8_lossy(&out.stdout));
}

/// An advisory-carrying project: the finding's three fields rendered as
/// data. The fixture is cylinder_bore.cam again — its finding is a
/// POST-generation family, so this pin also proves the JSON rendering runs
/// the whole pipeline rather than stopping at validate.
#[test]
fn test_check_json_renders_family_subject_and_message() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check=json", "cylinder_bore.cam"])
        .output()
        .expect("run nacs --check=json");
    assert!(out.status.success(), "an advisory is not a rejection");
    assert!(out.stderr.is_empty(), "stderr stays silent — the findings live in the document");
    let doc = String::from_utf8_lossy(&out.stdout);
    assert_valid_json(&doc);
    assert!(doc.contains("\"would_emit\": true"), "an advisory still emits: {}", doc);
    assert!(doc.contains("\"family\": \"entry\""), "the family is data: {}", doc);
    assert!(
        doc.contains("\"subject\": {\"kind\": \"op\", \"index\": 0, \"feature\": \"Bore\"}"),
        "the subject names the op it judges: {}",
        doc
    );
    assert!(
        doc.contains("\"message\": \"Finish on 'Bore' with T5:")
            && doc.contains("meets standing material"),
        "the message is the prose the family always said: {}",
        doc
    );
}

/// A rejecting project: `would_emit` false, the error carried as the
/// one-line message it already is (the envelope — stage, per-site codes —
/// stays the deferred later pass, DECISIONS.md fork 4 pricing), exit
/// nonzero like `--check`, and the document still the only output.
#[test]
fn test_check_json_carries_the_refusal_as_its_one_line_message() {
    let main_src = fs::read_to_string("profile_pocket_clear.cam/main.part").expect("read main.part");
    let huge_tool = main_src.replace("dia=10.0", "dia=90.0");
    let path = std::env::temp_dir().join(format!("nacs_check_json_refuse_{}.part", std::process::id()));
    fs::write(&path, huge_tool).expect("write temp .part");

    let json = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check=json"])
        .arg(&path)
        .output()
        .expect("run nacs --check=json");
    let prose = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check"])
        .arg(&path)
        .output()
        .expect("run nacs --check");
    fs::remove_file(&path).ok();

    assert!(!json.status.success(), "a rejecting project must exit nonzero under --check=json");
    assert_eq!(
        json.status.code(),
        prose.status.code(),
        "the exit code semantics are --check's, rendering-independent"
    );
    assert!(json.stderr.is_empty(), "the error lives in the document, not on stderr");
    let doc = String::from_utf8_lossy(&json.stdout);
    assert_valid_json(&doc);
    assert!(doc.contains("\"would_emit\": false"), "rejected: {}", doc);
    assert!(
        doc.contains("\"refusal\": \"error: ") && doc.contains("too small"),
        "the error is the prose mode's one-line message: {}",
        doc
    );
}

/// A project that does not parse still gets a document: the check result is the
/// artifact even when the check result is "this does not parse", so a caller
/// always has one parseable answer. No `Part` exists to lint, and the empty
/// findings list says so.
#[test]
fn test_check_json_renders_a_parse_refusal_too() {
    let path = std::env::temp_dir().join(format!("nacs_check_json_parse_{}.part", std::process::id()));
    fs::write(&path, "garbage line\n").expect("write temp .part");
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check=json"])
        .arg(&path)
        .output()
        .expect("run nacs --check=json");
    fs::remove_file(&path).ok();

    assert!(!out.status.success());
    assert!(out.stderr.is_empty(), "the error lives in the document, not on stderr");
    let doc = String::from_utf8_lossy(&out.stdout);
    assert_valid_json(&doc);
    assert!(doc.contains("\"would_emit\": false"), "rejected: {}", doc);
    assert!(
        doc.contains("\"refusal\": \"parse error: "),
        "the parse error is the prose line, carried whole: {}",
        doc
    );
    assert!(doc.contains("\"findings\": []"), "no Part existed to lint: {}", doc);
}

/// The escaping pin, through a REAL message: a feature name is any
/// non-whitespace token, so `"` and `\` reach the finding's message and
/// subject through the front door. The document must stay valid JSON with
/// both escaped.
#[test]
fn test_check_json_escapes_quotes_and_backslashes_from_a_feature_name() {
    let main_src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read main.part");
    // Only the standalone name — `CylinderBore` the type must survive.
    let quoted = main_src
        .replace("feature Bore ", "feature Bo\"r\\e ")
        .replace("surface=Bore", "surface=Bo\"r\\e");
    let path = std::env::temp_dir().join(format!("nacs_check_json_escape_{}.part", std::process::id()));
    fs::write(&path, quoted).expect("write temp .part");
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check=json"])
        .arg(&path)
        .output()
        .expect("run nacs --check=json");
    fs::remove_file(&path).ok();

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let doc = String::from_utf8_lossy(&out.stdout);
    assert_valid_json(&doc);
    assert!(
        doc.contains("\"feature\": \"Bo\\\"r\\\\e\""),
        "the subject's name is escaped: {}",
        doc
    );
    assert!(
        doc.contains("Finish on 'Bo\\\"r\\\\e' with T5"),
        "the message's quoted name is escaped: {}",
        doc
    );
}

/// An unknown rendering rejects by name at the command line (exit 2, a
/// usage error — not a check check result): left unhandled it would reach the
/// path reader, which would blame a missing file for a malformed flag.
#[test]
fn test_check_json_unknown_rendering_refuses_by_name() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check=yaml", "plate.cam"])
        .output()
        .expect("run nacs");
    assert_eq!(out.status.code(), Some(2), "a malformed command line is exit 2");
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("--check=yaml") && err.contains("--check=json"),
        "the error names the flag and the renderings: {}",
        err
    );
}

// ---- the gate floor and the MCP session (DECISIONS.md "the agent surface —
// every verb is a flag first…", forks 5A/1B/2A) -------------------------------

/// A clean facing program plus a heavy-skin Clear chained into a Finish: one
/// engagement finding, advisory at declared modes, rejected under the floor.
const GATES_HEAVY_SRC: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
    stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n\
    tool T1 Endmill(dia=10.0)\n\
    feature Bore CylinderBore(dia=30.0, extent=10.0, x=30.0, y=30.0)\n\
    run Clear(tool=T1, profile=Bore, stepover=5.0, stock_to_leave=5.0)\n\
    run Side(tool=T1, surface=Bore, stepover=5.0)\n";

#[test]
fn test_gates_gate_escalates_the_declared_advisory_to_a_refusal() {
    // Temp project dirs in this suite must NOT end in `.cam`: the corpus
    // scanners (the containment remainder, the tail list) walk repo-root
    // `*.cam` in PARALLEL threads, and a temp dir mid-creation or
    // mid-removal is a NotFound crash — a race this suite hit for real
    // (DECISIONS.md → "work offsets land…").
    let dir = "gates_cli_test_dir";
    fs::create_dir_all(dir).unwrap();
    fs::write(format!("{dir}/main.part"), GATES_HEAVY_SRC).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_nacs")).args(args).output().expect("run nacs")
    };

    // At the declared modes the finding is a warning and the program emits.
    let bare = run(&[dir]);
    assert_eq!(bare.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&bare.stderr).contains("warning:"));

    // `--gates declared` is the same thing said out loud.
    let declared = run(&["--gates", "declared", dir]);
    assert_eq!(declared.status.code(), Some(0));
    assert_eq!(bare.stdout, declared.stdout, "the explicit default changes nothing");

    // The floor: the same finding rejects, before any warning is voiced,
    // and no partial G-code exists.
    let floored = run(&["--gates", "gate", dir]);
    assert_eq!(floored.status.code(), Some(1));
    assert!(floored.stdout.is_empty(), "an error leaves no partial program");
    let err = String::from_utf8_lossy(&floored.stderr);
    assert!(
        err.starts_with("error: engagement gate:") && !err.contains("warning:"),
        "the floor rejects like the file-declared gate would: {err}"
    );

    // The JSON rendering carries the floor error as the check result, findings
    // [] — the declared-gate error's shape, one flag later.
    let json = run(&["--check=json", "--gates", "gate", dir]);
    assert_eq!(json.status.code(), Some(1));
    let doc = String::from_utf8_lossy(&json.stdout);
    assert!(
        doc.contains("\"would_emit\": false")
            && doc.contains("engagement gate:")
            && doc.contains("\"findings\": []"),
        "{doc}"
    );

    // A level nobody reads is a usage error, not a silently-ignored flag.
    let bad = run(&["--gates", "loud", dir]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--gates"));

    fs::remove_dir_all(dir).unwrap();
}

/// `--tool-line` is one artifact too: one declaration line, and no project.
///
/// The mode's errors are unit-tested where they live (`assembly::tests`);
/// what only the binary can answer is that the flag is wired — that it joins
/// the exclusion rule, that it never asks for a `.part`, and that what lands
/// on stdout is exactly the line and nothing else.
#[test]
fn test_tool_line_is_one_artifact_and_asks_for_no_project() {
    let nacs = env!("CARGO_BIN_EXE_nacs");
    let clash = Command::new(nacs)
        .args(["--tool-line", "--assembly-report", "tests/fixtures/sample.catalog"])
        .output()
        .expect("run nacs");
    assert_eq!(clash.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&clash.stderr).contains("one artifact each"));

    let out = Command::new(nacs)
        .args(["--tool-line", "tests/fixtures/sample.catalog", "3/8-carbide", "holder=er32-standard", "stickout=45"])
        .output()
        .expect("run nacs");
    assert_eq!(out.status.code(), Some(0));
    let line = String::from_utf8_lossy(&out.stdout);
    assert_eq!(line.lines().count(), 1, "the artifact is ONE line: {line}");
    assert!(
        line.starts_with("tool T? Endmill(") && line.contains("flute_count=4"),
        "{line}"
    );

    // A cutter the wall does not carry is a nonzero exit with a reason, never
    // an empty stdout that reads as success.
    let miss = Command::new(nacs)
        .args(["--tool-line", "tests/fixtures/sample.catalog", "1/2-carbide"])
        .output()
        .expect("run nacs");
    assert_eq!(miss.status.code(), Some(1));
    assert!(miss.stdout.is_empty(), "an error writes no line");
    assert!(String::from_utf8_lossy(&miss.stderr).contains("no cutter named"));
}

/// `--mcp` is one artifact like the rest: the session.
#[test]
fn test_mcp_joins_the_one_artifact_rule() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--mcp", "--check", "plate.cam"])
        .output()
        .expect("run nacs");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("one artifact each"));
}

/// The lean build keeps the flag but not the server — the `--view` pattern.
#[cfg(not(feature = "agent"))]
#[test]
fn test_mcp_refuses_by_name_in_the_lean_build() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).arg("--mcp").output().expect("run nacs");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("without the agent feature"),
        "the error names the feature, not an unexpected argument"
    );
}

/// The review's flags, the same pattern (Phase 17 lane A slice 2): the lean
/// build has no JSON reader, so `--review` and `--accept` refuse naming the
/// features — never "unexpected argument", never a missing-file blame. The
/// review's own pins live in tests/review_tests.rs, gated whole-file the
/// way `src/review.rs` is; this is the one pin only the lean leg can hold.
#[cfg(not(any(feature = "frontend", feature = "agent")))]
#[test]
fn test_review_refuses_by_name_in_the_lean_build() {
    for flags in [vec!["--review", "x.json"], vec!["--accept", "F1", "x.json"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(&flags).output().expect("run nacs");
        assert_eq!(out.status.code(), Some(1));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("without the frontend or agent feature"), "the error names the features: {err}");
        assert!(err.contains(flags[0]), "the error names the flag: {err}");
    }
}

/// The wire, end to end: a real client session over the spawned binary —
/// initialize, the initialized notification (silence), tools/list, a check
/// call on a shipped fixture — then EOF, and the server hangs up clean.
#[cfg(feature = "agent")]
#[test]
fn test_mcp_wire_end_to_end() {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .arg("--mcp")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn nacs --mcp");
    {
        let stdin = child.stdin.as_mut().expect("stdin");
        write!(
            stdin,
            "{}\n{}\n{}\n{}\n",
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"check","arguments":{"path":"plate.cam"}}}"#,
        )
        .expect("write requests");
    }
    let out = child.wait_with_output().expect("wait");
    assert_eq!(out.status.code(), Some(0), "EOF is a clean hangup");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "three requests with ids, three responses (the notification is silence): {stdout}");
    assert!(lines[0].contains("\"protocolVersion\":\"2025-06-18\""), "{}", lines[0]);
    assert!(lines[1].contains("\"name\":\"check\""), "{}", lines[1]);
    assert!(
        lines[2].contains("\"id\":2") && lines[2].contains(r#"\"would_emit\": true"#),
        "the shipped fixture checks clean over the wire: {}",
        lines[2]
    );
}

// ---- coolant as a modeled setting (DECISIONS.md "coolant as a modeled
// setting…", slices 0–1) ------------------------------------------------------

#[test]
fn test_floor_dry_golden() {
    // Slice 0's fixture: the first program that can say "no coolant". The
    // dry block carries no M08; the per-op wet override transitions
    // off-then-on at the op boundary; the end keeps its unconditional M09.
    let main_src = fs::read_to_string("floor_dry.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse floor_dry.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/floor_dry.golden.gcode").expect("failed to read floor_dry golden");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    // The block: M03 S… straight to G00 G43 — no coolant word between.
    assert!(
        generated_gcode.contains("M03 S3000.000\nG00 G43"),
        "a dry tool's block carries no coolant word"
    );
    // The boundary: off-then-on, exactly once, where the override begins.
    assert!(generated_gcode.contains("M09\nM08\n"), "the wet override transitions at the op boundary");

    // The undeclared twin: strip both declarations and the program is
    // always-flood again — M08 in the block, no mid-program transition.
    let bare = main_src.replace(", coolant=off", "").replace(", coolant=flood", "");
    let twin = gcode::program(&parse::parse(&bare).expect("twin parses")).expect("twin generates");
    assert!(twin.contains("M03 S3000.000\nM08\nG00 G43"), "undeclared = today's block");
    assert_eq!(twin.matches("M09").count(), 1, "no transition: the only M09 is the parting one");
}

#[test]
fn test_cylinder_bore_mist_golden() {
    // Slice 1's fixture: the machine fixture's mist twin. One menu line
    // (`coolant_mist M07`), one declared kind, and the golden differs from
    // cylinder_bore_machine.golden.gcode by exactly the coolant word.
    let main_src = fs::read_to_string("cylinder_bore_mist.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("cylinder_bore_mist.cam")))
        .expect("failed to parse cylinder_bore_mist.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/cylinder_bore_mist.golden.gcode").expect("failed to read mist golden");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    let machine_golden = fs::read_to_string("tests/cylinder_bore_machine.golden.gcode").expect("failed to read machine golden");
    assert_eq!(
        golden_gcode.replace("\nM07\n", "\nM08\n"),
        machine_golden,
        "the mist twin differs from the flood twin by exactly the coolant word"
    );
}

#[test]
fn test_an_unnamed_menu_kind_refuses_and_a_custom_slot_licenses() {
    // The fixture's own claim, mechanically: strip the menu line and the
    // same program rejects at validate, naming the key.
    let main_src = fs::read_to_string("cylinder_bore_mist.cam/main.part").expect("read fixture");
    let machine_src = fs::read_to_string("cylinder_bore_mist.cam/fanuc_mist.machine").expect("read machine");
    let dir = "coolant_menu_test_dir";
    fs::create_dir_all(dir).unwrap();
    fs::write(format!("{dir}/main.part"), main_src.replace("fanuc_mist.machine", "bare.machine")).unwrap();
    let stripped: String = machine_src.lines().filter(|l| !l.starts_with("coolant_")).map(|l| format!("{l}\n")).collect();
    fs::write(format!("{dir}/bare.machine"), stripped).unwrap();
    let src = fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let err = parse::parse_project_with_base(&src, None, Some(std::path::Path::new(dir)))
        .expect_err("a declared machine without the menu key must reject the kind");
    assert!(err.contains("coolant 'mist' has no word") && err.contains("coolant_mist"), "got: {err}");

    // A custom slot end to end (the blessing's amendment): menu-only, and
    // with a word it emits that word in the M08 slot.
    fs::write(
        format!("{dir}/bare.machine"),
        "coolant_custom1 M88\n",
    )
    .unwrap();
    let custom = src.replace("coolant=mist", "coolant=custom1");
    let part = parse::parse_project_with_base(&custom, None, Some(std::path::Path::new(dir)))
        .expect("a named custom slot licenses the kind");
    let g = gcode::program(&part).expect("generates");
    fs::remove_dir_all(dir).unwrap();
    assert!(g.contains("\nM88\n"), "the custom channel's word rides the block: {}", &g[..400]);
    assert!(!g.contains("\nM08\n"), "and the flood word is nowhere — the kind was custom, not flood");
}

// ---- the restart ladder (DECISIONS.md "N-words…", both slices) --------------

#[test]
fn test_floor_restart_golden() {
    // Three rungs, three first-line classes — the op's own state
    // re-establishment when it has one, its title otherwise — and one
    // deliberately unnumbered op, because the ladder is sparse by design.
    let main_src = fs::read_to_string("floor_restart.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse floor_restart.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/floor_restart.golden.gcode").expect("failed to read restart golden");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    assert!(generated_gcode.contains("\nN10 M06 T1\n"), "the first block's rung is its tool change");
    assert!(generated_gcode.contains("\nN15 (--- floor: Mid with T1 ---)\n"), "a state-neutral op's rung is its title");
    assert!(generated_gcode.contains("\nN30 M09\n"), "a tool-changing op's rung is its leaving retract — the full re-establishment executes from the rung");
    assert!(generated_gcode.contains("\nS2400.000\n"), "the unnumbered op's S line carries no label — the ladder is sparse");
    assert_eq!(generated_gcode.matches("\nN").count(), 3, "exactly the three declared rungs");

    // The stripped twin emits byte-identically minus the three labels: the
    // rung is a label on the text, never a change to the motion.
    let bare = main_src.replace(", n=10", "").replace(", n=15", "").replace(", n=30", "");
    let twin = gcode::program(&parse::parse(&bare).expect("twin parses")).expect("twin generates");
    let unlabeled: String = generated_gcode
        .lines()
        .map(|l| format!("{}\n", gcode::strip_n_word(l)))
        .collect();
    assert_eq!(unlabeled.trim(), twin.trim(), "label off, twin's bytes exactly");
}

// ---- work offsets (DECISIONS.md "work offsets…", both slices) ---------------

#[test]
fn test_floor_offset_golden() {
    // The K1 acceptance shape, fourth verse: a declared selection changes
    // exactly one word, pinned line by line against the stripped twin.
    let main_src = fs::read_to_string("floor_work_offset.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse floor_work_offset.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/floor_work_offset.golden.gcode").expect("failed to read offset golden");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    let twin_src = main_src.replace("work_offset G55\n", "");
    let twin = gcode::program(&parse::parse(&twin_src).expect("twin parses")).expect("twin generates");
    let diffs: Vec<(&str, &str)> = generated_gcode
        .lines()
        .zip(twin.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(
        diffs,
        vec![("G90 G55 G17", "G90 G54 G17")],
        "the selection is one word on one line, and nothing else can move"
    );
}

/// `work_offset G54 G55 G56` runs the program at each register: every tool
/// cuts all three parts before the next tool change, each part gets the
/// one-part program's moves, and a restart label stays on one line.
#[test]
fn test_several_offsets_golden() {
    let main_src = fs::read_to_string("several_offsets.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse several_offsets.cam");
    let (text, emissions) = gcode::program_with_moves(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/several_offsets.golden.gcode").expect("failed to read golden");
    assert_eq!(text.trim(), golden.trim());

    let twin_src = main_src.replace("work_offset G54 G55 G56\n", "");
    let (twin, twin_emissions) = gcode::program_with_moves(&parse::parse(&twin_src).expect("twin parses")).expect("twin generates");
    let count = |t: &str, word: &str| t.lines().filter(|l| gcode::strip_n_word(l).starts_with(word)).count();
    assert_eq!(count(&text, "M06"), count(&twin, "M06"), "no tool change is added");
    assert_eq!(count(&text, "M06"), 2);
    // Each tool selects each register once, in the order written.
    let selected: Vec<&str> = text.lines().filter(|l| matches!(*l, "G54" | "G55" | "G56")).collect();
    assert_eq!(selected, ["G54", "G55", "G56", "G54", "G55", "G56"]);
    // Every copy of every op holds the one-part program's moves.
    let lines: Vec<&str> = text.lines().collect();
    for (em, one) in emissions.iter().zip(&twin_emissions) {
        assert_eq!(em.moves, one.moves, "op {}", em.op_idx);
        assert_eq!(em.copies().count(), 3, "op {}", em.op_idx);
        for (block, moves_start) in em.copies() {
            for (k, mv) in em.moves.iter().enumerate() {
                assert!(block.contains(&(moves_start + k)));
                assert_eq!(gcode::strip_n_word(lines[moves_start + k]), mv);
            }
        }
    }
    // The first op ran at 3000 and the second at 2400: a later part starts
    // by setting 3000 again.
    let at = lines.iter().position(|l| *l == "G55").unwrap();
    assert_eq!(lines[at + 1], "S3000.000");
    for n in ["N10 ", "N20 ", "N30 "] {
        assert_eq!(lines.iter().filter(|l| l.starts_with(n)).count(), 1, "{n}labels one line");
    }
    // The clock counts all three parts.
    let (three, one) = (nacs::cut_time::time_report(&part).unwrap(), nacs::cut_time::time_report(&parse::parse(&twin_src).unwrap()).unwrap());
    let cutting = |r: &nacs::cut_time::TimeReport| r.rows.iter().filter(|row| row.op_idx.is_some()).map(|row| row.cutting_s).sum::<f64>();
    assert!((cutting(&three) - 3.0 * cutting(&one)).abs() < 1e-6, "{} vs 3 x {}", cutting(&three), cutting(&one));
}

#[test]
fn test_floor_offset_ext_golden() {
    // The extended register under its declaring machine: the vendor's
    // two-word spelling in the setup slot — and without the menu line the
    // same program rejects, naming the key.
    let main_src = fs::read_to_string("floor_work_offset_ext.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_work_offset_ext.cam")))
        .expect("failed to parse floor_work_offset_ext.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/floor_work_offset_ext.golden.gcode").expect("failed to read ext golden");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert!(generated_gcode.contains("\nG90 G54.1 P7 G17\n"), "the vendor's two-word spelling");

    let machine_src = fs::read_to_string("floor_work_offset_ext.cam/tombstone.machine").expect("read machine");
    let dir = "wo_ext_strip_test_dir";
    fs::create_dir_all(dir).unwrap();
    fs::write(format!("{dir}/main.part"), main_src.replace("tombstone.machine", "bare.machine")).unwrap();
    let stripped: String = machine_src.lines().filter(|l| !l.starts_with("extended_offsets")).map(|l| format!("{l}\n")).collect();
    fs::write(format!("{dir}/bare.machine"), stripped).unwrap();
    let src = fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let err = parse::parse_project_with_base(&src, None, Some(std::path::Path::new(dir)))
        .expect_err("without the menu the register is not sayable");
    fs::remove_dir_all(dir).unwrap();
    assert!(err.contains("extended_offsets"), "the error names the menu key: {err}");
}

// ---- reach shortfall: the assembly against the depth (DECISIONS.md → "K2,
// the tool body", fork 4c, built as its own follow-up) ----

/// The acceptance, on a fixture whose own header already stated the condition
/// in prose: `spline_funnel_holder.cam` says "the stickout is 8mm
/// against a cavity 11.14mm deep, so the holder's nose face ends up below the
/// stock top". That sentence is now a check.
#[test]
fn test_a_short_assembly_is_reported_against_the_depth_it_works_in() {
    let src = fs::read_to_string("spline_funnel_holder.cam/main.part")
        .expect("read spline_funnel_holder.cam");
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("the advisory does not reject");

    let found = stock::reach_findings(&part, &emitted_moves);
    assert_eq!(found.len(), 1, "one op, one shortfall: {:?}", found);
    let m = &found[0].message;
    // The numbers are the TIP's, not the cavity floor's: a ball's lowest point
    // hangs `r` below the coordinates the emitter writes, so the assembly has
    // to span further than the surface is deep. 8.000 of tool against 20.051
    // of tip depth leaves 12.051 of holder inside the stock.
    assert!(
        m.contains("8.000mm stickout") && m.contains("20.051mm") && m.contains("12.051mm"),
        "the finding must carry all three numbers: {m}"
    );
    assert!(m.contains("Vortex"), "and name the feature: {m}");

    // The control, run the way the K2 fixtures run theirs: give the assembly
    // the length it needs and the voice goes. Nothing else about the program
    // changes — the same funnel, the same ball, the same holder.
    let long = src.replace("stickout=8.0", "stickout=25.0");
    assert_ne!(long, src, "the substitution must actually substitute");
    let part = parse::parse_project_with_base(&long, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");
    assert!(stock::reach_findings(&part, &emitted_moves).is_empty(), "a reaching assembly is silent");
}

/// The reason fork 4c called this a NEW check rather than an existing lint
/// seeing more: it never consults the material model, so it speaks in an
/// already-open hole where every foul voice is correctly silent.
#[test]
fn test_reach_speaks_where_the_material_model_has_nothing_to_say() {
    // A 40mm bore 12mm deep, cut by a 10mm endmill that is declared to stand
    // only 8mm out of its holder. The bore is wide open — the tool has 15mm of
    // clearance all round — so nothing is standing at the nose and the foul
    // lints have no material to report. The assembly is still 4mm too short.
    let src = fs::read_to_string("cylinder_bore.cam/main.part").expect("read cylinder_bore.cam");
    let short = src.replace("Endmill(dia=10.0)", "Endmill(dia=10.0, stickout=8.0)");
    assert_ne!(short, src, "the substitution must actually substitute");
    let part = parse::parse(&short).expect("the declaration parses");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");

    let found = stock::reach_findings(&part, &emitted_moves);
    assert_eq!(found.len(), 2, "both ops work the same depth: {:?}", found);
    assert!(
        found[0].message.contains("12.000mm") && found[0].message.contains("4.000mm"),
        "got: {}", found[0].message
    );

    // The other voices, on the same program, saying nothing — which is correct
    // of them and is exactly the blindness this check closes.
    assert!(
        stock::body_foul_warnings(&part).is_empty(),
        "no material stands at the body: this is an open bore"
    );
    assert!(
        nacs::vox::body_foul_findings(&part, &emitted_moves).is_empty(),
        "and the field voice agrees"
    );

    // And the boundary: a stickout exactly equal to the depth is TOUCHING the
    // stock's top face, not entering it, so it is legal — the same
    // equality-is-legal call the neck fields made against `min_projection`.
    let exact = src.replace("Endmill(dia=10.0)", "Endmill(dia=10.0, stickout=12.0)");
    let part = parse::parse(&exact).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");
    assert!(stock::reach_findings(&part, &emitted_moves).is_empty(), "a nose face ON the top face is legal");
}

/// An undeclared `stickout` is today's behavior, byte for byte — the house law
/// every K2 field is built on, checked once more where a new voice could have
/// broken it.
#[test]
fn test_an_undeclared_stickout_is_silent_across_the_whole_corpus() {
    let mut checked = 0;
    for entry in fs::read_dir(".").expect("read the fixture corpus") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let main = path.join("main.part");
        let Ok(src) = fs::read_to_string(&main) else { continue };
        if src.contains("stickout=") {
            continue; // the declared ones are the two tests above
        }
        let Ok(part) = parse::parse_project_with_base(&src, None, Some(&path)) else { continue };
        let Ok((_, emitted_moves)) = gcode::program_with_moves(&part) else { continue };
        assert!(
            stock::reach_findings(&part, &emitted_moves).is_empty(),
            "{} declares no stickout and must stay silent",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 20, "the sweep must actually cover the corpus, saw {checked}");
}

// ---- the fifth certificate, narrowed by the corridor (DECISIONS.md → "the
// fifth certificate and the corridor — two shipped checks disagree about a
// nose in a mouth, and the one that can reject is the one measuring less") ----

/// A counterbore, which is where the certificate and the corridor actually
/// disagreed: a seat is a mouth a nose legitimately sits in, and the cavity's
/// EXTRA WIDTH there is the room the nose spends. Constructed here rather than
/// shipped as a fixture, because a program whose only purpose is to be
/// exonerated is a program chosen to prove the change.
fn seated_nose(cbore_depth: &str, holder: &str) -> String {
    format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 40\nspindle 3000\n\
         stock Box(w=120, l=80, h=30, x=0, y=0, z=0)\n\
         tool T4 Endmill(dia=40.0)\n\
         tool T7 Endmill(dia=8.0, stickout=22.0{holder})\n\
         feature Seat CylinderBore(dia=16.0, extent=25.0, x=60.0, y=40.0, cbore_dia=40.0, cbore_depth={cbore_depth})\n\
         run End(points=[Seat], tool=T4, cycle=Simple)\n\
         run Side(tool=T7, surface=Seat, stepover=5.0)\n"
    )
}

/// A stepped cavity DECLARED as two coaxial bores — one 60mm mouth 12mm deep
/// over a 30mm core 14mm deep, which is the shape `cylinder_bore_rest_flutes.cam`
/// ships and the shape `corridor_of` used to read as whichever half was named
/// (`DECISIONS.md` → "the multi-feature cavity…"). The tool is short: its nose
/// face stands 2.000mm below the stock's top, inside the counterbore.
///
/// Built here rather than shipped, for the reason the counterbore above it
/// gives: a program whose only purpose is to be exonerated is a program chosen
/// to prove the change.
fn stacked_cavity(runs: &str) -> String {
    format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
         tool T5 Endmill(dia=10.0, flute_len=10.0, shank_dia=10.0, stickout=12.0, \
         holder=\"ER32 slim nose\", holder_nose_dia=32.0, holder_nose_len=40.0, holder_body_dia=50.0)\n\
         feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
         feature Core CylinderBore(dia=30.0, extent=14.0, x=50.0, y=50.0)\n{runs}"
    )
}

/// THE ACCEPTANCE (`DECISIONS.md` → "the multi-feature cavity…"): the stake was
/// a ERROR, not an advisory.
///
/// Before this landed, the program below was rejected under `reach gate` while
/// `check_moves` — which also gates, and which measures the material as it
/// actually stands — had already let the same move through. Two gating checks,
/// one nose, one mouth, and the one that measured MORE was the one being
/// ignored.
#[test]
fn test_a_stepped_cavity_declared_as_two_features_is_read_whole() {
    // The order that opens it: the 60mm mouth is cleared, THEN the core.
    let opened = stacked_cavity(
        "run Clear(tool=T5, profile=Bore, stepover=4.0)\n\
         run Clear(tool=T5, profile=Core, stepover=4.0)\n",
    );
    assert_eq!(
        reach_voices(&opened),
        0,
        "the nose stands in a hole 60mm across that the op before it opened"
    );
    let gated = parse::parse(&format!("reach gate\n{opened}")).expect("parse gated");
    assert!(gcode::program(&gated).is_ok(), "and the gate must let the program through");

    // The other order — the corpus's own order — is where composing would be a
    // LIE, and it needs no rule of its own: the entry gate rejects the program
    // by name, citing the very material the composition declined to count.
    let closed = stacked_cavity(
        "run Clear(tool=T5, profile=Core, stepover=4.0)\n\
         run Clear(tool=T5, profile=Bore, stepover=4.0)\n",
    );
    let part = parse::parse(&closed).expect("parse");
    let e = gcode::program(&part).expect_err("a nose in standing material is rejected");
    assert!(e.contains("standing stock"), "and the error is the entry family's: {e}");

    // Same again for a neighbour nothing ever cuts: a declared feature is a
    // statement about the finished part, not about what is standing while it
    // is made.
    let never = stacked_cavity("run Clear(tool=T5, profile=Core, stepover=4.0)\n");
    let part = parse::parse(&never).expect("parse");
    let e = gcode::program(&part).expect_err("an unmachined neighbour is not room");
    assert!(e.contains("standing stock"), "and the same check says so: {e}");
}

/// The reach findings a program raises, for a test that only cares how many.
fn reach_voices(src: &str) -> usize {
    let part = parse::parse(src).expect("the program parses");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("and emits");
    stock::reach_findings(&part, &emitted_moves).len()
}

/// THE ACCEPTANCE, both halves (fork 6A). The certificate's own founding
/// fixture must STILL FIRE — the narrowing costs it nothing it was ever right
/// about — and a nose that legitimately sits in a counterbore must go silent.
#[test]
fn test_the_narrowing_exonerates_a_nose_with_room_and_nothing_else() {
    // HALF ONE: the fixture that made this check, and it is not silenced. The
    // reason is a NUMBER now rather than an envelope — its nose face sits
    // 4.943mm below the stock top, where the funnel's wall leaves about 6.1mm
    // of room against a nose of radius 16.000. The corridor CAN measure this
    // one and still rejects to exonerate it, which is what makes the narrowing
    // safe to do at all.
    let src = fs::read_to_string("spline_funnel_holder.cam/main.part").expect("read");
    let part = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");
    assert_eq!(stock::reach_findings(&part, &emitted_moves).len(), 1, "the acceptance fixture still fires");

    // Measured through the two homes the check itself reads, so this pin moves
    // if either of them does.
    let cavity = nacs::assembly::corridor_of(&part, &part.features["Vortex"], nacs::assembly::Opened::Declared).expect("a cavity");
    let shape = nacs::assembly::CutterShape::of_tool(&part.tools[&10]);
    let placed = cavity.corridor.place(&shape, cavity.reach).expect("a ball works it");
    let (deepest, excursion) = placed
        .locus
        .iter()
        .copied()
        .fold((0.0f64, 0.0f64), |a, b| if b.0 > a.0 { b } else { a });
    let room = placed.corridor.room_at(deepest - 8.0, shape.radius, excursion);
    // The room the staircase reports at the nose face is an inner bound of
    // the wall (every band takes its own narrowest radius), and a 16 mm nose
    // does not fit in it: that is why the fixture still fires.
    assert!(room > 0.0 && room < 16.0, "16.000mm of nose does not fit the room at the nose face: {room}");

    // HALF TWO: a nose that DOES have its room. A 16mm bore 25mm deep under a
    // 40mm counterbore 8mm deep, finished by an 8mm endmill standing 22mm out
    // of a 14mm nose. The tip reaches 25.000 and the stickout is 22.000, so the
    // nose face is 3.000mm inside the stock — inside the SEAT, where the
    // cavity is 20.000mm in radius and the tool's axis only 4.000mm off it.
    let holder = ", holder=\"slim nose\", holder_nose_dia=14.0, holder_nose_len=30.0, holder_body_dia=50.0";
    assert_eq!(
        reach_voices(&seated_nose("8.0", holder)),
        0,
        "a 7.000mm nose seated in a 40mm counterbore has its room, and the certificate withdraws"
    );

    // THE CONTROL, and it is the same program with ONE number moved: make the
    // seat 2mm deep instead of 8mm and the nose face lands below it, in the
    // 16mm bore — where the room is the cutter's own 4.000mm radius and a
    // 7.000mm nose does not fit. The exoneration is a measurement of the
    // cavity's extra width, not a category the word "counterbore" buys.
    assert_eq!(
        reach_voices(&seated_nose("2.0", holder)),
        1,
        "past the seat the nose is in the wall again, and the certificate says so"
    );
}

/// THE ACCEPTANCE THAT MATTERED: a shop that turns `reach gate` on and then
/// FOLLOWS THE REPORT is no longer rejected for taking the advice.
///
/// Nothing here is typed. The recommendation comes out of the shipped catalog
/// through `--assembly-report`, the tool line comes out of the recommendation,
/// and the program is that line pasted into the fixture the whole disagreement
/// was found on — so if either side of the pair ever drifts, this test is where
/// the two stop meeting.
#[test]
fn test_a_program_built_from_the_report_own_advice_passes_the_certificate() {
    let report = nacs::assembly::report_from_args(&[
        "tests/fixtures/sample.catalog".to_string(),
        "part=spline_funnel_holder.cam/main.part".to_string(),
        "feature=Vortex".to_string(),
    ])
    .expect("the shipped catalog reports on the shipped funnel");

    // The row this entry exists for, and the line it proposes right under it.
    let lines: Vec<&str> = report.lines().collect();
    let row = lines
        .iter()
        .position(|l| l.contains("[nose enters the opening]"))
        .expect("the funnel is where a slim nose earns its keep");
    let proposed = lines[row + 1].trim();
    assert!(proposed.starts_with("tool T? Ballmill("), "the row's own tool line: {proposed}");
    assert!(proposed.contains("holder_nose_dia="), "with the holder it proposes: {proposed}");

    // Declared as the program's own T10, changing nothing else about it.
    let src = fs::read_to_string("spline_funnel_holder.cam/main.part").expect("read");
    let declared: String = src
        .lines()
        .map(|l| if l.starts_with("tool T10 ") { proposed.replacen("T?", "T10", 1) } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(declared.contains("holder=\"slim-nose\""), "the substitution must actually substitute");

    let part = parse::parse_project_with_base(&declared, None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("the followed advice parses");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("and emits");
    assert!(
        stock::reach_findings(&part, &emitted_moves).is_empty(),
        "the report's own recommendation must not be rejected by the certificate: {:?}",
        stock::reach_findings(&part, &emitted_moves)
    );

    // And the gate, which is the half with teeth: the same program under the
    // directive that used to reject it.
    let gated = parse::parse_project_with_base(&format!("reach gate\n{declared}"), None, Some(std::path::Path::new("spline_funnel_holder.cam"))).expect("parse gated");
    assert!(gcode::program(&gated).is_ok(), "and `reach gate` must let it through");
}

/// The three conditions are ALL-OR-NOTHING (fork 4A), each taken away in turn
/// from the program the narrowing does silence.
#[test]
fn test_each_missing_condition_leaves_the_finding_standing() {
    let holder = ", holder=\"slim nose\", holder_nose_dia=14.0, holder_nose_len=30.0, holder_body_dia=50.0";
    assert_eq!(reach_voices(&seated_nose("8.0", holder)), 0, "the exonerated program is the baseline");

    // (3) GONE — no nose declared. There is no width to fit, so the envelope
    // claim is all this check has and the stickout is still short. This is the
    // house law the whole K2 family is built on, seen from the other side: an
    // undeclared field is never a guess, and here that means never an
    // exoneration either.
    assert_eq!(
        reach_voices(&seated_nose("8.0", "")),
        1,
        "an undeclared nose cannot be exonerated by a width"
    );

    // (1) GONE — the op's feature is not a cavity at all. A facing pass over a
    // plane: `corridor_of` rejects it by kind, so the certificate keeps
    // under-claiming exactly where it always did.
    let facing = fs::read_to_string("floor_dry.cam/main.part").expect("read floor_dry.cam");
    let short = facing.replace(
        "Endmill(dia=10.0, coolant=off)",
        "Endmill(dia=10.0, coolant=off, stickout=0.5, holder=\"wide\", holder_nose_dia=30.0, holder_nose_len=30.0, holder_body_dia=50.0)",
    );
    assert_ne!(short, facing, "the substitution must actually substitute");
    assert_eq!(reach_voices(&short), 1, "a facing pass states no corridor and is untouched");

    // (2) GONE — the tool is not admissible in the cavity. Asserted on the
    // placement call rather than through a program, and the reason is the point
    // of fork 4A's second condition: `validate` rejects a rod on a curved wall
    // FIRST ("Finish operation requires a Ballmill tool or a custom tool
    // profile", and Clear rejects a funnel outright), so a program with a rod
    // in a funnel does not exist to test. The pairing law is quoted here, not
    // invented, which is why the two errors agree by construction.
    let part = parse::parse_project_with_base(&fs::read_to_string("spline_funnel_holder.cam/main.part").expect("read"), None, Some(std::path::Path::new("spline_funnel_holder.cam")))
        .expect("parse");
    let cavity = nacs::assembly::corridor_of(&part, &part.features["Vortex"], nacs::assembly::Opened::Declared).expect("a cavity");
    let rod = nacs::assembly::CutterShape {
        radius: 3.0,
        ball: false,
        name: "T10".into(),
        kind: "Endmill".into(),
    };
    let err = cavity.corridor.place(&rod, cavity.reach).expect_err("a rod cannot work a curved wall");
    assert!(err.contains("finished by a BALL"), "the pairing law, quoted: {err}");

    // And the same shape, read off a declared tool, is a BALL — the conversion
    // the certificate actually uses.
    assert!(
        nacs::assembly::CutterShape::of_tool(&part.tools[&10]).ball,
        "T10 is declared a Ballmill"
    );
}

// ---- the per-point reach finding (DECISIONS.md → "the per-point reach
// finding — a Drill op works a LIST, so the certificate that names one feature
// can neither exonerate it nor say which hole is short") ----------------------

/// The subjects a program's reach findings name, in the order they are raised.
fn reach_subjects_named(src: &str) -> Vec<String> {
    let part = parse::parse(src).expect("the program parses");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("and emits");
    stock::reach_findings(&part, &emitted_moves)
        .iter()
        .map(|f| match &f.subject {
            parse::FindingSubject::Op { feature, .. } => feature.clone(),
            parse::FindingSubject::Feature { name } => name.clone(),
        })
        .collect()
}

/// ACCEPTANCE, HALF ONE (fork 6A): the split, measured on a shipped fixture
/// plus one declared field.
///
/// `cylinder_bore_form_points.cam` drills three holes across two ops — `[Hole1]`,
/// then `[Hole1,Hole2]` — and before this slice the certificate answered per
/// OPERATION, so it raised **two findings, character for character identical,
/// both `on ''`**: two sentences for three holes, naming none of them. The
/// subject is what a shortfall is about, and for a drill that is a hole.
///
/// No holder is declared here on purpose, so the narrowing cannot speak and
/// the split is what is being measured.
#[test]
fn test_a_drill_reports_the_hole_that_is_short_and_names_it() {
    let src = fs::read_to_string("cylinder_bore_form_points.cam/main.part").expect("read");
    let short = src.replace("Drill(dia=8.0)", "Drill(dia=8.0, stickout=12.0)");
    assert_ne!(short, src, "the substitution must actually substitute");

    // Both holes are 15.000mm deep against a 12.000mm stickout, so all three
    // drilled holes are short — one finding each, named, and the second op
    // raises TWO where an op-level answer could only ever raise one.
    assert_eq!(
        reach_subjects_named(&short),
        vec!["Hole1", "Hole1", "Hole2"],
        "three holes drilled, three findings, each naming its own"
    );

    // THE CONTROL, and it is the half an op-level maximum cannot do: shorten
    // `Hole2` to 8.000mm, inside the same 12.000mm stickout, and it drops out
    // of a finding it was never the subject of — while `Hole1`, drilled by the
    // very same op in the very same run, still fires.
    let mixed = short.replace(
        "feature Hole2 CylinderBore(dia=8.0, extent=15.0",
        "feature Hole2 CylinderBore(dia=8.0, extent=8.0",
    );
    assert_ne!(mixed, short, "the substitution must actually substitute");
    assert_eq!(
        reach_subjects_named(&mixed),
        vec!["Hole1", "Hole1"],
        "the shallow hole is never named"
    );

    // And the number each finding carries is that hole's own depth, read off
    // the emitted moves at that hole's XY — not the op's deepest tip.
    let part = parse::parse(&mixed).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");
    for f in stock::reach_findings(&part, &emitted_moves) {
        assert!(f.message.contains("15.000mm its TIP reaches"), "Hole1's own depth: {}", f.message);
        assert!(f.message.contains("on 'Hole1'"), "and its own name: {}", f.message);
    }
}

/// `cbore.cam` with its drill given an assembly, and its two runs in whatever
/// order the caller states. The seat is the room this item is about: a 16.000mm
/// counterbore 6.000mm deep on a 6.000mm hole 15.000mm deep.
fn counterbored_drill(runs: &str) -> String {
    let src = fs::read_to_string("cbore.cam/main.part").expect("read cbore.cam");
    let assembled = src.replace(
        "tool T7 Drill(dia=6.0)",
        "tool T7 Drill(dia=6.0, flute_len=10.0, shank_dia=6.0, stickout=12.0, \
         holder=\"slim nose\", holder_nose_dia=14.0, holder_nose_len=20.0, holder_body_dia=40.0)",
    );
    assert_ne!(assembled, src, "the substitution must actually substitute");
    let shipped = "run Form(points=[Hole1,Hole2], tool=T7, cycle=Simple)\n\
                   run End(points=[Hole1,Hole2], tool=T4, cycle=Simple)\n";
    assert!(assembled.contains(shipped), "the shipped runs must be the ones replaced");
    assembled.replace(shipped, runs)
}

/// ACCEPTANCE, HALF TWO (fork 6A): the narrowing #195 blessed now reaches a
/// `Drill`, which it never could — `nose_has_room` looked the cavity up by
/// `op.feature_name`, and a `Drill` names a list of points and carries none, so
/// the lookup missed every time and the exoneration was unreachable by
/// construction.
#[test]
fn test_the_narrowing_reaches_a_drill_op() {
    // The 16mm seat cut first, then the 6mm hole through it. The tip reaches
    // 15.000mm against a 12.000mm stickout, so the nose face sits 3.000mm
    // inside the stock — inside the SEAT, which is 8.000mm in radius against a
    // nose of 7.000. Before this slice: rejected, `on ''`, with the room right
    // there to be measured.
    let seat_first = counterbored_drill(
        "run End(points=[Hole1], tool=T4, cycle=Simple)\n\
         run Form(points=[Hole1], tool=T7, cycle=Simple)\n",
    );
    assert_eq!(reach_voices(&seat_first), 0, "the nose is in its seat, and the certificate withdraws");

    // The room, measured through the same two homes the check reads, so this
    // pin moves if either of them does. The op index is the drill's — the
    // certificate has a PROGRAM, so it reads the room an EARLIER op opened.
    let part = parse::parse(&seat_first).expect("parse");
    let cavity = nacs::assembly::corridor_of(
        &part,
        &part.features["Hole1"],
        nacs::assembly::Opened::Before(1),
    )
    .expect("a cavity");
    let shape = nacs::assembly::CutterShape::of_tool(&part.tools[&7]);
    let placed = cavity.corridor.place(&shape, 15.0).expect("a drill in a bore it fits");
    let (tip_depth, excursion) = placed.locus[0];
    let room = placed.corridor.room_at(tip_depth - 12.0, shape.radius, excursion);
    assert!((room - 8.000).abs() < 1e-9, "the seat's own room: {room}");
    assert!(room > 7.000, "which takes a 7.000mm nose — and that is the exoneration");

    // THE MOMENT-QUESTION KEEPS ITS OWNER. The same program in the SHIPPED
    // order — drill first, seat after — is still rejected, and by the gate whose
    // job that is: the seat is not open yet, so the nose is in solid stock and
    // the ENTRY gate says where.
    let drill_first = counterbored_drill(
        "run Form(points=[Hole1], tool=T7, cycle=Simple)\n\
         run End(points=[Hole1], tool=T4, cycle=Simple)\n",
    );
    let part = parse::parse(&drill_first).expect("parse");
    let e = gcode::program(&part).expect_err("a nose in an unopened seat is rejected");
    assert!(e.contains("standing stock at (37.000, 40.000"), "the entry family, by location: {e}");

    // AND THE TWO HOLES OF ONE OP GENUINELY DIFFER, which is why no wording of
    // an op-level finding could have been right. `Hole2`'s seat is a 0.800mm
    // spotface: same op, same tool, same order, and the nose has no room there
    // — the entry gate rejects at Hole2's own centre while the Hole1-only
    // program above emits.
    let both = counterbored_drill(
        "run End(points=[Hole1,Hole2], tool=T4, cycle=Simple)\n\
         run Form(points=[Hole1,Hole2], tool=T7, cycle=Simple)\n",
    );
    let part = parse::parse(&both).expect("parse");
    let e = gcode::program(&part).expect_err("a spotface is not a seat");
    assert!(e.contains("standing stock at (67.000, 40.000"), "and it names the hole's centre: {e}");
}

/// Phase 9, slice 0 (serial; `DECISIONS.md` → "Phase 9 slice 0, serial: the
/// probe learns what it is about…"): the two prose voices the per-point reach
/// slice left standing. `stock::body_foul_warnings` said *"outlives ''"* and
/// `stock::check_moves` rejected with *"Drill on '' with T7"* — each already
/// probed per point and named the XY, so neither measured the wrong thing;
/// each read `op.feature_name` for the SUBJECT, which a drill does not have.
///
/// The same counterbored drill the reach tests use, so the numbers are
/// already on record: a 14.000mm nose on a 6.000mm drill, 12.000mm of
/// stickout against 15.000mm of hole.
#[test]
fn test_the_foul_voices_name_the_hole_they_are_about() {
    // Drill first, seat after: the nose descends into solid stock at Hole1's
    // own centre, and both voices now say so BY NAME.
    let drill_first = counterbored_drill(
        "run Form(points=[Hole1], tool=T7, cycle=Simple)\n\
         run End(points=[Hole1], tool=T4, cycle=Simple)\n",
    );
    let part = parse::parse(&drill_first).expect("parse");
    let e = gcode::program(&part).expect_err("a nose in an unopened seat is rejected");
    assert!(e.starts_with("Form on 'Hole1' with T7"), "the error names the hole, not '': {e}");
    assert!(e.contains("standing stock at (37.000, 40.000"), "at the hole's own centre: {e}");
    let fouls: Vec<_> = stock::body_foul_warnings(&part)
        .into_iter()
        .filter(|f| f.family == parse::FindingFamily::BodyFoul)
        .collect();
    assert_eq!(fouls.len(), 1, "one foul, one voice: {fouls:?}");
    assert!(fouls[0].message.contains("outlives 'Hole1'"), "the advisory names it too: {}", fouls[0].message);
    assert_eq!(
        fouls[0].subject,
        parse::FindingSubject::Op { index: 0, feature: "Hole1".into() },
        "and the finding's SUBJECT is the declared hole"
    );

    // THE CONTROL, which no wording of an op-level finding could state: two
    // holes in one op, one seated (Hole1's 16mm counterbore, cut first) and
    // one merely spotfaced 0.800mm (Hole2). Same op, same tool, same order —
    // the seated hole is silent and the spotfaced one fires, and both voices
    // name Hole2 and only Hole2.
    let both = counterbored_drill(
        "run End(points=[Hole1,Hole2], tool=T4, cycle=Simple)\n\
         run Form(points=[Hole1,Hole2], tool=T7, cycle=Simple)\n",
    );
    let part = parse::parse(&both).expect("parse");
    let e = gcode::program(&part).expect_err("a spotface is not a seat");
    assert!(e.starts_with("Form on 'Hole2' with T7"), "the error names the spotfaced hole: {e}");
    assert!(e.contains("standing stock at (67.000, 40.000"), "at Hole2's own centre: {e}");
    let fouls: Vec<_> = stock::body_foul_warnings(&part)
        .into_iter()
        .filter(|f| f.family == parse::FindingFamily::BodyFoul)
        .collect();
    assert_eq!(fouls.len(), 1, "one foul per op, and it is the spotfaced hole's: {fouls:?}");
    assert!(fouls[0].message.contains("outlives 'Hole2'"), "{}", fouls[0].message);
    assert!(!fouls[0].message.contains("Hole1"), "the seated hole is never named: {}", fouls[0].message);
    assert_eq!(fouls[0].subject, parse::FindingSubject::Op { index: 1, feature: "Hole2".into() });
}

/// The invariant slice 0 promised: no finding's SUBJECT changes, only its
/// text — and only where the text was empty. Across the whole corpus every
/// body-foul finding on a non-drill op is still about `op.feature_name`, and
/// every one on a drill names a feature the program declares.
#[test]
fn test_no_foul_subject_moved_across_the_corpus_and_no_drill_names_nothing() {
    let mut seen = 0;
    for entry in fs::read_dir(".").expect("read the fixture corpus") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = fs::read_to_string(path.join("main.part")) else { continue };
        let geom = fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(part) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        for f in stock::body_foul_warnings(&part) {
            let parse::FindingSubject::Op { index, feature } = &f.subject else {
                panic!("{}: a body-foul finding is about an op: {:?}", path.display(), f.subject);
            };
            let op = &part.operations[*index];
            if op.at_points() {
                assert!(part.features.contains_key(feature), "{}: a drill foul names a declared hole, never '': {feature:?}", path.display());
            } else {
                assert_eq!(feature, &op.feature_name, "{}: a non-drill foul is still about its op's feature", path.display());
            }
            seen += 1;
        }
    }
    assert!(seen > 0, "the corpus must have at least one foul to sweep");
}

/// Phase 9 lane B, slice 0 (`DECISIONS.md` → "containment declines by name,
/// slice 0…"): ACCEPTANCE. The uncontained form-tool program emits
/// byte-identical, and the two analytic consumers of the stock model — the
/// body-foul probe and the entry restriction — each DECLINE by name on its
/// one op instead of handing back a clean bill for a dome the stock does not
/// contain. Its raised-stock twin, one `stock` line changed, is still rejected
/// where it always was.
///
/// The subject was the shipped fixture until Phase 10 lane B contained and
/// paired it; it is now that fixture's text with the Clear stripped and the
/// stock lowered — the program as it shipped before — and "byte-identical"
/// reads against the golden structurally: the finish's lines are the golden's
/// own, and what the golden has beyond them is exactly the Clear's.
#[test]
fn test_the_uncontained_form_tool_declines_by_name_and_still_emits() {
    let path = std::path::Path::new("sphere_formtool.cam");
    let shipped = fs::read_to_string(path.join("main.part")).expect("read");
    let src = lower_the_form_tool_blank(&strip_the_form_tool_clear(&shipped));
    let part = parse::parse_project_with_base(&src, None, Some(path)).expect("parse");
    let (g, emitted_moves) = gcode::program_with_moves(&part).expect("the uncontained program still emits");
    let golden = fs::read_to_string("tests/sphere_formtool.golden.gcode").expect("golden");
    {
        // Every line of the uncontained program appears in the golden, in
        // order, and the golden's surplus is the Clear's op alone: one
        // contiguous block from its tool change to the finish's own (the
        // twin still declares T2, so T2's register line is its own). A
        // decline is stderr and JSON only.
        let mut golden_lines = golden.trim_end().lines();
        let mut surplus: Vec<&str> = Vec::new();
        for line in g.trim_end().lines() {
            loop {
                let next = golden_lines.next().expect("the golden ran out before the finish did");
                if next == line {
                    break;
                }
                surplus.push(next);
            }
        }
        assert!(golden_lines.next().is_none(), "the golden ends where the finish ends");
        assert_eq!(surplus[0], "M06 T2", "the surplus is the Clear's block, from its tool change: {surplus:?}");
        assert!(surplus.iter().skip(1).all(|l| !l.starts_with("M06")), "to the finish's: {surplus:?}");
        assert!(surplus.iter().any(|l| l.starts_with("(--- revolve clear: TopDome with T2")), "{surplus:?}");
    }

    // The probe's voice, from `parse::lint` like every advisory.
    let lint = parse::lint(&part);
    let probe: Vec<_> = lint.iter().filter(|f| f.family == parse::FindingFamily::Decline).collect();
    assert_eq!(probe.len(), 1, "one op, one probe decline: {probe:?}");
    assert_eq!(probe[0].subject, parse::FindingSubject::Op { index: 0, feature: "TopDome".into() });
    assert!(probe[0].message.starts_with("body-foul probe declined on 'TopDome'"), "{}", probe[0].message);
    assert!(probe[0].message.contains("10.000mm above the stock top z 0.000"), "the reason quotes the lint's own face phrase: {}", probe[0].message);
    assert!(
        !lint.iter().any(|f| f.family == parse::FindingFamily::BodyFoul),
        "nothing is ACCUSED — a decline replaces a silence, never a finding: {lint:?}"
    );

    // The error's voice, post-generation off the same emitted moves.
    let moves = stock::move_declines(&part, &emitted_moves);
    assert_eq!(moves.len(), 1, "one op, one check_moves decline: {moves:?}");
    assert_eq!(moves[0].subject, parse::FindingSubject::Op { index: 0, feature: "TopDome".into() });
    assert!(moves[0].message.starts_with("check_moves declined on 'TopDome' (Form with T1)"), "{}", moves[0].message);
    assert!(moves[0].message.contains("is not a claim"), "{}", moves[0].message);

    // THE CONTROL: contain the stock — one line — and the same consumers stop
    // declining and start REFUSING. A decline is what a consumer says when it
    // cannot look; this is what it says when it can. (This twin is the
    // shipped fixture minus its Clear: the error is for the SURROUND the
    // Clear opens, which is why the pair could be contained — DECISIONS.md →
    // "Phase 10 lane B — the form-tool tube…".) #202 measured the error at
    // (45.044, 20.000, 5.044), which was the flat collar's own box-top at
    // z 5 — the over-claim's edge, not the stock's. With the tube (lane B,
    // slice 1) the `Z2.071` ring is inside its own sweep and the error is
    // the true one: the `Z-0.286` ring's SHANK, 10.125mm above the tip and
    // above its 10mm cutting top, in the top 0.29mm of the unroughed stock
    // beside the rim.
    let raised = strip_the_form_tool_clear(&shipped);
    let part = parse::parse_project_with_base(&raised, None, Some(path)).expect("parse");
    let lint = parse::lint(&part);
    assert!(!lint.iter().any(|f| f.family == parse::FindingFamily::Decline), "contained and complete: nothing to decline on: {lint:?}");
    assert!(lint.iter().any(|f| f.family == parse::FindingFamily::BodyFoul), "and the probe now finds the foul it was blind to: {lint:?}");
    let e = gcode::program(&part).expect_err("the contained form tool is rejected");
    assert!(e.contains("the move `G01 Z-0.286 F200.000`"), "at the ring whose shank meets the stock's top: {e}");
    assert!(e.contains("body 10.125mm above the tip"), "the shank, above the 10mm cutting top: {e}");
    assert!(e.contains("standing stock at (48.819, 20.000, 9.839)"), "by the entry restriction, in the stock's top beside the rim: {e}");
}

/// Phase 10 lane B, slice 1 (`DECISIONS.md` → "Phase 10 lane B, slice 1…"):
/// the point probe that names the defect (fork 8A). `(10.5, 20, -4.5)` from
/// the dome's axis — the flat collar's inner-lower corner, 0.5mm inside the
/// rim and 4.5mm below the base plane — is air a 45° cone from a point never
/// enters: the rim ring's tip sits at (rho 15, z -5) and its flank at rho
/// 10.5 is at z -0.5. The collar said removed; the tube says STANDING, in
/// the direction `check_moves` must never be handed the other way.
///
/// And the band twin's named remainder — fork 6, the pointed tip's ridges:
/// `(33.625, 20, 9.375)`, rho 3.6 near the pole, between the pole ring and
/// the 45° ring (a 7mm step in rho for a 3mm step in z), reads REMOVED here
/// because the cap claim says the finish leaves the nominal surface — honest
/// for a ball within its scallop budget, and a profile finish has no such
/// budget (`validate` rejects `scallop` for one). The undeclared twin's
/// CUTTING wall above the corner mows that ridge off; a band ending at the
/// corner cannot, and the field says so (`vox_tests` → "…agrees with the
/// field beside the rim"). The cusp budget for a piecewise-linear profile is
/// T3's, named out of this lane by the DRAFT; this is where it will register.
#[test]
fn test_the_form_tool_tube_says_the_collar_claimed_air() {
    let path = std::path::Path::new("sphere_formtool.cam");
    let src = fs::read_to_string(path.join("main.part")).expect("read");
    let part = parse::parse_project_with_base(&src, None, Some(path)).expect("parse");
    let after = stock::remaining_after(&part, part.operations.len() - 1);
    assert!(after.complete, "both ops have a closed form");
    assert!(after.material_at(40.5, 20.0, -4.5), "the collar's inner-lower corner stands: a cone from a point never entered it");
    assert!(!after.material_at(45.0, 20.0, -3.0), "under the rim ring's own tip the flank swept: rho 15, z -3");
    assert!(!after.material_at(48.0, 20.0, -1.5), "the flank outward of the tip, rho 18: -5 + 3 = -2 is the floor, and above it is swept");
    assert!(after.material_at(48.0, 20.0, -2.5), "and 0.5mm under that floor stands");

    let path = std::path::Path::new("sphere_formtool_band.cam");
    let src = fs::read_to_string(path.join("main.part")).expect("read");
    let part = parse::parse_project_with_base(&src, None, Some(path)).expect("parse");
    let after = stock::remaining_after(&part, part.operations.len() - 1);
    assert!(after.material_at(40.5, 20.0, -4.5), "the band twin's tube says the same beside the rim");
    assert!(
        !after.material_at(33.625, 20.0, 9.375),
        "NAMED REMAINDER (fork 6): the cap claims the nominal surface; the pointed tip's ridge stands there"
    );
}

/// The second silence the lane found, larger than its entry: `check_moves`
/// on an INCOMPLETE model returned `Ok(())` wordlessly since 2026-07-29. The
/// freeform pair contained its part and read `would_emit: true, findings: []`
/// — three checks never ran and the document said clean. Now the one that
/// would have run says it did not, and why.
#[test]
fn test_an_incomplete_model_declines_instead_of_passing_wordlessly() {
    let path = std::path::Path::new("mesh.cam");
    let src = fs::read_to_string(path.join("main.part")).expect("read");
    let raised = src.replace(
        "stock Box(w=60, l=60, h=15, x=0, y=0, z=0)",
        "stock Box(w=60, l=60, h=25, x=0, y=0, z=10)",
    )
    // The raised top takes the rapid plane with it (Phase 50 slice 1's law).
    .replace("rapid_z 5", "rapid_z 15");
    assert_ne!(raised, src, "the substitution must actually substitute");
    let part = parse::parse_project_with_base(&raised, None, Some(path)).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emits");
    assert!(parse::lint(&part).is_empty(), "contained, and the probe has no reading of a mesh to decline on");
    let moves = stock::move_declines(&part, &emitted_moves);
    assert_eq!(moves.len(), 1, "{moves:?}");
    assert_eq!(moves[0].family, parse::FindingFamily::Decline);
    assert_eq!(moves[0].subject, parse::FindingSubject::Op { index: 0, feature: "Blob".into() });
    assert!(moves[0].message.starts_with("check_moves declined on 'Blob' (Copy with T9): op [0] on 'Blob' has no closed-form removal"), "{}", moves[0].message);
    assert!(moves[0].message.contains("did not run for this op"), "the incomplete reason reads as 'did not run', not 'not a claim': {}", moves[0].message);
}

/// The decline wire: `--check=json` carries the family by name, so the MCP
/// `check` verb (the same document) stops telling an agent a program is
/// clean when a check never looked.
#[test]
fn test_check_json_carries_the_decline_family() {
    // The subject was the uncontained form tool until Phase 10 lane B
    // contained it; the rim wire declines by name (lane A, slice 2) and
    // carries the family the same way — two lines, per consumer per op, as
    // the roster pin counts them.
    let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
        .args(["--check=json", "contour_pocket.cam"])
        .output()
        .expect("run nacs");
    assert!(out.status.success(), "the declining program still would_emit");
    let doc = String::from_utf8_lossy(&out.stdout);
    assert!(doc.contains("\"would_emit\": true"), "{doc}");
    assert_eq!(doc.matches("\"family\": \"decline\"").count(), 2, "the rim wire's two declines: {doc}");
    assert!(out.stderr.is_empty(), "the document is the whole report");
}

/// The noise budget, pinned by name (fork 6A): exactly these fixtures carry a
/// decline, with exactly this many lines each — the scoreboard Phase 10
/// counts down (19 → 17 with lane C's pockets, → 13 with lane A's cylinder
/// bosses, → 7 with its elliptic and profile bosses, → 6 with the wire boss —
/// every boss now gains the `Clear` its finish always assumed; → 4 with lane
/// B's form-tool pair, contained and paired on its first slice; the rim wire
/// stays, by name, for T5's vocabulary). As first
/// written: 17 whose declared model is
/// incomplete (every boss finish, both contour pairs, the chamfer and
/// edge-round pairs, the two freeform, the two stepover-mode rough twins) and
/// the two form-tool programs whose stock does not contain their dome. The
/// count is per consumer per op, which is why a program with one unmodeled op
/// followed by three more carries four. If this list grows, a fixture shipped
/// with a model its own checks cannot read; if it shrinks, a removal gained a
/// closed form and the entry in T2 should say so.
#[test]
fn test_the_declining_fixtures_are_exactly_the_named_ones() {
    let expected: Vec<(&str, usize)> = vec![
        ("contour_pocket.cam", 2),
        ("contour_pocket_rough.cam", 2),
        ("mesh.cam", 1),
        ("nurbs_patch.cam", 1),
        // profile_pocket_clear_rough.cam and profile_pocket_rough.cam left
        // this list 2026-08-22 (Phase 10 lane C): a pocket with an allowance
        // has a closed-form removal now. sphere_formtool.cam and its band
        // twin left it the same day (Phase 10 lane B, slice 0): contained,
        // with a rough paired in front, nothing declines.
    ];

    let mut found: Vec<(String, usize)> = Vec::new();
    for entry in fs::read_dir(".").expect("read the fixture corpus") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = fs::read_to_string(path.join("main.part")) else { continue };
        let geom = fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(part) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        let Ok((_, emitted_moves)) = gcode::program_with_moves(&part) else { continue };
        let n = parse::lint(&part).iter().filter(|f| f.family == parse::FindingFamily::Decline).count()
            + stock::move_declines(&part, &emitted_moves).len();
        if n > 0 {
            found.push((path.file_name().unwrap().to_string_lossy().into_owned(), n));
        }
    }
    found.sort();
    let expected: Vec<(String, usize)> = expected.into_iter().map(|(n, c)| (n.to_string(), c)).collect();
    assert_eq!(found, expected, "the decline roster moved");
}

/// Phase 10 lane B, slice 0 (`DECISIONS.md` → "Phase 10 lane B, slice 0…"):
/// ACCEPTANCE. Both form-tool fixtures declare a stock that contains the
/// dome and pair a `Clear` in front, and emit SILENT — nothing from any
/// family under `--check`, and under `--gates gate` the program itself,
/// byte-for-byte the golden. No tube code was needed for this: the #202
/// error was the surround's, not the rim's.
#[test]
fn test_the_form_tool_pair_is_contained_paired_and_silent() {
    for name in ["sphere_formtool.cam", "sphere_formtool_band.cam"] {
        let path = std::path::Path::new(name);
        let src = fs::read_to_string(path.join("main.part")).expect("read");
        assert!(src.contains(FORM_TOOL_CLEAR_LINE), "{name} pairs the Clear its finish assumes");
        let part = parse::parse_project_with_base(&src, None, Some(path)).expect("parse");
        assert!(containment_warnings(&part).is_empty(), "{name}: the stock contains the dome");
        let lint = parse::lint(&part);
        assert!(lint.is_empty(), "{name}: nothing from any family: {lint:?}");
        let (g, emitted_moves) = gcode::program_with_moves(&part).expect("emits");
        assert!(stock::move_declines(&part, &emitted_moves).is_empty(), "{name}: check_moves looked, and claims");
        assert!(stock::entry_findings(&part, &emitted_moves).is_empty(), "{name}: the finish enters what its Clear opened");

        let out = Command::new(env!("CARGO_BIN_EXE_nacs"))
            .args(["--gates", "gate", name])
            .output()
            .expect("run nacs");
        assert!(out.status.success(), "{name}: every gate passes");
        assert!(out.stderr.is_empty(), "{name}: silent: {}", String::from_utf8_lossy(&out.stderr));
        let golden = fs::read_to_string(format!("tests/{}.golden.gcode", name.trim_end_matches(".cam"))).expect("golden");
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim_end(), golden.trim_end(), "{name}: the gated program is the golden");
        assert_eq!(g.trim_end(), golden.trim_end());
    }
}

/// The safety property the DRAFT proved, checked on the corpus rather than
/// argued: today's op-level reach was the MAXIMUM over the op's points, so
/// every per-point reach is at most that — the split can only ever fire on a
/// subset, and no program that emitted before can be rejected after.
///
/// Read here as the invariant that survives: every reach finding's number is
/// no deeper than the deepest tip its op emits, and every one names a feature
/// the program actually declares. The second half is the whole defect — `on ''`
/// named nothing at all.
#[test]
fn test_every_reach_finding_names_a_declared_feature_within_its_op_depth() {
    let src = fs::read_to_string("cylinder_bore_form_points.cam/main.part").expect("read");
    let short = src.replace("Drill(dia=8.0)", "Drill(dia=8.0, stickout=12.0)");
    let part = parse::parse(&short).expect("parse");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");
    let found = stock::reach_findings(&part, &emitted_moves);
    assert!(!found.is_empty(), "the sweep must have something to sweep");

    for f in &found {
        let parse::FindingSubject::Op { index, feature } = &f.subject else {
            panic!("a reach finding is about an op: {:?}", f.subject);
        };
        assert!(
            part.features.contains_key(feature),
            "every finding names a DECLARED feature, never '': {feature:?}"
        );
        // The op's own deepest emitted Z, the number the check used to answer
        // with for every point at once.
        let em = emitted_moves.iter().find(|e| e.op_idx == *index).expect("the op emitted");
        let mut m = gcode::Motion { rapid: false, x: None, y: None, z: None, f: None };
        let mut deepest = f64::INFINITY;
        for line in gcode::resolved_moves(em) {
            if gcode::apply_move(line, &mut m) {
                if let Some(z) = m.z {
                    deepest = deepest.min(z);
                }
            }
        }
        let op_reach = part.stock.z - deepest;
        let quoted: f64 = f
            .message
            .split("short of the ")
            .nth(1)
            .and_then(|s| s.split("mm").next())
            .and_then(|s| s.parse().ok())
            .expect("the finding quotes its reach");
        assert!(
            quoted <= op_reach + 1e-9,
            "a point's reach can never exceed its op's deepest tip: {quoted} vs {op_reach}"
        );
    }
}

/// `pocket_post.cam` was Phase 9 lane C's defect fixture: "its G-code is byte
/// for byte the program without the post… so a future emitter that learns to
/// go AROUND a declared post moves this test on purpose, not by accident."
/// Phase 26 slice 2 is that emitter (`DECISIONS.md` → "Phase 26 slice 2: the
/// pocket…"), and this is the test, moved on purpose: the golden is the
/// program that goes around, it is NOT the program without the post, and the
/// tool centre never comes within a radius of the post's wall.
#[test]
fn test_pocket_post_golden_goes_around_the_post() {
    let main_src = fs::read_to_string("pocket_post.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse pocket_post.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");

    let golden_gcode = fs::read_to_string("tests/pocket_post.golden.gcode").expect("failed to read pocket_post golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    let without_the_post = fs::read_to_string("tests/profile_pocket_clear.golden.gcode").expect("failed to read profile_pocket_clear golden G-code");
    assert_ne!(golden_gcode, without_the_post, "the post changes the program now — that is the build");
    assert!(generated_gcode.contains("(--- pocket clear: PocketProfile with T5 ---)\n(islands: Post)\n"), "the op says what it goes around");

    // Fork 3A: four disconnected corners at the deepest level, four ramps from
    // the stock top; the four loops outside them plunge beside cut air.
    assert_eq!(generated_gcode.matches("G01 Z0.000 F300.000").count(), 4, "one ramp per nest");
    assert_eq!(generated_gcode.matches("G01 Z-5.000 F300.000").count(), 4, "the rest plunge");

    // The post is 6mm in radius at (50, 50) and the tool 5: no cutting move
    // below the stock top brings the centre inside 11mm of the axis.
    // Read off the program's own words, modally.
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut cutting = 0;
    for line in generated_gcode.lines().filter(|l| l.starts_with('G')) {
        for word in line.split_whitespace() {
            let value = || word[1..].parse::<f64>().expect("a number");
            match word.as_bytes()[0] {
                b'X' => x = value(),
                b'Y' => y = value(),
                b'Z' => z = value(),
                _ => {}
            }
        }
        if z < -1e-9 {
            let rho = ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt();
            assert!(rho >= 11.0 - 1e-3, "`{line}` ends {rho:.3}mm from the post's axis");
            cutting += 1;
        }
    }
    assert!(cutting > 50, "the scan must actually scan ({cutting})");
}

/// The second fixture: two round posts and a rectangular pad in one pocket,
/// every wall finished after. Golden-pinned; and each island's Finish enters
/// under `check_moves` — which `gcode::program` runs — only because the
/// pocket's removal is its footprint minus its islands.
#[test]
fn test_pocket_islands_golden() {
    let main_src = fs::read_to_string("pocket_islands.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse pocket_islands.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/pocket_islands.golden.gcode").expect("failed to read pocket_islands golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert!(generated_gcode.contains("(islands: Pad, PostA, PostB)"), "by name, sorted");

    let removals = stock::remaining_after(&part, 0).removals;
    let stock::Section::Polygon { holes, .. } = &removals[0].section else { panic!("the pocket's removal is a polygon: {:?}", removals[0].section) };
    assert_eq!(holes.len(), 3, "the footprint minus its islands");
}

/// The intruder with an axis (Phase 41 slice 5): a pocket whose right wall
/// is the arc `nacs --arc` built tangent to a tilted bore's plan envelope,
/// pushed out by the web the machinist wants. The bore itself is never cut
/// — nothing in the program names it — and the golden pins the pocket's
/// family running the chorded arc as it runs any authored curve.
#[test]
fn test_tilted_bore_golden() {
    let main_src = fs::read_to_string("tilted_bore.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse tilted_bore.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/tilted_bore.golden.gcode").expect("failed to read tilted_bore golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert!(matches!(part.features["Drain"], parse::Feature::TiltedBore { .. }));
    assert!(!generated_gcode.contains("Drain"), "nothing cuts the intruder, and nothing bridges it: it lies beside the pocket");
}

/// The real part pinned to spine (Phase 44 slice 3; a fixture's name
/// carries its pin). The rows read the bore's lens as air — but on THIS
/// part the lens sits mid-height, between rows the walk joins to its first
/// chain without an entry of their own, and the bottom row lies below it:
/// so the entry is the first stroke's legs under the ramp's law, and the
/// program does not say "entered from given air", honestly. The pin in
/// `tests/given_entry_tests.rs` holds the entry from a lens that reaches
/// the first row; a walk that begins at the lens stays the pool's.
#[test]
fn test_tube_pocket_spine_golden() {
    let main_src = fs::read_to_string("tube_pocket_spine.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse tube_pocket_spine.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/tube_pocket_spine.golden.gcode").expect("failed to read tube_pocket_spine golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert!(generated_gcode.contains("(pattern: spine at 0.000deg, 6 rows"), "{generated_gcode}");
    assert!(generated_gcode.contains("(grown into given air: Bore)\n"), "{generated_gcode}");
    assert!(generated_gcode.contains("(ramp: 7 legs along the first stroke, 26.748mm long, to hold 3.000deg)\n"), "{generated_gcode}");
    assert!(!generated_gcode.contains("rows entered from given air"), "the bottom row is below the lens: {generated_gcode}");
}

/// The real part behind the tilted bore (Phase 42 slice 3): the bore is
/// GIVEN, so the pocket's floor grows into the lens a long-reach tool's body
/// can stand in, and the passes run out into the bore's open air — past the
/// authored right wall at x = 1.9" — with the certificate's blessing. The
/// golden holds the grown family; slice 4 moves its entry lines.
#[test]
fn test_tube_pocket_golden() {
    let main_src = fs::read_to_string("tube_pocket.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse tube_pocket.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/tube_pocket.golden.gcode").expect("failed to read tube_pocket golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());
    assert_eq!(part.given, ["Bore"]);
    assert!(generated_gcode.contains("(grown into given air: Bore)\n"));
    // Slice 4: the family's first loop is entered from the bore's air — a
    // plunge at depth where the whole body stands in the lens, then a
    // straight lead — never a ramp from the stock top.
    assert!(generated_gcode.contains("(entered from given air: Bore)\n"));
    assert!(!generated_gcode.contains("G01 Z0.000 F254.000"), "no ramp starts at the stock top");
    // The authored wall is at x = 1.9" = 48.26mm; the family reaches past it
    // into the bore's air, and never past the lens.
    let max_x = generated_gcode
        .lines()
        .filter(|l| l.starts_with("G01") || l.starts_with("G00"))
        .filter_map(|l| l.split_whitespace().find(|w| w.starts_with('X')).and_then(|w| w[1..].parse::<f64>().ok()))
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(max_x > 48.26 + 3.0 && max_x < 56.0, "{max_x}");
}

/// `strategy=regions` clamps to the island's wall as it does to the pocket's
/// (the second clamp, the mask machinery untouched): a small-tool rest pass
/// after the islanded Clear takes the four corner radii and goes nowhere near
/// the post — no emitted XY below the stock top inside the post's own radius
/// plus the small tool's.
#[test]
fn test_a_regions_pass_stays_off_the_island() {
    let src = fs::read_to_string("pocket_post.cam/main.part").unwrap();
    let rest = src
        .replace("tool T5 Endmill(dia=10.0)", "tool T5 Endmill(dia=10.0)\ntool T3 Endmill(dia=4.0)")
        .replace(
            "run Clear(tool=T5, profile=PocketProfile, stepover=4.0)\n",
            "run Clear(tool=T5, profile=PocketProfile, stepover=4.0)\nrun Clear(tool=T3, profile=PocketProfile, stepover=1.5, strategy=regions, voxel_cell=0.5)\n",
        );
    assert_ne!(rest, src);
    let part = parse::parse(&rest).expect("parses");
    let g = gcode::program(&part).expect("generates");
    let regions = g.split("(--- pocket regions:").nth(1).expect("the regions op is titled");
    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut cutting = 0;
    for line in regions.lines().filter(|l| l.starts_with('G')) {
        for word in line.split_whitespace() {
            let value = || word[1..].parse::<f64>().expect("a number");
            match word.as_bytes()[0] {
                b'X' => x = value(),
                b'Y' => y = value(),
                b'Z' => z = value(),
                _ => {}
            }
        }
        if z < -1e-9 {
            let rho = ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt();
            assert!(rho >= 8.0 - 1e-3, "`{line}` ends {rho:.3}mm from the post's axis");
            cutting += 1;
        }
    }
    assert!(cutting > 0, "the corners are standing work");
}

/// Phase 26 slice 3: the face that goes around. The Skim is the raster it
/// always was; the Deck is the island family with the blank as its outer wall,
/// every loop entered beside air (no ramp at all on this part), and no cutting
/// move below the skim inside a boss's radius plus the tool's.
#[test]
fn test_face_boss_golden() {
    let main_src = fs::read_to_string("face_boss_offset.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse face_boss_offset.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/face_boss_offset.golden.gcode").expect("failed to read face_boss golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    let skim = generated_gcode.split("(--- floor: Skim with T2 ---)").nth(1).unwrap().split("(---").next().unwrap();
    assert!(!skim.contains("(islands:"), "the skim meets the tops: a raster");
    let deck = generated_gcode.split("(--- floor: Deck with T2 ---)").nth(1).unwrap().split("(---").next().unwrap();
    assert!(deck.starts_with("\n(islands: Lobe, Post)\n"), "{}", &deck[..80]);
    assert!(!deck.contains("G01 Z0.000"), "air all around: every loop plunges beside air already cut");

    let (mut x, mut y, mut z) = (f64::NAN, f64::NAN, f64::NAN);
    let mut cutting = 0;
    for line in deck.lines().filter(|l| l.starts_with('G')) {
        for word in line.split_whitespace() {
            let value = || word[1..].parse::<f64>().expect("a number");
            match word.as_bytes()[0] {
                b'X' => x = value(),
                b'Y' => y = value(),
                b'Z' => z = value(),
                _ => {}
            }
        }
        if z < -1.0 - 1e-9 {
            let rho = ((x - 28.0).powi(2) + (y - 40.0).powi(2)).sqrt();
            // A point ON a chord of the 16mm ring sits inside it by up to the
            // chord's sagitta — the one geometric tolerance, the same one every
            // chorded ring in the engine is cut to — and a loop re-started
            // mid-chord begins at such a point.
            assert!(rho >= 16.0 - nacs::CHORD_TOL_MM - 1e-3, "`{line}` ends {rho:.3}mm from the post's axis");
            cutting += 1;
        }
    }
    assert!(cutting > 100, "the scan must actually scan ({cutting})");
}

/// Fork 4A: a tool that cannot pass between an island and a wall is a hard
/// error from a loop COUNT, naming the pair and the numbers. And the plain
/// pocket's two size refusals survive in the islanded family's own words.
#[test]
fn test_a_web_between_an_island_and_a_wall_is_refused() {
    let src = fs::read_to_string("pocket_post.cam/main.part").unwrap();
    // 8mm between the post and the x=20 wall; T5 is 10 across.
    let web = src.replace("x=50, y=50, z=-5", "x=34, y=50, z=-5");
    assert_ne!(web, src);
    let part = parse::parse(&web).expect("the declaration is legal; the op is not");
    // Since Phase 32 slice 2 the web is one STRETCH the op cannot reach, said
    // the way every such stretch is: where, how wide, between what, the tool
    // that would fit it, and the word that would concede it.
    let e = gcode::program(&part).unwrap_err();
    assert!(
        e.starts_with("Clear on 'PocketProfile' with T5 (10.000mm) cannot reach 1 stretch of its floor")
            && e.contains("between 'Post' and the wall of 'PocketProfile'")
            && e.contains("8.000mm wide — a tool up to 8.000mm would reach it")
            && e.contains("add cover="),
        "{e}"
    );
    // The allowance narrows the gap from both sides: 9.5mm of floor, and the
    // tool needs 10 plus 0.5 each side.
    let skinned = src.replace("x=50, y=50, z=-5", "x=35.5, y=50, z=-5").replace("stepover=4.0)", "stepover=4.0, stock_to_leave=0.5)");
    let e = gcode::program(&parse::parse(&skinned).unwrap()).unwrap_err();
    assert!(e.contains("cannot reach 1 stretch") && e.contains("9.500mm wide"), "{e}");
    // 10.5mm passes: the loop between them exists.
    let passes = src.replace("x=50, y=50, z=-5", "x=36.5, y=50, z=-5");
    gcode::program(&parse::parse(&passes).unwrap()).expect("a tool's width and a little more");

    // The plain pocket's "leaves an uncut island" has no twin here: a family
    // around islands is held to a stepover of at most the tool's radius (the
    // ridge law, `tests/island_tests.rs`), and under it the last level always
    // reaches what is left.
    let sparse = src.replace("stepover=4.0", "stepover=5.5");
    let e = gcode::program(&parse::parse(&sparse).unwrap()).unwrap_err();
    assert!(e.contains("goes around islands") && e.contains("more than T5's radius"), "{e}");
}

// ---- Phase 10 lane C: the polyline pocket with an allowance ----------------
// DECISIONS.md → "Phase 10 lane C lands…". The 2026-07-29 offset-decomposition
// DRAFT's Finding 2, built: the void a pocket with a `stock_to_leave` leaves
// is the polygon eroded by the skin, and the model is a LIST, so the
// offsetter's split/vanish outcomes have somewhere to go after all.

/// Distance from a point to a closed polygon's boundary — the test's own
/// arithmetic, not the model's, so the pin below is an independent reading.
fn dist_to_closed_polygon(pts: &[parse::Pt], x: f64, y: f64) -> f64 {
    let n = pts.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 <= 1e-18 { 0.0 } else { (((x - a.x) * dx + (y - a.y) * dy) / len2).clamp(0.0, 1.0) };
        let d = ((x - a.x - t * dx).powi(2) + (y - a.y - t * dy).powi(2)).sqrt();
        best = best.min(d);
    }
    best
}

/// Even-odd containment, the test's own.
fn inside_closed_polygon(pts: &[parse::Pt], x: f64, y: f64) -> bool {
    let n = pts.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if (a.y > y) != (b.y > y) && x < a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// ACCEPTANCE (the roadmap's): both fixtures' models are complete and their
/// rest reports make a claim where they said `no claim`.
#[test]
fn test_the_pocket_with_an_allowance_joins_the_model() {
    for (fixture, forgotten) in [
        // The Clear: nothing — the corner fillets a round tool leaves are
        // declared standing (2026-11-10), as its skinless twin's are, and the
        // allowance moved the wall, not the kind of leftover.
        ("profile_pocket_clear_rough.cam", "forgotten (declared removed, never swept): none"),
        // The finish-only program: the whole interior no wall pass sweeps,
        // which is what a finish without a rough in front of it leaves, and
        // what the entry advisory below is about.
        ("profile_pocket_rough.cam", "forgotten (declared removed, never swept): 29244 cells (~1743.1 mm³)"),
    ] {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read");
        let part = parse::parse(&src).expect("parse");
        assert!(stock::incomplete_ops(&part).is_empty(), "{fixture}: the model is complete now");
        let report = nacs::vox::rest_report(&part, None).expect("report");
        assert!(report.contains("declared model: complete"), "{fixture}: {report}");
        assert!(!report.contains("no claim"), "{fixture}: a claim, not a decline: {report}");
        assert!(report.contains(forgotten), "{fixture}: {report}");
    }
}

/// ONE HOME, pinned from the emitter's side: every tool centre the Clear emits
/// stands at least `r` inside the removal's polygon (the model claims nothing
/// the tool does not reach), and the wall pass stands at exactly `r` (it
/// claims nothing less). The removal is read from the MODEL, the distances
/// are the test's own arithmetic.
#[test]
fn test_the_allowance_removal_is_the_emitters_own_offset() {
    let src = fs::read_to_string("profile_pocket_clear_rough.cam/main.part").expect("read");
    let part = parse::parse(&src).expect("parse");
    let r = part.tools[&5].dia / 2.0;
    let remaining = stock::remaining_after(&part, 0);
    assert!(remaining.complete);
    assert_eq!(remaining.removals.len(), 1, "one loop, one removal");
    let stock::Section::Polygon { points, .. } = &remaining.removals[0].section else {
        panic!("a polygon: {:?}", remaining.removals[0].section);
    };
    // The eroded polygon is the 60mm square drawn in by the 0.500 skin, its
    // corners filleted by the tool's radius (2026-11-10: the model claims
    // the floor as the tool reaches it).
    assert!(points.len() > 4, "the fillets are chorded: {}", points.len());
    let (lo, hi) = (20.5, 79.5);
    for q in points {
        assert!(q.x >= lo - 1e-6 && q.x <= hi + 1e-6 && q.y >= lo - 1e-6 && q.y <= hi + 1e-6, "{q:?}");
        let on_edge = (q.x - lo).abs() < 1e-6 || (q.x - hi).abs() < 1e-6 || (q.y - lo).abs() < 1e-6 || (q.y - hi).abs() < 1e-6;
        let (cx, cy) = (if q.x < 50.0 { lo + r } else { hi - r }, if q.y < 50.0 { lo + r } else { hi - r });
        let on_fillet = ((q.x - cx).hypot(q.y - cy) - r).abs() <= nacs::CHORD_TOL_MM + 1e-6;
        assert!(on_edge || on_fillet, "{q:?} is neither on the eroded wall nor on a corner fillet");
    }

    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");
    let em = emitted_moves.iter().find(|e| e.op_idx == 0).expect("the Clear");
    let mut m = gcode::Motion { rapid: false, x: None, y: None, z: None, f: None };
    let mut closest = f64::INFINITY;
    let mut cutting = 0;
    for line in gcode::resolved_moves(em) {
        if !gcode::apply_move(line, &mut m) || m.rapid {
            continue;
        }
        let (Some(x), Some(y)) = (m.x, m.y) else { continue };
        assert!(inside_closed_polygon(points, x, y), "a tool centre outside the void: ({x}, {y})");
        // The corner fillets are chorded inside their arcs by a sagitta, so
        // the centre at a corner reads that much closer than r.
        let d = dist_to_closed_polygon(points, x, y);
        assert!(d >= r - nacs::CHORD_TOL_MM - 1e-6, "a tool centre closer than r to the skin surface: ({x}, {y}) at {d:.6}");
        closest = closest.min(d);
        cutting += 1;
    }
    assert!(cutting > 20, "the Clear must have cut something: {cutting} moves");
    assert!((closest - r).abs() <= nacs::CHORD_TOL_MM + 1e-6, "the wall pass rides exactly r inside the skin surface: {closest:.6}");
}

/// The skin is ONE number. The entry advisory — which the incomplete model had
/// silenced on profile_pocket_rough.cam — now fires exactly where it fires on
/// the skinless twin, moved by the skin: the model's skin and the emitter's
/// skin cannot be two numbers, or this pin reads a different offset.
#[test]
fn test_the_models_skin_and_the_emitters_skin_are_one_number() {
    let at = |fixture: &str| -> (f64, f64) {
        let src = fs::read_to_string(format!("{fixture}/main.part")).expect("read");
        let part = parse::parse(&src).expect("parse");
        let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emit");
        let findings = stock::entry_findings(&part, &emitted_moves);
        assert!(!findings.is_empty(), "{fixture}: a finish into an uncleared pocket is the category-3 entry");
        let msg = &findings[0].message;
        let key = "meets standing material at (";
        let i = msg.find(key).expect(msg) + key.len();
        let nums: Vec<f64> = msg[i..].split(')').next().unwrap().split(", ").map(|t| t.parse().unwrap()).collect();
        (nums[0], nums[1])
    };
    let (x0, y0) = at("profile_pocket.cam");
    let (x1, y1) = at("profile_pocket_rough.cam");
    assert!((x1 - x0 - 0.5).abs() < 1e-9 && (y1 - y0 - 0.5).abs() < 1e-9, "({x0}, {y0}) vs ({x1}, {y1})");
}

/// The two outcomes the old arm said it had nowhere to put. A VANISHED offset
/// declines — and the emitter rejects the same program, so no program exists
/// for the decline to be wrong about. A SPLIT offset is several removals, and
/// the Clear cuts both halves.
#[test]
fn test_a_vanished_offset_declines_where_the_emitter_refuses_and_a_split_is_several_removals() {
    let src = fs::read_to_string("profile_pocket_rough.cam/main.part").expect("read");
    // 30.0 of allowance on a 60mm square: the inward offset vanishes.
    let gone = src.replace("stock_to_leave=0.5", "stock_to_leave=30.0");
    assert_ne!(gone, src);
    let part = parse::parse(&gone).expect("a vanishing allowance is legal to declare");
    let named: Vec<usize> = stock::incomplete_ops(&part).into_iter().map(|(i, _)| i).collect();
    assert_eq!(named, vec![0, 1], "both finishes decline by name");
    let e = gcode::program(&part).expect_err("and the emitter rejects the same program");
    assert!(e.contains("offsets to 0 loop(s)") || e.contains("degenerate offset"), "{e}");

    // A dumbbell: two 20×20 squares joined by a 4mm-wide neck. A 3mm skin
    // erodes the neck away and the void is two loops.
    let dumbbell = "\
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=60, l=30, h=20, x=-5, y=-5, z=0)
tool T5 Endmill(dia=10.0)
feature Bell ProfilePocket(extent=5.0
    0 0
    20 0
    20 8
    30 8
    30 0
    50 0
    50 20
    30 20
    30 12
    20 12
    20 20
    0 20
)
run Clear(tool=T5, profile=Bell, stepover=4.0, stock_to_leave=3.0)
";
    let part = parse::parse(dumbbell).expect("parse");
    let remaining = stock::remaining_after(&part, 0);
    assert!(remaining.complete, "two loops are two removals, not a decline");
    assert_eq!(remaining.removals.len(), 2, "{:?}", remaining.removals);
    // Each half is a 14×14 square; the neck (x 20..30, y 8..12) is part.
    assert!(remaining.material_at(25.0, 10.0, -2.5), "the neck stands");
    assert!(!remaining.material_at(10.0, 10.0, -2.5), "the left half is void");
    assert!(!remaining.material_at(40.0, 10.0, -2.5), "the right half is void");
    gcode::program(&part).expect("the Clear cuts both halves");
}

// ---- Phase 15 slice 1: the travel half of the machine envelope ------------
// (DECISIONS.md "Phase 15 opens: the envelope DRAFT…", fork 1.)

/// The acceptance fixture emits its golden: a facing job on a declared
/// machine whose spans it respects, datum on the table, zero findings.
#[test]
fn test_floor_travel_golden() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("failed to parse floor_travel.cam");
    let generated_gcode = gcode::program(&part).expect("failed to generate program");
    let golden_gcode = fs::read_to_string("tests/floor_travel.golden.gcode").expect("failed to read travel golden G-code");
    assert_eq!(generated_gcode.trim(), golden_gcode.trim());

    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emits");
    assert!(stock::envelope_findings(&part, &emitted_moves).is_empty(),
        "a program inside its declared spans has nothing said about it");
}

/// The K1 acceptance form, applied to the envelope: an envelope the program
/// respects changes ZERO bytes — the twin (the same source minus its
/// `machine` and `datum` lines) is byte-identical, not merely equivalent.
#[test]
fn test_undeclared_envelope_twin_is_byte_identical() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let declared_part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("failed to parse declared fixture");
    let declared = gcode::program(&declared_part).expect("failed to generate declared program");

    let twin_src: String = main_src
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("machine ") && !t.starts_with("datum ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let twin_part = parse::parse(&twin_src).expect("failed to parse undeclared twin");
    assert!(twin_part.machine.is_none() && twin_part.datum.is_none(), "the twin declares no envelope");
    let undeclared = gcode::program(&twin_part).expect("failed to generate undeclared program");
    assert_eq!(declared, undeclared, "the envelope is a check, never a word in the program");
}

/// Shift the same setup toward the table's wall and the walk names the
/// crossing: the axis, the machine coordinate, the datum's share, and the
/// span — advisory, so the program still emits.
#[test]
fn test_travel_crossing_names_the_axis_and_the_numbers() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    // datum x=+10: the facing pass's X −5..125 lands at machine +5..+135,
    // past travel_x's max of 0 by 135.
    let shifted = main_src.replace("datum x=-500.0 y=-200.0", "datum x=10.0 y=-200.0");
    let part = parse::parse_project_with_base(&shifted, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("failed to parse shifted fixture");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("advisory, so it still emits");
    let findings = stock::envelope_findings(&part, &emitted_moves);
    assert_eq!(findings.len(), 1, "one op, one axis crossed: {findings:?}");
    let f = &findings[0];
    assert_eq!(f.family, parse::FindingFamily::Envelope);
    assert_eq!(nacs::check_result::family_word(f.family), "envelope");
    for needle in ["travel_x", "machine coordinates", "135.000", "-762.000 .. 0.000", "datum"] {
        assert!(f.message.contains(needle), "the sentence names {needle}: {}", f.message);
    }
    assert!(f.place.is_some(), "a travel finding carries the position it happened at");
    // And the same walk in Y: push the setup below the Y span's floor.
    let shifted_y = main_src.replace("datum x=-500.0 y=-200.0", "datum x=-500.0 y=-410.0");
    let part_y = parse::parse_project_with_base(&shifted_y, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("failed to parse y-shifted fixture");
    let (_, em_y) = gcode::program_with_moves(&part_y).expect("emits");
    let fy = stock::envelope_findings(&part_y, &em_y);
    assert_eq!(fy.len(), 1);
    assert!(fy[0].message.contains("travel_y"), "{}", fy[0].message);
}

/// Half a declaration is no claim: spans with no datum (or a datum with no
/// spans) and the check never speaks — the undeclared-means-today's-
/// behaviour law, on each home separately.
#[test]
fn test_travel_check_is_silent_unless_both_homes_speak() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    for strip in ["datum ", "machine "] {
        let half: String = main_src
            .lines()
            .filter(|l| !l.trim_start().starts_with(strip))
            .collect::<Vec<_>>()
            .join("\n");
        let part = parse::parse_project_with_base(&half, None, Some(std::path::Path::new("floor_travel.cam")))
            .expect("failed to parse half-declared fixture");
        let (_, emitted_moves) = gcode::program_with_moves(&part).expect("emits");
        assert!(stock::envelope_findings(&part, &emitted_moves).is_empty(),
            "with '{strip}' stripped the check has nothing to gauge against");
    }
}

/// The Z rulings, both doors: `travel_z` rejects by name in the machine
/// file, `datum z=` rejects by name in the part — neither is silently
/// unknown, because each names exactly why Z is not checkable today.
#[test]
fn test_the_z_doors_reject_by_name() {
    let err = parse::parse_machine("name m\ntravel_z -508 0\n").unwrap_err();
    assert!(err.contains("travel_z is declined by name") && err.contains("H VALUE"), "{err}");

    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let with_z = main_src.replace("datum x=-500.0 y=-200.0", "datum x=-500.0 y=-200.0 z=-400.0");
    let err = parse::parse_project_with_base(&with_z, None, Some(std::path::Path::new("floor_travel.cam"))).unwrap_err();
    assert!(err.contains("datum takes no z"), "{err}");
}

/// The travel keys' own grammar: two numbers, min strictly below max, and
/// an inch machine file's spans convert at its parse boundary like every
/// other length — declared in the controller's inches, held in mm.
#[test]
fn test_travel_span_grammar_and_units() {
    let err = parse::parse_machine("travel_x 0\n").unwrap_err();
    assert!(err.contains("two numbers"), "{err}");
    let err = parse::parse_machine("travel_x 5 5\n").unwrap_err();
    assert!(err.contains("min must sit strictly below max"), "{err}");
    let err = parse::parse_machine("travel_y 10 -10\n").unwrap_err();
    assert!(err.contains("min must sit strictly below max"), "{err}");

    let m = parse::parse_machine("units inch\ntravel_x -30 0\n").expect("an inch machine's spans parse");
    let (lo, hi) = m.travel_x.expect("declared");
    assert!((lo - (-762.0)).abs() < 1e-9 && hi.abs() < 1e-9, "inches convert at the boundary: {lo} {hi}");
}

/// The datum line's own grammar: both axes required, one line per program,
/// unknown fields named, and inch part inputs convert at the parse boundary.
#[test]
fn test_datum_grammar_and_units() {
    let base = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\nstock Box(w=10, l=10, h=5, x=0, y=0, z=0)\n\ntool T1 Endmill(dia=2.0)\nfeature F Plane(z=0.0)\nrun End(tool=T1, surface=F, stepover=1.0)\n";
    let err = parse::parse(&format!("{base}datum x=-500.0\n")).unwrap_err();
    assert!(err.contains("both axes"), "{err}");
    let err = parse::parse(&format!("{base}datum x=-1.0 y=-2.0\ndatum x=-3.0 y=-4.0\n")).unwrap_err();
    assert!(err.contains("duplicate 'datum'"), "{err}");
    let err = parse::parse(&format!("{base}datum x=-1.0 y=-2.0 w=3\n")).unwrap_err();
    assert!(err.contains("unknown datum field"), "{err}");

    let inch_src = base.replace("units mm", "units inch");
    let p = parse::parse(&format!("{inch_src}datum x=-10.0 y=-4.0\n")).expect("an inch part's datum parses");
    let (dx, dy) = p.datum.expect("declared");
    assert!((dx - (-254.0)).abs() < 1e-9 && (dy - (-101.6)).abs() < 1e-9, "the datum is a length pair: {dx} {dy}");
}

// ---- Phase 15 slice 2: the spindle ceiling --------------------------------
// (DECISIONS.md "Phase 15 opens: the envelope DRAFT…", fork 2.)

/// The ceiling names an over-asked spindle — the op, the resolved RPM, the
/// declared max, the overshoot — and it advises: the program still emits,
/// with the S word UNCLAMPED, exactly as authored.
#[test]
fn test_spindle_ceiling_names_the_overshoot_and_never_clamps() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let fast = main_src.replace("spindle 3000", "spindle 24000");
    let part = parse::parse_project_with_base(&fast, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("advisory, so it parses");
    let findings = parse::envelope_ceiling_findings(&part);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let f = &findings[0];
    assert_eq!(f.family, parse::FindingFamily::Envelope);
    for needle in ["24000 RPM", "spindle_max 12000", "by 12000", "never clamps"] {
        assert!(f.message.contains(needle), "the sentence names {needle}: {}", f.message);
    }
    assert!(f.place.is_none(), "an RPM happens everywhere the op cuts — no one place");
    // And the lint voice carries it: one home, `parse::lint`.
    assert!(parse::lint(&part).iter().any(|w| w.family == parse::FindingFamily::Envelope));
    // Unclamped: the emitted S word is the authored number, not the ceiling.
    let g = gcode::program(&part).expect("advisory, so it emits");
    assert!(g.contains("M03 S24000.000"), "the S word is authored, never clamped");
}

/// The clean fixture sits under its ceiling and nothing is said; the twin
/// discipline extends — a declared ceiling the program respects changes
/// zero bytes (the slice-1 twin test already strips the whole machine).
#[test]
fn test_spindle_ceiling_is_silent_under_the_max() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("failed to parse fixture");
    assert!(part.machine.as_ref().and_then(|m| m.spindle_max).is_some(), "the fixture declares its ceiling");
    assert!(parse::envelope_ceiling_findings(&part).is_empty());
}

/// The key's grammar: a positive RPM or nothing.
#[test]
fn test_spindle_max_grammar() {
    let err = parse::parse_machine("spindle_max fast\n").unwrap_err();
    assert!(err.contains("not a number"), "{err}");
    let err = parse::parse_machine("spindle_max 0\n").unwrap_err();
    assert!(err.contains("must be positive"), "{err}");
    let err = parse::parse_machine("spindle_max -200\n").unwrap_err();
    assert!(err.contains("must be positive"), "{err}");
    let m = parse::parse_machine("spindle_max 12000\n").expect("a ceiling parses");
    assert_eq!(m.spindle_max, Some(12000.0));
}

/// A per-op spindle override is judged as its own resolved number: the op
/// that asks past the ceiling is named, the one under it is not.
#[test]
fn test_spindle_ceiling_judges_each_op_resolved() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let two_ops = main_src.replace(
        "run End(tool=T5, surface=TopFace, stepover=6.0)",
        "run End(tool=T5, surface=TopFace, stepover=6.0)\nrun End(tool=T5, surface=TopFace, stepover=6.0, spindle=18000)",
    );
    let part = parse::parse_project_with_base(&two_ops, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("parses");
    let findings = parse::envelope_ceiling_findings(&part);
    assert_eq!(findings.len(), 1, "only the overriding op crosses: {findings:?}");
    assert!(findings[0].message.contains("18000 RPM"), "{}", findings[0].message);
    assert!(findings[0].message.contains("op [1]"), "{}", findings[0].message);
}

// ---- Phase 15 slice 3: the changer ----------------------------------------
// (DECISIONS.md "Phase 15 opens: the envelope DRAFT…", fork 3.)

/// One tool past the pockets and the overflowing op is named — with the
/// tool, the running distinct count, and the declared pockets — while a
/// RE-USED tool stays free: a pocket count is a count of tools, not runs.
#[test]
fn test_changer_counts_distinct_tools_not_runs() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let crowded = main_src
        .replace(
            "tool T5 Endmill(dia=10.0)",
            "tool T5 Endmill(dia=10.0)\ntool T6 Endmill(dia=6.0)",
        )
        .replace(
            "run End(tool=T5, surface=TopFace, stepover=6.0)",
            "run End(tool=T5, surface=TopFace, stepover=6.0)\nrun End(tool=T6, surface=TopFace, stepover=4.0)\nrun End(tool=T5, surface=TopFace, stepover=6.0)",
        );
    // Shrink the fixture's carousel to one pocket for the test.
    let machine_src = fs::read_to_string("floor_travel.cam/vmc.machine").expect("read vmc.machine");
    let one_pocket = machine_src.replace("atc_pockets 24", "atc_pockets 1");
    let dir = std::env::temp_dir().join("nacs_atc_test.cam");
    std::fs::create_dir_all(&dir).expect("scratch fixture dir");
    std::fs::write(dir.join("vmc.machine"), one_pocket).expect("write machine");
    let part = parse::parse_project_with_base(&crowded, None, Some(&dir)).expect("advisory, so it parses");

    let findings = parse::envelope_changer_findings(&part);
    assert_eq!(findings.len(), 1, "T5 re-used is free; T6 overflows once: {findings:?}");
    let f = &findings[0];
    assert_eq!(f.family, parse::FindingFamily::Envelope);
    for needle in ["T6", "distinct-tool count to 2", "op [1]", "holds 1 pocket", "count of TOOLS", "--assembly-report"] {
        assert!(f.message.contains(needle), "the sentence names {needle}: {}", f.message);
    }
    assert!(f.place.is_none(), "a pocket count has no place in the part");
    // The lint voice carries it, and the program still emits (advisory).
    assert!(parse::lint(&part).iter().any(|w| w.family == parse::FindingFamily::Envelope));
    gcode::program(&part).expect("advisory, so it emits");
}

/// The shipped fixture sits inside its 24 pockets with one tool: silence.
#[test]
fn test_changer_is_silent_within_the_pockets() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("failed to read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("failed to parse fixture");
    assert_eq!(part.machine.as_ref().and_then(|m| m.atc_pockets), Some(24), "the fixture declares its carousel");
    assert!(parse::envelope_changer_findings(&part).is_empty());
}

/// The key's grammar: a positive whole number of pockets or nothing.
#[test]
fn test_atc_pockets_grammar() {
    for bad in ["atc_pockets many\n", "atc_pockets 0\n", "atc_pockets -3\n", "atc_pockets 2.5\n"] {
        let err = parse::parse_machine(bad).unwrap_err();
        assert!(err.contains("not a pocket count"), "{bad}: {err}");
    }
    let m = parse::parse_machine("atc_pockets 24\n").expect("a carousel parses");
    assert_eq!(m.atc_pockets, Some(24));
}

// ---- Phase 15 slice 4: the envelope gate, sixth of the family -------------
// (DECISIONS.md "Phase 15 opens: the envelope DRAFT…", fork 4, taken at the
// DRAFT's blessing.)

/// The directive's grammar: two voices, advise the default, anything else
/// named and rejected.
#[test]
fn test_envelope_directive_grammar() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("parses");
    assert_eq!(part.envelope_mode, parse::EnvelopeMode::Advise, "advise is the undeclared default");
    let gated = parse::parse_project_with_base(&format!("envelope gate\n{main_src}"), None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("a clean program under the gate still parses");
    assert_eq!(gated.envelope_mode, parse::EnvelopeMode::Gate);
    let err = parse::parse("envelope loudly\n").unwrap_err();
    assert!(err.contains("unknown envelope mode"), "{err}");
}

/// The travel arm gates in `gcode::program`, where emitted moves exist: the
/// shifted-datum program that advises today rejects under `envelope gate`,
/// in the gate's own voice, on exactly the advisory's sentence.
#[test]
fn test_envelope_gate_travel_arm_rejects_the_crossing() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("read main.part");
    let shifted = main_src.replace("datum x=-500.0 y=-200.0", "datum x=10.0 y=-200.0");
    let advisory = parse::parse_project_with_base(&shifted, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("advisory parses");
    assert!(gcode::program(&advisory).is_ok(), "advise emits");
    let gated = parse::parse_project_with_base(&format!("envelope gate\n{shifted}"), None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("the gate is enforced at emission, so parse succeeds");
    let e = gcode::program(&gated).expect_err("the gate rejects the crossing");
    assert!(e.starts_with("envelope gate:"), "the gate's voice: {e}");
    assert!(e.contains("travel_x"), "the same sentence the advisory prints: {e}");
}

/// The ceiling arm gates at parse — the RPM is the declaration's own number
/// — and the error carries the advisory's sentence.
#[test]
fn test_envelope_gate_ceiling_arm_rejects_at_parse() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("read main.part");
    let fast = main_src.replace("spindle 3000", "spindle 24000");
    let e = parse::parse_project_with_base(&format!("envelope gate\n{fast}"), None, Some(std::path::Path::new("floor_travel.cam")))
        .expect_err("the gate rejects the over-asked spindle");
    assert!(e.starts_with("envelope gate:"), "the gate's voice: {e}");
    assert!(e.contains("spindle_max 12000"), "{e}");
}

/// The CLI floor escalates the sixth like its five siblings: the same
/// shifted-datum program that emits at its declared modes rejects once
/// `apply_gate_floor` runs.
#[test]
fn test_gate_floor_escalates_the_envelope() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("read main.part");
    let shifted = main_src.replace("datum x=-500.0 y=-200.0", "datum x=10.0 y=-200.0");
    let mut part = parse::parse_project_with_base(&shifted, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("advisory parses");
    assert!(gcode::program(&part).is_ok(), "declared modes emit");
    parse::apply_gate_floor(&mut part);
    assert_eq!(part.envelope_mode, parse::EnvelopeMode::Gate, "the floor reaches the sixth");
    assert!(gcode::program(&part).is_err(), "--gates gate must escalate the crossing");
}

// ---- Phase 16 slice 1: the cut-time model ---------------------------------
// (DECISIONS.md "Phase 16 opens: the clock-and-address DRAFT…", forks 1–2.)

/// The fixture's clock, pinned by the fixture's own shape: every cutting
/// millimetre but the one 5mm plunge runs at feed 800, the plunge at 300,
/// and every counted rapid millimetre is a Z move at the declared rapid_z —
/// facts a reader can check against the golden by eye, so the assertions
/// derive the times from the report's own distances without restating the
/// walk.
#[test]
fn test_time_report_prices_the_floor_fixture() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("read main.part");
    let part = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("parses");
    let r = nacs::cut_time::time_report(&part).expect("a clean program has a clock");
    assert!(!r.declined(), "all three rates are declared");
    // The face stands half a millimetre under the blank's top since ROADMAP
    // item 3 slice 6 (at the top it skimmed nothing), so the plunge from
    // rapid_z is 5.5 mm.
    let expected_cutting = (r.cutting_mm() - 5.5) / 800.0 * 60.0 + 5.5 / 300.0 * 60.0;
    assert!((r.cutting_s() - expected_cutting).abs() < 1e-6,
        "one 5.5mm plunge at 300, the rest at 800: {} vs {}", r.cutting_s(), expected_cutting);
    let expected_rapid = r.rapid_mm() / 24000.0 * 60.0;
    assert!((r.rapid_s() - expected_rapid).abs() < 1e-6,
        "every counted rapid is pure Z at rapid_z: {} vs {}", r.rapid_s(), expected_rapid);
    assert!(r.rapid_mm() > 0.0 && r.cutting_mm() > 1000.0, "the facing actually moved");
    let text = nacs::cut_time::render(&r);
    assert!(text.starts_with("cut time — path length over commanded feed"), "the honesty sentence heads the report");
    assert!(text.contains("total: "), "{text}");
}

/// No rates, no guessed traverse: the rapid share and the total decline by
/// name, the rapid DISTANCE is still stated, and cutting time is unchanged
/// to the last bit.
#[test]
fn test_time_report_declines_rapids_without_rates() {
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("read main.part");
    let machine_src = fs::read_to_string("floor_travel.cam/vmc.machine").expect("read vmc.machine");
    let no_rates: String = machine_src.lines().filter(|l| !l.trim_start().starts_with("rapid_")).collect::<Vec<_>>().join("\n");
    let dir = std::env::temp_dir().join("nacs_time_test.cam");
    std::fs::create_dir_all(&dir).expect("scratch fixture dir");
    std::fs::write(dir.join("vmc.machine"), no_rates).expect("write machine");
    let bare = parse::parse_project_with_base(&main_src, None, Some(&dir)).expect("parses");
    let r = nacs::cut_time::time_report(&bare).expect("still a clock");
    assert!(r.declined(), "an axis moved with no declared rate");
    assert!(r.rapid_mm() > 0.0, "the distance is measurable from the text alone");
    assert!((r.rapid_s() - 0.0).abs() < 1e-12, "no counted rapid seconds — never a guess");
    let text = nacs::cut_time::render(&r);
    assert!(text.contains("time declined") && text.contains("total: declined"), "{text}");

    let declared = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam")))
        .expect("parses");
    let rd = nacs::cut_time::time_report(&declared).expect("clock");
    assert_eq!(r.cutting_s(), rd.cutting_s(), "cutting time is exact either way");
}

/// The arc blocks pay their true length: a full circle prices 2πr, sweeps
/// read in the commanded direction — and over a real fixture pair, the
/// `compress=arcs` program's cutting length agrees with its chorded twin
/// within the chord tolerance's slack.
#[test]
fn test_arc_blocks_pay_their_true_length() {
    use nacs::cut_time::arc_len;
    let full = arc_len(15.0, 0.0, -15.0, 0.0, 15.0, 0.0, false);
    assert!((full - 2.0 * std::f64::consts::PI * 15.0).abs() < 1e-9, "start == end is the full circle: {full}");
    let quarter = arc_len(10.0, 0.0, -10.0, 0.0, 0.0, 10.0, false);
    assert!((quarter - std::f64::consts::FRAC_PI_2 * 10.0).abs() < 1e-9, "a CCW quarter: {quarter}");
    let quarter_cw = arc_len(0.0, 10.0, 0.0, -10.0, 10.0, 0.0, true);
    assert!((quarter_cw - std::f64::consts::FRAC_PI_2 * 10.0).abs() < 1e-9, "the CW reading: {quarter_cw}");

    let chords = parse::parse(&fs::read_to_string("cylinder_bore.cam/main.part").unwrap()).unwrap();
    let arcs_src = fs::read_to_string("cylinder_bore_arcs.cam/main.part").unwrap();
    let arcs = parse::parse(&arcs_src).unwrap();
    let rc = nacs::cut_time::time_report(&chords).expect("clock");
    let ra = nacs::cut_time::time_report(&arcs).expect("clock");
    let (c, a) = (rc.cutting_mm(), ra.cutting_mm());
    assert!(a >= c, "the exact circle is never shorter than its inscribed chords: {a} vs {c}");
    assert!((a - c) / c < 0.01, "and the chord tolerance keeps them within a percent: {a} vs {c}");
}

/// The rapid keys' grammar, and the units boundary: rates are lengths per
/// minute, so an inch machine file's rates convert like its spans.
#[test]
fn test_rapid_rate_grammar_and_units() {
    for bad in ["rapid_x fast\n", "rapid_y 0\n", "rapid_z -5\n"] {
        assert!(parse::parse_machine(bad).is_err(), "{bad}");
    }
    let m = parse::parse_machine("units inch\nrapid_x 1000\n").expect("an inch machine's rate parses");
    assert!((m.rapid_x.unwrap() - 25400.0).abs() < 1e-9, "in/min converts at the boundary");
    // And a rate changes zero emitted bytes — a time input, never a limit.
    let main_src = fs::read_to_string("floor_travel.cam/main.part").expect("read main.part");
    let with_rates = parse::parse_project_with_base(&main_src, None, Some(std::path::Path::new("floor_travel.cam"))).unwrap();
    let machine_src = fs::read_to_string("floor_travel.cam/vmc.machine").expect("read vmc.machine");
    let no_rates: String = machine_src.lines().filter(|l| !l.trim_start().starts_with("rapid_")).collect::<Vec<_>>().join("\n");
    let dir = std::env::temp_dir().join("nacs_time_bytes_test.cam");
    std::fs::create_dir_all(&dir).expect("scratch fixture dir");
    std::fs::write(dir.join("vmc.machine"), no_rates).expect("write machine");
    let without = parse::parse_project_with_base(&main_src, None, Some(&dir)).unwrap();
    assert_eq!(gcode::program(&with_rates).unwrap(), gcode::program(&without).unwrap());
}

// ---- Phase 16 slice 2: the move→line correspondence -----------------------
// (DECISIONS.md "Phase 16 opens: the clock-and-address DRAFT…", fork 3.)

/// The address, pinned over the whole shipped corpus: every op's k-th move
/// IS program line `moves_start + k` — the block sits inside the op's own
/// `lines` range, and on a default-format machine the line's text (N-rung
/// stripped) is the move string itself, byte for byte. A non-default
/// machine respells in place, so there the containment half still holds
/// and the strings are the respelling of these — which is exactly why the
/// correspondence is carried as indices and not text.
#[test]
fn test_every_corpus_move_knows_its_program_line() {
    let mut moves_pinned = 0usize;
    let mut fixtures = 0usize;
    for entry in fs::read_dir(".").expect("read the fixture corpus") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let Ok(src) = fs::read_to_string(path.join("main.part")) else { continue };
        let geom = fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(part) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        let Ok((text, emissions)) = gcode::program_with_moves(&part) else { continue };
        let lines: Vec<&str> = text.lines().collect();
        let default_format = part.machine.as_ref().map_or(true, |m| m.output_format_is_default());
        fixtures += 1;
        for em in &emissions {
            if em.moves.is_empty() {
                continue;
            }
            assert!(em.lines.contains(&em.moves_start),
                "{}: op {}'s move block starts inside its own range", path.display(), em.op_idx);
            assert!(em.moves_start + em.moves.len() <= em.lines.end,
                "{}: and ends inside it", path.display());
            for (k, mv) in em.moves.iter().enumerate() {
                let at = em.move_line(k);
                if default_format {
                    assert_eq!(gcode::strip_n_word(lines[at]), mv.as_str(),
                        "{}: op {} move {} is line {}", path.display(), em.op_idx, k, at);
                }
                moves_pinned += 1;
            }
        }
    }
    assert!(fixtures >= 90, "the corpus walked: {fixtures}");
    assert!(moves_pinned > 10_000, "the corpus's moves, each with an address: {moves_pinned}");
}


// ---- Phase 22 slice 3: the reference rule -----------------------------------
//
// A `run` line carries only the reference key its verb READS — the first
// row of the per-verb semantic matrix the whitelist law (2026-08-16)
// deferred by name, filled because a live run produced a Drill that named
// its hole three times (`points=`, `surface=`, `profile=`) and the parser
// accepted all three while reading one. The corpus is clean (every shipped
// fixture parses, above); these pin the refusal and its sentence.

const REF_PREAMBLE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T1 Endmill(dia=10)\ntool T3 Drill(dia=6)\n\
feature Top Plane(z=0)\nfeature Hole CylinderBore(dia=6, extent=5, x=20, y=20)\nfeature Bore CylinderBore(dia=30, extent=5, x=60, y=40)\n";

#[test]
fn test_a_run_line_carries_only_the_reference_key_its_verb_reads() {
    // The live run's own line, the one this rule exists for.
    let e = parse::parse(&format!("{REF_PREAMBLE}run Form(points=[Hole], tool=T3, cycle=Simple, surface=Hole, profile=Hole)\n")).unwrap_err();
    assert_eq!(e, "line 13: a Form follows a surface (surface=) or visits points (points=), and this line names both — the reference key is the motion, so keep the one it is", "{e}");
    let e = parse::parse(&format!("{REF_PREAMBLE}run End(tool=T1, surface=Top, stepover=5, profile=Top)\n")).unwrap_err();
    assert_eq!(e, "line 13: an End names its feature in surface=; profile= is not read");
    let e = parse::parse(&format!("{REF_PREAMBLE}run Side(tool=T1, surface=Hole, stepover=2, points=[Hole])\n")).unwrap_err();
    assert_eq!(e, "line 13: a Side names its feature in surface=; points= is not read");
    let e = parse::parse(&format!("{REF_PREAMBLE}run Clear(tool=T1, profile=Hole, stepover=4, surface=Hole)\n")).unwrap_err();
    assert_eq!(e, "line 13: a Clear names its feature in profile=; surface= is not read");
    // The rule fires at the line, before the reference is resolved: a
    // second key naming NOTHING is the same refusal, not "not defined".
    let e = parse::parse(&format!("{REF_PREAMBLE}run End(tool=T1, surface=Top, stepover=5, points=[Nowhere])\n")).unwrap_err();
    assert_eq!(e, "line 13: an End follows a surface (surface=) or visits points (points=), and this line names both — the reference key is the motion, so keep the one it is");
    // A visit reads `points=` and nothing else.
    let e = parse::parse(&format!("{REF_PREAMBLE}run Form(points=[Hole], tool=T3, cycle=Simple, profile=Hole)\n")).unwrap_err();
    assert_eq!(e, "line 13: a Form names its features in points=; profile= is not read");
    // One reference, the verb's own: every verb still parses as before.
    let ok = parse::parse(&format!("{REF_PREAMBLE}run Form(points=[Hole], tool=T3, cycle=Simple)\nrun End(tool=T1, surface=Top, stepover=5)\nrun Clear(tool=T1, profile=Bore, stepover=4)\nrun Side(tool=T1, surface=Bore, stepover=2)\n")).unwrap();
    assert_eq!(ok.operations.len(), 4);
    assert_eq!(parse::run_reference_key(parse::OpType::Form, true), "points");
    assert_eq!(parse::run_reference_key(parse::OpType::Clear, false), "profile");
    assert_eq!(parse::run_reference_key(parse::OpType::End, false), "surface");
    for verb in [parse::OpType::Side, parse::OpType::Copy, parse::OpType::Form] {
        assert_eq!(parse::run_reference_key(verb, false), "surface");
    }
}

/// Phase 36 slice 1: `Finish` was three operations under one name. The old
/// word refuses and says which of the three the line is; a verb that names
/// the wrong part of the tool refuses and names the right one; a floor is
/// not a contact verb's; and a drill follows no surface.
#[test]
fn test_the_contact_verbs_are_checked_against_the_tool_and_the_feature() {
    const PRE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T1 Endmill(dia=10.0)\ntool T2 Ballmill(dia=6.0)\ntool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\ntool T4 Drill(dia=6.0)\n\
feature Top Plane(z=0)\n\
feature Bore CylinderBore(dia=30.0, extent=6.0, x=50.0, y=40.0, chamfer_depth=1.0)\n\
feature Dome Sphere(dia=40.0, x=50.0, y=40.0, z=-20.0)\n";
    let refuse = |line: &str| {
        parse::parse(&format!("{PRE}{line}\n")).and_then(|p| parse::validate(&p).map(|_| p)).expect_err(line)
    };
    // The old word, with and without enough declared to say which.
    let e = refuse("run Finish(tool=T1, surface=Bore, stepover=2.0)");
    assert!(e.contains("the Finish op is retired") && e.contains("This line is a side cut") && e.contains("`run Side(...)`"), "{e}");
    let e = refuse("run Finish(tool=T2, surface=Dome, stepover=2.0)");
    assert!(e.contains("This line is a copy cut") && e.contains("`run Copy(...)`"), "{e}");
    let e = refuse("run Finish(tool=T3, surface=Bore)");
    assert!(e.contains("This line is a form cut") && e.contains("`run Form(...)`"), "{e}");
    let e = refuse("run Finish(tool=T1, surface=Nowhere)");
    assert!(e.contains("the Finish op is retired") && !e.contains("This line is"), "{e}");
    // A verb that names the wrong part of the tool.
    let e = refuse("run Copy(tool=T1, surface=Bore, stepover=2.0)");
    assert!(e.contains("is not a copy cut: it is a side cut") && e.contains("`run Side(...)`"), "{e}");
    let e = refuse("run Side(tool=T2, surface=Dome, stepover=2.0)");
    assert!(e.contains("is not a side cut: it is a copy cut") && e.contains("`run Copy(...)`"), "{e}");
    let e = refuse("run Side(tool=T3, surface=Bore)");
    assert!(e.contains("is not a side cut: it is a form cut (T3 is a ChamferMill"), "{e}");
    // A floor, and a drill.
    let e = refuse("run Side(tool=T1, surface=Top)");
    assert!(e.contains("is a Plane, a floor"), "{e}");
    let e = refuse("run Form(tool=T4, surface=Bore, stepover=2.0)");
    assert!(e.contains("a Drill — a drill cuts by its form at points and follows no surface"), "{e}");
    // And the three that are right.
    for line in ["run Side(tool=T1, surface=Bore, stepover=2.0)", "run Copy(tool=T2, surface=Dome, stepover=2.0)", "run Form(tool=T3, surface=Bore)"] {
        parse::parse(&format!("{PRE}{line}\n")).and_then(|p| parse::validate(&p).map(|_| p)).unwrap_or_else(|e| panic!("{line}: {e}"));
    }
}

/// Phase 36 slice 2: `Floor` is the surface and `End` is the verb. The old
/// word refuses, and a tool with no flat end cannot make a floor.
#[test]
fn test_end_is_the_verb_and_the_tool_must_have_one() {
    const PRE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T1 Endmill(dia=10.0)\ntool T2 Ballmill(dia=6.0)\ntool T4 Drill(dia=6.0)\n\
feature Top Plane(z=0)\n";
    let run = |line: &str| parse::parse(&format!("{PRE}{line}\n")).and_then(|p| parse::validate(&p).map(|_| p));
    run("run End(tool=T1, surface=Top, stepover=5.0)").expect("a flat end on a floor");
    let e = run("run Floor(tool=T1, surface=Top, stepover=5.0)").unwrap_err();
    assert!(e.contains("the Floor op is renamed End") && e.contains("`run End(...)`"), "{e}");
    let e = run("run End(tool=T2, surface=Top, stepover=2.0)").unwrap_err();
    assert!(e.contains("cut by T2, a Ballmill") && e.contains("no flat end (it cuts with its nose and side)"), "{e}");
    let e = run("run End(tool=T4, surface=Top, stepover=2.0)").unwrap_err();
    assert!(e.contains("cut by T4, a Drill") && e.contains("it cuts with its form"), "{e}");
}

/// Phase 36 slice 3: `Drill` dissolved. A visit to points is a motion, said
/// by `points=`, under the verb that names what cuts there: a drill or a
/// chamfer mill forms, an endmill's end seats, a ball does neither.
#[test]
fn test_a_visit_to_points_is_a_motion_under_form_or_end() {
    const PRE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T1 Endmill(dia=16.0)\ntool T2 Ballmill(dia=6.0)\ntool T7 Drill(dia=6.0)\n\
feature Hole CylinderBore(dia=6.0, extent=15.0, x=30.0, y=40.0, cbore_dia=16.0, cbore_depth=6.0)\n";
    let run = |line: &str| parse::parse(&format!("{PRE}{line}\n")).and_then(|p| parse::validate(&p).map(|_| p));
    let p = run("run Form(points=[Hole], tool=T7, cycle=Simple)\nrun End(points=[Hole], tool=T1, cycle=Simple)").expect("the drill forms, the endmill seats");
    assert!(p.operations.iter().all(|op| op.at_points() && !op.follows_surface()));
    assert!(p.operations.iter().all(|op| parse::derive_op_role(op) == parse::OpRole::Untagged && parse::derive_pattern(&p, op).is_none()));
    assert_eq!(parse::run_reference_key(parse::OpType::Form, true), "points");
    assert_eq!(parse::run_reference_key(parse::OpType::End, true), "points");

    let e = run("run Drill(points=[Hole], tool=T7, cycle=Simple)").unwrap_err();
    assert!(e.contains("the Drill op is retired") && e.contains("T7 is a Drill, which cuts with its form") && e.contains("`run Form(points=[...], ...)`"), "{e}");
    let e = run("run Drill(points=[Hole], tool=T1, cycle=Simple)").unwrap_err();
    assert!(e.contains("T1 is an Endmill, which cuts with its end"), "{e}");
    assert!(e.contains("`run End(points=[...], ...)`"), "{e}");
    let e = run("run Form(points=[Hole], tool=T1, cycle=Simple)").unwrap_err();
    assert!(e.contains("Form operation at points with T1") && e.contains("`run End(points=[...], ...)`"), "{e}");
    let e = run("run End(points=[Hole], tool=T7, cycle=Simple)").unwrap_err();
    assert!(e.contains("End operation at points with T7") && e.contains("`run Form(points=[...], ...)`"), "{e}");
    let e = run("run Form(points=[Hole], tool=T2, cycle=Simple)").unwrap_err();
    assert!(e.contains("a Ballmill has neither a form nor a flat end to plunge"), "{e}");
    let e = run("run Form(points=[Hole], tool=T7, cycle=Simple, surface=Hole)").unwrap_err();
    assert!(e.contains("and this line names both"), "{e}");
    let e = run("run Form(points=[Hole], tool=T7)").unwrap_err();
    assert!(e.contains("missing 'cycle' in a points= run statement"), "{e}");
    let e = run("run Form(points=[Hole], tool=T7, cycle=Simple, stock_to_leave=0.2)").unwrap_err();
    assert!(e.contains("at points does not carry stock_to_leave"), "{e}");
}

/// Phase 36 close: `Under` is reserved and refuses by name.
#[test]
fn test_under_is_reserved_until_a_ceiling_can_be_declared() {
    let e = parse::parse("units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\ntool T1 Endmill(dia=10.0)\nfeature Top Plane(z=0)\nrun Under(tool=T1, surface=Top)\n").unwrap_err();
    assert!(e.contains("Under is reserved") && e.contains("no feature declares a ceiling yet"), "{e}");
}

// ---- Phase 38 slice 2: the walk's arm of the engagement family -----------------
// (`DECISIONS.md` → "Phase 38 slice 2…", forks 3A, 4A, 5A.)

fn slot_findings_of(dir: &str) -> (parse::Part, Vec<parse::Finding>) {
    let src = fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = fs::read_to_string(format!("{dir}/geometry.part")).ok();
    let p = parse::parse_project_with_base(&src, geom.as_deref(), Some(std::path::Path::new(dir))).unwrap_or_else(|e| panic!("{dir}: {e}"));
    let (_, emissions) = gcode::program_with_moves(&p).unwrap_or_else(|e| panic!("{dir}: {e}"));
    let findings = nacs::engagement::slot_findings(&p, &emissions);
    (p, findings)
}

/// The roster: over the whole shipped corpus, the ops that are mostly a
/// slot are exactly the seven the DRAFT counted — six under spine (the
/// three rail fixtures' lands, two rails of their own, `wrench_step`'s box
/// top) and one under the offset family (`walled_rails_ladder`'s last op, a
/// rest floor under a 3mm tool). The ratchet: an eighth is a finding to
/// read, a sixth is a voice gone quiet, and either fails here by name.
#[test]
fn test_the_mostly_a_slot_advisory_speaks_on_exactly_seven_shipped_ops() {
    let mut spoke: Vec<(String, String)> = Vec::new();
    let mut dirs: Vec<String> = fs::read_dir(".").unwrap().filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string())).filter(|n| n.ends_with(".cam")).collect();
    dirs.sort();
    // The same arm says "cuts nothing" (ROADMAP item 3 slice 6) — on one
    // shipped op: pocket_levels' End, a floor finish after a Clear that
    // leaves it no skin (`stock_to_leave` unsaid), runs at the floor in the
    // air the Clear left. floor_travel's face stood at the blank's top and
    // skimmed nothing until that slice lowered it half a millimetre. The
    // same ratchet: a third is a finding to read, none is a voice gone quiet.
    let mut nothing: Vec<(String, String)> = Vec::new();
    for dir in &dirs {
        let (p, findings) = slot_findings_of(dir);
        for f in findings {
            assert_eq!(f.family, parse::FindingFamily::Engagement, "{dir}: the walk's arm is the engagement family's");
            let parse::FindingSubject::Op { index, feature } = &f.subject else { panic!("{dir}: about an op") };
            assert_eq!(&p.operations[*index].feature_name, feature);
            if f.message.contains("cuts nothing") {
                nothing.push((dir.clone(), feature.clone()));
                continue;
            }
            assert!(f.message.contains("mostly a slot") && f.message.contains("`nacs --patterns`"), "{dir}: {}", f.message);
            spoke.push((dir.clone(), feature.clone()));
        }
    }
    assert_eq!(nothing, vec![("pocket_levels.cam".to_string(), "Window".to_string())], "the shipped ops that cut nothing");
    let expected: Vec<(String, String)> = [
        ("curved_rails_spine.cam", "Land"),
        ("curved_rails_spine.cam", "SpiralBoss"),
        ("rail_plate_spine.cam", "Land"),
        ("rail_plate_spine.cam", "BarRail"),
        ("walled_rails_ladder_offset.cam", "Land"),
        ("walled_rails_spine.cam", "Land"),
        ("wrench_step_spine.cam", "BoxTop"),
    ]
    .iter()
    .map(|(d, f)| (d.to_string(), f.to_string()))
    .collect();
    assert_eq!(spoke, expected, "the roster of the seven");
}

/// The sentence, word for word on the rail plate's land: the length, the
/// share, the tool's width, the depth, and where to look. Every number in
/// it has an older home — the lengths are the report's `slot` and its
/// engaged length (`compare::load_as_emitted`), the depth the op-history
/// stock's.
#[test]
fn test_the_mostly_a_slot_sentence_says_the_length_the_share_and_the_depth() {
    let (_, findings) = slot_findings_of("rail_plate_spine.cam");
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(
        findings[0].message,
        "End on 'Land' with T5: mostly a slot — 888mm of the 1369mm it cuts in material at depth (65%) is cut at the tool's full width (10.000mm), 1.000mm deep. The walk measured it from the moves; `nacs --patterns` shows what each other pattern would meet on this floor"
    );
    // A rail the tool is wider than is a slot end to end.
    assert!(findings[1].message.starts_with("End on 'BarRail' with T5: mostly a slot — 32mm of the 32mm it cuts in material at depth (100%)"), "{}", findings[1].message);
    // The ladder's rest floor: a 3mm tool, eight deep — the depth is what
    // stood over the floor when the op began, not the stock's height.
    let (_, ladder) = slot_findings_of("walled_rails_ladder_offset.cam");
    assert_eq!(ladder.len(), 1);
    assert!(ladder[0].message.contains("with T6: mostly a slot") && ladder[0].message.contains("full width (3.000mm), 8.000mm deep"), "{}", ladder[0].message);
}

/// One op, one voice. Where the band answers — a `Clear` on a pocket under
/// the offset family — the band's corner advisory speaks and the walk's arm
/// is silent, even on `pocket_islands`, whose band reads a full slot. And
/// below the line it is silent: `thin_wall` under zigzag cuts 20mm of its
/// 315mm in a slot, and says nothing.
#[test]
fn test_the_walks_arm_is_silent_where_the_band_answers_and_below_the_line() {
    let (p, findings) = slot_findings_of("pocket_islands.cam");
    assert!(findings.is_empty(), "{findings:?}");
    let band = parse::engagement_findings(&p);
    assert_eq!(band.len(), 1, "the band's own voice on the pocket: {band:?}");
    assert!(band[0].message.contains("180.0°"));
    let row = nacs::compare::current_row(&p, 0).unwrap().outcome.unwrap();
    assert!(row.worst_deg.is_some(), "the band answers for this op");

    let (_, thin) = slot_findings_of("thin_wall.cam");
    assert!(thin.is_empty(), "{thin:?}");
    let (p, deck) = slot_findings_of("wrench_step_spine.cam");
    let names: Vec<&str> = deck.iter().map(|f| match &f.subject { parse::FindingSubject::Op { feature, .. } => feature.as_str(), _ => "" }).collect();
    assert_eq!(names, ["BoxTop"], "the deck beside it is wide enough to row");
    assert_eq!(p.operations.len(), 2);
}

/// Advisory only (fork 5A): under `engagement gate` the program still
/// emits, byte for byte the golden, and the finding is still said — the
/// gate escalates the band's findings at validate, before a move exists,
/// and this arm reads moves. The pipeline carries it in the engagement
/// family beside the post-generation advisories, and the CLI prints it on
/// stderr while stdout stays the golden.
#[test]
fn test_the_gate_does_not_escalate_the_walks_arm_and_the_cli_prints_it() {
    let src = fs::read_to_string("rail_plate_spine.cam/main.part").unwrap();
    let geom = fs::read_to_string("rail_plate_spine.cam/geometry.part").unwrap();
    let gated = src.replace("units mm", "units mm\nengagement gate");
    assert_ne!(gated, src);
    let p = parse::parse_project_with_base(&gated, Some(&geom), Some(std::path::Path::new("rail_plate_spine.cam"))).expect("the gate has nothing of the band's to reject");
    let (g, emissions) = gcode::program_with_moves(&p).expect("emits under the gate");
    assert_eq!(g.trim(), fs::read_to_string("tests/rail_plate_spine.golden.gcode").unwrap().trim());
    assert_eq!(nacs::engagement::slot_findings(&p, &emissions).len(), 2);
    let v = nacs::check_result::check_result(&p);
    assert!(v.would_emit());
    assert_eq!(v.findings.iter().filter(|f| f.family == parse::FindingFamily::Engagement).count(), 2);

    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).arg("rail_plate_spine.cam").output().expect("run");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), fs::read_to_string("tests/rail_plate_spine.golden.gcode").unwrap().trim());
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(err.matches("warning: End on 'Land' with T5: mostly a slot").count(), 1, "{err}");
    assert_eq!(err.matches("mostly a slot").count(), 2, "{err}");
}

/// One op's block of an emitted program: from its title line to the next.
fn op_block<'a>(g: &'a str, title: &str) -> &'a str {
    let start = g.find(title).unwrap_or_else(|| panic!("no block titled {title:?}"));
    let rest = &g[start + title.len()..];
    let end = rest.find("\n(--- ").map(|i| start + title.len() + i).unwrap_or(g.len());
    &g[start..end]
}

/// Every `Z` word on a `G01` line of a block, in order.
fn feed_zs(block: &str) -> Vec<f64> {
    block
        .lines()
        .filter(|l| l.starts_with("G01 "))
        .filter_map(|l| l.split_whitespace().find(|w| w.starts_with('Z')).map(|w| w[1..].parse::<f64>().unwrap()))
        .collect()
}

/// Phase 45 slice 1 (`DECISIONS.md` → "DRAFT: Phase 45 opens…", fork 1A):
/// `z` on a `CylinderBore` is its RIM. `step_bore.cam` faces a step to −8
/// and authors two bores from it; every verb that reads a bore's top reads
/// the rim, pinned here over the moves and not read off the golden.
#[test]
fn test_step_bore_golden_and_rim_pins() {
    let main_src = fs::read_to_string("step_bore.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse step_bore.cam");
    parse::validate(&part).expect("fixture must validate");
    let g = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/step_bore.golden.gcode").expect("failed to read step_bore golden");
    assert_eq!(g.trim(), golden.trim(), "step_bore.cam diverged from its golden");

    // The Clear: the descent starts AT the rim, not at the stock top, so the
    // ramp is priced over the bore's own 6mm (10 laps) and not over 14mm of
    // which 8 would be air; the floor is rim − extent.
    let clear = op_block(&g, "(--- bore clear: Bore with T5 ---)");
    let zs = feed_zs(clear);
    assert_eq!(zs[0], -8.0, "the first feed move is the plunge to the rim: {clear}");
    assert!(clear.contains("(ramp: 10 laps of this loop"), "{clear}");
    let floor = zs.iter().cloned().fold(f64::INFINITY, f64::min);
    assert!((floor - -14.0).abs() < 1e-9, "the Clear's floor is rim − extent, got {floor}");

    // The Side: stacked rings step down from the rim by `stepover`, the last
    // exactly on the floor.
    let side = op_block(&g, "(--- side bore wall (engine): Bore with T5 ---)");
    let zs = feed_zs(side);
    assert_eq!(zs.first().copied(), Some(-11.0), "first ring one stepover under the rim: {side}");
    assert_eq!(zs.last().copied(), Some(-14.0), "last ring on the floor: {side}");

    // The visit: pecks count from the rim (−11, −14, −16), never from the
    // stock top (which would have put the first at −3).
    let form = op_block(&g, "(--- form points: Hole with T7 ---)");
    assert_eq!(feed_zs(form), vec![-11.0, -14.0, -16.0], "{form}");
}

/// Premise 4 of the DRAFT, on the bore: what stands above a sunken rim is
/// another feature's material, and a plunge through it is refused at the
/// plunge, by name — never a program that cuts the step as if it were air.
#[test]
fn test_step_bore_without_its_step_is_refused_at_the_plunge() {
    let main_src = fs::read_to_string("step_bore.cam/main.part").expect("failed to read main.part");
    let no_end: String = main_src.lines().filter(|l| !l.starts_with("run End(")).map(|l| format!("{l}\n")).collect();
    let part = parse::parse(&no_end).expect("parse");
    parse::validate(&part).expect("validate does not hold the step — the entry restriction does");
    let err = gcode::program(&part).expect_err("the Clear must be refused");
    assert!(err.contains("Clear on 'Bore' with T5"), "{err}");
    assert!(err.contains("`G01 Z-8.000 F300.000`"), "refused at the plunge to the rim: {err}");
    assert!(err.contains("material that outlives this operation"), "{err}");
}

/// The rim is a length: it converts at the mm boundary like the pocket's.
#[test]
fn test_bore_rim_converts_at_the_inch_boundary() {
    let src = "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\n\
               stock Box(w=4, l=4, h=1, x=0, y=0, z=0)\ntool T5 Endmill(dia=0.5)\n\
               feature B CylinderBore(dia=1.0, extent=0.25, x=2, y=2, z=-0.5)\n";
    let p = parse::parse(src).expect("parse");
    match &p.features["B"] {
        parse::Feature::CylinderBore { z: Some(z), extent, .. } => {
            assert!((z - -12.7).abs() < 1e-9, "rim in mm: {z}");
            assert!((extent - 6.35).abs() < 1e-9, "extent in mm: {extent}");
            assert!((parse::derive_contour_top(p.stock.z, Some(*z)) - -12.7).abs() < 1e-9);
        }
        other => panic!("expected a bore with a rim, got {other:?}"),
    }
}

/// A ROUND blank (Phase 46 slice 1): `disc_step.cam` declares `stock
/// Cylinder` and cuts a step, a pad near the rim and a bore from the step.
/// The golden is the program; the facts about the blank are pinned over the
/// derived views in `tests/stock_shape_tests.rs`, never read off this file.
#[test]
fn test_disc_step_golden() {
    let main_src = fs::read_to_string("disc_step.cam/main.part").expect("failed to read main.part");
    let part = parse::parse(&main_src).expect("failed to parse disc_step.cam");
    parse::validate(&part).expect("fixture must validate");
    let g = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/disc_step.golden.gcode").expect("failed to read disc_step golden");
    assert_eq!(g.trim(), golden.trim(), "disc_step.cam diverged from its golden");
    // The pad's pass runs off the round: its farthest point is past r 60.
    let side = op_block(&g, "(--- side contour (engine): Pad with T5 ---)");
    let reach = side.lines().filter(|l| l.starts_with("G01") && l.contains('X')).map(|l| {
        let num = |key: char| l.split_whitespace().find(|w| w.starts_with(key)).and_then(|w| w[1..].parse::<f64>().ok()).unwrap_or(0.0);
        num('X').hypot(num('Y'))
    }).fold(0.0, f64::max);
    assert!(reach > 60.0, "the pass around the pad swings past the blank's radius: {reach:.3}");
}

/// Phase 47 slice 1 (fork 2A): `disc_ribs.cam` is the real part's upper
/// half in miniature — a `Face` level run out past the round, two pads ON
/// the round to three decimals, two sharp L-bosses across a slot 1.1 tool
/// diameters wide. Before this slice the containment lint doubted the three
/// (the pads "0.000mm past", the level 0.020), the entry restriction
/// DECLINED on every op that worked them, and the rest report declined
/// whole. Now: stderr empty, every op read, the rest report CLAIMS — and
/// forgets nothing: the two notch fillets the coverage calls nobody's to cut
/// are declared standing too (2026-11-10). The golden is the program; the
/// facts about the blank are pinned over the derived views in
/// `tests/stock_shape_tests.rs`.
#[test]
fn test_disc_ribs_golden_and_every_voice_reads() {
    let part = parse::load_project("disc_ribs.cam").expect("load disc_ribs.cam");
    parse::validate(&part).expect("fixture must validate");
    let g = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/disc_ribs.golden.gcode").expect("failed to read disc_ribs golden");
    assert_eq!(g.trim(), golden.trim(), "disc_ribs.cam diverged from its golden");
    let said: Vec<String> = parse::lint(&part).into_iter().map(|f| f.message).collect();
    assert!(said.is_empty(), "disc_ribs.cam is silent on every voice: {said:?}");
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--rest-report", "disc_ribs.cam"]).output().expect("run nacs");
    let report = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{report}");
    assert!(report.contains("swept field: complete") && report.contains("declared model: complete"), "{report}");
    assert!(!report.contains("UNCONTAINED") && !report.contains("decline"), "{report}");
    // The notch fillets are declared standing, so nothing is forgotten.
    assert!(report.contains("forgotten (declared removed, never swept): none"), "{report}");
}

/// Phase 49 slice 1 (fork 1A): `rim_boss_offset.cam` puts two wedge bosses
/// ON a round blank's silhouette — where every rim boss imported from a STEP
/// lands — under a `Clear` of the offset family. Read as islands, each rib's
/// loop a radius out and the pushed open edge's loop a radius in met on one
/// circle and the outermost level came back as one stub per chord (on the
/// four-rib experiment 39 loops, 28 under 3mm, 72 lifts, 18 s). Read as the
/// shore — bitten out of the boundary — the level is whole: the golden holds
/// the program, and this pins what the golden cannot say by itself: that no
/// loop of the Clear is a stub, and that the program says what it read.
#[test]
fn test_rim_boss_offset_golden_and_no_loop_is_a_stub() {
    let part = parse::load_project("rim_boss_offset.cam").expect("load rim_boss_offset.cam");
    parse::validate(&part).expect("fixture must validate");
    let g = gcode::program(&part).expect("failed to generate program");
    let golden = fs::read_to_string("tests/rim_boss_offset.golden.gcode").expect("failed to read rim_boss_offset golden");
    assert_eq!(g.trim(), golden.trim(), "rim_boss_offset.cam diverged from its golden");
    let said: Vec<String> = parse::lint(&part).into_iter().map(|f| f.message).collect();
    // One voice, and it is the offset family's own slot where the deepest
    // level's lenses end (the engagement-quiet test lists it); nothing about
    // the bite, the containment or the entry restriction.
    assert_eq!(said.len(), 1, "rim_boss_offset.cam says one thing: {said:?}");
    assert!(said[0].contains("the tool actually reaches 180.0°"), "{}", said[0]);
    assert!(g.contains("(on the shore: RimE, RimW)"), "the Clear says what it bit out");
    assert!(!g.contains("(islands:"), "nothing is an island of this level");
    // The Clear's loops, each measured from its first cutting move to its
    // lift: a stub is a loop shorter than one chord of the ribs' outer arc
    // (about 1.8mm); the old reading made 28 of them.
    let clear: Vec<&str> = g.lines().skip_while(|l| !l.starts_with("(--- pocket clear: Level")).skip(1).take_while(|l| !l.starts_with("(--- ")).collect();
    let xy = |l: &str| -> Option<(f64, f64)> {
        let w = |c: char| l.split_whitespace().find_map(|t| t.strip_prefix(c).and_then(|v| v.parse::<f64>().ok()));
        Some((w('X')?, w('Y')?))
    };
    let (mut loops, mut len, mut last): (Vec<f64>, f64, Option<(f64, f64)>) = (Vec::new(), 0.0, None);
    for l in &clear {
        if l.starts_with("G01 X") {
            let q = xy(l).expect("a cutting move has X and Y");
            if let Some(p) = last {
                len += (q.0 - p.0).hypot(q.1 - p.1);
            }
            last = Some(q);
        } else if l.starts_with("G00 Z") && last.is_some() {
            loops.push(len);
            len = 0.0;
            last = None;
        }
    }
    assert!(loops.len() >= 5 && loops.len() <= 12, "a handful of whole loops, not a shower of stubs: {loops:?}");
    assert!(loops.iter().all(|l| *l > 30.0), "no loop of the Clear is a stub: {loops:?}");
}
