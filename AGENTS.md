# lang198x-ca65

Read [PRINCIPLES.md](PRINCIPLES.md) first; [MANIFESTO.md](MANIFESTO.md) explains
why and [PRACTICE.md](PRACTICE.md) explains the shared method.

This crate is the ca65 assembler family: one parser for ca65, the cc65 suite's
macro assembler, with its CPU modes as an axis of one profile, and lowering to
`asm198x-ir`. Two records bind it, and both have *Drift triggers* to read
before changing anything here:

- Lang198x's [shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md):
  families, profiles, the tree, testing. ca65 is a family of one.
- 198x's [assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md):
  what Lang198x owns and what Asm198x owns, and the migration gates. This is
  its stage 3.

The plan is Lang198x's
[`docs/plans/ca65-front-end.md`](https://github.com/lang198x/lang198x/blob/main/docs/plans/ca65-front-end.md).

## What belongs here

The lexer, preprocessing (macros, `.define`, repeats, conditionals, includes
and the token functions such as `.concat` and `.ident`), the parser, typed
views, profiles and lowering to `asm198x-ir`. The tree itself is
`lang198x-syntax`. Assembly semantics, encoding, segments, linking (ld65),
artifacts and the verdict corpus belong to Asm198x.

## Rules that are easy to break

- **The reference decides,** not Asm198x's old ca65 reader. Every rule is
  measured on the profile's ca65 version, and Asm198x records the verdicts.
- **Profiles name the tool version they describe,** and so does everything
  written about them. The first is ca65 V2.19 at cc65 `8d86ca7`.
- **A CPU is a mode of the profile,** never a dialect or a parser of its own.
- **Tokens made by a token function carry `Synthesised` provenance,** with
  every input and the call site, in side tables.
- **Lowering to `asm198x-ir` sits behind an optional `ir` feature, off by
  default,** once it exists. Tools that need only the tree build without any
  Asm198x crate.
- **Cross-repository links go through published versions,** never `git` +
  `rev` pins. A local path override for development is never committed.
- **Parsing never fails and no input panics.** Broken source becomes `ERROR`
  nodes and diagnostics.
- **Documentation is plain English, at about Flesch-Kincaid grade 10,** and
  every public item is documented.

## Working here

- Rust 1.99.1, pinned in `rust-toolchain.toml`. In sessions that set
  `RUSTUP_TOOLCHAIN`, run cargo with `env -u RUSTUP_TOOLCHAIN` so the pin
  applies.
- Before pushing: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo llvm-cov --workspace`. CI fails below 95% line coverage.
- Land work through pull requests, rebase-merged. Every commit subject and PR
  title is a conventional commit.
- The plan lives in the Lang198x hub; plans for single steps live in
  [`docs/plans/`](docs/plans/).
