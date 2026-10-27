//! The ca65 assembler family for Lang198x.
//!
//! ca65 is the macro assembler of the cc65 suite. One dialect covers several
//! CPUs, from the 6502 to the 65816, so the CPU is an axis of the dialect's
//! [`Profile`] rather than a dialect of its own.
//!
//! So far the crate has the lexer, in ca65's default mode, and the parser:
//! lines, labels, assignments, instructions, expressions, control commands
//! in their own shapes, and macro definitions and calls. The plan, in the
//! Lang198x hub's `docs/plans/ca65-front-end.md`, adds preprocessing and
//! lowering to `asm198x-ir` in turn, each checked against ca65's own answers
//! recorded in Asm198x.
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
#[cfg(test)]
mod measured;
mod parser;
mod profile;

pub use keywords::{CONTROL_COMMANDS, is_control_command};
pub use lexer::{Lexed, Lexer, Token, lex};
pub use parser::{Context, ParseError, parse, parse_with};
pub use profile::{Cpu, Mode, Profile};
