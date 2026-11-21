// ---- given from: the program before this one, turned over (Phase 48 slice 1)
//
// `DECISIONS.md` → "Phase 48 slice 1…", fork 1A of the Phase 48 DRAFT: a
// second setup's program KNOWS what the first one did. `given from <project>
// up=<axis>` loads the earlier project by path, turns it as the sidecar's
// own table turns a model (`z-`: 180° about X through the origin, (x, y, z)
// → (x, −y, −z)), and subtracts every closed-form removal its ops declare at
// this program's prefix 0 — so the stock model of the second setup is the
// part as it arrived, and the voxel field reads the same prefix 0. The
// stocks must coincide (a turn, never a translation); a removal the model
// cannot turn — a shaped floor, which turned over is a shaped ceiling — is
// declined by name and the model says it is incomplete. These pins hold the
// statement's refusals, the turned air read by `remaining_before(p, 0)`,
// the web the earlier program left under its short bore standing in this
// frame, the decline, the outline's lines, and the field: the back at
// prefix 0 holds what the front's end state holds.

use nacs::parse::{self, Part};
use nacs::{outline, stock, vox};
use std::fs;
use std::path::PathBuf;

// The rapid and clearance planes stand above EVERY stock top a scratch
// project here declares (the tallest is z=65): the header law (Phase 50
// slice 1) refuses a rapid plane inside the blank, and this header once
// said `rapid_z 5` under tops of 20, 22, 25 and 65.
const HEAD: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 70\nclearance_z 80\nspindle 3000\n";

fn load(dir: &str) -> Part {
    match parse::load_project(dir) {
        Ok(p) => p,
        Err(parse::LoadError::Io(e)) | Err(parse::LoadError::Parse(e)) => panic!("{dir} must load: {e}"),
    }
}

fn load_err(dir: &str) -> String {
    match parse::load_project(dir) {
        Ok(_) => panic!("{dir} must refuse"),
        Err(parse::LoadError::Io(e)) | Err(parse::LoadError::Parse(e)) => e,
    }
}

/// A scratch project directory under the OS temp dir — NOT ending in `.cam`,
/// so no corpus sweep ever mistakes it for a fixture.
fn scratch(name: &str, main_src: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nacs_given_from_{name}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    fs::write(dir.join("main.part"), main_src).expect("scratch main.part");
    dir
}

/// The repo's front fixture, by an absolute path a scratch project can name.
fn front_abs() -> String {
    fs::canonicalize("through_plate.cam")
        .expect("through_plate.cam is a shipped fixture")
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string()
}

// ---- the pair, as shipped

#[test]
fn test_the_back_reads_the_front_as_air_turned_over() {
    let p = load("through_plate_back.cam");
    let before = stock::remaining_before(&p, 0);
    assert!(before.complete, "every removal of the front is a flat-floored column, so nothing is declined");
    // The front's Left bore was at (−25, 6), through; turned over it is at
    // (−25, −6), through — air at mid-depth in this frame.
    assert!(!before.material_at(-25.0, -6.0, -6.0), "the through-bore is air in the back's frame");
    assert!(!before.material_at(-25.0, -6.0, -11.5), "…down to what was the front's top");
    // The Short bore stopped at z 1 on the front (floor at 1, top at 12):
    // turned over it spans z −12..−1, a blind hole FROM BELOW — the web under
    // this face is steel, and this program never gave the bore.
    assert!(before.material_at(25.0, -6.0, -0.5), "the web the front left is steel to the back");
    assert!(!before.material_at(25.0, -6.0, -2.0), "and under the web, the bore the front cut is air");
    // The mirror is in y: the front's y 6 is not air here.
    assert!(before.material_at(-25.0, 6.0, -6.0), "the turn mirrors y — the front's side is steel on the back");
    // The plate itself stands.
    assert!(before.material_at(40.0, -20.0, -6.0));
}

#[test]
fn test_the_outline_says_the_statement_and_what_it_read() {
    let p = load("through_plate_back.cam");
    let text = outline::report(&p);
    assert!(
        text.contains("given from ../through_plate.cam up=z-: 6 removal(s) from 6 of its 6 op(s), turned over (y → −y, z → −z) — air at prefix 0"),
        "{text}"
    );
    assert!(!text.contains("declined:"), "{text}");
    let o = outline::outline(&p);
    assert_eq!(o.given_from.len(), 3, "the statement, then a verdict per restated void (slice 2): {:?}", o.given_from);
    assert_eq!(o.given.len(), 2, "the two restated bores are given as before");
}

#[test]
fn test_the_field_at_the_back_s_prefix_0_is_the_front_s_end_state() {
    // The same lattice both ways: the blank is 90 × 50 × 12, the cell the
    // same number, so a cell count on one side is a cell count on the other.
    let front = load("through_plate.cam");
    let back = load("through_plate_back.cam");
    let cell = Some(90.0 / 128.0);
    let f_end = vox::VoxField::build_with_cell(&front, cell).expect("the front's end state");
    let b_start = vox::VoxField::build_prefix(&back, 0, cell).expect("the back before its first op");
    let count = |field: &vox::VoxField, z_lo: f64| {
        let n = 128usize;
        let (sx, sy, sz) = (90.0 / n as f64, 50.0 / n as f64, 12.0 / n as f64);
        let mut standing = 0usize;
        for k in 0..n {
            for j in 0..n {
                for i in 0..n {
                    let (x, y, z) = (-45.0 + (i as f64 + 0.5) * sx, -25.0 + (j as f64 + 0.5) * sy, z_lo + (k as f64 + 0.5) * sz);
                    if field.material_at(x, y, z) {
                        standing += 1;
                    }
                }
            }
        }
        standing
    };
    let a = count(&f_end, 0.0);
    let b = count(&b_start, -12.0);
    // The front's field is SWEPT (the endmill's rod along its passes); the
    // back's is the front's DECLARED removals: three discs. The two truths
    // diverge by what the rest report exists to measure — 0.13% of the blank
    // when this was pinned (2,752 of 2,097,152 cells, ~70 mm³ of a 48,000
    // mm³ part), the sweep standing the more — and a quarter percent is the
    // bar. A back that still held the blank's far side would miss by six.
    let blank = 128usize.pow(3);
    assert!(
        (a as i64 - b as i64).unsigned_abs() as usize * 400 < blank,
        "front end state {a} cells, back prefix 0 {b} cells, of {blank}"
    );
    // And the back's field is honest about what it cannot promise: the
    // short bore's column stops under this face, so no XY column is one
    // interval from the bottom.
    assert!(!b_start.upward_closed(), "a blind hole from below breaks the column-contiguity lemma");
    assert!(f_end.upward_closed());
}

#[test]
fn test_the_rest_report_on_the_back_stands_the_part() {
    let report = vox::rest_report(&load("through_plate_back.cam"), Some(90.0 / 128.0)).unwrap();
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(report.contains("forgotten (declared removed, never swept): none"), "{report}");
    // The blank is 54,000 mm³; the front took three bores (~5,388 mm³) and
    // this program two chamfers (~47 mm³). The lattice quantizes the blank's
    // faces (0.7 mm cells: ~53,800 mm³ of blank), so the bar is wide — a
    // report that still counted the blank's far side would stand within a
    // few hundred mm³ of 54,000, and this one stands the part.
    let line = report.lines().find(|l| l.starts_with("standing material:")).expect("a standing line");
    let mm3: f64 = line.split("(~").nth(1).unwrap().split(' ').next().unwrap().parse().unwrap();
    assert!(mm3 < 50_000.0 && mm3 > 46_000.0, "{line}");
}

#[test]
fn test_the_pair_s_programs_match_their_goldens() {
    for name in ["through_plate", "through_plate_back"] {
        let p = load(&format!("{name}.cam"));
        let program = nacs::gcode::program(&p).expect("generates");
        let golden = fs::read_to_string(format!("tests/{name}.golden.gcode")).expect("golden");
        assert_eq!(program.trim(), golden.trim(), "{name}");
    }
}

// ---- the turn's other word, and the refusals

#[test]
fn test_z_plus_is_the_same_side() {
    let dir = scratch(
        "zplus",
        &format!(
            "{HEAD}given from {} up=z+\nstock Box(w=90, l=50, h=12, x=-45, y=-25, z=12)\ntool T5 Endmill(dia=8.0)\n",
            front_abs()
        ),
    );
    let p = load(&dir.to_string_lossy());
    let before = stock::remaining_before(&p, 0);
    assert!(!before.material_at(-25.0, 6.0, 6.0), "the front's Left bore, untouched by any turn");
    assert!(before.material_at(25.0, 6.0, 0.5), "the web under the Short bore, where the front left it");
    assert!(!before.material_at(25.0, 6.0, 2.0));
    let text = outline::report(&p);
    assert!(text.contains("up=z+: 6 removal(s) from 6 of its 6 op(s), the same side up (no turn)"), "{text}");
}

#[test]
fn test_the_stocks_must_coincide() {
    // The back zeroed at its own top (z 12) instead of where the turn puts
    // the blank (z 0): two origins, which `given from` refuses to bridge.
    let dir = scratch(
        "origin",
        &format!("{HEAD}given from {} up=z-\nstock Box(w=90, l=50, h=12, x=-45, y=-25, z=12)\ntool T5 Endmill(dia=8.0)\n", front_abs()),
    );
    let e = load_err(&dir.to_string_lossy());
    assert!(e.contains("the blank turned over must be the blank"), "{e}");
    assert!(e.contains("turned over (y → −y, z → −z) is Box(w=90, l=50, h=12, x=-45, y=-25, z=0)"), "{e}");
    assert!(e.contains("this program's is Box(w=90, l=50, h=12, x=-45, y=-25, z=12)"), "{e}");
}

#[test]
fn test_a_round_blank_s_refusal_prints_the_axis_not_the_corner() {
    // The stock line says a Cylinder's AXIS and the struct keeps its
    // bounding box; the refusal must speak the line's language (the CLOSE's
    // own fix: the first draft printed the corner, minus the radius for a round on
    // the origin).
    let front = scratch("round_front", &format!("{HEAD}stock Cylinder(dia=300, h=65, x=0, y=0, z=65)\ntool T5 Endmill(dia=8.0)\n"));
    let back = scratch(
        "round_back",
        &format!("{HEAD}given from {} up=z-\nstock Cylinder(dia=300, h=65, x=0, y=0, z=65)\n", front.to_string_lossy()),
    );
    let e = load_err(&back.to_string_lossy());
    assert!(e.contains("turned over (y → −y, z → −z) is Cylinder(dia=300, h=65, x=0, y=0, z=0), and this program's is Cylinder(dia=300, h=65, x=0, y=0, z=65)"), "{e}");
}

#[test]
fn test_the_statement_s_own_refusals() {
    let refuse = |line: &str| -> String {
        parse::parse(&format!("{HEAD}{line}\nstock Box(w=10, l=10, h=5, x=0, y=0, z=0)\n"))
            .err()
            .unwrap_or_else(|| panic!("must refuse: {line}"))
    };
    let front = front_abs();
    assert!(refuse(&format!("given from {front}")).contains("up= is not defaulted"));
    assert!(refuse("given from").contains("up= is not defaulted"));
    assert!(refuse(&format!("given from {front} z-")).contains("second word is up=<axis>"));
    assert!(refuse(&format!("given from {front} up=x+")).contains("quarter turn"));
    assert!(refuse(&format!("given from {front} up=sideways")).contains("not an axis word"));
    assert!(refuse("given from nowhere_such.cam up=z-").contains("no such project"));
    assert!(refuse(&format!("given from {front} up=z-\ngiven from {front} up=z-")).contains("declared twice"));
    // A broken earlier program is refused with its own sentence.
    let broken = scratch("broken", &format!("{HEAD}stock Box(w=1, l=1, h=1)\nrun Clear(tool=T5, profile=Nothing)\n"));
    let e = refuse(&format!("given from {} up=z-", broken.to_string_lossy()));
    assert!(e.contains("does not stand on its own"), "{e}");
}

#[test]
fn test_a_chain_of_setups_refuses() {
    let back = fs::canonicalize("through_plate_back.cam").unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let dir = scratch("chain", &format!("{HEAD}given from {back} up=z-\nstock Box(w=90, l=50, h=12, x=-45, y=-25, z=12)\n"));
    let e = load_err(&dir.to_string_lossy());
    assert!(e.contains("a chain of setups (a third setup) is not in the phase"), "{e}");
}

#[test]
fn test_the_earlier_air_is_turned_once_at_load_and_held() {
    // Ruling 2 of the Phase 48 CLOSE (slice 6): the earlier program's
    // turned removals are derived when `given from` loads and held in
    // `GivenFrom` beside the frozen earlier `Part` — a stored derived
    // value, asked and ruled. What is held IS the fresh derivation from
    // the held part (nothing can move either), and every reader reads the
    // held list rather than deriving again.
    let p = load("through_plate_back.cam");
    let gf = p.given_from.as_ref().expect("the back says given from");
    assert_eq!(stock::turn_program(&gf.part, gf.up, &gf.project), gf.air);
    let read = stock::given_from_air(&p).expect("read");
    assert!(std::ptr::eq(read, &gf.air), "the reader hands back the held list, not a copy");
    assert_eq!(read.removals.len(), 6);
    assert_eq!(read.ops_read, 6);
    assert!(read.declined.is_empty());
}

#[test]
fn test_a_form_s_table_turned_over_hangs_from_the_far_face() {
    // The corner rounder's fixture as the earlier program: its two Forms
    // declare `PathForm` floors — the real part's rib rounders, in
    // small. Turned over, the bore's rounded lip is a flare at this
    // program's FAR face: at 1.5 past the wall the quarter round (R3) is
    // 0.402 below the front's top, so 0.402 above this program's bottom.
    let round = fs::canonicalize("edge_round.cam").unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let back = scratch(
        "round_back_form",
        &format!("{HEAD}given from {round} up=z-\nstock Box(w=120, l=100, h=20, x=0, y=-100, z=20)\ntool T5 Endmill(dia=10.0)\n"),
    );
    let p = load(&back.to_string_lossy());
    let before = stock::remaining_before(&p, 0);
    assert!(before.complete);
    assert!(stock::given_from_declines(&p).is_empty());
    let text = outline::report(&p);
    assert!(text.contains("6 removal(s) from 6 of its 6 op(s), turned over (y → −y, z → −z) — air at prefix 0\n"), "{text}");
    // The bore (⌀40 at (30, 50), 12 deep): a hole from this program's far
    // face up to z 12, its wall at x 10 on the side the Post's moat does
    // not reach (the fixture's own moat, 44 from the Post's axis, clears
    // the bore's lip on the other side).
    assert!(!before.material_at(11.0, -50.0, 5.0), "in the bore");
    assert!(before.material_at(11.0, -50.0, 13.0), "over the bore's turned floor");
    // The round at the mouth: air under the flare, steel over it, steel
    // past its reach.
    assert!(!before.material_at(8.5, -50.0, 0.2), "under the turned quarter round");
    assert!(before.material_at(8.5, -50.0, 0.6), "over it");
    assert!(before.material_at(6.0, -50.0, 0.3), "past the round's 3 mm");
    // The field agrees with the model at the same places, a cell's margin
    // off the surfaces (at 0.5 past the wall the flare is 1.34 high), and
    // the report claims.
    let cell = Some(120.0 / 128.0);
    let field = vox::VoxField::build_prefix(&p, 0, cell).unwrap();
    assert!(!field.material_at(11.0, -50.0, 5.0));
    assert!(field.material_at(11.0, -50.0, 13.5));
    assert!(!field.material_at(9.5, -50.0, 0.4));
    assert!(field.material_at(9.5, -50.0, 2.3));
    let report = vox::rest_report(&p, cell).unwrap();
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(report.contains("forgotten (declared removed, never swept): none"), "{report}");
    assert!(report.contains("violated (declared part, swept anyway): none"), "{report}");
}

#[test]
fn test_a_dome_and_a_wire_s_drop_turned_over_are_declined_by_name() {
    // What ruling 1 left declined, by name: a `Dome` (the ball's sphere
    // finish — its floor, and the collar's Dome ceiling) and an `AlongWire`
    // (a wire boss followed by its Side). The model is incomplete and every
    // reader says so, as slices 1–4 did for every shape.
    let sphere = scratch(
        "sphere_front",
        &format!(
            "{HEAD}stock Box(w=100, l=80, h=47, x=0, y=0, z=22)\ntool T1 Endmill(dia=12.0)\ntool T9 Ballmill(dia=6.0)\n\
             feature TopDome Sphere(dia=40, x=50, y=40, z=0)\n\
             run Clear(tool=T1, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\nrun Copy(tool=T9, surface=TopDome, stepover=3.0)\n"
        ),
    );
    let back = scratch(
        "sphere_back",
        &format!("{HEAD}given from {} up=z-\nstock Box(w=100, l=80, h=47, x=0, y=-80, z=25)\ntool T1 Endmill(dia=12.0)\n", sphere.to_string_lossy()),
    );
    let p = load(&back.to_string_lossy());
    assert!(!stock::remaining_before(&p, 0).complete);
    let declines = stock::given_from_declines(&p);
    assert_eq!(declines.len(), 2, "{declines:?}");
    assert!(declines[0].contains("op [1] on 'TopDome' of"), "{declines:?}");
    assert!(declines[0].contains("a Dome floor turned over is the underside of a sphere"), "{declines:?}");
    assert!(declines[1].contains("a Dome ceiling turned over is the underside of a sphere"), "{declines:?}");
    let text = outline::report(&p);
    assert!(text.contains("1 removal(s) from 1 of its 2 op(s), turned over (y → −y, z → −z) — air at prefix 0; the model is INCOMPLETE for what follows"), "{text}");
    let report = vox::rest_report(&p, Some(100.0 / 32.0)).unwrap();
    assert!(report.contains("declared model: INCOMPLETE"), "{report}");
    assert!(report.contains("no claim:"), "{report}");

    let wire = fs::canonicalize("contour_boss.cam").unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_string();
    let back = scratch(
        "wire_back",
        &format!("{HEAD}given from {wire} up=z-\nstock Box(w=100, l=100, h=20, x=0, y=-100, z=20)\ntool T5 Endmill(dia=10.0)\n"),
    );
    let p = load(&back.to_string_lossy());
    let declines = stock::given_from_declines(&p);
    assert!(!declines.is_empty(), "{declines:?}");
    assert!(declines.iter().all(|d| d.contains("an AlongWire floor turned over is a rise above a wire")), "{declines:?}");
}

#[test]
fn test_the_earlier_program_s_own_givens_are_not_carried() {
    // The front says a bore is GIVEN — cut on another setup, which may well
    // be this one. Carrying it would claim air where this program is about
    // to cut steel; so the back reads the front's OPS, never its givens.
    let front = scratch(
        "givens_front",
        &format!(
            "{HEAD}stock Box(w=100, l=80, h=20, x=0, y=0, z=20)\ntool T5 Endmill(dia=8.0)\n\
             feature Told CylinderBore(dia=12.0, extent=20.0, x=15.0, y=15.0)\ngiven Told\n\
             feature Cut CylinderBore(dia=12.0, extent=20.0, x=50.0, y=15.0)\nrun Clear(tool=T5, profile=Cut, stepover=4.0)\n"
        ),
    );
    let back = scratch(
        "givens_back",
        &format!("{HEAD}given from {} up=z-\nstock Box(w=100, l=80, h=20, x=0, y=-80, z=0)\n", front.to_string_lossy()),
    );
    let p = load(&back.to_string_lossy());
    let before = stock::remaining_before(&p, 0);
    assert!(before.material_at(15.0, -15.0, -10.0), "what the front was only TOLD is steel here");
    assert!(!before.material_at(50.0, -15.0, -10.0), "what the front CUT is air here");
}

// ---- the certificate (Phase 48 slice 2): a restated void must lie in the
// earlier program's air

const BACK_HEAD: &str = "stock Box(w=90, l=50, h=12, x=-45, y=-25, z=0)\n\
tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n";

fn back_with(name: &str, lines: &str) -> PathBuf {
    scratch(name, &format!("{HEAD}given from {} up=z-\n{BACK_HEAD}{lines}", front_abs()))
}

#[test]
fn test_the_web_is_the_certificate_s_error() {
    // The front stopped Short a millimetre above the bottom; the back
    // restates it through. The DRAFT's premise 3, said by the engine: the
    // point in both frames, and the steel's depth to a tenth of a micron.
    let dir = back_with(
        "web",
        "feature Short CylinderBore(dia=14.0, extent=12.0, x=25.0, y=-6.0, chamfer_depth=1.0)\ngiven Short\nrun Form(tool=T3, surface=Short)\n",
    );
    let e = load_err(&dir.to_string_lossy());
    assert!(e.contains("given 'Short' is not air after"), "{e}");
    assert!(e.contains("turned z-: (31.960, -5.256, -0.001) here — (31.960, 5.256, 0.001) in its frame — stands"), "{e}");
    assert!(e.contains("steel from this face to 1.000 below it, where the earlier program's air begins"), "{e}");
    assert!(e.contains("Take it through there, or give less here"), "{e}");
}

#[test]
fn test_a_given_a_millimetre_off_is_the_certificate_s_error() {
    // Premise 4: the front's Left bore is at x −25; the back restates it at
    // −24. Phase 42 blessed this byte for byte; the certificate names the
    // rim point the earlier disc does not hold, through the whole depth.
    let dir = back_with(
        "stale",
        "feature Left CylinderBore(dia=14.0, extent=12.0, x=-24.0, y=-6.0, chamfer_depth=1.0)\ngiven Left\nrun Form(tool=T3, surface=Left)\n",
    );
    let e = load_err(&dir.to_string_lossy());
    assert!(e.contains("given 'Left' is not air after"), "{e}");
    assert!(e.contains("(-17.040, -5.256, -0.001) here"), "{e}");
    assert!(e.contains("steel through the whole 12.000 of the void — the earlier program cut nothing there"), "{e}");
}

#[test]
fn test_the_shipped_back_certifies_both_voids_and_advises_nothing() {
    let p = load("through_plate_back.cam");
    for name in ["Left", "Mid"] {
        assert_eq!(stock::certify_given(&p, name), Some(stock::Certificate::Certified { samples: 178 }), "{name}");
    }
    let text = outline::report(&p);
    assert!(text.contains("  certified: Left — 178 sample(s) in the earlier program's air"), "{text}");
    assert!(text.contains("  certified: Mid — 178 sample(s) in the earlier program's air"), "{text}");
    // Short reaches no face of the back (its web stands), so the reverse
    // advisory has nothing to name: the shipped program's stderr is empty.
    assert!(parse::lint(&p).is_empty(), "{:?}", parse::lint(&p));
}

#[test]
fn test_without_a_given_from_the_restatement_is_unread_as_before() {
    let p = load("tube_pocket.cam");
    assert_eq!(stock::certify_given(&p, "Bore"), None);
    assert!(stock::given_from_findings(&p).is_empty());
}

#[test]
fn test_the_reverse_advisory_names_each_hole_of_a_visit_to_points() {
    // A drill visiting points names no feature, so the advisory used to say
    // `on '' (?)` and, keyed on that empty name, spoke of the first hole only.
    let front = scratch(
        "points_front",
        &format!(
            "{HEAD}stock Box(w=90, l=50, h=12, x=-45, y=-25, z=12)\ntool T7 Drill(dia=8.0)\n\
             feature A CylinderBore(dia=8.0, extent=12.0, x=-25.0, y=6.0)\n\
             feature B CylinderBore(dia=8.0, extent=12.0, x=20.0, y=6.0)\n\
             run Form(points=[A,B], tool=T7, cycle=Simple)\n"
        ),
    );
    let back = scratch("points_back", &format!("{HEAD}given from {} up=z-\n{BACK_HEAD}", front.to_string_lossy()));
    let findings = parse::lint(&load(&back.to_string_lossy()));
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings[0].message.contains("op [0] at 'A' (CylinderBore) reaches this face at (-25.000, -6.000)"), "{}", findings[0].message);
    assert!(findings[1].message.contains("op [0] at 'B' (CylinderBore) reaches this face at (20.000, -6.000)"), "{}", findings[1].message);
}

#[test]
fn test_the_reverse_advisory_names_an_unnamed_through_bore_once() {
    // The back names nothing: the two through-bores reach this face and are
    // air to the stock model all the same, so this is advisory — once per
    // earlier feature, not per op (Left was cleared AND wall-finished).
    let dir = back_with("unnamed", "");
    let p = load(&dir.to_string_lossy());
    let findings = parse::lint(&p);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings.iter().all(|f| f.family == parse::FindingFamily::Containment));
    assert!(findings[0].message.contains("op [0] on 'Left' (CylinderBore) reaches this face at (-25.000, -6.000) and no `given` here names it"), "{}", findings[0].message);
    assert!(findings[1].message.contains("op [2] on 'Mid' (CylinderBore) reaches this face at (0.000, -6.000)"), "{}", findings[1].message);
    let place = findings[0].place.expect("the advisory carries its place");
    assert!((place[0] + 25.0).abs() < 1e-9 && (place[1] + 6.0).abs() < 1e-9 && place[2] == 0.0, "{place:?}");
    assert!(!findings.iter().any(|f| f.message.contains("Short")), "the short bore reaches no face here");
}

#[test]
fn test_an_incomplete_earlier_model_declines_the_certificate_never_passes_it() {
    // A sphere finished by a ball on the front: a `Dome`, which slice 5
    // still declines by name (a bowl's meridian now turns, so the bowl no
    // longer serves here).
    let front = scratch(
        "decline_front",
        &format!(
            "{HEAD}stock Box(w=100, l=80, h=47, x=0, y=0, z=22)\ntool T9 Ballmill(dia=6.0)\ntool T5 Endmill(dia=8.0)\n\
             feature TopDome Sphere(dia=40, x=50, y=40, z=0)\nfeature Hole CylinderBore(dia=12.0, extent=47.0, x=15.0, y=15.0)\n\
             run Copy(tool=T9, surface=TopDome, stepover=3.0)\nrun Clear(tool=T5, profile=Hole, stepover=4.0)\n"
        ),
    );
    let back = scratch(
        "decline_back",
        &format!(
            "{HEAD}given from {} up=z-\nstock Box(w=100, l=80, h=47, x=0, y=-80, z=25)\ntool T5 Endmill(dia=8.0)\n\
             feature Hole CylinderBore(dia=12.0, extent=47.0, x=15.0, y=-15.0)\ngiven Hole\n\
             feature Off CylinderBore(dia=12.0, extent=47.0, x=70.0, y=-15.0)\ngiven Off\n",
            front.to_string_lossy()
        ),
    );
    // Hole IS in the earlier air and Off is not — and neither is judged:
    // the earlier model is incomplete, so the program loads (a decline is
    // not an error) and the lint says so for each, by name.
    let p = load(&back.to_string_lossy());
    assert_eq!(stock::certify_given(&p, "Hole"), Some(stock::Certificate::Declined));
    assert_eq!(stock::certify_given(&p, "Off"), Some(stock::Certificate::Declined));
    let findings = parse::lint(&p);
    let declines: Vec<_> = findings.iter().filter(|f| f.family == parse::FindingFamily::Decline).collect();
    assert_eq!(declines.len(), 2, "{findings:?}");
    assert!(declines[0].message.contains("given 'Hole' not certified against"), "{}", declines[0].message);
    assert!(declines[0].message.contains("a Dome floor turned over is the underside of a sphere"), "{}", declines[0].message);
    let text = outline::report(&p);
    assert!(text.contains("  not certified: Off — the earlier program's model is incomplete"), "{text}");
}
