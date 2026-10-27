//! Each case was assembled by ca65 V2.19 at `8d86ca7` under a flat ld65
//! link. The expander, through a tiny test host, must give ca65's bytes, or
//! refuse what ca65 refuses.

use lang198x_syntax::DiagnosticKind;

use super::*;
use crate::Profile;

mod host;

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
