//! Token functions, each case measured on ca65 V2.19 at `8d86ca7`.

use super::{Bytes, Refused, Want, run};

const MEASURED: &[(&str, Want)] = &[
    (".byte .concat(\"ab\",\"c\")\n", Bytes(b"abc")),
    (".byte .concat(\"ab\")\n", Bytes(b"ab")),
    (".byte .concat (\"a\",\"b\")\n", Bytes(b"ab")),
    (".byte .concat(.string(ab), \"c\")\n", Bytes(b"abc")),
    (
        ".ident(.concat(\"la\",\"b\")): nop\n.word lab\n",
        Bytes(&[0xEA, 0, 0]),
    ),
    (".ident(\"@x\"):\n", Bytes(&[])),
    (".ident(\"z\"):\n.word z\n", Bytes(&[0, 0])),
    ("q=5\n.byte .ident(\"q\")\n", Bytes(&[5])),
    // `.ident` makes a name, even one spelt like a register.
    (".ident(\"A\")=7\n.byte .ident(\"A\")\n", Bytes(&[7])),
    (".ident(\"A\"): .byte 7\n", Bytes(&[7])),
    (
        ".repeat 1,.ident(\"A\")\n.byte 7\n.endrepeat\n",
        Bytes(&[7]),
    ),
    (
        "q1=3\n.byte .ident(.sprintf(\"%s%d\", \"q\", 1))\n",
        Bytes(&[3]),
    ),
    (
        ".repeat 2, i\n.ident(.sprintf(\"l%d\", i)):\n.endrepeat\n.word l1\n",
        Bytes(&[0, 0]),
    ),
    // A token list runs to the first `)`, or is in braces.
    (".byte .left(1, 7 , 8)\n", Bytes(&[7])),
    (".byte .left(3, (1))\n", Bytes(&[1])),
    (".byte .left(1, {1, 2})\n", Bytes(&[1])),
    (".byte .left(1, {1}) , 2\n", Bytes(&[1, 2])),
    (".byte .left(0, {1}) 5\n", Bytes(&[5])),
    (".byte .left(-1, {1}) 5\n", Bytes(&[5])),
    (".byte .left(9, {1})\n", Bytes(&[1])),
    ("n=1\n.byte .left(n, {4 5})\n", Bytes(&[4])),
    (".byte .right(1, {1 2 3})\n", Bytes(&[3])),
    (".byte .right(-1, {1}) 4\n", Bytes(&[4])),
    (".byte .mid(1, 1, {1 2 3})\n", Bytes(&[2])),
    (".byte .mid(1, -2, {1 2}) 4\n", Bytes(&[4])),
    (".byte .mid(-1, 1, {5 2})\n", Bytes(&[5])),
    // A function's result is read again, so functions nest.
    (".byte .left(1, {.left(1, {3 4})})\n", Bytes(&[3])),
    (".byte .left(1, .left(1, {3 4}))\n", Bytes(&[3])),
    (".byte .string(.left(1, {qq rr}))\n", Bytes(b"qq")),
    (".byte .string(foo)\n", Bytes(b"foo")),
    (".byte .string(::b)\n", Bytes(b"::b")),
    (".byte .string(@l)\n", Bytes(b"@l")),
    (".byte .string(1+2)\n", Bytes(b"3")),
    (
        ".byte .sprintf(\"%d-%x-%02X|%s|%c|%%\", 10, 255, 10, \"s\", 65)\n",
        Bytes(b"10-ff-0A|s|A|%"),
    ),
    (".byte .sprintf(\"%5d\", 1)\n", Bytes(b"    1")),
    (".byte .sprintf(\"%-3d|\", 1)\n", Bytes(b"1  |")),
    (
        ".byte .sprintf(\"%o %u %i %#x %+d % d %#o\", 8, 3, 4, 255, 5, 6, 8)\n",
        Bytes(b"10 3 4 0xff +5  6 010"),
    ),
    (
        ".byte .sprintf(\"%.3d|%5.2d|%-5s|%.1s|%05d|%-05d\", 7, 3, \"ab\", \"xy\", -12, 4)\n",
        Bytes(b"007|   03|ab   |x|-0012|4    "),
    ),
    // Functions run where their text is used, not where it is written.
    (".define D .left(1, {7 8})\n.byte D\n", Bytes(&[7])),
    (
        ".macro m p\n.byte .left(1, {p})\n.endmacro\nm 9 8\n",
        Bytes(&[9]),
    ),
    // Not in a skipped branch.
    (".if 0\n.byte .left(\n.endif\n", Bytes(&[])),
    (".byte .concat()\n", Refused("expected a string")),
    (".byte .concat(\"a\", 1)\n", Refused("expected a string")),
    (".byte .ident()\n", Refused("expected a string")),
    (".byte .ident(\"q\"\n", Refused("expected `)`")),
    (".ident(\"1x\"):\n", Refused("not a valid name")),
    (".ident(\"a b\"):\n", Refused("not a valid name")),
    (".byte .left 1, {1}\n", Refused("expected `(`")),
    (".byte .left(1, {1}\n", Refused("expected `)`")),
    (".byte .left(1, {1} 2)\n", Refused("expected `)`")),
    (
        ".byte .left(n, {4 5})\nn=1\n",
        Refused("`n` is not defined"),
    ),
    (".byte .string()\n", Refused("expected a value")),
    (".byte .sprintf(\"%c\", 0)\n", Refused("1 to 255")),
    (".byte .sprintf(\"%s\", 1)\n", Refused("expected a string")),
    (".byte .sprintf(\"%q\", 1)\n", Refused("is not a format")),
    (".byte .sprintf(\"%d\")\n", Refused("expected `,`")),
    (".byte .sprintf(\"%\")\n", Refused("ends partway")),
    (".byte .sprintf(\"%5\", 1)\n", Refused("ends partway")),
    (".byte .sprintf(\"ab\", 1)\n", Refused("expected `)`")),
];

#[test]
fn every_measured_token_function_matches_ca65() {
    for (source, want) in MEASURED {
        let got = run(source);
        match want {
            Bytes(bytes) => assert_eq!(got.as_deref(), Ok(*bytes), "{source:?}"),
            Refused(part) => {
                let problems = got.expect_err(source);
                assert!(
                    problems.iter().any(|p| p.contains(part)),
                    "{source:?}: {problems:?}"
                );
            }
        }
    }
}

#[test]
fn measured_results_that_ca65_refuses_once_read_again() {
    for source in [
        ".byte .left(2, {1 2 3})\n",
        ".byte .left(1+1, {4 5 6})\n",
        ".byte .left(1, {(1)})\n",
        ".byte .mid(1, 5, {1 2 3})\n",
        ".byte .string(\"x\")\n",
    ] {
        assert!(run(source).is_err(), "{source:?}");
    }
}

#[test]
fn where_ca65_strays_from_printf_c_is_followed() {
    // ca65's manual calls the format "similar to the one of the C printf
    // function". ca65 V2.19 gives `-1`, `0X0`, `B  ` and `ab   ` here,
    // from bugs in its own formatter; C's results are given instead.
    for (source, want) in [
        (".byte .sprintf(\"%x\", -1)\n", &b"ffffffffffffffff"[..]),
        (".byte .sprintf(\"%#X\", 0)\n", b"0"),
        (".byte .sprintf(\"%3c\", 66)\n", b"  B"),
        (".byte .sprintf(\"%5s\", \"ab\")\n", b"   ab"),
        (".byte .sprintf(\"%u\", -1)\n", b"18446744073709551615"),
    ] {
        assert_eq!(run(source).as_deref(), Ok(want), "{source:?}");
    }
}

#[test]
fn mid_takes_its_start_as_given() {
    // ca65 V2.19 gives 07: it treats a start above 100 as 0, which its
    // manual does not say. The start is taken as given, so nothing is left.
    assert!(run(".byte .mid(200, 1, {7})\n").is_err());
    assert_eq!(run(".byte .mid(200, 1, {7}) 5\n").as_deref(), Ok(&[5][..]));
}

/// Strings a token function makes that ca65's syntax cannot write: a
/// quote, a line break, or a byte above 127. Each is measured on ca65 V2.19.
const UNWRITABLE: &[(&str, &[u8])] = &[
    (".byte .sprintf(\"%c\",255)\n", &[0xFF]),
    (".byte .sprintf(\"%c\",34)\n", &[0x22]),
    (".byte .sprintf(\"%c%c\",34,34)\n", &[0x22, 0x22]),
    (".byte .concat(.sprintf(\"%c\",34),\"a\")\n", &[0x22, 0x61]),
    (".byte .sprintf(\"%c\",10)\n", &[0x0A]),
    (".byte .sprintf(\"%c\",13)\n", &[0x0D]),
    (".byte .sprintf(\"%s\",.sprintf(\"%c\",255))\n", &[0xFF]),
    (
        ".byte .string(5), .concat(\"a\",.sprintf(\"%c\",128))\n",
        &[0x35, 0x61, 0x80],
    ),
    (".byte .left(1,{.sprintf(\"%c\",255)})\n", &[0xFF]),
    (
        ".repeat 2,i\n.byte .sprintf(\"%c\",200+i)\n.endrepeat\n",
        &[0xC8, 0xC9],
    ),
    (".define D .sprintf(\"%c\",255)\n.byte D\n", &[0xFF]),
    (
        ".macro emit value\n.byte value\n.endmacro\nemit .sprintf(\"%c\",255)\n",
        &[0xFF],
    ),
    (
        ".macro emit value\n.byte value,2\n.endmacro\nemit {.sprintf(\"%c\",255)}\n",
        &[0xFF, 2],
    ),
    // ca65 V2.19 pads `%c` on the right, against C; C's result is given.
    (".byte .sprintf(\"%3c\",255)\n", &[0x20, 0x20, 0xFF]),
];

#[test]
fn strings_this_syntax_cannot_write_keep_their_bytes() {
    for (source, bytes) in UNWRITABLE {
        assert_eq!(run(source).as_deref(), Ok(*bytes), "{source:?}");
    }
    // ca65 refuses a name made of a byte above 127.
    let problems = run(".ident(.sprintf(\"%c\",255)):\n").expect_err("refused");
    assert!(
        problems.iter().any(|p| p.contains("not a valid name")),
        "{problems:?}"
    );
}

#[test]
fn a_made_string_shows_its_bytes() {
    use crate::ast::View;
    // A backslash is an ordinary character in ca65's strings, so the
    // format makes $FF, a backslash and a quote.
    let (_, lines) = super::active_lines(".byte .sprintf(\"%c\\%c\", 255, 34)\n");
    let line = &lines[0];
    let made = line
        .line()
        .syntax()
        .descendants()
        .filter_map(lang198x_syntax::SyntaxElement::into_token)
        .find(|t| t.kind() == crate::kinds::STRING)
        .expect("a string");
    assert_eq!(made.text(), "\"\\xFF\\\\\"\"");
    assert_eq!(line.string(&made), Some(vec![0xFF, b'\\', b'"']));
    // A string of plain bytes is shown as it is, with nothing kept beside.
    let (_, lines) = super::active_lines(".byte .concat(\"a\\\", \"b\")\n");
    let plain = super::token(&lines[0], "\"a\\b\"");
    assert_eq!(lines[0].string(&plain), Some(b"a\\b".to_vec()));
}

#[test]
fn a_functions_result_says_what_it_was_made_from() {
    use lang198x_syntax::Span;
    let source = ".byte .concat(\"a\", \"b\")\n";
    let (expander, lines) = super::active_lines(source);
    let line = &lines[0];
    let made = super::token(line, "\"ab\"");
    let lang198x_syntax::TextId::Expansion(id) = line.text() else {
        panic!("an expansion")
    };
    let origin = expander
        .provenance()
        .get(id)
        .and_then(|e| e.origin_at(made.text_range().start()))
        .expect("an origin");
    assert_eq!(
        origin.made_from().and_then(|s| s.range().slice(source)),
        Some(".concat")
    );
    let byte = super::token(line, ".byte");
    let traced = expander
        .provenance()
        .trace(Span::new(line.text(), byte.text_range()))
        .expect("copied");
    assert_eq!(traced.range().start().get(), 0);
}
