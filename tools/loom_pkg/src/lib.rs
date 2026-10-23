//! Package management: loom.toml, loom.lock and minimal version selection.
//!
//! Status: stub (Phase 0). See `docs/roadmap.md` for when this crate is implemented.

/// The name of this crate, used by the driver for diagnostics about the pipeline.
pub const CRATE_NAME: &str = "loom_pkg";

#[cfg(test)]
mod tests {
    use super::CRATE_NAME;

    #[test]
    fn crate_name_matches_package() {
        assert_eq!(CRATE_NAME, env!("CARGO_PKG_NAME"));
    }
}
