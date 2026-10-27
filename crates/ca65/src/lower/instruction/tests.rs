//! Instructions lowered, then encoded the way Asm198x's back end encodes
//! them, against the bytes ca65 V2.19 wrote.

use std::collections::HashMap;

use asm198x_ir::{Expr as Ir, Operation};
use isa198x::OperandKind;

use crate::ast::{SourceFile, Statement, View};
use crate::lower::{Known, Size, Value};
use crate::{Profile, parse};

mod forms;

/// What the tests' assembler knows: names, and the address.
#[derive(Default)]
struct Names {
    /// What is known of each name at the line.
    names: HashMap<&'static str, Value>,
    /// Each name's final value, for encoding.
    finals: HashMap<&'static str, i64>,
    pc: Option<Value>,
}

impl Known for Names {
    fn name(&self, name: &str) -> Value {
        self.names.get(name).copied().unwrap_or(Value::Unknown)
    }

    fn pc(&self) -> Value {
        self.pc.unwrap_or(Value::Address(Size::Absolute))
    }
}

/// The one instruction in `source`, lowered.
fn lowered(source: &str, known: &dyn Known) -> Result<Operation, String> {
    let parse = parse(&format!("{source}\n"), Profile::CA65_2_19_6502).expect("parses");
    let line = SourceFile::cast(parse.tree().root())
        .and_then(|file| file.lines().next())
        .expect("a line");
    let Some(Statement::Instruction(instruction)) = line.statement() else {
        return Err(format!("not an instruction: {source}"));
    };
    super::instruction(&instruction, Profile::CA65_2_19_6502, known)
        .map_err(|d| d.message().to_owned())
}

/// Encodes an instruction at 0 as the back end does, with `symbols` giving
/// each name's final value, in hex.
fn encode(op: &Operation, symbols: &dyn Fn(&str) -> Option<i64>) -> Option<String> {
    let Operation::Instruction {
        mnemonic,
        mode,
        operands,
    } = op
    else {
        return None;
    };
    let form = isa198x::mos6502::SET.find_form(mnemonic, mode)?;
    let len: i64 = 1 + form
        .operands
        .iter()
        .map(|o| i64::from(o.bytes))
        .sum::<i64>();
    let mut bytes = form.opcode.to_vec();
    for (slot, expr) in form.operands.iter().zip(operands) {
        let value = expr
            .eval_with(&|name: &str| symbols(name), Some(0), 0)
            .ok()?;
        let value = if slot.kind == OperandKind::RelativePc {
            let offset = value - len;
            (-128..=127).contains(&offset).then_some(offset)?
        } else {
            let limit = 1_i64 << (8 * slot.bytes);
            (0..limit).contains(&value).then_some(value)?
        };
        bytes.extend((0..slot.bytes).map(|i| (value >> (8 * i)) as u8));
    }
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn assembled(source: &str, known: &Names) -> Result<String, String> {
    let symbols = |name: &str| match known.name(name) {
        Value::Constant(value) => Some(value),
        _ => known.finals.get(name).copied(),
    };
    let op = lowered(source, known)?;
    encode(&op, &symbols).ok_or_else(|| "out of range".to_owned())
}

#[test]
fn every_6502_form_assembles_as_ca65_assembles_it() {
    for &(source, mode, hex) in forms::FORMS {
        let op = lowered(source, &Names::default()).expect(source);
        let Operation::Instruction { mode: chosen, .. } = &op else {
            panic!("{source}")
        };
        assert_eq!(*chosen, mode, "{source}");
        assert_eq!(encode(&op, &|_| None).as_deref(), Some(hex), "{source}");
    }
}

const ZERO: Size = Size::Zero;
const ABSOLUTE: Size = Size::Absolute;

/// A name: what is known of it at the line, and its final value.
type Name = (&'static str, Value, i64);

/// Lines measured on ca65 V2.19, with the names around them: what ca65 knew
/// at the line, and their final values. `None` is a line ca65 refused.
const MEASURED: &[(&[Name], &str, Option<&str>)] = &[
    (&[], "lda $12", Some("a512")),
    (&[], "lda $1234", Some("ad3412")),
    (&[], "lda $0012", Some("a512")),
    (&[], "lda $12+$100-$100", Some("a512")),
    (&[], "lda $12, x", Some("b512")),
    (&[], "LDA $12", Some("a512")),
    (&[], "Lda #1", Some("a901")),
    // A name defined further on is absolute, even when it turns out small.
    (&[("fwd", Value::Unknown, 0x12)], "lda fwd", Some("ad1200")),
    (&[("fwd", Value::Unknown, 5)], "lda fwd,x", Some("bd0500")),
    (
        &[("fwd", Value::Constant(0x12), 0x12)],
        "lda fwd",
        Some("a512"),
    ),
    (&[("fwd", Value::Unknown, 0x12)], "lda z:fwd", Some("a512")),
    (&[], "lda a:$12", Some("ad1200")),
    // A label in a segment takes the segment's size; under `.org`, a label
    // is a constant.
    (
        &[("lab", Value::Address(ABSOLUTE), 0)],
        "lda lab",
        Some("ad0000"),
    ),
    (
        &[("ol", Value::Constant(0x10), 0x10)],
        "lda ol",
        Some("a510"),
    ),
    (&[("zl", Value::Address(ZERO), 0)], "lda zl", Some("a500")),
    (&[("zl", Value::Address(ZERO), 0)], "lda zl+1", Some("a501")),
    (&[("zl", Value::Address(ZERO), 0)], "lda zl,x", Some("b500")),
    (&[("zl", Value::Unknown, 0)], "lda zl", Some("ad0000")),
    (
        &[("q", Value::Constant(0x12), 0x12)],
        "lda q+1",
        Some("a513"),
    ),
    (
        &[("q", Value::Constant(0x12), 0x12)],
        "lda q*$100",
        Some("ad0012"),
    ),
    (&[("q2", Value::Constant(10), 10)], "lda q2", Some("a50a")),
    (&[("q", Value::Constant(0x12), 0x12)], "lda q-$13", None),
    (
        &[("lab", Value::Address(ABSOLUTE), 0)],
        "lda <lab",
        Some("a500"),
    ),
    (
        &[("lab", Value::Address(ABSOLUTE), 0)],
        "lda #<lab",
        Some("a900"),
    ),
    (&[], "asl", Some("0a")),
    (&[], "asl a", Some("0a")),
    (&[], "asl $12", Some("0612")),
    (&[], "jmp $12", Some("4c1200")),
    (&[], "jsr $12", Some("201200")),
    (&[], "jmp ($12)", Some("6c1200")),
    (&[], "bne *", Some("d0fe")),
    (&[], "lda #'A'", Some("a941")),
    (&[], "lda #12h", Some("a912")),
    (&[], "lda #0FFh", Some("a9ff")),
    (&[], "lda #%101", Some("a905")),
    (&[], "lda #.lobyte($1234)", Some("a934")),
    (&[], "lda #.hibyte($1234)", Some("a912")),
    (&[], "lda #.bankbyte($123456)", Some("a912")),
    (&[], "lda .loword($123456)", Some("ad5634")),
    (&[], "lda .hiword($12345678)", Some("ad3412")),
    (&[], "lda #.max(3,5)", Some("a905")),
    (&[], "lda #.min(3,5)", Some("a903")),
    (&[], "lda #2 = 2", Some("a901")),
    (&[], "lda #3 < 4", Some("a901")),
    (&[], "lda #!0", Some("a901")),
    (&[], "lda f:$12", None),
    (&[], "lda z:$1234", None),
    (&[], "jmp z:$12", None),
    (&[], "stx $1234,y", None),
    (&[], "bne $1000", None),
    (&[], "lda #$100", None),
    (&[], "lda #-1", None),
    (&[], "lda -1", None),
    (&[], "lda -65537", None),
    (&[], "lda $10000", None),
    (&[], "lda ($1234,x)", None),
    (&[], "lda ($1234),y", None),
    (&[], "lda ($12)", None),
    (&[], "lda [$12]", None),
    (&[], "lda ($12,y)", None),
    (&[], "lda ($12),x", None),
    (&[], "lda $12,a", None),
    (&[], "lda 1 , 2", None),
    (&[], "nop $12", None),
    (&[], "lda", None),
];

#[test]
fn every_measured_line_matches_ca65() {
    for &(names, source, hex) in MEASURED {
        let known = Names {
            names: names.iter().map(|&(n, v, _)| (n, v)).collect(),
            finals: names.iter().map(|&(n, _, f)| (n, f)).collect(),
            pc: None,
        };
        let got = assembled(source, &known);
        assert_eq!(got.as_deref().ok(), hex, "{source}: {got:?}");
    }
}

#[test]
fn the_address_can_be_known() {
    let known = Names {
        pc: Some(Value::Constant(0x10)),
        ..Names::default()
    };
    let op = lowered("lda *", &known).expect("lowers");
    let Operation::Instruction { mode, operands, .. } = op else {
        panic!()
    };
    assert_eq!((mode, operands.as_slice()), ("zeropage", &[Ir::Pc][..]));
    let op = lowered("lda *", &Names::default()).expect("lowers");
    assert!(matches!(
        op,
        Operation::Instruction {
            mode: "absolute",
            ..
        }
    ));
}
