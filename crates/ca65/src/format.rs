//! Laying out source: the formatter.
//!
//! It follows Lang198x's shared syntax layer decision for line-oriented
//! languages. It never adds, removes or reorders lines, and it changes only
//! the spaces it knows are safe:
//!
//! - a label starts in column 1, spelled exactly as written, so `Z :`
//!   keeps its space (`Z:` would ask for the zero page);
//! - a statement starts at the statement column, or one space after a
//!   longer label, and its own text, from its first word to its last, is
//!   kept exactly as written;
//! - a comment after a label or statement starts at the comment column, or
//!   one space after a longer line, and is kept as written;
//! - spaces at the end of a line are removed.
//!
//! Some lines are kept exactly as written: a line holding only a comment,
//! a line the parser could not read and the lines after it up to the next
//! one it can, a macro's body,
//! since a macro substitutes its arguments into the body's tokens, and
//! every line after `.end`, which ca65 never reads.

use lang198x_syntax::TextRange;

use crate::ast::{Line, SourceFile, Statement, View};
use crate::{ParseError, Profile, parse};

/// The column a statement starts in, counting from 0.
const STATEMENT_COLUMN: usize = 8;

/// The column a comment after a statement starts in, counting from 0.
const COMMENT_COLUMN: usize = 32;

/// Lays out `text` as ca65 source under `profile`.
///
/// The result reads as the same program: its tokens, other than spaces, are
/// the same tokens in the same order, and formatting it again changes
/// nothing.
///
/// ```
/// use lang198x_ca65::{Profile, format};
///
/// let text = "loop:  dex ; next\n  bne loop\n";
/// let formatted = format(text, Profile::CA65_2_19_6502).expect("formats");
/// assert_eq!(
///     formatted,
///     "loop:   dex                     ; next\n        bne loop\n"
/// );
/// ```
///
/// # Errors
///
/// A [`ParseError`] if `text` is too long to parse.
pub fn format(text: &str, profile: Profile) -> Result<String, ParseError> {
    let parse = parse(text, profile)?;
    let Some(file) = SourceFile::cast(parse.tree().root()) else {
        return Ok(text.to_owned());
    };
    let problems: Vec<TextRange> = parse.diagnostics().iter().map(|d| d.range()).collect();
    let mut out = String::with_capacity(text.len());
    let mut ended = false;
    // Whether a line could not be read and no readable line has followed
    // it yet. ca65 reads an unclosed string or character on into the lines
    // after it, so their spaces can change what it finds there.
    let mut after_problem = false;
    for line in file.lines() {
        let range = line.syntax().text_range();
        // A problem at the very end counts too, for a last line with no
        // newline; keeping one line too many as written is always safe.
        let unreadable = problems
            .iter()
            .any(|p| p.start() >= range.start() && p.start() <= range.end());
        if ended || after_problem {
            out.push_str(slice(text, range));
            let something = line.label().is_some()
                || line.unnamed_label().is_some()
                || line.statement().is_some();
            after_problem = after_problem && (unreadable || !something);
            continue;
        }
        match line.statement() {
            Some(Statement::MacroBody(_)) => out.push_str(slice(text, range)),
            _ if unreadable => out.push_str(&kept(&line, text)),
            _ => out.push_str(&laid_out(&line, text)),
        }
        after_problem = unreadable;
        ended = ends(&line);
    }
    Ok(out)
}

/// Whether `line` is `.end`, after which ca65 reads nothing.
fn ends(line: &Line) -> bool {
    matches!(
        line.statement(),
        Some(Statement::Command(command))
            if command.word().is_some_and(|w| w.text().eq_ignore_ascii_case(".end"))
    )
}

/// `line` as written, without the spaces at its end.
fn kept(line: &Line, text: &str) -> String {
    let all = slice(text, line.syntax().text_range());
    let (body, newline) = split_newline(all);
    format!("{}{newline}", body.trim_end_matches([' ', '\t']))
}

/// `text` and the line ending at its end, which may be empty.
fn split_newline(text: &str) -> (&str, &str) {
    let body = text.trim_end_matches(['\n', '\r']);
    (body, &text[body.len()..])
}

/// `line` laid out, or as written where that is not safe.
fn laid_out(line: &Line, text: &str) -> String {
    let all = slice(text, line.syntax().text_range());
    let (_, newline) = split_newline(all);
    let label = match (line.unnamed_label(), line.label()) {
        (Some(unnamed), None) => Some(unnamed.syntax().text_range()),
        (None, Some(label)) => Some(label.syntax().text_range()),
        (None, None) => None,
        // Two labels on a line: rare enough to keep as written.
        (Some(_), Some(_)) => return kept(line, text),
    };
    let statement = line
        .statement()
        .map(|statement| statement.syntax().text_range());
    let comment = line.comment().map(|comment| comment.text_range());
    if line.errors().next().is_some() || (label.is_none() && statement.is_none()) {
        // Comment-only lines and blank ones keep their text.
        return kept(line, text);
    }
    let mut out = String::new();
    if let Some(label) = label {
        out.push_str(slice(text, label).trim_end_matches([' ', '\t']));
    }
    if let Some(statement) = statement {
        pad_to(&mut out, STATEMENT_COLUMN);
        out.push_str(slice(text, statement).trim_end_matches([' ', '\t']));
    }
    if let Some(comment) = comment {
        pad_to(&mut out, COMMENT_COLUMN);
        out.push_str(slice(text, comment).trim_end_matches([' ', '\t']));
    }
    out.push_str(newline);
    out
}

/// Pads `out` with spaces to `column`, or by one space if it is already
/// there or past it, so two parts never run together.
fn pad_to(out: &mut String, column: usize) {
    let width = out.chars().count();
    let spaces = if width < column { column - width } else { 1 };
    out.extend(std::iter::repeat_n(' ', spaces));
}

fn slice(text: &str, range: TextRange) -> &str {
    let start = range.start().get() as usize;
    let end = range.end().get() as usize;
    text.get(start..end).unwrap_or_default()
}

#[cfg(test)]
mod tests;
