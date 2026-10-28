//! Lexer unit tests. Conformance cases in `spec/conformance/02-lexical/` cover the spec
//! rule by rule; these tests pin down token boundaries and recovery.

use loom_span::FileId;
use loom_syntax::SyntaxKind;

use crate::{lex, lex_bytes};

/// Renders the significant tokens as `KIND "text"`, space separated, with `NEWLINE`
/// as `⏎` and suppressed line breaks omitted.
fn significant(source: &str) -> String {
    let lexed = lex(source, FileId::DETACHED);
    lexed
        .tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| !token.kind.is_trivia())
        .map(|(index, token)| match token.kind {
            SyntaxKind::NEWLINE => "⏎".to_owned(),
            kind => format!("{kind}:{}", lexed.text(index, source)),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders diagnostics as `CODE@start..end`.
fn errors(source: &str) -> Vec<String> {
    lex(source, FileId::DETACHED)
        .diagnostics
        .iter()
        .map(|d| format!("{}@{}", d.id, d.primary.span.range))
        .collect()
}

fn assert_lossless(source: &str) {
    let lexed = lex(source, FileId::DETACHED);
    let mut rebuilt = String::new();
    let mut expected_start = 0;
    for (index, token) in lexed.tokens.iter().enumerate() {
        assert_eq!(
            token.range.start().to_usize(),
            expected_start,
            "gap before token {index}"
        );
        assert!(!token.range.is_empty(), "empty token {index}");
        rebuilt.push_str(lexed.text(index, source));
        expected_start = token.range.end().to_usize();
    }
    assert_eq!(rebuilt, source);
}

#[test]
fn simple_function() {
    assert_eq!(
        significant("def main\n  puts \"hi\"\nend\n"),
        "DEF_KW:def IDENT:main ⏎ IDENT:puts STRING_START:\" STRING_FRAGMENT:hi STRING_END:\" ⏎ END_KW:end ⏎"
    );
}

#[test]
fn trivia_kinds() {
    #[allow(clippy::enum_glob_use, reason = "a list of kinds")]
    use SyntaxKind::*;
    let source = "#!/usr/bin/env loom\n##! module doc\n## item doc\n# plain\nx\t= 1";
    let lexed = lex(source, FileId::DETACHED);
    let kinds: Vec<_> = lexed.tokens.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [
            SHEBANG,
            LINE_BREAK,
            INNER_DOC_COMMENT,
            LINE_BREAK,
            DOC_COMMENT,
            LINE_BREAK,
            COMMENT,
            LINE_BREAK,
            IDENT,
            WHITESPACE,
            EQ,
            WHITESPACE,
            INT_LIT
        ]
    );
    assert_eq!(lexed.diagnostics, []);
}

#[test]
fn continuation_rules() {
    // Rule 1 (a): trailing operators, commas, `=`, `->`, `and` continue the line.
    for source in [
        "a +\nb", "a,\nb", "a =\nb", "a +=\nb", "a ->\nb", "a and\nb", "a not\nb", "a.\nb",
        "a ==\nb", "a ..\nb",
    ] {
        assert!(
            !significant(source).contains('⏎'),
            "{source}: {}",
            significant(source)
        );
    }
    // `|` does not continue (closure parameters); neither do ordinary tokens.
    for source in ["a |\nb", "a\nb", "a)\nb", "f do\nb"] {
        assert!(significant(source).contains('⏎'), "{source}");
    }
    // Comment-only and blank lines inside a continued expression don't end it.
    assert_eq!(
        significant("x = a +\n  # note\n\n  b"),
        "IDENT:x EQ:= IDENT:a PLUS:+ IDENT:b"
    );
}

#[test]
fn leading_dot_chains() {
    assert_eq!(
        significant("users\n  .filter\n  # comment\n\n  .count\n"),
        "IDENT:users DOT:. IDENT:filter DOT:. IDENT:count ⏎"
    );
    assert_eq!(significant("a\n  .@b"), "IDENT:a DOT:. AT_IDENT:@b");
    // A line starting with `..` does not continue (spec §2.3 example).
    assert_eq!(
        significant("r = 0\n..10"),
        "IDENT:r EQ:= INT_LIT:0 ⏎ DOT2:.. INT_LIT:10"
    );
}

#[test]
fn numbers() {
    assert_eq!(
        significant("0 42 1_000 0x1F 0o17 0b1010"),
        "INT_LIT:0 INT_LIT:42 INT_LIT:1_000 INT_LIT:0x1F INT_LIT:0o17 INT_LIT:0b1010"
    );
    assert_eq!(
        significant("1.5 2e10 3.0e-2 1_0.2_5"),
        "FLOAT_LIT:1.5 FLOAT_LIT:2e10 FLOAT_LIT:3.0e-2 FLOAT_LIT:1_0.2_5"
    );
    // §2.7.2: `10.seconds` is an integer, `1..2` is a range, `t.0.1` is field access.
    assert_eq!(significant("10.seconds"), "INT_LIT:10 DOT:. IDENT:seconds");
    assert_eq!(
        significant("1..2 1..=2"),
        "INT_LIT:1 DOT2:.. INT_LIT:2 INT_LIT:1 DOT2_EQ:..= INT_LIT:2"
    );
    assert_eq!(
        significant("t.0.1"),
        "IDENT:t DOT:. INT_LIT:0 DOT:. INT_LIT:1"
    );
    assert_eq!(significant("1."), "INT_LIT:1 DOT:.");
}

#[test]
fn number_errors() {
    let cases: &[(&str, &[&str])] = &[
        ("1__0", &["E0010@0..4"]),
        ("1_", &["E0010@0..2"]),
        ("0x_1", &["E0010@0..4"]),
        ("1_.5", &["E0010@0..4"]),
        ("012", &["E0011@0..3"]),
        ("0x", &["E0012@0..2"]),
        ("0b102", &["E0012@0..5"]),
        ("0o9", &["E0012@0..3"]),
        ("0X1F", &["E0013@0..4"]),
        ("10u8", &["E0014@2..4"]),
        ("1.5x", &["E0014@3..4"]),
        ("1e", &["E0014@1..2"]),
        ("00.5", &[]),
    ];
    for (source, expected) in cases {
        assert_eq!(errors(source), *expected, "{source}");
        assert_eq!(
            lex(source, FileId::DETACHED).tokens.len(),
            1,
            "{source} is one token"
        );
    }
}

#[test]
fn strings_and_interpolation() {
    assert_eq!(
        significant(r#""a#{x + 1}b""#),
        "STRING_START:\" STRING_FRAGMENT:a INTERP_START:#{ IDENT:x PLUS:+ INT_LIT:1 INTERP_END:} STRING_FRAGMENT:b STRING_END:\""
    );
    // Nested strings and braces inside an interpolation.
    assert_eq!(
        significant(r##""#{"in #{m {}}"}""##),
        "STRING_START:\" INTERP_START:#{ STRING_START:\" STRING_FRAGMENT:in  INTERP_START:#{ IDENT:m L_BRACE:{ R_BRACE:} INTERP_END:} STRING_END:\" INTERP_END:} STRING_END:\""
    );
    // `\#{` is not an interpolation; `#` alone is text.
    assert_eq!(
        significant(r#""\#{x} # y""#),
        r#"STRING_START:" STRING_FRAGMENT:\#{x} # y STRING_END:""#
    );
    // Strings may span lines without producing NEWLINE.
    assert_eq!(
        significant("\"a\nb\""),
        "STRING_START:\" STRING_FRAGMENT:a\nb STRING_END:\""
    );
}

#[test]
fn triple_quoted_strings() {
    let source = "x = \"\"\"\n    one\n      #{two}\n    \"\"\"\n";
    assert_eq!(errors(source), Vec::<String>::new());
    assert_eq!(
        significant(source),
        "IDENT:x EQ:= STRING_START:\"\"\" STRING_FRAGMENT:\n    one\n       INTERP_START:#{ IDENT:two INTERP_END:} STRING_FRAGMENT:\n     STRING_END:\"\"\" ⏎"
    );
    assert_eq!(errors("\"\"\"abc\n\"\"\""), ["E0023@0..3"]);
    assert_eq!(errors("\"\"\"\n  abc\"\"\""), ["E0024@9..12"]);
    assert_eq!(
        errors("\"\"\"\n    ok\n  bad\n    \"\"\""),
        ["E0025@11..11"]
    );
    // Blank lines need no prefix.
    assert_eq!(errors("\"\"\"\n    a\n\n    \"\"\""), Vec::<String>::new());
}

#[test]
fn raw_and_byte_strings() {
    assert_eq!(
        significant(r##"r"a\n" r#"x"y"# b"\xFF" br"z""##),
        r##"RAW_STRING_LIT:r"a\n" RAW_STRING_LIT:r#"x"y"# BYTE_STRING_LIT:b"\xFF" BYTE_STRING_LIT:br"z""##
    );
    // `r` followed by something else is an identifier; `r#x` is `r` then a comment.
    assert_eq!(significant("r rx r#x"), "IDENT:r IDENT:rx IDENT:r");
    assert_eq!(errors("x = r#\"abc\""), ["E0026@4..5"]);
    assert_eq!(errors("b\"é\""), ["E0022@2..4"]);
    assert_eq!(errors("br\"é\""), ["E0022@3..5"]);
    assert_eq!(errors("b\"\\u{41}\""), ["E0019@2..8"]);
}

#[test]
fn escapes() {
    let ok = r#""\n\r\t\0\\\"\'\#\x41\u{1F600}""#;
    assert_eq!(errors(ok), Vec::<String>::new());
    assert_eq!(errors(r#""\q""#), ["E0019@1..3"]);
    assert_eq!(errors(r#""\u{D800}""#), ["E0020@1..9"]);
    assert_eq!(errors(r#""\u{}""#), ["E0020@1..5"]);
    assert_eq!(errors(r#""\u{1234567}""#), ["E0020@1..12"]);
    assert_eq!(errors(r#""\u{110000}""#), ["E0020@1..11"]);
    assert_eq!(errors(r#""\u41""#), ["E0020@1..3"]);
    assert_eq!(errors(r#""\x80""#), ["E0021@1..5"]);
    assert_eq!(errors(r#""\xZZ""#), ["E0021@1..3"]);
    assert_eq!(errors(r#"b"\x80""#), Vec::<String>::new());
    assert_eq!(errors("\"a\\\n   b\""), Vec::<String>::new());
}

#[test]
fn char_literals() {
    assert_eq!(
        significant(r"'a' '\n' '\u{1F600}' '中'"),
        r"CHAR_LIT:'a' CHAR_LIT:'\n' CHAR_LIT:'\u{1F600}' CHAR_LIT:'中'"
    );
    assert_eq!(errors("''"), ["E0015@0..2"]);
    assert_eq!(errors("'ab'"), ["E0016@0..4"]);
    assert_eq!(errors("x = 'a\ny"), ["E0017@4..5"]);
    assert_eq!(
        significant("x = 'a\ny"),
        "IDENT:x EQ:= CHAR_LIT:'a ⏎ IDENT:y"
    );
    assert_eq!(errors(r"'\x80'"), ["E0021@1..5"]);
}

#[test]
fn unterminated_strings() {
    assert_eq!(errors("const X: String = \"abc"), ["E0018@18..19"]);
    // Only the outermost unterminated string is reported.
    assert_eq!(errors("\"a #{\"b"), ["E0018@0..1"]);
    assert_eq!(errors("x = \"\"\"\n  abc\n"), ["E0018@4..7"]);
    assert_eq!(errors("b\"abc"), ["E0018@0..2"]);
}

#[test]
fn operators_and_punctuation() {
    assert_eq!(
        significant("a <<= b >>= c ..= d -> e => f == g != h <= i >= j"),
        "IDENT:a SHL_EQ:<<= IDENT:b SHR_EQ:>>= IDENT:c DOT2_EQ:..= IDENT:d ARROW:-> IDENT:e FAT_ARROW:=> IDENT:f EQ2:== IDENT:g NEQ:!= IDENT:h LE:<= IDENT:i GE:>= IDENT:j"
    );
    assert_eq!(
        significant("@[test] @name @ x?"),
        "AT_BRACKET:@[ IDENT:test R_BRACKET:] AT_IDENT:@name AT:@ IDENT:x QUESTION:?"
    );
    // `||` after `{` or `do` is two pipes (an empty closure parameter list).
    assert_eq!(
        significant("{ || 42 }"),
        "L_BRACE:{ PIPE:| PIPE:| INT_LIT:42 R_BRACE:}"
    );
    assert_eq!(
        significant("do || x end"),
        "DO_KW:do PIPE:| PIPE:| IDENT:x END_KW:end"
    );
}

#[test]
fn foreign_operators_are_reported_but_tokenized() {
    assert_eq!(significant("a; b"), "IDENT:a SEMICOLON:; IDENT:b");
    assert_eq!(errors("a; b"), ["E0006@1..2"]);
    assert_eq!(errors("!a"), ["E0007@0..1"]);
    assert_eq!(errors("a && b || c"), ["E0008@2..4", "E0008@7..9"]);
    assert_eq!(significant("a || b"), "IDENT:a PIPE2:|| IDENT:b");
}

#[test]
fn keywords_identifiers_and_reserved_words() {
    assert_eq!(
        significant("Self self default größe _x"),
        "SELF_TYPE_KW:Self SELF_KW:self IDENT:default IDENT:größe IDENT:_x"
    );
    assert_eq!(errors("yield = 1"), ["E0009@0..5"]);
    assert_eq!(significant("yield"), "IDENT:yield");
}

#[test]
fn source_text_errors() {
    assert_eq!(errors("x = 1 $"), ["E0005@6..7"]);
    assert_eq!(errors("x\u{00A0}= 1"), ["E0004@1..3"]);
    assert_eq!(errors("a\rb"), ["E0003@1..2"]);
    assert_eq!(significant("a\r\nb"), "IDENT:a ⏎ IDENT:b");
    // Bidi controls are reported everywhere, including comments and strings.
    assert_eq!(
        errors("# \u{202E}\n\"\u{2066}\""),
        ["E0002@2..5", "E0002@7..10"]
    );
    assert_eq!(errors("#![attr]\nx"), ["E0027@0..1"]);
    // A byte order mark is ignored.
    assert_eq!(significant("\u{FEFF}x"), "IDENT:x");
}

#[test]
fn invalid_utf8() {
    let (text, lexed) = lex_bytes(b"x = 1\n# \xFF\ny = \xC0", FileId::DETACHED);
    assert!(text.contains('\u{FFFD}'));
    let codes: Vec<String> = lexed
        .diagnostics
        .iter()
        .map(|d| format!("{}@{}", d.id, d.primary.span.range))
        .collect();
    assert_eq!(codes, ["E0001@8..9"]);
}

#[test]
fn every_example_lexes_cleanly_and_losslessly() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut count = 0;
    for entry in std::fs::read_dir(examples).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "loom") {
            let source = std::fs::read_to_string(&path).unwrap();
            assert_lossless(&source);
            assert_eq!(errors(&source), Vec::<String>::new(), "{}", path.display());
            count += 1;
        }
    }
    assert!(count >= 25);
}

#[test]
fn arbitrary_inputs_are_lossless() {
    let samples = [
        "",
        "\"",
        "\"\"\"",
        "'",
        "#{",
        "\"#{",
        "\"#{}",
        "}",
        "0x",
        "@",
        "r#",
        "br#\"",
        "\\",
        "\"\\u{",
        "'\\",
        "\r",
        "\u{FEFF}",
        "a\u{202E}b",
        "\"#{\"#{\"",
        "1e+",
        "..=..",
    ];
    for sample in samples {
        assert_lossless(sample);
    }
    // A cheap deterministic pseudo-random sweep over a small alphabet.
    let alphabet: Vec<char> = "ab0_ .#{}\"'\\\n\r@|&!=xr$é\u{202E}".chars().collect();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..2000 {
        let mut source = String::new();
        for _ in 0..(state % 24) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            source.push(alphabet[usize::try_from(state % alphabet.len() as u64).unwrap()]);
        }
        assert_lossless(&source);
    }
}
