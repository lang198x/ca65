//! Expressions, with ca65 V2.19's precedence from its `expr.c`.
//!
//! From loosest to tightest:
//!
//! 1. `||` and `.or`;
//! 2. `&&`, `.and` and `.xor`;
//! 3. the comparisons `=`, `<>`, `<`, `>`, `<=` and `>=`;
//! 4. `+`, `-`, `|` and `.bitor`;
//! 5. `*`, `/`, `.mod`, `&`, `.bitand`, `^`, `.bitxor`, `<<`, `.shl`, `>>`
//!    and `.shr`;
//! 6. a factor: a value, or a prefix operator and the factor it applies to.
//!
//! Every binary level is left-associative. The prefixes `+`, `-`, `~`,
//! `.bitnot`, `<`, `>` and `^` bind tightest, but `!` and `.not` apply to the
//! whole expression after them, so `!0||1` is 0. Asm198x's probes of ca65's
//! expression grammar measure each rule (asm198x#994).

use lang198x_syntax::SyntaxKind;

use super::{MAX_DEPTH, Parser};
use crate::kinds;

/// An operator: a symbol token, or a word such as `.and`.
#[derive(Clone, Copy)]
enum Operator {
    Symbol(SyntaxKind),
    Word(&'static str),
}

use Operator::{Symbol, Word};

/// The binary levels, loosest first.
const LEVELS: [&[Operator]; 5] = [
    &[Symbol(kinds::PIPE_PIPE), Word(".or")],
    &[Symbol(kinds::AMP_AMP), Word(".and"), Word(".xor")],
    &[
        Symbol(kinds::EQ),
        Symbol(kinds::NE),
        Symbol(kinds::LT),
        Symbol(kinds::GT),
        Symbol(kinds::LE),
        Symbol(kinds::GE),
    ],
    &[
        Symbol(kinds::PLUS),
        Symbol(kinds::MINUS),
        Symbol(kinds::PIPE),
        Word(".bitor"),
    ],
    &[
        Symbol(kinds::STAR),
        Symbol(kinds::SLASH),
        Word(".mod"),
        Symbol(kinds::AMP),
        Word(".bitand"),
        Symbol(kinds::CARET),
        Word(".bitxor"),
        Symbol(kinds::SHL),
        Word(".shl"),
        Symbol(kinds::SHR),
        Word(".shr"),
    ],
];

/// Prefixes that apply to the next factor.
const TIGHT_PREFIXES: &[Operator] = &[
    Symbol(kinds::PLUS),
    Symbol(kinds::MINUS),
    Symbol(kinds::TILDE),
    Word(".bitnot"),
    Symbol(kinds::LT),
    Symbol(kinds::GT),
    Symbol(kinds::CARET),
];

/// Prefixes that apply to the whole expression after them.
const LOOSE_PREFIXES: &[Operator] = &[Symbol(kinds::BANG), Word(".not")];

/// How a function's arguments are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arguments {
    /// No parentheses: a value word, such as `.time`.
    None,
    /// Expressions in parentheses, such as `.lobyte(value)`.
    Values,
    /// Token lists in parentheses, such as `.tcount({1,2})`.
    Tokens,
    /// Values, except a braced argument is a token list, as in
    /// `.left(2,{1,2,3})`.
    Mixed,
}

/// The control commands that can stand in an expression, and how each reads
/// its arguments. ca65's `Factor` handles the first group; the token
/// functions, such as `.left`, are replaced by tokens before ca65 parses, but
/// in source they sit where a value does.
const FUNCTIONS: &[(&str, Arguments)] = &[
    (".addrsize", Arguments::Values),
    (".asize", Arguments::None),
    (".bank", Arguments::Values),
    (".bankbyte", Arguments::Values),
    (".blank", Arguments::Tokens),
    (".cap", Arguments::Values),
    (".capability", Arguments::Values),
    (".concat", Arguments::Mixed),
    (".const", Arguments::Values),
    (".cpu", Arguments::None),
    (".def", Arguments::Values),
    (".defined", Arguments::Values),
    (".definedmacro", Arguments::Values),
    (".hibyte", Arguments::Values),
    (".hiword", Arguments::Values),
    (".ident", Arguments::Mixed),
    (".isize", Arguments::None),
    (".ismnem", Arguments::Values),
    (".ismnemonic", Arguments::Values),
    (".left", Arguments::Mixed),
    (".lobyte", Arguments::Values),
    (".loword", Arguments::Values),
    (".match", Arguments::Tokens),
    (".max", Arguments::Values),
    (".mid", Arguments::Mixed),
    (".min", Arguments::Values),
    (".paramcount", Arguments::None),
    (".ref", Arguments::Values),
    (".referenced", Arguments::Values),
    (".right", Arguments::Mixed),
    (".sizeof", Arguments::Values),
    (".sprintf", Arguments::Mixed),
    (".strat", Arguments::Values),
    (".string", Arguments::Mixed),
    (".strlen", Arguments::Values),
    (".tcount", Arguments::Tokens),
    (".time", Arguments::None),
    (".version", Arguments::None),
    (".xmatch", Arguments::Tokens),
];

fn function(word: &str) -> Option<Arguments> {
    FUNCTIONS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(word))
        .map(|&(_, arguments)| arguments)
}

impl Parser<'_, '_> {
    pub(super) fn expression(&mut self) {
        self.trivia();
        if self.depth >= MAX_DEPTH {
            self.error_to_line_end("this expression is nested too deeply");
            return;
        }
        self.depth += 1;
        if self.at_operator(LOOSE_PREFIXES) {
            self.builder.start_node(kinds::UNARY_EXPR);
            self.bump();
            self.expression();
            self.builder.finish_node();
        } else {
            self.level(0);
        }
        self.depth -= 1;
    }

    fn level(&mut self, index: usize) {
        self.trivia();
        let Some(operators) = LEVELS.get(index) else {
            self.factor();
            return;
        };
        let checkpoint = self.builder.checkpoint();
        self.level(index + 1);
        while self.at_operator(operators) {
            self.builder.start_node_at(checkpoint, kinds::BIN_EXPR);
            self.bump_next();
            self.level(index + 1);
            self.builder.finish_node();
        }
    }

    fn factor(&mut self) {
        self.trivia();
        if self.at_operator(TIGHT_PREFIXES) {
            if self.depth >= MAX_DEPTH {
                self.error_to_line_end("this expression is nested too deeply");
                return;
            }
            self.depth += 1;
            self.builder.start_node(kinds::UNARY_EXPR);
            self.bump();
            self.factor();
            self.builder.finish_node();
            self.depth -= 1;
            return;
        }
        match self.current() {
            Some(kinds::NUMBER | kinds::CHAR | kinds::STRING | kinds::STAR) => {
                self.builder.start_node(kinds::LITERAL);
                self.bump();
                self.builder.finish_node();
            }
            Some(kinds::IDENT | kinds::CHEAP_IDENT | kinds::COLON_COLON) => {
                self.name(kinds::NAME_REF);
            }
            Some(kinds::UNNAMED) => {
                self.builder.start_node(kinds::UNNAMED_REF);
                self.bump();
                self.builder.finish_node();
            }
            Some(kinds::L_PAREN) => {
                self.builder.start_node(kinds::PAREN_EXPR);
                self.bump();
                self.expression();
                self.expect(kinds::R_PAREN, "expected `)`");
                self.builder.finish_node();
            }
            Some(kinds::CONTROL) if self.at_operator(LOOSE_PREFIXES) => {
                // `.not` after a tighter operator, as in `1 + .not 0`, still
                // takes the rest of the expression, as `!` does.
                self.expression();
            }
            Some(kinds::BANG) => self.expression(),
            Some(kinds::CONTROL) => match function(self.text_at(self.pos)) {
                Some(arguments) => self.call(arguments),
                None => self.refuse("expected a value"),
            },
            Some(SyntaxKind::ERROR) => {
                // The lexer has reported this character already.
                self.bump();
            }
            _ => self.refuse("expected a value"),
        }
    }

    /// The function at the current token, such as a token function that
    /// starts a statement, and its arguments.
    pub(super) fn call_here(&mut self) {
        let arguments = function(self.text_at(self.pos)).unwrap_or(Arguments::Mixed);
        self.call(arguments);
    }

    /// A function and its arguments.
    fn call(&mut self, arguments: Arguments) {
        self.builder.start_node(kinds::CALL_EXPR);
        self.bump();
        if arguments != Arguments::None {
            if self.next_kind() == Some(kinds::L_PAREN) {
                self.bump_next();
                loop {
                    self.argument(arguments);
                    if self.next_kind() != Some(kinds::COMMA) {
                        break;
                    }
                    self.bump_next();
                }
                self.require(kinds::R_PAREN, "expected `)`");
            } else {
                self.refuse("expected `(`");
            }
        }
        self.builder.finish_node();
    }

    fn argument(&mut self, arguments: Arguments) {
        self.trivia();
        self.builder.start_node(kinds::ARG);
        let braced = self.current() == Some(kinds::L_CURLY);
        match arguments {
            Arguments::Tokens => self.token_list(),
            Arguments::Mixed if braced => self.token_list(),
            _ => self.expression(),
        }
        self.builder.finish_node();
    }

    /// A braced list of tokens, or the tokens up to the next `,` or `)` that
    /// is not inside parentheses. Braces do not nest, as in ca65: the first
    /// `}` ends the list, so `{{1}` holds `{` and `1`.
    pub(super) fn token_list(&mut self) {
        self.builder.start_node(kinds::TOKEN_LIST);
        if self.current() == Some(kinds::L_CURLY) {
            self.bump();
            let mut closed = false;
            while !self.at_line_end() {
                closed = self.next_kind() == Some(kinds::R_CURLY);
                self.bump_next();
                if closed {
                    break;
                }
            }
            if !closed {
                self.missing("expected `}`");
            }
        } else {
            let mut depth = 0usize;
            while !self.at_line_end() {
                match self.next_kind() {
                    Some(kinds::COMMA) if depth == 0 => break,
                    Some(kinds::R_PAREN) if depth == 0 => break,
                    Some(kinds::L_PAREN) => depth += 1,
                    Some(kinds::R_PAREN) => depth -= 1,
                    _ => {}
                }
                self.bump_next();
            }
        }
        self.builder.finish_node();
    }

    /// Whether the next token, after any whitespace, is one of `operators`.
    fn at_operator(&self, operators: &[Operator]) -> bool {
        operators.iter().any(|operator| match *operator {
            Symbol(kind) => self.next_kind() == Some(kind),
            Word(word) => {
                self.next_kind() == Some(kinds::CONTROL)
                    && self.next_text().eq_ignore_ascii_case(word)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_function_is_a_control_command() {
        for (name, _) in FUNCTIONS {
            assert!(crate::is_control_command(name), "{name}");
        }
        for level in LEVELS
            .iter()
            .copied()
            .chain([TIGHT_PREFIXES, LOOSE_PREFIXES])
        {
            for operator in level {
                if let Word(word) = operator {
                    assert!(crate::is_control_command(word), "{word}");
                }
            }
        }
    }
}
