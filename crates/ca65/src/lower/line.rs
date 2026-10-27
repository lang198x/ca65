//! One line, as the expander hands it over, lowered to IR statements.

use asm198x_ir::{BinOp, DataRange, Expr as Ir, Operation};
use lang198x_syntax::{Diagnostic, SyntaxToken};

use super::expr::{self, invalid, unsupported};
use super::{Known, instruction};
use crate::Profile;
use crate::ast::{Command, Expr, Statement, View};
use crate::expand::ActiveLine;

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

/// A lowered line: its statements, in order, and any change it makes to
/// the scopes names live in.
#[derive(Clone)]
#[non_exhaustive]
pub struct Lowered {
    /// The statements. An unnamed label before a named one makes two.
    pub items: Vec<Item>,
    /// The change the line makes to the scopes, made after its statements.
    pub scope: Option<Scope>,
}

/// A change to the scopes names live in, from `.scope`, `.proc`,
/// `.endscope` or `.endproc`.
///
/// Lowering only reports these: the names in a line's statements are left
/// as written. The assembler keeps the scopes, because ca65 ties them to its
/// symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Scope {
    /// `.scope` or `.proc`: a scope opens, with a name or without one. A
    /// `.proc` with a name also defines that name at the address, in the
    /// scope around the new one, as a label among the line's items. ca65
    /// warns that a `.proc` without a name is deprecated, and opens an
    /// unnamed `.scope`, which `.endscope` closes.
    Open {
        /// The scope's name, if it has one.
        name: Option<String>,
        /// Whether `.proc` opened it, so only `.endproc` closes it.
        procedure: bool,
    },
    /// `.endscope` or `.endproc`: the innermost scope closes. The command
    /// must match the one that opened it. A label on the line belongs to
    /// the scope that closes.
    Close {
        /// Whether this is `.endproc`.
        procedure: bool,
    },
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
    let mut scope = None;
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
                let value = expr::expression(&value, known)?;
                let op = assignment.op().map(|t| t.text().to_ascii_lowercase());
                labels.push(name.text());
                Some(if op.as_deref() == Some(".set") {
                    Operation::Set(value)
                } else {
                    Operation::Equ(value)
                })
            }
            Some(Statement::Command(command)) => match scope_change(&command)? {
                Some(change) => {
                    if let Scope::Open {
                        name: Some(name),
                        procedure: true,
                    } = &change
                    {
                        labels.push(name.clone());
                    }
                    scope = Some(change);
                    None
                }
                None => command_op(&command, line, known)?,
            },
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
    Ok(Lowered { items, scope })
}

/// The change to the scopes a command makes, if it is `.scope`, `.proc`,
/// `.endscope` or `.endproc`. The parser refuses a label before `.scope` or
/// `.proc`, as ca65 does.
fn scope_change(command: &Command) -> Result<Option<Scope>, Diagnostic> {
    let range = command.syntax().text_range();
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    let procedure = word == ".proc" || word == ".endproc";
    match word.as_str() {
        ".scope" | ".proc" => {
            if command.address_size().is_some() {
                return Err(unsupported(range, &format!("an address size on `{word}`")));
            }
            let name = command.name().map(|name| name.text());
            Ok(Some(Scope::Open {
                procedure: procedure && name.is_some(),
                name,
            }))
        }
        ".endscope" | ".endproc" => Ok(Some(Scope::Close { procedure })),
        _ => Ok(None),
    }
}

/// The operation a control command lowers to.
fn command_op(
    command: &Command,
    line: &ActiveLine,
    known: &dyn Known,
) -> Result<Option<Operation>, Diagnostic> {
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
            .map(|value| expr::expression(value, known))
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
        ".byte" | ".byt" => sized(1, false, bytes(at_least_one(values())?, line, known)?),
        ".asciiz" => {
            let mut values = bytes(values(), line, known)?;
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
            Operation::Org(expr::expression(&value, known)?)
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
fn bytes(values: Vec<Expr>, line: &ActiveLine, known: &dyn Known) -> Result<Vec<Ir>, Diagnostic> {
    let mut out = Vec::new();
    for value in values {
        match string(&value, line) {
            Some(bytes) => out.extend(bytes.into_iter().map(|b| Ir::Num(i64::from(b)))),
            None => out.push(expr::expression(&value, known)?),
        }
    }
    Ok(out)
}

/// What a string literal holds, without its quotes. A string a token
/// function made may hold bytes its text cannot; the line keeps those.
fn string(value: &Expr, line: &ActiveLine) -> Option<Vec<u8>> {
    let Expr::Literal(literal) = value else {
        return None;
    };
    let token: SyntaxToken = literal.token()?;
    line.string(&token)
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
        let lowered = expr::expression(value, known)?;
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
