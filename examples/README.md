# Examples

Example programs written in the intended Loom syntax. They drive the parser in Phase 1
and become runnable as the roadmap progresses. CI will run every example whose phase has
been reached and compare its output byte-for-byte with the `.stdout` file next to it, on
the interpreter and on every native backend (differential testing).

## Header

Every example starts with:

```loom
# phase: N        earliest roadmap phase that can run it (see docs/roadmap.md)
# run: MODE       run | test | build-only
# args: ...       optional command-line arguments
```

- `run` — `loom run <file> -- <args>`; stdout must equal `<name>.stdout`.
- `test` — `loom test <file>`; stdout must equal `<name>.stdout`. The test-runner output
  format is tentative until Phase 2.
- `build-only` — must compile; not executed (servers). `<name>.stdout` is empty.

## Notes

- Std APIs used here (`std.cli`, `std.http`, `std.json`, ...) are the intended shape, not a
  commitment; their final API is decided by std RFCs in Phase 5. Examples are updated to
  match.
- Examples must be deterministic. Concurrent examples sort or otherwise order their output.
