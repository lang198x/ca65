//! Commands that change how ca65 reads or lists the program, not what it
//! makes: the CPU, the character map, smart mode, `.autoimport`, the
//! listing, debug information and file options.
//!
//! Each follows ca65 V2.19's `pseudo.c`. The expander follows the CPU a
//! line chooses, and each line says which is in force. The rest make no bytes, so they are checked as ca65 checks
//! them and then have no effect. `.pushcpu` and `.popcpu` are reported, so
//! the assembler can keep ca65's stack of CPUs and refuse a `.popcpu` with
//! nothing to pop. `.charmap`, `.pushcharmap` and `.popcharmap` are reported
//! the same way, and the assembler keeps the map lowering reads through
//! [`Known::translate`].

use asm198x_ir::{DiagSeverity, Operation};
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

/// A change a line makes to ca65's character map, which turns the
/// characters of strings and character constants into the bytes the
/// program holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Charmap {
    /// `.charmap index, value`: the character `index` is `value` from now on.
    Set {
        /// The character.
        index: u8,
        /// The byte it becomes.
        value: u8,
    },
    /// `.pushcharmap` keeps the map. ca65 keeps 16 at most.
    Push,
    /// `.popcharmap` takes back the one kept last.
    Pop,
}

/// What a character-map command does, if `command` is one: `.charmap`,
/// `.pushcharmap` or `.popcharmap`, measured on ca65 V2.19.
pub(super) fn charmap(command: &Command, known: &dyn Known) -> Result<Option<Charmap>, Diagnostic> {
    let range = command.syntax().text_range();
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    let values: Vec<Expr> = command.values().collect();
    let none = |change| match values.as_slice() {
        [] => Ok(Some(change)),
        [extra, ..] => Err(invalid(
            extra.syntax().text_range(),
            "expected the end of the line",
        )),
    };
    match word.as_str() {
        ".pushcharmap" => none(Charmap::Push),
        ".popcharmap" => none(Charmap::Pop),
        ".charmap" => {
            let [index, value] = values.as_slice() else {
                return Err(invalid(range, "`.charmap` takes a character and its byte"));
            };
            let constant = |value: &Expr, what: &str| -> Result<u8, Diagnostic> {
                let lowered = expr::expression(value, known)?;
                let number = expr::constant(&lowered, known).ok_or_else(|| {
                    invalid(value.syntax().text_range(), "Constant expression expected")
                })?;
                u8::try_from(number).map_err(|_| invalid(value.syntax().text_range(), what))
            };
            let index = constant(index, "Index must be in byte range")?;
            let value = constant(value, "Replacement character code must be in byte range")?;
            Ok(Some(Charmap::Set { index, value }))
        }
        _ => Ok(None),
    }
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
        // The expander reads names in upper case after `.case off`.
        ".case" | ".linecont" => switch(command)?,
        ".localchar" => local_char(command, &values)?,
        ".feature" => features(command)?,
        ".listbytes" => int_arg(command, &values, known, 4, 255)?,
        ".pagelen" | ".pagelength" => int_arg(command, &values, known, 32, 127)?,
        ".fileopt" | ".fopt" => file_option(command, &values, line, known)?,
        _ => return Ok(None),
    }
    Ok(Some(None))
}

/// Checks a `.feature` line, which the expander has already put in force.
fn features(command: &Command) -> Result<(), Diagnostic> {
    crate::features::switches(command)
        .map(|_| ())
        .map_err(|(range, message)| invalid(range, &message))
}

/// The warning ca65 gives for a `.feature` line that names the deprecated
/// `addrsize`, which does nothing, or `None` for any other line.
pub(super) fn deprecated(command: &Command) -> Option<Operation> {
    let word = command.word()?.text().to_ascii_lowercase();
    let switches = crate::features::switches(command).ok()?;
    (word == ".feature" && switches.iter().any(|switch| switch.feature.is_none())).then(|| {
        Operation::Diagnose {
            severity: DiagSeverity::Warning,
            message: "Deprecated feature `addrsize`".to_owned(),
        }
    })
}

/// Checks `.localchar`'s one argument, the character constant `'@'` or
/// `'?'`, which the expander has already put in force.
fn local_char(command: &Command, values: &[Expr]) -> Result<(), Diagnostic> {
    let range = command.syntax().text_range();
    match values {
        [] => Err(invalid(range, "Expected a character constant")),
        [_, extra, ..] => Err(invalid(extra.syntax().text_range(), "Expected end-of-line")),
        [value] => match crate::expand::local_char(command) {
            Some('@' | '?') => Ok(()),
            Some(_) => Err(invalid(
                value.syntax().text_range(),
                "Invalid start character for locals",
            )),
            None => Err(invalid(
                value.syntax().text_range(),
                "Expected a character constant",
            )),
        },
    }
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

/// What a `.dbg` line tells the assembler, which keeps ca65's file table and
/// its scopes. A C compiler writes these; none makes a byte.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Dbg {
    /// `.dbg file, "name", size, time` adds `name` to the file table.
    File {
        /// The file's name.
        name: String,
    },
    /// `.dbg line, "name", line` says which line of `name` the code after
    /// it came from. ca65 refuses a name not in its file table.
    Line {
        /// The file's name.
        file: String,
    },
    /// `.dbg line` alone ends the last line given.
    EndLine,
    /// `.dbg func, "name", "type", class, "asm"` describes the `.proc`
    /// it is in, which must be named `asm`, and may be described once.
    Func {
        /// The name of the `.proc` it describes.
        asm_name: String,
    },
    /// `.dbg sym, "name", "type", class, ...` describes a variable. One
    /// that is not `auto` names a symbol, which must be defined by the end
    /// of the program, in the scope of the line or one around it.
    Sym {
        /// The symbol it names, if it is not `auto`.
        asm_name: Option<String>,
    },
}

/// Reads a `.dbg` line as ca65 V2.19's `dbginfo.c` does, with its errors.
/// The numbers a compiler writes are read; an expression in their place is
/// not lowered yet.
pub(super) fn debug(command: &Command, line: &ActiveLine) -> Result<Option<Dbg>, Diagnostic> {
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    if word != ".dbg" {
        return Ok(None);
    }
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
    let mut at = Tokens {
        tokens: rest,
        next: 0,
        range,
        line,
    };
    let dbg = match key.as_str() {
        "line" if rest.is_empty() => Dbg::EndLine,
        "file" => {
            at.comma()?;
            let name = at.string("String constant expected")?;
            at.comma()?;
            at.number()?;
            at.comma()?;
            at.number()?;
            Dbg::File { name }
        }
        "line" => {
            at.comma()?;
            let file = at.string("Expected a string constant")?;
            at.comma()?;
            if at.number()? < 0 {
                return Err(invalid(range, "Line number is out of valid range"));
            }
            Dbg::Line { file }
        }
        "func" => {
            at.comma()?;
            at.string("String constant expected")?;
            at.comma()?;
            at.type_value()?;
            at.comma()?;
            at.class(&["extern", "static"])?;
            at.comma()?;
            let asm_name = at.string("Expected a string constant")?;
            Dbg::Func { asm_name }
        }
        "sym" => {
            at.comma()?;
            at.string("Expected a string constant")?;
            at.comma()?;
            at.type_value()?;
            at.comma()?;
            let class = at.class(&["auto", "extern", "register", "static"])?;
            at.comma()?;
            if class == "auto" {
                at.number()?;
                Dbg::Sym { asm_name: None }
            } else {
                let asm_name = at.string("Expected a string constant")?;
                if class == "register" {
                    at.comma()?;
                    at.number()?;
                }
                Dbg::Sym {
                    asm_name: Some(asm_name),
                }
            }
        }
        _ => return Err(invalid(range, "Expected FILE, FUNC, LINE or SYM")),
    };
    at.end()?;
    Ok(Some(dbg))
}

/// The tokens after `.dbg`'s keyword, read in order.
struct Tokens<'a> {
    tokens: &'a [lang198x_syntax::SyntaxToken],
    next: usize,
    range: lang198x_syntax::TextRange,
    line: &'a ActiveLine,
}

impl Tokens<'_> {
    fn peek(&self) -> Option<&lang198x_syntax::SyntaxToken> {
        self.tokens.get(self.next)
    }

    /// Where a problem with the next token is: that token, or the line.
    fn here(&self) -> lang198x_syntax::TextRange {
        self.peek()
            .map_or(self.range, lang198x_syntax::SyntaxToken::text_range)
    }

    fn comma(&mut self) -> Result<(), Diagnostic> {
        match self.peek() {
            Some(token) if token.kind() == kinds::COMMA => {
                self.next += 1;
                Ok(())
            }
            _ => Err(invalid(self.here(), "Expected `,`")),
        }
    }

    fn string(&mut self, message: &str) -> Result<String, Diagnostic> {
        let bytes = self
            .peek()
            .filter(|token| token.kind() == kinds::STRING)
            .and_then(|token| self.line.string(token))
            .ok_or_else(|| invalid(self.here(), message))?;
        self.next += 1;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// A number, perhaps negative, as a compiler writes one.
    fn number(&mut self) -> Result<i64, Diagnostic> {
        let negative = self.peek().is_some_and(|t| t.kind() == kinds::MINUS);
        if negative {
            self.next += 1;
        }
        let value = self
            .peek()
            .filter(|token| token.kind() == kinds::NUMBER)
            .and_then(|token| crate::number::value(token.text()))
            .ok_or_else(|| unsupported(self.here(), "a `.dbg` value other than a number"))?;
        self.next += 1;
        // A number with more after it is an expression.
        if self.peek().is_some_and(|t| t.kind() != kinds::COMMA) {
            return Err(unsupported(
                self.here(),
                "a `.dbg` value other than a number",
            ));
        }
        Ok(if negative { -value } else { value })
    }

    /// A type: a string of hex digits, two for each byte.
    fn type_value(&mut self) -> Result<(), Diagnostic> {
        let at = self.here();
        let text = self.string("Expected a string constant")?;
        if text.len() < 2 || text.len() % 2 != 0 {
            return Err(invalid(at, "Type value has invalid length"));
        }
        if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid(at, "Type value contains invalid characters"));
        }
        Ok(())
    }

    /// A storage class, one of `classes`, in lower case.
    fn class(&mut self, classes: &[&str]) -> Result<String, Diagnostic> {
        let class = self
            .peek()
            .filter(|token| token.kind() == kinds::IDENT)
            .map(|token| token.text().to_ascii_lowercase())
            .filter(|class| classes.contains(&class.as_str()))
            .ok_or_else(|| invalid(self.here(), "Storage class specifier expected"))?;
        self.next += 1;
        Ok(class)
    }

    fn end(&self) -> Result<(), Diagnostic> {
        match self.peek() {
            None => Ok(()),
            Some(token) => Err(invalid(token.text_range(), "Expected the end of the line")),
        }
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
