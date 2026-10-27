//! Token functions: `.concat`, `.ident`, `.left`, `.mid`, `.right`,
//! `.string` and `.sprintf`, and the functions whose value depends only on
//! their tokens: `.strlen`, `.strat`, `.tcount`, `.blank`, `.match`,
//! `.xmatch`, `.definedmacro` and `.ismnemonic`.
//!
//! This follows ca65 V2.19's `nexttok.c`. A token function runs as its line
//! is read, and is replaced by the tokens it makes, which are read again, so
//! one function's result may feed another. Functions run only in lines that
//! run: not in a skipped branch, and not in a body being collected or a
//! `.define`'s body, which ca65 reads in raw mode.
//!
//! A count or a number is a constant expression, which only the host can
//! evaluate. A token list runs to the first `)`, or is in braces. Every token
//! a function makes is `Synthesised` from the whole call.
//!
//! In ca65 the second group are functions of an expression, not of the
//! scanner. Each one's value comes from the tokens it reads, the macros
//! defined so far and the CPU, none of which can change partway through a
//! line, so it is made here as a number. The made string's bytes are here
//! too, which the parsed line does not have. The functions that ask about
//! symbols, such as `.defined`, depend on the names read before them on the
//! line, so they are left to whatever lowers the line.
//!
//! `.mid` takes tokens from its start, however large. ca65 V2.19 instead
//! treats a start above 100 as 0, which its manual does not say; that is
//! not copied.

use lang198x_syntax::{Origin, Span, SyntaxKind};

use super::defines::{Piece, Problem};
use crate::kinds;

/// The token functions of ca65's scanner, in lower case. They run wherever
/// they are read, even in another function's token list.
const SCANNER: &[&str] = &[
    ".concat", ".ident", ".left", ".mid", ".right", ".sprintf", ".string",
];

/// The functions of an expression made here, in lower case. In a token list
/// they are tokens, as in ca65, and run only when the list's result is read
/// again outside one.
const VALUES: &[&str] = &[
    ".blank",
    ".definedmacro",
    ".ismnem",
    ".ismnemonic",
    ".match",
    ".strat",
    ".strlen",
    ".tcount",
    ".xmatch",
];

/// Whether `piece` calls a function that runs here: any of them outside a
/// token list, and only the scanner's in one.
fn is_function(piece: &Piece, in_list: bool) -> bool {
    let named = |names: &[&str]| names.iter().any(|f| f.eq_ignore_ascii_case(&piece.text));
    piece.kind == kinds::CONTROL && (named(SCANNER) || (!in_list && named(VALUES)))
}

/// What a token function asks of whatever runs it.
pub(super) trait Answers {
    /// The value of the constant expression made of `pieces`, for a call
    /// at `site`.
    fn value(&mut self, pieces: &[Piece], site: Span) -> Result<i64, Problem>;

    /// Whether a classic macro is named `name`, in this case.
    fn is_macro(&self, name: &str) -> bool;

    /// Whether `name` is a mnemonic of the current CPU, in any case.
    fn is_mnemonic(&self, name: &str) -> bool;
}

/// Whether any of `pieces` calls a token function.
pub(super) fn any(pieces: &[Piece]) -> bool {
    pieces.iter().any(|piece| is_function(piece, false))
}

/// Runs the token functions in `pieces`, asking `answers` what they need.
pub(super) fn run(pieces: Vec<Piece>, answers: &mut dyn Answers) -> Result<Vec<Piece>, Problem> {
    let mut stream = Stream {
        stack: vec![pieces.into_iter()],
        answers,
        lists: 0,
    };
    let mut out = Vec::new();
    while let Some(piece) = stream.next()? {
        out.push(piece);
    }
    Ok(out)
}

struct Stream<'e> {
    stack: Vec<std::vec::IntoIter<Piece>>,
    answers: &'e mut dyn Answers,
    /// How many token lists are being read, one inside another.
    lists: usize,
}

/// A call being read: where it is, for messages and for what it makes.
struct Call {
    name: String,
    site: Span,
}

impl Call {
    fn problem(&self, message: impl Into<String>) -> Problem {
        Problem::invalid(self.site, message.into())
    }

    /// A piece the call made.
    fn made(&self, text: String, kind: SyntaxKind) -> Piece {
        Piece {
            text,
            kind,
            origin: Origin::Synthesised {
                inputs: self.site,
                call_site: self.site,
            },
            site: self.site,
            bytes: None,
        }
    }

    fn gap(&self) -> Piece {
        self.made(" ".to_owned(), SyntaxKind::WHITESPACE)
    }
}

impl Stream<'_> {
    fn next_raw(&mut self) -> Option<Piece> {
        while let Some(top) = self.stack.last_mut() {
            if let Some(piece) = top.next() {
                return Some(piece);
            }
            self.stack.pop();
        }
        None
    }

    /// The next piece, with any token function run first.
    fn next(&mut self) -> Result<Option<Piece>, Problem> {
        loop {
            let Some(piece) = self.next_raw() else {
                return Ok(None);
            };
            if is_function(&piece, self.lists > 0) {
                let call = Call {
                    name: piece.text.to_ascii_lowercase(),
                    site: piece.site,
                };
                let made = self.call(&call)?;
                let mut text = vec![call.gap()];
                text.extend(made);
                text.push(call.gap());
                self.stack.push(text.into_iter());
                continue;
            }
            return Ok(Some(piece));
        }
    }

    /// The next piece that is not blank, or `None` at the end of the line.
    fn next_solid(&mut self) -> Result<Option<Piece>, Problem> {
        loop {
            match self.next()? {
                Some(piece) if piece.is_blank() => {}
                Some(piece) if piece.ends_line() => {
                    self.stack.push(vec![piece].into_iter());
                    return Ok(None);
                }
                other => return Ok(other),
            }
        }
    }

    fn expect(&mut self, call: &Call, kind: SyntaxKind, what: &str) -> Result<Piece, Problem> {
        match self.next_solid()? {
            Some(piece) if piece.kind == kind => Ok(piece),
            _ => Err(call.problem(format!("expected {what} in `{}`", call.name))),
        }
    }

    /// A string argument's bytes.
    fn string(&mut self, call: &Call) -> Result<Vec<u8>, Problem> {
        let piece = self.expect(call, kinds::STRING, "a string")?;
        Ok(piece.string_bytes())
    }

    /// The pieces of an expression, up to a `,` or `)` outside parentheses,
    /// which is left to read, and its value.
    fn value(&mut self, call: &Call) -> Result<i64, Problem> {
        let lists = std::mem::take(&mut self.lists);
        let pieces = self.value_pieces();
        self.lists = lists;
        let pieces = pieces?;
        if pieces.iter().all(Piece::is_blank) {
            return Err(call.problem(format!("expected a value in `{}`", call.name)));
        }
        self.answers.value(&pieces, call.site)
    }

    /// The pieces of an expression, up to a `,` or `)` outside parentheses.
    fn value_pieces(&mut self) -> Result<Vec<Piece>, Problem> {
        let mut pieces = Vec::new();
        let mut depth = 0_usize;
        while let Some(piece) = self.next()? {
            let closes = matches!(piece.kind, kinds::R_PAREN | kinds::R_BRACK);
            if piece.ends_line() || (depth == 0 && (piece.kind == kinds::COMMA || closes)) {
                self.stack.push(vec![piece].into_iter());
                break;
            }
            if matches!(piece.kind, kinds::L_PAREN | kinds::L_BRACK) {
                depth += 1;
            } else if closes {
                depth -= 1;
            }
            pieces.push(piece);
        }
        Ok(pieces)
    }

    /// A token list: up to the first `)`, or in braces and then `)`. Blanks
    /// are left out.
    fn list(&mut self, call: &Call) -> Result<Vec<Piece>, Problem> {
        self.list_to(call, kinds::R_PAREN)
    }

    /// A token list: up to the first `after`, or in braces and then `after`.
    /// Braces do not nest: the first `}` ends the list.
    fn list_to(&mut self, call: &Call, after: SyntaxKind) -> Result<Vec<Piece>, Problem> {
        self.lists += 1;
        let list = self.list_pieces(call, after);
        self.lists -= 1;
        list
    }

    fn list_pieces(&mut self, call: &Call, after: SyntaxKind) -> Result<Vec<Piece>, Problem> {
        let mut piece = self.next_solid()?;
        let braced = piece.as_ref().is_some_and(|p| p.kind == kinds::L_CURLY);
        if braced {
            piece = self.next_solid()?;
        }
        let end = if braced { kinds::R_CURLY } else { after };
        let mut list = Vec::new();
        loop {
            match piece {
                None => {
                    return Err(call.problem(format!(
                        "the token list in `{}` runs to the end of the line",
                        call.name
                    )));
                }
                Some(p) if p.kind == end => break,
                Some(p) => list.push(p),
            }
            piece = self.next_solid()?;
        }
        if braced {
            let what = if after == kinds::COMMA { "`,`" } else { "`)`" };
            self.expect(call, after, what)?;
        }
        Ok(list)
    }

    /// A name for `.definedmacro` or `.ismnemonic`, and its `)`.
    fn name(&mut self, call: &Call) -> Result<String, Problem> {
        let name = self.expect(call, kinds::IDENT, "a name")?;
        self.expect(call, kinds::R_PAREN, "`)`")?;
        Ok(name.text)
    }

    /// The tokens of `.match` or `.xmatch`, and whether they match: `exact`
    /// for `.xmatch`, which compares names, strings and numbers too.
    fn matches(&mut self, call: &Call, exact: bool) -> Result<bool, Problem> {
        let first = self.list_to(call, kinds::COMMA)?;
        let second = self.list(call)?;
        Ok(first.len() == second.len()
            && first
                .iter()
                .zip(&second)
                .all(|(a, b)| same_token(a, b, exact)))
    }

    /// Runs a call of a token function, whose name has been read.
    fn call(&mut self, call: &Call) -> Result<Vec<Piece>, Problem> {
        self.expect(call, kinds::L_PAREN, "`(`")?;
        match call.name.as_str() {
            ".concat" => {
                let mut bytes = self.string(call)?;
                loop {
                    match self.next_solid()? {
                        Some(p) if p.kind == kinds::COMMA => bytes.extend(self.string(call)?),
                        Some(p) if p.kind == kinds::R_PAREN => break,
                        _ => return Err(call.problem("expected `,` or `)` in `.concat`")),
                    }
                }
                Ok(vec![string_piece(call, bytes)])
            }
            ".ident" => {
                let name = self.string(call)?;
                self.expect(call, kinds::R_PAREN, "`)`")?;
                Ok(vec![self.ident(call, &name)?])
            }
            ".left" | ".right" => {
                let count = count(self.value(call)?);
                self.expect(call, kinds::COMMA, "`,`")?;
                let list = self.list(call)?;
                let keep = count.min(list.len());
                let kept = if call.name == ".left" {
                    list[..keep].to_vec()
                } else {
                    list[list.len() - keep..].to_vec()
                };
                Ok(spaced(call, kept))
            }
            ".mid" => {
                let start = count(self.value(call)?);
                self.expect(call, kinds::COMMA, "`,`")?;
                let length = count(self.value(call)?);
                self.expect(call, kinds::COMMA, "`,`")?;
                let list = self.list(call)?;
                let start = start.min(list.len());
                let end = start.saturating_add(length).min(list.len());
                Ok(spaced(call, list[start..end].to_vec()))
            }
            ".string" => {
                let text = self.name_or_number(call)?;
                self.expect(call, kinds::R_PAREN, "`)`")?;
                Ok(vec![string_piece(call, text.into_bytes())])
            }
            ".strlen" => {
                let bytes = self.string(call)?;
                self.expect(call, kinds::R_PAREN, "`)`")?;
                Ok(vec![number_piece(call, bytes.len())])
            }
            ".strat" => {
                let bytes = self.string(call)?;
                self.expect(call, kinds::COMMA, "`,`")?;
                let index = self.value(call)?;
                self.expect(call, kinds::R_PAREN, "`)`")?;
                let byte = usize::try_from(index)
                    .ok()
                    .and_then(|i| bytes.get(i))
                    .ok_or_else(|| call.problem("the index of `.strat` is outside the string"))?;
                Ok(vec![number_piece(call, usize::from(*byte))])
            }
            ".tcount" => {
                let count = self.list(call)?.len();
                Ok(vec![number_piece(call, count)])
            }
            ".blank" => {
                let blank = self.list(call)?.is_empty();
                Ok(vec![number_piece(call, usize::from(blank))])
            }
            ".match" | ".xmatch" => {
                let matched = self.matches(call, call.name == ".xmatch")?;
                Ok(vec![number_piece(call, usize::from(matched))])
            }
            ".definedmacro" => {
                let name = self.name(call)?;
                let defined = self.answers.is_macro(&name);
                Ok(vec![number_piece(call, usize::from(defined))])
            }
            ".ismnem" | ".ismnemonic" => {
                let name = self.name(call)?;
                let mnemonic = self.answers.is_mnemonic(&name);
                Ok(vec![number_piece(call, usize::from(mnemonic))])
            }
            _ => {
                let bytes = self.sprintf(call)?;
                Ok(vec![string_piece(call, bytes)])
            }
        }
    }

    /// The identifier `.ident` makes from the bytes of `name`.
    fn ident(&self, call: &Call, name: &[u8]) -> Result<Piece, Problem> {
        let name = String::from_utf8_lossy(name);
        let rest = name.strip_prefix('@').unwrap_or(&name);
        let mut chars = rest.chars();
        let valid = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(call.problem(format!("`{name}` is not a valid name for `.ident`")));
        }
        // The name is handed on as a name, not lexed again, so one spelt
        // like a register stays a name, as in ca65.
        let kind = if name.starts_with('@') {
            kinds::CHEAP_IDENT
        } else {
            kinds::IDENT
        };
        Ok(call.made(name.into_owned(), kind))
    }

    /// `.string`'s argument: a name, perhaps scoped, or a number.
    fn name_or_number(&mut self, call: &Call) -> Result<String, Problem> {
        let first = self.next_solid()?;
        match first {
            Some(p) if p.kind == kinds::CHEAP_IDENT => Ok(p.text),
            Some(p) if matches!(p.kind, kinds::IDENT | kinds::COLON_COLON) => {
                let mut text = String::new();
                let mut scope = p.kind == kinds::COLON_COLON;
                text.push_str(&p.text);
                loop {
                    let next = self.next_solid()?;
                    match next {
                        Some(p) if scope && p.kind == kinds::IDENT => text.push_str(&p.text),
                        Some(p) if !scope && p.kind == kinds::COLON_COLON => {
                            text.push_str(&p.text);
                        }
                        other => {
                            if let Some(other) = other {
                                self.stack.push(vec![other].into_iter());
                            }
                            break;
                        }
                    }
                    scope = !scope;
                }
                Ok(text)
            }
            other => {
                if let Some(other) = other {
                    self.stack.push(vec![other].into_iter());
                }
                Ok(self.value(call)?.to_string())
            }
        }
    }

    /// `.sprintf(format, args)`, after its `(`, up to and including its `)`:
    /// the bytes it makes.
    fn sprintf(&mut self, call: &Call) -> Result<Vec<u8>, Problem> {
        let format = self.string(call)?;
        let mut out = Vec::new();
        let mut bytes = format.into_iter().peekable();
        while let Some(b) = bytes.next() {
            if b != b'%' {
                out.push(b);
                continue;
            }
            if bytes.peek() == Some(&b'%') {
                bytes.next();
                out.push(b'%');
                continue;
            }
            let mut spec = Spec::default();
            while let Some(&flag) = bytes.peek() {
                match flag {
                    b'-' => spec.left = true,
                    b'+' => spec.plus = true,
                    b' ' => spec.space = true,
                    b'#' => spec.alternate = true,
                    b'0' => spec.zero = true,
                    _ => break,
                }
                bytes.next();
            }
            spec.width = digits(&mut bytes);
            if bytes.peek() == Some(&b'.') {
                bytes.next();
                spec.precision = Some(digits(&mut bytes));
            }
            let Some(conversion) = bytes.next().map(char::from) else {
                return Err(call.problem("the format of `.sprintf` ends partway through"));
            };
            self.expect(call, kinds::COMMA, "`,`")?;
            let piece = match conversion {
                'd' | 'i' | 'o' | 'u' | 'x' | 'X' => spec.integer(self.value(call)?, conversion),
                's' => {
                    let mut text = self.string(call)?;
                    if let Some(n) = spec.precision {
                        text.truncate(n);
                    }
                    spec.pad(Vec::new(), text, false)
                }
                'c' => {
                    let value = self.value(call)?;
                    let byte = u8::try_from(value)
                        .ok()
                        .filter(|b| *b != 0)
                        .ok_or_else(|| call.problem("a `%c` argument must be 1 to 255"))?;
                    spec.pad(Vec::new(), vec![byte], false)
                }
                other => {
                    return Err(
                        call.problem(format!("`%{other}` is not a format `.sprintf` knows"))
                    );
                }
            };
            out.extend(piece);
        }
        self.expect(call, kinds::R_PAREN, "`)`")?;
        Ok(out)
    }
}

/// A count, with a negative one taken as 0, as in ca65.
fn count(value: i64) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// A string token holding `bytes`. Its text is the bytes in quotes, which
/// is how it is shown, since an expansion is not lexed again. A byte above
/// 127, which the text cannot hold, is shown as an escape such as `\xFF`,
/// and the piece keeps the bytes themselves.
fn string_piece(call: &Call, bytes: Vec<u8>) -> Piece {
    let mut piece = call.made(shown(&bytes), kinds::STRING);
    if !bytes.is_ascii() {
        piece.bytes = Some(bytes);
    }
    piece
}

/// How a made string's bytes are shown: in quotes, with each byte above 127
/// as `\xNN`, and a backslash doubled when there is such a byte.
fn shown(bytes: &[u8]) -> String {
    let escaped = !bytes.is_ascii();
    let mut text = String::from("\"");
    for &b in bytes {
        match b {
            0x80.. => text.push_str(&format!("\\x{b:02X}")),
            b'\\' if escaped => text.push_str("\\\\"),
            _ => text.push(char::from(b)),
        }
    }
    text.push('"');
    text
}

/// A number token for `value`.
fn number_piece(call: &Call, value: usize) -> Piece {
    call.made(value.to_string(), kinds::NUMBER)
}

/// Whether two tokens are the same token to `.match`, or, when `exact`, to
/// `.xmatch`, as ca65 V2.19's `TokCmp` decides. Each register and each
/// control word is a token of its own, with a word's long and short
/// spellings one token, and `.and` the same token as `&&`. To `.xmatch`,
/// names and strings must also be spelt alike, and numbers and characters
/// have the same value.
fn same_token(a: &Piece, b: &Piece, exact: bool) -> bool {
    let (ka, kb) = (token_identity(a), token_identity(b));
    if ka != kb {
        return false;
    }
    if !exact {
        return true;
    }
    match a.kind {
        kinds::IDENT | kinds::CHEAP_IDENT => a.text == b.text,
        kinds::STRING => a.string_bytes() == b.string_bytes(),
        kinds::NUMBER => crate::number::value(&a.text) == crate::number::value(&b.text),
        kinds::CHAR => a.text == b.text,
        _ => true,
    }
}

/// What makes a token the token it is, apart from the value `.xmatch`
/// compares.
fn token_identity(piece: &Piece) -> (SyntaxKind, String) {
    match piece.kind {
        kinds::REGISTER => (kinds::REGISTER, piece.text.to_ascii_uppercase()),
        kinds::CONTROL => {
            let word = piece.text.to_ascii_lowercase();
            match OPERATOR_WORDS.iter().find(|(w, _)| *w == word) {
                Some((_, kind)) => (*kind, String::new()),
                None => {
                    let word = SHORT_WORDS
                        .iter()
                        .find(|(short, _)| *short == word)
                        .map_or(word.clone(), |(_, long)| (*long).to_owned());
                    (kinds::CONTROL, word)
                }
            }
        }
        kind => (kind, String::new()),
    }
}

/// The control words that are the same token as an operator, from ca65
/// V2.19's `scanner.c`.
const OPERATOR_WORDS: &[(&str, SyntaxKind)] = &[
    (".and", kinds::AMP_AMP),
    (".bitand", kinds::AMP),
    (".bitnot", kinds::TILDE),
    (".bitor", kinds::PIPE),
    (".bitxor", kinds::CARET),
    (".not", kinds::BANG),
    (".or", kinds::PIPE_PIPE),
    (".shl", kinds::SHL),
    (".shr", kinds::SHR),
];

/// The short spellings of control words, each with its long one, from ca65
/// V2.19's `scanner.c`.
const SHORT_WORDS: &[(&str, &str)] = &[
    (".byt", ".byte"),
    (".cap", ".capability"),
    (".def", ".defined"),
    (".delmac", ".delmacro"),
    (".endmac", ".endmacro"),
    (".endrep", ".endrepeat"),
    (".exitmac", ".exitmacro"),
    (".fopt", ".fileopt"),
    (".ismnem", ".ismnemonic"),
    (".mac", ".macro"),
    (".pagelen", ".pagelength"),
    (".ref", ".referenced"),
    (".refto", ".referto"),
    (".undef", ".undefine"),
];

/// The tokens of a list, with a space between each, as tokens.
fn spaced(call: &Call, list: Vec<Piece>) -> Vec<Piece> {
    let mut out = Vec::new();
    for piece in list {
        out.push(call.gap());
        out.push(piece);
    }
    out
}

fn digits(bytes: &mut std::iter::Peekable<std::vec::IntoIter<u8>>) -> usize {
    let mut n = 0_usize;
    while let Some(d) = bytes
        .peek()
        .filter(|b| b.is_ascii_digit())
        .map(|b| b - b'0')
    {
        n = n.saturating_mul(10).saturating_add(usize::from(d));
        bytes.next();
    }
    n
}

/// One `%` conversion's flags, width and precision, as C's `printf` reads
/// them, which ca65 hands its formats to.
#[derive(Default)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alternate: bool,
    zero: bool,
    width: usize,
    precision: Option<usize>,
}

impl Spec {
    /// `value` as `%d`, `%i`, `%o`, `%u`, `%x` or `%X` on a `long`.
    fn integer(&self, value: i64, conversion: char) -> Vec<u8> {
        #[allow(clippy::cast_sign_loss)]
        let bits = value as u64;
        let (sign, mut digits) = match conversion {
            'd' | 'i' => {
                let sign = if value < 0 {
                    "-"
                } else if self.plus {
                    "+"
                } else if self.space {
                    " "
                } else {
                    ""
                };
                (sign, value.unsigned_abs().to_string())
            }
            'o' => ("", format!("{bits:o}")),
            'u' => ("", bits.to_string()),
            'x' => ("", format!("{bits:x}")),
            _ => ("", format!("{bits:X}")),
        };
        if let Some(precision) = self.precision {
            if precision == 0 && value == 0 {
                digits.clear();
            }
            while digits.len() < precision {
                digits.insert(0, '0');
            }
        }
        let mut prefix = sign.to_owned();
        if self.alternate {
            match conversion {
                'o' if !digits.starts_with('0') => digits.insert(0, '0'),
                'x' if value != 0 => prefix.push_str("0x"),
                'X' if value != 0 => prefix.push_str("0X"),
                _ => {}
            }
        }
        let zero = self.zero && self.precision.is_none();
        self.pad(prefix.into_bytes(), digits.into_bytes(), zero)
    }

    /// `body` after `prefix`, padded to the width in bytes.
    fn pad(&self, prefix: Vec<u8>, body: Vec<u8>, zero: bool) -> Vec<u8> {
        let fill = self.width.saturating_sub(prefix.len() + body.len());
        let mut out = Vec::with_capacity(fill + prefix.len() + body.len());
        if self.left {
            out.extend(prefix);
            out.extend(body);
            out.extend(std::iter::repeat_n(b' ', fill));
        } else if zero {
            out.extend(prefix);
            out.extend(std::iter::repeat_n(b'0', fill));
            out.extend(body);
        } else {
            out.extend(std::iter::repeat_n(b' ', fill));
            out.extend(prefix);
            out.extend(body);
        }
        out
    }
}
