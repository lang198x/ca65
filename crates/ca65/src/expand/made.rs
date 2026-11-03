//! An expansion's text, built together with its tokens.
//!
//! ca65 hands on the tokens a macro, a repeat, a define-style macro or a token
//! function makes. The expander does the same: each piece of an expansion is
//! pushed with its token kind, and the expansion is parsed from those tokens
//! rather than by lexing its text again. So a name `.ident` makes stays a
//! name even when it is spelt like a register.
//!
//! A string a token function made may hold a byte above 127, which the text
//! cannot, or characters `.sprintf` wrote, which ca65's character map leaves
//! alone. Its bytes and which of them are raw are kept in [`Strings`], beside
//! the text as provenance is, and copied with the token wherever it goes.

use std::collections::HashMap;

use lang198x_syntax::{
    ExpansionBuilder, Origin, Span, SyntaxKind, SyntaxToken, TextId, TextRange, TextSize,
};

use super::defines::Piece;
use crate::{Token, kinds};

/// What is kept of a made string: its bytes, and which of them are raw,
/// which ca65's character map leaves alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Kept {
    pub(super) bytes: Vec<u8>,
    pub(super) raw: Vec<bool>,
}

impl Kept {
    /// What to keep of a string spelt `text` and holding `bytes`, with
    /// `raw` marking the raw ones, or `None` if its text holds it all: the
    /// text between its quotes is its bytes, and none is raw. Escapes, and
    /// bytes a token function made above 127, are kept.
    pub(super) fn of(text: &str, bytes: &[u8], raw: Option<&[bool]>) -> Option<Self> {
        let raw = raw.map_or_else(|| vec![false; bytes.len()], <[bool]>::to_vec);
        let plain = crate::quoted::unquoted(text).as_bytes() == bytes && !raw.contains(&true);
        (!plain).then(|| Self {
            bytes: bytes.to_vec(),
            raw,
        })
    }
}

/// The made strings in one text that it cannot hold, each with its range
/// and what is kept of it.
pub(super) type MadeStrings = Vec<(TextRange, Kept)>;

/// What is kept of each made string whose text cannot hold it, by where
/// its token is.
#[derive(Debug, Default)]
pub(super) struct Strings(HashMap<Span, Kept>);

impl Strings {
    /// The bytes of the made string at `at`, if it is one.
    pub(super) fn get(&self, at: Span) -> Option<&[u8]> {
        self.0.get(&at).map(|kept| kept.bytes.as_slice())
    }

    /// A piece copied from `token`, at `site`, keeping what is kept of a
    /// made string.
    pub(super) fn piece(&self, token: &SyntaxToken, origin: Origin, site: Span) -> Piece {
        let mut piece = Piece::new(token, origin, site);
        if token.kind() == kinds::STRING
            && let Some(kept) = self.0.get(&site)
        {
            piece.bytes = Some(kept.bytes.clone());
            piece.raw = Some(kept.raw.clone());
        }
        piece
    }

    /// Keeps the bytes of each made string in an expansion just added as
    /// `text`.
    pub(super) fn add(&mut self, text: TextId, strings: MadeStrings) {
        for (range, kept) in strings {
            self.0.insert(Span::new(text, range), kept);
        }
    }

    /// The made strings in `text`, with their ranges in it.
    pub(super) fn in_text(&self, text: TextId) -> MadeStrings {
        self.0
            .iter()
            .filter(|(at, _)| at.text() == text)
            .map(|(at, kept)| (at.range(), kept.clone()))
            .collect()
    }
}

/// An expansion being built: its text, with each piece's origin, its
/// tokens, and the bytes of the made strings in it.
pub(super) struct Made {
    builder: ExpansionBuilder,
    tokens: Vec<Token>,
    end: TextSize,
    strings: MadeStrings,
}

impl Made {
    pub(super) fn new() -> Self {
        Self {
            builder: ExpansionBuilder::new(),
            tokens: Vec::new(),
            end: TextSize::ZERO,
            strings: Vec::new(),
        }
    }

    /// Adds one token of `kind`, spelt `text`.
    pub(super) fn push(&mut self, text: &str, kind: SyntaxKind, origin: Origin) {
        self.push_bytes(text, kind, origin, None);
    }

    /// Adds one token, keeping what is kept of it if it is a made string
    /// whose text cannot hold it.
    fn push_bytes(&mut self, text: &str, kind: SyntaxKind, origin: Origin, kept: Option<Kept>) {
        let Some(len) = TextSize::of(text).filter(|len| *len > TextSize::ZERO) else {
            return;
        };
        let Some(range) = TextRange::at(self.end, len) else {
            return;
        };
        self.end = range.end();
        self.builder.push(text, origin);
        self.tokens.push(Token::new(kind, len));
        if let Some(kept) = kept {
            self.strings.push((range, kept));
        }
    }

    /// Adds a copy of `token`, which lies at `from`, with a made string's
    /// bytes from `strings`.
    pub(super) fn token(
        &mut self,
        token: &SyntaxToken,
        from: Span,
        origin: Origin,
        strings: &Strings,
    ) {
        self.push_bytes(
            token.text(),
            token.kind(),
            origin,
            strings.0.get(&from).cloned(),
        );
    }

    /// Adds a string token, spelt `text`, holding `bytes` with `raw`
    /// marking the raw ones. They are kept beside the text whenever the
    /// text alone would not give them, as with escapes.
    pub(super) fn string(
        &mut self,
        text: &str,
        kind: SyntaxKind,
        origin: Origin,
        bytes: Vec<u8>,
        raw: Vec<bool>,
    ) {
        let kept = Kept::of(text, &bytes, Some(&raw));
        self.push_bytes(text, kind, origin, kept);
    }

    /// Adds `piece`.
    pub(super) fn piece(&mut self, piece: &Piece) {
        let kept = piece
            .bytes
            .as_deref()
            .and_then(|bytes| Kept::of(&piece.text, bytes, piece.raw.as_deref()));
        self.push_bytes(&piece.text, piece.kind, piece.origin, kept);
    }

    /// The text's builder, the tokens that cover it, and its made strings.
    pub(super) fn finish(self) -> (ExpansionBuilder, Vec<Token>, MadeStrings) {
        (self.builder, self.tokens, self.strings)
    }
}
