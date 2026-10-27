//! Lowering to `asm198x-ir`, the contract with the Asm198x back end.
//!
//! This module is built only with the `ir` feature. Lowering gives each line
//! the meaning ca65 V2.19 gives it, measured by running ca65, so the back end
//! assembles the bytes ca65 and ld65 write.
//!
//! Names stay names. A symbol becomes [`Expr::Sym`](asm198x_ir::Expr::Sym)
//! with its name as written, such as `x`, `@near`, `outer::x` or `::x`, and
//! an unnamed label's reference keeps its spelling, such as `:+` or `:--`.
//! The assembler resolves them, since only it keeps scopes. What ca65 needs
//! as soon as it reads a line, such as whether a name is known to be on the
//! zero page, [`Known`] answers.

mod expr;
mod instruction;
mod line;

pub use expr::expression;
pub use instruction::instruction;
pub use line::{Item, Lowered, Scope, Segment, line};

/// The address sizes ca65 gives a value: how many bytes an address needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Size {
    /// One byte: the zero page.
    Zero,
    /// Two bytes.
    Absolute,
    /// Three bytes.
    Far,
}

impl Size {
    /// The size a constant needs: the zero page up to `$FF`, two bytes up to
    /// `$FFFF`, and three beyond. ca65 gives a negative constant the zero
    /// page, where its range check then refuses it.
    #[must_use]
    pub fn of(value: i64) -> Self {
        match value {
            ..=0xFF => Self::Zero,
            0x100..=0xFFFF => Self::Absolute,
            _ => Self::Far,
        }
    }
}

/// What the assembler knows of a name, or of the address, at the line being
/// lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Value {
    /// A constant, such as `q = $12`, or a label under `.org`.
    Constant(i64),
    /// An address whose value the linker gives, such as a label in a
    /// segment, with the segment's address size.
    Address(Size),
    /// Nothing yet: a name defined further on. ca65 takes it as absolute.
    Unknown,
}

/// What lowering asks the assembler about the program so far.
pub trait Known {
    /// What is known of `name`, as written in the source.
    fn name(&self, name: &str) -> Value;

    /// What is known of the address of the line, `*`.
    fn pc(&self) -> Value;

    /// The size of `name`, as written, for `.sizeof`: the bytes a label's
    /// line made, or the bytes of a closed scope. A scope of that name comes
    /// before a symbol. `None`, the default, if it has no size yet.
    fn size_of(&self, name: &str) -> Option<i64> {
        let _ = name;
        None
    }
}

/// What is known of `expr` now: its value if every name in it is a
/// constant, and otherwise the address size ca65 gives it. Each name takes
/// its own size, a name not known yet is absolute, and `<`, `>` and `^`
/// make one byte.
///
/// An assembler can use this for the value of `name = expr`.
#[must_use]
pub fn value(expr: &asm198x_ir::Expr, known: &dyn Known) -> Value {
    match expr::constant(expr, known) {
        Some(value) => Value::Constant(value),
        None => Value::Address(expr::size(expr, known)),
    }
}
