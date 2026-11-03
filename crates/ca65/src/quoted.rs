//! Strings and character constants as ca65 V2.19's scanner reads them,
//! with `string_escapes`, `loose_string_term` and `missing_char_term`.
//!
//! The lexer uses this to find where a quoted token ends, the expander to
//! keep the bytes a string holds when escapes change them, and lowering to
//! read a character constant. One reader serves all three, so they agree.
//!
//! Source read from Latin-1 bytes holds each byte as the character with the
//! same number, so a string's character up to U+00FF is that one byte, as
//! ca65 copies it. Source given as text holds each character as its UTF-8
//! bytes.
//!
//! ca65 V2.19's scanner reads the byte $FF as the end of the file, since it
//! keeps characters in a signed `char`, so a string holding one ends there
//! with `Newline in string constant`. Here $FF is a byte like any other: a
//! named divergence.

use lang198x_syntax::CharSet;

/// A quoted constant read from the start of some text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Quoted {
    /// How much of the text it takes, quotes included.
    pub(crate) len: usize,
    /// The bytes it holds.
    pub(crate) bytes: Vec<u8>,
    /// Which bytes are raw: written as `\x` or octal escapes, which ca65's
    /// character map leaves alone.
    pub(crate) raw: Vec<bool>,
    /// The first thing wrong with it, in ca65's words, if anything is.
    pub(crate) problem: Option<&'static str>,
}

/// What a string token spelt `text` holds, without its quotes, which are
/// single with `loose_string_term`. A string whose escapes change its bytes
/// has them kept beside it instead.
pub(crate) fn unquoted(text: &str) -> &str {
    let quote = if text.starts_with('\'') { '\'' } else { '"' };
    let text = text.strip_prefix(quote).unwrap_or(text);
    text.strip_suffix(quote).unwrap_or(text)
}

/// The bytes a string token spelt `text` holds when it has no escapes:
/// the text between its quotes, read in `char_set`.
pub(crate) fn unquoted_bytes(text: &str, char_set: Option<CharSet>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for c in unquoted(text).chars() {
        push(c, char_set, &mut bytes);
    }
    bytes
}

/// Adds the bytes `c` was read from to `bytes`: one, for a character up to
/// U+00FF of Latin-1 source, or its UTF-8 bytes.
fn push(c: char, char_set: Option<CharSet>, bytes: &mut Vec<u8>) {
    match (char_set, u8::try_from(c)) {
        (Some(CharSet::Latin1), Ok(byte)) => bytes.push(byte),
        _ => {
            let mut buffer = [0; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
        }
    }
}

/// The string at the start of `rest`, from its opening quote `term` to the
/// next one. With `escapes`, a backslash starts an escape. A string the
/// line ends inside stops there, and says so. Its characters are read in
/// `char_set`.
pub(crate) fn string(rest: &str, term: char, escapes: bool, char_set: Option<CharSet>) -> Quoted {
    let mut quoted = Quoted {
        len: term.len_utf8(),
        bytes: Vec::new(),
        raw: Vec::new(),
        problem: None,
    };
    loop {
        let tail = &rest[quoted.len..];
        let Some(c) = tail.chars().next().filter(|c| !matches!(c, '\n' | '\r')) else {
            quoted.problem.get_or_insert("Newline in string constant");
            return quoted;
        };
        if c == term {
            quoted.len += c.len_utf8();
            return quoted;
        }
        if c == '\\' && escapes {
            let (byte, raw, len, problem) = escape(&tail[1..]);
            quoted.len += 1 + len;
            quoted.bytes.push(byte);
            quoted.raw.push(raw);
            if let Some(problem) = problem {
                quoted.problem.get_or_insert(problem);
            }
            continue;
        }
        quoted.len += c.len_utf8();
        push(c, char_set, &mut quoted.bytes);
        quoted.raw.resize(quoted.bytes.len(), false);
    }
}

/// The character constant at the start of `rest`, after its opening `'`:
/// one character or escape, then a closing `'`, which `missing_char_term`
/// lets it leave off.
///
/// With escapes, `'\'` is a backslash. That is what ca65 V2.19's scanner
/// means it to be, by a comment in `ReadStringConst`, but its special case
/// overwrites the character it then checks, so ca65 refuses it as an
/// illegal character constant. This follows the intent: a named divergence.
pub(crate) fn character(
    rest: &str,
    escapes: bool,
    missing_term: bool,
    char_set: Option<CharSet>,
) -> Quoted {
    let mut quoted = Quoted {
        len: 1,
        bytes: Vec::new(),
        raw: Vec::new(),
        problem: None,
    };
    let tail = &rest[1..];
    match tail.chars().next() {
        None | Some('\n' | '\r') => {
            quoted.problem = Some("Newline in string constant");
            return quoted;
        }
        Some('\\') if escapes && tail[1..].starts_with('\'') => {
            // ca65 reads `\'` here as a backslash and leaves the quote to
            // close the constant.
            quoted.len += 1;
            quoted.bytes.push(b'\\');
            quoted.raw.push(false);
        }
        Some('\\') if escapes => {
            let (byte, raw, len, problem) = escape(&tail[1..]);
            quoted.len += 1 + len;
            quoted.bytes.push(byte);
            quoted.raw.push(raw);
            quoted.problem = problem;
        }
        Some(c) => {
            quoted.len += c.len_utf8();
            push(c, char_set, &mut quoted.bytes);
            quoted.raw.resize(quoted.bytes.len(), false);
        }
    }
    if rest[quoted.len..].starts_with('\'') {
        quoted.len += 1;
    } else if !missing_term {
        quoted.problem.get_or_insert("Illegal character constant");
    }
    quoted
}

/// The escape after a backslash at the start of `rest`: its byte, whether
/// the byte is raw, how much of `rest` it takes, and what is wrong with it.
fn escape(rest: &str) -> (u8, bool, usize, Option<&'static str>) {
    let unsupported = Some("Unsupported escape sequence in string constant");
    let Some(c) = rest.chars().next().filter(|c| !matches!(c, '\n' | '\r')) else {
        return (
            b'\\',
            false,
            0,
            Some("Unterminated escape sequence in string constant"),
        );
    };
    let named = match c {
        '?' => Some(b'?'),
        'a' => Some(0x07),
        'b' => Some(0x08),
        'e' => Some(0x1B),
        'f' => Some(0x0C),
        'r' => Some(b'\r'),
        'n' => Some(b'\n'),
        't' => Some(b'\t'),
        'v' => Some(0x0B),
        '\\' => Some(b'\\'),
        '\'' => Some(b'\''),
        '"' => Some(b'"'),
        _ => None,
    };
    if let Some(byte) = named {
        return (byte, false, 1, None);
    }
    match c {
        // Up to three octal digits, which may not pass 255.
        '0'..='7' => {
            let digits = rest
                .bytes()
                .take(3)
                .take_while(|b| (b'0'..=b'7').contains(b))
                .count();
            let value = rest[..digits]
                .bytes()
                .fold(0u32, |value, b| value * 8 + u32::from(b - b'0'));
            match u8::try_from(value) {
                Ok(byte) => (byte, true, digits, None),
                Err(_) => (
                    0,
                    true,
                    digits,
                    Some("Octal character constant out of range"),
                ),
            }
        }
        // Exactly two hexadecimal digits.
        'x' | 'X' => match rest.get(1..3).map(|hex| u8::from_str_radix(hex, 16)) {
            Some(Ok(byte)) if rest[1..3].bytes().all(|b| b.is_ascii_hexdigit()) => {
                (byte, true, 3, None)
            }
            _ => (0, false, 1, unsupported),
        },
        _ => (0, false, c.len_utf8(), unsupported),
    }
}

#[cfg(test)]
mod tests;
