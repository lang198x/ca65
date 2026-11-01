//! Views of instruction operands.

use lang198x_syntax::SyntaxToken;

use super::{Expr, child, child_tokens, token, view};
use crate::kinds;

view! {
    /// One operand of an instruction.
    Operand, kinds::OPERAND
}

impl Operand {
    /// Returns the address size written before the operand: `z:`, `a:` or
    /// `f:`.
    #[must_use]
    pub fn address_size(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::ADDR_SIZE)
    }

    /// Returns `a` when the operand is the accumulator, or `q` when it is
    /// the 45GS02's 32-bit Q register.
    #[must_use]
    pub fn accumulator(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::REGISTER)
    }

    /// Returns the operand as `#value`, if it is one.
    #[must_use]
    pub fn immediate(&self) -> Option<Immediate> {
        child(&self.0)
    }

    /// Returns the operand as `(address…)`, if it is one.
    #[must_use]
    pub fn indirect(&self) -> Option<Indirect> {
        child(&self.0)
    }

    /// Returns the operand as `[address…]`, if it is one.
    #[must_use]
    pub fn long_indirect(&self) -> Option<LongIndirect> {
        child(&self.0)
    }

    /// Returns the address, when the operand is a plain or indexed address.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Returns the index after the address, such as `,x`.
    #[must_use]
    pub fn index(&self) -> Option<Index> {
        child(&self.0)
    }
}

view! {
    /// `#value`.
    Immediate, kinds::IMMEDIATE
}

impl Immediate {
    /// Returns the value.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// `(address)`, `(address,x)` or `(address),y`.
    Indirect, kinds::INDIRECT
}

impl Indirect {
    /// Returns the address inside the parentheses.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Returns the register inside the parentheses, as in `(address,x)`.
    #[must_use]
    pub fn inner_register(&self) -> Option<SyntaxToken> {
        registers(&self.0, kinds::R_PAREN).0
    }

    /// Returns the register after the parentheses, as in `(address),y`.
    #[must_use]
    pub fn outer_register(&self) -> Option<SyntaxToken> {
        registers(&self.0, kinds::R_PAREN).1
    }
}

view! {
    /// `[address]` or `[address],y`.
    LongIndirect, kinds::LONG_INDIRECT
}

impl LongIndirect {
    /// Returns the address inside the brackets.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Returns the register after the brackets, as in `[address],y`.
    #[must_use]
    pub fn outer_register(&self) -> Option<SyntaxToken> {
        registers(&self.0, kinds::R_BRACK).1
    }
}

view! {
    /// `,x`, `,y` or `,s` after an address.
    Index, kinds::INDEX
}

impl Index {
    /// Returns the register.
    #[must_use]
    pub fn register(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::REGISTER)
    }
}

/// The registers before and after `close` in an indirect operand.
fn registers(
    node: &lang198x_syntax::SyntaxNode,
    close: lang198x_syntax::SyntaxKind,
) -> (Option<SyntaxToken>, Option<SyntaxToken>) {
    let mut inner = None;
    let mut outer = None;
    let mut closed = false;
    for token in child_tokens(node) {
        if token.kind() == close {
            closed = true;
        } else if token.kind() == kinds::REGISTER {
            if closed {
                outer = Some(token);
            } else {
                inner = Some(token);
            }
        }
    }
    (inner, outer)
}
