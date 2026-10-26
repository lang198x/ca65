//! Control commands' shapes, macros and the parser's questions, each
//! measured on ca65 V2.19 or read from its `pseudo.c`.

use super::{line, parsed, problems};
use crate::{Context, Profile, kinds, parse_with};

/// The kinds of the diagnostics of `text`, for the unknown/invalid split.
fn kinds_of(text: &str) -> Vec<lang198x_syntax::DiagnosticKind> {
    parsed(text)
        .diagnostics()
        .iter()
        .map(|d| d.kind())
        .collect()
}

/// Line `n` of `text`, rendered.
fn nth_line(text: &str, n: usize) -> String {
    let parse = parsed(text);
    let node = parse
        .tree()
        .root()
        .children()
        .filter_map(lang198x_syntax::SyntaxElement::into_node)
        .nth(n)
        .expect("that line");
    super::render(&node)
}

// ---- Shapes -----------------------------------------------------------------

#[test]
fn bare_commands_take_nothing() {
    for bare in [".endif", ".code", ".p02", ".else", ".popseg", ".end"] {
        assert_eq!(line(bare), format!("(LINE (COMMAND {bare}))"));
        assert!(problems(bare).is_empty(), "{bare}");
    }
    assert_eq!(problems(".code 1"), ["6..7 expected the end of the line"]);
}

#[test]
fn values_separated_by_commas() {
    assert_eq!(
        line(".res 4, $ff"),
        "(LINE (COMMAND .res (LITERAL 4) , (LITERAL $ff)))"
    );
    assert_eq!(
        line(".if v > 1"),
        "(LINE (COMMAND .if (BIN_EXPR (NAME_REF v) > (LITERAL 1))))"
    );
    assert_eq!(
        line(".assert v, error, \"m\""),
        "(LINE (COMMAND .assert (NAME_REF v) , (NAME_REF error) , (LITERAL \"m\")))"
    );
    assert_eq!(problems(".byte"), ["5..5 expected a value"]);
    assert_eq!(problems(".byte 1,"), ["8..8 expected a value"]);
}

#[test]
fn import_and_export_lists() {
    assert_eq!(
        line(".import one, two: zeropage"),
        "(LINE (COMMAND .import (ITEM (NAME one)) , (ITEM (NAME two) : zeropage)))"
    );
    assert_eq!(
        line(".export v = 1, w := 2"),
        "(LINE (COMMAND .export (ITEM (NAME v) = (LITERAL 1)) , (ITEM (NAME w) := (LITERAL 2))))"
    );
    // ca65 refuses a register-spelt name here (asm198x#926).
    assert_eq!(problems(".import x"), ["8..9 expected a name"]);
    assert_eq!(problems(".import"), ["7..7 expected a name"]);
    assert_eq!(problems(".import v:"), ["10..10 expected an address size"]);
}

#[test]
fn one_name() {
    assert_eq!(
        line(".ifdef s::v"),
        "(LINE (COMMAND .ifdef (NAME_REF s :: v)))"
    );
    assert_eq!(problems(".ifdef 1"), ["7..8 expected a name"]);
}

#[test]
fn switches() {
    for on in [".smart", ".smart on", ".smart OFF", ".smart +", ".list -"] {
        assert!(problems(on).is_empty(), "{on}");
    }
    assert_eq!(problems(".smart maybe"), ["7..12 expected `on` or `off`"]);
}

#[test]
fn blocks_and_segments() {
    assert_eq!(line(".proc main"), "(LINE (COMMAND .proc (NAME main)))");
    assert_eq!(
        line(".proc main: far"),
        "(LINE (COMMAND .proc (NAME main) : far))"
    );
    assert_eq!(line(".scope"), "(LINE (COMMAND .scope))");
    assert_eq!(
        line(".segment \"ZP\": zeropage"),
        "(LINE (COMMAND .segment (LITERAL \"ZP\") : zeropage))"
    );
    assert!(problems(".scope\n.endscope\n.endproc\n").is_empty());
}

#[test]
fn struct_members_need_no_colon() {
    let source = ".struct point\n xcoord .word\n ycoord .word\n .byte\n.endstruct\nv = 1\n";
    assert!(problems(source).is_empty(), "{:?}", problems(source));
    assert_eq!(
        nth_line(source, 1),
        "(LINE (MEMBER (NAME xcoord) (COMMAND .word)) NL)"
    );
    assert_eq!(nth_line(source, 3), "(LINE (COMMAND .byte) NL)");
    // Storage takes an optional count, `.res` a size, and `.tag` a struct
    // and an optional count. Structs nest.
    let counts = ".struct s\n f1 .byte 2\n f2 .res 3\n f3 .tag point, 2\n .struct inner\n f4 .word\n .endstruct\n.endstruct\n";
    assert!(problems(counts).is_empty(), "{:?}", problems(counts));
    assert_eq!(
        nth_line(counts, 3),
        "(LINE (MEMBER (NAME f3) (COMMAND .tag (NAME_REF point) , (LITERAL 2))) NL)"
    );
    assert_eq!(
        problems(".struct s\n b .res\n.endstruct\n"),
        ["17..17 expected a value"]
    );
    // After `.endstruct`, a name needs its colon again.
    assert_eq!(problems(".struct s\n.endstruct\nf .word\n").len(), 1);
}

#[test]
fn enum_members() {
    let source = ".enum colour\n red\n green = 5\n.endenum\n";
    assert!(problems(source).is_empty(), "{:?}", problems(source));
    assert_eq!(nth_line(source, 1), "(LINE (MEMBER (NAME red)) NL)");
    assert_eq!(
        nth_line(source, 2),
        "(LINE (MEMBER (NAME green) = (LITERAL 5)) NL)"
    );
}

#[test]
fn rest_of_line_commands_keep_their_tokens() {
    assert_eq!(
        line(".ifblank {a, b}"),
        "(LINE (COMMAND .ifblank (TOKEN_LIST { a , b })))"
    );
    assert_eq!(
        line(".dbg line, \"x.c\", 3"),
        "(LINE (COMMAND .dbg (TOKEN_LIST line , \"x.c\" , 3)))"
    );
}

#[test]
fn a_token_function_can_start_a_statement() {
    // Measured: `.ident("there"): nop` defines the label `there`. Its tokens
    // are read once preprocessing has made them.
    assert_eq!(
        line(".ident(\"there\"): nop"),
        "(LINE (COMMAND (CALL_EXPR .ident ( (ARG (LITERAL \"there\")) )) (TOKEN_LIST : nop)))"
    );
    assert!(problems(".ident(\"there\"): nop").is_empty());
}

#[test]
fn reading_options_are_not_supported_yet() {
    use lang198x_syntax::DiagnosticKind;
    for command in [
        ".feature c_comments",
        ".case off",
        ".linecont +",
        ".localchar '?'",
    ] {
        assert_eq!(
            kinds_of(command),
            [DiagnosticKind::Unsupported],
            "{command}"
        );
    }
    assert!(problems(".feature c_comments")[0].contains("not supported yet"));
}

#[test]
fn expression_words_cannot_start_a_statement() {
    // ca65 calls these unexpected at the start of a statement.
    assert_eq!(
        problems(".lobyte(1)"),
        ["0..10 `.lobyte` cannot start a statement"]
    );
    assert_eq!(problems(".and"), ["0..4 `.and` cannot start a statement"]);
}

#[test]
fn an_unknown_command_is_reported_once() {
    assert_eq!(
        problems(".bogus 1, 2"),
        ["0..6 `.bogus` is not a control command ca65 knows"]
    );
    assert_eq!(line(".bogus 1"), "(LINE (COMMAND .bogus (TOKEN_LIST 1)))");
}

// ---- Macros -----------------------------------------------------------------

#[test]
fn a_macro_body_is_kept_as_tokens() {
    // Measured: `.macro m op` / ` op #1` / `.endmacro` / `m lda` gives
    // A9 01. ca65 reads the body only when `m` is called, so `op` at the
    // start of a line is not refused.
    let source = ".macro m op\n op #1\n.endmacro\nm lda\n";
    assert!(problems(source).is_empty(), "{:?}", problems(source));
    assert_eq!(
        nth_line(source, 0),
        "(LINE (COMMAND .macro (NAME m) (PARAMS op)) NL)"
    );
    assert_eq!(nth_line(source, 1), "(LINE (MACRO_BODY op # 1) NL)");
    assert_eq!(nth_line(source, 2), "(LINE (COMMAND .endmacro) NL)");
    assert_eq!(
        nth_line(source, 3),
        "(LINE (MACRO_CALL m (ARG (TOKEN_LIST lda))) NL)"
    );
}

#[test]
fn a_body_ends_only_at_endmacro_first_on_its_line() {
    // Measured: `q: .endmacro` does not end the body, and an indented
    // `.endmacro` does.
    let source = ".macro m\n.byte 1\nq: .endmacro\n  .endmac\nm\n";
    assert!(problems(source).is_empty(), "{:?}", problems(source));
    assert_eq!(nth_line(source, 2), "(LINE (MACRO_BODY q : .endmacro) NL)");
    assert_eq!(nth_line(source, 4), "(LINE (MACRO_CALL m) NL)");
}

#[test]
fn macro_arguments_split_at_commas_outside_braces() {
    // Measured: braces protect a comma and parentheses do not, so
    // `m (1,2)` passes two arguments, and an argument may be empty.
    let defined = ".macro m p, q\n.endmacro\n";
    assert_eq!(
        nth_line(&format!("{defined}m (1,2)"), 2),
        "(LINE (MACRO_CALL m (ARG (TOKEN_LIST ( 1)) , (ARG (TOKEN_LIST 2 )))))"
    );
    assert_eq!(
        nth_line(&format!("{defined}m {{1,2}}, 3 4"), 2),
        "(LINE (MACRO_CALL m (ARG (TOKEN_LIST { 1 , 2 })) , (ARG (TOKEN_LIST 3 4))))"
    );
    assert_eq!(
        nth_line(&format!("{defined}m ,7"), 2),
        "(LINE (MACRO_CALL m (ARG ) , (ARG (TOKEN_LIST 7))))"
    );
}

#[test]
fn a_macro_is_found_before_a_label() {
    // Measured: with `m` defined, `m:` and `m = 2` call it and are refused
    // for passing an argument.
    let defined = ".macro m\n.endmacro\n";
    assert_eq!(
        nth_line(&format!("{defined}m:"), 2),
        "(LINE (MACRO_CALL m (ARG (TOKEN_LIST :))))"
    );
    assert!(nth_line(&format!("{defined}m = 2"), 2).starts_with("(LINE (MACRO_CALL m"));
    // After a label, a macro can be called too.
    assert_eq!(
        nth_line(&format!("{defined}here: m"), 2),
        "(LINE (LABEL (NAME here) :) (MACRO_CALL m))"
    );
}

#[test]
fn macro_headers_are_checked() {
    // Measured: ca65 refuses an instruction as a macro's name.
    assert_eq!(
        problems(".macro lda\n.endmacro\n"),
        ["7..10 an instruction cannot be a macro's name"]
    );
    assert_eq!(problems(".macro\n"), ["6..6 expected a name"]);
    assert_eq!(
        problems(".macro m 1\n.endmacro\n"),
        ["9..10 expected a parameter name"]
    );
    assert_eq!(
        problems(".endmacro"),
        ["0..9 `.endmacro` ends a macro, but no macro is being defined"]
    );
    assert_eq!(
        problems(".endmacro 1"),
        ["10..11 `.endmacro` ends a macro, but no macro is being defined"]
    );
}

#[test]
fn define_style_macros() {
    // Measured: `.define D lda #3` then `D` gives A9 03.
    let source = ".define D lda #3\nD\n";
    assert!(problems(source).is_empty());
    assert_eq!(
        nth_line(source, 0),
        "(LINE (COMMAND .define (NAME D) (TOKEN_LIST lda # 3)) NL)"
    );
    assert_eq!(nth_line(source, 1), "(LINE (MACRO_CALL D) NL)");
    assert_eq!(
        line(".define F(p) p+1"),
        "(LINE (COMMAND .define (NAME F) (PARAMS ( p )) (TOKEN_LIST p + 1)))"
    );
    assert_eq!(problems(".define F(p"), ["11..11 expected `)`"]);
    assert_eq!(problems(".define F(p q)"), ["12..14 expected `)`"]);
    // After `.undef`, the name is an ordinary name again.
    assert_eq!(problems(".define D 1\n.undef D\nD\n").len(), 1);
}

#[test]
fn deleting_a_macro() {
    // Measured: after `.delmac m`, `m` is refused as a label without a
    // colon.
    let source = ".mac m\n.endmac\nm\n.delmac m\nm\n";
    assert_eq!(
        problems(source),
        ["27..28 expected `:` after a name to make it a label"]
    );
}

#[test]
fn after_an_include_an_unknown_name_may_be_a_macro() {
    use lang198x_syntax::DiagnosticKind;
    // Read on its own, a file cannot see the macros an include defines.
    assert_eq!(kinds_of("blink 3\n"), [DiagnosticKind::Invalid]);
    assert_eq!(
        kinds_of(".include \"x.inc\"\nblink 3\n"),
        [DiagnosticKind::Unknown]
    );
    assert_eq!(
        nth_line(".macpack longbranch\njeq there\n", 1),
        "(LINE (MACRO_CALL jeq (ARG (TOKEN_LIST there))) NL)"
    );
    // A name that can only be a label or an assignment stays one.
    assert!(problems(".include \"x.inc\"\nhere: nop\nv = 1\nw .set 2\n").is_empty());
}

#[test]
fn a_context_answers_for_macros_defined_elsewhere() {
    struct Known(&'static str);

    impl Context for Known {
        fn is_macro(&mut self, name: &str) -> Option<bool> {
            Some(name == self.0)
        }
    }

    let parse = parse_with(
        ".include \"x.inc\"\nblink 3\nother 4\n",
        Profile::CA65_2_19_6502,
        &mut Known("blink"),
    )
    .expect("short text always parses");
    let messages: Vec<&str> = parse.diagnostics().iter().map(|d| d.message()).collect();
    assert_eq!(messages, ["expected `:` after a name to make it a label"]);
    let calls = parse
        .tree()
        .root()
        .descendants()
        .filter_map(lang198x_syntax::SyntaxElement::into_node)
        .filter(|n| n.kind() == kinds::MACRO_CALL)
        .count();
    assert_eq!(calls, 1);
}
