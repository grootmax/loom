//! Character classes used by the lexer.
//!
//! Identifier characters use `XID_Start` / `XID_Continue` (spec §2.5). Until the generated
//! tables in `loom_unicode` land (task 0005), non-ASCII characters are classified with
//! `char::is_alphabetic` / `char::is_alphanumeric` plus combining marks, which agrees with
//! XID for every script used in practice. ASCII is exact.

/// Whether `c` can start an identifier: `_` or `XID_Start`.
pub(crate) fn is_ident_start(c: char) -> bool {
    if c.is_ascii() {
        c == '_' || c.is_ascii_alphabetic()
    } else {
        c.is_alphabetic() && !is_unicode_whitespace(c)
    }
}

/// Whether `c` can continue an identifier: `XID_Continue`.
pub(crate) fn is_ident_continue(c: char) -> bool {
    if c.is_ascii() {
        c == '_' || c.is_ascii_alphanumeric()
    } else {
        (c.is_alphanumeric() || is_combining_mark(c)) && !is_unicode_whitespace(c)
    }
}

/// Combining marks in the blocks most identifiers use (an approximation of `Mn`/`Mc`).
fn is_combining_mark(c: char) -> bool {
    matches!(u32::from(c),
        0x0300..=0x036F | 0x0483..=0x0487 | 0x0591..=0x05BD | 0x0610..=0x061A
        | 0x064B..=0x065F | 0x0900..=0x0903 | 0x093A..=0x094F | 0x0E31 | 0x0E34..=0x0E3A
        | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x200C | 0x200D | 0x20D0..=0x20FF
        | 0x3099 | 0x309A | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F)
}

/// Unicode `White_Space` characters other than space and tab (spec §2.2.2), plus the
/// line terminators handled elsewhere. This property is small and stable, so it is
/// listed here rather than generated.
pub(crate) fn is_unicode_whitespace(c: char) -> bool {
    matches!(
        u32::from(c),
        0x000B | 0x000C | 0x0085 | 0x00A0 | 0x1680 | 0x2000
            ..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000
    )
}

/// A readable name for a whitespace character, for diagnostics.
pub(crate) fn whitespace_name(c: char) -> &'static str {
    match u32::from(c) {
        0x000B => "vertical tab",
        0x000C => "form feed",
        0x0085 => "next line",
        0x00A0 => "no-break space",
        0x1680 => "Ogham space mark",
        0x2000..=0x200A => "typographic space",
        0x2028 => "line separator",
        0x2029 => "paragraph separator",
        0x202F => "narrow no-break space",
        0x205F => "medium mathematical space",
        0x3000 => "ideographic space",
        _ => "whitespace character",
    }
}

/// Bidirectional formatting controls (spec §2.1.4).
pub(crate) fn is_bidi_control(c: char) -> bool {
    matches!(u32::from(c), 0x202A..=0x202E | 0x2066..=0x2069)
}

#[cfg(test)]
mod tests {
    use super::{is_bidi_control, is_ident_continue, is_ident_start, is_unicode_whitespace};

    #[test]
    fn identifier_classes() {
        for c in ['a', 'Z', '_', 'é', 'λ', 'Ж', '中', 'ß'] {
            assert!(is_ident_start(c), "{c}");
            assert!(is_ident_continue(c), "{c}");
        }
        for c in ['0', '9', '\u{0301}'] {
            assert!(!is_ident_start(c), "{c}");
            assert!(is_ident_continue(c), "{c}");
        }
        for c in ['-', ' ', '$', '😀', '\u{00A0}', '?', '!'] {
            assert!(!is_ident_start(c), "{c}");
            assert!(!is_ident_continue(c), "{c}");
        }
    }

    #[test]
    fn special_characters() {
        assert!(is_unicode_whitespace('\u{00A0}'));
        assert!(is_unicode_whitespace('\u{3000}'));
        assert!(!is_unicode_whitespace(' '));
        assert!(!is_unicode_whitespace('\u{200B}'));
        assert!(is_bidi_control('\u{202E}'));
        assert!(is_bidi_control('\u{2066}'));
        assert!(!is_bidi_control('\u{200F}'));
    }
}
