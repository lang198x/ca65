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
mod record;
mod settings;

pub use expr::expression;
pub use instruction::{instruction, sets};
pub use line::{
    Binary, Declared, Item, Lowered, Scope, Segment, Visibility, Visible, condition, line,
};
pub use record::{Record, RecordKind};
pub use settings::{Charmap, CpuStack, Dbg};

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
    /// its address size. ca65 adds, takes away and multiplies these as it
    /// does labels, so `RED - RED` and `RED * 0` are constants.
    Address(Size),
    /// A value ca65 cannot work out until the link, and cannot fold with
    /// anything else, known only by its address size: a name not defined
    /// yet whose spelling is defined further out, or a value made with an
    /// operation the linker works out, such as `lab & $FF`.
    Opaque(Size),
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

    /// Whether `name`, as written, had been used by the time `finished`
    /// token functions on the line had run, for a `.referenced` that ca65
    /// reads after them. A use a later function makes does not count. Only
    /// an assembler that keeps the uses functions make aside, as
    /// [`Host::evaluate_in_function`](crate::expand::Host::evaluate_in_function)
    /// allows, can tell them apart. The default is
    /// [`is_referenced`](Self::is_referenced).
    fn is_referenced_before(&self, name: &str, finished: usize) -> bool {
        let _ = finished;
        self.is_referenced(name)
    }

    /// How many token functions had run before the `.referenced` that
    /// starts at `at` in the line's text, if lowering knows. Lowering
    /// answers this itself, from the line's
    /// [`Finished`](crate::expand::Finished) table, so an assembler leaves
    /// the default, which is `None`.
    fn finished_before(&self, at: lang198x_syntax::TextSize) -> Option<usize> {
        let _ = at;
        None
    }

    /// The value to use for ca65's `.bank(value)`: the `bank` attribute of
    /// the memory area that `value`'s segment runs in. Only the linker can
    /// tell, so an assembler that links answers with an expression of its
    /// own, which it resolves once segments are placed. ca65 gives the
    /// result the absolute address size and never counts it as a constant,
    /// so the expression should be known by that size alone. The default,
    /// `None`, leaves `.bank` not lowered.
    fn bank(&self, value: &asm198x_ir::Expr) -> Option<asm198x_ir::Expr> {
        let _ = value;
        None
    }

    /// The address size `name`, as written, was declared with, if it was,
    /// such as by `.proc p: far`. `.addrsize` gives this before the size of
    /// the name's value, since ca65 keeps the two apart: in the scope that
    /// defines `p`, `lda p` is sized by its value, but `.addrsize(p)` is 3.
    /// The default is `None`.
    fn address_size(&self, name: &str) -> Option<Size> {
        let _ = name;
        None
    }

    /// Told of each name lowering reads as a value, in the order ca65 reads
    /// them, which ca65 counts as a use. A name in `.defined`,
    /// `.referenced` or `.sizeof` is not read, but one in `.const` is. So
    /// `.byte .ref(v), v, .ref(v)` gives 0 and then 1, if the assembler
    /// counts the use at once. The default does nothing.
    fn read(&self, name: &str) {
        let _ = name;
    }

    /// The 65816's register widths in force on the line, which size an
    /// immediate and give `.asize` and `.isize`. Lowering a line answers
    /// this from the line; the default is 8 bits each.
    fn widths(&self) -> crate::Widths {
        crate::Widths::default()
    }

    /// The byte ca65's character map makes of `character`, for the
    /// characters of strings and character constants. The assembler keeps
    /// the map, from each line's [`Charmap`] change. The default leaves
    /// every character as it is.
    fn translate(&self, character: u8) -> u8 {
        character
    }

    /// The CPU in force on the line, which `.cpu` reports. Lowering a line
    /// answers this from the line; the default is the 6502.
    fn cpu(&self) -> crate::Cpu {
        crate::Cpu::Mos6502
    }

    /// Whether `feature` is on for the line, such as `loose_char_term`,
    /// which lets a one-byte string be a character. Lowering a line answers
    /// this from the line; by default every feature is off.
    fn has_feature(&self, feature: crate::Feature) -> bool {
        let _ = feature;
        false
    }

    /// Whether `.smart` is on for the line. Lowering a line answers this
    /// from the line; the default is off.
    fn smart(&self) -> bool {
        false
    }

    /// Whether the innermost scope open on the line is far, as
    /// `.proc p: far` opens. With the `long_jsr_jmp_rts` feature and
    /// `.smart`, ca65 makes `rts` there the 65816's `rtl`. The assembler
    /// follows scopes; the default is no.
    fn in_far_scope(&self) -> bool {
        false
    }

    /// The bytes a string `token` on the line holds, with which of them
    /// are raw, which the character map leaves alone. Lowering a line
    /// answers this from the line, which keeps the bytes of a string whose
    /// escapes change them; by default, `None`, and the text between the
    /// quotes is used.
    fn string(&self, token: &lang198x_syntax::SyntaxToken) -> Option<(Vec<u8>, Vec<bool>)> {
        let _ = token;
        None
    }

    /// The time ca65's `.time` gives: seconds since the Unix epoch, when the
    /// assembler reads the line. The default is the system clock; an
    /// assembler that wants repeatable output can give a fixed time.
    fn time(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
            })
    }
}

/// The address size ca65 V2.19 gives `expr`, as its `StudyExpr` works it
/// out. A value whose names are all constants takes the size its value
/// needs. Otherwise a number takes the size its value needs and a name its
/// own; `<`, `>`, `^`, a comparison and the logical operators make one
/// byte; `&` takes the smaller of its two sides; and every other operation
/// the larger.
///
/// ca65 sizes an instruction's operand by this as it reads the line. It
/// also refuses data whose value is not known until the link if this size
/// is bigger than the room the data takes, so `.byte lab` is an error for
/// a label in a segment whose address is two bytes. An assembler can check
/// that once every name is known.
#[must_use]
pub fn size(expr: &asm198x_ir::Expr, known: &dyn Known) -> Size {
    expr::size(expr, known)
}

/// Whether ca65 V2.19 gives `expr` an address size of its own as it reads
/// it, rather than the default a name not known yet gives: `<RED` and
/// `RED + 1` for a constant or label `RED` have one, `RED + 1` for a name
/// defined further on does not. ca65 checks an equate for a circular
/// definition at the end only if its value had the default.
#[must_use]
pub fn has_size(expr: &asm198x_ir::Expr, known: &dyn Known) -> bool {
    expr::has_size(expr, known)
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
        Some(sum) => match sum.terms.as_slice() {
            [(expr::Term::Segment(segment), 1)] => Value::Placed {
                segment: *segment,
                offset: sum.value,
                size,
            },
            _ => Value::Address(size),
        },
        None => Value::Opaque(size),
    }
}
