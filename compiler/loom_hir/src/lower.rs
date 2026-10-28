//! Lowering from the AST to HIR. Total: never fails, never reports.

use crate::{
    Arg, AssocTypeData, Attr, AttrArg, BinaryOp, Binding, BindingId, BindingKind, Body, BodyId,
    CaseArm, ClosureParam, ConstData, DefData, DefId, DefKind, EnumData, Expr, ExprId, FieldData,
    FnData, FnKind, GenericParam, Generics, Hir, ImplData, InterpolationPart, Literal, Mode,
    ModuleData, Name, Package, ParamData, Pat, PatId, Path, PathSegment, Stmt, StructData,
    TraitData, TypeAliasData, TypeRef, TypeRefKind, UnaryOp, UseItem, VariantData, VariantFields,
    Visibility,
};
use loom_ast::{self as ast, AstNode};
use loom_span::{FileId, Span, TextRange};
use loom_syntax::{SyntaxKind, SyntaxNode};
use std::rc::Rc;

/// Lowers one source file into a new module of `package`, as a child of `parent`
/// (`None` for a package root), and returns the module's id.
pub fn lower_file(
    hir: &mut Hir,
    file: FileId,
    source: &ast::SourceFile,
    package: Package,
    parent: Option<DefId>,
    name: &str,
) -> DefId {
    let mut lowerer = ItemLowerer { hir, file, package };
    let range = source.syntax().text_range();
    let module = lowerer.alloc(DefData {
        name: Name::new(name),
        kind: DefKind::Module(ModuleData {
            items: Vec::new(),
            uses: Vec::new(),
            file,
        }),
        parent,
        module: DefId(0),
        package,
        vis: Visibility::Public,
        attrs: Vec::new(),
        doc: None,
        span: Span::new(file, TextRange::empty(range.start())),
    });
    lowerer.hir.defs[module.index()].module = module;
    if parent.is_none() {
        lowerer.hir.roots.push((package, module));
    }
    lowerer.lower_items(source.items(), module);
    module
}

struct ItemLowerer<'a> {
    hir: &'a mut Hir,
    file: FileId,
    package: Package,
}

fn ast_children<N: AstNode>(node: &SyntaxNode) -> impl Iterator<Item = N> + use<N> {
    node.children().filter_map(N::cast)
}

fn ast_child<N: AstNode>(node: &SyntaxNode) -> Option<N> {
    node.children().find_map(N::cast)
}

fn lower_vis(node: &SyntaxNode) -> Visibility {
    match ast_child::<ast::Visibility>(node) {
        None => Visibility::Private,
        Some(v) if v.is_package() => Visibility::Package,
        Some(_) => Visibility::Public,
    }
}

fn lower_attrs(node: &SyntaxNode) -> Vec<Attr> {
    let mut out = Vec::new();
    for attr in ast_children::<ast::Attribute>(node) {
        for entry in attr.entries() {
            let Some(name) = entry.name() else { continue };
            let args = entry
                .args()
                .map(|arg| {
                    let range = arg.syntax().text_range();
                    match arg.value() {
                        Some(ast::Expr::PathExpr(p)) => {
                            p.name_ref().map_or(AttrArg::Other(range), |n| {
                                AttrArg::Path(Name::new(&n.text()), range)
                            })
                        }
                        Some(ast::Expr::StringLit(s)) => {
                            let text: String = s
                                .value_parts()
                                .into_iter()
                                .filter_map(|p| match p {
                                    ast::StringValuePart::Text(t) => Some(t),
                                    ast::StringValuePart::Interpolation(_) => None,
                                })
                                .collect();
                            AttrArg::Str(text, range)
                        }
                        _ => AttrArg::Other(range),
                    }
                })
                .collect();
            out.push(Attr {
                name: Name::new(name.text()),
                args,
                range: entry.syntax().text_range(),
            });
        }
    }
    out
}

fn doc_of(node: &SyntaxNode) -> Option<String> {
    let docs: Vec<String> = node
        .children_with_tokens()
        .filter_map(loom_syntax::SyntaxElement::into_token)
        .filter(|t| t.kind() == SyntaxKind::DOC_COMMENT)
        .map(|t| {
            let text = t.text().trim_start_matches("##");
            text.strip_prefix(' ').unwrap_or(text).to_owned()
        })
        .collect();
    (!docs.is_empty()).then(|| docs.join("\n"))
}

impl ItemLowerer<'_> {
    fn alloc(&mut self, data: DefData) -> DefId {
        let id = DefId(u32::try_from(self.hir.defs.len()).unwrap_or(u32::MAX));
        self.hir.defs.push(data);
        id
    }

    fn span(&self, range: TextRange) -> Span {
        Span::new(self.file, range)
    }

    fn name_span(&self, name: Option<&ast::Name>, node: &SyntaxNode) -> (Name, Span) {
        match name {
            Some(n) => (Name::new(&n.text()), self.span(n.syntax().text_range())),
            None => (Name::new(""), self.span(node.text_range())),
        }
    }

    fn new_def(
        &mut self,
        node: &SyntaxNode,
        name: Option<&ast::Name>,
        kind: DefKind,
        parent: DefId,
        module: DefId,
    ) -> DefId {
        let (name, span) = self.name_span(name, node);
        self.alloc(DefData {
            name,
            kind,
            parent: Some(parent),
            module,
            package: self.package,
            vis: lower_vis(node),
            attrs: lower_attrs(node),
            doc: doc_of(node),
            span,
        })
    }

    fn push_module_item(&mut self, module: DefId, item: DefId) {
        if let DefKind::Module(data) = &mut self.hir.defs[module.index()].kind {
            data.items.push(item);
        }
    }

    fn lower_items(&mut self, items: impl Iterator<Item = ast::Item>, module: DefId) {
        for item in items {
            self.lower_item(&item, module);
        }
    }

    fn lower_item(&mut self, item: &ast::Item, module: DefId) {
        match item {
            ast::Item::FnDef(f) => {
                let id = self.lower_fn(f, module, module, None);
                self.push_module_item(module, id);
            }
            ast::Item::StructDef(s) => {
                let id = self.lower_struct(s, module);
                self.push_module_item(module, id);
            }
            ast::Item::EnumDef(e) => {
                let id = self.lower_enum(e, module);
                self.push_module_item(module, id);
            }
            ast::Item::TraitDef(t) => {
                let id = self.lower_trait(t, module);
                self.push_module_item(module, id);
            }
            ast::Item::ImplDef(i) => {
                let id = self.lower_impl(i, module);
                self.push_module_item(module, id);
            }
            ast::Item::ModuleDef(m) => {
                let file = self.file;
                let id = self.new_def(
                    m.syntax(),
                    m.name().as_ref(),
                    DefKind::Module(ModuleData {
                        items: Vec::new(),
                        uses: Vec::new(),
                        file,
                    }),
                    module,
                    module,
                );
                self.hir.defs[id.index()].module = id;
                self.push_module_item(module, id);
                self.lower_items(m.items(), id);
            }
            ast::Item::UseDecl(u) => {
                let vis = lower_vis(u.syntax());
                let mut leaves = Vec::new();
                if let Some(tree) = u.tree() {
                    collect_use_leaves(&tree, &[], vis, &mut leaves);
                }
                if let DefKind::Module(data) = &mut self.hir.defs[module.index()].kind {
                    data.uses.extend(leaves);
                }
            }
            ast::Item::ConstDef(c) => {
                let id = self.lower_const(c, module, module);
                self.push_module_item(module, id);
            }
            ast::Item::TypeAlias(t) => {
                let id = self.lower_type_alias(t, module, module);
                self.push_module_item(module, id);
            }
            ast::Item::ExternBlock(e) => {
                let abi = ast_child::<ast::StringLit>(e.syntax())
                    .map(|s| {
                        s.value_parts()
                            .into_iter()
                            .filter_map(|p| match p {
                                ast::StringValuePart::Text(t) => Some(t),
                                ast::StringValuePart::Interpolation(_) => None,
                            })
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                for f in e.functions() {
                    let id = self.lower_fn(&f, module, module, Some(abi.clone()));
                    self.push_module_item(module, id);
                }
            }
        }
    }

    fn lower_generics(node: &SyntaxNode) -> Generics {
        let mut generics = Generics::default();
        if let Some(list) = ast_child::<ast::GenericParamList>(node) {
            for param in list.params() {
                let Some(name) = param.name() else { continue };
                generics.params.push(GenericParam {
                    name: Name::new(&name.text()),
                    bounds: param
                        .bounds()
                        .map(|b| b.bounds().map(|p| lower_path_type(&p)).collect())
                        .unwrap_or_default(),
                    range: name.syntax().text_range(),
                });
            }
        }
        if let Some(clause) = ast_child::<ast::WhereClause>(node) {
            for pred in clause.predicates() {
                let ty = lower_opt_type(pred.ty(), pred.syntax().text_range());
                let bounds = pred
                    .bounds()
                    .map(|b| b.bounds().map(|p| lower_path_type(&p)).collect())
                    .unwrap_or_default();
                generics.where_preds.push((ty, bounds));
            }
        }
        generics
    }

    fn lower_fn(
        &mut self,
        f: &ast::FnDef,
        parent: DefId,
        module: DefId,
        extern_abi: Option<String>,
    ) -> DefId {
        let in_type = parent != module;
        let receiver = if in_type && !f.is_associated() {
            Some(match f.receiver_mode().map(|t| t.kind()) {
                Some(SyntaxKind::MUT_KW) => Mode::Mut,
                Some(SyntaxKind::OWN_KW) => Mode::Own,
                _ => Mode::Borrow,
            })
        } else {
            None
        };
        let params: Vec<ast::Param> = f
            .param_list()
            .map(|l| l.params().collect())
            .unwrap_or_default();
        let param_data = params
            .iter()
            .map(|p| ParamData {
                mode: param_mode(p.mode().map(|t| t.kind())),
                name: p
                    .name()
                    .map_or_else(|| Name::new("_"), |n| Name::new(&n.text())),
                ty: lower_opt_type(p.ty(), p.syntax().text_range()),
                has_default: p.default_value().is_some(),
                range: p
                    .name()
                    .map_or(p.syntax().text_range(), |n| n.syntax().text_range()),
            })
            .collect::<Vec<_>>();
        let id = self.new_def(
            f.syntax(),
            f.name().as_ref(),
            DefKind::Fn(FnData {
                generics: Self::lower_generics(f.syntax()),
                receiver,
                params: param_data,
                ret: f.ret_type().map(|t| lower_type(&t)),
                body: None,
                is_default: f.is_default(),
                extern_abi,
            }),
            parent,
            module,
        );
        let has_body = f.expr_body().is_some() || f.block_body().is_some();
        if has_body || params.iter().any(|p| p.default_value().is_some()) {
            let mut lowerer = BodyLowerer::new(id, self.file);
            for p in &params {
                let mode = param_mode(p.mode().map(|t| t.kind()));
                let name = p
                    .name()
                    .map_or_else(|| Name::new("_"), |n| Name::new(&n.text()));
                let range = p
                    .name()
                    .map_or(p.syntax().text_range(), |n| n.syntax().text_range());
                let binding = lowerer.alloc_binding(name, BindingKind::Param(mode), range);
                lowerer.body.params.push(binding);
            }
            for p in &params {
                let default = p.default_value().map(|e| lowerer.lower_expr(&e));
                lowerer.body.defaults.push(default);
            }
            lowerer.body.root = if let Some(e) = f.expr_body() {
                Some(lowerer.lower_expr(&e))
            } else {
                f.block_body().map(|b| lowerer.lower_block(&b))
            };
            let body = self.push_body(lowerer.body);
            if let DefKind::Fn(data) = &mut self.hir.defs[id.index()].kind {
                data.body = Some(body);
            }
        }
        id
    }

    fn push_body(&mut self, body: Body) -> BodyId {
        let id = BodyId(u32::try_from(self.hir.bodies.len()).unwrap_or(u32::MAX));
        self.hir.bodies.push(body);
        id
    }

    fn lower_const(&mut self, c: &ast::ConstDef, parent: DefId, module: DefId) -> DefId {
        let id = self.new_def(
            c.syntax(),
            c.name().as_ref(),
            DefKind::Const(ConstData {
                ty: lower_opt_type(c.ty(), c.syntax().text_range()),
                body: None,
            }),
            parent,
            module,
        );
        if let Some(value) = c.value() {
            let mut lowerer = BodyLowerer::new(id, self.file);
            lowerer.body.root = Some(lowerer.lower_expr(&value));
            let body = self.push_body(lowerer.body);
            if let DefKind::Const(data) = &mut self.hir.defs[id.index()].kind {
                data.body = Some(body);
            }
        }
        id
    }

    fn lower_type_alias(&mut self, t: &ast::TypeAlias, parent: DefId, module: DefId) -> DefId {
        self.new_def(
            t.syntax(),
            t.name().as_ref(),
            DefKind::TypeAlias(TypeAliasData {
                generics: Self::lower_generics(t.syntax()),
                ty: lower_opt_type(t.ty(), t.syntax().text_range()),
            }),
            parent,
            module,
        )
    }

    /// Lowers the methods, constants and type aliases of a type, trait or impl body.
    fn lower_members(&mut self, node: &SyntaxNode, parent: DefId, module: DefId) -> Vec<DefId> {
        let mut members = Vec::new();
        for child in node.children() {
            if let Some(f) = ast::FnDef::cast(child.clone()) {
                members.push(self.lower_fn(&f, parent, module, None));
            } else if let Some(c) = ast::ConstDef::cast(child.clone()) {
                members.push(self.lower_const(&c, parent, module));
            } else if let Some(t) = ast::TypeAlias::cast(child.clone()) {
                members.push(self.lower_type_alias(&t, parent, module));
            } else if let Some(a) = ast::AssocType::cast(child) {
                let bounds = ast_child::<ast::BoundList>(a.syntax())
                    .map(|b| b.bounds().map(|p| lower_path_type(&p)).collect())
                    .unwrap_or_default();
                members.push(self.new_def(
                    a.syntax(),
                    a.name().as_ref(),
                    DefKind::AssocType(AssocTypeData { bounds }),
                    parent,
                    module,
                ));
            }
        }
        members
    }

    fn lower_struct(&mut self, s: &ast::StructDef, module: DefId) -> DefId {
        let fields = s.fields().map(|f| lower_field(&f)).collect();
        let id = self.new_def(
            s.syntax(),
            s.name().as_ref(),
            DefKind::Struct(StructData {
                generics: Self::lower_generics(s.syntax()),
                fields,
                members: Vec::new(),
            }),
            module,
            module,
        );
        let members = self.lower_members(s.syntax(), id, module);
        if let DefKind::Struct(data) = &mut self.hir.defs[id.index()].kind {
            data.members = members;
        }
        id
    }

    fn lower_enum(&mut self, e: &ast::EnumDef, module: DefId) -> DefId {
        let id = self.new_def(
            e.syntax(),
            e.name().as_ref(),
            DefKind::Enum(EnumData {
                generics: Self::lower_generics(e.syntax()),
                variants: Vec::new(),
                members: Vec::new(),
            }),
            module,
            module,
        );
        let mut variants = Vec::new();
        for (index, v) in e.variants().enumerate() {
            let named: Vec<FieldData> = v.fields().map(|f| lower_field(&f)).collect();
            let fields = if !named.is_empty() {
                VariantFields::Named(named)
            } else if v
                .syntax()
                .children()
                .any(|c| c.kind() == SyntaxKind::VARIANT_FIELD_LIST)
            {
                VariantFields::Positional(v.types().map(|t| lower_type(&t)).collect())
            } else {
                VariantFields::Unit
            };
            let variant = self.new_def(
                v.syntax(),
                v.name().as_ref(),
                DefKind::Variant(VariantData {
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                    fields,
                }),
                id,
                module,
            );
            self.hir.defs[variant.index()].vis = Visibility::Public;
            variants.push(variant);
        }
        let members = self.lower_members(e.syntax(), id, module);
        if let DefKind::Enum(data) = &mut self.hir.defs[id.index()].kind {
            data.variants = variants;
            data.members = members;
        }
        id
    }

    fn lower_trait(&mut self, t: &ast::TraitDef, module: DefId) -> DefId {
        let id = self.new_def(
            t.syntax(),
            t.name().as_ref(),
            DefKind::Trait(TraitData {
                generics: Self::lower_generics(t.syntax()),
                supertraits: t
                    .supertraits()
                    .map(|b| b.bounds().map(|p| lower_path_type(&p)).collect())
                    .unwrap_or_default(),
                members: Vec::new(),
            }),
            module,
            module,
        );
        let members = self.lower_members(t.syntax(), id, module);
        for &member in &members {
            // Trait members are as visible as the trait.
            let vis = self.hir.defs[id.index()].vis;
            self.hir.defs[member.index()].vis = vis;
        }
        if let DefKind::Trait(data) = &mut self.hir.defs[id.index()].kind {
            data.members = members;
        }
        id
    }

    fn lower_impl(&mut self, i: &ast::ImplDef, module: DefId) -> DefId {
        let range = i.syntax().text_range();
        let id = self.new_def(
            i.syntax(),
            None,
            DefKind::Impl(ImplData {
                generics: Self::lower_generics(i.syntax()),
                trait_ref: i.trait_().map_or(
                    TypeRef {
                        kind: TypeRefKind::Error,
                        range,
                    },
                    |p| lower_path_type(&p),
                ),
                self_ty: lower_opt_type(i.self_type(), range),
                members: Vec::new(),
            }),
            module,
            module,
        );
        if let Some(kw) = i
            .syntax()
            .children_with_tokens()
            .filter_map(loom_syntax::SyntaxElement::into_token)
            .find(|t| t.kind() == SyntaxKind::IMPL_KW)
        {
            self.hir.defs[id.index()].span = self.span(kw.text_range());
        }
        let members = self.lower_members(i.syntax(), id, module);
        for &member in &members {
            self.hir.defs[member.index()].vis = Visibility::Public;
        }
        if let DefKind::Impl(data) = &mut self.hir.defs[id.index()].kind {
            data.members = members;
        }
        id
    }
}

fn param_mode(kind: Option<SyntaxKind>) -> Mode {
    match kind {
        Some(SyntaxKind::MUT_KW) => Mode::Mut,
        Some(SyntaxKind::OWN_KW) => Mode::Own,
        _ => Mode::Borrow,
    }
}

fn lower_field(f: &ast::FieldDef) -> FieldData {
    FieldData {
        name: f
            .name()
            .map_or_else(|| Name::new("_"), |n| Name::new(&n.text())),
        ty: lower_opt_type(f.ty(), f.syntax().text_range()),
        vis: lower_vis(f.syntax()),
        range: f
            .name()
            .map_or(f.syntax().text_range(), |n| n.syntax().text_range()),
    }
}

fn collect_use_leaves(
    tree: &ast::UseTree,
    prefix: &[PathSegment],
    vis: Visibility,
    out: &mut Vec<UseItem>,
) {
    let mut segments = prefix.to_vec();
    if let Some(path) = tree.path() {
        segments.extend(lower_path(&path).segments);
    }
    let subtrees: Vec<ast::UseTree> = tree.subtrees().collect();
    if subtrees.is_empty() {
        let Some(last) = segments.last() else { return };
        let alias = tree
            .alias()
            .map_or_else(|| last.name.clone(), |a| Name::new(&a.text()));
        let start = segments.first().map_or(last.range, |s| s.range);
        out.push(UseItem {
            path: Path {
                range: start.cover(last.range),
                segments,
            },
            alias,
            vis,
            range: tree.syntax().text_range(),
        });
    } else {
        for sub in subtrees {
            collect_use_leaves(&sub, &segments, vis, out);
        }
    }
}

fn lower_path(path: &ast::Path) -> Path {
    Path {
        segments: path
            .segments()
            .map(|s| PathSegment {
                name: Name::new(&s.name().map(|n| n.text()).unwrap_or_default()),
                args: Vec::new(),
                range: s.syntax().text_range(),
            })
            .collect(),
        range: path.syntax().text_range(),
    }
}

fn lower_path_type(p: &ast::PathType) -> TypeRef {
    let range = p.syntax().text_range();
    let Some(path) = p.path() else {
        return TypeRef {
            kind: TypeRefKind::Error,
            range,
        };
    };
    let mut path = lower_path(&path);
    let args: Vec<TypeRef> = p.generic_args().map(|t| lower_type(&t)).collect();
    if let Some(last) = path.segments.last_mut() {
        last.args = args;
    }
    path.range = range;
    if path.segments.len() == 1
        && path.segments[0].name.as_str() == "Self"
        && path.segments[0].args.is_empty()
    {
        return TypeRef {
            kind: TypeRefKind::SelfType,
            range,
        };
    }
    TypeRef {
        kind: TypeRefKind::Path(path),
        range,
    }
}

fn lower_opt_type(t: Option<ast::Type>, fallback: TextRange) -> TypeRef {
    t.map_or(
        TypeRef {
            kind: TypeRefKind::Error,
            range: fallback,
        },
        |t| lower_type(&t),
    )
}

/// Lowers a written type.
fn lower_type(t: &ast::Type) -> TypeRef {
    let range = t.syntax().text_range();
    let kind = match t {
        ast::Type::PathType(p) => return lower_path_type(p),
        ast::Type::TupleType(tuple) => {
            TypeRefKind::Tuple(tuple.elements().map(|e| lower_type(&e)).collect())
        }
        ast::Type::FnType(f) => {
            let kind = match f.kind_token().map(|t| t.text().to_owned()).as_deref() {
                Some("FnMut") => FnKind::FnMut,
                Some("FnOnce") => FnKind::FnOnce,
                _ => FnKind::Fn,
            };
            TypeRefKind::Fn {
                kind,
                params: {
                    let mut types: Vec<TypeRef> = f.types().map(|t| lower_type(&t)).collect();
                    if f.ret_type().is_some() {
                        types.pop();
                    }
                    types
                },
                ret: f.ret_type().map(|t| Box::new(lower_type(&t))),
            }
        }
        ast::Type::DynType(d) => TypeRefKind::Dyn(
            d.bounds()
                .map(|b| b.bounds().map(|p| lower_path_type(&p)).collect())
                .unwrap_or_default(),
        ),
        ast::Type::RefType(r) => TypeRefKind::Ref {
            mutable: r.is_mut(),
            inner: Box::new(lower_opt_type(r.ty(), range)),
        },
    };
    TypeRef { kind, range }
}

/// Reads an expression as a type, for `Name[Args]` in expression position (spec §7.2).
fn expr_as_type(e: &ast::Expr) -> Option<TypeRef> {
    let range = e.syntax().text_range();
    let kind = match e {
        ast::Expr::PathExpr(p) => {
            let name = p.name_ref()?.text();
            if name == "Self" {
                TypeRefKind::SelfType
            } else {
                TypeRefKind::Path(Path {
                    segments: vec![PathSegment {
                        name: Name::new(&name),
                        args: Vec::new(),
                        range,
                    }],
                    range,
                })
            }
        }
        ast::Expr::FieldExpr(f) => {
            let base = expr_as_type(&f.receiver()?)?;
            let TypeRefKind::Path(mut path) = base.kind else {
                return None;
            };
            let name = f.name_ref()?;
            path.segments.push(PathSegment {
                name: Name::new(&name.text()),
                args: Vec::new(),
                range: name.syntax().text_range(),
            });
            path.range = range;
            TypeRefKind::Path(path)
        }
        ast::Expr::IndexExpr(i) => {
            let base = expr_as_type(&i.base()?)?;
            let TypeRefKind::Path(mut path) = base.kind else {
                return None;
            };
            let args = i
                .indices()
                .map(|e| expr_as_type(&e))
                .collect::<Option<Vec<_>>>()?;
            path.segments.last_mut()?.args = args;
            path.range = range;
            TypeRefKind::Path(path)
        }
        ast::Expr::TupleExpr(t) => TypeRefKind::Tuple(
            t.elements()
                .map(|e| expr_as_type(&e))
                .collect::<Option<Vec<_>>>()?,
        ),
        _ => return None,
    };
    Some(TypeRef { kind, range })
}

struct BodyLowerer {
    body: Body,
}

impl BodyLowerer {
    fn new(owner: DefId, file: FileId) -> Self {
        Self {
            body: Body {
                owner,
                file,
                exprs: Vec::new(),
                expr_ranges: Vec::new(),
                pats: Vec::new(),
                pat_ranges: Vec::new(),
                bindings: Vec::new(),
                params: Vec::new(),
                defaults: Vec::new(),
                root: None,
            },
        }
    }

    fn alloc_expr(&mut self, expr: Expr, range: TextRange) -> ExprId {
        let id = ExprId(u32::try_from(self.body.exprs.len()).unwrap_or(u32::MAX));
        self.body.exprs.push(expr);
        self.body.expr_ranges.push(range);
        id
    }

    fn alloc_pat(&mut self, pat: Pat, range: TextRange) -> PatId {
        let id = PatId(u32::try_from(self.body.pats.len()).unwrap_or(u32::MAX));
        self.body.pats.push(pat);
        self.body.pat_ranges.push(range);
        id
    }

    fn alloc_binding(&mut self, name: Name, kind: BindingKind, range: TextRange) -> BindingId {
        let id = BindingId(u32::try_from(self.body.bindings.len()).unwrap_or(u32::MAX));
        self.body.bindings.push(Binding { name, kind, range });
        id
    }

    fn missing(&mut self, range: TextRange) -> ExprId {
        self.alloc_expr(Expr::Missing, range)
    }

    fn lower_opt(&mut self, e: Option<ast::Expr>, fallback: TextRange) -> ExprId {
        match e {
            Some(e) => self.lower_expr(&e),
            None => self.missing(fallback),
        }
    }

    fn lower_block(&mut self, block: &ast::Block) -> ExprId {
        let mut stmts = Vec::new();
        for stmt in block.statements() {
            match stmt {
                ast::Stmt::BindStmt(b) => {
                    let range = b.syntax().text_range();
                    let value = self.lower_opt(b.value(), range);
                    let (name, name_range) = b.name().map_or((Name::new("_"), range), |n| {
                        (Name::new(&n.text()), n.syntax().text_range())
                    });
                    let kind = if b.is_var() {
                        BindingKind::Var
                    } else {
                        BindingKind::Let
                    };
                    let binding = self.alloc_binding(name, kind, name_range);
                    stmts.push(Stmt::Bind {
                        binding,
                        ty: b.ty().map(|t| lower_type(&t)),
                        value,
                    });
                }
                ast::Stmt::AssignStmt(a) => stmts.push(self.lower_assign(&a)),
                ast::Stmt::ExprStmt(e) => {
                    let range = e.syntax().text_range();
                    let expr = self.lower_opt(e.expr(), range);
                    stmts.push(Stmt::Expr(expr));
                }
            }
        }
        self.alloc_expr(Expr::Block { stmts }, block.syntax().text_range())
    }

    fn lower_opt_block(&mut self, block: Option<ast::Block>, fallback: TextRange) -> ExprId {
        match block {
            Some(b) => self.lower_block(&b),
            None => self.alloc_expr(Expr::Block { stmts: Vec::new() }, fallback),
        }
    }

    fn lower_assign(&mut self, a: &ast::AssignStmt) -> Stmt {
        let range = a.syntax().text_range();
        let place = self.lower_opt(a.place(), range);
        let value = self.lower_opt(a.value(), range);
        let operator = a.operator();
        let op = operator.as_ref().and_then(|t| match t.kind() {
            SyntaxKind::PLUS_EQ => Some(BinaryOp::Add),
            SyntaxKind::MINUS_EQ => Some(BinaryOp::Sub),
            SyntaxKind::STAR_EQ => Some(BinaryOp::Mul),
            SyntaxKind::SLASH_EQ => Some(BinaryOp::Div),
            SyntaxKind::PERCENT_EQ => Some(BinaryOp::Rem),
            SyntaxKind::AMP_EQ => Some(BinaryOp::BitAnd),
            SyntaxKind::PIPE_EQ => Some(BinaryOp::BitOr),
            SyntaxKind::CARET_EQ => Some(BinaryOp::BitXor),
            SyntaxKind::SHL_EQ => Some(BinaryOp::Shl),
            SyntaxKind::SHR_EQ => Some(BinaryOp::Shr),
            _ => None,
        });
        Stmt::Assign {
            place,
            op,
            value,
            op_range: operator.map_or(range, |t| t.text_range()),
        }
    }

    fn lower_args(
        &mut self,
        list: Option<ast::ArgList>,
        trailing: Option<ast::TrailingBlock>,
    ) -> Vec<Arg> {
        let mut args = Vec::new();
        if let Some(list) = list {
            for arg in list.args() {
                match arg {
                    ast::Argument::Arg(a) => {
                        let range = a.syntax().text_range();
                        let value = self.lower_opt(a.value(), range);
                        args.push(Arg {
                            name: None,
                            value,
                            trailing: false,
                        });
                    }
                    ast::Argument::NamedArg(n) => {
                        let range = n.syntax().text_range();
                        let (name, name_range) =
                            n.name_ref().map_or((Name::new("_"), range), |r| {
                                (Name::new(&r.text()), r.syntax().text_range())
                            });
                        let value = match n.value() {
                            Some(v) => self.lower_expr(&v),
                            None => self.alloc_expr(Expr::Name(name.clone()), name_range),
                        };
                        args.push(Arg {
                            name: Some((name, name_range)),
                            value,
                            trailing: false,
                        });
                    }
                }
            }
        }
        if let Some(block) = trailing {
            let value =
                self.lower_closure(block.params(), block.body(), block.syntax().text_range());
            args.push(Arg {
                name: None,
                value,
                trailing: true,
            });
        }
        args
    }

    fn lower_closure(
        &mut self,
        params: Option<ast::BlockParamList>,
        body: Option<ast::Block>,
        range: TextRange,
    ) -> ExprId {
        let params = params.map(|list| {
            list.params()
                .map(|p| {
                    let mode = param_mode(
                        p.syntax()
                            .children_with_tokens()
                            .filter_map(loom_syntax::SyntaxElement::into_token)
                            .find(|t| matches!(t.kind(), SyntaxKind::MUT_KW | SyntaxKind::OWN_KW))
                            .map(|t| t.kind()),
                    );
                    let pat = match p.pattern() {
                        Some(pattern) => {
                            self.lower_pat_with(&pattern, BindingKind::ClosureParam(mode))
                        }
                        None => self.alloc_pat(Pat::Missing, p.syntax().text_range()),
                    };
                    ClosureParam {
                        mode,
                        pat,
                        ty: p.ty().map(|t| lower_type(&t)),
                    }
                })
                .collect()
        });
        let body = self.lower_opt_block(body, range);
        self.alloc_expr(Expr::Closure { params, body }, range)
    }

    #[allow(clippy::too_many_lines, reason = "one arm per expression kind")]
    fn lower_expr(&mut self, node: &ast::Expr) -> ExprId {
        let range = node.syntax().text_range();
        let expr = match node {
            ast::Expr::Literal(l) => match l.value() {
                Some(ast::LiteralValue::Int(v)) => Expr::Literal(Literal::Int(v)),
                Some(ast::LiteralValue::Float(v)) => Expr::Literal(Literal::Float(v)),
                Some(ast::LiteralValue::Char(c)) => Expr::Literal(Literal::Char(c)),
                Some(ast::LiteralValue::String(s)) => {
                    Expr::Literal(Literal::String(Rc::from(s.as_str())))
                }
                Some(ast::LiteralValue::Bytes(b)) => {
                    Expr::Literal(Literal::Bytes(Rc::from(b.as_slice())))
                }
                Some(ast::LiteralValue::Bool(b)) => Expr::Literal(Literal::Bool(b)),
                None => Expr::Missing,
            },
            ast::Expr::StringLit(s) => {
                let parts = s.value_parts();
                if parts
                    .iter()
                    .all(|p| matches!(p, ast::StringValuePart::Text(_)))
                {
                    let text: String = parts
                        .into_iter()
                        .map(|p| match p {
                            ast::StringValuePart::Text(t) => t,
                            ast::StringValuePart::Interpolation(_) => String::new(),
                        })
                        .collect();
                    Expr::Literal(Literal::String(Rc::from(text.as_str())))
                } else {
                    let mut out = Vec::new();
                    for part in parts {
                        match part {
                            ast::StringValuePart::Text(t) => {
                                out.push(InterpolationPart::Text(Rc::from(t.as_str())));
                            }
                            ast::StringValuePart::Interpolation(i) => {
                                let r = i.syntax().text_range();
                                let id = self.lower_opt(i.expr(), r);
                                out.push(InterpolationPart::Expr(id));
                            }
                        }
                    }
                    Expr::Interpolate(out)
                }
            }
            ast::Expr::PathExpr(p) => match p.name_ref().map(|n| n.text()).as_deref() {
                Some("self") => Expr::SelfValue,
                Some("Self") => Expr::SelfType,
                Some(name) => Expr::Name(Name::new(name)),
                None => Expr::Missing,
            },
            ast::Expr::FieldRefExpr(f) => {
                let name = f.field_name();
                let self_id = self.alloc_expr(Expr::SelfValue, range);
                Expr::Field {
                    base: self_id,
                    name: Name::new(&name),
                    name_range: range,
                }
            }
            ast::Expr::ParenExpr(p) => return self.lower_opt(p.expr(), range),
            ast::Expr::TupleExpr(t) => {
                let elements = t.elements().map(|e| self.lower_expr(&e)).collect();
                Expr::Tuple(elements)
            }
            ast::Expr::ArrayExpr(a) => {
                let elements = a.elements().map(|e| self.lower_expr(&e)).collect();
                Expr::Array(elements)
            }
            ast::Expr::MapExpr(m) => {
                let entries = m
                    .entries()
                    .map(|entry| {
                        let r = entry.syntax().text_range();
                        let k = self.lower_opt(entry.key(), r);
                        let v = self.lower_opt(entry.value(), r);
                        (k, v)
                    })
                    .collect();
                Expr::Map(entries)
            }
            ast::Expr::ClosureExpr(c) => return self.lower_closure(c.params(), c.body(), range),
            ast::Expr::IfExpr(i) => return self.lower_if(i),
            ast::Expr::CaseExpr(c) => {
                let scrutinee = self.lower_opt(c.scrutinee(), range);
                let arms = c
                    .arms()
                    .map(|arm| {
                        let r = arm.syntax().text_range();
                        let pat = match arm.pattern() {
                            Some(p) => self.lower_pat(&p),
                            None => self.alloc_pat(Pat::Missing, r),
                        };
                        let guard = arm.guard().map(|g| self.lower_expr(&g));
                        let body = self.lower_opt_block(arm.body(), r);
                        CaseArm { pat, guard, body }
                    })
                    .collect();
                let else_branch = c.else_branch().map(|e| {
                    let r = e.syntax().text_range();
                    self.lower_opt_block(e.body(), r)
                });
                Expr::Case {
                    scrutinee,
                    arms,
                    else_branch,
                }
            }
            ast::Expr::LoopExpr(l) => Expr::Loop(self.lower_opt_block(l.body(), range)),
            ast::Expr::WhileExpr(w) => {
                let cond = self.lower_opt(w.condition(), range);
                let body = self.lower_opt_block(w.body(), range);
                Expr::While { cond, body }
            }
            ast::Expr::ForExpr(f) => {
                let pat = match f.pattern() {
                    Some(p) => self.lower_pat(&p),
                    None => self.alloc_pat(Pat::Missing, range),
                };
                let iter = self.lower_opt(f.iterable(), range);
                let body = self.lower_opt_block(f.body(), range);
                Expr::For { pat, iter, body }
            }
            ast::Expr::SelectExpr(_) => Expr::Unsupported("`select` (Phase 4)"),
            ast::Expr::UnsafeExpr(_) => Expr::Unsupported("`unsafe` blocks (Phase 3)"),
            ast::Expr::ReturnExpr(r) => Expr::Return(r.value().map(|v| self.lower_expr(&v))),
            ast::Expr::BreakExpr(b) => Expr::Break(b.value().map(|v| self.lower_expr(&v))),
            ast::Expr::NextExpr(_) => Expr::Next,
            ast::Expr::BinaryExpr(b) => {
                let lhs = self.lower_opt(b.lhs(), range);
                let rhs = self.lower_opt(b.rhs(), range);
                let operator = b.operator();
                let op = match operator.as_ref().map(loom_syntax::SyntaxToken::kind) {
                    Some(SyntaxKind::PLUS) => BinaryOp::Add,
                    Some(SyntaxKind::MINUS) => BinaryOp::Sub,
                    Some(SyntaxKind::STAR) => BinaryOp::Mul,
                    Some(SyntaxKind::SLASH) => BinaryOp::Div,
                    Some(SyntaxKind::PERCENT) => BinaryOp::Rem,
                    Some(SyntaxKind::AMP) => BinaryOp::BitAnd,
                    Some(SyntaxKind::PIPE) => BinaryOp::BitOr,
                    Some(SyntaxKind::CARET) => BinaryOp::BitXor,
                    Some(SyntaxKind::SHL) => BinaryOp::Shl,
                    Some(SyntaxKind::SHR) => BinaryOp::Shr,
                    Some(SyntaxKind::EQ2) => BinaryOp::Eq,
                    Some(SyntaxKind::NEQ) => BinaryOp::Ne,
                    Some(SyntaxKind::LT) => BinaryOp::Lt,
                    Some(SyntaxKind::LE) => BinaryOp::Le,
                    Some(SyntaxKind::GT) => BinaryOp::Gt,
                    Some(SyntaxKind::GE) => BinaryOp::Ge,
                    Some(SyntaxKind::AND_KW | SyntaxKind::AMP2) => BinaryOp::And,
                    Some(SyntaxKind::OR_KW | SyntaxKind::PIPE2) => BinaryOp::Or,
                    _ => return self.missing(range),
                };
                Expr::Binary {
                    op,
                    lhs,
                    rhs,
                    op_range: operator.map_or(range, |t| t.text_range()),
                }
            }
            ast::Expr::PrefixExpr(p) => {
                let operand = self.lower_opt(p.operand(), range);
                let op = match p.operator().map(|t| t.kind()) {
                    Some(SyntaxKind::MINUS) => UnaryOp::Neg,
                    Some(SyntaxKind::TILDE) => UnaryOp::BitNot,
                    _ => UnaryOp::Not,
                };
                Expr::Unary { op, operand }
            }
            ast::Expr::RangeExpr(r) => {
                let start = self.lower_opt(r.start(), range);
                let end = self.lower_opt(r.end(), range);
                Expr::Range {
                    start,
                    end,
                    inclusive: r.is_inclusive(),
                }
            }
            ast::Expr::InExpr(i) => {
                let expr = self.lower_opt(i.expr(), range);
                let pat = match i.pattern() {
                    Some(p) => self.lower_pat(&p),
                    None => self.alloc_pat(Pat::Missing, range),
                };
                Expr::In { expr, pat }
            }
            ast::Expr::CallExpr(c) => {
                let callee = self.lower_opt(c.callee(), range);
                let args = self.lower_args(c.arg_list(), c.trailing_block());
                Expr::Call { callee, args }
            }
            ast::Expr::MethodCallExpr(m) => {
                let receiver = self.lower_opt(m.receiver(), range);
                let (name, name_range) = m.name_ref().map_or((Name::new("_"), range), |n| {
                    (Name::new(&n.text()), n.syntax().text_range())
                });
                let args = self.lower_args(m.arg_list(), m.trailing_block());
                Expr::MethodCall {
                    receiver,
                    name,
                    name_range,
                    args,
                }
            }
            ast::Expr::FieldExpr(f) => {
                let base = self.lower_opt(f.receiver(), range);
                if let Some(index) = f.index() {
                    let value = index
                        .text()
                        .replace('_', "")
                        .parse::<u32>()
                        .unwrap_or(u32::MAX);
                    Expr::TupleField { base, index: value }
                } else {
                    let (name, name_range) = f.name_ref().map_or((Name::new("_"), range), |n| {
                        (Name::new(&n.text()), n.syntax().text_range())
                    });
                    Expr::Field {
                        base,
                        name,
                        name_range,
                    }
                }
            }
            ast::Expr::IndexExpr(i) => {
                let base = self.lower_opt(i.base(), range);
                let index_exprs: Vec<ast::Expr> = i.indices().collect();
                let type_args = index_exprs
                    .iter()
                    .map(expr_as_type)
                    .collect::<Option<Vec<_>>>();
                let indices = index_exprs.iter().map(|e| self.lower_expr(e)).collect();
                Expr::Index {
                    base,
                    indices,
                    type_args,
                }
            }
            ast::Expr::TryExpr(t) => Expr::Try(self.lower_opt(t.expr(), range)),
            ast::Expr::AssignStmt(_) => Expr::Missing,
        };
        self.alloc_expr(expr, range)
    }

    fn lower_if(&mut self, i: &ast::IfExpr) -> ExprId {
        let range = i.syntax().text_range();
        let cond = self.lower_opt(i.condition(), range);
        let then_branch = self.lower_opt_block(i.then_branch(), range);
        let mut branches: Vec<(ExprId, ExprId, TextRange)> = Vec::new();
        for elsif in i.elsif_branches() {
            let r = elsif.syntax().text_range();
            let c = self.lower_opt(elsif.condition(), r);
            let b = self.lower_opt_block(elsif.body(), r);
            branches.push((c, b, r));
        }
        let mut else_branch = i.else_branch().map(|e| {
            let r = e.syntax().text_range();
            self.lower_opt_block(e.body(), r)
        });
        for (c, b, r) in branches.into_iter().rev() {
            let nested = Expr::If {
                cond: c,
                then_branch: b,
                else_branch,
            };
            let end = range.end();
            else_branch = Some(self.alloc_expr(nested, TextRange::new(r.start(), end)));
        }
        self.alloc_expr(
            Expr::If {
                cond,
                then_branch,
                else_branch,
            },
            range,
        )
    }

    fn lower_pat(&mut self, p: &ast::Pattern) -> PatId {
        self.lower_pat_with(p, BindingKind::Pattern)
    }

    fn pat_literal(l: &ast::LiteralPat) -> Option<(Literal, bool)> {
        let negative = l.is_negative();
        if let Some(s) = ast_child::<ast::StringLit>(l.syntax()) {
            let text: String = s
                .value_parts()
                .into_iter()
                .filter_map(|p| match p {
                    ast::StringValuePart::Text(t) => Some(t),
                    ast::StringValuePart::Interpolation(_) => None,
                })
                .collect();
            return Some((Literal::String(Rc::from(text.as_str())), negative));
        }
        let lit = ast_child::<ast::Literal>(l.syntax())?;
        let value = match lit.value()? {
            ast::LiteralValue::Int(v) => Literal::Int(v),
            ast::LiteralValue::Float(v) => Literal::Float(v),
            ast::LiteralValue::Char(c) => Literal::Char(c),
            ast::LiteralValue::String(s) => Literal::String(Rc::from(s.as_str())),
            ast::LiteralValue::Bytes(b) => Literal::Bytes(Rc::from(b.as_slice())),
            ast::LiteralValue::Bool(b) => Literal::Bool(b),
        };
        Some((value, negative))
    }

    #[allow(clippy::too_many_lines, reason = "one arm per pattern kind")]
    fn lower_pat_with(&mut self, p: &ast::Pattern, kind: BindingKind) -> PatId {
        let range = p.syntax().text_range();
        let pat = match p {
            ast::Pattern::WildcardPat(_) => Pat::Wild,
            ast::Pattern::RestPat(_) => Pat::Rest,
            ast::Pattern::IdentPat(i) => {
                let name = i
                    .name()
                    .map_or_else(|| Name::new("_"), |n| Name::new(&n.text()));
                if name.is_capitalized() {
                    Pat::Path(Path {
                        segments: vec![PathSegment {
                            name,
                            args: Vec::new(),
                            range,
                        }],
                        range,
                    })
                } else {
                    let r = i.name().map_or(range, |n| n.syntax().text_range());
                    Pat::Bind(self.alloc_binding(name, kind, r))
                }
            }
            ast::Pattern::LiteralPat(l) => match Self::pat_literal(l) {
                Some((lit, negative)) => Pat::Literal { lit, negative },
                None => Pat::Missing,
            },
            ast::Pattern::PathPat(path) => match path.path() {
                Some(path) => Pat::Path(lower_path(&path)),
                None => Pat::Missing,
            },
            ast::Pattern::TupleStructPat(t) => {
                let path = t.path().map_or(
                    Path {
                        segments: Vec::new(),
                        range,
                    },
                    |p| lower_path(&p),
                );
                let mut rest = false;
                let mut positional = Vec::new();
                let mut fields = Vec::new();
                for child in t.syntax().children() {
                    if let Some(field) = ast::FieldPat::cast(child.clone()) {
                        let r = field.syntax().text_range();
                        let (name, name_range) = field.name().map_or((Name::new("_"), r), |n| {
                            (Name::new(&n.text()), n.syntax().text_range())
                        });
                        let sub = if let Some(sub) = field.pattern() {
                            self.lower_pat_with(&sub, kind)
                        } else {
                            let b = self.alloc_binding(name.clone(), kind, name_range);
                            self.alloc_pat(Pat::Bind(b), name_range)
                        };
                        fields.push((name, name_range, sub));
                    } else if child.kind() == SyntaxKind::REST_PAT {
                        rest = true;
                    } else if let Some(sub) = ast::Pattern::cast(child) {
                        positional.push(self.lower_pat_with(&sub, kind));
                    }
                }
                if fields.is_empty() {
                    Pat::TupleStruct {
                        path,
                        pats: positional,
                        rest,
                    }
                } else {
                    Pat::Struct { path, fields, rest }
                }
            }
            ast::Pattern::TuplePat(t) => {
                let elements = t
                    .elements()
                    .map(|e| self.lower_pat_with(&e, kind))
                    .collect();
                Pat::Tuple(elements)
            }
            ast::Pattern::ParenPat(p) => {
                return match p.pattern() {
                    Some(inner) => self.lower_pat_with(&inner, kind),
                    None => self.alloc_pat(Pat::Missing, range),
                };
            }
            ast::Pattern::RangePat(r) => {
                let bounds: Vec<ast::LiteralPat> = r.bounds().collect();
                let inclusive = r
                    .operator()
                    .is_some_and(|t| t.kind() == SyntaxKind::DOT2_EQ);
                match (
                    bounds.first().and_then(Self::pat_literal),
                    bounds.get(1).and_then(Self::pat_literal),
                ) {
                    (Some(lo), Some(hi)) => Pat::Range { lo, hi, inclusive },
                    _ => Pat::Missing,
                }
            }
            ast::Pattern::OrPat(o) => {
                let alternatives = o
                    .alternatives()
                    .map(|a| self.lower_pat_with(&a, kind))
                    .collect();
                Pat::Or(alternatives)
            }
        };
        self.alloc_pat(pat, range)
    }
}
