//! The value of a number token.

/// The value of a number as ca65 writes it: `$hex`, `%binary`, decimal, or
/// hex ending in `h`. ca65 keeps 32 bits.
pub(crate) fn value(text: &str) -> Option<i64> {
    let (digits, radix) = if let Some(hex) = text.strip_prefix('$') {
        (hex, 16)
    } else if let Some(binary) = text.strip_prefix('%') {
        (binary, 2)
    } else if let Some(hex) = text.strip_suffix(['h', 'H']) {
        (hex, 16)
    } else {
        (text, 10)
    };
    if radix == 2 || text.starts_with('$') {
        return i64::from_str_radix(digits, radix)
            .ok()
            .filter(|v| *v <= i64::from(u32::MAX));
    }
    // ca65 checks a decimal or `h` number before each digit, so the last
    // digit can carry it past 32 bits: 4294967299 is a number, 4294967300
    // is not.
    crate::lexer::digit_led_value(digits, radix)
        .ok()
        .and_then(|value| i64::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use super::value;

    #[test]
    fn a_decimal_number_can_pass_32_bits_on_its_last_digit() {
        // Measured: `.byte <(4294967299-4294967296)` gives 3, and
        // `4294967300` is "Number out of range".
        assert_eq!(value("4294967299"), Some(4_294_967_299));
        assert_eq!(value("4294967300"), None);
        assert_eq!(value("0FFFFFFFFh"), Some(0xFFFF_FFFF));
        assert_eq!(value("$FFFFFFFF"), Some(0xFFFF_FFFF));
        assert_eq!(value("$100000000"), None);
    }
}
