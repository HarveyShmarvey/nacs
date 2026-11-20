// ---- drill: hole drilling using unrolled G0/G1 moves -------------------------
// Supports Simple drilling (single plunge) and Peck drilling (incremental pecks 
// with retracts to rapid_z for chip clearing, returning with a safety entry clearance).

use crate::gcode::f;
use crate::parse::{derive_contour_top, derive_csink_tip_z, derive_z_cut, CycleType, Feature, Operation, Part};

pub fn drill(p: &Part, op: &Operation) -> Vec<String> {
    let mut g = Vec::new();

    let drill_points = op.drill_points.as_ref().expect("a visit to points names them");
    let cycle = op.cycle.expect("a visit to points names its cycle");
    // A ChamferMill turns the cycle into the mouth-cone arm (DECISIONS.md →
    // "spot drilling and countersinking…"): same motion, but the plunge Z
    // derives from the hole's declared seat through the ground cone rather
    // than from the hole's own extent. validate() guarantees the pairing.
    let tool = p
        .tools
        .get(&op.tool_num)
        .expect("validate: the op's tool exists");
    let cone = tool.ground_cone();
    // An Endmill turns a cbore-declaring point into the seat arm (DECISIONS.md
    // → "the counterbore…"): the same plunge-and-retract, to the DECLARED
    // depth — a cylinder has no apex to derive through — and validate() has
    // already held the seat's dia to the tool's own. The plunge presumes a
    // center-cutting endmill, which the tool record cannot yet say (named in
    // the entry; the machinist's discipline until it can).
    let is_endmill = tool.name == "Endmill";

    let plunge = op.plunge.unwrap_or(p.global_plunge);
    let rapid_z = op.rapid_z.unwrap_or(p.global_rapid_z);
    let clearance_z = op.clearance_z.unwrap_or(p.global_clearance_z);

    // Header comment lists the features being drilled — with the declared
    // seat named on the arm that cuts one, so the operator can check the
    // plunge Z on the controller against the print's callout.
    let listed = drill_points
        .iter()
        .map(|name| match (&cone, p.features.get(name.as_str())) {
            (Some(_), Some(Feature::CylinderBore { csink: Some(spec), .. })) => {
                format!("{} ({} {})", name, spec.job(), f(spec.seat_dia()))
            }
            (None, Some(Feature::CylinderBore { cbore: Some(cb), .. })) if is_endmill => {
                format!("{} (cbore {})", name, f(cb.dia))
            }
            _ => name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    // The title names the contact and the motion: `form points`, `end points`.
    g.push(format!("(--- {} points: {} with T{} ---)", op.op_type.verb().to_lowercase(), listed, op.tool_num));

    // First motion must be a pure Z retract (safety lint)
    g.push(format!("G00 Z{}", f(clearance_z)));

    for pt_name in drill_points {
        let feature = p.features.get(pt_name).expect("referenced feature must exist");
        // The bore's vertical field is `extent` — its occupancy as a swept
        // circle (DECISIONS.md "The `extent` vocabulary is scoped to the
        // extrusion features…"). Drill is the one consumer that spends it as a
        // *travel* rather than reading a wall off it, so the local keeps the
        // travel word; the two coincide because the hole's occupancy is exactly
        // how far down the tip goes.
        let (hx, hy, h_depth, rim, spec, cbore) = match feature {
            Feature::CylinderBore { dia: _, extent, x, y, z, csink, cbore, .. } => {
                // The rim the hole is authored down from (Phase 45): the
                // stock top unless `z` says otherwise, through the one home.
                (*x, *y, *extent, derive_contour_top(p.stock.z, *z), csink, cbore)
            }
            _ => panic!("Expected CylinderBore feature"),
        };
        let z_cut = match (&cone, cbore) {
            (Some(cone), _) => {
                let spec = spec.as_ref().expect("validate: a ChamferMill point declares its seat");
                derive_csink_tip_z(rim, spec.seat_dia(), cone)
            }
            // The seat's floor is its own vertical occupancy from the rim —
            // the same one home every extent reader spends.
            (None, Some(cb)) if is_endmill => derive_z_cut(rim, cb.depth),
            (None, _) => derive_z_cut(rim, h_depth),
        };
        
        // 1. Rapid to coordinate (hx, hy) at clearance_z
        g.push(format!("G00 X{} Y{}", f(hx), f(hy)));
        
        // 2. Rapid down to rapid_z
        g.push(format!("G00 Z{}", f(rapid_z)));
        
        // 3. Drill cycle
        match cycle {
            CycleType::Simple => {
                // Plunge down to final depth
                g.push(format!("G01 Z{} F{}", f(z_cut), f(plunge)));
                // Rapid retract to clearance_z
                g.push(format!("G00 Z{}", f(clearance_z)));
            }
            CycleType::Peck => {
                let peck_increment = op.peck_increment.expect("Peck increment must be defined for Peck cycle");
                // The pecks are counted from the RIM, not the stock top: a
                // hole on a step starts where its material starts.
                let mut current_peck_z = rim;
                let mut peck_idx = 0;
                
                while current_peck_z > z_cut {
                    let next_peck_z = (current_peck_z - peck_increment).max(z_cut);
                    
                    if peck_idx > 0 {
                        // Rapid down to previous peck depth + 0.5mm entry safety clearance
                        let entry_z = (current_peck_z + 0.5).min(rapid_z);
                        g.push(format!("G00 Z{}", f(entry_z)));
                    }
                    
                    // Plunge/feed to next peck depth
                    g.push(format!("G01 Z{} F{}", f(next_peck_z), f(plunge)));
                    
                    // Rapid retract to rapid_z to clear chips
                    g.push(format!("G00 Z{}", f(rapid_z)));
                    
                    current_peck_z = next_peck_z;
                    peck_idx += 1;
                }
                
                // Final retract to clearance_z
                g.push(format!("G00 Z{}", f(clearance_z)));
            }
        }
    }
    
    g
}
