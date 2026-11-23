use crate::math::Portable as _;
use super::*;

#[test]
fn test_derives() {
    assert_eq!(derive_t(5), 5);
    assert_eq!(derive_h(5), 5);
    assert_eq!(derive_d(5), 5);
    assert_eq!(derive_z_cut(0.0, 0.5), -0.5);
}

#[test]
fn test_parse_machine_v1_fields() {
    // The v1 machine file (DECISIONS.md "K1, the machine…", fork 2A):
    // identity plus the spindle-word format, in the `.part`
    // global-directive shape with `#` comments.
    let m = parse_machine("# a comment\nname Fanuc-family VMC\nspindle_word integer\n").unwrap();
    assert_eq!(m.name.as_deref(), Some("Fanuc-family VMC"));
    assert_eq!(m.spindle_word, SpindleWord::Integer);

    // An empty machine file is a declared machine whose every field is
    // the undeclared default — legal, and byte-identical by construction.
    assert_eq!(parse_machine("").unwrap(), Machine::default());
    assert_eq!(Machine::default().spindle_word, SpindleWord::Decimal);
}

// ---- the unread-key silences (DECISIONS.md "the unread-key silences…"):
// stock, feature, and run lines keep the tool line's whitelist law ----

#[test]
fn the_run_line_refuses_what_it_does_not_know() {
    let src = |op_args: &str| {
        format!(
            "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
             stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n\
             tool T1 Endmill(dia=10.0)\n\
             feature Bore CylinderBore(dia=30.0, extent=10.0, x=30.0, y=30.0)\n\
             run Clear(tool=T1, profile=Bore, stepover=5.0{op_args})\n"
        )
    };
    // The hazard this thread exists for, pinned as a refusal: one letter
    // off `stock_to_leave` used to parse clean and cut to final size
    // with no allowance — the wall half a millimetre under where the
    // author believed, silently.
    let err = parse(&src(", stok_to_leave=0.5")).unwrap_err();
    assert!(
        err.contains("run Clear has no field named 'stok_to_leave'")
            && err.contains("quietly does not exist")
            && err.contains("stock_to_leave"),
        "the error names the typo and lists the real key: {err}"
    );
    parse(&src(", stock_to_leave=0.5")).expect("the real spelling still parses");
    // Multiple unknowns are all named, sorted.
    let err = parse(&src(", wat=1, aat=2")).unwrap_err();
    assert!(err.contains("'aat', 'wat'"), "{err}");
    // The pointed `depth` error outranks the generic one.
    let err = parse(&src(", depth=5")).unwrap_err();
    assert!(err.contains("not an operation input"), "{err}");
}

// ---- work offsets (DECISIONS.md "work offsets…", both slices) ----

/// One preamble, one knob: the directive under test.
fn offset_part(directive: &str, machine_line: &str) -> Result<Part, String> {
    parse(&format!(
        "{machine_line}units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
         stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n\
         tool T1 Endmill(dia=10.0)\n\
         feature Top Plane(z=-0.5)\n\
         {directive}run End(tool=T1, surface=Top, stepover=6.0)\n"
    ))
}

#[test]
fn a_work_offset_is_selected_never_set() {
    // The six standard registers are the dialect — no menu gates them.
    for n in 54..=59u8 {
        let p = offset_part(&format!("work_offset G{n}\n"), "").unwrap();
        assert_eq!(p.work_offset, Some(WorkOffset::Standard(n)));
        assert_eq!(p.work_offset.unwrap().words(), format!("G{n}"));
    }
    // The datum-SETTING spellings reject by name (tenet 5's teeth), and
    // G53 is machine coordinates, not a register to select.
    for reg in ["G92", "G10", "G52"] {
        let err = offset_part(&format!("work_offset {reg}\n"), "").unwrap_err();
        assert!(err.contains("SETS a datum") && err.contains("setup sheet"), "{reg}: {err}");
    }
    let err = offset_part("work_offset G53\n", "").unwrap_err();
    assert!(err.contains("MACHINE coordinate system"), "{err}");
    // Junk and duplicates reject.
    let err = offset_part("work_offset fixture2\n", "").unwrap_err();
    assert!(err.contains("is not a register"), "{err}");
    let err = offset_part("work_offset G55\nwork_offset G56\n", "").unwrap_err();
    assert!(err.contains("duplicate 'work_offset'"), "{err}");
    // Several registers on one line: the program runs at each.
    let p = offset_part("work_offset G54 G56\n", "").unwrap();
    assert_eq!((p.work_offset, p.also_at), (Some(WorkOffset::Standard(54)), vec![WorkOffset::Standard(56)]));
    let err = offset_part("work_offset G54 G55 G54\n", "").unwrap_err();
    assert!(err.contains("names G54 twice"), "{err}");
    let err = offset_part("work_offset G54 G92\n", "").unwrap_err();
    assert!(err.contains("SETS a datum"), "{err}");
    let err = offset_part("work_offset G54 G55\ndatum x=-500 y=-200\n", "").unwrap_err();
    assert!(err.contains("datum says where one register points"), "{err}");
}

#[test]
fn an_extended_register_is_sayable_exactly_when_its_machine_names_it() {
    std::fs::write("wo_ext_test.machine", "extended_offsets G54.1P 48\n").unwrap();
    // No machine: the error names the menu key that would license it.
    let err = offset_part("work_offset G54.1P7\n", "").unwrap_err();
    assert!(err.contains("extended_offsets G54.1P") && err.contains("G54–G59"), "{err}");
    // The declared family licenses it, and the emission spelling is the
    // vendor's two-word form.
    let p = offset_part("work_offset G54.1P7\n", "machine wo_ext_test.machine\n").unwrap();
    assert_eq!(
        p.work_offset.as_ref().unwrap().words(),
        "G54.1 P7",
        "the family splits at its trailing P"
    );
    // The wrong family and a register past the count reject, each
    // naming what the machine actually says.
    let err = offset_part("work_offset G154P7\n", "machine wo_ext_test.machine\n").unwrap_err();
    assert!(err.contains("spelled G54.1P"), "{err}");
    let err = offset_part("work_offset G54.1P49\n", "machine wo_ext_test.machine\n").unwrap_err();
    assert!(err.contains("registers 1–48"), "{err}");
    let err = offset_part("work_offset G54.1P0\n", "machine wo_ext_test.machine\n").unwrap_err();
    assert!(err.contains("registers 1–48"), "{err}");
    std::fs::remove_file("wo_ext_test.machine").unwrap();
}

#[test]
fn the_extended_offsets_machine_key_validates_its_shape() {
    let m = parse_machine("extended_offsets G154P 96\n").unwrap();
    assert_eq!(m.extended_offsets, Some(("G154P".to_string(), 96)));
    for bad in [
        "extended_offsets G54.1P\n",         // no count
        "extended_offsets G54.1P 48 extra\n", // trailing junk
        "extended_offsets 54P 48\n",          // no G
        "extended_offsets G54.1 48\n",        // no trailing P
        "extended_offsets G.5P 48\n",         // leading dot
        "extended_offsets G54.1P 0\n",        // zero registers
    ] {
        assert!(parse_machine(bad).is_err(), "'{bad}' must reject");
    }
}

#[test]
fn the_restart_ladder_refuses_ambiguity_and_nothing_else() {
    // (DECISIONS.md → "N-words…", fork 2A): positive whole numbers,
    // duplicates reject naming both ops, order deliberately free.
    let src = |n1: &str, n2: &str| {
        format!(
            "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
             stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n\
             tool T1 Endmill(dia=10.0)\n\
             feature Top Plane(z=-0.5)\n\
             feature Mid Plane(z=-1.0)\n\
             run End(tool=T1, surface=Top, stepover=6.0{n1})\n\
             run End(tool=T1, surface=Mid, stepover=6.0{n2})\n"
        )
    };
    let err = parse(&src(", n=10", ", n=10")).unwrap_err();
    assert!(
        err.contains("operation [0] on 'Top' and operation [1] on 'Mid' both carry n=10")
            && err.contains("ambiguous"),
        "got: {err}"
    );
    for bad in [", n=0", ", n=-5", ", n=ten", ", n=1.5"] {
        let err = parse(&src(bad, "")).unwrap_err();
        assert!(err.contains("positive whole number"), "'{bad}' got: {err}");
    }
    // A descending ladder is the author's ladder: inserting an op must
    // not force renumbering the tail, so order is not enforced.
    parse(&src(", n=30", ", n=10")).expect("order is the author's");
}

#[test]
fn the_stock_and_feature_lines_refuse_what_they_do_not_know() {
    let preamble = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n";
    let err = parse(&format!("{preamble}stock Box(w=60, l=60, h=20, wat=5)\n")).unwrap_err();
    assert!(err.contains("stock Box has no field named 'wat'") && err.contains("material"), "{err}");

    let src = |feat: &str| {
        format!("{preamble}stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n{feat}\n")
    };
    // A junk key on a feature, named with the variant's own set.
    let err = parse(&src("feature B CylinderBore(dia=30.0, extent=10.0, x=30.0, y=30.0, waat=1)")).unwrap_err();
    assert!(
        err.contains("feature 'B' (CylinderBore) has no field named 'waat'") && err.contains("cbore_depth"),
        "{err}"
    );
    // The optional-key killer class: a misspelled edge_round used to
    // ship the feature without its declared break.
    let err = parse(&src("feature B CylinderBore(dia=30.0, extent=10.0, x=30.0, y=30.0, edge_rond=1.0)")).unwrap_err();
    assert!(err.contains("'edge_rond'"), "{err}");
    // `z` on a bore is its RIM since Phase 45 (DECISIONS.md → "DRAFT:
    // Phase 45 opens…"). This exact key was found riding two of this
    // file's own test programs when the whitelist landed and was refused
    // as junk for a phase — the authors reaching for a key the grammar
    // did not yet have. `z=0` on a stock whose top is 0 IS the stock
    // top: the same bore, said twice.
    let p = parse(&src("feature B CylinderBore(dia=30.0, extent=10.0, x=30.0, y=30.0, z=0)")).unwrap();
    match &p.features["B"] {
        Feature::CylinderBore { z, .. } => assert_eq!(derive_contour_top(p.stock.z, *z), p.stock.z),
        other => panic!("expected a bore, got {other:?}"),
    }
    // The retired spellings keep their pointed migration messages…
    let err = parse(&src("feature B CylinderBore(dia=30.0, depth=10.0, x=30.0, y=30.0)")).unwrap_err();
    assert!(err.contains("is now `extent`"), "{err}");
    // …and both spellings at once is two records for one fact.
    let err = parse(&src("feature B CylinderBore(dia=30.0, extent=10.0, depth=10.0, x=30.0, y=30.0)")).unwrap_err();
    assert!(err.contains("carries both `extent` and the retired `depth`"), "{err}");
    // An unknown VARIANT still rejects by its own name, not a key list.
    let err = parse(&src("feature B Cylinder(dia=30.0)")).unwrap_err();
    assert!(err.contains("unknown feature type"), "{err}");
    // The block-form pairs are under the same law.
    let err = parse(&src("feature P ProfilePocket(extent=5.0, wat=1\n    5 5\n    5 25\n    25 25\n    25 5\n)")).unwrap_err();
    assert!(err.contains("feature 'P' (ProfilePocket) has no field named 'wat'"), "{err}");
    // And the wire pair's pointed z error still outranks the generic.
    let err = parse(&src("feature W ContourPocket(extent=5.0, z=1\n    5 5 0\n    5 25 0\n    25 25 -1\n    25 5 0\n)")).unwrap_err();
    assert!(err.contains("takes no z datum"), "{err}");
}

#[test]
fn test_parse_machine_unknown_key_refuses_with_its_line() {
    // Fork 3B: named directives, unknowns rejected — an open dictionary of
    // dialect facts would be a post config wearing a new extension.
    // (`coolant` BARE stays unknown even now that the menu exists: the
    // menu's keys are coolant_<kind>, and the error below names them.)
    let err = parse_machine("name X\ncoolant flood\n").unwrap_err();
    assert!(err.contains("line 2") && err.contains("unknown machine key 'coolant'"), "got: {err}");
}

// ---- the coolant menu (DECISIONS.md "coolant as a modeled setting…") ----

#[test]
fn the_coolant_menu_parses_augments_and_refuses_by_name() {
    // Declared keys land in their slots; absent kinds fall back to the
    // undeclared dialect — flood and off always have words, the rest are
    // unsayable until the machine names them (custom slots included:
    // menu-only by construction, the blessing's amendment).
    let m = parse_machine("coolant_mist M07\ncoolant_custom1 M88\n").unwrap();
    assert_eq!(m.coolant.word(CoolantKind::Mist), Some("M07"));
    assert_eq!(m.coolant.word(CoolantKind::Custom1), Some("M88"));
    assert_eq!(m.coolant.word(CoolantKind::Flood), Some("M08"), "the undeclared dialect survives beside the menu");
    assert_eq!(m.coolant.word(CoolantKind::Off), Some("M09"));
    assert_eq!(m.coolant.word(CoolantKind::Air), None, "an unnamed kind stays unsayable");
    assert_eq!(m.coolant.word(CoolantKind::Custom2), None);
    // A declared word may respell even the defaults (a controller
    // wanting `M8` over `M08` is a spelling, and the menu owns spelling).
    let m = parse_machine("coolant_flood M8\n").unwrap();
    assert_eq!(m.coolant.word(CoolantKind::Flood), Some("M8"));

    // The errors, each with its line: a key suffix nobody reads, and a
    // value that does not spell an M-word.
    let err = parse_machine("coolant_wat M07\n").unwrap_err();
    assert!(err.contains("line 1") && err.contains("coolant_wat") && err.contains("custom3"), "got: {err}");
    for bad in ["coolant_mist wet\n", "coolant_mist M\n", "coolant_mist 7\n", "coolant_mist M0 7\n"] {
        let err = parse_machine(bad).unwrap_err();
        assert!(err.contains("not an M-word"), "'{bad}' got: {err}");
    }
}

#[test]
fn coolant_resolution_is_op_then_tool_then_flood() {
    let src = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n\
        tool T1 Endmill(dia=10.0, coolant=off)\n\
        feature Top Plane(z=-0.5)\n\
        run End(tool=T1, surface=Top, stepover=6.0)\n\
        run End(tool=T1, surface=Top, stepover=6.0, coolant=flood)\n";
    let p = parse(src).unwrap();
    assert_eq!(coolant_for(&p, &p.operations[0]), CoolantKind::Off, "the tool's fallback");
    assert_eq!(coolant_for(&p, &p.operations[1]), CoolantKind::Flood, "the op's override wins");
    // And the last arm: nothing declared anywhere resolves to flood —
    // today's behavior, said in exactly one place.
    let bare = src.replace(", coolant=off", "").replace(", coolant=flood", "");
    let p = parse(&bare).unwrap();
    assert_eq!(coolant_for(&p, &p.operations[0]), CoolantKind::Flood);
}

#[test]
fn an_unsayable_kind_refuses_naming_the_menu_key() {
    // The machine-config gap, converted into a pointer at the K1 file:
    // mist with no machine rejects at validate through the same
    // `coolant_word` home the emitter spells through. The kind resolves
    // per op — a tool-level mist overridden away on every op is not a
    // error, because nothing would ever ask the menu for it.
    let src = |tool_coolant: &str, op_coolant: &str| {
        format!(
            "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
             stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n\
             tool T1 Endmill(dia=10.0{tool_coolant})\n\
             feature Top Plane(z=-0.5)\n\
             run End(tool=T1, surface=Top, stepover=6.0{op_coolant})\n"
        )
    };
    let err = parse(&src(", coolant=mist", "")).unwrap_err();
    assert!(
        err.contains("operation [0] on 'Top'")
            && err.contains("coolant 'mist' has no word")
            && err.contains("coolant_mist"),
        "got: {err}"
    );
    let err = parse(&src("", ", coolant=custom2")).unwrap_err();
    assert!(err.contains("coolant_custom2"), "the custom slots reject the same way: {err}");
    parse(&src(", coolant=mist", ", coolant=flood")).expect("an overridden fallback is never asked of the menu");
    // And the vocabulary error itself, both homes, naming the kinds.
    let err = parse(&src(", coolant=wet", "")).unwrap_err();
    assert!(err.contains("tool T1 coolant 'wet' is not a kind") && err.contains("custom1"), "got: {err}");
    let err = parse(&src("", ", coolant=wet")).unwrap_err();
    assert!(err.contains("coolant 'wet' is not a kind"), "got: {err}");
}

#[test]
fn test_parse_machine_unknown_spindle_word_refuses() {
    let err = parse_machine("spindle_word float\n").unwrap_err();
    assert!(err.contains("line 1") && err.contains("unknown spindle_word 'float'"), "got: {err}");
    assert!(err.contains("'integer'") && err.contains("'decimal'"), "the error names the closed set: {err}");
}

#[test]
fn test_parse_machine_output_format_fields() {
    // K1 slice 1 (DECISIONS.md "K1 slice 1 — the formatting boundary…"):
    // `units` plus per-class decimals. Undeclared decimals resolve by
    // units — mm keeps today's 3, inch defaults to 4 (tenths of a thou)
    // — and a declared class overrides only itself.
    let m = parse_machine("units inch\n").unwrap();
    assert_eq!(m.units, OutputUnits::Inch);
    assert_eq!((m.coord_decimals, m.feed_decimals), (4, 4));
    assert!(!m.output_format_is_default());

    let m = parse_machine("units inch\nfeed_decimals 2\n").unwrap();
    assert_eq!((m.coord_decimals, m.feed_decimals), (4, 2));

    // Line order is not a fact: decimals declared before units are
    // judged under the file's final units, not the default's.
    let m = parse_machine("coord_decimals 4\nunits inch\n").unwrap();
    assert_eq!(m.coord_decimals, 4);

    // A declared machine that states the defaults is the default —
    // the formatting boundary's skip-by-construction case.
    let m = parse_machine("units mm\ncoord_decimals 3\nfeed_decimals 3\n").unwrap();
    assert!(m.output_format_is_default());
    assert!(Machine::default().output_format_is_default());
}

#[test]
fn test_parse_machine_unknown_units_refuses() {
    let err = parse_machine("units furlong\n").unwrap_err();
    assert!(err.contains("line 1") && err.contains("unknown units 'furlong'"), "got: {err}");
    assert!(err.contains("'mm'") && err.contains("'inch'"), "the error names the closed set: {err}");
}

#[test]
fn test_parse_machine_absurd_decimals_refuse() {
    // The absurdity band: 0 decimals cannot carry the model's
    // micrometers, and more than 6 outresolves any controller word.
    let err = parse_machine("coord_decimals 0\n").unwrap_err();
    assert!(err.contains("line 1") && err.contains("outside 1..=6"), "got: {err}");
    let err = parse_machine("units inch\nfeed_decimals 7\n").unwrap_err();
    assert!(err.contains("line 2") && err.contains("outside 1..=6"), "got: {err}");
    let err = parse_machine("coord_decimals many\n").unwrap_err();
    assert!(err.contains("whole number"), "got: {err}");
}

#[test]
fn test_parse_machine_decimals_beyond_the_internal_grid_refuse() {
    // The honesty ceiling, both units: the op emitters spell mm at 3
    // decimals, so mm output carries at most 3 and inch at most 4 —
    // finer declarations would print quantization noise dressed as
    // position, and reject naming the remainder instead.
    let err = parse_machine("coord_decimals 4\n").unwrap_err();
    assert!(err.contains("line 1") && err.contains("outresolves the internal dialect"), "got: {err}");
    assert!(err.contains("at most 3 mm"), "the error states the mm ceiling: {err}");
    let err = parse_machine("units inch\ncoord_decimals 5\n").unwrap_err();
    assert!(err.contains("line 2") && err.contains("at most 4 inch"), "the error states the inch ceiling: {err}");
    assert!(err.contains("Extended precision"), "the error names the ROADMAP remainder: {err}");
}

#[test]
fn test_machine_directive_missing_file_is_a_hard_error() {
    // An author who declared a machine meant it: a missing file rejects
    // with the referencing line, never a silent fall-through to the
    // undeclared machine.
    let mut p = Part::default();
    let err = parse_into("machine no_such.machine\n", &mut p, None).unwrap_err();
    assert!(err.contains("line 1") && err.contains("failed to read machine file 'no_such.machine'"), "got: {err}");
    assert!(p.machine.is_none(), "no partial machine survives a rejection");
}

#[test]
fn test_machine_directive_second_line_refuses() {
    // A program runs on exactly ONE machine (fork 1B).
    let mut p = Part::default();
    let src = "machine cylinder_bore_machine.cam/fanuc.machine\nmachine cylinder_bore_machine.cam/fanuc.machine\n";
    let err = parse_into(src, &mut p, None).unwrap_err();
    assert!(err.contains("line 2") && err.contains("exactly one machine"), "got: {err}");
}

#[test]
fn test_spline_revolve_step_flat_reduces_to_cone_chord() {
    // A degree-1 (straight-line) meridian has zero curvature everywhere, so
    // the march's flat band must produce exactly the cone's planar cusp
    // chord 2·sqrt(2·r_ball·h − h²) along the (parallel-line) CL locus —
    // pinning the spline march's flat limit to finish_cone's closed form.
    let meridian = crate::nurbs::BSplineCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![(0.0, 10.0), (10.0, 0.0)],
        None,
    )
    .unwrap();
    let (r_ball, scallop) = (3.0, 0.2);
    let u = 0.1;
    let u_next = derive_spline_revolve_step_u(scallop, r_ball, &meridian, u, 0.0);
    let c1 = spline_revolve_cl_point(&meridian, r_ball, u);
    let c2 = spline_revolve_cl_point(&meridian, r_ball, u_next);
    let chord = ((c2.0 - c1.0).powi(2) + (c2.1 - c1.1).powi(2)).sqrt();
    let flat = 2.0 * (2.0 * r_ball * scallop - scallop * scallop).sqrt();
    assert!((chord - flat).abs() < 1e-6, "flat-meridian CL chord {chord} must equal the planar cusp chord {flat}");
}

    // The two projections the convex R2 cap rests on (DECISIONS.md "Convex R2
    // march cap…"), checked by their defining property: a point pushed d along
    // the outward normal from a known contact point must read back exactly d.
    // Both must also CLAMP at the machined ends — past the rim the nearest
    // curve point is the rim itself (the cone's corner-fan convention), which
    // is what keeps a cusp tip beyond the last pass from projecting onto
    // curve the part does not have.

    #[test]
    fn test_scallop_angle_concave_mirrors_convex() {
        // Same cusp height, same ball, same local radius: the concave CL circle
        // (r_part - r_ball) is tighter than the convex one (r_part + r_ball),
        // so covering the same cusp costs a larger central angle.
        let (scallop, r_ball, r_part) = (0.5, 3.0, 40.0);
        let convex = derive_scallop_angle(scallop, r_ball, r_part);
        let concave = derive_scallop_angle_concave(scallop, r_ball, r_part);
        assert!(convex > 0.0 && concave > 0.0);
    assert!(concave > convex, "concave angle {} should exceed convex {}", concave, convex);
}

#[test]
fn test_parse_valid() {
    let input = "\
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=120, l=80, h=20, x=0, y=0, z=0)
tool T5 Endmill(dia=10.0)
feature TopFace Plane(z=-0.5)
feature Profile1 ProfilePocket(extent=0.5
    0 0
    120 0
    120 80
)
run End(tool=T5, surface=TopFace, stepover=6.0)
run Side(tool=T5, surface=Profile1)
run Clear(tool=T5, profile=Profile1, stepover=4.0)
";
    let part = parse(input).unwrap();
    assert!(part.units_mm);
    assert_eq!(part.stock.w, 120.0);
    assert_eq!(part.stock.l, 80.0);
    assert_eq!(part.tools.get(&5).unwrap().dia, 10.0);
    assert_eq!(part.operations.len(), 3);
    assert_eq!(part.operations[2].op_type, OpType::Clear);
    assert_eq!(part.operations[2].stepover, Some(4.0));
}

#[test]
fn test_inch_input_converts_at_parse() {
    let input = "\
units inch
feed 20
plunge 5
rapid_z 0.25
clearance_z 1
spindle 3000
stock Box(w=4, l=2, h=1)
tool T5 Endmill(dia=0.5)
feature TopFace Plane(z=-0.05)
run End(tool=T5, surface=TopFace, stepover=0.25)
";
        let part = parse(input).unwrap();
        // Part is always mm after parse_project; the flag reflects that.
        assert!(part.units_mm);
        assert_eq!(part.stock.w, 4.0 * 25.4);
        assert_eq!(part.tools.get(&5).unwrap().dia, 0.5 * 25.4);
        assert_eq!(part.global_feed, 20.0 * 25.4);
    // Vertical occupancy is the feature's now, so the inch->mm boundary has
    // to catch it there: the faced plane's own z, not an op input.
    assert_eq!(part.features.get("TopFace"), Some(&Feature::Plane { z: -0.05 * 25.4 }));
    assert_eq!(part.operations[0].stepover, Some(0.25 * 25.4));
    // RPM is not a length: spindle must not be scaled.
    assert_eq!(part.global_spindle, 3000.0);
}

/// Live run (2026-08-30): a file with two `tool T5` lines loaded, the
/// later one silently shadowing the first in the number-keyed map —
/// exactly the "declaration that quietly does not exist" the unknown-key
/// refusal exists to prevent. One number names one tool.
#[test]
    fn test_duplicate_tool_number_refuses_by_name() {
        let input = "\
feed 500
plunge 100
rapid_z 5
clearance_z 10
spindle 8000
stock Box(w=60, l=40, h=20)
tool T5 Endmill(dia=6)
tool T5 Endmill(dia=10)
feature TopFace Plane(z=0)
run End(tool=T5, surface=TopFace, stepover=4)
";
        let e = parse(input).unwrap_err();
        assert!(e.contains("tool T5 is already declared"), "{e}");
        assert!(e.starts_with("line 8"), "{e}");
    }

    #[test]
    fn test_unknown_units_is_an_error() {
    assert!(parse("units furlong\n").is_err());
}

#[test]
fn test_parse_invalid() {
    let input_no_stock = "\
units mm
tool T5 Endmill(dia=10.0)
";
    assert!(parse(input_no_stock).is_err());
}

    #[test]
    fn test_parse_quadrics() {
        let input = "\
units mm
feed 600
plunge 200
rapid_z 5
clearance_z 30
spindle 4000
stock Box(w=100, l=80, h=25)
tool T9 Ballmill(dia=6.0)
feature Dome Sphere(dia=40, x=50, y=40, z=0)
feature Point Cone(dia=30, height=15, x=10, y=10, z=2)
feature Ring Torus(major_dia=50, minor_dia=8, x=20, y=20, z=1)
run Copy(tool=T9, surface=Dome, stepover=3.0)
";
    let part = parse(input).unwrap();
    assert_eq!(part.features.get("Dome"), Some(&Feature::Sphere { dia: 40.0, x: 50.0, y: 40.0, z: 0.0 }));
    assert_eq!(part.features.get("Point"), Some(&Feature::Cone { dia: 30.0, height: 15.0, x: 10.0, y: 10.0, z: 2.0 }));
    assert_eq!(part.features.get("Ring"), Some(&Feature::Torus { major_dia: 50.0, minor_dia: 8.0, x: 20.0, y: 20.0, z: 1.0 }));
    assert_eq!(part.operations[0].op_type, OpType::Copy);
        assert_eq!(part.operations[0].stepover, Some(3.0));
    }

    #[test]
    fn test_sphere_converts_at_parse() {
        // Every Sphere field is a length, so inch input scales at the boundary.
        let input = "\
units inch
feed 20
plunge 5
rapid_z 0.25
clearance_z 1.5
spindle 4000
stock Box(w=4, l=3, h=1)
tool T9 Ballmill(dia=0.25)
feature Dome Sphere(dia=1.5, x=2, y=1.5, z=0)
run Copy(tool=T9, surface=Dome, stepover=0.1)
";
    let part = parse(input).unwrap();
    assert_eq!(
        part.features.get("Dome"),
        Some(&Feature::Sphere { dia: 1.5 * 25.4, x: 2.0 * 25.4, y: 1.5 * 25.4, z: 0.0 })
    );
    assert_eq!(part.operations[0].stepover, Some(0.1 * 25.4));
}

#[test]
fn test_parse_drill() {
    let input = "\
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=120, l=80, h=20, x=0, y=0, z=0)
tool T7 Drill(dia=8.0)
feature Hole1 CylinderBore(dia=10.0, extent=15.0, x=30.0, y=40.0)
feature Hole2 CylinderBore(dia=8.0, extent=15.0, x=50.0, y=40.0)
run Form(points=[Hole1,Hole2], tool=T7, cycle=Peck, peck_increment=3.0)
";
    let part = parse(input).unwrap();
    assert_eq!(part.operations.len(), 1);
    let op = &part.operations[0];
    assert_eq!(op.op_type, OpType::Form);
    assert_eq!(op.drill_points, Some(vec!["Hole1".to_string(), "Hole2".to_string()]));
    assert_eq!(op.cycle, Some(CycleType::Peck));
    assert_eq!(op.peck_increment, Some(3.0));
}

// ---- the corner rounder, slice 0: the tool ------------------------------
// DECISIONS.md → "the corner rounder gets an operation". The declaration is
// the ground arc; pilot and body OD are clearance and never gauged from.

const ROUNDER_PREAMBLE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\nstock Box(w=120, l=80, h=20, x=0, y=0, z=0)\n";

fn rounder_part(decl: &str) -> Result<Part, String> {
    parse(&format!("{ROUNDER_PREAMBLE}{decl}\n"))
}

// A nominal 3mm rounder: arc centre at rc = 3 + 3 = 6, zc = 6 - 3 = 3.
// Pilot 5.94 is ground 0.03 under the 6.00 cylinder the arc is tangent to.
const T2: &str = "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94)";

#[test]
fn the_ground_arc_derives_its_centre_and_never_shows_the_pilot() {
    let part = rounder_part(T2).unwrap();
    let tool = part.tools.get(&2).unwrap();
    let arc = tool.ground_arc().expect("a CornerRounder hands over its arc");

    // rc = effective_radius + R, zc = gauge_z - R. Both derived, so the
    // two declared numbers are the ones that can be checked at a presetter.
    assert!((arc.rc - 6.0).abs() < 1e-12, "rc = {}", arc.rc);
    assert!((arc.zc - 3.0).abs() < 1e-12, "zc = {}", arc.zc);

    // The two virtual tangencies round-trip to what was declared.
    assert!((arc.effective_radius() - 3.0).abs() < 1e-12);
    assert!((arc.gauge_z() - 6.0).abs() < 1e-12);

    // The virtual sharp corner IS the two declared numbers read as a point.
    let vsc = arc.virtual_sharp_corner();
    assert!((vsc.x - 3.0).abs() < 1e-12 && (vsc.y - 6.0).abs() < 1e-12);

    // The arc's own ends are tangent to those two virtual surfaces: phi=0
    // is vertical tangent on the wall cylinder, phi=90 horizontal on the
    // face plane.
    let (lo, hi) = (arc.at(0.0), arc.at(90.0));
    assert!((lo.x - arc.effective_radius()).abs() < 1e-12 && (lo.y - arc.zc).abs() < 1e-12);
    assert!((hi.x - arc.rc).abs() < 1e-12 && (hi.y - arc.gauge_z()).abs() < 1e-12);
}

#[test]
fn the_nominal_identity_holds_on_paper_and_the_relief_is_the_gap() {
    let part = rounder_part(T2).unwrap();
    let rd = part.tools.get(&2).unwrap().rounder.unwrap();
    // The catalogue identity: rc - R equals the pilot RADIUS exactly...
    assert!((rd.effective_radius() - 6.0 / 2.0).abs() < 1e-12);
    // ...except the ground pilot is under it, and that difference is the
    // whole reason `pilot` is declared. Comping off it would drive the
    // arc's wall-tangent point 0.03mm into the finished wall.
    assert!((rd.relief() - 0.03).abs() < 1e-12, "relief = {}", rd.relief());
    assert!(rd.pilot / 2.0 < rd.effective_radius());
}

#[test]
fn trail_off_moves_the_cutting_edge_and_prices_the_step() {
    let decl = "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, trail_wall=4.0, trail_face=1.5, source=measured)";
    let part = rounder_part(decl).unwrap();
    let tool = part.tools.get(&2).unwrap();
    let rd = tool.rounder.unwrap();
    let arc = tool.ground_arc().unwrap();
    assert!(rd.measured);

    // R(1 - cos theta) at each end, and it is exactly the radial gap
    // between the virtual tangent point and where the arc really starts.
    let expect_wall = 3.0 * (1.0 - 4.0f64.to_radians().pcos());
    assert!((rd.wall_step() - expect_wall).abs() < 1e-12);
    assert!((arc.at(4.0).x - arc.effective_radius() - expect_wall).abs() < 1e-12);

    // The face end runs out BELOW the gauge plane by its own step, which is
    // why the cutting top and the emitted datum are different heights.
    let expect_face = 3.0 * (1.0 - 1.5f64.to_radians().pcos());
    assert!((rd.face_step() - expect_face).abs() < 1e-12);
    assert!((arc.gauge_z() - arc.cutting_top() - expect_face).abs() < 1e-12);
}

#[test]
fn a_measured_radius_resizes_the_arc_without_renaming_the_tool() {
    let decl = "tool T2 CornerRounder(radius=3.0, effective_dia=6.01, gauge_z=5.99, dia=12.0, pilot=5.94, radius_error=-0.02, source=measured)";
    let part = rounder_part(decl).unwrap();
    let tool = part.tools.get(&2).unwrap();
    let rd = tool.rounder.unwrap();
    let arc = tool.ground_arc().unwrap();

    // The identity stays nominal — this is still a 3mm rounder, which is
    // what lets the pairing error against `edge_round` stay exact.
    assert_eq!(rd.radius, 3.0);
    // The arc that cuts is the measured one, and the centre follows it.
    assert!((arc.r - 2.98).abs() < 1e-12);
    assert!((arc.rc - (6.01 / 2.0 + 2.98)).abs() < 1e-12);
    assert!((arc.zc - (5.99 - 2.98)).abs() < 1e-12);
    // The declared setup numbers still round-trip: they are ACTUAL, so a
    // radius error must not move them.
    assert!((arc.effective_radius() - 6.01 / 2.0).abs() < 1e-12);
    assert!((arc.gauge_z() - 5.99).abs() < 1e-12);
}

#[test]
fn the_silhouette_is_the_body_and_the_datum_is_the_virtual_sharp_corner() {
    let part = rounder_part(T2).unwrap();
    let tool = part.tools.get(&2).unwrap();
    let sil = Silhouette::of(tool);

    // Emitted coordinates are the virtual sharp corner, so the tip hangs a
    // whole gauge_z below them — the Ball rule with the centre off-axis.
    assert!((sil.drop() - 6.0).abs() < 1e-12);
    // Cutting stops at the top of the arc; the shoulder above removes nothing.
    assert_eq!(sil.cutting_top(), Some(6.0));
    assert!(!sil.is_rod());

    // The body: relieved pilot below the arc, the arc through its span, the
    // shoulder at the OD above it.
    assert!((sil.radius_at(0.0) - 5.94 / 2.0).abs() < 1e-12);
    assert!((sil.radius_at(1.5) - 5.94 / 2.0).abs() < 1e-12);
    assert!((sil.radius_at(9.0) - 6.0).abs() < 1e-12, "the shank is the body OD");
    assert!((sil.max_radius() - 6.0).abs() < 1e-12);

    // The entry's split, measured: the body is the TESSELLATED half, and
    // the metric that matters is the one `meridian_d` uses — perpendicular
    // distance, not radius-at-height (which diverges near the horizontal
    // tangent for any chording at all, and is not what the stamp asks).
    let arc = tool.ground_arc().unwrap();
    let (pts, _) = sil.body_pts().unwrap();
    let on_arc: Vec<Pt> = pts
        .iter()
        .copied()
        .filter(|p| p.y >= arc.zc - 1e-9 && p.x >= arc.effective_radius() - 1e-9 && p.x <= arc.rc + 1e-9)
        .collect();
    assert!(on_arc.len() >= 4, "the arc should chord into several segments, got {}", on_arc.len());
    let radial = |p: &Pt| ((p.x - arc.rc).powi(2) + (p.y - arc.zc).powi(2)).sqrt();
    for p in &on_arc {
        assert!((radial(p) - arc.r).abs() < 1e-12, "vertex {p:?} is off the exact arc");
    }
    // Every chord's midpoint sits inside the arc by its sagitta, and the
    // sagitta is the tessellation error. It bounds under CHORD_TOL_MM, which
    // is well under any voxel cell the field is ever built at.
    for pair in on_arc.windows(2) {
        let mid = Pt { x: (pair[0].x + pair[1].x) / 2.0, y: (pair[0].y + pair[1].y) / 2.0 };
        let sagitta = arc.r - radial(&mid);
        assert!(
            sagitta >= 0.0 && sagitta <= crate::CHORD_TOL_MM,
            "chord {pair:?} deviates {sagitta}"
        );
    }
    // And the chords sit OUTSIDE the true solid, never inside it: the body
    // may over-state itself (a foul lint that over-warns is the safe
    // direction) but must never under-state.
    for i in 0..=40 {
        let phi = 90.0 * (i as f64) / 40.0;
        let exact = arc.at(phi);
        assert!(
            sil.radius_at(exact.y) >= exact.x - 1e-12,
            "at phi={phi} the body under-states the solid"
        );
    }
}

#[test]
fn a_relieved_shoulder_is_clearance_and_never_reads_as_an_undercut() {
    // Body OD ground back INSIDE the arc's outer extent (rc = 6.0): a
    // narrowing in the solid, and pure clearance in the cut. If this ever
    // answers Some the tool stops being stampable, the field goes
    // incomplete, and strategy=rest rejects — for a surface that removes
    // nothing.
    let decl = "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=11.0, pilot=5.94)";
    let part = rounder_part(decl).unwrap();
    let sil = Silhouette::of(part.tools.get(&2).unwrap());
    assert!((sil.radius_at(9.0) - 5.5).abs() < 1e-12, "the shoulder really is inboard of the arc");
    assert_eq!(sil.narrowing(), None);
}

// ---- K2, the tool body — slice 0 (DECISIONS.md "K2, the tool body") ----

fn body_part(decl: &str) -> Result<Part, String> {
    parse(&format!("{ROUNDER_PREAMBLE}{decl}\n"))
}

#[test]
fn the_tool_line_refuses_a_field_it_does_not_know() {
    // The blessed behavior change, measured in the design entry: these
    // exact keys used to parse clean and emit a full program, the declared
    // body quietly not existing. `shank_dia`/`stickout` are known NOW, so
    // the misspelling is what must reject.
    let err = body_part("tool T5 Endmill(dia=10.0, shankdia=12.0)").unwrap_err();
    assert!(err.contains("line 8") && err.contains("no field named 'shankdia'"), "got: {err}");
    assert!(err.contains("flute_len"), "the error lists the known fields: {err}");
    // Multiple unknowns are all named, deterministically ordered.
    let err = body_part("tool T5 Endmill(dia=10.0, zz=1, aa=2)").unwrap_err();
    assert!(err.contains("'aa', 'zz'"), "got: {err}");
    // Wrong-FAMILY keys keep their better, family-naming error.
    let err = body_part("tool T5 Endmill(dia=10.0, pilot=5.0)").unwrap_err();
    assert!(err.contains("corner-rounder geometry"), "got: {err}");
}

#[test]
fn the_body_fields_parse_and_land_on_the_tool() {
    let part = body_part(
        "tool T5 Ballmill(dia=6.0, flute_len=13.0, shank_dia=10.0, stickout=35.0, flute_count=4)",
    )
    .unwrap();
    let tool = part.tools.get(&5).unwrap();
    assert_eq!(tool.flute_len, Some(13.0));
    assert_eq!(tool.shank_dia, Some(10.0));
    assert_eq!(tool.stickout, Some(35.0));
    assert_eq!(tool.flute_count, Some(4));
}

#[test]
fn inch_programs_scale_the_body_fields_but_not_the_count() {
    let src = "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\n\
stock Box(w=4, l=3, h=1, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=0.25, flute_len=0.75, shank_dia=0.25, stickout=1.5, flute_count=3)\n";
    let part = parse(src).unwrap();
    let tool = part.tools.get(&5).unwrap();
    assert!((tool.flute_len.unwrap() - 0.75 * 25.4).abs() < 1e-9);
    assert!((tool.shank_dia.unwrap() - 0.25 * 25.4).abs() < 1e-9);
    assert!((tool.stickout.unwrap() - 1.5 * 25.4).abs() < 1e-9);
    assert_eq!(tool.flute_count, Some(3), "a count is not a length");
}

#[test]
fn the_body_claim_checks_refuse_at_parse() {
    // flute_len ≤ stickout: the flutes cannot run past the holder's face.
    let err = body_part("tool T5 Endmill(dia=6.0, flute_len=40.0, stickout=35.0)").unwrap_err();
    assert!(err.contains("exceeds stickout"), "got: {err}");
    // A shank with no bottom: on a rod only flute_len places it.
    let err = body_part("tool T5 Endmill(dia=6.0, shank_dia=10.0)").unwrap_err();
    assert!(err.contains("without flute_len"), "got: {err}");
    // A ball's flutes cannot end inside the ball itself.
    let err = body_part("tool T5 Ballmill(dia=6.0, flute_len=2.0)").unwrap_err();
    assert!(err.contains("shorter than the ball radius"), "got: {err}");
    // Positivity, one representative each.
    for decl in [
        "tool T5 Endmill(dia=6.0, flute_len=0.0)",
        "tool T5 Endmill(dia=6.0, flute_len=8.0, shank_dia=-6.0)",
        "tool T5 Endmill(dia=6.0, stickout=0.0)",
    ] {
        let err = body_part(decl).unwrap_err();
        assert!(err.contains("must be positive"), "{decl}: got: {err}");
    }
    // flute_count is a positive whole number.
    for decl in [
        "tool T5 Endmill(dia=6.0, flute_count=0)",
        "tool T5 Endmill(dia=6.0, flute_count=2.5)",
    ] {
        let err = body_part(decl).unwrap_err();
        assert!(err.contains("positive whole number"), "{decl}: got: {err}");
    }
}

#[test]
fn flute_len_refuses_where_the_cutting_top_already_has_an_owner() {
    // A drawn profile's top vertex owns the cutting top — the shipped
    // form-tool fixture's own SVG, with a flute_len beside it.
    let err = body_part(
        "tool T5 FormTool(dia=10.0, profile=\"sphere_formtool.cam/tool_profile.svg\", flute_len=8.0)",
    )
    .unwrap_err();
    assert!(err.contains("drawing's top vertex owns the cutting top"), "got: {err}");
    // …and a ground form's run-out is DERIVED, so a second record rejects.
    let err = body_part(
        "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, flute_len=6.0)",
    )
    .unwrap_err();
    assert!(err.contains("DERIVED from the declared form"), "got: {err}");
    let err = body_part(
        "tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5, flute_len=6.0)",
    )
    .unwrap_err();
    assert!(err.contains("DERIVED from the declared form"), "got: {err}");
}

#[test]
fn a_form_tools_shank_may_widen_the_body_but_never_erase_the_shoulder() {
    // Narrower than the declared body OD: the shoulder's length is not
    // declarable yet, so honoring the number would erase the shoulder —
    // under-stating the solid, the one direction a foul model must never
    // err. Rejected with the missing record named.
    let err = body_part(
        "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, shank_dia=8.0)",
    )
    .unwrap_err();
    assert!(err.contains("shoulder-length record"), "got: {err}");
    // At or above the OD it is a body fact the ladder holds.
    let part = body_part(
        "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, shank_dia=16.0)",
    )
    .unwrap();
    let sil = Silhouette::of(part.tools.get(&2).unwrap());
    assert!((sil.radius_at(9.0) - 8.0).abs() < 1e-12, "the declared shank widens the body");
    assert!((sil.max_radius() - 8.0).abs() < 1e-12);
    assert_eq!(sil.cutting_top(), Some(6.0), "the cutting envelope does not move");
}

#[test]
fn the_rod_ladder_answers_flutes_then_shank_and_moves_nothing_else() {
    // The stub ball on a fat bar — the acceptance fixture's tool.
    let part =
        body_part("tool T5 Ballmill(dia=6.0, flute_len=3.0, shank_dia=20.0)").unwrap();
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    // Below the flutes: the ball, untouched.
    assert!((sil.radius_at(0.0) - 0.0).abs() < 1e-12);
    assert!((sil.radius_at(1.5) - (9.0f64 - 2.25).sqrt()).abs() < 1e-12);
    // At the flute top and above: the bar.
    assert!((sil.radius_at(3.0) - 10.0).abs() < 1e-12);
    assert!((sil.radius_at(11.0) - 10.0).abs() < 1e-12);
    assert!((sil.max_radius() - 10.0).abs() < 1e-12);
    // Fork 5 landed (slice 2): declaring where the flutes end declares
    // where cutting ends, so the rod answers a cutting top and its stamp
    // truncates there. The shape family and the tip convention hold.
    assert!(sil.is_rod());
    assert_eq!(sil.cutting_top(), Some(3.0));
    assert_eq!(sil.narrowing(), None);
    assert!((sil.drop() - 3.0).abs() < 1e-12);

    // A long-reach neck — the shank NARROWER than the flutes, the other
    // blindness the rod could not say.
    let part = body_part("tool T5 Endmill(dia=10.0, flute_len=12.0, shank_dia=6.0)").unwrap();
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert!((sil.radius_at(5.0) - 5.0).abs() < 1e-12);
    assert!((sil.radius_at(12.0) - 3.0).abs() < 1e-12);
    assert!((sil.max_radius() - 5.0).abs() < 1e-12);
    assert_eq!(sil.narrowing(), None, "a shank removes nothing, so it is not an undercut");

    // flute_len alone: the shank is the cutting radius — the rod's own
    // assumption, now bounded in HEIGHT only for later slices; radius_at
    // answers exactly what it always did.
    let part = body_part("tool T5 Endmill(dia=10.0, flute_len=12.0)").unwrap();
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert!((sil.radius_at(30.0) - 5.0).abs() < 1e-12);
}

#[test]
fn the_relieved_neck_is_a_third_rung_and_not_an_undercut() {
    // The real long-reach cutter: flutes, a ground-back waist, then the
    // full shank. Three radii up one rod, which the ladder could not say
    // until the neck fields landed.
    let part = body_part(
        "tool T5 Endmill(dia=10.0, flute_len=12.0, neck_dia=8.0, neck_len=20.0, shank_dia=12.0)",
    )
    .unwrap();
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert!((sil.radius_at(6.0) - 5.0).abs() < 1e-12, "in the flutes, the cutting radius");
    assert!((sil.radius_at(12.0) - 4.0).abs() < 1e-12, "at the flute top the neck begins");
    assert!((sil.radius_at(31.9) - 4.0).abs() < 1e-12, "and holds to its top");
    assert!((sil.radius_at(32.0) - 6.0).abs() < 1e-12, "above which the shank stands");
    assert!((sil.max_radius() - 6.0).abs() < 1e-12);
    // Both breakpoints are published, so a consumer that BANDS the body
    // (the keep-out check) sees the narrow band without being told a neck
    // exists.
    assert_eq!(sil.rung_heights(), vec![12.0, 32.0]);

    // THE LOAD-BEARING ASSERTION OF THE WHOLE SLICE. A neck is relieved —
    // it removes nothing — so it is not an undercut, exactly as a narrow
    // shank is not. The undercut fence reads the DRAWN profile only, and
    // a rod's cutting envelope is a cylinder that stops at the flute top.
    assert_eq!(sil.narrowing(), None, "a relieved neck removes nothing, so it is not an undercut");
    assert_eq!(sil.cutting_top(), Some(12.0), "cutting still ends where the flutes do");

    // The T-SLOT CASE, which is where the catalog word "necked" and the
    // undercut class genuinely overlap: a disc on a neck, declared as a
    // rod. It is honest here — the declared flute length already truncates
    // the stamp at the cutting top and derives `upward_closed` false, so
    // what gets recorded is a disc, not a full-height column. The tools
    // this canNOT describe reject elsewhere and by name (below).
    let part = body_part(
        "tool T5 Endmill(dia=20.0, flute_len=3.0, neck_dia=6.0, neck_len=25.0, shank_dia=12.0)",
    )
    .unwrap();
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert_eq!(sil.cutting_top(), Some(3.0));
    assert!((sil.radius_at(4.0) - 3.0).abs() < 1e-12);
    assert_eq!(sil.narrowing(), None);

    // AND HERE IS THE ACTUAL LINE, which turns out to be geometric after
    // all: a rod's cutting envelope must attain its widest radius at its
    // TOP. A ball head is fine up to and including the equator —
    // `flute_len == dia/2` is a ball-nose endmill with no cylindrical
    // flute, monotone, declarable, and a neck may stand above it.
    let part = body_part("tool T5 Ballmill(dia=10.0, flute_len=5.0, neck_dia=4.0, neck_len=20.0)")
        .expect("a head that is at most a hemisphere is monotone");
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert!((sil.radius_at(5.0) - 2.0).abs() < 1e-12, "the neck starts at the equator");
    assert_eq!(sil.narrowing(), None);

    // Past the equator is the TRUE lollipop — the head wider than its own
    // top, the class that genuinely undercuts — and it rejects by a check
    // that has been there since slice 0, because its flutes would have to
    // end inside the ball. Three silhouette arms, three different doors,
    // one property: a rod's ball is bounded here, a drawn profile is
    // bounded by `narrowing()`, and a ground form is monotone by
    // construction.
    let err = body_part("tool T5 Ballmill(dia=10.0, flute_len=3.0, neck_dia=4.0, neck_len=20.0)")
        .expect_err("a head wider than its top is not a rod plus a neck");
    assert!(err.contains("shorter than the ball radius"), "got: {err}");
}

#[test]
fn the_neck_refuses_what_it_cannot_mean() {
    // Slice 1's holder-pairing mold: reject what cannot be placed, and
    // reject a field that would record nothing.
    for (decl, needle) in [
        ("tool T5 Endmill(dia=10.0, flute_len=12.0, neck_dia=8.0)", "without neck_len"),
        ("tool T5 Endmill(dia=10.0, flute_len=12.0, neck_len=8.0)", "without neck_dia"),
        // Nowhere to seat it: the neck's bottom IS the flute top.
        ("tool T5 Endmill(dia=10.0, neck_dia=8.0, neck_len=20.0)", "without flute_len"),
        // Not relieved, so it records nothing the shank does not.
        ("tool T5 Endmill(dia=10.0, flute_len=12.0, neck_dia=10.0, neck_len=20.0)", "not narrower than the cutting diameter"),
        ("tool T5 Endmill(dia=10.0, flute_len=12.0, neck_dia=12.0, neck_len=20.0)", "not narrower than the cutting diameter"),
        ("tool T5 Endmill(dia=10.0, flute_len=12.0, neck_dia=0.0, neck_len=20.0)", "must be positive"),
        // Past the nose face, where the holder's own rungs speak.
        ("tool T5 Endmill(dia=10.0, flute_len=12.0, neck_dia=8.0, neck_len=20.0, stickout=30.0)", "runs past stickout"),
        // A drawn or ground body already says where it narrows.
        ("tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, neck_dia=4.0, neck_len=10.0)", "drawn or derived polygon"),
    ] {
        let err = body_part(decl).expect_err(&format!("must reject: {decl}"));
        assert!(err.contains(needle), "wanted '{needle}', got: {err}");
    }

    // The MEASURED boundary: `flute_len + neck_len == stickout` is legal,
    // because `assembly::min_projection`'s floor is exactly flutes + neck
    // — at minimum projection the neck top lands precisely on the nose
    // face. Shipped data is the witness: `1/4-reach` in `slim-nose` ranks
    // at projection 38.100 = 12.700 + 25.400. A `>=` here would reject
    // the recommendation this project prints.
    let part = body_part(
        "tool T5 Endmill(dia=6.35, flute_len=12.7, neck_dia=5.8, neck_len=25.4, shank_dia=6.35, stickout=38.1)",
    )
    .expect("the optimizer's own minimum-projection answer must be declarable");
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert!((sil.radius_at(38.0) - 2.9).abs() < 1e-12, "the neck holds to the nose face");
}

#[test]
fn a_profile_tools_declared_shank_replaces_the_max_guess() {
    // Undeclared: the guess is max(top vertex, nominal r) — today's rule,
    // pinned so the default stays the default. Declared: the number wins,
    // and the narrower-than-the-flutes shank is finally expressible.
    let profile = vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 5.0, y: 10.0 }];
    let mk = |shank_dia: Option<f64>| Tool {
        num: 5,
        name: "FormTool".into(),
        dia: 10.0,
        profile: Some(profile.clone()),
        rounder: None,
        chamfer: None,
        neck_dia: None,
        neck_len: None,
        flute_len: None,
        shank_dia,
        stickout: None,
        holder: None,
        holder_nose_dia: None,
        holder_nose_len: None,
        holder_body_dia: None,
        flute_count: None, cut_bottom: None, cut_top: None, coolant: None,
    };
    let sil = Silhouette::of(&mk(None));
    assert!((sil.radius_at(20.0) - 5.0).abs() < 1e-12, "the guess: max(5.0, 5.0)");
    let sil = Silhouette::of(&mk(Some(6.0)));
    assert!((sil.radius_at(20.0) - 3.0).abs() < 1e-12, "the declaration replaces the guess");
    assert_eq!(sil.narrowing(), None, "and never trips the undercut fence");
}

// ---- K2, the tool body — slice 1: the holder rungs ----------------------

#[test]
fn the_holder_fields_parse_and_land_on_the_tool() {
    let part = body_part(
        "tool T5 Ballmill(dia=6.0, flute_len=3.0, shank_dia=10.0, stickout=8.0, holder=\"ER32 slim nose\", holder_nose_dia=32.0, holder_nose_len=40.0, holder_body_dia=50.0)",
    )
    .unwrap();
    let tool = part.tools.get(&5).unwrap();
    assert_eq!(tool.holder.as_deref(), Some("ER32 slim nose"), "the label, quotes stripped");
    assert_eq!(tool.holder_nose_dia, Some(32.0));
    assert_eq!(tool.holder_nose_len, Some(40.0));
    assert_eq!(tool.holder_body_dia, Some(50.0));
}

#[test]
fn inch_programs_scale_the_holder_lengths_but_not_the_label() {
    let src = "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\n\
stock Box(w=4, l=3, h=1, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=0.25, flute_len=0.75, shank_dia=0.25, stickout=1.5, holder=\"ER16\", holder_nose_dia=1.0, holder_nose_len=1.5, holder_body_dia=2.0)\n";
    let part = parse(src).unwrap();
    let tool = part.tools.get(&5).unwrap();
    assert!((tool.holder_nose_dia.unwrap() - 1.0 * 25.4).abs() < 1e-9);
    assert!((tool.holder_nose_len.unwrap() - 1.5 * 25.4).abs() < 1e-9);
    assert!((tool.holder_body_dia.unwrap() - 2.0 * 25.4).abs() < 1e-9);
    assert_eq!(tool.holder.as_deref(), Some("ER16"), "a label is not a length");
}

#[test]
fn the_holder_claim_checks_refuse_at_parse() {
    // The nose face sits at the stickout plane: no stickout, no placement.
    let err = body_part("tool T5 Endmill(dia=6.0, holder_nose_dia=32.0)").unwrap_err();
    assert!(err.contains("without stickout"), "got: {err}");
    // A length or a body with no nose diameter under it cannot be placed.
    let err =
        body_part("tool T5 Endmill(dia=6.0, stickout=30.0, holder_nose_len=40.0, holder_body_dia=50.0)")
            .unwrap_err();
    assert!(err.contains("without holder_nose_dia"), "got: {err}");
    // nose_len and body_dia are paired by error, both ways.
    let err = body_part(
        "tool T5 Endmill(dia=6.0, stickout=30.0, holder_nose_dia=32.0, holder_nose_len=40.0)",
    )
    .unwrap_err();
    assert!(err.contains("without holder_body_dia"), "got: {err}");
    let err = body_part(
        "tool T5 Endmill(dia=6.0, stickout=30.0, holder_nose_dia=32.0, holder_body_dia=50.0)",
    )
    .unwrap_err();
    assert!(err.contains("without holder_nose_len"), "got: {err}");
    // The nose is the narrow end.
    let err = body_part(
        "tool T5 Endmill(dia=6.0, stickout=30.0, holder_nose_dia=50.0, holder_nose_len=40.0, holder_body_dia=32.0)",
    )
    .unwrap_err();
    assert!(err.contains("wider than holder_body_dia"), "got: {err}");
    // The nose grips the shank — declared, or the rod's own diameter.
    let err = body_part(
        "tool T5 Endmill(dia=6.0, flute_len=8.0, shank_dia=10.0, stickout=12.0, holder_nose_dia=8.0)",
    )
    .unwrap_err();
    assert!(err.contains("narrower than the shank it grips"), "got: {err}");
    let err = body_part("tool T5 Ballmill(dia=6.0, stickout=10.0, holder_nose_dia=4.0)").unwrap_err();
    assert!(err.contains("narrower than the shank it grips"), "got: {err}");
    // Positivity, one representative.
    let err = body_part("tool T5 Endmill(dia=6.0, stickout=30.0, holder_nose_dia=0.0)").unwrap_err();
    assert!(err.contains("must be positive"), "got: {err}");
    // A stock label is a record of nothing.
    let err = body_part("tool T5 Endmill(dia=6.0, holder=\"\")").unwrap_err();
    assert!(err.contains("holder label is empty"), "got: {err}");
    // The polygon arms have no holder step yet: rejected with the gap
    // named, never accepted-and-unread (the whitelist's own law).
    let err = body_part(
        "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, stickout=30.0, holder_nose_dia=32.0)",
    )
    .unwrap_err();
    assert!(err.contains("rod ladder today"), "got: {err}");
    // …but the LABEL is provenance, legal anywhere — it claims nothing
    // the ladder has to hold.
    let part = body_part(
        "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, holder=\"shrink 12\")",
    )
    .unwrap();
    assert_eq!(part.tools.get(&2).unwrap().holder.as_deref(), Some("shrink 12"));
}

#[test]
fn the_ladder_answers_nose_then_body_and_the_cutting_envelope_stays_put() {
    // The acceptance fixture's tool: ball flutes to 3, shank r 5 to the
    // stickout plane at 8, nose r 16 for 40mm, body r 25 above.
    let part = body_part(
        "tool T5 Ballmill(dia=6.0, flute_len=3.0, shank_dia=10.0, stickout=8.0, holder=\"ER32 slim nose\", holder_nose_dia=32.0, holder_nose_len=40.0, holder_body_dia=50.0)",
    )
    .unwrap();
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert!((sil.radius_at(1.5) - (9.0f64 - 2.25).sqrt()).abs() < 1e-12, "the ball, untouched");
    assert!((sil.radius_at(5.0) - 5.0).abs() < 1e-12, "the shank");
    assert!((sil.radius_at(8.0) - 16.0).abs() < 1e-12, "the nose face IS the stickout plane");
    assert!((sil.radius_at(30.0) - 16.0).abs() < 1e-12, "the nose");
    assert!((sil.radius_at(48.0) - 25.0).abs() < 1e-12, "the body from the nose top up");
    assert!((sil.radius_at(200.0) - 25.0).abs() < 1e-12, "…and as high as anyone asks");
    assert!((sil.max_radius() - 25.0).abs() < 1e-12);
    // Fork 5 landed (slice 2): the declared flutes ARE the cutting top,
    // so the stamp truncates there and the field voice gates open — while
    // the SHAPE stays a rod and the ladder stays body data.
    assert!(sil.is_rod());
    assert_eq!(sil.cutting_top(), Some(3.0));
    assert_eq!(sil.narrowing(), None);
    assert!((sil.drop() - 3.0).abs() < 1e-12);
}

#[test]
fn a_holder_seats_on_the_rod_itself_when_no_flute_len_is_declared() {
    // A drill-in-chuck shape: no flute_len, so no shank rung — the rod
    // stays its cutting radius all the way to the nose face, and the nose
    // (declared alone, no length) stands for the whole holder above it.
    let part =
        body_part("tool T5 Endmill(dia=10.0, stickout=30.0, holder_nose_dia=40.0)").unwrap();
    let sil = Silhouette::of(part.tools.get(&5).unwrap());
    assert!((sil.radius_at(29.9) - 5.0).abs() < 1e-12, "the rod, to the nose face");
    assert!((sil.radius_at(30.0) - 20.0).abs() < 1e-12, "the nose at its face");
    assert!((sil.radius_at(500.0) - 20.0).abs() < 1e-12, "a lone nose is the holder entire");
    assert!((sil.max_radius() - 20.0).abs() < 1e-12);
    assert!(sil.is_rod(), "still a rod: the ladder is body data, not cutting data");
}

// ---- K2, the tool body — slice 2: the cutting band (fork 3A) ----

/// A profile-tool program over a temp SVG, parallel-safe by unique name:
/// the drawn cone `M 0,0 L 5,10` scales 1:1 under dia=10, so the parsed
/// profile is exactly (0,0)→(5,10) and every band height is hand-checkable.
fn band_part(svg_name: &str, tool_args: &str) -> Result<Part, String> {
    std::fs::write(svg_name, "<svg>\n  <path d=\"M 0,0 L 5,10\"/>\n</svg>").unwrap();
    let r = parse(&format!(
        "{ROUNDER_PREAMBLE}tool T6 FormTool(dia=10.0, profile=\"{svg_name}\"{tool_args})\n"
    ));
    let _ = std::fs::remove_file(svg_name);
    r
}

#[test]
fn a_declared_band_clips_the_cutting_envelope_and_keeps_the_body() {
    let part = band_part("band_clip.svg", ", cut_bottom=1.0, cut_top=6.0").unwrap();
    let tool = part.tools.get(&6).unwrap();
    assert_eq!(tool.cut_bottom, Some(1.0));
    assert_eq!(tool.cut_top, Some(6.0));
    let sil = Silhouette::of(tool);
    // The envelope stops at the band top, with the interpolated boundary
    // vertex stating the drawn width there (the cone is 3.0 wide at 6.0).
    assert_eq!(sil.cutting_top(), Some(6.0));
    assert!((sil.radius_at(6.0) - 3.0).abs() < 1e-12);
    // Above the band the tool is still its drawn relieved BODY, answered
    // at the relief's widest (the rounder's shoulder convention): the
    // drawn cone reaches 5.0 at its top, so the body above 6.0 answers
    // 5.0 — pessimistic where the relief is narrower, never under-stated.
    assert!((sil.radius_at(8.0) - 5.0).abs() < 1e-12);
    assert!((sil.radius_at(20.0) - 5.0).abs() < 1e-12);
    assert!((sil.max_radius() - 5.0).abs() < 1e-12);
    // `cut_bottom` bounds CONTACT only: like the rounder's pilot, the
    // solid below it stays in the envelope, so nothing here moves.
    assert!((sil.radius_at(0.5) - 0.25).abs() < 1e-12);
}

#[test]
fn a_band_needs_a_drawing_and_stays_inside_it() {
    // On a rod the flutes' end already has its one owner.
    let err = body_part("tool T5 Endmill(dia=10.0, cut_top=6.0)").unwrap_err();
    assert!(err.contains("without a drawn profile") && err.contains("flute_len"), "{err}");
    // On a ground form the run-out is derived; a second record rejects.
    let err = rounder_part(
        "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, cut_top=5.0)",
    )
    .unwrap_err();
    assert!(err.contains("DERIVED from the declared form"), "{err}");
    // The band cannot claim cutting on steel the drawing does not state.
    let err = band_part("band_over.svg", ", cut_top=12.0").unwrap_err();
    assert!(err.contains("above the drawn profile's top vertex"), "{err}");
    // Degenerate bands are tools that cannot cut.
    let err = band_part("band_zero.svg", ", cut_top=0.0").unwrap_err();
    assert!(err.contains("must be positive"), "{err}");
    let err = band_part("band_neg.svg", ", cut_bottom=-1.0").unwrap_err();
    assert!(err.contains("must not be negative"), "{err}");
    let err = band_part("band_empty.svg", ", cut_bottom=6.0, cut_top=6.0").unwrap_err();
    assert!(err.contains("no height"), "{err}");
    // And the explicit defaults are legal: today, said out loud.
    let part = band_part("band_dflt.svg", ", cut_bottom=0.0, cut_top=10.0").unwrap();
    let sil = Silhouette::of(part.tools.get(&6).unwrap());
    assert_eq!(sil.cutting_top(), Some(10.0), "the full drawing is the default band");
    assert!((sil.radius_at(10.0) - 5.0).abs() < 1e-12);
}

#[test]
fn a_shank_narrower_than_the_drawn_relief_refuses() {
    // With the band at 6.0 the drawn cone above it reaches 5.0: a 8mm
    // shank (r 4) would erase drawn steel, and where the relief steps
    // down is not a record the drawing carries.
    let err = band_part("band_shank.svg", ", cut_top=6.0, shank_dia=8.0").unwrap_err();
    assert!(err.contains("narrower than the drawn profile above cut_top"), "{err}");
    // At or above the relief it is a body fact the ladder holds.
    let part = band_part("band_shank_ok.svg", ", cut_top=6.0, shank_dia=12.0").unwrap();
    let sil = Silhouette::of(part.tools.get(&6).unwrap());
    assert!((sil.radius_at(8.0) - 6.0).abs() < 1e-12);
}

#[test]
fn a_narrowing_above_the_band_is_relief_and_stops_tripping_the_fence() {
    // A neck drawn ABOVE the cutting zone: wide at 5, necked to 3 above.
    // Undeclared, the whole drawing is assumed cutting and the undercut
    // fence rejects to stamp it; banded at 5.0, the neck is relief —
    // which removes nothing — and the envelope is monotone again.
    let svg = "band_neck.svg";
    std::fs::write(svg, "<svg>\n  <path d=\"M 0,0 L 5,5 L 3,8 L 3,10\"/>\n</svg>").unwrap();
    let src = |args: &str| {
        format!("{ROUNDER_PREAMBLE}tool T6 FormTool(dia=10.0, profile=\"{svg}\"{args})\n")
    };
    let bare = parse(&src("")).unwrap();
    let banded = parse(&src(", cut_top=5.0")).unwrap();
    let _ = std::fs::remove_file(svg);
    let sil = Silhouette::of(bare.tools.get(&6).unwrap());
    assert!(sil.narrowing().is_some(), "undeclared, the neck reads as an undercut");
    let sil = Silhouette::of(banded.tools.get(&6).unwrap());
    assert_eq!(sil.narrowing(), None, "banded, the neck is relief");
    assert_eq!(sil.cutting_top(), Some(5.0));
    // The relieved body above the band answers at its widest (5.0 at the
    // band top itself), never the neck's 3.0 — the safe direction.
    assert!((sil.radius_at(7.0) - 5.0).abs() < 1e-12);
}

#[test]
fn an_unrelieved_pilot_refuses_and_names_the_trap() {
    let decl = "tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=6.0)";
    let err = rounder_part(decl).unwrap_err();
    assert!(err.contains("not relieved"), "{err}");
    assert!(err.contains("transposed"), "{err}");
}

#[test]
fn the_rounder_refusals_hold_their_edges() {
    let bad = [
        ("radius=0.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94", "must be positive"),
        ("radius=3.0, effective_dia=6.0, gauge_z=2.0, dia=12.0, pilot=5.94", "arc centre at or under the tip"),
        ("radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=5.0, pilot=4.0", "BODY OD"),
        ("radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, radius_error=3.5", "transposition"),
        ("radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, trail_wall=95.0", "must be in [0, 90)"),
        ("radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, trail_wall=50.0, trail_face=45.0", "leaves no arc"),
        ("radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, source=guessed", "expected 'catalog'"),
    ];
    for (args, needle) in bad {
        let err = rounder_part(&format!("tool T2 CornerRounder({args})")).unwrap_err();
        assert!(err.contains(needle), "args `{args}` gave: {err}");
    }
    // A ground arc on a tool whose form is not an arc.
    let err = rounder_part("tool T2 Endmill(dia=10.0, radius=3.0)").unwrap_err();
    assert!(err.contains("corner-rounder geometry"), "{err}");
}

#[test]
fn a_rounder_is_refused_by_every_op_that_wants_a_floor() {
    // The flat-bottom rule would otherwise wave it through: a rounder
    // carries no `profile`, so it looks like an endmill from there.
    let src = format!(
        "{ROUNDER_PREAMBLE}{T2}\nfeature Pocket CylinderBore(dia=20.0, extent=5.0, x=10, y=10)\nrun Clear(tool=T2, profile=Pocket, stepover=4.0)\n"
    );
    let err = parse(&src).and_then(|p| validate(&p).map(|_| p)).unwrap_err();
    assert!(err.contains("RELIEVED pilot"), "{err}");

    // And Form rejects by name rather than blaming the ball-nose rule: a
    // rounder cuts a declared round, so the missing DECLARATION is the
    // complaint, and it names the field to add.
    let src = format!(
        "{ROUNDER_PREAMBLE}{T2}\nfeature Pocket CylinderBore(dia=20.0, extent=5.0, x=10, y=10)\nrun Form(tool=T2, surface=Pocket)\n"
    );
    let err = parse(&src).and_then(|p| validate(&p).map(|_| p)).unwrap_err();
    assert!(err.contains("declares no edge_round"), "{err}");
    assert!(err.contains("edge_round=3.000"), "the error names the round the tool has: {err}");
}

// A self-consistent 90-degree mill: apex 0.25 below the tip, exactly
// (tip_dia/2)/tan(45deg) — the gauge point, the angle, and the flat all
// describing one tool.
const T3: &str = "tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)";

#[test]
fn the_ground_cone_derives_its_apex_and_never_shows_the_flat_as_a_datum() {
    let part = rounder_part(T3).unwrap();
    let tool = part.tools.get(&3).unwrap();
    let cone = tool.ground_cone().expect("a ChamferMill hands over its cone");

    // The apex sits BELOW the physical tip by (tip_dia/2)/tan(phi_half) on
    // a consistently-ground tool — the catalogue's sharp point, out in the
    // air under the flat.
    assert!((cone.z_apex - (-0.25)).abs() < 1e-9, "z_apex = {}", cone.z_apex);
    // The gauge point round-trips: the cone reaches gauge_dia/2 at gauge_z.
    assert!((cone.radius_at(3.75) - 4.0).abs() < 1e-9);
    assert!((cone.height_at_radius(4.0) - 3.75).abs() < 1e-9);
    // The flank's bounds: it starts at the flat's edge (here exactly the
    // tip plane) and runs out into the body OD.
    assert!(cone.cutting_bottom().abs() < 1e-9, "cutting_bottom = {}", cone.cutting_bottom());
    assert!((cone.flank_top() - 5.75).abs() < 1e-9, "flank_top = {}", cone.flank_top());

    // An angle error retilts the whole derivation — the actual angle is
    // what pins the cone, the nominal is only the identity. (The flat
    // measures wider on this tool: a steeper flank meets the tip plane at
    // 0.565mm, and a declared 0.5 flat would be the consistency error —
    // the three records must describe ONE tool.)
    let part = rounder_part(
        "tool T3 ChamferMill(angle=90.0, angle_error=-0.5, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.6)",
    )
    .unwrap();
    let cone = part.tools.get(&3).unwrap().ground_cone().unwrap();
    let phi = (89.5f64 / 2.0).to_radians();
    assert!((cone.phi_half - phi).abs() < 1e-12);
    assert!((cone.z_apex - (3.75 - 4.0 / phi.ptan())).abs() < 1e-12);
}

#[test]
fn the_chamfer_silhouette_is_the_body_and_the_datum_is_the_tip() {
    let part = rounder_part(T3).unwrap();
    let tool = part.tools.get(&3).unwrap();
    let sil = Silhouette::of(tool);

    // Emitted coordinates are the physical TIP — the surface a presetter
    // finds — so nothing hangs below them. The apex trap lives in
    // GroundCone, not in the emission datum.
    assert!(sil.drop().abs() < 1e-12);
    // Cutting stops where the flank runs out into the body OD.
    let cut_top = sil.cutting_top().expect("a shaped tool has a cutting top");
    assert!((cut_top - 5.75).abs() < 1e-9);
    assert!(!sil.is_rod());

    // The body: the relieved flat at the tip, the straight flank, the
    // shank at the OD above the run-out. A cone flank is straight, so the
    // polygon is exact — no chords, no sagitta.
    assert!((sil.radius_at(0.0) - 0.25).abs() < 1e-9);
    assert!((sil.radius_at(3.75) - 4.0).abs() < 1e-9, "the flank IS the cone");
    assert!((sil.radius_at(9.0) - 6.0).abs() < 1e-12, "the shank is the body OD");
    assert!((sil.max_radius() - 6.0).abs() < 1e-12);
    // Monotone widening by construction: no narrowing, so the voxel walker
    // stamps it (the rounder's relief reasoning, without even a step).
    assert!(sil.narrowing().is_none());
}

#[test]
fn the_chamfer_refusals_hold_their_edges() {
    let bad = [
        ("angle=0.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5", "strictly inside (0, 180)"),
        ("angle=180.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5", "strictly inside (0, 180)"),
        ("angle=90.0, angle_error=95.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5", "transposition"),
        ("angle=90.0, gauge_dia=0.0, gauge_z=3.75, dia=12.0, tip_dia=0.5", "measured diameter ON the cone"),
        ("angle=90.0, gauge_dia=8.0, gauge_z=0.0, dia=12.0, tip_dia=0.5", "above the TIP"),
        ("angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.0", "catalogue's sharp point"),
        ("angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=9.0", "wrong surface"),
        ("angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=6.0, tip_dia=0.5", "BODY OD"),
        // The consistency refusal: gauge point + angle put the flank wider
        // than the declared flat at the tip plane.
        ("angle=90.0, gauge_dia=8.0, gauge_z=2.0, dia=12.0, tip_dia=0.5", "cannot all be true"),
        ("angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5, source=guessed", "expected 'catalog'"),
    ];
    for (args, needle) in bad {
        let err = rounder_part(&format!("tool T3 ChamferMill({args})")).unwrap_err();
        assert!(err.contains(needle), "args `{args}` gave: {err}");
    }

    // The missing flat is its own authored error, not a generic missing
    // key: the whole point of the field is that the sharp point is not
    // there.
    let err = rounder_part("tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0)").unwrap_err();
    assert!(err.contains("BELOW the physical tip"), "{err}");

    // Cross-rejections: each form's geometry belongs to its own record.
    let err = rounder_part("tool T3 Endmill(dia=10.0, angle=90.0)").unwrap_err();
    assert!(err.contains("chamfer-mill geometry"), "{err}");
    let err = rounder_part("tool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5, trail_wall=2.0)").unwrap_err();
    assert!(err.contains("corner-rounder geometry"), "{err}");
    let err = rounder_part("tool T2 CornerRounder(radius=3.0, effective_dia=6.0, gauge_z=6.0, dia=12.0, pilot=5.94, tip_dia=0.5)").unwrap_err();
    assert!(err.contains("chamfer-mill geometry"), "{err}");
    // gauge_z is shared by the two form tools and meaningless on a tool
    // that declares neither form.
    let err = rounder_part("tool T3 Endmill(dia=10.0, gauge_z=4.0)").unwrap_err();
    assert!(err.contains("declares neither"), "{err}");
}

#[test]
fn a_chamfer_mill_is_refused_by_every_op_that_wants_a_floor() {
    // The flat-bottom rule names the relieved flat, not a generic shape.
    let src = format!(
        "{ROUNDER_PREAMBLE}{T3}\nfeature Pocket CylinderBore(dia=20.0, extent=5.0, x=10, y=10)\nrun Clear(tool=T3, profile=Pocket, stepover=4.0)\n"
    );
    let err = parse(&src).and_then(|p| validate(&p).map(|_| p)).unwrap_err();
    assert!(err.contains("RELIEVED flat"), "{err}");

    // And Form rejects by naming the missing DECLARATION, with the field
    // to add — the rounder's landing shape, one form over.
    let src = format!(
        "{ROUNDER_PREAMBLE}{T3}\nfeature Pocket CylinderBore(dia=20.0, extent=5.0, x=10, y=10)\nrun Form(tool=T3, surface=Pocket)\n"
    );
    let err = parse(&src).and_then(|p| validate(&p).map(|_| p)).unwrap_err();
    assert!(err.contains("declares no chamfer"), "{err}");
    assert!(err.contains("chamfer_depth="), "the error names the field to add: {err}");
}

#[test]
fn inch_programs_scale_the_form_tool_records() {
    // The rounder's block was missing from the conversion boundary until
    // the chamfer's went in beside it — an inch program with a rounder got
    // unscaled arc geometry. Lengths scale; angles never do.
    let src = "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\n\
               stock Box(w=4, l=3, h=1, x=0, y=0, z=0)\n\
               tool T2 CornerRounder(radius=0.125, effective_dia=0.25, gauge_z=0.25, dia=0.5, pilot=0.24, trail_wall=4.0)\n\
               tool T3 ChamferMill(angle=90.0, gauge_dia=0.2, gauge_z=0.11, dia=0.5, tip_dia=0.02)\n";
    let part = parse(src).unwrap();

    let rd = part.tools.get(&2).unwrap().rounder.unwrap();
    assert!((rd.radius - 0.125 * 25.4).abs() < 1e-9);
    assert!((rd.effective_dia - 0.25 * 25.4).abs() < 1e-9);
    assert!((rd.gauge_z - 0.25 * 25.4).abs() < 1e-9);
    assert!((rd.pilot - 0.24 * 25.4).abs() < 1e-9);
    assert!((rd.trail_wall - 4.0).abs() < 1e-12, "degrees are not lengths");

    let ch = part.tools.get(&3).unwrap().chamfer.unwrap();
    assert!((ch.gauge_dia - 0.2 * 25.4).abs() < 1e-9);
    assert!((ch.gauge_z - 0.11 * 25.4).abs() < 1e-9);
    assert!((ch.tip_dia - 0.02 * 25.4).abs() < 1e-9);
    assert!((ch.angle - 90.0).abs() < 1e-12, "degrees are not lengths");
    }

#[test]
fn inch_programs_scale_the_mouth_cone() {
    // The seat is a length at the conversion boundary; `csink_angle` is
        // degrees and never scales (the `rotation` rule).
        let src = "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\n\
                   stock Box(w=4, l=3, h=1, x=0, y=0, z=0)\n\
                   feature H1 CylinderBore(dia=0.25, extent=0.6, x=1.0, y=1.0, csink_dia=0.44, csink_angle=90.0)\n\
                   feature H2 CylinderBore(dia=0.25, extent=0.6, x=2.0, y=1.0, spot_dia=0.1)\n";
        let part = parse(src).unwrap();
        let spec_of = |name: &str| match part.features.get(name).unwrap() {
            Feature::CylinderBore { csink: Some(s), .. } => *s,
            _ => panic!("expected a hole with a mouth cone"),
    };
    let s1 = spec_of("H1");
    assert_eq!(s1.seat, CsinkSeat::Csink(0.44 * 25.4));
    assert!((s1.angle.unwrap() - 90.0).abs() < 1e-12, "degrees are not lengths");
    assert_eq!(spec_of("H2").seat, CsinkSeat::Spot(0.1 * 25.4));
}

#[test]
fn inch_programs_scale_the_counterbore() {
    // Both counterbore fields are lengths at the conversion boundary, so
    // both scale — which keeps the pairing law's dia-vs-tool comparison
    // scale-invariant (the tool's dia converts through the same boundary).
    let src = "units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\n\
               stock Box(w=4, l=3, h=1, x=0, y=0, z=0)\n\
               feature H CylinderBore(dia=0.25, extent=0.6, x=1.0, y=1.0, cbore_dia=0.5, cbore_depth=0.2)\n";
    let part = parse(src).unwrap();
    let cb = match part.features.get("H").unwrap() {
        Feature::CylinderBore { cbore: Some(cb), .. } => *cb,
        _ => panic!("expected a hole with a counterbore"),
    };
    assert!((cb.dia - 0.5 * 25.4).abs() < 1e-9);
    assert!((cb.depth - 0.2 * 25.4).abs() < 1e-9);
}

#[test]
fn test_svg_profile_parsing() {
    let svg_path = "test_tool_profile.svg";
    let svg_content = r#"<svg>
  <path d="M 0,0 l 2,0 c 1,1 2,2 3,3 a 2,2 0 0,1 2,2 Z"/>
</svg>"#;
    std::fs::write(svg_path, svg_content).unwrap();

        let input = r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=120, l=80, h=20, x=0, y=0, z=0)
tool T6 FormTool(dia=10.0, profile="test_tool_profile.svg")
"#;
    let part = parse(input);
    
    let _ = std::fs::remove_file(svg_path);

    let part = part.unwrap();
    let tool = part.tools.get(&6).unwrap();
    assert_eq!(tool.name, "FormTool");
    assert_eq!(tool.dia, 10.0);
    
    let profile = tool.profile.as_ref().unwrap();
    assert!(!profile.is_empty());
    
    for i in 0..profile.len() - 1 {
        assert!(profile[i].y <= profile[i+1].y);
    }

    let max_r = profile.iter().map(|p| p.x).fold(0.0f64, |a, b| a.max(b));
    assert!((max_r - 5.0).abs() < 1e-9);
}

// One triangle, enough to parse; the same file body serves both mesh
    // parse tests below (each writes its own uniquely-named copy so the
    // tests can run in parallel).
    const TEST_STL: &str = "solid t\n\
         facet normal 0 0 1\n outer loop\n\
         vertex 0 0 0\n vertex 10 0 0\n vertex 10 5 5\n\
         endloop\n endfacet\nendsolid t\n";

    #[test]
    fn test_parse_mesh_feature() {
    let stl_path = "test_parse_mesh_feature.stl";
    std::fs::write(stl_path, TEST_STL).unwrap();

    let input = r#"
units mm
feed 800
plunge 300
rapid_z 5
clearance_z 25
spindle 3000
stock Box(w=120, l=80, h=20, x=0, y=0, z=0)
feature Blob Mesh(file="test_parse_mesh_feature.stl", x=10, y=20, z=3)
"#;
    let part = parse(input);
    let _ = std::fs::remove_file(stl_path);

    let part = part.unwrap();
    match part.features.get("Blob").unwrap() {
        Feature::Mesh { triangles, x, y, z, file } => {
            assert_eq!(triangles.len(), 1);
            assert_eq!(triangles[0].v[2], [10.0, 5.0, 5.0]);
            assert_eq!((*x, *y, *z), (10.0, 20.0, 3.0));
            assert_eq!(file, stl_path);
        }
        other => panic!("expected a Mesh feature, got {:?}", other),
    }
}

#[test]
fn test_mesh_offsets_convert_at_parse_but_vertices_do_not() {
    // STL vertex data is defined to be mm; under `units inch` only the
    // placement offsets scale (the Feature::Mesh doc comment's contract).
    let stl_path = "test_mesh_units.stl";
    std::fs::write(stl_path, TEST_STL).unwrap();

    let input = r#"
units inch
feed 30
plunge 10
rapid_z 0.2
clearance_z 1
spindle 3000
stock Box(w=5, l=4, h=1, x=0, y=0, z=0)
feature Blob Mesh(file="test_mesh_units.stl", x=1, y=2, z=0.5)
"#;
    let part = parse(input);
    let _ = std::fs::remove_file(stl_path);

    let part = part.unwrap();
    match part.features.get("Blob").unwrap() {
        Feature::Mesh { triangles, x, y, z, .. } => {
            assert_eq!((*x, *y, *z), (25.4, 50.8, 12.7));
            assert_eq!(triangles[0].v[1], [10.0, 0.0, 0.0]); // unscaled
        }
        other => panic!("expected a Mesh feature, got {:?}", other),
    }
}

// ---- the feeds table (K3) ----------------------------------------------

const DERIVE_FIXTURE: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
                              stock Box(w=120, l=80, h=20, x=0, y=0, z=0, material=6061)\n\
                              tool T5 Endmill(dia=10.0)\n\
                              feature TopFace Plane(z=-0.5)\n\
                              run End(tool=T5, surface=TopFace, stepover=6.0, spindle=derive)\n";

#[test]
fn test_feeds_grammar_happy_path() {
    let table = parse_feeds(
        "# comment\nunits mm\nfeeds 6061 Endmill\nsurface_speed 150\nsource measured in this shop\nchipload 1..3 0.010\nchipload 3..6 0.020\n",
    )
    .unwrap();
    assert_eq!(table.records.len(), 1);
    let r = &table.records[0];
    assert_eq!((r.material.as_str(), r.tool_type.as_str()), ("6061", "Endmill"));
    assert_eq!(r.surface_speed, 150.0);
    assert_eq!(r.source, "measured in this shop");
    assert_eq!(r.chiploads.len(), 2);
    assert_eq!(r.chiploads[1].dia_lo, 3.0);
    assert_eq!(r.chiploads[1].mm_per_tooth, 0.020);
}

#[test]
fn test_shipped_feeds_table_parses_and_holds_the_roster() {
    // The shipped table is program input compiled in — a syntax error
    // there is a build defect, so this pin keeps it honest. Six
    // materials × two tool types, per fork 3B.
    let table = parse_feeds(STARTER_FEEDS).unwrap();
    assert_eq!(table.records.len(), 12);
    let r = table.record("6061", "Endmill").expect("6061×Endmill must ship");
    assert_eq!(r.surface_speed, 150.0);
    // `source` is required by grammar; this asserts the *content* made it.
    assert!(table.records.iter().all(|r| r.source.contains("carbide")));
}

#[test]
fn test_feeds_source_is_required() {
    let err = parse_feeds("feeds 6061 Endmill\nsurface_speed 150\n").unwrap_err();
    assert!(err.contains("declares no source"), "got: {}", err);
}

#[test]
fn test_feeds_refusals_share_the_catalog_discipline() {
    // duplicate record
    let err = parse_feeds(
        "feeds 6061 Endmill\nsurface_speed 150\nsource a\nfeeds 6061 Endmill\nsurface_speed 90\nsource b\n",
    )
    .unwrap_err();
    assert!(err.contains("duplicate feeds record"), "got: {}", err);
    // field before any opener
    let err = parse_feeds("surface_speed 150\n").unwrap_err();
    assert!(err.contains("before any 'feeds"), "got: {}", err);
    // overlapping bands (half-open, so touching is legal — see below)
    let err = parse_feeds(
        "feeds 6061 Endmill\nsurface_speed 150\nsource a\nchipload 1..4 0.01\nchipload 3..6 0.02\n",
    )
    .unwrap_err();
    assert!(err.contains("overlaps"), "got: {}", err);
    // touching endpoints compose
    parse_feeds(
        "feeds 6061 Endmill\nsurface_speed 150\nsource a\nchipload 1..3 0.01\nchipload 3..6 0.02\n",
    )
    .unwrap();
    // unknown keyword
    let err = parse_feeds("feeds 6061 Endmill\nsurface_speed 150\nsource a\nsfm 100\n").unwrap_err();
    assert!(err.contains("unknown feeds keyword 'sfm'"), "got: {}", err);
}

#[test]
fn test_feeds_units_inch_converts_at_the_boundary() {
    let table = parse_feeds(
        "units inch\nfeeds 6061 Endmill\nsurface_speed 100\nsource sfm numbers\nchipload 0.25..0.5 0.001\n",
    )
    .unwrap();
    let r = &table.records[0];
    assert!((r.surface_speed - 30.48).abs() < 1e-9); // 100 SFM → m/min
    assert!((r.chiploads[0].dia_lo - 6.35).abs() < 1e-9);
    assert!((r.chiploads[0].dia_hi - 12.7).abs() < 1e-9);
    assert!((r.chiploads[0].mm_per_tooth - 0.0254).abs() < 1e-9);
}

#[test]
fn test_feeds_units_is_a_header_line() {
    let err =
        parse_feeds("feeds 6061 Endmill\nsurface_speed 150\nsource a\nunits inch\n").unwrap_err();
    assert!(err.contains("header line"), "got: {}", err);
}

#[test]
fn test_stock_material_parses_and_defaults_to_none() {
    let p = parse(DERIVE_FIXTURE).unwrap();
    assert_eq!(p.stock.material.as_deref(), Some("6061"));
    let plain = DERIVE_FIXTURE
        .replace(", material=6061", "")
        .replace("spindle=derive", "spindle=4000");
    let p = parse(&plain).unwrap();
    assert_eq!(p.stock.material, None);
}

#[test]
fn test_spindle_derive_resolves_from_the_shipped_table() {
    // RPM = 1000·v_c/(π·d) = 1000·150/(π·10), written into the op's own
    // Option so emission never learns the table exists (fork 4A).
    let p = parse(DERIVE_FIXTURE).unwrap();
    let expected = derive_spindle_rpm(150.0, 10.0);
    assert!((p.operations[0].spindle.unwrap() - expected).abs() < 1e-9);
    assert!((expected - 4774.648292).abs() < 1e-6);
    // The globals stay untouched — declared-outranks-derived is per op.
    assert_eq!(p.global_spindle, 3000.0);
}

#[test]
fn test_derive_without_material_refuses_by_name() {
    let src = DERIVE_FIXTURE.replace(", material=6061", "");
    let err = parse(&src).unwrap_err();
    assert!(err.contains("the stock declares no material"), "got: {}", err);
}

#[test]
fn test_derive_row_miss_refuses_and_offers_the_inventory() {
    let src = DERIVE_FIXTURE.replace("material=6061", "material=titanium");
    let err = parse(&src).unwrap_err();
    assert!(err.contains("no feeds record for material 'titanium'"), "got: {}", err);
    assert!(err.contains("the table holds:"), "got: {}", err);
    assert!(err.contains("6061\u{d7}Endmill"), "got: {}", err);
}

#[test]
fn test_feed_derive_refuses_until_the_tool_declares_flutes() {
    // The K2 seam, rejected by name (the cross-lane ruling): the chip
    // load is ours, the flute count is the tool's, and until the tool
    // line carries one the F half cannot honestly close.
    let src = DERIVE_FIXTURE.replace("spindle=derive", "feed=derive");
    let err = parse(&src).unwrap_err();
    assert!(err.contains("tool T5 declares no flute count"), "got: {}", err);
}

#[test]
fn test_feed_derive_band_miss_refuses_with_the_bands_held() {
    let src = DERIVE_FIXTURE
        .replace("spindle=derive", "feed=derive")
        .replace("dia=10.0", "dia=20.0")
        .replace("stepover=6.0", "stepover=12.0");
    let err = parse(&src).unwrap_err();
    assert!(err.contains("no chipload band covering dia 20"), "got: {}", err);
    assert!(err.contains("6..13"), "got: {}", err);
}

#[test]
fn test_feed_derive_closes_with_the_flute_count() {
    // Slice 1: the tool carries K2's flute field, so the F half closes —
    // F = RPM × z × f_z with the band's chip load and the RESOLVED
    // spindle, here the derived one (spindle=derive rides along). The
    // global feed stays untouched: declared-outranks-derived is per op.
    let src = DERIVE_FIXTURE
        .replace("dia=10.0", "dia=10.0, flute_count=3")
        .replace("spindle=derive", "spindle=derive, feed=derive");
    let p = parse(&src).unwrap();
    let rpm = derive_spindle_rpm(150.0, 10.0);
    let expected = derive_feed_mm_min(rpm, 3, 0.040);
    assert!((p.operations[0].feed.unwrap() - expected).abs() < 1e-9);
    assert!((expected - 572.957795).abs() < 1e-6);
    assert_eq!(p.global_feed, 800.0);
}

#[test]
fn test_feed_derive_reads_the_resolved_spindle_when_declared() {
    // The RPM-source position (slice 1): when the spindle is
    // author-declared, the feed identity multiplies by THAT number —
    // the chip thins by what the spindle actually turns, not by what
    // the table would have recommended. Global spindle 3000 resolves
    // the op, so F = 3000 × 3 × 0.040 = 360.
    let src = DERIVE_FIXTURE
        .replace("dia=10.0", "dia=10.0, flute_count=3")
        .replace("spindle=derive", "feed=derive");
    let p = parse(&src).unwrap();
    assert_eq!(p.operations[0].spindle, None, "no S word derived or declared on the op");
    assert!((p.operations[0].feed.unwrap() - 360.0).abs() < 1e-9);
}

#[test]
fn test_feed_derive_band_miss_still_refuses_first_with_flutes() {
    // The slice-0 ordering, kept pinned now that the F half can succeed:
    // a band miss rejects BEFORE the flute count is even consulted, so
    // the error a flute-carrying tool sees on a between-band diameter
    // is the band's, by name.
    let src = DERIVE_FIXTURE
        .replace("dia=10.0", "dia=20.0, flute_count=3")
        .replace("stepover=6.0", "stepover=12.0")
        .replace("spindle=derive", "feed=derive");
    let err = parse(&src).unwrap_err();
    assert!(err.contains("no chipload band covering dia 20"), "got: {}", err);
}

#[test]
fn test_feed_derive_zero_resolved_spindle_refuses() {
    // A resolved spindle of 0 would make the derived F an honest-looking
    // 0 nobody chose — fork 2B's exact failure mode — so it rejects by
    // name instead of writing it.
    let src = DERIVE_FIXTURE
        .replace("spindle 3000\n", "")
        .replace("dia=10.0", "dia=10.0, flute_count=3")
        .replace("spindle=derive", "feed=derive");
    let err = parse(&src).unwrap_err();
    assert!(err.contains("the resolved spindle is 0"), "got: {}", err);
}

#[test]
fn test_form_tool_dia_is_clearance_so_derive_refuses() {
    // Unit-level on the resolver: a form tool's `dia` is body OD, not a
    // cutting diameter, so the sentinel rejects regardless of table
    // contents.
    let mut p = Part::default();
    p.stock.material = Some("6061".into());
    p.tools.insert(7, Tool { num: 7, name: "ChamferMill".into(), dia: 12.0, profile: None, rounder: None, chamfer: None, flute_len: None, shank_dia: None, neck_dia: None, neck_len: None, stickout: None, holder: None, holder_nose_dia: None, holder_nose_len: None, holder_body_dia: None, flute_count: None, cut_bottom: None, cut_top: None, coolant: None });
    p.operations.push(Operation {
        op_type: OpType::Form,
        tool_num: 7,
        feature_name: "Edge".into(),
        stepover: None,
        stepdown: None,
        scallop: None,
        feed: None,
        plunge: None,
        rapid_z: None,
        clearance_z: None,
        spindle: None,
        feed_derive: false,
        spindle_derive: true,
        chip_thinning: None,
        pass_feeds: None,
        drill_points: None,
        cycle: None,
        peck_increment: None,
        comp: CompMode::Engine,
        compress: CompressMode::Chords,
        direction: None, angle: None, entry: None, lead: None, lead_angle: None, lead_descent: None,
        stock_to_leave: None,
        strategy: None,
        pattern: None,
        engagement: None,
        cover: None,
        voxel_cell: None, coolant: None, n: None,
        wall_standoff: 0.0,
        face_standoff: 0.0,
        tip_clearance: None,
    });
    let err = resolve_derived_words(&mut p).unwrap_err();
    assert!(err.contains("body clearance"), "got: {}", err);
}

#[test]
fn test_feeds_table_directive_shadows_whole_records() {
    // The shop's proven number outranks ours, whole-record (fork 1): a
    // shop 6061×Endmill at 300 m/min doubles the derived RPM.
    let dir = std::env::temp_dir().join("nacs_feeds_shadow_test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("shop.feeds"),
        "feeds 6061 Endmill\nsurface_speed 300\nsource our proven number\n",
    )
    .unwrap();
    let src = DERIVE_FIXTURE.replace(
        "stock Box",
        "feeds_table \"shop.feeds\"\nstock Box",
    );
    let p = parse_project_with_base(&src, None, Some(&dir)).unwrap();
    let expected = derive_spindle_rpm(300.0, 10.0);
    assert!((p.operations[0].spindle.unwrap() - expected).abs() < 1e-9);
    // A second declaration is two opinions in one program — rejected.
    let twice = src.replace(
        "feeds_table \"shop.feeds\"\n",
        "feeds_table \"shop.feeds\"\nfeeds_table \"shop.feeds\"\n",
    );
    let err = parse_project_with_base(&twice, None, Some(&dir)).unwrap_err();
    assert!(err.contains("feeds_table already declared"), "got: {}", err);
}

#[test]
fn test_sentinel_free_program_never_touches_the_table() {
    // The plain spelling still parses to plain numbers and no flags.
    let src = DERIVE_FIXTURE.replace("spindle=derive", "spindle=4000");
    let p = parse(&src).unwrap();
    assert_eq!(p.operations[0].spindle, Some(4000.0));
    assert!(!p.operations[0].spindle_derive && !p.operations[0].feed_derive);
    // And a non-number that isn't the sentinel is still a parse error.
    let src = DERIVE_FIXTURE.replace("spindle=derive", "spindle=fast");
    assert!(parse(&src).unwrap_err().contains("not a number"));
}

/// ROADMAP item 1 slice 9: a loop point may restate the chamfer's leg for
/// the edges from it on. The legs come out per edge from the first point,
/// the runs split the axis path where the leg changes, and the refusals
/// name what a point may and may not carry.
#[test]
fn a_loop_point_restates_the_chamfer_leg_for_the_edges_from_it_on() {
    let head = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\nstock Box(w=160, l=80, h=20, x=0, y=0, z=0)\ntool T3 ChamferMill(angle=90.0, gauge_dia=8.0, gauge_z=3.75, dia=12.0, tip_dia=0.5)\n";
    let part = parse(&format!("{head}feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 chamfer_depth=1.0\n    70 20 chamfer_width=1.5\n    20 20 chamfer_depth=0\n)\nrun Form(tool=T3, surface=Pad)\n")).expect("marks parse");
    let pad = part.features.get("Pad").unwrap();
    let Feature::ProfileBoss { chamfer_marks, .. } = pad else { panic!("a boss") };
    assert_eq!(chamfer_marks.len(), 3);
    assert_eq!(chamfer_marks[0], ChamferMark { at: 1, leg: Some(ChamferLeg::Depth(1.0)) });
    assert_eq!(chamfer_marks[1], ChamferMark { at: 2, leg: Some(ChamferLeg::Width(1.5)) });
    assert_eq!(chamfer_marks[2], ChamferMark { at: 3, leg: None });
    let ec = derive_chamfer(&part, pad).expect("a chamfer");
    assert_eq!(
        derive_chamfer_legs(&ec),
        vec![Some(ChamferLeg::Depth(1.5)), Some(ChamferLeg::Depth(1.0)), Some(ChamferLeg::Width(1.5)), None],
        "the feature's leg from the first point, restated where a point says so"
    );
    let path = derive_edge_round_path(&[Pt { x: 20.0, y: 60.0 }, Pt { x: 70.0, y: 60.0 }, Pt { x: 70.0, y: 20.0 }, Pt { x: 20.0, y: 20.0 }], 0.75).unwrap();
    let runs = derive_chamfer_runs(&ec, &path);
    assert_eq!(runs.len(), 4, "one run per change of leg");
    assert_eq!(runs.iter().map(|r| r.leg).collect::<Vec<_>>(), vec![Some(ChamferLeg::Depth(1.5)), Some(ChamferLeg::Depth(1.0)), Some(ChamferLeg::Width(1.5)), None]);
    assert!(runs.iter().all(|r| r.path.len() >= 2 && r.edge.len() == 2), "each run rides a chain between two authored points: {runs:?}");
    // The runs chain: each ends where the next starts, and the last closes on the first.
    for (a, b) in runs.iter().zip(runs.iter().cycle().skip(1)) {
        assert_eq!(a.path.last().map(|v| (v.x, v.y)), b.path.first().map(|v| (v.x, v.y)));
    }
    validate(&part).expect("engine comp lifts off the unbroken edge");

    // Unmarked: no runs, the loop is one closed pass as it always was.
    let plain = parse(&format!("{head}feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60\n    70 20\n    20 20\n)\n")).unwrap();
    let ec = derive_chamfer(&plain, plain.features.get("Pad").unwrap()).unwrap();
    assert!(ec.marks.is_empty());
    assert!(derive_chamfer_runs(&ec, &path).is_empty());
    assert_eq!(derive_chamfer_legs(&ec), vec![Some(ChamferLeg::Depth(1.5)); 4]);

    // Inch input scales a restated leg with the rest of the geometry.
    let inch = parse("units inch\nfeed 30\nplunge 10\nrapid_z 0.2\nclearance_z 1\nspindle 3000\nstock Box(w=6, l=4, h=1, x=0, y=0, z=0)\nfeature Pad ProfileBoss(extent=0.5, chamfer_depth=0.1\n    1 3\n    3 3 chamfer_depth=0.05\n    3 1\n    1 1\n)\n").unwrap();
    let Feature::ProfileBoss { chamfer_marks, .. } = inch.features.get("Pad").unwrap() else { panic!() };
    assert_eq!(chamfer_marks[0], ChamferMark { at: 1, leg: Some(ChamferLeg::Depth(1.27)) });

    // The refusals.
    let bad = [
        ("feature Pad ProfileBoss(extent=12.0\n    20 60\n    70 60 chamfer_depth=1.0\n    70 20\n    20 20\n)\n", "restates a chamfer the feature does not declare"),
        ("feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 chamfer_angle=45.0\n    70 20\n    20 20\n)\n", "the angle is the loop's"),
        ("feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 chamfer_depth=-1.0\n    70 20\n    20 20\n)\n", "a leg is positive, or 0"),
        ("feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 edge_round=1.0\n    70 20\n    20 20\n)\n", "`edge_round=` is neither"),
        ("feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 1.0\n    70 20\n    20 20\n)\n", "`1.0` is neither"),
        ("feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 chamfer_depth=1.0 chamfer_width=1.0\n    70 20\n    20 20\n)\n", "at most one restated leg"),
    ];
    for (feature, needle) in bad {
        let err = parse(&format!("{head}{feature}")).unwrap_err();
        assert!(err.contains(needle), "`{feature}` gave: {err}");
    }
    // Under control comp the compensated pass is one loop: an unbroken edge is refused.
    let ctl = parse(&format!("{head}feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 chamfer_depth=1.0\n    70 20 chamfer_depth=1.5\n    20 20 chamfer_depth=0\n)\nrun Form(tool=T3, surface=Pad, comp=control)\n")).unwrap_err();
    let err = ctl;
    assert!(err.contains("leaves its edges unbroken (chamfer_depth=0) under comp=control"), "{err}");
    // The deepest restated leg governs the wall check.
    let deep = parse(&format!("{head}feature Pad ProfileBoss(extent=12.0, chamfer_depth=1.5\n    20 60\n    70 60 chamfer_depth=12.0\n    70 20 chamfer_depth=1.5\n    20 20\n)\nrun Form(tool=T3, surface=Pad)\n")).unwrap_err();
    let err = deep;
    assert!(err.contains("as deep as the 12.000mm wall"), "{err}");
}
