# Loom

**Reads like Ruby. Safe and fast like Rust. Concurrent like Go.**

Loom is a statically typed, compiled, memory-safe programming language. It is built on
Rust — its compiler, runtime and tools are written in Rust, and its core semantics
(ownership, borrowing, `Option`/`Result`, traits, data-race freedom) are Rust's — but it
infers ownership and never asks you to write a lifetime in everyday code.

> **Status: pre-alpha (Phase 0 — Foundations).** Nothing runs yet. The design is written
> down in [ADRs](docs/adr/) and a [specification draft](spec/loom-spec.md); the compiler
> is a set of stub crates. See [docs/STATUS.md](docs/STATUS.md) for exactly what works.

## A 60-second tour

```loom
## A user of the system.
struct User
  name: String
  age: Int
end

def greet(user: User) -> String
  "Hello, #{user.name}!"
end

def main
  users = [User(name: "Asha", age: 31), User(name: "Ravi", age: 17)]

  users
    .filter { |u| u.age >= 18 }
    .each do |u|
      puts greet(u)
    end
end
```

- **No null, no exceptions.** Absence is `Option[T]`; failure is `Result[T, E]`, and `?`
  returns early on errors.
- **Ownership without ceremony.** Values have one owner and are freed deterministically.
  Parameters are borrowed by default; `mut` and `own` say when they are not. Shared
  ownership is explicit: `Shared[T]`.
- **Concurrency without colors.** `spawn` lightweight tasks inside a `scope`, talk over
  `Chan[T]`, wait on several things with `select`. There is no `async`/`await` split, and
  data races are compile errors.
- **One tool.** `loom` builds, runs, tests, formats, lints, documents and packages code.

More examples live in [`examples/`](examples/). Each has its expected output next to it.

## Building from source

Requirements: [rustup](https://rustup.rs). The toolchain version is pinned in
`rust-toolchain.toml` and installed automatically.

```sh
git clone https://github.com/grootmax/loom
cd loom
cargo xtask ci          # everything CI checks: fmt, clippy, tests, docs, cargo-deny
cargo run -p loom_cli -- --version
```

`cargo xtask ci` uses [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) if it is
installed (`cargo install cargo-deny --locked`); CI always runs it.

## Project map

| Path | What |
|---|---|
| [`docs/vision.md`](docs/vision.md), [`docs/principles.md`](docs/principles.md) | Why Loom exists and how trade-offs are decided |
| [`docs/adr/`](docs/adr/) | Architecture Decision Records — every important decision and its reasoning |
| [`spec/`](spec/) | The normative language specification and formal grammar |
| [`compiler/`](compiler/), [`runtime/`](runtime/), [`tools/`](tools/) | The toolchain (Rust) |
| [`examples/`](examples/) | Example programs with expected output |
| [`docs/roadmap.md`](docs/roadmap.md) | Phases and exit criteria |
| [`tasks/`](tasks/) | Delegated task specs and their ledger |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). Language changes go through the
[RFC process](docs/rfcs/README.md). Security issues: see [SECURITY.md](SECURITY.md).

## Name

"Loom" is a working name; see [ADR 0014](docs/adr/0014-name-and-trademark-risk.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
