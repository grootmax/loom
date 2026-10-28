//! Hand-written recursive-descent parser with error recovery, producing the CST.
//!
//! The parser follows [ADR 0018] §6: grammar functions emit events over the significant
//! tokens, and a sink builds the lossless green tree, attaching trivia. It never stops at
//! the first error and never panics: every input produces a tree whose text equals the
//! input.
//!
//! [ADR 0018]: https://github.com/grootmax/loom/blob/main/docs/adr/0018-front-end-design.md

mod grammar;
mod parser;
mod sink;
mod validation;

use loom_diagnostics::Diagnostic;
use loom_span::FileId;
use loom_syntax::{GreenNode, SyntaxNode};

/// The result of parsing a file: a lossless tree and every lexical and syntax diagnostic.
#[derive(Clone, Debug)]
pub struct Parse {
    green: GreenNode,
    diagnostics: Vec<Diagnostic>,
}

impl Parse {
    /// The root of the tree (a `SOURCE_FILE` node).
    #[must_use]
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// The green tree.
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Lexical errors, syntax errors and syntax-level warnings, in source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Whether any diagnostic is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// Lexes and parses `text`, which belongs to `file`.
#[must_use]
pub fn parse(text: &str, file: FileId) -> Parse {
    let lexed = loom_lexer::lex(text, file);
    parse_lexed(text, &lexed, file)
}

/// Parses raw bytes, reporting invalid UTF-8 (E0001) and continuing on the lossy text.
/// Returns the text the tree was built from.
#[must_use]
pub fn parse_bytes(bytes: &[u8], file: FileId) -> (String, Parse) {
    let (text, lexed) = loom_lexer::lex_bytes(bytes, file);
    let text = text.into_owned();
    let parse = parse_lexed(&text, &lexed, file);
    (text, parse)
}

fn parse_lexed(text: &str, lexed: &loom_lexer::Lexed, file: FileId) -> Parse {
    let mut parser = parser::Parser::new(text, lexed, file);
    grammar::source_file(&mut parser);
    let events = std::mem::take(&mut parser.events);
    let mut diagnostics = lexed.diagnostics.clone();
    diagnostics.append(&mut parser.diagnostics);
    let green = sink::build(text, lexed, events);
    let root = SyntaxNode::new_root(green.clone());
    diagnostics.extend(validation::validate(&root, file));
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.span.range.start());
    Parse { green, diagnostics }
}

#[cfg(test)]
mod tests;
