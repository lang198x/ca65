//! One line, as the expander hands it over, lowered to IR statements.

use asm198x_ir::{BinOp, DataRange, Expr as Ir, Operation};
use lang198x_syntax::{Diagnostic, SyntaxToken};

use super::expr::{self, invalid, unsupported};
use super::{Known, instruction};
use crate::Profile;
use crate::ast::{Command, Expr, Statement, View};
use crate::expand::ActiveLine;
use crate::kinds;

/// A statement a line lowers to: a label, an operation, or both.
///
/// The label is the name the line defines, as written: `loop`, `@near`, or
/// `:` for an unnamed label, whose references the assembler counts. With
/// [`Operation::Equ`] or [`Operation::Set`] it takes that value, and
/// otherwise the address of the operation.
#[derive(Clone)]
#[non_exhaustive]
pub struct Item {
    /// The name defined, if any.
    pub label: Option<String>,
    /// What the statement does, if anything.
    pub op: Option<Operation>,
}

/// A lowered line: its statements, in order.
#[derive(Clone)]
#[non_exhaustive]
pub struct Lowered {
    /// The statements. An unnamed label before a named one makes two.
    pub items: Vec<Item>,
}

/// Lowers `line` as ca65 V2.19 assembles it under `profile`.
///
/// `known` answers for what ca65 needs as soon as it reads the line: the
/// address size of an instruction's operand, and the counts of `.res` and
/// `.align`, which must be known now.
///
/// # Errors
///
/// A diagnostic, with its range in the line's [text](ActiveLine::text), if
/// ca65 refuses the line. Its kind is
/// [`Unsupported`](lang198x_syntax::DiagnosticKind::Unsupported) for a
/// command ca65 accepts that is not lowered yet.
pub fn line(line: &ActiveLine, profile: Profile, known: &dyn Known) -> Result<Lowered, Diagnostic> {
    let syntax = line.line();
    let mut labels: Vec<String> = Vec::new();
    if syntax.unnamed_label().is_some() {
        labels.push(":".to_owned());
    }
    if let Some(name) = syntax.label().and_then(|l| l.name()) {
        labels.push(name.text());
    }
    let op = if line.label_only() {
        None
    } else {
        match syntax.statement() {
            None => None,
            Some(Statement::Instruction(instruction)) => {
                Some(instruction::instruction(&instruction, profile, known)?)
            }
            Some(Statement::Assignment(assignment)) => {
                let range = assignment.syntax().text_range();
                let name = assignment
                    .name()
                    .ok_or_else(|| invalid(range, "expected a name"))?;
                let value = assignment
                    .value()
                    .ok_or_else(|| invalid(range, "expected a value"))?;
                let value = expr::expression(&value)?;
                let op = assignment.op().map(|t| t.text().to_ascii_lowercase());
                labels.push(name.text());
                Some(if op.as_deref() == Some(".set") {
                    Operation::Set(value)
                } else {
                    Operation::Equ(value)
                })
            }
            Some(Statement::Command(command)) => command_op(&command, known)?,
            Some(_) => {
                return Err(unsupported(
                    syntax.syntax().text_range(),
                    "this kind of statement",
                ));
            }
        }
    };
    let mut items: Vec<Item> = labels
        .into_iter()
        .map(|label| Item {
            label: Some(label),
            op: None,
        })
        .collect();
    match (items.last_mut(), op) {
        (Some(last), op) => last.op = op,
        (None, Some(op)) => items.push(Item {
            label: None,
            op: Some(op),
        }),
        (None, None) => {}
    }
    Ok(Lowered { items })
}

/// The operation a control command lowers to.
fn command_op(command: &Command, known: &dyn Known) -> Result<Option<Operation>, Diagnostic> {
    let range = command.syntax().text_range();
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    let values = || command.values().collect::<Vec<Expr>>();
    // ca65's `*` in a list is the address of the value it is in, so the
    // second `*` in `.word *, *` is two more than the first.
    let sized = |width: u8, big_endian: bool, values: Vec<Ir>| Operation::Sized {
        width,
        big_endian,
        range: DataRange::Unsigned,
        values: values
            .into_iter()
            .zip((0_i64..).step_by(usize::from(width)))
            .map(|(value, offset)| at_offset(value, offset))
            .collect(),
    };
    let lowered = |values: Vec<Expr>| {
        values
            .iter()
            .map(expr::expression)
            .collect::<Result<Vec<_>, _>>()
    };
    let at_least_one = |values: Vec<Expr>| {
        if values.is_empty() {
            Err(invalid(range, "expected a value"))
        } else {
            Ok(values)
        }
    };
    Ok(Some(match word.as_str() {
        ".byte" | ".byt" => sized(1, false, bytes(at_least_one(values())?)?),
        ".asciiz" => {
            let mut values = bytes(values())?;
            values.push(Ir::Num(0));
            sized(1, false, values)
        }
        ".word" | ".addr" => sized(2, false, lowered(at_least_one(values())?)?),
        ".dbyt" => sized(2, true, lowered(at_least_one(values())?)?),
        ".faraddr" => sized(3, false, lowered(at_least_one(values())?)?),
        ".dword" => sized(4, false, lowered(at_least_one(values())?)?),
        ".lobytes" | ".hibytes" | ".bankbytes" => {
            let wrap: fn(Box<Ir>) -> Ir = match word.as_str() {
                ".lobytes" => Ir::Lo,
                ".hibytes" => Ir::Hi,
                _ => Ir::Bank,
            };
            let values = lowered(at_least_one(values())?)?;
            sized(
                1,
                false,
                values.into_iter().map(|v| wrap(Box::new(v))).collect(),
            )
        }
        ".res" => {
            let (count, fill) = count_and_fill(command, known, "the number of bytes")?;
            let count = usize::try_from(count)
                .map_err(|_| invalid(range, "the number of bytes must not be negative"))?;
            match fill {
                None => Operation::Reserve(count),
                Some(value) => Operation::Fill {
                    count: Ir::Num(i64::try_from(count).unwrap_or(i64::MAX)),
                    value,
                },
            }
        }
        ".align" => {
            let (modulus, fill) = count_and_fill(command, known, "the alignment")?;
            Operation::AlignTo {
                modulus,
                fill: vec![fill.unwrap_or(0)],
            }
        }
        ".org" => {
            let [value] = <[Expr; 1]>::try_from(values())
                .map_err(|_| invalid(range, "`.org` takes one value"))?;
            Operation::Org(expr::expression(&value)?)
        }
        other => return Err(unsupported(range, &format!("`{other}`"))),
    }))
}

/// `value` with each `*` moved on by `offset` bytes.
fn at_offset(value: Ir, offset: i64) -> Ir {
    let moved = |inner: Box<Ir>| Box::new(at_offset(*inner, offset));
    match value {
        Ir::Pc if offset != 0 => Ir::Bin(BinOp::Add, Box::new(Ir::Pc), Box::new(Ir::Num(offset))),
        Ir::Lo(inner) => Ir::Lo(moved(inner)),
        Ir::Hi(inner) => Ir::Hi(moved(inner)),
        Ir::Bank(inner) => Ir::Bank(moved(inner)),
        Ir::Neg(inner) => Ir::Neg(moved(inner)),
        Ir::BitNot(inner) => Ir::BitNot(moved(inner)),
        Ir::LogNot(inner) => Ir::LogNot(moved(inner)),
        Ir::FixRound(inner) => Ir::FixRound(moved(inner)),
        Ir::TrailingZeros(inner) => Ir::TrailingZeros(moved(inner)),
        Ir::Bin(op, lhs, rhs) => Ir::Bin(op, moved(lhs), moved(rhs)),
        other => other,
    }
}

/// The values of `.byte` and its kin: a string gives each of its bytes.
fn bytes(values: Vec<Expr>) -> Result<Vec<Ir>, Diagnostic> {
    let mut out = Vec::new();
    for value in values {
        match string(&value) {
            Some(text) => out.extend(text.bytes().map(|b| Ir::Num(i64::from(b)))),
            None => out.push(expr::expression(&value)?),
        }
    }
    Ok(out)
}

/// What a string literal holds, without its quotes.
fn string(value: &Expr) -> Option<String> {
    let Expr::Literal(literal) = value else {
        return None;
    };
    let token: SyntaxToken = literal.token().filter(|t| t.kind() == kinds::STRING)?;
    let text = token.text().strip_prefix('"').unwrap_or(token.text());
    Some(text.strip_suffix('"').unwrap_or(text).to_owned())
}

/// The count of `.res` or `.align`, which ca65 needs now, and the fill byte
/// after it, if one is given.
fn count_and_fill(
    command: &Command,
    known: &dyn Known,
    what: &str,
) -> Result<(i64, Option<u8>), Diagnostic> {
    let range = command.syntax().text_range();
    let values: Vec<Expr> = command.values().collect();
    let now = |value: &Expr| -> Result<i64, Diagnostic> {
        let lowered = expr::expression(value)?;
        expr::constant(&lowered, known).ok_or_else(|| {
            invalid(
                value.syntax().text_range(),
                &format!("{what} must be known here"),
            )
        })
    };
    match values.as_slice() {
        [count] => Ok((now(count)?, None)),
        [count, fill] => {
            let byte = u8::try_from(now(fill)?)
                .map_err(|_| invalid(fill.syntax().text_range(), "the fill must be a byte"))?;
            Ok((now(count)?, Some(byte)))
        }
        _ => Err(invalid(
            range,
            &format!("expected {what}, and perhaps a fill"),
        )),
    }
}

#[cfg(test)]
mod tests;
