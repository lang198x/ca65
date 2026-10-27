//! The stack of open conditionals, as ca65 V2.19's `condasm.c` keeps it.
//!
//! Each level records its own condition, its parent's, whether it has had an
//! `.else`, and whether it needs an `.endif`. A `.elseif` turns the level it
//! is in over, as `.else` does, then opens a level of its own that needs no
//! `.endif`: the `.endif` closes the whole chain. So `.if 1`, `.elseif 1`
//! runs only the first branch, and two `.elseif`s after `.if 0` run the first
//! that is true.

use lang198x_syntax::Span;

/// One open conditional.
#[derive(Debug, Clone, Copy)]
struct Level {
    /// Whether this level's current branch is taken, were the lines around
    /// it to run.
    cond: bool,
    /// Whether every level around this one is taken.
    parent: bool,
    /// Whether this level has had an `.else` or `.elseif`.
    had_else: bool,
    /// Whether an `.endif` closes this level, rather than the one below it.
    needs_end: bool,
    /// The command that opened the level.
    site: Span,
}

/// Why a conditional command did not fit the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Misfit {
    /// `.else`, `.elseif` or `.endif` with no `.if` open.
    Unexpected,
    /// A second `.else` for the same `.if`.
    DuplicateElse,
}

/// The open conditionals, innermost last.
#[derive(Debug, Clone, Default)]
pub(super) struct Conditionals {
    levels: Vec<Level>,
}

impl Conditionals {
    /// Whether lines are being run: every open level is taken.
    pub(super) fn active(&self) -> bool {
        self.levels.last().is_none_or(|l| l.cond && l.parent)
    }

    /// Opens a level, `.if` or one of its kin, at `site`. Its branch runs if
    /// `cond` holds and the lines around it run.
    pub(super) fn open(&mut self, site: Span, cond: bool) {
        self.push(site, cond, true);
    }

    /// Opens the level a `.elseif` makes, after [`otherwise`](Self::otherwise),
    /// which the chain's `.endif` closes.
    pub(super) fn open_chained(&mut self, site: Span, cond: bool) {
        self.push(site, cond, false);
    }

    fn push(&mut self, site: Span, cond: bool, needs_end: bool) {
        let parent = self.active();
        self.levels.push(Level {
            cond,
            parent,
            had_else: false,
            needs_end,
            site,
        });
    }

    /// `.else`, or the first half of `.elseif`: turns the innermost level
    /// over.
    pub(super) fn otherwise(&mut self) -> Result<(), Misfit> {
        let level = self.levels.last_mut().ok_or(Misfit::Unexpected)?;
        if level.had_else {
            return Err(Misfit::DuplicateElse);
        }
        level.had_else = true;
        level.cond = !level.cond;
        Ok(())
    }

    /// How many levels are open.
    pub(super) fn depth(&self) -> usize {
        self.levels.len()
    }

    /// Closes the levels opened after the first `depth`, as `.exitmacro`
    /// does for those its macro opened.
    pub(super) fn truncate(&mut self, depth: usize) {
        self.levels.truncate(depth);
    }

    /// `.endif`: closes the innermost chain.
    pub(super) fn end(&mut self) -> Result<(), Misfit> {
        loop {
            let level = self.levels.pop().ok_or(Misfit::Unexpected)?;
            if level.needs_end {
                return Ok(());
            }
        }
    }

    /// Closes every level still open, returning where each `.if` chain left
    /// unclosed was opened, outermost first.
    pub(super) fn close_all(&mut self) -> Vec<Span> {
        let sites = self
            .levels
            .iter()
            .filter(|l| l.needs_end)
            .map(|l| l.site)
            .collect();
        self.levels.clear();
        sites
    }
}

#[cfg(test)]
mod tests {
    use lang198x_syntax::{FileId, TextId, TextRange, TextSize};

    use super::*;

    fn at(offset: u32) -> Span {
        Span::new(
            TextId::File(FileId::new(0)),
            TextRange::empty(TextSize::new(offset)),
        )
    }

    #[test]
    fn elseif_chains_take_the_first_true_branch() {
        // `.if 0`, `.elseif 0`, `.elseif 1`, `.else`: only the third branch
        // runs.
        let mut stack = Conditionals::default();
        stack.open(at(0), false);
        assert!(!stack.active());
        assert_eq!(stack.otherwise(), Ok(()));
        stack.open_chained(at(1), false);
        assert!(!stack.active());
        assert_eq!(stack.otherwise(), Ok(()));
        stack.open_chained(at(2), true);
        assert!(stack.active());
        assert_eq!(stack.otherwise(), Ok(()));
        assert!(!stack.active());
        assert_eq!(stack.end(), Ok(()));
        assert!(stack.active());
        assert!(stack.close_all().is_empty());
    }

    #[test]
    fn an_inner_level_of_a_skipped_branch_is_never_taken() {
        let mut stack = Conditionals::default();
        stack.open(at(0), false);
        stack.open(at(1), true);
        assert!(!stack.active());
        assert_eq!(stack.otherwise(), Ok(()));
        assert!(!stack.active());
    }

    #[test]
    fn misfits() {
        let mut stack = Conditionals::default();
        assert_eq!(stack.otherwise(), Err(Misfit::Unexpected));
        assert_eq!(stack.end(), Err(Misfit::Unexpected));
        stack.open(at(3), true);
        assert_eq!(stack.otherwise(), Ok(()));
        assert_eq!(stack.otherwise(), Err(Misfit::DuplicateElse));
        stack.open(at(5), true);
        stack.open_chained(at(7), true);
        assert_eq!(stack.depth(), 3);
        stack.truncate(2);
        assert_eq!(stack.depth(), 2);
        assert_eq!(stack.close_all(), [at(3), at(5)]);
    }
}
