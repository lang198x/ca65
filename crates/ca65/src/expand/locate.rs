//! Where a span came from, in the files a person wrote.

use lang198x_syntax::{FileId, Span, TextId, TextRange, TextSize};

use super::Expander;

/// A place in a file a person wrote: the root, or a file an `.include` or
/// `.macpack` read in, which [`Expander::files`] lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Location {
    /// The file. The root is 0.
    pub file: FileId,
    /// The line, counting from 1.
    pub line: u32,
    /// The column, counting from 1, in bytes.
    pub column: u32,
}

/// The offset of each line's start in `text`.
pub(super) fn line_starts(text: &str) -> Vec<TextSize> {
    let mut starts = vec![TextSize::ZERO];
    starts.extend(
        text.match_indices('\n')
            .filter_map(|(at, _)| u32::try_from(at + 1).ok().map(TextSize::new)),
    );
    starts
}

impl Expander {
    /// Returns the outermost macro call that made `span`, as a span in the
    /// text the call is in, or `None` if no macro call made it. A repeat's
    /// lines, and the text a define or a token function makes, are not a
    /// call's. ca65's own debug information gives the lines a macro makes
    /// to the line that called it, and a repeat's lines to themselves.
    ///
    /// ```
    /// use lang198x_ca65::ast::{Expr, NameRef};
    /// use lang198x_ca65::expand::{Expander, Host};
    /// use lang198x_ca65::Profile;
    ///
    /// struct One;
    ///
    /// impl Host for One {
    ///     fn evaluate(&mut self, _: &Expr) -> Result<i64, String> {
    ///         Ok(1)
    ///     }
    ///     fn is_defined(&mut self, _: &NameRef) -> bool {
    ///         false
    ///     }
    ///     fn is_referenced(&mut self, _: &NameRef) -> bool {
    ///         false
    ///     }
    ///     fn is_constant(&mut self, _: &Expr) -> bool {
    ///         true
    ///     }
    /// }
    ///
    /// let source = ".macro m\n  nop\n.endmacro\nm\n.repeat 1\n  nop\n.endrepeat\n";
    /// let mut expander = Expander::new(source, Profile::CA65_2_19_6502)?;
    /// let line = expander.next_line(&mut One).expect("the macro's line");
    /// let call = expander.macro_call(line.span()).expect("a call");
    /// assert_eq!(expander.locate(call).map(|at| at.line), Some(4));
    /// let line = expander.next_line(&mut One).expect("the repeat's line");
    /// assert_eq!(expander.macro_call(line.span()), None);
    /// # Ok::<(), lang198x_ca65::ParseError>(())
    /// ```
    #[must_use]
    pub fn macro_call(&self, span: Span) -> Option<Span> {
        let mut call = None;
        let mut at = span;
        while let TextId::Expansion(id) = at.text() {
            let origin = self.provenance().get(id)?.origin_at(at.range().start())?;
            at = match (origin.site(), origin.copied_from()) {
                (Some(site), _) => {
                    if self.macro_texts.contains(&id) {
                        call = Some(site);
                    }
                    site
                }
                (None, Some(from)) => from,
                (None, None) => break,
            };
        }
        call
    }

    /// Returns where `span` starts in a file a person wrote, following it
    /// back through includes, macros, repeats, define-style macros and
    /// token functions. Text a token function made traces to the call.
    /// Returns `None` for a span the expander did not make.
    ///
    /// ```
    /// use lang198x_ca65::ast::{Expr, NameRef, View};
    /// use lang198x_ca65::expand::{Expander, Host};
    /// use lang198x_ca65::syntax::FileId;
    /// use lang198x_ca65::Profile;
    ///
    /// struct Nothing;
    ///
    /// impl Host for Nothing {
    ///     fn evaluate(&mut self, _: &Expr) -> Result<i64, String> {
    ///         Ok(0)
    ///     }
    ///     fn is_defined(&mut self, _: &NameRef) -> bool {
    ///         false
    ///     }
    ///     fn is_referenced(&mut self, _: &NameRef) -> bool {
    ///         false
    ///     }
    ///     fn is_constant(&mut self, _: &Expr) -> bool {
    ///         true
    ///     }
    /// }
    ///
    /// let source = ".macro m\n  nop\n.endmacro\nm\n";
    /// let mut expander = Expander::new(source, Profile::CA65_2_19_6502)?;
    /// let line = expander.next_line(&mut Nothing).expect("the macro's line");
    /// let at = expander.locate(line.span()).expect("a place");
    /// assert_eq!((at.file, at.line, at.column), (FileId::new(0), 2, 1));
    /// # Ok::<(), lang198x_ca65::ParseError>(())
    /// ```
    #[must_use]
    pub fn locate(&self, span: Span) -> Option<Location> {
        let start = Span::new(span.text(), TextRange::empty(span.range().start()));
        let traced = match self.provenance().trace(start) {
            Some(traced) => traced,
            None => self.made_from(start)?,
        };
        let TextId::File(file) = traced.text() else {
            return None;
        };
        let starts = self.line_starts.get(usize::try_from(file.get()).ok()?)?;
        let offset = traced.range().start();
        let line = starts.partition_point(|&s| s <= offset);
        let line_start = starts.get(line.checked_sub(1)?)?;
        Some(Location {
            file,
            line: u32::try_from(line).ok()?,
            column: offset.checked_sub(*line_start)?.get() + 1,
        })
    }

    /// Where made-up text came from: the inputs it was made from, traced to
    /// a file.
    fn made_from(&self, span: Span) -> Option<Span> {
        let TextId::Expansion(id) = span.text() else {
            return None;
        };
        let origin = self.provenance().get(id)?.origin_at(span.range().start())?;
        let inputs = origin.made_from()?;
        let start = Span::new(inputs.text(), TextRange::empty(inputs.range().start()));
        self.provenance()
            .trace(start)
            .or_else(|| self.made_from(start))
    }
}

#[cfg(test)]
mod tests;
