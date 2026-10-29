# Loom Benchmark Suite

This directory contains benchmark programs written in Loom. The benchmark harness is executed via `cargo xtask bench`.

## Running Benchmarks

```sh
# Run native benchmarks with 5 runs each (default):
cargo xtask bench

# Specify number of runs:
cargo xtask bench --runs 3

# Include interpreter timing (with a 60-second timeout limit per benchmark):
cargo xtask bench --interp

# Filter benchmarks by name:
cargo xtask bench --filter fib
```

## Benchmark Suite Overview

1. `01_fib.loom`: Recursive Fibonacci function.
2. `02_sieve.loom`: Prime sieve over an `Array[Bool]`.
3. `03_strings.loom`: String building and splitting.
4. `04_wordcount.loom`: Word frequency analysis with `Map[String, Int]`.
5. `05_sort.loom`: Sorting 100,000 integers with `sort_by`.
6. `06_nbody.loom`: N-body particle position/velocity updates using structs.
7. `07_closures.loom`: Higher-order functions with `map` and `filter`.

## Benchmark Program Guidelines

- Every benchmark program prints a `checksum: <value>` string so work cannot be optimized away.
- Native wall-clock execution time in a release build should take between 0.1s and 2s per benchmark.
- Standard output printed by native executables and the reference interpreter must match identically.
- Only valid Loom syntax from `spec/grammar.ebnf` and `examples/` may be used.
