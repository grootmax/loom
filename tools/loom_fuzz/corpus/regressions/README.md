# Regression Corpus

This directory contains minimized crashing or invariant-violating inputs discovered by `loom_fuzz`.

When a bug is found by the fuzzer, the minimized input is saved here and replayed as part of the regression test suite in `tests/smoke.rs`.
