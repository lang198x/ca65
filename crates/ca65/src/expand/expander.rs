//! Running a program's lines: conditionals, macros, repeats, define-style
//! macros and `.end`.
//!
//! The expander follows ca65 V2.19's own model (`OneLine` in `main.c`,
//! `DoConditionals` in `condasm.c`, and `macro.c`). It reads lines from a
//! stack of sources: the program's file at the bottom, and above it the text
//! of each macro call or repeat being run. It keeps the stack of open
//! conditionals, which is not tied to the sources: a conditional may open in
//! one time round a repeat and close in the next, as in ca65.
//!
//! While lines run, a conditional command at the start of a statement opens,
//! turns or closes a level, asking the host for its condition. `.macro`
//! defines a macro from the lines up to `.endmacro`, and a call runs its text
//! as a new source. `.repeat` takes the lines up to its `.endrepeat` and runs
//! them as a new source, made again each time round. Any other line goes to
//! the host.
//!
//! Every line read has its define-style macros expanded first, whether it
//! runs, is skipped or goes into a body being collected, since ca65 expands
//! them in its scanner.
//!
//! While lines do not run, ca65 skips them token by token, looking only for
//! conditional commands, wherever they are on a line. So `.byte 1 .endif`
//! ends a skipped branch, and anything after the `.endif` on its line is
//! refused. A skipped branch's conditions are never asked for, and a macro
//! defined there is not defined.
//!
//! The lexer's problems in a file count on every line, skipped or not, as
//! ca65's scanner reads every token. Syntax problems count only on lines
//! that run. A conditional opened in a macro stays open after the macro
//! ends, as in ca65, unless `.exitmacro` leaves it.
//!
//! `.exitmacro` inside a repeat inside a macro leaves both at once. ca65
//! V2.19 instead finishes the repeat first, which its manual's "abort a macro
//! expansion immediately" does not allow; that is a bug not copied here.

use std::collections::HashMap;

use lang198x_syntax::{
    Diagnostic, DiagnosticKind, ExpansionBuilder, FileId, Origin, Provenance, Span, SyntaxElement,
    SyntaxKind, SyntaxToken, TextId, TextRange,
};

use super::conditional::{Conditionals, Misfit};
use super::defines::{self, Define};
use super::macros::{self, Macro};
use super::repeats::{self, Repeat};
use crate::ast::{Command, Expr, Line, MacroCall, NameRef, SourceFile, Statement, View};
use crate::{Context, Cpu, Mode, ParseError, Profile, kinds, lex, parse_with};

/// Bounds on how much work expansion may do, so a program that never ends
/// stops with a diagnostic instead.
///
/// ```
/// use lang198x_ca65::expand::Limits;
///
/// let limits = Limits::default();
/// assert_eq!(limits.lines, 10_000_000);
/// assert_eq!(limits.macro_depth, 256);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Limits {
    /// How many lines may be read in all, run or skipped.
    pub lines: usize,
    /// How deeply macro calls may nest. ca65 V2.19 allows 256.
    pub macro_depth: usize,
    /// How many bytes of macro text may be made in all. A macro that passes
    /// its argument on to itself makes text that grows at every level.
    pub expanded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            lines: 10_000_000,
            macro_depth: 256,
            expanded_bytes: 64 * 1024 * 1024,
        }
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
    label_only: bool,
}

impl ActiveLine {
    /// Returns the line.
    #[must_use]
    pub fn line(&self) -> &Line {
        &self.line
    }

    /// Returns the text the line is in: the program's file, or the text of
    /// a macro call. [`Expander::provenance`] traces it back.
    #[must_use]
    pub fn text(&self) -> TextId {
        self.text
    }

    /// Returns where the whole line is, in [`text`](Self::text).
    #[must_use]
    pub fn span(&self) -> Span {
        Span::new(self.text, self.line.syntax().text_range())
    }

    /// Returns whether only the line's labels are for the host: the rest is
    /// a macro call that the expander runs next.
    #[must_use]
    pub fn label_only(&self) -> bool {
        self.label_only
    }
}

/// A text being read: the program's file, or a macro call's text.
#[derive(Debug)]
struct Source {
    text: TextId,
    lines: Vec<Line>,
    /// The syntax problems on each line, reported only if it runs.
    line_problems: Vec<Vec<Diagnostic>>,
    next: usize,
    /// For a macro call, how many conditionals were open when it started,
    /// for `.exitmacro`.
    call_depth: Option<usize>,
    /// For a repeat, the times round still to come.
    repeat: Option<Repeat>,
}

/// The macros defined so far, for the parser's questions.
struct Defined<'m>(&'m HashMap<String, Macro>);

impl Context for Defined<'_> {
    fn is_macro(&mut self, name: &str) -> Option<bool> {
        Some(self.0.contains_key(name))
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
/// let source = ".macro twice op\n op\n op\n.endmacro\n.if 0\n nop\n.else\n twice rts\n.endif\n";
/// let mut expander = Expander::new(source, Profile::CA65_2_19_6502)?;
/// let mut lines = Vec::new();
/// while let Some(line) = expander.next_line(&mut Nothing) {
///     lines.push(line.line().syntax().text().trim().to_owned());
/// }
/// assert_eq!(lines, ["rts", "rts"]);
/// assert!(expander.diagnostics().is_empty());
/// # Ok::<(), lang198x_ca65::ParseError>(())
/// ```
#[derive(Debug)]
pub struct Expander {
    profile: Profile,
    mode: Mode,
    limits: Limits,
    provenance: Provenance,
    sources: Vec<Source>,
    macros: HashMap<String, Macro>,
    defines: HashMap<String, Define>,
    /// The number of the next `.local` name.
    next_local: usize,
    lines_read: usize,
    expanded_bytes: usize,
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
        let mut expander = Self {
            profile,
            mode: Mode::of(profile),
            limits,
            provenance: Provenance::new(),
            sources: Vec::new(),
            macros: HashMap::new(),
            defines: HashMap::new(),
            next_local: 0,
            lines_read: 0,
            expanded_bytes: 0,
            conditionals: Conditionals::default(),
            ended: false,
            finished: false,
            diagnostics: Vec::new(),
        };
        let source = expander.load(TextId::File(FileId::new(0)), text, None, true)?;
        expander.sources.push(source);
        Ok(expander)
    }

    /// Returns the next line to assemble, or `None` when the program is
    /// done. `host` answers the conditions met on the way.
    pub fn next_line(&mut self, host: &mut dyn Host) -> Option<ActiveLine> {
        while !self.ended {
            let Some((line, problems, text)) = self.read() else {
                break;
            };
            let Some((line, problems, text)) = self.expand_defines(line, problems, text) else {
                continue;
            };
            if self.lines_read >= self.limits.lines {
                self.report(
                    text,
                    line.syntax().text_range(),
                    "the program reads too many lines",
                );
                self.ended = true;
                break;
            }
            self.lines_read += 1;
            if !self.conditionals.active() {
                self.skip(&line, problems, text, host);
                continue;
            }
            for problem in problems {
                self.diagnostics.push((text, problem));
            }
            if let Some(active) = self.run(line, text, host) {
                return Some(active);
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

    /// Returns where every macro call's text came from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Parses `text` as a source. A file's lexer problems are reported now,
    /// for every line; a macro call's text is made of tokens already read,
    /// so it has none of its own.
    fn load(
        &mut self,
        id: TextId,
        text: &str,
        call_depth: Option<usize>,
        lexical: bool,
    ) -> Result<Source, ParseError> {
        let parse = parse_with(text, self.profile, &mut Defined(&self.macros))?;
        let lines: Vec<Line> = SourceFile::cast(parse.tree().root())
            .map(|file| file.lines().collect())
            .unwrap_or_default();
        let lexed = lex(text, self.mode);
        let mut line_problems = vec![Vec::new(); lines.len()];
        for diagnostic in parse.diagnostics() {
            if lexed.diagnostics().contains(diagnostic) {
                if lexical {
                    self.diagnostics.push((id, diagnostic.clone()));
                }
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
        Ok(Source {
            text: id,
            lines,
            line_problems,
            next: 0,
            call_depth,
            repeat: None,
        })
    }

    /// The next line from the innermost source, and its syntax problems.
    /// A source that has run out is left.
    fn read(&mut self) -> Option<(Line, Vec<Diagnostic>, TextId)> {
        loop {
            let source = self.sources.last_mut()?;
            if let Some(line) = source.lines.get(source.next).cloned() {
                let problems = source
                    .line_problems
                    .get(source.next)
                    .cloned()
                    .unwrap_or_default();
                source.next += 1;
                return Some((line, problems, source.text));
            }
            if self.sources.len() == 1 {
                return None;
            }
            if let Some(repeat) = self.sources.pop().and_then(|s| s.repeat) {
                self.run_again(repeat);
            }
        }
    }

    /// Reports any conditional left open, once, when the program is done.
    fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        for site in self.conditionals.close_all() {
            self.report(
                site.text(),
                site.range(),
                "this conditional is never closed with `.endif`",
            );
        }
    }

    /// A line while lines run: the expander's own commands run here, and
    /// anything else is the host's.
    fn run(&mut self, line: Line, text: TextId, host: &mut dyn Host) -> Option<ActiveLine> {
        let (line, text) = self.reparse_if_stale(&line, text).unwrap_or((line, text));
        match line.statement() {
            Some(Statement::Command(command)) => {
                let word = command.word().map(|w| w.text().to_ascii_lowercase());
                match word.as_deref() {
                    Some(word) if is_conditional(word) => {
                        self.conditional(&command, word, text, host);
                        return None;
                    }
                    Some(".end") => {
                        self.ended = true;
                        return None;
                    }
                    Some(".macro" | ".mac") => {
                        self.define(&command, text);
                        return None;
                    }
                    Some(".define") => {
                        self.define_style(&command, text);
                        return None;
                    }
                    Some(".undef" | ".undefine") => {
                        if let Some(name) = command.name()
                            && self.defines.remove(&name.text()).is_none()
                        {
                            let message =
                                format!("no define-style macro is named `{}`", name.text());
                            self.report(text, name.syntax().text_range(), &message);
                        }
                        return None;
                    }
                    Some(".delmac" | ".delmacro") => {
                        if let Some(name) = command.name()
                            && self.macros.remove(&name.text()).is_none()
                        {
                            let message = format!("no macro is named `{}`", name.text());
                            self.report(text, name.syntax().text_range(), &message);
                        }
                        return None;
                    }
                    Some(".exitmac" | ".exitmacro") => {
                        self.exit_macro(&command, text);
                        return None;
                    }
                    Some(word) if repeats::is_repeat(word) => {
                        let labelled = line.label().is_some() || line.unnamed_label().is_some();
                        self.repeat(&command, text, host);
                        return labelled.then_some(ActiveLine {
                            line,
                            text,
                            label_only: true,
                        });
                    }
                    Some(word) if repeats::is_end(word) => {
                        let message = format!("`{word}` with no `.repeat` open");
                        self.report(text, command.syntax().text_range(), &message);
                        return None;
                    }
                    _ => {}
                }
            }
            Some(Statement::MacroCall(call)) => {
                let name = call.name().map(|n| n.text().to_owned()).unwrap_or_default();
                if self.macros.contains_key(&name) {
                    let labelled = line.label().is_some() || line.unnamed_label().is_some();
                    self.call(&call, &name, text);
                    return labelled.then_some(ActiveLine {
                        line,
                        text,
                        label_only: true,
                    });
                }
            }
            _ => {}
        }
        Some(ActiveLine {
            line,
            text,
            label_only: false,
        })
    }

    /// A line whose reading depends on macros defined since its text was
    /// parsed is parsed again, on its own, with the macros actually
    /// defined: one defined in a skipped branch is not, and one deleted is
    /// gone.
    fn reparse_if_stale(&mut self, line: &Line, text: TextId) -> Option<(Line, TextId)> {
        let head = macros::head_name(line, |name| self.macros.contains_key(name))?;
        let parsed_as_call = matches!(line.statement(), Some(Statement::MacroCall(_)));
        let is_macro = self.macros.contains_key(&head);
        if parsed_as_call == is_macro {
            return None;
        }
        let mut builder = ExpansionBuilder::new();
        builder.push(
            line.syntax().text(),
            Origin::Source {
                from: Span::new(text, line.syntax().text_range()),
            },
        );
        let (id, source) = self.add_text(builder, None)?;
        let reparsed = source.lines.first().cloned()?;
        for problem in source.line_problems.into_iter().flatten() {
            self.diagnostics.push((id, problem));
        }
        Some((reparsed, id))
    }

    /// Adds an expansion's text to the provenance and parses it.
    fn add_text(
        &mut self,
        builder: ExpansionBuilder,
        call_depth: Option<usize>,
    ) -> Option<(TextId, Source)> {
        let expansion = builder.finish().ok()?;
        let text = expansion.text().to_owned();
        let id = TextId::Expansion(self.provenance.add(expansion).ok()?);
        let source = self.load(id, &text, call_depth, false).ok()?;
        Some((id, source))
    }

    /// `.macro name params`: the lines up to the first `.endmacro` that
    /// starts a line are its body.
    fn define(&mut self, command: &Command, text: TextId) {
        let mut body = Vec::new();
        let mut closed = false;
        while let Some((line, line_text)) = self.take() {
            let ends = matches!(
                macros::command_word(&line).as_deref(),
                Some(".endmacro" | ".endmac")
            );
            if ends {
                closed = true;
                break;
            }
            body.push((line, line_text));
        }
        let site = command.syntax().text_range();
        if !closed {
            self.report(text, site, "this macro has no `.endmacro`");
            return;
        }
        let Some(name) = command.name().map(|n| n.text()) else {
            // The parser has reported the missing name.
            return;
        };
        if self.is_defined(&name) {
            let message = format!("a macro named `{name}` is already defined");
            self.report(text, site, &message);
            return;
        }
        let params = command
            .params()
            .map(|p| p.names().map(|n| n.text().to_owned()).collect())
            .unwrap_or_default();
        self.macros.insert(name, Macro::new(params, body));
    }

    /// Whether a classic or define-style macro is named `name`: the two
    /// share their names, as in ca65.
    fn is_defined(&self, name: &str) -> bool {
        self.macros.contains_key(name) || self.defines.contains_key(name)
    }

    /// The next line of the innermost source, with its defines expanded,
    /// for a body being collected. Nothing on it runs.
    fn take(&mut self) -> Option<(Line, TextId)> {
        loop {
            let source = self.sources.last_mut()?;
            let line = source.lines.get(source.next).cloned()?;
            source.next += 1;
            let text = source.text;
            if let Some((line, _, text)) = self.expand_defines(line, Vec::new(), text) {
                return Some((line, text));
            }
        }
    }

    /// `line`, with every use of a define in force expanded and the result
    /// parsed again, or `None` if a use is broken, which is reported.
    fn expand_defines(
        &mut self,
        line: Line,
        problems: Vec<Diagnostic>,
        text: TextId,
    ) -> Option<(Line, Vec<Diagnostic>, TextId)> {
        let room = self
            .limits
            .expanded_bytes
            .saturating_sub(self.expanded_bytes);
        let pieces = match defines::expand(&self.defines, &line, text, room) {
            Ok(None) => return Some((line, problems, text)),
            Ok(Some(pieces)) => pieces,
            Err(problem) => {
                self.report(problem.site.text(), problem.site.range(), &problem.message);
                if problem.message.contains("too much text") {
                    self.ended = true;
                }
                return None;
            }
        };
        let mut builder = ExpansionBuilder::new();
        for piece in &pieces {
            builder.push(&piece.text, piece.origin);
        }
        self.expanded_bytes += pieces.iter().map(|p| p.text.len()).sum::<usize>();
        let Some((id, source)) = self.add_text(builder, None) else {
            self.report(
                text,
                line.syntax().text_range(),
                "this line's defines could not be expanded",
            );
            return None;
        };
        let expanded = source.lines.into_iter().next()?;
        let problems = source.line_problems.into_iter().next().unwrap_or_default();
        Some((expanded, problems, id))
    }

    /// `.define name[(params)] body`.
    fn define_style(&mut self, command: &Command, text: TextId) {
        let site = command.syntax().text_range();
        match Define::from_command(command, text) {
            Ok(None) => {}
            Ok(Some((name, _))) if self.is_defined(&name) => {
                let message = format!("a macro named `{name}` is already defined");
                self.report(text, site, &message);
            }
            Ok(Some((name, _))) if self.mode.cpu().is_mnemonic(&name) => {
                self.report(text, site, "an instruction cannot be a macro's name");
            }
            Ok(Some((name, define))) => {
                self.defines.insert(name, define);
            }
            Err(message) => self.report(text, site, &message),
        }
    }

    /// A call of a defined macro: its text becomes the next source.
    fn call(&mut self, call: &MacroCall, name: &str, text: TextId) {
        let site = call.syntax().text_range();
        let depth = self
            .sources
            .iter()
            .filter(|s| s.call_depth.is_some())
            .count();
        if depth >= self.limits.macro_depth {
            let message = format!("too many nested macro calls, in `{name}`");
            self.report(text, site, &message);
            self.ended = true;
            return;
        }
        let Some(definition) = self.macros.get(name).cloned() else {
            return;
        };
        let given = call.args().count();
        if given > definition.params.len() {
            let message = format!(
                "`{name}` takes {} arguments, and {given} are given",
                definition.params.len()
            );
            self.report(text, site, &message);
        }
        let first_local = self.next_local;
        self.next_local += definition.local_count();
        let builder = definition.expand(call, text, first_local);
        let Some((_, source)) = self.add_text(builder, Some(self.conditionals.depth())) else {
            self.report(text, site, "this macro call's text could not be made");
            return;
        };
        self.expanded_bytes += source
            .lines
            .last()
            .map_or(0, |l| l.syntax().text_range().end().to_usize());
        if self.expanded_bytes > self.limits.expanded_bytes {
            self.report(text, site, "the program's macros make too much text");
            self.ended = true;
            return;
        }
        self.sources.push(source);
    }

    /// `.repeat count[, counter]`: the lines up to the `.endrepeat` that
    /// balances it are its body, which runs as a new source `count` times.
    fn repeat(&mut self, command: &Command, text: TextId, host: &mut dyn Host) {
        let site = Span::new(text, command.syntax().text_range());
        let mut values = command.values();
        let count = match values.next() {
            Some(expr) => match host.evaluate(&expr) {
                Ok(count) => u64::try_from(count).unwrap_or_else(|_| {
                    self.report(
                        text,
                        expr.syntax().text_range(),
                        "a repeat count must not be negative",
                    );
                    0
                }),
                Err(message) => {
                    self.report(text, expr.syntax().text_range(), &message);
                    0
                }
            },
            None => {
                self.report(text, site.range(), "`.repeat` needs a count");
                0
            }
        };
        let counter = values.next().and_then(|expr| match &expr {
            Expr::Name(name)
                if !name.is_global()
                    && name.parts().count() == 1
                    && name.parts().all(|p| p.kind() == kinds::IDENT) =>
            {
                Some(name.text())
            }
            _ => {
                self.report(
                    text,
                    expr.syntax().text_range(),
                    "a repeat's counter must be a plain name",
                );
                None
            }
        });
        if let Some(extra) = values.next() {
            self.report(
                text,
                extra.syntax().text_range(),
                "`.repeat` takes a count and a counter, no more",
            );
        }
        let mut balance = repeats::Balance::default();
        let mut body = Vec::new();
        let end = loop {
            let Some((line, line_text)) = self.take() else {
                self.report(text, site.range(), "this `.repeat` has no `.endrepeat`");
                return;
            };
            if let Some(end) = balance.ends_on(&line) {
                break (end, line_text);
            }
            body.push((line, line_text));
        };
        let (end, end_text) = end;
        self.refuse_rest(&end.rest, end_text);
        if end.mid_line && count > 0 {
            self.report_kind(
                DiagnosticKind::Unsupported,
                Span::new(end_text, end.token.text_range()),
                "a `.endrepeat` after other tokens on its line is not supported",
            );
            return;
        }
        self.run_again(Repeat::new(body, counter, site, count));
    }

    /// Runs a repeat's body once more, if it has more times to run.
    fn run_again(&mut self, mut repeat: Repeat) {
        let Some(builder) = repeat.next_time() else {
            return;
        };
        let Some((_, mut source)) = self.add_text(builder, None) else {
            return;
        };
        self.expanded_bytes += source
            .lines
            .last()
            .map_or(0, |l| l.syntax().text_range().end().to_usize());
        if self.expanded_bytes > self.limits.expanded_bytes {
            let site = repeat.site();
            self.report(
                site.text(),
                site.range(),
                "the program's repeats make too much text",
            );
            self.ended = true;
            return;
        }
        source.repeat = Some(repeat);
        self.sources.push(source);
    }

    /// `.exitmacro`: leaves the innermost macro call, closing the
    /// conditionals opened inside it.
    fn exit_macro(&mut self, command: &Command, text: TextId) {
        let Some(index) = self.sources.iter().rposition(|s| s.call_depth.is_some()) else {
            self.report(
                text,
                command.syntax().text_range(),
                "`.exitmacro` outside a macro",
            );
            return;
        };
        if let Some(depth) = self.sources[index].call_depth {
            self.conditionals.truncate(depth);
        }
        self.sources.truncate(index);
    }

    /// A conditional command at the start of a statement, while lines run.
    fn conditional(&mut self, command: &Command, word: &str, text: TextId, host: &mut dyn Host) {
        let site = Span::new(text, command.syntax().text_range());
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
                let cond = self.condition(command, word, text, host);
                self.conditionals.open(site, cond);
            }
        }
    }

    /// The condition of an `.if` or one of its kin.
    fn condition(
        &mut self,
        command: &Command,
        word: &str,
        text: TextId,
        host: &mut dyn Host,
    ) -> bool {
        let value = command.values().next();
        let name = command.name_ref();
        match word {
            ".if" | ".elseif" => value.is_some_and(|expr| self.evaluate(&expr, text, host)),
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
    fn evaluate(&mut self, expr: &Expr, text: TextId, host: &mut dyn Host) -> bool {
        match host.evaluate(expr) {
            Ok(value) => value != 0,
            Err(message) => {
                self.report(text, expr.syntax().text_range(), &message);
                false
            }
        }
    }

    /// A line while lines do not run: only its conditional commands count,
    /// wherever they are.
    fn skip(&mut self, line: &Line, problems: Vec<Diagnostic>, text: TextId, host: &mut dyn Host) {
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
            let site = Span::new(text, token.text_range());
            let leads = statement
                .as_ref()
                .and_then(Command::word)
                .is_some_and(|w| w.text_range() == token.text_range());
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
                                Some(command) => self.condition(command, &word, text, host),
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
                    for problem in problems {
                        self.diagnostics.push((text, problem));
                    }
                } else {
                    self.refuse_rest(&tokens[at + 1..], text);
                }
                return;
            }
        }
    }

    /// Refuses whatever follows a conditional that ended a skipped branch
    /// partway through its line.
    fn refuse_rest(&mut self, rest: &[SyntaxToken], text: TextId) {
        let mut tokens = rest.iter().filter(|t| {
            !matches!(
                t.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE
            )
        });
        if let Some(first) = tokens.next() {
            let end = tokens
                .next_back()
                .map_or(first.text_range(), SyntaxToken::text_range);
            self.report(
                text,
                first.text_range().cover(end),
                "expected the end of the line",
            );
        }
    }

    fn misfit(&mut self, result: Result<(), Misfit>, site: Span, word: &str) {
        match result {
            Ok(()) => {}
            Err(Misfit::Unexpected) => {
                let message = format!("`{word}` with no `.if` open");
                self.report(site.text(), site.range(), &message);
            }
            Err(Misfit::DuplicateElse) => {
                self.report(
                    site.text(),
                    site.range(),
                    "a second `.else` for the same `.if`",
                );
            }
        }
    }

    fn report(&mut self, text: TextId, range: TextRange, message: &str) {
        self.report_kind(DiagnosticKind::Invalid, Span::new(text, range), message);
    }

    fn report_kind(&mut self, kind: DiagnosticKind, site: Span, message: &str) {
        self.diagnostics
            .push((site.text(), Diagnostic::new(kind, site.range(), message)));
    }
}
