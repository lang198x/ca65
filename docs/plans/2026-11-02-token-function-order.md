# Plan: token functions in line order

**Status:** Agreed 2026-11-02 as option 2; step 1 built the same day, for
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
| `.byte .sprintf("%d", V), .ref(V), .ref(W), .sprintf("%d", W), .ref(W)`, with `W = 3` | `32 01 00 33 01` | `32 01 01 33 01` |
| `.if .ref(V) + V + .strlen(.sprintf("%d", V)) = 3` | takes `.if` | takes `.else` |
| `.byte V, .ref(V), .sprintf("%d", V)` | `02 01 32` | the same |
| `.if V + .ref(V) + .strlen(.sprintf("%d", V)) = 4` | takes `.if` | the same |

The rows that already agree show what must not change:

- A plain read earlier on the line counts, as in `V, .ref(V)`. The host
  already gets this right, because it collects a line's reads as lowering
  reaches them.
- Inside a function's arguments, a `.ref` before a read does not see it,
  because the host evaluates each argument as it comes.
- A `.ref` inside a token list, such as `{.ref(V) ...}`, is only tokens.
  The function counts it as four tokens and does not answer it.
- `.string(V)` and `.defined(U)` do not count as uses of the name.
- A use that a function's result makes, such as the `V` that `.left`
  writes, counts when that result is read, not when the function runs.

So two things need to change:

- **A read a function makes must count from where the function sits.** A
  `.ref` sees the reads of the functions before it on the line, and not
  those after it, as the row with `V` and `W` shows. This holds in a
  condition too.
- **A line's label must be defined before its functions run.**

## Options

1. **Lower the line as it is read.** The expander would hand lowering one
   token at a time and run each function when lowering reached it, as ca65
   does. This is the most faithful, but it turns the expander's contract
   with its host inside out. Every caller would change, and so would the
   tests that check a whole line at a time.
2. **Tell the host which functions came before each `.ref`.** The expander
   counts the functions it has finished as it runs a line. It tells the host
   that count each time it evaluates for a function, and it notes the count
   at each `.ref` on the line. The host keeps a function's reads aside,
   tagged with the count, until the line is lowered. A `.ref` then sees only
   the reads tagged lower than its own count.
3. **Answer each `.ref` before a function, in the expander.** This was the
   first recommendation, and it is wrong. The expander would ask the host
   about the name before a later function ran. But the host has not lowered
   the line yet, so it has not seen a plain read earlier on the same line.
   `.byte V, .ref(V), .sprintf("%d", V)` would give `00` for the `.ref`,
   where ca65 gives `01`. The expander could look for the name earlier on
   the line itself, but deciding what counts as a read is the host's job.
   Doing it twice is how the two would drift apart.

**Recommendation: option 2.** It changes the contract, as #52 expected, but
only by adding to it. Each new method has a default that keeps today's
behaviour, so a host that does not care about `.ref` changes nothing.

## Settled

- **The expander counts finished functions.** As it runs a line's token
  functions, the expander counts each one that finishes, including one
  inside another's token list. The count starts at 0 on each line.
- **A function's evaluations carry the count.** `Host` gains
  `evaluate_in_function(expr, finished)`, whose default calls `evaluate`.
  The expander calls it whenever a token function needs a value.
- **Each `.ref` carries the count where it sits.** When the expander hands
  on a `.ref` or `.referenced` outside any token list, it notes how many
  functions had finished by then. `ActiveLine` gives that count for any
  `.ref` on the line, and a condition gets the same table.
- **Lowering asks with the count.** `Known` gains
  `is_referenced_before(name, finished)`, whose default calls
  `is_referenced`. Lowering a `.ref` that has a count asks this instead. A
  `.ref` with no count, such as one in a line with no functions, asks
  `is_referenced` as today. `Host` gains `evaluate_condition`, which takes
  the condition's table, and whose default calls `evaluate`.
- **A labelled line hands on its label first.** When a line has a label
  written before its first token function, and it runs a function, the
  expander first hands on the label alone, as a label-only `ActiveLine`. It
  then runs the functions and hands on the rest of the line without the
  label. This is the path a labelled conditional already takes, so hosts
  already handle it. A label that a function makes, such as `.ident("x"):`,
  is not split off, because ca65 cannot define it before the function runs.
- **Asm198x's walk keeps function reads aside.** It records the reads made
  by `evaluate_in_function` with their count. It answers
  `is_referenced_before(name, n)` from the reads before the line, the reads
  lowering has made on the line so far, and the function reads counted
  below `n`. Once a line is lowered or a condition decided, all of them
  count, as they do today.

- **As built.** The table is `expand::Finished`, which `ActiveLine::finished`
  gives. Lowering finds a `.ref`'s count through `Known::finished_before`,
  which lowering answers from the table itself, so a host implements only
  `is_referenced_before`. A host lowers a condition with
  `lower::condition`, which takes the table.

## Steps

1. **Lang198x.** Add the count, the two `Host` methods, the `Known` method,
   the table on `ActiveLine`, and the label split. Add every line in this
   plan to the tests, with a test host that keeps function reads aside, as
   the walk will. Then release. All the additions have defaults, so the
   release is compatible.
2. **Asm198x.** Take the release, and keep function reads aside in the walk.
   Remove the `walk_gap` marks naming ca65#52 from the data and conditional
   fixtures, and check that all 27 cases now match ca65. Add this plan's
   lines to `ca65-measured-lines.json`, with their verdicts. Close ca65#52.

## Risks

- **A host might treat the label-only line differently from a conditional's
  label.** Asm198x's walk defines the label at the current address for both,
  which is right. Its listing and sidecar give the label's line once,
  because both pieces come from the same source line. The Asm198x gates
  check this, including the listing and sidecar fixtures.
- **A read kept aside must still be reported.** The walk reports a name used
  but never defined from its reads. A read kept aside must reach that list
  once the line is done, or an undefined name in a `.sprintf` argument
  would go unreported. A measured line checks this.
- **A count may be needed somewhere the plan has not looked.** If another
  question about names turns out to depend on order, it takes a count in
  the same way. A measured line comes first.
