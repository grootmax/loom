//! Property tests for `loom_unicode`.

use loom_unicode::{UNICODE_VERSION, is_white_space, is_xid_continue, is_xid_start};

#[test]
fn test_unicode_version() {
    assert_eq!(UNICODE_VERSION, (17, 0, 0));
}

#[test]
#[allow(clippy::too_many_lines)]
fn test_spot_checks() {
    struct SpotCheck {
        c: char,
        start: bool,
        continue_: bool,
        space: bool,
    }

    let cases = [
        // XID_Start (also XID_Continue, not White_Space)
        SpotCheck {
            c: 'a',
            start: true,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: 'Z',
            start: true,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: 'é',
            start: true,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: 'ß',
            start: true,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: 'λ',
            start: true,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: 'Ж',
            start: true,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: '中',
            start: true,
            continue_: true,
            space: false,
        },
        // XID_Continue but not XID_Start
        SpotCheck {
            c: '0',
            start: false,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: '_',
            start: false,
            continue_: true,
            space: false,
        },
        SpotCheck {
            c: '\u{0301}',
            start: false,
            continue_: true,
            space: false,
        }, // combining acute
        // Neither XID_Start nor XID_Continue
        SpotCheck {
            c: '-',
            start: false,
            continue_: false,
            space: false,
        },
        SpotCheck {
            c: '😀',
            start: false,
            continue_: false,
            space: false,
        },
        SpotCheck {
            c: '$',
            start: false,
            continue_: false,
            space: false,
        },
        // White_Space checks
        SpotCheck {
            c: ' ',
            start: false,
            continue_: false,
            space: true,
        },
        SpotCheck {
            c: '\t',
            start: false,
            continue_: false,
            space: true,
        },
        SpotCheck {
            c: '\n',
            start: false,
            continue_: false,
            space: true,
        },
        SpotCheck {
            c: '\u{00A0}',
            start: false,
            continue_: false,
            space: true,
        }, // no-break space
        SpotCheck {
            c: '\u{2003}',
            start: false,
            continue_: false,
            space: true,
        }, // em space
        SpotCheck {
            c: '\u{3000}',
            start: false,
            continue_: false,
            space: true,
        }, // ideographic space
        SpotCheck {
            c: '\u{2028}',
            start: false,
            continue_: false,
            space: true,
        }, // line separator
        // Not White_Space
        SpotCheck {
            c: '\u{200B}',
            start: false,
            continue_: false,
            space: false,
        }, // zero width space
    ];

    for case in cases {
        assert_eq!(
            is_xid_start(case.c),
            case.start,
            "is_xid_start failed for {:?}",
            case.c
        );
        assert_eq!(
            is_xid_continue(case.c),
            case.continue_,
            "is_xid_continue failed for {:?}",
            case.c
        );
        assert_eq!(
            is_white_space(case.c),
            case.space,
            "is_white_space failed for {:?}",
            case.c
        );
    }
}

#[test]
fn test_all_char_invariants() {
    for i in 0..=0x0010_FFFF {
        if let Some(c) = char::from_u32(i) {
            let start = is_xid_start(c);
            let continue_ = is_xid_continue(c);
            let space = is_white_space(c);

            // Invariant: every XID_Start char is XID_Continue
            if start {
                assert!(continue_, "XID_Start char {c:?} must be XID_Continue");
            }

            // Invariant: every ASCII char agrees with requirement 2 ASCII definitions
            if c.is_ascii() {
                let ascii_start = c.is_ascii_alphabetic();
                let ascii_continue = c.is_ascii_alphanumeric() || c == '_';
                let ascii_space = matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C');

                assert_eq!(start, ascii_start, "ASCII XID_Start mismatch for {c:?}");
                assert_eq!(
                    continue_, ascii_continue,
                    "ASCII XID_Continue mismatch for {c:?}"
                );
                assert_eq!(space, ascii_space, "ASCII White_Space mismatch for {c:?}");
            }
        }
    }
}

#[test]
fn test_alphabetic_cross_check() {
    let mut failures = Vec::new();
    for i in 0..=0x0010_FFFF {
        if let Some(c) = char::from_u32(i)
            && c.is_alphabetic()
            && !(is_xid_start(c) || is_xid_continue(c))
            && !is_allowed_alphabetic_exception(c)
        {
            failures.push(c);
        }
    }
    assert!(
        failures.is_empty(),
        "Unexpected alphabetic chars not in XID_Start/Continue: {failures:?}"
    );
}

/// Allow-list of characters that Rust `std` considers `is_alphabetic()`, but UAX #31
/// excludes from `XID_Start` and `XID_Continue`.
///
/// UAX #31 excludes these character ranges from `XID_Start`/`XID_Continue` because their
/// NFKC normalization expands to spaces, multi-character sequences, or non-identifier forms:
/// - U+037A: Greek Ypogegrammeni (NFKC expands to space U+0020 + U+0345).
/// - U+24B6..=U+24E9: Enclosed Alphanumerics (circled letters like 'Ⓐ'; NFKC normalizes to ASCII letters).
/// - U+2E2F: Vertical Tilde.
/// - U+FC5E..=U+FC63, U+FDFA, U+FDFB, U+FE70..=U+FE7F: Arabic presentation forms & ligatures (NFKC expands to multi-word phrases like U+FDFA).
/// - U+1F130..=U+1F149, U+1F150..=U+1F169, U+1F170..=U+1F189: Enclosed Alphanumeric Supplement (squared letters like '🄰'; NFKC normalizes to ASCII letters).
fn is_allowed_alphabetic_exception(c: char) -> bool {
    matches!(
        c,
        '\u{037A}'
            | '\u{24B6}'..='\u{24E9}'
            | '\u{2E2F}'
            | '\u{FC5E}'..='\u{FC63}'
            | '\u{FDFA}'
            | '\u{FDFB}'
            | '\u{FE70}'..='\u{FE7F}'
            | '\u{1F130}'..='\u{1F149}'
            | '\u{1F150}'..='\u{1F169}'
            | '\u{1F170}'..='\u{1F189}'
    )
}
