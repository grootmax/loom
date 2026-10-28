//! Invariant checks for `lex`, `parse`, and `fmt` fuzzing targets.

use std::time::{Duration, Instant};

use loom_span::{FileId, TextSize};
use loom_syntax::SyntaxKind;

/// Error indicating an invariant failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvariantFailure {
    /// Invariant failure description.
    pub message: String,
}

impl std::fmt::Display for InvariantFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for InvariantFailure {}

/// Checks all invariants for `loom_lexer::lex`.
///
/// # Errors
///
/// Returns an error if any lexer invariant is violated.
pub fn check_lex(text: &str) -> Result<(), InvariantFailure> {
    let lexed1 = loom_lexer::lex(text, FileId::DETACHED);

    // Contiguous, non-empty, covering input
    if text.is_empty() {
        if !lexed1.tokens.is_empty()
            && lexed1.tokens
                != [loom_lexer::Token {
                    kind: SyntaxKind::WHITESPACE,
                    range: loom_span::TextRange::empty(TextSize::new(0)),
                }]
        {
            for token in &lexed1.tokens {
                if !token.range.is_empty() {
                    return Err(InvariantFailure {
                        message: "expected empty token ranges for empty input".to_string(),
                    });
                }
            }
        }
    } else {
        let mut curr = 0;
        for (i, token) in lexed1.tokens.iter().enumerate() {
            if token.range.is_empty() {
                return Err(InvariantFailure {
                    message: format!(
                        "token {i} ({:?}) has empty range {}",
                        token.kind, token.range
                    ),
                });
            }
            if token.range.start().to_usize() != curr {
                return Err(InvariantFailure {
                    message: format!(
                        "token {i} ({:?}) start {} != expected {}",
                        token.kind,
                        token.range.start(),
                        curr
                    ),
                });
            }
            curr = token.range.end().to_usize();
        }
        if curr != text.len() {
            return Err(InvariantFailure {
                message: format!("tokens end at byte {curr}, input length is {}", text.len()),
            });
        }
    }

    // Determinism
    let lexed2 = loom_lexer::lex(text, FileId::DETACHED);
    if lexed1.tokens != lexed2.tokens {
        return Err(InvariantFailure {
            message: "lexing the same input twice produced different tokens".to_string(),
        });
    }

    Ok(())
}

/// Checks all invariants for `loom_parser::parse`.
///
/// # Errors
///
/// Returns an error if any parser invariant is violated.
pub fn check_parse(text: &str) -> Result<(), InvariantFailure> {
    let start = Instant::now();
    let parse = loom_parser::parse(text, FileId::DETACHED);
    let elapsed = start.elapsed();

    if elapsed >= Duration::from_secs(2) {
        return Err(InvariantFailure {
            message: format!("slow input: parse took {elapsed:.2?}"),
        });
    }

    if parse.syntax().text() != text {
        return Err(InvariantFailure {
            message: "syntax().text() does not equal source text".to_string(),
        });
    }

    let input_len = TextSize::of(text);
    for diag in parse.diagnostics() {
        if diag.primary.span.range.end() > input_len {
            return Err(InvariantFailure {
                message: format!(
                    "primary diagnostic range {} exceeds input length {}",
                    diag.primary.span.range, input_len
                ),
            });
        }
        for label in &diag.secondary {
            if label.span.range.end() > input_len {
                return Err(InvariantFailure {
                    message: format!(
                        "secondary diagnostic range {} exceeds input length {}",
                        label.span.range, input_len
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Checks parser invariants for `loom_parser::parse_bytes`.
///
/// # Errors
///
/// Returns an error if any parser invariant is violated.
pub fn check_parse_bytes(bytes: &[u8]) -> Result<(), InvariantFailure> {
    let start = Instant::now();
    let (text, parse) = loom_parser::parse_bytes(bytes, FileId::DETACHED);
    let elapsed = start.elapsed();

    if elapsed >= Duration::from_secs(2) {
        return Err(InvariantFailure {
            message: format!("slow input: parse_bytes took {elapsed:.2?}"),
        });
    }

    if parse.syntax().text() != text {
        return Err(InvariantFailure {
            message: "syntax().text() does not equal lossy decoded text".to_string(),
        });
    }

    let text_len = TextSize::of(&text);
    for diag in parse.diagnostics() {
        if diag.primary.span.range.end() > text_len {
            return Err(InvariantFailure {
                message: format!(
                    "primary diagnostic range {} exceeds lossy text length {}",
                    diag.primary.span.range, text_len
                ),
            });
        }
        for label in &diag.secondary {
            if label.span.range.end() > text_len {
                return Err(InvariantFailure {
                    message: format!(
                        "secondary diagnostic range {} exceeds lossy text length {}",
                        label.span.range, text_len
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Checks all invariants for `loom_fmt::format_tree`.
///
/// # Errors
///
/// Returns an error if any formatter invariant is violated.
pub fn check_fmt(text: &str) -> Result<(), InvariantFailure> {
    let parse = loom_parser::parse(text, FileId::DETACHED);
    if parse.has_errors() {
        return Ok(());
    }

    let formatted = loom_fmt::format_tree(&parse.syntax());

    let reparse = loom_parser::parse(&formatted, FileId::DETACHED);
    if reparse.has_errors() {
        return Err(InvariantFailure {
            message: "formatted code produced syntax errors on re-parse".to_string(),
        });
    }

    let reformatted = loom_fmt::format_tree(&reparse.syntax());
    if reformatted != formatted {
        return Err(InvariantFailure {
            message: "formatter is not idempotent: format(format(x)) != format(x)".to_string(),
        });
    }

    let orig_tokens = extract_significant_tokens(text);
    let fmt_tokens = extract_significant_tokens(&formatted);
    if orig_tokens != fmt_tokens {
        return Err(InvariantFailure {
            message: "non-whitespace tokens changed after formatting".to_string(),
        });
    }

    Ok(())
}

fn extract_significant_tokens(text: &str) -> Vec<(SyntaxKind, String)> {
    let lexed = loom_lexer::lex(text, FileId::DETACHED);
    let mut result = Vec::new();
    for (i, token) in lexed.tokens.iter().enumerate() {
        if matches!(
            token.kind,
            SyntaxKind::WHITESPACE | SyntaxKind::LINE_BREAK | SyntaxKind::NEWLINE
        ) {
            continue;
        }
        match token.kind {
            SyntaxKind::COMMENT
            | SyntaxKind::DOC_COMMENT
            | SyntaxKind::INNER_DOC_COMMENT
            | SyntaxKind::SHEBANG => {
                let token_text = lexed.text(i, text);
                result.push((token.kind, token_text.trim_end().to_string()));
            }
            _ => {
                let token_text = lexed.text(i, text);
                result.push((token.kind, token_text.to_string()));
            }
        }
    }
    result
}
