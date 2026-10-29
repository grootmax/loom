//! The core intrinsics (`docs/internals/intrinsics.md`) and MIR built-ins in native code.

use crate::func::{BodyTx, panic_trap};
use crate::{GlueKind, Result, unsupported};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{InstBuilder, Type, Value, types};
use loom_hir::{DefId, Mode};
use loom_mir::{Constant, Operand, Place};
use loom_types::{FloatTy, IntTy, Subst, Ty, TyKind};

/// One arm of a native `select`: kind, channel handle, value buffer, timer nanoseconds,
/// and for a receive arm its output `Option` and that type.
type SelectArmCode = (i64, Value, Value, Value, Option<(Value, Ty)>);

impl BodyTx<'_, '_, '_, '_, '_> {
    /// The address of intrinsic argument `i`: an owned copy for an `own` parameter, the
    /// borrowed value's address otherwise.
    fn arg(&mut self, def: DefId, args: &[Operand], i: usize) -> Result<(Value, Ty)> {
        let Some(op) = args.get(i) else {
            return unsupported("an intrinsic call with too few arguments");
        };
        let mode = self
            .fx
            .cx()
            .items()
            .fns
            .get(&def)
            .and_then(|s| s.params.get(i))
            .map_or(Mode::Borrow, |p| p.0);
        if mode == Mode::Own {
            self.owned_temp(op)
        } else {
            self.arg_ptr(op)
        }
    }

    fn arg_value(&mut self, def: DefId, args: &[Operand], i: usize, t: Type) -> Result<Value> {
        let (addr, _) = self.arg(def, args, i)?;
        Ok(self.fx.load(t, addr, 0))
    }

    fn handle_arg(&mut self, def: DefId, args: &[Operand], i: usize) -> Result<Value> {
        let ptr = self.fx.ptr();
        self.arg_value(def, args, i, ptr)
    }

    /// The code of an operation name (`"add"`, `"upper"`, …), at compile time if constant.
    fn op_code(&mut self, def: DefId, args: &[Operand], i: usize) -> Result<Value> {
        if let Some(Operand::Const(Constant::Str(s), _)) = args.get(i) {
            let code = loom_runtime::ops::name_code(s);
            return Ok(self.fx.b.ins().iconst(types::I32, i64::from(code)));
        }
        let handle = self.handle_arg(def, args, i)?;
        self.fx.rt1("loom_name_code", &[handle])
    }

    fn generic(&self, def: DefId, subst: &Subst, i: usize) -> Ty {
        let cx = self.fx.cx();
        cx.items()
            .generics_of(def)
            .own
            .get(i)
            .and_then(|&p| subst.get(p))
            .unwrap_or_else(|| cx.interner().error())
    }

    fn int_of(&self, ty: Ty) -> Result<IntTy> {
        match self.fx.cx().interner().kind(ty) {
            TyKind::Int(i) => Ok(i),
            _ => unsupported(format!(
                "an integer intrinsic on `{}`",
                self.fx.cx().ty_name(ty)
            )),
        }
    }

    fn float_of(&self, ty: Ty) -> FloatTy {
        match self.fx.cx().interner().kind(ty) {
            TyKind::Float(f) => f,
            _ => FloatTy::F64,
        }
    }

    /// Stores a value widened to `I128` in a new slot.
    fn wide_slot(&mut self, v: Value, int: IntTy) -> Value {
        let t = crate::layout::int_type(int);
        let wide = self.fx.int_convert(v, t, types::I128, int.is_signed());
        let slot = self.fx.slot(16, 16);
        self.fx.store(wide, slot, 0);
        slot
    }

    #[allow(clippy::too_many_lines, reason = "one case per intrinsic")]
    pub(crate) fn intrinsic(
        &mut self,
        name: &str,
        def: DefId,
        subst: &Subst,
        args: &[Operand],
        dest: &Place,
    ) -> Result<()> {
        let ptr = self.fx.ptr();
        let span = self.span;
        let ret_ty = self.place_type(dest)?;
        let ret = self.fx.slot_for(ret_ty);
        let t0 = self.generic(def, subst, 0);
        match name {
            "write_stdout" => {
                let s = self.handle_arg(def, args, 0)?;
                self.fx.rt("loom_write_stdout", &[s])?;
            }
            "panic_with" => {
                let s = self.handle_arg(def, args, 0)?;
                let [file, len, line, column] = self.fx.position(span)?;
                self.fx
                    .rt("loom_panic_string", &[s, file, len, line, column])?;
                self.fx.b.ins().trap(panic_trap());
                self.fx.fresh_block();
                return Ok(());
            }
            "debug_string" => {
                let (v, ty) = self.arg(def, args, 0)?;
                let s = self.fx.rt1("loom_string_new", &[])?;
                let glue = self.fx.glue(GlueKind::Debug, ty)?;
                self.fx.call(glue, &[v, s]);
                self.fx.store(s, ret, 0);
            }
            "value_clone" => {
                let (v, ty) = self.arg(def, args, 0)?;
                self.fx.clone_into(v, ret, ty)?;
            }
            "int_to_string" => {
                let (addr, ty) = self.arg(def, args, 0)?;
                let s = match self.fx.cx().interner().kind(ty) {
                    TyKind::Int(int) => {
                        let v = self.fx.load(crate::layout::int_type(int), addr, 0);
                        self.fx.int_text(v, int)?
                    }
                    TyKind::Char => {
                        let v = self.fx.load(types::I32, addr, 0);
                        self.fx.rt1("loom_char_to_string", &[v])?
                    }
                    _ => {
                        return unsupported(format!(
                            "int_to_string on `{}`",
                            self.fx.cx().ty_name(ty)
                        ));
                    }
                };
                self.fx.store(s, ret, 0);
            }
            "int_parse" => {
                let int = self.int_of(t0)?;
                if int.bits() == 128 {
                    return unsupported("parsing 128-bit integers");
                }
                let s = self.handle_arg(def, args, 0)?;
                let out = self.fx.slot(8, 8);
                let bits = self.fx.b.ins().iconst(types::I32, i64::from(int.bits()));
                let signed = self
                    .fx
                    .b
                    .ins()
                    .iconst(types::I8, i64::from(int.is_signed()));
                let ok = self.fx.rt1("loom_int_parse", &[s, bits, signed, out])?;
                let v = self.fx.load(types::I64, out, 0);
                let t = crate::layout::int_type(int);
                let v = self.fx.int_convert(v, types::I64, t, int.is_signed());
                let some = self.fx.lang_tag("Option", "Some")?;
                let offset = self.fx.payload_offset(ret_ty, some);
                self.fx.store(v, ret, offset);
                self.fx.set_option(ret, ok)?;
            }
            "int_abs" => {
                let int = self.int_of(t0)?;
                let t = crate::layout::int_type(int);
                let v = self.arg_value(def, args, 0, t)?;
                if int.is_signed() && int.bits() <= 64 {
                    #[allow(clippy::cast_possible_truncation, reason = "the minimum fits the type")]
                    let min = int.min() as i64;
                    let overflow = self.fx.b.ins().icmp_imm_s(IntCC::Equal, v, min);
                    self.fx.panic_if(
                        overflow,
                        "attempt to take the absolute value with overflow",
                        span,
                    )?;
                    let negative = self.fx.b.ins().icmp_imm_s(IntCC::SignedLessThan, v, 0);
                    let negated = self.fx.b.ins().ineg(v);
                    let r = self.fx.b.ins().select(negative, negated, v);
                    self.fx.store(r, ret, 0);
                } else if int.is_signed() {
                    return unsupported("128-bit abs");
                } else {
                    self.fx.store(v, ret, 0);
                }
            }
            "int_pow" => {
                let int = self.int_of(t0)?;
                let t = crate::layout::int_type(int);
                let base = self.arg_value(def, args, 0, t)?;
                let exponent = self.arg_value(def, args, 1, types::I32)?;
                let base_slot = self.wide_slot(base, int);
                let out = self.fx.slot(16, 16);
                let bits = self.fx.b.ins().iconst(types::I32, i64::from(int.bits()));
                let signed = self
                    .fx
                    .b
                    .ins()
                    .iconst(types::I8, i64::from(int.is_signed()));
                let ok = self
                    .fx
                    .rt1("loom_int_pow", &[base_slot, exponent, bits, signed, out])?;
                let failed = self.fx.b.ins().icmp_imm_s(IntCC::Equal, ok, 0);
                self.fx
                    .panic_if(failed, "attempt to raise to a power with overflow", span)?;
                let wide = self.fx.load(types::I128, out, 0);
                let r = self.fx.int_convert(wide, types::I128, t, int.is_signed());
                self.fx.store(r, ret, 0);
            }
            "int_checked" | "int_wrapping" | "int_saturating" => {
                let int = self.int_of(t0)?;
                let t = crate::layout::int_type(int);
                let a = self.arg_value(def, args, 0, t)?;
                let b = self.arg_value(def, args, 1, t)?;
                let code = self.op_code(def, args, 2)?;
                let (sa, sb) = (self.wide_slot(a, int), self.wide_slot(b, int));
                let out = self.fx.slot(16, 16);
                let mode = match name {
                    "int_checked" => 0,
                    "int_wrapping" => 1,
                    _ => 2,
                };
                let mode = self.fx.b.ins().iconst(types::I32, mode);
                let bits = self.fx.b.ins().iconst(types::I32, i64::from(int.bits()));
                let signed = self
                    .fx
                    .b
                    .ins()
                    .iconst(types::I8, i64::from(int.is_signed()));
                let ok = self
                    .fx
                    .rt1("loom_int_arith", &[mode, code, sa, sb, bits, signed, out])?;
                let wide = self.fx.load(types::I128, out, 0);
                let r = self.fx.int_convert(wide, types::I128, t, int.is_signed());
                if name == "int_checked" {
                    let some = self.fx.lang_tag("Option", "Some")?;
                    let offset = self.fx.payload_offset(ret_ty, some);
                    self.fx.store(r, ret, offset);
                    self.fx.set_option(ret, ok)?;
                } else {
                    self.fx.store(r, ret, 0);
                }
            }
            "int_to_float" => {
                let int = self.int_of(t0)?;
                let v = self.arg_value(def, args, 0, crate::layout::int_type(int))?;
                let r = if int.is_signed() {
                    self.fx.b.ins().fcvt_from_sint(types::F64, v)
                } else {
                    self.fx.b.ins().fcvt_from_uint(types::F64, v)
                };
                self.fx.store(r, ret, 0);
            }
            "int_min" | "int_max" | "int_zero" => {
                let int = self.int_of(t0)?;
                let t = crate::layout::int_type(int);
                let value = match name {
                    "int_min" => int.min(),
                    "int_max" => int.max().cast_signed(),
                    _ => 0,
                };
                let v = self.fx.int_const(t, value);
                self.fx.store(v, ret, 0);
            }
            "int_add" => {
                let a = &args[0];
                let b = &args[1];
                let value = self.binary_value(loom_hir::BinaryOp::Add, def, args, a, b)?;
                self.fx.store(value, ret, 0);
            }
            "float_to_string" => {
                let float = self.float_of(t0);
                let t = if float == FloatTy::F32 {
                    types::F32
                } else {
                    types::F64
                };
                let v = self.arg_value(def, args, 0, t)?;
                let s = self.fx.float_text(v, float)?;
                self.fx.store(s, ret, 0);
            }
            "float_parse" => {
                let float = self.float_of(t0);
                let s = self.handle_arg(def, args, 0)?;
                let out = self.fx.slot(8, 8);
                let ok = self.fx.rt1("loom_float_parse", &[s, out])?;
                let mut v = self.fx.load(types::F64, out, 0);
                if float == FloatTy::F32 {
                    v = self.fx.b.ins().fdemote(types::F32, v);
                }
                let some = self.fx.lang_tag("Option", "Some")?;
                let offset = self.fx.payload_offset(ret_ty, some);
                self.fx.store(v, ret, offset);
                self.fx.set_option(ret, ok)?;
            }
            "float_math" => {
                let float = self.float_of(t0);
                let t = if float == FloatTy::F32 {
                    types::F32
                } else {
                    types::F64
                };
                let v = self.arg_value(def, args, 0, t)?;
                let r = if let Some(Operand::Const(Constant::Str(op), _)) = args.get(1) {
                    let ins = self.fx.b.ins();
                    match &**op {
                        "sqrt" => ins.sqrt(v),
                        "abs" => ins.fabs(v),
                        "floor" => ins.floor(v),
                        "ceil" => ins.ceil(v),
                        other => return unsupported(format!("float_math `{other}`")),
                    }
                } else {
                    let code = self.op_code(def, args, 1)?;
                    let wide = if float == FloatTy::F32 {
                        self.fx.b.ins().fpromote(types::F64, v)
                    } else {
                        v
                    };
                    let r = self.fx.rt1("loom_float_math", &[wide, code])?;
                    if float == FloatTy::F32 {
                        self.fx.b.ins().fdemote(types::F32, r)
                    } else {
                        r
                    }
                };
                self.fx.store(r, ret, 0);
            }
            "float_round_to" => {
                let float = self.float_of(t0);
                let t = if float == FloatTy::F32 {
                    types::F32
                } else {
                    types::F64
                };
                let mut v = self.arg_value(def, args, 0, t)?;
                let digits = self.arg_value(def, args, 1, types::I64)?;
                if float == FloatTy::F32 {
                    v = self.fx.b.ins().fpromote(types::F64, v);
                }
                let mut r = self.fx.rt1("loom_float_round_to", &[v, digits])?;
                if float == FloatTy::F32 {
                    r = self.fx.b.ins().fdemote(types::F32, r);
                }
                self.fx.store(r, ret, 0);
            }
            "float_truncate" => {
                let float = self.float_of(t0);
                let t = if float == FloatTy::F32 {
                    types::F32
                } else {
                    types::F64
                };
                let mut v = self.arg_value(def, args, 0, t)?;
                if float == FloatTy::F32 {
                    v = self.fx.b.ins().fpromote(types::F64, v);
                }
                let truncated = self.fx.b.ins().trunc(v);
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "the range check of the interpreter"
                )]
                let (low, high) = (i64::MIN as f64, i64::MAX as f64);
                let low = self.fx.b.ins().f64const(low);
                let high = self.fx.b.ins().f64const(high);
                let nan = self.fx.b.ins().fcmp(FloatCC::Unordered, v, v);
                let below = self.fx.b.ins().fcmp(FloatCC::LessThan, truncated, low);
                let above = self
                    .fx
                    .b
                    .ins()
                    .fcmp(FloatCC::GreaterThanOrEqual, truncated, high);
                let out = self.fx.b.ins().bor(nan, below);
                let out = self.fx.b.ins().bor(out, above);
                self.fx
                    .panic_if(out, "the value cannot be converted to `Int`", span)?;
                let r = self.fx.b.ins().fcvt_to_sint(types::I64, truncated);
                self.fx.store(r, ret, 0);
            }
            "char_is" => {
                let c = self.arg_value(def, args, 0, types::I32)?;
                let code = self.op_code(def, args, 1)?;
                let r = self.fx.rt1("loom_char_is", &[c, code])?;
                self.fx.store(r, ret, 0);
            }
            "char_convert" => {
                let c = self.arg_value(def, args, 0, types::I32)?;
                let code = self.op_code(def, args, 1)?;
                let r = self.fx.rt1("loom_char_convert", &[c, code])?;
                self.fx.store(r, ret, 0);
            }
            "char_code" => {
                let c = self.arg_value(def, args, 0, types::I32)?;
                self.fx.store(c, ret, 0);
            }
            "char_to_string" => {
                let c = self.arg_value(def, args, 0, types::I32)?;
                let s = self.fx.rt1("loom_char_to_string", &[c])?;
                self.fx.store(s, ret, 0);
            }
            "string_length" | "string_byte_length" => {
                let s = self.handle_arg(def, args, 0)?;
                let f = if name == "string_length" {
                    "loom_string_length"
                } else {
                    "loom_string_byte_length"
                };
                let r = self.fx.rt1(f, &[s])?;
                self.fx.store(r, ret, 0);
            }
            "string_chars" => {
                let s = self.handle_arg(def, args, 0)?;
                let char_ty = self.fx.cx().interner().intern(TyKind::Char);
                let info = self.fx.type_info(char_ty)?;
                let r = self.fx.rt1("loom_string_chars", &[s, info])?;
                self.fx.store(r, ret, 0);
            }
            "string_from_chars" => {
                let a = self.handle_arg(def, args, 0)?;
                let r = self.fx.rt1("loom_string_from_chars", &[a])?;
                self.fx.store(r, ret, 0);
            }
            "string_push_str" => {
                let s = self.handle_arg(def, args, 0)?;
                let other = self.handle_arg(def, args, 1)?;
                self.fx.rt("loom_string_push_str", &[s, other])?;
            }
            "string_push_char" => {
                let s = self.handle_arg(def, args, 0)?;
                let c = self.arg_value(def, args, 1, types::I32)?;
                self.fx.rt("loom_string_push_char", &[s, c])?;
            }
            "string_convert" => {
                let s = self.handle_arg(def, args, 0)?;
                let code = self.op_code(def, args, 1)?;
                let r = self.fx.rt1("loom_string_convert", &[s, code])?;
                self.fx.store(r, ret, 0);
            }
            "string_split_whitespace" | "string_split" => {
                let s = self.handle_arg(def, args, 0)?;
                let string = self.fx.cx().interner().string();
                let info = self.fx.type_info(string)?;
                let r = if name == "string_split" {
                    let sep = self.handle_arg(def, args, 1)?;
                    self.fx.rt1("loom_string_split", &[s, sep, info])?
                } else {
                    self.fx.rt1("loom_string_split_whitespace", &[s, info])?
                };
                self.fx.store(r, ret, 0);
            }
            "string_find" => {
                let s = self.handle_arg(def, args, 0)?;
                let pattern = self.handle_arg(def, args, 1)?;
                let index = self.fx.rt1("loom_string_find", &[s, pattern])?;
                let some = self.fx.lang_tag("Option", "Some")?;
                let offset = self.fx.payload_offset(ret_ty, some);
                self.fx.store(index, ret, offset);
                let found = self
                    .fx
                    .b
                    .ins()
                    .icmp_imm_s(IntCC::SignedGreaterThanOrEqual, index, 0);
                self.fx.set_option(ret, found)?;
            }
            "string_substring" => {
                let s = self.handle_arg(def, args, 0)?;
                let start = self.arg_value(def, args, 1, types::I64)?;
                let stop = self.arg_value(def, args, 2, types::I64)?;
                let [file, len, line, column] = self.fx.position(span)?;
                let r = self.fx.rt1(
                    "loom_string_substring",
                    &[s, start, stop, file, len, line, column],
                )?;
                self.fx.store(r, ret, 0);
            }
            "string_repeat" => {
                let s = self.handle_arg(def, args, 0)?;
                let count = self.arg_value(def, args, 1, types::I64)?;
                let r = self.fx.rt1("loom_string_repeat", &[s, count])?;
                self.fx.store(r, ret, 0);
            }
            "array_new" => {
                let info = self.fx.type_info(t0)?;
                let r = self.fx.rt1("loom_array_new", &[info])?;
                self.fx.store(r, ret, 0);
            }
            "array_length" => {
                let a = self.handle_arg(def, args, 0)?;
                let r = self.fx.rt1("loom_array_len", &[a])?;
                self.fx.store(r, ret, 0);
            }
            "array_get" => {
                let a = self.handle_arg(def, args, 0)?;
                let index = self.arg_value(def, args, 1, types::I64)?;
                let [file, len, line, column] = self.fx.position(span)?;
                let p = self
                    .fx
                    .rt1("loom_array_get", &[a, index, file, len, line, column])?;
                self.fx.store(p, ret, 0);
            }
            "array_set" => {
                let a = self.handle_arg(def, args, 0)?;
                let index = self.arg_value(def, args, 1, types::I64)?;
                let (value, _) = self.arg(def, args, 2)?;
                let [file, len, line, column] = self.fx.position(span)?;
                let p = self
                    .fx
                    .rt1("loom_array_get", &[a, index, file, len, line, column])?;
                self.fx.drop_at(p, t0)?;
                self.fx.copy_ty(p, value, t0);
            }
            "array_push" => {
                let a = self.handle_arg(def, args, 0)?;
                let (value, _) = self.arg(def, args, 1)?;
                self.fx.rt("loom_array_push", &[a, value])?;
            }
            "array_pop" => {
                let a = self.handle_arg(def, args, 0)?;
                let some = self.fx.lang_tag("Option", "Some")?;
                let payload = self.fx.offset(ret, self.fx.payload_offset(ret_ty, some));
                let ok = self.fx.rt1("loom_array_pop", &[a, payload])?;
                self.fx.set_option(ret, ok)?;
            }
            "array_insert" => {
                let a = self.handle_arg(def, args, 0)?;
                let index = self.arg_value(def, args, 1, types::I64)?;
                let (value, _) = self.arg(def, args, 2)?;
                let [file, len, line, column] = self.fx.position(span)?;
                self.fx.rt(
                    "loom_array_insert",
                    &[a, index, value, file, len, line, column],
                )?;
            }
            "array_remove" => {
                let a = self.handle_arg(def, args, 0)?;
                let index = self.arg_value(def, args, 1, types::I64)?;
                let [file, len, line, column] = self.fx.position(span)?;
                self.fx.rt(
                    "loom_array_remove",
                    &[a, index, ret, file, len, line, column],
                )?;
            }
            "array_slice" => {
                let a = self.handle_arg(def, args, 0)?;
                let start = self.arg_value(def, args, 1, types::I64)?;
                let stop = self.arg_value(def, args, 2, types::I64)?;
                let r = self.fx.rt1("loom_array_slice", &[a, start, stop])?;
                self.fx.store(r, ret, 0);
            }
            "array_reverse" => {
                let a = self.handle_arg(def, args, 0)?;
                self.fx.rt("loom_array_reverse", &[a])?;
            }
            "array_sort_by" => {
                let a = self.handle_arg(def, args, 0)?;
                let (pair, _) = self.arg(def, args, 1)?;
                let code = self.fx.load(ptr, pair, 0);
                let env = self.fx.load(ptr, pair, self.fx.word());
                self.fx.rt("loom_array_sort_by", &[a, code, env])?;
            }
            "map_new" => {
                let t1 = self.generic(def, subst, 1);
                let key_info = self.fx.type_info(t0)?;
                let value_info = self.fx.type_info(t1)?;
                let r = self.fx.rt1("loom_map_new", &[key_info, value_info])?;
                self.fx.store(r, ret, 0);
            }
            "map_length" => {
                let m = self.handle_arg(def, args, 0)?;
                let r = self.fx.rt1("loom_map_len", &[m])?;
                self.fx.store(r, ret, 0);
            }
            "map_insert" => {
                let m = self.handle_arg(def, args, 0)?;
                let (key, _) = self.arg(def, args, 1)?;
                let (value, _) = self.arg(def, args, 2)?;
                let some = self.fx.lang_tag("Option", "Some")?;
                let payload = self.fx.offset(ret, self.fx.payload_offset(ret_ty, some));
                let replaced = self.fx.rt1("loom_map_insert", &[m, key, value, payload])?;
                self.fx.set_option(ret, replaced)?;
            }
            "map_get" => {
                let m = self.handle_arg(def, args, 0)?;
                let (key, _) = self.arg(def, args, 1)?;
                let p = self.fx.rt1("loom_map_get", &[m, key])?;
                let some = self.fx.lang_tag("Option", "Some")?;
                let offset = self.fx.payload_offset(ret_ty, some);
                self.fx.store(p, ret, offset);
                let found = self.fx.b.ins().icmp_imm_s(IntCC::NotEqual, p, 0);
                self.fx.set_option(ret, found)?;
            }
            "map_remove" => {
                let m = self.handle_arg(def, args, 0)?;
                let (key, _) = self.arg(def, args, 1)?;
                let some = self.fx.lang_tag("Option", "Some")?;
                let payload = self.fx.offset(ret, self.fx.payload_offset(ret_ty, some));
                let found = self.fx.rt1("loom_map_remove", &[m, key, payload])?;
                self.fx.set_option(ret, found)?;
            }
            "map_entry_at" => {
                let m = self.handle_arg(def, args, 0)?;
                let index = self.arg_value(def, args, 1, types::I64)?;
                let fields = self.fx.cx().layout(ret_ty).fields;
                let key_out = self.fx.offset(ret, fields.first().map_or(0, |f| f.0));
                let value_out = self.fx.offset(ret, fields.get(1).map_or(0, |f| f.0));
                let [file, len, line, column] = self.fx.position(span)?;
                self.fx.rt(
                    "loom_map_entry",
                    &[m, index, key_out, value_out, file, len, line, column],
                )?;
            }
            "map_update" => {
                let m = self.handle_arg(def, args, 0)?;
                let (key, _) = self.arg(def, args, 1)?;
                let (default, _) = self.arg(def, args, 2)?;
                let (pair, _) = self.arg(def, args, 3)?;
                let code = self.fx.load(ptr, pair, 0);
                let env = self.fx.load(ptr, pair, self.fx.word());
                self.fx
                    .rt("loom_map_update", &[m, key, default, code, env])?;
            }
            "range_next" => self.range_next(def, args, ret, ret_ty, t0)?,
            "scope_run" => {
                let deadline = self.arg_value(def, args, 0, types::I64)?;
                let (pair, _) = self.arg(def, args, 1)?;
                let code = self.fx.load(ptr, pair, 0);
                let env = self.fx.load(ptr, pair, self.fx.word());
                self.fx.rt("loom_scope_run", &[deadline, code, env, ret])?;
            }
            "scope_spawn" => {
                let scope = self.handle_arg(def, args, 0)?;
                let (work, _) = self.arg(def, args, 1)?;
                let code = self.fx.load(ptr, work, 0);
                let env = self.fx.load(ptr, work, self.fx.word());
                let info = self.fx.type_info(t0)?;
                let task = self.fx.rt1("loom_scope_spawn", &[scope, code, env, info])?;
                self.fx.store(task, ret, 0);
            }
            "scope_cancel" => {
                let scope = self.handle_arg(def, args, 0)?;
                self.fx.rt("loom_scope_cancel", &[scope])?;
            }
            "scope_cancelled" => {
                let scope = self.handle_arg(def, args, 0)?;
                let r = self.fx.rt1("loom_scope_cancelled", &[scope])?;
                self.fx.store(r, ret, 0);
            }
            "task_join" => {
                let task = self.handle_arg(def, args, 0)?;
                self.fx.rt("loom_task_join", &[task, ret])?;
            }
            "task_detach" => {
                let (work, _) = self.arg(def, args, 0)?;
                let code = self.fx.load(ptr, work, 0);
                let env = self.fx.load(ptr, work, self.fx.word());
                self.fx.rt("loom_task_detach", &[code, env])?;
            }
            "task_yield" => {
                self.fx.rt("loom_task_yield", &[])?;
            }
            "chan_new" => {
                let info = self.fx.type_info(t0)?;
                let capacity = self.arg_value(def, args, 0, types::I64)?;
                let ch = self.fx.rt1("loom_chan_new", &[info, capacity])?;
                self.fx.store(ch, ret, 0);
            }
            "chan_send" => {
                let ch = self.handle_arg(def, args, 0)?;
                let (value, _) = self.arg(def, args, 1)?;
                let [file, len, line, column] = self.fx.position(span)?;
                let r = self
                    .fx
                    .rt1("loom_chan_send", &[ch, value, file, len, line, column])?;
                self.fx.store(r, ret, 0);
            }
            "chan_recv" | "chan_try_recv" => {
                let ch = self.handle_arg(def, args, 0)?;
                let some = self.fx.lang_tag("Option", "Some")?;
                let payload = self.fx.offset(ret, self.fx.payload_offset(ret_ty, some));
                let wait = self
                    .fx
                    .b
                    .ins()
                    .iconst(types::I8, i64::from(name == "chan_recv"));
                let got = self.fx.rt1("loom_chan_recv", &[ch, payload, wait])?;
                self.fx.set_option(ret, got)?;
            }
            "chan_close" => {
                let ch = self.handle_arg(def, args, 0)?;
                self.fx.rt("loom_chan_close", &[ch])?;
            }
            "time_sleep" => {
                let nanos = self.arg_value(def, args, 0, types::I64)?;
                let r = self.fx.rt1("loom_sleep", &[nanos])?;
                self.fx.store(r, ret, 0);
            }
            "time_now" => {
                let r = self.fx.rt1("loom_time_now", &[])?;
                self.fx.store(r, ret, 0);
            }
            "mutex_new" | "shared_new" => {
                let info = self.fx.type_info(t0)?;
                let (value, _) = self.arg(def, args, 0)?;
                let f = if name == "mutex_new" {
                    "loom_mutex_new"
                } else {
                    "loom_shared_new"
                };
                let h = self.fx.rt1(f, &[info, value])?;
                self.fx.store(h, ret, 0);
            }
            "mutex_lock" => {
                let m = self.handle_arg(def, args, 0)?;
                let (pair, _) = self.arg(def, args, 1)?;
                let code = self.fx.load(ptr, pair, 0);
                let env = self.fx.load(ptr, pair, self.fx.word());
                self.fx.rt("loom_mutex_lock", &[m, code, env, ret])?;
            }
            "shared_clone" | "shared_get" | "shared_count" => {
                let h = self.handle_arg(def, args, 0)?;
                let f = match name {
                    "shared_clone" => "loom_shared_clone",
                    "shared_get" => "loom_shared_get",
                    _ => "loom_shared_count",
                };
                let r = self.fx.rt1(f, &[h])?;
                self.fx.store(r, ret, 0);
            }
            "atomic_new" => {
                let v = self.arg_value(def, args, 0, types::I64)?;
                let h = self.fx.rt1("loom_atomic_new", &[v])?;
                self.fx.store(h, ret, 0);
            }
            "atomic_op" => {
                let h = self.handle_arg(def, args, 0)?;
                let op = self.arg_value(def, args, 1, types::I64)?;
                let a = self.arg_value(def, args, 2, types::I64)?;
                let b = self.arg_value(def, args, 3, types::I64)?;
                let r = self.fx.rt1("loom_atomic_op", &[h, op, a, b])?;
                self.fx.store(r, ret, 0);
            }
            other => return unsupported(format!("intrinsic `{other}`")),
        }
        self.assign_from(dest, ret)
    }

    /// `left op right` for the operands of an intrinsic (with overflow checks).
    fn binary_value(
        &mut self,
        op: loom_hir::BinaryOp,
        def: DefId,
        args: &[Operand],
        _a: &Operand,
        _b: &Operand,
    ) -> Result<Value> {
        let (pa, ta) = self.arg(def, args, 0)?;
        let (pb, _) = self.arg(def, args, 1)?;
        let int = self.int_of(ta)?;
        let t = crate::layout::int_type(int);
        let x = self.fx.load(t, pa, 0);
        let y = self.fx.load(t, pb, 0);
        self.int_binary(op, x, y, int, int)
    }

    /// The next value of an integer or `Char` range, advancing it (spec §7.10).
    fn range_next(
        &mut self,
        def: DefId,
        args: &[Operand],
        ret: Value,
        ret_ty: Ty,
        elem: Ty,
    ) -> Result<()> {
        let (range, range_ty) = self.arg(def, args, 0)?;
        let fields = self.fx.cx().layout(range_ty).fields;
        if fields.len() < 3 {
            return unsupported("a range without start, end and inclusive fields");
        }
        let (signed, t) = match self.fx.cx().interner().kind(elem) {
            TyKind::Int(i) if i.bits() <= 64 => (i.is_signed(), crate::layout::int_type(i)),
            TyKind::Char => (false, types::I32),
            _ => return unsupported(format!("a range of `{}`", self.fx.cx().ty_name(elem))),
        };
        let is_char = matches!(self.fx.cx().interner().kind(elem), TyKind::Char);
        let start = self.fx.load(t, range, fields[0].0);
        let end = self.fx.load(t, range, fields[1].0);
        let inclusive = self.fx.load(types::I8, range, fields[2].0);
        let (lt, eq) = if signed {
            (IntCC::SignedLessThan, IntCC::Equal)
        } else {
            (IntCC::UnsignedLessThan, IntCC::Equal)
        };
        let less = self.fx.b.ins().icmp(lt, start, end);
        let equal = self.fx.b.ins().icmp(eq, start, end);
        let last = self.fx.b.ins().band(equal, inclusive);
        let has_next = self.fx.b.ins().bor(less, last);
        let some = self.fx.lang_tag("Option", "Some")?;
        let offset = self.fx.payload_offset(ret_ty, some);
        self.fx.store(start, ret, offset);
        self.fx.set_option(ret, has_next)?;
        // Advance: start + 1 while start < end, otherwise the range is exhausted.
        let advance = self.fx.b.create_block();
        let finish = self.fx.b.create_block();
        let done = self.fx.b.create_block();
        self.fx.b.ins().brif(less, advance, &[], finish, &[]);
        self.fx.b.switch_to_block(advance);
        let mut next = self.fx.b.ins().iadd_imm_s(start, 1);
        if is_char {
            let surrogate = self.fx.b.ins().icmp_imm_s(IntCC::Equal, next, 0xD800);
            let skip = self.fx.b.ins().iconst(types::I32, 0xE000);
            next = self.fx.b.ins().select(surrogate, skip, next);
        }
        self.fx.store(next, range, fields[0].0);
        self.fx.b.ins().jump(done, &[]);
        self.fx.b.switch_to_block(finish);
        let zero = self.fx.b.ins().iconst(types::I8, 0);
        self.fx.store(zero, range, fields[2].0);
        self.fx.b.ins().jump(done, &[]);
        self.fx.b.switch_to_block(done);
        Ok(())
    }

    /// The MIR built-ins: string building for interpolation and map indexing.
    pub(crate) fn builtin(&mut self, name: &str, args: &[Operand], dest: &Place) -> Result<()> {
        let ptr = self.fx.ptr();
        let span = self.span;
        let ret_ty = self.place_type(dest)?;
        let ret = self.fx.slot_for(ret_ty);
        match name {
            "string_new" => {
                let s = self.fx.rt1("loom_string_new", &[])?;
                self.fx.store(s, ret, 0);
            }
            "string_push" => {
                let target = self.operand_scalar(&args[0])?;
                let s = self.fx.load(ptr, target, 0);
                let (text, _) = self.owned_temp(&args[1])?;
                let text = self.fx.load(ptr, text, 0);
                self.fx.append_and_free(s, text)?;
            }
            "map_index" => {
                let (map, _) = self.arg_ptr(&args[0])?;
                let m = self.fx.load(ptr, map, 0);
                let (key, _) = self.arg_ptr(&args[1])?;
                let [file, len, line, column] = self.fx.position(span)?;
                let p = self
                    .fx
                    .rt1("loom_map_index", &[m, key, file, len, line, column])?;
                self.fx.store(p, ret, 0);
            }
            "map_insert" => {
                let (map, map_ty) = self.arg_ptr(&args[0])?;
                let m = self.fx.load(ptr, map, 0);
                let (key, _) = self.owned_temp(&args[1])?;
                let (value, value_ty) = self.owned_temp(&args[2])?;
                let old = self.fx.slot_for(value_ty);
                let replaced = self.fx.rt1("loom_map_insert", &[m, key, value, old])?;
                if self.fx.needs_drop(value_ty) {
                    let drop_block = self.fx.b.create_block();
                    let cont = self.fx.b.create_block();
                    self.fx.b.ins().brif(replaced, drop_block, &[], cont, &[]);
                    self.fx.b.switch_to_block(drop_block);
                    self.fx.drop_at(old, value_ty)?;
                    self.fx.b.ins().jump(cont, &[]);
                    self.fx.b.switch_to_block(cont);
                }
                let _ = map_ty;
            }
            "select" => self.select(args, ret)?,
            other => return unsupported(format!("built-in `{other}`")),
        }
        self.assign_from(dest, ret)
    }

    /// The built-in `select` (spec §11.3; the arguments are described at its lowering in
    /// `loom_mir`): fills an array of runtime arms, calls `loom_select`, and completes the
    /// chosen receive arm's `Option`. Writes the chosen arm's index to `ret`.
    fn select(&mut self, args: &[Operand], ret: Value) -> Result<()> {
        let ptr = self.fx.ptr();
        let word = self.fx.word();
        let span = self.span;
        let constant = |op: &Operand| match op {
            Operand::Const(Constant::Int(v, _), _) => *v,
            _ => -1,
        };
        let has_else = constant(&args[0]) != 0;
        let mut arms: Vec<SelectArmCode> = Vec::new();
        let mut i = 1;
        while i < args.len() {
            let kind = constant(&args[i]);
            match kind {
                0 => {
                    let (ch_ptr, _) = self.arg_ptr(&args[i + 1])?;
                    let ch = self.fx.load(ptr, ch_ptr, 0);
                    let (out, option_ty) = self.arg_ptr(&args[i + 2])?;
                    let elem = match self.fx.cx().interner().kind(option_ty) {
                        TyKind::Adt(_, a) if !a.is_empty() => a[0],
                        _ => return unsupported("a receive arm without an `Option`"),
                    };
                    let buf = self.fx.slot_for(elem);
                    let zero = self.fx.b.ins().iconst(types::I64, 0);
                    arms.push((0, ch, buf, zero, Some((out, option_ty))));
                    i += 3;
                }
                1 => {
                    let (ch_ptr, _) = self.arg_ptr(&args[i + 1])?;
                    let ch = self.fx.load(ptr, ch_ptr, 0);
                    let (value, _) = self.owned_temp(&args[i + 2])?;
                    let zero = self.fx.b.ins().iconst(types::I64, 0);
                    arms.push((1, ch, value, zero, None));
                    i += 3;
                }
                _ => {
                    let (timeout, _) = self.arg_ptr(&args[i + 1])?;
                    let nanos = self.fx.load(types::I64, timeout, 0);
                    let null = self.fx.pconst(0);
                    arms.push((2, null, null, nanos, None));
                    i += 2;
                }
            }
        }
        let stride = 4 * word;
        let count = u32::try_from(arms.len()).unwrap_or(0);
        let table = self.fx.slot(stride * count.max(1), word);
        for (k, (kind, ch, data, nanos, _)) in arms.iter().enumerate() {
            let base = u32::try_from(k).unwrap_or(0) * stride;
            let kind_v = self.fx.b.ins().iconst(types::I32, *kind);
            self.fx.store(kind_v, table, base);
            self.fx.store(*ch, table, base + word);
            self.fx.store(*data, table, base + 2 * word);
            self.fx.store(*nanos, table, base + 3 * word);
        }
        let received = self.fx.slot(1, 1);
        let count_v = self.fx.pconst(i64::from(count));
        let else_v = self.fx.b.ins().iconst(types::I8, i64::from(has_else));
        let [file, len, line, column] = self.fx.position(span)?;
        let chosen = self.fx.rt1(
            "loom_select",
            &[table, count_v, else_v, received, file, len, line, column],
        )?;
        self.fx.store(chosen, ret, 0);
        // Complete the chosen receive arm: `Some(value)` if a value arrived.
        for (k, (_, _, buf, _, out)) in arms.iter().enumerate() {
            let Some((out, option_ty)) = out else {
                continue;
            };
            let is_this = self.fx.b.ins().icmp_imm_s(
                cranelift_codegen::ir::condcodes::IntCC::Equal,
                chosen,
                i64::try_from(k).unwrap_or(0),
            );
            let value_arrived = self.fx.load(types::I8, received, 0);
            let both = self.fx.b.ins().band(is_this, value_arrived);
            let fill_block = self.fx.b.create_block();
            let next_block = self.fx.b.create_block();
            self.fx.b.ins().brif(both, fill_block, &[], next_block, &[]);
            self.fx.b.switch_to_block(fill_block);
            let some = self.fx.lang_tag("Option", "Some")?;
            let payload = self
                .fx
                .offset(*out, self.fx.payload_offset(*option_ty, some));
            let elem = match self.fx.cx().interner().kind(*option_ty) {
                TyKind::Adt(_, a) => a[0],
                _ => *option_ty,
            };
            self.fx.copy_ty(payload, *buf, elem);
            let tag = self.fx.b.ins().iconst(types::I32, i64::from(some));
            self.fx.store(tag, *out, 0);
            self.fx.b.ins().jump(next_block, &[]);
            self.fx.b.switch_to_block(next_block);
        }
        Ok(())
    }
}
