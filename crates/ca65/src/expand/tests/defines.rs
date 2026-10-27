//! Define-style macros, each case measured on ca65 V2.19 at `8d86ca7`.

use super::{Bytes, Refused, Want, run};

const MEASURED: &[(&str, Want)] = &[
    (".define N 5\n.byte N\n", Bytes(&[5])),
    (".define N 2+3\n.byte N*2\n", Bytes(&[8])),
    (".define N 7 ; c\n.byte N\n", Bytes(&[7])),
    (".define F(p) p+1\n.byte F(2)\n", Bytes(&[3])),
    (".define F(p,q) p*q\n.byte F(2,3)\n", Bytes(&[6])),
    (".define F(p)p\n.byte F(1)\n", Bytes(&[1])),
    // Parentheses are only tokens, so they may be left out, or spaced.
    (".define F(p) p+1\n.byte F 2\n", Bytes(&[3])),
    (".define F(p) p\n.byte F (1)\n", Bytes(&[1])),
    (".define F(p) .byte p\nF 1\n", Bytes(&[1])),
    (".define F(p) .byte p\nF(1)\n", Bytes(&[1])),
    // An argument runs to a comma or the end of the line, unless braced.
    (".define F(p) p\n.byte F 1, 2\n", Bytes(&[1, 2])),
    (".define F(p) p\n.byte F 1 ; c\n", Bytes(&[1])),
    (".define F(p) p\n.byte F {1}, 2\n", Bytes(&[1, 2])),
    (".define F(p) .byte p\nF {1,2}\n", Bytes(&[1, 2])),
    (".define F(p,q) .byte p,q\nF {1,2}, 3\n", Bytes(&[1, 2, 3])),
    (".define F(p) p\n.byte F(F(3))\n", Bytes(&[3])),
    // G's body is `(q)+1*2`: F's argument ran to the end of G's line.
    (
        ".define F(p) p*2\n.define G(q) F(q)+1\n.byte G(3)\n",
        Bytes(&[5]),
    ),
    // Parameter names are case-sensitive.
    (".define F(P) p\np=4\n.byte F(1)\n", Bytes(&[4])),
    (".define E\n.byte 1 E\n", Bytes(&[1])),
    (".define B .byte\nB 3\n", Bytes(&[3])),
    (".define L lab:\nL nop\n.word lab\n", Bytes(&[0xEA, 0, 0])),
    (".define D .define\nD X2 4\n.byte X2\n", Bytes(&[4])),
    (".define N 5\n.undef N\nN=6\n.byte N\n", Bytes(&[6])),
    // A body uses the defines in force when it is defined.
    (
        ".define N 5\n.define M N+1\n.undef N\n.define N 9\n.byte M\n",
        Bytes(&[6]),
    ),
    (
        ".define N 5\n.macro m\n.byte N\n.endmacro\n.undef N\n.define N 6\nm\n",
        Bytes(&[5]),
    ),
    (
        ".define B .byte\n.macro m\nB 1\n.endmacro\nm\n",
        Bytes(&[1]),
    ),
    (".define N 5\n.ifblank N\n.byte 1\n.endif\n", Bytes(&[])),
    (".define N\n.ifblank N\n.byte 1\n.endif\n", Bytes(&[1])),
    // Uses expand even in a skipped branch, and in bodies being collected.
    (".define E .endif\n.if 0\nE\n.byte 1\n", Bytes(&[1])),
    (
        ".define N 1\n.if 0\n.undef N\n.endif\n.byte N\n",
        Bytes(&[1]),
    ),
    (
        ".define E .endrepeat\n.repeat 2\n.byte 1\nE\n",
        Bytes(&[1, 1]),
    ),
    (
        ".define E .endmacro\n.macro m\n.byte 1\nE\nm\n",
        Bytes(&[1]),
    ),
    (
        ".repeat 2, i\n.define D i\n.byte D\n.undefine D\n.endrepeat\n",
        Bytes(&[0, 1]),
    ),
    (
        ".define N 3\n.repeat N, i\n.byte i\n.endrepeat\n",
        Bytes(&[0, 1, 2]),
    ),
    (".define N 5\n.byte n\n", Refused("`n` is not defined")),
    (
        ".define N 5\n.undefine N\n.byte N\n",
        Refused("`N` is not defined"),
    ),
    (
        ".define F(p) p\n.undef F\n.byte F\n",
        Refused("`F` is not defined"),
    ),
    (
        ".if 0\n.define N 1\n.endif\n.byte N\n",
        Refused("`N` is not defined"),
    ),
    (".define N 5\n.define N 6\n", Refused("already defined")),
    (
        ".macro m\n.endmacro\n.define m 1\n",
        Refused("already defined"),
    ),
    (
        ".repeat 2, i\n.define D i\n.endrepeat\n",
        Refused("already defined"),
    ),
    (".define nop 1\n", Refused("an instruction cannot be")),
    (".define N N+1\n.byte N\n", Refused("uses itself")),
    (
        ".define F(p, p) p\n.byte F(1,2)\n",
        Refused("two parameters"),
    ),
    (".undef Q\n", Refused("no define-style macro")),
    (
        ".macro m\n.endmacro\n.undef m\n",
        Refused("no define-style macro"),
    ),
    (".define F(p) p+1\n.byte F\n", Refused("is empty")),
    (".define F (p) p\n.byte F\n", Refused("is empty")),
    (
        ".define F(p) p\n.if 0\nF\n.endif\n.byte 1\n",
        Refused("is empty"),
    ),
    (
        ".define F(p,q) p*q+0\n.byte F(2)\n",
        Refused("takes 2 arguments"),
    ),
    (
        ".define F(p,q) .byte p,q\nF {1,2} 3\n",
        Refused("takes 2 arguments"),
    ),
    (".define F(p) .byte p\nF {1,2\n", Refused("no closing `}`")),
    (
        ".define N 1\n.repeat 2, N\n.byte N\n.endrepeat\n",
        Refused("plain name"),
    ),
];

#[test]
fn every_measured_define_matches_ca65() {
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
fn measured_defines_that_ca65_refuses_for_the_parser() {
    // ca65 refuses each of these; once expanded, the parser does too.
    for source in [
        ".define\n",
        ".define .x 1\n",
        ".define N 5\n.undef N, M\n",
        ".define N 5\n.byte N .undef N\n",
        ".define N 5\nN = 3\n",
        ".define N 5\n.ifdef N\n.byte 1\n.endif\n",
        ".define m 1\n.macro m\n.endmacro\n",
        ".define N 1\n.delmac N\n",
        ".define F(p,q) p\n.byte F(2,3,4)\n",
        ".define F(p) p\n.byte F({1,2})\n",
        ".define F(p) p\n.byte F((1,2))\n",
        ".define F() 3\n.byte F()\n",
        ".define F(p) p\n.byte F(1\n",
        ".define F(p) p\n.byte F(1)),2\n",
        ".define F {1,2}\n.byte F\n",
        ".define N 5\n.define F(N) N+1\n.byte F(2)\n",
        ".define N 1\n.macro m N\n.byte N\n.endmacro\nm 2\n",
        ".define Z z\n.word Z:$10\n",
    ] {
        assert!(run(source).is_err(), "{source:?}");
    }
}

#[test]
fn a_used_define_traces_to_its_body_and_its_use() {
    use lang198x_syntax::Span;
    let source = ".define F(p) p+1\n.byte F 2\n";
    let (expander, lines) = super::active_lines(source);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    let provenance = expander.provenance();
    let traced = |text: &str| {
        let token = super::token(line, text);
        let span = Span::new(line.text(), token.text_range());
        let at = provenance.trace(span).expect("traces to the file");
        let sites = provenance.sites(span);
        (
            at.range().start().get(),
            sites
                .first()
                .and_then(|s| s.range().slice(source))
                .map(str::to_owned),
        )
    };
    // `.byte` is the line's own; `+` is the body's, used at `F`; `2` is the
    // argument, copied from the use.
    assert_eq!(traced(".byte"), (17, None));
    assert_eq!(traced("+"), (14, Some("F".to_owned())));
    assert_eq!(traced("2"), (25, None));
}

#[test]
fn a_define_that_grows_too_much_stops_the_program() {
    let limits = crate::expand::Limits {
        expanded_bytes: 200,
        ..crate::expand::Limits::default()
    };
    let source = ".define P 1,1,1,1\n.define Q P,P,P,P\n.define W Q,Q,Q,Q\n.byte W\n.byte 9\n";
    let mut expander =
        crate::expand::Expander::with_limits(source, crate::Profile::CA65_2_19_6502, limits)
            .expect("parses");
    let mut host = super::host::TestHost::default();
    while expander.next_line(&mut host).is_some() {}
    assert!(
        host.bytes.is_empty(),
        "{:?} {:?}",
        host.bytes,
        expander.diagnostics()
    );
    assert!(
        expander
            .diagnostics()
            .iter()
            .any(|(_, d)| d.message().contains("too much text")),
        "{:?}",
        expander.diagnostics()
    );
}
