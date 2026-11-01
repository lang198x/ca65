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

/// Every code of the 740 family manual's opcode map, written as ca65 V2.19
/// writes it, and some more lines, under `.setcpu "m740"` and `.org 0`, with
/// the bytes ca65 made. Where ca65 contradicts the manual (`INC A`, `DEC A`,
/// `LDM` and the accumulator bit branches), the bytes are the manual's, and
/// the comment gives ca65's. `None` is a line ca65 refused, or one lowering
/// leaves alone.
const MEASURED_M740: &[(&str, Option<&str>)] = &[
    ("brk", Some("00")),
    ("ora ($12,x)", Some("0112")),
    ("jsr ($12)", Some("0212")),
    ("bbs0 a, 2", Some("0300")), // ca65: 03ff; the manual counts from * + 2
    ("ora $12", Some("0512")),
    ("asl $12", Some("0612")),
    ("bbs0 $12, 3", Some("071200")),
    ("php", Some("08")),
    ("ora #$12", Some("0912")),
    ("asl a", Some("0a")),
    ("seb0 a", Some("0b")),
    ("ora $3456", Some("0d5634")),
    ("asl $3456", Some("0e5634")),
    ("seb0 $12", Some("0f12")),
    ("bpl 2", Some("1000")),
    ("ora ($12),y", Some("1112")),
    ("clt", Some("12")),
    ("bbc0 a, 2", Some("1300")), // ca65: 13ff; the manual counts from * + 2
    ("ora $12,x", Some("1512")),
    ("asl $12,x", Some("1612")),
    ("bbc0 $12, 3", Some("171200")),
    ("clc", Some("18")),
    ("ora $3456,y", Some("195634")),
    ("dec a", Some("1a")), // ca65: 3a; the manual gives $1A
    ("clb0 a", Some("1b")),
    ("ora $3456,x", Some("1d5634")),
    ("asl $3456,x", Some("1e5634")),
    ("clb0 $12", Some("1f12")),
    ("jsr $3456", Some("205634")),
    ("and ($12,x)", Some("2112")),
    ("jsr $FF12", Some("2212")),
    ("bbs1 a, 2", Some("2300")), // ca65: 23ff; the manual counts from * + 2
    ("bit $12", Some("2412")),
    ("and $12", Some("2512")),
    ("rol $12", Some("2612")),
    ("bbs1 $12, 3", Some("271200")),
    ("plp", Some("28")),
    ("and #$12", Some("2912")),
    ("rol a", Some("2a")),
    ("seb1 a", Some("2b")),
    ("bit $3456", Some("2c5634")),
    ("and $3456", Some("2d5634")),
    ("rol $3456", Some("2e5634")),
    ("seb1 $12", Some("2f12")),
    ("bmi 2", Some("3000")),
    ("and ($12),y", Some("3112")),
    ("set", Some("32")),
    ("bbc1 a, 2", Some("3300")), // ca65: 33ff; the manual counts from * + 2
    ("and $12,x", Some("3512")),
    ("rol $12,x", Some("3612")),
    ("bbc1 $12, 3", Some("371200")),
    ("sec", Some("38")),
    ("and $3456,y", Some("395634")),
    ("inc a", Some("3a")), // ca65: 1a; the manual gives $3A
    ("clb1 a", Some("3b")),
    ("ldm $12, #$34", Some("3c3412")), // ca65: 3c1234; the manual lays the value first
    ("and $3456,x", Some("3d5634")),
    ("rol $3456,x", Some("3e5634")),
    ("clb1 $12", Some("3f12")),
    ("rti", Some("40")),
    ("eor ($12,x)", Some("4112")),
    ("stp", Some("42")),
    ("bbs2 a, 2", Some("4300")), // ca65: 43ff; the manual counts from * + 2
    ("com $12", Some("4412")),
    ("eor $12", Some("4512")),
    ("lsr $12", Some("4612")),
    ("bbs2 $12, 3", Some("471200")),
    ("pha", Some("48")),
    ("eor #$12", Some("4912")),
    ("lsr a", Some("4a")),
    ("seb2 a", Some("4b")),
    ("jmp $3456", Some("4c5634")),
    ("eor $3456", Some("4d5634")),
    ("lsr $3456", Some("4e5634")),
    ("seb2 $12", Some("4f12")),
    ("bvc 2", Some("5000")),
    ("eor ($12),y", Some("5112")),
    ("bbc2 a, 2", Some("5300")), // ca65: 53ff; the manual counts from * + 2
    ("eor $12,x", Some("5512")),
    ("lsr $12,x", Some("5612")),
    ("bbc2 $12, 3", Some("571200")),
    ("cli", Some("58")),
    ("eor $3456,y", Some("595634")),
    ("clb2 a", Some("5b")),
    ("eor $3456,x", Some("5d5634")),
    ("lsr $3456,x", Some("5e5634")),
    ("clb2 $12", Some("5f12")),
    ("rts", Some("60")),
    ("adc ($12,x)", Some("6112")),
    ("mul $12,x", None),         // ca65 has no such mnemonic
    ("bbs3 a, 2", Some("6300")), // ca65: 63ff; the manual counts from * + 2
    ("tst $12", Some("6412")),
    ("adc $12", Some("6512")),
    ("ror $12", Some("6612")),
    ("bbs3 $12, 3", Some("671200")),
    ("pla", Some("68")),
    ("adc #$12", Some("6912")),
    ("ror a", Some("6a")),
    ("seb3 a", Some("6b")),
    ("jmp ($3456)", Some("6c5634")),
    ("adc $3456", Some("6d5634")),
    ("ror $3456", Some("6e5634")),
    ("seb3 $12", Some("6f12")),
    ("bvs 2", Some("7000")),
    ("adc ($12),y", Some("7112")),
    ("bbc3 a, 2", Some("7300")), // ca65: 73ff; the manual counts from * + 2
    ("adc $12,x", Some("7512")),
    ("ror $12,x", Some("7612")),
    ("bbc3 $12, 3", Some("771200")),
    ("sei", Some("78")),
    ("adc $3456,y", Some("795634")),
    ("clb3 a", Some("7b")),
    ("adc $3456,x", Some("7d5634")),
    ("ror $3456,x", Some("7e5634")),
    ("clb3 $12", Some("7f12")),
    ("bra 2", Some("8000")),
    ("sta ($12,x)", Some("8112")),
    ("rrf $12", Some("8212")),
    ("bbs4 a, 2", Some("8300")), // ca65: 83ff; the manual counts from * + 2
    ("sty $12", Some("8412")),
    ("sta $12", Some("8512")),
    ("stx $12", Some("8612")),
    ("bbs4 $12, 3", Some("871200")),
    ("dey", Some("88")),
    ("txa", Some("8a")),
    ("seb4 a", Some("8b")),
    ("sty $3456", Some("8c5634")),
    ("sta $3456", Some("8d5634")),
    ("stx $3456", Some("8e5634")),
    ("seb4 $12", Some("8f12")),
    ("bcc 2", Some("9000")),
    ("sta ($12),y", Some("9112")),
    ("bbc4 a, 2", Some("9300")), // ca65: 93ff; the manual counts from * + 2
    ("sty $12,x", Some("9412")),
    ("sta $12,x", Some("9512")),
    ("stx $12,y", Some("9612")),
    ("bbc4 $12, 3", Some("971200")),
    ("tya", Some("98")),
    ("sta $3456,y", Some("995634")),
    ("txs", Some("9a")),
    ("clb4 a", Some("9b")),
    ("sta $3456,x", Some("9d5634")),
    ("clb4 $12", Some("9f12")),
    ("ldy #$12", Some("a012")),
    ("lda ($12,x)", Some("a112")),
    ("ldx #$12", Some("a212")),
    ("bbs5 a, 2", Some("a300")), // ca65: a3ff; the manual counts from * + 2
    ("ldy $12", Some("a412")),
    ("lda $12", Some("a512")),
    ("ldx $12", Some("a612")),
    ("bbs5 $12, 3", Some("a71200")),
    ("tay", Some("a8")),
    ("lda #$12", Some("a912")),
    ("tax", Some("aa")),
    ("seb5 a", Some("ab")),
    ("ldy $3456", Some("ac5634")),
    ("lda $3456", Some("ad5634")),
    ("ldx $3456", Some("ae5634")),
    ("seb5 $12", Some("af12")),
    ("bcs 2", Some("b000")),
    ("lda ($12),y", Some("b112")),
    ("jmp ($12)", Some("b212")),
    ("bbc5 a, 2", Some("b300")), // ca65: b3ff; the manual counts from * + 2
    ("ldy $12,x", Some("b412")),
    ("lda $12,x", Some("b512")),
    ("ldx $12,y", Some("b612")),
    ("bbc5 $12, 3", Some("b71200")),
    ("clv", Some("b8")),
    ("lda $3456,y", Some("b95634")),
    ("tsx", Some("ba")),
    ("clb5 a", Some("bb")),
    ("ldy $3456,x", Some("bc5634")),
    ("lda $3456,x", Some("bd5634")),
    ("ldx $3456,y", Some("be5634")),
    ("clb5 $12", Some("bf12")),
    ("cpy #$12", Some("c012")),
    ("cmp ($12,x)", Some("c112")),
    ("wit", None),               // ca65 has no such mnemonic
    ("bbs6 a, 2", Some("c300")), // ca65: c3ff; the manual counts from * + 2
    ("cpy $12", Some("c412")),
    ("cmp $12", Some("c512")),
    ("dec $12", Some("c612")),
    ("bbs6 $12, 3", Some("c71200")),
    ("iny", Some("c8")),
    ("cmp #$12", Some("c912")),
    ("dex", Some("ca")),
    ("seb6 a", Some("cb")),
    ("cpy $3456", Some("cc5634")),
    ("cmp $3456", Some("cd5634")),
    ("dec $3456", Some("ce5634")),
    ("seb6 $12", Some("cf12")),
    ("bne 2", Some("d000")),
    ("cmp ($12),y", Some("d112")),
    ("bbc6 a, 2", Some("d300")), // ca65: d3ff; the manual counts from * + 2
    ("cmp $12,x", Some("d512")),
    ("dec $12,x", Some("d612")),
    ("bbc6 $12, 3", Some("d71200")),
    ("cld", Some("d8")),
    ("cmp $3456,y", Some("d95634")),
    ("clb6 a", Some("db")),
    ("cmp $3456,x", Some("dd5634")),
    ("dec $3456,x", Some("de5634")),
    ("clb6 $12", Some("df12")),
    ("cpx #$12", Some("e012")),
    ("sbc ($12,x)", Some("e112")),
    ("div $12,x", None),         // ca65 has no such mnemonic
    ("bbs7 a, 2", Some("e300")), // ca65: e3ff; the manual counts from * + 2
    ("cpx $12", Some("e412")),
    ("sbc $12", Some("e512")),
    ("inc $12", Some("e612")),
    ("bbs7 $12, 3", Some("e71200")),
    ("inx", Some("e8")),
    ("sbc #$12", Some("e912")),
    ("nop", Some("ea")),
    ("seb7 a", Some("eb")),
    ("cpx $3456", Some("ec5634")),
    ("sbc $3456", Some("ed5634")),
    ("inc $3456", Some("ee5634")),
    ("seb7 $12", Some("ef12")),
    ("beq 2", Some("f000")),
    ("sbc ($12),y", Some("f112")),
    ("bbc7 a, 2", Some("f300")), // ca65: f3ff; the manual counts from * + 2
    ("sbc $12,x", Some("f512")),
    ("inc $12,x", Some("f612")),
    ("bbc7 $12, 3", Some("f71200")),
    ("sed", Some("f8")),
    ("sbc $3456,y", Some("f95634")),
    ("clb7 a", Some("fb")),
    ("sbc $3456,x", Some("fd5634")),
    ("inc $3456,x", Some("fe5634")),
    ("clb7 $12", Some("ff12")),
    ("rmb0 $12", Some("1f12")),
    ("rmb7 $12", Some("ff12")),
    ("fst", None), // ca65: e2; not lowered: neither manual describes it
    ("slw", None), // ca65: c2; not lowered: neither manual describes it
    ("jsr $FF12", Some("2212")),
    ("jsr $FEFF", Some("20fffe")),
    ("jmp ($12)", Some("b212")),
    ("jmp ($3456)", Some("6c5634")),
    ("jsr ($3456)", None),
    ("seb0 $1234", None),
    ("clb3 a", Some("7b")),
    ("bit #1", None),
    ("stz $12", None),
    ("ldm #$34, $12", None),
    ("ldm $12, $34", None),
    ("bbs0 a", None),
    ("bbs0 $1234, 3", None),
    ("seb0 #1", None),
    ("jsr a:$FF12", Some("2212")),
    ("smb0 $12", None),
];

#[test]
fn every_measured_m740_line_matches_the_manual_or_ca65() {
    let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::M740);
    for &(source, hex) in MEASURED_M740 {
        let got = lowered_on(profile, source, &Names::default())
            .ok()
            .and_then(|op| encode_on(Cpu::M740, &op, &|_| None));
        assert_eq!(got.as_deref(), hex, "{source}");
    }
}

/// Every SWEET 16 instruction with every register, and some more lines,
/// measured on ca65 V2.19 under `.setcpu "sweet16"` and `.org 0`. `None` is
/// a line ca65 refused.
const MEASURED_SWEET16: &[(&str, Option<&str>)] = &[
    ("set r0, $1234", Some("103412")),
    ("set r1, $1234", Some("113412")),
    ("set r2, $1234", Some("123412")),
    ("set r3, $1234", Some("133412")),
    ("set r4, $1234", Some("143412")),
    ("set r5, $1234", Some("153412")),
    ("set r6, $1234", Some("163412")),
    ("set r7, $1234", Some("173412")),
    ("set r8, $1234", Some("183412")),
    ("set r9, $1234", Some("193412")),
    ("set r10, $1234", Some("1a3412")),
    ("set r11, $1234", Some("1b3412")),
    ("set r12, $1234", Some("1c3412")),
    ("set r13, $1234", Some("1d3412")),
    ("set r14, $1234", Some("1e3412")),
    ("set r15, $1234", Some("1f3412")),
    ("ld r0", Some("20")),
    ("ld r1", Some("21")),
    ("ld r2", Some("22")),
    ("ld r3", Some("23")),
    ("ld r4", Some("24")),
    ("ld r5", Some("25")),
    ("ld r6", Some("26")),
    ("ld r7", Some("27")),
    ("ld r8", Some("28")),
    ("ld r9", Some("29")),
    ("ld r10", Some("2a")),
    ("ld r11", Some("2b")),
    ("ld r12", Some("2c")),
    ("ld r13", Some("2d")),
    ("ld r14", Some("2e")),
    ("ld r15", Some("2f")),
    ("st r0", Some("30")),
    ("st r1", Some("31")),
    ("st r2", Some("32")),
    ("st r3", Some("33")),
    ("st r4", Some("34")),
    ("st r5", Some("35")),
    ("st r6", Some("36")),
    ("st r7", Some("37")),
    ("st r8", Some("38")),
    ("st r9", Some("39")),
    ("st r10", Some("3a")),
    ("st r11", Some("3b")),
    ("st r12", Some("3c")),
    ("st r13", Some("3d")),
    ("st r14", Some("3e")),
    ("st r15", Some("3f")),
    ("ld @r0", Some("40")),
    ("ld @r1", Some("41")),
    ("ld @r2", Some("42")),
    ("ld @r3", Some("43")),
    ("ld @r4", Some("44")),
    ("ld @r5", Some("45")),
    ("ld @r6", Some("46")),
    ("ld @r7", Some("47")),
    ("ld @r8", Some("48")),
    ("ld @r9", Some("49")),
    ("ld @r10", Some("4a")),
    ("ld @r11", Some("4b")),
    ("ld @r12", Some("4c")),
    ("ld @r13", Some("4d")),
    ("ld @r14", Some("4e")),
    ("ld @r15", Some("4f")),
    ("st @r0", Some("50")),
    ("st @r1", Some("51")),
    ("st @r2", Some("52")),
    ("st @r3", Some("53")),
    ("st @r4", Some("54")),
    ("st @r5", Some("55")),
    ("st @r6", Some("56")),
    ("st @r7", Some("57")),
    ("st @r8", Some("58")),
    ("st @r9", Some("59")),
    ("st @r10", Some("5a")),
    ("st @r11", Some("5b")),
    ("st @r12", Some("5c")),
    ("st @r13", Some("5d")),
    ("st @r14", Some("5e")),
    ("st @r15", Some("5f")),
    ("ldd @r0", Some("60")),
    ("ldd @r1", Some("61")),
    ("ldd @r2", Some("62")),
    ("ldd @r3", Some("63")),
    ("ldd @r4", Some("64")),
    ("ldd @r5", Some("65")),
    ("ldd @r6", Some("66")),
    ("ldd @r7", Some("67")),
    ("ldd @r8", Some("68")),
    ("ldd @r9", Some("69")),
    ("ldd @r10", Some("6a")),
    ("ldd @r11", Some("6b")),
    ("ldd @r12", Some("6c")),
    ("ldd @r13", Some("6d")),
    ("ldd @r14", Some("6e")),
    ("ldd @r15", Some("6f")),
    ("std @r0", Some("70")),
    ("std @r1", Some("71")),
    ("std @r2", Some("72")),
    ("std @r3", Some("73")),
    ("std @r4", Some("74")),
    ("std @r5", Some("75")),
    ("std @r6", Some("76")),
    ("std @r7", Some("77")),
    ("std @r8", Some("78")),
    ("std @r9", Some("79")),
    ("std @r10", Some("7a")),
    ("std @r11", Some("7b")),
    ("std @r12", Some("7c")),
    ("std @r13", Some("7d")),
    ("std @r14", Some("7e")),
    ("std @r15", Some("7f")),
    ("pop @r0", Some("80")),
    ("pop @r1", Some("81")),
    ("pop @r2", Some("82")),
    ("pop @r3", Some("83")),
    ("pop @r4", Some("84")),
    ("pop @r5", Some("85")),
    ("pop @r6", Some("86")),
    ("pop @r7", Some("87")),
    ("pop @r8", Some("88")),
    ("pop @r9", Some("89")),
    ("pop @r10", Some("8a")),
    ("pop @r11", Some("8b")),
    ("pop @r12", Some("8c")),
    ("pop @r13", Some("8d")),
    ("pop @r14", Some("8e")),
    ("pop @r15", Some("8f")),
    ("stp @r0", Some("90")),
    ("stp @r1", Some("91")),
    ("stp @r2", Some("92")),
    ("stp @r3", Some("93")),
    ("stp @r4", Some("94")),
    ("stp @r5", Some("95")),
    ("stp @r6", Some("96")),
    ("stp @r7", Some("97")),
    ("stp @r8", Some("98")),
    ("stp @r9", Some("99")),
    ("stp @r10", Some("9a")),
    ("stp @r11", Some("9b")),
    ("stp @r12", Some("9c")),
    ("stp @r13", Some("9d")),
    ("stp @r14", Some("9e")),
    ("stp @r15", Some("9f")),
    ("add r0", Some("a0")),
    ("add r1", Some("a1")),
    ("add r2", Some("a2")),
    ("add r3", Some("a3")),
    ("add r4", Some("a4")),
    ("add r5", Some("a5")),
    ("add r6", Some("a6")),
    ("add r7", Some("a7")),
    ("add r8", Some("a8")),
    ("add r9", Some("a9")),
    ("add r10", Some("aa")),
    ("add r11", Some("ab")),
    ("add r12", Some("ac")),
    ("add r13", Some("ad")),
    ("add r14", Some("ae")),
    ("add r15", Some("af")),
    ("sub r0", Some("b0")),
    ("sub r1", Some("b1")),
    ("sub r2", Some("b2")),
    ("sub r3", Some("b3")),
    ("sub r4", Some("b4")),
    ("sub r5", Some("b5")),
    ("sub r6", Some("b6")),
    ("sub r7", Some("b7")),
    ("sub r8", Some("b8")),
    ("sub r9", Some("b9")),
    ("sub r10", Some("ba")),
    ("sub r11", Some("bb")),
    ("sub r12", Some("bc")),
    ("sub r13", Some("bd")),
    ("sub r14", Some("be")),
    ("sub r15", Some("bf")),
    ("popd @r0", Some("c0")),
    ("popd @r1", Some("c1")),
    ("popd @r2", Some("c2")),
    ("popd @r3", Some("c3")),
    ("popd @r4", Some("c4")),
    ("popd @r5", Some("c5")),
    ("popd @r6", Some("c6")),
    ("popd @r7", Some("c7")),
    ("popd @r8", Some("c8")),
    ("popd @r9", Some("c9")),
    ("popd @r10", Some("ca")),
    ("popd @r11", Some("cb")),
    ("popd @r12", Some("cc")),
    ("popd @r13", Some("cd")),
    ("popd @r14", Some("ce")),
    ("popd @r15", Some("cf")),
    ("cpr r0", Some("d0")),
    ("cpr r1", Some("d1")),
    ("cpr r2", Some("d2")),
    ("cpr r3", Some("d3")),
    ("cpr r4", Some("d4")),
    ("cpr r5", Some("d5")),
    ("cpr r6", Some("d6")),
    ("cpr r7", Some("d7")),
    ("cpr r8", Some("d8")),
    ("cpr r9", Some("d9")),
    ("cpr r10", Some("da")),
    ("cpr r11", Some("db")),
    ("cpr r12", Some("dc")),
    ("cpr r13", Some("dd")),
    ("cpr r14", Some("de")),
    ("cpr r15", Some("df")),
    ("inr r0", Some("e0")),
    ("inr r1", Some("e1")),
    ("inr r2", Some("e2")),
    ("inr r3", Some("e3")),
    ("inr r4", Some("e4")),
    ("inr r5", Some("e5")),
    ("inr r6", Some("e6")),
    ("inr r7", Some("e7")),
    ("inr r8", Some("e8")),
    ("inr r9", Some("e9")),
    ("inr r10", Some("ea")),
    ("inr r11", Some("eb")),
    ("inr r12", Some("ec")),
    ("inr r13", Some("ed")),
    ("inr r14", Some("ee")),
    ("inr r15", Some("ef")),
    ("dcr r0", Some("f0")),
    ("dcr r1", Some("f1")),
    ("dcr r2", Some("f2")),
    ("dcr r3", Some("f3")),
    ("dcr r4", Some("f4")),
    ("dcr r5", Some("f5")),
    ("dcr r6", Some("f6")),
    ("dcr r7", Some("f7")),
    ("dcr r8", Some("f8")),
    ("dcr r9", Some("f9")),
    ("dcr r10", Some("fa")),
    ("dcr r11", Some("fb")),
    ("dcr r12", Some("fc")),
    ("dcr r13", Some("fd")),
    ("dcr r14", Some("fe")),
    ("dcr r15", Some("ff")),
    ("rtn", Some("00")),
    ("br 2", Some("0100")),
    ("bnc 2", Some("0200")),
    ("bc 2", Some("0300")),
    ("bp 2", Some("0400")),
    ("bm 2", Some("0500")),
    ("bz 2", Some("0600")),
    ("bnz 2", Some("0700")),
    ("bm1 2", Some("0800")),
    ("bnm1 2", Some("0900")),
    ("bk", Some("0a")),
    ("rs", Some("0b")),
    ("bs 3", Some("0c01")),
    ("br 0", Some("01fe")),
    ("br $81", Some("017f")),
    ("br $82", None),
    ("set 5, $1234", Some("153412")),
    ("ld 5", Some("25")),
    ("ld @5", Some("45")),
    ("ld 2+3", Some("25")),
    ("set r16, 1", None),
    ("ld", None),
    ("set r5", None),
    ("ld r5, 1", None),
    ("add @r2", None),
    ("pop r2", None),
    ("ld r5+1", None),
    ("br 300", None),
    ("ld @r16", None),
    ("inr", None),
    ("rtn r1", None),
    ("set r1, $12345", None),
    ("ld @2+3", Some("45")),
    ("br r5", None),
];

#[test]
fn every_measured_sweet16_line_matches_ca65() {
    let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::Sweet16);
    for &(source, hex) in MEASURED_SWEET16 {
        let got = lowered_on(profile, source, &Names::default())
            .ok()
            .and_then(|op| encode_on(Cpu::Sweet16, &op, &|_| None));
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
