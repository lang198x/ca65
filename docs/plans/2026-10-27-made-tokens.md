# Plan: expansions keep their tokens

**Status:** Done 2026-10-27. Agreed as option 1 of three put to the project
owner.
It follows step 5, [preprocessing](2026-10-27-preprocessing.md).
**Binding records:** Lang198x's
[shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md)
and 198x's
[assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md).

ca65 works on tokens. When a macro, a repeat, a define-style macro or a
token function makes something, ca65 hands on the tokens it made. The
expander hands on text instead. It joins its pieces into an expansion's
text, then lexes and parses that text again. Most of the time this gives
the same tokens. It fails where the text cannot say what the token was:

- **A name spelt like a register.** `.ident("A")` makes a name `A`.
  Lexed again, `A` is the accumulator. The expander reports it as
  unsupported. Asm198x's gate names five such cases.
- **A string that cannot be written.** `.sprintf("%c", 255)` makes a string
  holding the byte $FF, which UTF-8 text cannot hold. A string holding a
  quote or a line break cannot be written either, because ca65's strings
  have no escapes. All three are reported as unsupported. Asm198x's gate
  names one case, where such a string is a macro's argument.

The fix is to hand on tokens, as ca65 does, and not to lex made text again.

## Settled

- **An expansion is parsed from its tokens.** Each piece an expansion is
  built from keeps its token kind, and the parser reads the expansion's
  tokens from that list instead of lexing its text. Files are still lexed.
  A piece copied from source keeps the kind the lexer gave it, so nothing
  changes for those. Only a made piece can differ.
- **The text stays.** Each expansion still has its text, in the provenance,
  so the tree stays lossless and every span still traces back. The text of
  a made token is how it is shown, not how it is read.
- **A made string keeps its bytes beside the text.** A string token that a
  function made carries its bytes. Its text shows those bytes, with a byte
  that cannot be written shown as an escape such as `\xFF`. The expander
  keeps a side table from a made string's span to its bytes, as provenance
  is kept, never in the tree. Copying a token into another expansion copies
  its entry. `ActiveLine` gives the bytes of any string token on the line,
  and lowering reads `.byte` and `.asciiz` strings through it.

  As built: since expansions are not lexed again, a quote or a line break
  sits in a made string's text as it is. Only a string with a byte above
  127 has an entry in the table, and its text shows each such byte as
  `\xNN`, with a backslash doubled.
- **One way to copy a token.** Every expansion is built through one helper
  that pushes a token's text, origin, kind and bytes together. Macros,
  repeats, defines, functions and joined lines all use it, so no path can
  copy the text and drop the rest.
- **The parser takes a token source.** It uses the lexer through two calls,
  for the next token and for the lexer's diagnostics. A prepared list of
  tokens answers both, so the parser needs a small trait and no other
  change. A prepared token is not lexed again in a different mode; it keeps
  the kind it had when it was made.

## Steps

Each step is one pull request that passes format, Clippy, tests and the
coverage floor.

### 1. Expansions keep their kinds

The token-source trait, the single copying helper, and parsing expansions
from their tokens. `.ident` of a name spelt like a register makes a name.
A test checks that every existing expansion parses the same either way.

### 2. Made strings keep their bytes

Bytes on string pieces, the side table, `ActiveLine`'s accessor, and
lowering's strings. `.sprintf`'s `%c` above 127, and strings made with a
quote or a line break, are made rather than refused.

## Not changed

A token function still runs on a line's tokens as they are read. Lowering
still sees a `Line` from a tree. Files are lexed as before.
