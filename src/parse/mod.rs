// Part model and the new declarative syntax parser.
// Plus derived value functions (see AGENTS.md "DRY for derived values").
//
// One module split by topic; every item is re-exported here so `parse::X`
// stays the one name for it. A private item is `pub(super)` so the siblings
// can call it and the rest of the crate still cannot.

mod tool;
mod model;
mod grammar;
mod findings;
mod validate;
mod feeds;
mod derive;
mod surface;
mod revolve;
mod svg;

pub use tool::*;
pub use model::*;
pub use grammar::*;
pub use findings::*;
pub use validate::*;
pub use feeds::*;
pub use derive::*;
pub use surface::*;
pub use revolve::*;
use svg::*; // nothing public: the form-tool reader serves the grammar alone

#[cfg(test)]
mod tests;
