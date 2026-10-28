//! The textual form of MIR, for `loom check --dump=mir` (unstable).

use crate::{
    AggregateKind, Callee, Constant, MirBody, Operand, Place, Proj, Rvalue, StatementKind,
    TerminatorKind,
};
use loom_check::Items;
use loom_check::display::ty_to_string;
use loom_hir::Hir;
use loom_types::Interner;
use std::fmt::Write;

/// Renders `body`.
#[must_use]
pub fn dump_body(hir: &Hir, items: &Items, interner: &Interner, body: &MirBody) -> String {
    let ty = |t| ty_to_string(hir, items, interner, t);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "mir {} ({:?}) {{",
        hir.path_string(body.owner),
        body.source
    );
    for (i, local) in body.locals.iter().enumerate() {
        let _ = writeln!(
            out,
            "    let {}_{}: {}; // {:?}",
            if local.mutable { "mut " } else { "" },
            i,
            ty(local.ty),
            local.kind
        );
    }
    if !body.captures.is_empty() {
        let _ = writeln!(
            out,
            "    // captures (escaping: {}): {:?}",
            body.escaping,
            body.captures
                .iter()
                .map(|c| (c.binding.0, c.by_ref))
                .collect::<Vec<_>>()
        );
    }
    for (b, block) in body.blocks.iter().enumerate() {
        let _ = writeln!(out, "    bb{b}: {{");
        for stmt in &block.statements {
            let text = match &stmt.kind {
                StatementKind::Assign(p, rv) => format!("{} = {}", place(p), rvalue(hir, rv)),
                StatementKind::StorageLive(l) => format!("StorageLive({l})"),
                StatementKind::StorageDead(l) => format!("StorageDead({l})"),
            };
            let _ = writeln!(out, "        {text};");
        }
        if let Some(t) = &block.terminator {
            let text = match &t.kind {
                TerminatorKind::Goto(b) => format!("goto -> {b}"),
                TerminatorKind::SwitchInt {
                    discr,
                    targets,
                    otherwise,
                } => format!(
                    "switchInt({}) -> [{}otherwise: {otherwise}]",
                    operand(hir, discr),
                    targets.iter().fold(String::new(), |mut acc, (v, b)| {
                        let _ = write!(acc, "{v}: {b}, ");
                        acc
                    })
                ),
                TerminatorKind::Call {
                    callee,
                    args,
                    dest,
                    target,
                } => format!(
                    "{} = {}({}) -> {}",
                    place(dest),
                    callee_text(hir, callee),
                    args.iter()
                        .map(|a| operand(hir, a))
                        .collect::<Vec<_>>()
                        .join(", "),
                    target.map_or_else(|| "!".to_owned(), |b| b.to_string())
                ),
                TerminatorKind::Drop { place: p, target } => {
                    format!("drop({}) -> {target}", place(p))
                }
                TerminatorKind::Return => "return".to_owned(),
                TerminatorKind::Panic(m) => format!("panic({m:?})"),
                TerminatorKind::Unreachable => "unreachable".to_owned(),
            };
            let _ = writeln!(out, "        {text};");
        }
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    out
}

fn place(p: &Place) -> String {
    let mut s = p.local.to_string();
    for step in &p.proj {
        s = match step {
            Proj::Deref => format!("(*{s})"),
            Proj::Field(i) => format!("{s}.{i}"),
            Proj::Downcast(v) => format!("({s} as v{})", v.0),
            Proj::Index(l) => format!("{s}[{l}]"),
        };
    }
    s
}

fn operand(hir: &Hir, op: &Operand) -> String {
    match op {
        Operand::Copy(p) => place(p),
        Operand::Move(p) => format!("move {}", place(p)),
        Operand::Const(c, _) => constant(hir, c),
    }
}

fn constant(hir: &Hir, c: &Constant) -> String {
    match c {
        Constant::Int(v, _) => v.to_string(),
        Constant::Float(v, _) => format!("{v:?}"),
        Constant::Bool(b) => b.to_string(),
        Constant::Char(c) => format!("{c:?}"),
        Constant::Str(s) => format!("{s:?}"),
        Constant::Bytes(b) => format!("b{b:?}"),
        Constant::Unit => "()".to_owned(),
        Constant::Fn { def, .. } | Constant::Item(def) => hir.path_string(*def),
    }
}

fn rvalue(hir: &Hir, rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(op) => operand(hir, op),
        Rvalue::Ref { mutable, place: p } => {
            format!("&{}{}", if *mutable { "mut " } else { "" }, place(p))
        }
        Rvalue::Binary(op, a, b) => {
            format!("{}({}, {})", op.symbol(), operand(hir, a), operand(hir, b))
        }
        Rvalue::Unary(op, a) => format!("{op:?}({})", operand(hir, a)),
        Rvalue::Aggregate(kind, ops) => {
            let name = match kind {
                AggregateKind::Tuple => String::new(),
                AggregateKind::Struct(d, _) | AggregateKind::Variant(d, _) => hir.path_string(*d),
                AggregateKind::Array(_) => "array".to_owned(),
                AggregateKind::Map(..) => "map".to_owned(),
                AggregateKind::Closure(source) => format!("closure{source:?}"),
            };
            format!(
                "{name}({})",
                ops.iter()
                    .map(|o| operand(hir, o))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Rvalue::Discriminant(p) => format!("discriminant({})", place(p)),
        Rvalue::Unsize(op, _) => format!("unsize({})", operand(hir, op)),
    }
}

fn callee_text(hir: &Hir, callee: &Callee) -> String {
    match callee {
        Callee::Fn { def, .. } => hir.path_string(*def),
        Callee::Trait { method, .. } => format!("<trait>{}", hir.path_string(*method)),
        Callee::Value(op) => format!("(*{})", operand(hir, op)),
        Callee::Builtin(name) => format!("builtin::{name}"),
        Callee::Default { def, index, .. } => format!("{}::default#{index}", hir.path_string(*def)),
    }
}
