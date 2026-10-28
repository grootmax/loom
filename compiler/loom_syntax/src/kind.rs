//! `SyntaxKind`: every token and node kind in the Loom CST.

use std::fmt;

macro_rules! syntax_kinds {
    (
        tokens { $($token:ident),* $(,)? }
        keywords { $($keyword:ident = $keyword_text:literal),* $(,)? }
        punctuation { $($punct:ident = $punct_text:literal),* $(,)? }
        nodes { $($node:ident),* $(,)? }
    ) => {
        /// The kind of a token or node.
        ///
        /// Token kinds come first, then keywords, then punctuation, then node kinds, so
        /// that the category checks below are range comparisons.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u16)]
        #[allow(non_camel_case_types, reason = "kinds are named like the spec's token classes")]
        pub enum SyntaxKind {
            $(
                #[doc = concat!("Token `", stringify!($token), "`.")]
                $token,
            )*
            $(
                #[doc = concat!("Keyword `", $keyword_text, "`.")]
                $keyword,
            )*
            $(
                #[doc = concat!("Punctuation `", $punct_text, "`.")]
                $punct,
            )*
            $(
                #[doc = concat!("Node `", stringify!($node), "`.")]
                $node,
            )*
        }

        const KEYWORDS: &[(&str, SyntaxKind)] = &[$(($keyword_text, SyntaxKind::$keyword)),*];
        const PUNCTUATION: &[(&str, SyntaxKind)] = &[$(($punct_text, SyntaxKind::$punct)),*];

        impl SyntaxKind {
            /// Every kind, in declaration order.
            pub const ALL: &[SyntaxKind] = &[
                $(SyntaxKind::$token,)* $(SyntaxKind::$keyword,)*
                $(SyntaxKind::$punct,)* $(SyntaxKind::$node,)*
            ];

            /// The kind's name, as used in CST dumps.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(SyntaxKind::$token => stringify!($token),)*
                    $(SyntaxKind::$keyword => stringify!($keyword),)*
                    $(SyntaxKind::$punct => stringify!($punct),)*
                    $(SyntaxKind::$node => stringify!($node),)*
                }
            }

            /// The fixed text of a keyword or punctuation kind.
            #[must_use]
            pub const fn fixed_text(self) -> Option<&'static str> {
                match self {
                    $(SyntaxKind::$keyword => Some($keyword_text),)*
                    $(SyntaxKind::$punct => Some($punct_text),)*
                    _ => None,
                }
            }
        }
    };
}

syntax_kinds! {
    tokens {
        // Trivia (spec §2.9).
        WHITESPACE, LINE_BREAK, COMMENT, DOC_COMMENT, INNER_DOC_COMMENT, SHEBANG,
        // Significant line terminator (spec §2.3).
        NEWLINE,
        // Text the lexer could not make sense of; always comes with a diagnostic.
        ERROR_TOKEN,
        IDENT, AT_IDENT,
        INT_LIT, FLOAT_LIT, CHAR_LIT, RAW_STRING_LIT, BYTE_STRING_LIT,
        STRING_START, STRING_FRAGMENT, INTERP_START, INTERP_END, STRING_END,
        // Not part of Loom. The lexer reports these and the parser recovers from them:
        // `;` acts as a line break, `!` as `not`, `&&` as `and`, `||` as `or`.
        SEMICOLON, BANG, AMP2, PIPE2,
        // The end of the input. Never stored in a tree.
        EOF,
    }
    keywords {
        AND_KW = "and", AS_KW = "as", BREAK_KW = "break", CASE_KW = "case",
        CONST_KW = "const", DEF_KW = "def", DO_KW = "do", DYN_KW = "dyn",
        ELSE_KW = "else", ELSIF_KW = "elsif", END_KW = "end", ENUM_KW = "enum",
        EXTERN_KW = "extern", FALSE_KW = "false", FOR_KW = "for", IF_KW = "if",
        IMPL_KW = "impl", IN_KW = "in", LOOP_KW = "loop", MODULE_KW = "module",
        MUT_KW = "mut", NEXT_KW = "next", NOT_KW = "not", OR_KW = "or",
        OWN_KW = "own", PACKAGE_KW = "package", PUB_KW = "pub", REF_KW = "ref",
        RETURN_KW = "return", SELECT_KW = "select", SELF_KW = "self", SELF_TYPE_KW = "Self",
        STRUCT_KW = "struct", SUPER_KW = "super", THEN_KW = "then", TRAIT_KW = "trait",
        TRUE_KW = "true", TYPE_KW = "type", UNSAFE_KW = "unsafe", USE_KW = "use",
        VAR_KW = "var", WHEN_KW = "when", WHERE_KW = "where", WHILE_KW = "while",
    }
    punctuation {
        PLUS = "+", MINUS = "-", STAR = "*", SLASH = "/", PERCENT = "%",
        AMP = "&", PIPE = "|", CARET = "^", TILDE = "~", SHL = "<<", SHR = ">>",
        EQ2 = "==", NEQ = "!=", LT = "<", LE = "<=", GT = ">", GE = ">=",
        EQ = "=", PLUS_EQ = "+=", MINUS_EQ = "-=", STAR_EQ = "*=", SLASH_EQ = "/=",
        PERCENT_EQ = "%=", AMP_EQ = "&=", PIPE_EQ = "|=", CARET_EQ = "^=",
        SHL_EQ = "<<=", SHR_EQ = ">>=",
        DOT2 = "..", DOT2_EQ = "..=", DOT = ".", COMMA = ",", COLON = ":",
        ARROW = "->", FAT_ARROW = "=>", QUESTION = "?", AT = "@", AT_BRACKET = "@[",
        L_PAREN = "(", R_PAREN = ")", L_BRACKET = "[", R_BRACKET = "]",
        L_BRACE = "{", R_BRACE = "}",
    }
    nodes {
        SOURCE_FILE,
        // Wraps tokens the parser could not fit into the grammar.
        ERROR,
        // Items (spec §3).
        ATTRIBUTE, ATTR_ENTRY, ATTR_ARG_LIST, ATTR_ARG, VISIBILITY,
        FN_DEF, NAME, PARAM_LIST, PARAM, RET_TYPE,
        STRUCT_DEF, FIELD_DEF, ENUM_DEF, VARIANT_DEF, VARIANT_FIELD_LIST,
        TRAIT_DEF, ASSOC_TYPE, IMPL_DEF, MODULE_DEF,
        USE_DECL, USE_TREE, USE_TREE_LIST, CONST_DEF, TYPE_ALIAS, EXTERN_BLOCK,
        GENERIC_PARAM_LIST, GENERIC_PARAM, BOUND_LIST, WHERE_CLAUSE, WHERE_PRED,
        // Paths and types (spec §5, §7.2).
        PATH, PATH_SEGMENT, NAME_REF,
        PATH_TYPE, GENERIC_ARG_LIST, TUPLE_TYPE, FN_TYPE, DYN_TYPE, REF_TYPE,
        // Statements (spec §4).
        BLOCK, BIND_STMT, ASSIGN_STMT, EXPR_STMT,
        // Expressions (spec §7).
        LITERAL, STRING, INTERPOLATION, PATH_EXPR, FIELD_REF_EXPR,
        PAREN_EXPR, TUPLE_EXPR, ARRAY_EXPR, MAP_EXPR, MAP_ENTRY,
        CLOSURE_EXPR, TRAILING_BLOCK, BLOCK_PARAM_LIST, BLOCK_PARAM,
        IF_EXPR, ELSIF_BRANCH, ELSE_BRANCH, CASE_EXPR, CASE_ARM, GUARD,
        LOOP_EXPR, WHILE_EXPR, FOR_EXPR, SELECT_EXPR, SELECT_ARM, UNSAFE_EXPR,
        RETURN_EXPR, BREAK_EXPR, NEXT_EXPR,
        BINARY_EXPR, PREFIX_EXPR, RANGE_EXPR, IN_EXPR,
        CALL_EXPR, METHOD_CALL_EXPR, FIELD_EXPR, INDEX_EXPR, TRY_EXPR,
        ARG_LIST, ARG, NAMED_ARG,
        // Patterns (spec §9).
        WILDCARD_PAT, IDENT_PAT, LITERAL_PAT, PATH_PAT, TUPLE_STRUCT_PAT, FIELD_PAT,
        REST_PAT, TUPLE_PAT, PAREN_PAT, RANGE_PAT, OR_PAT,
    }
}

impl SyntaxKind {
    /// Whether this kind is trivia: whitespace, suppressed line breaks and comments.
    #[must_use]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::WHITESPACE
                | Self::LINE_BREAK
                | Self::COMMENT
                | Self::DOC_COMMENT
                | Self::INNER_DOC_COMMENT
                | Self::SHEBANG
        )
    }

    /// Whether this kind is a strict keyword.
    #[must_use]
    pub fn is_keyword(self) -> bool {
        Self::AND_KW <= self && self <= Self::WHILE_KW
    }

    /// Whether this kind is punctuation or an operator.
    #[must_use]
    pub fn is_punctuation(self) -> bool {
        Self::PLUS <= self && self <= Self::R_BRACE
    }

    /// Whether this kind is a token (as opposed to a node).
    #[must_use]
    pub fn is_token(self) -> bool {
        self < Self::SOURCE_FILE
    }

    /// Whether this kind is a literal token.
    #[must_use]
    pub fn is_literal(self) -> bool {
        matches!(
            self,
            Self::INT_LIT
                | Self::FLOAT_LIT
                | Self::CHAR_LIT
                | Self::RAW_STRING_LIT
                | Self::BYTE_STRING_LIT
                | Self::TRUE_KW
                | Self::FALSE_KW
        )
    }

    /// Whether this kind is `=` or a compound assignment operator.
    #[must_use]
    pub fn is_assignment_operator(self) -> bool {
        Self::EQ <= self && self <= Self::SHR_EQ
    }

    /// The keyword spelled `text`, if any. Contextual keywords such as `default` are not
    /// included; they are identifiers.
    #[must_use]
    pub fn from_keyword(text: &str) -> Option<Self> {
        KEYWORDS
            .iter()
            .find(|(keyword, _)| *keyword == text)
            .map(|&(_, kind)| kind)
    }

    /// The punctuation spelled exactly `text`, if any.
    #[must_use]
    pub fn from_punctuation(text: &str) -> Option<Self> {
        PUNCTUATION
            .iter()
            .find(|(punct, _)| *punct == text)
            .map(|&(_, kind)| kind)
    }

    /// A short human description for diagnostics, such as `` `end` `` or "identifier".
    #[must_use]
    pub fn describe(self) -> String {
        if let Some(text) = self.fixed_text() {
            return format!("`{text}`");
        }
        match self {
            Self::NEWLINE => "line break",
            Self::IDENT => "identifier",
            Self::AT_IDENT => "`@field`",
            Self::INT_LIT => "integer literal",
            Self::FLOAT_LIT => "float literal",
            Self::CHAR_LIT => "character literal",
            Self::RAW_STRING_LIT | Self::STRING_START => "string literal",
            Self::BYTE_STRING_LIT => "byte string literal",
            Self::STRING_FRAGMENT => "string text",
            Self::INTERP_START => "`#{`",
            Self::INTERP_END => "`}`",
            Self::STRING_END => "end of string",
            Self::SEMICOLON => "`;`",
            Self::BANG => "`!`",
            Self::AMP2 => "`&&`",
            Self::PIPE2 => "`||`",
            Self::EOF => "end of file",
            _ => return self.name().to_ascii_lowercase().replace('_', " "),
        }
        .to_owned()
    }
}

impl fmt::Display for SyntaxKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::SyntaxKind;

    #[test]
    fn keyword_lookup_covers_spec_strict_keywords() {
        // Spec §2.6, strict keywords.
        let strict = "and as break case const def do dyn else elsif end enum extern false \
                      for if impl in loop module mut next not or own package pub ref return \
                      select self Self struct super then trait true type unsafe use var when \
                      where while";
        for word in strict.split_whitespace() {
            let kind = SyntaxKind::from_keyword(word).unwrap_or_else(|| panic!("{word}"));
            assert!(kind.is_keyword());
            assert_eq!(kind.fixed_text(), Some(word));
        }
        assert_eq!(SyntaxKind::from_keyword("default"), None);
        assert_eq!(SyntaxKind::from_keyword("puts"), None);
    }

    #[test]
    fn categories_are_disjoint() {
        for &kind in SyntaxKind::ALL {
            let categories = [kind.is_keyword(), kind.is_punctuation(), !kind.is_token()];
            assert!(
                categories.iter().filter(|&&flag| flag).count() <= 1,
                "{kind}"
            );
        }
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(!SyntaxKind::NEWLINE.is_trivia());
        assert!(SyntaxKind::SHR_EQ.is_assignment_operator());
        assert!(!SyntaxKind::EQ2.is_assignment_operator());
    }

    #[test]
    fn punctuation_round_trips() {
        for &kind in SyntaxKind::ALL.iter().filter(|kind| kind.is_punctuation()) {
            let text = kind.fixed_text().unwrap();
            assert_eq!(SyntaxKind::from_punctuation(text), Some(kind));
        }
    }

    #[test]
    fn descriptions() {
        assert_eq!(SyntaxKind::END_KW.describe(), "`end`");
        assert_eq!(SyntaxKind::IDENT.describe(), "identifier");
        assert_eq!(SyntaxKind::FN_DEF.describe(), "fn def");
    }
}
