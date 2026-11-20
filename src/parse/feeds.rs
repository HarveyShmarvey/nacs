// The `.feeds` table: the shipped starter, the grammar, and the rpm/feed derivations.

// ---- the feeds table (K3, the material) ------------------------------------
// DECISIONS.md → "K3, the material — the stock says what it is, every number
// says where it came from, and a missing row rejects by name". The `.feeds`
// grammar is the `.catalog` family (assembly::parse_catalog): line-oriented,
// `#` comments, `feeds <material> <tool-type>` opens a record, fields one per
// line, the record closes at the next opener or EOF. Two value columns only —
// surface speed and chip load by diameter band — because every fancier column
// (Kc, coating factors, depth factors) has a named consumer that is not
// built. `source` is REQUIRED per record: the trust state of a number is a
// fact about that number, declared, never assumed.

/// The shipped starter table — a reviewed, diffable file compiled in so the
/// binary always has it. Deliberately small and conservative (fork 3B); a
/// shop table declared via `feeds_table` shadows its records whole.
pub const STARTER_FEEDS: &str = include_str!("../../tables/starter.feeds");

/// One chip-load band: covers `dia_lo <= dia < dia_hi` (half-open, so
/// touching bands compose without ambiguity). Bands are explicit — no
/// interpolation, no extrapolation; a between-band diameter is a miss.
#[derive(Clone, Debug, PartialEq)]
pub struct ChiploadBand {
    pub dia_lo: f64,
    pub dia_hi: f64,
    /// mm per tooth. The tooth count that turns this into a feed is the
    /// TOOL's fact (K2's flute field), never stored here.
    pub mm_per_tooth: f64,
}

/// One feeds record: what one (workpiece material × tool type) pair runs at.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedsRecord {
    pub material: String,
    pub tool_type: String,
    /// Cutting surface speed, m/min (converted at parse if the file says
    /// `units inch`, where it reads as SFM).
    pub surface_speed: f64,
    /// Where these numbers came from — required, free text. A record that
    /// cannot say rejects to parse.
    pub source: String,
    pub chiploads: Vec<ChiploadBand>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedsTable {
    pub records: Vec<FeedsRecord>,
}

impl FeedsTable {
    /// Exact-match lookup on the (material × tool type) key. Names match
    /// exactly (fork 2) — no aliasing, no case folding.
    pub fn record(&self, material: &str, tool_type: &str) -> Option<&FeedsRecord> {
        self.records.iter().find(|r| r.material == material && r.tool_type == tool_type)
    }
}

/// The `.feeds` grammar. Same error discipline as `assembly::parse_catalog`:
/// duplicate records reject, fields before an opener reject, unknown keywords
/// reject with their line number — plus this file's own laws: `source` is
/// required, chip-load bands must not overlap, and an optional `units
/// mm|inch` header (before any record; default mm) converts ONCE at this
/// boundary — SFM→m/min, in/tooth→mm/tooth, band inches→mm — so everything
/// downstream is metric, always.
pub fn parse_feeds(text: &str) -> Result<FeedsTable, String> {
    struct Open {
        material: String,
        tool_type: String,
        lineno: usize,
        surface_speed: Option<(f64, usize)>,
        source: Option<String>,
        chiploads: Vec<(ChiploadBand, usize)>,
    }

    let mut table = FeedsTable::default();
    let mut open: Option<Open> = None;
    let mut units_mm: Option<bool> = None;

    let close = |open: Option<Open>, table: &mut FeedsTable| -> Result<(), String> {
        let Some(rec) = open else { return Ok(()) };
        let key = format!("{}\u{d7}{}", rec.material, rec.tool_type);
        let (surface_speed, _) = rec.surface_speed.ok_or_else(|| {
            format!("line {}: feeds record {} is missing a 'surface_speed' line", rec.lineno + 1, key)
        })?;
        let source = rec.source.ok_or_else(|| {
            format!(
                "line {}: feeds record {} declares no source — every number says where it came from; a record that cannot say rejects",
                rec.lineno + 1, key
            )
        })?;
        table.records.push(FeedsRecord {
            material: rec.material,
            tool_type: rec.tool_type,
            surface_speed,
            source,
            chiploads: rec.chiploads.into_iter().map(|(b, _)| b).collect(),
        });
        Ok(())
    };

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let mut tok = line.split_whitespace();
        let key = tok.next().unwrap();
        match key {
            "units" => {
                if open.is_some() || !table.records.is_empty() {
                    return Err(format!(
                        "line {}: 'units' is a header line — it goes before any 'feeds' record so no number is read under two unit systems",
                        lineno + 1
                    ));
                }
                if units_mm.is_some() {
                    return Err(format!("line {}: duplicate 'units' line", lineno + 1));
                }
                match tok.next() {
                    Some("mm") => units_mm = Some(true),
                    Some("inch") => units_mm = Some(false),
                    other => {
                        return Err(format!(
                            "line {}: unknown units '{}' (expected 'mm' or 'inch')",
                            lineno + 1, other.unwrap_or("")
                        ));
                    }
                }
            }
            "feeds" => {
                let material = tok.next().ok_or_else(|| {
                    format!("line {}: 'feeds' needs a material and a tool type", lineno + 1)
                })?;
                let tool_type = tok.next().ok_or_else(|| {
                    format!("line {}: 'feeds {}' needs a tool type", lineno + 1, material)
                })?;
                if tok.next().is_some() {
                    return Err(format!(
                        "line {}: 'feeds' takes exactly two tokens — a material name and a tool type (no spaces in either)",
                        lineno + 1
                    ));
                }
                close(open.take(), &mut table)?;
                if table.records.iter().any(|r| r.material == material && r.tool_type == tool_type) {
                    return Err(format!(
                        "line {}: duplicate feeds record {}\u{d7}{}",
                        lineno + 1, material, tool_type
                    ));
                }
                open = Some(Open {
                    material: material.to_string(),
                    tool_type: tool_type.to_string(),
                    lineno,
                    surface_speed: None,
                    source: None,
                    chiploads: Vec::new(),
                });
            }
            "surface_speed" | "source" | "chipload" => {
                let Some(rec) = open.as_mut() else {
                    return Err(format!(
                        "line {}: field '{}' before any 'feeds <material> <tool-type>' opener",
                        lineno + 1, key
                    ));
                };
                match key {
                    "surface_speed" => {
                        if rec.surface_speed.is_some() {
                            return Err(format!("line {}: duplicate 'surface_speed'", lineno + 1));
                        }
                        let val = tok.next().ok_or_else(|| {
                            format!("line {}: 'surface_speed' needs a value", lineno + 1)
                        })?;
                        if tok.next().is_some() {
                            return Err(format!("line {}: 'surface_speed' takes one value", lineno + 1));
                        }
                        let v: f64 = val.parse().map_err(|_| {
                            format!("line {}: surface_speed '{}' is not a number", lineno + 1, val)
                        })?;
                        if v <= 0.0 {
                            return Err(format!("line {}: surface_speed ({}) must be positive", lineno + 1, v));
                        }
                        rec.surface_speed = Some((v, lineno));
                    }
                    "source" => {
                        if rec.source.is_some() {
                            return Err(format!("line {}: duplicate 'source'", lineno + 1));
                        }
                        // Free text: everything after the keyword, comments
                        // already stripped.
                        let text = line["source".len()..].trim();
                        if text.is_empty() {
                            return Err(format!(
                                "line {}: 'source' needs text — every number says where it came from",
                                lineno + 1
                            ));
                        }
                        rec.source = Some(text.to_string());
                    }
                    _ => {
                        let band = tok.next().ok_or_else(|| {
                            format!("line {}: 'chipload' needs a band (<dia-lo>..<dia-hi>) and a value", lineno + 1)
                        })?;
                        let val = tok.next().ok_or_else(|| {
                            format!("line {}: 'chipload {}' needs a value", lineno + 1, band)
                        })?;
                        if tok.next().is_some() {
                            return Err(format!("line {}: 'chipload' takes a band and one value", lineno + 1));
                        }
                        let (lo_str, hi_str) = band.split_once("..").ok_or_else(|| {
                            format!("line {}: chipload band '{}' is not <dia-lo>..<dia-hi>", lineno + 1, band)
                        })?;
                        let lo: f64 = lo_str.parse().map_err(|_| {
                            format!("line {}: chipload band low '{}' is not a number", lineno + 1, lo_str)
                        })?;
                        let hi: f64 = hi_str.parse().map_err(|_| {
                            format!("line {}: chipload band high '{}' is not a number", lineno + 1, hi_str)
                        })?;
                        if lo < 0.0 || hi <= lo {
                            return Err(format!(
                                "line {}: chipload band {}..{} must run low to high, non-negative",
                                lineno + 1, lo, hi
                            ));
                        }
                        let v: f64 = val.parse().map_err(|_| {
                            format!("line {}: chipload value '{}' is not a number", lineno + 1, val)
                        })?;
                        if v <= 0.0 {
                            return Err(format!("line {}: chipload ({}) must be positive", lineno + 1, v));
                        }
                        // Bands are half-open [lo, hi), so touching endpoints
                        // compose; any true intersection is two answers for
                        // one diameter and rejects.
                        for (b, b_line) in &rec.chiploads {
                            if lo < b.dia_hi && b.dia_lo < hi {
                                return Err(format!(
                                    "line {}: chipload band {}..{} overlaps the band {}..{} on line {} — one diameter, one answer",
                                    lineno + 1, lo, hi, b.dia_lo, b.dia_hi, b_line + 1
                                ));
                            }
                        }
                        rec.chiploads.push((ChiploadBand { dia_lo: lo, dia_hi: hi, mm_per_tooth: v }, lineno));
                    }
                }
            }
            other => {
                return Err(format!("line {}: unknown feeds keyword '{}'", lineno + 1, other));
            }
        }
    }
    close(open.take(), &mut table)?;

    // The one conversion boundary, the `.part` parser's exact stance: after
    // this, every consumer may assume metric without checking. Inch files
    // read surface_speed as SFM (ft/min) and chipload as in/tooth over inch
    // diameter bands.
    if units_mm == Some(false) {
        const MM_PER_INCH_LOCAL: f64 = 25.4;
        const M_PER_FT: f64 = 0.3048;
        for rec in &mut table.records {
            rec.surface_speed *= M_PER_FT;
            for b in &mut rec.chiploads {
                b.dia_lo *= MM_PER_INCH_LOCAL;
                b.dia_hi *= MM_PER_INCH_LOCAL;
                b.mm_per_tooth *= MM_PER_INCH_LOCAL;
            }
        }
    }
    Ok(table)
}

/// The effective feeds lookup: the shop record shadows the shipped record
/// WHOLE at (material × tool type) granularity — never field-merged. A miss
/// rejects with the missing row named and the inventory the table DOES hold
/// listed (fork 2A — the sim surface's decline-by-name-and-offer voice).
pub fn feeds_lookup<'a>(
    shop: Option<&'a FeedsTable>,
    shipped: &'a FeedsTable,
    material: &str,
    tool_type: &str,
) -> Result<&'a FeedsRecord, String> {
    if let Some(rec) = shop.and_then(|t| t.record(material, tool_type)) {
        return Ok(rec);
    }
    if let Some(rec) = shipped.record(material, tool_type) {
        return Ok(rec);
    }
    // Inventory: shop rows first (they win), then the shipped rows they
    // don't shadow — file order in both, so the listing reads like the files.
    let mut inventory: Vec<String> = Vec::new();
    if let Some(shop) = shop {
        for r in &shop.records {
            inventory.push(format!("{}\u{d7}{}", r.material, r.tool_type));
        }
    }
    for r in &shipped.records {
        if shop.map_or(true, |t| t.record(&r.material, &r.tool_type).is_none()) {
            inventory.push(format!("{}\u{d7}{}", r.material, r.tool_type));
        }
    }
    Err(format!(
        "no feeds record for material '{}' \u{d7} tool type '{}'; the table holds: {}",
        material, tool_type, inventory.join(", ")
    ))
}

/// RPM from surface speed — the first classical identity, RPM = 1000·v_c /
/// (π·d), v_c in m/min and d in mm. One home (DRY for derived values): every
/// consumer calls this, never inline math at an emission site. NOTE the K1
/// hazard, stated in the design: this is UNCLAMPED — a small tool in a fast
/// material can compute past any real spindle's ceiling, and the clamp needs
/// the machine's declared max (K1's noun, not ours).
pub fn derive_spindle_rpm(surface_speed_m_min: f64, dia_mm: f64) -> f64 {
    1000.0 * surface_speed_m_min / (std::f64::consts::PI * dia_mm)
}

/// Feed from chip load — the second classical identity, F = RPM · z · f_z,
/// z the tool's flute count (K2's field) and f_z the band's chip load in
/// mm/tooth. One home (DRY for derived values), same as the RPM identity
/// above. The RPM argument is the op's RESOLVED spindle — derived when the
/// op says `spindle=derive`, the declared/global number otherwise — because
/// the chip thins by what the spindle actually turns, not by what the table
/// would have recommended (the position is recorded in DECISIONS.md →
/// "K3, the material…", slice 1).
pub fn derive_feed_mm_min(rpm: f64, flute_count: usize, chipload_mm_tooth: f64) -> f64 {
    rpm * flute_count as f64 * chipload_mm_tooth
}
