//! Name resolution (ADR 0019 §2, spec §4.1, §7.2, §9.1, §12).
//!
//! [`resolve`] builds the [`DefMap`] (the two namespaces of every module, with imports
//! resolved to a fixed point) and then resolves every body: each name expression, each
//! module path prefix, each path pattern, and each `x = e` statement (a new binding or an
//! assignment). Paths that continue into a type (associated items, variants) or a value
//! (fields, methods) are left for `loom_check`. Types written in signatures and bodies are
//! resolved on demand through [`DefMap::resolve_type_path`].

mod body;
mod defmap;

pub use body::{BodyResolution, LocalRes};
pub use defmap::{DefMap, Namespace, Res, TyParamId};

use loom_diagnostics::Diagnostic;
use loom_hir::{BodyId, Hir};
use std::collections::HashMap;

/// The result of name resolution.
#[derive(Debug)]
pub struct Resolution {
    /// Module scopes and item lookup.
    pub def_map: DefMap,
    /// Per-body results.
    pub bodies: HashMap<BodyId, BodyResolution>,
    /// Errors found.
    pub diagnostics: Vec<Diagnostic>,
}

impl Resolution {
    /// The resolution of `body`.
    ///
    /// # Panics
    ///
    /// If `body` was not part of the resolved program.
    #[must_use]
    pub fn body(&self, body: BodyId) -> &BodyResolution {
        &self.bodies[&body]
    }
}

/// Resolves every module and body of `hir`.
#[must_use]
pub fn resolve(hir: &Hir) -> Resolution {
    let mut diagnostics = Vec::new();
    let def_map = DefMap::build(hir, &mut diagnostics);
    let mut bodies = HashMap::new();
    for (index, body) in hir.bodies.iter().enumerate() {
        let id = BodyId(u32::try_from(index).unwrap_or(u32::MAX));
        let resolution = body::resolve_body(hir, &def_map, body, &mut diagnostics);
        bodies.insert(id, resolution);
    }
    Resolution {
        def_map,
        bodies,
        diagnostics,
    }
}
