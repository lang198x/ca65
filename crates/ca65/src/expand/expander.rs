//! Running a program's lines: conditionals, macros, repeats, define-style
//! macros, token functions, includes and `.end`.
//!
//! The expander follows ca65 V2.19's own model (`OneLine` in `main.c`,
//! `DoConditionals` in `condasm.c`, and `macro.c`). It reads lines from a
//! stack of sources: the program's file at the bottom, and above it each
//! file included and the text of each macro call or repeat being run. It keeps the stack of open
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

use std::collections::{HashMap, HashSet};

use lang198x_syntax::{
    Diagnostic, DiagnosticKind, ExpansionId, FileId, Origin, Provenance, Span, SyntaxElement,
    SyntaxKind, SyntaxToken, TextId, TextRange, TextSize,
};

use super::conditional::{Conditionals, Misfit};
use super::defines::{self, Define, Piece, Problem};
use super::functions;
use super::locate::Reading;
use super::macros::{self, Macro};
use super::made::{Made, MadeStrings, Strings};
use super::repeats::{self, Repeat};
use crate::ast::{Command, Expr, Line, MacroCall, NameRef, SourceFile, Statement, View};
use crate::parser::commands::{Shape, shape};
use crate::parser::{Block, parse_in, parse_prepared};
use crate::{Context, Cpu, Mode, ParseError, Profile, Token, Widths, kinds, lex};

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
    /// How deeply `.include` and `.macpack` may nest. ca65 V2.19 stops only
    /// when it runs out of open files; a file that includes itself needs a
    /// bound.
    pub include_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            lines: 10_000_000,
            macro_depth: 256,
            expanded_bytes: 64 * 1024 * 1024,
            include_depth: 64,
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

    /// The text of the file `.include` names, or `None` if there is no such
    /// file. `from` is the file being read, since ca65 looks beside it
    /// first. The default has no files.
    fn include(&mut self, name: &str, from: FileId) -> Option<String> {
        let _ = (name, from);
        None
    }

    /// The text of the macro package `.macpack` names, such as `generic`,
    /// or `None` if there is no such package. ca65 V2.19 reads `name.mac`
    /// from its include path. The default has no packages.
    fn package(&mut self, name: &str) -> Option<String> {
        let _ = name;
        None
    }

    /// Told where the line the expander is about to act on is, before it
    /// asks anything about it, so an answer that finds a problem can say
    /// where. The default does nothing.
    fn reading(&mut self, reading: &Reading) {
        let _ = reading;
    }
}

/// A file `.include` or `.macpack` read in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludedFile {
    id: FileId,
    name: String,
    site: Span,
}

impl IncludedFile {
    /// Returns the file's ID. The root file is 0, and each file read in
    /// takes the next, in the order it was read.
    #[must_use]
    pub fn id(&self) -> FileId {
        self.id
    }

    /// Returns the name the command gave: a file name, or a package name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the command that read the file in.
    #[must_use]
    pub fn site(&self) -> Span {
        self.site
    }
}

/// A line for the host to assemble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLine {
    line: Line,
    text: TextId,
    label_only: bool,
    /// The made strings on the line whose text cannot hold their bytes.
    strings: MadeStrings,
    /// The CPU in force where the line was read.
    cpu: Cpu,
    /// The 65816's register widths where the line was read.
    widths: Widths,
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

    /// Returns the CPU in force where the line was read, which `.setcpu`
    /// and its kin choose. The line is lowered for it.
    #[must_use]
    pub fn cpu(&self) -> Cpu {
        self.cpu
    }

    /// Returns the 65816's register widths where the line was read, before
    /// the line itself changes them.
    #[must_use]
    pub fn widths(&self) -> Widths {
        self.widths
    }

    /// Returns whether only the line's labels are for the host: the rest is
    /// a macro call that the expander runs next.
    #[must_use]
    pub fn label_only(&self) -> bool {
        self.label_only
    }

    /// Returns the bytes a string token on the line holds, without its
    /// quotes, or `None` if `token` is not a string. A string a token
    /// function made may hold bytes its text cannot, such as `$FF`; those
    /// are kept beside the text and given here.
    #[must_use]
    pub fn string(&self, token: &SyntaxToken) -> Option<Vec<u8>> {
        if token.kind() != kinds::STRING {
            return None;
        }
        let kept = self
            .strings
            .iter()
            .find(|(at, _)| *at == token.text_range())
            .map(|(_, bytes)| bytes.clone());
        Some(kept.unwrap_or_else(|| defines::unquoted(token.text()).as_bytes().to_vec()))
    }
}

/// A text being read: the program's file, or a macro call's text.
#[derive(Debug)]
struct Source {
    text: TextId,
    lines: Vec<Line>,
    /// The syntax problems on each line, reported only if it runs.
    line_problems: Vec<Vec<Diagnostic>>,
    /// The problems the lexer found on each line, reported when the line
    /// is read, whether it runs or not. ca65 scans a line's tokens as it
    /// reads them, so nothing after `.end` is scanned.
    lexical: Vec<Vec<Diagnostic>>,
    next: usize,
    /// For a macro call, how many conditionals were open when it started,
    /// for `.exitmacro`.
    call_depth: Option<usize>,
    /// For a repeat, the times round still to come.
    repeat: Option<Repeat>,
    /// The CPU its lines were parsed for. A line read after `.setcpu`
    /// chose another is lexed and parsed again, since the CPU decides
    /// which names are registers and which are instructions.
    cpu: Cpu,
}

/// `line` for the host if it has a label, which is all of it the host
/// gets: the expander has run the rest.
fn labelled(line: Line, text: TextId) -> Option<ActiveLine> {
    (line.label().is_some() || line.unnamed_label().is_some()).then_some(ActiveLine {
        line,
        text,
        label_only: true,
        strings: Vec::new(),
        cpu: Cpu::Mos6502,
        widths: Widths::default(),
    })
}

/// Whether `line` ends with a line break, rather than running on into
/// the next.
fn ends_line(line: &Line) -> bool {
    line.syntax()
        .descendants()
        .filter_map(SyntaxElement::into_token)
        .last()
        .is_some_and(|t| t.kind() == SyntaxKind::NEWLINE)
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
    /// The CPUs `.pushcpu` kept, last on top.
    cpus: Vec<Cpu>,
    /// The 65816's register widths.
    widths: Widths,
    /// Whether `.smart` is on, so `rep` and `sep` change the widths.
    smart: bool,
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
    files: Vec<IncludedFile>,
    /// Where each line starts in each file, by file number, for
    /// [`locate`](Expander::locate).
    pub(super) line_starts: Vec<Vec<TextSize>>,
    /// The texts macro calls made, for
    /// [`macro_call`](Expander::macro_call).
    pub(super) macro_texts: HashSet<ExpansionId>,
    /// The texts a repeat's passes made, whose lines are reported where
    /// the body was written.
    pub(super) repeat_texts: HashSet<ExpansionId>,
    /// For each macro call's text, the macro's name and where its
    /// `.macro` line is.
    pub(super) macro_calls: HashMap<ExpansionId, (String, Span)>,
    /// Where each macro's `.macro` line is, by name.
    macro_sites: HashMap<String, Span>,
    /// The blocks open at the line being read, from the lines that ran, so
    /// text parsed on its own reads as it does where it goes: a macro's
    /// lines inside a `.struct` are members.
    blocks: Vec<Block>,
    /// How many problems had been found when the line being run was read.
    line_start: usize,
    /// A conditional whose line's label the host has been given, and which
    /// acts when the host asks for the next line.
    deferred: Option<(Command, String, TextId)>,
    ended: bool,
    finished: bool,
    diagnostics: Vec<(TextId, Diagnostic)>,
    /// A repeat round's unfinished last line, joined to the next line read.
    carry: Option<(Line, TextId)>,
    /// The bytes of each made string whose text cannot hold them.
    strings: Strings,
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
            cpus: Vec::new(),
            widths: Widths::default(),
            smart: false,
            limits,
            provenance: Provenance::new(),
            sources: Vec::new(),
            macros: HashMap::new(),
            defines: HashMap::new(),
            next_local: 0,
            lines_read: 0,
            expanded_bytes: 0,
            conditionals: Conditionals::default(),
            files: Vec::new(),
            line_starts: vec![super::locate::line_starts(text)],
            macro_texts: HashSet::new(),
            repeat_texts: HashSet::new(),
            macro_calls: HashMap::new(),
            macro_sites: HashMap::new(),
            blocks: Vec::new(),
            line_start: 0,
            deferred: None,
            ended: false,
            finished: false,
            diagnostics: Vec::new(),
            carry: None,
            strings: Strings::default(),
        };
        let source = expander.load(TextId::File(FileId::new(0)), text, None, None, true)?;
        expander.sources.push(source);
        Ok(expander)
    }

    /// Returns the next line to assemble, or `None` when the program is
    /// done. `host` answers the conditions met on the way.
    pub fn next_line(&mut self, host: &mut dyn Host) -> Option<ActiveLine> {
        if let Some((command, word, text)) = self.deferred.take() {
            host.reading(&self.reading(Span::new(text, command.syntax().text_range())));
            self.conditional(&command, &word, text, host);
        }
        while !self.ended {
            self.line_start = self.diagnostics.len();
            let Some((line, problems, text)) = self.read() else {
                break;
            };
            let Some((line, problems, text)) = self.expand_defines(line, problems, text, false)
            else {
                continue;
            };
            host.reading(&self.reading(Span::new(text, line.syntax().text_range())));
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
            let Some((line, problems, text)) = self.run_functions(line, problems, text, host)
            else {
                continue;
            };
            let (line, problems, text) = self.resolve(line, problems, text);
            for problem in problems {
                self.diagnostics.push((text, problem));
            }
            if let Some(mut active) = self.run(line, text, host) {
                active.cpu = self.mode.cpu();
                active.widths = self.widths;
                if !active.label_only {
                    self.track_block(&active.line);
                    self.follow_widths(&active.line, host);
                }
                let range = active.line.syntax().text_range();
                active.strings = self
                    .strings
                    .in_text(text)
                    .into_iter()
                    .filter(|(at, _)| range.contains_range(*at))
                    .collect();
                return Some(active);
            }
        }
        self.finish();
        None
    }

    /// Follows the 65816's register widths, which `line` may change for
    /// the lines after it, as ca65 V2.19 does: `.a8`, `.a16`, `.i8` and
    /// `.i16` on the 65816, and there under `.smart` a `rep` or `sep` whose
    /// value is known now. Bit 5 of the value is the accumulator's and bit 4
    /// the index registers'. A value not known yet leaves the widths as they
    /// are; ca65 warns of that.
    fn follow_widths(&mut self, line: &Line, host: &mut dyn Host) {
        let on_65816 = self.mode.cpu() == Cpu::Wdc65816;
        match line.statement() {
            Some(Statement::Command(command)) => {
                let Some(word) = command.word() else {
                    return;
                };
                let widths = self.widths;
                match word.text().to_ascii_lowercase().as_str() {
                    ".a8" if on_65816 => self.widths = widths.with_accumulator(false),
                    ".a16" if on_65816 => self.widths = widths.with_accumulator(true),
                    ".i8" if on_65816 => self.widths = widths.with_index(false),
                    ".i16" if on_65816 => self.widths = widths.with_index(true),
                    ".smart" => match command.switch() {
                        None => self.smart = true,
                        Some(token) => match token.kind() {
                            kinds::PLUS => self.smart = true,
                            kinds::MINUS => self.smart = false,
                            _ if token.text().eq_ignore_ascii_case("on") => self.smart = true,
                            _ if token.text().eq_ignore_ascii_case("off") => self.smart = false,
                            _ => {}
                        },
                    },
                    _ => {}
                }
            }
            Some(Statement::Instruction(instruction)) if on_65816 && self.smart => {
                let Some(mnemonic) = instruction.mnemonic() else {
                    return;
                };
                let sixteen = match mnemonic.text().to_ascii_lowercase().as_str() {
                    "rep" => true,
                    "sep" => false,
                    _ => return,
                };
                let Some(value) = instruction
                    .operands()
                    .next()
                    .and_then(|operand| operand.immediate())
                    .and_then(|immediate| immediate.expr())
                else {
                    return;
                };
                let Ok(value) = host.evaluate(&value) else {
                    return;
                };
                if value & 0x20 != 0 {
                    self.widths = self.widths.with_accumulator(sixteen);
                }
                if value & 0x10 != 0 {
                    self.widths = self.widths.with_index(sixteen);
                }
            }
            _ => {}
        }
    }

    /// Follows the blocks a line that runs opens or closes: `.struct`,
    /// `.union`, `.enum`, `.proc` and `.scope`, and their ends.
    fn track_block(&mut self, line: &Line) {
        let command = match line.statement() {
            Some(Statement::Command(command)) => Some(command),
            Some(Statement::Member(member)) => member.storage(),
            _ => None,
        };
        let Some(word) = command.and_then(|c| c.word()) else {
            return;
        };
        match shape(word.text()) {
            Some(Shape::Block(block)) => self.blocks.push(block),
            Some(Shape::EndBlock) => {
                self.blocks.pop();
            }
            _ => {}
        }
    }

    /// Returns the problems found so far, each with the text it is in.
    #[must_use]
    pub fn diagnostics(&self) -> &[(TextId, Diagnostic)] {
        &self.diagnostics
    }

    /// Returns the files `.include` and `.macpack` have read in so far.
    #[must_use]
    pub fn files(&self) -> &[IncludedFile] {
        &self.files
    }

    /// Returns where every macro call's text came from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Parses `text` as a source. A file's lexer problems are reported now,
    /// for every line; a macro call's text is made of tokens already read,
    /// so it has none of its own.
    /// Parses `text`, from `tokens` if it is an expansion, which are not
    /// lexed again. `lexical` reports the lexer's problems now, for a file.
    fn load(
        &mut self,
        id: TextId,
        text: &str,
        tokens: Option<Vec<Token>>,
        call_depth: Option<usize>,
        lexical: bool,
    ) -> Result<Source, ParseError> {
        let parse = match tokens {
            Some(tokens) => parse_prepared(
                text,
                tokens,
                self.profile,
                &mut Defined(&self.macros),
                &self.blocks,
            )?,
            None => parse_in(text, self.profile, &mut Defined(&self.macros), &self.blocks)?,
        };
        let lines: Vec<Line> = SourceFile::cast(parse.tree().root())
            .map(|file| file.lines().collect())
            .unwrap_or_default();
        let lexed = if lexical {
            lex(text, self.mode).diagnostics().to_vec()
        } else {
            Vec::new()
        };
        let mut line_problems = vec![Vec::new(); lines.len()];
        let mut lexical_problems = vec![Vec::new(); lines.len()];
        for diagnostic in parse.diagnostics() {
            let start = diagnostic.range().start();
            let index = lines
                .partition_point(|line| line.syntax().text_range().end() <= start)
                .min(lines.len().saturating_sub(1));
            let found = if lexed.contains(diagnostic) {
                if !lexical {
                    continue;
                }
                &mut lexical_problems
            } else {
                &mut line_problems
            };
            match found.get_mut(index) {
                Some(problems) => problems.push(diagnostic.clone()),
                None => self.diagnostics.push((id, diagnostic.clone())),
            }
        }
        Ok(Source {
            text: id,
            lines,
            line_problems,
            lexical: lexical_problems,
            cpu: self.mode.cpu(),
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
                if let Some(found) = source.lexical.get(source.next) {
                    let text = source.text;
                    self.diagnostics
                        .extend(found.iter().map(|problem| (text, problem.clone())));
                }
                source.next += 1;
                let unfinished = source.repeat.is_some()
                    && source.next == source.lines.len()
                    && !ends_line(&line);
                // A file is read as ca65 scans it, for the CPU in force; the
                // text of a macro or repeat keeps the tokens it was captured
                // with, as ca65's does.
                let stale = source.cpu != self.mode.cpu() && matches!(source.text, TextId::File(_));
                let mut read = (line, problems, source.text);
                if stale {
                    read = self.reread(&read.0, read.2).unwrap_or(read);
                }
                if let Some((carried, carried_text)) = self.carry.take() {
                    read = self.join(&carried, carried_text, &read.0, read.2)?;
                }
                if unfinished {
                    self.carry = Some((read.0, read.2));
                    continue;
                }
                return Some(read);
            }
            if self.sources.len() == 1 {
                let (line, text) = self.carry.take()?;
                return Some((line, Vec::new(), text));
            }
            self.leave();
        }
    }

    /// `line`, from `text`, lexed and parsed again for the current CPU.
    fn reread(&mut self, line: &Line, text: TextId) -> Option<(Line, Vec<Diagnostic>, TextId)> {
        let mut builder = Made::new();
        for token in line
            .syntax()
            .descendants()
            .filter_map(SyntaxElement::into_token)
        {
            let from = Span::new(text, token.text_range());
            builder.token(&token, from, self.copied(from), &self.strings);
        }
        let (builder, _, strings) = builder.finish();
        let expansion = builder.finish().ok()?;
        let made = expansion.text().to_owned();
        let id = TextId::Expansion(self.provenance.add(expansion).ok()?);
        self.strings.add(id, strings);
        let source = self.load(id, &made, None, None, false).ok()?;
        let reparsed = source.lines.first().cloned()?;
        let problems = source.line_problems.into_iter().flatten().collect();
        Some((reparsed, problems, id))
    }

    /// The CPU commands, which change how the lines after them are read:
    /// `.setcpu`, each CPU's `.P*` command, `.pushcpu` and `.popcpu`. A name
    /// `.setcpu` does not know, and a `.popcpu` with nothing to pop, are left
    /// for lowering to report.
    fn choose_cpu(&mut self, command: &Command, word: &str, text: TextId) {
        let cpu = match word {
            word if Cpu::chosen_by(word).is_some() => Cpu::chosen_by(word),
            ".setcpu" => command.values().next().and_then(|value| {
                let Expr::Literal(literal) = value else {
                    return None;
                };
                let token = literal.token().filter(|t| t.kind() == kinds::STRING)?;
                let name = match self.strings.get(Span::new(text, token.text_range())) {
                    Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                    None => token.text().trim_matches('"').to_owned(),
                };
                Cpu::named(&name)
            }),
            ".pushcpu" => {
                self.cpus.push(self.mode.cpu());
                None
            }
            ".popcpu" => self.cpus.pop(),
            _ => None,
        };
        if let Some(cpu) = cpu {
            self.profile = self.profile.with_cpu(cpu);
            self.mode = Mode::of(self.profile);
        }
    }

    /// A repeat round's unfinished last line joined to the next line read,
    /// as ca65 reads tokens rather than lines, and parsed again.
    fn join(
        &mut self,
        carried: &Line,
        carried_text: TextId,
        next: &Line,
        next_text: TextId,
    ) -> Option<(Line, Vec<Diagnostic>, TextId)> {
        let mut builder = Made::new();
        for (line, text) in [(carried, carried_text), (next, next_text)] {
            for token in line
                .syntax()
                .descendants()
                .filter_map(SyntaxElement::into_token)
            {
                let from = Span::new(text, token.text_range());
                builder.token(&token, from, self.copied(from), &self.strings);
            }
        }
        let (id, source) = self.add_text(builder, None)?;
        let line = source.lines.into_iter().next()?;
        let problems = source.line_problems.into_iter().next().unwrap_or_default();
        Some((line, problems, id))
    }

    /// Leaves the innermost source, which has run out. A repeat runs again;
    /// leaving an included file closes the conditionals opened in it, as ca65
    /// does, each with a diagnostic.
    fn leave(&mut self) {
        let Some(source) = self.sources.pop() else {
            return;
        };
        if let Some(repeat) = source.repeat {
            self.run_again(repeat);
        }
        if matches!(source.text, TextId::File(_)) {
            for site in self.conditionals.close_opened_in(source.text) {
                self.report(
                    site.text(),
                    site.range(),
                    "this conditional is never closed with `.endif` in its file",
                );
            }
        }
    }

    /// The `.include` or `.macpack` that read in `text`, if it is a file
    /// read in.
    fn include_site(&self, text: TextId) -> Option<Span> {
        self.files
            .iter()
            .find(|f| TextId::File(f.id) == text)
            .map(IncludedFile::site)
    }

    /// How a piece copied unchanged from `from` came there.
    fn copied(&self, from: Span) -> Origin {
        match self.include_site(from.text()) {
            Some(site) => Origin::Included { from, site },
            None => Origin::Source { from },
        }
    }

    /// The file being read: the innermost file on the stack of sources.
    fn current_file(&self) -> FileId {
        self.sources
            .iter()
            .rev()
            .find_map(|s| match s.text {
                TextId::File(id) => Some(id),
                TextId::Expansion(_) => None,
            })
            .unwrap_or(FileId::new(0))
    }

    /// `.include "name"` or `.macpack name`: the file is read in as the next
    /// source.
    fn include(&mut self, command: &Command, word: &str, text: TextId, host: &mut dyn Host) {
        let site = Span::new(text, command.syntax().text_range());
        let mut values = command.values();
        let value = values.next();
        if let Some(extra) = values.next() {
            self.report(
                text,
                extra.syntax().text_range(),
                "expected the end of the line",
            );
            return;
        }
        if value.is_none() {
            let message = if word == ".include" {
                "`.include` needs a file name in quotes"
            } else {
                "`.macpack` needs a package's name"
            };
            self.report(text, site.range(), message);
            return;
        }
        let name = match (&value, word) {
            // A made string's bytes are the name, as ca65 reads them.
            (Some(Expr::Literal(literal)), ".include") => literal
                .token()
                .filter(|t| t.kind() == kinds::STRING)
                .map(
                    |t| match self.strings.get(Span::new(text, t.text_range())) {
                        Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                        None => {
                            let text = t.text().strip_prefix('"').unwrap_or(t.text());
                            text.strip_suffix('"').unwrap_or(text).to_owned()
                        }
                    },
                ),
            (Some(Expr::Name(name)), ".macpack")
                if !name.is_global() && name.parts().count() == 1 =>
            {
                Some(name.text())
            }
            _ => None,
        };
        let Some(name) = name else {
            let message = if word == ".include" {
                "`.include` needs a file name in quotes"
            } else {
                "`.macpack` needs a package's name"
            };
            self.report(text, site.range(), message);
            return;
        };
        let depth = self
            .sources
            .iter()
            .skip(1)
            .filter(|s| matches!(s.text, TextId::File(_)))
            .count();
        if depth >= self.limits.include_depth {
            let message =
                format!("includes nest too deeply at `{name}`; does a file include itself?");
            self.report(text, site.range(), &message);
            return;
        }
        let from = self.current_file();
        let loaded = if word == ".include" {
            host.include(&name, from)
        } else {
            host.package(&name)
        };
        let Some(loaded) = loaded else {
            let message = if word == ".include" {
                format!("there is no file `{name}` to include")
            } else {
                format!("there is no macro package `{name}`")
            };
            // The problem is the name, so it is reported there.
            let at = value
                .as_ref()
                .map_or(site.range(), |value| value.syntax().text_range());
            self.report(text, at, &message);
            return;
        };
        let Ok(number) = u32::try_from(self.files.len() + 1) else {
            return;
        };
        let id = FileId::new(number);
        self.files.push(IncludedFile { id, name, site });
        self.line_starts.push(super::locate::line_starts(&loaded));
        match self.load(TextId::File(id), &loaded, None, None, true) {
            Ok(source) => self.sources.push(source),
            Err(_) => self.report(text, site.range(), "the included file is too long to read"),
        }
    }

    /// `.end`: ca65 reads nothing more, so a problem the lexer found further
    /// on, in this text or any text under it, is not one.
    fn end(&mut self, command: &Command, text: TextId) {
        self.ended = true;
        let at = command.word().map_or_else(
            || command.syntax().text_range().end(),
            |w| w.text_range().end(),
        );
        let mut cuts: Vec<(TextId, TextSize)> = vec![(text, at)];
        for source in &self.sources {
            if let Some(line) = source.lines.get(source.next)
                && source.text != text
            {
                cuts.push((source.text, line.syntax().text_range().start()));
            }
        }
        self.diagnostics.retain(|(text, problem)| {
            !cuts
                .iter()
                .any(|(cut, at)| cut == text && problem.range().start() >= *at)
        });
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
        match line.statement() {
            Some(Statement::Command(command)) => {
                let word = command.word().map(|w| w.text().to_ascii_lowercase());
                if let Some(word) = word.as_deref() {
                    self.choose_cpu(&command, word, text);
                }
                match word.as_deref() {
                    Some(word) if is_conditional(word) => {
                        // ca65 defines a label on a conditional's line when
                        // the line is read while lines run, before the
                        // conditional acts. So the host has the label first,
                        // and the conditional acts when it asks again.
                        let label = labelled(line, text);
                        if label.is_some() {
                            self.deferred = Some((command, word.to_owned(), text));
                        } else {
                            self.conditional(&command, word, text, host);
                        }
                        return label;
                    }
                    // ca65 defines a label on these lines too.
                    Some(".end") => {
                        self.end(&command, text);
                        return labelled(line, text);
                    }
                    Some(".macro" | ".mac") => {
                        self.define(&command, text);
                        return labelled(line, text);
                    }
                    Some(".define") => {
                        self.define_style(&command, text);
                        return None;
                    }
                    Some(word @ (".include" | ".macpack")) => {
                        self.include(&command, word, text, host);
                        return labelled(line, text);
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
                            strings: Vec::new(),
                            cpu: Cpu::Mos6502,
                            widths: Widths::default(),
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
                        strings: Vec::new(),
                        cpu: Cpu::Mos6502,
                        widths: Widths::default(),
                    });
                }
            }
            _ => {}
        }
        Some(ActiveLine {
            line,
            text,
            label_only: false,
            strings: Vec::new(),
            cpu: Cpu::Mos6502,
            widths: Widths::default(),
        })
    }

    /// A line whose reading depends on macros defined since its text was
    /// parsed is parsed again, on its own, with the macros actually
    /// defined: one defined in a skipped branch is not, and one deleted is
    /// gone.
    ///
    /// So is a line the parser could only guess at, after an `.include`: by
    /// now the expander knows which macros exist.
    fn resolve(
        &mut self,
        line: Line,
        problems: Vec<Diagnostic>,
        text: TextId,
    ) -> (Line, Vec<Diagnostic>, TextId) {
        let guessed = problems.iter().any(|p| p.kind() == DiagnosticKind::Unknown);
        let stale =
            macros::head_name(&line, |name| self.macros.contains_key(name)).is_some_and(|head| {
                let parsed_as_call = matches!(line.statement(), Some(Statement::MacroCall(_)));
                parsed_as_call != self.macros.contains_key(&head)
            });
        // A line the parser took for a macro's body, after a `.macro` that a
        // skipped branch never opened, is an ordinary line.
        let orphaned = matches!(line.statement(), Some(Statement::MacroBody(_)));
        if !guessed && !stale && !orphaned {
            return (line, problems, text);
        }
        let mut builder = Made::new();
        for token in line
            .syntax()
            .descendants()
            .filter_map(SyntaxElement::into_token)
        {
            let from = Span::new(text, token.text_range());
            builder.token(&token, from, self.copied(from), &self.strings);
        }
        let Some((id, source)) = self.add_text(builder, None) else {
            return (line, problems, text);
        };
        let Some(reparsed) = source.lines.first().cloned() else {
            return (line, problems, text);
        };
        let problems = source.line_problems.into_iter().flatten().collect();
        (reparsed, problems, id)
    }

    /// Adds an expansion's text to the provenance and parses it from the
    /// tokens it was made of.
    fn add_text(&mut self, made: Made, call_depth: Option<usize>) -> Option<(TextId, Source)> {
        let (builder, tokens, strings) = made.finish();
        let expansion = builder.finish().ok()?;
        let text = expansion.text().to_owned();
        let expansion = self.provenance.add(expansion).ok()?;
        // Only a macro call's text has a call depth.
        if call_depth.is_some() {
            self.macro_texts.insert(expansion);
        }
        let id = TextId::Expansion(expansion);
        self.strings.add(id, strings);
        let source = self.load(id, &text, Some(tokens), call_depth, false).ok()?;
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
        self.macro_sites
            .insert(name.clone(), Span::new(text, command.syntax().text_range()));
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
            let Some(line) = source.lines.get(source.next).cloned() else {
                // A body may run on past the end of an included file, as
                // ca65 reads tokens, but not past a macro's or repeat's text.
                let included = self.sources.len() > 1
                    && matches!(self.sources.last().map(|s| s.text), Some(TextId::File(_)));
                if !included {
                    return None;
                }
                self.leave();
                continue;
            };
            let text = source.text;
            if let Some(found) = source.lexical.get(source.next) {
                self.diagnostics
                    .extend(found.iter().map(|problem| (text, problem.clone())));
            }
            let source = self.sources.last_mut()?;
            source.next += 1;
            // A body is captured as tokens for the CPU in force now.
            let stale = source.cpu != self.mode.cpu() && matches!(text, TextId::File(_));
            let (line, text) = match stale.then(|| self.reread(&line, text)).flatten() {
                Some((line, _, text)) => (line, text),
                None => (line, text),
            };
            if let Some((line, _, text)) = self.expand_defines(line, Vec::new(), text, true) {
                return Some((line, text));
            }
        }
    }

    /// `line`, with every use of a define in force expanded and the result
    /// parsed again, or `None` if a use is broken, which is reported.
    /// `collecting` says the line goes into a body being collected, where a
    /// define's own name is expanded too.
    fn expand_defines(
        &mut self,
        line: Line,
        problems: Vec<Diagnostic>,
        text: TextId,
        collecting: bool,
    ) -> Option<(Line, Vec<Diagnostic>, TextId)> {
        let room = self
            .limits
            .expanded_bytes
            .saturating_sub(self.expanded_bytes);
        let site = self.include_site(text);
        let expanded = defines::expand(
            &self.defines,
            &line,
            text,
            site,
            room,
            &self.strings,
            collecting,
        );
        let pieces = match expanded {
            Ok(None) => return Some((line, problems, text)),
            Ok(Some(pieces)) => pieces,
            Err(problem) => {
                self.report_kind(problem.kind, problem.site, &problem.message);
                if problem.message.contains("too much text") {
                    self.ended = true;
                }
                return None;
            }
        };
        let mut builder = Made::new();
        for piece in &pieces {
            builder.piece(piece);
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

    /// `line`, with its token functions run and the result parsed again, or
    /// `None` if a call is broken, which is reported. A `.define`'s body is
    /// kept as written, as ca65 reads it in raw mode.
    fn run_functions(
        &mut self,
        line: Line,
        problems: Vec<Diagnostic>,
        text: TextId,
        host: &mut dyn Host,
    ) -> Option<(Line, Vec<Diagnostic>, TextId)> {
        if macros::command_word(&line).as_deref() == Some(".define") {
            return Some((line, problems, text));
        }
        let mut pieces: Vec<Piece> = line
            .syntax()
            .descendants()
            .filter_map(SyntaxElement::into_token)
            .map(|t| {
                let from = Span::new(text, t.text_range());
                self.strings.piece(&t, self.copied(from), from)
            })
            .collect();
        // ca65 reads no token after `.end`, so runs no function there.
        if let Some(end) = pieces.iter().position(|piece| {
            piece.kind != kinds::STRING && piece.text.eq_ignore_ascii_case(".end")
        }) {
            pieces.truncate(end + 1);
            // Nor does it scan them, so the problems found on this line
            // there go too. A line a define made traces back to the file it
            // was read from.
            let site = pieces[end].site;
            let at = self.provenance.trace(site).unwrap_or(site);
            let mine = self
                .diagnostics
                .split_off(self.line_start.min(self.diagnostics.len()));
            self.diagnostics
                .extend(mine.into_iter().filter(|(text, problem)| {
                    *text != at.text() || problem.range().start() < at.range().start()
                }));
        }
        if !functions::any(&pieces) {
            return Some((line, problems, text));
        }
        let result = functions::run(
            pieces,
            &mut Asked {
                expander: self,
                host,
            },
        );
        let pieces = match result {
            Ok(pieces) => pieces,
            Err(problem) => {
                self.report_kind(problem.kind, problem.site, &problem.message);
                return None;
            }
        };
        let mut builder = Made::new();
        for piece in &pieces {
            builder.piece(piece);
        }
        let Some((id, source)) = self.add_text(builder, None) else {
            self.report(
                text,
                line.syntax().text_range(),
                "this line's token functions could not be run",
            );
            return None;
        };
        let line = source.lines.into_iter().next()?;
        let problems = source.line_problems.into_iter().next().unwrap_or_default();
        Some((line, problems, id))
    }

    /// The value of the constant expression made of `pieces`, for a token
    /// function called at `site`, asked of the host.
    fn constant(
        &mut self,
        pieces: &[Piece],
        site: Span,
        host: &mut dyn Host,
    ) -> Result<i64, Problem> {
        let made = Origin::Synthesised {
            inputs: site,
            call_site: site,
        };
        let mut builder = Made::new();
        builder.push(".byte", kinds::CONTROL, made);
        builder.push(" ", SyntaxKind::WHITESPACE, made);
        for piece in pieces {
            builder.piece(piece);
        }
        builder.push("\n", SyntaxKind::NEWLINE, made);
        let expected = || Problem::invalid(site, "expected a constant value".to_owned());
        let (_, source) = self.add_text(builder, None).ok_or_else(expected)?;
        let line = source.lines.first().ok_or_else(expected)?;
        let clean = source.line_problems.iter().all(Vec::is_empty);
        let values: Vec<Expr> = match line.statement() {
            Some(Statement::Command(command)) if clean => command.values().collect(),
            _ => Vec::new(),
        };
        match values.as_slice() {
            [value] => host
                .evaluate(value)
                .map_err(|message| Problem::invalid(site, message)),
            _ => Err(expected()),
        }
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
        let builder = definition.expand(call, text, first_local, &self.strings);
        let Some((id, source)) = self.add_text(builder, Some(self.conditionals.depth())) else {
            self.report(text, site, "this macro call's text could not be made");
            return;
        };
        if let (TextId::Expansion(id), Some(defined)) = (id, self.macro_sites.get(name)) {
            self.macro_calls.insert(id, (name.to_owned(), *defined));
        }
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
        let tail = end.before.into_iter().map(|t| (t, end_text)).collect();
        self.run_again(Repeat::new(body, tail, counter, site, count));
    }

    /// Runs a repeat's body once more, if it has more times to run.
    fn run_again(&mut self, mut repeat: Repeat) {
        let Some(builder) = repeat.next_time(&self.strings) else {
            return;
        };
        let Some((id, mut source)) = self.add_text(builder, None) else {
            return;
        };
        if let TextId::Expansion(id) = id {
            self.repeat_texts.insert(id);
        }
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
            // `.ifp02`, `.ifp816` and the rest ask about the CPU in force.
            word => Cpu::asked_by(word).is_some_and(|cpu| cpu == self.mode.cpu()),
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

/// The expander and its host, answering a line's token functions.
struct Asked<'a> {
    expander: &'a mut Expander,
    host: &'a mut dyn Host,
}

impl functions::Answers for Asked<'_> {
    fn value(&mut self, pieces: &[Piece], site: Span) -> Result<i64, Problem> {
        self.expander.constant(pieces, site, self.host)
    }

    fn is_macro(&self, name: &str) -> bool {
        self.expander.macros.contains_key(name)
    }

    fn is_mnemonic(&self, name: &str) -> bool {
        self.expander.mode.cpu().is_mnemonic(name)
    }
}
