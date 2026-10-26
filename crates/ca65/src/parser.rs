//! Building a syntax tree from tokens.
//!
//! The parser asks the lexer for one line of tokens at a time, in the current
//! [`Mode`], and turns each line into a `LINE` node. Every token, including
//! whitespace and comments, lands in the tree, so the tree gives back the
//! source exactly.
//!
//! The rules follow ca65 V2.19's `OneLine` and `GetEA`:
//!
//! - A line may start with an unnamed label, `:` or `@:`.
//! - Then a name that is not a mnemonic defines a symbol. With `=`, `:=` or
//!   `.set` after it, the line is an assignment and ends there. Otherwise a
//!   colon must follow, and the name is a label.
//! - Then comes one statement: a control command or an instruction.
//! - An instruction operand that starts with `(` is always indirect, so
//!   `lda (1+2)*3` is refused. `#` makes it immediate, and `,x` or `,y`
//!   indexes it.
//!
//! Where a line does not fit, the parser records a diagnostic, wraps what it
//! could not read in an `ERROR` node, and carries on with the next line.
//!
//! So far a control command keeps its arguments as written, in an `ARGS`
//! node, and a name that is neither a label nor an assignment is refused, as
//! ca65 refuses it when no macro has that name. Each command's own shape and
//! macro calls come next.

use std::fmt;

use lang198x_syntax::{
    BuildError, Diagnostic, DiagnosticKind, DiagnosticOutsideText, Parse, SyntaxKind, TextRange,
    TextSize, TreeBuilder,
};

use crate::{Lexer, Mode, Profile, kinds};

mod expression;

/// How deeply expressions may nest before the rest of the line is skipped.
/// The limit keeps the parser's recursion well inside a thread's stack.
const MAX_DEPTH: usize = 128;

/// Why [`parse`] could not produce a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// The tree could not be built. The text is longer than 4 GiB, or the
    /// tree would need more than about four billion nodes and tokens.
    Build(BuildError),
    /// A diagnostic did not fit the text. This is a bug in this crate.
    Diagnostic(DiagnosticOutsideText),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build(error) => write!(f, "the syntax tree could not be built: {error}"),
            Self::Diagnostic(error) => write!(f, "a diagnostic did not fit the text: {error:?}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses `text` as ca65 source under `profile`.
///
/// ```
/// use lang198x_ca65::{Profile, kinds, parse};
///
/// let parse = parse("loop: dex\n bne loop\n", Profile::CA65_2_19_6502)?;
/// assert!(parse.diagnostics().is_empty());
/// let line = parse.tree().root().children().next().and_then(|c| c.into_node());
/// assert_eq!(line.map(|l| l.kind()), Some(kinds::LINE));
/// # Ok::<(), lang198x_ca65::ParseError>(())
/// ```
///
/// Broken source still parses: the problems are in the diagnostics.
///
/// # Errors
///
/// Fails only when no tree can be built at all, which needs text longer than
/// 4 GiB.
pub fn parse(text: &str, profile: Profile) -> Result<Parse, ParseError> {
    let mut parser = Parser {
        text,
        lexer: Lexer::new(text),
        mode: Mode::of(profile),
        next_start: 0,
        kinds: Vec::new(),
        starts: Vec::new(),
        pos: 0,
        depth: 0,
        name_start: 0,
        builder: TreeBuilder::new(text),
        diagnostics: Vec::new(),
    };
    parser.source_file();
    let Parser {
        builder,
        lexer,
        diagnostics: syntax,
        ..
    } = parser;
    let tree = builder.finish().map_err(ParseError::Build)?;
    let mut diagnostics = lexer.diagnostics().to_vec();
    diagnostics.extend(syntax);
    diagnostics.sort_by_key(|d| (d.range().start(), d.range().end()));
    Parse::new(tree, diagnostics).map_err(ParseError::Diagnostic)
}

struct Parser<'t> {
    text: &'t str,
    lexer: Lexer<'t>,
    /// The mode the next line is read in.
    mode: Mode,
    /// Where the next token from the lexer starts.
    next_start: usize,
    /// The current line's tokens.
    kinds: Vec<SyntaxKind>,
    /// The byte offset of each of the line's tokens, then the end of the line.
    starts: Vec<usize>,
    /// The next of the line's tokens to add to the tree.
    pos: usize,
    /// How deeply the current expression nests.
    depth: usize,
    /// Where the name being defined on this line starts.
    name_start: usize,
    builder: TreeBuilder<'t>,
    diagnostics: Vec<Diagnostic>,
}

impl Parser<'_> {
    fn source_file(&mut self) {
        self.builder.start_node(kinds::SOURCE_FILE);
        while self.read_line() {
            self.line();
        }
        self.builder.finish_node();
    }

    /// Reads the next line's tokens from the lexer, up to and including its
    /// line ending. Returns `false` at the end of the text.
    fn read_line(&mut self) -> bool {
        self.kinds.clear();
        self.starts.clear();
        self.pos = 0;
        while let Some(token) = self.lexer.next_token(self.mode) {
            self.kinds.push(token.kind());
            self.starts.push(self.next_start);
            self.next_start += token.len().to_usize();
            if token.kind() == SyntaxKind::NEWLINE {
                break;
            }
        }
        self.starts.push(self.next_start);
        !self.kinds.is_empty()
    }

    fn line(&mut self) {
        self.builder.start_node(kinds::LINE);
        self.trivia();
        self.unnamed_label();
        if self.definition() {
            self.statement();
        }
        if !self.at_line_end() {
            self.error_to_line_end("expected the end of the line");
        }
        // Whatever is left is trivia, a comment and the line ending.
        while self.pos < self.kinds.len() {
            self.bump();
        }
        self.builder.finish_node();
    }

    /// `:` or `@:` at the start of a line.
    fn unnamed_label(&mut self) {
        match self.current() {
            Some(kinds::COLON) => {
                self.builder.start_node(kinds::UNNAMED_LABEL);
                self.bump();
                self.builder.finish_node();
            }
            Some(kinds::UNNAMED) if self.text_at(self.pos).starts_with('@') => {
                self.builder.start_node(kinds::UNNAMED_LABEL);
                if self.text_at(self.pos).len() > 1 {
                    let (start, end) = (self.offset(self.pos), self.offset(self.pos + 1));
                    self.report(start, end, "an unnamed label is defined with `@` alone");
                }
                self.bump();
                self.expect(kinds::COLON, "expected `:` after `@`");
                self.builder.finish_node();
            }
            _ => {}
        }
    }

    /// A label or an assignment, if the line has one. Returns whether a
    /// statement may follow: an assignment ends its line.
    fn definition(&mut self) -> bool {
        let defines = match self.next_kind() {
            Some(kinds::CHEAP_IDENT | kinds::COLON_COLON) => true,
            Some(kinds::IDENT) => !self.is_mnemonic(self.next_text()),
            _ => false,
        };
        if !defines {
            return true;
        }
        self.trivia();
        self.name_start = self.offset(self.pos);
        let checkpoint = self.builder.checkpoint();
        self.name(kinds::NAME);
        match self.next_kind() {
            Some(kinds::EQ | kinds::COLON_EQ) => {
                self.builder.start_node_at(checkpoint, kinds::ASSIGNMENT);
                self.bump_next();
                self.expression();
                self.builder.finish_node();
                false
            }
            Some(kinds::CONTROL) if self.next_text().eq_ignore_ascii_case(".set") => {
                self.builder.start_node_at(checkpoint, kinds::ASSIGNMENT);
                self.bump_next();
                self.expression();
                self.builder.finish_node();
                false
            }
            Some(kinds::COLON) => {
                self.builder.start_node_at(checkpoint, kinds::LABEL);
                self.bump_next();
                self.builder.finish_node();
                true
            }
            _ => {
                // The whole line is refused, from the name on.
                let start = self.offset(self.pos);
                self.builder.start_node_at(checkpoint, SyntaxKind::ERROR);
                while !self.at_line_end() {
                    self.bump_next();
                }
                self.builder.finish_node();
                let name_start = self.name_start;
                self.report(
                    name_start.min(start),
                    self.offset(self.pos),
                    "expected `:` after a name to make it a label",
                );
                false
            }
        }
    }

    /// A symbol name: a cheap local, or names joined by `::`, perhaps with a
    /// leading `::` for the global scope. Wrapped in a node of `kind`.
    fn name(&mut self, kind: SyntaxKind) {
        self.builder.start_node(kind);
        if self.current() == Some(kinds::CHEAP_IDENT) {
            self.bump();
        } else {
            if self.current() == Some(kinds::COLON_COLON) {
                self.bump();
                self.expect_name();
            } else {
                self.bump();
            }
            while self.next_kind() == Some(kinds::COLON_COLON) {
                self.bump_next();
                self.expect_name();
            }
        }
        self.builder.finish_node();
    }

    fn expect_name(&mut self) {
        if self.next_kind() == Some(kinds::IDENT) {
            self.bump_next();
        } else {
            self.missing("expected a name after `::`");
        }
    }

    fn statement(&mut self) {
        match self.next_kind() {
            None | Some(SyntaxKind::COMMENT | SyntaxKind::NEWLINE) => {}
            Some(kinds::CONTROL) => self.command(),
            Some(kinds::IDENT) if self.is_mnemonic(self.next_text()) => self.instruction(),
            Some(_) => self.error_to_line_end("expected a mnemonic"),
        }
    }

    /// A control command, with its arguments kept as written.
    fn command(&mut self) {
        self.trivia();
        self.builder.start_node(kinds::COMMAND);
        self.bump();
        if !self.at_line_end() {
            self.trivia();
            self.builder.start_node(kinds::ARGS);
            while !self.at_line_end() {
                self.bump_next();
            }
            self.builder.finish_node();
        }
        self.builder.finish_node();
    }

    fn instruction(&mut self) {
        self.trivia();
        self.builder.start_node(kinds::INSTRUCTION);
        self.bump();
        if !self.at_line_end() {
            self.operand();
        }
        self.builder.finish_node();
    }

    /// An instruction's operand, as ca65's `GetEA` reads it.
    fn operand(&mut self) {
        self.trivia();
        self.builder.start_node(kinds::OPERAND);
        if self.current() == Some(kinds::ADDR_SIZE) {
            self.bump_next();
        }
        match self.next_kind() {
            None | Some(SyntaxKind::COMMENT | SyntaxKind::NEWLINE) => {}
            Some(kinds::HASH) => {
                self.trivia();
                self.builder.start_node(kinds::IMMEDIATE);
                self.bump();
                self.expression();
                self.builder.finish_node();
            }
            Some(kinds::REGISTER) if self.next_text().eq_ignore_ascii_case("a") => {
                self.bump_next();
            }
            Some(kinds::L_PAREN) => self.indirect(),
            Some(kinds::L_BRACK) => self.long_indirect(),
            Some(_) => {
                self.expression();
                if self.next_kind() == Some(kinds::COMMA) {
                    if self.peek_nth(1) == Some(kinds::REGISTER) {
                        self.trivia();
                        self.builder.start_node(kinds::INDEX);
                        self.bump();
                        self.bump_next();
                        self.builder.finish_node();
                    } else {
                        // `address, target`, which some CPUs' bit branches
                        // take. The instruction decides whether it fits.
                        self.bump_next();
                        self.builder.finish_node();
                        self.operand();
                        return;
                    }
                }
            }
        }
        self.builder.finish_node();
    }

    /// `(address)`, `(address,x)` or `(address),y`.
    fn indirect(&mut self) {
        self.trivia();
        self.builder.start_node(kinds::INDIRECT);
        self.bump();
        self.expression();
        let indexed = self.next_kind() == Some(kinds::COMMA);
        if indexed {
            self.bump_next();
        }
        let closed = (!indexed || self.require_register(&["x", "s"], "expected `x` or `s`"))
            && self.require(kinds::R_PAREN, "expected `)`");
        if closed && self.next_kind() == Some(kinds::COMMA) {
            self.bump_next();
            self.require_register(&["y"], "expected `y`");
        }
        self.builder.finish_node();
    }

    /// `[address]` or `[address],y`.
    fn long_indirect(&mut self) {
        self.trivia();
        self.builder.start_node(kinds::LONG_INDIRECT);
        self.bump();
        self.expression();
        if self.require(kinds::R_BRACK, "expected `]`") && self.next_kind() == Some(kinds::COMMA) {
            self.bump_next();
            self.require_register(&["y"], "expected `y`");
        }
        self.builder.finish_node();
    }

    /// One of `registers`, in any case. Otherwise the rest of the line is
    /// refused with `message`, and the result is `false`.
    fn require_register(&mut self, registers: &[&str], message: &str) -> bool {
        let found = self.next_kind() == Some(kinds::REGISTER)
            && registers
                .iter()
                .any(|r| r.eq_ignore_ascii_case(self.next_text()));
        if found {
            self.bump_next();
        } else {
            self.refuse(message);
        }
        found
    }

    /// A token of `kind`. Otherwise the rest of the line is refused with
    /// `message`, and the result is `false`.
    fn require(&mut self, kind: SyntaxKind, message: &str) -> bool {
        let found = self.next_kind() == Some(kind);
        if found {
            self.bump_next();
        } else {
            self.refuse(message);
        }
        found
    }

    /// Reports `message` where something was missing at the end of the line,
    /// or over the rest of the line where something else stands instead.
    fn refuse(&mut self, message: &str) {
        if self.at_line_end() {
            self.missing(message);
        } else {
            self.error_to_line_end(message);
        }
    }

    // Tokens.

    fn current(&self) -> Option<SyntaxKind> {
        self.kinds.get(self.pos).copied()
    }

    /// The index of the `n`th token from here that is not whitespace.
    fn nth_index(&self, n: usize) -> Option<usize> {
        (self.pos..self.kinds.len())
            .filter(|&i| self.kinds[i] != SyntaxKind::WHITESPACE)
            .nth(n)
    }

    fn next_kind(&self) -> Option<SyntaxKind> {
        self.peek_nth(0)
    }

    fn peek_nth(&self, n: usize) -> Option<SyntaxKind> {
        self.nth_index(n).map(|i| self.kinds[i])
    }

    /// The text of the next token that is not whitespace.
    fn next_text(&self) -> &str {
        self.nth_index(0).map_or("", |i| self.text_at(i))
    }

    fn text_at(&self, index: usize) -> &str {
        self.text
            .get(self.offset(index)..self.offset(index + 1))
            .unwrap_or("")
    }

    fn offset(&self, index: usize) -> usize {
        self.starts
            .get(index)
            .or(self.starts.last())
            .copied()
            .unwrap_or(0)
    }

    /// Adds the current token to the tree.
    fn bump(&mut self) {
        if let Some(kind) = self.current() {
            let len = self.offset(self.pos + 1) - self.offset(self.pos);
            // The lexer measured every token in 32 bits.
            let len = TextSize::try_from(len).unwrap_or(TextSize::ZERO);
            self.builder.token(kind, len);
            self.pos += 1;
        }
    }

    /// Adds any whitespace, then the next token, to the tree.
    fn bump_next(&mut self) {
        self.trivia();
        self.bump();
    }

    fn trivia(&mut self) {
        while self.current() == Some(SyntaxKind::WHITESPACE) {
            self.bump();
        }
    }

    fn at_line_end(&self) -> bool {
        matches!(
            self.next_kind(),
            None | Some(SyntaxKind::COMMENT | SyntaxKind::NEWLINE)
        )
    }

    fn expect(&mut self, kind: SyntaxKind, message: &str) {
        if self.next_kind() == Some(kind) {
            self.bump_next();
        } else {
            self.missing(message);
        }
    }

    /// Reports that something is missing before the next token.
    fn missing(&mut self, message: &str) {
        let at = self.offset(self.nth_index(0).unwrap_or(self.pos));
        self.report(at, at, message);
    }

    /// Wraps the rest of the line, up to any comment, in an `ERROR` node and
    /// reports it.
    fn error_to_line_end(&mut self, message: &str) {
        self.trivia();
        let start = self.offset(self.pos);
        self.builder.start_node(SyntaxKind::ERROR);
        while !self.at_line_end() {
            self.bump_next();
        }
        self.builder.finish_node();
        self.report(start, self.offset(self.pos), message);
    }

    fn report(&mut self, start: usize, end: usize, message: &str) {
        // Offsets come from the lexer's tokens, which all fit in 32 bits.
        let start = TextSize::try_from(start).unwrap_or(TextSize::ZERO);
        let end = TextSize::try_from(end).unwrap_or(start);
        let range = TextRange::new(start, end).unwrap_or(TextRange::empty(start));
        self.diagnostics
            .push(Diagnostic::new(DiagnosticKind::Invalid, range, message));
    }

    fn is_mnemonic(&self, word: &str) -> bool {
        self.mode.cpu().is_mnemonic(word)
    }
}

#[cfg(test)]
mod tests;
