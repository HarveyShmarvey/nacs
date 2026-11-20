//! The shell tests (Phase 51 slice 1; `DECISIONS.md` → "Phase 51 slice 1…").
//!
//! The one file that drives `src/frontend.rs` — the real `ViewApp`, frame by
//! frame, with no window and no GPU, through egui's own harness
//! (`egui_kittest`) and the AccessKit tree the window already builds. A thin
//! test for a thin shell: the view models stay where a fact LIVES (and
//! `tests/view_tests.rs` pins them); these tests assert that the shell SHOWS
//! it. Behind the `shell-tests` feature — `check.sh` runs
//! `cargo test --features shell-tests --test shell_tests`.
//!
//! The first tests are the five stuck doors of the Phase 51 DRAFT, pinned AS
//! THEY STAND TODAY (the slice 0 ruling: a red test cannot merge under the
//! lock), each naming the slice that will flip it. The harness proves it can
//! see the feeling; the slices turn the assertions round.
#![cfg(feature = "shell-tests")]

use eframe::egui;
use egui::accesskit::{Role, Toggled};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use nacs::frontend::{self, ViewApp};
use nacs::theme::{DockAction, Layout, SidebarTab, Workspace};
use nacs::view::{self, Selection};
use std::sync::Once;

/// The window saves its workspace at open, at close and in two acts
/// (`layout.revert`, a project switch). A test must never write the user's
/// `%APPDATA%\nacs\workspace`, so the config home is pointed at a scratch
/// directory ONCE, before any harness is built — every reader in
/// `theme::workspace_path` goes through the same variable.
fn redirect_config_home() {
    static ONCE: Once = Once::new();
    let dir = std::env::temp_dir().join(format!("nacs-shell-tests-{}", std::process::id()));
    ONCE.call_once(|| {
        std::fs::create_dir_all(&dir).expect("scratch config dir");
        // Windows reads APPDATA; the other platforms XDG_CONFIG_HOME.
        std::env::set_var("APPDATA", &dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
    });
    // The workspace FILE is this test's own (Phase 59 slice 4, the lock's
    // finding): tests run on parallel threads and every save went to the
    // one file under the config home, so a pick's line could be read back
    // after another test's save had rewritten it. The themes folder stays
    // shared — it is only read.
    let test = std::thread::current().name().unwrap_or("t").replace("::", "-");
    let mine = dir.join("workspaces").join(test);
    std::fs::create_dir_all(&mine).expect("this test's workspace dir");
    nacs::theme::workspace_for_this_thread(mine.join("workspace"));
}

/// `plate.cam` opened the way `nacs --view plate.cam` opens it, on the
/// arrangement the test asks for. 1200×800 is the window's own opening size.
fn open_with(workspace: Workspace) -> Harness<'static, ViewApp> {
    redirect_config_home();
    let part = nacs::parse::load_project("plate.cam").expect("plate.cam parses");
    let cr = nacs::check_result::check_result(&part);
    Harness::builder()
        .with_size(egui::Vec2::new(1200.0, 800.0))
        .with_max_steps(64)
        .build_eframe(move |_cc| frontend::harness_app(&part, cr, "plate.cam", None, workspace))
}

fn open() -> Harness<'static, ViewApp> {
    open_with(Workspace::default())
}

/// A COPY of a fixture in a scratch directory, opened as a project — for a
/// test that writes (Phase 53 slice 2): the fixture of record is never
/// touched, whatever the test does or fails to undo.
fn open_copy(fixture: &str) -> (Harness<'static, ViewApp>, std::path::PathBuf) {
    open_copy_with(fixture, false)
}

/// The same copy, rendered too (slice 0's seat) so the test can `shot` it.
fn open_copy_pixels(fixture: &str) -> (Harness<'static, ViewApp>, std::path::PathBuf) {
    open_copy_with(fixture, true)
}

fn open_copy_with(fixture: &str, pixels: bool) -> (Harness<'static, ViewApp>, std::path::PathBuf) {
    redirect_config_home();
    // The TEST's name in the path too: two tests copy plate.cam at once
    // under `cargo test`'s threads, and one removing the other's copy
    // mid-run was the flake the full suite found (Phase 54 slice 3).
    let test = std::thread::current().name().unwrap_or("t").replace("::", "-");
    let dir = std::env::temp_dir().join(format!("nacs-shell-copy-{}-{}-{}", std::process::id(), fixture.replace(".cam", ""), test));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join(fixture);
    std::fs::create_dir_all(&project).unwrap();
    for f in ["main.part", "geometry.part"] {
        let src = std::path::Path::new(fixture).join(f);
        if src.exists() {
            std::fs::copy(&src, project.join(f)).unwrap();
        }
    }
    let path = project.to_string_lossy().into_owned();
    let part = nacs::parse::load_project(&path).expect("the copy parses");
    let cr = nacs::check_result::check_result(&part);
    let p2 = path.clone();
    let b = Harness::builder().with_size(egui::Vec2::new(1200.0, 800.0)).with_max_steps(64);
    let b = if pixels { b.wgpu() } else { b };
    let h = b.build_eframe(move |_cc| frontend::harness_app(&part, cr, &p2, None, Workspace::default()));
    (h, project)
}

/// The same window at another size (Phase 52 slice 12): a narrow one, so
/// a pane can be narrower than what it holds.
fn open_sized(workspace: Workspace, size: egui::Vec2) -> Harness<'static, ViewApp> {
    redirect_config_home();
    let part = nacs::parse::load_project("plate.cam").expect("plate.cam parses");
    let cr = nacs::check_result::check_result(&part);
    Harness::builder().with_size(size).with_max_steps(64).build_eframe(move |_cc| frontend::harness_app(&part, cr, "plate.cam", None, workspace))
}

/// The agent's seat (ROADMAP item 3 slice 0): the same harness, on egui's
/// wgpu test renderer, which takes a CPU adapter where there is one (WARP on
/// Windows) so it never contends with the user's screen. Pixels cost a
/// frame each, so only a test that LOOKS opens this way.
fn open_pixels(workspace: Workspace) -> Harness<'static, ViewApp> {
    redirect_config_home();
    let part = nacs::parse::load_project("plate.cam").expect("plate.cam parses");
    let cr = nacs::check_result::check_result(&part);
    Harness::builder()
        .with_size(egui::Vec2::new(1200.0, 800.0))
        .with_max_steps(64)
        .wgpu()
        .build_eframe(move |_cc| frontend::harness_app(&part, cr, "plate.cam", None, workspace))
}

/// The last frame, written to `target/shots/<name>.png` for the agent to
/// read — under the manifest's own `target`, whatever `CARGO_TARGET_DIR`
/// says, so the pictures are always in one place and never in the tree.
fn shot(h: &mut Harness<'static, ViewApp>, name: &str) -> image::RgbaImage {
    let img = h.render().expect("the frame renders");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("shots");
    std::fs::create_dir_all(&dir).expect("target/shots");
    img.save(dir.join(format!("{name}.png"))).expect("the PNG is written");
    img
}

/// How many pixels differ between two frames of one size.
fn pixels_changed(a: &image::RgbaImage, b: &image::RgbaImage) -> usize {
    a.pixels().zip(b.pixels()).filter(|(p, q)| p != q).count()
}

/// A workspace whose form is shut (the user's own file on 2026-10-17 read
/// `form.open false`) and whose sidebar shows `tab`.
fn workspace_form_shut(tab: SidebarTab) -> Workspace {
    let mut layout = Layout::default();
    layout.apply(DockAction::ShowTab(tab));
    if layout.form.open {
        layout.apply(DockAction::ToggleForm);
    }
    Workspace { layout, ..Workspace::default() }
}

/// Settle: run frames until the window asks for no immediate repaint. The
/// window always asks for one 500 ms out (the file watch), which the
/// harness reads as settled.
fn settle(h: &mut Harness<'static, ViewApp>) {
    // A worker (the pattern table, the sim) asks a repaint every 100 ms
    // while it runs, which the harness reads as immediate; a settle that
    // meets one waits for it (Phase 53 slice 1: plate's Clear made the
    // table slower than 64 steps) instead of calling the window unsettled.
    for _ in 0..200 {
        if h.try_run().is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("the window did not settle in ten seconds");
}

/// The FIRST node whose label STARTS with `prefix` — an Outline row's text
/// is "Notch  ProfilePocket", and the name alone also appears in the
/// card and the operation lines, so "contains" is ambiguous where "starts
/// with" is not. Since Phase 52 slice 4 an operation's row stands twice in
/// the Outline — under its feature and under Operations, the same node —
/// and the first is the tree's; `rows_starting` counts them.
fn row<'t>(h: &'t Harness<'static, ViewApp>, prefix: &'t str) -> egui_kittest::Node<'t> {
    let mut hits: Vec<egui_kittest::Node<'t>> = h
        .query_all_by_label_contains(prefix)
        .filter(|n| n.accesskit_node().label().is_some_and(|l| l.starts_with(prefix)))
        .collect();
    assert!(!hits.is_empty(), "a row starts with {prefix:?}");
    hits.remove(0)
}

/// How many nodes' labels start with `prefix`.
fn rows_starting(h: &Harness<'static, ViewApp>, prefix: &str) -> usize {
    h.query_all_by_label_contains(prefix).filter(|n| n.accesskit_node().label().is_some_and(|l| l.starts_with(prefix))).count()
}

/// A submenu's button: egui labels it with the title and a trailing arrow
/// ("File ⏵"), so it is found by its start and never confused with a row
/// of the same word ("Help" under Help).
fn submenu<'t>(h: &'t Harness<'static, ViewApp>, title: &'t str) -> Option<egui_kittest::Node<'t>> {
    h.query_all_by_label_contains(title)
        .find(|n| n.accesskit_node().label().is_some_and(|l| l.starts_with(title) && l.len() > title.len() && l.trim_end().ends_with('⏵')))
}

/// How many nodes carry exactly this label.
fn count_label(h: &Harness<'static, ViewApp>, label: &str) -> usize {
    h.query_all_by_label(label).count()
}

/// A click OUTSIDE whatever is open over the picture: low on the Program
/// document's side of the center, where no overlay reaches — the palette
/// is anchored centre-top and its list runs to 60% of the height, so the
/// middle of the window is NOT outside it (the harness caught exactly that
/// in this test's first draft: the click ran a row).
fn click_outside(h: &mut Harness<'static, ViewApp>) {
    let p = egui::Pos2::new(1150.0, 650.0);
    h.hover_at(p);
    h.drag_at(p);
    h.drop_at(p);
    settle(h);
}

// ---- the harness sees the shell -------------------------------------------

#[test]
fn the_window_opens_on_plate_with_the_tabs_the_outline_and_the_doors() {
    let mut h = open();
    settle(&mut h);
    // The four tabs, by their persistent labels (DESIGN §5.2; the rail's
    // slots until Phase 52 slice 1).
    for t in SidebarTab::ALL {
        assert!(h.query_all_by_label_contains(t.title()).next().is_some(), "tab {}", t.title());
    }
    // The Outline's rows: the two features (name and kind), the two
    // operations — each once under its feature and once in run order
    // (Phase 52 slice 4).
    row(&h, "TopFace  Plane");
    row(&h, "Notch  ProfilePocket");
    assert_eq!(rows_starting(&h, "[0] End"), 2);
    assert_eq!(rows_starting(&h, "[1] Clear"), 2);
    assert_eq!(rows_starting(&h, "[2] Side"), 2);
    // The two status-line doors (Phase 50 slice 4).
    assert!(h.query_by_label("Commands").is_some());
    assert!(h.query_by_label("?").is_some());
    // The undo door, disabled with nothing to undo — on the status line
    // since Phase 52 slice 13.
    let undo = h.get_by_label("Undo");
    assert!(undo.accesskit_node().is_disabled());
}

/// Phase 52 slice 13 (§5.6, §5.7): the transport bar is the cursor's in
/// two rows and nothing else — Undo and Redo stand on the status line's
/// row beside `?` and *Commands* (fork 13a-A); the move row is present
/// with its own words before any move is named and after one is (fork
/// 13c-A); the Outline's run order draws the cursor as a rule with the
/// count beside it, between the rows it divides (fork 13b).
#[test]
fn the_transport_bar_is_the_cursors_in_two_rows_and_the_way_back_is_on_the_status_line() {
    let mut h = open();
    settle(&mut h);
    let help_y = h.get_by_label("?").rect().center().y;
    for label in ["Undo", "Redo", "Commands"] {
        let r = h.get_by_label(label).rect();
        assert!((r.center().y - help_y).abs() < 1.5, "{label} stands on the status line's row: {} vs {help_y}", r.center().y);
    }
    let undo_x = h.get_by_label("Undo").rect().left();
    assert!(undo_x > 600.0, "at the window's right end: {undo_x}");
    // The two rows: the op slider and the move slider, the move row's words
    // with nothing named yet.
    assert_eq!(h.query_all_by_role(Role::Slider).count(), 2, "the op slider and the move slider");
    assert!(h.query_all_by_label_contains("[2]: 13 moves").next().is_some(), "the move row names the operation and counts its moves");
    assert!(h.query_all_by_label_contains("none named").next().is_none(), "how to name a move is the slider's hover, not a standing instruction");
    // The cursor is said once: the transport's line has it, the sidebar's
    // line and the Program's row do not say it again.
    assert_eq!(h.query_all_by_label_contains("of 3 run").count(), 1, "one home for the cursor");
    assert!(h.query_all_by_label_contains("after operation").next().is_none() && h.query_all_by_label_contains("highlighted").next().is_none());
    // Name a move: the row says which.
    h.query_all_by_label_contains("G01").next().expect("a feed move of op 0 is on screen").click();
    settle(&mut h);
    let b = h.state().block_cursor().expect("the click named a move");
    assert!(h.query_all_by_label_contains(&format!("move {} of", b + 1)).next().is_some(), "the move row names move {}", b + 1);
    // The cursor's rule in the Outline: the click left the cursor after
    // operation 1, so the count stands between the run order's two rows.
    // The counts read in the rule's direction (slice 16): "1 ⏶" at the
    // left for what has run, "1 ⏷" at the right for what is left.
    let rule = h.query_all_by_label_contains(" ⏶").next().expect("the run count beside the rule").rect();
    let to_run = h.query_all_by_label_contains(" ⏷").next().expect("the to-run count at the rule's right").rect();
    assert!((to_run.center().y - rule.center().y).abs() < 1.5 && to_run.left() > rule.right(), "the two counts share the rule's row, run left and to-run right: {rule:?} {to_run:?}");
    assert!(h.query_all_by_label_contains("1 ⏶").next().is_some() && h.query_all_by_label_contains("2 ⏷").next().is_some(), "after operation 1 of 3: one up, two down");
    // Each operation stands twice in the Outline (under its feature and in
    // the run order): the run order's are the rows nearest the rule.
    let first = h.query_all_by_label_contains("[0] End").map(|n| n.rect()).filter(|r| r.bottom() <= rule.top() + 1.0).max_by(|a, b| a.top().total_cmp(&b.top())).expect("a [0] End row above the rule");
    let second = h.query_all_by_label_contains("[1] Clear").map(|n| n.rect()).filter(|r| r.top() >= rule.bottom() - 1.0).min_by(|a, b| a.top().total_cmp(&b.top())).expect("a [1] Clear row below the rule");
    assert!(rule.top() - first.bottom() < 12.0 && second.top() - rule.bottom() < 12.0, "the rule sits between the two rows: {first:?} {rule:?} {second:?}");
}

// ---- Phase 52: the band's first row ------------------------------------------

/// Fork 1 (Phase 52 slice 1, §5.1 retired, §5.2): the views are TABS on
/// the sidebar's first row, each word once — the rail's column and the
/// heading row that repeated the word are gone, and so is the hint line.
/// A tab click shows its view (I3, renamed) and never hides one.
#[test]
fn the_views_are_tabs_each_word_once_and_the_hint_line_is_gone() {
    let mut h = open();
    settle(&mut h);
    // One word, once: the tab. (Two before — the rail's slot and the
    // sidebar's heading.) The Outline's own rows do not say "Outline".
    assert_eq!(count_label(&h, "Outline"), 1, "the word Outline is on screen once, as the tab");
    assert_eq!(count_label(&h, "Review"), 1);
    assert!(h.query_by_label_contains("press ? for help").is_none(), "the hint line is gone (fork 2)");
    assert!(h.query_by_label(view::MENU_GLYPH).is_some(), "the menu keeps the upper-left corner");
    // A tab click shows its view; clicking it again hides nothing.
    h.get_by_label("Problems").click();
    settle(&mut h);
    assert_eq!(h.state().layout().sidebar.tab, SidebarTab::Problems);
    assert!(h.state().layout().sidebar.open);
    h.get_by_label("Problems").click();
    settle(&mut h);
    assert!(h.state().layout().sidebar.open, "a tab click never hides (I3)");
    assert_eq!(h.state().layout().sidebar.tab, SidebarTab::Problems);
    h.get_by_label("Outline").click();
    settle(&mut h);
    assert_eq!(h.state().layout().sidebar.tab, SidebarTab::Outline);
    row(&h, "Notch  ProfilePocket");
}

/// I14 at the retired rail: with the sidebar shut — by its `×` — the
/// band's left cell stays: the menu, and a *Sidebar* chip that reopens
/// it with one click (the rail's slots were that door). With the form
/// shut too, the same cell stands alone.
#[test]
fn the_shut_sidebar_keeps_the_menu_and_a_door_back() {
    let mut h = open();
    settle(&mut h);
    assert!(h.query_by_label("Sidebar").is_none(), "no chip while the sidebar is open (the menu's Window ▸ Sidebar row is inside a shut menu)");
    // The first × on screen is the sidebar's (the band's first row is
    // drawn before the form's header).
    h.query_all_by_label("×").next().expect("the sidebar's ×").click();
    settle(&mut h);
    assert!(!h.state().layout().sidebar.open, "× shut the sidebar");
    assert!(h.query_by_label(view::MENU_GLYPH).is_some(), "the menu stays");
    assert!(h.query_by_label("Outline").is_none(), "the tabs went with the sidebar");
    let chip = h.get_by_label("Sidebar");
    chip.click();
    settle(&mut h);
    assert!(h.state().layout().sidebar.open, "the chip reopens the sidebar (I14)");
    assert_eq!(count_label(&h, "Outline"), 1, "the tabs are back");
    // Both docks shut: the cell alone.
    let mut h = open_with(workspace_form_shut(SidebarTab::Outline));
    settle(&mut h);
    h.key_press(egui::Key::Num1);
    settle(&mut h);
    assert!(!h.state().layout().sidebar.open && !h.state().layout().form.open);
    assert!(h.query_by_label(view::MENU_GLYPH).is_some() && h.query_by_label("Sidebar").is_some(), "☰ and the chip stand with both docks shut");
    h.get_by_label("Sidebar").click();
    settle(&mut h);
    assert!(h.state().layout().sidebar.open);
}

/// Fork 0 and fork 3 (Phase 52 slice 2, §4, §5.3, §5.5): the band — two
/// rows at one height in every column. Row 1: the tabs over the sidebar,
/// the three acts over the center. Row 2: the view's own line, the Layers
/// button with the legend and the view strip over the Viewport, the lines
/// label with the gutters over the Program. The widgets of a row share a
/// centre line across the three columns, and the rows sit inside the
/// band's height; the viewport's corners hold nothing but the map.
#[test]
fn the_band_is_two_rows_at_one_height_in_every_column() {
    let mut h = open();
    h.remove_cursor();
    settle(&mut h);
    let band = view::BAND_ROWS as f32 * view::BAND_ROW_H + 16.0; // the panels' own margins
    let centre_y = |n: egui_kittest::Node<'_>| n.rect().center().y;
    // Row 1: a tab, and the center's first act.
    let tab = centre_y(h.get_by_label("Outline"));
    let strip = centre_y(h.get_by_label("Viewport"));
    assert!((tab - strip).abs() < 1.5, "row 1 shares a centre line: tab {tab}, strip {strip}");
    assert!(tab < band, "row 1 is in the band: {tab}");
    // Row 2: the view's line, the Layers button, the Program's toggle.
    let line = centre_y(h.get_by_label_contains("plate.cam — "));
    let layers = centre_y(h.get_by_label("Layers"));
    let numbers = centre_y(h.get_by_label("numbers"));
    assert!((line - layers).abs() < 1.5 && (layers - numbers).abs() < 1.5, "row 2 shares a centre line: {line} {layers} {numbers}");
    assert!(layers > tab && layers < band, "row 2 is the second row of the band: {layers}");
    // The Program's row carries its label and both ways out, once each
    // (the menu is shut).
    assert!(h.query_by_label_contains(" lines").is_some());
    assert_eq!(count_label(&h, "Write program"), 1);
    assert_eq!(count_label(&h, "Copy program"), 1);
    // The view strip is on the viewport's row, level with Layers.
    let fit = centre_y(h.get_by_label("Fit"));
    assert!((fit - layers).abs() < 1.5, "Fit {fit} beside Layers {layers}");
    // The view's line follows the tab.
    h.get_by_label("Problems").click();
    settle(&mut h);
    assert!(h.query_by_label_contains("no problems · ").is_some(), "the Problems view's own line, with the account");
    assert!(h.query_by_label_contains("plate.cam — ").is_none());
}

/// Fork 4 (Phase 52 slice 4, §5.2): the Outline is five groups, top to
/// bottom, each header carrying its door; the Sequence folded into
/// Operations, so an operation stands under its feature and in run order,
/// with the cursor's line; *New operation…* on the header waits for a
/// feature; *New target…* opens a path field, and with no report the
/// Target group says where one comes from.
#[test]
fn the_outline_is_five_groups_with_doors_on_their_headers() {
    let mut h = open();
    settle(&mut h);
    for group in ["Stock", "Target", "Features", "Operations", "Tools"] {
        assert_eq!(count_label(&h, group), 1, "the {group} header, once");
    }
    assert!(h.query_by_label("Sequence").is_none(), "no Sequence tab");
    assert!(h.query_by_label("New feature…").is_some());
    assert!(h.query_by_label("New tool…").is_some());
    assert!(h.query_by_label_contains("No target").is_some(), "no report: the Target group says where one comes from");
    assert!(h.query_by_label_contains(" ⏶").is_some() && h.query_by_label_contains(" ⏷").is_some(), "the run order carries the cursor's rule, the counts beside it (Phase 52 slices 13 and 16)");
    // New operation… on the Operations header waits for a feature.
    assert!(h.get_by_label("New operation…").accesskit_node().is_disabled(), "disabled with nothing selected");
    row(&h, "Notch  ProfilePocket").click();
    settle(&mut h);
    // The selection opened the form on the feature's card, which carries
    // its own New operation… (Phase 50): the header's is the enabled one
    // among them now.
    assert!(h.query_all_by_label("New operation…").any(|n| !n.accesskit_node().is_disabled()), "enabled on a selected feature");
    // A run-order row selects the operation, as the tree's does.
    let ops: Vec<egui_kittest::Node<'_>> = h.query_all_by_label_contains("[1] Clear").collect();
    assert_eq!(ops.len(), 2);
    ops.last().unwrap().click();
    settle(&mut h);
    assert!(matches!(h.state().selection(), Selection::Operation(1)));
    // New target… opens the path field; × shuts it. (The selected
    // operation's card has fields of its own, so the count is what moves.)
    let fields = h.query_all_by_role(Role::TextInput).count();
    h.get_by_label("New target…").click();
    settle(&mut h);
    assert_eq!(h.query_all_by_role(Role::TextInput).count(), fields + 4, "the path field, and the solid, allowance and given fields under it");
    assert!(h.query_by_label("Import").is_some());
    // The native dialog's door, where the platform has one (slice 9) — the
    // dialog itself is the system's and is not opened here.
    assert_eq!(h.query_by_label("Browse…").is_some(), cfg!(windows), "Browse… on Windows alone");
    h.query_all_by_label("×").last().expect("the field's ×").click();
    settle(&mut h);
    assert_eq!(h.query_all_by_role(Role::TextInput).count(), fields);
    assert!(h.query_by_label("Import").is_none(), "the field closed");
}

/// Fork 5 (Phase 52 slice 3, §5.3 — the user's "big thing"): the Program's
/// document fills its pane. The scroll area used to shrink-wrap the
/// longest line, so the vertical scrollbar hugged the text while the
/// minimap strip stood at the pane's edge, and the air between read as a
/// divider the minimap does not have. The area now reaches the strip.
#[test]
fn the_program_document_fills_its_pane_to_the_minimap() {
    let mut h = open();
    settle(&mut h);
    let (pane, area) = h.state().program_rects().expect("the Program is on screen in Both");
    assert!(area.left() <= pane.left() + 2.0, "the area starts at the pane's left: {} vs {}", area.left(), pane.left());
    // The solid vertical bar (slice 15) takes its room from the area's
    // right, before the strip.
    let bar = { let s = egui::style::ScrollStyle::solid(); s.bar_width + s.bar_inner_margin + s.bar_outer_margin };
    assert!(
        area.right() >= pane.right() - frontend::MINIMAP_STRIP_W - bar - 2.0,
        "the area reaches the minimap strip: area right {}, pane right {} less the strip {}",
        area.right(),
        pane.right(),
        frontend::MINIMAP_STRIP_W
    );
    // With the minimap off, it reaches the pane's edge.
    h.get_by_label("minimap").click();
    settle(&mut h);
    let (pane, area) = h.state().program_rects().unwrap();
    assert!(area.right() >= pane.right() - bar - 2.0, "no strip: area right {}, pane right {}", area.right(), pane.right());
}

/// Phase 52 slice 12 (§5.3): nothing in the Program pane crosses its LEFT
/// edge when the pane is narrower than its row or its lines — the user's
/// finding after slice 3 ("the content spills over the left edge of the
/// window if it ends up narrower than the lines of the code"). The window
/// is narrowed until the pane is narrower than its row and its lines; the
/// pane keeps its place beside the viewport (egui widens a ui to hold what
/// overflows it, and a row laid right to left overflowed LEFT — the
/// measured cause); the row's four controls and every program line on
/// screen start at or inside the pane's left; a line longer than the pane
/// is for the horizontal scroll, not the left margin.
#[test]
fn nothing_in_a_narrow_program_pane_crosses_its_left_edge() {
    let mut layout = Layout::default();
    layout.apply(DockAction::ShowSplit);
    let mut h = open_sized(Workspace { layout, ..Workspace::default() }, egui::Vec2::new(640.0, 800.0));
    settle(&mut h);
    settle(&mut h);
    let (pane, area) = h.state().program_rects().expect("the Program is on screen");
    assert!(area.left() >= pane.left() - 1.0, "the area starts in the pane: {} vs {}", area.left(), pane.left());
    // The pane stands beside the viewport, not over it or the sidebar.
    for label in ["Layers", "Outline"] {
        let r = h.get_by_label(label).rect();
        assert!(r.right() <= pane.left() + 1.0, "{label} ({r:?}) is left of the pane ({pane:?})");
    }
    // The row's controls, by name: each starts at or inside the pane's left.
    for label in ["Write program", "Copy program", "minimap", "numbers"] {
        let r = h.get_by_label(label).rect();
        assert!(r.left() >= pane.left() - 1.0, "{label} crosses the pane's left edge: {} vs {}", r.left(), pane.left());
    }
    // Every line on screen: the labels standing in the pane's own rows
    // (a program line's node carries no label text, only its rect).
    let lines: Vec<egui::Rect> = h
        .query_all_by_role(Role::Label)
        .map(|n| n.rect())
        .filter(|r| r.top() >= pane.top() && r.bottom() <= pane.bottom() && r.right() > pane.left())
        .collect();
    let feed = h.query_all_by_label_contains("G01").next().expect("a feed line is on screen").rect();
    assert!(feed.top() >= pane.top() && feed.left() >= area.left() - 1.0, "the feed line stands in the pane: {feed:?} vs {area:?}");
    assert!(lines.iter().any(|r| r.width() > pane.width()), "a line is wider than the pane ({}): {lines:?}", pane.width());
    for r in &lines {
        assert!(r.left() >= area.left() - 1.0, "a line crosses the area's left edge: {} vs {}", r.left(), area.left());
    }
    // A line wider than the pane has a SOLID horizontal bar under the
    // document (slice 15): it takes its room from the area's bottom, so a
    // mouse without a side wheel has a bar to drag.
    let bar = { let s = egui::style::ScrollStyle::solid(); s.bar_width + s.bar_inner_margin + s.bar_outer_margin };
    assert!(area.bottom() <= pane.bottom() - bar + 1.0, "the horizontal bar took its room: area bottom {}, pane bottom {}, bar {bar}", area.bottom(), pane.bottom());
}

/// Phase 53 slice 3 (§5.5, fork 2): nothing in a narrow VIEWPORT row
/// crosses its left edge — the same mechanism slice 12 pinned on the
/// Program row, a right-to-left strip walking left past the pane and egui
/// widening the ui to hold it. The split at its minimum in a narrow window
/// makes the viewport narrower than Layers, the legend and the five acts
/// together; every one of them starts at or right of the Layers button,
/// the row's first widget, and the Program pane stands to the right of it.
#[test]
fn nothing_in_a_narrow_viewport_row_crosses_its_left_edge() {
    let mut layout = Layout::default();
    layout.apply(DockAction::ShowSplit);
    layout.apply(DockAction::ResizeSplit(nacs::theme::SPLIT_MIN));
    let mut h = open_sized(Workspace { layout, ..Workspace::default() }, egui::Vec2::new(760.0, 800.0));
    settle(&mut h);
    settle(&mut h);
    let layers = h.get_by_label("Layers").rect();
    let sidebar_right = h.get_by_label("Outline").rect().right();
    assert!(layers.left() > sidebar_right, "the row starts right of the sidebar: {} vs {sidebar_right}", layers.left());
    let (pane, _) = h.state().program_rects().expect("the Program is on screen");
    assert!(pane.left() > layers.left(), "the Program pane stands right of the viewport's row start: {} vs {}", pane.left(), layers.left());
    let mut widest = 0.0_f32;
    for (_, label, _) in view::VIEW_STRIP {
        let r = h.get_by_label(*label).rect();
        assert!(r.left() >= layers.left() - 1.0, "{label} crosses the row's left edge: {} vs {}", r.left(), layers.left());
        assert!((r.center().y - layers.center().y).abs() < 1.5, "{label} shares the row: {} vs {}", r.center().y, layers.center().y);
        widest = widest.max(r.right());
    }
    assert!(widest > pane.left(), "the strip really overflows the narrow viewport (clipped at its right): {widest} vs {}", pane.left());
}

/// Fork 8 (Phase 52 slice 7, §5.3): the band's first row over the center
/// ends in the project's name — and, once the worker has read the
/// repository, its branch — level with the three acts.
#[test]
fn the_band_names_the_project_and_its_branch_at_the_right() {
    let mut h = open();
    settle(&mut h);
    // Two nodes say "plate.cam": the sidebar's view line at the left and
    // the chip at the center's right end — the one past the sidebar.
    let chip = h.query_all_by_label_contains("plate.cam").find(|n| n.rect().left() > 600.0).expect("the chip, the name alone until git answers");
    let strip_y = h.get_by_label("Viewport").rect().center().y;
    assert!((chip.rect().center().y - strip_y).abs() < 1.5, "the chip is on the first row: {} vs {strip_y}", chip.rect().center().y);
    // Give the worker its moment; with git in reach the branch appears.
    std::thread::sleep(std::time::Duration::from_millis(600));
    settle(&mut h);
    settle(&mut h);
    if let Some(Ok(head)) = h.state().git_head() {
        let branch = head.branch.clone();
        assert!(h.query_all_by_label_contains(&format!("plate.cam · {branch}")).next().is_some(), "the chip carries the branch {branch}");
    }
}

/// Fork 6 (Phase 52 slice 5, §5.6): the tool stands where the program
/// says. A click on a program line names a move; the tool body's pose is
/// that move's end point (`view::block_end`), not the operation's deepest
/// point; and the block grain gains a slider beside the op slider.
#[test]
fn the_tool_stands_at_the_move_a_program_click_names() {
    let mut h = open();
    settle(&mut h);
    // The window opens with every op run, so the highlighted op (the last)
    // has moves and the block slider stands beside the op slider already.
    assert_eq!(h.query_all_by_role(Role::Slider).count(), 2, "the op slider and the block slider");
    assert_eq!(h.state().block_cursor(), None, "and no move is named yet");
    // The first feed move on screen belongs to the first operation.
    h.query_all_by_label_contains("G01").next().expect("a feed move of op 0 is on screen").click();
    settle(&mut h);
    assert!(matches!(h.state().selection(), view::Selection::Operation(0)), "the click selected the operation");
    assert_eq!(h.state().layout().form.open, true);
    let b = h.state().block_cursor().expect("the click named a move");
    let expected = view::block_end(h.state().emitted_moves(), 0, b).expect("the move has an end");
    let (tip, tool) = h.state().tool_pose_drawn().expect("the tool is posed");
    assert_eq!((tip[0], tip[1]), (expected[0], expected[1]), "the tool stands over the move's end");
    assert!(tip[2] <= expected[2], "dropped by the silhouette to the tip");
    assert!(tool >= 1, "the op's own tool number, T{tool}");
    assert_eq!(h.query_all_by_role(Role::Slider).count(), 2, "the block slider beside the op slider");
    // The stock follows the move (Phase 52 slice 6): with the simulation
    // layer on, the surface in hand is built through the block, and a move
    // of the block makes it stale — the worker rebuilds it.
    h.get_by_label("Layers").click();
    settle(&mut h);
    h.get_by_label("Stock simulation").click();
    // The worker builds and the status line spins, so the window keeps
    // asking to repaint: step a few frames rather than settle.
    h.step();
    h.step();
    assert_eq!(h.state().sim_built_through(), (1, Some(b)), "the build was asked for op 1 through move {b}");
    // The op keys clear the block (the nesting invariant); the pose returns
    // to the deepest point. Stepped, not settled: the worker is still
    // building and the window still spinning.
    h.key_press(egui::Key::CloseBracket);
    h.step();
    h.step();
    assert_eq!(h.state().block_cursor(), None);
}

/// Phase 52 slice 14 (§5.6): one cursor, three doors, one answer. The
/// block cursor is a fact of the cursor, not of the selection — a click
/// that selects a feature leaves the tool at the move; a Program click on
/// the header seeks to before operation 1, the move row says so and the
/// tool stands nowhere — the way out of the program the user could not
/// find ("once I click into the Program, my only way out … is the
/// inter-op scrubber").
#[test]
fn one_cursor_three_doors_one_answer() {
    let mut h = open();
    settle(&mut h);
    h.query_all_by_label_contains("G01").next().expect("a feed move of op 0 is on screen").click();
    settle(&mut h);
    let b = h.state().block_cursor().expect("the click named a move");
    let at_move = h.state().tool_pose_drawn().expect("the tool stands at the move");
    assert_eq!(h.state().op_cursor(), 1);
    // A click elsewhere SELECTS and never scrubs: the feature's row.
    row(&h, "Notch  ProfilePocket").click();
    settle(&mut h);
    assert!(matches!(h.state().selection(), Selection::Feature(_)), "the click selected the feature");
    assert_eq!(h.state().block_cursor(), Some(b), "the move stands");
    assert_eq!(h.state().op_cursor(), 1, "the cursor stands");
    assert_eq!(h.state().tool_pose_drawn(), Some(at_move), "the tool stays at the move whatever is selected");
    // The header line: before operation 1, nothing named, no tool.
    h.query_all_by_label_contains("nacs").next().expect("the program's first comment line is on screen").click();
    settle(&mut h);
    assert_eq!(h.state().op_cursor(), 0, "a header click seeks to before operation 1");
    assert_eq!(h.state().block_cursor(), None);
    assert!(matches!(h.state().selection(), Selection::Feature(_)), "the header selects nothing — the feature stays selected");
    assert_eq!(h.state().tool_pose_drawn(), None, "no operation faces, no tool");
    assert!(h.query_all_by_label_contains("Shown before any operation").next().is_some(), "the op row says so");
    assert!(h.query_all_by_label_contains("step on for its moves").next().is_some(), "and the move row");
}

/// Phase 53 slice 2 (§5.4, fork 1A): a loop feature's card says its side
/// in words and carries *Flip side*. On plate.cam the one parser REFUSES
/// the flip by the Clear's own law (a boss is roughed in Z levels and
/// wants a stepdown): the status line says that sentence and nothing is
/// written. On profile_pocket.cam, whose Window has a Side alone, the
/// click rewrites the feature's block — the kind word swapped, the points
/// the other way — the window follows the file, and Undo puts it back.
/// Both on COPIES: the fixtures of record are never written.
/// Right-click the feature's row and fire *Flip side* from its menu — the
/// menu's row is the one nearest the top (the card's button stands lower,
/// in the form).
fn flip_by_menu(h: &mut Harness<'static, ViewApp>, feature: &str) {
    row(h, feature).click_secondary();
    settle(h);
    let node = h.query_all_by_label("Flip side").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the menu's Flip side");
    node.click();
}

#[test]
fn a_loops_side_is_said_on_its_card_and_flipped_by_one_click() {
    let (mut h, project) = open_copy("plate.cam");
    settle(&mut h);
    row(&h, "Notch  ProfilePocket").click();
    settle(&mut h);
    assert!(h.query_all_by_label_contains("a void inside the loop").next().is_some(), "the pocket's side, in words");
    let before = std::fs::read_to_string(project.join("geometry.part")).unwrap();
    assert!(h.query_all_by_label("Flip side").next().is_some(), "the card carries the door");
    // The menu's row fires the act (the card's button stands in the form's
    // scroll area, below what the harness's window shows of it).
    flip_by_menu(&mut h, "Notch  ProfilePocket");
    settle(&mut h);
    assert!(h.query_all_by_label_contains("'Notch' is a standing boss").next().is_some(), "refused by the Clear's law, said on the status line");
    assert_eq!(std::fs::read_to_string(project.join("geometry.part")).unwrap(), before, "nothing written");
    assert!(h.query_all_by_label_contains("Notch  ProfilePocket").next().is_some());
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
    // Where the ops read either side the same way, the flip lands.
    let (mut h, project) = open_copy("profile_pocket.cam");
    settle(&mut h);
    row(&h, "Window  ProfilePocket").click();
    settle(&mut h);
    flip_by_menu(&mut h, "Window  ProfilePocket");
    // The file is written and the watch reloads on its slow timer.
    std::thread::sleep(std::time::Duration::from_millis(700));
    settle(&mut h);
    settle(&mut h);
    let main = std::fs::read_to_string(project.join("main.part")).unwrap();
    assert!(main.contains("feature Window ProfileBoss(extent=2.0"), "the copy's file carries the flipped kind: {main}");
    assert!(h.query_all_by_label_contains("Window  ProfileBoss").next().is_some(), "the window followed the file");
    assert!(h.query_all_by_label_contains("standing material inside the loop").next().is_some(), "and says the boss's side");
    // Undo: the write before, byte for byte, and the window follows again.
    h.get_by_label("Undo").click();
    std::thread::sleep(std::time::Duration::from_millis(700));
    settle(&mut h);
    settle(&mut h);
    assert_eq!(std::fs::read_to_string(project.join("main.part")).unwrap(), std::fs::read_to_string("profile_pocket.cam/main.part").unwrap(), "undone to the fixture's bytes");
    assert!(h.query_all_by_label_contains("Window  ProfilePocket").next().is_some());
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// Phase 54 slice 1 (§5.8, fork 0A): `3` opens the bottom dock on *Output*;
/// a sentence the status line posts (Undo with nothing to undo) stands
/// there with its time and stays after the line has moved on; *Clear*
/// empties it; `3` again shuts the dock.
#[test]
fn the_output_dock_keeps_what_the_status_line_said() {
    let mut h = open();
    settle(&mut h);
    assert!(!h.state().layout().bottom.open);
    assert!(h.query_by_label("Output").is_none(), "shut: no dock");
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    assert!(h.state().layout().bottom.open, "3 opens the dock");
    assert!(h.query_by_label("Output").is_some(), "on Output");
    // `3` gave the runner's line the keyboard (slice 2); Esc gives it back
    // so the letter keys are acts again.
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    // A refusal the status line posts: Undo with nothing to undo.
    h.key_press(egui::Key::Z);
    settle(&mut h);
    let hits: Vec<_> = h.query_all_by_label_contains("Nothing to undo").collect();
    assert!(hits.len() >= 2, "the sentence stands on the status line and in the log: {}", hits.len());
    assert!(h.query_all_by_label_contains("  Nothing to undo").next().is_some(), "the log's line carries its time before the sentence");
    // A second sentence, then Clear: the earlier one is gone from the log;
    // the one the line is still saying is posted again at once, with its
    // new time — the line is still saying it.
    h.key_press(egui::Key::Y);
    settle(&mut h);
    assert!(h.query_all_by_label_contains("  Nothing to redo").next().is_some());
    h.get_by_label("Clear").click();
    settle(&mut h);
    assert!(h.query_all_by_label_contains("  Nothing to undo").next().is_none(), "cleared");
    assert!(h.query_all_by_label_contains("  Nothing to redo").next().is_some(), "the standing sentence is posted again");
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    assert!(!h.state().layout().bottom.open, "3 shuts it");
}

/// Phase 54 slice 2 (§5.8, fork 1A): the dock's bottom line runs the
/// house's own verbs as processes — `nacs --outline` on this project,
/// echoed, its words in the log line by line, its exit last — and a
/// verb the runner does not know is refused by name on the status line.
#[test]
fn the_runner_runs_the_houses_verbs_into_the_log() {
    let mut h = open();
    settle(&mut h);
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    // The runner's field is the lowest text input on screen; `3` gave it
    // the keyboard, and the harness says so explicitly (a Text event goes
    // to the focused widget).
    let field = h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line");
    field.focus();
    h.step();
    h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().type_text("nacs --outline");
    h.step();
    h.key_press(egui::Key::Enter);
    // The run is a worker: settle waits it out.
    settle(&mut h);
    settle(&mut h);
    assert!(h.query_all_by_label_contains("$ nacs --outline").next().is_some(), "the command echoed");
    assert!(h.query_all_by_label_contains("stock  120.000 x 80.000").next().is_some(), "the outline's first line in the log");
    assert!(h.query_all_by_label_contains("exit 0").next().is_some(), "the exit said last");
    let field = h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line");
    field.focus();
    h.step();
    h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().type_text("frobnicate");
    h.step();
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(h.query_all_by_label_contains("`frobnicate` is not a verb the runner knows").next().is_some(), "refused by name");
}

/// Phase 56 slice 2 (forks 1A + 2A): a sentence is posted once — the
/// runner's "Running …" stands in the log one time around a run's lines,
/// not before and again after them — and a refusal of the runner's line
/// lives as long as that line: typed over, it leaves the status line and
/// stays in the log as history.
#[test]
fn a_sentence_is_posted_once_and_a_refusal_lives_as_long_as_its_line() {
    let mut h = open();
    settle(&mut h);
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    macro_rules! line {
        () => {
            h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line")
        };
    }
    line!().focus();
    h.step();
    line!().type_text("nacs --outline");
    h.step();
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    settle(&mut h);
    assert!(h.query_all_by_label_contains("exit 0").next().is_some(), "the run ended");
    let running = h.query_all_by_label_contains("Running nacs --outline").count();
    assert_eq!(running, 1, "Running … once in the log, not again after the run's lines: {running}");
    line!().focus();
    h.step();
    line!().type_text("frobnicate");
    h.step();
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    let refused = h.query_all_by_label_contains("`frobnicate` is not a verb").count();
    assert_eq!(refused, 2, "the refusal on the status line and in the log: {refused}");
    // Typed over: the status line lets it go, the log keeps it.
    line!().focus();
    h.step();
    line!().type_text("x");
    settle(&mut h);
    let after = h.query_all_by_label_contains("`frobnicate` is not a verb").count();
    assert_eq!(after, 1, "edited, the refusal is history in the log alone: {after}");
}

/// Phase 54 slice 3 (§5.3, fork 2A): the chip is a menu of git's five
/// verbs. *Commit…* opens the dock with `git commit -m ` on its line and
/// the keyboard; *Fetch* runs at once — on a copy that is no repository,
/// git's own sentence and a non-zero exit land in the log; a typed
/// `git push --force` is refused by name and runs nothing.
#[test]
fn the_chip_offers_gits_verbs_and_nothing_here_rewrites_history() {
    let (mut h, project) = open_copy("plate.cam");
    // A repository of its own, with no remote: what git says to a fetch
    // is then the same whatever directory the copy stands in.
    assert!(std::process::Command::new("git").args(["-C", &project.to_string_lossy(), "init", "-q"]).status().map(|s| s.success()).unwrap_or(false), "git init in the copy");
    settle(&mut h);
    std::thread::sleep(std::time::Duration::from_millis(600));
    settle(&mut h);
    let chip = h.query_all_by_label_contains("plate.cam").filter(|n| n.rect().left() > 600.0).next().expect("the chip");
    chip.click();
    settle(&mut h);
    for label in ["Fetch", "Pull", "Push", "Commit…", "Branch…"] {
        assert!(h.query_by_label(label).is_some(), "{label} on the chip's menu");
    }
    h.get_by_label("Commit…").click();
    settle(&mut h);
    assert!(h.state().layout().bottom.open, "Commit… opens the dock");
    assert_eq!(h.state().runner_line(), "git commit -m ", "the message's opening on the line");
    // Push, on a repository with no remote: git says so, the exit is not 0
    // (a fetch with no remote is a silent no-op, found by the pin).
    let chip = h.query_all_by_label_contains("plate.cam").filter(|n| n.rect().left() > 600.0).next().expect("the chip");
    chip.click();
    settle(&mut h);
    h.get_by_label("Push").click();
    settle(&mut h);
    settle(&mut h);
    let log = h.state().output_lines().join(" | ");
    assert!(log.contains("$ git push"), "echoed: {log}");
    assert!(log.contains("No configured push destination"), "git's own sentence in the log: {log}");
    assert!(log.contains("exit 128"), "and its exit: {log}");
    // Typed: refused by name, nothing run.
    let field = h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line");
    field.focus();
    h.step();
    h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().type_text("git push --force");
    h.step();
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(h.query_all_by_label_contains("refused by name").next().is_some());
    assert!(!h.state().output_lines().join(" | ").contains("$ git push --force"), "nothing ran");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// Phase 54 slice 5b (§5.2): the Stock header carries *New stock…* now
/// that the noun exists; it opens the dock with `stock ` on the line and
/// the keyboard — the model's path is the user's to finish.
#[test]
fn new_stock_offers_the_stock_verb_on_the_output_line() {
    let mut h = open();
    settle(&mut h);
    assert!(!h.state().layout().bottom.open);
    h.get_by_label("New stock…").click();
    settle(&mut h);
    assert!(h.state().layout().bottom.open, "New stock… opens the dock");
    assert_eq!(h.state().runner_line(), "stock ", "the verb's opening on the line");
}

/// Every door's word is drawable. The shell's default fonts lack some
/// symbols, and a label they cannot draw is a square — the user's finding
/// on 2026-10-17: the transport's ↶ ↷, now *Undo* and *Redo*. Every label
/// a door, a tab, a menu row or the transport wears, asked of egui's own
/// fonts (`has_glyphs`), so the next arrow is a red test and not a square.
#[test]
fn every_door_wears_glyphs_the_fonts_have() {
    let mut h = open();
    settle(&mut h);
    let mut labels: Vec<String> = vec![view::MENU_GLYPH.into(), "×".into(), "⏴".into(), "⏵".into(), "⏶".into(), "⏷".into(), view::SIDEBAR_DOOR.1.into()];
    labels.extend(view::VIEW_STRIP.iter().chain(view::TRANSPORT_DOORS).chain(view::STATUS_DOORS).chain(view::CAPTION_DOORS).map(|(_, l, _)| l.to_string()));
    labels.push(view::caption_glyph("window.maximize", true).to_string());
    labels.extend(view::tabs().iter().map(|t| t.label.to_string()));
    labels.extend(view::MENU.iter().flat_map(|(title, rows)| std::iter::once(title.to_string()).chain(rows.iter().map(|(_, l)| l.to_string()))));
    let font = egui::FontId::proportional(14.0);
    let missing: Vec<String> = labels.into_iter().filter(|l| !l.is_empty() && !h.ctx.fonts_mut(|f| f.has_glyphs(&font, l))).collect();
    assert!(missing.is_empty(), "labels the fonts cannot draw: {missing:?}");
}

// ---- feeling 1: the palette --------------------------------------------------

#[test]
fn the_palette_opens_from_the_commands_door_and_esc_closes_it() {
    let mut h = open();
    settle(&mut h);
    assert!(!h.state().palette_is_open());
    h.get_by_label("Commands").click();
    settle(&mut h);
    assert!(h.state().palette_is_open());
    // Its filter field has the focus, so typing filters at once.
    assert!(h.query_by_role(Role::TextInput).is_some_and(|n| n.is_focused()));
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(!h.state().palette_is_open());
    assert!(h.query_by_role(Role::TextInput).is_none());
}

/// I14 (Phase 51 slice 2): the palette brings a `×` of its own, and a
/// click on the picture closes it — it was "locked open", the user said.
/// The click that OPENS it, which lands outside it in the same frame, does
/// not count as the click outside.
#[test]
fn the_palette_closes_on_its_close_button_and_on_a_click_outside() {
    let mut h = open();
    settle(&mut h);
    let docks_close = count_label(&h, "×");
    h.get_by_label("Commands").click();
    settle(&mut h);
    assert!(h.state().palette_is_open(), "the opening click is not the closing click");
    assert_eq!(count_label(&h, "×"), docks_close + 1, "the palette's own ×");
    click_outside(&mut h);
    assert!(!h.state().palette_is_open(), "a click outside closes the palette (I14)");
    // And the ×.
    h.get_by_label("Commands").click();
    settle(&mut h);
    assert!(h.state().palette_is_open());
    let palette_close = h.query_all_by_label("×").last().expect("the palette's ×");
    palette_close.click();
    settle(&mut h);
    assert!(!h.state().palette_is_open(), "the × closes the palette");
    // A row still runs: Commands, filter, Enter → the act, and the palette shuts.
    h.get_by_label("Commands").click();
    settle(&mut h);
    h.get_by_role(Role::TextInput).type_text("this help");
    settle(&mut h);
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    assert!(!h.state().palette_is_open() && h.state().help_is_open(), "Enter ran the lit row");
}

// ---- feeling 1b: the help ----------------------------------------------------

#[test]
fn the_help_opens_from_its_door_and_esc_closes_it() {
    let mut h = open();
    settle(&mut h);
    h.get_by_label("?").click();
    settle(&mut h);
    assert!(h.state().help_is_open());
    // The help's keys section is generated from ACTIONS: the palette's line.
    assert!(h.query_by_label_contains("quick-action palette").is_some());
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(!h.state().help_is_open());
}

/// I14 (Phase 51 slice 2): a click outside closes the help, and so does
/// its `×`; the `?` door's own click, which opens it, does not.
#[test]
fn the_help_closes_on_a_click_outside_and_on_its_close_button() {
    let mut h = open();
    settle(&mut h);
    let docks_close = count_label(&h, "×");
    h.get_by_label("?").click();
    settle(&mut h);
    assert!(h.state().help_is_open(), "the opening click is not the closing click");
    assert_eq!(count_label(&h, "×"), docks_close + 1, "the help's own ×");
    click_outside(&mut h);
    assert!(!h.state().help_is_open(), "a click outside closes the help (I14)");
    h.get_by_label("?").click();
    settle(&mut h);
    h.query_all_by_label("×").last().expect("the help's ×").click();
    settle(&mut h);
    assert!(!h.state().help_is_open(), "the × closes the help");
}

/// I14's third kind, already egui's own behaviour, pinned so it stays: a
/// context menu closes on a click outside it.
#[test]
fn a_context_menu_closes_on_a_click_outside() {
    let mut h = open();
    settle(&mut h);
    // The right-click also SELECTS the feature, so its card's own New
    // operation… button appears on the form beside the menu's row: the
    // menu is the one that goes when the click lands outside.
    row(&h, "Notch  ProfilePocket").click_secondary();
    settle(&mut h);
    let with_menu = count_label(&h, "New operation…");
    assert!(with_menu >= 1, "the menu opened");
    click_outside(&mut h);
    assert_eq!(count_label(&h, "New operation…"), with_menu - 1, "a click outside closed the menu; the card's button stays");
}

// ---- feeling 2: the layers ---------------------------------------------------

/// Fork 2 (Phase 51 slice 2): the Layers group is folded with no pointer
/// over it, unfolds while the pointer is there, folds when it leaves — no
/// act shuts it — and a click on its header pins it open, which the
/// workspace remembers as `layers.pinned`. Since Phase 52 slice 11 the
/// unfold WAITS `view::LAYERS_HOVER_DELAY`: a pointer that only crosses
/// the button opens nothing; the fold is immediate; the pin never waits.
#[test]
fn the_layers_group_opens_while_hovered_and_is_pinned_by_a_click() {
    let mut h = open();
    h.remove_cursor();
    settle(&mut h);
    assert!(h.query_by_label("Layers").is_some(), "the header line keeps its corner");
    assert!(h.query_by_label("Stock simulation").is_none(), "folded with no pointer over it");
    // Hover the header: nothing on the frame the pointer arrives — it may
    // be passing through. One frame, stepped: a settle would run the
    // harness's clock (a quarter second a frame) past the delay.
    h.get_by_label("Layers").hover();
    h.step();
    assert!(h.query_by_label("Stock simulation").is_none(), "a pointer that just arrived opens nothing (slice 11)");
    // Rest the delay (the settle runs the clock on, the window asking for
    // the frame the delay ends): the list unfolds with the pointer unmoved.
    settle(&mut h);
    settle(&mut h);
    assert!(h.query_by_label("Stock simulation").is_some(), "unfolded under the resting pointer");
    // Leave: it folds, with no click anywhere.
    h.remove_cursor();
    settle(&mut h);
    settle(&mut h);
    assert!(h.query_by_label("Stock simulation").is_none(), "folded when the pointer left");
    // Click the header: pinned, and it stays open with the pointer gone.
    h.get_by_label("Layers").click();
    settle(&mut h);
    h.remove_cursor();
    settle(&mut h);
    settle(&mut h);
    assert!(h.state().layout().layers_pinned);
    assert!(h.query_by_label("Stock simulation").is_some(), "pinned open");
    assert!(h.query_by_label("Layers  (pinned)").is_some(), "and the header says so");
    assert!(h.state().layout().render().contains("layers.pinned true"), "the workspace remembers the pin");
    // A second click unpins.
    h.get_by_label("Layers  (pinned)").click();
    settle(&mut h);
    h.remove_cursor();
    settle(&mut h);
    settle(&mut h);
    assert!(!h.state().layout().layers_pinned);
    assert!(h.query_by_label("Stock simulation").is_none());
}

// ---- feeling 3: the form's doors ---------------------------------------------

/// Fork 1 (Phase 51 slice 3, §5.4): with the form shut, clicking a feature
/// selects it AND opens the form on its card — the card used to be
/// composed into a shut dock, "live and invisible". The `×` shuts it
/// again until the next selection.
#[test]
fn a_feature_clicked_with_the_form_shut_opens_the_form_on_its_card() {
    let mut h = open_with(workspace_form_shut(SidebarTab::Outline));
    settle(&mut h);
    assert!(!h.state().layout().form.open);
    assert!(h.query_by_label("form").is_none(), "the dock's header line is not drawn while it is shut");
    row(&h, "Notch  ProfilePocket").click();
    settle(&mut h);
    assert!(matches!(h.state().selection(), Selection::Feature(_)), "the click selected the feature");
    assert!(h.state().layout().form.open, "a selection opens the form (I14)");
    assert!(h.query_by_label("form").is_some());
    assert!(h.query_by_label("Delete feature").is_some(), "the feature's card is on it");
    // The × shuts it, and it stays shut until something is selected again.
    h.get_by_label("form").hover();
    let form_close = h.query_all_by_label("×").last().expect("the form's ×");
    form_close.click();
    settle(&mut h);
    assert!(!h.state().layout().form.open, "× shuts the form");
    row(&h, "TopFace  Plane").click();
    settle(&mut h);
    assert!(h.state().layout().form.open, "the next selection opens it again");
}

/// Fork 1 (Phase 51 slice 3): the right-click menu on a feature leads with
/// *Edit…*, which opens the form on it — the way to a thing's settings
/// from the thing itself. (The right-click already selects, and a
/// selection opens the form, so the row is the explicit door beside the
/// implicit one.)
#[test]
fn the_feature_context_menu_leads_with_edit_which_opens_the_form() {
    let mut h = open_with(workspace_form_shut(SidebarTab::Outline));
    settle(&mut h);
    row(&h, "Notch  ProfilePocket").click_secondary();
    settle(&mut h);
    let edit = h.get_by_label("Edit…");
    edit.click();
    settle(&mut h);
    assert!(h.state().layout().form.open, "Edit… opens the form");
    assert!(h.query_by_label("Delete feature").is_some(), "on the feature's card");
    assert!(h.query_by_label("Edit…").is_none(), "and the menu closed");
}

/// The `2` key still toggles the form — shut, then open again on the
/// selected feature's card (its name and kind). Since slice 3 the click
/// itself opens the form, so the key's first press shuts it.
#[test]
fn the_form_key_shows_the_selected_features_card() {
    let mut h = open_with(workspace_form_shut(SidebarTab::Outline));
    settle(&mut h);
    row(&h, "Notch  ProfilePocket").click();
    settle(&mut h);
    assert!(h.state().layout().form.open, "the click opened it (slice 3)");
    h.key_press(egui::Key::Num2);
    settle(&mut h);
    assert!(!h.state().layout().form.open, "2 shuts it");
    h.key_press(egui::Key::Num2);
    settle(&mut h);
    assert!(h.state().layout().form.open);
    assert!(h.query_by_label("form").is_some(), "the dock's header line");
    assert!(h.query_all_by_label_contains("Notch  ProfilePocket").count() >= 2, "the row and the card's first line");
    assert!(h.query_by_label("Delete feature").is_some());
}

// ---- feeling 4: the form under the other tabs --------------------------------

/// Fork 3 (Phase 51 slice 3, §5.4): an operation's card does not follow
/// the sidebar to Problems or Review — a tab that could not have made the
/// card shuts the form; a tab that could (the Sequence) leaves it; and the
/// next click in any tab reopens the form on that row's own card.
#[test]
fn a_tab_change_shuts_a_card_the_tab_could_not_have_made() {
    let mut h = open();
    settle(&mut h);
    row(&h, "[0] End").click();
    settle(&mut h);
    assert!(matches!(h.state().selection(), Selection::Operation(0)));
    assert!(h.state().layout().form.open);
    assert!(h.query_by_label("Suppress operation").is_some());
    // Problems' rows are findings: the operation's card is stale there.
    h.get_by_label_contains("Problems").click();
    settle(&mut h);
    assert_eq!(h.state().layout().sidebar.tab, SidebarTab::Problems);
    assert!(!h.state().layout().form.open, "a tab that cannot make the card shuts the form");
    assert!(h.query_by_label("Suppress operation").is_none(), "the stale card is gone");
    assert!(matches!(h.state().selection(), Selection::Operation(0)), "the selection itself stands (I6: one selection, written by clicks)");
    // Back to the Outline: still shut — a tab change never OPENS the form;
    // a click does.
    h.get_by_label_contains("Outline").click();
    settle(&mut h);
    assert!(!h.state().layout().form.open);
    row(&h, "[2] Side").click();
    settle(&mut h);
    assert!(h.state().layout().form.open);
    assert!(matches!(h.state().selection(), Selection::Operation(2)));
}

// ---- feeling 5: the theme ----------------------------------------------------

/// Fork 5 (Phase 51 slice 5, §5.7): a theme named but found nowhere is a
/// status-line sentence that names the PATH it looked at, so a user can
/// tell where a copy would go.
#[test]
fn a_missing_theme_names_the_path_it_looked_at() {
    let mut h = open_with(Workspace { theme: Some("no-such-theme".to_string()), ..Workspace::default() });
    settle(&mut h);
    let note = h.state().theme_note().expect("a missing theme leaves a note");
    assert!(note.contains("not found at") && note.contains("no-such-theme.json") && note.contains("themes"), "{note}");
    assert!(h.query_by_label_contains("not found at").is_some(), "on the status line, where every note lives");
}

/// Fork 5 (Phase 51 slice 5): a shipped theme loads by name with no file
/// anywhere — the config folder is a scratch directory here — and Window ▸
/// Theme lists it checked; a pick re-dresses the window, is remembered in
/// the workspace, and says when its glass must wait for a launch.
#[test]
fn a_shipped_theme_loads_by_name_and_the_theme_menu_picks_one() {
    let mut h = open_with(Workspace { theme: Some("tokyo-night".to_string()), ..Workspace::default() });
    settle(&mut h);
    assert_eq!(h.state().theme_choice(), Some("tokyo-night"));
    assert_eq!(h.state().theme_note(), None, "the shipped copy loads clean");
    h.get_by_label(view::MENU_GLYPH).click();
    settle(&mut h);
    submenu(&h, "Window").expect("the Window submenu").click();
    settle(&mut h);
    submenu(&h, "Theme").expect("the Theme submenu").click();
    settle(&mut h);
    assert_eq!(h.get_by_label("tokyo-night").accesskit_node().toggled(), Some(Toggled::True), "the worn theme is checked");
    assert_eq!(h.get_by_label("Default (built-in)").accesskit_node().toggled(), Some(Toggled::False));
    // Pick the blur theme from an opaque launch: applied, remembered, and
    // honest about the glass.
    h.get_by_label("catppuccin-espresso-blur-light").click();
    settle(&mut h);
    assert_eq!(h.state().theme_choice(), Some("catppuccin-espresso-blur-light"));
    assert!(h.state().transient_note().is_some_and(|n| n.contains("next launch")), "{:?}", h.state().transient_note());
    assert!(h.query_by_label_contains("next launch").is_some(), "said on the status line");
    assert!(submenu(&h, "Window").is_none(), "the menu closed on the pick");
    let saved = std::fs::read_to_string(nacs::theme::workspace_path().expect("a redirected config home")).expect("the pick wrote the workspace");
    assert!(saved.contains("theme catppuccin-espresso-blur-light\n"), "{saved}");
    // Back to the built-in: no note, no theme line.
    h.get_by_label(view::MENU_GLYPH).click();
    settle(&mut h);
    submenu(&h, "Window").expect("the Window submenu").click();
    settle(&mut h);
    submenu(&h, "Theme").expect("the Theme submenu").click();
    settle(&mut h);
    h.get_by_label("Default (built-in)").click();
    settle(&mut h);
    assert_eq!(h.state().theme_choice(), None);
    assert_eq!(h.state().theme_note(), None);
}

// ---- the menu (fork 4) -------------------------------------------------------

/// Fork 4 (Phase 51 slice 4, §5.1 reopened): the corner menu stands at
/// the top of the rail; its rows fire the same acts a key does; a state
/// row wears its check; a click outside closes it (I14).
#[test]
fn the_corner_menu_opens_lists_the_tree_and_its_rows_act() {
    let mut h = open();
    settle(&mut h);
    assert!(submenu(&h, "Window").is_none(), "shut: no submenu titles on screen");
    let strip_write = count_label(&h, "Write program");
    h.get_by_label(view::MENU_GLYPH).click();
    settle(&mut h);
    for title in ["File", "Edit", "View", "Window", "Help"] {
        assert!(submenu(&h, title).is_some(), "submenu {title}");
    }
    // The job's verbs lead: Write program is on the top group (the
    // Program's strip has its own).
    assert_eq!(count_label(&h, "Write program"), strip_write + 1);
    // A click outside closes the menu.
    click_outside(&mut h);
    assert!(submenu(&h, "Window").is_none(), "a click outside closed the menu (I14)");
    // Window ▸ Sidebar: checked while the sidebar is open; the row shuts it.
    h.get_by_label(view::MENU_GLYPH).click();
    settle(&mut h);
    submenu(&h, "Window").expect("the Window submenu").click();
    settle(&mut h);
    let sidebar_row = h.get_by_label("Sidebar");
    assert_eq!(sidebar_row.accesskit_node().toggled(), Some(Toggled::True), "the check: the sidebar is open");
    sidebar_row.click();
    settle(&mut h);
    assert!(!h.state().layout().sidebar.open, "the row fired sidebar.toggle");
    assert!(submenu(&h, "Window").is_none(), "and the menu closed on the act");
    // Help ▸ Help opens the help, the same act as `?`.
    h.get_by_label(view::MENU_GLYPH).click();
    settle(&mut h);
    submenu(&h, "Help").expect("the Help submenu").click();
    settle(&mut h);
    h.get_by_label("Help").click();
    settle(&mut h);
    assert!(h.state().help_is_open());
}

/// Phase 56 slice 1 (fork 0A): the dock holds still. Phase 54's walk saw
/// the dock crawl to the top of the window frame by frame with its line
/// pushed out of frame: the content stood taller than the panel and
/// egui's panel stored the drawn rect as the next frame's size. So the
/// measurement, not a feeling: `3` opens the dock, the window is stepped
/// frame by frame — never settled, a settle hides a crawl — and the dock's
/// height on frame thirty is the height on frame two, with the runner's
/// line inside the dock and inside the window on both.
#[test]
fn the_dock_holds_still_frame_by_frame() {
    let mut h = open();
    settle(&mut h);
    let window_h = 800.0;
    h.key_press(egui::Key::Num3);
    h.step();
    h.step();
    assert!(h.state().layout().bottom.open, "3 opens the dock");
    let line_rect = |h: &Harness<'static, ViewApp>| {
        h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line").rect()
    };
    let h2 = h.state().layout().bottom.height;
    let l2 = line_rect(&h);
    for _ in 0..28 {
        h.step();
    }
    let h30 = h.state().layout().bottom.height;
    let l30 = line_rect(&h);
    // The crawl first, so a red run says how far it went, then the line.
    assert!((h30 - h2).abs() <= 0.5, "the dock crawled: {h2} tall on frame 2, {h30} on frame 30 (the line's bottom {} then {}, in a window {window_h} tall)", l2.bottom(), l30.bottom());
    assert!(l2.bottom() <= window_h + 0.5, "frame 2: the line is inside the window (bottom {} of {window_h})", l2.bottom());
    assert!(l2.top() >= window_h - h2 - 0.5, "frame 2: the line is inside the dock (top {} with the dock {h2} tall)", l2.top());
    assert!(l30.bottom() <= window_h + 0.5, "frame 30: the line is inside the window (bottom {} of {window_h})", l30.bottom());
    assert!(l30.top() >= window_h - h30 - 0.5, "frame 30: the line is inside the dock (top {} with the dock {h30} tall)", l30.top());
    // And the log still reads top-down — the cure placed the log from the
    // bottom, which must not turn its lines over: an earlier sentence
    // stands above a later one.
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    h.key_press(egui::Key::Z);
    settle(&mut h);
    h.key_press(egui::Key::Y);
    settle(&mut h);
    let undo = h.query_all_by_label_contains("  Nothing to undo").next().expect("the earlier sentence in the log").rect();
    let redo = h.query_all_by_label_contains("  Nothing to redo").next().expect("the later sentence in the log").rect();
    assert!(undo.top() < redo.top(), "the log reads top-down: the earlier line at {} above the later at {}", undo.top(), redo.top());
}

/// Phase 56 slice 4 (fork 4A): *New stock…* on a project that does not
/// exist yet makes it — the window opened on a directory with no
/// main.part, `stock <model>` typed on the line, the sidecar's proposal
/// accepted as a new main.part (the template's header, the planes above
/// the model's top, the Profile stock) and the project reloaded. Runs the
/// real sidecar, so it skips by name without the venv.
#[test]
fn new_stock_on_a_project_not_yet_made_makes_it() {
    #[cfg(windows)]
    const VENV_PYTHON: &str = "sidecar/.venv/Scripts/python.exe";
    #[cfg(not(windows))]
    const VENV_PYTHON: &str = "sidecar/.venv/bin/python";
    if !std::path::Path::new(VENV_PYTHON).is_file() {
        eprintln!("skipped: no sidecar venv at {VENV_PYTHON}");
        return;
    }
    redirect_config_home();
    let test = std::thread::current().name().unwrap_or("t").replace("::", "-");
    let root = std::env::temp_dir().join(format!("nacs-shell-empty-{}-{}", std::process::id(), test));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("lplate.cam");
    assert!(!project.exists(), "the project does not exist yet");
    let path = project.to_string_lossy().into_owned();
    let part = view::empty_project_part(&project);
    let cr = nacs::check_result::CheckResult { refusal: Some("no main.part yet".to_string()), findings: Vec::new(), emitted_moves: None };
    let mut h = Harness::builder()
        .with_size(egui::Vec2::new(1200.0, 800.0))
        .with_max_steps(64)
        .build_eframe(move |_cc| frontend::harness_app(&part, cr, &path, None, Workspace::default()));
    settle(&mut h);
    // The empty stance's sentence is painted, not a node; what the tree
    // shows is that no stock row stands.
    assert!(h.query_all_by_label_contains("profile of 6 points").next().is_none(), "the empty stance shows no stock");
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    let step = std::fs::canonicalize("tests/fixtures/l_plate.step").unwrap();
    let step = step.to_string_lossy().trim_start_matches("\\\\?\\").to_string();
    macro_rules! line {
        () => {
            h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line")
        };
    }
    line!().focus();
    h.step();
    line!().type_text(&format!("stock {step}"));
    h.step();
    h.key_press(egui::Key::Enter);
    // CadQuery takes seconds to wake: wait the run out, up to two minutes.
    let mut done = false;
    for _ in 0..1200 {
        settle(&mut h);
        if h.state().output_lines().iter().any(|l| l.contains("exit ")) {
            done = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let log = h.state().output_lines().join(" | ");
    assert!(done, "the stock run ended: {log}");
    assert!(log.contains("exit 0"), "the sidecar proposed: {log}");
    settle(&mut h);
    settle(&mut h);
    // The proposal is REVIEWED, not applied (ROADMAP item 1, slice 1): the
    // Review stands in front with the stock row selected, the card shows
    // the statement it will write, and nothing is on disk yet.
    let main = project.join("main.part");
    assert!(!main.exists(), "nothing written before Accept");
    assert_eq!(h.state().layout().sidebar.tab, nacs::theme::SidebarTab::Review, "the Review in front");
    assert!(matches!(h.state().selection(), Selection::ProposedStock), "the stock row selected: {:?}", h.state().selection());
    assert!(h.query_all_by_label("Stock  Profile of 6 points").next().is_some(), "the row (and the card's head)");
    assert!(h.query_all_by_label_contains("stock Profile(h=20.0, z=20.0").next().is_some(), "the card shows the statement");
    h.get_by_label("Accept into project").click();
    settle(&mut h);
    let text = std::fs::read_to_string(&main).unwrap_or_else(|e| panic!("main.part made: {e} — {log}"));
    assert!(text.contains("rapid_z 25.000\nclearance_z 45.000\n"), "the planes above the model's top: {text}");
    assert!(text.contains("stock Profile(h=20.0, z=20.0"), "the model's stock: {text}");
    assert!(h.state().output_lines().join(" | ").contains("main.part made from the model: a stock Profile of 6 points"), "said: {log}");
    // The reload rides the project poll, a tick behind the write; the row
    // then reads in project, off the program.
    let mut row = false;
    for _ in 0..50 {
        settle(&mut h);
        if h.query_all_by_label_contains("Stock  Profile of 6 points  · in project").next().is_some() {
            row = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(row, "the project reloaded and the row reads in project; the log: {}", h.state().output_lines().join(" | "));
    drop(h);
    let _ = std::fs::remove_dir_all(&root);
}

/// The import door driven on a folder that does not exist yet (ROADMAP
/// item 1): the Outline tab, New target…, the fixture's path typed, Import,
/// then the wait for the stock row. Hands back the harness with the Review
/// in front and the stock row selected, the root to remove and the project
/// folder; None without the sidecar's venv.
fn import_into_fresh_folder(project_name: &str, fixture: &str, stock_row: &str) -> Option<(Harness<'static, ViewApp>, std::path::PathBuf, std::path::PathBuf)> {
    #[cfg(windows)]
    const VENV_PYTHON: &str = "sidecar/.venv/Scripts/python.exe";
    #[cfg(not(windows))]
    const VENV_PYTHON: &str = "sidecar/.venv/bin/python";
    if !std::path::Path::new(VENV_PYTHON).is_file() {
        eprintln!("skipped: no sidecar venv at {VENV_PYTHON}");
        return None;
    }
    redirect_config_home();
    let test = std::thread::current().name().unwrap_or("t").replace("::", "-");
    let root = std::env::temp_dir().join(format!("nacs-shell-import-{}-{}", std::process::id(), test));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join(project_name);
    let path = project.to_string_lossy().into_owned();
    let part = view::empty_project_part(&project);
    let cr = nacs::check_result::CheckResult { refusal: Some("no main.part yet".to_string()), findings: Vec::new(), emitted_moves: None };
    let mut h = Harness::builder()
        .with_size(egui::Vec2::new(1200.0, 800.0))
        .with_max_steps(64)
        .build_eframe(move |_cc| frontend::harness_app(&part, cr, &path, None, Workspace::default()));
    settle(&mut h);
    let step = std::fs::canonicalize(fixture).unwrap();
    let step = step.to_string_lossy().trim_start_matches("\\\\?\\").to_string();
    // A fresh folder opens on Problems (no main.part yet); the Outline's
    // Target header carries the door. The menu's Import STEP… is the same act.
    h.get_by_label("Outline").click();
    settle(&mut h);
    // The door: New target… opens the path field; the path typed; Import.
    h.get_by_label("New target…").click();
    settle(&mut h);
    let fields = h.query_all_by_role(Role::TextInput).count();
    assert!(fields >= 1, "the path field is on screen");
    let field = h.query_all_by_role(Role::TextInput).min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the path field");
    field.focus();
    h.step();
    let field = h.query_all_by_role(Role::TextInput).min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the path field");
    field.type_text(&step);
    h.step();
    h.get_by_label("Import").click();
    // CadQuery takes seconds to wake: wait the import out, up to two minutes.
    let mut done = false;
    for _ in 0..1200 {
        settle(&mut h);
        if h.query_all_by_label(stock_row).next().is_some() {
            done = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(done, "the import ended with the stock proposed; note: {:?}", h.state().output_lines());
    assert!(project.join("report.json").is_file() && project.join("proposed_stock.part").is_file(), "the sidecar's two files beside the project");
    assert!(!project.join("main.part").exists(), "nothing written before Accept");
    assert_eq!(h.state().layout().sidebar.tab, nacs::theme::SidebarTab::Review, "the Review in front");
    assert!(matches!(h.state().selection(), Selection::ProposedStock), "the stock row selected first");
    Some((h, root, project))
}


/// The front door, driven end to end (ROADMAP item 1, slice 1): an empty
/// folder, New target…, the STEP's path typed into the field, Import —
/// the sidecar proposes the stock and the features, the Review lists the
/// stock first, Accept makes main.part from it, Accept all adds the
/// features. Every write is undoable; nothing lands without a click.
#[test]
fn an_import_into_an_empty_folder_is_reviewed_into_a_project() {
    let Some((mut h, root, project)) = import_into_fresh_folder("lplate.cam", "tests/fixtures/l_plate.step", "Stock  Profile of 6 points") else { return };
    // The features are listed too, every face once.
    assert!(h.query_by_label_contains("F1  plane").is_some(), "the outer profile's wall");
    assert!(h.query_by_label_contains("F8  plane").is_some(), "the top");
    h.get_by_label("Accept into project").click();
    settle(&mut h);
    let main = project.join("main.part");
    let text = std::fs::read_to_string(&main).expect("main.part made from the stock");
    assert!(text.contains("stock Profile(h=20.0, z=20.0"), "{text}");
    assert!(!text.contains("feature "), "no feature yet: {text}");
    // The reload, then every proposal in one write.
    let mut ready = false;
    for _ in 0..50 {
        settle(&mut h);
        if h.query_by_label_contains("Accept all (").is_some() && h.query_all_by_label_contains("· in project").next().is_some() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(ready, "the project reloaded with the stock in it: {:?}", h.state().output_lines());
    h.query_by_label_contains("Accept all (").expect("the button").click();
    settle(&mut h);
    let text = std::fs::read_to_string(&main).unwrap();
    assert!(text.contains("feature F1_Profile ProfileBoss(z=20.0, extent=20.0"), "{text}");
    assert!(text.contains("feature F8_Top Plane(z=20.0)"), "{text}");
    assert!(nacs::parse::load_project(&project.to_string_lossy()).is_ok(), "the project parses");
    drop(h);
    let _ = std::fs::remove_dir_all(&root);
}

/// Phase 57 slice 1 (forks 0A, 1A): under the window's own frame the band's
/// right end carries the caption buttons, and each sends the platform its
/// command through the frame — `Close`, `Maximized`, `Minimized` — which the
/// harness reads off the frame's output; the Window menu's switch sends
/// `Decorations(true)`, remembers `frame native`, and takes the caption
/// buttons off the band, because the system's frame has its own.
#[test]
fn the_caption_buttons_send_the_windows_commands_and_the_switch_puts_the_system_frame_back() {
    let mut h = open();
    settle(&mut h);
    assert!(!h.state().layout().frame_native, "the window's own frame by default");
    let sent = |h: &Harness<'static, ViewApp>| h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|o| o.commands.clone()).unwrap_or_default();
    h.get_by_label("🗖").click();
    h.step();
    h.step();
    assert!(sent(&h).contains(&egui::ViewportCommand::Maximized(true)), "maximize sent: {:?}", sent(&h));
    h.get_by_label("🗕").click();
    h.step();
    h.step();
    assert!(sent(&h).contains(&egui::ViewportCommand::Minimized(true)), "minimize sent: {:?}", sent(&h));
    h.get_by_label("🗙").click();
    h.step();
    h.step();
    assert!(sent(&h).contains(&egui::ViewportCommand::Close), "close sent: {:?}", sent(&h));
    // The switch, from the corner menu's Window group.
    h.get_by_label(view::MENU_GLYPH).click();
    settle(&mut h);
    submenu(&h, "Window").expect("the Window submenu").click();
    settle(&mut h);
    h.get_by_label("Switch window frame").click();
    h.step();
    h.step();
    assert!(sent(&h).contains(&egui::ViewportCommand::Decorations(true)), "the system's frame asked for: {:?}", sent(&h));
    settle(&mut h);
    assert!(h.state().layout().frame_native, "remembered");
    assert!(h.query_by_label("🗙").is_none() && h.query_by_label("🗖").is_none(), "the caption buttons leave with the frame");
}

/// Phase 57 slice 2 (fork 2A): under the window's own frame the edges are
/// the shell's — hovering the right edge shows the east resize cursor,
/// dragging from it sends `BeginResize(East)`; a corner sends its own;
/// inside the margin nothing is sent and the cursor is the default.
#[test]
fn the_edges_resize_the_window_under_its_own_frame() {
    let mut h = open();
    settle(&mut h);
    let sent = |h: &Harness<'static, ViewApp>| h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|o| o.commands.clone()).unwrap_or_default();
    let east = egui::pos2(1200.0 - 2.0, 400.0);
    h.hover_at(east);
    h.step();
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeEast, "the cursor says the edge");
    // The command goes out on the frame the press lands, and the harness
    // reports the last frame's output: read after each step.
    h.drag_at(east);
    h.step();
    let mut seen = sent(&h);
    h.hover_at(egui::pos2(1200.0 - 30.0, 400.0));
    h.step();
    seen.extend(sent(&h));
    assert!(seen.contains(&egui::ViewportCommand::BeginResize(egui::ResizeDirection::East)), "east sent: {seen:?}");
    h.drop_at(egui::pos2(1200.0 - 30.0, 400.0));
    settle(&mut h);
    let corner = egui::pos2(1200.0 - 2.0, 800.0 - 2.0);
    h.hover_at(corner);
    h.step();
    assert_eq!(h.output().platform_output.cursor_icon, egui::CursorIcon::ResizeSouthEast);
    h.drag_at(corner);
    h.step();
    let mut seen = sent(&h);
    h.hover_at(egui::pos2(1200.0 - 40.0, 800.0 - 40.0));
    h.step();
    seen.extend(sent(&h));
    assert!(seen.contains(&egui::ViewportCommand::BeginResize(egui::ResizeDirection::SouthEast)), "south-east sent: {seen:?}");
    h.drop_at(egui::pos2(1200.0 - 40.0, 800.0 - 40.0));
    settle(&mut h);
    h.hover_at(egui::pos2(600.0, 400.0));
    h.step();
    // Inside, the cursor is not an edge's. Not "Default": the point is in
    // the viewport, and a wire within pick tolerance of it earns the
    // pointing hand (Phase 50 slice 9) — which is where it stood once
    // Phase 59 slice 2's solid bars moved the picture by a bar's width.
    let inside = h.output().platform_output.cursor_icon;
    assert!(!matches!(inside, egui::CursorIcon::ResizeEast | egui::CursorIcon::ResizeWest | egui::CursorIcon::ResizeNorth | egui::CursorIcon::ResizeSouth | egui::CursorIcon::ResizeNorthEast | egui::CursorIcon::ResizeNorthWest | egui::CursorIcon::ResizeSouthEast | egui::CursorIcon::ResizeSouthWest), "inside: not an edge's cursor, got {inside:?}");
}

// ---- Phase 59: the window's loose ends from the Phase 58 walk -------------

/// Phase 62 slice 3 (fork 7A): the card shows the fifth row, `held`, and
/// its pin is the one write path's list of writes (`view::pin_writes`):
/// the word written and the stepover DROPPED — the ceiling derives it —
/// so the line the pin leaves is a line the parser takes, not a refusal
/// ("two governors"). The file is rewritten and the window follows; z
/// undoes to the fixture's bytes.
#[test]
fn the_card_shows_held_and_its_pin_drops_the_stepover() {
    let (mut h, project) = open_copy("plate.cam");
    settle(&mut h);
    row(&h, "[1] Clear").click();
    // The table is measured on a worker; settle waits it out.
    settle(&mut h);
    assert!(matches!(h.state().selection(), Selection::Operation(1)));
    // The table stands below the dock (Phase 59 slice 1): scroll its row
    // into the window before clicking in it.
    let title = "Patterns — measured, never scored";
    h.get_by_label(title).scroll_to_me();
    settle(&mut h);
    // …and the dock on down, a step at a time, until the row is inside it.
    let dock_bottom = |h: &Harness<'static, ViewApp>| h.query_all_by_role(Role::ScrollBar).map(|n| n.rect()).filter(|r| r.right() < 400.0 && r.width() < 20.0 && r.height() > 100.0).map(|r| r.bottom()).fold(0.0f64, |a, b| a.max(b as f64)) as f32;
    for _ in 0..8 {
        let held = h.query_all_by_label("held").next().expect("the fifth row, by its word").rect();
        if held.bottom() < dock_bottom(&h) {
            break;
        }
        h.get_by_label(title).scroll_down();
        settle(&mut h);
    }
    let held = h.query_all_by_label("held").next().expect("the fifth row, by its word").rect();
    assert!(held.bottom() < dock_bottom(&h), "the row is inside the dock: {held:?} under {}", dock_bottom(&h));
    // The held row's own pin: the `pin` button on its line.
    let pin = h
        .query_all_by_label("pin")
        .filter(|n| (n.rect().center().y - held.center().y).abs() < 4.0)
        .next()
        .expect("a pin on the held row");
    pin.click();
    std::thread::sleep(std::time::Duration::from_millis(700));
    settle(&mut h);
    settle(&mut h);
    let main = std::fs::read_to_string(project.join("main.part")).unwrap();
    assert!(main.contains("run Clear(tool=T5, profile=Notch, pattern=held)"), "the word written and the stepover dropped: {main}");
    assert!(!main.contains("two governors"), "{main}");
    assert!(h.query_all_by_label_contains("[1] Clear").next().is_some(), "the window followed the file");
    // Undo: the fixture's bytes.
    h.get_by_label("Undo").click();
    std::thread::sleep(std::time::Duration::from_millis(700));
    settle(&mut h);
    settle(&mut h);
    assert_eq!(std::fs::read_to_string(project.join("main.part")).unwrap(), std::fs::read_to_string("plate.cam/main.part").unwrap(), "undone to the fixture's bytes");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// Phase 59 slice 1 (fork 0A): the patterns table's bar stays with the
/// table. With the Clear's card on the form at its top the table stands
/// below the dock's bottom, and NO horizontal bar stands inside the dock
/// — egui used to relocate the table's bar up to the dock's edge, "there
/// when you can't see the patterns" (the Phase 58 walk); with the card
/// scrolled to the table, its bar stands inside the dock: scoped, not
/// lost.
#[test]
fn the_patterns_tables_bar_stays_with_the_table() {
    let mut h = open();
    settle(&mut h);
    row(&h, "[1] Clear").click();
    // The table is measured on a worker; settle waits it out.
    settle(&mut h);
    assert!(matches!(h.state().selection(), Selection::Operation(1)));
    let title = "Patterns — measured, never scored";
    // With the keys the line does not carry folded away the table is in
    // view at once; unfolded, the card is long enough to push it below
    // the dock, which is the case this test is about.
    assert!(h.get_by_label(title).rect().top() < 796.0, "folded, the table is in view without a scroll");
    let folds = h.query_all_by_label_contains("More keys").count();
    assert!(folds >= 1, "the card folds the keys the line does not carry");
    for k in 0..folds {
        h.query_all_by_label_contains("More keys").nth(k).expect("a fold").click();
        settle(&mut h);
    }
    let bars = |h: &Harness<'static, ViewApp>| -> Vec<egui::Rect> { h.query_all_by_role(Role::ScrollBar).map(|n| n.rect()).collect() };
    // The dock's extent is its own vertical bar's: the LOWEST thin one in
    // the left column (x 304..314, y 462..796 as the DRAFT measured it;
    // the sidebar's stands above it, taller). Thin by width, not by
    // aspect: the table's relocated bar carries a hit rect from the
    // dock's edge to the table's bottom, taller than wide (x 16..314,
    // y 786..1536), and is exactly the stray one.
    let form_bar = |h: &Harness<'static, ViewApp>| bars(h).into_iter().filter(|r| r.right() < 400.0 && r.width() < 20.0 && r.height() > 100.0).max_by(|a, b| a.bottom().total_cmp(&b.bottom())).expect("the form's vertical bar");
    let dock = form_bar(&h);
    let title_rect = h.get_by_label(title).rect();
    assert!(title_rect.top() > dock.bottom(), "the table is below the dock: its title's top {} under the dock's bottom {}", title_rect.top(), dock.bottom());
    // A horizontal bar in the dock: wider than a thin vertical bar, in
    // the dock's column, and overlapping its rows.
    let horizontal_inside = |dock: egui::Rect| move |r: &egui::Rect| r.width() > 20.0 && r.right() <= dock.right() + 0.5 && r.bottom() > dock.top() && r.top() < dock.bottom();
    let stray: Vec<egui::Rect> = bars(&h).into_iter().filter(horizontal_inside(dock)).collect();
    assert!(stray.is_empty(), "no horizontal bar inside the dock (y {}..{}) while the table is below it: {stray:?}", dock.top(), dock.bottom());
    // The card scrolled to the table: its bar is inside the dock.
    h.get_by_label(title).scroll_to_me();
    settle(&mut h);
    let dock = form_bar(&h);
    let title_rect = h.get_by_label(title).rect();
    assert!(title_rect.top() < dock.bottom(), "the title is in view after the scroll: top {} vs the dock's bottom {}", title_rect.top(), dock.bottom());
    let scoped: Vec<egui::Rect> = bars(&h).into_iter().filter(horizontal_inside(dock)).collect();
    assert_eq!(scoped.len(), 1, "the table's bar inside the dock (y {}..{}): {scoped:?}", dock.top(), dock.bottom());
}

/// Phase 59 slice 2 (fork 1A): one scroll style for the window. The
/// Program's bars were solid and six pixels (Phase 52 slice 15) and every
/// other pane's floating and ten; now every `ScrollBar` node's thinner
/// side is the one style's `bar_width`, read from the style's own number
/// as the pane tests read it — on the opening stance with Output open —
/// and the style the window installs is solid with its handle on the
/// foreground slot, the theme's `scrollbar` colour.
#[test]
fn every_scroll_bar_is_the_one_solid_style() {
    let mut h = open();
    settle(&mut h);
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    let want = frontend::scroll_style().bar_width;
    assert!((want - egui::style::ScrollStyle::solid().bar_width).abs() < 1e-6, "the one style is egui's solid");
    let bars: Vec<egui::Rect> = h.query_all_by_role(Role::ScrollBar).map(|n| n.rect()).collect();
    assert!(bars.len() >= 3, "the sidebar's, the form's and the Program's at least: {bars:?}");
    for r in &bars {
        let thick = r.width().min(r.height());
        assert!((thick - want).abs() < 0.5, "a bar {thick} px thick where the one style is {want}: {r:?} among {bars:?}");
    }
    let style = h.ctx.global_style();
    assert!(!style.spacing.scroll.floating, "solid, not floating");
    assert!(style.spacing.scroll.foreground_color, "the handle on the foreground slot — the theme's scrollbar colour");
}

/// Phase 59 slice 4 (forks 3A + 4A): `sh echo hello` typed on the line
/// runs the system's shell with the line whole; the log echoes it, names
/// the shell under the echo, carries the shell's answer and says the exit
/// last.
#[test]
fn sh_hands_a_line_to_the_shell_and_the_log_says_which() {
    let mut h = open();
    settle(&mut h);
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    let field = h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line");
    field.focus();
    h.step();
    h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().type_text("sh echo hello");
    h.step();
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    settle(&mut h);
    // A line as the log prints it is "m:ss  text", a run's own indented two
    // more; the text, trimmed, is what is read.
    let lines: Vec<String> = h.state().output_lines().iter().map(|l| l.split_once("  ").map(|(_, s)| s.trim().to_string()).unwrap_or_default()).collect();
    let log = lines.join(" | ");
    let at = |needle: &str| lines.iter().position(|l| l == needle).unwrap_or_else(|| panic!("{needle:?} in the log: {log}"));
    let via = lines.iter().position(|l| l.starts_with("via ")).unwrap_or_else(|| panic!("the shell named: {log}"));
    let (echo, hello, exit) = (at("$ sh echo hello"), at("hello"), at("exit 0"));
    assert!(echo < via && via < hello && hello < exit, "the echo, the shell, its answer, the exit — in that order: {log}");
    let (system, flag) = view::shell_program(None);
    assert_eq!(lines[via], format!("via {} {flag}", system.display()));
}

/// Phase 59 slice 5 (fork 5A): Stop. A run that would hold the line for
/// a minute (`sh ping -n 60 127.0.0.1`; `sleep 60` off Windows) is killed
/// from the header's *Stop*, its exit line "killed", *Stop* disabled
/// again, and the line free for the next run.
#[test]
fn stop_kills_the_run_and_the_line_is_free() {
    let mut h = open();
    settle(&mut h);
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    fn line<'t>(h: &'t Harness<'static, ViewApp>) -> egui_kittest::Node<'t> {
        h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line")
    }
    line(&h).focus();
    h.step();
    line(&h).type_text(if cfg!(windows) { "sh ping -n 60 127.0.0.1" } else { "sh sleep 60" });
    h.step();
    h.key_press(egui::Key::Enter);
    // The run stands: stepped, not settled — a settle would wait a minute.
    for _ in 0..10 {
        h.step();
    }
    let texts = |h: &Harness<'static, ViewApp>| -> Vec<String> { h.state().output_lines().iter().map(|l| l.split_once("  ").map(|(_, s)| s.trim().to_string()).unwrap_or_default()).collect() };
    let before = texts(&h);
    assert!(before.iter().any(|l| l.starts_with("$ sh ")), "the run echoed: {before:?}");
    assert!(!before.iter().any(|l| l == "killed" || l.starts_with("exit ")), "the run is still going: {before:?}");
    assert!(!h.get_by_label("Stop").accesskit_node().is_disabled(), "Stop is enabled while a run stands");
    h.get_by_label("Stop").click();
    settle(&mut h);
    settle(&mut h);
    let after = texts(&h);
    assert_eq!(after.last().map(String::as_str), Some("killed"), "the exit line: {after:?}");
    assert!(h.get_by_label("Stop").accesskit_node().is_disabled(), "nothing to stop");
    line(&h).focus();
    h.step();
    line(&h).type_text("sh echo again");
    h.step();
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    settle(&mut h);
    let again = texts(&h);
    let killed = again.iter().position(|l| l == "killed").expect("the kill stays in the log");
    assert!(again[killed..].iter().any(|l| l == "again") && again.last().map(String::as_str) == Some("exit 0"), "the line is free: {again:?}");
}

/// Phase 59 nudge 1 (§5.3, the user's walk of 2026-10-23: "the
/// Outline/Problems/Review pane has minimum width but the Program pane
/// does not"): the Program keeps a wall's floor and the split its RATIO
/// when the center is squeezed — the window narrowed, then the sidebar
/// dragged as wide as it will go. Before: the viewport kept its pixels,
/// the Program went to nothing, and the saved ratio was rewritten by a
/// drag that never touched the split.
#[test]
fn a_wall_stops_at_the_documents_floors_and_the_split_keeps_its_ratio() {
    let floor = nacs::theme::DOCK_MIN_SIZE;
    let mut layout = Layout::default();
    layout.apply(DockAction::ShowSplit);
    layout.apply(DockAction::ResizeSidebar(300.0));
    let mut h = open_sized(Workspace { layout, ..Workspace::default() }, egui::Vec2::new(1200.0, 800.0));
    settle(&mut h);
    settle(&mut h);
    let ratio = h.state().layout().split;
    let (pane, _) = h.state().program_rects().expect("the Program is on screen");
    let wide = pane.width();
    // The window narrowed by nearly half: the Program gives up width in
    // its share, keeps its floor, and the saved ratio is untouched.
    h.set_size(egui::Vec2::new(700.0, 800.0));
    settle(&mut h);
    settle(&mut h);
    let (pane, _) = h.state().program_rects().expect("the Program is still on screen");
    assert!(pane.width() >= floor - 8.0, "the Program keeps a wall's floor: {} vs {floor}", pane.width());
    assert!(pane.width() < wide - 100.0, "and gave up width in its share: {} vs {wide}", pane.width());
    assert!((h.state().layout().split - ratio).abs() < 0.01, "the saved ratio is untouched: {} vs {ratio}", h.state().layout().split);
    // The sidebar's handle, found by the cursor at its right edge.
    let before = h.state().layout().sidebar.width;
    let mut handle = None;
    for dx in -8..=8 {
        let p = egui::pos2(before + dx as f32, 400.0);
        h.hover_at(p);
        h.step();
        if h.output().platform_output.cursor_icon == egui::CursorIcon::ResizeHorizontal {
            handle = Some(p);
            break;
        }
    }
    let handle = handle.expect("the sidebar's edge shows the resize cursor");
    // Dragged to the window's right edge: the wall widens and stops where
    // the two documents' floors begin.
    h.drag_at(handle);
    h.step();
    for x in [500.0, 600.0, 690.0] {
        h.hover_at(egui::pos2(x, 400.0));
        h.step();
    }
    h.drop_at(egui::pos2(690.0, 400.0));
    h.step();
    settle(&mut h);
    settle(&mut h);
    let after = h.state().layout().sidebar.width;
    assert!(after > before + 100.0, "the wall widened: {after} vs {before}");
    assert!(after <= 700.0 - 2.0 * floor + 1.0, "and stopped at the documents' floors: {after}");
    let (pane, _) = h.state().program_rects().expect("the Program is still on screen");
    assert!(pane.width() >= floor - 8.0, "the Program keeps its floor under the wall: {} vs {floor}", pane.width());
    assert!((h.state().layout().split - ratio).abs() < 0.01, "the saved ratio is still untouched: {} vs {ratio}", h.state().layout().split);
}

/// Phase 59 nudge 2: ↑ and ↓ on Output's line recall what was entered —
/// two lines run, a third begun; ↑ shows the second, ↑ the first, ↑ again
/// still the first; ↓ the second, ↓ the third as it was being typed —
/// and the line keeps the focus throughout (the arrows do not walk it
/// to another widget).
#[test]
fn the_output_line_recalls_earlier_lines_with_the_arrows() {
    let mut h = open();
    settle(&mut h);
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    fn line<'t>(h: &'t Harness<'static, ViewApp>) -> egui_kittest::Node<'t> {
        h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the runner's line")
    }
    for text in ["sh echo one", "sh echo two"] {
        line(&h).focus();
        h.step();
        line(&h).type_text(text);
        h.step();
        h.key_press(egui::Key::Enter);
        settle(&mut h);
        settle(&mut h);
    }
    line(&h).focus();
    h.step();
    line(&h).type_text("sh e");
    h.step();
    assert_eq!(h.state().runner_line(), "sh e");
    let press = |h: &mut Harness<'static, ViewApp>, key: egui::Key, expect: &str| {
        h.key_press(key);
        h.step();
        h.step();
        assert_eq!(h.state().runner_line(), expect, "after {key:?}");
        assert!(h.ctx.memory(|m| m.focused().is_some()), "the line keeps the focus after {key:?}");
    };
    press(&mut h, egui::Key::ArrowUp, "sh echo two");
    press(&mut h, egui::Key::ArrowUp, "sh echo one");
    press(&mut h, egui::Key::ArrowUp, "sh echo one");
    press(&mut h, egui::Key::ArrowDown, "sh echo two");
    press(&mut h, egui::Key::ArrowDown, "sh e");
    // The recalled line runs as any typed line does.
    press(&mut h, egui::Key::ArrowUp, "sh echo two");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    settle(&mut h);
    let lines: Vec<String> = h.state().output_lines().iter().map(|l| l.split_once("  ").map(|(_, s)| s.trim().to_string()).unwrap_or_default()).collect();
    assert_eq!(lines.iter().filter(|l| *l == "$ sh echo two").count(), 2, "the second line ran twice: {}", lines.join(" | "));
}

/// Phase 59 nudge 3 (§5.2, the user's walk of 2026-10-23: the Outline
/// pane "can go further than some minimum but it bounces right back …
/// the contents continue to shrink to the floor value, but the pane
/// doesn't"): the sidebar dragged narrow follows the pointer down to the
/// wall's floor and stays there on release — nothing in it holds the
/// pane open, and the pane's floor is the model's, not egui's. Before:
/// the Operations rows, the tab strip and the headers' doors each held
/// the pane at their own width (341, then 227 px), and a wall under 120
/// sprang to 120 on release.
#[test]
fn a_wall_dragged_narrow_stops_at_its_floor_and_does_not_spring_back() {
    let floor = nacs::theme::DOCK_MIN_SIZE;
    let mut h = open();
    settle(&mut h);
    settle(&mut h);
    let before = h.state().layout().sidebar.width;
    assert!(before > floor + 100.0, "the sidebar opens wide: {before}");
    let mut handle = None;
    for dx in -8..=8 {
        let p = egui::pos2(before + dx as f32, 500.0);
        h.hover_at(p);
        h.step();
        if h.output().platform_output.cursor_icon == egui::CursorIcon::ResizeHorizontal {
            handle = Some(p);
            break;
        }
    }
    let handle = handle.expect("the sidebar's edge shows the resize cursor");
    h.drag_at(handle);
    h.step();
    // Each step of the drag is followed, down to the floor.
    for x in [before - 60.0, before - 120.0, 200.0, 160.0] {
        h.hover_at(egui::pos2(x, 500.0));
        h.step();
        h.step();
        let w = h.state().layout().sidebar.width;
        assert!((w - x).abs() < 12.0, "the wall follows the pointer at {x}: {w}");
    }
    // Under the floor the wall stops at the floor — and stays there when
    // the pointer lets go, with the viewport's row starting where it ends.
    h.hover_at(egui::pos2(60.0, 500.0));
    h.step();
    h.step();
    let w = h.state().layout().sidebar.width;
    assert!((w - floor).abs() < 1.0, "the wall stops at the floor during the drag: {w} vs {floor}");
    h.drop_at(egui::pos2(60.0, 500.0));
    h.step();
    settle(&mut h);
    let w = h.state().layout().sidebar.width;
    assert!((w - floor).abs() < 1.0, "and stays there on release: {w} vs {floor}");
    let layers = h.get_by_label("Layers").rect().left();
    assert!(layers > floor && layers < floor + 24.0, "the viewport's row starts at the wall's edge: {layers} vs {floor}");
    // The tab strip is clipped at the pane's edge, not laid past it: the
    // first tab stands and the close is inside the pane.
    assert!(h.get_by_label("Outline").rect().left() < floor);
    let close = h.query_all_by_label("×").map(|n| n.rect()).min_by(|a, b| a.top().total_cmp(&b.top())).expect("a close");
    assert!(close.right() <= floor + 1.0, "the close stands inside the pane: {}", close.right());
}

/// Phase 59 nudge 4 — the user's walk, clarified (2026-10-24): "when I
/// drag the handle, the rate at which the values appear in the shell
/// feels excessive." Every frame of a grip drag the draft stood in the
/// form's drafts, the status line said `Unsent — z = …`, and the Output
/// log recorded each distinct line — six times as often since slice 3
/// took vsync off. Now a draft under a HELD grip is not an unsent note:
/// the log is quiet for the whole drag and says `Written` once on release.
/// Red on the old shell at the first assertion inside the drag.
#[test]
fn a_grip_drag_posts_nothing_to_the_log_until_it_is_released() {
    let (mut h, project) = open_copy("plate.cam");
    settle(&mut h);
    row(&h, "TopFace  Plane").click();
    settle(&mut h);
    let grip = h.state().grips_drawn().iter().find(|(k, _)| k == "z").map(|(_, p)| *p).expect("the Plane's z grip is drawn");
    let before = h.state().output_lines().len();
    h.hover_at(grip);
    h.step();
    h.drag_at(grip);
    h.step();
    // Thirty frames of the hand moving up the screen, a few pixels each.
    for k in 1..=30 {
        h.hover_at(egui::pos2(grip.x, grip.y - 3.0 * k as f32));
        h.step();
        assert_eq!(h.state().output_lines().len(), before, "frame {k}: the log is quiet while the grip is held");
        assert!(h.query_all_by_label_contains("Unsent — z").next().is_none(), "frame {k}: a held grip's draft is not unsent");
    }
    h.drop_at(egui::pos2(grip.x, grip.y - 90.0));
    h.step();
    settle(&mut h);
    let lines = h.state().output_lines();
    let added: Vec<&String> = lines.iter().skip(before).collect();
    assert!(added.iter().any(|l| l.contains("Written") && l.contains("z = ")), "the release writes once and the log says so: {added:?}");
    assert!(added.len() <= 3, "a drag is one line in the log, not a frame's worth: {added:?}");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// Phase 61 slice 3 (fork 7A): *Clear around it…* on a boss that stands in
/// the blank with nothing around it proposes the floor whose wall is the
/// stock — a Plane at the boss's base — in the feature form, written only on
/// Create. Since ROADMAP item 3 slice 1 the form then CLOSES and the Plane
/// is selected, its card editable with *New operation…* on it (until then
/// the operation form reopened on it, Clear then End, and only Esc closed
/// it). On a boss that already stands in a host the status line says why
/// and nothing opens. Both on COPIES.
#[test]
fn clear_around_it_proposes_the_plane_and_selects_it() {
    let (mut h, project) = open_copy_pixels("cylinder_boss.cam");
    settle(&mut h);
    let before = std::fs::read_to_string(project.join("main.part")).unwrap();
    row(&h, "Post  CylinderBoss").click_secondary();
    settle(&mut h);
    h.get_by_label("Clear around it…").click();
    settle(&mut h);
    assert!(h.query_all_by_label("New feature").next().is_some(), "the proposal opens as the feature form");
    assert_eq!(std::fs::read_to_string(project.join("main.part")).unwrap(), before, "nothing written until Create");
    h.get_by_label("Create feature").click();
    std::thread::sleep(std::time::Duration::from_millis(700));
    settle(&mut h);
    settle(&mut h);
    let main = std::fs::read_to_string(project.join("main.part")).unwrap();
    assert!(main.contains("feature Floor1 Plane(z=-12"), "the Plane at the boss's base landed, named a floor: {main}");
    assert!(h.query_all_by_label("New feature").next().is_none(), "the form closed on Create");
    assert!(h.query_all_by_label_contains("New operation on Floor1").next().is_none(), "and no operation form opened in its place");
    let floor = h.state().feature_names().iter().position(|n| n == "Floor1").expect("the Plane is an edge now");
    assert_eq!(h.state().selection(), Selection::Feature(floor), "the Plane just made is selected");
    shot(&mut h, "clear-around-plane-landed");
    assert!(h.query_all_by_label_contains("Floor1  Plane").next().is_some(), "the card names it");
    assert!(h.query_all_by_label("New operation…").any(|n| !n.accesskit_node().is_disabled()), "its card offers the follow-on operation");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
    // A boss with a host is refused by name and nothing opens.
    let (mut h, project) = open_copy("plate_floated.cam");
    settle(&mut h);
    row(&h, "Plate  ProfileBoss").click_secondary();
    settle(&mut h);
    h.get_by_label("Clear around it…").click();
    settle(&mut h);
    assert!(h.query_all_by_label_contains("already stands in 'Floor'").next().is_some(), "refused by name on the status line");
    assert!(h.query_all_by_label("New feature").next().is_none(), "no form opened");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// Phase 61 slice 3 (the user, 2026-10-25: "new operations are to land after
/// the cursor"): with the op cursor after operation 1, *Create operation*
/// writes the new `run` line straight below the first run line, not at the
/// end of the file, and the cursor moves onto it. On a COPY of plate.cam.
/// ROADMAP item 3 slice 5 drove it and shot it (`new-op-under-cursor.png`):
/// the debt of intake note #611 is paid by this.
#[test]
fn a_new_operation_lands_after_the_cursor_and_the_cursor_follows_it() {
    let (mut h, project) = open_copy_pixels("plate.cam");
    settle(&mut h);
    // A click on a feed move of op 1 seeks the cursor to after it.
    h.query_all_by_label_contains("G01").next().expect("a feed move of op 0 is on screen").click();
    settle(&mut h);
    assert_eq!(h.state().op_cursor(), 1);
    row(&h, "Notch  ProfilePocket").click_secondary();
    settle(&mut h);
    h.query_all_by_label("New operation…").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).expect("the menu's New operation…").click();
    settle(&mut h);
    h.get_by_label("Create operation").click();
    std::thread::sleep(std::time::Duration::from_millis(700));
    settle(&mut h);
    settle(&mut h);
    let main = std::fs::read_to_string(project.join("main.part")).unwrap();
    let runs: Vec<&str> = main.lines().filter(|l| l.starts_with("run ")).collect();
    assert!(runs.len() >= 4, "plate.cam runs three; one landed: {runs:?}");
    assert!(runs[0].starts_with("run End(") && runs[1].starts_with("run Clear(") && runs[2].starts_with("run Clear("), "the new Clear is second, below the End: {runs:?}");
    assert_eq!(h.state().op_cursor(), 2, "the cursor stands on the new operation");
    // ROADMAP item 3 slice 1: the form closed and the new operation is
    // selected, its fields on the card.
    assert!(h.query_all_by_label("Create operation").next().is_none(), "the form closed on Create");
    assert_eq!(h.state().selection(), Selection::Operation(1), "the operation just made is selected");
    assert_eq!(rows_starting(&h, "[1] Clear"), 2, "the new Clear is row 1 in both lists");
    shot(&mut h, "new-op-under-cursor");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// A model that is not a prism gets the smallest blank it fits (ROADMAP
/// item 1, slice 2) plus the window's 1 mm allowance on the sides and the
/// top (slice 11): the stepped disc proposes its bounding cylinder, the
/// row is named by its size, and Accept writes the one line — with the
/// sidecar's comment saying what the allowance is.
#[test]
fn an_import_of_a_round_part_proposes_the_bounding_cylinder() {
    let Some((mut h, root, project)) = import_into_fresh_folder("disc.cam", "tests/fixtures/disc_step.step", "Stock  Cylinder ⌀122 × 31") else { return };
    h.get_by_label("Accept into project").click();
    settle(&mut h);
    let text = std::fs::read_to_string(project.join("main.part")).expect("main.part made from the stock");
    assert!(text.contains("stock Cylinder(dia=122.0, h=31.0, x=0.0, y=0.0, z=31.0)"), "{text}");
    assert!(text.contains("# the smallest blank the model fits"), "the sidecar's words ride into the project: {text}");
    let mut ready = false;
    for _ in 0..50 {
        settle(&mut h);
        if h.query_all_by_label_contains("· in project").next().is_some() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(ready, "the project reloaded with the cylinder in it: {:?}", h.state().output_lines());
    assert!(nacs::parse::load_project(&project.to_string_lossy()).is_ok(), "the project parses");
    drop(h);
    let _ = std::fs::remove_dir_all(&root);
}

/// Edit and reject on a review row (ROADMAP item 1, slice 3), on a copy of
/// disc_step.cam with its golden report dropped beside it. Reject writes a
/// `# rejected F7` line to main.part and the row — and its group-mate —
/// say so; the statement on the card is edited in place and Accept writes
/// the edited text, the row then in the project.
#[test]
fn a_review_row_is_rejected_or_edited_before_it_is_accepted() {
    let (mut h, project) = open_copy("disc_step.cam");
    std::fs::copy("tests/fixtures/disc_step.report.golden.json", project.join("report.json")).unwrap();
    // The watch picks the report up; the tab then carries the count.
    let mut seen = false;
    for _ in 0..50 {
        settle(&mut h);
        if let Some(tab) = h.query_all_by_label_contains("Review (").next() {
            tab.click();
            seen = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(seen, "the watch picked the report up and the Review tab counts its proposals");
    settle(&mut h);
    assert!(h.query_all_by_label("F7  cylinder").next().is_some(), "the Review lists the report's rows");
    h.get_by_label("F7  cylinder").click();
    settle(&mut h);
    h.get_by_label("Reject").click();
    settle(&mut h);
    let main = project.join("main.part");
    let text = std::fs::read_to_string(&main).unwrap();
    assert!(text.ends_with("\n# rejected F7\n"), "the rejection is the last line: {text}");
    let mut marked = false;
    for _ in 0..50 {
        settle(&mut h);
        if h.query_all_by_label("F7  cylinder  · rejected").next().is_some() && h.query_all_by_label("F12  plane  — member of F7_Bore  · rejected").next().is_some() {
            marked = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(marked, "the row and its group-mate read as rejected: {:?}", h.state().review_rows().iter().filter(|r| r.rejected).map(|r| r.id.as_str()).collect::<Vec<_>>());
    assert!(h.query_all_by_label("Take the rejection back").next().is_some(), "the card offers the way back");
    // The edit: the card's statement, replaced wholesale, then accepted.
    h.get_by_label("F5  plane").click();
    settle(&mut h);
    let field = h.query_all_by_role(Role::MultilineTextInput).find(|n| n.value().is_some_and(|v| v.contains("F5_Top"))).expect("the statement is a text field on the card");
    field.focus();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    let field = h.query_all_by_role(Role::MultilineTextInput).find(|n| n.value().is_some_and(|v| v.contains("F5_Top"))).expect("the statement field");
    field.type_text("feature F5_Top Plane(z=29.5)");
    settle(&mut h);
    assert!(h.query_all_by_label("edited — Accept writes this text").next().is_some(), "the card says the text is edited");
    h.get_by_label("Accept into project").click();
    settle(&mut h);
    let text = std::fs::read_to_string(&main).unwrap();
    assert!(text.contains("feature F5_Top Plane(z=29.5)"), "the edited statement is what was written: {text}");
    assert!(!text.contains("Plane(z=30.0)"), "{text}");
    let mut in_project = false;
    for _ in 0..50 {
        settle(&mut h);
        if h.query_all_by_label("F5  plane  · in project").next().is_some() {
            in_project = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(in_project, "the row reads as in the project after the reload: {:?}", h.state().output_lines());
    assert!(nacs::parse::load_project(&project.to_string_lossy()).is_ok(), "the project parses");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// The setup in the import field (ROADMAP item 1, slice 4): an assembly
/// refused for want of a solid keeps its path in the field with the
/// sidecar's inventory in the note; the solid typed and the up axis
/// chosen, the import runs on that solid and the Review says which.
#[test]
fn an_assembly_is_imported_once_its_solid_is_named_in_the_field() {
    #[cfg(windows)]
    const VENV_PYTHON: &str = "sidecar/.venv/Scripts/python.exe";
    #[cfg(not(windows))]
    const VENV_PYTHON: &str = "sidecar/.venv/bin/python";
    if !std::path::Path::new(VENV_PYTHON).is_file() {
        eprintln!("skipped: no sidecar venv at {VENV_PYTHON}");
        return;
    }
    redirect_config_home();
    let test = std::thread::current().name().unwrap_or("t").replace("::", "-");
    let root = std::env::temp_dir().join(format!("nacs-shell-assembly-{}-{}", std::process::id(), test));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("asm.cam");
    let path = project.to_string_lossy().into_owned();
    let part = view::empty_project_part(&project);
    let cr = nacs::check_result::CheckResult { refusal: Some("no main.part yet".to_string()), findings: Vec::new(), emitted_moves: None };
    let mut h = Harness::builder()
        .with_size(egui::Vec2::new(1200.0, 800.0))
        .with_max_steps(64)
        .build_eframe(move |_cc| frontend::harness_app(&part, cr, &path, None, Workspace::default()));
    settle(&mut h);
    let step = std::fs::canonicalize("tests/fixtures/assembly_pair.step").unwrap();
    let step = step.to_string_lossy().trim_start_matches("\\\\?\\").to_string();
    h.get_by_label("Outline").click();
    settle(&mut h);
    h.get_by_label("New target…").click();
    settle(&mut h);
    fn path_field<'t>(h: &'t Harness<'static, ViewApp>) -> egui_kittest::Node<'t> {
        h.query_all_by_role(Role::TextInput).max_by(|a, b| a.rect().width().total_cmp(&b.rect().width())).expect("the path field is the widest")
    }
    fn solid_field<'t>(h: &'t Harness<'static, ViewApp>) -> egui_kittest::Node<'t> {
        // The setup row under the path reads solid, allowance, given from
        // the left; the solid field is its first.
        let path_top = path_field(h).rect().min.y;
        h.query_all_by_role(Role::TextInput).filter(|n| n.rect().min.y > path_top + 1.0).min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x)).expect("the solid field leads the setup row")
    }
    path_field(&h).focus();
    h.step();
    path_field(&h).type_text(&step);
    h.step();
    h.get_by_label("Import").click();
    // The refusal: two solids, which one is the human's to say.
    let mut refused = false;
    for _ in 0..1200 {
        settle(&mut h);
        if h.state().edit_note().is_some_and(|n| n.contains("2 solids")) {
            refused = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(refused, "the sidecar's inventory is the note: {:?}", h.state().edit_note());
    assert!(path_field(&h).value().is_some_and(|v| v.ends_with("assembly_pair.step")), "the path stays in the field for the second try");
    assert!(!project.join("report.json").exists(), "nothing written on the refusal");
    // The setup: the solid typed, the up axis picked from the menu.
    solid_field(&h).focus();
    h.step();
    solid_field(&h).type_text("1");
    h.step();
    h.query_all_by_role(Role::ComboBox).next().expect("the up menu").click();
    settle(&mut h);
    h.get_by_label("y+").click();
    settle(&mut h);
    assert_eq!(h.state().import_setup(), &view::ImportSetup { up: Some("y+".into()), solid: "1".into(), ..Default::default() });
    h.get_by_label("Import").click();
    let mut done = false;
    for _ in 0..1200 {
        settle(&mut h);
        if h.query_all_by_label_contains("Stock  ").next().is_some() {
            done = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(done, "the import ended with a stock proposed: {:?}", h.state().edit_note());
    let report = std::fs::read_to_string(project.join("report.json")).unwrap();
    assert!(report.contains("\"solid\": \"S1\"") && report.contains("\"up\": \"y+\""), "the report declares the setup: {report}");
    assert!(h.query_all_by_label_contains("solid S1 of 2").next().is_some(), "the Review says which solid");
    assert!(h.query_all_by_label_contains("--up y+").next().is_some(), "and which way was up");
    drop(h);
    let _ = std::fs::remove_dir_all(&root);
}

/// ROADMAP item 3 slice 0: the agent can see the window. The frame is the
/// window's size and not one colour; selecting an operation changes the
/// picture (the path is drawn over the part); and both frames are on disk
/// as `target/shots/plate-opened.png` and `plate-clear-selected.png`.
#[test]
fn the_agent_can_see_the_window() {
    let mut h = open_pixels(Workspace::default());
    settle(&mut h);
    let before = shot(&mut h, "plate-opened");
    assert_eq!((before.width(), before.height()), (1200, 800), "the frame is the window's size");
    let distinct = before.pixels().collect::<std::collections::HashSet<_>>().len();
    assert!(distinct > 64, "a window is more than {distinct} colours");
    row(&h, "[1] Clear").click();
    settle(&mut h);
    assert_eq!(h.state().selection(), Selection::Operation(1));
    let after = shot(&mut h, "plate-clear-selected");
    let changed = pixels_changed(&before, &after);
    assert!(changed > 1000, "selecting the Clear draws its path: {changed} pixels moved");
}

/// ROADMAP item 3 slice 2: a reload runs on a worker. The project file is
/// rewritten on disk with the Clear's stepover at 0.5 mm (two seconds of
/// planning in a debug build, four levels of the offset family); the
/// window keeps painting meanwhile, its status line says `Reloading —
/// planning the program…`, the Output pane logs it, and the reload lands
/// with the longer program. Until 2026-11-07 the planning ran on the frame
/// and the window read as crashed. On a COPY of plate_floated.cam, with a
/// picture of the window mid-reload.
#[test]
fn a_reload_runs_on_a_worker_and_the_window_says_so_meanwhile() {
    let (mut h, project) = open_copy_pixels("plate_floated.cam");
    settle(&mut h);
    let before = h.state().program_lines();
    let main = project.join("main.part");
    let text = std::fs::read_to_string(&main).unwrap().replace("stepover=4.0, stepdown=5", "stepover=0.5, stepdown=5");
    assert_ne!(text, std::fs::read_to_string(&main).unwrap(), "the fixture's line is as the test expects");
    // Past the watch's mtime resolution, then the write.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&main, text).unwrap();
    let mut seen = false;
    for _ in 0..60 {
        h.step();
        if h.query_all_by_label_contains("Reloading — planning the program…").next().is_some() {
            seen = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(seen, "the status line said the reload was in flight");
    shot(&mut h, "reload-in-flight");
    assert_eq!(h.state().program_lines(), before, "the old program stands until the new one lands");
    settle(&mut h);
    settle(&mut h);
    assert!(h.query_all_by_label_contains("Reloading — planning the program…").next().is_none(), "and stopped once it landed");
    assert!(h.state().program_lines() > before * 2, "the program at 0.5 mm is the longer one: {} vs {before}", h.state().program_lines());
    assert!(h.state().output_lines().iter().any(|l| l.contains("Reloading — planning the program…")), "the Output pane logged it");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// ROADMAP item 3 slice 1: a feature made in the window can be edited at
/// once. *New feature…* opens the form; Create closes it, the feature just
/// made is selected, and its card stands with its fields and *New
/// operation…*. Until 2026-11-07 the form stayed over every selection and
/// only Esc closed it. On a COPY of cylinder_boss.cam (plate.cam declares
/// its features in geometry.part, where the form refuses to write), with
/// a picture.
#[test]
fn a_created_feature_is_selected_with_its_card_editable() {
    let (mut h, project) = open_copy_pixels("cylinder_boss.cam");
    settle(&mut h);
    h.get_by_label("New feature…").click();
    settle(&mut h);
    let head = h.query_all_by_label("New feature").next().expect("the form opened").rect().top();
    // The form opens on a pocket; this one is a Plane, picked from the menu.
    h.query_all_by_role(Role::ComboBox).find(|n| n.rect().top() > head).expect("the variant menu").click();
    settle(&mut h);
    h.get_by_label("Plane").click();
    settle(&mut h);
    // The form's fields, top to bottom: the name, then the Plane's z.
    let mut fields: Vec<_> = h.query_all_by_role(Role::TextInput).filter(|n| n.rect().top() > head).collect();
    fields.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    assert!(fields.len() >= 2, "the name and z fields: {}", fields.len());
    fields[0].focus();
    h.step();
    h.query_all_by_role(Role::TextInput).filter(|n| n.rect().top() > head).min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().type_text("Deck");
    h.step();
    let mut fields: Vec<_> = h.query_all_by_role(Role::TextInput).filter(|n| n.rect().top() > head).collect();
    fields.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    fields[1].focus();
    h.step();
    let mut fields: Vec<_> = h.query_all_by_role(Role::TextInput).filter(|n| n.rect().top() > head).collect();
    fields.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    fields[1].type_text("-5");
    h.step();
    shot(&mut h, "feature-form-filled");
    h.get_by_label("Create feature").click();
    std::thread::sleep(std::time::Duration::from_millis(700));
    settle(&mut h);
    settle(&mut h);
    let main = std::fs::read_to_string(project.join("main.part")).unwrap();
    assert!(main.contains("Deck Plane(z=-5"), "the Plane landed ({:?}): {main}", h.state().edit_note());
    assert!(h.query_all_by_label("New feature").next().is_none(), "the form closed on Create");
    let names = h.state().feature_names();
    let made = names.iter().position(|n| n.ends_with("Deck")).expect("the feature is an edge now");
    assert_eq!(h.state().selection(), Selection::Feature(made), "the feature just made is selected");
    assert!(h.query_all_by_label("New operation…").any(|n| !n.accesskit_node().is_disabled()), "its card offers the follow-on operation");
    assert!(h.query_all_by_label_contains("z (mm) =").next().is_some(), "and its field, editable");
    shot(&mut h, "feature-created-selected");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// A project at `project` (a directory holding `main.part`) opened the way
/// `nacs --view` opens it, on `workspace`, rendered when `pixels`.
fn open_path(project: &std::path::Path, workspace: Workspace, pixels: bool) -> Harness<'static, ViewApp> {
    redirect_config_home();
    let path = project.to_string_lossy().into_owned();
    let part = nacs::parse::load_project(&path).expect("the project parses");
    let cr = nacs::check_result::check_result(&part);
    let p2 = path.clone();
    let b = Harness::builder().with_size(egui::Vec2::new(1200.0, 800.0)).with_max_steps(64);
    let b = if pixels { b.wgpu() } else { b };
    b.build_eframe(move |_cc| frontend::harness_app(&part, cr, &p2, None, workspace))
}

/// The plate under a ⌀2 endmill (ROADMAP item 3 slice 3): the face rowed
/// at 1.5 mm, the notch at 1 mm — the fine-stepover project the crawl was
/// found on. Written fresh under the temp dir, named by the test.
fn fine_plate() -> std::path::PathBuf {
    let test = std::thread::current().name().unwrap_or("t").replace("::", "-");
    let dir = std::env::temp_dir().join(format!("nacs-shell-fine-{}-{}", std::process::id(), test));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("fine_plate.cam");
    std::fs::create_dir_all(&project).unwrap();
    let main = std::fs::read_to_string("plate.cam/main.part")
        .unwrap()
        .replace("tool T5 Endmill(dia=10.0)", "tool T5 Endmill(dia=2.0)")
        .replace("stepover=6.0", "stepover=1.5")
        .replace("stepover=4.0", "stepover=1.0");
    std::fs::write(project.join("main.part"), main).unwrap();
    std::fs::copy("plate.cam/geometry.part", project.join("geometry.part")).unwrap();
    project
}

/// ROADMAP item 3 slice 6: what the first pictures found clipped. At the
/// opening size the Program pane's controls stand whole inside the pane
/// (the row's label gives way first), and a feature card's button row
/// wraps instead of running off the sidebar. With pictures.
#[test]
fn the_program_row_and_the_feature_cards_buttons_fit_at_the_opening_size() {
    let mut h = open_pixels(Workspace::default());
    settle(&mut h);
    let (pane, _) = h.state().program_rects().expect("the Program is on screen");
    for label in ["Write program", "Copy program", "minimap", "numbers"] {
        let r = h.get_by_label(label).rect();
        assert!(r.right() <= pane.right() + 1.0, "{label} is clipped at the pane's right: {} vs {}", r.right(), pane.right());
        assert!(r.left() >= pane.left() - 1.0, "{label} crosses the pane's left edge");
    }
    row(&h, "Notch  ProfilePocket").click();
    settle(&mut h);
    let sidebar_right = h.get_by_label("Layers").rect().left();
    for label in ["Copy template", "New operation…", "Delete feature", "Flip side"] {
        let r = h.query_all_by_label(label).max_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap_or_else(|| panic!("{label} on the card")).rect();
        assert!(r.right() <= sidebar_right + 1.0, "{label} runs off the sidebar: {} vs {sidebar_right}", r.right());
    }
    shot(&mut h, "feature-card-buttons-wrapped");
}

/// ROADMAP item 3 slice 4: the walk drawn on the path. The plate's Clear
/// selected, its card says what the walk read — past 90°, slot, entries —
/// and the viewport draws those stretches over the path and a dot at each
/// entry. With a picture.
#[test]
fn the_selected_paths_walk_is_said_on_the_card_and_drawn_on_the_path() {
    let mut h = open_pixels(Workspace::default());
    settle(&mut h);
    row(&h, "[1] Clear").click();
    settle(&mut h);
    let mut said = false;
    for _ in 0..100 {
        settle(&mut h);
        if h.query_all_by_label_contains("drawn on the path").next().is_some() {
            said = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(said, "the card says what the walk read");
    let drawn = shot(&mut h, "walk-on-path");
    // The engagement family's colour (240, 120, 80 in the default theme)
    // at the overlay's brightness, over the dark viewport: orange pixels
    // that no other layer paints.
    let orange = drawn.pixels().filter(|p| p[0] > 170 && (80..=150).contains(&p[1]) && p[2] < 110).count();
    assert!(orange > 100, "the stretches and the entries are drawn over the path: {orange} pixels in the family's colour");
}

/// ROADMAP item 3 slice 3: the Engagement layer under a fine stepover is
/// drawn within its budget and says so — the plate's face under a ⌀2,
/// 153,600 cells, drawn 3 × 3 cells a square, the status line carrying
/// the words beside the field's own line. With a picture.
#[test]
fn the_engagement_layer_under_a_fine_stepover_is_drawn_coarser_and_says_so() {
    let project = fine_plate();
    let mut ws = Workspace::default();
    ws.layout.layers.engagement = true;
    // The layer's line stands under its row in the Layers list, so the
    // list is pinned open to read it.
    ws.layout.layers_pinned = true;
    let mut h = open_path(&project, ws, true);
    settle(&mut h);
    row(&h, "[0] End").click();
    settle(&mut h);
    h.step();
    h.step();
    assert!(h.query_all_by_label_contains("drawn 3×3 cells a square, the worst of each").next().is_some(), "the layer's line says how the field is drawn");
    assert!(!h.state().output_lines().iter().any(|l| l.contains("cells a square")), "a layer's readout is not an event");
    shot(&mut h, "engagement-fine-plate");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// ROADMAP item 3 slice 3, the measurement: what a frame costs with the
/// face's End selected under a ⌀2 — the toolpaths alone, and with the
/// Engagement layer on (its field is 153,600 cells at an eighth of the
/// diameter). Prints; runs only under `NACS_SHELL_BENCH=1`, since a
/// timing is not a test.
#[test]
fn the_frame_cost_under_a_fine_stepover_is_measured() {
    if std::env::var_os("NACS_SHELL_BENCH").is_none() {
        return;
    }
    let project = fine_plate();
    for (name, toolpaths, engagement) in [("nothing drawn", false, false), ("toolpaths", true, false), ("toolpaths + engagement", true, true)] {
        let mut ws = Workspace::default();
        ws.layout.layers.toolpaths = toolpaths;
        ws.layout.layers.engagement = engagement;
        let mut h = open_path(&project, ws, false);
        settle(&mut h);
        let t0 = std::time::Instant::now();
        row(&h, "[0] End").click();
        h.step();
        let first = t0.elapsed();
        let t1 = std::time::Instant::now();
        for _ in 0..20 {
            h.step();
        }
        let per = t1.elapsed() / 20;
        eprintln!("bench {name}: the selecting frame {first:?}, then {per:?} per frame");
    }
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// The Output log is one line an event. With the Engagement layer on and
/// nothing selected the layer's hint stands in the Layers list and never
/// on the status line or in the log; `3` opens the dock without taking
/// the keyboard, so `z` still acts; the refusal is one line however long
/// it stands; *Save* writes the log beside the project with each line's
/// kind and source.
#[test]
fn the_output_log_is_one_line_an_event_and_saves_beside_the_project() {
    let (mut h, project) = open_copy("plate.cam");
    settle(&mut h);
    h.key_press(egui::Key::Num3);
    settle(&mut h);
    h.key_press(egui::Key::Z);
    settle(&mut h);
    h.key_press(egui::Key::CloseBracket);
    settle(&mut h);
    h.key_press(egui::Key::OpenBracket);
    settle(&mut h);
    let undo = h.state().output_lines().iter().filter(|l| l.contains("Nothing to undo")).count();
    assert_eq!(undo, 1, "z acted with the dock just opened, and its refusal is one line: {:?}", h.state().output_lines());
    h.get_by_label("Save").click();
    settle(&mut h);
    let file = std::fs::read_to_string(project.join("plate.log")).expect("the log is beside the project");
    assert!(file.starts_with("# nacs Output — "), "{file}");
    assert!(file.lines().any(|l| l.contains("refused  edit") && l.ends_with("Nothing to undo")), "{file}");
    assert!(h.state().output_lines().iter().any(|l| l.contains("Written — plate.log: the Output log")), "the save is an event too");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// An operation's card opens on the keys its line carries. The plate's
/// Clear carries a tool, a profile and a stepover: those three are fields,
/// the rest of what a run takes is under *More keys*, nothing is tagged
/// "optional", and a key from the fold can still be set.
#[test]
fn an_operations_card_opens_on_the_keys_its_line_carries() {
    let (mut h, project) = open_copy("plate_floated.cam");
    settle(&mut h);
    row(&h, "[1] ").click();
    settle(&mut h);
    let carried = nacs::view::fields_of(&std::fs::read_to_string(project.join("main.part")).unwrap().lines().filter(|l| l.trim_start().starts_with("run ")).nth(1).unwrap().to_string());
    assert!(h.query_all_by_label_contains("optional").next().is_none(), "no field is tagged optional");
    assert!(h.query_all_by_label_contains("More keys").next().is_some(), "the rest is one fold away");
    let shown = |h: &Harness<'static, ViewApp>, key: &str| h.query_all_by_label(&view::field_label(key, true)).next().is_some();
    for (key, _) in &carried {
        assert!(shown(&h, key), "{key} is on the line, so it is on the card");
    }
    assert!(!shown(&h, "peck_increment"), "a key the line does not carry is folded");
    h.query_all_by_label_contains("More keys").next().unwrap().click();
    settle(&mut h);
    assert!(shown(&h, "peck_increment"), "unfolded, every key the kind takes is there");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}

/// A key under *More keys* is typed where it stands: the field keeps the
/// keyboard from the first character to Enter, the fold stays open, and
/// the value lands on the run line.
#[test]
fn a_key_under_more_keys_is_typed_where_it_stands() {
    let (mut h, project) = open_copy("plate_floated.cam");
    settle(&mut h);
    row(&h, "[1] ").click();
    settle(&mut h);
    h.query_all_by_label_contains("More keys").next().unwrap().click();
    settle(&mut h);
    let label = view::field_label("peck_increment", true);
    let y = h.query_all_by_label(&label).next().expect("the key is on the card").rect().center().y;
    h.query_all_by_role(Role::TextInput).min_by(|a, b| (a.rect().center().y - y).abs().total_cmp(&(b.rect().center().y - y).abs())).unwrap().focus();
    h.step();
    h.event(egui::Event::Text("2".into()));
    h.step();
    h.step();
    h.event(egui::Event::Text("5".into()));
    h.step();
    h.step();
    assert!(h.query_all_by_label(&view::field_label("rapid_z", true)).next().is_some(), "the fold is still open after a character");
    h.key_press(egui::Key::Enter);
    settle(&mut h);
    let text = std::fs::read_to_string(project.join("main.part")).unwrap();
    assert!(text.contains("peck_increment=25"), "both characters reached the field and Enter wrote them: {text}");
    drop(h);
    let _ = std::fs::remove_dir_all(project.parent().unwrap());
}
