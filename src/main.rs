// nacs — the nacs CLI
// ------------------------------------------------------------------
// Reads a human-readable project directory or .part file (geometry + stock + tools)
// and EMITS real G-code: facing raster, contour finishing, etc., in sequence.
// ------------------------------------------------------------------

use std::env;
use std::process;
use nacs::parse::{load_project, LoadError};
use nacs::{parse, gcode, outline, stock, template, vox, assembly};

// The MCP serve loop (DECISIONS.md "the agent surface — every verb is a flag
// first…", forks 1B/2A): a module of the BIN, not the library, because the
// verbs are thin wrappers over the same library calls the CLI modes make and
// the renderings they serve (the check document above all) live here — one
// home, now with two callers. Feature-gated so the lean build stays JSON-free.
#[cfg(feature = "agent")]
mod mcp;

fn main() {
    // `--rest-report` swaps the output: the rest-material report (what still
    // stands, and what the declarations claim gone that no pass swept)
    // instead of G-code. An explicit mode, so stdout is always exactly one
    // artifact and the no-partial-G-code rule keeps holding.
    //
    // `--outline` is the same shape for the project tree (Phase 6, slice 0 —
    // DECISIONS.md "Phase 6 opens…"): stock -> features -> operations with
    // each op's derived role. A mode, never a change to the program, so every
    // golden stays byte-identical whether or not it exists.
    //
    // `--view` opens the Phase-6 native window (slice 1) — a mode like the
    // others for exclusion purposes, except its artifact is a window and
    // stdout stays empty. The flag PARSES in every build; without the
    // `frontend` feature it rejects by name rather than falling through to
    // "unexpected argument", so the lean build keeps the same CLI surface.
    //
    // `--check` is the validate-only mode (DECISIONS.md "the agent surface —
    // every verb is a flag first…", the first of its two missing flags): the
    // findings ARE the artifact. The whole pipeline runs and the finished
    // program is discarded — stdout stays empty, stderr says exactly what
    // the default mode would say, and the exit code answers "would this
    // emit?". Engine-side like the pipeline it wraps, so it works in both
    // feature configs.
    //
    // `--check=json` is the same check result's structured rendering (the second
    // missing flag): the `Finding` structs and the would-emit answer as one
    // JSON document on stdout — the JSON IS the artifact, so in this
    // rendering stderr stays silent and everything the prose mode says
    // lives inside the document instead. Same pipeline, same exit codes;
    // one fact, two renderings, one home. Hand-rolled std-only (DECISIONS.md
    // "--check=json — the check result as data…"), so it too works in both
    // feature configs.
    // `--mcp` serves the agent surface: the whole session is the artifact and
    // stdout is the wire, so it excludes every other mode and takes no project
    // path — the tools name their paths per call, and the server re-reads the
    // disk each time (stateless: the folder of text is the truth). The flag
    // parses in every build; the lean build rejects it by name, the `--view`
    // pattern (DECISIONS.md "the agent surface — every verb is a flag first…").
    //
    // `--tool-line` is the clipboard V4 decided on (DECISIONS.md "V4, tool and
    // holder libraries…", forks 1A/4A): a catalog record printed as the `tool`
    // line an author would have typed, for them to paste and keep. Like
    // `--assembly-report` it takes no project — a shop wall is not a program —
    // and the artifact is one line on stdout. The language gains NO grammar
    // for this, which is the point: the program still declares its own tools.
    let mut args: Vec<String> = env::args().skip(1).collect();
    let mut rest_report = false;
    let mut time_report = false;
    let mut patterns_report = false;
    let mut show_reference = false;
    let mut assembly_report = false;
    let mut tool_line = false;
    let mut show_outline = false;
    let mut show_job = false;
    let mut show_schedule = false;
    let mut show_view = false;
    let mut serve_mcp = false;
    let mut review = false;
    let mut check: Option<CheckRender> = None;
    // `--help` / `-h` before anything reads a path (Phase 50 slice 1): it
    // used to fall through to the project reader and be blamed as a file.
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", usage_text());
        return;
    }
    args.retain(|a| {
        if a == "--patterns" {
            patterns_report = true;
            return false;
        }
        if a == "--reference" {
            show_reference = true;
            return false;
        }
        if a == "--time-report" {
            time_report = true;
            return false;
        }
        if a == "--review" {
            review = true;
            return false;
        }
        if a == "--rest-report" {
            rest_report = true;
            false
        } else if a == "--mcp" {
            serve_mcp = true;
            false
        } else if a == "--check" {
            check = Some(CheckRender::Prose);
            false
        } else if a == "--check=json" {
            check = Some(CheckRender::Json);
            false
        } else if a.starts_with("--check=") {
            // An unknown rendering must reject by name here: left in `args`
            // it would reach the path reader, which then blames a missing
            // file for what is a malformed command line.
            eprintln!("error: --check renders prose (bare) or json (--check=json) — `{}` names neither", a);
            process::exit(2);
        } else if a == "--assembly-report" {
            assembly_report = true;
            false
        } else if a == "--tool-line" {
            tool_line = true;
            false
        } else if a == "--outline" {
            show_outline = true;
            false
        } else if a == "--job" {
            show_job = true;
            false
        } else if a == "--schedule" {
            show_schedule = true;
            false
        } else if a == "--view" {
            show_view = true;
            false
        } else {
            true
        }
    });
    // `--template <feature>` is the click-a-face text, CLI-side (Phase 6,
    // slice 5): the suggested `run` skeleton(s) for one declared feature —
    // the same lines the window's template panel shows, printed. It takes a
    // VALUE, so it is pulled out by position rather than in the flag retain
    // above.
    // Repeated, it is a usage error and says so: taking the first and
    // leaving the second behind would hand the literal "--template" to the
    // path reader, which then blames a missing file for what is a
    // malformed command line.
    let mut template_feature: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--template") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --template needs a feature name (nacs --template <feature> <project>)");
            process::exit(2);
        }
        let name = args.remove(pos);
        if template_feature.is_some() {
            eprintln!("error: --template takes one feature name — pass it once");
            process::exit(2);
        }
        template_feature = Some(name);
    }
    // `--engagement <op>` is the immersion field's terminal voice (Phase 41
    // slice 2): the floor op `[n]` (the `--outline` index) and what the tool
    // would meet at every place it can stand on that floor, before the op
    // cuts — the line, then the map. Takes a VALUE, pulled by position like
    // `--template`; a value that is not an index says so here rather than
    // reaching the path reader.
    // `-o <file>` / `--output <file>` (Phase 50 slice 2, the way out): the
    // program to a file through the one atomic rename, stdout left empty —
    // the default mode's bytes, pinned. Pulled by position like
    // `--template`; judged against the other modes once they are known.
    let mut output: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "-o" || a == "--output") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: -o needs a file to write (nacs <project> -o <file>)");
            process::exit(2);
        }
        let file = args.remove(pos);
        if output.is_some() {
            eprintln!("error: -o takes one file — pass it once");
            process::exit(2);
        }
        output = Some(file);
    }
    // `--init '<Box|Cylinder> key=value…' <dir>` (Phase 50 slice 3, the way
    // in): a main.part from nothing, every value shown before it is
    // written. Pulled by position like `--template`.
    // `--import <model.step> [--up <axis>] [--solid N] <dir>` (Phase 50
    // slice 10): the sidecar run by its known path into the project directory.
    let mut import_step: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--import") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --import needs a STEP file (nacs --import <model.step> [--up <axis>] [--solid N] [--allowance <mm>] [--given <project>] <project>)");
            process::exit(2);
        }
        import_step = Some(args.remove(pos));
    }
    let mut import_up: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--up") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --up needs an axis (x+, x-, y+, y-, z+ or z-)");
            process::exit(2);
        }
        import_up = Some(args.remove(pos));
    }
    let mut import_solid: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--solid") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --solid needs a number (which solid is the part, S1..)");
            process::exit(2);
        }
        import_solid = Some(args.remove(pos));
    }
    let mut import_given: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--given") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --given needs the earlier program's project (the path as `given from` will name it from the new project's folder)");
            process::exit(2);
        }
        import_given = Some(args.remove(pos));
    }
    let mut import_allowance: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--allowance") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --allowance needs a size in mm (how far the blank stands outside the model; 0 for a blank cut to size)");
            process::exit(2);
        }
        import_allowance = Some(args.remove(pos));
    }
    let mut init_spec: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--init") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --init needs a spec (nacs --init 'Box w=100 l=60 h=12' <dir>)");
            process::exit(2);
        }
        let spec = args.remove(pos);
        if init_spec.is_some() {
            eprintln!("error: --init takes one spec — pass it once");
            process::exit(2);
        }
        init_spec = Some(spec);
    }
    let mut engagement_op: Option<usize> = None;
    while let Some(pos) = args.iter().position(|a| a == "--engagement") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --engagement needs an operation index (nacs --engagement <n> <project>; nacs --outline lists them)");
            process::exit(2);
        }
        let v = args.remove(pos);
        let Ok(n) = v.parse::<usize>() else {
            eprintln!("error: --engagement takes an operation index, the [n] of nacs --outline — `{v}` is not one");
            process::exit(2);
        };
        if engagement_op.is_some() {
            eprintln!("error: --engagement takes one operation index — pass it once");
            process::exit(2);
        }
        engagement_op = Some(n);
    }
    // `--arc 'x1,y1 x2,y2 tangent=<feature|stock> [offset=<mm>] [pick=<n>]'`
    // is the boundary construction's terminal voice (Phase 41 slice 3): the
    // arcs through two points tangent to a declared footprint, every
    // solution named, the picked one as loop lines to paste. Takes a VALUE,
    // one quoted argument like `--new`'s answer set.
    let mut arc_request: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--arc") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --arc needs a request (nacs --arc 'x1,y1 x2,y2 tangent=<feature|stock> [offset=<mm>] [pick=<n>]' <project>)");
            process::exit(2);
        }
        let v = args.remove(pos);
        if arc_request.is_some() {
            eprintln!("error: --arc takes one request — pass it once");
            process::exit(2);
        }
        arc_request = Some(v);
    }
    // `--new '<Variant> <name> key=value …'` is the wizard's terminal voice
    // (Phase 19, slice 2 — DECISIONS.md "Phase 19 opens…", fork 2): the
    // answer set is DATA on the command line, one quoted argument, and the
    // statement is the artifact, on stdout, never written — the shell is
    // the review (fork 4). The "series of questions" is the refusal loop: a
    // missing answer exits naming the key and its kind, so a person or an
    // agent converges the way a prompt would walk them, reproducibly.
    // Takes a VALUE, pulled by position like `--template`.
    let mut new_feature: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--new") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --new needs an answer set (nacs --new '<Variant> <name> key=value ...', '<Kind> T<n> key=value ...' or '<Op> <feature> key=value ...' [project])");
            process::exit(2);
        }
        let v = args.remove(pos);
        if new_feature.is_some() {
            eprintln!("error: --new takes one answer set — pass it once");
            process::exit(2);
        }
        new_feature = Some(v);
    }
    // `--review <report.json>` / `--accept F1,F6,… <report.json>` are the
    // sidecar review's terminal voice (Phase 17, lane A, slice 2 —
    // DECISIONS.md "Phase 17 opens…", fork A2): the first prints the report
    // grouped by row and code with the residue beneath, the second composes
    // the accepted statements as `.part` text. Non-interactive by design —
    // a prompt is untestable and an agent cannot drive it — and a report
    // CONSUMER only: neither runs the sidecar. No project path: the report
    // is the subject. `--accept` takes a VALUE, pulled by position like
    // `--template`.
    let mut accept_list: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--accept") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --accept needs a face list (nacs --accept F1,F6 <report.json>)");
            process::exit(2);
        }
        let v = args.remove(pos);
        if accept_list.is_some() {
            eprintln!("error: --accept takes one face list — pass it once");
            process::exit(2);
        }
        accept_list = Some(v);
    }
    // `--gates` is a MODIFIER, never a mode: the gate floor (DECISIONS.md
    // "the agent surface…", fork 5A), CLI-first because humans and CI want
    // the same floor an agent session gets. `gate` escalates the three gate
    // directives to errors in whatever mode runs; `declared` is the
    // program text's own modes — today's behavior, and the launch-time
    // opt-down for `--mcp` (where the floor is otherwise unconditional).
    // Escalation-only by construction: a file-declared `gate` cannot be
    // lowered by any spelling of this flag.
    // `--theme <name|path>` is a MODIFIER for `--view` (which theme the
    // window wears — remembered in the workspace file for the opens after);
    // `--theme-check <name|path>` is a MODE: the validator's CLI voice,
    // exit nonzero on a meaning violation (Phase 12, slice 3). Both take a
    // value, so both are pulled by position like `--template`.
    let mut theme_arg: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--theme") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --theme needs a theme name or path (nacs --view --theme <name|file.json> <project>)");
            process::exit(2);
        }
        let v = args.remove(pos);
        if theme_arg.is_some() {
            eprintln!("error: --theme takes one theme — pass it once");
            process::exit(2);
        }
        theme_arg = Some(v);
    }
    // `--frame <own|native>` is `--view`'s second modifier (Phase 57 slice
    // 1, fork 1A): which frame the window wears for this launch — its own
    // by default, the system's as the escape; the workspace file remembers
    // the switch made in the window.
    let mut frame_native: Option<bool> = None;
    while let Some(pos) = args.iter().position(|a| a == "--frame") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --frame needs a word: own or native (nacs --view --frame native <project>)");
            process::exit(2);
        }
        let v = args.remove(pos);
        frame_native = Some(match v.as_str() {
            "native" => true,
            "own" => false,
            other => {
                eprintln!("error: --frame takes own or native, not `{other}`");
                process::exit(2);
            }
        });
    }
    let mut theme_check: Option<String> = None;
    while let Some(pos) = args.iter().position(|a| a == "--theme-check") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --theme-check needs a theme name or path (nacs --theme-check <name|file.json>)");
            process::exit(2);
        }
        let v = args.remove(pos);
        if theme_check.is_some() {
            eprintln!("error: --theme-check takes one theme — pass it once");
            process::exit(2);
        }
        theme_check = Some(v);
    }
    let mut gates: Option<GateFloor> = None;
    while let Some(pos) = args.iter().position(|a| a == "--gates") {
        args.remove(pos);
        if pos >= args.len() {
            eprintln!("error: --gates needs a level: 'gate' (escalate every gate directive to errors) or 'declared' (the program text's own modes)");
            process::exit(2);
        }
        let v = args.remove(pos);
        if gates.is_some() {
            eprintln!("error: --gates takes one level — pass it once");
            process::exit(2);
        }
        gates = Some(match v.as_str() {
            "gate" => GateFloor::Gate,
            "declared" => GateFloor::Declared,
            other => {
                eprintln!("error: --gates is 'gate' or 'declared' — '{}' names neither", other);
                process::exit(2);
            }
        });
    }
    // One invocation, one artifact — the rule --assembly-report and
    // --rest-report first enforced between themselves, now over all eight
    // modes (the window is an artifact too; it just isn't stdout —
    // --check's artifact is the check result, --mcp's is the session, and
    // --tool-line's is one declaration line).
    if [check.is_some(), rest_report, time_report, patterns_report, assembly_report, tool_line, show_outline, show_job, show_schedule, show_view, serve_mcp, template_feature.is_some(), new_feature.is_some(), theme_check.is_some(), review, accept_list.is_some(), engagement_op.is_some(), arc_request.is_some()]
        .iter()
        .filter(|f| **f)
        .count()
        > 1
    {
        eprintln!("error: --check, --view, --outline, --job, --schedule, --template, --new, --rest-report, --time-report, --patterns, --assembly-report, --tool-line, --theme-check, --review, --accept and --mcp are one artifact each — pick one");
        process::exit(2);
    }
    // `--theme` dresses the window and nothing else — passed anywhere
    // else it would silently do nothing, which is a usage error, not a
    // shrug.
    if theme_arg.is_some() && !show_view {
        eprintln!("error: --theme is --view's modifier (which theme the window wears) — pass it with --view, or judge a theme with --theme-check");
        process::exit(2);
    }
    if frame_native.is_some() && !show_view {
        eprintln!("error: --frame is --view's modifier (which frame the window wears) — pass it with --view; a window already open switches its frame from Window ▸ Switch window frame");
        process::exit(2);
    }
    // The validator's voice needs no project: a theme is judged on its
    // own, before the path is even asked for.
    if let Some(choice) = theme_check {
        theme_check_mode(&choice);
    }
    // The MCP session: no project path — the verbs take theirs per call. The
    // gate floor is ON unless the human launching the server opted down;
    // that is a launch-time act, never a per-call parameter the agent could
    // pass (fork 5A).
    if serve_mcp {
        mcp_mode(!matches!(gates, Some(GateFloor::Declared)));
    }
    // The review: the report path is the one argument, asked for here so a
    // missing one is a usage error naming the report, never a project.
    if review || accept_list.is_some() {
        let Some(report_path) = args.first() else {
            match &accept_list {
                Some(_) => eprintln!("error: --accept needs a report (nacs --accept F1,F6 <report.json>)"),
                None => eprintln!("error: --review needs a report (nacs --review <report.json>)"),
            }
            process::exit(2);
        };
        review_mode(report_path, accept_list.as_deref());
    }
    // `--assembly-report` is the first nacs mode with no .cam project: the
    // cutter/holder recommender's v1 is manual-inputs by design (DECISIONS.md
    // → "the cutter/holder optimizer…"), so it consumes a catalog path plus
    // key=value inputs and returns before the project path is even asked for.
    if assembly_report {
        match assembly::report_from_args(&args) {
            Ok(r) => print!("{}", r),
            Err(e) => {
                eprintln!("error: {}", e);
                process::exit(if e.starts_with("usage:") { 2 } else { 1 });
            }
        }
        return;
    }
    // The clipboard, the same shape: a catalog and a name in, one `tool` line
    // out, no project path ever asked for.
    if tool_line {
        match assembly::tool_line_from_args(&args) {
            Ok(r) => print!("{}", r),
            Err(e) => {
                eprintln!("error: {}", e);
                process::exit(if e.starts_with("usage:") { 2 } else { 1 });
            }
        }
        return;
    }
    // The wizard runs before the path is REQUIRED: from nothing, the
    // statement-level check is the honest whole; with a project, the whole
    // project is re-parsed in memory with the statement appended (fork 3),
    // so validate and the parse-time laws refuse before anything reads as
    // accepted.
    // The reference needs no project (Phase 50 slice 8): the language and
    // the window's acts, generated from the tables they are read from.
    if show_reference {
        print!("{}", nacs::view::reference_text());
        return;
    }
    if let Some(spec) = new_feature {
        new_mode(&spec, args.first().map(String::as_str));
    }
    // `-o` writes the program, which only the default mode emits: beside
    // any other mode it is a usage error, said before a file is read.
    if output.is_some()
        && (check.is_some() || show_outline || show_schedule || show_view || rest_report || time_report || patterns_report
            || review || serve_mcp || template_feature.is_some() || engagement_op.is_some() || arc_request.is_some())
    {
        eprintln!("error: -o writes the program, which only the default mode emits — drop the mode flag, or the -o");
        process::exit(2);
    }
    // `--view` with no path reopens the last project the window saved
    // (Phase 50 slice 3): the workspace file's `project` line.
    if show_view && args.is_empty() {
        match nacs::theme::load_full_workspace().project {
            Some(p) => args.push(p),
            None => {
                eprintln!("error: no project to open — nacs --view <project>; from then on the window remembers the last one, and a folder dropped on it opens too");
                process::exit(2);
            }
        }
    }
    let path_arg = args.into_iter().next().unwrap_or_else(|| {
        eprint!("{}", usage_text());
        process::exit(2);
    });

    if let Some(spec) = init_spec {
        init_mode(&spec, &path_arg);
    }
    if let Some(step) = import_step {
        import_mode(&step, import_up.as_deref(), import_solid.as_deref(), import_allowance.as_deref(), import_given.as_deref(), &path_arg);
    }
    // The program string is built completely before anything is printed, so a
    // failure never leaves partial G-code on stdout (AGENTS.md error convention).
    let json_check = matches!(check, Some(CheckRender::Json));
    let mut part = match load_project(&path_arg) {
        Ok(p) => p,
        Err(LoadError::Io(msg)) => {
            // The window opens anyway (Phase 50 slice 3): a directory with
            // no main.part is the empty stance, the New project form offered.
            if show_view {
                view_mode_empty(&path_arg, msg.clone(), theme_arg, frame_native);
            }
            // The job's steps answer on a folder with nothing in it: step 1.
            if show_job && !std::path::Path::new(&path_arg).join("main.part").exists() {
                job_mode(&path_arg, &nacs::view::empty_project_part(std::path::Path::new(&path_arg)), false, Some("no main.part yet"));
            }
            // An unreadable path has no check result to render, in any mode.
            eprintln!("{}", msg);
            process::exit(1);
        }
        Err(LoadError::Parse(e)) => {
            // The window opens anyway (Phase 50 slice 3): the unparsed
            // stance, the error its Problems row, the watch live.
            if show_view {
                view_mode_empty(&path_arg, format!("parse error: {}", e), theme_arg, frame_native);
            }
            // In the JSON rendering the check result is the artifact even when the
            // check result is "this does not parse": the same one-line error the
            // prose modes put on stderr, carried in the document, so a caller
            // always gets one parseable answer. No findings ran — no `Part`
            // exists to lint — and the empty list says so.
            if json_check {
                print!("{}", check_json_report(Some(&format!("parse error: {}", e)), &[], &[]));
                process::exit(1);
            }
            eprintln!("parse error: {}", e); process::exit(1);
        }
    };
    // The gate floor, before any voicing: a floor error behaves like the
    // same gate declared in the file — which rejects inside parse, before a
    // single warning line — so the second validate pass sits here, ahead of
    // the lint loop and every mode. Findings stay [] in the JSON rendering
    // for the same reason the parse error's do: this is the declared-gate
    // error's shape, one flag later.
    if matches!(gates, Some(GateFloor::Gate)) {
        parse::apply_gate_floor(&mut part);
        if let Err(e) = parse::validate(&part) {
            if json_check {
                print!("{}", check_json_report(Some(&format!("error: {}", e)), &[], &[]));
            } else {
                eprintln!("error: {}", e);
            }
            process::exit(1);
        }
    }
    // The JSON check renders the findings inside its one document, so it
    // branches before the stderr voicing: saying each finding twice in one
    // invocation would make two authorities out of one fact.
    if json_check {
        check_program_json(&part);
        return;
    }
    // Advisory lints go to stderr so stdout — the program, and therefore
    // every golden — stays byte-identical whether or not a lint fires.
    for w in parse::lint(&part) {
        eprintln!("warning: {}", w);
    }
    // Before the field work: the outline is pure `Part`, so it answers on a
    // project the voxel engine would decline to model.
    if show_outline {
        print!("{}", outline::report(&part));
        return;
    }
    if show_job {
        let refusal = nacs::check_result::check_result(&part).refusal;
        job_mode(&path_arg, &part, true, refusal.as_deref());
    }
    // The run lines for every feature no run names yet (ROADMAP item 1,
    // slice 10): the project's own tools, picked by rule, each line parsed
    // with the project before it is offered.
    if show_schedule {
        match nacs::schedule::report(&part) {
            Ok(t) => print!("{}", t),
            Err(e) => { eprintln!("error: {}", e); process::exit(1); }
        }
        return;
    }
    if let Some(name) = template_feature {
        match template::report(&part, &name) {
            Ok(t) => print!("{}", t),
            Err(e) => { eprintln!("error: {}", e); process::exit(1); }
        }
        return;
    }
    // The immersion field (Phase 41 slice 2): pure `Part` and the op-history
    // stock, so it answers before any emission — which is the point of it.
    if let Some(n) = engagement_op {
        match nacs::engagement::immersion_report(&part, n) {
            Ok(t) => print!("{}", t),
            Err(e) => { eprintln!("error: {}", e); process::exit(1); }
        }
        return;
    }
    // The boundary construction (Phase 41 slice 3): a malformed request is
    // a usage error (exit 2), no tangent arc a refusal (exit 1) — the
    // `--new` split. The artifact is loop text on stdout, never written.
    if let Some(spec) = arc_request {
        let (a, b, reference, offset, pick) = match nacs::construct::parse_request(&spec) {
            Ok(r) => r,
            Err(e) => { eprintln!("error: {}", e); process::exit(2); }
        };
        match nacs::construct::report(&part, a, b, &reference, offset, pick) {
            Ok(t) => print!("{}", t),
            Err(e) => { eprintln!("error: {}", e); process::exit(1); }
        }
        return;
    }
    if show_view {
        view_mode(part, &path_arg, theme_arg, frame_native);
        return;
    }
    if rest_report {
        match vox::rest_report(&part, None) {
            Ok(r)  => print!("{}", r),
            Err(e) => { eprintln!("error: {}", e); process::exit(1); }
        }
        return;
    }
    // The comparison report (Phase 28 slice 4): every op that works a region,
    // every pattern valid on it, as a row of things already measured. It
    // re-runs the real emitter per candidate, so a pattern that would be
    // refused says so in its own row, by the emitter's own sentence.
    if patterns_report {
        print!("{}", nacs::compare::render(&part));
        return;
    }
    // The clock (Phase 16 slice 1): per-op cutting and rapid time over the
    // emitted text, the rapid share declined by name when the machine file
    // declares no rates. A rejected program has no clock — the emission
    // error is the answer, same exit shape as the rest report's.
    if time_report {
        match nacs::cut_time::time_report(&part) {
            Ok(r)  => print!("{}", nacs::cut_time::render(&r)),
            Err(e) => { eprintln!("error: {}", e); process::exit(1); }
        }
        return;
    }
    if matches!(check, Some(CheckRender::Prose)) {
        check_program(&part);
        return;
    }
    run_program(&part, output.as_deref());
}

/// The window mode. The window opens on the check's whole check result — the
/// same `check result::check result` the `--check` modes render — so every finding
/// the terminal would print is a row in its pane and, where it names a
/// place, a mark in its viewport (Phase 11, lane A). A REJECTED program
/// opens too, in a diagnostics stance: the stock, the wires, the error on
/// top of the pane with its mark, and no toolpath, because none was
/// emitted. "A picture must not say something the CLI cannot" still cuts
/// both ways: the CLI's answer to a rejected project is the error, and the
/// window shows exactly that — with the place. The error still goes to
/// stderr and the exit code is still the error's, so a script reading
/// either sees what it always saw.
#[cfg(feature = "frontend")]
fn view_mode(part: parse::Part, title: &str, theme: Option<String>, frame_native: Option<bool>) {
    let v = nacs::check_result::check_result(&part);
    let rejected = v.refusal.clone();
    if let Some(r) = &rejected {
        eprintln!("{}", r);
    }
    if let Err(e) = nacs::frontend::run(part, v, title, theme, frame_native) {
        eprintln!("error: {}", e);
        process::exit(1);
    }
    if rejected.is_some() {
        process::exit(1);
    }
}

/// The lean build keeps the flag but not the window: same CLI surface, a
/// error that names the feature instead of "unexpected argument".
#[cfg(not(feature = "frontend"))]
fn view_mode(_part: parse::Part, _title: &str, _theme: Option<String>, _frame_native: Option<bool>) {
    eprintln!("error: this nacs was built without the frontend feature (--no-default-features); rebuild with default features to open --view");
    process::exit(1);
}

/// `--job`: the six steps from model to program and which is next, read
/// off the project folder as the window's guide reads them.
fn job_mode(dir: &str, part: &nacs::parse::Part, has_file: bool, refusal: Option<&str>) -> ! {
    let d = std::path::Path::new(dir);
    let d = if d.is_file() { d.parent().unwrap_or(d) } else { d };
    // The import's report, where this build can read one.
    #[cfg(any(feature = "frontend", feature = "agent"))]
    let (model, waiting) = match std::fs::read_to_string(d.join("report.json")).ok().and_then(|t| nacs::review::read(&t).ok()) {
        Some(report) => {
            let rejected = std::fs::read_to_string(d.join("main.part")).map(|t| nacs::review::rejected_ids(&t)).unwrap_or_default();
            let rows = nacs::review::rows(&report, part, &rejected);
            let dups = nacs::view::duplicate_proposals(&rows);
            (Some(report.source.clone()), nacs::view::proposals_waiting(&rows, &dups))
        }
        None => (None, 0),
    };
    #[cfg(not(any(feature = "frontend", feature = "agent")))]
    let (model, waiting): (Option<String>, usize) = (None, 0);
    let stock_waiting = std::fs::read_to_string(d.join("proposed_stock.part")).ok().and_then(|t| nacs::view::stock_proposal(&t, part).ok()).is_some_and(|s| !s.in_project);
    print!("{}", nacs::view::job_report(&nacs::view::job_steps(part, has_file, model.as_deref(), stock_waiting, waiting, refusal)));
    process::exit(0)
}

/// `--new`: the wizard's terminal voice (Phase 19, slice 2). The one home
/// is `template::feature_statement`; this voice only reads the answer set
/// and picks the check's depth. The statement reaches stdout ONLY on
/// success, so no partial artifact exists to leak (the `--template`
/// convention). One refusal is the wizard's own, because the parser
/// cannot make it loudly: a name the project already declares — the
/// name-keyed feature map would silently shadow one of the two (the
/// duplicate-tool-number lesson, applied to features).
fn new_mode(answer_set: &str, project: Option<&str>) -> ! {
    // A malformed answer set is a usage error (exit 2); a refused
    // statement is a refusal (exit 1) — the --assembly-report split.
    let (variant, name, answers) = match template::parse_answer_set(answer_set) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(2);
        }
    };
    // The head word decides the statement (Phase 20 slice 2, fork 6): a
    // feature variant or a tool kind — disjoint vocabularies, pinned — and
    // a word in neither is refused with both lists.
    let head = match template::head_of(&variant, &name) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
    };
    // A run line has no meaning from nothing (Phase 20 slice 3): the
    // skeleton is the FEATURE's, so the run head needs the project first;
    // the other two heads render before any project is read.
    let rendered = match head {
        template::Head::Feature => template::feature_statement(&variant, &name, &answers),
        template::Head::Tool => {
            template::parse_tool_number(&name).and_then(|n| template::tool_statement(&variant, n, &answers))
        }
        template::Head::Run => Ok(String::new()),
    };
    let mut statement = match rendered {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
    };
    match project {
        None => {
            let checked = match head {
                template::Head::Feature => parse::check_feature_statement(&statement),
                template::Head::Tool => parse::check_tool_statement(&statement),
                template::Head::Run => Err(format!(
                    "a run line is composed on a declared feature's own skeleton, so it needs the project: nacs --new '{} {} …' <project>",
                    variant, name
                )),
            };
            if let Err(e) = checked {
                eprintln!("error: {}", e);
                process::exit(1);
            }
        }
        Some(path_arg) => {
            let part = match load_project(path_arg) {
                Ok(p) => p,
                Err(LoadError::Io(msg)) => {
                    eprintln!("{}", msg);
                    process::exit(1);
                }
                Err(LoadError::Parse(e)) => {
                    eprintln!("parse error: {}", e);
                    process::exit(1);
                }
            };
            if head == template::Head::Run {
                statement = match template::run_statement(&part, &variant, &name, &answers) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("error: {}", e);
                        process::exit(1);
                    }
                };
            }
            // A duplicate feature NAME is the wizard's own refusal (the
            // parser's map would shadow it silently); a duplicate tool
            // NUMBER is the parser's own loud one, made by the re-parse.
            if head == template::Head::Feature && part.features.contains_key(&name) {
                eprintln!(
                    "error: a feature named '{}' is already declared in {} — the parser would silently shadow one of the two; pick another name",
                    name, path_arg
                );
                process::exit(1);
            }
            // Re-parse the whole project in memory with the statement
            // appended — the sources as the load resolved them (the part
            // files lead the source list), the set_field precedent.
            let main_path = &part.sources[0];
            let read = |p: &std::path::Path| {
                std::fs::read_to_string(p).unwrap_or_else(|e| {
                    eprintln!("cannot read {}: {}", p.display(), e);
                    process::exit(1);
                })
            };
            let main_src = read(main_path);
            let geom_src = part
                .sources
                .get(1)
                .filter(|p| p.file_name().is_some_and(|n| n == "geometry.part"))
                .map(|p| read(p));
            let appended = format!("{main_src}\n{statement}\n");
            if let Err(e) =
                parse::parse_project_with_base(&appended, geom_src.as_deref(), main_path.parent())
            {
                eprintln!("error: {}", e);
                process::exit(1);
            }
        }
    }
    println!("{}", statement);
    process::exit(0);
}

/// The theme validator's CLI voice (Phase 12, slice 3): read the named
/// theme, print every finding — `meaning:` findings reject (the window
/// would fall back to the built-in palette for the viewport), `chrome:`
/// findings are notes — and exit nonzero exactly when a meaning violation
/// stands, so CI can hold a shipped theme to the same floor the window
/// does. The window's theme line is a view of this voice.
#[cfg(feature = "frontend")]
fn theme_check_mode(choice: &str) -> ! {
    use nacs::theme;
    let Some(source) = theme::theme_source(choice) else {
        eprintln!("error: no config dir to look a named theme up in, and no shipped theme of that name — pass a path");
        process::exit(1);
    };
    // The file a bare name resolved to, said first and once (Phase 24
    // slice 2, fork 4C): a name reads a COPY in the user's config dir,
    // and a copy goes stale the day the repo's file changes — the four
    // shipped themes were judged REJECTED by name on the morning slice 4
    // re-tuned them, while the repo's files passed by path. The verdict
    // that follows is about this file and no other. Since Phase 51 slice
    // 5 a name the folder does not hold reads the copy SHIPPED in the
    // binary, and says so — the same reader the window uses.
    let (where_, text) = match source {
        theme::ThemeSource::File(path) => {
            println!("judging {}", path.display());
            match std::fs::read_to_string(&path) {
                Ok(t) => (path.display().to_string(), t),
                Err(e) => {
                    eprintln!("error: cannot read {}: {e}", path.display());
                    process::exit(1);
                }
            }
        }
        theme::ThemeSource::Shipped(name, json) => {
            println!("judging the shipped theme '{name}' (carried in the binary; a {name}.json in the themes folder would win)");
            (format!("shipped '{name}'"), json.to_string())
        }
    };
    let candidate = match theme::theme_from_json(&text) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {where_}: {e}");
            process::exit(1);
        }
    };
    let findings = theme::validate(&candidate);
    let mut meaning = 0usize;
    let mut chrome = 0usize;
    for f in &findings {
        match f.grade {
            theme::ThemeGrade::Meaning => {
                meaning += 1;
                println!("meaning: {}", f.message);
            }
            theme::ThemeGrade::Chrome => {
                chrome += 1;
                println!("chrome: {}", f.message);
            }
        }
    }
    // A translucent fill is judged at both desks (Phase 25 slice 2, fork
    // 3); the clause appears only when a fill lets the desk through, so an
    // opaque theme's report is byte-identical to before.
    let desks = if candidate.translucent() {
        format!("; the glass (`background.appearance: {}`): every fill with an alpha judged at both desks, black and white, the floor holding at both", candidate.background_appearance.word())
    } else {
        String::new()
    };
    println!(
        "judged: every meaning slot against the ground at {}:1 (families and the error at {}:1, WCAG), the role triple and the stock's forgotten/violated against the roles at ΔE {} under normal vision and three dichromacies, the map's ground between the ground and the edge; advisory: text on the panel at {}:1, the border and the scrollbar at {}:1, the eleven families pairwise under each eye{desks}",
        theme::STROKE_CONTRAST_FLOOR, theme::TEXT_CONTRAST_FLOOR, theme::ROLE_DELTA_E_FLOOR, theme::TEXT_CONTRAST_FLOOR, theme::STROKE_CONTRAST_FLOOR
    );
    if meaning > 0 {
        println!(
            "REJECTED: {meaning} meaning violation(s) — the window would keep this theme's chrome and wear the built-in palette for the viewport"
        );
        process::exit(1);
    }
    println!("theme ok — {chrome} chrome note(s)");
    process::exit(0);
}

/// The lean build keeps the flag but not the reader — the `--view` rule.
#[cfg(not(feature = "frontend"))]
fn theme_check_mode(_choice: &str) -> ! {
    eprintln!("error: this nacs was built without the frontend feature (--no-default-features); rebuild with default features to run --theme-check");
    process::exit(1);
}

/// The sidecar review's terminal voice (`--review` / `--accept`): read the
/// report — never run the sidecar; a missing report says where one comes
/// from — then either print `review::render` or `review::compose`'s text.
/// A refusal is one line on stderr, exit 1, nothing on stdout: composing
/// half a `.part` is the half-a-program rule in a smaller room.
#[cfg(any(feature = "frontend", feature = "agent"))]
fn review_mode(report_path: &str, accept: Option<&str>) -> ! {
    use nacs::review;
    let text = match std::fs::read_to_string(report_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "error: cannot read {report_path}: {e} — a report.json is written by sidecar/import_step.py (sidecar/CONTRACT.md); --review reads one, it never runs the sidecar"
            );
            process::exit(1);
        }
    };
    let report = match review::read(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {report_path}: {e}");
            process::exit(1);
        }
    };
    match accept {
        None => {
            // The proposed stock beside the report, first, as the window's
            // Review lists it (the window shows nothing the CLI cannot print).
            let stock_path = std::path::Path::new(report_path).with_file_name("proposed_stock.part");
            if let Ok(proposal) = std::fs::read_to_string(&stock_path) {
                match nacs::view::stock_proposal(&proposal, &nacs::parse::Part::default()) {
                    Ok(s) => println!("Proposed stock ({})
{}
", stock_path.display(), s.statement),
                    Err(e) => println!("Proposed stock ({}) does not parse: {e}
", stock_path.display()),
                }
            }
            // The project's answers beside each proposal, when main.part is
            // there: what the window's rows say, printed.
            let dir = std::path::Path::new(report_path).parent().map(|d| d.to_path_buf()).unwrap_or_default();
            let main = dir.join("main.part");
            let rows = if main.is_file() {
                match nacs::parse::load_project(&dir.to_string_lossy()) {
                    Ok(part) => {
                        let rejected = std::fs::read_to_string(&main).map(|t| review::rejected_ids(&t)).unwrap_or_default();
                        review::rows(&report, &part, &rejected)
                    }
                    Err(LoadError::Io(e)) | Err(LoadError::Parse(e)) => {
                        eprintln!("note: {} does not parse ({e}); the rows are printed without the project's answers", main.display());
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            print!("{}", review::render_marked(&report, &rows));
        }
        Some(list) => {
            let answers = review::answers_from_list(list);
            if answers.is_empty() {
                eprintln!("error: --accept's face list is empty (nacs --accept F1,F6 <report.json>)");
                process::exit(2);
            }
            match review::compose(&report, &answers) {
                Ok(t) => print!("{t}"),
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            }
        }
    }
    process::exit(0);
}

/// The lean build keeps both flags but not the reader — the `--mcp` pattern:
/// same CLI surface, an error that names the features.
#[cfg(not(any(feature = "frontend", feature = "agent")))]
fn review_mode(_report_path: &str, accept: Option<&str>) -> ! {
    let flag = if accept.is_some() { "--accept" } else { "--review" };
    eprintln!("error: this nacs was built without the frontend or agent feature (--no-default-features); rebuild with default features to run {flag}");
    process::exit(1);
}

/// The two spellings of `--gates`: the floor, or the program text's own
/// modes (the explicit default — and the launch-time opt-down for `--mcp`).
#[derive(Clone, Copy, PartialEq)]
enum GateFloor {
    Gate,
    Declared,
}

/// The MCP session (`--mcp`): serve the agent surface over stdio until the
/// client hangs up. The exit code is the loop's own.
#[cfg(feature = "agent")]
fn mcp_mode(gate_floor: bool) -> ! {
    process::exit(mcp::serve(gate_floor));
}

/// The lean build keeps the flag but not the server — the `--view` pattern:
/// same CLI surface, an error that names the feature.
#[cfg(not(feature = "agent"))]
fn mcp_mode(_gate_floor: bool) -> ! {
    eprintln!("error: this nacs was built without the agent feature (--no-default-features); rebuild with default features to serve --mcp");
    process::exit(1);
}

/// `--check`: the default mode with the emission held back. The whole
/// pipeline still runs — validate, the gate directives at their declared
/// modes, generation, the entry restriction inside `gcode::program`, and
/// both post-generation advisories — because entries and field fouls exist
/// only once moves do, so a check that stopped at validate would be
/// green-but-blind on two families. `--check` reports what the pipeline
/// says; it never escalates a gate itself (the gate floor is the agent
/// session's job, a later slice). The program string is built and dropped:
/// no partial artifact exists to leak, and the exit code is the default
/// mode's exit code on the same project, by construction.
fn check_program(part: &parse::Part) {
    // The one pipeline (`check_result`): the parse-time findings were printed
    // already, so the post-generation ones follow them here, in the order the
    // families produce them, as before. On a refusal the error is the one
    // line on stderr and nothing reaches stdout.
    let v = nacs::check_result::check_result(part);
    let n_lint = parse::lint(part).len();
    for w in v.findings.iter().skip(n_lint) {
        eprintln!("warning: {}", w);
    }
    if let Some(refusal) = &v.refusal {
        eprintln!("{}", refusal);
        process::exit(1);
    }
    // The account (ROADMAP item 2, slice 6): what each family judged,
    // found, and could not judge, on stdout, which this mode left empty.
    for line in nacs::check_result::account_words(&nacs::check_result::account(part, &v)) {
        println!("{}", line);
    }
}

/// The two renderings of the check check result: today's prose (stderr, the
/// default mode's byte for byte) or one JSON document (stdout). Same
/// pipeline, same exit codes — the rendering is the only difference.
enum CheckRender {
    Prose,
    Json,
}

/// `--check=json`: the check check result as one JSON document on stdout — the
/// structs rendered, never a third source. Same pipeline as `check_program`
/// (parse-time lint first, then generation, then the two post-generation
/// families — the finding order is the order the families already produce),
/// same exit codes; the difference is only where the words go. Stderr stays
/// silent: the document is the whole report, and voicing each finding twice
/// in one invocation would make two authorities out of one fact.
///
/// The error is the one-line message the prose modes already say,
/// structured no further — the blessed entry's fork 4 priced the full
/// error envelope (stage, per-site codes) as a later, need-driven pass,
/// and this rendering does not pre-empt it.
fn check_program_json(part: &parse::Part) {
    let (doc, would_emit) = check_result_of(part);
    print!("{}", doc);
    if !would_emit {
        process::exit(1);
    }
}

/// The check pipeline with the document as the RETURN — the one home the
/// `--check=json` flag and the MCP `check` verb both render from, so the two
/// cannot drift (they are the same fact on two wires). On a generation
/// error the parse-time findings were already collected; the two
/// post-generation families never came to exist — exactly what the prose
/// modes voice on the same project.
fn check_result_of(part: &parse::Part) -> (String, bool) {
    let v = nacs::check_result::check_result(part);
    let acc = nacs::check_result::account(part, &v);
    (check_json_report(v.refusal.as_deref(), &v.findings, &acc), v.would_emit())
}

/// The whole document. Field order is fixed — `would_emit`, `refusal` (only
/// when rejected), `findings` — and the finding order is the callers': the
/// output is deterministic byte for byte on the same project.
fn check_json_report(error: Option<&str>, findings: &[parse::Finding], checked: &[nacs::check_result::FamilyAccount]) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"would_emit\": {},\n", error.is_none()));
    if let Some(r) = error {
        out.push_str(&format!("  \"refusal\": {},\n", json_string(r)));
    }
    if findings.is_empty() {
        out.push_str("  \"findings\": [],\n");
    } else {
        out.push_str("  \"findings\": [\n");
        for (i, f) in findings.iter().enumerate() {
            out.push_str(&finding_json(f, i + 1 == findings.len()));
        }
        out.push_str("  ],\n");
    }
    // The account (ROADMAP item 2, slice 6): one object per family, in the
    // window's order — what ran, what it found, what it could not judge,
    // and why it had nothing to judge where it had none. Empty where no
    // project was parsed.
    if checked.is_empty() {
        out.push_str("  \"checked\": []\n");
    } else {
        out.push_str("  \"checked\": [\n");
        for (i, a) in checked.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"family\": \"{}\", \"ran\": {}, \"found\": {}, \"declined\": {}, \"idle\": {}}}{}\n",
                nacs::check_result::family_word(a.family),
                a.ran(),
                a.found,
                a.declined,
                a.idle.as_deref().map_or("null".to_string(), json_string),
                if i + 1 == checked.len() { "" } else { "," }
            ));
        }
        out.push_str("  ]\n");
    }
    out.push_str("}\n");
    out
}

/// One finding: family, subject, message — the struct's three fields in the
/// struct's order, nothing invented. The subject keeps its two shapes as a
/// tagged object: `{"kind": "op", "index": …, "feature": …}` for the
/// op-judging families, `{"kind": "feature", "name": …}` for the
/// containment lint's declaration-only voice.
fn finding_json(f: &parse::Finding, last: bool) -> String {
    let family = nacs::check_result::family_word(f.family);
    let subject = match &f.subject {
        parse::FindingSubject::Op { index, feature } => format!(
            "{{\"kind\": \"op\", \"index\": {}, \"feature\": {}}}",
            index,
            json_string(feature)
        ),
        parse::FindingSubject::Feature { name } => {
            format!("{{\"kind\": \"feature\", \"name\": {}}}", json_string(name))
        }
    };
    format!(
        "    {{\n      \"family\": \"{}\",\n      \"subject\": {},\n      \"message\": {}\n    }}{}\n",
        family,
        subject,
        json_string(&f.message),
        if last { "" } else { "," }
    )
}

/// A JSON string literal, quotes included. Hand-rolled over std on purpose
/// (DECISIONS.md "--check=json — the verdict as data…"): emitting this
/// small, flat, known shape is not the consuming that pre-blessed
/// `serde_json`. Escaping is not optional — a feature NAME can carry `"`
/// and `\` (any non-whitespace token parses), and the messages quote names.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::json_string;

    /// The escaper, at the unit level: every class the writer must survive —
    /// quote, backslash, the named control escapes, and the \u00XX fallback
    /// for the rest of the control range. (The CLI-level escaping pin lives
    /// in tests/check_tests.rs, through a feature name carrying `"` and `\`.)
    #[test]
    fn test_json_string_escapes_every_class() {
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(json_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_string("a\nb\tc\rd"), "\"a\\nb\\tc\\rd\"");
        assert_eq!(json_string("a\u{1}b"), "\"a\\u0001b\"");
        // Non-ASCII passes through as UTF-8, unescaped — JSON is UTF-8 text.
        assert_eq!(json_string("5µm — off"), "\"5µm — off\"");
    }
}

/// The default mode: emit the program (or reject), voicing the post-
/// generation advisories. Split out so `--view` shares nothing with it but
/// the parse — the window mode has its own error shape.
fn run_program(part: &parse::Part, output: Option<&str>) {
    match gcode::program_with_moves(part) {
        Ok((g, emitted_moves)) => {
            // The plunge taxonomy's entry flag (DECISIONS.md "the plunge
            // taxonomy") reads the finished emitted moves — entries exist only once
            // moves do, so this is the one lint voiced after generation.
            // Stderr like every advisory; stdout stays byte-identical.
            for w in stock::entry_findings(part, &emitted_moves) {
                eprintln!("warning: {}", w);
            }
            // Reach shortfall (DECISIONS.md → "K2, the tool body", fork 4c):
            // the assembly shorter than the depth it works in. Reads the emitted moves
            // for the depth and nothing else — no material model, so it speaks
            // over an already-open cavity where the foul voices are silent.
            for w in stock::reach_findings(part, &emitted_moves) {
                eprintln!("warning: {}", w);
            }
            // The travel half of the machine envelope (Phase 15 slice 1):
            // emitted positions carried into machine coordinates by the
            // declared datum, against the machine file's declared spans.
            // Speaks only when both homes declare.
            for w in stock::envelope_findings(part, &emitted_moves) {
                eprintln!("warning: {}", w);
            }
            for w in nacs::fixture::fixture_findings(part, &emitted_moves) {
                eprintln!("warning: {}", w);
            }
            // The foul lint's field voice (DECISIONS.md "the profile
            // silhouette…", slice 3): the shank standing in stock nothing
            // removed, at the positions the tool actually visits. Gated on a
            // truncated silhouette, so a program without a form cutter pays
            // nothing to ask.
            for w in vox::body_foul_findings(part, &emitted_moves) {
                eprintln!("warning: {}", w);
            }
            // The walk's arm of the engagement family (Phase 38 slice 2):
            // an op that is mostly a slot, read from the moves it made.
            for w in nacs::engagement::slot_findings(part, &emitted_moves) {
                eprintln!("warning: {}", w);
            }
            for w in stock::move_declines(part, &emitted_moves) {
                eprintln!("warning: {}", w);
            }
            // The program: stdout, or the file `-o` named — the same bytes
            // (`view::program_text`, the one trailing newline), the file
            // through the atomic rename every write takes, so a reader never
            // sees half a program (the error convention, kept on disk).
            match output {
                None => println!("{}", g),
                Some(file) => {
                    let path = std::path::Path::new(file);
                    if let Err(e) = nacs::theme::write_atomically(path, &nacs::view::program_text(&g)) {
                        eprintln!("error: {}", e);
                        process::exit(1);
                    }
                }
            }
        }
        Err(e) => { eprintln!("error: {}", e); process::exit(1); }
    }
}

/// The usage, grouped by what a mode is FOR (Phase 50 slice 1): one home for
/// `--help` / `-h` (stdout, exit 0) and the no-argument refusal (stderr,
/// exit 2). Before this `--help` reached the path reader and was blamed as a
/// missing file.
fn usage_text() -> String {
    [
        "usage: nacs [mode] [--gates gate|declared] <project_directory | file.part>",
        "",
        "A project is a directory holding main.part (and, optionally, geometry.part).",
        "With no mode the G-code program is written to stdout, findings to stderr:",
        "    nacs plate.cam > plate.nc          or          nacs plate.cam -o plate.nc",
        "    -o, --output <file>                      the program to a file instead of stdout",
        "",
        "Look",
        "    --view [--theme <name|file.json>] [--frame own|native]",
        "                                             open the window on the project (no path: the last one opened)",
        "    --outline                                stock, features and operations in run order",
        "    --job                                    the six steps from model to program, and which is next",
        "    --check[=json]                           validate and lint; nothing printed means nothing found",
        "Measure",
        "    --time-report                            the cut-time model, per operation and whole",
        "    --rest-report                            what the voxel field says was left uncut",
        "    --patterns                               every pattern through the real emitter, measured",
        "    --engagement <op>                        the immersion ceiling over an operation's floor",
        "Author",
        "    --template <feature>                     the run skeleton(s) for one declared feature",
        "    --reference                              the .part language and the window's acts, generated (REFERENCE.md)",
        "    --new '<Variant> <name> key=value...'    compose a feature statement, checked by the parser",
        "    --new '<Kind> T<n> key=value...'         compose a tool statement",
        "    --new '<Op> <feature> key=value...'      compose an operation",
        "    --arc 'x1,y1 x2,y2 tangent=<feature|stock> [offset=<mm>] [pick=<n>]'",
        "                                             a boundary arc through two points, as loop text",
        "    --init '<Box|Cylinder> key=value...' <dir>   write a new project's main.part, every value shown",
        "Import",
        "    --import <model.step> [--up <axis>] [--solid N] [--allowance <mm>] [--given <project>] <dir>
                                             run the sidecar into the project directory (report.json beside main.part);
                                             the blank stands 1 mm outside the model unless --allowance says (0: cut to size);
                                             --given names the program before this one: the blank is its stock turned over, the given from line written",
        "    --review <report.json>                   the sidecar's proposals, as the window reviews them",
        "    --accept F1,F6,... <report.json>         print the accepted proposals as .part text",
        "    --schedule <project>                     a run line for every feature no run names yet, tools from the project's own tool lines",
        "Tools",
        "    --assembly-report <catalog> key=value... fit a cutter and holder from a catalog",
        "    --tool-line <catalog> <cutter> [holder=<name>] [stickout=<mm>]",
        "                                             the tool line a catalog record would be",
        "Window",
        "    --theme-check <name|file.json>           judge a theme file",
        "Agents",
        "    --mcp                                    serve these verbs over stdio (AGENT.md)",
        "",
        "    --help, -h                               this text",
        "",
    ]
    .join("\n")
}

/// The window on a path that holds no parsed project (Phase 50 slice 3):
/// a directory with no `main.part`, or a file that does not parse — the
/// empty stance, the reason the Problems row, the watch following the
/// files so the file appearing or being fixed reloads it. Exits 1 at
/// close, as a rejected open always has.
#[cfg(feature = "frontend")]
fn view_mode_empty(path_arg: &str, reason: String, theme: Option<String>, frame_native: Option<bool>) -> ! {
    eprintln!("{}", reason);
    let part = nacs::view::empty_project_part(std::path::Path::new(path_arg));
    let cr = nacs::check_result::CheckResult { refusal: Some(reason), findings: Vec::new(), emitted_moves: None };
    if let Err(e) = nacs::frontend::run(part, cr, path_arg, theme, frame_native) {
        eprintln!("error: {}", e);
    }
    process::exit(1);
}

#[cfg(not(feature = "frontend"))]
fn view_mode_empty(_path_arg: &str, _reason: String, _theme: Option<String>, _frame_native: Option<bool>) -> ! {
    eprintln!("error: this nacs was built without the frontend feature (--no-default-features); rebuild with default features to open --view");
    process::exit(1);
}

/// `--init`: the project's first file from the terminal (Phase 50 slice
/// 3). The one home is `template::project_text`; the one parser judges the
/// text before it is written; the directory is made; stdout carries the
/// text written (shown, never silent); an existing main.part refuses by
/// name (exit 1); a malformed spec is a usage error (exit 2).
fn init_mode(spec: &str, dir_arg: &str) -> ! {
    let (kind, answers) = match template::parse_init_spec(spec) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(2);
        }
    };
    let dir = std::path::Path::new(dir_arg);
    let main = dir.join("main.part");
    if main.exists() {
        eprintln!("error: {} exists — a project is edited, not written over", main.display());
        process::exit(1);
    }
    let text = match template::project_text(&kind, &answers).and_then(|t| parse::parse(&t).map(|_| t)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {}", e);
            process::exit(1);
        }
    };
    if let Err(e) = std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display())).and_then(|()| nacs::theme::write_atomically(&main, &text)) {
        eprintln!("error: {}", e);
        process::exit(1);
    }
    eprintln!("written: {}", main.display());
    print!("{}", text);
    process::exit(0);
}

/// `--import`: the sidecar from the terminal (Phase 50 slice 10). The
/// command is `view::import_command`'s, judged before it runs — no sidecar
/// in reach, no venv, not a STEP, not an axis: a sentence, exit 1 (the
/// axis's shape exit 2) — then run and waited for, its summary on stderr,
/// `report.json` beside the project. A directory with no main.part yet is
/// told what it still needs.
fn import_mode(step: &str, up: Option<&str>, solid: Option<&str>, allowance: Option<&str>, given: Option<&str>, dir_arg: &str) -> ! {
    // The allowance unsaid is the setup's own default, one home (view.rs).
    let mut setup = nacs::view::ImportSetup { up: up.map(str::to_string), solid: solid.unwrap_or("").to_string(), given: given.unwrap_or("").to_string(), ..Default::default() };
    if let Some(a) = allowance {
        setup.allowance = a.to_string();
    }
    if let Err(e) = setup.args() {
        eprintln!("error: {e}");
        process::exit(2);
    }
    let Some(sidecar) = nacs::view::sidecar_dir() else {
        eprintln!("error: no sidecar in reach — run from the repo root (./sidecar), or set NACS_SIDECAR to the sidecar directory");
        process::exit(1);
    };
    let dir = std::path::Path::new(dir_arg);
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("error: create {}: {e}", dir.display());
        process::exit(1);
    }
    match nacs::view::run_import(&sidecar, std::path::Path::new(step), dir, &setup) {
        Ok(summary) => {
            eprintln!("{summary}");
            eprintln!("written: {}", dir.join("report.json").display());
            // The blank beside the features: the model's own outline for a
            // prism, the smallest blank it fits otherwise, the file saying
            // which. Err is the sidecar failing, not a decline.
            match nacs::view::run_stock(&sidecar, std::path::Path::new(step), dir, &setup) {
                Ok(line) => {
                    eprintln!("{line}");
                    eprintln!("written: {}", dir.join("proposed_stock.part").display());
                }
                Err(e) => eprintln!("no stock proposed: {e}"),
            }
            if !dir.join("main.part").is_file() {
                eprintln!("no main.part yet in {} — nacs --view {} reviews the proposals; accepting the stock makes main.part, or nacs --init '<Box|Cylinder> key=value…' {} writes one by hand", dir.display(), dir_arg, dir_arg);
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}
