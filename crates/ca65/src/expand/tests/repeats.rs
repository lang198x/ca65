//! Repeats, each case measured on ca65 V2.19 at `8d86ca7` like the others.

use super::{Bytes, Refused, Want, run};
use crate::Profile;
use crate::expand::Expander;

const MEASURED: &[(&str, Want)] = &[
    (
        "n=3\n.repeat n, i\n.byte i*2\n.endrepeat\n",
        Bytes(&[0, 2, 4]),
    ),
    (".repeat 0\n.byte 1\n.endrepeat\n.byte 2\n", Bytes(&[2])),
    (
        ".repeat 2\n.byte 1\n.endrepeat\n.byte 2\n",
        Bytes(&[1, 1, 2]),
    ),
    (".repeat 2\n.byte 1\n.endrep\n", Bytes(&[1, 1])),
    (".repeat 2\n.endrepeat\n", Bytes(&[])),
    (".repeat 1+1*2, i\n.byte i\n.endrepeat\n", Bytes(&[0, 1, 2])),
    (".repeat 2 ,i\n.byte i\n.endrepeat\n", Bytes(&[0, 1])),
    (
        ".repeat 2, i ; c\n.byte i ; i\n.endrepeat ; c\n",
        Bytes(&[0, 1]),
    ),
    (
        ".repeat 2, i\n.byte $10+i\n.endrepeat\n",
        Bytes(&[0x10, 0x11]),
    ),
    (
        ".repeat 2, i\nlda #i\n.endrepeat\n",
        Bytes(&[0xA9, 0, 0xA9, 1]),
    ),
    // The counter is case-sensitive.
    (".repeat 2, I\n.byte I\n.endrepeat\n", Bytes(&[0, 1])),
    ("i=9\n.repeat 2, I\n.byte i\n.endrepeat\n", Bytes(&[9, 9])),
    // Repeats nest, and an inner count can use an outer counter.
    (
        ".repeat 2, i\n.repeat 2, j\n.byte i*16+j\n.endrepeat\n.endrepeat\n",
        Bytes(&[0, 1, 0x10, 0x11]),
    ),
    (
        ".repeat 2, i\n.repeat i+1, j\n.byte i, j\n.endrepeat\n.endrepeat\n",
        Bytes(&[0, 0, 1, 0, 1, 1]),
    ),
    (
        ".repeat 2, i\n.repeat i, j\n.byte j\n.endrepeat\n.endrepeat\n",
        Bytes(&[0]),
    ),
    (
        ".repeat 2, i\n.repeat 2\n.endrepeat\n.byte i\n.endrepeat\n",
        Bytes(&[0, 1]),
    ),
    // A label before `.repeat` is defined where the repeat starts.
    (
        "q: .repeat 2\n.byte 1\n.endrepeat\n.word q\n",
        Bytes(&[1, 1, 0, 0]),
    ),
    (
        ".repeat 2, i\n.if i\n.byte 5\n.endif\n.endrepeat\n",
        Bytes(&[5]),
    ),
    (
        ".macro m c\n.repeat c, k\n.byte k\n.endrepeat\n.endmacro\nm 3\n",
        Bytes(&[0, 1, 2]),
    ),
    // Conditionals are not tied to the repeat they open in.
    (".repeat 2\n.if 1\n.endrepeat\n.endif\n.endif\n", Bytes(&[])),
    (
        ".if 1\n.if 1\n.repeat 2\n.endif\n.endrepeat\n.byte 4\n",
        Bytes(&[4]),
    ),
    (
        ".if 1\n.repeat 2\n.endif\n.byte 1\n.endrepeat\n",
        Refused("`.endif` with no `.if` open"),
    ),
    (
        ".repeat 2\n.endif\n.endrepeat\n",
        Refused("`.endif` with no `.if` open"),
    ),
    // A skipped branch skips a repeat token by token, even a broken one.
    (
        ".if 0\n.repeat 2\n.byte 1\n.endrepeat\n.endif\n.byte 3\n",
        Bytes(&[3]),
    ),
    (".if 0\n.repeat\n.endif\n.byte 3\n", Bytes(&[3])),
    // A repeat that runs no times may end partway through a line.
    (".repeat 0\n.byte 1 .endrepeat\n.byte 2\n", Bytes(&[2])),
    (
        ".repeat -1\n.byte 1\n.endrepeat\n",
        Refused("must not be negative"),
    ),
    (
        ".repeat later\n.byte 1\n.endrepeat\nlater=1\n",
        Refused("`later` is not defined"),
    ),
    (".repeat 2\n.byte 1\n", Refused("has no `.endrepeat`")),
    (
        ".macro m\n.byte 1\n.endrepeat\n.endmacro\n.repeat 2\nm\n",
        Refused("has no `.endrepeat`"),
    ),
    (".endrepeat\n", Refused("with no `.repeat` open")),
    (
        ".repeat 2, @i\n.byte 1\n.endrepeat\n",
        Refused("must be a plain name"),
    ),
    // The outer counter replaces the inner one's name.
    (
        ".repeat 2, i\n.repeat 2, i\n.byte i\n.endrepeat\n.endrepeat\n",
        Refused("must be a plain name"),
    ),
    (
        ".repeat 1\n.byte 1\n.endrepeat .byte 9\n",
        Refused("expected the end of the line"),
    ),
    (
        ".repeat 2, i\n.macro m\n.byte i\n.endmacro\n.endrepeat\n",
        Refused("already defined"),
    ),
    (
        ".repeat 2, i\nlab: .byte i\n.endrepeat\n",
        Refused("defined twice"),
    ),
    (
        ".repeat 3, i\n.if i = 1\n.exitmacro\n.endif\n.byte i\n.endrepeat\n",
        Refused("`.exitmacro` outside a macro"),
    ),
];

#[test]
fn every_measured_repeat_matches_ca65() {
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
fn measured_programs_that_ca65_refuses_for_the_parser() {
    // ca65 refuses each of these; the message is the parser's own.
    for source in [
        ".repeat 2,\n.byte 1\n.endrepeat\n",
        ".repeat 2 3\n.byte 1\n.endrepeat\n",
        ".repeat 2, x\n.byte 1\n.endrepeat\n",
        ".repeat 2, nop\nnop\n.endrepeat\n",
        ".repeat 2, i\n.byte i::q\n.endrepeat\n",
        ".repeat 2, i\n.repeat 1\n.byte i .endrepeat\n.endrepeat\n",
    ] {
        assert!(run(source).is_err(), "{source:?}");
    }
}

/// A `.endrepeat` after other tokens on its line, measured on ca65 V2.19:
/// each round's unfinished last line runs on into the next line read, and
/// after the last round into the line after the `.endrepeat`.
const JOINED: &[(&str, Option<&[u8]>)] = &[
    (
        ".repeat 1\n.byte 7\ntail:.endrepeat\n.word tail\n",
        Some(&[7, 1, 0]),
    ),
    (".repeat 2\n.byte 7\ntail:.endrepeat\n.word tail\n", None),
    (".repeat 1\n.byte 1 .endrepeat\n", Some(&[1])),
    (".repeat 1\n.byte 1 .endrepeat\n.byte 2\n", None),
    (".repeat 1\n.byte 1, .endrepeat\n2\n", Some(&[1, 2])),
    (".repeat 1\n.byte 1 .endrepeat\n\n.byte 2\n", Some(&[1, 2])),
    (
        ".repeat 1\n.byte 7\ntail:.endrepeat\n.byte 2\n.word tail\n",
        Some(&[7, 2, 1, 0]),
    ),
    (".repeat 2\nnop\n.byte 1 .endrepeat\n", None),
    (".repeat 2\nnop\n: .endrepeat\n", Some(&[0xEA, 0xEA])),
    (".repeat 2\n: .endrepeat\nnop\n", None),
    (".repeat 2, i\nl:\n.byte i\n@c:.endrepeat\n", None),
    (
        ".repeat 2, i\n.byte i\nl .set i .endrepeat\n.byte l\n",
        None,
    ),
    (".repeat 0\n.byte 1 .endrepeat\n.byte 2\n", Some(&[2])),
];

#[test]
fn an_endrepeat_partway_through_a_line_joins_the_next_line() {
    for (source, want) in JOINED {
        let got = run(source);
        match want {
            Some(bytes) => assert_eq!(got.as_deref(), Ok(*bytes), "{source:?}"),
            None => assert!(got.is_err(), "{source:?}: {got:?}"),
        }
    }
}

#[test]
fn exitmacro_leaves_a_repeat_inside_the_macro_at_once() {
    // ca65 V2.19 gives 00 01 02 09: it finishes the repeat before leaving
    // the macro. Its manual says `.exitmacro` aborts the expansion
    // immediately, so this is a bug, and it is not copied.
    let source = ".macro m\n.repeat 3, i\n.if i = 1\n.exitmacro\n.endif\n.byte i\n.endrepeat\n.byte 7\n.endmacro\nm\n.byte 9\n";
    assert_eq!(run(source).as_deref(), Ok(&[0, 9][..]));
}

#[test]
fn end_inside_a_repeat_ends_the_program() {
    // ca65 V2.19 fails an internal check here, rather than reporting.
    assert_eq!(
        run(".repeat 3\n.end\n.endrepeat\n.byte 1\n").as_deref(),
        Ok(&[][..])
    );
}

#[test]
fn a_counter_says_what_it_was_made_from() {
    use lang198x_syntax::{Span, TextId};
    let source = ".repeat 2, i\n.byte i\n.endrepeat\n";
    let (expander, lines) = super::active_lines(source);
    assert_eq!(lines.len(), 2);
    let provenance = expander.provenance();
    for (number, line) in ["0", "1"].into_iter().zip(&lines) {
        let byte = super::token(line, ".byte");
        let span = provenance
            .trace(Span::new(line.text(), byte.text_range()))
            .expect("traces to the file");
        assert_eq!(span.range().slice(source), Some(".byte"));
        let sites = provenance.sites(Span::new(line.text(), byte.text_range()));
        assert_eq!(sites[0].range().slice(source), Some(".repeat 2, i"));
        let counter = super::token(line, number);
        let TextId::Expansion(id) = line.text() else {
            panic!("a repeat's text")
        };
        let origin = provenance
            .get(id)
            .and_then(|e| e.origin_at(counter.text_range().start()))
            .expect("an origin");
        assert_eq!(
            origin.made_from().and_then(|s| s.range().slice(source)),
            Some("i")
        );
    }
}

#[test]
fn the_line_limit_stops_a_repeat_that_runs_too_long() {
    let limits = crate::expand::Limits {
        lines: 100,
        ..crate::expand::Limits::default()
    };
    let source = ".repeat 1000000000\n.byte 1\n.endrepeat\n";
    let mut expander =
        Expander::with_limits(source, Profile::CA65_2_19_6502, limits).expect("parses");
    let mut host = super::host::TestHost::default();
    while expander.next_line(&mut host).is_some() {}
    assert!(host.bytes.len() < 100);
    assert!(
        expander
            .diagnostics()
            .iter()
            .any(|(_, d)| d.message().contains("too many lines"))
    );
}

#[test]
fn the_text_limit_stops_a_repeat_that_makes_too_much() {
    let limits = crate::expand::Limits {
        expanded_bytes: 64,
        ..crate::expand::Limits::default()
    };
    let source = ".repeat 100\n.byte 1, 2, 3, 4, 5, 6, 7, 8\n.endrepeat\n";
    let mut expander =
        Expander::with_limits(source, Profile::CA65_2_19_6502, limits).expect("parses");
    let mut host = super::host::TestHost::default();
    while expander.next_line(&mut host).is_some() {}
    assert!(
        expander
            .diagnostics()
            .iter()
            .any(|(_, d)| d.message().contains("repeats make too much text"))
    );
}

#[test]
fn a_repeat_needs_a_count_and_takes_no_more_than_a_counter() {
    for (source, part) in [
        (".repeat\n.endrepeat\n", "needs a count"),
        (".repeat 1, i, j\n.endrepeat\n", "no more"),
    ] {
        let problems = run(source).expect_err(source);
        assert!(problems.iter().any(|p| p.contains(part)), "{problems:?}");
    }
}
