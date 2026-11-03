# TCP Echo Benchmark

This directory contains TCP echo benchmark implementations and documentation for `cargo xtask bench echo`.

## Overview

The TCP echo benchmark evaluates network I/O throughput and latency under concurrent load. The benchmark client connects to each server over localhost TCP, transmits deterministic byte patterns across concurrent connections, and measures round-trip latency and message throughput.

## Supported Servers

1. **`rust-threads` (`benches/echo/reference_server.rs`)**:
   A baseline thread-per-connection server written in Rust using `std::net`. Compiled by the harness with `rustc -O`.
2. **`loom` (`benches/echo/echo_server.loom`)**:
   The native Loom echo server implementation using `std.net` and lightweight task concurrency. Compiled with `loom build`.

## Running the Benchmark

```sh
# Benchmark all available servers:
cargo xtask bench echo

# Benchmark only the Rust reference server:
cargo xtask bench echo --server rust-threads

# Benchmark with custom connection count and message parameters:
cargo xtask bench echo --conns 50 --msgs 500 --size 128

# Benchmark with a duration limit:
cargo xtask bench echo --duration-secs 10
```

## Protocol & Verification

- Servers accept the port as their first command-line argument and print `listening` on standard output when ready to accept connections.
- Each client connection transmits messages of configured size, where each byte is deterministically generated from the connection and message index.
- The client verifies byte-for-byte echo accuracy on every round-trip. Any mismatch or connection drop causes the benchmark to report an error identifying the server, connection, and message index.
- Latencies are aggregated to report p50, p99, and maximum round-trip latency in microseconds, alongside overall message throughput.
