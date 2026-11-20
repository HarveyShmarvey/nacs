//! The lead's shape (ROADMAP item 2, slice 5): `lead=`, `lead_angle=` and
//! `lead_descent=` say how far from the start the descent is made, which way
//! from the start it lies and how steeply the flight comes down; the start
//! turns round a closed path to where the whole lead lies in air, and where
//! none does, or no lead is made, the op refuses.
use nacs::{gcode, parse};
use std::fs;

fn project(dir: &str) -> parse::Part {
    parse::load_project(dir).unwrap_or_else(|e| panic!("{dir}: {e:?}"))
}

/// The fixture's program with `key` added to every `run <verb>(` line.
fn with_key(dir: &str, verb: &str, key: &str) -> Result<String, String> {
    let main = fs::read_to_string(format!("{dir}/main.part")).unwrap();
    let geom = fs::read_to_string(format!("{dir}/geometry.part")).ok();
    let needle = format!("run {verb}(");
    let mut out = String::new();
    let mut done = false;
    for line in main.lines() {
        if line.starts_with(&needle) {
            let k = line.rfind(')').unwrap();
            out.push_str(&format!("{}, {}{}\n", &line[..k], key, &line[k..]));
            done = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    assert!(done, "{dir} has no `{needle}` line");
    let p = parse::parse_project(&out, geom.as_deref()).map_err(|e| format!("{e:?}"))?;
    gcode::program(&p)
}

fn block<'a>(g: &'a str, title: &str) -> &'a str {
    g.split("\n(--- ").find(|b| b.starts_with(title)).unwrap_or_else(|| panic!("no block `{title}` in\n{g}"))
}

fn word(line: &str, letter: char) -> Option<f64> {
    line.split_whitespace().find(|w| w.starts_with(letter)).and_then(|w| w[1..].parse().ok())
}

fn xy(line: &str) -> (f64, f64) {
    (word(line, 'X').unwrap(), word(line, 'Y').unwrap())
}

/// A block's first entry: the lead point the tool descends at, the Z the
/// descent stops at, the line that flies onto the path, and the path's
/// next point.
struct Entry {
    lead: (f64, f64),
    stop_z: f64,
    onto: String,
    next: (f64, f64),
}

fn entry(b: &str) -> Entry {
    let lines: Vec<&str> = b.lines().collect();
    let down = lines.iter().position(|l| l.starts_with("G01 Z")).unwrap();
    let lead = xy(lines[..down].iter().rev().find(|l| l.starts_with("G00 X")).unwrap());
    let stop_z = word(lines[down], 'Z').unwrap();
    let onto = lines[down + 1].to_string();
    let next = xy(lines[down + 2..].iter().find(|l| l.starts_with("G0")).unwrap());
    Entry { lead, stop_z, onto, next }
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

const WALL: &str = "side contour (engine): Cavity";

#[test]
fn test_the_lead_takes_its_length_angle_and_descent() {
    let unsaid = entry(block(&gcode::program(&project("pocket_floor_offset.cam")).unwrap(), WALL));
    let start = xy(&unsaid.onto);
    assert!((dist(unsaid.lead, start) - 10.0).abs() < 1e-3, "the unsaid lead is a diameter long");
    // Shorter.
    let e = entry(block(&with_key("pocket_floor_offset.cam", "Side", "lead=4").unwrap(), WALL));
    assert_eq!(xy(&e.onto), start, "the start stands");
    assert!((dist(e.lead, start) - 4.0).abs() < 1e-3, "{:?}", e.lead);
    // Behind the start, on the path's own line: a closed path's incoming
    // line, so the lead point lies on the path itself.
    let e = entry(block(&with_key("pocket_floor_offset.cam", "Side", "lead_angle=0").unwrap(), WALL));
    assert_eq!(xy(&e.onto), start);
    assert!((e.lead.0 - start.0).abs() < 1e-6 && (e.lead.1 - start.1 - 10.0).abs() < 1e-3, "lead {:?} behind start {:?}", e.lead, start);
    // Oblique: 45 degrees between the path's line behind the start and the
    // air side's normal.
    let e = entry(block(&with_key("pocket_floor_offset.cam", "Side", "lead=4, lead_angle=45").unwrap(), WALL));
    let (ax, ay) = (e.lead.0 - start.0, e.lead.1 - start.1);
    let behind = (0.0, 1.0);
    let cos = (ax * behind.0 + ay * behind.1) / ax.hypot(ay);
    assert!((cos - 45f64.to_radians().cos()).abs() < 1e-3, "cos {cos}, lead {:?}", e.lead);
    assert!((ax.hypot(ay) - 4.0).abs() < 1e-3);
    // Descending: the plunge stops the rise short and the flight carries Z.
    let g = with_key("pocket_floor_offset.cam", "Side", "lead=4, lead_descent=10").unwrap();
    let e = entry(block(&g, WALL));
    let z_cut = word(&e.onto, 'Z').expect("the flight onto the path carries the cut's Z");
    assert!((z_cut - unsaid.stop_z).abs() < 1e-9, "the cut is at the same depth");
    assert!((e.stop_z - z_cut - 4.0 * 10f64.to_radians().tan()).abs() < 1e-3, "stop {} cut {z_cut}", e.stop_z);
    assert!(word(&unsaid.onto, 'Z').is_none(), "a flat flight carries no Z");
    assert!(g.contains("(lead: 4.000mm along the path's normal, descending 10.000deg, in air)"), "{g}");
    let g = with_key("pocket_floor_offset.cam", "Side", "lead=4, lead_angle=45, lead_descent=10").unwrap();
    assert!(g.contains("(lead: 4.000mm along a line 45.000deg off the path's own line, back from its start, toward the air, descending 10.000deg, in air)"), "{g}");
    let _ = e.next;
}

#[test]
fn test_a_ring_lead_takes_the_keys() {
    let title = "side bore wall (engine)";
    let unsaid = entry(block(&gcode::program(&project("cylinder_bore.cam")).unwrap(), title));
    let start = xy(&unsaid.onto);
    // Unsaid, from the axis; said, three millimetres toward it.
    assert!(dist(unsaid.lead, start) > 3.5, "the axis is further than that");
    let e = entry(block(&with_key("cylinder_bore.cam", "Side", "lead=3").unwrap(), title));
    assert!((dist(e.lead, start) - 3.0).abs() < 1e-3 && (e.lead.1 - start.1).abs() < 1e-6 && e.lead.0 < start.0, "toward the axis: {:?} from {:?}", e.lead, start);
    // Back along a bore's tangent the body stands in the wall; a boss has air there.
    let err = with_key("cylinder_bore.cam", "Side", "lead=3, lead_angle=0").unwrap_err();
    assert!(err.contains("the stock model reads standing material under the lead"), "{err}");
    let boss = "side boss wall (engine)";
    let start = xy(&entry(block(&gcode::program(&project("cylinder_boss.cam")).unwrap(), boss)).onto);
    let e = entry(block(&with_key("cylinder_boss.cam", "Side", "lead=5, lead_angle=0").unwrap(), boss));
    assert!((e.lead.0 - start.0).abs() < 1e-6 && (dist(e.lead, start) - 5.0).abs() < 1e-3, "{:?} from {:?}", e.lead, start);
    // A boss leads from outside; descending.
    let g = with_key("cylinder_boss.cam", "Side", "lead=5, lead_descent=15").unwrap();
    let e = entry(block(&g, boss));
    let z_cut = word(&e.onto, 'Z').expect("the flight carries Z");
    assert!((e.stop_z - z_cut - 5.0 * 15f64.to_radians().tan()).abs() < 1e-3);
    assert!(g.contains("(lead: 5.000mm along the ring's radius, outward, descending 15.000deg, in air)"), "{g}");
}

#[test]
fn test_the_offset_family_lead_takes_the_keys() {
    // The step's innermost loop stands 20mm from the air past the blank.
    let unsaid = gcode::program(&project("step.cam")).unwrap();
    assert!(unsaid.contains(", 20.000mm to the loop)"), "{unsaid}");
    let g = with_key("step.cam", "Clear", "lead=20, lead_descent=20").unwrap();
    assert!(g.contains("20.000mm to the loop, descending 20.000deg)"), "{g}");
    let e = entry(block(&g, "pocket clear"));
    let z_cut = word(&e.onto, 'Z').expect("the flight carries Z");
    assert!((e.stop_z - z_cut - 20.0 * 20f64.to_radians().tan()).abs() < 1e-3, "stop {} cut {z_cut}", e.stop_z);
    assert!((dist(e.lead, xy(&e.onto)) - 20.0).abs() < 1e-3);
    // Six millimetres reach no air from that loop.
    let err = with_key("step.cam", "Clear", "lead=6").unwrap_err();
    assert!(err.contains("no start on loop 1 of 6 has the whole lead in air"), "{err}");
}

#[test]
fn test_lead_keys_are_refused_where_no_lead_can_be_whole_or_none_is_made() {
    // A lead longer than the pocket finds no start whose flight is in air.
    let e = with_key("pocket_floor_offset.cam", "Side", "lead=200").unwrap_err();
    assert!(e.contains("no start on the path has the whole lead in air"), "{e}");
    // An open wall is entered along its own line.
    let e = with_key("plate.cam", "Side", "lead_angle=45").unwrap_err();
    assert!(e.contains("has no air side to turn the lead toward"), "{e}");
    // Rows and the raster make no lead.
    let e = with_key("plate.cam", "Clear", "lead=5").unwrap_err();
    assert!(e.contains("this op makes none"), "{e}");
    let e = with_key("plate_raster.cam", "End", "lead=5").unwrap_err();
    assert!(e.contains("this op makes none"), "{e}");
    // A plunge has no lead to shape; a Copy makes none.
    let e = with_key("plate.cam", "Side", "entry=plunge, lead=5").unwrap_err();
    assert!(e.contains("a plunge has no lead to shape"), "{e}");
    let e = with_key("sphere.cam", "Copy", "lead=5").unwrap_err();
    assert!(e.contains("a Copy makes none"), "{e}");
    for bad in ["lead=0", "lead_angle=91", "lead_descent=90", "lead_angle=-1"] {
        assert!(with_key("plate.cam", "Side", bad).is_err(), "{bad}");
    }
}

#[test]
fn test_pocket_wall_lead_golden() {
    let g = gcode::program(&project("pocket_wall_lead.cam")).unwrap();
    let golden = fs::read_to_string("tests/pocket_wall_lead.golden.gcode").expect("golden");
    assert_eq!(g.trim(), golden.trim(), "pocket_wall_lead golden drifted");
}
