# Conformance tests for Expressions (§7)

| Case | Rule | Accept / Reject |
|---|---|---|
| `7_5_operator_precedence.loom` | §7.5 | Accept |
| `7_5_not_precedence.loom` | §7.5 | Accept |
| `7_6_if_expression.loom` | §7.6 | Accept |
| `7_6_if_in_pattern.loom` | §7.6 | Accept |
| `7_7_case_expression.loom` | §7.7 | Accept |
| `7_8_loops.loom` | §7.8 | Accept |
| `7_8_while_in_pattern.loom` | §7.8 | Accept |
| `7_9_closures.loom` | §7.9 | Accept |
| `7_3_trailing_blocks.loom` | §7.3 | Accept |
| `7_13_tuples.loom` | §7.13 | Accept |
| `7_13_array_map_literals.loom` | §7.13 | Accept |
| `7_11_try_operator.loom` | §7.11 | Accept |
| `7_13_bracket_newlines.loom` | §2.3 | Accept |
| `7_14_return.loom` | §7.14 | Accept |
| `7_15_select_expression.loom` | §11.3 | Accept |
| `7_6_missing_then_if.loom` | §7.6 | Reject (E0100) |
| `7_5_chained_comparison.loom` | §7.5 | Reject (E0104) |
| `7_5_chained_range.loom` | §7.5 | Reject (E0105) |
| `7_6_assignment_in_expr.loom` | §7.5 | Reject (E0106) |
| `7_3_positional_after_named.loom` | §3.1 | Reject (E0107) |
| `7_6_else_if.loom` | §7.6 | Reject (E0110) |
