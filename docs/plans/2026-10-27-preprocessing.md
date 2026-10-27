# Plan: preprocessing (step 5)

**Status:** Started 2026-10-27, as step 5 of Lang198x's
[ca65 front-end plan](https://github.com/lang198x/lang198x/blob/main/docs/plans/ca65-front-end.md).
**Binding records:** Lang198x's
[shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md)
and 198x's
[assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md),
stage 3.

Turn the files a person wrote into the lines ca65 V2.19 assembles. That means
running macros and define-style macros, repeats, conditionals, includes and
the token functions, and keeping track of where every piece of text came from.

## How ca65 differs from PasmoNext

The Z80 family's expander (`lang198x/z80-asm`, step 4 of its plan) is the
model. ca65 differs from PasmoNext in ways that change the design, each
measured on V2.19 at `8d86ca7`:

- **Includes are lazy.** An `.include` inside `.if 0` is never opened, so a
  missing file there is not an error. Includes cannot be spliced in before
  anything runs; the expander opens each one when it reaches it.
- **ca65 reads tokens, not lines.** A false `.if` is skipped token by token, so
  `.byte 1 .endif` ends the skipped branch partway through its line. Even in
  a skipped branch the scanner still runs, so an unknown control command
  there is still an error.
- **Define-style macros expand in the scanner, before anything else.** They
  expand everywhere, including inside a macro's body as it is defined:
  `.define N 5`, a macro using `N`, then `.undef N` and `.define N 6` still
  gives 5 when the macro runs.
- **Token functions run as tokens are read,** except in skipped branches:
  `.ident(.concat("la","b")):` defines the label `lab`, and a broken
  `.left(` inside `.if 0` is not an error. Their results carry `Synthesised`
  provenance.
- **A macro's parameters and a repeat's counter are replaced token by token.**
  `.repeat 2, i` with `.byte i` gives 00 01. Braces around a macro argument
  are removed, and parentheses do not protect a comma (step 4c).
- **Repeats nest by counting tokens.** The body of `.repeat` runs to the
  `.endrepeat` that balances it, wherever on a line it is.

## Settled

- **The expander walks the program once and asks a host about values,** as in
  `z80-asm`. A condition, a repeat count or a token function's count needs
  symbols, and only the assembler knows them. The host evaluates expression
  views, and says whether a name is defined or used and whether an
  expression is constant yet, for `.ifdef`, `.ifref` and `.ifconst`.
- **Files come from a loader the caller supplies.** The crate never touches
  the file system. `.macpack` asks the loader for the package's text by name.
  The crate ships no text from cc65; Asm198x decides where packages come from.
- **Lines reach the host already parsed.** A line that nothing changed is
  parsed in place. A changed line is parsed again, as an expansion, with its
  provenance.
- **The tree stays one `LINE` per source line** (step 4). The expander works
  over the tree's tokens, so a `.endif` partway through a line is found
  without changing the parser.

## Steps

Each step is one pull request that passes format, Clippy, tests and the 95%
coverage floor. Each measured case is a test, run through a test host: a tiny
assembler for labels, `=`, `.byte` and `.word`, good enough to compare bytes
with ca65's.

### 5a. Typed views

Thin views over the tree, written by hand: `SourceFile`, `Line`, `Label`,
`Statement` (`Instruction`, `Command`, `MacroCall`, `Assignment`), `Operand`
and `Expr`. Every part is optional, because broken lines lack parts. Checked by
a second evaluator over the views that must give ca65's value for the 64
measured expressions.

### 5b. The expander and conditionals

`expand::{Expander, Host, ActiveLine, Limits}` over one text: the `.if`
family, `.else`, `.elseif` and `.endif`, skipped token by token, with
`.ifdef`, `.ifref`, `.ifconst`, `.ifblank` and the CPU tests. `.end` stops the
program. Limits on frames, lines and text stop a program that never ends.

### 5c. Macros

Classic macros: collect the body, call with arguments replaced token by
token, braces removed, `.paramcount`, `.local` names unique to each call,
`.exitmacro`, too many arguments refused, and recursion stopped by the frame
limit. A body sees the define-style macros in force when it was defined.

### 5d. Repeats

`.repeat count[, counter]`, with the counter replaced in the body and
repeats nested by counting tokens.

As built: each time round, the body's text is made again with every name
spelt exactly like the counter replaced, so `I` and `i` differ and an inner
`.repeat 2, i` loses its counter to an outer `i`, both as in ca65.
Conditionals are not tied to repeats: one may open in one time round and close
in the next. Two cases differ from ca65 V2.19 on purpose. `.exitmacro` inside
a repeat inside a macro leaves at once, where ca65 finishes the repeat first,
against its manual. A `.endrepeat` after other tokens on its line leaves each
round's last line unfinished, and ca65 joins it to the next line it reads: the
next round's first line, or after the last round the line after the
`.endrepeat`. So `tail: .endrepeat` defines `tail` at that line, and the
expander joins lines the same way.

### 5e. Define-style macros

`.define` with and without parameters, expanded everywhere they appear,
including in macro bodies as they are defined, and `.undef`.

As built: the expander rewrites each line it reads with the defines in
force, before the line runs or is skipped, and again as a classic macro's
or a repeat's body is collected, so a define can end a skipped branch, a
repeat or a macro body. A use takes exactly as many arguments as there are
parameters. Each runs to a comma or the end of the line, unless braced, so
parentheses are only tokens: `F(2)` passes `(2)`. Classic and define-style
macros share their names. A define whose body uses its own name is
refused, which also rules out a define that expands for ever.

This step was split in two: token functions need the scanner's view of a
line too, but nothing else here depends on them.

### 5f. Token functions

The token functions `.concat`, `.ident`, `.left`, `.mid`, `.right`,
`.string` and `.sprintf`, each with `Synthesised` provenance naming every
input.

As built: functions run on each line that runs, after its defines, and not
in skipped branches, bodies being collected or a `.define`'s body, which
ca65 reads in raw mode. Each result is read again, so functions nest. A
count is a constant expression the host evaluates. A token list runs to the
first `)` unless it is in braces. Each result is `Synthesised` from the
call.

Where ca65 V2.19 is wrong against its manual, the manual wins. `.sprintf`'s
format is "similar to the one of the C printf function", so C's results are
given where ca65's own formatter differs: `%x` of a negative number, `%#x` of
0, and the width of `%c` and `%s`. `.mid` takes its start as given, where
ca65 treats a start above 100 as 0. Results are handed on as tokens, not
text (see [expansions keep their tokens](2026-10-27-made-tokens.md)), so
`.ident` of a name spelt like a register makes a name, and a made string may
hold a quote, a line break or a byte above 127, as in ca65.

### 5g. Includes and packages

`.include` and `.macpack`, opened when reached, through the loader, with
`Included` provenance. Missing files, cycles and deep nesting give
diagnostics. `.incbin` stays a command for lowering.

As built: the loader is two methods of the host, `include` and `package`,
which return `None` by default. A file is read when its line runs, never in a
skipped branch or a body being collected, and is asked for with the file
being read, since ca65 looks beside that first. `.macpack name` asks for the
package by name; ca65 V2.19 reads `name.mac` from its include path. Each file
read in takes the next `FileId`, and `Expander::files` lists them with the
command that read them in. A line copied from one carries `Included`
provenance.

As in ca65, a macro's or repeat's body may run on past the end of an
included file, and a file may close a conditional opened before it, but a
conditional opened in a file is closed, with a diagnostic, when the file
ends. A line the parser had to guess at after an include is read again now
that the macros are known. Includes nest at most 64 deep, so a file that
includes itself stops.

## Afterwards

Step 6, in Asm198x, lowers the lines the expander hands over and gates the
new path against ca65's recorded verdicts.
