# Plan: settings that change how ca65 reads

**Status:** Proposed 2026-11-02.
**Binding records:** Lang198x's
[shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md)
and 198x's
[assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md).
**Follows:** Lang198x's proposed record on
[settings that change how source is read](https://github.com/lang198x/lang198x/blob/main/decisions/reading-settings.md),
which sets the rules here for every family. This plan is ca65's part of it.

Some ca65 commands change how ca65 reads the lines after them:

- `.case off` makes names ignore case.
- `.localchar` chooses `@` or `?` to start a cheap local.
- `.linecont +` lets a backslash join a line to the next.
- `.feature` turns on any of 19 compatibility features, one by one.

Lang198x refuses all of these today: `.case off` as "not lowered yet", and
the others as unsupported. One more gap belongs with them. ca65 reads
source as bytes, but this crate reads it as UTF-8 text, so a file with a
Latin-1 string cannot be read at all.

## What ca65 does

Measured on ca65 V2.19 at `8d86ca7`. These results decide the design:

- **A setting acts on the lines read after it, and nothing earlier.** With
  `Foo = 1` then `.case off`, `.byte FOO` is undefined. With `.case off`
  first, `Foo` and `FOO` are one name.
- **`.case off` upper-cases names as they are read.** A name defined while
  it is off is stored in upper case, so `.case off`, `foo = 1`,
  `.case on`, then `.byte FOO` finds it. Mnemonics and commands ignore case
  anyway.
- **A macro keeps the settings it was defined under.** A body defined while
  `.linecont +` is on joins its lines when it is called after
  `.linecont -`. A body defined while it is off does not join them when it
  is called while it is on.
- **A setting made in an included file stays in force** after the file
  ends. One in a skipped branch does nothing.
- **`.localchar` takes a character constant, `'@'` or `'?'`.** Any other
  character is `Invalid start character for locals`. Once it is `'?'`, an
  `@` is an invalid character. Cheap locals still belong to the label
  before them.
- **`.linecont +` joins at a backslash just before the end of a line**, but
  not one inside a comment.
- **Source is bytes.** A byte from `$80` to `$FF` in a string or a
  character constant is that byte, so a Latin-1 `é` is `$E9`, and a UTF-8
  `é` is the two bytes `$C3 $A9`. Two bytes in a character constant are
  `Illegal character constant`. In a comment, any byte is allowed. Anywhere
  else, it is `Invalid input character`.

The 19 features this ca65 knows, and the deprecated `addrsize`, which it
accepts with a warning, sort by what they change:

| What they change | Features |
|---|---|
| How a token is read | `at_in_identifiers`, `dollar_in_identifiers`, `leading_dot_in_identifiers`, `underline_in_numbers`, `loose_char_term`, `loose_string_term`, `missing_char_term`, `string_escapes`, `dollar_is_pc` |
| Where a line ends | `line_continuations` (the same as `.linecont`), `c_comments` |
| How a line is parsed | `labels_without_colons`, `bracket_as_indirect`, `ubiquitous_idents`, `pc_assignment` |
| What a line means | `force_range`, `org_per_seg`, `long_jsr_jmp_rts`, `merge_scopes` |

## How it fits what is here

The expander already handles one setting that changes how lines are read:
the CPU, which decides which names are registers. It parses each file under
the mode the file starts in. When a line is read after `.setcpu` has chosen
another CPU, it is lexed and parsed again under the current mode. A macro's
body keeps the tokens it was defined with.

The settings that change how a token or a line is read fit the same path.
`Mode` would grow from a CPU to a CPU plus the scanner settings. The
expander tracks them as it tracks the CPU, and a line read under an older
mode is lexed and parsed again. Macro bodies keep their tokens, which is
what ca65 does.

Line continuations and C comments are different, because they join lines.
Lexing one line again cannot join it to the next. When either is turned on
or off, the expander has to read the rest of the current file again, from
the next line, under the new mode. The file's text is in the provenance, so
this is a fresh parse of a known range, not a new source.

The settings that change what a line means are not about reading. They
travel to lowering as part of the line's mode, like the CPU and the 65816's
register widths, and the host acts on them.

## Options for source bytes

1. **Read source as Latin-1.** The host maps each byte to the character
   with the same number, which every byte is. The lexer then takes a
   character up to `$FF` in a string as one byte. This gives ca65's bytes
   exactly, and the errors outside strings follow. But a UTF-8 comment then
   shows as two odd characters in a listing.
2. **Read UTF-8 when it is valid, and Latin-1 otherwise**, with the choice
   kept for each file so strings convert the right way. Listings look right
   for UTF-8 files, but the expander has to carry an encoding for every
   text, including the ones it makes.
3. **Keep UTF-8 only**, and refuse other files, as now.

**Recommendation: option 1, through a new entry point.** `Expander::from_bytes`
and a `Host::include_bytes` take bytes and decode them as Latin-1 with the
shared decoder in `lang198x-syntax`, which the cross-language record adds
and which keeps an offset map back to the bytes. The present
`Expander::new(&str)` keeps reading text as it does now, so tools that hand
over text, such as an editor, do not change. A listing can show the bytes
back as UTF-8 when they are valid, since it has them.

## Settled, if this plan is agreed

- **`Mode` carries the scanner settings.** It keeps the CPU, and adds case,
  the local character, line continuations and every feature. The expander
  changes them as it runs each command, and a line read under an older mode
  is read again, as for the CPU.
- **The lexer reads under the mode.** Each token rule that a setting
  changes asks the mode: name characters, number digits, string and
  character terms and escapes, `$` as the address, the local character, and
  upper-casing names under `.case off`.
- **Joining settings read the rest of the file again.** The expander keeps
  where each file's remaining lines start, and parses from there under the
  new mode when line continuations or C comments change.
- **The parser reads under the mode too**, for labels without colons,
  brackets as indirection, `ubiquitous_idents` and assignment to `*`.
- **The meaning settings reach the host.** `ActiveLine` gives the features
  in force where the line was read. Lowering applies the ones it owns, and
  Asm198x's walk applies `org_per_seg` and `merge_scopes`.
- **Every feature has measured lines before it is built**, recorded in
  Asm198x's verdicts, with the cases where ca65 refuses it.

## Steps

Each step is a Lang198x release and an Asm198x PR, so each can be checked
on its own:

1. **Modes and token settings.** `Mode` with the settings, re-reading a line
   under a newer mode, `.case`, `.localchar` and the nine token features.
2. **Joining settings.** `.linecont` and `line_continuations`, and
   `c_comments`, by reading the rest of the file again.
3. **Parsing settings.** The four parser features.
4. **Meaning settings.** The four meaning features in lowering and the
   walk, and `addrsize`'s warning.
5. **Source bytes.** The shared Latin-1 decoder in `lang198x-syntax` first,
   then `Expander::from_bytes` and `Host::include_bytes`, and
   Asm198x reading files as bytes.

## Risks

- **Re-reading the rest of a file must not change what was read before.**
  Lines up to the joining command stay as they were. The provenance still
  traces every line to its file, so errors keep their places.
- **A setting could change how an earlier macro's arguments are read.**
  ca65 reads a call's arguments as it meets them, under the mode then. The
  expander does too, since it reads a call's line under the current mode.
  A measured line checks this.
- **Nineteen features is a lot of surface.** Each one is small, but they
  can combine, such as `loose_string_term` with `missing_char_term`, which
  ca65's manual says do not work together. Combinations get measured lines
  where the manual warns of them, and are refused where ca65 refuses them.
