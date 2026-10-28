//! Control commands, each read in its own shape.
//!
//! The shapes follow the handlers in ca65 V2.19's `pseudo.c`. Most commands
//! take values separated by commas. The rest take names, an on/off switch,
//! a block's name, a macro's header or a `.define`'s body. A few read the
//! rest of the line as tokens. Commands that only make sense inside an
//! expression, such as `.lobyte`, cannot start a statement.

use lang198x_syntax::DiagnosticKind;

use super::{Block, Parser};
use crate::kinds;

/// How a control command's arguments are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shape {
    /// Nothing follows: `.endif`, `.code`, `.p02`.
    Bare,
    /// Values separated by commas: `.byte 1, "a"`, `.res 4`, `.if x > 1`.
    Values,
    /// One value, then whatever is left of the line, as tokens, which ca65
    /// reads past: `.warning "careful" anything`.
    Message,
    /// Values, after which a macro defined elsewhere may be known:
    /// `.include "x.inc"`, `.macpack longbranch`.
    Include,
    /// Names, each with an optional address size and value:
    /// `.import a, b: zeropage`, `.export v = 1`.
    Names,
    /// One name, perhaps qualified by scopes: `.ifdef x`, `.refto s::v`.
    Name,
    /// An optional `+`, `-`, `on` or `off`: `.smart on`, `.autoimport +`.
    Switch,
    /// A block with an optional name and address size: `.proc main`,
    /// `.scope`, `.struct point`.
    Block(Block),
    /// The end of a block: `.endproc`, `.endstruct`.
    EndBlock,
    /// A segment's name and optional address size: `.segment "ZP": zeropage`.
    Segment,
    /// A macro's name and parameters, starting its body.
    Macro,
    /// The end of a macro's body, which only a body can hold.
    EndMacro,
    /// A define-style macro's name, parameters and body.
    Define,
    /// Deleting a define-style macro: `.undef name`.
    Undefine,
    /// Deleting a macro: `.delmac name`.
    DeleteMacro,
    /// The rest of the line, as tokens: `.ifblank`, `.dbg`.
    Tokens,
    /// A token function, such as `.ident("x")`, whose result makes the
    /// statement. Its tokens are read once preprocessing has made them.
    Synthesised,
    /// A command that changes how ca65 reads text, which this crate does
    /// not model yet: `.feature`, `.case`, `.linecont`, `.localchar`.
    Unsupported,
    /// A command that belongs inside an expression, or nowhere at the start
    /// of a statement: `.lobyte`, `.and`, `.sizeof`.
    Unexpected,
}

/// Every control command and its shape, as in ca65's `CtrlCmdTab`.
const SHAPES: &[(&str, Shape)] = &[
    (".A16", Shape::Bare),
    (".A8", Shape::Bare),
    (".ADDR", Shape::Values),
    (".ADDRSIZE", Shape::Unexpected),
    (".ALIGN", Shape::Values),
    (".AND", Shape::Unexpected),
    (".ASCIIZ", Shape::Values),
    (".ASIZE", Shape::Unexpected),
    (".ASSERT", Shape::Values),
    (".AUTOIMPORT", Shape::Switch),
    (".BANK", Shape::Unexpected),
    (".BANKBYTE", Shape::Unexpected),
    (".BANKBYTES", Shape::Values),
    (".BITAND", Shape::Unexpected),
    (".BITNOT", Shape::Unexpected),
    (".BITOR", Shape::Unexpected),
    (".BITXOR", Shape::Unexpected),
    (".BLANK", Shape::Unexpected),
    (".BSS", Shape::Bare),
    (".BYT", Shape::Values),
    (".BYTE", Shape::Values),
    (".CAP", Shape::Unexpected),
    (".CAPABILITY", Shape::Unexpected),
    (".CASE", Shape::Unsupported),
    (".CHARMAP", Shape::Values),
    (".CODE", Shape::Bare),
    (".CONCAT", Shape::Synthesised),
    (".CONDES", Shape::Values),
    (".CONST", Shape::Unexpected),
    (".CONSTRUCTOR", Shape::Values),
    (".CPU", Shape::Unexpected),
    (".DATA", Shape::Bare),
    (".DBG", Shape::Tokens),
    (".DBYT", Shape::Values),
    (".DEBUGINFO", Shape::Switch),
    (".DEF", Shape::Define),
    (".DEFINE", Shape::Define),
    (".DEFINED", Shape::Unexpected),
    (".DEFINEDMACRO", Shape::Unexpected),
    (".DELMAC", Shape::DeleteMacro),
    (".DELMACRO", Shape::DeleteMacro),
    (".DESTRUCTOR", Shape::Values),
    (".DWORD", Shape::Values),
    (".ELSE", Shape::Bare),
    (".ELSEIF", Shape::Values),
    (".END", Shape::Bare),
    (".ENDENUM", Shape::EndBlock),
    (".ENDIF", Shape::Bare),
    (".ENDMAC", Shape::EndMacro),
    (".ENDMACRO", Shape::EndMacro),
    (".ENDPROC", Shape::EndBlock),
    (".ENDREP", Shape::Bare),
    (".ENDREPEAT", Shape::Bare),
    (".ENDSCOPE", Shape::EndBlock),
    (".ENDSTRUCT", Shape::EndBlock),
    (".ENDUNION", Shape::EndBlock),
    (".ENUM", Shape::Block(Block::Enum)),
    (".ERROR", Shape::Message),
    (".EXITMAC", Shape::Bare),
    (".EXITMACRO", Shape::Bare),
    (".EXPORT", Shape::Names),
    (".EXPORTZP", Shape::Names),
    (".FARADDR", Shape::Values),
    (".FATAL", Shape::Message),
    (".FEATURE", Shape::Unsupported),
    (".FILEOPT", Shape::Values),
    (".FOPT", Shape::Values),
    (".FORCEIMPORT", Shape::Names),
    (".FORCEWORD", Shape::Unexpected),
    (".GLOBAL", Shape::Names),
    (".GLOBALZP", Shape::Names),
    (".HIBYTE", Shape::Unexpected),
    (".HIBYTES", Shape::Values),
    (".HIWORD", Shape::Unexpected),
    (".I16", Shape::Bare),
    (".I8", Shape::Bare),
    (".IDENT", Shape::Synthesised),
    (".IF", Shape::Values),
    (".IFBLANK", Shape::Tokens),
    (".IFCONST", Shape::Values),
    (".IFDEF", Shape::Name),
    (".IFNBLANK", Shape::Tokens),
    (".IFNCONST", Shape::Values),
    (".IFNDEF", Shape::Name),
    (".IFNREF", Shape::Name),
    (".IFP02", Shape::Bare),
    (".IFP02X", Shape::Bare),
    (".IFP4510", Shape::Bare),
    (".IFP45GS02", Shape::Bare),
    (".IFP6280", Shape::Bare),
    (".IFP816", Shape::Bare),
    (".IFPC02", Shape::Bare),
    (".IFPCE02", Shape::Bare),
    (".IFPDTV", Shape::Bare),
    (".IFPM740", Shape::Bare),
    (".IFPSC02", Shape::Bare),
    (".IFPSWEET16", Shape::Bare),
    (".IFPWC02", Shape::Bare),
    (".IFREF", Shape::Name),
    (".IMPORT", Shape::Names),
    (".IMPORTZP", Shape::Names),
    (".INCBIN", Shape::Values),
    (".INCLUDE", Shape::Include),
    (".INTERRUPTOR", Shape::Values),
    (".ISIZE", Shape::Unexpected),
    (".ISMNEM", Shape::Unexpected),
    (".ISMNEMONIC", Shape::Unexpected),
    (".LEFT", Shape::Synthesised),
    (".LINECONT", Shape::Unsupported),
    (".LIST", Shape::Switch),
    (".LISTBYTES", Shape::Values),
    (".LITERAL", Shape::Values),
    (".LOBYTE", Shape::Unexpected),
    (".LOBYTES", Shape::Values),
    (".LOCAL", Shape::Names),
    (".LOCALCHAR", Shape::Unsupported),
    (".LOWORD", Shape::Unexpected),
    (".MAC", Shape::Macro),
    (".MACPACK", Shape::Include),
    (".MACRO", Shape::Macro),
    (".MATCH", Shape::Unexpected),
    (".MAX", Shape::Unexpected),
    (".MID", Shape::Synthesised),
    (".MIN", Shape::Unexpected),
    (".MOD", Shape::Unexpected),
    (".NOT", Shape::Unexpected),
    (".NULL", Shape::Bare),
    (".OR", Shape::Unexpected),
    (".ORG", Shape::Values),
    (".OUT", Shape::Values),
    (".P02", Shape::Bare),
    (".P02X", Shape::Bare),
    (".P4510", Shape::Bare),
    (".P45GS02", Shape::Bare),
    (".P6280", Shape::Bare),
    (".P816", Shape::Bare),
    (".PAGELEN", Shape::Values),
    (".PAGELENGTH", Shape::Values),
    (".PARAMCOUNT", Shape::Unexpected),
    (".PC02", Shape::Bare),
    (".PCE02", Shape::Bare),
    (".PDTV", Shape::Bare),
    (".PM740", Shape::Bare),
    (".POPCHARMAP", Shape::Bare),
    (".POPCPU", Shape::Bare),
    (".POPSEG", Shape::Bare),
    (".PROC", Shape::Block(Block::Scope)),
    (".PSC02", Shape::Bare),
    (".PSWEET16", Shape::Bare),
    (".PUSHCHARMAP", Shape::Bare),
    (".PUSHCPU", Shape::Bare),
    (".PUSHSEG", Shape::Bare),
    (".PWC02", Shape::Bare),
    (".REF", Shape::Unexpected),
    (".REFERENCED", Shape::Unexpected),
    (".REFERTO", Shape::Name),
    (".REFTO", Shape::Name),
    (".RELOC", Shape::Bare),
    (".REPEAT", Shape::Values),
    (".RES", Shape::Values),
    (".RIGHT", Shape::Synthesised),
    (".RODATA", Shape::Bare),
    (".SCOPE", Shape::Block(Block::Scope)),
    (".SEGMENT", Shape::Segment),
    (".SET", Shape::Unexpected),
    (".SETCPU", Shape::Values),
    (".SHL", Shape::Unexpected),
    (".SHR", Shape::Unexpected),
    (".SIZEOF", Shape::Unexpected),
    (".SMART", Shape::Switch),
    (".SPRINTF", Shape::Synthesised),
    (".STRAT", Shape::Unexpected),
    (".STRING", Shape::Synthesised),
    (".STRLEN", Shape::Unexpected),
    (".STRUCT", Shape::Block(Block::Struct)),
    (".TAG", Shape::Values),
    (".TCOUNT", Shape::Unexpected),
    (".TIME", Shape::Unexpected),
    (".UNDEF", Shape::Undefine),
    (".UNDEFINE", Shape::Undefine),
    (".UNION", Shape::Block(Block::Struct)),
    (".VERSION", Shape::Unexpected),
    (".WARNING", Shape::Message),
    (".WORD", Shape::Values),
    (".XMATCH", Shape::Unexpected),
    (".XOR", Shape::Unexpected),
    (".ZEROPAGE", Shape::Bare),
];

/// The commands that allocate storage inside a `.struct` or `.union`.
const STORAGE: &[&str] = &[
    ".addr", ".byt", ".byte", ".dbyt", ".dword", ".faraddr", ".org", ".res", ".tag", ".word",
];

/// Whether `word` allocates storage inside a `.struct` or `.union`.
pub(super) fn is_storage(word: &str) -> bool {
    STORAGE.iter().any(|w| w.eq_ignore_ascii_case(word))
}

/// The shape of the control command `word`, or `None` for a word ca65 does
/// not know, which the lexer has reported already.
pub(crate) fn shape(word: &str) -> Option<Shape> {
    SHAPES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(word))
        .map(|&(_, shape)| shape)
}

impl Parser<'_, '_> {
    /// A control command at the start of a statement.
    pub(super) fn command(&mut self) {
        self.trivia();
        let word = self.text_at(self.pos).to_owned();
        let Some(shape) = shape(&word) else {
            // The lexer has reported the unknown word; keep what follows.
            self.builder.start_node(kinds::COMMAND);
            self.bump();
            self.tokens_to_line_end();
            self.builder.finish_node();
            return;
        };
        if shape == Shape::Unexpected {
            let message = format!("`{word}` cannot start a statement");
            self.error_to_line_end(&message);
            return;
        }
        if shape == Shape::Synthesised {
            // The function's result is the start of the statement, so the
            // whole line waits for preprocessing.
            self.builder.start_node(kinds::COMMAND);
            self.call_here();
            self.tokens_to_line_end();
            self.builder.finish_node();
            return;
        }
        self.builder.start_node(kinds::COMMAND);
        self.bump();
        if self.blocks.last() == Some(&Block::Struct) && is_storage(&word) {
            self.storage(&word);
        } else {
            self.arguments(shape, &word);
        }
        self.builder.finish_node();
    }

    /// A storage allocator inside a `.struct` or `.union`, as ca65's
    /// `DoStructInternal` reads it: `.byte` to `.dword` take an optional
    /// count, `.res` and `.org` a value, and `.tag` a struct's name and an
    /// optional count, with no comma between them.
    fn storage(&mut self, word: &str) {
        if word.eq_ignore_ascii_case(".tag") {
            self.one_name();
            if !self.at_line_end() {
                self.expression();
            }
        } else if word.eq_ignore_ascii_case(".res")
            || word.eq_ignore_ascii_case(".org")
            || !self.at_line_end()
        {
            self.expression();
        }
    }

    fn arguments(&mut self, shape: Shape, word: &str) {
        match shape {
            Shape::Bare | Shape::Unexpected | Shape::Synthesised => {}
            Shape::Values => self.values(),
            Shape::Message => {
                self.expression();
                self.tokens_to_line_end();
            }
            Shape::Include => {
                self.values();
                self.after_include = true;
            }
            Shape::Names => self.items(),
            Shape::Name => self.one_name(),
            Shape::Switch => self.switch(),
            Shape::Block(block) => {
                if self.next_kind() == Some(kinds::IDENT) {
                    self.trivia();
                    self.name(kinds::NAME);
                }
                self.address_size();
                self.blocks.push(block);
            }
            Shape::EndBlock => {
                self.blocks.pop();
            }
            Shape::Segment => {
                self.values();
                self.address_size();
            }
            Shape::Macro => self.macro_header(),
            Shape::EndMacro => {
                let message = format!("`{word}` ends a macro, but no macro is being defined");
                self.refuse_here(&message);
            }
            Shape::Define => self.define(),
            Shape::Undefine => {
                if let Some(name) = self.defined_name() {
                    self.defines.remove(&name);
                }
            }
            Shape::DeleteMacro => {
                if let Some(name) = self.defined_name() {
                    self.macros.remove(&name);
                }
            }
            Shape::Tokens => self.tokens_to_line_end(),
            Shape::Unsupported => {
                let start = self.offset(self.pos - 1);
                let end = self.offset(self.pos);
                let message =
                    format!("`{word}` changes how ca65 reads source, which is not supported yet");
                self.report_kind(DiagnosticKind::Unsupported, start, end, &message);
                self.tokens_to_line_end();
            }
        }
    }

    /// Values separated by commas.
    fn values(&mut self) {
        loop {
            self.expression();
            if self.next_kind() != Some(kinds::COMMA) {
                break;
            }
            self.bump_next();
        }
    }

    /// `name [: size] [= value]`, separated by commas.
    fn items(&mut self) {
        loop {
            self.trivia();
            self.builder.start_node(kinds::ITEM);
            if self.current() == Some(kinds::IDENT) {
                self.name(kinds::NAME);
                self.address_size();
                if matches!(self.next_kind(), Some(kinds::EQ | kinds::COLON_EQ)) {
                    self.bump_next();
                    self.expression();
                }
            } else {
                self.refuse("expected a name");
            }
            self.builder.finish_node();
            if self.next_kind() != Some(kinds::COMMA) {
                break;
            }
            self.bump_next();
        }
    }

    /// One name, perhaps qualified by scopes.
    fn one_name(&mut self) {
        self.trivia();
        if matches!(
            self.current(),
            Some(kinds::IDENT | kinds::CHEAP_IDENT | kinds::COLON_COLON)
        ) {
            self.name(kinds::NAME_REF);
        } else {
            self.refuse("expected a name");
        }
    }

    /// `+`, `-`, `on` or `off`, or nothing, which means on.
    fn switch(&mut self) {
        match self.next_kind() {
            Some(kinds::PLUS | kinds::MINUS) => self.bump_next(),
            Some(kinds::IDENT)
                if ["on", "off"]
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(self.next_text())) =>
            {
                self.bump_next();
            }
            _ if self.at_line_end() => {}
            _ => self.refuse("expected `on` or `off`"),
        }
    }

    /// `: size` after a name, if there is one.
    fn address_size(&mut self) {
        if self.next_kind() == Some(kinds::COLON) {
            self.bump_next();
            if self.next_kind() == Some(kinds::IDENT) {
                self.bump_next();
            } else {
                self.refuse("expected an address size");
            }
        }
    }

    /// `.macro name params`: records the name and starts the body.
    fn macro_header(&mut self) {
        let Some(name) = self.defined_name() else {
            return;
        };
        if self.is_mnemonic(&name) {
            let start = self.offset(self.pos - 1);
            let end = self.offset(self.pos);
            self.report(start, end, "an instruction cannot be a macro's name");
        }
        if !self.at_line_end() {
            self.params(false);
        }
        self.macros.insert(name);
        self.in_macro_body = true;
    }

    /// `.define name(params) body`: records the name and keeps the body as
    /// tokens. Parameters are in parentheses right after the name, even
    /// with a space before them, as in ca65.
    fn define(&mut self) {
        let Some(name) = self.defined_name() else {
            return;
        };
        if self.next_kind() == Some(kinds::L_PAREN) {
            self.params(true);
        }
        self.tokens_to_line_end();
        self.defines.insert(name);
    }

    /// The name a command defines or deletes, in a `NAME` node.
    fn defined_name(&mut self) -> Option<String> {
        if self.next_kind() != Some(kinds::IDENT) {
            self.refuse("expected a name");
            return None;
        }
        self.trivia();
        let name = self.text_at(self.pos).to_owned();
        self.builder.start_node(kinds::NAME);
        self.bump();
        self.builder.finish_node();
        Some(name)
    }

    /// Parameter names separated by commas, in parentheses if `parenthesised`.
    fn params(&mut self, parenthesised: bool) {
        self.trivia();
        self.builder.start_node(kinds::PARAMS);
        if parenthesised {
            self.bump();
        }
        loop {
            if self.next_kind() == Some(kinds::IDENT) {
                self.bump_next();
            } else {
                self.refuse("expected a parameter name");
                break;
            }
            if self.next_kind() != Some(kinds::COMMA) {
                break;
            }
            self.bump_next();
        }
        if parenthesised && !self.at_line_end() {
            self.require(kinds::R_PAREN, "expected `)`");
        } else if parenthesised {
            self.missing("expected `)`");
        }
        self.builder.finish_node();
    }

    /// The rest of the line, up to any comment, as a token list.
    pub(super) fn tokens_to_line_end(&mut self) {
        if self.at_line_end() {
            return;
        }
        self.trivia();
        self.builder.start_node(kinds::TOKEN_LIST);
        while !self.at_line_end() {
            self.bump_next();
        }
        self.builder.finish_node();
    }

    /// Refuses the rest of the line, from just after the command word.
    fn refuse_here(&mut self, message: &str) {
        if self.at_line_end() {
            let start = self.offset(self.pos - 1);
            let end = self.offset(self.pos);
            self.report(start, end, message);
        } else {
            self.error_to_line_end(message);
        }
    }

    fn report_kind(&mut self, kind: DiagnosticKind, start: usize, end: usize, message: &str) {
        let range = self.range(start, end);
        self.diagnostics
            .push(lang198x_syntax::Diagnostic::new(kind, range, message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_command_has_one_shape() {
        let names: Vec<&str> = SHAPES.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, crate::CONTROL_COMMANDS);
        assert_eq!(shape(".byte"), Some(Shape::Values));
        assert_eq!(shape(".BOGUS"), None);
    }
}
