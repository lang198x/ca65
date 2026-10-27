//! Each test pins behaviour measured by running ca65 V2.19 at `8d86ca7`.

use lang198x_syntax::{SyntaxElement, SyntaxNode};
use proptest::prelude::*;

use super::*;

mod commands;

pub(super) fn parsed(text: &str) -> Parse {
    parse(text, Profile::CA65_2_19_6502).expect("short text always parses")
}

/// The tree as `(KIND children...)`, without whitespace, so tests read like
/// the source.
pub(super) fn render(node: &SyntaxNode) -> String {
    let parts: Vec<String> = node
        .children()
        .filter_map(|child| match child {
            SyntaxElement::Node(node) => Some(render(&node)),
            SyntaxElement::Token(token) => match token.kind() {
                SyntaxKind::WHITESPACE => None,
                SyntaxKind::NEWLINE => Some("NL".to_owned()),
                _ => Some(token.text().to_owned()),
            },
            _ => None,
        })
        .collect();
    format!("({} {})", kind_name(node.kind()), parts.join(" "))
}

fn kind_name(kind: SyntaxKind) -> &'static str {
    kinds::name(kind).or(kind.shared_name()).unwrap_or("?")
}

/// The first line of `text`, rendered.
pub(super) fn line(text: &str) -> String {
    let parse = parsed(text);
    let first = parse
        .tree()
        .root()
        .children()
        .next()
        .and_then(SyntaxElement::into_node)
        .expect("a line");
    render(&first)
}

/// The diagnostics of `text` as `start..end message`.
pub(super) fn problems(text: &str) -> Vec<String> {
    parsed(text)
        .diagnostics()
        .iter()
        .map(|d| format!("{} {}", d.range(), d.message()))
        .collect()
}

// ---- Lines ------------------------------------------------------------------

#[test]
fn a_label_then_an_instruction() {
    assert_eq!(
        line("loop: dex ; again\n"),
        "(LINE (LABEL (NAME loop) :) (INSTRUCTION dex) ; again NL)"
    );
    assert!(problems("loop: dex\n").is_empty());
}

#[test]
fn every_line_is_its_own_node() {
    let parse = parsed("a1:\n\n nop\n");
    let lines = parse.tree().root().children().count();
    assert_eq!(lines, 3);
    assert_eq!(parse.tree().root().text(), "a1:\n\n nop\n");
}

#[test]
fn unnamed_labels() {
    // Measured: `:` alone and `@:` both define an unnamed label, and a
    // label may follow on the same line.
    assert_eq!(line(": nop"), "(LINE (UNNAMED_LABEL :) (INSTRUCTION nop))");
    assert_eq!(
        line("@: nop"),
        "(LINE (UNNAMED_LABEL @ :) (INSTRUCTION nop))"
    );
    assert_eq!(
        line(": here: nop"),
        "(LINE (UNNAMED_LABEL :) (LABEL (NAME here) :) (INSTRUCTION nop))"
    );
    assert_eq!(
        problems("@+:"),
        ["0..2 an unnamed label is defined with `@` alone"]
    );
    assert_eq!(problems("@ nop"), ["2..2 expected `:` after `@`"]);
}

#[test]
fn cheap_and_scoped_names() {
    assert_eq!(
        line("@loop: dex"),
        "(LINE (LABEL (NAME @loop) :) (INSTRUCTION dex))"
    );
    assert_eq!(
        line("outer::inner::v = 1"),
        "(LINE (ASSIGNMENT (NAME outer :: inner :: v) = (LITERAL 1)))"
    );
    assert_eq!(
        line("::v = 1"),
        "(LINE (ASSIGNMENT (NAME :: v) = (LITERAL 1)))"
    );
    assert_eq!(problems("q:: = 1"), ["4..4 expected a name after `::`"]);
    // `x` is a register, so it cannot end a scoped name.
    assert!(!problems("s::x = 1").is_empty());
}

#[test]
fn assignments_end_their_line() {
    // ca65 accepts all three, and refuses anything after one.
    assert_eq!(line("v = 2"), "(LINE (ASSIGNMENT (NAME v) = (LITERAL 2)))");
    assert_eq!(
        line("v := 2"),
        "(LINE (ASSIGNMENT (NAME v) := (LITERAL 2)))"
    );
    assert_eq!(
        line("v .set 2"),
        "(LINE (ASSIGNMENT (NAME v) .set (LITERAL 2)))"
    );
    assert_eq!(
        line("v .SET 2"),
        "(LINE (ASSIGNMENT (NAME v) .SET (LITERAL 2)))"
    );
    assert_eq!(problems("v = 2 nop"), ["6..9 expected the end of the line"]);
}

#[test]
fn a_name_needs_a_colon_to_be_a_label() {
    // ca65: "Expected `:' after identifier to form a label". With no macros
    // yet, `foo bar` is refused the same way.
    assert_eq!(
        problems("foo bar\n"),
        ["0..7 expected `:` after a name to make it a label"]
    );
    assert_eq!(
        problems("foo\n"),
        ["0..3 expected `:` after a name to make it a label"]
    );
    // Only one named label per line.
    assert_eq!(problems("a1: b1: nop"), ["4..11 expected a mnemonic"]);
}

#[test]
fn a_statement_must_start_with_a_mnemonic_or_command() {
    // ca65: "Expected a mnemonic". `a:` and `x:` are refused too: the
    // first is an address size, the second a register.
    for bad in ["1", "#1", "x: nop", "a: nop", "(1)"] {
        let found = problems(bad);
        assert_eq!(found.len(), 1, "{bad}");
        assert!(
            found[0].ends_with("expected a mnemonic"),
            "{bad}: {found:?}"
        );
    }
}

#[test]
fn a_mnemonic_is_never_a_label() {
    // Measured: `lda: nop` is refused, because `lda` is read as an
    // instruction.
    assert!(!problems("lda: nop").is_empty());
    assert!(!line("lda: nop").contains("LABEL"));
}

#[test]
fn a_label_before_a_control_command() {
    assert_eq!(
        line("tab: .byte 1, \"a\" ; data"),
        "(LINE (LABEL (NAME tab) :) (COMMAND .byte (LITERAL 1) , (LITERAL \"a\")) ; data)"
    );
}

#[test]
fn comment_only_and_blank_lines() {
    assert_eq!(line("; note\n"), "(LINE ; note NL)");
    assert_eq!(line("   \n"), "(LINE NL)");
    assert!(problems("; note\n\n").is_empty());
}

// ---- Operands ---------------------------------------------------------------

#[test]
fn implied_and_accumulator() {
    assert_eq!(line("rts"), "(LINE (INSTRUCTION rts))");
    assert_eq!(line("asl a"), "(LINE (INSTRUCTION asl (OPERAND a)))");
    assert_eq!(line("ASL A"), "(LINE (INSTRUCTION ASL (OPERAND A)))");
}

#[test]
fn immediate_and_addresses() {
    assert_eq!(
        line("lda #<$1234"),
        "(LINE (INSTRUCTION lda (OPERAND (IMMEDIATE # (UNARY_EXPR < (LITERAL $1234))))))"
    );
    assert_eq!(
        line("sta $0200,x"),
        "(LINE (INSTRUCTION sta (OPERAND (LITERAL $0200) (INDEX , x))))"
    );
    assert_eq!(
        line("ldx z:$12,Y"),
        "(LINE (INSTRUCTION ldx (OPERAND z: (LITERAL $12) (INDEX , Y))))"
    );
}

#[test]
fn indirect_operands() {
    assert_eq!(
        line("lda ($12,x)"),
        "(LINE (INSTRUCTION lda (OPERAND (INDIRECT ( (LITERAL $12) , x )))))"
    );
    assert_eq!(
        line("lda ($12),y"),
        "(LINE (INSTRUCTION lda (OPERAND (INDIRECT ( (LITERAL $12) ) , y))))"
    );
    assert_eq!(
        line("jmp ($fffc)"),
        "(LINE (INSTRUCTION jmp (OPERAND (INDIRECT ( (LITERAL $fffc) )))))"
    );
    assert_eq!(
        line("lda [$12],y"),
        "(LINE (INSTRUCTION lda (OPERAND (LONG_INDIRECT [ (LITERAL $12) ] , y))))"
    );
    assert!(problems("lda ($12,x),y").is_empty());
}

#[test]
fn a_leading_parenthesis_is_always_indirect() {
    // ca65 refuses `lda (1+2)*3`: the `(` starts an indirect operand.
    assert_eq!(
        problems("lda (1+2)*3"),
        ["9..11 expected the end of the line"]
    );
}

#[test]
fn indirect_operands_check_their_registers() {
    assert_eq!(problems("lda ($12,y)"), ["9..11 expected `x` or `s`"]);
    assert_eq!(problems("lda ($12),x"), ["10..11 expected `y`"]);
    assert_eq!(problems("lda ($12"), ["8..8 expected `)`"]);
    assert_eq!(problems("lda ($12,"), ["9..9 expected `x` or `s`"]);
    assert_eq!(problems("lda ($12 1"), ["9..10 expected `)`"]);
    assert_eq!(problems("lda [$12"), ["8..8 expected `]`"]);
    assert_eq!(problems("lda [$12],x"), ["10..11 expected `y`"]);
    // On the 6502, `s` is a name, not a register.
    assert_eq!(problems("lda ($12,s),y"), ["9..13 expected `x` or `s`"]);
}

#[test]
fn a_second_operand_after_a_comma() {
    // The bit branches of later CPUs take `address, target`; the 6502's
    // instructions refuse it when they are assembled.
    // `bbr0` is not a 6502 mnemonic, so ca65 refuses the line as a label
    // without its colon.
    assert_eq!(
        problems("bbr0 $12, there"),
        ["0..15 expected `:` after a name to make it a label"]
    );
    assert_eq!(
        line("lda $12, there"),
        "(LINE (INSTRUCTION lda (OPERAND (LITERAL $12) ,) (OPERAND (NAME_REF there))))"
    );
}

// ---- Expressions ------------------------------------------------------------

/// Evaluates the expression node `node` the way ca65 does, for the values
/// these tests use: numbers, characters, `.lobyte` and `.hibyte`.
fn evaluate(node: &SyntaxNode) -> i64 {
    let children: Vec<SyntaxElement> = node
        .children()
        .filter(|c| !matches!(c, SyntaxElement::Token(t) if t.kind() == SyntaxKind::WHITESPACE))
        .collect();
    let operator = |index: usize| match &children[index] {
        SyntaxElement::Token(token) => token.text().to_ascii_lowercase(),
        _ => String::new(),
    };
    let operand = |index: usize| match &children[index] {
        SyntaxElement::Node(node) => evaluate(node),
        _ => panic!("expected an expression"),
    };
    let truth = |b: bool| i64::from(b);
    match node.kind() {
        kinds::LITERAL => number(node.text()),
        kinds::PAREN_EXPR => operand(1),
        kinds::UNARY_EXPR => {
            let value = operand(1);
            match operator(0).as_str() {
                "+" => value,
                "-" => -value,
                "~" | ".bitnot" => !value,
                "<" => value & 0xFF,
                ">" => (value >> 8) & 0xFF,
                "^" => (value >> 16) & 0xFF,
                "!" | ".not" => truth(value == 0),
                other => panic!("unknown prefix {other}"),
            }
        }
        kinds::BIN_EXPR => {
            let (left, right) = (operand(0), operand(2));
            match operator(1).as_str() {
                "||" | ".or" => truth(left != 0 || right != 0),
                "&&" | ".and" => truth(left != 0 && right != 0),
                ".xor" => truth((left != 0) ^ (right != 0)),
                "=" => truth(left == right),
                "<>" => truth(left != right),
                "<" => truth(left < right),
                ">" => truth(left > right),
                "<=" => truth(left <= right),
                ">=" => truth(left >= right),
                "+" => left + right,
                "-" => left - right,
                "|" | ".bitor" => left | right,
                "*" => left * right,
                "/" => left / right,
                ".mod" => left % right,
                "&" | ".bitand" => left & right,
                "^" | ".bitxor" => left ^ right,
                "<<" | ".shl" => left << right,
                ">>" | ".shr" => left >> right,
                other => panic!("unknown operator {other}"),
            }
        }
        kinds::CALL_EXPR => {
            let argument = node
                .children()
                .filter_map(SyntaxElement::into_node)
                .next()
                .and_then(|arg| arg.children().find_map(SyntaxElement::into_node))
                .map_or(0, |value| evaluate(&value));
            match operator(0).as_str() {
                ".lobyte" => argument & 0xFF,
                ".hibyte" => (argument >> 8) & 0xFF,
                other => panic!("unknown function {other}"),
            }
        }
        other => panic!("cannot evaluate {}", kind_name(other)),
    }
}

fn number(text: &str) -> i64 {
    let parsed = if let Some(hex) = text.strip_prefix('$') {
        i64::from_str_radix(hex, 16)
    } else if let Some(binary) = text.strip_prefix('%') {
        i64::from_str_radix(binary, 2)
    } else if let Some(hex) = text.strip_suffix(['h', 'H']) {
        i64::from_str_radix(hex, 16)
    } else {
        text.parse()
    };
    parsed.unwrap_or_else(|_| panic!("not a number: {text}"))
}

/// The low byte of `expression`, parsed as an `lda #` operand.
fn byte(expression: &str) -> u8 {
    let parse = parsed(&format!("lda #{expression}"));
    assert!(
        parse.diagnostics().is_empty(),
        "{expression}: {:?}",
        parse.diagnostics()
    );
    let immediate = parse
        .tree()
        .root()
        .descendants()
        .filter_map(SyntaxElement::into_node)
        .find(|n| n.kind() == kinds::IMMEDIATE)
        .expect("an immediate operand");
    let value = immediate
        .children()
        .find_map(SyntaxElement::into_node)
        .map(|n| evaluate(&n))
        .expect("a value");
    (value & 0xFF) as u8
}

#[test]
fn precedence_matches_ca65() {
    for &(expression, expected) in crate::measured::EXPRESSIONS {
        assert_eq!(byte(expression), expected, "{expression}");
    }
}

#[test]
fn not_takes_the_rest_even_after_an_operator() {
    // ca65's `Factor` reads `!` with its loosest rule wherever it appears.
    assert_eq!(byte("1+!0||1"), 0x01);
    assert_eq!(
        line("lda #1+.not 0"),
        "(LINE (INSTRUCTION lda (OPERAND (IMMEDIATE # (BIN_EXPR (LITERAL 1) + (UNARY_EXPR .not (LITERAL 0)))))))"
    );
}

#[test]
fn values() {
    assert_eq!(
        line("lda @l,x"),
        "(LINE (INSTRUCTION lda (OPERAND (NAME_REF @l) (INDEX , x))))"
    );
    assert_eq!(
        line("jmp :+"),
        "(LINE (INSTRUCTION jmp (OPERAND (UNNAMED_REF :+))))"
    );
    assert_eq!(
        line("jmp s::v"),
        "(LINE (INSTRUCTION jmp (OPERAND (NAME_REF s :: v))))"
    );
    assert_eq!(
        line("lda #'a'"),
        "(LINE (INSTRUCTION lda (OPERAND (IMMEDIATE # (LITERAL 'a')))))"
    );
    assert_eq!(
        line("jmp *+3"),
        "(LINE (INSTRUCTION jmp (OPERAND (BIN_EXPR (LITERAL *) + (LITERAL 3)))))"
    );
}

#[test]
fn functions() {
    assert_eq!(
        line("v = .max(1, 2)"),
        "(LINE (ASSIGNMENT (NAME v) = (CALL_EXPR .max ( (ARG (LITERAL 1)) , (ARG (LITERAL 2)) ))))"
    );
    assert_eq!(
        line("v = .time"),
        "(LINE (ASSIGNMENT (NAME v) = (CALL_EXPR .time)))"
    );
    assert_eq!(
        line("v = .tcount({1, 2})"),
        "(LINE (ASSIGNMENT (NAME v) = (CALL_EXPR .tcount ( (ARG (TOKEN_LIST { 1 , 2 })) ))))"
    );
    assert_eq!(
        line("v = .match(a b, c)"),
        "(LINE (ASSIGNMENT (NAME v) = (CALL_EXPR .match ( (ARG (TOKEN_LIST a b)) , (ARG (TOKEN_LIST c)) ))))"
    );
    assert_eq!(
        line("v = .left(1, {x (y) z})"),
        "(LINE (ASSIGNMENT (NAME v) = (CALL_EXPR .left ( (ARG (LITERAL 1)) , (ARG (TOKEN_LIST { x ( y ) z })) ))))"
    );
    assert_eq!(
        line("v = .blank((a, b))"),
        "(LINE (ASSIGNMENT (NAME v) = (CALL_EXPR .blank ( (ARG (TOKEN_LIST ( a , b ))) ))))"
    );
    assert_eq!(
        line("v = .tcount({{1})"),
        "(LINE (ASSIGNMENT (NAME v) = (CALL_EXPR .tcount ( (ARG (TOKEN_LIST { { 1 })) ))))"
    );
}

#[test]
fn broken_expressions_are_reported() {
    assert_eq!(problems("v = "), ["4..4 expected a value"]);
    assert_eq!(problems("v = (1"), ["6..6 expected `)`"]);
    assert_eq!(problems("v = .lobyte 1"), ["12..13 expected `(`"]);
    assert_eq!(problems("v = .lobyte"), ["11..11 expected `(`"]);
    assert_eq!(problems("v = .max(1 2)"), ["11..13 expected `)`"]);
    assert_eq!(problems("v = .byte"), ["4..9 expected a value"]);
    assert_eq!(problems("v = )"), ["4..5 expected a value"]);
    assert_eq!(
        problems("v = .tcount({1"),
        ["14..14 expected `}`", "14..14 expected `)`"]
    );
    assert_eq!(problems("v = 1 +"), ["7..7 expected a value"]);
    // The lexer's own diagnostic stands for a bad character.
    assert_eq!(problems("v = \\"), ["4..5 '\\\\' is not allowed here"]);
}

#[test]
fn deep_nesting_stops_safely() {
    let deep = format!("v = {}1", "(".repeat(500));
    assert!(
        problems(&deep)
            .iter()
            .any(|p| p.ends_with("this expression is nested too deeply"))
    );
    let prefixes = format!("v = {}1", "-".repeat(500));
    assert!(
        problems(&prefixes)
            .iter()
            .any(|p| p.ends_with("this expression is nested too deeply"))
    );
}

#[test]
fn the_parse_error_describes_itself() {
    let error = ParseError::Build(lang198x_syntax::BuildError::TextTooLong);
    assert!(
        error
            .to_string()
            .starts_with("the syntax tree could not be built")
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Any text parses into a tree that gives the text back, with every
    /// diagnostic inside it.
    #[test]
    fn any_text_parses_losslessly(text in "\\PC{0,40}|[ -~\\t\\r\\n]{0,80}") {
        let parse = parsed(&text);
        let root = parse.tree().root();
        prop_assert_eq!(root.text(), text.as_str());
        for diagnostic in parse.diagnostics() {
            prop_assert!(diagnostic.range().slice(&text).is_some());
        }
    }

    /// Tokens prepared from the lexer give the tree the lexer gives, and the
    /// parser's own problems: an expansion parsed from its tokens reads as it
    /// would have read as text, unless a token was made.
    #[test]
    fn prepared_tokens_parse_as_the_text_does(text in "\\PC{0,40}|[ -~\\t\\r\\n]{0,80}") {
        let profile = Profile::CA65_2_19_6502;
        let lexed = crate::lex(&text, Mode::of(profile));
        let from_text = parsed(&text);
        let prepared = parse_prepared(&text, lexed.tokens().to_vec(), profile, &mut Standalone)
            .expect("short text always parses");
        let shape = |parse: &Parse| -> Vec<(SyntaxKind, TextRange)> {
            parse
                .tree()
                .root()
                .descendants()
                .map(|element| match element {
                    SyntaxElement::Node(node) => (node.kind(), node.text_range()),
                    SyntaxElement::Token(token) => (token.kind(), token.text_range()),
                    _ => (SyntaxKind::ERROR, TextRange::empty(TextSize::ZERO)),
                })
                .collect()
        };
        prop_assert_eq!(shape(&prepared), shape(&from_text));
        let own: Vec<&Diagnostic> = from_text
            .diagnostics()
            .iter()
            .filter(|d| !lexed.diagnostics().contains(d))
            .collect();
        prop_assert_eq!(prepared.diagnostics().iter().collect::<Vec<_>>(), own);
    }
}
