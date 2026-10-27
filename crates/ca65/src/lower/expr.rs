//! Expressions, lowered to IR expressions, and the address size ca65 gives
//! each one.

use asm198x_ir::{BinOp, Expr as Ir};
use lang198x_syntax::{Diagnostic, DiagnosticKind, SyntaxToken, TextRange};

use super::{Known, Size, Value};
use crate::ast::{CallExpr, Expr, View};
use crate::kinds;

/// Lowers `expr` to an IR expression with the value ca65 gives it. Names stay
/// names, as written. `known` answers what ca65 needs now: the size of a
/// name in `.sizeof`.
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
        Expr::Name(name) => Ir::Sym(name.text()),
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

/// A function: those that only compute a value, and `.sizeof`, which ca65
/// needs to know now. Those that ask other things about the program, such as
/// `.defined`, are not lowered yet.
fn function(call: &CallExpr, range: TextRange, known: &dyn Known) -> Result<Ir, Diagnostic> {
    let name = call
        .function()
        .map(|t| t.text().to_ascii_lowercase())
        .unwrap_or_default();
    if name == ".sizeof" {
        return size_of(call, range, known);
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

/// `.sizeof(name)`: the bytes a label's line made, or a scope's bytes. ca65
/// needs the size now, so a name further on has none.
fn size_of(call: &CallExpr, range: TextRange, known: &dyn Known) -> Result<Ir, Diagnostic> {
    let mut args = call.args();
    let name = match (args.next().and_then(|arg| arg.expr()), args.next()) {
        (Some(Expr::Name(name)), None) => name.text(),
        _ => return Err(invalid(range, "`.sizeof` takes a name")),
    };
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
        kinds::NUMBER => number(text)
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

/// The value of a number as ca65 writes it: `$hex`, `%binary`, decimal, or
/// hex ending in `h`. ca65 keeps 32 bits.
fn number(text: &str) -> Option<i64> {
    let (digits, radix) = if let Some(hex) = text.strip_prefix('$') {
        (hex, 16)
    } else if let Some(binary) = text.strip_prefix('%') {
        (binary, 2)
    } else if let Some(hex) = text.strip_suffix(['h', 'H']) {
        (hex, 16)
    } else {
        (text, 10)
    };
    i64::from_str_radix(digits, radix)
        .ok()
        .filter(|v| *v <= i64::from(u32::MAX))
}

/// The address size ca65 gives `expr`. A value known now takes the size its
/// value needs. Otherwise each name takes its own size, an unknown name
/// absolute, and `<`, `>` and `^` make one byte.
pub(super) fn size(expr: &Ir, known: &dyn Known) -> Size {
    match constant(expr, known) {
        Some(value) => Size::of(value),
        None => structural(expr, known).unwrap_or(Size::Absolute),
    }
}

/// The value of `expr` if it is known now: every name in it a constant.
pub(super) fn constant(expr: &Ir, known: &dyn Known) -> Option<i64> {
    let value = |name: &str| match known.name(name) {
        Value::Constant(value) => Some(value),
        Value::Address(_) | Value::Unknown => None,
    };
    let pc = match known.pc() {
        Value::Constant(value) => Some(value),
        Value::Address(_) | Value::Unknown => None,
    };
    expr.eval_with(&value, pc, 0).ok()
}

fn structural(expr: &Ir, known: &dyn Known) -> Option<Size> {
    let of = |value: Value| match value {
        Value::Constant(value) => Some(Size::of(value)),
        Value::Address(size) => Some(size),
        Value::Unknown => Some(Size::Absolute),
    };
    match expr {
        Ir::Num(_) => None,
        Ir::Sym(name) => of(known.name(name)),
        Ir::Pc => of(known.pc()),
        Ir::Lo(_) | Ir::Hi(_) | Ir::Bank(_) => Some(Size::Zero),
        Ir::Bin(_, lhs, rhs) => structural(lhs, known).max(structural(rhs, known)),
        Ir::Neg(inner)
        | Ir::BitNot(inner)
        | Ir::LogNot(inner)
        | Ir::FixRound(inner)
        | Ir::TrailingZeros(inner) => structural(inner, known),
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
