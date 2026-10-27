//! The views are checked on parsed source, including broken lines, and by a
//! second evaluator, written over the views, that must give ca65's value for
//! every measured expression.

use super::*;
use crate::{Profile, parse};

/// The views of `text`'s lines.
fn lines(text: &str) -> Vec<Line> {
    let parse = parse(text, Profile::CA65_2_19_6502).expect("short text always parses");
    SourceFile::cast(parse.tree().root())
        .expect("the root is a source file")
        .lines()
        .collect()
}

fn first(text: &str) -> Line {
    lines(text).into_iter().next().expect("a line")
}

fn texts(tokens: impl Iterator<Item = SyntaxToken>) -> Vec<String> {
    tokens.map(|t| t.text().to_owned()).collect()
}

/// Evaluates `expr` through the views, as ca65 does, for numbers,
/// characters, `.lobyte` and `.hibyte`.
fn value(expr: &Expr) -> i64 {
    let truth = |b: bool| i64::from(b);
    match expr {
        Expr::Literal(literal) => {
            let text = literal
                .token()
                .map(|t| t.text().to_owned())
                .unwrap_or_default();
            number(&text)
        }
        Expr::Paren(paren) => paren.expr().map_or(0, |e| value(&e)),
        Expr::Unary(unary) => {
            let operand = unary.operand().map_or(0, |e| value(&e));
            let op = unary
                .op()
                .map(|t| t.text().to_ascii_lowercase())
                .unwrap_or_default();
            match op.as_str() {
                "+" => operand,
                "-" => -operand,
                "~" | ".bitnot" => !operand,
                "<" => operand & 0xFF,
                ">" => (operand >> 8) & 0xFF,
                "^" => (operand >> 16) & 0xFF,
                "!" | ".not" => truth(operand == 0),
                other => panic!("unknown prefix {other}"),
            }
        }
        Expr::Binary(binary) => {
            let lhs = binary.lhs().map_or(0, |e| value(&e));
            let rhs = binary.rhs().map_or(0, |e| value(&e));
            let op = binary
                .op()
                .map(|t| t.text().to_ascii_lowercase())
                .unwrap_or_default();
            match op.as_str() {
                "||" | ".or" => truth(lhs != 0 || rhs != 0),
                "&&" | ".and" => truth(lhs != 0 && rhs != 0),
                ".xor" => truth((lhs != 0) ^ (rhs != 0)),
                "=" => truth(lhs == rhs),
                "<>" => truth(lhs != rhs),
                "<" => truth(lhs < rhs),
                ">" => truth(lhs > rhs),
                "<=" => truth(lhs <= rhs),
                ">=" => truth(lhs >= rhs),
                "+" => lhs + rhs,
                "-" => lhs - rhs,
                "|" | ".bitor" => lhs | rhs,
                "*" => lhs * rhs,
                "/" => lhs / rhs,
                ".mod" => lhs % rhs,
                "&" | ".bitand" => lhs & rhs,
                "^" | ".bitxor" => lhs ^ rhs,
                "<<" | ".shl" => lhs << rhs,
                ">>" | ".shr" => lhs >> rhs,
                other => panic!("unknown operator {other}"),
            }
        }
        Expr::Call(call) => {
            let argument = call
                .args()
                .next()
                .and_then(|arg| arg.expr())
                .map_or(0, |e| value(&e));
            match call
                .function()
                .map(|t| t.text().to_ascii_lowercase())
                .as_deref()
            {
                Some(".lobyte") => argument & 0xFF,
                Some(".hibyte") => (argument >> 8) & 0xFF,
                other => panic!("unknown function {other:?}"),
            }
        }
        Expr::Name(_) | Expr::Unnamed(_) => panic!("no symbols here"),
    }
}

fn number(text: &str) -> i64 {
    let parsed = if let Some(hex) = text.strip_prefix('$') {
        i64::from_str_radix(hex, 16)
    } else if let Some(binary) = text.strip_prefix('%') {
        i64::from_str_radix(binary, 2)
    } else {
        text.parse()
    };
    parsed.unwrap_or_else(|_| panic!("not a number: {text}"))
}

/// The immediate value of `lda #expression`, through the views.
fn immediate(expression: &str) -> Expr {
    let line = first(&format!("lda #{expression}"));
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

#[test]
fn the_views_give_ca65s_values() {
    for &(expression, expected) in crate::measured::EXPRESSIONS {
        let byte = value(&immediate(expression)) & 0xFF;
        assert_eq!(byte, i64::from(expected), "{expression}");
    }
}

#[test]
fn lines_and_labels() {
    let line = first(": here: dex ; go\n");
    assert!(line.unnamed_label().is_some());
    let name = line.label().and_then(|l| l.name()).expect("a name");
    assert_eq!(name.text(), "here");
    assert!(!name.is_global());
    assert_eq!(
        line.comment().map(|t| t.text().to_owned()),
        Some("; go".to_owned())
    );
    assert_eq!(line.errors().count(), 0);
    let scoped = first("::outer::x2 = 1").statement();
    let Some(Statement::Assignment(assignment)) = scoped else {
        panic!()
    };
    let name = assignment.name().expect("a name");
    assert!(name.is_global());
    assert_eq!(texts(name.parts()), ["outer", "x2"]);
    assert_eq!(name.text(), "::outer::x2");
    assert_eq!(
        assignment.op().map(|t| t.text().to_owned()),
        Some("=".to_owned())
    );
    assert!(assignment.value().is_some());
}

#[test]
fn a_broken_line_has_errors_and_missing_parts() {
    let line = first("foo bar");
    assert_eq!(line.errors().count(), 1);
    assert!(line.label().is_none());
    assert!(line.statement().is_none());
    let line = first("v = ");
    let Some(Statement::Assignment(assignment)) = line.statement() else {
        panic!()
    };
    assert!(assignment.value().is_none());
}

#[test]
fn operands() {
    let operand = |text: &str| {
        let Some(Statement::Instruction(i)) = first(text).statement() else {
            panic!()
        };
        assert!(i.mnemonic().is_some());
        i.operands().next().expect("an operand")
    };
    let accumulator = operand("asl a");
    assert!(accumulator.accumulator().is_some() && accumulator.expr().is_none());
    let indexed = operand("sta z:$12,x");
    assert_eq!(
        indexed.address_size().map(|t| t.text().to_owned()),
        Some("z:".to_owned())
    );
    assert!(indexed.expr().is_some());
    let register = indexed.index().and_then(|i| i.register());
    assert_eq!(register.map(|t| t.text().to_owned()), Some("x".to_owned()));
    let indirect = operand("lda ($12,x)").indirect().expect("indirect");
    assert!(indirect.expr().is_some());
    assert!(indirect.inner_register().is_some() && indirect.outer_register().is_none());
    let indirect = operand("lda ($12),y").indirect().expect("indirect");
    assert!(indirect.inner_register().is_none() && indirect.outer_register().is_some());
    let long = operand("lda [$12],y").long_indirect().expect("long");
    assert!(long.expr().is_some() && long.outer_register().is_some());
    assert!(operand("lda #1").immediate().is_some());
    let Some(Statement::Instruction(i)) = first("lda $12, there").statement() else {
        panic!()
    };
    assert_eq!(i.operands().count(), 2);
}

#[test]
fn expressions_by_kind() {
    let Expr::Name(name) = immediate("outer::v") else {
        panic!()
    };
    assert_eq!(texts(name.parts()), ["outer", "v"]);
    assert_eq!(name.text(), "outer::v");
    assert!(!name.is_global());
    let Expr::Unnamed(unnamed) = immediate(":+") else {
        panic!()
    };
    assert_eq!(
        unnamed.token().map(|t| t.text().to_owned()),
        Some(":+".to_owned())
    );
    let Expr::Call(call) = immediate(".tcount({1, 2})") else {
        panic!()
    };
    let list = call
        .args()
        .next()
        .and_then(|a| a.tokens())
        .expect("a token list");
    assert!(list.is_braced());
    assert_eq!(texts(list.contents()), ["1", ",", "2"]);
    assert_eq!(texts(list.tokens()).len(), 5);
    let Expr::Call(call) = immediate(".match(a b, c)") else {
        panic!()
    };
    let list = call
        .args()
        .next()
        .and_then(|a| a.tokens())
        .expect("a token list");
    assert!(!list.is_braced());
    assert_eq!(texts(list.contents()), ["a", "b"]);
    assert!(call.args().next().and_then(|a| a.expr()).is_none());
    let Expr::Call(open) = immediate(".tcount({1") else {
        panic!()
    };
    let list = open
        .args()
        .next()
        .and_then(|a| a.tokens())
        .expect("a token list");
    assert_eq!(texts(list.contents()), ["1"]);
}

#[test]
fn commands() {
    let command = |text: &str| match first(text).statement() {
        Some(Statement::Command(command)) => command,
        other => panic!("{text}: {other:?}"),
    };
    let byte = command(".byte 1, 2");
    assert_eq!(
        byte.word().map(|t| t.text().to_owned()),
        Some(".byte".to_owned())
    );
    assert_eq!(byte.values().count(), 2);
    let export = command(".export v: zeropage = 1, w");
    let items: Vec<Item> = export.items().collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].name().map(|n| n.text()), Some("v".to_owned()));
    assert_eq!(
        items[0].address_size().map(|t| t.text().to_owned()),
        Some("zeropage".to_owned())
    );
    assert!(items[0].value().is_some());
    assert!(items[1].address_size().is_none() && items[1].value().is_none());
    let segment = command(".segment \"ZP\": zeropage");
    assert_eq!(
        segment.address_size().map(|t| t.text().to_owned()),
        Some("zeropage".to_owned())
    );
    assert!(command(".segment \"ZP\"").address_size().is_none());
    assert!(command(".segment \"ZP\":").address_size().is_none());
    let ifdef = command(".ifdef s::v");
    assert_eq!(ifdef.name_ref().map(|n| n.text()), Some("s::v".to_owned()));
    let define = command(".define F(p, q) p+q");
    assert_eq!(define.name().map(|n| n.text()), Some("F".to_owned()));
    let params = define.params().expect("parameters");
    assert_eq!(texts(params.names()), ["p", "q"]);
    assert_eq!(
        texts(define.tokens().expect("a body").tokens()),
        ["p", "+", "q"]
    );
    assert_eq!(
        command(".smart on").switch().map(|t| t.text().to_owned()),
        Some("on".to_owned())
    );
    assert_eq!(
        command(".smart +").switch().map(|t| t.text().to_owned()),
        Some("+".to_owned())
    );
    assert!(command(".smart").switch().is_none());
    let synthesised = command(".ident(\"x\"): nop");
    assert!(synthesised.word().is_none());
    assert!(synthesised.call().is_some());
}

#[test]
fn macros_and_members() {
    let lines = lines(".macro m p\n op #1\n.endmacro\nm 1, {2,3}\n");
    let Some(Statement::Command(header)) = lines[0].statement() else {
        panic!()
    };
    assert_eq!(header.name().map(|n| n.text()), Some("m".to_owned()));
    let Some(Statement::MacroBody(body)) = lines[1].statement() else {
        panic!()
    };
    assert_eq!(texts(body.tokens()), ["op", "#", "1"]);
    let Some(Statement::MacroCall(call)) = lines[3].statement() else {
        panic!()
    };
    assert_eq!(
        call.name().map(|t| t.text().to_owned()),
        Some("m".to_owned())
    );
    let args: Vec<Arg> = call.args().collect();
    assert_eq!(args.len(), 2);
    assert_eq!(
        texts(args[1].tokens().expect("tokens").contents()),
        ["2", ",", "3"]
    );
    let lines = self::lines(".struct s\n f1 .word 2\n.endstruct\n.enum\n red = 4\n.endenum\n");
    let Some(Statement::Member(field)) = lines[1].statement() else {
        panic!()
    };
    assert_eq!(field.name().map(|n| n.text()), Some("f1".to_owned()));
    let storage = field.storage().expect("storage");
    assert_eq!(
        storage.word().map(|t| t.text().to_owned()),
        Some(".word".to_owned())
    );
    assert!(field.value().is_none());
    let Some(Statement::Member(colour)) = lines[4].statement() else {
        panic!()
    };
    assert!(colour.storage().is_none() && colour.value().is_some());
}

#[test]
fn every_statement_casts_back_to_its_node() {
    let source =
        "nop\n.byte 1\n.macro m\n x\n.endmacro\nm\nv = 1\n.struct s\n f1 .byte\n.endstruct\n";
    for line in lines(source) {
        if let Some(statement) = line.statement() {
            let again = Statement::cast(statement.syntax().clone());
            assert_eq!(again.as_ref(), Some(&statement));
        }
        assert_eq!(Line::cast(line.syntax().clone()).as_ref(), Some(&line));
    }
    let expr = immediate("(1)+-2");
    assert_eq!(Expr::cast(expr.syntax().clone()).as_ref(), Some(&expr));
    for part in ["1", "x9", ":+", "(1)", "-1", "1+1", ".lobyte(1)"] {
        let expr = immediate(part);
        assert_eq!(Expr::cast(expr.syntax().clone()), Some(expr));
    }
    assert!(Expr::cast(first("nop").syntax().clone()).is_none());
    assert!(Statement::cast(first("nop").syntax().clone()).is_none());
}
