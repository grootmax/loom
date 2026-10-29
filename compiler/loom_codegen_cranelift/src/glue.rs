//! Per-type glue (drop, clone, equality, comparison, hashing, debug text), the type
//! descriptors the runtime's collections use, and `dyn` vtables (ADR 0021 §3, §4).

use crate::func::{Fx, panic_trap};
use crate::{
    Cx, DataKey, Gen, GlueKind, Instance, Resolved, Result, internal, subst_key, unsupported,
};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{AbiParam, Block, InstBuilder, Signature, Type, Value, types};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::Switch;
use cranelift_module::{DataDescription, DataId, Linkage, Module};
use loom_hir::{DefId, DefKind};
use loom_mir::MirSource;
use loom_types::{FloatTy, IntTy, Subst, Ty, TyKind};

/// The index of the first method in a vtable: drop, size, align and clone come first.
pub(crate) const VTABLE_METHODS: u32 = 4;

/// The signature of a glue function.
pub(crate) fn signature(kind: GlueKind, ptr: Type, cc: CallConv) -> Signature {
    let mut sig = Signature::new(cc);
    match kind {
        GlueKind::Drop => sig.params.push(AbiParam::new(ptr)),
        GlueKind::Clone | GlueKind::Debug => {
            sig.params.push(AbiParam::new(ptr));
            sig.params.push(AbiParam::new(ptr));
        }
        GlueKind::Eq => {
            sig.params.push(AbiParam::new(ptr));
            sig.params.push(AbiParam::new(ptr));
            sig.returns.push(AbiParam::new(types::I8).uext());
        }
        GlueKind::Cmp => {
            sig.params.push(AbiParam::new(ptr));
            sig.params.push(AbiParam::new(ptr));
            sig.returns.push(AbiParam::new(types::I8).sext());
        }
        GlueKind::Hash => {
            sig.params.push(AbiParam::new(ptr));
            sig.returns.push(AbiParam::new(types::I64));
        }
    }
    sig
}

/// The type descriptor of `ty`: size, alignment, then the six glue functions.
pub(crate) fn type_info(g: &mut Gen<'_, '_>, ty: Ty) -> Result<DataId> {
    let key = DataKey::TypeInfo(ty);
    if let Some(&id) = g.data.get(&key) {
        return Ok(id);
    }
    let layout = g.cx.layout(ty);
    let word = g.cx.ptr_ty.bytes() as usize;
    g.counter += 1;
    let id = g
        .module
        .declare_data(
            &format!("loom_t{}", g.counter),
            Linkage::Local,
            false,
            false,
        )
        .map_err(internal)?;
    g.data.insert(key, id);
    let mut bytes = vec![0u8; word * 8];
    write_word(&mut bytes, 0, u64::from(layout.size), word);
    write_word(&mut bytes, word, u64::from(layout.align), word);
    let mut description = DataDescription::new();
    description.define(bytes.into_boxed_slice());
    description.set_align(word as u64);
    let kinds = [
        GlueKind::Drop,
        GlueKind::Clone,
        GlueKind::Eq,
        GlueKind::Cmp,
        GlueKind::Hash,
        GlueKind::Debug,
    ];
    for (i, kind) in kinds.into_iter().enumerate() {
        let f = g.func(Instance::Glue(kind, ty))?;
        let r = g.module.declare_func_in_data(f, &mut description);
        description.write_function_addr(u32::try_from((2 + i) * word).unwrap_or(0), r);
    }
    g.module.define_data(id, &description).map_err(internal)?;
    Ok(id)
}

fn write_word(bytes: &mut [u8], offset: usize, value: u64, word: usize) {
    let le = value.to_ne_bytes();
    bytes[offset..offset + word].copy_from_slice(&le[..word]);
}

/// The methods of `trait_` that take a receiver, in vtable order (by name).
fn vtable_methods(cx: &Cx<'_>, trait_: DefId) -> Vec<DefId> {
    let Some(info) = cx.items().traits.get(&trait_) else {
        return Vec::new();
    };
    let mut methods: Vec<(String, DefId)> = info
        .methods
        .iter()
        .filter(|&(_, &m)| matches!(&cx.hir.def(m).kind, DefKind::Fn(f) if f.receiver.is_some()))
        .map(|(n, &m)| (n.to_string(), m))
        .collect();
    methods.sort();
    methods.into_iter().map(|(_, m)| m).collect()
}

/// The vtable slot of `method` of `trait_`.
pub(crate) fn vtable_slot(cx: &Cx<'_>, trait_: DefId, method: DefId) -> Option<u32> {
    vtable_methods(cx, trait_)
        .iter()
        .position(|&m| m == method)
        .and_then(|p| u32::try_from(p).ok())
}

/// The vtable of concrete type `ty` for trait object type `dyn_ty`.
pub(crate) fn vtable(g: &mut Gen<'_, '_>, ty: Ty, dyn_ty: Ty) -> Result<DataId> {
    let key = DataKey::Vtable(ty, dyn_ty);
    if let Some(&id) = g.data.get(&key) {
        return Ok(id);
    }
    let TyKind::Dyn(trait_, trait_args) = g.cx.interner().kind(dyn_ty) else {
        return unsupported("a vtable for a type that is not `dyn`");
    };
    let layout = g.cx.layout(ty);
    let word = g.cx.ptr_ty.bytes() as usize;
    let methods = vtable_methods(&g.cx, trait_);
    g.counter += 1;
    let id = g
        .module
        .declare_data(
            &format!("loom_v{}", g.counter),
            Linkage::Local,
            false,
            false,
        )
        .map_err(internal)?;
    g.data.insert(key, id);
    let mut bytes = vec![0u8; word * (VTABLE_METHODS as usize + methods.len())];
    write_word(&mut bytes, word, u64::from(layout.size), word);
    write_word(&mut bytes, 2 * word, u64::from(layout.align), word);
    let mut description = DataDescription::new();
    description.define(bytes.into_boxed_slice());
    description.set_align(word as u64);
    let drop = g.func(Instance::Glue(GlueKind::Drop, ty))?;
    let r = g.module.declare_func_in_data(drop, &mut description);
    description.write_function_addr(0, r);
    let clone = g.func(Instance::Glue(GlueKind::Clone, ty))?;
    let r = g.module.declare_func_in_data(clone, &mut description);
    description.write_function_addr(u32::try_from(3 * word).unwrap_or(0), r);
    let mut subst = Subst::new();
    for (&p, &a) in g.cx.items().generics_of(trait_).own.iter().zip(&trait_args) {
        subst.insert(p, a);
    }
    for (i, &method) in methods.iter().enumerate() {
        let Resolved::Fn(def, s) = g.cx.resolve_trait_method(method, ty, &subst)? else {
            return unsupported("a built-in method in a vtable");
        };
        let Some(body) = g.cx.fn_body(def) else {
            return unsupported("a vtable method without a body");
        };
        let f = g.func(Instance::Body(MirSource::Item(body), subst_key(&s)))?;
        let r = g.module.declare_func_in_data(f, &mut description);
        let offset = (VTABLE_METHODS as usize + i) * word;
        description.write_function_addr(u32::try_from(offset).unwrap_or(0), r);
    }
    g.module.define_data(id, &description).map_err(internal)?;
    Ok(id)
}

/// The structure of a concrete type, for glue.
enum Shape {
    Int(IntTy),
    Float(FloatTy),
    Bool,
    Char,
    Unit,
    String,
    Array,
    Map,
    Ref(Ty),
    Record(Vec<(u32, Ty)>, Option<DefId>),
    Enum(DefId, Vec<Vec<(u32, Ty)>>),
    Callable,
    Dyn,
}

fn shape(cx: &Cx<'_>, ty: Ty) -> Shape {
    let interner = cx.interner();
    match interner.kind(ty) {
        TyKind::Int(i) => Shape::Int(i),
        TyKind::Float(f) => Shape::Float(f),
        TyKind::Bool => Shape::Bool,
        TyKind::Char => Shape::Char,
        TyKind::String | TyKind::Bytes => Shape::String,
        TyKind::Ref { inner, .. } => Shape::Ref(inner),
        TyKind::Tuple(_) => Shape::Record(cx.layout(ty).fields, None),
        TyKind::Callable { .. } => Shape::Callable,
        TyKind::Dyn(..) => Shape::Dyn,
        TyKind::Adt(def, _) => match cx.hir.def(def).lang() {
            Some("Array") => Shape::Array,
            Some("Map") => Shape::Map,
            _ => match &cx.hir.def(def).kind {
                DefKind::Enum(_) => Shape::Enum(def, cx.layout(ty).variants),
                DefKind::Struct(_) => Shape::Record(cx.layout(ty).fields, Some(def)),
                _ => Shape::Unit,
            },
        },
        _ => Shape::Unit,
    }
}

/// Emits the body of glue `kind` for `ty`.
pub(crate) fn translate(
    fx: &mut Fx<'_, '_, '_, '_>,
    kind: GlueKind,
    ty: Ty,
    params: &[Value],
) -> Result<()> {
    match kind {
        GlueKind::Drop => {
            drop_glue(fx, ty, params[0])?;
            fx.b.ins().return_(&[]);
        }
        GlueKind::Clone => {
            clone_glue(fx, ty, params[0], params[1])?;
            fx.b.ins().return_(&[]);
        }
        GlueKind::Eq => {
            let r = eq_glue(fx, ty, params[0], params[1])?;
            fx.b.ins().return_(&[r]);
        }
        GlueKind::Cmp => {
            let r = cmp_glue(fx, ty, params[0], params[1])?;
            fx.b.ins().return_(&[r]);
        }
        GlueKind::Hash => {
            let r = hash_glue(fx, ty, params[0])?;
            fx.b.ins().return_(&[r]);
        }
        GlueKind::Debug => {
            debug_glue(fx, ty, params[0], params[1])?;
            fx.b.ins().return_(&[]);
        }
    }
    Ok(())
}

/// Emits a switch on the enum tag at `addr`: `each` runs in the block of every variant
/// (index and payload fields), then control continues after the switch.
fn per_variant(
    fx: &mut Fx<'_, '_, '_, '_>,
    tag: Value,
    variants: &[Vec<(u32, Ty)>],
    mut each: impl FnMut(&mut Fx<'_, '_, '_, '_>, u32, &[(u32, Ty)]) -> Result<()>,
) -> Result<()> {
    let done = fx.b.create_block();
    let mut switch = Switch::new();
    let blocks: Vec<Block> = variants.iter().map(|_| fx.b.create_block()).collect();
    for (i, &block) in blocks.iter().enumerate() {
        switch.set_entry(i as u128, block);
    }
    switch.emit(&mut fx.b, tag, done);
    for (i, &block) in blocks.iter().enumerate() {
        fx.b.switch_to_block(block);
        each(fx, u32::try_from(i).unwrap_or(0), &variants[i])?;
        fx.b.ins().jump(done, &[]);
    }
    fx.b.switch_to_block(done);
    Ok(())
}

fn drop_glue(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, p: Value) -> Result<()> {
    let ptr = fx.ptr();
    let word = fx.word();
    match shape(fx.cx(), ty) {
        Shape::String | Shape::Array | Shape::Map => fx.drop_at(p, ty)?,
        Shape::Record(fields, _) => {
            for (offset, t) in fields {
                let at = fx.offset(p, offset);
                fx.drop_at(at, t)?;
            }
        }
        Shape::Enum(_, variants) => {
            if variants.iter().flatten().any(|&(_, t)| fx.needs_drop(t)) {
                let tag = fx.load(types::I32, p, 0);
                per_variant(fx, tag, &variants, |fx, _, fields| {
                    for &(offset, t) in fields {
                        let at = fx.offset(p, offset);
                        fx.drop_at(at, t)?;
                    }
                    Ok(())
                })?;
            }
        }
        Shape::Callable => {
            let env = fx.load(ptr, p, word);
            let some = fx.b.create_block();
            let done = fx.b.create_block();
            fx.b.ins().brif(env, some, &[], done, &[]);
            fx.b.switch_to_block(some);
            let drop_fn = fx.load(ptr, env, 0);
            let sig = signature(GlueKind::Drop, ptr, fx.cx().call_conv);
            fx.call_indirect(sig, drop_fn, &[env]);
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(done);
        }
        Shape::Dyn => {
            let data = fx.load(ptr, p, 0);
            let vtable = fx.load(ptr, p, word);
            let drop_fn = fx.load(ptr, vtable, 0);
            let sig = signature(GlueKind::Drop, ptr, fx.cx().call_conv);
            fx.call_indirect(sig, drop_fn, &[data]);
            let size = fx.load(ptr, vtable, word);
            let align = fx.load(ptr, vtable, 2 * word);
            fx.rt("loom_free", &[data, size, align])?;
        }
        _ => {}
    }
    Ok(())
}

fn clone_glue(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, s: Value, d: Value) -> Result<()> {
    let ptr = fx.ptr();
    let word = fx.word();
    match shape(fx.cx(), ty) {
        Shape::String | Shape::Array | Shape::Map => {
            let name = match shape(fx.cx(), ty) {
                Shape::String => "loom_string_clone",
                Shape::Array => "loom_array_clone",
                _ => "loom_map_clone",
            };
            let handle = fx.load(ptr, s, 0);
            let copy = fx.rt1(name, &[handle])?;
            fx.store(copy, d, 0);
        }
        Shape::Record(fields, _) => {
            fx.copy_ty(d, s, ty);
            for (offset, t) in fields {
                if fx.needs_drop(t) {
                    let (fs, fd) = (fx.offset(s, offset), fx.offset(d, offset));
                    fx.clone_into(fs, fd, t)?;
                }
            }
        }
        Shape::Enum(_, variants) => {
            fx.copy_ty(d, s, ty);
            if variants.iter().flatten().any(|&(_, t)| fx.needs_drop(t)) {
                let tag = fx.load(types::I32, s, 0);
                per_variant(fx, tag, &variants, |fx, _, fields| {
                    for &(offset, t) in fields {
                        if fx.needs_drop(t) {
                            let (fs, fd) = (fx.offset(s, offset), fx.offset(d, offset));
                            fx.clone_into(fs, fd, t)?;
                        }
                    }
                    Ok(())
                })?;
            }
        }
        Shape::Callable => {
            fx.copy_ty(d, s, ty);
            let env = fx.load(ptr, s, word);
            let some = fx.b.create_block();
            let done = fx.b.create_block();
            fx.b.ins().brif(env, some, &[], done, &[]);
            fx.b.switch_to_block(some);
            let clone_fn = fx.load(ptr, env, word);
            let mut sig = Signature::new(fx.cx().call_conv);
            sig.params.push(AbiParam::new(ptr));
            sig.returns.push(AbiParam::new(ptr));
            let copy = fx.call_indirect(sig, clone_fn, &[env])[0];
            fx.store(copy, d, word);
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(done);
        }
        Shape::Dyn => {
            let data = fx.load(ptr, s, 0);
            let vtable = fx.load(ptr, s, word);
            let size = fx.load(ptr, vtable, word);
            let align = fx.load(ptr, vtable, 2 * word);
            let copy = fx.rt1("loom_alloc", &[size, align])?;
            let clone_fn = fx.load(ptr, vtable, 3 * word);
            let sig = signature(GlueKind::Clone, ptr, fx.cx().call_conv);
            fx.call_indirect(sig, clone_fn, &[data, copy]);
            fx.store(copy, d, 0);
            fx.store(vtable, d, word);
        }
        _ => fx.copy_ty(d, s, ty),
    }
    Ok(())
}

/// Calls the user `impl` method `(def, subst)` taking two borrowed values; returns the
/// address of its result.
fn call_user_binary(
    fx: &mut Fx<'_, '_, '_, '_>,
    def: DefId,
    subst: &Subst,
    a: Value,
    b: Value,
    ret_ty: Ty,
) -> Result<Value> {
    let Some(body) = fx.cx().fn_body(def) else {
        return unsupported("a trait method without a body");
    };
    let ret = fx.slot_for(ret_ty);
    let id =
        fx.g.func(Instance::Body(MirSource::Item(body), subst_key(subst)))?;
    fx.call(id, &[ret, a, b]);
    Ok(ret)
}

/// Equality of the values of type `ty` at `a` and `b` (an `I8`).
pub(crate) fn field_eq(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value, b: Value) -> Result<Value> {
    if let Some(t) = simple_scalar(fx.cx(), ty) {
        let x = fx.load(t, a, 0);
        let y = fx.load(t, b, 0);
        return Ok(if t.is_float() {
            fx.b.ins().fcmp(FloatCC::Equal, x, y)
        } else {
            fx.b.ins().icmp(IntCC::Equal, x, y)
        });
    }
    let id = fx.glue(GlueKind::Eq, ty)?;
    Ok(fx.call(id, &[a, b])[0])
}

/// The scalar type of a primitive that glue handles inline.
fn simple_scalar(cx: &Cx<'_>, ty: Ty) -> Option<Type> {
    match cx.interner().kind(ty) {
        TyKind::Int(i) if i.bits() <= 64 => cx.layout(ty).scalar_ty(),
        TyKind::Float(_) | TyKind::Bool | TyKind::Char => cx.layout(ty).scalar_ty(),
        _ => None,
    }
}

fn eq_glue(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value, b: Value) -> Result<Value> {
    let ptr = fx.ptr();
    if let Some((def, subst)) = fx.cx().user_impl("Eq", "eq", ty) {
        let bool_ty = fx.cx().interner().bool();
        let ret = call_user_binary(fx, def, &subst, a, b, bool_ty)?;
        return Ok(fx.load(types::I8, ret, 0));
    }
    Ok(match shape(fx.cx(), ty) {
        Shape::Int(i) if i.bits() == 128 => {
            let x = fx.load(types::I128, a, 0);
            let y = fx.load(types::I128, b, 0);
            fx.b.ins().icmp(IntCC::Equal, x, y)
        }
        Shape::Int(_) | Shape::Float(_) | Shape::Bool | Shape::Char => field_eq(fx, ty, a, b)?,
        Shape::Unit => fx.b.ins().iconst(types::I8, 1),
        Shape::String | Shape::Array | Shape::Map => {
            let name = match shape(fx.cx(), ty) {
                Shape::String => "loom_string_eq",
                Shape::Array => "loom_array_eq",
                _ => "loom_map_eq",
            };
            let x = fx.load(ptr, a, 0);
            let y = fx.load(ptr, b, 0);
            fx.rt1(name, &[x, y])?
        }
        Shape::Ref(inner) => {
            let x = fx.load(ptr, a, 0);
            let y = fx.load(ptr, b, 0);
            field_eq(fx, inner, x, y)?
        }
        Shape::Record(fields, _) => {
            let result = fx.slot(1, 1);
            let one = fx.b.ins().iconst(types::I8, 1);
            fx.store(one, result, 0);
            let done = fx.b.create_block();
            for (offset, t) in fields {
                let (x, y) = (fx.offset(a, offset), fx.offset(b, offset));
                let r = field_eq(fx, t, x, y)?;
                let next = fx.b.create_block();
                let unequal = fx.b.create_block();
                fx.b.ins().brif(r, next, &[], unequal, &[]);
                fx.b.switch_to_block(unequal);
                let zero = fx.b.ins().iconst(types::I8, 0);
                fx.store(zero, result, 0);
                fx.b.ins().jump(done, &[]);
                fx.b.switch_to_block(next);
            }
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(done);
            fx.load(types::I8, result, 0)
        }
        Shape::Enum(_, variants) => {
            let result = fx.slot(1, 1);
            let zero = fx.b.ins().iconst(types::I8, 0);
            fx.store(zero, result, 0);
            let ta = fx.load(types::I32, a, 0);
            let tb = fx.load(types::I32, b, 0);
            let same = fx.b.ins().icmp(IntCC::Equal, ta, tb);
            let compare = fx.b.create_block();
            let done = fx.b.create_block();
            fx.b.ins().brif(same, compare, &[], done, &[]);
            fx.b.switch_to_block(compare);
            per_variant(fx, ta, &variants, |fx, _, fields| {
                let one = fx.b.ins().iconst(types::I8, 1);
                fx.store(one, result, 0);
                for &(offset, t) in fields {
                    let (x, y) = (fx.offset(a, offset), fx.offset(b, offset));
                    let r = field_eq(fx, t, x, y)?;
                    let acc = fx.load(types::I8, result, 0);
                    let both = fx.b.ins().band(acc, r);
                    fx.store(both, result, 0);
                }
                Ok(())
            })?;
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(done);
            fx.load(types::I8, result, 0)
        }
        Shape::Callable | Shape::Dyn => fx.b.ins().iconst(types::I8, 0),
    })
}

/// Compares two scalars: -1, 0 or 1 as an `I8`.
fn scalar_cmp(fx: &mut Fx<'_, '_, '_, '_>, x: Value, y: Value, float: bool, signed: bool) -> Value {
    let (lt, gt) = if float {
        (
            fx.b.ins().fcmp(FloatCC::LessThan, x, y),
            fx.b.ins().fcmp(FloatCC::GreaterThan, x, y),
        )
    } else if signed {
        (
            fx.b.ins().icmp(IntCC::SignedLessThan, x, y),
            fx.b.ins().icmp(IntCC::SignedGreaterThan, x, y),
        )
    } else {
        (
            fx.b.ins().icmp(IntCC::UnsignedLessThan, x, y),
            fx.b.ins().icmp(IntCC::UnsignedGreaterThan, x, y),
        )
    };
    let minus = fx.b.ins().iconst(types::I8, -1);
    let one = fx.b.ins().iconst(types::I8, 1);
    let zero = fx.b.ins().iconst(types::I8, 0);
    let upper = fx.b.ins().select(gt, one, zero);
    fx.b.ins().select(lt, minus, upper)
}

fn field_cmp(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value, b: Value) -> Result<Value> {
    if let Some(t) = simple_scalar(fx.cx(), ty) {
        let signed = matches!(fx.cx().interner().kind(ty), TyKind::Int(i) if i.is_signed());
        let x = fx.load(t, a, 0);
        let y = fx.load(t, b, 0);
        return Ok(scalar_cmp(fx, x, y, t.is_float(), signed));
    }
    let id = fx.glue(GlueKind::Cmp, ty)?;
    Ok(fx.call(id, &[a, b])[0])
}

/// Lexicographic comparison of fields: stores the first non-zero result in `result` and
/// jumps to `done`; falls through when all fields are equal.
fn cmp_fields(
    fx: &mut Fx<'_, '_, '_, '_>,
    fields: &[(u32, Ty)],
    a: Value,
    b: Value,
    result: Value,
    done: Block,
) -> Result<()> {
    for &(offset, t) in fields {
        let (x, y) = (fx.offset(a, offset), fx.offset(b, offset));
        let c = field_cmp(fx, t, x, y)?;
        let differ = fx.b.create_block();
        let next = fx.b.create_block();
        fx.b.ins().brif(c, differ, &[], next, &[]);
        fx.b.switch_to_block(differ);
        fx.store(c, result, 0);
        fx.b.ins().jump(done, &[]);
        fx.b.switch_to_block(next);
    }
    Ok(())
}

fn cmp_glue(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value, b: Value) -> Result<Value> {
    let ptr = fx.ptr();
    if let Some((def, subst)) = fx.cx().user_impl("Ord", "compare", ty) {
        let ordering = match fx.cx().lang("Ordering") {
            Some(o) => fx.cx().interner().intern(TyKind::Adt(o, Vec::new())),
            None => return unsupported("missing Ordering"),
        };
        let ret = call_user_binary(fx, def, &subst, a, b, ordering)?;
        let tag = fx.load(types::I32, ret, 0);
        return fx.ordering_value(tag);
    }
    Ok(match shape(fx.cx(), ty) {
        Shape::Int(i) if i.bits() == 128 => {
            let x = fx.load(types::I128, a, 0);
            let y = fx.load(types::I128, b, 0);
            scalar_cmp(fx, x, y, false, i.is_signed())
        }
        Shape::Int(_) | Shape::Float(_) | Shape::Bool | Shape::Char => field_cmp(fx, ty, a, b)?,
        Shape::String | Shape::Array => {
            let name = if matches!(shape(fx.cx(), ty), Shape::String) {
                "loom_string_cmp"
            } else {
                "loom_array_cmp"
            };
            let x = fx.load(ptr, a, 0);
            let y = fx.load(ptr, b, 0);
            fx.rt1(name, &[x, y])?
        }
        Shape::Ref(inner) => {
            let x = fx.load(ptr, a, 0);
            let y = fx.load(ptr, b, 0);
            field_cmp(fx, inner, x, y)?
        }
        Shape::Record(fields, _) => {
            let result = fx.slot(1, 1);
            let zero = fx.b.ins().iconst(types::I8, 0);
            fx.store(zero, result, 0);
            let done = fx.b.create_block();
            cmp_fields(fx, &fields, a, b, result, done)?;
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(done);
            fx.load(types::I8, result, 0)
        }
        Shape::Enum(_, variants) => {
            let result = fx.slot(1, 1);
            let ta = fx.load(types::I32, a, 0);
            let tb = fx.load(types::I32, b, 0);
            let c = scalar_cmp(fx, ta, tb, false, false);
            fx.store(c, result, 0);
            let done = fx.b.create_block();
            let same = fx.b.create_block();
            fx.b.ins().brif(c, done, &[], same, &[]);
            fx.b.switch_to_block(same);
            per_variant(fx, ta, &variants, |fx, _, fields| {
                cmp_fields(fx, fields, a, b, result, done)
            })?;
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(done);
            fx.load(types::I8, result, 0)
        }
        Shape::Unit | Shape::Map | Shape::Callable | Shape::Dyn => fx.b.ins().iconst(types::I8, 0),
    })
}

fn mix(fx: &mut Fx<'_, '_, '_, '_>, h: Value, v: Value) -> Value {
    let x = fx.b.ins().bxor(h, v);
    fx.b.ins().imul_imm_s(x, 0x0100_0000_01b3)
}

fn field_hash(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value) -> Result<Value> {
    let id = fx.glue(GlueKind::Hash, ty)?;
    Ok(fx.call(id, &[a])[0])
}

fn hash_glue(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value) -> Result<Value> {
    let ptr = fx.ptr();
    Ok(match shape(fx.cx(), ty) {
        Shape::Int(i) => {
            let t = crate::layout::int_type(i);
            let v = fx.load(t, a, 0);
            if i.bits() == 128 {
                let (lo, hi) = fx.b.ins().isplit(v);
                fx.b.ins().bxor(lo, hi)
            } else {
                fx.int_convert(v, t, types::I64, i.is_signed())
            }
        }
        Shape::Float(f) => {
            if f == FloatTy::F32 {
                let v = fx.load(types::I32, a, 0);
                fx.b.ins().uextend(types::I64, v)
            } else {
                fx.load(types::I64, a, 0)
            }
        }
        Shape::Bool => {
            let v = fx.load(types::I8, a, 0);
            fx.b.ins().uextend(types::I64, v)
        }
        Shape::Char => {
            let v = fx.load(types::I32, a, 0);
            fx.b.ins().uextend(types::I64, v)
        }
        Shape::String | Shape::Array => {
            let name = if matches!(shape(fx.cx(), ty), Shape::String) {
                "loom_string_hash"
            } else {
                "loom_array_hash"
            };
            let x = fx.load(ptr, a, 0);
            fx.rt1(name, &[x])?
        }
        Shape::Ref(inner) => {
            let x = fx.load(ptr, a, 0);
            field_hash(fx, inner, x)?
        }
        Shape::Record(fields, _) => {
            #[allow(clippy::cast_possible_wrap, reason = "a hash seed")]
            let mut h =
                fx.b.ins()
                    .iconst(types::I64, 0xcbf2_9ce4_8422_2325_u64 as i64);
            for (offset, t) in fields {
                let at = fx.offset(a, offset);
                let fh = field_hash(fx, t, at)?;
                h = mix(fx, h, fh);
            }
            h
        }
        Shape::Enum(_, variants) => {
            let result = fx.slot(8, 8);
            let tag = fx.load(types::I32, a, 0);
            let wide = fx.b.ins().uextend(types::I64, tag);
            fx.store(wide, result, 0);
            per_variant(fx, tag, &variants, |fx, _, fields| {
                for &(offset, t) in fields {
                    let at = fx.offset(a, offset);
                    let fh = field_hash(fx, t, at)?;
                    let h = fx.load(types::I64, result, 0);
                    let h = mix(fx, h, fh);
                    fx.store(h, result, 0);
                }
                Ok(())
            })?;
            fx.load(types::I64, result, 0)
        }
        Shape::Unit | Shape::Map | Shape::Callable | Shape::Dyn => fx.b.ins().iconst(types::I64, 0),
    })
}

impl Fx<'_, '_, '_, '_> {
    /// A new string with the decimal text of integer `v` of type `int` (spec §7.12).
    pub fn int_text(&mut self, v: Value, int: IntTy) -> Result<Value> {
        let t = crate::layout::int_type(int);
        if int.bits() == 128 {
            let (lo, hi) = self.b.ins().isplit(v);
            let signed = self.b.ins().iconst(types::I8, i64::from(int.is_signed()));
            return self.rt1("loom_int128_to_string", &[lo, hi, signed]);
        }
        let wide = self.int_convert(v, t, types::I64, int.is_signed());
        if int.is_signed() {
            self.rt1("loom_int_to_string", &[wide])
        } else {
            self.rt1("loom_uint_to_string", &[wide])
        }
    }

    /// A new string with the text of float `v` (spec §7.12).
    pub fn float_text(&mut self, v: Value, float: FloatTy) -> Result<Value> {
        let (wide, is_f32) = if float == FloatTy::F32 {
            (self.b.ins().fpromote(types::F64, v), 1)
        } else {
            (v, 0)
        };
        let flag = self.b.ins().iconst(types::I8, is_f32);
        self.rt1("loom_float_to_string", &[wide, flag])
    }

    /// Appends the string `s` to `out` and frees `s`.
    pub fn append_and_free(&mut self, out: Value, s: Value) -> Result<()> {
        self.rt("loom_string_push_str", &[out, s])?;
        self.rt("loom_string_drop", &[s])?;
        Ok(())
    }
}

fn field_debug(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value, out: Value) -> Result<()> {
    let id = fx.glue(GlueKind::Debug, ty)?;
    fx.call(id, &[a, out]);
    Ok(())
}

fn debug_glue(fx: &mut Fx<'_, '_, '_, '_>, ty: Ty, a: Value, out: Value) -> Result<()> {
    let ptr = fx.ptr();
    match shape(fx.cx(), ty) {
        Shape::Int(i) => {
            let v = fx.load(crate::layout::int_type(i), a, 0);
            let s = fx.int_text(v, i)?;
            fx.append_and_free(out, s)?;
        }
        Shape::Float(f) => {
            let t = if f == FloatTy::F32 {
                types::F32
            } else {
                types::F64
            };
            let v = fx.load(t, a, 0);
            let s = fx.float_text(v, f)?;
            fx.append_and_free(out, s)?;
        }
        Shape::Bool => {
            let v = fx.load(types::I8, a, 0);
            let yes = fx.b.create_block();
            let no = fx.b.create_block();
            let done = fx.b.create_block();
            fx.b.ins().brif(v, yes, &[], no, &[]);
            fx.b.switch_to_block(yes);
            fx.push_text(out, "true")?;
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(no);
            fx.push_text(out, "false")?;
            fx.b.ins().jump(done, &[]);
            fx.b.switch_to_block(done);
        }
        Shape::Char => {
            let v = fx.load(types::I32, a, 0);
            fx.rt("loom_char_debug", &[v, out])?;
        }
        Shape::Unit => fx.push_text(out, "()")?,
        Shape::String => {
            let s = fx.load(ptr, a, 0);
            fx.rt("loom_string_debug", &[s, out])?;
        }
        Shape::Array => {
            let s = fx.load(ptr, a, 0);
            fx.rt("loom_array_debug", &[s, out])?;
        }
        Shape::Map => {
            let s = fx.load(ptr, a, 0);
            fx.rt("loom_map_debug", &[s, out])?;
        }
        Shape::Ref(inner) => {
            let p = fx.load(ptr, a, 0);
            field_debug(fx, inner, p, out)?;
        }
        Shape::Record(fields, def) => {
            let names: Vec<String> = match def {
                Some(d) => {
                    let name = fx.cx().hir.def(d).name.to_string();
                    fx.push_text(out, &format!("{name}("))?;
                    fx.cx()
                        .items()
                        .fields
                        .get(&d)
                        .map(|fs| fs.iter().map(|f| format!("{}: ", f.name)).collect())
                        .unwrap_or_default()
                }
                None => {
                    fx.push_text(out, "(")?;
                    Vec::new()
                }
            };
            for (i, (offset, t)) in fields.into_iter().enumerate() {
                let mut prefix = if i > 0 {
                    ", ".to_owned()
                } else {
                    String::new()
                };
                if let Some(n) = names.get(i) {
                    prefix.push_str(n);
                }
                if !prefix.is_empty() {
                    fx.push_text(out, &prefix)?;
                }
                let at = fx.offset(a, offset);
                field_debug(fx, t, at, out)?;
            }
            fx.push_text(out, ")")?;
        }
        Shape::Enum(def, variants) => {
            let names: Vec<String> = match &fx.cx().hir.def(def).kind {
                DefKind::Enum(data) => data
                    .variants
                    .iter()
                    .map(|&v| fx.cx().hir.def(v).name.to_string())
                    .collect(),
                _ => Vec::new(),
            };
            let tag = fx.load(types::I32, a, 0);
            per_variant(fx, tag, &variants, |fx, i, fields| {
                let name = names.get(i as usize).cloned().unwrap_or_default();
                if fields.is_empty() {
                    return fx.push_text(out, &name);
                }
                fx.push_text(out, &format!("{name}("))?;
                for (k, &(offset, t)) in fields.iter().enumerate() {
                    if k > 0 {
                        fx.push_text(out, ", ")?;
                    }
                    let at = fx.offset(a, offset);
                    field_debug(fx, t, at, out)?;
                }
                fx.push_text(out, ")")
            })?;
        }
        Shape::Callable => fx.push_text(out, "<function>")?,
        Shape::Dyn => fx.push_text(out, "<dyn>")?,
    }
    let _ = panic_trap;
    Ok(())
}
