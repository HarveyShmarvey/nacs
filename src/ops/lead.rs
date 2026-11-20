//! The lead's shape (ROADMAP item 2, slice 5). A lead is the descent made in
//! the air beside a pass and the flight from there onto the pass's start,
//! flat or sloping. `lead=` says how far from the start the descent is made,
//! `lead_angle=` which way from the start it lies — 0 behind the start on the
//! pass's own line, 90 off to the air side, oblique between — and
//! `lead_descent=` how steeply the flight comes down, 0 flat. One home, read
//! by the wall traces, the rings and the offset family's side entries.
use crate::math::Portable as _;
use crate::gcode::f;
use crate::parse::{Operation, Pt};

pub(crate) struct LeadShape {
    pub len: f64,
    pub angle_deg: f64,
    pub descent_deg: f64,
    /// Any of the three keys said. The lead must then be whole and in air —
    /// never shortened to the room there is — or the op refuses.
    pub said: bool,
}

impl LeadShape {
    /// The op's lead: a diameter long, off to the air side, flat, where
    /// nothing is said.
    pub fn of(op: &Operation, dia: f64) -> LeadShape {
        LeadShape {
            len: op.lead.unwrap_or(dia),
            angle_deg: op.lead_angle.unwrap_or(90.0),
            descent_deg: op.lead_descent.unwrap_or(0.0),
            said: op.lead.is_some() || op.lead_angle.is_some() || op.lead_descent.is_some(),
        }
    }

    /// The lead point for a pass that leaves `start` along `tangent`, the air
    /// on the `air` side; both unit vectors.
    pub fn point(&self, start: Pt, tangent: (f64, f64), air: (f64, f64)) -> Pt {
        let a = self.angle_deg.to_radians();
        let (dx, dy) = (-tangent.0 * a.pcos() + air.0 * a.psin(), -tangent.1 * a.pcos() + air.1 * a.psin());
        let n = dx.phypot(dy).max(1e-9);
        Pt { x: start.x + self.len * dx / n, y: start.y + self.len * dy / n }
    }

    /// How far above the cut the plunge stops; the flight descends the rest.
    pub fn rise(&self) -> f64 {
        self.len * self.descent_deg.to_radians().ptan()
    }

    /// The Z word the flight onto the start carries where it descends; empty
    /// where the flight is flat, so an unsaid program keeps its bytes.
    pub fn z_word(&self, z_cut: f64) -> String {
        if self.descent_deg > 0.0 { format!(" Z{}", f(z_cut)) } else { String::new() }
    }

    /// The words after "(lead: ": how long, along what line, how steep.
    /// `air` names the air side's line ("the path's normal"), `back` the
    /// pass's own line behind its start.
    pub fn words(&self, air: &str, back: &str) -> String {
        let line = if self.angle_deg == 90.0 {
            air.to_string()
        } else if self.angle_deg == 0.0 {
            back.to_string()
        } else {
            format!("a line {}deg off {back}, toward the air", f(self.angle_deg))
        };
        let down = if self.descent_deg > 0.0 { format!(", descending {}deg", f(self.descent_deg)) } else { String::new() };
        format!("{}mm along {line}{down}", f(self.len))
    }

    /// The refusal where an op makes no lead for the keys to shape.
    pub fn nothing_to_shape(op: &Operation, why: &str) -> String {
        format!("lead keys on '{}': {} — lead=, lead_angle= and lead_descent= shape a lead, and this op makes none", op.feature_name, why)
    }
}
