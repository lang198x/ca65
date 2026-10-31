//! Instructions lowered, then encoded the way Asm198x's back end encodes
//! them, against the bytes ca65 V2.19 wrote.

use std::collections::HashMap;

use asm198x_ir::{Expr as Ir, Operation};
use isa198x::OperandKind;

use crate::ast::{SourceFile, Statement, View};
use crate::lower::{Known, Size, Value};
use crate::{Cpu, Profile, parse};

mod forms;
mod forms_4510;
mod forms_relatives;

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
    lowered_on(Profile::CA65_2_19_6502, source, known)
}

/// The one instruction in `source`, lowered under `profile`.
fn lowered_on(profile: Profile, source: &str, known: &dyn Known) -> Result<Operation, String> {
    let parse = parse(&format!("{source}\n"), profile).expect("parses");
    // A line the parser refuses never reaches lowering.
    if let Some(problem) = parse.diagnostics().first() {
        return Err(problem.message().to_owned());
    }
    let line = SourceFile::cast(parse.tree().root())
        .and_then(|file| file.lines().next())
        .expect("a line");
    let Some(Statement::Instruction(instruction)) = line.statement() else {
        return Err(format!("not an instruction: {source}"));
    };
    super::instruction(&instruction, profile, known).map_err(|d| d.message().to_owned())
}

/// Encodes an instruction at 0 as the back end does, with `symbols` giving
/// each name's final value, in hex.
fn encode(op: &Operation, symbols: &dyn Fn(&str) -> Option<i64>) -> Option<String> {
    encode_on(Cpu::Mos6502, op, symbols)
}

/// Encodes an instruction for `cpu` at 0 as the back end does.
fn encode_on(cpu: Cpu, op: &Operation, symbols: &dyn Fn(&str) -> Option<i64>) -> Option<String> {
    let Operation::Instruction {
        mnemonic,
        mode,
        operands,
    } = op
    else {
        return None;
    };
    let form = super::form(cpu, mnemonic, mode)?;
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
            let limit = 1_i64 << (8 * slot.bytes - 1);
            (-limit..limit).contains(&offset).then_some(offset)?
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
    // Measured: an absolute import forced onto the zero page, or a far
    // value forced to absolute, is "Illegal addressing mode".
    (&[("e", Value::Address(ABSOLUTE), 0)], "lda z:e", None),
    (&[], "lda a:$123456", None),
    (&[], "lda z:#$12", None),
    (&[], "lda a:($12,x)", None),
    (&[], "lda z:($12),y", None),
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

#[test]
fn every_4510_line_of_cc65s_opcode_test_assembles_as_ca65_assembles_it() {
    for &(source, hex) in forms_4510::FORMS_4510 {
        let op = lowered_on(Profile::CA65_2_19_4510, source, &Names::default()).expect(source);
        assert_eq!(
            encode_on(Cpu::Csg4510, &op, &|_| None).as_deref(),
            Some(hex),
            "{source}"
        );
    }
}

/// Lines measured on ca65 V2.19 under `.p4510`. `None` is a line ca65
/// refused.
const MEASURED_4510: &[(&str, Option<&str>)] = &[
    // `PHD` is `PHW`, and `INA` and `DEA` are `INC` and `DEC` of the
    // accumulator.
    ("phd #$1234", Some("f43412")),
    ("phd $1234", Some("fc3412")),
    ("ina", Some("1a")),
    ("dea", Some("3a")),
    ("ina a", None),
    ("inc a", Some("1a")),
    ("lda ($12,sp),y", Some("e212")),
    ("lda ($12,SP),Y", Some("e212")),
    ("lda ($12),z", Some("b212")),
    ("sta ($12,s),y", Some("8212")),
    ("jmp ($1234,x)", Some("7c3412")),
    ("jsr ($1234,x)", Some("233412")),
    ("lda ($12,x),y", None),
    ("lda ($12),q", None),
    ("lda ($12),y,z", None),
    ("lda ($12),z,y", None),
    ("lbne $1003", Some("d30110")),
    ("bbr0 $12", None),
    ("bbr0 #1,$10", None),
    ("bbr0 $12,$10,x", None),
    ("bbr0 ($12),$10", Some("0f120d")),
    ("bbr0 ($12,x),$10", None),
    ("bbr0 z:$12,$10", None),
    ("bbr0 $12,($10)", Some("0f120d")),
];

#[test]
fn every_measured_4510_line_matches_ca65() {
    for &(source, hex) in MEASURED_4510 {
        let got = lowered_on(Profile::CA65_2_19_4510, source, &Names::default())
            .ok()
            .and_then(|op| encode_on(Cpu::Csg4510, &op, &|_| None));
        assert_eq!(got.as_deref(), hex, "{source}");
    }
}

/// Lines measured on ca65 V2.19 under `.setcpu "huc6280"` and `.org 0`.
/// `None` is a line ca65 refused, with its message.
const MEASURED_HUC6280: &[(&str, Option<&str>)] = &[
    ("tam0", Some("5301")),
    ("tam7", Some("5380")),
    ("tma3", Some("4308")),
    ("tam3 #1", None), // Expected `end-of-line` but found `#`
    ("tam #$80", Some("5380")),
    ("tma #$04", Some("4304")),
    ("tma #3", None), // Argument of TMA must be a power of two
    ("tma #0", Some("4300")),
    ("tma #$100", None), // Range error (256 not in [0..255])
    ("tst #$12,$EA", Some("8312ea")),
    ("tst #$12,$EAEA", Some("9312eaea")),
    ("tst #$12,$EA,x", Some("a312ea")),
    ("tst #$12,$EAEA,x", Some("b312eaea")),
    ("tst #$12,a:$EA", Some("9312ea00")),
    ("tst #$12,z:$EAEA", None), // Illegal addressing mode
    ("tst #$12,$EA,y", None),   // Illegal addressing mode
    ("tst #$12,($EA)", None),   // Illegal addressing mode
    ("tst $12,$EA", None),      // Invalid addressing mode
    ("tst #$12", None),         // Expected `,` but found `end-of-line`
    ("tst #$12,$EA,x,y", None), // Expected `end-of-line` but found `,`
    ("tst #$123,$EA", None),    // Range error (291 not in [0..255])
    ("tst #$12,$12345", None),  // Illegal addressing mode
    ("tst z:#$12,$EA", None),   // Invalid addressing mode
    ("tii $1234,$5678,$9abc", Some("7334127856bc9a")),
    ("tii ($1234),$5678,($9abc)", Some("7334127856bc9a")),
    ("tii $1234,$5678", None), // Expected `,` but found `end-of-line`
    ("tii $1234,$5678,$9abc,$1", None), // Expected `end-of-line` but found `,`
    ("tii #$1234,$5678,$9abc", None), // Expected an expression but found `#`
    ("tii $12345,$5678,$9abc", None), // Range error (74565 not in [0..65535])
    ("tii $1234,$5678,$1234,x", None), // Expected `end-of-line` but found `,`
    ("tai a:$1234,$5678,$9abc", None), // Expected an expression but found `a:`
    ("st0 #$12", Some("0312")),
    ("st1 #$12", Some("1312")),
    ("st2 #$12", Some("2312")),
    ("bsr 2", Some("4400")),
    ("csl", Some("54")),
    ("csh", Some("d4")),
    ("set", Some("f4")),
    ("sax", Some("22")),
    ("say", Some("42")),
    ("sxy", Some("02")),
    ("cla", Some("62")),
    ("clx", Some("82")),
    ("cly", Some("c2")),
];

#[test]
fn every_measured_huc6280_line_matches_ca65() {
    let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::HuC6280);
    for &(source, hex) in MEASURED_HUC6280 {
        let got = lowered_on(profile, source, &Names::default())
            .ok()
            .and_then(|op| encode_on(Cpu::HuC6280, &op, &|_| None));
        assert_eq!(got.as_deref(), hex, "{source}");
    }
}

#[test]
fn a_4510_instruction_is_not_a_6502_one() {
    assert!(lowered("lda ($12),z", &Names::default()).is_err());
    assert!(lowered("bbr0 $12,$10", &Names::default()).is_err());
    assert!(lowered("lbne $1003", &Names::default()).is_err());
}

#[test]
fn an_instruction_for_a_cpu_not_lowered_yet_is_refused_by_name() {
    let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::Wdc65816);
    let Err(error) = lowered_on(profile, "nop", &Names::default()) else {
        panic!("the 65816 is not lowered yet");
    };
    assert!(error.contains("`65816`"), "{error}");
    assert!(error.contains("not lowered yet"), "{error}");
}

#[test]
fn every_line_of_cc65s_opcode_tests_for_the_6502s_relatives_assembles_as_ca65_assembles_it() {
    let mut wrong = Vec::new();
    for &(cpu, lines) in forms_relatives::FORMS {
        let profile = Profile::CA65_2_19_6502.with_cpu(cpu);
        for &(source, hex) in lines {
            let got = lowered_on(profile, source, &Names::default())
                .map(|op| encode_on(cpu, &op, &|_| None));
            if got.as_ref().ok().and_then(Option::as_deref) != Some(hex) {
                wrong.push(format!("{}: {source}: {got:?}, want {hex}", cpu.name()));
            }
        }
    }
    assert!(wrong.is_empty(), "{}\n{}", wrong.len(), wrong.join("\n"));
}
