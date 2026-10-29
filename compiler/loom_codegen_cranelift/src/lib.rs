//! The Cranelift backend (ADR 0021): monomorphized MIR to machine code, run in-process
//! with the JIT (`loom run`) or written as an object file and linked (`loom build`).
//!
//! The backend is complete for the constructs it accepts and reports
//! [`Error::Unsupported`] for the rest, so the driver can fall back to the interpreter.

mod func;
mod glue;
mod intrinsics;
mod layout;
pub mod runtime;

use cranelift_codegen::ir::{AbiParam, Signature, Type, types};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{Context, ir};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use loom_check::{Checked, Env, Items, Selection, Solver};
use loom_hir::{DefId, DefKind, Hir};
use loom_mir::{Mir, MirBody, MirSource};
use loom_span::{SourceMap, Span};
use loom_types::{InferTable, Interner, Subst, Ty, TyKind};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;

pub use layout::{Layout, Repr};

/// The name of this crate, used by the driver for diagnostics about the pipeline.
pub const CRATE_NAME: &str = "loom_codegen_cranelift";

/// Why a program could not be compiled.
#[derive(Debug)]
pub enum Error {
    /// The program uses a construct the backend does not support yet.
    Unsupported(String),
    /// Cranelift rejected the generated code (a backend bug).
    Internal(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "not supported by the native backend yet: {what}"),
            Self::Internal(what) => write!(f, "internal error in the native backend: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// A result of the backend.
pub type Result<T> = std::result::Result<T, Error>;

fn unsupported<T>(what: impl Into<String>) -> Result<T> {
    Err(Error::Unsupported(what.into()))
}

fn internal(error: impl fmt::Display) -> Error {
    Error::Internal(error.to_string())
}

/// A checked program with its MIR.
#[derive(Clone, Copy)]
pub struct Program<'a> {
    /// The program's HIR (core library included).
    pub hir: &'a Hir,
    /// Types and signatures.
    pub checked: &'a Checked,
    /// MIR of every body.
    pub mir: &'a Mir,
    /// Source files, for panic locations.
    pub sources: &'a SourceMap,
}

/// The substitution of an instance, in a hashable form.
pub(crate) type SubstKey = Vec<(loom_resolve::TyParamId, Ty)>;

pub(crate) fn subst_key(subst: &Subst) -> SubstKey {
    let mut key: SubstKey = subst.iter().collect();
    key.sort_by_key(|&(p, t)| (p, t));
    key
}

pub(crate) fn subst_of(key: &SubstKey) -> Subst {
    let mut subst = Subst::new();
    for &(p, t) in key {
        subst.insert(p, t);
    }
    subst
}

/// Glue generated per concrete type (ADR 0021 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum GlueKind {
    Drop,
    Clone,
    Eq,
    Cmp,
    Hash,
    Debug,
}

/// One function of the native program.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Instance {
    /// A MIR body under a concrete substitution.
    Body(MirSource, SubstKey),
    /// A function or constructor used as a value: the closure calling convention.
    FnShim(DefId, SubstKey),
    /// Drops a closure environment and frees it.
    EnvDrop(MirSource, SubstKey),
    /// Clones a closure environment.
    EnvClone(MirSource, SubstKey),
    /// Per-type glue.
    Glue(GlueKind, Ty),
    /// The program entry: runs `main` and handles its result.
    Entry(DefId),
}

/// Data objects of the native program.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DataKey {
    /// Static bytes (string literals, file names).
    Bytes(Vec<u8>),
    /// The type descriptor of a concrete type.
    TypeInfo(Ty),
    /// The vtable of a concrete type for a trait object type.
    Vtable(Ty, Ty),
}

/// How a trait method call is resolved for concrete types.
pub(crate) enum Resolved {
    /// A function body with its substitution.
    Fn(DefId, Subst),
    /// A built-in implementation (primitives, tuples, derived impls).
    Builtin(String, String),
}

/// Shared analysis context.
pub(crate) struct Cx<'a> {
    pub hir: &'a Hir,
    pub checked: &'a Checked,
    pub mir: &'a Mir,
    pub sources: &'a SourceMap,
    pub ptr_ty: Type,
    pub call_conv: CallConv,
    pub layouts: RefCell<HashMap<Ty, Layout>>,
}

impl<'a> Cx<'a> {
    pub fn interner(&self) -> &'a Interner {
        &self.checked.interner
    }

    pub fn items(&self) -> &'a Items {
        &self.checked.items
    }

    /// Normalizes associated types in a concrete type.
    pub fn normalize(&self, ty: Ty) -> Ty {
        let mut table = InferTable::new();
        Solver {
            hir: self.hir,
            items: self.items(),
            interner: self.interner(),
            table: &mut table,
        }
        .normalize(ty, &Env::default())
    }

    /// `ty` made concrete under `subst`.
    pub fn concrete(&self, ty: Ty, subst: &Subst) -> Ty {
        let ty = self.interner().subst(ty, subst);
        self.normalize(ty)
    }

    /// `subst` with every type made concrete under `outer`.
    pub fn concrete_subst(&self, subst: &Subst, outer: &Subst) -> Subst {
        subst.map(|t| self.concrete(t, outer))
    }

    /// The display name of a type, for messages.
    pub fn ty_name(&self, ty: Ty) -> String {
        loom_check::display::ty_to_string(self.hir, self.items(), self.interner(), ty)
    }

    /// The lang item `name`.
    pub fn lang(&self, name: &str) -> Option<DefId> {
        self.items().lang.get(name)
    }

    /// The position of variant `variant` in its enum.
    pub fn variant_index(&self, variant: DefId) -> u32 {
        match &self.hir.def(variant).kind {
            DefKind::Variant(v) => v.index,
            _ => 0,
        }
    }

    /// The variant of lang enum `enum_lang` named `name`.
    pub fn lang_variant(&self, enum_lang: &str, name: &str) -> Option<DefId> {
        let def = self.lang(enum_lang)?;
        match &self.hir.def(def).kind {
            DefKind::Enum(data) => data
                .variants
                .iter()
                .copied()
                .find(|&v| self.hir.def(v).name.as_str() == name),
            _ => None,
        }
    }

    /// The MIR body built from `source`.
    pub fn body(&self, source: MirSource) -> Result<&'a MirBody> {
        match self.mir.body(source) {
            Some(b) => Ok(b),
            None => unsupported(format!("missing MIR for {source:?}")),
        }
    }

    /// The body of function `def`, if it has one.
    pub fn fn_body(&self, def: DefId) -> Option<loom_hir::BodyId> {
        match &self.hir.def(def).kind {
            DefKind::Fn(f) => f.body,
            DefKind::Const(c) => c.body,
            _ => None,
        }
    }

    /// Whether `def` is a core intrinsic.
    pub fn is_intrinsic(&self, def: DefId) -> bool {
        matches!(&self.hir.def(def).kind, DefKind::Fn(f) if f.extern_abi.as_deref() == Some("intrinsic"))
    }

    /// The file, line and column of `span` (spec §10.4).
    pub fn position(&self, span: Span) -> (String, u32, u32) {
        if span.file.raw() == u32::MAX {
            return (String::new(), 0, 0);
        }
        let file = self.sources.file(span.file);
        let position = file.line_column(span.range.start());
        (file.name().to_owned(), position.line, position.column)
    }

    /// Resolves trait method `method` for concrete `self_ty` (as the interpreter does).
    pub fn resolve_trait_method(
        &self,
        method: DefId,
        self_ty: Ty,
        subst: &Subst,
    ) -> Result<Resolved> {
        let interner = self.interner();
        let Some(trait_) = self.hir.def(method).parent else {
            return unsupported("a trait method without a trait");
        };
        let generics = self.items().generics_of(trait_);
        let trait_ref = loom_check::TraitRef {
            trait_,
            args: generics
                .own
                .iter()
                .map(|&p| subst.get(p).unwrap_or_else(|| interner.error()))
                .collect(),
        };
        let mut table = InferTable::new();
        let selection = Solver {
            hir: self.hir,
            items: self.items(),
            interner,
            table: &mut table,
        }
        .select(self_ty, &trait_ref, &Env::default());
        let name = self.hir.def(method).name.clone();
        let lang = self.hir.def(trait_).lang().unwrap_or("").to_owned();
        let mut full = subst.clone();
        full.insert(Items::self_param(trait_), self_ty);
        match selection {
            Ok(Selection::Impl {
                impl_id,
                subst: impl_subst,
            }) => {
                let info = &self.items().impls[impl_id];
                if let loom_check::ImplKind::Derived(_) = info.kind {
                    return Ok(Resolved::Builtin(lang, name.to_string()));
                }
                match info.methods.get(&name) {
                    Some(&impl_method) => {
                        let mut s = impl_subst;
                        let decl_own = self.items().generics_of(method).own;
                        let impl_own = self.items().generics_of(impl_method).own;
                        for (&d, &i) in decl_own.iter().zip(&impl_own) {
                            if let Some(t) = subst.get(d) {
                                s.insert(i, t);
                            }
                        }
                        Ok(Resolved::Fn(impl_method, s))
                    }
                    None => Ok(Resolved::Fn(method, full)),
                }
            }
            Ok(Selection::Builtin) => Ok(Resolved::Builtin(lang, name.to_string())),
            other => unsupported(format!(
                "dispatch of `{name}` for `{}` ({other:?})",
                self.ty_name(self_ty)
            )),
        }
    }

    /// The method `name` of lang trait `trait_lang`.
    pub fn lang_method(&self, trait_lang: &str, name: &str) -> Option<DefId> {
        let t = self.lang(trait_lang)?;
        self.items()
            .traits
            .get(&t)?
            .methods
            .get(&loom_hir::Name::new(name))
            .copied()
    }

    /// The user `impl` of lang trait `trait_lang` for `ty`, if the type has an `impl` item
    /// (not a derived or built-in one): the method `method` with its substitution.
    pub fn user_impl(&self, trait_lang: &str, method: &str, ty: Ty) -> Option<(DefId, Subst)> {
        let decl = self.lang_method(trait_lang, method)?;
        match self.resolve_trait_method(decl, ty, &Subst::new()).ok()? {
            Resolved::Fn(def, subst) if self.fn_body(def).is_some() && !self.is_trait_decl(def) => {
                Some((def, subst))
            }
            _ => None,
        }
    }

    fn is_trait_decl(&self, def: DefId) -> bool {
        self.hir
            .def(def)
            .parent
            .is_some_and(|p| matches!(self.hir.def(p).kind, DefKind::Trait(_)))
    }
}

/// Code generation state over a Cranelift module.
pub(crate) struct Gen<'a, 'm> {
    pub cx: Cx<'a>,
    pub module: &'m mut dyn Module,
    pub funcs: HashMap<Instance, FuncId>,
    pub queue: Vec<(Instance, FuncId)>,
    pub data: HashMap<DataKey, DataId>,
    pub runtime: HashMap<&'static str, FuncId>,
    pub counter: u32,
}

impl<'a> Gen<'a, '_> {
    /// The function for `instance`, declared (and queued) on first use.
    pub fn func(&mut self, instance: Instance) -> Result<FuncId> {
        if let Some(&id) = self.funcs.get(&instance) {
            return Ok(id);
        }
        let sig = self.instance_signature(&instance)?;
        self.counter += 1;
        let (name, linkage) = match &instance {
            Instance::Entry(_) => ("loom_main".to_owned(), Linkage::Export),
            _ => (format!("loom_f{}", self.counter), Linkage::Local),
        };
        let id = self
            .module
            .declare_function(&name, linkage, &sig)
            .map_err(internal)?;
        self.funcs.insert(instance.clone(), id);
        self.queue.push((instance, id));
        Ok(id)
    }

    /// The runtime function `name`.
    pub fn runtime_fn(&mut self, name: &'static str) -> Result<FuncId> {
        if let Some(&id) = self.runtime.get(name) {
            return Ok(id);
        }
        let sig = runtime::signature(name, self.cx.ptr_ty, self.cx.call_conv);
        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(internal)?;
        self.runtime.insert(name, id);
        Ok(id)
    }

    /// A data object with static bytes.
    pub fn bytes(&mut self, bytes: &[u8]) -> Result<DataId> {
        let key = DataKey::Bytes(bytes.to_vec());
        if let Some(&id) = self.data.get(&key) {
            return Ok(id);
        }
        self.counter += 1;
        let id = self
            .module
            .declare_data(
                &format!("loom_d{}", self.counter),
                Linkage::Local,
                false,
                false,
            )
            .map_err(internal)?;
        let mut description = DataDescription::new();
        let mut contents = bytes.to_vec();
        contents.push(0);
        description.define(contents.into_boxed_slice());
        self.module
            .define_data(id, &description)
            .map_err(internal)?;
        self.data.insert(key, id);
        Ok(id)
    }

    /// The signature of a function of the Loom calling convention (ADR 0021 §3): a result
    /// pointer, the closure environment (for closures and shims), then each argument: a
    /// scalar by value, an aggregate by pointer, nothing for a zero-size value.
    pub fn loom_signature(&self, args: &[Ty], with_env: bool) -> Signature {
        let ptr = self.cx.ptr_ty;
        let mut sig = Signature::new(self.cx.call_conv);
        sig.params.push(AbiParam::new(ptr));
        if with_env {
            sig.params.push(AbiParam::new(ptr));
        }
        for &arg in args {
            let layout = self.cx.layout(arg);
            match layout.repr {
                Repr::Zero => {}
                Repr::Scalar(t) => sig.params.push(AbiParam::new(t)),
                Repr::Memory => sig.params.push(AbiParam::new(ptr)),
            }
        }
        sig
    }

    /// The concrete argument types of a MIR body (excluding the closure environment).
    pub fn body_arg_types(&self, body: &MirBody, subst: &Subst) -> Vec<Ty> {
        (1..=body.arg_count)
            .filter(|&i| body.locals[i].kind != loom_mir::LocalKind::ClosureEnv)
            .map(|i| self.cx.concrete(body.locals[i].ty, subst))
            .collect()
    }

    fn instance_signature(&self, instance: &Instance) -> Result<Signature> {
        let ptr = self.cx.ptr_ty;
        let cc = self.cx.call_conv;
        Ok(match instance {
            Instance::Body(source, key) => {
                let body = self.cx.body(*source)?;
                let subst = subst_of(key);
                let args = self.body_arg_types(body, &subst);
                self.loom_signature(&args, matches!(source, MirSource::Closure(..)))
            }
            Instance::FnShim(def, key) => {
                // Closure convention: every argument is borrowed (a pointer).
                let subst = subst_of(key);
                let n = self.shim_arity(*def, &subst);
                let refs: Vec<Ty> = (0..n)
                    .map(|_| {
                        self.cx.interner().intern(TyKind::Ref {
                            mutable: false,
                            inner: self.cx.interner().unit(),
                        })
                    })
                    .collect();
                self.loom_signature(&refs, true)
            }
            Instance::EnvDrop(..) => {
                let mut sig = Signature::new(cc);
                sig.params.push(AbiParam::new(ptr));
                sig
            }
            Instance::EnvClone(..) => {
                let mut sig = Signature::new(cc);
                sig.params.push(AbiParam::new(ptr));
                sig.returns.push(AbiParam::new(ptr));
                sig
            }
            Instance::Glue(kind, _) => glue::signature(*kind, ptr, cc),
            Instance::Entry(_) => {
                let mut sig = Signature::new(cc);
                sig.returns.push(AbiParam::new(types::I32));
                sig
            }
        })
    }

    /// The number of arguments of a function or constructor used as a value.
    pub fn shim_arity(&self, def: DefId, _subst: &Subst) -> usize {
        match &self.cx.hir.def(def).kind {
            DefKind::Fn(f) => f.params.len() + usize::from(f.receiver.is_some()),
            DefKind::Variant(_) => match self.cx.items().variants.get(&def) {
                Some(loom_check::VariantTy::Positional(t)) => t.len(),
                Some(loom_check::VariantTy::Named(f)) => f.len(),
                _ => 0,
            },
            DefKind::Struct(_) => self.cx.items().fields.get(&def).map_or(0, Vec::len),
            _ => 0,
        }
    }

    /// Compiles every queued instance.
    fn drain(&mut self) -> Result<()> {
        while let Some((instance, id)) = self.queue.pop() {
            let mut ctx = Context::new();
            ctx.func.signature = self.instance_signature(&instance)?;
            ctx.func.name = ir::UserFuncName::user(0, id.as_u32());
            func::translate(self, &instance, &mut ctx.func)?;
            if let Err(e) = self.module.define_function(id, &mut ctx) {
                let text = match &e {
                    cranelift_module::ModuleError::Compilation(
                        cranelift_codegen::CodegenError::Verifier(errors),
                    ) => format!("{instance:?}: {}\n{}", errors, ctx.func.display()),
                    other => format!("{instance:?}: {other:?}"),
                };
                return Err(Error::Internal(text));
            }
        }
        Ok(())
    }
}

/// The ISA of the host, configured for the backend.
fn host_isa(pic: bool) -> Result<OwnedTargetIsa> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").map_err(internal)?;
    flags
        .set("is_pic", if pic { "true" } else { "false" })
        .map_err(internal)?;
    flags
        .set("preserve_frame_pointers", "true")
        .map_err(internal)?;
    let builder =
        cranelift_codegen::isa::lookup(target_lexicon::Triple::host()).map_err(internal)?;
    builder
        .finish(settings::Flags::new(flags))
        .map_err(internal)
}

fn generate(program: Program<'_>, module: &mut dyn Module, main: DefId) -> Result<FuncId> {
    let ptr_ty = module.target_config().pointer_type();
    let call_conv = module.target_config().default_call_conv;
    let mut generator = Gen {
        cx: Cx {
            hir: program.hir,
            checked: program.checked,
            mir: program.mir,
            sources: program.sources,
            ptr_ty,
            call_conv,
            layouts: RefCell::new(HashMap::new()),
        },
        module,
        funcs: HashMap::new(),
        queue: Vec::new(),
        data: HashMap::new(),
        runtime: HashMap::new(),
        counter: 0,
    };
    let entry = generator.func(Instance::Entry(main))?;
    generator.drain()?;
    Ok(entry)
}

/// Compiles the program with the JIT and runs `main`; returns the exit status. Panics and
/// errors from `main` end the process (spec §14.2) after flushing output.
///
/// # Errors
///
/// If the program uses a construct the backend does not support, nothing runs and
/// [`Error::Unsupported`] is returned.
pub fn run_jit(program: Program<'_>, main: DefId) -> Result<i32> {
    let isa = host_isa(false)?;
    let mut builder =
        cranelift_jit::JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    for f in runtime::functions() {
        builder.symbol(f.name, f.addr);
    }
    let mut module = cranelift_jit::JITModule::new(builder);
    let entry = generate(program, &mut module, main)?;
    module.finalize_definitions().map_err(internal)?;
    let code = module.get_finalized_function(entry);
    Ok(loom_runtime::run_jit_entry(code))
}

/// Compiles the program to an object file for the host, with a C `main` function.
///
/// # Errors
///
/// [`Error::Unsupported`] if the program uses a construct the backend does not support.
pub fn build_object(program: Program<'_>, main: DefId, name: &str) -> Result<Vec<u8>> {
    let isa = host_isa(true)?;
    let builder =
        cranelift_object::ObjectBuilder::new(isa, name, cranelift_module::default_libcall_names())
            .map_err(internal)?;
    let mut module = cranelift_object::ObjectModule::new(builder);
    let entry = generate(program, &mut module, main)?;
    func::define_c_main(&mut module, entry)?;
    let product = module.finish();
    product.emit().map_err(internal)
}

#[cfg(test)]
mod tests {
    use super::CRATE_NAME;

    #[test]
    fn crate_name_matches_package() {
        assert_eq!(CRATE_NAME, env!("CARGO_PKG_NAME"));
    }
}
