# nacs: agent guide

Read this, then `README.md` (why the project exists), `DECISIONS.md` (what is
settled), `ROADMAP.md` (what is next). `PRINCIPLES.md` says how a feature
should behave for the person using it. `CHECKLIST.md` says which surfaces the
engine handles today. `REFERENCE.md` is the grammar and the window's acts,
generated from the code with `nacs --reference`.

## What this is

A text-driven CAM toolpath generator in Rust. A project is a folder of plain
text files, so it diffs and reviews like code. The binary is `nacs`. The
engine depends on one geometry crate; the optional native window adds
`eframe`. Full dependency list with reasons: the `[dependencies]` block in
`Cargo.toml`.

It is a solo, for-fun project. Prefer the simple readable solution. A 10-line
approximation with an honest comment beats a 40-page algorithm.

## The map

| Where | What |
|---|---|
| `src/parse/` | The `.part` grammar (`grammar.rs`), `Part` model (`model.rs`, `tool.rs`), `validate.rs`, the `derive_*()` homes (`derive.rs`, `surface.rs`, `revolve.rs`), findings, feeds, the SVG profile reader |
| `src/gcode.rs` | Number formatting and `program()`, which dispatches each `run` verb to an emitter |
| `src/ops/` | Emitters by surface math: `floor`, `profile`, `clear`, `drill`, `finish` |
| `src/offset.rs`, `region.rs`, `pattern.rs`, `spine.rs` | 2D offsetting wrapper, the floor an op works, the five floor patterns |
| `src/stock.rs`, `vox.rs` | Declared stock model and voxel swept field |
| `src/engagement.rs`, `compare.rs`, `cover.rs` | Engagement measures and the `--patterns` report |
| `src/construct.rs`, `nurbs.rs`, `mesh.rs` | Arc construction, analytic NURBS meridian, STL reader |
| `src/assembly.rs` | Cutter and holder optimizer (advisory report) |
| `src/outline.rs`, `view.rs`, `template.rs`, `theme.rs` | View models, always compiled, tested headless |
| `src/frontend.rs` | The eframe window. Thin. If a fact on screen is wrong, the bug is in `view.rs` |
| `src/review.rs`, `mcp.rs`, `main.rs` | Sidecar report reader, MCP serve loop, CLI |
| `sidecar/` | Python CadQuery/OCP STEP import and recognition; `CONTRACT.md` is the seam |
| `*.cam/` | Fixture projects; each has a golden in `tests/` |
| `tests/` | Integration tests; `check_tests.rs` holds the goldens and the safety lint |

Run it:

```sh
cargo run -- plate.cam              # G-code on stdout
cargo run -- --view plate.cam       # the window
cargo test                          # everything, about 15 minutes
cargo test --test given_from_tests  # the loop: the file you touched
./lock.sh                           # the whole gate by hand; CI runs it for you
```

Every push runs `./lock.sh` on Linux and Windows (`.github/workflows/lock.yml`);
a branch merges when both are green. That run is the gate and takes about
17 minutes. Push, then read it with `gh pr checks --watch`. Do not run
`./lock.sh` first: it is the same tests, slower (30 minutes on Windows, over
an hour in a cloud session). `rust-toolchain.toml` pins the compiler.

On Windows run scripts from Git Bash. If a window is open on
`target/debug/nacs.exe`, build with `CARGO_TARGET_DIR=target/agent`. Never
run two cargo commands at once. Run sidecar scripts with
`sidecar/.venv/Scripts/python.exe`.

On Linux (a cloud session, WSL) `sh setup.sh` installs everything the lock
needs; a cloud session runs it by itself at start and logs to
`target/setup.log`. The sidecar's Python is `sidecar/.venv/bin/python`.
`.claude/settings.json` says which commands run without asking. It allows
`gh pr merge` and refuses a push to `main`: work reaches `main` only through
a PR whose CI run is green.

## Rules

**Write plainly.** Commit messages: one line, then at most five lines of body.
PR descriptions the same. Code comments say why, not what. A coined term
(shore, lens, comb, band, walk) must be defined in the sentence that uses it
or not used. The person reading this is a machinist and a programmer, not a
member of the house.

**Decisions are short and rare.** Add a `DECISIONS.md` entry, under ten lines,
when a change alters what the tool does to metal, what the grammar means,
what the G-code says, or what the program depends on. Nothing else goes
there.

**Propose before doing** only for: a new dependency, a change to G-code
semantics beyond formatting, a change to `.part` syntax, deleting a feature.
Everything else: do it, test it, open a PR.

**No ceremony.** No DRAFT PRs, forks, blessings, CLOSE entries or walks. One
branch per change, one PR, merged by the human or, once CI is green, by the
agent. If you want the user to check
something in the window, run it yourself first and say exactly what to click
and what they should see.

**Tests.** While working, run the test files for what you touched. Then push
and let CI run the lock; report its result, not a local one.
Every new op ships with a fixture and a golden. A test asserts behaviour, not
prose; do not add tests that pin documentation text or count fixtures. When
you change output on purpose, regenerate the goldens in the same commit and
say which lines moved and why.

**Derived values have one home.** Anything computed from `Part` is a
`derive_*()` function. Never cache or copy one.

**Math is portable.** In the engine write `x.psin()`, `y.patan2(x)` and so on
(`src/math.rs`), never std's `sin`/`cos`/`atan2`/`hypot`: those differ between
Windows and Linux in the last bit and move goldens.

**Errors.** Parse and geometry failures return `Result` with file and line.
The CLI exits nonzero with one line on stderr and writes nothing to stdout.
An op that cannot be machined as declared is an error, never a comment or a
skipped op.

**Safety lint.** Units before motion, `G43 H` before a tool's first cut,
retract to `clearance_z` between ops, spindle before feed. One test,
`test_gcode_safety_lint`; add each new fixture to its list.

**The window.** A thin consumer of view models. Anything it shows must be
printable from the CLI. `cargo test --no-default-features` stays green. No
further UI crates without proposing first.

**A window change is shown, not described.** A PR that changes what the
window looks like carries pictures: the same view before and after, taken by
a shell test (`shot()` writes `target/shots/<name>.png`) and published with
`./shots.sh <name>...`, which prints the lines to paste into the PR. When the
design is an open question, build two or three variants, shoot each, and let
the user pick from the pictures.

**Keep the project a folder of readable text.** Any new file format must be
diffable and hand-editable.

**No telemetry, no network calls.**

**Update `ROADMAP.md` and `CHECKLIST.md` in the same change** that lands the
work.

## Glossary

Terms the code and the older docs use. Keep to these; do not coin more.

- **Blank**: the stock before any op. **Floor**: the planar region an op
  works. **Wall / open / bridged edge**: a floor edge against standing
  material, against air past the blank, or across a void the tool spans.
- **Island**: a feature standing inside a floor, derived by containment.
  **Shore**: the standing material a row pattern's closing pass runs along.
- **Spine**: the default floor pattern (rows where they own floor, one shore
  pass). **Comb**: spine's set of candidate row directions. **Rail**: one pass
  down a narrow floor's centreline.
- **Held**: spine at the spacing an engagement ceiling gives, lifting where a
  link would exceed it.
- **Given**: a void cut on another setup, read as air. **Lens**: the plan
  region where a whole tool body fits inside a given void.
- **Band / walk / immersion**: engagement per pass, from moves, before a path.
- **Declared truth / swept truth**: the analytical stock model / the voxel
  field. **Rest report**: where they disagree.
- **Decline**: a check saying it cannot judge a case, by name.
