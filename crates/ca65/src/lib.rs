//! The ca65 assembler family for Lang198x.
//!
//! ca65 is the macro assembler of the cc65 suite. One dialect covers several
//! CPUs, from the 6502 to the 65816, so the CPU is an axis of the dialect's
//! [`Profile`] rather than a dialect of its own.
//!
//! The crate has the lexer, in ca65's default mode; the parser, with lines,
//! labels, assignments, instructions, expressions, control commands in
//! their own shapes, and macro definitions and calls; typed views over the
//! tree; and preprocessing in [`expand`]. With the `ir` feature, `lower`
//! lowers expressions, instructions and whole lines to `asm198x-ir`. The plan, in the
//! Lang198x hub's `docs/plans/ca65-front-end.md`, adds the rest of lowering
//! in turn, each step checked against ca65's own answers.
//!
//! ```
//! use lang198x_ca65::{Mode, Profile, kinds, lex};
//!
//! let lexed = lex("loop: dex\n bne loop\n", Mode::of(Profile::CA65_2_19_6502));
//! assert_eq!(lexed.tokens()[0].kind(), kinds::IDENT);
//! assert_eq!(lexed.tokens()[1].kind(), kinds::COLON);
//! assert!(lexed.diagnostics().is_empty());
//! ```

pub mod ast;
pub mod expand;
mod keywords;
pub mod kinds;
mod lexer;
#[cfg(feature = "ir")]
pub mod lower;
#[cfg(test)]
mod measured;
mod parser;
mod profile;

pub use keywords::{CONTROL_COMMANDS, is_control_command};
/// The shared syntax layer this crate builds on, for its spans, provenance
/// and diagnostics.
pub use lang198x_syntax as syntax;
pub use lexer::{Lexed, Lexer, Token, lex};
pub use parser::{Context, ParseError, parse, parse_with};
pub use profile::{Cpu, Mode, Profile};
