//! Records: `.struct`, `.union` and `.enum`, their members and ends, and
//! `.tag`, which makes room for a record in the program.
//!
//! Each follows ca65 V2.19's `struct.c`, `enum.c` and `pseudo.c`. Lowering
//! reports what each line does, and the assembler, which keeps the scopes,
//! counts the members' offsets. Only it knows which records are open and how
//! big each one is.

use asm198x_ir::Expr as Ir;
use lang198x_syntax::Diagnostic;

use super::Known;
use super::expr::{self, invalid};
use crate::ast::{Command, Expr, Member, Statement, View};

/// The kinds of record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RecordKind {
    /// `.struct`: each member follows the one before.
    Struct,
    /// `.union`: every member starts at the same place, and the union is as
    /// big as its biggest member.
    Union,
    /// `.enum`: each member is a constant, one more than the one before.
    Enum,
}

/// What a line does in a record, or the room a `.tag` makes in the program.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Record {
    /// `.struct`, `.union` or `.enum` opens a record. A record with a name
    /// is a scope, and its members are reached as `name::member`. One
    /// without a name puts its members in the scope around it, and inside
    /// another struct or union it carries on from where that record has got
    /// to. A record opened as a member's storage, as in `f .struct`, gives
    /// that member its size when it closes.
    Open {
        /// The kind of record.
        kind: RecordKind,
        /// The record's name, if it has one.
        name: Option<String>,
        /// The member whose storage this record is, if any.
        field: Option<String>,
    },
    /// A member of a struct or union that takes `bytes` bytes, such as
    /// `f .word 2`, or room with no name, such as `.byte`.
    Field {
        /// The member's name, if it has one.
        name: Option<String>,
        /// How many bytes it takes.
        bytes: i64,
    },
    /// A member whose storage is another record, `f .tag name [count]`. Only
    /// the assembler knows how big that record is.
    Tag {
        /// The member's name, if it has one.
        name: Option<String>,
        /// The record's name, as written.
        record: String,
        /// How many of the record the member holds.
        count: i64,
    },
    /// `.org` in a struct: the next member goes this far into it. A member
    /// named on the line is here and takes no room.
    Offset {
        /// The member named on the line, if any.
        name: Option<String>,
        /// How far into the struct the next member goes.
        offset: i64,
    },
    /// A member of an enum. With no value, it is one more than the member
    /// before, or zero for the first.
    Enumerator {
        /// The member's name.
        name: String,
        /// The value written, if any.
        value: Option<Ir>,
    },
    /// `.endstruct`, `.endunion` or `.endenum`: the innermost record closes.
    /// It must be the kind that opened it.
    Close(RecordKind),
    /// `.tag name[, count]` in the program: room for `count` of the record,
    /// as `.res` would make, filled as `.res` fills it.
    Reserve {
        /// The record's name, as written.
        record: String,
        /// How many of the record to make room for.
        count: i64,
    },
}

/// The largest size ca65 lets a member take, one byte under 16 MB.
const LARGEST: i64 = 0xFF_FFFF;

/// What `statement` does in a record, if it is part of one, or the room a
/// `.tag` in the program makes.
pub(super) fn record(
    statement: &Statement,
    known: &dyn Known,
) -> Result<Option<Record>, Diagnostic> {
    match statement {
        Statement::Member(member) => member_record(member, known).map(Some),
        Statement::Command(command) => {
            let word = command
                .word()
                .map(|w| w.text().to_ascii_lowercase())
                .unwrap_or_default();
            Ok(match word.as_str() {
                ".struct" | ".union" | ".enum" => Some(Record::Open {
                    kind: kind_of(&word),
                    name: command.name().map(|n| n.text()),
                    field: None,
                }),
                ".endstruct" => Some(Record::Close(RecordKind::Struct)),
                ".endunion" => Some(Record::Close(RecordKind::Union)),
                ".endenum" => Some(Record::Close(RecordKind::Enum)),
                ".tag" => {
                    let (record, count) = tag(command, known)?;
                    Some(Record::Reserve { record, count })
                }
                _ => None,
            })
        }
        _ => Ok(None),
    }
}

fn kind_of(word: &str) -> RecordKind {
    match word {
        ".union" => RecordKind::Union,
        ".enum" => RecordKind::Enum,
        _ => RecordKind::Struct,
    }
}

/// A line inside a record: a struct's or union's storage, or an enum's
/// member.
fn member_record(member: &Member, known: &dyn Known) -> Result<Record, Diagnostic> {
    let range = member.syntax().text_range();
    let name = member.name().map(|n| n.text());
    let Some(command) = member.storage() else {
        let name = name.ok_or_else(|| invalid(range, "Expected an identifier"))?;
        let value = member
            .value()
            .map(|value| expr::expression(&value, known))
            .transpose()?;
        return Ok(Record::Enumerator { name, value });
    };
    let word = command
        .word()
        .map(|w| w.text().to_ascii_lowercase())
        .unwrap_or_default();
    let values: Vec<Expr> = command.values().collect();
    let unit = match word.as_str() {
        ".byte" | ".byt" => 1,
        ".dbyt" | ".word" | ".addr" => 2,
        ".faraddr" => 3,
        ".dword" => 4,
        ".res" => {
            if values.is_empty() {
                return Err(invalid(range, "Expected a byte count"));
            }
            1
        }
        ".org" => {
            let [value] = values.as_slice() else {
                return Err(invalid(range, "Expected an address"));
            };
            let offset = constant(value, known)?;
            if offset > LARGEST {
                return Err(invalid(value.syntax().text_range(), "Range error"));
            }
            return Ok(Record::Offset { name, offset });
        }
        ".tag" => {
            let (record, count) = tag(&command, known)?;
            return Ok(Record::Tag {
                name,
                record,
                count,
            });
        }
        ".struct" | ".union" => {
            return Ok(Record::Open {
                kind: kind_of(&word),
                name: command.name().map(|n| n.text()),
                field: name,
            });
        }
        _ => {
            let message = if name.is_some() {
                "Expected a storage allocator after the field name"
            } else {
                "Expected a storage allocator"
            };
            return Err(invalid(range, message));
        }
    };
    let bytes = match values.first() {
        Some(value) => unit * count(value, known)?,
        None => unit,
    };
    if bytes > LARGEST {
        return Err(invalid(range, "Range error"));
    }
    Ok(Record::Field { name, bytes })
}

/// The record a `.tag` names, and how many of it.
fn tag(command: &Command, known: &dyn Known) -> Result<(String, i64), Diagnostic> {
    let range = command.syntax().text_range();
    let mut values = command.values();
    let Some(Expr::Name(record)) = values.next() else {
        return Err(invalid(range, "Expected an identifier"));
    };
    let count = match values.next() {
        Some(value) => count(&value, known)?,
        None => 1,
    };
    Ok((record.text(), count))
}

/// A count, which must be known now and more than zero.
fn count(value: &Expr, known: &dyn Known) -> Result<i64, Diagnostic> {
    let count = constant(value, known)?;
    if count <= 0 {
        return Err(invalid(value.syntax().text_range(), "Range error"));
    }
    Ok(count)
}

fn constant(value: &Expr, known: &dyn Known) -> Result<i64, Diagnostic> {
    let lowered = expr::expression(value, known)?;
    expr::constant(&lowered, known)
        .ok_or_else(|| invalid(value.syntax().text_range(), "Constant expression expected"))
}
