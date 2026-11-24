//! The window's configuration: the theme (every colour the window speaks, as
//! a named slot — meaning slots the view models bake into rows and paths,
//! chrome slots the shell's `egui::Visuals` is built from) and the workspace
//! layout it arranges itself by (`Layout`, at the bottom of this file).
//! Std-only and ALWAYS compiled, like `view.rs`: the slot table, its
//! validator and the layout model are pinned headless in both feature legs;
//! the JSON theme reader and everything egui stay behind `frontend`. A colour
//! that MEANS something (a role, a family, a stock truth) is read through one
//! home, so a theme cannot make a roughing pass read as finishing in one pane
//! and not another; `frontend.rs` constructs no colour of its own (pinned).

use crate::math::Portable as _;
use crate::parse::{FindingFamily, OpRole};

/// Every colour the window draws, by name. `[u8; 3]` and never an egui
/// type — the same std-only stance as `view.rs`, so the lean build and the
/// headless tests reach every slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    // ---- meaning slots: the viewport's vocabulary -------------------------
    /// Warm for material coming off in bulk.
    pub role_roughing: [u8; 3],
    /// Cool for the surface pass.
    pub role_finishing: [u8; 3],
    /// Untagged is ITS OWN colour, never Finishing's: a drill cycle coloured
    /// as a finishing pass is the lie the role slots exist to prevent.
    pub role_untagged: [u8; 3],
    /// Rapids are motion, not cutting — dim, one step above the ground.
    pub rapid: [u8; 3],
    /// The selected op's paths, and the picked edge, override with this.
    pub select: [u8; 3],
    /// Feature wireframes: the quiet part under the loud toolpaths.
    pub wire: [u8; 3],
    /// The stock's box, and the map's frame — the quiet reference grey.
    pub stock: [u8; 3],
    /// The viewport's ground — a machinist's dark sheet; every slot above
    /// is legible against it, and the pins keep them so.
    pub ground: [u8; 3],
    /// The map inset's ground, one step above the viewport's so the inset
    /// reads as a sheet lying on the desk rather than a hole in it.
    pub map_ground: [u8; 3],
    /// The map's viewport indicator — light and a little translucent so the
    /// edges under it stay readable. The one slot that carries its alpha:
    /// translucency is this slot's meaning, not a presentation choice.
    pub indicator: ([u8; 3], u8),
    /// The machined surface's base grey (lambert-shaded in the sim shader).
    pub sim_base: [u8; 3],
    /// The declared stock's boundaries at the cursor.
    pub removal: [u8; 3],
    /// A forgotten cell — declared removed, never swept.
    pub forgotten: [u8; 3],
    /// A violated cell — declared part, swept anyway.
    pub violated: [u8; 3],
    /// The error — the gate that fired. Brightest thing in the pane;
    /// advisory versus error is a brightness, never a hue.
    pub refusal: [u8; 3],
    /// The tool body's edges, and the fixture's keep-outs.
    pub tool: [u8; 3],
    pub fixture: [u8; 3],
    // The families, one slot each — `family_color` is the exhaustive match
    // that keeps a family added to the engine a compile error here rather
    // than a grey dot. The five that can gate, warm to cold; then the
    // advisories; then the decline, quietest of them.
    pub family_engagement: [u8; 3],
    pub family_deviation: [u8; 3],
    pub family_entry: [u8; 3],
    pub family_fixture: [u8; 3],
    pub family_reach: [u8; 3],
    pub family_envelope: [u8; 3],
    pub family_containment: [u8; 3],
    pub family_body_foul: [u8; 3],
    pub family_field_body_foul: [u8; 3],
    pub family_part_violation: [u8; 3],
    pub family_decline: [u8; 3],

    // ---- chrome slots: what the shell's Visuals is built from -------------
    // Defaults are egui's stock dark values, field for field (the mapping
    // lives in `frontend::visuals`, the one function that builds a Visuals).
    /// Windows and overlays (egui `window_fill`).
    pub background: [u8; 3],
    /// The panel column's fill (egui `panel_fill`, and the noninteractive
    /// widget fills that must match it).
    pub panel_background: [u8; 3],
    /// Separators, indentation lines, window strokes.
    pub border: [u8; 3],
    /// The border's hover state (a window edge or button under the pointer).
    pub border_hover: [u8; 3],
    /// Normal text — labels, the pane bodies.
    pub text: [u8; 3],
    /// The interactive text ladder egui draws widgets with, named because
    /// Zed's vocabulary has no per-state word for it: resting, hovered,
    /// pressed, and an open header's.
    pub text_interactive: [u8; 3],
    pub text_hover: [u8; 3],
    pub text_active: [u8; 3],
    pub text_open: [u8; 3],
    /// Widget bodies: a button or checkbox at rest, hovered, pressed, and
    /// an open header's fill.
    pub element: [u8; 3],
    pub element_hover: [u8; 3],
    pub element_active: [u8; 3],
    pub element_open: [u8; 3],
    /// The selection fill (a selected row, selected text).
    pub element_selected: [u8; 3],
    /// Two chrome slots the review found unthemed (2026-09-17, Phase 23
    /// slice 4): the text field's background (`editor.background` in
    /// Zed's vocabulary) and the scrollbar's handle (`scrollbar.thumb.
    /// background`). Same shape as every slot; no format change.
    pub text_field: [u8; 3],
    pub scrollbar: [u8; 3],
    /// Which of egui's two stock visuals the chrome starts from — the
    /// theme file's `appearance` word. Not a slot: nothing is drawn with
    /// it, it says what the slots are laid over.
    pub appearance: Appearance,
    /// The alpha each FILL wears (Phase 25 slice 2, fork 2): the ten slots
    /// a see-through window shows the desk through. Strokes and text never
    /// carry one — legibility is judged on an opaque mark over whatever the
    /// fills let through. 255 everywhere is the opaque window as shipped,
    /// and every law below is byte-identical there.
    pub fill_alpha: FillAlpha,
    /// Zed's `background.appearance` word: whether the window asks the
    /// platform for a see-through surface, and for a blur behind it.
    pub background_appearance: BackgroundAppearance,
}

/// The alpha of each fill slot, 255 = opaque. The ten fills, by name:
/// `background`, `panel.background`, `text.field`, `element`,
/// `element.hover`, `element.active`, `element.selected`, `element.open`,
/// `ground`, `map.ground` (the indicator keeps its own alpha beside its
/// colour, as it always did — eleven slots carry one in all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillAlpha {
    pub background: u8,
    pub panel_background: u8,
    pub text_field: u8,
    pub element: u8,
    pub element_hover: u8,
    pub element_active: u8,
    pub element_selected: u8,
    pub element_open: u8,
    pub ground: u8,
    pub map_ground: u8,
}

impl Default for FillAlpha {
    fn default() -> Self {
        Self {
            background: 255,
            panel_background: 255,
            text_field: 255,
            element: 255,
            element_hover: 255,
            element_active: 255,
            element_selected: 255,
            element_open: 255,
            ground: 255,
            map_ground: 255,
        }
    }
}

/// Zed's `background.appearance`: `opaque` (absent, or any word the
/// reader does not know — the unknown-line rule), `transparent` (the
/// fills' alpha shows the desk), `blurred` (and the platform is asked to
/// blur what shows — slice 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BackgroundAppearance {
    #[default]
    Opaque,
    Transparent,
    Blurred,
}

impl BackgroundAppearance {
    pub fn word(self) -> &'static str {
        match self {
            BackgroundAppearance::Opaque => "opaque",
            BackgroundAppearance::Transparent => "transparent",
            BackgroundAppearance::Blurred => "blurred",
        }
    }
}

/// A theme's light-or-dark word (Zed's `appearance`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Dark,
    Light,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // The meaning palette, unchanged from the view.rs constants it
            // replaced.
            role_roughing: [235, 160, 60],
            role_finishing: [90, 190, 235],
            role_untagged: [150, 150, 158],
            rapid: [101, 101, 108],
            select: [255, 235, 100],
            wire: [98, 102, 112],
            stock: [96, 102, 113],
            ground: [24, 26, 30],
            map_ground: [31, 34, 40],
            indicator: ([235, 238, 245], 190),
            sim_base: [148, 152, 158],
            removal: [120, 210, 140],
            forgotten: [255, 210, 160],
            violated: [255, 80, 200],
            refusal: [255, 70, 70],
            tool: [235, 235, 120],
            fixture: [200, 140, 90],
            family_engagement: [240, 120, 80],
            family_deviation: [235, 180, 70],
            family_entry: [250, 90, 120],
            family_fixture: [200, 120, 240],
            family_reach: [160, 140, 250],
            family_envelope: [95, 165, 255],
            family_containment: [120, 200, 160],
            family_body_foul: [240, 160, 200],
            family_field_body_foul: [200, 200, 120],
            family_part_violation: [120, 220, 230],
            family_decline: [150, 160, 170],
            // The chrome, faithful to egui's stock dark Visuals — each value
            // is the number `Visuals::dark()`/`Widgets::dark()` ships for
            // the field `frontend::visuals` maps it onto.
            background: [27, 27, 27],
            panel_background: [27, 27, 27],
            border: [103, 103, 103],
            border_hover: [150, 150, 150],
            text: [140, 140, 140],
            text_interactive: [180, 180, 180],
            text_hover: [240, 240, 240],
            text_active: [255, 255, 255],
            text_open: [210, 210, 210],
            element: [60, 60, 60],
            element_hover: [70, 70, 70],
            element_active: [55, 55, 55],
            element_open: [45, 45, 45],
            element_selected: [0, 92, 128],
            text_field: [19, 19, 19],
            scrollbar: [103, 103, 103],
            appearance: Appearance::Dark,
            fill_alpha: FillAlpha::default(),
            background_appearance: BackgroundAppearance::Opaque,
        }
    }
}

impl Theme {
    /// The role's colour — ONE home, shared by the window's two painters
    /// (main view, minimap) and any future CLI-side map, so no two surfaces
    /// can disagree about what roughing looks like. An exhaustive match: if
    /// `OpRole` ever grows an arm, this stops compiling instead of silently
    /// painting it grey. (It will not grow one for a role SCALE — closed by
    /// ruling 2026-08-21; the tag is the only bucket a
    /// colour carries.)
    pub fn role_color(&self, role: OpRole) -> [u8; 3] {
        match role {
            OpRole::Roughing => self.role_roughing,
            OpRole::Finishing => self.role_finishing,
            OpRole::Untagged => self.role_untagged,
        }
    }

    /// The colour a finding's family uses — the other exhaustive match: a
    /// family added to the engine is a compile error here rather than a
    /// grey dot. A family's colour is its meaning; advisory versus error
    /// is a brightness (`refusal` is the error's), never a hue.
    pub fn family_color(&self, family: FindingFamily) -> [u8; 3] {
        use FindingFamily as F;
        match family {
            F::Engagement => self.family_engagement,
            F::Deviation => self.family_deviation,
            F::Entry => self.family_entry,
            F::Fixture => self.family_fixture,
            F::Reach => self.family_reach,
            F::Envelope => self.family_envelope,
            F::Containment => self.family_containment,
            F::BodyFoul => self.family_body_foul,
            F::FieldBodyFoul => self.family_field_body_foul,
            F::PartViolation => self.family_part_violation,
            F::Decline => self.family_decline,
        }
    }

    /// Every MEANING slot, by the name the theme file will use — the
    /// totality surface the pins (and slice 3's validator) quantify over,
    /// so a slot added to the struct without joining its half of the table
    /// is caught by the "new field, stale list" review this one function
    /// concentrates. The indicator appears as its colour; its alpha rides
    /// the struct.
    pub fn meaning_slots(&self) -> Vec<(&'static str, [u8; 3])> {
        vec![
            ("role.roughing", self.role_roughing),
            ("role.finishing", self.role_finishing),
            ("role.untagged", self.role_untagged),
            ("rapid", self.rapid),
            ("select", self.select),
            ("wire", self.wire),
            ("stock", self.stock),
            ("ground", self.ground),
            ("map.ground", self.map_ground),
            ("indicator", self.indicator.0),
            ("sim.base", self.sim_base),
            ("removal", self.removal),
            ("forgotten", self.forgotten),
            ("violated", self.violated),
            ("refusal", self.refusal),
            ("tool", self.tool),
            ("fixture", self.fixture),
            ("family.engagement", self.family_engagement),
            ("family.deviation", self.family_deviation),
            ("family.entry", self.family_entry),
            ("family.fixture", self.family_fixture),
            ("family.reach", self.family_reach),
            ("family.envelope", self.family_envelope),
            ("family.containment", self.family_containment),
            ("family.body_foul", self.family_body_foul),
            ("family.field_body_foul", self.family_field_body_foul),
            ("family.part_violation", self.family_part_violation),
            ("family.decline", self.family_decline),
        ]
    }

    /// Every CHROME slot, same contract as `meaning_slots`. The names lean
    /// on Zed's style vocabulary where it has the word.
    pub fn chrome_slots(&self) -> Vec<(&'static str, [u8; 3])> {
        vec![
            ("background", self.background),
            ("panel.background", self.panel_background),
            ("border", self.border),
            ("border.hover", self.border_hover),
            ("text", self.text),
            ("text.interactive", self.text_interactive),
            ("text.hover", self.text_hover),
            ("text.active", self.text_active),
            ("text.open", self.text_open),
            ("element", self.element),
            ("element.hover", self.element_hover),
            ("element.active", self.element_active),
            ("element.open", self.element_open),
            ("element.selected", self.element_selected),
            ("text.field", self.text_field),
            ("scrollbar", self.scrollbar),
        ]
    }
}

/// Perceived luminance (Rec. 601 weights) of a drawn colour, 0..=255 — the
/// one legibility measure the ground pins use (moved here from `view.rs`
/// with the slots it measures), so "darker than the ground" means the same
/// thing for every slot; slice 3's validator reads the same function.
pub fn luminance(rgb: [u8; 3]) -> f64 {
    0.299 * f64::from(rgb[0]) + 0.587 * f64::from(rgb[1]) + 0.114 * f64::from(rgb[2])
}

// ---- the validator ----------------------------------------------------------
//
// The half of the themes item with a correctness obligation (`DECISIONS.md` → "DRAFT: Phase 12…", fork 5A): this renderer's colours CARRY MEANING,
// so a theme is judged before it is active. Two grades. A MEANING violation
// — a role or family colour that stops reading, or the three role colours
// confusable under normal vision or under any of the three common
// dichromacies — REJECTS the theme's meaning slots: the built-in palette
// stands for the viewport, the chrome half still applies, and the reason
// is named. A CHROME finding is named and applied anyway: taste is the
// user's, safety is not. Std-only and always compiled, so the lean leg
// pins the laws and `--theme-check` is a thin voice over `validate`.

/// One validator finding: the grade decides what happens (a `Meaning`
/// finding rejects the meaning slots; a `Chrome` one is advisory), the
/// message is the sentence `--theme-check` prints.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeFinding {
    pub grade: ThemeGrade,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeGrade {
    Meaning,
    Chrome,
}

/// The three dichromacies the role triple is judged under, beside normal
/// vision — two of the three role colours being confusable under any of
/// them is a safety-adjacent defect, not a cosmetic one (the pool entry's
/// own words).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cvd {
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

impl Cvd {
    pub const ALL: [Cvd; 3] = [Cvd::Protanopia, Cvd::Deuteranopia, Cvd::Tritanopia];

    pub fn word(self) -> &'static str {
        match self {
            Cvd::Protanopia => "protanopia",
            Cvd::Deuteranopia => "deuteranopia",
            Cvd::Tritanopia => "tritanopia",
        }
    }

    /// The Machado et al. (2009) severity-1.0 simulation matrix, applied
    /// in LINEAR RGB. Each row sums to 1, so a pure grey is a fixed point
    /// — pinned, since a mistyped constant here would quietly judge the
    /// wrong colours.
    fn matrix(self) -> [[f64; 3]; 3] {
        match self {
            Cvd::Protanopia => [
                [0.152_286, 1.052_583, -0.204_868],
                [0.114_503, 0.786_281, 0.099_216],
                [-0.003_882, -0.048_116, 1.051_998],
            ],
            Cvd::Deuteranopia => [
                [0.367_322, 0.860_646, -0.227_968],
                [0.280_085, 0.672_501, 0.047_413],
                [-0.011_820, 0.042_940, 0.968_881],
            ],
            Cvd::Tritanopia => [
                [1.255_528, -0.076_749, -0.178_779],
                [-0.078_411, 0.930_809, 0.147_602],
                [0.004_733, 0.691_367, 0.303_900],
            ],
        }
    }
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).ppowf(2.4) }
}

/// A colour as the named dichromat sees it, in linear RGB.
fn simulate_linear(rgb: [u8; 3], cvd: Cvd) -> [f64; 3] {
    let lin = [srgb_to_linear(rgb[0]), srgb_to_linear(rgb[1]), srgb_to_linear(rgb[2])];
    let m = cvd.matrix();
    let mut out = [0.0; 3];
    for (i, row) in m.iter().enumerate() {
        out[i] = (row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2]).clamp(0.0, 1.0);
    }
    out
}

/// CIE L*a*b* (D65) from linear RGB — the space the perceptual distance
/// is measured in.
fn lab_from_linear(lin: [f64; 3]) -> [f64; 3] {
    let x = (0.4124 * lin[0] + 0.3576 * lin[1] + 0.1805 * lin[2]) / 0.95047;
    let y = 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
    let z = (0.0193 * lin[0] + 0.1192 * lin[1] + 0.9505 * lin[2]) / 1.08883;
    let f = |t: f64| if t > 0.008856 { t.pcbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// The perceptual distance (CIE76 ΔE) between two colours as seen under
/// `cvd`, or under normal vision when `cvd` is `None` — the one measure
/// the role-distinguishability law uses.
pub fn delta_e(a: [u8; 3], b: [u8; 3], cvd: Option<Cvd>) -> f64 {
    let (la, lb) = match cvd {
        Some(c) => (lab_from_linear(simulate_linear(a, c)), lab_from_linear(simulate_linear(b, c))),
        None => (
            lab_from_linear([srgb_to_linear(a[0]), srgb_to_linear(a[1]), srgb_to_linear(a[2])]),
            lab_from_linear([srgb_to_linear(b[0]), srgb_to_linear(b[1]), srgb_to_linear(b[2])]),
        ),
    };
    ((la[0] - lb[0]).powi(2) + (la[1] - lb[1]).powi(2) + (la[2] - lb[2]).powi(2)).sqrt()
}

/// The role triple's pairwise floor, under normal vision and under every
/// simulated dichromacy. Calibrated against the built-in palette (whose
/// tightest pair must clear it with room) and pinned; a confusable pair
/// sits far below it.
pub const ROLE_DELTA_E_FLOOR: f64 = 18.0;

/// The legibility law's floors (2026-09-17, Phase 23 slice 4): WCAG
/// contrast RATIOS, unsigned — a light ground is as legal as a dark one.
/// A stroke in the viewport and a chrome edge read at 3:1; text, and the
/// family colours the findings pane prints text in, at 4.5:1. The old
/// law was a signed Rec.601 luminance delta wearing a contrast label: a
/// slot at its floor measured 1.5:1, blue failed while grey passed, and
/// a light ground failed every meaning slot at once — whereupon
/// `apply_policy` laid the dark palette over the light chrome.
pub const STROKE_CONTRAST_FLOOR: f64 = 3.0;
pub const TEXT_CONTRAST_FLOOR: f64 = 4.5;

/// A colour's relative luminance, 0..=1 — the sRGB transfer undone, the
/// Rec.709 weights (WCAG 2's own arithmetic).
pub fn relative_luminance(rgb: [u8; 3]) -> f64 {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).ppowf(2.4)
        }
    };
    0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2])
}

/// The contrast ratio between two colours, 1..=21, either way round —
/// WCAG 2's `(L1 + 0.05) / (L2 + 0.05)` with the brighter on top.
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

impl Theme {
    /// Whether any fill lets the desk through — the one question that
    /// decides whether the law below judges at two desks or one.
    pub fn translucent(&self) -> bool {
        let f = self.fill_alpha;
        [f.background, f.panel_background, f.text_field, f.element, f.element_hover, f.element_active, f.element_selected, f.element_open, f.ground, f.map_ground]
            .iter()
            .any(|&a| a < 255)
    }

    /// The layers under a mark in the viewport, bottom-up, each with its
    /// alpha: the swapchain's `background`, the panel, the ground — the
    /// paint order the shell actually uses, so the law judges what the
    /// eye meets.
    fn ground_stack(&self) -> Vec<([u8; 3], u8)> {
        let f = self.fill_alpha;
        vec![(self.background, f.background), (self.panel_background, f.panel_background), (self.ground, f.ground)]
    }

    /// The layers under text on the panel.
    fn panel_stack(&self) -> Vec<([u8; 3], u8)> {
        let f = self.fill_alpha;
        vec![(self.background, f.background), (self.panel_background, f.panel_background)]
    }
}

/// Gamma-space source-over: `top` at `alpha` laid on `under`. The
/// compositor blends the swapchain's 8-bit values as they are, which is
/// what the eye meets; a linear-light blend would flatter every dark
/// glass over a light desk.
pub fn over(top: [u8; 3], alpha: u8, under: [u8; 3]) -> [u8; 3] {
    let a = f64::from(alpha) / 255.0;
    let mut out = [0u8; 3];
    for i in 0..3 {
        out[i] = (f64::from(top[i]) * a + f64::from(under[i]) * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// A stack of fills, bottom-up, composited onto a desk.
pub fn stack_on(desk: [u8; 3], layers: &[([u8; 3], u8)]) -> [u8; 3] {
    layers.iter().fold(desk, |acc, &(c, a)| over(c, a, acc))
}

pub const BLACK_DESK: [u8; 3] = [0, 0, 0];
pub const WHITE_DESK: [u8; 3] = [255, 255, 255];

/// The contrast a mark gets against a stack of fills on ANY desk (Phase
/// 25 slice 2, fork 3): the desk is unseen but bounded by black and
/// white, the composite is monotone in the desk's brightness, so the
/// worst case is at an endpoint — unless the mark's own luminance lies
/// inside the interval the stack sweeps, where some desk meets it at
/// 1:1. Returns the ratio and, when the stack is not opaque, the words
/// for where it was worst (`None` for an opaque stack: the number is the
/// one the old law computed, byte for byte).
pub fn worst_contrast(mark: [u8; 3], layers: &[([u8; 3], u8)]) -> (f64, [u8; 3], Option<&'static str>) {
    let on_black = stack_on(BLACK_DESK, layers);
    let on_white = stack_on(WHITE_DESK, layers);
    if on_black == on_white {
        return (contrast(mark, on_black), on_black, None);
    }
    let (lm, lb, lw) = (relative_luminance(mark), relative_luminance(on_black), relative_luminance(on_white));
    if lm > lb.min(lw) && lm < lb.max(lw) {
        return (1.0, on_white, Some("on some desk between black and white, where the glass meets its own brightness"));
    }
    let (cb, cw) = (contrast(mark, on_black), contrast(mark, on_white));
    if cw <= cb { (cw, on_white, Some("on a white desk")) } else { (cb, on_black, Some("on a black desk")) }
}

/// Judge a theme. Every law a `Meaning` finding states here is one the
/// built-in palette passes (pinned), so rejecting on them can never brick
/// the window — the fallback always stands. A translucent fill is judged
/// at both desks (`worst_contrast`); an opaque theme's findings are the
/// old law's, byte for byte.
pub fn validate(t: &Theme) -> Vec<ThemeFinding> {
    let mut out = Vec::new();
    let meaning = |msg: String| ThemeFinding { grade: ThemeGrade::Meaning, message: msg };
    for (name, c) in t.meaning_slots() {
        if name == "ground" || name == "map.ground" {
            continue;
        }
        // A family colour is printed as TEXT in the findings pane (and
        // the error is), so it reads at the text floor; every other
        // meaning slot is a stroke in the viewport.
        let floor = if name.starts_with("family.") || name == "refusal" {
            TEXT_CONTRAST_FLOOR
        } else {
            STROKE_CONTRAST_FLOOR
        };
        let (ratio, seen, desk) = worst_contrast(c, &t.ground_stack());
        if ratio < floor {
            out.push(meaning(match desk {
                None => format!("`{name}` {c:?} does not read against the ground {seen:?} ({ratio:.2}:1, floor {floor}:1)"),
                Some(d) => format!("`{name}` {c:?} does not read against the ground {seen:?} {d} ({ratio:.2}:1, floor {floor}:1)"),
            }));
        }
    }
    // The map's ground layers between the viewport's and the quietest
    // edge — on the ground's own side of the wire, whichever way the
    // theme is lit (a light theme's sheet is a shade darker than its
    // desk, a dark theme's a shade lighter).
    // Judged on every desk: the ground and the sheet composited on black
    // and on white (an opaque theme's two answers are one answer).
    {
        let ground_stack = t.ground_stack();
        let mut map_stack = ground_stack.clone();
        map_stack.push((t.map_ground, t.fill_alpha.map_ground));
        let w = relative_luminance(t.wire);
        let mut failed: Option<&'static str> = None;
        for (desk, word) in [(BLACK_DESK, "on a black desk"), (WHITE_DESK, "on a white desk")] {
            let g = relative_luminance(stack_on(desk, &ground_stack));
            let m = relative_luminance(stack_on(desk, &map_stack));
            let between = (m - g) * (w - g) > 0.0 && (m - g).abs() < (w - g).abs();
            if !between && failed.is_none() {
                failed = Some(word);
            }
        }
        if let Some(word) = failed {
            let desk_words = if t.translucent() { format!(" {word}") } else { String::new() };
            out.push(meaning(format!(
                "`map.ground` {:?} must sit between the ground and the edge colour{desk_words} — a sheet on the desk, not a hole in it",
                t.map_ground
            )));
        }
    }
    // The role triple: pairwise distinguishable under normal vision AND
    // under each dichromacy — the item's safety obligation.
    let roles = [
        ("role.roughing", t.role_roughing),
        ("role.finishing", t.role_finishing),
        ("role.untagged", t.role_untagged),
    ];
    for i in 0..roles.len() {
        for j in (i + 1)..roles.len() {
            for cvd in [None, Some(Cvd::Protanopia), Some(Cvd::Deuteranopia), Some(Cvd::Tritanopia)] {
                let d = delta_e(roles[i].1, roles[j].1, cvd);
                if d < ROLE_DELTA_E_FLOOR {
                    let eye = cvd.map_or("normal vision", Cvd::word);
                    out.push(meaning(format!(
                        "`{}` {:?} and `{}` {:?} are confusable under {eye} (ΔE {d:.1}, floor {ROLE_DELTA_E_FLOOR:.0}) — a roughing pass must never read as a finishing pass",
                        roles[i].0, roles[i].1, roles[j].0, roles[j].1
                    )));
                }
            }
        }
    }
    // Families: each its own colour, and the error brighter than all of
    // them — advisory-versus-error is a brightness, never a hue.
    let families: Vec<(&str, [u8; 3])> =
        t.meaning_slots().into_iter().filter(|(n, _)| n.starts_with("family.")).collect();
    for i in 0..families.len() {
        for j in (i + 1)..families.len() {
            if families[i].1 == families[j].1 {
                out.push(meaning(format!(
                    "`{}` and `{}` share a colour {:?} — two families, one meaning",
                    families[i].0, families[j].0, families[i].1
                )));
                continue;
            }
            // The eleven families under each eye (2026-09-17): ADVISORY,
            // not meaning. A family's colour is never its only cue — the
            // row carries the family's word, as `--check` does — and
            // eleven hues cannot be pairwise ΔE 18 under a dichromacy:
            // measured on every shipped theme (6 to 14 confusable pairs
            // each). The note names the pair and the eye, so a theme
            // author can spend the pairs they care about.
            for cvd in [None, Some(Cvd::Protanopia), Some(Cvd::Deuteranopia), Some(Cvd::Tritanopia)] {
                let d = delta_e(families[i].1, families[j].1, cvd);
                if d < ROLE_DELTA_E_FLOOR {
                    out.push(ThemeFinding {
                        grade: ThemeGrade::Chrome,
                        message: format!(
                            "`{}` and `{}` are close under {} (ΔE {d:.1}) — the row's word tells them apart; the colours alone may not",
                            families[i].0,
                            families[j].0,
                            cvd.map_or("normal vision", Cvd::word)
                        ),
                    });
                }
            }
        }
    }
    // The stock's other two voices beside the roles: a forgotten cell is
    // uncut material, a roughing pass is a cut — the same amber said
    // both (ΔE 7.7 in the built-in until 2026-09-17). Meaning, like the
    // roles: one of them is a scrap part.
    for (name, c) in [("forgotten", t.forgotten), ("violated", t.violated)] {
        for (role, rc) in [("role.roughing", t.role_roughing), ("role.finishing", t.role_finishing), ("role.untagged", t.role_untagged)] {
            let d = delta_e(c, rc, None);
            if d < ROLE_DELTA_E_FLOOR {
                out.push(meaning(format!(
                    "`{name}` {c:?} and `{role}` {rc:?} are confusable (ΔE {d:.1}, floor {ROLE_DELTA_E_FLOOR:.0}) — a cell the program forgot must never read as a pass that cut it"
                )));
            }
        }
    }
    // The error is DISTINCT from every family — a family row that uses
    // (or nearly uses) the error's colour forges the gate's voice. The
    // DRAFT said "out-luminances every family"; measured, that was false
    // of the very palette it described (deviation's amber out-luminances
    // the error's red), so the law is distance, same floor as the roles.
    for (name, c) in &families {
        let d = delta_e(t.refusal, *c, None);
        if d < ROLE_DELTA_E_FLOOR {
            out.push(meaning(format!(
                "`{name}` {c:?} is confusable with the error {:?} (ΔE {d:.1}, floor {ROLE_DELTA_E_FLOOR:.0}) — only the gate uses the error's colour",
                t.refusal
            )));
        }
    }
    // The chrome, advisory: text reads against the panel it sits on at
    // the text floor; the border — the resize handle — and the scrollbar
    // are visible against it at the stroke floor; a text field's text
    // reads on the field.
    let panel_stack = t.panel_stack();
    let desk_clause = |d: Option<&str>| d.map_or(String::new(), |d| format!(" {d}"));
    for (name, c) in [
        ("text", t.text),
        ("text.interactive", t.text_interactive),
        ("text.hover", t.text_hover),
        ("text.active", t.text_active),
        ("text.open", t.text_open),
    ] {
        let (ratio, panel, desk) = worst_contrast(c, &panel_stack);
        if ratio < TEXT_CONTRAST_FLOOR {
            out.push(ThemeFinding {
                grade: ThemeGrade::Chrome,
                message: format!("`{name}` {c:?} is hard to read against `panel.background` {panel:?}{} ({ratio:.2}:1, floor {TEXT_CONTRAST_FLOOR}:1)", desk_clause(desk)),
            });
        }
    }
    for (name, c) in [("border", t.border), ("scrollbar", t.scrollbar)] {
        let (ratio, panel, desk) = worst_contrast(c, &panel_stack);
        if ratio < STROKE_CONTRAST_FLOOR {
            out.push(ThemeFinding {
                grade: ThemeGrade::Chrome,
                message: format!("`{name}` {c:?} is invisible against `panel.background` {panel:?}{} ({ratio:.2}:1, floor {STROKE_CONTRAST_FLOOR}:1)", desk_clause(desk)),
            });
        }
    }
    let mut field_stack = panel_stack.clone();
    field_stack.push((t.text_field, t.fill_alpha.text_field));
    let (ratio, field, desk) = worst_contrast(t.text_interactive, &field_stack);
    if ratio < TEXT_CONTRAST_FLOOR {
        out.push(ThemeFinding {
            grade: ThemeGrade::Chrome,
            message: format!("`text.interactive` {:?} is hard to read in a text field `text.field` {field:?}{} ({ratio:.2}:1, floor {TEXT_CONTRAST_FLOOR}:1)", t.text_interactive, desk_clause(desk)),
        });
    }
    out
}

/// The error policy (fork 5A): a candidate with any `Meaning` finding
/// keeps its CHROME and loses its meaning slots to the built-in palette —
/// a theme physically cannot make a roughing pass read as a finishing
/// pass, and a taste tweak is never an outage. The findings ride along
/// for the caller to voice.
pub fn apply_policy(candidate: Theme) -> (Theme, Vec<ThemeFinding>) {
    let findings = validate(&candidate);
    if findings.iter().any(|f| f.grade == ThemeGrade::Meaning) {
        let mut t = Theme::default();
        for (name, c) in candidate.chrome_slots() {
            t.set_slot(name, c, None);
        }
        t.appearance = candidate.appearance;
        (t, findings)
    } else {
        (candidate, findings)
    }
}

// ---- the theme's file form: names and hex -----------------------------------

/// `#rrggbb` or `#rrggbbaa` — the two forms Zed writes. The alpha comes
/// back beside the colour; only the indicator slot keeps one.
pub fn parse_hex(s: &str) -> Option<([u8; 3], Option<u8>)> {
    let hex = s.trim().strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(([byte(0)?, byte(2)?, byte(4)?], None)),
        8 => Some(([byte(0)?, byte(2)?, byte(4)?], Some(byte(6)?))),
        _ => None,
    }
}

impl Theme {
    /// Set one slot by the name the tables print — the one door a file's
    /// colour enters through, meaning and chrome alike. Returns false for
    /// a name no table knows (the caller ignores it, the unknown-line
    /// rule). The alpha lands only where a slot carries one (the
    /// indicator); elsewhere it is dropped — translucency is otherwise the
    /// shell's presentation number.
    pub fn set_slot(&mut self, name: &str, c: [u8; 3], alpha: Option<u8>) -> bool {
        // A fill keeps the alpha it was written with — `#rrggbb` is
        // opaque — and nothing else does (Phase 25 slice 2, fork 2).
        let a = alpha.unwrap_or(255);
        match name {
            "background" => self.fill_alpha.background = a,
            "panel.background" => self.fill_alpha.panel_background = a,
            "text.field" => self.fill_alpha.text_field = a,
            "element" => self.fill_alpha.element = a,
            "element.hover" => self.fill_alpha.element_hover = a,
            "element.active" => self.fill_alpha.element_active = a,
            "element.selected" => self.fill_alpha.element_selected = a,
            "element.open" => self.fill_alpha.element_open = a,
            "ground" => self.fill_alpha.ground = a,
            "map.ground" => self.fill_alpha.map_ground = a,
            _ => {}
        }
        let field: &mut [u8; 3] = match name {
            "role.roughing" => &mut self.role_roughing,
            "role.finishing" => &mut self.role_finishing,
            "role.untagged" => &mut self.role_untagged,
            "rapid" => &mut self.rapid,
            "select" => &mut self.select,
            "wire" => &mut self.wire,
            "stock" => &mut self.stock,
            "ground" => &mut self.ground,
            "map.ground" => &mut self.map_ground,
            "indicator" => {
                self.indicator.0 = c;
                if let Some(a) = alpha {
                    self.indicator.1 = a;
                }
                return true;
            }
            "sim.base" => &mut self.sim_base,
            "removal" => &mut self.removal,
            "forgotten" => &mut self.forgotten,
            "violated" => &mut self.violated,
            "refusal" => &mut self.refusal,
            "tool" => &mut self.tool,
            "fixture" => &mut self.fixture,
            "family.engagement" => &mut self.family_engagement,
            "family.deviation" => &mut self.family_deviation,
            "family.entry" => &mut self.family_entry,
            "family.fixture" => &mut self.family_fixture,
            "family.reach" => &mut self.family_reach,
            "family.envelope" => &mut self.family_envelope,
            "family.containment" => &mut self.family_containment,
            "family.body_foul" => &mut self.family_body_foul,
            "family.field_body_foul" => &mut self.family_field_body_foul,
            "family.part_violation" => &mut self.family_part_violation,
            "family.decline" => &mut self.family_decline,
            "background" => &mut self.background,
            "panel.background" => &mut self.panel_background,
            "border" => &mut self.border,
            "border.hover" => &mut self.border_hover,
            "text" => &mut self.text,
            "text.interactive" => &mut self.text_interactive,
            "text.hover" => &mut self.text_hover,
            "text.active" => &mut self.text_active,
            "text.open" => &mut self.text_open,
            "element" => &mut self.element,
            "element.hover" => &mut self.element_hover,
            "element.active" => &mut self.element_active,
            "element.open" => &mut self.element_open,
            "element.selected" => &mut self.element_selected,
            "text.field" => &mut self.text_field,
            "scrollbar" => &mut self.scrollbar,
            _ => return false,
        };
        *field = c;
        true
    }
}

// ---- the Zed-shape reader ---------------------------------------------------
//
// The adopted format (fork 4A): a theme is one JSON file in Zed's own
// theme shape, so a file written for Zed drops in and the chrome follows
// it. Read by the `serde_json` the ledger already carries — behind the
// `frontend` gate so the lean build stays JSON-free; everything the
// reader FEEDS (`set_slot`, `parse_hex`, `validate`, `apply_policy`) is
// std-only above, where the lean leg's pins reach it.

/// A `Theme` from a Zed-shape JSON file: either a theme FAMILY
/// (`{"themes": [...]}` — the first entry is active, a family's default) or
/// a single theme object (`{"style": {...}}`). The chrome maps from Zed's
/// style vocabulary; the meaning slots come from one optional `"nacs"`
/// object per theme and default to the built-in palette — a pure Zed file
/// is a valid theme whose viewport speaks the stock, already-validated
/// colours. The caller judges the result through `apply_policy`.
#[cfg(feature = "frontend")]
pub fn theme_from_json(text: &str) -> Result<Theme, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let theme_obj = if v.get("style").is_some() {
        &v
    } else {
        v.get("themes")
            .and_then(|t| t.as_array())
            .and_then(|a| a.first())
            .ok_or("neither a Zed theme family (`themes: [...]`) nor a single theme (`style: {...}`)")?
    };
    let style = theme_obj
        .get("style")
        .and_then(|s| s.as_object())
        .ok_or("the theme has no `style` object")?;
    let mut t = Theme::default();
    // Zed key → nacs chrome slots. Where egui draws more states than Zed
    // has words, one Zed colour feeds the ladder (slice 0's finding,
    // resolved by adoption: an imported theme flattens the text ladder,
    // and the `nacs` object can name the rungs back apart).
    let map: &[(&str, &[&str])] = &[
        ("background", &["background"]),
        ("panel.background", &["panel.background"]),
        ("border", &["border"]),
        ("border.focused", &["border.hover"]),
        ("text", &["text", "text.interactive", "text.hover", "text.active", "text.open"]),
        ("element.background", &["element", "element.open"]),
        ("element.hover", &["element.hover"]),
        ("element.active", &["element.active"]),
        ("element.selected", &["element.selected"]),
        ("editor.background", &["text.field"]),
        ("scrollbar.thumb.background", &["scrollbar"]),
    ];
    // The theme's light-or-dark word: what the chrome is laid over.
    if theme_obj.get("appearance").and_then(|a| a.as_str()) == Some("light") {
        t.appearance = Appearance::Light;
    }
    // Zed's own word for the glass (Phase 25 slice 2): whether the window
    // asks for a see-through surface, and a blur behind it. Absent or
    // unknown is opaque — the unknown-line rule.
    t.background_appearance = match style.get("background.appearance").and_then(|a| a.as_str()) {
        Some("transparent") => BackgroundAppearance::Transparent,
        Some("blurred") => BackgroundAppearance::Blurred,
        _ => BackgroundAppearance::Opaque,
    };
    for (zed_key, slots) in map {
        if let Some((c, a)) = style.get(*zed_key).and_then(|x| x.as_str()).and_then(parse_hex) {
            for slot in *slots {
                t.set_slot(slot, c, a);
            }
        }
    }
    if let Some(nacs) = theme_obj.get("nacs").and_then(|o| o.as_object()) {
        for (k, val) in nacs {
            if let Some((c, a)) = val.as_str().and_then(parse_hex) {
                // An unknown name is the unknown-line rule again: ignored.
                t.set_slot(k, c, a);
            }
        }
    }
    Ok(t)
}

/// The themes carried in the binary (Phase 51 slice 5, DESIGN.md §5.1 and
/// §5.7): the repo's `themes/` folder, embedded at build — the folder is
/// the source and this list is derived from it (a test pins each string
/// to its file and the list to the folder's listing). A bare `--theme
/// <name>` that the user's config folder does not hold reads one of these,
/// so a machine reset that empties the folder cannot turn the glass off.
pub const SHIPPED_THEMES: &[(&str, &str)] = &[
    ("catppuccin-espresso-blur-light", include_str!("../themes/catppuccin-espresso-blur-light.json")),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.json")),
    ("nacs-dark", include_str!("../themes/nacs-dark.json")),
    ("nacs-light", include_str!("../themes/nacs-light.json")),
    ("solarized-dark", include_str!("../themes/solarized-dark.json")),
    ("tokyo-night", include_str!("../themes/tokyo-night.json")),
];

/// Where a theme choice reads from: a FILE (a path as typed, or the
/// user's copy under the config folder, which wins by name), or a theme
/// SHIPPED in the binary by that name. A bare name found nowhere resolves
/// to the file it would have been, so the refusal names the path looked
/// at; `None` only when a bare name has no config dir and is not shipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThemeSource {
    File(std::path::PathBuf),
    Shipped(&'static str, &'static str),
}

pub fn theme_source(choice: &str) -> Option<ThemeSource> {
    if choice.contains('/') || choice.contains('\\') || choice.ends_with(".json") {
        return Some(ThemeSource::File(std::path::PathBuf::from(choice)));
    }
    let in_folder = themes_folder().map(|d| d.join(format!("{choice}.json")));
    if let Some(p) = &in_folder {
        if p.is_file() {
            return Some(ThemeSource::File(p.clone()));
        }
    }
    if let Some((name, json)) = SHIPPED_THEMES.iter().find(|(n, _)| *n == choice) {
        return Some(ThemeSource::Shipped(name, json));
    }
    in_folder.map(ThemeSource::File)
}

/// The user's themes folder, beside the workspace file.
pub fn themes_folder() -> Option<std::path::PathBuf> {
    Some(workspace_path()?.parent()?.join("themes"))
}

/// The `.json` names the user's themes folder holds, sorted — what the
/// Theme menu lists beside the shipped ones. An unreadable folder is an
/// empty list.
pub fn folder_themes() -> Vec<String> {
    let Some(dir) = themes_folder() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter_map(|f| f.strip_suffix(".json").map(str::to_string))
        .collect();
    out.sort();
    out
}

/// Where a `--theme <choice>` points: a path when it reads like one (a
/// separator or a `.json` tail), else a named theme in the user's themes
/// dir, beside the workspace file. `None` only when a bare name has no
/// config dir to live in. Since Phase 51 slice 5 `theme_source` is the
/// reader the window and `--theme-check` use; this stays for the file
/// half of its answer.
pub fn theme_path(choice: &str) -> Option<std::path::PathBuf> {
    if choice.contains('/') || choice.contains('\\') || choice.ends_with(".json") {
        return Some(std::path::PathBuf::from(choice));
    }
    let ws = workspace_path()?;
    Some(ws.parent()?.join("themes").join(format!("{choice}.json")))
}

// ---- the workspace layout ---------------------------------------------------
//
// The docks: pre-allotted, movable panel space — the arrangement half of
// the phase (`DECISIONS.md` → "DRAFT: Phase 12…", fork 2A). Every pane is
// a peer with exactly one home (assignment totality, pinned), each dock
// shows one visible pane of several behind a tab strip, and EVERY mutation
// of the arrangement is a `DockAction` through `Layout::apply` — the shell
// maps keys and clicks to actions and never edits the fields itself, which
// is what makes the arrangement testable headless here and persistable as
// text in slice 2. This lives beside the theme because both are the
// window's configuration — what it looks like, and how it is arranged —
// and slice 2's workspace file (fork 3A) renders and parses both from this
// module. Std-only, like everything above.

/// The sidebar's two views (DESIGN.md §5.2): the project outline and the
/// check's problems. One tab strip, one visible, and the sidebar is the
/// only wall they can live on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarTab {
    /// The project as five groups (DESIGN.md §5.2, amended 2026-10-17):
    /// Stock · Target · Features · Operations · Tools. The run order was
    /// its own view, the Sequence, from 2026-09-04 until Phase 52 slice 4
    /// folded it into the Operations group; `sidebar.tab sequence` in an
    /// old workspace file is a token this enum no longer knows and falls
    /// to the default, as `mode` did.
    Outline,
    Problems,
    Review,
}

impl SidebarTab {
    pub const ALL: [SidebarTab; 3] = [SidebarTab::Outline, SidebarTab::Problems, SidebarTab::Review];

    /// The tab's word, and the rail's.
    pub fn title(self) -> &'static str {
        match self {
            SidebarTab::Outline => "Outline",
            SidebarTab::Problems => "Problems",
            SidebarTab::Review => "Review",
        }
    }

    /// The workspace file's token.
    pub fn key(self) -> &'static str {
        match self {
            SidebarTab::Outline => "outline",
            SidebarTab::Problems => "problems",
            SidebarTab::Review => "review",
        }
    }

    pub fn from_key(key: &str) -> Option<SidebarTab> {
        SidebarTab::ALL.into_iter().find(|t| t.key() == key)
    }

    /// The act that shows this view in the sidebar (and opens it).
    pub const fn action_id(self) -> &'static str {
        match self {
            SidebarTab::Outline => "sidebar.outline",
            SidebarTab::Problems => "sidebar.problems",
            SidebarTab::Review => "sidebar.review",
        }
    }

    /// The hover's one sentence.
    pub fn sentence(self) -> &'static str {
        match self {
            SidebarTab::Outline => "the project as five groups — the stock, the target, the features, the operations in run order, the tools",
            SidebarTab::Problems => "the check's problems, a row per finding",
            SidebarTab::Review => "the import's report, a row per face — proposals first, then the residue by row and code",
        }
    }
}

/// The center's two documents (DESIGN.md §5.3): the viewport and the
/// emitted program. Something you look at for minutes; a tab strip over
/// the center, one showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Document {
    Viewport,
    Program,
}

impl Document {
    pub const ALL: [Document; 2] = [Document::Viewport, Document::Program];

    pub fn title(self) -> &'static str {
        match self {
            Document::Viewport => "Viewport",
            Document::Program => "Program",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Document::Viewport => "viewport",
            Document::Program => "program",
        }
    }

    pub fn from_key(key: &str) -> Option<Document> {
        Document::ALL.into_iter().find(|d| d.key() == key)
    }

    pub const fn action_id(self) -> &'static str {
        match self {
            Document::Viewport => "document.viewport",
            Document::Program => "document.program",
        }
    }

    pub fn sentence(self) -> &'static str {
        match self {
            Document::Viewport => "the part, its toolpaths and the viewport's layers",
            Document::Program => "the emitted program, the cursor's operation highlighted",
        }
    }
}

/// What the center shows (DESIGN.md §5.3, amended 2026-09-04): the two
/// documents side by side — the viewport left, the program right, the
/// default — or one of them alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Center {
    Split,
    Alone(Document),
}

impl Center {
    /// The workspace file's token: `both`, or the document's own.
    pub fn key(self) -> &'static str {
        match self {
            Center::Split => "both",
            Center::Alone(d) => d.key(),
        }
    }

    pub fn from_key(key: &str) -> Option<Center> {
        if key == "both" {
            return Some(Center::Split);
        }
        Document::from_key(key).map(Center::Alone)
    }

    /// The tab strip's word.
    pub fn title(self) -> &'static str {
        match self {
            Center::Split => "Both",
            Center::Alone(d) => d.title(),
        }
    }
}

/// The split's share of the center the viewport takes, and its bounds —
/// neither document can be dragged to a sliver.
pub const SPLIT_DEFAULT: f32 = 0.55;
pub const SPLIT_MIN: f32 = 0.15;
pub const SPLIT_MAX: f32 = 0.85;

fn clamp_split(ratio: f32) -> f32 {
    if ratio.is_finite() { ratio.clamp(SPLIT_MIN, SPLIT_MAX) } else { SPLIT_DEFAULT }
}

/// A viewport layer: something the picture can show or hide, each a
/// checkbox in the viewport's layers widget (DESIGN.md I9) — not a pane,
/// not a rail slot, not a status owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// The declared features' edges.
    Outlines,
    /// The emitted motion through the op cursor.
    Toolpaths,
    /// The declared removals' boundaries at the cursor (`nacs --outline`).
    DeclaredStock,
    /// The swept stock surface after the ops that have run.
    SimulatedStock,
    /// Forgotten and violated cells, the rest report's own scan (`nacs --rest-report`).
    RestMaterial,
    /// The fixture's keep-outs.
    Fixtures,
    /// The tool body at its position.
    Tool,
    /// The problems' annotations — a cross at a point, a line along a segment.
    Annotations,
    /// The immersion field over the selected floor — what the tool would
    /// meet before a path exists (`nacs --engagement <op>`; Phase 41 slice 2).
    Engagement,
}

impl Layer {
    pub const ALL: [Layer; 9] = [
        Layer::Outlines,
        Layer::Toolpaths,
        Layer::DeclaredStock,
        Layer::SimulatedStock,
        Layer::RestMaterial,
        Layer::Fixtures,
        Layer::Tool,
        Layer::Annotations,
        Layer::Engagement,
    ];

    /// The checkbox's word.
    pub fn label(self) -> &'static str {
        match self {
            Layer::Outlines => "Outlines",
            Layer::Toolpaths => "Toolpaths",
            Layer::DeclaredStock => "Removal boundaries",
            Layer::SimulatedStock => "Stock simulation",
            Layer::RestMaterial => "Uncut material",
            Layer::Fixtures => "Fixtures",
            Layer::Tool => "Tool",
            Layer::Annotations => "Annotations",
            Layer::Engagement => "Engagement",
        }
    }

    /// The workspace file's token.
    pub fn key(self) -> &'static str {
        match self {
            Layer::Outlines => "outlines",
            Layer::Toolpaths => "toolpaths",
            Layer::DeclaredStock => "removal_boundaries",
            Layer::SimulatedStock => "stock_simulation",
            Layer::RestMaterial => "uncut_material",
            Layer::Fixtures => "fixtures",
            Layer::Tool => "tool",
            Layer::Annotations => "annotations",
            Layer::Engagement => "engagement",
        }
    }

    pub fn from_key(key: &str) -> Option<Layer> {
        Layer::ALL.into_iter().find(|l| l.key() == key)
    }

    /// The act that toggles this layer — palette-reachable, keyless.
    pub const fn action_id(self) -> &'static str {
        match self {
            Layer::Outlines => "layer.outlines",
            Layer::Toolpaths => "layer.toolpaths",
            Layer::DeclaredStock => "layer.removal_boundaries",
            Layer::SimulatedStock => "layer.stock_simulation",
            Layer::RestMaterial => "layer.uncut_material",
            Layer::Fixtures => "layer.fixtures",
            Layer::Tool => "layer.tool",
            Layer::Annotations => "layer.annotations",
            Layer::Engagement => "layer.engagement",
        }
    }
}

/// Which layers are on — part of a mode's stored arrangement, so it is
/// saved in the workspace file's stanza and reverts with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layers {
    pub outlines: bool,
    pub toolpaths: bool,
    pub declared_stock: bool,
    pub simulated_stock: bool,
    pub rest_material: bool,
    pub fixtures: bool,
    pub tool: bool,
    pub annotations: bool,
    /// The immersion field (Phase 41 slice 2): off until asked for — a
    /// steering picture for the floor being worked, not the default view.
    pub engagement: bool,
}

impl Default for Layers {
    /// Today's picture: everything the viewport always drew is on; the two
    /// worker-built layers (the swept stock and the rest scan) are off
    /// until asked for, because each costs a thread — and the immersion
    /// field, which is asked for when a floor is being shaped.
    fn default() -> Self {
        Layers {
            outlines: true,
            toolpaths: true,
            declared_stock: true,
            simulated_stock: false,
            rest_material: false,
            fixtures: true,
            tool: true,
            annotations: true,
            engagement: false,
        }
    }
}

impl Layers {
    pub fn get(self, l: Layer) -> bool {
        match l {
            Layer::Outlines => self.outlines,
            Layer::Toolpaths => self.toolpaths,
            Layer::DeclaredStock => self.declared_stock,
            Layer::SimulatedStock => self.simulated_stock,
            Layer::RestMaterial => self.rest_material,
            Layer::Fixtures => self.fixtures,
            Layer::Tool => self.tool,
            Layer::Annotations => self.annotations,
            Layer::Engagement => self.engagement,
        }
    }

    pub fn set(&mut self, l: Layer, on: bool) {
        match l {
            Layer::Outlines => self.outlines = on,
            Layer::Toolpaths => self.toolpaths = on,
            Layer::DeclaredStock => self.declared_stock = on,
            Layer::SimulatedStock => self.simulated_stock = on,
            Layer::RestMaterial => self.rest_material = on,
            Layer::Fixtures => self.fixtures = on,
            Layer::Tool => self.tool = on,
            Layer::Annotations => self.annotations = on,
            Layer::Engagement => self.engagement = on,
        }
    }

    pub fn toggle(&mut self, l: Layer) {
        self.set(l, !self.get(l));
    }

    /// The layers that are on, in `Layer::ALL` order — the file's list.
    pub fn on(self) -> Vec<Layer> {
        Layer::ALL.into_iter().filter(|l| self.get(*l)).collect()
    }

    /// Exactly these on, everything else off — the file's list, read back.
    pub fn from_on(on: &[Layer]) -> Layers {
        let mut out = Layers { outlines: false, toolpaths: false, declared_stock: false, simulated_stock: false, rest_material: false, fixtures: false, tool: false, annotations: false, engagement: false };
        for l in on {
            out.set(*l, true);
        }
        out
    }
}

/// A wall's size is clamped so no act can wedge one over the whole window
/// or collapse it to an invisible sliver it cannot be dragged back from.
pub const DOCK_MIN_SIZE: f32 = 120.0;
pub const DOCK_MAX_SIZE: f32 = 1200.0;

fn clamp_size(size: f32) -> f32 {
    if size.is_finite() { size.clamp(DOCK_MIN_SIZE, DOCK_MAX_SIZE) } else { DOCK_MIN_SIZE }
}

/// The sidebar (DESIGN.md §5.2): one of its views showing, open or
/// collapsed, and its width.
#[derive(Clone, Debug, PartialEq)]
pub struct Sidebar {
    pub tab: SidebarTab,
    pub open: bool,
    pub width: f32,
}

/// The form's dock (§5.4, amended 2026-09-04): the left wall's LOWER dock,
/// beneath the sidebar's view, sharing the wall's width. No tabs — its
/// content is a function of the selection — so only whether it is open
/// and how tall it is. (It was the right dock, the inspector, until the
/// form became where the work is; the right wall is retired and its
/// width went to the center.)
#[derive(Clone, Debug, PartialEq)]
pub struct FormDock {
    pub open: bool,
    pub height: f32,
}

/// The bottom dock (§5.8, I1): reserved for a terminal that does not exist
/// yet. It has NO content field — an empty dock is unspellable as anything
/// else — and it is a struct all the same, so its height survives and the
/// terminal's arrival is not a re-layout.
#[derive(Clone, Debug, PartialEq)]
pub struct Bottom {
    pub open: bool,
    pub height: f32,
}

/// The window's arrangement — ONE (DESIGN.md §5.1, the modes retired
/// 2026-09-04): the sidebar, which document the center shows, the
/// form's dock, the bottom dock, and the viewport's layers. Every tool has
/// one fixed home (DESIGN.md §1): nothing here says WHERE a view lives,
/// only whether it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub sidebar: Sidebar,
    pub center: Center,
    /// The split's ratio — the viewport's share of the center's width
    /// when both documents show (§5.3); kept when one shows alone.
    pub split: f32,
    pub form: FormDock,
    pub bottom: Bottom,
    pub layers: Layers,
    /// The Program's two gutters (§5.3, 2026-09-17): line numbers on the
    /// left, the minimap on the right — each a toggle, both on by default.
    pub program_numbers: bool,
    pub program_minimap: bool,
    /// The Layers group held open (Phase 51 slice 2, DESIGN.md §5.5): the
    /// group unfolds while hovered and folds otherwise; a click on its
    /// header pins it open — the one remembered bit, `layers.pinned`.
    pub layers_pinned: bool,
    /// The frame the window wears (Phase 57 slice 1, fork 1A): its own —
    /// the band the title bar — unless the file says `frame native`, the
    /// one escape, toggled live by `window.frame`. Absent is own.
    pub frame_native: bool,
}

impl Default for Layout {
    /// The defaults, a literal row and nothing derived — what `0` returns
    /// to: the Outline facing you with the form beneath it, both documents
    /// in the center, the bottom dock shut. (The modes' three rows differed only
    /// in the sidebar view and the document; this is the row a fresh
    /// window opened in.)
    fn default() -> Self {
        Layout {
            sidebar: Sidebar { tab: SidebarTab::Outline, open: true, width: 320.0 },
            center: Center::Split,
            split: SPLIT_DEFAULT,
            form: FormDock { open: true, height: 360.0 },
            bottom: Bottom { open: false, height: 200.0 },
            layers: Layers::default(),
            program_numbers: true,
            program_minimap: true,
            layers_pinned: false,
            frame_native: false,
        }
    }
}

impl Layout {
    /// What the window opens on (DESIGN.md §5.2, the diagnose stance): the
    /// arrangement it was closed in — unless the project was REJECTED,
    /// which opens with Problems showing, its error the first row. One
    /// rule, at open, for one condition; since 2026-09-04 a rule about
    /// the view, not a mode. Nothing else moves.
    pub fn at_open(rejected: bool, kept: Layout) -> Layout {
        let mut l = kept;
        if rejected {
            l.apply(DockAction::ShowTab(SidebarTab::Problems));
        }
        l
    }
}

/// Every act the arrangement answers to. The shell TRANSLATES input into
/// these and `apply` is the only mutator. Nothing relocates a view (I4),
/// nothing splits a wall (I5), and every toggle does one thing (I12).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockAction {
    /// Show this view in the sidebar, and open the sidebar (a rail click,
    /// a tab click, the palette's act). Never a toggle.
    ShowTab(SidebarTab),
    ToggleSidebar,
    ResizeSidebar(f32),
    /// Show this document ALONE in the center.
    ShowDocument(Document),
    /// Show both documents side by side (§5.3, the default).
    ShowSplit,
    /// The split's ratio, the viewport's share — read back off the drawn
    /// rect like a wall's size.
    ResizeSplit(f32),
    ToggleForm,
    /// Open the form's dock if it is shut, and leave it open if it is —
    /// a creation act's door (§5.4): a form composed into a shut dock was
    /// live and invisible. Never a toggle.
    OpenForm,
    /// Shut the form's dock if it is open, and leave it shut if it is —
    /// a tab change's door out (§5.4, Phase 51 slice 3): a card the shown
    /// tab could not have made is stale. Never a toggle.
    ShutForm,
    ResizeForm(f32),
    ToggleBottom,
    ResizeBottom(f32),
    /// Show or hide a viewport layer.
    Layer(Layer, bool),
    /// Show or hide the Program's line numbers / its minimap (§5.3).
    ProgramNumbers(bool),
    ProgramMinimap(bool),
    /// Pin the Layers group open, or let it fold when the pointer leaves
    /// (§5.5, Phase 51 slice 2).
    LayersPinned(bool),
    /// Return the arrangement to its defaults — a visible escape from any
    /// layout experiment (`0`).
    Revert,
}

impl Layout {
    /// The one mutator.
    pub fn apply(&mut self, action: DockAction) {
        match action {
            DockAction::ShowTab(tab) => {
                self.sidebar.tab = tab;
                self.sidebar.open = true;
            }
            DockAction::ToggleSidebar => self.sidebar.open = !self.sidebar.open,
            DockAction::ResizeSidebar(w) => self.sidebar.width = clamp_size(w),
            DockAction::ShowDocument(d) => self.center = Center::Alone(d),
            DockAction::ShowSplit => self.center = Center::Split,
            DockAction::ResizeSplit(r) => self.split = clamp_split(r),
            DockAction::ToggleForm => self.form.open = !self.form.open,
            DockAction::OpenForm => self.form.open = true,
            DockAction::ShutForm => self.form.open = false,
            DockAction::ResizeForm(h) => self.form.height = clamp_size(h),
            DockAction::ToggleBottom => self.bottom.open = !self.bottom.open,
            DockAction::ResizeBottom(h) => self.bottom.height = clamp_size(h),
            DockAction::Layer(layer, on) => self.layers.set(layer, on),
            DockAction::ProgramNumbers(on) => self.program_numbers = on,
            DockAction::LayersPinned(on) => self.layers_pinned = on,
            DockAction::ProgramMinimap(on) => self.program_minimap = on,
            DockAction::Revert => *self = Layout::default(),
        }
    }

    /// The sidebar view on screen, if the sidebar is open — what the rail
    /// lights.
    pub fn shown_tab(&self) -> Option<SidebarTab> {
        self.sidebar.open.then_some(self.sidebar.tab)
    }
}

// ---- the workspace file -----------------------------------------------------
//
// Where the arrangement survives the window: ONE hand-written key-value
// text file in the user config dir, chrome only — which sidebar view
// faces you, which document the center shows, which walls are open and how
// big, which layers are on; never the cursor, the camera, or the selection
// (those are about the part and die with the window). One file for all
// projects: the layout is the user's room, not the part's. Written on
// close through a temp file and an atomic rename. Read at open; a missing
// file is the default; and EVERY unreadable or unknown line falls silently
// to the default already in place, so a mangled file can dent the layout
// but never block the window. One arrangement: the modes era (2026-08-26
// to 2026-09-04) wrote three stanzas and a `mode` line, and such a file
// still opens — its unprefixed (author) stanza is the layout, the
// `diagnose.`/`verify.` lines and the `mode` line drop as unknown.

const WORKSPACE_HEADER: &str = "# nacs workspace — the window's arrangement, chrome only.\n\
                                # Hand-editable; unknown or unreadable lines fall to defaults.\n";

/// The keys older files wrote, mapped onto the keys the file writes now —
/// one table, so a file from any earlier version of the window still opens
/// on the room it meant. A key not in this table and not current is
/// unknown, and drops.
const LEGACY_KEYS: &[(&str, &str)] = &[
    ("left.open", "sidebar.open"),
    ("left.size", "sidebar.width"),
    // The right wall's open flag is the form's (the form moved to the
    // left wall 2026-09-04); its width went to the center, so `right.size`
    // and `inspector.width` are unknown now and drop.
    ("right.open", "form.open"),
    ("inspector.open", "form.open"),
    ("bottom.size", "bottom.height"),
];

impl Layout {
    /// The arrangement as the file's text — one line per fact, diffable,
    /// hand-editable, in a stated order so two saves of the same state are
    /// byte-identical.
    pub fn render(&self) -> String {
        let mut out = String::from(WORKSPACE_HEADER);
        out.push_str(&format!("sidebar.tab {}\n", self.sidebar.tab.key()));
        out.push_str(&format!("sidebar.open {}\n", self.sidebar.open));
        // `{}` on an f32 round-trips exactly through `parse::<f32>`, so a
        // save and a load cannot drift a size.
        out.push_str(&format!("sidebar.width {}\n", self.sidebar.width));
        out.push_str(&format!("center {}\n", self.center.key()));
        out.push_str(&format!("center.ratio {}\n", self.split));
        out.push_str(&format!("form.open {}\n", self.form.open));
        out.push_str(&format!("form.height {}\n", self.form.height));
        out.push_str(&format!("bottom.open {}\n", self.bottom.open));
        out.push_str(&format!("bottom.height {}\n", self.bottom.height));
        if self.frame_native {
            out.push_str("frame native\n");
        }
        // Additive: the default layer set writes nothing, so every file
        // written before layers existed still loads and still means the
        // same thing.
        if self.layers != Layers::default() {
            let on: Vec<&str> = self.layers.on().iter().map(|l| l.key()).collect();
            out.push_str(&format!("layers {}\n", on.join(",")));
        }
        // Additive likewise: on by default, written only when off.
        if !self.program_numbers {
            out.push_str("program.numbers false\n");
        }
        if !self.program_minimap {
            out.push_str("program.minimap false\n");
        }
        // Additive likewise: unpinned by default, written only when pinned.
        if self.layers_pinned {
            out.push_str("layers.pinned true\n");
        }
        out
    }

    /// The file's text back to an arrangement. Starts from the defaults
    /// and patches what parses, line by line; a legacy key is read through
    /// `LEGACY_KEYS` (and the two legacy lines with a VALUE to translate —
    /// `left.active` naming a pane, `right.active gcode` — by hand);
    /// everything else is unknown and dropped — which is how a modes-era
    /// file's `diagnose.sidebar.tab` and `mode verify` lines fall away
    /// and its unprefixed stanza wins.
    pub fn parse(text: &str) -> Layout {
        let mut l = Layout::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once(' ') else { continue };
            let value = value.trim();
            let key = LEGACY_KEYS.iter().find(|(old, _)| *old == key).map_or(key, |(_, new)| *new);
            match key {
                "sidebar.tab" => {
                    if let Some(t) = SidebarTab::from_key(value) {
                        l.sidebar.tab = t;
                    }
                }
                // The era of panes: `left.active project|findings` named the
                // sidebar's view by the pane's old token.
                "left.active" => match value {
                    "project" => l.sidebar.tab = SidebarTab::Outline,
                    "findings" => l.sidebar.tab = SidebarTab::Problems,
                    _ => {}
                },
                "sidebar.open" => set_bool(&mut l.sidebar.open, value),
                "sidebar.width" => set_size(&mut l.sidebar.width, value),
                "center" => {
                    if let Some(c) = Center::from_key(value) {
                        l.center = c;
                    }
                }
                "center.ratio" => {
                    if let Ok(v) = value.parse::<f32>() {
                        if v.is_finite() {
                            l.split = clamp_split(v);
                        }
                    }
                }
                // The program on the right wall's tab, in the era before
                // it was a document.
                "right.active" if value == "gcode" => l.center = Center::Alone(Document::Program),
                "form.open" => set_bool(&mut l.form.open, value),
                "form.height" => set_size(&mut l.form.height, value),
                "bottom.open" => set_bool(&mut l.bottom.open, value),
                "bottom.height" => set_size(&mut l.bottom.height, value),
                "layers" => {
                    let on: Vec<Layer> = value.split(',').filter_map(|t| Layer::from_key(t.trim())).collect();
                    l.layers = Layers::from_on(&on);
                }
                "program.numbers" => set_bool(&mut l.program_numbers, value),
                "program.minimap" => set_bool(&mut l.program_minimap, value),
                "layers.pinned" => set_bool(&mut l.layers_pinned, value),
                "frame" => l.frame_native = value == "native",
                _ => {}
            }
        }
        l
    }
}

fn set_bool(slot: &mut bool, value: &str) {
    match value {
        "true" => *slot = true,
        "false" => *slot = false,
        _ => {}
    }
}

fn set_size(slot: &mut f32, value: &str) {
    if let Ok(v) = value.parse::<f32>() {
        if v.is_finite() {
            *slot = clamp_size(v);
        }
    }
}

/// Where the workspace file lives: the user config dir, never the project
/// (a churning chrome file inside a directory whose pitch is reviewable
/// diffs is noise in every review — fork 3C, rejected). `None` when the
/// environment names no home at all, in which case the window simply
/// opens at the default and saves nowhere.
pub fn workspace_path() -> Option<std::path::PathBuf> {
    #[cfg(feature = "shell-tests")]
    if let Some(p) = WORKSPACE_FOR_THIS_THREAD.with(|o| o.borrow().clone()) {
        return Some(p);
    }
    let base = if cfg!(windows) {
        std::path::PathBuf::from(std::env::var_os("APPDATA")?)
    } else {
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(x) if !x.is_empty() => std::path::PathBuf::from(x),
            _ => std::path::PathBuf::from(std::env::var_os("HOME")?).join(".config"),
        }
    };
    Some(base.join("nacs").join("workspace"))
}

// The shell tests' seam (Phase 59 slice 4, the lock's finding): one
// config home per PROCESS could not keep two tests' saves apart — a
// pick's `theme` line was read back after another test's save had
// rewritten the one file, twice under the lock — so a test thread names
// its own workspace file, and every reader and writer goes through
// `workspace_path` as before. The harness drives an app on the test's
// thread, which is why a thread-local is the right scope. Behind the
// `shell-tests` feature: no shipped binary carries it.
#[cfg(feature = "shell-tests")]
thread_local! {
    static WORKSPACE_FOR_THIS_THREAD: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Name this thread's workspace file (the shell tests only; see above).
#[cfg(feature = "shell-tests")]
pub fn workspace_for_this_thread(path: std::path::PathBuf) {
    WORKSPACE_FOR_THIS_THREAD.with(|o| *o.borrow_mut() = Some(path));
}

/// Write the whole workspace for the next open: temp file beside the
/// target, then an atomic rename — two instances racing at close settle
/// by last rename, and locking is never load-bearing. A failure names its
/// reason on `Err`; the caller says it once on stderr and moves on
/// (chrome must never block closing).
pub fn save_workspace(ws: &Workspace) -> Result<(), String> {
    let Some(path) = workspace_path() else {
        return Err("no config dir in this environment (HOME/APPDATA unset)".to_string());
    };
    let dir = path.parent().expect("workspace path has a dir");
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    write_atomically(&path, &ws.render())
}

/// The one way this program replaces a file it did not create: the whole
/// text to a temp file beside the target, then an atomic rename. Two
/// writers racing settle by last rename and locking is never load-bearing
/// (the Phase 12 multi-instance rule); the editing workspace's writes to
/// the project's own files go through here too (`DESIGN.md` §1).
pub fn write_atomically(path: &std::path::Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{} has no directory", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dir.join(format!("{name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename to {}: {e}", path.display()))
}

/// Everything the workspace file holds: the arrangements, the active
/// theme's name, and the custom key lines (`key <action-id> <key>` — RAW
/// strings here; the action inventory that judges them lives in `view`,
/// which this module must not import). All chrome, all under the
/// fall-to-default rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub layout: Layout,
    pub theme: Option<String>,
    pub keys: Vec<(String, String)>,
    /// The window's text zoom (egui's zoom factor, Ctrl +/−), `ui.zoom`
    /// (2026-09-17): chrome, so it lives here; `None` is 1.0 unwritten.
    pub zoom: Option<f32>,
    /// The project the window opened last (`project <path>`, Phase 50
    /// slice 3): what `nacs --view` with no path reopens. Written at open
    /// and at close, so a crash still remembers.
    pub project: Option<String>,
    /// The shell the runner's `sh` verb hands a line to (`shell
    /// <program>`, Phase 59 slice 4, fork 4A): run as `<program> -c
    /// <line>` (`/C` when the program is cmd). Absent is the system's —
    /// `COMSPEC` on Windows, `/bin/sh` elsewhere. Declared, never guessed
    /// from `$SHELL` or PATH.
    pub shell: Option<String>,
    /// The options page's answers (`orbit`, `orbit.about`): how a drag
    /// turns the view. A default writes no line.
    pub options: Options,
}

impl Default for Workspace {
    fn default() -> Self {
        Self { layout: Layout::default(), theme: None, keys: Vec::new(), zoom: None, project: None, shell: None, options: Options::default() }
    }
}

/// How a drag turns the view. Both turn about world Z for a sideways drag
/// and tilt for an up-and-down one; they differ at the poles.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Orbit {
    /// The tilt goes on over the top: the part can be seen upside down.
    #[default]
    Free,
    /// The tilt stops at straight down and straight up.
    Turntable,
}

/// The point a drag turns the view about: it stays where it is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Pivot {
    #[default]
    Part,
    View,
    Cursor,
}

/// What the options page holds. One line a field in the workspace file.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Options {
    pub orbit: Orbit,
    pub pivot: Pivot,
}

/// One option as the page, `nacs --options` and the workspace file spell
/// it: its key, its title, and each choice as (word, label, sentence, on).
pub struct OptionRow {
    pub key: &'static str,
    pub title: &'static str,
    pub choices: Vec<(&'static str, &'static str, &'static str, bool)>,
}

impl Options {
    /// Every option with its choices, the chosen one marked. The one table:
    /// the page draws it, the CLI prints it, the file is read through it.
    pub fn rows(&self) -> Vec<OptionRow> {
        vec![
            OptionRow {
                key: "orbit",
                title: "Orbit style",
                choices: vec![
                    ("free", "Free turntable", "a sideways drag turns about Z, an up-and-down drag tilts, and the tilt goes on over the top", self.orbit == Orbit::Free),
                    ("turntable", "Turntable", "the same, but the tilt stops at straight down and straight up", self.orbit == Orbit::Turntable),
                ],
            },
            OptionRow {
                key: "orbit.about",
                title: "Rotate about",
                choices: vec![
                    ("part", "Part center", "the center of the part and its stock", self.pivot == Pivot::Part),
                    ("view", "View center", "whatever is at the middle of the viewport", self.pivot == Pivot::View),
                    ("cursor", "Cursor", "the point the drag began on", self.pivot == Pivot::Cursor),
                ],
            },
        ]
    }

    /// Choose by the file's words. An unknown key or word changes nothing
    /// and answers false.
    pub fn set(&mut self, key: &str, word: &str) -> bool {
        match (key, word) {
            ("orbit", "free") => self.orbit = Orbit::Free,
            ("orbit", "turntable") => self.orbit = Orbit::Turntable,
            ("orbit.about", "part") => self.pivot = Pivot::Part,
            ("orbit.about", "view") => self.pivot = Pivot::View,
            ("orbit.about", "cursor") => self.pivot = Pivot::Cursor,
            _ => return false,
        }
        true
    }

    /// What `nacs --options` prints: each option, its choices, `*` on the
    /// chosen one.
    pub fn report(&self) -> String {
        let mut out = String::new();
        for row in self.rows() {
            out.push_str(&format!("{} ({})\n", row.title, row.key));
            for (word, label, says, on) in row.choices {
                out.push_str(&format!("  {} {word:<10} {label}: {says}\n", if on { '*' } else { ' ' }));
            }
        }
        out
    }
}

/// The zoom factors the file accepts — egui's own band, so a hand-typed
/// `ui.zoom 40` cannot open a window nobody can read.
pub const ZOOM_MIN: f32 = 0.5;
pub const ZOOM_MAX: f32 = 4.0;

impl Workspace {
    /// The whole file: the arrangement, then the theme and the key lines.
    pub fn render(&self) -> String {
        let mut out = self.layout.render();
        if let Some(t) = &self.theme {
            out.push_str(&format!("theme {t}\n"));
        }
        if let Some(p) = &self.project {
            out.push_str(&format!("project {p}\n"));
        }
        if let Some(sh) = &self.shell {
            out.push_str(&format!("shell {sh}\n"));
        }
        for (id, key) in &self.keys {
            out.push_str(&format!("key {id} {key}\n"));
        }
        if let Some(z) = self.zoom {
            out.push_str(&format!("ui.zoom {z}\n"));
        }
        // Only what differs from the defaults is written.
        let unset = Options::default().rows();
        for (row, base) in self.options.rows().iter().zip(&unset) {
            if let Some((word, ..)) = row.choices.iter().zip(&base.choices).find(|(c, b)| c.3 && !b.3).map(|(c, _)| c) {
                out.push_str(&format!("{} {word}\n", row.key));
            }
        }
        out
    }

    /// The file back: the layout through its own parser, the extras
    /// collected as written — a malformed extra line (a `key` with no
    /// second word) is dropped like any other unreadable line.
    pub fn parse(text: &str) -> Workspace {
        let mut ws = Workspace { layout: Layout::parse(text), ..Workspace::default() };
        for line in text.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("theme ") {
                ws.theme = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("project ") {
                ws.project = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("shell ") {
                ws.shell = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("key ") {
                let mut it = v.split_whitespace();
                if let (Some(id), Some(key)) = (it.next(), it.next()) {
                    ws.keys.push((id.to_string(), key.to_string()));
                }
            } else if let Some(v) = line.strip_prefix("ui.zoom ") {
                if let Ok(z) = v.trim().parse::<f32>() {
                    if z.is_finite() {
                        ws.zoom = Some(z.clamp(ZOOM_MIN, ZOOM_MAX));
                    }
                }
            }
            else if let Some((key, word)) = line.split_once(' ') {
                ws.options.set(key, word.trim());
            }
            // A `toolbar` line is a line older files wrote for a surface
            // that no longer exists: unknown, and dropped.
        }
        ws
    }
}

/// Read the whole workspace — a missing or unreadable file is the
/// default, never an error.
pub fn load_full_workspace() -> Workspace {
    match workspace_path().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => Workspace::parse(&text),
        None => Workspace::default(),
    }
}
