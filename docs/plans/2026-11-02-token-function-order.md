# Plan: token functions in line order

**Status:** Proposed 2026-11-02, for
[ca65#52](https://github.com/lang198x/ca65/issues/52).
**Binding records:** Lang198x's
[shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md)
and 198x's
[assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md).

ca65 runs a token function such as `.sprintf` when its scanner reaches it,
partway through a line. By then, everything before it on the line has been
read and evaluated. The expander runs all of a line's token functions first,
and only then hands the line on to be lowered. Two things come out in the
wrong order:

- **A read counts too early.** A token function that evaluates a name marks
  it as used. A `.ref` earlier on the line should not see that use, but it
  does, because it is answered when the line is lowered.
- **A label is defined too late.** ca65 defines a line's label before it
  reads the rest of the line. The expander runs the functions first, so a
  function that asks about the label finds it undefined.

Asm198x marks 27 fixtures as gaps of the walk for this reason: three shapes
of source, each written directly, in an included file and in a macro.

## What ca65 does

Measured on ca65 V2.19 at `8d86ca7`, linked flat, with `V = 2` before each
line:

| Line | ca65 | Expander today |
|---|---|---|
| `.byte .ref(V), .sprintf("%d", V), .ref(V)` | `00 32 01` | `01 32 01` |
| `.if .ref(V) .or .strlen(.sprintf("%d", V)) = 9` | takes `.else` | takes `.if` |
| `H: .byte .sprintf("%d", .defined(H)), .const(H-H)` | `31 01` | `30 01` |
| `.byte .sprintf("%d%d", .ref(V), V), .ref(V)` | `30 32 01` | the same |
| `.byte .referenced(V), .ref(V), V, .ref(V)` | `00 00 02 01` | the same |
| `.byte .tcount({.ref(V) .sprintf("%d", V)}), .ref(V)` | `05 01` | the same |
| `.byte .ref(V), .left(1, {V}), .ref(V)` | `00 02 01` | the same |
| `.byte .ref(V), .strlen(.string(V)), .ref(V)` | `00 01 00` | the same |
| `.byte .ref(U), .sprintf("%d", .defined(U)), .ref(U)` | `00 30 00` | the same |

The rows that already agree show what must not change:

- Inside a function's arguments, a `.ref` before a read does not see it,
  because the host evaluates each argument as it comes.
- A `.ref` inside a token list, such as `{.ref(V) ...}`, is only tokens.
  The function counts it as four tokens and does not answer it.
- `.string(V)` and `.defined(U)` do not count as uses of the name.
- A use that a function's result makes, such as the `V` that `.left`
  writes, counts when that result is read, not when the function runs.

So only two things need to change: a `.ref` before a token function, and a
line's label.

## Options

1. **Lower the line as it is read.** The expander would hand lowering one
   token at a time and run each function when lowering reached it, as ca65
   does. This is the most faithful, but it turns the expander's contract
   with its host inside out. Every caller would change, and so would the
   tests that check a whole line at a time.
2. **Tell the host where each read happened.** The expander would mark each
   read with its place on the line, and the host would answer `.ref` by
   comparing places. This keeps the expander as it is but adds a new part to
   the host's contract. Each host would have to trace places across made
   text to get it right, which is easy to get wrong.
3. **Answer what is asked before each function, in the expander.** ca65
   answers a `.ref` before any function after it runs. The expander can do
   the same: as it runs a line's functions from left to right, it answers
   each `.ref` that comes before a later function, by asking the host. It
   then puts the answer, `0` or `1`, in the line in place of the call. A
   labelled line is handed on as its label first, before its functions run,
   as a labelled conditional already is.

**Recommendation: option 3.** It needs no change to the `Host` trait: the
expander already asks `Host::is_referenced` for `.ifref`. It changes only
lines that have both a token function and a `.ref` or a label, so nothing
else can move.

## Settled, if option 3 is agreed

- **A `.ref` is answered when a function after it is about to run.** As the
  expander runs a line's functions, it answers a `.ref` or `.referenced`
  call only if it sits outside any token list and a scanner function
  follows it on the line. That includes one inside a later token list, as
  in `.tcount({.sprintf(...)})`. The answer is a number token, made
  `Synthesised` from the call, like any other function's result. A `.ref`
  after the last function is left to lowering, which already sees every
  use the functions made.
- **Only `.ref` and `.referenced` are answered early.** A token function can
  change only which names have been used. `.defined`, `.const` and the
  other functions about names give the same answer either side of a
  function, so they stay with lowering. If a later measurement shows
  otherwise, the function joins this list, with the measurement as a test.
- **A labelled line hands on its label first.** When a line has a label
  written before its first token function, and it runs a function, the
  expander first hands on the label alone, as a label-only `ActiveLine`. It
  then runs the functions and hands on the rest of the line without the
  label. This is the path a labelled conditional already takes, so hosts
  already handle it. A label that a function makes, such as `.ident("x"):`,
  is not split off, because ca65 cannot define it before the function runs.
- **The answers go through `Answers`.** The functions module asks the
  expander through its `Answers` trait. The trait gains an `is_referenced`
  question, which the expander answers from the host.
- **No public change.** `Host`, `ActiveLine` and `Lowered` keep their shape,
  so this is a patch release.

## Steps

1. **Lang198x.** Answer an early `.ref` and split off the label, as above.
   Add the measured lines in this plan to the expander's function tests,
   including the rows that already agree. Then release.
2. **Asm198x.** Take the release. Remove the `walk_gap` marks naming
   ca65#52 from the data and conditional fixtures, and check that all 27
   cases now match ca65. Add this plan's lines to
   `ca65-measured-lines.json`, with their verdicts. Close ca65#52.

## Risks

- **A host might treat the label-only line differently from a conditional's
  label.** Asm198x's walk defines the label at the current address for both,
  which is right. Its listing and sidecar give the label's line once,
  because both pieces come from the same source line. The Asm198x gates
  check this, including the listing and sidecar fixtures.
- **An answered `.ref` no longer reaches lowering.** A host that counted a
  name inside `.ref` as asked for would lose that. ca65 does not count it,
  and the walk does not either, so this should change nothing. The `U` row
  above checks the undefined case.
