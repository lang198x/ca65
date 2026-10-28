//! Each test pins behaviour measured by running ca65 V2.19 at `8d86ca7`.
//!
//! Most cases are Asm198x's probes of ca65's default lexical rules
//! (asm198x#993), whose verdicts Asm198x records; the test names follow the
//! probes' notes. The rest were measured the same way while reading ca65's
//! scanner, and say so.

use proptest::prelude::*;

use super::*;
use crate::Profile;

const MODE: Mode = Mode::of(Profile::CA65_2_19_6502);

/// The tokens of `text` as `KIND text`, skipping whitespace, so tests read
/// like the source.
fn tokens(text: &str) -> Vec<String> {
    let lexed = lex(text, MODE);
    let mut pos = 0;
    let mut out = Vec::new();
    for token in lexed.tokens() {
        let end = pos + token.len().to_usize();
        if token.kind() != SyntaxKind::WHITESPACE {
            let name = kinds::name(token.kind())
                .or(token.kind().shared_name())
                .unwrap_or("?");
            out.push(format!("{name} {}", &text[pos..end]));
        }
        pos = end;
    }
    out
}

/// The diagnostics of `text` as `start..end message`.
fn problems(text: &str) -> Vec<String> {
    lex(text, MODE)
        .diagnostics()
        .iter()
        .map(|d| format!("{} {}", d.range(), d.message()))
        .collect()
}

#[test]
fn an_instruction_line_lexes_into_its_parts() {
    assert_eq!(
        tokens("start: lda ($12),y ; go\n"),
        [
            "IDENT start",
            "COLON :",
            "IDENT lda",
            "L_PAREN (",
            "NUMBER $12",
            "R_PAREN )",
            "COMMA ,",
            "REGISTER y",
            "COMMENT ; go",
            "NEWLINE \n",
        ]
    );
}

// ---- Numbers ----------------------------------------------------------------

#[test]
fn hex_binary_and_decimal_numbers() {
    // ca65 emits FF FE 0A FF 00 CD AB 0C 00.
    let source = ".byte $ff,$Fe,%1010,255,0\n.word $ABCD,00012\n";
    for number in ["$ff", "$Fe", "%1010", "255", "0", "$ABCD", "00012"] {
        assert!(
            tokens(source).contains(&format!("NUMBER {number}")),
            "{number}"
        );
    }
    assert!(problems(source).is_empty());
}

#[test]
fn a_0x_prefix_is_not_hex() {
    // ca65: "Expected 'end-of-line' but found 'x10'". The lexer splits it as
    // ca65 does; the parser refuses the name after the number.
    assert_eq!(tokens("0x10"), ["NUMBER 0", "IDENT x10"]);
    assert!(problems("0x10").is_empty());
}

#[test]
fn an_h_suffix_after_a_leading_digit_is_hex() {
    // ca65 emits 10 FF FF for `.byte 10h,0ffh,0FFH`.
    for number in ["10h", "0ffh", "0FFH"] {
        assert_eq!(tokens(number), [format!("NUMBER {number}")]);
        assert!(problems(number).is_empty(), "{number}");
    }
}

#[test]
fn underscores_in_a_number_need_a_feature() {
    // ca65: "Expected 'end-of-line' but found '_000'".
    assert_eq!(tokens("1_000"), ["NUMBER 1", "IDENT _000"]);
}

#[test]
fn a_lone_dollar_or_percent_is_reported() {
    // ca65: "Hexadecimal digit expected" and "Binary digit expected". With
    // no modulo operator, `7 % 2` is a broken binary number, measured too.
    assert_eq!(problems("$"), ["0..1 a hexadecimal digit must follow `$`"]);
    assert_eq!(problems("%"), ["0..1 a binary digit must follow `%`"]);
    assert_eq!(tokens("%2"), ["NUMBER %", "NUMBER 2"]);
    assert_eq!(tokens("7 % 2"), ["NUMBER 7", "NUMBER %", "NUMBER 2"]);
    assert_eq!(problems("$g"), ["0..1 a hexadecimal digit must follow `$`"]);
}

#[test]
fn a_leading_digit_takes_every_hex_digit() {
    // Measured: `.byte 1e3` gives "Invalid digits in number"; ca65 reads the
    // whole run, then finds `e` outside base 10.
    assert_eq!(tokens("1e3"), ["NUMBER 1e3"]);
    assert_eq!(problems("1e3"), ["0..3 `1e3` has digits outside its base"]);
    assert_eq!(
        problems("12AB"),
        ["0..4 `12AB` has digits outside its base"]
    );
    assert!(problems("12ABh").is_empty());
}

#[test]
fn numbers_keep_32_bits() {
    // Measured: each line is accepted or refused by ca65's scanner as here.
    // A decimal number passes the scanner up to 4294967299; see `number`.
    for fits in [
        "4294967295",
        "4294967296",
        "$FFFFFFFF",
        "$0FFFFFFFF",
        "0FFFFFFFFh",
    ] {
        assert!(problems(fits).is_empty(), "{fits}");
    }
    assert_eq!(
        problems("$123456789"),
        ["0..10 `$123456789` overflows 32 bits"]
    );
    assert_eq!(
        problems("1FFFFFFFFh"),
        ["0..10 `1FFFFFFFFh` is out of range"]
    );
    let bits = format!("%1{}", "0".repeat(32));
    assert_eq!(
        problems(&bits),
        [format!("0..34 `{bits}` overflows 32 bits")]
    );
    assert!(problems(&format!("%0{}", "1".repeat(32))).is_empty());
    // ca65 reads at most 16 digits after leading zeros; this many overflow
    // before then.
    assert_eq!(
        problems("99999999999999999999"),
        ["0..20 `99999999999999999999` is out of range"]
    );
}

#[test]
fn star_is_the_address() {
    // ca65 emits 00 10 04 10 for `.org $1000` then `.word *,*+2`.
    assert_eq!(
        tokens("*,*+2"),
        ["STAR *", "COMMA ,", "STAR *", "PLUS +", "NUMBER 2"]
    );
}

// ---- Characters and strings -------------------------------------------------

#[test]
fn character_constants() {
    // ca65 emits 61 41 20 22.
    assert_eq!(
        tokens("'a','A',' ','\"'"),
        [
            "CHAR 'a'",
            "COMMA ,",
            "CHAR 'A'",
            "COMMA ,",
            "CHAR ' '",
            "COMMA ,",
            "CHAR '\"'"
        ]
    );
    // Measured: `'''` is a quote.
    assert_eq!(tokens("'''"), ["CHAR '''"]);
}

#[test]
fn a_bad_character_constant_runs_to_the_line_end() {
    // ca65: "Illegal character constant" for each of these, and it reads on
    // to the end of the line. `é` is two bytes, and a zero byte ends the
    // constant, both measured too.
    for bad in ["'ab', 1", "''", "'", "'é'", "'\0'"] {
        assert_eq!(tokens(bad), [format!("CHAR {bad}")], "{bad}");
        assert_eq!(
            problems(bad),
            [format!(
                "0..{} a character constant holds exactly one character",
                bad.len()
            )]
        );
    }
    assert_eq!(tokens("'\n1"), ["CHAR '", "NEWLINE \n", "NUMBER 1"]);
}

#[test]
fn strings_and_numbers_in_one_list() {
    // ca65 emits 61 62 01 63.
    assert_eq!(
        tokens("\"ab\",1,\"c\""),
        [
            "STRING \"ab\"",
            "COMMA ,",
            "NUMBER 1",
            "COMMA ,",
            "STRING \"c\""
        ]
    );
}

#[test]
fn a_backslash_in_a_string_is_plain_text() {
    // ca65 emits 61 5C 6E, and 69 74 27 73 for "it's".
    assert_eq!(tokens("\"a\\n\""), ["STRING \"a\\n\""]);
    assert_eq!(tokens("\"it's\""), ["STRING \"it's\""]);
    assert!(problems("\"a\\n\"").is_empty());
}

#[test]
fn an_empty_string_and_adjacent_strings() {
    // ca65 emits nothing for `""`. Measured: `"a""b"` is two strings, so
    // ca65 refuses the second.
    assert_eq!(tokens("\"\""), ["STRING \"\""]);
    assert_eq!(tokens("\"a\"\"b\""), ["STRING \"a\"", "STRING \"b\""]);
}

#[test]
fn an_unterminated_string_stops_at_the_line_end() {
    // ca65: "Newline in string constant".
    assert_eq!(tokens("\"ab\n1"), ["STRING \"ab", "NEWLINE \n", "NUMBER 1"]);
    assert_eq!(problems("\"ab\n"), ["0..3 this string has no closing \""]);
    assert_eq!(tokens("\"a\r\""), ["STRING \"a", "NEWLINE \r", "STRING \""]);
}

// ---- Names ------------------------------------------------------------------

#[test]
fn names_keep_their_case() {
    // ca65 emits 01 02 for `Foo=1`, `foo=2`, `.byte Foo,foo`.
    assert_eq!(tokens("Foo=1"), ["IDENT Foo", "EQ =", "NUMBER 1"]);
    assert_eq!(tokens("_a1 b_2"), ["IDENT _a1", "IDENT b_2"]);
}

#[test]
fn control_commands_ignore_case() {
    // ca65 emits 01 02 03 for `.BYTE`, `.Byte` and `.byte`.
    for word in [".BYTE", ".Byte", ".byte", ".lobyte", ".AND"] {
        assert_eq!(tokens(word), [format!("CONTROL {word}")]);
        assert!(problems(word).is_empty(), "{word}");
    }
}

#[test]
fn an_unknown_control_command_is_reported_wherever_it_is() {
    // ca65 refuses `.foo=1`, and `.bogus` even in an untaken `.if 0`
    // branch, because its scanner reports it before any condition is
    // looked at.
    assert_eq!(tokens(".foo=1"), ["CONTROL .foo", "EQ =", "NUMBER 1"]);
    assert_eq!(
        problems(".bogus"),
        ["0..6 `.bogus` is not a control command ca65 knows"]
    );
}

#[test]
fn a_dot_on_its_own() {
    // Measured: `. byte 1` gives "Expected 'end-of-line' but found '.'".
    assert_eq!(tokens(". byte"), ["DOT .", "IDENT byte"]);
    assert_eq!(tokens(".1"), ["DOT .", "NUMBER 1"]);
}

#[test]
fn at_and_dollar_inside_a_name_need_a_feature() {
    // ca65 refuses `a@b=1` and `a$b=1`; its first error, "Expected a
    // mnemonic but found 'A'", shows it read `a` as the register.
    assert_eq!(tokens("a@b"), ["REGISTER a", "CHEAP_IDENT @b"]);
    assert_eq!(tokens("a$b"), ["REGISTER a", "NUMBER $b"]);
    assert_eq!(tokens("q@w"), ["IDENT q", "CHEAP_IDENT @w"]);
}

#[test]
fn mnemonics_are_names() {
    // ca65 emits A9 01 A9 02 A9 03 for `LDA #1`, `lda #2`, `LdA #3`.
    assert_eq!(tokens("LdA #3"), ["IDENT LdA", "HASH #", "NUMBER 3"]);
}

#[test]
fn registers_ignore_case() {
    for register in ["a", "A", "x", "X", "y", "Y"] {
        assert_eq!(tokens(register), [format!("REGISTER {register}")]);
    }
    // On the 6502, S and Z are names, and longer words are never registers.
    assert_eq!(tokens("s z ax"), ["IDENT s", "IDENT z", "IDENT ax"]);
}

#[test]
fn cheap_locals() {
    // ca65 emits 01 02 01 00 03 04 05 00.
    assert_eq!(
        tokens("@l: .word @l"),
        [
            "CHEAP_IDENT @l",
            "COLON :",
            "CONTROL .word",
            "CHEAP_IDENT @l"
        ]
    );
    assert_eq!(tokens("@1x"), ["CHEAP_IDENT @1x"]);
}

#[test]
fn unnamed_labels_and_their_references() {
    // ca65 emits 00 00 06 00 07 00 01 02 for the probe. Measured too: `:<`
    // and `:>` count like `:-` and `:+`, and `@` works like `:`.
    assert_eq!(
        tokens(":\n.word :-,:+,:++"),
        [
            "COLON :",
            "NEWLINE \n",
            "CONTROL .word",
            "UNNAMED :-",
            "COMMA ,",
            "UNNAMED :+",
            "COMMA ,",
            "UNNAMED :++",
        ]
    );
    assert_eq!(tokens(":<< :>"), ["UNNAMED :<<", "UNNAMED :>"]);
    assert_eq!(
        tokens("@: @-- @"),
        ["UNNAMED @", "COLON :", "UNNAMED @--", "UNNAMED @"]
    );
    assert_eq!(tokens(":+-"), ["UNNAMED :+", "MINUS -"]);
}

#[test]
fn the_scope_operator_and_assignment() {
    // ca65 emits 03 04 for `.byte s::v,::v`.
    assert_eq!(
        tokens("s::v,::v"),
        [
            "IDENT s",
            "COLON_COLON ::",
            "IDENT v",
            "COMMA ,",
            "COLON_COLON ::",
            "IDENT v"
        ]
    );
    assert_eq!(tokens("v:=7"), ["IDENT v", "COLON_EQ :=", "NUMBER 7"]);
}

// ---- Comments and line ends -------------------------------------------------

#[test]
fn comments_after_a_semicolon() {
    // ca65 emits 01.
    assert_eq!(
        tokens("; whole line\n.byte 1 ; trailing\n;"),
        [
            "COMMENT ; whole line",
            "NEWLINE \n",
            "CONTROL .byte",
            "NUMBER 1",
            "COMMENT ; trailing",
            "NEWLINE \n",
            "COMMENT ;",
        ]
    );
}

#[test]
fn c_comments_and_double_slashes_are_not_comments() {
    // ca65 refuses both.
    assert_eq!(
        tokens("/* w */"),
        ["SLASH /", "STAR *", "IDENT w", "STAR *", "SLASH /"]
    );
    assert_eq!(tokens("// w"), ["SLASH /", "SLASH /", "IDENT w"]);
}

#[test]
fn three_line_endings() {
    // ca65 emits 01 02 for CRLF. Its line reader also ends a line at a lone
    // CR, measured with `.byte 1\r,2`, which it refuses on the second line.
    assert_eq!(
        tokens("1\r\n2\r,3\n"),
        [
            "NUMBER 1",
            "NEWLINE \r\n",
            "NUMBER 2",
            "NEWLINE \r",
            "COMMA ,",
            "NUMBER 3",
            "NEWLINE \n",
        ]
    );
}

#[test]
fn a_backslash_does_not_continue_a_line() {
    // ca65: "Invalid input character: 0x5C".
    assert_eq!(
        tokens("1, \\\n"),
        ["NUMBER 1", "COMMA ,", "ERROR \\", "NEWLINE \n"]
    );
    assert_eq!(problems("\\"), ["0..1 '\\\\' is not allowed here"]);
}

#[test]
fn tabs_and_spaces_separate_tokens() {
    // ca65 emits A9 01 02.
    let lexed = lex("\tlda\t#1 \t;c", MODE);
    let kinds: Vec<_> = lexed.tokens().iter().map(|t| t.kind()).collect();
    assert_eq!(
        kinds,
        [
            SyntaxKind::WHITESPACE,
            kinds::IDENT,
            SyntaxKind::WHITESPACE,
            kinds::HASH,
            kinds::NUMBER,
            SyntaxKind::WHITESPACE,
            SyntaxKind::COMMENT,
        ]
    );
}

#[test]
fn form_feeds_count_as_space_only_at_a_line_end() {
    // Measured: ca65 accepts `.byte 1` followed by a form feed or a vertical
    // tab, because it trims trailing space from each line.
    assert_eq!(tokens("1\x0C\n"), ["NUMBER 1", "NEWLINE \n"]);
    assert_eq!(tokens("1 \x0B"), ["NUMBER 1"]);
    assert!(problems("1 \x0B \t").is_empty());
    // Anywhere else it is not allowed.
    assert_eq!(
        tokens("1\x0C,2"),
        ["NUMBER 1", "ERROR \x0C", "COMMA ,", "NUMBER 2"]
    );
    assert_eq!(
        tokens("1 \x0C,2"),
        ["NUMBER 1", "ERROR \x0C", "COMMA ,", "NUMBER 2"]
    );
}

#[test]
fn characters_outside_ascii_are_not_allowed_outside_strings() {
    assert_eq!(tokens("é"), ["ERROR é"]);
    assert_eq!(problems("é"), ["0..2 'é' is not allowed here"]);
    assert!(problems("\"é\"").is_empty());
}

// ---- Address sizes ----------------------------------------------------------

#[test]
fn an_address_size_needs_its_colon_right_after_it() {
    // ca65 emits A5 12 AD 12 00 A5 12 for `lda z:$12`, `lda a:$12` and
    // `lda $12`, and refuses `lda f:$12` on the 6502 as an addressing mode,
    // after its scanner has read `f:` as a size. Measured too: `A:` works
    // in capitals, and `a: lda #1` is refused because `a:` is a size.
    for size in ["z:", "a:", "f:", "A:", "Z:", "F:"] {
        assert_eq!(
            tokens(&format!("lda {size}$12")),
            ["IDENT lda", &format!("ADDR_SIZE {size}"), "NUMBER $12"]
        );
    }
    // With a space before the colon, `Z` is a label (asm198x#848).
    assert_eq!(
        tokens("Z : x:"),
        ["IDENT Z", "COLON :", "REGISTER x", "COLON :"]
    );
    assert_eq!(tokens("b:"), ["IDENT b", "COLON :"]);
}

// ---- Operators ----------------------------------------------------------------

#[test]
fn symbol_operators() {
    // ca65 emits 02 07 05 FF 01 01 01 01 01 03 03.
    assert_eq!(
        tokens("6&3|1^3~!&&||<> < >= >=<<>>/"),
        [
            "NUMBER 6",
            "AMP &",
            "NUMBER 3",
            "PIPE |",
            "NUMBER 1",
            "CARET ^",
            "NUMBER 3",
            "TILDE ~",
            "BANG !",
            "AMP_AMP &&",
            "PIPE_PIPE ||",
            "NE <>",
            "LT <",
            "GE >=",
            "GE >=",
            "SHL <<",
            "SHR >>",
            "SLASH /",
        ]
    );
    assert_eq!(
        tokens("<= < > = -"),
        ["LE <=", "LT <", "GT >", "EQ =", "MINUS -"]
    );
    assert_eq!(
        tokens("#()[]{}"),
        [
            "HASH #",
            "L_PAREN (",
            "R_PAREN )",
            "L_BRACK [",
            "R_BRACK ]",
            "L_CURLY {",
            "R_CURLY }"
        ]
    );
}

#[test]
fn word_operators_are_control_commands() {
    // ca65 emits 01 01 00 01 04 04 02 07 05 FF.
    for word in [
        ".and", ".or", ".xor", ".not", ".shl", ".shr", ".mod", ".bitand", ".bitnot",
    ] {
        assert_eq!(tokens(word), [format!("CONTROL {word}")]);
        assert!(problems(word).is_empty(), "{word}");
    }
}

#[test]
fn byte_selection_prefixes() {
    // ca65 emits 34 12 12.
    assert_eq!(
        tokens("<$1234,>$1234,^$123456"),
        [
            "LT <",
            "NUMBER $1234",
            "COMMA ,",
            "GT >",
            "NUMBER $1234",
            "COMMA ,",
            "CARET ^",
            "NUMBER $123456",
        ]
    );
}

// ---- The on-demand lexer ----------------------------------------------------

#[test]
fn the_parser_can_ask_for_one_token_at_a_time() {
    let mut lexer = Lexer::new("x .bogus");
    let first = lexer.next_token(MODE);
    assert_eq!(first.map(Token::kind), Some(kinds::REGISTER));
    assert!(lexer.diagnostics().is_empty());
    let kinds: Vec<_> = std::iter::from_fn(|| lexer.next_token(MODE))
        .map(Token::kind)
        .collect();
    assert_eq!(kinds, [SyntaxKind::WHITESPACE, kinds::CONTROL]);
    assert_eq!(lexer.diagnostics().len(), 1);
    assert_eq!(lexer.next_token(MODE), None);
}

#[test]
fn empty_text_has_no_tokens() {
    assert!(lex("", MODE).tokens().is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Any text lexes into non-empty tokens that cover it exactly, in order,
    /// on character boundaries, with every diagnostic inside the text.
    #[test]
    fn any_text_lexes_into_tokens_that_cover_it(
        text in "\\PC{0,40}|[ -~\\t\\r\\n\\x0B\\x0C]{0,60}"
    ) {
        let lexed = lex(&text, MODE);
        let mut pos = 0;
        for token in lexed.tokens() {
            let len = token.len().to_usize();
            prop_assert!(len > 0, "an empty token at {}", pos);
            prop_assert!(text.get(pos..pos + len).is_some(), "token at {} splits a character", pos);
            pos += len;
        }
        prop_assert_eq!(pos, text.len());
        for diagnostic in lexed.diagnostics() {
            prop_assert!(diagnostic.range().slice(&text).is_some());
        }
    }
}
