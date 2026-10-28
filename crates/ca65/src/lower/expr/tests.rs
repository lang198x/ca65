//! Expressions lowered and evaluated by the back end's evaluator, against
//! the values ca65 V2.19 gave.

use asm198x_ir::Expr as Ir;
use lang198x_syntax::DiagnosticKind;

use super::expression;
use crate::ast::{Expr, SourceFile, Statement, View};
use crate::lower::{self, Known, Size, Value};
use crate::{Profile, parse};

/// An assembler that knows one constant, `q = 3`, whose line was a label's
/// that made two bytes.
struct One;

impl Known for One {
    fn name(&self, name: &str) -> Value {
        match name {
            "q" => Value::Constant(3),
            "z" => Value::Address(Size::Zero),
            _ => Value::Unknown,
        }
    }

    fn pc(&self) -> Value {
        Value::Unknown
    }

    fn size_of(&self, name: &str) -> Option<i64> {
        (name == "q").then_some(2)
    }
}

/// An assembler that knows nothing, and keeps the default `size_of`.
struct Nothing;

impl Known for Nothing {
    fn name(&self, _: &str) -> Value {
        Value::Unknown
    }

    fn pc(&self) -> Value {
        Value::Unknown
    }
}

/// The immediate value of `lda #expression`.
fn immediate(expression: &str) -> Expr {
    let parse = parse(&format!("lda #{expression}\n"), Profile::CA65_2_19_6502).expect("parses");
    let line = SourceFile::cast(parse.tree().root())
        .and_then(|file| file.lines().next())
        .expect("a line");
    let Some(Statement::Instruction(instruction)) = line.statement() else {
        panic!("an instruction")
    };
    instruction
        .operands()
        .next()
        .and_then(|o| o.immediate())
        .and_then(|i| i.expr())
        .expect("a value")
}

fn value(text: &str) -> Result<i64, String> {
    let lowered = expression(&immediate(text), &One).map_err(|d| d.message().to_owned())?;
    lowered
        .eval_with(&|_| None, Some(0), 0)
        .map_err(|e| e.message)
}

#[test]
fn every_measured_expression_has_ca65s_value() {
    for &(text, byte) in crate::measured::EXPRESSIONS {
        assert_eq!(value(text).map(|v| v & 0xFF), Ok(i64::from(byte)), "{text}");
    }
}

#[test]
fn names_stay_as_written() {
    for (text, name) in [
        ("x2", "x2"),
        ("@near", "@near"),
        ("outer::x2", "outer::x2"),
        ("::x2", "::x2"),
        (":+", ":+"),
        (":--", ":--"),
    ] {
        assert_eq!(
            expression(&immediate(text), &One).ok(),
            Some(Ir::Sym(name.to_owned())),
            "{text}"
        );
    }
}

#[test]
fn what_is_not_lowered_yet_says_so() {
    for (text, kind) in [
        (".cpu", DiagnosticKind::Unsupported),
        (".defined(1)", DiagnosticKind::Invalid),
        (".strlen(\"ab\")", DiagnosticKind::Unsupported),
        ("\"A\"", DiagnosticKind::Invalid),
        (".lobyte(1, 2)", DiagnosticKind::Invalid),
        ("'AB'", DiagnosticKind::Invalid),
        ("$1FFFFFFFF", DiagnosticKind::Invalid),
    ] {
        let problem = expression(&immediate(text), &One).expect_err(text);
        assert_eq!(problem.kind(), kind, "{text}: {problem:?}");
    }
}

#[test]
fn sizeof_asks_the_assembler_now() {
    assert_eq!(
        expression(&immediate(".sizeof(q)+1"), &One).ok(),
        Some(Ir::Bin(
            asm198x_ir::BinOp::Add,
            Box::new(Ir::Num(2)),
            Box::new(Ir::Num(1))
        ))
    );
    for (text, known) in [
        (".sizeof(q)", &Nothing as &dyn Known),
        (".sizeof(other)", &One),
        (".sizeof(1)", &One),
        (".sizeof(q, q)", &One),
    ] {
        let problem = expression(&immediate(text), known).expect_err(text);
        assert_eq!(problem.kind(), DiagnosticKind::Invalid, "{text}");
    }
}

#[test]
fn value_is_a_constant_or_an_address_size() {
    let of = |text: &str| {
        let lowered = expression(&immediate(text), &One).expect("lowers");
        lower::value(&lowered, &One)
    };
    assert_eq!(of("q*2"), Value::Constant(6));
    assert_eq!(of("z+1"), Value::Address(Size::Zero));
    assert_eq!(of("<later"), Value::Opaque(Size::Zero));
    assert_eq!(of("later"), Value::Opaque(Size::Absolute));
    assert_eq!(of("q*$100"), Value::Constant(0x300));
}

/// Labels `l` and `r`, at offsets 0 and 1 into segment 0, `o` at offset 0
/// into segment 1, and the address `*` at offset 2 into segment 0, all
/// absolute; `q`, which is 3; `k`, a distance of 1; `e`, an absolute
/// import; and `z`, an import on the zero page.
struct Placed;

impl Known for Placed {
    fn name(&self, name: &str) -> Value {
        let placed = |segment, offset| Value::Placed {
            segment,
            offset,
            size: Size::Absolute,
        };
        match name {
            "l" => placed(0, 0),
            "r" => placed(0, 1),
            "o" => placed(1, 0),
            "q" => Value::Constant(3),
            "k" => Value::Measured {
                value: 1,
                size: Size::Absolute,
            },
            "e" => Value::Address(Size::Absolute),
            "z" => Value::Address(Size::Zero),
            _ => Value::Unknown,
        }
    }

    fn pc(&self) -> Value {
        Value::Placed {
            segment: 0,
            offset: 2,
            size: Size::Absolute,
        }
    }
}

/// What ca65 V2.19 knows of `text` now, with the labels of [`Placed`].
fn placed(text: &str) -> Value {
    let lowered = expression(&immediate(text), &Placed).expect(text);
    lower::value(&lowered, &Placed)
}

/// In ca65 V2.19, labels in one segment are a known distance apart, and
/// may be added, taken away, negated and multiplied by a number. The value
/// keeps the labels' address size. Measured with `.res` and `lda`.
#[test]
fn labels_in_one_segment_are_a_known_distance_apart() {
    let measured = |value| Value::Measured {
        value,
        size: Size::Absolute,
    };
    for (text, value) in [
        ("r-l", 1),
        ("r-l+l-l+r-l", 2),
        ("(r-l)*2", 2),
        ("r*2-l*2", 2),
        ("r+r-l-l", 2),
        ("-l+r", 1),
        ("l+1-l", 1),
        ("r-l-1+1", 1),
        ("(r-l)*(r-l)", 1),
        ("(r-l)/1", 1),
        ("(r-l)<<1", 2),
        ("*-l", 2),
        ("k+q", 4),
    ] {
        assert_eq!(placed(text), measured(value), "{text}");
    }
    assert_eq!(placed("q*2"), Value::Constant(6));
    // `&&` reads no further than it needs to.
    assert_eq!(placed("0 && (r-l)"), Value::Constant(0));
    assert_eq!(
        placed("l+1"),
        Value::Placed {
            segment: 0,
            offset: 1,
            size: Size::Absolute
        }
    );
    // `&` takes the smaller of its sides.
    assert_eq!(
        placed("(r-l)&3"),
        Value::Measured {
            value: 1,
            size: Size::Zero
        }
    );
    // `<` makes one byte, whatever the labels' size.
    assert_eq!(
        placed("<(r-l)"),
        Value::Measured {
            value: 1,
            size: Size::Zero
        }
    );
}

/// An import cancels itself, and nothing times an import is nothing, as
/// ca65 V2.19's `StudyExpr` counts imports: `.const(RED-RED)` and
/// `.const(RED*0)` are true for an imported `RED`, measured.
#[test]
fn an_import_counts_as_a_label_does() {
    let constant =
        |text: &str| super::constant(&expression(&immediate(text), &Placed).expect(text), &Placed);
    for (text, value) in [("e-e", 0), ("e*0", 0), ("e+1-e", 1), ("z-z", 0)] {
        assert_eq!(constant(text), Some(value), "{text}");
    }
    assert_eq!(constant("e-z"), None);
}

/// What ca65 V2.19 does not know until the link: labels in two segments, a
/// label added to a label, a label in any other operation, and an import.
#[test]
fn other_mixes_of_labels_wait_for_the_link() {
    for text in ["o-l", "r+l", "r&$FF-l&$FF", "^r-^l", "l*r", "e-l", "f-l"] {
        assert!(
            matches!(placed(text), Value::Address(_) | Value::Opaque(_)),
            "{text}: {:?}",
            placed(text)
        );
    }
    // A label times nothing is nothing.
    assert_eq!(
        placed("l*0"),
        Value::Measured {
            value: 0,
            size: Size::Absolute
        }
    );
}

/// ca65 V2.19 sizes `lda r-l` by its labels, absolute, though the value is
/// one, and `lda <(r-l)` as the zero page.
#[test]
fn a_distance_takes_its_labels_size() {
    for (text, size) in [
        ("r-l", Size::Absolute),
        ("<(r-l)", Size::Zero),
        ("q", Size::Zero),
    ] {
        let lowered = expression(&immediate(text), &Placed).expect(text);
        assert_eq!(super::size(&lowered, &Placed), size, "{text}");
    }
}

/// The address size ca65 V2.19 gives each operand, read from its listings.
/// In names and numbers added, taken away, negated or multiplied by a
/// number, only the names count. Any other operation counts its numbers
/// too, except `&`, which takes the smaller side, and `<`, `>`, `^`, the
/// comparisons and the logical operators, which make one byte.
#[test]
fn operands_take_the_size_ca65_studies() {
    for (text, size) in [
        ("z+1", Size::Zero),
        ("z+$100", Size::Zero),
        ("$100+z", Size::Zero),
        ("z-$100", Size::Zero),
        ("z*2", Size::Zero),
        ("z|$1000", Size::Absolute),
        ("z&$FF", Size::Zero),
        ("l&$FF", Size::Zero),
        ("l=3", Size::Zero),
        ("l && 1", Size::Zero),
        ("!l", Size::Zero),
        ("<l+1", Size::Zero),
        ("<l+$100", Size::Absolute),
        ("3&(r-l)", Size::Zero),
        ("-l", Size::Absolute),
        ("~z", Size::Zero),
        ("r-l+$100", Size::Absolute),
        ("2*(r-l)", Size::Absolute),
        ("0+(r-l)*(r-l)", Size::Absolute),
        ("$1234-$1234+r-r", Size::Absolute),
        ("q|1", Size::Zero),
        ("l/2", Size::Absolute),
        ("z<<1", Size::Zero),
    ] {
        let lowered = expression(&immediate(text), &Placed).expect(text);
        assert_eq!(super::size(&lowered, &Placed), size, "{text}");
    }
}
