//! An expansion's text, built together with its tokens.
//!
//! ca65 hands on the tokens a macro, a repeat, a define-style macro or a token
//! function makes. The expander does the same: each piece of an expansion is
//! pushed with its token kind, and the expansion is parsed from those tokens
//! rather than by lexing its text again. So a name `.ident` makes stays a
//! name even when it is spelt like a register.

use lang198x_syntax::{ExpansionBuilder, Origin, SyntaxKind, SyntaxToken, TextSize};

use super::defines::Piece;
use crate::Token;

/// An expansion being built: its text, with each piece's origin, and its
/// tokens.
pub(super) struct Made {
    builder: ExpansionBuilder,
    tokens: Vec<Token>,
}

impl Made {
    pub(super) fn new() -> Self {
        Self {
            builder: ExpansionBuilder::new(),
            tokens: Vec::new(),
        }
    }

    /// Adds one token of `kind`, spelt `text`.
    pub(super) fn push(&mut self, text: &str, kind: SyntaxKind, origin: Origin) {
        let Some(len) = TextSize::of(text).filter(|len| *len > TextSize::ZERO) else {
            return;
        };
        self.builder.push(text, origin);
        self.tokens.push(Token::new(kind, len));
    }

    /// Adds a copy of `token`.
    pub(super) fn token(&mut self, token: &SyntaxToken, origin: Origin) {
        self.push(token.text(), token.kind(), origin);
    }

    /// Adds `piece`.
    pub(super) fn piece(&mut self, piece: &Piece) {
        self.push(&piece.text, piece.kind, piece.origin);
    }

    /// The text's builder, and the tokens that cover it.
    pub(super) fn finish(self) -> (ExpansionBuilder, Vec<Token>) {
        (self.builder, self.tokens)
    }
}
