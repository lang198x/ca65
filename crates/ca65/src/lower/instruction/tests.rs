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

/// Lines measured on ca65 V2.19 under `.p816` and `.org 0`, with both
/// registers 8 bits wide. `None` is a line ca65 refused, with its message,
/// or one lowering does not handle yet.
const MEASURED_65816: &[(&str, Option<&str>)] = &[
    ("cop", None), // ca65: 02, a form Isa198x lacks; refused as not lowered
    ("cop #$12", Some("0212")),
    ("cop $12", Some("0212")),
    ("wdm", None), // Illegal addressing mode
    ("wdm #$12", Some("4212")),
    ("wdm $12", Some("4212")),
    ("brk", Some("00")),
    ("brk #$12", None), // ca65: 0012, a form Isa198x lacks; refused as not lowered
    ("brk $12", None),  // ca65: 0012, a form Isa198x lacks; refused as not lowered
    ("jsl $1234", Some("22341200")),
    ("jsl $123456", Some("22563412")),
    ("jml $1234", Some("5c341200")),
    ("jml $123456", Some("5c563412")),
    ("jml [$1234]", Some("dc3412")),
    ("jmp [$1234]", Some("dc3412")),
    ("jmp [$12]", Some("dc1200")),
    ("jmp $123456", None), // Illegal addressing mode
    ("jmp f:$1234", None), // Illegal addressing mode
    ("jsr $123456", None), // Illegal addressing mode
    ("jsr ($1234,x)", Some("fc3412")),
    ("lda $123456", Some("af563412")),
    ("lda $123456,x", Some("bf563412")),
    ("lda $123456,y", None), // Range error: constant too large for assumed near address.
    ("lda f:$12", Some("af120000")),
    ("lda f:$12,x", Some("bf120000")),
    ("lda a:$12", Some("ad1200")),
    ("lda [$12]", Some("a712")),
    ("lda [$12],y", Some("b712")),
    ("lda [$1234]", None), // Range error (4660 not in [0..255])
    ("lda $12,s", Some("a312")),
    ("lda ($12,s),y", Some("b312")),
    ("lda $1234,s", None), // Range error (4660 not in [0..255])
    ("sta [$12]", Some("8712")),
    ("pei ($12)", Some("d412")),
    ("pei $12", None), // Illegal addressing mode
    ("pea $1234", Some("f43412")),
    ("pea #$1234", None), // Illegal addressing mode
    ("per 3", Some("620000")),
    ("brl 3", Some("820000")),
    ("mvn $01,$02", Some("540000")),
    ("mvn #$01,#$02", Some("540201")),
    ("mvn $010000,$020000", Some("540201")),
    ("mvp #1,#2", Some("440201")),
    ("mvn $01", None), // Expected `,` but found `end-of-line`
    ("rep #$30", Some("c230")),
    ("sep #$30", Some("e230")),
    ("rep $30", None),    // Illegal addressing mode
    ("lda #$1234", None), // Range error (4660 not in [0..255])
    ("ldx #$1234", None), // Range error (4660 not in [0..255])
    ("bit #$12", Some("8912")),
    ("xba", Some("eb")),
    ("xce", Some("fb")),
    ("tcd", Some("5b")),
    ("rtl", Some("6b")),
    ("stz $123456", None), // Illegal addressing mode
    ("ora ($12)", Some("1212")),
    ("lda ($1234)", None),     // Illegal addressing mode
    ("lda (a:$12),y", None),   // Expected an expression but found `a:`
    ("mvn $01,$02,$03", None), // Expected `end-of-line` but found `,`
    ("mvn z:#$01,#$02", None), // Expected an expression but found `z:`
    ("mvn ($010000),$020000", Some("540201")),
    ("cop #$12,1", None),    // Expected `end-of-line` but found `,`
    ("cop $1234", None),     // Illegal addressing mode
    ("cop ($12)", None),     // Illegal addressing mode
    ("cop z:#$12", None),    // Illegal addressing mode
    ("wdm $12,1", None),     // Illegal addressing mode
    ("jml [$1234],y", None), // Illegal addressing mode
    ("lda [$12],x", None),   // Expected `Y` but found `X`
    ("lda z:[$12]", None),   // Illegal addressing mode
    ("lda ($12,s),x", None), // Expected `Y` but found `X`
    ("lda a:$12,s", None),   // Illegal addressing mode
];

#[test]
fn every_measured_65816_line_matches_ca65() {
    let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::Wdc65816);
    for &(source, hex) in MEASURED_65816 {
        let got = lowered_on(profile, source, &Names::default())
            .ok()
            .and_then(|op| encode_on(Cpu::Wdc65816, &op, &|_| None));
        assert_eq!(got.as_deref(), hex, "{source}");
    }
}

/// Lines measured on ca65 V2.19 under `.setcpu "45GS02"` and `.org 0`:
/// every Q instruction, and some plain ones, with each operand spelling.
/// `None` is a line ca65 refused.
const MEASURED_45GS02: &[(&str, Option<&str>)] = &[
    ("adcq", None),
    ("adcq a", None),
    ("adcq q", None),
    ("adcq $12", Some("42426512")),
    ("adcq $1234", Some("42426d3412")),
    ("adcq $12,x", None),
    ("adcq $1234,x", None),
    ("adcq $12,y", None),
    ("adcq ($12)", Some("42427212")),
    ("adcq ($12),z", Some("42427212")),
    ("adcq ($12),y", None),
    ("adcq ($12,x)", None),
    ("adcq [$12]", Some("4242ea7212")),
    ("adcq [$12],z", None),
    ("adcq #$12", None),
    ("andq", None),
    ("andq a", None),
    ("andq q", None),
    ("andq $12", Some("42422512")),
    ("andq $1234", Some("42422d3412")),
    ("andq $12,x", None),
    ("andq $1234,x", None),
    ("andq $12,y", None),
    ("andq ($12)", Some("42423212")),
    ("andq ($12),z", Some("42423212")),
    ("andq ($12),y", None),
    ("andq ($12,x)", None),
    ("andq [$12]", Some("4242ea3212")),
    ("andq [$12],z", None),
    ("andq #$12", None),
    ("aslq", Some("42420a")),
    ("aslq a", Some("42420a")),
    ("aslq q", Some("42420a")),
    ("aslq $12", Some("42420612")),
    ("aslq $1234", Some("42420e3412")),
    ("aslq $12,x", Some("42421612")),
    ("aslq $1234,x", Some("42421e3412")),
    ("aslq $12,y", None),
    ("aslq ($12)", None),
    ("aslq ($12),z", None),
    ("aslq ($12),y", None),
    ("aslq ($12,x)", None),
    ("aslq [$12]", None),
    ("aslq [$12],z", None),
    ("aslq #$12", None),
    ("asrq", Some("424243")),
    ("asrq a", Some("424243")),
    ("asrq q", Some("424243")),
    ("asrq $12", Some("42424412")),
    ("asrq $1234", None),
    ("asrq $12,x", Some("42425412")),
    ("asrq $1234,x", None),
    ("asrq $12,y", None),
    ("asrq ($12)", None),
    ("asrq ($12),z", None),
    ("asrq ($12),y", None),
    ("asrq ($12,x)", None),
    ("asrq [$12]", None),
    ("asrq [$12],z", None),
    ("asrq #$12", None),
    ("bitq", None),
    ("bitq a", None),
    ("bitq q", None),
    ("bitq $12", Some("42422412")),
    ("bitq $1234", Some("42422c3412")),
    ("bitq $12,x", None),
    ("bitq $1234,x", None),
    ("bitq $12,y", None),
    ("bitq ($12)", None),
    ("bitq ($12),z", None),
    ("bitq ($12),y", None),
    ("bitq ($12,x)", None),
    ("bitq [$12]", None),
    ("bitq [$12],z", None),
    ("bitq #$12", None),
    ("cmpq", None),
    ("cmpq a", None),
    ("cmpq q", None),
    ("cmpq $12", Some("4242c512")),
    ("cmpq $1234", Some("4242cd3412")),
    ("cmpq $12,x", None),
    ("cmpq $1234,x", None),
    ("cmpq $12,y", None),
    ("cmpq ($12)", Some("4242d212")),
    ("cmpq ($12),z", Some("4242d212")),
    ("cmpq ($12),y", None),
    ("cmpq ($12,x)", None),
    ("cmpq [$12]", Some("4242ead212")),
    ("cmpq [$12],z", None),
    ("cmpq #$12", None),
    ("deq", Some("42423a")),
    ("deq a", Some("42423a")),
    ("deq q", Some("42423a")),
    ("deq $12", Some("4242c612")),
    ("deq $1234", Some("4242ce3412")),
    ("deq $12,x", Some("4242d612")),
    ("deq $1234,x", Some("4242de3412")),
    ("deq $12,y", None),
    ("deq ($12)", None),
    ("deq ($12),z", None),
    ("deq ($12),y", None),
    ("deq ($12,x)", None),
    ("deq [$12]", None),
    ("deq [$12],z", None),
    ("deq #$12", None),
    ("eorq", None),
    ("eorq a", None),
    ("eorq q", None),
    ("eorq $12", Some("42424512")),
    ("eorq $1234", Some("42424d3412")),
    ("eorq $12,x", None),
    ("eorq $1234,x", None),
    ("eorq $12,y", None),
    ("eorq ($12)", Some("42425212")),
    ("eorq ($12),z", Some("42425212")),
    ("eorq ($12),y", None),
    ("eorq ($12,x)", None),
    ("eorq [$12]", Some("4242ea5212")),
    ("eorq [$12],z", None),
    ("eorq #$12", None),
    ("inq", Some("42421a")),
    ("inq a", Some("42421a")),
    ("inq q", Some("42421a")),
    ("inq $12", Some("4242e612")),
    ("inq $1234", Some("4242ee3412")),
    ("inq $12,x", Some("4242f612")),
    ("inq $1234,x", Some("4242fe3412")),
    ("inq $12,y", None),
    ("inq ($12)", None),
    ("inq ($12),z", None),
    ("inq ($12),y", None),
    ("inq ($12,x)", None),
    ("inq [$12]", None),
    ("inq [$12],z", None),
    ("inq #$12", None),
    ("ldq", None),
    ("ldq a", None),
    ("ldq q", None),
    ("ldq $12", Some("4242a512")),
    ("ldq $1234", Some("4242ad3412")),
    ("ldq $12,x", None),
    ("ldq $1234,x", None),
    ("ldq $12,y", None),
    ("ldq ($12)", Some("4242b212")),
    ("ldq ($12),z", Some("4242b212")),
    ("ldq ($12),y", None),
    ("ldq ($12,x)", None),
    ("ldq [$12]", Some("4242eab212")),
    ("ldq [$12],z", Some("4242eab212")),
    ("ldq #$12", None),
    ("lsrq", Some("42424a")),
    ("lsrq a", Some("42424a")),
    ("lsrq q", Some("42424a")),
    ("lsrq $12", Some("42424612")),
    ("lsrq $1234", Some("42424e3412")),
    ("lsrq $12,x", Some("42425612")),
    ("lsrq $1234,x", Some("42425e3412")),
    ("lsrq $12,y", None),
    ("lsrq ($12)", None),
    ("lsrq ($12),z", None),
    ("lsrq ($12),y", None),
    ("lsrq ($12,x)", None),
    ("lsrq [$12]", None),
    ("lsrq [$12],z", None),
    ("lsrq #$12", None),
    ("orq", None),
    ("orq a", None),
    ("orq q", None),
    ("orq $12", Some("42420512")),
    ("orq $1234", Some("42420d3412")),
    ("orq $12,x", None),
    ("orq $1234,x", None),
    ("orq $12,y", None),
    ("orq ($12)", Some("42421212")),
    ("orq ($12),z", Some("42421212")),
    ("orq ($12),y", None),
    ("orq ($12,x)", None),
    ("orq [$12]", Some("4242ea1212")),
    ("orq [$12],z", None),
    ("orq #$12", None),
    ("rolq", Some("42422a")),
    ("rolq a", Some("42422a")),
    ("rolq q", Some("42422a")),
    ("rolq $12", Some("42422612")),
    ("rolq $1234", Some("42422e3412")),
    ("rolq $12,x", Some("42423612")),
    ("rolq $1234,x", Some("42423e3412")),
    ("rolq $12,y", None),
    ("rolq ($12)", None),
    ("rolq ($12),z", None),
    ("rolq ($12),y", None),
    ("rolq ($12,x)", None),
    ("rolq [$12]", None),
    ("rolq [$12],z", None),
    ("rolq #$12", None),
    ("rorq", Some("42426a")),
    ("rorq a", Some("42426a")),
    ("rorq q", Some("42426a")),
    ("rorq $12", Some("42426612")),
    ("rorq $1234", Some("42426e3412")),
    ("rorq $12,x", Some("42427612")),
    ("rorq $1234,x", Some("42427e3412")),
    ("rorq $12,y", None),
    ("rorq ($12)", None),
    ("rorq ($12),z", None),
    ("rorq ($12),y", None),
    ("rorq ($12,x)", None),
    ("rorq [$12]", None),
    ("rorq [$12],z", None),
    ("rorq #$12", None),
    ("sbcq", None),
    ("sbcq a", None),
    ("sbcq q", None),
    ("sbcq $12", Some("4242e512")),
    ("sbcq $1234", Some("4242ed3412")),
    ("sbcq $12,x", None),
    ("sbcq $1234,x", None),
    ("sbcq $12,y", None),
    ("sbcq ($12)", Some("4242f212")),
    ("sbcq ($12),z", Some("4242f212")),
    ("sbcq ($12),y", None),
    ("sbcq ($12,x)", None),
    ("sbcq [$12]", Some("4242eaf212")),
    ("sbcq [$12],z", None),
    ("sbcq #$12", None),
    ("stq", None),
    ("stq a", None),
    ("stq q", None),
    ("stq $12", Some("42428512")),
    ("stq $1234", Some("42428d3412")),
    ("stq $12,x", None),
    ("stq $1234,x", None),
    ("stq $12,y", None),
    ("stq ($12)", Some("42429212")),
    ("stq ($12),z", Some("42429212")),
    ("stq ($12),y", None),
    ("stq ($12,x)", None),
    ("stq [$12]", Some("4242ea9212")),
    ("stq [$12],z", None),
    ("stq #$12", None),
    ("lda", None),
    ("lda a", None),
    ("lda q", None),
    ("lda $12", Some("a512")),
    ("lda $1234", Some("ad3412")),
    ("lda $12,x", Some("b512")),
    ("lda $1234,x", Some("bd3412")),
    ("lda $12,y", Some("b91200")),
    ("lda ($12)", Some("b212")),
    ("lda ($12),z", Some("b212")),
    ("lda ($12),y", Some("b112")),
    ("lda ($12,x)", Some("a112")),
    ("lda [$12]", None),
    ("lda [$12],z", Some("eab212")),
    ("lda #$12", Some("a912")),
    ("sta", None),
    ("sta a", None),
    ("sta q", None),
    ("sta $12", Some("8512")),
    ("sta $1234", Some("8d3412")),
    ("sta $12,x", Some("9512")),
    ("sta $1234,x", Some("9d3412")),
    ("sta $12,y", Some("991200")),
    ("sta ($12)", Some("9212")),
    ("sta ($12),z", Some("9212")),
    ("sta ($12),y", Some("9112")),
    ("sta ($12,x)", Some("8112")),
    ("sta [$12]", None),
    ("sta [$12],z", Some("ea9212")),
    ("sta #$12", None),
    ("adc", None),
    ("adc a", None),
    ("adc q", None),
    ("adc $12", Some("6512")),
    ("adc $1234", Some("6d3412")),
    ("adc $12,x", Some("7512")),
    ("adc $1234,x", Some("7d3412")),
    ("adc $12,y", Some("791200")),
    ("adc ($12)", Some("7212")),
    ("adc ($12),z", Some("7212")),
    ("adc ($12),y", Some("7112")),
    ("adc ($12,x)", Some("6112")),
    ("adc [$12]", None),
    ("adc [$12],z", Some("ea7212")),
    ("adc #$12", Some("6912")),
    ("ora", None),
    ("ora a", None),
    ("ora q", None),
    ("ora $12", Some("0512")),
    ("ora $1234", Some("0d3412")),
    ("ora $12,x", Some("1512")),
    ("ora $1234,x", Some("1d3412")),
    ("ora $12,y", Some("191200")),
    ("ora ($12)", Some("1212")),
    ("ora ($12),z", Some("1212")),
    ("ora ($12),y", Some("1112")),
    ("ora ($12,x)", Some("0112")),
    ("ora [$12]", None),
    ("ora [$12],z", Some("ea1212")),
    ("ora #$12", Some("0912")),
    ("aslq q", Some("42420a")),
    ("inq q", Some("42421a")),
    ("aslq a", Some("42420a")),
    ("ldq ($12)", Some("4242b212")),
    ("stq ($12),z", Some("42429212")),
    ("adcq ($12),z", Some("42427212")),
    ("lda [$12]", None),    // Illegal addressing mode
    ("lda [$12],y", None),  // Expected `Z` but found `Y`
    ("adcq #1", None),      // Illegal addressing mode
    ("ldq #1", None),       // Illegal addressing mode
    ("ldq $12,x", None),    // Illegal addressing mode
    ("stq $1234,y", None),  // Illegal addressing mode
    ("adcq [$12],z", None), // Illegal addressing mode
    ("ldq [$12]", Some("4242eab212")),
    ("lda [$1234],z", None), // Range error (4660 not in [0..255])
    ("sta z:[$12],z", None), // Illegal addressing mode
    ("neg", Some("42")),
    ("neg a", None), // Illegal addressing mode
    ("phd #$1234", Some("f43412")),
    ("eom", Some("ea")),
    ("map", Some("5c")),
];

#[test]
fn every_measured_45gs02_line_matches_ca65() {
    let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::Mega45gs02);
    for &(source, hex) in MEASURED_45GS02 {
        let got = lowered_on(profile, source, &Names::default())
            .ok()
            .and_then(|op| encode_on(Cpu::Mega45gs02, &op, &|_| None));
        assert_eq!(got.as_deref(), hex, "{source}");
    }
}

#[test]
fn a_plain_zero_page_indirect_is_z_indexed_but_on_the_4510() {
    // Measured on ca65 V2.19: `lda ($12)` is `b2 12` on the 65CE02 and
    // 45GS02, and "Illegal addressing mode" on the 4510.
    for (cpu, hex) in [
        (Cpu::Csg65ce02, Some("b212")),
        (Cpu::Mega45gs02, Some("b212")),
        (Cpu::Csg4510, None),
    ] {
        let profile = Profile::CA65_2_19_6502.with_cpu(cpu);
        let got = lowered_on(profile, "lda ($12)", &Names::default())
            .ok()
            .and_then(|op| encode_on(cpu, &op, &|_| None));
        assert_eq!(got.as_deref(), hex, "{cpu:?}");
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
    let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::M740);
    let Err(error) = lowered_on(profile, "nop", &Names::default()) else {
        panic!("the M740 is not lowered yet");
    };
    assert!(error.contains("`m740`"), "{error}");
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
