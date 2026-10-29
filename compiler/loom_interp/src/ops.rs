//! Arithmetic, equality, ordering, hashing and text of values.

use crate::eval::ArgVal;
use crate::value::MapValue;
use crate::{Eval, Flow, Interp, Value};
use loom_check::{ImplKind, Selection, Solver, TraitRef};
use loom_hir::{BinaryOp, Literal};
use loom_span::Span;
use loom_types::{InferTable, IntTy, TyKind};
use std::cmp::Ordering;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

/// Wraps `value` to the range of `ty` (two's complement).
pub(crate) fn wrap(value: i128, ty: IntTy) -> i128 {
    let bits = ty.bits();
    if bits == 128 {
        return value;
    }
    let mask = (1i128 << bits) - 1;
    let low = value & mask;
    if ty.is_signed() && low >> (bits - 1) == 1 {
        low - (1i128 << bits)
    } else {
        low
    }
}

pub(crate) fn primitive_text(value: &Value) -> String {
    match value {
        Value::Int(v, IntTy::U128) => v.cast_unsigned().to_string(),
        Value::Int(v, _) => v.to_string(),
        Value::Float(v, loom_types::FloatTy::F32) => {
            #[allow(clippy::cast_possible_truncation, reason = "the value is an F32")]
            let f = *v as f32;
            float_text_32(f)
        }
        Value::Float(v, _) => float_text(*v),
        Value::Bool(b) => b.to_string(),
        Value::Char(c) => c.to_string(),
        Value::Unit => "()".to_owned(),
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

/// Spec §7.12 rule 2: shortest round-trip digits; positional between 1e-4 and 1e16.
pub(crate) fn float_text(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_owned();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    format!("{v:?}")
}

fn float_text_32(v: f32) -> String {
    if v.is_nan() {
        return "NaN".to_owned();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    format!("{v:?}")
}

#[allow(clippy::float_cmp, reason = "IEEE equality is the specified semantics")]
pub(crate) fn literal_matches(value: &Value, lit: &Literal, negative: bool) -> bool {
    match (value, lit) {
        (Value::Int(v, _), Literal::Int(l)) => {
            let l = i128::try_from(*l).unwrap_or(i128::MAX);
            *v == if negative { -l } else { l }
        }
        (Value::Float(v, _), Literal::Float(l)) => *v == if negative { -l } else { *l },
        (Value::Char(c), Literal::Char(l)) => c == l,
        (Value::Str(s), Literal::String(l)) => s.as_str() == &**l,
        (Value::Bool(b), Literal::Bool(l)) => b == l,
        (Value::Bytes(b), Literal::Bytes(l)) => b.as_slice() == &**l,
        _ => false,
    }
}

fn literal_key(lit: &Literal, negative: bool) -> Option<i128> {
    match lit {
        Literal::Int(v) => {
            let v = i128::try_from(*v).ok()?;
            Some(if negative { -v } else { v })
        }
        Literal::Char(c) => Some(i128::from(u32::from(*c))),
        _ => None,
    }
}

pub(crate) fn range_matches(
    value: &Value,
    lo: &(Literal, bool),
    hi: &(Literal, bool),
    inclusive: bool,
) -> bool {
    let v = match value {
        Value::Int(v, _) => *v,
        Value::Char(c) => i128::from(u32::from(*c)),
        _ => return false,
    };
    let (Some(lo), Some(hi)) = (literal_key(&lo.0, lo.1), literal_key(&hi.0, hi.1)) else {
        return false;
    };
    v >= lo && if inclusive { v <= hi } else { v < hi }
}

impl Interp<'_> {
    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    pub(crate) fn int_result(
        &self,
        value: Option<i128>,
        ty: IntTy,
        what: &str,
        span: Option<Span>,
    ) -> Eval<Value> {
        match value {
            Some(v) if fits(v, ty) => Ok(Value::Int(v, ty)),
            _ => Err(Flow::Panic(
                format!("attempt to {what} with overflow"),
                span,
            )),
        }
    }

    /// Integer, float and boolean operators (spec §7.5 rules 1–2).
    #[allow(
        clippy::many_single_char_names,
        reason = "short operand names in arithmetic"
    )]
    pub(crate) fn arith(
        &self,
        op: BinaryOp,
        a: &Value,
        b: &Value,
        span: Option<Span>,
    ) -> Eval<Value> {
        match (a, b) {
            (Value::Int(x, t), Value::Int(y, _)) => {
                let (x, y, t) = (*x, *y, *t);
                if t == IntTy::U128 {
                    return self.arith_u128(op, x.cast_unsigned(), y.cast_unsigned(), span);
                }
                match op {
                    BinaryOp::Add => self.int_result(x.checked_add(y), t, "add", span),
                    BinaryOp::Sub => self.int_result(x.checked_sub(y), t, "subtract", span),
                    BinaryOp::Mul => self.int_result(x.checked_mul(y), t, "multiply", span),
                    BinaryOp::Div | BinaryOp::Rem => {
                        if y == 0 {
                            let what = if op == BinaryOp::Div {
                                "divide"
                            } else {
                                "calculate the remainder"
                            };
                            return Err(Flow::Panic(format!("attempt to {what} by zero"), span));
                        }
                        let r = if op == BinaryOp::Div {
                            x.checked_div(y)
                        } else {
                            x.checked_rem(y)
                        };
                        self.int_result(
                            r,
                            t,
                            if op == BinaryOp::Div {
                                "divide"
                            } else {
                                "calculate the remainder"
                            },
                            span,
                        )
                    }
                    BinaryOp::BitAnd => Ok(Value::Int(x & y, t)),
                    BinaryOp::BitOr => Ok(Value::Int(x | y, t)),
                    BinaryOp::BitXor => Ok(Value::Int(x ^ y, t)),
                    BinaryOp::Shl | BinaryOp::Shr => {
                        if y < 0 || y >= i128::from(t.bits()) {
                            return Err(Flow::Panic(
                                format!("shift amount {y} is out of range for `{}`", t.name()),
                                span,
                            ));
                        }
                        let shift = u32::try_from(y).unwrap_or(0);
                        let r = if op == BinaryOp::Shl {
                            wrap(x << shift, t)
                        } else {
                            x >> shift
                        };
                        Ok(Value::Int(r, t))
                    }
                    _ => Err(Flow::Panic(
                        "internal error: bad integer operator".to_owned(),
                        span,
                    )),
                }
            }
            (Value::Float(x, t), Value::Float(y, _)) => {
                let r = match op {
                    BinaryOp::Add => x + y,
                    BinaryOp::Sub => x - y,
                    BinaryOp::Mul => x * y,
                    BinaryOp::Div => x / y,
                    BinaryOp::Rem => x % y,
                    _ => {
                        return Err(Flow::Panic(
                            "internal error: bad float operator".to_owned(),
                            span,
                        ));
                    }
                };
                let r = if *t == loom_types::FloatTy::F32 {
                    #[allow(clippy::cast_possible_truncation, reason = "F32 arithmetic")]
                    let f = r as f32;
                    f64::from(f)
                } else {
                    r
                };
                Ok(Value::Float(r, *t))
            }
            (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(match op {
                BinaryOp::BitAnd => *x && *y,
                BinaryOp::BitOr => *x || *y,
                _ => *x ^ *y,
            })),
            _ => Err(Flow::Panic("internal error: bad operands".to_owned(), span)),
        }
    }

    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    fn arith_u128(&self, op: BinaryOp, x: u128, y: u128, span: Option<Span>) -> Eval<Value> {
        let overflow = |what: &str| Flow::Panic(format!("attempt to {what} with overflow"), span);
        let r = match op {
            BinaryOp::Add => x.checked_add(y).ok_or_else(|| overflow("add"))?,
            BinaryOp::Sub => x.checked_sub(y).ok_or_else(|| overflow("subtract"))?,
            BinaryOp::Mul => x.checked_mul(y).ok_or_else(|| overflow("multiply"))?,
            BinaryOp::Div => x
                .checked_div(y)
                .ok_or_else(|| Flow::Panic("attempt to divide by zero".to_owned(), span))?,
            BinaryOp::Rem => x.checked_rem(y).ok_or_else(|| {
                Flow::Panic(
                    "attempt to calculate the remainder by zero".to_owned(),
                    span,
                )
            })?,
            BinaryOp::BitAnd => x & y,
            BinaryOp::BitOr => x | y,
            BinaryOp::BitXor => x ^ y,
            BinaryOp::Shl | BinaryOp::Shr => {
                if y >= 128 {
                    return Err(Flow::Panic(
                        format!("shift amount {y} is out of range for `U128`"),
                        span,
                    ));
                }
                let s = u32::try_from(y).unwrap_or(0);
                if op == BinaryOp::Shl { x << s } else { x >> s }
            }
            _ => {
                return Err(Flow::Panic(
                    "internal error: bad integer operator".to_owned(),
                    span,
                ));
            }
        };
        Ok(Value::Int(r.cast_signed(), IntTy::U128))
    }

    /// The user's `Eq` implementation for a struct or enum value, if it has one.
    fn user_impl(&self, value: &Value, trait_lang: &str) -> Option<()> {
        let ty = self.value_ty(value);
        let trait_ = self.items().lang.get(trait_lang)?;
        let mut table = InferTable::new();
        let selection = Solver {
            hir: self.hir,
            items: self.items(),
            interner: self.interner(),
            table: &mut table,
        }
        .select(
            ty,
            &TraitRef {
                trait_,
                args: Vec::new(),
            },
            &loom_check::Env::default(),
        );
        match selection {
            Ok(Selection::Impl { impl_id, .. })
                if matches!(self.items().impls[impl_id].kind, ImplKind::Item(_)) =>
            {
                Some(())
            }
            _ => None,
        }
    }

    /// `a == b` (spec §7.5 rule 3).
    #[allow(
        clippy::many_single_char_names,
        reason = "short operand names in arithmetic"
    )]
    pub(crate) fn value_eq(&self, a: &Value, b: &Value) -> Eval<bool> {
        Ok(match (a, b) {
            (Value::Unit, Value::Unit) => true,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::Int(x, _), Value::Int(y, _)) => x == y,
            #[allow(clippy::float_cmp, reason = "IEEE equality is the specified semantics")]
            (Value::Float(x, _), Value::Float(y, _)) => x == y,
            (Value::Char(x), Value::Char(y)) => x == y,
            (Value::Str(x), Value::Str(y)) => x == y,
            (Value::Bytes(x), Value::Bytes(y)) => x == y,
            (Value::Tuple(x), Value::Tuple(y)) | (Value::Array(x), Value::Array(y)) => {
                if x.len() != y.len() {
                    return Ok(false);
                }
                for (p, q) in x.iter().zip(y.iter()) {
                    if !self.value_eq(p, q)? {
                        return Ok(false);
                    }
                }
                true
            }
            (Value::Struct(_) | Value::Enum(_), _) if self.user_impl(a, "Eq").is_some() => {
                let ty = self.value_ty(a);
                let result = self.call_trait_method(
                    "Eq",
                    "eq",
                    ty,
                    ArgVal::Value(a.clone()),
                    vec![Some(ArgVal::Value(b.clone()))],
                    None,
                )?;
                result.as_bool()
            }
            (Value::Struct(x), Value::Struct(y)) => {
                for (p, q) in x.fields.iter().zip(&y.fields) {
                    if !self.value_eq(p, q)? {
                        return Ok(false);
                    }
                }
                true
            }
            (Value::Enum(x), Value::Enum(y)) => {
                if x.variant != y.variant {
                    return Ok(false);
                }
                for (p, q) in x.fields.iter().zip(&y.fields) {
                    if !self.value_eq(p, q)? {
                        return Ok(false);
                    }
                }
                true
            }
            (Value::Map(x), Value::Map(y)) => {
                if x.entries.len() != y.entries.len() {
                    return Ok(false);
                }
                for (k, v) in &x.entries {
                    match self.map_find(y, k)? {
                        Some(i) => {
                            if !self.value_eq(v, &y.entries[i].1)? {
                                return Ok(false);
                            }
                        }
                        None => return Ok(false),
                    }
                }
                true
            }
            _ => false,
        })
    }

    /// The ordering of `a` and `b` (spec §7.5 rule 3; floats use IEEE 754 totalOrder).
    #[allow(
        clippy::many_single_char_names,
        reason = "short operand names in arithmetic"
    )]
    pub(crate) fn value_cmp(&self, a: &Value, b: &Value) -> Eval<Ordering> {
        Ok(match (a, b) {
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            (Value::Int(x, IntTy::U128), Value::Int(y, _)) => {
                x.cast_unsigned().cmp(&y.cast_unsigned())
            }
            (Value::Int(x, _), Value::Int(y, _)) => x.cmp(y),
            (Value::Float(x, _), Value::Float(y, _)) => x.total_cmp(y),
            (Value::Char(x), Value::Char(y)) => x.cmp(y),
            (Value::Str(x), Value::Str(y)) => x.cmp(y),
            (Value::Bytes(x), Value::Bytes(y)) => x.cmp(y),
            (Value::Tuple(x), Value::Tuple(y)) | (Value::Array(x), Value::Array(y)) => {
                for (p, q) in x.iter().zip(y.iter()) {
                    let o = self.value_cmp(p, q)?;
                    if o != Ordering::Equal {
                        return Ok(o);
                    }
                }
                x.len().cmp(&y.len())
            }
            (Value::Struct(_) | Value::Enum(_), _) if self.user_impl(a, "Ord").is_some() => {
                let ty = self.value_ty(a);
                let result = self.call_trait_method(
                    "Ord",
                    "compare",
                    ty,
                    ArgVal::Value(a.clone()),
                    vec![Some(ArgVal::Value(b.clone()))],
                    None,
                )?;
                match result {
                    Value::Enum(e) => match e.index {
                        0 => Ordering::Less,
                        1 => Ordering::Equal,
                        _ => Ordering::Greater,
                    },
                    _ => Ordering::Equal,
                }
            }
            (Value::Struct(x), Value::Struct(y)) => {
                for (p, q) in x.fields.iter().zip(&y.fields) {
                    let o = self.value_cmp(p, q)?;
                    if o != Ordering::Equal {
                        return Ok(o);
                    }
                }
                Ordering::Equal
            }
            (Value::Enum(x), Value::Enum(y)) => {
                let o = x.index.cmp(&y.index);
                if o != Ordering::Equal {
                    return Ok(o);
                }
                for (p, q) in x.fields.iter().zip(&y.fields) {
                    let o = self.value_cmp(p, q)?;
                    if o != Ordering::Equal {
                        return Ok(o);
                    }
                }
                Ordering::Equal
            }
            _ => Ordering::Equal,
        })
    }

    /// The `Ordering` enum value for `ordering`.
    pub(crate) fn ordering_value(&self, ordering: Ordering) -> Value {
        let name = match ordering {
            Ordering::Less => "Less",
            Ordering::Equal => "Equal",
            Ordering::Greater => "Greater",
        };
        self.lang_variant("Ordering", name, Vec::new(), Vec::new())
    }

    /// A hash consistent with `value_eq` for the types that implement `Hash`.
    #[allow(
        clippy::self_only_used_in_recursion,
        reason = "recursive helper kept as a method"
    )]
    pub(crate) fn value_hash(&self, value: &Value, hasher: &mut DefaultHasher) {
        match value {
            Value::Unit => 0u8.hash(hasher),
            Value::Bool(b) => b.hash(hasher),
            Value::Int(v, _) => v.hash(hasher),
            Value::Float(v, _) => v.to_bits().hash(hasher),
            Value::Char(c) => c.hash(hasher),
            Value::Str(s) => s.hash(hasher),
            Value::Bytes(b) => b.hash(hasher),
            Value::Tuple(items) | Value::Array(items) => {
                items.len().hash(hasher);
                for item in items.iter() {
                    self.value_hash(item, hasher);
                }
            }
            Value::Struct(s) => {
                for f in &s.fields {
                    self.value_hash(f, hasher);
                }
            }
            Value::Enum(e) => {
                e.index.hash(hasher);
                for f in &e.fields {
                    self.value_hash(f, hasher);
                }
            }
            Value::Map(m) => m.entries.len().hash(hasher),
            Value::Closure(_) | Value::Function { .. } => 1u8.hash(hasher),
            Value::Handle(h) => std::ptr::from_ref::<crate::value::Handle>(h).hash(hasher),
        }
    }

    fn hash_of(&self, value: &Value) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.value_hash(value, &mut hasher);
        hasher.finish()
    }

    /// The position of `key` in `map`.
    pub(crate) fn map_find(&self, map: &MapValue, key: &Value) -> Eval<Option<usize>> {
        let hash = self.hash_of(key);
        if let Some(positions) = map.index.get(&hash) {
            for &i in positions {
                if self.value_eq(&map.entries[i].0, key)? {
                    return Ok(Some(i));
                }
            }
        }
        Ok(None)
    }

    /// Inserts or replaces; returns the previous value.
    pub(crate) fn map_insert(
        &self,
        map: &mut MapValue,
        key: Value,
        value: Value,
    ) -> Eval<Option<Value>> {
        if let Some(i) = self.map_find(map, &key)? {
            return Ok(Some(std::mem::replace(&mut map.entries[i].1, value)));
        }
        let hash = self.hash_of(&key);
        map.index.entry(hash).or_default().push(map.entries.len());
        map.entries.push((key, value));
        Ok(None)
    }

    /// Removes `key`; returns its value.
    pub(crate) fn map_remove(&self, map: &mut MapValue, key: &Value) -> Eval<Option<Value>> {
        let Some(i) = self.map_find(map, key)? else {
            return Ok(None);
        };
        let (_, value) = map.entries.remove(i);
        map.index.clear();
        for (position, (k, _)) in map.entries.iter().enumerate() {
            let hash = self.hash_of(k);
            map.index.entry(hash).or_default().push(position);
        }
        Ok(Some(value))
    }

    /// A structural rendering for `assert_eq` messages.
    pub(crate) fn debug_text(&self, value: &Value) -> String {
        match value {
            Value::Str(s) => format!("{s:?}"),
            Value::Char(c) => format!("{c:?}"),
            Value::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|v| self.debug_text(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Array(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|v| self.debug_text(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Map(m) => format!(
                "{{{}}}",
                m.entries
                    .iter()
                    .map(|(k, v)| format!("{} => {}", self.debug_text(k), self.debug_text(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Struct(s) => {
                let TyKind::Adt(def, _) = self.interner().kind(s.ty) else {
                    return "?".to_owned();
                };
                let names = self.items().fields.get(&def).cloned().unwrap_or_default();
                let fields: Vec<String> = s
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        format!(
                            "{}: {}",
                            names.get(i).map_or("?".to_owned(), |f| f.name.to_string()),
                            self.debug_text(v)
                        )
                    })
                    .collect();
                format!("{}({})", self.hir.def(def).name, fields.join(", "))
            }
            Value::Enum(e) => {
                let name = self.hir.def(e.variant).name.to_string();
                if e.fields.is_empty() {
                    name
                } else {
                    format!(
                        "{name}({})",
                        e.fields
                            .iter()
                            .map(|v| self.debug_text(v))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Value::Closure(_) | Value::Function { .. } => "<function>".to_owned(),
            Value::Handle(h) => format!("<{}>", h.lang()),
            other => primitive_text(other),
        }
    }
}

fn fits(value: i128, ty: IntTy) -> bool {
    match ty {
        IntTy::I128 => true,
        IntTy::U128 => value >= 0,
        _ => value >= ty.min() && value <= i128::try_from(ty.max()).unwrap_or(i128::MAX),
    }
}

/// Whether `value` fits in `ty`.
pub(crate) fn int_fits(value: i128, ty: IntTy) -> bool {
    fits(value, ty)
}

/// A string value from text (helper for intrinsics).
pub(crate) fn text(s: String) -> Value {
    Value::Str(Rc::new(s))
}
