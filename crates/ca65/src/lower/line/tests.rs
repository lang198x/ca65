//! Whole programs, run through the expander and lowered line by line, then
//! assembled by a two-pass assembler as small as the tests need, against
//! the bytes ca65 V2.19 and ld65 wrote under a flat link at 0.

use std::collections::HashMap;

use asm198x_ir::{Expr as Ir, Operation, align_pad, next_pc};
use isa198x::OperandKind;
use lang198x_syntax::DiagnosticKind;

use super::line;
use crate::Profile;
use crate::ast::{Expr, NameRef};
use crate::expand::{Expander, Host};
use crate::lower::{self, Known, Scope, Segment, Size, Value};

/// The assembler: what each name is known as so far, and the final values
/// from the pass before.
#[derive(Default)]
struct Mini {
    known: HashMap<String, Value>,
    finals: HashMap<String, i64>,
    /// The bytes each label's line made.
    sizes: HashMap<String, i64>,
    pc: i64,
    org: bool,
}

impl Mini {
    fn value(&self, expr: &Ir) -> Option<i64> {
        let lookup = |name: &str| match self.known.get(name) {
            Some(Value::Constant(value)) => Some(*value),
            _ => self.finals.get(name).copied(),
        };
        expr.eval_with(&lookup, Some(self.pc), 0).ok()
    }
}

impl Known for Mini {
    fn name(&self, name: &str) -> Value {
        self.known.get(name).copied().unwrap_or(Value::Unknown)
    }

    fn pc(&self) -> Value {
        if self.org {
            Value::Constant(self.pc)
        } else {
            Value::Address(Size::Absolute)
        }
    }

    fn size_of(&self, name: &str) -> Option<i64> {
        self.sizes.get(name).copied()
    }
}

impl Host for Mini {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        let lowered = lower::expression(expr, &*self).map_err(|d| d.message().to_owned())?;
        self.value(&lowered).ok_or_else(|| "not known".to_owned())
    }

    fn is_defined(&mut self, name: &NameRef) -> bool {
        self.known.contains_key(&name.text())
    }

    fn is_referenced(&mut self, _: &NameRef) -> bool {
        false
    }

    fn is_constant(&mut self, expr: &Expr) -> bool {
        self.evaluate(expr).is_ok()
    }
}

/// One pass over `source`, given the final values from the pass before:
/// the bytes, and every name's value.
fn pass(source: &str, finals: HashMap<String, i64>) -> Result<(Vec<u8>, Mini), String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).map_err(|e| e.to_string())?;
    let mut mini = Mini {
        finals,
        ..Mini::default()
    };
    let mut bytes = Vec::new();
    while let Some(active) = expander.next_line(&mut mini) {
        let lowered = line(&active, profile, &mini).map_err(|d| d.message().to_owned())?;
        let before = bytes.len();
        let mut labels = Vec::new();
        for item in lowered.items {
            if let Some(label) = item.label {
                let value = match &item.op {
                    Some(Operation::Equ(e) | Operation::Set(e)) => match mini.value(e) {
                        Some(v) => Value::Constant(v),
                        None => Value::Unknown,
                    },
                    _ if mini.org => Value::Constant(mini.pc),
                    _ => Value::Address(Size::Absolute),
                };
                let fin = match (&item.op, value) {
                    (_, Value::Constant(v)) => v,
                    (Some(Operation::Equ(e) | Operation::Set(e)), _) => mini.value(e).unwrap_or(0),
                    _ => mini.pc,
                };
                mini.known.insert(label.clone(), value);
                if !matches!(item.op, Some(Operation::Equ(_) | Operation::Set(_))) {
                    labels.push(label.clone());
                }
                mini.finals.entry(label).or_insert(fin);
            }
            let Some(op) = item.op else { continue };
            emit(&op, &mut mini, &mut bytes)?;
        }
        let made = i64::try_from(bytes.len() - before).unwrap_or(i64::MAX);
        for label in labels {
            mini.sizes.insert(label, made);
        }
    }
    if let Some((_, problem)) = expander.diagnostics().first() {
        return Err(problem.message().to_owned());
    }
    Ok((bytes, mini))
}

fn emit(op: &Operation, mini: &mut Mini, bytes: &mut Vec<u8>) -> Result<(), String> {
    let out_of_range = || "out of range".to_owned();
    match op {
        Operation::Equ(_) | Operation::Set(_) => return Ok(()),
        Operation::Org(e) => {
            mini.pc = mini.value(e).ok_or_else(out_of_range)?;
            mini.org = true;
            return Ok(());
        }
        Operation::Sized {
            width,
            big_endian,
            values,
            ..
        } => {
            for value in values {
                let v = mini.value(value).unwrap_or(0);
                if v < 0 || v >= 1_i64 << (8 * width) {
                    return Err(out_of_range());
                }
                let mut le: Vec<u8> = (0..*width).map(|i| (v >> (8 * i)) as u8).collect();
                if *big_endian {
                    le.reverse();
                }
                bytes.extend(le);
            }
        }
        Operation::Reserve(n) => bytes.extend(std::iter::repeat_n(0, *n)),
        Operation::Fill { count, value } => {
            let n = usize::try_from(mini.value(count).unwrap_or(0)).map_err(|_| out_of_range())?;
            bytes.extend(std::iter::repeat_n(*value, n));
        }
        Operation::AlignTo { modulus, fill } => {
            let n = usize::try_from(align_pad(mini.pc, *modulus)).map_err(|_| out_of_range())?;
            bytes.extend(std::iter::repeat_n(fill[0], n));
        }
        Operation::Instruction {
            mnemonic,
            mode,
            operands,
        } => {
            let form = isa198x::mos6502::SET
                .find_form(mnemonic, mode)
                .ok_or_else(out_of_range)?;
            let len = 1 + form
                .operands
                .iter()
                .map(|o| i64::from(o.bytes))
                .sum::<i64>();
            bytes.extend(form.opcode);
            for (slot, value) in form.operands.iter().zip(operands) {
                let v = mini.value(value).unwrap_or(0);
                let v = if slot.kind == OperandKind::RelativePc {
                    v - mini.pc - len
                } else {
                    v
                };
                bytes.extend((0..slot.bytes).map(|i| (v >> (8 * i)) as u8));
            }
        }
        _ => return Err("not in this test's assembler".to_owned()),
    }
    mini.pc = next_pc(op, mini.pc, &isa198x::mos6502::SET, None, 1, 0).map_err(|e| e.message)?;
    Ok(())
}

/// `source`, assembled in two passes, as hex.
fn assemble(source: &str) -> Result<String, String> {
    let (_, first) = pass(source, HashMap::new())?;
    let (bytes, _) = pass(source, first.finals)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Programs measured on ca65 V2.19, with the bytes it wrote, or `None` if it
/// refused the program.
const MEASURED: &[(&str, Option<&str>)] = &[
    (".byte 1, $ff, \"ab\", 'c'\n", Some("01ff616263")),
    (".byt 1\n", Some("01")),
    (".byte 256\n", None),
    (".byte -1\n", None),
    (".byte -129\n", None),
    (".byte\n", None),
    (".word $1234, 1\n.addr 2\n", Some("341201000200")),
    (".word -1\n", None),
    (".word $10000\n", None),
    (".dbyt $1234\n", Some("1234")),
    (
        ".faraddr $123456\n.dword $12345678\n",
        Some("56341278563412"),
    ),
    (
        ".lobytes $1234, $5678\n.hibytes $1234\n.bankbytes $123456\n",
        Some("34781212"),
    ),
    (".asciiz \"ab\"\n", Some("616200")),
    (".res 3\n.res 2, $ea\n.byte 1\n", Some("000000eaea01")),
    (".res 0\n", Some("")),
    (".res -1\n", None),
    (".res fwd\nfwd=2\n", None),
    (".res 2, 256\n", None),
    (".org $10\nl: .byte <l\n", Some("10")),
    ("nop\n.align 4\n.byte 1\n", Some("ea00000001")),
    ("nop\n.align 4, $ea\n.byte 1\n", Some("eaeaeaea01")),
    ("nop\n.align 3\n.byte 1\n", Some("ea000001")),
    ("q = 5\nq2 := 6\n.byte q, q2\n", Some("0506")),
    ("q .set 1\n.byte q\nq .set 2\n.byte q\n", Some("0102")),
    ("l: .byte 1\n.word l\n", Some("010000")),
    ("l: \nnop\n.word l\n", Some("ea0000")),
    ("@c: .byte 1\n.word @c\n", Some("010000")),
    ("q2 := *\n.word q2\n", Some("0000")),
    (".word *\n", Some("0000")),
    // `*` in a list is the address of the value it is in.
    (".word *, *+2, *\n", Some("000004000400")),
    (".dbyt *, *\n", Some("00000002")),
    (".org $1000\n.word *,*+2\n", Some("00100410")),
    ("lda fwd\nfwd = $12\n", Some("ad1200")),
    ("fwd = $12\nlda fwd\n", Some("a512")),
    ("lab: nop\nlda lab\n", Some("eaad0000")),
    ("lda lab\nlab: nop\n", Some("ad0300ea")),
    (".org $10\nol: nop\nlda ol\n", Some("eaa510")),
    ("bne *\nbne lab\nlab:\n", Some("d0fed000")),
    ("q = 1000\nq2 = q - 990\nlda q2\n", Some("a50a")),
    // A made string keeps bytes its text cannot hold.
    (".asciiz .sprintf(\"%c%c\",200,1)\n", Some("c80100")),
    (".byte .sprintf(\"%c\",255), 1\n", Some("ff01")),
    // `.sizeof` a label: the bytes its line made, known now.
    ("a1: .byte 1,2,3\n.byte .sizeof(a1)\n", Some("01020303")),
    ("a1:\n.byte 1,2,3\n.byte .sizeof(a1)\n", Some("01020300")),
    ("a1: nop\nlda #1\n.byte .sizeof(a1)\n", Some("eaa90101")),
    ("a1: .res 3\n.byte .sizeof(a1)\n", Some("00000003")),
    (".byte .sizeof(a1)\na1: .word 1,2\n", None),
    ("v=5\n.byte .sizeof(v)\n", None),
    (".byte .sizeof(nosuch)\n", None),
    (".byte .sizeof(*)\n", None),
    ("a1: .byte 1\n.byte .SIZEOF(a1)\n", Some("0101")),
];

#[test]
fn every_measured_program_matches_ca65() {
    for &(source, hex) in MEASURED {
        let got = assemble(source);
        assert_eq!(got.as_deref().ok(), hex, "{source:?}: {got:?}");
    }
}

/// The items `source`'s first line lowers to: each label, and whether an
/// operation came with it.
fn items(source: &str) -> Vec<(Option<String>, bool)> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    line(&active, profile, &mini)
        .expect("lowers")
        .items
        .into_iter()
        .map(|item| (item.label, item.op.is_some()))
        .collect()
}

#[test]
fn labels_are_items_of_their_own_or_carry_the_operation() {
    let named = |n: &str| Some(n.to_owned());
    assert_eq!(
        items(": l2: nop\n"),
        [(named(":"), false), (named("l2"), true)]
    );
    assert_eq!(items("@near:\n"), [(named("@near"), false)]);
    assert_eq!(items("nop\n"), [(None, true)]);
    assert_eq!(items("\n"), []);
    // A label before a macro call reaches the host on its own.
    assert_eq!(
        items(".macro m\nnop\n.endmacro\nl3: m\n"),
        [(named("l3"), false)]
    );
}

/// A line's items, as `items` gives them, and its change to the scopes.
type Reported = (Vec<(Option<String>, bool)>, Option<Scope>);

/// The change to the scopes `source`'s first line makes, and its items.
fn scope(source: &str) -> Result<Reported, String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    let lowered = line(&active, profile, &mini).map_err(|d| d.message().to_owned())?;
    let items = lowered
        .items
        .into_iter()
        .map(|item| (item.label, item.op.is_some()))
        .collect();
    Ok((items, lowered.scope))
}

#[test]
fn scope_commands_are_reported_after_the_lines_items() {
    let open = |name: Option<&str>, procedure| Scope::Open {
        name: name.map(str::to_owned),
        procedure,
    };
    let label = |n: &str| (Some(n.to_owned()), false);
    assert_eq!(
        scope(".scope inner\n"),
        Ok((vec![], Some(open(Some("inner"), false))))
    );
    assert_eq!(scope(".SCOPE\n"), Ok((vec![], Some(open(None, false)))));
    // `.proc` also defines its name, in the scope around it.
    assert_eq!(
        scope(".proc main\n"),
        Ok((vec![label("main")], Some(open(Some("main"), true))))
    );
    // An unnamed `.proc` is an unnamed `.scope`.
    assert_eq!(scope(".proc\n"), Ok((vec![], Some(open(None, false)))));
    // ca65 takes an unnamed label before either.
    assert_eq!(
        scope(": .proc p\n"),
        Ok((vec![label(":"), label("p")], Some(open(Some("p"), true))))
    );
    // A label on the closing line belongs to the scope that closes.
    assert_eq!(
        scope("tail: .endscope\n"),
        Ok((vec![label("tail")], Some(Scope::Close { procedure: false })))
    );
    assert_eq!(
        scope(".endproc\n"),
        Ok((vec![], Some(Scope::Close { procedure: true })))
    );
    assert_eq!(scope("nop\n"), Ok((vec![(None, true)], None)));
}

/// The change to the segment `source`'s first line makes.
fn segment(source: &str) -> Result<Option<Segment>, String> {
    let profile = Profile::CA65_2_19_6502;
    let mut expander = Expander::new(source, profile).expect("parses");
    let mut mini = Mini::default();
    let active = expander.next_line(&mut mini).expect("a line");
    line(&active, profile, &mini)
        .map(|lowered| lowered.segment)
        .map_err(|d| d.message().to_owned())
}

#[test]
fn segment_commands_are_reported() {
    let used = |name: &str, size| {
        Ok(Some(Segment::Use {
            name: name.to_owned(),
            size,
        }))
    };
    assert_eq!(segment(".segment \"CODE\"\n"), used("CODE", None));
    // Names are case-sensitive, as ld65 reads them.
    assert_eq!(segment(".segment \"code\"\n"), used("code", None));
    assert_eq!(
        segment(".segment \"ZP2\": zeropage\n"),
        used("ZP2", Some(Size::Zero))
    );
    assert_eq!(
        segment(".segment \"A\": abs\n"),
        used("A", Some(Size::Absolute))
    );
    assert_eq!(segment(".segment \"F\": far\n"), used("F", Some(Size::Far)));
    for (word, name) in [
        (".code", "CODE"),
        (".RODATA", "RODATA"),
        (".data", "DATA"),
        (".bss", "BSS"),
        (".zeropage", "ZEROPAGE"),
    ] {
        assert_eq!(segment(&format!("{word}\n")), used(name, None), "{word}");
    }
    assert_eq!(segment(".pushseg\n"), Ok(Some(Segment::Push)));
    assert_eq!(segment(".popseg\n"), Ok(Some(Segment::Pop)));
    assert_eq!(segment("nop\n"), Ok(None));
    assert!(segment(".segment 5\n").is_err());
}

#[test]
fn ca65_refuses_a_label_before_a_scope() {
    let profile = Profile::CA65_2_19_6502;
    for source in ["x: .scope s\n", "x: .proc p\n"] {
        let mut expander = Expander::new(source, profile).expect("parses");
        let _ = expander.next_line(&mut Mini::default());
        assert!(!expander.diagnostics().is_empty(), "{source:?}");
    }
}

#[test]
fn what_is_not_lowered_yet_says_so() {
    let profile = Profile::CA65_2_19_6502;
    for source in [
        ".byte .strlen(\"ab\")\n",
        ".segment \"X\": dword\n",
        ".struct s\n",
        ".proc p: far\n",
    ] {
        let mut expander = Expander::new(source, profile).expect("parses");
        let mut mini = Mini::default();
        let active = expander.next_line(&mut mini).expect("a line");
        let problem = line(&active, profile, &mini)
            .err()
            .unwrap_or_else(|| panic!("{source:?} lowers"));
        assert_eq!(problem.kind(), DiagnosticKind::Unsupported, "{source:?}");
    }
}
