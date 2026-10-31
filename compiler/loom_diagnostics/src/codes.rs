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

    // Name and module errors (spec Appendix C.4).
    UNRESOLVED_NAME = "E0200", "Unresolved name";
    UNKNOWN_VARIANT = "E0201", "Unknown variant or unqualified variant without an enum type";
    DUPLICATE_DEFINITION = "E0202", "Name declared twice in one namespace";
    ASSIGN_TO_IMMUTABLE = "E0203", "Assignment to an immutable binding";
    BINDING_PATTERN_TEST = "E0204", "Pattern test binds names outside a condition";
    PRIVATE_ITEM = "E0205", "Item, field or method is not visible here";
    UNRESOLVED_IMPORT = "E0206", "Unresolved import";
    SELF_OUTSIDE_METHOD = "E0207", "`self` or `@field` outside a method";
    BREAK_OUTSIDE_LOOP = "E0208", "`break` or `next` outside a loop";
    WRONG_NAME_KIND = "E0209", "Name of the wrong kind (a type where a value is expected, or the reverse)";
    BINDING_REDECLARED = "E0210", "Binding declared again in the same function";
    INVALID_MAIN = "E0211", "Missing or invalid `main` function";
    INVALID_INTRINSIC = "E0212", "Intrinsic declared outside the core library, or unknown intrinsic";
    UNKNOWN_ATTRIBUTE = "E0213", "Unknown attribute";
    TYPE_ANNOTATIONS_NEEDED = "E0282", "Type annotations needed";

    // Type, trait and pattern errors (spec Appendix C.5).
    MISMATCHED_TYPES = "E0300", "Mismatched types";
    WRONG_ARGUMENT_COUNT = "E0301", "Wrong number of arguments, or a missing argument";
    REF_OUTSIDE_RETURN = "E0302", "`ref` type outside a function's return type";
    WRONG_GENERIC_COUNT = "E0303", "Wrong number of generic arguments";
    UNKNOWN_FIELD = "E0304", "Unknown field";
    BAD_CONSTRUCTION_FIELDS = "E0305", "Missing or repeated field in a construction";
    NO_SUCH_METHOD = "E0306", "No method with this name for this type";
    BAD_NAMED_ARGUMENT = "E0307", "Unknown or repeated named argument";
    NOT_CALLABLE = "E0308", "Value is not callable";
    NOT_INDEXABLE = "E0309", "Type cannot be indexed";
    LITERAL_OUT_OF_RANGE = "E0310", "Literal out of range for its type";
    UNSUPPORTED_OPERATOR = "E0311", "Operator not supported for these operand types";
    MUTATE_IMMUTABLE = "E0312", "Cannot mutate an immutable place";
    INVALID_TRY = "E0313", "`?` used on a value or in a function that does not support it";
    BREAK_VALUE_OUTSIDE_LOOP = "E0314", "`break` with a value outside `loop`";
    UNSATISFIED_BOUND = "E0320", "Trait bound not satisfied";
    MISSING_IMPL_ITEM = "E0321", "Missing item in an implementation";
    AMBIGUOUS_METHOD = "E0322", "Ambiguous method";
    NOT_A_TRAIT_MEMBER = "E0323", "Item is not a member of the trait";
    IMPL_SIGNATURE_MISMATCH = "E0324", "Method does not match the trait's declaration";
    EXPECTED_TRAIT = "E0325", "Expected a trait";
    CONFLICTING_IMPLS = "E0326", "Conflicting implementations";
    ORPHAN_IMPL = "E0327", "Implementation violates the orphan rule";
    NOT_OBJECT_SAFE = "E0328", "Trait is not object-safe";
    INVALID_DERIVE = "E0329", "Trait cannot be derived for this type";
    UNKNOWN_ASSOCIATED_ITEM = "E0330", "Unknown associated item";
    NOT_IMPLEMENTED_YET = "E0398", "Feature not implemented yet in this phase";
    NON_EXHAUSTIVE_CASE = "E0400", "Non-exhaustive `case`";
    REFUTABLE_PATTERN = "E0401", "Refutable pattern where an irrefutable one is required";
    OR_PATTERN_BINDINGS = "E0402", "Or-pattern alternatives bind different names";
    DUPLICATE_PATTERN_BINDING = "E0403", "Name bound twice in one pattern";
    PATTERN_TYPE_MISMATCH = "E0404", "Pattern does not fit the matched type";
    INVALID_RANGE_PATTERN = "E0405", "Invalid range pattern";

    // Ownership errors (spec Appendix C.6).
    USE_AFTER_MOVE = "E0500", "Use of a moved value";
    MOVE_OUT_OF_BORROW = "E0501", "Move out of a borrowed place";
    BORROW_CONFLICT = "E0502", "Conflicting borrow, or use of a borrowed place";
    BORROW_ESCAPES = "E0503", "Borrow escapes: stored, moved to `own`, or captured by an escaping closure";
    REF_RETURN_WITHOUT_SOURCE = "E0504", "`ref` return type without an elided source";

    // Concurrency errors (spec Appendix C.7).
    SELECT_ARM = "E0603", "A `select` arm is not a channel operation or timer";
}

/// Outer doc comment not followed by an item, field, variant or method (spec §2.4.2).
pub const DANGLING_DOC_COMMENT: Lint = Lint {
    name: "dangling_doc_comment",
    title: "Outer doc comment not followed by an item, field, variant or method",
};

/// `case` arm that can never match (spec §9.4).
pub const UNREACHABLE_PATTERN: Lint = Lint {
    name: "unreachable_pattern",
    title: "`case` arm that can never match",
};

/// Unused value of type `Result` (spec §10.6).
pub const UNUSED_RESULT: Lint = Lint {
    name: "unused_result",
    title: "Unused value of type `Result`",
};

/// Every lint, in alphabetical order.
pub const ALL_LINTS: &[Lint] = &[DANGLING_DOC_COMMENT, UNREACHABLE_PATTERN, UNUSED_RESULT];

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
