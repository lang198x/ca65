//! The formatter's rules. Every change of layout here was checked against
//! ca65 V2.19 at `8d86ca7`: the formatted source assembles to the same bytes,
//! and a refused source is refused with the same errors.

use crate::Profile;

fn formatted(text: &str) -> String {
    let once = super::format(text, Profile::CA65_2_19_6502).expect("formats");
    let twice = super::format(&once, Profile::CA65_2_19_6502).expect("formats again");
    assert_eq!(twice, once, "formatting again changes nothing");
    once
}

#[test]
fn labels_statements_and_comments_take_their_columns() {
    assert_eq!(formatted(" lda #1\n"), "        lda #1\n");
    assert_eq!(formatted("start: lda #1\n"), "start:  lda #1\n");
    assert_eq!(
        formatted("  lda #1 ; one\n"),
        "        lda #1                  ; one\n"
    );
    assert_eq!(formatted("@loop:dex\n"), "@loop:  dex\n");
    assert_eq!(formatted(":  bne :-\n"), ":       bne :-\n");
    assert_eq!(formatted("value = 1\n"), "        value = 1\n");
    // A label longer than the column is followed by one space.
    assert_eq!(formatted("a_long_label: nop\n"), "a_long_label: nop\n");
}

#[test]
fn what_a_statement_says_is_kept_as_written() {
    assert_eq!(
        formatted("  .byte  1,   2 ; spaced\n"),
        "        .byte  1,   2           ; spaced\n"
    );
    // `Z :` keeps its space: `Z:` would be an address size.
    assert_eq!(formatted("Z :.byte 1\n"), "Z :     .byte 1\n");
}

#[test]
fn comments_and_blank_lines_keep_their_text() {
    assert_eq!(formatted("; a note   \n\n   \n"), "; a note\n\n\n");
    assert_eq!(formatted(" nop   "), "        nop");
}

#[test]
fn a_macros_body_is_kept_as_written() {
    let text = ".macro emit v\n  .byte   v   \n.endmacro\n  emit 1\n";
    assert_eq!(
        formatted(text),
        "        .macro emit v\n  .byte   v   \n        .endmacro\n        emit 1\n"
    );
}

#[test]
fn nothing_after_end_is_touched() {
    assert_eq!(
        formatted(" .byte 1\n .end\n  anything  \n"),
        "        .byte 1\n        .end\n  anything  \n"
    );
}

#[test]
fn an_unreadable_line_and_what_ca65_reads_after_it_are_kept() {
    // ca65 reads an unclosed character on into the next line, and how far
    // it gets depends on that line's spaces (measured).
    assert_eq!(
        formatted(".byte 6\n.byte '4294967300'\n.byte 7\n.byte 8\n"),
        "        .byte 6\n.byte '4294967300'\n.byte 7\n        .byte 8\n"
    );
    // A line break inside the quotes leaves a line with nothing readable,
    // so the line after it is kept too.
    assert_eq!(
        formatted(".byte '\n'\n.byte 7\n.byte 8\n"),
        ".byte '\n'\n.byte 7\n        .byte 8\n"
    );
}

#[test]
fn line_endings_are_kept() {
    assert_eq!(
        formatted(" nop\r\n lda #1\r\n"),
        "        nop\r\n        lda #1\r\n"
    );
}
