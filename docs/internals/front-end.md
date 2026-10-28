# The front end: lexer, CST, parser, AST, formatter

Design: [ADR 0018](../adr/0018-front-end-design.md). Normative behavior: spec §2–§11 and
Appendix C. This page explains how the code is organized and how to work on it.

## Data flow

```
bytes ─► loom_lexer::lex_bytes ─► Lexed { tokens (with trivia), diagnostics }
      ─► loom_parser (grammar → events → sink) ─► GreenNode (lossless) + diagnostics
      ─► SyntaxNode (red cursor) ─► loom_ast views ─► loom_fmt / later phases
```

## Lexer (`compiler/loom_lexer`)

- `lib.rs`: the main loop, the mode stack (`Mode::String` / `Mode::Interpolation`), trivia,
  line terminators (§2.3) and punctuation. `literals.rs`: numbers, chars, raw and byte
  strings, escapes. `chars.rs`: character classes.
- A line terminator becomes `NEWLINE` or `LINE_BREAK` (trivia). `LINE_BREAK` is used when
  §2.3 rule 1 suppresses the terminator: after a continuation token, before a leading-dot
  line, or at the end of a line with no tokens.
- Foreign operators (`;`, `!`, `&&`, `||`) get their own token kinds plus a diagnostic,
  so that the parser can recover silently.

## CST (`compiler/loom_syntax`)

- `kind.rs`: `SyntaxKind` (tokens, then keywords, then punctuation, then nodes).
- `green.rs`: immutable `Arc` nodes and the `GreenNodeBuilder` (with checkpoints).
- `red.rs`: `SyntaxNode` / `SyntaxToken` cursors, navigation, and `debug_dump()` for tests.

## Parser (`compiler/loom_parser`)

- `parser.rs`: the token cursor over *significant* tokens, newline contexts, markers
  (`start` / `complete` / `precede`), recovery helpers and the nesting limit.
- `grammar/`: one function per production (`items.rs`, `expressions.rs`, `patterns.rs`,
  `types.rs`). Expressions use a Pratt loop with the binding powers of spec §7.5.
- `sink.rs`: replays events into the green tree and attaches comments directly above an
  item to that item's node.
- `validation.rs`: checks on the finished tree (doc comment placement).
- To see a tree: `cargo run -p loom_cli -- check --dump=cst file.loom`.

## Formatter (`tools/loom_fmt`)

The style is in [`docs/reference/style.md`](../reference/style.md). The formatter walks the
tokens line by line. Indentation comes from containers (bodies, member lists, brackets)
that were opened on earlier lines, plus one level for continuation lines. Spacing comes from
the kinds of neighboring tokens and their parents. It never changes tokens, so
"formatting preserves the token sequence" is a cheap, strong test.

## Testing

- Unit tests in each crate, plus `every_example_*` tests over `examples/`.
- `cargo xtask conformance` runs `spec/conformance/`.
- Snapshot tests use `loom_snapshot` (`LOOM_BLESS=1` to update).
- `tools/loom_fuzz` fuzzes the lexer, parser and formatter (ADR 0018 §9).
