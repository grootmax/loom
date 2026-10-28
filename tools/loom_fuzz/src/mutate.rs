//! Mutation strategies for corpus inputs in `loom_fuzz`.

use crate::prng::Rng;

const DICTIONARY: &[&str] = &[
    // Spec §2.6 Keywords & Reserved Words
    "def", "struct", "enum", "trait", "impl", "module", "use", "const", "type", "var", "if", "then",
    "else", "elsif", "case", "when", "while", "do", "for", "in", "return", "break", "continue",
    "select", "pub", "self", "Self", "and", "or", "not", "end", "abstract", "async", "await",
    "become", "box", "catch", "defer", "do_while", "final", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield", "default", // Spec §2.8 Punctuation
    "+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>", "==", "!=", "<", "<=", ">", ">=", "..",
    "..=", "=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", "->", "=>", ":",
    "::", ".", ",", ";", "!", "&&", "||", "?", "~", "(", ")", "[", "]", "{", "}",
    // Additional requested tokens
    "\"", "\"\"\"", "#{", "}", "\\", "'", "r#\"", "b\"", "@[", "##", "\\u{", "\n", "\t", "\u{00A0}",
    "\u{202E}",
];

/// Mutates input at the byte level.
#[must_use]
pub fn mutate_bytes(input: &str, rng: &mut Rng) -> String {
    let mut bytes = input.as_bytes().to_vec();
    if bytes.is_empty() {
        bytes.push(rng.gen_byte());
        return String::from_utf8_lossy(&bytes).into_owned();
    }

    match rng.gen_range(0, 3) {
        0 => {
            // Flip a byte
            let idx = rng.gen_usize(bytes.len());
            bytes[idx] ^= rng.gen_byte().max(1);
        }
        1 => {
            // Insert a byte
            let idx = rng.gen_range(0, bytes.len());
            bytes.insert(idx, rng.gen_byte());
        }
        2 => {
            // Delete a range
            let start = rng.gen_usize(bytes.len());
            let len = rng.gen_range(1, (bytes.len() - start).min(10));
            bytes.drain(start..start + len);
        }
        _ => {
            // Duplicate a range
            let start = rng.gen_usize(bytes.len());
            let len = rng.gen_range(1, (bytes.len() - start).min(10));
            let sub = bytes[start..start + len].to_vec();
            let dest = rng.gen_range(0, bytes.len());
            bytes.splice(dest..dest, sub);
        }
    }

    String::from_utf8_lossy(&bytes).into_owned()
}

/// Mutates input at the token level using dictionary tokens.
#[must_use]
pub fn mutate_tokens(input: &str, rng: &mut Rng) -> String {
    let token = *rng.choose(DICTIONARY);
    if input.is_empty() {
        return token.to_string();
    }

    let mut chars: Vec<char> = input.chars().collect();
    match rng.gen_range(0, 2) {
        0 => {
            // Insert dictionary token
            let idx = rng.gen_range(0, chars.len());
            let mut res: String = chars[..idx].iter().collect();
            res.push_str(token);
            res.extend(chars[idx..].iter());
            res
        }
        1 => {
            // Replace range with dictionary token
            let start = rng.gen_usize(chars.len());
            let end = rng.gen_range(start, chars.len());
            let mut res: String = chars[..start].iter().collect();
            res.push_str(token);
            res.extend(chars[end..].iter());
            res
        }
        _ => {
            // Delete a character range
            let start = rng.gen_usize(chars.len());
            let end = rng.gen_range(start + 1, chars.len().min(start + 5));
            chars.drain(start..end);
            chars.into_iter().collect()
        }
    }
}

/// Mutates input at the line level.
#[must_use]
pub fn mutate_lines(input: &str, rng: &mut Rng) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    if lines.is_empty() {
        return "def f()\nend\n".to_string();
    }

    match rng.gen_range(0, 2) {
        0 => {
            // Duplicate a line
            let idx = rng.gen_usize(lines.len());
            let line = lines[idx];
            lines.insert(idx, line);
        }
        1 => {
            // Delete a line
            if lines.len() > 1 {
                let idx = rng.gen_usize(lines.len());
                lines.remove(idx);
            }
        }
        _ => {
            // Swap two lines
            if lines.len() > 1 {
                let i = rng.gen_usize(lines.len());
                let j = rng.gen_usize(lines.len());
                lines.swap(i, j);
            }
        }
    }

    lines.join("\n")
}

/// Splices parts of two corpus inputs.
#[must_use]
pub fn splice(input1: &str, input2: &str, rng: &mut Rng) -> String {
    let chars1: Vec<char> = input1.chars().collect();
    let chars2: Vec<char> = input2.chars().collect();

    let split1 = if chars1.is_empty() {
        0
    } else {
        rng.gen_usize(chars1.len())
    };
    let split2 = if chars2.is_empty() {
        0
    } else {
        rng.gen_usize(chars2.len())
    };

    let mut result: String = chars1[..split1].iter().collect();
    result.extend(chars2[split2..].iter());
    result
}

/// Wraps a random substring in nesting construct(s), potentially repeated 50+ times.
#[must_use]
pub fn nest(input: &str, rng: &mut Rng) -> String {
    let chars: Vec<char> = input.chars().collect();
    if chars.is_empty() {
        return "()".to_string();
    }

    let start = rng.gen_usize(chars.len());
    let end = rng.gen_range(start, chars.len());
    let sub: String = chars[start..end].iter().collect();

    let wrappers = [
        ("(", ")"),
        ("[", "]"),
        ("do\n", "\nend"),
        ("if x then\n", "\nend"),
    ];

    let depth = if rng.gen_bool(0.2) {
        rng.gen_range(50, 70)
    } else {
        rng.gen_range(1, 10)
    };

    let mut nested = sub;
    for _ in 0..depth {
        let (open, close) = *rng.choose(&wrappers);
        nested = format!("{open}{nested}{close}");
    }

    let mut result: String = chars[..start].iter().collect();
    result.push_str(&nested);
    result.extend(chars[end..].iter());
    result
}
