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
