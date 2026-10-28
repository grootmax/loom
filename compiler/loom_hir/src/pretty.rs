//! A readable dump of a body, for `loom check --dump=hir` and snapshot tests. The format
//! is unstable.

use crate::{Arg, Body, Expr, ExprId, InterpolationPart, Literal, Pat, PatId, Stmt};
use std::fmt::Write;

/// Renders `body` as an indented tree.
#[must_use]
pub fn dump_body(body: &Body) -> String {
    let mut out = String::new();
    for (i, &param) in body.params.iter().enumerate() {
        let binding = body.binding(param);
        let _ = write!(out, "param {} ({:?})", binding.name, binding.kind);
        if let Some(Some(default)) = body.defaults.get(i) {
            out.push_str(" = ");
            expr(body, *default, 1, &mut out);
        }
        out.push('\n');
    }
    if let Some(root) = body.root {
        expr(body, root, 0, &mut out);
        out.push('\n');
    }
    out
}

fn indent(depth: usize, out: &mut String) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn literal(lit: &Literal) -> String {
    match lit {
        Literal::Int(v) => v.to_string(),
        Literal::Float(v) => format!("{v:?}"),
        Literal::Char(c) => format!("{c:?}"),
        Literal::String(s) => format!("{s:?}"),
        Literal::Bytes(b) => format!("b{b:?}"),
        Literal::Bool(b) => b.to_string(),
    }
}

fn args(body: &Body, args: &[Arg], depth: usize, out: &mut String) {
    for arg in args {
        out.push('\n');
        indent(depth, out);
        if let Some((name, _)) = &arg.name {
            let _ = write!(out, "{name}: ");
        }
        if arg.trailing {
            out.push_str("trailing ");
        }
        expr(body, arg.value, depth, out);
    }
}

#[allow(clippy::too_many_lines, reason = "one arm per expression kind")]
fn expr(body: &Body, id: ExprId, depth: usize, out: &mut String) {
    match body.expr(id) {
        Expr::Missing => out.push_str("<missing>"),
        Expr::Literal(lit) => out.push_str(&literal(lit)),
        Expr::Interpolate(parts) => {
            out.push_str("interpolate");
            for part in parts {
                out.push('\n');
                indent(depth + 1, out);
                match part {
                    InterpolationPart::Text(t) => {
                        let _ = write!(out, "{t:?}");
                    }
                    InterpolationPart::Expr(e) => expr(body, *e, depth + 1, out),
                }
            }
        }
        Expr::Name(name) => out.push_str(name.as_str()),
        Expr::SelfValue => out.push_str("self"),
        Expr::SelfType => out.push_str("Self"),
        Expr::Field { base, name, .. } => {
            expr(body, *base, depth, out);
            let _ = write!(out, ".{name}");
        }
        Expr::TupleField { base, index } => {
            expr(body, *base, depth, out);
            let _ = write!(out, ".{index}");
        }
        Expr::Call { callee, args: a } => {
            out.push_str("call ");
            expr(body, *callee, depth, out);
            args(body, a, depth + 1, out);
        }
        Expr::MethodCall {
            receiver,
            name,
            args: a,
            ..
        } => {
            let _ = write!(out, "method {name} on ");
            expr(body, *receiver, depth, out);
            args(body, a, depth + 1, out);
        }
        Expr::Index { base, indices, .. } => {
            out.push_str("index ");
            expr(body, *base, depth, out);
            for i in indices {
                out.push('\n');
                indent(depth + 1, out);
                expr(body, *i, depth + 1, out);
            }
        }
        Expr::Unary { op, operand } => {
            let _ = write!(out, "{op:?} ");
            expr(body, *operand, depth, out);
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            let _ = write!(out, "({} ", op.symbol());
            expr(body, *lhs, depth, out);
            out.push(' ');
            expr(body, *rhs, depth, out);
            out.push(')');
        }
        Expr::Range {
            start,
            end,
            inclusive,
        } => {
            expr(body, *start, depth, out);
            out.push_str(if *inclusive { "..=" } else { ".." });
            expr(body, *end, depth, out);
        }
        Expr::In { expr: e, pat: p } => {
            expr(body, *e, depth, out);
            out.push_str(" in ");
            pat(body, *p, out);
        }
        Expr::Try(e) => {
            expr(body, *e, depth, out);
            out.push('?');
        }
        Expr::Tuple(elements) | Expr::Array(elements) => {
            out.push_str(if matches!(body.expr(id), Expr::Tuple(_)) {
                "tuple"
            } else {
                "array"
            });
            for e in elements {
                out.push('\n');
                indent(depth + 1, out);
                expr(body, *e, depth + 1, out);
            }
        }
        Expr::Map(entries) => {
            out.push_str("map");
            for (k, v) in entries {
                out.push('\n');
                indent(depth + 1, out);
                expr(body, *k, depth + 1, out);
                out.push_str(" => ");
                expr(body, *v, depth + 1, out);
            }
        }
        Expr::Closure { params, body: b } => {
            out.push_str("closure |");
            if let Some(params) = params {
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    pat(body, p.pat, out);
                }
            } else {
                out.push_str("<none>");
            }
            out.push_str("| ");
            expr(body, *b, depth, out);
        }
        Expr::Block { stmts } => {
            out.push_str("block");
            for stmt in stmts {
                out.push('\n');
                indent(depth + 1, out);
                match stmt {
                    Stmt::Bind { binding, value, .. } => {
                        let b = body.binding(*binding);
                        let _ = write!(out, "bind {} ({:?}) = ", b.name, b.kind);
                        expr(body, *value, depth + 1, out);
                    }
                    Stmt::Assign {
                        place, op, value, ..
                    } => {
                        out.push_str("assign ");
                        expr(body, *place, depth + 1, out);
                        let _ = write!(out, " {}= ", op.map_or("", |o| o.symbol()));
                        expr(body, *value, depth + 1, out);
                    }
                    Stmt::Expr(e) => expr(body, *e, depth + 1, out),
                }
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            out.push_str("if ");
            expr(body, *cond, depth, out);
            out.push('\n');
            indent(depth + 1, out);
            expr(body, *then_branch, depth + 1, out);
            if let Some(e) = else_branch {
                out.push('\n');
                indent(depth, out);
                out.push_str("else ");
                expr(body, *e, depth, out);
            }
        }
        Expr::Case {
            scrutinee,
            arms,
            else_branch,
        } => {
            out.push_str("case ");
            expr(body, *scrutinee, depth, out);
            for arm in arms {
                out.push('\n');
                indent(depth + 1, out);
                out.push_str("in ");
                pat(body, arm.pat, out);
                if let Some(g) = arm.guard {
                    out.push_str(" if ");
                    expr(body, g, depth + 1, out);
                }
                out.push_str(" => ");
                expr(body, arm.body, depth + 1, out);
            }
            if let Some(e) = else_branch {
                out.push('\n');
                indent(depth + 1, out);
                out.push_str("else ");
                expr(body, *e, depth + 1, out);
            }
        }
        Expr::Loop(b) => {
            out.push_str("loop ");
            expr(body, *b, depth, out);
        }
        Expr::While { cond, body: b } => {
            out.push_str("while ");
            expr(body, *cond, depth, out);
            out.push(' ');
            expr(body, *b, depth, out);
        }
        Expr::For {
            pat: p,
            iter,
            body: b,
        } => {
            out.push_str("for ");
            pat(body, *p, out);
            out.push_str(" in ");
            expr(body, *iter, depth, out);
            out.push(' ');
            expr(body, *b, depth, out);
        }
        Expr::Return(e) | Expr::Break(e) => {
            out.push_str(if matches!(body.expr(id), Expr::Return(_)) {
                "return"
            } else {
                "break"
            });
            if let Some(e) = e {
                out.push(' ');
                expr(body, *e, depth, out);
            }
        }
        Expr::Next => out.push_str("next"),
        Expr::Unsupported(what) => {
            let _ = write!(out, "<unsupported {what}>");
        }
    }
}

fn pat(body: &Body, id: PatId, out: &mut String) {
    match body.pat(id) {
        Pat::Missing => out.push_str("<missing>"),
        Pat::Wild => out.push('_'),
        Pat::Rest => out.push_str(".."),
        Pat::Bind(b) => out.push_str(body.binding(*b).name.as_str()),
        Pat::Literal { lit, negative } => {
            if *negative {
                out.push('-');
            }
            out.push_str(&literal(lit));
        }
        Pat::Range { lo, hi, inclusive } => {
            let _ = write!(
                out,
                "{}{}{}{}{}",
                if lo.1 { "-" } else { "" },
                literal(&lo.0),
                if *inclusive { "..=" } else { ".." },
                if hi.1 { "-" } else { "" },
                literal(&hi.0)
            );
        }
        Pat::Tuple(elements) => {
            out.push('(');
            for (i, e) in elements.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pat(body, *e, out);
            }
            out.push(')');
        }
        Pat::Path(path) => out.push_str(&path.text()),
        Pat::TupleStruct { path, pats, rest } => {
            let _ = write!(out, "{}(", path.text());
            for (i, e) in pats.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                pat(body, *e, out);
            }
            if *rest {
                out.push_str(if pats.is_empty() { ".." } else { ", .." });
            }
            out.push(')');
        }
        Pat::Struct { path, fields, rest } => {
            let _ = write!(out, "{}(", path.text());
            for (i, (name, _, p)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{name}: ");
                pat(body, *p, out);
            }
            if *rest {
                out.push_str(", ..");
            }
            out.push(')');
        }
        Pat::Or(alternatives) => {
            for (i, a) in alternatives.iter().enumerate() {
                if i > 0 {
                    out.push_str(" | ");
                }
                pat(body, *a, out);
            }
        }
    }
}
