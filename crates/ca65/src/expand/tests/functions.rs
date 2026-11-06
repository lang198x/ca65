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
        Bytes(b"007|   03|   ab|x|-0012|4    "),
    ),
    // Functions run where their text is used, not where it is written.
    (".define D .left(1, {7 8})\n.byte D\n", Bytes(&[7])),
    (
        ".macro m p\n.byte .left(1, {p})\n.endmacro\nm 9 8\n",
        Bytes(&[9]),
    ),
    // Not in a skipped branch.
    (".if 0\n.byte .left(\n.endif\n", Bytes(&[])),
    // A `.elseif` runs its token functions before its condition.
    (
        ".if 0\n.byte 1\n.elseif .match(.left(1, {a b}), a)\n.byte 2\n.endif\n",
        Bytes(&[2]),
    ),
    (
        ".if 0\n.byte 1\n.elseif .left(1, {0 1})\n.byte 2\n.else\n.byte 3\n.endif\n",
        Bytes(&[3]),
    ),
    // One after a branch that was taken is not run, and its syntax does
    // not count.
    (
        ".if 1\n.byte 1\n.elseif .left(\n.byte 2\n.endif\n",
        Bytes(&[1]),
    ),
    (
        ".if 1\n.byte 1\n.elseif 1 2 3\n.byte 2\n.endif\n",
        Bytes(&[1]),
    ),
    (".if 1\n.byte 1\n.elseif\n.byte 2\n.endif\n", Bytes(&[1])),
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
    // Functions whose value depends only on their tokens.
    (
        ".byte .strlen(\"abc\"),.strat(\"abc\",1)\n",
        Bytes(&[3, 0x62]),
    ),
    (
        ".byte .strlen(.concat(\"a\",\"bc\")),.strlen(\"\"),.strat(\"a\",0)\n",
        Bytes(&[3, 0, 0x61]),
    ),
    (
        ".byte .strlen(.sprintf(\"%c\",255)),.strat(.sprintf(\"%c\",255),0)\n",
        Bytes(&[1, 0xFF]),
    ),
    ("i=1\n.byte .strat(\"abc\",i)\n", Bytes(&[0x62])),
    (".byte .strlen(1)\n", Refused("expected a string")),
    (".byte .strat(\"abc\",3)\n", Refused("outside the string")),
    // ca65 V2.19 fails an internal check here.
    (".byte .strat(\"abc\",-1)\n", Refused("outside the string")),
    (
        ".byte .tcount(a b c),.tcount({a,b}),.tcount()\n",
        Bytes(&[3, 3, 0]),
    ),
    (".byte .tcount({{1})\n", Bytes(&[2])),
    (".byte .tcount({{1}})\n", Refused("")),
    (".byte .tcount((1))\n", Refused("")),
    (".byte .tcount(a,b)\n", Bytes(&[3])),
    (".byte .tcount({a)b})\n", Bytes(&[3])),
    (".byte .tcount(.paramcount)\n", Bytes(&[1])),
    (
        ".define d 1 2\n.byte .tcount(d),.tcount({d})\n",
        Bytes(&[2, 2]),
    ),
    (".byte .blank(),.blank({}),.blank(a)\n", Bytes(&[1, 1, 0])),
    (".byte .blank({ }),.blank(,)\n", Bytes(&[1, 0])),
    // `a` and `x` are registers, each a token of its own.
    (
        ".byte .match(a,b),.match(1,2),.xmatch(1,1),.xmatch(1,2),.match({a b},{c d})\n",
        Bytes(&[0, 1, 1, 0, 0]),
    ),
    (
        ".byte .match(lda, ldx),.xmatch(lda,ldx),.match(\"a\",\"b\"),.xmatch(\"a\",\"a\"),.match(1,)\n",
        Bytes(&[1, 0, 1, 1, 0]),
    ),
    (
        ".byte .match({1+2},{3+4}),.match({1+2},{3-4}),.xmatch({a},{A})\n",
        Bytes(&[1, 0, 1]),
    ),
    (
        ".byte .xmatch($10,16),.xmatch(%1,1),.xmatch(1,1.0)\n",
        Bytes(&[1, 1, 0]),
    ),
    (
        ".byte .xmatch('a',97),.xmatch('a','a'),.match('a',1),.xmatch(ab,AB),.xmatch(@c,@c),.match(@c,c)\n",
        Bytes(&[0, 1, 0, 0, 1, 0]),
    ),
    (
        ".byte .xmatch(.byte,.BYTE),.xmatch(.byt,.byte),.match(.byte,.word),.xmatch(+,+),.match(+,-)\n",
        Bytes(&[1, 1, 0, 1, 0]),
    ),
    (
        ".byte .xmatch(x,X),.match(a,x),.xmatch(\"a\",\"A\")\n",
        Bytes(&[1, 0, 0]),
    ),
    (
        ".byte .xmatch(.and,&&),.match(.and,&&),.xmatch(.bitand,&)\n",
        Bytes(&[1, 1, 1]),
    ),
    (".byte .match(:+,:-),.xmatch(:+,:-)\n", Bytes(&[1, 1])),
    (".byte .match(1 2)\n", Refused("")),
    (
        ".byte .definedmacro(m)\n.macro m\n.endmacro\n.byte .definedmacro(m)\n",
        Bytes(&[0, 1]),
    ),
    (
        ".macro m p,q\n.byte .definedmacro(m),.definedmacro(n)\n.endmacro\nm\n",
        Bytes(&[1, 0]),
    ),
    (".macro m\n.endmacro\n.byte .definedmacro(M)\n", Bytes(&[0])),
    (".byte .definedmacro(lda)\n", Bytes(&[0])),
    (
        ".byte .definedmacro(.ident(\"m\"))\n.macro m\n.endmacro\n.byte .definedmacro(.ident(\"m\"))\n",
        Bytes(&[0, 1]),
    ),
    (
        ".define d 1\n.byte .definedmacro(d)\n",
        Refused("expected a name"),
    ),
    (
        ".byte .ismnemonic(lda),.ismnem(foo),.ismnemonic(LDA),.ismnemonic(bra)\n",
        Bytes(&[1, 0, 1, 0]),
    ),
    (
        ".byte .ismnemonic(nop),.ismnemonic(bit),.ismnemonic(jmp),.ismnemonic(stz),.ismnemonic(inc)\n",
        Bytes(&[1, 1, 1, 0, 1]),
    ),
    (".byte .ismnemonic(.ident(\"nop\"))\n", Bytes(&[1])),
    (".byte .ismnemonic(1)\n", Refused("expected a name")),
    (".byte .ismnemonic(.byte)\n", Refused("expected a name")),
    // In a token list these functions are tokens, run only if the list's
    // result is read again.
    (".byte .tcount({.match({1},{2})})\n", Refused("")),
    (".byte .tcount(.strlen(\"ab\"))\n", Refused("")),
    (".byte .left(.strlen(\"a\"),{7 8})\n", Bytes(&[7])),
    (
        ".byte .left(1,{.left(.strlen(\"a\"),{7 8})})\n",
        Bytes(&[7]),
    ),
    (".byte .left(1,{.strlen(\"abc\")})\n", Refused("")),
    (".byte .tcount({.concat(\"a\",\"b\")})\n", Bytes(&[1])),
    (
        ".if .xmatch({.tcount(a)},{.tcount(a)})\n.byte 1\n.endif\n",
        Bytes(&[1]),
    ),
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
fn sprintf_formats_as_ca65_does_where_c_would_not() {
    // ca65 V2.19 formats with cc65's own `xvsnprintf`, not C's `printf`:
    // `%x` is signed, `+` marks unsigned conversions, `#` shows `0x` for 0,
    // and `%s` and `%c` pad on the right unless `-` is given. Each line is
    // one of Asm198x's differential probes, with ca65 V2.19's bytes.
    for (source, want) in [
        (
            ".byte .sprintf(\"%d|%i|%u|%c|%s\", 5, -5, -5, 65, \"ab\")\n\
             .byte .sprintf(\"%x|%X|%o|100%%\", -255, -255, -9)\n",
            &b"5|-5|18446744073709551611|A|ab-ff|FFFFFFFFFFFFFF01|1777777777777777777767|100%"[..],
        ),
        (
            ".byte .sprintf(\"%+d|% d|%6d|%-6d|%06d|%+06d\", 5, 5, -5, -5, -5, 0)\n\
             .byte .sprintf(\"%#x|%#X|%#o|%#x|%#o\", 255, 255, 9, 0, 0)\n\
             .byte .sprintf(\"%#08x|%+#08x|%#08X|%+#08X\", -255, 255, -255, 255)\n",
            b"+5| 5|    -5|-5    |-00005|+000000xff|0XFF|011|0x0|0-0x000ff|+0x000ff|0XFFFFFFFFFFFFFF01|+0X000FF",
        ),
        (
            ".byte .sprintf(\"[%6s][%-6s][%06s][%.2s]\", \"ab\", \"ab\", \"ab\", \"abcd\")\n\
             .byte .sprintf(\"[%6c][%-6c]\", 65, 65)\n",
            b"[ab    ][    ab][ab    ][ab][A     ][     A]",
        ),
        (
            ".byte .sprintf(\"%.3d|%.4x|%.4X|%.4o|%.4u\", -5, -255, -255, -9, 5)\n\
             .byte .sprintf(\"[%.0d][%.0x][%08.3d][%#.4o]\", 0, 0, 5, 9)\n",
            b"-005|-00ff|FFFFFFFFFFFFFF01|1777777777777777777767|0005[][][     005][0011]",
        ),
        (".byte .sprintf(\"%x\", -1)\n", b"-1"),
        (".byte .sprintf(\"%#X\", 0)\n", b"0X0"),
        (".byte .sprintf(\"%3c\", 66)\n", b"B  "),
        (".byte .sprintf(\"%5s\", \"ab\")\n", b"ab   "),
        (".byte .sprintf(\"%u\", -1)\n", b"18446744073709551615"),
        (".byte .sprintf(\"[%.0c][% +d]\", 66, 5)\n", b"[][+5]"),
    ] {
        assert_eq!(run(source).as_deref(), Ok(want), "{source:?}");
    }
}

/// `.sprintf` of one value, one row per case read off ca65 V2.18 for
/// Asm198x's #357. cc65 has not changed how `xvsnprintf` formats these
/// conversions since, so V2.19 gives the same.
#[rustfmt::skip]
const SPRINTF_MEASURED: &[(&str, &str, &str)] = &[
    ("%d", "5", "5"),
    ("%d", "-5", "-5"),
    ("%d", "0", "0"),
    ("%i", "5", "5"),
    ("%i", "-5", "-5"),
    ("%u", "5", "5"),
    ("%u", "-5", "18446744073709551611"),
    ("%x", "255", "ff"),
    ("%x", "-255", "-ff"),
    ("%x", "-1", "-1"),
    ("%X", "255", "FF"),
    ("%X", "-255", "FFFFFFFFFFFFFF01"),
    ("%X", "-1", "FFFFFFFFFFFFFFFF"),
    ("%o", "9", "11"),
    ("%o", "-9", "1777777777777777777767"),
    ("%c", "65", "A"),
    ("%s", "\"ab\"", "ab"),
    ("%+d", "5", "+5"),
    ("%+d", "-5", "-5"),
    ("%+d", "0", "+0"),
    ("% d", "5", " 5"),
    ("% d", "-5", "-5"),
    ("% d", "0", " 0"),
    ("%#x", "255", "0xff"),
    ("%#x", "-255", "-0xff"),
    ("%#x", "-1", "-0x1"),
    ("%#X", "255", "0XFF"),
    ("%#X", "-255", "0XFFFFFFFFFFFFFF01"),
    ("%#X", "-1", "0XFFFFFFFFFFFFFFFF"),
    ("%#o", "9", "011"),
    ("%#o", "-9", "01777777777777777777767"),
    ("%#d", "5", "5"),
    ("%#d", "-5", "-5"),
    ("%#d", "0", "0"),
    ("%#s", "\"ab\"", "ab"),
    ("%#c", "65", "A"),
    ("%+u", "5", "+5"),
    ("%+u", "-5", "+18446744073709551611"),
    ("%+x", "255", "+ff"),
    ("%+x", "-255", "-ff"),
    ("%+x", "-1", "-1"),
    ("%+X", "255", "+FF"),
    ("%+X", "-255", "+FFFFFFFFFFFFFF01"),
    ("%+X", "-1", "+FFFFFFFFFFFFFFFF"),
    ("%+o", "9", "+11"),
    ("%+o", "-9", "+1777777777777777777767"),
    ("%+c", "65", "A"),
    ("%+s", "\"ab\"", "ab"),
    ("% u", "5", " 5"),
    ("% u", "-5", " 18446744073709551611"),
    ("% x", "255", " ff"),
    ("% x", "-255", "-ff"),
    ("% x", "-1", "-1"),
    ("% X", "255", " FF"),
    ("% X", "-255", " FFFFFFFFFFFFFF01"),
    ("% X", "-1", " FFFFFFFFFFFFFFFF"),
    ("% o", "9", " 11"),
    ("% o", "-9", " 1777777777777777777767"),
    ("% s", "\"ab\"", "ab"),
    ("% c", "65", "A"),
    ("%6d", "5", "     5"),
    ("%6d", "-5", "    -5"),
    ("%6d", "0", "     0"),
    ("%-6d", "5", "5     "),
    ("%-6d", "-5", "-5    "),
    ("%-6d", "0", "0     "),
    ("%06d", "5", "000005"),
    ("%06d", "-5", "-00005"),
    ("%06d", "0", "000000"),
    ("%-06d", "5", "5     "),
    ("%-06d", "-5", "-5    "),
    ("%-06d", "0", "0     "),
    ("%+06d", "5", "+00005"),
    ("%+06d", "-5", "-00005"),
    ("%+06d", "0", "+00000"),
    ("%6x", "255", "    ff"),
    ("%6x", "-255", "   -ff"),
    ("%6x", "-1", "    -1"),
    ("%-6x", "255", "ff    "),
    ("%-6x", "-255", "-ff   "),
    ("%-6x", "-1", "-1    "),
    ("%06x", "255", "0000ff"),
    ("%06x", "-255", "-000ff"),
    ("%06x", "-1", "-00001"),
    ("%#08x", "255", "0x0000ff"),
    ("%#08x", "-255", "-0x000ff"),
    ("%#08x", "-1", "-0x00001"),
    ("%+#08x", "255", "+0x000ff"),
    ("%+#08x", "-255", "-0x000ff"),
    ("%+#08x", "-1", "-0x00001"),
    ("%#08X", "255", "0X0000FF"),
    ("%#08X", "-255", "0XFFFFFFFFFFFFFF01"),
    ("%#08X", "-1", "0XFFFFFFFFFFFFFFFF"),
    ("%+#08X", "255", "+0X000FF"),
    ("%+#08X", "-255", "+0XFFFFFFFFFFFFFF01"),
    ("%+#08X", "-1", "+0XFFFFFFFFFFFFFFFF"),
    ("%6X", "255", "    FF"),
    ("%6X", "-255", "FFFFFFFFFFFFFF01"),
    ("%6X", "-1", "FFFFFFFFFFFFFFFF"),
    ("%06X", "255", "0000FF"),
    ("%06X", "-255", "FFFFFFFFFFFFFF01"),
    ("%06X", "-1", "FFFFFFFFFFFFFFFF"),
    ("%6o", "9", "    11"),
    ("%6o", "-9", "1777777777777777777767"),
    ("%06o", "9", "000011"),
    ("%06o", "-9", "1777777777777777777767"),
    ("%6u", "5", "     5"),
    ("%6u", "-5", "18446744073709551611"),
    ("%06u", "5", "000005"),
    ("%06u", "-5", "18446744073709551611"),
    ("%6s", "\"ab\"", "ab    "),
    ("%-6s", "\"ab\"", "    ab"),
    ("%06s", "\"ab\"", "ab    "),
    ("%.2s", "\"ab\"", "ab"),
    ("%.6s", "\"ab\"", "ab"),
    ("%6c", "65", "A     "),
    ("%-6c", "65", "     A"),
    ("%06c", "65", "A     "),
    ("%.1c", "65", "A"),
    ("%.3d", "5", "005"),
    ("%.3d", "-5", "-005"),
    ("%.3d", "0", "000"),
    ("%.4x", "255", "00ff"),
    ("%.4x", "-255", "-00ff"),
    ("%.4x", "-1", "-0001"),
    ("%.4X", "255", "00FF"),
    ("%.4X", "-255", "FFFFFFFFFFFFFF01"),
    ("%.4X", "-1", "FFFFFFFFFFFFFFFF"),
    ("%.4o", "9", "0011"),
    ("%.4o", "-9", "1777777777777777777767"),
    ("%.4u", "5", "0005"),
    ("%.4u", "-5", "18446744073709551611"),
    ("%.0d", "5", "5"),
    ("%.0d", "-5", "-5"),
    ("%.0d", "0", ""),
];

#[test]
fn sprintf_gives_what_ca65_gave_for_each_measured_case() {
    for (spec, value, want) in SPRINTF_MEASURED {
        let source = format!(".byte .sprintf(\"{spec}\", {value})\n");
        assert_eq!(
            run(&source).as_deref(),
            Ok(want.as_bytes()),
            "`{spec}` of `{value}`"
        );
    }
}

#[test]
fn mid_treats_a_start_above_100_as_0() {
    // ca65 V2.19's `FuncMid` corrects a start below 0 or above 100 to 0,
    // silently. Its manual does not say so; ca65 gives 07 for the first.
    assert_eq!(run(".byte .mid(200, 1, {7})\n").as_deref(), Ok(&[7][..]));
    assert_eq!(run(".byte .mid(101, 1, {7})\n").as_deref(), Ok(&[7][..]));
    assert_eq!(run(".byte .mid(-1, 1, {7})\n").as_deref(), Ok(&[7][..]));
    // 100 is kept, so nothing is left of a short list.
    assert!(run(".byte .mid(100, 1, {7})\n").is_err());
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
    // ca65 V2.19 pads `%c` on the right, against C.
    (".byte .sprintf(\"%3c\",255)\n", &[0xFF, 0x20, 0x20]),
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
