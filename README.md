# lang198x-ca65

[![CI](https://github.com/lang198x/ca65/actions/workflows/ci.yml/badge.svg)](https://github.com/lang198x/ca65/actions/workflows/ci.yml)

The ca65 assembler family for [Lang198x](https://github.com/lang198x): one
lossless parser for ca65, the macro assembler of the
[cc65](https://cc65.github.io/) suite, with its CPU modes as a profile axis,
lowering to `asm198x-ir` for the Asm198x back end.

Status: building. The crate reads ca65's default mode, with every `.feature`
off: the lexer, the parser, preprocessing (conditionals, macros, repeats,
defines, token functions and includes), and lowering to `asm198x-ir` behind the
`ir` feature. Two CPUs are modelled: the 6502 and the 4510 (`.p4510`), and the
expander follows `.setcpu`, `.pushcpu` and `.popcpu` as it reads. The other
CPUs come next. The plan is
[`ca65-front-end.md`](https://github.com/lang198x/lang198x/blob/main/docs/plans/ca65-front-end.md)
in the Lang198x hub. The reference is ca65 V2.19 at cc65 commit `8d86ca7`.

## Licence

GPL-2.0-or-later. See [`LICENSE`](LICENSE).
