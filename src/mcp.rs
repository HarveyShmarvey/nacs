// The MCP serve loop — `nacs --mcp` (DECISIONS.md "the agent surface — every
// verb is a flag first…", forks 1B/2A/3B/5A; the wire contract is AGENT.md,
// that entry the why).
//
// The shape, in one breath: JSON-RPC 2.0 over stdio, newline-delimited, one
// request at a time — stdio is serial and the verbs are subsecond pure
// functions, so a blocking loop is the whole server. `serde_json` PARSES the
// requests (the pre-blessed Rust-JSON-consumer trigger, fired here and only
// here); every response is hand-rendered like `--check=json`'s document,
// because emitting a known shape is not consuming. The server is STATELESS:
// every call re-reads the project from disk, because the folder of text is
// the truth and a session cache is a second copy that can drift.
//
// Every tool wraps a CLI mode — the standing law that the MCP is never the
// only way to do anything; it is where safety is enforced (the gate floor is
// applied to every verb unless the human launched with `--gates declared`),
// not where capability lives. There is no write verb: `check` with a `text`
// source is the propose-an-edit evaluator, and it physically cannot touch
// disk — review-before-land is enforced by shape, not promised.

use crate::{check_json_report, check_result_of, json_string};
use nacs::parse::{load_project, LoadError};
use nacs::{assembly, gcode, outline, parse, template, vox};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::path::Path;

/// The one MCP protocol revision this server speaks (AGENT.md declares it;
/// spec drift is absorbed by revving that doc, never by tracking an SDK).
const PROTOCOL_VERSION: &str = "2025-06-18";

/// The operator's manual, handed over at `initialize` — a distillation of
/// AGENTS.md, which stays the canonical manual (a terminal agent with no MCP
/// reads it directly and loses nothing).
const INSTRUCTIONS: &str = "nacs turns a plain-text CAM project (a directory with main.part, or a bare .part file) into G-code, with errors and lints as first-class answers. Conventions: (1) errors are hard and explanatory — a rejected program emits nothing, and the message says why and what to change; (2) the gate floor is ON in this session unless the human launched otherwise — advisory findings from the engagement, deviation, and entry families reject instead of warning; (3) this server is stateless and re-reads the project from disk on every call — the folder of text is the truth; (4) nothing here writes: to evaluate an edit before landing it, call check with `text` (the whole candidate main.part) — you land the edit with your own file tools under the user's normal diff review; (5) every tool wraps a CLI flag (check, emit via the default mode, outline, template, rest_report, patterns, assembly_report), so anything this surface does, `nacs` can do in a terminal; (6) assembly_report is the one verb whose subject is NOT the program — it ranks shelf hardware against one declared feature, so the gate floor does not apply to it and it answers on projects the other verbs reject.";

/// Serve until the client hangs up (EOF on stdin). The exit code is the
/// loop's own: 0 on a clean hangup — a rejected *tool call* is a normal
/// response, never a server exit.
pub(crate) fn serve(gate_floor: bool) -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if let Some(resp) = handle_line(&line, gate_floor) {
            let mut out = stdout.lock();
            let _ = writeln!(out, "{}", resp);
            let _ = out.flush();
        }
    }
    0
}

/// One wire line in, at most one wire line out. `None` for blank lines and
/// notifications (a message without an `id` cannot be answered, by spec).
fn handle_line(line: &str, gate_floor: bool) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error_line("null", -32700, &format!("parse error: the line is not JSON ({})", e))),
    };
    let obj = match msg.as_object() {
        Some(o) => o,
        None => return Some(error_line("null", -32600, "invalid request: a JSON-RPC message is an object")),
    };
    // No id = a notification. `notifications/initialized` and kin are
    // acknowledged by silence — the server keeps no handshake state to
    // update (stateless: any method may be called at any time).
    let id = match obj.get("id") {
        None => return None,
        Some(v) => match render_id(v) {
            Some(s) => s,
            None => return Some(error_line("null", -32600, "invalid request: id must be a string, a number, or null")),
        },
    };
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error_line(&id, -32600, "invalid request: jsonrpc must be \"2.0\""));
    }
    let method = match obj.get("method").and_then(Value::as_str) {
        Some(m) => m,
        None => return Some(error_line(&id, -32600, "invalid request: method must be a string")),
    };
    Some(match method {
        "initialize" => result_line(&id, &initialize_result()),
        "ping" => result_line(&id, "{}"),
        "tools/list" => result_line(&id, &tools_list_result()),
        "tools/call" => {
            let params = obj.get("params").cloned().unwrap_or(Value::Null);
            tools_call(&id, &params, gate_floor)
        }
        other => error_line(
            &id,
            -32601,
            &format!("method '{}' is not in this server's subset (initialize, ping, tools/list, tools/call) — see AGENT.md", other),
        ),
    })
}

// ---- the four methods ---------------------------------------------------------

fn initialize_result() -> String {
    format!(
        "{{\"protocolVersion\":{},\"capabilities\":{{\"tools\":{{}}}},\"serverInfo\":{{\"name\":\"nacs\",\"version\":{}}},\"instructions\":{}}}",
        json_string(PROTOCOL_VERSION),
        json_string(env!("CARGO_PKG_VERSION")),
        json_string(INSTRUCTIONS),
    )
}

/// The static tool list: five verbs, each wrapping a CLI mode, each
/// description carrying its verb's conventions (onboarding lives here — the
/// blessed 3C half). Descriptions are the distillation; AGENTS.md stays the
/// manual.
fn tools_list_result() -> String {
    let path_desc = "Path to a project directory (main.part + optional geometry.part) or a bare .part file, resolved against the server's working directory.";
    let tool = |name: &str, desc: &str, schema: String| {
        format!(
            "{{\"name\":{},\"description\":{},\"inputSchema\":{}}}",
            json_string(name),
            json_string(desc),
            schema,
        )
    };
    let one_path_schema = format!(
        "{{\"type\":\"object\",\"properties\":{{\"path\":{{\"type\":\"string\",\"description\":{}}}}},\"required\":[\"path\"],\"additionalProperties\":false}}",
        json_string(path_desc),
    );
    let check_schema = format!(
        "{{\"type\":\"object\",\"properties\":{{\"path\":{{\"type\":\"string\",\"description\":{}}},\"text\":{{\"type\":\"string\",\"description\":\"Candidate main.part content to evaluate WITHOUT touching disk — the propose-an-edit source. Exactly one of path or text.\"}},\"dir\":{{\"type\":\"string\",\"description\":\"Only with text: the project directory the candidate is FOR — its geometry.part and relative assets (SVG profiles, machine and feeds files) resolve from here.\"}}}},\"additionalProperties\":false}}",
        json_string(path_desc),
    );
    let template_schema = format!(
        "{{\"type\":\"object\",\"properties\":{{\"path\":{{\"type\":\"string\",\"description\":{}}},\"feature\":{{\"type\":\"string\",\"description\":\"The declared feature to suggest run skeletons for.\"}}}},\"required\":[\"path\",\"feature\"],\"additionalProperties\":false}}",
        json_string(path_desc),
    );
    let mm = |what: &str| format!("{{\"type\":\"number\",\"description\":{}}}", json_string(what));
    let assembly_schema = format!(
        "{{\"type\":\"object\",\"properties\":{{\"catalog\":{{\"type\":\"string\",\"description\":\"Path to the cutter/holder catalog to search.\"}},\"path\":{{\"type\":\"string\",\"description\":{}}},\"feature\":{{\"type\":\"string\",\"description\":\"The declared cavity in that project to reach into — a bore, a pocket, or a revolved bowl/throat/funnel. Comes with path; the two state the job together.\"}},\"reach\":{},\"opening\":{},\"depth\":{},\"clearance\":{},\"load\":{},\"material\":{{\"type\":\"string\",\"description\":\"The workpiece material, for the rpm column. Without it the report prints its honest omission line.\"}},\"feeds\":{{\"type\":\"string\",\"description\":\"Path to the shop's own feeds table; its rows outrank the shipped ones. Needs material.\"}}}},\"required\":[\"catalog\"],\"additionalProperties\":false}}",
        json_string(path_desc),
        mm("How far below the part top the tip must reach, mm. Typed alternative to path+feature."),
        mm("The cavity's crossing at the working depth, mm. Typed alternative to path+feature."),
        mm("The cavity's own depth, mm. Typed alternative to path+feature."),
        mm("Radial clearance at the holder nose, mm (default 0.5)."),
        mm("Representative tip load the deflection column is priced at, N (default 10)."),
    );
    let tools = [
        tool(
            "check",
            "Parse and validate a project and return the full check_result as one JSON document: would_emit, the error when rejected, and every lint finding (family, subject, message) — the same document `nacs --check=json` prints. An error is a VERDICT, not a tool error. Pass `path` for a project on disk, or `text` (with optional `dir`) to evaluate a candidate main.part you have not written anywhere — nothing is landed either way.",
            check_schema,
        ),
        tool(
            "emit",
            "Generate the project's G-code and return the program text (the default `nacs` mode). The program is never written to a file. An error returns a structured envelope {stage, message} with isError true — stages: parse, validate (the gate floor's errors surface here), emit. For findings without the program, call check.",
            one_path_schema.clone(),
        ),
        tool(
            "outline",
            "The project tree as text (`nacs --outline`): stock, features, and operations with each op's derived role. Pure declaration reading — it answers on projects the voxel engine would decline.",
            one_path_schema.clone(),
        ),
        tool(
            "schedule",
            "A `run` line for every declared feature no run names yet (`nacs --schedule`), in shop order, each tool picked from the project's own tool lines by a stated rule; a feature no declared tool fits gets a note instead. Text to append to main.part, never written by this tool.",
            one_path_schema.clone(),
        ),
        tool(
            "template",
            "Suggested `run` skeleton(s) for one declared feature (`nacs --template <feature>`). Templates are skeletons: `T?`/`?` placeholders are yours to fill — the tool never guesses.",
            template_schema,
        ),
        tool(
            "rest_report",
            "The rest-material report (`nacs --rest-report`): what still stands, and what the declarations claim removed that no pass swept. A declining report is still a report — it says why in its own text.",
            one_path_schema.clone(),
        ),
        tool(
            "patterns",
            "The pattern comparison (`nacs --patterns`): for every op that works a region (an End, or a Clear on a ProfilePocket), each walking pattern — offset, zigzag, oneway, spine — as a row of measured consequences in four groups: COVER (reachable floor left uncut in mm², passes across each narrow land), COST (length cut at depth, the entries that reach it, time, plunges, ramps, lifts, turns), WHERE (how much of the at-depth length was over the floor / other stock / air), LOAD (length past half immersion, length in a full slot, the band's worst engagement where the band answers). Every % is a share of the at-depth length. Never a score; the engine does not choose. To pin one, add `pattern=<word>` to that op's run line in a candidate main.part and call check with `text`.",
            one_path_schema,
        ),
        tool(
            "assembly_report",
            "Rank cutter+holder assemblies from a catalog against one job (`nacs --assembly-report`): projection, L/D, deflection, rpm, and the exact `tool` line to declare each one. State the job EITHER by naming a declared cavity (path + feature, read off the declaration) OR by passing reach/opening/depth as numbers — not both. This is the one verb whose subject is not the program: it ranks shelf hardware, so the gate floor does not apply and it answers on projects the other verbs reject. To act on a row, put its `tool` line into a candidate main.part and call check with `text`.",
            assembly_schema,
        ),
    ];
    format!("{{\"tools\":[{}]}}", tools.join(","))
}

// ---- tools/call ----------------------------------------------------------------

/// How a tool call fails, split where JSON-RPC splits it: `Args` is a
/// protocol-level fault (the caller broke the schema — JSON-RPC -32602),
/// while `Tool` is the engine answering (an error envelope in the result,
/// isError true — the call itself worked).
enum CallFail {
    Args(String),
    Tool { stage: &'static str, message: String },
}

fn tools_call(id: &str, params: &Value, gate_floor: bool) -> String {
    let name = match params.get("name").and_then(Value::as_str) {
        Some(n) => n,
        None => return error_line(id, -32602, "tools/call needs params.name (a string)"),
    };
    let empty = Value::Object(serde_json::Map::new());
    let args = params.get("arguments").unwrap_or(&empty);
    if !args.is_object() {
        return error_line(id, -32602, "tools/call params.arguments must be an object when present");
    }
    match call_tool(name, args, gate_floor) {
        Ok(text) => result_line(id, &content_result(&text, false)),
        Err(CallFail::Args(m)) => error_line(id, -32602, &m),
        Err(CallFail::Tool { stage, message }) => {
            let envelope = format!(
                "{{\n  \"stage\": {},\n  \"message\": {}\n}}\n",
                json_string(stage),
                json_string(&message),
            );
            result_line(id, &content_result(&envelope, true))
        }
    }
}

fn call_tool(name: &str, args: &Value, gate_floor: bool) -> Result<String, CallFail> {
    check_arg_keys(name, args)?;
    match name {
        "check" => tool_check(args, gate_floor),
        "emit" => tool_emit(args, gate_floor),
        "outline" => Ok(outline::report(&loaded_and_floored(args, gate_floor)?)),
        "template" => tool_template(args, gate_floor),
        "schedule" => nacs::schedule::report(&loaded_and_floored(args, gate_floor)?)
            .map_err(|e| CallFail::Tool { stage: "schedule", message: format!("error: {}", e) }),
        "rest_report" => tool_rest_report(args, gate_floor),
        "patterns" => tool_patterns(args, gate_floor),
        "assembly_report" => tool_assembly_report(args),
        other => Err(CallFail::Args(format!(
            "no tool named '{}' — the tools are check, emit, outline, template, rest_report, patterns, assembly_report",
            other
        ))),
    }
}

/// The whitelist, the tool line's own law one wire up: an argument nobody
/// reads is a request that quietly does not exist, so unknown keys reject by
/// name instead of vanishing.
fn check_arg_keys(name: &str, args: &Value) -> Result<(), CallFail> {
    let known: &[&str] = match name {
        "check" => &["path", "text", "dir"],
        "template" => &["path", "feature"],
        "emit" | "outline" | "schedule" | "rest_report" | "patterns" => &["path"],
        "assembly_report" => {
            &["catalog", "path", "feature", "reach", "opening", "depth", "clearance", "load", "material", "feeds"]
        }
        _ => return Ok(()), // the unknown-tool error names itself downstream
    };
    if let Some(obj) = args.as_object() {
        let mut unknown: Vec<&str> =
            obj.keys().map(String::as_str).filter(|k| !known.contains(k)).collect();
        if !unknown.is_empty() {
            unknown.sort_unstable();
            return Err(CallFail::Args(format!(
                "tool '{}' has no argument named '{}' — known arguments: {}",
                name,
                unknown.join("', '"),
                known.join(", "),
            )));
        }
    }
    Ok(())
}

fn arg_str(args: &Value, key: &str) -> Result<Option<String>, CallFail> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(CallFail::Args(format!("argument '{}' must be a string", key))),
    }
}

fn require_path(args: &Value) -> Result<String, CallFail> {
    arg_str(args, "path")?.ok_or_else(|| CallFail::Args("this tool needs a 'path' argument".into()))
}

/// Load a path-source project for a verb whose artifact is not the check result:
/// I/O and parse errors become envelopes (for `check`, a parse error is
/// the check result instead — it builds its own flow).
fn load_for_verb(args: &Value) -> Result<parse::Part, CallFail> {
    match load_project(&require_path(args)?) {
        Ok(p) => Ok(p),
        Err(LoadError::Io(m)) => Err(CallFail::Tool { stage: "io", message: m }),
        Err(LoadError::Parse(e)) => {
            Err(CallFail::Tool { stage: "parse", message: format!("parse error: {}", e) })
        }
    }
}

/// The gate floor, applied to every verb unconditionally (fork 5A): the
/// escalation is `parse::apply_gate_floor`'s — one home — and the second
/// validate pass is what surfaces its errors, stage "validate".
fn floor(part: &mut parse::Part, gate_floor: bool) -> Result<(), CallFail> {
    if gate_floor {
        parse::apply_gate_floor(part);
        if let Err(e) = parse::validate(part) {
            return Err(CallFail::Tool { stage: "validate", message: format!("error: {}", e) });
        }
    }
    Ok(())
}

fn loaded_and_floored(args: &Value, gate_floor: bool) -> Result<parse::Part, CallFail> {
    let mut part = load_for_verb(args)?;
    floor(&mut part, gate_floor)?;
    Ok(part)
}

/// `check`, two sources, one verb (fork 3B): a `path` is `--check=json` over
/// the wire; `text` is the propose-an-edit evaluator — the whole candidate
/// main.part, judged without touching disk. Engine errors of every stage
/// are VERDICTS here (would_emit false in the document, isError false);
/// only an unreadable path/dir is a tool error.
fn tool_check(args: &Value, gate_floor: bool) -> Result<String, CallFail> {
    let path = arg_str(args, "path")?;
    let text = arg_str(args, "text")?;
    let dir = arg_str(args, "dir")?;
    if dir.is_some() && text.is_none() {
        return Err(CallFail::Args("'dir' accompanies 'text' only — with 'path' the project directory is the path itself".into()));
    }
    let parsed = match (path, text) {
        (Some(_), Some(_)) | (None, None) => {
            return Err(CallFail::Args("check takes exactly one source: 'path' (a project on disk) or 'text' (candidate main.part content)".into()));
        }
        (Some(p), None) => match load_project(&p) {
            Ok(part) => Ok(part),
            Err(LoadError::Io(m)) => return Err(CallFail::Tool { stage: "io", message: m }),
            Err(LoadError::Parse(e)) => Err(e),
        },
        (None, Some(t)) => {
            let (geom, base) = match &dir {
                Some(d) => {
                    let dp = Path::new(d);
                    if !dp.is_dir() {
                        return Err(CallFail::Tool {
                            stage: "io",
                            message: format!("dir '{}' is not a directory", d),
                        });
                    }
                    let geom_path = dp.join("geometry.part");
                    let geom = if geom_path.exists() {
                        Some(std::fs::read_to_string(&geom_path).map_err(|e| CallFail::Tool {
                            stage: "io",
                            message: format!("cannot read {}: {}", geom_path.display(), e),
                        })?)
                    } else {
                        None
                    };
                    (geom, Some(dp))
                }
                None => (None, None),
            };
            parse::parse_project_with_base(&t, geom.as_deref(), base)
        }
    };
    let mut part = match parsed {
        Ok(p) => p,
        Err(e) => return Ok(check_json_report(Some(&format!("parse error: {}", e)), &[], &[])),
    };
    if gate_floor {
        parse::apply_gate_floor(&mut part);
        if let Err(e) = parse::validate(&part) {
            // The declared-gate error's shape, one wire up: findings [],
            // exactly what the CLI renders for the same program.
            return Ok(check_json_report(Some(&format!("error: {}", e)), &[], &[]));
        }
    }
    Ok(check_result_of(&part).0)
}

fn tool_emit(args: &Value, gate_floor: bool) -> Result<String, CallFail> {
    let part = loaded_and_floored(args, gate_floor)?;
    match gcode::program(&part) {
        // The default mode's stdout bytes: the program plus its newline.
        Ok(g) => Ok(format!("{}\n", g)),
        Err(e) => Err(CallFail::Tool { stage: "emit", message: format!("error: {}", e) }),
    }
}

fn tool_template(args: &Value, gate_floor: bool) -> Result<String, CallFail> {
    let feature = arg_str(args, "feature")?
        .ok_or_else(|| CallFail::Args("template needs a 'feature' argument".into()))?;
    let part = loaded_and_floored(args, gate_floor)?;
    template::report(&part, &feature)
        .map_err(|e| CallFail::Tool { stage: "template", message: format!("error: {}", e) })
}

/// A number argument, JSON-typed. Numbers arrive as numbers on this wire —
/// the CLI's `key=value` text is a shell's limitation, not the model's.
fn arg_f64(args: &Value, key: &str) -> Result<Option<f64>, CallFail> {
    match args.get(key) {
        None => Ok(None),
        Some(v) => match v.as_f64() {
            Some(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(CallFail::Args(format!("argument '{}' must be a finite number", key))),
        },
    }
}

/// `assembly_report` — **the one verb the gate floor does not touch**
/// (`DECISIONS.md` → "`--assembly-report` as the sixth verb…", fork 2A).
///
/// The floor exists so an agent cannot talk itself past a finding ABOUT A
/// PROGRAM. Every other verb has the program as its subject; this one ranks
/// shelf hardware against one declared feature's geometry, and the program is
/// merely where that feature is written down. Measured, at the DRAFT: a
/// project with a heavy roughing skin rejects under the floor with an
/// `engagement gate` message about a different op, while the recommendation it
/// was asked for is perfectly well-formed. And the clinching argument is a
/// spelling — the same job passed as `reach`/`opening`/`depth` has no program
/// to floor at all, so a floor here would answer one question two ways
/// depending on how it was asked.
///
/// The project's own load keeps the shared stages (`io`, `parse`), because the
/// project is the surface's shared noun; everything else this mode rejects is
/// its own stage, the contract's rule for a reporting verb.
fn tool_assembly_report(args: &Value) -> Result<String, CallFail> {
    let mine = |m: String| CallFail::Tool { stage: "assembly_report", message: format!("error: {}", m) };
    let catalog_path = arg_str(args, "catalog")?
        .ok_or_else(|| CallFail::Args("this tool needs a 'catalog' argument (the path to the cutter/holder catalog to search)".into()))?;
    let catalog = assembly::read_catalog(&catalog_path).map_err(mine)?;

    let mut req = assembly::ReportRequest {
        reach: arg_f64(args, "reach")?,
        opening: arg_f64(args, "opening")?,
        depth: arg_f64(args, "depth")?,
        material: arg_str(args, "material")?,
        ..assembly::ReportRequest::default()
    };
    if let Some(c) = arg_f64(args, "clearance")? {
        req.clearance = c;
    }
    if let Some(l) = arg_f64(args, "load")? {
        req.load = l;
    }
    if let Some(f) = arg_str(args, "feeds")? {
        req.shop_feeds = Some(assembly::read_feeds(&f).map_err(mine)?);
    }
    // `path` + `feature` are the CLI's `part=`/`feature=` under the surface's
    // own noun: here a path means what it means for every other verb, and
    // resolves through the same home.
    match (arg_str(args, "path")?, arg_str(args, "feature")?) {
        (None, None) => {}
        (Some(_), None) | (None, Some(_)) => {
            return Err(CallFail::Args(
                "'path' and 'feature' come together — a path with no feature named says which project to read and not what to reach into, and a feature with no path says the opposite".into(),
            ))
        }
        (Some(path), Some(feature)) => {
            let part = match load_project(&path) {
                Ok(p) => p,
                Err(LoadError::Io(m)) => return Err(CallFail::Tool { stage: "io", message: m }),
                Err(LoadError::Parse(e)) => {
                    return Err(CallFail::Tool { stage: "parse", message: format!("parse error: {}", e) })
                }
            };
            req.declared = Some(assembly::DeclaredJob { part, feature, shown_as: path });
        }
    }
    assembly::report_for(&catalog, req).map_err(mine)
}

fn tool_patterns(args: &Value, gate_floor: bool) -> Result<String, CallFail> {
    let part = loaded_and_floored(args, gate_floor)?;
    Ok(nacs::compare::render(&part))
}

fn tool_rest_report(args: &Value, gate_floor: bool) -> Result<String, CallFail> {
    let part = loaded_and_floored(args, gate_floor)?;
    vox::rest_report(&part, None)
        .map_err(|e| CallFail::Tool { stage: "rest_report", message: format!("error: {}", e) })
}

// ---- the wire renderings (hand-rolled; serde_json only ever parses) -----------

/// The echoed request id, rendered back as the JSON literal it arrived as.
/// `None` for types the spec forbids (bool, array, object).
fn render_id(id: &Value) -> Option<String> {
    match id {
        Value::Null => Some("null".into()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(json_string(s)),
        _ => None,
    }
}

fn result_line(id: &str, result: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":{}}}", id, result)
}

fn error_line(id: &str, code: i64, message: &str) -> String {
    format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"error\":{{\"code\":{},\"message\":{}}}}}",
        id,
        code,
        json_string(message),
    )
}

/// A tools/call result: one text content block — the artifact, exactly the
/// bytes the wrapped CLI mode would put on stdout — and the explicit isError
/// check result both ways (self-describing beats a default the reader must know).
fn content_result(text: &str, is_error: bool) -> String {
    format!(
        "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"isError\":{}}}",
        json_string(text),
        is_error,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal clean program: parses, validates, emits, no findings.
    const CLEAN: &str = "units mm\nfeed 800\nplunge 300\nrapid_z 5\nclearance_z 25\nspindle 3000\n\
        stock Box(w=60, l=60, h=20, x=0, y=0, z=0)\n\
        tool T1 Endmill(dia=10.0)\n\
        feature Top Plane(z=-1.0)\n\
        run End(tool=T1, surface=Top, stepover=6.0)\n";

    /// The same program with a heavy roughing skin chained into a finish —
    /// one engagement finding, advisory by default, rejected under the floor.
    fn heavy_src() -> String {
        format!(
            "{}feature Bore CylinderBore(dia=30.0, extent=10.0, x=30.0, y=30.0)\n\
             run Clear(tool=T1, profile=Bore, stepover=5.0, stock_to_leave=5.0)\n\
             run Side(tool=T1, surface=Bore, stepover=5.0)\n",
            CLEAN
        )
    }

    fn write_project(name: &str, src: &str) -> String {
        let dir = format!("{}.cam", name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{}/main.part", dir), src).unwrap();
        dir
    }

    fn drop_project(dir: &str) {
        let _ = std::fs::remove_dir_all(dir);
    }

    fn call(line: &str, floor: bool) -> String {
        handle_line(line, floor).expect("a request with an id gets a response")
    }

    /// The handshake, pinned as exact bytes: the contract doc quotes this
    /// shape, and a drifting handshake is a protocol change that must be
    /// noticed, not absorbed.
    #[test]
    fn initialize_is_deterministic_and_names_the_protocol() {
        let resp = call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#, true);
        assert!(resp.starts_with(r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"nacs","version":"#), "{resp}");
        assert!(resp.contains("\"instructions\":\""), "{resp}");
        assert!(resp.contains("gate floor"), "the manual rides the handshake: {resp}");
        assert!(!resp.contains('\n'), "one line per message — the transport is newline-delimited");
    }

    #[test]
    fn tools_list_names_the_six_verbs_and_their_schemas() {
        let resp = call(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#, true);
        for name in ["check", "emit", "outline", "schedule", "template", "rest_report", "assembly_report"] {
            assert!(resp.contains(&format!("{{\"name\":\"{}\"", name)), "missing {name}: {resp}");
        }
        assert!(resp.contains("\"inputSchema\""), "{resp}");
        assert!(resp.contains("propose-an-edit"), "the text source says what it is for: {resp}");
        // The sixth verb's schema takes NUMBERS as numbers — the CLI's
        // `key=value` text is a shell's limitation, not the model's.
        assert!(resp.contains(r#""reach":{"type":"number""#), "{resp}");
        assert!(!resp.contains('\n'));
    }

    /// The wire's own errors, JSON-RPC's exact vocabulary.
    #[test]
    fn protocol_faults_answer_with_the_named_codes() {
        assert!(call("this is not json", true).contains("\"code\":-32700"));
        assert!(call(r#"{"jsonrpc":"2.0","id":7,"method":"resources/list"}"#, true).contains("\"code\":-32601"));
        assert!(call(r#"{"jsonrpc":"1.0","id":7,"method":"ping"}"#, true).contains("\"code\":-32600"));
        assert!(call(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"drive_spindle"}}"#, true).contains("\"code\":-32602"));
        // A notification — no id — is acknowledged by silence, whatever it says.
        assert_eq!(handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, true), None);
        assert_eq!(handle_line("", true), None);
        // Ping answers the empty result.
        assert_eq!(call(r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#, true), r#"{"jsonrpc":"2.0","id":9,"result":{}}"#);
    }

    /// The unknown-argument whitelist, the tool line's law one wire up.
    #[test]
    fn an_argument_nobody_reads_refuses_by_name() {
        let resp = call(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"check","arguments":{"path":"x.cam","force":true}}}"#, true);
        assert!(resp.contains("\"code\":-32602") && resp.contains("no argument named 'force'"), "{resp}");
    }

    /// check over a path: the check result document, verbatim `--check=json`.
    #[test]
    fn check_returns_the_cli_documents_bytes() {
        let dir = write_project("mcp_check_clean", CLEAN);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"check","arguments":{{"path":"{}"}}}}}}"#,
            dir
        );
        let resp = call(&req, true);
        drop_project(&dir);
        assert!(resp.contains("\"isError\":false"), "{resp}");
        assert!(
            resp.contains(r#"\"would_emit\": true"#) && resp.contains(r#"\"findings\": []"#),
            "the content is the check document: {resp}"
        );
    }

    /// The gate floor over the wire (fork 5A): the same project that checks
    /// clean at its declared modes rejects under the floor — and the error
    /// is a VERDICT (isError false), carrying the gate's own prose.
    #[test]
    fn the_gate_floor_turns_the_advisory_into_a_refusal_verdict() {
        let dir = write_project("mcp_check_floor", &heavy_src());
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"check","arguments":{{"path":"{}"}}}}}}"#,
            dir
        );
        let advisory = call(&req, false);
        let floored = call(&req, true);
        drop_project(&dir);
        assert!(
            advisory.contains(r#"\"would_emit\": true"#) && advisory.contains(r#"\"family\": \"engagement\""#),
            "at declared modes the finding is advisory data: {advisory}"
        );
        assert!(
            floored.contains(r#"\"would_emit\": false"#)
                && floored.contains("engagement gate:")
                && floored.contains("\"isError\":false"),
            "under the floor the same finding rejects, as a check_result: {floored}"
        );
    }

    /// The propose source (fork 3B): candidate text is judged without disk,
    /// and a candidate that rejects is a check result — nothing was landed to
    /// find that out.
    #[test]
    fn check_over_text_is_the_dry_run_evaluator() {
        let good = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"check","arguments":{{"text":{}}}}}}}"#,
            json_string(CLEAN)
        );
        let resp = call(&good, true);
        assert!(resp.contains(r#"\"would_emit\": true"#), "{resp}");

        let bad = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"check","arguments":{{"text":{}}}}}}}"#,
            json_string("units mm\nfeed 800\nwat T1\n")
        );
        let resp = call(&bad, true);
        assert!(
            resp.contains(r#"\"would_emit\": false"#) && resp.contains("\"isError\":false"),
            "a rejecting candidate is a check_result, not a tool error: {resp}"
        );

        let both = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"check","arguments":{"path":"a","text":"b"}}}"#;
        assert!(call(both, true).contains("exactly one source"));
        let neither = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"check","arguments":{}}}"#;
        assert!(call(neither, true).contains("exactly one source"));
    }

    /// emit returns the default mode's stdout bytes; its errors are the
    /// structured envelope with the stage named.
    #[test]
    fn emit_returns_the_program_and_envelopes_its_refusals() {
        let dir = write_project("mcp_emit_clean", CLEAN);
        let req = |name: &str, dir: &str| {
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{}","arguments":{{"path":"{}"}}}}}}"#,
                name, dir
            )
        };
        let resp = call(&req("emit", &dir), true);
        drop_project(&dir);
        assert!(resp.contains("\"isError\":false"), "{resp}");
        assert!(resp.contains(r#"(nacs — nacs output)"#), "the artifact is the program: {resp}");

        // An unreadable path: the one error class with no check result to render.
        let resp = call(&req("emit", "no_such_project.cam"), true);
        assert!(
            resp.contains("\"isError\":true") && resp.contains(r#"\"stage\": \"io\""#),
            "{resp}"
        );

        // A parse refusal: envelope, stage parse, the prose line verbatim.
        let dir = write_project("mcp_emit_bad", "units mm\nwat\n");
        let resp = call(&req("emit", &dir), true);
        drop_project(&dir);
        assert!(
            resp.contains("\"isError\":true")
                && resp.contains(r#"\"stage\": \"parse\""#)
                && resp.contains("parse error:"),
            "{resp}"
        );

        // The floor's own stage: a program whose gate error exists only
        // because the floor escalated it envelopes as stage validate.
        let dir = write_project("mcp_emit_floor", &heavy_src());
        let resp = call(&req("emit", &dir), true);
        drop_project(&dir);
        assert!(
            resp.contains("\"isError\":true")
                && resp.contains(r#"\"stage\": \"validate\""#)
                && resp.contains("engagement gate:"),
            "{resp}"
        );
    }

    /// The three report verbs wrap their modes; template's own error keeps
    /// its mode as the stage.
    #[test]
    fn the_report_verbs_wrap_their_cli_modes() {
        let dir = write_project("mcp_reports", CLEAN);
        let call_verb = |name: &str, extra: &str| {
            call(
                &format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"{}","arguments":{{"path":"{}"{}}}}}}}"#,
                    name, dir, extra
                ),
                true,
            )
        };
        assert!(call_verb("outline", "").contains("] End"), "the outline names the op");
        assert!(call_verb("rest_report", "").contains("\"isError\":false"));
        let patterns = call_verb("patterns", "");
        assert!(patterns.contains("\"isError\":false") && patterns.contains("nothing here is a score"), "{patterns}");
        let resp = call_verb("template", r#","feature":"Top""#);
        assert!(resp.contains("\"isError\":false") && resp.contains("End"), "{resp}");
        let resp = call_verb("template", r#","feature":"Nope""#);
        assert!(
            resp.contains("\"isError\":true") && resp.contains(r#"\"stage\": \"template\""#),
            "{resp}"
        );
        drop_project(&dir);
    }

    // ---- the sixth verb (DECISIONS.md → "`--assembly-report` as the sixth
    // verb…") ----

    const CATALOG: &str = "tests/fixtures/sample.catalog";

    fn report_call(id: u32, args: &str, floor: bool) -> String {
        let resp = call(
            &format!(
                r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"assembly_report","arguments":{}}}}}"#,
                id, args
            ),
            floor,
        );
        resp
    }

    /// ACCEPTANCE 1: the result is the CLI mode's stdout, byte for byte. The
    /// EQUALITY is the pin — not a sample of the text — because the contract's
    /// law for a verb's result is the wrapped mode's exact bytes, and a
    /// sampled assertion would let the two drift between the sampled lines.
    #[test]
    fn the_sixth_verb_returns_the_cli_modes_exact_bytes() {
        let resp = report_call(
            1,
            r#"{"catalog":"tests/fixtures/sample.catalog","path":"spline_funnel_holder.cam","feature":"Vortex"}"#,
            true,
        );
        let cli = assembly::report_from_args(&[
            CATALOG.to_string(),
            "part=spline_funnel_holder.cam".to_string(),
            "feature=Vortex".to_string(),
        ])
        .expect("the CLI door answers");
        assert!(resp.contains(&json_string(&cli)), "the verb must serve the mode's own bytes: {resp}");
        assert!(resp.contains(r#""isError":false"#), "{resp}");
    }

    /// ACCEPTANCE 2, and the fork the whole entry turned on: **this verb
    /// answers where the floor rejects the program.** A heavy roughing skin
    /// makes `check` reject under the floor; the recommendation is about a
    /// different question entirely, and comes out in the same session.
    #[test]
    fn the_gate_floor_does_not_reach_the_one_verb_whose_subject_is_not_the_program() {
        let dir = write_project("mcp_assembly_floor", &heavy_src());
        let check_result = call(
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"check","arguments":{{"path":"{}"}}}}}}"#,
                dir
            ),
            true,
        );
        assert!(check_result.contains("engagement gate"), "the floor rejects this program: {check_result}");

        let resp = report_call(
            2,
            &format!(r#"{{"catalog":"{}","path":"{}","feature":"Bore"}}"#, CATALOG, dir),
            true,
        );
        drop_project(&dir);
        assert!(resp.contains(r#""isError":false"#), "the recommendation still comes: {resp}");
        assert!(resp.contains("assembly report"), "{resp}");
        // And the same job PASSED AS NUMBERS answers identically — the
        // clinching argument for fork 2A: with no program in the call there is
        // nothing to floor, so a floor on the other spelling would answer one
        // question two ways depending on how it was asked.
        let typed = report_call(
            3,
            &format!(r#"{{"catalog":"{}","reach":10,"opening":30,"depth":10}}"#, CATALOG),
            true,
        );
        assert!(typed.contains(r#""isError":false"#), "{typed}");
    }

    /// ACCEPTANCE 4: the agent's loop, in three calls — read, propose,
    /// re-check — closing on the family it set out to fix.
    ///
    /// **Run with the floor DOWN, and that is the finding rather than a
    /// dodge.** Under the floor this fixture's first error is the ENTRY
    /// gate's, and a rejected program never reaches the post-generation
    /// advisories, so the reach finding it was called about is not in the
    /// document at all. Gates are ORDERED (`DECISIONS.md` → "the reach gate
    /// lands…"), and an acceptance that pinned the reach family under the
    /// floor here would be measuring the wrong gate.
    #[test]
    fn the_loop_closes_in_three_calls_on_the_family_it_was_called_about() {
        let src = std::fs::read_to_string("spline_funnel_holder.cam/main.part").expect("read");
        let families = |doc: &str| -> Vec<String> {
            let v: Value = serde_json::from_str(doc).expect("the check document is JSON");
            v["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .map(|f| f["family"].as_str().unwrap_or("?").to_string())
                .collect()
        };
        let text_of = |resp: &str| -> String {
            let v: Value = serde_json::from_str(resp).expect("a response");
            v["result"]["content"][0]["text"].as_str().expect("text").to_string()
        };

        // 1. What is wrong: the certificate's own finding, among others.
        let one = text_of(&call(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"check","arguments":{"path":"spline_funnel_holder.cam"}}}"#,
            false,
        ));
        assert!(families(&one).iter().any(|f| f == "reach"), "the reach finding is what we are here for: {one}");

        // 2. What to do about it: the row that proposes a nose which follows
        //    the tool in, and the `tool` line that declares it.
        let two = text_of(&report_call(
            2,
            r#"{"catalog":"tests/fixtures/sample.catalog","path":"spline_funnel_holder.cam","feature":"Vortex"}"#,
            false,
        ));
        let rows: Vec<&str> = two.lines().collect();
        let at = rows.iter().position(|l| l.contains("[nose enters the opening]")).expect("a nose-enters row");
        let proposed = rows[at + 1].trim().replacen("T?", "T10", 1);
        assert!(proposed.starts_with("tool T10 Ballmill("), "{proposed}");

        // 3. The same program with that line in it, judged without touching
        //    disk — and the reach family is GONE.
        let candidate: String = src
            .lines()
            .map(|l| if l.starts_with("tool T10 ") { proposed.clone() } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n");
        let three = text_of(&call(
            &format!(
                r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"check","arguments":{{"text":{},"dir":"spline_funnel_holder.cam"}}}}}}"#,
                json_string(&candidate)
            ),
            false,
        ));
        let after = families(&three);
        assert!(!after.iter().any(|f| f == "reach"), "the loop must close on reach: {after:?}");
        // The body foul goes with it — the slimmer nose is no longer standing
        // in the wall — and what remains is the entry gate's, which is a
        // different question and this fixture's own documented one.
        assert_eq!(after, vec!["entry".to_string()], "{three}");
    }

    /// The argument faults this verb adds, in its own words: a missing
    /// catalog, a half-named job, and a key nobody reads.
    #[test]
    fn the_sixth_verbs_arguments_refuse_the_ways_they_can_be_wrong() {
        let missing = report_call(1, r#"{"reach":10,"opening":30,"depth":10}"#, true);
        assert!(missing.contains("needs a 'catalog' argument"), "{missing}");

        let half = report_call(
            2,
            &format!(r#"{{"catalog":"{}","path":"cylinder_bore.cam"}}"#, CATALOG),
            true,
        );
        assert!(half.contains("come together"), "{half}");

        // The error that belongs to `report_for` rather than to either door:
        // one fact in two places, asked over the wire.
        let both = report_call(
            3,
            &format!(r#"{{"catalog":"{}","path":"cylinder_bore.cam","feature":"Bore","reach":40}}"#, CATALOG),
            true,
        );
        assert!(both.contains("one fact in two places"), "{both}");
        assert!(both.contains(r#"\"stage\": \"assembly_report\""#), "the mode's own stage: {both}");

        // The whitelist law, one wire up.
        let unknown = report_call(
            4,
            &format!(r#"{{"catalog":"{}","reach":10,"opening":30,"depth":10,"holder":"er32"}}"#, CATALOG),
            true,
        );
        assert!(unknown.contains("has no argument named 'holder'"), "{unknown}");

        // A number is a number: passing one as text is a schema fault, not a
        // silent re-parse.
        let stringly = report_call(
            5,
            &format!(r#"{{"catalog":"{}","reach":"10","opening":30,"depth":10}}"#, CATALOG),
            true,
        );
        assert!(stringly.contains("must be a finite number"), "{stringly}");

        // And the project's own load keeps the SHARED stages, because the
        // project is the surface's shared noun.
        let nothere = report_call(
            6,
            &format!(r#"{{"catalog":"{}","path":"no_such_project.cam","feature":"Bore"}}"#, CATALOG),
            true,
        );
        assert!(nothere.contains(r#"\"stage\": \"io\""#), "{nothere}");
    }

}
