//! The kinds of tokens and nodes in a ca65 syntax tree.
//!
//! Whitespace, newlines, comments and errors use the kinds every Lang198x
//! language shares, from [`SyntaxKind`]. Everything else is numbered here.

use lang198x_syntax::SyntaxKind;

const fn kind(offset: u16) -> SyntaxKind {
    match SyntaxKind::language(SyntaxKind::FIRST_LANGUAGE + offset) {
        Some(kind) => kind,
        None => panic!("language kinds start at SyntaxKind::FIRST_LANGUAGE"),
    }
}

/// Declares each kind as a constant, numbered in order, and the table of
/// their names, so a kind and its name can never drift apart.
macro_rules! kinds {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        kinds!(@number 0u16; $($(#[$doc])* $name,)*);

        const NAMES: &[&str] = &[$(stringify!($name),)*];
    };
    (@number $n:expr; $(#[$doc:meta])* $name:ident, $($rest:tt)*) => {
        $(#[$doc])*
        pub const $name: SyntaxKind = kind($n);
        kinds!(@number $n + 1; $($rest)*);
    };
    (@number $n:expr;) => {};
}

kinds! {
    // Tokens.

    /// A name: a label, a symbol or a mnemonic. Which one is decided by the
    /// parser, not the lexer.
    IDENT,
    /// A cheap local name, such as `@loop`. It belongs to the last ordinary
    /// label.
    CHEAP_IDENT,
    /// A word starting with a dot, such as `.byte`, `.and` or `.lobyte`. An
    /// unknown one, such as `.bogus`, is still this kind, with a diagnostic.
    CONTROL,
    /// A register of the current CPU, such as `A` or `x`.
    REGISTER,
    /// An address size written before an operand, with its colon: `z:` for
    /// zero page, `a:` for absolute and `f:` for far.
    ADDR_SIZE,
    /// A reference to an unnamed label, such as `:+`, `:--` or `:<`, or an
    /// `@` used the same way.
    UNNAMED,
    /// A number, such as `42`, `$2A`, `%101010` or `2Ah`. An invalid one,
    /// such as `$` or `1e3`, is still a number token, with a diagnostic.
    NUMBER,
    /// A character constant with its quotes, such as `'a'`.
    CHAR,
    /// A string with its quotes, such as `"abc"`.
    STRING,
    /// `.` on its own.
    DOT,
    /// `:`, which ends a label or, alone, defines an unnamed one.
    COLON,
    /// `::`, the scope operator.
    COLON_COLON,
    /// `:=`
    COLON_EQ,
    /// `,`
    COMMA,
    /// `#`
    HASH,
    /// `(`
    L_PAREN,
    /// `)`
    R_PAREN,
    /// `[`
    L_BRACK,
    /// `]`
    R_BRACK,
    /// `{`
    L_CURLY,
    /// `}`
    R_CURLY,
    /// `+`
    PLUS,
    /// `-`
    MINUS,
    /// `*`, which multiplies or stands for the current address.
    STAR,
    /// `/`
    SLASH,
    /// `^`, which is exclusive or, or picks the bank byte as a prefix.
    CARET,
    /// `&`
    AMP,
    /// `&&`
    AMP_AMP,
    /// `|`
    PIPE,
    /// `||`
    PIPE_PIPE,
    /// `~`
    TILDE,
    /// `!`
    BANG,
    /// `=`
    EQ,
    /// `<`, which compares, or picks the low byte as a prefix.
    LT,
    /// `<=`
    LE,
    /// `<<`
    SHL,
    /// `<>`
    NE,
    /// `>`, which compares, or picks the high byte as a prefix.
    GT,
    /// `>=`
    GE,
    /// `>>`
    SHR,

    // Nodes.

    /// A whole source file: one `LINE` for each line of text.
    SOURCE_FILE,
    /// One line of source, with its line ending.
    LINE,
    /// A named label: its `NAME` and colon, as in `loop:`.
    LABEL,
    /// An unnamed label: `:` alone, or `@:`.
    UNNAMED_LABEL,
    /// A symbol set to a value: `name = value`, `name := value` or
    /// `name .set value`.
    ASSIGNMENT,
    /// A name being defined, perhaps qualified by scopes, as in `outer::x`.
    NAME,
    /// A machine instruction: its mnemonic and any `OPERAND`.
    INSTRUCTION,
    /// An instruction's operand.
    OPERAND,
    /// `#value`.
    IMMEDIATE,
    /// `(address)`, `(address,x)` or `(address),y`.
    INDIRECT,
    /// `[address]` or `[address],y`.
    LONG_INDIRECT,
    /// `,x`, `,y` or `,s` after an address.
    INDEX,
    /// A control command at the start of a statement, such as `.byte 1,2`.
    COMMAND,
    /// A control command's arguments, kept as written until each command's
    /// shape is read.
    ARGS,
    /// A number, a character constant, a string or `*`.
    LITERAL,
    /// A use of a symbol, perhaps qualified by scopes.
    NAME_REF,
    /// A reference to an unnamed label, such as `:+`.
    UNNAMED_REF,
    /// `(value)` inside an expression.
    PAREN_EXPR,
    /// A prefix operator and what it applies to.
    UNARY_EXPR,
    /// Two values and the operator between them.
    BIN_EXPR,
    /// A function such as `.lobyte(value)`, or a value word such as `.time`.
    CALL_EXPR,
    /// One argument of a function.
    ARG,
    /// Tokens taken as they are, such as `{1, 2}` in `.tcount({1, 2})`.
    TOKEN_LIST,
}

/// The name of one of this crate's kinds, such as `"NUMBER"`, for tests and
/// tree dumps. `None` for a shared kind, such as whitespace, or a number no
/// kind uses.
///
/// ```
/// use lang198x_ca65::kinds;
///
/// assert_eq!(kinds::name(kinds::NUMBER), Some("NUMBER"));
/// ```
#[must_use]
pub fn name(kind: SyntaxKind) -> Option<&'static str> {
    let offset = kind.get().checked_sub(SyntaxKind::FIRST_LANGUAGE)?;
    NAMES.get(usize::from(offset)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_numbered_in_order_and_named() {
        assert_eq!(name(IDENT), Some("IDENT"));
        assert_eq!(name(SOURCE_FILE), Some("SOURCE_FILE"));
        assert_eq!(name(TOKEN_LIST), Some("TOKEN_LIST"));
        assert_eq!(
            usize::from(TOKEN_LIST.get() - IDENT.get()) + 1,
            NAMES.len(),
            "the last kind is numbered one below the table's length"
        );
        let mut names = NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), NAMES.len(), "two kinds share a name");
    }

    #[test]
    fn shared_and_unknown_kinds_have_no_ca65_name() {
        assert_eq!(name(SyntaxKind::COMMENT), None);
        let past_the_end = SyntaxKind::language(SyntaxKind::FIRST_LANGUAGE + 500);
        assert_eq!(past_the_end.and_then(name), None);
    }
}
