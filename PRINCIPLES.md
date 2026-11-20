# Principles

How a capability should behave for the person using it, once it exists.
`README.md` says why the project exists; `AGENTS.md` says how work lands;
`DECISIONS.md` says what is settled. This file is the standing preferences
that shape every user-facing surface. Push back on any of them in a PR.

1. **No dependency graph between operations.** `run` order is the sequence.
   Creating an operation may read earlier ones to suggest settings; wiring
   ops together is not allowed. The stock model reading what earlier ops left
   is physics, not a declared dependency.

2. **Defaults are smart, else dumb, never last-used.** Smart means derived
   from the current context (the feature picked, the declared tools, the
   stock). Dumb means the documented neutral value. A remembered last value
   encodes the previous part, not this one. A smart default never commits
   silently; the person still reviews before it lands.

3. **The feature is the authority; the part model is context.** The engine
   refuses against the declared feature. Whether the imported CAD is violated
   is a later, softer question.

4. **Make the right feature instead of overriding a wrong one.** If an op
   needs to cut where a feature forbids, fix the declaration. Anything meant
   to differ from the CAD is caught at simulation, by choice, not silently.

5. **Three kinds of "move the work" never blur.** Where the part physically
   sits, where a work offset (G54) says it sits, and a temporary transform of
   geometry are three acts. Every surface that translates or orients must
   make which one is happening unmistakable.

6. **The post is a formatter, never a fixer.** Every check gauges the emitted
   program, so nothing may change it afterwards. Dialect lives in the declared
   `machine`. A user post is the escape hatch and its ideal size is zero.

7. **Anything the system can say, an agent can read.** One fact, one home,
   with a structured rendering wherever a tool must act on it and prose
   where a person reads fast. Two renderings, never two sources.

8. **A click-heavy user finishes an ordinary job without a terminal.** Open a
   project, declare stock, tools and features, schedule operations, read what
   the checker found, write the program. The text stays the source of truth
   and every act in the window has a CLI twin.

9. **The person owns the program, not the algorithm.** Every choice the engine
   makes on their behalf (pattern, entry point, direction, where it lifted or
   plunged) is written in the program as a plain sentence and is overridable
   as a key in the `.part` text. Nothing automatic that cannot be seen,
   understood and undone. The machine is never answerable for a crash; the
   person must be able to know what happened before it does.

10. **An empty problems list is not a certificate.** The window and the CLI
    say what was checked and what could not be checked, not only what was
    found. "No findings" with no account of what was looked at is a silence,
    and silences are refused everywhere else in this project.

11. **STEP is the front door.** The ordinary job starts from a model: import
    it, accept a stock proposal, run recognition, review each proposed feature
    as the text it will write, then program. Hand-written projects are tests
    and teaching examples, not the primary workflow.

Tenets 1 to 5 came from the user's wishlist of 2026-08-15, 6 and 7 the same
day, 8 on 2026-10-15, and 9 to 11 from the reset of 2026-10-28.
