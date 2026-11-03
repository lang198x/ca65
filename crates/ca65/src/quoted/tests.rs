use super::*;

fn bytes(rest: &str, escapes: bool) -> (Vec<u8>, Option<&'static str>) {
    let quoted = string(rest, '"', escapes);
    (quoted.bytes, quoted.problem)
}

#[test]
fn strings_read_escapes_as_ca65_does() {
    // Measured on ca65 V2.19 with `string_escapes`.
    assert_eq!(bytes(r#""a\nb""#, true), (vec![0x61, 0x0A, 0x62], None));
    assert_eq!(
        bytes(r#""\t\r\n\\\"\'""#, true),
        (vec![0x09, 0x0D, 0x0A, 0x5C, 0x22, 0x27], None)
    );
    assert_eq!(
        bytes(r#""\a\b\e\f\v\?""#, true).0,
        [7, 8, 0x1B, 0x0C, 0x0B, b'?']
    );
    assert_eq!(bytes(r#""\x41\x4a\X4A""#, true).0, [0x41, 0x4A, 0x4A]);
    assert_eq!(bytes(r#""\x414""#, true).0, [0x41, b'4']);
    assert_eq!(bytes(r#""\0\101\0a""#, true).0, [0, 0x41, 0, b'a']);
    for bad in [r#""\x4""#, r#""\x""#, r#""\q""#, r#""\8""#, r#""\xg1""#] {
        assert_eq!(
            bytes(bad, true).1,
            Some("Unsupported escape sequence in string constant"),
            "{bad}"
        );
    }
    assert_eq!(
        bytes(r#""\777""#, true).1,
        Some("Octal character constant out of range")
    );
    assert_eq!(bytes(r#""a\""#, true).1, Some("Newline in string constant"));
    assert_eq!(
        bytes("\"a\\", true).1,
        Some("Unterminated escape sequence in string constant")
    );
    // Without escapes, a backslash is itself.
    assert_eq!(bytes(r#""a\nb""#, false).0, b"a\\nb");
}

#[test]
fn hex_and_octal_escapes_are_raw() {
    let quoted = string(r#""\n\x0a\12""#, '"', true);
    assert_eq!(quoted.bytes, [10, 10, 10]);
    assert_eq!(quoted.raw, [false, true, true]);
}

#[test]
fn a_string_ends_at_its_own_quote() {
    let quoted = string("'a\"b' rest", '\'', false);
    assert_eq!(
        (quoted.len, quoted.bytes.as_slice()),
        (5, b"a\"b".as_slice())
    );
    let quoted = string(r#""a\"b" rest"#, '"', true);
    assert_eq!(quoted.len, 6);
}

#[test]
fn characters_follow_ca65() {
    let read = |rest: &str, escapes, missing| {
        let quoted = character(rest, escapes, missing);
        (quoted.len, quoted.bytes, quoted.problem)
    };
    assert_eq!(read("'a'", false, false), (3, vec![b'a'], None));
    assert_eq!(read(r"'\n'", true, false), (4, vec![10], None));
    assert_eq!(read(r"'\x41'", true, false), (6, vec![0x41], None));
    // `'\'` is a backslash even with escapes.
    assert_eq!(read(r"'\'", true, false), (3, vec![b'\\'], None));
    assert_eq!(read("'a", false, true), (2, vec![b'a'], None));
    assert_eq!(read("'ab", false, true), (2, vec![b'a'], None));
    assert_eq!(
        read("'a", false, false).2,
        Some("Illegal character constant")
    );
    assert_eq!(read("'", false, true).2, Some("Newline in string constant"));
}
