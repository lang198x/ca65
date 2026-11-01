//! Commands that change how ca65 reads or lists the program, not what it
//! makes: the CPU, smart mode, `.autoimport`, the listing, debug
//! information and file options.
//!
//! Each follows ca65 V2.19's `pseudo.c`. The expander follows the CPU a
//! line chooses, and each line says which is in force. The rest make no bytes, so they are checked as ca65 checks
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
        // The expander follows the CPU, and each line it hands on says which
        // is in force.
        word if crate::Cpu::chosen_by(word).is_some() => {}
        ".setcpu" => {
            let name = match values.as_slice() {
                [value] => super::line::string(value, line),
                _ => None,
            }
            .ok_or_else(|| invalid(range, "`.setcpu` takes the CPU's name in quotes"))?;
            let name = String::from_utf8_lossy(&name).into_owned();
            match crate::Cpu::named(&name) {
                Some(_) => {}
                // ca65 knows `none` too, a CPU with no instructions.
                None if name.eq_ignore_ascii_case("none") => {
                    return Err(unsupported(range, &format!("the CPU `{name}`")));
                }
                None => return Err(invalid(range, "CPU not supported")),
            }
        }
        // The widths are the expander's to follow; ca65 allows them only on
        // the 65816.
        ".a8" | ".a16" | ".i8" | ".i16" if line.cpu() != crate::Cpu::Wdc65816 => {
            return Err(invalid(range, "Command is only valid in 65816 mode"));
        }
        ".a8" | ".a16" | ".i8" | ".i16" => {}
        ".pushcpu" => return Ok(Some(Some(CpuStack::Push))),
        ".popcpu" => return Ok(Some(Some(CpuStack::Pop))),
        ".smart" | ".autoimport" | ".list" | ".debuginfo" => switch(command)?,
        ".listbytes" => int_arg(command, &values, known, 4, 255)?,
        ".pagelen" | ".pagelength" => int_arg(command, &values, known, 32, 127)?,
        ".fileopt" | ".fopt" => file_option(command, &values, line, known)?,
        ".dbg" => debug(command)?,
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

/// Checks `.dbg`, the debug information a compiler writes. The forms
/// written by hand are lowered: `.dbg line`, which ends the last line
/// given, and `.dbg file, "name", size, time`, with the numbers written
/// out. Lines, functions and symbols with arguments need ca65's file table
/// and type checker, and are not lowered yet.
fn debug(command: &Command) -> Result<(), Diagnostic> {
    let range = command.syntax().text_range();
    let tokens: Vec<_> = command
        .tokens()
        .map(|list| list.tokens().collect())
        .unwrap_or_default();
    let Some((key, rest)) = tokens.split_first() else {
        return Err(invalid(range, "Expected an identifier"));
    };
    if key.kind() != kinds::IDENT {
        return Err(invalid(key.text_range(), "Expected an identifier"));
    }
    let key = key.text().to_ascii_lowercase();
    let kinds: Vec<_> = rest.iter().map(|t| t.kind()).collect();
    match key.as_str() {
        "line" if rest.is_empty() => Ok(()),
        "file"
            if kinds
                == [
                    kinds::COMMA,
                    kinds::STRING,
                    kinds::COMMA,
                    kinds::NUMBER,
                    kinds::COMMA,
                    kinds::NUMBER,
                ] =>
        {
            Ok(())
        }
        "file" | "func" | "line" | "sym" => Err(unsupported(
            range,
            &format!("`.dbg {key}` with these arguments"),
        )),
        _ => Err(invalid(range, "Expected FILE, FUNC, LINE or SYM")),
    }
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
