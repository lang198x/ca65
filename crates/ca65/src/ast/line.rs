//! Views of the file, its lines, their labels and the names they define.

use lang198x_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::{Statement, child, child_nodes, child_tokens, children, token, view};
use crate::kinds;

view! {
    /// A whole source file, the root of every tree.
    SourceFile, kinds::SOURCE_FILE
}

impl SourceFile {
    /// Returns the file's lines, in order.
    pub fn lines(&self) -> impl Iterator<Item = Line> + use<> {
        children(&self.0)
    }
}

view! {
    /// One line of source: an optional unnamed label, an optional label, an
    /// optional statement and an optional comment.
    Line, kinds::LINE
}

impl Line {
    /// Returns the line's unnamed label, `:` or `@:`, if it has one.
    #[must_use]
    pub fn unnamed_label(&self) -> Option<UnnamedLabel> {
        child(&self.0)
    }

    /// Returns the line's named label, if it has one.
    #[must_use]
    pub fn label(&self) -> Option<Label> {
        child(&self.0)
    }

    /// Returns what the line does, if anything: an instruction, a control
    /// command, a macro call, an assignment, a member or a macro's body.
    #[must_use]
    pub fn statement(&self) -> Option<Statement> {
        child(&self.0)
    }

    /// Returns the line's comment, if it has one.
    #[must_use]
    pub fn comment(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::COMMENT)
    }

    /// Returns the parts of the line the parser could not read, each in an
    /// `ERROR` node.
    pub fn errors(&self) -> impl Iterator<Item = SyntaxNode> + use<> {
        child_nodes(&self.0).filter(|node| node.kind() == SyntaxKind::ERROR)
    }
}

view! {
    /// An unnamed label at the start of a line: `:` or `@:`.
    UnnamedLabel, kinds::UNNAMED_LABEL
}

view! {
    /// A named label, such as `loop:`.
    Label, kinds::LABEL
}

impl Label {
    /// Returns the name the label defines.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }
}

view! {
    /// A name being defined, perhaps qualified by scopes, such as `loop`,
    /// `@near` or `outer::x`.
    Name, kinds::NAME
}

impl Name {
    /// Returns the name's parts: its names, without the `::` between them.
    pub fn parts(&self) -> impl Iterator<Item = SyntaxToken> + use<> {
        name_parts(&self.0)
    }

    /// Returns whether the name starts with `::`, for the global scope.
    #[must_use]
    pub fn is_global(&self) -> bool {
        starts_global(&self.0)
    }

    /// Returns the name as written, without whitespace.
    #[must_use]
    pub fn text(&self) -> String {
        name_text(&self.0)
    }
}

/// The name tokens of a `NAME` or `NAME_REF`.
pub(super) fn name_parts(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + use<> {
    child_tokens(node).filter(|t| matches!(t.kind(), kinds::IDENT | kinds::CHEAP_IDENT))
}

/// Whether a `NAME` or `NAME_REF` starts with `::`.
pub(super) fn starts_global(node: &SyntaxNode) -> bool {
    child_tokens(node)
        .next()
        .is_some_and(|t| t.kind() == kinds::COLON_COLON)
}

/// A `NAME` or `NAME_REF` as written, without whitespace.
pub(super) fn name_text(node: &SyntaxNode) -> String {
    child_tokens(node).map(|t| t.text().to_owned()).collect()
}
