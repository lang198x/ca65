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
//! Each control command is read in its own shape, from ca65's `pseudo.c`.
//! A macro's body is kept as tokens, one `MACRO_BODY` per line, because ca65
//! reads it only when the macro is called. Inside a `.struct` or `.union`, a
//! name before a storage command is a member without a colon, and inside an
//! `.enum` a line is a member name with an optional value.
//!
//! A name at the start of a statement calls a macro if one has that name.
//! ca65 checks for a macro before it reads a label, so `m:` calls `m`. The
//! parser knows the macros and define-style macros this text defines, and
//! asks a [`Context`] about any other name. [`parse`] answers for a text read
//! on its own: before any `.include` or `.macpack`, no other macro exists;
//! after one, a name that cannot be a label or an assignment is read as a
//! call, with an *unknown* diagnostic.
//!
//! The parser checks every line. ca65 does not read lines after `.end`,
//! inside a false `.if` or in a `.repeat` that runs no times; deciding which
//! lines count is the job of preprocessing.

use std::collections::HashSet;
use std::fmt;

use lang198x_syntax::{
    BuildError, Diagnostic, DiagnosticKind, DiagnosticOutsideText, Parse, SyntaxKind, TextRange,
    TextSize, TreeBuilder,
};

use crate::lexer::{Prepared, TokenSource};
use crate::{Lexer, Mode, Profile, Token, kinds};

mod commands;
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

/// What the parser may ask about the program around the text it reads.
///
/// A file read in by `.include`, or a package read in by `.macpack`, can
/// define macros the text itself cannot show. The front end that reads the
/// whole program answers for them.
pub trait Context {
    /// Whether `name` is a macro defined outside this text by the time the
    /// current line is read, or `None` if this context cannot tell. The
    /// parser asks only about names that are not mnemonics and that this
    /// text has not defined as a macro, and it may ask more than once.
    fn is_macro(&mut self, name: &str) -> Option<bool>;
}

/// The context of a text read on its own, which cannot tell about any macro
/// defined outside it.
struct Standalone;

impl Context for Standalone {
    fn is_macro(&mut self, _name: &str) -> Option<bool> {
        None
    }
}

/// Parses `text` as ca65 source under `profile`, on its own.
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
    parse_with(text, profile, &mut Standalone)
}

/// Parses `text` like [`parse`], asking `context` about macros defined
/// outside it.
///
/// ```
/// use lang198x_ca65::{Context, Profile, kinds, parse_with};
///
/// struct Included;
///
/// impl Context for Included {
///     fn is_macro(&mut self, name: &str) -> Option<bool> {
///         Some(name == "blink")
///     }
/// }
///
/// let parse = parse_with("blink 3\n", Profile::CA65_2_19_6502, &mut Included)?;
/// assert!(parse.diagnostics().is_empty());
/// let call = parse
///     .tree()
///     .root()
///     .descendants()
///     .filter_map(|e| e.into_node())
///     .find(|n| n.kind() == kinds::MACRO_CALL);
/// assert!(call.is_some());
/// # Ok::<(), lang198x_ca65::ParseError>(())
/// ```
///
/// # Errors
///
/// As for [`parse`].
pub fn parse_with(
    text: &str,
    profile: Profile,
    context: &mut dyn Context,
) -> Result<Parse, ParseError> {
    parse_from(text, Box::new(Lexer::new(text)), profile, context)
}

/// Parses `text` from `tokens`, which cover it exactly and are not lexed
/// again: the text an expansion was made of.
pub(crate) fn parse_prepared(
    text: &str,
    tokens: Vec<Token>,
    profile: Profile,
    context: &mut dyn Context,
) -> Result<Parse, ParseError> {
    parse_from(
        text,
        Box::new(Prepared(tokens.into_iter())),
        profile,
        context,
    )
}

fn parse_from<'t>(
    text: &'t str,
    lexer: Box<dyn TokenSource + 't>,
    profile: Profile,
    context: &mut dyn Context,
) -> Result<Parse, ParseError> {
    let mut parser = Parser {
        text,
        lexer,
        mode: Mode::of(profile),
        next_start: 0,
        kinds: Vec::new(),
        starts: Vec::new(),
        pos: 0,
        depth: 0,
        name_start: 0,
        builder: TreeBuilder::new(text),
        diagnostics: Vec::new(),
        context,
        macros: HashSet::new(),
        defines: HashSet::new(),
        after_include: false,
        in_macro_body: false,
        blocks: Vec::new(),
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

/// A block whose lines read differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// `.proc` or `.scope`, whose lines are ordinary.
    Scope,
    /// `.struct` or `.union`, whose lines may be members without colons.
    Struct,
    /// `.enum`, whose lines are members.
    Enum,
}

/// What a name at the start of a statement is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Macro {
    /// A macro or define-style macro, so the statement calls it.
    Known,
    /// Perhaps a macro defined outside the text: only a front end could say.
    Unknown,
    /// Not a macro.
    Not,
}

struct Parser<'t, 'c> {
    text: &'t str,
    lexer: Box<dyn TokenSource + 't>,
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
    context: &'c mut dyn Context,
    /// Macros this text has defined so far. Names are case-sensitive.
    macros: HashSet<String>,
    /// Define-style macros this text has defined so far.
    defines: HashSet<String>,
    /// Whether an `.include` or `.macpack` has been read, so a macro might be
    /// defined that this text cannot see.
    after_include: bool,
    /// Whether the lines being read are a macro's body.
    in_macro_body: bool,
    /// The blocks open at this line, innermost last.
    blocks: Vec<Block>,
}

impl Parser<'_, '_> {
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
        if self.in_macro_body {
            self.macro_body_line();
        } else if self.blocks.last() == Some(&Block::Enum) && self.at(kinds::IDENT) {
            self.enum_member();
        } else if self.blocks.last() == Some(&Block::Struct)
            && self.at(kinds::IDENT)
            && self.peek_nth(1) == Some(kinds::CONTROL)
        {
            self.struct_member();
        } else {
            self.unnamed_label();
            if self.definition() {
                self.statement();
            }
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

    /// A line of a macro's body, or the `.endmacro` that ends it. ca65 ends
    /// a body only at `.endmacro` first on its line.
    fn macro_body_line(&mut self) {
        let ends = self.at(kinds::CONTROL)
            && [".endmacro", ".endmac"]
                .iter()
                .any(|w| w.eq_ignore_ascii_case(self.next_text()));
        if ends {
            self.in_macro_body = false;
            self.builder.start_node(kinds::COMMAND);
            self.bump();
            self.builder.finish_node();
        } else if !self.at_line_end() {
            self.builder.start_node(kinds::MACRO_BODY);
            while !self.at_line_end() {
                self.bump_next();
            }
            self.builder.finish_node();
        }
    }

    /// `name .word` inside a `.struct`: a member, then its storage.
    fn struct_member(&mut self) {
        self.builder.start_node(kinds::MEMBER);
        self.name(kinds::NAME);
        self.command();
        self.builder.finish_node();
    }

    /// `name` or `name = value` inside an `.enum`.
    fn enum_member(&mut self) {
        self.builder.start_node(kinds::MEMBER);
        self.name(kinds::NAME);
        if self.next_kind() == Some(kinds::EQ) {
            self.bump_next();
            self.expression();
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
            Some(kinds::IDENT) if self.is_mnemonic(self.next_text()) => false,
            Some(kinds::IDENT) => match self.next_macro_kind() {
                Macro::Known => false,
                // A name that can only be a label or an assignment is one;
                // anything else may call a macro from an included file.
                Macro::Unknown => {
                    matches!(
                        self.peek_nth(1),
                        Some(kinds::COLON | kinds::EQ | kinds::COLON_EQ)
                    ) || self.peek_is_set(1)
                }
                Macro::Not => true,
            },
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
            Some(kinds::CONTROL) if self.peek_is_set(0) => {
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
            Some(kinds::IDENT) => match self.next_macro_kind() {
                Macro::Known => self.macro_call(),
                Macro::Unknown => {
                    let start = self.offset(self.nth_index(0).unwrap_or(self.pos));
                    let name = self.next_text().to_owned();
                    let message = format!(
                        "`{name}` is read as a macro call; only a file that `.include` or \
                         `.macpack` reads in could define it"
                    );
                    let range = self.range(start, start + name.len());
                    self.diagnostics
                        .push(Diagnostic::new(DiagnosticKind::Unknown, range, message));
                    self.macro_call();
                }
                Macro::Not => self.error_to_line_end("expected a mnemonic"),
            },
            Some(_) => self.error_to_line_end("expected a mnemonic"),
        }
    }

    /// Whether the next name, at the start of a statement and not a
    /// mnemonic, calls a macro.
    fn next_macro_kind(&mut self) -> Macro {
        let name = self.next_text().to_owned();
        if self.macros.contains(&name) || self.defines.contains(&name) {
            return Macro::Known;
        }
        match self.context.is_macro(&name) {
            Some(true) => Macro::Known,
            Some(false) => Macro::Not,
            None if self.after_include => Macro::Unknown,
            None => Macro::Not,
        }
    }

    /// A macro's name and its arguments. As measured on ca65, arguments are
    /// split at every comma outside braces: parentheses do not protect a
    /// comma, and braces around a whole argument let it hold commas.
    fn macro_call(&mut self) {
        self.trivia();
        self.builder.start_node(kinds::MACRO_CALL);
        self.bump();
        if !self.at_line_end() {
            loop {
                self.trivia();
                self.builder.start_node(kinds::ARG);
                if self.current() == Some(kinds::L_CURLY) {
                    self.token_list();
                } else if !self.at_line_end() && !self.at(kinds::COMMA) {
                    self.builder.start_node(kinds::TOKEN_LIST);
                    while !self.at_line_end() && !self.at(kinds::COMMA) {
                        self.bump_next();
                    }
                    self.builder.finish_node();
                }
                self.builder.finish_node();
                if !self.at(kinds::COMMA) {
                    break;
                }
                self.bump_next();
            }
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

    fn at(&self, kind: SyntaxKind) -> bool {
        self.next_kind() == Some(kind)
    }

    /// Whether the `n`th token from here that is not whitespace is `.set`.
    fn peek_is_set(&self, n: usize) -> bool {
        self.nth_index(n).is_some_and(|i| {
            self.kinds[i] == kinds::CONTROL && self.text_at(i).eq_ignore_ascii_case(".set")
        })
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
        let range = self.range(start, end);
        self.diagnostics
            .push(Diagnostic::new(DiagnosticKind::Invalid, range, message));
    }

    fn range(&self, start: usize, end: usize) -> TextRange {
        // Offsets come from the lexer's tokens, which all fit in 32 bits.
        let start = TextSize::try_from(start).unwrap_or(TextSize::ZERO);
        let end = TextSize::try_from(end).unwrap_or(start);
        TextRange::new(start, end).unwrap_or(TextRange::empty(start))
    }

    fn is_mnemonic(&self, word: &str) -> bool {
        self.mode.cpu().is_mnemonic(word)
    }
}

#[cfg(test)]
mod tests;
