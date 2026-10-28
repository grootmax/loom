# Lexical Conformance Tests (§2)

| Case | Rule | Type |
|---|---|---|
| `2_1_1_invalid_utf8_byte_invalid_utf8.loom` | §2.1.1 rule 1 | reject |
| `2_1_2_bom_ignored.loom` | §2.1.2 rule 2 | accept |
| `2_1_3_shebang_ignored.loom` | §2.1.3 rule 3 | accept |
| `2_1_3_shebang_inner_attribute_reserved.loom` | §2.1.3 rule 3 | reject |
| `2_1_4_bidi_character.loom` | §2.1.4 rule 4 | reject |
| `2_2_1_crlf_line_endings_crlf.loom` | §2.2.1 rule 1 | accept |
| `2_2_1_lone_carriage_return.loom` | §2.2.1 rule 1 | reject |
| `2_2_2_non_ascii_whitespace.loom` | §2.2.2 rule 2 | reject |
| `2_3_continuation_binary_operators.loom` | §2.3 rule 1 | accept |
| `2_3_continuation_comma_dot.loom` | §2.3 rule 1 | accept |
| `2_3_continuation_comment_line.loom` | §2.3 rule 1 | accept |
| `2_3_continuation_signature_arrow.loom` | §2.3 rule 1 | accept |
| `2_3_leading_dot_chain.loom` | §2.3 rule 1 | accept |
| `2_3_semicolon_prohibited.loom` | §2.3 rule 4 | reject |
| `2_4_comments_and_doc_comments.loom` | §2.4 rules 1-3 | accept |
| `2_5_unexpected_character.loom` | §2.5 rule 7 | reject |
| `2_5_unicode_and_default_identifiers.loom` | §2.5 rules 1, 5 | accept |
| `2_6_reserved_word_yield.loom` | §2.6 rule 3 | reject |
| `2_7_1_binary_invalid_digit.loom` | §2.7.1 rule 1 | reject |
| `2_7_1_hex_missing_digits.loom` | §2.7.1 rule 1 | reject |
| `2_7_1_integer_literals.loom` | §2.7.1 rules 1-3 | accept |
| `2_7_1_invalid_numeric_suffix.loom` | §2.7.1 rule 4 | reject |
| `2_7_1_leading_zero.loom` | §2.7.1 rule 2 | reject |
| `2_7_1_misplaced_underscore.loom` | §2.7.1 rule 1 | reject |
| `2_7_1_octal_invalid_digit.loom` | §2.7.1 rule 1 | reject |
| `2_7_1_uppercase_hex_prefix.loom` | §2.7.1 rule 3 | reject |
| `2_7_2_tuple_field_access.loom` | §2.7.2 rule 3 | accept |
| `2_7_3_empty_char_literal.loom` | §2.7.3 rule 2 | reject |
| `2_7_3_multi_char_literal.loom` | §2.7.3 rule 2 | reject |
| `2_7_3_unterminated_char_literal.loom` | §2.7.3 rule 1 | reject |
| `2_7_4_non_ascii_byte_string.loom` | §2.7.4 rule 5 | reject |
| `2_7_4_raw_and_byte_strings.loom` | §2.7.4 rules 4-5 | accept |
| `2_7_4_string_escapes_and_interp.loom` | §2.7.4 rule 2 | accept |
| `2_7_4_triple_quote_closing_not_on_own_line.loom` | §2.7.4 rule 3 | reject |
| `2_7_4_triple_quote_missing_indentation.loom` | §2.7.4 rule 3 | reject |
| `2_7_4_triple_quote_no_newline.loom` | §2.7.4 rule 3 | reject |
| `2_7_4_triple_quoted_strings.loom` | §2.7.4 rule 3 | accept |
| `2_7_4_unterminated_raw_string.loom` | §2.7.4 rule 4 | reject |
| `2_7_4_unterminated_string_literal.loom` | §2.7.4 rule 1 | reject |
| `2_7_5_hex_escape_invalid_digit.loom` | §2.7.5 | reject |
| `2_7_5_hex_escape_non_ascii_in_string.loom` | §2.7.5 | reject |
| `2_7_5_unicode_escape_above_max.loom` | §2.7.5 | reject |
| `2_7_5_unicode_escape_no_digits.loom` | §2.7.5 | reject |
| `2_7_5_unicode_escape_surrogate.loom` | §2.7.5 | reject |
| `2_7_5_unicode_escape_too_many_digits.loom` | §2.7.5 | reject |
| `2_7_5_unknown_escape_sequence.loom` | §2.7.5 | reject |
| `2_8_and_and_operator.loom` | §2.8 rule 4 | reject |
| `2_8_empty_closure_params.loom` | §2.8 rule 4 | accept |
| `2_8_exclamation_mark_prohibited.loom` | §2.8 rule 3 | reject |
| `2_8_or_or_operator.loom` | §2.8 rule 4 | reject |
