//! Lines traced back to where a person wrote them.

use lang198x_syntax::FileId;

use crate::Profile;
use crate::expand::Expander;
use crate::expand::tests::host::TestHost;

/// Where each line `source` hands over was written: file, line and column.
fn places(source: &str, files: &[(&str, &str)]) -> Vec<(u32, u32, u32)> {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = TestHost::with_files(files);
    let mut places = Vec::new();
    while let Some(line) = expander.next_line(&mut host) {
        let at = expander.locate(line.span()).expect("a place");
        places.push((at.file.get(), at.line, at.column));
    }
    places
}

#[test]
fn a_line_is_traced_through_macros_repeats_and_includes() {
    let source = "  nop\n.macro m\n   rts\n.endmacro\nm\n.repeat 2\n    nop\n.endrepeat\n\
                  .include \"a.inc\"\n";
    assert_eq!(
        places(source, &[("a.inc", "\n .byte 1\n")]),
        [
            (0, 1, 1),
            (0, 3, 1),
            (0, 7, 1),
            (0, 7, 1),
            (1, 1, 1),
            (1, 2, 1)
        ]
    );
}

#[test]
fn a_defined_or_made_line_is_traced_too() {
    // A line with a define-style macro is parsed again; one with a token
    // function's result starts with text the function made.
    let source = ".define D 5\n .byte D\n.ident(\"lab\"): nop\n";
    assert_eq!(places(source, &[]), [(0, 2, 1), (0, 3, 1)]);
}

#[test]
fn a_span_from_elsewhere_is_nowhere() {
    let expander = Expander::new("nop\n", Profile::CA65_2_19_6502).expect("parses");
    let elsewhere = lang198x_syntax::Span::new(
        lang198x_syntax::TextId::File(FileId::new(9)),
        lang198x_syntax::TextRange::empty(lang198x_syntax::TextSize::ZERO),
    );
    assert_eq!(expander.locate(elsewhere), None);
}

/// The line of the outermost macro call that made each line `source` hands
/// over, or `None` for one no call made.
fn calls(source: &str) -> Vec<Option<u32>> {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = TestHost::with_files(&[]);
    let mut calls = Vec::new();
    while let Some(line) = expander.next_line(&mut host) {
        let call = expander.macro_call(line.span());
        calls.push(
            call.and_then(|call| expander.locate(call))
                .map(|at| at.line),
        );
    }
    calls
}

#[test]
fn a_macros_lines_belong_to_its_outermost_call() {
    // A macro inside a repeat: each time round, the call's own line.
    let source = ".macro m\n nop\n.endmacro\n.repeat 2\n m\n.endrepeat\n";
    assert_eq!(calls(source), [Some(5), Some(5)]);
    // A repeat inside a macro: the macro's call.
    let source = ".macro m\n.repeat 2\n nop\n.endrepeat\n.endmacro\nm\n";
    assert_eq!(calls(source), [Some(6), Some(6)]);
    // A macro inside a macro: the outer call.
    let source = ".macro i\n nop\n.endmacro\n.macro o\n i\n.endmacro\n o\n";
    assert_eq!(calls(source), [Some(7)]);
    // No call: a plain line, a repeat's, and a define's.
    let source = " nop\n.repeat 1\n nop\n.endrepeat\n.define D nop\n D\n";
    assert_eq!(calls(source), [None, None, None]);
}

/// Where each of the expander's problems in `source` is reported.
fn reported(source: &str, files: &[(&str, &str)]) -> Vec<(u32, u32, u32)> {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = TestHost::with_files(files);
    while expander.next_line(&mut host).is_some() {}
    expander
        .diagnostics()
        .iter()
        .map(|(text, problem)| {
            let at = expander
                .reading(lang198x_syntax::Span::new(*text, problem.range()))
                .at
                .expect("a place");
            (at.file.get(), at.line, at.column)
        })
        .collect()
}

#[test]
fn a_problem_in_an_included_file_is_reported_there() {
    // Measured: `definitions.inc:2`, even on a line `.ident` made.
    let files = [("d.inc", "; c\n.ident(\"LDA\"):.if 1\n.byte 7\n.endif\n")];
    let got = reported(".include \"d.inc\"\n", &files);
    assert_eq!(got.first(), Some(&(1, 2, 14)));
}

/// Where the expander says each line `source` hands over is reported.
fn readings(source: &str) -> Vec<(u32, u32)> {
    let mut expander = Expander::new(source, Profile::CA65_2_19_6502).expect("parses");
    let mut host = TestHost::with_files(&[]);
    let mut lines = Vec::new();
    while let Some(line) = expander.next_line(&mut host) {
        let at = expander.reading(line.span()).at.expect("a place");
        lines.push((at.file.get(), at.line));
    }
    lines
}

#[test]
fn a_repeats_lines_are_reported_where_the_body_was_written() {
    // ca65 reports at `.endrepeat`, with a note at the body's line; the
    // note is the place that helps.
    let source = ".repeat 2\n nop\n nop\n.endrepeat\n";
    assert_eq!(readings(source), [(0, 2), (0, 3), (0, 2), (0, 3)]);
    // A repeat in a macro is still reported at the call.
    let source = ".macro m\n.repeat 1\n nop\n.endrepeat\n.endmacro\nm\n";
    assert_eq!(readings(source), [(0, 6)]);
}
