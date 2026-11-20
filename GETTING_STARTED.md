# Getting started — the window, first

This is the page for a machinist, not an agent: open the window, see the
part, click, get G-code. The language underneath is one text file per
project and it stays that way; you do not have to write it by hand to do
ordinary work. (`REFERENCE.md` lists every word of it, generated from the
code; `README.md` says why the project exists; `AGENTS.md` is for agents.)

## Try it in sixty seconds

```sh
cargo build                              # once
./target/debug/nacs --view plate.cam     # the sample: a plate, a face, a profile
```

In the window: the **Outline** on the left lists the stock, the features
and the operations; click one and its card opens beneath (the form) and
the viewport lights it. The **Program** on the right is the G-code; **Write
program** on its strip puts it beside the project as `plate.nc`, **Copy
program** on the clipboard. The buttons at the viewport's top-right fit
and turn the view; the Layers box at its top-left shows or hides what is
drawn, including the stock simulation — it unfolds while the pointer is
over it, and a click on its header pins it open. The **λ** at the left
of the sidebar's tabs (Outline, Problems, Review) is the menu: the job's verbs first (new project, new
feature, new tool, new operation, write program), then File, Edit, View,
Window and Help — every row is the same act a key or a button fires, and
`nacs --reference` prints the whole tree. Right-click anything for what
applies to it; *Edit…* opens its card.

From then on `nacs --view` alone reopens the last project, and a folder
dropped on the window opens that one.

## Your own part, from nothing

1. **A project.** `nacs --init 'Box w=100 l=60 h=12' job.cam` writes
   `job.cam/main.part` with the stock and the header (feed, plunge,
   spindle, the two safe heights), every value shown. Or open the window on
   an empty folder (`nacs --view job.cam`) and use **New project**.
2. **A tool.** Outline → **New tool…** → the kind (Endmill, Ballmill, Drill,
   …), the number is suggested, `dia` and the rest are fields. Create.
3. **A feature.** Outline → **New feature…** → the kinds are grouped
   (holes · pockets and faces · bosses and outside profiles · 3D surfaces),
   the name is suggested, every number says its unit. A pocket or a boss is
   a loop: type four numbers under *rectangle → points* (or *circle →
   points*) and the loop is made, wound the right way. Create.
4. **An operation.** Select the feature → **New operation…** (or `n`): the
   skeletons are the menu (a pocket offers Clear, End, Side in order; a Plane
   or a Face, Clear then End), the tool is preselected when you have one,
   the stepover suggested. Create, and the form moves to the next skeleton.
   The new line lands **after the op cursor** — straight below the
   operation the cursor stands on, at the end when it stands at the end —
   and the cursor moves onto it.
5. **Check.** The Problems view says what the checker found, in plain
   words; a decline ("Not checked") is counted apart from a mistake. The
   transport bar says the cut time; the Uncut layer says what was left.
6. **Write program.** The `.nc` beside the project is exactly what
   `nacs job.cam -o job.nc` writes.

Every write is one line of the file the way you could have typed it, and
`z` (or *Undo* on the transport bar) undoes it.

**Output** (`3`, or Window ▸ Output) is the window's log: one line for each
thing that happened or was refused, with its time. *Copy* puts it on the
clipboard and *Save* writes it beside the project as `<name>.log`.

## The verbs, for someone who knows another CAM

| here | what it cuts | elsewhere |
|---|---|---|
| **End** | the end of the tool — a face, a pocket's floor | 2D Face, floor finishing |
| **Side** | the flank — a wall, an outside profile | 2D Contour, wall finishing |
| **Clear** | a volume — the pocket's material, by a pattern | 2D Pocket / Adaptive |
| **Copy** | a surface — a 3D finish with a ball nose | Parallel / Scallop / 3D contour |
| **Form** | a form tool, or with `points=[…]` a drill's visit to holes | Drill, chamfer, radius |

`stepdown=` on a Clear cuts the volume in equal levels (a Clear over a boss
or a revolve needs it); without it a Clear is one level at the floor. An End
never takes it: it works one surface.

**A Plane is the floor whose wall is the stock.** It has no outline: its
boundary is the blank, and anything standing inside it (a boss, a post) is
an island the cut goes around, never named in the operation. So to clear
the material around a part that stands in the blank, declare a Plane at its
base and `Clear` it; add an `End` on it only if the floor itself serves the
part. A floor at the blank's bottom is a cut *through* the blank, and
`--outline` says so.

`pattern=` on a Clear or an End: `offset` (spiral), `zigzag`, `oneway`,
`spine` (rows that cut only where they own floor), `held` (spine's rows
under a ceiling on engagement — `engagement=<deg>`, 90° without it — with
no `stepover=`: the ceiling derives the spacing, the rows start from the
air, a turn that would exceed the ceiling is a lift, and the program says
what ran over it). The pattern card on an operation measures every
pattern on it; **pin** writes the one you pick.

## Words you will meet

- **stock** — the blank. `z` on it is the TOP.
- **extent** — how far a feature reaches down from its top (or up, for a
  boss). There is no `depth` on an operation: the feature owns it.
- **stock_to_leave** — the allowance a roughing pass leaves for the next.
- **given** — a void another setup already cut, air before this program.
- **fixture** — workholding the tool must avoid.
- **Not checked** — a check that could not run on this operation; the
  reason says why. Not a mistake.

## When something is red

The Problems view's first row says what. A reference refusal names the
line and what is declared; a header missing a word names the word; a
file that no longer parses stays up as it last was, with *Open main.part*
to hand the file to your editor — the window reloads when you save.

## Importing a STEP file

The recognizer is a Python sidecar (`sidecar/README.md`: a one-time venv
of about 1.5 GB). Open the window on a folder, empty or not:

```sh
cargo run -- --view bracket.cam
```

**Import STEP…** (File menu, the Target header's *New target…*, or drop
the file on the window) runs the sidecar into that folder. It writes
`report.json`, `proposed.part` and `proposed_stock.part`; then the
**Review** view opens with the proposed stock as its first row and every
face of the model under it. Each row's card shows the exact text it will
write. **Accept into project** on the stock makes `main.part` when the
folder is fresh (or replaces the stock line when it is not); **Accept
all** adds every proposed feature in one write. `z` undoes each. The holes
and planes it finds are the usual yield; the rest you declare.

The statement on a row's card is a text field: change it there and Accept
writes your text instead of the report's. **Reject** on a row appends a
`# rejected F7` line to `main.part`, so the row says so, Accept all skips
it, and `nacs --review` prints the same mark; the card offers the way back.

The stock proposal is one of two things, and its comment says which. A
model that is a plain prism is its own blank: a `stock Profile` with the
top face's outline. Anything else gets the smallest blank it fits, a
`stock Cylinder` for a round part or a `stock Box`, at the model's exact
size: the allowance is yours to add before the first cut.

Under the path field sit the import's two setup words: **up**, which model
axis points up on the machine (`z+` is as modeled), and **solid**, which
solid is the part when the file is an assembly. An assembly imported
without one is refused with the solids listed; the path stays in the
field, so you name the solid and press Import again. The Review's head
says which solid and which way was up.

From a terminal, `nacs --import model.step [--up y+] [--solid 1]
bracket.cam` runs the same two scripts and `nacs --review
bracket.cam/report.json` prints what the Review shows.
