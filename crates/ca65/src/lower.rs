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
mod settings;

pub use expr::expression;
pub use instruction::instruction;
pub use line::{Binary, Declared, Item, Lowered, Scope, Segment, Visibility, Visible, line};
pub use settings::CpuStack;

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
    /// An address whose value the linker gives, such as an import, with
    /// its address size.
    Address(Size),
    /// A label the linker places, `offset` bytes into the segment the
    /// assembler numbers `segment`, which has address size `size`. Two of
    /// these in one segment are a known distance apart, so `r - l` is a
    /// constant once both are defined.
    Placed {
        /// The segment, numbered however the assembler likes.
        segment: u32,
        /// How far into the segment the label is.
        offset: i64,
        /// The segment's address size.
        size: Size,
    },
    /// A constant made from labels the linker places, such as `r - l` for
    /// two labels in one segment. ca65 knows its value but gives it the
    /// labels' address size, so `lda r - l` is absolute.
    Measured {
        /// The value.
        value: i64,
        /// The address size of the labels it came from.
        size: Size,
    },
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

    /// Whether `name`, as written, is defined so far, for `.defined`. The
    /// default is no.
    fn is_defined(&self, name: &str) -> bool {
        let _ = name;
        false
    }

    /// Whether `name`, as written, has been used so far, for `.referenced`.
    /// The default is no.
    fn is_referenced(&self, name: &str) -> bool {
        let _ = name;
        false
    }

    /// Told of each name lowering reads as a value, in the order ca65 reads
    /// them, which ca65 counts as a use. A name in `.defined`,
    /// `.referenced` or `.sizeof` is not read, but one in `.const` is. So
    /// `.byte .ref(v), v, .ref(v)` gives 0 and then 1, if the assembler
    /// counts the use at once. The default does nothing.
    fn read(&self, name: &str) {
        let _ = name;
    }
}

/// What is known of `expr` now. Its value if every name in it is a
/// constant. If it is made from labels in one segment, such as `r - l` or
/// `l + 1`, what [`Value::Measured`] or [`Value::Placed`] says. Otherwise
/// the address size ca65 gives it: each name takes its own size, a name not
/// known yet is absolute, and `<`, `>` and `^` make one byte.
///
/// An assembler can use this for the value of `name = expr`.
#[must_use]
pub fn value(expr: &asm198x_ir::Expr, known: &dyn Known) -> Value {
    if let Some(value) = expr::plain(expr, known) {
        return Value::Constant(value);
    }
    let size = expr::size(expr, known);
    match expr::linear(expr, known) {
        Some(sum) if sum.terms.is_empty() => Value::Measured {
            value: sum.value,
            size,
        },
        Some(sum) if matches!(sum.terms.as_slice(), [(_, 1)]) => Value::Placed {
            segment: sum.terms[0].0,
            offset: sum.value,
            size,
        },
        _ => Value::Address(size),
    }
}
