//! Classic macros: their definitions, and the text of each call.
//!
//! This follows ca65 V2.19's `MacDef`, `StartExpClassic` and `MacExpand` in
//! `macro.c`. A body is the lines after `.macro` up to the first `.endmacro`
//! that starts a line, so definitions do not nest. A `.local` line in a body
//! is not part of it: it names symbols to rename in each call.
//!
//! A call's text is the body's, with each parameter replaced by the tokens
//! of its argument, braces removed, and each `.local` name replaced by one
//! unique to the call. `.paramcount` becomes the number of arguments given.
//! A missing argument is empty. Parameter names are case-sensitive, as in
//! ca65.

use lang198x_syntax::{
    ExpansionBuilder, Origin, Span, SyntaxElement, SyntaxKind, SyntaxToken, TextId,
};

use crate::ast::{Line, MacroCall, Statement, View};
use crate::kinds;

/// A classic macro, as defined.
#[derive(Debug, Clone)]
pub(super) struct Macro {
    /// The parameter names, in order.
    pub(super) params: Vec<String>,
    /// The `.local` names, in order.
    locals: Vec<String>,
    /// The body's lines, each with the text it is in.
    body: Vec<(Line, TextId)>,
}

impl Macro {
    /// A macro with `params` whose body is `lines`, each with its text.
    /// `.local` lines are taken out of the body and their names kept.
    pub(super) fn new(params: Vec<String>, lines: Vec<(Line, TextId)>) -> Self {
        let mut locals = Vec::new();
        let mut body = Vec::new();
        for (line, text) in lines {
            let tokens: Vec<SyntaxToken> = significant(&line).collect();
            let is_local = tokens.first().is_some_and(|t| {
                t.kind() == kinds::CONTROL && t.text().eq_ignore_ascii_case(".local")
            });
            if is_local {
                locals.extend(
                    tokens
                        .iter()
                        .filter(|t| matches!(t.kind(), kinds::IDENT | kinds::CHEAP_IDENT))
                        .map(|t| t.text().to_owned()),
                );
            } else {
                body.push((line, text));
            }
        }
        Self {
            params,
            locals,
            body,
        }
    }

    /// How many `.local` names the macro has.
    pub(super) fn local_count(&self) -> usize {
        self.locals.len()
    }

    /// The text of a call, with its pieces' origins. `first_local` numbers
    /// the call's first local name, so each call's names are its own.
    pub(super) fn expand(
        &self,
        call: &MacroCall,
        call_text: TextId,
        first_local: usize,
    ) -> ExpansionBuilder {
        let call_site = Span::new(call_text, call.syntax().text_range());
        let args: Vec<Option<(String, Span)>> = call
            .args()
            .map(|arg| {
                let tokens: Vec<SyntaxToken> = arg
                    .tokens()
                    .map(|list| list.contents().collect())
                    .unwrap_or_default();
                let (first, last) = (tokens.first()?, tokens.last()?);
                let range = first.text_range().cover(last.text_range());
                let root = call.syntax().ancestors().last()?;
                Some((
                    range.slice(root.text())?.to_owned(),
                    Span::new(call_text, range),
                ))
            })
            .collect();
        let mut builder = ExpansionBuilder::new();
        for (line, text) in &self.body {
            for token in line
                .syntax()
                .descendants()
                .filter_map(SyntaxElement::into_token)
            {
                let from = Span::new(*text, token.text_range());
                let synthesised = |text: &str, builder: &mut ExpansionBuilder| {
                    builder.push(
                        text,
                        Origin::Synthesised {
                            inputs: from,
                            call_site,
                        },
                    );
                };
                if token.kind() == kinds::IDENT
                    && let Some(index) = self.params.iter().position(|p| p == token.text())
                {
                    if let Some(Some((text, span))) = args.get(index) {
                        builder.push(
                            text,
                            Origin::Expanded {
                                from: *span,
                                call_site,
                            },
                        );
                    }
                } else if matches!(token.kind(), kinds::IDENT | kinds::CHEAP_IDENT)
                    && let Some(index) = self.locals.iter().position(|l| l == token.text())
                {
                    let cheap = if token.kind() == kinds::CHEAP_IDENT {
                        "@"
                    } else {
                        ""
                    };
                    let name = format!("{cheap}LOCAL_MACRO_SYMBOL_{:04X}", first_local + index);
                    synthesised(&name, &mut builder);
                } else if token.kind() == kinds::CONTROL
                    && token.text().eq_ignore_ascii_case(".paramcount")
                {
                    synthesised(&args.len().to_string(), &mut builder);
                } else {
                    builder.push(token.text(), Origin::Expanded { from, call_site });
                }
            }
        }
        builder
    }
}

/// A line's tokens other than whitespace, comments and its line ending.
pub(super) fn significant(line: &Line) -> impl Iterator<Item = SyntaxToken> + use<> {
    line.syntax()
        .descendants()
        .filter_map(SyntaxElement::into_token)
        .filter(|t| {
            !matches!(
                t.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE
            )
        })
}

/// The word of a line's control command, in lower case, if it has one.
pub(super) fn command_word(line: &Line) -> Option<String> {
    match line.statement() {
        Some(Statement::Command(command)) => command.word().map(|w| w.text().to_ascii_lowercase()),
        _ => None,
    }
}

/// The name a statement starts with that could call a macro: the first name
/// after any unnamed label and any `name:` label that is not itself
/// `is_macro`'s. ca65 looks for a macro before a label, so `m:` calls `m`.
pub(super) fn head_name(line: &Line, is_macro: impl Fn(&str) -> bool) -> Option<String> {
    let tokens: Vec<SyntaxToken> = significant(line).collect();
    let mut at = 0;
    if tokens.first().is_some_and(|t| t.kind() == kinds::COLON) {
        at += 1;
    } else if tokens.first().is_some_and(|t| t.kind() == kinds::UNNAMED)
        && tokens.get(1).is_some_and(|t| t.kind() == kinds::COLON)
    {
        at += 2;
    }
    let first = tokens.get(at).filter(|t| t.kind() == kinds::IDENT)?;
    if is_macro(first.text()) {
        return Some(first.text().to_owned());
    }
    if tokens.get(at + 1).is_some_and(|t| t.kind() == kinds::COLON) {
        let next = tokens.get(at + 2).filter(|t| t.kind() == kinds::IDENT)?;
        return Some(next.text().to_owned());
    }
    Some(first.text().to_owned())
}
