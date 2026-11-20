//! The check's account (ROADMAP item 2, slice 6): what each finding family
//! judged, found, and could not judge, and why one had nothing to judge —
//! on `--check`'s stdout, which was empty, and in `--check=json` as
//! `checked`. The empty Problems pane was never a certificate; this is what
//! stands behind it.
use nacs::check_result::{account, account_counts, account_words, check_result, family_word, FamilyAccount, FAMILY_ORDER};
use nacs::parse::{self, FindingFamily};
use std::fs;
use std::process::Command;

fn project(dir: &str) -> parse::Part {
    parse::load_project(dir).unwrap_or_else(|e| panic!("{dir}: {e:?}"))
}

fn accounted(dir: &str) -> Vec<FamilyAccount> {
    let p = project(dir);
    let v = check_result(&p);
    account(&p, &v)
}

fn row(acc: &[FamilyAccount], family: FindingFamily) -> &FamilyAccount {
    acc.iter().find(|a| a.family == family).unwrap_or_else(|| panic!("no row for {family:?}"))
}

#[test]
fn test_every_family_but_decline_is_accounted_for_in_the_windows_order() {
    let acc = accounted("plate.cam");
    let expected: Vec<FindingFamily> = FAMILY_ORDER.iter().copied().filter(|f| *f != FindingFamily::Decline).collect();
    assert_eq!(acc.iter().map(|a| a.family).collect::<Vec<_>>(), expected);
}

#[test]
fn test_a_plain_plate_says_what_ran_and_what_had_nothing_to_judge() {
    let acc = accounted("plate.cam");
    for family in [FindingFamily::Engagement, FindingFamily::Entry, FindingFamily::Containment, FindingFamily::BodyFoul, FindingFamily::FieldBodyFoul] {
        let r = row(&acc, family);
        assert!(r.ran() && r.found == 0 && r.declined == 0, "{family:?}: {r:?}");
    }
    assert_eq!(row(&acc, FindingFamily::Envelope).idle.as_deref(), Some("no machine is declared"));
    assert_eq!(row(&acc, FindingFamily::Fixture).idle.as_deref(), Some("no keep-out is declared"));
    assert_eq!(row(&acc, FindingFamily::Reach).idle.as_deref(), Some("no tool declares its stickout"));
    assert_eq!(row(&acc, FindingFamily::Deviation).idle.as_deref(), Some("no Side runs a chamfer tool or a corner rounder"));
    assert!(row(&acc, FindingFamily::PartViolation).idle.is_some());
    let (ran, idle, declined) = account_counts(&acc);
    assert_eq!((ran + idle, declined), (acc.len(), 0));
    assert!(ran >= 5, "{ran} ran");
}

#[test]
fn test_a_decline_is_counted_against_the_check_it_stood_in_for() {
    // The rim wire's interior is undeclared, so the entry restriction
    // declines on both Side ops.
    let acc = accounted("contour_pocket.cam");
    let entry = row(&acc, FindingFamily::Entry);
    assert!(entry.ran() && entry.declined == 2, "{entry:?}");
    assert_eq!(account_counts(&acc).2, 2);
    let words = account_words(&acc);
    assert!(words[0].ends_with(", 2 operations could not be judged"), "{}", words[0]);
    assert!(words.iter().any(|l| l == "entry: ran — nothing found; could not judge 2 operations"), "{words:?}");
}

#[test]
fn test_a_refused_program_leaves_the_families_that_read_moves_idle() {
    // A raster has nothing for direction= to govern, so the program is
    // refused at emission; the parse-time families still ran.
    let main = fs::read_to_string("plate_raster.cam/main.part").unwrap().replace("pattern=raster)", "pattern=raster, direction=climb)");
    let geom = fs::read_to_string("plate_raster.cam/geometry.part").ok();
    let p = parse::parse_project(&main, geom.as_deref()).unwrap();
    let v = check_result(&p);
    assert!(v.refusal.is_some());
    let acc = account(&p, &v);
    assert_eq!(row(&acc, FindingFamily::Entry).idle.as_deref(), Some("the program was refused before any move"));
    assert_eq!(row(&acc, FindingFamily::FieldBodyFoul).idle.as_deref(), Some("the program was refused before any move"));
    assert!(row(&acc, FindingFamily::Engagement).ran());
    assert!(row(&acc, FindingFamily::Containment).ran());
}

#[test]
fn test_the_words_name_each_family_once_after_the_summary() {
    let acc = accounted("plate.cam");
    let words = account_words(&acc);
    assert_eq!(words.len(), acc.len() + 1);
    let (ran, idle, _) = account_counts(&acc);
    assert_eq!(words[0], format!("checked: {ran} checks ran, {idle} had nothing to judge"));
    for (a, l) in acc.iter().zip(&words[1..]) {
        assert!(l.starts_with(&format!("{}: ", family_word(a.family))), "{l}");
        assert!(l.contains(if a.ran() { "ran — " } else { "nothing to judge — " }), "{l}");
    }
}

#[test]
fn test_check_prints_the_account_on_stdout_and_json_carries_it() {
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--check", "plate.cam"]).output().expect("run");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let acc = accounted("plate.cam");
    assert_eq!(stdout.lines().collect::<Vec<_>>(), account_words(&acc));
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--check=json", "plate.cam"]).output().expect("run");
    let doc = String::from_utf8_lossy(&out.stdout);
    assert!(doc.contains("\"checked\": [\n"), "{doc}");
    assert!(doc.contains("{\"family\": \"engagement\", \"ran\": true, \"found\": 0, \"declined\": 0, \"idle\": null}"), "{doc}");
    assert!(doc.contains("{\"family\": \"envelope\", \"ran\": false, \"found\": 0, \"declined\": 0, \"idle\": \"no machine is declared\"}"), "{doc}");
    // A refused program writes nothing to stdout in the prose mode.
    let dir = std::env::temp_dir().join("nacs_check_account_refused");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let main = fs::read_to_string("plate_raster.cam/main.part").unwrap().replace("pattern=raster)", "pattern=raster, direction=climb)");
    fs::write(dir.join("main.part"), main).unwrap();
    if let Ok(g) = fs::read_to_string("plate_raster.cam/geometry.part") {
        fs::write(dir.join("geometry.part"), g).unwrap();
    }
    let out = Command::new(env!("CARGO_BIN_EXE_nacs")).args(["--check", dir.to_str().unwrap()]).output().expect("run");
    assert!(!out.status.success() && out.stdout.is_empty(), "stdout: {}", String::from_utf8_lossy(&out.stdout));
    let _ = fs::remove_dir_all(&dir);
}
