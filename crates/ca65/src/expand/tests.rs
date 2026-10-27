//! Each case was assembled by ca65 V2.19 at `8d86ca7` under a flat ld65
//! link. The expander, through a tiny test host, must give ca65's bytes, or
//! refuse what ca65 refuses.

use lang198x_syntax::DiagnosticKind;

use super::*;
use crate::Profile;
use crate::ast::View;

mod host;
mod repeats;

use host::run;

/// A program, and ca65's bytes for it, or a part of our message for the
/// problem ca65 found.
enum Want {
    Bytes(&'static [u8]),
    Refused(&'static str),
}

use Want::{Bytes, Refused};

const MEASURED: &[(&str, Want)] = &[
    (
        ".if 0\n.byte 1\n.elseif 1\n.byte 2\n.else\n.byte 3\n.endif\n",
        Bytes(&[2]),
    ),
    (
        ".if 1\n.byte 1\n.elseif 1\n.byte 2\n.else\n.byte 3\n.endif\n",
        Bytes(&[1]),
    ),
    (
        ".if 0\n.elseif 0\n.byte 2\n.elseif 1\n.byte 4\n.endif\n",
        Bytes(&[4]),
    ),
    (
        ".if 0\n.if 1\n.byte 1\n.else\n.byte 2\n.endif\n.else\n.byte 3\n.endif\n",
        Bytes(&[3]),
    ),
    (
        ".ifdef v\n.byte 1\n.endif\nv=1\n.ifdef v\n.byte 2\n.endif\n",
        Bytes(&[2]),
    ),
    (".ifndef v\n.byte 1\n.endif\n", Bytes(&[1])),
    (
        ".ifblank\n.byte 1\n.endif\n.ifnblank x9\n.byte 2\n.endif\n.ifblank {}\n.byte 3\n.endif\n",
        Bytes(&[1, 2]),
    ),
    (
        ".ifp02\n.byte 1\n.endif\n.ifp816\n.byte 2\n.endif\n",
        Bytes(&[1]),
    ),
    (
        ".ifconst 1\n.byte 1\n.endif\n.ifconst *\n.byte 2\n.endif\n.ifnconst *\n.byte 3\n.endif\n",
        Bytes(&[1, 3]),
    ),
    (
        ".ifref v\n.byte 1\n.endif\nv=2\n.word v\n.ifref v\n.byte 3\n.endif\n.ifnref v\n.byte 4\n.endif\n",
        Bytes(&[2, 0, 3]),
    ),
    (".byte 1\n.end\n.byte 2\n", Bytes(&[1])),
    (".if 0\n.end\n.endif\n.byte 1\n", Bytes(&[1])),
    // ca65 skips a false branch token by token, so a `.endif` partway
    // through a line ends it, and anything after it is refused.
    (".if 0\n.byte 1 .endif\n.byte 2\n", Bytes(&[2])),
    (
        ".if 0\n.byte 1\n.endif .byte 3\n.byte 2\n",
        Refused("expected the end of the line"),
    ),
    // A skipped branch's syntax is not checked, but its scanner still runs.
    (".if 0\n.byte .left(\n.endif\n.byte 1\n", Bytes(&[1])),
    (
        ".if 0\n.bogus\n.endif\n.byte 1\n",
        Refused("not a control command"),
    ),
    (".if 1\n.byte 1\n", Refused("never closed")),
    (".else\n", Refused("`.else` with no `.if` open")),
    (".endif\n", Refused("`.endif` with no `.if` open")),
    (".if 1\n.else\n.else\n.endif\n", Refused("a second `.else`")),
    (
        ".if later\n.endif\nlater=1\n",
        Refused("`later` is not defined"),
    ),
    // Macros.
    (".macro m op\n op #1\n.endmacro\nm lda\n", Bytes(&[0xA9, 1])),
    (
        ".macro m p\n.byte p\n.endmacro\nm {1}\nm 2\n",
        Bytes(&[1, 2]),
    ),
    (
        ".macro m p\n.byte .paramcount, p\n.endmacro\nm {1}\n",
        Bytes(&[1, 1]),
    ),
    (
        ".macro m p,q\n.byte .paramcount\n.endmacro\nm 1\nm 1,2\n",
        Bytes(&[1, 2]),
    ),
    (".macro m p\n.byte p+0\n.endmacro\nm\n", Bytes(&[0])),
    (".macro m p\n.byte <p\n.endmacro\nm $1234\n", Bytes(&[0x34])),
    (".macro m P\n.byte p\n.endmacro\np=5\nm 1\n", Bytes(&[5])),
    (
        ".macro m p\n.local l\nl: .byte p\n.word l\n.endmacro\nm 1\nm 2\n",
        Bytes(&[1, 0, 0, 2, 3, 0]),
    ),
    (
        ".macro m\n.byte 1\n.exitmacro\n.byte 2\n.endmacro\nm\n",
        Bytes(&[1]),
    ),
    (
        ".macro m\n.if 0\n.exitmacro\n.endif\n.byte 1\n.if 1\n.exitmacro\n.endif\n.byte 2\n.endmacro\nm\n.byte 3\n",
        Bytes(&[1, 3]),
    ),
    // A conditional opened in a macro stays open after it ends.
    (
        ".macro m\n.if 1\n.endmacro\nm\n.byte 1\n.endif\n",
        Bytes(&[1]),
    ),
    (".exitmacro\n", Refused("`.exitmacro` outside a macro")),
    (
        ".macro m p\nm p\n.endmacro\nm 1\n",
        Refused("too many nested macro calls"),
    ),
    (
        ".macro m p, q\n.byte p, q\n.endmacro\nm 1\n",
        Refused("expected a value"),
    ),
    (
        ".macro m\n.endmacro\nm 1\n",
        Refused("takes 0 arguments, and 1 are given"),
    ),
    // Definitions do not nest: the first `.endmacro` ends the outer one.
    (
        ".macro outer\n.macro inner\n.byte 7\n.endmacro\n.endmacro\nouter\ninner\n",
        Refused("no `.endmacro`"),
    ),
    (
        ".macro m\n.byte 1\n.endmacro\n.macro m\n.endmacro\n",
        Refused("already defined"),
    ),
    (
        ".if 0\n.macro m\n.endmacro\n.endif\nm\n",
        Refused("expected `:` after a name"),
    ),
    (
        ".mac m\n.byte 1\n.endmac\nm\n.delmac m\nm\n",
        Refused("expected `:` after a name"),
    ),
];

#[test]
fn every_measured_program_matches_ca65() {
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
fn a_skipped_lines_syntax_problems_do_not_count() {
    // `.byte (` is broken, but it never runs.
    assert_eq!(run(".if 0\n.byte (\n.endif\n"), Ok(vec![]));
    assert!(run(".byte (\n").is_err());
}

#[test]
fn conditionals_inside_a_skipped_branch_nest() {
    let source = ".if 0\n.if 1\n.endif\n.byte 1\n.else\n.byte 2\n.endif\n";
    assert_eq!(run(source), Ok(vec![2]));
    // A `.elseif` in a skipped `.if` is not asked for.
    assert_eq!(
        run(".if 0\n.if 0\n.elseif later\n.endif\n.endif\n"),
        Ok(vec![])
    );
}

#[test]
fn a_skipped_branch_can_end_at_else_or_elseif_partway_through_a_line() {
    assert_eq!(run(".if 0\n.byte 1 .else\n.byte 2\n.endif\n"), Ok(vec![2]));
    // Whose condition counts there is not supported yet.
    let problems = run(".if 0\n.byte 1 .elseif 1\n.endif\n").expect_err("unsupported");
    assert!(problems[0].contains("not supported yet"), "{problems:?}");
    // Misplaced conditionals in a skipped branch are still reported.
    let problems = run(".if 0\n.else\n.else\n.endif\n").expect_err("duplicate");
    assert!(problems[0].contains("a second `.else`"), "{problems:?}");
    let problems = run(".if 0\n.else\n.elseif 1\n.endif\n").expect_err("duplicate");
    assert!(problems[0].contains("a second `.else`"), "{problems:?}");
}

#[test]
fn a_misplaced_elseif_while_lines_run() {
    let problems = run(".elseif 1\n").expect_err("unexpected");
    assert!(
        problems[0].contains("`.elseif` with no `.if` open"),
        "{problems:?}"
    );
}

#[test]
fn every_conditional_command_is_known() {
    for word in [".if", ".ifp4510", ".endif", ".IFREF"] {
        assert!(super::expander::is_conditional(word), "{word}");
    }
    assert!(!super::expander::is_conditional(".byte"));
}

#[test]
fn the_line_limit_stops_a_program() {
    let limits = Limits {
        lines: 2,
        ..Limits::default()
    };
    let mut expander = Expander::with_limits(
        ".byte 1\n.byte 2\n.byte 3\n",
        Profile::CA65_2_19_6502,
        limits,
    )
    .expect("short text parses");
    let mut host = host::TestHost::default();
    let mut count = 0;
    while expander.next_line(&mut host).is_some() {
        count += 1;
    }
    assert_eq!(count, 2);
    let (_, diagnostic) = &expander.diagnostics()[0];
    assert_eq!(diagnostic.kind(), DiagnosticKind::Invalid);
    assert!(diagnostic.message().contains("too many lines"));
    // Once done, the expander stays done.
    assert!(expander.next_line(&mut host).is_none());
}

#[test]
fn an_active_line_knows_where_it_is() {
    let mut expander = Expander::new("\n nop\n", Profile::CA65_2_19_6502).expect("parses");
    let mut host = host::TestHost::default();
    let _blank = expander.next_line(&mut host).expect("the blank line");
    let line = expander.next_line(&mut host).expect("a line");
    assert_eq!(line.text(), line.span().text());
    assert_eq!(line.span().range().start().get(), 1);
}

/// Every line `source` hands to the host.
fn active_lines(source: &str) -> (Expander, Vec<ActiveLine>) {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = host::TestHost::default();
    let mut lines = Vec::new();
    while let Some(line) = expander.next_line(&mut host) {
        lines.push(line);
    }
    (expander, lines)
}

/// The first token of `line` whose text is `text`.
fn token(line: &ActiveLine, text: &str) -> lang198x_syntax::SyntaxToken {
    line.line()
        .syntax()
        .descendants()
        .filter_map(lang198x_syntax::SyntaxElement::into_token)
        .find(|t| t.text() == text)
        .expect("the token")
}

#[test]
fn a_macro_lines_text_traces_to_the_body_and_the_argument() {
    use lang198x_syntax::Span;
    let source = ".macro m p\n lda #p\n.endmacro\nm 5\n";
    let (expander, lines) = active_lines(source);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    let provenance = expander.provenance();
    let traced = |text: &str| {
        let token = token(line, text);
        let span = provenance
            .trace(Span::new(line.text(), token.text_range()))
            .expect("traces to the file");
        span.range().slice(source).map(str::to_owned)
    };
    assert_eq!(traced("lda").as_deref(), Some("lda"));
    let five = token(line, "5");
    let span = provenance
        .trace(Span::new(line.text(), five.text_range()))
        .expect("traces");
    assert_eq!(span.range().start().get(), 31, "the argument, not the body");
    assert_eq!(span.range().slice(source), Some("5"));
    let sites = provenance.sites(Span::new(line.text(), five.text_range()));
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].range().slice(source), Some("m 5"));
}

#[test]
fn made_up_text_says_what_it_was_made_from() {
    use lang198x_syntax::Span;
    let source = ".macro m\n.local l\nl: .byte .paramcount\n.endmacro\nm\n";
    let (expander, lines) = active_lines(source);
    let line = &lines[0];
    let name = token(line, "LOCAL_MACRO_SYMBOL_0000");
    let span = Span::new(line.text(), name.text_range());
    let provenance = expander.provenance();
    assert_eq!(provenance.trace(span), None);
    let lang198x_syntax::TextId::Expansion(id) = line.text() else {
        panic!("a macro's text")
    };
    let origin = provenance
        .get(id)
        .and_then(|e| e.origin_at(name.text_range().start()))
        .expect("an origin");
    let made_from = origin.made_from().expect("synthesised");
    assert_eq!(made_from.range().slice(source), Some("l"));
    let count = token(line, "0");
    let origin = provenance
        .get(id)
        .and_then(|e| e.origin_at(count.text_range().start()))
        .expect("an origin");
    assert_eq!(
        origin.made_from().and_then(|s| s.range().slice(source)),
        Some(".paramcount")
    );
}

#[test]
fn a_label_before_a_call_reaches_the_host_first() {
    let (_, lines) = active_lines(".macro m\n nop\n.endmacro\nhere: m\n: m\n");
    let flags: Vec<bool> = lines.iter().map(ActiveLine::label_only).collect();
    assert_eq!(flags, [true, false, true, false]);
    assert_eq!(
        run(".macro m\n.word here\n.endmacro\n.byte 9\nhere: m\n"),
        Ok(vec![9, 1, 0])
    );
}

#[test]
fn deleting_a_macro_that_does_not_exist() {
    let problems = run(".delmac m\n").expect_err("no such macro");
    assert!(
        problems[0].contains("no macro is named `m`"),
        "{problems:?}"
    );
}

#[test]
fn the_text_limit_stops_a_growing_macro() {
    let limits = Limits {
        expanded_bytes: 100,
        ..Limits::default()
    };
    let source = ".macro m p\n.byte 1\nm {p p}\n.endmacro\nm 1\n";
    let mut expander =
        Expander::with_limits(source, Profile::CA65_2_19_6502, limits).expect("parses");
    let mut host = host::TestHost::default();
    while expander.next_line(&mut host).is_some() {}
    let messages: Vec<&str> = expander
        .diagnostics()
        .iter()
        .map(|(_, d)| d.message())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains("too much text")),
        "{messages:?}"
    );
}

#[test]
fn each_call_has_its_own_local_names() {
    let (_, lines) = active_lines(".macro m\n.local l\nl: nop\n.endmacro\nm\nm\n");
    let names: Vec<String> = lines
        .iter()
        .filter_map(|l| {
            l.line()
                .label()
                .and_then(|label| label.name())
                .map(|n| n.text())
        })
        .collect();
    assert_eq!(
        names,
        ["LOCAL_MACRO_SYMBOL_0000", "LOCAL_MACRO_SYMBOL_0001"]
    );
}
