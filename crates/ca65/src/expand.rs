//! Preprocessing: turning the files a person wrote into the lines ca65
//! assembles.
//!
//! The [`Expander`] runs a program's lines, with its conditionals and macros,
//! and hands each line that assembles to a [`Host`]. The host is whatever assembles
//! them. It also answers the questions that decide which lines run, such as
//! the value of an `.if` condition, since only it knows the symbols so far.
//!
//! The plan, `docs/plans/2026-10-27-preprocessing.md`, adds repeats,
//! define-style macros, token functions and includes in turn.

mod conditional;
mod expander;
mod macros;
mod repeats;

pub use expander::{ActiveLine, Expander, Host, Limits};

#[cfg(test)]
mod tests;
