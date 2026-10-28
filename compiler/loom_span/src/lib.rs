//! Source files, byte spans and line/column mapping.
//!
//! Offsets are byte offsets into UTF-8 text, stored as `u32` ([ADR 0018] §2), so a single
//! source file is limited to 4 GiB. Positions shown to people use a 1-based line and a
//! 1-based column that counts Unicode scalar values.
//!
//! [ADR 0018]: https://github.com/grootmax/loom/blob/main/docs/adr/0018-front-end-design.md

use std::fmt;
use std::ops::{Add, AddAssign, Range, Sub};
use std::sync::Arc;

/// A byte offset or length in a source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextSize(u32);

impl TextSize {
    /// Creates a size from a raw byte count.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// The length of `text` in bytes.
    ///
    /// # Panics
    ///
    /// Panics if `text` is longer than `u32::MAX` bytes. Source files are checked against
    /// that limit when they are loaded ([`SourceMap::add`]).
    #[must_use]
    pub fn of(text: &str) -> Self {
        Self(u32::try_from(text.len()).expect("text longer than 4 GiB"))
    }

    /// The raw byte count.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// The byte count as a `usize`, for indexing.
    #[must_use]
    pub const fn to_usize(self) -> usize {
        self.0 as usize
    }
}

impl From<u32> for TextSize {
    fn from(raw: u32) -> Self {
        Self(raw)
    }
}

impl TryFrom<usize> for TextSize {
    type Error = std::num::TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map(Self)
    }
}

impl Add for TextSize {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl AddAssign for TextSize {
    fn add_assign(&mut self, other: Self) {
        self.0 += other.0;
    }
}

impl Sub for TextSize {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl fmt::Display for TextSize {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// A half-open byte range `[start, end)` in a source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextRange {
    start: TextSize,
    end: TextSize,
}

impl TextRange {
    /// Creates the range `[start, end)`.
    ///
    /// # Panics
    ///
    /// Panics if `start > end`.
    #[must_use]
    pub fn new(start: TextSize, end: TextSize) -> Self {
        assert!(start <= end, "invalid text range {start}..{end}");
        Self { start, end }
    }

    /// Creates the range starting at `start` with length `length`.
    #[must_use]
    pub fn at(start: TextSize, length: TextSize) -> Self {
        Self::new(start, start + length)
    }

    /// The empty range at `offset`.
    #[must_use]
    pub fn empty(offset: TextSize) -> Self {
        Self::new(offset, offset)
    }

    /// The first byte of the range.
    #[must_use]
    pub const fn start(self) -> TextSize {
        self.start
    }

    /// The byte just past the end of the range.
    #[must_use]
    pub const fn end(self) -> TextSize {
        self.end
    }

    /// The length of the range in bytes.
    #[must_use]
    pub fn len(self) -> TextSize {
        self.end - self.start
    }

    /// Whether the range is empty.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    /// Whether `offset` lies inside the range (the end is excluded).
    #[must_use]
    pub fn contains(self, offset: TextSize) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Whether `other` lies entirely inside this range.
    #[must_use]
    pub fn contains_range(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// The smallest range that covers both ranges.
    #[must_use]
    pub fn cover(self, other: Self) -> Self {
        Self::new(self.start.min(other.start), self.end.max(other.end))
    }

    /// The range as a `usize` range, for slicing strings.
    #[must_use]
    pub const fn to_usize(self) -> Range<usize> {
        self.start.to_usize()..self.end.to_usize()
    }
}

impl fmt::Display for TextRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}..{}", self.start, self.end)
    }
}

/// Identifies a file in a [`SourceMap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileId(u32);

impl FileId {
    /// A placeholder file for code that is not tied to a [`SourceMap`], such as unit
    /// tests of a single lexer or parser call.
    pub const DETACHED: Self = Self(u32::MAX);

    /// The raw index.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// A range in a specific file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    /// The file the range is in.
    pub file: FileId,
    /// The byte range in that file.
    pub range: TextRange,
}

impl Span {
    /// Creates a span.
    #[must_use]
    pub const fn new(file: FileId, range: TextRange) -> Self {
        Self { file, range }
    }
}

/// A position shown to people: a 1-based line and a 1-based column that counts Unicode
/// scalar values from the start of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineColumn {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column, in Unicode scalar values.
    pub column: u32,
}

impl fmt::Display for LineColumn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.line, self.column)
    }
}

/// Maps byte offsets to lines and columns.
///
/// A line ends after `\n`. A lone `\r` does not end a line (it is an error, spec §2.2.1),
/// so `\r\n` counts as a single terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offset of the start of each line. Always starts with 0.
    line_starts: Vec<TextSize>,
}

impl LineIndex {
    /// Builds the index for `text`.
    ///
    /// # Panics
    ///
    /// Panics if `text` is longer than `u32::MAX` bytes.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![TextSize::new(0)];
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(TextSize::try_from(offset + 1).expect("text longer than 4 GiB"));
            }
        }
        Self { line_starts }
    }

    /// The number of lines. An empty text has one (empty) line.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// The 0-based line containing `offset`. An offset at the very end of the text belongs
    /// to the last line.
    #[must_use]
    pub fn line_of(&self, offset: TextSize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next - 1,
        }
    }

    /// The byte offset where 0-based `line` starts, or `None` if there is no such line.
    #[must_use]
    pub fn line_start(&self, line: usize) -> Option<TextSize> {
        self.line_starts.get(line).copied()
    }

    /// The byte range of 0-based `line` in `text`, excluding its line terminator.
    #[must_use]
    pub fn line_range(&self, line: usize, text: &str) -> Option<TextRange> {
        let start = self.line_start(line)?;
        let end = self
            .line_start(line + 1)
            .map_or_else(|| TextSize::of(text), |next| next - TextSize::new(1));
        let mut end = end.max(start);
        if end > start && text.as_bytes()[end.to_usize() - 1] == b'\r' {
            end = end - TextSize::new(1);
        }
        Some(TextRange::new(start, end))
    }

    /// Converts a byte offset in `text` to a line and column.
    ///
    /// An offset in the middle of a multi-byte character is counted as the column of that
    /// character.
    #[must_use]
    pub fn line_column(&self, offset: TextSize, text: &str) -> LineColumn {
        let offset = offset.min(TextSize::of(text));
        let line = self.line_of(offset);
        let start = self.line_starts[line].to_usize();
        let prefix = &text.as_bytes()[start..offset.to_usize()];
        // Count scalar values by counting bytes that are not UTF-8 continuation bytes.
        let column = prefix.iter().filter(|&&byte| byte & 0xC0 != 0x80).count();
        LineColumn {
            line: saturating_u32(line + 1),
            column: saturating_u32(column + 1),
        }
    }
}

/// Converts a count that is bounded by the 4 GiB file limit to `u32`.
fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// A source file: its name, its text and a line index.
#[derive(Clone, Debug)]
pub struct SourceFile {
    name: String,
    text: Arc<str>,
    lines: LineIndex,
}

impl SourceFile {
    /// Creates a source file.
    ///
    /// # Panics
    ///
    /// Panics if `text` is longer than `u32::MAX` bytes. Use [`SourceMap::add`], which
    /// reports that as an error instead.
    #[must_use]
    pub fn new(name: impl Into<String>, text: impl Into<Arc<str>>) -> Self {
        let text = text.into();
        let lines = LineIndex::new(&text);
        Self {
            name: name.into(),
            text,
            lines,
        }
    }

    /// The name used in diagnostics, usually a path relative to the working directory.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The full text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// A shared handle to the full text.
    #[must_use]
    pub fn shared_text(&self) -> Arc<str> {
        Arc::clone(&self.text)
    }

    /// The line index.
    #[must_use]
    pub fn lines(&self) -> &LineIndex {
        &self.lines
    }

    /// Converts a byte offset to a line and column.
    #[must_use]
    pub fn line_column(&self, offset: TextSize) -> LineColumn {
        self.lines.line_column(offset, &self.text)
    }

    /// The text of 0-based `line`, without its line terminator.
    #[must_use]
    pub fn line_text(&self, line: usize) -> Option<&str> {
        let range = self.lines.line_range(line, &self.text)?;
        Some(&self.text[range.to_usize()])
    }

    /// The text covered by `range`.
    ///
    /// # Panics
    ///
    /// Panics if `range` is out of bounds or not on character boundaries.
    #[must_use]
    pub fn slice(&self, range: TextRange) -> &str {
        &self.text[range.to_usize()]
    }
}

/// The error returned when a file is too large to be represented with `u32` offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTooLarge {
    /// The file's name.
    pub name: String,
    /// The file's size in bytes.
    pub size: usize,
}

impl fmt::Display for FileTooLarge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "`{}` is {} bytes; source files are limited to 4 GiB",
            self.name, self.size
        )
    }
}

impl std::error::Error for FileTooLarge {}

/// All source files known to one compilation.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Creates an empty source map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a file and returns its id.
    ///
    /// # Errors
    ///
    /// Returns [`FileTooLarge`] if the text is longer than `u32::MAX` bytes.
    ///
    /// # Panics
    ///
    /// Panics if the map already holds `u32::MAX - 1` files.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        text: impl Into<Arc<str>>,
    ) -> Result<FileId, FileTooLarge> {
        let name = name.into();
        let text = text.into();
        if u32::try_from(text.len()).is_err() {
            return Err(FileTooLarge {
                name,
                size: text.len(),
            });
        }
        let id = u32::try_from(self.files.len()).expect("too many source files");
        assert!(id != FileId::DETACHED.0, "too many source files");
        self.files.push(SourceFile::new(name, text));
        Ok(FileId(id))
    }

    /// The file with the given id.
    ///
    /// # Panics
    ///
    /// Panics if `id` does not belong to this map (including [`FileId::DETACHED`]).
    #[must_use]
    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    /// The file with the given id, or `None` if it does not belong to this map.
    #[must_use]
    pub fn get(&self, id: FileId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    /// The start position of `span`.
    #[must_use]
    pub fn line_column(&self, span: Span) -> LineColumn {
        self.file(span.file).line_column(span.range.start())
    }
}

#[cfg(test)]
mod tests {
    use super::{FileId, LineColumn, LineIndex, SourceMap, TextRange, TextSize};

    fn size(raw: u32) -> TextSize {
        TextSize::new(raw)
    }

    fn position(line: u32, column: u32) -> LineColumn {
        LineColumn { line, column }
    }

    #[test]
    fn text_range_operations() {
        let range = TextRange::new(size(2), size(5));
        assert_eq!(range.len(), size(3));
        assert!(range.contains(size(2)));
        assert!(!range.contains(size(5)));
        assert!(TextRange::empty(size(4)).is_empty());
        assert_eq!(
            range.cover(TextRange::new(size(7), size(9))),
            TextRange::new(size(2), size(9))
        );
        assert!(range.contains_range(TextRange::new(size(3), size(5))));
        assert_eq!(range.to_string(), "2..5");
    }

    #[test]
    #[should_panic(expected = "invalid text range")]
    fn text_range_rejects_reversed_bounds() {
        let _ = TextRange::new(size(3), size(2));
    }

    #[test]
    fn line_columns() {
        let text = "ab\ncd\r\n\nxyz";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 4);
        let cases = [
            (0, position(1, 1)),
            (2, position(1, 3)),
            (3, position(2, 1)),
            (5, position(2, 3)), // the `\r`
            (7, position(3, 1)),
            (8, position(4, 1)),
            (11, position(4, 4)), // end of text
        ];
        for (offset, expected) in cases {
            assert_eq!(
                index.line_column(size(offset), text),
                expected,
                "offset {offset}"
            );
        }
    }

    #[test]
    fn columns_count_scalar_values_not_bytes() {
        // `é` is 2 bytes, `😀` is 4 bytes, a tab counts as one column.
        let text = "é😀\tx";
        let index = LineIndex::new(text);
        assert_eq!(index.line_column(size(2), text), position(1, 2));
        assert_eq!(index.line_column(size(6), text), position(1, 3));
        assert_eq!(index.line_column(size(7), text), position(1, 4));
    }

    #[test]
    fn line_ranges_exclude_terminators() {
        let text = "ab\r\ncd\n\nlast";
        let index = LineIndex::new(text);
        let lines: Vec<&str> = (0..index.line_count())
            .map(|line| &text[index.line_range(line, text).unwrap().to_usize()])
            .collect();
        assert_eq!(lines, ["ab", "cd", "", "last"]);
        assert_eq!(index.line_range(4, text), None);
    }

    #[test]
    fn empty_text_has_one_line() {
        let index = LineIndex::new("");
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.line_column(size(0), ""), position(1, 1));
    }

    #[test]
    fn source_map_round_trip() {
        let mut map = SourceMap::new();
        let first = map.add("a.loom", "def main\nend\n").unwrap();
        let second = map.add("b.loom", "x").unwrap();
        assert_ne!(first, second);
        assert_eq!(map.file(first).name(), "a.loom");
        assert_eq!(map.file(first).line_text(1), Some("end"));
        assert_eq!(map.file(second).text(), "x");
        let span = super::Span::new(first, TextRange::new(size(9), size(12)));
        assert_eq!(map.line_column(span), position(2, 1));
        assert_eq!(map.file(first).slice(span.range), "end");
        assert!(map.get(FileId::DETACHED).is_none());
    }
}
