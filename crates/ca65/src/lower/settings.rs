//! Commands that change how ca65 reads or lists the program, not what it
//! makes: the CPU, smart mode, `.autoimport`, the listing, debug
//! information and file options.
//!
//! Each follows ca65 V2.19's `pseudo.c`. Only the 6502 profile exists, so
//! choosing the 6502 changes nothing, and choosing another CPU is not
//! lowered yet. The rest make no bytes, so they are checked as ca65 checks
//! them and then have no effect. `.pushcpu` and `.popcpu` are reported, so
//! the assembler can keep ca65's stack of CPUs and refuse a `.popcpu` with
//! nothing to pop.

use lang198x_syntax::Diagnostic;

use super::Known;
use super::expr::{self, invalid, unsupported};
use crate::ast::{Command, Expr, View};
use crate::expand::ActiveLine;
use crate::kinds;

/// A change a line makes to ca65's stack of CPUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CpuStack {
    /// `.pushcpu` keeps the CPU. ca65 keeps eight at most.
    Push,
    /// `.popcpu` takes back the one kept last.
    Pop,
}

/// The CPUs ca65 V2.19 knows, as `.setcpu` spells them, in any case.
const CPUS: &[&str] = &[
    "none", "6502", "6502X", "6502DTV", "65SC02", "65C02", "65816", "sweet16", "huc6280", "m740",
    "4510", "45GS02", "W65C02", "65CE02",
];

/// The commands that choose a CPU other than the 6502.
const OTHER_CPUS: &[&str] = &[
    ".p02x",
    ".p4510",
    ".p45gs02",
    ".p6280",
    ".p816",
    ".pc02",
    ".pce02",
    ".pdtv",
    ".pm740",
    ".psc02",
    ".psweet16",
    ".pwc02",
];

/// What a setting command does to the CPU stack, as `Some`, if `command`
/// is one. `None` means it is some other command.
pub(super) fn setting(
    command: &Command,
    line: &ActiveLine,
    known: &dyn Known,
) -> Result<Option<Option<CpuStack>>, Diagnostic> {
    let range = command.syntax().text_range();
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    let values: Vec<Expr> = command.values().collect();
    match word.as_str() {
        ".p02" => {}
        word if OTHER_CPUS.contains(&word) => {
            return Err(unsupported(
                range,
                &format!("`{word}`, which chooses another CPU,"),
            ));
        }
        ".setcpu" => {
            let name = match values.as_slice() {
                [value] => super::line::string(value, line),
                _ => None,
            }
            .ok_or_else(|| invalid(range, "`.setcpu` takes the CPU's name in quotes"))?;
            let name = String::from_utf8_lossy(&name).into_owned();
            if !name.eq_ignore_ascii_case("6502") {
                if CPUS.iter().any(|cpu| cpu.eq_ignore_ascii_case(&name)) {
                    return Err(unsupported(range, &format!("the CPU `{name}`")));
                }
                return Err(invalid(range, "CPU not supported"));
            }
        }
        ".pushcpu" => return Ok(Some(Some(CpuStack::Push))),
        ".popcpu" => return Ok(Some(Some(CpuStack::Pop))),
        ".smart" | ".autoimport" | ".list" | ".debuginfo" => switch(command)?,
        ".listbytes" => int_arg(command, &values, known, 4, 255)?,
        ".pagelen" | ".pagelength" => int_arg(command, &values, known, 32, 127)?,
        ".fileopt" | ".fopt" => file_option(command, &values, line, known)?,
        _ => return Ok(None),
    }
    Ok(Some(None))
}

/// Checks a switch: nothing, `+`, `-`, `on` or `off`, in any case.
fn switch(command: &Command) -> Result<(), Diagnostic> {
    match command.switch() {
        None => Ok(()),
        Some(token) if matches!(token.kind(), kinds::PLUS | kinds::MINUS) => Ok(()),
        Some(token)
            if token.text().eq_ignore_ascii_case("on")
                || token.text().eq_ignore_ascii_case("off") =>
        {
            Ok(())
        }
        Some(token) => Err(invalid(token.text_range(), "Expected ON or OFF")),
    }
}

/// Checks a count from `min` to `max` that must be known now, or the word
/// `unlimited`, which ca65 spells only in lower case.
fn int_arg(
    command: &Command,
    values: &[Expr],
    known: &dyn Known,
    min: i64,
    max: i64,
) -> Result<(), Diagnostic> {
    let range = command.syntax().text_range();
    let [value] = values else {
        return Err(invalid(range, "expected one value"));
    };
    if let Expr::Name(name) = value
        && name.text() == "unlimited"
    {
        return Ok(());
    }
    let lowered = expr::expression(value, known)?;
    let count = expr::constant(&lowered, known)
        .ok_or_else(|| invalid(value.syntax().text_range(), "Constant expression expected"))?;
    if !(min..=max).contains(&count) {
        return Err(invalid(value.syntax().text_range(), "Range error"));
    }
    Ok(())
}

/// Checks `.fileopt`: a keyword or a byte, then a string.
fn file_option(
    command: &Command,
    values: &[Expr],
    line: &ActiveLine,
    known: &dyn Known,
) -> Result<(), Diagnostic> {
    let range = command.syntax().text_range();
    let [option, text] = values else {
        return Err(invalid(range, "`.fileopt` takes an option and a string"));
    };
    match option {
        Expr::Name(name) => {
            let keyword = name.text();
            if !["author", "comment", "compiler"]
                .iter()
                .any(|k| k.eq_ignore_ascii_case(&keyword))
            {
                return Err(invalid(
                    option.syntax().text_range(),
                    "Expected a file option keyword",
                ));
            }
        }
        _ => {
            let lowered = expr::expression(option, known)?;
            let number = expr::constant(&lowered, known).ok_or_else(|| {
                invalid(option.syntax().text_range(), "Constant expression expected")
            })?;
            if !(0..=255).contains(&number) {
                return Err(invalid(
                    option.syntax().text_range(),
                    "Option number must be in byte range",
                ));
            }
        }
    }
    if super::line::string(text, line).is_none() {
        return Err(invalid(
            text.syntax().text_range(),
            "Expected a string constant",
        ));
    }
    Ok(())
}
