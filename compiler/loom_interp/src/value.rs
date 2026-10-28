//! Runtime values and places (ADR 0019 §8 rules 2–3).
//!
//! Scalars are stored inline. Everything else is reference-counted with copy-on-write:
//! copying a value copies a pointer, and the first write through a shared pointer copies
//! the data. A valid program never observes two owners or a mutable alias, so this is
//! indistinguishable from Rust's value semantics.

use loom_hir::{BodyId, DefId, ExprId};
use loom_types::{FloatTy, IntTy, Subst, Ty};
use std::cell::RefCell;
use std::rc::Rc;

/// A runtime value.
#[derive(Clone, Debug)]
pub enum Value {
    /// `()`.
    Unit,
    /// `Bool`.
    Bool(bool),
    /// An integer of the given type. `U128` values are stored as their bit pattern.
    Int(i128, IntTy),
    /// A float of the given type.
    Float(f64, FloatTy),
    /// `Char`.
    Char(char),
    /// `String`.
    Str(Rc<String>),
    /// `Bytes`.
    Bytes(Rc<Vec<u8>>),
    /// A tuple.
    Tuple(Rc<Vec<Value>>),
    /// A struct.
    Struct(Rc<StructValue>),
    /// An enum value.
    Enum(Rc<EnumValue>),
    /// `Array[T]`.
    Array(Rc<Vec<Value>>),
    /// `Map[K, V]`.
    Map(Rc<MapValue>),
    /// A closure.
    Closure(Rc<Closure>),
    /// A function or variant constructor used as a value.
    Function {
        /// The function or variant.
        def: DefId,
        /// Its generic arguments (concrete).
        subst: Rc<Subst>,
    },
}

/// A struct value. `ty` is its concrete type, used for dynamic dispatch.
#[derive(Clone, Debug)]
pub struct StructValue {
    /// The concrete type.
    pub ty: Ty,
    /// Field values, in declaration order.
    pub fields: Vec<Value>,
}

/// An enum value.
#[derive(Clone, Debug)]
pub struct EnumValue {
    /// The concrete type.
    pub ty: Ty,
    /// The variant.
    pub variant: DefId,
    /// The variant's position.
    pub index: u32,
    /// Payload values.
    pub fields: Vec<Value>,
}

/// An insertion-ordered hash map.
#[derive(Clone, Debug, Default)]
pub struct MapValue {
    /// Entries in insertion order.
    pub entries: Vec<(Value, Value)>,
    /// Entry positions by key hash.
    pub index: std::collections::HashMap<u64, Vec<usize>>,
}

/// A closure: its code and the binding cells it captured.
#[derive(Debug)]
pub struct Closure {
    /// The body containing the closure expression.
    pub body: BodyId,
    /// The closure expression.
    pub expr: ExprId,
    /// The enclosing frame's binding slots when the closure was created.
    pub slots: Vec<Option<Place>>,
    /// The enclosing method's `self`.
    pub self_place: Option<Place>,
    /// The enclosing frame's concrete generic arguments.
    pub subst: Rc<Subst>,
}

/// One step from a value to a part of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Projection {
    /// A struct field or tuple element, or an enum payload field.
    Field(usize),
    /// An array element.
    Index(usize),
    /// The value of a map entry, by position.
    MapValue(usize),
}

/// A place: a cell and a path into its value.
#[derive(Clone, Debug)]
pub struct Place {
    /// The cell holding the root value.
    pub cell: Rc<RefCell<Value>>,
    /// The path from the root value to the place.
    pub path: Vec<Projection>,
}

impl Place {
    /// A new place holding `value`.
    #[must_use]
    pub fn new(value: Value) -> Self {
        Self {
            cell: Rc::new(RefCell::new(value)),
            path: Vec::new(),
        }
    }

    /// The place one projection further.
    #[must_use]
    pub fn project(&self, step: Projection) -> Self {
        let mut path = self.path.clone();
        path.push(step);
        Self {
            cell: Rc::clone(&self.cell),
            path,
        }
    }

    /// Reads the value at the place.
    #[must_use]
    pub fn read(&self) -> Value {
        let root = self.cell.borrow();
        let mut current: &Value = &root;
        for step in &self.path {
            current = match project_ref(current, step) {
                Some(v) => v,
                None => return Value::Unit,
            };
        }
        current.clone()
    }

    /// Applies `f` to the value at the place, copying shared data on the way.
    pub fn modify<R>(&self, f: impl FnOnce(&mut Value) -> R) -> R {
        let mut root = self.cell.borrow_mut();
        let mut current: &mut Value = &mut root;
        for step in &self.path {
            current = project_mut(current, step);
        }
        f(current)
    }

    /// Writes `value` to the place.
    pub fn write(&self, value: Value) {
        self.modify(|slot| *slot = value);
    }
}

fn project_ref<'v>(value: &'v Value, step: &Projection) -> Option<&'v Value> {
    match (value, step) {
        (Value::Struct(s), Projection::Field(i)) => s.fields.get(*i),
        (Value::Enum(e), Projection::Field(i)) => e.fields.get(*i),
        (Value::Tuple(t), Projection::Field(i)) => t.get(*i),
        (Value::Array(a), Projection::Index(i)) => a.get(*i),
        (Value::Map(m), Projection::MapValue(i)) => m.entries.get(*i).map(|(_, v)| v),
        _ => None,
    }
}

fn project_mut<'v>(value: &'v mut Value, step: &Projection) -> &'v mut Value {
    match (value, step) {
        (Value::Struct(s), Projection::Field(i)) => &mut Rc::make_mut(s).fields[*i],
        (Value::Enum(e), Projection::Field(i)) => &mut Rc::make_mut(e).fields[*i],
        (Value::Tuple(t), Projection::Field(i)) => &mut Rc::make_mut(t)[*i],
        (Value::Array(a), Projection::Index(i)) => &mut Rc::make_mut(a)[*i],
        (Value::Map(m), Projection::MapValue(i)) => &mut Rc::make_mut(m).entries[*i].1,
        (other, _) => other,
    }
}

impl Value {
    /// A string value.
    #[must_use]
    pub fn string(text: impl Into<String>) -> Self {
        Self::Str(Rc::new(text.into()))
    }

    /// `Int`.
    #[must_use]
    pub fn int(value: i128) -> Self {
        Self::Int(value, IntTy::I64)
    }

    /// The integer of an `Int` value.
    #[must_use]
    pub fn as_int(&self) -> i128 {
        match self {
            Self::Int(v, _) => *v,
            _ => 0,
        }
    }

    /// The text of a `String` value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Str(s) => s,
            _ => "",
        }
    }

    /// The boolean of a `Bool` value.
    #[must_use]
    pub fn as_bool(&self) -> bool {
        matches!(self, Self::Bool(true))
    }
}
