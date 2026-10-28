//! The values of literals (spec §2.7).
//!
//! The lexer has already reported malformed literals, so these functions return `None`
//! instead of a diagnostic when a literal cannot be decoded.

use crate::{Interpolation, Literal, StringLit, StringPart};
use loom_syntax::SyntaxKind;

/// The value of a literal other than an interpolated string.
#[derive(Clone, Debug, PartialEq)]
pub enum LiteralValue {
    /// An integer literal, before its type is known.
    Int(u128),
    /// A float literal.
    Float(f64),
    /// A character literal.
    Char(char),
    /// A raw string literal.
    String(String),
    /// A byte string literal.
    Bytes(Vec<u8>),
    /// `true` or `false`.
    Bool(bool),
}

/// A decoded part of a string literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringValuePart {
    /// Text with escapes processed.
    Text(String),
    /// `#{expr}`.
    Interpolation(Interpolation),
}

impl Literal {
    /// The literal's value, or `None` if it is malformed.
    #[must_use]
    pub fn value(&self) -> Option<LiteralValue> {
        token_value(&self.token()?)
    }
}

impl crate::LiteralPat {
    /// The value of the literal in the pattern (without its leading `-`), or `None` if it
    /// is malformed or a string with interpolation.
    #[must_use]
    pub fn value(&self) -> Option<LiteralValue> {
        use crate::AstNode;
        for element in self.syntax().children_with_tokens() {
            match element {
                loom_syntax::SyntaxElement::Token(t) => {
                    if let Some(value) = token_value(&t) {
                        return Some(value);
                    }
                }
                loom_syntax::SyntaxElement::Node(n) => {
                    if let Some(lit) = Literal::cast(n.clone()) {
                        return lit.value();
                    }
                    if let Some(s) = StringLit::cast(n) {
                        let mut text = String::new();
                        for part in s.value_parts() {
                            match part {
                                StringValuePart::Text(t) => text.push_str(&t),
                                StringValuePart::Interpolation(_) => return None,
                            }
                        }
                        return Some(LiteralValue::String(text));
                    }
                }
            }
        }
        None
    }
}

/// The value of a literal token.
fn token_value(token: &loom_syntax::SyntaxToken) -> Option<LiteralValue> {
    {
        let text = token.text();
        match token.kind() {
            SyntaxKind::INT_LIT => int_value(text).map(LiteralValue::Int),
            SyntaxKind::FLOAT_LIT => float_value(text).map(LiteralValue::Float),
            SyntaxKind::CHAR_LIT => char_value(text).map(LiteralValue::Char),
            SyntaxKind::RAW_STRING_LIT => raw_contents(text.strip_prefix('r')?)
                .map(|s| LiteralValue::String(normalize_newlines(s))),
            SyntaxKind::BYTE_STRING_LIT => byte_string_value(text).map(LiteralValue::Bytes),
            SyntaxKind::TRUE_KW => Some(LiteralValue::Bool(true)),
            SyntaxKind::FALSE_KW => Some(LiteralValue::Bool(false)),
            _ => None,
        }
    }
}

impl StringLit {
    /// The decoded text and interpolations, with adjacent text merged. Malformed escapes
    /// are dropped (the lexer has reported them).
    #[must_use]
    pub fn value_parts(&self) -> Vec<StringValuePart> {
        let raw: Vec<RawPart> = self
            .parts()
            .map(|part| match part {
                StringPart::Text(token) => RawPart::Text(token.text().replace("\r\n", "\n")),
                StringPart::Interpolation(i) => RawPart::Interpolation(i),
            })
            .collect();
        let raw = if self.is_triple_quoted() {
            dedent(raw)
        } else {
            raw
        };
        let mut out: Vec<StringValuePart> = Vec::new();
        for part in raw {
            match part {
                RawPart::Text(text) => {
                    let decoded = unescape(&text, false).unwrap_or_default();
                    match out.last_mut() {
                        Some(StringValuePart::Text(previous)) => previous.push_str(&decoded),
                        _ => out.push(StringValuePart::Text(decoded)),
                    }
                }
                RawPart::Interpolation(i) => out.push(StringValuePart::Interpolation(i)),
            }
        }
        out
    }
}

enum RawPart {
    Text(String),
    Interpolation(Interpolation),
}

/// Applies spec §2.7.4 rule 3 to the raw parts of a triple-quoted string.
fn dedent(mut parts: Vec<RawPart>) -> Vec<RawPart> {
    // The indentation prefix is the whitespace after the last line terminator.
    let prefix = match parts.last() {
        Some(RawPart::Text(text)) => match text.rfind('\n') {
            Some(index) if text[index + 1..].chars().all(|c| c == ' ' || c == '\t') => {
                text[index + 1..].to_owned()
            }
            _ => String::new(),
        },
        _ => String::new(),
    };
    // Remove the line terminator and the prefix before the closing delimiter.
    if let Some(RawPart::Text(text)) = parts.last_mut()
        && let Some(index) = text.rfind('\n')
    {
        text.truncate(index);
    }
    // Remove the line terminator after the opening delimiter.
    if let Some(RawPart::Text(text)) = parts.first_mut()
        && text.starts_with('\n')
    {
        text.remove(0);
    }
    let mut at_line_start = true;
    let count = parts.len();
    for (index, part) in parts.iter_mut().enumerate() {
        let RawPart::Text(text) = part else {
            at_line_start = false;
            continue;
        };
        let next_is_interpolation = index + 1 < count;
        let mut result = String::with_capacity(text.len());
        let mut rest = text.as_str();
        loop {
            let (line, terminator) = match rest.find('\n') {
                Some(i) => (&rest[..i], true),
                None => (rest, false),
            };
            if at_line_start {
                let whitespace_only = line.chars().all(|c| c == ' ' || c == '\t');
                if whitespace_only && (terminator || !next_is_interpolation) {
                    // A blank line becomes empty.
                } else {
                    result.push_str(line.strip_prefix(prefix.as_str()).unwrap_or(line));
                }
            } else {
                result.push_str(line);
            }
            if terminator {
                result.push('\n');
                rest = &rest[line.len() + 1..];
                at_line_start = true;
            } else {
                at_line_start = false;
                break;
            }
        }
        *text = result;
    }
    parts
}

/// The value of an integer literal: decimal, `0x`, `0o` or `0b`, with `_` separators.
#[must_use]
pub fn int_value(text: &str) -> Option<u128> {
    let cleaned: String = text.chars().filter(|&c| c != '_').collect();
    let (digits, radix) = if let Some(rest) = cleaned.strip_prefix("0x") {
        (rest, 16)
    } else if let Some(rest) = cleaned.strip_prefix("0o") {
        (rest, 8)
    } else if let Some(rest) = cleaned.strip_prefix("0b") {
        (rest, 2)
    } else {
        (cleaned.as_str(), 10)
    };
    if digits.is_empty() {
        return None;
    }
    u128::from_str_radix(digits, radix).ok()
}

/// The value of a float literal. A value that overflows to infinity gives `None`.
#[must_use]
pub fn float_value(text: &str) -> Option<f64> {
    let cleaned: String = text.chars().filter(|&c| c != '_').collect();
    cleaned
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn char_value(text: &str) -> Option<char> {
    let inner = text.strip_prefix('\'')?.strip_suffix('\'')?;
    let decoded = unescape(inner, false)?;
    let mut chars = decoded.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

fn byte_string_value(text: &str) -> Option<Vec<u8>> {
    let rest = text.strip_prefix('b')?;
    if let Some(raw) = rest.strip_prefix('r') {
        return raw_contents(raw).map(|s| normalize_newlines(s).into_bytes());
    }
    let inner = rest.strip_prefix('"')?.strip_suffix('"')?;
    let inner = inner.replace("\r\n", "\n");
    let mut bytes = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            if !c.is_ascii() {
                return None;
            }
            bytes.push(u8::try_from(c).ok()?);
            continue;
        }
        if chars.peek() == Some(&'x') {
            chars.next();
            let high = chars.next()?.to_digit(16)?;
            let low = chars.next()?.to_digit(16)?;
            bytes.push(u8::try_from(high * 16 + low).ok()?);
            continue;
        }
        let mut one = String::from('\\');
        let escaped = chars.next()?;
        one.push(escaped);
        if escaped == '\n' {
            while chars.peek().is_some_and(|&c| c == ' ' || c == '\t') {
                chars.next();
            }
            continue;
        }
        if escaped == 'u' {
            for c in chars.by_ref() {
                one.push(c);
                if c == '}' {
                    break;
                }
            }
        }
        let decoded = unescape(&one, true)?;
        if !decoded.is_ascii() {
            return None;
        }
        bytes.extend(decoded.bytes());
    }
    Some(bytes)
}

/// The contents of `"…"`, `#"…"#`, `##"…"##`, … (after the `r`).
fn raw_contents(text: &str) -> Option<&str> {
    let hashes = text.len() - text.trim_start_matches('#').len();
    let inner = text.get(hashes..text.len().checked_sub(hashes)?)?;
    inner.strip_prefix('"')?.strip_suffix('"')
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Processes the escapes of spec §2.7.5. `bytes` allows `\x80`–`\xFF`.
fn unescape(text: &str, bytes: bool) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut ok = true;
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(escaped) = chars.next() else {
            ok = false;
            break;
        };
        match escaped {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            '0' => out.push('\0'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            '#' => out.push('#'),
            'x' => {
                let high = chars.next().and_then(|c| c.to_digit(16));
                let low = chars.next().and_then(|c| c.to_digit(16));
                match (high, low) {
                    (Some(high), Some(low)) if bytes || high * 16 + low <= 0x7F => {
                        out.push(char::from_u32(high * 16 + low)?);
                    }
                    _ => ok = false,
                }
            }
            'u' => {
                if chars.next() != Some('{') {
                    ok = false;
                    continue;
                }
                let mut digits = String::new();
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                    digits.push(c);
                }
                match u32::from_str_radix(&digits, 16)
                    .ok()
                    .and_then(char::from_u32)
                {
                    Some(c) if (1..=6).contains(&digits.len()) => out.push(c),
                    _ => ok = false,
                }
            }
            '\n' => {
                while chars.peek().is_some_and(|&c| c == ' ' || c == '\t') {
                    chars.next();
                }
            }
            _ => ok = false,
        }
    }
    ok.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::{LiteralValue, StringValuePart, float_value, int_value};
    use crate::{AstNode, Literal, StringLit};
    use loom_span::FileId;

    fn first<N: AstNode>(source: &str) -> N {
        let source = if source.starts_with("def ") {
            source.to_owned()
        } else {
            format!("def main\n{source}end\n")
        };
        let parse = loom_parser::parse(&source, FileId::DETACHED);
        parse
            .syntax()
            .descendants()
            .find_map(N::cast)
            .expect("node present")
    }

    fn lit(expr: &str) -> Option<LiteralValue> {
        first::<Literal>(&format!("x = {expr}\n")).value()
    }

    fn text(source: &str) -> String {
        first::<StringLit>(source)
            .value_parts()
            .into_iter()
            .map(|part| match part {
                StringValuePart::Text(t) => t,
                StringValuePart::Interpolation(_) => "<I>".to_owned(),
            })
            .collect()
    }

    #[test]
    fn integers_and_floats() {
        assert_eq!(int_value("1_000"), Some(1000));
        assert_eq!(int_value("0xff"), Some(255));
        assert_eq!(int_value("0o17"), Some(15));
        assert_eq!(int_value("0b1010"), Some(10));
        assert_eq!(int_value("340282366920938463463374607431768211456"), None);
        assert_eq!(float_value("1_0.5e1"), Some(105.0));
        assert_eq!(float_value("1e400"), None);
        assert_eq!(lit("42"), Some(LiteralValue::Int(42)));
        assert_eq!(lit("true"), Some(LiteralValue::Bool(true)));
    }

    #[test]
    fn chars_raw_and_bytes() {
        assert_eq!(lit(r"'\u{1F600}'"), Some(LiteralValue::Char('😀')));
        assert_eq!(lit(r"'\n'"), Some(LiteralValue::Char('\n')));
        assert_eq!(
            lit(r##"r#"a "b" \n"#"##),
            Some(LiteralValue::String(r#"a "b" \n"#.to_owned()))
        );
        assert_eq!(
            lit(r#"b"a\xFF""#),
            Some(LiteralValue::Bytes(vec![b'a', 0xFF]))
        );
    }

    #[test]
    fn escapes_and_continuations() {
        assert_eq!(
            text("x = \"a\\tb\\\\ \\#{c} \\u{41}\\x41\"\n"),
            "a\tb\\ #{c} AA"
        );
        assert_eq!(text("x = \"one \\\n     two\"\n"), "one two");
        assert_eq!(text("x = \"a\r\nb\"\n"), "a\nb");
        assert_eq!(text("x = \"a #{1} b\"\n"), "a <I> b");
    }

    #[test]
    fn triple_quoted_strings_are_dedented() {
        let source =
            "x = \"\"\"\n    Roses are red,\n      #{language} is new.\n\n    end\n    \"\"\"\n";
        assert_eq!(text(source), "Roses are red,\n  <I> is new.\n\nend");
        let poem = "def main\n  poem = \"\"\"\n    Roses are red,\n      #{language} is new.\n    \"\"\"\nend\n";
        assert_eq!(text(poem), "Roses are red,\n  <I> is new.");
    }
}
