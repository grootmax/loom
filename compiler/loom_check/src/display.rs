//! Types as users write them, for diagnostics.

use crate::items::{Items, fn_kind_name};
use loom_hir::Hir;
use loom_types::{Interner, SELF_INDEX, Ty, TyKind, VarKind};

/// Renders `ty` in Loom syntax (`Array[Int]`, `(Int, String)`, `Fn(Int) -> Bool`).
#[must_use]
#[allow(
    clippy::only_used_in_recursion,
    reason = "context threaded through recursion"
)]
pub fn ty_to_string(hir: &Hir, items: &Items, interner: &Interner, ty: Ty) -> String {
    let show = |t: Ty| ty_to_string(hir, items, interner, t);
    let list = |ts: &[Ty]| ts.iter().map(|&t| show(t)).collect::<Vec<_>>().join(", ");
    match interner.kind(ty) {
        TyKind::Int(i) => i.name().to_owned(),
        TyKind::Float(f) => f.name().to_owned(),
        TyKind::Bool => "Bool".to_owned(),
        TyKind::Char => "Char".to_owned(),
        TyKind::String => "String".to_owned(),
        TyKind::Bytes => "Bytes".to_owned(),
        TyKind::Unit => "()".to_owned(),
        TyKind::Never => "Never".to_owned(),
        TyKind::Tuple(elements) => format!("({})", list(&elements)),
        TyKind::Adt(def, args) => {
            let name = hir.def(def).name.to_string();
            if args.is_empty() {
                name
            } else {
                format!("{name}[{}]", list(&args))
            }
        }
        TyKind::Param(param) => {
            if param.index == SELF_INDEX {
                "Self".to_owned()
            } else {
                hir.own_generics(param.owner)
                    .and_then(|g| g.params.get(param.index as usize))
                    .map_or_else(|| "?".to_owned(), |p| p.name.to_string())
            }
        }
        TyKind::Projection { args, assoc, .. } => {
            let base = args.first().map_or_else(|| "?".to_owned(), |&t| show(t));
            format!("{base}.{}", hir.def(assoc).name)
        }
        TyKind::Callable { kind, params, ret } => {
            let ret_text = if matches!(interner.kind(ret), TyKind::Unit) {
                String::new()
            } else {
                format!(" -> {}", show(ret))
            };
            format!("{}({}){ret_text}", fn_kind_name(kind), list(&params))
        }
        TyKind::Dyn(def, args) => {
            let name = hir.def(def).name.to_string();
            if args.is_empty() {
                format!("dyn {name}")
            } else {
                format!("dyn {name}[{}]", list(&args))
            }
        }
        TyKind::Ref { mutable, inner } => {
            format!("{}ref {}", if mutable { "mut " } else { "" }, show(inner))
        }
        TyKind::Infer(_, VarKind::Int) => "{integer}".to_owned(),
        TyKind::Infer(_, VarKind::Float) => "{float}".to_owned(),
        TyKind::Infer(_, VarKind::General) => "_".to_owned(),
        TyKind::Error => "{error}".to_owned(),
    }
}
