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
    i64::from_str_radix(digits, radix)
        .ok()
        .filter(|v| *v <= i64::from(u32::MAX))
}
