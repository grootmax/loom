//! The parser state: a cursor over significant tokens that records events.

use loom_diagnostics::{Code, Diagnostic, codes};
use loom_lexer::Lexed;
use loom_span::{FileId, Span, TextRange, TextSize};
use loom_syntax::SyntaxKind::{self, AND_KW, EOF, ERROR, NEWLINE, NOT_KW, OR_KW};

/// Nesting deeper than this reports E0112 (ADR 0018 §6).
pub(crate) const MAX_DEPTH: u32 = 256;

/// What the grammar code produced, replayed by the sink to build the tree.
#[derive(Debug, Clone)]
pub(crate) enum Event {
    /// Starts a node. `forward_parent` points (relative) at a later `Start` that becomes
    /// this node's parent; it is how `precede` wraps an already-built node.
    Start {
        kind: SyntaxKind,
        forward_parent: Option<usize>,
    },
    Finish,
    /// Consumes the next significant token.
    Token,
    /// A `Start` that was abandoned or already consumed through `forward_parent`.
    Tombstone,
}

/// A significant token as the grammar sees it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lookahead {
    /// The kind the grammar matches on. Foreign operators are mapped to their Loom
    /// spelling (`;` to `NEWLINE`, `!` to `not`, `&&` to `and`, `||` to `or`), so that the
    /// parser recovers from them silently after the lexer reported them.
    pub(crate) kind: SyntaxKind,
    pub(crate) range: TextRange,
}

pub(crate) struct Parser<'t> {
    pub(crate) text: &'t str,
    file: FileId,
    tokens: Vec<Lookahead>,
    pos: usize,
    pub(crate) events: Vec<Event>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// `true` while `NEWLINE` separates statements; `false` inside brackets (§2.3 rule 2).
    newline_modes: Vec<bool>,
    pub(crate) depth: u32,
    reported_depth: bool,
    /// The lexer reported a literal that runs to the end of the file; errors about
    /// constructs left open at the end are consequences of it and are not reported.
    unterminated_literal: bool,
}

impl<'t> Parser<'t> {
    pub(crate) fn new(text: &'t str, lexed: &Lexed, file: FileId) -> Self {
        let mut tokens: Vec<Lookahead> = lexed
            .tokens
            .iter()
            .filter(|token| is_significant(token.kind))
            .map(|token| Lookahead {
                kind: match token.kind {
                    SyntaxKind::SEMICOLON => NEWLINE,
                    SyntaxKind::BANG => NOT_KW,
                    SyntaxKind::AMP2 => AND_KW,
                    SyntaxKind::PIPE2 => OR_KW,
                    kind => kind,
                },
                range: token.range,
            })
            .collect();
        let end = TextSize::try_from(text.len()).unwrap_or_default();
        tokens.push(Lookahead {
            kind: EOF,
            range: TextRange::empty(end),
        });
        Self {
            text,
            file,
            tokens,
            pos: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
            newline_modes: vec![true],
            depth: 0,
            reported_depth: false,
            unterminated_literal: lexed.diagnostics.iter().any(|diagnostic| {
                diagnostic.code().is_some_and(|code| {
                    code == codes::UNTERMINATED_STRING || code == codes::UNTERMINATED_RAW_STRING
                })
            }),
        }
    }

    // ----- lookahead ---------------------------------------------------------------------

    fn newlines_significant(&self) -> bool {
        *self.newline_modes.last().unwrap_or(&true)
    }

    /// The index of the `n`th token ahead, skipping `NEWLINE`s in bracketed contexts.
    fn index_of(&self, n: usize) -> usize {
        let skip_newlines = !self.newlines_significant();
        let mut index = self.pos;
        let mut remaining = n;
        loop {
            while skip_newlines && self.tokens[index].kind == NEWLINE {
                index += 1;
            }
            if remaining == 0 || self.tokens[index].kind == EOF {
                return index;
            }
            remaining -= 1;
            index += 1;
        }
    }

    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        self.tokens[self.index_of(n)].kind
    }

    pub(crate) fn nth_range(&self, n: usize) -> TextRange {
        self.tokens[self.index_of(n)].range
    }

    /// The `n`th token ahead, including `NEWLINE`s even in bracketed contexts.
    pub(crate) fn nth_raw(&self, n: usize) -> SyntaxKind {
        self.tokens[(self.pos + n).min(self.tokens.len() - 1)].kind
    }

    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn current_range(&self) -> TextRange {
        self.nth_range(0)
    }

    pub(crate) fn current_text(&self) -> &'t str {
        &self.text[self.current_range().to_usize()]
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(crate) fn at_any(&self, kinds: &[SyntaxKind]) -> bool {
        kinds.contains(&self.current())
    }

    /// Whether the current token is the contextual keyword `word` (an `IDENT`).
    pub(crate) fn at_contextual(&self, word: &str) -> bool {
        self.at(SyntaxKind::IDENT) && self.current_text() == word
    }

    /// Whether there is trivia (whitespace or comments) between token `n` and `n + 1`.
    pub(crate) fn space_after(&self, n: usize) -> bool {
        self.nth_range(n).end() < self.nth_range(n + 1).start()
    }

    /// Whether token `n` is the first token on its line.
    pub(crate) fn first_on_line(&self, n: usize) -> bool {
        let index = self.index_of(n);
        let start = self.tokens[index].range.start().to_usize();
        let previous_end = index
            .checked_sub(1)
            .map_or(0, |previous| self.tokens[previous].range.end().to_usize());
        self.text[previous_end.min(start)..start].contains('\n')
    }

    /// Whether token `n` starts in column 1.
    pub(crate) fn nth_at_line_start(&self, n: usize) -> bool {
        let start = self.nth_range(n).start().to_usize();
        start == 0 || self.text.as_bytes()[start - 1] == b'\n'
    }

    // ----- consuming ---------------------------------------------------------------------

    /// Consumes the current token (and any `NEWLINE`s skipped before it).
    pub(crate) fn bump(&mut self) {
        let target = self.index_of(0);
        if self.tokens[target].kind == EOF {
            return;
        }
        while self.pos <= target {
            self.events.push(Event::Token);
            self.pos += 1;
        }
    }

    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes `kind` or reports E0100 without consuming anything.
    pub(crate) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.error_expected(&kind.describe());
        false
    }

    /// Consumes consecutive `NEWLINE`s.
    pub(crate) fn eat_newlines(&mut self) {
        while self.nth_raw(0) == NEWLINE {
            self.events.push(Event::Token);
            self.pos += 1;
        }
    }

    // ----- newline contexts ----------------------------------------------------------------

    pub(crate) fn push_newlines(&mut self, significant: bool) {
        self.newline_modes.push(significant);
    }

    pub(crate) fn pop_newlines(&mut self) {
        self.newline_modes.pop();
    }

    // ----- nodes -------------------------------------------------------------------------

    pub(crate) fn start(&mut self) -> Marker {
        let position = self.events.len();
        self.events.push(Event::Start {
            kind: ERROR,
            forward_parent: None,
        });
        Marker { position }
    }

    /// Wraps the current token in an `ERROR` node.
    pub(crate) fn bump_error(&mut self) {
        let marker = self.start();
        self.bump();
        marker.complete(self, ERROR);
    }

    // ----- diagnostics -------------------------------------------------------------------

    pub(crate) fn span(&self, range: TextRange) -> Span {
        Span::new(self.file, range)
    }

    pub(crate) fn error_at(&mut self, code: Code, range: TextRange, message: impl Into<String>) {
        let diagnostic = Diagnostic::error(code, self.span(range), message);
        self.push_diagnostic(diagnostic);
    }

    pub(crate) fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        if self.unterminated_literal
            && self.at(EOF)
            && diagnostic.code().is_some_and(|code| {
                code == codes::MISSING_END
                    || code == codes::UNCLOSED_DELIMITER
                    || code == codes::EXPECTED_TOKEN
            })
        {
            return;
        }
        // One error per position is enough; later ones are usually consequences.
        let start = diagnostic.primary.span.range.start();
        if self
            .diagnostics
            .iter()
            .any(|existing| existing.is_error() && existing.primary.span.range.start() == start)
        {
            return;
        }
        self.diagnostics.push(diagnostic);
    }

    /// Reports E0100 "expected `what`, found …" at the current token.
    pub(crate) fn error_expected(&mut self, what: &str) {
        let found = self.current().describe();
        let range = self.current_range();
        let diagnostic = Diagnostic::error(
            codes::EXPECTED_TOKEN,
            self.span(range),
            format!("expected {what}, found {found}"),
        )
        .with_label(format!("expected {what}"));
        self.push_diagnostic(diagnostic);
    }

    /// Enters one level of nesting. Returns `false` (after reporting E0112 once and
    /// skipping the deeply nested region) if the limit is reached.
    pub(crate) fn enter(&mut self) -> bool {
        if self.depth >= MAX_DEPTH {
            if !self.reported_depth {
                self.reported_depth = true;
                let range = self.current_range();
                self.error_at(codes::NESTING_TOO_DEEP, range, "nesting too deep");
            }
            self.skip_nested_region();
            return false;
        }
        self.depth += 1;
        true
    }

    pub(crate) fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Skips tokens up to the end of the current bracketed region or line.
    fn skip_nested_region(&mut self) {
        let marker = self.start();
        let mut balance = 0usize;
        loop {
            match self.current() {
                EOF => break,
                NEWLINE if balance == 0 => break,
                SyntaxKind::L_PAREN | SyntaxKind::L_BRACKET | SyntaxKind::L_BRACE => balance += 1,
                SyntaxKind::R_PAREN | SyntaxKind::R_BRACKET | SyntaxKind::R_BRACE => {
                    if balance == 0 {
                        break;
                    }
                    balance -= 1;
                }
                _ => {}
            }
            self.bump();
        }
        marker.complete(self, ERROR);
    }
}

/// Whether the parser sees a token at all. Trivia and error tokens (already reported by
/// the lexer) are attached to the tree by the sink but never matched by the grammar.
pub(crate) fn is_significant(kind: SyntaxKind) -> bool {
    !kind.is_trivia() && kind != SyntaxKind::ERROR_TOKEN
}

/// An unfinished node.
#[must_use = "a marker must be completed or abandoned"]
pub(crate) struct Marker {
    position: usize,
}

impl Marker {
    pub(crate) fn complete(self, parser: &mut Parser<'_>, kind: SyntaxKind) -> CompletedMarker {
        if let Event::Start { kind: slot, .. } = &mut parser.events[self.position] {
            *slot = kind;
        }
        parser.events.push(Event::Finish);
        CompletedMarker {
            position: self.position,
            kind,
        }
    }

    pub(crate) fn abandon(self, parser: &mut Parser<'_>) {
        if self.position == parser.events.len() - 1 {
            parser.events.pop();
        } else {
            parser.events[self.position] = Event::Tombstone;
        }
    }
}

/// A finished node, which can still be wrapped by a new parent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompletedMarker {
    position: usize,
    kind: SyntaxKind,
}

impl CompletedMarker {
    pub(crate) fn kind(self) -> SyntaxKind {
        self.kind
    }

    /// Starts a new node that will contain this one.
    pub(crate) fn precede(self, parser: &mut Parser<'_>) -> Marker {
        let marker = parser.start();
        if let Event::Start { forward_parent, .. } = &mut parser.events[self.position] {
            *forward_parent = Some(marker.position - self.position);
        }
        marker
    }
}
