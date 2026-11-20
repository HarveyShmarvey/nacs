// ---- the voxel stock engine's two pins (DECISIONS.md "Phase 5's voxel stock
// engine…", slice 1) -----------------------------------------------------------
//
// Pin 1 — flat-vs-octree brute force: at hundreds of thousands of random cell
// centers, the tree's stored value equals the unaccelerated min-over-stamps
// evaluation EXACTLY (bitwise as f32). The octree — its culling, collapse
// margins, and brick indexing — is an accelerator, not an approximation, and
// this is the pin that keeps it one (the march-cap thread's testing lesson:
// ceilings pin at the algorithm's level, not through a golden's resolution).
//
// Pin 2 — the analytic oracle: on fixtures whose `stock.rs` model is
// `complete`, the field agrees with `material_at` outside a band of the
// declared surfaces (the CPU-oracle pattern the wgpu decision committed).
// The agreement is one-sided where the two models honestly diverge: the
// field is the SWEEP's truth and the stock model is the DECLARATION's truth,
// and a finish op may declare a void its passes never sweep (a bore's core
// with no clearing op — cylinder_bore.cam is exactly that program). So:
// wherever the declared model says material stands, the field must agree —
// the gouge-side direction, asserted everywhere; and wherever an op's sweep
// provably covers its declaration (facing's full-width raster, a drill's own
// disc, Clear's island-rejected spiral), the void is asserted two-sided.

use nacs::parse::Part;
use nacs::{gcode, parse, stock, vox};

fn load(dir: &str) -> Part {
    let main_src = std::fs::read_to_string(format!("{dir}/main.part"))
        .unwrap_or_else(|_| panic!("read {dir}/main.part"));
    let geom = std::fs::read_to_string(format!("{dir}/geometry.part")).ok();
    parse::parse_project_with_base(&main_src, geom.as_deref(), Some(std::path::Path::new(dir)))
        .unwrap_or_else(|e| panic!("parse {dir}: {e}"))
}

/// The one silhouette the walker still declines to stamp: a NECKED profile —
/// wide at 4mm above the tip, narrower at 8mm — which can undercut and whose
/// swept void is not upward-closed (DECISIONS.md "the profile silhouette…",
/// slice 2). It is attached AFTER parse on purpose: the front door is shut
/// upstream, since a profile tool can only reach a `Finish` on a `Sphere` and
/// that contact math rejects a non-flaring profile first. Post-parse is how a
/// test reaches the walker's own fence, and the fence is what these tests need
/// — an honestly incomplete field, now that the shaped stamp has retired every
/// other front-door source of one.
fn necked(mut p: Part, tool_num: usize) -> Part {
    p.tools.get_mut(&tool_num).unwrap().profile = Some(vec![
        parse::Pt { x: 0.0, y: 0.0 },
        parse::Pt { x: 5.0, y: 4.0 },
        parse::Pt { x: 2.0, y: 8.0 },
    ]);
    p
}

// The deterministic LCG the house brute-force pins use — reproducible probes,
// not entropy.
fn lcg(state: &mut u64) -> f64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    ((*state >> 11) as f64) / ((1u64 << 53) as f64)
}

/// Pin 1. Three fixtures chosen to exercise every stamp arm: the pocket
/// clear's ramp entry (the sloped covering-interval stamp, flat tool), the
/// sphere's arc rings (full-circle G03 arc-capsules, ball tool, vertical
/// stepdowns), and the drill cycles (point stamps, peck re-plunges).
/// ~390k probes across the three — the 400k-precedent scale.
#[test]
fn test_octree_equals_flat_evaluation_exactly() {
    let fixtures = [
        "profile_pocket_clear.cam",
        "sphere_arcs.cam",
        "cylinder_bore_form_points.cam",
    ];
    let mut state = 0x5eed_0001u64;
    for dir in fixtures {
        let p = load(dir);
        let side = p.stock.w.max(p.stock.l).max(p.stock.h);
        let field = vox::VoxField::build_with_cell(&p, Some(side / 64.0))
            .unwrap_or_else(|e| panic!("{dir}: field must build: {e}"));
        assert!(
            field.complete(),
            "{dir} is engine-comp with rod tools; its field must be complete"
        );
        let n = field.cells();
        for _ in 0..130_000 {
            let i = (lcg(&mut state) * n as f64) as usize;
            let j = (lcg(&mut state) * n as f64) as usize;
            let k = (lcg(&mut state) * n as f64) as usize;
            let (x, y, z) = field.cell_center(i, j, k);
            let tree = field.value(x, y, z);
            let flat = field
                .flat_value(x, y, z)
                .expect("a built field carries its stamps");
            assert!(
                tree.to_bits() == flat.to_bits(),
                "{dir}: tree {tree} != flat {flat} at cell ({i}, {j}, {k}) = ({x}, {y}, {z})"
            );
        }
    }
}

/// Pin 2, direction one — the gouge side, asserted on every probe: wherever
/// the complete declared model says material stands (a band clear of every
/// removal surface and of the stock's own faces), the field must not claim
/// void. The band covers the cell granularity (a probe's cell center sits up
/// to half a cell diagonal away), the chord tolerance the waterline rings are
/// flattened to, and the writer's 1e-3 rounding.
#[test]
fn test_field_never_voids_declared_material() {
    let fixtures = ["profile_pocket_clear.cam", "cylinder_bore_form_points.cam", "sphere.cam"];
    let mut state = 0x5eed_0002u64;
    for dir in fixtures {
        let p = load(dir);
        let side = p.stock.w.max(p.stock.l).max(p.stock.h);
        let field = vox::VoxField::build_with_cell(&p, Some(side / 64.0))
            .unwrap_or_else(|e| panic!("{dir}: field must build: {e}"));
        assert!(field.complete(), "{dir}: the field must be complete for the oracle");
        let model = stock::remaining_after(&p, p.operations.len() - 1);
        assert!(model.complete, "{dir}: the declared model must be complete for the oracle");

        let band = field.cell() * 3f64.sqrt() + 0.03;
        let s = &p.stock;
        let mut probed = 0usize;
        for _ in 0..60_000 {
            let x = s.x + lcg(&mut state) * s.w;
            let y = s.y + lcg(&mut state) * s.l;
            let z = (s.z - s.h) + lcg(&mut state) * s.h;
            // Deep inside the stock's own faces…
            let in_core = x > s.x + band
                && x < s.x + s.w - band
                && y > s.y + band
                && y < s.y + s.l - band
                && z > (s.z - s.h) + band
                && z < s.z - band;
            if !in_core {
                continue;
            }
            // …and band-deep inside declared material. `material_at_slack`
            // grows removals in Z only (the plan section keeps its own EPS),
            // so lateral depth is certified by an erosion probe: material at
            // the point and at ±band along every axis. For a surface at the
            // worst diagonal that certifies perpendicular depth >= band/√3,
            // which still covers the half-cell-diagonal the piecewise-constant
            // query can displace a probe by, plus the chord and rounding slop.
            let eroded = model.material_at_slack(x, y, z, band)
                && [
                    (x - band, y, z),
                    (x + band, y, z),
                    (x, y - band, z),
                    (x, y + band, z),
                    (x, y, z - band),
                    (x, y, z + band),
                ]
                .iter()
                .all(|&(px, py, pz)| model.material_at(px, py, pz));
            if !eroded {
                continue;
            }
            probed += 1;
            assert!(
                field.material_at(x, y, z),
                "{dir}: field voids declared standing material at ({x:.3}, {y:.3}, {z:.3})"
            );
        }
        assert!(
            probed > 5_000,
            "{dir}: the material probe set went thin ({probed}) — the oracle stopped testing anything"
        );
    }
}

/// Pin 2, direction two — the void side, asserted only where the sweep
/// provably covers the declaration. The pocket's Clear rejects uncut islands
/// (`ops::clear`), so its declared void eroded by the tool radius plus the
/// band must read void; a drill's removal IS its own swept disc, exact to
/// the flat floor `stock.rs` declares from the same `derive_z_cut`.
#[test]
fn test_swept_declarations_read_void() {
    let mut state = 0x5eed_0003u64;

    // profile_pocket_clear.cam: square 20..80, extent 5, tool r 5, stock top 0.
    {
        let p = load("profile_pocket_clear.cam");
        let field = vox::VoxField::build_with_cell(&p, Some(p.stock.w / 64.0)).unwrap();
        let band = field.cell() * 3f64.sqrt() + 0.03;
        let (lo, hi) = (20.0 + 5.0 + band, 80.0 - 5.0 - band);
        for _ in 0..8_000 {
            let x = lo + lcg(&mut state) * (hi - lo);
            let y = lo + lcg(&mut state) * (hi - lo);
            let z = -5.0 + band + lcg(&mut state) * (5.0 - band - 0.1);
            assert!(
                !field.material_at(x, y, z),
                "pocket interior must be swept void at ({x:.3}, {y:.3}, {z:.3})"
            );
        }
    }

    // cylinder_bore_form_points.cam: holes at (30,40) and (50,40), drill dia 8
    // (the hole is the TOOL's diameter — the `stock.rs` drill rule), z to -15.
    {
        let p = load("cylinder_bore_form_points.cam");
        let side = p.stock.w.max(p.stock.l).max(p.stock.h);
        let field = vox::VoxField::build_with_cell(&p, Some(side / 64.0)).unwrap();
        let band = field.cell() * 3f64.sqrt() + 0.03;
        let r_probe = 4.0 - band;
        assert!(r_probe > 0.5, "the probe annulus collapsed; coarsen the band");
        for &(hx, hy) in &[(30.0, 40.0), (50.0, 40.0)] {
            for _ in 0..6_000 {
                let ang = lcg(&mut state) * std::f64::consts::TAU;
                let rho = lcg(&mut state).sqrt() * r_probe;
                let x = hx + rho * ang.cos();
                let y = hy + rho * ang.sin();
                let z = -15.0 + band + lcg(&mut state) * (15.0 - band - 0.1);
                assert!(
                    !field.material_at(x, y, z),
                    "drilled hole at ({hx}, {hy}) must be void at ({x:.3}, {y:.3}, {z:.3})"
                );
            }
        }
    }
}

/// Slice 2, the `.vox` cache — round trip: a fresh cache hits, the loaded
/// field answers every query bitwise-identically to the built one, carries
/// the same honesty flag, declines `flat_value` (its stamps were derivable
/// state, deliberately not serialized), and re-serializes to the identical
/// bytes. Queries and bytes are the pins — no golden `.vox` exists or ever
/// will (the cache is never a source and never a golden).
#[test]
fn test_vox_cache_round_trip_is_bitwise() {
    let p = load("profile_pocket_clear.cam");
    let cell = Some(p.stock.w / 64.0);
    let built = vox::VoxField::build_with_cell(&p, cell).unwrap();
    let dir = std::path::Path::new("target/vox_cache_tests");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("round_trip.vox");
    built.save(&path).unwrap();

    let loaded = vox::VoxField::load(&path, &p, cell)
        .unwrap()
        .expect("a fresh cache for the same inputs must hit");
    assert_eq!(loaded.complete(), built.complete());
    assert_eq!(loaded.cells(), built.cells());
    assert_eq!(loaded.cell().to_bits(), built.cell().to_bits());
    assert!(
        loaded.flat_value(50.0, 50.0, -2.0).is_none(),
        "a loaded field must decline the flat evaluation rather than lie"
    );

    let mut state = 0x5eed_0004u64;
    for _ in 0..30_000 {
        let n = built.cells();
        let i = (lcg(&mut state) * n as f64) as usize;
        let j = (lcg(&mut state) * n as f64) as usize;
        let k = (lcg(&mut state) * n as f64) as usize;
        let (x, y, z) = built.cell_center(i, j, k);
        assert!(
            built.value(x, y, z).to_bits() == loaded.value(x, y, z).to_bits(),
            "loaded field diverges at cell ({i}, {j}, {k})"
        );
    }

    let path2 = dir.join("round_trip_resaved.vox");
    loaded.save(&path2).unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        std::fs::read(&path2).unwrap(),
        "save -> load -> save must be byte-identical"
    );
}

/// The validity rule: the hash covers everything the field is a function of,
/// so an edited program or a different resolved resolution reads as stale —
/// recomputed, never trusted. The unchanged part still hits, so the rule
/// discriminates rather than always missing.
#[test]
fn test_vox_cache_invalidates_on_program_or_resolution_change() {
    let src = std::fs::read_to_string("profile_pocket_clear.cam/main.part").unwrap();
    let p = parse::parse_project_with_base(
        &src,
        None,
        Some(std::path::Path::new("profile_pocket_clear.cam")),
    )
    .unwrap();
    let cell = Some(p.stock.w / 64.0);
    let field = vox::VoxField::build_with_cell(&p, cell).unwrap();
    let dir = std::path::Path::new("target/vox_cache_tests");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("staleness.vox");
    field.save(&path).unwrap();

    assert!(
        vox::VoxField::load(&path, &p, cell).unwrap().is_some(),
        "the unchanged part must hit its own cache"
    );

    let edited = src.replace("stepover=4.0", "stepover=5.0");
    assert_ne!(edited, src, "the fixture edit must actually bite");
    let p2 = parse::parse_project_with_base(
        &edited,
        None,
        Some(std::path::Path::new("profile_pocket_clear.cam")),
    )
    .unwrap();
    assert!(
        vox::VoxField::load(&path, &p2, cell).unwrap().is_none(),
        "an edited program must read the cache as stale"
    );
    assert!(
        vox::VoxField::load(&path, &p, Some(p.stock.w / 32.0)).unwrap().is_none(),
        "a different resolved resolution must read the cache as stale"
    );
}

/// The cache fails closed: truncation and bit rot (the body checksum) read
/// as stale, never as a field — and `load_or_build` rebuilds over the
/// wreckage and heals the file in passing. A cache must not be able to fail
/// a run its absence would let succeed.
#[test]
fn test_vox_cache_fails_closed_and_heals() {
    let p = load("cylinder_bore_form_points.cam");
    let side = p.stock.w.max(p.stock.l).max(p.stock.h);
    let cell = Some(side / 32.0);
    let dir = std::path::Path::new("target/vox_cache_tests");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("fails_closed.vox");

    let built = vox::VoxField::load_or_build(&p, &path, cell).unwrap();
    assert!(path.exists(), "load_or_build must write the cache it built");
    let good = std::fs::read(&path).unwrap();

    // Truncated mid-write: the tree runs out of bytes.
    std::fs::write(&path, &good[..good.len() * 3 / 4]).unwrap();
    assert!(vox::VoxField::load(&path, &p, cell).unwrap().is_none());

    // One flipped bit in the tail: the body checksum rejects.
    let mut rotted = good.clone();
    let last = rotted.len() - 1;
    rotted[last] ^= 0x01;
    std::fs::write(&path, &rotted).unwrap();
    assert!(vox::VoxField::load(&path, &p, cell).unwrap().is_none());

    // A foreign file: the magic rejects.
    std::fs::write(&path, b"not a vox file at all").unwrap();
    assert!(vox::VoxField::load(&path, &p, cell).unwrap().is_none());

    // The healing path: rebuild over the wreckage, and the rewritten file is
    // valid again — byte-identical to the first build's.
    let healed = vox::VoxField::load_or_build(&p, &path, cell).unwrap();
    assert_eq!(healed.complete(), built.complete());
    assert_eq!(std::fs::read(&path).unwrap(), good, "the healed cache must be the built bytes again");
    assert!(vox::VoxField::load(&path, &p, cell).unwrap().is_some());
}

/// The retired refusal asserts acceptance (DECISIONS.md "the control-comp
/// centerline resolve"): a control-comp op carries its emitter-authored
/// resolved channel, so its program COMPLETES — and the stamps are the tool
/// center's, not the part edge's. A control-only bore wall finish is the
/// probe: the ring band reads void, the bore core the op never swept still
/// stands (the entries ride inside the band), and nothing names control comp.
#[test]
fn test_control_comp_programs_stamp_their_resolved_centerline() {
    let bore = load("cylinder_bore.cam"); // second run is comp=control
    let side = bore.stock.w.max(bore.stock.l).max(bore.stock.h);
    let field = vox::VoxField::build_with_cell(&bore, Some(side / 32.0)).unwrap();
    assert!(
        field.complete(),
        "a control-comp op stamps its resolved centerline; the reasons were {:?}",
        field.incomplete_reasons
    );

    // The control-only variant separates the resolved stamps from the engine
    // op's: dia-40 bore at (50,50), dia-10 tool, ring at radius 15. The
    // entries plunge at (60,50) — inside the band the ring itself sweeps —
    // so the band is the op's only void and the core is the op's residue.
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Bore CylinderBore(dia=40.0, extent=12.0, x=50.0, y=50.0)\n\
run Side(tool=T5, surface=Bore, stepover=5.0, comp=control)\n";
    let p = parse::parse(src).unwrap();
    let field = vox::VoxField::build_with_cell(&p, Some(p.stock.w / 64.0)).unwrap();
    assert!(field.complete(), "reasons: {:?}", field.incomplete_reasons);
    assert!(
        !field.material_at(65.0, 50.0, -6.0),
        "the resolved ring's band (radius 10..20) must be stamped void"
    );
    assert!(
        !field.material_at(50.0, 35.0, -6.0),
        "the band is a full annulus, not just the engage ray"
    );
    assert!(
        field.material_at(44.0, 50.0, -6.0),
        "the core inside the band stands — the resolved channel stamps the sweep the machine makes, nothing more"
    );
    assert!(
        field.material_at(50.0, 50.0, -6.0),
        "the bore center was never swept by a control-only wall finish"
    );
}

/// The error that remains: a NECKED silhouette is not stamped, the program
/// marks the field incomplete, and total accounting keeps the REASONS — which
/// survive the cache round trip, because a loaded field must be able to say
/// why it declines. (Until slice 2 of "the profile silhouette…" this was every
/// profile tool; the shaped stamp took all of them but the necked one.)
#[test]
fn test_a_necked_silhouette_marks_the_field_incomplete() {
    // A faced slab, not the form-cutter fixture: a necked profile cannot ride
    // a `Finish` on a `Sphere` (the contact math rejects it before the walker
    // ever sees it), so the carrier op has to be one that never consults the
    // profile.
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-3.0)\n\
run End(tool=T5, surface=Top, stepover=6.0)\n";
    let form = necked(parse::parse(src).unwrap(), 5);
    let cell = Some(100.0 / 32.0);
    let field = vox::VoxField::build_with_cell(&form, cell).unwrap();
    assert!(
        !field.complete(),
        "a necked tool must mark the field incomplete: its void is not upward-closed"
    );
    assert!(
        field.incomplete_reasons.iter().any(|r| r.contains("narrows going up")),
        "the error must name the necking: {:?}",
        field.incomplete_reasons
    );
    let dir = std::path::Path::new("target/vox_cache_tests");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("reasons_round_trip.vox");
    field.save(&path).unwrap();
    let loaded = vox::VoxField::load(&path, &form, cell)
        .unwrap()
        .expect("an incomplete field's cache is still a valid cache");
    assert_eq!(
        loaded.incomplete_reasons, field.incomplete_reasons,
        "the reasons must survive the cache round trip"
    );
}

/// The slice-3 audit finding, turned into a theorem and pinned: the
/// semi-infinite rod stamp IS the tool body, so at every position an op's
/// emitted path visits, the end-state field is void up the whole body column
/// by construction — a stampable tool can never foul its own program's
/// field. This is why the foul lint keeps its analytic voice (DECISIONS.md
/// "The voxel stock engine, slice 3…"): a field-voiced lint at emitted
/// positions is structurally inert until the stamped cutting envelope and
/// the true body can differ (the profile-tool split). The pin is
/// radius-independent — `Foul` reads only the axis column — so it holds for
/// any body silhouette a future split could hang on these positions.
#[test]
fn test_no_emitted_position_fouls_its_own_field() {
    // The ball case rides the paraboloid BOWL — a cavity cut down into the
    // stock, so the body columns genuinely stand inside the stock (the sphere
    // fixtures' domes poke above the stock top, which zeroes the reach and
    // would leave the pin checking nothing).
    let fixtures: [(&str, f64); 2] = [
        ("profile_pocket_clear.cam", 0.0),
        ("cylinder_bore_form_points.cam", 0.0),
    ];
    for (dir, tip_drop) in fixtures {
        let p = load(dir);
        let side = p.stock.w.max(p.stock.l).max(p.stock.h);
        let field = vox::VoxField::build_with_cell(&p, Some(side / 64.0)).unwrap();
        assert!(field.complete());
        let text = gcode::program(&p).unwrap();
        let mut m = gcode::Motion { rapid: true, x: None, y: None, z: None, f: None };
        let mut checked = 0usize;
        for line in text.lines() {
            if !gcode::apply_move(line, &mut m) {
                continue;
            }
            let (x, y, z) = match (m.x, m.y, m.z) {
                (Some(x), Some(y), Some(z)) => (x, y, z),
                _ => continue,
            };
            let tip = z - tip_drop;
            let reach = p.stock.z - tip;
            if reach <= 0.0 {
                continue;
            }
            let check_result = field.body_clearance(&|_| 0.0, x, y, tip, reach);
            assert!(
                !matches!(check_result, vox::BodyVerdict::Foul { .. }),
                "{dir}: emitted position `{line}` fouls its own field — the rod/body identity broke"
            );
            checked += 1;
        }
        // The drill fixture's cycles are short programs — 19 real positions —
        // so the guard asks only that the pin kept checking, not that the
        // fixture be long.
        assert!(checked > 10, "{dir}: the pin stopped checking anything ({checked})");
    }
}

/// `body_clearance`'s three check results on a known slab: a buried axis is a
/// certified Foul; a thin body in open air is a certified Clear; a body
/// wider than the truncation band is honestly Uncertain (clearance beyond τ
/// is not representable, so it is not certified); and an incomplete field
/// never certifies anything.
#[test]
fn test_body_clearance_verdicts_on_a_faced_slab() {
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-3.0)\n\
run End(tool=T5, surface=Top, stepover=6.0)\n";
    let p = parse::parse(src).unwrap();
    let field = vox::VoxField::build_with_cell(&p, Some(100.0 / 128.0)).unwrap();
    assert!(field.complete());

    // Axis buried in the slab the facing left: certified foul.
    match field.body_clearance(&|_| 5.0, 50.0, 40.0, -15.0, 15.0) {
        vox::BodyVerdict::Foul { z, .. } => {
            assert!(z < -3.0, "the foul must sit inside the slab, got z {z}")
        }
        v => panic!("a buried axis must be a certified Foul, got {v:?}"),
    }
    // A thin body just above the faced floor: certified clear (0.5mm body
    // against a ~1.56mm truncation — certifiable daylight).
    assert_eq!(
        field.body_clearance(&|_| 0.5, 50.0, 40.0, -1.0, 1.0),
        vox::BodyVerdict::Clear
    );
    // A full-radius body in the same air: clearance beyond τ is not
    // representable, so the honest answer is Uncertain, not Clear.
    assert_eq!(
        field.body_clearance(&|_| 5.0, 50.0, 40.0, -1.0, 1.0),
        vox::BodyVerdict::Uncertain
    );
    // An incomplete field certifies nothing, not even a buried axis: it
    // over-states material, and a Foul from an over-statement is an
    // accusation the program may not deserve. (The necked silhouette is the
    // walker's remaining error since the shaped stamp took the rest.)
    let incomplete =
        vox::VoxField::build_with_cell(&necked(p.clone(), 5), Some(100.0 / 128.0)).unwrap();
    assert!(!incomplete.complete());
    assert_eq!(
        incomplete.body_clearance(&|_| 5.0, 10.0, 10.0, -15.0, 15.0),
        vox::BodyVerdict::Uncertain
    );
}

// ---- adaptive clearing, slice 0: per-prefix fields (DECISIONS.md "adaptive
// clearing — the field picks the passes…") --------------------------------------

/// The three-op staircase every prefix pin walks: face the top to -2, clear a
/// pocket to -6, drill a hole to -15 — three disjoint removals, so each
/// prefix's field is separable by where it does and does not claim void.
const PREFIX_SRC: &str = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
tool T7 Drill(dia=8.0)\n\
feature Top Plane(z=-2.0)\n\
feature Pocket ProfilePocket(extent=6.0\n    20 20\n    60 20\n    60 60\n    20 60\n)\n\
feature Hole CylinderBore(dia=10.0, extent=15.0, x=80.0, y=80.0)\n\
run End(tool=T5, surface=Top, stepover=6.0)\n\
run Clear(tool=T5, profile=Pocket, stepover=4.0)\n\
run Form(points=[Hole], tool=T7, cycle=Simple)\n";

/// The replay pin: `build_prefix(p, k, …)` is the stock BEFORE op k — each
/// staged probe flips from material to void at exactly the prefix whose op
/// removes it, prefix 0 is the untouched stock, and the whole-program prefix
/// is bitwise the end-state build (one build path, not two — the saved bytes
/// agree too, hash included). Alongside, the physical law the prefixes must
/// obey: material only ever shrinks, so void at prefix k stays void at every
/// later prefix, probed at random.
#[test]
fn test_prefix_fields_replay_the_op_history() {
    let p = parse::parse(PREFIX_SRC).unwrap();
    let cell = Some(p.stock.w / 64.0);
    let fields: Vec<vox::VoxField> = (0..=3)
        .map(|k| vox::VoxField::build_prefix(&p, k, cell).unwrap())
        .collect();
    for (k, f) in fields.iter().enumerate() {
        assert!(f.complete(), "prefix {k} of an engine-comp rod-tool program must be complete");
        assert_eq!(f.prefix_ops(), k);
    }

    // Staged probes, each ≥2mm from every surface of every state (the
    // piecewise-constant query reads the containing cell's center, up to a
    // half cell diagonal ~1.35mm away at this resolution).
    let faced = (50.0, 50.0, -1.0); // above the -2 facing floor, mid-plate
    let pocket = (40.0, 40.0, -4.0); // pocket interior between the two floors
    let hole = (80.0, 80.0, -10.0); // drill column, far from pocket and floor
    let keeps = (40.0, 40.0, -8.0); // under the pocket floor: never removed
    let flips: [((f64, f64, f64), usize); 3] = [(faced, 1), (pocket, 2), (hole, 3)];
    for &((x, y, z), at) in &flips {
        for (k, f) in fields.iter().enumerate() {
            assert_eq!(
                f.material_at(x, y, z),
                k < at,
                "probe ({x}, {y}, {z}) must flip to void exactly at prefix {at}, wrong at {k}"
            );
        }
    }
    for f in &fields {
        assert!(f.material_at(keeps.0, keeps.1, keeps.2), "under the pocket floor no op cuts");
    }

    // Monotonicity: void is monotone in the prefix, at random cell centers.
    let mut state = 0x5eed_0005u64;
    let n = fields[0].cells();
    for _ in 0..20_000 {
        let i = (lcg(&mut state) * n as f64) as usize;
        let j = (lcg(&mut state) * n as f64) as usize;
        let k = (lcg(&mut state) * n as f64) as usize;
        let (x, y, z) = fields[0].cell_center(i, j, k);
        for w in 0..3 {
            assert!(
                fields[w].material_at(x, y, z) || !fields[w + 1].material_at(x, y, z),
                "material reappeared between prefix {w} and {} at ({x:.3}, {y:.3}, {z:.3})",
                w + 1
            );
        }
    }

    // The end state IS the whole-program prefix: bitwise values, and the
    // serialized bytes (input hash included) are identical.
    let full = vox::VoxField::build_with_cell(&p, cell).unwrap();
    for _ in 0..20_000 {
        let i = (lcg(&mut state) * n as f64) as usize;
        let j = (lcg(&mut state) * n as f64) as usize;
        let k = (lcg(&mut state) * n as f64) as usize;
        let (x, y, z) = full.cell_center(i, j, k);
        assert!(fields[3].value(x, y, z).to_bits() == full.value(x, y, z).to_bits());
    }
    let dir = std::path::Path::new("target/vox_cache_tests");
    std::fs::create_dir_all(dir).unwrap();
    let (pa, pb) = (dir.join("prefix_full_a.vox"), dir.join("prefix_full_b.vox"));
    fields[3].save(&pa).unwrap();
    full.save(&pb).unwrap();
    assert_eq!(
        std::fs::read(&pa).unwrap(),
        std::fs::read(&pb).unwrap(),
        "the whole-program prefix and the end-state build must serialize identically"
    );

    // Past the end is a request for a state the program never reaches.
    assert!(vox::VoxField::build_prefix(&p, 4, cell).is_err());
}

/// The cache keying pin — the `.vox` format's "hash of the prefix it
/// represents" earning the word: a prefix's cache entry hits for exactly its
/// own prefix and reads as stale for every other, `prefix_ops` survives the
/// round trip, and `load_or_build_prefix` heals a wrong-prefix file into the
/// requested one.
#[test]
fn test_prefix_cache_is_keyed_by_the_prefix() {
    let p = parse::parse(PREFIX_SRC).unwrap();
    let cell = Some(p.stock.w / 64.0);
    let dir = std::path::Path::new("target/vox_cache_tests");
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("prefix_keyed.vox");

    let before_clear = vox::VoxField::build_prefix(&p, 1, cell).unwrap();
    before_clear.save(&path).unwrap();
    let hit = vox::VoxField::load_prefix(&path, &p, 1, cell)
        .unwrap()
        .expect("a prefix's own cache must hit");
    assert_eq!(hit.prefix_ops(), 1, "the stamped prefix count must survive the round trip");
    assert_eq!(hit.complete(), before_clear.complete());
    for other in [0usize, 2, 3] {
        assert!(
            vox::VoxField::load_prefix(&path, &p, other, cell).unwrap().is_none(),
            "a prefix-1 cache must read as stale for prefix {other}"
        );
    }
    assert!(
        vox::VoxField::load(&path, &p, cell).unwrap().is_none(),
        "the end-state load must not accept a mid-history cache"
    );

    // Healing across prefixes: asking for prefix 2 over the prefix-1 file
    // rebuilds, rewrites, and the file then answers for 2, not 1.
    let healed = vox::VoxField::load_or_build_prefix(&p, 2, &path, cell).unwrap();
    assert_eq!(healed.prefix_ops(), 2);
    assert!(vox::VoxField::load_prefix(&path, &p, 2, cell).unwrap().is_some());
    assert!(vox::VoxField::load_prefix(&path, &p, 1, cell).unwrap().is_none());
    assert!(vox::VoxField::load_prefix(&path, &p, 9, cell).is_err(), "past the end is an error");
}

/// The incompleteness a prefix carries is the PREFIX's own — the law the
/// rest error reads: an unstampable op cannot poison the stock states that
/// precede it. The unstampable op carries the NECKED silhouette (the walker's
/// remaining error since the shaped stamp landed); a Drill ahead of it stays
/// stamped and complete.
#[test]
fn test_prefix_incompleteness_is_the_prefixs_own() {
    let src = "\
units mm\nfeed 600\nplunge 200\nrapid_z 5\nclearance_z 30\nspindle 4000\n\
stock Box(w=100, l=80, h=25, x=0, y=0, z=0)\n\
tool T1 Endmill(dia=10.0)\n\
tool T7 Drill(dia=8.0)\n\
feature Top Plane(z=-2.0)\n\
feature Hole CylinderBore(dia=8.0, extent=10.0, x=80.0, y=60.0)\n\
run Form(points=[Hole], tool=T7, cycle=Simple)\n\
run End(tool=T1, surface=Top, stepover=6.0)\n";
    let p = necked(parse::parse(src).unwrap(), 1);
    let side = p.stock.w.max(p.stock.l).max(p.stock.h);
    let cell = Some(side / 32.0);
    let before = vox::VoxField::build_prefix(&p, 1, cell).unwrap();
    assert!(
        before.complete(),
        "the field before the necked-tool op must be complete, its reasons are {:?}",
        before.incomplete_reasons
    );
    let after = vox::VoxField::build_prefix(&p, 2, cell).unwrap();
    assert!(!after.complete(), "the prefix containing the necked-tool op stays incomplete");
    assert!(after.incomplete_reasons.iter().any(|r| r.contains("narrows going up")));
}

/// The rest-material report on a program that really does forget material: a
/// bore wall-finished but never cleared. The declarations say the whole
/// cylinder is void; the sweep left an annular core standing between the
/// entry column and the wall band — the report names it, sizes it, and boxes
/// it inside the bore.
#[test]
fn test_rest_report_names_the_forgotten_core() {
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
run Side(tool=T5, surface=Bore, stepover=5.0)\n";
    let p = parse::parse(src).unwrap();
    let report = vox::rest_report(&p, Some(100.0 / 64.0)).unwrap();
    assert!(report.contains("swept field: complete"), "{report}");
    assert!(report.contains("declared model: complete"), "{report}");
    let forgotten_line = report
        .lines()
        .find(|l| l.starts_with("forgotten"))
        .expect("the report must state the forgotten set");
    assert!(
        !forgotten_line.contains("none"),
        "the un-cleared core must be reported: {report}"
    );
    // The standing core: roughly a cylinder r=20 over z -12..-5 plus an
    // annulus 5..20 over z -5..0 minus the entry spoke — on the order of
    // 10^4 mm³. A generous window still catches a sign error, a band error,
    // or a scan that quietly counted nothing.
    let vol: f64 = forgotten_line
        .split("(~")
        .nth(1)
        .and_then(|s| s.split(" mm³").next())
        .and_then(|s| s.parse().ok())
        .expect("the forgotten line must carry a volume");
    assert!(
        (4_000.0..20_000.0).contains(&vol),
        "forgotten volume {vol} mm³ is out of the expected window: {report}"
    );
    let bbox_line = report
        .lines()
        .find(|l| l.trim_start().starts_with("bounding box"))
        .expect("a non-empty forgotten set must be boxed");
    for (lo, hi) in [(20.0, 80.0)] {
        // The whole box must sit inside the bore's own footprint.
        let nums: Vec<f64> = bbox_line
            .split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
            .filter_map(|s| s.parse().ok())
            .collect();
        assert!(
            nums.iter().take(4).all(|&v| v >= lo && v <= hi),
            "the forgotten box must sit inside the bore: {bbox_line}"
        );
    }
}

/// The band is the dial, pinned from both sides on the same cleared pocket.
/// At a coarse band the report is quiet — floor slop lives inside it. At a
/// finer band it stays quiet since 2026-11-10: a square pocket's corner
/// fillets, ~r(√2−1) deep, are material a round tool cannot cut, and the
/// declared model now holds them as standing, so the two truths agree there
/// at any band. (Until then the fine band named them forgotten.)
#[test]
fn test_rest_report_band_governs_the_corner_fillets() {
    let p = load("profile_pocket_clear.cam");
    let coarse = vox::rest_report(&p, Some(p.stock.w / 64.0)).unwrap();
    assert!(
        coarse.contains("forgotten (declared removed, never swept): none"),
        "at the coarse band the fillets must stay inside it: {coarse}"
    );

    // The default CLI resolution (256 along the long edge, band ~0.7mm),
    // under which the fillets used to emerge: declared standing now, they
    // are not forgotten. This pins exactly what `--rest-report` shows.
    let fine = vox::rest_report(&p, Some(p.stock.w / 256.0)).unwrap();
    assert!(
        fine.contains("forgotten (declared removed, never swept): none"),
        "at the fine band the corner fillets are declared standing, not forgotten: {fine}"
    );
}

/// Both decline paths, each naming its reason: a NECKED-tool program's field
/// is incomplete (the sweep side over-states — the walker's remaining error
/// now that the shaped stamp takes every monotone profile), and a Mesh
/// program's declared model is incomplete (the baseline side over-states).
/// Either way the report declines rather than accusing or absolving
/// wrongly — and says which side, and why.
#[test]
fn test_rest_report_declines_with_reasons() {
    let src = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=80, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Top Plane(z=-3.0)\n\
run End(tool=T5, surface=Top, stepover=6.0)\n";
    let form = necked(parse::parse(src).unwrap(), 5);
    let report = vox::rest_report(&form, Some(100.0 / 32.0)).unwrap();
    assert!(report.contains("swept field: INCOMPLETE"), "{report}");
    assert!(report.contains("narrows going up"), "{report}");
    assert!(report.contains("the report declines"), "{report}");
    assert!(!report.contains("forgotten ("), "a declining report makes no claim: {report}");

    let mesh = load("mesh.cam");
    let side = mesh.stock.w.max(mesh.stock.l).max(mesh.stock.h);
    let report = vox::rest_report(&mesh, Some(side / 32.0)).unwrap();
    assert!(report.contains("declared model: INCOMPLETE"), "{report}");
    assert!(report.contains("no closed-form removal"), "{report}");
    assert!(report.contains("the report declines"), "{report}");
}

/// Phase 9 lane B, slice 1 (`DECISIONS.md` → "containment declines by name,
/// slice 1…"): the field voice's two declines. The field voice runs only
/// where the analytic model is incomplete — that is its charter, never a
/// reason to decline — so its reasons are the other two: the FIELD being
/// incomplete, and the op working a feature the stock does not contain.
///
/// The fixture is the field-voice test program with its dome raised to the
/// stock top: the boss finish makes the analytic model incomplete (so the
/// field voice is the authority on op 1), and the dome standing in air means
/// the sweep had no material to leave standing there — a quiet that was
/// never a claim, and now says so.
#[test]
fn test_the_field_voice_declines_on_an_uncontained_dome() {
    let src = "units mm\nfeed 600\nplunge 200\nrapid_z 5\nclearance_z 30\nspindle 4000\n\
        stock Box(w=100, l=80, h=25, x=0, y=0, z=0)\n\
        tool T1 FormTool(dia=10.0, profile=\"tool_profile.svg\")\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Rim ContourPocket(extent=3.0\n    20 20 0\n    80 20 -2\n    80 80 0\n    20 80 -2\n)\n\
        feature Sunk Sphere(dia=20, x=50, y=40, z=0)\n\
        run Side(tool=T5, surface=Rim)\n\
        run Form(tool=T1, surface=Sunk, stepover=3.0)\n";
    let p = parse::parse_project_with_base(src, None, Some(std::path::Path::new("sphere_formtool.cam")))
        .expect("parses");
    let (_, emitted_moves) = gcode::program_with_moves(&p).expect("the uncontained program still emits");
    let findings = vox::body_foul_findings(&p, &emitted_moves);
    assert_eq!(findings.len(), 1, "one op the field reads, one decline: {findings:?}");
    let f = &findings[0];
    assert_eq!(f.family, parse::FindingFamily::Decline);
    assert_eq!(f.subject, parse::FindingSubject::Op { index: 1, feature: "Sunk".into() });
    assert!(
        f.message.starts_with("field body-foul probe declined on 'Sunk' (Form with T1): the stock does not contain 'Sunk' (10.000mm above the stock top z 0.000)"),
        "{}", f.message
    );
    assert!(f.message.contains("is not a claim"), "{}", f.message);

    // THE CONTROL: sink the dome 12mm into the stock — the existing
    // field-voice test's own program — and the same op gets a FOUL, not a
    // decline: the field has material there, and reads it.
    let sunk = src.replace("Sphere(dia=20, x=50, y=40, z=0)", "Sphere(dia=20, x=50, y=40, z=-12)");
    assert_ne!(sunk, src);
    let p = parse::parse_project_with_base(&sunk, None, Some(std::path::Path::new("sphere_formtool.cam")))
        .expect("parses");
    let (_, emitted_moves) = gcode::program_with_moves(&p).expect("emits");
    let findings = vox::body_foul_findings(&p, &emitted_moves);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].family, parse::FindingFamily::FieldBodyFoul, "a foul FOUND stands: {findings:?}");
}

/// The field voice's other reason: the swept field itself incomplete (a
/// necked profile the stamp rejects — "narrows going up"). It used to skip
/// the op wordlessly, which on the one op it was the authority on was a
/// clean bill nobody earned.
///
/// The program: a boss finish first (a rod, which the field voice never
/// reads, and which makes the analytic model incomplete from here on), then
/// a `Floor` with a tool necked after parse — the one op type that never
/// reads a profile, so the necked tool reaches the walker's fence instead of
/// the front door (the `necked` helper's note; a profile tool on anything
/// but a `Sphere` is an invariant the emitter panics on, by design). The
/// field voice is the authority on the facing op, and the field it builds
/// rejects the facing's own stamp.
#[test]
fn test_the_field_voice_declines_on_an_incomplete_field() {
    let src = "units mm\nfeed 600\nplunge 200\nrapid_z 5\nclearance_z 30\nspindle 4000\n\
        stock Box(w=100, l=80, h=25, x=0, y=0, z=0)\n\
        tool T5 Endmill(dia=10.0)\n\
        tool T7 Endmill(dia=10.0)\n\
        feature Top Plane(z=-3.0)\n\
        feature Rim ContourPocket(extent=3.0\n    20 20 0\n    80 20 -2\n    80 80 0\n    20 80 -2\n)\n\
        run Side(tool=T5, surface=Rim)\n\
        run End(tool=T7, surface=Top, stepover=6.0)\n";
    let p = necked(parse::parse(src).expect("parses"), 7);
    let (_, emitted_moves) = gcode::program_with_moves(&p).expect("emits");
    let findings = vox::body_foul_findings(&p, &emitted_moves);
    assert_eq!(findings.len(), 1, "the rod's op is never read; the facing's is the one: {findings:?}");
    let f = &findings[0];
    assert_eq!(f.family, parse::FindingFamily::Decline);
    assert_eq!(f.subject, parse::FindingSubject::Op { index: 1, feature: "Top".into() });
    assert!(f.message.contains("the swept field is incomplete"), "{}", f.message);
    assert!(f.message.contains("narrows going up"), "the field's own reason, quoted: {}", f.message);
}

/// The uncontained form-tool program — the shipped fixture's text with its
/// Clear stripped and its stock lowered, which is the program as it shipped
/// before Phase 10 lane B contained it. `lower_the_form_tool_blank` in
/// `check_tests` is the same substitution; each crate spells its own.
fn uncontained_form_tool_src() -> String {
    let src = std::fs::read_to_string("sphere_formtool.cam/main.part").unwrap();
    let twin = src
        .replace("run Clear(tool=T2, profile=TopDome, stepover=8.0, stepdown=5.0, stock_to_leave=0.5)\n", "")
        .replace("stock Box(w=100, l=80, h=35, x=0, y=0, z=10)", "stock Box(w=100, l=80, h=25, x=0, y=0, z=0)");
    assert_ne!(twin, src, "both substitutions must substitute");
    assert!(!twin.contains("run Clear") && twin.contains("z=0)"), "{twin}");
    twin
}

/// ACCEPTANCE (the roadmap's, slice 1 half): the uncontained form-tool
/// program's rest report stops claiming `forgotten ~1285.3 mm³` — and lane
/// C's `violated … none` — against a stock the dome is not in. The model IS
/// complete (every op has its closed form), so `declared model: complete`
/// still prints and the decline sits under its own key: it is the STOCK that
/// is wrong, a different fact with the opposite sign. (The subject was the
/// shipped fixture until Phase 10 lane B contained it; it is that fixture's
/// lowered, unpaired twin now.)
#[test]
fn test_the_rest_report_declines_for_an_uncontained_feature_the_program_works() {
    let src = uncontained_form_tool_src();
    let p = parse::parse_project_with_base(&src, None, Some(std::path::Path::new("sphere_formtool.cam"))).unwrap();
    let cell = Some(p.stock.w / 64.0);
    let report = vox::rest_report(&p, cell).unwrap();
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(report.contains("stock: UNCONTAINED —\n  - 'TopDome' (Sphere) stands 10.000mm above the stock top z 0.000 — and op [0] works it"), "{report}");
    assert!(report.contains("the report declines"), "{report}");
    assert!(!report.contains("forgotten ("), "no number against air where the part is: {report}");
    assert!(!report.contains("violated ("), "lane C's line declines with it: {report}");

    // CONTROL ONE: declared, never run. Delete the run line and the dome is
    // the outline's "declared, never run" tail — it declines nothing, and the
    // report proceeds (Phase 6 slice 2's case, fork 2A).
    let unrun: String = src.lines().filter(|l| !l.starts_with("run ")).map(|l| format!("{l}\n")).collect();
    assert_ne!(unrun, src);
    let p = parse::parse_project_with_base(&unrun, None, Some(std::path::Path::new("sphere_formtool.cam"))).unwrap();
    let report = vox::rest_report(&p, cell).unwrap();
    assert!(!report.contains("stock: UNCONTAINED"), "{report}");
    assert!(report.contains("forgotten (declared removed, never swept): none"), "{report}");

    // CONTROL TWO: the contained sphere reads exactly as it did — the arm is
    // inert on every contained program, which is every golden's program.
    let p = load("sphere.cam");
    let report = vox::rest_report(&p, Some(p.stock.w / 64.0)).unwrap();
    assert!(!report.contains("stock: UNCONTAINED"), "{report}");
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(report.contains("forgotten (declared removed, never swept): "), "still a claim, because it may be: {report}");

    // Both reasons at once: the freeform pair is incomplete AND uncontained,
    // and the report says both — under two keys — before one no-claim line.
    let mesh = load("mesh.cam");
    let side = mesh.stock.w.max(mesh.stock.l).max(mesh.stock.h);
    let report = vox::rest_report(&mesh, Some(side / 32.0)).unwrap();
    assert!(report.contains("declared model: INCOMPLETE"), "{report}");
    assert!(report.contains("stock: UNCONTAINED —\n  - 'Blob' (Mesh) stands 10.000mm above the stock top z 0.000 — and op [0] works it"), "{report}");
    assert_eq!(report.matches("no claim:").count(), 1, "one no-claim line, the incomplete side's: {report}");
}

// ---- adaptive clearing, slice 1: rest selection pinned (DECISIONS.md
// "adaptive clearing — the field picks the passes…") ---------------------------

/// The partial-drop program: a deeper concentric core Clear, then a rest
/// Clear of the full bore. The inner loops' bands sit inside the prior void
/// with margin past the certifier's threshold, so they drop; the loops that
/// touch standing material — or graze it within τ — stay. The kept set is
/// read back from the emitted program's own loop starts.
const PARTIAL_DROP_SRC: &str = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
tool T5 Endmill(dia=10.0)\n\
feature Core CylinderBore(dia=30.0, extent=14.0, x=50.0, y=50.0)\n\
feature Bore CylinderBore(dia=60.0, extent=12.0, x=50.0, y=50.0)\n\
run Clear(tool=T5, profile=Core, stepover=4.0)\n\
run Clear(tool=T5, profile=Bore, stepover=4.0, strategy=rest, voxel_cell=1.0)\n";

/// The rest op's section of an emitted program, and the loop radii it kept:
/// every loop block opens with a `G00 X<start> Y50.000` rapid (the family's
/// start points sit at angle 0, x = cx + rho), so the kept radii are exactly
/// those X words minus the center.
fn kept_radii(g: &str) -> Vec<f64> {
    // rsplit: the REST op is the last bore-clear section (the prior core
    // Clear emits the same title).
    let section = g.rsplit("(--- bore clear:").next().expect("the rest op must emit its section");
    section
        .lines()
        .filter_map(|l| {
            if !l.starts_with("G00 X") {
                return None;
            }
            let x: f64 = l.split_whitespace().nth(1)?.strip_prefix('X')?.parse().ok()?;
            Some(x - 50.0)
        })
        .collect()
}

/// Whole-loop selection, end to end: the family is 25, 21, 17, 13, 9, 5, 1;
/// the prior core (r 15, floor −14) certifies the 5 and 1 loops' bands void
/// with margin, the 9 loop's band edge sits 1mm from the core wall — inside
/// the certifier's ~1.2mm threshold at this cell, so uncertainty keeps it —
/// and the outer loops stand in material. Dropped loops leave no trace but
/// the audit line.
#[test]
fn test_rest_selection_drops_certified_loops_whole() {
    let p = parse::parse(PARTIAL_DROP_SRC).unwrap();
    let g = gcode::program(&p).unwrap();
    assert!(g.contains("(rest: kept 5 of 7 loops"), "the audit line must count the drop: {g}");
    let kept = kept_radii(&g);
    let expected = [9.0, 13.0, 17.0, 21.0, 25.0]; // innermost-out
    assert_eq!(kept.len(), expected.len(), "kept radii {kept:?}");
    for (k, e) in kept.iter().zip(expected) {
        assert!((k - e).abs() < 1e-6, "kept radii {kept:?} != expected {expected:?}");
    }
}

/// The certification brute force (the slice's stated pin): every DROPPED
/// loop's band re-checked against the flat evaluation — the unaccelerated
/// truth the octree is pinned bitwise to — must be void at every dense
/// sample, at the cutting depth and up the column (the upward-closure lemma
/// checked, not assumed); every KEPT loop must contain a sample within the
/// certifier's threshold of material (or in it), showing the keep was
/// material or honest uncertainty, never a wasted pass the certifier could
/// have taken.
#[test]
fn test_rest_certification_brute_force() {
    let p = parse::parse(PARTIAL_DROP_SRC).unwrap();
    let g = gcode::program(&p).unwrap();
    let kept = kept_radii(&g);
    let field = vox::VoxField::build_prefix(&p, 1, Some(1.0)).unwrap();
    assert!(field.complete());
    let (r, z_cut, cx, cy) = (5.0, -12.0, 50.0, 50.0);
    let need = field.cell() * (3f64.sqrt() / 2.0 + std::f64::consts::SQRT_2 / 2.0);

    let family = [25.0, 21.0, 17.0, 13.0, 9.0, 5.0, 1.0];
    let mut dropped_checked = 0usize;
    for rho in family {
        let is_kept = kept.iter().any(|k| (k - rho).abs() < 1e-6);
        let mut min_flat = f64::INFINITY;
        for ia in 0..360 {
            let ang = std::f64::consts::TAU * ia as f64 / 360.0;
            for il in 0..=20 {
                let rr = (rho - r + 2.0 * r * il as f64 / 20.0).max(0.0);
                let (x, y) = (cx + rr * ang.cos(), cy + rr * ang.sin());
                for z in [z_cut, z_cut + 4.0, z_cut + 8.0, -0.5] {
                    let v = field.flat_value(x, y, z).expect("built field") as f64;
                    if z == z_cut {
                        min_flat = min_flat.min(v);
                    }
                    if !is_kept {
                        assert!(
                            v > 0.0,
                            "dropped loop rho {rho}: material (flat {v}) at ({x:.3}, {y:.3}, {z:.3}) — the certification lied"
                        );
                    }
                }
            }
        }
        if is_kept {
            assert!(
                min_flat < need,
                "kept loop rho {rho}: min flat {min_flat} clears the threshold {need} everywhere — the certifier kept a loop it could have dropped"
            );
        } else {
            dropped_checked += 1;
        }
    }
    assert_eq!(dropped_checked, 2, "the fixture must actually exercise drops");
}

/// The report/op pair, end to end — the slice's closing demonstration: on the
/// wall-finished bore the report names the forgotten core (pinned in
/// test_rest_report_names_the_forgotten_core); adding the rest Clear the
/// report was pointing at, the same report reports NONE — the op cleared
/// exactly what the report found.
#[test]
fn test_rest_clear_answers_the_rest_report() {
    let p = load("cylinder_bore_rest.cam");
    let report = vox::rest_report(&p, Some(100.0 / 64.0)).unwrap();
    assert!(report.contains("swept field: complete"), "{report}");
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(
        report.contains("forgotten (declared removed, never swept): none"),
        "after the rest Clear nothing may remain forgotten: {report}"
    );
}

// ---- adaptive clearing, slice 2: region decomposition (DECISIONS.md
// "adaptive clearing, region decomposition…") ----------------------------------

/// The declaration-side clamp, brute-forced against AUTHORED geometry (never
/// the field): every coordinate the regions op emits — region loops, offsets,
/// ramp points — keeps at least the tool radius of clearance to the declared
/// pocket boundary, from inside. This is the gouge-safety law's testable
/// face: whatever the field got wrong, no emitted point can be closer to a
/// wall than the tool's own radius.
#[test]
fn test_regions_declaration_clamp_brute() {
    let src = std::fs::read_to_string("profile_pocket_regions.cam/main.part").unwrap();
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).unwrap();
    let section = g.split("(--- pocket regions:").nth(1).expect("the regions op must emit");
    let (lo, hi, r) = (15.0, 45.0, 1.5);
    let mut checked = 0usize;
    for line in section.lines() {
        if !line.starts_with("G0") {
            continue;
        }
        let (mut x, mut y) = (None, None);
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix('X') {
                x = v.parse::<f64>().ok();
            }
            if let Some(v) = tok.strip_prefix('Y') {
                y = v.parse::<f64>().ok();
            }
        }
        if let (Some(x), Some(y)) = (x, y) {
            let inside = x > lo && x < hi && y > lo && y < hi;
            let d = (x - lo).min(hi - x).min(y - lo).min(hi - y);
            assert!(
                inside && d >= r - 1e-6,
                "emitted point ({x}, {y}) violates the declaration clamp: {d:.3}mm from the wall < r {r}"
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "the clamp pin stopped checking anything ({checked})");
}

/// The report→op→report loop, regions edition: the dense clear's report
/// names the four corner fillets (~49 mm³ at the default band); after the
/// small tool's regions op the same report reads NONE — the op cleared what
/// the report found to within the band, and what remains below it is exactly
/// the residue the op's own text states.
#[test]
fn test_regions_clear_answers_the_rest_report() {
    let src = std::fs::read_to_string("profile_pocket_regions.cam/main.part").unwrap();
    let full = parse::parse(&src).unwrap();

    let dense_only = src
        .lines()
        .filter(|l| !l.contains("strategy=regions"))
        .collect::<Vec<_>>()
        .join("\n");
    let dense = parse::parse(&dense_only).unwrap();

    // The dense clear's corner fillets are declared standing (2026-11-10),
    // so the report forgets nothing before the regions op; it still answers
    // the field, which finds them, and forgets nothing after.
    let before = vox::rest_report(&dense, None).unwrap();
    assert!(
        before.contains("forgotten (declared removed, never swept): none"),
        "the dense clear's fillets are declared standing, not forgotten: {before}"
    );

    let after = vox::rest_report(&full, None).unwrap();
    assert!(
        after.contains("forgotten (declared removed, never swept): none"),
        "after the regions op the report must read none: {after}"
    );
}

// ---- adaptive clearing, the regions bore arm (DECISIONS.md "adaptive
// clearing, the regions bore arm…") --------------------------------------------

/// The declaration-side clamp on the circle arm, brute-forced against
/// AUTHORED geometry (never the field): every coordinate the bore regions op
/// emits keeps at least the tool radius of clearance to the declared bore
/// wall, from inside — whatever the field got wrong about the lune, no
/// emitted point can be closer to the wall than the tool's own radius.
#[test]
fn test_bore_regions_declaration_clamp_brute() {
    let src = std::fs::read_to_string("bore_regions.cam/main.part").unwrap();
    let p = parse::parse(&src).unwrap();
    let g = gcode::program(&p).unwrap();
    let section = g.split("(--- bore regions:").nth(1).expect("the regions op must emit");
    let (cx, cy, big_r, r) = (38.0, 30.0, 7.0, 1.5);
    let mut checked = 0usize;
    for line in section.lines() {
        if !line.starts_with("G0") {
            continue;
        }
        let (mut x, mut y) = (None, None);
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix('X') {
                x = v.parse::<f64>().ok();
            }
            if let Some(v) = tok.strip_prefix('Y') {
                y = v.parse::<f64>().ok();
            }
        }
        if let (Some(x), Some(y)) = (x, y) {
            let clearance = big_r - ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            assert!(
                clearance >= r - 1e-6,
                "emitted point ({x}, {y}) violates the declaration clamp: {clearance:.3}mm from the bore wall < r {r}"
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "the clamp pin stopped checking anything ({checked})");
}

/// The report as the auditor of the bore arm's contract. The regions op's
/// declared removal is the WHOLE bore; its passes cut only the standing lune
/// — and the two must reconcile: with the regions op the report reads NONE
/// (prior sweep + the lune spiral cover the declaration to the band, the
/// cusp residue standing only below it), and the standing-material delta
/// against the regions-less program is the lune's own volume, nothing more.
#[test]
fn test_bore_regions_clear_answers_the_rest_report() {
    let src = std::fs::read_to_string("bore_regions.cam/main.part").unwrap();
    let full = parse::parse(&src).unwrap();

    let dense_only = src
        .lines()
        .filter(|l| !l.contains("strategy=regions"))
        .collect::<Vec<_>>()
        .join("\n");
    let dense = parse::parse(&dense_only).unwrap();

    let after = vox::rest_report(&full, None).unwrap();
    assert!(
        after.contains("forgotten (declared removed, never swept): none"),
        "with the regions op the report must read none: {after}"
    );

    let standing = |report: &str| -> f64 {
        report
            .lines()
            .find(|l| l.starts_with("standing material"))
            .and_then(|l| l.split("(~").nth(1))
            .and_then(|s| s.split(" mm³").next())
            .and_then(|s| s.parse().ok())
            .expect("the report must state standing material")
    };
    let before = vox::rest_report(&dense, None).unwrap();
    let cleared = standing(&before) - standing(&after);
    // The lune (bore B minus bore A's disc) at 6mm deep is ~131 mm²·6 ≈ 787
    // mm³; the spiral clears it minus the named cusp residue. A window pins
    // the scale without pinning cell-count noise.
    assert!(
        (500.0..900.0).contains(&cleared),
        "the regions op must clear the lune and only the lune: delta {cleared} mm³\nbefore: {before}\nafter: {after}"
    );
}

/// **The form cutter joins the field** (DECISIONS.md → "the profile
/// silhouette…", slice 2). `sphere_formtool.cam` was the one shipped fixture
/// whose swept field came back INCOMPLETE — a profile tool had no stamp — so
/// the rest report declined on it. The shaped stamp retires that refusal: the
/// field is complete, the report answers, and the answer is checked from both
/// sides rather than merely being non-empty.
///
/// The dome is roughed by an endmill and finished with the 45°-cone form
/// tool, and the report ANSWERS — and since Phase 10 lane B's second slice
/// the answer is the pointed tip's ridges alone (see the tail of this test).
#[test]
fn test_the_form_cutter_field_stamps_and_the_report_answers() {
    let p = load("sphere_formtool.cam");
    let field = vox::VoxField::build_with_cell(&p, Some(0.5)).unwrap();
    assert!(
        field.complete(),
        "a monotone profile tool stamps; the reasons were {:?}",
        field.incomplete_reasons
    );

    // The tool's own cutting envelope: at the sphere's equator the contact
    // rides the profile's corner vertex, so the tip reaches a tool radius
    // BELOW the stock top and the material there is swept.
    assert!(
        !field.material_at(45.0, 20.0, -1.0),
        "the wall the form tool traced at the dome's rim must read as swept"
    );
    // And the stock far from the dome is untouched.
    assert!(
        field.material_at(90.0, 70.0, -1.0),
        "a corner of the stock no pass visits must still stand"
    );

    let report = vox::rest_report(&p, Some(0.5)).unwrap();
    assert!(
        report.contains("swept field: complete"),
        "the report must no longer decline on the profile tool: {report}"
    );
    assert!(
        report.contains("declared model: complete"),
        "nor on the declaration: {report}"
    );

    // Phase 10 lane B, slice 0 (`DECISIONS.md` → "Phase 10 lane B, slice 0…"):
    // the shipped fixture contains its dome and pairs a rough, so the stock's
    // decline (Phase 9 lane B's, which stood here 2026-08-22 for a morning)
    // is gone and the report makes a CLAIM where it declined. Under the flat
    // collar that claim read `forgotten 21627 cells (~1289.1 mm³)` — the
    // collar's box below the base plane beside the rim, air a 45° cone never
    // enters. Slice 1 (the tube) is ACCEPTED here: it fell to the pointed
    // tip's ridges alone — 63 cells, ~3.8 mm³, one cell layer at the pole
    // (fork 6, the profile cusp budget, named and left). `Some(0.5)` is a
    // ceiling the builder rounds to 256 cells per axis — 0.391mm, the CLI's
    // own cell — so this reads the CLI's number exactly.
    assert!(!report.contains("stock: UNCONTAINED"), "contained: {report}");
    let forgotten = report
        .lines()
        .find(|l| l.starts_with("forgotten (declared removed, never swept): "))
        .unwrap_or_else(|| panic!("the report claims: {report}"));
    let mm3: f64 = forgotten
        .split("(~")
        .nth(1)
        .and_then(|s| s.split(" mm³").next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("a number, not `none`: {forgotten}"));
    assert!(
        (mm3 - 3.8).abs() < 0.05,
        "the pointed tip's ridges at the pole, the CLI's own number: {forgotten}"
    );
    assert!(
        report.contains("bounding box: x 25.977..33.789, y 16.211..24.023, z 9.961..9.961"),
        "one cell layer at the pole, nothing beside the rim: {report}"
    );

    // At a 6× finer ladder the ridges go too: `forgotten: none` — the
    // DRAFT's "within a band of zero" for the fine-stepover twin.
    let src = std::fs::read_to_string("sphere_formtool.cam/main.part").unwrap();
    let fine = src.replace("surface=TopDome, stepover=3.0", "surface=TopDome, stepover=0.5");
    assert_ne!(fine, src);
    let p = parse::parse_project_with_base(&fine, None, Some(std::path::Path::new("sphere_formtool.cam"))).unwrap();
    let report = vox::rest_report(&p, Some(0.5)).unwrap();
    assert!(
        report.contains("forgotten (declared removed, never swept): none"),
        "the fine twin: nothing the tube claims is unswept: {report}"
    );
}

/// Phase 10 lane B, slice 1 (`DECISIONS.md` → "Phase 10 lane B, slice 1…",
/// fork 8A): the form's tube against the FIELD. Beside the rim, below the
/// base plane, the analytic floor at rho stations versus the field's own
/// swept depth — the rim-area thread's habit, at the field's resolution. And
/// the point probe from both sides: the collar's inner-lower corner stands
/// in both models.
///
/// Then the band twin, where the two models DISAGREE by name: the pointed
/// tip's ridge near the pole (rho 3.6, z 9.4 — between the pole ring and the
/// 45° ring, a 7mm step in rho for a 3mm step in z) stands in the field and
/// the cap claim calls it removed. 2150 cells, ~128.1 mm³ at stepover 3, and
/// 108.4 at 0.5 — a pointed tip's cusps near a pole shrink slowly, because a
/// z-step there is a large rho-step. The undeclared twin reads 3.8 mm³ for
/// the same rings because its wall above the corner is drawn as CUTTING and
/// mows the ridges off; the band twin's is relieved. The DRAFT forecast this
/// 124 mm³ as the collar's slab and the tube did not take it, because it was
/// never the collar's: it is fork 6, the profile cusp budget, named out of
/// the lane by the DRAFT itself (T3). Pinned so that budget registers here.
#[test]
fn test_the_form_tool_tube_agrees_with_the_field_beside_the_rim() {
    let p = load("sphere_formtool.cam");
    let field = vox::VoxField::build_with_cell(&p, Some(0.5)).unwrap();
    assert!(field.complete());
    let after = stock::remaining_after(&p, p.operations.len() - 1);
    assert!(after.complete);
    let band = field.cell() * 3f64.sqrt() + 0.03;

    // The floor beside the rim: lowest z that reads as air, scanned upward
    // from under the rim ring's tip, each model at its own resolution.
    let floor_of = |material: &dyn Fn(f64) -> bool, step: f64| -> f64 {
        let mut z = -6.0;
        while z < 0.0 && material(z) {
            z += step;
        }
        z
    };
    for rho in [10.5, 12.0, 13.5, 15.0, 16.5, 18.0, 19.5] {
        let x = 30.0 + rho;
        let analytic = floor_of(&|z| after.material_at(x, 20.0, z), 0.005);
        let swept = floor_of(&|z| field.material_at(x, 20.0, z), field.cell() / 4.0);
        assert!(
            (analytic - swept).abs() <= band,
            "rho {rho}: the tube's floor {analytic:.3} vs the field's {swept:.3} (band {band:.3})"
        );
    }
    assert!(field.material_at(40.5, 20.0, -4.5), "the field: the collar's inner-lower corner was never swept");
    assert!(after.material_at(40.5, 20.0, -4.5), "the tube: and is not claimed");

    let p = load("sphere_formtool_band.cam");
    let field = vox::VoxField::build_with_cell(&p, Some(0.5)).unwrap();
    let after = stock::remaining_after(&p, p.operations.len() - 1);
    assert!(field.material_at(33.625, 20.0, 9.375), "the field: the pointed tip's ridge near the pole stands");
    assert!(!after.material_at(33.625, 20.0, 9.375), "NAMED REMAINDER (fork 6): the cap claims the nominal surface");
    assert!(
        [(band, 0.0), (-band, 0.0), (0.0, band), (0.0, -band)]
            .iter()
            .all(|(dx, dz)| field.material_at(33.625 + dx, 20.0, 9.375 + dz)),
        "and it stands more than a band from anything swept — the report's own criterion"
    );
    let report = vox::rest_report(&p, Some(0.5)).unwrap();
    let forgotten = report
        .lines()
        .find(|l| l.starts_with("forgotten (declared removed, never swept): "))
        .unwrap_or_else(|| panic!("{report}"));
    let mm3: f64 = forgotten.split("(~").nth(1).and_then(|s| s.split(" mm³").next()).and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("{forgotten}"));
    assert!((mm3 - 128.1).abs() < 0.05, "the band twin's ledge, the cap's over-claim: {forgotten}");
    assert!(report.contains("bounding box: x 19.727..40.039, y 9.570..30.273, z 1.367..9.961"), "{report}");
}

/// **The payoff the whole thread is named for** (DECISIONS.md → "the profile
/// silhouette…", slice 3, and ROADMAP's "Stock-aware planning for concave /
/// form-profile cutters": *"the tool's shoulder or shank can foul remaining
/// material even when the tip clears"*). A dome sunk 12mm into an unroughed
/// stock, finished with the 45°-cone form tool: the tip rides the surface, the
/// cutting edge ends 10mm up, and above that the shank stands in solid stock
/// that no pass opened.
///
/// The analytic voice cannot see it — an unmodeled `ProfileBoss` op runs first
/// and incompleteness is contagious forward, so its model declines for
/// everything after. The field is nothing but path truth and carries on. That
/// is fork 4's division of labour with something actually at stake in it.
#[test]
fn test_the_field_voice_finds_the_shank_the_analytic_voice_cannot() {
    let src = "units mm\nfeed 600\nplunge 200\nrapid_z 5\nclearance_z 30\nspindle 4000\n\
        stock Box(w=100, l=80, h=25, x=0, y=0, z=0)\n\
        tool T1 FormTool(dia=10.0, profile=\"tool_profile.svg\")\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Rim ContourPocket(extent=3.0\n    20 20 0\n    80 20 -2\n    80 80 0\n    20 80 -2\n)\n\
        feature Sunk Sphere(dia=20, x=50, y=40, z=-12)\n\
        run Side(tool=T5, surface=Rim)\n\
        run Form(tool=T1, surface=Sunk, stepover=3.0)\n";
    let p = parse::parse_project_with_base(
        src,
        None,
        Some(std::path::Path::new("sphere_formtool.cam")),
    )
    .expect("parses");
    let (_, emitted_moves) = gcode::program_with_moves(&p).expect("the program generates");

    // The analytic voice is silent on the form-cutter op: its model declined.
    // The analytic voice must not ACCUSE here, or the field voice proves
    // nothing — and since Phase 9 lane B it is not silent either: its one
    // word about T1 is a decline saying why it cannot read the op.
    let analytic = stock::body_foul_warnings(&p);
    assert!(
        !analytic.iter().any(|w| w.family != parse::FindingFamily::Decline && w.message.contains("T1")),
        "the analytic lint must raise no foul here: {analytic:?}"
    );
    assert!(
        analytic.iter().any(|w| w.family == parse::FindingFamily::Decline && w.message.contains("T1")),
        "and it says it declined, by name: {analytic:?}"
    );

    let findings = vox::body_foul_findings(&p, &emitted_moves);
    assert_eq!(findings.len(), 1, "one finding, one op: {findings:?}");
    let f = &findings[0];
    assert!(f.message.contains("T1's body"), "{f}");
    assert!(f.message.contains("'Sunk'"), "{f}");

    // And it is the SHANK that fouls, not the cutting edge: the height is
    // above the profile's 10mm cutting top, where the tool removes nothing.
    let h: f64 = f
        .message.split("body ")
        .nth(1)
        .and_then(|s| s.split("mm above").next())
        .and_then(|s| s.parse().ok())
        .expect("the finding must quote a height above the tip");
    assert!(h > 10.0, "the foul must sit above the cutting top, got {h}mm");
}

/// The other half of fork 4, on the SAME geometry: drop the unmodeled op and
/// the dome's model is exact again — at which point the analytic machinery
/// owns the program and says so at the top of its voice, rejecting outright
/// (the entry restriction, `stock::check_moves`) rather than advising. One
/// sunk dome, two regimes: where the declared model is exact it is the
/// authority and this program does not exist; where it declines, the field
/// voice is the only thing left that can see the shank.
///
/// It also states the honest limit. The analytic model's incompleteness is
/// contagious forward, so the same program with a bare ROD tool gets neither
/// voice: an undeclared rod's stamp is its body — the theorem — and the
/// declared model has already declined. A rod plowing into unroughed stock is
/// the plunge taxonomy's question, not the body's. (A rod that DECLARES its
/// flute_len is different since K2 slice 2 — its stamp truncates and the
/// field voice opens: test_the_field_voice_opens_for_a_rod_with_declared_flutes.)
#[test]
fn test_an_exact_model_owns_the_op_and_refuses_outright() {
    let src = "units mm\nfeed 600\nplunge 200\nrapid_z 5\nclearance_z 30\nspindle 4000\n\
        stock Box(w=100, l=80, h=25, x=0, y=0, z=0)\n\
        tool T1 FormTool(dia=10.0, profile=\"tool_profile.svg\")\n\
        feature Sunk Sphere(dia=20, x=50, y=40, z=-12)\n\
        run Form(tool=T1, surface=Sunk, stepover=3.0)\n";
    let p = parse::parse_project_with_base(
        src,
        None,
        Some(std::path::Path::new("sphere_formtool.cam")),
    )
    .expect("parses");
    let err = match gcode::program_with_moves(&p) {
        Err(e) => e,
        Ok(_) => panic!("an exact model must reject this program"),
    };
    assert!(
        err.contains("drives the tool into material that outlives this operation"),
        "got: {err}"
    );

    // And the shipped fixture — same tool, model exact, program valid — draws
    // no field-voiced warning: nothing is double-voiced.
    let fixture = load("sphere_formtool.cam");
    let (_, emitted_moves) = gcode::program_with_moves(&fixture).expect("the fixture generates");
    assert!(vox::body_foul_findings(&fixture, &emitted_moves).is_empty());
}

// ---- the isosurface, pinned against answers geometry already knows ----------
//
// DECISIONS.md → "the field is already undercut-capable and the RENDERER
// throws it away", slice 0. The extraction is the half that CAN be checked,
// so it is checked here before anything draws it — the order the wire and
// sliver pins were built in. What these hold, in order of how confidently a
// picture would lie otherwise:
//   (1) the surface lands on the declared geometry, to the field's own
//       resolution — a bore's wall at its declared radius;
//   (2) every triangle faces OUT of material, because a mesh with inverted
//       winding renders as an inside-out shell under a depth test and looks
//       merely "wrong-ish" rather than obviously broken;
//   (3) it is watertight — every edge shared by exactly two triangles — which
//       is the property that makes the depth test meaningful at all;
//   (4) it extracts the form-cutter fields the heightfield view REJECTS,
//       which is the entire point of the thread.

/// The wall of a cleared bore stands at its declared radius. The pin is on
/// the extracted VERTICES, not on a rendering of them: every vertex on the
/// bore wall (below the rim, off the floor) must sit within a cell of the
/// declared radius, and the extremes must actually reach it — a surface that
/// merely stayed inside the tolerance band without touching the wall would
/// pass a one-sided check while drawing a bore that is too small.
#[test]
fn test_the_isosurface_lands_on_the_declared_bore_wall() {
    let p = load("cylinder_bore.cam");
    let f = vox::VoxField::build_with_cell(&p, Some(0.5)).expect("the bore field builds");
    let iso = f.iso_surface();
    assert!(!iso.verts.is_empty(), "a cut stock has a surface");
    assert!(!iso.tris.is_empty(), "a surface has triangles");

    // cylinder_bore.cam: CylinderBore(dia=40) at (50,50) in a 100x100x20 stock,
    // extent 12, finished by a dia-10 Endmill. Two DECLARED surfaces are
    // analytic and undisturbed, one curved and one planar, and pinning both
    // is much harder to pass by accident than pinning either: the bore wall
    // at R=20, and the stock's top face at z=0 out where nothing was cut.
    //
    // The core the finish pass leaves standing is deliberately NOT pinned. It
    // is not a clean cylinder — this program never clears, and the passes'
    // entry moves cut into the middle — so its radius is a fact about the
    // toolpath rather than about a declaration, and a pin on it would be
    // pinning today's lead-in geometry by accident.
    let (cx, cy) = (50.0f64, 50.0f64);
    let rad = |v: &[f64; 3]| ((v[0] - cx).powi(2) + (v[1] - cy).powi(2)).sqrt();

    let wall: Vec<f64> = iso
        .verts
        .iter()
        .filter(|v| v[2] < -2.0 && v[2] > -10.0)
        .map(rad)
        .filter(|d| *d > 16.0 && *d < 24.0)
        .collect();
    assert!(wall.len() > 200, "the wall should extract many vertices, got {}", wall.len());
    let worst = wall.iter().map(|d| (d - 20.0).abs()).fold(0.0f64, f64::max);
    assert!(worst < 1.0, "a wall vertex strayed {worst:.3} mm from the declared R=20");
    // Both sides: a surface hugging the inside of the tolerance band would
    // pass a one-sided check while drawing a bore of the wrong size.
    let hi = wall.iter().fold(f64::MIN, |m, d| m.max(*d));
    let lo = wall.iter().fold(f64::MAX, |m, d| m.min(*d));
    assert!(hi > 19.5 && lo < 20.5, "the wall never reached R=20 (spans {lo:.3}..{hi:.3})");

    // The uncut faces, in the annulus between the bore and the stock's edge.
    // BOTH of them: the extraction is a closed shell, so the stock's bottom
    // is in there too — and pinning the pair also pins the datum convention
    // `stock Box(h=20, z=0)` means, which is that the stock HANGS BELOW its
    // z rather than standing on it.
    let faces: Vec<f64> = iso
        .verts
        .iter()
        .filter(|v| {
            let d = rad(v);
            d > 25.0 && d < 35.0
        })
        .map(|v| v[2])
        .collect();
    assert!(faces.len() > 400, "the two faces should extract many vertices, got {}", faces.len());
    for (name, want) in [("top", 0.0f64), ("bottom", -20.0f64)] {
        let got: Vec<f64> = faces.iter().copied().filter(|z| (z - want).abs() < 10.0).collect();
        assert!(got.len() > 200, "the {name} face should extract many vertices, got {}", got.len());
        let off = got.iter().map(|z| (z - want).abs()).fold(0.0f64, f64::max);
        assert!(off < 1.0, "a {name}-face vertex stood {off:.3} mm off z={want}");
    }
    // Nothing outside the stock's own slab, which is the shell being closed
    // stated as a fact about where material can be at all.
    let stray = faces.iter().filter(|z| **z > 1.0 || **z < -21.0).count();
    assert_eq!(stray, 0, "{stray} vertices stood outside the stock's slab");
}

/// Every triangle faces out of material. Checked against the FIELD rather
/// than against itself: step a short way along each triangle's geometric
/// normal and the field must read less negative than a step the other way —
/// the direction "out" means, asked of the only thing that knows.
#[test]
fn test_every_triangle_faces_out_of_material() {
    let p = load("cylinder_bore.cam");
    let f = vox::VoxField::build_with_cell(&p, Some(0.5)).expect("the bore field builds");
    let iso = f.iso_surface();

    let step = 0.75;
    let mut checked = 0;
    let mut wrong = 0;
    for t in &iso.tris {
        let (a, b, c) = (
            iso.verts[t[0] as usize],
            iso.verts[t[1] as usize],
            iso.verts[t[2] as usize],
        );
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * w[2] - u[2] * w[1],
            u[2] * w[0] - u[0] * w[2],
            u[0] * w[1] - u[1] * w[0],
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len < 1e-12 {
            continue; // a degenerate triangle names no direction
        }
        let ctr = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        let out = |sgn: f64| {
            f.value(
                ctr[0] + sgn * step * n[0] / len,
                ctr[1] + sgn * step * n[1] / len,
                ctr[2] + sgn * step * n[2] / len,
            )
        };
        checked += 1;
        if out(1.0) <= out(-1.0) {
            wrong += 1;
        }
    }
    assert!(checked > 500, "the pin must see real triangles, saw {checked}");
    // Exactly zero: winding is a property of the algorithm, not a statistic.
    assert_eq!(wrong, 0, "{wrong} of {checked} triangles faced INTO material");
}

/// Watertight: every edge is shared by exactly two triangles. This is the
/// property a depth test depends on — a crack lets the inside of the shell
/// show through, and a crack is exactly what a per-brick extraction produces
/// if the vertices are not welded across brick boundaries.
#[test]
fn test_the_isosurface_is_watertight() {
    use std::collections::HashMap;
    let p = load("cylinder_bore.cam");
    let f = vox::VoxField::build_with_cell(&p, Some(0.5)).expect("the bore field builds");
    let iso = f.iso_surface();

    let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
    for t in &iso.tris {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let key = if a < b { (a, b) } else { (b, a) };
            *edges.entry(key).or_insert(0) += 1;
        }
    }
    let open: Vec<_> = edges.iter().filter(|(_, &n)| n != 2).collect();
    assert!(
        open.is_empty(),
        "{} of {} edges are not shared by exactly two triangles",
        open.len(),
        edges.len()
    );
}

/// The thread's whole point: a field the heightfield view REJECTS extracts
/// without complaint. `sphere_formtool.cam` stamps a truncated (form-cutter)
/// silhouette, so `upward_closed` is false and the retired heightfield view
/// declined it by name —
/// while the field itself has always known the shape.
#[test]
fn test_the_isosurface_extracts_what_the_heightfield_refuses() {
    let p = load("sphere_formtool.cam");
    let f = vox::VoxField::build_with_cell(&p, Some(0.6)).expect("the form-tool field builds");
    assert!(
        !f.upward_closed(),
        "this fixture is only interesting because it is NOT upward-closed"
    );
    let iso = f.iso_surface();
    assert!(
        iso.verts.len() > 1000 && iso.tris.len() > 1000,
        "the rejected field still has a surface: {} verts, {} tris",
        iso.verts.len(),
        iso.tris.len()
    );
    assert!(
        iso.verts.iter().all(|v| v.iter().all(|c| c.is_finite())),
        "every extracted vertex is finite"
    );
    assert!(
        iso.normals.iter().all(|n| {
            let m = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            (m - 1.0).abs() < 1e-9
        }),
        "every normal is a unit vector"
    );
}

// ---- K2 slice 2, fork 5A: a declared flute_len truncates the rod's stamp
// (DECISIONS.md "K2, the tool body") — the field derives upward_closed false,
// rest declines per loop, regions rejects by name, and the rod gains the foul
// lint's field voice. Undeclared rods are untouched, so every pin here is on
// a program that says flute_len out loud. -----------------------------------

/// PARTIAL_DROP_SRC with the one difference under test: the tool declares
/// where its flutes end. Same geometry, same family, same certifier — and the
/// field is no longer upward-closed, so `band_certified_void` answers false
/// for every loop and the rest op honestly cuts all seven as spring passes
/// where the undeclared twin drops two.
#[test]
fn test_a_declared_flute_len_declines_rest_selection() {
    let src = PARTIAL_DROP_SRC.replace(
        "tool T5 Endmill(dia=10.0)",
        "tool T5 Endmill(dia=10.0, flute_len=12.0)",
    );
    let p = parse::parse(&src).unwrap();
    let field = vox::VoxField::build_prefix(&p, 1, Some(1.0)).unwrap();
    assert!(field.complete(), "a truncated rod still stamps exactly: {:?}", field.incomplete_reasons);
    assert!(!field.upward_closed(), "declared flutes end, so the sweep is not upward-closed");
    let g = gcode::program(&p).unwrap();
    assert!(
        g.contains("(rest: kept 7 of 7 loops"),
        "the certifier must decline every loop under a truncated prefix: {g}"
    );
    // And the column above the flutes really is the difference: the core's
    // walls were cut with the tip at -14, so 12mm of flutes end at -2 and the
    // last 2mm of the core's rim column was never swept.
    assert!(
        field.material_at(50.0 + 14.9, 50.0, -0.5),
        "material above the flute cap must stand where the undeclared rod claimed void"
    );
}

/// The regions arm cannot decline per loop — its work-mask reads ONE field
/// layer under the upward-closure lemma, and a blind mask would silently miss
/// standing work. So it rejects by name, like the incompleteness law.
#[test]
fn test_regions_refuses_a_truncated_prefix_by_name() {
    let regions_src = PARTIAL_DROP_SRC.replace("strategy=rest", "strategy=regions");
    parse::parse(&regions_src)
        .map(|p| gcode::program(&p).expect("an unbounded prefix keeps regions"))
        .expect("the control twin parses");
    let src = regions_src.replace(
        "tool T5 Endmill(dia=10.0)",
        "tool T5 Endmill(dia=10.0, flute_len=12.0)",
    );
    let p = parse::parse(&src).unwrap();
    let err = gcode::program(&p).expect_err("a truncated prefix must reject regions");
    assert!(err.contains("upward-closure lemma"), "{err}");
    assert!(err.contains("flute_len"), "the error must name the rod's own spelling: {err}");
}

/// Fork 5's second stated consequence: a rod with declared flutes gains the
/// foul lint's FIELD voice — the gate is a truncated silhouette, and this rod
/// now has one. Same sunk-dome geometry as the form-cutter payoff test, same
/// silenced analytic voice (the unmodeled control-comp op runs first), and
/// the ball's shank stands in stock its flutes never opened.
#[test]
fn test_the_field_voice_opens_for_a_rod_with_declared_flutes() {
    let src = "units mm\nfeed 600\nplunge 200\nrapid_z 5\nclearance_z 30\nspindle 4000\n\
        stock Box(w=100, l=80, h=25, x=0, y=0, z=0)\n\
        tool T1 Ballmill(dia=10.0, flute_len=8.0)\n\
        tool T5 Endmill(dia=10.0)\n\
        feature Rim ContourPocket(extent=3.0\n    20 20 0\n    80 20 -2\n    80 80 0\n    20 80 -2\n)\n\
        feature Sunk Sphere(dia=20, x=50, y=40, z=-12)\n\
        run Side(tool=T5, surface=Rim)\n\
        run Copy(tool=T1, surface=Sunk, stepover=3.0)\n";
    let p = parse::parse(src).expect("parses");
    let (_, emitted_moves) = gcode::program_with_moves(&p).expect("the program generates");

    // The analytic voice must not ACCUSE here, or the field voice proves
    // nothing — and since Phase 9 lane B it is not silent either: its one
    // word about T1 is a decline saying why it cannot read the op.
    let analytic = stock::body_foul_warnings(&p);
    assert!(
        !analytic.iter().any(|w| w.family != parse::FindingFamily::Decline && w.message.contains("T1")),
        "the analytic lint must raise no foul here: {analytic:?}"
    );
    assert!(
        analytic.iter().any(|w| w.family == parse::FindingFamily::Decline && w.message.contains("T1")),
        "and it says it declined, by name: {analytic:?}"
    );
    let findings = vox::body_foul_findings(&p, &emitted_moves);
    assert_eq!(findings.len(), 1, "one finding, one op: {findings:?}");
    let f = &findings[0];
    assert!(f.message.contains("T1's body"), "{f}");
    let h: f64 = f
        .message
        .split("body ")
        .nth(1)
        .and_then(|s| s.split("mm above").next())
        .and_then(|s| s.parse().ok())
        .expect("the finding must quote a height above the tip");
    assert!(h > 8.0, "the foul must sit above the declared flute top, got {h}mm");

    // The control: strip the declaration and the theorem returns — a bare
    // rod's stamp is its body, and the field voice's gate stays shut.
    let bare = src.replace(", flute_len=8.0", "");
    let p = parse::parse(&bare).expect("parses");
    let (_, emitted_moves) = gcode::program_with_moves(&p).expect("generates");
    assert!(
        vox::body_foul_findings(&p, &emitted_moves).is_empty(),
        "an undeclared rod must stay exactly today's silence"
    );
}

// ---- Phase 9 lane C, slice 0: the swept field against the DECLARED PART
// (DECISIONS.md → "Phase 9 lane C — over-cut of the declared part, in the
// field…" and its landing entry) -------------------------------------------

/// `pocket_post.cam` as it cut before Phase 26: the post swept by its own
/// pocket's Clear. The post is lifted 2µm off the pocket's floor AFTER the
/// parse — "a boss floating above a cavity's floor", which `validate` refuses
/// and `stock::islands_of` therefore does not call an island — so the emitter
/// is blind to it again, and the declaration is the same solid to the cell
/// (the lowest cell centre is 39µm up). A program the front door would turn
/// away, which is the point: the field voice judges the sweep it is handed.
/// (Lifted, not made taller: 2µm above the stock top is a containment
/// finding, and the report then rightly declines.)
fn swept_post() -> parse::Part {
    let mut p = load("pocket_post.cam");
    match p.features.get_mut("Post") {
        Some(parse::Feature::CylinderBoss { extent, z, .. }) => {
            *z += 0.002;
            *extent -= 0.002;
        }
        other => panic!("the post is a round boss: {other:?}"),
    }
    assert!(nacs::stock::islands_of(&p, "PocketProfile").is_empty(), "blind to the post again");
    p
}

/// And the shipped fixture, as it cuts NOW (Phase 26 slice 2's acceptance):
/// the post is an island, the Clear goes around it, and the 388.1 mm³ is gone
/// by construction — the removal never claimed it and the sweep never took it.
/// Nothing is forgotten: the four corner radii are declared standing too.
#[test]
fn test_the_shipped_post_survives_its_pocket() {
    let p = load("pocket_post.cam");
    let report = vox::rest_report(&p, None).unwrap();
    assert!(report.contains("swept field: complete") && report.contains("declared model: complete"), "{report}");
    assert!(report.contains("violated (declared part, swept anyway): none"), "{report}");
    assert!(report.contains("forgotten (declared removed, never swept): none"), "{report}");
}

/// ACCEPTANCE (the roadmap's): the post the pocket ramps into becomes a named
/// finding from the field voice. `pocket_post.cam` is `profile_pocket_clear`
/// plus one line, and that line is a 12mm post standing dead centre in the
/// footprint the Clear sweeps. Every analytic voice is silent by construction
/// — the end-state model reads the post as removed — so the rest report is the
/// only place this can be said, and it says it about the FEATURE.
///
/// Phase 26 slice 2: the shipped fixture no longer does this — the pocket's
/// Clear goes AROUND the post, and `pocket_post.cam` is pinned below reading
/// `none`. The over-cut check still has to be shown a sweep that takes a
/// declared solid, and must not lean on the emitter being right: `swept_post`
/// is the old defect, rebuilt.
#[test]
fn test_the_post_the_pocket_ramps_into_is_a_named_violation() {
    let p = swept_post();
    let report = vox::rest_report(&p, None).unwrap();
    assert!(report.contains("swept field: complete"), "{report}");
    assert!(report.contains("declared model: complete"), "{report}");
    let line = report
        .lines()
        .find(|l| l.starts_with("violated"))
        .expect("the report must state the violated set");
    assert!(!line.contains("none"), "the post must be reported: {report}");
    assert!(report.contains("'Post' (CylinderBoss): the sweep removed material the declaration says stands"), "{report}");

    // The same text as a finding, about the feature — the containment lint's
    // shape — from the function a caller holding the field reads.
    let field = vox::VoxField::build(&p).unwrap();
    let found = vox::part_violations(&p, &field);
    assert_eq!(found.len(), 1, "one claimant, one finding: {found:?}");
    assert_eq!(found[0].family, parse::FindingFamily::PartViolation);
    assert_eq!(found[0].subject, parse::FindingSubject::Feature { name: "Post".into() });
    assert!(report.contains(&found[0].message), "the report prints the finding's own text, not a second wording");

    // The volume, in a window that catches a sign error, a band error, or an
    // attribution that lost the base: the post is 565mm³ of declared solid,
    // and what counts is the part of it more than one band (0.707mm) inside
    // the PART — which at the base includes the stock below the pocket floor,
    // so the count runs deeper than the post's own core. Measured 388.1mm³
    // at the default 0.391mm cell (6,512 cells); the DRAFT's 317.6mm³ was the
    // stricter probe that required the neighbours inside the POST alone.
    let mm3: f64 = found[0]
        .message
        .split("(~")
        .nth(1)
        .and_then(|s| s.split(" mm³").next())
        .and_then(|s| s.parse().ok())
        .expect("the finding states its volume");
    assert!((350.0..=430.0).contains(&mm3), "the post's lost volume: {mm3}");
    // The lowest violated cell is at the post's base — the ramp went all the
    // way down — and the box is the post's own footprint.
    assert!(found[0].message.contains("the lowest at (") && found[0].message.contains(", -4.961)"), "{}", found[0].message);
    assert!(found[0].message.contains("bounding box x 44.727..55.273, y 44.727..55.273"), "{}", found[0].message);

    // SLICE 1 (DECISIONS.md → "Phase 9 lane C, slice 1 lands…"): WHO cut it,
    // read from the built field's own stamps — the pocket's Clear, every
    // cell of it, and at the post's base by its ENTRY. Measurement 3's
    // answer, stated by the engine. (The entry was a ramp down the first
    // loop until Phase 44 slice 1; that loop is under a diameter round, so
    // it is now a plunge at the plunge feed, said by name in the program.)
    assert!(
        found[0].message.contains("Swept by Clear on 'PocketProfile' (op [0]): all of it; at its lowest by a plunge of op [0]."),
        "the op and the move kind: {}",
        found[0].message
    );
}

/// SLICE 1's correction to the blessed fork 3A, measured: "solid wins" was
/// argued for a boss inside a pocket and is right there — but a bore declared
/// coaxially INSIDE the boss and drilled is a hollow post, a real part, and
/// under plain solid-wins it read as 100.9mm³ of violation. The narrower
/// declaration wins: a void NESTED in the standing footprint is the
/// feature's own. A bore that STRADDLES the post's wall is not nested, and a
/// hole through a post wall is a violation the Drill is named for.
#[test]
fn test_a_void_nested_in_a_boss_is_the_boss_own_and_a_straddling_one_is_not() {
    let base = "\
units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
stock Box(w=100, l=100, h=20, x=0, y=0, z=0)\n\
tool T7 Drill(dia=6.0)\n\
feature Post CylinderBoss(dia=12, extent=5, x=50, y=50, z=-5)\n\
feature Hole CylinderBore(dia=6, extent=12, x=HX, y=50)\n\
run Form(points=[Hole], tool=T7, cycle=Simple)\n";

    // Coaxial: the hole is the post's own. Silent.
    let hollow = parse::parse(&base.replace("HX", "50")).unwrap();
    let report = vox::rest_report(&hollow, None).unwrap();
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(report.contains("violated (declared part, swept anyway): none"), "a hollow post is a part, not a violation: {report}");

    // Offset 4mm on a 6mm post radius: the 3mm hole runs from r 1 to r 7 and
    // breaks the wall. Reported, sized, and attributed to the Drill's plunge.
    //
    // Phase 26 slice 1: `validate` refuses this declaration now (lane D fork
    // 5A — a void that crosses a standing wall has no meaning), so the program
    // never reaches the field through the front door. The over-cut check is
    // still the authority for what a sweep takes, and must not lean on that
    // gate having run: the hole is moved AFTER the parse.
    let refused = parse::parse(&base.replace("HX", "54")).expect_err("a straddle refuses");
    assert_eq!(
        refused,
        "'Hole' straddles 'Post''s wall — a feature is inside another's footprint (an island, or a hole) or outside it; neither has a meaning in this vocabulary"
    );
    let mut straddle = parse::parse(&base.replace("HX", "50")).unwrap();
    match straddle.features.get_mut("Hole") {
        Some(parse::Feature::CylinderBore { x, .. }) => *x = 54.0,
        other => panic!("the hole is a bore: {other:?}"),
    }
    let field = vox::VoxField::build(&straddle).unwrap();
    let found = vox::part_violations(&straddle, &field);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].subject, parse::FindingSubject::Feature { name: "Post".into() });
    assert!(found[0].message.contains("Swept by Form (op [0]): all of it; at its lowest by a plunge of op [0]."), "{}", found[0].message);
    let mm3: f64 = found[0].message.split("(~").nth(1).and_then(|s| s.split(" mm³").next()).and_then(|s| s.parse().ok()).unwrap();
    assert!((55.0..=90.0).contains(&mm3), "the part of the hole inside the post wall, banded: {mm3}");
}

/// THE CONTROLS, each one a way the check could lie and does not.
#[test]
fn test_the_part_violation_controls_stay_silent_or_decline_by_name() {
    let src = std::fs::read_to_string("pocket_post.cam/main.part").unwrap();

    // The program without the post: nothing to violate.
    let base = load("profile_pocket_clear.cam");
    let report = vox::rest_report(&base, None).unwrap();
    assert!(report.contains("violated (declared part, swept anyway): none"), "{report}");

    // The post OUTSIDE the pocket, standing in stock the Clear never visits:
    // the declaration is honoured and the report must not invent a loss.
    let outside = src.replace("x=50, y=50, z=-5", "x=90, y=90, z=-5");
    assert_ne!(outside, src);
    let p = parse::parse(&outside).unwrap();
    let report = vox::rest_report(&p, None).unwrap();
    assert!(report.contains("violated (declared part, swept anyway): none"), "{report}");
    let field = vox::VoxField::build(&p).unwrap();
    assert!(vox::part_violations(&p, &field).is_empty());

    // The post FINISHED. When this lane landed, the boss finish had no
    // closed-form removal, the model was incomplete, and the report declined
    // by name — the DRAFT named that as its first remainder. Phase 10 lane A
    // paid it: the cylinder boss finish states its band and the model is
    // complete. Phase 26 slice 2 paid the rest: the pocket's Clear goes around
    // the post, the Finish's lead-in lands in the annulus that Clear opened —
    // `check_moves` lets it through BECAUSE the holed removal says that air is
    // there — and the finished post is a whole post.
    let finished = src.replace(
        "run Clear(tool=T5, profile=PocketProfile, stepover=4.0)\n",
        "run Clear(tool=T5, profile=PocketProfile, stepover=4.0)\nrun Side(tool=T5, surface=Post, stepover=5.0)\n",
    );
    assert_ne!(finished, src);
    let mut p = parse::parse(&finished).unwrap();
    gcode::program(&p).expect("the finish enters through air the pocket's Clear declared");
    let report = vox::rest_report(&p, None).unwrap();
    assert!(report.contains("declared model: complete"), "{report}");
    assert!(!report.contains("the report declines"), "{report}");
    assert!(report.contains("violated (declared part, swept anyway): none"), "{report}");

    // The same program with the emitter blind to the post (`swept_post`'s
    // device): the violation fires and names the CLEAR that swept it, never
    // the finish that came to a post already gone.
    match p.features.get_mut("Post") {
        Some(parse::Feature::CylinderBoss { extent, z, .. }) => {
            *z += 0.002;
            *extent -= 0.002;
        }
        other => panic!("{other:?}"),
    }
    let report = vox::rest_report(&p, None).unwrap();
    assert!(!report.contains("violated (declared part, swept anyway): none"), "the finished post is still a swept post: {report}");
    assert!(report.contains("Swept by Clear on 'PocketProfile' (op [0])"), "the Clear did it, not the finish: {report}");
    let field = vox::VoxField::build(&p).unwrap();
    let found = vox::part_violations(&p, &field);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].message.contains("'Post'"), "{}", found[0].message);
}

/// `stock_to_leave` costs no special case: an allowance is material OUTSIDE
/// the declared surface, and a check that asks "air where the part is solid"
/// never looks there. And a surface finished to SIZE reads air in a handful
/// of cells microns inside itself (35 cells, 8µm, measured at the default
/// cell) — the sign's sub-cell slop on a tangent pass — which is exactly what
/// the band exists to keep out. Both at a coarse cell so the pin is cheap;
/// the band scales with the cell, so the silence is the same law.
#[test]
fn test_allowances_and_tangent_finishes_are_not_violations() {
    for fixture in ["sphere_rough.cam", "sphere.cam", "sphere_clear.cam"] {
        let p = load(fixture);
        let report = vox::rest_report(&p, Some(p.stock.w / 64.0)).unwrap();
        assert!(
            report.contains("violated (declared part, swept anyway): none"),
            "{fixture} honours its own dome: {report}"
        );
    }
}

/// The corpus: no shipped program violates its own declaration — or the
/// report declines by name, which is the other honest answer. At a coarse
/// cell (the stock's longest edge over 32) so the sweep is cheap; the band
/// scales with the cell and the rule is the same. The one fixture built to
/// violate is the acceptance above and is excluded here by name, so this pin
/// fails the day a second fixture starts losing declared material silently.
#[test]
fn test_no_shipped_program_violates_its_own_declaration() {
    let mut swept = 0;
    for entry in std::fs::read_dir(".").expect("read the fixture corpus") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("cam") {
            continue;
        }
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let Ok(src) = std::fs::read_to_string(path.join("main.part")) else { continue };
        let geom = std::fs::read_to_string(path.join("geometry.part")).ok();
        let Ok(p) = parse::parse_project_with_base(&src, geom.as_deref(), Some(&path)) else { continue };
        let longest = p.stock.w.max(p.stock.l).max(p.stock.h);
        let Ok(report) = vox::rest_report(&p, Some(longest / 32.0)) else { continue };
        if report.contains("the report declines") {
            continue;
        }
        assert!(
            report.contains("violated (declared part, swept anyway): none"),
            "{name} loses declared material to its own sweep: {report}"
        );
        swept += 1;
    }
    assert!(swept > 20, "the corpus sweep must actually sweep ({swept})");
}

/// Phase 11, lane B: the cells the window draws are the report's own scan.
/// `pocket_post.cam` after every op: the forgotten and violated counts equal
/// the numbers `--rest-report` prints, the violated cells all lie inside the
/// box the report names (`z -4.961..-1.055`), and at k = 0 — no op run —
/// nothing is forgotten and nothing is violated.
#[test]
fn test_the_rest_cells_are_the_report_s_own_scan() {
    let p = swept_post();
    let emitted_moves = gcode::program_with_moves(&p).unwrap().1;
    let n = p.operations.len();
    let cells = vox::rest_cells(&p, &emitted_moves, n, None).expect("a complete model, a contained part");
    let report = vox::rest_report(&p, None).unwrap();
    let count = |key: &str| -> usize {
        report
            .lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split(": ").nth(1))
            .and_then(|s| s.split(' ').next())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0)
    };
    assert_eq!(cells.violated.len(), count("violated (declared part, swept anyway)"), "{report}");
    assert_eq!(cells.forgotten.len(), count("forgotten (declared removed, never swept)"), "{report}");
    assert!(!cells.violated.is_empty(), "the post's 388 mm³");
    for c in &cells.violated {
        assert!(c[2] >= -4.961 - cells.cell && c[2] <= -1.055 + cells.cell, "inside the report's box: {c:?}");
        assert!((c[0] - 50.0).abs() <= 5.273 + cells.cell && (c[1] - 50.0).abs() <= 5.273 + cells.cell, "{c:?}");
    }
    let none = vox::rest_cells(&p, &emitted_moves, 0, None).expect("the untouched stock scans");
    assert!(none.forgotten.is_empty() && none.violated.is_empty(), "nothing run, nothing forgotten or violated");
}
