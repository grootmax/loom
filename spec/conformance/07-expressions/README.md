# Conformance tests for Expressions (§7)

| Case | Rule | Accept / Reject |
|---|---|---|
| `7_1_primitive_literals.loom` | §7.1 rule 1 | Accept |
| `7_1_eval_order_arguments.loom` | §7.1 rule 1 | Accept |
| `7_1_eval_order_operands.loom` | §7.1 rule 1 | Accept |
| `7_1_literal_out_of_range.loom` | §7.1 rule 1 | Reject (E0310) |
| `7_2_associated_item_path.loom` | §7.2 rule 1 | Accept |
| `7_2_unknown_associated_item.loom` | §7.2 rule 1 | Reject (E0330) |
| `7_3_trailing_blocks.loom` | §7.3 | Accept |
| `7_3_struct_constructor_call.loom` | §7.3 rule 1 | Accept |
| `7_3_callable_value.loom` | §7.3 rule 1 | Accept |
| `7_3_positional_after_named.loom` | §3.1 | Reject (E0107) |
| `7_3_call_non_callable.loom` | §7.3 rule 1 | Reject (E0308) |
| `7_3_call_wrong_arg_count.loom` | §7.3 rule 1 | Reject (E0301) |
| `7_4_uniform_access_field.loom` | §7.4 rule 1 | Accept |
| `7_4_uniform_access_method.loom` | §7.4 rule 1 | Accept |
| `7_4_at_field.loom` | §7.4 rule 1 | Accept |
| `7_4_tuple_indexing.loom` | §7.4 rule 1 | Accept |
| `7_4_field_callable_call.loom` | §7.4 rule 2 | Accept |
| `7_4_array_indexing.loom` | §7.4 rule 5 | Accept |
| `7_4_map_indexing.loom` | §7.4 rule 5 | Accept |
| `7_4_array_oob_panic.loom` | §7.4 rule 5 | Accept (exit 101) |
| `7_4_map_missing_key_panic.loom` | §7.4 rule 5 | Accept (exit 101) |
| `7_4_unknown_field.loom` | §7.4 rule 1 | Reject (E0304) |
| `7_4_no_method_found.loom` | §7.4 rule 3 | Reject (E0306) |
| `7_4_type_not_indexable.loom` | §7.4 rule 5 | Reject (E0309) |
| `7_4_at_field_outside_method.loom` | §7.4 rule 1 | Reject (E0207) |
| `7_4_mutate_immutable_place.loom` | §7.4 rule 5 | Reject (E0312) |
| `7_5_operator_precedence.loom` | §7.5 | Accept |
| `7_5_not_precedence.loom` | §7.5 | Accept |
| `7_5_not_binds_loosely.loom` | §7.5 | Accept |
| `7_5_overflow_panics.loom` | §7.5 rule 1 | Accept (exit 101) |
| `7_5_arithmetic_ops.loom` | §7.5 rule 1 | Accept |
| `7_5_division_by_zero_panic.loom` | §7.5 rule 1 | Accept (exit 101) |
| `7_5_shift_out_of_range_panic.loom` | §7.5 rule 2 | Accept (exit 101) |
| `7_5_bitwise_ops.loom` | §7.5 rule 2 | Accept |
| `7_5_logical_short_circuit.loom` | §7.5 rule 4 | Accept |
| `7_5_in_pattern.loom` | §7.5 rule 5 | Accept |
| `7_5_float_ieee754.loom` | §7.5 rule 3 | Accept |
| `7_5_bitwise_bool.loom` | §7.5 rule 2 | Accept |
| `7_5_chained_comparison.loom` | §7.5 | Reject (E0104) |
| `7_5_chained_range.loom` | §7.5 | Reject (E0105) |
| `7_5_mismatched_operand_types.loom` | §7.5 rule 1 | Reject (E0300) |
| `7_5_bitwise_not_non_int.loom` | §7.5 rule 2 | Reject (E0311) |
| `7_6_if_expression.loom` | §7.6 | Accept |
| `7_6_if_in_pattern.loom` | §7.6 | Accept |
| `7_6_if_elsif_else_value.loom` | §7.6 rule 1 | Accept |
| `7_6_if_without_else_unit.loom` | §7.6 rule 2 | Accept |
| `7_6_missing_then_if.loom` | §7.6 | Reject (E0100) |
| `7_6_assignment_in_expr.loom` | §7.5 | Reject (E0106) |
| `7_6_else_if.loom` | §7.6 | Reject (E0110) |
| `7_6_if_non_bool_cond.loom` | §7.6 rule 1 | Reject (E0300) |
| `7_6_if_branch_type_mismatch.loom` | §7.6 rule 2 | Reject (E0300) |
| `7_7_case_expression.loom` | §7.7 | Accept |
| `7_7_case_arm_guards.loom` | §7.7 rule 1 | Accept |
| `7_7_case_eval_scrutinee_once.loom` | §7.7 rule 1 | Accept |
| `7_7_case_non_exhaustive.loom` | §7.7 rule 1 | Reject (E0400) |
| `7_7_case_arm_type_mismatch.loom` | §7.7 rule 1 | Reject (E0300) |
| `7_8_loops.loom` | §7.8 | Accept |
| `7_8_while_in_pattern.loom` | §7.8 | Accept |
| `7_8_loop_break_value.loom` | §7.8 rule 1 | Accept |
| `7_8_while_next.loom` | §7.8 rule 2 | Accept |
| `7_8_for_loop_iterator.loom` | §7.8 rule 3 | Accept |
| `7_8_break_outside_loop.loom` | §7.8 rule 4 | Reject (E0208) |
| `7_8_next_outside_loop.loom` | §7.8 rule 4 | Reject (E0208) |
| `7_8_break_value_in_while.loom` | §7.8 rule 4 | Reject (E0314) |
| `7_8_break_value_in_for.loom` | §7.8 rule 4 | Reject (E0314) |
| `7_9_closures.loom` | §7.9 | Accept |
| `7_9_closure_mutates_capture.loom` | §7.9 | Accept |
| `7_9_closure_type_annotated.loom` | §7.9 rule 1 | Accept |
| `7_9_closure_return_expr.loom` | §7.9 rule 4 | Accept |
| `7_9_closure_type_annotations_needed.loom` | §7.9 rule 1 | Reject (E0282) |
| `7_9_break_across_closure.loom` | §7.9 rule 4 | Reject (E0208) |
| `7_10_range_exclusive_inclusive.loom` | §7.10 rule 1 | Accept |
| `7_10_range_type_mismatch.loom` | §7.10 rule 1 | Reject (E0300) |
| `7_11_try_operator.loom` | §7.11 | Accept |
| `7_11_try_on_non_result.loom` | §7.11 rule 1 | Reject (E0313) |
| `7_12_display_of_floats.loom` | §7.12 | Accept |
| `7_12_interpolation_primitives.loom` | §7.12 rule 1 | Accept |
| `7_12_interpolation_eval_order.loom` | §7.12 rule 1 | Accept |
| `7_12_interpolation_non_display.loom` | §7.12 rule 1 | Reject (E0320) |
| `7_13_tuples.loom` | §7.13 | Accept |
| `7_13_array_map_literals.loom` | §7.13 | Accept |
| `7_13_bracket_newlines.loom` | §2.3 | Accept |
| `7_13_map_duplicate_key_overwrite.loom` | §7.13 rule 1 | Accept |
| `7_13_multiline_array_map.loom` | §7.13 rule 1 | Accept |
| `7_13_array_elem_type_mismatch.loom` | §7.13 rule 1 | Reject (E0300) |
| `7_13_map_key_type_mismatch.loom` | §7.13 rule 1 | Reject (E0300) |
| `7_13_map_val_type_mismatch.loom` | §7.13 rule 1 | Reject (E0300) |
| `7_14_return.loom` | §7.14 | Accept |
| `7_14_return_early_value.loom` | §7.14 rule 1 | Accept |
| `7_14_return_type_mismatch.loom` | §7.14 rule 1 | Reject (E0300) |
| `7_15_select_expression.loom` | §11.3 | Accept |
