# Conformance tests for Items (§3)

| Case | Rule | Accept / Reject |
|---|---|---|
| `3_1_fn_def.loom` | §3.1 | Accept |
| `3_1_fn_generics_where.loom` | §3.1 | Accept |
| `3_1_receiver_modes.loom` | §3.1 | Accept |
| `3_1_params_defaults_kwargs.loom` | §3.1 | Accept |
| `3_2_struct_def.loom` | §3.2 | Accept |
| `3_3_enum_def.loom` | §3.3 | Accept |
| `3_4_trait_def.loom` | §3.4 | Accept |
| `3_5_impl_def.loom` | §3.5 | Accept |
| `3_6_module_use.loom` | §3.6 | Accept |
| `3_8_const_def.loom` | §3.8 | Accept |
| `3_9_type_alias.loom` | §3.9 | Accept |
| `3_10_attributes.loom` | §3.10 | Accept |
| `3_11_visibility.loom` | §3.11 | Accept |
| `3_1_missing_fn_name.loom` | §3.1 | Reject (E0100) |
| `3_2_missing_struct_name.loom` | §3.2 | Reject (E0100) |
| `3_8_unclosed_delimiter.loom` | §3.8 | Reject (E0101) |
| `3_1_missing_end.loom` | §3.1 | Reject (E0102) |
| `3_10_inner_doc_after_item.loom` | §2.4 | Reject (E0108) |
| `3_5_impl_without_trait.loom` | §3.5 | Reject (E0109) |
| `3_7_glob_import.loom` | §3.7 | Reject (E0113) |
