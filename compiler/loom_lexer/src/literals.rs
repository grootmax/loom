//! Numeric, character, raw and byte-string literals, and escapes (spec §2.7).

use loom_diagnostics::{Code, codes};
use loom_syntax::SyntaxKind;
use loom_syntax::SyntaxKind::{BYTE_STRING_LIT, CHAR_LIT, DOT, FLOAT_LIT, INT_LIT, RAW_STRING_LIT};

use crate::{Lexer, chars};

/// Where an escape appears, which decides what it may contain (§2.7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Context {
    String,
    Char,
    Byte,
}

/// Lexes one escape sequence starting at the `\`. Returns `true` if it was a line
/// continuation (`\` followed by a line terminator).
pub(crate) fn lex_escape(lexer: &mut Lexer<'_>, context: Context) -> bool {
    let start = lexer.pos;
    lexer.pos += 1;
    let Some(character) = lexer.peek() else {
        return false;
    };
    match character {
        'n' | 'r' | 't' | '0' | '\\' | '"' | '\'' | '#' => lexer.pos += 1,
        'x' => {
            lexer.pos += 1;
            let digits: String = lexer
                .rest()
                .chars()
                .take(2)
                .take_while(char::is_ascii_hexdigit)
                .collect();
            lexer.pos += digits.len();
            if digits.len() < 2 {
                lexer.error(
                    codes::INVALID_HEX_ESCAPE,
                    start,
                    lexer.pos,
                    "`\\x` must be followed by two hexadecimal digits",
                );
            } else if context != Context::Byte
                && u8::from_str_radix(&digits, 16).unwrap_or(0) > 0x7F
            {
                lexer
                    .error(
                        codes::INVALID_HEX_ESCAPE,
                        start,
                        lexer.pos,
                        "`\\x` escapes above `\\x7F` are only allowed in byte strings",
                    )
                    .help
                    .push(format!(
                        "use `\\u{{{digits}}}` for the code point U+00{digits}"
                    ));
            }
        }
        'u' => {
            lexer.pos += 1;
            lex_unicode_escape(lexer, start, context);
        }
        '\n' | '\r' if lexer.rest().starts_with('\n') || lexer.rest().starts_with("\r\n") => {
            lexer.pos += if character == '\r' { 2 } else { 1 };
            if context == Context::Char {
                lexer.error(
                    codes::UNKNOWN_ESCAPE,
                    start,
                    start + 1,
                    "a line continuation is not allowed in a character literal",
                );
                return false;
            }
            return true;
        }
        other => {
            lexer.pos += other.len_utf8();
            let shown = if other.is_control() || other == '\r' {
                format!("U+{:04X}", u32::from(other))
            } else {
                format!("`\\{other}`")
            };
            lexer
                .error(
                    codes::UNKNOWN_ESCAPE,
                    start,
                    lexer.pos,
                    format!("unknown escape sequence {shown}"),
                )
                .help
                .push(
                    "valid escapes are \\n \\r \\t \\0 \\\\ \\\" \\' \\# \\xHH \\u{…}".to_owned(),
                );
        }
    }
    false
}

/// Lexes the part of a `\u{…}` escape after the `u`.
fn lex_unicode_escape(lexer: &mut Lexer<'_>, start: usize, context: Context) {
    if lexer.peek() != Some('{') {
        lexer.error(
            codes::INVALID_UNICODE_ESCAPE,
            start,
            lexer.pos,
            "`\\u` must be followed by `{`, one to six hexadecimal digits and `}`",
        );
        return;
    }
    lexer.pos += 1;
    let digits: String = lexer
        .rest()
        .chars()
        .take_while(char::is_ascii_hexdigit)
        .collect();
    lexer.pos += digits.len();
    let closed = lexer.peek() == Some('}');
    if closed {
        lexer.pos += 1;
    }
    if context == Context::Byte {
        lexer.error(
            codes::UNKNOWN_ESCAPE,
            start,
            lexer.pos,
            "`\\u{…}` escapes are not allowed in byte strings; use `\\xHH`",
        );
        return;
    }
    let message = if !closed {
        Some("missing `}` to close the `\\u{…}` escape".to_owned())
    } else if digits.is_empty() {
        Some("`\\u{}` needs at least one hexadecimal digit".to_owned())
    } else if digits.len() > 6 {
        Some("a `\\u{…}` escape has at most six hexadecimal digits".to_owned())
    } else {
        let value = u32::from_str_radix(&digits, 16).unwrap_or(u32::MAX);
        if (0xD800..=0xDFFF).contains(&value) {
            Some(format!(
                "U+{value:04X} is a surrogate, not a Unicode scalar value"
            ))
        } else if value > 0x0010_FFFF {
            Some(format!(
                "U+{value:X} is beyond the last Unicode code point U+10FFFF"
            ))
        } else {
            None
        }
    };
    if let Some(message) = message {
        lexer.error(codes::INVALID_UNICODE_ESCAPE, start, lexer.pos, message);
    }
}

/// Whether underscores in a digit group are only between two digits (§2.7.1 rule 1).
fn underscores_are_valid(group: &str) -> bool {
    !group.starts_with('_') && !group.ends_with('_') && !group.contains("__")
}

impl Lexer<'_> {
    pub(crate) fn lex_number(&mut self, start: usize) {
        let rest = self.rest().as_bytes();
        if rest[0] == b'0' && matches!(rest.get(1), Some(b'x' | b'o' | b'b' | b'X' | b'O' | b'B')) {
            self.lex_radix_number(start, char::from(rest[1]));
            return;
        }
        let mut problems: Vec<(Code, usize, usize, String)> = Vec::new();

        // A literal right after `.` is always an integer, so `t.0.1` is `t . 0 . 1`
        // (§2.7.2 rule 3).
        let after_dot = self.last_significant == Some(DOT);
        let integer = self.take_digits();
        let mut groups = vec![integer];
        let mut is_float = false;
        if !after_dot
            && self.peek() == Some('.')
            && self.peek_nth(1).is_some_and(|c| c.is_ascii_digit())
        {
            self.pos += 1;
            groups.push(self.take_digits());
            is_float = true;
        }
        if !after_dot && matches!(self.peek(), Some('e' | 'E')) {
            let sign = usize::from(matches!(self.peek_nth(1), Some('+' | '-')));
            if self.peek_nth(1 + sign).is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1 + sign;
                groups.push(self.take_digits());
                is_float = true;
            }
        }
        let digits_end = self.pos;
        if self.peek().is_some_and(chars::is_ident_continue) {
            let suffix_start = self.pos;
            self.pos += self
                .rest()
                .chars()
                .take_while(|&c| chars::is_ident_continue(c))
                .map(char::len_utf8)
                .sum::<usize>();
            let suffix = &self.text[suffix_start..self.pos];
            let mut message = format!("invalid suffix `{suffix}` on a numeric literal");
            if suffix.starts_with(['e', 'E']) {
                message.push_str("; an exponent needs digits, as in `1e5`");
            }
            problems.push((
                codes::INVALID_NUMERIC_SUFFIX,
                suffix_start,
                self.pos,
                message,
            ));
        }
        if groups
            .iter()
            .any(|group| !underscores_are_valid(&self.text[group.0..group.1]))
        {
            problems.insert(0, underscore_problem(start, digits_end));
        } else if !is_float {
            let digits = &self.text[groups[0].0..groups[0].1];
            if digits.len() > 1 && digits.starts_with('0') {
                problems.insert(
                    0,
                    (
                        codes::LEADING_ZERO,
                        start,
                        digits_end,
                        "decimal literals cannot start with `0`".to_owned(),
                    ),
                );
            }
        }
        self.report_first(problems);
        self.push_literal(if is_float { FLOAT_LIT } else { INT_LIT }, start);
    }

    /// Lexes `0x…`, `0o…` or `0b…` (§2.7.1). `prefix` is the letter after the `0`.
    fn lex_radix_number(&mut self, start: usize, prefix: char) {
        let mut problems: Vec<(Code, usize, usize, String)> = Vec::new();
        let radix = match prefix.to_ascii_lowercase() {
            'x' => 16,
            'o' => 8,
            _ => 2,
        };
        self.pos += 2;
        let body_start = self.pos;
        self.pos += self
            .rest()
            .bytes()
            .take_while(|&b| b.is_ascii_alphanumeric() || b == b'_')
            .count();
        let body = &self.text[body_start..self.pos];
        let end = self.pos;
        if prefix.is_ascii_uppercase() {
            problems.push((
                codes::UPPERCASE_RADIX_PREFIX,
                start,
                end,
                format!(
                    "radix prefixes are lowercase: write `0{}`",
                    prefix.to_ascii_lowercase()
                ),
            ));
        } else if !body.bytes().any(|b| b != b'_') {
            problems.push((
                codes::INVALID_DIGITS,
                start,
                end,
                format!("`0{prefix}` must be followed by at least one digit"),
            ));
        } else if let Some(bad) = body.chars().find(|&c| c != '_' && !c.is_digit(radix)) {
            let base = match radix {
                16 => "hexadecimal",
                8 => "octal",
                _ => "binary",
            };
            problems.push((
                codes::INVALID_DIGITS,
                start,
                end,
                format!("`{bad}` is not a valid digit in a {base} literal"),
            ));
        } else if !underscores_are_valid(body) {
            problems.push(underscore_problem(start, end));
        }
        self.report_first(problems);
        self.push_literal(INT_LIT, start);
    }

    /// Consumes decimal digits and underscores, returning their range.
    fn take_digits(&mut self) -> (usize, usize) {
        let start = self.pos;
        self.pos += self
            .rest()
            .bytes()
            .take_while(|&b| b.is_ascii_digit() || b == b'_')
            .count();
        (start, self.pos)
    }

    fn report_first(&mut self, mut problems: Vec<(Code, usize, usize, String)>) {
        if !problems.is_empty() {
            let (code, start, end, message) = problems.swap_remove(0);
            let diagnostic = self.error(code, start, end, message);
            if code == codes::LEADING_ZERO {
                diagnostic
                    .help
                    .push("write `0o12` for octal or drop the leading zeros".to_owned());
            }
        }
    }

    fn push_literal(&mut self, kind: SyntaxKind, start: usize) {
        self.push(kind, start);
    }

    pub(crate) fn lex_char_literal(&mut self, start: usize) {
        self.pos += 1;
        match self.peek() {
            Some('\'') => {
                self.pos += 1;
                self.error(
                    codes::EMPTY_CHARACTER_LITERAL,
                    start,
                    self.pos,
                    "empty character literal",
                )
                .help
                .push(
                    "a character literal holds exactly one character; strings use `\"`".to_owned(),
                );
            }
            None | Some('\n') => self.unterminated_char(start),
            Some('\r') if self.rest().starts_with("\r\n") => self.unterminated_char(start),
            Some(first) => {
                if first == '\\' {
                    lex_escape(self, Context::Char);
                } else {
                    self.pos += first.len_utf8();
                }
                if self.peek() == Some('\'') {
                    self.pos += 1;
                } else {
                    let line = &self.rest()[..self.line_length_here()];
                    if let Some(close) = line.find('\'') {
                        self.pos += close + 1;
                        self.error(
                            codes::MULTI_CHARACTER_LITERAL,
                            start,
                            self.pos,
                            "character literal contains more than one character",
                        )
                        .help
                        .push("use double quotes for a string: `\"…\"`".to_owned());
                    } else {
                        self.unterminated_char(start);
                    }
                }
            }
        }
        self.push_literal(CHAR_LIT, start);
    }

    /// Consumes to the end of the line and reports E0017.
    fn unterminated_char(&mut self, start: usize) {
        self.pos += self.line_length_here();
        self.error(
            codes::UNTERMINATED_CHARACTER,
            start,
            start + 1,
            "unterminated character literal",
        )
        .primary
        .message
        .push_str("missing closing `'`");
    }

    fn line_length_here(&self) -> usize {
        let rest = self.rest();
        let end = rest.find('\n').unwrap_or(rest.len());
        if end > 0 && rest.as_bytes()[end - 1] == b'\r' {
            end - 1
        } else {
            end
        }
    }

    /// Whether a raw string starts `skip` bytes ahead: any number of `#` then `"`.
    pub(crate) fn raw_string_ahead(&self, skip: usize) -> bool {
        let rest = &self.rest().as_bytes()[skip.min(self.rest().len())..];
        let hashes = rest.iter().take_while(|&&b| b == b'#').count();
        rest.get(hashes) == Some(&b'"')
    }

    /// Lexes `r#"…"#` (or `br#"…"#` with `prefix_length` 2).
    pub(crate) fn lex_raw_string(&mut self, start: usize, prefix_length: usize, kind: SyntaxKind) {
        self.pos += prefix_length;
        let hashes = self.rest().bytes().take_while(|&b| b == b'#').count();
        self.pos += hashes + 1;
        let content_start = self.pos;
        let closing: String = std::iter::once('"')
            .chain(std::iter::repeat_n('#', hashes))
            .collect();
        let content_end = if let Some(end) = self.rest().find(&closing) {
            let content_end = self.pos + end;
            self.pos = content_end + closing.len();
            content_end
        } else {
            self.pos = self.text.len();
            self.error(
                codes::UNTERMINATED_RAW_STRING,
                start,
                start + prefix_length,
                format!("unterminated raw string; it must end with `{closing}`"),
            );
            self.pos
        };
        if kind == BYTE_STRING_LIT {
            self.report_non_ascii(content_start, content_end);
        }
        debug_assert!(matches!(kind, RAW_STRING_LIT | BYTE_STRING_LIT));
        self.push_literal(kind, start);
    }

    /// Lexes `b"…"`.
    pub(crate) fn lex_byte_string(&mut self, start: usize) {
        self.pos += 2;
        loop {
            match self.peek() {
                None => {
                    self.error(
                        codes::UNTERMINATED_STRING,
                        start,
                        start + 2,
                        "unterminated byte string literal",
                    );
                    break;
                }
                Some('"') => {
                    self.pos += 1;
                    break;
                }
                Some('\\') => {
                    lex_escape(self, Context::Byte);
                }
                Some(character) => {
                    let at = self.pos;
                    self.pos += character.len_utf8();
                    self.report_non_ascii(at, self.pos);
                }
            }
        }
        self.push_literal(BYTE_STRING_LIT, start);
    }

    fn report_non_ascii(&mut self, start: usize, end: usize) {
        let text = self.text;
        for (offset, character) in text[start..end].char_indices() {
            if !character.is_ascii() && !chars::is_bidi_control(character) {
                let at = start + offset;
                self.error(
                    codes::NON_ASCII_IN_BYTE_STRING,
                    at,
                    at + character.len_utf8(),
                    format!("non-ASCII character `{character}` in a byte string"),
                )
                .help
                .push("write the bytes as `\\xHH` escapes".to_owned());
            }
        }
    }
}

fn underscore_problem(start: usize, end: usize) -> (Code, usize, usize, String) {
    (
        codes::MISPLACED_UNDERSCORE,
        start,
        end,
        "`_` in a numeric literal must be between two digits".to_owned(),
    )
}
