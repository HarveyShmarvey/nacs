pub mod math;
pub mod tie;
pub mod parse;
pub mod gcode;
pub mod ops;
pub mod offset;
pub mod mesh;
pub mod nurbs;
pub mod outline;
pub mod region;
pub mod pattern;
pub mod spine;
pub mod compare;
pub mod cover;
pub mod rest;
pub mod stock;
pub mod template;
pub mod schedule;
pub mod theme;
pub mod view;
pub mod check_result;
pub mod cut_time;
#[cfg(feature = "frontend")]
pub mod frontend;
// The sidecar-report reader and the review wizard (Phase 17, lane A): a JSON
// consumer, so it lives behind the features that already own `serde_json`
// and the lean build never sees it.
#[cfg(any(feature = "frontend", feature = "agent"))]
pub mod review;
pub mod vox;
pub mod assembly;
pub mod engagement;
pub mod fixture;
pub mod construct;

/// The single geometric tolerance (DECISIONS.md "One geometric tolerance, one
/// home"): the maximum chord/approximation error, in mm, when a curve is
/// flattened to straight segments. First real consumer is ball-nose waterline
/// finishing (`ops::finish`), which flattens each circular pass to this bound.
/// (Offsetting still rides `cavalier_contours`' own default for now — a known
/// gap; when that is made configurable it should read this same constant.)
pub const CHORD_TOL_MM: f64 = 0.01;

