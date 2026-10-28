//! Item signatures: generics, predicates, fields, function signatures, traits and impls.

use crate::lower::TyLowerer;
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{DefId, DefKind, FnKind, Hir, Mode, Name, Package, VariantFields, Visibility};
use loom_resolve::{DefMap, TyParamId};
use loom_span::Span;
use loom_types::{FloatTy, IntTy, Interner, SELF_INDEX, Subst, Ty, TyKind};
use std::collections::HashMap;

/// A trait with generic arguments (excluding the self type).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TraitRef {
    /// The trait.
    pub trait_: DefId,
    /// Its generic arguments.
    pub args: Vec<Ty>,
}

/// `ty: Trait[args]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Predicate {
    /// The constrained type.
    pub ty: Ty,
    /// The required trait.
    pub trait_ref: TraitRef,
}

/// The generic parameters in scope in an item and the predicates on them.
#[derive(Clone, Debug, Default)]
pub struct GenericsInfo {
    /// Every parameter in scope: the parent's first, then the item's own.
    pub params: Vec<TyParamId>,
    /// The item's own parameters.
    pub own: Vec<TyParamId>,
    /// Predicates from bounds and `where` clauses (the parent's included).
    pub predicates: Vec<Predicate>,
}

/// A field with its type.
#[derive(Clone, Debug)]
pub struct FieldTy {
    /// The field name.
    pub name: Name,
    /// Its type, in terms of the owner's generic parameters.
    pub ty: Ty,
    /// Its visibility.
    pub vis: Visibility,
}

/// A variant's payload with types.
#[derive(Clone, Debug)]
pub enum VariantTy {
    /// No payload.
    Unit,
    /// Positional fields.
    Positional(Vec<Ty>),
    /// Named fields.
    Named(Vec<FieldTy>),
}

/// A function's signature.
#[derive(Clone, Debug)]
pub struct FnSig {
    /// The receiver's mode, for methods.
    pub receiver: Option<Mode>,
    /// Parameters: mode, name, type, whether it has a default.
    pub params: Vec<(Mode, Name, Ty, bool)>,
    /// The return type.
    pub ret: Ty,
    /// The type of `self` (methods) or `Self` (members of types, traits and impls).
    pub self_ty: Option<Ty>,
}

/// Where an implementation comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImplKind {
    /// An `impl` item.
    Item(DefId),
    /// `@[derive(…)]` on the type.
    Derived(DefId),
}

/// An implementation of a trait.
#[derive(Clone, Debug)]
pub struct ImplInfo {
    /// Its origin.
    pub kind: ImplKind,
    /// The implemented trait.
    pub trait_ref: TraitRef,
    /// The implementing type, in terms of `params`.
    pub self_ty: Ty,
    /// The impl's generic parameters.
    pub params: Vec<TyParamId>,
    /// Predicates that must hold for the impl to apply.
    pub predicates: Vec<Predicate>,
    /// Associated type definitions: trait's associated type → type.
    pub assoc_types: HashMap<DefId, Ty>,
    /// Methods, by name.
    pub methods: HashMap<Name, DefId>,
}

/// An index into [`Items::impls`].
pub type ImplId = usize;

/// Information about a trait.
#[derive(Clone, Debug, Default)]
pub struct TraitInfo {
    /// Supertraits, in terms of the trait's parameters (with `Self`).
    pub supertraits: Vec<TraitRef>,
    /// Methods (required and provided), by name.
    pub methods: HashMap<Name, DefId>,
    /// Associated types, by name.
    pub assoc_types: HashMap<Name, DefId>,
    /// Bounds on associated types.
    pub assoc_bounds: HashMap<DefId, Vec<TraitRef>>,
    /// Associated constants, by name.
    pub consts: HashMap<Name, DefId>,
}

/// The lang items and the primitive types they stand for.
#[derive(Debug, Default)]
pub struct Lang {
    /// Lang structs that stand for primitive types.
    pub prims: HashMap<DefId, TyKind>,
    /// Lang items by name.
    pub items: HashMap<String, DefId>,
}

impl Lang {
    /// The lang item `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<DefId> {
        self.items.get(name).copied()
    }
}

/// Every item's signature.
#[derive(Debug, Default)]
pub struct Items {
    /// Lang items.
    pub lang: Lang,
    /// Generics of every generic-capable item.
    pub generics: HashMap<DefId, GenericsInfo>,
    /// Struct fields.
    pub fields: HashMap<DefId, Vec<FieldTy>>,
    /// Variant payloads.
    pub variants: HashMap<DefId, VariantTy>,
    /// Function signatures.
    pub fns: HashMap<DefId, FnSig>,
    /// Constant types.
    pub consts: HashMap<DefId, Ty>,
    /// Type alias targets.
    pub aliases: HashMap<DefId, Ty>,
    /// Traits.
    pub traits: HashMap<DefId, TraitInfo>,
    /// Implementations.
    pub impls: Vec<ImplInfo>,
    /// Implementations by trait.
    pub impls_by_trait: HashMap<DefId, Vec<ImplId>>,
    /// The implementation an `impl` item became.
    pub impl_of_item: HashMap<DefId, ImplId>,
    /// Inherent methods, associated functions and constants of a type, by name.
    pub inherent: HashMap<DefId, HashMap<Name, DefId>>,
}

impl Items {
    /// The generics of `def` (empty if it has none).
    #[must_use]
    pub fn generics_of(&self, def: DefId) -> GenericsInfo {
        self.generics.get(&def).cloned().unwrap_or_default()
    }

    /// The type of a struct or enum with its own parameters as arguments.
    pub fn identity_ty(&self, interner: &Interner, def: DefId) -> Ty {
        if let Some(prim) = self.lang.prims.get(&def) {
            return interner.intern(prim.clone());
        }
        let generics = self.generics_of(def);
        let args = generics
            .own
            .iter()
            .map(|&p| interner.intern(TyKind::Param(p)))
            .collect();
        interner.intern(TyKind::Adt(def, args))
    }

    /// The substitution of a type definition's parameters by `args`.
    #[must_use]
    pub fn adt_subst(&self, def: DefId, args: &[Ty]) -> Subst {
        let mut subst = Subst::new();
        for (&param, &arg) in self.generics_of(def).own.iter().zip(args) {
            subst.insert(param, arg);
        }
        subst
    }

    /// The lang struct whose methods a primitive type has.
    #[must_use]
    pub fn prim_def(&self, kind: &TyKind) -> Option<DefId> {
        let name = match kind {
            TyKind::Int(i) => i.lang(),
            TyKind::Float(f) => f.lang(),
            TyKind::Bool => "Bool",
            TyKind::Char => "Char",
            TyKind::String => "String",
            TyKind::Bytes => "Bytes",
            TyKind::Unit => "Unit",
            TyKind::Never => "Never",
            _ => return None,
        };
        self.lang.get(name)
    }

    /// The trait's `Self` parameter.
    #[must_use]
    pub fn self_param(trait_: DefId) -> TyParamId {
        TyParamId {
            owner: trait_,
            index: SELF_INDEX,
        }
    }
}

fn prim_kind(name: &str) -> Option<TyKind> {
    Some(match name {
        "I8" => TyKind::Int(IntTy::I8),
        "I16" => TyKind::Int(IntTy::I16),
        "I32" => TyKind::Int(IntTy::I32),
        "I64" => TyKind::Int(IntTy::I64),
        "I128" => TyKind::Int(IntTy::I128),
        "ISize" => TyKind::Int(IntTy::ISize),
        "U8" => TyKind::Int(IntTy::U8),
        "U16" => TyKind::Int(IntTy::U16),
        "U32" => TyKind::Int(IntTy::U32),
        "U64" => TyKind::Int(IntTy::U64),
        "U128" => TyKind::Int(IntTy::U128),
        "USize" => TyKind::Int(IntTy::USize),
        "F32" => TyKind::Float(FloatTy::F32),
        "F64" => TyKind::Float(FloatTy::F64),
        "Bool" => TyKind::Bool,
        "Char" => TyKind::Char,
        "String" => TyKind::String,
        "Bytes" => TyKind::Bytes,
        "Unit" => TyKind::Unit,
        "Never" => TyKind::Never,
        _ => return None,
    })
}

/// Collects every signature.
#[allow(clippy::too_many_lines, reason = "one pass per item kind")]
pub fn collect(
    hir: &Hir,
    def_map: &DefMap,
    interner: &Interner,
    diagnostics: &mut Vec<Diagnostic>,
) -> Items {
    let mut items = Items::default();
    for (name, def) in def_map.lang_items() {
        items.lang.items.insert(name.to_owned(), def);
        if let Some(kind) = prim_kind(name) {
            items.lang.prims.insert(def, kind);
        }
    }
    // Generics first: everything else is lowered in terms of them.
    for (def, _) in hir.iter_defs() {
        if let Some(generics) = hir.own_generics(def) {
            let own: Vec<TyParamId> = (0..generics.params.len())
                .map(|i| TyParamId {
                    owner: def,
                    index: u32::try_from(i).unwrap_or(u32::MAX),
                })
                .collect();
            let mut params = hir
                .generics_parent(def)
                .and_then(|p| items.generics.get(&p))
                .map(|g| g.params.clone())
                .unwrap_or_default();
            params.extend(own.iter().copied());
            items.generics.insert(
                def,
                GenericsInfo {
                    params,
                    own,
                    predicates: Vec::new(),
                },
            );
        }
    }
    // Type aliases, before anything that may mention them. An alias may refer to an
    // alias declared later, so lower them until nothing changes (a few passes at most).
    for pass in 0..4 {
        let mut scratch = Vec::new();
        for (def, data) in hir.iter_defs() {
            let DefKind::TypeAlias(alias) = &data.kind else {
                continue;
            };
            if matches!(
                data.parent.map(|p| &hir.def(p).kind),
                Some(DefKind::Impl(_))
            ) {
                continue;
            }
            let lowerer = TyLowerer::new(hir, def_map, &items, interner, def);
            let sink = if pass == 3 {
                &mut *diagnostics
            } else {
                &mut scratch
            };
            let ty = lowerer.lower(&alias.ty, sink);
            items.aliases.insert(def, ty);
        }
    }
    // Traits come before predicates: `Self.Item` in a `where` clause needs them.
    collect_traits(hir, def_map, interner, &mut items, diagnostics);
    // Generics of a method are only complete after its parent's; the loop above visits
    // parents first because items are allocated parent-first.
    let mut predicates: Vec<(DefId, Vec<Predicate>)> = Vec::new();
    for (def, data) in hir.iter_defs() {
        let Some(generics) = hir.own_generics(def) else {
            continue;
        };
        let lowerer = TyLowerer::new(hir, def_map, &items, interner, def);
        let mut preds = Vec::new();
        for (i, param) in generics.params.iter().enumerate() {
            let param_ty = interner.intern(TyKind::Param(TyParamId {
                owner: def,
                index: u32::try_from(i).unwrap_or(u32::MAX),
            }));
            for bound in &param.bounds {
                if let Some(trait_ref) = lowerer.lower_trait_ref(bound, diagnostics) {
                    preds.push(Predicate {
                        ty: param_ty,
                        trait_ref,
                    });
                }
            }
        }
        for (ty, bounds) in &generics.where_preds {
            let ty = lowerer.lower(ty, diagnostics);
            for bound in bounds {
                if let Some(trait_ref) = lowerer.lower_trait_ref(bound, diagnostics) {
                    preds.push(Predicate { ty, trait_ref });
                }
            }
        }
        if let DefKind::Trait(_) = data.kind {
            // Inside a trait, `Self` implements it.
            let self_ty = interner.intern(TyKind::Param(Items::self_param(def)));
            let args = (0..generics.params.len())
                .map(|i| {
                    interner.intern(TyKind::Param(TyParamId {
                        owner: def,
                        index: u32::try_from(i).unwrap_or(u32::MAX),
                    }))
                })
                .collect();
            preds.push(Predicate {
                ty: self_ty,
                trait_ref: TraitRef { trait_: def, args },
            });
        }
        predicates.push((def, preds));
    }
    for (def, preds) in predicates {
        let parent_preds = hir
            .generics_parent(def)
            .and_then(|p| items.generics.get(&p))
            .map(|g| g.predicates.clone())
            .unwrap_or_default();
        if let Some(info) = items.generics.get_mut(&def) {
            info.predicates = parent_preds;
            info.predicates.extend(preds);
        }
    }
    // Fields, variants, aliases, inherent members.
    for (def, data) in hir.iter_defs() {
        match &data.kind {
            DefKind::Struct(s) => {
                let lowerer = TyLowerer::new(hir, def_map, &items, interner, def);
                let fields = s
                    .fields
                    .iter()
                    .map(|f| FieldTy {
                        name: f.name.clone(),
                        ty: lowerer.lower(&f.ty, diagnostics),
                        vis: f.vis,
                    })
                    .collect();
                items.fields.insert(def, fields);
                let members = s
                    .members
                    .iter()
                    .map(|&m| (hir.def(m).name.clone(), m))
                    .collect();
                items.inherent.insert(def, members);
            }
            DefKind::Enum(e) => {
                let members = e
                    .members
                    .iter()
                    .map(|&m| (hir.def(m).name.clone(), m))
                    .collect();
                items.inherent.insert(def, members);
            }
            DefKind::Variant(v) => {
                let parent = data.parent.unwrap_or(def);
                let lowerer = TyLowerer::new(hir, def_map, &items, interner, parent);
                let ty = match &v.fields {
                    VariantFields::Unit => VariantTy::Unit,
                    VariantFields::Positional(types) => VariantTy::Positional(
                        types
                            .iter()
                            .map(|t| lowerer.lower(t, diagnostics))
                            .collect(),
                    ),
                    VariantFields::Named(fields) => VariantTy::Named(
                        fields
                            .iter()
                            .map(|f| FieldTy {
                                name: f.name.clone(),
                                ty: lowerer.lower(&f.ty, diagnostics),
                                vis: Visibility::Public,
                            })
                            .collect(),
                    ),
                };
                items.variants.insert(def, ty);
            }
            _ => {}
        }
    }
    // Function signatures and constants.
    for (def, data) in hir.iter_defs() {
        match &data.kind {
            DefKind::Fn(f) => {
                let lowerer = TyLowerer::new(hir, def_map, &items, interner, def);
                let params = f
                    .params
                    .iter()
                    .map(|p| {
                        (
                            p.mode,
                            p.name.clone(),
                            lowerer.lower(&p.ty, diagnostics),
                            p.has_default,
                        )
                    })
                    .collect();
                let ret = f
                    .ret
                    .as_ref()
                    .map_or_else(|| interner.unit(), |r| lowerer.lower_return(r, diagnostics));
                let self_ty = lowerer.self_ty();
                items.fns.insert(
                    def,
                    FnSig {
                        receiver: f.receiver,
                        params,
                        ret,
                        self_ty,
                    },
                );
            }
            DefKind::Const(c) => {
                let lowerer = TyLowerer::new(hir, def_map, &items, interner, def);
                let ty = lowerer.lower(&c.ty, diagnostics);
                items.consts.insert(def, ty);
            }
            _ => {}
        }
    }
    collect_impls(hir, def_map, interner, &mut items, diagnostics);
    items
}

fn collect_traits(
    hir: &Hir,
    def_map: &DefMap,
    interner: &Interner,
    items: &mut Items,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (def, data) in hir.iter_defs() {
        let DefKind::Trait(trait_data) = &data.kind else {
            continue;
        };
        let lowerer = TyLowerer::new(hir, def_map, items, interner, def);
        let mut info = TraitInfo::default();
        for sup in &trait_data.supertraits {
            if let Some(trait_ref) = lowerer.lower_trait_ref(sup, diagnostics) {
                info.supertraits.push(trait_ref);
            }
        }
        for &member in &trait_data.members {
            let m = hir.def(member);
            match &m.kind {
                DefKind::Fn(_) => {
                    info.methods.insert(m.name.clone(), member);
                }
                DefKind::AssocType(assoc) => {
                    info.assoc_types.insert(m.name.clone(), member);
                    let bounds = assoc
                        .bounds
                        .iter()
                        .filter_map(|b| lowerer.lower_trait_ref(b, diagnostics))
                        .collect();
                    info.assoc_bounds.insert(member, bounds);
                }
                DefKind::Const(_) => {
                    info.consts.insert(m.name.clone(), member);
                }
                _ => {}
            }
        }
        items.traits.insert(def, info);
    }
}

fn collect_impls(
    hir: &Hir,
    def_map: &DefMap,
    interner: &Interner,
    items: &mut Items,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (def, data) in hir.iter_defs() {
        match &data.kind {
            DefKind::Impl(impl_data) => {
                let lowerer = TyLowerer::new(hir, def_map, items, interner, def);
                let Some(trait_ref) = lowerer.lower_trait_ref(&impl_data.trait_ref, diagnostics)
                else {
                    continue;
                };
                let self_ty = lowerer.lower(&impl_data.self_ty, diagnostics);
                let generics = items.generics_of(def);
                let mut assoc_types = HashMap::new();
                let mut methods = HashMap::new();
                let trait_info = items
                    .traits
                    .get(&trait_ref.trait_)
                    .cloned()
                    .unwrap_or_default();
                for &member in &impl_data.members {
                    let m = hir.def(member);
                    match &m.kind {
                        DefKind::Fn(_) => {
                            methods.insert(m.name.clone(), member);
                        }
                        DefKind::TypeAlias(alias) => {
                            let ty = lowerer.lower(&alias.ty, diagnostics);
                            if let Some(&assoc) = trait_info.assoc_types.get(&m.name) {
                                assoc_types.insert(assoc, ty);
                            }
                        }
                        _ => {}
                    }
                }
                check_orphan(
                    hir,
                    items,
                    def,
                    data.package,
                    &trait_ref,
                    self_ty,
                    interner,
                    diagnostics,
                );
                let id = items.impls.len();
                items.impls.push(ImplInfo {
                    kind: ImplKind::Item(def),
                    trait_ref: trait_ref.clone(),
                    self_ty,
                    params: generics.params.clone(),
                    predicates: generics.predicates.clone(),
                    assoc_types,
                    methods,
                });
                items
                    .impls_by_trait
                    .entry(trait_ref.trait_)
                    .or_default()
                    .push(id);
                items.impl_of_item.insert(def, id);
            }
            DefKind::Struct(_) | DefKind::Enum(_) => {
                for (name, range) in data.derives() {
                    let Some(trait_) = def_map.lookup_in_scope(
                        hir,
                        data.module,
                        &name,
                        loom_resolve::Namespace::Types,
                    ) else {
                        diagnostics.push(
                            Diagnostic::error(
                                codes::UNRESOLVED_NAME,
                                Span::new(data.span.file, range),
                                format!("cannot find trait `{name}` in this scope"),
                            )
                            .with_label("not found"),
                        );
                        continue;
                    };
                    let derivable = ["Copy", "Clone", "Eq", "Ord", "Hash", "Default"];
                    let lang_name = hir.def(trait_).lang().unwrap_or("");
                    if !derivable.contains(&lang_name) {
                        diagnostics.push(
                            Diagnostic::error(
                                codes::INVALID_DERIVE,
                                Span::new(data.span.file, range),
                                format!("`{name}` cannot be derived"),
                            )
                            .with_label("derivable traits are `Copy`, `Clone`, `Eq`, `Ord`, `Hash` and `Default`"),
                        );
                        continue;
                    }
                    let generics = items.generics_of(def);
                    let self_ty = items.identity_ty(interner, def);
                    let mut predicates = generics.predicates.clone();
                    for &param in &generics.own {
                        predicates.push(Predicate {
                            ty: interner.intern(TyKind::Param(param)),
                            trait_ref: TraitRef {
                                trait_,
                                args: Vec::new(),
                            },
                        });
                    }
                    let methods = items
                        .traits
                        .get(&trait_)
                        .map(|t| t.methods.clone())
                        .unwrap_or_default();
                    let id = items.impls.len();
                    items.impls.push(ImplInfo {
                        kind: ImplKind::Derived(def),
                        trait_ref: TraitRef {
                            trait_,
                            args: Vec::new(),
                        },
                        self_ty,
                        params: generics.params.clone(),
                        predicates,
                        assoc_types: HashMap::new(),
                        methods,
                    });
                    items.impls_by_trait.entry(trait_).or_default().push(id);
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments, reason = "context for one diagnostic")]
fn check_orphan(
    hir: &Hir,
    items: &Items,
    impl_def: DefId,
    package: Package,
    trait_ref: &TraitRef,
    self_ty: Ty,
    interner: &Interner,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if package == Package::Core {
        return;
    }
    let local_trait = hir.package_of(trait_ref.trait_) == package;
    let local_type = match interner.kind(self_ty) {
        TyKind::Adt(def, _) | TyKind::Dyn(def, _) => {
            hir.package_of(def) == package && !items.lang.prims.contains_key(&def)
        }
        _ => false,
    };
    if !local_trait && !local_type {
        diagnostics.push(
            Diagnostic::error(
                codes::ORPHAN_IMPL,
                hir.def(impl_def).span,
                "only traits or types defined in this package can be implemented here",
            )
            .with_label("both the trait and the type are defined elsewhere")
            .with_note(
                "an `impl` must be in the package of the trait or of the type (the orphan rule)",
            ),
        );
    }
}

/// The callable kind of a `Fn` type.
#[must_use]
pub fn fn_kind_name(kind: FnKind) -> &'static str {
    match kind {
        FnKind::Fn => "Fn",
        FnKind::FnMut => "FnMut",
        FnKind::FnOnce => "FnOnce",
    }
}
