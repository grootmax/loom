//! Translation of MIR bodies (and shims, closure environments and the entry point) to
//! Cranelift IR (ADR 0021 §3, §5, §6).
//!
//! Every local lives in memory: a stack slot, or for aggregate arguments the caller's copy.
//! Locals whose type needs dropping carry a drop flag, set on initialization and cleared on
//! a move; MIR `Drop` drops a local only if its flag is set.

use crate::layout::Repr;
use crate::{
    Cx, Gen, GlueKind, Instance, Resolved, Result, internal, subst_key, subst_of, unsupported,
};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    self, AbiParam, Block, FuncRef, InstBuilder, MemFlagsData, Signature, StackSlotData,
    StackSlotKind, TrapCode, Type, Value, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch};
use cranelift_module::{DataId, FuncId, Linkage, Module};
use loom_hir::{BinaryOp, DefKind, UnaryOp};
use loom_mir::{
    AggregateKind, Callee, Constant, Local, LocalKind, MirBody, MirSource, Operand, Place, Proj,
    Rvalue, StatementKind, TerminatorKind,
};
use loom_span::Span;
use loom_types::{FloatTy, IntTy, Subst, Ty, TyKind};

/// The trap code of code after a panic (never reached: the runtime exits).
pub(crate) fn panic_trap() -> TrapCode {
    TrapCode::user(1).unwrap_or(TrapCode::STACK_OVERFLOW)
}

/// Code emission shared by every kind of generated function.
pub(crate) struct Fx<'g, 'a, 'm, 'f> {
    pub g: &'g mut Gen<'a, 'm>,
    pub b: FunctionBuilder<'f>,
}

impl<'a> Fx<'_, 'a, '_, '_> {
    pub fn cx(&self) -> &Cx<'a> {
        &self.g.cx
    }

    pub fn ptr(&self) -> Type {
        self.g.cx.ptr_ty
    }

    pub fn word(&self) -> u32 {
        self.g.cx.ptr_ty.bytes()
    }

    pub fn pconst(&mut self, value: i64) -> Value {
        let ptr = self.ptr();
        self.b.ins().iconst(ptr, value)
    }

    pub fn slot(&mut self, size: u32, align: u32) -> Value {
        let shift = u8::try_from(align.max(1).trailing_zeros()).unwrap_or(3);
        let ss = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size.max(1),
            shift.max(3),
        ));
        let ptr = self.ptr();
        self.b.ins().stack_addr(ptr, ss, 0)
    }

    pub fn slot_for(&mut self, ty: Ty) -> Value {
        let l = self.cx().layout(ty);
        self.slot(l.size, l.align)
    }

    pub fn load(&mut self, t: Type, addr: Value, offset: u32) -> Value {
        self.b.ins().load(
            t,
            MemFlagsData::new(),
            addr,
            i32::try_from(offset).unwrap_or(0),
        )
    }

    pub fn store(&mut self, value: Value, addr: Value, offset: u32) {
        self.b.ins().store(
            MemFlagsData::new(),
            value,
            addr,
            i32::try_from(offset).unwrap_or(0),
        );
    }

    pub fn offset(&mut self, addr: Value, offset: u32) -> Value {
        if offset == 0 {
            addr
        } else {
            self.b.ins().iadd_imm_s(addr, i64::from(offset))
        }
    }

    pub fn copy(&mut self, dst: Value, src: Value, size: u32, align: u32) {
        if size == 0 {
            return;
        }
        let config = self.g.module.target_config();
        let align = u8::try_from(align.clamp(1, 8)).unwrap_or(1);
        self.b.emit_small_memory_copy(
            config,
            dst,
            src,
            u64::from(size),
            align,
            align,
            true,
            MemFlagsData::new(),
        );
    }

    pub fn copy_ty(&mut self, dst: Value, src: Value, ty: Ty) {
        let l = self.cx().layout(ty);
        self.copy(dst, src, l.size, l.align);
    }

    pub fn func_ref(&mut self, id: FuncId) -> FuncRef {
        self.g.module.declare_func_in_func(id, self.b.func)
    }

    pub fn func_addr(&mut self, id: FuncId) -> Value {
        let r = self.func_ref(id);
        let ptr = self.ptr();
        self.b.ins().func_addr(ptr, r)
    }

    pub fn call(&mut self, id: FuncId, args: &[Value]) -> Vec<Value> {
        let r = self.func_ref(id);
        let inst = self.b.ins().call(r, args);
        self.b.inst_results(inst).to_vec()
    }

    pub fn call_indirect(&mut self, sig: Signature, code: Value, args: &[Value]) -> Vec<Value> {
        let sig = self.b.import_signature(sig);
        let inst = self.b.ins().call_indirect(sig, code, args);
        self.b.inst_results(inst).to_vec()
    }

    /// Calls runtime function `name`; returns its result, if any.
    pub fn rt(&mut self, name: &'static str, args: &[Value]) -> Result<Option<Value>> {
        let id = self.g.runtime_fn(name)?;
        Ok(self.call(id, args).first().copied())
    }

    /// Calls runtime function `name`, which returns a value.
    pub fn rt1(&mut self, name: &'static str, args: &[Value]) -> Result<Value> {
        match self.rt(name, args)? {
            Some(v) => Ok(v),
            None => Err(internal(format!("runtime function {name} returns nothing"))),
        }
    }

    pub fn data_addr(&mut self, id: DataId) -> Value {
        let gv = self.g.module.declare_data_in_func(id, self.b.func);
        let ptr = self.ptr();
        self.b.ins().symbol_value(ptr, gv)
    }

    /// The address and length of static bytes.
    pub fn static_bytes(&mut self, bytes: &[u8]) -> Result<(Value, Value)> {
        let id = self.g.bytes(bytes)?;
        let addr = self.data_addr(id);
        let len = self.pconst(i64::try_from(bytes.len()).unwrap_or(0));
        Ok((addr, len))
    }

    /// A new string object with static text.
    pub fn static_string(&mut self, text: &str) -> Result<Value> {
        let (addr, len) = self.static_bytes(text.as_bytes())?;
        self.rt1("loom_string_from_static", &[addr, len])
    }

    /// Appends static text to the string `out`.
    pub fn push_text(&mut self, out: Value, text: &str) -> Result<()> {
        let (addr, len) = self.static_bytes(text.as_bytes())?;
        self.rt("loom_debug_text", &[addr, len, out])?;
        Ok(())
    }

    /// File, file length, line and column of `span`.
    pub fn position(&mut self, span: Span) -> Result<[Value; 4]> {
        let (file, line, column) = self.cx().position(span);
        let (addr, len) = self.static_bytes(file.as_bytes())?;
        let line = self.b.ins().iconst(types::I32, i64::from(line));
        let column = self.b.ins().iconst(types::I32, i64::from(column));
        Ok([addr, len, line, column])
    }

    /// Continues in a new block (after a terminator).
    pub fn fresh_block(&mut self) {
        let block = self.b.create_block();
        self.b.switch_to_block(block);
    }

    /// Panics with `message` at `span`.
    pub fn panic(&mut self, message: &str, span: Span) -> Result<()> {
        let (addr, len) = self.static_bytes(message.as_bytes())?;
        let [file, file_len, line, column] = self.position(span)?;
        self.rt("loom_panic", &[addr, len, file, file_len, line, column])?;
        self.b.ins().trap(panic_trap());
        self.fresh_block();
        Ok(())
    }

    /// Panics with `message` if `condition` is non-zero.
    pub fn panic_if(&mut self, condition: Value, message: &str, span: Span) -> Result<()> {
        let fail = self.b.create_block();
        let cont = self.b.create_block();
        self.b.ins().brif(condition, fail, &[], cont, &[]);
        self.b.switch_to_block(fail);
        self.panic(message, span)?;
        self.b.ins().trap(panic_trap());
        self.b.switch_to_block(cont);
        Ok(())
    }

    /// Whether values of `ty` own resources.
    pub fn needs_drop(&self, ty: Ty) -> bool {
        self.cx().needs_drop(ty)
    }

    pub fn glue(&mut self, kind: GlueKind, ty: Ty) -> Result<FuncId> {
        self.g.func(Instance::Glue(kind, ty))
    }

    /// Drops the value of type `ty` at `addr`.
    pub fn drop_at(&mut self, addr: Value, ty: Ty) -> Result<()> {
        if !self.needs_drop(ty) {
            return Ok(());
        }
        let ptr = self.ptr();
        let handle_drop = match self.cx().interner().kind(ty) {
            TyKind::String | TyKind::Bytes => Some("loom_string_drop"),
            TyKind::Adt(def, _) => match self.cx().hir.def(def).lang() {
                Some("Array") => Some("loom_array_drop"),
                Some("Map") => Some("loom_map_drop"),
                _ => None,
            },
            _ => None,
        };
        if let Some(name) = handle_drop {
            let handle = self.load(ptr, addr, 0);
            self.rt(name, &[handle])?;
        } else {
            let id = self.glue(GlueKind::Drop, ty)?;
            self.call(id, &[addr]);
        }
        Ok(())
    }

    /// Writes a deep copy of the value of type `ty` at `src` to `dst`.
    pub fn clone_into(&mut self, src: Value, dst: Value, ty: Ty) -> Result<()> {
        if !self.needs_drop(ty) {
            self.copy_ty(dst, src, ty);
            return Ok(());
        }
        let id = self.glue(GlueKind::Clone, ty)?;
        self.call(id, &[src, dst]);
        Ok(())
    }

    /// The address of the type descriptor of `ty`.
    pub fn type_info(&mut self, ty: Ty) -> Result<Value> {
        let id = crate::glue::type_info(self.g, ty)?;
        Ok(self.data_addr(id))
    }

    /// The tag of the variant named `name` of lang enum `enum_lang`.
    pub fn lang_tag(&self, enum_lang: &str, name: &str) -> Result<u32> {
        match self.cx().lang_variant(enum_lang, name) {
            Some(v) => Ok(self.cx().variant_index(v)),
            None => unsupported(format!("missing {enum_lang}.{name}")),
        }
    }

    /// The offset of the first payload field of variant `tag` of enum type `ty`.
    pub fn payload_offset(&self, ty: Ty, tag: u32) -> u32 {
        let l = self.cx().layout(ty);
        l.variants
            .get(tag as usize)
            .and_then(|v| v.first())
            .map_or(4, |&(o, _)| o)
    }

    /// Stores `Some`/`None` of `Option` type `ty` at `addr` according to `present` (the
    /// payload, if present, is written by the caller).
    pub fn set_option(&mut self, addr: Value, present: Value) -> Result<()> {
        let some = self.lang_tag("Option", "Some")?;
        let none = self.lang_tag("Option", "None")?;
        let s = self.b.ins().iconst(types::I32, i64::from(some));
        let n = self.b.ins().iconst(types::I32, i64::from(none));
        let tag = self.b.ins().select(present, s, n);
        self.store(tag, addr, 0);
        Ok(())
    }

    /// An `Ordering` tag from a comparison result (-1, 0, 1 as `I8`).
    pub fn ordering_tag(&mut self, c: Value) -> Result<Value> {
        let less = self.lang_tag("Ordering", "Less")?;
        let equal = self.lang_tag("Ordering", "Equal")?;
        let greater = self.lang_tag("Ordering", "Greater")?;
        let l = self.b.ins().iconst(types::I32, i64::from(less));
        let e = self.b.ins().iconst(types::I32, i64::from(equal));
        let gr = self.b.ins().iconst(types::I32, i64::from(greater));
        let is_less = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, c, 0);
        let is_greater = self.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, c, 0);
        let upper = self.b.ins().select(is_greater, gr, e);
        Ok(self.b.ins().select(is_less, l, upper))
    }

    /// A comparison result (-1, 0, 1 as `I8`) from an `Ordering` tag.
    pub fn ordering_value(&mut self, tag: Value) -> Result<Value> {
        let less = self.lang_tag("Ordering", "Less")?;
        let greater = self.lang_tag("Ordering", "Greater")?;
        let is_less = self.b.ins().icmp_imm_s(IntCC::Equal, tag, i64::from(less));
        let is_greater = self
            .b
            .ins()
            .icmp_imm_s(IntCC::Equal, tag, i64::from(greater));
        let minus = self.b.ins().iconst(types::I8, -1);
        let one = self.b.ins().iconst(types::I8, 1);
        let zero = self.b.ins().iconst(types::I8, 0);
        let upper = self.b.ins().select(is_greater, one, zero);
        Ok(self.b.ins().select(is_less, minus, upper))
    }

    /// An integer constant of type `t` (including 128-bit values).
    pub fn int_const(&mut self, t: Type, value: i128) -> Value {
        if t == types::I128 {
            let bits = value.cast_unsigned();
            let half = |v: u128| {
                u64::try_from(v & u128::from(u64::MAX))
                    .unwrap_or(0)
                    .cast_signed()
            };
            let lo = self.b.ins().iconst(types::I64, half(bits));
            let hi = self.b.ins().iconst(types::I64, half(bits >> 64));
            self.b.ins().iconcat(lo, hi)
        } else {
            #[allow(clippy::cast_possible_truncation, reason = "the value fits the type")]
            let v = value as i64;
            self.b.ins().iconst(t, v)
        }
    }

    /// Converts an integer value between Cranelift types.
    pub fn int_convert(&mut self, v: Value, from: Type, to: Type, signed: bool) -> Value {
        if from == to {
            v
        } else if from.bits() < to.bits() {
            if signed {
                self.b.ins().sextend(to, v)
            } else {
                self.b.ins().uextend(to, v)
            }
        } else {
            self.b.ins().ireduce(to, v)
        }
    }
}

impl Cx<'_> {
    /// Whether values of `ty` own resources (strings, collections, closures, `dyn`).
    pub fn needs_drop(&self, ty: Ty) -> bool {
        let interner = self.interner();
        match interner.kind(ty) {
            TyKind::String | TyKind::Bytes | TyKind::Callable { .. } | TyKind::Dyn(..) => true,
            TyKind::Tuple(elements) => elements.iter().any(|&e| self.needs_drop(e)),
            TyKind::Adt(def, _) => {
                if self.is_handle(ty) {
                    return true;
                }
                let l = self.layout(ty);
                l.fields.iter().any(|&(_, t)| self.needs_drop(t))
                    || l.variants
                        .iter()
                        .flatten()
                        .any(|&(_, t)| self.needs_drop(t))
                    || matches!(self.hir.def(def).kind, DefKind::Struct(_) if false)
            }
            _ => false,
        }
    }
}

/// One local's storage.
#[derive(Clone, Copy)]
struct LocalInfo {
    ty: Ty,
    addr: Value,
    flag: Option<Value>,
}

/// Translates one MIR body.
pub(crate) struct BodyTx<'x, 'g, 'a, 'm, 'f> {
    pub fx: &'x mut Fx<'g, 'a, 'm, 'f>,
    pub body: &'a MirBody,
    pub subst: Subst,
    locals: Vec<LocalInfo>,
    blocks: Vec<Block>,
    env_local: Option<Local>,
    captures: Vec<(u32, Ty)>,
    /// Constants materialized to be read in place, dropped after the statement.
    const_temps: Vec<(Value, Ty)>,
    /// Field paths of locals that are moved, dropped or assigned separately, each with a
    /// drop flag: whether the path holds a value, given that its local does (ADR 0021 §6).
    paths: Vec<(Local, Vec<Proj>, Value)>,
    pub span: Span,
}

/// Translates `instance` into `func`.
pub(crate) fn translate(
    g: &mut Gen<'_, '_>,
    instance: &Instance,
    func: &mut ir::Function,
) -> Result<()> {
    let mut fctx = FunctionBuilderContext::new();
    let b = FunctionBuilder::new(func, &mut fctx);
    let mut fx = Fx { g, b };
    let entry = fx.b.create_block();
    fx.b.append_block_params_for_function_params(entry);
    fx.b.switch_to_block(entry);
    let params = fx.b.block_params(entry).to_vec();
    match instance {
        Instance::Body(source, key) => {
            let body = fx.g.cx.body(*source)?;
            BodyTx::run(&mut fx, body, subst_of(key), &params)?;
        }
        Instance::FnShim(def, key) => fn_shim(&mut fx, *def, &subst_of(key), &params)?,
        Instance::EnvDrop(source, key) => env_drop(&mut fx, *source, &subst_of(key), params[0])?,
        Instance::EnvClone(source, key) => env_clone(&mut fx, *source, &subst_of(key), params[0])?,
        Instance::Glue(kind, ty) => crate::glue::translate(&mut fx, *kind, *ty, &params)?,
        Instance::Entry(main) => entry_point(&mut fx, *main)?,
    }
    let config = fx.g.module.target_config();
    let Fx { mut b, .. } = fx;
    b.seal_all_blocks();
    b.finalize(config);
    Ok(())
}

/// The layout of a closure environment: total size, alignment, and per capture its offset
/// (from the environment pointer) and type. Two words of header hold the drop and clone
/// functions of the environment.
pub(crate) fn env_layout(cx: &Cx<'_>, body: &MirBody, subst: &Subst) -> (u32, u32, Vec<(u32, Ty)>) {
    let interner = cx.interner();
    let types: Vec<Ty> = body
        .captures
        .iter()
        .map(|c| {
            let t = cx.concrete(c.ty, subst);
            match c.by_ref {
                Some(mutable) => interner.intern(TyKind::Ref { mutable, inner: t }),
                None => t,
            }
        })
        .collect();
    let record = cx.record(&types);
    let word = cx.ptr_ty.bytes();
    let base = crate::layout::align_to(2 * word, record.align);
    let align = record.align.max(word);
    let size = crate::layout::align_to(base + record.size, align);
    let fields = record.fields.iter().map(|&(o, t)| (base + o, t)).collect();
    (size, align, fields)
}

impl<'x, 'g, 'a, 'm, 'f> BodyTx<'x, 'g, 'a, 'm, 'f> {
    fn run(
        fx: &'x mut Fx<'g, 'a, 'm, 'f>,
        body: &'a MirBody,
        subst: Subst,
        params: &[Value],
    ) -> Result<()> {
        let blocks: Vec<Block> = body.blocks.iter().map(|_| fx.b.create_block()).collect();
        let is_closure = matches!(body.source, MirSource::Closure(..));
        let captures = if is_closure {
            env_layout(fx.cx(), body, &subst).2
        } else {
            Vec::new()
        };
        let mut tx = BodyTx {
            fx,
            body,
            subst,
            locals: Vec::new(),
            blocks,
            env_local: None,
            captures,
            const_temps: Vec::new(),
            paths: Vec::new(),
            span: body.span,
        };
        tx.allocate_locals(params);
        tx.allocate_paths()?;
        tx.fx.b.ins().jump(tx.blocks[0], &[]);
        for (i, block) in body.blocks.iter().enumerate() {
            tx.fx.b.switch_to_block(tx.blocks[i]);
            for statement in &block.statements {
                tx.span = statement.span;
                tx.statement(&statement.kind)?;
                tx.drop_const_temps()?;
            }
            match &block.terminator {
                Some(t) => {
                    tx.span = t.span;
                    tx.terminator(&t.kind)?;
                }
                None => {
                    tx.fx.b.ins().trap(panic_trap());
                }
            }
        }
        Ok(())
    }

    fn cx(&self) -> &Cx<'a> {
        &self.fx.g.cx
    }

    fn concrete(&self, ty: Ty) -> Ty {
        self.fx.g.cx.concrete(ty, &self.subst)
    }

    fn allocate_locals(&mut self, params: &[Value]) {
        let mut next_param = 1;
        let ptr = self.fx.ptr();
        for (i, decl) in self.body.locals.iter().enumerate() {
            let ty = self.concrete(decl.ty);
            let layout = self.cx().layout(ty);
            let is_arg = i >= 1 && i <= self.body.arg_count;
            let addr = if decl.kind == LocalKind::ClosureEnv {
                self.env_local = Some(Local(u32::try_from(i).unwrap_or(0)));
                let env = params[next_param];
                next_param += 1;
                let slot = self.fx.slot(ptr.bytes(), ptr.bytes());
                self.fx.store(env, slot, 0);
                slot
            } else if i == 0 {
                if layout.size == 0 {
                    self.fx.pconst(i64::from(layout.align))
                } else {
                    params[0]
                }
            } else if is_arg {
                match layout.repr {
                    Repr::Zero => self.fx.pconst(i64::from(layout.align)),
                    Repr::Scalar(_) => {
                        let slot = self.fx.slot(layout.size, layout.align);
                        self.fx.store(params[next_param], slot, 0);
                        next_param += 1;
                        slot
                    }
                    Repr::Memory => {
                        let p = params[next_param];
                        next_param += 1;
                        p
                    }
                }
            } else if layout.size == 0 {
                self.fx.pconst(i64::from(layout.align))
            } else {
                self.fx.slot(layout.size, layout.align)
            };
            let flag = if i != 0 && self.fx.needs_drop(ty) {
                let slot = self.fx.slot(1, 1);
                let init = self.fx.b.ins().iconst(types::I8, i64::from(is_arg));
                self.fx.store(init, slot, 0);
                Some(slot)
            } else {
                None
            };
            self.locals.push(LocalInfo { ty, addr, flag });
        }
    }

    /// Finds the field paths that need their own drop flags: every place below a local
    /// (not behind a reference) whose type owns resources and that MIR moves, drops or
    /// assigns.
    fn allocate_paths(&mut self) -> Result<()> {
        fn visit_operand(op: &Operand, out: &mut Vec<Place>) {
            if let Operand::Move(p) = op {
                out.push(p.clone());
            }
        }
        let mut places: Vec<Place> = Vec::new();
        for block in &self.body.blocks {
            for statement in &block.statements {
                if let StatementKind::Assign(place, rvalue) = &statement.kind {
                    places.push(place.clone());
                    match rvalue {
                        Rvalue::Use(op) | Rvalue::Unary(_, op) | Rvalue::Unsize(op, _) => {
                            visit_operand(op, &mut places);
                        }
                        Rvalue::Binary(_, a, b) => {
                            visit_operand(a, &mut places);
                            visit_operand(b, &mut places);
                        }
                        Rvalue::Aggregate(_, ops) => {
                            for op in ops {
                                visit_operand(op, &mut places);
                            }
                        }
                        Rvalue::Ref { .. } | Rvalue::Discriminant(_) => {}
                    }
                }
            }
            match block.terminator.as_ref().map(|t| &t.kind) {
                Some(TerminatorKind::Call { callee, args, .. }) => {
                    if let Callee::Value(op) = callee {
                        visit_operand(op, &mut places);
                    }
                    for op in args {
                        visit_operand(op, &mut places);
                    }
                }
                Some(TerminatorKind::Drop { place, .. }) => places.push(place.clone()),
                _ => {}
            }
        }
        for place in places {
            if place.proj.is_empty()
                || place.is_behind_ref()
                || place.proj.iter().any(|p| matches!(p, Proj::Index(_)))
                || self.locals[place.local.index()].flag.is_none()
                || self
                    .paths
                    .iter()
                    .any(|(l, p, _)| *l == place.local && *p == place.proj)
            {
                continue;
            }
            if !self.fx.needs_drop(self.place_type(&place)?) {
                continue;
            }
            let flag = self.fx.slot(1, 1);
            let is_arg = place.local.index() >= 1 && place.local.index() <= self.body.arg_count;
            let init = self.fx.b.ins().iconst(types::I8, i64::from(is_arg));
            self.fx.store(init, flag, 0);
            self.paths.push((place.local, place.proj.clone(), flag));
        }
        Ok(())
    }

    /// The drop flag of the field path `proj` of `local`, if it has one.
    fn path_flag(&self, local: Local, proj: &[Proj]) -> Option<Value> {
        self.paths
            .iter()
            .find(|(l, p, _)| *l == local && p.as_slice() == proj)
            .map(|&(_, _, f)| f)
    }

    /// Sets the flags of `proj` and of every tracked path below it.
    fn set_path_flags(&mut self, local: Local, proj: &[Proj], value: bool) {
        let flags: Vec<Value> = self
            .paths
            .iter()
            .filter(|(l, p, _)| *l == local && p.starts_with(proj))
            .map(|&(_, _, f)| f)
            .collect();
        for flag in flags {
            let v = self.fx.b.ins().iconst(types::I8, i64::from(value));
            self.fx.store(v, flag, 0);
        }
    }

    /// Drops the value of type `ty` at `addr`, the path `prefix` of `local`, except for the
    /// tracked paths below it that no longer hold a value.
    fn drop_elaborated(
        &mut self,
        local: Local,
        prefix: &[Proj],
        ty: Ty,
        addr: Value,
    ) -> Result<()> {
        if !self.fx.needs_drop(ty) {
            return Ok(());
        }
        let cont = self.fx.b.create_block();
        if let Some(flag) = self.path_flag(local, prefix) {
            let set = self.fx.load(types::I8, flag, 0);
            let present = self.fx.b.create_block();
            self.fx.b.ins().brif(set, present, &[], cont, &[]);
            self.fx.b.switch_to_block(present);
        }
        let below = self
            .paths
            .iter()
            .any(|(l, p, _)| *l == local && p.len() > prefix.len() && p.starts_with(prefix));
        if below {
            let layout = self.cx().layout(ty);
            if layout.variants.is_empty() {
                for (i, &(offset, field_ty)) in layout.fields.iter().enumerate() {
                    let mut path = prefix.to_vec();
                    path.push(Proj::Field(u32::try_from(i).unwrap_or(0)));
                    let at = self.fx.offset(addr, offset);
                    self.drop_elaborated(local, &path, field_ty, at)?;
                }
            } else {
                let variants = self.variant_defs(ty);
                let tag = self.fx.load(types::I32, addr, 0);
                let done = self.fx.b.create_block();
                let mut switch = Switch::new();
                let blocks: Vec<Block> = layout
                    .variants
                    .iter()
                    .map(|_| self.fx.b.create_block())
                    .collect();
                for (i, &block) in blocks.iter().enumerate() {
                    switch.set_entry(i as u128, block);
                }
                switch.emit(&mut self.fx.b, tag, done);
                for (i, &block) in blocks.iter().enumerate() {
                    self.fx.b.switch_to_block(block);
                    for (k, &(offset, field_ty)) in layout.variants[i].iter().enumerate() {
                        let mut path = prefix.to_vec();
                        if let Some(&v) = variants.get(i) {
                            path.push(Proj::Downcast(v));
                        }
                        path.push(Proj::Field(u32::try_from(k).unwrap_or(0)));
                        let at = self.fx.offset(addr, offset);
                        self.drop_elaborated(local, &path, field_ty, at)?;
                    }
                    self.fx.b.ins().jump(done, &[]);
                }
                self.fx.b.switch_to_block(done);
            }
        } else {
            self.fx.drop_at(addr, ty)?;
        }
        self.fx.b.ins().jump(cont, &[]);
        self.fx.b.switch_to_block(cont);
        Ok(())
    }

    /// The variants of enum type `ty`, in declaration order.
    fn variant_defs(&self, ty: Ty) -> Vec<loom_hir::DefId> {
        match self.cx().interner().kind(ty) {
            TyKind::Adt(def, _) => match &self.cx().hir.def(def).kind {
                DefKind::Enum(data) => data.variants.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn set_flag(&mut self, local: Local, value: bool) {
        if let Some(flag) = self.locals[local.index()].flag {
            let v = self.fx.b.ins().iconst(types::I8, i64::from(value));
            self.fx.store(v, flag, 0);
        }
    }

    // ----- places ------------------------------------------------------------------------

    /// The type of `place` (concrete), without emitting code.
    pub fn place_type(&self, place: &Place) -> Result<Ty> {
        let interner = self.cx().interner();
        let mut ty = self.locals[place.local.index()].ty;
        let mut variant: Option<u32> = None;
        let mut env_mode = false;
        for (i, step) in place.proj.iter().enumerate() {
            match step {
                Proj::Deref => {
                    if i == 0 && Some(place.local) == self.env_local {
                        env_mode = true;
                    }
                    match interner.kind(ty) {
                        TyKind::Ref { inner, .. } => ty = inner,
                        _ => return unsupported(format!("deref of `{}`", self.cx().ty_name(ty))),
                    }
                }
                Proj::Field(f) => {
                    if env_mode {
                        env_mode = false;
                        ty = match self.captures.get(*f as usize) {
                            Some(&(_, t)) => t,
                            None => return unsupported("closure capture out of range"),
                        };
                        continue;
                    }
                    let l = self.cx().layout(ty);
                    let fields = match variant.take() {
                        Some(v) => l.variants.get(v as usize).cloned().unwrap_or_default(),
                        None => l.fields.clone(),
                    };
                    ty = match fields.get(*f as usize) {
                        Some(&(_, t)) => t,
                        None => {
                            return unsupported(format!(
                                "field {f} of `{}`",
                                self.cx().ty_name(ty)
                            ));
                        }
                    };
                }
                Proj::Downcast(v) => variant = Some(self.cx().variant_index(*v)),
                Proj::Index(_) => {
                    ty = match interner.kind(ty) {
                        TyKind::Adt(_, args) if !args.is_empty() => args[0],
                        _ => return unsupported(format!("index into `{}`", self.cx().ty_name(ty))),
                    };
                }
            }
        }
        Ok(ty)
    }

    /// The address and type of `place`.
    pub fn place_addr(&mut self, place: &Place) -> Result<(Value, Ty)> {
        let interner = self.cx().interner();
        let ptr = self.fx.ptr();
        let info = self.locals[place.local.index()];
        let (mut addr, mut ty) = (info.addr, info.ty);
        let mut variant: Option<u32> = None;
        let mut env_mode = false;
        for (i, step) in place.proj.iter().enumerate() {
            match step {
                Proj::Deref => {
                    if i == 0 && Some(place.local) == self.env_local {
                        env_mode = true;
                    }
                    match interner.kind(ty) {
                        TyKind::Ref { inner, .. } => {
                            addr = self.fx.load(ptr, addr, 0);
                            ty = inner;
                        }
                        _ => return unsupported(format!("deref of `{}`", self.cx().ty_name(ty))),
                    }
                }
                Proj::Field(f) => {
                    let (offset, t) = if env_mode {
                        env_mode = false;
                        match self.captures.get(*f as usize) {
                            Some(&c) => c,
                            None => return unsupported("closure capture out of range"),
                        }
                    } else {
                        let l = self.cx().layout(ty);
                        let fields = match variant.take() {
                            Some(v) => l.variants.get(v as usize).cloned().unwrap_or_default(),
                            None => l.fields.clone(),
                        };
                        match fields.get(*f as usize) {
                            Some(&c) => c,
                            None => {
                                return unsupported(format!(
                                    "field {f} of `{}`",
                                    self.cx().ty_name(ty)
                                ));
                            }
                        }
                    };
                    addr = self.fx.offset(addr, offset);
                    ty = t;
                }
                Proj::Downcast(v) => variant = Some(self.cx().variant_index(*v)),
                Proj::Index(local) => {
                    let elem = match interner.kind(ty) {
                        TyKind::Adt(_, args) if !args.is_empty() => args[0],
                        _ => return unsupported(format!("index into `{}`", self.cx().ty_name(ty))),
                    };
                    let handle = self.fx.load(ptr, addr, 0);
                    let index_addr = self.locals[local.index()].addr;
                    let index = self.fx.load(types::I64, index_addr, 0);
                    let [file, len, line, column] = self.fx.position(self.span)?;
                    addr = self
                        .fx
                        .rt1("loom_array_get", &[handle, index, file, len, line, column])?;
                    ty = elem;
                }
            }
        }
        Ok((addr, ty))
    }

    /// The type of an operand (concrete).
    pub fn operand_ty(&self, op: &Operand) -> Result<Ty> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => self.place_type(p),
            Operand::Const(_, ty) => Ok(self.concrete(*ty)),
        }
    }

    /// Records that `place` was moved out of.
    fn mark_moved(&mut self, place: &Place) {
        if place.is_behind_ref() {
            return;
        }
        if place.proj.is_empty() {
            self.set_flag(place.local, false);
        } else if let Some(flag) = self.path_flag(place.local, &place.proj) {
            let zero = self.fx.b.ins().iconst(types::I8, 0);
            self.fx.store(zero, flag, 0);
        } else {
            self.set_flag(place.local, false);
        }
    }

    /// Writes an owned copy of the operand's value to `dst`: a move, a deep copy of a
    /// place that keeps its value (value semantics, spec §8), or a constant.
    pub fn operand_into(&mut self, op: &Operand, dst: Value) -> Result<Ty> {
        match op {
            Operand::Const(c, ty) => {
                let ty = self.concrete(*ty);
                self.const_into(c, ty, dst)?;
                Ok(ty)
            }
            Operand::Move(p) => {
                let (addr, ty) = self.place_addr(p)?;
                self.fx.copy_ty(dst, addr, ty);
                if self.fx.needs_drop(ty) {
                    self.mark_moved(p);
                }
                Ok(ty)
            }
            Operand::Copy(p) => {
                let (addr, ty) = self.place_addr(p)?;
                self.fx.clone_into(addr, dst, ty)?;
                Ok(ty)
            }
        }
    }

    /// The scalar value of an operand (read without taking ownership).
    pub fn operand_scalar(&mut self, op: &Operand) -> Result<Value> {
        let ty = self.operand_ty(op)?;
        let Some(t) = self.cx().layout(ty).scalar_ty() else {
            return unsupported(format!("`{}` used as a scalar", self.cx().ty_name(ty)));
        };
        match op {
            Operand::Const(c, _) => self.const_scalar(c, ty, t),
            Operand::Copy(p) | Operand::Move(p) => {
                let (addr, _) = self.place_addr(p)?;
                Ok(self.fx.load(t, addr, 0))
            }
        }
    }

    /// The address of an operand's value (read without taking ownership).
    pub fn operand_addr(&mut self, op: &Operand) -> Result<(Value, Ty)> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => self.place_addr(p),
            Operand::Const(c, ty) => {
                let ty = self.concrete(*ty);
                let slot = self.fx.slot_for(ty);
                self.const_into(c, ty, slot)?;
                if self.fx.needs_drop(ty) {
                    self.const_temps.push((slot, ty));
                }
                Ok((slot, ty))
            }
        }
    }

    /// Drops the constants materialized for reads by the current statement.
    fn drop_const_temps(&mut self) -> Result<()> {
        for (slot, ty) in std::mem::take(&mut self.const_temps) {
            self.fx.drop_at(slot, ty)?;
        }
        Ok(())
    }

    /// A pointer to the value an argument designates: the pointer itself for a reference,
    /// otherwise the value's address.
    pub fn arg_ptr(&mut self, op: &Operand) -> Result<(Value, Ty)> {
        let ty = self.operand_ty(op)?;
        if let TyKind::Ref { inner, .. } = self.cx().interner().kind(ty) {
            Ok((self.operand_scalar(op)?, inner))
        } else {
            self.operand_addr(op)
        }
    }

    /// An owned copy of an operand in a new stack slot.
    pub fn owned_temp(&mut self, op: &Operand) -> Result<(Value, Ty)> {
        let ty = self.operand_ty(op)?;
        let slot = self.fx.slot_for(ty);
        self.operand_into(op, slot)?;
        Ok((slot, ty))
    }

    fn const_scalar(&mut self, c: &Constant, ty: Ty, t: Type) -> Result<Value> {
        Ok(match c {
            Constant::Int(v, _) => self.fx.int_const(t, *v),
            Constant::Float(v, FloatTy::F32) => {
                #[allow(clippy::cast_possible_truncation, reason = "an F32 literal")]
                let f = *v as f32;
                self.fx.b.ins().f32const(f)
            }
            Constant::Float(v, _) => self.fx.b.ins().f64const(*v),
            Constant::Bool(v) => self.fx.b.ins().iconst(types::I8, i64::from(*v)),
            Constant::Char(c) => self.fx.b.ins().iconst(types::I32, i64::from(u32::from(*c))),
            Constant::Str(s) => self.fx.static_string(s)?,
            Constant::Bytes(bytes) => {
                let (addr, len) = self.fx.static_bytes(bytes)?;
                self.fx.rt1("loom_string_from_static", &[addr, len])?
            }
            _ => {
                let slot = self.fx.slot_for(ty);
                self.const_into(c, ty, slot)?;
                self.fx.load(t, slot, 0)
            }
        })
    }

    fn const_into(&mut self, c: &Constant, ty: Ty, dst: Value) -> Result<()> {
        match c {
            Constant::Unit => {}
            Constant::Fn { def, subst } => {
                let s = self.cx().concrete_subst(subst, &self.subst);
                let shim = self.fx.g.func(Instance::FnShim(*def, subst_key(&s)))?;
                let code = self.fx.func_addr(shim);
                let null = self.fx.pconst(0);
                let word = self.fx.word();
                self.fx.store(code, dst, 0);
                self.fx.store(null, dst, word);
            }
            Constant::Item(def) => {
                let Some(body) = self.cx().fn_body(*def) else {
                    return unsupported("a constant without a body");
                };
                let id = self
                    .fx
                    .g
                    .func(Instance::Body(MirSource::Item(body), Vec::new()))?;
                self.fx.call(id, &[dst]);
            }
            _ => {
                let Some(t) = self.cx().layout(ty).scalar_ty() else {
                    return unsupported(format!("constant of type `{}`", self.cx().ty_name(ty)));
                };
                let v = self.const_scalar(c, ty, t)?;
                self.fx.store(v, dst, 0);
            }
        }
        Ok(())
    }

    // ----- statements --------------------------------------------------------------------

    fn statement(&mut self, kind: &StatementKind) -> Result<()> {
        match kind {
            StatementKind::Assign(place, rvalue) => {
                let ty = self.place_type(place)?;
                let tmp = self.fx.slot_for(ty);
                self.rvalue_into(rvalue, tmp, ty)?;
                self.assign_from(place, tmp)
            }
            StatementKind::StorageLive(local) | StatementKind::StorageDead(local) => {
                self.set_flag(*local, false);
                Ok(())
            }
        }
    }

    /// Moves the value at `src` into `place`. The old value is not dropped here: MIR drops
    /// it explicitly before an assignment to an initialized place, as Rust's MIR does.
    pub fn assign_from(&mut self, place: &Place, src: Value) -> Result<()> {
        let (dst, ty) = self.place_addr(place)?;
        self.fx.copy_ty(dst, src, ty);
        if !place.is_behind_ref() {
            if place.proj.is_empty() {
                self.set_flag(place.local, true);
            }
            self.set_path_flags(place.local, &place.proj, true);
        }
        Ok(())
    }

    fn rvalue_into(&mut self, rvalue: &Rvalue, dst: Value, dst_ty: Ty) -> Result<()> {
        match rvalue {
            Rvalue::Use(op) => {
                self.operand_into(op, dst)?;
            }
            Rvalue::Ref { place, .. } => {
                let (addr, _) = self.place_addr(place)?;
                self.fx.store(addr, dst, 0);
            }
            Rvalue::Binary(op, a, b) => {
                let v = self.binary(*op, a, b)?;
                self.fx.store(v, dst, 0);
            }
            Rvalue::Unary(op, a) => {
                let v = self.unary(*op, a)?;
                self.fx.store(v, dst, 0);
            }
            Rvalue::Aggregate(kind, ops) => self.aggregate_into(kind, ops, dst, dst_ty)?,
            Rvalue::Discriminant(place) => {
                let (addr, _) = self.place_addr(place)?;
                let tag = self.fx.load(types::I32, addr, 0);
                let Some(t) = self.cx().layout(dst_ty).scalar_ty() else {
                    return unsupported("discriminant into a non-scalar");
                };
                let v = self.fx.int_convert(tag, types::I32, t, false);
                self.fx.store(v, dst, 0);
            }
            Rvalue::Unsize(op, target) => {
                let target = self.concrete(*target);
                if !matches!(self.cx().interner().kind(target), TyKind::Dyn(..)) {
                    return unsupported(format!("unsizing to `{}`", self.cx().ty_name(target)));
                }
                let src_ty = self.operand_ty(op)?;
                let l = self.cx().layout(src_ty);
                let size = self.fx.pconst(i64::from(l.size));
                let align = self.fx.pconst(i64::from(l.align));
                let data = self.fx.rt1("loom_alloc", &[size, align])?;
                self.operand_into(op, data)?;
                let vtable = crate::glue::vtable(self.fx.g, src_ty, target)?;
                let vt = self.fx.data_addr(vtable);
                let word = self.fx.word();
                self.fx.store(data, dst, 0);
                self.fx.store(vt, dst, word);
            }
        }
        Ok(())
    }

    fn aggregate_into(
        &mut self,
        kind: &AggregateKind,
        ops: &[Operand],
        dst: Value,
        dst_ty: Ty,
    ) -> Result<()> {
        let ptr = self.fx.ptr();
        match kind {
            AggregateKind::Tuple | AggregateKind::Struct(..) => {
                let fields = self.cx().layout(dst_ty).fields;
                for (op, &(offset, _)) in ops.iter().zip(&fields) {
                    let at = self.fx.offset(dst, offset);
                    self.operand_into(op, at)?;
                }
            }
            AggregateKind::Variant(def, _) => {
                let index = self.cx().variant_index(*def);
                let tag = self.fx.b.ins().iconst(types::I32, i64::from(index));
                self.fx.store(tag, dst, 0);
                let fields = self
                    .cx()
                    .layout(dst_ty)
                    .variants
                    .get(index as usize)
                    .cloned()
                    .unwrap_or_default();
                for (op, &(offset, _)) in ops.iter().zip(&fields) {
                    let at = self.fx.offset(dst, offset);
                    self.operand_into(op, at)?;
                }
            }
            AggregateKind::Array(elem) => {
                let elem = self.concrete(*elem);
                let info = self.fx.type_info(elem)?;
                let array = self.fx.rt1("loom_array_new", &[info])?;
                for op in ops {
                    let (tmp, _) = self.owned_temp(op)?;
                    self.fx.rt("loom_array_push", &[array, tmp])?;
                }
                self.fx.store(array, dst, 0);
            }
            AggregateKind::Map(key, value) => {
                let (key, value) = (self.concrete(*key), self.concrete(*value));
                let key_info = self.fx.type_info(key)?;
                let value_info = self.fx.type_info(value)?;
                let map = self.fx.rt1("loom_map_new", &[key_info, value_info])?;
                for pair in ops.chunks(2) {
                    let (k, _) = self.owned_temp(&pair[0])?;
                    let (v, _) = self.owned_temp(&pair[1])?;
                    let old = self.fx.slot_for(value);
                    let replaced = self.fx.rt1("loom_map_insert", &[map, k, v, old])?;
                    if self.fx.needs_drop(value) {
                        let drop_block = self.fx.b.create_block();
                        let cont = self.fx.b.create_block();
                        self.fx.b.ins().brif(replaced, drop_block, &[], cont, &[]);
                        self.fx.b.switch_to_block(drop_block);
                        self.fx.drop_at(old, value)?;
                        self.fx.b.ins().jump(cont, &[]);
                        self.fx.b.switch_to_block(cont);
                    }
                }
                self.fx.store(map, dst, 0);
            }
            AggregateKind::Closure(source) => {
                let closure = self.cx().body(*source)?;
                let key = subst_key(&self.subst);
                let (size, align, fields) = env_layout(self.cx(), closure, &self.subst);
                let size_v = self.fx.pconst(i64::from(size));
                let align_v = self.fx.pconst(i64::from(align));
                let env = self.fx.rt1("loom_alloc", &[size_v, align_v])?;
                let drop_fn = self.fx.g.func(Instance::EnvDrop(*source, key.clone()))?;
                let clone_fn = self.fx.g.func(Instance::EnvClone(*source, key.clone()))?;
                let drop_addr = self.fx.func_addr(drop_fn);
                let clone_addr = self.fx.func_addr(clone_fn);
                let word = self.fx.word();
                self.fx.store(drop_addr, env, 0);
                self.fx.store(clone_addr, env, word);
                for (op, &(offset, _)) in ops.iter().zip(&fields) {
                    let at = self.fx.offset(env, offset);
                    self.operand_into(op, at)?;
                }
                let code_fn = self.fx.g.func(Instance::Body(*source, key))?;
                let code = self.fx.func_addr(code_fn);
                self.fx.store(code, dst, 0);
                self.fx.store(env, dst, word);
                let _ = ptr;
            }
        }
        Ok(())
    }

    // ----- operators ---------------------------------------------------------------------

    #[allow(clippy::many_single_char_names, reason = "operands and their values")]
    fn binary(&mut self, op: BinaryOp, a: &Operand, b: &Operand) -> Result<Value> {
        let ty = self.operand_ty(a)?;
        let interner = self.cx().interner();
        match interner.kind(ty) {
            TyKind::Int(int) => {
                let b_ty = self.operand_ty(b)?;
                let x = self.operand_scalar(a)?;
                let y = self.operand_scalar(b)?;
                let b_int = match interner.kind(b_ty) {
                    TyKind::Int(t) => t,
                    _ => int,
                };
                self.int_binary(op, x, y, int, b_int)
            }
            TyKind::Float(float) => {
                let x = self.operand_scalar(a)?;
                let y = self.operand_scalar(b)?;
                self.float_binary(op, x, y, float)
            }
            TyKind::Bool | TyKind::Char => {
                let x = self.operand_scalar(a)?;
                let y = self.operand_scalar(b)?;
                let ins = self.fx.b.ins();
                Ok(match op {
                    BinaryOp::And | BinaryOp::BitAnd => ins.band(x, y),
                    BinaryOp::Or | BinaryOp::BitOr => ins.bor(x, y),
                    BinaryOp::BitXor => ins.bxor(x, y),
                    _ => match int_cc(op, false) {
                        Some(cc) => ins.icmp(cc, x, y),
                        None => {
                            return unsupported(format!(
                                "operator `{op:?}` on `{}`",
                                self.cx().ty_name(ty)
                            ));
                        }
                    },
                })
            }
            TyKind::Unit => {
                let v = matches!(op, BinaryOp::Eq | BinaryOp::Le | BinaryOp::Ge);
                Ok(self.fx.b.ins().iconst(types::I8, i64::from(v)))
            }
            _ => {
                let (x, xt) = self.arg_ptr(a)?;
                let (y, _) = self.arg_ptr(b)?;
                match op {
                    BinaryOp::Eq | BinaryOp::Ne => {
                        let eq = self.fx.glue(GlueKind::Eq, xt)?;
                        let r = self.fx.call(eq, &[x, y])[0];
                        Ok(if op == BinaryOp::Ne {
                            self.fx.b.ins().bxor_imm_u(r, 1)
                        } else {
                            r
                        })
                    }
                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        let cmp = self.fx.glue(GlueKind::Cmp, xt)?;
                        let c = self.fx.call(cmp, &[x, y])[0];
                        let cc = int_cc(op, true).unwrap_or(IntCC::Equal);
                        Ok(self.fx.b.ins().icmp_imm_s(cc, c, 0))
                    }
                    _ => unsupported(format!("operator `{op:?}` on `{}`", self.cx().ty_name(ty))),
                }
            }
        }
    }

    #[allow(clippy::too_many_lines, reason = "one case per operator")]
    pub(crate) fn int_binary(
        &mut self,
        op: BinaryOp,
        x: Value,
        y: Value,
        int: IntTy,
        b_int: IntTy,
    ) -> Result<Value> {
        let t = crate::layout::int_type(int);
        let signed = int.is_signed();
        let bits = int.bits();
        let span = self.span;
        if let Some(cc) = int_cc(op, signed) {
            return Ok(self.fx.b.ins().icmp(cc, x, y));
        }
        match op {
            BinaryOp::BitAnd => return Ok(self.fx.b.ins().band(x, y)),
            BinaryOp::BitOr => return Ok(self.fx.b.ins().bor(x, y)),
            BinaryOp::BitXor => return Ok(self.fx.b.ins().bxor(x, y)),
            _ => {}
        }
        if bits == 128 {
            return unsupported("128-bit integer arithmetic");
        }
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                let what = match op {
                    BinaryOp::Add => "add",
                    BinaryOp::Sub => "subtract",
                    _ => "multiply",
                };
                let message = format!("attempt to {what} with overflow");
                if bits == 64 {
                    let ins = self.fx.b.ins();
                    let (r, overflow) = match (op, signed) {
                        (BinaryOp::Add, true) => ins.sadd_overflow(x, y),
                        (BinaryOp::Add, false) => ins.uadd_overflow(x, y),
                        (BinaryOp::Sub, true) => ins.ssub_overflow(x, y),
                        (BinaryOp::Sub, false) => ins.usub_overflow(x, y),
                        (_, true) => ins.smul_overflow(x, y),
                        (_, false) => ins.umul_overflow(x, y),
                    };
                    self.fx.panic_if(overflow, &message, span)?;
                    Ok(r)
                } else {
                    let wx = self.fx.int_convert(x, t, types::I64, signed);
                    let wy = self.fx.int_convert(y, t, types::I64, signed);
                    let ins = self.fx.b.ins();
                    let r = match op {
                        BinaryOp::Add => ins.iadd(wx, wy),
                        BinaryOp::Sub => ins.isub(wx, wy),
                        _ => ins.imul(wx, wy),
                    };
                    let out = self.out_of_range(r, int);
                    self.fx.panic_if(out, &message, span)?;
                    Ok(self.fx.b.ins().ireduce(t, r))
                }
            }
            BinaryOp::Div | BinaryOp::Rem => {
                let what = if op == BinaryOp::Div {
                    "divide"
                } else {
                    "calculate the remainder"
                };
                let zero = self.fx.b.ins().icmp_imm_s(IntCC::Equal, y, 0);
                self.fx
                    .panic_if(zero, &format!("attempt to {what} by zero"), span)?;
                if signed {
                    #[allow(clippy::cast_possible_truncation, reason = "the minimum fits the type")]
                    let min = int.min() as i64;
                    let is_min = self.fx.b.ins().icmp_imm_s(IntCC::Equal, x, min);
                    let is_minus_one = self.fx.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
                    let overflow = self.fx.b.ins().band(is_min, is_minus_one);
                    self.fx.panic_if(
                        overflow,
                        &format!("attempt to {what} with overflow"),
                        span,
                    )?;
                }
                let ins = self.fx.b.ins();
                Ok(match (op, signed) {
                    (BinaryOp::Div, true) => ins.sdiv(x, y),
                    (BinaryOp::Div, false) => ins.udiv(x, y),
                    (_, true) => ins.srem(x, y),
                    (_, false) => ins.urem(x, y),
                })
            }
            BinaryOp::Shl | BinaryOp::Shr => {
                let bt = crate::layout::int_type(b_int);
                let wide = self.fx.int_convert(y, bt, types::I64, b_int.is_signed());
                let out = self.fx.b.ins().icmp_imm_s(
                    IntCC::UnsignedGreaterThanOrEqual,
                    wide,
                    i64::from(bits),
                );
                self.fx.panic_if(
                    out,
                    &format!("shift amount is out of range for `{}`", int.name()),
                    span,
                )?;
                let ins = self.fx.b.ins();
                Ok(match (op, signed) {
                    (BinaryOp::Shl, _) => ins.ishl(x, y),
                    (_, true) => ins.sshr(x, y),
                    (_, false) => ins.ushr(x, y),
                })
            }
            _ => unsupported(format!("operator `{op:?}` on `{}`", int.name())),
        }
    }

    /// Whether a widened 64-bit result `r` does not fit `int` (narrower than 64 bits).
    fn out_of_range(&mut self, r: Value, int: IntTy) -> Value {
        #[allow(clippy::cast_possible_truncation, reason = "bounds of a narrow type")]
        let (min, max) = (int.min() as i64, int.max() as i64);
        let ins = self.fx.b.ins();
        if int.is_signed() {
            let low = ins.icmp_imm_s(IntCC::SignedLessThan, r, min);
            let high = self.fx.b.ins().icmp_imm_s(IntCC::SignedGreaterThan, r, max);
            self.fx.b.ins().bor(low, high)
        } else {
            ins.icmp_imm_s(IntCC::UnsignedGreaterThan, r, max)
        }
    }

    fn float_binary(&mut self, op: BinaryOp, x: Value, y: Value, float: FloatTy) -> Result<Value> {
        let ins = self.fx.b.ins();
        Ok(match op {
            BinaryOp::Add => ins.fadd(x, y),
            BinaryOp::Sub => ins.fsub(x, y),
            BinaryOp::Mul => ins.fmul(x, y),
            BinaryOp::Div => ins.fdiv(x, y),
            BinaryOp::Rem => {
                let (wx, wy) = if float == FloatTy::F32 {
                    (
                        ins.fpromote(types::F64, x),
                        self.fx.b.ins().fpromote(types::F64, y),
                    )
                } else {
                    (x, y)
                };
                let r = self.fx.rt1("loom_float_rem", &[wx, wy])?;
                if float == FloatTy::F32 {
                    self.fx.b.ins().fdemote(types::F32, r)
                } else {
                    r
                }
            }
            BinaryOp::Eq => ins.fcmp(FloatCC::Equal, x, y),
            BinaryOp::Ne => ins.fcmp(FloatCC::NotEqual, x, y),
            BinaryOp::Lt => ins.fcmp(FloatCC::LessThan, x, y),
            BinaryOp::Le => ins.fcmp(FloatCC::LessThanOrEqual, x, y),
            BinaryOp::Gt => ins.fcmp(FloatCC::GreaterThan, x, y),
            BinaryOp::Ge => ins.fcmp(FloatCC::GreaterThanOrEqual, x, y),
            _ => return unsupported(format!("operator `{op:?}` on floats")),
        })
    }

    fn unary(&mut self, op: UnaryOp, a: &Operand) -> Result<Value> {
        let ty = self.operand_ty(a)?;
        let x = self.operand_scalar(a)?;
        let span = self.span;
        match (op, self.cx().interner().kind(ty)) {
            (UnaryOp::Neg, TyKind::Int(int)) => {
                if int.bits() == 128 {
                    return unsupported("128-bit negation");
                }
                if int.is_signed() {
                    #[allow(clippy::cast_possible_truncation, reason = "the minimum fits the type")]
                    let min = int.min() as i64;
                    let overflow = self.fx.b.ins().icmp_imm_s(IntCC::Equal, x, min);
                    self.fx
                        .panic_if(overflow, "attempt to negate with overflow", span)?;
                } else {
                    let overflow = self.fx.b.ins().icmp_imm_s(IntCC::NotEqual, x, 0);
                    self.fx
                        .panic_if(overflow, "attempt to negate with overflow", span)?;
                }
                Ok(self.fx.b.ins().ineg(x))
            }
            (UnaryOp::Neg, TyKind::Float(_)) => Ok(self.fx.b.ins().fneg(x)),
            (UnaryOp::Not, TyKind::Bool) => Ok(self.fx.b.ins().bxor_imm_u(x, 1)),
            (UnaryOp::Not | UnaryOp::BitNot, TyKind::Int(_)) => Ok(self.fx.b.ins().bnot(x)),
            _ => unsupported(format!("operator `{op:?}` on `{}`", self.cx().ty_name(ty))),
        }
    }

    // ----- terminators -------------------------------------------------------------------

    fn terminator(&mut self, kind: &TerminatorKind) -> Result<()> {
        match kind {
            TerminatorKind::Goto(target) => {
                self.fx.b.ins().jump(self.blocks[target.index()], &[]);
            }
            TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise,
            } => {
                let v = self.operand_scalar(discr)?;
                let mut switch = Switch::new();
                for &(value, target) in targets {
                    switch.set_entry(value, self.blocks[target.index()]);
                }
                switch.emit(&mut self.fx.b, v, self.blocks[otherwise.index()]);
            }
            TerminatorKind::Call {
                callee,
                args,
                dest,
                target,
            } => {
                self.call(callee, args, dest)?;
                self.drop_const_temps()?;
                match target {
                    Some(t) => {
                        self.fx.b.ins().jump(self.blocks[t.index()], &[]);
                    }
                    None => {
                        self.fx.b.ins().trap(panic_trap());
                    }
                }
            }
            TerminatorKind::Drop { place, target } => {
                self.drop_place(place)?;
                self.fx.b.ins().jump(self.blocks[target.index()], &[]);
            }
            TerminatorKind::Return => {
                self.fx.b.ins().return_(&[]);
            }
            TerminatorKind::Panic(message) => {
                let span = self.span;
                self.fx.panic(message, span)?;
                self.fx.b.ins().trap(panic_trap());
            }
            TerminatorKind::Unreachable => {
                self.fx.b.ins().trap(panic_trap());
            }
        }
        Ok(())
    }

    fn drop_place(&mut self, place: &Place) -> Result<()> {
        let ty = self.place_type(place)?;
        if !self.fx.needs_drop(ty) {
            return Ok(());
        }
        if place.is_behind_ref() || place.proj.iter().any(|p| matches!(p, Proj::Index(_))) {
            let (addr, ty) = self.place_addr(place)?;
            return self.fx.drop_at(addr, ty);
        }
        let Some(flag) = self.locals[place.local.index()].flag else {
            return Ok(());
        };
        let set = self.fx.load(types::I8, flag, 0);
        let drop_block = self.fx.b.create_block();
        let cont = self.fx.b.create_block();
        self.fx.b.ins().brif(set, drop_block, &[], cont, &[]);
        self.fx.b.switch_to_block(drop_block);
        let (addr, ty) = self.place_addr(place)?;
        self.drop_elaborated(place.local, &place.proj, ty, addr)?;
        if place.proj.is_empty() {
            let zero = self.fx.b.ins().iconst(types::I8, 0);
            self.fx.store(zero, flag, 0);
        }
        self.set_path_flags(place.local, &place.proj, false);
        self.fx.b.ins().jump(cont, &[]);
        self.fx.b.switch_to_block(cont);
        Ok(())
    }

    // ----- calls -------------------------------------------------------------------------

    fn call(&mut self, callee: &Callee, args: &[Operand], dest: &Place) -> Result<()> {
        match callee {
            Callee::Fn { def, subst } => {
                let s = self.cx().concrete_subst(subst, &self.subst);
                self.call_fn(*def, &s, args, dest)
            }
            Callee::Trait {
                method,
                self_ty,
                subst,
            } => {
                let self_ty = self.concrete(*self_ty);
                let s = self.cx().concrete_subst(subst, &self.subst);
                if let TyKind::Dyn(trait_, _) = self.cx().interner().kind(self_ty) {
                    return self.dyn_call(trait_, *method, args, dest);
                }
                match self.cx().resolve_trait_method(*method, self_ty, &s)? {
                    Resolved::Fn(def, s) => self.call_fn(def, &s, args, dest),
                    Resolved::Builtin(lang, name) => {
                        self.builtin_method(&lang, &name, self_ty, args, dest)
                    }
                }
            }
            Callee::Value(op) => self.call_value(op, args, dest),
            Callee::Default { def, index, subst } => {
                let s = self.cx().concrete_subst(subst, &self.subst);
                let Some(body) = self.cx().fn_body(*def) else {
                    return unsupported("a default argument without a body");
                };
                self.call_instance(
                    Instance::Body(MirSource::Default(body, *index), subst_key(&s)),
                    args,
                    dest,
                    None,
                )
            }
            Callee::Builtin(name) => self.builtin(name, args, dest),
        }
    }

    fn call_fn(
        &mut self,
        def: loom_hir::DefId,
        subst: &Subst,
        args: &[Operand],
        dest: &Place,
    ) -> Result<()> {
        if self.cx().is_intrinsic(def) {
            let name = self.cx().hir.def(def).name.to_string();
            return self.intrinsic(&name, def, subst, args, dest);
        }
        match &self.cx().hir.def(def).kind {
            DefKind::Variant(_) | DefKind::Struct(_) => {
                let ty = self.place_type(dest)?;
                let kind = if matches!(self.cx().hir.def(def).kind, DefKind::Variant(_)) {
                    AggregateKind::Variant(def, subst.clone())
                } else {
                    AggregateKind::Struct(def, subst.clone())
                };
                let tmp = self.fx.slot_for(ty);
                self.aggregate_into(&kind, args, tmp, ty)?;
                return self.assign_from(dest, tmp);
            }
            _ => {}
        }
        let Some(body) = self.cx().fn_body(def) else {
            return unsupported(format!(
                "a call to `{}`, which has no body",
                self.cx().hir.def(def).name
            ));
        };
        self.call_instance(
            Instance::Body(MirSource::Item(body), subst_key(subst)),
            args,
            dest,
            None,
        )
    }

    /// The ABI value of an argument (an owned copy; `None` for a zero-size value).
    fn abi_arg(&mut self, op: &Operand) -> Result<Option<Value>> {
        let ty = self.operand_ty(op)?;
        let layout = self.cx().layout(ty);
        match layout.repr {
            Repr::Zero => {
                if let Operand::Move(p) = op {
                    self.mark_moved(p);
                }
                Ok(None)
            }
            Repr::Scalar(t) => {
                if !self.fx.needs_drop(ty) {
                    return Ok(Some(self.operand_scalar(op)?));
                }
                let (tmp, _) = self.owned_temp(op)?;
                Ok(Some(self.fx.load(t, tmp, 0)))
            }
            Repr::Memory => Ok(Some(self.owned_temp(op)?.0)),
        }
    }

    fn call_instance(
        &mut self,
        instance: Instance,
        args: &[Operand],
        dest: &Place,
        env: Option<Value>,
    ) -> Result<()> {
        let id = self.fx.g.func(instance)?;
        let ret_ty = self.place_type(dest)?;
        let ret = self.fx.slot_for(ret_ty);
        let mut values = vec![ret];
        values.extend(env);
        for op in args {
            if let Some(v) = self.abi_arg(op)? {
                values.push(v);
            }
        }
        self.fx.call(id, &values);
        self.assign_from(dest, ret)
    }

    fn call_value(&mut self, callee: &Operand, args: &[Operand], dest: &Place) -> Result<()> {
        let ptr = self.fx.ptr();
        let word = self.fx.word();
        let (mut pair, mut ty) = self.operand_addr(callee)?;
        while let TyKind::Ref { inner, .. } = self.cx().interner().kind(ty) {
            pair = self.fx.load(ptr, pair, 0);
            ty = inner;
        }
        let code = self.fx.load(ptr, pair, 0);
        let env = self.fx.load(ptr, pair, word);
        let ret_ty = self.place_type(dest)?;
        let ret = self.fx.slot_for(ret_ty);
        let mut values = vec![ret, env];
        let mut arg_types = Vec::new();
        for op in args {
            arg_types.push(self.operand_ty(op)?);
            if let Some(v) = self.abi_arg(op)? {
                values.push(v);
            }
        }
        let sig = self.fx.g.loom_signature(&arg_types, true);
        self.fx.call_indirect(sig, code, &values);
        self.assign_from(dest, ret)
    }

    fn dyn_call(
        &mut self,
        trait_: loom_hir::DefId,
        method: loom_hir::DefId,
        args: &[Operand],
        dest: &Place,
    ) -> Result<()> {
        let ptr = self.fx.ptr();
        let word = self.fx.word();
        let Some(receiver) = args.first() else {
            return unsupported("a `dyn` call without a receiver");
        };
        let receiver_ty = self.operand_ty(receiver)?;
        if !matches!(self.cx().interner().kind(receiver_ty), TyKind::Ref { .. }) {
            return unsupported("a `dyn` method taking `own self`");
        }
        let Some(slot) = crate::glue::vtable_slot(self.cx(), trait_, method) else {
            return unsupported("a `dyn` call to a method of another trait");
        };
        let pair = self.operand_scalar(receiver)?;
        let data = self.fx.load(ptr, pair, 0);
        let vtable = self.fx.load(ptr, pair, word);
        let code = self
            .fx
            .load(ptr, vtable, (crate::glue::VTABLE_METHODS + slot) * word);
        let ret_ty = self.place_type(dest)?;
        let ret = self.fx.slot_for(ret_ty);
        let mut values = vec![ret, data];
        let mut arg_types = vec![receiver_ty];
        for op in &args[1..] {
            arg_types.push(self.operand_ty(op)?);
            if let Some(v) = self.abi_arg(op)? {
                values.push(v);
            }
        }
        let sig = self.fx.g.loom_signature(&arg_types, false);
        self.fx.call_indirect(sig, code, &values);
        self.assign_from(dest, ret)
    }

    fn builtin_method(
        &mut self,
        lang: &str,
        name: &str,
        self_ty: Ty,
        args: &[Operand],
        dest: &Place,
    ) -> Result<()> {
        let ret_ty = self.place_type(dest)?;
        let ret = self.fx.slot_for(ret_ty);
        match (lang, name) {
            ("Eq", "eq" | "ne") => {
                let (a, _) = self.arg_ptr(&args[0])?;
                let (b, _) = self.arg_ptr(&args[1])?;
                let eq = self.fx.glue(GlueKind::Eq, self_ty)?;
                let mut r = self.fx.call(eq, &[a, b])[0];
                if name == "ne" {
                    r = self.fx.b.ins().bxor_imm_u(r, 1);
                }
                self.fx.store(r, ret, 0);
            }
            ("Ord", "compare") => {
                let (a, _) = self.arg_ptr(&args[0])?;
                let (b, _) = self.arg_ptr(&args[1])?;
                let cmp = self.fx.glue(GlueKind::Cmp, self_ty)?;
                let c = self.fx.call(cmp, &[a, b])[0];
                let tag = self.fx.ordering_tag(c)?;
                self.fx.store(tag, ret, 0);
            }
            ("Clone", "clone") => {
                let (a, _) = self.arg_ptr(&args[0])?;
                self.fx.clone_into(a, ret, self_ty)?;
            }
            _ => return unsupported(format!("built-in `{lang}.{name}`")),
        }
        self.assign_from(dest, ret)
    }
}

/// The comparison condition of a comparison operator.
fn int_cc(op: BinaryOp, signed: bool) -> Option<IntCC> {
    Some(match (op, signed) {
        (BinaryOp::Eq, _) => IntCC::Equal,
        (BinaryOp::Ne, _) => IntCC::NotEqual,
        (BinaryOp::Lt, true) => IntCC::SignedLessThan,
        (BinaryOp::Le, true) => IntCC::SignedLessThanOrEqual,
        (BinaryOp::Gt, true) => IntCC::SignedGreaterThan,
        (BinaryOp::Ge, true) => IntCC::SignedGreaterThanOrEqual,
        (BinaryOp::Lt, false) => IntCC::UnsignedLessThan,
        (BinaryOp::Le, false) => IntCC::UnsignedLessThanOrEqual,
        (BinaryOp::Gt, false) => IntCC::UnsignedGreaterThan,
        (BinaryOp::Ge, false) => IntCC::UnsignedGreaterThanOrEqual,
        _ => return None,
    })
}

/// A function or constructor called through the closure convention: every argument
/// arrives as a pointer to the value.
fn fn_shim(
    fx: &mut Fx<'_, '_, '_, '_>,
    def: loom_hir::DefId,
    subst: &Subst,
    params: &[Value],
) -> Result<()> {
    let ret = params[0];
    let args = &params[2..];
    let cx_kind = fx.cx().hir.def(def).kind.clone();
    match cx_kind {
        DefKind::Fn(_) => {
            if fx.cx().is_intrinsic(def) {
                return unsupported("an intrinsic used as a value");
            }
            let Some(body_id) = fx.cx().fn_body(def) else {
                return unsupported("a function without a body used as a value");
            };
            let body = fx.cx().body(MirSource::Item(body_id))?;
            let arg_types = fx.g.body_arg_types(body, subst);
            let mut values = vec![ret];
            for (&p, &ty) in args.iter().zip(&arg_types) {
                let interner = fx.cx().interner();
                if matches!(interner.kind(ty), TyKind::Ref { .. }) {
                    values.push(p);
                    continue;
                }
                let layout = fx.cx().layout(ty);
                match layout.repr {
                    Repr::Zero => {}
                    Repr::Scalar(t) => {
                        let tmp = fx.slot_for(ty);
                        fx.clone_into(p, tmp, ty)?;
                        values.push(fx.load(t, tmp, 0));
                    }
                    Repr::Memory => {
                        let tmp = fx.slot_for(ty);
                        fx.clone_into(p, tmp, ty)?;
                        values.push(tmp);
                    }
                }
            }
            let id =
                fx.g.func(Instance::Body(MirSource::Item(body_id), subst_key(subst)))?;
            fx.call(id, &values);
        }
        DefKind::Variant(_) | DefKind::Struct(_) => {
            let interner = fx.cx().interner();
            let parent = if matches!(cx_kind, DefKind::Variant(_)) {
                fx.cx().hir.def(def).parent.unwrap_or(def)
            } else {
                def
            };
            let generics = fx.cx().items().generics_of(parent);
            let ty_args: Vec<Ty> = generics
                .own
                .iter()
                .map(|&p| subst.get(p).unwrap_or_else(|| interner.error()))
                .collect();
            let ty = interner.intern(TyKind::Adt(parent, ty_args));
            let layout = fx.cx().layout(ty);
            let fields = if matches!(cx_kind, DefKind::Variant(_)) {
                let index = fx.cx().variant_index(def);
                let tag = fx.b.ins().iconst(types::I32, i64::from(index));
                fx.store(tag, ret, 0);
                layout
                    .variants
                    .get(index as usize)
                    .cloned()
                    .unwrap_or_default()
            } else {
                layout.fields.clone()
            };
            for (&p, &(offset, fty)) in args.iter().zip(&fields) {
                let at = fx.offset(ret, offset);
                fx.clone_into(p, at, fty)?;
            }
        }
        _ => return unsupported("a value that is not a function"),
    }
    fx.b.ins().return_(&[]);
    Ok(())
}

fn env_drop(
    fx: &mut Fx<'_, '_, '_, '_>,
    source: MirSource,
    subst: &Subst,
    env: Value,
) -> Result<()> {
    let body = fx.cx().body(source)?;
    let (size, align, fields) = env_layout(fx.cx(), body, subst);
    for (capture, &(offset, ty)) in body.captures.iter().zip(&fields) {
        if capture.by_ref.is_none() {
            let at = fx.offset(env, offset);
            fx.drop_at(at, ty)?;
        }
    }
    let size = fx.pconst(i64::from(size));
    let align = fx.pconst(i64::from(align));
    fx.rt("loom_free", &[env, size, align])?;
    fx.b.ins().return_(&[]);
    Ok(())
}

fn env_clone(
    fx: &mut Fx<'_, '_, '_, '_>,
    source: MirSource,
    subst: &Subst,
    env: Value,
) -> Result<()> {
    let body = fx.cx().body(source)?;
    let (size, align, fields) = env_layout(fx.cx(), body, subst);
    let size_v = fx.pconst(i64::from(size));
    let align_v = fx.pconst(i64::from(align));
    let copy = fx.rt1("loom_alloc", &[size_v, align_v])?;
    let word = fx.word();
    fx.copy(copy, env, 2 * word, word);
    for (capture, &(offset, ty)) in body.captures.iter().zip(&fields) {
        let src = fx.offset(env, offset);
        let dst = fx.offset(copy, offset);
        if capture.by_ref.is_some() {
            fx.copy_ty(dst, src, ty);
        } else {
            fx.clone_into(src, dst, ty)?;
        }
    }
    fx.b.ins().return_(&[copy]);
    Ok(())
}

/// `loom_main`: runs `main`, reports an `Err` result (spec §14.2), flushes output.
fn entry_point(fx: &mut Fx<'_, '_, '_, '_>, main: loom_hir::DefId) -> Result<()> {
    let Some(body_id) = fx.cx().fn_body(main) else {
        return unsupported("`main` has no body");
    };
    let body = fx.cx().body(MirSource::Item(body_id))?;
    if body.arg_count != 0 {
        return unsupported("`main` with parameters");
    }
    let ret_ty = fx.cx().concrete(body.locals[0].ty, &Subst::new());
    let ret = fx.slot_for(ret_ty);
    let id =
        fx.g.func(Instance::Body(MirSource::Item(body_id), Vec::new()))?;
    fx.call(id, &[ret]);
    if let TyKind::Adt(def, args) = fx.cx().interner().kind(ret_ty)
        && Some(def) == fx.cx().lang("Result")
    {
        let err = fx.lang_tag("Result", "Err")?;
        let tag = fx.load(types::I32, ret, 0);
        let is_err = fx.b.ins().icmp_imm_s(IntCC::Equal, tag, i64::from(err));
        let fail = fx.b.create_block();
        let done = fx.b.create_block();
        fx.b.ins().brif(is_err, fail, &[], done, &[]);
        fx.b.switch_to_block(fail);
        let error_ty = args
            .get(1)
            .copied()
            .unwrap_or_else(|| fx.cx().interner().error());
        let payload = fx.offset(ret, fx.payload_offset(ret_ty, err));
        let Some(method) = fx.cx().lang_method("Error", "message") else {
            return unsupported("missing Error.message");
        };
        let Resolved::Fn(def, s) = fx
            .cx()
            .resolve_trait_method(method, error_ty, &Subst::new())?
        else {
            return unsupported("a built-in Error.message");
        };
        let Some(message_body) = fx.cx().fn_body(def) else {
            return unsupported("Error.message without a body");
        };
        let string = fx.cx().interner().string();
        let message = fx.slot_for(string);
        let mid =
            fx.g.func(Instance::Body(MirSource::Item(message_body), subst_key(&s)))?;
        fx.call(mid, &[message, payload]);
        let ptr = fx.ptr();
        let handle = fx.load(ptr, message, 0);
        fx.rt("loom_exit_error", &[handle])?;
        fx.b.ins().trap(panic_trap());
        fx.b.switch_to_block(done);
    }
    fx.rt("loom_flush", &[])?;
    let zero = fx.b.ins().iconst(types::I32, 0);
    fx.b.ins().return_(&[zero]);
    Ok(())
}

/// Defines the C `main` of an executable, which calls `loom_main`.
pub(crate) fn define_c_main(module: &mut dyn Module, entry: FuncId) -> Result<()> {
    let ptr = module.target_config().pointer_type();
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(types::I32));
    sig.params.push(AbiParam::new(ptr));
    sig.returns.push(AbiParam::new(types::I32));
    let id = module
        .declare_function("main", Linkage::Export, &sig)
        .map_err(internal)?;
    let mut ctx = cranelift_codegen::Context::new();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        let r = module.declare_func_in_func(entry, b.func);
        let inst = b.ins().call(r, &[]);
        let status = b.inst_results(inst)[0];
        b.ins().return_(&[status]);
        b.seal_all_blocks();
        b.finalize(module.target_config());
    }
    module.define_function(id, &mut ctx).map_err(internal)
}
