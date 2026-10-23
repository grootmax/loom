# Fuzzing

`cargo-fuzz` targets for the lexer, parser, type checker and interpreter. The first
targets (lexer, parser) arrive in Phase 1; Phase 1 exits only after 24 hours of clean
parser fuzzing. Crashes become regression tests.
