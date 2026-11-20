//! Phase 46 slice 3 — the chamfer's three words are PICK TWO (`DECISIONS.md`
//! → "DRAFT: Phase 46 opens…", fork 2 as ruled): one leg and the tool's
//! angle, one leg and a declared angle, or both legs with the included angle
//! derived from them; all three only where they agree. The derived angle
//! meets the check a declared one meets — it must be the cone the tool
//! grinds — so a print's "0.5 X 45 DEG" typed over a model's 0.48 at 44° is
//! refused by name until the reviewer makes the line say one thing.
use nacs::parse::{self, ChamferLeg, Feature};
use nacs::gcode;

const PREAMBLE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
    stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
    tool T5 Endmill(dia=10.0)\n\
    tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n\
    tool T6 ChamferMill(angle=60.0, gauge_dia=8.0, gauge_z=6.2, dia=12.0, tip_dia=1.0)\n";

fn with_pad(keys: &str, tool: &str) -> Result<parse::Part, String> {
    parse::parse(&format!(
        "{PREAMBLE}feature Pad ProfileBoss(extent=12.0, {keys}\n    20 20\n    20 60\n    60 60\n    60 20\n)\n\
         run Clear(tool=T5, profile=Pad, stepover=4.0, stepdown=4.0)\nrun Side(tool=T5, surface=Pad)\nrun Form(tool={tool}, surface=Pad)\n"
    ))
}

fn chamfer_of(p: &parse::Part) -> parse::ChamferSpec {
    match &p.features["Pad"] {
        Feature::ProfileBoss { chamfer, .. } => chamfer.expect("the pad declares a chamfer"),
        other => panic!("{other:?}"),
    }
}

/// Two legs derive the included angle: equal legs are 90°, and the leg kept
/// is the depth.
#[test]
fn test_two_legs_derive_the_included_angle() {
    let p = with_pad("chamfer_depth=1.0, chamfer_width=1.0", "T3").unwrap();
    let c = chamfer_of(&p);
    assert_eq!(c.leg, ChamferLeg::Depth(1.0));
    assert!((c.angle.unwrap() - 90.0).abs() < 1e-9, "{:?}", c.angle);
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(--- form chamfer") || g.contains("Pad with T3"), "the Form runs on the derived angle: {g}");

    // 1 deep by 0.577 wide is a 60° chamfer — the 60° tool cuts it, the 90° refuses.
    let w = (30.0f64).to_radians().tan();
    let p = with_pad(&format!("chamfer_depth=1.0, chamfer_width={w}"), "T6").unwrap();
    assert!((chamfer_of(&p).angle.unwrap() - 60.0).abs() < 1e-9);
    gcode::program(&p).unwrap();
    // (`parse` runs `validate`, which is where the Form's tool check lives.)
    let err = with_pad(&format!("chamfer_depth=1.0, chamfer_width={w}"), "T3").unwrap_err();
    assert!(err.contains("declares chamfer_angle=60.000deg but T3 grinds a 90.000deg cone"), "{err}");
}

/// Three words agree, or the line refuses naming all three.
#[test]
fn test_three_words_agree_or_refuse() {
    let p = with_pad("chamfer_depth=1.0, chamfer_width=1.0, chamfer_angle=90", "T3").unwrap();
    assert_eq!(chamfer_of(&p).leg, ChamferLeg::Depth(1.0));
    let err = with_pad("chamfer_depth=1.0, chamfer_width=1.0, chamfer_angle=60", "T3").unwrap_err();
    assert!(err.contains("chamfer_depth=1 and chamfer_width=1 imply a 90.000deg chamfer, but chamfer_angle=60"), "{err}");
    assert!(err.contains("drop the one the drawing does not say"), "{err}");
    let err = with_pad("chamfer_depth=0, chamfer_width=1.0", "T3").unwrap_err();
    assert!(err.contains("must be positive"), "{err}");
}

/// The one-leg forms are what they were: the tool's angle converts, a
/// declared angle is checked against the tool, an angle alone is refused.
#[test]
fn test_one_leg_forms_are_unchanged() {
    let p = with_pad("chamfer_width=0.8", "T3").unwrap();
    assert_eq!(chamfer_of(&p).leg, ChamferLeg::Width(0.8));
    assert_eq!(chamfer_of(&p).angle, None);
    gcode::program(&p).unwrap();
    let p = with_pad("chamfer_depth=1.0, chamfer_angle=90", "T3").unwrap();
    assert_eq!(chamfer_of(&p).angle, Some(90.0));
    let err = with_pad("chamfer_angle=90", "T3").unwrap_err();
    assert!(err.contains("chamfer_angle without chamfer_depth or chamfer_width"), "{err}");
}
