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

pub use expr::expression;
pub use instruction::instruction;

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
}
