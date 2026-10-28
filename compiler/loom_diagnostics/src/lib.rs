//! Diagnostics engine: stable error codes, labeled spans, suggestions and rendering.
//!
//! A [`Diagnostic`] is plain data. Stages of the compiler create diagnostics and the
//! driver renders them. The data model is described in [ADR 0018] §3, and the registry of
//! codes in [`codes`] mirrors spec Appendix C.
//!
//! [ADR 0018]: https://github.com/grootmax/loom/blob/main/docs/adr/0018-front-end-design.md

pub mod codes;

use std::fmt::{self, Write as _};

use loom_span::{SourceMap, Span};

pub use codes::{Code, Lint};

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The program is rejected.
    Error,
    /// The program is accepted, but probably not what was meant.
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
        })
    }
}

/// What identifies a diagnostic: an error code or a lint name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticId {
    /// An error with a stable code.
    Code(Code),
    /// A named warning.
    Lint(Lint),
}

impl fmt::Display for DiagnosticId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code(code) => code.fmt(formatter),
            Self::Lint(lint) => lint.fmt(formatter),
        }
    }
}

/// A span with an optional message, underlined in the rendered snippet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    /// The code the label points at.
    pub span: Span,
    /// Text shown next to the underline. May be empty.
    pub message: String,
}

impl Label {
    /// Creates a label.
    #[must_use]
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

/// How confident a suggestion is, which decides whether tools apply it automatically.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Applicability {
    /// The edit is certainly what was meant and can be applied automatically.
    MachineApplicable,
    /// The edit is probably right but should be reviewed.
    MaybeIncorrect,
    /// The edit contains placeholders the user must fill in.
    HasPlaceholders,
}

/// A text replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// The text to replace. An empty range is an insertion.
    pub span: Span,
    /// The new text.
    pub replacement: String,
}

/// A proposed fix: a message and the edits that implement it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// What the fix does, for example "use `not` instead".
    pub message: String,
    /// The edits, which must not overlap.
    pub edits: Vec<Edit>,
    /// How confident the fix is.
    pub applicability: Applicability,
}

impl Suggestion {
    /// A suggestion that replaces `span` with `replacement`.
    #[must_use]
    pub fn replace(
        message: impl Into<String>,
        span: Span,
        replacement: impl Into<String>,
        applicability: Applicability,
    ) -> Self {
        Self {
            message: message.into(),
            edits: vec![Edit {
                span,
                replacement: replacement.into(),
            }],
            applicability,
        }
    }
}

/// A message about the program, pointing at the code it concerns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Error or warning.
    pub severity: Severity,
    /// The error code or lint name.
    pub id: DiagnosticId,
    /// The main message, starting lowercase, without a trailing period.
    pub message: String,
    /// The primary label: where the problem is.
    pub primary: Label,
    /// Related places, for example where a delimiter was opened.
    pub secondary: Vec<Label>,
    /// Extra facts (`= note: …`).
    pub notes: Vec<String>,
    /// Advice without an edit (`= help: …`).
    pub help: Vec<String>,
    /// Proposed fixes.
    pub suggestions: Vec<Suggestion>,
}

impl Diagnostic {
    /// Creates an error with the given code, pointing at `span`.
    #[must_use]
    pub fn error(code: Code, span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, DiagnosticId::Code(code), span, message)
    }

    /// Creates a warning for the given lint, pointing at `span`.
    #[must_use]
    pub fn warning(lint: Lint, span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, DiagnosticId::Lint(lint), span, message)
    }

    fn new(severity: Severity, id: DiagnosticId, span: Span, message: impl Into<String>) -> Self {
        Self {
            severity,
            id,
            message: message.into(),
            primary: Label::new(span, ""),
            secondary: Vec::new(),
            notes: Vec::new(),
            help: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    /// Sets the text of the primary label.
    #[must_use]
    pub fn with_label(mut self, message: impl Into<String>) -> Self {
        self.primary.message = message.into();
        self
    }

    /// Adds a secondary label.
    #[must_use]
    pub fn with_secondary(mut self, span: Span, message: impl Into<String>) -> Self {
        self.secondary.push(Label::new(span, message));
        self
    }

    /// Adds a note.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Adds a help line.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help.push(help.into());
        self
    }

    /// Adds a suggestion.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }

    /// The error code, if this is an error.
    #[must_use]
    pub fn code(&self) -> Option<Code> {
        match self.id {
            DiagnosticId::Code(code) => Some(code),
            DiagnosticId::Lint(_) => None,
        }
    }

    /// Whether this diagnostic rejects the program.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// A one-line form: `file:line:column: error[E0007]: message`.
    ///
    /// Used by the conformance runner and in tests. The full rendering with source
    /// snippets is in the renderer.
    #[must_use]
    pub fn short(&self, sources: &SourceMap) -> String {
        let mut out = String::new();
        if let Some(file) = sources.get(self.primary.span.file) {
            let position = file.line_column(self.primary.span.range.start());
            let _ = write!(out, "{}:{position}: ", file.name());
        }
        let _ = write!(out, "{}[{}]: {}", self.severity, self.id, self.message);
        out
    }
}

/// Diagnostics collected while processing one input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// An empty collection.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    /// Adds every diagnostic from `other`.
    pub fn extend(&mut self, other: impl IntoIterator<Item = Diagnostic>) {
        self.items.extend(other);
    }

    /// Whether any diagnostic is an error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(Diagnostic::is_error)
    }

    /// The number of diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are no diagnostics.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterates over the diagnostics in the order they were added.
    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.items.iter()
    }

    /// Sorts by file and position, keeping the original order for equal positions.
    pub fn sort(&mut self) {
        self.items.sort_by_key(|diagnostic| {
            (
                diagnostic.primary.span.file,
                diagnostic.primary.span.range.start(),
            )
        });
    }

    /// Returns the diagnostics as a vector.
    #[must_use]
    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.items
    }
}

impl IntoIterator for Diagnostics {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a Diagnostics {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl Extend<Diagnostic> for Diagnostics {
    fn extend<I: IntoIterator<Item = Diagnostic>>(&mut self, iter: I) {
        self.items.extend(iter);
    }
}

#[cfg(test)]
mod tests {
    use super::{Applicability, Diagnostic, Diagnostics, Suggestion, codes};
    use loom_span::{SourceMap, Span, TextRange, TextSize};

    fn span_in(map: &mut SourceMap, text: &str, start: u32, end: u32) -> Span {
        let file = map.add("test.loom", text).unwrap();
        Span::new(
            file,
            TextRange::new(TextSize::new(start), TextSize::new(end)),
        )
    }

    #[test]
    fn short_form_names_file_position_code_and_message() {
        let mut map = SourceMap::new();
        let span = span_in(&mut map, "def main\n  if !ready\n", 14, 15);
        let diagnostic = Diagnostic::error(codes::BANG_OPERATOR, span, "`!` is not an operator")
            .with_label("not an operator")
            .with_suggestion(Suggestion::replace(
                "use `not`",
                span,
                "not ",
                Applicability::MachineApplicable,
            ));
        assert_eq!(
            diagnostic.short(&map),
            "test.loom:2:6: error[E0007]: `!` is not an operator"
        );
        assert_eq!(diagnostic.code(), Some(codes::BANG_OPERATOR));
        assert!(diagnostic.is_error());
    }

    #[test]
    fn warnings_use_lint_names() {
        let mut map = SourceMap::new();
        let span = span_in(&mut map, "## doc\n", 0, 6);
        let diagnostic = Diagnostic::warning(
            codes::DANGLING_DOC_COMMENT,
            span,
            "doc comment is not followed by an item",
        );
        assert_eq!(
            diagnostic.short(&map),
            "test.loom:1:1: warning[dangling_doc_comment]: doc comment is not followed by an item"
        );
        assert_eq!(diagnostic.code(), None);
    }

    #[test]
    fn collection_sorts_by_position_and_tracks_errors() {
        let mut map = SourceMap::new();
        let late = span_in(&mut map, "abcdef", 4, 5);
        let early = Span::new(
            late.file,
            TextRange::new(TextSize::new(1), TextSize::new(2)),
        );
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::warning(codes::DANGLING_DOC_COMMENT, late, "w"));
        assert!(!diagnostics.has_errors());
        diagnostics.push(Diagnostic::error(codes::SEMICOLON, early, "e"));
        assert!(diagnostics.has_errors());
        diagnostics.sort();
        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(messages, ["e", "w"]);
    }
}
