//! Expressions, lowered to IR expressions, and the address size ca65 gives
//! each one.

use asm198x_ir::{BinOp, Expr as Ir};
use lang198x_syntax::{Diagnostic, DiagnosticKind, SyntaxToken, TextRange};

use super::{Known, Size, Value};
use crate::ast::{CallExpr, Expr, View};
use crate::kinds;

/// Lowers `expr` to an IR expression with the value ca65 gives it. Names stay
/// names, as written, and each one is [read](Known::read). `known` answers
/// what ca65 needs now: the size of a name in `.sizeof`, and what `.defined`,
/// `.referenced` and `.const` ask.
///
/// ```
/// use lang198x_ca65::ast::{SourceFile, Statement, View};
/// use lang198x_ca65::lower::{Known, Value};
/// use lang198x_ca65::{Profile, lower, parse};
///
/// struct Nothing;
///
/// impl Known for Nothing {
///     fn name(&self, _: &str) -> Value {
///         Value::Unknown
///     }
///     fn pc(&self) -> Value {
///         Value::Unknown
///     }
/// }
///
/// let parse = parse(".byte <(table + 2)\n", Profile::CA65_2_19_6502)?;
/// let line = SourceFile::cast(parse.tree().root()).unwrap().lines().next().unwrap();
/// let Some(Statement::Command(command)) = line.statement() else { panic!() };
/// let value = lower::expression(&command.values().next().unwrap(), &Nothing).expect("lowers");
/// let symbols = |name: &str| (name == "table").then_some(0x12FF);
/// assert_eq!(value.eval_with(&symbols, None, 0).ok(), Some(0x01));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
///
/// A diagnostic, with its range in `expr`'s tree, for a part ca65 refuses,
/// or one not lowered yet, which is
/// [`Unsupported`](DiagnosticKind::Unsupported).
pub fn expression(expr: &Expr, known: &dyn Known) -> Result<Ir, Diagnostic> {
    lower(expr, known)
}

fn lower(expr: &Expr, known: &dyn Known) -> Result<Ir, Diagnostic> {
    let range = expr.syntax().text_range();
    let missing = || invalid(range, "expected a value");
    Ok(match expr {
        Expr::Literal(literal) => {
            let token = literal.token().ok_or_else(missing)?;
            literal_value(&token)?
        }
        Expr::Name(name) => {
            let name = name.text();
            known.read(&name);
            Ir::Sym(name)
        }
        Expr::Unnamed(unnamed) => Ir::Sym(
            unnamed
                .token()
                .map(|t| t.text().to_owned())
                .ok_or_else(missing)?,
        ),
        Expr::Paren(paren) => lower(&paren.expr().ok_or_else(missing)?, known)?,
        Expr::Unary(unary) => {
            let operand = Box::new(lower(&unary.operand().ok_or_else(missing)?, known)?);
            let op = unary
                .op()
                .map(|t| t.text().to_ascii_lowercase())
                .unwrap_or_default();
            match op.as_str() {
                "+" => *operand,
                "-" => Ir::Neg(operand),
                "~" | ".bitnot" => Ir::BitNot(operand),
                "<" => Ir::Lo(operand),
                ">" => Ir::Hi(operand),
                "^" => Ir::Bank(operand),
                "!" | ".not" => Ir::LogNot(operand),
                other => return Err(unsupported(range, &format!("the prefix `{other}`"))),
            }
        }
        Expr::Binary(binary) => {
            let lhs = Box::new(lower(&binary.lhs().ok_or_else(missing)?, known)?);
            let rhs = Box::new(lower(&binary.rhs().ok_or_else(missing)?, known)?);
            let text = binary
                .op()
                .map(|t| t.text().to_ascii_lowercase())
                .unwrap_or_default();
            let op = match text.as_str() {
                "||" | ".or" => BinOp::LogOr,
                "&&" | ".and" => BinOp::LogAnd,
                ".xor" => BinOp::LogXor,
                "=" => BinOp::Eq,
                "<>" => BinOp::Ne,
                "<" => BinOp::Lt,
                ">" => BinOp::Gt,
                "<=" => BinOp::Le,
                ">=" => BinOp::Ge,
                "+" => BinOp::Add,
                "-" => BinOp::Sub,
                "|" | ".bitor" => BinOp::Or,
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                ".mod" => BinOp::Mod,
                "&" | ".bitand" => BinOp::And,
                "^" | ".bitxor" => BinOp::Xor,
                "<<" | ".shl" => BinOp::Shl,
                ">>" | ".shr" => BinOp::Shr,
                other => return Err(unsupported(range, &format!("the operator `{other}`"))),
            };
            Ir::Bin(op, lhs, rhs)
        }
        Expr::Call(call) => function(call, range, known)?,
    })
}

/// A function: those that only compute a value, and those that ask about
/// the program now: `.sizeof`, `.defined`, `.referenced` and `.const`. The
/// expander has already replaced the functions of tokens, such as `.strlen`.
fn function(call: &CallExpr, range: TextRange, known: &dyn Known) -> Result<Ir, Diagnostic> {
    let name = call
        .function()
        .map(|t| t.text().to_ascii_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        ".sizeof" => return size_of(call, range, known),
        ".defined" | ".def" => {
            let name = named(call, range, "`.defined`")?;
            return Ok(Ir::Num(i64::from(known.is_defined(&name))));
        }
        ".referenced" | ".ref" => {
            let name = named(call, range, "`.referenced`")?;
            return Ok(Ir::Num(i64::from(known.is_referenced(&name))));
        }
        ".const" => {
            let mut args = call.args();
            let value = match (args.next().and_then(|arg| arg.expr()), args.next()) {
                (Some(value), None) => lower(&value, known)?,
                _ => return Err(invalid(range, "`.const` takes one value")),
            };
            return Ok(Ir::Num(i64::from(constant(&value, known).is_some())));
        }
        _ => {}
    }
    const LOWERED: &[&str] = &[
        ".lobyte",
        ".hibyte",
        ".bankbyte",
        ".loword",
        ".hiword",
        ".max",
        ".min",
    ];
    if !LOWERED.contains(&name.as_str()) {
        return Err(unsupported(range, &format!("the function `{name}`")));
    }
    let args = call
        .args()
        .map(|arg| {
            arg.expr()
                .ok_or_else(|| invalid(arg.syntax().text_range(), "expected a value"))
                .and_then(|e| lower(&e, known))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let wrong = |count: &str| invalid(range, &format!("`{name}` takes {count}"));
    let one = |args: Vec<Ir>| match <[Ir; 1]>::try_from(args) {
        Ok([arg]) => Ok(Box::new(arg)),
        Err(_) => Err(wrong("one value")),
    };
    let two = |args: Vec<Ir>| match <[Ir; 2]>::try_from(args) {
        Ok([a, b]) => Ok((Box::new(a), Box::new(b))),
        Err(_) => Err(wrong("two values")),
    };
    Ok(match name.as_str() {
        ".lobyte" => Ir::Lo(one(args)?),
        ".hibyte" => Ir::Hi(one(args)?),
        ".bankbyte" => Ir::Bank(one(args)?),
        ".loword" => Ir::Bin(BinOp::And, one(args)?, Box::new(Ir::Num(0xFFFF))),
        ".hiword" => Ir::Bin(
            BinOp::And,
            Box::new(Ir::Bin(BinOp::Shr, one(args)?, Box::new(Ir::Num(16)))),
            Box::new(Ir::Num(0xFFFF)),
        ),
        ".max" => {
            let (a, b) = two(args)?;
            Ir::Bin(BinOp::Max, a, b)
        }
        // `.min`, the last of the functions lowered.
        _ => {
            let (a, b) = two(args)?;
            Ir::Bin(BinOp::Min, a, b)
        }
    })
}

/// The one name `call` takes, for `what`. It is not read.
fn named(call: &CallExpr, range: TextRange, what: &str) -> Result<String, Diagnostic> {
    let mut args = call.args();
    match (args.next().and_then(|arg| arg.expr()), args.next()) {
        (Some(Expr::Name(name)), None) => Ok(name.text()),
        _ => Err(invalid(range, &format!("{what} takes a name"))),
    }
}

/// `.sizeof(name)`: the bytes a label's line made, or a scope's bytes. ca65
/// needs the size now, so a name further on has none.
fn size_of(call: &CallExpr, range: TextRange, known: &dyn Known) -> Result<Ir, Diagnostic> {
    let name = named(call, range, "`.sizeof`")?;
    known
        .size_of(&name)
        .map(Ir::Num)
        .ok_or_else(|| invalid(range, &format!("the size of `{name}` is unknown")))
}

/// A number, a character or `*`.
fn literal_value(token: &SyntaxToken) -> Result<Ir, Diagnostic> {
    let text = token.text();
    let range = token.text_range();
    match token.kind() {
        kinds::NUMBER => crate::number::value(text)
            .map(Ir::Num)
            .ok_or_else(|| invalid(range, "this number is too large")),
        kinds::CHAR => {
            let inner = text.strip_prefix('\'').unwrap_or(text);
            let inner = inner.strip_suffix('\'').unwrap_or(inner);
            let mut bytes = inner.bytes();
            match (bytes.next(), bytes.next()) {
                (Some(byte), None) => Ok(Ir::Num(i64::from(byte))),
                _ => Err(invalid(range, "a character is one byte")),
            }
        }
        kinds::STAR => Ok(Ir::Pc),
        _ => Err(invalid(range, "a string is not a value here")),
    }
}

/// The address size ca65 gives `expr`. A value whose names are all
/// constants takes the size its value needs. Otherwise each name takes its
/// own size, an unknown name absolute, and `<`, `>` and `^` make one byte.
/// So `r - l` is absolute, though its value is known.
pub(super) fn size(expr: &Ir, known: &dyn Known) -> Size {
    match plain(expr, known) {
        Some(value) => Size::of(value),
        None => structural(expr, known).unwrap_or(Size::Absolute),
    }
}

/// The value of `expr` if it is known now. Every name in it is a constant,
/// or the labels in it cancel out, as in `r - l` for two labels in one
/// segment.
pub(super) fn constant(expr: &Ir, known: &dyn Known) -> Option<i64> {
    linear(expr, known)
        .filter(|sum| sum.terms.is_empty())
        .map(|sum| sum.value)
}

/// The value of `expr` if every name in it is a constant.
pub(super) fn plain(expr: &Ir, known: &dyn Known) -> Option<i64> {
    let value = |name: &str| match known.name(name) {
        Value::Constant(value) => Some(value),
        _ => None,
    };
    let pc = match known.pc() {
        Value::Constant(value) => Some(value),
        _ => None,
    };
    expr.eval_with(&value, pc, 0).ok()
}

/// What a [`Sum`] counts: a segment's start, or an import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Term {
    /// The start of the segment the assembler numbers so.
    Segment(u32),
    /// A name the linker gives a value, as written.
    Import(String),
}

/// A value as ca65 keeps it while it reads: a number plus a sum of labels
/// and imports, each counted some number of times.
#[derive(Debug, Default)]
pub(super) struct Sum {
    /// The number, with each label's offset into its segment counted in.
    pub(super) value: i64,
    /// How many times each segment's start or import is counted, for those
    /// that do not cancel out.
    pub(super) terms: Vec<(Term, i64)>,
}

impl Sum {
    fn number(value: i64) -> Self {
        Self {
            value,
            terms: Vec::new(),
        }
    }

    fn known(&self) -> Option<i64> {
        self.terms.is_empty().then_some(self.value)
    }

    fn add(mut self, other: Self, sign: i64) -> Option<Self> {
        self.value = self.value.checked_add(other.value.checked_mul(sign)?)?;
        for (term, count) in other.terms {
            match self.terms.iter_mut().find(|(mine, _)| *mine == term) {
                Some((_, mine)) => *mine = mine.checked_add(count.checked_mul(sign)?)?,
                None => self.terms.push((term, count.checked_mul(sign)?)),
            }
        }
        self.terms.retain(|&(_, count)| count != 0);
        Some(self)
    }

    fn scale(mut self, by: i64) -> Option<Self> {
        self.value = self.value.checked_mul(by)?;
        for (_, count) in &mut self.terms {
            *count = count.checked_mul(by)?;
        }
        self.terms.retain(|&(_, count)| count != 0);
        Some(self)
    }
}

/// `expr` as a [`Sum`], if it is one. Labels may be added, taken away,
/// negated and multiplied by a known number, as ca65 allows. Any other
/// operation needs a known value on each side.
pub(super) fn linear(expr: &Ir, known: &dyn Known) -> Option<Sum> {
    let of = |value: Value, name: Option<&str>| match value {
        Value::Constant(value) | Value::Measured { value, .. } => Some(Sum::number(value)),
        Value::Placed {
            segment, offset, ..
        } => Some(Sum {
            value: offset,
            terms: vec![(Term::Segment(segment), 1)],
        }),
        Value::Address(_) => Some(Sum {
            value: 0,
            terms: vec![(Term::Import(name?.to_owned()), 1)],
        }),
        Value::Opaque(_) | Value::Unknown => None,
    };
    match expr {
        Ir::Num(value) => Some(Sum::number(*value)),
        Ir::Sym(name) => of(known.name(name), Some(name)),
        Ir::Pc => of(known.pc(), None),
        Ir::Neg(inner) => linear(inner, known)?.scale(-1),
        Ir::Bin(BinOp::Add, lhs, rhs) => linear(lhs, known)?.add(linear(rhs, known)?, 1),
        Ir::Bin(BinOp::Sub, lhs, rhs) => linear(lhs, known)?.add(linear(rhs, known)?, -1),
        Ir::Bin(BinOp::Mul, lhs, rhs) => {
            let (lhs, rhs) = (linear(lhs, known)?, linear(rhs, known)?);
            match (lhs.known(), rhs.known()) {
                (_, Some(by)) => lhs.scale(by),
                (Some(by), None) => rhs.scale(by),
                (None, None) => None,
            }
        }
        // Each side that is known becomes its number, and the rest is left
        // to the evaluator, so `&&` and `||` stay lazy.
        other => {
            let folded = map_children(other, &mut |child| match linear(child, known) {
                Some(sum) if sum.terms.is_empty() => Ir::Num(sum.value),
                _ => child.clone(),
            });
            plain(&folded, known).map(Sum::number)
        }
    }
}

/// `expr` with each of its direct parts replaced by `map`.
fn map_children(expr: &Ir, map: &mut dyn FnMut(&Ir) -> Ir) -> Ir {
    let boxed = |inner: &Ir, map: &mut dyn FnMut(&Ir) -> Ir| Box::new(map(inner));
    match expr {
        Ir::Num(_) | Ir::Sym(_) | Ir::Pc => expr.clone(),
        Ir::Lo(inner) => Ir::Lo(boxed(inner, map)),
        Ir::Hi(inner) => Ir::Hi(boxed(inner, map)),
        Ir::Bank(inner) => Ir::Bank(boxed(inner, map)),
        Ir::Neg(inner) => Ir::Neg(boxed(inner, map)),
        Ir::BitNot(inner) => Ir::BitNot(boxed(inner, map)),
        Ir::FixRound(inner) => Ir::FixRound(boxed(inner, map)),
        Ir::TrailingZeros(inner) => Ir::TrailingZeros(boxed(inner, map)),
        Ir::LogNot(inner) => Ir::LogNot(boxed(inner, map)),
        Ir::Bin(op, lhs, rhs) => Ir::Bin(*op, boxed(lhs, map), boxed(rhs, map)),
    }
}

/// What ca65 V2.19's `StudyExpr` learns of an expression's address size.
#[derive(Clone, Copy)]
struct Study {
    /// The largest size of the names in it, if any: `None` for numbers alone.
    names: Option<Size>,
    /// The size a number in it needs, for numbers alone.
    number: Option<Size>,
    /// Whether it is still names and numbers added, taken away, negated or
    /// multiplied by a number. While it is, its numbers do not count
    /// towards its size, so `zz + $100` is on the zero page.
    linear: bool,
}

impl Study {
    fn size(self) -> Option<Size> {
        self.names.or(self.number)
    }

    /// The result of an operation ca65 cannot fold: the larger of both
    /// sides, numbers included.
    fn merged(self, other: Self) -> Self {
        Self::fixed(self.size().max(other.size()))
    }

    fn fixed(size: Option<Size>) -> Self {
        Self {
            names: size,
            number: None,
            linear: false,
        }
    }
}

/// The address size ca65 V2.19's `StudyExpr` gives `expr`, measured with
/// its listings. In names and numbers added, taken away, negated or
/// multiplied by a number, only the names count, each by its own size, so
/// `zz + $100` is on the zero page and `r - l` is absolute. `<`, `>`, `^`, a
/// comparison and the logical operators make one byte. `&` takes the
/// smaller of its sides, so `lab & $FF` is on the zero page. Any other
/// operation takes the larger, numbers included, so `<lab + $100` is
/// absolute.
fn structural(expr: &Ir, known: &dyn Known) -> Option<Size> {
    study(expr, known).size()
}

fn study(expr: &Ir, known: &dyn Known) -> Study {
    let number = |value: i64| Study {
        names: None,
        number: Some(Size::of(value)),
        linear: true,
    };
    let of = |value: Value| match value {
        Value::Constant(value) => number(value),
        Value::Address(size)
        | Value::Opaque(size)
        | Value::Placed { size, .. }
        | Value::Measured { size, .. } => Study {
            names: Some(size),
            number: None,
            linear: true,
        },
        Value::Unknown => Study {
            names: Some(Size::Absolute),
            number: None,
            linear: true,
        },
    };
    match expr {
        Ir::Num(value) => number(*value),
        Ir::Sym(name) => of(known.name(name)),
        Ir::Pc => of(known.pc()),
        Ir::Lo(_) | Ir::Hi(_) | Ir::Bank(_) | Ir::LogNot(_) => Study::fixed(Some(Size::Zero)),
        Ir::Bin(
            BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::LogAnd
            | BinOp::LogOr
            | BinOp::LogXor,
            _,
            _,
        ) => Study::fixed(Some(Size::Zero)),
        Ir::Bin(op, lhs, rhs) => {
            let (lhs, rhs) = (study(lhs, known), study(rhs, known));
            let both = lhs.linear && rhs.linear;
            match op {
                BinOp::Add | BinOp::Sub if both => Study {
                    names: lhs.names.max(rhs.names),
                    number: lhs.number.max(rhs.number),
                    linear: true,
                },
                BinOp::Mul if both && lhs.names.is_none() => rhs,
                BinOp::Mul if both && rhs.names.is_none() => lhs,
                BinOp::And => Study::fixed(lhs.size().min(rhs.size())),
                _ => lhs.merged(rhs),
            }
        }
        Ir::Neg(inner) => study(inner, known),
        Ir::BitNot(inner) | Ir::FixRound(inner) | Ir::TrailingZeros(inner) => {
            Study::fixed(study(inner, known).size())
        }
    }
}

pub(super) fn invalid(range: TextRange, message: &str) -> Diagnostic {
    Diagnostic::new(DiagnosticKind::Invalid, range, message)
}

pub(super) fn unsupported(range: TextRange, what: &str) -> Diagnostic {
    Diagnostic::new(
        DiagnosticKind::Unsupported,
        range,
        format!("{what} is not lowered yet"),
    )
}

#[cfg(test)]
mod tests;
