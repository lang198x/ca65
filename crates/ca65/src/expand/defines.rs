//! Define-style macros: `.define name[(params)] body`, and their uses.
//!
//! This follows ca65 V2.19's `MacDef`, `StartExpDefine` and `NextRawTok` in
//! `macro.c` and `scanner.c`. ca65 expands a define-style macro in its
//! scanner, as each token is read, so a use is expanded wherever it is: in
//! a line that runs, in a skipped branch, in a classic macro's body or a
//! repeat's body as it is collected, and in a define's own body as it is
//! defined. The one exception is the name just after `.define` or `.undef`,
//! which would otherwise never be seen.
//!
//! A use takes exactly as many arguments as the define has parameters. Each
//! runs to the next comma or the end of the line, unless it is in braces,
//! which are removed. Parentheses are only tokens: in `F(2)`, the argument
//! is `(2)`, which gives the same value. The text a use makes is read again,
//! so a define's body may use defines made after it.

use std::collections::HashMap;

use lang198x_syntax::{
    DiagnosticKind, Origin, Span, SyntaxElement, SyntaxKind, SyntaxToken, TextId,
};

use crate::ast::{Command, Line, View};
use crate::kinds;

/// A token on its way into a line's new text, with where it came from.
#[derive(Debug, Clone)]
pub(super) struct Piece {
    pub(super) text: String,
    pub(super) kind: SyntaxKind,
    pub(super) origin: Origin,
    /// Where the piece's text lies, or what it was made from.
    pub(super) site: Span,
}

impl Piece {
    pub(super) fn new(token: &SyntaxToken, origin: Origin, site: Span) -> Self {
        Self {
            text: token.text().to_owned(),
            kind: token.kind(),
            origin,
            site,
        }
    }

    pub(super) fn is_blank(&self) -> bool {
        self.kind == SyntaxKind::WHITESPACE
    }

    /// Whether the piece ends an argument: the end of the line.
    pub(super) fn ends_line(&self) -> bool {
        matches!(self.kind, SyntaxKind::NEWLINE | SyntaxKind::COMMENT)
    }
}

/// A define-style macro, as defined.
#[derive(Debug, Clone)]
pub(super) struct Define {
    params: Vec<String>,
    /// The body's tokens, with where each one is.
    body: Vec<(SyntaxToken, Span)>,
}

impl Define {
    /// The define a `.define` command makes, in `text`, or a message saying
    /// why it makes none. A command with no name makes none, and the parser
    /// has said why.
    pub(super) fn from_command(
        command: &Command,
        text: TextId,
    ) -> Result<Option<(String, Self)>, String> {
        let Some(name) = command.name().map(|n| n.text()) else {
            return Ok(None);
        };
        let mut params: Vec<String> = Vec::new();
        for param in command.params().iter().flat_map(|p| p.names()) {
            let param = param.text().to_owned();
            if params.contains(&param) {
                return Err(format!("`{name}` has two parameters named `{param}`"));
            }
            params.push(param);
        }
        let mut body: Vec<(SyntaxToken, Span)> = command
            .tokens()
            .iter()
            .flat_map(|list| list.syntax().descendants())
            .filter_map(SyntaxElement::into_token)
            .map(|t| {
                let span = Span::new(text, t.text_range());
                (t, span)
            })
            .collect();
        while body
            .last()
            .is_some_and(|(t, _)| t.kind() == SyntaxKind::WHITESPACE)
        {
            body.pop();
        }
        let uses_itself = body.iter().any(|(t, _)| {
            t.kind() == kinds::IDENT && t.text() == name && !params.iter().any(|p| p == &name)
        });
        if uses_itself {
            return Err(format!(
                "`{name}` uses itself, and is not defined until its line ends"
            ));
        }
        Ok(Some((name, Self { params, body })))
    }
}

/// Why a line could not be expanded.
pub(super) struct Problem {
    pub(super) site: Span,
    pub(super) message: String,
    pub(super) kind: DiagnosticKind,
}

impl Problem {
    pub(super) fn invalid(site: Span, message: String) -> Self {
        Self {
            site,
            message,
            kind: DiagnosticKind::Invalid,
        }
    }
}

/// The pieces of `line`, in `text`, with every use of a define in force
/// expanded, or `None` if it uses none. `limit` bounds the bytes made.
pub(super) fn expand(
    defines: &HashMap<String, Define>,
    line: &Line,
    text: TextId,
    limit: usize,
) -> Result<Option<Vec<Piece>>, Problem> {
    let tokens: Vec<SyntaxToken> = line
        .syntax()
        .descendants()
        .filter_map(SyntaxElement::into_token)
        .collect();
    if !tokens
        .iter()
        .any(|t| t.kind() == kinds::IDENT && defines.contains_key(t.text()))
    {
        return Ok(None);
    }
    let base: Vec<Piece> = tokens
        .iter()
        .map(|t| {
            let from = Span::new(text, t.text_range());
            Piece::new(t, Origin::Source { from }, from)
        })
        .collect();
    let mut stream = Stream {
        defines,
        stack: vec![base.into_iter()],
        made: 0,
        limit,
    };
    let mut out = Vec::new();
    let mut naming = false;
    while let Some(piece) = stream.next(naming)? {
        if !piece.is_blank() {
            naming = piece.kind == kinds::CONTROL && names_a_define(&piece.text);
        }
        out.push(piece);
    }
    Ok(Some(out))
}

/// Whether the name after `word` is a define's own name, not a use.
fn names_a_define(word: &str) -> bool {
    [".define", ".undef", ".undefine"]
        .iter()
        .any(|w| w.eq_ignore_ascii_case(word))
}

/// The tokens still to read: the line's own, under the text of each use
/// being read, innermost last.
struct Stream<'d> {
    defines: &'d HashMap<String, Define>,
    stack: Vec<std::vec::IntoIter<Piece>>,
    made: usize,
    limit: usize,
}

impl Stream<'_> {
    fn next_raw(&mut self) -> Option<Piece> {
        while let Some(top) = self.stack.last_mut() {
            if let Some(piece) = top.next() {
                return Some(piece);
            }
            self.stack.pop();
        }
        None
    }

    fn unread(&mut self, piece: Piece) {
        self.stack.push(vec![piece].into_iter());
    }

    /// The next piece, with any use of a define expanded first, unless
    /// `naming`, when the next name is a define's own.
    fn next(&mut self, naming: bool) -> Result<Option<Piece>, Problem> {
        loop {
            let Some(piece) = self.next_raw() else {
                return Ok(None);
            };
            if piece.kind == kinds::IDENT
                && !naming
                && let Some(define) = self.defines.get(&piece.text)
            {
                self.call(&piece, define)?;
                continue;
            }
            return Ok(Some(piece));
        }
    }

    /// The next piece that is not blank.
    fn next_solid(&mut self) -> Result<Option<Piece>, Problem> {
        loop {
            match self.next(false)? {
                Some(piece) if piece.is_blank() => {}
                other => return Ok(other),
            }
        }
    }

    /// A use of `define`, named by `name`: its arguments are read, and its
    /// text is read next.
    fn call(&mut self, name: &Piece, define: &Define) -> Result<(), Problem> {
        let site = name.site;
        let problem = |message: String| Problem::invalid(site, message);
        let mut args: Vec<Vec<Piece>> = Vec::new();
        for number in 1..=define.params.len() {
            let mut piece = self.next_solid()?;
            let braced = piece.as_ref().is_some_and(|p| p.kind == kinds::L_CURLY);
            if braced {
                piece = self.next_solid()?;
            }
            let ends = |p: &Piece| {
                p.ends_line() || p.kind == if braced { kinds::R_CURLY } else { kinds::COMMA }
            };
            let mut arg = Vec::new();
            let after = loop {
                match piece {
                    Some(p) if !ends(&p) => {
                        arg.push(p);
                        piece = self.next(false)?;
                    }
                    other => break other,
                }
            };
            while arg.last().is_some_and(Piece::is_blank) {
                arg.pop();
            }
            if arg.is_empty() {
                return Err(problem(format!(
                    "argument {number} of `{}` is empty",
                    name.text
                )));
            }
            args.push(arg);
            let mut after = after;
            if braced {
                if after.as_ref().is_none_or(Piece::ends_line) {
                    return Err(problem(format!(
                        "argument {number} of `{}` has no closing `}}`",
                        name.text
                    )));
                }
                after = if number < define.params.len() {
                    self.next_solid()?
                } else {
                    None
                };
            }
            if number < define.params.len() {
                if after.as_ref().is_none_or(|p| p.kind != kinds::COMMA) {
                    return Err(problem(format!(
                        "`{}` takes {} arguments",
                        name.text,
                        define.params.len()
                    )));
                }
            } else if let Some(after) = after {
                self.unread(after);
            }
        }
        let gap = Piece {
            text: " ".to_owned(),
            kind: SyntaxKind::WHITESPACE,
            origin: Origin::Synthesised {
                inputs: site,
                call_site: site,
            },
            site,
        };
        let mut text = vec![gap.clone()];
        for (token, from) in &define.body {
            if token.kind() == kinds::IDENT
                && let Some(index) = define.params.iter().position(|p| p == token.text())
            {
                text.extend(args[index].iter().cloned());
            } else {
                text.push(Piece::new(
                    token,
                    Origin::Expanded {
                        from: *from,
                        call_site: site,
                    },
                    *from,
                ));
            }
        }
        text.push(gap);
        self.made += text.iter().map(|p| p.text.len()).sum::<usize>();
        if self.made > self.limit {
            return Err(problem(
                "the program's defines make too much text".to_owned(),
            ));
        }
        self.stack.push(text.into_iter());
        Ok(())
    }
}
