//! One line, as the expander hands it over, lowered to IR statements.

use asm198x_ir::{BinOp, DataRange, DiagSeverity, Expr as Ir, Operation};
use lang198x_syntax::{Diagnostic, SyntaxToken};

use super::expr::{self, invalid, unsupported};
use super::{Known, Size, Value, instruction};
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

/// A lowered line: its statements, in order, and any change it makes to
/// the scopes names live in.
#[derive(Clone)]
#[non_exhaustive]
pub struct Lowered {
    /// The statements. An unnamed label before a named one makes two.
    pub items: Vec<Item>,
    /// The change the line makes to the scopes, made after its statements.
    pub scope: Option<Scope>,
    /// The change the line makes to the segment later lines go into, made
    /// after its statements.
    pub segment: Option<Segment>,
    /// The names the line shows a linker, or asks one for.
    pub visibility: Option<Visibility>,
    /// The file whose bytes the line puts here, from `.incbin`.
    pub binary: Option<Binary>,
}

/// The bytes of a file, put where the line is, from
/// `.incbin "name"[, offset[, size]]`.
///
/// Lowering only reports the file, since the assembler reads it. ca65 needs
/// the offset and size as soon as it reads the line. A negative size, like
/// none, means the rest of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Binary {
    /// The file's name, as written.
    pub name: String,
    /// Where in the file to start, if written.
    pub offset: Option<i64>,
    /// How many bytes to take, if written.
    pub size: Option<i64>,
}

/// The names a line shows a linker, or asks one for, from `.import`,
/// `.export`, `.global`, their zero-page forms, or `.forceimport`.
///
/// Lowering only reports these, since the assembler keeps the names. An
/// export written with a value, such as `.export v := 3`, also defines the
/// name, as an assignment among the line's items.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Visibility {
    /// What the line does with the names.
    pub kind: Visible,
    /// The names, in order.
    pub names: Vec<Declared>,
}

/// What a line does with the names it declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Visible {
    /// `.import`: each name is defined elsewhere, by another program.
    Import,
    /// `.forceimport`: each name is imported even if it is never used.
    ForceImport,
    /// `.export`: each name is defined here, for another program.
    Export,
    /// `.global`: each name is exported if this program defines it, and
    /// imported if it does not.
    Global,
}

/// One name a line declares, with the address size written for it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Declared {
    /// The name, which has no scope written before it.
    pub name: String,
    /// The address size: the zero page for the `zp` forms, the size written
    /// after the name, or `None` for neither, which ca65 takes as absolute.
    pub size: Option<Size>,
}

/// A change to the segment lines go into, from `.segment`, its short forms
/// `.code`, `.rodata`, `.data`, `.bss` and `.zeropage`, or `.pushseg` and
/// `.popseg`.
///
/// Lowering only reports these. The assembler keeps the segments, and the
/// linker's configuration places them. A program starts in `CODE`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Segment {
    /// Later lines go into the segment `name`, which is case-sensitive.
    /// `size` is the address size written after the name, if any; ca65
    /// gives `ZEROPAGE` the zero page and every other segment absolute
    /// addresses unless one is written.
    Use {
        /// The segment's name.
        name: String,
        /// The address size written after it.
        size: Option<Size>,
    },
    /// `.pushseg`: the current segment is saved.
    Push,
    /// `.popseg`: the segment saved last is current again.
    Pop,
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
    let label = syntax
        .label()
        .and_then(|l| l.name())
        .map(|name| name.text());
    labels.extend(label.clone());
    let on_line = OnLine { known, label };
    let known: &dyn Known = &on_line;
    let mut scope = None;
    let mut segment = None;
    let mut visibility = None;
    let mut binary = None;
    let mut defined = Vec::new();
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
                None => match segment_change(&command)? {
                    Some(change) => {
                        segment = Some(change);
                        None
                    }
                    None => match declared(&command, known)? {
                        Some((declared, values)) => {
                            visibility = Some(declared);
                            defined = values;
                            None
                        }
                        None => match included_binary(&command, line, known)? {
                            Some(included) => {
                                binary = Some(included);
                                None
                            }
                            None => command_op(&command, line, known)?,
                        },
                    },
                },
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
    items.extend(defined);
    Ok(Lowered {
        items,
        scope,
        segment,
        visibility,
        binary,
    })
}

/// What is known on a line. ca65 defines a line's label before it reads the
/// rest, so the line can use it: in `l: lda l` under `.org $10`, `l` is on
/// the zero page.
struct OnLine<'a> {
    known: &'a dyn Known,
    label: Option<String>,
}

impl OnLine<'_> {
    fn is_label(&self, name: &str) -> bool {
        self.label.as_deref() == Some(name)
    }
}

impl Known for OnLine<'_> {
    fn name(&self, name: &str) -> Value {
        if self.is_label(name) {
            self.known.pc()
        } else {
            self.known.name(name)
        }
    }

    fn pc(&self) -> Value {
        self.known.pc()
    }

    fn size_of(&self, name: &str) -> Option<i64> {
        self.known.size_of(name)
    }

    fn is_defined(&self, name: &str) -> bool {
        self.is_label(name) || self.known.is_defined(name)
    }

    fn is_referenced(&self, name: &str) -> bool {
        self.known.is_referenced(name)
    }

    fn read(&self, name: &str) {
        self.known.read(name);
    }
}

/// The change to the segment a command makes, if it is `.segment`, one of
/// its short forms, `.pushseg` or `.popseg`.
fn segment_change(command: &Command) -> Result<Option<Segment>, Diagnostic> {
    let range = command.syntax().text_range();
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    let named = |name: &str| {
        Some(Segment::Use {
            name: name.to_owned(),
            size: None,
        })
    };
    Ok(match word.as_str() {
        ".code" => named("CODE"),
        ".rodata" => named("RODATA"),
        ".data" => named("DATA"),
        ".bss" => named("BSS"),
        ".zeropage" => named("ZEROPAGE"),
        ".pushseg" => Some(Segment::Push),
        ".popseg" => Some(Segment::Pop),
        ".segment" => {
            let mut values = command.values();
            let name = match (values.next(), values.next()) {
                (Some(Expr::Literal(literal)), None) => literal
                    .token()
                    .filter(|t| t.kind() == kinds::STRING)
                    .map(|t| {
                        let text = t.text().strip_prefix('"').unwrap_or(t.text());
                        text.strip_suffix('"').unwrap_or(text).to_owned()
                    }),
                _ => None,
            }
            .ok_or_else(|| invalid(range, "`.segment` takes the segment's name, in quotes"))?;
            let size = match command.address_size() {
                None => None,
                Some(word) => Some(address_size(&word.text().to_ascii_lowercase()).ok_or_else(
                    || unsupported(range, &format!("the address size `{}`", word.text())),
                )?),
            };
            Some(Segment::Use { name, size })
        }
        _ => None,
    })
}

/// The address size a word after a colon names, as ca65 spells them.
fn address_size(word: &str) -> Option<Size> {
    match word {
        "zeropage" | "zp" | "direct" | "dp" => Some(Size::Zero),
        "absolute" | "abs" | "near" => Some(Size::Absolute),
        "far" | "long" => Some(Size::Far),
        _ => None,
    }
}

/// The file a command puts here, if it is `.incbin`.
fn included_binary(
    command: &Command,
    line: &ActiveLine,
    known: &dyn Known,
) -> Result<Option<Binary>, Diagnostic> {
    let is_incbin = command
        .word()
        .is_some_and(|w| w.text().eq_ignore_ascii_case(".incbin"));
    if !is_incbin {
        return Ok(None);
    }
    let range = command.syntax().text_range();
    let values: Vec<Expr> = command.values().collect();
    let Some((first, rest)) = values.split_first() else {
        return Err(invalid(range, "`.incbin` takes a file name, in quotes"));
    };
    let name = string(first, line).ok_or_else(|| {
        invalid(
            first.syntax().text_range(),
            "expected a file name, in quotes",
        )
    })?;
    if rest.len() > 2 {
        return Err(invalid(
            range,
            "`.incbin` takes a file name, an offset and a size",
        ));
    }
    let now = |value: &Expr| -> Result<i64, Diagnostic> {
        let lowered = expr::expression(value, known)?;
        expr::constant(&lowered, known)
            .ok_or_else(|| invalid(value.syntax().text_range(), "Constant expression expected"))
    };
    Ok(Some(Binary {
        name: String::from_utf8_lossy(&name).into_owned(),
        offset: rest.first().map(now).transpose()?,
        size: rest.get(1).map(now).transpose()?,
    }))
}

/// The names a command declares, if it is `.import`, `.export`, `.global`,
/// one of their zero-page forms or `.forceimport`, with an assignment for
/// each export written with a value.
fn declared(
    command: &Command,
    known: &dyn Known,
) -> Result<Option<(Visibility, Vec<Item>)>, Diagnostic> {
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    let (kind, zero_page) = match word.as_str() {
        ".import" => (Visible::Import, false),
        ".importzp" => (Visible::Import, true),
        ".forceimport" => (Visible::ForceImport, false),
        ".export" => (Visible::Export, false),
        ".exportzp" => (Visible::Export, true),
        ".global" => (Visible::Global, false),
        ".globalzp" => (Visible::Global, true),
        _ => return Ok(None),
    };
    let mut names = Vec::new();
    let mut defined = Vec::new();
    for item in command.items() {
        let range = item.syntax().text_range();
        let name = item
            .name()
            .ok_or_else(|| invalid(range, "expected a name"))?
            .text();
        if name.contains("::") {
            return Err(invalid(range, "a declared name has no scope before it"));
        }
        let size = match item.address_size() {
            Some(word) => match address_size(&word.text().to_ascii_lowercase()) {
                Some(Size::Far) => {
                    return Err(invalid(
                        range,
                        "the address size `far` is not one the 6502 has",
                    ));
                }
                Some(size) => Some(size),
                None => {
                    return Err(invalid(
                        range,
                        &format!("`{}` is not an address size", word.text()),
                    ));
                }
            },
            None => zero_page.then_some(Size::Zero),
        };
        if let Some(value) = item.value() {
            if kind != Visible::Export {
                return Err(invalid(range, &format!("`{word}` takes no value")));
            }
            defined.push(Item {
                label: Some(name.clone()),
                op: Some(Operation::Equ(expr::expression(&value, known)?)),
            });
        }
        names.push(Declared { name, size });
    }
    if names.is_empty() {
        let range = command.syntax().text_range();
        return Err(invalid(range, "expected a name"));
    }
    Ok(Some((Visibility { kind, names }, defined)))
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
        ".out" => {
            let [value] = <[Expr; 1]>::try_from(values())
                .map_err(|_| invalid(range, "`.out` takes one string"))?;
            say(DiagSeverity::Note, &value, line)?
        }
        // These read their string and ignore what follows it, as ca65 does.
        ".warning" | ".error" | ".fatal" => {
            let value = values()
                .into_iter()
                .next()
                .ok_or_else(|| invalid(range, &format!("`{word}` takes a string")))?;
            let severity = if word == ".warning" {
                DiagSeverity::Warning
            } else {
                DiagSeverity::Error
            };
            say(severity, &value, line)?
        }
        ".assert" => assertion(command, line, known)?,
        other => return Err(unsupported(range, &format!("`{other}`"))),
    }))
}

/// A message to say, from the string `value`.
fn say(severity: DiagSeverity, value: &Expr, line: &ActiveLine) -> Result<Operation, Diagnostic> {
    let bytes = string(value, line)
        .ok_or_else(|| invalid(value.syntax().text_range(), "expected a string"))?;
    Ok(Operation::Diagnose {
        severity,
        message: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// `.assert condition, action[, "message"]`. The condition is checked once
/// the program is linked, so it may name a label further on. The actions
/// `error` and `lderror` refuse the program if it is 0; `warning` and
/// `ldwarning` only warn. Assembly and linking are one here, so the `ld`
/// forms are the same as the others.
fn assertion(
    command: &Command,
    line: &ActiveLine,
    known: &dyn Known,
) -> Result<Operation, Diagnostic> {
    let range = command.syntax().text_range();
    let values: Vec<Expr> = command.values().collect();
    let (condition, action, message) = match values.as_slice() {
        [condition, action] => (condition, action, None),
        [condition, action, message] => (condition, action, Some(message)),
        _ => {
            return Err(invalid(
                range,
                "`.assert` takes a condition, an action and perhaps a message",
            ));
        }
    };
    let action = match action {
        Expr::Name(name) => name.text().to_ascii_lowercase(),
        _ => String::new(),
    };
    let fatal = match action.as_str() {
        "error" | "lderror" => true,
        "warning" | "ldwarning" => false,
        _ => {
            return Err(invalid(
                range,
                "the action of `.assert` is `error`, `warning`, `lderror` or `ldwarning`",
            ));
        }
    };
    let message = match message {
        Some(message) => {
            let bytes = string(message, line)
                .ok_or_else(|| invalid(message.syntax().text_range(), "expected a string"))?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
        None => "Assertion failed".to_owned(),
    };
    Ok(Operation::Assert {
        cond: expr::expression(condition, known)?,
        fatal,
        message,
    })
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
