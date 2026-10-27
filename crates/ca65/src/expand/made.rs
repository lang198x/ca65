//! An expansion's text, built together with its tokens.
//!
//! ca65 hands on the tokens a macro, a repeat, a define-style macro or a token
//! function makes. The expander does the same: each piece of an expansion is
//! pushed with its token kind, and the expansion is parsed from those tokens
//! rather than by lexing its text again. So a name `.ident` makes stays a
//! name even when it is spelt like a register.
//!
//! A string a token function made may hold a byte above 127, which the text
//! cannot. Its bytes are kept in [`Strings`], beside the text as provenance
//! is, and copied with the token wherever it goes.

use std::collections::HashMap;

use lang198x_syntax::{
    ExpansionBuilder, Origin, Span, SyntaxKind, SyntaxToken, TextId, TextRange, TextSize,
};

use super::defines::Piece;
use crate::{Token, kinds};

/// The made strings in one text that it cannot hold, each with its range
/// and its bytes.
pub(super) type MadeStrings = Vec<(TextRange, Vec<u8>)>;

/// The bytes of each made string whose text cannot hold them, by where its
/// token is.
#[derive(Debug, Default)]
pub(super) struct Strings(HashMap<Span, Vec<u8>>);

impl Strings {
    /// The bytes of the made string at `at`, if it is one.
    pub(super) fn get(&self, at: Span) -> Option<&[u8]> {
        self.0.get(&at).map(Vec::as_slice)
    }

    /// A piece copied from `token`, at `site`, keeping a made string's bytes.
    pub(super) fn piece(&self, token: &SyntaxToken, origin: Origin, site: Span) -> Piece {
        let mut piece = Piece::new(token, origin, site);
        if token.kind() == kinds::STRING {
            piece.bytes = self.get(site).map(<[u8]>::to_vec);
        }
        piece
    }

    /// Keeps the bytes of each made string in an expansion just added as
    /// `text`.
    pub(super) fn add(&mut self, text: TextId, strings: MadeStrings) {
        for (range, bytes) in strings {
            self.0.insert(Span::new(text, range), bytes);
        }
    }

    /// The made strings in `text`, with their ranges in it.
    pub(super) fn in_text(&self, text: TextId) -> MadeStrings {
        self.0
            .iter()
            .filter(|(at, _)| at.text() == text)
            .map(|(at, bytes)| (at.range(), bytes.clone()))
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

    /// Adds one token, keeping `bytes` if it is a made string whose text
    /// cannot hold them.
    fn push_bytes(&mut self, text: &str, kind: SyntaxKind, origin: Origin, bytes: Option<&[u8]>) {
        let Some(len) = TextSize::of(text).filter(|len| *len > TextSize::ZERO) else {
            return;
        };
        let Some(range) = TextRange::at(self.end, len) else {
            return;
        };
        self.end = range.end();
        self.builder.push(text, origin);
        self.tokens.push(Token::new(kind, len));
        if let Some(bytes) = bytes {
            self.strings.push((range, bytes.to_vec()));
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
        self.push_bytes(token.text(), token.kind(), origin, strings.get(from));
    }

    /// Adds `piece`.
    pub(super) fn piece(&mut self, piece: &Piece) {
        self.push_bytes(
            &piece.text,
            piece.kind,
            piece.origin,
            piece.bytes.as_deref(),
        );
    }

    /// The text's builder, the tokens that cover it, and its made strings.
    pub(super) fn finish(self) -> (ExpansionBuilder, Vec<Token>, MadeStrings) {
        (self.builder, self.tokens, self.strings)
    }
}
