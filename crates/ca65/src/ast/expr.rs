//! Views of expressions.

use lang198x_syntax::{SyntaxNode, SyntaxToken};

use super::line::{name_parts, name_text, starts_global};
use super::{View, child, child_tokens, children, view};
use crate::kinds;

/// An expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    /// A number, a character, a string or `*`.
    Literal(Literal),
    /// A use of a symbol.
    Name(NameRef),
    /// A reference to an unnamed label, such as `:+`.
    Unnamed(UnnamedRef),
    /// An expression in parentheses.
    Paren(ParenExpr),
    /// A prefix operator and what it applies to.
    Unary(UnaryExpr),
    /// Two operands and the operator between them.
    Binary(BinExpr),
    /// A function, such as `.lobyte(x)`, or a value word, such as `.time`.
    Call(CallExpr),
}

impl View for Expr {
    fn cast(node: SyntaxNode) -> Option<Self> {
        Some(match node.kind() {
            kinds::LITERAL => Self::Literal(Literal(node)),
            kinds::NAME_REF => Self::Name(NameRef(node)),
            kinds::UNNAMED_REF => Self::Unnamed(UnnamedRef(node)),
            kinds::PAREN_EXPR => Self::Paren(ParenExpr(node)),
            kinds::UNARY_EXPR => Self::Unary(UnaryExpr(node)),
            kinds::BIN_EXPR => Self::Binary(BinExpr(node)),
            kinds::CALL_EXPR => Self::Call(CallExpr(node)),
            _ => return None,
        })
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Literal(view) => view.syntax(),
            Self::Name(view) => view.syntax(),
            Self::Unnamed(view) => view.syntax(),
            Self::Paren(view) => view.syntax(),
            Self::Unary(view) => view.syntax(),
            Self::Binary(view) => view.syntax(),
            Self::Call(view) => view.syntax(),
        }
    }
}

/// The node's first token, which for these views is the one they wrap or
/// the operator they start with.
fn first_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    child_tokens(node).next()
}

view! {
    /// A number, character, string or `*`, as written.
    Literal, kinds::LITERAL
}

impl Literal {
    /// Returns the token: a `NUMBER`, a `CHAR`, a `STRING` or a `STAR`.
    #[must_use]
    pub fn token(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }
}

view! {
    /// A use of a symbol, perhaps qualified by scopes, such as `x`, `@near`
    /// or `outer::x`.
    NameRef, kinds::NAME_REF
}

impl NameRef {
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

view! {
    /// A reference to an unnamed label, such as `:+` or `:--`.
    UnnamedRef, kinds::UNNAMED_REF
}

impl UnnamedRef {
    /// Returns the token.
    #[must_use]
    pub fn token(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }
}

view! {
    /// An expression in parentheses.
    ParenExpr, kinds::PAREN_EXPR
}

impl ParenExpr {
    /// Returns the expression inside.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// A prefix operator and what it applies to.
    UnaryExpr, kinds::UNARY_EXPR
}

impl UnaryExpr {
    /// Returns the operator token, such as `-`, `<` or `.not`.
    #[must_use]
    pub fn op(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Returns the operand.
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// Two operands and the operator between them.
    BinExpr, kinds::BIN_EXPR
}

impl BinExpr {
    /// Returns the left operand.
    #[must_use]
    pub fn lhs(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Returns the operator token, such as `+` or `.and`.
    #[must_use]
    pub fn op(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Returns the right operand.
    #[must_use]
    pub fn rhs(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

view! {
    /// A function and its arguments, such as `.lobyte(x)`, or a value word,
    /// such as `.time`.
    CallExpr, kinds::CALL_EXPR
}

impl CallExpr {
    /// Returns the function's word, such as `.lobyte`.
    #[must_use]
    pub fn function(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Returns the arguments, in order.
    pub fn args(&self) -> impl Iterator<Item = Arg> + use<> {
        children(&self.0)
    }
}

view! {
    /// One argument of a function or macro call.
    Arg, kinds::ARG
}

impl Arg {
    /// Returns the argument as a value, if it is one.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Returns the argument as tokens, if it is a token list.
    #[must_use]
    pub fn tokens(&self) -> Option<TokenList> {
        child(&self.0)
    }
}

view! {
    /// Tokens taken as they are, such as `{1, 2}`.
    TokenList, kinds::TOKEN_LIST
}

impl TokenList {
    /// Returns the tokens, without whitespace.
    pub fn tokens(&self) -> impl Iterator<Item = SyntaxToken> + use<> {
        child_tokens(&self.0)
    }

    /// Returns whether braces surround the tokens.
    #[must_use]
    pub fn is_braced(&self) -> bool {
        child_tokens(&self.0)
            .next()
            .is_some_and(|t| t.kind() == kinds::L_CURLY)
    }

    /// Returns the tokens inside the braces, or all of them when there are
    /// none, without whitespace. ca65 removes the braces from a braced list.
    pub fn contents(&self) -> impl Iterator<Item = SyntaxToken> + use<> {
        let mut tokens: Vec<SyntaxToken> = child_tokens(&self.0).collect();
        if self.is_braced() {
            tokens.remove(0);
            if tokens.last().is_some_and(|t| t.kind() == kinds::R_CURLY) {
                tokens.pop();
            }
        }
        tokens.into_iter()
    }
}
