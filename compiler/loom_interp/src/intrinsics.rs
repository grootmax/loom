//! The intrinsics of the core library (ADR 0019 §4 rule 2), listed in
//! `docs/internals/intrinsics.md`.

use crate::eval::ArgVal;
use crate::ops::{int_fits, text, wrap};
use crate::value::{MapValue, Projection};
use crate::{Eval, Flow, Interp, Place, Value};
use loom_hir::DefId;
use loom_resolve::TyParamId;
use loom_span::Span;
use loom_types::{FloatTy, IntTy, Subst, TyKind};
use std::rc::Rc;

impl Interp<'_> {
    fn type_arg(&self, def: DefId, subst: &Subst, index: u32) -> TyKind {
        subst
            .get(TyParamId { owner: def, index })
            .map_or(TyKind::Error, |t| self.interner().kind(t))
    }

    #[allow(clippy::too_many_lines, reason = "one arm per intrinsic")]
    #[allow(
        clippy::needless_pass_by_value,
        reason = "the value is moved into the callee in most callers"
    )]
    #[allow(
        clippy::match_same_arms,
        reason = "arms listed per case for readability"
    )]
    pub(crate) fn intrinsic(
        &self,
        def: DefId,
        name: &str,
        subst: &Subst,
        args: Vec<ArgVal>,
        span: Option<Span>,
    ) -> Eval<Value> {
        let arg = |i: usize| args.get(i).map_or(Value::Unit, ArgVal::value);
        let place = |i: usize| match args.get(i) {
            Some(ArgVal::Place(p)) => p.clone(),
            Some(ArgVal::Value(v)) => Place::new(v.clone()),
            None => Place::new(Value::Unit),
        };
        let panic = |message: String| Err(Flow::Panic(message, span));
        let int_ty = || match self.type_arg(def, subst, 0) {
            TyKind::Int(t) => t,
            _ => IntTy::I64,
        };
        let float_ty = || match self.type_arg(def, subst, 0) {
            TyKind::Float(t) => t,
            _ => FloatTy::F64,
        };
        match name {
            // ----- output and panics -----
            "write_stdout" => {
                self.write_out(arg(0).as_str());
                Ok(Value::Unit)
            }
            "panic_with" => panic(arg(0).as_str().to_owned()),
            "debug_string" => Ok(text(self.debug_text(&arg(0)))),
            "value_clone" => Ok(arg(0)),
            // ----- integers -----
            "int_to_string" => Ok(text(crate::ops::primitive_text(&arg(0)))),
            "int_parse" => {
                let t = int_ty();
                let s = arg(0);
                let s = s.as_str();
                let parsed = if t == IntTy::U128 {
                    s.parse::<u128>().ok().map(u128::cast_signed)
                } else {
                    s.parse::<i128>().ok().filter(|&v| int_fits(v, t))
                };
                Ok(match parsed {
                    Some(v) => self.lang_variant(
                        "Option",
                        "Some",
                        vec![self.interner().intern(TyKind::Int(t))],
                        vec![Value::Int(v, t)],
                    ),
                    None => self.lang_variant(
                        "Option",
                        "None",
                        vec![self.interner().intern(TyKind::Int(t))],
                        Vec::new(),
                    ),
                })
            }
            "int_abs" => match arg(0) {
                Value::Int(v, t) => {
                    self.int_result(v.checked_abs(), t, "take the absolute value", span)
                }
                other => Ok(other),
            },
            "int_pow" => match (arg(0), arg(1)) {
                (Value::Int(b, t), Value::Int(e, _)) => {
                    let e = u32::try_from(e).unwrap_or(u32::MAX);
                    self.int_result(b.checked_pow(e), t, "raise to a power", span)
                }
                (other, _) => Ok(other),
            },
            "int_checked" | "int_wrapping" | "int_saturating" => {
                let (Value::Int(a, t), Value::Int(b, _)) = (arg(0), arg(1)) else {
                    return Ok(Value::Unit);
                };
                let op = arg(2);
                let raw = match op.as_str() {
                    "add" => a.checked_add(b),
                    "sub" => a.checked_sub(b),
                    "mul" => a.checked_mul(b),
                    _ => {
                        if b == 0 {
                            None
                        } else {
                            a.checked_div(b)
                        }
                    }
                };
                let ty = self.interner().intern(TyKind::Int(t));
                match name {
                    "int_checked" => Ok(match raw.filter(|&v| int_fits(v, t)) {
                        Some(v) => {
                            self.lang_variant("Option", "Some", vec![ty], vec![Value::Int(v, t)])
                        }
                        None => self.lang_variant("Option", "None", vec![ty], Vec::new()),
                    }),
                    "int_wrapping" => {
                        let wide = match op.as_str() {
                            "add" => a.wrapping_add(b),
                            "sub" => a.wrapping_sub(b),
                            _ => a.wrapping_mul(b),
                        };
                        Ok(Value::Int(wrap(wide, t), t))
                    }
                    _ => {
                        let v = match raw {
                            Some(v) if int_fits(v, t) => v,
                            _ => {
                                let negative = match op.as_str() {
                                    "add" => (a < 0) && (b < 0),
                                    "sub" => b > 0,
                                    _ => (a < 0) != (b < 0),
                                };
                                if negative {
                                    t.min()
                                } else {
                                    i128::try_from(t.max()).unwrap_or(i128::MAX)
                                }
                            }
                        };
                        Ok(Value::Int(v, t))
                    }
                }
            }
            "int_to_float" => match arg(0) {
                #[allow(clippy::cast_precision_loss, reason = "documented rounding")]
                Value::Int(v, _) => Ok(Value::Float(v as f64, FloatTy::F64)),
                _ => Ok(Value::Float(0.0, FloatTy::F64)),
            },
            "int_min" => {
                let t = int_ty();
                Ok(Value::Int(t.min(), t))
            }
            "int_max" => {
                let t = int_ty();
                Ok(Value::Int(t.max().cast_signed(), t))
            }
            "int_zero" => Ok(Value::Int(0, int_ty())),
            "int_add" => {
                let (a, b) = (arg(0), arg(1));
                self.arith(loom_hir::BinaryOp::Add, &a, &b, span)
            }
            // ----- floats -----
            "float_to_string" => Ok(text(crate::ops::primitive_text(&arg(0)))),
            "float_parse" => {
                let t = float_ty();
                let ty = self.interner().intern(TyKind::Float(t));
                Ok(match arg(0).as_str().trim().parse::<f64>() {
                    Ok(v) => {
                        self.lang_variant("Option", "Some", vec![ty], vec![Value::Float(v, t)])
                    }
                    Err(_) => self.lang_variant("Option", "None", vec![ty], Vec::new()),
                })
            }
            "float_math" => {
                let Value::Float(v, t) = arg(0) else {
                    return Ok(arg(0));
                };
                let r = match arg(1).as_str() {
                    "sqrt" => v.sqrt(),
                    "abs" => v.abs(),
                    "floor" => v.floor(),
                    "ceil" => v.ceil(),
                    _ => v,
                };
                Ok(Value::Float(r, t))
            }
            "float_round_to" => {
                let (Value::Float(v, t), digits) = (arg(0), arg(1).as_int()) else {
                    return Ok(arg(0));
                };
                let factor = 10f64.powi(i32::try_from(digits).unwrap_or(0));
                Ok(Value::Float((v * factor).round() / factor, t))
            }
            "float_truncate" => {
                let Value::Float(v, _) = arg(0) else {
                    return Ok(Value::int(0));
                };
                #[allow(clippy::cast_precision_loss, reason = "range check")]
                if v.is_nan() || v.trunc() < i64::MIN as f64 || v.trunc() >= i64::MAX as f64 {
                    return panic(format!("{v} cannot be converted to `Int`"));
                }
                #[allow(clippy::cast_possible_truncation, reason = "checked above")]
                Ok(Value::int(i128::from(v.trunc() as i64)))
            }
            // ----- characters -----
            "char_is" => {
                let Value::Char(c) = arg(0) else {
                    return Ok(Value::Bool(false));
                };
                Ok(Value::Bool(match arg(1).as_str() {
                    "alphabetic" => c.is_alphabetic(),
                    "alphanumeric" => c.is_alphanumeric(),
                    "digit" => c.is_ascii_digit(),
                    "whitespace" => c.is_whitespace(),
                    "uppercase" => c.is_uppercase(),
                    "lowercase" => c.is_lowercase(),
                    _ => false,
                }))
            }
            "char_convert" => {
                let Value::Char(c) = arg(0) else {
                    return Ok(arg(0));
                };
                let converted = match arg(1).as_str() {
                    "upper" => single(c.to_uppercase()).unwrap_or(c),
                    _ => single(c.to_lowercase()).unwrap_or(c),
                };
                Ok(Value::Char(converted))
            }
            "char_code" => match arg(0) {
                Value::Char(c) => Ok(Value::Int(i128::from(u32::from(c)), IntTy::U32)),
                _ => Ok(Value::Int(0, IntTy::U32)),
            },
            "char_to_string" => match arg(0) {
                Value::Char(c) => Ok(text(c.to_string())),
                _ => Ok(text(String::new())),
            },
            // ----- strings -----
            "string_length" => Ok(Value::int(
                i128::try_from(arg(0).as_str().chars().count()).unwrap_or(0),
            )),
            "string_byte_length" => Ok(Value::int(
                i128::try_from(arg(0).as_str().len()).unwrap_or(0),
            )),
            "string_chars" => Ok(Value::Array(Rc::new(
                arg(0).as_str().chars().map(Value::Char).collect(),
            ))),
            "string_from_chars" => match arg(0) {
                Value::Array(items) => Ok(text(
                    items
                        .iter()
                        .filter_map(|v| {
                            if let Value::Char(c) = v {
                                Some(*c)
                            } else {
                                None
                            }
                        })
                        .collect(),
                )),
                _ => Ok(text(String::new())),
            },
            "string_push_str" => {
                let other = arg(1);
                place(0).modify(|v| {
                    if let Value::Str(s) = v {
                        Rc::make_mut(s).push_str(other.as_str());
                    }
                });
                Ok(Value::Unit)
            }
            "string_push_char" => {
                let c = arg(1);
                place(0).modify(|v| {
                    if let (Value::Str(s), Value::Char(c)) = (v, &c) {
                        Rc::make_mut(s).push(*c);
                    }
                });
                Ok(Value::Unit)
            }
            "string_convert" => {
                let s = arg(0);
                let s = s.as_str();
                Ok(text(match arg(1).as_str() {
                    "upper" => s.to_uppercase(),
                    "lower" => s.to_lowercase(),
                    "trim" => s.trim().to_owned(),
                    "trim_start" => s.trim_start().to_owned(),
                    "trim_end" => s.trim_end().to_owned(),
                    _ => s.to_owned(),
                }))
            }
            "string_split_whitespace" => Ok(Value::Array(Rc::new(
                arg(0)
                    .as_str()
                    .split_whitespace()
                    .map(|w| text(w.to_owned()))
                    .collect(),
            ))),
            "string_split" => {
                let (s, sep) = (arg(0), arg(1));
                let parts: Vec<Value> = if sep.as_str().is_empty() {
                    s.as_str().chars().map(|c| text(c.to_string())).collect()
                } else {
                    s.as_str()
                        .split(sep.as_str())
                        .map(|w| text(w.to_owned()))
                        .collect()
                };
                Ok(Value::Array(Rc::new(parts)))
            }
            "string_find" => {
                let (s, pattern) = (arg(0), arg(1));
                let found = s
                    .as_str()
                    .find(pattern.as_str())
                    .map(|byte| s.as_str()[..byte].chars().count());
                let int = self.interner().int();
                Ok(match found {
                    Some(i) => self.lang_variant(
                        "Option",
                        "Some",
                        vec![int],
                        vec![Value::int(i128::try_from(i).unwrap_or(0))],
                    ),
                    None => self.lang_variant("Option", "None", vec![int], Vec::new()),
                })
            }
            "string_substring" => {
                let s = arg(0);
                let chars: Vec<char> = s.as_str().chars().collect();
                let (start, stop) = (arg(1).as_int(), arg(2).as_int());
                let length = i128::try_from(chars.len()).unwrap_or(0);
                if start < 0 || stop < start || stop > length {
                    return panic(format!(
                        "substring {start}..{stop} is out of bounds for a string of length {length}"
                    ));
                }
                let (start, stop) = (
                    usize::try_from(start).unwrap_or(0),
                    usize::try_from(stop).unwrap_or(0),
                );
                Ok(text(chars[start..stop].iter().collect()))
            }
            "string_repeat" => {
                let count = usize::try_from(arg(1).as_int()).unwrap_or(0);
                Ok(text(arg(0).as_str().repeat(count)))
            }
            // ----- arrays -----
            "array_new" => Ok(Value::Array(Rc::new(Vec::new()))),
            "array_length" => match arg(0) {
                Value::Array(items) => Ok(Value::int(i128::try_from(items.len()).unwrap_or(0))),
                _ => Ok(Value::int(0)),
            },
            "array_get" => match arg(0) {
                Value::Array(items) => {
                    let i = self.check_index(&arg(1), items.len(), span)?;
                    Ok(items[i].clone())
                }
                _ => Ok(Value::Unit),
            },
            "array_set" => {
                let target = place(0);
                let length = array_len(&target);
                let i = self.check_index(&arg(1), length, span)?;
                target.project(Projection::Index(i)).write(arg(2));
                Ok(Value::Unit)
            }
            "array_push" => {
                let value = arg(1);
                place(0).modify(|v| {
                    if let Value::Array(items) = v {
                        Rc::make_mut(items).push(value);
                    }
                });
                Ok(Value::Unit)
            }
            "array_pop" => {
                let mut popped = None;
                place(0).modify(|v| {
                    if let Value::Array(items) = v {
                        popped = Rc::make_mut(items).pop();
                    }
                });
                let elem = self.type_arg(def, subst, 0);
                let elem = self.interner().intern(elem);
                Ok(match popped {
                    Some(v) => self.lang_variant("Option", "Some", vec![elem], vec![v]),
                    None => self.lang_variant("Option", "None", vec![elem], Vec::new()),
                })
            }
            "array_insert" => {
                let target = place(0);
                let length = array_len(&target);
                let i = arg(1).as_int();
                if i < 0 || i > i128::try_from(length).unwrap_or(i128::MAX) {
                    return panic(format!(
                        "insertion index {i} is out of bounds for length {length}"
                    ));
                }
                let value = arg(2);
                target.modify(|v| {
                    if let Value::Array(items) = v {
                        Rc::make_mut(items).insert(usize::try_from(i).unwrap_or(0), value);
                    }
                });
                Ok(Value::Unit)
            }
            "array_remove" => {
                let target = place(0);
                let i = self.check_index(&arg(1), array_len(&target), span)?;
                let mut removed = Value::Unit;
                target.modify(|v| {
                    if let Value::Array(items) = v {
                        removed = Rc::make_mut(items).remove(i);
                    }
                });
                Ok(removed)
            }
            "array_slice" => match arg(0) {
                Value::Array(items) => {
                    let start = usize::try_from(arg(1).as_int())
                        .unwrap_or(0)
                        .min(items.len());
                    let stop = usize::try_from(arg(2).as_int())
                        .unwrap_or(0)
                        .min(items.len())
                        .max(start);
                    Ok(Value::Array(Rc::new(items[start..stop].to_vec())))
                }
                _ => Ok(Value::Array(Rc::new(Vec::new()))),
            },
            "array_reverse" => {
                place(0).modify(|v| {
                    if let Value::Array(items) = v {
                        Rc::make_mut(items).reverse();
                    }
                });
                Ok(Value::Unit)
            }
            "array_sort_by" => {
                let target = place(0);
                let compare = arg(1);
                let Value::Array(items) = target.read() else {
                    return Ok(Value::Unit);
                };
                let sorted = self.merge_sort(items.to_vec(), &compare, span)?;
                target.write(Value::Array(Rc::new(sorted)));
                Ok(Value::Unit)
            }
            // ----- maps -----
            "map_new" => Ok(Value::Map(Rc::new(MapValue::default()))),
            "map_length" => match arg(0) {
                Value::Map(m) => Ok(Value::int(i128::try_from(m.entries.len()).unwrap_or(0))),
                _ => Ok(Value::int(0)),
            },
            "map_insert" | "map_remove" => {
                let target = place(0);
                let (key, value) = (arg(1), arg(2));
                let mut result = Ok(None);
                target.modify(|v| {
                    if let Value::Map(m) = v {
                        let m = Rc::make_mut(m);
                        result = if name == "map_insert" {
                            self.map_insert(m, key, value)
                        } else {
                            self.map_remove(m, &key)
                        };
                    }
                });
                let previous = result?;
                let value_ty = self.interner().intern(self.type_arg(def, subst, 1));
                Ok(match previous {
                    Some(v) => self.lang_variant("Option", "Some", vec![value_ty], vec![v]),
                    None => self.lang_variant("Option", "None", vec![value_ty], Vec::new()),
                })
            }
            "map_get" => {
                let value_ty = self.interner().intern(self.type_arg(def, subst, 1));
                let Value::Map(m) = arg(0) else {
                    return Ok(Value::Unit);
                };
                Ok(match self.map_find(&m, &arg(1))? {
                    Some(i) => self.lang_variant(
                        "Option",
                        "Some",
                        vec![value_ty],
                        vec![m.entries[i].1.clone()],
                    ),
                    None => self.lang_variant("Option", "None", vec![value_ty], Vec::new()),
                })
            }
            "map_entry_at" => {
                let Value::Map(m) = arg(0) else {
                    return Ok(Value::Unit);
                };
                let i = self.check_index(&arg(1), m.entries.len(), span)?;
                let (k, v) = m.entries[i].clone();
                Ok(Value::Tuple(Rc::new(vec![k, v])))
            }
            "map_update" => {
                let target = place(0);
                let (key, default, f) = (arg(1), arg(2), arg(3));
                let mut position = Ok(None);
                target.modify(|v| {
                    if let Value::Map(m) = v {
                        let m = Rc::make_mut(m);
                        position = match self.map_find(m, &key) {
                            Ok(Some(i)) => Ok(Some(i)),
                            Ok(None) => self
                                .map_insert(m, key, default)
                                .map(|_| Some(m.entries.len() - 1)),
                            Err(e) => Err(e),
                        };
                    }
                });
                let Some(i) = position? else {
                    return Ok(Value::Unit);
                };
                let entry = target.project(Projection::MapValue(i));
                self.call_value(&f, vec![ArgVal::Place(entry)], span)?;
                Ok(Value::Unit)
            }
            // ----- ranges -----
            "range_next" => {
                let target = place(0);
                let Value::Struct(range) = target.read() else {
                    return Ok(Value::Unit);
                };
                let (start, end, inclusive) = (
                    range.fields[0].clone(),
                    range.fields[1].clone(),
                    range.fields[2].as_bool(),
                );
                let elem_ty = self.value_ty(&start);
                let has_next = match self.value_cmp(&start, &end)? {
                    std::cmp::Ordering::Less => true,
                    std::cmp::Ordering::Equal => inclusive,
                    std::cmp::Ordering::Greater => false,
                };
                if !has_next {
                    return Ok(self.lang_variant("Option", "None", vec![elem_ty], Vec::new()));
                }
                let next = match &start {
                    Value::Int(v, t) => {
                        let n = v.checked_add(1).filter(|&n| int_fits(n, *t));
                        n.map(|n| Value::Int(n, *t))
                    }
                    Value::Char(c) => char::from_u32(u32::from(*c) + 1)
                        .or_else(|| char::from_u32(0xE000))
                        .map(Value::Char),
                    _ => None,
                };
                let mut fields = range.fields.clone();
                match next {
                    Some(n) if self.value_cmp(&start, &end)? == std::cmp::Ordering::Less => {
                        fields[0] = n;
                    }
                    _ => fields[2] = Value::Bool(false),
                }
                target.write(Value::Struct(Rc::new(crate::value::StructValue {
                    ty: range.ty,
                    fields,
                })));
                Ok(self.lang_variant("Option", "Some", vec![elem_ty], vec![start]))
            }
            name if crate::tasks::is_task_intrinsic(name) => {
                self.task_intrinsic(def, name, subst, &args, span)
            }
            other => panic(format!("internal error: unknown intrinsic `{other}`")),
        }
    }

    /// A stable merge sort calling the Loom comparison `compare`.
    fn merge_sort(
        &self,
        items: Vec<Value>,
        compare: &Value,
        span: Option<Span>,
    ) -> Eval<Vec<Value>> {
        if items.len() <= 1 {
            return Ok(items);
        }
        let mut items = items;
        let right = items.split_off(items.len() / 2);
        let left = self.merge_sort(items, compare, span)?;
        let right = self.merge_sort(right, compare, span)?;
        let mut out = Vec::with_capacity(left.len() + right.len());
        let (mut i, mut j) = (0, 0);
        while i < left.len() && j < right.len() {
            let ordering = self.call_value(
                compare,
                vec![
                    ArgVal::Value(right[j].clone()),
                    ArgVal::Value(left[i].clone()),
                ],
                span,
            )?;
            // Take from the right only if it is strictly less, so equal elements keep order.
            let right_less = matches!(&ordering, Value::Enum(e) if e.index == 0);
            if right_less {
                out.push(right[j].clone());
                j += 1;
            } else {
                out.push(left[i].clone());
                i += 1;
            }
        }
        out.extend_from_slice(&left[i..]);
        out.extend_from_slice(&right[j..]);
        Ok(out)
    }
}

fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

fn array_len(place: &Place) -> usize {
    match place.read() {
        Value::Array(items) => items.len(),
        _ => 0,
    }
}
