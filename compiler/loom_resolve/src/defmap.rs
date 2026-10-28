//! Module scopes, imports, visibility and item paths.

use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{
    BindingId, DefId, DefKind, Hir, Name, Package, Path, TypeRef, TypeRefKind, Visibility,
};
use loom_span::Span;
use std::collections::HashMap;

/// The two namespaces of a module (spec §12.1 rule 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// Modules, structs, enums, traits, type aliases.
    Types,
    /// Functions, constants, struct constructors.
    Values,
}

/// A generic parameter: the item that declares it and its position among that item's
/// own parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TyParamId {
    /// The declaring item.
    pub owner: DefId,
    /// The position in the owner's own parameter list.
    pub index: u32,
}

/// What a name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Res {
    /// An item.
    Def(DefId),
    /// A generic parameter.
    TyParam(TyParamId),
    /// A local binding.
    Local(BindingId),
    /// `self` in a method.
    SelfValue,
    /// `Self`.
    SelfType,
    /// An error has been reported.
    Err,
}

#[derive(Clone, Debug)]
struct Entry {
    def: DefId,
    /// Visibility of this name in this scope (the item's, or the `use`'s).
    vis: Visibility,
}

#[derive(Clone, Debug, Default)]
struct Scope {
    types: HashMap<Name, Entry>,
    values: HashMap<Name, Entry>,
}

impl Scope {
    fn get(&self, ns: Namespace, name: &Name) -> Option<&Entry> {
        match ns {
            Namespace::Types => self.types.get(name),
            Namespace::Values => self.values.get(name),
        }
    }
}

/// Every module's namespaces, plus the prelude and lang items.
#[derive(Debug, Default)]
pub struct DefMap {
    scopes: HashMap<DefId, Scope>,
    prelude: Option<DefId>,
    lang: HashMap<String, DefId>,
}

/// Why a path did not resolve.
#[derive(Clone, Copy, Debug)]
pub enum PathError {
    /// The segment at this index was not found.
    NotFound(usize),
    /// The segment at this index is not visible.
    Private(usize, DefId),
}

const KNOWN_ATTRIBUTES: &[&str] = &[
    "derive",
    "test",
    "inline",
    "deprecated",
    "stable",
    "unstable",
    "lang",
];

impl DefMap {
    /// Builds the module scopes of `hir`, reporting duplicates, unresolved imports and
    /// invalid attributes.
    pub(crate) fn build(hir: &Hir, diagnostics: &mut Vec<Diagnostic>) -> Self {
        let mut map = Self::default();
        for (id, data) in hir.iter_defs() {
            if let Some(lang) = data.lang() {
                map.lang.insert(lang.to_owned(), id);
            }
            for attr in &data.attrs {
                let known = KNOWN_ATTRIBUTES.contains(&attr.name.as_str());
                let core_only = attr.name.as_str() == "lang" && data.package != Package::Core;
                if !known || core_only {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::UNKNOWN_ATTRIBUTE,
                            Span::new(data.span.file, attr.range),
                            format!("unknown attribute `{}`", attr.name),
                        )
                        .with_label("not an attribute of edition `preview`"),
                    );
                }
            }
            if let DefKind::Fn(f) = &data.kind
                && let Some(abi) = &f.extern_abi
            {
                if abi == "intrinsic" {
                    if data.package != Package::Core {
                        diagnostics.push(
                            Diagnostic::error(
                                codes::INVALID_INTRINSIC,
                                data.span,
                                "intrinsics can only be declared in the core library",
                            )
                            .with_label("`extern \"intrinsic\"` is reserved for `core`"),
                        );
                    }
                } else {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::NOT_IMPLEMENTED_YET,
                            data.span,
                            format!("`extern \"{abi}\"` functions are not implemented yet"),
                        )
                        .with_label("foreign functions arrive in Phase 3"),
                    );
                }
            }
            if let DefKind::Module(module) = &data.kind {
                let mut scope = Scope::default();
                for &item in &module.items {
                    let item_data = hir.def(item);
                    let namespaces: &[Namespace] = match item_data.kind {
                        DefKind::Struct(_) => &[Namespace::Types, Namespace::Values],
                        DefKind::Enum(_)
                        | DefKind::Trait(_)
                        | DefKind::TypeAlias(_)
                        | DefKind::Module(_) => &[Namespace::Types],
                        DefKind::Fn(_) | DefKind::Const(_) => &[Namespace::Values],
                        _ => &[],
                    };
                    for &ns in namespaces {
                        insert(
                            hir,
                            &mut scope,
                            ns,
                            item_data.name.clone(),
                            Entry {
                                def: item,
                                vis: item_data.vis,
                            },
                            item_data.span,
                            diagnostics,
                        );
                    }
                }
                map.scopes.insert(id, scope);
            }
        }
        if let Some(core) = hir.root(Package::Core) {
            map.prelude = map
                .scopes
                .get(&core)
                .and_then(|s| s.types.get(&Name::new("prelude")))
                .map(|e| e.def);
        }
        map.resolve_imports(hir, diagnostics);
        map
    }

    fn resolve_imports(&mut self, hir: &Hir, diagnostics: &mut Vec<Diagnostic>) {
        let mut pending: Vec<(DefId, usize)> = Vec::new();
        for (id, data) in hir.iter_defs() {
            if let DefKind::Module(module) = &data.kind {
                pending.extend((0..module.uses.len()).map(|i| (id, i)));
            }
        }
        loop {
            let mut progress = false;
            let mut still = Vec::new();
            for (module, index) in pending {
                let DefKind::Module(data) = &hir.def(module).kind else {
                    continue;
                };
                let item = &data.uses[index];
                let mut found = false;
                for ns in [Namespace::Types, Namespace::Values] {
                    if let Ok(def) = self.resolve_path(hir, module, &item.path, ns) {
                        found = true;
                        let span = Span::new(data.file, item.range);
                        let scope = self.scopes.entry(module).or_default();
                        insert(
                            hir,
                            scope,
                            ns,
                            item.alias.clone(),
                            Entry { def, vis: item.vis },
                            span,
                            diagnostics,
                        );
                    }
                }
                if found {
                    progress = true;
                } else {
                    still.push((module, index));
                }
            }
            pending = still;
            if !progress || pending.is_empty() {
                break;
            }
        }
        for (module, index) in pending {
            let DefKind::Module(data) = &hir.def(module).kind else {
                continue;
            };
            let item = &data.uses[index];
            let error = [Namespace::Types, Namespace::Values]
                .into_iter()
                .filter_map(|ns| self.resolve_path(hir, module, &item.path, ns).err())
                .max_by_key(|e| match e {
                    PathError::NotFound(i) | PathError::Private(i, _) => *i,
                });
            let diagnostic = match error {
                Some(PathError::Private(i, def)) => Diagnostic::error(
                    codes::PRIVATE_ITEM,
                    Span::new(data.file, item.path.segments[i].range),
                    format!("`{}` is private", item.path.segments[i].name),
                )
                .with_label(format!(
                    "private to module `{}`",
                    module_name(hir, hir.def(def).module)
                )),
                Some(PathError::NotFound(i)) => Diagnostic::error(
                    codes::UNRESOLVED_IMPORT,
                    Span::new(data.file, item.path.segments[i].range),
                    format!("unresolved import `{}`", item.path.text()),
                )
                .with_label(format!("no `{}` here", item.path.segments[i].name)),
                None => continue,
            };
            diagnostics.push(diagnostic);
        }
    }

    /// The item marked `@[lang("name")]` in the core library.
    #[must_use]
    pub fn lang(&self, name: &str) -> Option<DefId> {
        self.lang.get(name).copied()
    }

    /// Every lang item.
    pub fn lang_items(&self) -> impl Iterator<Item = (&str, DefId)> {
        self.lang.iter().map(|(k, &v)| (k.as_str(), v))
    }

    /// The core library's prelude module.
    #[must_use]
    pub fn prelude(&self) -> Option<DefId> {
        self.prelude
    }

    /// Looks `name` up as the first segment of a path in `module`: the module's items and
    /// imports, then the prelude, then package names (spec §12.2 rule 2, after locals and
    /// generic parameters).
    #[must_use]
    pub fn lookup_in_scope(
        &self,
        hir: &Hir,
        module: DefId,
        name: &Name,
        ns: Namespace,
    ) -> Option<DefId> {
        match (name.as_str(), ns) {
            ("package", Namespace::Types) => return hir.root(hir.package_of(module)),
            ("super", Namespace::Types) => {
                return hir.def(module).parent.map(|p| hir.def(p).module);
            }
            ("self", Namespace::Types) => return Some(module),
            _ => {}
        }
        if let Some(entry) = self.scopes.get(&module).and_then(|s| s.get(ns, name)) {
            return Some(entry.def);
        }
        if let Some(prelude) = self.prelude
            && let Some(entry) = self.scopes.get(&prelude).and_then(|s| s.get(ns, name))
        {
            return Some(entry.def);
        }
        if ns == Namespace::Types && name.as_str() == "core" {
            return hir.root(Package::Core);
        }
        None
    }

    /// The item named `name` in `module`'s namespace `ns`, with the visibility of that
    /// name there.
    #[must_use]
    pub fn member(&self, module: DefId, name: &Name, ns: Namespace) -> Option<(DefId, Visibility)> {
        self.scopes
            .get(&module)
            .and_then(|s| s.get(ns, name))
            .map(|e| (e.def, e.vis))
    }

    /// Looks up `name` inside module `module`, as seen from `from`.
    fn lookup_in_module(
        &self,
        hir: &Hir,
        module: DefId,
        name: &Name,
        ns: Namespace,
        from: DefId,
    ) -> Result<DefId, bool> {
        let entry = self
            .scopes
            .get(&module)
            .and_then(|s| s.get(ns, name))
            .ok_or(false)?;
        if is_visible(hir, entry.vis, module, from) {
            Ok(entry.def)
        } else {
            Err(true)
        }
    }

    /// Resolves a path made only of modules and items (as in `use`) in namespace `ns`
    /// for its last segment.
    ///
    /// # Errors
    ///
    /// The first segment that does not resolve or is not visible.
    pub fn resolve_path(
        &self,
        hir: &Hir,
        from: DefId,
        path: &Path,
        ns: Namespace,
    ) -> Result<DefId, PathError> {
        let count = path.segments.len();
        let first_ns = if count == 1 { ns } else { Namespace::Types };
        let mut current = self
            .lookup_in_scope(hir, from, &path.segments[0].name, first_ns)
            .ok_or(PathError::NotFound(0))?;
        for (i, segment) in path.segments.iter().enumerate().skip(1) {
            let segment_ns = if i + 1 == count { ns } else { Namespace::Types };
            current = self.resolve_member(hir, current, &segment.name, segment_ns, from, i)?;
        }
        Ok(current)
    }

    fn resolve_member(
        &self,
        hir: &Hir,
        parent: DefId,
        name: &Name,
        ns: Namespace,
        from: DefId,
        index: usize,
    ) -> Result<DefId, PathError> {
        match &hir.def(parent).kind {
            DefKind::Module(_) => match self.lookup_in_module(hir, parent, name, ns, from) {
                Ok(def) => Ok(def),
                Err(true) => {
                    let def = self
                        .scopes
                        .get(&parent)
                        .and_then(|s| s.get(ns, name))
                        .map_or(parent, |e| e.def);
                    Err(PathError::Private(index, def))
                }
                Err(false) => Err(PathError::NotFound(index)),
            },
            DefKind::Enum(data) if ns == Namespace::Values => data
                .variants
                .iter()
                .copied()
                .find(|&v| hir.def(v).name == *name)
                .ok_or(PathError::NotFound(index)),
            _ => Err(PathError::NotFound(index)),
        }
    }

    /// Resolves the start of a type path written inside `scope_item` (whose generic
    /// parameters, and those of its parent type, trait or impl, are in scope) in module
    /// `module`. Returns what the longest prefix of modules and items names, and how many
    /// segments that prefix has. The rest (associated types) is for the type checker.
    ///
    /// # Errors
    ///
    /// The first segment that does not resolve or is not visible.
    pub fn resolve_type_path(
        &self,
        hir: &Hir,
        scope_item: Option<DefId>,
        module: DefId,
        path: &Path,
    ) -> Result<(Res, usize), PathError> {
        let first = &path.segments[0].name;
        if first.as_str() == "Self" {
            return Ok((Res::SelfType, 1));
        }
        if let Some(param) = scope_item.and_then(|item| find_generic_param(hir, item, first)) {
            return Ok((Res::TyParam(param), 1));
        }
        let mut current = self
            .lookup_in_scope(hir, module, first, Namespace::Types)
            .ok_or(PathError::NotFound(0))?;
        let mut consumed = 1;
        for (i, segment) in path.segments.iter().enumerate().skip(1) {
            if !matches!(hir.def(current).kind, DefKind::Module(_)) {
                break;
            }
            current =
                self.resolve_member(hir, current, &segment.name, Namespace::Types, module, i)?;
            consumed += 1;
        }
        Ok((Res::Def(current), consumed))
    }

    /// Resolves a trait path (in a bound, an `impl` or `dyn`).
    ///
    /// # Errors
    ///
    /// The first segment that does not resolve or is not visible.
    #[must_use]
    pub fn resolve_trait_ref(
        &self,
        hir: &Hir,
        scope_item: Option<DefId>,
        module: DefId,
        ty: &TypeRef,
    ) -> Option<Result<DefId, PathError>> {
        let TypeRefKind::Path(path) = &ty.kind else {
            return None;
        };
        Some(
            match self.resolve_type_path(hir, scope_item, module, path) {
                Ok((Res::Def(def), n)) if n == path.segments.len() => Ok(def),
                Ok(_) => Err(PathError::NotFound(path.segments.len() - 1)),
                Err(e) => Err(e),
            },
        )
    }

    /// Whether the item `def`, whose own visibility is `vis`, is visible from `from`.
    #[must_use]
    pub fn def_visible(hir: &Hir, def: DefId, from: DefId) -> bool {
        let data = hir.def(def);
        is_visible(hir, data.vis, data.module, from)
    }

    /// The traits in scope in `module`: declared in it or imported into it, plus the
    /// prelude's (spec §3.4 rule 4).
    #[must_use]
    pub fn traits_in_scope(&self, hir: &Hir, module: DefId) -> Vec<DefId> {
        let mut out = Vec::new();
        for scope in [Some(module), self.prelude].into_iter().flatten() {
            if let Some(s) = self.scopes.get(&scope) {
                for entry in s.types.values() {
                    if matches!(hir.def(entry.def).kind, DefKind::Trait(_))
                        && !out.contains(&entry.def)
                    {
                        out.push(entry.def);
                    }
                }
            }
        }
        out.sort();
        out
    }
}

/// Finds a generic parameter named `name` declared by `item` or its generics parent.
#[must_use]
pub fn find_generic_param(hir: &Hir, item: DefId, name: &Name) -> Option<TyParamId> {
    let mut current = Some(item);
    while let Some(owner) = current {
        if let Some(generics) = hir.own_generics(owner)
            && let Some(index) = generics.params.iter().position(|p| p.name == *name)
        {
            return Some(TyParamId {
                owner,
                index: u32::try_from(index).unwrap_or(u32::MAX),
            });
        }
        current = hir.generics_parent(owner);
    }
    None
}

/// Visibility check (spec §3.11): `vis` declared in `module`, seen from `from`.
#[must_use]
pub fn is_visible(hir: &Hir, vis: Visibility, module: DefId, from: DefId) -> bool {
    match vis {
        Visibility::Public => true,
        Visibility::Package => hir.package_of(module) == hir.package_of(from),
        Visibility::Private => {
            let mut current = Some(from);
            while let Some(m) = current {
                if m == module {
                    return true;
                }
                current = hir.def(m).parent.map(|p| hir.def(p).module);
            }
            false
        }
    }
}

fn module_name(hir: &Hir, module: DefId) -> String {
    let path = hir.path_string(module);
    if path.is_empty() {
        "the root module".to_owned()
    } else {
        path
    }
}

fn insert(
    hir: &Hir,
    scope: &mut Scope,
    ns: Namespace,
    name: Name,
    entry: Entry,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if name.as_str().is_empty() {
        return;
    }
    let map = match ns {
        Namespace::Types => &mut scope.types,
        Namespace::Values => &mut scope.values,
    };
    if let Some(existing) = map.get(&name) {
        if existing.def == entry.def {
            return;
        }
        let previous = hir.def(existing.def);
        // A struct occupies both namespaces; report it once.
        if ns == Namespace::Values
            && matches!(previous.kind, DefKind::Struct(_))
            && matches!(hir.def(entry.def).kind, DefKind::Struct(_))
        {
            return;
        }
        diagnostics.push(
            Diagnostic::error(
                codes::DUPLICATE_DEFINITION,
                span,
                format!("`{name}` is declared twice in this module"),
            )
            .with_label("second declaration")
            .with_secondary(previous.span, "first declared here"),
        );
        return;
    }
    map.insert(name, entry);
}

/// `true` if `Res` names a type-like item.
#[must_use]
pub fn is_type_def(hir: &Hir, def: DefId) -> bool {
    matches!(
        hir.def(def).kind,
        DefKind::Struct(_) | DefKind::Enum(_) | DefKind::Trait(_) | DefKind::TypeAlias(_)
    )
}
