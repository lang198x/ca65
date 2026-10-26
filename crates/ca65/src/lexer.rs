//! Splitting source text into tokens.
//!
//! The lexer gives every byte of the text to exactly one token. Nothing is
//! skipped, so joining the tokens' text gives back the source exactly.
//! Whitespace, comments and line endings are tokens too, because the tree
//! keeps them.
//!
//! The parser asks for one token at a time, in a [`Mode`], because ca65 can
//! change how it reads text partway through a file. [`lex`] reads a whole
//! text in one mode, for tools and tests that need no parser.
//!
//! Lexing never fails. A character ca65 does not allow becomes an `ERROR`
//! token, and a malformed number, character or string keeps its usual kind.
//! Each comes with a diagnostic saying what is wrong.
//!
//! The rules follow ca65 V2.19's scanner in its default mode, with every
//! `.feature` off:
//!
//! - A line ends at `\n`, `\r\n` or a lone `\r`. Spaces and tabs separate
//!   tokens. Form feeds and vertical tabs count as spaces only at the end of
//!   a line, where ca65 trims them.
//! - A comment runs from `;` to the end of the line. `//` and `/* */` are not
//!   comments.
//! - A name starts with a letter or `_` and goes on with letters, digits and
//!   `_`. `@` and `$` are not part of names.
//! - `$` starts a hexadecimal number and `%` a binary one; each needs at least
//!   one digit. A run that starts with a digit takes every hexadecimal digit
//!   that follows, and is hexadecimal if `h` ends it and decimal otherwise.
//! - A character constant is one byte between single quotes, and a string is
//!   anything between double quotes on one line. A backslash is an ordinary
//!   character in both.

use lang198x_syntax::{Diagnostic, DiagnosticKind, SyntaxKind, TextRange, TextSize};

use crate::{Mode, is_control_command, kinds};

/// One token: its kind and how many bytes of text it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token {
    kind: SyntaxKind,
    len: TextSize,
}

impl Token {
    /// Returns the token's kind, such as [`kinds::NUMBER`].
    #[must_use]
    pub fn kind(self) -> SyntaxKind {
        self.kind
    }

    /// Returns how many bytes of text the token covers. It is never zero.
    #[must_use]
    pub fn len(self) -> TextSize {
        self.len
    }
}

/// The tokens of a text, in order, and the problems found while lexing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lexed {
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexed {
    /// Returns the tokens in source order. Their lengths add up to the length
    /// of the text.
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Returns the problems found, in source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Splits all of `text` into tokens, in one mode.
///
/// ```
/// use lang198x_ca65::{Mode, Profile, kinds, lex};
/// use lang198x_syntax::SyntaxKind;
///
/// let lexed = lex("lda #$2A", Mode::of(Profile::CA65_2_19_6502));
/// let kinds: Vec<_> = lexed.tokens().iter().map(|t| t.kind()).collect();
/// assert_eq!(
///     kinds,
///     [kinds::IDENT, SyntaxKind::WHITESPACE, kinds::HASH, kinds::NUMBER],
/// );
/// assert!(lexed.diagnostics().is_empty());
/// ```
#[must_use]
pub fn lex(text: &str, mode: Mode) -> Lexed {
    let mut lexer = Lexer::new(text);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token(mode) {
        tokens.push(token);
    }
    Lexed {
        tokens,
        diagnostics: lexer.diagnostics,
    }
}

/// Reads a text one token at a time, each in the mode the caller gives.
///
/// Text longer than 4 GiB cannot be measured with 32-bit offsets. It gives no
/// tokens and one diagnostic.
#[derive(Debug, Clone)]
pub struct Lexer<'t> {
    text: &'t str,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'t> Lexer<'t> {
    /// Starts reading `text` from its beginning.
    #[must_use]
    pub fn new(text: &'t str) -> Self {
        let mut lexer = Self {
            text,
            pos: 0,
            diagnostics: Vec::new(),
        };
        if TextSize::of(text).is_none() {
            lexer.pos = text.len();
            lexer.diagnostics.push(Diagnostic::new(
                DiagnosticKind::Invalid,
                TextRange::empty(TextSize::ZERO),
                "the text is longer than 4 GiB",
            ));
        }
        lexer
    }

    /// Returns the problems found so far, in source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Reads the next token in `mode`, or returns `None` at the end of the
    /// text.
    pub fn next_token(&mut self, mode: Mode) -> Option<Token> {
        let rest = &self.text[self.pos..];
        let c = rest.chars().next()?;
        let (kind, len) = match c {
            '\n' => (SyntaxKind::NEWLINE, 1),
            '\r' => (
                SyntaxKind::NEWLINE,
                if rest.starts_with("\r\n") { 2 } else { 1 },
            ),
            ' ' | '\t' | '\x0C' | '\x0B' if let Some(len) = space(rest) => {
                (SyntaxKind::WHITESPACE, len)
            }
            ';' => (SyntaxKind::COMMENT, line_rest(rest)),
            '$' => (kinds::NUMBER, self.prefixed(rest, 16)),
            '%' => (kinds::NUMBER, self.prefixed(rest, 2)),
            '0'..='9' => (kinds::NUMBER, self.digit_led(rest)),
            '.' => self.dotted(rest),
            '@' => at(rest),
            '\'' => (kinds::CHAR, self.character(rest)),
            '"' => (kinds::STRING, self.string(rest)),
            ':' => colon(rest),
            _ if starts_name(c) => name(rest, mode),
            _ => match operator(rest) {
                Some(found) => found,
                None => {
                    let len = c.len_utf8();
                    self.report(len, format!("{c:?} is not allowed here"));
                    (SyntaxKind::ERROR, len)
                }
            },
        };
        // The whole text fits in 32 bits, so every token does.
        let len_size = TextSize::try_from(len).unwrap_or(TextSize::ZERO);
        self.pos += len;
        Some(Token {
            kind,
            len: len_size,
        })
    }

    /// Records an invalid-source diagnostic for the next `len` bytes.
    fn report(&mut self, len: usize, message: String) {
        let start = TextSize::try_from(self.pos).unwrap_or(TextSize::ZERO);
        let len = TextSize::try_from(len).unwrap_or(TextSize::ZERO);
        let range = TextRange::at(start, len).unwrap_or(TextRange::empty(start));
        self.diagnostics
            .push(Diagnostic::new(DiagnosticKind::Invalid, range, message));
    }

    /// A number after `$` or `%`. The prefix needs at least one digit of its
    /// base; on its own it is still a number, and is reported.
    fn prefixed(&mut self, rest: &str, radix: u32) -> usize {
        let digits = &rest[1..];
        let run = digits
            .find(|c: char| !c.is_digit(radix))
            .unwrap_or(digits.len());
        let len = 1 + run;
        let base = if radix == 16 { "hexadecimal" } else { "binary" };
        if run == 0 {
            self.report(len, format!("a {base} digit must follow `{}`", &rest[..1]));
        } else {
            // ca65 keeps 32 bits, so a number overflows once it has more
            // significant digits than that holds. Leading zeros are free.
            let limit = if radix == 16 { 8 } else { 32 };
            if digits[..run].trim_start_matches('0').len() > limit {
                self.report(len, format!("`{}` overflows 32 bits", &rest[..len]));
            }
        }
        len
    }

    /// A run that starts with a digit. It takes every hexadecimal digit that
    /// follows, then an `h` if there is one, as ca65's scanner does, and is
    /// reported if its digits do not suit its base.
    fn digit_led(&mut self, rest: &str) -> usize {
        let run = rest
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(rest.len());
        let hex = rest[run..].starts_with(['h', 'H']);
        let len = run + usize::from(hex);
        if let Err(problem) = digit_led_value(&rest[..run], if hex { 16 } else { 10 }) {
            self.report(len, format!("`{}` {problem}", &rest[..len]));
        }
        len
    }

    /// A word starting with a dot, or a dot on its own.
    fn dotted(&mut self, rest: &str) -> (SyntaxKind, usize) {
        if !rest[1..].starts_with(|c: char| starts_name(c)) {
            return (kinds::DOT, 1);
        }
        let len = 1 + name_len(&rest[1..]);
        let word = &rest[..len];
        if !is_control_command(word) {
            self.report(len, format!("`{word}` is not a control command ca65 knows"));
        }
        (kinds::CONTROL, len)
    }

    /// A character constant: one byte between single quotes. Anything else
    /// runs to the end of the line, as it does in ca65, and is reported.
    fn character(&mut self, rest: &str) -> usize {
        let mut chars = rest[1..].chars();
        let well_formed = matches!(
            (chars.next(), chars.next()),
            (Some(c), Some('\'')) if c.is_ascii() && c != '\n' && c != '\r'
        );
        if well_formed {
            return 3;
        }
        let len = line_rest(rest);
        self.report(
            len,
            "a character constant holds exactly one character".into(),
        );
        len
    }

    /// A string, from its opening quote to its closing one. If the line ends
    /// first, the string stops there and is reported.
    fn string(&mut self, rest: &str) -> usize {
        let line = line_rest(rest);
        match rest[1..line].find('"') {
            Some(close) => close + 2,
            None => {
                self.report(line, "this string has no closing \"".into());
                line
            }
        }
    }
}

/// The value of the digits of a number that starts with a digit, read as
/// ca65's scanner reads them, in base `radix`.
///
/// Leading zeros are skipped, and only the next 16 digits are read, as in
/// ca65's buffer. Before taking each digit, a value above `u32::MAX / radix`
/// is out of range; a digit outside the base is invalid.
fn digit_led_value(digits: &str, radix: u32) -> Result<u64, &'static str> {
    let max = u64::from(u32::MAX / radix);
    let significant = digits.trim_start_matches('0');
    let mut value: u64 = 0;
    for c in significant.chars().take(16) {
        if value > max {
            return Err("is out of range");
        }
        let digit = c.to_digit(radix).ok_or("has digits outside its base")?;
        value = value * u64::from(radix) + u64::from(digit);
    }
    Ok(value)
}

/// Whether a character can start a name: an ASCII letter or `_`.
fn starts_name(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

/// The length of the name at the start of `rest`: letters, digits and `_`.
fn name_len(rest: &str) -> usize {
    rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len())
}

/// A name, which may be a register of the mode's CPU or an address size.
fn name(rest: &str, mode: Mode) -> (SyntaxKind, usize) {
    let len = name_len(rest);
    let word = &rest[..len];
    // A one-letter `a`, `f` or `z` with a colon right after it is an address
    // size; with a space before the colon, it is not.
    if len == 1 && rest[1..].starts_with(':') && matches!(word, "a" | "A" | "f" | "F" | "z" | "Z") {
        return (kinds::ADDR_SIZE, 2);
    }
    let register = mode
        .cpu()
        .registers()
        .iter()
        .any(|r| r.eq_ignore_ascii_case(word));
    (
        if register {
            kinds::REGISTER
        } else {
            kinds::IDENT
        },
        len,
    )
}

/// After `@`: a cheap local name, or, with no name, an unnamed label
/// reference such as `@-`.
fn at(rest: &str) -> (SyntaxKind, usize) {
    let after = &rest[1..];
    if after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        (kinds::CHEAP_IDENT, 1 + name_len(after))
    } else {
        (kinds::UNNAMED, 1 + direction_run(after))
    }
}

/// After `:`: the scope operator, an assignment, an unnamed label reference
/// such as `:++` or `:<`, or a plain colon.
fn colon(rest: &str) -> (SyntaxKind, usize) {
    let after = &rest[1..];
    if after.starts_with(':') {
        (kinds::COLON_COLON, 2)
    } else if after.starts_with('=') {
        (kinds::COLON_EQ, 2)
    } else {
        match direction_run(after) {
            0 => (kinds::COLON, 1),
            run => (kinds::UNNAMED, 1 + run),
        }
    }
}

/// The length of a run of one of `+`, `-`, `<` or `>` at the start of
/// `rest`, counting how many unnamed labels a reference skips.
fn direction_run(rest: &str) -> usize {
    match rest.chars().next() {
        Some(c @ ('+' | '-' | '<' | '>')) => rest.find(|next| next != c).unwrap_or(rest.len()),
        _ => 0,
    }
}

/// An operator or punctuation mark, longest first.
fn operator(rest: &str) -> Option<(SyntaxKind, usize)> {
    const PAIRS: [(&str, SyntaxKind); 7] = [
        ("&&", kinds::AMP_AMP),
        ("||", kinds::PIPE_PIPE),
        ("<=", kinds::LE),
        ("<<", kinds::SHL),
        ("<>", kinds::NE),
        (">=", kinds::GE),
        (">>", kinds::SHR),
    ];
    if let Some(&(pair, kind)) = PAIRS.iter().find(|(p, _)| rest.starts_with(p)) {
        return Some((kind, pair.len()));
    }
    let kind = match rest.chars().next()? {
        ',' => kinds::COMMA,
        '#' => kinds::HASH,
        '(' => kinds::L_PAREN,
        ')' => kinds::R_PAREN,
        '[' => kinds::L_BRACK,
        ']' => kinds::R_BRACK,
        '{' => kinds::L_CURLY,
        '}' => kinds::R_CURLY,
        '+' => kinds::PLUS,
        '-' => kinds::MINUS,
        '*' => kinds::STAR,
        '/' => kinds::SLASH,
        '^' => kinds::CARET,
        '&' => kinds::AMP,
        '|' => kinds::PIPE,
        '~' => kinds::TILDE,
        '!' => kinds::BANG,
        '=' => kinds::EQ,
        '<' => kinds::LT,
        '>' => kinds::GT,
        _ => return None,
    };
    Some((kind, 1))
}

/// The length of the whitespace at the start of `rest`, or `None` if there
/// is none. Spaces and tabs always count. Form feeds and vertical tabs count
/// only in a run that reaches the end of the line, because ca65 trims
/// trailing space from each line and reads them nowhere else.
fn space(rest: &str) -> Option<usize> {
    let line = &rest[..line_rest(rest)];
    if line
        .chars()
        .all(|c| matches!(c, ' ' | '\t' | '\x0C' | '\x0B'))
    {
        return Some(line.len());
    }
    let run = line
        .find(|c| !matches!(c, ' ' | '\t'))
        .unwrap_or(line.len());
    (run > 0).then_some(run)
}

/// The length of `rest` up to its line ending, not including it.
fn line_rest(rest: &str) -> usize {
    rest.find(['\n', '\r']).unwrap_or(rest.len())
}

#[cfg(test)]
mod tests;
