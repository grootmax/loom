# Conformance tests for Statements (§4)

| Case | Rule | Accept / Reject |
|---|---|---|
| `4_1_bindings.loom` | §4.1 | Accept |
| `4_2_assignments.loom` | §4.2 | Accept |
| `4_2_compound_assignment.loom` | §4.2 | Accept |
| `4_4_command_call.loom` | §4.4 | Accept |
| `4_missing_in_for.loom` | §7.8 | Reject (E0100) |
| `4_range_stmt_start.loom` | §2.3 | Reject (E0100) |
| `4_stray_closing_paren.loom` | §4 | Reject (E0103) |
| `4_two_stmts_one_line.loom` | §2.3 | Reject (E0111) |
