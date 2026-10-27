//! Expressions lowered and evaluated by the back end's evaluator, against
//! the values ca65 V2.19 gave.

use asm198x_ir::Expr as Ir;
use lang198x_syntax::DiagnosticKind;

use super::expression;
use crate::ast::{Expr, SourceFile, Statement, View};
use crate::{Profile, parse};

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
    let lowered = expression(&immediate(text)).map_err(|d| d.message().to_owned())?;
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
            expression(&immediate(text)).ok(),
            Some(Ir::Sym(name.to_owned())),
            "{text}"
        );
    }
}

#[test]
fn what_is_not_lowered_yet_says_so() {
    for (text, kind) in [
        (".defined(q)", DiagnosticKind::Unsupported),
        (".strlen(\"ab\")", DiagnosticKind::Unsupported),
        ("\"A\"", DiagnosticKind::Invalid),
        (".lobyte(1, 2)", DiagnosticKind::Invalid),
        ("'AB'", DiagnosticKind::Invalid),
        ("$1FFFFFFFF", DiagnosticKind::Invalid),
    ] {
        let problem = expression(&immediate(text)).expect_err(text);
        assert_eq!(problem.kind(), kind, "{text}: {problem:?}");
    }
}
