# Core intrinsics

Operations the core library cannot express in Loom are declared in
`extern "intrinsic" … end` blocks in `std/core/*.loom` (ADR 0019 §4) and implemented by
`compiler/loom_interp/src/intrinsics.rs`; the Phase 3 backends implement the same list.
Adding an intrinsic needs a row here.

| Intrinsic | File | Meaning |
|---|---|---|
| `write_stdout(text)` | io | Append to the buffered standard output |
| `panic_with(message) -> Never` | io, option, array | Panic with `message` |
| `debug_string[T](value) -> String` | io | Structural rendering for `assert_eq` |
| `value_clone[T](value) -> T` | string | Copy a value (copy-on-write) |
| `int_to_string`, `int_parse`, `int_abs`, `int_pow` | primitives | Integer text and arithmetic; overflow panics |
| `int_checked`, `int_wrapping`, `int_saturating` | primitives | `add`/`sub`/`mul`/`div` with the named overflow behavior |
| `int_to_float`, `int_min`, `int_max`, `int_zero`, `int_add` | primitives | Conversions, bounds, zero and checked addition of the type argument |
| `float_to_string`, `float_parse` | primitives | Float text (spec §7.12) and parsing |
| `float_math(value, operation)` | primitives | `sqrt`, `abs`, `floor`, `ceil` |
| `float_round_to(value, digits)`, `float_truncate(value)` | primitives | Rounding (halves away from zero); truncation to `Int` (panics if out of range) |
| `char_is(value, class)`, `char_convert`, `char_code`, `char_to_string` | primitives | Character classes and case mapping |
| `string_length`, `string_byte_length`, `string_chars`, `string_from_chars` | string | Lengths in scalar values and bytes; conversion to and from characters |
| `string_push_str`, `string_push_char` | string | Append (through a `mut` place) |
| `string_convert(text, operation)` | string | `upper`, `lower`, `trim`, `trim_start`, `trim_end` |
| `string_split_whitespace`, `string_split`, `string_find`, `string_substring`, `string_repeat` | string | Splitting and searching by scalar-value index |
| `array_new`, `array_length`, `array_get`, `array_set` | array | Creation and element access (out of bounds panics) |
| `array_push`, `array_pop`, `array_insert`, `array_remove`, `array_reverse` | array | Mutation through a `mut` place |
| `array_slice(array, start, stop)` | array | A copy of a clamped range |
| `array_sort_by(array, compare)` | array | Stable merge sort calling `compare` |
| `map_new`, `map_length`, `map_get`, `map_entry_at` | map | Insertion-ordered hash map access |
| `map_insert`, `map_remove`, `map_update` | map | Mutation; `map_update` passes the entry's place to the closure |
| `range_next(range)` | iter | The next value of an integer or `Char` range |
