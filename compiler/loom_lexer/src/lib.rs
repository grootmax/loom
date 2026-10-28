//! Lexer: turns UTF-8 Loom source into tokens, preserving trivia.
//!
//! The lexer implements spec §2. Its contract ([ADR 0018] §4):
//!
//! - It produces every token, trivia included, so that concatenating the token texts
//!   gives back the input exactly.
//! - It never fails and never panics. Bad input becomes an `ERROR_TOKEN` or a best-effort
//!   token, plus a diagnostic with a code from spec Appendix C.1.
//! - A line terminator is a `NEWLINE` token, or `LINE_BREAK` trivia when §2.3 rule 1
//!   suppresses it.
//! - An interpolated string is a token sequence: `STRING_START`, `STRING_FRAGMENT`,
//!   `INTERP_START`, the expression's tokens, `INTERP_END`, `STRING_END`.
//!
//! [ADR 0018]: https://github.com/grootmax/loom/blob/main/docs/adr/0018-front-end-design.md

mod chars;
mod literals;

use std::borrow::Cow;

use loom_diagnostics::{Applicability, Diagnostic, Suggestion, codes};
use loom_span::{FileId, Span, TextRange, TextSize};
#[allow(
    clippy::enum_glob_use,
    reason = "token matches read best with bare kind names, as in rust-analyzer"
)]
use loom_syntax::SyntaxKind::{self, *};

/// A token: its kind and where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    /// The token's kind.
    pub kind: SyntaxKind,
    /// The token's byte range in the source.
    pub range: TextRange,
}

/// The result of lexing a file.
#[derive(Clone, Debug, Default)]
pub struct Lexed {
    /// Every token, including trivia, in order. The ranges are contiguous and cover the
    /// whole input.
    pub tokens: Vec<Token>,
    /// Lexical errors, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Lexed {
    /// The text of token `index` in `source`.
    #[must_use]
    pub fn text<'s>(&self, index: usize, source: &'s str) -> &'s str {
        &source[self.tokens[index].range.to_usize()]
    }
}

/// Reserved words (spec §2.6): lexed as identifiers, with error E0009.
const RESERVED_WORDS: &[&str] = &[
    "abstract", "async", "await", "become", "box", "catch", "defer", "do_while", "final", "macro",
    "override", "priv", "try", "typeof", "unsized", "virtual", "yield",
];

/// Lexes `text`, which belongs to `file` (used for diagnostic spans).
#[must_use]
pub fn lex(text: &str, file: FileId) -> Lexed {
    Lexer::new(text, file, false).run()
}

/// Lexes raw bytes. If they are not valid UTF-8, reports E0001 at the first invalid byte
/// and lexes the lossy conversion (with U+FFFD for each invalid sequence) without
/// reporting the replacement characters again.
///
/// Returns the text that was lexed, so that token ranges can be resolved.
#[must_use]
pub fn lex_bytes(bytes: &[u8], file: FileId) -> (Cow<'_, str>, Lexed) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (Cow::Borrowed(text), lex(text, file)),
        Err(error) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            let mut lexed = Lexer::new(&text, file, true).run();
            let offset = TextSize::try_from(error.valid_up_to()).unwrap_or_default();
            lexed.diagnostics.insert(
                0,
                Diagnostic::error(
                    codes::INVALID_UTF8,
                    Span::new(file, TextRange::at(offset, TextSize::new(1))),
                    "source file is not valid UTF-8",
                )
                .with_label("invalid byte")
                .with_note("Loom source files must be encoded as UTF-8 (spec §2.1)"),
            );
            lexed
                .diagnostics
                .sort_by_key(|d| d.primary.span.range.start());
            (Cow::Owned(text), lexed)
        }
    }
}

/// What the lexer is in the middle of.
#[derive(Debug)]
enum Mode {
    /// Inside `#{ … }`. `depth` counts unmatched `{` inside the interpolation.
    Interpolation { depth: u32 },
    /// Inside the text of an ordinary or triple-quoted string.
    String(StringState),
}

#[derive(Debug)]
struct StringState {
    /// Offset of the opening delimiter.
    start: usize,
    triple: bool,
    /// For triple-quoted strings: the start offset of every content line that begins in
    /// string text (not inside an interpolation), and whether it follows a `\` line
    /// continuation (whose leading whitespace is removed anyway).
    line_starts: Vec<(usize, bool)>,
}

pub(crate) struct Lexer<'a> {
    pub(crate) text: &'a str,
    pub(crate) pos: usize,
    file: FileId,
    tokens: Vec<Token>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    modes: Vec<Mode>,
    /// The kind of the last token that is neither trivia nor a line terminator.
    last_significant: Option<SyntaxKind>,
    /// The kind of the last token other than `WHITESPACE` (for the `||` rule, §2.8.4).
    last_non_whitespace: Option<SyntaxKind>,
    /// Whether the current line has a token that is neither trivia nor a line terminator.
    line_has_token: bool,
    /// The input was lossily decoded; U+FFFD has already been reported as E0001.
    replacement_reported: bool,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str, file: FileId, replacement_reported: bool) -> Self {
        Self {
            text,
            pos: 0,
            file,
            tokens: Vec::new(),
            diagnostics: Vec::new(),
            modes: Vec::new(),
            last_significant: None,
            last_non_whitespace: None,
            line_has_token: false,
            replacement_reported,
        }
    }

    fn run(mut self) -> Lexed {
        if u32::try_from(self.text.len()).is_err() {
            // `SourceMap::add` rejects such files; never index past `u32` here.
            return Lexed::default();
        }
        self.scan_whole_file();
        self.lex_file_start();
        while self.pos < self.text.len() {
            if matches!(self.modes.last(), Some(Mode::String(_))) {
                self.lex_string_part();
            } else {
                self.lex_token();
            }
        }
        self.finish_unterminated_modes();
        self.diagnostics
            .sort_by_key(|diagnostic| diagnostic.primary.span.range.start());
        Lexed {
            tokens: self.tokens,
            diagnostics: self.diagnostics,
        }
    }

    // ----- helpers -------------------------------------------------------------------

    pub(crate) fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(crate) fn peek_nth(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    pub(crate) fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, TextRange::new(size(start), size(end)))
    }

    pub(crate) fn error(
        &mut self,
        code: loom_diagnostics::Code,
        start: usize,
        end: usize,
        message: impl Into<String>,
    ) -> &mut Diagnostic {
        let diagnostic = Diagnostic::error(code, self.span(start, end), message);
        self.diagnostics.push(diagnostic);
        self.diagnostics.last_mut().expect("just pushed")
    }

    fn push(&mut self, kind: SyntaxKind, start: usize) {
        debug_assert!(self.pos > start, "empty token {kind}");
        self.tokens.push(Token {
            kind,
            range: TextRange::new(size(start), size(self.pos)),
        });
        if kind != WHITESPACE {
            self.last_non_whitespace = Some(kind);
        }
        if matches!(kind, NEWLINE | LINE_BREAK) {
            self.line_has_token = false;
        } else if !kind.is_trivia() {
            self.last_significant = Some(kind);
            self.line_has_token = true;
        }
    }

    // ----- whole-file checks -----------------------------------------------------------

    /// Reports bidirectional formatting characters (§2.1.4) and lone carriage returns
    /// (§2.2.1) anywhere in the file, including comments and strings.
    fn scan_whole_file(&mut self) {
        let text = self.text;
        let bytes = text.as_bytes();
        for (offset, character) in text.char_indices() {
            if chars::is_bidi_control(character) {
                let end = offset + character.len_utf8();
                self.error(
                    codes::BIDI_CHARACTER,
                    offset,
                    end,
                    format!(
                        "bidirectional formatting character U+{:04X} is not allowed",
                        u32::from(character)
                    ),
                )
                .primary
                .message
                .push_str("invisible character here");
                if let Some(last) = self.diagnostics.last_mut() {
                    last.notes.push(
                        "it can make the displayed code differ from the compiled code \
                         (\"Trojan Source\"); inside a string, write it as a `\\u{…}` escape"
                            .to_owned(),
                    );
                }
            } else if character == '\r' && bytes.get(offset + 1) != Some(&b'\n') {
                self.error(
                    codes::LONE_CARRIAGE_RETURN,
                    offset,
                    offset + 1,
                    "carriage return (U+000D) not followed by a line feed",
                )
                .notes
                .push("line terminators are LF or CR LF (spec §2.2)".to_owned());
            }
        }
    }

    /// Handles a byte order mark and a shebang line (§2.1.2, §2.1.3).
    fn lex_file_start(&mut self) {
        if self.rest().starts_with('\u{FEFF}') {
            self.pos += '\u{FEFF}'.len_utf8();
            self.push(WHITESPACE, 0);
        }
        if self.rest().starts_with("#!") {
            let start = self.pos;
            let reserved = self.rest().starts_with("#![");
            self.pos += self.line_length();
            if reserved {
                self.error(
                    codes::RESERVED_SHEBANG,
                    start,
                    start + 1,
                    "`#![` at the start of a file is reserved",
                )
                .help
                .push("start the first line with `# ` to write a comment".to_owned());
                self.push(COMMENT, start);
            } else {
                self.push(SHEBANG, start);
            }
        }
    }

    /// The length of the rest of the current line, excluding its terminator.
    fn line_length(&self) -> usize {
        self.rest().find('\n').map_or(self.rest().len(), |end| {
            if end > 0 && self.rest().as_bytes()[end - 1] == b'\r' {
                end - 1
            } else {
                end
            }
        })
    }

    // ----- ordinary tokens ---------------------------------------------------------------

    fn lex_token(&mut self) {
        let start = self.pos;
        let Some(character) = self.peek() else {
            return;
        };
        match character {
            ' ' | '\t' => {
                self.pos += self
                    .rest()
                    .bytes()
                    .take_while(|&b| b == b' ' || b == b'\t')
                    .count();
                self.push(WHITESPACE, start);
            }
            '\n' => {
                self.pos += 1;
                self.line_terminator(start);
            }
            '\r' if self.rest().starts_with("\r\n") => {
                self.pos += 2;
                self.line_terminator(start);
            }
            '\r' => {
                // Already reported by `scan_whole_file`; recover as a line terminator.
                self.pos += 1;
                self.line_terminator(start);
            }
            '#' => self.lex_comment(start),
            '"' => {
                if self.rest().starts_with("\"\"\"") {
                    self.pos += 3;
                    self.push(STRING_START, start);
                    self.open_triple_quoted(start);
                } else {
                    self.pos += 1;
                    self.push(STRING_START, start);
                    self.modes.push(Mode::String(StringState {
                        start,
                        triple: false,
                        line_starts: Vec::new(),
                    }));
                }
            }
            '\'' => self.lex_char_literal(start),
            '0'..='9' => self.lex_number(start),
            '@' => self.lex_at(start),
            'r' if self.raw_string_ahead(1) => self.lex_raw_string(start, 1, RAW_STRING_LIT),
            'b' if self.rest().starts_with("b\"") => self.lex_byte_string(start),
            'b' if self.rest().starts_with("br") && self.raw_string_ahead(2) => {
                self.lex_raw_string(start, 2, BYTE_STRING_LIT);
            }
            '{' => {
                self.pos += 1;
                if let Some(Mode::Interpolation { depth }) = self.modes.last_mut() {
                    *depth += 1;
                }
                self.push(L_BRACE, start);
            }
            '}' => {
                self.pos += 1;
                match self.modes.last_mut() {
                    Some(Mode::Interpolation { depth: 0 }) => {
                        self.modes.pop();
                        self.push(INTERP_END, start);
                    }
                    Some(Mode::Interpolation { depth }) => {
                        *depth -= 1;
                        self.push(R_BRACE, start);
                    }
                    _ => self.push(R_BRACE, start),
                }
            }
            c if chars::is_ident_start(c) => self.lex_identifier(start),
            c if chars::is_unicode_whitespace(c) => {
                self.pos += c.len_utf8();
                self.error(
                    codes::NON_ASCII_WHITESPACE,
                    start,
                    self.pos,
                    format!(
                        "{} (U+{:04X}) is not allowed here",
                        chars::whitespace_name(c),
                        u32::from(c)
                    ),
                )
                .help
                .push("use a space (U+0020) or a tab".to_owned());
                self.push(WHITESPACE, start);
            }
            c if chars::is_bidi_control(c) => {
                // Already reported by `scan_whole_file`; treat it as whitespace.
                self.pos += c.len_utf8();
                self.push(WHITESPACE, start);
            }
            _ => self.lex_punctuation(start, character),
        }
    }

    fn lex_comment(&mut self, start: usize) {
        let kind = if self.rest().starts_with("##!") {
            INNER_DOC_COMMENT
        } else if self.rest().starts_with("##") {
            DOC_COMMENT
        } else {
            COMMENT
        };
        self.pos += self.line_length();
        self.push(kind, start);
    }

    fn lex_identifier(&mut self, start: usize) {
        let length: usize = self
            .rest()
            .chars()
            .take_while(|&c| chars::is_ident_continue(c))
            .map(char::len_utf8)
            .sum();
        self.pos += length;
        let word = &self.text[start..self.pos];
        if let Some(keyword) = SyntaxKind::from_keyword(word) {
            self.push(keyword, start);
            return;
        }
        if RESERVED_WORDS.contains(&word) {
            self.error(
                codes::RESERVED_WORD,
                start,
                self.pos,
                format!("`{word}` is a reserved word"),
            )
            .notes
            .push("reserved words cannot be used as identifiers (spec §2.6)".to_owned());
        }
        self.push(IDENT, start);
    }

    fn lex_at(&mut self, start: usize) {
        self.pos += 1;
        match self.peek() {
            Some('[') => {
                self.pos += 1;
                self.push(AT_BRACKET, start);
            }
            Some(c) if chars::is_ident_start(c) => {
                let length: usize = self
                    .rest()
                    .chars()
                    .take_while(|&c| chars::is_ident_continue(c))
                    .map(char::len_utf8)
                    .sum();
                self.pos += length;
                self.push(AT_IDENT, start);
            }
            _ => self.push(AT, start),
        }
    }

    fn lex_punctuation(&mut self, start: usize, character: char) {
        let rest = self.rest();
        // `||` directly after `{` or `do` is an empty closure parameter list (§2.8.4).
        if rest.starts_with("||") && matches!(self.last_non_whitespace, Some(L_BRACE | DO_KW)) {
            self.pos += 1;
            self.push(PIPE, start);
            return;
        }
        for length in [3, 2, 1] {
            let Some(candidate) = rest.get(..length) else {
                continue;
            };
            let kind = match candidate {
                ";" => Some(SEMICOLON),
                "!" => Some(BANG),
                "&&" => Some(AMP2),
                "||" => Some(PIPE2),
                _ => SyntaxKind::from_punctuation(candidate)
                    .filter(|kind| !matches!(kind, AT | AT_BRACKET | L_BRACE | R_BRACE)),
            };
            if let Some(kind) = kind {
                self.pos += length;
                self.report_foreign_operator(kind, start);
                self.push(kind, start);
                return;
            }
        }
        self.pos += character.len_utf8();
        if !(self.replacement_reported && character == char::REPLACEMENT_CHARACTER) {
            let description = if character.is_control() {
                format!("U+{:04X}", u32::from(character))
            } else {
                format!("`{character}`")
            };
            self.error(
                codes::UNEXPECTED_CHARACTER,
                start,
                self.pos,
                format!("unexpected character {description}"),
            );
        }
        self.push(ERROR_TOKEN, start);
    }

    /// Reports `;`, `!`, `&&` and `||`, which are not Loom operators (§2.3.4, §2.8).
    fn report_foreign_operator(&mut self, kind: SyntaxKind, start: usize) {
        let end = self.pos;
        let span = self.span(start, end);
        let diagnostic = match kind {
            SEMICOLON => Diagnostic::error(codes::SEMICOLON, span, "`;` is not used in Loom")
                .with_label("remove this")
                .with_help("statements end at a line break")
                .with_suggestion(Suggestion::replace(
                    "use a line break instead",
                    span,
                    "\n",
                    Applicability::MaybeIncorrect,
                )),
            BANG => Diagnostic::error(codes::BANG_OPERATOR, span, "`!` is not an operator")
                .with_label("not an operator")
                .with_help("use `not` for logical negation, or `~` for bitwise complement")
                .with_suggestion(Suggestion::replace(
                    "use `not`",
                    span,
                    "not ",
                    Applicability::MaybeIncorrect,
                )),
            AMP2 | PIPE2 => {
                let (spelled, word) = if kind == AMP2 {
                    ("&&", "and")
                } else {
                    ("||", "or")
                };
                Diagnostic::error(
                    codes::DOUBLE_AMPERSAND_OR_PIPE,
                    span,
                    format!("`{spelled}` is not an operator"),
                )
                .with_label(format!("use `{word}`"))
                .with_suggestion(Suggestion::replace(
                    format!("use `{word}`"),
                    span,
                    word,
                    Applicability::MachineApplicable,
                ))
            }
            _ => return,
        };
        self.diagnostics.push(diagnostic);
    }

    // ----- line terminators (§2.3) --------------------------------------------------------

    /// A line with no tokens (blank or comment-only) never produces `NEWLINE`: by §2.3
    /// rule 3 it would be redundant, and as trivia it lets comments attach to the item
    /// that follows them.
    fn line_terminator(&mut self, start: usize) {
        let kind =
            if !self.line_has_token || self.continues_after_line() || self.leading_dot_follows() {
                LINE_BREAK
            } else {
                NEWLINE
            };
        self.push(kind, start);
    }

    /// §2.3 rule 1 (a): the last token, ignoring trivia, continues the expression.
    fn continues_after_line(&self) -> bool {
        matches!(
            self.last_significant,
            Some(
                PLUS | MINUS
                    | STAR
                    | SLASH
                    | PERCENT
                    | AMP
                    | CARET
                    | SHL
                    | SHR
                    | EQ2
                    | NEQ
                    | LT
                    | LE
                    | GT
                    | GE
                    | DOT2
                    | DOT2_EQ
                    | COMMA
                    | DOT
                    | EQ
                    | PLUS_EQ
                    | MINUS_EQ
                    | STAR_EQ
                    | SLASH_EQ
                    | PERCENT_EQ
                    | AMP_EQ
                    | PIPE_EQ
                    | CARET_EQ
                    | SHL_EQ
                    | SHR_EQ
                    | ARROW
                    | FAT_ARROW
                    | AND_KW
                    | OR_KW
                    | NOT_KW
                    | AMP2
                    | PIPE2
            )
        )
    }

    /// §2.3 rule 1 (b): the next line that is not blank or comment-only starts with `.`
    /// followed by an identifier or `@`.
    fn leading_dot_follows(&self) -> bool {
        let mut rest = self.rest();
        loop {
            rest = rest.trim_start_matches([' ', '\t']);
            if rest.starts_with('#') {
                let end = rest.find('\n').unwrap_or(rest.len());
                rest = &rest[end..];
            } else if let Some(after) = rest.strip_prefix('\n') {
                rest = after;
            } else if let Some(after) = rest.strip_prefix("\r\n") {
                rest = after;
            } else {
                break;
            }
        }
        let mut characters = rest.chars();
        characters.next() == Some('.')
            && characters
                .next()
                .is_some_and(|c| c == '@' || chars::is_ident_start(c))
    }

    // ----- strings -----------------------------------------------------------------------

    fn open_triple_quoted(&mut self, start: usize) {
        if !(self.rest().starts_with('\n') || self.rest().starts_with("\r\n")) {
            self.error(
                codes::TRIPLE_QUOTE_OPENING,
                start,
                start + 3,
                "opening `\"\"\"` must be followed by a line break",
            )
            .help
            .push("start the text on the next line".to_owned());
        }
        self.modes.push(Mode::String(StringState {
            start,
            triple: true,
            line_starts: Vec::new(),
        }));
    }

    fn lex_string_part(&mut self) {
        let start = self.pos;
        let Some(Mode::String(state)) = self.modes.last() else {
            return;
        };
        let triple = state.triple;
        if triple && self.rest().starts_with("\"\"\"") || !triple && self.rest().starts_with('"') {
            self.pos += if triple { 3 } else { 1 };
            self.push(STRING_END, start);
            if let Some(Mode::String(state)) = self.modes.pop()
                && triple
            {
                self.check_triple_quoted(&state, start);
            }
            return;
        }
        if self.rest().starts_with("#{") {
            self.pos += 2;
            self.push(INTERP_START, start);
            self.modes.push(Mode::Interpolation { depth: 0 });
            return;
        }
        let mut new_lines = Vec::new();
        let mut after_continuation = false;
        while let Some(character) = self.peek() {
            if character == '"' && (!triple || self.rest().starts_with("\"\"\"")) {
                break;
            }
            if self.rest().starts_with("#{") {
                break;
            }
            match character {
                '\\' => {
                    let continuation = literals::lex_escape(self, literals::Context::String);
                    after_continuation |= continuation;
                    if continuation {
                        new_lines.push((self.pos, true));
                    }
                }
                '\n' => {
                    self.pos += 1;
                    new_lines.push((self.pos, after_continuation));
                    after_continuation = false;
                }
                c => self.pos += c.len_utf8(),
            }
        }
        if triple && let Some(Mode::String(state)) = self.modes.last_mut() {
            state.line_starts.extend(new_lines);
        }
        if self.pos > start {
            self.push(STRING_FRAGMENT, start);
        }
    }

    /// Checks the closing-line and indentation rules of a triple-quoted string (§2.7.4.3).
    /// `closing` is the offset of the closing `"""`.
    fn check_triple_quoted(&mut self, state: &StringState, closing: usize) {
        let line_start = self.text[..closing]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        let prefix = &self.text[line_start..closing];
        if line_start <= state.start || !prefix.bytes().all(|b| b == b' ' || b == b'\t') {
            self.error(
                codes::TRIPLE_QUOTE_CLOSING,
                closing,
                closing + 3,
                "closing `\"\"\"` must be on its own line",
            )
            .help
            .push("put the closing `\"\"\"` on a new line, after only whitespace".to_owned());
            return;
        }
        for &(start, after_continuation) in &state.line_starts {
            if start >= line_start || after_continuation {
                continue;
            }
            let line_end = self.text[start..]
                .find('\n')
                .map_or(self.text.len(), |end| start + end);
            let line = self.text[start..line_end].trim_end_matches('\r');
            if line.bytes().all(|b| b == b' ' || b == b'\t') {
                continue;
            }
            if !line.starts_with(prefix) {
                let closing_span = self.span(closing, closing + 3);
                self.error(
                    codes::TRIPLE_QUOTE_INDENTATION,
                    start,
                    start,
                    "line is not indented as far as the closing `\"\"\"`",
                )
                .with_secondary_inline(closing_span, "closing delimiter")
                .help
                .push("indent every line at least as far as the closing `\"\"\"`".to_owned());
            }
        }
    }

    fn finish_unterminated_modes(&mut self) {
        // Report only the outermost unterminated string: the inner ones are a consequence.
        let outermost = self.modes.iter().find_map(|mode| match mode {
            Mode::String(state) => Some((state.start, state.triple)),
            Mode::Interpolation { .. } => None,
        });
        if let Some((start, triple)) = outermost {
            let length = if triple { 3 } else { 1 };
            self.error(
                codes::UNTERMINATED_STRING,
                start,
                start + length,
                "unterminated string literal",
            )
            .primary
            .message
            .push_str("string starts here");
        }
        self.modes.clear();
    }
}

/// Converts an offset that is known to fit (inputs are limited to 4 GiB) to a `TextSize`.
pub(crate) fn size(offset: usize) -> TextSize {
    TextSize::try_from(offset).unwrap_or(TextSize::new(u32::MAX))
}

/// Adds a secondary label in builder style to a borrowed diagnostic.
trait SecondaryInline {
    fn with_secondary_inline(&mut self, span: Span, message: &str) -> &mut Self;
}

impl SecondaryInline for Diagnostic {
    fn with_secondary_inline(&mut self, span: Span, message: &str) -> &mut Self {
        self.secondary
            .push(loom_diagnostics::Label::new(span, message));
        self
    }
}

#[cfg(test)]
mod tests;
