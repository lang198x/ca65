//! Views of statements: instructions, control commands, macro calls,
//! assignments, members and macro bodies.

use lang198x_syntax::{SyntaxNode, SyntaxToken};

use super::{
    CallExpr, Expr, Name, NameRef, Operand, TokenList, View, child, child_tokens, children, token,
    view,
};
use crate::kinds;

/// What a line does.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Statement {
    /// A 6502 instruction, such as `lda #1`.
    Instruction(Instruction),
    /// A control command, such as `.byte 1, 2`.
    Command(Command),
    /// A use of a macro or define-style macro.
    MacroCall(MacroCall),
    /// `name = value`, `name := value` or `name .set value`.
    Assignment(Assignment),
    /// A member of a `.struct`, `.union` or `.enum`.
    Member(Member),
    /// A line of a macro's body, kept as tokens.
    MacroBody(MacroBody),
}

impl View for Statement {
    fn cast(node: SyntaxNode) -> Option<Self> {
        Some(match node.kind() {
            kinds::INSTRUCTION => Self::Instruction(Instruction(node)),
            kinds::COMMAND => Self::Command(Command(node)),
            kinds::MACRO_CALL => Self::MacroCall(MacroCall(node)),
            kinds::ASSIGNMENT => Self::Assignment(Assignment(node)),
            kinds::MEMBER => Self::Member(Member(node)),
            kinds::MACRO_BODY => Self::MacroBody(MacroBody(node)),
            _ => return None,
        })
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Instruction(view) => view.syntax(),
            Self::Command(view) => view.syntax(),
            Self::MacroCall(view) => view.syntax(),
            Self::Assignment(view) => view.syntax(),
            Self::Member(view) => view.syntax(),
            Self::MacroBody(view) => view.syntax(),
        }
    }
}

view! {
    /// A machine instruction: its mnemonic and operands.
    Instruction, kinds::INSTRUCTION
}

impl Instruction {
    /// Returns the mnemonic, as written.
    #[must_use]
    pub fn mnemonic(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::IDENT)
    }

    /// Returns the operands: usually none or one, and two for an address
    /// followed by a target.
    pub fn operands(&self) -> impl Iterator<Item = Operand> + use<> {
        children(&self.0)
    }
}

view! {
    /// A control command and what follows it. Each command's parts depend
    /// on its shape, so each accessor gives what this command has of that
    /// part.
    Command, kinds::COMMAND
}

impl Command {
    /// Returns the command's word, such as `.byte`. A command that starts
    /// with a token function, such as `.ident("x"):`, has none; see
    /// [`call`](Self::call).
    #[must_use]
    pub fn word(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::CONTROL)
    }

    /// Returns the token function that starts the statement, as in
    /// `.ident("x"): nop`.
    #[must_use]
    pub fn call(&self) -> Option<CallExpr> {
        child(&self.0)
    }

    /// Returns the values the command takes, such as `.byte`'s, in order.
    pub fn values(&self) -> impl Iterator<Item = Expr> + use<> {
        children(&self.0)
    }

    /// Returns the items of an `.import` or `.export` list.
    pub fn items(&self) -> impl Iterator<Item = Item> + use<> {
        children(&self.0)
    }

    /// Returns the name the command defines, such as a macro's, a block's
    /// or a `.define`'s.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// Returns the name the command asks about, as in `.ifdef x`.
    #[must_use]
    pub fn name_ref(&self) -> Option<NameRef> {
        child(&self.0)
    }

    /// Returns a macro's or `.define`'s parameters.
    #[must_use]
    pub fn params(&self) -> Option<Params> {
        child(&self.0)
    }

    /// Returns the tokens the command keeps as written, such as a
    /// `.define`'s body or `.ifblank`'s argument.
    #[must_use]
    pub fn tokens(&self) -> Option<TokenList> {
        child(&self.0)
    }

    /// Returns the address size after a colon, as in `.segment "ZP":
    /// zeropage`.
    #[must_use]
    pub fn address_size(&self) -> Option<SyntaxToken> {
        let mut tokens = child_tokens(&self.0).skip_while(|t| t.kind() != kinds::COLON);
        tokens.next()?;
        tokens.next().filter(|t| t.kind() == kinds::IDENT)
    }

    /// Returns the switch's setting, such as `on` or `+`, if one was written.
    #[must_use]
    pub fn switch(&self) -> Option<SyntaxToken> {
        child_tokens(&self.0)
            .skip(1)
            .find(|t| matches!(t.kind(), kinds::PLUS | kinds::MINUS | kinds::IDENT))
    }
}

view! {
    /// A macro's or `.define`'s parameter names.
    Params, kinds::PARAMS
}

impl Params {
    /// Returns the parameter names, in order.
    pub fn names(&self) -> impl Iterator<Item = SyntaxToken> + use<> {
        child_tokens(&self.0).filter(|t| t.kind() == kinds::IDENT)
    }
}

view! {
    /// One name in an `.import` or `.export` list.
    Item, kinds::ITEM
}

impl Item {
    /// Returns the name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// Returns the address size after a colon, if one was written.
    #[must_use]
    pub fn address_size(&self) -> Option<SyntaxToken> {
        let mut tokens = child_tokens(&self.0).skip_while(|t| t.kind() != kinds::COLON);
        tokens.next()?;
        tokens.next().filter(|t| t.kind() == kinds::IDENT)
    }

    /// Returns the value after `=` or `:=`, if one was written.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// A use of a macro or define-style macro, and its arguments.
    MacroCall, kinds::MACRO_CALL
}

impl MacroCall {
    /// Returns the macro's name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::IDENT)
    }

    /// Returns the arguments, in order. An empty argument has no tokens.
    pub fn args(&self) -> impl Iterator<Item = super::Arg> + use<> {
        children(&self.0)
    }
}

view! {
    /// `name = value`, `name := value` or `name .set value`.
    Assignment, kinds::ASSIGNMENT
}

impl Assignment {
    /// Returns the name being set.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// Returns the operator: `=`, `:=` or `.set`.
    #[must_use]
    pub fn op(&self) -> Option<SyntaxToken> {
        child_tokens(&self.0)
            .find(|t| matches!(t.kind(), kinds::EQ | kinds::COLON_EQ | kinds::CONTROL))
    }

    /// Returns the value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// A member of a `.struct` or `.union`, with its storage, or of an
    /// `.enum`, with its optional value.
    Member, kinds::MEMBER
}

impl Member {
    /// Returns the member's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// Returns a struct member's storage command, such as `.word`.
    #[must_use]
    pub fn storage(&self) -> Option<Command> {
        child(&self.0)
    }

    /// Returns an enum member's value, if one was written.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// A line of a macro's body, kept as tokens until the macro is called.
    MacroBody, kinds::MACRO_BODY
}

impl MacroBody {
    /// Returns the line's tokens, without whitespace.
    pub fn tokens(&self) -> impl Iterator<Item = SyntaxToken> + use<> {
        child_tokens(&self.0)
    }
}
