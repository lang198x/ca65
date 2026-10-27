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
    assert_eq!(of("<later"), Value::Address(Size::Zero));
    assert_eq!(of("later"), Value::Address(Size::Absolute));
    assert_eq!(of("q*$100"), Value::Constant(0x300));
}
