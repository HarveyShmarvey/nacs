// Phase-4 sidecar contract tests (sidecar/CONTRACT.md). Kept out of
// check_tests.rs so the core test file stays sidecar-free.
//
// Only test_sidecar_golden runs the Python sidecar, and it skips when the
// venv isn't installed — the same gating convention as test_python_parity.
// A *present but broken* venv fails loudly instead of skipping: a silent
// skip on a half-installed sidecar is exactly the failure shape the
// total-accounting decision exists to ban. The other two tests run against
// the committed golden artifacts, so the contract's invariants stay pinned
// even on machines without the 1.5 GB venv.

use std::fs;
use std::path::Path;
use std::process::Command;
use nacs::{parse, gcode, outline};

// The venv's interpreter, where `python -m venv` puts it on each platform.
// Same path either way from the user's side (sidecar/README.md); the tests
// skip-gate on it, so the Windows layout must be named or the sidecar tests
// skip silently beside an installed venv.
#[cfg(not(windows))]
const VENV_PYTHON: &str = "sidecar/.venv/bin/python";
#[cfg(windows)]
const VENV_PYTHON: &str = "sidecar/.venv/Scripts/python.exe";

// Every committed fixture: (STEP input, extra sidecar args, report golden,
// proposed golden). block_bore is the slice-1 walking skeleton (planes +
// bores); quadric_boss is the slice-2 quadric breadth fixture (sphere /
// cone / torus); pocket_plate is the slice-3 pocket-recognition fixture
// (grouped floor-plus-wall-loop Contour proposals); drilled_plate is the
// slice-4 hole-vs-bore fixture (one cylindrical void per bottom kind);
// assembly_pair and tilted_plate are the slice-5 import-breadth fixtures
// (a declared --solid selection and a declared --up remap); outer_profile
// is the slice-7 outer-profile fixture (cap-anchored CW contours, the
// Feature::CylinderBoss circular boss, and the two deliberate errors);
// trim_wire is the 3D-wire trim fixture (part-bottom-anchored ContourBoss/
// ContourPocket 3D wires and the curved-trim-edge error); lip_bore is the
// mouth-round fixture (a filleted bore mouth absorbed as a declared
// edge_round, with extent corrected to the virtual sharp corner).
const FIXTURES: &[(&str, &[&str], &str, &str)] = &[
    (
        "tests/fixtures/block_bore.step",
        &[],
        "tests/fixtures/block_bore.report.golden.json",
        "tests/fixtures/block_bore.proposed.golden.part",
    ),
    (
        "tests/fixtures/quadric_boss.step",
        &[],
        "tests/fixtures/quadric_boss.report.golden.json",
        "tests/fixtures/quadric_boss.proposed.golden.part",
    ),
    (
        "tests/fixtures/pocket_plate.step",
        &[],
        "tests/fixtures/pocket_plate.report.golden.json",
        "tests/fixtures/pocket_plate.proposed.golden.part",
    ),
    (
        "tests/fixtures/drilled_plate.step",
        &[],
        "tests/fixtures/drilled_plate.report.golden.json",
        "tests/fixtures/drilled_plate.proposed.golden.part",
    ),
    (
        "tests/fixtures/assembly_pair.step",
        &["--solid", "1"],
        "tests/fixtures/assembly_pair.report.golden.json",
        "tests/fixtures/assembly_pair.proposed.golden.part",
    ),
    (
        "tests/fixtures/tilted_plate.step",
        &["--up", "y+"],
        "tests/fixtures/tilted_plate.report.golden.json",
        "tests/fixtures/tilted_plate.proposed.golden.part",
    ),
    (
        "tests/fixtures/outer_profile.step",
        &[],
        "tests/fixtures/outer_profile.report.golden.json",
        "tests/fixtures/outer_profile.proposed.golden.part",
    ),
    (
        "tests/fixtures/trim_wire.step",
        &[],
        "tests/fixtures/trim_wire.report.golden.json",
        "tests/fixtures/trim_wire.proposed.golden.part",
    ),
    (
        "tests/fixtures/lip_bore.step",
        &[],
        "tests/fixtures/lip_bore.report.golden.json",
        "tests/fixtures/lip_bore.proposed.golden.part",
    ),
    // Phase 26 slice 4: the bossed pocket — a floor with two inner wires,
    // which refused (`pocket_islands`) until the engine could clear around
    // what stands on them.
    (
        "tests/fixtures/pocket_post.step",
        &[],
        "tests/fixtures/pocket_post.report.golden.json",
        "tests/fixtures/pocket_post.proposed.golden.part",
    ),
    // Phase 28 slice 4: a ledge — a below-top +Z plane capping an outer
    // profile, with an ELLIPTIC inner wire — proposes as a bounded `Face`.
    (
        "tests/fixtures/ledge_face.step",
        &[],
        "tests/fixtures/ledge_face.report.golden.json",
        "tests/fixtures/ledge_face.proposed.golden.part",
    ),
    // A bore on a step below the stock top is PLACED by `z=` (Phase 45
    // slice 3) beside one at the top that omits it — the first outside STEP
    // part's counterbores were proposed 30 mm too high before 2026-10-09,
    // refused for a day, and propose at their true height since.
    (
        "tests/fixtures/sunken_bore.step",
        &[],
        "tests/fixtures/sunken_bore.report.golden.json",
        "tests/fixtures/sunken_bore.proposed.golden.part",
    ),
    // Phase 46 slice 2: a floor whose wire runs out to the part's
    // SILHOUETTE (and along the rim of a bore from below) proposes as a
    // pocket carrying its true wire, the silhouette edge drawn past the
    // blank for the engine to clip; it was `pocket_floor_of_bore` before.
    (
        "tests/fixtures/disc_step.step",
        &[],
        "tests/fixtures/disc_step.report.golden.json",
        "tests/fixtures/disc_step.proposed.golden.part",
    ),
    // Phase 46 slice 3: a bevel is a leg on the feature whose edge it
    // breaks — a boss's top edge, a pocket's mouth, a bore's mouth — and a
    // bevel that is a wall's whole height refuses by the engine's sentence.
    (
        "tests/fixtures/chamfer_plate.step",
        &[],
        "tests/fixtures/chamfer_plate.report.golden.json",
        "tests/fixtures/chamfer_plate.proposed.golden.part",
    ),
    // ROADMAP item 1, slice 5: a boss whose top edges carry two different
    // bevels (the real part's ribs) proposes as a ProfileBoss without a
    // chamfer, its loop read at the walls; each strip stays residue saying
    // which bevels it saw. Before, the whole boss refused.
    (
        "tests/fixtures/uneven_bevels.step",
        &[],
        "tests/fixtures/uneven_bevels.report.golden.json",
        "tests/fixtures/uneven_bevels.proposed.golden.part",
    ),
    // ROADMAP item 1, slice 5: a rib running out to the part's edge (one
    // wall of its loop is the part's own side, down to the bottom) proposes
    // to the highest base and names the wall that runs on; a head on a
    // narrower neck proposes with its underside named as the other setup's.
    (
        "tests/fixtures/rib_head.step",
        &[],
        "tests/fixtures/rib_head.report.golden.json",
        "tests/fixtures/rib_head.proposed.golden.part",
    ),
    // ROADMAP item 1, slice 5: a head on a neck that stands on the part
    // bottom has no proposed floor under it, so it proposes with its
    // underside named as the other setup's (the real part's ribs).
    (
        "tests/fixtures/mushroom.step",
        &[],
        "tests/fixtures/mushroom.report.golden.json",
        "tests/fixtures/mushroom.proposed.golden.part",
    ),
    // ROADMAP item 1, slice 6: the real part's shape in small -- heads on
    // necks on pads over a floor. Each head proposes down to the floor it
    // stands over, what is under it is residue saying nothing from above
    // reaches it, and the floor proposes as the level, one Plane.
    (
        "tests/fixtures/rib_disc.step",
        &[],
        "tests/fixtures/rib_disc.report.golden.json",
        "tests/fixtures/rib_disc.proposed.golden.part",
    ),
    // Phase 48 slice 3: the far mouth's bevel. This way up the cone at the
    // hole's bottom is the other setup's chamfer and the hole proposes
    // through; turned over it is the near mouth's `chamfer_depth=`. The two
    // imports agree on the hole's column.
    (
        "tests/fixtures/through_chamfer.step",
        &[],
        "tests/fixtures/through_chamfer.report.golden.json",
        "tests/fixtures/through_chamfer.proposed.golden.part",
    ),
    (
        "tests/fixtures/through_chamfer.step",
        &["--up", "z-"],
        "tests/fixtures/through_chamfer_back.report.golden.json",
        "tests/fixtures/through_chamfer_back.proposed.golden.part",
    ),
    (
        "tests/fixtures/rib_disc.step",
        &["--up", "z-"],
        "tests/fixtures/rib_disc_back.report.golden.json",
        "tests/fixtures/rib_disc_back.proposed.golden.part",
    ),
    (
        "tests/fixtures/step_hole.step",
        &["--up", "z-"],
        "tests/fixtures/step_hole_back.report.golden.json",
        "tests/fixtures/step_hole_back.proposed.golden.part",
    ),
    (
        "tests/fixtures/step_hole.step",
        &[],
        "tests/fixtures/step_hole.report.golden.json",
        "tests/fixtures/step_hole.proposed.golden.part",
    ),
    // A groove across a drilled hole: two wall faces on one axis with air
    // between are one hole, proposed once, from either side.
    (
        "tests/fixtures/split_hole.step",
        &[],
        "tests/fixtures/split_hole.report.golden.json",
        "tests/fixtures/split_hole.proposed.golden.part",
    ),
    (
        "tests/fixtures/split_hole.step",
        &["--up", "z-"],
        "tests/fixtures/split_hole_back.report.golden.json",
        "tests/fixtures/split_hole_back.proposed.golden.part",
    ),
    // ROADMAP item 9: a pocket with a floor fillet and a rounded mouth that
    // runs out onto a bevel. It proposes, open on the bevel's side.
    (
        "tests/fixtures/open_pocket.step",
        &[],
        "tests/fixtures/open_pocket.report.golden.json",
        "tests/fixtures/open_pocket.proposed.golden.part",
    ),
    (
        "tests/fixtures/block_level.step",
        &[],
        "tests/fixtures/block_level.report.golden.json",
        "tests/fixtures/block_level.proposed.golden.part",
    ),
];

/// The allowance (ROADMAP item 1, slice 11): `--allowance 1` grows a
/// bounding blank by 1 on the sides and the top and leaves a prism, its own
/// blank, alone; the recognizer run with it drills a plain through hole from
/// the blank top, places a bore whose mouth is its rim by `z=`, and proposes
/// the plan-bounding cylinder as the rim boss to bring to size. Without the
/// flag both scripts write what their goldens hold. Skips without the venv.
#[test]
fn test_the_allowance_grows_the_blank_places_the_top_bores_and_proposes_the_rim() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping sidecar allowance test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_sidecar_allowance_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);
    let stock = |step: &str, sub: &str| {
        let out = Command::new(VENV_PYTHON).args(["sidecar/import_stock.py", step, "--allowance", "1", "-o"]).arg(outdir.join(sub)).output().expect("failed to run the sidecar");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        fs::read_to_string(outdir.join(sub).join("proposed_stock.part")).unwrap()
    };
    let disc = stock("tests/fixtures/disc_step.step", "disc");
    assert!(disc.ends_with("stock Cylinder(dia=122.0, h=31.0, x=0.0, y=0.0, z=31.0)\n") && disc.contains("Plus an allowance of 1.0"), "{disc}");
    let plate = stock("tests/fixtures/chamfer_plate.step", "plate");
    assert!(plate.ends_with("stock Box(w=102.0, l=82.0, h=29.0, x=-1.0, y=-1.0, z=29.0)\n"), "{plate}");
    let prism = stock("tests/fixtures/l_plate.step", "prism");
    assert!(prism.contains("stock Profile(h=20.0, z=20.0\n") && prism.contains("the allowance 1.0 is not added"), "{prism}");
    let import = |step: &str, sub: &str| {
        let out = Command::new(VENV_PYTHON).args(["sidecar/import_step.py", step, "--allowance", "1", "-o"]).arg(outdir.join(sub)).output().expect("failed to run the sidecar");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        fs::read_to_string(outdir.join(sub).join("proposed.part")).unwrap()
    };
    let disc = import("tests/fixtures/disc_step.step", "disc_f");
    assert!(disc.contains("\nfeature F1_Rim CylinderBoss(dia=120.0, extent=20.0, x=0.0, y=0.0, z=0.0)\n"), "{disc}");
    assert!(disc.contains("\nfeature F7_Bore CylinderBore(dia=12.0, extent=18.0, x=20.0, y=-35.0, z=30.0)\n"), "{disc}");
    let plate = import("tests/fixtures/drilled_plate.step", "plate_f");
    assert!(plate.contains("\nfeature F7_Hole CylinderBore(dia=12.0, extent=16.0, x=20.0, y=20.0)\n"), "{plate}");
    assert!(plate.contains("\nfeature F9_Bore CylinderBore(dia=16.0, extent=6.0, x=80.0, y=20.0, z=15.0)\n"), "{plate}");
    let _ = fs::remove_dir_all(&outdir);
}

/// A second setup (slice 12): with `--given <project> --blank <words>` the
/// stock proposal is the words as given — the earlier program's blank turned
/// over, which the engine computes — with the `given from` line beside it,
/// and the recognizer withdraws the plan-bounding rim by name (the earlier
/// setup's to bring to size) and reads the blank's margins instead of an
/// allowance. The two flags go together, a model outside the blank is an
/// error, and so is an allowance beside a blank. Skips without the venv.
#[test]
fn test_a_given_blank_is_the_earlier_programs_turned_over_and_the_rim_is_that_setups() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping sidecar given-blank test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_sidecar_given_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);
    let blank = "Cylinder(dia=122, h=31, x=0, y=0, z=0)";
    let out = Command::new(VENV_PYTHON).args(["sidecar/import_stock.py", "tests/fixtures/disc_step.step", "--up", "z-", "--given", "../front.cam", "--blank", blank, "-o"]).arg(outdir.join("stock")).output().expect("failed to run the sidecar");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stock = fs::read_to_string(outdir.join("stock").join("proposed_stock.part")).unwrap();
    assert!(stock.ends_with("stock Cylinder(dia=122.0, h=31.0, x=0.0, y=0.0, z=0.0)\ngiven from ../front.cam up=z-\n"), "{stock}");
    assert!(stock.contains("stands 1.0 outside the model's plan and 0.0 above its top"), "{stock}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("the blank of ../front.cam turned over"));
    let out = Command::new(VENV_PYTHON).args(["sidecar/import_step.py", "tests/fixtures/disc_step.step", "--up", "z-", "--given", "../front.cam", "--blank", blank, "-o"]).arg(outdir.join("import")).output().expect("failed to run the sidecar");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report = fs::read_to_string(outdir.join("import").join("report.json")).unwrap();
    assert!(report.contains("\"code\": \"rim_given\"") && report.contains("\"given\": \"../front.cam\"") && report.contains("\"blank\": \"Cylinder(dia=122.0, h=31.0, x=0.0, y=0.0, z=0.0)\""), "{report}");
    let part = fs::read_to_string(outdir.join("import").join("proposed.part")).unwrap();
    assert!(part.contains("# imported with --given ../front.cam") && !part.contains("_Boss") && !part.contains("_Rim"), "{part}");
    for (args, says) in [
        (vec!["--blank", blank], "go together"),
        (vec!["--given", "../f.cam", "--blank", "Cylinder(dia=100, h=31, x=0, y=0, z=0)"], "stands 10.0 outside the given blank"),
        (vec!["--given", "../f.cam", "--blank", blank, "--allowance", "1"], "--allowance with --blank"),
        (vec!["--up", "x+", "--given", "../f.cam", "--blank", blank], "only z+"),
    ] {
        let out = Command::new(VENV_PYTHON).args(["sidecar/import_stock.py", "tests/fixtures/disc_step.step", "-o"]).arg(outdir.join("refused")).args(&args).output().unwrap();
        assert!(!out.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains(says), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(!outdir.join("refused").exists(), "a refusal writes nothing");
    let _ = fs::remove_dir_all(&outdir);
}

/// The stock proposal: `import_stock.py` on a plain prism writes the
/// `stock Profile` block the golden holds, byte for byte; on anything else
/// the smallest blank the model fits — a `stock Cylinder` for the disc, a
/// `stock Box` for the chamfered plate — with the comment saying so
/// (ROADMAP item 1, slice 2). A solid it cannot choose (an assembly with no
/// --solid) is an error that writes nothing. Skips without the venv.
#[test]
fn test_sidecar_stock_proposal_golden() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping sidecar stock test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_sidecar_stock_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);
    let output = Command::new(VENV_PYTHON).args(["sidecar/import_stock.py", "tests/fixtures/l_plate.step", "-o"]).arg(&outdir).output().expect("failed to run the sidecar");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let proposed = fs::read_to_string(outdir.join("proposed_stock.part")).expect("the sidecar wrote no proposed_stock.part");
    assert_eq!(proposed, fs::read_to_string("tests/fixtures/l_plate.stock.golden.part").unwrap(), "proposed_stock.part diverged from its golden");
    for (step, golden) in [("tests/fixtures/disc_step.step", "tests/fixtures/disc_step.stock.golden.part"), ("tests/fixtures/chamfer_plate.step", "tests/fixtures/chamfer_plate.stock.golden.part")] {
        let sub = outdir.join(Path::new(step).file_stem().unwrap());
        let output = Command::new(VENV_PYTHON).args(["sidecar/import_stock.py", step, "-o"]).arg(&sub).output().expect("failed to run the sidecar");
        assert!(output.status.success(), "{step}: {}", String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("the smallest blank the model fits"), "{step}: the summary line says what kind of proposal it is");
        let proposed = fs::read_to_string(sub.join("proposed_stock.part")).expect("the sidecar wrote no proposed_stock.part");
        assert_eq!(proposed, fs::read_to_string(golden).unwrap(), "{step}: proposed_stock.part diverged from its golden");
    }
    let refused = Command::new(VENV_PYTHON).args(["sidecar/import_stock.py", "tests/fixtures/assembly_pair.step", "-o"]).arg(outdir.join("refused")).output().unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("2 solids"), "{}", String::from_utf8_lossy(&refused.stderr));
    assert!(!outdir.join("refused").join("proposed_stock.part").exists(), "nothing written on a refusal");
    let _ = fs::remove_dir_all(&outdir);
}

#[test]
fn test_sidecar_golden() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping sidecar golden test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    for (step, extra_args, golden_report, golden_proposed) in FIXTURES {
        let outdir = std::env::temp_dir().join(format!(
            "nacs_sidecar_golden_{}_{}",
            std::process::id(),
            Path::new(step).file_stem().unwrap().to_string_lossy()
        ));
        let _ = fs::remove_dir_all(&outdir);

        let output = Command::new(VENV_PYTHON)
            .args(["sidecar/import_step.py", step, "-o"])
            .arg(&outdir)
            .args(*extra_args)
            .output()
            .expect("failed to run the sidecar");
        assert!(
            output.status.success(),
            "sidecar failed on {}: {}",
            step,
            String::from_utf8_lossy(&output.stderr)
        );

        let report = fs::read_to_string(outdir.join("report.json")).expect("sidecar wrote no report.json");
        let golden = fs::read_to_string(golden_report).expect("failed to read report golden");
        assert_eq!(report, golden, "{step}: report.json diverged from its golden");

        let proposed = fs::read_to_string(outdir.join("proposed.part")).expect("sidecar wrote no proposed.part");
        let golden = fs::read_to_string(golden_proposed).expect("failed to read proposed golden");
        assert_eq!(proposed, golden, "{step}: proposed.part diverged from its golden");

        let _ = fs::remove_dir_all(&outdir);
    }
}

#[test]
fn test_sidecar_report_total_accounting() {
    // The totality invariant (CONTRACT.md): every face is accounted for,
    // as `proposed` or `unsupported`, each with its CHECKLIST row. Checked
    // at the string level on the committed goldens — the Rust core is
    // deliberately JSON-free (the lean-core rule), and the golden's
    // determinism rules (sorted keys, indent=2) make these matches exact.
    for (step, _, golden_report, _) in FIXTURES {
        let report = fs::read_to_string(golden_report).expect("failed to read report golden");

        let faces_total: usize = report
            .lines()
            .find_map(|l| l.trim().strip_prefix("\"faces_total\": "))
            .expect("report has no faces_total")
            .trim_end_matches(',')
            .parse()
            .expect("faces_total is not a number");

        let count = |needle: &str| report.matches(needle).count();
        let proposed = count("\"status\": \"proposed\"");
        let unsupported = count("\"status\": \"unsupported\"");

        assert_eq!(count("\"id\": \"F"), faces_total, "{step}: one entry per face");
        assert_eq!(proposed + unsupported, faces_total, "{step}: every entry is proposed or unsupported");
        assert_eq!(count("\"checklist_row\":"), faces_total, "{step}: every entry names its CHECKLIST row");
        assert_eq!(count("\"part\":"), proposed, "{step}: every proposal carries a feature line");
        assert_eq!(count("\"reason\":"), unsupported, "{step}: every residue entry says why");
        assert_eq!(count("\"code\":"), unsupported, "{step}: every residue entry names its check");
    }

    // Fixture-specific pins on each part's deliberate residue entry — the
    // graduation rule's "rejected with a reason, never dropped" in evidence.
    let block_bore = fs::read_to_string(FIXTURES[0].2).unwrap();
    assert!(block_bore.contains("\"faces_total\": 10"), "block_bore fixture face count changed");
    // The slice-1 deliberate residue graduated in the outer-profile slice:
    // the boss now PROPOSES as row 2.4 (Feature::CylinderBoss), and the plate's
    // silhouette proposes as a CW outer-profile contour.
    assert!(
        block_bore.contains("\"checklist_row\": \"2.4\"") && block_bore.contains("a standing boss"),
        "block_bore's boss must propose as the row-2.4 Feature::CylinderBoss"
    );
    // The below-top plate top anchored that contour as a named residue until
    // Phase 28 ("a below-top plane has no Floor proposal of its own"); it
    // proposes as a bounded `Face` now — outer wire, then the boss's foot and
    // the bore's mouth as inner loops.
    assert!(
        block_bore.contains("a bounded face at z=15.0, 3 loops (outer + 2 inner); caps recognized outer profile F1_Profile"),
        "the below-top plate top must propose as a Face"
    );

    let quadric_boss = fs::read_to_string(FIXTURES[1].2).unwrap();
    assert!(quadric_boss.contains("\"faces_total\": 11"), "quadric_boss fixture face count changed");
    assert!(quadric_boss.contains("\"checklist_row\": \"3.1\""), "bowl must land as row-3.1 residue");

    let pocket_plate = fs::read_to_string(FIXTURES[2].2).unwrap();
    assert!(pocket_plate.contains("\"faces_total\": 35"), "pocket_plate fixture face count changed");
    // A grouped proposal is one feature owned by several faces: every
    // member (2 floors + 4 + 8 pocket walls, plus the 4 silhouette walls
    // from the outer-profile slice) carries the shared block in `part` and
    // names the group in the optional `feature` field.
    assert_eq!(
        pocket_plate.matches("\"feature\":").count(),
        18,
        "pocket_plate: both pocket groups' and the silhouette's member faces carry the feature field"
    );
    // The slice's two deliberate errors, in evidence: a tapered pocket
    // fails the wall-verticality check (since Phase 46 slice 3 under the
    // name that says both of what a floor-to-plane oblique face can be: a
    // draft wall, or a bevel as deep as its wall), and a lipped pocket
    // fails the mouth-rim undercut check (the case a single-point probe
    // would miss).
    assert!(
        pocket_plate.contains("a draft wall (CHECKLIST row 1.2, no tilted-plane primitive) or a bevel as deep as its wall"),
        "tapered pocket must be rejected for non-vertical walls"
    );
    assert!(
        pocket_plate.contains("capped by an overhanging lip"),
        "lipped pocket must be rejected as undercut at the mouth rim"
    );

    // Hole-vs-bore semantics (slice 4): the bottom face decides. One pin
    // per bottom kind, plus the deliberate fallback residue.
    let drilled_plate = fs::read_to_string(FIXTURES[3].2).unwrap();
    assert!(drilled_plate.contains("\"faces_total\": 13"), "drilled_plate fixture face count changed");
    assert!(
        drilled_plate.contains("a drillable through hole"),
        "the through hole must propose as a _Hole"
    );
    assert!(
        drilled_plate.contains("drill-point bottom") && drilled_plate.contains("a drilled hole"),
        "the drill-point hole must propose as a _Hole with its cone absorbed"
    );
    assert!(
        drilled_plate.contains("flat floor F12 -- a milled bore"),
        "the flat-bottom bore must propose as a _Bore with its floor absorbed"
    );
    assert!(
        drilled_plate.contains("proposed wall-only as a milled bore")
            && drilled_plate.contains("concave spherical face"),
        "the ball-end bottom must fall back to a wall-only _Bore with the sphere as row-3.1 residue"
    );
    // Grouped members carrying the optional feature field: the drill-point
    // hole (wall + cone), the flat bore (wall + floor), and the silhouette
    // (4 walls); the through hole and the wall-only bore are single-member,
    // so they omit it.
    assert_eq!(
        drilled_plate.matches("\"feature\":").count(),
        8,
        "drilled_plate: exactly the two 2-member groups and the silhouette carry the feature field"
    );

    // Import breadth (slice 5): the setup declarations are additive v1
    // fields, present exactly when the import needed one -- and absent from
    // every earlier golden (pinned by those goldens being byte-identical).
    let assembly_pair = fs::read_to_string(FIXTURES[4].2).unwrap();
    assert!(assembly_pair.contains("\"faces_total\": 8"), "assembly_pair recognizes S1 alone");
    assert!(
        assembly_pair.contains("\"solid\": \"S1\"") && assembly_pair.contains("\"solids_total\": 2"),
        "assembly_pair: the report must record which solid was declared the part"
    );

    let tilted_plate = fs::read_to_string(FIXTURES[5].2).unwrap();
    assert!(tilted_plate.contains("\"faces_total\": 9"), "tilted_plate fixture face count changed");
    assert!(
        tilted_plate.contains("\"up\": \"y+\""),
        "tilted_plate: the report must record the declared up axis"
    );
    // The remap recovers the Z-up twin's numbers exactly: the machined top
    // at z=15 proposes as the topmost +Z plane.
    assert!(
        tilted_plate.contains("feature F3_Top Plane(z=15.0)"),
        "tilted_plate: --up y+ must recover the Z-up top plane"
    );

    // Outer-profile recognition (slice 7): cap-anchored wall loops. One pin
    // per branch -- the CW silhouette from a below-top anchor cap, the
    // analytic Feature::CylinderBoss (never a chord list), and the two deliberate
    // errors plus the reworded not-under-a-flat-cap boss residue.
    let outer_profile = fs::read_to_string(FIXTURES[6].2).unwrap();
    assert!(outer_profile.contains("\"faces_total\": 23"), "outer_profile fixture face count changed");
    // (Phase 28: that anchor cap proposes as a bounded `Face` — the plate's top
    // with the four things standing on it as inner loops — and still says
    // which profile it caps.)
    assert!(
        outer_profile.contains("a bounded face at z=10.0, 5 loops (outer + 4 inner); caps recognized outer profile F6_Profile"),
        "the below-top plate top must propose as a Face and say what it caps"
    );
    assert!(
        outer_profile.contains("CylinderBoss(dia=16.0, extent=6.0"),
        "the circular boss must propose the analytic Feature::CylinderBoss"
    );
    assert!(
        outer_profile.contains("F16 is a 43.6deg-included oblique face spanning 5.0mm from the floor to the surrounding plane with nothing vertical under it"),
        "the tapered boss must be rejected for non-vertical walls (Phase 46 slice 3's dual-reading name)"
    );
    // ROADMAP item 1, slices 5 and 6: the mushroom's cap was refused at its
    // base rim (an overhang) until the real part's ribs, every one a head
    // on a narrower neck, showed what the program makes from above: the
    // head's outline carried down to the floor it stands over, the neck
    // and underside left standing under it.
    assert!(
        outer_profile.contains("feature F1_Boss CylinderBoss(dia=14.0, extent=6.0, x=42.0, y=36.0, z=10.0)"),
        "the mushroom cap proposes down to the plate top it stands over"
    );
    assert!(
        outer_profile.contains("so the boss is proposed down to z=10.0, the floor F5 it stands over"),
        "the cap's entry says which floor it was carried down to"
    );
    assert!(
        outer_profile.contains("\"code\": \"under_overhang\"") && outer_profile.contains("a wall of the neck, under the overhang of F1_Boss"),
        "the mushroom neck is residue under the head, naming it"
    );

    // 3D-wire trim recognition (the Contour slice 3): part-bottom-anchored
    // wall loops whose straight top edges form a non-planar wire. One pin
    // per branch -- the CW ContourBoss from the outer wire (its 6-point
    // wire crossing the gabled walls), the CCW ContourPocket from an inner
    // wire, the curved-trim-edge error on all five walls of the filleted
    // cutout, and the bottom face's rewritten anchor note.
    let trim_wire = fs::read_to_string(FIXTURES[7].2).unwrap();
    assert!(trim_wire.contains("\"faces_total\": 16"), "trim_wire fixture face count changed");
    assert!(
        trim_wire.contains("ContourBoss(extent=12.0\\n    0.0 0.0 9.0\\n    0.0 50.0 9.0\\n    30.0 50.0 12.0"),
        "the silhouette must propose a CW ContourBoss riding the gable ridge"
    );
    assert!(
        trim_wire.contains("ContourPocket(extent=11.0"),
        "the through cutout must propose a CCW ContourPocket"
    );
    assert_eq!(
        trim_wire.matches("curved trim edges have not graduated").count(),
        5,
        "the filleted cutout's error must land on all five walls"
    );
    assert!(
        trim_wire.contains("part-bottom anchor of recognized trim wire(s) F2_Trim F6_Trim"),
        "the bottom face must name the trim wires anchored on it"
    );
    // Grouped members carrying the optional feature field: 4 boss walls +
    // 4 pocket walls (the rejected candidate's walls are residue).
    assert_eq!(
        trim_wire.matches("\"feature\":").count(),
        8,
        "trim_wire: both trim groups' member faces carry the feature field"
    );

    // lip_bore: the mouth-round pass (DECISIONS.md -> "sidecar recognition of
    // a bore's mouth round"). The blend is absorbed as a declared edge_round
    // — never the standalone Torus ring its local surface imitates — and the
    // bore's extent runs to the virtual sharp corner, not the wall face's own
    // top (which measures 3.0 short under a filleted mouth).
    let lip_bore = fs::read_to_string(FIXTURES[8].2).unwrap();
    assert!(lip_bore.contains("\"faces_total\": 9"), "lip_bore fixture face count changed");
    assert!(
        lip_bore.contains("CylinderBore(dia=24.0, extent=10.0, x=30.0, y=30.0, edge_round=3.0)"),
        "the mouth blend must propose as edge_round on the bore, extent to the sharp corner"
    );
    assert!(
        !lip_bore.contains("Torus("),
        "the blend must never mis-propose as a standalone ring"
    );
    assert!(
        lip_bore.contains("mouth round of recognized bore F7_Bore"),
        "the blend face carries the group's why"
    );
    // The consume hint is proposed-part text (extras never enter the report).
    let lip_bore_part = fs::read_to_string(FIXTURES[8].3).unwrap();
    assert!(
        lip_bore_part.contains("must be a CornerRounder whose ground arc is R3.0"),
        "the consume hint names the tool the declared round pairs with"
    );
}

/// One face entry of a report golden, read at the string level: the
/// determinism rules (sorted keys, indent=2) make `    {` … `    }` the
/// entry boundary and `      "key": ` the field prefix, exactly.
struct FaceEntry {
    status: String,
    code: Option<String>,
    refs: Option<Vec<String>>,
}

fn face_entries(report: &str) -> Vec<FaceEntry> {
    let field = |lines: &[&str], key: &str| -> Option<String> {
        let prefix = format!("      \"{key}\": \"");
        lines.iter().find_map(|l| l.strip_prefix(&prefix)).map(|rest| {
            rest.trim_end_matches(',').trim_end_matches('"').to_string()
        })
    };
    let mut entries = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in report.lines() {
        match (line, current.as_mut()) {
            ("    {", None) => current = Some(Vec::new()),
            ("    }" | "    },", Some(lines)) => {
                // `refs` is the one list-valued per-face field: its items sit
                // one per line between `"refs": [` and `]`.
                let refs = lines.iter().position(|l| *l == "      \"refs\": [").map(|start| {
                    lines[start + 1..]
                        .iter()
                        .take_while(|l| !l.starts_with("      ]"))
                        .map(|l| l.trim().trim_end_matches(',').trim_matches('"').to_string())
                        .collect::<Vec<_>>()
                });
                entries.push(FaceEntry {
                    status: field(lines, "status").expect("entry has a status"),
                    code: field(lines, "code"),
                    refs,
                });
                current = None;
            }
            (_, Some(lines)) => lines.push(line),
            _ => {}
        }
    }
    entries
}

/// Every backticked token that opens a table row in a CONTRACT.md section —
/// the "Residue codes" tables list one code per row, first column.
fn contract_table_codes(section: &str) -> std::collections::BTreeSet<String> {
    section
        .lines()
        .filter_map(|l| l.strip_prefix("| `"))
        .map(|rest| rest.split('`').next().unwrap().to_string())
        .collect()
}

/// Every `<key>="…"` spelled in the sidecar source, for `key` = `code` or
/// `role` — the one home's own declarations.
fn sidecar_declared(source: &str, key: &str) -> std::collections::BTreeSet<String> {
    let needle = format!("{key}=\"");
    source
        .match_indices(&needle)
        .map(|(at, _)| {
            let rest = &source[at + needle.len()..];
            rest[..rest.find('"').unwrap()].to_string()
        })
        .collect()
}

/// The two imports of the far-mouth fixture agree on the hole's column
/// (Phase 48 slice 3, fork 2A): this way up the cone at the bottom is the
/// other setup's chamfer and the hole goes through; turned over it is the
/// mouth's `chamfer_depth=`. Pinned on the committed goldens, so it runs
/// without the venv, through the engine's own parser and the turn the
/// engine's `given from` applies: the turned-over hole, turned back, is the
/// same bore — and the blind control bore did not move.
#[test]
fn test_the_far_mouth_s_two_imports_agree_on_the_hole_s_column() {
    let up = fs::read_to_string("tests/fixtures/through_chamfer.proposed.golden.part").unwrap();
    let back = fs::read_to_string("tests/fixtures/through_chamfer_back.proposed.golden.part").unwrap();
    let hole = |src: &str| -> (f64, f64, f64, f64) {
        let line = src.lines().find(|l| l.starts_with("feature F7_Hole CylinderBore(")).expect("the through hole proposes both ways");
        let num = |key: &str| -> f64 {
            line.split(&format!("{key}="))
                .nth(1)
                .unwrap_or_else(|| panic!("{key} on {line}"))
                .trim_end_matches(')')
                .split(',')
                .next()
                .unwrap()
                .parse()
                .unwrap()
        };
        (num("dia"), num("extent"), num("x"), num("y"))
    };
    let (d1, e1, x1, y1) = hole(&up);
    let (d2, e2, x2, y2) = hole(&back);
    assert_eq!((d1, e1, x1, y1), (12.0, 12.0, 20.0, 20.0), "this way up: through, {up}");
    assert_eq!((d2, e2, x2, -y2), (12.0, 12.0, 20.0, 20.0), "turned over: the same column at y mirrored, {back}");
    assert!(back.contains("feature F7_Hole CylinderBore(dia=12.0, extent=12.0, x=20.0, y=-20.0, chamfer_depth=1.0, chamfer_angle=90.0)"), "{back}");
    // The comment under the hole NAMES chamfer_depth (as the other import's
    // key); the feature line carries none.
    assert!(
        !up.lines().any(|l| l.starts_with("feature") && l.contains("chamfer_depth=")),
        "this setup cannot cut the far mouth's bevel: no chamfer key, {up}"
    );
    assert!(up.contains("far mouth bevel F2 (1.0 deep at 90.0deg included) opens through -- the other setup's chamfer"), "{up}");
    assert!(up.contains("feature F9_Bore CylinderBore(dia=8.0, extent=7.0, x=45.0, y=20.0)"), "the blind control did not move, {up}");
}

#[test]
fn test_sidecar_residue_codes() {
    // The Phase 17 lane A slice-1 pin (CONTRACT.md "Residue codes"), on the
    // committed goldens so it runs without the venv: every residue entry
    // carries a non-empty `code`; each code is `check` or `role/check` with
    // the role and the check both enumerated in the contract; `refs`, when
    // present, is non-empty and names only faces of the same report or
    // feature names its proposals carry. And the enumeration is the
    // sidecar's own: the set of `code=`/`role=` strings spelled in
    // import_step.py equals the contract's tables, so a new check cannot
    // ship without a documented code (and a documented code cannot outlive
    // its check).
    let contract = fs::read_to_string("sidecar/CONTRACT.md").expect("failed to read CONTRACT.md");
    let section = contract
        .split("## Residue codes")
        .nth(1)
        .expect("CONTRACT.md has a Residue codes section")
        .split("\n## ")
        .next()
        .unwrap();
    let (roles_table, checks_table) = section.split_once("Checks (").expect("a checks table follows the roles table");
    let roles = contract_table_codes(roles_table);
    let checks = contract_table_codes(checks_table);
    assert!(!roles.is_empty() && !checks.is_empty(), "the contract enumerates roles and checks");
    for word in roles.iter().chain(&checks) {
        assert!(
            word.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "a code is snake_case words, never a number or a slash: {word}"
        );
    }

    let source = fs::read_to_string("sidecar/import_step.py").expect("failed to read the sidecar");
    assert_eq!(
        sidecar_declared(&source, "code"),
        checks,
        "the checks the sidecar spells (`code=\"…\"`) must be exactly the contract's checks table"
    );
    assert_eq!(
        sidecar_declared(&source, "role"),
        roles,
        "the roles the sidecar spells (`role=\"…\"`) must be exactly the contract's roles table"
    );

    let mut seen = std::collections::BTreeSet::new();
    for (step, _, golden_report, _) in FIXTURES {
        let report = fs::read_to_string(golden_report).expect("failed to read report golden");
        let entries = face_entries(&report);
        let faces_total: usize = report
            .lines()
            .find_map(|l| l.trim().strip_prefix("\"faces_total\": "))
            .unwrap()
            .trim_end_matches(',')
            .parse()
            .unwrap();
        assert_eq!(entries.len(), faces_total, "{step}: the entry walker sees every face");

        for e in &entries {
            match (e.status.as_str(), &e.code) {
                ("proposed", None) => {}
                ("proposed", Some(c)) => panic!("{step}: a proposal carries a code {c}"),
                ("unsupported", None) => panic!("{step}: a residue entry without a code"),
                ("unsupported", Some(code)) => {
                    assert!(!code.is_empty(), "{step}: empty code");
                    let mut parts = code.split('/');
                    let (first, second) = (parts.next().unwrap(), parts.next());
                    assert!(parts.next().is_none(), "{step}: roles never nest: {code}");
                    match second {
                        None => assert!(checks.contains(first), "{step}: undocumented check {first}"),
                        Some(check) => {
                            assert!(roles.contains(first), "{step}: undocumented role {first} in {code}");
                            assert!(checks.contains(check), "{step}: undocumented check {check} in {code}");
                        }
                    }
                    seen.insert(code.clone());
                }
                (s, _) => panic!("{step}: unknown status {s}"),
            }
            if let Some(refs) = &e.refs {
                assert_eq!(e.status, "unsupported", "{step}: refs live on residue entries only");
                assert!(!refs.is_empty(), "{step}: refs is never an empty list (absent instead)");
                for r in refs {
                    let is_face = report.contains(&format!("\"id\": \"{r}\""));
                    let is_feature = report.contains(&format!("\"feature\": \"{r}\""))
                        || report.contains(&format!("\"part\": \"feature {r} "));
                    assert!(is_face || is_feature, "{step}: ref {r} names nothing in its own report");
                }
            }
        }
    }
    // The goldens exercise the encoding's every shape: a bare check, a role
    // over a check, and both anchor notes (a residue entry that refused
    // nothing but names what proposed on it).
    assert!(seen.contains("plane_downward"), "a bare check is in evidence");
    assert!(
        seen.contains("outer_profile_candidate_wall/chamfer_consumes_wall")
            && seen.contains("outer_profile_candidate_cap/chamfer_consumes_wall")
            && seen.contains("trim_wire_candidate_wall/trim_wire_edge_not_line"),
        "every role is in evidence over its inner check"
    );
    // (`outer_profile_anchor_cap`, the other anchor note, retired in Phase 28:
    // every cap it named proposes as a `Face` now.)
    assert!(seen.contains("trim_wire_anchor_bottom"), "the anchor note is in evidence");
}

#[test]
fn test_sidecar_multi_solid_guard() {
    // A multi-solid STEP with no --solid must reject with a per-solid
    // inventory and write NO output files (never guess which solid is the
    // part; never emit half a result) -- and an out-of-range --solid must
    // reject too. Venv-gated like test_sidecar_golden.
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping multi-solid guard test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_sidecar_guard_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step.py", "tests/fixtures/assembly_pair.step", "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the sidecar");
    assert!(!output.status.success(), "a multi-solid STEP without --solid must reject");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("2 solids") && stderr.contains("--solid") && stderr.contains("S2 6 faces"),
        "the error must carry the per-solid inventory, got: {stderr}"
    );
    assert!(!outdir.exists(), "a rejected import must write no output files");

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step.py", "tests/fixtures/assembly_pair.step", "--solid", "3", "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the sidecar");
    assert!(!output.status.success(), "an out-of-range --solid must reject");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("out of range"),
        "the out-of-range error must say so"
    );
    assert!(!outdir.exists(), "a rejected import must write no output files");
}

// The DXF tool-profile importer (sidecar/import_dxf_profile.py): a DXF
// half-profile in, the SVG the Rust core already reads out. No report —
// it converts one drawing, it doesn't recognize faces.
const DXF_FIXTURE: &str = "tests/fixtures/form_tool.dxf";
const DXF_GOLDEN_SVG: &str = "tests/fixtures/form_tool.profile.golden.svg";

#[test]
fn test_dxf_profile_golden() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping DXF profile golden test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_dxf_golden_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_dxf_profile.py", DXF_FIXTURE, "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the DXF profile importer");
    assert!(
        output.status.success(),
        "DXF profile importer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let svg = fs::read_to_string(outdir.join("profile.svg")).expect("importer wrote no profile.svg");
    let golden = fs::read_to_string(DXF_GOLDEN_SVG).expect("failed to read SVG golden");
    assert_eq!(svg, golden, "profile.svg diverged from its golden");

    let _ = fs::remove_dir_all(&outdir);
}

#[test]
fn test_dxf_profile_guard() {
    // The honest-error gate: an unsupported entity type and a
    // disconnected chain each reject with the reason named and NO output
    // files — never an approximated spline, never a bridged gap.
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping DXF profile guard test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    for (dxf, needle) in [
        ("tests/fixtures/spline_profile.dxf", "unsupported DXF entity SPLINE"),
        ("tests/fixtures/split_profile.dxf", "disconnected"),
    ] {
        let outdir = std::env::temp_dir().join(format!("nacs_dxf_guard_{}", std::process::id()));
        let _ = fs::remove_dir_all(&outdir);

        let output = Command::new(VENV_PYTHON)
            .args(["sidecar/import_dxf_profile.py", dxf, "-o"])
            .arg(&outdir)
            .output()
            .expect("failed to run the DXF profile importer");
        assert!(!output.status.success(), "{dxf} must reject");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(needle), "{dxf}: error must name the reason, got: {stderr}");
        assert!(!outdir.exists(), "{dxf}: a rejected import must write no output files");
    }
}

#[test]
fn test_dxf_profile_round_trip() {
    // The slice's closure, off the committed golden so it runs without the
    // venv: DXF -> (sidecar) -> SVG -> unchanged Rust pipeline -> G-code.
    // The FormTool consumer is Sphere + stepover today (DECISIONS.md
    // "Custom tool-profile finishing…"), so that's the consuming run.
    let src = format!(
        "units mm\n\
         feed 600\n\
         plunge 200\n\
         rapid_z 5\n\
         clearance_z 30\n\
         spindle 4000\n\
         stock Box(w=100, l=80, h=25, x=0, y=0, z=0)\n\
         tool T1 FormTool(dia=10.0, profile=\"{DXF_GOLDEN_SVG}\")\n\
         feature TopDome Sphere(dia=20, x=30, y=20, z=0)\n\
         run Form(tool=T1, surface=TopDome, stepover=3.0)\n"
    );

    let part = parse::parse(&src).expect("the DXF-derived SVG must parse as a tool profile");
    let program = gcode::program(&part).expect("a DXF-profiled FormTool must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
}

#[test]
fn test_proposed_part_round_trip() {
    // The walking-skeleton closure: STEP -> sidecar -> proposed.part ->
    // unchanged Rust pipeline -> G-code. The proposal is a fragment for
    // review, so the test plays the human: wrap it with a preamble and
    // consuming runs, then parse and post it. block_bore's own bore sits on
    // the plate, 5 below the boss's top, and proposes with `z=15.0` since
    // Phase 45 slice 3; the bore's `Side` here reads sunken_bore's top bore,
    // whose rim is its stock top — moved there on 2026-10-09 when the sunken
    // bore refused, and left there because the wrap below cuts no step.
    let wrap = |proposed: &str, runs: &str| {
        format!(
            "units mm\n\
             feed 800\n\
             plunge 300\n\
             rapid_z 5\n\
             clearance_z 25\n\
             spindle 3000\n\
             stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
             tool T5 Endmill(dia=10.0)\n\
             {proposed}\n\
             {runs}"
        )
    };
    let block = fs::read_to_string(FIXTURES[0].3).expect("failed to read proposed golden");
    let sunken = fs::read_to_string("tests/fixtures/sunken_bore.proposed.golden.part").expect("failed to read proposed golden");
    for src in [
        wrap(&block, "run End(tool=T5, surface=F10_Top, stepover=6.0)\n"),
        wrap(&sunken, "run End(tool=T5, surface=F3_Top, stepover=6.0)\nrun Side(tool=T5, surface=F9_Bore, stepover=5.0)\n"),
    ] {
        let part = parse::parse(&src).expect("proposed feature lines must parse in the current grammar");
        let program = gcode::program(&part).expect("wrapped proposals must generate G-code");
        assert!(program.contains("M30"), "program should run to completion");
    }
}

#[test]
fn test_proposed_quadrics_round_trip() {
    // The same closure for the slice-2 quadric proposals: every graduated
    // camp-2 type (Sphere / Cone / Torus) has an ops::finish consumer, so
    // the wrapped proposals must post all the way to G-code with a Ballmill.
    let proposed = fs::read_to_string(FIXTURES[1].3).expect("failed to read proposed golden");
    let src = format!(
        "units mm\n\
         feed 600\n\
         plunge 200\n\
         rapid_z 5\n\
         clearance_z 35\n\
         spindle 4000\n\
         stock Box(w=100, l=80, h=27, x=0, y=0, z=0)\n\
         tool T5 Endmill(dia=10.0)\n\
         tool T9 Ballmill(dia=6.0)\n\
         {proposed}\n\
         run End(tool=T5, surface=F3_Top, stepover=6.0)\n\
         run Copy(tool=T9, surface=F7_Dome, stepover=3.0)\n\
         run Copy(tool=T9, surface=F8_Boss, stepover=3.0)\n\
         run Copy(tool=T9, surface=F10_Ring, stepover=1.5)\n"
    );

    let part = parse::parse(&src).expect("quadric proposals must parse in the current grammar");
    let program = gcode::program(&part).expect("wrapped quadric proposals must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
}

#[test]
fn test_proposed_pockets_round_trip() {
    // The slice-3 closure: grouped Contour proposals post to G-code. The
    // sharp pocket runs Pocket (offset clearing); the filleted pocket is
    // cleared and then wall-finished with comp=control -- the settled
    // "recognized contours propose comp=control" decision consumed end to
    // end (its chord-tessellated corners would hard-error under engine comp,
    // by design). The Clear before the Finish is the machinist's sequence
    // (DECISIONS.md "the plunge taxonomy"): without it the wall finish
    // plunge-cuts its own standing pocket -- a category-3 entry the flag
    // below pins.
    let proposed = fs::read_to_string(FIXTURES[2].3).expect("failed to read proposed golden");
    let wrap = |runs: &str| {
        format!(
            "units mm\n\
             feed 800\n\
             plunge 300\n\
             rapid_z 17\n\
             clearance_z 25\n\
             spindle 3000\n\
             stock Box(w=100, l=80, h=15, x=0, y=0, z=15)\n\
             tool T5 Endmill(dia=6.0)\n\
             {proposed}\n\
             run End(tool=T5, surface=F3_Top, stepover=4.0)\n\
             run Clear(tool=T5, profile=F27_Pocket, stepover=3.0)\n\
             {runs}"
        )
    };
    let src = wrap(
        "run Clear(tool=T5, profile=F28_Pocket, stepover=3.0)\n\
         run Side(tool=T5, surface=F28_Pocket, comp=control)\n",
    );

    let part = parse::parse(&src).expect("pocket proposals must parse in the current grammar");
    let (program, emitted_moves) = gcode::program_with_moves(&part).expect("wrapped pocket proposals must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(program.contains("G41") || program.contains("G42"),
            "the comp=control profile Finish run must emit control-side comp");
    assert!(nacs::stock::entry_findings(&part, &emitted_moves).is_empty(),
            "the cleared-then-finished sequence is licensed end to end");

    // The flag doing its job on our own test: drop the Clear and the wall
    // finish's entry is category 3 -- flagged, while the program still
    // generates (advisory, not error).
    let unclear = wrap("run Side(tool=T5, surface=F28_Pocket, comp=control)\n");
    let part = parse::parse(&unclear).expect("the uncleared wrapper parses");
    let (_, emitted_moves) = gcode::program_with_moves(&part).expect("a manual plunge entry still generates");
    let findings = nacs::stock::entry_findings(&part, &emitted_moves);
    assert!(findings.iter().any(|f| f.message.contains("F28_Pocket")),
            "the uncleared wall finish is flagged, got: {:?}", findings);
}

/// Phase 26 slice 4's acceptance: a STEP part with a bossed pocket imports as
/// a pocket AND the bosses standing in it, with no new syntax, and the engine
/// — reading the PROPOSAL, never the STEP — derives both as the pocket's
/// islands and clears around them. Reads the committed golden, so it runs
/// without the venv; `test_sidecar_golden` is what ties the golden to the
/// sidecar.
#[test]
fn test_a_proposed_bossed_pocket_is_cleared_around_its_bosses() {
    let proposed = fs::read_to_string("tests/fixtures/pocket_post.proposed.golden.part").expect("failed to read proposed golden");
    let report = fs::read_to_string("tests/fixtures/pocket_post.report.golden.json").unwrap();
    assert!(report.contains("\"faces_total\": 18"), "pocket_post fixture face count changed");
    assert!(!report.contains("pocket_islands"), "the refusal is retired");
    assert_eq!(report.matches("\"status\": \"unsupported\"").count(), 1, "only the plate's underside is residue");
    assert!(proposed.contains("2 inner wires"), "the floor says what stands on it");

    let src = format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 25\nclearance_z 45\nspindle 3000\n\
         stock Box(w=100, l=100, h=20, x=0, y=0, z=20)\n\
         tool T5 Endmill(dia=8.0)\n\
         {proposed}\n\
         run Clear(tool=T5, profile=F11_Pocket, stepover=4.0)\n\
         run Side(tool=T5, surface=F12_Boss, stepover=5.0)\n\
         run Side(tool=T5, surface=F13_Profile)\n"
    );
    let part = parse::parse(&src).expect("the bossed-pocket proposal must parse in the current grammar");
    let islands: Vec<&str> = nacs::stock::islands_of(&part, "F11_Pocket").into_iter().map(|(n, _)| n).collect();
    assert_eq!(islands, ["F12_Boss", "F13_Profile"], "derived from containment — the proposal never says 'island'");
    let (program, emitted_moves) = gcode::program_with_moves(&part).expect("the bossed pocket generates");
    assert!(program.contains("(islands: F12_Boss, F13_Profile)"), "the Clear says what it goes around");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(nacs::stock::entry_findings(&part, &emitted_moves).is_empty(), "every island finish enters through the pocket's own annulus");
}

/// `CylinderBore` has no `z`: its top is the stock top. A bore whose rim is
/// on a step below it was proposed anyway, and would have been machined as
/// far too high as the step is deep — no error, the program generated. For
/// one day (2026-10-09) it was residue naming both heights; since Phase 45
/// slice 3 it is PLACED: `z=` on the line is the rim, the top bore omits the
/// key, the wall's reason names the rim and the step, and the residue code
/// is gone from the contract. Reads the committed golden; `test_sidecar_golden`
/// ties the golden to the sidecar.
#[test]
fn test_a_bore_below_the_stock_top_is_placed_by_z() {
    let proposed = fs::read_to_string("tests/fixtures/sunken_bore.proposed.golden.part").expect("failed to read proposed golden");
    let report = fs::read_to_string("tests/fixtures/sunken_bore.report.golden.json").unwrap();
    assert!(proposed.contains("feature F9_Bore CylinderBore(dia=16.0, extent=8.0, x=25.0, y=40.0)
"), "the top bore omits z: {proposed}");
    assert!(proposed.contains("feature F10_Bore CylinderBore(dia=16.0, extent=6.0, x=75.0, y=40.0, z=10.0)
"), "the step's bore is placed at its rim: {proposed}");
    assert_eq!(proposed.matches("CylinderBore(").count(), 2, "{proposed}");
    assert!(!report.contains("cylinder_bore_sunken"), "the residue code is retired: {report}");
    assert!(report.contains("rim at z=10.0, 10.0 below the stock top z=20.0 -- authored from its rim by z="), "{report}");
    // The placed proposal parses under the grammar that carries the key, and
    // the engine reads the rim: the step's bore removes z 4..10, not 14..20.
    let wrapped = format!(
        "units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=10.0)
{proposed}"
    );
    let part = parse::parse(&wrapped).expect("the placed proposal parses");
    let text = outline::render(&part, &outline::outline(&part));
    assert!(text.contains("F10_Bore  CylinderBore"), "{text}");
    match &part.features["F10_Bore"] {
        parse::Feature::CylinderBore { z: Some(z), extent, .. } => {
            assert_eq!(*z, 10.0);
            assert_eq!(parse::derive_z_cut(*z, *extent), 4.0, "the floor is rim − extent");
        }
        other => panic!("{other:?}"),
    }
}

/// Phase 28 slice 4: a face lifted straight from how STEP states it. The
/// ledge of `ledge_face.step` was residue for one stated reason — "a below-top
/// plane has no Floor proposal of its own" — and proposes now as a `Face`: its
/// outer wire, then its inner wire, which is an ELLIPSE (neither a line nor an
/// arc: chorded, because a face's boundary is overhung or stood off from,
/// never the surface itself). The engine faces it around the pad that stands
/// on that inner loop. Reads the committed golden; `test_sidecar_golden` ties
/// the golden to the sidecar.
#[test]
fn test_a_proposed_ledge_is_a_face_and_is_faced_around_what_stands_on_it() {
    let proposed = fs::read_to_string("tests/fixtures/ledge_face.proposed.golden.part").expect("failed to read proposed golden");
    let report = fs::read_to_string("tests/fixtures/ledge_face.report.golden.json").unwrap();
    assert!(report.contains("\"faces_total\": 8") && !report.contains("outer_profile_anchor_cap"), "the ledge is a proposal now");
    assert!(proposed.contains("feature F3_Face Face(z=10.0\n    loop\n"), "{}", &proposed[..600.min(proposed.len())]);
    assert_eq!(proposed.matches("\n    loop\n").count(), 2, "the outer wire and the pad's foot");

    // The pad itself is an extruded ellipse the sidecar cannot name yet
    // (`surface_not_graduated`), so the reviewer declares it — which is what
    // review is for — and the engine does the rest.
    let src = format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 23\nclearance_z 43\nspindle 3000\n\
         stock Box(w=100, l=80, h=18, x=0, y=0, z=18)\n\
         tool T5 Endmill(dia=10.0)\n\
         {proposed}\n\
         feature Pad CylinderBoss(dia=50, extent=8, x=50, y=40, z=10)\n\
         run End(tool=T5, surface=F3_Face, stepover=5.0)\n"
    );
    let part = parse::parse(&src).expect("the Face proposal must parse in the current grammar");
    let region = nacs::region::of(&part, &part.operations[0]).unwrap();
    assert_eq!(region.feature_islands(), ["Pad"]);
    assert!(region.bridged().is_empty() && region.open_voids().is_empty(), "the inner loop is the pad's foot, not a void");
    let program = gcode::program(&part).expect("the ledge is faced around its pad");
    assert!(program.contains("(--- floor: F3_Face with T5 ---)\n(islands: Pad)\n") && program.contains("M30"));
}

#[test]
fn test_proposed_outer_profiles_round_trip() {
    // The slice-7 closure: cap-anchored proposals consumed by the ops they
    // suggest -- the circular boss by Finish (finish_boss's R + r_tool
    // convex flank), the plateau loop and the part silhouette by Contour
    // with comp=control on their CW winding (G41 + CW travel = tool
    // outside, the blessed winding convention consumed end to end).
    //
    // The boss finish runs behind a moat `Clear` (Phase 10 lane A): a boss
    // finish leads in one tool diameter outside its circle, "in cleared air",
    // and since the finish's band joined the stock model an unroughed boss
    // rejects by name at that plunge — `(34.000, 14.000, 14.000)` on this
    // wrapper. The proposal is untouched; the run lines are the wrapper's own
    // landing of it, and the landing now says what it always assumed.
    let proposed = fs::read_to_string(FIXTURES[6].3).expect("failed to read proposed golden");
    let src = format!(
        "units mm\n\
         feed 800\n\
         plunge 300\n\
         rapid_z 18\n\
         clearance_z 25\n\
         spindle 3000\n\
         stock Box(w=60, l=50, h=16, x=0, y=0, z=16)\n\
         tool T5 Endmill(dia=6.0)\n\
         {proposed}\n\
         run End(tool=T5, surface=F21_Top, stepover=4.0)\n\
         run Clear(tool=T5, profile=F10_Boss, stepover=2.0, stepdown=4.0)\n\
         run Side(tool=T5, surface=F10_Boss, stepover=2.0)\n\
         run Side(tool=T5, surface=F11_Profile, comp=control)\n"
    );

    let part = parse::parse(&src).expect("outer-profile proposals must parse in the current grammar");
    let program = gcode::program(&part).expect("wrapped outer-profile proposals must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(program.contains("(--- side boss wall"), "the _Boss proposal must take the boss flank path");
    assert!(program.contains("G41"), "the comp=control contours must emit control-side comp");

    // Phase 60 slice 2: `F6_Profile` is the part's OUTER profile, and this
    // wrapper's stock is the part's own footprint, so every edge of it lies
    // on the blank — there is no wall to trace. Until this phase its Side
    // traced the blank's outline a radius outside the stock, cutting air;
    // now it is refused by name (fork 4A), and the sentence is the one the
    // containment lint has always ended with: declare the stock the part is
    // actually made from.
    let tight = format!("{src}run Side(tool=T5, surface=F6_Profile, comp=control)\n");
    let part = parse::parse(&tight).expect("parses");
    let err = gcode::program(&part).expect_err("an outer profile on a blank its own size has no wall to trace");
    assert!(err.contains("Side on 'F6_Profile' with T5") && err.contains("there is no wall to trace"), "{err}");
}

#[test]
fn test_proposed_trim_wires_round_trip() {
    // The trim-wire closure: part-bottom-anchored 3D-wire proposals
    // consumed by the op they suggest, one comp mode per variant so both
    // Rust paths run -- the ContourBoss under engine comp (projection
    // offset + Z re-attachment, the ridge vertex plan-collinear), the
    // ContourPocket under control comp (authored edge with Z words, G41).
    // The stock top sits at the wire's own maximum (the crown ridge), and
    // both extents reach the part bottom per the proposal's through-trim
    // comment.
    let proposed = fs::read_to_string(FIXTURES[7].3).expect("failed to read proposed golden");
    let src = format!(
        "units mm\n\
         feed 800\n\
         plunge 300\n\
         rapid_z 15\n\
         clearance_z 25\n\
         spindle 3000\n\
         stock Box(w=60, l=50, h=12, x=0, y=0, z=0)\n\
         tool T5 Endmill(dia=6.0)\n\
         {proposed}\n\
         run Side(tool=T5, surface=F2_Trim)\n\
         run Side(tool=T5, surface=F6_Trim, comp=control)\n"
    );

    let part = parse::parse(&src).expect("trim-wire proposals must parse in the current grammar");
    let program = gcode::program(&part).expect("wrapped trim-wire proposals must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(
        program.contains("(--- side contour wire (engine): F2_Trim"),
        "the ContourBoss proposal must take the wire flank path under engine comp"
    );
    assert!(
        program.contains("(--- side contour wire: F6_Trim"),
        "the ContourPocket proposal must take the wire flank path under control comp"
    );
    assert!(program.contains("G41"), "the comp=control pocket rim must emit control-side comp");
}

#[test]
fn test_proposed_holes_round_trip() {
    // The slice-4 closure: hole-vs-bore proposals consumed by the op each
    // one suggests -- the two _Hole proposals by Drill (the drill-point
    // hole's depth reaches the tip, so with the stock top at the plate top
    // the tip lands exactly at the cone's apex), the recognized _Bore and
    // the wall-only fallback _Bore by Finish (exact-flank wall finishing).
    let proposed = fs::read_to_string(FIXTURES[3].3).expect("failed to read proposed golden");
    let src = format!(
        "units mm\n\
         feed 800\n\
         plunge 300\n\
         rapid_z 17\n\
         clearance_z 25\n\
         spindle 3000\n\
         stock Box(w=100, l=80, h=15, x=0, y=0, z=15)\n\
         tool T5 Endmill(dia=6.0)\n\
         tool T7 Drill(dia=12.0)\n\
         tool T8 Drill(dia=10.0)\n\
         {proposed}\n\
         run End(tool=T5, surface=F3_Top, stepover=4.0)\n\
         run Form(points=[F7_Hole], tool=T7, cycle=Simple)\n\
         run Form(points=[F8_Hole], tool=T8, cycle=Peck, peck_increment=3.0)\n\
         run Side(tool=T5, surface=F9_Bore, stepover=2.0)\n\
         run Side(tool=T5, surface=F10_Bore, stepover=2.0)\n"
    );

    let part = parse::parse(&src).expect("hole/bore proposals must parse in the current grammar");
    let program = gcode::program(&part).expect("wrapped hole/bore proposals must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(program.contains("(--- form points:"), "the _Hole proposals must drill");
    // The drill-point hole: depth=11.0 from the stock top (z=15) puts the
    // tip at z=4.0 -- the fixture's cone apex, exactly.
    assert!(program.contains("G01 Z4."), "the drill tip must reach the cone apex");
}

// The STEP->mesh export tool (sidecar/import_step_mesh.py): a STEP's
// freeform faces in, the STL the Rust core already reads out (plus a
// reviewable Feature::Mesh proposal). Its own outputs (mesh.stl +
// proposed.part), not the recognition report — this tool tessellates the
// tail no equation captures, it doesn't classify faces.
const MESH_STEP: &str = "tests/fixtures/freeform_cap.step";
const MESH_GOLDEN_STL: &str = "tests/fixtures/freeform_cap.mesh.golden.stl";
const MESH_GOLDEN_PART: &str = "tests/fixtures/freeform_cap.proposed.golden.part";

#[test]
fn test_step_mesh_golden() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping STEP->mesh golden test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_step_mesh_golden_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step_mesh.py", MESH_STEP, "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the STEP->mesh exporter");
    assert!(
        output.status.success(),
        "STEP->mesh exporter failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stl = fs::read_to_string(outdir.join("mesh.stl")).expect("exporter wrote no mesh.stl");
    let golden = fs::read_to_string(MESH_GOLDEN_STL).expect("failed to read STL golden");
    assert_eq!(stl, golden, "mesh.stl diverged from its golden");

    let proposed = fs::read_to_string(outdir.join("proposed.part")).expect("exporter wrote no proposed.part");
    let golden = fs::read_to_string(MESH_GOLDEN_PART).expect("failed to read proposed golden");
    assert_eq!(proposed, golden, "proposed.part diverged from its golden");

    let _ = fs::remove_dir_all(&outdir);
}

#[test]
fn test_step_mesh_refuses_all_analytic() {
    // The honest-error gate: a part with no freeform face has nothing to
    // tessellate, so the exporter rejects (naming the count) and writes NO
    // output files — an empty STL is never emitted. block_bore is all
    // planes/cylinders. Venv-gated like the golden test.
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping STEP->mesh error test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_step_mesh_refuse_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step_mesh.py", "tests/fixtures/block_bore.step", "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the STEP->mesh exporter");
    assert!(!output.status.success(), "an all-analytic part must reject");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no freeform faces") && stderr.contains("import_step.py"),
        "the error must name why and point at recognition, got: {stderr}"
    );
    assert!(!outdir.exists(), "a rejected export must write no output files");
}

#[test]
fn test_freeform_mesh_round_trip() {
    // The slice's closure, off the committed golden so it runs without the
    // venv: STEP -> (sidecar) -> STL -> unchanged Rust pipeline -> G-code.
    // The proposed.part's file="mesh.stl" resolves against the project dir,
    // so here we play the human and point Feature::Mesh straight at the
    // committed golden STL (the DXF round trip does the same with its SVG).
    let src = format!(
        "units mm\n\
         feed 600\n\
         plunge 200\n\
         rapid_z 5\n\
         clearance_z 30\n\
         spindle 4000\n\
         stock Box(w=60, l=60, h=24, x=0, y=0, z=0)\n\
         tool T9 Ballmill(dia=6.0)\n\
         feature Freeform_cap_Mesh Mesh(file=\"{MESH_GOLDEN_STL}\", x=30, y=30, z=0)\n\
         run Copy(tool=T9, surface=Freeform_cap_Mesh, stepover=2.0)\n"
    );

    let part = parse::parse(&src).expect("the exported STL must read as a Feature::Mesh");
    let program = gcode::program(&part).expect("a mesh-finished freeform cap must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(
        program.contains("mesh raster drop-cutter"),
        "the Mesh surface must take the raster drop-cutter finishing path"
    );
}

#[test]
fn test_proposed_breadth_round_trip() {
    // The slice-5 closure: proposals from a declared setup post to G-code
    // like any other. The --solid 1 selection recognizes the assembly's
    // plate as if it were alone in the file; the --up y+ remap yields
    // machine-orientation coordinates a stock declaration can sit on
    // directly (the tilted plate's top is z=15 after rotation, so the same
    // wrap as its Z-up twin drilled_plate works verbatim).
    let proposed = fs::read_to_string(FIXTURES[4].3).expect("failed to read proposed golden");
    let src = format!(
        "units mm\n\
         feed 800\n\
         plunge 300\n\
         rapid_z 15\n\
         clearance_z 25\n\
         spindle 3000\n\
         stock Box(w=60, l=40, h=10, x=0, y=0, z=10)\n\
         tool T5 Endmill(dia=6.0)\n\
         {proposed}\n\
         run End(tool=T5, surface=F3_Top, stepover=4.0)\n\
         run Side(tool=T5, surface=F7_Bore, stepover=2.0)\n"
    );
    let part = parse::parse(&src).expect("assembly proposals must parse in the current grammar");
    let program = gcode::program(&part).expect("wrapped assembly proposals must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");

    let proposed = fs::read_to_string(FIXTURES[5].3).expect("failed to read proposed golden");
    let src = format!(
        "units mm\n\
         feed 800\n\
         plunge 300\n\
         rapid_z 17\n\
         clearance_z 25\n\
         spindle 3000\n\
         stock Box(w=100, l=80, h=15, x=0, y=0, z=15)\n\
         tool T5 Endmill(dia=6.0)\n\
         tool T7 Drill(dia=12.0)\n\
         {proposed}\n\
         run End(tool=T5, surface=F3_Top, stepover=4.0)\n\
         run Form(points=[F7_Hole], tool=T7, cycle=Simple)\n\
         run Side(tool=T5, surface=F8_Bore, stepover=2.0)\n"
    );
    let part = parse::parse(&src).expect("tilted-part proposals must parse in the current grammar");
    let program = gcode::program(&part).expect("wrapped tilted-part proposals must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(program.contains("(--- form points:"), "the remapped _Hole proposal must drill");
}

// The analytic NURBS reader (sidecar/import_step_nurbs.py): read a freeform
// revolved face's b-spline MERIDIAN exactly into a `.spline` + a SplineRevolve
// proposal — the analytic counterpart to import_step_mesh.py. Its own outputs
// (meridian.spline + proposed.part), not a recognition report — it reads one
// kind of face exactly, it doesn't classify faces.
const NURBS_STEP: &str = "tests/fixtures/spline_cap.step";
const NURBS_GOLDEN_SPLINE: &str = "tests/fixtures/spline_cap.meridian.golden.spline";
const NURBS_GOLDEN_PART: &str = "tests/fixtures/spline_cap.proposed.golden.part";

#[test]
fn test_step_nurbs_golden() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping STEP->nurbs golden test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_step_nurbs_golden_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step_nurbs.py", NURBS_STEP, "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the STEP->nurbs reader");
    assert!(
        output.status.success(),
        "STEP->nurbs reader failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let spline = fs::read_to_string(outdir.join("meridian.spline")).expect("reader wrote no meridian.spline");
    let golden = fs::read_to_string(NURBS_GOLDEN_SPLINE).expect("failed to read spline golden");
    assert_eq!(spline, golden, "meridian.spline diverged from its golden");

    let proposed = fs::read_to_string(outdir.join("proposed.part")).expect("reader wrote no proposed.part");
    let golden = fs::read_to_string(NURBS_GOLDEN_PART).expect("failed to read proposed golden");
    assert_eq!(proposed, golden, "proposed.part diverged from its golden");

    let _ = fs::remove_dir_all(&outdir);
}

#[test]
fn test_step_nurbs_refuses_no_spline_of_revolution() {
    // The honest-error gate: a part with no analytic spline-of-revolution
    // face (block_bore is all planes/cylinders) has nothing to read, so the
    // reader rejects (naming why and pointing at the other two tools) and
    // writes NO output. Venv-gated like the golden test.
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping STEP->nurbs error test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_step_nurbs_refuse_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step_nurbs.py", "tests/fixtures/block_bore.step", "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the STEP->nurbs reader");
    assert!(!output.status.success(), "a part with no spline-of-revolution face must reject");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no analytic spline-of-revolution")
            && stderr.contains("import_step_mesh.py"),
        "the error must name why and point at the other tools, got: {stderr}"
    );
    assert!(!outdir.exists(), "a rejected read must write no output files");
}

// The cavity branch (DECISIONS.md "Spline funnel…"): the same reader tells a
// cavity from a cap by the face's oriented outward normal (radial component
// negative ⇒ into the void) and emits the meridian MOUTH-first as a
// SplineRevolveFunnel proposal with no z field.
const NURBS_FUNNEL_STEP: &str = "tests/fixtures/spline_funnel.step";
const NURBS_FUNNEL_GOLDEN_SPLINE: &str = "tests/fixtures/spline_funnel.meridian.golden.spline";
const NURBS_FUNNEL_GOLDEN_PART: &str = "tests/fixtures/spline_funnel.proposed.golden.part";

#[test]
fn test_step_nurbs_funnel_golden() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping STEP->nurbs funnel golden test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_step_nurbs_funnel_golden_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step_nurbs.py", NURBS_FUNNEL_STEP, "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the STEP->nurbs reader");
    assert!(
        output.status.success(),
        "STEP->nurbs reader failed on the cavity fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let spline = fs::read_to_string(outdir.join("meridian.spline")).expect("reader wrote no meridian.spline");
    let golden = fs::read_to_string(NURBS_FUNNEL_GOLDEN_SPLINE).expect("failed to read funnel spline golden");
    assert_eq!(spline, golden, "funnel meridian.spline diverged from its golden");
    assert!(spline.contains("mouth-first"), "the cavity emission must say its order");

    let proposed = fs::read_to_string(outdir.join("proposed.part")).expect("reader wrote no proposed.part");
    let golden = fs::read_to_string(NURBS_FUNNEL_GOLDEN_PART).expect("failed to read funnel proposed golden");
    assert_eq!(proposed, golden, "funnel proposed.part diverged from its golden");
    assert!(
        proposed.contains("SplineRevolveFunnel(file=\"meridian.spline\", x=0.0, y=0.0)"),
        "the cavity proposal must be a SplineRevolveFunnel with no z field"
    );
    assert!(
        proposed.contains("mouth circle at model z=16.0"),
        "the dropped z must survive as the mouth-plane comment"
    );

    let _ = fs::remove_dir_all(&outdir);
}

#[test]
fn test_spline_revolve_funnel_round_trip() {
    // The cavity closure, off the committed goldens so it runs without the
    // venv: STEP -> (sidecar) -> mouth-first .spline -> unchanged Rust
    // pipeline -> G-code, and crucially it MACHINES — the emitted meridian is
    // a machinable funnel that finish_spline_revolve_funnel accepts. We play
    // the human, as the cap round trip does: the proposal has no z, so the
    // stock top IS the mouth plane (h=16 puts the top at the fixture's model
    // mouth height, per the proposal's comment).
    let src = format!(
        "units mm\n\
         feed 600\n\
         plunge 200\n\
         rapid_z 21\n\
         clearance_z 40\n\
         spindle 4000\n\
         stock Box(w=60, l=60, h=16, x=0, y=0, z=16)\n\
         tool T9 Ballmill(dia=6.0)\n\
         feature Spline_funnel_Funnel SplineRevolveFunnel(file=\"{NURBS_FUNNEL_GOLDEN_SPLINE}\", x=30, y=30)\n\
         run Copy(tool=T9, surface=Spline_funnel_Funnel, stepover=3.0)\n"
    );

    let part = parse::parse(&src).expect("the exported .spline must read as a Feature::SplineRevolveFunnel");
    let program = gcode::program(&part).expect("a spline-revolved funnel must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(
        program.contains("analytic funnel of revolution"),
        "the SplineRevolveFunnel surface must take the analytic funnel finishing path"
    );
}

#[test]
fn test_spline_revolve_round_trip() {
    // The slice's closure, off the committed goldens so it runs without the
    // venv: STEP -> (sidecar) -> .spline -> unchanged Rust pipeline -> G-code,
    // and crucially it MACHINES — the reader's meridian is a machinable cap
    // that finish_spline_revolve accepts, not just a parseable one. We play the
    // human: point the SplineRevolve at the committed golden .spline (the mesh
    // and DXF round trips do the same with their golden artifacts) and post the
    // consuming Finish. z=6 is the cap's base plane (the plate top).
    let src = format!(
        "units mm\n\
         feed 600\n\
         plunge 200\n\
         rapid_z 5\n\
         clearance_z 40\n\
         spindle 4000\n\
         stock Box(w=60, l=60, h=30, x=0, y=0, z=0)\n\
         tool T9 Ballmill(dia=6.0)\n\
         feature Spline_cap_Cap SplineRevolve(file=\"{NURBS_GOLDEN_SPLINE}\", x=30, y=30, z=6)\n\
         run Copy(tool=T9, surface=Spline_cap_Cap, stepover=3.0)\n"
    );

    let part = parse::parse(&src).expect("the exported .spline must read as a Feature::SplineRevolve");
    let program = gcode::program(&part).expect("a spline-revolved cap must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(
        program.contains("pointwise spline offset"),
        "the SplineRevolve surface must take the analytic pointwise-offset finishing path"
    );
}

// The general NURBS patch (DECISIONS.md "the general NURBS patch…", slice 1):
// the same reader learns the untrimmed B_SPLINE_SURFACE face — the control
// net read exactly into a `.patch` + a NurbsPatch proposal. The fixture solid
// is the nurbs_patch.cam Bézier net extruded downward, so the top face is the
// read patch, the bottom face pins the downward-face skip, and the emitted
// net is verifiable against numbers already pinned end to end in the Rust
// fixture.
const PATCH_STEP: &str = "tests/fixtures/patch_cap.step";
const PATCH_GOLDEN_SURFACE: &str = "tests/fixtures/patch_cap.surface.golden.patch";
const PATCH_GOLDEN_PART: &str = "tests/fixtures/patch_cap.proposed.golden.part";

#[test]
fn test_step_patch_golden() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping STEP->patch golden test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_step_patch_golden_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step_nurbs.py", PATCH_STEP, "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the STEP->nurbs reader");
    assert!(
        output.status.success(),
        "STEP->nurbs reader failed on the patch fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let patch = fs::read_to_string(outdir.join("surface.patch")).expect("reader wrote no surface.patch");
    let golden = fs::read_to_string(PATCH_GOLDEN_SURFACE).expect("failed to read patch golden");
    assert_eq!(patch, golden, "surface.patch diverged from its golden");
    // The downward bottom face (the same untrimmed patch) must be skipped:
    // exactly one patch read, no surface_2.patch.
    assert!(!outdir.join("surface_2.patch").exists(), "the downward bottom face must be skipped");

    let proposed = fs::read_to_string(outdir.join("proposed.part")).expect("reader wrote no proposed.part");
    let golden = fs::read_to_string(PATCH_GOLDEN_PART).expect("failed to read proposed golden");
    assert_eq!(proposed, golden, "proposed.part diverged from its golden");
    assert!(
        proposed.contains("NurbsPatch(file=\"surface.patch\")"),
        "the proposal must be a NurbsPatch riding the emitted .patch"
    );

    let _ = fs::remove_dir_all(&outdir);
}

#[test]
fn test_step_patch_trimmed_refuses() {
    // The trimmed-face error (the slice's committed scope cut): a face over
    // a sub-domain of its surface must never be read as the full rectangle —
    // that would invent surface the part does not have. The fixture's only
    // candidate faces are trimmed, so the whole part rejects, counts them,
    // and points at the mesh path. Venv-gated like the golden test.
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping STEP->patch trimmed error test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_step_patch_trimmed_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);

    let output = Command::new(VENV_PYTHON)
        .args(["sidecar/import_step_nurbs.py", "tests/fixtures/patch_trimmed.step", "-o"])
        .arg(&outdir)
        .output()
        .expect("failed to run the STEP->nurbs reader");
    assert!(!output.status.success(), "a part with only trimmed patches must reject");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("TRIMMED") && stderr.contains("import_step_mesh.py"),
        "the error must name the trimmed faces and point at the mesh path, got: {stderr}"
    );
    assert!(!outdir.exists(), "a rejected read must write no output files");
}

#[test]
fn test_nurbs_patch_round_trip() {
    // The patch closure, off the committed goldens so it runs without the
    // venv: STEP -> (sidecar) -> .patch -> unchanged Rust pipeline -> G-code,
    // and crucially it MACHINES — the reader's control net parses as a
    // Feature::NurbsPatch and the raster drop-cutter takes it. We play the
    // human: poles are model mm coords, so a placement offset positions the
    // sheet in stock coords exactly as nurbs_patch.cam does.
    let src = format!(
        "units mm\n\
         feed 600\n\
         plunge 200\n\
         rapid_z 5\n\
         clearance_z 30\n\
         spindle 4000\n\
         stock Box(w=60, l=60, h=15, x=0, y=0, z=0)\n\
         tool T9 Ballmill(dia=8.0)\n\
         feature Patch_cap_Patch NurbsPatch(file=\"{PATCH_GOLDEN_SURFACE}\", x=18, y=18, z=0)\n\
         run Copy(tool=T9, surface=Patch_cap_Patch, stepover=2.0)\n"
    );

    let part = parse::parse(&src).expect("the exported .patch must read as a Feature::NurbsPatch");
    let program = gcode::program(&part).expect("a nurbs patch must generate G-code");
    assert!(program.contains("M30"), "program should run to completion");
    assert!(
        program.contains("nurbs patch raster drop-cutter"),
        "the NurbsPatch surface must take the analytic raster drop-cutter path"
    );
    // The reader's net is the Rust fixture's net, so the fixture's hand
    // anchor holds here too: the apex sample rides the exact apex contact.
    assert!(
        program.contains("G01 X30.000 Y30.000 Z13.000"),
        "the apex sample must land at 9 + r_ball = 13.000 exactly"
    );
}

// ---- the optimizer's STEP slice: a RECOGNIZED part feeds the recommender
// (DECISIONS.md → "the optimizer's STEP slice…", forks 1A/3A/5A) ----

/// The acceptance, on a committed golden: the same top bore the
/// walking-skeleton round trip's `Side` reads (`sunken_bore`'s `F9_Bore`;
/// `block_bore`'s own bore sits on the plate and proposes with `z=15.0`),
/// wrapped the same way (the test plays the human, because a `proposed.part`
/// is a fragment by contract), then handed to `--assembly-report` as a part
/// plus a feature name instead of three hand-typed numbers.
///
/// This is the whole STEP slice in one line of arguments: STEP → sidecar →
/// `proposed.part` → a human's program → the assembly recommender. Nothing in
/// the middle is new; the DRAFT's finding was that the section had already
/// been extracted and only a consumer was missing.
#[test]
fn test_a_recognized_bore_states_its_own_job_to_the_optimizer() {
    let proposed = fs::read_to_string("tests/fixtures/sunken_bore.proposed.golden.part")
        .expect("failed to read proposed golden");
    let src = format!(
        "units mm\n\
         feed 800\n\
         plunge 300\n\
         rapid_z 5\n\
         clearance_z 25\n\
         spindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
         {proposed}\n"
    );
    let dir = std::env::temp_dir().join(format!("step_slice_acceptance_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("recognized.part");
    fs::write(&path, &src).unwrap();
    let args: Vec<String> = vec![
        "tests/fixtures/sample.catalog".into(),
        format!("part={}", path.display()),
        "feature=F9_Bore".into(),
    ];
    let report = nacs::assembly::report_from_args(&args);
    fs::remove_dir_all(&dir).ok();
    let report = report.expect("a recognized bore must state its own job");

    // The golden declares `CylinderBore(dia=16.0, extent=8.0)`, so the three
    // numbers are 8 / 16 / 8 — and reach == depth is not a bug: for a plain
    // bore the opening IS the whole cavity, so the tip's reach below the top
    // face and the opening's own depth are one measurement.
    assert!(
        report.starts_with("assembly report — reach 8.000mm through a 16.000mm opening, 8.000mm deep"),
        "the header must carry the numbers read off the declaration: {report}"
    );
    // Fork 3A: a derived input whose provenance the reader cannot see is a
    // number they cannot check.
    assert!(
        report.contains("reach/opening/depth READ from feature 'F9_Bore' (CylinderBore)")
            && report.contains("not typed"),
        "the header must say the numbers were read, and from what: {report}"
    );
    // And it is a real report, not just a header: the recommender ran.
    assert!(report.contains("cutter '") && report.contains("tool T? Endmill("), "{report}");
}

/// The same numbers, reached by hand — the control that says the derivation
/// is a *reading* and not a second model. Typing what the declaration says
/// must produce the identical ranking.
#[test]
fn test_reading_the_part_and_typing_it_agree() {
    let proposed = fs::read_to_string("tests/fixtures/sunken_bore.proposed.golden.part")
        .expect("failed to read proposed golden");
    let src = format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n{proposed}\n"
    );
    let dir = std::env::temp_dir().join(format!("step_slice_control_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("recognized.part");
    fs::write(&path, &src).unwrap();
    let read = nacs::assembly::report_from_args(&[
        "tests/fixtures/sample.catalog".into(),
        format!("part={}", path.display()),
        "feature=F9_Bore".into(),
    ]);
    fs::remove_dir_all(&dir).ok();
    let read = read.expect("report");
    let typed = nacs::assembly::report_from_args(&[
        "tests/fixtures/sample.catalog".into(),
        "reach=8".into(),
        "opening=16".into(),
        "depth=8".into(),
    ])
    .expect("report");

    // Identical but for the provenance line the read one is entitled to.
    let stripped: String = read
        .lines()
        .filter(|l| !l.contains("READ from feature"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(stripped.trim(), typed.trim(),
        "reading the declaration must rank exactly as typing it does");
}

/// Phase 46 slice 2: a floor whose wire runs out to the part's SILHOUETTE
/// — a full-revolution cylinder with material INSIDE it, spanning the
/// floor's height — proposes as a pocket carrying its true wire: the
/// silhouette edge drawn two chord tolerances past the blank, the step wall
/// a wall, the rim of a bore piercing the floor's edge from below a wall the
/// Clear rides to (the bore proposed beside it). The engine, reading the
/// PROPOSAL on a `stock Cylinder` of the part's diameter, clips the outline
/// to the blank and reads the silhouette's chords OPEN; on a larger Box it
/// reads every edge WALL. Before this slice the floor was refused as
/// `pocket_floor_of_bore` naming the silhouette. Reads the committed golden;
/// `test_sidecar_golden` ties the golden to the sidecar.
#[test]
fn test_a_floor_open_at_the_silhouette_proposes_as_a_pocket_with_its_true_wire() {
    use nacs::region::{self, EdgeKind};
    let proposed = fs::read_to_string("tests/fixtures/disc_step.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F3_Pocket ProfilePocket(z=30.0, extent=10.0\n"), "{proposed}");
    assert!(proposed.contains("# open along F1: the part's silhouette, drawn 0.02 past the blank --"), "{proposed}");
    let report = fs::read_to_string("tests/fixtures/disc_step.report.golden.json").expect("failed to read report golden");
    assert!(report.contains("open along F1 (the blank's silhouette, drawn 0.02 past it); the edge along the rim of F7 is a wall the Clear rides to (a void proposed beside it)"), "{report}");
    assert!(!report.contains("pocket_floor_of_bore"), "the silhouette is not a bore's wall: {report}");
    // The silhouette's chords sit on r 60.02 — past the blank, never on it.
    let block = proposed.split("feature F3_Pocket").nth(1).unwrap().split(')').next().unwrap();
    let on_arc = block.lines().skip(1).filter_map(|l| {
        let mut it = l.split_whitespace().map(|w| w.parse::<f64>().unwrap());
        Some((it.next()?, it.next()?))
    }).filter(|(x, y)| (x.hypot(*y) - 60.02).abs() < 1e-3).count();
    assert!(on_arc > 60, "the arc's chords, pushed: {on_arc}");

    let wrap = |blank: &str| format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 35\nclearance_z 45\nspindle 3000\n{blank}\ntool T5 Endmill(dia=8.0)\n{proposed}\n\
         run End(tool=T5, surface=F5_Top, stepover=4.0)\n\
         run Clear(tool=T5, profile=F3_Pocket, stepover=3.0)\n\
         run Side(tool=T5, surface=F3_Pocket)\n\
         run Clear(tool=T5, profile=F7_Bore, stepover=3.0)\n"
    );
    let round = parse::parse(&wrap("stock Cylinder(dia=120, h=30, x=0, y=0, z=30)")).expect("the proposal parses on a round blank");
    let r = region::of(&round, &round.operations[1]).expect("the Clear works a region");
    let open = r.edges.iter().filter(|k| **k == EdgeKind::Open).count();
    assert!(open > 40 && open < r.edges.len(), "the silhouette's chords open; the step wall and the bore's rim walls: {open} of {}", r.edges.len());
    assert_eq!(r.feature_islands(), vec!["F8_Profile"], "the pad on the step is its island");
    // Phase 58 slice 1 (fork 1A). From Phase 47 slice 1 this test pinned a
    // refusal here — "the crescent beside the pad is floor the Clear cannot
    // reach … 0.000mm wide, a tool up to 0.000mm would reach it" — and that
    // sentence was the open corner's bug describing itself: the silhouette's
    // chords are open edges, their reflex vertices pulled the family's
    // allowed region a radius INSIDE the blank, and no loop rode the crescent
    // between the pad at r 55 and the round at r 60. The round is AIR beyond
    // r 60; the crescent is cut from the air side; the program generates
    // with its outermost loop one radius outside the blank, nothing conceded,
    // every entry licensed.
    let (program, moves) = gcode::program_with_moves(&round).expect("the round blank's program generates: nothing is unreachable");
    assert!(program.contains("M30"));
    assert!(!program.contains("(unreachable"), "nothing left by name: {program}");
    assert!(nacs::stock::entry_findings(&round, &moves).is_empty(), "every entry licensed");
    let clear = program.split("(--- pocket clear: F3_Pocket").nth(1).expect("the Clear is emitted").split("(--- side contour").next().unwrap();
    let (mut x, mut y, mut far) = (f64::NAN, f64::NAN, 0.0f64);
    for line in clear.lines().filter(|l| l.starts_with("G0")) {
        for (key, slot) in [('X', &mut x), ('Y', &mut y)] {
            if let Some(v) = line.split_whitespace().find_map(|t| t.strip_prefix(key)) {
                *slot = v.parse().unwrap();
            }
        }
        if line.starts_with("G01") && !x.is_nan() && !y.is_nan() {
            far = far.max(x.hypot(y));
        }
    }
    // 60 is the blank's radius and 4 the tool's: the loop rides a radius outside, to a chord.
    assert!((far - 64.0).abs() < 0.05, "the Clear's farthest point is r {far:.3}, not a radius outside the blank");

    let boxed = parse::parse(&wrap("stock Box(w=130, l=130, h=30, x=-65, y=-65, z=30)")).expect("the proposal parses on a box");
    let r = region::of(&boxed, &boxed.operations[1]).expect("the Clear works a region");
    assert!(r.edges.iter().all(|k| *k == EdgeKind::Wall), "inside a larger box, every edge is a wall: {:?}", r.edges);
}

/// ROADMAP item 1, slice 6: a head on a narrower neck, from above. Nothing
/// reaches what is under the head, so the head proposes down to the floor
/// it stands over (a ProfileBoss with a stepped base out to the rim, and a
/// CylinderBoss), every face under it is residue `under_overhang` naming the
/// head, the pad's foot bevel says it is cleared away, and the floor
/// proposes as the level: one `Plane`, open along the rim. The composed
/// proposal parses on the blank the import proposes, and the Clear on the
/// level derives both heads as islands and the bore as a void. Reads the
/// committed goldens; `test_sidecar_golden` ties them to the sidecar.
#[test]
fn test_a_head_proposes_down_to_the_floor_it_stands_over_and_the_floor_is_the_level() {
    let proposed = fs::read_to_string("tests/fixtures/rib_disc.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F1_Boss CylinderBoss(dia=24.0, extent=15.0, x=-15.0, y=0.0, z=20.0)"), "the round head, carried down to the floor: {proposed}");
    assert!(proposed.contains("feature F17_Profile ProfileBoss(z=35.0, extent=15.0\n"), "the rim head, carried down to the floor: {proposed}");
    assert!(proposed.contains("feature F7_Floor Plane(z=20.0)"), "the floor is the level, one Plane: {proposed}");
    assert!(proposed.contains("open along F9 (the blank's silhouette)"), "{proposed}");
    assert!(proposed.contains("run Clear(tool=..., profile=F7_Floor, stepover=...)"), "{proposed}");
    let report = fs::read_to_string("tests/fixtures/rib_disc.report.golden.json").expect("failed to read report golden");
    let faces = face_entries(&report);
    let under: Vec<usize> = faces.iter().enumerate().filter(|(_, f)| f.code.as_deref() == Some("under_overhang")).map(|(i, _)| i + 1).collect();
    assert_eq!(under.len(), 12, "the necks, undersides, pads and foot bevels of two heads: {under:?}");
    assert!(report.contains("the foot bevel of the pad under F17_Profile (cap F18): it stands outside the head's outline below the overhang"), "{report}");
    assert!(report.contains("cleared away to the outline, so the wall runs straight down to z=20.0 where the model has this bevel"), "{report}");
    assert!(report.contains("the head's underside, under the overhang of F1_Boss (cap F3)"), "{report}");

    let wrap = |blank: &str| format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 40\nclearance_z 50\nspindle 3000\n{blank}\ntool T5 Endmill(dia=8.0)\n{proposed}\n\
         run Clear(tool=T5, profile=F7_Floor, stepover=3.0)\n\
         run Side(tool=T5, surface=F17_Profile, comp=control)\n\
         run Side(tool=T5, surface=F1_Boss, stepover=2.0)\n"
    );
    use nacs::region::{self, EdgeKind};
    let round = parse::parse(&wrap("stock Cylinder(dia=100, h=35, x=0, y=0, z=35)")).expect("the proposal parses on the blank the import proposes");
    let r = region::of(&round, &round.operations[0]).expect("the Clear on the level works a region");
    // The round head stands inside the level: its island. The rim head stands
    // ON the blank's edge: bitten out of the region's boundary (Phase 49), not
    // an island -- the real part's ribs run out to the rim the same way.
    assert_eq!(r.feature_islands(), vec!["F1_Boss"], "the round head is the level's island, derived from containment");
    assert_eq!(r.on_the_shore, vec!["F17_Profile"], "the rim head is bitten out of the level's boundary");
    assert!(r.edges.iter().any(|k| *k == EdgeKind::Open), "the level is open along the rim: {:?}", r.edges);
    let (program, _moves) = gcode::program_with_moves(&round).expect("the level and the two heads generate");
    assert!(program.contains("M30"));
    assert!(!program.contains("(unreachable"), "nothing left by name: {program}");
}

/// ROADMAP item 1, slice 7: what the other setup owns says so. The small
/// rib disc from the back is its bottom, its through bore and its rim; every
/// other face has the part's own material straight above it and names the
/// face over it and the `--up` that shows it. A stepped plate from the back:
/// the hole across the step breaks out onto four faces and is a through
/// hole, the counterbore is a void under a ceiling and is withdrawn, and the
/// hole through its floor is through. Reads the committed goldens;
/// `test_sidecar_golden` ties them to the sidecar.
#[test]
fn test_what_the_other_setup_owns_says_so_from_the_back() {
    let proposed = fs::read_to_string("tests/fixtures/rib_disc_back.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F14_Top Plane(z=0.0)"), "the bottom, now the top: {proposed}");
    assert!(proposed.contains("feature F12_Hole CylinderBore(dia=10.0, extent=20.0, x=-15.0, y=-30.0)"), "the bore, through from this side: {proposed}");
    assert!(!proposed.contains("ProfileBoss"), "no head proposes from the back: {proposed}");
    let report = fs::read_to_string("tests/fixtures/rib_disc_back.report.golden.json").expect("failed to read report golden");
    let faces = face_entries(&report);
    let other: Vec<&FaceEntry> = faces.iter().filter(|f| f.code.as_deref() == Some("other_setup")).collect();
    let down = faces.iter().filter(|f| f.code.as_deref() == Some("plane_downward")).count();
    assert_eq!((other.len(), down, faces.len()), (14, 5, 22), "every face not proposed is under the part or faces down");
    assert!(other.iter().all(|f| f.refs.as_ref().is_some_and(|r| r.len() == 1)), "each names the one face over it: {report}");
    assert!(report.contains("cylindrical face: the part's own material lies straight above it -- conical F6 is 10.0 up -- so nothing on this setup's tool axis reaches it; see what the other side makes of it with --up z+"), "{report}");

    let proposed = fs::read_to_string("tests/fixtures/step_hole_back.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F10_Hole CylinderBore(dia=8.0, extent=10.0, x=30.0, y=-12.0)"), "the hole across the step is through: {proposed}");
    assert!(proposed.contains("feature F12_Hole CylinderBore(dia=6.0, extent=6.0, x=15.0, y=-28.0)"), "the hole under the counterbore is through: {proposed}");
    assert!(!proposed.contains("F11_"), "the counterbore does not propose from the back: {proposed}");
    let report = fs::read_to_string("tests/fixtures/step_hole_back.report.golden.json").expect("failed to read report golden");
    assert!(report.contains("bottom opens through -- a drillable through hole, breaking out across 4 faces (F8, F3, F9, F7)"), "{report}");
    let faces = face_entries(&report);
    assert_eq!(faces[10].code.as_deref(), Some("other_setup"), "the counterbore wall: {report}");
    assert_eq!(faces[10].refs.as_deref(), Some(&["F13".to_string()][..]), "names the floor over it: {report}");

    // The two sides compose: a front program drills the holes the back
    // sees, and the back's proposals parse and face on the blank turned over.
    let front = "units mm\nfeed 800\nplunge 300\nrapid_z 15\nclearance_z 25\nspindle 3000\n\
                 stock Box(w=60, l=40, h=10, x=0, y=0, z=10)\n\
                 tool T8 Drill(dia=8.0)\ntool T6 Drill(dia=6.0)\n\
                 feature Step CylinderBore(dia=8.0, extent=10.0, x=30.0, y=12.0)\n\
                 feature Small CylinderBore(dia=6.0, extent=10.0, x=15.0, y=28.0)\n\
                 run Form(points=[Step], tool=T8, cycle=Simple)\nrun Form(points=[Small], tool=T6, cycle=Simple)\n";
    let front = parse::parse(front).expect("the front program parses");
    let (program, _) = gcode::program_with_moves(&front).expect("the front drills");
    assert!(program.contains("M30"));
    let back = format!("units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
                 stock Box(w=60, l=40, h=10, x=0, y=-40, z=0)\ntool T1 Endmill(dia=6.0)\n{proposed}\n\
                 run Clear(tool=T1, profile=F5_Top, stepover=3.0)\n");
    let back = parse::parse(&back).expect("the back's proposals parse on the blank turned over");
    let (program, _) = gcode::program_with_moves(&back).expect("the back faces");
    assert!(program.contains("M30"));
}

/// ROADMAP item 1 slice 8: a drilled hole whose rim lies below the stock
/// top with open air straight above it proposes from the top, extent the
/// whole way down, so a drill through the blank cuts it before anything
/// else is. The stepped plate from the front: the hole under the
/// counterbore (rim at z=6, counterbore open above it to z=10) proposes
/// from z=10 with extent 10, and a drill run on it alone, nothing cut
/// before, writes; the same hole stated at its rim fails the drill's body
/// check, which is what the real part's holes did. The counterbore
/// itself, a milled bore, keeps its rim; the rib disc's through hole on
/// its stepped base moves from z=20 to the top. Reads the committed
/// goldens; `test_sidecar_golden` ties them to the sidecar.
#[test]
fn test_a_hole_under_open_air_proposes_from_the_top() {
    let proposed = fs::read_to_string("tests/fixtures/step_hole.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F12_Hole CylinderBore(dia=6.0, extent=10.0, x=15.0, y=28.0)"), "the hole under the counterbore, from the top: {proposed}");
    assert!(proposed.contains("feature F11_Bore CylinderBore(dia=12.0, extent=4.0, x=15.0, y=28.0)"), "the counterbore at the top, a milled bore: {proposed}");
    assert!(proposed.contains("feature F10_Hole CylinderBore(dia=8.0, extent=10.0, x=30.0, y=12.0)"), "the hole across the step, its rim at the top: {proposed}");
    assert!(proposed.contains("# through hole -- drill past extent=10.0 by the tip's point length"), "the drill note carries the new extent: {proposed}");
    let report = fs::read_to_string("tests/fixtures/step_hole.report.golden.json").expect("failed to read report golden");
    assert!(report.contains("rim at z=6.0, 4.0 below the stock top z=10.0, with open air straight above it to the top -- proposed from the top, where a drill through the blank starts, extent 10.0 the whole way down"), "{report}");
    let disc = fs::read_to_string("tests/fixtures/rib_disc.proposed.golden.part").expect("failed to read proposed golden");
    assert!(disc.contains("feature F12_Hole CylinderBore(dia=10.0, extent=35.0, x=-15.0, y=30.0)"), "the disc's through hole on the stepped base, from the top: {disc}");

    let program = |hole: &str| {
        let src = format!("units mm\nfeed 800\nplunge 300\nrapid_z 15\nclearance_z 25\nspindle 3000\n\
                 stock Box(w=60, l=40, h=10, x=0, y=0, z=10)\ntool T6 Drill(dia=6.0)\n\
                 feature Small CylinderBore({hole}, x=15.0, y=28.0)\n\
                 run Form(points=[Small], tool=T6, cycle=Simple)\n");
        let part = parse::parse(&src).expect("the drill program parses");
        gcode::program_with_moves(&part)
    };
    let (gcode, _) = program("dia=6.0, extent=10.0").expect("drilled from the top through the uncut blank, the hole writes");
    assert!(gcode.contains("M30"));
    let err = program("dia=6.0, extent=6.0, z=6.0").err().expect("stated at its rim with the blank above it uncut, the drill's body is in material");
    assert!(err.to_string().contains("body"), "{err}");
}

/// A hole the model interrupts is one hole. The block's hole at x=32
/// crosses a groove, so the model has two wall faces there; each side
/// proposes it once, the whole way through, and the drill run the
/// proposal suggests writes on the uncut blank. Reads the committed
/// goldens; `test_sidecar_golden` ties them to the sidecar.
#[test]
fn test_a_hole_across_a_groove_proposes_once() {
    for (golden, y) in [("split_hole", "15.0"), ("split_hole_back", "-15.0")] {
        let proposed = fs::read_to_string(format!("tests/fixtures/{golden}.proposed.golden.part")).expect("failed to read proposed golden");
        let holes: Vec<&str> = proposed.lines().filter(|l| l.starts_with("feature") && l.contains("x=32.0")).collect();
        assert_eq!(holes.len(), 1, "{golden}: one hole on the split axis: {holes:?}");
        assert!(holes[0].ends_with(&format!("CylinderBore(dia=4.0, extent=20.0, x=32.0, y={y})")), "{golden}: through the whole block: {holes:?}");
        let report = fs::read_to_string(format!("tests/fixtures/{golden}.report.golden.json")).expect("failed to read report golden");
        let faces = face_entries(&report);
        assert_eq!(faces.iter().filter(|f| f.status == "proposed").count(), 5, "{golden}: both walls of the split hole are proposed faces: {report}");
        let name = holes[0].split_whitespace().nth(1).unwrap();
        let (stock_y, stock_z) = if y == "15.0" { ("0", "20") } else { ("-30", "0") };
        let src = format!("units mm\nfeed 800\nplunge 300\nrapid_z 25\nclearance_z 30\nspindle 3000\n\
                 stock Box(w=40, l=30, h=20, x=0, y={stock_y}, z={stock_z})\ntool T4 Drill(dia=4.0)\n{}\n\
                 run Form(points=[{name}], tool=T4, cycle=Simple)\n", holes[0]);
        let part = parse::parse(&src).expect("the drill program parses");
        let (program, _) = gcode::program_with_moves(&part).expect("the split hole drills once through the blank");
        assert!(program.contains("M30"), "{golden}: {program}");
    }
}

/// ROADMAP item 9: a pocket that runs out onto a bevel. The floor's fillet
/// is read through to the walls behind it, the mouth's round is the
/// pocket's `edge_round`, and the side where the bevel falls away is drawn
/// out past the blank, so the clearing enters from that side in air.
/// Reads the committed goldens; `test_sidecar_golden` ties them to the
/// sidecar.
#[test]
fn test_a_pocket_that_runs_out_onto_a_bevel_proposes_open() {
    let proposed = fs::read_to_string("tests/fixtures/open_pocket.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F9_Pocket ProfilePocket(z=20.0, extent=6.0, edge_round=1.0\n    -0.02 8.0\n    40.0 8.0\n"), "the walls' own line, past the blank on the open side: {proposed}");
    assert!(proposed.contains("    40.0 32.0\n    -0.02 32.0\n)"), "{proposed}");
    let report = fs::read_to_string("tests/fixtures/open_pocket.report.golden.json").expect("failed to read report golden");
    let faces = face_entries(&report);
    assert_eq!(faces.iter().filter(|f| f.status == "proposed").count(), 21, "the fillets and the rounds are the pocket's faces: {report}");
    assert_eq!(faces[0].code.as_deref(), Some("plane_oblique"), "the bevel itself is still refused: {report}");

    let pocket = proposed.split("\n\n").find(|b| b.contains("feature F9_Pocket")).unwrap();
    let pocket: String = pocket.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n");
    let src = format!("units mm\nfeed 800\nplunge 300\nrapid_z 25\nclearance_z 30\nspindle 3000\n\
                 stock Box(w=60, l=40, h=20, x=0, y=0, z=20)\ntool T6 Endmill(dia=6.0)\n{pocket}\n\
                 run Clear(tool=T6, profile=F9_Pocket, stepover=3.0)\n");
    let part = parse::parse(&src).expect("the proposed pocket parses");
    let (program, _) = gcode::program_with_moves(&part).expect("the pocket clears");
    assert!(program.contains("(open edges: 1 of 30 — entered from air)"), "the bevel's side reads open: {program}");
}

/// ROADMAP item 9: the open_pocket block with a boss standing in the pocket,
/// taller than the block. The block's top is below the part's topmost face
/// and nothing beside it rises above it, so it proposes as a level: the
/// whole blank cleared to it round the boss, and the pocket cut from it. The
/// engine takes the boss as the island of both. Reads the committed goldens;
/// `test_sidecar_golden` ties them to the sidecar.
#[test]
fn test_a_block_top_below_a_taller_boss_proposes_as_a_level() {
    let proposed = fs::read_to_string("tests/fixtures/block_level.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F2_Floor Plane(z=20.0)"), "{proposed}");
    assert!(proposed.contains("feature F22_Boss CylinderBoss(dia=10.0, extent=12.0, x=28.0, y=20.0, z=14.0)"), "{proposed}");

    // The outline's own block is left out: a pocket drawn out past the blank
    // crosses it, and that pair is its own change.
    let features: String = proposed.split("

").filter(|b| !b.contains("feature F4_Trim")).collect::<Vec<_>>().join("

");
    let src = format!("units mm
feed 800
plunge 300
rapid_z 31
clearance_z 36
spindle 3000
                 stock Box(w=60, l=40, h=26, x=0, y=0, z=26)
tool T6 Endmill(dia=6.0)
{features}
                 run Clear(tool=T6, profile=F2_Floor, stepover=3.0)
run Clear(tool=T6, profile=F9_Pocket, stepover=3.0)
");
    let part = parse::parse(&src).expect("the proposals parse together");
    let (program, _) = gcode::program_with_moves(&part).expect("the level and the pocket clear");
    let level = program.split("(--- pocket clear: F9_Pocket").next().unwrap();
    assert!(level.contains("(--- volume clear: F2_Floor") && level.contains("(islands: F22_Boss)"), "the level goes round the boss: {program}");
    assert_eq!(program.matches("(islands: F22_Boss)").count(), 2, "and so does the pocket: {program}");
}

/// ROADMAP item 1 slice 9: a boss whose top edges carry two legs at one
/// angle proposes with a chamfer, the feature carrying the leg that runs the
/// longest way round the loop and a point restating the leg where it
/// changes; every strip is a member. The proposal parses, and the engine
/// forms it in runs. Reads the committed golden; `test_sidecar_golden` ties
/// it to the sidecar.
#[test]
fn test_two_legs_round_one_cap_propose_as_marks_on_the_loop() {
    let proposed = fs::read_to_string("tests/fixtures/uneven_bevels.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F7_Profile ProfileBoss(z=28.0, extent=8.0, chamfer_depth=1.0, chamfer_angle=90.0\n    10.0 10.0 chamfer_depth=1.5\n    10.0 30.0 chamfer_depth=1.0\n    40.0 30.0 chamfer_depth=1.5\n    40.0 10.0 chamfer_depth=1.0\n)"), "the long edges 1.0, the short ones 1.5, restated where the leg changes: {proposed}");
    let report = fs::read_to_string("tests/fixtures/uneven_bevels.report.golden.json").expect("failed to read report golden");
    assert!(!report.contains("chamfer_uneven"), "one angle, two legs is no longer a refusal: {report}");
    let faces = face_entries(&report);
    for strip in [11usize, 12, 13, 14] {
        assert_eq!(faces[strip - 1].status, "proposed", "F{strip} is a member of the boss: {report}");
    }
    assert!(report.contains("legs that differ edge to edge (F13 F12 F14 F11: 1.0, 1.5) -- the loop carries 1.0 as its chamfer_depth and its points restate the leg where it changes"), "{report}");

    let src = format!("units mm\nfeed 800\nplunge 300\nrapid_z 35\nclearance_z 45\nspindle 3000\n\
                 stock Box(w=100, l=80, h=28, x=0, y=0, z=28)\ntool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n\
                 {proposed}\nrun Form(tool=T3, surface=F7_Profile)\n");
    let part = parse::parse(&src).expect("the proposal parses");
    let (program, _) = gcode::program_with_moves(&part).expect("the engine forms the two legs");
    assert!(program.contains("legs restated on the loop: 4 runs (1.500 deep, 1.000 deep, 1.500 deep, 1.000 deep)"), "{program}");
}

/// Phase 46 slice 3: a 45° strip (or a cone at a round mouth) bounded above
/// by a +Z plane and below by a vertical wall is that wall's edge break,
/// absorbed into the wall's group and written as the engine's own keys —
/// `chamfer_depth=` and `chamfer_angle=` (the included angle), the loop
/// read at the WALL. A strip that is a wall's whole height has no flank under
/// it and refuses by the engine's sentence, landing on the cap and every
/// strip. The proposal, with a `ChamferMill` beside the `Endmill`, forms all
/// three bevels. Reads the committed golden; `test_sidecar_golden` ties the
/// golden to the sidecar.
#[test]
fn test_a_bevel_is_a_leg_on_the_feature_whose_edge_it_breaks() {
    let proposed = fs::read_to_string("tests/fixtures/chamfer_plate.proposed.golden.part").expect("failed to read proposed golden");
    assert!(proposed.contains("feature F7_Profile ProfileBoss(z=28.0, extent=8.0, chamfer_depth=1.0, chamfer_angle=90.0\n    10.0 10.0\n"), "the boss, its loop at the walls: {proposed}");
    assert!(proposed.contains("feature F32_Pocket ProfilePocket(z=20.0, extent=8.0, chamfer_depth=1.0, chamfer_angle=90.0\n"), "{proposed}");
    assert!(proposed.contains("feature F19_Bore CylinderBore(dia=12.0, extent=10.0, x=70.0, y=60.0, z=20.0, chamfer_depth=1.0, chamfer_angle=90.0)"), "{proposed}");
    let report = fs::read_to_string("tests/fixtures/chamfer_plate.report.golden.json").expect("failed to read report golden");
    let faces = face_entries(&report);  // in face order: F1 is faces[0]
    let code = |n: usize| faces[n - 1].code.clone().unwrap_or_default();
    assert_eq!(code(29), "outer_profile_candidate_cap/chamfer_consumes_wall", "the pad's cap");
    assert_eq!(code(15), "outer_profile_candidate_wall/chamfer_consumes_wall", "the pad's strip");
    assert!(report.contains("a draft wall (CHECKLIST row 1.2, no tilted-plane primitive) or a bevel as deep as its wall"), "{report}");
    for strip in [11usize, 21, 2] {
        assert_eq!(faces[strip - 1].status, "proposed", "F{strip} is a member of its feature");
    }

    let src = format!(
        "units mm\nfeed 800\nplunge 300\nrapid_z 33\nclearance_z 45\nspindle 3000\n\
         stock Box(w=100, l=80, h=28, x=0, y=0, z=28)\n\
         tool T5 Endmill(dia=8.0)\n\
         tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n\
         {proposed}\n\
         run End(tool=T5, surface=F1_Face, stepover=4.0)\n\
         run Side(tool=T5, surface=F7_Profile)\n\
         run Form(tool=T3, surface=F7_Profile, comp=control, tip_clearance=0.6)\n\
         run Clear(tool=T5, profile=F32_Pocket, stepover=3.0)\n\
         run Side(tool=T5, surface=F32_Pocket, comp=control)\n\
         run Form(tool=T3, surface=F32_Pocket)\n\
         run Clear(tool=T5, profile=F19_Bore, stepover=3.0)\n\
         run Form(tool=T3, surface=F19_Bore)\n"
    );
    let part = parse::parse(&src).expect("the chamfer proposals parse in the current grammar");
    // The Face's inner wire at the pocket's beveled mouth is a leg wider
    // than the pocket; it is the pocket's mouth, bridged, not a hole to go
    // round (the engine's slice-3 rule) — else the column over the pocket
    // would stand for the Clear to be refused into.
    let r = nacs::region::of(&part, &part.operations[0]).unwrap();
    assert!(r.bridged().contains(&"F32_Pocket") && r.bridged().contains(&"F19_Bore"), "{:?}", r.bridged());
    let (program, moves) = gcode::program_with_moves(&part).expect("the beveled plate's program generates");
    assert_eq!(program.matches("(--- chamfer edge").count(), 3, "three bevels formed: {program}");
    assert!(nacs::stock::entry_findings(&part, &moves).is_empty(), "every entry licensed");
}

/// `--allowance 1,bottom=3,x+=10` gives a side of the blank its own margin:
/// the Box grows by each, the origin's words read the grown blank in both
/// scripts, and a round blank, which has one margin on its sides, refuses
/// sides that differ. Skips without the venv.
#[test]
fn test_an_allowance_per_side_places_the_part_in_the_blank() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping sidecar margins test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_sidecar_margins_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);
    let run = |script: &str, step: &str, allowance: &str| Command::new(VENV_PYTHON).args([script, step, "--allowance", allowance, "--origin", "mid,mid,max", "-o"]).arg(&outdir).output().expect("failed to run the sidecar");
    // The plate is 100 x 80 x 15: 1 + 10 on X, 1 + 1 on Y, 1 above and 3 under.
    for script in ["sidecar/import_step.py", "sidecar/import_stock.py"] {
        let out = run(script, "tests/fixtures/drilled_plate.step", "1,bottom=3,x+=10");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let stock = fs::read_to_string(outdir.join("proposed_stock.part")).unwrap();
    assert!(stock.ends_with("stock Box(w=111.0, l=82.0, h=19.0, x=-55.5, y=-41.0, z=0.0)\n"), "{stock}");
    // The features are measured from the same zero: the bore sits 4.5 off
    // where the even blank had it, and 1 below the blank's top.
    let part = fs::read_to_string(outdir.join("proposed.part")).unwrap();
    assert!(part.contains("\nfeature F9_Bore CylinderBore(dia=16.0, extent=6.0, x=25.5, y=-20.0, z=-1.0)\n"), "{part}");
    assert!(part.contains("top 1.0, bottom 3.0, x- 1.0, x+ 10.0, y- 1.0, y+ 1.0"), "{part}");
    // A round blank: top and bottom of its own, the sides alike.
    let out = run("sidecar/import_stock.py", "tests/fixtures/disc_step.step", "1,bottom=2");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let disc = fs::read_to_string(outdir.join("proposed_stock.part")).unwrap();
    assert!(disc.ends_with("stock Cylinder(dia=122.0, h=33.0, x=0.0, y=0.0, z=0.0)\n"), "{disc}");
    for script in ["sidecar/import_step.py", "sidecar/import_stock.py"] {
        let out = run(script, "tests/fixtures/disc_step.step", "1,x+=5");
        assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("a round blank has one margin on its sides"), "{script}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let out = run("sidecar/import_stock.py", "tests/fixtures/drilled_plate.step", "1,left=2");
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("`left=2` is not a size in mm"), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = fs::remove_dir_all(&outdir);
}

/// The flip with a zero of its own, through the doors the window uses: a
/// first setup imported at the blank's centre and top, then the same model
/// turned over with `given=`. The second import finds the model in the
/// first one's frame from that project's report, zeroes at the turned
/// blank's centre and new top, and writes a `given from … at=` line the
/// engine accepts. Skips without the venv.
#[test]
fn test_a_flipped_setup_is_proposed_at_its_own_zero_and_the_engine_reads_it() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping sidecar flip test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let root = std::env::temp_dir().join(format!("nacs_sidecar_flip_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (front, back) = (root.join("front"), root.join("back"));
    fs::create_dir_all(&back).unwrap();
    let sidecar = Path::new("sidecar");
    let step = Path::new("tests/fixtures/drilled_plate.step");
    let head = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n";
    let import = |dir: &Path, setup: &nacs::view::ImportSetup| {
        for (python, args) in [nacs::view::import_command(sidecar, step, dir, setup).unwrap(), nacs::view::stock_command(sidecar, step, dir, setup).unwrap()] {
            let out = Command::new(python).args(args).output().expect("failed to run the sidecar");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
        let stock = fs::read_to_string(dir.join("proposed_stock.part")).unwrap();
        fs::write(dir.join("main.part"), format!("{head}{stock}")).unwrap();
        stock
    };
    // The first setup: the plate is 100 x 80 x 15 in a blank 1 larger.
    let stock = import(&front, &nacs::view::ImportSetup::default());
    assert!(stock.ends_with("stock Box(w=102.0, l=82.0, h=16.0, x=-51.0, y=-41.0, z=0.0)\n"), "{stock}");
    // Turned over: the blank's centre and its new top are zero, and the
    // first program's zero is 16 under it.
    let given = front.to_string_lossy().to_string();
    let flipped = nacs::view::ImportSetup { up: Some("z-".into()), given: given.clone(), ..Default::default() };
    let stock = import(&back, &flipped);
    assert!(stock.ends_with(&format!("stock Box(w=102.0, l=82.0, h=16.0, x=-51.0, y=-41.0, z=0.0)\ngiven from {given} up=z- at=0.0,0.0,-16.0\n")), "{stock}");
    let part = nacs::parse::load_project(&back.to_string_lossy()).unwrap_or_else(|e| panic!("the engine reads the flipped setup: {e:?}"));
    assert_eq!(part.given_from.as_ref().unwrap().at, [0.0, 0.0, -16.0]);
    // The features are the back's: a hole mirrored in Y, from the new top.
    let features = fs::read_to_string(back.join("proposed.part")).unwrap();
    assert!(features.contains("\nfeature F7_Hole CylinderBore(dia=12.0, extent=15.0, x=-30.0, y=20.0)\n"), "{features}");
    // A corner of the turned blank instead: the same line with its own at=.
    let corner = nacs::view::ImportSetup { origin: "min,min,max".into(), ..flipped.clone() };
    let stock = import(&back, &corner);
    assert!(stock.contains("stock Box(w=102.0, l=82.0, h=16.0, x=0.0, y=0.0, z=0.0)\n") && stock.contains("up=z- at=51.0,41.0,-16.0\n"), "{stock}");
    assert!(nacs::parse::load_project(&back.to_string_lossy()).is_ok());
    let _ = fs::remove_dir_all(&root);
}

/// `--origin x,y,z` moves the model so the named point is 0,0,0 before
/// anything is recognized: both scripts agree, a word reads the blank or
/// (`part-`) the model, and a number is a model coordinate. Skips without
/// the venv.
#[test]
fn test_the_origin_is_the_point_asked_for_in_both_proposals() {
    if !Path::new(VENV_PYTHON).exists() {
        println!("Skipping sidecar origin test: sidecar/.venv not installed (see sidecar/README.md)");
        return;
    }
    let outdir = std::env::temp_dir().join(format!("nacs_sidecar_origin_{}", std::process::id()));
    let _ = fs::remove_dir_all(&outdir);
    let step = "tests/fixtures/drilled_plate.step";
    let both = |origin: &str| {
        for script in ["sidecar/import_step.py", "sidecar/import_stock.py"] {
            let out = Command::new(VENV_PYTHON).args([script, step, "--allowance", "1", "--origin", origin, "-o"]).arg(&outdir).output().expect("failed to run the sidecar");
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
        (fs::read_to_string(outdir.join("proposed_stock.part")).unwrap(), fs::read_to_string(outdir.join("proposed.part")).unwrap())
    };
    // The blank's centre and its top.
    let (stock, part) = both("mid,mid,max");
    assert!(stock.ends_with("stock Box(w=102.0, l=82.0, h=16.0, x=-51.0, y=-41.0, z=0.0)\n"), "{stock}");
    assert!(stock.contains("--origin mid,mid,max: the model's point (50.0, 40.0, 16.0) is 0,0,0 here"), "{stock}");
    assert!(part.contains("\nfeature F9_Bore CylinderBore(dia=16.0, extent=6.0, x=30.0, y=-20.0, z=-1.0)\n"), "{part}");
    // The blank's corner in plan, the part's own top.
    let (stock, part) = both("min,min,part-max");
    assert!(stock.ends_with("x=0.0, y=0.0, z=1.0)\n"), "{stock}");
    assert!(part.contains("\nfeature F9_Bore CylinderBore(dia=16.0, extent=6.0, x=81.0, y=21.0, z=0.0)\n"), "{part}");
    // A typed point, an axis left as modeled, the bottom.
    let (stock, part) = both("10,model,min");
    assert!(stock.ends_with("x=-11.0, y=-1.0, z=16.0)\n"), "{stock}");
    assert!(part.contains("x=70.0, y=20.0, z=15.0)\n"), "{part}");
    // The report says what was asked.
    let report = fs::read_to_string(outdir.join("report.json")).unwrap();
    assert!(report.contains("\"origin\": \"10,model,min\""), "{report}");
    for (args, says) in [(vec!["--origin", "mid,mid"], "not three components"), (vec!["--origin", "mid,mid,up"], "is not model, a number")] {
        let out = Command::new(VENV_PYTHON).args(["sidecar/import_step.py", step, "-o"]).arg(&outdir).args(&args).output().expect("failed to run the sidecar");
        assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains(says), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let _ = fs::remove_dir_all(&outdir);
}
