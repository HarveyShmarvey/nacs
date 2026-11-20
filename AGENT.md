# The nacs MCP contract (v1)

This file is the canonical definition of the wire an agent client speaks to
`nacs --mcp` — the protocol subset, the tools, and their exact shapes. The
*reasoning* lives in `DECISIONS.md` ("the agent surface — every verb is a
flag first…"); this file is the spec, `sidecar/CONTRACT.md`'s sibling in
spirit. It is NOT the agent guide — that is `AGENTS.md`, which stays the
canonical operator's manual; a terminal agent with no MCP reads it directly
and loses nothing, because every tool below wraps a CLI flag.

## Invocation

```
nacs --mcp [--gates declared]
```

- stdio only, forever. MCP defines HTTP/SSE transports; this server will
  never implement them — the no-network line is unchanged.
- No project path at launch: the tools name their paths per call, resolved
  against the server's working directory.
- **Stateless.** Every call re-reads the project from disk. The folder of
  text is the truth; there is no session cache to drift, no state carried
  between calls, and no handshake order enforced.
- **The gate floor is ON by default** (see below). `--gates declared` is the
  launch-time opt-down — a human act in the config that wires the agent,
  never a per-call parameter.
- A build without the `agent` feature (`--no-default-features`) rejects the
  flag by name, exit 1.
- Exit 0 on EOF (the client hanging up is the clean shutdown).

## Wire

JSON-RPC 2.0, newline-delimited UTF-8: one message per line in, one per
line out. No batch arrays (removed in the MCP revision this server speaks).
`serde_json` parses the requests; every response is hand-rendered, so
response bytes are deterministic for a given request and project state.

Declared protocol revision: **`2025-06-18`** — `initialize` always answers
this version, whatever the client offered. Spec drift is absorbed by
revving this document, not by tracking an SDK.

Methods, exhaustively:

| method                      | answer                                                        |
|-----------------------------|---------------------------------------------------------------|
| `initialize`                | `protocolVersion`, `capabilities: {tools: {}}`, `serverInfo`, `instructions` (the operator's manual, distilled from `AGENTS.md`) |
| `ping`                      | `{}`                                                          |
| `tools/list`                | the six tools below, static (no `listChanged` notifications) |
| `tools/call`                | the tool's result (below)                                     |
| anything else               | JSON-RPC error `-32601`                                       |

Messages without an `id` are notifications and are answered by silence,
whatever their method. Protocol faults use JSON-RPC's own vocabulary:
`-32700` (not JSON), `-32600` (not a valid request), `-32601` (unknown
method), `-32602` (bad tool name or arguments — including any argument key
a tool does not read: unknown keys reject by name, the tool line's law one
wire up).

## Tool results

A successful call returns one text content block — **the exact bytes the
wrapped CLI mode would put on stdout** — and an explicit `isError` both
ways:

```json
{"content":[{"type":"text","text":"…"}],"isError":false}
```

A failed call (`isError: true`) carries the **error envelope** instead:

```json
{
  "stage": "…",
  "message": "…"
}
```

`message` is verbatim the one-line error the CLI would print on stderr
(prefix included). `stage` names how far the pipeline got: `io` (an
unreadable path — the one class with no check result to render), `parse`
(parsing + the program's own declared-mode validation, fused today),
`validate` (the gate floor's second validate pass — an error here exists
because the floor escalated it), `emit` (generation, including the entry
gate), or the reporting verb's own name (`template`, `rest_report`) for a
mode-specific error after a clean parse. Per-site error codes are
deliberately absent — the envelope structures the boundary, not the ~227
sites behind it (a future, need-driven pass).

**Exception: `check` never envelopes an engine error.** Its artifact IS
the check result, so a program that rejects at any stage is a successful check
(`isError: false`) with the error inside the document. Only `io` and
argument faults fail a `check` call.

## The gate floor

Unless launched with `--gates declared`, the server escalates the six
gate directives (`engagement`, `deviation`, `entry`, `fixture`, `reach`,
`envelope`)
from advise to gate on
**every verb whose subject is the program** before answering — the same
escalation the CLI's `--gates gate` applies, one home
(`parse::apply_gate_floor`). **`assembly_report` is the exception, and the
only one**: it ranks shelf hardware against one declared feature's geometry,
so a program's own findings are not what it is answering about — and the same
job stated as `reach`/`opening`/`depth` carries no program to floor at all.
A floor there would answer one question two ways depending on how it was
asked (`DECISIONS.md` → "`--assembly-report` as the sixth verb…", fork 2A). Escalation
only: a file-declared `gate` cannot be lowered from this surface, or any
other. Body-foul stays advisory everywhere by its own standing rationale (a
sampled probe biased to silence must not gate). Practical consequence: a
program whose findings are warnings in a human terminal rejects here —
that is the design, not a defect ("an agent reads an error better than a
warning it can talk itself past").

## Tools

Six verbs, each wrapping a CLI mode. There is no write verb, no
send-to-machine verb, and no network anywhere — the machine boundary is
absolute because the surface physically contains no such verb.

| tool              | wraps               | arguments                              | text result                     |
|-------------------|---------------------|----------------------------------------|---------------------------------|
| `check`           | `--check=json`      | `path` XOR `text` (+ optional `dir`)   | the check check result document      |
| `emit`            | the default mode    | `path`                                 | the G-code program              |
| `outline`         | `--outline`         | `path`                                 | the project tree report         |
| `template`        | `--template`        | `path`, `feature`                      | the run skeleton(s)             |
| `rest_report`     | `--rest-report`     | `path`                                 | the rest-material report        |
| `patterns`        | `--patterns`        | `path`                                 | the pattern comparison: each walking pattern's measured consequences, per region-working op, in four groups — COVER (`uncut`, passes across each land), COST (`at depth`, `entries`, `time`, entries and turns by kind), WHERE (`on floor`, `other stock`, `air`), LOAD (`past 90°`, `slot`, `worst (band)`); every `%` a share of `at depth` — never a score; pin with `pattern=` on the run line |
| `assembly_report` | `--assembly-report` | `catalog`, then `path`+`feature` XOR `reach`/`opening`/`depth`; optional `clearance`, `load`, `material`, `feeds` | the ranked assemblies |

`path` is a project directory (`main.part` + optional `geometry.part`) or a
bare `.part` file — the CLI's own argument, same resolution.

### check — and the propose-an-edit source

`check` returns the same JSON document `nacs --check=json` prints (see that
entry in `DECISIONS.md` for the document's shape: `would_emit`, `refusal`
when rejected, `findings` as `{family, subject, message}` in the order the
families produce them). **A gate-floor error reports the findings the
pipeline had already produced when it stopped, which is often none:** the
`engagement` and `deviation` gates reject at validate, before any move
exists, so the document is `findings: []` — the declared-gate error's
shape, one wire up. The `entry`, `fixture` and `reach` gates reject during
generation, so whatever earlier families found (body-foul, say) is in the
list beside the error. *(Corrected 2026-08-20: this paragraph read "a
gate-floor error reports `findings: []`" flatly, which is only the
validate-stage half.)* Since Phase 38 slice 2 the `engagement` family has
a second arm — an op that is MOSTLY A SLOT, read from the emitted moves
after generation — which the `engagement` gate does not escalate (fork
5A): it is in `findings` beside the post-generation families, never in
`refusal`.

With `text` instead of `path`, `check` is the **dry-run evaluator** — the
propose-an-edit verb, enforced by shape: it takes the whole candidate
`main.part` content the agent intends and returns the engine's full check result
without touching disk. Nothing needs to be landed to find out what the
engine thinks of it. Landing the edit stays the agent's own file tools
under the user's normal diff review. Optional `dir` names the project
directory the candidate is FOR: its `geometry.part` and relative assets
(SVG profiles, `machine` files, `.feeds` tables) resolve from there;
without `dir`, the candidate is judged alone and relative assets resolve
against the server's working directory.

### assembly_report — the verb that is not about the program

Ranks cutter+holder assemblies from a catalog against one job: projection,
L/D, deflection, rpm, and **the exact `tool` line to declare each one**. That
line is the point of the verb on this surface — an agent lifts it, puts it in
a candidate `main.part`, and calls `check` with `text`, which is the whole
read → propose → re-check loop with nothing landed in between.

State the job one of two ways, never both (a fact in two places is an error
here, as everywhere in this project):

- **`path` + `feature`** — read off the declaration. `path` means what it
  means for every other verb: a project directory or a bare `.part` file,
  resolved through the same home.
- **`reach` / `opening` / `depth`** — passed as numbers, no project involved.

A `path` + `feature` job reads the whole cavity, not just the feature named:
coaxial neighbours the part declares — a stepped cavity spelled as two bores on
one axis — are composed into one corridor, and the header names which ones and
the condition (*the room above is there once they are open, and not before*).
The named feature still states the work; a neighbour only ever adds room.

Numbers arrive as JSON numbers, not strings. `material` turns on the rpm
column; `feeds` names the shop's own table (whose rows outrank the shipped
ones) and requires `material`. Errors carry stage `assembly_report`, except
a `path` that will not load or parse, which keeps the shared `io`/`parse`
stages — the project is the surface's shared noun.

## Versioning

This is contract **v1**. Additive growth (a new tool, a new optional
argument) revs the minor text here without ceremony; any change to an
existing shape — the envelope, the check document, a result's bytes — is a
new major version and a `DECISIONS.md` entry saying why.

`assembly_report` (2026-08-20) is such additive growth: a sixth tool, no
existing verb's bytes moved, pinned by the five verbs' tests staying green
untouched. The gate-floor paragraph above was restated in the same change —
it read "on **every** verb" and that was no longer true as written.
