//! Generated Unicode character property tables and query functions for Loom.
//!
//! Provides fast ASCII-aware predicates for UAX #31 identifier characters
//! (`XID_Start`, `XID_Continue`) and UCD `White_Space` characters based on
//! Unicode version 17.0.0.
//!
//! # Data Sources
//! The underlying tables are generated from the Unicode Character Database (UCD) 17.0.0:
//! - `https://www.unicode.org/Public/17.0.0/ucd/DerivedCoreProperties.txt`
//! - `https://www.unicode.org/Public/17.0.0/ucd/PropList.txt`

mod tables;

/// The supported version of the Unicode Character Database (UCD).
pub const UNICODE_VERSION: (u8, u8, u8) = (17, 0, 0);

/// Checks if a character has the Unicode `XID_Start` property.
///
/// For ASCII characters, `XID_Start` consists of `A`–`Z` and `a`–`z`.
/// Note that `_` (underscore) is not included in `XID_Start` per UAX #31.
#[must_use]
pub fn is_xid_start(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphabetic()
    } else {
        in_ranges(tables::XID_START, c as u32)
    }
}

/// Checks if a character has the Unicode `XID_Continue` property.
///
/// For ASCII characters, `XID_Continue` consists of `A`–`Z`, `a`–`z`, `0`–`9`,
/// and `_` (underscore).
#[must_use]
pub fn is_xid_continue(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        in_ranges(tables::XID_CONTINUE, c as u32)
    }
}

/// Checks if a character has the Unicode `White_Space` property.
///
/// For ASCII characters, `White_Space` consists of `' '`, `'\t'`, `'\n'`,
/// `'\r'`, `'\x0B'` (vertical tab), and `'\x0C'` (form feed).
#[must_use]
pub fn is_white_space(c: char) -> bool {
    if c.is_ascii() {
        matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0B' | '\x0C')
    } else {
        in_ranges(tables::WHITE_SPACE, c as u32)
    }
}

fn in_ranges(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(start, end)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}
