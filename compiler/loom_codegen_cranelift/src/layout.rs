//! Memory layout of concrete types (ADR 0021 §3).

use crate::Cx;
use cranelift_codegen::ir::{Type, types};
use loom_hir::DefKind;
use loom_types::{FloatTy, IntTy, Ty, TyKind};

/// How a value of a type is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repr {
    /// Size zero: nothing is stored.
    Zero,
    /// A single machine value.
    Scalar(Type),
    /// Bytes in memory (tuples, structs, enums, callables, `dyn`).
    Memory,
}

/// The layout of a concrete type.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Size in bytes.
    pub size: u32,
    /// Alignment in bytes.
    pub align: u32,
    /// How values are held.
    pub repr: Repr,
    /// Field offsets (tuples, structs, callables `{code, env}`, `dyn` `{data, vtable}`).
    pub fields: Vec<(u32, Ty)>,
    /// For enums: per variant (in declaration order), the payload field offsets.
    pub variants: Vec<Vec<(u32, Ty)>>,
}

/// Rounds `value` up to a multiple of `align`.
#[must_use]
pub fn align_to(value: u32, align: u32) -> u32 {
    value.next_multiple_of(align.max(1))
}

impl Layout {
    fn scalar(ty: Type) -> Self {
        let size = ty.bytes();
        Self {
            size,
            align: size,
            repr: Repr::Scalar(ty),
            fields: Vec::new(),
            variants: Vec::new(),
        }
    }

    fn zero() -> Self {
        Self {
            size: 0,
            align: 1,
            repr: Repr::Zero,
            fields: Vec::new(),
            variants: Vec::new(),
        }
    }

    /// The scalar type, if the value is a scalar.
    #[must_use]
    pub fn scalar_ty(&self) -> Option<Type> {
        match self.repr {
            Repr::Scalar(t) => Some(t),
            _ => None,
        }
    }
}

/// The Cranelift integer type of an integer type.
#[must_use]
pub fn int_type(int: IntTy) -> Type {
    match int.bits() {
        8 => types::I8,
        16 => types::I16,
        32 => types::I32,
        128 => types::I128,
        _ => types::I64,
    }
}

impl Cx<'_> {
    /// Whether values of `ty` are runtime handles (pointers to runtime objects).
    pub fn is_handle(&self, ty: Ty) -> bool {
        match self.interner().kind(ty) {
            TyKind::String | TyKind::Bytes => true,
            TyKind::Adt(def, _) => {
                let lang = self.hir.def(def).lang();
                matches!(
                    lang,
                    Some(
                        "Array"
                            | "Map"
                            | "Chan"
                            | "Scope"
                            | "Task"
                            | "Mutex"
                            | "Shared"
                            | "AtomicInt"
                    )
                )
            }
            _ => false,
        }
    }

    /// The lang name of a concurrency handle type (`Chan`, `Task`, `Mutex`, `Shared`,
    /// `AtomicInt`, `Scope`), if `ty` is one.
    pub fn concurrency_handle(&self, ty: Ty) -> Option<&'static str> {
        match self.interner().kind(ty) {
            TyKind::Adt(def, _) => match self.hir.def(def).lang()? {
                "Chan" => Some("chan"),
                "Scope" => Some("scope"),
                "Task" => Some("task"),
                "Mutex" => Some("mutex"),
                "Shared" => Some("shared"),
                "AtomicInt" => Some("atomic"),
                _ => None,
            },
            _ => None,
        }
    }

    /// The layout of a concrete type.
    pub fn layout(&self, ty: Ty) -> Layout {
        if let Some(l) = self.layouts.borrow().get(&ty) {
            return l.clone();
        }
        let layout = self.compute_layout(ty);
        self.layouts.borrow_mut().insert(ty, layout.clone());
        layout
    }

    fn compute_layout(&self, ty: Ty) -> Layout {
        let ptr = self.ptr_ty;
        let interner = self.interner();
        match interner.kind(ty) {
            TyKind::Int(i) => Layout::scalar(int_type(i)),
            TyKind::Float(FloatTy::F32) => Layout::scalar(types::F32),
            TyKind::Float(FloatTy::F64) => Layout::scalar(types::F64),
            TyKind::Bool => Layout::scalar(types::I8),
            TyKind::Char => Layout::scalar(types::I32),
            TyKind::Unit | TyKind::Never | TyKind::Error => Layout::zero(),
            TyKind::Ref { .. } => Layout::scalar(ptr),
            _ if self.is_handle(ty) => Layout::scalar(ptr),
            TyKind::Tuple(elements) => self.record(&elements),
            TyKind::Callable { .. } | TyKind::Dyn(..) => {
                let word = ptr.bytes();
                Layout {
                    size: word * 2,
                    align: word,
                    repr: Repr::Memory,
                    fields: vec![(0, interner.unit()), (word, interner.unit())],
                    variants: Vec::new(),
                }
            }
            TyKind::Adt(def, args) => match &self.hir.def(def).kind {
                DefKind::Struct(_) => {
                    let subst = self.items().adt_subst(def, &args);
                    let fields: Vec<Ty> = self
                        .items()
                        .fields
                        .get(&def)
                        .map(|fs| fs.iter().map(|f| self.concrete(f.ty, &subst)).collect())
                        .unwrap_or_default();
                    let layout = self.record(&fields);
                    if layout.size == 0 {
                        Layout {
                            repr: Repr::Zero,
                            ..layout
                        }
                    } else {
                        layout
                    }
                }
                DefKind::Enum(data) => {
                    let subst = self.items().adt_subst(def, &args);
                    let mut size = 4;
                    let mut align = 4;
                    let mut variants = Vec::new();
                    for &v in &data.variants {
                        let payload: Vec<Ty> = match self.items().variants.get(&v) {
                            Some(loom_check::VariantTy::Positional(types)) => {
                                types.iter().map(|&t| self.concrete(t, &subst)).collect()
                            }
                            Some(loom_check::VariantTy::Named(fields)) => {
                                fields.iter().map(|f| self.concrete(f.ty, &subst)).collect()
                            }
                            _ => Vec::new(),
                        };
                        let mut offset = 4;
                        let mut offsets = Vec::new();
                        for field in payload {
                            let l = self.layout(field);
                            offset = align_to(offset, l.align);
                            offsets.push((offset, field));
                            offset += l.size;
                            align = align.max(l.align);
                        }
                        size = size.max(offset);
                        variants.push(offsets);
                    }
                    Layout {
                        size: align_to(size, align),
                        align,
                        repr: Repr::Memory,
                        fields: Vec::new(),
                        variants,
                    }
                }
                _ => Layout::zero(),
            },
            _ => Layout::zero(),
        }
    }

    /// The C layout of fields in order.
    pub fn record(&self, fields: &[Ty]) -> Layout {
        let mut offset = 0;
        let mut align = 1;
        let mut out = Vec::new();
        for &field in fields {
            let l = self.layout(field);
            offset = align_to(offset, l.align);
            out.push((offset, field));
            offset += l.size;
            align = align.max(l.align);
        }
        Layout {
            size: align_to(offset, align),
            align,
            repr: Repr::Memory,
            fields: out,
            variants: Vec::new(),
        }
    }
}
