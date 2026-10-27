//! Running a program's lines and its conditionals.
//!
//! The expander follows ca65 V2.19's own model (`OneLine` in `main.c`, and
//! `DoConditionals` and `CheckConditionals` in `condasm.c`). It reads the
//! program's lines in order and keeps the stack of open conditionals.
//!
//! While lines run, a conditional command at the start of a statement opens,
//! turns or closes a level, asking the host for its condition. A line that is
//! not a conditional, and is not `.end`, goes to the host.
//!
//! While lines do not run, ca65 skips them token by token, looking only for
//! conditional commands, wherever they are on a line. So `.byte 1 .endif`
//! ends a skipped branch, and anything after the `.endif` on its line is
//! refused. A skipped branch's conditions are never asked for.
//!
//! The lexer's problems count on every line, skipped or not, as ca65's
//! scanner reads every token. Syntax problems count only on lines that run.

use lang198x_syntax::{
    Diagnostic, DiagnosticKind, FileId, Parse, Span, SyntaxElement, SyntaxToken, TextId, TextRange,
};

use super::conditional::{Conditionals, Misfit};
use crate::ast::{Command, Expr, Line, NameRef, SourceFile, Statement, View};
use crate::{Cpu, Mode, ParseError, Profile, kinds, lex, parse};

/// Bounds on how much work expansion may do, so a program that never ends
/// stops with a diagnostic instead.
///
/// ```
/// use lang198x_ca65::expand::Limits;
///
/// let mut limits = Limits::default();
/// limits.lines = 1000;
/// assert_eq!(Limits::default().lines, 10_000_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Limits {
    /// How many lines may be read in all, run or skipped.
    pub lines: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { lines: 10_000_000 }
    }
}

/// What the expander needs from whatever assembles its lines.
///
/// ca65 reads a program once, from the top, so each answer is about the
/// symbols the host has seen so far.
pub trait Host {
    /// Evaluates `expr`, the condition of an `.if` or `.elseif`.
    ///
    /// # Errors
    ///
    /// A message saying why the expression has no value yet, such as a name
    /// defined further on: ca65 needs a constant. The expander reports it at
    /// the expression and takes the condition as false.
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String>;

    /// Whether `name` is defined, for `.ifdef` and `.ifndef`.
    fn is_defined(&mut self, name: &NameRef) -> bool;

    /// Whether `name` has been used, for `.ifref` and `.ifnref`.
    fn is_referenced(&mut self, name: &NameRef) -> bool;

    /// Whether `expr` has a value now, for `.ifconst` and `.ifnconst`.
    fn is_constant(&mut self, expr: &Expr) -> bool;
}

/// A line for the host to assemble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLine {
    line: Line,
    text: TextId,
}

impl ActiveLine {
    /// Returns the line.
    #[must_use]
    pub fn line(&self) -> &Line {
        &self.line
    }

    /// Returns the text the line is in.
    #[must_use]
    pub fn text(&self) -> TextId {
        self.text
    }

    /// Returns where the whole line is, in [`text`](Self::text).
    #[must_use]
    pub fn span(&self) -> Span {
        Span::new(self.text, self.line.syntax().text_range())
    }
}

/// Runs a program's lines, handing each one that assembles to a [`Host`].
///
/// ```
/// use lang198x_ca65::ast::{Expr, NameRef, View};
/// use lang198x_ca65::expand::{Expander, Host};
/// use lang198x_ca65::Profile;
///
/// struct Nothing;
///
/// impl Host for Nothing {
///     fn evaluate(&mut self, _: &Expr) -> Result<i64, String> {
///         Ok(0)
///     }
///     fn is_defined(&mut self, _: &NameRef) -> bool {
///         false
///     }
///     fn is_referenced(&mut self, _: &NameRef) -> bool {
///         false
///     }
///     fn is_constant(&mut self, _: &Expr) -> bool {
///         true
///     }
/// }
///
/// let mut expander = Expander::new(".if 0\n nop\n.else\n rts\n.endif\n", Profile::CA65_2_19_6502)?;
/// let line = expander.next_line(&mut Nothing).expect("one line runs");
/// assert_eq!(line.line().syntax().text().trim(), "rts");
/// assert!(expander.next_line(&mut Nothing).is_none());
/// assert!(expander.diagnostics().is_empty());
/// # Ok::<(), lang198x_ca65::ParseError>(())
/// ```
#[derive(Debug)]
pub struct Expander {
    mode: Mode,
    limits: Limits,
    text: TextId,
    lines: Vec<Line>,
    /// The syntax problems on each line, reported only if it runs.
    line_problems: Vec<Vec<Diagnostic>>,
    next: usize,
    lines_read: usize,
    conditionals: Conditionals,
    ended: bool,
    finished: bool,
    diagnostics: Vec<(TextId, Diagnostic)>,
}

/// The commands that open, turn or close a conditional.
const CONDITIONALS: &[&str] = &[
    ".else",
    ".elseif",
    ".endif",
    ".if",
    ".ifblank",
    ".ifconst",
    ".ifdef",
    ".ifnblank",
    ".ifnconst",
    ".ifndef",
    ".ifnref",
    ".ifp02",
    ".ifp02x",
    ".ifp4510",
    ".ifp45gs02",
    ".ifp6280",
    ".ifp816",
    ".ifpc02",
    ".ifpce02",
    ".ifpdtv",
    ".ifpm740",
    ".ifpsc02",
    ".ifpsweet16",
    ".ifpwc02",
    ".ifref",
];

pub(super) fn is_conditional(word: &str) -> bool {
    CONDITIONALS.iter().any(|w| w.eq_ignore_ascii_case(word))
}

impl Expander {
    /// Starts running `text`, the program's root file, under `profile`.
    ///
    /// # Errors
    ///
    /// Fails only when the text cannot be parsed at all, which needs text
    /// longer than 4 GiB.
    pub fn new(text: &str, profile: Profile) -> Result<Self, ParseError> {
        Self::with_limits(text, profile, Limits::default())
    }

    /// Starts running `text` like [`new`](Self::new), within `limits`.
    ///
    /// # Errors
    ///
    /// As for [`new`](Self::new).
    pub fn with_limits(text: &str, profile: Profile, limits: Limits) -> Result<Self, ParseError> {
        let mode = Mode::of(profile);
        let parse: Parse = parse(text, profile)?;
        let lines: Vec<Line> = SourceFile::cast(parse.tree().root())
            .map(|file| file.lines().collect())
            .unwrap_or_default();
        let id = TextId::File(FileId::new(0));
        let lexical = lex(text, mode);
        let mut diagnostics = Vec::new();
        let mut line_problems = vec![Vec::new(); lines.len()];
        for diagnostic in parse.diagnostics() {
            if lexical.diagnostics().contains(diagnostic) {
                diagnostics.push((id, diagnostic.clone()));
            } else {
                let start = diagnostic.range().start();
                let index = lines
                    .partition_point(|line| line.syntax().text_range().end() <= start)
                    .min(lines.len().saturating_sub(1));
                if let Some(problems) = line_problems.get_mut(index) {
                    problems.push(diagnostic.clone());
                }
            }
        }
        Ok(Self {
            mode,
            limits,
            text: id,
            lines,
            line_problems,
            next: 0,
            lines_read: 0,
            conditionals: Conditionals::default(),
            ended: false,
            finished: false,
            diagnostics,
        })
    }

    /// Returns the next line to assemble, or `None` when the program is
    /// done. `host` answers the conditions met on the way.
    pub fn next_line(&mut self, host: &mut dyn Host) -> Option<ActiveLine> {
        while !self.ended {
            let Some(line) = self.lines.get(self.next).cloned() else {
                break;
            };
            let index = self.next;
            self.next += 1;
            if self.lines_read >= self.limits.lines {
                self.report(
                    line.syntax().text_range(),
                    "the program reads too many lines",
                );
                self.ended = true;
                break;
            }
            self.lines_read += 1;
            if !self.conditionals.active() {
                self.skip(&line, index, host);
                continue;
            }
            for problem in self.line_problems.get(index).cloned().unwrap_or_default() {
                self.diagnostics.push((self.text, problem));
            }
            let command = match line.statement() {
                Some(Statement::Command(command)) => Some(command),
                _ => None,
            };
            let word = command
                .as_ref()
                .and_then(Command::word)
                .map(|w| w.text().to_ascii_lowercase());
            match (command, word.as_deref()) {
                (Some(command), Some(word)) if is_conditional(word) => {
                    self.conditional(&command, word, host);
                }
                (Some(_), Some(".end")) => self.ended = true,
                _ => {
                    return Some(ActiveLine {
                        line,
                        text: self.text,
                    });
                }
            }
        }
        self.finish();
        None
    }

    /// Returns the problems found so far, each with the text it is in.
    #[must_use]
    pub fn diagnostics(&self) -> &[(TextId, Diagnostic)] {
        &self.diagnostics
    }

    /// Reports any conditional left open, once, when the program is done.
    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        for site in self.conditionals.close_all() {
            self.report(site, "this conditional is never closed with `.endif`");
        }
    }

    /// A conditional command at the start of a statement, while lines run.
    fn conditional(&mut self, command: &Command, word: &str, host: &mut dyn Host) {
        let site = command.syntax().text_range();
        match word {
            ".else" => {
                let result = self.conditionals.otherwise();
                self.misfit(result, site, word);
            }
            ".elseif" => {
                // While lines run, the branch before is taken, so the chain's
                // later branches are not, and the condition is not asked for.
                let result = self.conditionals.otherwise();
                self.misfit(result, site, word);
                if result.is_ok() {
                    self.conditionals.open_chained(site, false);
                }
            }
            ".endif" => {
                let result = self.conditionals.end();
                self.misfit(result, site, word);
            }
            _ => {
                let cond = self.condition(command, word, host);
                self.conditionals.open(site, cond);
            }
        }
    }

    /// The condition of an `.if` or one of its kin.
    fn condition(&mut self, command: &Command, word: &str, host: &mut dyn Host) -> bool {
        let value = command.values().next();
        let name = command.name_ref();
        match word {
            ".if" | ".elseif" => value.is_some_and(|expr| self.evaluate(&expr, host)),
            ".ifconst" => value.is_some_and(|expr| host.is_constant(&expr)),
            ".ifnconst" => value.is_some_and(|expr| !host.is_constant(&expr)),
            ".ifdef" => name.is_some_and(|n| host.is_defined(&n)),
            ".ifndef" => name.is_some_and(|n| !host.is_defined(&n)),
            ".ifref" => name.is_some_and(|n| host.is_referenced(&n)),
            ".ifnref" => name.is_some_and(|n| !host.is_referenced(&n)),
            ".ifblank" => command.tokens().is_none(),
            ".ifnblank" => command.tokens().is_some(),
            ".ifp02" => self.mode.cpu() == Cpu::Mos6502,
            // Every other CPU test asks about a CPU this crate does not
            // model yet, so none is the current one.
            _ => false,
        }
    }

    /// Evaluates a condition, reporting the host's problem at the
    /// expression and taking the condition as false.
    fn evaluate(&mut self, expr: &Expr, host: &mut dyn Host) -> bool {
        match host.evaluate(expr) {
            Ok(value) => value != 0,
            Err(message) => {
                self.report(expr.syntax().text_range(), &message);
                false
            }
        }
    }

    /// A line while lines do not run: only its conditional commands count,
    /// wherever they are.
    fn skip(&mut self, line: &Line, index: usize, host: &mut dyn Host) {
        let statement = match line.statement() {
            Some(Statement::Command(command)) => Some(command),
            _ => None,
        };
        let tokens: Vec<SyntaxToken> = line
            .syntax()
            .descendants()
            .filter_map(SyntaxElement::into_token)
            .collect();
        for (at, token) in tokens.iter().enumerate() {
            if token.kind() != kinds::CONTROL || !is_conditional(token.text()) {
                continue;
            }
            let word = token.text().to_ascii_lowercase();
            let site = token.text_range();
            let leads = statement
                .as_ref()
                .and_then(Command::word)
                .is_some_and(|w| w.text_range() == site);
            match word.as_str() {
                ".else" => {
                    let result = self.conditionals.otherwise();
                    self.misfit(result, site, &word);
                }
                ".elseif" => {
                    let result = self.conditionals.otherwise();
                    self.misfit(result, site, &word);
                    if result.is_ok() {
                        let cond = self.conditionals.active()
                            && match statement.as_ref().filter(|_| leads) {
                                Some(command) => self.condition(command, &word, host),
                                None => {
                                    self.report_kind(
                                        DiagnosticKind::Unsupported,
                                        site,
                                        "a `.elseif` after other tokens on its line, whose \
                                         condition counts, is not supported yet",
                                    );
                                    false
                                }
                            };
                        self.conditionals.open_chained(site, cond);
                    }
                }
                ".endif" => {
                    let result = self.conditionals.end();
                    self.misfit(result, site, &word);
                }
                // An `.if` in a skipped branch opens a level that is not
                // taken, whatever its condition.
                _ => self.conditionals.open(site, false),
            }
            if self.conditionals.active() {
                if leads {
                    // The line's own command ran, so its syntax counts.
                    for problem in self.line_problems.get(index).cloned().unwrap_or_default() {
                        self.diagnostics.push((self.text, problem));
                    }
                } else {
                    self.refuse_rest(&tokens[at + 1..]);
                }
                return;
            }
        }
    }

    /// Refuses whatever follows a conditional that ended a skipped branch
    /// partway through its line.
    fn refuse_rest(&mut self, rest: &[SyntaxToken]) {
        let mut tokens = rest.iter().filter(|t| {
            !matches!(
                t.kind(),
                lang198x_syntax::SyntaxKind::WHITESPACE
                    | lang198x_syntax::SyntaxKind::COMMENT
                    | lang198x_syntax::SyntaxKind::NEWLINE
            )
        });
        if let Some(first) = tokens.next() {
            let end = tokens
                .next_back()
                .map_or(first.text_range(), SyntaxToken::text_range);
            self.report(
                first.text_range().cover(end),
                "expected the end of the line",
            );
        }
    }

    fn misfit(&mut self, result: Result<(), Misfit>, site: TextRange, word: &str) {
        match result {
            Ok(()) => {}
            Err(Misfit::Unexpected) => {
                let message = format!("`{word}` with no `.if` open");
                self.report(site, &message);
            }
            Err(Misfit::DuplicateElse) => {
                self.report(site, "a second `.else` for the same `.if`");
            }
        }
    }

    fn report(&mut self, range: TextRange, message: &str) {
        self.report_kind(DiagnosticKind::Invalid, range, message);
    }

    fn report_kind(&mut self, kind: DiagnosticKind, range: TextRange, message: &str) {
        self.diagnostics
            .push((self.text, Diagnostic::new(kind, range, message)));
    }
}
