//! Repeats: `.repeat count[, counter]` up to its `.endrepeat`.
//!
//! This follows ca65 V2.19's `repeat.c`. The body runs to the `.endrepeat`
//! that balances it, found by counting `.repeat` and `.endrepeat` tokens
//! wherever they are, even inside a macro body being defined. The body is
//! not read as it is collected: a macro call in it is just a name, so an
//! `.endrepeat` inside a macro's text does not end it.
//!
//! Each time round, the body's text is made again with the counter replaced
//! by the number of the time round, from 0. The counter is any name token
//! spelt exactly like it, so a counter named `i` also replaces the `i` in an
//! inner `.repeat 2, i`, and `i::x` becomes `0::x`. Strings and characters
//! are not names, so `"i"` stays as it is.
//!
//! A `.endrepeat` after other tokens on its line leaves each round's last
//! line unfinished, since ca65 reads tokens rather than lines. The expander
//! joins that line to the next line it reads, which is the first line of the
//! next round, or after the last round the line after the `.endrepeat`.

use lang198x_syntax::{ExpansionBuilder, Origin, Span, SyntaxElement, SyntaxToken, TextId};

use super::macros::significant;
use crate::ast::{Line, View};
use crate::kinds;

/// A repeat that is running: its body, and how many times are left.
#[derive(Debug, Clone)]
pub(super) struct Repeat {
    /// The body's lines, each with the text it is in.
    body: Vec<(Line, TextId)>,
    /// The tokens before a `.endrepeat` that ends its line's other tokens,
    /// with the text they are in: the unfinished last line of each round.
    tail: Vec<(SyntaxToken, TextId)>,
    /// The counter's name, if it has one.
    counter: Option<String>,
    /// The `.repeat` command, where each time round's text is made.
    site: Span,
    /// The number of the next time round.
    next: u64,
    /// How many times the body runs.
    count: u64,
}

impl Repeat {
    /// A repeat of `body` that runs `count` times.
    pub(super) fn new(
        body: Vec<(Line, TextId)>,
        tail: Vec<(SyntaxToken, TextId)>,
        counter: Option<String>,
        site: Span,
        count: u64,
    ) -> Self {
        Self {
            body,
            tail,
            counter,
            site,
            next: 0,
            count,
        }
    }

    /// The `.repeat` command.
    pub(super) fn site(&self) -> Span {
        self.site
    }

    /// The text of the next time round, with its pieces' origins, or `None`
    /// once the body has run `count` times. A round ends with the tokens
    /// before a mid-line `.endrepeat`, which leave its last line unfinished.
    pub(super) fn next_time(&mut self) -> Option<ExpansionBuilder> {
        if self.next >= self.count || (self.body.is_empty() && self.tail.is_empty()) {
            return None;
        }
        let round = self.next;
        self.next += 1;
        let mut builder = ExpansionBuilder::new();
        for (line, text) in &self.body {
            for token in line
                .syntax()
                .descendants()
                .filter_map(SyntaxElement::into_token)
            {
                self.push(&mut builder, &token, *text, round);
            }
        }
        for (token, text) in &self.tail {
            self.push(&mut builder, token, *text, round);
        }
        Some(builder)
    }

    /// Adds `token`, in `text`, to a round's text, with the counter replaced
    /// by the round's number.
    fn push(&self, builder: &mut ExpansionBuilder, token: &SyntaxToken, text: TextId, round: u64) {
        let from = Span::new(text, token.text_range());
        let call_site = self.site;
        if token.kind() == kinds::IDENT && self.counter.as_deref() == Some(token.text()) {
            builder.push(
                &round.to_string(),
                Origin::Synthesised {
                    inputs: from,
                    call_site,
                },
            );
        } else {
            builder.push(token.text(), Origin::Expanded { from, call_site });
        }
    }
}

/// Where a repeat's body ends.
pub(super) struct End {
    /// The tokens before the `.endrepeat` on its line, if anything but
    /// space comes before it.
    pub(super) before: Vec<SyntaxToken>,
    /// The tokens after the `.endrepeat` on its line.
    pub(super) rest: Vec<SyntaxToken>,
}

/// Looks for the `.endrepeat` that balances a `.repeat`, one line of its
/// body at a time.
#[derive(Debug, Default)]
pub(super) struct Balance {
    depth: usize,
}

impl Balance {
    /// Where the body ends, if it ends on `line`.
    pub(super) fn ends_on(&mut self, line: &Line) -> Option<End> {
        let tokens: Vec<SyntaxToken> = line
            .syntax()
            .descendants()
            .filter_map(SyntaxElement::into_token)
            .collect();
        for (at, token) in tokens.iter().enumerate() {
            if token.kind() != kinds::CONTROL {
                continue;
            }
            if is_repeat(token.text()) {
                self.depth += 1;
            } else if is_end(token.text()) {
                if self.depth == 0 {
                    let first = significant(line).next();
                    let mid_line = first.is_some_and(|f| f.text_range() != token.text_range());
                    return Some(End {
                        before: if mid_line {
                            tokens[..at].to_vec()
                        } else {
                            Vec::new()
                        },
                        rest: tokens[at + 1..].to_vec(),
                    });
                }
                self.depth -= 1;
            }
        }
        None
    }
}

/// Whether `word` is `.repeat`.
pub(super) fn is_repeat(word: &str) -> bool {
    word.eq_ignore_ascii_case(".repeat")
}

/// Whether `word` is `.endrepeat` or its short form, `.endrep`.
pub(super) fn is_end(word: &str) -> bool {
    word.eq_ignore_ascii_case(".endrepeat") || word.eq_ignore_ascii_case(".endrep")
}
