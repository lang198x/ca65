//! Preprocessing: turning the files a person wrote into the lines ca65
//! assembles.
//!
//! The [`Expander`] runs a program's lines, with its conditionals, macros,
//! repeats, define-style macros, token functions and includes, and hands
//! each line that assembles to a [`Host`]. The host is whatever assembles
//! them. It also answers the questions that decide which lines run, such as
//! the value of an `.if` condition, since only it knows the symbols so far,
//! and it reads the files the program includes.
//!
//! The plan is `docs/plans/2026-10-27-preprocessing.md`.

mod conditional;
mod defines;
mod expander;
mod functions;
mod locate;
mod macros;
mod repeats;

pub use expander::{ActiveLine, Expander, Host, IncludedFile, Limits};
pub use locate::Location;

#[cfg(test)]
mod tests;
