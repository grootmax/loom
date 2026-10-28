//! The registry of error codes and lints.
//!
//! This module mirrors spec Appendix C, which is normative. A test in this crate checks
//! that the two list exactly the same codes ([ADR 0018] §3).
//!
//! [ADR 0018]: https://github.com/grootmax/loom/blob/main/docs/adr/0018-front-end-design.md

use std::fmt;

/// A stable error code such as `E0007`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code {
    /// The code, `E` followed by four digits.
    pub id: &'static str,
    /// A short description, as listed in spec Appendix C.
    pub title: &'static str,
}

impl fmt::Display for Code {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.id)
    }
}

/// A named warning, such as `dangling_doc_comment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lint {
    /// The lint's `snake_case` name.
    pub name: &'static str,
    /// A short description, as listed in spec Appendix C.
    pub title: &'static str,
}

impl fmt::Display for Lint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name)
    }
}

macro_rules! codes {
    ($($name:ident = $id:literal, $title:literal;)*) => {
        $(
            #[doc = $title]
            pub const $name: Code = Code { id: $id, title: $title };
        )*

        /// Every error code, in numeric order.
        pub const ALL: &[Code] = &[$($name),*];
    };
}

codes! {
    // Lexical errors (spec Appendix C.1).
    INVALID_UTF8 = "E0001", "Source file is not valid UTF-8";
    BIDI_CHARACTER = "E0002", "Bidirectional formatting character";
    LONE_CARRIAGE_RETURN = "E0003", "Carriage return not followed by line feed";
    NON_ASCII_WHITESPACE = "E0004", "Non-ASCII whitespace character";
    UNEXPECTED_CHARACTER = "E0005", "Unexpected character";
    SEMICOLON = "E0006", "`;` is not used in Loom";
    BANG_OPERATOR = "E0007", "`!` is not an operator";
    DOUBLE_AMPERSAND_OR_PIPE = "E0008", "`&&` or `||` is not an operator";
    RESERVED_WORD = "E0009", "Reserved word used as an identifier";
    MISPLACED_UNDERSCORE = "E0010", "Misplaced `_` in a numeric literal";
    LEADING_ZERO = "E0011", "Decimal literal with a leading zero";
    INVALID_DIGITS = "E0012", "Invalid or missing digits in a numeric literal";
    UPPERCASE_RADIX_PREFIX = "E0013", "Uppercase radix prefix";
    INVALID_NUMERIC_SUFFIX = "E0014", "Invalid suffix on a numeric literal";
    EMPTY_CHARACTER_LITERAL = "E0015", "Empty character literal";
    MULTI_CHARACTER_LITERAL = "E0016", "Character literal with more than one character";
    UNTERMINATED_CHARACTER = "E0017", "Unterminated character literal";
    UNTERMINATED_STRING = "E0018", "Unterminated string literal";
    UNKNOWN_ESCAPE = "E0019", "Unknown escape sequence";
    INVALID_UNICODE_ESCAPE = "E0020", "Invalid `\\u{…}` escape";
    INVALID_HEX_ESCAPE = "E0021", "Invalid `\\x` escape";
    NON_ASCII_IN_BYTE_STRING = "E0022", "Non-ASCII character in a byte string";
    TRIPLE_QUOTE_OPENING = "E0023", "Opening `\"\"\"` not followed by a line break";
    TRIPLE_QUOTE_CLOSING = "E0024", "Closing `\"\"\"` not on its own line";
    TRIPLE_QUOTE_INDENTATION = "E0025", "Line in a triple-quoted string lacks the indentation prefix";
    UNTERMINATED_RAW_STRING = "E0026", "Unterminated raw string";
    RESERVED_SHEBANG = "E0027", "`#![` at the start of a file is reserved";

    // Syntax errors (spec Appendix C.2).
    EXPECTED_TOKEN = "E0100", "Expected a different token (general syntax error)";
    UNCLOSED_DELIMITER = "E0101", "Unclosed delimiter `(`, `[`, `{` or `#{`";
    MISSING_END = "E0102", "Missing `end`";
    UNEXPECTED_CLOSING = "E0103", "Unexpected closing delimiter or `end`";
    CHAINED_COMPARISON = "E0104", "Comparison operators cannot be chained";
    CHAINED_RANGE = "E0105", "Range operators cannot be chained";
    ASSIGNMENT_IN_EXPRESSION = "E0106", "Assignment is only allowed as a statement";
    POSITIONAL_AFTER_NAMED = "E0107", "Positional argument after a named argument";
    MISPLACED_INNER_DOC = "E0108", "Inner doc comment (`##!`) after an item";
    INHERENT_IMPL = "E0109", "`impl` without a trait; inherent methods belong in the type's body";
    ELSE_IF = "E0110", "`else if` is written `elsif`";
    STATEMENTS_ON_ONE_LINE = "E0111", "Two statements on one line";
    NESTING_TOO_DEEP = "E0112", "Nesting too deep";
    GLOB_IMPORT = "E0113", "Glob imports are not supported";
}

/// Outer doc comment not followed by an item, field, variant or method (spec §2.4.2).
pub const DANGLING_DOC_COMMENT: Lint = Lint {
    name: "dangling_doc_comment",
    title: "Outer doc comment not followed by an item, field, variant or method",
};

/// Every lint, in alphabetical order.
pub const ALL_LINTS: &[Lint] = &[DANGLING_DOC_COMMENT];

/// Looks up an error code by its id, such as `"E0007"`.
#[must_use]
pub fn lookup(id: &str) -> Option<Code> {
    ALL.iter().copied().find(|code| code.id == id)
}

#[cfg(test)]
mod tests {
    use super::{ALL, ALL_LINTS, lookup};
    use std::collections::BTreeSet;

    /// Reads the code ids listed in the tables of spec Appendix C.
    fn spec_codes() -> BTreeSet<String> {
        let spec = include_str!("../../../spec/loom-spec.md");
        let appendix = spec
            .split("## Appendix C.")
            .nth(1)
            .expect("spec has Appendix C");
        appendix
            .lines()
            .filter_map(|line| line.strip_prefix("| E"))
            .map(|rest| format!("E{}", &rest[..4]))
            .collect()
    }

    fn spec_lints() -> BTreeSet<String> {
        let spec = include_str!("../../../spec/loom-spec.md");
        let warnings = spec.split("### C.3 Warnings").nth(1).expect("spec has C.3");
        warnings
            .lines()
            .filter_map(|line| line.strip_prefix("| `"))
            .filter_map(|rest| rest.split('`').next())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn registry_matches_spec_appendix_c() {
        let ours: BTreeSet<String> = ALL.iter().map(|code| code.id.to_owned()).collect();
        assert_eq!(ours, spec_codes());
        let lints: BTreeSet<String> = ALL_LINTS.iter().map(|lint| lint.name.to_owned()).collect();
        assert_eq!(lints, spec_lints());
    }

    #[test]
    fn codes_are_well_formed_sorted_and_unique() {
        for code in ALL {
            assert_eq!(code.id.len(), 5, "{code}");
            assert!(code.id.starts_with('E'), "{code}");
            assert!(
                code.id[1..].bytes().all(|byte| byte.is_ascii_digit()),
                "{code}"
            );
        }
        assert!(ALL.windows(2).all(|pair| pair[0].id < pair[1].id));
    }

    #[test]
    fn lookup_finds_codes() {
        assert_eq!(
            lookup("E0007").map(|code| code.title),
            Some("`!` is not an operator")
        );
        assert_eq!(lookup("E9999"), None);
    }
}
